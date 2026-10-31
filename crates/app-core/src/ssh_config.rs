//! Import of saved sessions from an OpenSSH client config (`~/.ssh/config`).
//!
//! Supported subset (documented, not "full compatibility"): `Host` blocks
//! with `*`, `?` and `!` patterns, top-level defaults, and the keywords
//! `HostName`, `User`, `Port`, `IdentityFile`. Like OpenSSH, the first value
//! obtained for a keyword wins. Anything else that would change how a
//! connection behaves (ProxyJump, ProxyCommand, Match, Include, …) is
//! reported as a warning instead of being silently ignored.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::AppError;
use crate::sessions::{AuthMethod, SessionInput, SessionStore};

/// Folder imported sessions are placed in.
pub const IMPORT_FOLDER: &str = "SSH config";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    pub alias: String,
    pub input: SessionInput,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub created: Vec<String>,
    /// Aliases skipped because a session with that name already exists.
    pub skipped: Vec<String>,
}

struct Block {
    patterns: Vec<String>,
    /// Lowercased keyword -> values, in file order.
    options: Vec<(String, String)>,
}

/// Keywords that change connection behaviour but are not imported.
const UNSUPPORTED: &[&str] = &[
    "proxyjump",
    "proxycommand",
    "localforward",
    "remoteforward",
    "dynamicforward",
    "forwardagent",
    "forwardx11",
    "certificatefile",
    "pkcs11provider",
    "hostkeyalias",
];

fn split_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let idx = line.find(|c: char| c.is_whitespace() || c == '=')?;
    let key = line[..idx].to_ascii_lowercase();
    let rest = line[idx..].trim_start_matches(|c: char| c.is_whitespace() || c == '=');
    let value = rest.trim().trim_matches('"').to_owned();
    Some((key, value))
}

fn glob(pattern: &str, text: &str) -> bool {
    fn rec(p: &[u8], t: &[u8]) -> bool {
        match (p.first(), t.first()) {
            (None, None) => true,
            (Some(b'*'), _) => rec(&p[1..], t) || (!t.is_empty() && rec(p, &t[1..])),
            (Some(b'?'), Some(_)) => rec(&p[1..], &t[1..]),
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => rec(&p[1..], &t[1..]),
            _ => false,
        }
    }
    rec(pattern.as_bytes(), text.as_bytes())
}

fn block_matches(patterns: &[String], alias: &str) -> bool {
    let mut positive = false;
    for p in patterns {
        if let Some(neg) = p.strip_prefix('!') {
            if glob(neg, alias) {
                return false;
            }
        } else if glob(p, alias) {
            positive = true;
        }
    }
    positive
}

fn is_concrete(pattern: &str) -> bool {
    !pattern.contains(['*', '?', '!'])
}

/// Parses `text` and returns one candidate per concrete `Host` alias.
pub fn parse_ssh_config(text: &str, home: &Path, default_user: &str) -> Vec<ImportCandidate> {
    let mut blocks = vec![Block {
        patterns: vec!["*".into()],
        options: Vec::new(),
    }];
    let mut global_warnings = Vec::new();
    let mut in_match = false;
    for line in text.lines() {
        let Some((key, value)) = split_line(line) else {
            continue;
        };
        match key.as_str() {
            "host" => {
                in_match = false;
                blocks.push(Block {
                    patterns: value.split_whitespace().map(str::to_owned).collect(),
                    options: Vec::new(),
                });
            }
            "match" => {
                in_match = true;
                global_warnings.push("Match blocks are not supported and were ignored".to_owned());
            }
            "include" => global_warnings.push(format!("Include {value} was not followed")),
            _ if in_match => {}
            _ => blocks
                .last_mut()
                .expect("there is always a block")
                .options
                .push((key, value)),
        }
    }
    global_warnings.dedup();

    let mut aliases: Vec<String> = Vec::new();
    for b in &blocks[1..] {
        for p in &b.patterns {
            if is_concrete(p) && !aliases.iter().any(|a| a.eq_ignore_ascii_case(p)) {
                aliases.push(p.clone());
            }
        }
    }

    aliases
        .into_iter()
        .map(|alias| {
            // First value wins, across all blocks that match, in file order.
            let mut opts: HashMap<String, String> = HashMap::new();
            let mut warnings = global_warnings.clone();
            for b in blocks.iter().filter(|b| block_matches(&b.patterns, &alias)) {
                for (k, v) in &b.options {
                    if UNSUPPORTED.contains(&k.as_str()) {
                        let w = format!("{k} is not supported yet and was not imported");
                        if !warnings.contains(&w) {
                            warnings.push(w);
                        }
                    }
                    opts.entry(k.clone()).or_insert_with(|| v.clone());
                }
            }
            let host = opts
                .get("hostname")
                .map(|h| h.replace("%h", &alias))
                .unwrap_or_else(|| alias.clone());
            let user = opts
                .get("user")
                .cloned()
                .unwrap_or_else(|| default_user.to_owned());
            let port = match opts.get("port").map(|p| p.parse::<u16>()) {
                None => 22,
                Some(Ok(p)) if p > 0 => p,
                Some(_) => {
                    warnings.push(format!("invalid Port {:?}; using 22", opts["port"]));
                    22
                }
            };
            let auth = match opts.get("identityfile") {
                Some(f) => {
                    let expanded = expand_path(f, home, &user, &alias);
                    if expanded.is_absolute() {
                        AuthMethod::KeyFile { path: expanded }
                    } else {
                        warnings.push(format!(
                            "IdentityFile {f} is not an absolute path; using ssh-agent"
                        ));
                        AuthMethod::Agent
                    }
                }
                None => AuthMethod::Agent,
            };
            ImportCandidate {
                input: SessionInput {
                    name: alias.clone(),
                    host,
                    port,
                    username: user,
                    auth,
                    folder: Some(IMPORT_FOLDER.into()),
                },
                alias,
                warnings,
            }
        })
        .collect()
}

fn expand_path(raw: &str, home: &Path, user: &str, alias: &str) -> std::path::PathBuf {
    let s = raw
        .replace("%d", &home.to_string_lossy())
        .replace("%u", user)
        .replace("%h", alias)
        .replace("%%", "%");
    if let Some(rest) = s.strip_prefix("~/") {
        home.join(rest)
    } else if s == "~" {
        home.to_path_buf()
    } else {
        s.into()
    }
}

/// Creates sessions for `selected` aliases (all if empty). Existing session
/// names are skipped, never overwritten.
pub fn import_candidates(
    store: &SessionStore,
    candidates: &[ImportCandidate],
    selected: &[String],
) -> Result<ImportReport, AppError> {
    let existing: Vec<String> = store
        .list()?
        .into_iter()
        .map(|s| s.input.name.to_lowercase())
        .collect();
    let mut report = ImportReport::default();
    for c in candidates {
        if !selected.is_empty() && !selected.iter().any(|s| s == &c.alias) {
            continue;
        }
        if existing.contains(&c.alias.to_lowercase()) {
            report.skipped.push(c.alias.clone());
            continue;
        }
        store.create(&c.input)?;
        report.created.push(c.alias.clone());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const CONFIG: &str = r#"
# Global defaults (top of file)
User root

Host web-01 web-02
    HostName %h.example.com
    Port 2222

Host db
    HostName=10.0.0.9
    User postgres
    IdentityFile ~/.ssh/id_db

Host bastion
    HostName bastion.example.com
    ProxyJump jump.example.com

Host *.internal !secret.internal
    User internal

Host *
    User fallback
    IdentityFile "~/.ssh/id_default"
    ServerAliveInterval 30

Match host foo
    User ignored
"#;

    /// An absolute home directory on the OS running the tests (a Unix path
    /// is not absolute on Windows).
    fn home() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\me")
        } else {
            PathBuf::from("/home/me")
        }
    }

    fn by_alias(c: &[ImportCandidate], a: &str) -> ImportCandidate {
        c.iter()
            .find(|x| x.alias == a)
            .cloned()
            .unwrap_or_else(|| panic!("no {a}"))
    }

    #[test]
    fn concrete_aliases_in_order_without_wildcards() {
        let c = parse_ssh_config(CONFIG, &home(), "me");
        let aliases: Vec<_> = c.iter().map(|x| x.alias.as_str()).collect();
        assert_eq!(aliases, ["web-01", "web-02", "db", "bastion"]);
    }

    #[test]
    fn first_value_wins_like_openssh() {
        let c = parse_ssh_config(CONFIG, &home(), "me");
        let web = by_alias(&c, "web-01");
        assert_eq!(web.input.host, "web-01.example.com");
        assert_eq!(web.input.port, 2222);
        // Top-level `User root` comes first, so `Host *`'s fallback loses.
        assert_eq!(web.input.username, "root");
        assert_eq!(
            web.input.auth,
            AuthMethod::KeyFile {
                path: home().join(".ssh/id_default")
            }
        );
        assert_eq!(web.input.folder.as_deref(), Some(IMPORT_FOLDER));

        let db = by_alias(&c, "db");
        assert_eq!((db.input.host.as_str(), db.input.port), ("10.0.0.9", 22));
        assert_eq!(db.input.username, "root"); // top-level still first
        assert_eq!(
            db.input.auth,
            AuthMethod::KeyFile {
                path: home().join(".ssh/id_db")
            }
        );
    }

    #[test]
    fn unsupported_features_are_reported() {
        let c = parse_ssh_config(CONFIG, &home(), "me");
        let bastion = by_alias(&c, "bastion");
        assert!(
            bastion.warnings.iter().any(|w| w.contains("proxyjump")),
            "{:?}",
            bastion.warnings
        );
        assert!(bastion.warnings.iter().any(|w| w.contains("Match")));
        let web = by_alias(&c, "web-01");
        assert!(!web.warnings.iter().any(|w| w.contains("proxyjump")));
    }

    #[test]
    fn defaults_without_identity_use_agent_and_default_user() {
        let c = parse_ssh_config("Host plain\n  HostName plain.example.com\n", &home(), "me");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].input.username, "me");
        assert_eq!(c[0].input.auth, AuthMethod::Agent);
        assert!(c[0].warnings.is_empty());
    }

    #[test]
    fn negated_patterns_and_globs() {
        assert!(block_matches(
            &["*.internal".into(), "!secret.internal".into()],
            "a.internal"
        ));
        assert!(!block_matches(
            &["*.internal".into(), "!secret.internal".into()],
            "secret.internal"
        ));
        assert!(glob("web-??", "web-01"));
        assert!(!glob("web-??", "web-1"));
        assert!(glob("*", ""));
    }

    #[test]
    fn invalid_port_and_relative_identity_warn() {
        let c = parse_ssh_config("Host x\n Port 99999\n IdentityFile id_rsa\n", &home(), "me");
        assert_eq!(c[0].input.port, 22);
        assert_eq!(c[0].input.auth, AuthMethod::Agent);
        assert_eq!(c[0].warnings.len(), 2, "{:?}", c[0].warnings);
    }

    #[test]
    fn import_skips_existing_names_and_respects_selection() {
        let store = SessionStore::open_in_memory().unwrap();
        let c = parse_ssh_config(CONFIG, &home(), "me");
        let first = import_candidates(&store, &c, &["db".into()]).unwrap();
        assert_eq!(first.created, ["db"]);
        let all = import_candidates(&store, &c, &[]).unwrap();
        assert_eq!(all.created, ["web-01", "web-02", "bastion"]);
        assert_eq!(all.skipped, ["db"]);
        assert_eq!(store.list().unwrap().len(), 4);
    }
}
