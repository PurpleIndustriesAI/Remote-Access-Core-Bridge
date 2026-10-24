//! Moonlight link: pair a streaming client with its host without a typed code.
//!
//! Trust comes from the existing Moonlight/Sunshine pairing, in both directions:
//!
//! * The client learns the host bridge's fingerprint from Sunshine's app list, which
//!   Moonlight fetches over HTTPS pinned to the host's Sunshine certificate. The client then
//!   pins that fingerprint, so a machine pretending to be the host fails the TLS handshake.
//! * The client signs both bridge fingerprints with its Moonlight private key. The host
//!   accepts the link only when the certificate is one Sunshine has already paired (read
//!   from `sunshine_state.json`) and the signature verifies. A replayed signature is useless
//!   because it names the client's own bridge certificate, which TLS proves possession of.

use anyhow::{Context, Result, bail};
use rustls::pki_types::CertificateDer;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Prefix of the Sunshine app title that carries the host bridge fingerprint.
pub const APP_TITLE_PREFIX: &str = "RAC:";

/// Exact bytes the client signs with its Moonlight key.
pub fn link_message(server_fp: &str, client_fp: &str) -> String {
    format!("remote-access-crew/moonlight-link/v1/{server_fp}/{client_fp}")
}

/// Sunshine app title that publishes this host's bridge fingerprint.
pub fn app_title(fp: &str) -> String {
    format!("{APP_TITLE_PREFIX}{fp}")
}

/// Decode every certificate in a PEM string.
pub fn pem_certs(pem: &str) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut body: Option<String> = None;
    for line in pem.lines().map(str::trim) {
        if line == "-----BEGIN CERTIFICATE-----" {
            body = Some(String::new());
        } else if line == "-----END CERTIFICATE-----" {
            if let Some(b) = body.take() {
                if let Ok(der) = data_encoding::BASE64.decode(b.as_bytes()) {
                    out.push(der);
                }
            }
        } else if let Some(b) = body.as_mut() {
            b.push_str(line);
        }
    }
    out
}

/// A client certificate Sunshine has paired, with the device name the owner gave it.
pub struct PairedClient {
    pub name: String,
    pub cert_der: Vec<u8>,
}

/// Read the clients Sunshine has paired. Only public certificates are read.
pub fn sunshine_clients(state_path: &Path) -> Result<Vec<PairedClient>> {
    let raw = std::fs::read(state_path).with_context(|| format!("cannot read {}", state_path.display()))?;
    let v: serde_json::Value = serde_json::from_slice(&raw).context("Sunshine state is not valid JSON")?;
    let mut out = Vec::new();
    for d in v["root"]["named_devices"].as_array().into_iter().flatten() {
        let name = d["name"].as_str().unwrap_or("Moonlight client").to_string();
        for der in pem_certs(d["cert"].as_str().unwrap_or("")) {
            out.push(PairedClient { name: name.clone(), cert_der: der });
        }
    }
    Ok(out)
}

/// Verify a link signature made with the key of `cert_der`. Moonlight keys are RSA 2048;
/// ECDSA P-256 is also accepted for clients that use it.
pub fn verify_signature(cert_der: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let der = CertificateDer::from(cert_der);
    let Ok(ee) = webpki::EndEntityCert::try_from(&der) else { return false };
    let algs: [&dyn rustls::pki_types::SignatureVerificationAlgorithm; 2] =
        [webpki::ring::RSA_PKCS1_2048_8192_SHA256, webpki::ring::ECDSA_P256_SHA256];
    algs.iter().any(|a| ee.verify_signature(*a, msg, sig).is_ok())
}

/// Check a link request against Sunshine's paired clients. Returns the Sunshine device name.
pub fn check_link(state_path: &Path, moonlight_cert_pem: &str, sig_hex: &str, server_fp: &str, client_fp: &str) -> Result<String> {
    let presented = pem_certs(moonlight_cert_pem);
    let [cert] = presented.as_slice() else { bail!("send exactly one Moonlight certificate") };
    let paired = sunshine_clients(state_path)?;
    let Some(client) = paired.iter().find(|c| c.cert_der == *cert) else {
        bail!("this Moonlight client is not paired with the host's Sunshine");
    };
    let sig = hex::decode(sig_hex.trim()).context("signature is not hex")?;
    if !verify_signature(cert, link_message(server_fp, client_fp).as_bytes(), &sig) {
        bail!("the Moonlight signature does not match");
    }
    Ok(client.name.clone())
}

/// Failed link attempts are limited so an unpaired LAN device cannot hammer the check.
pub struct Limiter {
    window: Mutex<(Instant, u32)>,
}

const WINDOW: Duration = Duration::from_secs(60);
const MAX_FAILURES: u32 = 10;

impl Default for Limiter {
    fn default() -> Self {
        Limiter { window: Mutex::new((Instant::now(), 0)) }
    }
}

impl Limiter {
    pub fn allowed(&self) -> bool {
        let mut w = self.window.lock().unwrap();
        if w.0.elapsed() > WINDOW {
            *w = (Instant::now(), 0);
        }
        w.1 < MAX_FAILURES
    }
    pub fn failed(&self) {
        let mut w = self.window.lock().unwrap();
        if w.0.elapsed() > WINDOW {
            *w = (Instant::now(), 0);
        }
        w.1 += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pem_round_trip_and_message() {
        let pem = "junk\n-----BEGIN CERTIFICATE-----\nAAEC\nAw==\n-----END CERTIFICATE-----\n";
        assert_eq!(pem_certs(pem), vec![vec![0, 1, 2, 3]]);
        assert_eq!(link_message("s", "c"), "remote-access-crew/moonlight-link/v1/s/c");
        assert_eq!(app_title("ab"), "RAC:ab");
    }

    #[test]
    fn limiter_blocks_after_failures() {
        let l = Limiter::default();
        for _ in 0..MAX_FAILURES {
            assert!(l.allowed());
            l.failed();
        }
        assert!(!l.allowed());
    }
}
