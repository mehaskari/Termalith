//! Saved sessions in SQLite. Secrets are never stored here: a session holds
//! at most an opaque [`CredentialRef`] into the OS credential store.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use credential_store::CredentialRef;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    fn generate() -> Result<Self, AppError> {
        let mut b = [0u8; 12];
        getrandom::fill(&mut b).map_err(|e| AppError::Db(e.to_string()))?;
        Ok(Self(format!(
            "ses-{}",
            b.iter().map(|x| format!("{x:02x}")).collect::<String>()
        )))
    }

    pub fn parse(s: &str) -> Result<Self, AppError> {
        let ok = s.strip_prefix("ses-").is_some_and(|h| {
            h.len() == 24
                && h.bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        });
        if ok {
            Ok(Self(s.to_owned()))
        } else {
            Err(AppError::invalid("session id", "malformed"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AuthMethod {
    Password,
    KeyFile {
        path: PathBuf,
    },
    /// Keys from the user's SSH agent.
    Agent,
}

/// User-editable fields of a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInput {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    pub folder: Option<String>,
    /// Saved session to tunnel through (OpenSSH `ProxyJump`).
    #[serde(default)]
    pub jump_via: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: SessionId,
    #[serde(flatten)]
    pub input: SessionInput,
    /// Whether a password/passphrase is saved in the OS credential store.
    pub has_saved_secret: bool,
    pub favorite: bool,
    #[serde(skip)]
    pub credential: Option<CredentialRef>,
    pub updated_at: i64,
}

fn validate_text(field: &'static str, v: &str, max: usize) -> Result<(), AppError> {
    if v.trim().is_empty() {
        return Err(AppError::invalid(field, "must not be empty"));
    }
    if v.chars().count() > max {
        return Err(AppError::invalid(
            field,
            format!("longer than {max} characters"),
        ));
    }
    if v.chars().any(char::is_control) {
        return Err(AppError::invalid(field, "contains control characters"));
    }
    Ok(())
}

impl SessionInput {
    pub fn validate(&self) -> Result<(), AppError> {
        validate_text("name", &self.name, 200)?;
        validate_text("host", &self.host, 253)?;
        if self.host.chars().any(char::is_whitespace) || self.host.starts_with('-') {
            return Err(AppError::invalid(
                "host",
                "not a valid host name or address",
            ));
        }
        validate_text("username", &self.username, 128)?;
        if self.username.chars().any(char::is_whitespace) {
            return Err(AppError::invalid("username", "must not contain spaces"));
        }
        if self.port == 0 {
            return Err(AppError::invalid("port", "must be 1-65535"));
        }
        if let Some(f) = &self.folder {
            validate_text("folder", f, 200)?;
        }
        if let AuthMethod::KeyFile { path } = &self.auth
            && (path.as_os_str().is_empty() || !path.is_absolute())
        {
            return Err(AppError::invalid("key file", "must be an absolute path"));
        }
        Ok(())
    }
}

/// Longest allowed ProxyJump chain.
pub const MAX_JUMP_DEPTH: usize = 4;

const MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE sessions (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        host TEXT NOT NULL,
        port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
        username TEXT NOT NULL,
        auth_kind TEXT NOT NULL CHECK (auth_kind IN ('password', 'key_file')),
        key_path TEXT,
        credential_ref TEXT,
        folder TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE INDEX sessions_name ON sessions (name COLLATE NOCASE);",
    // v2: allow 'agent' auth. SQLite cannot alter a CHECK constraint, so the
    // table is rebuilt (inside the migration transaction).
    "CREATE TABLE sessions_v2 (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        host TEXT NOT NULL,
        port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
        username TEXT NOT NULL,
        auth_kind TEXT NOT NULL CHECK (auth_kind IN ('password', 'key_file', 'agent')),
        key_path TEXT,
        credential_ref TEXT,
        folder TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    INSERT INTO sessions_v2 SELECT id, name, host, port, username, auth_kind, key_path,
        credential_ref, folder, created_at, updated_at FROM sessions;
    DROP TABLE sessions;
    ALTER TABLE sessions_v2 RENAME TO sessions;
    CREATE INDEX sessions_name ON sessions (name COLLATE NOCASE);",
    // v3: jump host (ProxyJump). Deleting the jump session clears the link.
    "ALTER TABLE sessions ADD COLUMN jump_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL;",
    // v4: favorites and application settings.
    "ALTER TABLE sessions ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0;
    CREATE TABLE settings (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);",
];

pub struct SessionStore {
    conn: Mutex<Connection>,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn migrate(conn: &mut Connection) -> Result<(), AppError> {
    conn.pragma_update(None, "foreign_keys", true)?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let target = MIGRATIONS.len() as i64;
    if version > target {
        return Err(AppError::Db(format!(
            "database schema v{version} is newer than this build (v{target})"
        )));
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

impl SessionStore {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| AppError::Db(e.to_string()))?;
        }
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self, AppError> {
        let mut conn = Connection::open_in_memory()?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub(crate) fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn list(&self) -> Result<Vec<SessionRecord>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, host, port, username, auth_kind, key_path, credential_ref, folder, updated_at, jump_session_id, favorite
             FROM sessions ORDER BY folder IS NOT NULL, folder COLLATE NOCASE, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], row_to_record)?;
        rows.map(|r| r.map_err(AppError::from).and_then(|x| x))
            .collect()
    }

    pub fn get(&self, id: &SessionId) -> Result<SessionRecord, AppError> {
        let conn = self.conn();
        conn.query_row(
            "SELECT id, name, host, port, username, auth_kind, key_path, credential_ref, folder, updated_at, jump_session_id, favorite
             FROM sessions WHERE id = ?1",
            [id.as_str()],
            row_to_record,
        )
        .optional()?
        .ok_or(AppError::NotFound)?
    }

    pub fn create(&self, input: &SessionInput) -> Result<SessionRecord, AppError> {
        input.validate()?;
        let id = SessionId::generate()?;
        self.check_jump(&id, input.jump_via.as_ref())?;
        let (kind, key) = auth_columns(&input.auth);
        let t = now();
        self.conn().execute(
            "INSERT INTO sessions (id, name, host, port, username, auth_kind, key_path, folder, created_at, updated_at, jump_session_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10)",
            params![
                id.as_str(),
                input.name.trim(),
                input.host.trim(),
                input.port,
                input.username,
                kind,
                key,
                input.folder,
                t,
                input.jump_via.as_ref().map(SessionId::as_str)
            ],
        )?;
        self.get(&id)
    }

    /// A jump host must exist, must not be the session itself, and the chain
    /// must not loop (or get absurdly long).
    fn check_jump(&self, id: &SessionId, jump: Option<&SessionId>) -> Result<(), AppError> {
        let mut next = jump.cloned();
        let mut depth = 0;
        while let Some(j) = next {
            if &j == id {
                return Err(AppError::invalid("jump host", "would create a loop"));
            }
            depth += 1;
            if depth > MAX_JUMP_DEPTH {
                return Err(AppError::invalid("jump host", "chain is too long"));
            }
            next = match self.get(&j) {
                Ok(r) => r.input.jump_via,
                Err(AppError::NotFound) => {
                    return Err(AppError::invalid(
                        "jump host",
                        "the selected session does not exist",
                    ));
                }
                Err(e) => return Err(e),
            };
        }
        Ok(())
    }

    /// Updates editable fields. Returns the credential that is no longer
    /// applicable (auth method changed), which the caller should delete.
    pub fn update(
        &self,
        id: &SessionId,
        input: &SessionInput,
    ) -> Result<(SessionRecord, Option<CredentialRef>), AppError> {
        input.validate()?;
        let old = self.get(id)?;
        self.check_jump(id, input.jump_via.as_ref())?;
        let auth_changed =
            std::mem::discriminant(&old.input.auth) != std::mem::discriminant(&input.auth);
        let (kind, key) = auth_columns(&input.auth);
        let stale = if auth_changed {
            old.credential.clone()
        } else {
            None
        };
        let cred = if auth_changed {
            None
        } else {
            old.credential.as_ref().map(|c| c.as_str().to_owned())
        };
        self.conn().execute(
            "UPDATE sessions SET name = ?2, host = ?3, port = ?4, username = ?5, auth_kind = ?6,
             key_path = ?7, folder = ?8, credential_ref = ?9, updated_at = ?10, jump_session_id = ?11
             WHERE id = ?1",
            params![
                id.as_str(),
                input.name.trim(),
                input.host.trim(),
                input.port,
                input.username,
                kind,
                key,
                input.folder,
                cred,
                now(),
                input.jump_via.as_ref().map(SessionId::as_str)
            ],
        )?;
        Ok((self.get(id)?, stale))
    }

    pub fn set_credential(
        &self,
        id: &SessionId,
        cred: Option<&CredentialRef>,
    ) -> Result<(), AppError> {
        let n = self.conn().execute(
            "UPDATE sessions SET credential_ref = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.as_str(), cred.map(|c| c.as_str()), now()],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    pub fn set_favorite(&self, id: &SessionId, favorite: bool) -> Result<(), AppError> {
        let n = self.conn().execute(
            "UPDATE sessions SET favorite = ?2 WHERE id = ?1",
            params![id.as_str(), i64::from(favorite)],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    /// Deletes the session and returns its credential reference (if any) so
    /// the caller can remove the secret.
    pub fn delete(&self, id: &SessionId) -> Result<Option<CredentialRef>, AppError> {
        let old = self.get(id)?;
        self.conn()
            .execute("DELETE FROM sessions WHERE id = ?1", [id.as_str()])?;
        Ok(old.credential)
    }
}

fn auth_columns(a: &AuthMethod) -> (&'static str, Option<String>) {
    match a {
        AuthMethod::Password => ("password", None),
        AuthMethod::KeyFile { path } => ("key_file", Some(path.to_string_lossy().into_owned())),
        AuthMethod::Agent => ("agent", None),
    }
}

fn row_to_record(r: &rusqlite::Row<'_>) -> rusqlite::Result<Result<SessionRecord, AppError>> {
    let id: String = r.get(0)?;
    let kind: String = r.get(5)?;
    let key: Option<String> = r.get(6)?;
    let cred: Option<String> = r.get(7)?;
    let auth = match (kind.as_str(), key) {
        ("password", _) => AuthMethod::Password,
        ("key_file", Some(p)) => AuthMethod::KeyFile { path: p.into() },
        ("agent", _) => AuthMethod::Agent,
        _ => return Ok(Err(AppError::Db(format!("session {id}: bad auth columns")))),
    };
    let credential = match cred.as_deref().map(CredentialRef::parse).transpose() {
        Ok(c) => c,
        Err(_) => {
            return Ok(Err(AppError::Db(format!(
                "session {id}: bad credential ref"
            ))));
        }
    };
    Ok(Ok(SessionRecord {
        has_saved_secret: credential.is_some(),
        favorite: r.get::<_, i64>(11)? != 0,
        credential,
        id: SessionId(id),
        input: SessionInput {
            name: r.get(1)?,
            host: r.get(2)?,
            port: r.get(3)?,
            username: r.get(4)?,
            auth,
            folder: r.get(8)?,
            jump_via: r.get::<_, Option<String>>(10)?.map(SessionId),
        },
        updated_at: r.get(9)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> SessionInput {
        SessionInput {
            name: "web-01".into(),
            host: "10.0.0.5".into(),
            port: 22,
            username: "admin".into(),
            auth: AuthMethod::Password,
            folder: Some("Production".into()),
            jump_via: None,
        }
    }

    #[test]
    fn crud_round_trip() {
        let s = SessionStore::open_in_memory().unwrap();
        let rec = s.create(&input()).unwrap();
        assert_eq!(s.list().unwrap(), vec![rec.clone()]);
        assert!(!rec.has_saved_secret);

        let mut changed = input();
        changed.name = "web-01 (renamed)".into();
        let (upd, stale) = s.update(&rec.id, &changed).unwrap();
        assert_eq!(upd.input.name, "web-01 (renamed)");
        assert!(stale.is_none());

        assert!(s.delete(&rec.id).unwrap().is_none());
        assert!(matches!(s.get(&rec.id), Err(AppError::NotFound)));
    }

    #[test]
    fn credential_ref_is_stored_not_the_secret() {
        let s = SessionStore::open_in_memory().unwrap();
        let rec = s.create(&input()).unwrap();
        let cred = CredentialRef::generate().unwrap();
        s.set_credential(&rec.id, Some(&cred)).unwrap();
        let got = s.get(&rec.id).unwrap();
        assert!(got.has_saved_secret);
        assert_eq!(got.credential.as_ref(), Some(&cred));
        // Serialized form (what the UI sees) never exposes the reference.
        let json = serde_json::to_string(&got).unwrap();
        assert!(!json.contains(cred.as_str()), "{json}");
        assert!(json.contains("\"hasSavedSecret\":true"), "{json}");
        assert_eq!(s.delete(&rec.id).unwrap(), Some(cred));
    }

    #[test]
    fn changing_auth_method_returns_stale_credential() {
        let s = SessionStore::open_in_memory().unwrap();
        let rec = s.create(&input()).unwrap();
        let cred = CredentialRef::generate().unwrap();
        s.set_credential(&rec.id, Some(&cred)).unwrap();
        let mut k = input();
        k.auth = AuthMethod::KeyFile {
            path: abs("id_ed25519"),
        };
        let (upd, stale) = s.update(&rec.id, &k).unwrap();
        assert_eq!(stale, Some(cred));
        assert!(!upd.has_saved_secret);
    }

    fn abs(name: &str) -> PathBuf {
        std::env::temp_dir().join(name)
    }

    #[test]
    fn validation_rejects_bad_input() {
        let s = SessionStore::open_in_memory().unwrap();
        type Mutation = fn(&mut SessionInput);
        let cases: Vec<(Mutation, &str)> = vec![
            (|i| i.name = "  ".into(), "name"),
            (|i| i.host = "bad host".into(), "host"),
            (|i| i.host = "-oProxyCommand=x".into(), "host"),
            (|i| i.username = "a b".into(), "username"),
            (|i| i.port = 0, "port"),
            (
                |i| {
                    i.auth = AuthMethod::KeyFile {
                        path: "relative/key".into(),
                    }
                },
                "key file",
            ),
            (|i| i.name = "x\u{7}".into(), "name"),
        ];
        for (mutate, field) in cases {
            let mut i = input();
            mutate(&mut i);
            match s.create(&i) {
                Err(AppError::Invalid { field: f, .. }) => assert_eq!(f, field),
                other => panic!("expected invalid {field}, got {other:?}"),
            }
        }
        assert!(s.list().unwrap().is_empty());
    }

    #[test]
    fn persists_across_reopen_and_rejects_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("termalith.db");
        let id = SessionStore::open(&path)
            .unwrap()
            .create(&input())
            .unwrap()
            .id;
        let reopened = SessionStore::open(&path).unwrap();
        assert_eq!(reopened.get(&id).unwrap().input, input());
        drop(reopened);

        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        drop(conn);
        assert!(matches!(SessionStore::open(&path), Err(AppError::Db(_))));
    }

    #[test]
    fn v1_database_is_migrated_and_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, name, host, port, username, auth_kind, created_at, updated_at)
                 VALUES ('ses-0123456789abcdef01234567', 'old', 'h', 22, 'u', 'password', 1, 1)",
                [],
            )
            .unwrap();
        }
        let s = SessionStore::open(&path).unwrap();
        let old = s
            .get(&SessionId::parse("ses-0123456789abcdef01234567").unwrap())
            .unwrap();
        assert_eq!(old.input.name, "old");
        let mut i = input();
        i.auth = AuthMethod::Agent;
        let rec = s.create(&i).unwrap();
        assert_eq!(s.get(&rec.id).unwrap().input.auth, AuthMethod::Agent);
    }

    #[test]
    fn jump_host_links_are_validated_and_cleared_on_delete() {
        let s = SessionStore::open_in_memory().unwrap();
        let bastion = s.create(&input()).unwrap();
        let mut target = input();
        target.name = "behind bastion".into();
        target.jump_via = Some(bastion.id.clone());
        let t = s.create(&target).unwrap();
        assert_eq!(
            s.get(&t.id).unwrap().input.jump_via,
            Some(bastion.id.clone())
        );

        // Loop: bastion -> target -> bastion.
        let mut looping = input();
        looping.jump_via = Some(t.id.clone());
        assert!(matches!(
            s.update(&bastion.id, &looping),
            Err(AppError::Invalid {
                field: "jump host",
                ..
            })
        ));
        // Self reference.
        let mut own = target.clone();
        own.jump_via = Some(t.id.clone());
        assert!(matches!(
            s.update(&t.id, &own),
            Err(AppError::Invalid { .. })
        ));
        // Unknown session.
        let mut ghost = input();
        ghost.jump_via = Some(SessionId::parse("ses-000000000000000000000000").unwrap());
        assert!(matches!(s.create(&ghost), Err(AppError::Invalid { .. })));

        // Deleting the bastion clears the link (ON DELETE SET NULL).
        s.delete(&bastion.id).unwrap();
        assert_eq!(s.get(&t.id).unwrap().input.jump_via, None);
    }

    #[test]
    fn session_id_parse() {
        assert!(SessionId::parse("ses-0123456789abcdef01234567").is_ok());
        for bad in [
            "",
            "ses-",
            "ses-XYZ",
            "abc-0123456789abcdef01234567",
            "ses-0123456789ABCDEF01234567",
        ] {
            assert!(SessionId::parse(bad).is_err(), "{bad}");
        }
    }
}
