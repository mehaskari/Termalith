//! Saved-session IPC. Records never carry secrets to the frontend; only a
//! `hasSavedSecret` flag.

use app_core::{ImportCandidate, ImportReport, SessionId, SessionInput, SessionRecord};
use serde::Serialize;
use tauri::State;

use crate::error::IpcError;
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    secrets_available: bool,
    agent_available: bool,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        secrets_available: state.secrets_available,
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

#[tauri::command]
pub async fn session_forget_secret(state: State<'_, AppState>, id: String) -> Result<(), IpcError> {
    let id = SessionId::parse(&id)?;
    Ok(state.connector.forget_secret(&id).await?)
}

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Result<app_core::Settings, IpcError> {
    Ok(state.connector.sessions.settings()?)
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

/// Imports sessions from pasted export JSON.
#[tauri::command]
pub fn sessions_import(state: State<'_, AppState>, json: String) -> Result<ImportReport, IpcError> {
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
pub fn snippet_delete(state: State<'_, AppState>, id: String) -> Result<(), IpcError> {
    Ok(state.connector.sessions.delete_snippet(&id)?)
}
