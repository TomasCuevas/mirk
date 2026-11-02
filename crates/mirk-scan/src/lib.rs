//! Watched-folder scanning for `mirk` (T6).
//!
//! A scan turns a folder the user nominated into rows in [`mirk_store::Store`]: it walks the folder
//! recursively, finds PDFs, counts their pages with a real parser, reads their metadata, and
//! persists each one. It is **read-only over the PDFs** — nothing here modifies a document.
//!
//! # Real page counting, not a byte scan
//!
//! Page counts come from [`pdfium`], never from counting `/Type /Page` in the file's bytes. That
//! naive method returns `0` for a document whose page objects live in a FlateDecode-compressed
//! object stream — a normal, increasingly common shape — which is risk **R2**. The acceptance test
//! asserts the real count *and* that the naive method is wrong on the same fixture, so the fixture
//! cannot quietly stop proving anything.
//!
//! `pdfium` is a native library loaded at runtime, not linked at build time, so it is fetched rather
//! than committed: `scripts/fetch-pdfium.sh` populates `vendor/pdfium/lib` (gitignored, ~8 MB per
//! platform). **The test suite requires it**, because proving real page counting means parsing real
//! PDFs; a missing library fails the tests with an actionable message instead of passing without
//! checking anything.
//!
//! # An installed application carries the library inside its own package
//!
//! `vendor/pdfium` and `$MIRK_PDFIUM_LIB` are development facts: an installed application has
//! neither, so resolving a page count from those alone would leave a released build unable to count
//! a single page. The desktop crate therefore declares the library as a bundle resource and hands
//! this crate the resource directory it landed in ([`set_packaged_pdfium_directory`]); the packaged
//! location is one more candidate in [`resolve_pdfium_library_with_packaged`], ranked between the
//! explicit override and the development checkout. Development is unchanged: when no packaged copy
//! exists, resolution falls through to `vendor/pdfium/lib` exactly as before, so a developer sets
//! nothing.
//!
//! Every entry point that parses loads it first, so a library that cannot be found or loaded fails
//! the whole call with one actionable [`MirkError::ComponentUnavailable`] (see
//! [`resolve_pdfium_library`]) rather than producing one identical failure per document or, worse, a
//! page count of `0`. Discovery is the exception on purpose: walking a folder is filesystem work and
//! must not need a native parser.
//!
//! # Unknown is `None`, never `0`
//!
//! A PDF that cannot be parsed (corrupt, encrypted, unreadable, vanished mid-scan) appears in the
//! report as a [`ScanFailure`] with its path and reason, and its stored `pages` stays `None`. `0`
//! would be indistinguishable from a real page count and would render as "0 % read" — a wrong
//! fact rather than an absent one. A discovered PDF is still stored so the library knows it exists.
//!
//! # Filesystem robustness
//!
//! The watched root is canonicalised; symlinks found *inside* it are **not** followed, so a symlink
//! loop cannot hang the scan. They are listed as skipped rather than dropped silently, because a
//! folder that contributes nothing has to be explainable. A permission-denied directory, an entry
//! that cannot be read, and a file that disappears between discovery and parsing are all reported
//! rather than fatal. The walk is iterative, so a deep tree cannot exhaust the stack.
//!
//! # Identity is the provider's business
//!
//! A failure to determine a document's identity does not fail the document: it is counted and stored
//! without one, and the store resolves it by path. The limitation that follows is worth stating — a
//! document that *moves* while its identity cannot be determined is stored as a new row, because
//! identity is the only thing that lets the library follow a move.
//!
//! # Paths are opaque
//!
//! Paths are never split, trimmed, shell-quoted or Unicode-normalised (risk **R7**). Spaces,
//! accents and multi-byte UTF-8 survive byte-for-byte into the store.
//!
//! # Layering (S7)
//!
//! `mirk-scan` depends only on [`mirk_core`], [`mirk_store`] and the PDF parser — **not** on
//! `mirk-platform`. The [`mirk_core::FileIdentity`] provider is injected, so a scan can run with a
//! deterministic fake and the crate stays testable without a filesystem backend.
//!
//! # What is deliberately deferred
//!
//! Size/mtime-based incremental rescan and the metadata columns it needs (`size`, `mtime`,
//! `discovered_via`, `platform`, `status`); the application that consumes [`WatchOutcome`]s and
//! decides what to show the user; the suggestion queue for PDFs
//! found outside watched folders; and scanning a folder the user has not nominated. Each is a
//! separate work unit, not an omission here.

use std::path::{Path, PathBuf};

use mirk_core::{Document, FileIdentity, MirkError};
use mirk_store::Store;
use pdfium_render::prelude::{PdfDocumentMetadataTagType, Pdfium};

mod cover;
mod watch;

pub use cover::{cover_for, CoverOutcome, COVER_WIDTH};
pub use watch::{WatchBridge, WatchOutcome};

/// The remedies named by every pdfium failure, so the error is actionable on its own and a caller
/// never has to guess what to install.
///
/// The third remedy is the one an end user of a packaged build can act on: `scripts/fetch-pdfium.sh`
/// and `$MIRK_PDFIUM_LIB` only mean something to someone running from a checkout.
const PDFIUM_REMEDIES: &str = "fetch it with scripts/fetch-pdfium.sh, set MIRK_PDFIUM_LIB to the \
     library file or its directory, or reinstall the application if this is a packaged build";

/// Where the pdfium native library should be loaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfiumLibrary {
    /// A specific library file, for example `libpdfium.so` or `pdfium.dll`.
    File(PathBuf),
    /// A directory containing the platform's library file, for example `vendor/pdfium/lib`.
    Directory(PathBuf),
    /// The system library path — the last resort when neither an explicit path nor the vendor
    /// directory is available.
    System,
}

/// Resolves which pdfium library to load when there is no packaged copy of it — the development
/// chain, unchanged.
///
/// This is [`resolve_pdfium_library_with_packaged`] with the packaged candidate absent, which is
/// exactly what a checkout has: no bundle, no resource directory, nothing installed to carry the
/// library. It is kept as its own entry point because development resolution is the case a person
/// running `cargo test` needs, and because the development contract — `$MIRK_PDFIUM_LIB`, then
/// `vendor/pdfium/lib`, then the system path — is worth being able to name without mentioning
/// packaging at all.
///
/// `explicit` is the value of `$MIRK_PDFIUM_LIB`; the caller reads the environment and passes it in,
/// so this function reads no process state and can be tested without mutating it (an earlier review
/// in this project flagged in-process environment mutation as a race). `vendor` is the directory
/// `scripts/fetch-pdfium.sh` populates.
pub fn resolve_pdfium_library(
    explicit: Option<&Path>,
    vendor: &Path,
) -> Result<PdfiumLibrary, MirkError> {
    resolve_pdfium_library_with_packaged(explicit, None, vendor)
}

/// Resolves which pdfium library to load from every candidate the caller knows about.
///
/// Pure, like [`resolve_pdfium_library`]: the candidates arrive as arguments, so nothing here reads
/// the environment, the process's resource directory or any other process state, and a test can
/// drive every branch without mutating a shared cell.
///
/// Resolution order, and why it is this order:
///
/// 1. **`explicit`** — an existing file or directory wins outright. It is the caller's deliberate
///    override, so it outranks everything a default would pick. A path that does **not** exist is
///    an error rather than a fallback, even when a lower candidate would have worked: the caller
///    asked for that library specifically, and quietly ignoring a stale setting would hide the
///    misconfiguration behind a library they did not think they were using.
/// 2. **`packaged`** — the directory an installed application carries the library in. It outranks
///    the development checkout because a released build must load the copy that shipped with it,
///    not one that happens to sit in whatever directory it was started from; and it is
///    `Option<&Path>` because the location only exists in an installed app, so a caller outside one
///    passes `None`.
/// 3. **`vendor`** — `vendor/pdfium/lib`, what `scripts/fetch-pdfium.sh` populates. This is the
///    development path: it is chosen whenever the first two candidates are absent, which is what
///    keeps a developer from having to set anything.
/// 4. **the system library path** — the last resort, for a machine that installed pdfium itself.
///
/// A candidate that does not exist is skipped, never silently substituted: the returned
/// [`PdfiumLibrary`] names the place the library will be loaded from, so a load failure can be
/// traced back to the exact directory [`bind_pdfium`] was given.
pub fn resolve_pdfium_library_with_packaged(
    explicit: Option<&Path>,
    packaged: Option<&Path>,
    vendor: &Path,
) -> Result<PdfiumLibrary, MirkError> {
    if let Some(path) = explicit {
        if path.is_file() {
            return Ok(PdfiumLibrary::File(path.to_path_buf()));
        }
        if path.is_dir() {
            return Ok(PdfiumLibrary::Directory(path.to_path_buf()));
        }
        return Err(pdfium_unavailable(format!(
            "the configured MIRK_PDFIUM_LIB path does not exist: {}",
            path.display()
        )));
    }

    if let Some(directory) = packaged {
        if directory.is_dir() {
            return Ok(PdfiumLibrary::Directory(directory.to_path_buf()));
        }
    }

    if vendor.is_dir() {
        Ok(PdfiumLibrary::Directory(vendor.to_path_buf()))
    } else {
        Ok(PdfiumLibrary::System)
    }
}

/// The single, actionable error reported when the pdfium library cannot be found or loaded.
///
/// It carries the underlying cause, so "the library is missing" and "the library is the wrong
/// architecture" are different messages rather than one shrug. It is a *component* failure, not
/// [`MirkError::NotSupported`]: page counting is implemented on every platform `mirk` targets, and it
/// is the native library that is absent.
fn pdfium_unavailable(cause: impl std::fmt::Display) -> MirkError {
    MirkError::ComponentUnavailable {
        component: "pdfium",
        message: format!("{cause}; {PDFIUM_REMEDIES}"),
    }
}

/// The repository-local pdfium directory, `vendor/pdfium/lib`.
///
/// Resolved from `CARGO_MANIFEST_DIR` rather than the current working directory: `cargo test` runs
/// with the crate directory as its working directory, so a cwd-relative path would not find it.
fn vendor_pdfium_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/pdfium/lib")
}

/// The directory an installed application keeps the bundled pdfium in, as reported by the desktop
/// crate at startup.
///
/// This is process state on purpose, and it lives here rather than in
/// [`resolve_pdfium_library_with_packaged`] so that the resolver stays pure. The only way to learn
/// where a package put its resources is to ask the packaging layer, which happens once in
/// `mirk-desktop`'s setup through Tauri's `PathResolver::resource_dir()`; every scan, watch event and
/// cover render then resolves against the same answer instead of reaching for a window handle it
/// does not have.
///
/// `None` when nothing was recorded — a test, a library user that is not the desktop app, or a
/// development run where the resource directory holds no bundled library. `None` means "no packaged
/// candidate", not "pdfium is missing": resolution falls through to the vendor directory and then
/// the system path exactly as it did before packaging existed.
static PACKAGED_PDFIUM_DIRECTORY: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

/// Records where the installed application keeps its bundled pdfium, for every later binding.
///
/// Called once from the desktop crate's setup, with the directory the bundle's resources were copied
/// to (see `bundle.resources` in `tauri.conf.json`). Passing `None` — or never calling this — leaves
/// resolution on the development chain, so a test or a non-desktop consumer is unaffected.
///
/// A second call is ignored rather than fatal: the value describes one installed layout for the life
/// of the process, and a caller that sets it twice has nothing to correct.
pub fn set_packaged_pdfium_directory(directory: Option<PathBuf>) {
    let _ = PACKAGED_PDFIUM_DIRECTORY.set(directory);
}

/// The packaged directory recorded by [`set_packaged_pdfium_directory`], if any.
fn packaged_pdfium_directory() -> Option<PathBuf> {
    PACKAGED_PDFIUM_DIRECTORY.get().cloned().flatten()
}

/// Loads and initializes the pdfium library described by `library`.
///
/// Every failure — a wrong path, a wrong architecture, a corrupt file — collapses to the same
/// actionable [`pdfium_unavailable`] error: the underlying `pdfium-render` message is not a
/// `&'static str` and the caller cannot act on it beyond the two remedies anyway.
fn bind_pdfium(library: &PdfiumLibrary) -> Result<Pdfium, MirkError> {
    let bindings = match library {
        PdfiumLibrary::File(path) => Pdfium::bind_to_library(path),
        PdfiumLibrary::Directory(directory) => {
            Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(directory))
        }
        PdfiumLibrary::System => Pdfium::bind_to_system_library(),
    };

    bindings
        .map(Pdfium::new)
        .map_err(|error| pdfium_unavailable(format!("{error:?}")))
}

/// Binds the pdfium native library the same way every scan does.
///
/// The cover path (R32) runs outside a scan pass — a card asks for a cover on demand — but it must
/// resolve the library by exactly the same rules as page counting, so this reuses
/// [`resolve_pdfium_library`] and the vendor directory instead of growing a second discovery path.
/// Tests call it too, so a cover test and a scan test cannot disagree about where pdfium comes from.
///
/// A library that cannot be found or loaded is one actionable [`MirkError::ComponentUnavailable`],
/// exactly as [`Scanner::scan`] reports it.
pub fn bind_pdfium_library() -> Result<Pdfium, MirkError> {
    bind_pdfium_specifying(
        std::env::var_os("MIRK_PDFIUM_LIB")
            .map(PathBuf::from)
            .as_deref(),
    )
}

/// [`bind_pdfium_library`]'s testable core: the explicit library path is a parameter rather than
/// process state, so resolution can be exercised without mutating the environment.
///
/// The packaged candidate is read from [`PACKAGED_PDFIUM_DIRECTORY`] here rather than being passed
/// through every caller, so a packaged application resolves from its own resources without any
/// scan, watch or cover call site having to know that packaging exists.
fn bind_pdfium_specifying(explicit: Option<&Path>) -> Result<Pdfium, MirkError> {
    let packaged = packaged_pdfium_directory();
    let library = resolve_pdfium_library_with_packaged(
        explicit,
        packaged.as_deref(),
        &vendor_pdfium_directory(),
    )?;
    bind_pdfium(&library)
}

/// A PDF whose page count and metadata were read and persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountedDocument {
    /// Absolute path of the document.
    pub path: PathBuf,
    /// Real page count, from the PDF parser. Never `0` — an uncountable document is not counted.
    pub pages: u32,
    /// The Info dictionary's `Title`, when it exists.
    pub title: Option<String>,
    /// The Info dictionary's `Author`, when it exists.
    pub author: Option<String>,
}

/// A path the scan could not turn into a counted document, and why.
///
/// A failure is never fatal: it is reported and the scan continues. When the path names a
/// discovered PDF, the document is still stored, with `pages` left `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanFailure {
    /// The path that failed. For a directory this is the directory that could not be read.
    pub path: PathBuf,
    /// A human-readable, non-fatal description.
    pub reason: String,
}

/// The counted documents and failures produced by inspecting a set of paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanOutcome {
    /// Documents whose page count and metadata were read and persisted.
    pub counted: Vec<CountedDocument>,
    /// Paths that could not be counted, or could not be persisted.
    pub failures: Vec<ScanFailure>,
}

/// The PDFs found under a watched root, before any of them is parsed.
///
/// Discovery and parsing are separate so that "the file vanished between the two" is an ordinary
/// reported failure rather than a special case inside the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    /// The canonicalised watched root.
    pub root: PathBuf,
    /// PDF-extension files found, sorted by path. Includes files that may fail to parse later.
    pub pdfs: Vec<PathBuf>,
    /// Paths deliberately **not** followed: symlinks found inside the watched root. Reported so a
    /// symlinked folder that contributed nothing can be explained, instead of being invisible.
    pub skipped: Vec<PathBuf>,
    /// Directories and entries that could not be read, with why.
    pub failures: Vec<ScanFailure>,
}

/// The outcome of scanning one watched folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanReport {
    /// The canonicalised watched root.
    pub root: PathBuf,
    /// Every PDF-extension path discovered, sorted by path.
    pub discovered: Vec<PathBuf>,
    /// PDFs whose page count and metadata were read and persisted.
    pub counted: Vec<CountedDocument>,
    /// Paths deliberately not followed (symlinks inside the root).
    pub skipped: Vec<PathBuf>,
    /// Unreadable directories plus PDFs that could not be counted or persisted.
    pub failures: Vec<ScanFailure>,
}

impl ScanReport {
    /// How many PDFs were discovered.
    pub fn discovered_count(&self) -> usize {
        self.discovered.len()
    }

    /// How many PDFs were counted and persisted.
    pub fn counted_count(&self) -> usize {
        self.counted.len()
    }

    /// How many failures were reported. A failure does not abort the scan.
    pub fn failure_count(&self) -> usize {
        self.failures.len()
    }

    /// How many paths were deliberately not followed.
    pub fn skipped_count(&self) -> usize {
        self.skipped.len()
    }
}

/// Scans watched folders into a [`Store`], using an injected [`FileIdentity`].
///
/// The scanner borrows both collaborators, so it is cheap to construct and cannot outlive them.
/// The pdfium native library is bound per public call rather than held for the scanner's lifetime,
/// so a process can safely hold several scanners and the native library is released as soon as a
/// call returns.
pub struct Scanner<'a> {
    store: &'a Store,
    identity: &'a dyn FileIdentity,
    /// The value of `$MIRK_PDFIUM_LIB`, read once when the scanner is built and passed to the pure
    /// resolver. `None` means "not configured".
    explicit_pdfium: Option<PathBuf>,
}

impl<'a> Scanner<'a> {
    /// Builds a scanner over `store`, deriving document identity from `identity`.
    ///
    /// The pdfium library location is read from `$MIRK_PDFIUM_LIB` here, once, rather than inside
    /// the resolver; tests can then drive resolution without mutating the process environment.
    pub fn new(store: &'a Store, identity: &'a dyn FileIdentity) -> Self {
        Self::with_pdfium_library(
            store,
            identity,
            std::env::var_os("MIRK_PDFIUM_LIB").map(PathBuf::from),
        )
    }

    /// Builds a scanner that resolves pdfium from `explicit` instead of `$MIRK_PDFIUM_LIB`.
    ///
    /// An explicit path that does not exist is a hard error, not a fallback, so a caller can prove
    /// the missing-library path without mutating process state.
    pub fn with_pdfium_library(
        store: &'a Store,
        identity: &'a dyn FileIdentity,
        explicit: Option<PathBuf>,
    ) -> Self {
        Self {
            store,
            identity,
            explicit_pdfium: explicit,
        }
    }

    /// Resolves and binds the pdfium native library for this scanner.
    ///
    /// Crate-visible so [`WatchBridge`] can probe the library before it starts watching, turning a
    /// missing library into one actionable error instead of one failure per event. The instance this
    /// returns must not outlive the work it was bound for: pdfium is single-threaded behind a
    /// process-wide lock that is held while any instance is alive, so a binding that stayed alive
    /// would block every other pdfium caller in the process (see `watch`'s module documentation).
    pub(crate) fn pdfium(&self) -> Result<Pdfium, MirkError> {
        bind_pdfium_specifying(self.explicit_pdfium.as_deref())
    }

    /// Records `folder`, walks it, counts and persists every PDF found, and returns a report.
    ///
    /// The folder is canonicalised before it is recorded, and recorded idempotently, so scanning
    /// the same folder twice leaves one `folders` row and the same documents. Returns an error only
    /// when the pdfium library cannot be loaded, the folder itself cannot be used (it does not
    /// exist, or is not a directory) or the store cannot be written; every per-file problem is a
    /// [`ScanFailure`] in the report.
    pub fn scan(&self, folder: impl AsRef<Path>) -> Result<ScanReport, MirkError> {
        // Bind once for the whole scan, so a missing or unloadable library is reported before the
        // folder is recorded and before any document is inspected.
        let pdfium = self.pdfium()?;
        let discovery = self.discover_with(folder)?;
        self.store.record_folder(&discovery.root, true)?;

        let outcome = self.count_and_persist_with(&pdfium, &discovery.pdfs);
        let mut failures = discovery.failures;
        failures.extend(outcome.failures);

        Ok(ScanReport {
            root: discovery.root,
            discovered: discovery.pdfs,
            counted: outcome.counted,
            skipped: discovery.skipped,
            failures,
        })
    }

    /// Walks `folder` and returns the PDFs under it, without parsing or persisting anything.
    ///
    /// This is pure filesystem work and deliberately does **not** need the pdfium library: listing
    /// what is in a folder has to keep working when page counting cannot run at all. The root is
    /// canonicalised and symlinks inside it are not followed; unreadable directories are collected
    /// as failures rather than aborting the walk. [`Scanner::scan`] binds the library before it
    /// walks, so a missing library is still one actionable error rather than a folder recorded for
    /// nothing.
    pub fn discover(&self, folder: impl AsRef<Path>) -> Result<Discovery, MirkError> {
        self.discover_with(folder)
    }

    /// The filesystem walk behind [`Scanner::discover`], with no library check.
    fn discover_with(&self, folder: impl AsRef<Path>) -> Result<Discovery, MirkError> {
        let root = folder.as_ref().canonicalize()?;
        if !root.is_dir() {
            return Err(MirkError::InvalidPath(format!(
                "the watched folder is not a directory: {}",
                root.display()
            )));
        }

        let mut pdfs = Vec::new();
        let mut skipped = Vec::new();
        let mut failures = Vec::new();
        let mut pending = vec![root.clone()];

        while let Some(directory) = pending.pop() {
            let entries = match std::fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) => {
                    failures.push(ScanFailure {
                        path: directory,
                        reason: format!("cannot read the directory: {error}"),
                    });
                    continue;
                }
            };

            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        failures.push(ScanFailure {
                            path: directory.clone(),
                            reason: format!("cannot read a directory entry: {error}"),
                        });
                        continue;
                    }
                };

                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(error) => {
                        failures.push(ScanFailure {
                            path: entry.path(),
                            reason: format!("cannot read the entry's type: {error}"),
                        });
                        continue;
                    }
                };

                let path = entry.path();

                // Symlinks found inside the watched root are never followed: the root itself was
                // canonicalised, and following a link back up the tree would loop forever. They are
                // reported rather than silently dropped, so a folder that contributed nothing is
                // explainable instead of invisible.
                if file_type.is_symlink() {
                    skipped.push(path);
                    continue;
                }

                if file_type.is_dir() {
                    pending.push(path);
                } else if file_type.is_file() {
                    match extension_of(&path) {
                        Extension::Pdf => pdfs.push(path),
                        // A name this store cannot represent is reported rather than skipped: a file
                        // that vanishes from the report is indistinguishable from a file that is not
                        // there, which is the failure mode this project exists to avoid.
                        Extension::Undecodable => failures.push(ScanFailure {
                            path,
                            reason:
                                "the file name is not valid UTF-8, so its path cannot be stored"
                                    .to_string(),
                        }),
                        Extension::Other => {}
                    }
                }
            }
        }

        pdfs.sort();
        skipped.sort();
        Ok(Discovery {
            root,
            pdfs,
            skipped,
            failures,
        })
    }

    /// Counts and persists exactly `paths`, skipping discovery entirely.
    ///
    /// Exposed because discovery and parsing are separate steps in reality as well as here: a file
    /// can be discovered and then disappear before it is read. Callers that only have a known list
    /// of files (a retry, a single-file refresh) can use this directly. Per-path problems are
    /// reported in the outcome; the only error returned is the one actionable failure of the whole
    /// batch — pdfium could not be loaded, which would otherwise be one identical failure per path.
    pub fn count_and_persist(&self, paths: &[PathBuf]) -> Result<ScanOutcome, MirkError> {
        let pdfium = self.pdfium()?;
        Ok(self.count_and_persist_with(&pdfium, paths))
    }

    /// The per-path loop behind [`Scanner::count_and_persist`], with pdfium already loaded.
    ///
    /// Crate-visible so [`WatchBridge`] can bind the library for one event, call this with that
    /// binding, and release it before the watcher waits for the next event: a binding kept across
    /// events would hold pdfium's process-wide lock for as long as the watch ran.
    pub(crate) fn count_and_persist_with(&self, pdfium: &Pdfium, paths: &[PathBuf]) -> ScanOutcome {
        let mut outcome = ScanOutcome::default();

        for path in paths {
            // Identity first: it is cheap on POSIX, and a failure to determine it is not fatal.
            // Without one the document is still resolved by path, which the store supports.
            let identity = self.identity.identity(path).ok();

            match inspect(pdfium, path) {
                Ok(inspection) => {
                    let mut document = Document::new(path.clone());
                    document.identity = identity;
                    document.pages = Some(inspection.pages);
                    document.title = inspection.title.clone();
                    document.author = inspection.author.clone();

                    match self.store.upsert_document(&document) {
                        Ok(_) => outcome.counted.push(CountedDocument {
                            path: path.clone(),
                            pages: inspection.pages,
                            title: inspection.title,
                            author: inspection.author,
                        }),
                        Err(error) => outcome.failures.push(ScanFailure {
                            path: path.clone(),
                            reason: format!("cannot persist the document: {error}"),
                        }),
                    }
                }
                Err(reason) => {
                    // The document exists but cannot be counted. Store it with an unknown page
                    // count so the library still knows about it — and never with 0.
                    let mut document = Document::new(path.clone());
                    document.identity = identity;
                    if let Err(error) = self.store.upsert_document(&document) {
                        outcome.failures.push(ScanFailure {
                            path: path.clone(),
                            reason: format!("cannot persist the document: {error}"),
                        });
                        continue;
                    }
                    outcome.failures.push(ScanFailure {
                        path: path.clone(),
                        reason,
                    });
                }
            }
        }

        outcome
    }
}

/// What a single readable PDF contributes.
struct Inspection {
    pages: u32,
    title: Option<String>,
    author: Option<String>,
}

/// Reads the page count and metadata of one PDF, or explains why it cannot.
///
/// pdfium refuses a document that is encrypted and given no password, so an encrypted PDF never
/// reaches the page count: it is reported as a failure instead. A document that loads but reports
/// no pages is a failure too, never a `0`-page book.
fn inspect(pdfium: &Pdfium, path: &Path) -> Result<Inspection, String> {
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|error| format!("cannot parse the PDF: {error}"))?;

    let pages = document.pages().len();
    if pages == 0 {
        // A "PDF" that parses but has no pages cannot be read; it is a failure, not a 0-page book.
        return Err("the PDF contains no pages".to_string());
    }

    let metadata = document.metadata();
    let title = metadata
        .get(PdfDocumentMetadataTagType::Title)
        .map(|tag| tag.value().to_string());
    let author = metadata
        .get(PdfDocumentMetadataTagType::Author)
        .map(|tag| tag.value().to_string());

    Ok(Inspection {
        pages: u32::from(pages),
        title,
        author,
    })
}

/// What a path's extension says about the file.
enum Extension {
    /// A PDF, by extension (case-insensitive).
    Pdf,
    /// Not a PDF, or no extension at all.
    Other,
    /// The extension is present but not valid UTF-8, so the path cannot be stored in the first
    /// place. Distinguished from [`Extension::Other`] because this one must be *reported*.
    Undecodable,
}

/// Classifies a path by its extension.
fn extension_of(path: &Path) -> Extension {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("pdf") => Extension::Pdf,
        Some(_) => Extension::Other,
        None if path.extension().is_some() => Extension::Undecodable,
        None => Extension::Other,
    }
}

#[cfg(test)]
mod tests {
    //! S5: the packaged candidate is a fact of resolution, so it is tested on the pure resolver —
    //! no process state, no native library, no Tauri application. These fixtures are directories
    //! that *look like* a library directory; nothing here loads pdfium, and nothing here needs the
    //! ~8 MB file to be present. The development chain is covered by `tests/scanner.rs`, which stays
    //! untouched: a test that had to change to accommodate packaging would be the warning sign.

    use super::{resolve_pdfium_library, resolve_pdfium_library_with_packaged};
    use crate::PdfiumLibrary;
    use mirk_core::MirkError;
    use std::path::PathBuf;

    /// A temporary directory holding one library-shaped directory per test.
    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("create a temporary directory");
            let root = dir.path().to_path_buf();
            Self { _dir: dir, root }
        }

        /// A directory that exists, named `name`.
        fn directory(&self, name: &str) -> PathBuf {
            let path = self.root.join(name);
            std::fs::create_dir_all(&path).expect("create the fixture directory");
            path
        }

        /// A directory that deliberately does not exist.
        fn absent(&self, name: &str) -> PathBuf {
            self.root.join(name)
        }
    }

    #[test]
    fn chooses_the_packaged_directory_when_it_is_the_only_candidate_that_exists() {
        let fixture = Fixture::new();
        let packaged = fixture.directory("resources/pdfium");
        let vendor = fixture.absent("vendor/pdfium/lib");

        let resolved =
            resolve_pdfium_library_with_packaged(None, Some(packaged.as_path()), &vendor)
                .expect("a packaged directory resolves without a development checkout");
        assert_eq!(
            resolved,
            PdfiumLibrary::Directory(packaged),
            "an installed application must load the copy that shipped with it"
        );
    }

    #[test]
    fn the_packaged_directory_outranks_the_development_checkout() {
        let fixture = Fixture::new();
        let packaged = fixture.directory("resources/pdfium");
        let vendor = fixture.directory("vendor/pdfium/lib");

        let resolved =
            resolve_pdfium_library_with_packaged(None, Some(packaged.as_path()), &vendor)
                .expect("both candidates exist, so resolution succeeds");
        assert_eq!(
            resolved,
            PdfiumLibrary::Directory(packaged),
            "a released build must not prefer a vendor directory that happens to sit next to it"
        );
    }

    #[test]
    fn an_explicit_library_still_wins_over_the_packaged_directory() {
        let fixture = Fixture::new();
        let packaged = fixture.directory("resources/pdfium");
        let vendor = fixture.directory("vendor/pdfium/lib");
        let library = fixture.root.join("chosen/libpdfium.so");
        std::fs::create_dir_all(library.parent().expect("the library has a parent"))
            .expect("create the library directory");
        std::fs::write(&library, b"a stand-in; resolution never loads it")
            .expect("write the stand-in library");

        let resolved =
            resolve_pdfium_library_with_packaged(Some(&library), Some(packaged.as_path()), &vendor)
                .expect("an existing explicit file resolves");
        assert_eq!(
            resolved,
            PdfiumLibrary::File(library),
            "$MIRK_PDFIUM_LIB is the caller's deliberate override and outranks the bundle"
        );
    }

    #[test]
    fn a_missing_explicit_library_is_an_error_that_names_the_remedy() {
        let fixture = Fixture::new();
        // A packaged copy *does* exist: the error must still be raised, because falling through
        // here would silently ignore a setting the caller believes is in force.
        let packaged = fixture.directory("resources/pdfium");
        let vendor = fixture.directory("vendor/pdfium/lib");
        let stale = fixture.absent("stale/libpdfium.so");

        let error =
            resolve_pdfium_library_with_packaged(Some(&stale), Some(packaged.as_path()), &vendor)
                .expect_err("a stale explicit path is an error, not a fallback");
        let MirkError::ComponentUnavailable { component, message } = error else {
            panic!("expected MirkError::ComponentUnavailable, got {error:?}");
        };
        assert_eq!(component, "pdfium");
        assert!(
            message.contains("does not exist"),
            "the error names the cause: {message}"
        );
        assert!(
            message.contains("fetch-pdfium.sh"),
            "names the fetch script: {message}"
        );
        assert!(
            message.contains("MIRK_PDFIUM_LIB"),
            "names the environment variable: {message}"
        );
        assert!(
            message.contains("reinstall"),
            "names the remedy an end user of a packaged build can act on: {message}"
        );
    }

    #[test]
    fn the_development_chain_is_unchanged_when_there_is_no_packaged_copy() {
        let fixture = Fixture::new();
        let absent_packaged = fixture.absent("resources/pdfium");
        let vendor = fixture.directory("vendor/pdfium/lib");

        // Both entry points agree, so packaging cannot have changed the developer's path.
        let packaged_aware =
            resolve_pdfium_library_with_packaged(None, Some(absent_packaged.as_path()), &vendor)
                .expect("the vendor directory resolves");
        assert_eq!(packaged_aware, PdfiumLibrary::Directory(vendor.clone()));
        assert_eq!(
            resolve_pdfium_library(None, &vendor).expect("the vendor directory resolves"),
            PdfiumLibrary::Directory(vendor),
        );
    }

    #[test]
    fn the_system_library_is_still_the_last_resort() {
        let fixture = Fixture::new();
        let absent_packaged = fixture.absent("resources/pdfium");
        let absent_vendor = fixture.absent("vendor/pdfium/lib");

        let resolved = resolve_pdfium_library_with_packaged(
            None,
            Some(absent_packaged.as_path()),
            &absent_vendor,
        )
        .expect("the system library is always the last resort");
        assert_eq!(resolved, PdfiumLibrary::System);
    }

    /// `None` is what a caller outside an installed application passes, and it must behave exactly
    /// like the development resolver rather than pretending a packaged directory exists.
    #[test]
    fn no_packaged_candidate_is_not_an_error() {
        let fixture = Fixture::new();
        let vendor = fixture.directory("vendor/pdfium/lib");

        let resolved = resolve_pdfium_library_with_packaged(None, None, &vendor)
            .expect("a missing packaged candidate falls through");
        assert_eq!(resolved, PdfiumLibrary::Directory(vendor));
    }
}
