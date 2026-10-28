//! On-disk state: identity, paired peers, settings. Nothing here holds clipboard content.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 48765;
pub const CONTROL_PORT_OFFSET: u16 = 1;

/// Local consent, bound to the authenticated peer certificate. Pairing alone
/// grants no data access. Older states fail closed until approved locally.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Permissions {
    #[serde(default)]
    pub clipboard: bool,
    #[serde(default)]
    pub files: bool,
    #[serde(default)]
    pub device_inventory: bool,
}

impl Permissions {
    pub fn allows(&self, kind: &str) -> bool {
        match kind {
            "text" | "image" => self.clipboard,
            "files" => self.files,
            "device_inventory" => self.device_inventory,
            _ => false,
        }
    }

    /// Whether one named capability ("clipboard" or "files") is granted.
    pub fn has(&self, capability: &str) -> bool {
        match capability {
            "clipboard" => self.clipboard,
            "files" => self.files,
            "device_inventory" => self.device_inventory,
            _ => false,
        }
    }

    pub fn set(&mut self, capability: &str, allowed: bool) -> Result<()> {
        match capability {
            "clipboard" => self.clipboard = allowed,
            "files" => self.files = allowed,
            "device_inventory" => self.device_inventory = allowed,
            _ => bail!("unknown capability '{capability}'"),
        }
        Ok(())
    }
}

/// Validate a requested capability list: clipboard and/or files, no repeats.
pub fn capabilities(list: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for c in list {
        if c != "clipboard" && c != "files" && c != "device_inventory" {
            bail!("unknown capability '{c}'");
        }
        if !out.contains(c) {
            out.push(c.clone());
        }
    }
    if out.is_empty() {
        bail!("ask for clipboard, files, or both");
    }
    Ok(out)
}

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
    #[serde(default)]
    pub permissions: Permissions,
}

fn yes() -> bool {
    true
}

#[cfg(test)]
mod consent_tests {
    use super::*;
    #[test]
    fn legacy_pairing_does_not_imply_consent() {
        let peer: Peer = serde_json::from_str(r#"{"name":"old","fingerprint":"abc","addrs":[],"paired_at":0}"#).unwrap();
        assert!(!peer.permissions.allows("text"));
        assert!(!peer.permissions.allows("image"));
        assert!(!peer.permissions.allows("files"));
        assert!(!Permissions { clipboard:true, files:true, ..Default::default() }.allows("commands"));
        assert!(!peer.permissions.allows("device_inventory"));
    }

    #[test]
    fn capability_lists_are_validated() {
        let ok = capabilities(&["files".into(), "clipboard".into(), "files".into()]).unwrap();
        assert_eq!(ok, vec!["files".to_string(), "clipboard".to_string()]);
        assert!(capabilities(&[]).is_err());
        assert!(capabilities(&["commands".into()]).is_err());
        let mut p = Permissions::default();
        p.set("files", true).unwrap();
        assert!(p.has("files") && !p.has("clipboard"));
        assert!(p.set("admin", true).is_err());
        p.set("device_inventory", true).unwrap();
        assert!(p.has("device_inventory"));
        assert!(!p.has("device_control"));
        p.set("device_inventory", false).unwrap();
        assert!(!p.has("device_inventory"));
    }

    #[test]
    fn legacy_grants_require_fresh_consent_once() {
        let dir = std::env::temp_dir().join(format!("rac-legacy-consent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let data = r#"{"settings":{},"peers":[
            {"name":"Miku via Moonlight","fingerprint":"abc","addrs":[],"paired_at":0,"permissions":{"clipboard":true,"files":true}},
            {"name":"Phone","fingerprint":"def","addrs":[],"paired_at":0,"permissions":{"clipboard":true,"files":true}}
        ]}"#;
        std::fs::write(dir.join("state.json"),data).unwrap();
        let mut state = load_state(&dir).unwrap();
        assert!(!state.peers[0].permissions.clipboard && !state.peers[0].permissions.files);
        assert!(!state.peers[1].permissions.clipboard && !state.peers[1].permissions.files);
        state.peers[1].permissions.clipboard = true;
        save_state(&dir,&state).unwrap();
        let reloaded = load_state(&dir).unwrap();
        assert!(!reloaded.peers[0].permissions.clipboard);
        assert!(reloaded.peers[1].permissions.clipboard);
        assert_eq!(reloaded.peers[1].fingerprint,"def");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn older_settings_allow_access_requests() {
        let s: Settings = serde_json::from_str(r#"{"port":48765}"#).unwrap();
        assert!(s.allow_access_requests);
        assert!(s.transport.allows("wifi"));
        assert!(!s.transport.allows("usb"));
        assert!(!s.transport.allows("bluetooth"));
        assert!(!s.transport.allows("unknown"));
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TransportPolicy {
    /// Ordinary WiFi, Ethernet, loopback, and Tailscale links.
    pub wifi: bool,
    /// An existing USB tether or USB accessory network link.
    pub usb: bool,
    /// An existing Bluetooth personal area network link.
    pub bluetooth: bool,
}

impl Default for TransportPolicy {
    fn default() -> Self {
        TransportPolicy { wifi: true, usb: false, bluetooth: false }
    }
}

impl TransportPolicy {
    pub fn allows(&self, transport: &str) -> bool {
        match transport {
            "wifi" => self.wifi,
            "usb" => self.usb,
            "bluetooth" => self.bluetooth,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
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
    /// Host only: Sunshine's `sunshine_state.json`. When set, clients Sunshine has paired
    /// may link with a Moonlight signature instead of a typed code.
    pub moonlight_link: Option<PathBuf>,
    /// Host-side link origin: bridge fingerprint to Sunshine certificate fingerprint.
    pub moonlight_peers: std::collections::HashMap<String, String>,
    /// Old releases did not retain reliable link provenance. Reset their grants once.
    #[serde(default)]
    pub consent_revision: u8,
    /// App channels this machine accepts (for example "crew-link"). Empty accepts none.
    pub channels: Vec<String>,
    /// Hard ceiling on one received channel file.
    pub max_channel_bytes: u64,
    /// Let paired devices ask for clipboard and file access. Off refuses every request.
    pub allow_access_requests: bool,
    /// Both peers must allow the actual link before any content is exchanged.
    pub transport: TransportPolicy,
}

/// Channel names are short lowercase words so they are safe as folder names.
pub fn valid_channel(name: &str) -> bool {
    (1..=32).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Where files received on an app channel land: `<home>/channels/<name>/inbox`.
pub fn channel_inbox(dir: &Path, channel: &str) -> Result<PathBuf> {
    if !valid_channel(channel) {
        bail!("channel names use 1-32 lowercase letters, digits or '-'");
    }
    Ok(dir.join("channels").join(channel).join("inbox"))
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
            moonlight_link: None,
            moonlight_peers: Default::default(),
            consent_revision: 1,
            channels: Vec::new(),
            max_channel_bytes: 64 * 1024 * 1024,
            allow_access_requests: true,
            transport: TransportPolicy::default(),
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
    let mut state: State = serde_json::from_slice(&raw).context("state.json is not valid; fix or remove it")?;
    // Old labels can be truncated, so they cannot identify every automatic Moonlight grant.
    // Reset all legacy grants once; retain identities and addresses for explicit local approval.
    if state.settings.consent_revision == 0 {
        for peer in &mut state.peers { peer.permissions = Permissions::default(); }
        state.settings.consent_revision = 1;
    }
    // Known old links also require a fresh certificate binding.
    for peer in &mut state.peers {
        if peer.name.ends_with(" via Moonlight") && !state.settings.moonlight_peers.contains_key(&peer.fingerprint) {
            state.settings.moonlight_peers.insert(peer.fingerprint.clone(), String::new());
            peer.permissions = Permissions::default();
        }
    }
    Ok(state)
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
