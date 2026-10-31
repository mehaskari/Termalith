//! Remote desktop sessions run by external clients (RDP/VNC), optionally
//! through an SSH tunnel. Tracks each client process so the UI can show
//! whether it is still running and close it.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use protocol_ssh::{ForwardHandle, ForwardKind, SshSession, start_forward};
use remote_desktop::{Client, DesktopTarget, LaunchPlan, Os, Protocol};
use serde::Serialize;
use zeroize::Zeroizing;

use crate::{AppError, SessionId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DesktopState {
    /// The client process is running this connection.
    Running,
    /// The connection was handed to an application that manages its own
    /// windows (macOS `open`); Termalith cannot see when it ends.
    HandedOff,
    Exited {
        code: Option<i32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopInfo {
    pub id: u64,
    pub session_id: Option<SessionId>,
    pub label: String,
    pub protocol: Protocol,
    pub client: String,
    /// Where the client connects: the server, or the local tunnel end.
    pub address: String,
    /// Jump session the connection is tunnelled through.
    pub via: Option<String>,
    #[serde(flatten)]
    pub state: DesktopState,
}

/// What to open.
pub struct DesktopLaunch {
    pub label: String,
    pub session_id: Option<SessionId>,
    pub target: DesktopTarget,
    pub password: Option<Zeroizing<String>>,
    /// SSH connection to tunnel through, with its display name.
    pub tunnel: Option<(Arc<SshSession>, String)>,
}

struct Entry {
    info: DesktopInfo,
    child: Option<tokio::process::Child>,
    _forward: Option<ForwardHandle>,
    file: Option<PathBuf>,
}

impl Drop for Entry {
    fn drop(&mut self) {
        if let Some(f) = &self.file {
            let _ = std::fs::remove_file(f);
        }
    }
}

pub struct DesktopHub {
    temp_dir: PathBuf,
    next: AtomicU64,
    entries: Mutex<HashMap<u64, Entry>>,
}

impl DesktopHub {
    /// `temp_dir` holds connection files (no secrets) while a session is open.
    pub fn new(temp_dir: PathBuf) -> Self {
        Self {
            temp_dir,
            next: AtomicU64::new(1),
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn entries(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Installed clients, preferred first.
    pub fn clients(protocol: Protocol) -> Vec<Client> {
        remote_desktop::available_clients(protocol)
    }

    /// The client that [`DesktopHub::open`] will use.
    pub fn preferred_client(protocol: Protocol) -> Result<Client, AppError> {
        Self::clients(protocol)
            .into_iter()
            .next()
            .ok_or(AppError::Desktop(remote_desktop::DesktopError::NoClient(
                match protocol {
                    Protocol::Rdp => "RDP",
                    Protocol::Vnc => "VNC",
                },
            )))
    }

    /// Opens the session in the preferred client. Returns
    /// [`AppError::PasswordRequired`] when that client needs the password up
    /// front and none was given.
    pub async fn open(&self, launch: DesktopLaunch) -> Result<DesktopInfo, AppError> {
        launch.target.validate()?;
        let client = Self::preferred_client(launch.target.protocol)?;
        if client.reads_password() && launch.password.is_none() {
            return Err(AppError::PasswordRequired);
        }
        let mut target = launch.target.clone();
        let (forward, via) = match launch.tunnel {
            Some((session, name)) => {
                let fwd = start_forward(
                    session,
                    SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                    ForwardKind::Local {
                        host: target.host.trim().to_owned(),
                        port: target.port,
                    },
                )
                .await?;
                target.host = Ipv4Addr::LOCALHOST.to_string();
                target.port = fwd.local_addr().port();
                (Some(fwd), Some(name))
            }
            None => (None, None),
        };
        let plan = remote_desktop::plan_for(Os::current(), &client, &target, &self.temp_dir)?;
        let info = DesktopInfo {
            id: 0,
            session_id: launch.session_id,
            label: launch.label,
            protocol: target.protocol,
            client: client.name.to_owned(),
            address: format!("{}:{}", target.host, target.port),
            via,
            state: DesktopState::Running,
        };
        self.start(plan, info, launch.password, forward).await
    }

    async fn start(
        &self,
        plan: LaunchPlan,
        mut info: DesktopInfo,
        password: Option<Zeroizing<String>>,
        forward: Option<ForwardHandle>,
    ) -> Result<DesktopInfo, AppError> {
        let launched = remote_desktop::launch(&plan, password, &info.client).await?;
        info.id = self.next.fetch_add(1, Ordering::Relaxed);
        if !launched.tracks_session {
            info.state = DesktopState::HandedOff;
        }
        let entry = Entry {
            info: info.clone(),
            child: launched.child,
            _forward: forward,
            file: plan.file.map(|(p, _)| p),
        };
        self.entries().insert(info.id, entry);
        Ok(info)
    }

    /// Current sessions. A tracked client that exited releases its tunnel.
    pub fn list(&self) -> Vec<DesktopInfo> {
        let mut entries = self.entries();
        let mut out: Vec<DesktopInfo> = entries
            .values_mut()
            .map(|e| {
                if e.info.state == DesktopState::Running
                    && let Some(child) = e.child.as_mut()
                    && let Ok(Some(status)) = child.try_wait()
                {
                    e.info.state = DesktopState::Exited {
                        code: status.code(),
                    };
                    e._forward = None;
                }
                e.info.clone()
            })
            .collect();
        out.sort_by_key(|i| i.id);
        out
    }

    /// Closes everything (application exit).
    pub fn close_all(&self) {
        let ids: Vec<u64> = self.entries().keys().copied().collect();
        for id in ids {
            let _ = self.close(id);
        }
    }

    /// Stops the client (if Termalith started and still tracks it) and
    /// closes the tunnel.
    pub fn close(&self, id: u64) -> Result<(), AppError> {
        let mut entry = self.entries().remove(&id).ok_or(AppError::NotFound)?;
        if entry.info.state == DesktopState::Running
            && let Some(child) = entry.child.as_mut()
        {
            let _ = child.start_kill();
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn info(label: &str) -> DesktopInfo {
        DesktopInfo {
            id: 0,
            session_id: None,
            label: label.into(),
            protocol: Protocol::Rdp,
            client: "sh".into(),
            address: "127.0.0.1:3389".into(),
            via: None,
            state: DesktopState::Running,
        }
    }

    fn plan(script: &str, tracks: bool, file: Option<PathBuf>) -> LaunchPlan {
        LaunchPlan {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            password_via_stdin: false,
            file: file.map(|f| (f, "full address:s:x:1\r\n".into())),
            tracks_session: tracks,
        }
    }

    #[tokio::test]
    async fn tracks_exit_and_close_removes_connection_file() {
        let dir = tempfile::tempdir().unwrap();
        let hub = DesktopHub::new(dir.path().into());

        let quick = hub
            .start(plan("exit 3", true, None), info("quick"), None, None)
            .await
            .unwrap();
        let mut state = DesktopState::Running;
        for _ in 0..100 {
            state = hub
                .list()
                .into_iter()
                .find(|i| i.id == quick.id)
                .unwrap()
                .state;
            if state != DesktopState::Running {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(state, DesktopState::Exited { code: Some(3) });

        let file = dir.path().join("c.rdp");
        let long = hub
            .start(
                plan("sleep 30", true, Some(file.clone())),
                info("long"),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(file.exists());
        assert_eq!(hub.list().len(), 2);
        hub.close(long.id).unwrap();
        assert!(!file.exists(), "connection file is removed on close");
        assert_eq!(hub.list().len(), 1);
        assert!(matches!(hub.close(long.id), Err(AppError::NotFound)));

        let handed = hub
            .start(plan("exit 0", false, None), info("open"), None, None)
            .await
            .unwrap();
        assert_eq!(handed.state, DesktopState::HandedOff);
    }
}
