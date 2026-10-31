//! Port forwarding over an SSH connection: local forwards (`ssh -L`) and a
//! dynamic SOCKS5 proxy (`ssh -D`).
//!
//! The bind address is the caller's choice; restricting it to loopback by
//! default is application policy (see app-core).

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use crate::{SshError, SshSession};

/// Simultaneous connections accepted per forward; further ones are closed.
pub const MAX_FORWARD_CONNECTIONS: usize = 256;
const SOCKS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardKind {
    /// Every connection to the local port is tunnelled to `host:port` as
    /// seen from the server.
    Local { host: String, port: u16 },
    /// SOCKS5 (CONNECT, no authentication): the client chooses the target.
    Socks,
}

#[derive(Debug, Default)]
pub struct ForwardStats {
    pub active: AtomicUsize,
    pub total: AtomicU64,
}

/// A running forward. Dropping it stops the listener and closes its
/// connections.
pub struct ForwardHandle {
    local_addr: SocketAddr,
    cancel: CancellationToken,
    stats: Arc<ForwardStats>,
}

impl ForwardHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn active_connections(&self) -> usize {
        self.stats.active.load(Ordering::Relaxed)
    }

    pub fn total_connections(&self) -> u64 {
        self.stats.total.load(Ordering::Relaxed)
    }

    pub fn stop(&self) {
        self.cancel.cancel();
    }
}

impl Drop for ForwardHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Binds `bind` and starts forwarding. Port 0 picks a free port.
pub async fn start_forward(
    session: Arc<SshSession>,
    bind: SocketAddr,
    kind: ForwardKind,
) -> Result<ForwardHandle, SshError> {
    let listener = TcpListener::bind(bind).await?;
    let local_addr = listener.local_addr()?;
    let cancel = CancellationToken::new();
    let stats = Arc::new(ForwardStats::default());
    let (c, s) = (cancel.clone(), Arc::clone(&stats));
    tokio::spawn(async move {
        loop {
            let (tcp, peer) = tokio::select! {
                () = c.cancelled() => break,
                r = listener.accept() => match r {
                    Ok(x) => x,
                    Err(_) => continue,
                },
            };
            if s.active.load(Ordering::Relaxed) >= MAX_FORWARD_CONNECTIONS {
                drop(tcp);
                continue;
            }
            s.active.fetch_add(1, Ordering::Relaxed);
            s.total.fetch_add(1, Ordering::Relaxed);
            let (session, kind, conn_cancel, stats) = (
                Arc::clone(&session),
                kind.clone(),
                c.child_token(),
                Arc::clone(&s),
            );
            tokio::spawn(async move {
                tokio::select! {
                    () = conn_cancel.cancelled() => {}
                    _ = serve(session, tcp, peer, kind) => {}
                }
                stats.active.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
    Ok(ForwardHandle {
        local_addr,
        cancel,
        stats,
    })
}

async fn serve(
    session: Arc<SshSession>,
    mut tcp: TcpStream,
    peer: SocketAddr,
    kind: ForwardKind,
) -> Result<(), SshError> {
    let _ = tcp.set_nodelay(true);
    match kind {
        ForwardKind::Local { host, port } => {
            let mut channel = session.open_direct_tcpip(&host, port, peer).await?;
            let _ = tokio::io::copy_bidirectional(&mut tcp, &mut channel).await;
        }
        ForwardKind::Socks => {
            let (host, port) =
                match tokio::time::timeout(SOCKS_HANDSHAKE_TIMEOUT, socks5_request(&mut tcp)).await
                {
                    Ok(Ok(target)) => target,
                    _ => return Ok(()), // malformed, unsupported or too slow
                };
            // The SOCKS reply depends on whether the server opened the tunnel.
            match session.open_direct_tcpip(&host, port, peer).await {
                Ok(mut channel) => {
                    reply(&mut tcp, 0x00).await?;
                    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut channel).await;
                }
                Err(e) => {
                    let _ = reply(&mut tcp, 0x05).await; // connection refused
                    return Err(e);
                }
            }
        }
    }
    Ok(())
}

/// Reads a SOCKS5 greeting and CONNECT request; returns the target.
async fn socks5_request(tcp: &mut TcpStream) -> Result<(String, u16), SshError> {
    let bad = |m: &str| SshError::Connect(format!("SOCKS: {m}"));
    let mut head = [0u8; 2];
    tcp.read_exact(&mut head).await?;
    if head[0] != 5 {
        return Err(bad("not SOCKS5"));
    }
    let mut methods = vec![0u8; usize::from(head[1])];
    tcp.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        tcp.write_all(&[5, 0xFF]).await?;
        return Err(bad("client requires authentication"));
    }
    tcp.write_all(&[5, 0]).await?;

    let mut req = [0u8; 4];
    tcp.read_exact(&mut req).await?;
    if req[0] != 5 {
        return Err(bad("bad request version"));
    }
    if req[1] != 1 {
        reply(tcp, 0x07).await?; // command not supported
        return Err(bad("only CONNECT is supported"));
    }
    let host = match req[3] {
        1 => {
            let mut a = [0u8; 4];
            tcp.read_exact(&mut a).await?;
            Ipv4Addr::from(a).to_string()
        }
        3 => {
            let mut len = [0u8; 1];
            tcp.read_exact(&mut len).await?;
            let mut name = vec![0u8; usize::from(len[0])];
            tcp.read_exact(&mut name).await?;
            match String::from_utf8(name) {
                Ok(n) if !n.is_empty() => n,
                _ => {
                    reply(tcp, 0x01).await?;
                    return Err(bad("invalid domain name"));
                }
            }
        }
        4 => {
            let mut a = [0u8; 16];
            tcp.read_exact(&mut a).await?;
            Ipv6Addr::from(a).to_string()
        }
        _ => {
            reply(tcp, 0x08).await?; // address type not supported
            return Err(bad("unsupported address type"));
        }
    };
    let mut port = [0u8; 2];
    tcp.read_exact(&mut port).await?;
    Ok((host, u16::from_be_bytes(port)))
}

async fn reply(tcp: &mut TcpStream, code: u8) -> std::io::Result<()> {
    // Bound address is not meaningful for a tunnel; report 0.0.0.0:0.
    tcp.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await
}
