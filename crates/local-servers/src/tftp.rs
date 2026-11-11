//! TFTP server: RFC 1350 with the option extension (RFC 2347) for block
//! size (RFC 2348), transfer size and timeout (RFC 2349). Each transfer
//! runs on its own UDP port, as the protocol defines. `netascii` is served
//! byte for byte (devices that ask for it accept that in practice).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{Action, PathError, ServerError, Shared, display_path, resolve_existing, resolve_new};

const RRQ: u16 = 1;
const WRQ: u16 = 2;
const DATA: u16 = 3;
const ACK: u16 = 4;
const ERROR: u16 = 5;
const OACK: u16 = 6;

const NOT_FOUND: u16 = 1;
const ACCESS: u16 = 2;
const ILLEGAL: u16 = 4;
const EXISTS: u16 = 6;

const DEFAULT_BLOCK: usize = 512;
const MAX_BLOCK: usize = 65464;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);
const RETRIES: u32 = 5;
/// Transfers at once; more requests get a "busy" error.
const MAX_TRANSFERS: usize = 64;
/// Largest upload accepted (block numbers would also wrap many times).
const MAX_UPLOAD: u64 = 4 * 1024 * 1024 * 1024;

pub(crate) async fn serve(
    addr: SocketAddr,
    shared: Arc<Shared>,
    stop: CancellationToken,
) -> Result<(SocketAddr, JoinHandle<()>), ServerError> {
    let socket = UdpSocket::bind(addr)
        .await
        .map_err(|source| ServerError::Listen { addr, source })?;
    let local = socket
        .local_addr()
        .map_err(|source| ServerError::Listen { addr, source })?;
    let slots = Arc::new(Semaphore::new(MAX_TRANSFERS));
    let task = tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        loop {
            let (n, peer) = tokio::select! {
                () = stop.cancelled() => return,
                r = socket.recv_from(&mut buf) => match r {
                    Ok(r) => r,
                    // ICMP "port unreachable" from an earlier reply shows up
                    // here on Windows; keep serving.
                    Err(_) => continue,
                },
            };
            let Some(request) = Request::parse(&buf[..n]) else {
                let _ = socket
                    .send_to(&error_packet(ILLEGAL, "not a TFTP request"), peer)
                    .await;
                continue;
            };
            let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else {
                let _ = socket.send_to(&error_packet(0, "server busy"), peer).await;
                continue;
            };
            let (shared, stop) = (Arc::clone(&shared), stop.clone());
            tokio::spawn(async move {
                let _permit = permit;
                tokio::select! {
                    () = stop.cancelled() => {}
                    () = transfer(request, peer, local, &shared) => {}
                }
            });
        }
    });
    Ok((local, task))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    write: bool,
    path: String,
    blksize: Option<usize>,
    timeout: Option<u64>,
    /// `Some(0)` on a read request asks for the size; on a write it is the
    /// announced size.
    tsize: Option<u64>,
}

impl Request {
    fn parse(p: &[u8]) -> Option<Self> {
        let op = u16::from_be_bytes([*p.first()?, *p.get(1)?]);
        let write = match op {
            RRQ => false,
            WRQ => true,
            _ => return None,
        };
        let mut fields = p[2..].split(|b| *b == 0);
        let path = String::from_utf8(fields.next()?.to_vec()).ok()?;
        let mode = String::from_utf8(fields.next()?.to_vec())
            .ok()?
            .to_ascii_lowercase();
        if mode != "octet" && mode != "netascii" {
            return None;
        }
        let mut req = Request {
            write,
            path,
            blksize: None,
            timeout: None,
            tsize: None,
        };
        // Options come in name/value pairs; unknown ones are ignored.
        while let (Some(name), Some(value)) = (fields.next(), fields.next()) {
            let (Ok(name), Ok(value)) = (std::str::from_utf8(name), std::str::from_utf8(value))
            else {
                continue;
            };
            let Ok(n) = value.parse::<u64>() else {
                continue;
            };
            match name.to_ascii_lowercase().as_str() {
                "blksize" if (8..=65464).contains(&n) => req.blksize = Some(n as usize),
                "timeout" if (1..=255).contains(&n) => req.timeout = Some(n),
                "tsize" => req.tsize = Some(n),
                _ => {}
            }
        }
        Some(req)
    }
}

fn error_packet(code: u16, msg: &str) -> Vec<u8> {
    let mut p = Vec::with_capacity(5 + msg.len());
    p.extend_from_slice(&ERROR.to_be_bytes());
    p.extend_from_slice(&code.to_be_bytes());
    p.extend_from_slice(msg.as_bytes());
    p.push(0);
    p
}

fn oack(options: &[(&str, u64)]) -> Vec<u8> {
    let mut p = OACK.to_be_bytes().to_vec();
    for (name, value) in options {
        p.extend_from_slice(name.as_bytes());
        p.push(0);
        p.extend_from_slice(value.to_string().as_bytes());
        p.push(0);
    }
    p
}

/// One read or write, on its own socket connected to the client.
async fn transfer(req: Request, peer: SocketAddr, server: SocketAddr, shared: &Shared) {
    let action = if req.write {
        Action::Upload
    } else {
        Action::Download
    };
    let shown = display_path(&req.path);
    let Ok(socket) = UdpSocket::bind(SocketAddr::new(server.ip(), 0)).await else {
        return;
    };
    if socket.connect(peer).await.is_err() {
        return;
    }
    let result = if req.write {
        receive(&req, &socket, shared).await
    } else {
        send(&req, &socket, shared).await
    };
    match result {
        Ok(bytes) => shared.report(peer, action, &shown, bytes, None),
        Err(Failure {
            code,
            message,
            sent,
        }) => {
            if let Some(code) = code {
                let _ = socket.send(&error_packet(code, &message)).await;
            }
            shared.report(peer, action, &shown, sent, Some(message));
        }
    }
}

struct Failure {
    /// Error code to send to the client; `None` when the client is gone.
    code: Option<u16>,
    message: String,
    sent: u64,
}

impl Failure {
    fn tell(code: u16, message: impl Into<String>) -> Self {
        Failure {
            code: Some(code),
            message: message.into(),
            sent: 0,
        }
    }
    fn gone(message: &str, sent: u64) -> Self {
        Failure {
            code: None,
            message: message.to_owned(),
            sent,
        }
    }
}

fn path_failure(e: PathError) -> Failure {
    match e {
        PathError::NotFound => Failure::tell(NOT_FOUND, "file not found"),
        PathError::Outside => Failure::tell(ACCESS, "access denied"),
    }
}

/// Sends `packet` and waits for a reply that `accept` takes, resending on
/// timeout. Returns the accepted reply's value.
async fn exchange<T>(
    socket: &UdpSocket,
    packet: &[u8],
    timeout: Duration,
    mut accept: impl FnMut(&[u8]) -> Reply<T>,
) -> Result<T, &'static str> {
    let mut buf = vec![0u8; MAX_BLOCK + 4];
    for _ in 0..=RETRIES {
        if socket.send(packet).await.is_err() {
            return Err("client unreachable");
        }
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match tokio::time::timeout_at(deadline, socket.recv(&mut buf)).await {
                Err(_) => break, // resend
                Ok(Err(_)) => return Err("client unreachable"),
                Ok(Ok(n)) => match accept(&buf[..n]) {
                    Reply::Done(v) => return Ok(v),
                    Reply::Ignore => {}
                    Reply::Abort(why) => return Err(why),
                },
            }
        }
    }
    Err("client stopped answering")
}

enum Reply<T> {
    Done(T),
    /// A duplicate or stray packet: keep waiting.
    Ignore,
    Abort(&'static str),
}

fn opcode(p: &[u8]) -> Option<u16> {
    Some(u16::from_be_bytes([*p.first()?, *p.get(1)?]))
}

fn block_of(p: &[u8]) -> Option<u16> {
    Some(u16::from_be_bytes([*p.get(2)?, *p.get(3)?]))
}

/// Read request: send the file block by block.
async fn send(req: &Request, socket: &UdpSocket, shared: &Shared) -> Result<u64, Failure> {
    let path = resolve_existing(&shared.root, &req.path).map_err(path_failure)?;
    let meta = tokio::fs::metadata(&path)
        .await
        .map_err(|_| Failure::tell(NOT_FOUND, "file not found"))?;
    if !meta.is_file() {
        return Err(Failure::tell(ACCESS, "not a file"));
    }
    let mut file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| Failure::tell(ACCESS, "cannot read the file"))?;
    let block = req.blksize.unwrap_or(DEFAULT_BLOCK);
    let timeout = req.timeout.map_or(DEFAULT_TIMEOUT, Duration::from_secs);

    let mut options = Vec::new();
    if let Some(b) = req.blksize {
        options.push(("blksize", b as u64));
    }
    if let Some(t) = req.timeout {
        options.push(("timeout", t));
    }
    if req.tsize.is_some() {
        options.push(("tsize", meta.len()));
    }
    if !options.is_empty() {
        // The client acknowledges the options with ACK 0.
        exchange(socket, &oack(&options), timeout, |p| {
            match (opcode(p), block_of(p)) {
                (Some(ACK), Some(0)) => Reply::Done(()),
                (Some(ERROR), _) => Reply::Abort("client refused the options"),
                _ => Reply::Ignore,
            }
        })
        .await
        .map_err(|why| Failure::gone(why, 0))?;
    }

    let mut sent = 0u64;
    let mut number: u16 = 1;
    let mut data = vec![0u8; block];
    loop {
        let n = read_full(&mut file, &mut data)
            .await
            .map_err(|_| Failure::tell(0, "read error"))?;
        let mut packet = Vec::with_capacity(4 + n);
        packet.extend_from_slice(&DATA.to_be_bytes());
        packet.extend_from_slice(&number.to_be_bytes());
        packet.extend_from_slice(&data[..n]);
        let want = number;
        exchange(socket, &packet, timeout, |p| {
            match (opcode(p), block_of(p)) {
                (Some(ACK), Some(b)) if b == want => Reply::Done(()),
                (Some(ERROR), _) => Reply::Abort("client cancelled"),
                _ => Reply::Ignore,
            }
        })
        .await
        .map_err(|why| Failure::gone(why, sent))?;
        sent += n as u64;
        if n < block {
            return Ok(sent);
        }
        number = number.wrapping_add(1);
    }
}

/// Fills `buf` unless the file ends first; returns the bytes read.
async fn read_full(file: &mut tokio::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = file.read(&mut buf[filled..]).await?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

/// Write request: receive a new file (uploads must be allowed; an existing
/// file is never replaced).
async fn receive(req: &Request, socket: &UdpSocket, shared: &Shared) -> Result<u64, Failure> {
    if !shared.allow_uploads {
        return Err(Failure::tell(ACCESS, "uploads are turned off"));
    }
    if req.tsize.is_some_and(|t| t > MAX_UPLOAD) {
        return Err(Failure::tell(3, "file too large"));
    }
    let path = resolve_new(&shared.root, &req.path).map_err(path_failure)?;
    let mut file = match tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .await
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(Failure::tell(EXISTS, "file already exists"));
        }
        Err(_) => return Err(Failure::tell(ACCESS, "cannot create the file")),
    };
    let result = receive_into(req, socket, &mut file).await;
    if result.is_err() {
        drop(file);
        let _ = tokio::fs::remove_file(&path).await;
    }
    result
}

async fn receive_into(
    req: &Request,
    socket: &UdpSocket,
    file: &mut tokio::fs::File,
) -> Result<u64, Failure> {
    let block = req.blksize.unwrap_or(DEFAULT_BLOCK);
    let timeout = req.timeout.map_or(DEFAULT_TIMEOUT, Duration::from_secs);
    let mut options = Vec::new();
    if let Some(b) = req.blksize {
        options.push(("blksize", b as u64));
    }
    if let Some(t) = req.timeout {
        options.push(("timeout", t));
    }
    if let Some(t) = req.tsize {
        options.push(("tsize", t));
    }
    // OACK (or ACK 0 without options) invites the first block.
    let mut reply = if options.is_empty() {
        let mut p = ACK.to_be_bytes().to_vec();
        p.extend_from_slice(&0u16.to_be_bytes());
        p
    } else {
        oack(&options)
    };
    let mut received = 0u64;
    let mut expected: u16 = 1;
    loop {
        let want = expected;
        let data = exchange(socket, &reply, timeout, |p| {
            match (opcode(p), block_of(p)) {
                (Some(DATA), Some(b)) if b == want && p.len() <= block + 4 => {
                    Reply::Done(p[4..].to_vec())
                }
                (Some(ERROR), _) => Reply::Abort("client cancelled"),
                _ => Reply::Ignore,
            }
        })
        .await
        .map_err(|why| Failure::gone(why, received))?;
        received += data.len() as u64;
        if received > MAX_UPLOAD {
            return Err(Failure::tell(3, "file too large"));
        }
        file.write_all(&data)
            .await
            .map_err(|_| Failure::tell(3, "disk full or write error"))?;
        reply = ACK.to_be_bytes().to_vec();
        reply.extend_from_slice(&want.to_be_bytes());
        if data.len() < block {
            file.flush()
                .await
                .map_err(|_| Failure::tell(3, "write error"))?;
            // The last ACK; a lost one makes the client resend the final
            // block, which is answered while the socket lingers briefly.
            let _ = socket.send(&reply).await;
            linger_for_final(socket, &reply, want, timeout).await;
            return Ok(received);
        }
        expected = expected.wrapping_add(1);
    }
}

/// Answers a resent final block for a short while (dally, RFC 1350 §6).
async fn linger_for_final(socket: &UdpSocket, ack: &[u8], last: u16, timeout: Duration) {
    let mut buf = vec![0u8; MAX_BLOCK + 4];
    let deadline = tokio::time::Instant::now() + timeout;
    while let Ok(Ok(n)) = tokio::time::timeout_at(deadline, socket.recv(&mut buf)).await {
        if opcode(&buf[..n]) == Some(DATA) && block_of(&buf[..n]) == Some(last) {
            let _ = socket.send(ack).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_options_parse() {
        let mut p = RRQ.to_be_bytes().to_vec();
        p.extend_from_slice(
            b"fw/image.bin\x00octet\x00blksize\x001428\x00tsize\x000\x00bogus\x00x\x00",
        );
        let r = Request::parse(&p).unwrap();
        assert_eq!(
            r,
            Request {
                write: false,
                path: "fw/image.bin".into(),
                blksize: Some(1428),
                timeout: None,
                tsize: Some(0),
            }
        );
        let mut w = WRQ.to_be_bytes().to_vec();
        w.extend_from_slice(b"cfg.txt\x00NETASCII\x00blksize\x004\x00");
        let r = Request::parse(&w).unwrap();
        assert!(r.write);
        assert_eq!(r.blksize, None, "block size below 8 is ignored");
        let mut bad = RRQ.to_be_bytes().to_vec();
        bad.extend_from_slice(b"x\0mail\0");
        assert_eq!(Request::parse(&bad), None, "mail mode is obsolete");
        assert_eq!(Request::parse(&[0, 3, 0, 1]), None);
    }

    #[test]
    fn errors_and_oacks_encode() {
        assert_eq!(error_packet(1, "no"), [0, 5, 0, 1, b'n', b'o', 0]);
        assert_eq!(oack(&[("tsize", 42)]), b"\0\x06tsize\x0042\0".to_vec());
    }
}
