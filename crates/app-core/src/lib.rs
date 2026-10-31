//! Application services shared by every UI: saved sessions, terminals
//! (local PTY and SSH behind one id space), SSH connection setup and
//! host-key prompts. No Tauri or React dependencies.

mod connect;
mod error;
mod files;
mod host_keys;
mod sessions;
mod ssh_config;
mod terminals;
mod transfers;
mod tunnels;

pub use connect::{ConnectRequest, Connected, Connector, PasswordInput};
pub use error::AppError;
pub use files::{DroppedFile, DroppedFiles, safe_file_name, unique_destination};
pub use host_keys::{HostKeyPrompt, PromptBroker};
pub use sessions::{AuthMethod, SessionId, SessionInput, SessionRecord, SessionStore};
pub use ssh_config::{
    IMPORT_FOLDER, ImportCandidate, ImportReport, import_candidates, parse_ssh_config,
};
pub use terminals::{FLOW_HIGH_WATERMARK, TerminalHub};
pub use transfers::{TransferEvent, Transfers, remote_join};
pub use tunnels::{TunnelInfo, TunnelKind, TunnelSpec, Tunnels};
