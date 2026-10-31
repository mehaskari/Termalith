use std::fmt;

use crate::PtyError;

/// Largest single input write accepted from the UI. Pastes larger than this
/// must be split by the caller.
pub const MAX_INPUT_CHUNK: usize = 64 * 1024;

const MAX_COLS: u16 = 1000;
const MAX_ROWS: u16 = 500;

/// Opaque, process-unique terminal identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TerminalId(u64);

impl TerminalId {
    pub(crate) fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn as_u64(self) -> u64 {
        self.0
    }

    pub fn from_u64(raw: u64) -> Self {
        Self(raw)
    }
}

impl fmt::Display for TerminalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A validated terminal grid size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    cols: u16,
    rows: u16,
}

impl TerminalSize {
    pub fn new(cols: u16, rows: u16) -> Result<Self, PtyError> {
        if (1..=MAX_COLS).contains(&cols) && (1..=MAX_ROWS).contains(&rows) {
            Ok(Self { cols, rows })
        } else {
            Err(PtyError::InvalidSize { cols, rows })
        }
    }

    pub fn cols(self) -> u16 {
        self.cols
    }

    pub fn rows(self) -> u16 {
        self.rows
    }
}

/// How a terminal's child process ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: u32,
    pub signal: Option<String>,
}

/// Events delivered to the UI layer, in order, for one terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// Raw bytes from the PTY. Untrusted; may be split mid UTF-8 sequence.
    Output(Vec<u8>),
    /// The session ended. Always the last event. `None` if the exit status
    /// could not be determined.
    Exited(Option<ExitInfo>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_accepts_bounds() {
        assert!(TerminalSize::new(1, 1).is_ok());
        assert!(TerminalSize::new(MAX_COLS, MAX_ROWS).is_ok());
    }

    #[test]
    fn size_rejects_zero_and_oversize() {
        for (c, r) in [(0, 24), (80, 0), (MAX_COLS + 1, 24), (80, MAX_ROWS + 1)] {
            assert!(matches!(
                TerminalSize::new(c, r),
                Err(PtyError::InvalidSize { .. })
            ));
        }
    }
}
