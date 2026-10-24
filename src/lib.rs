//! Remote Access Core (Bridge): secure two-way clipboard, image, and file sharing between paired
//! machines over a LAN or Tailscale.

pub mod clip;
pub mod config;
pub mod daemon;
pub mod net;
pub mod netdiag;
pub mod proto;
pub mod tls;

/// Run the bridge until the process exits.
pub fn run_bridge(clipboard: bool) -> anyhow::Result<()> {
    let dir = config::ensure_home()?;
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    rt.block_on(daemon::run(daemon::Options { dir, clipboard }))
}
