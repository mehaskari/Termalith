//! Session folders. A session's folder is a `/`-separated path such as
//! `Production/DB`; folders are also stored on their own so an empty one can
//! exist, be renamed, moved and deleted like in a file manager.

use rusqlite::params;

use crate::AppError;
use crate::sessions::{SessionId, SessionStore};

pub const MAX_FOLDER_LEN: usize = 200;
pub const MAX_FOLDER_DEPTH: usize = 10;

/// `" Production / DB "` → `"Production/DB"`. Errors on an empty path, a
/// control character, or a path that is too long or too deep.
pub fn normalize_folder(path: &str) -> Result<String, AppError> {
    if path.chars().any(char::is_control) {
        return Err(AppError::invalid("folder", "contains control characters"));
    }
    let parts: Vec<&str> = path
        .split('/')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(AppError::invalid("folder", "must not be empty"));
    }
    if parts.len() > MAX_FOLDER_DEPTH {
        return Err(AppError::invalid(
            "folder",
            format!("at most {MAX_FOLDER_DEPTH} levels"),
        ));
    }
    let out = parts.join("/");
    if out.chars().count() > MAX_FOLDER_LEN {
        return Err(AppError::invalid(
            "folder",
            format!("longer than {MAX_FOLDER_LEN} characters"),
        ));
    }
    Ok(out)
}

/// The path of `path` after moving `from` to `to`, if `path` is `from` or
/// inside it (case-insensitive, like the tree).
fn moved(path: &str, from: &str, to: &str) -> Option<String> {
    let lower = path.to_lowercase();
    let from_l = from.to_lowercase();
    if lower == from_l {
        Some(to.to_owned())
    } else if lower.starts_with(&format!("{from_l}/")) {
        Some(format!("{to}{}", &path[from.len()..]))
    } else {
        None
    }
}

fn inside(path: &str, folder: &str) -> bool {
    moved(path, folder, "").is_some()
}

impl SessionStore {
    /// Stored folders (not those that exist only through a session).
    pub fn folders(&self) -> Result<Vec<String>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare("SELECT path FROM folders ORDER BY path COLLATE NOCASE")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Creates a folder (parents are implied). Returns the normalized path.
    pub fn create_folder(&self, path: &str) -> Result<String, AppError> {
        let path = normalize_folder(path)?;
        self.conn().execute(
            "INSERT OR IGNORE INTO folders (path) VALUES (?1)",
            params![path],
        )?;
        Ok(path)
    }

    fn session_folders(conn: &rusqlite::Connection) -> Result<Vec<(String, String)>, AppError> {
        let mut stmt = conn.prepare("SELECT id, folder FROM sessions WHERE folder IS NOT NULL")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Renames or moves a folder with its subfolders and sessions. Moving a
    /// folder into itself is refused. Returns the new normalized path.
    pub fn rename_folder(&self, from: &str, to: &str) -> Result<String, AppError> {
        let from = normalize_folder(from)?;
        let to = normalize_folder(to)?;
        if from.to_lowercase() != to.to_lowercase() && inside(&to, &from) {
            return Err(AppError::invalid(
                "folder",
                "cannot move a folder into itself",
            ));
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut changed = false;
        for (id, folder) in Self::session_folders(&tx)? {
            let Ok(norm) = normalize_folder(&folder) else {
                continue;
            };
            if let Some(new) = moved(&norm, &from, &to) {
                tx.execute(
                    "UPDATE sessions SET folder = ?2 WHERE id = ?1",
                    params![id, new],
                )?;
                changed = true;
            }
        }
        let stored: Vec<String> = {
            let mut stmt = tx.prepare("SELECT path FROM folders")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for path in stored {
            if let Some(new) = moved(&path, &from, &to) {
                tx.execute("DELETE FROM folders WHERE path = ?1", params![path])?;
                tx.execute(
                    "INSERT OR IGNORE INTO folders (path) VALUES (?1)",
                    params![new],
                )?;
                changed = true;
            }
        }
        if !changed {
            return Err(AppError::NotFound);
        }
        // Keep the folder even if it only existed through its sessions.
        tx.execute(
            "INSERT OR IGNORE INTO folders (path) VALUES (?1)",
            params![to],
        )?;
        tx.commit()?;
        Ok(to)
    }

    /// Deletes a folder and its subfolders. Refused while any session is
    /// inside, so nothing is deleted by accident.
    pub fn delete_folder(&self, path: &str) -> Result<(), AppError> {
        let path = normalize_folder(path)?;
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let used = Self::session_folders(&tx)?
            .into_iter()
            .any(|(_, f)| normalize_folder(&f).is_ok_and(|n| inside(&n, &path)));
        if used {
            return Err(AppError::invalid(
                "folder",
                "move or delete the sessions in it first",
            ));
        }
        let stored: Vec<String> = {
            let mut stmt = tx.prepare("SELECT path FROM folders")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for p in stored.into_iter().filter(|p| inside(p, &path)) {
            tx.execute("DELETE FROM folders WHERE path = ?1", params![p])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Moves a session into a folder (`None`: top level).
    pub fn move_session(&self, id: &SessionId, folder: Option<&str>) -> Result<(), AppError> {
        let folder = folder.map(normalize_folder).transpose()?;
        let n = self.conn().execute(
            "UPDATE sessions SET folder = ?2 WHERE id = ?1",
            params![id.as_str(), folder],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{AuthMethod, SessionInput};

    fn session(store: &SessionStore, name: &str, folder: Option<&str>) -> SessionId {
        store
            .create(&SessionInput {
                name: name.into(),
                host: "example.com".into(),
                port: 22,
                username: "me".into(),
                auth: AuthMethod::Agent,
                folder: folder.map(Into::into),
                jump_via: None,
                protocol: Default::default(),
                domain: None,
                tags: Vec::new(),
                forward_agent: false,
                x11_forward: false,
                startup_command: None,
                files_only: false,
                mosh: false,
                appearance: None,
            })
            .unwrap()
            .id
    }

    fn folder_of(store: &SessionStore, id: &SessionId) -> Option<String> {
        store.get(id).unwrap().input.folder
    }

    #[test]
    fn normalizes_paths() {
        assert_eq!(
            normalize_folder(" Production / DB ").unwrap(),
            "Production/DB"
        );
        assert_eq!(normalize_folder("a//b/").unwrap(), "a/b");
        assert!(normalize_folder(" / ").is_err());
        assert!(normalize_folder("a\nb").is_err());
        assert!(normalize_folder(&"x/".repeat(11)).is_err());
        assert!(normalize_folder(&"x".repeat(201)).is_err());
    }

    #[test]
    fn empty_folders_can_be_created_and_deleted() {
        let s = SessionStore::open_in_memory().unwrap();
        assert_eq!(s.create_folder("Lab / Switches").unwrap(), "Lab/Switches");
        s.create_folder("lab/switches").unwrap();
        s.create_folder("Lab/Switches/Core").unwrap();
        assert_eq!(s.folders().unwrap(), ["Lab/Switches", "Lab/Switches/Core"]);
        s.delete_folder("Lab/Switches").unwrap();
        assert!(s.folders().unwrap().is_empty());
    }

    #[test]
    fn deleting_a_folder_with_sessions_is_refused() {
        let s = SessionStore::open_in_memory().unwrap();
        s.create_folder("Prod").unwrap();
        session(&s, "db", Some("Prod / DB"));
        assert!(matches!(
            s.delete_folder("prod"),
            Err(AppError::Invalid { .. })
        ));
        assert_eq!(s.folders().unwrap(), ["Prod"]);
    }

    #[test]
    fn renaming_moves_subfolders_and_sessions() {
        let s = SessionStore::open_in_memory().unwrap();
        let a = session(&s, "a", Some("Prod"));
        let b = session(&s, "b", Some("prod / DB"));
        let c = session(&s, "c", Some("Production"));
        s.create_folder("Prod/DB/Replicas").unwrap();
        assert_eq!(
            s.rename_folder("Prod", "Clients/Acme").unwrap(),
            "Clients/Acme"
        );
        assert_eq!(folder_of(&s, &a).as_deref(), Some("Clients/Acme"));
        assert_eq!(folder_of(&s, &b).as_deref(), Some("Clients/Acme/DB"));
        assert_eq!(
            folder_of(&s, &c).as_deref(),
            Some("Production"),
            "prefix must end at /"
        );
        assert_eq!(
            s.folders().unwrap(),
            ["Clients/Acme", "Clients/Acme/DB/Replicas"]
        );
    }

    #[test]
    fn a_folder_cannot_move_into_itself() {
        let s = SessionStore::open_in_memory().unwrap();
        s.create_folder("A").unwrap();
        assert!(s.rename_folder("A", "A/B").is_err());
        assert!(matches!(
            s.rename_folder("missing", "x"),
            Err(AppError::NotFound)
        ));
        // Changing only the case is a rename, not a move into itself.
        assert_eq!(s.rename_folder("A", "a").unwrap(), "a");
        assert_eq!(s.folders().unwrap(), ["a"]);
    }

    #[test]
    fn sessions_move_between_folders() {
        let s = SessionStore::open_in_memory().unwrap();
        let a = session(&s, "a", None);
        s.move_session(&a, Some(" Lab / Core ")).unwrap();
        assert_eq!(folder_of(&s, &a).as_deref(), Some("Lab/Core"));
        s.move_session(&a, None).unwrap();
        assert_eq!(folder_of(&s, &a), None);
        assert!(s.move_session(&a, Some("/")).is_err());
    }
}
