//! The `oct` desktop application.
//!
//! This is the first vertical slice: the library listing, a folder scan and the progress-provider
//! registry, reached from the screen over Tauri IPC. The three commands below are deliberately
//! thin — they open the production database and delegate to the plain functions in [`commands`],
//! which take their collaborators as arguments and are tested without a Tauri application.
//!
//! The first-party reader, the tray, filters and the suggestion queue arrive in later tasks.

// Prevents an extra console window from appearing alongside the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::Path;

use commands::{LibraryEntryView, ProviderView, ScanReportView};
use oct_core::OctError;
use oct_store::{database_path, Store};

mod commands;

/// Lists every document in the library with its latest reading position and provenance.
#[tauri::command]
fn list_library() -> Result<Vec<LibraryEntryView>, String> {
    let store = open_store()?;
    commands::library_entries(&store).map_err(error_message)
}

/// Scans `folder` into the library and returns the report, including any per-file failure.
#[tauri::command]
fn scan_folder(folder: String) -> Result<ScanReportView, String> {
    let store = open_store()?;
    let identity = oct_platform::backends().file_identity;
    commands::scan_path(&store, identity.as_ref(), Path::new(&folder)).map_err(error_message)
}

/// Reports every progress provider with its capability and whether this machine can use it.
#[tauri::command]
fn progress_providers() -> Vec<ProviderView> {
    commands::provider_registry()
}

/// Opens the production database at its per-platform user data location.
///
/// Each command opens its own connection; the store is cheap to open and `oct` currently runs one
/// process, so this is simpler and safer than sharing a connection behind a lock.
fn open_store() -> Result<Store, String> {
    let path = database_path().map_err(error_message)?;
    Store::open(path).map_err(error_message)
}

/// Renders an error into the string the frontend shows. `OctError` is not `Serialize`, and the
/// screen only needs to display the message, so the conversion happens at the IPC boundary.
fn error_message(error: OctError) -> String {
    error.to_string()
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_library,
            scan_folder,
            progress_providers
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the oct desktop application");
}
