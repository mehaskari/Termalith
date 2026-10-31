//! Output flow control.
//!
//! PTY output is read in a dedicated thread and pushed through a bounded
//! channel, so a stalled consumer blocks the reader and, through the kernel
//! PTY buffer, the child process. The [`FlowGate`] extends that backpressure
//! across the IPC boundary: the UI acknowledges bytes once the terminal
//! emulator has rendered them, and the forwarder stops sending while too many
//! bytes are unacknowledged.

use std::sync::{Condvar, Mutex};

/// Tunables for buffering between the PTY and the UI.
#[derive(Debug, Clone, Copy)]
pub struct FlowConfig {
    /// Size of each PTY read.
    pub read_chunk: usize,
    /// Capacity, in chunks, of the reader → forwarder channel.
    pub channel_chunks: usize,
    /// Largest coalesced payload sent to the UI in one event.
    pub max_batch: usize,
    /// Pause sending once this many bytes are unacknowledged.
    pub high_watermark: usize,
}

impl Default for FlowConfig {
    fn default() -> Self {
        Self {
            read_chunk: 16 * 1024,
            channel_chunks: 16,
            max_batch: 64 * 1024,
            high_watermark: 512 * 1024,
        }
    }
}

impl FlowConfig {
    /// Upper bound on output bytes buffered in this crate plus unacknowledged
    /// bytes in flight to the UI, per terminal.
    pub fn max_buffered_bytes(&self) -> usize {
        // channel + one in-progress batch + in-flight window (which may be
        // exceeded by at most one batch).
        self.channel_chunks * self.read_chunk
            + self.max_batch
            + self.high_watermark
            + self.max_batch
    }
}

#[derive(Debug, Default)]
struct GateState {
    in_flight: usize,
    closed: bool,
}

#[derive(Debug, Default)]
pub(crate) struct FlowGate {
    state: Mutex<GateState>,
    cond: Condvar,
}

impl FlowGate {
    /// Blocks until fewer than `high_watermark` bytes are in flight, then
    /// reserves `len` bytes. Returns `false` if the gate was closed.
    pub(crate) fn reserve(&self, len: usize, high_watermark: usize) -> bool {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while !st.closed && st.in_flight >= high_watermark {
            st = self.cond.wait(st).unwrap_or_else(|e| e.into_inner());
        }
        if st.closed {
            return false;
        }
        st.in_flight += len;
        true
    }

    pub(crate) fn ack(&self, len: usize) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.in_flight = st.in_flight.saturating_sub(len);
        self.cond.notify_all();
    }

    pub(crate) fn close(&self) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.closed = true;
        self.cond.notify_all();
    }

    #[cfg(test)]
    pub(crate) fn in_flight(&self) -> usize {
        self.state.lock().unwrap().in_flight
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn reserve_below_watermark_does_not_block() {
        let g = FlowGate::default();
        assert!(g.reserve(100, 1000));
        assert!(g.reserve(100, 1000));
        assert_eq!(g.in_flight(), 200);
    }

    #[test]
    fn reserve_blocks_until_ack() {
        let g = Arc::new(FlowGate::default());
        assert!(g.reserve(1000, 1000));
        let g2 = Arc::clone(&g);
        let t = thread::spawn(move || g2.reserve(10, 1000));
        thread::sleep(Duration::from_millis(50));
        assert!(!t.is_finished(), "reserve should block at the watermark");
        g.ack(600);
        assert!(t.join().unwrap());
        assert_eq!(g.in_flight(), 410);
    }

    #[test]
    fn close_releases_blocked_reserve() {
        let g = Arc::new(FlowGate::default());
        assert!(g.reserve(1000, 1000));
        let g2 = Arc::clone(&g);
        let t = thread::spawn(move || g2.reserve(10, 1000));
        thread::sleep(Duration::from_millis(50));
        g.close();
        assert!(!t.join().unwrap());
    }

    #[test]
    fn over_ack_saturates_at_zero() {
        let g = FlowGate::default();
        g.reserve(10, 1000);
        g.ack(1_000_000);
        assert_eq!(g.in_flight(), 0);
    }
}
