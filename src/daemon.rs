//! The bridge: peer listener, pairing, transfers, and the local control API.

use crate::clip::{self, ClipHandle, Content};
use crate::config::{self, Identity, Peer, State, short_fingerprint};
use crate::net;
use crate::proto::{self, FileEntry, Msg, Reply};
use crate::tls::{self, Trust};
use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

const PAIR_WINDOW: Duration = Duration::from_secs(300);
const PAIR_ATTEMPTS: u32 = 5;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const IO_TIMEOUT: Duration = Duration::from_secs(30);
const COPY_BUF: usize = 256 * 1024;

pub struct Options {
    pub dir: PathBuf,
    /// Read and write the system clipboard. Off for headless tests.
    pub clipboard: bool,
}

struct Pairing {
    code: String,
    until: Instant,
    attempts: u32,
}

#[derive(Clone, serde::Serialize)]
struct Event {
    ts: u64,
    dir: &'static str,
    peer: String,
    kind: String,
    bytes: u64,
    ok: bool,
    detail: String,
}

pub struct Node {
    dir: PathBuf,
    id: Identity,
    name: String,
    trust: Arc<Trust>,
    state: Mutex<State>,
    pairing: Mutex<Option<Pairing>>,
    events: Mutex<VecDeque<Event>>,
    clip: Option<ClipHandle>,
    acceptor: TlsAcceptor,
    send_lock: tokio::sync::Mutex<()>,
    token: String,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Node {
    fn port(&self) -> u16 {
        self.state.lock().unwrap().settings.port
    }

    fn event(&self, dir: &'static str, peer: &str, kind: &str, bytes: u64, ok: bool, detail: impl Into<String>) {
        let e = Event { ts: now(), dir, peer: peer.to_string(), kind: kind.to_string(), bytes, ok, detail: detail.into() };
        eprintln!("[rac] {} {} {} {} bytes ok={} {}", e.dir, e.peer, e.kind, e.bytes, e.ok, e.detail);
        let mut q = self.events.lock().unwrap();
        q.push_front(e);
        q.truncate(200);
    }

    fn save(&self) -> Result<()> {
        let s = self.state.lock().unwrap().clone();
        config::save_state(&self.dir, &s)
    }

    fn peer(&self, fp: &str) -> Option<Peer> {
        self.state.lock().unwrap().peers.iter().find(|p| p.fingerprint == fp).cloned()
    }

    fn peer_name(&self, fp: &str) -> String {
        self.peer(fp).map(|p| p.name).unwrap_or_else(|| short_fingerprint(fp))
    }

    fn sync_trust(&self) {
        let fps: HashSet<String> = self.state.lock().unwrap().peers.iter().map(|p| p.fingerprint.clone()).collect();
        *self.trust.peers.write().unwrap() = fps;
    }

    /// Remember an address a peer reached us from, keeping its listening port.
    fn learn_addr(&self, fp: &str, ip: std::net::IpAddr) {
        let mut st = self.state.lock().unwrap();
        if let Some(p) = st.peers.iter_mut().find(|p| p.fingerprint == fp) {
            let port = p.addrs.first().and_then(|a| a.parse::<SocketAddr>().ok()).map(|a| a.port()).unwrap_or(config::DEFAULT_PORT);
            let a = SocketAddr::new(ip, port).to_string();
            if !p.addrs.contains(&a) {
                p.addrs.insert(0, a);
                p.addrs.truncate(8);
                drop(st);
                let _ = self.save();
            }
        }
    }
}

pub async fn run(opts: Options) -> Result<()> {
    let dir = opts.dir.clone();
    std::fs::create_dir_all(&dir)?;
    let id = config::load_or_create_identity(&dir)?;
    let state = config::load_state(&dir)?;
    let token = config::control_token(&dir)?;
    let port = state.settings.port;

    // Bind the control port first: it doubles as the single-instance lock.
    let control = TcpListener::bind(("127.0.0.1", port + config::CONTROL_PORT_OFFSET))
        .await
        .with_context(|| format!("control port {} is busy; is the bridge already running?", port + 1))?;
    let v4 = TcpListener::bind(("0.0.0.0", port)).await.with_context(|| format!("peer port {port} is busy"))?;
    let v6 = TcpListener::bind(("::", port)).await.ok();

    let trust = Arc::new(Trust::default());
    let acceptor = TlsAcceptor::from(tls::server_config(&id, trust.clone())?);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let clip = if opts.clipboard { Some(clip::start(true, tx)?) } else { None };

    let node = Arc::new(Node {
        dir,
        name: config::hostname(),
        id,
        trust,
        state: Mutex::new(state),
        pairing: Mutex::new(None),
        events: Mutex::new(VecDeque::new()),
        clip,
        acceptor,
        send_lock: tokio::sync::Mutex::new(()),
        token,
    });
    node.sync_trust();
    eprintln!("[rac] Remote Access Crew {} listening on port {port}; identity {}", env!("CARGO_PKG_VERSION"), short_fingerprint(&node.id.fingerprint));

    // Local clipboard changes -> peers.
    let n = node.clone();
    tokio::spawn(async move {
        while let Some(c) = rx.recv().await {
            if !n.state.lock().unwrap().settings.autosync {
                continue;
            }
            let n2 = n.clone();
            tokio::spawn(async move {
                let _ = send_to(&n2, None, c, false).await;
            });
        }
    });

    for l in std::iter::once(v4).chain(v6) {
        let n = node.clone();
        tokio::spawn(async move { accept_loop(n, l).await });
    }
    control_loop(node, control).await
}

async fn accept_loop(node: Arc<Node>, l: TcpListener) {
    // Bound concurrent inbound connections so a noisy LAN device cannot exhaust the bridge.
    let slots = Arc::new(tokio::sync::Semaphore::new(64));
    loop {
        let Ok((sock, addr)) = l.accept().await else { continue };
        if !net::is_allowed_source(&addr.ip()) {
            // Outside LAN/Tailscale ranges: close before any TLS work.
            continue;
        }
        let Ok(permit) = slots.clone().try_acquire_owned() else { continue };
        let n = node.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(e) = handle_peer(n.clone(), sock, addr).await {
                eprintln!("[rac] connection from {addr} rejected: {e:#}");
            }
        });
    }
}

async fn handle_peer(node: Arc<Node>, sock: TcpStream, addr: SocketAddr) -> Result<()> {
    let _ = sock.set_nodelay(true);
    let mut tls = tokio::time::timeout(IO_TIMEOUT, node.acceptor.accept(sock)).await??;
    let fp = {
        let (_, conn) = tls.get_ref();
        let c = conn.peer_certificates().and_then(|c| c.first()).context("no client certificate")?;
        config::fingerprint(c)
    };
    let msg: Msg = tokio::time::timeout(IO_TIMEOUT, proto::read_json(&mut tls)).await??;
    let known = node.trust.is_peer(&fp);
    if let Msg::Pair { proof, name, addrs } = msg {
        let reply = accept_pairing(&node, &fp, addr, &proof, &name, addrs);
        proto::write_json(&mut tls, &reply).await?;
        tls.shutdown().await.ok();
        return Ok(());
    }
    if !known {
        proto::write_json(&mut tls, &Reply::err("not paired")).await?;
        bail!("unpaired certificate tried {}", short_fingerprint(&fp));
    }
    node.learn_addr(&fp, addr.ip());
    let peer = node.peer_name(&fp);
    let result = receive(&node, &mut tls, msg, &peer).await;
    let reply = match &result {
        Ok(()) => Reply::ok(),
        Err(e) => Reply::err(format!("{e:#}")),
    };
    proto::write_json(&mut tls, &reply).await.ok();
    tls.shutdown().await.ok();
    result
}

fn accept_pairing(node: &Node, client_fp: &str, addr: SocketAddr, proof: &str, name: &str, addrs: Vec<String>) -> Reply {
    let mut guard = node.pairing.lock().unwrap();
    let Some(p) = guard.as_mut().filter(|p| p.until > Instant::now()) else {
        *guard = None;
        *node.trust.pairing_until.lock().unwrap() = None;
        return Reply::err("pairing is not open on this machine");
    };
    p.attempts += 1;
    let expected = proto::pair_proof(&p.code, "client", &node.id.fingerprint, client_fp);
    if !proto::proof_matches(&expected, proof) {
        let left = PAIR_ATTEMPTS.saturating_sub(p.attempts);
        if left == 0 {
            *guard = None;
            *node.trust.pairing_until.lock().unwrap() = None;
        }
        node.event("in", &short_fingerprint(client_fp), "pair", 0, false, "wrong pairing code");
        return Reply::err("wrong pairing code");
    }
    let server_proof = proto::pair_proof(&p.code, "server", &node.id.fingerprint, client_fp);
    *guard = None;
    *node.trust.pairing_until.lock().unwrap() = None;
    drop(guard);

    let port = addrs.first().and_then(|a| a.parse::<SocketAddr>().ok()).map(|a| a.port()).unwrap_or(config::DEFAULT_PORT);
    let mut list = vec![SocketAddr::new(addr.ip(), port).to_string()];
    for a in addrs.into_iter().filter(|a| a.parse::<SocketAddr>().map(|s| net::is_allowed_source(&s.ip())).unwrap_or(false)) {
        if !list.contains(&a) {
            list.push(a);
        }
    }
    list.truncate(8);
    let name = clean_name(name);
    upsert_peer(node, Peer { name: name.clone(), fingerprint: client_fp.to_string(), addrs: list, autosync: true, paired_at: now() });
    node.event("in", &name, "pair", 0, true, "paired");
    Reply { ok: true, proof: Some(server_proof), name: Some(node.name.clone()), addrs: net::local_addrs(node.port()), error: None }
}

fn clean_name(n: &str) -> String {
    let s: String = n.chars().filter(|c| c.is_ascii_alphanumeric() || "-_ .".contains(*c)).take(48).collect();
    if s.trim().is_empty() { "peer".into() } else { s.trim().to_string() }
}

fn upsert_peer(node: &Node, peer: Peer) {
    {
        let mut st = node.state.lock().unwrap();
        st.peers.retain(|p| p.fingerprint != peer.fingerprint);
        st.peers.push(peer);
    }
    node.sync_trust();
    let _ = node.save();
}

async fn read_body<R: AsyncRead + Unpin>(r: &mut R, len: u64) -> Result<Vec<u8>> {
    let mut b = vec![0u8; len as usize];
    tokio::time::timeout(IO_TIMEOUT * 4, r.read_exact(&mut b)).await??;
    Ok(b)
}

async fn receive<S: AsyncRead + AsyncWrite + Unpin>(node: &Node, s: &mut S, msg: Msg, peer: &str) -> Result<()> {
    let settings = node.state.lock().unwrap().settings.clone();
    match msg {
        Msg::Ping => Ok(()),
        Msg::Pair { .. } => unreachable!(),
        Msg::Text { len } => {
            if len > settings.max_text_bytes {
                bail!("text is larger than the receiver's limit");
            }
            let b = read_body(s, len).await?;
            let t = String::from_utf8(b).context("text was not valid UTF-8")?;
            apply(node, Content::Text(t)).await?;
            node.event("in", peer, "text", len, true, "copied to clipboard");
            Ok(())
        }
        Msg::Image { len } => {
            if len > settings.max_image_bytes {
                bail!("image is larger than the receiver's limit");
            }
            let b = read_body(s, len).await?;
            clip::decode_png(&b)?; // validate before touching the clipboard
            apply(node, Content::Image(b)).await?;
            node.event("in", peer, "image", len, true, "copied to clipboard");
            Ok(())
        }
        Msg::Files { files } => {
            let total = receive_files(node, s, files, &settings, peer).await;
            match total {
                Ok((bytes, roots)) => {
                    let n = roots.len();
                    apply(node, Content::Files(roots)).await?;
                    node.event("in", peer, "files", bytes, true, format!("{n} item(s) saved and copied to clipboard"));
                    Ok(())
                }
                Err(e) => {
                    node.event("in", peer, "files", 0, false, format!("{e:#}"));
                    Err(e)
                }
            }
        }
    }
}

async fn apply(node: &Node, c: Content) -> Result<()> {
    match &node.clip {
        Some(h) => {
            let h = h.clone();
            tokio::task::spawn_blocking(move || h.apply(c)).await?
        }
        None => Ok(()),
    }
}

async fn receive_files<R: AsyncRead + Unpin>(node: &Node, r: &mut R, files: Vec<FileEntry>, st: &config::Settings, _peer: &str) -> Result<(u64, Vec<PathBuf>)> {
    if files.is_empty() || files.len() > proto::MAX_FILES {
        bail!("bad file count");
    }
    let total: u64 = files.iter().map(|f| f.len).try_fold(0u64, |a, b| a.checked_add(b)).context("size overflow")?;
    if total > st.max_batch_bytes {
        bail!("files are larger than the receiver's limit");
    }
    let base = config::receive_dir(st)?;
    std::fs::create_dir_all(&base)?;
    let _ = node;

    // Validate every path before writing anything, and map each top-level name to a fresh one.
    let mut plan = Vec::with_capacity(files.len());
    let mut roots: Vec<(String, PathBuf)> = Vec::new();
    let mut seen = HashSet::new();
    for f in &files {
        let segs = proto::safe_relative(&f.path)?;
        if f.sha256.len() != 64 || !f.sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
            bail!("bad checksum format");
        }
        let root = match roots.iter().find(|(n, _)| *n == segs[0]) {
            Some((_, p)) => p.clone(),
            None => {
                let p = proto::unique_child(&base, &segs[0]);
                // Reserve the name so a second root with the same name gets its own suffix.
                roots.push((segs[0].clone(), p.clone()));
                p
            }
        };
        let mut dest = root.clone();
        for s in &segs[1..] {
            dest.push(s);
        }
        if !seen.insert(dest.clone()) {
            bail!("duplicate file path in batch");
        }
        plan.push((dest, f));
    }

    let mut written: Vec<PathBuf> = Vec::new();
    let res = async {
        let mut buf = vec![0u8; COPY_BUF];
        for (dest, f) in &plan {
            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let part = part_path(dest);
            let mut out = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&part).await?;
            written.push(part.clone());
            let mut h = Sha256::new();
            let mut left = f.len;
            while left > 0 {
                let n = (left as usize).min(buf.len());
                tokio::time::timeout(IO_TIMEOUT, r.read_exact(&mut buf[..n])).await??;
                h.update(&buf[..n]);
                out.write_all(&buf[..n]).await?;
                left -= n as u64;
            }
            out.flush().await?;
            drop(out);
            if hex::encode(h.finalize()) != f.sha256.to_ascii_lowercase() {
                bail!("checksum mismatch; transfer discarded");
            }
            if dest.exists() {
                bail!("destination appeared during transfer");
            }
            tokio::fs::rename(&part, dest).await?;
            written.pop();
            written.push(dest.clone());
            mark_downloaded(dest);
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(e) = res {
        for p in written.iter().rev() {
            let _ = std::fs::remove_file(p);
        }
        for (_, root) in &roots {
            if root.is_dir() {
                let _ = remove_empty_dirs(root);
            }
        }
        return Err(e);
    }
    Ok((total, roots.into_iter().map(|(_, p)| p).collect()))
}

fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".rac-part");
    PathBuf::from(s)
}

fn remove_empty_dirs(p: &Path) -> std::io::Result<()> {
    for e in std::fs::read_dir(p)? {
        let e = e?;
        if e.file_type()?.is_dir() {
            let _ = remove_empty_dirs(&e.path());
        }
    }
    std::fs::remove_dir(p)
}

/// Windows: tag received files as downloaded from another computer (Mark of the Web), so
/// SmartScreen and Office Protected View apply. No effect elsewhere.
fn mark_downloaded(p: &Path) {
    #[cfg(windows)]
    {
        let mut ads = p.as_os_str().to_owned();
        ads.push(":Zone.Identifier");
        let _ = std::fs::write(PathBuf::from(ads), b"[ZoneTransfer]\r\nZoneId=3\r\n");
    }
    #[cfg(not(windows))]
    let _ = p;
}

// ---------------------------------------------------------------- sending

async fn connect(node: &Node, peer: &Peer) -> Result<(tokio_rustls::client::TlsStream<TcpStream>, String)> {
    let cfg = tls::client_config(&node.id, Some(peer.fingerprint.clone()))?;
    let connector = TlsConnector::from(cfg);
    let mut last = anyhow!("no address known for {}", peer.name);
    for a in &peer.addrs {
        let Ok(sa) = a.parse::<SocketAddr>() else { continue };
        if !net::is_allowed_source(&sa.ip()) {
            continue;
        }
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(sa)).await {
            Ok(Ok(sock)) => {
                let _ = sock.set_nodelay(true);
                match tokio::time::timeout(IO_TIMEOUT, connector.connect(tls::server_name(), sock)).await {
                    Ok(Ok(t)) => return Ok((t, a.clone())),
                    Ok(Err(e)) => last = anyhow!("{a}: TLS failed ({e}); the peer may have been reset or replaced"),
                    Err(_) => last = anyhow!("{a}: TLS timed out"),
                }
            }
            Ok(Err(e)) => last = anyhow!("{a}: {e}"),
            Err(_) => last = anyhow!("{a}: timed out"),
        }
    }
    Err(last)
}

fn promote_addr(node: &Node, fp: &str, a: &str) {
    let mut st = node.state.lock().unwrap();
    if let Some(p) = st.peers.iter_mut().find(|p| p.fingerprint == fp) {
        if p.addrs.first().map(|s| s.as_str()) != Some(a) {
            p.addrs.retain(|x| x != a);
            p.addrs.insert(0, a.to_string());
            drop(st);
            let _ = node.save();
        }
    }
}

struct Outgoing {
    msg: Msg,
    body: Body,
    bytes: u64,
}

enum Body {
    None,
    Bytes(Vec<u8>),
    Files(Vec<PathBuf>),
}

/// Collect files (folders are walked; symlinks are skipped) and hash them.
fn prepare_files(paths: &[PathBuf], limit: u64) -> Result<(Vec<FileEntry>, Vec<PathBuf>, u64)> {
    let mut entries = Vec::new();
    let mut locals = Vec::new();
    let mut total = 0u64;
    fn walk(base: &Path, p: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
        let md = std::fs::symlink_metadata(p)?;
        if md.file_type().is_symlink() {
            return Ok(());
        }
        if md.is_dir() {
            let mut kids: Vec<_> = std::fs::read_dir(p)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            kids.sort();
            for k in kids {
                walk(base, &k, out)?;
            }
        } else if md.is_file() {
            let rel = proto::rel_path(base, p).context("unexpected path")?;
            out.push((rel, p.to_path_buf()));
        }
        if out.len() > proto::MAX_FILES {
            bail!("too many files");
        }
        Ok(())
    }
    let mut list = Vec::new();
    for p in paths {
        let base = p.parent().unwrap_or(Path::new(""));
        walk(base, p, &mut list)?;
    }
    if list.is_empty() {
        bail!("nothing to send (empty folders and links are skipped)");
    }
    for (_, p) in &list {
        total += std::fs::metadata(p)?.len();
        if total > limit {
            bail!("files exceed the size limit for this send");
        }
    }
    for (rel, p) in list {
        let mut f = std::fs::File::open(&p)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; COPY_BUF];
        let mut len = 0u64;
        loop {
            let n = std::io::Read::read(&mut f, &mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            len += n as u64;
        }
        entries.push(FileEntry { path: rel, len, sha256: hex::encode(h.finalize()) });
        locals.push(p);
    }
    Ok((entries, locals, total))
}

fn outgoing(c: Content, limit: u64) -> Result<Outgoing> {
    Ok(match c {
        Content::Text(t) => {
            let b = t.into_bytes();
            Outgoing { msg: Msg::Text { len: b.len() as u64 }, bytes: b.len() as u64, body: Body::Bytes(b) }
        }
        Content::Image(png) => Outgoing { msg: Msg::Image { len: png.len() as u64 }, bytes: png.len() as u64, body: Body::Bytes(png) },
        Content::Files(paths) => {
            let (entries, locals, total) = prepare_files(&paths, limit)?;
            Outgoing { msg: Msg::Files { files: entries }, bytes: total, body: Body::Files(locals) }
        }
    })
}

async fn deliver(node: &Node, peer: &Peer, out: &Outgoing) -> Result<()> {
    let (mut t, addr) = connect(node, peer).await?;
    proto::write_json(&mut t, &out.msg).await?;
    match &out.body {
        Body::None => {}
        Body::Bytes(b) => t.write_all(b).await?,
        Body::Files(paths) => {
            let Msg::Files { files } = &out.msg else { unreachable!() };
            let mut buf = vec![0u8; COPY_BUF];
            for (p, e) in paths.iter().zip(files) {
                let mut f = tokio::fs::File::open(p).await?;
                let mut left = e.len;
                while left > 0 {
                    let n = (left as usize).min(buf.len());
                    f.read_exact(&mut buf[..n]).await.context("a file changed while it was being sent")?;
                    t.write_all(&buf[..n]).await?;
                    left -= n as u64;
                }
            }
        }
    }
    t.flush().await?;
    let wait = IO_TIMEOUT + Duration::from_secs(out.bytes / (5 * 1024 * 1024));
    let reply: Reply = tokio::time::timeout(wait, proto::read_json(&mut t)).await??;
    t.shutdown().await.ok();
    if !reply.ok {
        bail!("{} declined: {}", peer.name, reply.error.unwrap_or_default());
    }
    promote_addr(node, &peer.fingerprint, &addr);
    Ok(())
}

/// Send content to one peer or, when `target` is None, to every peer with sync on.
async fn send_to(node: &Arc<Node>, target: Option<String>, c: Content, explicit: bool) -> Result<Vec<Value>> {
    let _g = node.send_lock.lock().await;
    let (peers, st) = {
        let s = node.state.lock().unwrap();
        (s.peers.clone(), s.settings.clone())
    };
    let peers: Vec<Peer> = match &target {
        Some(fp) => peers.into_iter().filter(|p| &p.fingerprint == fp).collect(),
        None => peers.into_iter().filter(|p| explicit || p.autosync).collect(),
    };
    if peers.is_empty() {
        bail!(if target.is_some() { "unknown peer" } else { "no paired peers" });
    }
    let kind = c.kind();
    let limit = if explicit { st.max_batch_bytes } else { st.auto_file_limit };
    let c2 = c.clone();
    let out = match tokio::task::spawn_blocking(move || outgoing(c2, limit)).await? {
        Ok(o) => o,
        Err(e) => {
            let detail = if !explicit && kind == "files" { format!("{e:#}; use Send files for large items") } else { format!("{e:#}") };
            node.event("out", "all", kind, 0, false, detail.clone());
            bail!(detail);
        }
    };
    let mut results = Vec::new();
    for p in &peers {
        let r = deliver(node, p, &out).await;
        match &r {
            Ok(()) => node.event("out", &p.name, kind, out.bytes, true, "delivered"),
            Err(e) => node.event("out", &p.name, kind, out.bytes, false, format!("{e:#}")),
        }
        results.push(json!({"peer": p.name, "fingerprint": p.fingerprint, "ok": r.is_ok(), "error": r.err().map(|e| format!("{e:#}"))}));
    }
    Ok(results)
}

async fn ping(node: &Node, fp: &str) -> Result<()> {
    let peer = node.peer(fp).context("unknown peer")?;
    deliver(node, &peer, &Outgoing { msg: Msg::Ping, body: Body::None, bytes: 0 }).await
}

async fn pair_with(node: &Node, target: &str, code: &str) -> Result<Peer> {
    if proto::normalize_code(code).len() != 16 {
        bail!("pairing codes have 16 letters and digits");
    }
    let target = if target.contains(':') && (target.contains(']') || target.matches(':').count() == 1) {
        target.to_string()
    } else if target.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{target}]:{}", config::DEFAULT_PORT)
    } else {
        format!("{target}:{}", config::DEFAULT_PORT)
    };
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host(&target).await.context("cannot resolve the peer address")?.collect();
    let sa = addrs.into_iter().find(|a| net::is_allowed_source(&a.ip())).context("the address is not on your LAN or Tailscale network")?;
    let cfg = tls::client_config(&node.id, None)?;
    let sock = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(sa)).await.context("connection timed out")??;
    const NOT_OPEN: &str = "the other machine refused; open pairing there first (Pair a device, or rac pair-code) and use its current code";
    let mut t = tokio::time::timeout(IO_TIMEOUT, TlsConnector::from(cfg).connect(tls::server_name(), sock)).await?.context(NOT_OPEN)?;
    let server_fp = {
        let (_, c) = t.get_ref();
        config::fingerprint(c.peer_certificates().and_then(|c| c.first()).context("no server certificate")?)
    };
    let proof = proto::pair_proof(code, "client", &server_fp, &node.id.fingerprint);
    proto::write_json(&mut t, &Msg::Pair { proof, name: node.name.clone(), addrs: net::local_addrs(node.port()) }).await.context(NOT_OPEN)?;
    let reply: Reply = tokio::time::timeout(IO_TIMEOUT, proto::read_json(&mut t)).await?.context(NOT_OPEN)?;
    if !reply.ok {
        bail!("pairing refused: {}", reply.error.unwrap_or_default());
    }
    let expected = proto::pair_proof(code, "server", &server_fp, &node.id.fingerprint);
    if !proto::proof_matches(&expected, reply.proof.as_deref().unwrap_or("")) {
        bail!("the other machine could not prove it knows the code; pairing stopped");
    }
    let mut list = vec![sa.to_string()];
    for a in reply.addrs {
        if a.parse::<SocketAddr>().map(|s| net::is_allowed_source(&s.ip())).unwrap_or(false) && !list.contains(&a) {
            list.push(a);
        }
    }
    list.truncate(8);
    let peer = Peer { name: clean_name(reply.name.as_deref().unwrap_or("peer")), fingerprint: server_fp, addrs: list, autosync: true, paired_at: now() };
    upsert_peer(node, peer.clone());
    node.event("out", &peer.name, "pair", 0, true, "paired");
    Ok(peer)
}

// ---------------------------------------------------------------- control API

async fn control_loop(node: Arc<Node>, l: TcpListener) -> Result<()> {
    loop {
        let (sock, addr) = l.accept().await?;
        if !addr.ip().is_loopback() {
            continue;
        }
        let n = node.clone();
        tokio::spawn(async move {
            let (r, mut w) = sock.into_split();
            let mut line = String::new();
            // Large enough for a pasted screenshot sent as base64 from the Crew app.
            let mut r = BufReader::new(r.take(192 * 1024 * 1024));
            if tokio::time::timeout(IO_TIMEOUT, r.read_line(&mut line)).await.is_err() {
                return;
            }
            let resp = match serde_json::from_str::<Value>(&line) {
                Ok(req) => {
                    let tok = req.get("token").and_then(|t| t.as_str()).unwrap_or("");
                    if !proto::proof_matches(&n.token, tok) {
                        json!({"ok": false, "error": "bad control token"})
                    } else {
                        match control(&n, &req).await {
                            Ok(v) => json!({"ok": true, "result": v}),
                            Err(e) => json!({"ok": false, "error": format!("{e:#}")}),
                        }
                    }
                }
                Err(_) => json!({"ok": false, "error": "bad request"}),
            };
            let mut s = resp.to_string();
            s.push('\n');
            let _ = w.write_all(s.as_bytes()).await;
        });
    }
}

fn status(node: &Node) -> Value {
    let st = node.state.lock().unwrap().clone();
    let pairing = node.pairing.lock().unwrap();
    let pair = match pairing.as_ref().filter(|p| p.until > Instant::now()) {
        Some(p) => json!({"open": true, "code": p.code, "expires_in": p.until.saturating_duration_since(Instant::now()).as_secs()}),
        None => json!({"open": false}),
    };
    let events: Vec<Event> = node.events.lock().unwrap().iter().take(60).cloned().collect();
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "name": node.name,
        "fingerprint": node.id.fingerprint,
        "short_fingerprint": short_fingerprint(&node.id.fingerprint),
        "port": st.settings.port,
        "addrs": net::local_addrs(st.settings.port),
        "clipboard": node.clip.is_some(),
        "settings": st.settings,
        "receive_dir": config::receive_dir(&st.settings).ok(),
        "peers": st.peers.iter().map(|p| json!({
            "name": p.name, "fingerprint": p.fingerprint, "short_fingerprint": short_fingerprint(&p.fingerprint),
            "addrs": p.addrs, "autosync": p.autosync, "paired_at": p.paired_at,
            "tailscale": p.addrs.iter().any(|a| a.parse::<SocketAddr>().map(|s| net::is_tailscale(&s.ip())).unwrap_or(false)),
        })).collect::<Vec<_>>(),
        "pairing": pair,
        "events": events,
    })
}

fn str_arg<'a>(req: &'a Value, k: &str) -> Result<&'a str> {
    req.get(k).and_then(|v| v.as_str()).with_context(|| format!("missing '{k}'"))
}

async fn control(node: &Arc<Node>, req: &Value) -> Result<Value> {
    match str_arg(req, "cmd")? {
        "status" => Ok(status(node)),
        "pair_code" => {
            let code = proto::new_pair_code();
            let until = Instant::now() + PAIR_WINDOW;
            *node.pairing.lock().unwrap() = Some(Pairing { code: code.clone(), until, attempts: 0 });
            *node.trust.pairing_until.lock().unwrap() = Some(until);
            Ok(json!({"code": code, "expires_in": PAIR_WINDOW.as_secs(), "addrs": net::local_addrs(node.port())}))
        }
        "pair_cancel" => {
            *node.pairing.lock().unwrap() = None;
            *node.trust.pairing_until.lock().unwrap() = None;
            Ok(json!({}))
        }
        "pair" => {
            let p = pair_with(node, str_arg(req, "addr")?, str_arg(req, "code")?).await?;
            Ok(json!({"name": p.name, "fingerprint": p.fingerprint, "short_fingerprint": short_fingerprint(&p.fingerprint)}))
        }
        "unpair" => {
            let fp = str_arg(req, "peer")?.to_string();
            node.state.lock().unwrap().peers.retain(|p| p.fingerprint != fp);
            node.sync_trust();
            node.save()?;
            Ok(json!({}))
        }
        "ping" => {
            ping(node, str_arg(req, "peer")?).await?;
            Ok(json!({}))
        }
        "set" => {
            {
                let mut st = node.state.lock().unwrap();
                if let Some(b) = req.get("autosync").and_then(|v| v.as_bool()) {
                    match req.get("peer").and_then(|v| v.as_str()) {
                        Some(fp) => st.peers.iter_mut().filter(|p| p.fingerprint == fp).for_each(|p| p.autosync = b),
                        None => st.settings.autosync = b,
                    }
                }
                if let Some(d) = req.get("receive_dir").and_then(|v| v.as_str()) {
                    st.settings.receive_dir = if d.trim().is_empty() { None } else { Some(PathBuf::from(d)) };
                }
            }
            node.save()?;
            Ok(status(node))
        }
        "send" => {
            let target = req.get("peer").and_then(|v| v.as_str()).map(String::from);
            let content = if let Some(t) = req.get("text").and_then(|v| v.as_str()) {
                Content::Text(t.to_string())
            } else if let Some(b) = req.get("image_png_b64").and_then(|v| v.as_str()) {
                let png = data_encoding::BASE64.decode(b.as_bytes()).context("image is not valid base64")?;
                clip::decode_png(&png)?;
                Content::Image(png)
            } else if let Some(arr) = req.get("paths").and_then(|v| v.as_array()) {
                let paths: Vec<PathBuf> = arr.iter().filter_map(|v| v.as_str()).map(PathBuf::from).collect();
                if paths.is_empty() {
                    bail!("no paths given");
                }
                Content::Files(paths)
            } else {
                let h = node.clip.clone().context("clipboard access is off on this bridge")?;
                tokio::task::spawn_blocking(move || h.read()).await??.context("the clipboard is empty")?
            };
            let r = send_to(node, target, content, true).await?;
            Ok(json!({"results": r}))
        }
        other => bail!("unknown command '{other}'"),
    }
}

/// Client side of the control API, used by the CLI.
pub async fn control_call(dir: &Path, req: Value) -> Result<Value> {
    let state = config::load_state(dir)?;
    let token = std::fs::read_to_string(dir.join("control.token")).context("the bridge has not run yet")?;
    let mut req = req;
    req["token"] = Value::String(token.trim().to_string());
    let sock = TcpStream::connect(("127.0.0.1", state.settings.port + config::CONTROL_PORT_OFFSET))
        .await
        .context("the Remote Access Crew bridge is not running")?;
    let (r, mut w) = sock.into_split();
    let mut s = req.to_string();
    s.push('\n');
    w.write_all(s.as_bytes()).await?;
    let mut line = String::new();
    BufReader::new(r).read_line(&mut line).await?;
    let v: Value = serde_json::from_str(&line)?;
    if v["ok"].as_bool() == Some(true) {
        Ok(v["result"].clone())
    } else {
        Err(anyhow!("{}", v["error"].as_str().unwrap_or("request failed")))
    }
}
