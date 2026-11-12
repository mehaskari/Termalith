//! Built-in TFTP, HTTP and FTP servers (ADR-036). The UI never names a
//! folder: the backend keeps the folder to share, which the user picks in
//! the system folder dialog or drops onto the window (by token); without a
//! choice it is the default "Termalith Share" folder.

use std::path::PathBuf;

use app_core::{ServerInfo, ServerSpec, ServerState};
use local_servers::ServerKind;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::error::IpcError;
use crate::state::AppState;

/// The folder shared when the user does not choose one: "Termalith Share"
/// in Downloads, created on first use.
fn default_folder(state: &AppState) -> Result<PathBuf, IpcError> {
    let dir = state.downloads_dir.join("Termalith Share");
    std::fs::create_dir_all(&dir)
        .map_err(|e| IpcError::coded("folder", format!("cannot create {}: {e}", dir.display())))?;
    Ok(dir)
}

/// The folder the next server shares, as the panel shows it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedFolder {
    path: String,
    /// True for the default "Termalith Share" folder.
    is_default: bool,
}

fn chosen(state: &AppState) -> Option<PathBuf> {
    state
        .server_folder
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn set_chosen(state: &AppState, folder: Option<PathBuf>) {
    *state
        .server_folder
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = folder;
}

/// The folder to share now: the chosen one, or the default. A chosen folder
/// that was removed falls back to the default, so what is shown is what is
/// shared.
fn current(state: &AppState) -> Result<(PathBuf, bool), IpcError> {
    match chosen(state) {
        Some(dir) if dir.is_dir() => Ok((dir, false)),
        Some(_) => {
            set_chosen(state, None);
            Ok((default_folder(state)?, true))
        }
        None => Ok((default_folder(state)?, true)),
    }
}

fn describe(state: &AppState) -> Result<SharedFolder, IpcError> {
    let (dir, is_default) = current(state)?;
    Ok(SharedFolder {
        path: dir.display().to_string(),
        is_default,
    })
}

/// The folder the next server will share.
#[tauri::command]
pub fn server_folder(state: State<'_, AppState>) -> Result<SharedFolder, IpcError> {
    describe(&state)
}

/// Shows the system folder dialog; a picked folder becomes the one to share.
/// Cancelling keeps the current folder.
#[tauri::command]
pub async fn server_pick_folder(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SharedFolder, IpcError> {
    let (start, _) = current(&state)?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let window = app.get_webview_window("main");
    // Native dialogs belong on the main thread (AppKit, GTK).
    app.run_on_main_thread(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Folder to share")
            .set_directory(&start);
        if let Some(window) = window.as_ref() {
            dialog = dialog.set_parent(window);
        }
        let _ = tx.send(dialog.pick_folder());
    })
    .map_err(|e| IpcError::coded("dialog", format!("could not show the folder dialog: {e}")))?;
    if let Ok(Some(dir)) = rx.await {
        if !dir.is_dir() {
            return Err(IpcError::coded("invalid", "only a folder can be shared"));
        }
        set_chosen(&state, Some(dir));
    }
    describe(&state)
}

/// Makes a folder dropped onto the window the one to share.
#[tauri::command]
pub fn server_use_dropped(
    state: State<'_, AppState>,
    token: u64,
) -> Result<SharedFolder, IpcError> {
    let path = state.dropped.take(token)?;
    if !path.is_dir() {
        return Err(IpcError::coded("invalid", "only a folder can be shared"));
    }
    set_chosen(&state, Some(path));
    describe(&state)
}

/// Goes back to the default "Termalith Share" folder.
#[tauri::command]
pub fn server_reset_folder(state: State<'_, AppState>) -> Result<SharedFolder, IpcError> {
    set_chosen(&state, None);
    describe(&state)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStart {
    kind: ServerKind,
    lan: bool,
    port: Option<u16>,
    allow_uploads: bool,
    /// FTP: a chosen user name; empty or `None` is "termalith".
    ftp_user: Option<String>,
    /// FTP: a chosen password; empty or `None` makes a random one.
    ftp_password: Option<String>,
}

/// Starts a server for the folder [`server_folder`] reports.
#[tauri::command]
pub async fn server_start(
    state: State<'_, AppState>,
    request: ServerStart,
) -> Result<ServerInfo, IpcError> {
    let (root, _) = current(&state)?;
    let ftp_login = if request.kind == ServerKind::Ftp {
        let user = request
            .ftp_user
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| local_servers::FTP_USER.to_owned());
        let password = request
            .ftp_password
            .filter(|p| !p.is_empty())
            .unwrap_or_else(local_servers::ftp::random_password);
        Some((user, password))
    } else {
        None
    };
    Ok(state
        .servers
        .start(ServerSpec {
            kind: request.kind,
            root,
            lan: request.lan,
            port: request.port,
            allow_uploads: request.allow_uploads,
            ftp_login,
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

/// Opens a folder in the file manager: a running server's folder by id, or
/// the folder the next server shares. Paths come from the backend only.
#[tauri::command]
pub fn server_open_folder(state: State<'_, AppState>, id: Option<u64>) -> Result<(), IpcError> {
    let dir = match id {
        Some(id) => state
            .servers
            .list()
            .into_iter()
            .find(|s| s.info.id == id)
            .map(|s| PathBuf::from(s.info.folder))
            .ok_or_else(|| IpcError::coded("not_found", "that server is not running"))?,
        None => current(&state)?.0,
    };
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
