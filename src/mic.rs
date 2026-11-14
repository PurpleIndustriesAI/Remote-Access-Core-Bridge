//! Microphone sharing. A client streams its default microphone to a paired host over the
//! verified TLS link, and the host plays it into the "Steam Streaming Microphone" virtual
//! cable, so host apps (voice chat, games, Command Core) record it as an ordinary microphone.
//!
//! Wire format after the go ahead reply: frames of 48 kHz mono 16 bit little endian PCM,
//! 20 ms each, prefixed by their byte length (u16 big endian). About 770 kbit/s.
// shortcut: raw PCM, not Opus; fine on a LAN or Tailscale, add an Opus CODEC for slow links.

use anyhow::{Result, anyhow, bail};
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const CODEC: &str = "pcm16le/48000/mono/20ms";
pub const RATE: u32 = 48_000;
pub const FRAME: usize = 960;
const MAX_FRAME_BYTES: usize = FRAME * 2 * 4;
/// Playback starts once 60 ms is queued; audio queued past 120 ms is late and dropped.
const TARGET: usize = RATE as usize * 60 / 1000;
const MAX_QUEUE: usize = RATE as usize * 120 / 1000;
/// The host's playback device. Its recording side is "Microphone (Steam Streaming Microphone)".
pub const HOST_SINK: &str = "Steam Streaming Microphone";

/// Small jitter buffer between the network and the playback device.
#[derive(Default)]
pub struct Jitter {
    q: VecDeque<i16>,
    playing: bool,
    pub dropped: u64,
    pub underruns: u64,
}

impl Jitter {
    pub fn push(&mut self, s: &[i16]) {
        self.q.extend(s);
        if self.q.len() > MAX_QUEUE {
            let late = self.q.len() - TARGET;
            self.q.drain(..late);
            self.dropped += late as u64;
        }
    }

    /// Next sample for the device, or silence while (re)buffering.
    pub fn pull(&mut self) -> i16 {
        if !self.playing {
            if self.q.len() < TARGET {
                return 0;
            }
            self.playing = true;
        }
        self.q.pop_front().unwrap_or_else(|| {
            self.playing = false;
            self.underruns += 1;
            0
        })
    }
}

/// Linear resampler from a microphone's native rate to 48 kHz.
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: f32,
}

impl Resampler {
    pub fn new(from: u32) -> Self {
        Resampler { step: from as f64 / RATE as f64, pos: 1.0, prev: 0.0 }
    }

    pub fn push(&mut self, x: f32, out: &mut Vec<i16>) {
        while self.pos <= 1.0 {
            let y = self.prev + (x - self.prev) * self.pos as f32;
            out.push((y.clamp(-1.0, 1.0) * 32767.0) as i16);
            self.pos += self.step;
        }
        self.pos -= 1.0;
        self.prev = x;
    }
}

pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, s: &[i16]) -> Result<()> {
    let mut b = Vec::with_capacity(2 + s.len() * 2);
    b.extend_from_slice(&((s.len() * 2) as u16).to_be_bytes());
    s.iter().for_each(|x| b.extend_from_slice(&x.to_le_bytes()));
    w.write_all(&b).await?;
    w.flush().await?;
    Ok(())
}

/// Next frame, or None once the sender closed the stream.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Option<Vec<i16>>> {
    let mut l = [0u8; 2];
    if let Err(e) = r.read_exact(&mut l).await {
        return if e.kind() == std::io::ErrorKind::UnexpectedEof { Ok(None) } else { Err(e.into()) };
    }
    let n = u16::from_be_bytes(l) as usize;
    if n == 0 || n % 2 != 0 || n > MAX_FRAME_BYTES {
        bail!("bad microphone frame");
    }
    let mut b = vec![0u8; n];
    r.read_exact(&mut b).await?;
    Ok(Some(b.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect()))
}

/// The local microphone (or a test tone) is live while this exists. Dropping it stops
/// capture at once, which also clears Windows' microphone in use indicator.
pub struct Capture {
    pub device: String,
    _stop: Sender<()>,
}

impl Capture {
    /// Start capturing; audio arrives as 48 kHz mono chunks on the returned receiver.
    pub fn start(tone: bool) -> Result<(Capture, tokio::sync::mpsc::Receiver<Vec<i16>>)> {
        let (tx, rx) = tokio::sync::mpsc::channel(50);
        let (stop, stopped) = std::sync::mpsc::channel();
        let (ready_tx, ready) = std::sync::mpsc::channel();
        std::thread::Builder::new().name("rac mic".into()).spawn(move || {
            if tone {
                let _ = ready_tx.send(Ok("test tone".to_string()));
                play_tone(tx, stopped);
            } else {
                dev::capture(tx, stopped, ready_tx);
            }
        })?;
        let device = ready.recv().map_err(|_| anyhow!("the microphone thread stopped"))??;
        Ok((Capture { device, _stop: stop }, rx))
    }
}

/// 440 Hz at -12 dBFS, paced in real time, for checking a host without speaking.
fn play_tone(tx: tokio::sync::mpsc::Sender<Vec<i16>>, stopped: Receiver<()>) {
    let start = Instant::now();
    let mut n = 0u64;
    loop {
        let chunk = (0..FRAME as u64)
            .map(|i| ((n + i) as f32 / RATE as f32 * 440.0 * std::f32::consts::TAU).sin() * 8192.0)
            .map(|x| x as i16)
            .collect();
        n += FRAME as u64;
        if tx.try_send(chunk).is_err() && tx.is_closed() {
            return;
        }
        let due = start + Duration::from_micros(n * 1_000_000 / RATE as u64);
        if stopped.recv_timeout(due.saturating_duration_since(Instant::now())) != Err(RecvTimeoutError::Timeout) {
            return;
        }
    }
}

/// Plays received audio into the host's virtual microphone until dropped.
pub struct Player {
    pub jitter: Arc<Mutex<Jitter>>,
    pub device: String,
    _stop: Sender<()>,
}

impl Player {
    pub fn open() -> Result<Player> {
        let jitter = Arc::new(Mutex::new(Jitter::default()));
        let (stop, stopped) = std::sync::mpsc::channel();
        // Tests and machines without audio hardware: accept the stream and discard it.
        if std::env::var_os("RAC_MIC_NULL_SINK").is_some() {
            return Ok(Player { jitter, device: "null sink".into(), _stop: stop });
        }
        let (ready_tx, ready) = std::sync::mpsc::channel();
        let j = jitter.clone();
        std::thread::Builder::new().name("rac mic playback".into()).spawn(move || dev::play(j, stopped, ready_tx))?;
        let device = ready.recv().map_err(|_| anyhow!("the playback thread stopped"))??;
        Ok(Player { jitter, device, _stop: stop })
    }
}

#[cfg(windows)]
mod dev {
    use super::{HOST_SINK, Jitter, RATE, Resampler};
    use anyhow::{Context, Result, bail};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Arc, Mutex};

    fn hold(r: Result<(cpal::Stream, String)>, stopped: Receiver<()>, ready: Sender<Result<String>>) {
        match r {
            Ok((stream, name)) => {
                let _ = ready.send(Ok(name));
                let _ = stopped.recv();
                drop(stream);
            }
            Err(e) => {
                let _ = ready.send(Err(e));
            }
        }
    }

    pub fn capture(tx: tokio::sync::mpsc::Sender<Vec<i16>>, stopped: Receiver<()>, ready: Sender<Result<String>>) {
        let open = || -> Result<(cpal::Stream, String)> {
            let dev = cpal::default_host().default_input_device().context("this machine has no microphone")?;
            let name = dev.to_string();
            let cfg = dev.default_input_config().context("cannot read the microphone format")?;
            let ch = cfg.channels().max(1) as usize;
            let mut rs = Resampler::new(cfg.sample_rate());
            // Downmix each frame to mono, resample, and drop the chunk if the network is behind.
            let mut feed = move |it: &mut dyn Iterator<Item = f32>| {
                let mut out = Vec::with_capacity(1024);
                let (mut acc, mut k) = (0.0f32, 0);
                for x in it {
                    acc += x;
                    k += 1;
                    if k == ch {
                        rs.push(acc / ch as f32, &mut out);
                        (acc, k) = (0.0, 0);
                    }
                }
                if !out.is_empty() {
                    let _ = tx.try_send(out);
                }
            };
            let err = |e| eprintln!("[rac] microphone capture error: {e}");
            let stream = match cfg.sample_format() {
                cpal::SampleFormat::F32 => dev.build_input_stream(cfg.config(), move |d: &[f32], _: &_| feed(&mut d.iter().copied()), err, None)?,
                cpal::SampleFormat::I16 => dev.build_input_stream(cfg.config(), move |d: &[i16], _: &_| feed(&mut d.iter().map(|&x| x as f32 / 32768.0)), err, None)?,
                f => bail!("unsupported microphone sample format {f:?}"),
            };
            stream.play().context("Windows refused the microphone; check Settings > Privacy > Microphone for desktop apps")?;
            Ok((stream, name))
        };
        hold(open(), stopped, ready);
    }

    pub fn play(jitter: Arc<Mutex<Jitter>>, stopped: Receiver<()>, ready: Sender<Result<String>>) {
        let open = || -> Result<(cpal::Stream, String)> {
            // Never fall back to the default output: Sunshine streams that back to the client,
            // which would then hear itself.
            let dev = cpal::default_host().output_devices()?.find(|d| d.to_string().contains(HOST_SINK))
                .with_context(|| format!("this machine has no \"{HOST_SINK}\" playback device; installing Steam adds it"))?;
            let name = dev.to_string();
            // The Steam driver copies bytes between its two sides without converting, so
            // different formats play voice slowed and broken. Refuse instead.
            let mic = cpal::default_host().input_devices()?.find(|d| d.to_string().contains(HOST_SINK));
            if let (Some(o), Some(i)) = (format_of(&dev, "Render"), mic.as_ref().and_then(|m| format_of(m, "Capture"))) {
                if o != i {
                    bail!("\"{name}\" runs at {} but its microphone side at {}, and the Steam driver does not convert, so voice would \
                        arrive slowed and broken. Fix once in Sound settings: More sound settings > Playback > {name} > Properties > \
                        Advanced > Default Format: {}", show(o), show(i), show(i));
                }
            }
            let ch = dev.default_output_config()?.channels().max(1) as usize;
            // Windows converts the rate if the device mix format differs.
            let config = cpal::StreamConfig { channels: ch as u16, sample_rate: RATE, buffer_size: cpal::BufferSize::Default };
            let stream = dev.build_output_stream(config, move |out: &mut [f32], _: &_| {
                let mut j = jitter.lock().unwrap();
                for f in out.chunks_mut(ch) {
                    f.fill(j.pull() as f32 / 32768.0);
                }
            }, |e| eprintln!("[rac] microphone playback error: {e}"), None)?;
            stream.play()?;
            Ok((stream, name))
        };
        hold(open(), stopped, ready);
    }

    fn show((ch, rate, bits): (u16, u32, u16)) -> String {
        format!("{ch} channel, {bits} bit, {rate} Hz")
    }

    /// Channels, rate and bits of an endpoint's Default Format, as the audio engine opens it.
    fn format_of(dev: &cpal::Device, flow: &str) -> Option<(u16, u32, u16)> {
        use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_BINARY, RegGetValueW};
        let id = dev.id().ok()?.to_string();
        let guid = id.rsplit('.').next().filter(|g| g.len() == 38 && g.bytes().all(|c| c.is_ascii_hexdigit() || b"{}-".contains(&c)))?;
        let key: Vec<u16> = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\{flow}\{guid}\Properties")
            .encode_utf16().chain([0]).collect();
        let value: Vec<u16> = "{f19f064d-082c-4e27-bc73-6882a1bb8e4c},0".encode_utf16().chain([0]).collect();
        let mut b = [0u8; 256];
        let mut n = b.len() as u32;
        let rc = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), value.as_ptr(), RRF_RT_REG_BINARY, std::ptr::null_mut(), b.as_mut_ptr().cast(), &mut n) };
        // A PROPVARIANT header (8 bytes), then the WAVEFORMATEX.
        if rc != 0 || n < 24 {
            return None;
        }
        Some((u16::from_le_bytes([b[10], b[11]]), u32::from_le_bytes([b[12], b[13], b[14], b[15]]), u16::from_le_bytes([b[22], b[23]])))
    }
}

#[cfg(not(windows))]
mod dev {
    use super::Jitter;
    use anyhow::{Result, anyhow};
    use std::sync::mpsc::{Receiver, Sender};
    use std::sync::{Arc, Mutex};

    pub fn capture(_: tokio::sync::mpsc::Sender<Vec<i16>>, _: Receiver<()>, ready: Sender<Result<String>>) {
        let _ = ready.send(Err(anyhow!("microphone sharing needs Windows")));
    }

    pub fn play(_: Arc<Mutex<Jitter>>, _: Receiver<()>, ready: Sender<Result<String>>) {
        let _ = ready.send(Err(anyhow!("microphone sharing needs Windows")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_buffers_then_drops_late_audio() {
        let mut j = Jitter::default();
        j.push(&vec![1; TARGET - 1]);
        assert_eq!(j.pull(), 0, "silent until 60 ms is queued");
        j.push(&[1]);
        assert_eq!(j.pull(), 1);
        j.push(&vec![2; MAX_QUEUE]);
        assert_eq!(j.q.len(), TARGET, "late audio is cut back to the target");
        assert!(j.dropped > 0);
        for _ in 0..TARGET {
            j.pull();
        }
        assert_eq!(j.pull(), 0);
        assert_eq!(j.underruns, 1);
        j.push(&[3; 10]);
        assert_eq!(j.pull(), 0, "an underrun buffers again before playing");
    }

    #[test]
    fn resampler_keeps_48k_and_converts_44k1() {
        let mut out = Vec::new();
        let mut r = Resampler::new(48_000);
        (0..480).for_each(|i| r.push(i as f32 / 1000.0, &mut out));
        assert_eq!(out.len(), 480);
        assert_eq!(out[100], (0.1f32 * 32767.0) as i16);
        let mut out = Vec::new();
        let mut r = Resampler::new(44_100);
        (0..44_100).for_each(|_| r.push(0.5, &mut out));
        assert!((out.len() as i64 - 48_000).abs() <= 1, "{}", out.len());
    }

    #[tokio::test]
    async fn frames_round_trip_and_reject_garbage() {
        let (mut a, mut b) = tokio::io::duplex(64 * 1024);
        write_frame(&mut a, &[1, -2, i16::MAX]).await.unwrap();
        drop(a);
        assert_eq!(read_frame(&mut b).await.unwrap(), Some(vec![1, -2, i16::MAX]));
        assert_eq!(read_frame(&mut b).await.unwrap(), None);
        let (mut a, mut b) = tokio::io::duplex(64);
        a.write_all(&3u16.to_be_bytes()).await.unwrap();
        assert!(read_frame(&mut b).await.is_err(), "odd lengths are refused");
    }
}
