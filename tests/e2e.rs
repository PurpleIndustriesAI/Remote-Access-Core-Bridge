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
    for _ in 0..100 {
        if call(dir, json!({"cmd": "status"})).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("bridge did not start");
}

fn start(dir: &Path) {
    let d = dir.to_path_buf();
    tokio::spawn(async move { daemon::run(daemon::Options { dir: d, clipboard: false }).await });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pair_send_and_reject() {
    let root = std::env::temp_dir().join(format!("rac-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let a = node_dir(&root, "a", 49710);
    let b = node_dir(&root, "b", 49720);
    let c = node_dir(&root, "c", 49730);
    for d in [&a, &b, &c] {
        start(d);
        wait_up(d).await;
    }

    // Without an open pairing window, a correct-looking code is refused.
    let err = call(&b, json!({"cmd": "pair", "addr": "127.0.0.1:49710", "code": "AAAA-AAAA-AAAA-AAAA"})).await.unwrap_err();
    assert!(format!("{err:#}").contains("open pairing there first"), "{err:#}");

    // Five wrong codes close the window; the right code is then refused too.
    let code = call(&a, json!({"cmd": "pair_code"})).await.unwrap()["code"].as_str().unwrap().to_string();
    for _ in 0..5 {
        assert!(call(&c, json!({"cmd": "pair", "addr": "127.0.0.1:49710", "code": "BBBB-BBBB-BBBB-BBBB"})).await.is_err());
    }
    assert!(call(&c, json!({"cmd": "pair", "addr": "127.0.0.1:49710", "code": code})).await.is_err());

    // Proper pairing.
    let code = call(&a, json!({"cmd": "pair_code"})).await.unwrap()["code"].as_str().unwrap().to_string();
    let r = call(&b, json!({"cmd": "pair", "addr": "127.0.0.1:49710", "code": code})).await.unwrap();
    let a_fp = r["fingerprint"].as_str().unwrap().to_string();
    let sa = call(&a, json!({"cmd": "status"})).await.unwrap();
    assert_eq!(sa["fingerprint"].as_str().unwrap(), a_fp);
    assert_eq!(sa["peers"].as_array().unwrap().len(), 1);
    let b_fp = sa["peers"][0]["fingerprint"].as_str().unwrap().to_string();
    // The code is single use.
    assert!(call(&c, json!({"cmd": "pair", "addr": "127.0.0.1:49710", "code": code})).await.is_err());

    // Both directions work.
    call(&b, json!({"cmd": "send", "text": "hello from b"})).await.unwrap();
    call(&a, json!({"cmd": "send", "peer": b_fp, "text": "hello from a"})).await.unwrap();
    call(&a, json!({"cmd": "ping", "peer": b_fp})).await.unwrap();

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
    st.peers.push(config::Peer { name: "a".into(), fingerprint: a_fp.clone(), addrs: vec!["127.0.0.1:49710".into()], autosync: true, paired_at: 0 });
    config::save_state(&c, &st).unwrap();
    // C loaded its state at start, so restart is not possible in-process; use a fresh node D instead.
    let d = node_dir(&root, "d", 49740);
    let mut st = config::load_state(&d).unwrap();
    st.peers = config::load_state(&c).unwrap().peers;
    config::save_state(&d, &st).unwrap();
    start(&d);
    wait_up(&d).await;
    let r = call(&d, json!({"cmd": "send", "text": "intruder"})).await.unwrap();
    assert_eq!(r["results"][0]["ok"], json!(false), "{r}");

    // A peer that pins the wrong fingerprint for A refuses to talk to it.
    let e = node_dir(&root, "e", 49750);
    let mut st = config::load_state(&e).unwrap();
    st.peers.push(config::Peer { name: "fake".into(), fingerprint: "00".repeat(32), addrs: vec!["127.0.0.1:49710".into()], autosync: true, paired_at: 0 });
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
    call(&a, json!({"cmd": "unpair", "peer": b_fp})).await.unwrap();
    let r = call(&b, json!({"cmd": "send", "text": "after unpair"})).await.unwrap();
    assert_eq!(r["results"][0]["ok"], json!(false), "{r}");
    let _ = std::fs::remove_dir_all(&root);
}
