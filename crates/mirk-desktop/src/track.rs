//! The ingestion loop: the connection between the open-document detector, the progress providers
//! and the store.
//!
//! Before this module the three parts existed and were correct, and the application was the gap
//! between them: nothing called the detector, nothing asked a provider for a position, and
//! [`mirk_store::Store::record_progress`] had no caller outside a test. Reading a document in Evince
//! therefore produced no record at all (`L35`).
//!
//! Every decision lives here as a plain function taking its collaborators as arguments — the
//! detector, the provider registry and the store are all passed in — so the rules below are pinned
//! by tests that need neither a display nor a running Tauri application. `main.rs` owns only the
//! tick and the launch sweep that call them.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mirk_core::{Availability, MirkError, OpenDocumentDetector, ProgressProvider, StoredProgress};
use mirk_platform::recent::{RecentDocuments, recently_used_path};
use mirk_store::{LibraryEntry, Store};

/// How often the tick asks the detector what is open and the providers what those documents say.
///
/// A reading position changes when a page turns, not continuously, and T1 measured Evince landing
/// its GVFS write within roughly three seconds of a page change, with the document still open.
/// Three seconds is therefore short enough for a position to appear immediate and long enough that
/// the cost is nothing: a tick reads only the handful of documents that are actually open
/// (~10 ms/file, T3), while the library-wide sweep runs once at launch instead of on this interval.
pub const POLL_INTERVAL: Duration = Duration::from_secs(3);

/// How old a known reading time has to be before watching a document open is news again (R30).
///
/// The live observation is exact when a page turns. This bounds the other case — somebody opens a
/// document and reads the same page — so that "you read this today" can reach the store without a row
/// per tick: the tick runs every three seconds and a position that does not move is not news, but a
/// document open in front of somebody is. A day is the granularity the rule is measured in (thirty of
/// them), so anything finer would add rows nobody can see, and anything coarser would let a document
/// read this morning claim to have been read yesterday.
const LIVE_READ_GRANULARITY: i64 = 24 * 60 * 60;

/// A capability this build or machine could not provide, named rather than shown as an empty answer.
///
/// [`mirk_core::MirkError::NotSupported`] means *"this cannot be answered here"*, which is a different
/// fact from *"I looked and there is no position"* (`Ok(None)`). Collapsing the first into the
/// second is exactly the lie `mirk-platform`'s honest stubs exist to prevent, so a missing capability
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

    // R30. The desktop's own recent-files history is the one source that knows WHEN a document was read
    // — the viewer wrote it down when the document was opened — so the sweep is where a document nobody
    // has watched being read can still learn its reading time. It is read ONCE per sweep rather than once
    // per document: the file holds a whole desktop's worth of entries, and a document it does not know is
    // answered by an empty set instead of a second read. A history that cannot be read at all is a
    // failure worth reporting and nothing more: a position is still worth recording when the time it was
    // read is not known, and `None` is how this module says exactly that.
    let history = recently_used_path()
        .and_then(|path| RecentDocuments::read(&path))
        .map_err(|error| {
            report.failures.push(format!(
                "the recent-files history could not be read: {error}"
            ));
        })
        .ok();

    sweep_with_history(store, providers, history.as_ref(), &mut report);
    report
}

/// The library's document-set shape: how many documents it holds, and the highest `documents.id`
/// among them.
///
/// This is the signature the tick compares between calls. `R27` takes a removed folder's documents
/// with it, and a later rescan re-adds them as NEW rows with NEW, higher ids, so a change in the
/// count or in the highest id is a change in the set. It is derived from what [`Store::library`]
/// already returns rather than from a new store method, which is what keeps the rule a plain function
/// that can be tested without a store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryShape {
    /// How many documents the library holds.
    pub documents: usize,
    /// The highest `documents.id`, or [`None`] for an empty library.
    pub highest_id: Option<i64>,
}

impl LibraryShape {
    /// The shape of `entries`, as [`Store::library`] returns them.
    pub fn of(entries: &[LibraryEntry]) -> Self {
        Self {
            documents: entries.len(),
            highest_id: entries.iter().map(|entry| entry.document.id).max(),
        }
    }
}

/// Whether the library's set of documents changed since the shape the loop last observed.
///
/// The first observation (`None`) is always a change: the loop has never seen this set, so the sweep
/// it triggers is the one that catches up. A pure function rather than a method so the rule can be
/// pinned without a store.
pub fn library_changed(previous: Option<LibraryShape>, current: LibraryShape) -> bool {
    previous != Some(current)
}

/// The tick loop's memory of the library's document set (`R27` + `S5`).
///
/// Removing a watched folder takes its documents and their positions with it, by the user's own
/// choice. Re-watching the folder brings the documents back through a rescan, but nothing would ask
/// the viewer for their readings again: the launch sweep is the only place mirk reads the viewer's
/// history, and at launch there was nothing to observe. This watch closes that gap — when the set of
/// documents changed it runs the same sweep the launch does, and when it has not it does nothing at
/// all, so the history is still read once per sweep and never once per three-second tick.
#[derive(Debug, Default)]
pub struct LibraryWatch {
    /// The shape seen at the last observation, or [`None`] before the first one.
    seen: Option<LibraryShape>,
}

impl LibraryWatch {
    /// Records the library's current shape WITHOUT sweeping.
    ///
    /// The launch sweep has just observed this exact set, so seeding the watch from it is what keeps
    /// the first tick, three seconds later, from re-reading the history for a set that has not
    /// changed. It costs one extra [`Store::library`] query at launch and saves a whole
    /// `recently-used.xbel` parse; a library that cannot be read here leaves the watch unseeded, and
    /// the first tick's [`LibraryWatch::refresh`] reports that failure rather than swallowing it.
    pub fn seed(&mut self, store: &Store) {
        if let Ok(entries) = store.library() {
            self.seen = Some(LibraryShape::of(&entries));
        }
    }

    /// Runs the launch sweep when the set of documents changed, and nothing when it has not.
    ///
    /// `None` means the set is the one already observed — the common case, once per tick. `Some`
    /// carries the sweep's report, including an empty one, so "nothing changed" and "a sweep that
    /// found nothing" stay distinguishable. A library that cannot be read is reported through the
    /// returned report, never swallowed and never mistaken for an unchanged set.
    pub fn refresh(
        &mut self,
        store: &Store,
        providers: &[Box<dyn ProgressProvider>],
    ) -> Option<TrackReport> {
        let entries = match store.library() {
            Ok(entries) => entries,
            Err(error) => {
                let mut report = TrackReport::default();
                report
                    .failures
                    .push(format!("the library could not be read: {error}"));
                return Some(report);
            }
        };
        let current = LibraryShape::of(&entries);
        if !library_changed(self.seen, current) {
            return None;
        }
        self.seen = Some(current);
        Some(sweep_library(store, providers))
    }
}

/// The sweep's body, with the history injected.
///
/// The decision is separable from where the history lives for the same reason the providers are
/// injected: this is the part with a rule in it, and a rule that can only be exercised against whatever
/// happens to be in `~/.local/share` is a rule nobody can test.
fn sweep_with_history(
    store: &Store,
    providers: &[Box<dyn ProgressProvider>],
    history: Option<&RecentDocuments>,
    report: &mut TrackReport,
) {
    let entries = match store.library() {
        Ok(entries) => entries,
        Err(error) => {
            report
                .failures
                .push(format!("the library could not be read: {error}"));
            return;
        }
    };

    // One clock reading for the whole sweep: every observation is judged against the same "now", so a
    // slow sweep cannot make an early document's reading look like the future.
    let now = now_seconds();

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
                    let candidate = with_swept_reading_time(candidate, entry, history, now);
                    record_if_changed(store, entry, candidate, report);
                    // One source of truth per pass: a second provider agreeing on the same page is
                    // not a second fact, and the registry order decides which one is heard.
                    break;
                }
                Err(error) => note_error(report, provider.id(), error),
            }
        }
    }
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
/// same across the OSes and filesystems `mirk` targets, and a viewer silently failing to match its
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
        Ok(Some(candidate)) => {
            let candidate = with_live_reading_time(candidate, entry, now_seconds());
            record_if_changed(store, entry, candidate, report)
        }
        Err(error) => note_error(report, provider.id(), error),
    }
}

/// Records `candidate` unless `entry` already states the same observation.
///
/// This is **not an optimization.** `progress` is an append-only log, so writing the same position
/// on every tick would grow the table without bound and turn "what has actually been read" into
/// noise. The comparison is what keeps one row per real change.
///
/// It compares the *position* — page and total — and the reading time, and deliberately not the
/// provenance: a manual entry and a provider that agree on the same page describe the same fact, and
/// writing a second row for it would make the record lie about how often the page changed. The reading
/// time is the second fact this comparison exists for (R30): a document can be read without its page
/// moving, and the history's answer has to be able to reach the store, so "nothing changed" now means
/// neither fact changed.
fn record_if_changed(
    store: &Store,
    entry: &LibraryEntry,
    candidate: StoredProgress,
    report: &mut TrackReport,
) {
    if is_same_observation(entry.progress.as_ref(), &candidate) {
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

/// Whether `recorded` already states the same reading position AND the same reading time as `candidate`.
///
/// A document with no recorded position never agrees with a candidate; page zero is a real position
/// and must be written as one when a provider reports it.
fn is_same_observation(recorded: Option<&StoredProgress>, candidate: &StoredProgress) -> bool {
    match recorded {
        Some(recorded) => {
            recorded.page == candidate.page
                && recorded.total == candidate.total
                && recorded.read_at == candidate.read_at
        }
        None => false,
    }
}

/// What the store already knows about when this document was read.
fn known_reading_time(entry: &LibraryEntry) -> Option<i64> {
    entry.progress.as_ref().and_then(|progress| progress.read_at)
}

/// Whether `candidate` is a reading time we can believe.
///
/// A time later than `now` is not an observation about the past — it is a clock that moved, or a
/// history somebody edited — and believing it would let a false time into the store. `read_at` is a
/// high-water mark, so such a time would be kept forever and the stale rule, which asks whether
/// `now - read_at` is large, could never call the document stale again.
///
/// `None` for `now` is a clock that cannot say what time it is. That is not evidence against a known
/// value, so the candidate is kept: an unreadable clock must not erase the only time the store knew.
fn credible_reading_time(candidate: Option<i64>, now: Option<i64>) -> Option<i64> {
    match (candidate, now) {
        (Some(candidate), Some(now)) if candidate > now => None,
        (candidate, _) => candidate,
    }
}

/// The newer of two reading times, or whichever one exists (R30).
///
/// A reading time is a **high-water mark**: the answer to "when did you last read this" can only move
/// forward. Without this, a later sweep finding an OLDER time — the history records the moment a document
/// was opened, which is seconds before the tick first saw the page turn — would walk the fact backwards
/// and make a document read today look older than the store already knew it was.
///
/// BOTH sides go through [`credible_reading_time`] first, because a time in the future is not a reading
/// whichever side it arrives from. An observation the history reports from the future is ignored and the
/// sweep keeps what the store already knew (or `None`) while still recording the position; a value a
/// previous build already wrote into the store is dropped for the same reason, which is what heals it —
/// the sweep runs at every launch, so a document nobody opens again does not stay stamped in the future
/// for ever, which is the one state the stale rule could never reach. `now` is needed because a sweep
/// without a clock has no frame for belief.
fn newest_reading_time(known: Option<i64>, observed: Option<i64>, now: Option<i64>) -> Option<i64> {
    match (
        credible_reading_time(known, now),
        credible_reading_time(observed, now),
    ) {
        (Some(known), Some(observed)) => Some(known.max(observed)),
        (known, observed) => known.or(observed),
    }
}

/// The candidate the tick records: the position the provider reported, carrying a reading time, because
/// this is the one code path that watched the document being read.
fn with_live_reading_time(
    candidate: StoredProgress,
    entry: &LibraryEntry,
    now: Option<i64>,
) -> StoredProgress {
    candidate.with_read_at(live_reading_time(known_reading_time(entry), now))
}

/// The reading time to record for a document somebody has open (R30).
///
/// `None` for `now` is a clock that cannot say what time it is, and an unknown clock cannot claim a
/// reading happened: the time we already knew is kept instead, which writes nothing because nothing
/// changed.
fn live_reading_time(known: Option<i64>, now: Option<i64>) -> Option<i64> {
    let Some(now) = now else {
        return known;
    };
    // The stored time is judged by the same rule the sweep applies from the other side: a time in the
    // future is not a reading we can believe. Here the application is watching the document open, so
    // `now` is itself an observation, and the honest answer is to replace the moved clock rather than
    // keep it. A credible, recent time is already the answer, and refreshing it every three seconds
    // would fill the log with rows that say the same thing.
    match credible_reading_time(known, Some(now)) {
        Some(known) if now - known < LIVE_READ_GRANULARITY => Some(known),
        _ => Some(now),
    }
}

/// The candidate the sweep records: the position the provider reported, carrying the reading time the
/// history knows unless the store already knows a newer one.
fn with_swept_reading_time(
    candidate: StoredProgress,
    entry: &LibraryEntry,
    history: Option<&RecentDocuments>,
    now: Option<i64>,
) -> StoredProgress {
    let observed = history.and_then(|documents| documents.read_at(&entry.document.document.path));
    candidate.with_read_at(newest_reading_time(known_reading_time(entry), observed, now))
}

/// The clock, in Unix seconds, or `None` when it cannot answer. A sweep that cannot say what time it is
/// records positions without claiming a reading time, which is the honest half it can still do.
fn now_seconds() -> Option<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_secs() as i64)
}

/// Files `error` under the right heading: a missing capability, or a failure to report.
///
/// [`MirkError::NotSupported`] is a machine or build that cannot answer a question — a different
/// fact from "there is no position" — so it is never allowed to look like an empty answer.
fn note_error(report: &mut TrackReport, source: &str, error: MirkError) {
    match error {
        MirkError::NotSupported {
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

    use mirk_core::{
        Availability, Confidence, Document, FileId, LaunchOutcome, MirkError, OpenDocument,
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

        fn read(&self, doc: &Document) -> Result<Option<StoredProgress>, MirkError> {
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
                Some(Answer::NotSupported) => Err(MirkError::NotSupported {
                    capability: "reading position",
                    platform: Platform::current(),
                }),
                Some(Answer::Fails) => Err(MirkError::Adapter {
                    provider: self.id,
                    message: "the adapter exploded".to_string(),
                }),
            }
        }

        fn resume(&self, _doc: &Document, _page: u32) -> Result<LaunchOutcome, MirkError> {
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

        fn open_documents(&self) -> Result<Vec<OpenDocument>, MirkError> {
            if self.unsupported {
                Err(MirkError::NotSupported {
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

    /// S2, as `R30` restated it: an unchanged position is not written again, and neither is a reading
    /// time that has already arrived.
    ///
    /// The tick runs every three seconds. The first tick that sees a document open when the store knows
    /// nothing about when it was read has something new to say — somebody is reading it now — and writes
    /// one row. Every tick after that has nothing to add: the page has not moved and the reading time is
    /// already the answer. This is the test that keeps an append-only log from growing a row per tick,
    /// which is the whole reason the comparison exists.
    #[test]
    fn an_unchanged_position_is_not_written_again() {
        let store = in_memory_store();
        let doc_id = store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("insert");
        store.record_progress(doc_id, &position(9)).expect("record");

        let reading = || {
            let provider = FakeProvider::new("evince-gvfs", &["evince"])
                .answering("/lib/Cosmos.pdf", Answer::At(9));
            let detector = FakeDetector::reporting(vec![open_document("/lib/Cosmos.pdf", "evince")]);
            track_open_documents(&store, &detector, &[Box::new(provider)])
        };

        // The first tick learns when the document is being read, which is a fact the row did not carry.
        let first = reading();
        assert_eq!(first.recorded.len(), 1, "{first:?}");
        let stored = store.progress_for_document(doc_id).expect("read back");
        assert_eq!(stored.len(), 2);
        assert!(
            stored[1].read_at.is_some(),
            "the reading time is what the new row carries"
        );

        // And then nothing moves: the page has not changed and the reading time is already known.
        let second = reading();
        assert!(second.recorded.is_empty(), "nothing changed: {second:?}");
        assert_eq!(
            store.progress_for_document(doc_id).expect("read back").len(),
            2,
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

        let mut report = TrackReport::default();
        sweep_with_history(&store, &[Box::new(provider)], None, &mut report);

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

        let mut report = TrackReport::default();
        sweep_with_history(&store, &[Box::new(provider)], None, &mut report);

        assert!(report.recorded.is_empty(), "{report:?}");
        assert!(report.missing_capabilities.is_empty(), "{report:?}");
        assert!(report.failures.is_empty(), "{report:?}");
        assert!(
            store.progress_for_document(doc_id).expect("read back").is_empty(),
            "a provider that cannot run is not asked"
        );
    }

    // --- the trigger: a re-add is observed again ------------------------------------------------

    /// The user's report: they stopped watching a folder and added it again, and every document read
    /// 0%. `R27` takes a removed folder's documents and their positions with it, which is intended;
    /// what was missing is that nothing RE-OBSERVES after the rescan re-adds them. The readings must
    /// come back from the viewer, not from mirk, so the loop notices the set of documents changed and
    /// runs the launch sweep again.
    ///
    /// The test walks exactly that: a recorded position, the folder removal that takes it, the rescan
    /// that re-adds the document as a NEW row with no progress, and then the loop's own decision. The
    /// second document keeps the table non-empty so the re-add cannot reuse the removed row's id, which
    /// is what "a new row" means.
    #[test]
    fn a_readded_document_is_observed_again_by_the_triggered_sweep() {
        let store = in_memory_store();
        let watched = store.record_folder("/lib", true).expect("record the watch");
        let gone = store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("insert");
        store
            .upsert_document(&document("/keep/Other.pdf", 2, 2))
            .expect("insert a document that stays");
        store.record_progress(gone, &position(11)).expect("record");

        // The loop has already observed this library: the launch sweep ran and the watch was seeded.
        let mut watch = LibraryWatch::default();
        watch.seed(&store);
        assert!(
            watch.refresh(&store, &[]).is_none(),
            "a set that has not changed is not swept"
        );

        // The user stops watching the folder: the document and its position go with it (R27).
        store
            .forget_folder(watched)
            .expect("remove")
            .expect("the watch was known");
        assert!(
            store.progress_for_document(gone).expect("read").is_empty(),
            "the removal takes the position with the document"
        );

        // The user adds the folder again. A rescan re-adds the document as a NEW row: new id, no progress.
        let readded = store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("re-add");
        assert_ne!(readded, gone, "the rescan's row is a new one, not the removed row");
        assert!(
            store.progress_for_document(readded).expect("read").is_empty(),
            "the re-added document has no progress yet"
        );

        // The loop's own decision: the set of documents changed, so the launch sweep runs again and
        // asks the viewer, which is the honest source of the reading.
        let provider = FakeProvider::new("evince-gvfs", &["evince"])
            .answering("/lib/Cosmos.pdf", Answer::At(11));
        let report = watch
            .refresh(&store, &[Box::new(provider)])
            .expect("the changed set is swept");

        assert_eq!(report.recorded.len(), 1, "{report:?}");
        assert_eq!(report.recorded[0].doc_id, readded, "the reading landed on the new row");
        assert_eq!(report.recorded[0].progress.page, 11);
        let stored = store.progress_for_document(readded).expect("read back");
        assert_eq!(
            stored.last().expect("a position").page,
            11,
            "the viewer's reading is back"
        );
    }

    /// The other half of the rule: a three-second tick over a library whose set of documents has not
    /// changed must do nothing at all, so the history is read once per sweep and never once per tick.
    #[test]
    fn an_unchanged_library_is_not_swept_again() {
        let store = in_memory_store();
        store
            .upsert_document(&document("/lib/Cosmos.pdf", 1, 1))
            .expect("insert");
        let mut watch = LibraryWatch::default();
        watch.seed(&store);

        let provider = FakeProvider::new("evince-gvfs", &["evince"])
            .answering("/lib/Cosmos.pdf", Answer::At(11));
        let asked = provider.recorder();

        assert!(
            watch.refresh(&store, &[Box::new(provider)]).is_none(),
            "the same set is not swept again"
        );
        assert!(
            asked.lock().expect("the recording lock").is_empty(),
            "an unchanged library is not even asked about"
        );
    }

    /// The shape rule itself: the first observation is a change, a re-add changes the highest id even
    /// when the count is the same, a removal changes the count, and an equal shape is not a change.
    #[test]
    fn a_changed_document_set_is_what_the_shape_notices() {
        let one = LibraryShape { documents: 1, highest_id: Some(1) };
        assert!(
            library_changed(None, one),
            "the loop has never seen this set, so its first sweep is a change"
        );
        assert!(!library_changed(Some(one), one), "the same set is not news");
        assert!(
            library_changed(Some(one), LibraryShape { documents: 2, highest_id: Some(2) }),
            "a re-add changes the count"
        );
        assert!(
            library_changed(
                Some(LibraryShape { documents: 2, highest_id: Some(2) }),
                LibraryShape { documents: 2, highest_id: Some(3) }
            ),
            "a re-add with the same count is still a change: the new row has a higher id"
        );
        assert_eq!(LibraryShape::of(&[]), LibraryShape { documents: 0, highest_id: None });
    }

    // --- R30: when the document was read ---------------------------------------------------------

    /// Writes a `recently-used.xbel` with one bookmark per path, reads it back through the same reader
    /// the application uses, and removes the file. Teaching the reader a shortcut instead would test the
    /// shortcut.
    ///
    /// The stamps are given as the ISO text the viewer writes, and `1_600_000_000` is
    /// `2020-09-13T12:26:40Z` — derived with `date -u -d @1600000000`, not with the conversion under test.
    fn history_file(entries: &[(&str, &str)]) -> RecentDocuments {
        // The name must be unique per CALL, not per process: every test runs in the same process, so a
        // shared name lets one test overwrite or delete the file another one is about to read. That race
        // was real — it failed a different test on each run and passed on the third — and it had nothing
        // to do with the code under test, which is the worst kind of test failure.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "mirk-test-history-{}-{unique}.xbel",
            std::process::id()
        ));
        let mut xml = String::from("<xbel version=\"1.0\">\n");
        for (document, stamp) in entries {
            xml.push_str(&format!(
                "  <bookmark href=\"file://{document}\" added=\"{stamp}\" modified=\"{stamp}\"/>\n"
            ));
        }
        xml.push_str("</xbel>\n");
        std::fs::write(&path, xml).expect("write the history");
        let documents = RecentDocuments::read(&path).expect("read the history back");
        std::fs::remove_file(&path).expect("remove the fixture");
        documents
    }

    /// The live observation: what the tick records for a document somebody has open.
    #[test]
    fn watching_a_document_being_read_stamps_the_time_it_happened() {
        let now = 1_700_000_000;

        // Nothing known: the moment we watched it is the only answer there is.
        assert_eq!(live_reading_time(None, Some(now)), Some(now));
        // A time from the same day is already that answer, and refreshing it every three seconds would
        // write a row per tick that says nothing new — the table is an append-only log.
        assert_eq!(live_reading_time(Some(now - 60), Some(now)), Some(now - 60));
        assert_eq!(
            live_reading_time(Some(now - LIVE_READ_GRANULARITY + 1), Some(now)),
            Some(now - LIVE_READ_GRANULARITY + 1)
        );
        // Older than that, and seeing the document open IS news.
        assert_eq!(
            live_reading_time(Some(now - LIVE_READ_GRANULARITY - 1), Some(now)),
            Some(now)
        );
        // A clock that cannot answer cannot claim a reading happened: the time already known is kept.
        assert_eq!(live_reading_time(Some(now - 90_000), None), Some(now - 90_000));
        assert_eq!(live_reading_time(None, None), None);
    }

    /// A stored time in the future is a clock that moved, not evidence about a reading, so the live
    /// path REPLACES it rather than keeping it: the application is watching the document open, and
    /// `now` is the moment that observation is happening. This is the same `credible_reading_time`
    /// rule the sweep applies from the other side, where a time in the future is ignored instead.
    #[test]
    fn a_stored_time_in_the_future_is_replaced_when_the_document_is_watched() {
        let now = 1_700_000_000;

        assert_eq!(
            live_reading_time(Some(now + 5_000), Some(now)),
            Some(now),
            "a moved clock is not a reading, so the moment we watched it is the answer"
        );
        assert_eq!(
            live_reading_time(Some(now + 1), Some(now)),
            Some(now),
            "even one second in the future is not a reading"
        );
    }

    /// The predicate both paths share (R30): a candidate later than the known time is not a reading we
    /// can believe, and an unreadable clock is not evidence that a known value should disappear.
    #[test]
    fn a_reading_time_in_the_future_is_not_credible() {
        let now = Some(1_700_000_000);

        assert_eq!(
            credible_reading_time(Some(1_700_000_000), now),
            Some(1_700_000_000),
            "a time equal to now is a real instant"
        );
        assert_eq!(
            credible_reading_time(Some(1_699_999_999), now),
            Some(1_699_999_999),
            "a time in the past is credible"
        );
        assert_eq!(
            credible_reading_time(Some(1_700_000_001), now),
            None,
            "a time in the future is not a reading"
        );
        assert_eq!(
            credible_reading_time(None, now),
            None,
            "nothing observed, nothing credible"
        );
        // An unreadable clock cannot judge, so it must not erase the time the store already knew.
        assert_eq!(
            credible_reading_time(Some(1_700_000_001), None),
            Some(1_700_000_001),
            "no clock is not evidence against a known value"
        );
        assert_eq!(credible_reading_time(None, None), None);
    }

    /// The history's answer against what the store already knew.
    #[test]
    fn a_reading_time_never_moves_backwards() {
        let now = Some(1_700_000_000);

        assert_eq!(newest_reading_time(Some(300), Some(200), now), Some(300));
        assert_eq!(newest_reading_time(Some(200), Some(300), now), Some(300));
        assert_eq!(newest_reading_time(Some(200), None, now), Some(200));
        assert_eq!(newest_reading_time(None, Some(300), now), Some(300));
        assert_eq!(newest_reading_time(None, None, now), None);
    }

    /// The comparison that decides whether anything is written at all.
    #[test]
    fn a_new_reading_time_is_a_change_even_when_the_page_did_not_move() {
        // This is the rule that lets the history reach the store: a document can be read without its page
        // moving, so "the position is the same" is not the same answer as "nothing happened".
        let recorded = position(11).with_read_at(Some(100));

        assert!(is_same_observation(Some(&recorded), &position(11).with_read_at(Some(100))));
        assert!(
            !is_same_observation(Some(&recorded), &position(11).with_read_at(Some(200))),
            "a newer time is news"
        );
        assert!(
            !is_same_observation(Some(&recorded), &position(11)),
            "forgetting a time is not nothing"
        );
        assert!(!is_same_observation(None, &recorded), "no row at all never agrees");
    }

    /// The wiring: the sweep records the history's reading time even when the position did not move, and
    /// writes nothing a second time. A rule that is computed and never used is a defect this project has
    /// already paid for twice.
    #[test]
    fn the_sweep_records_the_historys_reading_time_exactly_once() {
        let store = in_memory_store();
        let path = "/lib/Cosmos.pdf";
        let doc_id = store.upsert_document(&document(path, 1, 1)).expect("insert");
        // The store already knows the position and knows nothing about when it was read: the state the
        // maintainer's whole library is in, and the reason the stale carousel is empty.
        store
            .record_progress(doc_id, &position(11))
            .expect("record the position the provider will report again");

        let history = history_file(&[(path, "2020-09-13T12:26:40Z")]);
        let provider = FakeProvider::new("evince-gvfs", &["evince"]).answering(path, Answer::At(11));

        let mut first = TrackReport::default();
        sweep_with_history(&store, &[Box::new(provider)], Some(&history), &mut first);

        let recorded = store.progress_for_document(doc_id).expect("read back");
        assert_eq!(recorded.len(), 2, "a new reading time is a new row: {first:?}");
        assert_eq!(recorded[1].read_at, Some(1_600_000_000), "the history's own instant");

        // A second sweep with the same history writes nothing: a time alone, like a position alone, is
        // not news, and the log must not grow a row per launch for a fact that did not change.
        let provider = FakeProvider::new("evince-gvfs", &["evince"]).answering(path, Answer::At(11));
        let mut second = TrackReport::default();
        sweep_with_history(&store, &[Box::new(provider)], Some(&history), &mut second);

        assert!(second.recorded.is_empty(), "{second:?}");
        assert_eq!(store.progress_for_document(doc_id).expect("read back").len(), 2);
    }

    /// A document the history does not know keeps an unknown reading time rather than borrowing one.
    #[test]
    fn the_sweep_invents_no_reading_time_for_a_document_the_history_does_not_know() {
        let store = in_memory_store();
        let doc_id = store
            .upsert_document(&document("/lib/Unknown.pdf", 1, 1))
            .expect("insert");
        store.record_progress(doc_id, &position(11)).expect("record");

        let history = history_file(&[("/lib/Other.pdf", "2020-09-13T12:26:40Z")]);
        let provider =
            FakeProvider::new("evince-gvfs", &["evince"]).answering("/lib/Unknown.pdf", Answer::At(11));

        let mut report = TrackReport::default();
        sweep_with_history(&store, &[Box::new(provider)], Some(&history), &mut report);

        let recorded = store.progress_for_document(doc_id).expect("read back");
        assert_eq!(recorded.len(), 1, "nothing changed, so nothing was written: {report:?}");
        assert_eq!(recorded[0].read_at, None, "unknown stays unknown");
    }

    /// A history that dates a document in the FUTURE is not evidence about a reading, and because
    /// `read_at` is a high-water mark such a time would be kept forever: a document nobody has touched
    /// for a year would be stamped as read tomorrow, and the stale rule could never call it stale again.
    /// The sweep still records the position it can see — that half it can believe — and leaves the
    /// reading time alone, so a real, older time the store already knew survives the bad observation.
    /// A time a previous build already wrote into the store is judged by the same rule (R30).
    ///
    /// Filtering the observation stops a bad time from entering; this covers the state a build before
    /// that could have left behind. A stored time in the future is a moved clock too, and because
    /// `read_at` is a high-water mark nothing would ever lower it: the stale rule asks whether
    /// `now - read_at` is large, so that document could never be called stale again. The sweep runs at
    /// every launch and now heals it without waiting for somebody to open the document — what remains is
    /// whatever the history can honestly say, or nothing at all.
    #[test]
    fn the_sweep_heals_a_reading_time_a_previous_build_left_in_the_future() {
        let store = in_memory_store();
        let healed = "/lib/Healed.pdf";
        let replaced = "/lib/Replaced.pdf";
        let healed_id = store.upsert_document(&document(healed, 1, 1)).expect("insert");
        let replaced_id = store.upsert_document(&document(replaced, 1, 2)).expect("insert");
        // Two rows a broken clock left stamped in the future; nothing else knows a time for them.
        let year_2999 = 32_472_144_000;
        store
            .record_progress(healed_id, &position(11).with_read_at(Some(year_2999)))
            .expect("record");
        store
            .record_progress(replaced_id, &position(11).with_read_at(Some(year_2999)))
            .expect("record");

        // The history can honestly date one of them and knows nothing about the other.
        let history = history_file(&[(replaced, "2020-09-13T12:26:40Z")]);
        let provider = FakeProvider::new("evince-gvfs", &["evince"])
            .answering(healed, Answer::At(12))
            .answering(replaced, Answer::At(12));

        let mut report = TrackReport::default();
        sweep_with_history(&store, &[Box::new(provider)], Some(&history), &mut report);

        assert_eq!(report.recorded.len(), 2, "the positions are still recorded: {report:?}");

        let healed_rows = store.progress_for_document(healed_id).expect("read back");
        assert_eq!(healed_rows.last().expect("a position").page, 12);
        assert_eq!(
            healed_rows.last().expect("a position").read_at,
            None,
            "no source can honestly date this document, so nothing is claimed"
        );

        let replaced_rows = store.progress_for_document(replaced_id).expect("read back");
        assert_eq!(replaced_rows.last().expect("a position").page, 12);
        assert_eq!(
            replaced_rows.last().expect("a position").read_at,
            Some(1_600_000_000),
            "the history's real time replaces the one the moved clock wrote"
        );
    }

    /// A history that dates a document in the FUTURE is not evidence about a reading, and because
    /// `read_at` is a high-water mark such a time would be kept forever: a document nobody has touched
    /// for a year would be stamped as read tomorrow, and the stale rule could never call it stale again.
    /// The sweep still records the position it can see — that half it can believe — and leaves the
    /// reading time alone, so a real, older time the store already knew survives the bad observation.
    #[test]
    fn the_sweep_ignores_a_reading_time_in_the_future() {
        let store = in_memory_store();
        let unknown = "/lib/Unknown.pdf";
        let known = "/lib/Known.pdf";
        let unknown_id = store.upsert_document(&document(unknown, 1, 1)).expect("insert");
        let known_id = store.upsert_document(&document(known, 1, 2)).expect("insert");
        // One document knows nothing about when it was read, the other knew a real, older time.
        store.record_progress(unknown_id, &position(11)).expect("record");
        store
            .record_progress(known_id, &position(11).with_read_at(Some(1_600_000_000)))
            .expect("record");

        // A machine whose clock moved, or a hand-edited file: everything after the current instant is
        // not a reading we can believe.
        let history = history_file(&[
            (unknown, "2999-01-01T00:00:00Z"),
            (known, "2999-01-01T00:00:00Z"),
        ]);
        let provider = FakeProvider::new("evince-gvfs", &["evince"])
            .answering(unknown, Answer::At(12))
            .answering(known, Answer::At(12));

        let mut report = TrackReport::default();
        sweep_with_history(&store, &[Box::new(provider)], Some(&history), &mut report);

        assert_eq!(report.recorded.len(), 2, "the positions are still recorded: {report:?}");

        let unknown_rows = store.progress_for_document(unknown_id).expect("read back");
        assert_eq!(unknown_rows.last().expect("a position").page, 12);
        assert_eq!(
            unknown_rows.last().expect("a position").read_at,
            None,
            "a time in the future is not a reading, so nothing is claimed"
        );

        let known_rows = store.progress_for_document(known_id).expect("read back");
        assert_eq!(known_rows.last().expect("a position").page, 12);
        assert_eq!(
            known_rows.last().expect("a position").read_at,
            Some(1_600_000_000),
            "a real time the store already knew is what remains"
        );
    }
}
