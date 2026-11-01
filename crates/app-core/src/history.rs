//! A record of finished SFTP transfers (what, where, when, outcome). Only
//! metadata is kept — never file contents or credentials — and only the
//! most recent [`MAX_HISTORY`] entries.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::AppError;
use crate::sessions::SessionStore;

pub const MAX_HISTORY: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferDirection {
    Download,
    Upload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferOutcome {
    Done,
    Failed,
    Cancelled,
}

/// What a transfer is, known when it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferMeta {
    pub direction: TransferDirection,
    pub name: String,
    pub remote_path: String,
    /// The connection's label (tab title), for the user's orientation.
    pub connection: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferRecord {
    pub id: i64,
    pub direction: TransferDirection,
    pub name: String,
    pub remote_path: String,
    pub local_path: Option<String>,
    pub size: Option<u64>,
    pub outcome: TransferOutcome,
    pub message: Option<String>,
    pub connection: String,
    /// Seconds since the Unix epoch.
    pub finished_at: i64,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn clip(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

impl SessionStore {
    /// Records a finished transfer and drops entries beyond [`MAX_HISTORY`].
    pub fn record_transfer(
        &self,
        meta: &TransferMeta,
        outcome: TransferOutcome,
        local_path: Option<&str>,
        message: Option<&str>,
    ) -> Result<(), AppError> {
        let dir = match meta.direction {
            TransferDirection::Download => "download",
            TransferDirection::Upload => "upload",
        };
        let out = match outcome {
            TransferOutcome::Done => "done",
            TransferOutcome::Failed => "failed",
            TransferOutcome::Cancelled => "cancelled",
        };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO transfer_history (direction, name, remote_path, local_path, size, outcome, message, connection, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                dir,
                clip(&meta.name, 255),
                clip(&meta.remote_path, 4096),
                local_path.map(|p| clip(p, 4096)),
                meta.size.and_then(|s| i64::try_from(s).ok()),
                out,
                message.map(|m| clip(m, 500)),
                clip(&meta.connection, 200),
                now()
            ],
        )?;
        conn.execute(
            "DELETE FROM transfer_history WHERE id <= (SELECT MAX(id) FROM transfer_history) - ?1",
            [MAX_HISTORY],
        )?;
        Ok(())
    }

    /// Most recent first.
    pub fn transfer_history(&self, limit: u32) -> Result<Vec<TransferRecord>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, direction, name, remote_path, local_path, size, outcome, message, connection, finished_at
             FROM transfer_history ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([i64::from(limit.min(MAX_HISTORY as u32))], |r| {
            let direction: String = r.get(1)?;
            let outcome: String = r.get(6)?;
            Ok(TransferRecord {
                id: r.get(0)?,
                direction: if direction == "upload" {
                    TransferDirection::Upload
                } else {
                    TransferDirection::Download
                },
                name: r.get(2)?,
                remote_path: r.get(3)?,
                local_path: r.get(4)?,
                size: r
                    .get::<_, Option<i64>>(5)?
                    .and_then(|s| u64::try_from(s).ok()),
                outcome: match outcome.as_str() {
                    "done" => TransferOutcome::Done,
                    "cancelled" => TransferOutcome::Cancelled,
                    _ => TransferOutcome::Failed,
                },
                message: r.get(7)?,
                connection: r.get(8)?,
                finished_at: r.get(9)?,
            })
        })?;
        rows.map(|r| r.map_err(AppError::from)).collect()
    }

    pub fn clear_transfer_history(&self) -> Result<(), AppError> {
        self.conn().execute("DELETE FROM transfer_history", [])?;
        Ok(())
    }
}

/// A [`crate::TransferRecorder`] that writes finished transfers to the
/// history in `store` (progress and retries are not recorded).
pub fn history_recorder(store: std::sync::Arc<SessionStore>) -> crate::TransferRecorder {
    std::sync::Arc::new(move |meta: &TransferMeta, ev: &crate::TransferEvent| {
        use crate::TransferEvent as E;
        let (outcome, local, message) = match ev {
            E::Done { destination } => (
                TransferOutcome::Done,
                (meta.direction == TransferDirection::Download).then_some(destination.as_str()),
                None,
            ),
            E::Failed { message, .. } => (TransferOutcome::Failed, None, Some(message.as_str())),
            E::Cancelled => (TransferOutcome::Cancelled, None, None),
            _ => return,
        };
        // History is best effort: a failed insert must not affect transfers.
        let _ = store.record_transfer(meta, outcome, local, message);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(name: &str) -> TransferMeta {
        TransferMeta {
            direction: TransferDirection::Download,
            name: name.into(),
            remote_path: format!("/srv/{name}"),
            connection: "web-01".into(),
            size: Some(42),
        }
    }

    #[test]
    fn records_newest_first_and_caps_the_size() {
        let s = SessionStore::open_in_memory().unwrap();
        s.record_transfer(
            &meta("a.log"),
            TransferOutcome::Done,
            Some("/Users/me/Downloads/a.log"),
            None,
        )
        .unwrap();
        let mut up = meta("b.tar");
        up.direction = TransferDirection::Upload;
        s.record_transfer(
            &up,
            TransferOutcome::Failed,
            None,
            Some("permission\u{1b}[31m denied"),
        )
        .unwrap();
        let h = s.transfer_history(10).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].name, "b.tar");
        assert_eq!(h[0].direction, TransferDirection::Upload);
        assert_eq!(h[0].outcome, TransferOutcome::Failed);
        assert_eq!(
            h[0].message.as_deref(),
            Some("permission[31m denied"),
            "control chars removed"
        );
        assert_eq!(
            h[1].local_path.as_deref(),
            Some("/Users/me/Downloads/a.log")
        );
        assert_eq!(h[1].size, Some(42));

        for i in 0..(MAX_HISTORY + 20) {
            s.record_transfer(
                &meta(&format!("f{i}")),
                TransferOutcome::Cancelled,
                None,
                None,
            )
            .unwrap();
        }
        let all = s.transfer_history(10_000).unwrap();
        assert_eq!(all.len() as i64, MAX_HISTORY);
        assert_eq!(all[0].name, format!("f{}", MAX_HISTORY + 19));
        s.clear_transfer_history().unwrap();
        assert!(s.transfer_history(10).unwrap().is_empty());
    }
}
