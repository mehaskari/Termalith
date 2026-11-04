//! The file operations the Files panel, the editor and transfers need,
//! independent of the protocol behind them (ADR-031).

use std::path::Path;
use std::sync::Arc;

use protocol_ssh::{
    CancellationToken, RemoteEntry, RemoteStat, SftpClient, SshError, TransferOptions,
};

/// A connection's file access. Cheap to clone.
#[derive(Clone)]
pub enum RemoteFiles {
    Sftp(Arc<SftpClient>),
}

impl From<Arc<SftpClient>> for RemoteFiles {
    fn from(client: Arc<SftpClient>) -> Self {
        Self::Sftp(client)
    }
}

impl RemoteFiles {
    /// The folder a session starts in.
    pub async fn home(&self) -> Result<String, SshError> {
        match self {
            Self::Sftp(c) => c.canonicalize(".").await,
        }
    }

    pub async fn list(&self, dir: &str) -> Result<Vec<RemoteEntry>, SshError> {
        match self {
            Self::Sftp(c) => c.list(dir).await,
        }
    }

    pub async fn upload(
        &self,
        local: &Path,
        remote: &str,
        opts: TransferOptions,
        cancel: &CancellationToken,
        progress: impl FnMut(u64) + Send,
    ) -> Result<u64, SshError> {
        match self {
            Self::Sftp(c) => c.upload(local, remote, opts, cancel, progress).await,
        }
    }

    pub async fn download(
        &self,
        remote: &str,
        local: &Path,
        opts: TransferOptions,
        cancel: &CancellationToken,
        progress: impl FnMut(u64) + Send,
    ) -> Result<u64, SshError> {
        match self {
            Self::Sftp(c) => c.download(remote, local, opts, cancel, progress).await,
        }
    }

    pub async fn read_file(
        &self,
        remote: &str,
        limit: u64,
    ) -> Result<(Vec<u8>, RemoteStat), SshError> {
        match self {
            Self::Sftp(c) => c.read_file(remote, limit).await,
        }
    }

    pub async fn replace_file(
        &self,
        remote: &str,
        data: &[u8],
        expected: &RemoteStat,
    ) -> Result<RemoteStat, SshError> {
        match self {
            Self::Sftp(c) => c.replace_file(remote, data, expected).await,
        }
    }

    pub async fn create_dir(&self, remote: &str) -> Result<(), SshError> {
        match self {
            Self::Sftp(c) => c.create_dir(remote).await,
        }
    }

    pub async fn rename(&self, from: &str, to: &str) -> Result<(), SshError> {
        match self {
            Self::Sftp(c) => c.rename(from, to).await,
        }
    }

    pub async fn delete(&self, remote: &str) -> Result<(), SshError> {
        match self {
            Self::Sftp(c) => c.delete(remote).await,
        }
    }
}
