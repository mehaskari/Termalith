//! Tauri boundary. Translates IPC to calls on platform-neutral crates; holds
//! no domain logic of its own.

mod desktop;
mod error;
mod files;
mod nettools;
mod sessions;
mod ssh;
mod state;
mod terminal;

use tauri::webview::PageLoadEvent;
use tauri::{DragDropEvent, Emitter, Manager, RunEvent, WindowEvent};

use crate::state::AppState;

pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let state = AppState::init(app.handle())?;
            let dropped = state.dropped.clone();
            app.manage(state);
            // Dropped paths stay in the backend; the UI only gets tokens,
            // names and sizes, so it can never name a local file itself.
            if let Some(window) = app.get_webview_window("main") {
                let handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event {
                        let files = dropped.register(paths);
                        if !files.is_empty() {
                            let _ = handle.emit("files-dropped", files);
                        }
                    }
                });
            }
            Ok(())
        })
        .on_page_load(|webview, payload| {
            // A reload orphans every channel the old page held; nothing would
            // ever acknowledge their output, so close those sessions now.
            if payload.event() == PageLoadEvent::Started
                && let Some(state) = webview.try_state::<AppState>()
            {
                let hub = state.hub.clone();
                tauri::async_runtime::spawn_blocking(move || hub.close_all());
            }
        })
        .invoke_handler(tauri::generate_handler![
            terminal::terminal_open,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_ack,
            terminal::terminal_close,
            terminal::terminal_split,
            terminal::tunnel_start,
            terminal::tunnel_list,
            terminal::tunnel_stop,
            terminal::serial_ports,
            terminal::serial_open,
            nettools::net_tcp_check,
            nettools::net_dns_lookup,
            nettools::net_probe_start,
            nettools::net_probe_cancel,
            ssh::ssh_open,
            desktop::desktop_clients,
            desktop::desktop_open,
            desktop::desktop_list,
            desktop::desktop_close,
            ssh::host_key_respond,
            sessions::app_info,
            sessions::sessions_list,
            sessions::session_create,
            sessions::session_update,
            sessions::session_delete,
            sessions::session_forget_secret,
            sessions::ssh_config_preview,
            sessions::ssh_config_import,
            sessions::settings_get,
            sessions::settings_save,
            sessions::session_set_favorite,
            sessions::sessions_export,
            sessions::sessions_import,
            sessions::snippets_list,
            sessions::snippet_create,
            sessions::snippet_update,
            sessions::snippet_delete,
            files::sftp_home,
            files::sftp_list,
            files::sftp_download,
            files::sftp_upload,
            files::transfer_cancel,
            ssh::known_host_forget,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Tauri application");

    app.run(|handle, event| {
        if let RunEvent::Exit = event
            && let Some(state) = handle.try_state::<AppState>()
        {
            state.hub.close_all();
            state.desktops.close_all();
        }
    });
}
