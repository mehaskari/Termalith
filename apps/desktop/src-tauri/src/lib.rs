//! Tauri boundary. Translates IPC to calls on platform-neutral crates; holds
//! no domain logic of its own.

mod error;
mod terminal;

use std::sync::Arc;

use tauri::webview::PageLoadEvent;
use tauri::{Manager, RunEvent};
use terminal_pty::{FlowConfig, NativePtyBackend, TerminalManager};

pub fn run() {
    let terminals = Arc::new(TerminalManager::new(
        Arc::new(NativePtyBackend),
        FlowConfig::default(),
    ));

    let app = tauri::Builder::default()
        .manage(Arc::clone(&terminals))
        .on_page_load(|webview, payload| {
            // A reload orphans every channel the old page held; nothing would
            // ever acknowledge their output, so close those sessions now.
            if payload.event() == PageLoadEvent::Started {
                let terminals = webview.state::<Arc<TerminalManager>>().inner().clone();
                tauri::async_runtime::spawn_blocking(move || terminals.close_all());
            }
        })
        .invoke_handler(tauri::generate_handler![
            terminal::terminal_open,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_ack,
            terminal::terminal_close,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Tauri application");

    app.run(move |_handle, event| {
        if let RunEvent::Exit = event {
            terminals.close_all();
        }
    });
}
