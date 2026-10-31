//! Serial console through the hub, with a pseudo-terminal standing in for
//! the device (Linux only; see serial-console's tests for why).
#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_core::TerminalHub;
use serial_console::SerialConfig;
use terminal_pty::TerminalEvent;

#[tokio::test(flavor = "multi_thread")]
async fn serial_console_through_hub() {
    let pty = nix::pty::openpty(None, None).unwrap();
    let path = nix::unistd::ttyname(&pty.slave)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut t = nix::sys::termios::tcgetattr(&pty.master).unwrap();
    nix::sys::termios::cfmakeraw(&mut t);
    nix::sys::termios::tcsetattr(&pty.master, nix::sys::termios::SetArg::TCSANOW, &t).unwrap();
    drop::<OwnedFd>(pty.slave);
    let mut device = std::fs::File::from(pty.master);

    let hub = TerminalHub::with_default_flow();
    let out = Arc::new(Mutex::new(Vec::new()));
    let o = out.clone();
    let (tx, exited) = std::sync::mpsc::channel();
    let id = hub
        .open_serial(&SerialConfig::new(&path), move |ev| {
            match ev {
                TerminalEvent::Output(b) => o.lock().unwrap().extend_from_slice(&b),
                TerminalEvent::Exited(_) => {
                    let _ = tx.send(());
                }
            }
            true
        })
        .unwrap();

    hub.write(id, b"enable\r").await.unwrap();
    let mut buf = [0u8; 7];
    device.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"enable\r");

    device.write_all(b"Switch#").unwrap();
    let start = Instant::now();
    while !String::from_utf8_lossy(&out.lock().unwrap()).contains("Switch#") {
        assert!(start.elapsed() < Duration::from_secs(5));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!hub.is_ssh(id).unwrap());
    assert!(hub.sftp(id).await.is_err());

    hub.close(id).await.unwrap();
    exited
        .recv_timeout(Duration::from_secs(5))
        .expect("Exited after close");
    assert!(hub.is_empty());
}
