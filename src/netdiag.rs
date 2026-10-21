//! Network helper: live Wi-Fi readings, nearby access points for the same network, and
//! plain advice for better streaming. Reads only local OS tools; nothing leaves the machine.

use serde::Serialize;
use std::process::Command;

#[derive(Debug, Default, Clone, Serialize)]
pub struct Link {
    pub connected: bool,
    pub ssid: String,
    pub bssid: String,
    /// "2.4 GHz", "5 GHz", "6 GHz", or "".
    pub band: String,
    pub channel: u32,
    pub radio: String,
    /// 0-100 as reported by the OS.
    pub signal: u32,
    pub rx_mbps: f64,
    pub tx_mbps: f64,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct AccessPoint {
    pub bssid: String,
    pub band: String,
    pub channel: u32,
    pub signal: u32,
    /// Channel busy percentage when the access point advertises it.
    pub utilization: Option<u32>,
    pub current: bool,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Wifi {
    pub available: bool,
    pub link: Link,
    /// Access points broadcasting the same network name.
    pub same_network: Vec<AccessPoint>,
    pub advice: Vec<Advice>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Advice {
    /// "good", "tip", or "warn".
    pub level: &'static str,
    pub text: String,
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = Command::new(cmd);
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: never flash a console
    }
    let out = c.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn band_of(freq_mhz: u32) -> String {
    match freq_mhz {
        2400..=2500 => "2.4 GHz",
        4900..=5900 => "5 GHz",
        5925..=7125 => "6 GHz",
        _ => "",
    }
    .into()
}

fn num(s: &str) -> f64 {
    s.trim().trim_end_matches('%').split_whitespace().next().unwrap_or("").parse().unwrap_or(0.0)
}

/// Parse `netsh wlan show interfaces` (English labels).
pub fn parse_netsh_interfaces(text: &str) -> Link {
    let mut l = Link::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "State" => l.connected = v.eq_ignore_ascii_case("connected"),
            "SSID" => l.ssid = v.to_string(),
            "AP BSSID" | "BSSID" => l.bssid = line.splitn(2, ':').nth(1).unwrap_or("").trim().to_lowercase(),
            "Band" => l.band = v.to_string(),
            "Channel" => l.channel = num(v) as u32,
            "Radio type" => l.radio = v.to_string(),
            "Signal" => l.signal = num(v) as u32,
            "Receive rate (Mbps)" => l.rx_mbps = num(v),
            "Transmit rate (Mbps)" => l.tx_mbps = num(v),
            _ => {}
        }
    }
    if l.band.is_empty() && l.channel > 0 {
        l.band = if l.channel <= 14 { "2.4 GHz".into() } else { "5 GHz".into() };
    }
    l
}

/// Parse `netsh wlan show networks mode=bssid`, keeping access points for `ssid`.
pub fn parse_netsh_networks(text: &str, ssid: &str, current_bssid: &str) -> Vec<AccessPoint> {
    let mut out = Vec::new();
    let mut in_ssid = false;
    let mut ap: Option<AccessPoint> = None;
    let flush = |ap: &mut Option<AccessPoint>, out: &mut Vec<AccessPoint>| {
        if let Some(a) = ap.take() {
            out.push(a);
        }
    };
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("SSID ") {
            flush(&mut ap, &mut out);
            in_ssid = t.split_once(':').map(|(_, v)| v.trim() == ssid).unwrap_or(false);
            continue;
        }
        if !in_ssid {
            continue;
        }
        if t.starts_with("BSSID ") {
            flush(&mut ap, &mut out);
            let b = t.splitn(2, ':').nth(1).unwrap_or("").trim().to_lowercase();
            ap = Some(AccessPoint { current: !current_bssid.is_empty() && b == current_bssid, bssid: b, ..Default::default() });
            continue;
        }
        let (Some(a), Some((k, v))) = (ap.as_mut(), t.split_once(':')) else { continue };
        match k.trim() {
            "Signal" => a.signal = num(v) as u32,
            "Band" => a.band = v.trim().to_string(),
            "Channel" => a.channel = num(v) as u32,
            // "Channel Utilization: 128 (50 %)"
            "Channel Utilization" => a.utilization = v.split('(').nth(1).map(|p| num(p) as u32),
            _ => {}
        }
    }
    flush(&mut ap, &mut out);
    out
}

/// Parse `nmcli -t -f IN-USE,SSID,BSSID,CHAN,FREQ,RATE,SIGNAL dev wifi list`.
pub fn parse_nmcli(text: &str) -> (Link, Vec<AccessPoint>) {
    let mut link = Link::default();
    let mut rows = Vec::new();
    for line in text.lines() {
        // nmcli escapes ':' inside fields as '\:'.
        let mut fields = Vec::new();
        let mut cur = String::new();
        let mut esc = false;
        for ch in line.chars() {
            match (esc, ch) {
                (true, c) => {
                    cur.push(c);
                    esc = false
                }
                (false, '\\') => esc = true,
                (false, ':') => fields.push(std::mem::take(&mut cur)),
                (false, c) => cur.push(c),
            }
        }
        fields.push(cur);
        if fields.len() < 7 {
            continue;
        }
        let freq = num(&fields[4]) as u32;
        let ap = AccessPoint {
            bssid: fields[2].to_lowercase(),
            band: band_of(freq),
            channel: num(&fields[3]) as u32,
            signal: num(&fields[6]) as u32,
            utilization: None,
            current: fields[0].trim() == "*",
        };
        if ap.current {
            link = Link { connected: true, ssid: fields[1].clone(), bssid: ap.bssid.clone(), band: ap.band.clone(), channel: ap.channel, radio: String::new(), signal: ap.signal, rx_mbps: num(&fields[5]), tx_mbps: num(&fields[5]) };
        }
        rows.push((fields[1].clone(), ap));
    }
    let same = rows.into_iter().filter(|(s, _)| link.connected && *s == link.ssid).map(|(_, a)| a).collect();
    (link, same)
}

/// Read Wi-Fi now. `scan` also lists access points for the same network (slower; ~1 call per few seconds).
pub fn wifi(scan: bool) -> Wifi {
    #[cfg(windows)]
    let (link, same) = match run("netsh", &["wlan", "show", "interfaces"]) {
        Some(t) => {
            let link = parse_netsh_interfaces(&t);
            let same = if scan && link.connected {
                run("netsh", &["wlan", "show", "networks", "mode=bssid"]).map(|n| parse_netsh_networks(&n, &link.ssid, &link.bssid)).unwrap_or_default()
            } else {
                Vec::new()
            };
            (Some(link), same)
        }
        None => (None, Vec::new()),
    };
    #[cfg(not(windows))]
    let (link, same) = match run("nmcli", &["-t", "-f", "IN-USE,SSID,BSSID,CHAN,FREQ,RATE,SIGNAL", "dev", "wifi", "list", "--rescan", if scan { "auto" } else { "no" }]) {
        Some(t) => {
            let (l, s) = parse_nmcli(&t);
            (Some(l), s)
        }
        None => (None, Vec::new()),
    };
    let available = link.is_some();
    let link = link.unwrap_or_default();
    let advice = advise(&link, &same);
    Wifi { available, link, same_network: same, advice }
}

pub fn advise(link: &Link, same: &[AccessPoint]) -> Vec<Advice> {
    let mut a = Vec::new();
    let tip = |t: String| Advice { level: "tip", text: t };
    let warn = |t: String| Advice { level: "warn", text: t };
    if !link.connected {
        a.push(Advice { level: "good", text: "Not on Wi-Fi. A wired connection is the most reliable choice for streaming.".into() });
        return a;
    }
    a.push(tip("A network cable beats any Wi-Fi for streaming. Plug in the host if you can.".into()));
    let cur = same.iter().find(|p| p.current);
    let better = same
        .iter()
        .filter(|p| !p.current && (p.band == "5 GHz" || p.band == "6 GHz") && p.signal >= 50)
        .max_by_key(|p| (p.signal, 100 - p.utilization.unwrap_or(50)));
    if link.band == "2.4 GHz" {
        match better {
            Some(b) => a.push(warn(format!(
                "You are on 2.4 GHz (channel {}{}). The same network is on {} channel {} at {}% signal{}. Switch to it: set the Wi-Fi adapter's Preferred Band to 5 GHz, or reconnect.",
                link.channel,
                cur.and_then(|c| c.utilization).map(|u| format!(", {u}% busy")).unwrap_or_default(),
                b.band,
                b.channel,
                b.signal,
                b.utilization.map(|u| format!(", {u}% busy")).unwrap_or_default()
            ))),
            None => a.push(warn("You are on 2.4 GHz, which is slow and crowded. Use a 5 GHz or 6 GHz network if your router offers one.".into())),
        }
    }
    match link.signal {
        0..=49 => a.push(warn(format!("Weak signal ({}%). Move closer to the router, keep a clear line of sight, or lift the router off the floor.", link.signal))),
        50..=69 => a.push(tip(format!("Fair signal ({}%). Moving a little closer or removing obstacles will cut packet loss.", link.signal))),
        _ => a.push(Advice { level: "good", text: format!("Strong signal ({}%).", link.signal) }),
    }
    if let Some(u) = cur.and_then(|c| c.utilization) {
        if u >= 40 {
            a.push(warn(format!("Channel {} is {u}% busy. Pick a quieter channel in the router settings, or use 5 GHz.", link.channel)));
        }
    }
    let rate = link.rx_mbps.min(link.tx_mbps);
    if rate > 0.0 && rate < 200.0 {
        a.push(tip(format!("Link rate is {rate:.0} Mbps. Real throughput is about half of that, so keep Moonlight's bitrate under {:.0} Mbps.", (rate * 0.35).max(5.0))));
    }
    if link.radio.contains("802.11n") || link.radio.contains("802.11g") {
        a.push(tip(format!("The connection uses {}, an older standard. A Wi-Fi 6 network (802.11ax) gives more headroom.", link.radio)));
    }
    a
}

/// Suggest a Moonlight bitrate from a measured throughput and loss.
pub fn recommend_bitrate(throughput_mbps: f64, loss_pct: f64) -> u32 {
    let mut b = throughput_mbps * 0.6;
    if loss_pct > 1.0 {
        b *= 0.7;
    }
    b.clamp(5.0, 150.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const IFACE: &str = "
    Name                   : Wi-Fi
    State                  : connected
    SSID                   : HomeNet
    AP BSSID               : aa:bb:cc:dd:ee:01
    Band                   : 2.4 GHz
    Channel                : 1
    Radio type             : 802.11n
    Receive rate (Mbps)    : 104
    Transmit rate (Mbps)   : 144.4
    Signal                 : 95%
";
    const NETS: &str = "
SSID 1 : HomeNet
    Network type            : Infrastructure
    BSSID 1                 : aa:bb:cc:dd:ee:01
         Signal             : 95%
         Band               : 2.4 GHz
         Channel            : 1
         Channel Utilization: 128 (50 %)
    BSSID 2                 : aa:bb:cc:dd:ee:02
         Signal             : 85%
         Band               : 5 GHz
         Channel            : 100
         Channel Utilization: 5 (1 %)
SSID 2 : Neighbor
    BSSID 1                 : 11:22:33:44:55:66
         Signal             : 99%
         Band               : 5 GHz
         Channel            : 36
";

    #[test]
    fn parses_windows_and_recommends_5ghz() {
        let l = parse_netsh_interfaces(IFACE);
        assert!(l.connected);
        assert_eq!((l.band.as_str(), l.channel, l.signal), ("2.4 GHz", 1, 95));
        assert_eq!(l.bssid, "aa:bb:cc:dd:ee:01");
        let aps = parse_netsh_networks(NETS, &l.ssid, &l.bssid);
        assert_eq!(aps.len(), 2, "neighbor network must be excluded");
        assert!(aps[0].current && aps[0].utilization == Some(50));
        assert_eq!((aps[1].band.as_str(), aps[1].channel, aps[1].utilization), ("5 GHz", 100, Some(1)));
        let adv = advise(&l, &aps);
        assert!(adv.iter().any(|a| a.level == "warn" && a.text.contains("5 GHz channel 100")), "{adv:?}");
        assert!(adv.iter().any(|a| a.text.contains("50% busy")));
    }

    #[test]
    fn parses_nmcli() {
        let t = "*:HomeNet:AA\\:BB\\:CC\\:DD\\:EE\\:02:100:5500 MHz:540 Mbit/s:82\n :HomeNet:AA\\:BB\\:CC\\:DD\\:EE\\:01:1:2412 MHz:144 Mbit/s:95\n :Other:11\\:22\\:33\\:44\\:55\\:66:36:5180 MHz:270 Mbit/s:60\n";
        let (l, same) = parse_nmcli(t);
        assert!(l.connected);
        assert_eq!((l.band.as_str(), l.channel, l.signal, l.bssid.as_str()), ("5 GHz", 100, 82, "aa:bb:cc:dd:ee:02"));
        assert_eq!(same.len(), 2);
    }

    #[test]
    fn bitrate_recommendation() {
        assert_eq!(recommend_bitrate(100.0, 0.0), 60);
        assert_eq!(recommend_bitrate(100.0, 3.0), 42);
        assert_eq!(recommend_bitrate(2.0, 0.0), 5);
    }
}
