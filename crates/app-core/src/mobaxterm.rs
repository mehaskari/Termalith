//! Import of MobaXterm session exports (`.mxtsessions`).
//!
//! The file is INI-like: `[Bookmarks…]` sections with `SubRep=` (folder)
//! and one `name= #type#flags%host%port%user%…` line per session. Only the
//! first fields are used; everything else (fonts, colours, terminal
//! options) is MobaXterm-specific. Passwords are never in these files.

use crate::AppError;
use crate::sessions::{AuthMethod, SessionInput, SessionProtocol};
use crate::{ImportCandidate, ImportReport, SessionStore, import_candidates};

/// Whether `text` looks like a MobaXterm export.
pub fn is_mobaxterm(text: &str) -> bool {
    text.lines().any(|l| {
        l.trim_start_matches('\u{feff}')
            .trim()
            .starts_with("[Bookmarks")
    })
}

/// Parses sessions; returns the candidates and notes about entries that
/// cannot be imported.
pub fn parse_mobaxterm(text: &str) -> (Vec<ImportCandidate>, Vec<String>) {
    let mut folder: Option<String> = None;
    let mut out: Vec<ImportCandidate> = Vec::new();
    let mut notes = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_start_matches('\u{feff}').trim();
        if line.starts_with('[') {
            folder = None;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key.eq_ignore_ascii_case("SubRep") {
            let f = value.replace('\\', " / ");
            folder = (!f.trim().is_empty()).then(|| f.trim().to_owned());
            continue;
        }
        if key.eq_ignore_ascii_case("ImgNum") || !value.starts_with('#') || key.is_empty() {
            continue;
        }
        let parts: Vec<&str> = value.splitn(3, '#').collect();
        let (Some(kind), Some(rest)) = (parts.get(1), parts.get(2)) else {
            continue;
        };
        let protocol = match *kind {
            "109" => SessionProtocol::Ssh,
            "91" => SessionProtocol::Rdp,
            "128" => SessionProtocol::Vnc,
            _ => {
                notes.push(format!(
                    "{key}: this MobaXterm session type is not supported"
                ));
                continue;
            }
        };
        let fields: Vec<&str> = rest.split('%').collect();
        let host = fields.get(1).copied().unwrap_or("").trim().to_owned();
        let default_port = match protocol {
            SessionProtocol::Ssh => 22,
            SessionProtocol::Rdp => 3389,
            SessionProtocol::Vnc => 5900,
        };
        let port = fields
            .get(2)
            .and_then(|p| p.trim().parse::<u16>().ok())
            .filter(|p| *p > 0)
            .unwrap_or(default_port);
        let user = fields.get(3).copied().unwrap_or("").trim().to_owned();
        let input = SessionInput {
            name: key.to_owned(),
            host,
            port,
            username: user,
            auth: AuthMethod::Password,
            folder: folder.clone(),
            jump_via: None,
            protocol,
            domain: None,
            tags: Vec::new(),
            forward_agent: false,
        };
        if let Err(e) = input.validate() {
            notes.push(format!("{key}: {e}"));
            continue;
        }
        if out.iter().any(|c| c.alias.eq_ignore_ascii_case(key)) {
            notes.push(format!("{key}: duplicate name"));
            continue;
        }
        out.push(ImportCandidate {
            alias: key.to_owned(),
            input,
            warnings: Vec::new(),
            jump_alias: None,
        });
    }
    (out, notes)
}

/// Imports every session from a MobaXterm export. Existing names are
/// skipped; entries that cannot be imported are listed in the report.
pub fn import_mobaxterm(store: &SessionStore, text: &str) -> Result<ImportReport, AppError> {
    if text.len() > crate::settings::MAX_IMPORT_BYTES {
        return Err(AppError::invalid("import", "file is too large"));
    }
    let (candidates, notes) = parse_mobaxterm(text);
    let mut report = import_candidates(store, &candidates, &[])?;
    report.not_imported = notes;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{feff}[Bookmarks]
SubRep=
ImgNum=42
web01= #109#0%192.168.1.10%22%root%%-1%-1%%%22%%0%0%0%%%-1%0%0%0%%1080%%0%0%1#MobaFont%10%0%0%0%15%236,236,236%30,30,30%180,180,192%0%-1%0%%xterm%-1%-1%_Std_Colors_0_%80%24%0%1%-1%<none>%%0#0# #-1
telnet box= #98#1%10.0.0.2%23%%%2%%%%%0%0%%1080%#MobaFont%10#0# #-1

[Bookmarks_1]
SubRep=Prod\\DB
ImgNum=41
db primary= #109#0%10.0.0.9%2222%postgres%%-1%-1%%%%%0%0%0%%%-1%0%0%0%%1080%%0%0%1#MobaFont#0# #-1
win= #91#4%win.corp.local%3389%admin%0%-1%-1%%%%%0%0%0%%%-1#MobaFont#0# #-1
vnc lab= #128#5%192.168.1.40%5901%%%-1#MobaFont#0# #-1
nouser= #109#0%10.0.0.5%22%%%-1#MobaFont#0# #-1
web01= #109#0%10.9.9.9%22%root%#MobaFont#0# #-1
";

    #[test]
    fn parses_ssh_rdp_vnc_with_folders() {
        assert!(is_mobaxterm(SAMPLE));
        assert!(!is_mobaxterm("{\"format\":\"termalith-sessions\"}"));
        let (c, notes) = parse_mobaxterm(SAMPLE);
        let names: Vec<_> = c.iter().map(|x| x.alias.as_str()).collect();
        assert_eq!(names, ["web01", "db primary", "win", "vnc lab"]);
        let web = &c[0].input;
        assert_eq!(
            (web.host.as_str(), web.port, web.username.as_str()),
            ("192.168.1.10", 22, "root")
        );
        assert_eq!(web.folder, None);
        let db = &c[1].input;
        assert_eq!(db.folder.as_deref(), Some("Prod / DB"));
        assert_eq!(db.port, 2222);
        assert_eq!(c[2].input.protocol, SessionProtocol::Rdp);
        assert_eq!(c[2].input.username, "admin");
        assert_eq!(c[3].input.protocol, SessionProtocol::Vnc);
        assert_eq!(c[3].input.port, 5901);
        assert!(
            notes.iter().any(|n| n.starts_with("telnet box")),
            "{notes:?}"
        );
        assert!(notes.iter().any(|n| n.starts_with("nouser")), "{notes:?}");
        assert!(
            notes.iter().any(|n| n == "web01: duplicate name"),
            "{notes:?}"
        );
    }

    #[test]
    fn import_creates_sessions_and_reports_the_rest() {
        let store = SessionStore::open_in_memory().unwrap();
        let r = import_mobaxterm(&store, SAMPLE).unwrap();
        assert_eq!(r.created.len(), 4);
        assert_eq!(r.not_imported.len(), 3);
        let again = import_mobaxterm(&store, SAMPLE).unwrap();
        assert!(again.created.is_empty());
        assert_eq!(again.skipped.len(), 4);
    }
}
