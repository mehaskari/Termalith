//! Crash reports: a panic hook for the backend, and IPC to list, dismiss and
//! record UI crashes. Reports stay on disk until the user dismisses them.

use std::sync::Arc;

use app_core::{CrashKind, CrashReport, CrashReports};
use tauri::State;

pub struct CrashState(pub Arc<CrashReports>);

/// Records every later panic, then runs the previous hook (which prints it).
pub fn install_hook(reports: Arc<CrashReports>) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let (message, details) = app_core::panic_report(info);
        let _ = reports.write(CrashKind::Backend, &message, &details);
        previous(info);
    }));
}

/// Reports from earlier runs that the user has not dismissed.
#[tauri::command]
pub fn crash_reports(state: State<'_, CrashState>) -> Vec<CrashReport> {
    state.0.pending()
}

#[tauri::command]
pub fn crash_dismiss(state: State<'_, CrashState>, ids: Vec<String>) {
    state.0.dismiss(&ids);
}

/// Saves a UI crash caught by the error boundary.
#[tauri::command]
pub fn frontend_error(state: State<'_, CrashState>, message: String, details: String) {
    let _ = state.0.write(CrashKind::Ui, &message, &details);
}
