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

use protocol_ssh::{CancellationToken, SftpClient, SshError, TransferOptions};
use serde::Serialize;
use tokio::sync::Semaphore;

use crate::AppError;
use crate::files::{safe_file_name, unique_destination};

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
}

type Progress = Box<dyn FnMut(u64) + Send>;
type OpFuture = Pin<Box<dyn Future<Output = Result<String, SshError>> + Send>>;
/// One attempt of a transfer: returns the destination on success.
type Op = Arc<dyn Fn(CancellationToken, Progress) -> OpFuture + Send + Sync>;

#[derive(Clone)]
struct Job {
    op: Op,
    total: Option<u64>,
}

pub struct Transfers {
    next: AtomicU64,
    active: Mutex<HashMap<u64, CancellationToken>>,
    retryable: Mutex<VecDeque<(u64, Job)>>,
    slots: Arc<Semaphore>,
    retry_delays: [Duration; AUTO_RETRIES as usize],
}

impl Default for Transfers {
    fn default() -> Self {
        Self::new(
            MAX_CONCURRENT_TRANSFERS,
            [Duration::from_secs(1), Duration::from_secs(3)],
        )
    }
}

/// Joins a remote directory and a validated file name.
pub fn remote_join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
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
            slots: Arc::new(Semaphore::new(concurrent)),
            retry_delays,
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
        sftp: Arc<SftpClient>,
        remote_dir: &str,
        name: &str,
        total: Option<u64>,
        dest_dir: PathBuf,
        sink: impl FnMut(TransferEvent) + Send + 'static,
    ) -> Result<u64, AppError> {
        let name = safe_file_name(name)?.to_owned();
        let remote = remote_join(remote_dir, &name);
        let op: Op = Arc::new(move |cancel, mut progress| {
            let (sftp, remote) = (Arc::clone(&sftp), remote.clone());
            // Chosen per attempt: an earlier attempt never took the name.
            let dest = unique_destination(&dest_dir, &name);
            Box::pin(async move {
                sftp.download(&remote, &dest, TransferOptions::default(), &cancel, |n| {
                    progress(n)
                })
                .await?;
                Ok(dest.to_string_lossy().into_owned())
            })
        });
        Ok(self.spawn(Job { op, total }, sink))
    }

    /// Uploads a local file into `remote_dir`, keeping its name. Refuses to
    /// overwrite an existing remote file.
    pub fn start_upload(
        self: &Arc<Self>,
        sftp: Arc<SftpClient>,
        local: PathBuf,
        remote_dir: &str,
        sink: impl FnMut(TransferEvent) + Send + 'static,
    ) -> Result<u64, AppError> {
        let name = local
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| AppError::invalid("file", "has no name"))?;
        let remote = remote_join(remote_dir, safe_file_name(&name)?);
        let total = std::fs::metadata(&local).ok().map(|m| m.len());
        let op: Op = Arc::new(move |cancel, mut progress| {
            let (sftp, local, remote) = (Arc::clone(&sftp), local.clone(), remote.clone());
            Box::pin(async move {
                sftp.upload(&local, &remote, TransferOptions::default(), &cancel, |n| {
                    progress(n)
                })
                .await?;
                Ok(remote)
            })
        });
        Ok(self.spawn(Job { op, total }, sink))
    }

    /// Runs a failed or cancelled transfer again (from the start) under a
    /// new id. It must still be remembered and its connection open.
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
            let ev = this.run(&job, &cancel, &mut sink).await;
            if matches!(
                ev,
                TransferEvent::Cancelled
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
        (Job { op, total: None }, calls)
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
