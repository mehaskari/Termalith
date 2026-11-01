//! X11 forwarding (`ssh -X`), the way OpenSSH does it: the server is given
//! a random fake cookie; when it opens an X11 channel, the connection setup
//! must carry that fake cookie, which is then replaced by the real local
//! cookie (or none) before the bytes reach the local X server. The real
//! cookie never leaves this machine.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MIT_COOKIE: &[u8] = b"MIT-MAGIC-COOKIE-1";
/// Longest auth name/data accepted in a connection setup.
const MAX_AUTH_FIELD: usize = 1024;

/// Where the local X server listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum X11Display {
    Unix(PathBuf),
    Tcp { host: String, port: u16 },
}

/// Local X server to forward to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X11Forward {
    pub display: X11Display,
    pub screen: u32,
    /// The local server's MIT-MAGIC-COOKIE-1, if it uses one.
    pub cookie: Option<Vec<u8>>,
}

/// Parses a `DISPLAY` value: `:0`, `:0.1`, `unix:0`, `host:0`, or a socket
/// path such as XQuartz's `/private/tmp/com.apple.launchd.…/org.xquartz:0`.
pub fn parse_display(display: &str) -> Option<(X11Display, u32)> {
    let d = display.trim();
    if d.starts_with('/') {
        // launchd-style: the whole value is the socket path.
        return Some((X11Display::Unix(PathBuf::from(d)), 0));
    }
    let (host, rest) = d.rsplit_once(':')?;
    let (num, screen) = match rest.split_once('.') {
        Some((n, s)) => (n, s.parse().ok()?),
        None => (rest, 0),
    };
    let num: u16 = num.parse().ok()?;
    if host.is_empty() || host == "unix" {
        return Some((
            X11Display::Unix(PathBuf::from(format!("/tmp/.X11-unix/X{num}"))),
            screen,
        ));
    }
    Some((
        X11Display::Tcp {
            host: host.to_owned(),
            port: 6000u16.checked_add(num)?,
        },
        screen,
    ))
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

pub(crate) fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The MIT-MAGIC-COOKIE-1 for `display` from `xauth list` output.
pub fn cookie_from_xauth_list(output: &str) -> Option<Vec<u8>> {
    output.lines().find_map(|l| {
        let mut f = l.split_whitespace();
        let (_, proto, hex) = (f.next()?, f.next()?, f.next()?);
        (proto == "MIT-MAGIC-COOKIE-1")
            .then(|| from_hex(hex))
            .flatten()
    })
}

/// Asks `xauth` for the local cookie (fixed arguments, no shell). `None`
/// when xauth is missing or has no entry (the server may not need one).
pub fn local_cookie(display: &str) -> Option<Vec<u8>> {
    const XAUTH: &[&str] = &["/opt/X11/bin/xauth", "/usr/bin/xauth", "/usr/X11/bin/xauth"];
    let xauth = XAUTH.iter().map(Path::new).find(|p| p.is_file())?;
    let out = std::process::Command::new(xauth)
        .arg("list")
        .arg(display)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    cookie_from_xauth_list(&String::from_utf8_lossy(&out.stdout))
}

pub(crate) fn fake_cookie() -> [u8; 16] {
    use rand::RngExt;
    rand::rng().random()
}

fn pad(n: usize) -> usize {
    (4 - n % 4) % 4
}

/// Reads the client's connection setup from `remote`, checks the fake
/// cookie and returns the setup rewritten with `real` (or no auth).
/// `None` if the setup is malformed or carries the wrong cookie.
pub(crate) async fn rewrite_setup<R: AsyncRead + Unpin>(
    remote: &mut R,
    fake: &[u8],
    real: Option<&[u8]>,
) -> Option<Vec<u8>> {
    let mut head = [0u8; 12];
    remote.read_exact(&mut head).await.ok()?;
    let big = match head[0] {
        b'B' => true,
        b'l' => false,
        _ => return None,
    };
    let get = |i: usize| -> usize {
        let b = [head[i], head[i + 1]];
        usize::from(if big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    };
    let (name_len, data_len) = (get(6), get(8));
    if name_len > MAX_AUTH_FIELD || data_len > MAX_AUTH_FIELD {
        return None;
    }
    let mut name = vec![0u8; name_len + pad(name_len)];
    let mut data = vec![0u8; data_len + pad(data_len)];
    remote.read_exact(&mut name).await.ok()?;
    remote.read_exact(&mut data).await.ok()?;
    if &name[..name_len] != MIT_COOKIE || &data[..data_len] != fake {
        return None;
    }
    let (new_name, new_data): (&[u8], &[u8]) = match real {
        Some(c) => (MIT_COOKIE, c),
        None => (&[], &[]),
    };
    let put = |v: usize| -> [u8; 2] {
        let v = u16::try_from(v).unwrap_or(0);
        if big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let mut out = head[..6].to_vec();
    out.extend_from_slice(&put(new_name.len()));
    out.extend_from_slice(&put(new_data.len()));
    out.extend_from_slice(&head[10..12]);
    out.extend_from_slice(new_name);
    out.resize(out.len() + pad(new_name.len()), 0);
    out.extend_from_slice(new_data);
    out.resize(out.len() + pad(new_data.len()), 0);
    Some(out)
}

/// Connects a forwarded X11 channel to the local display.
pub(crate) async fn proxy<S: AsyncRead + AsyncWrite + Unpin>(
    mut remote: S,
    forward: &X11Forward,
    fake: &[u8],
) {
    let Some(setup) = rewrite_setup(&mut remote, fake, forward.cookie.as_deref()).await else {
        let _ = remote.shutdown().await;
        return;
    };
    match &forward.display {
        X11Display::Unix(path) => {
            #[cfg(unix)]
            if let Ok(mut local) = tokio::net::UnixStream::connect(path).await
                && local.write_all(&setup).await.is_ok()
            {
                let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
            }
            #[cfg(not(unix))]
            let _ = path;
        }
        X11Display::Tcp { host, port } => {
            if let Ok(mut local) = tokio::net::TcpStream::connect((host.as_str(), *port)).await
                && local.write_all(&setup).await.is_ok()
            {
                let _ = local.set_nodelay(true);
                let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
            }
        }
    }
    let _ = remote.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays() {
        assert_eq!(
            parse_display(":0"),
            Some((X11Display::Unix("/tmp/.X11-unix/X0".into()), 0))
        );
        assert_eq!(
            parse_display("unix:1.2"),
            Some((X11Display::Unix("/tmp/.X11-unix/X1".into()), 2))
        );
        assert_eq!(
            parse_display("localhost:10.0"),
            Some((
                X11Display::Tcp {
                    host: "localhost".into(),
                    port: 6010
                },
                0
            ))
        );
        assert_eq!(
            parse_display("/private/tmp/com.apple.launchd.x/org.xquartz:0"),
            Some((
                X11Display::Unix("/private/tmp/com.apple.launchd.x/org.xquartz:0".into()),
                0
            ))
        );
        assert_eq!(parse_display("nonsense"), None);
        assert_eq!(parse_display("h:99999"), None);
    }

    #[test]
    fn xauth_output() {
        let out = "mac/unix:0  MIT-MAGIC-COOKIE-1  00ff10ab\n";
        assert_eq!(cookie_from_xauth_list(out), Some(vec![0, 0xff, 0x10, 0xab]));
        assert_eq!(cookie_from_xauth_list("x XDM-AUTHORIZATION-1 00"), None);
    }

    fn setup(order: u8, name: &[u8], data: &[u8]) -> Vec<u8> {
        let le = order == b'l';
        let u = |v: usize| {
            let v = v as u16;
            if le { v.to_le_bytes() } else { v.to_be_bytes() }
        };
        let mut s = vec![order, 0];
        s.extend_from_slice(&u(11));
        s.extend_from_slice(&u(0));
        s.extend_from_slice(&u(name.len()));
        s.extend_from_slice(&u(data.len()));
        s.extend_from_slice(&[0, 0]);
        s.extend_from_slice(name);
        s.resize(s.len() + pad(name.len()), 0);
        s.extend_from_slice(data);
        s.resize(s.len() + pad(data.len()), 0);
        s
    }

    #[tokio::test]
    async fn swaps_the_fake_cookie_for_the_real_one() {
        let fake = [7u8; 16];
        for order in *b"lB" {
            let mut input = &setup(order, MIT_COOKIE, &fake)[..];
            let out = rewrite_setup(&mut input, &fake, Some(b"real-cookie-1234"))
                .await
                .unwrap();
            assert_eq!(out, setup(order, MIT_COOKIE, b"real-cookie-1234"));
            let mut input = &setup(order, MIT_COOKIE, &fake)[..];
            assert_eq!(
                rewrite_setup(&mut input, &fake, None).await.unwrap(),
                setup(order, b"", b"")
            );
        }
        // Wrong cookie or protocol: refused.
        let mut wrong = &setup(b'l', MIT_COOKIE, &[1u8; 16])[..];
        assert!(rewrite_setup(&mut wrong, &fake, None).await.is_none());
        let mut other = &setup(b'l', b"XDM-AUTHORIZATION-1", &fake)[..];
        assert!(rewrite_setup(&mut other, &fake, None).await.is_none());
    }
}
