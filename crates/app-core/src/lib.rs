//! Application services shared by every UI: saved sessions, terminals
//! (local PTY and SSH behind one id space), SSH connection setup and
//! host-key prompts. No Tauri or React dependencies.

mod auth_prompts;
mod connect;
mod desktops;
mod error;
mod files;
mod host_keys;
mod keys;
mod logs;
mod mobaxterm;
mod sessions;
mod settings;
mod snippets;
mod ssh_config;
mod terminals;
mod transfers;
mod tunnels;

pub use auth_prompts::{AuthPrompt, AuthPromptBroker, AuthPromptField};
pub use connect::{ConnectRequest, Connected, Connector, PasswordInput};
pub use desktops::{DesktopHub, DesktopInfo, DesktopLaunch, DesktopState};
pub use error::AppError;
pub use files::{
    DroppedFile, DroppedFiles, MAX_EDIT_BYTES, decode_text, safe_file_name, unique_destination,
};
pub use host_keys::{HostKeyPrompt, PromptBroker};
pub use keys::{KeyInstall, install_public_key};
pub use logs::{AnsiStripper, LogInfo, LogTap, TerminalLogs, tapped};
pub use mobaxterm::{import_mobaxterm, is_mobaxterm, parse_mobaxterm};
pub use sessions::{
    AuthMethod, SessionId, SessionInput, SessionProtocol, SessionRecord, SessionStore,
};
pub use settings::{CursorStyle, Settings, Theme, export_sessions, import_sessions};
pub use snippets::{MAX_SNIPPET_LEN, Snippet, SnippetInput};
pub use ssh_config::{
    IMPORT_FOLDER, ImportCandidate, ImportReport, import_candidates, parse_ssh_config,
};
pub use terminals::{FLOW_HIGH_WATERMARK, TerminalHub};
pub use transfers::{TransferEvent, Transfers, remote_join};
pub use tunnels::{TunnelInfo, TunnelKind, TunnelSpec, Tunnels};
