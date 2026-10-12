//! The `oct` desktop application.
//!
//! A shell, for now: it opens a window and reports what the platform layer resolved to. The
//! library view, the first-party reader and the tray arrive in later phases.
//!
//! `oct-core` and `oct-platform` are wired in from the start on purpose — an application shell that
//! only compiles is worth much less than one that already demonstrates the architecture is
//! reachable from the interface.

// Prevents an extra console window from appearing alongside the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use oct_core::Platform;

/// The platform this build targets, as reported to the frontend.
#[tauri::command]
fn platform_name() -> &'static str {
    Platform::current().id()
}

/// The identifiers of the platform backends this build selected.
///
/// Exposed to the interface so that "which backends am I actually running?" is answerable without
/// reading logs. Once real implementations land (T7, T8, T11, T12) the same call reports their
/// capabilities, and unsupported ones become visible instead of silently doing nothing.
#[tauri::command]
fn backend_ids() -> Vec<String> {
    let backends = oct_platform::backends();
    vec![
        backends.recent_documents.id().to_string(),
        backends.open_documents.id().to_string(),
        backends.launcher.id().to_string(),
        backends.file_identity.id().to_string(),
    ]
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![platform_name, backend_ids])
        .run(tauri::generate_context!())
        .expect("failed to start the oct desktop application");
}
