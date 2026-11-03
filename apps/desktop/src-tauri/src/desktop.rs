//! RDP/VNC IPC: open a saved remote desktop session in an external client
//! (optionally through an SSH tunnel), list and close them; and the
//! experimental embedded RDP viewer (ADR-030).

use app_core::{AppError, DesktopHub, DesktopInfo, DesktopLaunch, SessionId, SessionProtocol};
use remote_desktop::embedded::{ViewerEvent, ViewerInput};
use remote_desktop::{DesktopTarget, Protocol};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};
use zeroize::Zeroizing;

use crate::error::IpcError;
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientChoice {
    name: &'static str,
    /// The client needs the password before it starts (FreeRDP); others
    /// ask for it in their own window.
    asks_password_first: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopClients {
    rdp: Option<ClientChoice>,
    vnc: Option<ClientChoice>,
}

fn choice(p: Protocol) -> Option<ClientChoice> {
    DesktopHub::preferred_client(p).ok().map(|c| ClientChoice {
        name: c.name,
        asks_password_first: c.reads_password(),
    })
}

/// The external clients that will be used, if installed.
#[tauri::command]
pub fn desktop_clients() -> DesktopClients {
    DesktopClients {
        rdp: choice(Protocol::Rdp),
        vnc: choice(Protocol::Vnc),
    }
}

/// Opens saved session `session_id`. The password (only sent when the
/// client reads it up front) lives for this call and is never stored.
#[tauri::command]
pub async fn desktop_open(
    state: State<'_, AppState>,
    session_id: String,
    password: Option<String>,
) -> Result<DesktopInfo, IpcError> {
    let id = SessionId::parse(&session_id)?;
    let rec = state.connector.sessions.get(&id)?;
    let protocol = match rec.input.protocol {
        SessionProtocol::Rdp => Protocol::Rdp,
        SessionProtocol::Vnc => Protocol::Vnc,
        SessionProtocol::Ssh | SessionProtocol::Telnet => {
            return Err(AppError::Invalid {
                field: "session",
                reason: "not a remote desktop session".into(),
            }
            .into());
        }
    };
    let password = password.map(Zeroizing::new);
    // Ask for a needed password before connecting any tunnel.
    if DesktopHub::preferred_client(protocol)?.reads_password() && password.is_none() {
        return Err(AppError::PasswordRequired.into());
    }
    let launch = launch_for(&state, id, rec, protocol, password).await?;
    let info = state.desktops.open(launch).await?;
    Ok(info)
}

/// Builds the launch for a saved session, connecting its jump host first.
async fn launch_for(
    state: &AppState,
    id: SessionId,
    rec: app_core::SessionRecord,
    protocol: Protocol,
    password: Option<Zeroizing<String>>,
) -> Result<DesktopLaunch, IpcError> {
    let tunnel = match &rec.input.jump_via {
        Some(jump) => {
            let name = state
                .connector
                .sessions
                .get(jump)
                .map(|j| j.input.name)
                .unwrap_or_else(|_| "jump host".into());
            Some((state.connector.connect_tunnel(jump).await?, name))
        }
        None => None,
    };
    let username = Some(rec.input.username.clone()).filter(|u| !u.is_empty());
    Ok(DesktopLaunch {
        label: rec.input.name.clone(),
        session_id: Some(id),
        target: DesktopTarget {
            protocol,
            host: rec.input.host.clone(),
            port: rec.input.port,
            username,
            domain: rec.input.domain.clone(),
        },
        password,
        tunnel,
    })
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EmbeddedEventDto {
    Connected {
        width: u16,
        height: u16,
    },
    /// The remote computer copied this text.
    Clipboard {
        text: String,
    },
    Closed {
        reason: Option<String>,
    },
}

/// Input from the viewer; keys are PC/AT scan codes.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EmbeddedInputDto {
    Move { x: u16, y: u16 },
    Button { button: u8, down: bool },
    Wheel { vertical: bool, units: i16 },
    Key { scancode: u16, down: bool },
    Text { ch: char, down: bool },
    ReleaseAll,
}

impl From<EmbeddedInputDto> for ViewerInput {
    fn from(d: EmbeddedInputDto) -> Self {
        match d {
            EmbeddedInputDto::Move { x, y } => ViewerInput::MouseMove { x, y },
            EmbeddedInputDto::Button { button, down } => ViewerInput::Button { button, down },
            EmbeddedInputDto::Wheel { vertical, units } => ViewerInput::Wheel { vertical, units },
            EmbeddedInputDto::Key { scancode, down } => ViewerInput::Key { scancode, down },
            EmbeddedInputDto::Text { ch, down } => ViewerInput::Unicode { ch, down },
            EmbeddedInputDto::ReleaseAll => ViewerInput::ReleaseAll,
        }
    }
}

/// Frame wire format: x, y, w, h as little-endian u16, then RGBA rows.
fn encode_frame(x: u16, y: u16, w: u16, h: u16, rgba: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + rgba.len());
    for v in [x, y, w, h] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&rgba);
    out
}

/// Opens saved RDP session `session_id` inside Termalith (experimental).
/// The password lives for this call only.
#[tauri::command]
pub async fn embedded_open(
    state: State<'_, AppState>,
    session_id: String,
    password: Option<String>,
    width: u16,
    height: u16,
    on_frame: Channel<InvokeResponseBody>,
    on_event: Channel<EmbeddedEventDto>,
) -> Result<u64, IpcError> {
    let id = SessionId::parse(&session_id)?;
    let rec = state.connector.sessions.get(&id)?;
    if rec.input.protocol != SessionProtocol::Rdp {
        return Err(AppError::Invalid {
            field: "session",
            reason: "only RDP can be shown inside Termalith".into(),
        }
        .into());
    }
    let password = password.map(Zeroizing::new);
    if password.is_none() {
        return Err(AppError::PasswordRequired.into());
    }
    let launch = launch_for(&state, id, rec, Protocol::Rdp, password).await?;
    let sink = move |ev: ViewerEvent| match ev {
        ViewerEvent::Frame { x, y, w, h, rgba } => on_frame
            .send(InvokeResponseBody::Raw(encode_frame(x, y, w, h, rgba)))
            .is_ok(),
        ViewerEvent::Connected { width, height } => on_event
            .send(EmbeddedEventDto::Connected { width, height })
            .is_ok(),
        ViewerEvent::Clipboard(text) => on_event.send(EmbeddedEventDto::Clipboard { text }).is_ok(),
        ViewerEvent::Closed { reason } => {
            on_event.send(EmbeddedEventDto::Closed { reason }).is_ok()
        }
    };
    Ok(state
        .desktops
        .open_embedded(launch, width, height, sink)
        .await?)
}

#[tauri::command]
pub fn embedded_input(
    state: State<'_, AppState>,
    id: u64,
    events: Vec<EmbeddedInputDto>,
) -> Result<(), IpcError> {
    if events.len() > 256 {
        return Err(IpcError::coded("invalid", "too many input events at once"));
    }
    Ok(state
        .desktops
        .embedded_input(id, events.into_iter().map(Into::into).collect())?)
}

/// Largest clipboard text offered to the remote (1 MiB).
const MAX_CLIPBOARD: usize = 1024 * 1024;

/// Offers text copied on this computer to the remote one.
#[tauri::command]
pub fn embedded_clipboard(
    state: State<'_, AppState>,
    id: u64,
    text: String,
) -> Result<(), IpcError> {
    if text.len() > MAX_CLIPBOARD {
        return Err(IpcError::coded(
            "invalid",
            "the clipboard text is larger than 1 MiB",
        ));
    }
    Ok(state.desktops.embedded_clipboard(id, text)?)
}

/// Asks the server to resize the desktop to the viewer's size.
#[tauri::command]
pub fn embedded_resize(
    state: State<'_, AppState>,
    id: u64,
    width: u16,
    height: u16,
) -> Result<(), IpcError> {
    Ok(state.desktops.embedded_resize(id, width, height)?)
}

#[tauri::command]
pub fn embedded_ack(state: State<'_, AppState>, id: u64) -> Result<(), IpcError> {
    Ok(state.desktops.embedded_ack(id)?)
}

#[tauri::command]
pub fn embedded_close(state: State<'_, AppState>, id: u64) -> Result<(), IpcError> {
    Ok(state.desktops.embedded_close(id)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_header_is_little_endian() {
        let f = encode_frame(1, 2, 3, 0x0104, vec![9; 4]);
        assert_eq!(&f[..8], &[1, 0, 2, 0, 3, 0, 4, 1]);
        assert_eq!(&f[8..], &[9; 4]);
    }

    #[test]
    fn input_json_maps_to_viewer_input() {
        let v: Vec<EmbeddedInputDto> = serde_json::from_str(
            r#"[{"kind":"move","x":5,"y":6},{"kind":"key","scancode":57419,"down":true},{"kind":"releaseAll"}]"#,
        )
        .unwrap();
        let v: Vec<ViewerInput> = v.into_iter().map(Into::into).collect();
        assert_eq!(v[0], ViewerInput::MouseMove { x: 5, y: 6 });
        assert_eq!(
            v[1],
            ViewerInput::Key {
                scancode: 0xE04B,
                down: true
            }
        );
        assert_eq!(v[2], ViewerInput::ReleaseAll);
    }
}

#[tauri::command]
pub fn desktop_list(state: State<'_, AppState>) -> Vec<DesktopInfo> {
    state.desktops.list()
}

#[tauri::command]
pub fn desktop_close(state: State<'_, AppState>, id: u64) -> Result<(), IpcError> {
    Ok(state.desktops.close(id)?)
}
