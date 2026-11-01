//! TCP port scanning (connect scan, bounded) and Wake-on-LAN.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::{ToolError, validate_host};

/// Most ports one scan may cover.
pub const MAX_SCAN_PORTS: usize = 1024;
/// Connections attempted at the same time.
pub const SCAN_CONCURRENCY: usize = 64;

fn invalid(field: &'static str, reason: impl Into<String>) -> ToolError {
    ToolError::Invalid {
        field,
        reason: reason.into(),
    }
}

/// Parses "22,80,443,8000-8010" into sorted, unique ports.
pub fn parse_ports(spec: &str) -> Result<Vec<u16>, ToolError> {
    let mut out = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (part, part),
        };
        let parse = |s: &str| {
            s.parse::<u16>()
                .ok()
                .filter(|p| *p > 0)
                .ok_or_else(|| invalid("ports", format!("{s:?} is not a port (1-65535)")))
        };
        let (a, b) = (parse(a)?, parse(b)?);
        if a > b {
            return Err(invalid("ports", format!("range {part} is reversed")));
        }
        if usize::from(b - a) >= MAX_SCAN_PORTS {
            return Err(invalid("ports", format!("at most {MAX_SCAN_PORTS} ports")));
        }
        out.extend(a..=b);
        if out.len() > MAX_SCAN_PORTS * 2 {
            break; // de-duplication below decides; avoid unbounded growth
        }
    }
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        return Err(invalid("ports", "no ports given"));
    }
    if out.len() > MAX_SCAN_PORTS {
        return Err(invalid("ports", format!("at most {MAX_SCAN_PORTS} ports")));
    }
    Ok(out)
}

/// Tries a TCP connection to each port (at most [`SCAN_CONCURRENCY`] at a
/// time) and reports every result. Returns the resolved address scanned;
/// stops early when cancelled.
pub async fn scan_ports(
    host: &str,
    ports: &[u16],
    timeout: Duration,
    cancel: &CancellationToken,
    mut on_result: impl FnMut(u16, bool) + Send,
) -> Result<IpAddr, ToolError> {
    let host = validate_host(host)?;
    let addr = tokio::net::lookup_host((host, 0))
        .await
        .map_err(|e| ToolError::Failed(format!("lookup failed: {e}")))?
        .next()
        .ok_or_else(|| ToolError::Failed("the name has no address".into()))?
        .ip();
    let slots = Arc::new(Semaphore::new(SCAN_CONCURRENCY));
    let mut tasks = JoinSet::new();
    for &port in ports {
        let permit = tokio::select! {
            () = cancel.cancelled() => break,
            p = Arc::clone(&slots).acquire_owned() => p.map_err(|e| ToolError::Failed(e.to_string()))?,
        };
        tasks.spawn(async move {
            let open = tokio::time::timeout(
                timeout,
                tokio::net::TcpStream::connect(SocketAddr::new(addr, port)),
            )
            .await
            .is_ok_and(|r| r.is_ok());
            drop(permit);
            (port, open)
        });
        // Report finished ones as we go.
        while let Some(Ok((p, open))) = tasks.try_join_next() {
            on_result(p, open);
        }
    }
    loop {
        tokio::select! {
            () = cancel.cancelled() => {
                tasks.abort_all();
                break;
            }
            next = tasks.join_next() => match next {
                Some(Ok((p, open))) => on_result(p, open),
                Some(Err(_)) => {}
                None => break,
            },
        }
    }
    Ok(addr)
}

/// Parses a MAC address (`aa:bb:cc:dd:ee:ff`, `aa-bb-…` or `aabbccddeeff`).
pub fn parse_mac(mac: &str) -> Result<[u8; 6], ToolError> {
    let hex: String = mac
        .chars()
        .filter(|c| !matches!(c, ':' | '-' | '.'))
        .collect();
    if hex.len() != 12 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid(
            "MAC address",
            "use six hex pairs, e.g. aa:bb:cc:dd:ee:ff",
        ));
    }
    let mut out = [0u8; 6];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| invalid("MAC address", "not hex"))?;
    }
    Ok(out)
}

/// The Wake-on-LAN magic packet: 6 × 0xFF, then the MAC 16 times.
pub fn magic_packet(mac: [u8; 6]) -> Vec<u8> {
    let mut p = vec![0xFF; 6];
    for _ in 0..16 {
        p.extend_from_slice(&mac);
    }
    p
}

/// Sends a magic packet to UDP port 9 on `broadcast` (default
/// 255.255.255.255; a subnet broadcast like 192.168.1.255 is more reliable).
pub async fn wake_on_lan(mac: &str, broadcast: Option<&str>) -> Result<(), ToolError> {
    let mac = parse_mac(mac)?;
    let target: Ipv4Addr = match broadcast.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => b.parse().map_err(|_| {
            invalid(
                "broadcast address",
                "use an IPv4 address such as 192.168.1.255",
            )
        })?,
        None => Ipv4Addr::BROADCAST,
    };
    let sock = tokio::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .await
        .map_err(|e| ToolError::Failed(e.to_string()))?;
    sock.set_broadcast(true)
        .map_err(|e| ToolError::Failed(e.to_string()))?;
    sock.send_to(&magic_packet(mac), (target, 9))
        .await
        .map_err(|e| ToolError::Failed(format!("could not send: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_specs() {
        assert_eq!(parse_ports("443, 22,80,22").unwrap(), [22, 80, 443]);
        assert_eq!(parse_ports("8000-8003").unwrap(), [8000, 8001, 8002, 8003]);
        assert!(parse_ports("").is_err());
        assert!(parse_ports("0").is_err());
        assert!(parse_ports("70000").is_err());
        assert!(parse_ports("10-5").is_err());
        assert!(parse_ports("1-65535").is_err(), "too many");
        assert_eq!(parse_ports("1-1024").unwrap().len(), 1024);
    }

    #[test]
    fn mac_and_magic_packet() {
        let m = parse_mac("AA-bb:cc.dd:ee:ff").unwrap();
        assert_eq!(m, [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        assert!(parse_mac("aa:bb").is_err());
        assert!(parse_mac("zz:bb:cc:dd:ee:ff").is_err());
        let p = magic_packet(m);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[0xff; 6]);
        assert_eq!(&p[96..], &m);
    }

    #[tokio::test]
    async fn scan_finds_the_open_port_only() {
        let open = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let open_port = open.local_addr().unwrap().port();
        let reserved = tokio::net::TcpSocket::new_v4().unwrap();
        reserved.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let closed_port = reserved.local_addr().unwrap().port();
        let mut results = Vec::new();
        let addr = scan_ports(
            "127.0.0.1",
            &[open_port, closed_port],
            Duration::from_secs(2),
            &CancellationToken::new(),
            |p, o| results.push((p, o)),
        )
        .await
        .unwrap();
        results.sort();
        let mut want = vec![(open_port, true), (closed_port, false)];
        want.sort();
        assert_eq!(results, want);
        assert_eq!(addr, IpAddr::from([127, 0, 0, 1]));
        assert!(
            scan_ports(
                "-x",
                &[1],
                Duration::from_secs(1),
                &CancellationToken::new(),
                |_, _| {}
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn cancelled_scan_stops() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut n = 0;
        scan_ports(
            "127.0.0.1",
            &(1..=500).collect::<Vec<_>>(),
            Duration::from_secs(1),
            &cancel,
            |_, _| n += 1,
        )
        .await
        .unwrap();
        assert!(n < 500);
    }
}
