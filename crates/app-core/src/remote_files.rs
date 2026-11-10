//! The file operations the Files panel, the editor and transfers need,
//! independent of the protocol behind them (ADR-031).

use std::path::Path;
use std::sync::Arc;

use protocol_ftp::{EntryKind, FtpClient, FtpError, Stat};
use protocol_ssh::{
    CancellationToken, RemoteEntry, RemoteKind, RemoteStat, SftpClient, SshError, TransferOptions,
};

/// A connection's file access. Cheap to clone.
#[derive(Clone)]
pub enum RemoteFiles {
    Sftp(Arc<SftpClient>),
    Ftp(Arc<FtpClient>),
}

/// FTP errors in the shared error type (ADR-031).
pub(crate) fn ftp_error(e: FtpError) -> SshError {
    match e {
        FtpError::Connect(m) => SshError::Connect(m),
        FtpError::AuthFailed => SshError::AuthFailed,
        FtpError::Cancelled => SshError::Cancelled,
        FtpError::DestinationExists(p) => SshError::DestinationExists(p),
        FtpError::TooLarge { limit } => SshError::TooLarge { limit },
        FtpError::Modified(p) => SshError::Modified(p),
        FtpError::Io(e) => SshError::Io(e),
        // Retried by transfers, like a dropped SSH connection.
        e @ (FtpError::Timeout | FtpError::Protocol(_)) => SshError::Protocol(e.to_string()),
        e @ (FtpError::Refused(_) | FtpError::InvalidPath | FtpError::Tls(_)) => {
            SshError::Remote(e.to_string())
        }
    }
}

fn remote_stat(s: Stat) -> RemoteStat {
    RemoteStat {
        size: s.size,
        modified: s.modified,
        permissions: None,
    }
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
            Self::Ftp(c) => c.home().await.map_err(ftp_error),
        }
    }

    pub async fn list(&self, dir: &str) -> Result<Vec<RemoteEntry>, SshError> {
        match self {
            Self::Sftp(c) => c.list(dir).await,
            Self::Ftp(c) => Ok(c
                .list(dir)
                .await
                .map_err(ftp_error)?
                .into_iter()
                .map(|e| RemoteEntry {
                    name: e.name,
                    kind: match e.kind {
                        EntryKind::Dir => RemoteKind::Dir,
                        EntryKind::File => RemoteKind::File,
                        EntryKind::Symlink => RemoteKind::Symlink,
                    },
                    size: Some(e.size),
                    modified: e.modified,
                    permissions: None,
                })
                .collect()),
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
            Self::Ftp(c) => c
                .upload(local, remote, opts.overwrite, cancel, progress)
                .await
                .map_err(ftp_error),
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
            Self::Ftp(c) => c
                .download(remote, local, opts.overwrite, cancel, progress)
                .await
                .map_err(ftp_error),
        }
    }

    /// Downloads through `partial`, continuing after what it holds; see
    /// `SftpClient::download_resume`. Returns the file's full length.
    pub async fn download_resume(
        &self,
        remote: &str,
        local: &Path,
        partial: &Path,
        cancel: &CancellationToken,
        progress: impl FnMut(u64) + Send,
    ) -> Result<u64, SshError> {
        match self {
            Self::Sftp(c) => {
                c.download_resume(
                    remote,
                    local,
                    partial,
                    TransferOptions::default(),
                    cancel,
                    progress,
                )
                .await
            }
            Self::Ftp(c) => c
                .download_resume(remote, local, partial, cancel, progress)
                .await
                .map_err(ftp_error),
        }
    }

    /// Size and modification time of a remote file.
    pub async fn stat(&self, remote: &str) -> Result<RemoteStat, SshError> {
        match self {
            Self::Sftp(c) => c.stat(remote).await,
            Self::Ftp(c) => c.stat(remote).await.map(remote_stat).map_err(ftp_error),
        }
    }

    pub async fn read_file(
        &self,
        remote: &str,
        limit: u64,
    ) -> Result<(Vec<u8>, RemoteStat), SshError> {
        match self {
            Self::Sftp(c) => c.read_file(remote, limit).await,
            Self::Ftp(c) => {
                let (data, stat) = c.read_file(remote, limit).await.map_err(ftp_error)?;
                Ok((data, remote_stat(stat)))
            }
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
            Self::Ftp(c) => {
                let expected = Stat {
                    size: expected.size,
                    modified: expected.modified,
                };
                c.replace_file(remote, data, &expected)
                    .await
                    .map(remote_stat)
                    .map_err(ftp_error)
            }
        }
    }

    pub async fn create_dir(&self, remote: &str) -> Result<(), SshError> {
        match self {
            Self::Sftp(c) => c.create_dir(remote).await,
            Self::Ftp(c) => c.create_dir(remote).await.map_err(ftp_error),
        }
    }

    pub async fn rename(&self, from: &str, to: &str) -> Result<(), SshError> {
        match self {
            Self::Sftp(c) => c.rename(from, to).await,
            Self::Ftp(c) => c.rename(from, to).await.map_err(ftp_error),
        }
    }

    pub async fn delete(&self, remote: &str) -> Result<(), SshError> {
        match self {
            Self::Sftp(c) => c.delete(remote).await,
            Self::Ftp(c) => c.delete(remote).await.map_err(ftp_error),
        }
    }
}
