//! The commands the screens call, with their logic in plain functions.
//!
//! The Tauri wrappers in `main.rs` are deliberately thin: they open the production database, call
//! one of the functions here, and render an [`oct_core::OctError`] into a string an IPC caller can
//! read. Everything that decides *what* the screen shows lives in this module, takes its
//! collaborators as arguments, and is therefore testable without starting a Tauri application —
//! the same injection pattern the scanner and the provider adapters already use.
//!
//! Each command exists because a pixel of a screen needs it: the library, a folder scan, the
//! progress-provider registry, the watched folders Configuración lists and now removes (R20, R27),
//! the card action that opens a document's containing folder, and the watched-folder action that
//! opens the folder itself (R24). Anything else is a later task.

use std::fs;
use std::path::{Path, PathBuf};

use oct_core::{FileIdentity, LaunchOutcome, Launcher, OctError, StoredProgress};
use oct_scan::{cover_for, CoverOutcome, ScanReport, Scanner};
use oct_store::{covering_folder, FolderRemoval, LibraryEntry, Store, WatchedFolder};
use serde::Serialize;

use crate::shell;

/// A reading position serialised for the interface, provenance included.
///
/// `source` and `confidence` stay two fields, mirroring the two store columns: the screen renders
/// them separately and must never be handed a single flattened value. `recorded_at` is when the app
/// wrote the row; `read_at` is when a source observed the reading, and serialises as `readAt` to
/// match the interface's own spelling. They are deliberately two fields, because a row the app
/// wrote is not evidence of a reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressView {
    pub page: u32,
    pub total: Option<u32>,
    pub source: String,
    pub confidence: String,
    pub recorded_at: Option<i64>,
    pub read_at: Option<i64>,
}

impl ProgressView {
    fn new(progress: &StoredProgress, recorded_at: Option<i64>) -> Self {
        Self {
            page: progress.page,
            total: progress.total,
            source: progress.source.id().to_string(),
            confidence: confidence_id(progress.confidence),
            recorded_at,
            read_at: progress.read_at,
        }
    }
}

/// One library row as the list library command returns it.
///
/// `title` is already the **displayed** title (R31): the core resolves the reader's title over the
/// document's own, so no screen has to decide whose title it is rendering. `user_title` carries the
/// reader's title on its own — `null` when they have not set one — so the interface can say "this
/// one is yours" without guessing. It is deliberately not a `title_source` column: with the two facts
/// kept separate, no source is needed to tell them apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntryView {
    pub id: i64,
    pub path: String,
    /// The title to show, resolved by the core: the reader's when set, otherwise the document's own.
    pub title: String,
    /// The reader's own title, or `null` when they have not set one (`userTitle` over IPC).
    pub user_title: Option<String>,
    pub author: Option<String>,
    pub pages: Option<u32>,
    pub progress: Option<ProgressView>,
}

impl From<LibraryEntry> for LibraryEntryView {
    fn from(entry: LibraryEntry) -> Self {
        let LibraryEntry {
            document,
            progress,
            progress_recorded_at,
        } = entry;

        Self {
            id: document.id,
            path: display_path(&document.document.path),
            title: document.document.display_title(),
            user_title: document.document.user_title.clone(),
            author: document.document.author,
            pages: document.document.pages,
            progress: progress
                .as_ref()
                .map(|progress| ProgressView::new(progress, progress_recorded_at)),
        }
    }
}

/// A document the scan counted and persisted, as the scan command returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CountedView {
    pub path: String,
    pub pages: u32,
    pub title: Option<String>,
    pub author: Option<String>,
}

/// A path the scan could not turn into a counted document, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailureView {
    pub path: String,
    pub reason: String,
}

/// What the scan command returns. Mirrors [`oct_scan::ScanReport`] with paths as strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReportView {
    pub root: String,
    pub discovered: Vec<String>,
    pub counted: Vec<CountedView>,
    pub skipped: Vec<String>,
    pub failures: Vec<FailureView>,
}

impl From<ScanReport> for ScanReportView {
    fn from(report: ScanReport) -> Self {
        let ScanReport {
            root,
            discovered,
            counted,
            skipped,
            failures,
        } = report;

        Self {
            root: display_path(&root),
            discovered: discovered.iter().map(|path| display_path(path)).collect(),
            counted: counted
                .into_iter()
                .map(|document| CountedView {
                    path: display_path(&document.path),
                    pages: document.pages,
                    title: document.title,
                    author: document.author,
                })
                .collect(),
            skipped: skipped.iter().map(|path| display_path(path)).collect(),
            failures: failures
                .into_iter()
                .map(|failure| FailureView {
                    path: display_path(&failure.path),
                    reason: failure.reason,
                })
                .collect(),
        }
    }
}

/// A progress provider's identity, capability and availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderView {
    pub id: String,
    /// `"live"`, `"close-time"` or `"none"` — what the viewer can report and the UI can promise.
    pub capability: String,
    /// `"installed"`, `"missing"` or `"unsupported"`.
    pub availability: String,
}

/// A watched folder as Configuración lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolderView {
    pub id: i64,
    pub path: String,
    pub recursive: bool,
    pub enabled: bool,
}

impl From<WatchedFolder> for WatchedFolderView {
    fn from(folder: WatchedFolder) -> Self {
        Self {
            id: folder.id,
            path: display_path(&folder.path),
            recursive: folder.recursive,
            enabled: folder.enabled,
        }
    }
}

/// What removing one watch would take with it, as Configuración's confirmation reads it (R27).
///
/// The store type never crosses this boundary, exactly like every other view here: the screen sees
/// paths as strings and never a [`Store`] type, and `folder` is a [`WatchedFolderView`] built by
/// the same `display_path` the folder list uses, so the two cannot spell a path differently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderRemovalView {
    pub folder: WatchedFolderView,
    /// Paths of the documents that leave the library.
    pub documents: Vec<String>,
    /// Paths of the documents that stay because another remaining watch still covers them.
    pub kept: Vec<String>,
}

impl From<FolderRemoval> for FolderRemovalView {
    fn from(removal: FolderRemoval) -> Self {
        Self {
            folder: WatchedFolderView::from(removal.folder),
            documents: removal.documents,
            kept: removal.kept,
        }
    }
}

/// The folder a card action opened, and the program that opened it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenedFolderView {
    pub folder: String,
    pub viewer: String,
}

/// The library listing: every document with its latest progress and provenance.
pub fn library_entries(store: &Store) -> Result<Vec<LibraryEntryView>, OctError> {
    Ok(store
        .library()?
        .into_iter()
        .map(LibraryEntryView::from)
        .collect())
}

/// Sets or clears the reader's title for one document (R31).
///
/// `title` is a `String` on the IPC boundary, so the interface sends either the reader's text or
/// nothing at all: an absent value means "restore the document's own title" and is a legitimate
/// action, while a blank value is refused as the invalid argument it is. The refusal — the variant,
/// the field and the words — is decided in [`Store::set_user_title`], so this command and any future
/// caller cannot disagree about what a blank title means.
///
/// Nothing here touches the document's own title: the reader's title is a separate fact, and a
/// document named in this application can always be restored to the title the file itself carries.
pub fn set_document_title(store: &Store, id: i64, title: Option<&str>) -> Result<(), OctError> {
    store.set_user_title(id, title)
}

// --- R32: a document's cover, rendered on demand and cached in the application's data directory ---

/// The directory covers live in: `<data directory>/covers` (L20).
///
/// The data directory is passed in rather than read here, exactly like every other collaborator in
/// this module: `main.rs` derives it from `oct_store::database_path`, so the covers and the database
/// live in one place by construction instead of by two functions agreeing.
pub fn covers_directory(data_directory: &Path) -> PathBuf {
    data_directory.join("covers")
}

/// The cached cover file for the document with `id`.
///
/// Named by the document's row id, not by its path: a document that moves keeps its row (the store
/// follows identity), so a path-derived name would lose the cached cover on a move and leave the old
/// file behind. This half renders page one only; when the reader's choice arrives (V6) it changes
/// what is rendered into this same file.
fn cover_file(covers_directory: &Path, id: i64) -> PathBuf {
    covers_directory.join(format!("{id}.png"))
}

/// A document's cover as a data URL, or `None` when it cannot be shown (R32).
///
/// `None` is reserved for facts about the **document**: it has no readable PDF (encrypted, deleted,
/// unreadable), or the id names no row. Those keep the card's placeholder and the reason goes to
/// stderr, where this application reports everything it cannot do. A failure of the application's
/// own storage — the covers directory, the write, a missing pdfium — is returned as an error instead,
/// because reporting "this document has no cover" when the disk refused the write would be a lie
/// about the document. Either way nothing reaches a screen as an error: the card turns a rejected
/// request into its placeholder too.
pub fn document_cover(
    store: &Store,
    id: i64,
    data_directory: &Path,
) -> Result<Option<String>, OctError> {
    let Some(document) = document_path(store, id)? else {
        // An id that names nothing is "no cover", the same answer as a document with no readable
        // PDF. The interface only ever asks about documents it is showing, so this is a race at
        // worst, not a caller error worth a message on screen.
        return Ok(None);
    };

    let cover_path = cover_file(&covers_directory(data_directory), id);

    let pdfium = oct_scan::bind_pdfium_library().map_err(|error| {
        eprintln!(
            "the cover of {} could not be produced: {error}",
            display_path(&document)
        );
        error
    })?;

    let cached = match cover_for(&pdfium, &cover_path, &document) {
        Ok(CoverOutcome::Ready { path }) => path,
        Ok(CoverOutcome::Unavailable { reason }) => {
            // A fact about the document: no cover, no error, and the reason is written down.
            eprintln!("no cover for {}: {reason}", display_path(&document));
            return Ok(None);
        }
        Err(error) => {
            eprintln!(
                "the cover of {} could not be produced: {error}",
                display_path(&document)
            );
            return Err(error);
        }
    };

    match fs::read(&cached) {
        Ok(bytes) => Ok(Some(png_data_url(&bytes))),
        Err(error) => {
            eprintln!(
                "the cover of {} could not be read back: {error}",
                display_path(&document)
            );
            Err(error.into())
        }
    }
}

/// The path of the document with `id`, or `None` when the id names nothing.
///
/// The cover belongs to the document row, not to a path the interface hands back, so the path is
/// resolved from the store. The listing is the only lookup `oct-store` exposes today; it is ordered
/// by path, which is irrelevant to a `find` by id.
fn document_path(store: &Store, id: i64) -> Result<Option<PathBuf>, OctError> {
    Ok(store
        .library()?
        .into_iter()
        .find(|entry| entry.document.id == id)
        .map(|entry| entry.document.document.path))
}

/// `bytes` as a PNG `data:` URL for an `<img src>`.
///
/// A data URL is deliberate: Tauri's asset protocol would need a new configuration entry plus a
/// capability grant, and a 400 px cover is small, so the extra protocol surface buys nothing. The
/// base64 is written here rather than pulled in as a dependency — the standard alphabet is twenty
/// lines, and this workspace adds a crate only when it earns its place.
fn png_data_url(bytes: &[u8]) -> String {
    const PREFIX: &str = "data:image/png;base64,";
    let mut url = String::with_capacity(PREFIX.len() + bytes.len().div_ceil(3) * 4);
    url.push_str(PREFIX);
    base64_encode(bytes, &mut url);
    url
}

/// The RFC 4648 standard alphabet.
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Appends the Base64 encoding of `bytes` to `out`.
fn base64_encode(bytes: &[u8], out: &mut String) {
    for chunk in bytes.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = u32::from(chunk.get(1).copied().unwrap_or(0));
        let third = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (first << 16) | (second << 8) | third;

        out.push(BASE64_ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(BASE64_ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            BASE64_ALPHABET[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_ALPHABET[triple as usize & 0x3f] as char
        } else {
            '='
        });
    }
}

/// Scans `folder` into `store` and returns what happened.
///
/// `identity` is injected so the scan can be driven with a real or fake provider, exactly as
/// [`Scanner`] documents.
///
/// A folder that a watched folder already covers is refused here, before anything is walked or
/// recorded: the same tree under two watches discovers every document in it twice, and the subfolder
/// adds nothing the watch does not already reach. The refusal carries the covering folder's path,
/// because "it was blocked" with no "by which one" leaves the reader with nothing to do about it.
///
/// The candidate is resolved with the same `canonicalize` the scanner uses on the root it records,
/// so a folder the user selected through a symlinked ancestor is still recognised as one the store
/// has already recorded. Canonicalising first also means a folder that does not exist fails here
/// rather than after a pdfium bind, which is the same honest `Io` error arriving one step earlier.
pub fn scan_path(
    store: &Store,
    identity: &dyn FileIdentity,
    folder: &Path,
) -> Result<ScanReportView, OctError> {
    let root = folder.canonicalize()?;

    if let Some(covering) = covering_folder(&store.folders()?, &root) {
        return Err(OctError::PathCoveredByWatchedFolder {
            path: display_path(&root),
            covering: display_path(&covering.path),
        });
    }

    let scanner = Scanner::new(store, identity);
    Ok(ScanReportView::from(scanner.scan(&root)?))
}

/// The progress providers this build knows, with their declared capability and live availability.
pub fn provider_registry() -> Vec<ProviderView> {
    oct_platform::progress_providers()
        .iter()
        .map(|provider| ProviderView {
            id: provider.id().to_string(),
            capability: provider.capability().id().to_string(),
            availability: provider.detect().id().to_string(),
        })
        .collect()
}

/// Every folder the store has recorded, oldest first.
pub fn watched_folders(store: &Store) -> Result<Vec<WatchedFolderView>, OctError> {
    Ok(store
        .folders()?
        .into_iter()
        .map(WatchedFolderView::from)
        .collect())
}

/// Computes what removing the watch with `id` would take with it, changing nothing (R27).
///
/// The confirmation reads this first, so the count it states is the store's own answer rather
/// than a guess the screen makes. `None` means the id names no watch.
pub fn folder_removal(store: &Store, id: i64) -> Result<Option<FolderRemovalView>, OctError> {
    Ok(store.folder_removal(id)?.map(FolderRemovalView::from))
}

/// Stops watching the folder with `id`, taking the documents it covers and their positions with it.
///
/// The decision is [`Store::forget_folder`]'s, including the guard that keeps a document another
/// remaining watch still covers. The view it returns is the same plan [`folder_removal`] shows, so
/// what the confirmation said and what actually left cannot disagree. All this adds is the store
/// the production command opens.
pub fn remove_folder(store: &Store, id: i64) -> Result<Option<FolderRemovalView>, OctError> {
    Ok(store.forget_folder(id)?.map(FolderRemovalView::from))
}

/// The `settings` key the interface language is stored under. The interface writes it and the
/// desktop process reads it to word the tray (R1), so both halves use this one name.
const LANGUAGE_SETTING: &str = "language";

/// The stored interface language, but only when it is a locale this build knows.
///
/// The database accepts any string under `settings`; a value this build does not recognise is not a
/// language and must not reach a caller as one. It is reported as absent — the same answer as "never
/// chosen" — so the interface keeps its default instead of rendering keys with no words behind them.
pub fn get_language(store: &Store) -> Result<Option<String>, OctError> {
    Ok(store
        .get_setting(LANGUAGE_SETTING)?
        .filter(|language| shell::is_known_language(language)))
}

/// Stores the interface language, refusing a locale this build does not know.
///
/// Refusing rather than storing keeps the single source honest: a typo written once would otherwise
/// be read back as "no language stored" forever, and a newer build's locale would be silently
/// downgraded by an older one. The tray is rebuilt by the command wrapper in `main.rs`, not here, so
/// this decision stays testable without a display or a Tauri application.
pub fn set_language(store: &Store, locale: &str) -> Result<(), OctError> {
    if !shell::is_known_language(locale) {
        // `R35`: a bad argument answers as a bad argument. This used to be `Storage`, which was a small
        // lie in a log — the database was never involved, and anyone reading a failure later would look
        // in the wrong place. Not reachable from the interface today: the selector offers what
        // `shell::LANGUAGES` holds.
        return Err(OctError::InvalidInput {
            field: "locale",
            value: locale.to_string(),
            expected: shell::LANGUAGES.join(" or "),
        });
    }
    store.set_setting(LANGUAGE_SETTING, locale)
}

/// Opens the folder that contains `path` with the platform launcher.
///
/// The document's own path is deliberately not opened: the reader asked for its folder. The launcher
/// is injected so this can be tested without starting a file manager, exactly as the scanner takes
/// its identity and the adapters take their spawner.
pub fn open_containing_folder(
    launcher: &dyn Launcher,
    path: &Path,
) -> Result<OpenedFolderView, OctError> {
    let folder = containing_folder(path)?;
    open_directory(launcher, &folder)
}

/// Opens `path`, which must itself be the folder the caller wants on screen (R24).
///
/// This is the watched-folder action, and it opens the folder **itself** rather than its parent:
/// [`open_containing_folder`] answers "where does this document live", and this answers "show me
/// this folder". Both go through [`open_directory`], so there is one place that decides what an
/// openable folder is and one place that reports a failure to open it.
pub fn open_folder(launcher: &dyn Launcher, path: &Path) -> Result<OpenedFolderView, OctError> {
    open_directory(launcher, path)
}

/// Opens `folder` with `launcher`, refusing anything that is not a folder on disk right now.
///
/// A folder that is gone is an honest error rather than a silent no-op: the store can hold a
/// document — or a watch — whose directory was deleted or moved since it was last seen, and
/// "nothing happened" would be the least useful report of that. The launcher's own failure (no
/// `xdg-open`, say) travels back unchanged.
fn open_directory(launcher: &dyn Launcher, folder: &Path) -> Result<OpenedFolderView, OctError> {
    match fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(OctError::InvalidPath(format!(
                "the path is not a folder: {}",
                display_path(folder)
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(OctError::InvalidPath(format!(
                "the folder no longer exists: {}",
                display_path(folder)
            )))
        }
        Err(error) => return Err(OctError::Io(error)),
    }

    let viewer = match launcher.launch(folder, None)? {
        LaunchOutcome::Delegated { viewer } => viewer,
        // `oct`'s own reader is a legitimate answer rather than a failure, once it exists.
        LaunchOutcome::OpenedInternally => "oct".to_string(),
    };

    Ok(OpenedFolderView {
        folder: display_path(folder),
        viewer,
    })
}

/// The directory a document lives in.
///
/// `Path::parent` returns `None` only for a path with no parent (a bare `/`), and an empty parent
/// for a bare file name; neither is a path a stored document can meaningfully have.
fn containing_folder(path: &Path) -> Result<PathBuf, OctError> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Ok(parent.to_path_buf()),
        _ => Err(OctError::InvalidPath(format!(
            "the document path has no folder: {}",
            display_path(path)
        ))),
    }
}

/// The stable string a [`oct_core::Confidence`] is persisted under.
fn confidence_id(confidence: oct_core::Confidence) -> String {
    match confidence {
        oct_core::Confidence::Exact => "exact",
        oct_core::Confidence::CloseTime => "close-time",
        oct_core::Confidence::Manual => "manual",
        oct_core::Confidence::Unknown => "unknown",
    }
    .to_string()
}

/// A path rendered for display. The store only ever holds valid UTF-8 paths, and a scan reports a
/// genuinely undecodable name as a failure rather than a document, so this is lossless in practice.
fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oct_core::{Confidence, Document, FileId, LaunchOutcome, Launcher, Platform, ProgressSource, StoredProgress};
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn in_memory_store() -> Store {
        Store::open_in_memory().expect("open an in-memory store")
    }

    fn document(path: &str, dev: u64, ino: u64) -> Document {
        let mut doc = Document::new(path);
        doc.identity = Some(FileId::Posix { dev, ino });
        doc
    }

    #[test]
    fn the_library_view_carries_the_latest_progress_and_its_provenance() {
        let store = in_memory_store();
        let read_id = store
            .upsert_document(&document("/library/read.pdf", 1, 1))
            .expect("insert");
        let unread_id = store
            .upsert_document(&document("/library/unread.pdf", 1, 2))
            .expect("insert");

        // Two rows; the later one must win and the earlier one must not leak into the view.
        store
            .record_progress(
                read_id,
                &StoredProgress::new(
                    3,
                    Some(80),
                    ProgressSource::External("evince-gvfs"),
                    Confidence::Exact,
                ),
            )
            .expect("record");
        store
            .record_progress(
                read_id,
                &StoredProgress::new(
                    11,
                    Some(80),
                    ProgressSource::External("evince-gvfs"),
                    Confidence::Exact,
                ),
            )
            .expect("record");

        let entries = library_entries(&store).expect("list the library");
        assert_eq!(entries.len(), 2);

        let read = entries
            .iter()
            .find(|entry| entry.id == read_id)
            .expect("the read document");
        let progress = read.progress.as_ref().expect("progress present");
        assert_eq!(progress.page, 11);
        assert_eq!(progress.total, Some(80));
        assert_eq!(progress.source, "evince-gvfs");
        assert_eq!(progress.confidence, "exact");
        assert!(progress.recorded_at.is_some());

        let unread = entries
            .iter()
            .find(|entry| entry.id == unread_id)
            .expect("the unread document");
        assert_eq!(unread.progress, None, "no progress must stay absent, not become zero");
    }

    #[test]
    fn the_library_view_carries_the_reading_time_apart_from_the_write_time() {
        let store = in_memory_store();
        let doc_id = store
            .upsert_document(&document("/library/read.pdf", 1, 3))
            .expect("insert");

        // A reading time a year old, written now. Both facts must reach the view, and the write
        // time must not be allowed to stand in for the reading time.
        let old = 1_600_000_000;
        store
            .record_progress(
                doc_id,
                &StoredProgress::new(
                    4,
                    Some(80),
                    ProgressSource::External("evince-gvfs"),
                    Confidence::Exact,
                )
                .with_read_at(Some(old)),
            )
            .expect("record");

        let entries = library_entries(&store).expect("list the library");
        let progress = entries[0].progress.as_ref().expect("progress present");
        // `ProgressView` derives `Serialize` with `rename_all = "camelCase"`, so `read_at` reaches
        // the interface as `readAt`, exactly as `recorded_at` reaches it as `recordedAt`.
        assert_eq!(progress.read_at, Some(old), "the reading time must reach the view");
        assert!(
            progress.recorded_at.is_some(),
            "the row still records when the app wrote it"
        );
        assert_ne!(
            progress.recorded_at,
            Some(old),
            "the write time must not be mistaken for the reading time"
        );
    }

    // --- R31: the reader's title travels beside the document's own -------------------------------

    #[test]
    fn the_library_view_displays_the_readers_title_and_marks_it_as_theirs() {
        let store = in_memory_store();
        let mut doc = document("/library/name.pdf", 5, 5);
        doc.title = Some("The document's own title".to_string());
        let id = store.upsert_document(&doc).expect("insert");

        // Before anyone names it, the view shows the document's own title and no reader's title.
        let entries = library_entries(&store).expect("list");
        assert_eq!(entries[0].title, "The document's own title");
        assert_eq!(entries[0].user_title, None);

        set_document_title(&store, id, Some("What the reader called it")).expect("name it");

        let entries = library_entries(&store).expect("list again");
        // `title` is the displayed value, and `userTitle` marks it as the reader's.
        assert_eq!(entries[0].title, "What the reader called it");
        assert_eq!(
            entries[0].user_title.as_deref(),
            Some("What the reader called it")
        );
    }

    #[test]
    fn a_document_with_no_title_at_all_reaches_the_view_as_the_files_own_name() {
        // The rule L18 shipped, now resolved in the core instead of the interface: a card shows the
        // file's name rather than its path, and the interface never has to decide that.
        let store = in_memory_store();
        store
            .upsert_document(&document("/library/Deep Work.pdf", 5, 6))
            .expect("insert");

        let entries = library_entries(&store).expect("list");
        assert_eq!(entries[0].title, "Deep Work");
        assert!(!entries[0].title.contains('/'), "a path is not a title");
        assert_eq!(entries[0].user_title, None);
    }

    #[test]
    fn restoring_the_documents_own_title_clears_the_readers() {
        let store = in_memory_store();
        let mut doc = document("/library/name.pdf", 5, 7);
        doc.title = Some("The document's own title".to_string());
        let id = store.upsert_document(&doc).expect("insert");

        set_document_title(&store, id, Some("The reader's title")).expect("name it");
        assert_eq!(
            library_entries(&store).expect("list")[0].title,
            "The reader's title"
        );

        // `None` is the legitimate "restore" action, not an error.
        set_document_title(&store, id, None).expect("restore the document's own title");
        let entries = library_entries(&store).expect("list again");
        assert_eq!(entries[0].title, "The document's own title");
        assert_eq!(entries[0].user_title, None);
    }

    #[test]
    fn a_blank_title_is_refused_by_the_command_as_an_invalid_argument() {
        let store = in_memory_store();
        let mut doc = document("/library/name.pdf", 5, 8);
        doc.title = Some("The document's own title".to_string());
        let id = store.upsert_document(&doc).expect("insert");

        match set_document_title(&store, id, Some("   ")) {
            Err(OctError::InvalidInput {
                field,
                value,
                expected,
            }) => {
                assert_eq!(field, "title");
                assert_eq!(value, "   ");
                assert!(expected.contains("non-empty"), "expected: {expected}");
            }
            other => panic!("a blank title must be an invalid argument, got {other:?}"),
        }

        // Nothing was written, so the document still displays its own title.
        let entries = library_entries(&store).expect("list");
        assert_eq!(entries[0].title, "The document's own title");
        assert_eq!(entries[0].user_title, None);
    }

    #[test]
    fn scanning_a_real_folder_populates_the_library_view() {
        let store = in_memory_store();
        let identity = oct_platform::backends().file_identity;
        // The scanner's own committed fixtures: two real PDFs, read-only, never the maintainer's
        // library. Using them here exercises the same code path the command wraps.
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/oct-scan/tests/fixtures");

        let report = scan_path(&store, identity.as_ref(), &fixtures).expect("scan the fixtures");
        assert_eq!(report.discovered.len(), 2, "two fixtures: {report:?}");
        assert_eq!(
            report.counted.len(),
            report.discovered.len(),
            "both fixtures are countable: {report:?}"
        );
        assert!(report.failures.is_empty(), "no fixture fails: {report:?}");

        let entries = library_entries(&store).expect("list the library");
        assert_eq!(entries.len(), report.counted.len());
        assert!(
            entries.iter().all(|entry| entry.pages.is_some()),
            "a counted document must reach the library with its page count: {entries:?}"
        );
    }

    #[test]
    fn scanning_a_missing_folder_is_an_error_not_an_empty_success() {
        let store = in_memory_store();
        let identity = oct_platform::backends().file_identity;
        let missing = std::env::temp_dir().join("oct-desktop-folder-that-is-not-there-9f3c1a");

        let result = scan_path(&store, identity.as_ref(), &missing);
        assert!(
            result.is_err(),
            "a folder that does not exist must surface as an error, got {result:?}"
        );
    }

    #[test]
    fn the_provider_registry_reports_capability_and_availability() {
        let providers = provider_registry();
        for provider in &providers {
            assert!(!provider.id.is_empty(), "every provider has an id");
            assert!(
                ["live", "close-time", "none"].contains(&provider.capability.as_str()),
                "unexpected capability `{}`",
                provider.capability
            );
            assert!(
                ["installed", "missing", "unsupported"].contains(&provider.availability.as_str()),
                "unexpected availability `{}`",
                provider.availability
            );
        }
        #[cfg(target_os = "linux")]
        assert_eq!(
            providers.len(),
            1,
            "the Linux build registers exactly the Evince adapter"
        );
    }

    // --- The UI shell's two new commands -----------------------------------------------------

    /// A launcher that records what it was asked to open instead of opening anything.
    struct RecordingLauncher {
        calls: Mutex<Vec<(PathBuf, Option<u32>)>>,
    }

    impl RecordingLauncher {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl Launcher for RecordingLauncher {
        fn id(&self) -> &'static str {
            "test-recorder"
        }

        fn platform(&self) -> Platform {
            Platform::current()
        }

        fn launch(&self, path: &Path, page: Option<u32>) -> Result<LaunchOutcome, OctError> {
            self.calls
                .lock()
                .expect("the recording lock")
                .push((path.to_path_buf(), page));
            Ok(LaunchOutcome::Delegated {
                viewer: "test-file-manager".to_string(),
            })
        }
    }

    /// A unique temporary directory that removes itself on drop, including on panic.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "oct-desktop-{label}-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create the temporary directory");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn watched_folders_lists_what_the_store_recorded() {
        let store = in_memory_store();
        assert!(
            watched_folders(&store).expect("list folders").is_empty(),
            "a fresh store has recorded nothing"
        );

        store.record_folder("/library", true).expect("record");
        store.record_folder("/library/short", false).expect("record");

        let folders = watched_folders(&store).expect("list folders");
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].path, "/library");
        assert!(folders[0].recursive);
        assert!(folders[0].enabled);
        assert_eq!(folders[1].path, "/library/short");
        assert!(!folders[1].recursive);
    }

    // --- R27: removal computes its plan first, then carries it out ------------------------------

    #[test]
    fn folder_removal_plans_before_removing_and_names_the_documents() {
        let store = in_memory_store();
        let doomed = store.record_folder("/lib/doomed", true).expect("record");
        let keep = store.record_folder("/lib/keep", true).expect("record");
        store
            .upsert_document(&document("/lib/doomed/gone.pdf", 8, 1))
            .expect("insert");
        store
            .upsert_document(&document("/lib/keep/stays.pdf", 8, 2))
            .expect("insert");

        let planned = folder_removal(&store, doomed)
            .expect("plan")
            .expect("the watch is known");
        assert_eq!(planned.folder.id, doomed);
        assert_eq!(planned.documents, vec!["/lib/doomed/gone.pdf".to_string()]);
        assert!(planned.kept.is_empty());
        // The plan changed nothing, so the document is still there to be removed.
        assert_eq!(store.document_count().expect("count"), 2);

        let removed = remove_folder(&store, doomed)
            .expect("remove")
            .expect("the watch was known");
        assert_eq!(removed, planned, "the removal carries out the plan it showed");
        assert_eq!(store.document_count().expect("count"), 1);
        let folders = watched_folders(&store).expect("list folders");
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].id, keep);
    }

    #[test]
    fn removing_an_unknown_folder_is_none_and_deletes_nothing() {
        let store = in_memory_store();
        let watched = store.record_folder("/lib/doomed", true).expect("record");
        store
            .upsert_document(&document("/lib/doomed/gone.pdf", 9, 1))
            .expect("insert");

        assert!(
            folder_removal(&store, 404).expect("plan").is_none(),
            "an unknown id plans nothing"
        );
        assert!(
            remove_folder(&store, 404).expect("remove").is_none(),
            "an unknown id removes nothing"
        );
        assert_eq!(store.folders().expect("list folders").len(), 1);
        assert_eq!(store.document_count().expect("count"), 1);
        // The watch that does exist is untouched and still removable.
        assert!(folder_removal(&store, watched).expect("plan").is_some());
    }

    #[test]
    fn opening_the_containing_folder_asks_the_launcher_for_the_folder() {
        let dir = TempDir::new("open-folder");
        let folder = dir.path.join("libros");
        let document = folder.join("Cosmos.pdf");
        std::fs::create_dir_all(&folder).expect("create the document's folder");
        std::fs::write(&document, b"not really a pdf").expect("write the file");

        let launcher = RecordingLauncher::new();
        let opened = open_containing_folder(&launcher, &document).expect("open the folder");

        assert_eq!(opened.folder, display_path(&folder));
        assert_eq!(opened.viewer, "test-file-manager");

        let calls = launcher.calls.lock().expect("the recording lock");
        assert_eq!(calls.len(), 1, "exactly one launch");
        assert_eq!(calls[0].0, folder, "the folder, not the document");
        assert_eq!(calls[0].1, None, "opening a folder is not a page request");
    }

    #[test]
    fn a_folder_that_no_longer_exists_is_an_honest_error() {
        let dir = TempDir::new("gone-folder");
        let folder = dir.path.join("libros");
        let document = folder.join("Cosmos.pdf");
        std::fs::create_dir_all(&folder).expect("create the document's folder");
        std::fs::write(&document, b"not really a pdf").expect("write the file");
        // The library keeps a document whose folder was deleted after the last scan.
        std::fs::remove_dir_all(&folder).expect("remove the folder");

        let launcher = RecordingLauncher::new();
        let result = open_containing_folder(&launcher, &document);

        match result {
            Err(OctError::InvalidPath(message)) => {
                assert!(message.contains("libros"), "names the folder: {message}");
            }
            other => panic!("expected an InvalidPath error, got {other:?}"),
        }
        assert!(
            launcher.calls.lock().expect("the recording lock").is_empty(),
            "a folder that is gone must not be launched"
        );
    }

    #[test]
    fn a_path_with_no_folder_is_an_honest_error() {
        let launcher = RecordingLauncher::new();
        let result = open_containing_folder(&launcher, Path::new("/"));

        assert!(
            matches!(result, Err(OctError::InvalidPath(_))),
            "got {result:?}"
        );
        assert!(launcher.calls.lock().expect("the recording lock").is_empty());
    }

    #[test]
    fn a_launcher_failure_travels_back_unchanged() {
        struct FailingLauncher;

        impl Launcher for FailingLauncher {
            fn id(&self) -> &'static str {
                "test-failing"
            }

            fn platform(&self) -> Platform {
                Platform::current()
            }

            fn launch(&self, _path: &Path, _page: Option<u32>) -> Result<LaunchOutcome, OctError> {
                Err(OctError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no xdg-open",
                )))
            }
        }

        let dir = TempDir::new("launcher-failure");
        let file = dir.path.join("Cosmos.pdf");
        std::fs::write(&file, b"not really a pdf").expect("write the file");

        let result = open_containing_folder(&FailingLauncher, &file);
        match result {
            Err(OctError::Io(error)) => assert!(error.to_string().contains("no xdg-open")),
            other => panic!("expected an io error, got {other:?}"),
        }
    }

    // --- R22: a subfolder of a watched folder is refused, and the refusal says why ------------

    #[test]
    fn scanning_a_subfolder_of_a_watched_folder_is_refused_by_name() {
        let store = in_memory_store();
        let identity = oct_platform::backends().file_identity;
        let dir = TempDir::new("nested-scan");
        let library = dir.path.join("library");
        let subfolder = library.join("pdfs");
        std::fs::create_dir_all(&subfolder).expect("create the folders");
        // The store holds the canonical root, because that is what the scanner records (T6), and
        // the chosen path is resolved the same way so the two spellings can be compared at all.
        let watched_root = library.canonicalize().expect("canonical root");
        store.record_folder(&watched_root, true).expect("record");

        let result = scan_path(&store, identity.as_ref(), &subfolder);

        match result {
            Err(OctError::PathCoveredByWatchedFolder { path, covering }) => {
                assert!(path.ends_with("pdfs"), "names the refused folder: {path}");
                assert!(
                    covering.ends_with("library"),
                    "names the folder that already covers it: {covering}"
                );
            }
            other => panic!("expected the nesting refusal, got {other:?}"),
        }
        assert_eq!(
            store.folders().expect("list folders").len(),
            1,
            "a refused folder must not be recorded"
        );
    }

    #[test]
    fn scanning_a_folder_a_watch_already_covers_is_refused_for_the_folder_itself_too() {
        let store = in_memory_store();
        let identity = oct_platform::backends().file_identity;
        let dir = TempDir::new("same-scan");
        let library = dir.path.join("library");
        std::fs::create_dir_all(&library).expect("create the folder");
        let watched_root = library.canonicalize().expect("canonical root");
        store.record_folder(&watched_root, true).expect("record");

        let result = scan_path(&store, identity.as_ref(), &library);
        assert!(
            matches!(result, Err(OctError::PathCoveredByWatchedFolder { .. })),
            "re-adding the watched folder is the same refusal, not a second watch: {result:?}"
        );
        assert_eq!(store.folders().expect("list folders").len(), 1);
    }

    #[test]
    fn the_refusal_reads_as_a_sentence_naming_the_covering_folder() {
        // The interface renders `OctError`'s own message, so the message — not only the two fields —
        // has to carry the answer: "it was blocked" without "by which one" is a half-answer.
        let error = OctError::PathCoveredByWatchedFolder {
            path: "/library/pdfs/book".to_string(),
            covering: "/library/pdfs".to_string(),
        };
        let message = error.to_string();

        assert!(message.contains("/library/pdfs/book"), "names the choice: {message}");
        assert!(
            message.contains("/library/pdfs"),
            "names the covering folder: {message}"
        );
        assert!(
            message.contains("already covered") && message.contains("not added"),
            "states what happened and why: {message}"
        );
    }

    // --- R24: a watched folder opens itself, not its parent ------------------------------------

    #[test]
    fn opening_a_watched_folder_asks_the_launcher_for_that_folder_itself() {
        let dir = TempDir::new("open-watched");
        let folder = dir.path.join("libros");
        std::fs::create_dir_all(&folder).expect("create the folder");
        std::fs::write(folder.join("Cosmos.pdf"), b"not really a pdf").expect("write a file");

        let launcher = RecordingLauncher::new();
        let opened = open_folder(&launcher, &folder).expect("open the watched folder");

        assert_eq!(opened.folder, display_path(&folder));
        assert_eq!(opened.viewer, "test-file-manager");

        let calls = launcher.calls.lock().expect("the recording lock");
        assert_eq!(calls.len(), 1, "exactly one launch");
        assert_eq!(calls[0].0, folder, "the folder itself, not its parent");
        assert_eq!(calls[0].1, None, "opening a folder is not a page request");
    }

    #[test]
    fn opening_a_watched_folder_that_is_gone_is_an_honest_error() {
        let dir = TempDir::new("gone-watched");
        let folder = dir.path.join("libros");
        std::fs::create_dir_all(&folder).expect("create the folder");
        // The watch outlives the folder it names: nothing re-checks the disk until someone asks.
        std::fs::remove_dir_all(&folder).expect("remove the folder");

        let launcher = RecordingLauncher::new();
        let result = open_folder(&launcher, &folder);

        match result {
            Err(OctError::InvalidPath(message)) => {
                assert!(message.contains("libros"), "names the folder: {message}");
            }
            other => panic!("expected an InvalidPath error, got {other:?}"),
        }
        assert!(
            launcher.calls.lock().expect("the recording lock").is_empty(),
            "a folder that is gone must not be launched"
        );
    }

    // --- R1: the language the interface and the tray share ------------------------------------

    #[test]
    fn the_language_is_stored_and_read_back() {
        let store = in_memory_store();
        assert_eq!(get_language(&store).expect("read"), None);

        set_language(&store, "es").expect("store Spanish");
        assert_eq!(get_language(&store).expect("read"), Some("es".to_string()));

        set_language(&store, "en").expect("store English");
        assert_eq!(get_language(&store).expect("read"), Some("en".to_string()));
    }

    #[test]
    fn an_unknown_locale_is_rejected_and_not_stored() {
        let store = in_memory_store();
        let result = set_language(&store, "fr");
        // `R35`. The variant matters, and so do its three fields: the whole reason this is not `Storage`
        // is that whoever reads the failure must be able to see WHICH value arrived, in which field, and
        // what the build accepts — so the test asserts all three rather than only the variant's name.
        match result.expect_err("an unknown locale must be refused") {
            OctError::InvalidInput {
                field,
                value,
                expected,
            } => {
                assert_eq!(field, "locale");
                assert_eq!(value, "fr");
                assert_eq!(expected, "en or es");
            }
            other => panic!("a bad locale must be an invalid argument, not {other:?}"),
        }
        assert_eq!(
            store.get_setting("language").expect("read the raw setting"),
            None,
            "a refused locale must never reach the store"
        );
    }

    #[test]
    fn a_stored_value_that_is_not_a_language_never_reaches_a_caller() {
        let store = in_memory_store();
        // A newer build, a hand-edited database or a typo can leave anything under this key. A
        // caller must see "no language" rather than a value it cannot word the tray with.
        store
            .set_setting("language", "fr")
            .expect("write a value the build does not know");
        assert_eq!(
            get_language(&store).expect("read"),
            None,
            "an unknown stored value is reported as no language, not as one"
        );
    }

    // --- R32: the cover's data URL and its file name ------------------------------------------

    #[test]
    fn the_cover_is_a_png_data_url_in_the_rfc_4648_encoding() {
        // RFC 4648's own vectors, so the encoder is pinned against the standard rather than against
        // itself. The three padding lengths are the cases that can go wrong.
        let encoded = |input: &[u8]| {
            let mut out = String::new();
            base64_encode(input, &mut out);
            out
        };
        assert_eq!(encoded(b""), "");
        assert_eq!(encoded(b"f"), "Zg==");
        assert_eq!(encoded(b"fo"), "Zm8=");
        assert_eq!(encoded(b"foo"), "Zm9v");
        assert_eq!(encoded(b"foob"), "Zm9vYg==");
        assert_eq!(encoded(b"fooba"), "Zm9vYmE=");
        assert_eq!(encoded(b"foobar"), "Zm9vYmFy");

        // The full URL: the prefix a browser needs, then the PNG signature's own encoding, so the
        // bytes the interface receives are an image and not text pretending to be one.
        let url = png_data_url(b"\x89PNG\r\n\x1a\n");
        assert!(url.starts_with("data:image/png;base64,"), "{url}");
        assert_eq!(&url["data:image/png;base64,".len()..], "iVBORw0KGgo=");
    }

    #[test]
    fn the_cover_file_is_named_by_the_document_id_inside_the_covers_directory() {
        assert_eq!(
            covers_directory(Path::new("/data/oct")),
            PathBuf::from("/data/oct/covers")
        );
        assert_eq!(
            cover_file(Path::new("/data/oct/covers"), 42),
            PathBuf::from("/data/oct/covers/42.png")
        );
    }

    #[test]
    fn an_id_that_names_no_document_has_no_cover_and_no_error() {
        let store = in_memory_store();
        assert_eq!(
            document_cover(&store, 999, Path::new("/data/oct")).expect("no document, no cover"),
            None
        );
    }
}
