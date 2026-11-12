//! Bandwidth test against an iperf3 server (`iperf3 -s`, port 5201): one
//! TCP stream, upload (this computer sends) or download (the server sends,
//! iperf3's `-R`). Our own implementation of the iperf3 control protocol;
//! nothing from iperf3 is bundled.

use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::{ToolError, validate_host};

const TEST_START: u8 = 1;
const TEST_RUNNING: u8 = 2;
const TEST_END: u8 = 4;
const PARAM_EXCHANGE: u8 = 9;
const CREATE_STREAMS: u8 = 10;
const SERVER_TERMINATE: u8 = 11;
const EXCHANGE_RESULTS: u8 = 13;
const DISPLAY_RESULTS: u8 = 14;
const IPERF_DONE: u8 = 16;
/// -1 and -2 as one byte.
const ACCESS_DENIED: u8 = 0xff;
const SERVER_ERROR: u8 = 0xfe;

/// iperf3's default port.
pub const IPERF_PORT: u16 = 5201;
/// Longest test, in seconds.
pub const MAX_IPERF_SECONDS: u32 = 60;
const BLOCK: usize = 128 * 1024;
const STATE_TIMEOUT: Duration = Duration::from_secs(10);
/// Largest results document accepted from the server.
const MAX_JSON: u32 = 1024 * 1024;

/// One second of the test, as measured here.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandwidthSample {
    pub second: u32,
    pub bits_per_second: f64,
}

/// The whole test.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandwidthResult {
    /// Bytes the receiving side counted (the server for uploads).
    pub bytes: u64,
    pub seconds: f64,
    pub bits_per_second: f64,
    pub download: bool,
}

fn failed(what: impl std::fmt::Display) -> ToolError {
    ToolError::Failed(what.to_string())
}

fn cookie() -> [u8; 37] {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut c = [0u8; 37];
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
        | 1;
    for b in c.iter_mut().take(36) {
        // xorshift: the cookie only tells streams apart, it is no secret.
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        *b = CHARS[(seed % CHARS.len() as u64) as usize];
    }
    c
}

async fn read_state(control: &mut TcpStream) -> Result<u8, ToolError> {
    let mut b = [0u8; 1];
    tokio::time::timeout(STATE_TIMEOUT, control.read_exact(&mut b))
        .await
        .map_err(|_| failed("the iperf3 server stopped answering"))?
        .map_err(|e| failed(format!("connection to the iperf3 server lost: {e}")))?;
    match b[0] {
        ACCESS_DENIED => Err(failed("the iperf3 server is busy with another test")),
        SERVER_ERROR => Err(failed("the iperf3 server reported an error")),
        SERVER_TERMINATE => Err(failed("the iperf3 server ended the test")),
        s => Ok(s),
    }
}

async fn expect(control: &mut TcpStream, want: u8) -> Result<(), ToolError> {
    let got = read_state(control).await?;
    if got == want {
        Ok(())
    } else {
        Err(failed(format!(
            "unexpected answer from the iperf3 server (state {got}, expected {want})"
        )))
    }
}

async fn write_json(control: &mut TcpStream, json: &str) -> Result<(), ToolError> {
    let len = u32::try_from(json.len()).map_err(failed)?;
    control
        .write_all(&len.to_be_bytes())
        .await
        .map_err(failed)?;
    control.write_all(json.as_bytes()).await.map_err(failed)
}

async fn read_json(control: &mut TcpStream) -> Result<serde_json::Value, ToolError> {
    let mut len = [0u8; 4];
    tokio::time::timeout(STATE_TIMEOUT, control.read_exact(&mut len))
        .await
        .map_err(|_| failed("no results from the iperf3 server"))?
        .map_err(failed)?;
    let len = u32::from_be_bytes(len);
    if len > MAX_JSON {
        return Err(failed("results from the iperf3 server are too large"));
    }
    let mut buf = vec![0u8; len as usize];
    control.read_exact(&mut buf).await.map_err(failed)?;
    serde_json::from_slice(&buf).map_err(|e| failed(format!("bad results from the server: {e}")))
}

/// Runs a test for `seconds` (1 to 60) and reports every second.
pub async fn bandwidth_test(
    host: &str,
    port: u16,
    seconds: u32,
    download: bool,
    cancel: &CancellationToken,
    mut on_sample: impl FnMut(BandwidthSample) + Send,
) -> Result<BandwidthResult, ToolError> {
    let host = validate_host(host)?;
    if !(1..=MAX_IPERF_SECONDS).contains(&seconds) {
        return Err(ToolError::Invalid {
            field: "duration",
            reason: format!("1 to {MAX_IPERF_SECONDS} seconds"),
        });
    }
    let connect = |what: &'static str| async move {
        tokio::time::timeout(STATE_TIMEOUT, TcpStream::connect((host, port)))
            .await
            .map_err(|_| failed(format!("{what}: no answer from {host}:{port}")))?
            .map_err(|e| failed(format!("{what}: {e}")))
    };
    let cookie = cookie();
    let mut control = connect("cannot reach the iperf3 server").await?;
    control.set_nodelay(true).ok();
    control.write_all(&cookie).await.map_err(failed)?;
    expect(&mut control, PARAM_EXCHANGE).await?;
    let params = serde_json::json!({
        "tcp": true,
        "omit": 0,
        "time": seconds,
        "num": 0,
        "blockcount": 0,
        "parallel": 1,
        "len": BLOCK,
        "reverse": download,
        "pacing_timer": 1000,
        "client_version": "termalith",
    });
    write_json(&mut control, &params.to_string()).await?;
    expect(&mut control, CREATE_STREAMS).await?;
    let mut data = connect("cannot open the data connection").await?;
    data.write_all(&cookie).await.map_err(failed)?;
    expect(&mut control, TEST_START).await?;
    expect(&mut control, TEST_RUNNING).await?;

    // Move data until the time is up (or the user stops), sampling each second.
    let started = Instant::now();
    let end = started + Duration::from_secs(u64::from(seconds));
    let mut total: u64 = 0;
    let mut window: u64 = 0;
    let mut second: u32 = 0;
    let mut next_tick = started + Duration::from_secs(1);
    let mut buf = vec![0u8; BLOCK];
    loop {
        let now = Instant::now();
        if now >= next_tick {
            second += 1;
            on_sample(BandwidthSample {
                second,
                bits_per_second: window as f64 * 8.0,
            });
            window = 0;
            next_tick += Duration::from_secs(1);
        }
        if now >= end || cancel.is_cancelled() {
            break;
        }
        let step = next_tick.min(end).saturating_duration_since(now);
        let moved = if download {
            match tokio::time::timeout(step, data.read(&mut buf)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => n,
                Ok(Err(e)) => return Err(failed(format!("data connection lost: {e}"))),
                Err(_) => 0,
            }
        } else {
            match tokio::time::timeout(step, data.write(&buf)).await {
                Ok(Ok(n)) => n,
                Ok(Err(e)) => return Err(failed(format!("data connection lost: {e}"))),
                Err(_) => 0,
            }
        };
        total += moved as u64;
        window += moved as u64;
    }
    let elapsed = started.elapsed().as_secs_f64().max(0.001);

    control.write_all(&[TEST_END]).await.map_err(failed)?;
    // A download keeps arriving until the server reads TEST_END; drain it
    // so the server is not stuck writing.
    if download {
        let drain = async {
            loop {
                match data.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
        };
        let mut states = [0u8; 1];
        tokio::select! {
            () = drain => {}
            r = control.peek(&mut states) => { let _ = r; }
        }
    }
    expect(&mut control, EXCHANGE_RESULTS).await?;
    let ours = serde_json::json!({
        "cpu_util_total": 0.0,
        "cpu_util_user": 0.0,
        "cpu_util_system": 0.0,
        "sender_has_retransmits": 0,
        "streams": [{
            "id": 1,
            "bytes": total,
            "retransmits": 0,
            "jitter": 0,
            "errors": 0,
            "packets": 0,
            "start_time": 0,
            "end_time": elapsed,
        }],
    });
    write_json(&mut control, &ours.to_string()).await?;
    let theirs = read_json(&mut control).await?;
    expect(&mut control, DISPLAY_RESULTS).await?;
    control.write_all(&[IPERF_DONE]).await.map_err(failed)?;
    let _ = data.shutdown().await;

    // For uploads the server is the receiver: its count is the real one.
    let bytes = if download {
        total
    } else {
        theirs["streams"][0]["bytes"].as_u64().unwrap_or(total)
    };
    Ok(BandwidthResult {
        bytes,
        seconds: elapsed,
        bits_per_second: bytes as f64 * 8.0 / elapsed,
        download,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_have_the_iperf3_shape() {
        let c = cookie();
        assert_eq!(c[36], 0);
        assert!(
            c[..36]
                .iter()
                .all(|b| b"abcdefghijklmnopqrstuvwxyz234567".contains(b))
        );
    }

    #[tokio::test]
    async fn bad_input_is_refused_before_connecting() {
        let cancel = CancellationToken::new();
        assert!(
            bandwidth_test("-x", IPERF_PORT, 5, false, &cancel, |_| {})
                .await
                .is_err()
        );
        assert!(
            bandwidth_test("127.0.0.1", IPERF_PORT, 0, false, &cancel, |_| {})
                .await
                .is_err()
        );
    }
}
