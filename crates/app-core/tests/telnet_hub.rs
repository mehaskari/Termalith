//! Telnet terminals in the hub against a scripted local server.

use std::time::Duration;

use app_core::TerminalHub;
use terminal_pty::{TerminalEvent, TerminalSize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

const IAC: u8 = 255;
const DO: u8 = 253;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const ECHO: u8 = 1;
const NAWS: u8 = 31;

async fn read_exactly(s: &mut tokio::net::TcpStream, n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    tokio::time::timeout(Duration::from_secs(5), s.read_exact(&mut buf))
        .await
        .expect("server read timed out")
        .unwrap();
    buf
}

#[tokio::test(flavor = "multi_thread")]
async fn telnet_negotiates_types_and_resizes() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        s.write_all(&[IAC, DO, NAWS, IAC, WILL, ECHO])
            .await
            .unwrap();
        s.write_all(b"login: ").await.unwrap();
        // WILL NAWS, the initial size, DO ECHO.
        let hello = read_exactly(&mut s, 3 + 9 + 3).await;
        assert_eq!(
            hello,
            [
                IAC, WILL, NAWS, IAC, SB, NAWS, 0, 80, 0, 24, IAC, SE, IAC, DO, ECHO
            ]
        );
        // Enter is sent as CR LF.
        assert_eq!(read_exactly(&mut s, 7).await, b"admin\r\n");
        // A resize sends the new size.
        assert_eq!(
            read_exactly(&mut s, 9).await,
            [IAC, SB, NAWS, 0, 100, 0, 30, IAC, SE]
        );
        s.write_all(b"bye\r\n").await.unwrap();
        // Hang up.
    });

    let hub = TerminalHub::with_default_flow();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let id = hub
        .open_telnet(
            "127.0.0.1",
            port,
            TerminalSize::new(80, 24).unwrap(),
            move |ev: TerminalEvent| tx.send(ev).is_ok(),
        )
        .await
        .unwrap();

    let mut text = Vec::new();
    let mut exited = false;
    let mut typed = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !exited {
        let ev = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("events timed out")
            .expect("channel open");
        match ev {
            TerminalEvent::Output(b) => {
                hub.ack(id, b.len()).ok();
                text.extend_from_slice(&b);
                if !typed && text.ends_with(b"login: ") {
                    typed = true;
                    hub.write(id, b"admin\r").await.unwrap();
                    hub.resize(id, TerminalSize::new(100, 30).unwrap())
                        .await
                        .unwrap();
                }
            }
            TerminalEvent::Exited(_) => exited = true,
        }
    }
    server.await.unwrap();
    assert_eq!(String::from_utf8_lossy(&text), "login: bye\r\n");
    // Negotiation bytes never reach the terminal.
    assert!(!text.contains(&IAC));
}

#[tokio::test]
async fn telnet_connection_refused_is_an_error() {
    let unused = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = unused.local_addr().unwrap().port();
    drop(unused);
    let hub = TerminalHub::with_default_flow();
    let r = hub
        .open_telnet(
            "127.0.0.1",
            port,
            TerminalSize::new(80, 24).unwrap(),
            |_ev: TerminalEvent| true,
        )
        .await;
    assert!(
        matches!(r, Err(app_core::AppError::Network(_))),
        "{:?}",
        r.err()
    );
}
