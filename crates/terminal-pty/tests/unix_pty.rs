//! End-to-end tests against the real OS PTY. Unix only; Windows equivalents
//! are added with the Phase 0b native runner.
#![cfg(unix)]

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use terminal_pty::{
    ExitInfo, FlowConfig, MAX_INPUT_CHUNK, NativePtyBackend, PtyError, ShellSpec, TerminalEvent,
    TerminalId, TerminalManager, TerminalSize, default_shell,
};

const TIMEOUT: Duration = Duration::from_secs(10);

struct Collector {
    output: Arc<Mutex<Vec<u8>>>,
    exit: Receiver<Option<ExitInfo>>,
}

impl Collector {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }

    fn len(&self) -> usize {
        self.output.lock().unwrap().len()
    }

    fn wait_for(&self, needle: &str) {
        let start = Instant::now();
        while !self.text().contains(needle) {
            assert!(
                start.elapsed() < TIMEOUT,
                "timed out waiting for {needle:?}; got {:?}",
                self.text()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_exit(&self) -> Option<ExitInfo> {
        self.exit.recv_timeout(TIMEOUT).expect("no Exited event")
    }
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

fn sh() -> ShellSpec {
    // -i forces an interactive shell; PS1 makes prompts easy to ignore.
    ShellSpec::new("/bin/sh").arg("-i").env("PS1", "$ ")
}

fn process_gone(pid: u32) -> bool {
    kill(Pid::from_raw(pid as i32), None) == Err(Errno::ESRCH)
}

#[test]
fn non_interactive_output_and_exit_code() {
    let m = manager(unthrottled());
    let spec = ShellSpec::new("/bin/sh")
        .arg("-c")
        .arg("printf 'hello-%s' termalith; exit 7");
    let (_id, c) = open(&m, &spec);
    c.wait_for("hello-termalith");
    assert_eq!(c.wait_exit().map(|e| e.code), Some(7));
}

#[test]
fn interactive_input_reaches_shell() {
    let m = manager(unthrottled());
    let (id, c) = open(&m, &sh());
    m.write(id, b"echo $((40 + 2))\n").unwrap();
    c.wait_for("42");
    m.write(id, b"exit 3\n").unwrap();
    assert_eq!(c.wait_exit().map(|e| e.code), Some(3));
    // Session is removed after a natural exit.
    let start = Instant::now();
    while !m.is_empty() {
        assert!(start.elapsed() < TIMEOUT);
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn utf8_round_trip() {
    let m = manager(unthrottled());
    let (id, c) = open(&m, &sh().env("LC_ALL", "en_US.UTF-8"));
    m.write(id, "printf '%s\\n' 'سلام-✓-😀'\n".as_bytes())
        .unwrap();
    c.wait_for("سلام-✓-😀\r\n");
    m.close(id).unwrap();
}

#[test]
fn resize_is_visible_to_child() {
    let m = manager(unthrottled());
    let (id, c) = open(&m, &sh());
    m.write(id, b"stty size\n").unwrap();
    c.wait_for("24 80");
    m.resize(id, size(132, 43)).unwrap();
    m.write(id, b"stty size\n").unwrap();
    c.wait_for("43 132");
    m.close(id).unwrap();
}

#[test]
fn close_terminates_and_reaps_child() {
    let m = manager(unthrottled());
    let (id, c) = open(&m, &sh());
    let pid = m.pid(id).unwrap().expect("pid");
    assert!(!process_gone(pid));
    m.close(id).unwrap();
    assert!(process_gone(pid), "shell {pid} still exists after close");
    assert!(m.is_empty());
    // Exited is still delivered as the final event.
    let _ = c.wait_exit();
}

#[test]
fn close_escalates_when_sighup_is_ignored() {
    let m = manager(unthrottled());
    let spec = ShellSpec::new("/bin/sh")
        .arg("-c")
        .arg("trap '' HUP; echo ready; while :; do sleep 1; done");
    let (id, c) = open(&m, &spec);
    c.wait_for("ready");
    let pid = m.pid(id).unwrap().expect("pid");
    let start = Instant::now();
    m.close(id).unwrap();
    // SIGKILL leaves a zombie until reaped; the forwarder's wait reaps it.
    while !process_gone(pid) {
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "pid {pid} survived"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn exit_is_reported_while_background_job_holds_pty() {
    let m = manager(unthrottled());
    // The shell exits at once; `sleep` inherits the PTY slave and keeps it
    // open. On Linux EOF on the master would not arrive for 30 s. (macOS
    // revokes the tty when the session leader exits, so this passes there
    // regardless; it is a regression test for Linux.)
    let spec = ShellSpec::new("/bin/sh")
        .arg("-c")
        .arg("sleep 30 & echo started; exit 4");
    let (_id, c) = open(&m, &spec);
    c.wait_for("started");
    let start = Instant::now();
    assert_eq!(c.wait_exit().map(|e| e.code), Some(4));
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn drop_closes_all_sessions() {
    let m = manager(unthrottled());
    let (a, _ca) = open(&m, &sh());
    let (b, _cb) = open(&m, &sh());
    let pids = [m.pid(a).unwrap().unwrap(), m.pid(b).unwrap().unwrap()];
    drop(m);
    for pid in pids {
        assert!(process_gone(pid), "pid {pid} survived manager drop");
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
    let (id, c) = open(&m, &ShellSpec::new("/usr/bin/yes"));

    thread::sleep(Duration::from_millis(500));
    let first = c.len();
    assert!(first > 0, "no output received");
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
        thread::sleep(Duration::from_millis(10));
    }
    m.close(id).unwrap();
}

#[test]
fn rejects_oversized_input_and_unknown_ids() {
    let m = manager(unthrottled());
    let (id, _c) = open(&m, &sh());
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
    assert!(matches!(m.close(id), Err(PtyError::UnknownTerminal(_))));
}

#[test]
fn default_shell_starts_on_this_host() {
    let m = manager(unthrottled());
    let spec = default_shell().expect("default shell");
    assert!(spec.program.is_absolute());
    let (id, c) = open(&m, &spec);
    m.write(id, b"echo marker-$((6 * 7))\n").unwrap();
    c.wait_for("marker-42");
    m.write(id, b"exit\n").unwrap();
    let _ = c.wait_exit();
}
