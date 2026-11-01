//! One id space for every terminal the UI shows, whatever backs it: a local
//! PTY (terminal-pty) or a remote shell over SSH (protocol-ssh). Both use
//! the same events, ack-based flow control and lifecycle.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use protocol_ssh::{SftpClient, ShellEvent, ShellWriter, SshSession};
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
    Serial(Arc<SerialTerminal>),
    Telnet(Arc<TelnetTerminal>),
    /// An SSH connection used only for files (no shell).
    Files(Arc<SshConnection>),
}

/// A byte stream a Telnet session runs over: TCP, or a tunnel channel.
pub trait TelnetStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin> TelnetStream for T {}
type BoxedStream = Box<dyn TelnetStream>;

struct TelnetTerminal {
    writer: tokio::sync::Mutex<tokio::io::WriteHalf<BoxedStream>>,
    codec: Arc<Mutex<protocol_telnet::Telnet>>,
    gate: Arc<AsyncGate>,
    cancel: CancellationToken,
    /// SSH connection the stream is tunnelled through, kept open.
    _via: Option<Arc<SshSession>>,
}

impl TelnetTerminal {
    async fn send(&self, bytes: &[u8]) -> Result<(), AppError> {
        use tokio::io::AsyncWriteExt;
        self.writer
            .lock()
            .await
            .write_all(bytes)
            .await
            .map_err(|e| AppError::Network(e.to_string()))
    }
}

/// How long a Telnet TCP connect may take.
const TELNET_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

struct SerialTerminal {
    writer: Mutex<serial_console::SerialWriter>,
    gate: Arc<AsyncGate>,
    cancel: CancellationToken,
}

/// One SSH connection, shared by every terminal (split pane) opened on it.
struct SshConnection {
    session: Arc<SshSession>,
    /// Terminals still using the connection; the last one disconnects it.
    users: AtomicUsize,
    /// Created on first use, shared by the file browser and transfers.
    sftp: tokio::sync::Mutex<Option<Arc<SftpClient>>>,
}

struct SshTerminal {
    writer: ShellWriter,
    gate: Arc<AsyncGate>,
    cancel: CancellationToken,
    conn: Arc<SshConnection>,
    released: AtomicBool,
}

impl SshTerminal {
    /// Gives up this terminal's share of the connection exactly once (the
    /// tab may be closed and the remote shell exit at the same time).
    async fn release(&self) {
        if self.released.swap(true, Ordering::SeqCst) {
            return;
        }
        if self.conn.users.fetch_sub(1, Ordering::SeqCst) == 1 {
            let _ = self.conn.session.disconnect().await;
        }
    }
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

    /// Keeps an authenticated SSH connection for file browsing only (no
    /// shell). The id works with [`TerminalHub::sftp`] and
    /// [`TerminalHub::close`]; terminal operations are refused.
    pub fn open_files(&self, session: Arc<SshSession>) -> u64 {
        let conn = Arc::new(SshConnection {
            session,
            users: AtomicUsize::new(1),
            sftp: tokio::sync::Mutex::new(None),
        });
        let id = self.alloc();
        self.lock().insert(id, Entry::Files(conn));
        id
    }

    /// Opens an interactive shell on an authenticated SSH session. The hub
    /// owns the session from now on; it is disconnected when the last
    /// terminal using it closes.
    pub async fn open_ssh(
        &self,
        session: Arc<SshSession>,
        size: TerminalSize,
        sink: impl TerminalEventSink,
    ) -> Result<u64, AppError> {
        let conn = Arc::new(SshConnection {
            session,
            users: AtomicUsize::new(0),
            sftp: tokio::sync::Mutex::new(None),
        });
        self.open_shell_on(conn, size, sink).await
    }

    /// Opens another shell on the connection behind SSH terminal `id`
    /// (split pane): no new login, no new host-key check.
    pub async fn open_ssh_sibling(
        &self,
        id: u64,
        size: TerminalSize,
        sink: impl TerminalEventSink,
    ) -> Result<u64, AppError> {
        let conn = match self.target(id)? {
            Target::Ssh(t) => Arc::clone(&t.conn),
            // A shell on a file browser's connection: no second login.
            Target::Files(c) => c,
            Target::Local(_) | Target::Serial(_) | Target::Telnet(_) => {
                return Err(AppError::invalid("terminal", "not an SSH terminal"));
            }
        };
        self.open_shell_on(conn, size, sink).await
    }

    async fn open_shell_on(
        &self,
        conn: Arc<SshConnection>,
        size: TerminalSize,
        mut sink: impl TerminalEventSink,
    ) -> Result<u64, AppError> {
        conn.users.fetch_add(1, Ordering::SeqCst);
        let shell = match conn
            .session
            .open_shell(u32::from(size.cols()), u32::from(size.rows()))
            .await
        {
            Ok(s) => s,
            Err(e) => {
                if conn.users.fetch_sub(1, Ordering::SeqCst) == 1 {
                    let _ = conn.session.disconnect().await;
                }
                return Err(e.into());
            }
        };
        let (mut reader, writer) = shell.split();
        let id = self.alloc();
        let gate = Arc::new(AsyncGate::default());
        let cancel = CancellationToken::new();
        let term = Arc::new(SshTerminal {
            writer,
            gate: Arc::clone(&gate),
            cancel: cancel.clone(),
            conn,
            released: AtomicBool::new(false),
        });
        self.lock().insert(id, Entry::Ssh(Arc::clone(&term)));

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
            term.release().await;
        });
        Ok(id)
    }

    /// The SSH connection behind terminal `id` (for tunnels).
    pub fn ssh_session(&self, id: u64) -> Result<Arc<SshSession>, AppError> {
        match self.target(id)? {
            Target::Ssh(t) => Ok(Arc::clone(&t.conn.session)),
            Target::Files(c) => Ok(Arc::clone(&c.session)),
            Target::Local(_) | Target::Serial(_) | Target::Telnet(_) => {
                Err(AppError::invalid("terminal", "not an SSH terminal"))
            }
        }
    }

    /// Opens a serial console. Blocking (opens the device); call off the UI
    /// thread. The terminal ends when closed or when the device goes away.
    /// Opens a Telnet connection (unencrypted) to `host:port`, directly or
    /// through `via` (a `direct-tcpip` channel of that SSH connection, so
    /// only the last leg is unencrypted).
    pub async fn open_telnet(
        &self,
        host: &str,
        port: u16,
        via: Option<Arc<SshSession>>,
        size: TerminalSize,
        sink: impl TerminalEventSink,
    ) -> Result<u64, AppError> {
        let stream: BoxedStream = match &via {
            None => {
                let tcp = tokio::time::timeout(
                    TELNET_CONNECT_TIMEOUT,
                    tokio::net::TcpStream::connect((host, port)),
                )
                .await
                .map_err(|_| AppError::Network(format!("{host}:{port} did not answer")))?
                .map_err(|e| AppError::Network(format!("{host}:{port}: {e}")))?;
                let _ = tcp.set_nodelay(true);
                Box::new(tcp)
            }
            Some(ssh) => Box::new(
                ssh.open_direct_tcpip(host, port, std::net::SocketAddr::from(([127, 0, 0, 1], 0)))
                    .await?,
            ),
        };
        self.open_telnet_stream(stream, via, size, sink)
    }

    fn open_telnet_stream(
        &self,
        stream: BoxedStream,
        via: Option<Arc<SshSession>>,
        size: TerminalSize,
        mut sink: impl TerminalEventSink,
    ) -> Result<u64, AppError> {
        use tokio::io::AsyncReadExt;
        let (mut reader, writer) = tokio::io::split(stream);
        let codec = Arc::new(Mutex::new(protocol_telnet::Telnet::new(
            size.cols(),
            size.rows(),
        )));
        let id = self.alloc();
        let gate = Arc::new(AsyncGate::default());
        let cancel = CancellationToken::new();
        let term = Arc::new(TelnetTerminal {
            writer: tokio::sync::Mutex::new(writer),
            codec: Arc::clone(&codec),
            gate: Arc::clone(&gate),
            cancel: cancel.clone(),
            _via: via,
        });
        self.lock().insert(id, Entry::Telnet(Arc::clone(&term)));
        let entries = Arc::downgrade(&self.entries);
        tokio::spawn(async move {
            let mut buf = vec![0u8; 16 * 1024];
            let mut receiver_alive = true;
            loop {
                if !gate.wait_below(FLOW_HIGH_WATERMARK, &cancel).await {
                    break;
                }
                let n = tokio::select! {
                    () = cancel.cancelled() => break,
                    r = reader.read(&mut buf) => match r {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    },
                };
                let decoded = codec
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .decode(&buf[..n]);
                if !decoded.reply.is_empty() && term.send(&decoded.reply).await.is_err() {
                    break;
                }
                if !decoded.data.is_empty() {
                    gate.add(decoded.data.len());
                    if !sink.emit(TerminalEvent::Output(decoded.data)) {
                        receiver_alive = false;
                        break;
                    }
                }
            }
            if receiver_alive {
                let _ = sink.emit(TerminalEvent::Exited(None));
            }
            if let Some(e) = entries.upgrade() {
                e.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
            }
        });
        Ok(id)
    }

    pub fn open_serial(
        &self,
        cfg: &serial_console::SerialConfig,
        mut sink: impl TerminalEventSink,
    ) -> Result<u64, AppError> {
        let (mut reader, writer) = serial_console::open(cfg)?;
        let id = self.alloc();
        let gate = Arc::new(AsyncGate::default());
        let cancel = CancellationToken::new();
        self.lock().insert(
            id,
            Entry::Serial(Arc::new(SerialTerminal {
                writer: Mutex::new(writer),
                gate: Arc::clone(&gate),
                cancel: cancel.clone(),
            })),
        );
        let entries = Arc::downgrade(&self.entries);
        std::thread::Builder::new()
            .name(format!("serial-{id}"))
            .spawn(move || {
                let mut buf = vec![0u8; 16 * 1024];
                let mut receiver_alive = true;
                while !cancel.is_cancelled() {
                    if gate.in_flight() >= FLOW_HIGH_WATERMARK {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        continue;
                    }
                    match reader.read(&mut buf) {
                        Ok(0) => {}
                        Ok(n) => {
                            gate.add(n);
                            if !sink.emit(TerminalEvent::Output(buf[..n].to_vec())) {
                                receiver_alive = false;
                                break;
                            }
                        }
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::TimedOut
                                    | std::io::ErrorKind::WouldBlock
                                    | std::io::ErrorKind::Interrupted
                            ) => {}
                        Err(_) => break, // device unplugged or failed
                    }
                }
                if receiver_alive {
                    let _ = sink.emit(TerminalEvent::Exited(None));
                }
                if let Some(e) = entries.upgrade() {
                    e.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
                }
            })
            .map_err(|e| AppError::Serial(e.to_string()))?;
        Ok(id)
    }

    /// Whether `id` is backed by an SSH connection (terminal or file
    /// browser), as opposed to a local shell.
    pub fn is_ssh(&self, id: u64) -> Result<bool, AppError> {
        Ok(matches!(
            self.target(id)?,
            Target::Ssh(_) | Target::Files(_)
        ))
    }

    /// Number of terminals sharing the SSH connection behind `id`.
    pub fn connection_users(&self, id: u64) -> Result<usize, AppError> {
        match self.target(id)? {
            Target::Ssh(t) => Ok(t.conn.users.load(Ordering::SeqCst)),
            Target::Files(c) => Ok(c.users.load(Ordering::SeqCst)),
            Target::Local(_) | Target::Serial(_) | Target::Telnet(_) => {
                Err(AppError::invalid("terminal", "not an SSH terminal"))
            }
        }
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
            Target::Serial(t) => {
                let data = data.to_vec();
                blocking(move || {
                    let mut w = t.writer.lock().unwrap_or_else(|e| e.into_inner());
                    w.write_all(&data)
                        .and_then(|()| w.flush())
                        .map_err(|e| AppError::Serial(e.to_string()))
                })
                .await
            }
            Target::Telnet(t) => t.send(&protocol_telnet::Telnet::encode(data)).await,
            Target::Files(_) => Err(AppError::invalid("terminal", "a file browser has no shell")),
        }
    }

    pub async fn resize(&self, id: u64, size: TerminalSize) -> Result<(), AppError> {
        match self.target(id)? {
            Target::Local(lid) => Ok(self.local.resize(lid, size)?),
            Target::Ssh(t) => Ok(t
                .writer
                .resize(u32::from(size.cols()), u32::from(size.rows()))
                .await?),
            // A serial line (or a file browser) has no window size.
            Target::Serial(_) | Target::Files(_) => Ok(()),
            Target::Telnet(t) => {
                let update = t
                    .codec
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .resize(size.cols(), size.rows());
                match update {
                    Some(bytes) => t.send(&bytes).await,
                    None => Ok(()),
                }
            }
        }
    }

    /// SFTP on the connection behind an SSH terminal (opened once, then
    /// reused).
    pub async fn sftp(&self, id: u64) -> Result<Arc<SftpClient>, AppError> {
        let conn = match self.target(id)? {
            Target::Ssh(t) => Arc::clone(&t.conn),
            Target::Files(c) => c,
            Target::Local(_) | Target::Serial(_) | Target::Telnet(_) => {
                return Err(AppError::invalid("terminal", "not an SSH terminal"));
            }
        };
        let mut slot = conn.sftp.lock().await;
        if let Some(c) = slot.as_ref() {
            return Ok(Arc::clone(c));
        }
        let client = Arc::new(conn.session.sftp().await?);
        *slot = Some(Arc::clone(&client));
        Ok(client)
    }

    pub fn ack(&self, id: u64, bytes: usize) -> Result<(), AppError> {
        match self.target(id)? {
            Target::Local(lid) => Ok(self.local.ack(lid, bytes)?),
            Target::Ssh(t) => {
                t.gate.ack(bytes);
                Ok(())
            }
            Target::Serial(t) => {
                t.gate.ack(bytes);
                Ok(())
            }
            Target::Telnet(t) => {
                t.gate.ack(bytes);
                Ok(())
            }
            Target::Files(_) => Ok(()),
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
                t.release().await;
                Ok(())
            }
            Entry::Serial(t) => {
                t.cancel.cancel();
                t.gate.close();
                Ok(())
            }
            Entry::Files(conn) => {
                if conn.users.fetch_sub(1, Ordering::SeqCst) == 1 {
                    let _ = conn.session.disconnect().await;
                }
                Ok(())
            }
            Entry::Telnet(t) => {
                use tokio::io::AsyncWriteExt;
                t.cancel.cancel();
                t.gate.close();
                let _ = t.writer.lock().await.shutdown().await;
                Ok(())
            }
        }
    }

    /// Closes everything (app exit, page reload). Blocking for local PTYs.
    pub fn close_all(&self) {
        let drained: Vec<Entry> = self.lock().drain().map(|(_, e)| e).collect();
        for e in drained {
            match e {
                Entry::Ssh(t) => {
                    t.cancel.cancel();
                    t.gate.close();
                }
                Entry::Serial(t) => {
                    t.cancel.cancel();
                    t.gate.close();
                }
                Entry::Telnet(t) => {
                    t.cancel.cancel();
                    t.gate.close();
                }
                Entry::Local(_) | Entry::Files(_) => {}
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
            Some(Entry::Serial(t)) => Ok(Target::Serial(Arc::clone(t))),
            Some(Entry::Telnet(t)) => Ok(Target::Telnet(Arc::clone(t))),
            Some(Entry::Files(c)) => Ok(Target::Files(Arc::clone(c))),
            None => Err(AppError::NotFound),
        }
    }
}

enum Target {
    Local(TerminalId),
    Ssh(Arc<SshTerminal>),
    Serial(Arc<SerialTerminal>),
    Telnet(Arc<TelnetTerminal>),
    Files(Arc<SshConnection>),
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
    fn in_flight(&self) -> usize {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).0
    }

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
