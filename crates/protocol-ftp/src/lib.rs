//! FTP client for the file browser (ADR-031). One control connection serves
//! listings and small operations; each transfer gets its own connection, as
//! FTP runs one operation at a time per connection. Uploads and downloads go
//! to a temporary name and are renamed into place, so a cancelled or failed
//! transfer leaves no partial file.
//!
//! Paths come from the UI and names from the server: anything with a line
//! break is refused, as it could inject commands on the control connection.

use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use suppaftp::list::{File, ListParser};
use suppaftp::tokio::{AsyncRustlsConnector, AsyncRustlsFtpStream};
use suppaftp::tokio_rustls::TlsConnector;
use suppaftp::tokio_rustls::rustls::client::danger::{
    HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier,
};
use suppaftp::tokio_rustls::rustls::crypto::{
    CryptoProvider, verify_tls12_signature, verify_tls13_signature,
};
use suppaftp::tokio_rustls::rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use suppaftp::tokio_rustls::rustls::{self, ClientConfig, DigitallySignedStruct, SignatureScheme};
use suppaftp::types::FileType;
use suppaftp::{FtpError as RawError, Mode, Status};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{Mutex, MutexGuard};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

type Stream = AsyncRustlsFtpStream;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest wait for one answer or one chunk of data.
const IO_TIMEOUT: Duration = Duration::from_secs(60);
const CHUNK: usize = 64 * 1024;
/// Used when a session has no user name.
const ANONYMOUS: (&str, &str) = ("anonymous", "anonymous@");

#[derive(Debug, thiserror::Error)]
pub enum FtpError {
    #[error("could not connect: {0}")]
    Connect(String),
    #[error("authentication failed")]
    AuthFailed,
    #[error("the server did not answer in time")]
    Timeout,
    #[error("transfer cancelled")]
    Cancelled,
    #[error("{0} already exists")]
    DestinationExists(String),
    #[error("file is larger than {limit} bytes")]
    TooLarge { limit: u64 },
    #[error("{0} was changed by someone else")]
    Modified(String),
    #[error("invalid path")]
    InvalidPath,
    #[error("secure connection failed: {0}")]
    Tls(String),
    #[error("the server refused: {0}")]
    Refused(String),
    #[error("connection problem: {0}")]
    Protocol(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<RawError> for FtpError {
    fn from(e: RawError) -> Self {
        match e {
            RawError::ConnectionError(e) => Self::Io(e),
            RawError::UnexpectedResponse(r) if r.status == Status::NotLoggedIn => Self::AuthFailed,
            RawError::UnexpectedResponse(r) => {
                Self::Refused(String::from_utf8_lossy(&r.body).trim().to_owned())
            }
            RawError::SecureError(m) => Self::Tls(m),
            other => Self::Protocol(other.to_string()),
        }
    }
}

pub struct FtpConfig {
    pub host: String,
    pub port: u16,
    /// Empty for anonymous login.
    pub username: String,
    pub password: Zeroizing<String>,
    /// FTPS (explicit TLS) accepting only the certificate with this SHA-256;
    /// `None` is plain FTP. There is no fallback from one to the other.
    pub tls_pin: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Dir,
    File,
    Symlink,
}

/// One directory entry. Names come from the server and are untrusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub modified: Option<u32>,
}

/// What a save needs to detect that someone else changed a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub size: Option<u64>,
    pub modified: Option<u32>,
}

pub struct FtpClient {
    config: FtpConfig,
    mlsd: bool,
    control: Mutex<Stream>,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_name(path: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{path}.termalith-part-{}-{n}-{nanos}", std::process::id())
}

fn check_path(path: &str) -> Result<(), FtpError> {
    if path.is_empty() || path.contains(['\r', '\n', '\0']) {
        return Err(FtpError::InvalidPath);
    }
    Ok(())
}

async fn timed<T>(what: impl Future<Output = Result<T, RawError>>) -> Result<T, FtpError> {
    match tokio::time::timeout(IO_TIMEOUT, what).await {
        Ok(r) => Ok(r?),
        Err(_) => Err(FtpError::Timeout),
    }
}

/// Parent folder and name of an absolute path.
fn split(path: &str) -> (&str, &str) {
    let path = path.trim_end_matches('/');
    match path.rsplit_once('/') {
        Some(("", name)) => ("/", name),
        Some(x) => x,
        None => (".", path),
    }
}

fn entry_of(file: &File) -> Option<Entry> {
    let name = file.name();
    if name == "." || name == ".." || name.is_empty() || name.contains(['/', '\r', '\n', '\0']) {
        return None;
    }
    let kind = if file.is_directory() {
        EntryKind::Dir
    } else if file.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::File
    };
    Some(Entry {
        name: name.to_owned(),
        kind,
        size: file.size() as u64,
        modified: file
            .modified()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| u32::try_from(d.as_secs()).ok()),
    })
}

/// Entries of a `LIST` answer; lines in other formats ("total 8") are skipped.
fn parse_list(lines: &[String]) -> Vec<Entry> {
    lines
        .iter()
        .filter_map(|l| {
            ListParser::parse_posix(l)
                .or_else(|_| ListParser::parse_dos(l))
                .ok()
        })
        .filter_map(|f| entry_of(&f))
        .collect()
}

fn parse_mlsd(lines: &[String]) -> Vec<Entry> {
    lines
        .iter()
        .filter_map(|l| ListParser::parse_mlsd(l).ok())
        .filter_map(|f| entry_of(&f))
        .collect()
}

/// Accepts one exact certificate, or none while reading it. Handshake
/// signatures are still checked, so the server must hold the private key.
/// Applies to the data connections too, which use the same configuration.
#[derive(Debug)]
struct PinnedCertificate {
    provider: Arc<CryptoProvider>,
    pin: Option<[u8; 32]>,
    seen: StdMutex<Option<[u8; 32]>>,
}

impl ServerCertVerifier for PinnedCertificate {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let hash: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(hash);
        if self.pin == Some(hash) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "certificate is not the trusted one".into(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn tls_connector(verifier: Arc<PinnedCertificate>) -> Result<AsyncRustlsConnector, FtpError> {
    let config = ClientConfig::builder_with_provider(Arc::clone(&verifier.provider))
        .with_safe_default_protocol_versions()
        .map_err(|e| FtpError::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)).into())
}

fn verifier(pin: Option<[u8; 32]>) -> Arc<PinnedCertificate> {
    Arc::new(PinnedCertificate {
        provider: Arc::new(rustls::crypto::aws_lc_rs::default_provider()),
        pin,
        seen: StdMutex::new(None),
    })
}

/// SHA-256 of the certificate an FTPS server presents, without logging in.
/// Fails on a server that does not offer TLS.
pub async fn read_certificate(host: &str, port: u16) -> Result<[u8; 32], FtpError> {
    let reader = verifier(None);
    let ftp = plain_connection(host, port).await?;
    // Rejected by the verifier on purpose: the certificate is all we want.
    let result = ftp
        .into_secure(tls_connector(Arc::clone(&reader))?, host)
        .await;
    let seen = *reader.seen.lock().unwrap_or_else(|e| e.into_inner());
    match (seen, result) {
        (Some(hash), _) => Ok(hash),
        (None, Err(e)) => Err(e.into()),
        (None, Ok(_)) => Err(FtpError::Tls("no certificate".into())),
    }
}

async fn plain_connection(host: &str, port: u16) -> Result<Stream, FtpError> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| FtpError::Connect(e.to_string()))?
        .next()
        .ok_or_else(|| FtpError::Connect("no address".into()))?;
    let mut ftp = Stream::connect_timeout(addr, CONNECT_TIMEOUT)
        .await
        .map_err(|e| FtpError::Connect(e.to_string()))?;
    // Data connections go to the host we chose, never to an address the
    // server names (FTP bounce, private addresses behind NAT).
    ftp.set_passive_nat_workaround(true);
    ftp.set_mode(Mode::Passive);
    Ok(ftp)
}

async fn login(config: &FtpConfig) -> Result<Stream, FtpError> {
    let mut ftp = plain_connection(&config.host, config.port).await?;
    if let Some(pin) = config.tls_pin {
        ftp = timed(ftp.into_secure(tls_connector(verifier(Some(pin)))?, &config.host)).await?;
    }
    let result = if config.username.is_empty() {
        timed(ftp.login(ANONYMOUS.0, ANONYMOUS.1)).await
    } else {
        timed(ftp.login(config.username.as_str(), config.password.as_str())).await
    };
    result?;
    timed(ftp.transfer_type(FileType::Binary)).await?;
    Ok(ftp)
}

async fn copy<R, W>(
    src: &mut R,
    dst: &mut W,
    cancel: &CancellationToken,
    progress: &mut (impl FnMut(u64) + Send),
) -> Result<u64, FtpError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; CHUNK];
    let mut total = 0;
    loop {
        let n = tokio::select! {
            () = cancel.cancelled() => return Err(FtpError::Cancelled),
            r = tokio::time::timeout(IO_TIMEOUT, src.read(&mut buffer)) => {
                r.map_err(|_| FtpError::Timeout)??
            }
        };
        if n == 0 {
            dst.flush().await?;
            return Ok(total);
        }
        tokio::select! {
            () = cancel.cancelled() => return Err(FtpError::Cancelled),
            r = tokio::time::timeout(IO_TIMEOUT, dst.write_all(&buffer[..n])) => {
                r.map_err(|_| FtpError::Timeout)??
            }
        }
        total += n as u64;
        progress(total);
    }
}

impl FtpClient {
    pub async fn connect(config: FtpConfig) -> Result<Self, FtpError> {
        let mut ftp = login(&config).await?;
        let mlsd = timed(ftp.feat())
            .await
            .is_ok_and(|f| f.contains_key("MLSD") || f.contains_key("MLST"));
        Ok(Self {
            config,
            mlsd,
            control: Mutex::new(ftp),
        })
    }

    /// The control connection; servers drop idle ones, so it is checked
    /// and replaced if needed.
    async fn control(&self) -> Result<MutexGuard<'_, Stream>, FtpError> {
        let mut guard = self.control.lock().await;
        if timed(guard.noop()).await.is_err() {
            *guard = login(&self.config).await?;
        }
        Ok(guard)
    }

    async fn list_on(&self, ftp: &mut Stream, dir: &str) -> Result<Vec<Entry>, FtpError> {
        if self.mlsd {
            return Ok(parse_mlsd(&timed(ftp.mlsd(Some(dir))).await?));
        }
        // A name starting with "-" would be read as LIST options.
        timed(ftp.cwd(dir)).await?;
        Ok(parse_list(&timed(ftp.list(None)).await?))
    }

    async fn entry_on(&self, ftp: &mut Stream, path: &str) -> Result<Option<Entry>, FtpError> {
        let (dir, name) = split(path);
        Ok(self
            .list_on(ftp, dir)
            .await?
            .into_iter()
            .find(|e| e.name == name))
    }

    async fn stat_on(&self, ftp: &mut Stream, path: &str) -> Result<Stat, FtpError> {
        let size = timed(ftp.size(path)).await?;
        let modified = timed(ftp.mdtm(path)).await.ok();
        Ok(Stat {
            size: Some(size as u64),
            modified: modified.and_then(|t| u32::try_from(t.and_utc().timestamp()).ok()),
        })
    }

    /// Best effort: a connection of its own, the transfer's may be mid-reply.
    async fn remove_quietly(&self, path: &str) {
        if let Ok(mut ftp) = login(&self.config).await {
            let _ = timed(ftp.rm(path)).await;
            let _ = ftp.quit().await;
        }
    }

    pub async fn home(&self) -> Result<String, FtpError> {
        timed(self.control().await?.pwd()).await
    }

    pub async fn list(&self, dir: &str) -> Result<Vec<Entry>, FtpError> {
        check_path(dir)?;
        let mut ftp = self.control().await?;
        self.list_on(&mut ftp, dir).await
    }

    pub async fn stat(&self, path: &str) -> Result<Stat, FtpError> {
        check_path(path)?;
        let mut ftp = self.control().await?;
        self.stat_on(&mut ftp, path).await
    }

    pub async fn create_dir(&self, path: &str) -> Result<(), FtpError> {
        check_path(path)?;
        let mut ftp = self.control().await?;
        match timed(ftp.mkdir(path)).await {
            Ok(()) => Ok(()),
            Err(e) => match self.entry_on(&mut ftp, path).await {
                Ok(Some(_)) => Err(FtpError::DestinationExists(path.to_owned())),
                _ => Err(e),
            },
        }
    }

    /// Never replaces an existing entry.
    pub async fn rename(&self, from: &str, to: &str) -> Result<(), FtpError> {
        check_path(from)?;
        check_path(to)?;
        let mut ftp = self.control().await?;
        if self.entry_on(&mut ftp, to).await?.is_some() {
            return Err(FtpError::DestinationExists(to.to_owned()));
        }
        timed(ftp.rename(from, to)).await
    }

    /// Deletes a file or an empty folder.
    pub async fn delete(&self, path: &str) -> Result<(), FtpError> {
        check_path(path)?;
        let mut ftp = self.control().await?;
        let dir = matches!(
            self.entry_on(&mut ftp, path).await?,
            Some(Entry {
                kind: EntryKind::Dir,
                ..
            })
        );
        if dir {
            timed(ftp.rmdir(path)).await
        } else {
            timed(ftp.rm(path)).await
        }
    }

    pub async fn upload(
        &self,
        local: &Path,
        remote: &str,
        overwrite: bool,
        cancel: &CancellationToken,
        mut progress: impl FnMut(u64) + Send,
    ) -> Result<u64, FtpError> {
        check_path(remote)?;
        let mut ftp = login(&self.config).await?;
        let existing = self.entry_on(&mut ftp, remote).await?;
        if !overwrite && existing.is_some() {
            return Err(FtpError::DestinationExists(remote.to_owned()));
        }
        let mut src = tokio::fs::File::open(local).await?;
        let tmp = temp_name(remote);
        let mut dst = timed(ftp.put_with_stream(&tmp)).await?;
        let copied = copy(&mut src, &mut dst, cancel, &mut progress).await;
        let finished = match copied {
            Ok(n) => timed(dst.finish()).await.map(|()| n),
            Err(e) => {
                drop(dst);
                Err(e)
            }
        };
        let n = match finished {
            Ok(n) => n,
            Err(e) => {
                self.remove_quietly(&tmp).await;
                return Err(e);
            }
        };
        // FTP rename may fail on an existing target, so replacing is
        // remove-then-rename, not atomic (as for SFTP v3).
        if existing.is_some() {
            timed(ftp.rm(remote)).await?;
        }
        if let Err(e) = timed(ftp.rename(tmp.as_str(), remote)).await {
            self.remove_quietly(&tmp).await;
            return Err(e);
        }
        let _ = ftp.quit().await;
        Ok(n)
    }

    pub async fn download(
        &self,
        remote: &str,
        local: &Path,
        overwrite: bool,
        cancel: &CancellationToken,
        mut progress: impl FnMut(u64) + Send,
    ) -> Result<u64, FtpError> {
        check_path(remote)?;
        if !overwrite && tokio::fs::try_exists(local).await? {
            return Err(FtpError::DestinationExists(local.display().to_string()));
        }
        let tmp = PathBuf::from(temp_name(&local.to_string_lossy()));
        let mut ftp = login(&self.config).await?;
        let mut src = timed(ftp.retr_as_stream(remote)).await?;
        let mut dst = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .await?;
        let copied = copy(&mut src, &mut dst, cancel, &mut progress).await;
        let finished = match copied {
            Ok(n) => match timed(src.finish()).await {
                Ok(()) => dst.sync_all().await.map(|()| n).map_err(FtpError::from),
                Err(e) => Err(e),
            },
            Err(e) => {
                drop(src);
                Err(e)
            }
        };
        drop(dst);
        let result = match finished {
            Ok(n) => tokio::fs::rename(&tmp, local)
                .await
                .map(|()| n)
                .map_err(FtpError::from),
            Err(e) => Err(e),
        };
        if result.is_err() {
            let _ = tokio::fs::remove_file(&tmp).await;
        }
        result
    }

    /// Downloads `remote` to `local` through `partial`, continuing after
    /// the bytes `partial` already holds (REST). The partial file is kept
    /// when the download stops, so a later call continues; it becomes
    /// `local` when complete. A server that refuses to continue gets the
    /// whole file again. Returns the file's full length.
    pub async fn download_resume(
        &self,
        remote: &str,
        local: &Path,
        partial: &Path,
        cancel: &CancellationToken,
        mut progress: impl FnMut(u64) + Send,
    ) -> Result<u64, FtpError> {
        check_path(remote)?;
        let mut offset = match tokio::fs::metadata(partial).await {
            Ok(m) => m.len(),
            Err(_) => 0,
        };
        let mut ftp = login(&self.config).await?;
        let started = if offset > 0 {
            match timed(ftp.resume_transfer(usize::try_from(offset).unwrap_or(usize::MAX))).await {
                Ok(()) => timed(ftp.retr_as_stream(remote)).await,
                Err(e) => Err(e),
            }
        } else {
            timed(ftp.retr_as_stream(remote)).await
        };
        let mut src = match started {
            Ok(src) => src,
            // Refused to continue: start over on a fresh connection, so a
            // pending reply cannot confuse the next command.
            Err(_) if offset > 0 => {
                drop(ftp);
                tokio::fs::remove_file(partial).await?;
                offset = 0;
                ftp = login(&self.config).await?;
                timed(ftp.retr_as_stream(remote)).await?
            }
            Err(e) => return Err(e),
        };
        let mut dst = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(partial)
            .await?;
        let mut report = |n: u64| progress(offset + n);
        let copied = copy(&mut src, &mut dst, cancel, &mut report).await;
        let n = match copied {
            Ok(n) => {
                timed(src.finish()).await?;
                n
            }
            Err(e) => {
                drop(src);
                return Err(e);
            }
        };
        dst.sync_all().await?;
        drop(dst);
        tokio::fs::rename(partial, local).await?;
        Ok(offset + n)
    }

    /// Reads a whole file of at most `limit` bytes.
    pub async fn read_file(&self, path: &str, limit: u64) -> Result<(Vec<u8>, Stat), FtpError> {
        check_path(path)?;
        let mut ftp = login(&self.config).await?;
        let stat = self.stat_on(&mut ftp, path).await?;
        if stat.size.is_some_and(|s| s > limit) {
            return Err(FtpError::TooLarge { limit });
        }
        let mut src = timed(ftp.retr_as_stream(path)).await?;
        let mut data = Vec::new();
        // The size may be stale: never read more than limit + 1.
        (&mut src).take(limit + 1).read_to_end(&mut data).await?;
        if data.len() as u64 > limit {
            return Err(FtpError::TooLarge { limit });
        }
        timed(src.finish()).await?;
        let _ = ftp.quit().await;
        Ok((data, stat))
    }

    /// Replaces a file with `data`, refusing with [`FtpError::Modified`] if
    /// it no longer matches `expected`. The original is moved aside and only
    /// deleted once the new file is in place, and restored if that fails.
    pub async fn replace_file(
        &self,
        path: &str,
        data: &[u8],
        expected: &Stat,
    ) -> Result<Stat, FtpError> {
        check_path(path)?;
        let mut ftp = login(&self.config).await?;
        if self.stat_on(&mut ftp, path).await? != *expected {
            return Err(FtpError::Modified(path.to_owned()));
        }
        let tmp = temp_name(path);
        let mut dst = timed(ftp.put_with_stream(&tmp)).await?;
        let written = dst.write_all(data).await;
        if let Err(e) = written {
            drop(dst);
            self.remove_quietly(&tmp).await;
            return Err(e.into());
        }
        if let Err(e) = timed(dst.finish()).await {
            self.remove_quietly(&tmp).await;
            return Err(e);
        }
        let aside = temp_name(path);
        timed(ftp.rename(path, aside.as_str())).await?;
        if let Err(e) = timed(ftp.rename(tmp.as_str(), path)).await {
            let _ = timed(ftp.rename(aside.as_str(), path)).await;
            self.remove_quietly(&tmp).await;
            return Err(e);
        }
        timed(ftp.rm(&aside)).await?;
        let stat = self.stat_on(&mut ftp, path).await?;
        let _ = ftp.quit().await;
        Ok(stat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_breaks_in_paths_are_refused() {
        assert!(check_path("/a b/c").is_ok());
        for bad in ["", "/a\r\nDELE x", "/a\n", "/a\0"] {
            assert!(
                matches!(check_path(bad), Err(FtpError::InvalidPath)),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn splits_paths() {
        assert_eq!(split("/a/b"), ("/a", "b"));
        assert_eq!(split("/a"), ("/", "a"));
        assert_eq!(split("/a/b/"), ("/a", "b"));
        assert_eq!(split("a"), (".", "a"));
    }

    #[test]
    fn parses_unix_and_windows_listings() {
        let lines = [
            "total 8",
            "drwxr-xr-x 2 ftp ftp 4096 Jan 02 2024 logs",
            "-rw-r--r-- 1 ftp ftp  123 Jan 02 2024 app log.txt",
            "lrwxrwxrwx 1 ftp ftp    4 Jan 02 2024 link -> logs",
            "drwxr-xr-x 2 ftp ftp 4096 Jan 02 2024 ..",
            "01-02-24  10:15AM       <DIR>          win",
            "01-02-24  10:15AM                  10 win.txt",
        ]
        .map(String::from);
        let e = parse_list(&lines);
        let names: Vec<_> = e.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["logs", "app log.txt", "link", "win", "win.txt"]);
        assert_eq!(e[0].kind, EntryKind::Dir);
        assert_eq!((e[1].kind, e[1].size), (EntryKind::File, 123));
        assert_eq!(e[2].kind, EntryKind::Symlink);
        assert_eq!(e[3].kind, EntryKind::Dir);
    }

    #[test]
    fn parses_mlsd_and_drops_unsafe_names() {
        let lines = [
            "type=cdir;modify=20240102101500; .",
            "type=dir;modify=20240102101500; logs",
            "type=file;size=7;modify=20240102101500; a.txt",
            "type=file;size=7;modify=20240102101500; ../evil",
        ]
        .map(String::from);
        let e = parse_mlsd(&lines);
        assert_eq!(e.len(), 2, "{e:?}");
        assert_eq!((e[1].size, e[1].kind), (7, EntryKind::File));
        assert!(e[1].modified.is_some());
    }

    #[test]
    fn refusals_are_told_apart() {
        use suppaftp::types::Response;
        let bad = |s| RawError::UnexpectedResponse(Response::new(s, b"550 no".to_vec()));
        assert!(matches!(
            FtpError::from(bad(Status::NotLoggedIn)),
            FtpError::AuthFailed
        ));
        assert!(matches!(
            FtpError::from(bad(Status::FileUnavailable)),
            FtpError::Refused(_)
        ));
    }
}
