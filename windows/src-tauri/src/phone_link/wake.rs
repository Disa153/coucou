//! What the iPhone needs to switch this computer on again (Wake-on-LAN): the
//! wired card's MAC address, this computer's address and its network's
//! broadcast address. Sent in the welcome to a phone that asks for "wake"
//! (a paired phone only); the phone keeps it for when the computer is off.
//!
//! Read once in the background when the phone link starts: on Windows through
//! PowerShell's Get-NetAdapter, on Linux through `ip` and /sys/class/net.

use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static KNOWN: Mutex<Option<Value>> = Mutex::new(None);
static LOOKING: AtomicBool = AtomicBool::new(false);

/// What was found, if it has been yet.
pub fn known() -> Option<Value> {
    let known = KNOWN.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if known.is_none() {
        refresh();
    }
    known
}

/// Looks it up again in the background (one look at a time).
pub fn refresh() {
    if LOOKING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        let found = lookup();
        if let Some(found) = found {
            *KNOWN.lock().unwrap_or_else(|e| e.into_inner()) = Some(found);
        }
        LOOKING.store(false, Ordering::SeqCst);
    });
}

fn lookup() -> Option<Value> {
    let ip = local_ipv4()?;
    let (mac, prefix) = imp::mac_and_prefix(ip)?;
    let mac = normalize_mac(&mac)?;
    Some(json!({ "mac": mac, "ip": ip.to_string(), "broadcast": broadcast(ip, prefix).to_string() }))
}

/// The address this computer goes out from (no packet is sent: a UDP
/// "connect" only picks the route).
fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_unspecified() => Some(v4),
        _ => None,
    }
}

/// "AA-BB-CC-DD-EE-FF" or "aa:bb:…" → "aa:bb:cc:dd:ee:ff"; None for anything else, or all zeros.
pub fn normalize_mac(raw: &str) -> Option<String> {
    let hex: String = raw.trim().chars().filter(|c| !matches!(c, ':' | '-' | '.')).collect();
    if hex.len() != 12 || !hex.chars().all(|c| c.is_ascii_hexdigit()) || hex.chars().all(|c| c == '0') {
        return None;
    }
    let hex = hex.to_lowercase();
    Some((0..6).map(|i| &hex[i * 2..i * 2 + 2]).collect::<Vec<_>>().join(":"))
}

/// The network's broadcast address for `ip`/`prefix`.
pub fn broadcast(ip: Ipv4Addr, prefix: u8) -> Ipv4Addr {
    let prefix = prefix.min(32);
    let mask: u32 = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
    Ipv4Addr::from(u32::from(ip) | !mask)
}

/// PowerShell's answer: "AA-BB-CC-DD-EE-FF 24".
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_powershell(out: &str) -> Option<(String, u8)> {
    let line = out.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut parts = line.split_whitespace();
    let mac = parts.next()?.to_string();
    let prefix = parts.next()?.parse().ok()?;
    Some((mac, prefix))
}

/// A line of `ip -o -4 addr show`: "2: enp3s0    inet 192.168.1.5/24 brd … " → ("enp3s0", 24) when it is `ip`'s.
#[cfg_attr(windows, allow(dead_code))]
pub fn parse_ip_line(line: &str, ip: Ipv4Addr) -> Option<(String, u8)> {
    let mut parts = line.split_whitespace();
    let _index = parts.next()?;
    let name = parts.next()?.trim_end_matches(':');
    let name = name.split('@').next()?.to_string();
    let mut rest = parts.skip_while(|p| *p != "inet");
    rest.next()?;
    let (addr, prefix) = rest.next()?.split_once('/')?;
    (addr.parse::<Ipv4Addr>().ok()? == ip).then_some(())?;
    Some((name, prefix.parse().ok()?))
}

#[cfg(windows)]
mod imp {
    use std::net::Ipv4Addr;
    use std::process::Command;

    pub fn mac_and_prefix(ip: Ipv4Addr) -> Option<(String, u8)> {
        let script = format!(
            "$a = Get-NetIPAddress -AddressFamily IPv4 -IPAddress '{ip}' -ErrorAction Stop | Select-Object -First 1; \
             $n = Get-NetAdapter -InterfaceIndex $a.InterfaceIndex -ErrorAction Stop; \
             \"$($n.MacAddress) $($a.PrefixLength)\""
        );
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        let out = crate::platform::no_console(&mut cmd).output().ok()?;
        if !out.status.success() {
            crate::log::line("phone link: could not read this computer's network card for Wake-on-LAN");
            return None;
        }
        super::parse_powershell(&String::from_utf8_lossy(&out.stdout))
    }
}

#[cfg(not(windows))]
mod imp {
    use std::net::Ipv4Addr;
    use std::process::Command;

    pub fn mac_and_prefix(ip: Ipv4Addr) -> Option<(String, u8)> {
        let out = Command::new("ip").args(["-o", "-4", "addr", "show"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let (name, prefix) = text.lines().find_map(|l| super::parse_ip_line(l, ip))?;
        let mac = std::fs::read_to_string(format!("/sys/class/net/{name}/address")).ok()?;
        Some((mac.trim().to_string(), prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macs_are_written_one_way() {
        assert_eq!(normalize_mac("AA-BB-CC-0D-EE-FF").as_deref(), Some("aa:bb:cc:0d:ee:ff"));
        assert_eq!(normalize_mac("aa:bb:cc:dd:ee:ff\n").as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(normalize_mac("00-00-00-00-00-00"), None);
        assert_eq!(normalize_mac("AA-BB-CC"), None);
        assert_eq!(normalize_mac("GG-BB-CC-DD-EE-FF"), None);
    }

    #[test]
    fn the_broadcast_address_of_the_network() {
        let ip = Ipv4Addr::new(192, 168, 1, 37);
        assert_eq!(broadcast(ip, 24), Ipv4Addr::new(192, 168, 1, 255));
        assert_eq!(broadcast(ip, 16), Ipv4Addr::new(192, 168, 255, 255));
        assert_eq!(broadcast(ip, 32), ip);
        assert_eq!(broadcast(ip, 0), Ipv4Addr::BROADCAST);
        assert_eq!(broadcast(ip, 40), ip);
    }

    #[test]
    fn powershell_and_ip_answers() {
        assert_eq!(parse_powershell("\r\nD8-BB-C1-11-22-33 24\r\n"), Some(("D8-BB-C1-11-22-33".into(), 24)));
        assert_eq!(parse_powershell(""), None);
        let ip = Ipv4Addr::new(192, 168, 1, 5);
        let line = "2: enp3s0    inet 192.168.1.5/24 brd 192.168.1.255 scope global dynamic enp3s0\\       valid_lft 80000sec";
        assert_eq!(parse_ip_line(line, ip), Some(("enp3s0".into(), 24)));
        assert_eq!(parse_ip_line(line, Ipv4Addr::new(192, 168, 1, 6)), None);
        assert_eq!(parse_ip_line("1: lo    inet 127.0.0.1/8 scope host lo", ip), None);
    }
}
