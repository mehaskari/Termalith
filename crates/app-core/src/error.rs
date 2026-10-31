use credential_store::SecretError;
use protocol_ssh::SshError;
use terminal_pty::PtyError;

/// Errors surfaced to the UI. Never contain secrets.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("invalid {field}: {reason}")]
    Invalid { field: &'static str, reason: String },

    #[error("not found")]
    NotFound,

    /// The session uses password auth and no saved password exists.
    #[error("a password is required")]
    PasswordRequired,

    #[error(transparent)]
    Ssh(#[from] SshError),

    #[error(transparent)]
    Secret(#[from] SecretError),

    #[error(transparent)]
    Pty(#[from] PtyError),

    #[error("database error: {0}")]
    Db(String),
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Db(e.to_string())
    }
}

impl AppError {
    pub(crate) fn invalid(field: &'static str, reason: impl Into<String>) -> Self {
        AppError::Invalid {
            field,
            reason: reason.into(),
        }
    }
}
