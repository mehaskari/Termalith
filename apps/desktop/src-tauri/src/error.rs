use app_core::AppError;
use protocol_ssh::SshError;
use serde::Serialize;
use terminal_pty::PtyError;

/// Error shape returned over IPC: a stable machine-readable code plus a
/// message that is safe to display (never contains secrets or terminal I/O).
#[derive(Debug, Serialize, thiserror::Error)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    code: &'static str,
    message: String,
    /// Extra structured data for some codes (e.g. fingerprints).
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<serde_json::Value>,
}

impl IpcError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message)
    }

    pub fn coded(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(code, message)
    }

    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail: None,
        }
        .local_network(cfg!(target_os = "macos"))
    }

    /// macOS answers "No route to host" (EHOSTUNREACH, os error 65) at once
    /// when Local Network privacy blocks the app. Such errors get their own
    /// code and a message that says where to fix it; the UI also shows a
    /// dialog. A host that is really unreachable gives the same error, so
    /// the wording says "probably".
    fn local_network(mut self, macos: bool) -> Self {
        if macos && self.message.contains("(os error 65)") {
            self.code = "local_network_blocked";
            self.message = format!(
                "{} — macOS is probably blocking Termalith from your local network: \
                 turn Termalith on in System Settings → Privacy & Security → Local Network, \
                 then quit and reopen Termalith",
                self.message
            );
        }
        self
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
        Self::new(code, e.to_string())
    }
}

impl From<SshError> for IpcError {
    fn from(e: SshError) -> Self {
        let message = e.to_string();
        match e {
            SshError::Timeout { .. } => Self::new("timeout", message),
            SshError::Connect(_) => Self::new("connect_failed", message),
            SshError::HostKeyRejected { .. } => Self::new("host_key_rejected", message),
            SshError::HostKeyChanged {
                host,
                port,
                presented,
                known,
            } => Self {
                code: "host_key_changed",
                message,
                detail: Some(serde_json::json!({
                    "host": host, "port": port, "presented": presented, "known": known,
                })),
            },
            SshError::HostCertificateUnsupported => {
                Self::new("host_certificate_unsupported", message)
            }
            SshError::AuthFailed => Self::new("auth_failed", message),
            SshError::Key(_) => Self::new("key_error", message),
            SshError::Agent(_) => Self::new("agent_error", message),
            SshError::KnownHosts(_) => Self::new("known_hosts_error", message),
            SshError::Protocol(_) => Self::new("protocol_error", message),
            SshError::Sftp(_) => Self::new("sftp_error", message),
            SshError::DestinationExists(_) => Self::new("destination_exists", message),
            SshError::Cancelled => Self::new("cancelled", message),
            SshError::TooLarge { .. } => Self::new("too_large", message),
            SshError::Modified(_) => Self::new("remote_modified", message),
            SshError::NotAFile(_) => Self::new("not_a_file", message),
            SshError::Io(_) => Self::new("io", message),
        }
    }
}

impl From<AppError> for IpcError {
    fn from(e: AppError) -> Self {
        match e {
            AppError::Invalid { .. } => Self::new("invalid", e.to_string()),
            AppError::NotFound => Self::new("not_found", e.to_string()),
            AppError::PasswordRequired => Self::new("password_required", e.to_string()),
            AppError::Ssh(s) => s.into(),
            AppError::Pty(p) => p.into(),
            AppError::Secret(s) => Self::new("secret_store", s.to_string()),
            AppError::Db(_) => Self::new("storage", e.to_string()),
            AppError::SerialPort(serial_console::SerialError::PermissionDenied { .. }) => {
                Self::new("serial_permission", e.to_string())
            }
            AppError::SerialPort(_) | AppError::Serial(_) => {
                Self::new("serial_error", e.to_string())
            }
            AppError::Desktop(remote_desktop::DesktopError::NoClient(_)) => {
                Self::new("desktop_no_client", e.to_string())
            }
            AppError::Desktop(_) => Self::new("desktop_error", e.to_string()),
            AppError::Network(_) => Self::new("connect_failed", e.to_string()),
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
        assert!(json.get("detail").is_none());
    }

    #[test]
    fn host_key_changed_carries_fingerprints() {
        let e = IpcError::from(AppError::Ssh(SshError::HostKeyChanged {
            host: "h".into(),
            port: 22,
            presented: "SHA256:new".into(),
            known: vec!["SHA256:old".into()],
        }));
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["code"], "host_key_changed");
        assert_eq!(json["detail"]["presented"], "SHA256:new");
        assert_eq!(json["detail"]["known"][0], "SHA256:old");
    }

    #[test]
    fn password_required_code() {
        let json = serde_json::to_value(IpcError::from(AppError::PasswordRequired)).unwrap();
        assert_eq!(json["code"], "password_required");
    }

    #[test]
    fn no_route_to_host_on_macos_points_to_local_network_settings() {
        let raw = "could not connect: No route to host (os error 65)";
        let e = IpcError {
            code: "connect_failed",
            message: raw.into(),
            detail: None,
        };
        let mac = e.local_network(true);
        assert_eq!(mac.code, "local_network_blocked");
        assert!(mac.message.starts_with(raw));
        assert!(mac.message.contains("Privacy & Security → Local Network"));

        let other = IpcError {
            code: "connect_failed",
            message: raw.into(),
            detail: None,
        }
        .local_network(false);
        assert_eq!(other.code, "connect_failed");
        assert_eq!(other.message, raw);

        let refused = IpcError {
            code: "connect_failed",
            message: "could not connect: Connection refused (os error 61)".into(),
            detail: None,
        }
        .local_network(true);
        assert_eq!(refused.code, "connect_failed");
    }
}
