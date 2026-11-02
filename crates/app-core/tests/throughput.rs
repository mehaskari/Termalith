//! Terminal output throughput through the hub, with flow control and
//! acknowledgements as the UI does them. Prints MB/s; fails only far below
//! any normal machine so CI catches large regressions without flaking.
#![cfg(unix)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use app_core::TerminalHub;
use terminal_pty::{ShellSpec, TerminalEvent, TerminalSize};

const BYTES: usize = 64 * 1024 * 1024;
/// Shared CI runners measured 4 MB/s (macOS Intel) and more; a broken
/// flow-control path (stalls, per-byte work) is far below this.
const MIN_MB_PER_S: f64 = 1.0;

#[tokio::test(flavor = "multi_thread")]
async fn local_terminal_output_throughput() {
    let hub = Arc::new(TerminalHub::with_default_flow());
    let received = Arc::new(AtomicUsize::new(0));
    let r = received.clone();
    let (ack_tx, mut ack_rx) = tokio::sync::mpsc::unbounded_channel::<usize>();
    // `head` writes a fixed amount of printable data and exits.
    let mut shell = ShellSpec::new("/bin/sh");
    shell.args = vec![
        "-c".into(),
        format!("head -c {BYTES} /dev/zero | tr '\\0' 'a'").into(),
    ];
    let start = Instant::now();
    let id = hub
        .open_local(&shell, TerminalSize::new(200, 50).unwrap(), move |ev| {
            if let TerminalEvent::Output(b) = ev {
                r.fetch_add(b.len(), Ordering::Relaxed);
                let _ = ack_tx.send(b.len());
            }
            true
        })
        .unwrap();
    // Acknowledge like the UI does after rendering.
    let deadline = start + Duration::from_secs(120);
    while received.load(Ordering::Relaxed) < BYTES {
        match tokio::time::timeout(Duration::from_secs(10), ack_rx.recv()).await {
            Ok(Some(n)) => {
                let _ = hub.ack(id, n);
            }
            _ => panic!(
                "stalled after {} of {BYTES} bytes",
                received.load(Ordering::Relaxed)
            ),
        }
        assert!(Instant::now() < deadline, "too slow");
    }
    let secs = start.elapsed().as_secs_f64();
    let mb_s = BYTES as f64 / 1_000_000.0 / secs;
    eprintln!("terminal throughput: {mb_s:.1} MB/s ({BYTES} bytes in {secs:.2} s)");
    assert!(
        mb_s > MIN_MB_PER_S,
        "{mb_s:.1} MB/s is below {MIN_MB_PER_S} MB/s"
    );
    let _ = hub.close(id).await;
}
