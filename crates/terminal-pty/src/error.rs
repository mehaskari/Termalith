use crate::TerminalId;

/// Errors produced by the terminal subsystem.
///
/// Messages never include terminal input or output, since either may contain
/// secrets typed by the user.
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    #[error("terminal size {cols}x{rows} is out of range")]
    InvalidSize { cols: u16, rows: u16 },

    #[error("input chunk of {len} bytes exceeds the {max}-byte limit")]
    InputTooLarge { len: usize, max: usize },

    #[error("no usable shell was found")]
    NoShell,

    #[error("terminal {0} does not exist")]
    UnknownTerminal(TerminalId),

    #[error("failed to open pseudo-terminal: {0}")]
    Open(String),

    #[error("failed to spawn shell: {0}")]
    Spawn(String),

    #[error("terminal I/O failed: {0}")]
    Io(#[from] std::io::Error),
}
