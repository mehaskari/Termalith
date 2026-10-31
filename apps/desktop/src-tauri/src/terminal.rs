//! Terminal IPC commands.
//!
//! The frontend can only ask for "a terminal of this size". It cannot choose
//! the program, arguments, working directory or environment; the backend
//! resolves the shell from OS configuration.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};
use terminal_pty::{ExitInfo, TerminalEvent, TerminalId, TerminalManager, TerminalSize};

use crate::error::IpcError;

type Terminals<'a> = State<'a, Arc<TerminalManager>>;

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

/// Runs blocking terminal work off the main thread and the async executor.
async fn blocking<T, F>(f: F) -> Result<T, IpcError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, IpcError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|_| IpcError::internal("terminal task failed"))?
}

#[tauri::command]
pub async fn terminal_open(
    terminals: Terminals<'_>,
    cols: u16,
    rows: u16,
    on_output: Channel<InvokeResponseBody>,
    on_exit: Channel<ExitPayload>,
) -> Result<u64, IpcError> {
    let size = TerminalSize::new(cols, rows)?;
    let terminals = Arc::clone(&terminals);
    blocking(move || {
        let shell = terminal_pty::default_shell()?;
        let id = terminals.open(&shell, size, move |event| match event {
            TerminalEvent::Output(bytes) => on_output.send(InvokeResponseBody::Raw(bytes)).is_ok(),
            TerminalEvent::Exited(info) => on_exit.send(ExitPayload::from(info)).is_ok(),
        })?;
        Ok(id.as_u64())
    })
    .await
}

#[tauri::command]
pub async fn terminal_write(
    terminals: Terminals<'_>,
    id: u64,
    data: String,
) -> Result<(), IpcError> {
    let terminals = Arc::clone(&terminals);
    blocking(move || Ok(terminals.write(TerminalId::from_u64(id), data.as_bytes())?)).await
}

#[tauri::command]
pub async fn terminal_resize(
    terminals: Terminals<'_>,
    id: u64,
    cols: u16,
    rows: u16,
) -> Result<(), IpcError> {
    let size = TerminalSize::new(cols, rows)?;
    Ok(terminals.resize(TerminalId::from_u64(id), size)?)
}

#[tauri::command]
pub async fn terminal_ack(terminals: Terminals<'_>, id: u64, bytes: u32) -> Result<(), IpcError> {
    Ok(terminals.ack(TerminalId::from_u64(id), bytes as usize)?)
}

#[tauri::command]
pub async fn terminal_close(terminals: Terminals<'_>, id: u64) -> Result<(), IpcError> {
    let terminals = Arc::clone(&terminals);
    blocking(move || Ok(terminals.close(TerminalId::from_u64(id))?)).await
}
