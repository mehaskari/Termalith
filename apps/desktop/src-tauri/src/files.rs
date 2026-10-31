//! SFTP file-browser IPC. Downloads always land in the backend-chosen
//! downloads directory; uploads take a token for a file the user dropped.

use app_core::TransferEvent;
use protocol_ssh::{RemoteEntry, RemoteKind};
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;

use crate::error::IpcError;
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryDto {
    name: String,
    kind: &'static str,
    size: Option<u64>,
    modified: Option<u32>,
    permissions: Option<u32>,
}

impl From<RemoteEntry> for EntryDto {
    fn from(e: RemoteEntry) -> Self {
        Self {
            name: e.name,
            kind: match e.kind {
                RemoteKind::Dir => "dir",
                RemoteKind::File => "file",
                RemoteKind::Symlink => "symlink",
                RemoteKind::Other => "other",
            },
            size: e.size,
            modified: e.modified,
            permissions: e.permissions,
        }
    }
}

#[tauri::command]
pub async fn sftp_home(state: State<'_, AppState>, terminal_id: u64) -> Result<String, IpcError> {
    let sftp = state.hub.sftp(terminal_id).await?;
    Ok(sftp.canonicalize(".").await?)
}

#[tauri::command]
pub async fn sftp_list(
    state: State<'_, AppState>,
    terminal_id: u64,
    path: String,
) -> Result<Vec<EntryDto>, IpcError> {
    let sftp = state.hub.sftp(terminal_id).await?;
    Ok(sftp
        .list(&path)
        .await?
        .into_iter()
        .map(EntryDto::from)
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferStarted {
    id: u64,
    /// Where downloads are saved, for display.
    downloads_dir: String,
}

#[tauri::command]
pub async fn sftp_download(
    state: State<'_, AppState>,
    terminal_id: u64,
    dir: String,
    name: String,
    size: Option<u64>,
    on_event: Channel<TransferEvent>,
) -> Result<TransferStarted, IpcError> {
    let sftp = state.hub.sftp(terminal_id).await?;
    let id = state.transfers.start_download(
        sftp,
        &dir,
        &name,
        size,
        state.downloads_dir.clone(),
        move |e| {
            let _ = on_event.send(e);
        },
    )?;
    Ok(TransferStarted {
        id,
        downloads_dir: state.downloads_dir.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn sftp_upload(
    state: State<'_, AppState>,
    terminal_id: u64,
    token: u64,
    dir: String,
    on_event: Channel<TransferEvent>,
) -> Result<u64, IpcError> {
    let sftp = state.hub.sftp(terminal_id).await?;
    let local = state.dropped.take(token)?;
    Ok(state.transfers.start_upload(sftp, local, &dir, move |e| {
        let _ = on_event.send(e);
    })?)
}

#[tauri::command]
pub fn transfer_cancel(state: State<'_, AppState>, id: u64) -> bool {
    state.transfers.cancel(id)
}
