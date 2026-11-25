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
/// Longest pause in a step: the recorder shortens pauses to 2 s, an edited
/// step may wait up to 10 minutes (e.g. for a device to reboot).
const MAX_DELAY_MS: u32 = 600_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroStep {
    /// Pause before sending `data`.
    pub delay_ms: u32,
    pub data: String,
    /// Before sending `data`, wait until the terminal shows this text (a
    /// prompt such as `#`); edited steps only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_for: Option<String>,
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
        if self.steps.iter().any(|s| {
            s.wait_for.as_deref().is_some_and(|w| {
                w.is_empty() || w.chars().count() > 200 || w.chars().any(char::is_control)
            })
        }) {
            return Err(AppError::invalid(
                "macro",
                "text to wait for: 1-200 characters, no control characters",
            ));
        }
        if self.steps.iter().any(|s| s.delay_ms > MAX_DELAY_MS) {
            return Err(AppError::invalid("macro", "pause longer than 10 minutes"));
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

    /// Replaces a macro's name and steps (edited in the UI).
    pub fn update_macro(&self, id: &str, input: &MacroInput) -> Result<Macro, AppError> {
        input.validate()?;
        let steps = serde_json::to_string(&input.steps).map_err(|e| AppError::Db(e.to_string()))?;
        let n = self.conn().execute(
            "UPDATE macros SET name = ?2, steps = ?3 WHERE id = ?1",
            params![id, input.name.trim(), steps],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        Ok(Macro {
            id: id.to_owned(),
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
                    wait_for: None,
                },
                MacroStep {
                    delay_ms: 1500,
                    data: data.into(),
                    wait_for: None,
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
    fn edits_name_and_steps() {
        let s = SessionStore::open_in_memory().unwrap();
        let m = s.create_macro(&input("pwd\r")).unwrap();
        let mut edited = m.input.clone();
        edited.name = "renamed".into();
        edited.steps.pop();
        let u = s.update_macro(&m.id, &edited).unwrap();
        assert_eq!(u.input.name, "renamed");
        assert_eq!(s.macros().unwrap()[0].input.steps.len(), 1);
        edited.name = String::new();
        assert!(
            s.update_macro(&m.id, &edited).is_err(),
            "validated like a new macro"
        );
        assert!(matches!(
            s.update_macro("mac-missing", &m.input),
            Err(AppError::NotFound)
        ));
    }

    #[test]
    fn steps_can_wait_for_text() {
        let s = SessionStore::open_in_memory().unwrap();
        let mut m = input("q");
        m.steps[1].wait_for = Some("#".into());
        let saved = s.create_macro(&m).unwrap();
        assert_eq!(
            s.macros().unwrap()[0].input.steps[1].wait_for.as_deref(),
            Some("#")
        );
        let json = serde_json::to_string(&saved.input.steps[0]).unwrap();
        assert!(!json.contains("waitFor"), "left out when unset: {json}");
        m.steps[1].wait_for = Some(String::new());
        assert!(m.validate().is_err());
        m.steps[1].wait_for = Some("a\nb".into());
        assert!(m.validate().is_err());
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
