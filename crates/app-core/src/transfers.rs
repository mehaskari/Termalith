//! Background SFTP transfers with progress events, cancellation, a limit on
//! simultaneous transfers, automatic retry of transient failures and manual
//! retry of failed or cancelled ones.
//!
//! Retrying restarts the file from the beginning: downloads and uploads
//! write to a temporary name and rename on success, so a failed attempt
//! never leaves a partial file under the final name.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use protocol_ssh::{CancellationToken, SshError, TransferOptions};
use serde::Serialize;
use tokio::sync::Semaphore;

use crate::files::{MAX_FOLDER_LEVELS, safe_file_name, unique_destination, walk_local_folder};
use crate::history::{TransferDirection, TransferMeta};
use crate::{AppError, RemoteFiles};

/// Minimum bytes between progress events, to keep IPC traffic low.
const PROGRESS_STEP: u64 = 256 * 1024;
/// Transfers running at once; more wait in a queue.
pub const MAX_CONCURRENT_TRANSFERS: usize = 3;
/// Automatic attempts after the first, for transient errors only.
pub const AUTO_RETRIES: u32 = 2;
/// Failed/cancelled transfers remembered for manual retry.
const RETRY_MEMORY: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TransferEvent {
    /// Waiting for a free slot.
    Queued,
    #[serde(rename_all = "camelCase")]
    Progress {
        transferred: u64,
        total: Option<u64>,
    },
    /// A transient error; the transfer restarts after a short pause.
    #[serde(rename_all = "camelCase")]
    Retrying {
        attempt: u32,
        max: u32,
        message: String,
    },
    /// `destination` is a local path (download) or remote path (upload).
    Done {
        destination: String,
    },
    /// `retryable`: [`Transfers::retry`] can run it again.
    Failed {
        message: String,
        retryable: bool,
    },
    Cancelled,
    /// Stopped by [`Transfers::pause`]; [`Transfers::retry`] continues where
    /// it stopped.
    Paused,
}

type Progress = Box<dyn FnMut(u64) + Send>;
type OpFuture = Pin<Box<dyn Future<Output = Result<String, SshError>> + Send>>;
/// One attempt of a transfer: returns the destination on success.
type Op = Arc<dyn Fn(CancellationToken, Progress) -> OpFuture + Send + Sync>;

/// Removes what an unfinished transfer left behind (partial files).
type Cleanup = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
struct Job {
    op: Op,
    total: Option<u64>,
    /// What and where, for the history (None in tests).
    meta: Option<TransferMeta>,
    /// Run when the transfer will not continue where it stopped: cancelled,
    /// or failed for good. Not run when paused or failed retryably.
    cleanup: Option<Cleanup>,
}

/// A file to download.
pub struct Download<'a> {
    pub remote_dir: &'a str,
    /// Untrusted name from the server; validated before use.
    pub name: &'a str,
    pub size: Option<u64>,
    /// Where to save (chosen by the backend, not the UI).
    pub dest_dir: PathBuf,
    /// Connection label for the history.
    pub connection: &'a str,
}

/// Called once per finished transfer (also retries) with its outcome.
pub type TransferRecorder = Arc<dyn Fn(&TransferMeta, &TransferEvent) + Send + Sync>;

pub struct Transfers {
    next: AtomicU64,
    active: Mutex<HashMap<u64, CancellationToken>>,
    retryable: Mutex<VecDeque<(u64, Job)>>,
    /// Running transfers asked to pause (their cancellation is a pause).
    pausing: Mutex<std::collections::HashSet<u64>>,
    slots: Arc<Semaphore>,
    retry_delays: [Duration; AUTO_RETRIES as usize],
    recorder: Option<TransferRecorder>,
}

impl Default for Transfers {
    fn default() -> Self {
        Self::new(
            MAX_CONCURRENT_TRANSFERS,
            [Duration::from_secs(1), Duration::from_secs(3)],
        )
    }
}

/// Most entries (files and folders) a downloaded folder may contain; a
/// guard against servers that list the same folders over and over.
pub const MAX_DOWNLOAD_ENTRIES: usize = 500_000;

/// Joins a remote directory and a validated file name.
pub fn remote_join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// The hidden file a download of `dest` writes into until it is complete.
fn stat_key(s: protocol_ssh::RemoteStat) -> (Option<u64>, Option<u32>) {
    (s.size, s.modified)
}

fn partial_path(dest: &std::path::Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dest.with_file_name(format!(".{name}.termalith-partial"))
}

/// Connection-level problems that may succeed on another attempt. Server
/// answers (permission denied, no such file, already exists) are final.
fn is_transient(e: &SshError) -> bool {
    matches!(
        e,
        SshError::Io(_) | SshError::Protocol(_) | SshError::Timeout { .. }
    )
}

impl Transfers {
    fn new(concurrent: usize, retry_delays: [Duration; AUTO_RETRIES as usize]) -> Self {
        Self {
            next: AtomicU64::new(0),
            active: Mutex::default(),
            retryable: Mutex::default(),
            pausing: Mutex::default(),
            slots: Arc::new(Semaphore::new(concurrent)),
            retry_delays,
            recorder: None,
        }
    }

    /// Transfers that report each finished transfer to `recorder`
    /// (e.g. the transfer history).
    pub fn with_recorder(recorder: TransferRecorder) -> Self {
        Self {
            recorder: Some(recorder),
            ..Self::default()
        }
    }

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

    fn remember(&self, id: u64, job: Job) {
        let mut r = self.retryable.lock().unwrap_or_else(|e| e.into_inner());
        if r.len() >= RETRY_MEMORY {
            r.pop_front();
        }
        r.push_back((id, job));
    }

    /// Downloads `remote_dir/name` into `dest_dir` under a unique,
    /// validated name. Returns the transfer id at once.
    pub fn start_download(
        self: &Arc<Self>,
        files: impl Into<RemoteFiles>,
        req: Download<'_>,
        sink: impl FnMut(TransferEvent) + Send + 'static,
    ) -> Result<u64, AppError> {
        let files = files.into();
        let Download {
            remote_dir,
            name,
            size: total,
            dest_dir,
            connection,
        } = req;
        let name = safe_file_name(name)?.to_owned();
        let remote = remote_join(remote_dir, &name);
        let meta = TransferMeta {
            direction: TransferDirection::Download,
            name: name.clone(),
            remote_path: remote.clone(),
            connection: connection.to_owned(),
            size: total,
        };
        // Chosen once: later attempts continue in the same partial file.
        let dest: Arc<Mutex<Option<PathBuf>>> = Arc::default();
        // The file as first seen; a changed file is downloaded from the start.
        let seen: Arc<Mutex<Option<protocol_ssh::RemoteStat>>> = Arc::default();
        let cleanup: Cleanup = {
            let dest = Arc::clone(&dest);
            Arc::new(move || {
                if let Some(d) = dest.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                    let _ = std::fs::remove_file(partial_path(d));
                }
            })
        };
        let op: Op = Arc::new(move |cancel, progress| {
            let (files, remote, dest, seen) = (
                files.clone(),
                remote.clone(),
                Arc::clone(&dest),
                Arc::clone(&seen),
            );
            let dest = dest
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_or_insert_with(|| unique_destination(&dest_dir, &name))
                .clone();
            Box::pin(async move {
                let partial = partial_path(&dest);
                let now = files.stat(&remote).await.ok();
                let changed = {
                    let mut seen = seen.lock().unwrap_or_else(|e| e.into_inner());
                    let first = seen.is_none();
                    let changed = !first && now.map(stat_key) != seen.map(stat_key);
                    if now.is_some() {
                        *seen = now;
                    }
                    changed
                };
                let too_long = now
                    .and_then(|s| s.size)
                    .zip(std::fs::metadata(&partial).ok())
                    .is_some_and(|(size, m)| m.len() > size);
                if changed || too_long {
                    let _ = std::fs::remove_file(&partial);
                }
                files
                    .download_resume(&remote, &dest, &partial, &cancel, progress)
                    .await?;
                Ok(dest.to_string_lossy().into_owned())
            })
        });
        Ok(self.spawn(
            Job {
                op,
                total,
                meta: Some(meta),
                cleanup: Some(cleanup),
            },
            sink,
        ))
    }
    /// Downloads the remote folder `remote_dir/name` with everything in it
    /// into `dest_dir` under a unique name (one transfer). Symbolic links
    /// are not followed. A retry continues in the folder this transfer
    /// created, keeping files that are complete.
    pub fn start_folder_download(
        self: &Arc<Self>,
        files: impl Into<RemoteFiles>,
        req: Download<'_>,
        sink: impl FnMut(TransferEvent) + Send + 'static,
    ) -> Result<u64, AppError> {
        let files = files.into();
        let Download {
            remote_dir,
            name,
            dest_dir,
            connection,
            ..
        } = req;
        let name = safe_file_name(name)?.to_owned();
        let remote = remote_join(remote_dir, &name);
        let meta = TransferMeta {
            direction: TransferDirection::Download,
            name: format!("{name}/"),
            remote_path: remote.clone(),
            connection: connection.to_owned(),
            size: None,
        };
        // The local folder, once created, so retries reuse it.
        let chosen: Arc<Mutex<Option<PathBuf>>> = Arc::default();
        // The file being downloaded: its partial file and how the listing
        // described it, so a retry continues it only if it is unchanged.
        type Current = Option<(PathBuf, (Option<u64>, Option<u32>))>;
        let current: Arc<Mutex<Current>> = Arc::default();
        let cleanup: Cleanup = {
            let current = Arc::clone(&current);
            Arc::new(move || {
                if let Some((p, _)) = current.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    let _ = std::fs::remove_file(p);
                }
            })
        };
        let op: Op = Arc::new(move |cancel, mut progress| {
            let (files, remote, name, dest_dir, chosen, current) = (
                files.clone(),
                remote.clone(),
                name.clone(),
                dest_dir.clone(),
                Arc::clone(&chosen),
                Arc::clone(&current),
            );
            Box::pin(async move {
                use protocol_ssh::RemoteKind;
                let earlier = chosen.lock().unwrap_or_else(|e| e.into_inner()).clone();
                let retrying = earlier.is_some();
                let root = match earlier {
                    Some(root) => root,
                    None => {
                        let root = unique_destination(&dest_dir, &name);
                        std::fs::create_dir(&root)?;
                        *chosen.lock().unwrap_or_else(|e| e.into_inner()) = Some(root.clone());
                        root
                    }
                };
                // Folders are listed as the download goes, so files arrive
                // (and progress shows) from the first folder on, also on
                // FTP where every listing is a new connection.
                let mut pending: VecDeque<Vec<String>> = VecDeque::from([Vec::new()]);
                let mut entries = 0usize;
                let mut done = 0u64;
                while let Some(rel) = pending.pop_front() {
                    if cancel.is_cancelled() {
                        return Err(SshError::Cancelled);
                    }
                    let dir = rel
                        .iter()
                        .fold(remote.clone(), |acc, p| remote_join(&acc, p));
                    let local_dir = rel.iter().fold(root.clone(), |acc, p| acc.join(p));
                    std::fs::create_dir_all(&local_dir)?;
                    let mut list = files.list(&dir).await?;
                    list.sort_by(|a, b| a.name.cmp(&b.name));
                    for e in list {
                        if e.name == "." || e.name == ".." {
                            continue;
                        }
                        let is_dir = match e.kind {
                            RemoteKind::Dir => true,
                            RemoteKind::File => false,
                            // Symbolic links are not followed.
                            _ => continue,
                        };
                        entries += 1;
                        if entries > MAX_DOWNLOAD_ENTRIES {
                            return Err(SshError::Remote(format!(
                                "{remote} has more than {MAX_DOWNLOAD_ENTRIES} entries"
                            )));
                        }
                        let from = remote_join(&dir, &e.name);
                        if safe_file_name(&e.name).is_err() {
                            return Err(SshError::Remote(format!(
                                "{from}: the name is not allowed on this computer"
                            )));
                        }
                        if is_dir {
                            if rel.len() + 1 > MAX_FOLDER_LEVELS {
                                return Err(SshError::Remote(format!(
                                    "{remote} is nested too deeply"
                                )));
                            }
                            let mut child = rel.clone();
                            child.push(e.name);
                            pending.push_back(child);
                            continue;
                        }
                        let local = local_dir.join(&e.name);
                        // A retry keeps files an earlier attempt finished.
                        if retrying
                            && let Some(size) = e.size
                            && std::fs::metadata(&local).is_ok_and(|m| m.len() == size)
                        {
                            done += size;
                            progress(done);
                            continue;
                        }
                        let partial = partial_path(&local);
                        let key = (e.size, e.modified);
                        {
                            let mut cur = current.lock().unwrap_or_else(|e| e.into_inner());
                            if cur
                                .as_ref()
                                .is_some_and(|(p, k)| *p == partial && *k != key)
                            {
                                let _ = std::fs::remove_file(&partial);
                            }
                            *cur = Some((partial.clone(), key));
                        }
                        if e.size.is_some_and(|size| {
                            std::fs::metadata(&partial).is_ok_and(|m| m.len() > size)
                        }) {
                            let _ = std::fs::remove_file(&partial);
                        }
                        let n = files
                            .download_resume(&from, &local, &partial, &cancel, |n| {
                                progress(done + n)
                            })
                            .await?;
                        *current.lock().unwrap_or_else(|e| e.into_inner()) = None;
                        done += n;
                    }
                }
                Ok(root.to_string_lossy().into_owned())
            })
        });
        Ok(self.spawn(
            Job {
                op,
                total: None,
                meta: Some(meta),
                cleanup: Some(cleanup),
            },
            sink,
        ))
    }

    /// Uploads a local file into `remote_dir`, keeping its name. Refuses to
    /// overwrite an existing remote file.
    pub fn start_upload(
        self: &Arc<Self>,
        files: impl Into<RemoteFiles>,
        local: PathBuf,
        remote_dir: &str,
        connection: &str,
        sink: impl FnMut(TransferEvent) + Send + 'static,
    ) -> Result<u64, AppError> {
        let files = files.into();
        let name = local
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| AppError::invalid("file", "has no name"))?;
        let remote = remote_join(remote_dir, safe_file_name(&name)?);
        if local.is_dir() {
            return self.start_folder_upload(files, local, name, remote, connection, sink);
        }
        let total = std::fs::metadata(&local).ok().map(|m| m.len());
        let meta = TransferMeta {
            direction: TransferDirection::Upload,
            name: name.clone(),
            remote_path: remote.clone(),
            connection: connection.to_owned(),
            size: total,
        };
        let op: Op = Arc::new(move |cancel, mut progress| {
            let (files, local, remote) = (files.clone(), local.clone(), remote.clone());
            Box::pin(async move {
                files
                    .upload(&local, &remote, TransferOptions::default(), &cancel, |n| {
                        progress(n)
                    })
                    .await?;
                Ok(remote)
            })
        });
        Ok(self.spawn(
            Job {
                op,
                total,
                meta: Some(meta),
                cleanup: None,
            },
            sink,
        ))
    }

    /// Uploads a local folder with everything in it as `remote` (one
    /// transfer). Refuses when `remote` already exists. A retry continues in
    /// the folder this transfer created, replacing files it wrote before.
    /// Symbolic links are not followed.
    fn start_folder_upload(
        self: &Arc<Self>,
        files: RemoteFiles,
        local: PathBuf,
        name: String,
        remote: String,
        connection: &str,
        sink: impl FnMut(TransferEvent) + Send + 'static,
    ) -> Result<u64, AppError> {
        let tree = Arc::new(walk_local_folder(&local)?);
        let total = tree.total_bytes();
        let meta = TransferMeta {
            direction: TransferDirection::Upload,
            name: format!("{name}/"),
            remote_path: remote.clone(),
            connection: connection.to_owned(),
            size: Some(total),
        };
        // Set once the remote folder is ours, so retries may reuse it.
        let created = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let op: Op = Arc::new(move |cancel, mut progress| {
            let (files, tree, remote, created) = (
                files.clone(),
                Arc::clone(&tree),
                remote.clone(),
                Arc::clone(&created),
            );
            Box::pin(async move {
                let ours = created.load(Ordering::SeqCst);
                let mkdir = |path: String| {
                    let files = files.clone();
                    async move {
                        match files.create_dir(&path).await {
                            Err(SshError::DestinationExists(_)) if ours => Ok(()),
                            r => r,
                        }
                    }
                };
                mkdir(remote.clone()).await?;
                created.store(true, Ordering::SeqCst);
                let path_of = |rel: &[String]| {
                    rel.iter()
                        .fold(remote.clone(), |acc, part| remote_join(&acc, part))
                };
                for dir in &tree.dirs {
                    if cancel.is_cancelled() {
                        return Err(SshError::Cancelled);
                    }
                    mkdir(path_of(dir)).await?;
                }
                let mut done = 0u64;
                let opts = TransferOptions {
                    overwrite: true,
                    ..TransferOptions::default()
                };
                for file in &tree.files {
                    let n = files
                        .upload(&file.path, &path_of(&file.rel), opts, &cancel, |n| {
                            progress(done + n)
                        })
                        .await?;
                    done += n;
                }
                Ok(remote)
            })
        });
        Ok(self.spawn(
            Job {
                op,
                total: Some(total),
                meta: Some(meta),
                cleanup: None,
            },
            sink,
        ))
    }

    /// Runs a failed, cancelled or paused transfer again under a new id.
    /// Downloads continue in their partial file when one was kept. It must still be remembered and its connection open.
    pub fn retry(
        self: &Arc<Self>,
        id: u64,
        sink: impl FnMut(TransferEvent) + Send + 'static,
    ) -> Result<u64, AppError> {
        let job = {
            let mut r = self.retryable.lock().unwrap_or_else(|e| e.into_inner());
            let i = r
                .iter()
                .position(|(x, _)| *x == id)
                .ok_or(AppError::NotFound)?;
            r.remove(i).map(|(_, j)| j).ok_or(AppError::NotFound)?
        };
        Ok(self.spawn(job, sink))
    }

    fn spawn(
        self: &Arc<Self>,
        job: Job,
        mut sink: impl FnMut(TransferEvent) + Send + 'static,
    ) -> u64 {
        let (id, cancel) = self.register();
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut ev = this.run(&job, &cancel, &mut sink).await;
            let paused = this
                .pausing
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            if paused && matches!(ev, TransferEvent::Cancelled) {
                ev = TransferEvent::Paused;
            }
            if let (Some(rec), Some(meta)) = (&this.recorder, &job.meta) {
                rec(meta, &ev);
            }
            // Partial files stay only while the transfer can continue.
            if matches!(
                ev,
                TransferEvent::Cancelled
                    | TransferEvent::Failed {
                        retryable: false,
                        ..
                    }
            ) && let Some(cleanup) = &job.cleanup
            {
                cleanup();
            }
            if matches!(
                ev,
                TransferEvent::Cancelled
                    | TransferEvent::Paused
                    | TransferEvent::Failed {
                        retryable: true,
                        ..
                    }
            ) {
                this.remember(id, job);
            }
            sink(ev);
            this.finish(id);
        });
        id
    }

    async fn run(
        &self,
        job: &Job,
        cancel: &CancellationToken,
        sink: &mut (impl FnMut(TransferEvent) + Send),
    ) -> TransferEvent {
        let _slot = match Arc::clone(&self.slots).try_acquire_owned() {
            Ok(s) => s,
            Err(_) => {
                sink(TransferEvent::Queued);
                tokio::select! {
                    () = cancel.cancelled() => return TransferEvent::Cancelled,
                    s = Arc::clone(&self.slots).acquire_owned() => match s {
                        Ok(s) => s,
                        Err(_) => return TransferEvent::Cancelled,
                    },
                }
            }
        };
        let mut attempt = 0;
        loop {
            // Progress goes through a channel so the op owns a 'static closure.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
            let mut last = 0u64;
            let progress: Progress = Box::new(move |n| {
                if n.saturating_sub(last) >= PROGRESS_STEP {
                    last = n;
                    let _ = tx.send(n);
                }
            });
            let fut = (job.op)(cancel.clone(), progress);
            tokio::pin!(fut);
            let result = loop {
                tokio::select! {
                    r = &mut fut => break r,
                    Some(n) = rx.recv() => sink(TransferEvent::Progress { transferred: n, total: job.total }),
                }
            };
            while let Ok(n) = rx.try_recv() {
                sink(TransferEvent::Progress {
                    transferred: n,
                    total: job.total,
                });
            }
            match result {
                Ok(destination) => return TransferEvent::Done { destination },
                Err(SshError::Cancelled) => return TransferEvent::Cancelled,
                Err(e) if is_transient(&e) && attempt < AUTO_RETRIES => {
                    let delay = self.retry_delays[attempt as usize];
                    attempt += 1;
                    sink(TransferEvent::Retrying {
                        attempt,
                        max: AUTO_RETRIES,
                        message: e.to_string(),
                    });
                    tokio::select! {
                        () = cancel.cancelled() => return TransferEvent::Cancelled,
                        () = tokio::time::sleep(delay) => {}
                    }
                }
                Err(SshError::DestinationExists(p)) => {
                    return TransferEvent::Failed {
                        message: format!("{p} already exists"),
                        retryable: false,
                    };
                }
                Err(e) => {
                    return TransferEvent::Failed {
                        message: e.to_string(),
                        retryable: true,
                    };
                }
            }
        }
    }

    /// Requests cancellation. A paused (or failed) transfer drops its
    /// partial files, so a retry starts from the beginning. `false` if the
    /// transfer is unknown or done.
    pub fn cancel(&self, id: u64) -> bool {
        if let Some(t) = self
            .active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
        {
            t.cancel();
            return true;
        }
        let r = self.retryable.lock().unwrap_or_else(|e| e.into_inner());
        match r.iter().find(|(x, _)| *x == id) {
            Some((_, job)) => {
                if let Some(cleanup) = &job.cleanup {
                    cleanup();
                }
                true
            }
            None => false,
        }
    }

    /// Stops a running transfer so that [`Transfers::retry`] continues
    /// where it stopped (downloads keep their partial file). It ends with
    /// [`TransferEvent::Paused`]. `false` if the transfer is not running.
    pub fn pause(&self, id: u64) -> bool {
        let active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        match active.get(&id) {
            Some(t) => {
                self.pausing
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(id);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use tokio::sync::mpsc;

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
        let f = serde_json::to_value(TransferEvent::Failed {
            message: "x".into(),
            retryable: true,
        })
        .unwrap();
        assert_eq!(
            f,
            serde_json::json!({"kind": "failed", "message": "x", "retryable": true})
        );
    }

    #[test]
    fn cancel_unknown_is_false() {
        assert!(!Transfers::default().cancel(1));
    }

    fn fast() -> Arc<Transfers> {
        Arc::new(Transfers::new(2, [Duration::from_millis(5); 2]))
    }

    fn sink() -> (
        impl FnMut(TransferEvent) + Send + 'static,
        mpsc::UnboundedReceiver<TransferEvent>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            move |e| {
                let _ = tx.send(e);
            },
            rx,
        )
    }

    async fn last(rx: &mut mpsc::UnboundedReceiver<TransferEvent>) -> Vec<TransferEvent> {
        let mut all = Vec::new();
        while let Some(e) = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("event")
        {
            let end = matches!(
                e,
                TransferEvent::Done { .. }
                    | TransferEvent::Failed { .. }
                    | TransferEvent::Cancelled
                    | TransferEvent::Paused
            );
            all.push(e);
            if end {
                break;
            }
        }
        all
    }

    /// An op that fails `fail` times with `err`, then succeeds.
    fn flaky(fail: usize, err: fn() -> SshError) -> (Job, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&calls);
        let op: Op = Arc::new(move |_cancel, mut progress| {
            let n = c.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                progress(PROGRESS_STEP);
                if n < fail {
                    Err(err())
                } else {
                    Ok("dest".into())
                }
            })
        });
        (
            Job {
                op,
                total: None,
                meta: None,
                cleanup: None,
            },
            calls,
        )
    }

    #[tokio::test]
    async fn transient_errors_are_retried_automatically() {
        let t = fast();
        let (job, calls) = flaky(2, || SshError::Protocol("reset".into()));
        let (s, mut rx) = sink();
        t.spawn(job, s);
        let ev = last(&mut rx).await;
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            ev.iter()
                .filter(|e| matches!(e, TransferEvent::Retrying { .. }))
                .count(),
            2
        );
        assert!(matches!(ev.last(), Some(TransferEvent::Done { .. })));
        assert!(
            ev.iter()
                .any(|e| matches!(e, TransferEvent::Progress { .. }))
        );
    }

    #[tokio::test]
    async fn permanent_errors_fail_at_once_and_can_be_retried_manually() {
        let t = fast();
        let (job, calls) = flaky(1, || SshError::Sftp("permission denied".into()));
        let (s, mut rx) = sink();
        let id = t.spawn(job, s);
        let ev = last(&mut rx).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(
            ev.last(),
            Some(TransferEvent::Failed {
                retryable: true,
                ..
            })
        ));
        while t.active_count() > 0 {
            tokio::task::yield_now().await;
        }
        let (s, mut rx) = sink();
        let again = t.retry(id, s).unwrap();
        assert_ne!(again, id);
        assert!(matches!(
            last(&mut rx).await.last(),
            Some(TransferEvent::Done { .. })
        ));
        let (s, _rx) = sink();
        assert!(matches!(t.retry(id, s), Err(AppError::NotFound)));
    }

    #[tokio::test]
    async fn pause_keeps_partial_files_and_cancel_drops_them() {
        let t = fast();
        let cleaned = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&calls);
        // The first attempt runs until stopped; the next one finishes.
        let op: Op = Arc::new(move |cancel, _progress| {
            let n = c.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if n == 0 {
                    cancel.cancelled().await;
                    Err(SshError::Cancelled)
                } else {
                    Ok("dest".into())
                }
            })
        });
        let cl = Arc::clone(&cleaned);
        let job = Job {
            op,
            total: None,
            meta: None,
            cleanup: Some(Arc::new(move || {
                cl.fetch_add(1, Ordering::SeqCst);
            })),
        };
        let (s, mut rx) = sink();
        let id = t.spawn(job, s);
        while calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        assert!(t.pause(id));
        assert_eq!(last(&mut rx).await.last(), Some(&TransferEvent::Paused));
        while t.active_count() > 0 {
            tokio::task::yield_now().await;
        }
        assert_eq!(cleaned.load(Ordering::SeqCst), 0, "paused keeps partials");
        assert!(!t.pause(id), "only running transfers pause");

        // Cancelling the paused transfer drops its partial files; it can
        // still be retried (from the beginning).
        assert!(t.cancel(id));
        assert_eq!(cleaned.load(Ordering::SeqCst), 1);
        let (s, mut rx) = sink();
        t.retry(id, s).unwrap();
        assert!(matches!(
            last(&mut rx).await.last(),
            Some(TransferEvent::Done { .. })
        ));
    }

    #[tokio::test]
    async fn existing_destination_is_not_retryable() {
        let t = fast();
        let (job, _) = flaky(9, || SshError::DestinationExists("/x".into()));
        let (s, mut rx) = sink();
        let id = t.spawn(job, s);
        assert!(matches!(
            last(&mut rx).await.last(),
            Some(TransferEvent::Failed {
                retryable: false,
                ..
            })
        ));
        while t.active_count() > 0 {
            tokio::task::yield_now().await;
        }
        let (s, _rx) = sink();
        assert!(t.retry(id, s).is_err());
    }

    #[tokio::test]
    async fn at_most_n_run_at_once_and_queued_ones_can_be_cancelled() {
        let t = fast();
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(tokio::sync::Notify::new());
        let op: Op = {
            let (running, peak, gate) = (running.clone(), peak.clone(), gate.clone());
            Arc::new(move |cancel, _p| {
                let (running, peak, gate) = (running.clone(), peak.clone(), gate.clone());
                Box::pin(async move {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::select! {
                        () = gate.notified() => {}
                        () = cancel.cancelled() => {
                            running.fetch_sub(1, Ordering::SeqCst);
                            return Err(SshError::Cancelled);
                        }
                    }
                    running.fetch_sub(1, Ordering::SeqCst);
                    Ok("d".into())
                })
            })
        };
        let mut rxs = Vec::new();
        let mut ids = Vec::new();
        for _ in 0..4 {
            let (s, rx) = sink();
            ids.push(t.spawn(
                Job {
                    op: op.clone(),
                    total: None,
                    meta: None,
                    cleanup: None,
                },
                s,
            ));
            rxs.push(rx);
        }
        while running.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
        // The 4th is queued; cancel it before it starts.
        assert_eq!(rxs[3].recv().await, Some(TransferEvent::Queued));
        assert!(t.cancel(ids[3]));
        assert_eq!(rxs[3].recv().await, Some(TransferEvent::Cancelled));
        // Release the rest one by one.
        for _ in 0..3 {
            gate.notify_one();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        for rx in rxs.iter_mut().take(3) {
            assert!(matches!(
                last(rx).await.last(),
                Some(TransferEvent::Done { .. })
            ));
        }
        assert_eq!(peak.load(Ordering::SeqCst), 2);
    }
}
