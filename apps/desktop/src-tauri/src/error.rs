use serde::Serialize;
use terminal_pty::PtyError;

/// Error shape returned over IPC: a stable machine-readable code plus a
/// message that is safe to display (never contains terminal I/O).
#[derive(Debug, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct IpcError {
    code: &'static str,
    message: String,
}

impl IpcError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "internal",
            message: message.into(),
        }
    }
}

impl From<PtyError> for IpcError {
    fn from(e: PtyError) -> Self {
        let code = match &e {
            PtyError::InvalidSize { .. } => "invalid_size",
            PtyError::InputTooLarge { .. } => "input_too_large",
            PtyError::NoShell => "no_shell",
            PtyError::UnknownTerminal(_) => "unknown_terminal",
            PtyError::Open(_) | PtyError::Spawn(_) => "spawn_failed",
            PtyError::Io(_) => "io",
        };
        Self {
            code,
            message: e.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_code_and_message() {
        let e = IpcError::from(PtyError::InvalidSize { cols: 0, rows: 1 });
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["code"], "invalid_size");
        assert_eq!(json["message"], "terminal size 0x1 is out of range");
    }
}
