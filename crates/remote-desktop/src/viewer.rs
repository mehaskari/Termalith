//! What every embedded viewer (RDP in `embedded.rs`, VNC in `vnc.rs`)
//! shares with the app: the events a session sends, the input it takes,
//! the handle that controls it, and the frame helpers both protocols use.
//! Frames reach the UI with a 12-byte LE header (x, y, w, h, desktop w, h).

/// Events from a live session to the viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerEvent {
    /// Size of the remote desktop; frames refer to it.
    Connected { width: u16, height: u16 },
    /// Changed pixels, RGBA, `w * h * 4` bytes, row by row. `desktop` is
    /// the whole desktop's size, so a viewer can follow a resize with the
    /// pixels themselves.
    Frame {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        desktop: (u16, u16),
        rgba: Vec<u8>,
    },
    /// The remote computer copied this text.
    Clipboard(String),
    /// The remote pointer's shape changed.
    Cursor(Cursor),
    /// The session ended; `reason` is set when it was not a normal close.
    Closed { reason: Option<String> },
}

/// The remote pointer's shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cursor {
    /// The viewer's normal arrow.
    Default,
    Hidden,
    /// RGBA (not premultiplied), `width * height * 4` bytes; the hot spot is
    /// the pixel that points.
    Image {
        width: u16,
        height: u16,
        hot_x: u16,
        hot_y: u16,
        rgba: Vec<u8>,
    },
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

pub(crate) enum Control {
    Input(Vec<ViewerInput>),
    /// Ask the server for a new desktop size and scale (it may adjust or
    /// refuse it).
    Resize(u16, u16, u32),
    /// Text copied on this computer, offered to the remote.
    Clipboard(String),
    /// The viewer drew one frame.
    Ack,
    Close,
}

/// Frames sent but not yet drawn; more changes are merged meanwhile.
pub(crate) const MAX_FRAMES_IN_FLIGHT: usize = 2;

/// Handle to a running session. Dropping it closes the session.
pub struct Viewer {
    tx: std::sync::mpsc::SyncSender<Control>,
}

impl Viewer {
    pub(crate) fn new(tx: std::sync::mpsc::SyncSender<Control>) -> Self {
        Self { tx }
    }
}

impl Viewer {
    pub fn input(&self, events: Vec<ViewerInput>) -> bool {
        self.tx.send(Control::Input(events)).is_ok()
    }

    /// Asks the server to change the desktop size and display scale (in
    /// percent); a `Connected` event with the new size follows when it does.
    pub fn resize(&self, width: u16, height: u16, scale: u32) -> bool {
        self.tx.send(Control::Resize(width, height, scale)).is_ok()
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

/// Smallest rectangle covering both (inclusive coordinates).
pub(crate) fn union(
    a: Option<(u16, u16, u16, u16)>,
    b: (u16, u16, u16, u16),
) -> (u16, u16, u16, u16) {
    match a {
        None => b,
        Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
    }
}

/// RGBA pixels of an inclusive rectangle, alpha forced opaque.
pub(crate) fn crop(data: &[u8], width: u16, height: u16, r: (u16, u16, u16, u16)) -> ViewerEvent {
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
        desktop: (width, height),
        rgba,
    }
}

/// Diagnostics for tests and troubleshooting (`TERMALITH_RDP_DEBUG=1`,
/// for RDP and VNC alike), prefixed with the protocol; never includes
/// credentials or screen contents.
pub(crate) fn debug(protocol: &str, message: &str) {
    if std::env::var_os("TERMALITH_RDP_DEBUG").is_some() {
        eprintln!("{protocol}: {message}");
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
}
