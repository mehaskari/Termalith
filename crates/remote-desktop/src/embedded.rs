//! Experimental embedded RDP (ADR-030, `embedded` feature): connects with
//! IronRDP, checks the server certificate against the pinned fingerprint
//! (the same trust as ADR-027) and decodes the desktop into a framebuffer.
//! This is the spike's measuring tool; the viewer UI comes later.

use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ironrdp_blocking::Framed;
use ironrdp_connector::sspi::NetworkRequest;
use ironrdp_connector::sspi::network_client::NetworkClient;
use ironrdp_connector::{ClientConnector, ConnectionOutcome, Credentials, DesktopSize, ServerName};
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_pdu::gcc::{ConnectionType, KeyboardType};
use ironrdp_pdu::rdp::capability_sets::{MajorPlatformType, RailSupportLevel};
use ironrdp_pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{ActiveStageBuilder, ActiveStageOutput};
use zeroize::Zeroizing;

use crate::rdp_cert::Capture;
use crate::{CertFingerprint, DesktopError};

/// What one capture saw: for measuring, not for display.
#[derive(Debug, Clone)]
pub struct CaptureStats {
    pub width: u16,
    pub height: u16,
    pub graphics_updates: usize,
    /// From TCP connect to the first graphics update.
    pub first_update: Option<Duration>,
    /// Pixels that are not black at the end (the desktop was drawn).
    pub drawn_pixels: usize,
}

fn failed(what: &str, e: impl std::fmt::Display) -> DesktopError {
    DesktopError::Launch {
        client: "embedded RDP".into(),
        reason: format!("{what}: {e}"),
    }
}

/// Kerberos is not used; NTLM inside CredSSP needs no network client.
struct NoNetwork;

impl NetworkClient for NoNetwork {
    fn send(&self, _request: &NetworkRequest) -> ironrdp_connector::sspi::Result<Vec<u8>> {
        Err(ironrdp_connector::sspi::Error::new(
            ironrdp_connector::sspi::ErrorKind::NoAuthenticatingAuthority,
            "Kerberos is not supported",
        ))
    }
}

fn config(username: &str, password: &str, size: DesktopSize) -> ironrdp_connector::Config {
    ironrdp_connector::Config {
        desktop_size: size,
        monitor_layout: None,
        desktop_scale_factor: 0,
        desktop_physical_size: None,
        desktop_orientation: None,
        device_scale_factor: None,
        monitor_extended_layout: None,
        relative_mouse_input: false,
        enable_tls: true,
        enable_credssp: true,
        enable_standard_rdp_security: false,
        credentials: Credentials::UsernamePassword {
            username: username.to_owned(),
            password: password.to_owned(),
        },
        domain: None,
        client_build: 0,
        client_name: "termalith".to_owned(),
        keyboard_type: KeyboardType::IBM_ENHANCED,
        keyboard_subtype: 0,
        keyboard_functional_keys_count: 12,
        keyboard_layout: 0,
        connection_type: ConnectionType::Lan,
        ime_file_name: String::new(),
        bitmap: None,
        dig_product_id: String::new(),
        client_dir: String::new(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        remote_application_mode: false,
        rail_support_level: RailSupportLevel::empty(),
        platform: if cfg!(windows) {
            MajorPlatformType::WINDOWS
        } else if cfg!(target_os = "macos") {
            MajorPlatformType::MACINTOSH
        } else {
            MajorPlatformType::UNIX
        },
        hardware_id: None,
        request_data: None,
        autologon: false,
        enable_audio_playback: false,
        enable_audio_capture: false,
        performance_flags: PerformanceFlags::default(),
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
        compression_type: None,
        enable_server_pointer: false,
        pointer_software_rendering: true,
        multitransport_flags: None,
        support_dyn_vc_gfx_protocol: false,
    }
}

/// Connects, logs in and reads the desktop for `duration`. The server's
/// certificate must have fingerprint `pin`.
pub fn capture(
    host: &str,
    port: u16,
    username: &str,
    password: Zeroizing<String>,
    pin: CertFingerprint,
    duration: Duration,
) -> Result<CaptureStats, DesktopError> {
    let size = DesktopSize {
        width: 1024,
        height: 768,
    };
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| failed("resolve", e))?
        .next()
        .ok_or_else(|| failed("resolve", "no address"))?;
    let start = Instant::now();
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
        .map_err(|e| failed("connect", e))?;
    tcp.set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| failed("socket", e))?;
    let client_addr = tcp.local_addr().map_err(|e| failed("socket", e))?;

    let mut framed = Framed::new(tcp);
    let mut connector = ClientConnector::new(config(username, &password, size), client_addr);
    let should_upgrade = ironrdp_blocking::connect_begin(&mut framed, &mut connector)
        .map_err(|e| failed("negotiation", e))?;

    // TLS with the pinned certificate (the probe's verifier keeps it).
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let capture = Arc::new(Capture {
        provider: Arc::clone(&provider),
        cert: Mutex::new(None),
    });
    let mut tls_config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| failed("TLS", e))?
        .dangerous()
        .with_custom_certificate_verifier(capture.clone())
        .with_no_client_auth();
    // CredSSP does not support TLS session resumption.
    tls_config.resumption = rustls::client::Resumption::disabled();
    let name = rustls::pki_types::ServerName::try_from(host.to_owned())
        .unwrap_or_else(|_| rustls::pki_types::ServerName::try_from("rdp.invalid").expect("valid"));
    let conn =
        rustls::ClientConnection::new(Arc::new(tls_config), name).map_err(|e| failed("TLS", e))?;
    let mut tls = rustls::StreamOwned::new(conn, framed.into_inner_no_leftover());
    tls.flush().map_err(|e| failed("TLS handshake", e))?;
    while tls.conn.is_handshaking() {
        tls.conn
            .complete_io(&mut tls.sock)
            .map_err(|e| failed("TLS handshake", e))?;
    }
    let der = capture
        .cert
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .ok_or_else(|| failed("TLS", "no certificate"))?;
    if CertFingerprint::of(&der) != pin {
        return Err(DesktopError::Certificate(
            "the server certificate does not match the trusted one".into(),
        ));
    }
    let public_key = crate::rdp_cert::subject_public_key(&der).unwrap_or_default();

    let upgraded = ironrdp_blocking::mark_as_upgraded(should_upgrade, &mut connector);
    let mut framed = Framed::new(tls);
    let outcome = ironrdp_blocking::connect_finalize(
        upgraded,
        connector,
        &mut framed,
        &mut NoNetwork,
        ServerName::new(host),
        public_key,
        None,
    )
    .map_err(|e| failed("login", e))?;
    let result = match outcome {
        ConnectionOutcome::Connected(r) => r,
        ConnectionOutcome::Redirected(_) => {
            return Err(failed("login", "server redirection is not supported"));
        }
    };

    let mut image = DecodedImage::new(
        PixelFormat::RgbA32,
        result.desktop_size.width,
        result.desktop_size.height,
    );
    let mut stage = ActiveStageBuilder {
        static_channels: result.static_channels,
        user_channel_id: result.user_channel_id,
        io_channel_id: result.io_channel_id,
        message_channel_id: result.message_channel_id,
        share_id: result.share_id,
        compression_type: result.compression_type,
        enable_server_pointer: false,
        pointer_software_rendering: true,
    }
    .build();

    let mut stats = CaptureStats {
        width: image.width(),
        height: image.height(),
        graphics_updates: 0,
        first_update: None,
        drawn_pixels: 0,
    };
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        let (action, payload) = match framed.read_pdu() {
            Ok(x) => x,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(e) => return Err(failed("read", e)),
        };
        let outputs = stage
            .process(&mut image, action, &payload, None)
            .map_err(|e| failed("decode", e))?;
        for out in outputs {
            match out {
                ActiveStageOutput::ResponseFrame(frame) => {
                    framed.write_all(&frame).map_err(|e| failed("write", e))?
                }
                ActiveStageOutput::GraphicsUpdate(_) => {
                    stats.graphics_updates += 1;
                    stats.first_update.get_or_insert_with(|| start.elapsed());
                }
                ActiveStageOutput::Terminate(_) => return Ok(finish(stats, &image)),
                _ => {}
            }
        }
    }
    Ok(finish(stats, &image))
}

fn finish(mut stats: CaptureStats, image: &DecodedImage) -> CaptureStats {
    stats.drawn_pixels = image
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
        .count();
    stats
}
