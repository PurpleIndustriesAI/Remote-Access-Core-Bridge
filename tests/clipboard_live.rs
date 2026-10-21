//! Live system-clipboard test. It overwrites the clipboard, so it only runs on request:
//!   cargo test --test clipboard_live -- --ignored --test-threads=1
//! CI runs it on Linux under Xvfb and on Windows.

use rac::clip;
use rac::config::{self, Settings, State};
use rac::daemon::{self, control_call};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn node_dir(root: &Path, name: &str, port: u16) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let st = State { peers: vec![], settings: Settings { port, receive_dir: Some(dir.join("inbox")), ..Settings::default() } };
    config::save_state(&dir, &st).unwrap();
    dir
}

async fn call(dir: &Path, v: Value) -> Value {
    control_call(dir, v).await.unwrap()
}

async fn start(dir: &Path, clipboard: bool) {
    let d = dir.to_path_buf();
    tokio::spawn(async move {
        if let Err(e) = daemon::run(daemon::Options { dir: d, clipboard }).await {
            panic!("bridge failed: {e:#}");
        }
    });
    for _ in 0..100 {
        if control_call(dir, json!({"cmd": "status"})).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("bridge did not start");
}

fn incoming(status: &Value, kind: &str) -> usize {
    status["events"].as_array().unwrap().iter().filter(|e| e["dir"] == "in" && e["kind"] == kind && e["ok"] == true).count()
}

async fn wait_for(dir: &Path, kind: &str, n: usize) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(8) {
        if incoming(&call(dir, json!({"cmd": "status"})).await, kind) >= n {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

fn cb() -> arboard::Clipboard {
    arboard::Clipboard::new().expect("clipboard")
}

async fn eventually<F: FnMut() -> bool>(mut f: F) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn clipboard_both_directions() {
    let root = std::env::temp_dir().join(format!("rac-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let saved = cb().get_text().ok();

    // A uses the real clipboard. B is a headless peer that records what it receives.
    let a = node_dir(&root, "a", 49810);
    let b = node_dir(&root, "b", 49820);
    start(&a, true).await;
    start(&b, false).await;
    let code = call(&a, json!({"cmd": "pair_code"})).await["code"].as_str().unwrap().to_string();
    call(&b, json!({"cmd": "pair", "addr": "127.0.0.1:49810", "code": code})).await;

    // Peer -> clipboard: text.
    call(&b, json!({"cmd": "send", "text": "Remote Access Crew text check"})).await;
    assert!(eventually(|| cb().get_text().ok().as_deref() == Some("Remote Access Crew text check")).await, "text did not reach the clipboard");

    // Peer -> clipboard: image.
    let rgba: Vec<u8> = (0..(40 * 30)).flat_map(|i| [(i % 251) as u8, 80, 200, 255]).collect();
    let png = clip::encode_png(40, 30, &rgba).unwrap();
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    rac_send_image(&b, &png).await;
    assert!(
        eventually(|| cb().get_image().map(|i| i.width == 40 && i.height == 30 && i.bytes[..4] == rgba[..4]).unwrap_or(false)).await,
        "image did not reach the clipboard"
    );

    // Peer -> clipboard: files.
    std::fs::write(src.join("notes.txt"), b"file check").unwrap();
    call(&b, json!({"cmd": "send", "paths": [src.join("notes.txt")]})).await;
    let expect = a.join("inbox").join("notes.txt");
    assert!(
        eventually(|| cb().get().file_list().map(|l| l.iter().any(|p| p.file_name() == expect.file_name())).unwrap_or(false)).await,
        "file list did not reach the clipboard"
    );
    assert_eq!(std::fs::read(&expect).unwrap(), b"file check");

    // Nothing that arrived was echoed back to B.
    tokio::time::sleep(Duration::from_millis(1800)).await;
    let s = call(&b, json!({"cmd": "status"})).await;
    assert_eq!(incoming(&s, "text") + incoming(&s, "image") + incoming(&s, "files"), 0, "echo detected: {s}");

    // Clipboard -> peer: text, image, files.
    cb().set_text("copied on A").unwrap();
    assert!(wait_for(&b, "text", 1).await, "local text copy was not sent");
    let rgba2: Vec<u8> = (0..(16 * 16)).flat_map(|i| [10, (i % 200) as u8, 30, 255]).collect();
    cb().set_image(arboard::ImageData { width: 16, height: 16, bytes: Cow::Owned(rgba2) }).unwrap();
    assert!(wait_for(&b, "image", 1).await, "local image copy was not sent");
    std::fs::write(src.join("report.csv"), b"a,b\n1,2\n").unwrap();
    cb().set().file_list(&[src.join("report.csv")]).unwrap();
    assert!(wait_for(&b, "files", 1).await, "local file copy was not sent");
    assert_eq!(std::fs::read(b.join("inbox").join("report.csv")).unwrap(), b"a,b\n1,2\n");

    // Content marked private by a password manager is never sent (Windows marks it this way).
    #[cfg(windows)]
    {
        use arboard::SetExtWindows;
        cb().set().exclude_from_monitoring().text("hunter2-secret").unwrap();
        tokio::time::sleep(Duration::from_millis(2500)).await;
        let s = call(&b, json!({"cmd": "status"})).await;
        assert_eq!(incoming(&s, "text"), 1, "a private clipboard item was sent");
    }

    if let Some(t) = saved {
        let _ = cb().set_text(t);
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Send raw PNG bytes from B as an image message (B has no clipboard to read from).
async fn rac_send_image(b: &Path, png: &[u8]) {
    let r = control_call(b, json!({"cmd": "send", "image_png_b64": data_encoding::BASE64.encode(png)})).await.unwrap();
    assert_eq!(r["results"][0]["ok"], json!(true), "{r}");
}
