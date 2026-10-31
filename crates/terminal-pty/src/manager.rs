//! Owns terminal sessions and their I/O threads.

use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::{Duration, Instant};

use crate::flow::FlowGate;
use crate::{
    ExitInfo, FlowConfig, MAX_INPUT_CHUNK, PtyBackend, PtyControl, PtyError, ShellSpec,
    TerminalEvent, TerminalId, TerminalSize,
};

/// How long a closing terminal's child gets to exit after SIGHUP before it is
/// killed.
const TERMINATE_GRACE: Duration = Duration::from_millis(500);
/// How long to wait for an exit status after the PTY reports EOF.
const EXIT_STATUS_WAIT: Duration = Duration::from_secs(1);
/// How often an idle forwarder checks whether the child has exited.
const EXIT_POLL: Duration = Duration::from_millis(100);
/// After the child exits, stop waiting for PTY EOF once output has been idle
/// this long. Covers descendants that keep the PTY open (e.g. `cmd &`).
const DRAIN_AFTER_EXIT: Duration = Duration::from_secs(1);

/// Receives events for one terminal. Returning `false` means the receiver is
/// gone; the session is then closed.
pub trait TerminalEventSink: Send + 'static {
    fn emit(&mut self, event: TerminalEvent) -> bool;
}

impl<F> TerminalEventSink for F
where
    F: FnMut(TerminalEvent) -> bool + Send + 'static,
{
    fn emit(&mut self, event: TerminalEvent) -> bool {
        self(event)
    }
}

type Control = Arc<Mutex<Box<dyn PtyControl>>>;
type Sessions = Arc<Mutex<HashMap<TerminalId, Session>>>;

struct Session {
    control: Control,
    gate: Arc<FlowGate>,
}

pub struct TerminalManager {
    backend: Arc<dyn PtyBackend>,
    flow: FlowConfig,
    sessions: Sessions,
    next_id: AtomicU64,
}

impl TerminalManager {
    pub fn new(backend: Arc<dyn PtyBackend>, flow: FlowConfig) -> Self {
        Self {
            backend,
            flow,
            sessions: Arc::default(),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn open(
        &self,
        shell: &ShellSpec,
        size: TerminalSize,
        sink: impl TerminalEventSink,
    ) -> Result<TerminalId, PtyError> {
        let spawned = self.backend.spawn(shell, size)?;
        let id = TerminalId::new(self.next_id.fetch_add(1, Ordering::Relaxed));
        let control: Control = Arc::new(Mutex::new(spawned.control));
        let gate = Arc::new(FlowGate::default());

        let (tx, rx) = sync_channel::<Vec<u8>>(self.flow.channel_chunks);
        let read_chunk = self.flow.read_chunk;
        thread::Builder::new()
            .name(format!("pty-read-{id}"))
            .spawn(move || read_loop(spawned.reader, tx, read_chunk))?;

        let fwd = Forwarder {
            id,
            rx,
            sink: Box::new(sink),
            gate: Arc::clone(&gate),
            control: Arc::clone(&control),
            sessions: Arc::downgrade(&self.sessions),
            flow: self.flow,
        };
        lock(&self.sessions).insert(id, Session { control, gate });
        thread::Builder::new()
            .name(format!("pty-fwd-{id}"))
            .spawn(move || fwd.run())?;
        Ok(id)
    }

    pub fn write(&self, id: TerminalId, data: &[u8]) -> Result<(), PtyError> {
        if data.len() > MAX_INPUT_CHUNK {
            return Err(PtyError::InputTooLarge {
                len: data.len(),
                max: MAX_INPUT_CHUNK,
            });
        }
        let control = self.control(id)?;
        let mut c = lock(&control);
        c.write_all(data)
    }

    pub fn resize(&self, id: TerminalId, size: TerminalSize) -> Result<(), PtyError> {
        let control = self.control(id)?;
        let c = lock(&control);
        c.resize(size)
    }

    /// Acknowledge that the UI has processed `bytes` of output.
    pub fn ack(&self, id: TerminalId, bytes: usize) -> Result<(), PtyError> {
        let sessions = lock(&self.sessions);
        let s = sessions.get(&id).ok_or(PtyError::UnknownTerminal(id))?;
        s.gate.ack(bytes);
        Ok(())
    }

    pub fn pid(&self, id: TerminalId) -> Result<Option<u32>, PtyError> {
        let control = self.control(id)?;
        let c = lock(&control);
        Ok(c.pid())
    }

    /// Terminate the child and release the session. Blocks for up to the
    /// termination grace period; call off the UI thread.
    pub fn close(&self, id: TerminalId) -> Result<(), PtyError> {
        let session = lock(&self.sessions)
            .remove(&id)
            .ok_or(PtyError::UnknownTerminal(id))?;
        shutdown(session)
    }

    pub fn close_all(&self) {
        let drained: Vec<Session> = lock(&self.sessions).drain().map(|(_, s)| s).collect();
        for s in drained {
            let _ = shutdown(s);
        }
    }

    pub fn len(&self) -> usize {
        lock(&self.sessions).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn control(&self, id: TerminalId) -> Result<Control, PtyError> {
        lock(&self.sessions)
            .get(&id)
            .map(|s| Arc::clone(&s.control))
            .ok_or(PtyError::UnknownTerminal(id))
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        self.close_all();
    }
}

fn shutdown(session: Session) -> Result<(), PtyError> {
    let result = lock(&session.control).terminate(TERMINATE_GRACE);
    session.gate.close();
    result
}

fn read_loop(mut reader: Box<dyn Read + Send>, tx: SyncSender<Vec<u8>>, chunk: usize) {
    let mut buf = vec![0u8; chunk];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                // Blocks while the channel is full: this is the backpressure.
                if tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            // EIO is how macOS/Linux report that the slave side closed.
            Err(_) => break,
        }
    }
}

struct Forwarder {
    id: TerminalId,
    rx: Receiver<Vec<u8>>,
    sink: Box<dyn TerminalEventSink>,
    gate: Arc<FlowGate>,
    control: Control,
    sessions: Weak<Mutex<HashMap<TerminalId, Session>>>,
    flow: FlowConfig,
}

impl Forwarder {
    fn run(mut self) {
        let mut receiver_alive = true;
        let mut exited_at: Option<Instant> = None;
        loop {
            let mut batch = match self.rx.recv_timeout(EXIT_POLL) {
                Ok(chunk) => chunk,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    match exited_at {
                        None if child_exited(&self.control) => {
                            exited_at = Some(Instant::now());
                            release_pty_async(&self.control);
                        }
                        Some(t) if t.elapsed() >= DRAIN_AFTER_EXIT => break,
                        _ => {}
                    }
                    continue;
                }
            };
            // Coalesce whatever is already queued to cut per-event IPC cost.
            while batch.len() < self.flow.max_batch {
                match self.rx.try_recv() {
                    Ok(more) => batch.extend_from_slice(&more),
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
            }
            if !self.gate.reserve(batch.len(), self.flow.high_watermark) {
                break; // closed by the manager
            }
            if !self.sink.emit(TerminalEvent::Output(batch)) {
                receiver_alive = false;
                break;
            }
        }
        // Dropping the receiver unblocks a reader stuck on a full channel.
        drop(self.rx);

        let exit = wait_for_exit(&self.control, EXIT_STATUS_WAIT);
        if receiver_alive {
            let _ = self.sink.emit(TerminalEvent::Exited(exit));
        }
        if let Some(sessions) = self.sessions.upgrade() {
            let removed = lock(&sessions).remove(&self.id);
            if let Some(s) = removed {
                // Child exited on its own or the UI went away.
                let _ = shutdown(s);
            }
        }
    }
}

fn child_exited(control: &Control) -> bool {
    matches!(lock(control).try_wait(), Ok(Some(_)))
}

/// Releases the PTY on a helper thread: closing a ConPTY can block until its
/// output is drained, and the forwarder must keep draining meanwhile.
fn release_pty_async(control: &Control) {
    let control = Arc::clone(control);
    let _ = thread::Builder::new()
        .name("pty-release".into())
        .spawn(move || lock(&control).release_pty());
}

fn wait_for_exit(control: &Control, timeout: Duration) -> Option<ExitInfo> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(Some(info)) = lock(control).try_wait() {
            return Some(info);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn lock<T: ?Sized>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
