//! Reads an RDP server's TLS certificate before the external client
//! connects, so the user can verify it like an SSH host key and FreeRDP can
//! be told to accept exactly that certificate (`/cert:fingerprint`).
//!
//! The probe sends the RDP connection request (X.224 with an RDP
//! negotiation request for TLS or CredSSP), then completes a TLS handshake
//! and keeps the server's end-entity certificate. Nothing is authenticated
//! here: the certificate is only fingerprinted, and the real connection is
//! made by the client, which checks the fingerprint itself.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};

use crate::DesktopError;

const PROTOCOL_SSL: u32 = 0x1;
const PROTOCOL_HYBRID: u32 = 0x2;
const PROTOCOL_HYBRID_EX: u32 = 0x8;

/// TPKT header, X.224 Connection Request, RDP_NEG_REQ asking for TLS or
/// CredSSP (MS-RDPBCGR 2.2.1.1).
const CONNECTION_REQUEST: [u8; 19] = [
    0x03, 0x00, 0x00, 0x13, // TPKT, length 19
    0x0e, 0xe0, 0x00, 0x00, 0x00, 0x00, 0x00, // X.224 CR
    0x01, 0x00, 0x08, 0x00, // RDP_NEG_REQ, length 8
    0x03, 0x00, 0x00, 0x00, // PROTOCOL_SSL | PROTOCOL_HYBRID
];

/// SHA-256 of the server certificate (DER).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CertFingerprint(pub [u8; 32]);

impl CertFingerprint {
    pub fn of(der: &[u8]) -> Self {
        Self(Sha256::digest(der).into())
    }

    /// Lowercase hex without separators (storage form).
    pub fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Lowercase hex with colons, as FreeRDP and most tools print it.
    pub fn display(&self) -> String {
        self.0
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":")
    }

    /// Parses [`CertFingerprint::hex`] or [`CertFingerprint::display`].
    pub fn parse(s: &str) -> Option<Self> {
        let hex: String = s.chars().filter(|c| *c != ':').collect();
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(Self(out))
    }
}

fn cert_error(reason: impl Into<String>) -> DesktopError {
    DesktopError::Certificate(reason.into())
}

/// Connects to `host:port` and returns the fingerprint of the certificate
/// the server presents. `server_name` is sent as SNI (the real host when
/// `host` is a local tunnel end).
pub fn read_certificate(
    host: &str,
    port: u16,
    server_name: &str,
    timeout: Duration,
) -> Result<CertFingerprint, DesktopError> {
    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| cert_error(format!("could not resolve {host}: {e}")))?
        .collect();
    let mut last = None;
    let mut tcp = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => {
                tcp = Some(s);
                break;
            }
            Err(e) => last = Some(e),
        }
    }
    let mut tcp = tcp.ok_or_else(|| {
        cert_error(match last {
            Some(e) => format!("could not connect: {e}"),
            None => format!("{host} has no address"),
        })
    })?;
    let io = |e: std::io::Error| cert_error(format!("connection failed: {e}"));
    tcp.set_read_timeout(Some(timeout)).map_err(io)?;
    tcp.set_write_timeout(Some(timeout)).map_err(io)?;

    tcp.write_all(&CONNECTION_REQUEST).map_err(io)?;
    negotiate(&mut tcp)?;
    tls_certificate(&mut tcp, server_name)
}

/// Reads the X.224 Connection Confirm and checks that the server selected
/// a TLS-based security protocol.
fn negotiate(tcp: &mut impl Read) -> Result<(), DesktopError> {
    let mut head = [0u8; 4];
    tcp.read_exact(&mut head)
        .map_err(|e| cert_error(format!("no RDP answer from the server: {e}")))?;
    let len = u16::from_be_bytes([head[2], head[3]]) as usize;
    if head[0] != 0x03 || !(11..=512).contains(&len) {
        return Err(cert_error("the server did not answer like an RDP server"));
    }
    let mut body = vec![0u8; len - 4];
    tcp.read_exact(&mut body)
        .map_err(|e| cert_error(format!("incomplete RDP answer: {e}")))?;
    if body.len() < 7 || body[1] & 0xf0 != 0xd0 {
        return Err(cert_error("the server did not accept the RDP connection"));
    }
    let neg = &body[7..];
    if neg.len() < 8 {
        return Err(legacy());
    }
    let value = u32::from_le_bytes([neg[4], neg[5], neg[6], neg[7]]);
    match neg[0] {
        0x02 if value & (PROTOCOL_SSL | PROTOCOL_HYBRID | PROTOCOL_HYBRID_EX) != 0 => Ok(()),
        0x02 => Err(legacy()),
        0x03 => Err(cert_error(format!(
            "the server refused TLS security (failure code {value})"
        ))),
        _ => Err(cert_error("unexpected RDP negotiation answer")),
    }
}

fn legacy() -> DesktopError {
    cert_error(
        "the server only offers legacy RDP security, whose identity cannot be verified; \
         enable TLS or NLA on the server",
    )
}

fn tls_certificate(
    tcp: &mut TcpStream,
    server_name: &str,
) -> Result<CertFingerprint, DesktopError> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let capture = Arc::new(Capture {
        provider: Arc::clone(&provider),
        cert: Mutex::new(None),
    });
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| cert_error(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(capture.clone())
        .with_no_client_auth();
    // SNI is only a hint for RDP servers; fall back to a fixed name when the
    // host is not a valid DNS name or address.
    let name = ServerName::try_from(server_name.trim().to_owned())
        .unwrap_or_else(|_| ServerName::try_from("rdp.invalid").expect("valid name"));
    let mut conn =
        ClientConnection::new(Arc::new(config), name).map_err(|e| cert_error(e.to_string()))?;
    while conn.is_handshaking() {
        conn.complete_io(tcp)
            .map_err(|e| cert_error(format!("TLS handshake failed: {e}")))?;
    }
    conn.send_close_notify();
    let _ = conn.complete_io(tcp);
    let der = capture
        .cert
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or_else(|| cert_error("the server sent no certificate"))?;
    Ok(CertFingerprint::of(&der))
}

/// Accepts any certificate and keeps it; handshake signatures are still
/// checked so the server must hold the certificate's private key.
#[derive(Debug)]
struct Capture {
    provider: Arc<CryptoProvider>,
    cert: Mutex<Option<Vec<u8>>>,
}

impl ServerCertVerifier for Capture {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        *self.cert.lock().unwrap_or_else(|e| e.into_inner()) = Some(end_entity.to_vec());
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// A minimal RDP endpoint for tests: answers the connection request with
/// `confirm` and, if it selected TLS, completes a TLS handshake with
/// `cert`. Returns the port and the certificate's fingerprint.
#[cfg(test)]
pub(crate) mod fake_server {
    use super::*;
    use rustls::pki_types::PrivateKeyDer;
    use rustls::{ServerConfig, ServerConnection};
    use std::net::TcpListener;

    pub const CONFIRM_TLS: &[u8] = &[
        0x03, 0x00, 0x00, 0x13, 0x0e, 0xd0, 0x00, 0x00, 0x12, 0x34, 0x00, 0x02, 0x00, 0x08, 0x00,
        0x01, 0x00, 0x00, 0x00,
    ];
    pub const CONFIRM_LEGACY: &[u8] = &[
        0x03, 0x00, 0x00, 0x13, 0x0e, 0xd0, 0x00, 0x00, 0x12, 0x34, 0x00, 0x02, 0x00, 0x08, 0x00,
        0x00, 0x00, 0x00, 0x00,
    ];
    pub const CONFIRM_FAILURE: &[u8] = &[
        0x03, 0x00, 0x00, 0x13, 0x0e, 0xd0, 0x00, 0x00, 0x12, 0x34, 0x00, 0x03, 0x00, 0x08, 0x00,
        0x02, 0x00, 0x00, 0x00,
    ];

    /// Serves `connections` connections, then stops. Each TLS connection
    /// reports whether the peer went on after the handshake (sent data)
    /// through the returned channel.
    pub fn start(
        confirm: &'static [u8],
        connections: usize,
    ) -> (u16, CertFingerprint, std::sync::mpsc::Receiver<bool>) {
        let key = rcgen::generate_simple_self_signed(vec!["rdp.test".into()]).unwrap();
        let fp = CertFingerprint::of(key.cert.der());
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = Arc::new(
            ServerConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![key.cert.der().clone()],
                    PrivateKeyDer::try_from(key.signing_key.serialize_der()).unwrap(),
                )
                .unwrap(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for _ in 0..connections {
                let Ok((mut tcp, _)) = listener.accept() else {
                    return;
                };
                tcp.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                let mut head = [0u8; 4];
                if tcp.read_exact(&mut head).is_err() {
                    continue;
                }
                let len = u16::from_be_bytes([head[2], head[3]]) as usize;
                let mut rest = vec![0u8; len.saturating_sub(4)];
                if tcp.read_exact(&mut rest).is_err() || tcp.write_all(confirm).is_err() {
                    continue;
                }
                if confirm != CONFIRM_TLS {
                    continue;
                }
                let mut conn = ServerConnection::new(Arc::clone(&config)).unwrap();
                let mut ok = true;
                while conn.is_handshaking() {
                    if conn.complete_io(&mut tcp).is_err() {
                        ok = false;
                        break;
                    }
                }
                // Did the client continue past the handshake (accepted the
                // certificate) or hang up (rejected it)?
                let mut went_on = false;
                if ok {
                    let mut buf = [0u8; 1];
                    let mut stream = rustls::Stream::new(&mut conn, &mut tcp);
                    went_on = matches!(stream.read(&mut buf), Ok(n) if n > 0);
                }
                let _ = tx.send(went_on);
            }
        });
        (port, fp, rx)
    }
}

#[cfg(test)]
mod tests {
    use super::fake_server::*;
    use super::*;

    const T: Duration = Duration::from_secs(10);

    #[test]
    fn fingerprint_forms_round_trip() {
        let fp = CertFingerprint::of(b"certificate");
        assert_eq!(fp.hex().len(), 64);
        assert_eq!(fp.display().len(), 95);
        assert_eq!(CertFingerprint::parse(&fp.hex()), Some(fp));
        assert_eq!(
            CertFingerprint::parse(&fp.display().to_uppercase()),
            Some(fp)
        );
        assert_eq!(CertFingerprint::parse("abc"), None);
        assert_eq!(CertFingerprint::parse(&"zz".repeat(32)), None);
    }

    #[test]
    fn reads_the_certificate_over_tls() {
        let (port, expected, _rx) = start(CONFIRM_TLS, 1);
        let fp = read_certificate("127.0.0.1", port, "rdp.test", T).unwrap();
        assert_eq!(fp, expected);
    }

    #[test]
    fn legacy_security_and_refusals_are_errors() {
        let (port, _, _) = start(CONFIRM_LEGACY, 1);
        let err = read_certificate("127.0.0.1", port, "x", T).unwrap_err();
        assert!(err.to_string().contains("legacy"), "{err}");

        let (port, _, _) = start(CONFIRM_FAILURE, 1);
        let err = read_certificate("127.0.0.1", port, "x", T).unwrap_err();
        assert!(err.to_string().contains("refused"), "{err}");
    }

    #[test]
    fn non_rdp_servers_are_errors() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let _ = s.write_all(b"SSH-2.0-OpenSSH_9.9\r\n");
        });
        let err = read_certificate("127.0.0.1", port, "x", T).unwrap_err();
        assert!(matches!(err, DesktopError::Certificate(_)), "{err}");
    }
}

/// Runs a real FreeRDP against the fake server with the launch plan
/// Termalith uses. Set `TERMALITH_FREERDP` to the client binary
/// (`xfreerdp3`, `sdl-freerdp3`, ...) to run it; skipped otherwise.
#[cfg(all(test, unix))]
mod freerdp {
    use super::fake_server::*;
    use super::*;
    use crate::{Client, DesktopTarget, Os, Protocol, launch, plan_for};
    use zeroize::Zeroizing;

    /// Whether FreeRDP went on past the TLS handshake (accepted the
    /// certificate) when told to accept only `pin`.
    async fn goes_on(program: &str, pin: impl FnOnce(CertFingerprint) -> CertFingerprint) -> bool {
        let (port, fp, rx) = start(CONFIRM_TLS, 1);
        let client = Client {
            id: "xfreerdp",
            name: "FreeRDP",
            protocol: Protocol::Rdp,
            program: program.into(),
        };
        let target = DesktopTarget {
            protocol: Protocol::Rdp,
            host: "127.0.0.1".into(),
            port,
            username: Some("tester".into()),
            domain: None,
        };
        let plan = plan_for(
            Os::Linux,
            &client,
            &target,
            std::path::Path::new("/tmp"),
            Some(pin(fp)),
        )
        .unwrap();
        let mut l = launch(&plan, Some(Zeroizing::new("pw".into())), "FreeRDP")
            .await
            .unwrap();
        let went_on = tokio::task::spawn_blocking(move || {
            rx.recv_timeout(Duration::from_secs(30)).unwrap_or(false)
        })
        .await
        .unwrap();
        if let Some(c) = l.child.as_mut() {
            let _ = c.start_kill();
        }
        went_on
    }

    #[tokio::test]
    async fn freerdp_accepts_only_the_pinned_certificate() {
        let Ok(program) = std::env::var("TERMALITH_FREERDP") else {
            eprintln!("TERMALITH_FREERDP not set; skipped");
            return;
        };
        assert!(
            goes_on(&program, |real| real).await,
            "FreeRDP refused the pinned certificate"
        );
        assert!(
            !goes_on(&program, |_| CertFingerprint::of(b"another certificate")).await,
            "FreeRDP accepted a certificate that was not pinned"
        );
    }
}
