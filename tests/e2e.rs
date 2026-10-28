//! Three real bridges on loopback (clipboard off): pairing, transfers, and rejections.

use rac::config::{self, Settings, State};
use rac::daemon::{self, control_call};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn node_dir(root: &Path, name: &str, port: u16) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let st = State { peers: vec![], settings: Settings { port, receive_dir: Some(dir.join("inbox")), ..Settings::default() } };
    config::save_state(&dir, &st).unwrap();
    dir
}

async fn call(dir: &Path, v: Value) -> anyhow::Result<Value> {
    control_call(dir, v).await
}

async fn wait_up(dir: &Path) {
    let mut last = String::new();
    for _ in 0..100 {
        match call(dir, json!({"cmd": "status"})).await {
            Ok(_) => return,
            Err(e) => last = format!("{e:#}"),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("bridge did not start: {last}");
}

/// A block of 100 ports where each bridge's peer port and control port (+1) can be bound.
/// Fixed ports break when Windows reserves their range (Hyper-V, WSL and Docker do).
fn free_base() -> u16 {
    use std::net::TcpListener;
    let ok = |p: u16| TcpListener::bind(("127.0.0.1", p)).is_ok() && TcpListener::bind(("127.0.0.1", p + 1)).is_ok();
    (20000..60000).step_by(100).find(|&b| (1..=5).all(|k| ok(b + k * 10))).expect("no free port block")
}

fn start(dir: &Path) {
    let d = dir.to_path_buf();
    tokio::spawn(async move {
        if let Err(e) = daemon::run(daemon::Options { dir: d.clone(), clipboard: false, loopback_only: true }).await {
            eprintln!("bridge in {} exited: {e:#}", d.display());
        }
    });
}

#[tokio::test(flavor = "current_thread")]
async fn transport_consent_is_bound_to_the_payload_connection() {
    use rac::{proto::{self, Msg, Reply}, tls};
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;
    use tokio_rustls::TlsConnector;

    let root = std::env::temp_dir().join(format!("rac-transport-{}", std::process::id()));
    let base = free_base();
    let a = node_dir(&root, "a", base + 10);
    let b = node_dir(&root, "b", base + 20);
    start(&a);
    start(&b);
    wait_up(&a).await;
    wait_up(&b).await;
    let addr = format!("127.0.0.1:{}", base + 10);
    let code = call(&a, json!({"cmd":"pair_code"})).await.unwrap()["code"].as_str().unwrap().to_owned();
    let paired = call(&b, json!({"cmd":"pair", "addr":addr, "code":code})).await.unwrap();
    let fp = paired["fingerprint"].as_str().unwrap().to_owned();
    let status = call(&a, json!({"cmd":"status"})).await.unwrap();
    let b_fp = status["peers"][0]["fingerprint"].as_str().unwrap();
    call(&a, json!({"cmd":"permissions", "peer":b_fp, "capability":"clipboard", "allowed":true})).await.unwrap();
    let id = config::load_or_create_identity(&b).unwrap();
    let connector = TlsConnector::from(tls::client_config(&id, Some(fp)).unwrap());

    // A paired client must not bypass transport consent by sending content first.
    let socket = TcpStream::connect(&addr).await.unwrap();
    let mut stream = connector.connect(tls::server_name(), socket).await.unwrap();
    proto::write_json(&mut stream, &Msg::Text { len: 4 }).await.unwrap();
    stream.write_all(b"test").await.unwrap();
    let reply: Reply = proto::read_json(&mut stream).await.unwrap();
    assert!(!reply.ok, "unverified content was accepted");

    for revoke in [true, false] {
        call(&a, json!({"cmd":"set", "transport_wifi":true})).await.unwrap();
        let socket = TcpStream::connect(&addr).await.unwrap();
        let mut stream = connector.connect(tls::server_name(), socket).await.unwrap();
        proto::write_json(&mut stream, &Msg::VerifyTransport {
            transport: "wifi".into(), policy: Default::default(),
        }).await.unwrap();
        let reply: Value = proto::read_json(&mut stream).await.unwrap();
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["transport_session"], "wifi", "missing connection bound receipt");
        if revoke {
            call(&a, json!({"cmd":"set", "transport_wifi":false})).await.unwrap();
        }
        proto::write_json(&mut stream, &Msg::Text { len: 4 }).await.unwrap();
        stream.write_all(b"test").await.unwrap();
        let reply: Reply = proto::read_json(&mut stream).await.unwrap();
        assert_eq!(reply.ok, !revoke, "policy revocation was not enforced");
    }

    // A legacy preflight reply must not trigger a second connection or a payload.
    let c = node_dir(&root, "c", base + 30);
    let client_id = config::load_or_create_identity(&c).unwrap();
    let server_id = config::load_or_create_identity(&a).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let legacy_addr = listener.local_addr().unwrap().to_string();
    let trust = std::sync::Arc::new(tls::Trust::default());
    trust.peers.write().unwrap().insert(client_id.fingerprint.clone());
    let acceptor = tokio_rustls::TlsAcceptor::from(tls::server_config(&server_id, trust).unwrap());
    let mut state = config::load_state(&c).unwrap();
    state.peers.push(config::Peer {
        name: "legacy fixture".into(), fingerprint: server_id.fingerprint.clone(),
        addrs: vec![legacy_addr], autosync: false, paired_at: 0, permissions: Default::default(),
    });
    config::save_state(&c, &state).unwrap();
    let legacy = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let mut stream = acceptor.accept(socket).await.unwrap();
        let message: Msg = proto::read_json(&mut stream).await.unwrap();
        assert!(matches!(message, Msg::VerifyTransport { .. }));
        proto::write_json(&mut stream, &json!({"ok":true,"policy":{"wifi":true,"usb":false,"bluetooth":false}})).await.unwrap();
        let next = tokio::time::timeout(Duration::from_secs(2), proto::read_json::<_, Msg>(&mut stream)).await;
        assert!(matches!(next, Ok(Err(_))), "client sent content or kept an unverified stream open");
    });
    start(&c);
    wait_up(&c).await;
    let error = call(&c, json!({"cmd":"ping", "peer":server_id.fingerprint})).await.unwrap_err();
    assert!(format!("{error:#}").contains("payload connection"), "{error:#}");
    legacy.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pair_send_and_reject() {
    let root = std::env::temp_dir().join(format!("rac-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let base = free_base();
    let a_addr = format!("127.0.0.1:{}", base + 10);
    let a = node_dir(&root, "a", base + 10);
    let b = node_dir(&root, "b", base + 20);
    let c = node_dir(&root, "c", base + 30);

    // A terminal running as another Windows user has no token; the error says what to do.
    let err = call(&c, json!({"cmd": "status"})).await.unwrap_err();
    assert!(format!("{err:#}").contains("normal (not Administrator) terminal"), "{err:#}");
    for d in [&a, &b, &c] {
        start(d);
        wait_up(d).await;
    }

    // Without an open pairing window, a correct-looking code is refused.
    let err = call(&b, json!({"cmd": "pair", "addr": a_addr, "code": "AAAA-AAAA-AAAA-AAAA"})).await.unwrap_err();
    assert!(format!("{err:#}").contains("open pairing there first"), "{err:#}");

    // Five wrong codes close the window; the right code is then refused too.
    let code = call(&a, json!({"cmd": "pair_code"})).await.unwrap()["code"].as_str().unwrap().to_string();
    for _ in 0..5 {
        assert!(call(&c, json!({"cmd": "pair", "addr": a_addr, "code": "BBBB-BBBB-BBBB-BBBB"})).await.is_err());
    }
    assert!(call(&c, json!({"cmd": "pair", "addr": a_addr, "code": code})).await.is_err());

    // Proper pairing.
    let code = call(&a, json!({"cmd": "pair_code"})).await.unwrap()["code"].as_str().unwrap().to_string();
    let r = call(&b, json!({"cmd": "pair", "addr": a_addr, "code": code})).await.unwrap();
    let a_fp = r["fingerprint"].as_str().unwrap().to_string();
    let sa = call(&a, json!({"cmd": "status"})).await.unwrap();
    assert_eq!(sa["fingerprint"].as_str().unwrap(), a_fp);
    assert_eq!(sa["peers"].as_array().unwrap().len(), 1);
    let b_fp = sa["peers"][0]["fingerprint"].as_str().unwrap().to_string();
    // The code is single use.
    assert!(call(&c, json!({"cmd": "pair", "addr": a_addr, "code": code})).await.is_err());

    // Both directions work.
    // Pairing is identity verification, not consent. Explicit sends cannot bypass it.
    assert_eq!(sa["permissions_version"], 1);
    assert_eq!(sa["peers"][0]["permissions"]["clipboard"], false);
    assert!(call(&b, json!({"cmd":"send", "text":"not approved"})).await.is_err());
    assert!(call(&a, json!({"cmd":"permissions", "peer":"unknown", "capability":"clipboard", "allowed":true})).await.is_err());
    assert!(call(&a, json!({"cmd":"permissions", "peer":b_fp, "capability":"all", "allowed":true})).await.is_err());
    call(&b, json!({"cmd":"permissions", "peer":a_fp, "capability":"clipboard", "allowed":true})).await.unwrap();
    let denied = call(&b, json!({"cmd":"send", "text":"receiver has not approved"})).await.unwrap();
    assert_eq!(denied["results"][0]["ok"], false);
    call(&a, json!({"cmd":"permissions", "peer":b_fp, "capability":"clipboard", "allowed":true})).await.unwrap();
    assert!(config::load_state(&a).unwrap().peers[0].permissions.clipboard);
    assert!(!config::load_state(&a).unwrap().peers[0].permissions.files);
    call(&b, json!({"cmd": "send", "text": "hello from b"})).await.unwrap();
    call(&a, json!({"cmd": "send", "peer": b_fp, "text": "hello from a"})).await.unwrap();
    call(&a, json!({"cmd": "ping", "peer": b_fp})).await.unwrap();

    // Either machine can block the actual transport. Both directions fail closed and recover.
    call(&a, json!({"cmd": "set", "transport_wifi": false})).await.unwrap();
    let denied = call(&b, json!({"cmd": "send", "text": "blocked by receiver"})).await.unwrap();
    assert_eq!(denied["results"][0]["ok"], json!(false), "{denied}");
    call(&a, json!({"cmd": "set", "transport_wifi": true})).await.unwrap();
    call(&b, json!({"cmd": "set", "transport_wifi": false})).await.unwrap();
    let denied = call(&a, json!({"cmd": "send", "peer": b_fp, "text": "blocked by sender"})).await.unwrap();
    assert_eq!(denied["results"][0]["ok"], json!(false), "{denied}");
    call(&b, json!({"cmd": "set", "transport_wifi": true})).await.unwrap();
    call(&a, json!({"cmd": "send", "peer": b_fp, "text": "allowed again"})).await.unwrap();

    // Network helper: latency probe and two-way speed test over the paired link.
    let p = call(&a, json!({"cmd": "probe", "peer": b_fp, "n": 10})).await.unwrap();
    assert_eq!(p["rtts_ms"].as_array().unwrap().len(), 10, "{p}");
    let s = call(&a, json!({"cmd": "speedtest", "peer": b_fp, "mib": 4})).await.unwrap();
    assert!(s["up_mbps"].as_f64().unwrap() > 1.0 && s["down_mbps"].as_f64().unwrap() > 1.0, "{s}");
    assert!(s["recommended_moonlight_mbps"].as_u64().unwrap() >= 5, "{s}");
    let w = call(&a, json!({"cmd": "wifi"})).await.unwrap();
    assert!(w.get("advice").is_some(), "{w}");

    // Files and a folder, with unsafe-looking names.
    let src = root.join("src");
    std::fs::create_dir_all(src.join("folder/sub")).unwrap();
    std::fs::write(src.join("report.txt"), b"quarterly").unwrap();
    std::fs::write(src.join("folder/sub/data.bin"), vec![7u8; 3_000_000]).unwrap();
    std::fs::write(src.join("folder/CON.txt"), b"reserved name").unwrap();
    let paths = json!([src.join("report.txt"), src.join("folder")]);
    assert!(call(&b, json!({"cmd":"send", "paths":paths})).await.is_err());
    call(&b, json!({"cmd":"permissions", "peer":a_fp, "capability":"files", "allowed":true})).await.unwrap();
    let denied = call(&b, json!({"cmd":"send", "paths":paths})).await.unwrap();
    assert_eq!(denied["results"][0]["ok"], false);
    assert!(!a.join("inbox").exists());
    call(&a, json!({"cmd":"permissions", "peer":b_fp, "capability":"files", "allowed":true})).await.unwrap();
    let r = call(&b, json!({"cmd": "send", "paths": paths})).await.unwrap();
    assert_eq!(r["results"][0]["ok"], json!(true), "{r}");
    let inbox = a.join("inbox");
    assert_eq!(std::fs::read(inbox.join("report.txt")).unwrap(), b"quarterly");
    assert_eq!(std::fs::read(inbox.join("folder/sub/data.bin")).unwrap().len(), 3_000_000);
    assert!(inbox.join("folder/_CON.txt").exists());
    // Sending again never overwrites.
    call(&b, json!({"cmd": "send", "paths": [src.join("report.txt")]})).await.unwrap();
    assert!(inbox.join("report (2).txt").exists());
    assert!(!std::fs::read_dir(&inbox).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with(".rac-part")));

    // An unpaired machine that pins A's identity is rejected during the TLS handshake.
    let mut st = config::load_state(&c).unwrap();
    st.peers.push(config::Peer { name: "a".into(), fingerprint: a_fp.clone(), addrs: vec![a_addr.clone()], autosync: true, paired_at: 0, permissions: config::Permissions { clipboard:true, files:true, ..Default::default() } });
    config::save_state(&c, &st).unwrap();
    // C loaded its state at start, so restart is not possible in-process; use a fresh node D instead.
    let d = node_dir(&root, "d", base + 40);
    let mut st = config::load_state(&d).unwrap();
    st.peers = config::load_state(&c).unwrap().peers;
    config::save_state(&d, &st).unwrap();
    start(&d);
    wait_up(&d).await;
    let r = call(&d, json!({"cmd": "send", "text": "intruder"})).await.unwrap();
    assert_eq!(r["results"][0]["ok"], json!(false), "{r}");

    // A peer that pins the wrong fingerprint for A refuses to talk to it.
    let e = node_dir(&root, "e", base + 50);
    let mut st = config::load_state(&e).unwrap();
    st.peers.push(config::Peer { name: "fake".into(), fingerprint: "00".repeat(32), addrs: vec![a_addr.clone()], autosync: true, paired_at: 0, permissions: config::Permissions { clipboard:true, files:true, ..Default::default() } });
    config::save_state(&e, &st).unwrap();
    start(&e);
    wait_up(&e).await;
    let r = call(&e, json!({"cmd": "send", "text": "secret"})).await.unwrap();
    assert_eq!(r["results"][0]["ok"], json!(false), "{r}");

    // A bad control token is refused.
    let tok = std::fs::read_to_string(b.join("control.token")).unwrap();
    std::fs::write(b.join("control.token"), "0".repeat(64)).unwrap();
    assert!(call(&b, json!({"cmd": "status"})).await.is_err());
    std::fs::write(b.join("control.token"), tok).unwrap();

    // After A unpairs B, B can no longer deliver anything to A.
    call(&a, json!({"cmd":"permissions", "peer":b_fp, "capability":"clipboard", "allowed":false})).await.unwrap();
    let denied = call(&b, json!({"cmd":"send", "text":"revoked"})).await.unwrap();
    assert_eq!(denied["results"][0]["ok"], false);
    assert!(call(&a, json!({"cmd":"send", "text":"revoked outbound"})).await.is_err());
    assert!(!config::load_state(&a).unwrap().peers[0].permissions.clipboard);
    call(&a, json!({"cmd": "unpair", "peer": b_fp})).await.unwrap();
    let r = call(&b, json!({"cmd": "send", "text": "after unpair"})).await.unwrap();
    assert_eq!(r["results"][0]["ok"], json!(false), "{r}");
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "current_thread")]
async fn device_inventory_requires_separate_client_consent() {
    let root = std::env::temp_dir().join(format!("racdevices{}", std::process::id()));
    let base = free_base();
    let host = node_dir(&root, "host", base + 40);
    let client = node_dir(&root, "client", base + 50);
    start(&host);
    start(&client);
    wait_up(&host).await;
    wait_up(&client).await;
    let code = call(&host, json!({"cmd":"pair_code"})).await.unwrap()["code"].as_str().unwrap().to_owned();
    let host_fp = call(&client, json!({"cmd":"pair", "addr":format!("127.0.0.1:{}",base+40), "code":code})).await.unwrap()["fingerprint"].as_str().unwrap().to_owned();
    let client_fp = call(&host, json!({"cmd":"status"})).await.unwrap()["peers"][0]["fingerprint"].as_str().unwrap().to_owned();
    let denied = call(&client, json!({"cmd":"devices", "peer":host_fp})).await.unwrap_err();
    assert!(format!("{denied:#}").contains("local client approval"));
    assert!(call(&client, json!({"cmd":"request_access", "peer":host_fp, "capabilities":["device_inventory", 1]})).await.is_err());
    call(&host, json!({"cmd":"set", "allow_access_requests":false})).await.unwrap();
    assert!(call(&client, json!({"cmd":"request_access", "peer":host_fp, "capabilities":["device_inventory"]})).await.is_err());
    assert!(!config::load_state(&client).unwrap().peers[0].permissions.device_inventory);
    // Existing clipboard and file consent must never authorize hardware inspection.
    for capability in ["clipboard", "files"] {
        call(&host, json!({"cmd":"permissions", "peer":client_fp, "capability":capability, "allowed":true})).await.unwrap();
    }
    assert!(call(&client, json!({"cmd":"devices", "peer":host_fp})).await.is_err());
    call(&host, json!({"cmd":"permissions", "peer":client_fp, "capability":"device_inventory", "allowed":true})).await.unwrap();
    assert!(config::load_state(&host).unwrap().peers[0].permissions.device_inventory);
    assert!(!config::load_state(&client).unwrap().peers[0].permissions.device_inventory);
    #[cfg(windows)]
    {
        let report = call(&client, json!({"cmd":"devices", "peer":host_fp})).await.unwrap();
        let devices = report["inventory"]["devices"].as_array().unwrap();
        assert!(devices.len() <= 64);
        for device in devices {
            assert_eq!(device.as_object().unwrap().len(), 3);
            assert!(device.get("serial").is_none() && device.get("InstanceId").is_none());
        }
    }
    call(&host, json!({"cmd":"permissions", "peer":client_fp, "capability":"device_inventory", "allowed":false})).await.unwrap();
    assert!(call(&client, json!({"cmd":"devices", "peer":host_fp})).await.is_err());
}

/// A client asks the host for access; the host owner approves, and the host setting can refuse requests.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn access_request_flow() {
    let root = std::env::temp_dir().join(format!("rac-access-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    // Owner notifications go to a fake Crew home, never the real one.
    let crew = root.join("crew");
    std::fs::create_dir_all(crew.join("apps/notification-bridge/data")).unwrap();
    unsafe { std::env::set_var("CODEXCREW_HOME", &crew) };
    // Offset by 5 so a parallel test using the same block never shares a port.
    let base = free_base() + 5;
    let host = node_dir(&root, "host", base + 10);
    let client = node_dir(&root, "client", base + 20);
    for d in [&host, &client] {
        start(d);
        wait_up(d).await;
    }
    let code = call(&host, json!({"cmd": "pair_code"})).await.unwrap()["code"].as_str().unwrap().to_string();
    let host_fp = call(&client, json!({"cmd": "pair", "addr": format!("127.0.0.1:{}", base + 10), "code": code})).await.unwrap()["fingerprint"].as_str().unwrap().to_string();
    let client_fp = call(&host, json!({"cmd": "status"})).await.unwrap()["peers"][0]["fingerprint"].as_str().unwrap().to_string();

    // Invalid capability lists are refused before anything is sent or granted.
    assert!(call(&client, json!({"cmd": "request_access", "peer": host_fp, "capabilities": ["admin"]})).await.is_err());
    assert!(!config::load_state(&client).unwrap().peers[0].permissions.clipboard);

    // Requests off: refused, and the host records nothing pending.
    call(&host, json!({"cmd": "set", "allow_access_requests": false})).await.unwrap();
    let err = call(&client, json!({"cmd": "request_access", "peer": host_fp, "capabilities": ["clipboard", "files"]})).await.unwrap_err();
    assert!(format!("{err:#}").contains("not accepting access requests"), "{err:#}");
    assert!(call(&host, json!({"cmd": "status"})).await.unwrap()["access_requests"].as_array().unwrap().is_empty());

    // Requests on: pending until the owner decides; a request alone grants nothing.
    call(&host, json!({"cmd": "set", "allow_access_requests": true})).await.unwrap();
    call(&client, json!({"cmd": "request_access", "peer": host_fp, "capabilities": ["clipboard", "files"]})).await.unwrap();
    let st = call(&host, json!({"cmd": "status"})).await.unwrap();
    assert_eq!(st["access_requests"][0]["peer"], client_fp.as_str());
    assert_eq!(st["access_requests"][0]["capabilities"], json!(["clipboard", "files"]));
    assert_eq!(st["peers"][0]["permissions"]["clipboard"], false);
    let r = call(&client, json!({"cmd": "send", "text": "before approval"})).await.unwrap();
    assert_eq!(r["results"][0]["ok"], false, "{r}");
    let inbox = std::fs::read_to_string(crew.join("apps/notification-bridge/data/inbox.jsonl")).unwrap();
    assert_eq!(inbox.lines().count(), 1);
    assert!(inbox.contains("remote-access-crew.access-requests") && inbox.contains("clipboard and files"), "{inbox}");

    // Deny clears it; a second request comes back; approve grants exactly what was asked.
    call(&host, json!({"cmd": "access_decision", "peer": client_fp, "approve": false})).await.unwrap();
    assert!(call(&host, json!({"cmd": "access_decision", "peer": client_fp, "approve": true})).await.is_err());
    call(&client, json!({"cmd": "request_access", "peer": host_fp, "capabilities": ["clipboard"]})).await.unwrap();
    call(&host, json!({"cmd": "access_decision", "peer": client_fp, "approve": true})).await.unwrap();
    let p = &config::load_state(&host).unwrap().peers[0].permissions;
    assert!(p.clipboard && !p.files);
    // Both directions now work for the clipboard.
    call(&client, json!({"cmd": "send", "text": "after approval"})).await.unwrap();
    call(&host, json!({"cmd": "send", "peer": client_fp, "text": "reply"})).await.unwrap();
    // Asking again for something already granted is a no-op.
    call(&client, json!({"cmd": "request_access", "peer": host_fp, "capabilities": ["clipboard"]})).await.unwrap();
    assert!(call(&host, json!({"cmd": "status"})).await.unwrap()["access_requests"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&root);
}
