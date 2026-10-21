//! Clipboard actor. One thread owns the system clipboard for the life of the bridge, which
//! keeps X11/Wayland ownership alive after we set content. Clipboard content is never logged.

use anyhow::{Context, Result, anyhow, bail};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const MAX_PIXELS: u64 = 120_000_000;

#[derive(Debug, Clone)]
pub enum Content {
    Text(String),
    /// PNG bytes.
    Image(Vec<u8>),
    Files(Vec<PathBuf>),
}

impl Content {
    pub fn kind(&self) -> &'static str {
        match self {
            Content::Text(_) => "text",
            Content::Image(_) => "image",
            Content::Files(_) => "files",
        }
    }
}

enum Cmd {
    Apply(Content, mpsc::Sender<Result<()>>),
    Read(mpsc::Sender<Result<Option<Content>>>),
}

#[derive(Clone)]
pub struct ClipHandle {
    tx: mpsc::Sender<Cmd>,
}

impl ClipHandle {
    pub fn apply(&self, c: Content) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(Cmd::Apply(c, tx)).map_err(|_| anyhow!("clipboard thread stopped"))?;
        rx.recv().map_err(|_| anyhow!("clipboard thread stopped"))?
    }
    pub fn read(&self) -> Result<Option<Content>> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(Cmd::Read(tx)).map_err(|_| anyhow!("clipboard thread stopped"))?;
        rx.recv().map_err(|_| anyhow!("clipboard thread stopped"))?
    }
}

/// Start the clipboard thread. Local changes are delivered on `changes` when `watch` is true.
pub fn start(watch: bool, changes: tokio::sync::mpsc::UnboundedSender<Content>) -> Result<ClipHandle> {
    let (tx, rx) = mpsc::channel::<Cmd>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
    std::thread::Builder::new().name("rac-clipboard".into()).spawn(move || {
        let cb = match arboard::Clipboard::new() {
            Ok(c) => {
                let _ = ready_tx.send(Ok(()));
                c
            }
            Err(e) => {
                let _ = ready_tx.send(Err(anyhow!("clipboard unavailable: {e}")));
                return;
            }
        };
        Actor { cb, last_fp: None, applied_fp: None, applied_at: None, seq: platform::seq(), recheck: false }.run(rx, watch, changes);
    })?;
    ready_rx.recv().map_err(|_| anyhow!("clipboard thread failed"))??;
    Ok(ClipHandle { tx })
}

struct Actor {
    cb: arboard::Clipboard,
    /// Fingerprint of the last clipboard content we observed.
    last_fp: Option<String>,
    /// Fingerprint of the content we last placed on the clipboard from a peer.
    applied_fp: Option<String>,
    applied_at: Option<Instant>,
    seq: u32,
    recheck: bool,
}

impl Actor {
    fn run(mut self, rx: mpsc::Receiver<Cmd>, watch: bool, changes: tokio::sync::mpsc::UnboundedSender<Content>) {
        // Treat whatever is on the clipboard at start as already seen.
        if let Ok(Some((_, fp))) = self.snapshot() {
            self.last_fp = Some(fp);
        }
        let poll = if cfg!(windows) { Duration::from_millis(250) } else { Duration::from_millis(700) };
        loop {
            match rx.recv_timeout(poll) {
                Ok(Cmd::Apply(c, reply)) => {
                    let _ = reply.send(self.apply(c));
                }
                Ok(Cmd::Read(reply)) => {
                    let _ = reply.send(self.snapshot().map(|o| o.map(|(c, _)| c)));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if watch {
                        if let Some(c) = self.poll_change() {
                            if changes.send(c).is_err() {
                                return;
                            }
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn poll_change(&mut self) -> Option<Content> {
        if cfg!(windows) {
            let s = platform::seq();
            if s == self.seq && !self.recheck {
                return None;
            }
            self.seq = s;
        }
        self.recheck = false;
        if platform::is_private_content() {
            // Password managers mark secrets this way. Never send them.
            self.last_fp = None;
            return None;
        }
        let (c, fp) = self.snapshot().ok()??;
        if self.last_fp.as_deref() == Some(fp.as_str()) {
            return None;
        }
        if self.applied_fp.as_deref() == Some(fp.as_str()) {
            self.last_fp = Some(fp);
            return None;
        }
        // Some apps re-encode content we just placed. Wait briefly, then look again, so a
        // genuine new copy is still sent but an immediate re-encode does not bounce back.
        if matches!(self.applied_at, Some(t) if t.elapsed() < Duration::from_millis(1500)) {
            self.recheck = true;
            return None;
        }
        self.last_fp = Some(fp);
        Some(c)
    }

    /// Read the clipboard. Priority: files, then text, then image.
    fn snapshot(&mut self) -> Result<Option<(Content, String)>> {
        if let Ok(files) = self.cb.get().file_list() {
            if !files.is_empty() {
                let fp = fingerprint_files(&files);
                return Ok(Some((Content::Files(files), fp)));
            }
        }
        if let Ok(t) = self.cb.get_text() {
            if !t.is_empty() {
                let fp = fingerprint_text(&t);
                return Ok(Some((Content::Text(t), fp)));
            }
        }
        if let Ok(img) = self.cb.get_image() {
            if (img.width as u64) * (img.height as u64) <= MAX_PIXELS && img.width > 0 && img.height > 0 {
                let fp = fingerprint_rgba(img.width, img.height, &img.bytes);
                let png = encode_png(img.width as u32, img.height as u32, &img.bytes)?;
                return Ok(Some((Content::Image(png), fp)));
            }
        }
        Ok(None)
    }

    fn apply(&mut self, c: Content) -> Result<()> {
        match c {
            Content::Text(t) => {
                set_text(&mut self.cb, &t)?;
            }
            Content::Image(png) => {
                let (w, h, rgba) = decode_png(&png)?;
                self.cb
                    .set_image(arboard::ImageData { width: w as usize, height: h as usize, bytes: Cow::Owned(rgba) })
                    .context("could not place the image on the clipboard")?;
            }
            Content::Files(paths) => {
                self.cb.set().file_list(&paths).context("could not place the files on the clipboard")?;
            }
        }
        // Record what is on the clipboard now, as read back, so the watcher does not re-send it.
        self.seq = platform::seq();
        let now = self.snapshot().ok().flatten().map(|(_, fp)| fp);
        self.applied_fp = now.clone();
        self.last_fp = now;
        self.applied_at = Some(Instant::now());
        Ok(())
    }
}

fn set_text(cb: &mut arboard::Clipboard, t: &str) -> Result<()> {
    cb.set_text(t.to_string()).context("could not place the text on the clipboard")
}

pub fn fingerprint_text(t: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"t");
    h.update(t.as_bytes());
    hex::encode(h.finalize())
}

pub fn fingerprint_rgba(w: usize, h: usize, b: &[u8]) -> String {
    let mut s = Sha256::new();
    s.update(b"i");
    s.update((w as u64).to_le_bytes());
    s.update((h as u64).to_le_bytes());
    s.update(b);
    hex::encode(s.finalize())
}

pub fn fingerprint_files(files: &[PathBuf]) -> String {
    let mut s = Sha256::new();
    s.update(b"f");
    for f in files {
        s.update(f.to_string_lossy().as_bytes());
        s.update([0u8]);
    }
    hex::encode(s.finalize())
}

pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    if rgba.len() != (w as usize) * (h as usize) * 4 {
        bail!("unexpected image buffer size");
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut wr = enc.write_header()?;
        wr.write_image_data(rgba)?;
    }
    Ok(out)
}

/// Decode a PNG to RGBA8 with size limits checked before allocating.
pub fn decode_png(data: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(data));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = dec.read_info().context("not a valid PNG image")?;
    let (w, h) = (r.info().width, r.info().height);
    if w == 0 || h == 0 || (w as u64) * (h as u64) > MAX_PIXELS {
        bail!("image dimensions are out of range");
    }
    let mut buf = vec![0u8; r.output_buffer_size().context("image too large")?];
    let info = r.next_frame(&mut buf).context("corrupt PNG image")?;
    buf.truncate(info.buffer_size());
    let px = (w as usize) * (h as usize);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => bail!("unexpected indexed PNG after expansion"),
    };
    if rgba.len() != px * 4 {
        bail!("unexpected PNG layout");
    }
    Ok((w, h, rgba))
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::System::DataExchange::{GetClipboardSequenceNumber, IsClipboardFormatAvailable, RegisterClipboardFormatW};

    pub fn seq() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    fn format(name: &str) -> u32 {
        let w: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        unsafe { RegisterClipboardFormatW(w.as_ptr()) }
    }

    /// True when the clipboard owner asked monitors and history to skip this content.
    pub fn is_private_content() -> bool {
        // Apps add the history/cloud formats only to opt out, so their presence means private.
        ["ExcludeClipboardContentFromMonitorProcessing", "Clipboard Viewer Ignore", "CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"]
            .iter()
            .any(|n| unsafe { IsClipboardFormatAvailable(format(n)) } != 0)
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn seq() -> u32 {
        0
    }
    pub fn is_private_content() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_roundtrip() {
        let rgba: Vec<u8> = (0..(3 * 2 * 4)).map(|i| i as u8).collect();
        let png = encode_png(3, 2, &rgba).unwrap();
        let (w, h, back) = decode_png(&png).unwrap();
        assert_eq!((w, h), (3, 2));
        assert_eq!(back, rgba);
    }

    #[test]
    fn rejects_garbage_png() {
        assert!(decode_png(b"not a png").is_err());
    }
}
