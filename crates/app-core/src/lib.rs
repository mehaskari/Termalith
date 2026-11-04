//! Application services shared by every UI: saved sessions, terminals
//! (local PTY and SSH behind one id space), SSH connection setup and
//! host-key prompts. No Tauri or React dependencies.

mod auth_prompts;
mod connect;
mod crash;
mod desktops;
mod error;
mod files;
mod folders;
mod history;
mod host_keys;
mod keys;
mod logs;
mod mobaxterm;
mod monitor;
mod rdp_certs;
mod remote_files;
mod sessions;
mod settings;
mod snippets;
mod ssh_config;
mod terminals;
mod transfers;
mod tunnels;
mod updates;

pub use auth_prompts::{AuthPrompt, AuthPromptBroker, AuthPromptField};
pub use connect::{ConnectRequest, Connected, Connector, PasswordInput};
pub use crash::{CrashKind, CrashReport, CrashReports, MAX_REPORTS, panic_report};
pub use desktops::{DesktopHub, DesktopInfo, DesktopLaunch, DesktopState};
pub use error::AppError;
pub use files::{
    DroppedFile, DroppedFiles, LocalTree, LocalTreeFile, MAX_EDIT_BYTES, MAX_FOLDER_ENTRIES,
    decode_text, safe_file_name, unique_destination, walk_local_folder,
};
pub use folders::{MAX_FOLDER_DEPTH, MAX_FOLDER_LEN, normalize_folder};
pub use history::{
    MAX_HISTORY, TransferDirection, TransferMeta, TransferOutcome, TransferRecord, history_recorder,
};
pub use host_keys::{HostKeyPrompt, PromptBroker};
pub use keys::{KeyInstall, install_public_key};
pub use logs::{AnsiStripper, LogInfo, LogTap, TerminalLogs, tapped};
pub use mobaxterm::{import_mobaxterm, is_mobaxterm, parse_mobaxterm};
pub use monitor::{ServerMonitor, ServerStats};
pub use rdp_certs::{CertificateCheck, FTPS_CERT_ALGORITHM, RDP_CERT_ALGORITHM};
pub use remote_files::RemoteFiles;
pub use sessions::{
    AuthMethod, SessionId, SessionInput, SessionProtocol, SessionRecord, SessionStore,
};
pub use settings::{
    CursorStyle, ProxySetting, ProxyType, Settings, Theme, export_sessions, import_sessions,
};
pub use snippets::{MAX_SNIPPET_LEN, Snippet, SnippetInput};
pub use ssh_config::{
    IMPORT_FOLDER, ImportCandidate, ImportReport, import_candidates, parse_ssh_config,
};
pub use terminals::{FLOW_HIGH_WATERMARK, TerminalHub};
pub use transfers::{Download, TransferEvent, TransferRecorder, Transfers, remote_join};
pub use tunnels::{TunnelInfo, TunnelKind, TunnelSpec, Tunnels};
pub use updates::{UpdateInfo, check_for_updates, newest_release};
