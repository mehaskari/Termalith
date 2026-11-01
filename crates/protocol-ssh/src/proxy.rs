//! Outgoing connections through a SOCKS5 or HTTP CONNECT proxy (no proxy
//! authentication yet). The proxy only carries the encrypted SSH stream.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::SshError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyKind {
    Socks5,
    HttpConnect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proxy {
    pub kind: ProxyKind,
    pub host: String,
    pub port: u16,
}

fn err(m: impl Into<String>) -> SshError {
    SshError::Connect(format!("proxy: {}", m.into()))
}

/// Opens a TCP stream to `host:port` through `proxy`.
pub async fn connect(proxy: &Proxy, host: &str, port: u16) -> Result<TcpStream, SshError> {
    if host.len() > 255 || host.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(err("invalid target host"));
    }
    let mut s = TcpStream::connect((proxy.host.as_str(), proxy.port))
        .await
        .map_err(|e| err(format!("cannot reach {}:{}: {e}", proxy.host, proxy.port)))?;
    let _ = s.set_nodelay(true);
    match proxy.kind {
        ProxyKind::Socks5 => socks5(&mut s, host, port).await?,
        ProxyKind::HttpConnect => http_connect(&mut s, host, port).await?,
    }
    Ok(s)
}

async fn socks5(s: &mut TcpStream, host: &str, port: u16) -> Result<(), SshError> {
    // Greeting: version 5, one method, "no authentication".
    s.write_all(&[5, 1, 0]).await?;
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply).await?;
    if reply != [5, 0] {
        return Err(err(
            "the SOCKS proxy requires authentication, which is not supported yet",
        ));
    }
    let mut req = vec![5, 1, 0, 3, host.len() as u8];
    req.extend_from_slice(host.as_bytes());
    req.extend_from_slice(&port.to_be_bytes());
    s.write_all(&req).await?;
    let mut head = [0u8; 4];
    s.read_exact(&mut head).await?;
    if head[0] != 5 {
        return Err(err("not a SOCKS5 proxy"));
    }
    if head[1] != 0 {
        let why = match head[1] {
            2 => "connection not allowed by the proxy's rules",
            3 => "network unreachable",
            4 => "host unreachable",
            5 => "connection refused",
            6 => "TTL expired",
            _ => "request failed",
        };
        return Err(err(format!("{host}:{port}: {why}")));
    }
    // Skip the bound address.
    let skip = match head[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut n = [0u8; 1];
            s.read_exact(&mut n).await?;
            usize::from(n[0])
        }
        _ => return Err(err("bad SOCKS reply")),
    };
    let mut rest = vec![0u8; skip + 2];
    s.read_exact(&mut rest).await?;
    Ok(())
}

async fn http_connect(s: &mut TcpStream, host: &str, port: u16) -> Result<(), SshError> {
    let authority = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    s.write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
        .await?;
    // Read the response head byte by byte so no SSH bytes are consumed.
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > 8192 {
            return Err(err("response header too long"));
        }
        if s.read(&mut b).await? == 0 {
            return Err(err("closed the connection"));
        }
        head.push(b[0]);
    }
    let text = String::from_utf8_lossy(&head);
    let status = text.lines().next().unwrap_or("");
    let code = status.split_whitespace().nth(1).unwrap_or("");
    if code != "200" {
        return Err(err(format!("refused CONNECT ({status})")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// A one-shot proxy that checks the request and then relays to `target`.
    async fn fake_proxy(kind: ProxyKind, target: u16, accept: bool) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut c, _) = l.accept().await.unwrap();
            match kind {
                ProxyKind::Socks5 => {
                    let mut g = [0u8; 3];
                    c.read_exact(&mut g).await.unwrap();
                    c.write_all(&[5, 0]).await.unwrap();
                    let mut h = [0u8; 5];
                    c.read_exact(&mut h).await.unwrap();
                    let mut name = vec![0u8; usize::from(h[4]) + 2];
                    c.read_exact(&mut name).await.unwrap();
                    assert_eq!(&name[..name.len() - 2], b"localhost");
                    if !accept {
                        c.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
                        return;
                    }
                    c.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                        .await
                        .unwrap();
                }
                ProxyKind::HttpConnect => {
                    let mut buf = vec![0u8; 512];
                    let n = c.read(&mut buf).await.unwrap();
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    assert!(
                        req.starts_with(&format!("CONNECT localhost:{target} HTTP/1.1\r\n")),
                        "{req}"
                    );
                    if !accept {
                        c.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n")
                            .await
                            .unwrap();
                        return;
                    }
                    c.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                        .await
                        .unwrap();
                }
            }
            let mut up = TcpStream::connect(("127.0.0.1", target)).await.unwrap();
            let _ = tokio::io::copy_bidirectional(&mut c, &mut up).await;
        });
        port
    }

    #[tokio::test]
    async fn socks5_and_http_connect_relay_bytes() {
        for kind in [ProxyKind::Socks5, ProxyKind::HttpConnect] {
            let echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let target = echo.local_addr().unwrap().port();
            tokio::spawn(async move {
                let (mut c, _) = echo.accept().await.unwrap();
                c.write_all(b"SSH-2.0-test\r\n").await.unwrap();
            });
            let p = fake_proxy(kind, target, true).await;
            let proxy = Proxy {
                kind,
                host: "127.0.0.1".into(),
                port: p,
            };
            let mut s = connect(&proxy, "localhost", target).await.unwrap();
            let mut banner = [0u8; 14];
            s.read_exact(&mut banner).await.unwrap();
            assert_eq!(&banner, b"SSH-2.0-test\r\n", "{kind:?}");
        }
    }

    #[tokio::test]
    async fn refusals_are_reported() {
        for kind in [ProxyKind::Socks5, ProxyKind::HttpConnect] {
            let p = fake_proxy(kind, 1, false).await;
            let proxy = Proxy {
                kind,
                host: "127.0.0.1".into(),
                port: p,
            };
            let e = connect(&proxy, "localhost", 1)
                .await
                .unwrap_err()
                .to_string();
            assert!(e.contains("proxy"), "{kind:?}: {e}");
        }
    }
}
