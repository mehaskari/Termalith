//! Port-forwarding tunnels attached to an SSH terminal's connection.
//!
//! Policy: listeners bind to loopback unless the caller explicitly allows
//! other machines to connect (SECURITY.md: "bind to loopback by default").

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use protocol_ssh::{ForwardHandle, ForwardKind, start_forward};
use serde::{Deserialize, Serialize};

use crate::{AppError, TerminalHub};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TunnelKind {
    /// `ssh -L`: local port -> `remote_host:remote_port` as seen from the server.
    #[serde(rename_all = "camelCase")]
    Local {
        remote_host: String,
        remote_port: u16,
    },
    /// `ssh -D`: SOCKS5 proxy.
    Socks,
    /// `ssh -R`: the server listens on `local_port` (its side) and forwards
    /// to `target_host:target_port` on this machine.
    #[serde(rename_all = "camelCase")]
    Remote {
        target_host: String,
        target_port: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelSpec {
    #[serde(flatten)]
    pub kind: TunnelKind,
    /// 0 picks a free port.
    pub local_port: u16,
    /// Listen on all interfaces instead of loopback only.
    #[serde(default)]
    pub allow_remote_clients: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelInfo {
    pub id: u64,
    pub terminal_id: u64,
    #[serde(flatten)]
    pub kind: TunnelKind,
    /// Where connections are accepted: a local address, or `server:<addr>`
    /// for remote forwards.
    pub local_address: String,
    /// True when other machines can reach the listener.
    pub exposed: bool,
    pub active_connections: usize,
    pub total_connections: u64,
}

enum Handle {
    Local(ForwardHandle),
    Remote {
        session: std::sync::Arc<protocol_ssh::SshSession>,
        bind: String,
        port: u16,
    },
}

impl Drop for Handle {
    fn drop(&mut self) {
        if let Handle::Remote {
            session,
            bind,
            port,
        } = self
        {
            let (session, bind, port) = (session.clone(), bind.clone(), *port);
            tokio::spawn(async move {
                let _ = session.cancel_remote_forward(&bind, port).await;
            });
        }
    }
}

struct Running {
    terminal_id: u64,
    spec: TunnelSpec,
    handle: Handle,
}

#[derive(Default)]
pub struct Tunnels {
    next: AtomicU64,
    running: Mutex<HashMap<u64, Running>>,
}

impl TunnelSpec {
    pub fn validate(&self) -> Result<(), AppError> {
        if let TunnelKind::Remote {
            target_host,
            target_port,
        } = &self.kind
        {
            let h = target_host.trim();
            if h.is_empty()
                || h.len() > 253
                || h.chars().any(|c| c.is_whitespace() || c.is_control())
            {
                return Err(AppError::invalid(
                    "local target",
                    "not a valid host name or address",
                ));
            }
            if *target_port == 0 {
                return Err(AppError::invalid("local target port", "must be 1-65535"));
            }
        }
        if let TunnelKind::Local {
            remote_host,
            remote_port,
        } = &self.kind
        {
            let h = remote_host.trim();
            if h.is_empty()
                || h.len() > 253
                || h.chars().any(|c| c.is_whitespace() || c.is_control())
            {
                return Err(AppError::invalid(
                    "remote host",
                    "not a valid host name or address",
                ));
            }
            if *remote_port == 0 {
                return Err(AppError::invalid("remote port", "must be 1-65535"));
            }
        }
        Ok(())
    }

    fn bind_addr(&self) -> SocketAddr {
        let ip = if self.allow_remote_clients {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        };
        SocketAddr::new(ip, self.local_port)
    }
}

impl Tunnels {
    pub async fn start(
        &self,
        hub: &TerminalHub,
        terminal_id: u64,
        spec: TunnelSpec,
    ) -> Result<TunnelInfo, AppError> {
        spec.validate()?;
        let session = hub.ssh_session(terminal_id)?;
        if let TunnelKind::Remote {
            target_host,
            target_port,
        } = &spec.kind
        {
            // Loopback on the server unless explicitly exposed (the server's
            // GatewayPorts setting may still restrict this).
            let bind = if spec.allow_remote_clients {
                "0.0.0.0"
            } else {
                "127.0.0.1"
            };
            let port = session
                .forward_remote(bind, spec.local_port, target_host.trim(), *target_port)
                .await?;
            let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
            let handle = Handle::Remote {
                session,
                bind: bind.to_owned(),
                port,
            };
            let info = info(id, terminal_id, &spec, &handle);
            self.lock().insert(
                id,
                Running {
                    terminal_id,
                    spec,
                    handle,
                },
            );
            return Ok(info);
        }
        let kind = match &spec.kind {
            TunnelKind::Local {
                remote_host,
                remote_port,
            } => ForwardKind::Local {
                host: remote_host.trim().to_owned(),
                port: *remote_port,
            },
            TunnelKind::Socks => ForwardKind::Socks,
            TunnelKind::Remote { .. } => unreachable!("handled above"),
        };
        let handle = start_forward(session, spec.bind_addr(), kind)
            .await
            .map_err(|e| match e {
                protocol_ssh::SshError::Io(io) if io.kind() == std::io::ErrorKind::AddrInUse => {
                    AppError::invalid(
                        "local port",
                        format!("{} is already in use", spec.local_port),
                    )
                }
                other => other.into(),
            })?;
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let handle = Handle::Local(handle);
        let info = info(id, terminal_id, &spec, &handle);
        self.lock().insert(
            id,
            Running {
                terminal_id,
                spec,
                handle,
            },
        );
        Ok(info)
    }

    pub fn list(&self, terminal_id: Option<u64>) -> Vec<TunnelInfo> {
        let mut v: Vec<_> = self
            .lock()
            .iter()
            .filter(|(_, r)| terminal_id.is_none_or(|t| t == r.terminal_id))
            .map(|(id, r)| info(*id, r.terminal_id, &r.spec, &r.handle))
            .collect();
        v.sort_by_key(|t| t.id);
        v
    }

    pub fn stop(&self, id: u64) -> bool {
        self.lock().remove(&id).is_some() // dropping the handle stops it
    }

    /// Stops every tunnel on a terminal (called when the terminal closes).
    pub fn stop_for_terminal(&self, terminal_id: u64) -> usize {
        let mut map = self.lock();
        let before = map.len();
        map.retain(|_, r| r.terminal_id != terminal_id);
        before - map.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Running>> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn info(id: u64, terminal_id: u64, spec: &TunnelSpec, h: &Handle) -> TunnelInfo {
    match h {
        Handle::Local(f) => TunnelInfo {
            id,
            terminal_id,
            kind: spec.kind.clone(),
            local_address: f.local_addr().to_string(),
            exposed: !f.local_addr().ip().is_loopback(),
            active_connections: f.active_connections(),
            total_connections: f.total_connections(),
        },
        Handle::Remote { bind, port, .. } => TunnelInfo {
            id,
            terminal_id,
            kind: spec.kind.clone(),
            local_address: format!("server:{bind}:{port}"),
            exposed: bind != "127.0.0.1",
            active_connections: 0,
            total_connections: 0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_unless_explicitly_exposed() {
        let mut s = TunnelSpec {
            kind: TunnelKind::Socks,
            local_port: 1080,
            allow_remote_clients: false,
        };
        assert!(s.bind_addr().ip().is_loopback());
        s.allow_remote_clients = true;
        assert!(s.bind_addr().ip().is_unspecified());
    }

    #[test]
    fn validates_local_target() {
        let spec = |h: &str, p: u16| TunnelSpec {
            kind: TunnelKind::Local {
                remote_host: h.into(),
                remote_port: p,
            },
            local_port: 0,
            allow_remote_clients: false,
        };
        assert!(spec("db.internal", 5432).validate().is_ok());
        assert!(spec("", 5432).validate().is_err());
        assert!(spec("a b", 5432).validate().is_err());
        assert!(spec("db", 0).validate().is_err());
    }

    #[test]
    fn spec_json_shape() {
        let s: TunnelSpec = serde_json::from_value(serde_json::json!({
            "kind": "local", "remoteHost": "db", "remotePort": 5432, "localPort": 15432
        }))
        .unwrap();
        assert_eq!(
            s.kind,
            TunnelKind::Local {
                remote_host: "db".into(),
                remote_port: 5432
            }
        );
        assert!(!s.allow_remote_clients, "exposure must default to off");
    }
}
