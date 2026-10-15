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
use oct_scan::{ScanReport, Scanner};
use oct_store::{covering_folder, FolderRemoval, LibraryEntry, Store, WatchedFolder};
use serde::Serialize;

use crate::shell;

/// A reading position serialised for the interface, provenance included.
///
/// `source` and `confidence` stay two fields, mirroring the two store columns: the screen renders
/// them separately and must never be handed a single flattened value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressView {
    pub page: u32,
    pub total: Option<u32>,
    pub source: String,
    pub confidence: String,
    pub recorded_at: Option<i64>,
}

impl ProgressView {
    fn new(progress: &StoredProgress, recorded_at: Option<i64>) -> Self {
        Self {
            page: progress.page,
            total: progress.total,
            source: progress.source.id().to_string(),
            confidence: confidence_id(progress.confidence),
            recorded_at,
        }
    }
}

/// One library row as the list library command returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntryView {
    pub id: i64,
    pub path: String,
    pub title: Option<String>,
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
            title: document.document.title,
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
        return Err(OctError::Storage(format!(
            "`{locale}` is not an interface language this build knows"
        )));
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
        assert!(
            matches!(result, Err(OctError::Storage(_))),
            "an unknown locale must be refused, got {result:?}"
        );
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
}
