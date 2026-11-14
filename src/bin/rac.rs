//! Command-line control for the Remote Access Core (Bridge).

use anyhow::{Context, Result, bail};
use rac::{config, daemon};
use serde_json::{Value, json};

const HELP: &str = "Remote Access Core (Bridge)

USAGE:
  rac daemon [--no-clipboard]     Run the bridge in this terminal
  rac status                      Show this machine, peers, and recent transfers
  rac pair-code                   Open pairing for 5 minutes and print a one-time code
  rac pair <address> <code>       Pair with a machine showing a code (LAN IP, Tailscale IP or name)
  rac send [--to PEER] [--text TEXT | FILE...]
                                  Send text, files, or (with no input) the current clipboard
  rac ping PEER                   Check that a peer is reachable and still trusted
  rac devices PEER                Read USB metadata after approval on that client
  rac sync on|off [--peer PEER]   Turn automatic clipboard sync on or off
  rac unpair PEER                 Forget a peer (it can no longer connect)
  rac allow PEER clipboard|files|device_inventory|microphone on|off
                                  Grant or revoke a paired device's access on this machine
                                  (microphone: PEER may play into this PC's Steam Streaming Microphone)
  rac request-access PEER [clipboard] [files] [device_inventory] [microphone]
                                  Ask PEER's owner for access (default: both); this machine
                                  allows the same exchange with PEER
  rac approve PEER | rac deny PEER
                                  Answer a pending access request from PEER
  rac access-requests on|off      Let paired devices ask this machine for access
  rac net [--watch]               Wi-Fi signal, band, channel, and advice (--watch: live signal finder)
  rac probe PEER                  Latency and jitter to a peer
  rac speedtest PEER              Two-way throughput and a suggested Moonlight bitrate
  rac moonlight-link on [STATE] | off
                                  Host: let clients Sunshine paired link without a code
                                  (STATE defaults to Sunshine's sunshine_state.json)
  rac host-app-title              Host: the Sunshine app title that publishes this identity
  rac channel on|off NAME         Accept or stop accepting files for an app channel
  rac channel-send --to PEER --channel NAME FILE
                                  Send one file on an app channel (never the clipboard)
  rac mic on PEER [tone]          Stream this machine's default microphone to PEER, which must
                                  allow it (tone: a 440 Hz test tone instead)
  rac mic off                     Turn this machine's microphone stream off now
  rac mic                         Show microphone streams
  rac version

PEER is a peer name or the start of its fingerprint.";

fn main() {
    if let Err(e) = real_main() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn rt() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread().enable_all().build()?)
}

fn call(req: Value) -> Result<Value> {
    let dir = config::home()?;
    rt()?.block_on(daemon::control_call(&dir, req))
}

fn list(v: &Value) -> String {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()
}

fn resolve_peer(q: &str) -> Result<String> {
    let st = call(json!({"cmd": "status"}))?;
    let q = q.to_lowercase();
    let fpq = q.replace('-', "");
    let hits: Vec<String> = st["peers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| {
            p["name"].as_str().map(|n| n.to_lowercase() == q).unwrap_or(false)
                || p["fingerprint"].as_str().map(|f| fpq.len() >= 4 && f.starts_with(&fpq)).unwrap_or(false)
        })
        .filter_map(|p| p["fingerprint"].as_str().map(String::from))
        .collect();
    match hits.len() {
        1 => Ok(hits[0].clone()),
        0 => bail!("no paired peer matches '{q}'"),
        _ => bail!("'{q}' matches more than one peer; use more of the fingerprint"),
    }
}

fn print_mic(m: &Value) {
    if let Some(o) = m["sending"].as_object() {
        println!("MICROPHONE LIVE: {} to {}. Stop it with: rac mic off", o["device"].as_str().unwrap_or("?"), o["peer"].as_str().unwrap_or("?"));
    }
    if let Some(i) = m["receiving"].as_object() {
        println!("Microphone in: {} playing into {}", i["peer"].as_str().unwrap_or("?"), i["device"].as_str().unwrap_or("?"));
    }
}

fn on_off(v: &Value) -> &'static str {
    if v.as_bool() == Some(true) { "on" } else { "off" }
}

fn real_main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    match cmd {
        "daemon" => rac::run_bridge(!args.iter().any(|a| a == "--no-clipboard")),
        "status" => {
            let s = call(json!({"cmd": "status"}))?;
            println!("{} (v{}), identity {}", s["name"].as_str().unwrap_or("?"), s["version"].as_str().unwrap_or("?"), s["short_fingerprint"].as_str().unwrap_or("?"));
            println!("Reachable at: {}", list(&s["addrs"]));
            println!("Automatic sync: {}", on_off(&s["settings"]["autosync"]));
            println!("Access requests: {}", on_off(&s["settings"]["allow_access_requests"]));
            println!("Peers:");
            for p in s["peers"].as_array().into_iter().flatten() {
                println!("  {}  {}  sync {}  clipboard {}  files {}  microphone {}  {}", p["name"].as_str().unwrap_or("?"), p["short_fingerprint"].as_str().unwrap_or(""), on_off(&p["autosync"]),
                    on_off(&p["permissions"]["clipboard"]), on_off(&p["permissions"]["files"]), on_off(&p["permissions"]["microphone"]), list(&p["addrs"]));
            }
            print_mic(&s["microphone"]);
            for r in s["access_requests"].as_array().into_iter().flatten() {
                println!("  PENDING: {} asks for {} access. Run: rac approve \"{}\"", r["name"].as_str().unwrap_or("?"), list(&r["capabilities"]).replace(", ", " and "), r["peer"].as_str().unwrap_or("?"));
            }
            println!("Recent transfers:");
            for e in s["events"].as_array().into_iter().flatten().take(15) {
                let ok = if e["ok"].as_bool() == Some(true) { "ok" } else { "FAILED" };
                println!("  {} {} {} {} bytes {} {}", e["dir"].as_str().unwrap_or(""), e["peer"].as_str().unwrap_or(""), e["kind"].as_str().unwrap_or(""), e["bytes"], ok, e["detail"].as_str().unwrap_or(""));
            }
            Ok(())
        }
        "pair-code" => {
            let r = call(json!({"cmd": "pair_code"}))?;
            println!("Pairing code: {}", r["code"].as_str().unwrap_or("?"));
            println!("Valid for {} seconds, one use, 5 attempts.", r["expires_in"]);
            println!("On the other machine run:  rac pair <one of these addresses> <code>");
            for a in r["addrs"].as_array().into_iter().flatten() {
                println!("  {}", a.as_str().unwrap_or(""));
            }
            Ok(())
        }
        "pair" => {
            let (addr, code) = (args.get(1).context("missing address")?, args.get(2).context("missing code")?);
            let r = call(json!({"cmd": "pair", "addr": addr, "code": code}))?;
            println!("Paired with {} ({})", r["name"].as_str().unwrap_or("?"), r["short_fingerprint"].as_str().unwrap_or(""));
            Ok(())
        }
        "send" => {
            let mut req = json!({"cmd": "send"});
            let mut files = Vec::new();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--to" => {
                        i += 1;
                        req["peer"] = json!(resolve_peer(args.get(i).context("--to needs a peer")?)?);
                    }
                    "--text" => {
                        i += 1;
                        req["text"] = json!(args.get(i).context("--text needs a value")?);
                    }
                    f => files.push(std::fs::canonicalize(f).with_context(|| format!("cannot find {f}"))?.to_string_lossy().to_string()),
                }
                i += 1;
            }
            if !files.is_empty() {
                req["paths"] = json!(files);
            }
            let r = call(req)?;
            let mut failed = false;
            for x in r["results"].as_array().into_iter().flatten() {
                if x["ok"].as_bool() == Some(true) {
                    println!("Delivered to {}", x["peer"].as_str().unwrap_or("?"));
                } else {
                    failed = true;
                    println!("Failed for {}: {}", x["peer"].as_str().unwrap_or("?"), x["error"].as_str().unwrap_or(""));
                }
            }
            if failed {
                bail!("not every peer received it");
            }
            Ok(())
        }
        "ping" => {
            let fp = resolve_peer(args.get(1).context("missing peer")?)?;
            call(json!({"cmd": "ping", "peer": fp}))?;
            println!("Peer reachable and trusted.");
            Ok(())
        }
        "devices" => {
            let fp = resolve_peer(args.get(1).context("missing peer")?)?;
            let report = call(json!({"cmd": "devices", "peer": fp}))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        "sync" => {
            let on = match args.get(1).map(String::as_str) {
                Some("on") => true,
                Some("off") => false,
                _ => bail!("use: rac sync on|off [--peer PEER]"),
            };
            let mut req = json!({"cmd": "set", "autosync": on});
            if args.get(2).map(String::as_str) == Some("--peer") {
                req["peer"] = json!(resolve_peer(args.get(3).context("--peer needs a value")?)?);
            }
            call(req)?;
            println!("Automatic sync {}.", if on { "on" } else { "off" });
            Ok(())
        }
        "unpair" => {
            let fp = resolve_peer(args.get(1).context("missing peer")?)?;
            call(json!({"cmd": "unpair", "peer": fp}))?;
            println!("Peer removed.");
            Ok(())
        }
        "allow" => {
            let fp = resolve_peer(args.get(1).context("missing peer")?)?;
            let capability = args.get(2).context("use: rac allow PEER clipboard|files on|off")?;
            let allowed = match args.get(3).map(String::as_str) {
                Some("on") => true,
                Some("off") => false,
                _ => bail!("use: rac allow PEER clipboard|files on|off"),
            };
            call(json!({"cmd": "permissions", "peer": fp, "capability": capability, "allowed": allowed}))?;
            println!("{capability} access {}.", if allowed { "approved until revoked" } else { "revoked" });
            Ok(())
        }
        "request-access" => {
            let fp = resolve_peer(args.get(1).context("missing peer")?)?;
            let mut caps: Vec<&str> = args[2..].iter().map(String::as_str).collect();
            if caps.is_empty() {
                caps = vec!["clipboard", "files"];
            }
            let r = call(json!({"cmd": "request_access", "peer": fp, "capabilities": caps}))?;
            println!("Asked {} for {} access; its owner approves or denies it there. Device inventory grants remain local to each client.", r["peer"].as_str().unwrap_or("?"), list(&r["requested"]).replace(", ", " and "));
            Ok(())
        }
        "approve" | "deny" => {
            let fp = resolve_peer(args.get(1).context("missing peer")?)?;
            call(json!({"cmd": "access_decision", "peer": fp, "approve": cmd == "approve"}))?;
            println!("Request {}.", if cmd == "approve" { "approved" } else { "denied" });
            Ok(())
        }
        "access-requests" => {
            let on = match args.get(1).map(String::as_str) {
                Some("on") => true,
                Some("off") => false,
                _ => bail!("use: rac access-requests on|off"),
            };
            call(json!({"cmd": "set", "allow_access_requests": on}))?;
            println!("Access requests {}.", if on { "on" } else { "off" });
            Ok(())
        }
        "net" => {
            // Works without the bridge: it only reads local Wi-Fi state.
            let w = rac::netdiag::wifi(true);
            if !w.available {
                bail!("no Wi-Fi information available on this machine");
            }
            let l = &w.link;
            println!("Wi-Fi: {} | {} channel {} | {} | signal {}% | link {:.0}/{:.0} Mbps", if l.connected { "connected" } else { "not connected" }, l.band, l.channel, l.radio, l.signal, l.rx_mbps, l.tx_mbps);
            for p in &w.same_network {
                let busy = p.utilization.map(|u| format!(", {u}% busy")).unwrap_or_default();
                println!("  {} {} channel {} signal {}%{}", if p.current { "*" } else { " " }, p.band, p.channel, p.signal, busy);
            }
            for a in &w.advice {
                println!("[{}] {}", a.level, a.text);
            }
            if args.get(1).map(String::as_str) == Some("--watch") {
                println!("\nSignal finder: walk the device around; Ctrl+C to stop.");
                let mut best = 0u32;
                let mut last = l.signal;
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    let l = rac::netdiag::wifi(false).link;
                    best = best.max(l.signal);
                    let trend = if l.signal > last { "better" } else if l.signal < last { "worse " } else { "same  " };
                    last = l.signal;
                    let bar = "#".repeat((l.signal / 5) as usize);
                    println!("{:>3}% {:<20} {} | best {}% | {} ch {} | {:.0} Mbps", l.signal, bar, trend, best, l.band, l.channel, l.rx_mbps.min(l.tx_mbps));
                }
            }
            Ok(())
        }
        "probe" => {
            let fp = resolve_peer(args.get(1).context("missing peer")?)?;
            let r = call(json!({"cmd": "probe", "peer": fp}))?;
            println!("{} via {}: avg {:.1} ms, min {:.1}, max {:.1}, jitter {:.1} ms, delayed frames {:.0}%", r["peer"].as_str().unwrap_or("?"), r["via"].as_str().unwrap_or("?"), r["avg_ms"].as_f64().unwrap_or(0.0), r["min_ms"].as_f64().unwrap_or(0.0), r["max_ms"].as_f64().unwrap_or(0.0), r["jitter_ms"].as_f64().unwrap_or(0.0), r["spike_pct"].as_f64().unwrap_or(0.0));
            Ok(())
        }
        "speedtest" => {
            let fp = resolve_peer(args.get(1).context("missing peer")?)?;
            let r = call(json!({"cmd": "speedtest", "peer": fp}))?;
            println!("{} via {}: upload {:.0} Mbps, download {:.0} Mbps", r["peer"].as_str().unwrap_or("?"), r["via"].as_str().unwrap_or("?"), r["up_mbps"].as_f64().unwrap_or(0.0), r["down_mbps"].as_f64().unwrap_or(0.0));
            println!("Suggested Moonlight video bitrate: {} Mbps", r["recommended_moonlight_mbps"]);
            Ok(())
        }
        "moonlight-link" => {
            let path = match args.get(1).map(String::as_str) {
                Some("on") => args.get(2).cloned().unwrap_or_else(|| r"C:\Program Files\Sunshine\config\sunshine_state.json".into()),
                Some("off") => String::new(),
                _ => bail!("use: rac moonlight-link on [STATE] | off"),
            };
            call(json!({"cmd": "set", "moonlight_link": path}))?;
            println!("Moonlight link {}.", if path.is_empty() { "off" } else { "on" });
            Ok(())
        }
        "host-app-title" => {
            println!("{}", call(json!({"cmd": "host_app_title"}))?["title"].as_str().unwrap_or(""));
            Ok(())
        }
        "channel" => {
            let (on, name) = match (args.get(1).map(String::as_str), args.get(2)) {
                (Some("on"), Some(n)) => (true, n.clone()),
                (Some("off"), Some(n)) => (false, n.clone()),
                _ => bail!("use: rac channel on|off NAME"),
            };
            let st = call(json!({"cmd": "status"}))?;
            let mut list: Vec<String> = st["settings"]["channels"].as_array().into_iter().flatten().filter_map(|v| v.as_str().map(String::from)).collect();
            list.retain(|c| *c != name);
            if on {
                list.push(name.clone());
            }
            call(json!({"cmd": "set", "channels": list}))?;
            println!("Channel {name} {}.", if on { "on" } else { "off" });
            Ok(())
        }
        "channel-send" => {
            let (mut peer, mut channel, mut file) = (None, None, None);
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--to" => {
                        i += 1;
                        peer = Some(resolve_peer(args.get(i).context("--to needs a peer")?)?);
                    }
                    "--channel" => {
                        i += 1;
                        channel = args.get(i).cloned();
                    }
                    f => file = Some(std::fs::canonicalize(f).with_context(|| format!("cannot find {f}"))?.to_string_lossy().to_string()),
                }
                i += 1;
            }
            let r = call(json!({"cmd": "channel_send", "peer": peer.context("--to is required")?, "channel": channel.context("--channel is required")?, "path": file.context("a file is required")?}))?;
            println!("Delivered {} bytes to {}", r["bytes"], r["peer"].as_str().unwrap_or("?"));
            Ok(())
        }
        "mic" => {
            match args.get(1).map(String::as_str) {
                Some("on") => {
                    let fp = resolve_peer(args.get(2).context("use: rac mic on PEER [tone]")?)?;
                    let r = call(json!({"cmd": "mic_start", "peer": fp, "tone": args.get(3).map(String::as_str) == Some("tone")}))?;
                    println!("MICROPHONE LIVE: {} to {}. Stop it with: rac mic off", r["device"].as_str().unwrap_or("?"), r["peer"].as_str().unwrap_or("?"));
                }
                Some("off") => {
                    let r = call(json!({"cmd": "mic_stop"}))?;
                    println!("{}", if r["stopped"].as_bool() == Some(true) { "Microphone off." } else { "The microphone was not live." });
                }
                None => {
                    let s = call(json!({"cmd": "status"}))?;
                    print_mic(&s["microphone"]);
                    if s["microphone"]["sending"].is_null() && s["microphone"]["receiving"].is_null() {
                        println!("No microphone stream.");
                    }
                }
                _ => bail!("use: rac mic on PEER [tone] | rac mic off | rac mic"),
            }
            Ok(())
        }
        "version" | "--version" => {
            println!("rac {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            println!("{HELP}");
            Ok(())
        }
    }
}
