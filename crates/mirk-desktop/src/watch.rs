//! Watches the reader's folders while the application runs (S2–S4 of `odd/tasks/live-watch.md`).
//!
//! # Why this module exists
//!
//! `mirk-scan` carries a complete watcher bridge and `mirk-platform` a real recursive inotify
//! backend, and both are tested. Neither was ever constructed here, so a folder was read exactly
//! once — at the moment the reader added it — and a PDF that appeared afterwards was invisible
//! until they removed the watch and lost the reading positions with it. The defect was a missing
//! wire in the composition root, which is the one place no unit test can see.
//!
//! # Two threads, two jobs
//!
//! [`WatchBridge::watch`] blocks until its stop flag is set, by contract. So the *comparison* runs
//! on the caller's thread ([`REARM_INTERVAL`], cheap, it only reads the stored folders) and the
//! *watch* runs on a thread of its own ([`Armed`]). Rebuilding the watch is therefore a
//! stop-and-start of one thread rather than a restart of the process, which is what makes adding or
//! removing a folder take effect while the application keeps running (S4).
//!
//! # What each event does
//!
//! Nothing is decided here: every event goes through [`WatchBridge`], which applies the scanner's own
//! rules — `pages = None` and never a fabricated `0` for a file that cannot be parsed, a removed file
//! reported without deleting its history, a dropped-event notice triggering a rescan. This module
//! only owns the lifetime and the reporting.
//!
//! # The gap a watch cannot close
//!
//! A watch reports changes that happen *after* its watches are installed, and nothing else: the
//! bridge's own documentation states the rule ("the caller scans first, then watches", S6) and the
//! reader found the price of skipping it — "agregué un nuevo PDF ... y no aparece". A document that
//! arrived while the application was closed was therefore invisible forever, because the watch had
//! never heard of it and re-scanning a covered folder is refused on purpose.
//!
//! [`catch_up`] closes it without re-reading the library: [`unseen`] walks the same trees with
//! [`Scanner::discover`], which needs no pdfium, and keeps only the paths the store does not already
//! hold. Only those are parsed, so a launch over a library of ninety documents costs ninety
//! directory entries and one page count instead of ninety page counts.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use mirk_core::{MirkError, WatchTarget};
use mirk_scan::{Scanner, WatchBridge, WatchOutcome};
use mirk_store::{Store, WatchedFolder};

/// How often the armed watch is compared with the folders the store holds, in seconds.
///
/// The comparison is one indexed read, so the interval is chosen for how fast a folder the reader
/// just added should start being watched — not for the cost of a scan. Five seconds is invisible
/// next to the fifteen the listing takes to re-read itself (S5).
pub const REARM_INTERVAL: Duration = Duration::from_secs(5);

/// The trees `folders` asks the watcher for.
///
/// Every enabled watch, recursive when the watch itself is: a PDF dropped into a subfolder of a
/// recursive watch is the same reader gesture as one dropped at the root, and the inotify backend
/// already watches directories created after the watch starts. A disabled watch is not watched,
/// because the store keeps it for another purpose than reading it now.
pub fn targets(folders: &[WatchedFolder]) -> Vec<WatchTarget> {
    folders
        .iter()
        .filter(|folder| folder.enabled)
        .map(|folder| WatchTarget {
            path: folder.path.clone(),
            recursive: folder.recursive,
        })
        .collect()
}

/// Whether an armed set no longer matches the folders the store holds.
///
/// Compared as a set, not as a sequence: the store orders folders by their id, and a removal that
/// leaves the same trees in another order changes nothing the watcher cares about. A change of
/// `recursive` on the same path is a different set, because it changes what the watcher must report.
pub fn is_stale(armed: &[WatchTarget], current: &[WatchTarget]) -> bool {
    fn signature(targets: &[WatchTarget]) -> Vec<(&Path, bool)> {
        let mut rows: Vec<(&Path, bool)> = targets
            .iter()
            .map(|target| (target.path.as_path(), target.recursive))
            .collect();
        rows.sort_unstable();
        rows
    }

    signature(armed) != signature(current)
}

/// The documents in the enabled folders that the library does not already hold.
///
/// This is the catch-up pass's whole question, and it is answered by walking the folders rather
/// than by reading them: [`Scanner::discover`] lists PDFs without binding pdfium, so a folder holding
/// a hundred documents costs one directory walk, not a hundred page counts. The comparison is by
/// path, which is what the store resolves a document by.
///
/// A folder that is no longer there is one honest line and nothing else: a vanished folder must not
/// fail the pass, and it must not make the pass look empty either. Symlinks and unreadable
/// directories discovered inside a folder are reported the way the rest of this module reports —
/// they are the only trace a folder that contributed nothing leaves.
pub fn unseen(store: &Store, folders: &[WatchedFolder]) -> Result<Vec<PathBuf>, MirkError> {
    let known: HashSet<PathBuf> = store
        .library()?
        .into_iter()
        .map(|entry| entry.document.document.path)
        .collect();

    let backends = mirk_platform::backends();
    let scanner = Scanner::new(store, backends.file_identity.as_ref());
    let mut found: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for folder in folders.iter().filter(|folder| folder.enabled) {
        if !folder.path.is_dir() {
            eprintln!(
                "catch-up: {} is not there any more; nothing to catch up in it",
                folder.path.display()
            );
            continue;
        }

        let discovery = match scanner.discover(&folder.path) {
            Ok(discovery) => discovery,
            Err(error) => {
                // One unusable folder is not a reason to abandon the others: the reader's documents
                // are spread over every folder they watch.
                eprintln!(
                    "catch-up: {} could not be listed: {error}",
                    folder.path.display()
                );
                continue;
            }
        };

        for failure in &discovery.failures {
            eprintln!(
                "catch-up: {} could not be listed: {}",
                failure.path.display(),
                failure.reason
            );
        }
        for path in &discovery.skipped {
            eprintln!(
                "catch-up: {} is a symlink and is not followed",
                path.display()
            );
        }

        for path in discovery.pdfs {
            // Nested watched folders discover the same file twice; counting it twice would report
            // the same document twice for one arrival.
            if known.contains(&path) || !seen.insert(path.clone()) {
                continue;
            }
            found.push(path);
        }
    }

    Ok(found)
}

/// Counts and persists the documents that arrived before the watch was installed.
///
/// Run right after the watch is armed, never before: a document created while this pass runs is
/// then reported by the watch instead of falling into the gap between the two. Only the paths
/// [`unseen`] found are parsed, and [`Scanner::count_and_persist`] binds pdfium for that batch and
/// lets it go, which is the rule this module documents — a library held alive would block every
/// other pdfium caller in the process.
pub fn catch_up(store: &Store, folders: &[WatchedFolder]) {
    let fresh = match unseen(store, folders) {
        Ok(fresh) => fresh,
        Err(error) => {
            eprintln!("catch-up: the library could not be read: {error}");
            return;
        }
    };

    let mut entered = 0;
    if !fresh.is_empty() {
        let backends = mirk_platform::backends();
        let scanner = Scanner::new(store, backends.file_identity.as_ref());

        match scanner.count_and_persist(&fresh) {
            Ok(outcome) => {
                for counted in &outcome.counted {
                    eprintln!("catch-up: {} entered the library", counted.path.display());
                }
                for failure in &outcome.failures {
                    eprintln!(
                        "catch-up: a document could not be counted: {}: {}",
                        failure.path.display(),
                        failure.reason
                    );
                }
                entered = outcome.counted.len();
            }
            Err(error) => {
                // One line for the whole batch, because the only error here is one failure of the
                // whole pass: pdfium could not be loaded.
                eprintln!("catch-up: the documents found at launch could not be counted: {error}");
            }
        }
    }

    // Always stated, including the zero: this feature already failed twice by being silent, so the
    // trace of a launch that caught nothing up is worth one line.
    eprintln!(
        "catch-up: {entered} new document(s) entered the library from {} folder(s)",
        folders.iter().filter(|folder| folder.enabled).count()
    );
}

/// One running watch: a thread blocked in [`WatchBridge::watch`], and the flag that ends it.
///
/// The store is owned by the watch thread, so the library the events write to cannot be closed under
/// it by a caller that forgot to stop the watch first: stopping joins, and joining is what drops the
/// store.
pub struct Armed {
    stop: Arc<AtomicBool>,
    targets: Vec<WatchTarget>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Armed {
    /// Starts watching `targets`, writing through `store`.
    ///
    /// A thread that cannot be spawned is reported and leaves the library as it was: the reader's
    /// application keeps running (S6), and the next comparison retries.
    pub fn start(store: Store, targets: Vec<WatchTarget>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let armed = targets.clone();
        let thread = std::thread::Builder::new()
            .name("mirk-watch".to_string())
            .spawn(move || watch_until_stopped(store, &armed, &flag));

        match thread {
            Ok(thread) => Self {
                stop,
                targets,
                thread: Some(thread),
            },
            Err(error) => {
                eprintln!("the watcher thread could not start: {error}");
                Self {
                    stop,
                    targets,
                    thread: None,
                }
            }
        }
    }

    /// The trees this watch was armed for.
    pub fn targets(&self) -> &[WatchTarget] {
        &self.targets
    }

    /// Ends the watch and waits for it, so the store it owns is closed before this returns.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            // A panicking watcher thread is reported by the join, not re-raised here: the caller is
            // rebuilding the watch, and a panic in the old one must not take the application down.
            if thread.join().is_err() {
                eprintln!("the watcher thread ended unexpectedly");
            }
        }
    }
}

impl Drop for Armed {
    fn drop(&mut self) {
        // Best effort: a caller that drops the armed watch without stopping it still ends the loop.
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Runs one watch until `stop` is set. This is the watch thread's whole body.
///
/// The backends are built here rather than passed in, so the thread owns everything it touches. A
/// bridge that cannot start — a missing pdfium, an exhausted inotify limit, a folder that no longer
/// exists — is one honest line on stderr, and the caller's next comparison tries again.
fn watch_until_stopped(store: Store, targets: &[WatchTarget], stop: &AtomicBool) {
    let backends = mirk_platform::backends();
    let scanner = Scanner::new(&store, backends.file_identity.as_ref());
    let bridge = WatchBridge::new(backends.folder_watcher.as_ref(), scanner);
    let mut report = |outcome: WatchOutcome| report_outcome(&outcome);

    if let Err(error) = bridge.watch(targets, stop, &mut report) {
        eprintln!(
            "watching {} folder(s) failed: {error}",
            targets.len()
        );
    }
}

/// Writes one event's outcome to stderr.
///
/// There is no screen for the watcher, so stderr is where its story goes — the same place the
/// ingestion loop reports what it cannot do. A counted document is logged because it is the one
/// outcome that changes what the reader sees, and its absence from stderr is the only trace a
/// silently unwired watcher leaves.
fn report_outcome(outcome: &WatchOutcome) {
    match outcome {
        WatchOutcome::Counted(counted) => {
            eprintln!("watched: {} entered the library", counted.path.display());
        }
        WatchOutcome::Failed(failure) => {
            eprintln!("watched: a document could not be counted: {failure:?}");
        }
        WatchOutcome::Removed(path) => {
            eprintln!(
                "watched: {} disappeared; its history is kept",
                path.display()
            );
        }
        WatchOutcome::Lost(path) => {
            eprintln!("watched: events were dropped for {}; rescanned", path.display());
        }
        WatchOutcome::Unwatchable(path) => {
            eprintln!("watched: {} cannot be watched", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    /// A directory the test owns, removed when it goes out of scope.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "mirk-live-watch-{label}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create the temporary directory");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// The repository's own committed PDF, so the count is real rather than a fake page number.
    const PDF: &[u8] = include_bytes!("../../mirk-scan/tests/fixtures/with-info-metadata.pdf");

    /// How long the test waits for the watch thread to install its inotify watches.
    ///
    /// `WatchBridge::watch` installs them on its own thread and offers no hook, exactly as
    /// `mirk-platform`'s own tests document. This bounds the startup race; everything after it is
    /// event-driven with a timeout.
    const WATCH_STARTUP_WAIT: Duration = Duration::from_millis(500);

    fn folder(id: i64, path: &str, recursive: bool) -> WatchedFolder {
        WatchedFolder {
            id,
            path: PathBuf::from(path),
            recursive,
            enabled: true,
        }
    }

    #[test]
    fn the_targets_are_the_enabled_folders_and_keep_their_recursion() {
        let mut disabled = folder(3, "/library/hidden", true);
        disabled.enabled = false;
        let folders = vec![
            folder(1, "/library", true),
            folder(2, "/papers", false),
            disabled,
        ];

        let targets = targets(&folders);

        assert_eq!(
            targets,
            vec![
                WatchTarget {
                    path: PathBuf::from("/library"),
                    recursive: true,
                },
                WatchTarget {
                    path: PathBuf::from("/papers"),
                    recursive: false,
                },
            ],
            "a disabled watch is not watched, and recursion travels with the folder"
        );
    }

    #[test]
    fn the_armed_set_is_stale_when_a_folder_is_added_or_removed() {
        let armed = vec![
            WatchTarget {
                path: PathBuf::from("/library"),
                recursive: true,
            },
            WatchTarget {
                path: PathBuf::from("/papers"),
                recursive: false,
            },
        ];

        assert!(
            !is_stale(&armed, &armed),
            "the same set in the same order is not a rebuild"
        );

        let mut reversed = armed.clone();
        reversed.reverse();
        assert!(
            !is_stale(&armed, &reversed),
            "the set is compared as a set: order alone is not a change"
        );

        let mut added = armed.clone();
        added.push(WatchTarget {
            path: PathBuf::from("/magazines"),
            recursive: true,
        });
        assert!(
            is_stale(&armed, &added),
            "a folder added while the application runs must be watched"
        );

        let removed = vec![armed[0].clone()];
        assert!(
            is_stale(&armed, &removed),
            "a folder the reader stopped watching must stop being watched"
        );

        let narrowed = vec![
            WatchTarget {
                path: PathBuf::from("/library"),
                recursive: false,
            },
            armed[1].clone(),
        ];
        assert!(
            is_stale(&armed, &narrowed),
            "recursion is part of what the watcher is armed for"
        );
    }

    #[test]
    fn a_document_that_arrived_while_the_application_was_closed_enters_the_library_on_the_next_launch()
    {
        // The reader's case, exactly: the folder was already recorded, the document arrived while
        // the application was not running, and the watch never heard of it.
        let directory = TempDir::new("catch-up-arrival");
        let library = directory.path.join("library");
        std::fs::create_dir_all(&library).expect("create the watched folder");
        std::fs::write(library.join("Cosmos.pdf"), PDF).expect("write the document that was there");
        std::fs::write(library.join("The Demon-Haunted World.pdf"), PDF)
            .expect("write the document that arrived while the application was closed");
        let database = directory.path.join("mirk.sqlite3");

        let store = Store::open(&database).expect("open the store");
        store
            .record_folder(&library, true)
            .expect("record the watched folder");
        let folders = store.folders().expect("the stored folders");

        // Nothing was scanned at launch: both documents are new to the store.
        let fresh = unseen(&store, &folders).expect("list what the library does not hold");
        assert_eq!(fresh.len(), 2, "neither document is known yet");

        catch_up(&store, &folders);

        let listed = store.library().expect("read the library");
        let arrived = listed
            .iter()
            .find(|entry| {
                entry
                    .document
                    .document
                    .path
                    .ends_with("The Demon-Haunted World.pdf")
            })
            .expect("the document that arrived while the application was closed must enter the library");
        assert_eq!(
            arrived.document.document.pages,
            Some(4),
            "the catch-up pass counts real pages, not a stand-in"
        );
    }

    #[test]
    fn the_catch_up_pass_counts_nothing_twice() {
        let directory = TempDir::new("catch-up-once");
        let library = directory.path.join("library");
        std::fs::create_dir_all(&library).expect("create the watched folder");
        let document = library.join("Arrival.pdf");
        std::fs::write(&document, PDF).expect("write the document");
        let database = directory.path.join("mirk.sqlite3");

        let store = Store::open(&database).expect("open the store");
        store
            .record_folder(&library, true)
            .expect("record the watched folder");
        let folders = store.folders().expect("the stored folders");

        catch_up(&store, &folders);
        catch_up(&store, &folders);

        let listed = store.library().expect("read the library");
        let rows = listed
            .iter()
            .filter(|entry| entry.document.document.path == document)
            .count();
        assert_eq!(rows, 1, "a second pass must not add a second row for one path");

        let fresh = unseen(&store, &folders).expect("list what the library does not hold");
        assert!(
            fresh.is_empty(),
            "once a document is in the library the pass finds nothing: {fresh:?}"
        );
    }

    #[test]
    fn the_catch_up_ignores_folders_that_are_gone() {
        let directory = TempDir::new("catch-up-gone");
        let library = directory.path.join("library");
        std::fs::create_dir_all(&library).expect("create the watched folder");
        std::fs::write(library.join("Kept.pdf"), PDF).expect("write the document");
        let database = directory.path.join("mirk.sqlite3");

        let store = Store::open(&database).expect("open the store");
        store
            .record_folder(&library, true)
            .expect("record the watched folder");
        store
            .record_folder(directory.path.join("unplugged-drive"), true)
            .expect("record a folder that is not there");
        let folders = store.folders().expect("the stored folders");
        assert_eq!(folders.len(), 2, "the fixture recorded two folders");

        // Neither listing nor counting may fail because one folder is unplugged.
        let fresh = unseen(&store, &folders).expect("a vanished folder must not fail the pass");
        catch_up(&store, &folders);

        let listed = store.library().expect("read the library");
        assert_eq!(
            listed.len(),
            1,
            "the folder that is still there is caught up, and nothing is removed"
        );
        assert!(
            listed[0]
                .document
                .document
                .path
                .ends_with("Kept.pdf"),
            "the surviving document is the one in the folder that exists"
        );
        assert!(
            fresh
                .iter()
                .all(|path| path.ends_with("Kept.pdf")),
            "the vanished folder contributes no paths: {fresh:?}"
        );
    }

    #[test]
    fn a_pdf_written_after_the_watch_starts_enters_the_library() {
        let directory = TempDir::new("arrival");
        let library = directory.path.join("library");
        std::fs::create_dir_all(&library).expect("create the watched folder");
        let database = directory.path.join("mirk.sqlite3");

        let store = Store::open(&database).expect("open the store");
        store
            .record_folder(&library, true)
            .expect("record the watched folder");
        let targets = targets(&store.folders().expect("the stored folders"));
        assert_eq!(targets.len(), 1, "the fixture recorded one folder");

        let armed = Armed::start(store, targets);
        // The watch is installed on its own thread; this is the bounded startup wait.
        std::thread::sleep(WATCH_STARTUP_WAIT);

        // The reader's case, exactly: the file did not exist when the folder was added.
        std::fs::write(library.join("New Arrival.pdf"), PDF).expect("write the new document");

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut entered = false;
        while Instant::now() < deadline {
            // A second connection is the reader's own view of the library: the assertion is about
            // what the store says, not about what the watcher believes it did.
            let probe = Store::open(&database).expect("reopen the store");
            let listed = probe.library().expect("read the library");
            if listed
                .iter()
                .any(|entry| entry.document.document.path.ends_with("New Arrival.pdf"))
            {
                entered = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }

        armed.stop();

        assert!(
            entered,
            "a PDF written after the watch started must enter the library, or the reader is back to \
             removing and re-adding the folder"
        );
    }
}
