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
use ironrdp_egfx::client::{GraphicsPipelineClient, GraphicsPipelineHandler};
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

/// An error with its sources, so wrapped causes (e.g. inside CredSSP) are
/// not lost.
fn error_chain(e: &(dyn std::error::Error + 'static)) -> String {
    let mut out = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        let text = s.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        source = s.source();
    }
    out
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
        // The graphics pipeline (RDPEGFX, registered in `connect`): Windows
        // only offers Display Control (resizing) to clients that use it.
        support_dyn_vc_gfx_protocol: true,
    }
}

type Tls = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

/// Where and how to log in. `host` is what the client connects to (a local
/// tunnel end when tunnelled), `server_name` the real server.
pub struct LoginParams<'a> {
    pub host: &'a str,
    pub port: u16,
    pub server_name: &'a str,
    pub username: &'a str,
    pub password: Zeroizing<String>,
    pub domain: Option<&'a str>,
    /// The certificate the user trusts (ADR-027).
    pub pin: CertFingerprint,
    pub width: u16,
    pub height: u16,
}

/// TCP, X.224 negotiation, TLS with the pinned certificate, login.
fn connect(
    p: &LoginParams<'_>,
) -> Result<
    (
        ironrdp_connector::ConnectionResult,
        Framed<Tls>,
        clipboard::Queue,
    ),
    DesktopError,
> {
    let (host, port, username, password, pin) = (p.host, p.port, p.username, &p.password, p.pin);
    let size = DesktopSize {
        width: p.width.clamp(200, 8192),
        height: p.height.clamp(200, 8192),
    };
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| failed("resolve", e))?
        .next()
        .ok_or_else(|| failed("resolve", "no address"))?;
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(10))
        .map_err(|e| failed("connect", e))?;
    // Generous while connecting (slow servers and links, NLA); the session
    // switches to short polling timeouts once it runs.
    tcp.set_read_timeout(Some(CONNECT_TIMEOUT))
        .map_err(|e| failed("socket", e))?;
    tcp.set_write_timeout(Some(CONNECT_TIMEOUT))
        .map_err(|e| failed("socket", e))?;
    let client_addr = tcp.local_addr().map_err(|e| failed("socket", e))?;

    let mut framed = Framed::new(tcp);
    let mut cfg = config(username, password, size);
    cfg.domain = p.domain.map(str::to_owned);
    // Display Control lets the viewer resize the remote desktop; the
    // graphics pipeline carries the desktop on servers that support it
    // (without an H.264 decoder only its RemoteFX/planar/ClearCodec codecs
    // are advertised).
    let drdynvc = ironrdp_dvc::DrdynvcClient::new()
        .with_dynamic_channel(ironrdp_displaycontrol::client::DisplayControlClient::new(
            |_| Ok(Vec::new()),
        ))
        .with_dynamic_channel(GraphicsPipelineClient::new(Box::new(GfxEvents), None));
    let clip_queue = clipboard::Queue::default();
    let cliprdr = ironrdp_cliprdr::CliprdrClient::new(Box::new(clipboard::TextBackend {
        queue: clip_queue.clone(),
    }));
    let mut connector = ClientConnector::new(cfg, client_addr)
        .with_static_channel(drdynvc)
        .with_static_channel(cliprdr);
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
    let name = rustls::pki_types::ServerName::try_from(p.server_name.to_owned())
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
        ServerName::new(p.server_name),
        public_key,
        None,
    )
    .map_err(|e| failed("login", error_chain(&e)))?;
    let result = match outcome {
        ConnectionOutcome::Connected(r) => r,
        ConnectionOutcome::Redirected(_) => {
            return Err(failed("login", "server redirection is not supported"));
        }
    };

    Ok((result, framed, clip_queue))
}

/// Connects, logs in and reads the desktop for `duration` (measuring).
pub fn capture(p: &LoginParams<'_>, duration: Duration) -> Result<CaptureStats, DesktopError> {
    let start = Instant::now();
    let (result, mut framed, _clip) = connect(p)?;
    framed
        .get_inner_mut()
        .0
        .sock
        .set_read_timeout(Some(Duration::from_millis(500)))
        .map_err(|e| failed("socket", e))?;
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
    let mut canvas: Option<Canvas> = None;
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
                ActiveStageOutput::Terminate(_) => return Ok(finish(stats, &image, &canvas)),
                _ => {}
            }
        }
        let mut dirty = None;
        if let Some((w, h)) = pump_graphics(&mut stage, &mut canvas, &mut dirty) {
            (stats.width, stats.height) = (w, h);
        }
        if dirty.is_some() {
            stats.graphics_updates += 1;
            stats.first_update.get_or_insert_with(|| start.elapsed());
        }
    }
    Ok(finish(stats, &image, &canvas))
}

fn finish(mut stats: CaptureStats, image: &DecodedImage, canvas: &Option<Canvas>) -> CaptureStats {
    let data = canvas.as_ref().map_or(image.data(), |c| &c.data[..]);
    stats.drawn_pixels = data
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
        .count();
    stats
}

/// Text clipboard (CF_UNICODETEXT) shared with the viewer. The backend's
/// callbacks only queue events; the session loop acts on them, because
/// replies go through the session's channel encoder.
mod clipboard {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use ironrdp_cliprdr::backend::CliprdrBackend;
    use ironrdp_cliprdr::pdu::{
        ClipboardFormat, ClipboardFormatId, ClipboardGeneralCapabilityFlags, FileContentsRequest,
        FileContentsResponse, FormatDataRequest, FormatDataResponse, LockDataId,
    };

    #[derive(Debug, PartialEq, Eq)]
    pub enum ClipEvent {
        /// The server wants our format list (start of the session).
        AnnounceFormats,
        /// The remote copied something that includes text.
        RemoteHasText,
        /// The remote's text arrived.
        RemoteText(String),
        /// The remote pastes and asks for our data in this format.
        DataRequested(ClipboardFormatId),
    }

    pub type Queue = Arc<Mutex<VecDeque<ClipEvent>>>;

    #[derive(Debug)]
    pub struct TextBackend {
        pub queue: Queue,
    }

    impl TextBackend {
        fn push(&self, e: ClipEvent) {
            self.queue
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push_back(e);
        }
    }

    impl ironrdp_pdu::AsAny for TextBackend {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    /// UTF-16LE with a terminating NUL, as CF_UNICODETEXT carries it.
    pub fn encode_text(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    /// Up to the first NUL; invalid sequences are replaced.
    pub fn decode_text(data: &[u8]) -> String {
        let units: Vec<u16> = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .take_while(|u| *u != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }

    pub fn text_format() -> ClipboardFormat {
        ClipboardFormat::new(ClipboardFormatId::CF_UNICODETEXT)
    }

    impl CliprdrBackend for TextBackend {
        fn temporary_directory(&self) -> &str {
            ".termalith-cliprdr"
        }
        fn client_capabilities(&self) -> ClipboardGeneralCapabilityFlags {
            ClipboardGeneralCapabilityFlags::empty()
        }
        fn on_ready(&mut self) {}
        fn on_request_format_list(&mut self) {
            self.push(ClipEvent::AnnounceFormats);
        }
        fn on_process_negotiated_capabilities(&mut self, _: ClipboardGeneralCapabilityFlags) {}
        fn on_remote_copy(&mut self, formats: &[ClipboardFormat]) {
            if formats
                .iter()
                .any(|f| f.id() == ClipboardFormatId::CF_UNICODETEXT)
            {
                self.push(ClipEvent::RemoteHasText);
            }
        }
        fn on_format_data_request(&mut self, request: FormatDataRequest) {
            self.push(ClipEvent::DataRequested(request.format));
        }
        fn on_format_data_response(&mut self, response: FormatDataResponse<'_>) {
            if !response.is_error() {
                self.push(ClipEvent::RemoteText(decode_text(response.data())));
            }
        }
        fn on_file_contents_request(&mut self, _: FileContentsRequest) {}
        fn on_file_contents_response(&mut self, _: FileContentsResponse<'_>) {}
        fn on_lock(&mut self, _: LockDataId) {}
        fn on_unlock(&mut self, _: LockDataId) {}
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn unicode_text_round_trips() {
            let s = "héllo 👋 سلام";
            let enc = encode_text(s);
            assert_eq!(&enc[enc.len() - 2..], &[0, 0], "NUL-terminated");
            assert_eq!(decode_text(&enc), s);
            assert_eq!(decode_text(&[0x41, 0, 0, 0, 0x42, 0]), "A", "stops at NUL");
        }
    }
}

/// Events from a live session to the viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerEvent {
    /// Size of the remote desktop; frames refer to it.
    Connected { width: u16, height: u16 },
    /// Changed pixels, RGBA, `w * h * 4` bytes, row by row.
    Frame {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        rgba: Vec<u8>,
    },
    /// The remote computer copied this text.
    Clipboard(String),
    /// The session ended; `reason` is set when it was not a normal close.
    Closed { reason: Option<String> },
}

/// Input from the viewer. Keys are PC/AT scan codes (`0xE0xx` = extended),
/// buttons use the DOM numbering (0 left, 1 middle, 2 right, 3/4 back/forward).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerInput {
    MouseMove {
        x: u16,
        y: u16,
    },
    Button {
        button: u8,
        down: bool,
    },
    Wheel {
        vertical: bool,
        units: i16,
    },
    Key {
        scancode: u16,
        down: bool,
    },
    Unicode {
        ch: char,
        down: bool,
    },
    /// Releases every pressed key and button (the viewer lost focus).
    ReleaseAll,
}

enum Control {
    Input(Vec<ViewerInput>),
    /// Ask the server for a new desktop size (it may adjust or refuse it).
    Resize(u16, u16),
    /// Text copied on this computer, offered to the remote.
    Clipboard(String),
    /// The viewer drew one frame.
    Ack,
    Close,
}

/// Longest wait for one server answer while connecting and logging in.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Frames sent but not yet drawn; more changes are merged meanwhile.
const MAX_FRAMES_IN_FLIGHT: usize = 2;
/// How often the session loop looks at viewer input while the server is quiet.
const POLL: Duration = Duration::from_millis(15);
/// Without Display Control a resize reconnects at the new size, once the
/// requests have stopped for this long (a window being dragged)...
const RESIZE_SETTLE: Duration = Duration::from_millis(1500);
/// ...and not before the server had time to open Display Control...
const DISPLAY_CONTROL_WAIT: Duration = Duration::from_secs(4);
/// ...or when a Display Control request was not followed by the new size
/// within this time (servers that accept the channel but ignore it).
const DISPLAY_CONTROL_ANSWER: Duration = Duration::from_secs(3);

/// What the session thread keeps to log in again at a new size (servers
/// without Display Control). The password stays in this thread's memory
/// for the session's life and is wiped when it ends.
struct Login {
    host: String,
    port: u16,
    server_name: String,
    username: String,
    password: Zeroizing<String>,
    domain: Option<String>,
    pin: CertFingerprint,
    width: u16,
    height: u16,
}

impl Login {
    fn of(p: &LoginParams<'_>) -> Self {
        Self {
            host: p.host.to_owned(),
            port: p.port,
            server_name: p.server_name.to_owned(),
            username: p.username.to_owned(),
            password: p.password.clone(),
            domain: p.domain.map(str::to_owned),
            pin: p.pin,
            width: p.width,
            height: p.height,
        }
    }

    fn params(&self) -> LoginParams<'_> {
        LoginParams {
            host: &self.host,
            port: self.port,
            server_name: &self.server_name,
            username: &self.username,
            password: self.password.clone(),
            domain: self.domain.as_deref(),
            pin: self.pin,
            width: self.width,
            height: self.height,
        }
    }
}

/// Connects and switches the socket to the session's short polling reads.
fn connect_session(
    p: &LoginParams<'_>,
) -> Result<
    (
        ironrdp_connector::ConnectionResult,
        Framed<Tls>,
        clipboard::Queue,
    ),
    DesktopError,
> {
    let (result, mut framed, clip) = connect(p)?;
    framed
        .get_inner_mut()
        .0
        .sock
        .set_read_timeout(Some(POLL))
        .map_err(|e| failed("socket", e))?;
    Ok((result, framed, clip))
}

/// How one connection's session ended.
enum Ended {
    Done,
    /// Log in again at this size.
    Resize(u16, u16),
}

/// Handle to a running session. Dropping it closes the session.
pub struct Viewer {
    tx: std::sync::mpsc::SyncSender<Control>,
}

impl Viewer {
    pub fn input(&self, events: Vec<ViewerInput>) -> bool {
        self.tx.send(Control::Input(events)).is_ok()
    }

    /// Asks the server to change the desktop size; a `Connected` event with
    /// the new size follows when it does.
    pub fn resize(&self, width: u16, height: u16) -> bool {
        self.tx.send(Control::Resize(width, height)).is_ok()
    }

    /// Offers locally copied text to the remote computer (it is sent when
    /// the remote pastes).
    pub fn clipboard(&self, text: String) -> bool {
        self.tx.send(Control::Clipboard(text)).is_ok()
    }

    pub fn ack(&self) -> bool {
        self.tx.send(Control::Ack).is_ok()
    }

    pub fn close(&self) {
        let _ = self.tx.send(Control::Close);
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.close();
    }
}

/// Connects and logs in (blocking; call off the UI thread), then runs the
/// session on its own thread, sending [`ViewerEvent`]s to `sink`. `sink`
/// returning `false` ends the session.
pub fn start(
    p: &LoginParams<'_>,
    sink: impl FnMut(ViewerEvent) -> bool + Send + 'static,
) -> Result<Viewer, DesktopError> {
    let (result, framed, clip) = connect_session(p)?;
    let login = Login::of(p);
    let (tx, rx) = std::sync::mpsc::sync_channel(256);
    std::thread::Builder::new()
        .name("rdp-session".into())
        .spawn(move || {
            let mut sink = sink;
            let reason = run(login, result, framed, &rx, clip, &mut sink).err();
            sink(ViewerEvent::Closed { reason });
        })
        .map_err(|e| failed("thread", e))?;
    Ok(Viewer { tx })
}

/// Smallest rectangle covering both (inclusive coordinates).
fn union(a: Option<(u16, u16, u16, u16)>, b: (u16, u16, u16, u16)) -> (u16, u16, u16, u16) {
    match a {
        None => b,
        Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
    }
}

/// The graphics pipeline's handler: everything it decodes is read from the
/// client's compositor instead (`pump_graphics`).
struct GfxEvents;

impl GraphicsPipelineHandler for GfxEvents {
    fn on_capabilities_confirmed(&mut self, caps: &ironrdp_egfx::pdu::CapabilitySet) {
        debug(&format!("graphics pipeline active: {caps:?}"));
    }

    fn on_reset_graphics(&mut self, width: u32, height: u32) {
        debug(&format!("graphics output reset to {width}x{height}"));
    }
}

/// The desktop as the graphics pipeline draws it (RGBA), used instead of
/// the legacy framebuffer once the server sends graphics that way.
struct Canvas {
    width: u16,
    height: u16,
    data: Vec<u8>,
}

impl Canvas {
    fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            data: vec![0; usize::from(width) * usize::from(height) * 4],
        }
    }

    /// Copies an update in; returns the inclusive rectangle it changed.
    fn blit(
        &mut self,
        r: &ironrdp_pdu::geometry::ExclusiveRectangle,
        data: &[u8],
    ) -> Option<(u16, u16, u16, u16)> {
        let src_w = usize::from(r.right.saturating_sub(r.left));
        let right = r.right.min(self.width);
        let bottom = r.bottom.min(self.height);
        if r.left >= right || r.top >= bottom {
            return None;
        }
        let w = usize::from(right - r.left);
        let stride = usize::from(self.width) * 4;
        for (i, row) in (r.top..bottom).enumerate() {
            let src = data.get(i * src_w * 4..i * src_w * 4 + w * 4)?;
            let dst = usize::from(row) * stride + usize::from(r.left) * 4;
            self.data[dst..dst + w * 4].copy_from_slice(src);
        }
        Some((r.left, r.top, right - 1, bottom - 1))
    }
}

/// Moves what the graphics pipeline drew into `canvas` and widens `dirty`.
/// Returns the new desktop size when the server reset the output.
fn pump_graphics(
    stage: &mut ironrdp_session::ActiveStage,
    canvas: &mut Option<Canvas>,
    dirty: &mut Option<(u16, u16, u16, u16)>,
) -> Option<(u16, u16)> {
    let mut channel = stage.get_dvc_mut::<GraphicsPipelineClient>()?;
    let gfx = channel.processor_mut();
    let reset = gfx.take_output_reset();
    if let Some((w, h)) = reset {
        *canvas = Some(Canvas::new(w, h));
        *dirty = None;
    }
    if let Some(c) = canvas.as_mut() {
        for update in gfx.drain_output() {
            if let Some(r) = c.blit(&update.region, &update.data) {
                *dirty = Some(union(*dirty, r));
            }
        }
    }
    reset
}

/// RGBA pixels of an inclusive rectangle, alpha forced opaque.
fn crop(data: &[u8], width: u16, height: u16, r: (u16, u16, u16, u16)) -> ViewerEvent {
    let (iw, ih) = (usize::from(width), usize::from(height));
    let left = usize::from(r.0).min(iw.saturating_sub(1));
    let top = usize::from(r.1).min(ih.saturating_sub(1));
    let right = usize::from(r.2).min(iw.saturating_sub(1));
    let bottom = usize::from(r.3).min(ih.saturating_sub(1));
    let (w, h) = (right - left + 1, bottom - top + 1);
    let mut rgba = Vec::with_capacity(w * h * 4);
    for row in top..=bottom {
        let start = (row * iw + left) * 4;
        rgba.extend_from_slice(&data[start..start + w * 4]);
    }
    for px in rgba.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    // Sizes come from u16 coordinates, so they fit.
    ViewerEvent::Frame {
        x: left as u16,
        y: top as u16,
        w: w as u16,
        h: h as u16,
        rgba,
    }
}

fn to_operation(input: ViewerInput) -> Option<ironrdp_input::Operation> {
    use ironrdp_input::{MouseButton, MousePosition, Operation, Scancode, WheelRotations};
    Some(match input {
        ViewerInput::MouseMove { x, y } => Operation::MouseMove(MousePosition { x, y }),
        ViewerInput::Button { button, down } => {
            let b = match button {
                0 => MouseButton::Left,
                1 => MouseButton::Middle,
                2 => MouseButton::Right,
                3 => MouseButton::X1,
                4 => MouseButton::X2,
                _ => return None,
            };
            if down {
                Operation::MouseButtonPressed(b)
            } else {
                Operation::MouseButtonReleased(b)
            }
        }
        ViewerInput::Wheel { vertical, units } => Operation::WheelRotations(WheelRotations {
            is_vertical: vertical,
            rotation_units: units,
        }),
        ViewerInput::Key { scancode, down } => {
            let s = Scancode::from_u16(scancode);
            if down {
                Operation::KeyPressed(s)
            } else {
                Operation::KeyReleased(s)
            }
        }
        ViewerInput::Unicode { ch, down } => {
            if down {
                Operation::UnicodeKeyPressed(ch)
            } else {
                Operation::UnicodeKeyReleased(ch)
            }
        }
        ViewerInput::ReleaseAll => return None,
    })
}

fn run(
    mut login: Login,
    mut result: ironrdp_connector::ConnectionResult,
    mut framed: Framed<Tls>,
    rx: &std::sync::mpsc::Receiver<Control>,
    mut clip: clipboard::Queue,
    sink: &mut impl FnMut(ViewerEvent) -> bool,
) -> Result<(), String> {
    loop {
        match session(result, framed, rx, clip, sink)? {
            Ended::Done => return Ok(()),
            Ended::Resize(width, height) => {
                debug(&format!("reconnecting at {width}x{height}"));
                (login.width, login.height) = (width, height);
                (result, framed, clip) =
                    connect_session(&login.params()).map_err(|e| e.to_string())?;
            }
        }
    }
}

/// One connection: graphics, input, clipboard and resizing until it ends.
fn session(
    result: ironrdp_connector::ConnectionResult,
    mut framed: Framed<Tls>,
    rx: &std::sync::mpsc::Receiver<Control>,
    clip: clipboard::Queue,
    sink: &mut impl FnMut(ViewerEvent) -> bool,
) -> Result<Ended, String> {
    let started = Instant::now();
    let mut image = DecodedImage::new(
        PixelFormat::RgbA32,
        result.desktop_size.width,
        result.desktop_size.height,
    );
    let activation = result.activation_factory;
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
    let mut input = ironrdp_input::Database::new();
    if !sink(ViewerEvent::Connected {
        width: image.width(),
        height: image.height(),
    }) {
        return Ok(Ended::Done);
    }
    let mut dirty: Option<(u16, u16, u16, u16)> = None;
    let mut in_flight = 0usize;
    // Last text copied here, served when the remote pastes.
    let mut local_text: Option<String> = None;
    // Requested size and when: sent once Display Control is ready (it opens
    // some time after login), or a reconnect at that size where it never does.
    // The last element is when it went out through Display Control.
    let mut pending_resize: Option<(u32, u32, Instant, Option<Instant>)> = None;
    let mut canvas: Option<Canvas> = None;
    // Display Control channel state, logged when it changes (diagnostics).
    let mut display_control = (false, false);
    /// What handling a batch of outputs asks the loop to do next.
    enum Next {
        Go,
        Stop,
        Reactivate,
    }
    let write = |framed: &mut Framed<Tls>,
                 outputs: Vec<ActiveStageOutput>,
                 dirty: &mut Option<_>|
     -> Result<Next, String> {
        let mut next = Next::Go;
        for out in outputs {
            match out {
                ActiveStageOutput::ResponseFrame(f) => {
                    framed.write_all(&f).map_err(|e| format!("write: {e}"))?
                }
                ActiveStageOutput::GraphicsUpdate(r) => {
                    *dirty = Some(union(*dirty, (r.left, r.top, r.right, r.bottom)));
                }
                ActiveStageOutput::DeactivateAll => {
                    debug("server deactivated the session (reactivation follows)");
                    next = Next::Reactivate
                }
                ActiveStageOutput::Terminate(_) => return Ok(Next::Stop),
                _ => {}
            }
        }
        Ok(next)
    };
    loop {
        // Viewer input, acks and close requests first.
        loop {
            match rx.try_recv() {
                Ok(Control::Input(events)) => {
                    let mut fp = Vec::new();
                    for e in events {
                        if e == ViewerInput::ReleaseAll {
                            fp.extend(input.release_all());
                        } else if let Some(op) = to_operation(e) {
                            fp.extend(input.apply([op]));
                        }
                    }
                    if !fp.is_empty() {
                        let outputs = stage
                            .process_fastpath_input(&mut image, &fp)
                            .map_err(|e| format!("input: {e}"))?;
                        match write(&mut framed, outputs, &mut dirty)? {
                            Next::Go => {}
                            Next::Stop => return Ok(Ended::Done),
                            Next::Reactivate => {
                                reactivate(&activation, &mut framed, &mut stage, &mut image)?;
                                dirty = None;
                                in_flight = 0;
                                if !sink(ViewerEvent::Connected {
                                    width: image.width(),
                                    height: image.height(),
                                }) {
                                    return Ok(Ended::Done);
                                }
                            }
                        }
                    }
                }
                Ok(Control::Resize(w, h)) => {
                    let (w, h) =
                        ironrdp_displaycontrol::pdu::MonitorLayoutEntry::adjust_display_size(
                            u32::from(w),
                            u32::from(h),
                        );
                    // Sent once Display Control is ready (below); a newer
                    // request replaces an unsent one.
                    pending_resize = Some((w, h, Instant::now(), None));
                }
                Ok(Control::Clipboard(text)) => {
                    local_text = Some(text);
                    clip_send(&mut framed, &mut stage, |c| {
                        c.initiate_copy(&[clipboard::text_format()])
                    })?;
                }
                Ok(Control::Ack) => in_flight = in_flight.saturating_sub(1),
                Ok(Control::Close) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if let Ok(outputs) = stage.graceful_shutdown() {
                        let _ = write(&mut framed, outputs, &mut dirty);
                    }
                    return Ok(Ended::Done);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
            }
        }
        match framed.read_pdu() {
            Ok((action, payload)) => {
                let outputs = stage
                    .process(&mut image, action, &payload, None)
                    .map_err(|e| format!("decode: {e}"))?;
                match write(&mut framed, outputs, &mut dirty)? {
                    Next::Go => {}
                    Next::Stop => return Ok(Ended::Done),
                    Next::Reactivate => {
                        reactivate(&activation, &mut framed, &mut stage, &mut image)?;
                        dirty = None;
                        in_flight = 0;
                        if !sink(ViewerEvent::Connected {
                            width: image.width(),
                            height: image.height(),
                        }) {
                            return Ok(Ended::Done);
                        }
                    }
                }
                let state = stage
                    .get_dvc::<ironrdp_displaycontrol::client::DisplayControlClient>()
                    .map_or((false, false), |c| (true, c.processor().ready()));
                if state != display_control {
                    display_control = state;
                    debug(&format!(
                        "display control: channel open {}, capabilities {}",
                        state.0, state.1
                    ));
                }
                if let Some((width, height)) = pump_graphics(&mut stage, &mut canvas, &mut dirty) {
                    in_flight = 0;
                    if !sink(ViewerEvent::Connected { width, height }) {
                        return Ok(Ended::Done);
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(Ended::Done),
            Err(e) => return Err(format!("connection lost: {e}")),
        }
        if let Some((w, h, asked, sent)) = pending_resize {
            let size = canvas
                .as_ref()
                .map_or((image.width(), image.height()), |c| (c.width, c.height));
            let wanted = (
                u16::try_from(w).unwrap_or(u16::MAX),
                u16::try_from(h).unwrap_or(u16::MAX),
            );
            if wanted == size {
                pending_resize = None;
            } else if sent.is_none()
                && let Some(frame) = stage.encode_resize(w, h, None, None)
            {
                pending_resize = Some((w, h, asked, Some(Instant::now())));
                debug(&format!("resize to {w}x{h} sent"));
                let frame = frame.map_err(|e| format!("resize: {e}"))?;
                framed
                    .write_all(&frame)
                    .map_err(|e| format!("write: {e}"))?;
            } else if asked.elapsed() >= RESIZE_SETTLE
                && started.elapsed() >= DISPLAY_CONTROL_WAIT
                && sent.is_none_or(|t| t.elapsed() >= DISPLAY_CONTROL_ANSWER)
            {
                // No (working) Display Control: log in again at the new
                // size (Windows reconnects to the same session).
                if let Ok(outputs) = stage.graceful_shutdown() {
                    let _ = write(&mut framed, outputs, &mut dirty);
                }
                return Ok(Ended::Resize(wanted.0, wanted.1));
            }
        }
        loop {
            let ev = clip.lock().unwrap_or_else(|p| p.into_inner()).pop_front();
            let Some(ev) = ev else { break };
            match ev {
                clipboard::ClipEvent::AnnounceFormats => {
                    let formats: Vec<_> = local_text
                        .iter()
                        .map(|_| clipboard::text_format())
                        .collect();
                    clip_send(&mut framed, &mut stage, |c| c.initiate_copy(&formats))?;
                }
                clipboard::ClipEvent::RemoteHasText => clip_send(&mut framed, &mut stage, |c| {
                    c.initiate_paste(ironrdp_cliprdr::pdu::ClipboardFormatId::CF_UNICODETEXT)
                })?,
                clipboard::ClipEvent::RemoteText(text) => {
                    if !sink(ViewerEvent::Clipboard(text)) {
                        return Ok(Ended::Done);
                    }
                }
                clipboard::ClipEvent::DataRequested(format) => {
                    let response = match (&local_text, format) {
                        (Some(t), ironrdp_cliprdr::pdu::ClipboardFormatId::CF_UNICODETEXT) => {
                            ironrdp_cliprdr::pdu::OwnedFormatDataResponse::new_data(
                                clipboard::encode_text(t),
                            )
                        }
                        _ => ironrdp_cliprdr::pdu::OwnedFormatDataResponse::new_error(),
                    };
                    clip_send(&mut framed, &mut stage, |c| c.submit_format_data(response))?;
                }
            }
        }
        if in_flight < MAX_FRAMES_IN_FLIGHT
            && let Some(r) = dirty.take()
        {
            in_flight += 1;
            let frame = match &canvas {
                Some(c) => crop(&c.data, c.width, c.height, r),
                None => crop(image.data(), image.width(), image.height(), r),
            };
            if !sink(frame) {
                return Ok(Ended::Done);
            }
        }
    }
}

/// Diagnostics for tests and troubleshooting (`TERMALITH_RDP_DEBUG=1`);
/// never includes credentials or screen contents.
fn debug(message: &str) {
    if std::env::var_os("TERMALITH_RDP_DEBUG").is_some() {
        eprintln!("rdp: {message}");
    }
}

/// Runs a clipboard channel action and sends what it produced.
fn clip_send(
    framed: &mut Framed<Tls>,
    stage: &mut ironrdp_session::ActiveStage,
    action: impl FnOnce(
        &mut ironrdp_cliprdr::CliprdrClient,
    ) -> ironrdp_pdu::PduResult<
        ironrdp_cliprdr::CliprdrSvcMessages<ironrdp_cliprdr::Client>,
    >,
) -> Result<(), String> {
    let Some(c) = stage.get_svc_processor_mut::<ironrdp_cliprdr::CliprdrClient>() else {
        return Ok(()); // the server did not open the clipboard channel
    };
    let messages = action(c).map_err(|e| format!("clipboard: {e}"))?;
    let frame = stage
        .process_svc_processor_messages(messages)
        .map_err(|e| format!("clipboard: {e}"))?;
    framed.write_all(&frame).map_err(|e| format!("write: {e}"))
}

/// Deactivation-Reactivation Sequence (after a resize): renegotiates the
/// session and starts a framebuffer of the new size.
fn reactivate(
    factory: &ironrdp_connector::connection_activation::ConnectionActivationFactory,
    framed: &mut Framed<Tls>,
    stage: &mut ironrdp_session::ActiveStage,
    image: &mut DecodedImage,
) -> Result<(), String> {
    use ironrdp_connector::Sequence as _;
    use ironrdp_connector::connection_activation::ConnectionActivationState;
    let mut seq = factory.create();
    let mut buf = ironrdp_pdu::WriteBuf::new();
    loop {
        buf.clear();
        let written = match seq.next_pdu_hint() {
            Some(hint) => {
                let pdu = loop {
                    match framed.read_by_hint(hint) {
                        Ok(p) => break p,
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Err(e) => return Err(format!("reactivation: {e}")),
                    }
                };
                seq.step(&pdu, None, &mut buf)
            }
            None => seq.step_no_input(&mut buf),
        }
        .map_err(|e| format!("reactivation: {e}"))?;
        if written.size().is_some() {
            framed
                .write_all(buf.filled())
                .map_err(|e| format!("write: {e}"))?;
        }
        if let ConnectionActivationState::Finalized {
            desktop_size,
            share_id,
            enable_server_pointer,
            pointer_software_rendering,
            ..
        } = seq.connection_activation_state()
        {
            *image =
                DecodedImage::new(PixelFormat::RgbA32, desktop_size.width, desktop_size.height);
            stage.set_fastpath_processor(
                ironrdp_session::fast_path::ProcessorBuilder {
                    io_channel_id: seq.io_channel_id(),
                    user_channel_id: seq.user_channel_id(),
                    share_id,
                    enable_server_pointer,
                    pointer_software_rendering,
                }
                .build(),
            );
            stage.set_share_id(share_id);
            stage.set_enable_server_pointer(enable_server_pointer);
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_rectangles_merge() {
        assert_eq!(union(None, (1, 2, 3, 4)), (1, 2, 3, 4));
        assert_eq!(
            union(Some((10, 10, 20, 20)), (5, 15, 12, 30)),
            (5, 10, 20, 30)
        );
    }

    #[test]
    fn crop_copies_rows_and_makes_pixels_opaque() {
        let image = DecodedImage::new(PixelFormat::RgbA32, 4, 3);
        let crop = |r| crop(image.data(), image.width(), image.height(), r);
        let ViewerEvent::Frame { x, y, w, h, rgba } = crop((1, 1, 2, 2)) else {
            unreachable!()
        };
        assert_eq!((x, y, w, h), (1, 1, 2, 2));
        assert_eq!(rgba.len(), 16);
        assert!(rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        // Out-of-range coordinates are clamped to the image.
        let ViewerEvent::Frame { w, h, .. } = crop((3, 2, 100, 100)) else {
            unreachable!()
        };
        assert_eq!((w, h), (1, 1));
    }

    #[test]
    fn graphics_pipeline_updates_land_on_the_canvas() {
        use ironrdp_pdu::geometry::ExclusiveRectangle;
        let mut canvas = Canvas::new(4, 3);
        // A 2x2 update at (1, 1): exclusive right/bottom.
        let region = ExclusiveRectangle {
            left: 1,
            top: 1,
            right: 3,
            bottom: 3,
        };
        let pixels: Vec<u8> = (0..16).collect();
        assert_eq!(canvas.blit(&region, &pixels), Some((1, 1, 2, 2)));
        assert_eq!(&canvas.data[(4 + 1) * 4..(4 + 3) * 4], &pixels[..8]);
        assert_eq!(&canvas.data[(8 + 1) * 4..(8 + 3) * 4], &pixels[8..]);
        // Clipped at the edge; short data is ignored, not a panic.
        let wide = ExclusiveRectangle {
            left: 3,
            top: 0,
            right: 6,
            bottom: 1,
        };
        assert_eq!(canvas.blit(&wide, &[7; 12]), Some((3, 0, 3, 0)));
        assert_eq!(canvas.blit(&wide, &[7; 2]), None);
        let outside = ExclusiveRectangle {
            left: 4,
            top: 0,
            right: 5,
            bottom: 1,
        };
        assert_eq!(canvas.blit(&outside, &[0; 4]), None);
    }

    #[test]
    fn dom_buttons_and_keys_map_to_rdp_input() {
        use ironrdp_input::{MouseButton, Operation};
        assert!(matches!(
            to_operation(ViewerInput::Button {
                button: 2,
                down: true
            }),
            Some(Operation::MouseButtonPressed(MouseButton::Right))
        ));
        assert!(
            to_operation(ViewerInput::Button {
                button: 9,
                down: true
            })
            .is_none()
        );
        let Some(Operation::KeyPressed(s)) = to_operation(ViewerInput::Key {
            scancode: 0xE04B,
            down: true,
        }) else {
            panic!("key")
        };
        assert_eq!(s.as_u8(), (true, 0x4B), "extended Left arrow");
    }
}
