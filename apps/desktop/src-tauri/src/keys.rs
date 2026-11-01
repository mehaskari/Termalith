//! SSH key IPC: list and generate key pairs in `~/.ssh`, copy a public key,
//! and install one on a connected server. Keys are referred to by name; the
//! backend resolves the path, so the frontend cannot read arbitrary files.

use std::path::PathBuf;

use app_core::{AppError, KeyInstall, install_public_key};
use protocol_ssh::KeyInfo;
use serde::Serialize;
use tauri::State;
use zeroize::Zeroizing;

use crate::error::IpcError;
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyView {
    name: String,
    algorithm: String,
    fingerprint: String,
    comment: String,
    encrypted: Option<bool>,
    has_private: bool,
}

impl From<KeyInfo> for KeyView {
    fn from(k: KeyInfo) -> Self {
        Self {
            name: k.name,
            algorithm: k.algorithm,
            fingerprint: k.fingerprint,
            comment: k.comment,
            encrypted: k.encrypted,
            has_private: k.private_path.is_some(),
        }
    }
}

fn ssh_dir(state: &AppState) -> Result<PathBuf, IpcError> {
    state
        .home_dir
        .as_ref()
        .map(|h| h.join(".ssh"))
        .ok_or_else(|| IpcError::internal("home directory is unknown"))
}

/// The public key line of key `name` (must be one that `keys_list` shows).
fn public_line(state: &AppState, name: &str) -> Result<String, IpcError> {
    let key = protocol_ssh::list_keys(&ssh_dir(state)?)
        .into_iter()
        .find(|k| k.name == name)
        .ok_or(AppError::NotFound)?;
    Ok(protocol_ssh::read_public_key(&key.public_path)?.1)
}

#[tauri::command]
pub async fn keys_list(state: State<'_, AppState>) -> Result<Vec<KeyView>, IpcError> {
    let dir = ssh_dir(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        protocol_ssh::list_keys(&dir)
            .into_iter()
            .map(KeyView::from)
            .collect()
    })
    .await
    .map_err(|_| IpcError::internal("key listing failed"))
}

/// Generates an Ed25519 key pair `~/.ssh/<name>`. The passphrase is only
/// used to encrypt the file and is zeroized afterwards.
#[tauri::command]
pub async fn key_generate(
    state: State<'_, AppState>,
    name: String,
    comment: String,
    passphrase: Option<String>,
) -> Result<KeyView, IpcError> {
    let dir = ssh_dir(&state)?;
    let passphrase = passphrase.map(Zeroizing::new);
    tauri::async_runtime::spawn_blocking(move || {
        protocol_ssh::generate_ed25519(
            &dir,
            &name,
            &comment,
            passphrase.as_deref().map(String::as_str),
        )
        .map(KeyView::from)
        .map_err(IpcError::from)
    })
    .await
    .map_err(|_| IpcError::internal("key generation failed"))?
}

/// The public key text, for copying.
#[tauri::command]
pub fn key_public(state: State<'_, AppState>, name: String) -> Result<String, IpcError> {
    public_line(&state, &name)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    added: bool,
    path: String,
}

/// Adds public key `name` to `~/.ssh/authorized_keys` on the server behind
/// SSH terminal `terminal_id`.
#[tauri::command]
pub async fn key_install(
    state: State<'_, AppState>,
    terminal_id: u64,
    name: String,
) -> Result<Installed, IpcError> {
    let line = public_line(&state, &name)?;
    let sftp = state.hub.sftp(terminal_id).await?;
    let home = sftp.canonicalize(".").await?;
    Ok(match install_public_key(&sftp, &home, &line).await? {
        KeyInstall::Added { path } => Installed { added: true, path },
        KeyInstall::AlreadyPresent { path } => Installed { added: false, path },
    })
}
