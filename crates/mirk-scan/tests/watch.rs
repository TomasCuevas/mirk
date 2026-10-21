//! T7b acceptance tests: turning watcher events into library updates.
//!
//! Every test works inside a `tempfile` directory, and a fake [`FolderWatcher`] emits scripted
//! events, so nothing here depends on real inotify timing or on the machine's speed. The
//! collaborators are injected exactly as they are in production: the fake watcher, a
//! [`Scanner`] with a deterministic fake identity, and the store.
//!
//! **These tests need the native pdfium library**, because proving a PDF is counted means parsing
//! a real one. Run `scripts/fetch-pdfium.sh` once per clone (or set `MIRK_PDFIUM_LIB`) before
//! `cargo test`, exactly as `tests/scanner.rs` requires.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mirk_core::{
    Confidence, FileId, FileIdentity, FolderWatcher, MirkError, Platform, ProgressSource,
    StoredProgress, WatchEvent, WatchEventKind, WatchTarget,
};
use mirk_scan::{CountedDocument, Scanner, WatchBridge, WatchOutcome};
use mirk_store::Store;

/// The R2 fixture: three page objects inside a FlateDecode-compressed object stream.
const OBJECT_STREAM_FIXTURE: &[u8] = include_bytes!("fixtures/pages-in-object-stream.pdf");
/// A classic-xref PDF with four pages, `Title: A Known Title` and `Author: A Known Author`.
const INFO_FIXTURE: &[u8] = include_bytes!("fixtures/with-info-metadata.pdf");

/// A deterministic identity derived from the path bytes, never from inode numbers.
///
/// The wiring cannot tell how identity is produced — that is the point of the injected
/// [`FileIdentity`] — so the tests drive it with this fake instead of `mirk-platform`.
struct FakeIdentity;

impl FileIdentity for FakeIdentity {
    fn id(&self) -> &'static str {
        "fake-path-hash"
    }

    fn identity(&self, path: &Path) -> Result<FileId, MirkError> {
        Ok(FileId::ContentHash(format!(
            "fnv1a64:{:016x}",
            fnv1a64(path.as_os_str().as_encoded_bytes())
        )))
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// A watcher that emits a scripted list of events instead of touching the filesystem.
///
/// It never observes a real folder, which is the whole point: the wiring is tested against the
/// `FolderWatcher` contract, not against inotify. When `block_until_stopped` is set it behaves
/// like a real watcher after delivering its events — it polls the caller's stop flag until the
/// caller sets it — so a test can prove the loop ends on that flag.
struct FakeWatcher {
    events: Vec<WatchEvent>,
    block_until_stopped: bool,
}

impl FakeWatcher {
    fn new(events: Vec<WatchEvent>) -> Self {
        Self {
            events,
            block_until_stopped: false,
        }
    }

    fn blocking(mut self) -> Self {
        self.block_until_stopped = true;
        self
    }
}

impl FolderWatcher for FakeWatcher {
    fn id(&self) -> &'static str {
        "fake-watcher"
    }

    fn platform(&self) -> Platform {
        Platform::Linux
    }

    fn watch(
        &self,
        _targets: &[WatchTarget],
        stop: &AtomicBool,
        on_event: &mut dyn FnMut(WatchEvent),
    ) -> Result<(), MirkError> {
        for event in &self.events {
            on_event(event.clone());
        }

        if self.block_until_stopped {
            // A real watcher blocks until the caller stops it. The deadline only exists so a bug
            // in the wiring fails the assertion instead of hanging the suite.
            let deadline = Instant::now() + Duration::from_secs(10);
            while !stop.load(Ordering::SeqCst) {
                if Instant::now() > deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        Ok(())
    }
}

fn temp_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path().join("library");
    std::fs::create_dir(&root).expect("create library dir");
    (dir, root)
}

fn temp_store(dir: &tempfile::TempDir) -> Store {
    Store::open(dir.path().join("mirk.sqlite3")).expect("open store")
}

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directory");
    }
    std::fs::write(path, bytes).expect("write file");
}

fn event(path: &Path, kind: WatchEventKind) -> WatchEvent {
    WatchEvent {
        path: path.to_path_buf(),
        kind,
    }
}

fn target(root: &Path) -> WatchTarget {
    WatchTarget {
        path: root.to_path_buf(),
        recursive: true,
    }
}

/// Drives the wiring once, collecting every outcome into `outcomes`.
fn run_bridge<'a>(
    watcher: &'a dyn FolderWatcher,
    store: &'a Store,
    identity: &'a dyn FileIdentity,
    root: &Path,
    stop: &AtomicBool,
    outcomes: &mut Vec<WatchOutcome>,
) -> Result<(), MirkError> {
    let bridge = WatchBridge::new(watcher, Scanner::new(store, identity));
    let mut sink = |outcome: WatchOutcome| outcomes.push(outcome);
    bridge.watch(&[target(root)], stop, &mut sink)
}

/// A short, stable label for an outcome, so a test can assert the exact sequence.
fn describe(outcome: &WatchOutcome) -> String {
    match outcome {
        WatchOutcome::Counted(document) => format!("counted:{}", document.path.display()),
        WatchOutcome::Failed(failure) => format!("failed:{}", failure.path.display()),
        WatchOutcome::Removed(path) => format!("removed:{}", path.display()),
        WatchOutcome::Lost(path) => format!("lost:{}", path.display()),
        WatchOutcome::Unwatchable(path) => format!("unwatchable:{}", path.display()),
    }
}

// --- S2: Added/Changed reach the library through T6's path ----------------------------------

#[test]
fn an_added_pdf_is_counted_and_stored_with_its_real_page_count() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    let path = root.join("new.pdf");
    write_file(&path, OBJECT_STREAM_FIXTURE);

    let watcher = FakeWatcher::new(vec![event(&path, WatchEventKind::Added)]);
    let mut outcomes = Vec::new();
    run_bridge(
        &watcher,
        &store,
        &identity,
        &root,
        &AtomicBool::new(false),
        &mut outcomes,
    )
    .expect("the wiring runs to completion");

    assert_eq!(
        outcomes,
        vec![WatchOutcome::Counted(CountedDocument {
            path: path.clone(),
            pages: 3,
            title: None,
            author: None,
        })],
        "an added PDF is reported as counted, once",
    );

    let stored = store
        .find_document_by_identity(&identity.identity(&path).expect("fake identity never fails"))
        .expect("lookup succeeds")
        .expect("the added PDF is stored");
    assert_eq!(stored.document.pages, Some(3), "the real page count is stored");
}

#[test]
fn a_changed_file_is_updated_in_place_and_never_duplicated() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    let path = root.join("doc.pdf");
    write_file(&path, INFO_FIXTURE);

    let first = FakeWatcher::new(vec![event(&path, WatchEventKind::Added)]);
    let mut outcomes = Vec::new();
    run_bridge(
        &first,
        &store,
        &identity,
        &root,
        &AtomicBool::new(false),
        &mut outcomes,
    )
    .expect("the first run completes");
    assert_eq!(store.document_count().expect("count documents"), 1);

    // The file is rewritten with a different page count, then reported as changed.
    write_file(&path, OBJECT_STREAM_FIXTURE);
    let second = FakeWatcher::new(vec![event(&path, WatchEventKind::Changed)]);
    let mut outcomes = Vec::new();
    run_bridge(
        &second,
        &store,
        &identity,
        &root,
        &AtomicBool::new(false),
        &mut outcomes,
    )
    .expect("the second run completes");

    assert_eq!(
        store.document_count().expect("count documents"),
        1,
        "a change must update the existing row, not add a second one",
    );
    let stored = store
        .find_document_by_identity(&identity.identity(&path).expect("fake identity never fails"))
        .expect("lookup succeeds")
        .expect("the document is still stored");
    assert_eq!(stored.document.pages, Some(3), "the new count replaced the old one");
}

#[test]
fn an_unreadable_added_file_is_reported_and_keeps_pages_unknown() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    let path = root.join("broken.pdf");
    write_file(&path, b"this file is named .pdf but is plain text\n");

    let watcher = FakeWatcher::new(vec![event(&path, WatchEventKind::Added)]);
    let mut outcomes = Vec::new();
    run_bridge(
        &watcher,
        &store,
        &identity,
        &root,
        &AtomicBool::new(false),
        &mut outcomes,
    )
    .expect("the wiring runs to completion");

    let WatchOutcome::Failed(failure) = &outcomes[0] else {
        panic!("expected a failure outcome, got {:?}", outcomes[0]);
    };
    assert_eq!(failure.path, path);
    assert!(!failure.reason.is_empty(), "a failure carries a reason");

    let stored = store
        .find_document_by_identity(&identity.identity(&path).expect("fake identity never fails"))
        .expect("lookup succeeds")
        .expect("a discovered PDF is stored even when it cannot be read");
    assert_eq!(stored.document.pages, None, "unknown pages stay None");
    assert_ne!(stored.document.pages, Some(0), "unknown pages must never become 0");
}

// --- S3: Removed reports and changes nothing -------------------------------------------------

#[test]
fn a_removed_file_is_reported_and_the_library_is_left_untouched() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    let path = root.join("counted.pdf");
    write_file(&path, INFO_FIXTURE);

    // The document is in the library, with reading progress that must outlive the file.
    let first = FakeWatcher::new(vec![event(&path, WatchEventKind::Added)]);
    let mut outcomes = Vec::new();
    run_bridge(
        &first,
        &store,
        &identity,
        &root,
        &AtomicBool::new(false),
        &mut outcomes,
    )
    .expect("the document is counted first");
    let record = store
        .find_document_by_identity(&identity.identity(&path).expect("fake identity never fails"))
        .expect("lookup succeeds")
        .expect("the document is stored");
    assert_eq!(record.document.pages, Some(4));
    store
        .record_progress(
            record.id,
            &StoredProgress::new(2, Some(4), ProgressSource::Manual, Confidence::Manual),
        )
        .expect("record progress");

    // The file disappears from the watched folder.
    let second = FakeWatcher::new(vec![event(&path, WatchEventKind::Removed)]);
    let mut outcomes = Vec::new();
    run_bridge(
        &second,
        &store,
        &identity,
        &root,
        &AtomicBool::new(false),
        &mut outcomes,
    )
    .expect("the removal run completes");

    assert_eq!(
        outcomes,
        vec![WatchOutcome::Removed(path.clone())],
        "a removal is reported and nothing else happens",
    );

    // The store is unchanged: the row, its page count and its progress all survive.
    assert_eq!(store.document_count().expect("count documents"), 1);
    let after = store
        .find_document_by_identity(&identity.identity(&path).expect("fake identity never fails"))
        .expect("lookup succeeds")
        .expect("the removed file's row is deliberately kept");
    assert_eq!(after.document.path, path);
    assert_eq!(after.document.pages, Some(4));
    let progress = store.progress_for_document(after.id).expect("read progress");
    assert_eq!(progress.len(), 1, "the reading history survives the removal");
    assert_eq!(progress[0].page, 2);
    assert_eq!(progress[0].total, Some(4));
}

// --- S4: the watcher's own blind spots -------------------------------------------------------

#[test]
fn a_lost_event_rescans_the_reported_path() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    let path = root.join("never-added.pdf");
    write_file(&path, OBJECT_STREAM_FIXTURE);

    // The file was never reported as added — its events were dropped. The `Lost` event is the
    // recovery: rescan the path and the document appears.
    let watcher = FakeWatcher::new(vec![event(&path, WatchEventKind::Lost)]);
    let mut outcomes = Vec::new();
    run_bridge(
        &watcher,
        &store,
        &identity,
        &root,
        &AtomicBool::new(false),
        &mut outcomes,
    )
    .expect("the wiring runs to completion");

    assert_eq!(
        outcomes,
        vec![
            WatchOutcome::Lost(path.clone()),
            WatchOutcome::Counted(CountedDocument {
                path: path.clone(),
                pages: 3,
                title: None,
                author: None,
            }),
        ],
        "a lost path is reported and then rescanned",
    );

    let stored = store
        .find_document_by_identity(&identity.identity(&path).expect("fake identity never fails"))
        .expect("lookup succeeds")
        .expect("the lost path was rescanned and stored");
    assert_eq!(stored.document.pages, Some(3));
}

#[test]
fn an_unwatchable_path_is_reported_and_never_scanned() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    let path = root.join("cannot-watch.pdf");
    write_file(&path, OBJECT_STREAM_FIXTURE);

    let watcher = FakeWatcher::new(vec![event(&path, WatchEventKind::Unwatchable)]);
    let mut outcomes = Vec::new();
    run_bridge(
        &watcher,
        &store,
        &identity,
        &root,
        &AtomicBool::new(false),
        &mut outcomes,
    )
    .expect("the wiring runs to completion");

    assert_eq!(
        outcomes,
        vec![WatchOutcome::Unwatchable(path.clone())],
        "an unwatchable path is reported and nothing else is attempted",
    );
    assert!(
        store
            .find_document_by_identity(&identity.identity(&path).expect("fake identity never fails"))
            .expect("lookup succeeds")
            .is_none(),
        "rescanning cannot fix an unwatchable path, so it is not scanned",
    );
    assert_eq!(store.document_count().expect("count documents"), 0);
}

// --- S5/S6: a single sink, and the loop ends on the stop flag --------------------------------

#[test]
fn every_outcome_reaches_the_sink_exactly_once() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;

    let added = root.join("added.pdf");
    let broken = root.join("broken.pdf");
    let removed = root.join("removed.pdf");
    let lost = root.join("lost.pdf");
    let unwatchable = root.join("unwatchable.pdf");
    write_file(&added, OBJECT_STREAM_FIXTURE);
    write_file(&broken, b"not a pdf at all");
    write_file(&lost, OBJECT_STREAM_FIXTURE);
    write_file(&unwatchable, OBJECT_STREAM_FIXTURE);

    let watcher = FakeWatcher::new(vec![
        event(&added, WatchEventKind::Added),
        event(&broken, WatchEventKind::Added),
        event(&removed, WatchEventKind::Removed),
        event(&lost, WatchEventKind::Lost),
        event(&unwatchable, WatchEventKind::Unwatchable),
    ]);
    let mut outcomes = Vec::new();
    run_bridge(
        &watcher,
        &store,
        &identity,
        &root,
        &AtomicBool::new(false),
        &mut outcomes,
    )
    .expect("the wiring runs to completion");

    let seen: Vec<String> = outcomes.iter().map(describe).collect();
    assert_eq!(
        seen,
        vec![
            format!("counted:{}", added.display()),
            format!("failed:{}", broken.display()),
            format!("removed:{}", removed.display()),
            format!("lost:{}", lost.display()),
            format!("counted:{}", lost.display()),
            format!("unwatchable:{}", unwatchable.display()),
        ],
        "every outcome reaches the sink, including the ones with nothing to do",
    );

    // The failure carried a reason, not just a path.
    let WatchOutcome::Failed(failure) = &outcomes[1] else {
        panic!("expected a failure outcome, got {:?}", outcomes[1]);
    };
    assert!(!failure.reason.is_empty());

    // Nothing was scanned for the removal or the unwatchable path.
    for path in [&removed, &unwatchable] {
        assert!(
            store
                .find_document_by_identity(&identity.identity(path).expect("fake identity never fails"))
                .expect("lookup succeeds")
                .is_none(),
            "{} must not have been stored",
            path.display(),
        );
    }
}

#[test]
fn setting_the_stop_flag_ends_the_watch_and_returns() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    let path = root.join("doc.pdf");
    write_file(&path, OBJECT_STREAM_FIXTURE);

    let watcher = FakeWatcher::new(vec![event(&path, WatchEventKind::Added)]).blocking();
    let stop = AtomicBool::new(false);
    let mut outcomes = Vec::new();
    let bridge = WatchBridge::new(&watcher, Scanner::new(&store, &identity));
    let mut sink = |outcome: WatchOutcome| {
        outcomes.push(outcome);
        stop.store(true, Ordering::SeqCst);
    };

    bridge
        .watch(&[target(&root)], &stop, &mut sink)
        .expect("the loop returns once the stop flag is set");

    assert!(stop.load(Ordering::SeqCst), "the stop flag reached the watcher");
    assert_eq!(outcomes.len(), 1, "the one scripted event was handled before the loop ended");
}
