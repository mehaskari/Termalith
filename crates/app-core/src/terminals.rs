//! One id space for every terminal the UI shows, whatever backs it: a local
//! PTY (terminal-pty) or a remote shell over SSH (protocol-ssh). Both use
//! the same events, ack-based flow control and lifecycle.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use protocol_ssh::{ShellEvent, ShellWriter, SshSession};
use terminal_pty::{
    ExitInfo, FlowConfig, MAX_INPUT_CHUNK, PtyError, ShellSpec, TerminalEvent, TerminalEventSink,
    TerminalId, TerminalManager, TerminalSize,
};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::AppError;

/// SSH output stops being read while this many bytes are unacknowledged
/// (same window as local terminals).
pub const FLOW_HIGH_WATERMARK: usize = 512 * 1024;

enum Entry {
    Local(TerminalId),
    Ssh(Arc<SshTerminal>),
}

struct SshTerminal {
    writer: ShellWriter,
    gate: Arc<AsyncGate>,
    cancel: CancellationToken,
    session: Arc<SshSession>,
}

pub struct TerminalHub {
    local: Arc<TerminalManager>,
    entries: Arc<Mutex<HashMap<u64, Entry>>>,
    next: AtomicU64,
}

impl TerminalHub {
    pub fn new(local: Arc<TerminalManager>) -> Self {
        Self {
            local,
            entries: Arc::default(),
            next: AtomicU64::new(1),
        }
    }

    pub fn with_default_flow() -> Self {
        Self::new(Arc::new(TerminalManager::new(
            Arc::new(terminal_pty::NativePtyBackend),
            FlowConfig::default(),
        )))
    }

    fn alloc(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Opens a local shell. Blocking (spawns a process); call off the UI
    /// thread.
    pub fn open_local(
        &self,
        shell: &ShellSpec,
        size: TerminalSize,
        sink: impl TerminalEventSink,
    ) -> Result<u64, AppError> {
        let id = self.alloc();
        let entries = Arc::downgrade(&self.entries);
        let mut sink = sink;
        let local_id = self.local.open(shell, size, move |ev: TerminalEvent| {
            let exited = matches!(ev, TerminalEvent::Exited(_));
            let alive = sink.emit(ev);
            if exited && let Some(e) = entries.upgrade() {
                e.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
            }
            alive
        })?;
        self.lock().insert(id, Entry::Local(local_id));
        Ok(id)
    }

    /// Opens an interactive shell on an authenticated SSH session. The hub
    /// owns the session from now on; it is disconnected when the terminal
    /// closes.
    pub async fn open_ssh(
        &self,
        session: Arc<SshSession>,
        size: TerminalSize,
        mut sink: impl TerminalEventSink,
    ) -> Result<u64, AppError> {
        let shell = session
            .open_shell(u32::from(size.cols()), u32::from(size.rows()))
            .await?;
        let (mut reader, writer) = shell.split();
        let id = self.alloc();
        let gate = Arc::new(AsyncGate::default());
        let cancel = CancellationToken::new();
        let term = Arc::new(SshTerminal {
            writer,
            gate: Arc::clone(&gate),
            cancel: cancel.clone(),
            session: Arc::clone(&session),
        });
        self.lock().insert(id, Entry::Ssh(term));

        let entries = Arc::downgrade(&self.entries);
        tokio::spawn(async move {
            let mut exit_code = None;
            let mut receiver_alive = true;
            loop {
                if !gate.wait_below(FLOW_HIGH_WATERMARK, &cancel).await {
                    break;
                }
                let ev = tokio::select! {
                    () = cancel.cancelled() => break,
                    ev = reader.next_event() => ev,
                };
                match ev {
                    ShellEvent::Output(bytes) => {
                        gate.add(bytes.len());
                        if !sink.emit(TerminalEvent::Output(bytes)) {
                            receiver_alive = false;
                            break;
                        }
                    }
                    ShellEvent::Exited(code) => exit_code = Some(code),
                    ShellEvent::Closed => break,
                }
            }
            if receiver_alive {
                let _ = sink.emit(TerminalEvent::Exited(
                    exit_code.map(|code| ExitInfo { code, signal: None }),
                ));
            }
            if let Some(e) = entries.upgrade() {
                e.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
            }
            let _ = session.disconnect().await;
        });
        Ok(id)
    }

    pub async fn write(&self, id: u64, data: &[u8]) -> Result<(), AppError> {
        if data.len() > MAX_INPUT_CHUNK {
            return Err(PtyError::InputTooLarge {
                len: data.len(),
                max: MAX_INPUT_CHUNK,
            }
            .into());
        }
        match self.target(id)? {
            Target::Local(lid) => {
                let local = Arc::clone(&self.local);
                let data = data.to_vec();
                blocking(move || local.write(lid, &data).map_err(AppError::from)).await
            }
            Target::Ssh(t) => Ok(t.writer.write(data).await?),
        }
    }

    pub async fn resize(&self, id: u64, size: TerminalSize) -> Result<(), AppError> {
        match self.target(id)? {
            Target::Local(lid) => Ok(self.local.resize(lid, size)?),
            Target::Ssh(t) => Ok(t
                .writer
                .resize(u32::from(size.cols()), u32::from(size.rows()))
                .await?),
        }
    }

    pub fn ack(&self, id: u64, bytes: usize) -> Result<(), AppError> {
        match self.target(id)? {
            Target::Local(lid) => Ok(self.local.ack(lid, bytes)?),
            Target::Ssh(t) => {
                t.gate.ack(bytes);
                Ok(())
            }
        }
    }

    pub async fn close(&self, id: u64) -> Result<(), AppError> {
        let entry = self.lock().remove(&id).ok_or(AppError::NotFound)?;
        match entry {
            Entry::Local(lid) => {
                let local = Arc::clone(&self.local);
                blocking(move || match local.close(lid) {
                    Ok(()) | Err(PtyError::UnknownTerminal(_)) => Ok(()),
                    Err(e) => Err(e.into()),
                })
                .await
            }
            Entry::Ssh(t) => {
                t.cancel.cancel();
                t.gate.close();
                let _ = t.writer.close().await;
                let _ = t.session.disconnect().await;
                Ok(())
            }
        }
    }

    /// Closes everything (app exit, page reload). Blocking for local PTYs.
    pub fn close_all(&self) {
        let drained: Vec<Entry> = self.lock().drain().map(|(_, e)| e).collect();
        for e in drained {
            if let Entry::Ssh(t) = e {
                t.cancel.cancel();
                t.gate.close();
            }
        }
        self.local.close_all();
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn target(&self, id: u64) -> Result<Target, AppError> {
        match self.lock().get(&id) {
            Some(Entry::Local(l)) => Ok(Target::Local(*l)),
            Some(Entry::Ssh(t)) => Ok(Target::Ssh(Arc::clone(t))),
            None => Err(AppError::NotFound),
        }
    }
}

enum Target {
    Local(TerminalId),
    Ssh(Arc<SshTerminal>),
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Db(format!("background task failed: {e}")))?
}

/// Async counterpart of terminal-pty's flow gate.
#[derive(Default)]
struct AsyncGate {
    state: Mutex<(usize, bool)>, // (in_flight, closed)
    notify: Notify,
}

impl AsyncGate {
    fn add(&self, n: usize) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).0 += n;
    }

    fn ack(&self, n: usize) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.0 = st.0.saturating_sub(n);
        drop(st);
        self.notify.notify_waiters();
    }

    fn close(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).1 = true;
        self.notify.notify_waiters();
    }

    /// Waits until fewer than `high` bytes are in flight. `false` if closed
    /// or cancelled.
    async fn wait_below(&self, high: usize, cancel: &CancellationToken) -> bool {
        loop {
            let notified = self.notify.notified();
            {
                let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if st.1 {
                    return false;
                }
                if st.0 < high {
                    return true;
                }
            }
            tokio::select! {
                () = notified => {}
                () = cancel.cancelled() => return false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn gate_blocks_until_ack_and_releases_on_close() {
        let g = Arc::new(AsyncGate::default());
        let cancel = CancellationToken::new();
        g.add(100);
        assert!(g.wait_below(200, &cancel).await);

        g.add(200);
        let g2 = Arc::clone(&g);
        let c2 = cancel.clone();
        let waiter = tokio::spawn(async move { g2.wait_below(200, &c2).await });
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(!waiter.is_finished());
        g.ack(250);
        assert!(waiter.await.unwrap());

        g.add(1000);
        let g3 = Arc::clone(&g);
        let c3 = cancel.clone();
        let waiter = tokio::spawn(async move { g3.wait_below(200, &c3).await });
        tokio::time::sleep(Duration::from_millis(30)).await;
        g.close();
        assert!(!waiter.await.unwrap());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn local_terminal_through_hub() {
        let hub = TerminalHub::with_default_flow();
        let out = Arc::new(Mutex::new(Vec::<u8>::new()));
        let o = Arc::clone(&out);
        let (tx, rx) = std::sync::mpsc::channel();
        let shell = ShellSpec::new("/bin/sh");
        let id = hub
            .open_local(&shell, TerminalSize::new(80, 24).unwrap(), move |ev| {
                match ev {
                    TerminalEvent::Output(b) => o.lock().unwrap().extend_from_slice(&b),
                    TerminalEvent::Exited(_) => {
                        let _ = tx.send(());
                    }
                }
                true
            })
            .unwrap();
        hub.write(id, b"echo hub-$((20+22))\n").await.unwrap();
        let start = std::time::Instant::now();
        while !String::from_utf8_lossy(&out.lock().unwrap()).contains("hub-42") {
            assert!(start.elapsed() < Duration::from_secs(10), "no output");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        hub.close(id).await.unwrap();
        assert!(hub.is_empty());
        rx.recv_timeout(Duration::from_secs(5)).expect("exit event");
        assert!(matches!(hub.write(id, b"x").await, Err(AppError::NotFound)));
    }
}
