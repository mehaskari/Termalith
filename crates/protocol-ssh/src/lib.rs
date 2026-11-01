//! SSH and SFTP client adapter built on russh (ADR-005).
//!
//! Independent of Tauri and React. Host-key trust decisions are made by the
//! caller through [`HostKeyDecider`]; nothing is accepted silently.

mod algorithms;
mod error;
mod forward;
mod host_key;
mod known_hosts;
mod session;
mod sftp;

pub use error::SshError;
pub use forward::{ForwardHandle, ForwardKind, MAX_FORWARD_CONNECTIONS, start_forward};
pub use host_key::{HostKeyDecider, HostKeyDecision, HostKeyInfo, HostKeyStatus, RejectUnknown};
pub use known_hosts::{FileKnownHosts, KnownHostsStore, RemovedHostKeys};
pub use russh::keys::PublicKey;
pub use session::{
    Auth, ConnectOptions, ExecOutput, ShellChannel, ShellEvent, ShellReader, ShellWriter,
    SshSession, Target,
};
pub use sftp::{RemoteEntry, RemoteKind, RemoteStat, SftpClient, TransferOptions};
pub use tokio_util::sync::CancellationToken;
