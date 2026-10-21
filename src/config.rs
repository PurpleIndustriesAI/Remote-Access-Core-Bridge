//! On-disk state: identity, paired peers, settings. Nothing here holds clipboard content.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 48765;
pub const CONTROL_PORT_OFFSET: u16 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Peer {
    pub name: String,
    /// SHA-256 of the peer's certificate (lowercase hex). This is the peer's identity.
    pub fingerprint: String,
    /// Candidate socket addresses (LAN and Tailscale), most recently successful first.
    pub addrs: Vec<String>,
    #[serde(default = "yes")]
    pub autosync: bool,
    pub paired_at: u64,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub port: u16,
    /// Sync clipboard changes to peers automatically.
    pub autosync: bool,
    /// Largest clipboard file batch sent automatically. Bigger batches need an explicit send.
    pub auto_file_limit: u64,
    /// Hard ceiling on any received file batch.
    pub max_batch_bytes: u64,
    pub max_image_bytes: u64,
    pub max_text_bytes: u64,
    pub receive_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            port: DEFAULT_PORT,
            autosync: true,
            auto_file_limit: 512 * 1024 * 1024,
            max_batch_bytes: 8 * 1024 * 1024 * 1024,
            max_image_bytes: 128 * 1024 * 1024,
            max_text_bytes: 8 * 1024 * 1024,
            receive_dir: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub peers: Vec<Peer>,
    #[serde(default)]
    pub settings: Settings,
}

pub struct Identity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    pub fingerprint: String,
}

pub fn home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("RAC_HOME") {
        return Ok(PathBuf::from(p));
    }
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    let base = base.context("cannot locate the user configuration directory")?;
    Ok(base.join(if cfg!(windows) { "RemoteAccessCrew" } else { "remote-access-crew" }))
}

pub fn ensure_home() -> Result<PathBuf> {
    let dir = home()?;
    fs::create_dir_all(&dir)?;
    restrict_dir(&dir);
    Ok(dir)
}

#[cfg(unix)]
fn restrict_dir(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o700));
}
#[cfg(not(unix))]
fn restrict_dir(_: &Path) {
    // %APPDATA% is already private to the signed-in user.
}

/// Write a private file atomically (temp file then rename).
pub fn write_private(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        #[cfg(unix)]
        let mut f = {
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?
        };
        #[cfg(not(unix))]
        let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).open(&tmp)?;
        use std::io::Write;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn fingerprint(cert_der: &[u8]) -> String {
    hex::encode(Sha256::digest(cert_der))
}

/// Short grouped form for humans to compare, e.g. "3F2A-91C0-...".
pub fn short_fingerprint(fp: &str) -> String {
    fp.chars()
        .take(24)
        .collect::<Vec<_>>()
        .chunks(4)
        .map(|c| c.iter().collect::<String>().to_uppercase())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn load_or_create_identity(dir: &Path) -> Result<Identity> {
    let cert_path = dir.join("identity.cert.der");
    let key_path = dir.join("identity.key.der");
    if cert_path.exists() && key_path.exists() {
        let cert_der = fs::read(&cert_path)?;
        let key_der = fs::read(&key_path)?;
        let fingerprint = fingerprint(&cert_der);
        return Ok(Identity { cert_der, key_der, fingerprint });
    }
    let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256)?;
    let mut params = rcgen::CertificateParams::new(vec!["rac.peer".to_string()])?;
    params.distinguished_name = rcgen::DistinguishedName::new();
    params.distinguished_name.push(rcgen::DnType::CommonName, "Remote Access Crew peer");
    params.not_before = rcgen::date_time_ymd(2025, 1, 1);
    params.not_after = rcgen::date_time_ymd(2075, 1, 1);
    let cert = params.self_signed(&key)?;
    let cert_der = cert.der().to_vec();
    let key_der = key.serialize_der();
    write_private(&key_path, &key_der)?;
    write_private(&cert_path, &cert_der)?;
    let fingerprint = fingerprint(&cert_der);
    Ok(Identity { cert_der, key_der, fingerprint })
}

pub fn load_state(dir: &Path) -> Result<State> {
    let p = dir.join("state.json");
    if !p.exists() {
        return Ok(State::default());
    }
    let raw = fs::read(&p)?;
    serde_json::from_slice(&raw).context("state.json is not valid; fix or remove it")
}

pub fn save_state(dir: &Path, state: &State) -> Result<()> {
    write_private(&dir.join("state.json"), &serde_json::to_vec_pretty(state)?)
}

/// The local control token. Only the signed-in user can read it.
pub fn control_token(dir: &Path) -> Result<String> {
    let p = dir.join("control.token");
    if let Ok(t) = fs::read_to_string(&p) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return Ok(t);
        }
    }
    let mut b = [0u8; 32];
    rand::fill(&mut b);
    let t = hex::encode(b);
    write_private(&p, t.as_bytes())?;
    Ok(t)
}

pub fn receive_dir(settings: &Settings) -> Result<PathBuf> {
    if let Some(d) = &settings.receive_dir {
        return Ok(d.clone());
    }
    #[cfg(windows)]
    let base = std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Downloads"));
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DOWNLOAD_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Downloads")));
    match base {
        Some(b) => Ok(b.join("Remote Access Crew")),
        None => bail!("cannot locate the Downloads folder; set receive_dir"),
    }
}

pub fn hostname() -> String {
    let raw = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| fs::read_to_string("/etc/hostname").ok())
        .unwrap_or_else(|| "peer".into());
    let clean: String = raw.trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(48).collect();
    if clean.is_empty() { "peer".into() } else { clean }
}
