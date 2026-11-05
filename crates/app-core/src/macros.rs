//! Recorded keystrokes. Like snippets they are text the user sends to a
//! terminal; nothing is executed by the app. A recording can contain
//! whatever was typed, so the UI warns while recording and shows the text
//! before it is saved.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::AppError;
use crate::sessions::SessionStore;

pub const MAX_MACRO_BYTES: usize = 64 * 1024;
const MAX_STEPS: usize = 10_000;
/// Longer pauses are shortened when recording.
const MAX_DELAY_MS: u32 = 2_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroStep {
    /// Pause before sending `data`.
    pub delay_ms: u32,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroInput {
    pub name: String,
    pub steps: Vec<MacroStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Macro {
    pub id: String,
    #[serde(flatten)]
    pub input: MacroInput,
}

impl MacroInput {
    pub fn validate(&self) -> Result<(), AppError> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
            return Err(AppError::invalid(
                "name",
                "1-200 characters, no control characters",
            ));
        }
        if self.steps.is_empty() || self.steps.len() > MAX_STEPS {
            return Err(AppError::invalid("macro", "must have 1-10000 steps"));
        }
        if self.steps.iter().map(|s| s.data.len()).sum::<usize>() > MAX_MACRO_BYTES {
            return Err(AppError::invalid("macro", "longer than 64 KiB"));
        }
        if self.steps.iter().any(|s| s.delay_ms > MAX_DELAY_MS) {
            return Err(AppError::invalid("macro", "pause longer than 2 seconds"));
        }
        Ok(())
    }
}

fn new_id() -> Result<String, AppError> {
    let mut b = [0u8; 12];
    getrandom::fill(&mut b).map_err(|e| AppError::Db(e.to_string()))?;
    Ok(format!(
        "mac-{}",
        b.iter().map(|x| format!("{x:02x}")).collect::<String>()
    ))
}

impl SessionStore {
    pub fn macros(&self) -> Result<Vec<Macro>, AppError> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT id, name, steps FROM macros ORDER BY name COLLATE NOCASE")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, steps) = row?;
            // Unreadable steps must not hide the other macros.
            if let Ok(steps) = serde_json::from_str(&steps) {
                out.push(Macro {
                    id,
                    input: MacroInput { name, steps },
                });
            }
        }
        Ok(out)
    }

    pub fn create_macro(&self, input: &MacroInput) -> Result<Macro, AppError> {
        input.validate()?;
        let id = new_id()?;
        let steps = serde_json::to_string(&input.steps).map_err(|e| AppError::Db(e.to_string()))?;
        self.conn().execute(
            "INSERT INTO macros (id, name, steps) VALUES (?1, ?2, ?3)",
            params![id, input.name.trim(), steps],
        )?;
        Ok(Macro {
            id,
            input: MacroInput {
                name: input.name.trim().to_owned(),
                steps: input.steps.clone(),
            },
        })
    }

    pub fn delete_macro(&self, id: &str) -> Result<(), AppError> {
        if self
            .conn()
            .execute("DELETE FROM macros WHERE id = ?1", [id])?
            == 0
        {
            return Err(AppError::NotFound);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(data: &str) -> MacroInput {
        MacroInput {
            name: " deploy ".into(),
            steps: vec![
                MacroStep {
                    delay_ms: 0,
                    data: "ls\r".into(),
                },
                MacroStep {
                    delay_ms: 1500,
                    data: data.into(),
                },
            ],
        }
    }

    #[test]
    fn keeps_keystrokes_exactly() {
        let s = SessionStore::open_in_memory().unwrap();
        let m = s.create_macro(&input("\x1b[A\x03")).unwrap();
        assert_eq!(m.input.name, "deploy");
        assert_eq!(s.macros().unwrap(), std::slice::from_ref(&m));
        s.delete_macro(&m.id).unwrap();
        assert!(s.macros().unwrap().is_empty());
        assert!(matches!(s.delete_macro(&m.id), Err(AppError::NotFound)));
    }

    #[test]
    fn rejects_empty_oversized_and_long_pauses() {
        let s = SessionStore::open_in_memory().unwrap();
        let mut bad = input("x");
        bad.steps.clear();
        assert!(s.create_macro(&bad).is_err());
        assert!(
            s.create_macro(&input(&"a".repeat(MAX_MACRO_BYTES + 1)))
                .is_err()
        );
        let mut slow = input("x");
        slow.steps[1].delay_ms = MAX_DELAY_MS + 1;
        assert!(s.create_macro(&slow).is_err());
        let mut unnamed = input("x");
        unnamed.name = "  ".into();
        assert!(s.create_macro(&unnamed).is_err());
    }
}
