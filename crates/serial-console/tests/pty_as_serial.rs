//! A pseudo-terminal pair stands in for a serial device (no hardware
//! needed): the slave side is opened as a serial port, the master side plays
//! the remote console.
//!
//! Linux only: on macOS serialport sets the speed with the IOSSIOSPEED ioctl,
//! which real serial adapters support but pseudo-terminals reject (ENOTTY).
#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::time::{Duration, Instant};

use serial_console::{SerialConfig, open};

fn pty_pair() -> (std::fs::File, String) {
    let pty = nix::pty::openpty(None, None).expect("openpty");
    let path = nix::unistd::ttyname(&pty.slave)
        .expect("ttyname")
        .to_string_lossy()
        .into_owned();
    // Raw mode on the master side so bytes pass unmodified.
    let mut t = nix::sys::termios::tcgetattr(&pty.master).unwrap();
    nix::sys::termios::cfmakeraw(&mut t);
    nix::sys::termios::tcsetattr(&pty.master, nix::sys::termios::SetArg::TCSANOW, &t).unwrap();
    drop::<OwnedFd>(pty.slave); // the serial side reopens it by path
    (std::fs::File::from(pty.master), path)
}

fn read_until(r: &mut dyn Read, want: &[u8]) -> Vec<u8> {
    let start = Instant::now();
    let mut got = Vec::new();
    let mut buf = [0u8; 256];
    while !got.windows(want.len()).any(|w| w == want) {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed out; got {got:?}"
        );
        match r.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => got.extend_from_slice(&buf[..n]),
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("read error: {e}"),
        }
    }
    got
}

#[test]
fn bytes_flow_both_ways_at_115200() {
    let (mut console, path) = pty_pair();
    let mut cfg = SerialConfig::new(&path);
    cfg.baud_rate = 115_200;
    let (mut reader, mut writer) = open(&cfg).expect("open pty slave as serial port");

    writer.write_all(b"show version\r").unwrap();
    writer.flush().unwrap();
    let mut seen = [0u8; 13];
    console.read_exact(&mut seen).unwrap();
    assert_eq!(&seen, b"show version\r");

    console.write_all(b"Router#").unwrap();
    read_until(reader.as_mut(), b"Router#");
}

#[test]
fn reads_time_out_instead_of_blocking() {
    let (_console, path) = pty_pair();
    let (mut reader, _w) = open(&SerialConfig::new(&path)).unwrap();
    let start = Instant::now();
    let mut buf = [0u8; 16];
    let r = reader.read(&mut buf);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(
        matches!(r, Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut),
        "{r:?}"
    );
}
