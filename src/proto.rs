//! Wire framing, the pairing proof, and safe path handling for received files.

use crate::config::TransportPolicy;
use anyhow::{Result, bail};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::path::{Component, Path, PathBuf};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_HEADER: usize = 256 * 1024;
pub const MAX_FILES: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// Relative path using '/' separators. The receiver re-validates every segment.
    pub path: String,
    pub len: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Msg {
    Pair { proof: String, name: String, addrs: Vec<String> },
    Ping,
    DeviceInventory,
    Text { len: u64 },
    Image { len: u64 },
    Files { files: Vec<FileEntry> },
    /// Latency probe: `n` 8-byte frames are echoed back one at a time.
    Echo { n: u32 },
    /// Throughput test: the receiver discards `up` bytes, then sends `down` bytes back.
    Bench { up: u64, down: u64 },
    /// Link a Moonlight client to its host. `sig` is the Moonlight key's signature (hex) over
    /// `moonlight::link_message(host_fp, client_fp)`.
    MoonlightLink { moonlight_cert: String, sig: String, name: String, addrs: Vec<String> },
    /// One file for an app channel such as Crew Link. It never touches the clipboard.
    Channel { channel: String, name: String, len: u64, sha256: String },
    /// A paired device asks this machine's owner for clipboard and/or file access.
    /// It never grants anything by itself; the owner approves or denies locally.
    AccessRequest { capabilities: Vec<String> },
    /// Both peers prove their local transport consent before any content is sent.
    VerifyTransport { transport: String, policy: TransportPolicy },
}

pub const MAX_ECHO: u32 = 200;
pub const MAX_BENCH: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub addrs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<TransportPolicy>,
    /// A successful transport check authorizes the next frame on this TLS connection only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport_session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_inventory: Option<crate::devices::Inventory>,
}

impl Reply {
    pub fn ok() -> Self {
        Reply { ok: true, ..Default::default() }
    }
    pub fn err(e: impl Into<String>) -> Self {
        Reply { ok: false, error: Some(e.into()), ..Default::default() }
    }
}

pub async fn write_json<W: AsyncWrite + Unpin, T: Serialize>(w: &mut W, v: &T) -> Result<()> {
    let b = serde_json::to_vec(v)?;
    if b.len() > MAX_HEADER {
        bail!("header too large");
    }
    w.write_all(&(b.len() as u32).to_be_bytes()).await?;
    w.write_all(&b).await?;
    Ok(())
}

pub async fn read_json<R: AsyncRead + Unpin, T: for<'de> Deserialize<'de>>(r: &mut R) -> Result<T> {
    let mut l = [0u8; 4];
    r.read_exact(&mut l).await?;
    let n = u32::from_be_bytes(l) as usize;
    if n > MAX_HEADER {
        bail!("header too large");
    }
    let mut b = vec![0u8; n];
    r.read_exact(&mut b).await?;
    Ok(serde_json::from_slice(&b)?)
}

/// Pairing code: 80 random bits as 16 Crockford-style base32 characters, shown as XXXX-XXXX-XXXX-XXXX.
pub fn new_pair_code() -> String {
    let mut b = [0u8; 10];
    rand::fill(&mut b);
    let s = data_encoding::BASE32_NOPAD.encode(&b);
    s.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).unwrap()).collect::<Vec<_>>().join("-")
}

pub fn normalize_code(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

/// HMAC over both certificate fingerprints. A relay that substitutes its own certificate
/// changes one of the fingerprints, so it cannot produce or forward a valid proof.
pub fn pair_proof(code: &str, role: &str, server_fp: &str, client_fp: &str) -> String {
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(normalize_code(code).as_bytes()).expect("hmac key");
    m.update(b"remote-access-crew/pair/v1/");
    m.update(role.as_bytes());
    m.update(b"/");
    m.update(server_fp.as_bytes());
    m.update(b"/");
    m.update(client_fp.as_bytes());
    hex::encode(m.finalize().into_bytes())
}

pub fn proof_matches(expected: &str, got: &str) -> bool {
    // Constant-time comparison.
    let (a, b) = (expected.as_bytes(), got.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Make one path segment safe on both Windows and Linux. Returns None for segments that
/// must be rejected outright (empty, "." or "..").
pub fn safe_segment(seg: &str) -> Option<String> {
    if seg.is_empty() || seg == "." || seg == ".." {
        return None;
    }
    let mut s: String = seg
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
        .collect();
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    let s2 = s.trim_start().to_string();
    s = s2;
    if s.is_empty() {
        s = "file".into();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        s = format!("_{s}");
    }
    if s.chars().count() > 180 {
        s = s.chars().take(180).collect();
    }
    Some(s)
}

/// Validate a relative '/' path from a peer and return its safe segments.
pub fn safe_relative(path: &str) -> Result<Vec<String>> {
    if path.starts_with('/') || path.starts_with('\\') || path.contains('\\') || path.contains(':') && path.len() > 1 && path.as_bytes()[1] == b':' {
        bail!("absolute paths are not accepted");
    }
    let mut out = Vec::new();
    for seg in path.split('/') {
        match safe_segment(seg) {
            Some(s) => out.push(s),
            None => bail!("unsafe path segment"),
        }
    }
    if out.is_empty() || out.len() > 64 {
        bail!("bad path depth");
    }
    Ok(out)
}

/// Pick a name under `dir` that does not exist yet: "name", "name (2)", ...
pub fn unique_child(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(name);
    let (stem, ext) = match (p.file_stem(), p.extension()) {
        (Some(s), Some(e)) if !s.is_empty() => (s.to_string_lossy().to_string(), format!(".{}", e.to_string_lossy())),
        _ => (name.to_string(), String::new()),
    };
    for i in 2..10_000 {
        let c = dir.join(format!("{stem} ({i}){ext}"));
        if !c.exists() {
            return c;
        }
    }
    dir.join(format!("{stem} ({}){ext}", rand::random::<u32>()))
}

/// Relative '/' path for a local file inside a copied folder.
pub fn rel_path(base: &Path, p: &Path) -> Option<String> {
    let rel = p.strip_prefix(base).ok()?;
    let mut parts = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().to_string()),
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_and_absolute() {
        for p in ["../x", "a/../../x", "/etc/passwd", "C:/Windows/x", "a\\b", "", "a//b", "./a"] {
            assert!(safe_relative(p).is_err(), "{p} should be rejected");
        }
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(safe_relative("dir/re<po>rt?.txt").unwrap(), vec!["dir", "re_po_rt_.txt"]);
        assert_eq!(safe_segment("CON.txt").unwrap(), "_CON.txt");
        assert_eq!(safe_segment("evil. . ").unwrap(), "evil");
        assert_eq!(safe_segment("a\u{0}b").unwrap(), "a_b");
    }

    #[test]
    fn proof_binds_both_fingerprints() {
        let c = new_pair_code();
        assert_eq!(c.len(), 19);
        let p = pair_proof(&c, "client", "s", "c");
        assert!(proof_matches(&p, &pair_proof(&c.to_lowercase().replace('-', ""), "client", "s", "c")));
        assert!(!proof_matches(&p, &pair_proof(&c, "client", "mitm", "c")));
        assert!(!proof_matches(&p, &pair_proof(&c, "server", "s", "c")));
        assert!(!proof_matches(&p, &pair_proof("AAAA-AAAA-AAAA-AAAA", "client", "s", "c")));
    }

    /// Same vector as android/test/RacTest.java, so the phone app and the desktop agree.
    #[test]
    fn proof_known_vector() {
        assert_eq!(
            pair_proof("abcd-efgh-2345-6789", "client", &"aa".repeat(32), &"bb".repeat(32)),
            "ffd48229226b9d27dd14b831a871581a1e8227a323da0886f289ebd95047abec"
        );
    }
}
