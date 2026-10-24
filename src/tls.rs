//! Mutual TLS 1.3 with certificate pinning. There is no CA: a peer is trusted only if its
//! certificate fingerprint was pinned during pairing.

use crate::config::{Identity, fingerprint};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{CertificateError, DigitallySignedStruct, DistinguishedName, Error, SignatureScheme};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

pub fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Which client certificates the listener accepts.
#[derive(Default)]
pub struct Trust {
    pub peers: RwLock<HashSet<String>>,
    /// While set and in the future, unknown certificates may connect, but only to pair.
    pub pairing_until: Mutex<Option<Instant>>,
    /// Host with Moonlight link on: unknown certificates may connect, but only to present a
    /// Moonlight signature. The daemon refuses every other message from them.
    pub moonlight_link: AtomicBool,
}

impl Trust {
    pub fn is_peer(&self, fp: &str) -> bool {
        self.peers.read().unwrap().contains(fp)
    }
    pub fn pairing_open(&self) -> bool {
        matches!(*self.pairing_until.lock().unwrap(), Some(t) if t > Instant::now())
    }
    pub fn moonlight_link_open(&self) -> bool {
        self.moonlight_link.load(Ordering::Relaxed)
    }
}

fn bad_cert() -> Error {
    Error::InvalidCertificate(CertificateError::ApplicationVerificationFailure)
}

#[derive(Debug)]
struct Sigs(Arc<CryptoProvider>);

impl Sigs {
    fn v12(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn v13(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

struct PinnedClients {
    trust: Arc<Trust>,
    sigs: Sigs,
}

impl std::fmt::Debug for PinnedClients {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PinnedClients")
    }
}

impl ClientCertVerifier for PinnedClients {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(&self, end: &CertificateDer<'_>, _i: &[CertificateDer<'_>], _now: UnixTime) -> Result<ClientCertVerified, Error> {
        let fp = fingerprint(end);
        if self.trust.is_peer(&fp) || self.trust.pairing_open() || self.trust.moonlight_link_open() {
            Ok(ClientCertVerified::assertion())
        } else {
            Err(bad_cert())
        }
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        self.sigs.v12(m, c, d)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        self.sigs.v13(m, c, d)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.sigs.schemes()
    }
}

/// Server verifier for outbound connections. `None` is used only while pairing; the pairing
/// proof then binds both fingerprints so a man in the middle cannot complete it.
#[derive(Debug)]
struct PinnedServer {
    expected: Option<String>,
    sigs: Sigs,
}

impl ServerCertVerifier for PinnedServer {
    fn verify_server_cert(&self, end: &CertificateDer<'_>, _i: &[CertificateDer<'_>], _n: &ServerName<'_>, _o: &[u8], _now: UnixTime) -> Result<ServerCertVerified, Error> {
        match &self.expected {
            Some(fp) if *fp != fingerprint(end) => Err(bad_cert()),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }
    fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        self.sigs.v12(m, c, d)
    }
    fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        self.sigs.v13(m, c, d)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.sigs.schemes()
    }
}

fn key(id: &Identity) -> PrivateKeyDer<'static> {
    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(id.key_der.clone()))
}

pub fn server_config(id: &Identity, trust: Arc<Trust>) -> anyhow::Result<Arc<rustls::ServerConfig>> {
    let p = provider();
    let verifier = Arc::new(PinnedClients { trust, sigs: Sigs(p.clone()) });
    let cfg = rustls::ServerConfig::builder_with_provider(p)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(verifier)
        .with_single_cert(vec![CertificateDer::from(id.cert_der.clone())], key(id))?;
    Ok(Arc::new(cfg))
}

pub fn client_config(id: &Identity, expected: Option<String>) -> anyhow::Result<Arc<rustls::ClientConfig>> {
    let p = provider();
    let verifier = Arc::new(PinnedServer { expected, sigs: Sigs(p.clone()) });
    let cfg = rustls::ClientConfig::builder_with_provider(p)
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_client_auth_cert(vec![CertificateDer::from(id.cert_der.clone())], key(id))?;
    Ok(Arc::new(cfg))
}

pub fn server_name() -> ServerName<'static> {
    ServerName::try_from("rac.peer").expect("static name")
}
