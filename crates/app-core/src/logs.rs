//! Opt-in session logging: a terminal's output saved as plain text.
//!
//! Escape sequences are stripped so the file reads like the screen. Writing
//! happens on a dedicated thread behind a bounded queue, so a slow disk can
//! never stall terminal output; if the queue is full the bytes are counted
//! and a marker is written instead. Only output is logged, never typed
//! input (passwords typed at no-echo prompts are not captured).

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use terminal_pty::{TerminalEvent, TerminalEventSink};

use crate::AppError;

const QUEUE_CHUNKS: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogInfo {
    pub path: PathBuf,
    /// Output bytes received since logging started (before stripping).
    pub bytes: u64,
    /// Bytes that could not be queued (disk too slow).
    pub dropped: u64,
}

enum Msg {
    Data(Vec<u8>),
    Stop,
}

struct Writer {
    path: PathBuf,
    tx: SyncSender<Msg>,
    bytes: AtomicU64,
    dropped: Arc<AtomicU64>,
}

impl Writer {
    fn info(&self) -> LogInfo {
        LogInfo {
            path: self.path.clone(),
            bytes: self.bytes.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // Blocking send is fine: the thread drains quickly and exits.
        let _ = self.tx.send(Msg::Stop);
    }
}

/// Sits between a terminal and its UI sink; logs while a writer is set.
#[derive(Default)]
pub struct LogTap {
    writer: Mutex<Option<Writer>>,
}

impl LogTap {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Writer>> {
        self.writer.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn record(&self, data: &[u8]) {
        let guard = self.lock();
        let Some(w) = guard.as_ref() else { return };
        w.bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
        match w.tx.try_send(Msg::Data(data.to_vec())) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                w.dropped.fetch_add(data.len() as u64, Ordering::Relaxed);
            }
        }
    }

    fn stop(&self) -> Option<LogInfo> {
        self.lock().take().map(|w| w.info())
    }
}

/// Wraps `sink` so output also reaches `tap`. Logging ends with the session.
pub fn tapped(tap: Arc<LogTap>, mut sink: impl TerminalEventSink) -> impl TerminalEventSink {
    move |ev: TerminalEvent| {
        match &ev {
            TerminalEvent::Output(b) => tap.record(b),
            TerminalEvent::Exited(_) => {
                tap.stop();
            }
        }
        sink.emit(ev)
    }
}

/// Log files of open terminals, by terminal id.
pub struct TerminalLogs {
    dir: PathBuf,
    taps: Mutex<HashMap<u64, Arc<LogTap>>>,
}

impl TerminalLogs {
    /// Log files are created in `dir` (created on first use).
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            taps: Mutex::default(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn taps(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Arc<LogTap>>> {
        self.taps.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Associates a tap (already wrapped around the terminal's sink) with
    /// terminal `id`.
    pub fn register(&self, id: u64, tap: Arc<LogTap>) {
        self.taps().insert(id, tap);
    }

    /// Stops logging and forgets terminal `id` (it closed).
    pub fn forget(&self, id: u64) {
        if let Some(t) = self.taps().remove(&id) {
            t.stop();
        }
    }

    pub fn status(&self, id: u64) -> Result<Option<LogInfo>, AppError> {
        let tap = self.taps().get(&id).cloned().ok_or(AppError::NotFound)?;
        Ok(tap.lock().as_ref().map(Writer::info))
    }

    /// Starts logging terminal `id` to a new file named after `label`.
    /// Already logging: returns the current file.
    pub fn start(&self, id: u64, label: &str) -> Result<LogInfo, AppError> {
        let tap = self.taps().get(&id).cloned().ok_or(AppError::NotFound)?;
        let mut slot = tap.lock();
        if let Some(w) = slot.as_ref() {
            return Ok(w.info());
        }
        std::fs::create_dir_all(&self.dir).map_err(io_err)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let (path, file) = create_log_file(&self.dir, &file_name(now, label))?;
        let mut out = BufWriter::new(file);
        writeln!(
            out,
            "# Termalith session log: {}\n# Started {} UTC\n",
            sanitize_line(label),
            timestamp(now)
        )
        .map_err(io_err)?;
        let (tx, rx) = sync_channel::<Msg>(QUEUE_CHUNKS);
        let dropped = Arc::new(AtomicU64::new(0));
        let seen_dropped = Arc::clone(&dropped);
        std::thread::Builder::new()
            .name(format!("log-{id}"))
            .spawn(move || {
                let mut strip = AnsiStripper::default();
                let mut reported = 0;
                let mut text = Vec::new();
                while let Ok(msg) = rx.recv() {
                    let Msg::Data(bytes) = msg else { break };
                    let lost = seen_dropped.load(Ordering::Relaxed);
                    if lost > reported {
                        let _ = writeln!(
                            out,
                            "\n[termalith: {} bytes of output were not logged]",
                            lost - reported
                        );
                        reported = lost;
                    }
                    text.clear();
                    strip.push(&bytes, &mut text);
                    if out.write_all(&text).is_err() {
                        break;
                    }
                    if rx
                        .try_recv()
                        .is_err_and(|e| e == std::sync::mpsc::TryRecvError::Empty)
                    {
                        let _ = out.flush();
                    }
                }
                let _ = writeln!(out, "\n# Ended {} UTC", timestamp(unix_now()));
                let _ = out.flush();
            })
            .map_err(io_err)?;
        let w = Writer {
            path,
            tx,
            bytes: AtomicU64::new(0),
            dropped,
        };
        let info = w.info();
        *slot = Some(w);
        Ok(info)
    }

    /// Stops logging terminal `id`; returns the finished file, if any.
    pub fn stop(&self, id: u64) -> Result<Option<LogInfo>, AppError> {
        let tap = self.taps().get(&id).cloned().ok_or(AppError::NotFound)?;
        Ok(tap.stop())
    }

    /// Stops every log (application exit).
    pub fn stop_all(&self) {
        for t in self.taps().drain().map(|(_, t)| t) {
            t.stop();
        }
    }
}

fn io_err(e: std::io::Error) -> AppError {
    AppError::invalid("session log", e.to_string())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Creates a new file readable only by the user (logs may hold sensitive
/// output). Never overwrites.
fn create_log_file(dir: &Path, name: &str) -> Result<(PathBuf, File), AppError> {
    let path = crate::unique_destination(dir, name);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts.open(&path).map_err(io_err)?;
    Ok((path, file))
}

fn sanitize_line(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// `2026-11-01_14-03-09_web-01.log`, with unsafe characters replaced.
fn file_name(unix: u64, label: &str) -> String {
    let safe: String = label
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '@') {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    let safe = safe.trim_matches('.');
    let safe = if safe.is_empty() { "terminal" } else { safe };
    format!(
        "{}_{}.log",
        timestamp(unix).replace(' ', "_").replace(':', "-"),
        safe
    )
}

/// `YYYY-MM-DD HH:MM:SS` (UTC) without a date library.
fn timestamp(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    let secs = unix % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum StripState {
    #[default]
    Text,
    Esc,
    Csi,
    /// OSC/DCS/APC/PM/SOS string, until BEL or ESC \.
    Str,
    StrEsc,
    /// Charset designation etc.: one more byte.
    EscArg,
}

/// Removes terminal control sequences; keeps text, newlines and tabs.
/// State carries across chunks (sequences may be split).
#[derive(Default)]
pub struct AnsiStripper {
    state: StripState,
}

impl AnsiStripper {
    pub fn push(&mut self, input: &[u8], out: &mut Vec<u8>) {
        use StripState::*;
        for &b in input {
            self.state = match (self.state, b) {
                (Text, 0x1b) => Esc,
                (Text, b'\n' | b'\t') => {
                    out.push(b);
                    Text
                }
                (Text, 0x9b) => Csi, // 8-bit CSI (only valid outside UTF-8 text; rare)
                (Text, c) if c < 0x20 || c == 0x7f => Text,
                (Text, c) => {
                    out.push(c);
                    Text
                }
                (Esc, b'[') => Csi,
                (Esc, b']' | b'P' | b'_' | b'^' | b'X') => Str,
                (Esc, b'(' | b')' | b'*' | b'+' | b'#' | b'%') => EscArg,
                (Esc, _) => Text,
                (EscArg, _) => Text,
                (Csi, 0x40..=0x7e) => Text,
                (Csi, _) => Csi,
                (Str, 0x07) => Text,
                (Str, 0x1b) => StrEsc,
                (Str, _) => Str,
                (StrEsc, b'\\') => Text,
                (StrEsc, _) => Str,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(chunks: &[&[u8]]) -> String {
        let mut s = AnsiStripper::default();
        let mut out = Vec::new();
        for c in chunks {
            s.push(c, &mut out);
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn strips_sequences_even_when_split() {
        assert_eq!(
            strip(&[b"\x1b[1;32mok\x1b[0m\r\n", b"\x1b]0;title\x07next\n"]),
            "ok\nnext\n"
        );
        assert_eq!(
            strip(&[b"a\x1b[", b"38;5;1", b"mb\x1b]8;;http://x\x1b", b"\\c"]),
            "abc"
        );
        assert_eq!(strip(&[b"\x1b(Bplain\ttab\x07"]), "plain\ttab");
        assert_eq!(strip(&["héllo ✓".as_bytes()]), "héllo ✓");
    }

    #[test]
    fn timestamps_and_names() {
        assert_eq!(timestamp(0), "1970-01-01 00:00:00");
        assert_eq!(timestamp(1_793_486_589), "2026-10-31 22:43:09");
        assert_eq!(timestamp(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(
            file_name(0, "deploy@web 01/../x"),
            "1970-01-01_00-00-00_deploy@web_01_.._x.log"
        );
        assert_eq!(file_name(0, ".."), "1970-01-01_00-00-00_terminal.log");
    }

    #[test]
    fn logs_output_while_enabled_only() {
        let dir = tempfile::tempdir().unwrap();
        let logs = TerminalLogs::new(dir.path().join("logs"));
        let tap = Arc::new(LogTap::default());
        let mut sink = tapped(Arc::clone(&tap), |_ev: TerminalEvent| true);
        logs.register(7, tap);

        sink.emit(TerminalEvent::Output(b"before\n".to_vec()));
        assert_eq!(logs.status(7).unwrap(), None);
        let info = logs.start(7, "web-01").unwrap();
        assert_eq!(
            logs.start(7, "web-01").unwrap().path,
            info.path,
            "idempotent"
        );
        sink.emit(TerminalEvent::Output(b"\x1b[31mhello\x1b[0m\r\n".to_vec()));
        let done = logs.stop(7).unwrap().unwrap();
        assert_eq!(done.bytes, 16);
        sink.emit(TerminalEvent::Output(b"after\n".to_vec()));

        // The writer thread finishes asynchronously.
        let mut body = String::new();
        for _ in 0..100 {
            body = std::fs::read_to_string(&info.path).unwrap();
            if body.contains("# Ended") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            body.starts_with("# Termalith session log: web-01\n"),
            "{body}"
        );
        assert!(body.contains("hello\n"), "{body}");
        assert!(
            !body.contains("before") && !body.contains("after"),
            "{body}"
        );
        assert!(body.contains("# Ended"), "{body}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&info.path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(matches!(logs.status(99), Err(AppError::NotFound)));
        logs.forget(7);
        assert!(logs.status(7).is_err());
    }

    #[test]
    fn exit_ends_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let logs = TerminalLogs::new(dir.path().into());
        let tap = Arc::new(LogTap::default());
        let mut sink = tapped(Arc::clone(&tap), |_ev: TerminalEvent| true);
        logs.register(1, tap);
        logs.start(1, "x").unwrap();
        sink.emit(TerminalEvent::Exited(None));
        assert_eq!(logs.status(1).unwrap(), None);
    }
}
