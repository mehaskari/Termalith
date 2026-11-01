//! Local terminal sessions backed by a native pseudo-terminal.
//!
//! This crate is independent of Tauri and React. The UI layer supplies a
//! [`TerminalEventSink`] and forwards acknowledgements; everything else
//! (process lifecycle, bounded buffering, flow control) lives here.
//!
//! Platform-specific behaviour is isolated behind [`PtyBackend`] /
//! [`PtyControl`] and the `shell` module's per-OS resolvers.

mod backend;
mod error;
mod flow;
mod manager;
mod shell;
mod types;

pub use backend::{NativePtyBackend, PtyBackend, PtyControl, SpawnedPty};
pub use error::PtyError;
pub use flow::FlowConfig;
pub use manager::{TerminalEventSink, TerminalManager};
pub use shell::{ShellProfile, ShellSpec, available_shells, default_shell, shell_by_id};
pub use types::{ExitInfo, MAX_INPUT_CHUNK, TerminalEvent, TerminalId, TerminalSize};
