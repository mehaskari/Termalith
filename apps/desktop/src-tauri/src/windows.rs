//! Terminal tabs moved into their own windows (P8-10, ADR-042). The terminal
//! keeps running in the backend; only where its output goes changes.

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{
    AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, Window, WindowEvent,
};

use crate::error::IpcError;
use crate::state::AppState;
use crate::terminal::{ExitPayload, channel_sink};

/// Window labels of moved terminals: `terminal-<id>`.
const LABEL_PREFIX: &str = "terminal-";
const MAX_TITLE_CHARS: usize = 120;

pub fn window_label(id: u64) -> String {
    format!("{LABEL_PREFIX}{id}")
}

/// The terminal id a moved terminal's window belongs to.
pub fn terminal_of(label: &str) -> Option<u64> {
    label.strip_prefix(LABEL_PREFIX)?.parse().ok()
}

/// A window title from a tab title: no control characters, not too long.
pub fn window_title(title: &str) -> String {
    let clean: String = title
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TITLE_CHARS)
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        "Terminal".to_owned()
    } else {
        format!("{clean} — Termalith")
    }
}

/// Opens terminal `id` in a new window with `history` (the text the tab
/// showed). The tab must stop showing it once this succeeds; on failure the
/// terminal stays in the tab.
#[tauri::command]
pub async fn terminal_move_to_window(
    app: AppHandle,
    state: State<'_, AppState>,
    id: u64,
    title: String,
    history: String,
) -> Result<(), IpcError> {
    let route = state.routes.get(id).ok_or_else(|| {
        IpcError::coded("not_found", "This terminal is closed or cannot be moved.")
    })?;
    if state.routes.is_moved(id) {
        return Err(IpcError::coded(
            "invalid",
            "This terminal already has its own window.",
        ));
    }
    state.routes.mark_moved(id, &history);
    // Held back from now on, so nothing between the tab's last picture and
    // the new window is lost.
    route.detach();
    let url = WebviewUrl::App(format!("index.html?terminal={id}").into());
    let built = WebviewWindowBuilder::new(&app, window_label(id), url)
        .title(window_title(&title))
        .inner_size(900.0, 600.0)
        .min_inner_size(360.0, 220.0)
        .build();
    match built {
        Ok(window) => {
            close_with_window(&app, &window, id);
            Ok(())
        }
        Err(e) => {
            route.restore();
            state.routes.unmark_moved(id);
            Err(IpcError::internal(format!(
                "Could not open a new window: {e}"
            )))
        }
    }
}

/// Closing a moved terminal's window closes the terminal.
fn close_with_window(app: &AppHandle, window: &WebviewWindow, id: u64) {
    let app = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event
            && let Some(state) = app.try_state::<AppState>()
        {
            state.tunnels.stop_for_terminal(id);
            state.logs.forget(id);
            state.routes.remove(id);
            let hub = state.hub.clone();
            tauri::async_runtime::spawn(async move {
                let _ = hub.close(id).await;
            });
        }
    });
}

/// Only the window terminal `id` moved to may take it over.
fn own_window(window: &Window, id: u64) -> Result<(), IpcError> {
    if terminal_of(window.label()) == Some(id) {
        Ok(())
    } else {
        Err(IpcError::coded(
            "invalid",
            "This terminal belongs to another window.",
        ))
    }
}

/// The text the terminal showed before it moved (once; empty after a reload).
#[tauri::command]
pub fn terminal_take_history(
    window: Window,
    state: State<'_, AppState>,
    id: u64,
) -> Result<String, IpcError> {
    own_window(&window, id)?;
    state
        .routes
        .take_history(id)
        .ok_or_else(|| IpcError::coded("not_found", "This terminal is closed."))
}

/// Sends terminal `id`'s output to this window from now on, starting with
/// what was held back during the move.
#[tauri::command]
pub fn terminal_attach(
    window: Window,
    state: State<'_, AppState>,
    id: u64,
    on_output: Channel<InvokeResponseBody>,
    on_exit: Channel<ExitPayload>,
) -> Result<u64, IpcError> {
    own_window(&window, id)?;
    let route = state
        .routes
        .get(id)
        .filter(|_| state.routes.is_moved(id))
        .ok_or_else(|| IpcError::coded("not_found", "This terminal is closed."))?;
    // The old window never acknowledges what it last received.
    state.hub.reset_flow(id)?;
    route.attach(channel_sink(on_output, on_exit));
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_titles() {
        assert_eq!(window_label(42), "terminal-42");
        assert_eq!(terminal_of("terminal-42"), Some(42));
        assert_eq!(terminal_of("main"), None);
        assert_eq!(terminal_of("terminal-x"), None);
        assert_eq!(window_title("web-01"), "web-01 — Termalith");
        assert_eq!(window_title(" \u{7}\n "), "Terminal");
        assert_eq!(
            window_title(&"a".repeat(500)).chars().count(),
            MAX_TITLE_CHARS + " — Termalith".chars().count()
        );
    }
}
