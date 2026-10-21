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
    fn rejects_public() {
        for a in ["8.8.8.8", "100.63.255.255", "100.128.0.1", "172.32.0.1", "2001:4860::8888", "::ffff:8.8.8.8"] {
            assert!(!ok(a), "{a} should be rejected");
        }
    }
}
