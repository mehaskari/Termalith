//! FTP server on libunftp (Apache-2.0) with its file system backend, which
//! uses `cap-std` so paths cannot leave the shared folder. One user with a
//! random password per start. The backend is wrapped so that, as for the
//! other servers, uploads are refused unless allowed, never replace a file,
//! and nothing is ever deleted or renamed.

use std::fmt::Debug;
use std::net::{IpAddr, SocketAddr, TcpListener};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use libunftp::notification::{DataEvent, DataListener, EventMeta};
use libunftp::options::{PassiveHost, Shutdown};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use unftp_core::auth::{AuthenticationError, Authenticator, Credentials, DefaultUser, Principal};
use unftp_core::storage::{Error, ErrorKind, Fileinfo, Result, StorageBackend};
use unftp_sbe_fs::{Filesystem, Meta};

use crate::{Action, ServerError, Shared};

/// The FTP user name; the password is random per start.
pub const FTP_USER: &str = "termalith";

pub(crate) async fn serve(
    addr: SocketAddr,
    shared: Arc<Shared>,
    stop: CancellationToken,
    password: String,
) -> std::result::Result<(SocketAddr, JoinHandle<()>), ServerError> {
    // libunftp binds by itself, so a free port picked for 0 can be taken by
    // another socket before it listens: then another one is tried.
    let attempts = if addr.port() == 0 { 5 } else { 1 };
    let mut last = None;
    for _ in 0..attempts {
        match try_serve(addr, &shared, &stop, &password).await {
            Ok(started) => return Ok(started),
            Err(e) => last = Some(e),
        }
    }
    Err(last.expect("at least one attempt"))
}

async fn try_serve(
    addr: SocketAddr,
    shared: &Arc<Shared>,
    stop: &CancellationToken,
    password: &str,
) -> std::result::Result<(SocketAddr, JoinHandle<()>), ServerError> {
    // Check the address first (and pick a free port for 0), so a busy port
    // is reported to the user at once.
    let local = TcpListener::bind(addr)
        .and_then(|l| l.local_addr())
        .map_err(|source| ServerError::Listen { addr, source })?;
    let root = shared.root.clone();
    let uploads = shared.allow_uploads;
    let storage = move || Guarded {
        inner: Filesystem::new(root.clone()).expect("shared folder checked at start"),
        uploads,
    };
    let auth = Arc::new(OneUser {
        password: password.to_owned(),
        shared: Arc::clone(shared),
    });
    let shutdown = {
        let stop = stop.clone();
        async move {
            stop.cancelled().await;
            Shutdown::new().grace_period(Duration::from_secs(1))
        }
    };
    let mut builder = libunftp::ServerBuilder::with_authenticator(Box::new(storage), auth)
        .greeting("Termalith file server")
        .idle_session_timeout(600)
        .notify_data(Activity(Arc::clone(shared)))
        .shutdown_indicator(shutdown);
    // Passive connections go to the address the client used; on loopback
    // that is 127.0.0.1 either way.
    if let IpAddr::V4(v4) = local.ip()
        && v4.is_loopback()
    {
        builder = builder.passive_host(PassiveHost::Ip(v4));
    }
    let server = builder.build().map_err(|e| ServerError::Listen {
        addr,
        source: std::io::Error::other(e.to_string()),
    })?;
    let (failed_tx, failed_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        if let Err(e) = server.listen(local.to_string()).await {
            let _ = failed_tx.send(e.to_string());
        }
    });
    // A failure to listen shows up right away.
    if let Ok(Ok(why)) = tokio::time::timeout(Duration::from_millis(200), failed_rx).await {
        return Err(ServerError::Listen {
            addr: local,
            source: std::io::Error::other(why),
        });
    }
    Ok((local, task))
}

/// The one user of this server.
struct OneUser {
    password: String,
    shared: Arc<Shared>,
}

impl Debug for OneUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OneUser")
    }
}

#[async_trait]
impl Authenticator for OneUser {
    async fn authenticate(
        &self,
        username: &str,
        creds: &Credentials,
    ) -> std::result::Result<Principal, AuthenticationError> {
        let peer = SocketAddr::new(creds.source_ip, 0);
        let ok = username == FTP_USER
            && creds
                .password
                .as_deref()
                .is_some_and(|p| constant_time_eq(p.as_bytes(), self.password.as_bytes()));
        if ok {
            self.shared.report(peer, Action::Login, "/", 0, None);
            Ok(Principal {
                username: username.to_owned(),
            })
        } else {
            self.shared.report(
                peer,
                Action::Login,
                "/",
                0,
                Some("wrong user name or password".into()),
            );
            Err(AuthenticationError::BadPassword)
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Successful downloads and uploads for the activity list (libunftp
/// reports no client address with them; the login entry has it).
struct Activity(Arc<Shared>);

impl Debug for Activity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Activity")
    }
}

#[async_trait]
impl DataListener for Activity {
    async fn receive_data_event(&self, e: DataEvent, m: EventMeta) {
        let peer = format!("user {}", m.username);
        let (action, path, bytes) = match e {
            DataEvent::Got { path, bytes } => (Action::Download, path, bytes),
            DataEvent::Put { path, bytes } => (Action::Upload, path, bytes),
            DataEvent::MadeDir { path } => (Action::Upload, format!("{path}/"), 0),
            _ => return,
        };
        // Windows paths come with backslashes; the list uses `/`.
        let path = path.replace('\\', "/");
        let path = if path.starts_with('/') {
            path
        } else {
            format!("/{path}")
        };
        (self.0.activity)(crate::Activity {
            peer,
            action,
            path,
            bytes,
            error: None,
        });
    }
}

/// The file system backend with Termalith's rules on top.
#[derive(Debug)]
struct Guarded {
    inner: Filesystem,
    uploads: bool,
}

fn denied() -> Error {
    Error::from(ErrorKind::PermissionDenied)
}

#[async_trait]
impl StorageBackend<DefaultUser> for Guarded {
    type Metadata = Meta;

    fn supported_features(&self) -> u32 {
        // Resumed downloads (REST) are fine; resumed uploads would write
        // into an existing file, which is refused anyway.
        unftp_core::storage::FEATURE_RESTART
    }

    async fn metadata<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> Result<Meta> {
        self.inner.metadata(user, path).await
    }

    async fn list<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> Result<Vec<Fileinfo<std::path::PathBuf, Meta>>>
    where
        <Self as StorageBackend<DefaultUser>>::Metadata: unftp_core::storage::Metadata,
    {
        self.inner.list(user, path).await
    }

    async fn get<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
        start_pos: u64,
    ) -> Result<Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin>> {
        self.inner.get(user, path, start_pos).await
    }

    async fn put<
        P: AsRef<Path> + Send + Debug,
        R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
    >(
        &self,
        user: &DefaultUser,
        input: R,
        path: P,
        start_pos: u64,
    ) -> Result<u64> {
        if !self.uploads || start_pos != 0 {
            return Err(denied());
        }
        // Never replace a file (the backend would truncate it).
        if self.inner.metadata(user, path.as_ref()).await.is_ok() {
            return Err(Error::new(
                ErrorKind::PermissionDenied,
                "file already exists",
            ));
        }
        self.inner.put(user, input, path, 0).await
    }

    async fn del<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &DefaultUser,
        _path: P,
    ) -> Result<()> {
        Err(denied())
    }

    async fn mkd<P: AsRef<Path> + Send + Debug>(&self, user: &DefaultUser, path: P) -> Result<()> {
        if !self.uploads {
            return Err(denied());
        }
        self.inner.mkd(user, path).await
    }

    async fn rename<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &DefaultUser,
        _from: P,
        _to: P,
    ) -> Result<()> {
        Err(denied())
    }

    async fn rmd<P: AsRef<Path> + Send + Debug>(
        &self,
        _user: &DefaultUser,
        _path: P,
    ) -> Result<()> {
        Err(denied())
    }

    async fn cwd<P: AsRef<Path> + Send + Debug>(&self, user: &DefaultUser, path: P) -> Result<()> {
        self.inner.cwd(user, path).await
    }
}

/// A random password for one server start (letters and digits, easy to type).
pub(crate) fn random_password() -> String {
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).expect("system random number generator");
    bytes
        .iter()
        .map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_are_random_and_compared_in_full() {
        let a = random_password();
        assert_eq!(a.len(), 12);
        assert_ne!(a, random_password());
        assert!(constant_time_eq(a.as_bytes(), a.as_bytes()));
        assert!(!constant_time_eq(a.as_bytes(), &a.as_bytes()[..11]));
    }
}
