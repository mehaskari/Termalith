//! End-to-end tests against Windows ConPTY. Run on a native Windows runner.
#![cfg(windows)]

use std::process::Command;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use terminal_pty::{
    ExitInfo, FlowConfig, MAX_INPUT_CHUNK, NativePtyBackend, PtyError, ShellSpec, TerminalEvent,
    TerminalId, TerminalManager, TerminalSize, default_shell,
};

const TIMEOUT: Duration = Duration::from_secs(20);

struct Collector {
    output: Arc<Mutex<Vec<u8>>>,
    exit: Receiver<Option<ExitInfo>>,
}

impl Collector {
    /// Output with escape sequences and all whitespace removed. ConPTY
    /// repaints the screen and may replace runs of spaces with cursor moves,
    /// so assertions compare this normalised form.
    fn plain(&self) -> String {
        let raw = String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned();
        strip_escapes(&raw)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect()
    }

    fn len(&self) -> usize {
        self.output.lock().unwrap().len()
    }

    fn wait_for(&self, needle: &str) {
        let start = Instant::now();
        while !self.plain().contains(needle) {
            assert!(
                start.elapsed() < TIMEOUT,
                "timed out waiting for {needle:?}; got {:?}",
                self.plain()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_exit(&self) -> Option<ExitInfo> {
        self.exit.recv_timeout(TIMEOUT).expect("no Exited event")
    }
}

/// Removes CSI (`ESC [ ... final`) and OSC (`ESC ] ... BEL|ST`) sequences.
fn strip_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn manager(flow: FlowConfig) -> TerminalManager {
    TerminalManager::new(Arc::new(NativePtyBackend), flow)
}

fn unthrottled() -> FlowConfig {
    FlowConfig {
        high_watermark: usize::MAX,
        ..FlowConfig::default()
    }
}

fn open(m: &TerminalManager, shell: &ShellSpec) -> (TerminalId, Collector) {
    let output = Arc::new(Mutex::new(Vec::new()));
    let (tx, exit) = channel();
    let out = Arc::clone(&output);
    let id = m
        .open(shell, size(80, 24), move |ev| {
            match ev {
                TerminalEvent::Output(b) => out.lock().unwrap().extend_from_slice(&b),
                TerminalEvent::Exited(info) => {
                    let _ = tx.send(info);
                }
            }
            true
        })
        .expect("open");
    (id, Collector { output, exit })
}

fn size(cols: u16, rows: u16) -> TerminalSize {
    TerminalSize::new(cols, rows).unwrap()
}

fn cmd() -> ShellSpec {
    let comspec = std::env::var_os("COMSPEC").expect("COMSPEC");
    ShellSpec::new(comspec)
}

fn process_gone(pid: u32) -> bool {
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .expect("tasklist");
    !String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
}

fn wait_gone(pid: u32) {
    let start = Instant::now();
    while !process_gone(pid) {
        assert!(start.elapsed() < TIMEOUT, "pid {pid} still running");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn non_interactive_output_and_exit_code() {
    let m = manager(unthrottled());
    let spec = cmd().arg("/C").arg("echo hello-termalith& exit /b 7");
    let (_id, c) = open(&m, &spec);
    c.wait_for("hello-termalith");
    // Requires the forwarder to release the ConPTY after the child exits;
    // otherwise the output pipe never reports EOF.
    assert_eq!(c.wait_exit().map(|e| e.code), Some(7));
}

#[test]
fn interactive_input_reaches_shell() {
    let m = manager(unthrottled());
    let (id, c) = open(&m, &cmd());
    m.write(id, b"set /a 40+2\r").unwrap();
    c.wait_for("42");
    m.write(id, b"exit 3\r").unwrap();
    assert_eq!(c.wait_exit().map(|e| e.code), Some(3));
    let start = Instant::now();
    while !m.is_empty() {
        assert!(start.elapsed() < TIMEOUT, "session not removed after exit");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn resize_is_visible_to_child() {
    let m = manager(unthrottled());
    let (id, c) = open(&m, &cmd());
    m.write(id, b"mode con\r").unwrap();
    c.wait_for("Columns:80");
    m.resize(id, size(132, 43)).unwrap();
    m.write(id, b"mode con\r").unwrap();
    c.wait_for("Columns:132");
    m.close(id).unwrap();
}

#[test]
fn close_terminates_child() {
    let m = manager(unthrottled());
    let (id, c) = open(&m, &cmd());
    c.wait_for(">"); // prompt
    let pid = m.pid(id).unwrap().expect("pid");
    assert!(!process_gone(pid));
    m.close(id).unwrap();
    wait_gone(pid);
    assert!(m.is_empty());
    let _ = c.wait_exit();
}

#[test]
fn drop_closes_all_sessions() {
    let m = manager(unthrottled());
    let (a, _ca) = open(&m, &cmd());
    let (b, _cb) = open(&m, &cmd());
    let pids = [m.pid(a).unwrap().unwrap(), m.pid(b).unwrap().unwrap()];
    drop(m);
    for pid in pids {
        wait_gone(pid);
    }
}

#[test]
fn unacknowledged_output_is_bounded() {
    let flow = FlowConfig {
        read_chunk: 4096,
        channel_chunks: 4,
        max_batch: 16 * 1024,
        high_watermark: 64 * 1024,
    };
    let m = manager(flow);
    let spec = cmd()
        .arg("/C")
        .arg("for /L %i in (0,0,1) do @echo yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy");
    let (id, c) = open(&m, &spec);

    let start = Instant::now();
    while c.len() < flow.high_watermark {
        assert!(
            start.elapsed() < TIMEOUT,
            "output never reached the watermark"
        );
        thread::sleep(Duration::from_millis(20));
    }
    thread::sleep(Duration::from_millis(500));
    let first = c.len();
    assert!(
        first <= flow.high_watermark + flow.max_batch,
        "received {first} bytes without any ack"
    );
    thread::sleep(Duration::from_millis(300));
    assert_eq!(c.len(), first, "output kept flowing without acks");

    m.ack(id, first).unwrap();
    let start = Instant::now();
    while c.len() == first {
        assert!(start.elapsed() < TIMEOUT, "ack did not resume output");
        thread::sleep(Duration::from_millis(20));
    }
    m.close(id).unwrap();
}

#[test]
fn rejects_oversized_input_and_unknown_ids() {
    let m = manager(unthrottled());
    let (id, _c) = open(&m, &cmd());
    let big = vec![b'a'; MAX_INPUT_CHUNK + 1];
    assert!(matches!(
        m.write(id, &big),
        Err(PtyError::InputTooLarge { .. })
    ));
    m.close(id).unwrap();
    assert!(matches!(
        m.write(id, b"x"),
        Err(PtyError::UnknownTerminal(_))
    ));
}

#[test]
fn default_shell_starts_on_this_host() {
    let m = manager(unthrottled());
    let spec = default_shell().expect("default shell");
    assert!(spec.program.is_absolute());
    let (id, c) = open(&m, &spec);
    m.write(id, b"set /a 6*7+1000\r").unwrap();
    c.wait_for("1042");
    m.write(id, b"exit\r").unwrap();
    let _ = c.wait_exit();
}
