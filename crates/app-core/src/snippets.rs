//! Saved command snippets with `{{variable}}` placeholders. Snippets are
//! text the user sends to a terminal; nothing is executed by the app.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::AppError;
use crate::sessions::SessionStore;

pub const MAX_SNIPPET_LEN: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetInput {
    pub name: String,
    pub command: String,
    pub folder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snippet {
    pub id: String,
    #[serde(flatten)]
    pub input: SnippetInput,
}

impl SnippetInput {
    pub fn validate(&self) -> Result<(), AppError> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
            return Err(AppError::invalid(
                "name",
                "1-200 characters, no control characters",
            ));
        }
        if self.command.trim().is_empty() {
            return Err(AppError::invalid("command", "must not be empty"));
        }
        if self.command.len() > MAX_SNIPPET_LEN {
            return Err(AppError::invalid("command", "longer than 16 KiB"));
        }
        // Tabs and newlines are fine (multi-line snippets); other control
        // characters (escape sequences) are not.
        if self
            .command
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t' | '\r'))
        {
            return Err(AppError::invalid("command", "contains control characters"));
        }
        if let Some(f) = &self.folder
            && (f.chars().count() > 200 || f.chars().any(char::is_control))
        {
            return Err(AppError::invalid(
                "folder",
                "too long or contains control characters",
            ));
        }
        Ok(())
    }
}

fn new_id() -> Result<String, AppError> {
    let mut b = [0u8; 12];
    getrandom::fill(&mut b).map_err(|e| AppError::Db(e.to_string()))?;
    Ok(format!(
        "snp-{}",
        b.iter().map(|x| format!("{x:02x}")).collect::<String>()
    ))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl SessionStore {
    pub fn snippets(&self) -> Result<Vec<Snippet>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, command, folder FROM snippets
             ORDER BY folder IS NULL, folder COLLATE NOCASE, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Snippet {
                id: r.get(0)?,
                input: SnippetInput {
                    name: r.get(1)?,
                    command: r.get(2)?,
                    folder: r.get(3)?,
                },
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn create_snippet(&self, input: &SnippetInput) -> Result<Snippet, AppError> {
        input.validate()?;
        let id = new_id()?;
        let t = now();
        self.conn().execute(
            "INSERT INTO snippets (id, name, command, folder, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, input.name.trim(), input.command, input.folder.as_deref().map(str::trim).filter(|f| !f.is_empty()), t],
        )?;
        self.snippet(&id)
    }

    pub fn update_snippet(&self, id: &str, input: &SnippetInput) -> Result<Snippet, AppError> {
        input.validate()?;
        let n = self.conn().execute(
            "UPDATE snippets SET name = ?2, command = ?3, folder = ?4, updated_at = ?5 WHERE id = ?1",
            params![id, input.name.trim(), input.command, input.folder.as_deref().map(str::trim).filter(|f| !f.is_empty()), now()],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        self.snippet(id)
    }

    pub fn delete_snippet(&self, id: &str) -> Result<(), AppError> {
        if self
            .conn()
            .execute("DELETE FROM snippets WHERE id = ?1", [id])?
            == 0
        {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    fn snippet(&self, id: &str) -> Result<Snippet, AppError> {
        self.conn()
            .query_row(
                "SELECT id, name, command, folder FROM snippets WHERE id = ?1",
                [id],
                |r| {
                    Ok(Snippet {
                        id: r.get(0)?,
                        input: SnippetInput {
                            name: r.get(1)?,
                            command: r.get(2)?,
                            folder: r.get(3)?,
                        },
                    })
                },
            )
            .optional()?
            .ok_or(AppError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(name: &str, command: &str) -> SnippetInput {
        SnippetInput {
            name: name.into(),
            command: command.into(),
            folder: Some("Cisco".into()),
        }
    }

    #[test]
    fn crud_and_ordering() {
        let s = SessionStore::open_in_memory().unwrap();
        let b = s
            .create_snippet(&input("show int", "show interfaces {{iface}}"))
            .unwrap();
        s.create_snippet(&SnippetInput {
            folder: None,
            ..input("uptime", "uptime")
        })
        .unwrap();
        s.create_snippet(&input("a first", "show version")).unwrap();
        let names: Vec<_> = s
            .snippets()
            .unwrap()
            .into_iter()
            .map(|x| x.input.name)
            .collect();
        assert_eq!(names, ["a first", "show int", "uptime"]);
        let u = s
            .update_snippet(
                &b.id,
                &input("show int", "show interfaces {{iface}} status"),
            )
            .unwrap();
        assert!(u.input.command.ends_with("status"));
        s.delete_snippet(&b.id).unwrap();
        assert!(matches!(s.delete_snippet(&b.id), Err(AppError::NotFound)));
    }

    #[test]
    fn validation() {
        assert!(input("multi", "cd /tmp\nls -la\n").validate().is_ok());
        assert!(input("", "ls").validate().is_err());
        assert!(input("x", "  ").validate().is_err());
        assert!(
            input("x", "echo \u{1b}[31m").validate().is_err(),
            "escape sequences rejected"
        );
        assert!(
            input("x", &"a".repeat(MAX_SNIPPET_LEN + 1))
                .validate()
                .is_err()
        );
    }
}
