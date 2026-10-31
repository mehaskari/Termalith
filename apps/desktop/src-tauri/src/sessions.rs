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
