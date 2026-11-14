//! Shared (team) session files: a Termalith session export on a network
//! share or an HTTPS address. Its sessions appear in the list under a
//! folder named after the source, read-only, and are refreshed from the
//! file. Saved passwords and favorites stay local and survive a refresh.
//! Export files never contain passwords.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::AppError;
use crate::folders::normalize_folder;
use crate::sessions::{
    Icon, SessionId, SessionStore, appearance_json, auth_columns, now, tags_json,
};
use crate::settings::{MAX_IMPORT_BYTES, parse_export};

/// Most shared sources.
const MAX_SOURCES: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedSource {
    pub id: String,
    /// Shown as the top folder of its sessions.
    pub label: String,
    /// A file path or an `https://` address.
    pub location: String,
    pub synced_at: Option<i64>,
    pub last_error: Option<String>,
    pub sessions: usize,
}

/// Where a source is read from.
pub enum Location {
    File(PathBuf),
    Https(String),
}

impl Location {
    fn as_text(&self) -> String {
        match self {
            Location::File(p) => p.display().to_string(),
            Location::Https(u) => u.clone(),
        }
    }

    fn parse(text: &str) -> Location {
        if text.starts_with("https://") {
            Location::Https(text.to_owned())
        } else {
            Location::File(PathBuf::from(text))
        }
    }
}

/// Checks a typed address: `https://` only (no plain HTTP: the file would
/// travel unprotected and could be changed on the way).
pub fn check_url(url: &str) -> Result<(), AppError> {
    let url = url.trim();
    if !url.starts_with("https://") || url.len() > 2048 || url.chars().any(char::is_whitespace) {
        return Err(AppError::invalid("address", "use an https:// address"));
    }
    Ok(())
}

fn check_label(label: &str) -> Result<String, AppError> {
    let label = normalize_folder(label)
        .map_err(|_| AppError::invalid("name", "use a short folder-style name"))?;
    if label.contains('/') {
        return Err(AppError::invalid("name", "no '/' in the name"));
    }
    Ok(label)
}

impl SessionStore {
    pub fn shared_sources(&self) -> Result<Vec<SharedSource>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.label, s.location, s.synced_at, s.last_error,
                    (SELECT count(*) FROM sessions WHERE shared_source = s.id)
             FROM shared_sources s ORDER BY s.label COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(SharedSource {
                id: r.get(0)?,
                label: r.get(1)?,
                location: r.get(2)?,
                synced_at: r.get(3)?,
                last_error: r.get(4)?,
                sessions: r.get::<_, i64>(5)? as usize,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn add_shared_source(&self, label: &str, location: Location) -> Result<String, AppError> {
        let label = check_label(label)?;
        if let Location::Https(u) = &location {
            check_url(u)?;
        }
        if self.shared_sources()?.len() >= MAX_SOURCES {
            return Err(AppError::invalid("shared sessions", "at most 10 sources"));
        }
        let id = SessionId::generate()?.as_str().to_owned();
        self.conn().execute(
            "INSERT INTO shared_sources (id, label, location) VALUES (?1, ?2, ?3)",
            params![id, label, location.as_text()],
        )?;
        Ok(id)
    }

    /// Removes a source and its sessions; returns their credentials so the
    /// caller can delete saved passwords.
    pub fn remove_shared_source(
        &self,
        id: &str,
    ) -> Result<Vec<credential_store::CredentialRef>, AppError> {
        let creds: Vec<String> = {
            let conn = self.conn();
            let mut stmt = conn.prepare(
                "SELECT credential_ref FROM sessions WHERE shared_source = ?1 AND credential_ref IS NOT NULL",
            )?;
            let rows = stmt.query_map([id], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM sessions WHERE shared_source = ?1", [id])?;
        let n = tx.execute("DELETE FROM shared_sources WHERE id = ?1", [id])?;
        tx.commit()?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        Ok(creds
            .iter()
            .filter_map(|c| credential_store::CredentialRef::parse(c).ok())
            .collect())
    }

    fn source(&self, id: &str) -> Result<(String, Location), AppError> {
        self.conn()
            .query_row(
                "SELECT label, location FROM shared_sources WHERE id = ?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
            .map(|(l, loc)| (l, Location::parse(&loc)))
            .ok_or(AppError::NotFound)
    }

    /// Replaces a source's sessions with the file's. Sessions keep their id
    /// (matched by name), so saved passwords and favorites stay.
    pub fn apply_shared(&self, id: &str, json: &str) -> Result<usize, AppError> {
        let (label, _) = self.source(id)?;
        let mut entries = parse_export(json)?;
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let existing: Vec<(String, String)> = {
            let mut stmt = tx.prepare("SELECT id, name FROM sessions WHERE shared_source = ?1")?;
            let rows = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let mut kept: Vec<(String, String)> = Vec::new();
        let t = now();
        for (input, _) in &mut entries {
            input.folder = Some(match input.folder.as_deref().map(normalize_folder) {
                Some(Ok(f)) if !f.is_empty() => format!("{label}/{f}"),
                _ => label.clone(),
            });
            input.jump_via = None;
            input.icon = input.icon.take().filter(|i: &Icon| i.validate().is_ok());
            if input.validate().is_err()
                || kept
                    .iter()
                    .any(|(_, n)| n.eq_ignore_ascii_case(&input.name))
            {
                continue;
            }
            let (kind, key) = auth_columns(&input.auth);
            let sid = match existing
                .iter()
                .find(|(_, n)| n.eq_ignore_ascii_case(&input.name))
            {
                Some((sid, _)) => {
                    tx.execute(
                        "UPDATE sessions SET name = ?2, host = ?3, port = ?4, username = ?5, auth_kind = ?6,
                         key_path = ?7, folder = ?8, updated_at = ?9, jump_session_id = NULL, protocol = ?10,
                         domain = ?11, tags = ?12, forward_agent = ?13, x11_forward = ?14,
                         startup_command = ?15, files_only = ?16, mosh = ?17, appearance = ?18, icon = ?19
                         WHERE id = ?1",
                        params![
                            sid, input.name.trim(), input.host.trim(), input.port, input.username,
                            kind, key, input.folder, t, input.protocol.column(), input.domain,
                            tags_json(&input.tags), input.forward_agent, input.x11_forward,
                            input.startup_command, input.files_only, input.mosh,
                            appearance_json(&input.appearance), Icon::to_json(&input.icon)
                        ],
                    )?;
                    sid.clone()
                }
                None => {
                    let sid = SessionId::generate()?.as_str().to_owned();
                    tx.execute(
                        "INSERT INTO sessions (id, name, host, port, username, auth_kind, key_path, folder,
                         created_at, updated_at, protocol, domain, tags, forward_agent, x11_forward,
                         startup_command, files_only, mosh, appearance, icon, shared_source)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
                        params![
                            sid, input.name.trim(), input.host.trim(), input.port, input.username,
                            kind, key, input.folder, t, input.protocol.column(), input.domain,
                            tags_json(&input.tags), input.forward_agent, input.x11_forward,
                            input.startup_command, input.files_only, input.mosh,
                            appearance_json(&input.appearance), Icon::to_json(&input.icon), id
                        ],
                    )?;
                    sid
                }
            };
            kept.push((sid, input.name.clone()));
        }
        // Jump hosts by name, within the same file.
        for (input, jump) in &entries {
            let Some(jump) = jump else { continue };
            let me = kept
                .iter()
                .find(|(_, n)| n.eq_ignore_ascii_case(&input.name));
            let via = kept.iter().find(|(_, n)| n.eq_ignore_ascii_case(jump));
            if let (Some((me, _)), Some((via, _))) = (me, via)
                && me != via
            {
                tx.execute(
                    "UPDATE sessions SET jump_session_id = ?2 WHERE id = ?1",
                    params![me, via],
                )?;
            }
        }
        for (sid, _) in &existing {
            if !kept.iter().any(|(k, _)| k == sid) {
                tx.execute("DELETE FROM sessions WHERE id = ?1", [sid])?;
            }
        }
        tx.execute(
            "UPDATE shared_sources SET synced_at = ?2, last_error = NULL WHERE id = ?1",
            params![id, t],
        )?;
        tx.commit()?;
        Ok(kept.len())
    }

    fn record_error(&self, id: &str, error: &str) {
        let _ = self.conn().execute(
            "UPDATE shared_sources SET last_error = ?2 WHERE id = ?1",
            params![id, error],
        );
    }

    /// Reads every source and applies it (blocking: run off the UI thread).
    /// A source that cannot be read keeps its last sessions.
    pub fn sync_shared(&self, user_agent: &str) -> Vec<(String, Result<usize, String>)> {
        let sources = self.shared_sources().unwrap_or_default();
        sources
            .into_iter()
            .map(|s| {
                let result = read_location(&Location::parse(&s.location), user_agent)
                    .and_then(|json| self.apply_shared(&s.id, &json))
                    .map_err(|e| e.to_string());
                if let Err(e) = &result {
                    self.record_error(&s.id, e);
                }
                (s.label, result)
            })
            .collect()
    }
}

fn read_location(location: &Location, user_agent: &str) -> Result<String, AppError> {
    match location {
        Location::File(path) => {
            let meta = std::fs::metadata(path)
                .map_err(|e| AppError::Network(format!("cannot read {}: {e}", path.display())))?;
            if meta.len() > MAX_IMPORT_BYTES as u64 {
                return Err(AppError::invalid("session file", "file is too large"));
            }
            std::fs::read_to_string(path)
                .map_err(|e| AppError::Network(format!("cannot read {}: {e}", path.display())))
        }
        Location::Https(url) => {
            check_url(url)?;
            use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
            let tls = TlsConfig::builder()
                .provider(TlsProvider::Rustls)
                .root_certs(RootCerts::WebPki)
                .unversioned_rustls_crypto_provider(Arc::new(
                    rustls::crypto::aws_lc_rs::default_provider(),
                ))
                .build();
            let agent = ureq::Agent::config_builder()
                .tls_config(tls)
                .timeout_global(Some(Duration::from_secs(15)))
                .user_agent(user_agent)
                .build()
                .new_agent();
            let net = |e: ureq::Error| AppError::Network(format!("could not read {url}: {e}"));
            let mut resp = agent.get(url).call().map_err(net)?;
            resp.body_mut()
                .with_config()
                .limit(MAX_IMPORT_BYTES as u64)
                .read_to_string()
                .map_err(net)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(sessions: &str) -> String {
        format!(r#"{{"format":"termalith-sessions","version":1,"sessions":[{sessions}]}}"#)
    }

    const WEB: &str = r#"{"name":"web-01","host":"10.0.0.5","port":22,"username":"deploy","auth":{"kind":"agent"},"folder":"Prod"}"#;
    const JUMP: &str = r#"{"name":"bastion","host":"10.0.0.1","port":22,"username":"me","auth":{"kind":"agent"},"folder":null}"#;

    #[test]
    fn shared_sessions_sync_keep_ids_and_are_read_only() {
        let store = SessionStore::open_in_memory().unwrap();
        let src = store
            .add_shared_source("Team", Location::Https("https://example.com/s.json".into()))
            .unwrap();
        let via = WEB.replace(
            r#""folder":"Prod"}"#,
            r#""folder":"Prod","jumpVia":"bastion"}"#,
        );
        assert_eq!(
            store
                .apply_shared(&src, &file(&format!("{via},{JUMP}")))
                .unwrap(),
            2
        );
        let all = store.list().unwrap();
        let web = all
            .iter()
            .find(|s| s.input.name == "web-01")
            .unwrap()
            .clone();
        let bastion = all
            .iter()
            .find(|s| s.input.name == "bastion")
            .unwrap()
            .clone();
        assert_eq!(web.input.folder.as_deref(), Some("Team/Prod"));
        assert_eq!(bastion.input.folder.as_deref(), Some("Team"));
        assert_eq!(web.input.jump_via.as_ref(), Some(&bastion.id));
        assert_eq!(web.shared.as_deref(), Some(src.as_str()));
        // Read-only here.
        assert!(store.update(&web.id, &web.input).is_err());
        assert!(store.delete(&web.id).is_err());
        // A refresh keeps the id, changes fields and drops removed sessions.
        let changed = WEB.replace("10.0.0.5", "10.0.0.6");
        assert_eq!(store.apply_shared(&src, &file(&changed)).unwrap(), 1);
        let again = store.list().unwrap();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].id, web.id);
        assert_eq!(again[0].input.host, "10.0.0.6");
        assert_eq!(store.shared_sources().unwrap()[0].sessions, 1);
        // A bad file changes nothing.
        assert!(store.apply_shared(&src, "{nope").is_err());
        assert_eq!(store.list().unwrap().len(), 1);
        store.remove_shared_source(&src).unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn only_https_addresses_and_simple_names() {
        assert!(check_url("https://intranet.example/sessions.json").is_ok());
        assert!(check_url("http://intranet.example/sessions.json").is_err());
        assert!(check_url("file:///etc/passwd").is_err());
        let store = SessionStore::open_in_memory().unwrap();
        assert!(
            store
                .add_shared_source("a/b", Location::File("/x".into()))
                .is_err()
        );
        assert!(
            store
                .add_shared_source("", Location::File("/x".into()))
                .is_err()
        );
    }
}
