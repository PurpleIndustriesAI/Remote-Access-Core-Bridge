//! Windowless bridge process. Shows as "Remote Access Crew" with its icon in Task Manager.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    // No console exists on Windows, so keep a small private log next to the settings.
    if let Ok(dir) = rac::config::ensure_home() {
        let log = dir.join("bridge.log");
        if std::fs::metadata(&log).map(|m| m.len() > 2 * 1024 * 1024).unwrap_or(false) {
            let _ = std::fs::rename(&log, dir.join("bridge.old.log"));
        }
        redirect_stderr(&log);
    }
    let clipboard = !std::env::args().any(|a| a == "--no-clipboard");
    if let Err(e) = rac::run_bridge(clipboard) {
        eprintln!("[rac] bridge stopped: {e:#}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn redirect_stderr(path: &std::path::Path) {
    use std::os::windows::io::IntoRawHandle;
    unsafe extern "system" {
        fn SetStdHandle(n: u32, h: *mut core::ffi::c_void) -> i32;
    }
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        // STD_ERROR_HANDLE is (DWORD)-12.
        unsafe { SetStdHandle((-12i32) as u32, f.into_raw_handle() as _) };
    }
}

#[cfg(not(windows))]
fn redirect_stderr(_: &std::path::Path) {
    // systemd and the Crew backend capture stderr already.
}
