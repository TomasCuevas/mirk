//! The ingestion loop: the connection between the open-document detector, the progress providers
//! and the store.
//!
//! Before this module the three parts existed and were correct, and the application was the gap
//! between them: nothing called the detector, nothing asked a provider for a position, and
//! [`oct_store::Store::record_progress`] had no caller outside a test. Reading a document in Evince
//! therefore produced no record at all (`L35`).
//!
//! Every decision lives here as a plain function taking its collaborators as arguments — the
//! detector, the provider registry and the store are all passed in — so the rules below are pinned
//! by tests that need neither a display nor a running Tauri application. `main.rs` owns only the
//! tick and the launch sweep that call them.

use std::path::Path;
use std::time::Duration;

use oct_core::{Availability, OctError, OpenDocumentDetector, ProgressProvider, StoredProgress};
use oct_store::{LibraryEntry, Store};

/// How often the tick asks the detector what is open and the providers what those documents say.
///
/// A reading position changes when a page turns, not continuously, and T1 measured Evince landing
/// its GVFS write within roughly three seconds of a page change, with the document still open.
/// Three seconds is therefore short enough for a position to appear immediate and long enough that
/// the cost is nothing: a tick reads only the handful of documents that are actually open
/// (~10 ms/file, T3), while the library-wide sweep runs once at launch instead of on this interval.
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// A capability this build or machine could not provide, named rather than shown as an empty answer.
///
/// [`oct_core::OctError::NotSupported`] means *"this cannot be answered here"*, which is a different
/// fact from *"I looked and there is no position"* (`Ok(None)`). Collapsing the first into the
/// second is exactly the lie `oct-platform`'s honest stubs exist to prevent, so a missing capability
/// travels out of the loop as this value and is reported instead of being swallowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingCapability {
    /// Who could not answer: a provider id, or the detector's id.
    pub source: String,
    /// The human-readable reason from the error itself.
    pub reason: String,
}

/// A reading position this pass actually wrote, with the document it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedPosition {
    /// The `documents.id` the progress row was inserted against.
    pub doc_id: i64,
    /// The position exactly as the provider reported it.
    pub progress: StoredProgress,
}

/// What one ingestion pass did, for the caller to log.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct TrackReport {
    /// Positions written this pass. Empty when every answer agreed with what was already stored.
    pub recorded: Vec<RecordedPosition>,
    /// Capabilities that were missing, kept apart from an empty answer on purpose.
    pub missing_capabilities: Vec<MissingCapability>,
    /// Other failures. They are reported and survived; a failing provider never ends the loop.
    pub failures: Vec<String>,
}

/// One tick: records what the providers say about the documents the detector reports as open.
///
/// This is the thin wrapper `S1` asks for — the detector is asked once, the library is read once,
/// and the per-document decision is made by the functions below. It never panics and never returns
/// an error; everything that went wrong is in the returned [`TrackReport`] so the loop can go on.
pub fn track_open_documents(
    store: &Store,
    detector: &dyn OpenDocumentDetector,
    providers: &[Box<dyn ProgressProvider>],
) -> TrackReport {
    let mut report = TrackReport::default();

    let entries = match store.library() {
        Ok(entries) => entries,
        Err(error) => {
            report
                .failures
                .push(format!("the library could not be read: {error}"));
            return report;
        }
    };

    let open = match detector.open_documents() {
        Ok(open) => open,
        // A detector that cannot answer on this platform must say so rather than look like a
        // machine with nothing open. Reporting tears down nothing: the next tick asks again.
        Err(error) => {
            note_error(&mut report, detector.id(), error);
            return report;
        }
    };

    for document in &open {
        // The watched folder is the source of truth. A document the store does not know is ignored
        // rather than added: discovering PDFs outside the watched folders belongs to the suggestion
        // queue in a later unit, and a library that grows from whatever happens to be open would be
        // exactly the lie this product refuses.
        let Some(entry) = entry_for_path(&entries, &document.path) else {
            continue;
        };
        // Only the provider that claims the viewer holding the document is asked. A document no
        // provider claims is silence, not a missing capability.
        let Some(provider) = provider_for_viewer(providers, document.viewer.as_deref()) else {
            continue;
        };
        read_and_record(store, entry, provider, &mut report);
    }

    report
}

/// The launch sweep (`S5`): every known document, not only the open ones.
///
/// Progress written while the application was closed is recorded here instead of being lost. Like
/// the tick it reports failures in its return value and never propagates them. The provider that
/// answers is the first installed one in registry order that has something to say, because there is
/// no open viewer to attribute a document to and [`ProgressProvider::read`] is keyed by path.
pub fn sweep_library(store: &Store, providers: &[Box<dyn ProgressProvider>]) -> TrackReport {
    let mut report = TrackReport::default();

    let entries = match store.library() {
        Ok(entries) => entries,
        Err(error) => {
            report
                .failures
                .push(format!("the library could not be read: {error}"));
            return report;
        }
    };

    for entry in &entries {
        for provider in providers {
            // A provider that is not installed here cannot answer this machine's question, so it is
            // not asked; `detect` is what distinguishes "absent" from "nothing to report".
            if provider.detect() != Availability::Installed {
                continue;
            }
            match provider.read(&entry.document.document) {
                Ok(None) => continue,
                Ok(Some(candidate)) => {
                    record_if_changed(store, entry, candidate, &mut report);
                    // One source of truth per pass: a second provider agreeing on the same page is
                    // not a second fact, and the registry order decides which one is heard.
                    break;
                }
                Err(error) => note_error(&mut report, provider.id(), error),
            }
        }
    }

    report
}

/// The stored entry for `path`, if the library knows that document.
///
/// Compared as opaque [`Path`] values; nothing is trimmed, shell-split or Unicode-normalised on the
/// way (R7).
fn entry_for_path<'a>(entries: &'a [LibraryEntry], path: &Path) -> Option<&'a LibraryEntry> {
    entries
        .iter()
        .find(|entry| entry.document.document.path == path)
}

/// The first provider that claims `viewer`.
///
/// Matching is ASCII-case-insensitive because a process name's case is not guaranteed to be the
/// same across the OSes and filesystems `oct` targets, and a viewer silently failing to match its
/// own provider is the exact gap this loop exists to close. When the viewer is unknown, no provider
/// is asked: attributing a position to a viewer nobody identified would be a guess, and the trait
/// forbids guessing.
fn provider_for_viewer<'a>(
    providers: &'a [Box<dyn ProgressProvider>],
    viewer: Option<&str>,
) -> Option<&'a dyn ProgressProvider> {
    let viewer = viewer?;
    providers
        .iter()
        .map(|provider| provider.as_ref())
        .find(|provider| {
            provider
                .viewer_ids()
                .iter()
                .any(|id| id.eq_ignore_ascii_case(viewer))
        })
}

/// Asks `provider` for `entry`'s position and records it when it differs from what is stored.
fn read_and_record(
    store: &Store,
    entry: &LibraryEntry,
    provider: &dyn ProgressProvider,
    report: &mut TrackReport,
) {
    match provider.read(&entry.document.document) {
        // `S3`: a provider with no record says nothing, and nothing must not become page zero.
        Ok(None) => {}
        Ok(Some(candidate)) => record_if_changed(store, entry, candidate, report),
        Err(error) => note_error(report, provider.id(), error),
    }
}

/// Records `candidate` unless `entry` already states the same position.
///
/// This is **not an optimization.** `progress` is an append-only log, so writing the same position
/// on every tick would grow the table without bound and turn "what has actually been read" into
/// noise. The comparison is what keeps one row per real change.
///
/// It compares the *position* — page and total — and deliberately not the provenance: a manual
/// entry and a provider that agree on the same page describe the same fact, and writing a second
/// row for it would make the record lie about how often the page changed.
fn record_if_changed(
    store: &Store,
    entry: &LibraryEntry,
    candidate: StoredProgress,
    report: &mut TrackReport,
) {
    if is_same_position(entry.progress.as_ref(), &candidate) {
        return;
    }
    match store.record_progress(entry.document.id, &candidate) {
        Ok(_) => report.recorded.push(RecordedPosition {
            doc_id: entry.document.id,
            progress: candidate,
        }),
        Err(error) => report.failures.push(format!(
            "progress for `{}` could not be recorded: {error}",
            entry.document.document.path.display()
        )),
    }
}

/// Whether `recorded` already states the same reading position as `candidate`.
///
/// A document with no recorded position never agrees with a candidate; page zero is a real position
/// and must be written as one when a provider reports it.
fn is_same_position(recorded: Option<&StoredProgress>, candidate: &StoredProgress) -> bool {
    match recorded {
        Some(recorded) => recorded.page == candidate.page && recorded.total == candidate.total,
        None => false,
    }
}

/// Files `error` under the right heading: a missing capability, or a failure to report.
///
/// [`OctError::NotSupported`] is a machine or build that cannot answer a question — a different
/// fact from "there is no position" — so it is never allowed to look like an empty answer.
fn note_error(report: &mut TrackReport, source: &str, error: OctError) {
    match error {
        OctError::NotSupported {
            capability,
            platform,
        } => report.missing_capabilities.push(MissingCapability {
            source: source.to_string(),
            reason: format!("{capability} is not supported on {platform}"),
        }),
        other => report.failures.push(format!("{source}: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use oct_core::{
        Availability, Confidence, Document, FileId, LaunchOutcome, OctError, OpenDocument,
        Platform, ProgressCapability, ProgressSource,
    };

    fn in_memory_store() -> Store {
        Store::open_in_memory().expect("open an in-memory store")
    }

    fn document(path: &str, dev: u64, ino: u64) -> Document {
        let mut doc = Document::new(path);
        doc.identity = Some(FileId::Posix { dev, ino });
        doc.pages = Some(80);
        doc
    }

    fn open_document(path: &str, viewer: &str) -> OpenDocument {
        OpenDocument {
            path: PathBuf::from(path),
            pid: Some(4242),
            viewer: Some(viewer.to_string()),
        }
    }

    fn position(page: u32) -> StoredProgress {
        StoredProgress::new(
            page,
            Some(80),
            ProgressSource::External("evince-gvfs"),
            Confidence::Exact,
        )
    }

    /// What a [`FakeProvider`] answers for one path.
    enum Answer {
        Nothing,
        At(u32),
        NotSupported,
        Fails,
    }

    /// A provider whose answers are scripted per path, and which records what it was asked.
    struct FakeProvider {
        id: &'static str,
        viewers: &'static [&'static str],
        availability: Availability,
        answers: Vec<(PathBuf, Answer)>,
        asked: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl FakeProvider {
        fn new(id: &'static str, viewers: &'static [&'static str]) -> Self {
            Self {
                id,
                viewers,
                availability: Availability::Installed,
                answers: Vec::new(),
                asked: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn answering(mut self, path: &str, answer: Answer) -> Self {
            self.answers.push((PathBuf::from(path), answer));
            self
        }

        fn missing(mut self) -> Self {
            self.availability = Availability::Missing;
            self
        }

        /// A handle to the ask log that survives moving the provider into the registry slice.
        fn recorder(&self) -> Arc<Mutex<Vec<PathBuf>>> {
            Arc::clone(&self.asked)
        }
    }

    impl ProgressProvider for FakeProvider {
        fn id(&self) -> &'static str {
            self.id
        }

        fn platform(&self) -> Platform {
            Platform::current()
        }

        fn viewer_ids(&self) -> &'static [&'static str] {
            self.viewers
        }

        fn capability(&self) -> ProgressCapability {
            ProgressCapability::Live
        }

        fn detect(&self) -> Availability {
            self.availability
        }

        fn read(&self, doc: &Document) -> Result<Option<StoredProgress>, OctError> {
            self.asked
                .lock()
                .expect("the recording lock")
                .push(doc.path.clone());
            match self
                .answers
                .iter()
                .find(|(path, _)| path == &doc.path)
                .map(|(_, answer)| answer)
            {
                Some(Answer::Nothing) | None => Ok(None),
                Some(Answer::At(page)) => Ok(Some(position(*page))),
                Some(Answer::NotSupported) => Err(OctError::NotSupported {
                    capability: "reading position",
                    platform: Platform::current(),
                }),
                Some(Answer::Fails) => Err(OctError::Adapter {
                    provider: self.id,
                    message: "the adapter exploded".to_string(),
                }),
            }
        }

        fn resume(&self, _doc: &Document, _page: u32) -> Result<LaunchOutcome, OctError> {
            unreachable!("the ingestion loop never resumes a document")
        }
    }

    /// A detector that reports a fixed list, or refuses the question entirely.
    struct FakeDetector {
        open: Vec<OpenDocument>,
        unsupported: bool,
    }

    impl FakeDetector {
        fn reporting(open: Vec<OpenDocument>) -> Self {
            Self {
                open,
                unsupported: false,
            }
        }

        fn unsupported() -> Self {
            Self {
                open: Vec::new(),
                unsupported: true,
            }
        }
    }

    impl OpenDocumentDetector for FakeDetector {
        fn id(&self) -> &'static str {
            "fake-detector"
        }

        fn platform(&self) -> Platform {
            Platform::current()
        }

        fn open_documents(&self) -> Result<Vec<OpenDocument>, OctError> {
            if self.unsupported {
                Err(OctError::NotSupported {
                    capability: "open-document detection",
                    platform: Platform::current(),
                })
            } else {
                Ok(self.open.clone())
            }
        }
    }

    #[test]
    fn a_changed_position_is_recorded() {
        let store = in_memory_store();
        let doc_id = store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("insert");
        store.record_progress(doc_id, &position(3)).expect("record");

        let provider = FakeProvider::new("evince-gvfs", &["evince"])
            .answering("/lib/Cosmos.pdf", Answer::At(9));
        let detector = FakeDetector::reporting(vec![open_document("/lib/Cosmos.pdf", "evince")]);

        let report = track_open_documents(&store, &detector, &[Box::new(provider)]);

        assert_eq!(report.recorded.len(), 1, "{report:?}");
        assert_eq!(report.recorded[0].doc_id, doc_id);
        assert_eq!(report.recorded[0].progress.page, 9);
        let stored = store.progress_for_document(doc_id).expect("read back");
        assert_eq!(stored.len(), 2, "the changed position is a new row");
        assert_eq!(stored.last().expect("a position").page, 9);
    }

    #[test]
    fn an_unchanged_position_is_not_written_again() {
        let store = in_memory_store();
        let doc_id = store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("insert");
        store.record_progress(doc_id, &position(9)).expect("record");

        let provider = FakeProvider::new("evince-gvfs", &["evince"])
            .answering("/lib/Cosmos.pdf", Answer::At(9));
        let detector = FakeDetector::reporting(vec![open_document("/lib/Cosmos.pdf", "evince")]);

        let report = track_open_documents(&store, &detector, &[Box::new(provider)]);

        assert!(report.recorded.is_empty(), "nothing changed: {report:?}");
        assert_eq!(
            store.progress_for_document(doc_id).expect("read back").len(),
            1,
            "a tick must not append a duplicate row for a page that has not moved"
        );
    }

    #[test]
    fn a_silent_provider_writes_nothing() {
        let store = in_memory_store();
        let doc_id = store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("insert");

        let provider = FakeProvider::new("evince-gvfs", &["evince"])
            .answering("/lib/Cosmos.pdf", Answer::Nothing);
        let detector = FakeDetector::reporting(vec![open_document("/lib/Cosmos.pdf", "evince")]);

        let report = track_open_documents(&store, &detector, &[Box::new(provider)]);

        assert!(report.recorded.is_empty(), "no record, no row: {report:?}");
        assert!(
            store.progress_for_document(doc_id).expect("read back").is_empty(),
            "Ok(None) must never become a page-zero row"
        );
    }

    #[test]
    fn a_document_the_store_does_not_know_is_ignored() {
        let store = in_memory_store();
        store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("insert");

        // A PDF open somewhere outside the watched folder: discovering it belongs to the
        // suggestion queue in a later unit, not to the library. The detector is not the source of
        // truth and must not add documents.
        let provider = FakeProvider::new("evince-gvfs", &["evince"])
            .answering("/elsewhere/Unknown.pdf", Answer::At(12));
        let asked = provider.recorder();
        let detector = FakeDetector::reporting(vec![open_document("/elsewhere/Unknown.pdf", "evince")]);

        let report = track_open_documents(&store, &detector, &[Box::new(provider)]);

        assert!(report.recorded.is_empty(), "{report:?}");
        assert_eq!(store.library().expect("list").len(), 1, "the library grows nowhere");
        assert!(
            asked.lock().expect("the recording lock").is_empty(),
            "a document the store does not know is not even asked about"
        );
    }

    #[test]
    fn the_provider_that_claims_the_viewer_is_the_one_asked() {
        let store = in_memory_store();
        store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("insert");

        let evince = FakeProvider::new("evince-gvfs", &["evince"])
            .answering("/lib/Cosmos.pdf", Answer::At(9));
        let zathura = FakeProvider::new("zathura", &["zathura"])
            .answering("/lib/Cosmos.pdf", Answer::At(3));
        let evince_log = evince.recorder();
        let zathura_log = zathura.recorder();
        let detector = FakeDetector::reporting(vec![open_document("/lib/Cosmos.pdf", "evince")]);

        let report = track_open_documents(
            &store,
            &detector,
            &[Box::new(zathura), Box::new(evince)],
        );

        assert_eq!(report.recorded.len(), 1, "{report:?}");
        // Zathura was registered first, but the open document belongs to Evince.
        assert_eq!(report.recorded[0].progress.page, 9);
        assert!(
            zathura_log.lock().expect("the recording lock").is_empty(),
            "a provider whose viewer does not own the document is never asked"
        );
        assert_eq!(
            evince_log.lock().expect("the recording lock").len(),
            1,
            "the provider that claims the viewer is the one asked"
        );
    }

    #[test]
    fn no_provider_is_asked_when_no_viewer_claims_the_document() {
        let store = in_memory_store();
        store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("insert");

        let evince = FakeProvider::new("evince-gvfs", &["evince"])
            .answering("/lib/Cosmos.pdf", Answer::At(9));
        let detector = FakeDetector::reporting(vec![open_document("/lib/Cosmos.pdf", "okular")]);

        let report = track_open_documents(&store, &detector, &[Box::new(evince)]);

        assert!(report.recorded.is_empty(), "{report:?}");
        assert_eq!(
            report.missing_capabilities.len(),
            0,
            "an unclaimed viewer is silence, not a missing capability"
        );
    }

    #[test]
    fn a_missing_capability_is_reported_and_the_loop_survives() {
        let store = in_memory_store();
        let broken = store
            .upsert_document(&document("/lib/Broken.pdf", 1, 1))
            .expect("insert");
        let good = store
            .upsert_document(&document("/lib/Good.pdf", 1, 2))
            .expect("insert");

        let provider = FakeProvider::new("evince-gvfs", &["evince"])
            .answering("/lib/Broken.pdf", Answer::NotSupported)
            .answering("/lib/Good.pdf", Answer::At(7));
        let detector = FakeDetector::reporting(vec![
            open_document("/lib/Broken.pdf", "evince"),
            open_document("/lib/Good.pdf", "evince"),
        ]);

        let report = track_open_documents(&store, &detector, &[Box::new(provider)]);

        assert_eq!(report.missing_capabilities.len(), 1, "{report:?}");
        assert_eq!(report.missing_capabilities[0].source, "evince-gvfs");
        assert!(
            report.missing_capabilities[0].reason.contains("not supported"),
            "the reason names the missing capability: {report:?}"
        );
        assert!(
            store.progress_for_document(broken).expect("read").is_empty(),
            "a missing capability is not an empty position"
        );
        assert_eq!(
            report.recorded.len(),
            1,
            "the next document is still processed: {report:?}"
        );
        assert_eq!(report.recorded[0].doc_id, good);
    }

    #[test]
    fn an_adapter_failure_is_reported_and_the_loop_survives() {
        let store = in_memory_store();
        let broken = store
            .upsert_document(&document("/lib/Broken.pdf", 1, 1))
            .expect("insert");
        let good = store
            .upsert_document(&document("/lib/Good.pdf", 1, 2))
            .expect("insert");

        let provider = FakeProvider::new("evince-gvfs", &["evince"])
            .answering("/lib/Broken.pdf", Answer::Fails)
            .answering("/lib/Good.pdf", Answer::At(2));
        let detector = FakeDetector::reporting(vec![
            open_document("/lib/Broken.pdf", "evince"),
            open_document("/lib/Good.pdf", "evince"),
        ]);

        let report = track_open_documents(&store, &detector, &[Box::new(provider)]);

        assert_eq!(report.failures.len(), 1, "{report:?}");
        assert!(
            report.failures[0].contains("evince-gvfs"),
            "the failing provider is named: {report:?}"
        );
        assert!(
            report.missing_capabilities.is_empty(),
            "a failure is not a missing capability: {report:?}"
        );
        assert!(store.progress_for_document(broken).expect("read").is_empty());
        assert_eq!(report.recorded.len(), 1, "the next document still records: {report:?}");
        assert_eq!(report.recorded[0].doc_id, good);
    }

    #[test]
    fn an_unsupported_detector_is_reported_as_a_missing_capability() {
        let store = in_memory_store();
        store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("insert");
        let provider = FakeProvider::new("evince-gvfs", &["evince"]);

        let report = track_open_documents(&store, &FakeDetector::unsupported(), &[Box::new(provider)]);

        assert_eq!(report.missing_capabilities.len(), 1, "{report:?}");
        assert_eq!(report.missing_capabilities[0].source, "fake-detector");
    }

    #[test]
    fn the_launch_sweep_covers_a_document_that_is_not_open() {
        let store = in_memory_store();
        let doc_id = store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("insert");

        // Nothing is open: this is the catch-up case. The position was written while the
        // application was closed and must still be recorded.
        let provider = FakeProvider::new("evince-gvfs", &["evince"])
            .answering("/lib/Cosmos.pdf", Answer::At(11));

        let report = sweep_library(&store, &[Box::new(provider)]);

        assert_eq!(report.recorded.len(), 1, "{report:?}");
        assert_eq!(report.recorded[0].doc_id, doc_id);
        let stored = store.progress_for_document(doc_id).expect("read back");
        assert_eq!(stored.last().expect("a position").page, 11);
    }

    #[test]
    fn the_sweep_leaves_a_missing_provider_alone() {
        let store = in_memory_store();
        let doc_id = store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("insert");
        let provider = FakeProvider::new("evince-gvfs", &["evince"])
            .answering("/lib/Cosmos.pdf", Answer::At(11))
            .missing();

        let report = sweep_library(&store, &[Box::new(provider)]);

        assert!(report.recorded.is_empty(), "{report:?}");
        assert!(report.missing_capabilities.is_empty(), "{report:?}");
        assert!(report.failures.is_empty(), "{report:?}");
        assert!(
            store.progress_for_document(doc_id).expect("read back").is_empty(),
            "a provider that cannot run is not asked"
        );
    }
}
