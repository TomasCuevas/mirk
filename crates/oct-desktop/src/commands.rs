//! The three commands the first screen calls, with their logic in plain functions.
//!
//! The Tauri wrappers in `main.rs` are deliberately thin: they open the production database, call
//! one of the functions here, and render an [`oct_core::OctError`] into a string an IPC caller can
//! read. Everything that decides *what* the screen shows lives in this module, takes its
//! collaborators as arguments, and is therefore testable without starting a Tauri application —
//! the same injection pattern the scanner and the provider adapters already use.
//!
//! There are exactly three commands because there are exactly three things the screen asks for:
//! the library, a folder scan, and the progress-provider registry. Anything else is a later task.

use std::path::Path;

use oct_core::{FileIdentity, OctError, StoredProgress};
use oct_scan::{ScanReport, Scanner};
use oct_store::{LibraryEntry, Store};
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
    use oct_core::{Confidence, Document, FileId, ProgressSource, StoredProgress};
    use std::path::PathBuf;

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
}
