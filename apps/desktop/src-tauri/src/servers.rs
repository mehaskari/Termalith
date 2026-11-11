//! Built-in TFTP and HTTP servers (ADR-036). The UI never names a folder:
//! it shares the default folder or one the user dropped onto the window
//! (by token).

use app_core::{ServerInfo, ServerSpec, ServerState};
use local_servers::ServerKind;
use serde::Deserialize;
use tauri::State;

use crate::error::IpcError;
use crate::state::AppState;

/// The folder shared when the user does not drop one: "Termalith Share" in
/// Downloads, created on first use.
fn default_folder(state: &AppState) -> Result<std::path::PathBuf, IpcError> {
    let dir = state.downloads_dir.join("Termalith Share");
    std::fs::create_dir_all(&dir)
        .map_err(|e| IpcError::coded("folder", format!("cannot create {}: {e}", dir.display())))?;
    Ok(dir)
}

/// Where the default shared folder is (created if missing), for display.
#[tauri::command]
pub fn server_default_folder(state: State<'_, AppState>) -> Result<String, IpcError> {
    Ok(default_folder(&state)?.display().to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStart {
    kind: ServerKind,
    /// A folder dropped onto the window; `None` shares the default folder.
    folder_token: Option<u64>,
    lan: bool,
    port: Option<u16>,
    allow_uploads: bool,
}

#[tauri::command]
pub async fn server_start(
    state: State<'_, AppState>,
    request: ServerStart,
) -> Result<ServerInfo, IpcError> {
    let root = match request.folder_token {
        Some(token) => {
            let path = state.dropped.take(token)?;
            if !path.is_dir() {
                return Err(IpcError::coded("invalid", "only a folder can be shared"));
            }
            path
        }
        None => default_folder(&state)?,
    };
    Ok(state
        .servers
        .start(ServerSpec {
            kind: request.kind,
            root,
            lan: request.lan,
            port: request.port,
            allow_uploads: request.allow_uploads,
        })
        .await?)
}

#[tauri::command]
pub fn server_list(state: State<'_, AppState>) -> Vec<ServerState> {
    state.servers.list()
}

#[tauri::command]
pub async fn server_stop(state: State<'_, AppState>, id: u64) -> Result<bool, IpcError> {
    Ok(state.servers.stop(id).await)
}

/// Opens the default shared folder in the file manager (fixed path, no
/// input from the UI).
#[tauri::command]
pub fn server_open_folder(state: State<'_, AppState>) -> Result<(), IpcError> {
    let dir = default_folder(&state)?;
    let program = if cfg!(target_os = "macos") {
        "/usr/bin/open"
    } else if cfg!(windows) {
        "explorer.exe"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| IpcError::coded("open_failed", format!("could not open the folder: {e}")))?;
    Ok(())
}
