//! SSH and SFTP client adapter built on russh (ADR-005).
//!
//! Independent of Tauri and React. Host-key trust decisions are made by the
//! caller through [`HostKeyDecider`]; nothing is accepted silently.

mod algorithms;
mod error;
mod forward;
mod host_key;
mod kbd;
mod keys;
mod known_hosts;
mod proxy;
mod session;
mod sftp;
mod x11;

pub use error::SshError;
pub use forward::{ForwardHandle, ForwardKind, MAX_FORWARD_CONNECTIONS, start_forward};
pub use host_key::{HostKeyDecider, HostKeyDecision, HostKeyInfo, HostKeyStatus, RejectUnknown};
pub use kbd::{KbdAnswers, KbdInteractive, KbdPrompt, KbdRequest};
pub use keys::{KeyInfo, generate_ed25519, list_keys, read_public_key};
pub use known_hosts::{FileKnownHosts, KnownHostsStore, RemovedHostKeys};
pub use proxy::{Proxy, ProxyKind, connect as connect_via_proxy};
pub use russh::keys::PublicKey;
pub use session::{
    Auth, ConnectOptions, ExecOutput, ShellChannel, ShellEvent, ShellReader, ShellWriter,
    SshSession, Target,
};
pub use sftp::{RemoteEntry, RemoteKind, RemoteStat, SftpClient, TransferOptions};
pub use tokio_util::sync::CancellationToken;
pub use x11::{X11Display, X11Forward, cookie_from_xauth_list, local_cookie, parse_display};
