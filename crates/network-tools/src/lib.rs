//! Network diagnostics. Nothing here runs a shell: ping and traceroute are
//! the OS programs with a fixed argument list, and the target is validated
//! first so it can never be read as an option.

use std::net::IpAddr;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ToolError {
    #[error("invalid {field}: {reason}")]
    Invalid { field: &'static str, reason: String },
    #[error("{0}")]
    Failed(String),
}

/// A host name or IP literal that is safe to pass as a program argument.
pub fn validate_host(host: &str) -> Result<&str, ToolError> {
    let bad = |r: &str| {
        Err(ToolError::Invalid {
            field: "host",
            reason: r.into(),
        })
    };
    let h = host.trim();
    if h.is_empty() || h.len() > 253 {
        return bad("must be 1-253 characters");
    }
    if h.starts_with('-') {
        return bad("must not start with '-'");
    }
    if h.parse::<IpAddr>().is_ok() {
        return Ok(h);
    }
    let label_ok = |l: &str| {
        !l.is_empty()
            && l.len() <= 63
            && l.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    if h.trim_end_matches('.').split('.').all(label_ok) {
        Ok(h)
    } else {
        bad("not a valid host name or IP address")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TcpCheck {
    pub open: bool,
    pub address: Option<String>,
    pub millis: u64,
    pub error: Option<String>,
}

/// Tries a TCP connection; reports latency or why it failed.
pub async fn tcp_check(host: &str, port: u16, timeout: Duration) -> Result<TcpCheck, ToolError> {
    let host = validate_host(host)?;
    if port == 0 {
        return Err(ToolError::Invalid {
            field: "port",
            reason: "must be 1-65535".into(),
        });
    }
    let start = Instant::now();
    let attempt = tokio::time::timeout(timeout, tokio::net::TcpStream::connect((host, port))).await;
    let millis = start.elapsed().as_millis() as u64;
    Ok(match attempt {
        Ok(Ok(s)) => TcpCheck {
            open: true,
            address: s.peer_addr().ok().map(|a| a.to_string()),
            millis,
            error: None,
        },
        Ok(Err(e)) => TcpCheck {
            open: false,
            address: None,
            millis,
            error: Some(e.to_string()),
        },
        Err(_) => TcpCheck {
            open: false,
            address: None,
            millis,
            error: Some("timed out".into()),
        },
    })
}

/// Resolves a name with the system resolver (A and AAAA).
pub async fn dns_lookup(name: &str) -> Result<Vec<String>, ToolError> {
    let name = validate_host(name)?;
    let addrs = tokio::net::lookup_host((name, 0))
        .await
        .map_err(|e| ToolError::Failed(format!("lookup failed: {e}")))?;
    let mut out: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();
    out.sort();
    out.dedup();
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Ping,
    Traceroute,
}

/// Program and arguments for a probe on this OS. Arguments are a list;
/// the host comes last and has been validated.
pub fn probe_command(probe: Probe, host: &str) -> Result<(String, Vec<String>), ToolError> {
    let host = validate_host(host)?.to_owned();
    let (program, mut args): (&str, Vec<&str>) = match (probe, cfg!(windows)) {
        (Probe::Ping, true) => ("ping", vec!["-n", "4"]),
        (Probe::Ping, false) => ("ping", vec!["-c", "4"]),
        (Probe::Traceroute, true) => ("tracert", vec!["-d", "-h", "30"]),
        (Probe::Traceroute, false) => ("traceroute", vec!["-n", "-m", "30"]),
    };
    let mut owned: Vec<String> = args.drain(..).map(str::to_owned).collect();
    owned.push(host);
    Ok((program.to_owned(), owned))
}

/// Runs a probe, sending each output line to `on_line`. Returns the exit
/// code (`None` if cancelled or killed).
pub async fn run_probe(
    probe: Probe,
    host: &str,
    cancel: &CancellationToken,
    mut on_line: impl FnMut(String) + Send,
) -> Result<Option<i32>, ToolError> {
    let (program, args) = probe_command(probe, host)?;
    let mut child = Command::new(&program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| ToolError::Failed(format!("cannot run {program}: {e}")))?;
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let mut out = BufReader::new(stdout).lines();
    let mut err = BufReader::new(stderr).lines();
    let (mut out_done, mut err_done) = (false, false);
    while !(out_done && err_done) {
        tokio::select! {
            () = cancel.cancelled() => {
                let _ = child.kill().await;
                return Ok(None);
            }
            l = out.next_line(), if !out_done => match l {
                Ok(Some(l)) => on_line(l),
                _ => out_done = true,
            },
            l = err.next_line(), if !err_done => match l {
                Ok(Some(l)) => on_line(l),
                _ => err_done = true,
            },
        }
    }
    let status = child
        .wait()
        .await
        .map_err(|e| ToolError::Failed(e.to_string()))?;
    Ok(status.code())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_validation_blocks_option_injection() {
        for ok in [
            "example.com",
            "10.0.0.1",
            "::1",
            "fe80::1",
            "host_name.local",
            "a.b.c.",
            "localhost",
        ] {
            assert!(validate_host(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-c 1000 x",
            "-oProxyCommand=x",
            "a b",
            "a;b",
            "a|b",
            "$(id)",
            "a..b",
            "-x",
            &"a".repeat(254),
        ] {
            assert!(validate_host(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn probe_commands_are_fixed_argument_lists() {
        let (p, a) = probe_command(Probe::Ping, "example.com").unwrap();
        assert_eq!(p, "ping");
        assert_eq!(a.last().unwrap(), "example.com");
        assert!(probe_command(Probe::Traceroute, "-n").is_err());
    }

    #[tokio::test]
    async fn tcp_check_open_and_closed() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let open = tcp_check("127.0.0.1", port, Duration::from_secs(2))
            .await
            .unwrap();
        assert!(open.open && open.error.is_none());
        // A port that was never connected to, then closed. (Reusing `port`
        // is flaky: the earlier connection can still be in the backlog.)
        let unused = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dead = unused.local_addr().unwrap().port();
        drop(unused);
        drop(l);
        let closed = tcp_check("127.0.0.1", dead, Duration::from_secs(2))
            .await
            .unwrap();
        assert!(!closed.open && closed.error.is_some());
        assert!(
            tcp_check("127.0.0.1", 0, Duration::from_secs(1))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn dns_resolves_localhost() {
        let addrs = dns_lookup("localhost").await.unwrap();
        assert!(
            addrs.iter().any(|a| a == "127.0.0.1" || a == "::1"),
            "{addrs:?}"
        );
    }

    #[tokio::test]
    async fn ping_loopback_streams_lines_and_can_be_cancelled() {
        let cancel = CancellationToken::new();
        let mut lines = Vec::new();
        let code = run_probe(Probe::Ping, "127.0.0.1", &cancel, |l| lines.push(l))
            .await
            .unwrap();
        assert_eq!(code, Some(0), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("127.0.0.1")), "{lines:?}");

        let cancel = CancellationToken::new();
        cancel.cancel();
        let code = run_probe(Probe::Ping, "127.0.0.1", &cancel, |_| {})
            .await
            .unwrap();
        assert_eq!(code, None);
    }
}
