//! Command-line control for the Remote Access Crew bridge.

use anyhow::{Context, Result, bail};
use rac::{config, daemon};
use serde_json::{Value, json};

const HELP: &str = "Remote Access Crew

USAGE:
  rac daemon [--no-clipboard]     Run the bridge in this terminal
  rac status                      Show this machine, peers, and recent transfers
  rac pair-code                   Open pairing for 5 minutes and print a one-time code
  rac pair <address> <code>       Pair with a machine showing a code (LAN IP, Tailscale IP or name)
  rac send [--to PEER] [--text TEXT | FILE...]
                                  Send text, files, or (with no input) the current clipboard
  rac ping PEER                   Check that a peer is reachable and still trusted
  rac sync on|off [--peer PEER]   Turn automatic clipboard sync on or off
  rac unpair PEER                 Forget a peer (it can no longer connect)
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
            println!("Peers:");
            for p in s["peers"].as_array().into_iter().flatten() {
                println!("  {}  {}  sync {}  {}", p["name"].as_str().unwrap_or("?"), p["short_fingerprint"].as_str().unwrap_or(""), on_off(&p["autosync"]), list(&p["addrs"]));
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
