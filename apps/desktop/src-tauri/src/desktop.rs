//! RDP/VNC IPC: open a saved remote desktop session in an external client
//! (optionally through an SSH tunnel), list and close them.

use app_core::{AppError, DesktopHub, DesktopInfo, DesktopLaunch, SessionId, SessionProtocol};
use remote_desktop::{DesktopTarget, Protocol};
use serde::Serialize;
use tauri::State;
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
    let info = state
        .desktops
        .open(DesktopLaunch {
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
        .await?;
    Ok(info)
}

#[tauri::command]
pub fn desktop_list(state: State<'_, AppState>) -> Vec<DesktopInfo> {
    state.desktops.list()
}

#[tauri::command]
pub fn desktop_close(state: State<'_, AppState>, id: u64) -> Result<(), IpcError> {
    Ok(state.desktops.close(id)?)
}
