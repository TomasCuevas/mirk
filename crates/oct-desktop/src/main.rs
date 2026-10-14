//! The `oct` desktop application.
//!
//! This is the shell a person uses: Inicio, Biblioteca and Configuración, reached from the bottom
//! navbar, inside a deliberately phone-shaped window with a custom top bar and a tray. The commands
//! below are deliberately thin — they open the production database and delegate to the plain
//! functions in [`commands`], or perform a [`shell`] action, which are tested without a Tauri
//! application.
//!
//! # The window shape is a product statement
//!
//! The window is `430x844` logical pixels — a phone's logical viewport, widened from 390 in R5 —
//! undecorated, fixed-size and not fullscreen. This is not a limitation to apologise for: `oct` is
//! meant to read as a phone-shaped companion that lives in the tray, not as a small desktop window
//! that happens to render a mobile layout. The custom top bar exists for the same reason — a native
//! title bar would bring a maximise button the user explicitly refused.
//!
//! The first-party reader, covers and the suggestion queue arrive in later tasks.

// Prevents an extra console window from appearing alongside the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::Path;

use commands::{LibraryEntryView, OpenedFolderView, ProviderView, ScanReportView, WatchedFolderView};
use oct_core::OctError;
use oct_store::{database_path, Store};
use shell::{action_for_menu_id, dispatch, ShellAction, ShellActions};
use tauri::menu::{Menu, MenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};

mod commands;
mod shell;

/// The label of the window declared in `tauri.conf.json`. Referenced rather than repeated so the
/// tray and the commands always act on the same window.
const MAIN_WINDOW: &str = "main";

/// The tooltip of the tray icon. It is the product name rather than a translated word, so it does
/// not follow the language.
const TRAY_TOOLTIP: &str = "oct";

/// The tray icon's id, so the language command can find and re-word the existing tray instead of
/// building a second icon.
const TRAY_ID: &str = "oct-tray";

/// Performs a [`shell`] action on the real window and application.
struct AppShell {
    app: AppHandle,
    window: WebviewWindow,
}

impl AppShell {
    /// The shell for `window`, with the application handle it belongs to.
    fn new(window: WebviewWindow) -> Self {
        let app = window.app_handle().clone();
        Self { app, window }
    }

    /// The shell for the application's main window, if it still exists.
    fn from_app(app: &AppHandle) -> Option<Self> {
        app.get_webview_window(MAIN_WINDOW).map(Self::new)
    }
}

impl ShellActions for AppShell {
    fn minimize(&self) -> Result<(), String> {
        self.window.minimize().map_err(|error| error.to_string())
    }

    fn hide(&self) -> Result<(), String> {
        self.window.hide().map_err(|error| error.to_string())
    }

    fn show(&self) -> Result<(), String> {
        self.window.show().map_err(|error| error.to_string())?;
        self.window.set_focus().map_err(|error| error.to_string())
    }

    /// Quit the application through the event loop, not by abandoning the process.
    ///
    /// `exit` requests a graceful shutdown, which drops the GTK/WebKit window and lets WebKit's
    /// child processes end with it. This is the single quit path: the top bar's close button and
    /// the tray's quit both land here (A4), so neither can leave a process behind that the other
    /// would have cleaned up.
    fn close(&self) -> Result<(), String> {
        self.app.exit(0);
        Ok(())
    }
}

/// Minimises the window, the way the top bar's minimise control asks.
#[tauri::command]
fn minimize_window(window: WebviewWindow) -> Result<(), String> {
    dispatch(ShellAction::Minimize, &AppShell::new(window))
}

/// Hides the window without closing it, so the tray can bring it back.
#[tauri::command]
fn hide_window(window: WebviewWindow) -> Result<(), String> {
    dispatch(ShellAction::Hide, &AppShell::new(window))
}

/// Quits the application. The same [`ShellAction::Close`] the tray's quit performs.
#[tauri::command]
fn close_app(window: WebviewWindow) -> Result<(), String> {
    dispatch(ShellAction::Close, &AppShell::new(window))
}

/// Starts dragging the window, so the custom top bar can move it.
///
/// This is an application command rather than Tauri's `data-tauri-drag-region` attribute on
/// purpose (A2). The attribute would also make a double-click ask the window to toggle maximise,
/// and the request would only be refused because the window happens to be non-resizable. This
/// command gives the bar the move gesture and gives double-click nowhere to go.
#[tauri::command]
fn start_drag(window: WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|error| error.to_string())
}

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

/// Lists the folders the store has recorded as sources of truth.
#[tauri::command]
fn watched_folders() -> Result<Vec<WatchedFolderView>, String> {
    let store = open_store()?;
    commands::watched_folders(&store).map_err(error_message)
}

/// The stored interface language, or `null` when nothing — or nothing this build knows — is stored.
#[tauri::command]
fn get_language() -> Result<Option<String>, String> {
    stored_language()
}

/// Stores the interface language and re-words the tray so it changes without a restart (R1).
#[tauri::command]
fn set_language(locale: String, app: AppHandle) -> Result<(), String> {
    let store = open_store()?;
    commands::set_language(&store, &locale).map_err(error_message)?;
    rebuild_tray(&app, Some(&locale))
}

/// Opens the folder that contains the document at `path`, reporting an honest error when the
/// folder is gone or nothing can open it.
#[tauri::command]
fn open_containing_folder(path: String) -> Result<OpenedFolderView, String> {
    let launcher = oct_platform::backends().launcher;
    commands::open_containing_folder(launcher.as_ref(), Path::new(&path)).map_err(error_message)
}

/// Opens the production database at its per-platform user data location.
///
/// Each command opens its own connection; the store is cheap to open and `oct` currently runs one
/// process, so this is simpler and safer than sharing a connection behind a lock.
fn open_store() -> Result<Store, String> {
    let path = database_path().map_err(error_message)?;
    Store::open(path).map_err(error_message)
}

/// The language stored in the production database, or an error naming the failure.
///
/// The commands open the production database themselves; `setup` reads it through here so the tray is
/// worded at launch exactly as a later change words it.
fn stored_language() -> Result<Option<String>, String> {
    let store = open_store()?;
    commands::get_language(&store).map_err(error_message)
}

/// Renders an error into the string the frontend shows. `OctError` is not `Serialize`, and the
/// screen only needs to display the message, so the conversion happens at the IPC boundary.
fn error_message(error: OctError) -> String {
    error.to_string()
}

/// Builds a tray menu whose words are `labels`'.
fn tray_menu(app: &AppHandle, labels: shell::TrayLabels) -> tauri::Result<Menu<tauri::Wry>> {
    MenuBuilder::new(app)
        .text(shell::MENU_SHOW, labels.show)
        .text(shell::MENU_HIDE, labels.hide)
        .text(shell::MENU_QUIT, labels.quit)
        .build()
}

/// Re-words the tray in `language` without rebuilding the icon or restarting the application.
///
/// [`tauri::tray::TrayIcon::set_menu`] replaces the menu on the retained tray, and the manager keeps
/// the icon under [`TRAY_ID`] so no handle has to be threaded through the setup closure. On Linux the
/// replacement goes through the app-indicator's own `set_menu`, which is why the words can change
/// while the ids stay fixed.
fn rebuild_tray(app: &AppHandle, language: Option<&str>) -> Result<(), String> {
    let menu = tray_menu(app, shell::tray_labels(language)).map_err(|error| error.to_string())?;
    match app.tray_by_id(TRAY_ID) {
        Some(tray) => tray.set_menu(Some(menu)).map_err(|error| error.to_string()),
        None => Err("the tray icon is not present".to_string()),
    }
}

/// Builds the tray in `language`: an icon with Show, Hide and Quit worded in that language.
///
/// The tray is not decoration. The top bar offers "hide", and a hide button with no way back would
/// make the application vanish forever, which is a trap rather than a feature — that is why the
/// tray lands with the window chrome instead of in a later phase (A3, L20). The icon is the
/// application's own committed placeholder; there is no visual identity to invent.
fn setup_tray(app: &AppHandle, language: Option<&str>) -> tauri::Result<()> {
    let menu = tray_menu(app, shell::tray_labels(language))?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip(TRAY_TOOLTIP)
        // On Linux only the menu is reachable (click events are not emitted), so the left click
        // must open the menu rather than doing nothing.
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let Some(action) = action_for_menu_id(event.id().as_ref()) else {
                return;
            };
            let Some(shell) = AppShell::from_app(app) else {
                return;
            };
            // There is no screen to report a failed window operation to, so it is written to
            // stderr rather than swallowed.
            if let Err(error) = dispatch(action, &shell) {
                eprintln!("the tray could not perform {action:?}: {error}");
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

/// Makes the window a fixed size for real.
///
/// `tauri.conf.json` declares `resizable: false`, and the configuration test asserts it, but on this
/// Wayland/GTK host that value does not survive window creation: the live GTK window reports itself
/// resizable even though the configuration says otherwise, which would let the window manager
/// stretch it and would keep the double-click resize inset armed. Re-applying the flag after the
/// window exists is what makes it stick, and it is applied to the window whose size is already
/// pinned by equal minimum and maximum bounds, so the shape holds even if a window manager ignores
/// the flag.
fn enforce_fixed_shape(window: &WebviewWindow) {
    if let Err(error) = window.set_resizable(false) {
        eprintln!("the window could not be made non-resizable: {error}");
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let window = app
                .get_webview_window(MAIN_WINDOW)
                .expect("the main window is declared in tauri.conf.json");
            enforce_fixed_shape(&window);

            // The tray's words come from the stored language, read from the same production database
            // the commands use. A database that cannot be opened must not take the tray down with it
            // — hiding the window would then leave no way back — so the failure is reported and
            // English stands in for this launch.
            let language = match stored_language() {
                Ok(language) => language,
                Err(error) => {
                    eprintln!("the stored language could not be read for the tray: {error}");
                    None
                }
            };

            setup_tray(app.handle(), language.as_deref())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing really closes: a close request from the window manager takes the same
            // graceful-quit path as the top bar's close button and the tray's quit, rather than
            // being intercepted into a hide the user did not ask for (A4).
            if matches!(event, WindowEvent::CloseRequested { .. }) {
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            minimize_window,
            hide_window,
            close_app,
            start_drag,
            list_library,
            scan_folder,
            progress_providers,
            watched_folders,
            get_language,
            set_language,
            open_containing_folder
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the oct desktop application");
}

#[cfg(test)]
mod tests {
    /// The phone-shaped window, asserted from the configuration the application actually loads.
    ///
    /// A tray icon and a window's decorations are visual and cannot be asserted here; what can be
    /// asserted is that the window is declared undecorated, fixed and not maximisable, and that
    /// its proportions read as a phone rather than a small desktop window (A1, A2).
    #[test]
    fn the_window_is_phone_shaped_fixed_and_undecorated() {
        // The runtime parameter is explicit because the test never builds an app; it reads the
        // same compiled-in configuration `main` hands to the builder.
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let window = context
            .config()
            .app
            .windows
            .first()
            .expect("a window is declared");

        assert!(!window.decorations, "A1: the window must be undecorated");
        assert!(!window.resizable, "A1: the window must be a fixed size");
        assert!(!window.fullscreen, "A1: the window must not go fullscreen");
        assert!(!window.maximizable, "A2: the window must not be maximisable");
        assert!(window.minimizable, "A2: the window must be minimisable");

        // The size is pinned in both directions, so neither the user nor the window manager can
        // stretch it into a desktop window.
        assert_eq!(window.min_width, Some(window.width));
        assert_eq!(window.max_width, Some(window.width));
        assert_eq!(window.min_height, Some(window.height));
        assert_eq!(window.max_height, Some(window.height));

        // R5: the width widened from 390 to 430; the height and the shape did not move.
        assert_eq!(window.width, 430.0, "R5: the window width");
        assert_eq!(window.height, 844.0, "R5: the window height");

        let ratio = window.height / window.width;
        assert!(
            (1.9..=2.4).contains(&ratio),
            "the window must have phone proportions, but its height/width ratio is {ratio}"
        );
    }
}
