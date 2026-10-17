//! What the iPhone needs to switch this computer on again (Wake-on-LAN): the
//! wired card's MAC address, this computer's address and its network's
//! broadcast address. Sent in the welcome to a phone that asks for "wake"
//! (a paired phone only); the phone keeps it for when the computer is off.
//!
//! The card is the one holding the address the phone reached (not the route to
//! the internet, which a VPN's virtual adapter may own); failing that, on
//! Windows, the wired card that is up. Read on Windows through PowerShell's
//! Get-NetAdapter, on Linux through `ip` and /sys/class/net.

use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static KNOWN: Mutex<Option<Value>> = Mutex::new(None);
static LOOKING: AtomicBool = AtomicBool::new(false);
/// The address on this computer a phone last reached over the local network.
static LOCAL: Mutex<Option<Ipv4Addr>> = Mutex::new(None);

/// A phone reached this computer at `ip` (the accepted connection's own address).
pub fn seen_local(ip: IpAddr) {
    let IpAddr::V4(v4) = ip else { return };
    if v4.is_loopback() || v4.is_unspecified() || !v4.is_private() {
        return;
    }
    let changed = LOCAL.lock().unwrap_or_else(|e| e.into_inner()).replace(v4) != Some(v4);
    let known_ip = KNOWN.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|k| k["ip"].as_str().map(str::to_string));
    if changed && known_ip.as_deref() != Some(v4.to_string().as_str()) {
        *KNOWN.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// For the welcome: what is known, else a look now (a few seconds at most, first time only).
pub async fn for_welcome() -> Option<Value> {
    if let Some(known) = KNOWN.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Some(known);
    }
    let look = tokio::task::spawn_blocking(lookup);
    let found = tokio::time::timeout(std::time::Duration::from_secs(8), look).await.ok()?.ok()??;
    *KNOWN.lock().unwrap_or_else(|e| e.into_inner()) = Some(found.clone());
    Some(found)
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
    let seen = *LOCAL.lock().unwrap_or_else(|e| e.into_inner());
    let ip = seen.or_else(local_ipv4)?;
    let Some((mac, prefix, card_ip)) = imp::mac_and_prefix(ip) else {
        crate::log::line(format!("phone link: no network card found for Wake-on-LAN (address {ip})"));
        return None;
    };
    let Some(mac) = normalize_mac(&mac) else {
        crate::log::line(format!("phone link: the card for {ip} has no usable MAC address for Wake-on-LAN"));
        return None;
    };
    let ip = card_ip.unwrap_or(ip);
    crate::log::line(format!("phone link: Wake-on-LAN card {mac} ({ip}/{prefix})"));
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

/// PowerShell's answer: "AA-BB-CC-DD-EE-FF 24 192.168.1.5" (the address may be missing).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn parse_powershell(out: &str) -> Option<(String, u8, Option<Ipv4Addr>)> {
    let line = out.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut parts = line.split_whitespace();
    let mac = parts.next()?.to_string();
    let prefix = parts.next()?.parse().ok()?;
    let ip = parts.next().and_then(|p| p.parse().ok());
    Some((mac, prefix, ip))
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

    /// The card holding `ip`; if that one is virtual or has no MAC (a VPN's tunnel), the wired card that is up.
    pub fn mac_and_prefix(ip: Ipv4Addr) -> Option<(String, u8, Option<Ipv4Addr>)> {
        let script = format!(
            "$a = Get-NetIPAddress -AddressFamily IPv4 -IPAddress '{ip}' -ErrorAction SilentlyContinue | Select-Object -First 1; \
             $n = $null; if ($a) {{ $n = Get-NetAdapter -InterfaceIndex $a.InterfaceIndex -ErrorAction SilentlyContinue }}; \
             if (-not $n -or -not $n.MacAddress -or $n.Virtual) {{ \
               $n = Get-NetAdapter -Physical -ErrorAction SilentlyContinue | Where-Object {{ $_.Status -eq 'Up' -and $_.MediaType -eq '802.3' }} | Select-Object -First 1; \
               if ($n) {{ $a = Get-NetIPAddress -AddressFamily IPv4 -InterfaceIndex $n.InterfaceIndex -ErrorAction SilentlyContinue | Select-Object -First 1 }} \
             }}; \
             if ($n -and $a) {{ \"$($n.MacAddress) $($a.PrefixLength) $($a.IPAddress)\" }}"
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

    pub fn mac_and_prefix(ip: Ipv4Addr) -> Option<(String, u8, Option<Ipv4Addr>)> {
        let out = Command::new("ip").args(["-o", "-4", "addr", "show"]).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let (name, prefix) = text.lines().find_map(|l| super::parse_ip_line(l, ip))?;
        let mac = std::fs::read_to_string(format!("/sys/class/net/{name}/address")).ok()?;
        Some((mac.trim().to_string(), prefix, Some(ip)))
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
        assert_eq!(parse_powershell("\r\nD8-BB-C1-11-22-33 24\r\n"), Some(("D8-BB-C1-11-22-33".into(), 24, None)));
        assert_eq!(
            parse_powershell("D8-BB-C1-11-22-33 24 192.168.1.5\r\n"),
            Some(("D8-BB-C1-11-22-33".into(), 24, Some(Ipv4Addr::new(192, 168, 1, 5))))
        );
        assert_eq!(parse_powershell(""), None);
        let ip = Ipv4Addr::new(192, 168, 1, 5);
        let line = "2: enp3s0    inet 192.168.1.5/24 brd 192.168.1.255 scope global dynamic enp3s0\\       valid_lft 80000sec";
        assert_eq!(parse_ip_line(line, ip), Some(("enp3s0".into(), 24)));
        assert_eq!(parse_ip_line(line, Ipv4Addr::new(192, 168, 1, 6)), None);
        assert_eq!(parse_ip_line("1: lo    inet 127.0.0.1/8 scope host lo", ip), None);
    }
}
