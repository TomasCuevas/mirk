//! The platform-facing traits: the public interface contributors implement.
//!
//! # The contract every implementation must honour
//!
//! These rules are not style preferences; the rest of `mirk` relies on them.
//!
//! 1. **Never panic.** A viewer changing its private file format, a file vanishing mid-read, or a
//!    sandbox denying access are all ordinary conditions. Return [`MirkError`]; an adapter that
//!    takes the process down with it is a bug in the adapter.
//! 2. **Never guess.** When state cannot be understood, return [`None`] or an error. A plausible
//!    wrong page number is far worse than an absent one, because the interface cannot tell the
//!    difference and will present it as fact.
//! 3. **Declare capability honestly.** [`ProgressProvider::capability`] is shown to the user before
//!    a document is opened, so overstating it produces a broken promise rather than a feature.
//! 4. **Be cheap.** These are called during a scan over potentially thousands of documents.
//!    An adapter that opens and parses a large file per call will make the application feel broken.
//! 5. **Stay read-only.** Providers observe other applications; they do not write to them.
//!
//! Every trait here is object-safe and `Send + Sync`, because the registry stores
//! `Box<dyn ProgressProvider>` and the scanning happens off the interface thread.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::domain::{Document, FileId, OpenDocument, Platform, RecentDocument};
use crate::error::MirkError;
use crate::progress::{ProgressCapability, StoredProgress};

/// Whether a viewer or backend is actually usable on this machine.
///
/// Distinct from [`ProgressCapability`]: a viewer can be installed but expose nothing useful
/// (a browser), or expose plenty but not be installed at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Availability {
    /// Present on this machine and usable.
    Installed,
    /// Not present. Not an error — most viewers are absent on most machines.
    Missing,
    /// Present, but this platform cannot use it. On macOS this is the common case, because reading
    /// another application's container requires Full Disk Access.
    Unsupported,
}

impl Availability {
    /// A stable machine-readable identifier.
    pub const fn id(self) -> &'static str {
        match self {
            Availability::Installed => "installed",
            Availability::Missing => "missing",
            Availability::Unsupported => "unsupported",
        }
    }
}

/// What happened when `mirk` tried to open a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchOutcome {
    /// Handed to an external application. `viewer` is the executable name or bundle identifier
    /// that was used.
    Delegated { viewer: String },
    /// Opened by `mirk`'s own reader.
    OpenedInternally,
}

/// Reads reading position out of an external viewer's stored state.
///
/// This is the extensibility point of the product: anyone can teach `mirk` about a viewer it does
/// not know yet by implementing this trait.
pub trait ProgressProvider: Send + Sync {
    /// Stable identifier, used as the persisted [`crate::ProgressSource::External`] value.
    ///
    /// Must not change between releases, because stored progress records reference it.
    fn id(&self) -> &'static str;

    /// The platform this provider is for.
    ///
    /// A provider is compiled only on its own platform, so this is largely descriptive — but it
    /// lets the registry report *why* a viewer is missing rather than treating an absent Windows
    /// viewer on Linux as a defect.
    fn platform(&self) -> Platform;

    /// Executable names, desktop-entry ids or bundle identifiers this provider claims.
    ///
    /// Used to map an open document's owning process to a provider, so it must be the name the
    /// OS reports — not a display name.
    fn viewer_ids(&self) -> &'static [&'static str];

    /// What this viewer can tell `mirk`, and therefore whether `mirk` can promise to track it.
    fn capability(&self) -> ProgressCapability;

    /// Whether the viewer is present and usable here.
    fn detect(&self) -> Availability;

    /// Reads the stored position for `doc`.
    ///
    /// Returns `Ok(None)` when the viewer is installed but has no record for this document, which
    /// is a normal answer and not an error. Also returns `Ok(None)` when the state exists but
    /// cannot be understood: see contract rule 2.
    fn read(&self, doc: &Document) -> Result<Option<StoredProgress>, MirkError>;

    /// Asks the viewer to open `doc` at `page`.
    ///
    /// `page` is **zero-based**, matching [`StoredProgress::page`]. Viewers whose command-line
    /// interface counts from one must convert here — this off-by-one is silent and has already
    /// cost real debugging time once in this project.
    fn resume(&self, doc: &Document, page: u32) -> Result<LaunchOutcome, MirkError>;
}

/// Axis A — the desktop's own list of recently opened documents.
///
/// Better than process inspection for the question "what did the user actually open?", because
/// the desktop maintains this list from real user actions rather than from file access.
pub trait RecentDocumentsSource: Send + Sync {
    /// Stable identifier for this source.
    fn id(&self) -> &'static str;

    /// The platform this source reads.
    fn platform(&self) -> Platform;

    /// The most recently used documents, newest first, at most `limit` entries.
    ///
    /// Entries whose file no longer exists should be omitted rather than returned with a dead
    /// path.
    fn recent(&self, limit: usize) -> Result<Vec<RecentDocument>, MirkError>;
}

/// Axis B — what is open right now.
///
/// Deliberately separate from [`RecentDocumentsSource`]: this answers "what is the user reading at
/// this moment", which is what makes a live progress indicator possible. It is heuristic on every
/// supported platform and its output is expected to contain noise.
pub trait OpenDocumentDetector: Send + Sync {
    /// Stable identifier for this detector.
    fn id(&self) -> &'static str;

    /// The platform this detector inspects.
    fn platform(&self) -> Platform;

    /// Documents currently open, in no particular order.
    fn open_documents(&self) -> Result<Vec<OpenDocument>, MirkError>;
}

/// Axis C — opening a document, optionally at a page.
pub trait Launcher: Send + Sync {
    /// Stable identifier for this launcher.
    fn id(&self) -> &'static str;

    /// The platform this launcher drives.
    fn platform(&self) -> Platform;

    /// Opens `path`, optionally at `page`.
    ///
    /// `page` is **zero-based**, as in [`ProgressProvider::resume`]. A launcher that cannot honour
    /// a page request must still open the document rather than failing: landing on page one is a
    /// degraded success, not an error.
    fn launch(&self, path: &Path, page: Option<u32>) -> Result<LaunchOutcome, MirkError>;
}

/// Portable file identity.
///
/// On POSIX this is `(device, inode)`; on Windows a volume serial number plus a file index. Some
/// filesystems — network shares, and FAT/exFAT in particular — report values that are not unique
/// or not stable, which is why implementors may fall back to hashing content.
pub trait FileIdentity: Send + Sync {
    /// Stable identifier for this implementation.
    fn id(&self) -> &'static str;

    /// Computes the identity of `path`.
    ///
    /// For [`FileId::ContentHash`] this reads the whole file, so implementations should prefer a
    /// native identifier and only hash when the native one is unusable.
    fn identity(&self, path: &Path) -> Result<FileId, MirkError>;
}

/// A folder to watch and whether its subdirectories are watched as well.
///
/// `path` is kept as a [`PathBuf`] on purpose: see contract rule 3 on [`FolderWatcher`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchTarget {
    /// The folder to watch.
    ///
    /// A watcher canonicalises this path when it starts, so a symlink target resolves to the real
    /// directory and events are reported under the same canonical path the scanner stores. The
    /// path is never rewritten for display — see contract rule 3 on [`FolderWatcher`].
    pub path: PathBuf,
    /// When true, every directory under `path` is watched too, *including directories that appear
    /// after the watch starts*. A subdirectory that was created while watching must never become
    /// invisible.
    pub recursive: bool,
}

/// What happened to a watched path.
///
/// The first three describe changes to a file in the tree. The last two describe the **watcher
/// itself**: they are not file changes, and a consumer must treat them as "my view of this path
/// may be wrong" rather than as an add, change or remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WatchEventKind {
    /// The path appeared in a watched folder.
    Added,
    /// The path was already there and a write to it completed.
    Changed,
    /// The path disappeared from a watched folder.
    Removed,
    /// Events for this path may have been dropped — for example the kernel's event queue
    /// overflowed. The consumer must rescan `path`; it must not assume its view is current.
    Lost,
    /// This path could not be watched, so changes inside it are invisible until it is watched
    /// again. Typically a directory that could not be read, or for which a watch could not be
    /// installed for a reason other than the kernel's hard watch limit.
    Unwatchable,
}

/// A settled change to a watched folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEvent {
    /// The full path the event concerns, exactly as the watcher composed it — never rewritten.
    pub path: PathBuf,
    /// What happened to `path`.
    pub kind: WatchEventKind,
}

/// Watches folders and reports files that have **settled**, meaning the writer has finished with
/// them.
///
/// This is what lets `mirk` notice new and updated PDFs without re-reading the whole library on a
/// timer. Unlike every other trait here it does not answer a question and return: [`watch`] runs a
/// blocking loop on the caller's own thread until the caller asks it to stop.
///
/// [`watch`]: FolderWatcher::watch
///
/// # The contract
///
/// 1. **Settled files only.** A file being copied is opened, written in many chunks and closed.
///    `Added`/`Changed` describe the *completed* write, never the chunks: reporting a half-copied
///    PDF would make the scanner parse it, fail, and leave a perfectly good document in the
///    library with no page count one second later.
/// 2. **Coalesced, not streamed.** A burst of events for the same path within a short window is
///    reported once. Copying one large PDF must not produce hundreds of events.
/// 3. **Paths are opaque.** Events carry [`PathBuf`], never `String`. Nothing is split, trimmed,
///    shell-quoted or Unicode-normalised on the way through — a path is bytes the filesystem
///    understands, not text for display.
/// 4. **Report your own blind spots.** Watching can fail quietly in ways no file event can
///    express: the kernel's event queue can overflow, and a corner of the tree may be unreadable
///    or unwatchable. The watcher reports those as [`WatchEventKind::Lost`] and
///    [`WatchEventKind::Unwatchable`] instead of staying silent, because a library that goes stale
///    without saying so is exactly the failure this project refuses everywhere else.
/// 5. **Honest failure.** If the operating system refuses a watch — the watch limit is exhausted,
///    for example — the call returns one error naming the limit. Continuing with a silently
///    partial watch would leave part of the tree permanently invisible, which is worse than
///    failing.
/// 6. **Never panic, never spin, no async.** The loop belongs on the caller's thread; it checks
///    `stop` regularly and sleeps between checks rather than busy-waiting.
///
/// # What a removal means
///
/// A `Removed` event is reported and nothing else happens. It deliberately does not delete anything
/// from the library: the reading history attached to a document is the reason `mirk` exists, and
/// representing "the file is gone" needs a `documents.status` column that does not exist yet.
/// Treating a removal as a deletion would destroy the very data the product protects.
pub trait FolderWatcher: Send + Sync {
    /// Stable identifier for this watcher implementation.
    fn id(&self) -> &'static str;

    /// The platform this watcher is for.
    fn platform(&self) -> Platform;

    /// Watches `targets`, calling `on_event` once per settled change, until `stop` is set.
    ///
    /// Runs on the calling thread and returns only when `stop` becomes true. Implementations must
    /// check `stop` regularly enough to return promptly, and must not panic: a folder that
    /// disappears mid-watch is an ordinary event, not a fatal error.
    ///
    /// `on_event` is called from this thread, so it must not block for long. Events already
    /// observed but not yet coalesced may be delivered before the call returns.
    ///
    /// Besides the three file changes, it may report the watcher's own blind spots as
    /// [`WatchEventKind::Lost`] and [`WatchEventKind::Unwatchable`]; see the contract above.
    fn watch(
        &self,
        targets: &[WatchTarget],
        stop: &AtomicBool,
        on_event: &mut dyn FnMut(WatchEvent),
    ) -> Result<(), MirkError>;
}
