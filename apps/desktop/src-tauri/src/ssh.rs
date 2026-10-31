//! SSH IPC: open a terminal on a saved session or a quick-connect target,
//! and answer host-key prompts.

use app_core::{AuthMethod, ConnectRequest, PasswordInput, SessionId};
use protocol_ssh::HostKeyDecision;
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};
use terminal_pty::TerminalSize;
use zeroize::Zeroizing;

use crate::error::IpcError;
use crate::state::AppState;
use crate::terminal::{ExitPayload, channel_sink};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickTarget {
    host: String,
    port: u16,
    username: String,
    auth: AuthMethod,
}

/// Exactly one of `session_id` / `quick` must be set. The password, if any,
/// lives only for this call and is zeroized afterwards.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshOpenRequest {
    session_id: Option<String>,
    quick: Option<QuickTarget>,
    password: Option<String>,
    #[serde(default)]
    save_password: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshOpened {
    id: u64,
    label: String,
}

#[tauri::command]
pub async fn ssh_open(
    state: State<'_, AppState>,
    request: SshOpenRequest,
    cols: u16,
    rows: u16,
    on_output: Channel<InvokeResponseBody>,
    on_exit: Channel<ExitPayload>,
) -> Result<SshOpened, IpcError> {
    let size = TerminalSize::new(cols, rows)?;
    let secret = match request.password {
        Some(p) => PasswordInput::Provided {
            secret: Zeroizing::new(p),
            save: request.save_password && state.secrets_available,
        },
        None => PasswordInput::None,
    };
    let req = match (request.session_id, request.quick) {
        (Some(id), None) => ConnectRequest::Saved {
            id: SessionId::parse(&id)?,
            secret,
        },
        (None, Some(q)) => ConnectRequest::Quick {
            host: q.host,
            port: q.port,
            username: q.username,
            auth: q.auth,
            secret,
        },
        _ => {
            return Err(IpcError::internal(
                "exactly one of sessionId or quick is required",
            ));
        }
    };
    let connected = state.connector.connect(req).await?;
    let id = state
        .hub
        .open_ssh(connected.session, size, channel_sink(on_output, on_exit))
        .await?;
    Ok(SshOpened {
        id,
        label: connected.label,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HostKeyAnswer {
    Reject,
    AcceptOnce,
    AcceptAndSave,
}

#[tauri::command]
pub fn host_key_respond(
    state: State<'_, AppState>,
    request_id: u64,
    answer: HostKeyAnswer,
) -> bool {
    let decision = match answer {
        HostKeyAnswer::Reject => HostKeyDecision::Reject,
        HostKeyAnswer::AcceptOnce => HostKeyDecision::AcceptOnce,
        HostKeyAnswer::AcceptAndSave => HostKeyDecision::AcceptAndSave,
    };
    state.broker.respond(request_id, decision)
}
