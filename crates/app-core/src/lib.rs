//! Application services shared by every UI: saved sessions, terminals
//! (local PTY and SSH behind one id space), SSH connection setup and
//! host-key prompts. No Tauri or React dependencies.

mod connect;
mod error;
mod host_keys;
mod sessions;
mod terminals;

pub use connect::{ConnectRequest, Connected, Connector, PasswordInput};
pub use error::AppError;
pub use host_keys::{HostKeyPrompt, PromptBroker};
pub use sessions::{AuthMethod, SessionId, SessionInput, SessionRecord, SessionStore};
pub use terminals::{FLOW_HIGH_WATERMARK, TerminalHub};
