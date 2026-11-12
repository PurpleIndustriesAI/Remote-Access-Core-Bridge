//! Moonlight link and app channels on real loopback bridges (clipboard off).
//!
//! A host trusts a client because Sunshine paired its Moonlight key; the client trusts the
//! host because it pins the fingerprint Sunshine published. Every way around that must fail.

use rac::config::{self, Settings, State};
use rac::daemon::{self, control_call};
use rac::moonlight;
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, RSA_PKCS1_SHA256, RsaKeyPair};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

const CERT_PEM: &str = include_str!("fixtures/moonlight-test-client.cert.pem");
const KEY_PK8: &[u8] = include_bytes!("fixtures/moonlight-test-client.key.pk8.der");

fn node_dir(root: &Path, name: &str, port: u16, tweak: impl FnOnce(&mut Settings)) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let mut settings = Settings { port, receive_dir: Some(dir.join("inbox")), ..Settings::default() };
    tweak(&mut settings);
    config::save_state(&dir, &State { peers: vec![], settings }).unwrap();
    dir
}

async fn call(dir: &Path, v: Value) -> anyhow::Result<Value> {
    control_call(dir, v).await
}

async fn wait_up(dir: &Path) {
    for _ in 0..100 {
        if call(dir, json!({"cmd": "status"})).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("bridge did not start");
}

fn free_base() -> u16 {
    use std::net::TcpListener;
    let ok = |p: u16| TcpListener::bind(("127.0.0.1", p)).is_ok() && TcpListener::bind(("127.0.0.1", p + 1)).is_ok();
    (20050..60000).step_by(100).find(|&b| (1..=4).all(|k| ok(b + k * 10))).expect("no free port block")
}

fn start(dir: &Path) {
    let d = dir.to_path_buf();
    tokio::spawn(async move {
        let _ = daemon::run(daemon::Options { dir: d, clipboard: false, loopback_only: true }).await;
    });
}

fn rsa_sign(msg: &str) -> String {
    let key = RsaKeyPair::from_pkcs8(KEY_PK8).unwrap();
    let mut sig = vec![0u8; key.public().modulus_len()];
    key.sign(&RSA_PKCS1_SHA256, &ring::rand::SystemRandom::new(), msg.as_bytes(), &mut sig).unwrap();
    hex::encode(sig)
}

fn sunshine_state(path: &Path, certs: &[&str]) {
    let devices: Vec<Value> = certs.iter().map(|c| json!({"name": "Miku Desktop", "cert": c, "uuid": "x"})).collect();
    std::fs::write(path, json!({"root": {"uniqueid": "u", "named_devices": devices}}).to_string()).unwrap();
}

/// A second, unpaired client: a fresh ECDSA identity that Sunshine has never seen.
fn stranger() -> (String, EcdsaKeyPair) {
    let rng = ring::rand::SystemRandom::new();
    let pk8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
    let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pk8.as_ref(), &rng).unwrap();
    let kp = rcgen::KeyPair::try_from(pk8.as_ref()).unwrap();
    let cert = rcgen::CertificateParams::new(vec!["x".into()]).unwrap().self_signed(&kp).unwrap();
    (cert.pem(), key)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn moonlight_link_and_channels() {
    let root = std::env::temp_dir().join(format!("rac-ml-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let base = free_base();
    let state = root.join("sunshine_state.json");
    let (stranger_pem, stranger_key) = stranger();
    sunshine_state(&state, &[CERT_PEM]);

    let host_addr = format!("127.0.0.1:{}", base + 10);
    let host = node_dir(&root, "host", base + 10, |s| {
        s.moonlight_link = Some(state.clone());
        s.channels = vec!["crew-link".into()];
    });
    let client = node_dir(&root, "client", base + 20, |s| s.channels = vec!["crew-link".into()]);
    let other = node_dir(&root, "other", base + 30, |_| {});
    let off = node_dir(&root, "off", base + 40, |_| {});
    for d in [&host, &client, &other, &off] {
        start(d);
        wait_up(d).await;
    }
    let host_fp = call(&host, json!({"cmd": "status"})).await.unwrap()["fingerprint"].as_str().unwrap().to_string();
    let title = call(&host, json!({"cmd": "host_app_title"})).await.unwrap()["title"].as_str().unwrap().to_string();
    assert_eq!(title, format!("RAC:{host_fp}"));

    // What the client signs names its own bridge; the caller cannot choose another one.
    let m = call(&client, json!({"cmd": "link_message", "host_fp": host_fp})).await.unwrap();
    let client_fp = m["client_fp"].as_str().unwrap().to_string();
    let message = m["message"].as_str().unwrap().to_string();
    assert_eq!(message, moonlight::link_message(&host_fp, &client_fp));
    let sig = rsa_sign(&message);

    // A wrong pinned host fingerprint (an impostor host) fails before anything is sent.
    let err = call(&client, json!({"cmd": "moonlight_link", "addr": host_addr, "host_fp": "11".repeat(32), "moonlight_cert": CERT_PEM, "sig": sig})).await.unwrap_err();
    assert!(format!("{err:#}").contains("did not prove the identity"), "{err:#}");

    // A host with Moonlight link off rejects unknown certificates at the TLS layer.
    let off_fp = call(&off, json!({"cmd": "status"})).await.unwrap()["fingerprint"].as_str().unwrap().to_string();
    let err = call(&client, json!({"cmd": "moonlight_link", "addr": format!("127.0.0.1:{}", base + 40), "host_fp": off_fp, "moonlight_cert": CERT_PEM, "sig": sig})).await.unwrap_err();
    assert!(format!("{err:#}").contains("refused") || format!("{err:#}").contains("did not prove"), "{err:#}");

    // A signature from the paired key but over another bridge's fingerprint is refused (replay).
    let replay = rsa_sign(&moonlight::link_message(&host_fp, &"22".repeat(32)));
    let err = call(&client, json!({"cmd": "moonlight_link", "addr": host_addr, "host_fp": host_fp, "moonlight_cert": CERT_PEM, "sig": replay})).await.unwrap_err();
    assert!(format!("{err:#}").contains("signature does not match"), "{err:#}");

    // A valid signature from a client Sunshine never paired is refused.
    let om = call(&other, json!({"cmd": "link_message", "host_fp": host_fp})).await.unwrap()["message"].as_str().unwrap().to_string();
    let osig = hex::encode(stranger_key.sign(&ring::rand::SystemRandom::new(), om.as_bytes()).unwrap().as_ref());
    let err = call(&other, json!({"cmd": "moonlight_link", "addr": host_addr, "host_fp": host_fp, "moonlight_cert": stranger_pem, "sig": osig})).await.unwrap_err();
    assert!(format!("{err:#}").contains("not paired with the host's Sunshine"), "{err:#}");

    // An unknown certificate admitted only for linking cannot send clipboard data.
    // The intruder grants itself approval locally; only the host's own trust decides.
    let mut st = config::load_state(&other).unwrap();
    st.peers.push(config::Peer { name: "host".into(), fingerprint: host_fp.clone(), addrs: vec![host_addr.clone()], autosync: true, paired_at: 0, permissions: config::Permissions { clipboard: true, files: true, ..Default::default() } });
    config::save_state(&other, &st).unwrap();
    let other2 = node_dir(&root, "other2", base + 50, |_| {});
    let mut st2 = config::load_state(&other2).unwrap();
    st2.peers = st.peers.clone();
    config::save_state(&other2, &st2).unwrap();
    start(&other2);
    wait_up(&other2).await;
    let r = call(&other2, json!({"cmd": "send", "text": "sneaky"})).await.unwrap();
    assert_eq!(r["results"][0]["ok"], json!(false), "{r}");

    // Linking proves identity. The host still requires per-device consent.
    let r = call(&client, json!({"cmd": "moonlight_link", "addr": host_addr, "host_fp": host_fp, "moonlight_cert": CERT_PEM, "sig": sig})).await.unwrap();
    assert_eq!(r["fingerprint"].as_str().unwrap(), host_fp);
    let hs = call(&host, json!({"cmd": "status"})).await.unwrap();
    let linked: Vec<&Value> = hs["peers"].as_array().unwrap().iter().filter(|p| p["fingerprint"] == json!(client_fp)).collect();
    assert_eq!(linked.len(), 1, "{hs}");
    assert_eq!(linked[0]["name"], json!("Miku Desktop via Moonlight"));
    for capability in ["clipboard", "files"] {
        call(&host, json!({"cmd":"permissions", "peer":client_fp, "capability":capability, "allowed":true})).await.unwrap();
    }
    call(&client, json!({"cmd": "send", "text": "from client"})).await.unwrap();
    call(&host, json!({"cmd": "send", "peer": client_fp, "text": "from host"})).await.unwrap();

    // App channel: verified file lands in the channel inbox, never in Downloads.
    let env = root.join("envelope.json");
    std::fs::write(&env, br#"{"signed":"envelope"}"#).unwrap();
    let r = call(&client, json!({"cmd": "channel_send", "peer": host_fp, "channel": "crew-link", "path": env})).await.unwrap();
    assert_eq!(r["bytes"], json!(21));
    // Files land in a folder named for the sender, proven by its TLS certificate.
    let inbox = config::channel_inbox(&host, "crew-link").unwrap().join(&client_fp);
    assert_eq!(std::fs::read(inbox.join("envelope.json")).unwrap(), br#"{"signed":"envelope"}"#);
    assert!(!host.join("inbox").join("envelope.json").exists());
    // A channel the receiver did not enable is refused.
    let err = call(&client, json!({"cmd": "channel_send", "peer": host_fp, "channel": "other-app", "path": env})).await.unwrap_err();
    assert!(format!("{err:#}").contains("not enabled"), "{err:#}");
    // Bad channel names are refused locally.
    assert!(call(&client, json!({"cmd": "channel_send", "peer": host_fp, "channel": "../x", "path": env})).await.is_err());
    // Host to client works the same way.
    call(&host, json!({"cmd": "channel_send", "peer": client_fp, "channel": "crew-link", "path": env})).await.unwrap();
    assert!(config::channel_inbox(&client, "crew-link").unwrap().join(&host_fp).join("envelope.json").exists());

    // Session routing needs its own grant on both machines, beyond files and the channel.
    for d in [&host, &client] {
        call(d, json!({"cmd":"set", "channels":["crew-link", "crew-sessions"]})).await.unwrap();
    }
    let session = json!({"cmd":"channel_send", "peer":host_fp, "channel":"crew-sessions", "path":env});
    assert!(call(&client, session.clone()).await.is_err(), "session channel bypassed routing consent");
    call(&host, json!({"cmd":"permissions", "peer":client_fp, "capability":"session_routing", "allowed":true})).await.unwrap();
    assert!(call(&client, session.clone()).await.is_err(), "session channel bypassed sender consent");
    call(&client, json!({"cmd":"permissions", "peer":host_fp, "capability":"session_routing", "allowed":true})).await.unwrap();
    call(&client, session.clone()).await.unwrap();
    call(&host, json!({"cmd":"permissions", "peer":client_fp, "capability":"session_routing", "allowed":false})).await.unwrap();
    assert!(call(&client, session).await.is_err(), "revoked session consent still accepted");

    // Turning Moonlight link off closes the door for new unknown certificates.
    call(&host, json!({"cmd": "set", "moonlight_link": ""})).await.unwrap();
    let err = call(&other, json!({"cmd": "moonlight_link", "addr": host_addr, "host_fp": host_fp, "moonlight_cert": stranger_pem, "sig": osig})).await.unwrap_err();
    assert!(!format!("{err:#}").is_empty());
    // Disabling linking also blocks data from linked peers.
    let r = call(&client, json!({"cmd": "send", "text": "link disabled"})).await.unwrap();
    assert_eq!(r["results"][0]["ok"], json!(false), "{r}");
    // The host can still revoke a linked client's approval, like any other peer's.
    call(&host, json!({"cmd": "permissions", "peer": client_fp, "capability": "clipboard", "allowed": false})).await.unwrap();
    let r = call(&client, json!({"cmd": "send", "text": "after revoke"})).await.unwrap();
    assert_eq!(r["results"][0]["ok"], json!(false), "{r}");
    let _ = std::fs::remove_dir_all(&root);
}


#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn link_security_regressions() {
    use tokio::io::AsyncReadExt;
    let root = std::env::temp_dir().join(format!("rac-link-security-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let base = free_base();
    let state = root.join("sunshine_state.json");
    sunshine_state(&state, &[CERT_PEM]);
    let host = node_dir(&root, "host", base + 10, |s| {
        s.moonlight_link = Some(state.clone());
        s.channels = vec!["crew-link".into()];
    });
    let client = node_dir(&root, "client", base + 20, |_| {});
    for d in [&host, &client] { start(d); wait_up(d).await; }
    let hs = call(&host, json!({"cmd":"status"})).await.unwrap();
    let host_fp = hs["fingerprint"].as_str().unwrap();
    let m = call(&client, json!({"cmd":"link_message", "host_fp":host_fp})).await.unwrap();
    let client_fp = m["client_fp"].as_str().unwrap();
    let link = json!({"cmd":"moonlight_link", "addr":format!("127.0.0.1:{}",base+10),
        "host_fp":host_fp, "moonlight_cert":CERT_PEM, "sig":rsa_sign(m["message"].as_str().unwrap())});
    call(&client, link.clone()).await.unwrap();
    let mut failures = Vec::new();
    let denied = |r: Value| r["results"][0]["ok"] == json!(false);
    if !denied(call(&client,json!({"cmd":"send","text":"before consent"})).await.unwrap()) {
        failures.push("Moonlight pairing granted host clipboard access without peer approval");
    }
    for capability in ["clipboard","files"] {
        call(&host,json!({"cmd":"permissions","peer":client_fp,"capability":capability,"allowed":true})).await.unwrap();
    }
    assert!(!denied(call(&client,json!({"cmd":"send","text":"approved"})).await.unwrap()));
    call(&host,json!({"cmd":"permissions","peer":client_fp,"capability":"clipboard","allowed":false})).await.unwrap();
    call(&client,link).await.unwrap();
    if !denied(call(&client,json!({"cmd":"send","text":"after relink"})).await.unwrap()) {
        failures.push("Relinking restored revoked clipboard approval");
    }
    let file = root.join("probe.txt");
    std::fs::write(&file,b"synthetic channel probe").unwrap();
    call(&host,json!({"cmd":"permissions","peer":client_fp,"capability":"files","allowed":false})).await.unwrap();
    let channel = json!({"cmd":"channel_send","peer":host_fp,"channel":"crew-link","path":file});
    if call(&client,channel.clone()).await.is_ok() { failures.push("Channel bypassed receiver file approval"); }
    call(&host,json!({"cmd":"permissions","peer":client_fp,"capability":"files","allowed":true})).await.unwrap();
    call(&client,json!({"cmd":"permissions","peer":host_fp,"capability":"files","allowed":false})).await.unwrap();
    if call(&client,channel.clone()).await.is_ok() { failures.push("Channel bypassed sender file approval"); }
    call(&client,json!({"cmd":"permissions","peer":host_fp,"capability":"files","allowed":true})).await.unwrap();
    call(&host,json!({"cmd":"permissions","peer":client_fp,"capability":"clipboard","allowed":true})).await.unwrap();
    sunshine_state(&state,&[]);
    if !denied(call(&client,json!({"cmd":"send","text":"after Sunshine unpair"})).await.unwrap()) {
        failures.push("Data access survived Sunshine unpairing");
    }
    if call(&client,channel).await.is_ok() { failures.push("Channel access survived Sunshine unpairing"); }
    let arbitrary = root.join("other.json");
    sunshine_state(&arbitrary,&[CERT_PEM]);
    if call(&host,json!({"cmd":"set","moonlight_link":arbitrary})).await.is_ok() {
        failures.push("Moonlight link accepted an arbitrary JSON file");
    }
    let mut socket = tokio::net::TcpStream::connect(format!("127.0.0.1:{}",base+10)).await.unwrap();
    let mut buf=[0u8;1];
    if tokio::time::timeout(Duration::from_secs(8),socket.read(&mut buf)).await.is_err() {
        failures.push("Unauthenticated TLS connection held a slot longer than eight seconds");
    }
    assert!(failures.is_empty(),"Security regressions: {failures:?}");
}
