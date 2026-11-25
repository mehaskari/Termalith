//! Import of PuTTY sessions from a registry export: the text of
//! `reg export HKCU\Software\SimonTatham\PuTTY\Sessions putty.reg` (or the
//! same branch saved from regedit). Each session is a key
//! `[...\PuTTY\Sessions\<name>]` with values such as `"HostName"="…"` and
//! `"PortNumber"=dword:00000016`. SSH and Telnet sessions are imported;
//! passwords are never in these files.

use crate::AppError;
use crate::sessions::{AuthMethod, SessionInput, SessionProtocol};
use crate::{ImportCandidate, ImportReport, SessionStore, import_candidates};

const SESSIONS_KEY: &str = "\\software\\simontatham\\putty\\sessions\\";

/// Folder the imported sessions go to.
pub const PUTTY_FOLDER: &str = "PuTTY";

/// Whether `text` is a registry export with PuTTY sessions.
pub fn is_putty_reg(text: &str) -> bool {
    text.lines().any(|l| session_name(l.trim()).is_some())
}

/// The session name of a `[HKEY_CURRENT_USER\…\Sessions\<name>]` line.
fn session_name(line: &str) -> Option<String> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    let lower = inner.to_ascii_lowercase();
    let at = lower.find(SESSIONS_KEY)? + SESSIONS_KEY.len();
    let raw = &inner[at..];
    (!raw.is_empty() && !raw.contains('\\')).then(|| percent_decode(raw))
}

/// PuTTY stores names with `%XX` escapes (`My%20server`).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(b) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A `"Name"=value` line: a string (with `\\` and `\"` escapes) or a dword.
fn value(line: &str) -> Option<(String, Value)> {
    let rest = line.strip_prefix('"')?;
    let end = rest.find("\"=")?;
    let name = rest[..end].to_owned();
    let raw = &rest[end + 2..];
    if let Some(hex) = raw.strip_prefix("dword:") {
        return Some((
            name,
            Value::Number(u32::from_str_radix(hex.trim(), 16).ok()?),
        ));
    }
    let s = raw.strip_prefix('"')?.strip_suffix('"')?;
    Some((
        name,
        Value::Text(s.replace("\\\\", "\\").replace("\\\"", "\"")),
    ))
}

enum Value {
    Text(String),
    Number(u32),
}

#[derive(Default)]
struct Raw {
    name: String,
    host: String,
    port: Option<u32>,
    user: String,
    protocol: String,
    key_file: String,
    agent_forward: bool,
    x11: bool,
}

/// Parses the sessions; returns the candidates and notes about entries
/// that cannot be imported.
pub fn parse_putty(text: &str) -> (Vec<ImportCandidate>, Vec<String>) {
    let mut raws: Vec<Raw> = Vec::new();
    for line in text
        .lines()
        .map(|l| l.trim_start_matches('\u{feff}').trim())
    {
        if line.starts_with('[') {
            if let Some(name) = session_name(line) {
                raws.push(Raw {
                    name,
                    ..Raw::default()
                });
            } else {
                // Another key: values below it are not a session's.
                raws.push(Raw::default());
            }
            continue;
        }
        let (Some(r), Some((key, v))) = (raws.last_mut(), value(line)) else {
            continue;
        };
        match (key.as_str(), v) {
            ("HostName", Value::Text(t)) => r.host = t,
            ("PortNumber", Value::Number(n)) => r.port = Some(n),
            ("UserName", Value::Text(t)) => r.user = t,
            ("Protocol", Value::Text(t)) => r.protocol = t.to_ascii_lowercase(),
            ("PublicKeyFile", Value::Text(t)) => r.key_file = t,
            ("AgentFwd", Value::Number(n)) => r.agent_forward = n != 0,
            ("X11Forward", Value::Number(n)) => r.x11 = n != 0,
            _ => {}
        }
    }

    let mut out: Vec<ImportCandidate> = Vec::new();
    let mut notes = Vec::new();
    for r in raws.into_iter().filter(|r| !r.name.is_empty()) {
        // PuTTY's template, not a server.
        if r.name.eq_ignore_ascii_case("Default Settings") {
            continue;
        }
        let protocol = match r.protocol.as_str() {
            "ssh" | "" => SessionProtocol::Ssh,
            "telnet" => SessionProtocol::Telnet,
            other => {
                notes.push(format!(
                    "{}: PuTTY {other} sessions are not imported",
                    r.name
                ));
                continue;
            }
        };
        // "user@host" in the host field is allowed in PuTTY.
        let (mut user, mut host) = (r.user.trim().to_owned(), r.host.trim().to_owned());
        if let Some((u, h)) = host.clone().split_once('@')
            && user.is_empty()
        {
            user = u.to_owned();
            host = h.to_owned();
        }
        if host.is_empty() {
            notes.push(format!("{}: no host name", r.name));
            continue;
        }
        let default_port = if protocol == SessionProtocol::Telnet {
            23
        } else {
            22
        };
        let port = r
            .port
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p > 0)
            .unwrap_or(default_port);
        let mut warnings = Vec::new();
        let auth = if protocol == SessionProtocol::Ssh && !r.key_file.trim().is_empty() {
            let path = r.key_file.trim().to_owned();
            if path.to_ascii_lowercase().ends_with(".ppk") {
                warnings.push(
                    "uses a PuTTY key (.ppk): convert it to OpenSSH format with PuTTYgen (Conversions → Export OpenSSH key) and choose that file".to_owned(),
                );
                AuthMethod::Password
            } else if !std::path::Path::new(&path).is_absolute() {
                // A Windows path imported on macOS or Linux.
                warnings.push(format!(
                    "key file {path} is not a path on this computer: choose the key in the session"
                ));
                AuthMethod::Password
            } else {
                AuthMethod::KeyFile { path: path.into() }
            }
        } else {
            AuthMethod::Password
        };
        let ssh = protocol == SessionProtocol::Ssh;
        let input = SessionInput {
            name: r.name.clone(),
            host,
            port,
            username: user,
            auth,
            folder: Some(PUTTY_FOLDER.to_owned()),
            jump_via: None,
            protocol,
            domain: None,
            tags: Vec::new(),
            forward_agent: ssh && r.agent_forward,
            x11_forward: ssh && r.x11,
            startup_command: None,
            files_only: false,
            mosh: false,
            appearance: None,
            icon: None,
            s3: None,
            legacy_ssh: false,
        };
        if let Err(e) = input.validate() {
            notes.push(format!("{}: {e}", r.name));
            continue;
        }
        if out.iter().any(|c| c.alias.eq_ignore_ascii_case(&r.name)) {
            notes.push(format!("{}: duplicate name", r.name));
            continue;
        }
        if let Some(w) = warnings.first() {
            notes.push(format!("{}: {w}", r.name));
        }
        out.push(ImportCandidate {
            alias: r.name,
            input,
            warnings,
            jump_alias: None,
        });
    }
    (out, notes)
}

/// Imports every SSH and Telnet session from a PuTTY registry export into
/// the "PuTTY" folder. Existing names are skipped.
pub fn import_putty(store: &SessionStore, text: &str) -> Result<ImportReport, AppError> {
    if text.len() > crate::settings::MAX_IMPORT_BYTES {
        return Err(AppError::invalid("import", "file is too large"));
    }
    let (candidates, notes) = parse_putty(text);
    let mut report = import_candidates(store, &candidates, &[])?;
    report.not_imported = notes;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Windows Registry Editor Version 5.00

[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions]

[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\Default%20Settings]
\"HostName\"=\"\"
\"Protocol\"=\"ssh\"

[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\core%20switch]
\"HostName\"=\"10.0.0.254\"
\"PortNumber\"=dword:00000017
\"Protocol\"=\"telnet\"

[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\web-01]
\"HostName\"=\"deploy@web1.example.com\"
\"PortNumber\"=dword:00000016
\"UserName\"=\"\"
\"Protocol\"=\"ssh\"
\"AgentFwd\"=dword:00000001
\"X11Forward\"=dword:00000000
\"PublicKeyFile\"=\"C:\\\\Users\\\\me\\\\.ssh\\\\id_ed25519\"

[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\old%20box]
\"HostName\"=\"10.1.1.1\"
\"PortNumber\"=dword:000008ae
\"UserName\"=\"admin\"
\"Protocol\"=\"ssh\"
\"PublicKeyFile\"=\"C:\\\\keys\\\\box.ppk\"

[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\console]
\"HostName\"=\"\"
\"Protocol\"=\"serial\"

[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\SshHostKeys]
\"ssh-ed25519@22:web1.example.com\"=\"0x1234\"
";

    #[test]
    fn reads_ssh_and_telnet_sessions() {
        assert!(is_putty_reg(SAMPLE));
        assert!(!is_putty_reg("[Bookmarks]\nSubRep="));
        let (sessions, notes) = parse_putty(SAMPLE);
        let names: Vec<&str> = sessions.iter().map(|c| c.alias.as_str()).collect();
        assert_eq!(names, ["core switch", "web-01", "old box"]);

        let sw = &sessions[0].input;
        assert_eq!(
            (sw.protocol, sw.host.as_str(), sw.port),
            (SessionProtocol::Telnet, "10.0.0.254", 23)
        );
        assert_eq!(sw.folder.as_deref(), Some(PUTTY_FOLDER));

        let web = &sessions[1].input;
        assert_eq!(
            (web.username.as_str(), web.host.as_str(), web.port),
            ("deploy", "web1.example.com", 22)
        );
        assert!(web.forward_agent && !web.x11_forward);
        if cfg!(windows) {
            assert_eq!(
                web.auth,
                AuthMethod::KeyFile {
                    path: "C:\\Users\\me\\.ssh\\id_ed25519".into()
                }
            );
        } else {
            assert_eq!(
                web.auth,
                AuthMethod::Password,
                "a Windows path is no key here"
            );
            assert!(sessions[1].warnings[0].contains("not a path on this computer"));
        }

        let old = &sessions[2].input;
        assert_eq!(old.port, 2222);
        assert_eq!(
            old.auth,
            AuthMethod::Password,
            ".ppk keys cannot be used directly"
        );
        assert!(sessions[2].warnings[0].contains("PuTTYgen"));

        assert!(
            notes
                .iter()
                .any(|n| n.starts_with("console:") && n.contains("serial")),
            "{notes:?}"
        );
        assert!(notes.iter().any(|n| n.starts_with("old box:")));
        assert!(!notes.iter().any(|n| n.contains("Default")));
    }

    #[test]
    fn imports_into_the_store_and_skips_existing_names() {
        let store = SessionStore::open_in_memory().unwrap();
        let first = import_putty(&store, SAMPLE).unwrap();
        assert_eq!(first.created.len(), 3);
        let again = import_putty(&store, SAMPLE).unwrap();
        assert!(again.created.is_empty());
        assert_eq!(again.skipped.len(), 3);
    }

    #[test]
    fn decodes_names_and_ignores_other_keys() {
        assert_eq!(percent_decode("a%20b%2Fc%"), "a b/c%");
        assert_eq!(
            session_name("[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions]"),
            None
        );
        assert_eq!(
            session_name("[HKEY_CURRENT_USER\\Software\\SimonTatham\\PuTTY\\Sessions\\x\\y]"),
            None,
            "sub-keys are not sessions"
        );
    }
}
