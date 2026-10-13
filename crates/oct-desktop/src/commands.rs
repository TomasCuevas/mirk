//! The commands the screens call, with their logic in plain functions.
//!
//! The Tauri wrappers in `main.rs` are deliberately thin: they open the production database, call
//! one of the functions here, and render an [`oct_core::OctError`] into a string an IPC caller can
//! read. Everything that decides *what* the screen shows lives in this module, takes its
//! collaborators as arguments, and is therefore testable without starting a Tauri application —
//! the same injection pattern the scanner and the provider adapters already use.
//!
//! Each command exists because a pixel of a screen needs it: the library, a folder scan, the
//! progress-provider registry, the watched folders Configuración lists, and the card action that
//! opens a document's containing folder. Anything else is a later task.

use std::fs;
use std::path::{Path, PathBuf};

use oct_core::{FileIdentity, LaunchOutcome, Launcher, OctError, StoredProgress};
use oct_scan::{ScanReport, Scanner};
use oct_store::{LibraryEntry, Store, WatchedFolder};
use serde::Serialize;

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
pub fn scan_path(
    store: &Store,
    identity: &dyn FileIdentity,
    folder: &Path,
) -> Result<ScanReportView, OctError> {
    let scanner = Scanner::new(store, identity);
    Ok(ScanReportView::from(scanner.scan(folder)?))
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

/// Opens the folder that contains `path` with the platform launcher.
///
/// The document's own path is deliberately not opened: the reader asked for its folder. A folder
/// that is gone is an honest error rather than a silent no-op — the store can hold a document whose
/// directory was deleted or moved since the last scan, and "nothing happened" would be the least
/// useful report of that. The launcher's own failure (no `xdg-open`, say) travels back unchanged.
///
/// `launcher` is injected so this can be tested without starting a file manager, exactly as the
/// scanner takes its identity and the adapters take their spawner.
pub fn open_containing_folder(
    launcher: &dyn Launcher,
    path: &Path,
) -> Result<OpenedFolderView, OctError> {
    let folder = containing_folder(path)?;

    match fs::metadata(&folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(OctError::InvalidPath(format!(
                "the containing path is not a folder: {}",
                display_path(&folder)
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(OctError::InvalidPath(format!(
                "the folder no longer exists: {}",
                display_path(&folder)
            )))
        }
        Err(error) => return Err(OctError::Io(error)),
    }

    let viewer = match launcher.launch(&folder, None)? {
        LaunchOutcome::Delegated { viewer } => viewer,
        // `oct`'s own reader is a legitimate answer rather than a failure, once it exists.
        LaunchOutcome::OpenedInternally => "oct".to_string(),
    };

    Ok(OpenedFolderView {
        folder: display_path(&folder),
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
}
