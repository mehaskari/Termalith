//! Background SFTP transfers with progress events and cancellation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use protocol_ssh::{CancellationToken, SftpClient, SshError, TransferOptions};
use serde::Serialize;

use crate::AppError;
use crate::files::{safe_file_name, unique_destination};

/// Minimum bytes between progress events, to keep IPC traffic low.
const PROGRESS_STEP: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TransferEvent {
    #[serde(rename_all = "camelCase")]
    Progress {
        transferred: u64,
        total: Option<u64>,
    },
    /// `destination` is a local path (download) or remote path (upload).
    Done {
        destination: String,
    },
    Failed {
        message: String,
    },
    Cancelled,
}

#[derive(Default)]
pub struct Transfers {
    next: AtomicU64,
    active: Mutex<HashMap<u64, CancellationToken>>,
}

/// Joins a remote directory and a validated file name.
pub fn remote_join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

impl Transfers {
    fn register(&self) -> (u64, CancellationToken) {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let token = CancellationToken::new();
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, token.clone());
        (id, token)
    }

    fn finish(&self, id: u64) {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }

    /// Downloads `remote_dir/name` into `dest_dir` under a unique,
    /// validated name. Returns the transfer id at once.
    pub fn start_download(
        self: &Arc<Self>,
        sftp: Arc<SftpClient>,
        remote_dir: &str,
        name: &str,
        total: Option<u64>,
        dest_dir: PathBuf,
        mut sink: impl FnMut(TransferEvent) + Send + 'static,
    ) -> Result<u64, AppError> {
        let name = safe_file_name(name)?.to_owned();
        let remote = remote_join(remote_dir, &name);
        let (id, cancel) = self.register();
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let dest = unique_destination(&dest_dir, &name);
            let mut last = 0u64;
            let result = sftp
                .download(&remote, &dest, TransferOptions::default(), &cancel, |n| {
                    if n - last >= PROGRESS_STEP {
                        last = n;
                        sink(TransferEvent::Progress {
                            transferred: n,
                            total,
                        });
                    }
                })
                .await;
            sink(outcome(result, dest.to_string_lossy().into_owned(), total));
            this.finish(id);
        });
        Ok(id)
    }

    /// Uploads a local file into `remote_dir`, keeping its name. Refuses to
    /// overwrite an existing remote file.
    pub fn start_upload(
        self: &Arc<Self>,
        sftp: Arc<SftpClient>,
        local: PathBuf,
        remote_dir: &str,
        mut sink: impl FnMut(TransferEvent) + Send + 'static,
    ) -> Result<u64, AppError> {
        let name = local
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| AppError::invalid("file", "has no name"))?;
        let remote = remote_join(remote_dir, safe_file_name(&name)?);
        let total = std::fs::metadata(&local).ok().map(|m| m.len());
        let (id, cancel) = self.register();
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut last = 0u64;
            let result = sftp
                .upload(&local, &remote, TransferOptions::default(), &cancel, |n| {
                    if n - last >= PROGRESS_STEP {
                        last = n;
                        sink(TransferEvent::Progress {
                            transferred: n,
                            total,
                        });
                    }
                })
                .await;
            sink(outcome(result, remote, total));
            this.finish(id);
        });
        Ok(id)
    }

    /// Requests cancellation. `false` if the transfer is unknown or done.
    pub fn cancel(&self, id: u64) -> bool {
        match self
            .active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
        {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    pub fn active_count(&self) -> usize {
        self.active.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

fn outcome(
    result: Result<u64, SshError>,
    destination: String,
    _total: Option<u64>,
) -> TransferEvent {
    match result {
        Ok(_) => TransferEvent::Done { destination },
        Err(SshError::Cancelled) => TransferEvent::Cancelled,
        Err(SshError::DestinationExists(p)) => TransferEvent::Failed {
            message: format!("{p} already exists"),
        },
        Err(e) => TransferEvent::Failed {
            message: e.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_join_handles_trailing_slash() {
        assert_eq!(remote_join("/home/me", "a.txt"), "/home/me/a.txt");
        assert_eq!(remote_join("/", "a.txt"), "/a.txt");
        assert_eq!(remote_join("/srv/", "a.txt"), "/srv/a.txt");
    }

    #[test]
    fn events_serialize_with_kind_tag() {
        let p = serde_json::to_value(TransferEvent::Progress {
            transferred: 5,
            total: Some(10),
        })
        .unwrap();
        assert_eq!(
            p,
            serde_json::json!({"kind": "progress", "transferred": 5, "total": 10})
        );
        let c = serde_json::to_value(TransferEvent::Cancelled).unwrap();
        assert_eq!(c, serde_json::json!({"kind": "cancelled"}));
    }

    #[test]
    fn cancel_unknown_is_false() {
        assert!(!Transfers::default().cancel(1));
    }
}
