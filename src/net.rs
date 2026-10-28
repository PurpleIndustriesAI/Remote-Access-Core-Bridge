//! Network policy: only private LAN, link-local, loopback, and Tailscale ranges may connect.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

pub fn is_tailscale(ip: &IpAddr) -> bool {
    match ip {
        // Tailscale CGNAT range 100.64.0.0/10.
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 100 && (o[1] & 0xC0) == 64
        }
        // Tailscale ULA prefix fd7a:115c:a1e0::/48.
        IpAddr::V6(v6) => {
            let s = v6.segments();
            s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
        }
    }
}

pub fn is_allowed_source(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4_allowed(v4) || is_tailscale(ip),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return v4_allowed(&v4) || is_tailscale(&IpAddr::V4(v4));
            }
            v6_allowed(v6)
        }
    }
}

fn v4_allowed(v4: &Ipv4Addr) -> bool {
    v4.is_loopback() || v4.is_private() || v4.is_link_local() || is_tailscale(&IpAddr::V4(*v4))
}

fn v6_allowed(v6: &Ipv6Addr) -> bool {
    let first = v6.segments()[0];
    v6.is_loopback()
        || (first & 0xfe00) == 0xfc00 // unique local fc00::/7 (includes Tailscale)
        || (first & 0xffc0) == 0xfe80 // link-local fe80::/10
}

/// VM, container, and VPN adapters that other machines cannot reach us through.
fn is_virtual(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["vethernet", "wsl", "hyper-v", "virtualbox", "vmware", "docker", "veth", "virbr", "br-", "nordlynx", "wireguard", "openvpn", "tap-", "zerotier"]
        .iter()
        .any(|k| n.contains(k))
}

/// Classify an interface without changing any network settings.
pub fn transport_name(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    if ["rndis", "usb", "ncm", "cdc"].iter().any(|k| n.contains(k)) {
        "usb"
    } else if ["bluetooth", "bnep", "bt-pan", "btpan"].iter().any(|k| n.contains(k)) {
        "bluetooth"
    } else if ["wlan", "wifi", "wi-fi", "ethernet", "eth", "enp", "eno"].iter().any(|k| n.starts_with(k)) {
        "wifi"
    } else {
        "unknown"
    }
}

#[cfg(windows)]
fn windows_transport_kind(kind: u32, medium: i32, device: &str) -> &'static str {
    let device = device.to_ascii_uppercase();
    if medium == 10 || device.starts_with("BTH\\") || device.starts_with("BTHENUM\\") {
        "bluetooth"
    } else if kind == 71 || medium == 1 || medium == 9 {
        "wifi"
    } else if kind == 6 && device.starts_with("USB\\") {
        "usb"
    } else if kind == 6 && (device.starts_with("PCI\\") || device.starts_with("ACPI\\")) {
        "wifi"
    } else {
        "unknown"
    }
}

#[cfg(windows)]
fn interface_transport(interface: &if_addrs::Interface) -> &'static str {
    use windows_sys::Win32::NetworkManagement::IpHelper::{GetIfEntry2, MIB_IF_ROW2};
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let Some(index) = interface.index else { return "unknown" };
    let mut row = MIB_IF_ROW2 { InterfaceIndex: index, ..Default::default() };
    if unsafe { GetIfEntry2(&mut row) } != 0 { return "unknown"; }
    // The kernel adapter identity is fixed; the editable friendly name is never used.
    let guid = &interface.adapter_name;
    if guid.len() != 38 || !guid.bytes().all(|c| c.is_ascii_hexdigit() || b"{}-".contains(&c)) {
        return "unknown";
    }
    let key: Vec<u16> = format!("SYSTEM\\CurrentControlSet\\Control\\Network\\{{4d36e972-e325-11ce-bfc1-08002be10318}}\\{guid}\\Connection\0").encode_utf16().collect();
    let value: Vec<u16> = "PnpInstanceID\0".encode_utf16().collect();
    let mut data = [0u16; 1024];
    let mut bytes = std::mem::size_of_val(&data) as u32;
    let rc = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ,
        std::ptr::null_mut(), data.as_mut_ptr().cast(), &mut bytes) };
    let device = if rc == 0 {
        String::from_utf16_lossy(&data[..data.iter().position(|&c| c == 0).unwrap_or(data.len())])
    } else { String::new() };
    windows_transport_kind(row.Type, row.PhysicalMediumType, &device)
}

#[cfg(target_os = "linux")]
fn interface_transport(interface: &if_addrs::Interface) -> &'static str {
    let name = &interface.name;
    if name.contains('/') || name == "." || name == ".." { return "unknown"; }
    let path = std::path::Path::new("/sys/class/net").join(name);
    if path.join("wireless").is_dir() { return "wifi"; }
    if let Ok(bus) = std::fs::canonicalize(path.join("device/subsystem")) {
        match bus.file_name().and_then(|v| v.to_str()) {
            Some("usb") => return "usb",
            Some("bluetooth") => return "bluetooth",
            Some("pci") | Some("platform") => return "wifi",
            _ => {}
        }
    }
    // Tether and PAN interfaces can be virtual, with no device symlink.
    match transport_name(name) { "usb" => "usb", "bluetooth" => "bluetooth", _ => "unknown" }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn interface_transport(_: &if_addrs::Interface) -> &'static str { "unknown" }

/// Identify the actual local interface carrying a peer connection.
pub fn transport_for(ip: &IpAddr) -> &'static str {
    if is_tailscale(ip) || ip.is_loopback() {
        return "wifi";
    }
    if let Ok(ifs) = if_addrs::get_if_addrs() {
        if let Some(i) = ifs.iter().find(|i| i.ip() == *ip) {
            return interface_transport(i);
        }
    }
    // An unidentified link must not inherit the ordinary WiFi/LAN permission.
    "unknown"
}

/// Local addresses peers can reach us on (no loopback, no public addresses).
pub fn local_addrs(port: u16) -> Vec<String> {
    let mut out: Vec<(u8, String)> = Vec::new();
    if let Ok(ifs) = if_addrs::get_if_addrs() {
        for i in ifs {
            let ip = i.ip();
            if ip.is_loopback() || !is_allowed_source(&ip) || (is_virtual(&i.name) && !is_tailscale(&ip)) {
                continue;
            }
            if let IpAddr::V6(v6) = ip {
                // Link-local needs a scope id; skip it for stored addresses.
                if (v6.segments()[0] & 0xffc0) == 0xfe80 {
                    continue;
                }
            }
            let rank = if is_tailscale(&ip) { 1 } else { 0 };
            let s = match ip {
                IpAddr::V4(v4) => format!("{v4}:{port}"),
                IpAddr::V6(v6) => format!("[{v6}]:{port}"),
            };
            out.push((rank, s));
        }
    }
    out.sort();
    out.dedup();
    out.into_iter().map(|(_, s)| s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> bool {
        is_allowed_source(&s.parse().unwrap())
    }

    #[test]
    fn allows_lan_and_tailscale() {
        for a in ["192.168.1.50", "10.1.2.3", "172.16.0.1", "100.100.1.2", "127.0.0.1", "fd7a:115c:a1e0::1", "fe80::1", "::1", "::ffff:192.168.1.2"] {
            assert!(ok(a), "{a} should be allowed");
        }
    }

    #[test]
    fn classifies_transports_and_refuses_unknown() {
        assert_eq!(transport_name("mystery0"), "unknown");
        assert_eq!(transport_name("tun0"), "unknown");
        assert_eq!(transport_name("rndis0"), "usb");
        assert_eq!(transport_name("Bluetooth Network Connection"), "bluetooth");
        assert_eq!(transport_name("Ethernet 2"), "wifi");
        assert_eq!(transport_for(&"127.0.0.1".parse().unwrap()), "wifi");
        assert_eq!(transport_for(&"0.0.0.0".parse().unwrap()), "unknown");
    }

    #[cfg(windows)]
    #[test]
    fn classifies_windows_hardware_without_friendly_names() {
        assert_eq!(windows_transport_kind(71, 9, "USB\\RADIO"), "wifi");
        assert_eq!(windows_transport_kind(6, 10, "USB\\RADIO"), "bluetooth");
        assert_eq!(windows_transport_kind(6, 14, "USB\\TETHER"), "usb");
        assert_eq!(windows_transport_kind(6, 14, "PCI\\NETWORK"), "wifi");
        assert_eq!(windows_transport_kind(6, 14, "ROOT\\VIRTUAL"), "unknown");
        assert_eq!(windows_transport_kind(6, 14, ""), "unknown");
        assert_eq!(windows_transport_kind(131, 0, "PCI\\NETWORK"), "unknown");
    }

    #[test]
    fn rejects_public() {
        for a in ["8.8.8.8", "100.63.255.255", "100.128.0.1", "172.32.0.1", "2001:4860::8888", "::ffff:8.8.8.8"] {
            assert!(!ok(a), "{a} should be rejected");
        }
    }
}
