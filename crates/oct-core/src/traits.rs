//! The platform-facing traits: the public interface contributors implement.
//!
//! # The contract every implementation must honour
//!
//! These rules are not style preferences; the rest of `oct` relies on them.
//!
//! 1. **Never panic.** A viewer changing its private file format, a file vanishing mid-read, or a
//!    sandbox denying access are all ordinary conditions. Return [`OctError`]; an adapter that
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

use std::path::Path;

use crate::domain::{Document, FileId, OpenDocument, Platform, RecentDocument};
use crate::error::OctError;
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

/// What happened when `oct` tried to open a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchOutcome {
    /// Handed to an external application. `viewer` is the executable name or bundle identifier
    /// that was used.
    Delegated { viewer: String },
    /// Opened by `oct`'s own reader.
    OpenedInternally,
}

/// Reads reading position out of an external viewer's stored state.
///
/// This is the extensibility point of the product: anyone can teach `oct` about a viewer it does
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

    /// What this viewer can tell `oct`, and therefore whether `oct` can promise to track it.
    fn capability(&self) -> ProgressCapability;

    /// Whether the viewer is present and usable here.
    fn detect(&self) -> Availability;

    /// Reads the stored position for `doc`.
    ///
    /// Returns `Ok(None)` when the viewer is installed but has no record for this document, which
    /// is a normal answer and not an error. Also returns `Ok(None)` when the state exists but
    /// cannot be understood: see contract rule 2.
    fn read(&self, doc: &Document) -> Result<Option<StoredProgress>, OctError>;

    /// Asks the viewer to open `doc` at `page`.
    ///
    /// `page` is **zero-based**, matching [`StoredProgress::page`]. Viewers whose command-line
    /// interface counts from one must convert here — this off-by-one is silent and has already
    /// cost real debugging time once in this project.
    fn resume(&self, doc: &Document, page: u32) -> Result<LaunchOutcome, OctError>;
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
    fn recent(&self, limit: usize) -> Result<Vec<RecentDocument>, OctError>;
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
    fn open_documents(&self) -> Result<Vec<OpenDocument>, OctError>;
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
    fn launch(&self, path: &Path, page: Option<u32>) -> Result<LaunchOutcome, OctError>;
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
    fn identity(&self, path: &Path) -> Result<FileId, OctError>;
}
