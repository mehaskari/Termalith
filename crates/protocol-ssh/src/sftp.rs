//! SFTP transfers with cancellation and no partial files at the destination.
//!
//! Data is streamed through one fixed-size buffer into a temporary file next
//! to the destination, which is renamed into place only after the whole file
//! was written. On cancellation or error the temporary file is removed.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::SshError;

#[derive(Debug, Clone, Copy)]
pub struct TransferOptions {
    /// Replace an existing destination. Off by default.
    pub overwrite: bool,
    pub chunk_size: usize,
}

impl Default for TransferOptions {
    fn default() -> Self {
        Self {
            overwrite: false,
            chunk_size: 64 * 1024,
        }
    }
}

pub struct SftpClient {
    inner: SftpSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteKind {
    Dir,
    File,
    Symlink,
    Other,
}

/// One directory entry. Names come from the server and are untrusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    pub name: String,
    pub kind: RemoteKind,
    pub size: Option<u64>,
    /// Seconds since the Unix epoch.
    pub modified: Option<u32>,
    /// Unix mode bits, if reported.
    pub permissions: Option<u32>,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!(
        "termalith-part-{}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed),
        nanos
    )
}

impl SftpClient {
    pub(crate) fn new(inner: SftpSession) -> Self {
        Self { inner }
    }

    pub async fn exists(&self, remote: &str) -> Result<bool, SshError> {
        Ok(self.inner.try_exists(remote).await?)
    }

    pub async fn remove_file(&self, remote: &str) -> Result<(), SshError> {
        Ok(self.inner.remove_file(remote).await?)
    }

    /// Lists file names in a remote directory (spike helper).
    pub async fn list_names(&self, remote_dir: &str) -> Result<Vec<String>, SshError> {
        Ok(self
            .inner
            .read_dir(remote_dir)
            .await?
            .map(|e| e.file_name())
            .collect())
    }

    /// Lists a directory: directories first, then by name; `.`/`..` omitted.
    pub async fn list(&self, remote_dir: &str) -> Result<Vec<RemoteEntry>, SshError> {
        use russh_sftp::protocol::FileType;
        let mut out: Vec<RemoteEntry> = self
            .inner
            .read_dir(remote_dir)
            .await?
            .filter(|e| e.file_name() != "." && e.file_name() != "..")
            .map(|e| {
                let m = e.metadata();
                RemoteEntry {
                    name: e.file_name(),
                    kind: match e.file_type() {
                        FileType::Dir => RemoteKind::Dir,
                        FileType::File => RemoteKind::File,
                        FileType::Symlink => RemoteKind::Symlink,
                        FileType::Other => RemoteKind::Other,
                    },
                    size: m.size,
                    modified: m.mtime,
                    permissions: m.permissions,
                }
            })
            .collect();
        out.sort_by(|a, b| {
            (a.kind != RemoteKind::Dir)
                .cmp(&(b.kind != RemoteKind::Dir))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(out)
    }

    /// Resolves a path on the server (e.g. `"."` to the home directory).
    pub async fn canonicalize(&self, path: &str) -> Result<String, SshError> {
        Ok(self.inner.canonicalize(path).await?)
    }

    /// Uploads `local` to `remote`. Returns bytes transferred.
    pub async fn upload(
        &self,
        local: &Path,
        remote: &str,
        opts: TransferOptions,
        cancel: &CancellationToken,
        mut progress: impl FnMut(u64) + Send,
    ) -> Result<u64, SshError> {
        if !opts.overwrite && self.inner.try_exists(remote).await? {
            return Err(SshError::DestinationExists(remote.to_owned()));
        }
        let mut src = tokio::fs::File::open(local).await?;
        let tmp = format!("{remote}.{}", temp_suffix());
        let mut dst = self
            .inner
            .open_with_flags(
                &tmp,
                OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE,
            )
            .await?;

        let copied = copy(&mut src, &mut dst, opts.chunk_size, cancel, &mut progress).await;
        let finished = match copied {
            Ok(n) => dst.shutdown().await.map(|()| n).map_err(SshError::from),
            Err(e) => Err(e),
        };
        drop(dst);
        let n = match finished {
            Ok(n) => n,
            Err(e) => {
                let _ = self.inner.remove_file(&tmp).await;
                return Err(e);
            }
        };
        // SFTP v3 rename fails if the target exists. Replacing is
        // remove-then-rename, so not atomic (posix-rename@openssh.com is a
        // possible improvement).
        if opts.overwrite && self.inner.try_exists(remote).await? {
            self.inner.remove_file(remote).await?;
        }
        if let Err(e) = self.inner.rename(&tmp, remote).await {
            let _ = self.inner.remove_file(&tmp).await;
            return Err(e.into());
        }
        Ok(n)
    }

    /// Downloads `remote` to `local`. Returns bytes transferred.
    pub async fn download(
        &self,
        remote: &str,
        local: &Path,
        opts: TransferOptions,
        cancel: &CancellationToken,
        mut progress: impl FnMut(u64) + Send,
    ) -> Result<u64, SshError> {
        if !opts.overwrite && tokio::fs::try_exists(local).await? {
            return Err(SshError::DestinationExists(local.display().to_string()));
        }
        let tmp = local_temp_path(local)?;
        let mut src = self.inner.open(remote).await?;
        let mut dst = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .await?;

        let copied = copy(&mut src, &mut dst, opts.chunk_size, cancel, &mut progress).await;
        let finished = match copied {
            Ok(n) => dst.sync_all().await.map(|()| n).map_err(SshError::from),
            Err(e) => Err(e),
        };
        drop(dst);
        match finished {
            Ok(n) => {
                if let Err(e) = tokio::fs::rename(&tmp, local).await {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return Err(e.into());
                }
                Ok(n)
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                Err(e)
            }
        }
    }

    pub async fn close(self) -> Result<(), SshError> {
        Ok(self.inner.close().await?)
    }
}

fn local_temp_path(local: &Path) -> Result<PathBuf, SshError> {
    let name = local
        .file_name()
        .ok_or_else(|| SshError::Io(std::io::Error::other("destination has no file name")))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(".");
    tmp_name.push(temp_suffix());
    Ok(local.with_file_name(tmp_name))
}

async fn copy<R, W>(
    src: &mut R,
    dst: &mut W,
    chunk: usize,
    cancel: &CancellationToken,
    progress: &mut (impl FnMut(u64) + Send),
) -> Result<u64, SshError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; chunk.max(1)];
    let mut total = 0u64;
    loop {
        let n = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(SshError::Cancelled),
            r = src.read(&mut buf) => r?,
        };
        if n == 0 {
            return Ok(total);
        }
        tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(SshError::Cancelled),
            r = dst.write_all(&buf[..n]) => r?,
        }
        total += n as u64;
        progress(total);
    }
}
