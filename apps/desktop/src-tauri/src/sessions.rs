//! Saved-session IPC. Records never carry secrets to the frontend; only a
//! `hasSavedSecret` flag.

use app_core::{SessionId, SessionInput, SessionRecord};
use serde::Serialize;
use tauri::State;

use crate::error::IpcError;
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    secrets_available: bool,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        secrets_available: state.secrets_available,
    }
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
