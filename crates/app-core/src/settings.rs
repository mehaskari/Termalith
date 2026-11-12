//! Application settings and portable session export/import.

use credential_store::CredentialRef;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::sessions::{AuthMethod, SessionInput, SessionProtocol, SessionStore};
use crate::{AppError, ImportReport};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyType {
    Socks5,
    Http,
}

/// Proxy for outgoing SSH and Telnet connections (first hop only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySetting {
    pub kind: ProxyType,
    pub host: String,
    pub port: u16,
    /// Login for proxies that require one; the password is kept in the OS
    /// credential store (see `Connector::set_proxy_password`).
    #[serde(default)]
    pub username: Option<String>,
}

impl ProxySetting {
    /// The proxy with its login, when a user name is set.
    pub fn to_proxy(&self, password: Option<Zeroizing<String>>) -> protocol_ssh::Proxy {
        let username = self
            .username
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty());
        protocol_ssh::Proxy {
            kind: match self.kind {
                ProxyType::Socks5 => protocol_ssh::ProxyKind::Socks5,
                ProxyType::Http => protocol_ssh::ProxyKind::HttpConnect,
            },
            host: self.host.trim().to_owned(),
            port: self.port,
            auth: username.map(|u| protocol_ssh::ProxyAuth {
                username: u.to_owned(),
                password: password.unwrap_or_default(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: Theme,
    pub font_size: u8,
    pub font_family: String,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub scrollback: u32,
    /// Start a session log for every new terminal.
    pub log_sessions: bool,
    /// Shell id (see `terminal_pty::available_shells`) for new local
    /// terminals; `None` uses the system default.
    pub default_shell: Option<String>,
    /// Copy terminal selections to the clipboard automatically.
    pub copy_on_select: bool,
    /// Right-click in a terminal pastes.
    pub right_click_paste: bool,
    /// Ask before pasting several lines, a download piped into a shell, or
    /// invisible/control characters into a terminal.
    pub paste_warning: bool,
    /// Reconnect SSH tabs whose connection was lost.
    pub auto_reconnect: bool,
    /// Terminal color scheme id; "app" follows the app theme.
    pub terminal_scheme: String,
    /// Proxy for outgoing connections; `None` connects directly.
    pub proxy: Option<ProxySetting>,
    /// Ask GitHub for a newer release at startup (off by default: no
    /// network request without the user's choice).
    pub check_updates: bool,
    /// Show RDP sessions inside Termalith (experimental, ADR-030).
    pub embedded_rdp: bool,
    /// Show the server monitor bar under SSH terminals (runs a read-only
    /// command on the server every few seconds).
    pub server_monitor: bool,
    /// Color keywords such as error and warning in terminal output.
    pub highlight_output: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            font_size: 13,
            font_family: String::new(),
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            scrollback: 5000,
            log_sessions: false,
            default_shell: None,
            copy_on_select: false,
            right_click_paste: false,
            paste_warning: true,
            auto_reconnect: true,
            terminal_scheme: "app".into(),
            proxy: None,
            check_updates: false,
            embedded_rdp: false,
            server_monitor: false,
            highlight_output: false,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), AppError> {
        if !(8..=32).contains(&self.font_size) {
            return Err(AppError::invalid("font size", "must be 8-32"));
        }
        if self.font_family.chars().count() > 200 || self.font_family.chars().any(char::is_control)
        {
            return Err(AppError::invalid(
                "font family",
                "too long or contains control characters",
            ));
        }
        if let Some(s) = &self.default_shell
            && (s.is_empty() || s.chars().count() > 1024 || s.chars().any(char::is_control))
        {
            return Err(AppError::invalid("default shell", "not a valid shell id"));
        }
        if self.terminal_scheme.is_empty()
            || self.terminal_scheme.len() > 40
            || !self
                .terminal_scheme
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(AppError::invalid("terminal colors", "unknown scheme"));
        }
        if let Some(p) = &self.proxy {
            let h = p.host.trim();
            if h.is_empty()
                || h.len() > 253
                || h.starts_with('-')
                || h.chars().any(|c| c.is_whitespace() || c.is_control())
                || p.port == 0
            {
                return Err(AppError::invalid("proxy", "enter a host and a port"));
            }
            if let Some(u) = &p.username
                && (u.len() > 255 || u.chars().any(char::is_control))
            {
                return Err(AppError::invalid(
                    "proxy user name",
                    "too long or contains control characters",
                ));
            }
        }
        if !(100..=100_000).contains(&self.scrollback) {
            return Err(AppError::invalid("scrollback", "must be 100-100000 lines"));
        }
        Ok(())
    }
}

const SETTINGS_KEY: &str = "app";
/// Reference to the proxy password in the OS credential store.
const PROXY_CREDENTIAL_KEY: &str = "proxy_credential";

impl SessionStore {
    /// Stored settings; defaults for anything missing or unreadable.
    pub fn settings(&self) -> Result<Settings, AppError> {
        let raw: Option<String> = self
            .conn()
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [SETTINGS_KEY],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw
            .and_then(|v| serde_json::from_str::<Settings>(&v).ok())
            .filter(|s| s.validate().is_ok())
            .unwrap_or_default())
    }

    pub fn save_settings(&self, s: &Settings) -> Result<(), AppError> {
        s.validate()?;
        let json = serde_json::to_string(s).map_err(|e| AppError::Db(e.to_string()))?;
        self.conn().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![SETTINGS_KEY, json],
        )?;
        Ok(())
    }

    pub fn proxy_credential(&self) -> Result<Option<CredentialRef>, AppError> {
        let raw: Option<String> = self
            .conn()
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [PROXY_CREDENTIAL_KEY],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw.and_then(|r| CredentialRef::parse(&r).ok()))
    }

    pub fn set_proxy_credential(&self, cred: Option<&CredentialRef>) -> Result<(), AppError> {
        match cred {
            Some(c) => self.conn().execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![PROXY_CREDENTIAL_KEY, c.as_str()],
            )?,
            None => self.conn().execute(
                "DELETE FROM settings WHERE key = ?1",
                [PROXY_CREDENTIAL_KEY],
            )?,
        };
        Ok(())
    }
}

// ---------------------------------------------------------------- export

pub const EXPORT_FORMAT: &str = "termalith-sessions";
pub const EXPORT_VERSION: u32 = 1;
/// Refuse absurdly large imports (pasted text).
pub const MAX_IMPORT_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_IMPORT_SESSIONS: usize = 10_000;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportFile {
    format: String,
    version: u32,
    sessions: Vec<ExportedSession>,
}

/// A session without secrets or credential references. The jump host is
/// referred to by name so the file is portable between machines.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportedSession {
    name: String,
    host: String,
    port: u16,
    username: String,
    auth: AuthMethod,
    folder: Option<String>,
    #[serde(default)]
    favorite: bool,
    #[serde(default)]
    jump_via: Option<String>,
    #[serde(default)]
    protocol: SessionProtocol,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    forward_agent: bool,
    #[serde(default)]
    x11_forward: bool,
    #[serde(default)]
    startup_command: Option<String>,
    #[serde(default)]
    files_only: bool,
    #[serde(default)]
    mosh: bool,
    #[serde(default)]
    appearance: Option<crate::sessions::Appearance>,
    #[serde(default)]
    icon: Option<crate::sessions::Icon>,
}

/// Serializes every session as versioned JSON. Never includes secrets.
pub fn export_sessions(store: &SessionStore) -> Result<String, AppError> {
    let all = store.list()?;
    let name_of = |id: &crate::SessionId| {
        all.iter()
            .find(|s| &s.id == id)
            .map(|s| s.input.name.clone())
    };
    let file = ExportFile {
        format: EXPORT_FORMAT.into(),
        version: EXPORT_VERSION,
        sessions: all
            .iter()
            .map(|s| ExportedSession {
                name: s.input.name.clone(),
                host: s.input.host.clone(),
                port: s.input.port,
                username: s.input.username.clone(),
                auth: s.input.auth.clone(),
                folder: s.input.folder.clone(),
                favorite: s.favorite,
                jump_via: s.input.jump_via.as_ref().and_then(name_of),
                protocol: s.input.protocol,
                domain: s.input.domain.clone(),
                tags: s.input.tags.clone(),
                forward_agent: s.input.forward_agent,
                x11_forward: s.input.x11_forward,
                startup_command: s.input.startup_command.clone(),
                files_only: s.input.files_only,
                mosh: s.input.mosh,
                appearance: s.input.appearance.clone(),
                icon: s.input.icon.clone(),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&file).map_err(|e| AppError::Db(e.to_string()))
}

/// Imports sessions from [`export_sessions`] output. Existing names are
/// skipped; every entry is validated like a manually created session.
pub fn import_sessions(store: &SessionStore, json: &str) -> Result<ImportReport, AppError> {
    if json.len() > MAX_IMPORT_BYTES {
        return Err(AppError::invalid("import", "file is too large"));
    }
    let file: ExportFile = serde_json::from_str(json)
        .map_err(|e| AppError::invalid("import", format!("not a Termalith export: {e}")))?;
    if file.format != EXPORT_FORMAT {
        return Err(AppError::invalid(
            "import",
            "not a Termalith session export",
        ));
    }
    if file.version > EXPORT_VERSION {
        return Err(AppError::invalid(
            "import",
            format!("made by a newer version (format v{})", file.version),
        ));
    }
    if file.sessions.len() > MAX_IMPORT_SESSIONS {
        return Err(AppError::invalid("import", "too many sessions"));
    }
    // Validate everything first so a bad entry doesn't leave a half import.
    for s in &file.sessions {
        to_input(s)
            .validate()
            .map_err(|e| AppError::invalid("import", format!("{}: {e}", s.name)))?;
    }
    let existing: Vec<String> = store
        .list()?
        .into_iter()
        .map(|s| s.input.name.to_lowercase())
        .collect();
    let mut report = ImportReport::default();
    let mut created = Vec::new();
    for s in &file.sessions {
        if existing.contains(&s.name.to_lowercase())
            || report
                .created
                .iter()
                .any(|c| c.eq_ignore_ascii_case(&s.name))
        {
            report.skipped.push(s.name.clone());
            continue;
        }
        let rec = store.create(&to_input(s))?;
        if s.favorite {
            store.set_favorite(&rec.id, true)?;
        }
        report.created.push(s.name.clone());
        created.push((rec, s));
    }
    let all = store.list()?;
    for (rec, s) in created {
        let Some(jump) = &s.jump_via else { continue };
        match all.iter().find(|x| x.input.name.eq_ignore_ascii_case(jump)) {
            Some(j) if j.id != rec.id => {
                let mut input = rec.input.clone();
                input.jump_via = Some(j.id.clone());
                store.update(&rec.id, &input)?;
            }
            _ => report
                .unresolved_jumps
                .push(format!("{} -> {jump}", s.name)),
        }
    }
    Ok(report)
}

fn to_input(s: &ExportedSession) -> SessionInput {
    SessionInput {
        name: s.name.clone(),
        host: s.host.clone(),
        port: s.port,
        username: s.username.clone(),
        auth: s.auth.clone(),
        folder: s.folder.clone(),
        jump_via: None,
        protocol: s.protocol,
        domain: s.domain.clone(),
        tags: s.tags.clone(),
        forward_agent: s.forward_agent,
        x11_forward: s.x11_forward,
        startup_command: s.startup_command.clone(),
        files_only: s.files_only,
        mosh: s.mosh,
        appearance: s.appearance.clone(),
        // A damaged icon in the file must not refuse the session.
        icon: s.icon.clone().filter(|i| i.validate().is_ok()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use credential_store::CredentialRef;

    #[test]
    fn settings_default_round_trip_and_validation() {
        let s = SessionStore::open_in_memory().unwrap();
        assert_eq!(s.settings().unwrap(), Settings::default());
        let mut custom = Settings {
            theme: Theme::Light,
            font_size: 15,
            ..Settings::default()
        };
        s.save_settings(&custom).unwrap();
        assert_eq!(s.settings().unwrap(), custom);
        custom.font_size = 99;
        assert!(s.save_settings(&custom).is_err());
        custom.font_size = 15;
        custom.scrollback = 10;
        assert!(s.save_settings(&custom).is_err());
        custom.scrollback = 5000;
        custom.proxy = Some(ProxySetting {
            kind: ProxyType::Socks5,
            host: "".into(),
            port: 1080,
            username: None,
        });
        assert!(s.save_settings(&custom).is_err(), "proxy needs a host");
        custom.proxy = Some(ProxySetting {
            kind: ProxyType::Http,
            host: "proxy.corp".into(),
            port: 3128,
            username: Some("alice".into()),
        });
        s.save_settings(&custom).unwrap();
        assert_eq!(s.settings().unwrap().proxy, custom.proxy);
        let mut bad = custom.clone();
        bad.proxy.as_mut().unwrap().username = Some("a\nb".into());
        assert!(
            s.save_settings(&bad).is_err(),
            "control characters in the user name"
        );
        // A saved proxy from before user names existed still loads.
        let old: ProxySetting =
            serde_json::from_str(r#"{"kind":"socks5","host":"p","port":1080}"#).unwrap();
        assert_eq!(old.username, None);
        assert!(old.to_proxy(None).auth.is_none());
        // Partial JSON (older version) fills defaults.
        let parsed: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(parsed.font_size, 13);
    }

    fn input(name: &str) -> SessionInput {
        SessionInput {
            name: name.into(),
            host: "h.example.com".into(),
            port: 22,
            username: "u".into(),
            auth: AuthMethod::Password,
            folder: Some("F".into()),
            jump_via: None,
            protocol: SessionProtocol::Ssh,
            domain: None,
            tags: Vec::new(),
            forward_agent: false,
            x11_forward: false,
            startup_command: None,
            files_only: false,
            mosh: false,
            appearance: None,
            icon: None,
        }
    }

    #[test]
    fn export_has_no_secrets_and_round_trips_with_jumps() {
        let src = SessionStore::open_in_memory().unwrap();
        let bastion = src.create(&input("bastion")).unwrap();
        let cred = CredentialRef::generate().unwrap();
        src.set_credential(&bastion.id, Some(&cred)).unwrap();
        src.set_favorite(&bastion.id, true).unwrap();
        let mut tagged = bastion.input.clone();
        tagged.tags = vec!["edge".into()];
        tagged.startup_command = Some("cd /srv".into());
        let mut files = input("files");
        files.files_only = true;
        src.create(&files).unwrap();
        src.update(&bastion.id, &tagged).unwrap();
        let mut app = input("app");
        app.jump_via = Some(bastion.id.clone());
        src.create(&app).unwrap();

        let json = export_sessions(&src).unwrap();
        assert!(!json.contains(cred.as_str()), "credential reference leaked");
        assert!(!json.contains("ses-"), "internal ids leaked");
        assert!(json.contains("\"jumpVia\": \"bastion\""));

        let dst = SessionStore::open_in_memory().unwrap();
        let report = import_sessions(&dst, &json).unwrap();
        assert_eq!(report.created.len(), 3);
        let all = dst.list().unwrap();
        assert!(
            all.iter()
                .any(|s| s.input.name == "files" && s.input.files_only)
        );
        let b = all.iter().find(|s| s.input.name == "bastion").unwrap();
        let a = all.iter().find(|s| s.input.name == "app").unwrap();
        assert!(b.favorite);
        assert_eq!(b.input.tags, ["edge"]);
        assert_eq!(b.input.startup_command.as_deref(), Some("cd /srv"));
        assert!(!b.has_saved_secret, "secrets are never imported");
        assert_eq!(a.input.jump_via.as_ref(), Some(&b.id));

        // Importing again skips everything.
        let again = import_sessions(&dst, &json).unwrap();
        assert!(again.created.is_empty());
        assert_eq!(again.skipped.len(), 3);
    }

    #[test]
    fn import_rejects_bad_files_without_partial_writes() {
        let s = SessionStore::open_in_memory().unwrap();
        assert!(import_sessions(&s, "not json").is_err());
        assert!(import_sessions(&s, r#"{"format":"other","version":1,"sessions":[]}"#).is_err());
        assert!(
            import_sessions(
                &s,
                r#"{"format":"termalith-sessions","version":99,"sessions":[]}"#
            )
            .is_err()
        );
        let one_bad = r#"{"format":"termalith-sessions","version":1,"sessions":[
            {"name":"ok","host":"h","port":22,"username":"u","auth":{"kind":"password"},"folder":null},
            {"name":"bad","host":"-oProxyCommand=x","port":22,"username":"u","auth":{"kind":"password"},"folder":null}
        ]}"#;
        assert!(import_sessions(&s, one_bad).is_err());
        assert!(
            s.list().unwrap().is_empty(),
            "nothing may be imported when any entry is invalid"
        );
    }
}
