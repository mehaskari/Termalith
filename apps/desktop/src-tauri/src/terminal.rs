//! Terminal IPC commands, shared by local and SSH terminals.
//!
//! For local terminals the frontend can only ask for "a terminal of this
//! size"; it cannot choose the program, arguments, working directory or
//! environment. The backend resolves the shell from OS configuration.

use serde::Serialize;
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};
use terminal_pty::{ExitInfo, TerminalEvent, TerminalSize};

use crate::error::IpcError;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitPayload {
    /// `None` when the exit status could not be determined.
    exit: Option<ExitStatusPayload>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExitStatusPayload {
    code: u32,
    signal: Option<String>,
}

impl From<Option<ExitInfo>> for ExitPayload {
    fn from(info: Option<ExitInfo>) -> Self {
        Self {
            exit: info.map(|i| ExitStatusPayload {
                code: i.code,
                signal: i.signal,
            }),
        }
    }
}

/// Forwards terminal events to the two frontend channels.
pub fn channel_sink(
    on_output: Channel<InvokeResponseBody>,
    on_exit: Channel<ExitPayload>,
) -> impl FnMut(TerminalEvent) -> bool + Send + 'static {
    move |event| match event {
        TerminalEvent::Output(bytes) => on_output.send(InvokeResponseBody::Raw(bytes)).is_ok(),
        TerminalEvent::Exited(info) => on_exit.send(ExitPayload::from(info)).is_ok(),
    }
}

#[tauri::command]
pub async fn terminal_open(
    state: State<'_, AppState>,
    cols: u16,
    rows: u16,
    on_output: Channel<InvokeResponseBody>,
    on_exit: Channel<ExitPayload>,
) -> Result<u64, IpcError> {
    let size = TerminalSize::new(cols, rows)?;
    let hub = state.hub.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let shell = terminal_pty::default_shell()?;
        Ok(hub.open_local(&shell, size, channel_sink(on_output, on_exit))?)
    })
    .await
    .map_err(|_| IpcError::internal("terminal task failed"))?
}

#[tauri::command]
pub async fn terminal_write(
    state: State<'_, AppState>,
    id: u64,
    data: String,
) -> Result<(), IpcError> {
    Ok(state.hub.write(id, data.as_bytes()).await?)
}

#[tauri::command]
pub async fn terminal_resize(
    state: State<'_, AppState>,
    id: u64,
    cols: u16,
    rows: u16,
) -> Result<(), IpcError> {
    let size = TerminalSize::new(cols, rows)?;
    Ok(state.hub.resize(id, size).await?)
}

#[tauri::command]
pub async fn terminal_ack(state: State<'_, AppState>, id: u64, bytes: u32) -> Result<(), IpcError> {
    Ok(state.hub.ack(id, bytes as usize)?)
}

#[tauri::command]
pub async fn terminal_close(state: State<'_, AppState>, id: u64) -> Result<(), IpcError> {
    match state.hub.close(id).await {
        Ok(()) | Err(app_core::AppError::NotFound) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Opens a second terminal next to `source`: another local shell, or
/// another shell on the same SSH connection (no new login).
#[tauri::command]
pub async fn terminal_split(
    state: State<'_, AppState>,
    source: u64,
    cols: u16,
    rows: u16,
    on_output: Channel<InvokeResponseBody>,
    on_exit: Channel<ExitPayload>,
) -> Result<u64, IpcError> {
    let size = TerminalSize::new(cols, rows)?;
    let sink = channel_sink(on_output, on_exit);
    if state.hub.is_ssh(source)? {
        return Ok(state.hub.open_ssh_sibling(source, size, sink).await?);
    }
    let hub = state.hub.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let shell = terminal_pty::default_shell()?;
        Ok(hub.open_local(&shell, size, sink)?)
    })
    .await
    .map_err(|_| IpcError::internal("terminal task failed"))?
}
