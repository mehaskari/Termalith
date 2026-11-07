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
use crate::terminal::{ExitPayload, logged_sink};

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
    /// Why a password that should be saved was not.
    save_error: Option<String>,
}

fn connect_request(state: &AppState, request: SshOpenRequest) -> Result<ConnectRequest, IpcError> {
    let secret = match request.password {
        Some(p) => PasswordInput::Provided {
            secret: Zeroizing::new(p),
            save: request.save_password && state.secrets_available,
        },
        None => PasswordInput::None,
    };
    match (request.session_id, request.quick) {
        (Some(id), None) => Ok(ConnectRequest::Saved {
            id: SessionId::parse(&id)?,
            secret,
        }),
        (None, Some(q)) => Ok(ConnectRequest::Quick {
            host: q.host,
            port: q.port,
            username: q.username,
            auth: q.auth,
            secret,
        }),
        _ => Err(IpcError::internal(
            "exactly one of sessionId or quick is required",
        )),
    }
}

/// Connects like [`ssh_open`] but without a shell: the id is used with the
/// SFTP commands (file browser tab) and `terminal_close`.
#[tauri::command]
pub async fn ssh_files_open(
    state: State<'_, AppState>,
    request: SshOpenRequest,
) -> Result<SshOpened, IpcError> {
    let req = connect_request(&state, request)?;
    let connected = state.connector.connect(req).await?;
    Ok(SshOpened {
        id: state.hub.open_files(connected.session),
        label: connected.label,
        save_error: connected.save_error,
    })
}

/// Connects a saved FTP session; the id works like [`ssh_files_open`]'s.
#[tauri::command]
pub async fn ftp_files_open(
    state: State<'_, AppState>,
    request: SshOpenRequest,
) -> Result<SshOpened, IpcError> {
    let ConnectRequest::Saved { id, secret } = connect_request(&state, request)? else {
        return Err(IpcError::internal("FTP sessions must be saved"));
    };
    let (client, label, save_error) = state.connector.connect_ftp(&id, secret).await?;
    Ok(SshOpened {
        id: state.hub.open_ftp(std::sync::Arc::new(client)),
        label,
        save_error,
    })
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
    let req = connect_request(&state, request)?;
    let connected = state.connector.connect(req).await?;
    let (tap, sink) = logged_sink(on_output, on_exit);
    let id = state.hub.open_ssh(connected.session, size, sink).await?;
    state.logs.register(id, tap);
    Ok(SshOpened {
        id,
        label: connected.label,
        save_error: connected.save_error,
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

/// Answers a keyboard-interactive prompt; `None` cancels the login.
/// Answers are zeroized after being handed over.
#[tauri::command]
pub fn auth_prompt_respond(
    state: State<'_, AppState>,
    request_id: u64,
    answers: Option<Vec<String>>,
) -> bool {
    let answers = answers.map(|a| a.into_iter().map(Zeroizing::new).collect());
    state.auth_prompts.respond(request_id, answers)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgetResult {
    removed: usize,
    /// Keys still recorded in the user's own OpenSSH known_hosts, which
    /// Termalith does not edit.
    remaining_in_user_file: usize,
}

/// Removes recorded keys for a host after the user verified a changed key.
/// Only the app's own known_hosts file is modified.
#[tauri::command]
pub fn known_host_forget(
    state: State<'_, AppState>,
    host: String,
    port: u16,
) -> Result<ForgetResult, IpcError> {
    let r = state.connector.forget_host_key(&host, port)?;
    Ok(ForgetResult {
        removed: r.removed,
        remaining_in_user_file: r.remaining_read_only,
    })
}
