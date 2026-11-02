//! Terminal IPC commands, shared by local and SSH terminals.
//!
//! For local terminals the frontend can only ask for "a terminal of this
//! size"; it cannot choose the program, arguments, working directory or
//! environment. The backend resolves the shell from OS configuration.

use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};

use app_core::{LogInfo, LogTap, tapped};
use terminal_pty::{ExitInfo, TerminalEvent, TerminalEventSink, TerminalSize};

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

/// [`channel_sink`] with a log tap in front; register the tap under the
/// terminal id once it is known.
pub fn logged_sink(
    on_output: Channel<InvokeResponseBody>,
    on_exit: Channel<ExitPayload>,
) -> (Arc<LogTap>, impl TerminalEventSink) {
    let tap = Arc::new(LogTap::default());
    let sink = tapped(Arc::clone(&tap), channel_sink(on_output, on_exit));
    (tap, sink)
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellChoice {
    id: String,
    name: String,
}

/// Shells installed on this machine, the system default first.
#[tauri::command]
pub async fn terminal_shells() -> Result<Vec<ShellChoice>, IpcError> {
    tauri::async_runtime::spawn_blocking(|| {
        terminal_pty::available_shells()
            .into_iter()
            .map(|p| ShellChoice {
                id: p.id,
                name: p.name,
            })
            .collect()
    })
    .await
    .map_err(|_| IpcError::internal("shell detection failed"))
}

/// Opens a local terminal. `shell` is an id from [`terminal_shells`]; the
/// frontend can pick a shell but never supply a program or arguments.
#[tauri::command]
pub async fn terminal_open(
    state: State<'_, AppState>,
    cols: u16,
    rows: u16,
    shell: Option<String>,
    on_output: Channel<InvokeResponseBody>,
    on_exit: Channel<ExitPayload>,
) -> Result<u64, IpcError> {
    let size = TerminalSize::new(cols, rows)?;
    let hub = state.hub.clone();
    let (tap, sink) = logged_sink(on_output, on_exit);
    let id = tauri::async_runtime::spawn_blocking(move || -> Result<u64, IpcError> {
        let shell = match shell {
            Some(id) => terminal_pty::shell_by_id(&id)?,
            None => terminal_pty::default_shell()?,
        };
        Ok(hub.open_local(&shell, size, sink)?)
    })
    .await
    .map_err(|_| IpcError::internal("terminal task failed"))??;
    state.logs.register(id, tap);
    Ok(id)
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
    state.tunnels.stop_for_terminal(id);
    state.logs.forget(id);
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
    let (tap, sink) = logged_sink(on_output, on_exit);
    let id = if state.hub.is_ssh(source)? {
        state.hub.open_ssh_sibling(source, size, sink).await?
    } else {
        let hub = state.hub.clone();
        tauri::async_runtime::spawn_blocking(move || -> Result<u64, IpcError> {
            let shell = terminal_pty::default_shell()?;
            Ok(hub.open_local(&shell, size, sink)?)
        })
        .await
        .map_err(|_| IpcError::internal("terminal task failed"))??
    };
    state.logs.register(id, tap);
    Ok(id)
}

/// Opens saved Telnet session `session_id` (unencrypted; the UI says so).
#[tauri::command]
pub async fn telnet_open(
    state: State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
    on_output: Channel<InvokeResponseBody>,
    on_exit: Channel<ExitPayload>,
) -> Result<u64, IpcError> {
    let size = TerminalSize::new(cols, rows)?;
    let rec = state
        .connector
        .sessions
        .get(&app_core::SessionId::parse(&session_id)?)?;
    if rec.input.protocol != app_core::SessionProtocol::Telnet {
        return Err(app_core::AppError::Invalid {
            field: "session",
            reason: "not a Telnet session".into(),
        }
        .into());
    }
    // Through a jump host: only the bastion-to-device leg is unencrypted.
    let via = match &rec.input.jump_via {
        Some(jump) => Some(state.connector.connect_tunnel(jump).await?),
        None => None,
    };
    let (tap, sink) = logged_sink(on_output, on_exit);
    let id = state
        .hub
        .open_telnet(
            rec.input.host.trim(),
            rec.input.port,
            via,
            state.connector.proxy().await?,
            size,
            sink,
        )
        .await?;
    state.logs.register(id, tap);
    Ok(id)
}

/// Starts saving terminal `id`'s output to a new plain-text file in the
/// logs folder (chosen by the backend). `label` only names the file.
#[tauri::command]
pub fn terminal_log_start(
    state: State<'_, AppState>,
    id: u64,
    label: String,
) -> Result<LogInfo, IpcError> {
    Ok(state.logs.start(id, &label)?)
}

/// Saves a snapshot of a terminal's visible text and scrollback (sent by
/// the UI) to a new file in the logs folder; returns the path.
#[tauri::command]
pub fn terminal_save_output(
    state: State<'_, AppState>,
    label: String,
    text: String,
) -> Result<String, IpcError> {
    Ok(state
        .logs
        .save_text(&label, &text)?
        .to_string_lossy()
        .into_owned())
}

#[tauri::command]
pub fn terminal_log_stop(state: State<'_, AppState>, id: u64) -> Result<Option<LogInfo>, IpcError> {
    Ok(state.logs.stop(id)?)
}

#[tauri::command]
pub fn terminal_log_status(
    state: State<'_, AppState>,
    id: u64,
) -> Result<Option<LogInfo>, IpcError> {
    Ok(state.logs.status(id)?)
}

#[tauri::command]
pub async fn tunnel_start(
    state: State<'_, AppState>,
    terminal_id: u64,
    spec: app_core::TunnelSpec,
) -> Result<app_core::TunnelInfo, IpcError> {
    Ok(state.tunnels.start(&state.hub, terminal_id, spec).await?)
}

#[tauri::command]
pub fn tunnel_list(
    state: State<'_, AppState>,
    terminal_id: Option<u64>,
) -> Vec<app_core::TunnelInfo> {
    state.tunnels.list(terminal_id)
}

#[tauri::command]
pub fn tunnel_stop(state: State<'_, AppState>, id: u64) -> bool {
    state.tunnels.stop(id)
}

#[tauri::command]
pub fn serial_ports() -> Result<Vec<serial_console::PortInfo>, IpcError> {
    serial_console::list_ports().map_err(|e| app_core::AppError::from(e).into())
}

#[tauri::command]
pub async fn serial_open(
    state: State<'_, AppState>,
    config: serial_console::SerialConfig,
    on_output: Channel<InvokeResponseBody>,
    on_exit: Channel<ExitPayload>,
) -> Result<u64, IpcError> {
    let hub = state.hub.clone();
    let (tap, sink) = logged_sink(on_output, on_exit);
    let id = tauri::async_runtime::spawn_blocking(move || -> Result<u64, IpcError> {
        Ok(hub.open_serial(&config, sink)?)
    })
    .await
    .map_err(|_| IpcError::internal("serial task failed"))??;
    state.logs.register(id, tap);
    Ok(id)
}
