//! LAN scan: finds hosts on a local subnet without raw sockets or admin
//! rights. Each address gets TCP connection attempts to a few common ports;
//! a host that accepts or refuses (RST) is alive. Hardware addresses come
//! from the system's ARP table afterwards (no vendor names: the IEEE list's
//! redistribution terms are unclear).

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::ToolError;

/// Ports tried on every address (SSH, Telnet, HTTP(S), SMB, RDP, VNC…).
pub const LAN_PORTS: [u16; 10] = [22, 23, 80, 443, 445, 139, 3389, 5900, 8080, 8443];
/// Largest subnet scanned at once (/22: 1022 hosts).
pub const MIN_LAN_PREFIX: u8 = 22;
/// Addresses probed at the same time.
const LAN_CONCURRENCY: usize = 96;

/// A host that answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanHost {
    pub address: String,
    /// Ports that accepted a connection.
    pub open_ports: Vec<u16>,
}

fn invalid(reason: impl Into<String>) -> ToolError {
    ToolError::Invalid {
        field: "subnet",
        reason: reason.into(),
    }
}

/// Parses `192.168.1.0/24` (prefix 22 to 30) into its host addresses.
pub fn subnet_hosts(spec: &str) -> Result<Vec<Ipv4Addr>, ToolError> {
    let (ip, prefix) = spec
        .trim()
        .split_once('/')
        .ok_or_else(|| invalid("use an IPv4 subnet like 192.168.1.0/24"))?;
    let ip: Ipv4Addr = ip
        .trim()
        .parse()
        .map_err(|_| invalid("use an IPv4 subnet like 192.168.1.0/24"))?;
    let prefix: u8 = prefix
        .trim()
        .parse()
        .map_err(|_| invalid("the prefix must be a number"))?;
    if !(MIN_LAN_PREFIX..=30).contains(&prefix) {
        return Err(invalid(format!(
            "the prefix must be /{MIN_LAN_PREFIX} to /30 (at most 1022 hosts)"
        )));
    }
    let mask = u32::MAX << (32 - prefix);
    let network = u32::from(ip) & mask;
    let broadcast = network | !mask;
    Ok((network + 1..broadcast).map(Ipv4Addr::from).collect())
}

/// This computer's subnet as a /24 guess (`192.168.1.0/24`), from the
/// address used for outgoing traffic. Sends nothing.
pub fn local_subnet_guess() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_unspecified() => {
            let o = v4.octets();
            Some(format!("{}.{}.{}.0/24", o[0], o[1], o[2]))
        }
        _ => None,
    }
}

/// Probes every host of `hosts` and reports each one that answers.
pub async fn scan_lan(
    hosts: Vec<Ipv4Addr>,
    timeout: Duration,
    cancel: &CancellationToken,
    mut on_host: impl FnMut(LanHost) + Send,
) {
    let slots = Arc::new(Semaphore::new(LAN_CONCURRENCY));
    let mut tasks = JoinSet::new();
    for ip in hosts {
        let permit = tokio::select! {
            () = cancel.cancelled() => break,
            p = Arc::clone(&slots).acquire_owned() => match p {
                Ok(p) => p,
                Err(_) => break,
            },
        };
        tasks.spawn(async move {
            let result = probe_host(ip, timeout).await;
            drop(permit);
            result
        });
        while let Some(Ok(found)) = tasks.try_join_next() {
            if let Some(h) = found {
                on_host(h);
            }
        }
    }
    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                tasks.abort_all();
                break;
            }
            next = tasks.join_next() => match next {
                Some(Ok(Some(h))) => on_host(h),
                Some(_) => {}
                None => break,
            },
        }
    }
}

async fn probe_host(ip: Ipv4Addr, timeout: Duration) -> Option<LanHost> {
    let mut probes = JoinSet::new();
    for port in LAN_PORTS {
        probes.spawn(async move {
            let r = tokio::time::timeout(
                timeout,
                tokio::net::TcpStream::connect(SocketAddr::from((ip, port))),
            )
            .await;
            match r {
                Ok(Ok(_)) => (port, true, true),
                // Refused: something answered, so the host is up.
                Ok(Err(e)) if e.kind() == ErrorKind::ConnectionRefused => (port, false, true),
                _ => (port, false, false),
            }
        });
    }
    let mut open = Vec::new();
    let mut alive = false;
    while let Some(Ok((port, is_open, answered))) = probes.join_next().await {
        alive |= answered;
        if is_open {
            open.push(port);
        }
    }
    open.sort_unstable();
    alive.then(|| LanHost {
        address: ip.to_string(),
        open_ports: open,
    })
}

/// IPv4 → hardware address from the system's ARP table (best effort).
pub async fn arp_table() -> HashMap<String, String> {
    #[cfg(target_os = "linux")]
    {
        tokio::fs::read_to_string("/proc/net/arp")
            .await
            .map(|t| parse_arp(&t))
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "linux"))]
    {
        // A fixed program and argument, no input from anywhere.
        tokio::process::Command::new("arp")
            .arg("-a")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map(|o| parse_arp(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default()
    }
}

/// Reads `arp -a` (macOS, BSD, Windows) and `/proc/net/arp` (Linux) output:
/// every line with an IPv4 address and a hardware address.
pub fn parse_arp(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let words: Vec<&str> = line
            .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
            .filter(|w| !w.is_empty())
            .collect();
        let ip = words.iter().find_map(|w| w.parse::<Ipv4Addr>().ok());
        let mac = words.iter().find_map(|w| normalize_mac(w));
        if let (Some(ip), Some(mac)) = (ip, mac) {
            out.insert(ip.to_string(), mac);
        }
    }
    out
}

/// `a4-2b-b0-1:2:3`… → `a4:2b:b0:01:02:03`; None for anything else or an
/// all-zero/broadcast address.
fn normalize_mac(word: &str) -> Option<String> {
    let parts: Vec<&str> = word.split([':', '-']).collect();
    if parts.len() != 6 || parts.iter().any(|p| p.is_empty() || p.len() > 2) {
        return None;
    }
    let bytes: Vec<u8> = parts
        .iter()
        .map(|p| u8::from_str_radix(p, 16))
        .collect::<Result<_, _>>()
        .ok()?;
    if bytes.iter().all(|b| *b == 0) || bytes.iter().all(|b| *b == 0xff) {
        return None;
    }
    Some(
        bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subnets_list_their_hosts_and_stay_small() {
        let hosts = subnet_hosts("192.168.1.77/24").unwrap();
        assert_eq!(hosts.len(), 254);
        assert_eq!(hosts[0], Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(hosts[253], Ipv4Addr::new(192, 168, 1, 254));
        assert_eq!(subnet_hosts("10.0.0.0/30").unwrap().len(), 2);
        assert_eq!(subnet_hosts("10.0.0.0/22").unwrap().len(), 1022);
        assert!(subnet_hosts("10.0.0.0/16").is_err());
        assert!(subnet_hosts("10.0.0.0").is_err());
        assert!(subnet_hosts("host/24").is_err());
    }

    #[test]
    fn arp_output_of_each_system_is_read() {
        let mac = "? (192.168.1.1) at a4:2b:b0:1:2:3 on en0 ifscope [ethernet]\n\
                   ? (192.168.1.255) at ff:ff:ff:ff:ff:ff on en0 [ethernet]";
        let win = "Interface: 192.168.1.20 --- 0x4\n  Internet Address      Physical Address      Type\n  192.168.1.1           a4-2b-b0-01-02-03     dynamic";
        let linux = "IP address       HW type     Flags       HW address            Mask     Device\n\
                     192.168.1.1      0x1         0x2         a4:2b:b0:01:02:03     *        eth0\n\
                     192.168.1.9      0x1         0x0         00:00:00:00:00:00     *        eth0";
        for text in [mac, win, linux] {
            let t = parse_arp(text);
            assert_eq!(
                t.get("192.168.1.1").map(String::as_str),
                Some("a4:2b:b0:01:02:03")
            );
            assert_eq!(t.len(), 1, "{t:?}");
        }
    }

    #[tokio::test]
    async fn a_listening_host_is_found_with_its_open_port() {
        // Loopback answers on every port (refused or accepted).
        let found = probe_host(Ipv4Addr::LOCALHOST, Duration::from_secs(2)).await;
        assert!(found.is_some());
    }
}
