//! Built-in file servers (ADR-036): start, stop and list them, and keep a
//! short activity list per server for the panel. Loopback unless the user
//! allows other computers; uploads off unless turned on.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use local_servers::{Activity, Running, ServerConfig, ServerKind};
use serde::Serialize;

use crate::AppError;

/// Activity entries kept per server.
const ACTIVITY_KEPT: usize = 200;
/// Servers running at once.
const MAX_SERVERS: usize = 8;

/// What to start.
#[derive(Debug, Clone)]
pub struct ServerSpec {
    pub kind: ServerKind,
    pub root: PathBuf,
    /// Listen on all interfaces instead of loopback.
    pub lan: bool,
    /// `None`: the protocol's unprivileged default.
    pub port: Option<u16>,
    pub allow_uploads: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub id: u64,
    pub kind: ServerKind,
    /// The shared folder, for display.
    pub folder: String,
    pub port: u16,
    pub lan: bool,
    pub allow_uploads: bool,
    /// Addresses to give to clients (`tftp://…`, `http://…`, `ftp://…`).
    pub urls: Vec<String>,
    /// FTP: the user name and this start's random password.
    pub credentials: Option<FtpCredentials>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FtpCredentials {
    pub user: String,
    pub password: String,
}

/// One activity entry, numbered so the panel can show what is new.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub seq: u64,
    /// Seconds since the Unix epoch.
    pub at: u64,
    #[serde(flatten)]
    pub activity: Activity,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerState {
    #[serde(flatten)]
    pub info: ServerInfo,
    pub activity: Vec<ActivityEntry>,
}

struct Entry {
    info: ServerInfo,
    running: Running,
    activity: Arc<Mutex<VecDeque<ActivityEntry>>>,
}

#[derive(Default)]
pub struct LocalServers {
    next: AtomicU64,
    seq: Arc<AtomicU64>,
    servers: Mutex<HashMap<u64, Entry>>,
}

impl LocalServers {
    pub async fn start(&self, spec: ServerSpec) -> Result<ServerInfo, AppError> {
        if self.servers.lock().unwrap_or_else(|e| e.into_inner()).len() >= MAX_SERVERS {
            return Err(AppError::invalid(
                "server",
                format!("at most {MAX_SERVERS} servers run at once"),
            ));
        }
        let bind = if spec.lan {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        };
        let port = spec.port.unwrap_or_else(|| spec.kind.default_port());
        let activity: Arc<Mutex<VecDeque<ActivityEntry>>> = Arc::default();
        let (log, seq) = (Arc::clone(&activity), Arc::clone(&self.seq));
        let running = local_servers::start(
            spec.kind,
            ServerConfig {
                root: spec.root.clone(),
                bind,
                port,
                allow_uploads: spec.allow_uploads,
            },
            Arc::new(move |a: Activity| {
                let entry = ActivityEntry {
                    seq: seq.fetch_add(1, Ordering::Relaxed) + 1,
                    at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs()),
                    activity: a,
                };
                let mut log = log.lock().unwrap_or_else(|e| e.into_inner());
                if log.len() >= ACTIVITY_KEPT {
                    log.pop_front();
                }
                log.push_back(entry);
            }),
        )
        .await
        .map_err(|e| AppError::invalid("server", e.to_string()))?;
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let info = ServerInfo {
            id,
            kind: spec.kind,
            folder: spec.root.display().to_string(),
            port: running.local_addr.port(),
            lan: spec.lan,
            allow_uploads: spec.allow_uploads,
            urls: urls(spec.kind, running.local_addr, spec.lan),
            credentials: running
                .credentials
                .clone()
                .map(|(user, password)| FtpCredentials { user, password }),
        };
        self.servers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                id,
                Entry {
                    info: info.clone(),
                    running,
                    activity,
                },
            );
        Ok(info)
    }

    /// Running servers with their recent activity.
    pub fn list(&self) -> Vec<ServerState> {
        let mut out: Vec<ServerState> = self
            .servers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|e| e.running.is_running())
            .map(|e| ServerState {
                info: e.info.clone(),
                activity: e
                    .activity
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .iter()
                    .cloned()
                    .collect(),
            })
            .collect();
        out.sort_by_key(|s| s.info.id);
        out
    }

    pub async fn stop(&self, id: u64) -> bool {
        let entry = self
            .servers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        match entry {
            Some(e) => {
                e.running.stop().await;
                true
            }
            None => false,
        }
    }

    pub async fn stop_all(&self) {
        let all: Vec<Entry> = self
            .servers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .map(|(_, e)| e)
            .collect();
        for e in all {
            e.running.stop().await;
        }
    }
}

/// Client addresses for a server: loopback, or this computer's address on
/// the network when other computers may connect.
fn urls(kind: ServerKind, addr: SocketAddr, lan: bool) -> Vec<String> {
    let scheme = match kind {
        ServerKind::Tftp => "tftp",
        ServerKind::Http => "http",
        ServerKind::Ftp => "ftp",
    };
    let port = addr.port();
    let mut hosts = vec!["127.0.0.1".to_owned()];
    if lan {
        hosts = primary_addresses()
            .into_iter()
            .map(|ip| match ip {
                IpAddr::V6(v6) => format!("[{v6}]"),
                v4 => v4.to_string(),
            })
            .collect();
        hosts.push("127.0.0.1".to_owned());
    }
    hosts
        .into_iter()
        .map(|h| format!("{scheme}://{h}:{port}/"))
        .collect()
}

/// The address this computer uses towards the network (found by routing a
/// UDP socket; no packet is sent).
fn primary_addresses() -> Vec<IpAddr> {
    let mut out = Vec::new();
    for (bind, probe) in [
        (IpAddr::V4(Ipv4Addr::UNSPECIFIED), "192.0.2.1:9"),
        (IpAddr::V6(Ipv6Addr::UNSPECIFIED), "[2001:db8::1]:9"),
    ] {
        let found = UdpSocket::bind(SocketAddr::new(bind, 0))
            .and_then(|s| s.connect(probe).map(|()| s))
            .and_then(|s| s.local_addr());
        if let Ok(a) = found
            && !a.ip().is_unspecified()
            && !a.ip().is_loopback()
            && a.ip().is_ipv4()
        {
            out.push(a.ip());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn servers_start_list_and_stop() {
        let dir = tempfile::tempdir().unwrap();
        let hub = LocalServers::default();
        let info = hub
            .start(ServerSpec {
                kind: ServerKind::Http,
                root: dir.path().to_path_buf(),
                lan: false,
                port: Some(0),
                allow_uploads: false,
            })
            .await
            .unwrap();
        assert!(!info.lan && info.port > 0);
        assert_eq!(info.urls, [format!("http://127.0.0.1:{}/", info.port)]);
        assert_eq!(hub.list().len(), 1);
        assert!(hub.stop(info.id).await);
        assert!(hub.list().is_empty());
        assert!(!hub.stop(info.id).await);

        let ftp = hub
            .start(ServerSpec {
                kind: ServerKind::Ftp,
                root: dir.path().to_path_buf(),
                lan: false,
                port: Some(0),
                allow_uploads: false,
            })
            .await
            .unwrap();
        let creds = ftp.credentials.clone().expect("FTP credentials");
        assert_eq!(creds.user, "termalith");
        assert_eq!(creds.password.len(), 12);
        assert!(ftp.urls[0].starts_with("ftp://127.0.0.1:"));
        hub.stop_all().await;
        assert!(hub.list().is_empty());

        let missing = hub
            .start(ServerSpec {
                kind: ServerKind::Tftp,
                root: dir.path().join("nope"),
                lan: false,
                port: Some(0),
                allow_uploads: false,
            })
            .await;
        assert!(missing.is_err());
    }
}
