//! RDP/VNC IPC: open a saved remote desktop session in an external client
//! (optionally through an SSH tunnel), list and close them; and the
//! experimental embedded RDP viewer (ADR-030).

use app_core::{AppError, DesktopHub, DesktopInfo, DesktopLaunch, SessionId, SessionProtocol};
use remote_desktop::embedded::{Cursor, ViewerEvent, ViewerInput};
use remote_desktop::{DesktopError, DesktopTarget, Protocol};
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
        SessionProtocol::Ssh
        | SessionProtocol::Telnet
        | SessionProtocol::Ftp
        | SessionProtocol::Ftps => {
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

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CursorImageDto {
    width: u16,
    height: u16,
    hot_x: u16,
    hot_y: u16,
    rgba: Vec<u8>,
}

impl From<Cursor> for EmbeddedEventDto {
    fn from(c: Cursor) -> Self {
        match c {
            Cursor::Default => EmbeddedEventDto::Cursor {
                hidden: false,
                image: None,
            },
            Cursor::Hidden => EmbeddedEventDto::Cursor {
                hidden: true,
                image: None,
            },
            Cursor::Image {
                width,
                height,
                hot_x,
                hot_y,
                rgba,
            } => EmbeddedEventDto::Cursor {
                hidden: false,
                image: Some(CursorImageDto {
                    width,
                    height,
                    hot_x,
                    hot_y,
                    rgba,
                }),
            },
        }
    }
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
    /// The remote pointer's shape: `image` is RGBA; none with `hidden`
    /// false means the normal arrow.
    #[serde(rename_all = "camelCase")]
    Cursor {
        hidden: bool,
        image: Option<CursorImageDto>,
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
fn encode_frame(rect: [u16; 4], desktop: (u16, u16), rgba: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + rgba.len());
    let [x, y, w, h] = rect;
    for v in [x, y, w, h, desktop.0, desktop.1] {
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
    login: EmbeddedLogin,
    width: u16,
    height: u16,
    scale: u32,
    on_frame: Channel<InvokeResponseBody>,
    on_event: Channel<EmbeddedEventDto>,
) -> Result<EmbeddedOpened, IpcError> {
    let EmbeddedLogin {
        session_id,
        password,
        save_password,
    } = login;
    let id = SessionId::parse(&session_id)?;
    let rec = state.connector.sessions.get(&id)?;
    let protocol = match rec.input.protocol {
        SessionProtocol::Rdp => Protocol::Rdp,
        SessionProtocol::Vnc => Protocol::Vnc,
        _ => {
            return Err(AppError::Invalid {
                field: "session",
                reason: "only RDP and VNC can be shown inside Termalith".into(),
            }
            .into());
        }
    };
    // A typed password wins; otherwise the saved one is tried.
    let typed = password.map(Zeroizing::new);
    let credential = rec.credential.clone();
    let used_saved = typed.is_none() && credential.is_some();
    let secret = match &typed {
        Some(p) => Some(p.clone()),
        None => state.connector.saved_secret(credential.clone()).await,
    };
    // VNC servers often need no password: it is asked for only if they do.
    if secret.is_none() && protocol == Protocol::Rdp {
        return Err(AppError::PasswordRequired.into());
    }
    let launch = launch_for(&state, id.clone(), rec, protocol, secret).await?;
    let sink = move |ev: ViewerEvent| match ev {
        ViewerEvent::Frame {
            x,
            y,
            w,
            h,
            desktop,
            rgba,
        } => on_frame
            .send(InvokeResponseBody::Raw(encode_frame(
                [x, y, w, h],
                desktop,
                rgba,
            )))
            .is_ok(),
        ViewerEvent::Connected { width, height } => on_event
            .send(EmbeddedEventDto::Connected { width, height })
            .is_ok(),
        ViewerEvent::Clipboard(text) => on_event.send(EmbeddedEventDto::Clipboard { text }).is_ok(),
        ViewerEvent::Cursor(c) => on_event.send(c.into()).is_ok(),
        ViewerEvent::Closed { reason } => {
            on_event.send(EmbeddedEventDto::Closed { reason }).is_ok()
        }
    };
    let viewer = match state
        .desktops
        .open_embedded(launch, (width, height, scale.clamp(100, 500)), sink)
        .await
    {
        Ok(viewer) => viewer,
        // A saved password that no longer works: ask the user again.
        Err(AppError::Desktop(DesktopError::AuthFailed)) if used_saved => {
            return Err(AppError::PasswordRequired.into());
        }
        Err(AppError::Desktop(DesktopError::PasswordNeeded)) => {
            return Err(AppError::PasswordRequired.into());
        }
        Err(e) => return Err(e.into()),
    };
    let save = save_password && state.secrets_available;
    let save_error = state.connector.remember(&id, credential, typed, save).await;
    Ok(EmbeddedOpened {
        id: viewer,
        save_error,
    })
}

/// Which session to open and, if the user typed one, its password.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedLogin {
    session_id: String,
    password: Option<String>,
    save_password: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedOpened {
    id: u64,
    /// Why a password that should be saved was not. The login still worked.
    save_error: Option<String>,
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
    scale: u32,
) -> Result<(), IpcError> {
    Ok(state
        .desktops
        .embedded_resize(id, width, height, scale.clamp(100, 500))?)
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
        let f = encode_frame([1, 2, 3, 0x0104], (1920, 1080), vec![9; 4]);
        assert_eq!(&f[..8], &[1, 0, 2, 0, 3, 0, 4, 1]);
        assert_eq!(&f[8..12], &[0x80, 7, 0x38, 4]);
        assert_eq!(&f[12..], &[9; 4]);
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
