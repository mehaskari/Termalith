//! Asking before Termalith quits while connections are open: the UI says
//! when to ask (setting on and connected tabs), the backend holds the
//! close or quit and asks the main window, which calls `app_quit` once the
//! user agreed.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, State};

/// Event the main window gets when a close or quit was held.
pub const QUIT_REQUESTED: &str = "quit-requested";

#[derive(Default)]
pub struct QuitGuard {
    ask: AtomicBool,
    confirmed: AtomicBool,
}

impl QuitGuard {
    /// Whether a close or quit should wait for the user's answer.
    pub fn should_ask(&self) -> bool {
        self.ask.load(Ordering::SeqCst) && !self.confirmed.load(Ordering::SeqCst)
    }

    /// Holds the close or quit when asking is on; true when held.
    pub fn hold(&self, app: &AppHandle) -> bool {
        if !self.should_ask() {
            return false;
        }
        let _ = app.emit_to("main", QUIT_REQUESTED, ());
        true
    }
}

/// The UI's current answer to "ask before quitting?".
#[tauri::command]
pub fn app_set_quit_guard(guard: State<'_, QuitGuard>, ask: bool) {
    guard.ask.store(ask, Ordering::SeqCst);
}

/// Quits after the user agreed (open sessions are closed as on any quit).
#[tauri::command]
pub fn app_quit(app: AppHandle, guard: State<'_, QuitGuard>) {
    guard.confirmed.store(true, Ordering::SeqCst);
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_only_when_on_and_not_yet_agreed() {
        let g = QuitGuard::default();
        assert!(!g.should_ask());
        g.ask.store(true, Ordering::SeqCst);
        assert!(g.should_ask());
        g.confirmed.store(true, Ordering::SeqCst);
        assert!(!g.should_ask(), "an agreed quit goes through");
    }
}
