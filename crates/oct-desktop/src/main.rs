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

use std::path::{Path, PathBuf};

use commands::{
    FolderRemovalView, LibraryEntryView, OpenedFolderView, ProviderView, ScanReportView,
    WatchedFolderView,
};
use oct_core::OctError;
use oct_store::{database_path, Store};
use shell::{action_for_menu_id, dispatch, ShellAction, ShellActions};
use tauri::menu::{Menu, MenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, WindowEvent};
use tauri_plugin_dialog::{DialogExt, FilePath};

mod commands;
mod shell;
mod track;

/// The label of the window declared in `tauri.conf.json`. Referenced rather than repeated so the
/// tray and the commands always act on the same window.
const MAIN_WINDOW: &str = "main";

/// The tooltip of the tray icon. It is the product name rather than a translated word, so it does
/// not follow the language.
const TRAY_TOOLTIP: &str = "oct";

/// The tray icon's id, so the language command can find and re-word the existing tray instead of
/// building a second icon.
const TRAY_ID: &str = "oct-tray";

/// The event the desktop process emits to ask the interface to open a screen (R12). The interface
/// spells the same name in `web/src/lib/tauri.ts`; the payload is a screen name, because the
/// interface's own vocabulary is what names its screens — this process does not keep a second list.
const OPEN_SCREEN_EVENT: &str = "open-screen";

/// The screen the tray's settings entry asks for, as the interface's own vocabulary spells it.
const SETTINGS_SCREEN: &str = "settings";

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
        // A minimized window is not something the user can see, so the tray reports it as hidden;
        // `show` alone does not restore it on every window manager, so the restore is explicit
        // rather than assumed. Without this the tray could offer "Show OCT" and do nothing.
        self.window.unminimize().map_err(|error| error.to_string())?;
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
    let app = window.app_handle().clone();
    dispatch(ShellAction::Minimize, &AppShell::new(window))?;
    // A minimized window is reported as not visible, so the menu's visibility entry just changed
    // and must be re-worded (R14). The state is handed in rather than read back: the platform's
    // answer lags the action, and dispatch only returns success when the window accepted it.
    refresh_tray(&app, Some(false));
    Ok(())
}

/// Hides the window without closing it, so the tray can bring it back.
#[tauri::command]
fn hide_window(window: WebviewWindow) -> Result<(), String> {
    let app = window.app_handle().clone();
    dispatch(ShellAction::Hide, &AppShell::new(window))?;
    // Hiding changes what the menu should say, and the top bar is one of the two ways to hide (R14).
    // The state is handed in for the same reason as minimize: reading the window back right here
    // still returns the previous one, which is the defect the user found as a second click.
    refresh_tray(&app, Some(false));
    Ok(())
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

/// Sets or clears the reader's title for the document with `id` (R31).
///
/// `title: None` restores the document's own title, and is a legitimate action rather than an error.
/// A blank string is refused as an invalid argument by the decision in
/// [`commands::set_document_title`], so the interface receives the same named refusal every other
/// bad argument does.
#[tauri::command]
fn set_document_title(id: i64, title: Option<String>) -> Result<(), String> {
    let store = open_store()?;
    commands::set_document_title(&store, id, title.as_deref()).map_err(error_message)
}

/// Sets or clears the page the reader chose for the document's cover (R32).
///
/// `page: None` restores the default, the first page, and is a legitimate action rather than an error.
/// The page arrives zero-based from the interface, exactly as it is stored; the interface converts from
/// the one-based number it shows, at its own boundary.
#[tauri::command]
fn set_cover_page(id: i64, page: Option<u32>) -> Result<(), String> {
    let store = open_store()?;
    commands::set_cover_page(&store, id, page).map_err(error_message)
}

/// Opens the system file browser and installs the image the reader chose as the cover (R32).
///
/// The dialog is driven from Rust for exactly the reasons [`pick_folder`] documents: the interface
/// talks to our thin commands, not to a plugin, so no capability file changes and the decision stays
/// testable in [`commands::set_cover_image`].
///
/// The contract is: `true` when an image was chosen and installed, `false` when the user cancelled,
/// and `Err` for a real failure — including a chosen file that is not an image, which
/// [`commands::set_cover_image`] refuses as an invalid argument. Cancel is a normal answer and never an
/// error, so the interface can close its modal in both cases without inventing a message.
#[tauri::command]
async fn choose_cover_image(id: i64, app: AppHandle) -> Result<bool, String> {
    let chosen = chosen_file(
        app.dialog()
            .file()
            .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
            .blocking_pick_file(),
    )?;
    let Some(source) = chosen else {
        return Ok(false);
    };

    let data_directory = data_directory()?;
    let store = open_store()?;
    commands::set_cover_image(&store, id, &data_directory, Some(Path::new(&source)))
        .map_err(error_message)?;
    Ok(true)
}

/// Removes the image the reader supplied, so the document shows whichever page it shows (R32).
#[tauri::command]
fn remove_cover_image(id: i64) -> Result<(), String> {
    let data_directory = data_directory()?;
    let store = open_store()?;
    commands::set_cover_image(&store, id, &data_directory, None).map_err(error_message)
}

/// Returns the cover of the document with `id` as a data URL, or `null` when it has none (R32).
///
/// The work runs on [`tauri::async_runtime::spawn_blocking`] because rendering is CPU-bound and
/// pdfium is single-threaded behind this binding: a cover must never occupy the thread that paints
/// the interface while it is produced. The command itself decides nothing — the cache, the render
/// and the data URL are [`commands::document_cover`]'s, which is tested without an application.
///
/// `null` is the honest answer for a document that cannot be read, and it is not an error: the card
/// keeps its placeholder. The reason is written to stderr inside `commands::document_cover`.
#[tauri::command]
async fn document_cover(id: i64) -> Result<Option<String>, String> {
    let data_directory = data_directory()?;
    tauri::async_runtime::spawn_blocking(move || {
        let store = open_store()?;
        commands::document_cover(&store, id, &data_directory).map_err(error_message)
    })
    .await
    .map_err(|error| format!("the cover task could not run: {error}"))?
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

/// Computes what removing the watch with `id` would take with it, changing nothing (R27).
///
/// The confirmation reads this before offering the destructive action, so the count it states is
/// the store's answer rather than the screen's estimate.
#[tauri::command]
fn folder_removal(id: i64) -> Result<Option<FolderRemovalView>, String> {
    let store = open_store()?;
    commands::folder_removal(&store, id).map_err(error_message)
}

/// Stops watching a folder, taking the documents it covers and their positions with it (R27).
///
/// The reversal is deliberate: the user asked that a folder they stopped watching leave their
/// library, and the store keeps a document another remaining watch still covers. `None` means the
/// id named no watch, and nothing was deleted.
#[tauri::command]
fn remove_folder(id: i64) -> Result<Option<FolderRemovalView>, String> {
    let store = open_store()?;
    commands::remove_folder(&store, id).map_err(error_message)
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
    // The store now holds `locale`, so the shared refresh reads it back and words the tray exactly
    // as every other refresh does; there is no second path that could word it differently. Nothing
    // about the window changed here, so the visibility is read from the window as usual.
    rebuild_tray(&app, None)
}

/// Opens the folder that contains the document at `path`, reporting an honest error when the
/// folder is gone or nothing can open it.
#[tauri::command]
fn open_containing_folder(path: String) -> Result<OpenedFolderView, String> {
    let launcher = oct_platform::backends().launcher;
    commands::open_containing_folder(launcher.as_ref(), Path::new(&path)).map_err(error_message)
}

/// Opens the folder at `path` itself in the system's file manager (R24).
///
/// The watched-folder action: [`open_containing_folder`] opens a document's folder, this opens the
/// folder the user pointed at, and the two share one implementation and one honesty rule — a
/// folder that no longer exists is an error, never a silent no-op.
#[tauri::command]
fn open_folder(path: String) -> Result<OpenedFolderView, String> {
    let launcher = oct_platform::backends().launcher;
    commands::open_folder(launcher.as_ref(), Path::new(&path)).map_err(error_message)
}

/// Opens the system folder browser and returns the folder the user chose (R11).
///
/// The chooser is driven from Rust, through `tauri-plugin-dialog`'s `DialogExt`, rather than from the
/// interface through `@tauri-apps/plugin-dialog`, for two reasons. First, the interface talking to a
/// plugin directly would break this project's one interaction pattern: screens call our thin commands
/// and the decision lives in Rust, where it can be testable without a display. Second, it would add
/// an npm package where a crate already suffices for what is one system dialog.
///
/// Because the application calls its own plugin, no capability file change is needed: capabilities
/// gate what the webview may invoke over IPC, and the webview invokes nothing here.
///
/// The contract is frozen: the chosen absolute path as a `String`, or `null` when the user cancels.
/// Cancelling is a normal answer, not a failure, so it never becomes an `Err`; `Err` is reserved for
/// a real failure, a choice that cannot be turned into a path.
#[tauri::command]
async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    // The blocking variant is the plugin's own recommendation for asynchronous commands (its module
    // docs say so), and a Tauri async command does not run on the main thread, so waiting here does
    // not freeze the interface. It is also the route taken because it turns cancel into an ordinary
    // `Option` return instead of a callback that has to funnel the answer back out of a closure, which
    // is what keeps the frozen cancel-is-not-an-error contract expressible in one place below.
    chosen_folder(app.dialog().file().blocking_pick_folder())
}

/// The frozen result of a folder choice: the path as a `String`, `None` for a cancel, or an error.
///
/// It is kept apart from the command so the contract — cancel is a normal answer, and only a choice
/// that cannot become a path is a failure — can be asserted without opening a native dialog, which no
/// test can drive.
fn chosen_folder(chosen: Option<FilePath>) -> Result<Option<String>, String> {
    match chosen {
        None => Ok(None),
        Some(path) => {
            let path = path
                .into_path()
                .map_err(|error| format!("the chosen folder could not be read as a path: {error}"))?;
            // Paths are opaque UTF-8 everywhere in this project (R7), and the store only ever holds
            // valid UTF-8, so a path that cannot be spelled as UTF-8 is a real failure rather than a
            // value silently mangled by a lossy conversion.
            match path.to_str() {
                Some(path) => Ok(Some(path.to_string())),
                None => Err(format!("the chosen folder is not valid UTF-8: {}", path.display())),
            }
        }
    }
}

/// The frozen result of an image choice (R32): the path as a `String`, `None` for a cancel, or an error.
///
/// Deliberately the same shape as [`chosen_folder`] and, like it, kept apart from the command so the
/// contract can be asserted without opening a native dialog. A cancel is a normal answer; only a choice
/// that cannot become a UTF-8 path is a failure, because every path this project stores is UTF-8 (R7).
fn chosen_file(chosen: Option<FilePath>) -> Result<Option<String>, String> {
    match chosen {
        None => Ok(None),
        Some(path) => {
            let path = path
                .into_path()
                .map_err(|error| format!("the chosen image could not be read as a path: {error}"))?;
            match path.to_str() {
                Some(path) => Ok(Some(path.to_string())),
                None => Err(format!("the chosen image is not valid UTF-8: {}", path.display())),
            }
        }
    }
}

/// Opens the production database at its per-platform user data location.
///
/// Each command opens its own connection; the store is cheap to open and `oct` currently runs one
/// process, so this is simpler and safer than sharing a connection behind a lock.
fn open_store() -> Result<Store, String> {
    let path = database_path().map_err(error_message)?;
    Store::open(path).map_err(error_message)
}

/// The application's data directory: the directory the database lives in (L20).
///
/// The covers live beside the database rather than in a directory of their own resolved a second
/// time, so "the same place the database lives" is a fact of one path instead of two functions that
/// have to agree. `oct_store::database_path` already names that directory, and this takes its
/// parent rather than re-reading `$XDG_DATA_HOME` here.
fn data_directory() -> Result<PathBuf, String> {
    let database = database_path().map_err(error_message)?;
    database
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("the database path has no directory: {}", database.display()))
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

/// Builds a tray menu from `items`, in order, using the builder's own separator for a divider (R15).
///
/// The menu is drawn from the tested decision ([`shell::tray_menu_entries`]) rather than assembled
/// here, so the shape — entry, line, entry, line, entry — cannot drift from what the tests pin.
fn tray_menu(app: &AppHandle, items: &[shell::TrayMenuItem]) -> tauri::Result<Menu<tauri::Wry>> {
    let mut builder = MenuBuilder::new(app);
    for item in items {
        builder = match item {
            shell::TrayMenuItem::Entry(entry) => builder.text(entry.id, entry.label),
            shell::TrayMenuItem::Divider => builder.separator(),
        };
    }
    builder.build()
}

/// Whether the user can actually see the window right now.
///
/// The question is asked of the window itself, never remembered in a flag beside it, because a copy
/// would be a second truth that can drift from the window on screen (R14). A **minimized** window
/// counts as not visible: the user cannot see it, and the tray's Show restores it because
/// [`AppShell::show`] un-minimizes before showing, so the menu offers Show rather than claiming
/// Hide for a window that is not on screen. A failed query is treated as not visible for the same
/// reason — Show is the entry that cannot be wrong to offer, since clicking it only tries to reveal
/// the window.
fn window_is_visible(app: &AppHandle) -> bool {
    match app.get_webview_window(MAIN_WINDOW) {
        Some(window) => {
            let visible = window.is_visible().unwrap_or(false);
            let minimized = window.is_minimized().unwrap_or(false);
            visible && !minimized
        }
        None => false,
    }
}

/// The tray menu the stored language and the window's real visibility call for.
///
/// Reading the language here, from the same production database the commands use, is what lets every
/// caller refresh the menu without carrying a language around: there is one source, so two rebuilds
/// cannot disagree. A database that cannot be read worded nothing at launch either; the failure is
/// reported and English stands in, because hiding the window with no tray would leave no way back.
///
/// `visible_after` is `Some` only when the caller itself just performed the action, and it exists
/// because the platform's own answer lags that action. Measured on this desktop by driving the tray's
/// own menu over the session bus: after a hide the window is still read back as visible, while after
/// a show it is not, which is what left the menu one click behind the window — the user reported
/// having to click twice for the entry to change. The value is the outcome of an action that already
/// returned success (a failed action never reaches a rebuild), so at that instant it is the truthful
/// answer, and it is passed through rather than remembered: nothing is stored between refreshes.
fn current_tray_menu(
    app: &AppHandle,
    visible_after: Option<bool>,
) -> Result<Menu<tauri::Wry>, String> {
    let language = stored_language().unwrap_or_else(|error| {
        eprintln!("the stored language could not be read for the tray: {error}");
        None
    });
    let visible = visible_after.unwrap_or_else(|| window_is_visible(app));
    let items = shell::tray_menu_entries(language.as_deref(), visible);
    tray_menu(app, &items).map_err(|error| error.to_string())
}

/// Re-words the tray to match the stored language and the window's real visibility (R12–R15).
///
/// [`tauri::tray::TrayIcon::set_menu`] replaces the menu on the retained tray, and the manager keeps
/// the icon under [`TRAY_ID`] so no handle has to be threaded through the setup closure. On Linux the
/// replacement goes through the app-indicator's own `set_menu`, which is why the words and the
/// dividers can change while the ids stay fixed. Every path that can leave the menu stale — launch, a
/// language change, and any action that can show, hide or minimize the window — goes through here.
fn rebuild_tray(app: &AppHandle, visible_after: Option<bool>) -> Result<(), String> {
    let menu = current_tray_menu(app, visible_after)?;
    match app.tray_by_id(TRAY_ID) {
        Some(tray) => tray.set_menu(Some(menu)).map_err(|error| error.to_string()),
        None => Err("the tray icon is not present".to_string()),
    }
}

/// Re-words the tray and reports a failure to stderr.
///
/// The refresh is secondary to whatever changed the window: there is no screen to report a failed
/// rebuild to, and a failed window action must not be reported to the interface as a failed rebuild,
/// so the failure is written out rather than returned.
fn refresh_tray(app: &AppHandle, visible_after: Option<bool>) {
    if let Err(error) = rebuild_tray(app, visible_after) {
        eprintln!("the tray's menu could not be refreshed: {error}");
    }
}

/// Asks the interface to open the settings screen (R12).
///
/// One event whose payload is the screen's own name; the interface answers it through the routing it
/// already has, so a tray click and a navbar click land on the same screen by the same path. This is
/// a deliberately thin wrapper: the decision being tested is which entry exists and what id it
/// carries ([`shell::tray_menu_entries`]), and this emit is the part that cannot be asserted without
/// a running application. It is emitted application-wide because the interface listens without a
/// target, and there is one webview to reach.
fn ask_for_settings(app: &AppHandle) -> Result<(), String> {
    app.emit(OPEN_SCREEN_EVENT, SETTINGS_SCREEN)
        .map_err(|error| error.to_string())
}

/// Builds the tray: an icon whose menu matches the stored language and the window's state.
///
/// The tray is not decoration. The top bar offers "hide", and a hide button with no way back would
/// make the application vanish forever, which is a trap rather than a feature — that is why the
/// tray lands with the window chrome instead of in a later phase (A3, L20). The icon is the
/// application's own committed placeholder; there is no visual identity to invent.
fn setup_tray(app: &AppHandle) -> Result<(), String> {
    // At launch the window is created HIDDEN by the configuration, and that is a fact rather than a
    // guess — while the platform's own answer at this instant is not one: measured across two launches,
    // the read right after the window is created sometimes reports the opposite of the truth, which
    // worded the menu "Show OCT" over a window the user was looking at. Every other refresh reads the
    // window; this one knows.
    //
    // This line said `Some(true)` until R33, and it was correct then: the window WAS visible at launch,
    // because the configuration said so. Changing the configuration made it a lie, and nothing could
    // notice — the menu would have offered "Hide" over a window nobody could see, so the first click
    // would have done nothing at all. It is the same shape as three other defects this project has
    // found: a fact that was true, with no test able to notice it stopping being true.
    let menu = current_tray_menu(app, Some(false))?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip(TRAY_TOOLTIP)
        // On Linux only the menu is reachable (click events are not emitted), so the left click
        // must open the menu rather than doing nothing.
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            // Settings has no window action (R12): it asks the interface to navigate, and the tray
            // stays out of the interface's routing beyond naming a screen. It does bring the window
            // up first, though. The user reported that this entry "does not open the settings
            // screen, whether or not the application is open", and a screen nobody can see is not
            // an opened screen: navigating while the window is hidden changes a hash behind a
            // hidden window, which from the outside is indistinguishable from doing nothing.
            if id == shell::MENU_SETTINGS {
                let shown = match AppShell::from_app(app) {
                    Some(shell) => match dispatch(ShellAction::Show, &shell) {
                        Ok(()) => true,
                        Err(error) => {
                            eprintln!("the tray could not show the window for the settings screen: {error}");
                            false
                        }
                    },
                    None => false,
                };
                // The menu's visibility entry follows the window (R14). The state is handed in
                // rather than read back: a show is read back correctly, but the very same path
                // serves hide, where the platform's own answer lags, and one rule for both is
                // better than one that depends on which action ran.
                refresh_tray(app, Some(shown));
                if let Err(error) = ask_for_settings(app) {
                    eprintln!("the tray could not ask for the settings screen: {error}");
                }
                return;
            }
            let Some(action) = action_for_menu_id(id) else {
                return;
            };
            let Some(shell) = AppShell::from_app(app) else {
                return;
            };
            // There is no screen to report a failed window operation to, so it is written to
            // stderr rather than swallowed.
            if let Err(error) = dispatch(action, &shell) {
                eprintln!("the tray could not perform {action:?}: {error}");
                return;
            }
            // Show and Hide change what the menu should say; re-word it now rather than leave a
            // stale choice behind (R14). Quit ends the process, so there is nothing to refresh, and
            // minimize is not offered here. The state comes from the action that just ran, because
            // the window's own answer still reports the previous one at this instant.
            match action {
                ShellAction::Show => refresh_tray(app, Some(true)),
                ShellAction::Hide => refresh_tray(app, Some(false)),
                _ => {}
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app).map_err(|error| error.to_string())?;
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

/// Starts the ingestion loop: one catch-up sweep at launch, then a tick every
/// [`track::POLL_INTERVAL`].
///
/// Both halves run on Tauri's own runtime rather than the main thread. A tick walks `/proc` and a
/// sweep shells out to `gio`; neither belongs on the thread that paints the window, and `S5` asks
/// for the sweep to be off the main thread explicitly. The task is deliberately never joined: it is
/// the application's background job, and the process ending is its stop signal.
fn start_tracking() {
    tauri::async_runtime::spawn_blocking(|| {
        let detector = oct_platform::backends().open_documents;
        let providers = oct_platform::progress_providers();

        // The tick loop's memory of the library's document set. Removing a watched folder takes its
        // documents and their positions with it (R27); when a rescan re-adds them, this is what
        // notices the set changed and runs the launch sweep again, so the readings come back from
        // the viewer instead of staying at 0%.
        let mut watch = track::LibraryWatch::default();

        // S5: catch up once, over every known document, so a position written while the
        // application was closed is recorded instead of lost.
        match open_store() {
            Ok(store) => {
                report_tracking(&track::sweep_library(&store, &providers));
                // The sweep just observed this exact set, so seeding the watch keeps the first tick,
                // three seconds later, from re-reading the history for a set that has not changed. It
                // costs one extra library query at launch and saves a whole recently-used.xbel parse.
                watch.seed(&store);
            }
            Err(error) => eprintln!("the launch sweep could not open the store: {error}"),
        }

        // S1: record what is open while the application runs. A store that is briefly busy must not
        // end tracking, so a failed open is reported and the next tick tries again.
        loop {
            std::thread::sleep(track::POLL_INTERVAL);
            match open_store() {
                Ok(store) => {
                    // A re-added document is a new set of documents: sweep it as the launch does.
                    // When the set has not changed this is `None`, so the history is read once per
                    // sweep and never once per three-second tick.
                    if let Some(sweep) = watch.refresh(&store, &providers) {
                        report_tracking(&sweep);
                    }
                    report_tracking(&track::track_open_documents(
                        &store,
                        detector.as_ref(),
                        &providers,
                    ));
                }
                Err(error) => eprintln!("the ingestion tick could not open the store: {error}"),
            }
        }
    });
}

/// Writes one pass's outcome to stderr.
///
/// Missing capabilities and failures are logged apart, because they are different facts: one says
/// this machine cannot answer the question, the other says the answer went wrong. There is no
/// screen for either, so stderr is where they go.
fn report_tracking(report: &track::TrackReport) {
    for missing in &report.missing_capabilities {
        eprintln!("missing capability ({}): {}", missing.source, missing.reason);
    }
    for failure in &report.failures {
        eprintln!("ingestion: {failure}");
    }
}

fn main() {
    tauri::Builder::default()
        // The dialog plugin is initialized so the application can open the system folder browser
        // from Rust through `DialogExt` (R11). It is a plugin of this process, not of the interface.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let window = app
                .get_webview_window(MAIN_WINDOW)
                .expect("the main window is declared in tauri.conf.json");
            enforce_fixed_shape(&window);

            // The tray's menu is built from the stored language and the window's real visibility;
            // `current_tray_menu` reports a database it cannot read and falls back to English, so a
            // launch without a readable store still leaves a tray with a way back.
            setup_tray(app.handle())?;
            // The ingestion loop is what connects the detector and the providers to the store; it
            // runs for the life of the process and reports everything it cannot do to stderr.
            start_tracking();
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
            set_document_title,
            set_cover_page,
            choose_cover_image,
            remove_cover_image,
            document_cover,
            scan_folder,
            progress_providers,
            watched_folders,
            folder_removal,
            remove_folder,
            get_language,
            set_language,
            open_containing_folder,
            open_folder,
            pick_folder
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

    // --- R11: the folder chooser's frozen contract --------------------------------------------
    //
    // A native dialog cannot be driven by a test, so the command itself is not asserted. What is
    // asserted is the choice-to-contract mapping it delegates to: cancelling is a normal answer, a
    // chosen folder is its path, and a choice that cannot be a path is an error rather than a silent
    // `None` — the one failure mode that would be indistinguishable from a cancel to the interface.

    use super::{chosen_file, chosen_folder, FilePath};
    use std::path::PathBuf;
    use std::str::FromStr;

    #[test]
    fn cancelling_the_folder_chooser_is_a_normal_answer() {
        assert_eq!(chosen_folder(None), Ok(None));
    }

    #[test]
    fn a_chosen_folder_becomes_its_path() {
        let chosen = FilePath::from(PathBuf::from("/library/pdfs"));
        assert_eq!(
            chosen_folder(Some(chosen)),
            Ok(Some("/library/pdfs".to_string()))
        );
    }

    #[test]
    fn a_choice_that_cannot_be_a_path_is_an_error() {
        // `content://` is a URI with no filesystem path. It is a real failure and must surface as
        // an `Err`, never as `Ok(None)`, which the interface would render as a cancel.
        let chosen = FilePath::from_str("content://com.example/document/1").expect("parse a URI");
        assert!(chosen_folder(Some(chosen)).is_err());
    }

    // --- R32: the image chooser's frozen contract ---------------------------------------------
    //
    // The same three answers as the folder chooser, for the same reasons: cancelling is normal, a
    // chosen file is its path, and a choice that cannot be a path is an error rather than a silent
    // `None` the interface would read as a cancel.

    #[test]
    fn cancelling_the_image_chooser_is_a_normal_answer() {
        assert_eq!(chosen_file(None), Ok(None));
    }

    #[test]
    fn a_chosen_image_becomes_its_path() {
        let chosen = FilePath::from(PathBuf::from("/home/u/Pictures/cover.png"));
        assert_eq!(
            chosen_file(Some(chosen)),
            Ok(Some("/home/u/Pictures/cover.png".to_string()))
        );
    }

    #[test]
    fn an_image_choice_that_cannot_be_a_path_is_an_error() {
        let chosen = FilePath::from_str("content://com.example/document/1").expect("parse a URI");
        assert!(chosen_file(Some(chosen)).is_err());
    }
}

/// The window's capability file, and the one grant that makes the tray work at all.
///
/// The original defect (L34) was that the project had **no capabilities file**, so the core event API
/// rejected the listener in silence and the tray's settings entry did nothing — and nothing anywhere said
/// so. `cargo build` validates the file's SHAPE, not what it grants, and no test looked at its contents.
#[cfg(test)]
mod capability_grants {
    /// `include_str!` is deliberate: if the file disappears, the BUILD fails, which is the loud version of
    /// this defect. The assertions below cover the quiet version, where the file stays and loses a grant.
    const CAPABILITIES: &str = include_str!("../capabilities/main.json");

    #[test]
    fn the_window_may_listen_for_the_screens_the_tray_asks_for() {
        assert!(
            CAPABILITIES.contains("core:event:allow-listen"),
            "without this grant the tray's settings entry does nothing, silently: {CAPABILITIES}"
        );
        assert!(
            CAPABILITIES.contains("core:event:allow-unlisten"),
            "a listener the interface can never remove leaks for the life of the window: {CAPABILITIES}"
        );
    }
}

/// How the application introduces itself (R33).
///
/// The user was closing the app because its window jumped over whatever they were doing, and the
/// platform answer is measured rather than assumed: with tauri 2.12.3 and tao 0.37.1 a window that is
/// created VISIBLE is mapped at launch and the compositor focuses it, `focus: false` is only a hint that
/// tao re-enables after the first draw, and `gtk_window_set_focus_on_map` is not exposed at all. An
/// unmapped surface cannot take focus, so the app starts hidden and the tray is how it is asked for — a
/// user action, which is exactly when focusing is right.
///
/// The line is asserted because nothing else would notice it going away: the build validates the config's
/// SHAPE, not what it says, and a window that started visible again would only be reported by a person
/// being interrupted.
#[cfg(test)]
mod launch_never_steals_focus {
    /// `include_str!` fails the BUILD if the file disappears, which is the loud half.
    const CONFIG: &str = include_str!("../tauri.conf.json");

    #[test]
    fn the_main_window_is_not_mapped_at_launch() {
        assert!(
            CONFIG.contains("\"visible\": false"),
            "the app must never start visible (R33): {CONFIG}"
        );
        assert!(
            CONFIG.contains("\"label\": \"main\""),
            "this is the main window's config, so the assertion above is about the right window: {CONFIG}"
        );
    }
}
