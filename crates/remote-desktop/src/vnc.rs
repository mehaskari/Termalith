//! Embedded VNC (M-013, ADR-035): the same viewer contract as embedded RDP
//! (`Viewer`, `ViewerEvent`, `ViewerInput`), built on vnc-rs (MIT/Apache).
//! VNC itself is not encrypted; the app offers it through an SSH tunnel.

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use vnc::{PixelFormat, VncClient, VncConnector, VncEncoding, VncError, VncEvent, X11Event};
use zeroize::Zeroizing;

use crate::DesktopError;
use crate::embedded::{
    Control, MAX_FRAMES_IN_FLIGHT, Viewer, ViewerEvent, ViewerInput, crop, union,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// How often the server is asked for changes while frames are flowing.
const REFRESH: Duration = Duration::from_millis(33);

pub struct VncParams<'a> {
    pub host: &'a str,
    pub port: u16,
    /// Sent only if the server asks for one (VNC authentication).
    pub password: Option<Zeroizing<String>>,
}

fn failed(what: &str, e: impl std::fmt::Display) -> DesktopError {
    DesktopError::Launch {
        client: "embedded VNC".into(),
        reason: format!("{what}: {e}"),
    }
}

fn vnc_error(e: VncError) -> DesktopError {
    match e {
        VncError::NoPassword => DesktopError::PasswordNeeded,
        VncError::WrongPassword => DesktopError::AuthFailed,
        e => failed("login", e),
    }
}

async fn connect(p: &VncParams<'_>) -> Result<VncClient, DesktopError> {
    let tcp = tokio::time::timeout(
        CONNECT_TIMEOUT,
        tokio::net::TcpStream::connect((p.host, p.port)),
    )
    .await
    .map_err(|_| failed("connect", "timed out"))?
    .map_err(|e| failed("connect", e))?;
    let password = p.password.clone();
    let connector = VncConnector::new(tcp)
        .set_auth_method(async move {
            password
                .map(|p| p.as_str().to_owned())
                .ok_or(VncError::NoPassword)
        })
        .add_encoding(VncEncoding::Zrle)
        .add_encoding(VncEncoding::Trle)
        .add_encoding(VncEncoding::CopyRect)
        .add_encoding(VncEncoding::Raw)
        .add_encoding(VncEncoding::DesktopSizePseudo)
        .add_encoding(VncEncoding::ExtendedDesktopSizePseudo)
        .add_encoding(VncEncoding::LastRectPseudo)
        .allow_shared(true)
        .set_pixel_format(PixelFormat::rgba())
        .build()
        .map_err(vnc_error)?;
    let started = tokio::time::timeout(CONNECT_TIMEOUT, connector.try_start())
        .await
        .map_err(|_| failed("login", "timed out"))?
        .map_err(vnc_error)?;
    started.finish().map_err(vnc_error)
}

/// Connects and logs in (blocking; call off the UI thread), then runs the
/// session on its own thread, like [`crate::embedded::start`].
pub fn start(
    p: &VncParams<'_>,
    sink: impl FnMut(ViewerEvent) -> bool + Send + 'static,
) -> Result<Viewer, DesktopError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| failed("runtime", e))?;
    let client = runtime.block_on(connect(p))?;
    let (tx, rx) = std::sync::mpsc::sync_channel(256);
    std::thread::Builder::new()
        .name("vnc-session".into())
        .spawn(move || {
            let mut sink = sink;
            let reason = runtime.block_on(run(client, rx, &mut sink)).err();
            sink(ViewerEvent::Closed { reason });
        })
        .map_err(|e| failed("thread", e))?;
    Ok(Viewer::new(tx))
}

/// X11 keysym for a PC/AT scan code; printable keys arrive as characters.
pub(crate) fn keysym(scancode: u16) -> Option<u32> {
    Some(match scancode {
        0x01 => 0xff1b,                                     // Escape
        0x0e => 0xff08,                                     // BackSpace
        0x0f => 0xff09,                                     // Tab
        0x1c => 0xff0d,                                     // Return
        0x1d => 0xffe3,                                     // Control_L
        0x2a => 0xffe1,                                     // Shift_L
        0x36 => 0xffe2,                                     // Shift_R
        0x38 => 0xffe9,                                     // Alt_L
        0x39 => 0x0020,                                     // space
        0x3a => 0xffe5,                                     // Caps_Lock
        0x3b..=0x44 => 0xffbe + u32::from(scancode - 0x3b), // F1-F10
        0x57 => 0xffc8,                                     // F11
        0x58 => 0xffc9,                                     // F12
        0xe01c => 0xff8d,                                   // KP_Enter
        0xe01d => 0xffe4,                                   // Control_R
        0xe038 => 0xffea,                                   // Alt_R
        0xe047 => 0xff50,                                   // Home
        0xe048 => 0xff52,                                   // Up
        0xe049 => 0xff55,                                   // Prior
        0xe04b => 0xff51,                                   // Left
        0xe04d => 0xff53,                                   // Right
        0xe04f => 0xff57,                                   // End
        0xe050 => 0xff54,                                   // Down
        0xe051 => 0xff56,                                   // Next
        0xe052 => 0xff63,                                   // Insert
        0xe053 => 0xffff,                                   // Delete
        0xe05b => 0xffeb,                                   // Super_L
        0xe05c => 0xffec,                                   // Super_R
        _ => return None,
    })
}

/// X11 keysym for a character: Latin-1 as is, the rest as Unicode keysyms.
pub(crate) fn char_keysym(ch: char) -> u32 {
    let c = u32::from(ch);
    if c < 0x100 { c } else { 0x0100_0000 + c }
}

/// The desktop image, RGBA.
struct Framebuffer {
    width: u16,
    height: u16,
    data: Vec<u8>,
}

impl Framebuffer {
    fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            data: vec![0; usize::from(width) * usize::from(height) * 4],
        }
    }

    /// Copies a rectangle in; returns the inclusive area it changed.
    fn put(&mut self, x: u16, y: u16, w: u16, h: u16, rgba: &[u8]) -> Option<(u16, u16, u16, u16)> {
        let right = x.checked_add(w)?.min(self.width);
        let bottom = y.checked_add(h)?.min(self.height);
        if x >= right || y >= bottom || rgba.len() < usize::from(w) * usize::from(h) * 4 {
            return None;
        }
        let (cw, stride) = (usize::from(right - x) * 4, usize::from(self.width) * 4);
        for row in 0..usize::from(bottom - y) {
            let src = row * usize::from(w) * 4;
            let dst = (usize::from(y) + row) * stride + usize::from(x) * 4;
            self.data[dst..dst + cw].copy_from_slice(&rgba[src..src + cw]);
        }
        Some((x, y, right - 1, bottom - 1))
    }

    /// Copies one area of the screen to another (CopyRect).
    fn copy(
        &mut self,
        dst: (u16, u16),
        src: (u16, u16),
        w: u16,
        h: u16,
    ) -> Option<(u16, u16, u16, u16)> {
        let (sx, sy) = src;
        if sx.checked_add(w)? > self.width || sy.checked_add(h)? > self.height {
            return None;
        }
        let stride = usize::from(self.width) * 4;
        let mut block = Vec::with_capacity(usize::from(w) * usize::from(h) * 4);
        for row in 0..usize::from(h) {
            let at = (usize::from(sy) + row) * stride + usize::from(sx) * 4;
            block.extend_from_slice(&self.data[at..at + usize::from(w) * 4]);
        }
        self.put(dst.0, dst.1, w, h, &block)
    }
}

/// When sending fails, the engine has stopped; the reason waits among the
/// events (a decoding error stops the network side too).
async fn engine_error(client: &VncClient, e: VncError) -> String {
    while let Ok(Some(ev)) = client.poll_event().await {
        if let VncEvent::Error(reason) = ev {
            return format!("VNC: {reason}");
        }
    }
    format!("input: {e}")
}

async fn run(
    client: VncClient,
    rx: Receiver<Control>,
    sink: &mut impl FnMut(ViewerEvent) -> bool,
) -> Result<(), String> {
    let mut fb: Option<Framebuffer> = None;
    let mut dirty: Option<(u16, u16, u16, u16)> = None;
    let mut in_flight = 0usize;
    let (mut px, mut py, mut buttons) = (0u16, 0u16, 0u8);
    let mut pressed: HashSet<u32> = HashSet::new();
    let mut last_refresh = tokio::time::Instant::now();
    let input = |ev: X11Event| {
        let c = client.clone();
        async move {
            match c.input(ev).await {
                Ok(()) => Ok(()),
                Err(e) => Err(engine_error(&c, e).await),
            }
        }
    };
    loop {
        loop {
            match rx.try_recv() {
                Ok(Control::Input(events)) => {
                    for e in events {
                        match e {
                            ViewerInput::MouseMove { x, y } => {
                                (px, py) = (x, y);
                                input(X11Event::PointerEvent((px, py, buttons).into())).await?;
                            }
                            ViewerInput::Button { button, down } => {
                                let bit = match button {
                                    0 => 1,
                                    1 => 2,
                                    2 => 4,
                                    _ => continue,
                                };
                                buttons = if down { buttons | bit } else { buttons & !bit };
                                input(X11Event::PointerEvent((px, py, buttons).into())).await?;
                            }
                            ViewerInput::Wheel { vertical, units } => {
                                // A wheel step is a press and release of a button.
                                let bit = match (vertical, units > 0) {
                                    (true, true) => 8,
                                    (true, false) => 16,
                                    (false, true) => 32,
                                    (false, false) => 64,
                                };
                                input(X11Event::PointerEvent((px, py, buttons | bit).into()))
                                    .await?;
                                input(X11Event::PointerEvent((px, py, buttons).into())).await?;
                            }
                            ViewerInput::Key { scancode, down } => {
                                if let Some(k) = keysym(scancode) {
                                    if down {
                                        pressed.insert(k);
                                    } else {
                                        pressed.remove(&k);
                                    }
                                    input(X11Event::KeyEvent((k, down).into())).await?;
                                }
                            }
                            ViewerInput::Unicode { ch, down } => {
                                let k = char_keysym(ch);
                                if down {
                                    pressed.insert(k);
                                } else {
                                    pressed.remove(&k);
                                }
                                input(X11Event::KeyEvent((k, down).into())).await?;
                            }
                            ViewerInput::ReleaseAll => {
                                for k in pressed.drain() {
                                    input(X11Event::KeyEvent((k, false).into())).await?;
                                }
                                if buttons != 0 {
                                    buttons = 0;
                                    input(X11Event::PointerEvent((px, py, 0).into())).await?;
                                }
                            }
                        }
                    }
                }
                Ok(Control::Resize(w, h, _)) => {
                    // Waits for the server's answer, which comes with the
                    // events drained below: not awaited here.
                    let c = client.clone();
                    tokio::spawn(async move {
                        let _ = c.resize_desktop(w, h).await;
                    });
                }
                Ok(Control::Clipboard(text)) => {
                    // RFB clipboard text is Latin-1 only.
                    if text.chars().all(|c| u32::from(c) < 0x100) {
                        input(X11Event::CopyText(text)).await?;
                    }
                }
                Ok(Control::Ack) => in_flight = in_flight.saturating_sub(1),
                Ok(Control::Close) | Err(TryRecvError::Disconnected) => {
                    let _ = client.close().await;
                    return Ok(());
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        let mut got = false;
        while let Some(ev) = client
            .poll_event()
            .await
            .map_err(|e| format!("connection lost: {e}"))?
        {
            got = true;
            match ev {
                VncEvent::SetResolution(s) => {
                    fb = Some(Framebuffer::new(s.width, s.height));
                    dirty = None;
                    in_flight = 0;
                    if !sink(ViewerEvent::Connected {
                        width: s.width,
                        height: s.height,
                    }) {
                        return Ok(());
                    }
                }
                VncEvent::RawImage(r, data) => {
                    if let Some(f) = fb.as_mut()
                        && let Some(area) = f.put(r.x, r.y, r.width, r.height, &data)
                    {
                        dirty = Some(union(dirty, area));
                    }
                }
                VncEvent::Copy(dst, src) => {
                    if let Some(f) = fb.as_mut()
                        && let Some(area) =
                            f.copy((dst.x, dst.y), (src.x, src.y), dst.width, dst.height)
                    {
                        dirty = Some(union(dirty, area));
                    }
                }
                VncEvent::Text(text) => {
                    if !sink(ViewerEvent::Clipboard(text)) {
                        return Ok(());
                    }
                }
                VncEvent::Error(e) => return Err(e),
                _ => {}
            }
        }
        if in_flight < MAX_FRAMES_IN_FLIGHT
            && let (Some(f), Some(r)) = (fb.as_ref(), dirty)
        {
            dirty = None;
            in_flight += 1;
            if !sink(crop(&f.data, f.width, f.height, r)) {
                return Ok(());
            }
        }
        if in_flight < MAX_FRAMES_IN_FLIGHT && last_refresh.elapsed() >= REFRESH {
            last_refresh = tokio::time::Instant::now();
            input(X11Event::Refresh).await?;
        }
        if !got {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_keysyms() {
        assert_eq!(keysym(0x1c), Some(0xff0d));
        assert_eq!(keysym(0x3b), Some(0xffbe));
        assert_eq!(keysym(0x44), Some(0xffc7));
        assert_eq!(keysym(0xe053), Some(0xffff));
        assert_eq!(keysym(0x1e), None, "letters come as characters");
        assert_eq!(char_keysym('a'), 0x61);
        assert_eq!(char_keysym('é'), 0xe9);
        assert_eq!(char_keysym('س'), 0x0100_0633);
    }

    #[test]
    fn framebuffer_puts_and_copies_with_clipping() {
        let mut f = Framebuffer::new(4, 3);
        let px: Vec<u8> = (0..16).collect();
        assert_eq!(f.put(1, 1, 2, 2, &px), Some((1, 1, 2, 2)));
        assert_eq!(&f.data[(4 + 1) * 4..(4 + 3) * 4], &px[..8]);
        // Clipped at the right edge; short data is refused.
        assert_eq!(f.put(3, 0, 2, 1, &[7; 8]), Some((3, 0, 3, 0)));
        assert_eq!(f.put(0, 0, 2, 2, &[0; 4]), None);
        // CopyRect moves pixels within the screen.
        assert_eq!(f.copy((0, 0), (1, 1), 2, 1), Some((0, 0, 1, 0)));
        assert_eq!(&f.data[..8], &px[..8]);
        assert_eq!(f.copy((0, 0), (3, 2), 2, 1), None, "source outside");
    }
}
