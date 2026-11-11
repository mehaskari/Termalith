//! Built-in file servers that share one local folder for a while: TFTP
//! (firmware and configs for network devices) and HTTP (a file a server
//! can `curl`). See ADR-036.
//!
//! Every server is confined to its folder: request paths are checked
//! component by component, and the resolved file must still be inside the
//! folder after symbolic links are followed. Uploads are off unless the
//! caller turns them on, and an upload never replaces an existing file.

use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub mod http;
pub mod tftp;

/// Which protocol a server speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerKind {
    Tftp,
    Http,
}

impl ServerKind {
    /// Unprivileged default port (69 and 80 need admin rights on Linux and
    /// macOS).
    pub fn default_port(self) -> u16 {
        match self {
            ServerKind::Tftp => 6969,
            ServerKind::Http => 8080,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// The shared folder.
    pub root: PathBuf,
    /// Loopback unless the user allows other computers.
    pub bind: IpAddr,
    /// 0 picks a free port.
    pub port: u16,
    pub allow_uploads: bool,
}

/// What a client did, for the activity list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub peer: String,
    pub action: Action,
    /// Path inside the shared folder, with `/` separators.
    pub path: String,
    pub bytes: u64,
    /// Why it failed; `None` when it succeeded.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Download,
    Upload,
    List,
}

pub type ActivitySink = Arc<dyn Fn(Activity) + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("the shared folder is not usable: {0}")]
    Folder(String),
    #[error("cannot listen on {addr}: {source}")]
    Listen {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },
}

/// A started server; dropping it does not stop it, [`Running::stop`] does.
pub struct Running {
    pub kind: ServerKind,
    pub local_addr: SocketAddr,
    stop: CancellationToken,
    task: JoinHandle<()>,
}

impl Running {
    /// Stops accepting and ends transfers in progress.
    pub async fn stop(self) {
        self.stop.cancel();
        let _ = self.task.await;
    }

    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }
}

/// Starts a server of `kind` for `config`.
pub async fn start(
    kind: ServerKind,
    config: ServerConfig,
    activity: ActivitySink,
) -> Result<Running, ServerError> {
    let root = std::fs::canonicalize(&config.root)
        .map_err(|e| ServerError::Folder(format!("{}: {e}", config.root.display())))?;
    if !root.is_dir() {
        return Err(ServerError::Folder(format!(
            "{} is not a folder",
            root.display()
        )));
    }
    let shared = Arc::new(Shared {
        root,
        allow_uploads: config.allow_uploads,
        activity,
    });
    let addr = SocketAddr::new(config.bind, config.port);
    let stop = CancellationToken::new();
    let (local_addr, task) = match kind {
        ServerKind::Tftp => tftp::serve(addr, shared, stop.clone()).await?,
        ServerKind::Http => http::serve(addr, shared, stop.clone()).await?,
    };
    Ok(Running {
        kind,
        local_addr,
        stop,
        task,
    })
}

/// What every connection of one server shares.
pub(crate) struct Shared {
    /// Canonical path of the shared folder.
    pub root: PathBuf,
    pub allow_uploads: bool,
    pub activity: ActivitySink,
}

impl Shared {
    pub fn report(
        &self,
        peer: SocketAddr,
        action: Action,
        path: &str,
        bytes: u64,
        error: Option<String>,
    ) {
        (self.activity)(Activity {
            peer: peer.ip().to_string(),
            action,
            path: path.to_owned(),
            bytes,
            error,
        });
    }
}

/// Why a request path was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathError {
    /// `..`, a drive letter, a NUL or similar: never inside the folder.
    Outside,
    NotFound,
}

/// Splits a client path (`/` or `\` separated) into checked names.
pub(crate) fn components(request: &str) -> Result<Vec<String>, PathError> {
    let mut out = Vec::new();
    for part in request.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => return Err(PathError::Outside),
            p if p.contains(['\0', ':']) => return Err(PathError::Outside),
            p => {
                // A single normal component, whatever the OS thinks of it.
                let mut c = Path::new(p).components();
                match (c.next(), c.next()) {
                    (Some(Component::Normal(_)), None) => out.push(p.to_owned()),
                    _ => return Err(PathError::Outside),
                }
            }
        }
    }
    Ok(out)
}

/// An existing file or folder for `request`, inside `root` after symbolic
/// links are followed.
pub(crate) fn resolve_existing(root: &Path, request: &str) -> Result<PathBuf, PathError> {
    let parts = components(request)?;
    let joined = parts.iter().fold(root.to_path_buf(), |p, c| p.join(c));
    let real = std::fs::canonicalize(&joined).map_err(|_| PathError::NotFound)?;
    if real.starts_with(root) {
        Ok(real)
    } else {
        Err(PathError::Outside)
    }
}

/// Where a new upload for `request` goes: its folder exists inside `root`
/// and the file itself does not exist yet.
pub(crate) fn resolve_new(root: &Path, request: &str) -> Result<PathBuf, PathError> {
    let mut parts = components(request)?;
    let name = parts.pop().ok_or(PathError::Outside)?;
    let dir = parts.iter().fold(root.to_path_buf(), |p, c| p.join(c));
    let real_dir = std::fs::canonicalize(&dir).map_err(|_| PathError::NotFound)?;
    if !real_dir.starts_with(root) || !real_dir.is_dir() {
        return Err(PathError::Outside);
    }
    Ok(real_dir.join(name))
}

/// The path shown in the activity list.
pub(crate) fn display_path(request: &str) -> String {
    match components(request) {
        Ok(parts) => format!("/{}", parts.join("/")),
        Err(_) => request
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_paths_stay_inside_the_folder() {
        assert_eq!(components("/a/./b//c").unwrap(), ["a", "b", "c"]);
        assert_eq!(components("a\\b").unwrap(), ["a", "b"]);
        for bad in ["../x", "a/../../x", "C:\\Windows", "a\0b", "/a/..", "c:x"] {
            assert_eq!(components(bad), Err(PathError::Outside), "{bad:?}");
        }
    }

    #[test]
    fn symbolic_links_out_of_the_folder_are_refused() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"x").unwrap();
        let shared = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(shared.path()).unwrap();
        std::fs::write(root.join("ok.txt"), b"y").unwrap();
        assert_eq!(
            resolve_existing(&root, "/ok.txt").unwrap(),
            root.join("ok.txt")
        );
        assert_eq!(resolve_existing(&root, "/nope"), Err(PathError::NotFound));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path().join("secret"), root.join("link")).unwrap();
            assert_eq!(resolve_existing(&root, "/link"), Err(PathError::Outside));
            std::os::unix::fs::symlink(outside.path(), root.join("dir")).unwrap();
            assert_eq!(resolve_new(&root, "/dir/new"), Err(PathError::Outside));
        }
        assert_eq!(
            resolve_new(&root, "/new.bin").unwrap(),
            root.join("new.bin")
        );
        assert_eq!(resolve_new(&root, "/"), Err(PathError::Outside));
    }
}
