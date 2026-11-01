//! SFTP file-browser IPC. Downloads always land in the backend-chosen
//! downloads directory; uploads take a token for a file the user dropped.

use app_core::{MAX_EDIT_BYTES, TransferEvent, decode_text, remote_join, safe_file_name};
use protocol_ssh::{RemoteEntry, RemoteKind};
use serde::{Deserialize, Serialize};
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

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct EditVersion {
    size: Option<u64>,
    modified: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteText {
    path: String,
    content: String,
    /// Pass back when saving; detects changes made by someone else.
    version: EditVersion,
}

/// Opens `dir/name` for the built-in editor (UTF-8 text up to 2 MiB).
#[tauri::command]
pub async fn sftp_read_text(
    state: State<'_, AppState>,
    terminal_id: u64,
    dir: String,
    name: String,
) -> Result<RemoteText, IpcError> {
    let path = remote_join(&dir, safe_file_name(&name)?);
    let sftp = state.hub.sftp(terminal_id).await?;
    let (bytes, stat) = sftp.read_file(&path, MAX_EDIT_BYTES).await?;
    Ok(RemoteText {
        content: decode_text(bytes)?,
        path,
        version: EditVersion {
            size: stat.size,
            modified: stat.modified,
        },
    })
}

/// Saves editor content over `dir/name`, refusing if the file changed
/// since `version` was read. Returns the new version.
#[tauri::command]
pub async fn sftp_save_text(
    state: State<'_, AppState>,
    terminal_id: u64,
    dir: String,
    name: String,
    content: String,
    version: EditVersion,
) -> Result<EditVersion, IpcError> {
    if content.len() as u64 > MAX_EDIT_BYTES {
        return Err(protocol_ssh::SshError::TooLarge {
            limit: MAX_EDIT_BYTES,
        }
        .into());
    }
    let path = remote_join(&dir, safe_file_name(&name)?);
    let sftp = state.hub.sftp(terminal_id).await?;
    let expected = protocol_ssh::RemoteStat {
        size: version.size,
        modified: version.modified,
        permissions: None,
    };
    let stat = sftp
        .replace_file(&path, content.as_bytes(), &expected)
        .await?;
    Ok(EditVersion {
        size: stat.size,
        modified: stat.modified,
    })
}

/// Creates folder `name` in `dir`.
#[tauri::command]
pub async fn sftp_mkdir(
    state: State<'_, AppState>,
    terminal_id: u64,
    dir: String,
    name: String,
) -> Result<(), IpcError> {
    let path = remote_join(&dir, safe_file_name(&name)?);
    Ok(state.hub.sftp(terminal_id).await?.create_dir(&path).await?)
}

/// Renames `from` to `to` inside `dir` (no moves between folders, never
/// replaces an existing entry).
#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, AppState>,
    terminal_id: u64,
    dir: String,
    from: String,
    to: String,
) -> Result<(), IpcError> {
    let old = remote_join(&dir, safe_file_name(&from)?);
    let new = remote_join(&dir, safe_file_name(&to)?);
    Ok(state
        .hub
        .sftp(terminal_id)
        .await?
        .rename(&old, &new)
        .await?)
}

/// Deletes a file, symlink or empty folder `name` in `dir`.
#[tauri::command]
pub async fn sftp_delete(
    state: State<'_, AppState>,
    terminal_id: u64,
    dir: String,
    name: String,
) -> Result<(), IpcError> {
    let path = remote_join(&dir, safe_file_name(&name)?);
    Ok(state.hub.sftp(terminal_id).await?.delete(&path).await?)
}

/// Runs a failed or cancelled transfer again; returns the new id.
#[tauri::command]
pub fn transfer_retry(
    state: State<'_, AppState>,
    id: u64,
    on_event: Channel<TransferEvent>,
) -> Result<u64, IpcError> {
    Ok(state.transfers.retry(id, move |e| {
        let _ = on_event.send(e);
    })?)
}

#[tauri::command]
pub fn transfer_cancel(state: State<'_, AppState>, id: u64) -> bool {
    state.transfers.cancel(id)
}
