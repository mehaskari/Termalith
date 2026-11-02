//! Outgoing connections through a SOCKS5 or HTTP CONNECT proxy, optionally
//! with a user name and password (SOCKS5 RFC 1929, HTTP Basic). The proxy
//! only carries the encrypted SSH stream.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use zeroize::Zeroizing;

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
    pub auth: Option<ProxyAuth>,
}

/// Proxy login. The password is never printed (`Debug` redacts it).
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyAuth {
    pub username: String,
    pub password: Zeroizing<String>,
}

impl std::fmt::Debug for ProxyAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyAuth")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// Standard base64 (RFC 4648) for the HTTP Basic header.
fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(A[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
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
    let auth = proxy.auth.as_ref();
    match proxy.kind {
        ProxyKind::Socks5 => socks5(&mut s, host, port, auth).await?,
        ProxyKind::HttpConnect => http_connect(&mut s, host, port, auth).await?,
    }
    Ok(s)
}

async fn socks5(
    s: &mut TcpStream,
    host: &str,
    port: u16,
    auth: Option<&ProxyAuth>,
) -> Result<(), SshError> {
    // Greeting: version 5 and the methods we offer: "no authentication",
    // plus "user name/password" when we have one.
    if auth.is_some() {
        s.write_all(&[5, 2, 0, 2]).await?;
    } else {
        s.write_all(&[5, 1, 0]).await?;
    }
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply).await?;
    match (reply, auth) {
        ([5, 0], _) => {}
        ([5, 2], Some(a)) => socks5_login(s, a).await?,
        ([5, 2], None) | ([5, 0xff], None) => {
            return Err(err(
                "the SOCKS proxy requires a user name and password (Settings → Proxy)",
            ));
        }
        ([5, _], _) => return Err(err("the SOCKS proxy accepts none of our login methods")),
        _ => return Err(err("not a SOCKS5 proxy")),
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

/// RFC 1929 user name/password sub-negotiation.
async fn socks5_login(s: &mut TcpStream, a: &ProxyAuth) -> Result<(), SshError> {
    let (user, pass) = (a.username.as_bytes(), a.password.as_bytes());
    if user.is_empty() || user.len() > 255 || pass.len() > 255 {
        return Err(err("user name and password must be 1-255 and 0-255 bytes"));
    }
    let mut msg = Zeroizing::new(Vec::with_capacity(3 + user.len() + pass.len()));
    msg.push(1);
    msg.push(user.len() as u8);
    msg.extend_from_slice(user);
    msg.push(pass.len() as u8);
    msg.extend_from_slice(pass);
    s.write_all(&msg).await?;
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply).await?;
    if reply[1] != 0 {
        return Err(err("the SOCKS proxy rejected the user name or password"));
    }
    Ok(())
}

async fn http_connect(
    s: &mut TcpStream,
    host: &str,
    port: u16,
    auth: Option<&ProxyAuth>,
) -> Result<(), SshError> {
    let authority = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let mut req = Zeroizing::new(format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n"
    ));
    if let Some(a) = auth {
        if a.username.contains(':') {
            return Err(err("an HTTP proxy user name cannot contain ':'"));
        }
        let pair = Zeroizing::new(format!("{}:{}", a.username, a.password.as_str()));
        req.push_str(&format!(
            "Proxy-Authorization: Basic {}\r\n",
            base64(pair.as_bytes())
        ));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).await?;
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
    if code == "407" {
        return Err(err(if auth.is_some() {
            "the HTTP proxy rejected the user name or password"
        } else {
            "the HTTP proxy requires a user name and password (Settings → Proxy)"
        }));
    }
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
                auth: None,
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
                auth: None,
            };
            let e = connect(&proxy, "localhost", 1)
                .await
                .unwrap_err()
                .to_string();
            assert!(e.contains("proxy"), "{kind:?}: {e}");
        }
    }

    #[test]
    fn base64_matches_rfc_4648_vectors() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
            ("user:pass", "dXNlcjpwYXNz"),
        ] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }

    #[test]
    fn debug_never_shows_the_password() {
        let a = ProxyAuth {
            username: "u".into(),
            password: Zeroizing::new("hunter2".into()),
        };
        assert!(!format!("{a:?}").contains("hunter2"));
    }

    /// One connection: checks the login the client sends and answers it.
    async fn auth_proxy(kind: ProxyKind, expect: &'static [u8], ok: bool) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut c, _) = l.accept().await.unwrap();
            match kind {
                ProxyKind::Socks5 => {
                    let mut g = [0u8; 4];
                    c.read_exact(&mut g).await.unwrap();
                    assert_eq!(g, [5, 2, 0, 2], "offers no-auth and user/password");
                    c.write_all(&[5, 2]).await.unwrap();
                    let mut login = vec![0u8; expect.len()];
                    c.read_exact(&mut login).await.unwrap();
                    assert_eq!(login, expect);
                    c.write_all(&[1, if ok { 0 } else { 1 }]).await.unwrap();
                    if !ok {
                        return;
                    }
                    let mut h = [0u8; 5];
                    c.read_exact(&mut h).await.unwrap();
                    let mut rest = vec![0u8; usize::from(h[4]) + 2];
                    c.read_exact(&mut rest).await.unwrap();
                    c.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0])
                        .await
                        .unwrap();
                }
                ProxyKind::HttpConnect => {
                    let mut buf = vec![0u8; 1024];
                    let n = c.read(&mut buf).await.unwrap();
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    assert!(req.contains(std::str::from_utf8(expect).unwrap()), "{req}");
                    let answer: &[u8] = if ok {
                        b"HTTP/1.1 200 OK\r\n\r\n"
                    } else {
                        b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n"
                    };
                    c.write_all(answer).await.unwrap();
                }
            }
        });
        port
    }

    fn with_login(kind: ProxyKind, port: u16) -> Proxy {
        Proxy {
            kind,
            host: "127.0.0.1".into(),
            port,
            auth: Some(ProxyAuth {
                username: "user".into(),
                password: Zeroizing::new("pass".into()),
            }),
        }
    }

    #[tokio::test]
    async fn socks5_user_password_login() {
        let login: &[u8] = b"\x01\x04user\x04pass";
        let p = auth_proxy(ProxyKind::Socks5, login, true).await;
        connect(&with_login(ProxyKind::Socks5, p), "localhost", 22)
            .await
            .unwrap();
        let p = auth_proxy(ProxyKind::Socks5, login, false).await;
        let e = connect(&with_login(ProxyKind::Socks5, p), "localhost", 22)
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("rejected"), "{e}");
    }

    #[tokio::test]
    async fn http_basic_login() {
        let header: &[u8] = b"Proxy-Authorization: Basic dXNlcjpwYXNz\r\n";
        let p = auth_proxy(ProxyKind::HttpConnect, header, true).await;
        connect(&with_login(ProxyKind::HttpConnect, p), "localhost", 22)
            .await
            .unwrap();
        let p = auth_proxy(ProxyKind::HttpConnect, header, false).await;
        let e = connect(&with_login(ProxyKind::HttpConnect, p), "localhost", 22)
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("rejected"), "{e}");
    }
}
