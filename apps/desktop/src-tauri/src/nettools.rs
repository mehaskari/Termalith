//! Network toolbox IPC. Probes run fixed OS programs with validated
//! arguments (see the network-tools crate); there is no generic command
//! execution.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use network_tools::{Probe, TcpCheck, ToolError};
use tauri::State;
use tauri::ipc::Channel;
use tokio_util::sync::CancellationToken;

use crate::error::IpcError;
use crate::state::AppState;

#[derive(Default)]
pub struct Probes {
    next: AtomicU64,
    running: Mutex<HashMap<u64, CancellationToken>>,
}

fn tool_err(e: ToolError) -> IpcError {
    match e {
        ToolError::Invalid { .. } => IpcError::coded("invalid", e.to_string()),
        ToolError::Failed(_) => IpcError::coded("tool_failed", e.to_string()),
    }
}

#[tauri::command]
pub async fn net_tcp_check(host: String, port: u16) -> Result<TcpCheck, IpcError> {
    network_tools::tcp_check(&host, port, Duration::from_secs(5))
        .await
        .map_err(tool_err)
}

#[tauri::command]
pub async fn net_dns_lookup(name: String) -> Result<Vec<String>, IpcError> {
    network_tools::dns_lookup(&name).await.map_err(tool_err)
}

/// Starts ping or traceroute; lines stream to `on_line`, the exit code
/// (or null if cancelled) to `on_done`. Returns an id for cancelling.
#[tauri::command]
pub fn net_probe_start(
    state: State<'_, AppState>,
    kind: String,
    host: String,
    on_line: Channel<String>,
    on_done: Channel<Option<i32>>,
) -> Result<u64, IpcError> {
    let probe = match kind.as_str() {
        "ping" => Probe::Ping,
        "traceroute" => Probe::Traceroute,
        _ => return Err(IpcError::coded("invalid", "unknown probe")),
    };
    network_tools::validate_host(&host).map_err(tool_err)?;
    let id = state.probes.next.fetch_add(1, Ordering::Relaxed) + 1;
    let cancel = CancellationToken::new();
    state
        .probes
        .running
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, cancel.clone());
    let probes = state.probes.clone();
    tauri::async_runtime::spawn(async move {
        let result = network_tools::run_probe(probe, &host, &cancel, |l| {
            let _ = on_line.send(l);
        })
        .await;
        if let Err(e) = &result {
            let _ = on_line.send(e.to_string());
        }
        let _ = on_done.send(result.ok().flatten());
        probes
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    });
    Ok(id)
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    port: u16,
    open: bool,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScanDone {
    /// Address that was scanned, when the name resolved.
    address: Option<String>,
    error: Option<String>,
    cancelled: bool,
}

/// Starts a TCP connect scan of `ports` ("22,80,8000-8010", at most 1024)
/// on `host`. Results stream to `on_result`; cancel with
/// `net_probe_cancel`.
#[tauri::command]
pub fn net_scan_start(
    state: State<'_, AppState>,
    host: String,
    ports: String,
    on_result: Channel<ScanResult>,
    on_done: Channel<ScanDone>,
) -> Result<u64, IpcError> {
    network_tools::validate_host(&host).map_err(tool_err)?;
    let ports = network_tools::parse_ports(&ports).map_err(tool_err)?;
    let id = state.probes.next.fetch_add(1, Ordering::Relaxed) + 1;
    let cancel = CancellationToken::new();
    state
        .probes
        .running
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, cancel.clone());
    let probes = state.probes.clone();
    tauri::async_runtime::spawn(async move {
        let result = network_tools::scan_ports(
            &host,
            &ports,
            Duration::from_millis(1500),
            &cancel,
            |port, open| {
                let _ = on_result.send(ScanResult { port, open });
            },
        )
        .await;
        let _ = on_done.send(match result {
            Ok(addr) => ScanDone {
                address: Some(addr.to_string()),
                error: None,
                cancelled: cancel.is_cancelled(),
            },
            Err(e) => ScanDone {
                address: None,
                error: Some(e.to_string()),
                cancelled: false,
            },
        });
        probes
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    });
    Ok(id)
}

/// Sends a Wake-on-LAN magic packet.
#[tauri::command]
pub async fn net_wake(mac: String, broadcast: Option<String>) -> Result<(), IpcError> {
    network_tools::wake_on_lan(&mac, broadcast.as_deref())
        .await
        .map_err(tool_err)
}

#[tauri::command]
pub fn net_probe_cancel(state: State<'_, AppState>, id: u64) -> bool {
    match state
        .probes
        .running
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&id)
    {
        Some(c) => {
            c.cancel();
            true
        }
        None => false,
    }
}
