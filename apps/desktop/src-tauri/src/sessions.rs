//! Saved-session IPC. Records never carry secrets to the frontend; only a
//! `hasSavedSecret` flag.

use app_core::{ImportCandidate, ImportReport, SessionId, SessionInput, SessionRecord};
use serde::Serialize;
use tauri::State;
use zeroize::Zeroizing;

use crate::error::IpcError;
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    /// Operating system and CPU architecture, e.g. "macos aarch64".
    platform: String,
    secrets_available: bool,
    agent_available: bool,
    /// Running from a portable folder: passwords are saved only with a
    /// master password.
    portable: bool,
}

/// Compares this version with the newest published release on GitHub.
/// Never downloads anything.
#[tauri::command]
pub async fn update_check() -> Result<app_core::UpdateInfo, IpcError> {
    tauri::async_runtime::spawn_blocking(|| app_core::check_for_updates(env!("CARGO_PKG_VERSION")))
        .await
        .map_err(|e| IpcError::internal(e.to_string()))?
        .map_err(Into::into)
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        secrets_available: state.secrets_available,
        portable: state.portable,
        agent_available: state.agent_available,
    }
}

fn read_ssh_config(state: &AppState) -> Result<Vec<ImportCandidate>, IpcError> {
    let Some(home) = state.home_dir.as_ref() else {
        return Ok(Vec::new());
    };
    let text = match std::fs::read_to_string(home.join(".ssh").join("config")) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(IpcError::internal(format!(
                "cannot read ~/.ssh/config: {e}"
            )));
        }
    };
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    Ok(app_core::parse_ssh_config(&text, home, &user))
}

/// Sessions that would be imported from `~/.ssh/config` (nothing is saved).
#[tauri::command]
pub fn ssh_config_preview(state: State<'_, AppState>) -> Result<Vec<ImportCandidate>, IpcError> {
    read_ssh_config(&state)
}

/// Imports the selected aliases; existing session names are skipped.
#[tauri::command]
pub fn ssh_config_import(
    state: State<'_, AppState>,
    aliases: Vec<String>,
) -> Result<ImportReport, IpcError> {
    let candidates = read_ssh_config(&state)?;
    Ok(app_core::import_candidates(
        &state.connector.sessions,
        &candidates,
        &aliases,
    )?)
}

#[tauri::command]
pub fn sessions_list(state: State<'_, AppState>) -> Result<Vec<SessionRecord>, IpcError> {
    Ok(state.connector.sessions.list()?)
}

#[tauri::command]
pub fn session_create(
    state: State<'_, AppState>,
    input: SessionInput,
) -> Result<SessionRecord, IpcError> {
    Ok(state.connector.sessions.create(&input)?)
}

#[tauri::command]
pub async fn session_update(
    state: State<'_, AppState>,
    id: String,
    input: SessionInput,
) -> Result<SessionRecord, IpcError> {
    let id = SessionId::parse(&id)?;
    let (record, stale) = state.connector.sessions.update(&id, &input)?;
    if stale.is_some() {
        // Auth method changed: the old secret no longer applies.
        state.connector.forget_secret(&id).await.ok();
    }
    Ok(record)
}

#[tauri::command]
pub async fn session_delete(state: State<'_, AppState>, id: String) -> Result<(), IpcError> {
    let id = SessionId::parse(&id)?;
    Ok(state.connector.delete_session(&id).await?)
}

/// Shared (team) session files.
#[tauri::command]
pub fn shared_sources_list(
    state: State<'_, AppState>,
) -> Result<Vec<app_core::SharedSource>, IpcError> {
    Ok(state.connector.sessions.shared_sources()?)
}

/// Adds an https:// address as a shared session source.
#[tauri::command]
pub fn shared_source_add_url(
    state: State<'_, AppState>,
    label: String,
    url: String,
) -> Result<String, IpcError> {
    app_core::check_shared_url(&url)?;
    Ok(state.connector.sessions.add_shared_source(
        &label,
        app_core::SharedLocation::Https(url.trim().to_owned()),
    )?)
}

/// Picks a session file in the system dialog and adds it as a shared
/// source. `None` when the dialog was cancelled.
#[tauri::command]
pub async fn shared_source_add_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    label: String,
) -> Result<Option<String>, IpcError> {
    use tauri::Manager;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let window = app.get_webview_window("main");
    app.run_on_main_thread(move || {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Shared session file")
            .add_filter("Termalith sessions", &["json"]);
        if let Some(window) = window.as_ref() {
            dialog = dialog.set_parent(window);
        }
        let _ = tx.send(dialog.pick_file());
    })
    .map_err(|e| IpcError::coded("dialog", format!("could not show the file dialog: {e}")))?;
    let Ok(Some(path)) = rx.await else {
        return Ok(None);
    };
    Ok(Some(state.connector.sessions.add_shared_source(
        &label,
        app_core::SharedLocation::File(path),
    )?))
}

#[tauri::command]
pub async fn shared_source_remove(state: State<'_, AppState>, id: String) -> Result<(), IpcError> {
    Ok(state.connector.remove_shared_source(&id).await?)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedSyncItem {
    label: String,
    sessions: Option<usize>,
    error: Option<String>,
}

/// Reads every shared source again and updates its sessions.
#[tauri::command]
pub async fn shared_sync(state: State<'_, AppState>) -> Result<Vec<SharedSyncItem>, IpcError> {
    let store = std::sync::Arc::clone(&state.connector.sessions);
    let agent = format!("Termalith/{}", env!("CARGO_PKG_VERSION"));
    let report = tauri::async_runtime::spawn_blocking(move || store.sync_shared(&agent))
        .await
        .map_err(|e| IpcError::coded("internal", e.to_string()))?;
    Ok(report
        .into_iter()
        .map(|(label, r)| SharedSyncItem {
            label,
            sessions: r.as_ref().ok().copied(),
            error: r.err(),
        })
        .collect())
}

#[tauri::command]
pub async fn session_forget_secret(state: State<'_, AppState>, id: String) -> Result<(), IpcError> {
    let id = SessionId::parse(&id)?;
    Ok(state.connector.forget_secret(&id).await?)
}

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Result<app_core::Settings, IpcError> {
    Ok(state.connector.sessions.settings()?)
}

/// Whether a proxy password is saved (the password itself never goes to
/// the UI).
#[tauri::command]
pub fn proxy_password_status(state: State<'_, AppState>) -> Result<bool, IpcError> {
    Ok(state.connector.has_proxy_password()?)
}

/// Saves the proxy password in the OS credential store; `None` removes it.
#[tauri::command]
pub async fn proxy_password_set(
    state: State<'_, AppState>,
    password: Option<String>,
) -> Result<(), IpcError> {
    let password = password.map(zeroize::Zeroizing::new);
    if password.as_ref().is_some_and(|p| p.len() > 255) {
        return Err(IpcError::coded(
            "invalid",
            "the proxy password is longer than 255 bytes",
        ));
    }
    if password.is_some() && !state.secrets_available {
        return Err(IpcError::coded(
            "secret_store",
            "no credential store is available, so the proxy password cannot be saved",
        ));
    }
    Ok(state.connector.set_proxy_password(password).await?)
}

#[tauri::command]
pub fn settings_save(
    state: State<'_, AppState>,
    settings: app_core::Settings,
) -> Result<(), IpcError> {
    Ok(state.connector.sessions.save_settings(&settings)?)
}

#[tauri::command]
pub fn session_set_favorite(
    state: State<'_, AppState>,
    id: String,
    favorite: bool,
) -> Result<(), IpcError> {
    let id = SessionId::parse(&id)?;
    Ok(state.connector.sessions.set_favorite(&id, favorite)?)
}

/// Folders stored on their own (empty ones included).
#[tauri::command]
pub fn folders_list(state: State<'_, AppState>) -> Result<Vec<String>, IpcError> {
    Ok(state.connector.sessions.folders()?)
}

#[tauri::command]
pub fn folder_create(state: State<'_, AppState>, path: String) -> Result<String, IpcError> {
    Ok(state.connector.sessions.create_folder(&path)?)
}

/// Folders that have an icon.
#[tauri::command]
pub fn folder_icons_list(
    state: State<'_, AppState>,
) -> Result<Vec<(String, app_core::Icon)>, IpcError> {
    Ok(state.connector.sessions.folder_icons()?)
}

/// Sets or clears a folder's icon.
#[tauri::command]
pub fn folder_icon_set(
    state: State<'_, AppState>,
    path: String,
    icon: Option<app_core::Icon>,
) -> Result<(), IpcError> {
    Ok(state
        .connector
        .sessions
        .set_folder_icon(&path, icon.as_ref())?)
}

/// Renames or moves a folder with everything in it.
#[tauri::command]
pub fn folder_rename(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> Result<String, IpcError> {
    Ok(state.connector.sessions.rename_folder(&from, &to)?)
}

/// Deletes an empty folder (and its empty subfolders).
#[tauri::command]
pub fn folder_delete(state: State<'_, AppState>, path: String) -> Result<(), IpcError> {
    Ok(state.connector.sessions.delete_folder(&path)?)
}

#[tauri::command]
pub fn session_move(
    state: State<'_, AppState>,
    id: String,
    folder: Option<String>,
) -> Result<(), IpcError> {
    let id = SessionId::parse(&id)?;
    Ok(state
        .connector
        .sessions
        .move_session(&id, folder.as_deref())?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    /// The exported JSON (also offered for copying).
    json: String,
    /// Where a copy was saved, if writing it succeeded.
    saved_to: Option<String>,
}

/// Exports all sessions (no secrets) and saves a copy in Downloads.
#[tauri::command]
pub fn sessions_export(state: State<'_, AppState>) -> Result<ExportResult, IpcError> {
    let json = app_core::export_sessions(&state.connector.sessions)?;
    let dest = app_core::unique_destination(&state.downloads_dir, "termalith-sessions.json");
    let saved_to = std::fs::write(&dest, &json)
        .ok()
        .map(|()| dest.to_string_lossy().into_owned());
    Ok(ExportResult { json, saved_to })
}

/// Largest pasted import accepted (a few thousand sessions fit easily).
const MAX_IMPORT_BYTES: usize = 5 * 1024 * 1024;

/// Imports sessions from pasted export JSON.
#[tauri::command]
pub fn sessions_import(state: State<'_, AppState>, json: String) -> Result<ImportReport, IpcError> {
    if json.len() > MAX_IMPORT_BYTES {
        return Err(IpcError::coded(
            "invalid",
            "the import is larger than 5 MiB; split it into smaller files",
        ));
    }
    // The same box accepts a Termalith export or a MobaXterm .mxtsessions file.
    if app_core::is_mobaxterm(&json) {
        return Ok(app_core::import_mobaxterm(
            &state.connector.sessions,
            &json,
        )?);
    }
    Ok(app_core::import_sessions(&state.connector.sessions, &json)?)
}

#[tauri::command]
pub fn snippets_list(state: State<'_, AppState>) -> Result<Vec<app_core::Snippet>, IpcError> {
    Ok(state.connector.sessions.snippets()?)
}

#[tauri::command]
pub fn snippet_create(
    state: State<'_, AppState>,
    input: app_core::SnippetInput,
) -> Result<app_core::Snippet, IpcError> {
    Ok(state.connector.sessions.create_snippet(&input)?)
}

#[tauri::command]
pub fn snippet_update(
    state: State<'_, AppState>,
    id: String,
    input: app_core::SnippetInput,
) -> Result<app_core::Snippet, IpcError> {
    Ok(state.connector.sessions.update_snippet(&id, &input)?)
}

#[tauri::command]
pub fn macros_list(state: State<'_, AppState>) -> Result<Vec<app_core::Macro>, IpcError> {
    Ok(state.connector.sessions.macros()?)
}

#[tauri::command]
pub fn macro_create(
    state: State<'_, AppState>,
    input: app_core::MacroInput,
) -> Result<app_core::Macro, IpcError> {
    Ok(state.connector.sessions.create_macro(&input)?)
}

#[tauri::command]
pub fn macro_delete(state: State<'_, AppState>, id: String) -> Result<(), IpcError> {
    Ok(state.connector.sessions.delete_macro(&id)?)
}

#[tauri::command]
pub fn snippet_delete(state: State<'_, AppState>, id: String) -> Result<(), IpcError> {
    Ok(state.connector.sessions.delete_snippet(&id)?)
}

// ---------------------------------------------------------------- master password

#[tauri::command]
pub fn master_status(state: State<'_, AppState>) -> app_core::MasterStatus {
    state.master.status()
}

#[tauri::command]
pub async fn master_enable(state: State<'_, AppState>, password: String) -> Result<(), IpcError> {
    Ok(state.master.enable(Zeroizing::new(password)).await?)
}

#[tauri::command]
pub async fn master_unlock(state: State<'_, AppState>, password: String) -> Result<(), IpcError> {
    Ok(state.master.unlock(Zeroizing::new(password)).await?)
}

#[tauri::command]
pub fn master_lock(state: State<'_, AppState>) {
    state.master.lock();
}

#[tauri::command]
pub async fn master_change(
    state: State<'_, AppState>,
    old: String,
    new: String,
) -> Result<(), IpcError> {
    Ok(state
        .master
        .change(Zeroizing::new(old), Zeroizing::new(new))
        .await?)
}

#[tauri::command]
pub async fn master_disable(state: State<'_, AppState>, password: String) -> Result<(), IpcError> {
    Ok(state.master.disable(Zeroizing::new(password)).await?)
}

/// Forgotten master password: deletes every saved password and the master
/// password itself.
#[tauri::command]
pub fn master_reset(state: State<'_, AppState>) -> Result<(), IpcError> {
    Ok(state.master.reset()?)
}
