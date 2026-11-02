//! The `mirk` desktop application.
//!
//! This is the shell a person uses: Inicio, Biblioteca and Configuración, reached from the bottom
//! navbar, inside a deliberately phone-shaped window with a custom top bar and a tray. The commands
//! below are deliberately thin — they open the production database and delegate to the plain
//! functions in [`commands`], or perform a [`shell`] action, which are tested without a Tauri
//! application.
//!
//! # The window shape is a product statement
//!
//! The window starts as `430x844` logical pixels — a phone's logical viewport, widened from 390 in R5 —
//! undecorated, and never resized by dragging an edge. This is not a limitation to apologise for: `mirk`
//! is meant to read as a phone-shaped companion that lives in the tray, not as a small desktop window
//! that happens to render a mobile layout. The custom top bar exists for the same reason — a native
//! title bar would bring a maximise button the user explicitly refused.
//!
//! Its size is a stored choice instead (`odd/tasks/window-layout.md`, S2): Ajustes → Ventana offers two,
//! three or four documents per row, and full screen. [`layout`] is the only place that turns those
//! choices into pixels, [`apply_window_layout`] applies one, and the same value is what the interface
//! reads to draw the grid — so the window and its cards cannot disagree about how many fit.
//!
//! The first-party reader, covers and the suggestion queue arrive in later tasks.

// Prevents an extra console window from appearing alongside the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::time::Duration;

use commands::{
    FolderRemovalView, LibraryEntryView, OpenedDocumentView, OpenedFolderView, ProviderView,
    ScanReportView, SuggestionView, WatchedFolderView,
};
use mirk_core::MirkError;
use mirk_store::{database_path, Store};
use shell::{action_for_menu_id, dispatch, ShellAction, ShellActions};
use tauri::menu::{Menu, MenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, WebviewWindow, WindowEvent};
use tauri_plugin_dialog::{DialogExt, FilePath};

mod commands;
mod layout;
mod shell;
mod track;
mod watch;

/// The subdirectory of the bundle's resources that carries the pdfium native library (S5).
///
/// It mirrors the key of `bundle.resources` in `tauri.conf.json`, which copies
/// `vendor/pdfium/lib/*` into `pdfium/` inside the package; the two spellings are one fact, so they
/// are changed together. `mirk-scan` appends the platform's library file name to this directory, so
/// only the directory is named here.
const PDFIUM_RESOURCE_DIRECTORY: &str = "pdfium";

/// The label of the window declared in `tauri.conf.json`. Referenced rather than repeated so the
/// tray and the commands always act on the same window.
const MAIN_WINDOW: &str = "main";

/// The tooltip of the tray icon. It is the product name rather than a translated word, so it does
/// not follow the language.
const TRAY_TOOLTIP: &str = "mirk";

/// The tray icon's id, so the language command can find and re-word the existing tray instead of
/// building a second icon.
const TRAY_ID: &str = "mirk-tray";

/// The event the desktop process emits to ask the interface to open a screen (R12). The interface
/// spells the same name in `web/src/lib/tauri.ts`; the payload is a screen name, because the
/// interface's own vocabulary is what names its screens — this process does not keep a second list.
const OPEN_SCREEN_EVENT: &str = "open-screen";

/// The screen the tray's settings entry asks for, as the interface's own vocabulary spells it.
const SETTINGS_SCREEN: &str = "settings";

/// The event the desktop process emits when it changes the stored window layout without the interface
/// having asked (S2 of `odd/tasks/window-drag.md`). The interface spells the same name in `web/src`; the
/// payload is the layout key as a plain string, because the keys are the vocabulary both halves already
/// share — this process does not keep a second list of layouts to translate between.
const WINDOW_LAYOUT_EVENT: &str = "window-layout-changed";

/// How often the window's real shape is compared with the stored layout (S2).
///
/// One second, and two consecutive disagreeing ticks are needed before anything is done about them — see
/// [`start_reconciling`] for why both numbers are what they are.
const RECONCILE_INTERVAL: Duration = Duration::from_secs(1);

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
        // rather than assumed. Without this the tray could offer "Show Mirk" and do nothing.
        self.window
            .unminimize()
            .map_err(|error| error.to_string())?;
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
///
/// R5: this is a true minimise and nothing else. An earlier version hid the window when the platform
/// had not reported the minimised state within 250 ms, to work around a measured no-op; the user's
/// report that "minimise works like a tray button" is that workaround firing, so it is gone. The
/// iconify request is the whole command, and if the compositor refuses it the tray's Hide remains the
/// separate, explicit way to take the window off the screen.
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

/// Saves the reader's naming of the document with `id` in ONE write (R31, R37, R42/S5).
///
/// The editor's Save decides the reader's title and the reader's authors in one action, so this is one
/// command: two commands each opening the store and each writing one fact is how a failed author write
/// used to leave the title saved while the screen said nothing was.
///
/// The shapes the interface can send are deliberately distinct, because the editor's controls decide
/// different things:
///
/// - `title: null` restores the document's own title, a legitimate action rather than an error. A blank
///   string is refused as an invalid argument by [`commands::set_document_naming`].
/// - `authors: ["Carl Sagan", "Ann Druyan"]` is the reader's ordered list (R42/S5). An **empty list**
///   — `authors: []` — is *no author of my own*: a decision, deliberately different from the title,
///   because an author list may be empty and the document's own author then shows again.
/// - `authors` **absent from the call** means this action said nothing about the authors, so the
///   reader's list is left exactly as it is. That is how Restore decides the title and nothing else. An
///   absent argument is `None` here because the outer `Option` makes it optional to send, and the web
///   side says it by omitting the key: `JSON.stringify` drops an `undefined` property, and `authors?:
///   readonly string[]` in the editor is exactly this shape.
///
/// **`author` is the pre-list spelling and it is still accepted** (a native review found this): a
/// caller that has not moved to the list sends `author: "Carl Sagan"`, and dropping that key would make
/// `authors` arrive absent, which the store reads as *said nothing* — the save would report success and
/// the reader's edit would be discarded in silence. So the single name is mapped to a one-entry list,
/// and `authors` **wins** when both are sent: one caller, one decision, and the newer spelling is the
/// one that carries an order.
#[tauri::command]
fn set_document_naming(
    id: i64,
    title: Option<String>,
    authors: Option<Vec<String>>,
    // The pre-list spelling keeps the type it always had. A review pointed out that a single
    // `Option<String>` would make an old caller's `author: null` indistinguishable from an absent key,
    // and the two-option type is what says *the shape distinguishes them* — including to anyone reading
    // the signature.
    //
    // One measured limitation, because a reviewer should not have to rediscover it: **at this IPC
    // boundary they cannot actually be told apart.** Tauri's argument deserializer calls
    // `visit_none()` both for a missing key and for a JSON `null`
    // (`tauri-2.12.3/src/ipc/command.rs`, `impl Deserializer for CommandItem`, `deserialize_option`),
    // so `author: null` and an absent `author` both arrive as `None`. That is not a regression this
    // change introduced — the pre-list command had the identical type and therefore the identical
    // behaviour — and it is not fixable without a custom `CommandArg`. What a pre-list caller CAN do,
    // and now still can, is send a name, which becomes a one-entry list. Clearing the reader's author is
    // expressible only through the list spelling (`authors: []`), which is why the editor moved to it.
    author: Option<Option<String>>,
) -> Result<(), String> {
    let store = open_store()?;
    commands::set_document_naming(&store, id, title.as_deref(), authored(authors, author))
        .map_err(error_message)
}

/// The reader's author list, from whichever spelling the caller sent.
///
/// `authors` is the list the editor sends, and it is the answer whenever it is present — an empty list
/// included, because *no author of my own* is a decision. The pre-list `author` keeps its baseline
/// meaning exactly: a name becomes a one-entry list, an explicit `null` becomes the empty list (the
/// reader clearing their author), and a blank name becomes the empty list for the same reason a blank
/// is never an author. Both absent is `None`: this action said nothing about the authors.
///
/// A free function rather than a line inside the command so the rule can be tested without a Tauri
/// application, which is the same reason every other decision in this workspace lives outside its
/// wrapper.
fn authored(authors: Option<Vec<String>>, author: Option<Option<String>>) -> Option<Vec<String>> {
    match (authors, author) {
        (Some(list), _) => Some(list),
        // `Some(None)` is an explicit null: *no author of my own*, the empty list.
        (None, Some(None)) => Some(Vec::new()),
        (None, Some(Some(name))) if name.trim() == "" => Some(Vec::new()),
        (None, Some(Some(name))) => Some(vec![name]),
        (None, None) => None,
    }
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

/// Makes the image the reader already supplied the cover again (R19).
///
/// The other half of "a page can take over": `set_cover_page` switches the cover to a page and keeps
/// the image stored, and this brings the stored image back without choosing the file again.
#[tauri::command]
fn use_cover_image(id: i64) -> Result<(), String> {
    let store = open_store()?;
    commands::use_cover_image(&store, id).map_err(error_message)
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

/// Returns the first page of a suggested document as a data URL, or `null` when it has none (L60).
///
/// The path comes from the suggestion queue, which is the application's own observation. Nothing is
/// stored: a preview is a look at a file the reader has not accepted, and if they accept it the
/// library produces its own cover. The work runs off the interface thread for the same reason a
/// library cover does — rendering is CPU-bound and pdfium is single-threaded behind the binding.
#[tauri::command]
async fn suggestion_cover(path: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        commands::suggestion_cover(Path::new(&path)).map_err(error_message)
    })
    .await
    .map_err(|error| format!("the preview task could not run: {error}"))?
}

/// Reports whether this machine can say which documents are open (R4).
///
/// The queue's own capability. It is what keeps an empty queue from looking like good news on a
/// machine that has no detector, which is the case the producer's own doc comment names.
#[tauri::command]
fn suggestion_detector() -> commands::DetectorView {
    commands::suggestion_detector(mirk_platform::backends().open_documents.as_ref())
}

/// Reads the desktop's recent-files history into the suggestion queue (S10).
///
/// The interface asks for this before listing the queue, so the screen shows what the reader's own
/// desktop recorded opening — each suggestion carrying the moment the history wrote down. It is not
/// part of the ingestion tick: the tick runs every few seconds, and reading the history that often
/// would bump every suggestion's sighting count for no reason and re-read a file nobody changed.
///
/// Reading the history is file I/O over a list another application maintains, so it runs off the
/// interface thread. A platform with no backend answers `NotSupported` and the caller can say the
/// capability is missing instead of showing an empty queue that merely looks like good news.
#[tauri::command]
async fn observe_recent_documents() -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = open_store()?;
        let source = mirk_platform::backends().recent_documents;
        commands::observe_recent_documents(&store, source.as_ref()).map_err(error_message)
    })
    .await
    .map_err(|error| format!("the recent-documents task could not run: {error}"))?
}

/// Scans `folder` into the library and returns the report, including any per-file failure.
#[tauri::command]
fn scan_folder(folder: String) -> Result<ScanReportView, String> {
    let store = open_store()?;
    let identity = mirk_platform::backends().file_identity;
    commands::scan_path(&store, identity.as_ref(), Path::new(&folder)).map_err(error_message)
}

/// Reports every progress provider with its capability and whether this machine can use it.
#[tauri::command]
fn progress_providers() -> Vec<ProviderView> {
    commands::provider_registry()
}

/// Lists the documents the application observed open outside the watched folders (R4).
///
/// Only the undecided queue: a decision already taken is history, and it is not offered again.
#[tauri::command]
fn list_suggestions() -> Result<Vec<SuggestionView>, String> {
    let store = open_store()?;
    commands::list_suggestions(&store).map_err(error_message)
}

/// Accepts a suggestion, adding its document to the library (R4).
///
/// This is a reader action and only a reader action: nothing in the application calls it on its
/// own. The document is counted and persisted before the decision is recorded, so a file that has
/// since disappeared leaves the suggestion waiting rather than silently accepted.
///
/// The scan is CPU-bound and pdfium is single-threaded behind the binding, so it runs off the
/// interface thread for the same reason a cover render does.
#[tauri::command]
async fn accept_suggestion(id: i64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = open_store()?;
        let identity = mirk_platform::backends().file_identity;
        commands::accept_suggestion(&store, identity.as_ref(), id).map_err(error_message)
    })
    .await
    .map_err(|error| format!("the suggestion task could not run: {error}"))?
}

/// Dismisses a suggestion, recording that the reader declined it (R4).
#[tauri::command]
fn discard_suggestion(id: i64) -> Result<(), String> {
    let store = open_store()?;
    commands::discard_suggestion(&store, id).map_err(error_message)
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

/// The stored window layout, when it is one this build knows (S3).
#[tauri::command]
fn get_window_layout() -> Result<Option<String>, String> {
    stored_layout()
}

/// The reader's remembered windowed layout, always one of the three presets (S3).
///
/// It is a second read beside [`get_window_layout`] because the two are different facts: the layout says
/// what the window is now — `auto` for full screen — and the preset says which rows a window that leaves
/// full screen comes back to. The interface needs the second one at startup, or its double click and the
/// process's own drag answer the same question with different values. The answer is never `auto`:
/// [`preset_from`] resolves through [`layout::preset`], so an absent key, an unusable value and a stored
/// `auto` all come back as three per row, and a store that cannot be read is the command's error.
#[tauri::command]
fn get_window_preset() -> Result<String, String> {
    stored_preset().map(|preset| preset.key().to_string())
}

/// Stores the chosen layout, remembers it as the preset when it is a windowed one, and sizes the window
/// from it.
///
/// One command rather than two so the store and the window cannot disagree: the value is validated and
/// written first, and only then does the window take its new size. A refused value never reaches the
/// store and the window is untouched; a write that fails leaves the window as it was too, so what is read
/// and what is drawn stay one answer either way. The tray is not touched at all — the window's size is not
/// something the tray says. The preset is written by [`remember_layout`], which is also what the
/// reconciliation loop calls, so the two cannot remember different things.
#[tauri::command]
fn set_window_layout(layout: String, window: WebviewWindow) -> Result<(), String> {
    let chosen = self::layout::WindowLayout::parse(&layout)
        .ok_or_else(|| format!("`{layout}` is not a window layout this build knows"))?;
    let store = open_store()?;
    remember_layout(&store, chosen).map_err(error_message)?;
    apply_window_layout(&window, chosen)
}

/// Opens the document the card names, in the system's own application for it.
///
/// The document is identified by its row id, not by a path the interface hands over: the store
/// holds the canonical path, and the page the document resumes at is the newest recorded position
/// — the very number the card shows, so the card and the document cannot disagree.
///
/// The viewer's page capability is the platform's business: a viewer that cannot be asked for a
/// page still opens the document, and the answer names the program that actually ran.
#[tauri::command]
fn open_document(id: i64) -> Result<OpenedDocumentView, String> {
    let store = open_store()?;
    let launcher = mirk_platform::backends().launcher;
    commands::open_document(&store, launcher.as_ref(), id).map_err(error_message)
}

/// Opens the folder that contains the document at `path`, reporting an honest error when the
/// folder is gone or nothing can open it.
#[tauri::command]
fn open_containing_folder(path: String) -> Result<OpenedFolderView, String> {
    let launcher = mirk_platform::backends().launcher;
    commands::open_containing_folder(launcher.as_ref(), Path::new(&path)).map_err(error_message)
}

/// Opens the folder at `path` itself in the system's file manager (R24).
///
/// The watched-folder action: [`open_containing_folder`] opens a document's folder, this opens the
/// folder the user pointed at, and the two share one implementation and one honesty rule — a
/// folder that no longer exists is an error, never a silent no-op.
#[tauri::command]
fn open_folder(path: String) -> Result<OpenedFolderView, String> {
    let launcher = mirk_platform::backends().launcher;
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
            let path = path.into_path().map_err(|error| {
                format!("the chosen folder could not be read as a path: {error}")
            })?;
            // Paths are opaque UTF-8 everywhere in this project (R7), and the store only ever holds
            // valid UTF-8, so a path that cannot be spelled as UTF-8 is a real failure rather than a
            // value silently mangled by a lossy conversion.
            match path.to_str() {
                Some(path) => Ok(Some(path.to_string())),
                None => Err(format!(
                    "the chosen folder is not valid UTF-8: {}",
                    path.display()
                )),
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
            let path = path.into_path().map_err(|error| {
                format!("the chosen image could not be read as a path: {error}")
            })?;
            match path.to_str() {
                Some(path) => Ok(Some(path.to_string())),
                None => Err(format!(
                    "the chosen image is not valid UTF-8: {}",
                    path.display()
                )),
            }
        }
    }
}

/// Opens the production database at its per-platform user data location.
///
/// Each command opens its own connection; the store is cheap to open and `mirk` currently runs one
/// process, so this is simpler and safer than sharing a connection behind a lock.
fn open_store() -> Result<Store, String> {
    let path = database_path().map_err(error_message)?;
    Store::open(path).map_err(error_message)
}

/// The application's data directory: the directory the database lives in (L20).
///
/// The covers live beside the database rather than in a directory of their own resolved a second
/// time, so "the same place the database lives" is a fact of one path instead of two functions that
/// have to agree. `mirk_store::database_path` already names that directory, and this takes its
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

/// The window layout stored in the production database, or an error naming the failure.
///
/// It stands beside [`stored_language`] for the same reason: `setup` reads it before the window is
/// shown, so a restart restores the size the reader chose instead of opening at the default and jumping
/// once the interface mounts.
fn stored_layout() -> Result<Option<String>, String> {
    let store = open_store()?;
    commands::get_window_layout(&store).map_err(error_message)
}

/// The windowed layout `store` remembers, or [`layout::RESTORE_LAYOUT`] when nothing usable is stored.
///
/// It reads the same `settings` table [`stored_layout`] reads, one key over, and [`layout::preset`] is
/// where *nothing usable* is decided: an absent key, a value this build does not know and a stored `auto`
/// all come back as three per row, because full screen is not a preset. A store that cannot be read is an
/// error rather than the fallback, so the caller can say so and try again instead of quietly changing the
/// reader's rows. Taking the store as an argument is what lets a test assert the read without a database
/// on disk; the reconciliation loop and the command both come through here.
fn preset_from(store: &Store) -> Result<layout::WindowLayout, String> {
    let stored = store
        .get_setting(layout::PRESET_SETTING)
        .map_err(error_message)?;
    Ok(layout::preset(stored.as_deref()))
}

/// The windowed layout the reader last chose, read from the production database.
fn stored_preset() -> Result<layout::WindowLayout, String> {
    let store = open_store()?;
    preset_from(&store)
}

/// Stores `chosen` as the window layout, and as the reader's preset when it is a windowed one (S3).
///
/// The preset is what a window that leaves full screen comes back to, so only a windowed choice writes it:
/// `auto` is full screen, not a preset, and choosing it must leave the rows the reader last chose in
/// force. The layout itself is written by [`commands::set_window_layout`], the same call the command has
/// always made, so there is one write path and one place that refuses a value this build does not know.
fn remember_layout(store: &Store, chosen: layout::WindowLayout) -> Result<(), MirkError> {
    // The canonical key is stored, never the caller's spelling: `parse` accepted it as one of the four,
    // and the store keeps the one the build names.
    commands::set_window_layout(store, chosen.key())?;
    match chosen.columns() {
        Some(_) => store.set_setting(layout::PRESET_SETTING, chosen.key()),
        None => Ok(()),
    }
}

/// Renders an error into the string the frontend shows. `MirkError` is not `Serialize`, and the
/// screen only needs to display the message, so the conversion happens at the IPC boundary.
fn error_message(error: MirkError) -> String {
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
    // worded the menu "Show Mirk" over a window the user was looking at. Every other refresh reads the
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

/// Sizes the window from `layout`: a pinned logical size, or full screen.
///
/// `tauri.conf.json` declares the window non-resizable, and on this Wayland/GTK host that value does not
/// survive window creation — so a resize request reaches a window the window manager will not let grow.
/// The flag is therefore lifted only for the one call that needs a new size and put back immediately:
/// the reader's window stays fixed between layouts, and choosing a layout is the one moment its size is
/// supposed to change. Centering after the change keeps a widened window on screen instead of letting it
/// grow off the right edge; a window manager that refuses to center is not an error, because the size
/// already reached the window.
/// Sizes the window from `layout`: a row's own width, or the work area.
///
/// Two properties of this host (Wayland/GNOME with WebKitGTK) decide the shape of this function, and
/// both were measured rather than guessed:
///
/// - `resizable: false` is a promise the window manager enforces. While it is in force, a programmatic
///   `set_size` is ignored, which is why the first attempt left three and four per row squeezing their
///   cards into the shipped 430 px window. The flag is therefore left `true`, and the window is made
///   fixed by pinning its minimum and maximum size to the same value: that is what stops an edge drag,
///   not the flag, and it can be re-pinned whenever the reader chooses another layout.
/// - Native full screen is ignored here: `set_fullscreen(true)` returns `Ok` and `is_fullscreen()`
///   reports `true` while the window keeps its size. The compositor's own maximise does fill the work
///   area, so "Pantalla completa" is carried by `maximize()` and the grid's `auto-fill` does the rest —
///   the request asks for the documents that fit, and this is the state in which the width decides.
fn apply_window_layout(window: &WebviewWindow, layout: layout::WindowLayout) -> Result<(), String> {
    // A pin from the previous layout would clamp the size being asked for, so both hints are released
    // before anything else. The order matters: setting the maximum below a still-pinned minimum is
    // refused by GTK, and clearing them first is what makes every transition work in both directions.
    window
        .set_min_size(None::<tauri::Size>)
        .map_err(|error| format!("the window's minimum size could not be released: {error}"))?;
    window
        .set_max_size(None::<tauri::Size>)
        .map_err(|error| format!("the window's maximum size could not be released: {error}"))?;
    window
        .set_resizable(true)
        .map_err(|error| format!("the window could not be resized: {error}"))?;

    let Some((width, height)) = layout.window_size() else {
        if window.is_maximized().unwrap_or(false) {
            return Ok(());
        }
        return window
            .maximize()
            .map_err(|error| format!("the window could not fill the screen: {error}"));
    };

    // Leaving a maximized window for a preset needs the maximize state cleared first, or the
    // compositor keeps the work-area size and ignores the requested one.
    if window.is_maximized().unwrap_or(false) {
        window
            .unmaximize()
            .map_err(|error| format!("the window could not leave the work area: {error}"))?;
    }

    let size = LogicalSize::new(width, height);
    window
        .set_size(size)
        .map_err(|error| format!("the window could not take its {width}x{height} size: {error}"))?;
    // The pin is what makes the window fixed at the size the layout names: equal minimum and maximum
    // leave the window manager no room to stretch it and no reason to ignore the next requested size,
    // because the next apply releases both before asking.
    window
        .set_min_size(Some(size))
        .map_err(|error| format!("the window's minimum size could not be pinned: {error}"))?;
    window
        .set_max_size(Some(size))
        .map_err(|error| format!("the window's maximum size could not be pinned: {error}"))?;
    Ok(())
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
        let detector = mirk_platform::backends().open_documents;
        let providers = mirk_platform::progress_providers();

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
                    // R4: the same detector feeds the reviewable suggestion queue. This is a second
                    // walk of `/proc` on this tick — `track_open_documents` made the first — and it
                    // is deliberate: the queue is populated only from what the application actually
                    // observed open, and the two callers want different things from the same
                    // snapshot. The cost is one best-effort directory walk, and the alternative of
                    // threading the snapshot through `track` would put queue policy inside the
                    // ingestion loop that deliberately keeps it out (a document the store does not
                    // know is ignored there rather than added, so the queue stays the only producer).
                    if let Err(error) = commands::observe_open_documents(&store, detector.as_ref())
                    {
                        eprintln!("suggestion detection: {error}");
                    }
                }
                Err(error) => eprintln!("the ingestion tick could not open the store: {error}"),
            }
        }
    });
}

/// Keeps the reader's folders watched while the application runs (S3–S4 of `odd/tasks/live-watch.md`).
///
/// A folder was previously read exactly once — at the moment it was added — so a PDF that appeared
/// afterwards never entered the library, and re-scanning that folder is refused on purpose because a
/// second watch would discover every document in it twice. The firing and the writing are not this
/// function's business: the comparison lives on [`tauri::async_runtime::spawn_blocking`]'s thread
/// because it is one indexed read every [`watch::REARM_INTERVAL`], and the watch itself runs on a
/// blocking thread of its own because [`mirk_scan::WatchBridge::watch`] blocks by contract.
///
/// Every time the watch is (re)armed, [`watch::catch_up`] counts the documents that arrived while the
/// application was closed. That is the other half of the same wire (S6): a watch reports changes from
/// the moment it arms, so without the pass a PDF added between two launches stays invisible forever.
///
/// The task is deliberately never joined, for the same reason the ingestion loop is not: it is the
/// application's background job, and the process ending is its stop signal.
fn start_watching() {
    tauri::async_runtime::spawn_blocking(|| {
        let mut armed: Option<watch::Armed> = None;

        loop {
            // A store that cannot be read is reported and retried: an application that stopped
            // watching because a database was briefly busy would go stale without saying so.
            match open_store().and_then(|store| {
                let folders = store.folders().map_err(error_message)?;
                Ok((store, folders))
            }) {
                Ok((store, folders)) => {
                    let current = watch::targets(&folders);
                    let stale = armed
                        .as_ref()
                        .map(|previous| watch::is_stale(previous.targets(), &current))
                        .unwrap_or(true);
                    if stale {
                        // Stopping joins the old thread, so the store it owned is closed before the
                        // next watch opens its own. No folder means no thread to run.
                        if let Some(previous) = armed.take() {
                            previous.stop();
                        }
                        if current.is_empty() {
                            // A silent "nothing to watch" is indistinguishable from a broken
                            // watcher, and this feature already failed twice by being silent. One
                            // line when the set becomes empty is the trace that says which of the
                            // two the reader is looking at.
                            eprintln!(
                                "watching nothing: the library has no enabled folder to watch, so a \
                                 document added while the application runs will not be seen"
                            );
                        } else {
                            for target in &current {
                                eprintln!(
                                    "watching: {} ({})",
                                    target.path.display(),
                                    if target.recursive {
                                        "recursive"
                                    } else {
                                        "this folder only"
                                    }
                                );
                            }
                            armed = Some(watch::Armed::start(store, current));
                            // S6: the watch only sees changes from the moment it arms, so the
                            // documents that arrived while the application was closed are counted
                            // here. After the watch is armed, never before: a document created
                            // while this pass runs is then reported by the watch instead of
                            // falling into the gap between the two. It runs on its own connection
                            // because the armed thread owns the store it was given.
                            match open_store() {
                                Ok(store) => watch::catch_up(&store, &folders),
                                Err(error) => eprintln!(
                                    "the launch catch-up pass could not open the store: {error}"
                                ),
                            }
                        }
                    }
                }
                Err(error) => eprintln!("the watched folders could not be read: {error}"),
            }

            std::thread::sleep(watch::REARM_INTERVAL);
        }
    });
}

/// Keeps the stored layout and the window's real shape from disagreeing (S2).
///
/// The window manager's drag gesture changes the window behind everyone's back: dragging a maximised
/// window by its bar unmaximises it, while the store still says `auto`, so the interface goes on drawing
/// the sidebar, the pressed maximise control and the auto-filling grid over a window that is no longer
/// full screen. This host tells the process nothing reliable about that — `set_fullscreen` is accepted and
/// ignored here and `is_minimized` never becomes true — so the question is asked once a second instead:
/// the window's real maximise state against what the store says, compared by [`layout::reconcile`]. When
/// the rule fires, the reader's preset becomes the stored layout, the window takes it through
/// [`apply_window_layout`], and [`WINDOW_LAYOUT_EVENT`] tells the interface to redraw now rather than at
/// its next read.
///
/// **Two consecutive disagreeing ticks, not one.** A single disagreement can be a state in flight: the
/// maximize `setup` asks for, or a layout being applied, is a moment when the window and the store are
/// honestly mid-sentence. It can also be the middle of the drag itself, and acting there would pin the
/// window's size while the compositor still holds it under the pointer. A second tick a second later
/// cannot be any of those, because each settles within one tick; the cost is one second before the
/// correction the reader is watching for, which is cheaper than a layout fighting the gesture that
/// caused it.
///
/// A store that cannot be read is reported and retried, never fatal — the same rule as the two loops
/// above, and for the same reason: an application that stopped reconciling because a database was
/// briefly busy would go on drawing full screen over a windowed window for the rest of the session.
/// The task is deliberately never joined: the process ending is its stop signal.
fn start_reconciling(app: AppHandle) {
    tauri::async_runtime::spawn_blocking(move || {
        let mut disagreed = false;

        loop {
            std::thread::sleep(RECONCILE_INTERVAL);
            let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
                // The window is the whole subject here, so there is nothing to compare without it.
                disagreed = false;
                continue;
            };
            let stored = match stored_layout() {
                Ok(stored) => stored
                    .as_deref()
                    .and_then(layout::WindowLayout::parse)
                    .unwrap_or(layout::DEFAULT_LAYOUT),
                Err(error) => {
                    eprintln!("the stored window layout could not be read: {error}");
                    disagreed = false;
                    continue;
                }
            };
            let maximised = match window.is_maximized() {
                Ok(maximised) => maximised,
                Err(error) => {
                    eprintln!("the window's maximise state could not be read: {error}");
                    disagreed = false;
                    continue;
                }
            };
            let preset = match stored_preset() {
                Ok(preset) => preset,
                Err(error) => {
                    eprintln!("the stored window preset could not be read: {error}");
                    disagreed = false;
                    continue;
                }
            };

            let Some(reconciled) = layout::reconcile(stored, maximised, preset) else {
                disagreed = false;
                continue;
            };
            if !disagreed {
                disagreed = true;
                continue;
            }
            disagreed = false;

            // The store goes first, so the interface reads the layout the window is about to take even
            // if applying it fails; a failure is reported and the next tick tries again, because the
            // two still disagree.
            let remembered = open_store()
                .and_then(|store| remember_layout(&store, reconciled).map_err(error_message));
            if let Err(error) = remembered {
                eprintln!("the reconciled window layout could not be stored: {error}");
                continue;
            }
            if let Err(error) = apply_window_layout(&window, reconciled) {
                eprintln!("the reconciled window layout could not be applied: {error}");
                continue;
            }
            // The interface's layout changed without it asking, so it is told rather than left to notice
            // on its next read: the grid, the sidebar and the maximise control are all drawn from this
            // one value.
            if let Err(error) = app.emit(WINDOW_LAYOUT_EVENT, reconciled.key()) {
                eprintln!("the interface could not be told the layout changed: {error}");
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
        eprintln!(
            "missing capability ({}): {}",
            missing.source, missing.reason
        );
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
        // S7: the update channel. The plugin is registered unconditionally and reads its endpoints and
        // the public key from `plugins.updater` in `tauri.conf.json`; with no configuration yet it answers
        // every check with an error, which the interface reports as "could not check" instead of
        // pretending the version is current. Silence here would be the exact defect the check exists to
        // avoid — an installed copy that quietly never updates.
        .plugin(tauri_plugin_updater::Builder::new().build())
        // S7: and this is what turns an installed update into a running one. Kept beside the updater
        // because they are one story: install writes the new bundle, relaunch enters it.
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // S5: an installed application carries pdfium inside its own package, where neither
            // `vendor/pdfium` nor `$MIRK_PDFIUM_LIB` exists. The resource directory is the one place
            // that knows where a bundle put its files, so it is resolved here, once, and handed to
            // `mirk-scan` before any scan, watch event or cover render can ask where pdfium is. In
            // development the resolver reports the same directory, which holds no bundled library,
            // so resolution falls through to `vendor/pdfium/lib` and the developer sets nothing.
            // A resource directory that cannot be resolved is reported rather than fatal: the
            // development chain still works without it, and a genuinely missing library still fails
            // with the one actionable error that names every remedy.
            let packaged_pdfium = match app.path().resource_dir() {
                Ok(directory) => Some(directory.join(PDFIUM_RESOURCE_DIRECTORY)),
                Err(error) => {
                    eprintln!("the bundle's resource directory could not be resolved: {error}");
                    None
                }
            };
            mirk_scan::set_packaged_pdfium_directory(packaged_pdfium);

            let window = app
                .get_webview_window(MAIN_WINDOW)
                .expect("the main window is declared in tauri.conf.json");
            // S3: the size is restored before the window is first shown, so a restart opens where the
            // reader left it. A store that cannot be read is reported and the default stands; a size is
            // worth a warning, never a launch that refuses to open.
            let restored = match stored_layout() {
                Ok(stored) => stored
                    .as_deref()
                    .and_then(layout::WindowLayout::parse)
                    .unwrap_or(layout::DEFAULT_LAYOUT),
                Err(error) => {
                    eprintln!("the stored window layout could not be read: {error}");
                    layout::DEFAULT_LAYOUT
                }
            };
            if let Err(error) = apply_window_layout(&window, restored) {
                eprintln!("the stored window layout could not be applied: {error}");
            }

            // The tray's menu is built from the stored language and the window's real visibility;
            // `current_tray_menu` reports a database it cannot read and falls back to English, so a
            // launch without a readable store still leaves a tray with a way back.
            setup_tray(app.handle())?;
            // The ingestion loop is what connects the detector and the providers to the store; it
            // runs for the life of the process and reports everything it cannot do to stderr.
            start_tracking();
            // S3 of `odd/tasks/live-watch.md`: a folder is not read once and forgotten. Without
            // this, a PDF that appeared after the reader added the folder was invisible forever,
            // because re-scanning that folder is refused as already covered.
            start_watching();
            // S2: the window manager's drag gesture can change the window's real shape without telling
            // anyone, so the stored layout is compared with it while the application runs.
            start_reconciling(app.handle().clone());
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
            set_document_naming,
            set_cover_page,
            use_cover_image,
            choose_cover_image,
            remove_cover_image,
            document_cover,
            suggestion_cover,
            suggestion_detector,
            observe_recent_documents,
            scan_folder,
            progress_providers,
            list_suggestions,
            accept_suggestion,
            discard_suggestion,
            watched_folders,
            folder_removal,
            remove_folder,
            get_language,
            set_language,
            get_window_layout,
            get_window_preset,
            set_window_layout,
            open_containing_folder,
            open_folder,
            open_document,
            pick_folder
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the mirk desktop application");
}

#[cfg(test)]
mod tests {
    use super::{
        authored, commands, layout, preset_from, remember_layout, LibraryEntryView, Store,
    };

    /// R42/S5, corrected after a native review: a caller that still sends the pre-list `author` must not
    /// lose its edit in silence.
    ///
    /// The defect the review found is a shape, not a typo: `authors` absent means *said nothing*, so a
    /// dropped legacy key turns a real edit into no edit and the save still returns success. These four
    /// cases are the whole mapping, and the first one is the rule that the newer spelling wins.
    #[test]
    fn the_naming_command_accepts_both_author_spellings_and_the_list_wins() {
        assert_eq!(
            authored(
                Some(vec!["Carl Sagan".into()]),
                Some(Some("Ignored".into()))
            ),
            Some(vec!["Carl Sagan".to_string()]),
            "the list is the order and it wins when both are sent"
        );
        assert_eq!(
            authored(None, Some(Some("Carl Sagan".into()))),
            Some(vec!["Carl Sagan".to_string()]),
            "the legacy single name becomes a one-entry list instead of being dropped"
        );
        assert_eq!(
            authored(None, Some(None)),
            Some(Vec::new()),
            "an explicit legacy null is *no author of my own*, not *said nothing*"
        );
        assert_eq!(
            authored(None, Some(Some("   ".into()))),
            Some(Vec::new()),
            "a blank legacy name is *no author of my own*, never an author called spaces"
        );
        assert_eq!(
            authored(None, None),
            None,
            "neither key means the action said nothing"
        );
    }

    /// The transient single-author projection must keep serialising `null`, not omit the key.
    ///
    /// It stands in for fields that always emitted an explicit `null`, and the native review was right
    /// that omitting them is an output change nobody asked for: a client cannot tell a missing property
    /// from one it should read as "no author".
    #[test]
    fn the_view_keeps_emitting_null_for_the_transient_single_author() {
        let document = mirk_core::Document::new("/library/no-author.pdf");
        let view = LibraryEntryView {
            id: 1,
            path: document.path.to_string_lossy().into_owned(),
            title: document.display_title(),
            user_title: None,
            authors: Vec::new(),
            author: None,
            user_author: None,
            reader_authors: Vec::new(),
            pages: None,
            progress: None,
            cover_page: None,
            cover_image: None,
            cover_source: None,
        };
        let json = serde_json::to_string(&view).expect("serialize the view");
        assert!(
            json.contains("\"author\":null"),
            "the transient field keeps its null: {json}"
        );
        assert!(
            json.contains("\"userAuthor\":null"),
            "the transient reader field keeps its null: {json}"
        );
    }

    /// The window's shape, asserted from the configuration the application actually loads.
    ///
    /// A tray icon and a window's decorations are visual and cannot be asserted here; what can be
    /// asserted is that the window is declared undecorated, still not resizable by hand and still
    /// without a maximise control, and that its launch size is the phone shape. What changed with
    /// S2/S6 is the ceiling: the four layouts widen the window to 430, 460 or 605 px, so only the
    /// minimum stays pinned and the desktop process is the one that resizes it. `fullscreen: false` is
    /// only the launch state — full screen is one of the choices.
    #[test]
    fn the_window_is_undecorated_and_sized_only_by_its_layouts() {
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
        assert!(
            !window.resizable,
            "A1: dragging an edge must not resize the window; the layouts are the only way"
        );
        assert!(
            !window.maximizable,
            "A2: the window must not be maximisable"
        );
        assert!(window.minimizable, "A2: the window must be minimisable");
        assert!(
            !window.fullscreen,
            "S2: the application launches in a window, not full screen"
        );

        // S2 of `desktop-chrome.md`: the window is created at the size the restore control returns to
        // — three per row, 634 px — and `setup` then fills the work area, because R7 made full screen
        // the stored default. Declaring the restore size rather than a number no layout names is what
        // keeps the frame before the maximise from being a size the reader can never get back to.
        assert_eq!(window.width, 634.0, "S2: the launch width is three per row");
        assert_eq!(window.height, 844.0, "R5: the window height");

        // The floor is still pinned — no layout shrinks the window below the phone shape — while the
        // ceiling is gone, because a preset has to be able to widen it.
        assert_eq!(window.min_width, Some(430.0));
        assert_eq!(window.min_height, Some(844.0));
        assert_eq!(
            window.max_width, None,
            "S2: a layout must be able to widen the window past 430"
        );
        assert_eq!(
            window.max_height, None,
            "S2: full screen needs the height the screen has"
        );

        // The phone proportion belongs to the two-per-row window, which is the narrowest one; the default
        // three-per-row window is deliberately wider than that shape (S2 of `desktop-chrome.md`).
        let (narrow_width, narrow_height) = layout::WindowLayout::Columns2
            .window_size()
            .expect("a windowed layout");
        let ratio = narrow_height / narrow_width;
        assert!(
            (1.9..=2.4).contains(&ratio),
            "the narrow window must keep phone proportions, but its height/width ratio is {ratio}"
        );

        // The declared launch size is the restore layout's own size, so the one frame the window has
        // before `setup` fills the work area is a size the restore control really returns to.
        assert_eq!(
            layout::RESTORE_LAYOUT.window_size(),
            Some((window.width, window.height)),
            "the launch size is the window restore returns to"
        );
    }

    // --- S2/S3: the preset the layout command remembers ----------------------------------------
    //
    // The command's other half sizes a real window, which no test can do without a display, so what is
    // asserted here is the writing it delegates to: `set_window_layout` calls `remember_layout` and then
    // `apply_window_layout`, and these are the store's side of that one call.

    fn in_memory_store() -> Store {
        Store::open_in_memory().expect("open an in-memory store")
    }

    #[test]
    fn a_windowed_layout_is_stored_and_remembered_as_the_preset() {
        let store = in_memory_store();
        remember_layout(&store, layout::WindowLayout::Columns4).expect("store four per row");

        assert_eq!(
            commands::get_window_layout(&store).expect("read the layout"),
            Some("columns-4".to_string()),
            "the layout itself is stored, as it always was"
        );
        assert_eq!(
            store
                .get_setting(layout::PRESET_SETTING)
                .expect("read the preset"),
            Some("columns-4".to_string()),
            "the windowed choice is what a window leaving full screen comes back to"
        );
    }

    #[test]
    fn full_screen_leaves_the_remembered_preset_alone() {
        let store = in_memory_store();
        remember_layout(&store, layout::WindowLayout::Columns2).expect("store two per row");
        remember_layout(&store, layout::WindowLayout::Auto).expect("store full screen");

        assert_eq!(
            commands::get_window_layout(&store).expect("read the layout"),
            Some("auto".to_string()),
            "full screen is the layout in force"
        );
        assert_eq!(
            store
                .get_setting(layout::PRESET_SETTING)
                .expect("read the preset"),
            Some("columns-2".to_string()),
            "full screen is not a preset, so the rows the reader chose survive it"
        );
        assert_eq!(
            layout::preset(
                store
                    .get_setting(layout::PRESET_SETTING)
                    .expect("read the preset")
                    .as_deref()
            ),
            layout::WindowLayout::Columns2,
            "and reading it back names the layout the reader chose"
        );
    }

    // S3/T1: the interface reads the preset through `get_window_preset`, whose whole answer is this
    // resolution and its key — `auto` is not one of the answers it can give.
    #[test]
    fn the_preset_read_path_answers_three_per_row_when_nothing_is_stored() {
        let store = in_memory_store();
        assert_eq!(
            preset_from(&store).expect("read the preset").key(),
            "columns-3",
            "a fresh store has no preset, so the build's fallback is the answer the interface gets"
        );
    }

    #[test]
    fn the_preset_read_path_answers_the_stored_key() {
        let store = in_memory_store();
        remember_layout(&store, layout::WindowLayout::Columns4).expect("store four per row");

        assert_eq!(
            preset_from(&store).expect("read the preset").key(),
            "columns-4",
            "the interface must set its preset from the rows the reader chose, not from three"
        );
    }

    #[test]
    fn a_store_with_no_preset_reads_as_three_per_row() {
        let store = in_memory_store();
        assert_eq!(
            store
                .get_setting(layout::PRESET_SETTING)
                .expect("read the preset"),
            None,
            "a fresh store has no preset, so the build's own fallback decides"
        );
        assert_eq!(layout::preset(None), layout::RESTORE_LAYOUT);
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
