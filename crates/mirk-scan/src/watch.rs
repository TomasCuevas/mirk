//! Wiring watcher events into the library (T7b).
//!
//! [`FolderWatcher`] says *what happened to a watched path*; [`Scanner`] knows *how to turn a PDF
//! into library rows*. This module is the piece between them: it consumes settled watcher events
//! and updates the library, reporting every outcome to one caller-supplied sink.
//!
//! # Injected, never coupled (S1)
//!
//! The watcher and the scanner both arrive by injection, so `mirk-scan` still does not depend on
//! `mirk-platform`: a fake watcher can drive every test, and the real inotify backend is used only
//! by whoever composes the application. There is no async runtime and no thread of this module's
//! own — [`WatchBridge::watch`] blocks on the caller's thread until the caller sets the stop flag,
//! exactly as [`FolderWatcher::watch`] does.
//!
//! # What each event does
//!
//! | Event | Reaction |
//! |---|---|
//! | [`WatchEventKind::Added`] / [`WatchEventKind::Changed`] | Count and persist the path through [`Scanner::count_and_persist`], so T6's rules apply unchanged — including `pages = None`, never `0`, for a file that cannot be parsed. |
//! | [`WatchEventKind::Removed`] | Report it and **nothing else**. The row is kept on purpose: reading history outlives the file, and representing absence needs a `documents.status` column that does not exist yet (L7). |
//! | [`WatchEventKind::Lost`] | Report it and rescan the path. Events were dropped, so the library's view may be stale, and a rescan is the recovery. |
//! | [`WatchEventKind::Unwatchable`] | Report it and **do not** rescan: a rescan cannot fix a path that cannot be watched. |
//!
//! # One sink, nothing swallowed (S5)
//!
//! Every one of those reactions becomes a [`WatchOutcome`] delivered to the sink, including the
//! ones with nothing to do. There are no silent branches: an application can log every event or
//! tell the user, and a library that goes stale without saying so is impossible by construction.
//!
//! # The caller scans first, then watches (S6)
//!
//! The watcher cannot report a change that happened before its watches were installed, so the
//! caller runs [`Scanner::scan`] once and only then calls [`WatchBridge::watch`]. This module does
//! not perform that initial scan; it reacts to changes.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use mirk_core::{FolderWatcher, MirkError, WatchEvent, WatchEventKind, WatchTarget};
use pdfium_render::prelude::Pdfium;

use crate::{CountedDocument, ScanFailure, Scanner};

/// One outcome of turning a watcher event into a library update.
///
/// Every event produces at least one outcome, and the [`Lost`](WatchOutcome::Lost) variant is
/// followed by the result of its rescan. A caller collecting these can log the whole story of a
/// watched folder without inspecting the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchOutcome {
    /// The path was counted and persisted.
    Counted(CountedDocument),
    /// The path could not be counted, or could not be persisted. The reason is human-readable and
    /// the store keeps `pages = None` rather than a fabricated `0`.
    Failed(ScanFailure),
    /// The path disappeared from a watched folder. Reported only: the library is deliberately left
    /// untouched so reading history survives the file.
    Removed(PathBuf),
    /// Events for the path may have been dropped. Reported, and the path was rescanned.
    Lost(PathBuf),
    /// The path could not be watched, so changes inside it are invisible. Reported, and **not**
    /// rescanned — a rescan cannot fix a path that cannot be watched.
    Unwatchable(PathBuf),
}

/// Turns settled watcher events into library updates.
///
/// Constructed from an injected [`FolderWatcher`] and [`Scanner`], both borrowed, so the bridge is
/// cheap and cannot outlive its collaborators. See the module documentation for the per-event
/// contract.
pub struct WatchBridge<'a> {
    watcher: &'a dyn FolderWatcher,
    scanner: Scanner<'a>,
}

impl<'a> WatchBridge<'a> {
    /// Builds a bridge over `watcher` that writes through `scanner`.
    pub fn new(watcher: &'a dyn FolderWatcher, scanner: Scanner<'a>) -> Self {
        Self { watcher, scanner }
    }

    /// Watches `targets`, processing each settled event until `stop` is set.
    ///
    /// Blocks on the calling thread and returns when the watcher's loop ends — normally because
    /// `stop` was set, or with the watcher's own error (for example a kernel watch limit, which
    /// T7a reports as one honest failure rather than a silently partial watch). The pdfium native
    /// library is bound once, before watching starts, so a missing or unloadable library is one
    /// actionable error instead of one identical failure per event.
    ///
    /// Every processed event produces a [`WatchOutcome`] delivered to `on_outcome`. The sink is
    /// called from this thread, so it should not block for long.
    pub fn watch(
        &self,
        targets: &[WatchTarget],
        stop: &AtomicBool,
        on_outcome: &mut dyn FnMut(WatchOutcome),
    ) -> Result<(), MirkError> {
        // Bind before the loop installs any watch: if page counting cannot run at all, the caller
        // gets one honest error rather than a stream of per-event failures, and no watch is left
        // running behind a broken wiring.
        let pdfium = self.scanner.pdfium()?;

        self.watcher.watch(targets, stop, &mut |event| {
            self.handle_event(event, &pdfium, on_outcome);
        })
    }

    /// Applies one event's reaction and reports every outcome it produces.
    fn handle_event(
        &self,
        event: WatchEvent,
        pdfium: &Pdfium,
        on_outcome: &mut dyn FnMut(WatchOutcome),
    ) {
        match event.kind {
            WatchEventKind::Added | WatchEventKind::Changed => {
                self.count_and_report(&event.path, pdfium, on_outcome);
            }
            WatchEventKind::Lost => {
                // The library's view of this path may be stale because events were dropped, so the
                // rescan is the recovery. Both the drop and the rescan's result reach the sink.
                on_outcome(WatchOutcome::Lost(event.path.clone()));
                self.count_and_report(&event.path, pdfium, on_outcome);
            }
            WatchEventKind::Removed => on_outcome(WatchOutcome::Removed(event.path)),
            // Deliberately no rescan: a path that cannot be watched does not become watchable by
            // reading it, and pretending otherwise would report a recovery that did not happen.
            WatchEventKind::Unwatchable => on_outcome(WatchOutcome::Unwatchable(event.path)),
        }
    }

    /// Counts and persists one path, reporting each counted document and failure to the sink.
    fn count_and_report(
        &self,
        path: &Path,
        pdfium: &Pdfium,
        on_outcome: &mut dyn FnMut(WatchOutcome),
    ) {
        let paths = [path.to_path_buf()];
        let outcome = self.scanner.count_and_persist_with(pdfium, &paths);

        for document in outcome.counted {
            on_outcome(WatchOutcome::Counted(document));
        }
        for failure in outcome.failures {
            on_outcome(WatchOutcome::Failed(failure));
        }
    }
}
