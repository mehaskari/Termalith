//! The SQLite schema of the session store: one SQL batch per version,
//! applied in order. The schema version is the number of entries
//! (`PRAGMA user_version`). Add a migration at the end, never edit an old
//! one, and add a migration test in `sessions.rs`.

use rusqlite::Connection;

use crate::AppError;

pub(crate) const MIGRATIONS: &[&str] = &[
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
    // v5: command snippets.
    "CREATE TABLE snippets (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        command TEXT NOT NULL,
        folder TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );",
    // v6: RDP/VNC sessions.
    "ALTER TABLE sessions ADD COLUMN protocol TEXT NOT NULL DEFAULT 'ssh'
        CHECK (protocol IN ('ssh', 'rdp', 'vnc'));
    ALTER TABLE sessions ADD COLUMN domain TEXT;",
    // v7: tags (JSON array of strings).
    "ALTER TABLE sessions ADD COLUMN tags TEXT NOT NULL DEFAULT '[]';",
    // v8: agent forwarding.
    "ALTER TABLE sessions ADD COLUMN forward_agent INTEGER NOT NULL DEFAULT 0;",
    // v9: X11 forwarding.
    "ALTER TABLE sessions ADD COLUMN x11_forward INTEGER NOT NULL DEFAULT 0;",
    // v10: startup command.
    "ALTER TABLE sessions ADD COLUMN startup_command TEXT;",
    // v11: Telnet sessions. The protocol CHECK constraint cannot be altered,
    // so the table is rebuilt (foreign keys are off during migrations, so
    // dropping the old table does not clear jump links).
    "CREATE TABLE sessions_v11 (
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
        updated_at INTEGER NOT NULL,
        jump_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
        favorite INTEGER NOT NULL DEFAULT 0,
        protocol TEXT NOT NULL DEFAULT 'ssh'
            CHECK (protocol IN ('ssh', 'rdp', 'vnc', 'telnet')),
        domain TEXT,
        tags TEXT NOT NULL DEFAULT '[]',
        forward_agent INTEGER NOT NULL DEFAULT 0,
        x11_forward INTEGER NOT NULL DEFAULT 0,
        startup_command TEXT
    );
    INSERT INTO sessions_v11 SELECT id, name, host, port, username, auth_kind, key_path,
        credential_ref, folder, created_at, updated_at, jump_session_id, favorite, protocol,
        domain, tags, forward_agent, x11_forward, startup_command FROM sessions;
    DROP TABLE sessions;
    ALTER TABLE sessions_v11 RENAME TO sessions;
    CREATE INDEX sessions_name ON sessions (name COLLATE NOCASE);",
    // v12: transfer history (no file contents, no credentials).
    "CREATE TABLE transfer_history (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        direction TEXT NOT NULL CHECK (direction IN ('download', 'upload')),
        name TEXT NOT NULL,
        remote_path TEXT NOT NULL,
        local_path TEXT,
        size INTEGER,
        outcome TEXT NOT NULL CHECK (outcome IN ('done', 'failed', 'cancelled')),
        message TEXT,
        connection TEXT NOT NULL,
        finished_at INTEGER NOT NULL
    );",
    // v13: folders that exist on their own (may be empty).
    "CREATE TABLE folders (path TEXT PRIMARY KEY NOT NULL COLLATE NOCASE);",
    // v14: RDP server certificates the user trusts (SHA-256, hex).
    "CREATE TABLE rdp_certificates (
        host TEXT NOT NULL COLLATE NOCASE,
        port INTEGER NOT NULL,
        sha256 TEXT NOT NULL,
        trusted_at INTEGER NOT NULL,
        PRIMARY KEY (host, port)
    );",
    // v15: sessions that open as a file browser, without a terminal.
    "ALTER TABLE sessions ADD COLUMN files_only INTEGER NOT NULL DEFAULT 0;",
    // v16: FTP sessions. Rebuilt for the protocol CHECK constraint, as in v11.
    "CREATE TABLE sessions_v16 (
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
        updated_at INTEGER NOT NULL,
        jump_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
        favorite INTEGER NOT NULL DEFAULT 0,
        protocol TEXT NOT NULL DEFAULT 'ssh'
            CHECK (protocol IN ('ssh', 'rdp', 'vnc', 'telnet', 'ftp')),
        domain TEXT,
        tags TEXT NOT NULL DEFAULT '[]',
        forward_agent INTEGER NOT NULL DEFAULT 0,
        x11_forward INTEGER NOT NULL DEFAULT 0,
        startup_command TEXT,
        files_only INTEGER NOT NULL DEFAULT 0
    );
    INSERT INTO sessions_v16 SELECT id, name, host, port, username, auth_kind, key_path,
        credential_ref, folder, created_at, updated_at, jump_session_id, favorite, protocol,
        domain, tags, forward_agent, x11_forward, startup_command, files_only FROM sessions;
    DROP TABLE sessions;
    ALTER TABLE sessions_v16 RENAME TO sessions;
    CREATE INDEX sessions_name ON sessions (name COLLATE NOCASE);",
    // v17: FTPS sessions. Rebuilt for the protocol CHECK constraint, as in v11.
    "CREATE TABLE sessions_v17 (
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
        updated_at INTEGER NOT NULL,
        jump_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
        favorite INTEGER NOT NULL DEFAULT 0,
        protocol TEXT NOT NULL DEFAULT 'ssh'
            CHECK (protocol IN ('ssh', 'rdp', 'vnc', 'telnet', 'ftp', 'ftps')),
        domain TEXT,
        tags TEXT NOT NULL DEFAULT '[]',
        forward_agent INTEGER NOT NULL DEFAULT 0,
        x11_forward INTEGER NOT NULL DEFAULT 0,
        startup_command TEXT,
        files_only INTEGER NOT NULL DEFAULT 0
    );
    INSERT INTO sessions_v17 SELECT id, name, host, port, username, auth_kind, key_path,
        credential_ref, folder, created_at, updated_at, jump_session_id, favorite, protocol,
        domain, tags, forward_agent, x11_forward, startup_command, files_only FROM sessions;
    DROP TABLE sessions;
    ALTER TABLE sessions_v17 RENAME TO sessions;
    CREATE INDEX sessions_name ON sessions (name COLLATE NOCASE);",
    // v18: recorded macros.
    "CREATE TABLE macros (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        steps TEXT NOT NULL
    );",
    // v19: Mosh for SSH sessions.
    "ALTER TABLE sessions ADD COLUMN mosh INTEGER NOT NULL DEFAULT 0;",
    // v20: per-session terminal appearance (JSON).
    "ALTER TABLE sessions ADD COLUMN appearance TEXT;",
    // v21: icons for sessions (JSON) and folders.
    "ALTER TABLE sessions ADD COLUMN icon TEXT;
     CREATE TABLE folder_icons (
        path TEXT PRIMARY KEY NOT NULL COLLATE NOCASE,
        icon TEXT NOT NULL
     );",
    // v22: shared (team) session files; their sessions are marked read-only.
    "CREATE TABLE shared_sources (
        id TEXT PRIMARY KEY NOT NULL,
        label TEXT NOT NULL,
        location TEXT NOT NULL,
        synced_at INTEGER,
        last_error TEXT
     );
     ALTER TABLE sessions ADD COLUMN shared_source TEXT;",
];

pub(crate) fn migrate(conn: &mut Connection) -> Result<(), AppError> {
    // Table rebuilds must not trigger ON DELETE actions (SQLite's documented
    // procedure); integrity is checked before foreign keys are turned on.
    conn.pragma_update(None, "foreign_keys", false)?;
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
    let broken: i64 = conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
        r.get(0)
    })?;
    if broken > 0 {
        return Err(AppError::Db(format!(
            "{broken} broken references after migration"
        )));
    }
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}
