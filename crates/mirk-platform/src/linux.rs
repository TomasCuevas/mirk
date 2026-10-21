//! Linux backends.
//!
//! This module is the shape of the platform layer; the behaviour arrives with its own tasks. Every
//! unimplemented method returns [`MirkError::NotSupported`] rather than an empty success, so that a
//! missing capability is visible instead of looking like an empty library.
//!
//! Planned sources, for the implementer who picks this up:
//!
//! | Backend | Source of truth | Task |
//! | --- | --- | --- |
//! | Axis A | `~/.local/share/recently-used.xbel` (freedesktop Recent files, GTK) | T12 |
//! | Axis B | `/proc/<pid>/fd` plus `/proc/<pid>/cmdline` | T8 |
//! | Axis C | `xdg-open`, or a viewer CLI such as `evince -i <N>` | T7 |
//! | Watched folders | `inotify`, one watch per directory, coalesced per path | T7a |
//! | Identity | `st_dev` plus `st_ino` from `std::os::unix::fs::MetadataExt` | T11 |
//!
//! Two facts from the project's own measurements are worth carrying into that work:
//!
//! - `evince --page-index N` is **one-based** while the page Evince stores in GVFS metadata is
//!   **zero-based**. Converting at the wrong boundary resumes the reader one page off, silently.
//! - Metadata reads cost roughly 10 ms per document on the reference machine, so a scan over a
//!   large library must be incremental rather than a full re-read on every pass.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use inotify::{EventMask, Inotify, WatchDescriptor, WatchMask};

use mirk_core::{
    Availability, Confidence, Document, FolderWatcher, LaunchOutcome, Launcher, MirkError,
    OpenDocument, OpenDocumentDetector, Platform, ProgressCapability, ProgressProvider,
    ProgressSource, RecentDocument, RecentDocumentsSource, StoredProgress, WatchEvent,
    WatchEventKind, WatchTarget,
};

use crate::{not_supported, Backends};

const PLATFORM: Platform = Platform::Linux;

/// Every Linux backend, in the state this build provides it.
pub(super) fn backends() -> Backends {
    Backends {
        recent_documents: Box::new(XbelRecentDocuments),
        open_documents: Box::new(ProcOpenDocuments),
        launcher: Box::new(XdgLauncher::new()),
        file_identity: Box::new(PosixFileIdentity),
        folder_watcher: Box::new(LinuxFolderWatcher),
    }
}

/// Linux progress providers.
///
/// The first real entry is the Evince adapter, reading `metadata::evince::page` from the GVFS store
/// at `~/.local/share/gvfs-metadata/`. It is `ProgressCapability::Live` — measured, not assumed:
/// the store updates within roughly three seconds of a page change while the document is still
/// open.
pub(super) fn progress_providers() -> Vec<Box<dyn ProgressProvider>> {
    vec![Box::new(EvinceGvfsProvider::new())]
}

/// The frozen provider id. Stored progress rows reference this string, so it must never change.
const EVINCE_PROVIDER_ID: &str = "evince-gvfs";

/// The GVFS metadata attribute Evince writes its **zero-based** page index into.
const EVINCE_PAGE_ATTRIBUTE: &str = "metadata::evince::page";

/// `evince`'s one-based page flag (long form `--page-index`), shared with Papers.
const PAGE_INDEX_FLAG: &str = "-i";

/// Viewers that share the GVFS metadata store and accept a page on the command line, in preference
/// order. Papers is Evince's successor and uses the same flag.
const PAGE_VIEWERS: &[&str] = &["evince", "papers"];

/// The GVFS command-line tool.
const GIO: &str = "gio";

/// The system's "open this with the desktop default" helper.
const XDG_OPEN: &str = "xdg-open";

/// Reads reading position out of the GVFS metadata store Evince (and Papers) write to.
///
/// # Why a subprocess
///
/// The store at `~/.local/share/gvfs-metadata/home` is a log-structured format that is
/// undocumented and version-fragile; parsing it directly would be exactly the adapter fragility
/// this project treats as a liability (R10). `gio info -a metadata::evince::page` is the supported
/// way in, and it was measured at roughly 9 ms per document, which is affordable at library scale.
///
/// # The off-by-one
///
/// The stored value is a **zero-based** page index, and that is what [`read`](ProgressProvider::read)
/// returns. [`resume`](ProgressProvider::resume) hands the same zero-based page to a [`Launcher`],
/// whose command line converts to `evince`'s one-based flag exactly once. The conversion lives in
/// exactly one place, [`viewer_page_arguments`].
pub struct EvinceGvfsProvider {
    launcher: Box<dyn Launcher>,
    search_path: Vec<PathBuf>,
}

impl EvinceGvfsProvider {
    /// The production provider: the real Linux launcher and this process's own `PATH`.
    pub fn new() -> Self {
        Self {
            launcher: Box::new(XdgLauncher::new()),
            search_path: environment_search_path(),
        }
    }

    /// The injectable core. `launcher` lets a test observe where a resume was sent, and
    /// `search_path` makes the `gio`/viewer lookup deterministic, exactly as `mirk-store`'s
    /// `user_data_dir_for` takes its environment as parameters instead of reading it.
    #[cfg(test)]
    fn with_launcher(launcher: Box<dyn Launcher>, search_path: Vec<PathBuf>) -> Self {
        Self {
            launcher,
            search_path,
        }
    }
}

impl Default for EvinceGvfsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressProvider for EvinceGvfsProvider {
    fn id(&self) -> &'static str {
        EVINCE_PROVIDER_ID
    }

    fn platform(&self) -> Platform {
        PLATFORM
    }

    fn viewer_ids(&self) -> &'static [&'static str] {
        &["evince", "org.gnome.Evince", "papers", "org.gnome.Papers"]
    }

    fn capability(&self) -> ProgressCapability {
        // T1 measured the store updating within ~3 s of a page change with the document still open,
        // and surviving SIGTERM: this is a live source, not a close-time one.
        ProgressCapability::Live
    }

    fn detect(&self) -> Availability {
        // Without `gio` there is nothing to read, and without a viewer there is nothing to resume
        // into, so either one missing means the provider cannot work at all (S1).
        let has_gio = find_program(GIO, &self.search_path).is_some();
        let has_viewer = find_viewer(&self.search_path).is_some();
        if has_gio && has_viewer {
            Availability::Installed
        } else {
            Availability::Missing
        }
    }

    fn read(&self, doc: &Document) -> Result<Option<StoredProgress>, MirkError> {
        let Some(output) = gio_metadata_output(&self.search_path, &doc.path)? else {
            return Ok(None);
        };
        let Some(page) = parse_gio_page(&output) else {
            return Ok(None);
        };
        Ok(Some(StoredProgress::new(
            page,
            doc.pages,
            ProgressSource::External(EVINCE_PROVIDER_ID),
            Confidence::Exact,
        )))
    }

    fn resume(&self, doc: &Document, page: u32) -> Result<LaunchOutcome, MirkError> {
        self.launcher.launch(&doc.path, Some(page))
    }
}

/// Runs `gio info -a metadata::evince::page <path>` and returns its standard output.
///
/// `Ok(None)` means there is no readable record: `gio` is not installed, or it ran and refused the
/// path. Both are normal answers for a provider whose job is to report only a position it is sure
/// about. Failing to *start* `gio` is an error, because that is a broken environment rather than a
/// missing record.
fn gio_metadata_output(search_path: &[PathBuf], path: &Path) -> Result<Option<String>, MirkError> {
    let Some(gio) = find_program(GIO, search_path) else {
        return Ok(None);
    };
    let output = Command::new(&gio)
        .arg("info")
        .arg("-a")
        .arg(EVINCE_PAGE_ATTRIBUTE)
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| MirkError::Adapter {
            provider: EVINCE_PROVIDER_ID,
            message: format!("could not run `{}`: {error}", gio.display()),
        })?;
    if !output.status.success() {
        // `gio` exits nonzero for a path whose metadata it cannot read, which is "no record" from
        // this provider's point of view, not an adapter failure.
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
}

/// Extracts the stored zero-based page index from `gio info` output.
///
/// Pure, so the value path is tested against real `gio` output without running it or writing any
/// metadata. Returns `None` when the attribute is absent, when its value is not a plain
/// non-negative integer, or when it does not fit a page index. It never guesses (contract rule 2).
fn parse_gio_page(output: &str) -> Option<u32> {
    output.lines().find_map(|line| {
        let value = line
            .trim()
            .strip_prefix(EVINCE_PAGE_ATTRIBUTE)?
            .trim_start_matches(':')
            .trim();
        value.parse::<u32>().ok()
    })
}

/// Finds an executable `program` in `search_path`.
///
/// Pure: the search path is a parameter, so the lookup is tested against a temporary directory
/// without mutating the process environment or consulting the machine's real `PATH` — the same
/// injection pattern `mirk-store::user_data_dir_for` uses for its environment.
fn find_program(program: &str, search_path: &[PathBuf]) -> Option<PathBuf> {
    search_path.iter().find_map(|directory| {
        let candidate = directory.join(program);
        is_executable_file(&candidate).then_some(candidate)
    })
}

/// Whether `path` is a regular file with at least one execute bit set.
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// The first page-capable viewer found in `search_path`.
fn find_viewer(search_path: &[PathBuf]) -> Option<PathBuf> {
    PAGE_VIEWERS
        .iter()
        .find_map(|program| find_program(program, search_path))
}

/// This process's own `PATH`, split into directories.
fn environment_search_path() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default()
}

/// Axis A — the freedesktop recent-documents list.
pub struct XbelRecentDocuments;

impl RecentDocumentsSource for XbelRecentDocuments {
    fn id(&self) -> &'static str {
        "linux-xbel-recent"
    }

    fn platform(&self) -> Platform {
        PLATFORM
    }

    fn recent(&self, _limit: usize) -> Result<Vec<RecentDocument>, MirkError> {
        // TODO(T12): parse `~/.local/share/recently-used.xbel`, keep `application/pdf` entries,
        // skip entries whose file no longer exists, and return them newest first.
        Err(not_supported("recent-document ingestion", PLATFORM))
    }
}

/// Axis B — documents currently open, discovered by inspecting process file descriptors.
///
/// This is the plan's acknowledged **heuristic** (R4): thumbnail indexers, backups and antivirus
/// open PDFs without a human reading them, and `/proc` cannot say which is which. The detector
/// therefore reports every open `.pdf` target — including one held by this very process, which may
/// legitimately be the first-party reader — and leaves the judgement to the review queue this axis
/// feeds (S3). There is deliberately no viewer allowlist here.
///
/// Best effort is the contract (S5): a process whose `/proc/<pid>` cannot be listed (another
/// user's, `EACCES`) or that exits mid-scan is skipped without failing the call. Only an unreadable
/// `/proc` itself, which would make the answer meaningless, is an error.
pub struct ProcOpenDocuments;

impl OpenDocumentDetector for ProcOpenDocuments {
    fn id(&self) -> &'static str {
        "linux-proc-fd"
    }

    fn platform(&self) -> Platform {
        PLATFORM
    }

    /// A best-effort snapshot, one entry per `(path, pid)`, sorted by path then pid (S6).
    fn open_documents(&self) -> Result<Vec<OpenDocument>, MirkError> {
        let processes = fs::read_dir(PROC)?;
        // Keyed by `(path, pid)`, so duplicate descriptors in one process collapse to one entry and
        // the iteration order is already the contract's sort order: path, then pid.
        let mut found: BTreeMap<(PathBuf, u32), Option<String>> = BTreeMap::new();

        for process in processes {
            let Ok(process) = process else {
                continue;
            };
            let Some(pid) = parse_pid(&process.file_name()) else {
                continue;
            };
            // Resolved lazily: most processes hold no PDF, so their viewer name is never read.
            let mut viewer: Option<Option<String>> = None;
            let Ok(descriptors) = fs::read_dir(process.path().join("fd")) else {
                // The process exited, or belongs to another user. S5.
                continue;
            };

            for descriptor in descriptors {
                let Ok(descriptor) = descriptor else {
                    continue;
                };
                let Ok(target) = fs::read_link(descriptor.path()) else {
                    // The descriptor was closed between `read_dir` and `read_link`, or the target
                    // cannot be resolved. S4/S5: skip it rather than inventing a path.
                    continue;
                };
                if is_deleted(&target) || !is_pdf(&target) {
                    continue;
                }
                // Check the type through the resolved path: the `.pdf` suffix alone would accept a
                // symlink named `report.pdf` pointing at a directory.
                if !fs::metadata(&target).map(|meta| meta.is_file()).unwrap_or(false) {
                    continue;
                }

                let viewer = viewer.get_or_insert_with(|| read_viewer(pid)).clone();
                found.entry((target, pid)).or_insert(viewer);
            }
        }

        Ok(found
            .into_iter()
            .map(|((path, pid), viewer)| OpenDocument {
                path,
                pid: Some(pid),
                viewer,
            })
            .collect())
    }
}

/// The process filesystem root. A constant so the scan and its tests agree on one path.
const PROC: &str = "/proc";

/// The marker the kernel appends to a `/proc/<pid>/fd` target that has been unlinked.
const DELETED_SUFFIX: &[u8] = b" (deleted)";

/// The PDF extension, matched case-insensitively.
const PDF_EXTENSION: &[u8] = b".pdf";

/// Parses a `/proc` directory name as a process id.
///
/// `/proc` also holds `sys`, `self`, `thread-self` and similar; only numeric names are processes.
fn parse_pid(name: &std::ffi::OsStr) -> Option<u32> {
    name.to_str()?.parse().ok()
}

/// Whether a resolved descriptor target names a file the kernel has already unlinked.
///
/// The kernel appends ` (deleted)`. Reporting such a target would create a suggestion pointing at
/// a path that no longer exists, so it is skipped (S4).
fn is_deleted(target: &Path) -> bool {
    target.as_os_str().as_bytes().ends_with(DELETED_SUFFIX)
}

/// Whether the file name ends in `.pdf`, case-insensitively.
///
/// Compared as bytes rather than through a `String`: paths are opaque (R7), and a name that is not
/// valid UTF-8 must not be silently dropped.
fn is_pdf(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|name| name.as_bytes()) else {
        return false;
    };
    name.len() >= PDF_EXTENSION.len()
        && name[name.len() - PDF_EXTENSION.len()..].eq_ignore_ascii_case(PDF_EXTENSION)
}

/// The viewer name for `pid`, read from `/proc/<pid>/cmdline` with `/proc/<pid>/comm` as fallback.
///
/// `None` when neither is readable. The name is never invented and never the pid as a string (S2).
fn read_viewer(pid: u32) -> Option<String> {
    let base = Path::new(PROC).join(pid.to_string());
    let cmdline = fs::read(base.join("cmdline")).ok();
    let comm = fs::read(base.join("comm")).ok();
    viewer_name(cmdline.as_deref(), comm.as_deref())
}

/// Chooses the viewer name from the two `/proc` files' contents.
///
/// Pure, so the `cmdline` -> `comm` fallback is testable without a process that omits `cmdline`,
/// which cannot be produced on demand.
fn viewer_name(cmdline: Option<&[u8]>, comm: Option<&[u8]>) -> Option<String> {
    cmdline
        .and_then(first_argument_basename)
        .or_else(|| comm.and_then(comm_name))
}

/// The basename of `argv[0]` in a `/proc/<pid>/cmdline` image.
///
/// `cmdline` is a NUL-separated argument list; the first field is the executable. An empty first
/// field (a kernel thread, for instance) yields `None` so `viewer_name` can fall back.
fn first_argument_basename(cmdline: &[u8]) -> Option<String> {
    let first = cmdline.split(|byte| *byte == 0).next()?;
    if first.is_empty() {
        return None;
    }
    let argument = std::str::from_utf8(first).ok()?;
    Path::new(argument).file_name()?.to_str().map(str::to_owned)
}

/// A process name from `/proc/<pid>/comm`, which the kernel terminates with a newline.
fn comm_name(comm: &[u8]) -> Option<String> {
    let name = std::str::from_utf8(comm).ok()?;
    let name = name.trim_end_matches(['\n', '\0']);
    (!name.is_empty()).then(|| name.to_owned())
}

/// Axis C — opening documents through the desktop's own association mechanism.
///
/// `launch(path, None)` hands the document to `xdg-open`, the desktop default. `launch(path,
/// Some(page))` starts a page-capable viewer's command line. The zero-to-one-based conversion
/// lives in exactly one place, [`viewer_page_arguments`]; nothing else adds or subtracts one.
pub struct XdgLauncher {
    spawner: Box<dyn ProcessSpawner>,
    search_path: Vec<PathBuf>,
}

impl XdgLauncher {
    /// The production launcher: spawns real processes and looks programs up on this process's
    /// `PATH`.
    pub fn new() -> Self {
        Self {
            spawner: Box::new(SystemSpawner),
            search_path: environment_search_path(),
        }
    }

    /// The injectable core: a recording spawner and an explicit search path.
    #[cfg(test)]
    fn with_spawner(spawner: Box<dyn ProcessSpawner>, search_path: Vec<PathBuf>) -> Self {
        Self {
            spawner,
            search_path,
        }
    }

    /// Opens `path` with the desktop's default handler.
    fn launch_default(&self, path: &Path) -> Result<LaunchOutcome, MirkError> {
        let Some(program) = find_program(XDG_OPEN, &self.search_path) else {
            return Err(missing_program(XDG_OPEN, "open the document"));
        };
        self.spawn(&program, &[path.as_os_str().to_os_string()])?;
        Ok(LaunchOutcome::Delegated {
            viewer: XDG_OPEN.to_string(),
        })
    }

    /// Opens `path` at `page` with the first page-capable viewer installed.
    ///
    /// When no page-capable viewer is installed the document is **still opened** with the desktop's
    /// default handler, because [`Launcher::launch`]'s contract is explicit: landing on page one is a
    /// degraded success, not an error, and refusing to open anything would be worse for the user than
    /// opening at the start. The returned `Delegated { viewer }` names whichever program actually
    /// ran, so a caller can tell the two cases apart. An error remains honest when *nothing* can open
    /// the document at all, which is [`Self::launch_default`]'s own case.
    ///
    /// Known limitation, recorded rather than hidden: `LaunchOutcome` cannot say "opened, but not at
    /// the page you asked for". The viewer name is the only signal. Adding a variant is additive and
    /// cheap, and should happen when a caller actually needs to tell the user that their viewer
    /// cannot be asked for a page.
    fn launch_at_page(&self, path: &Path, page: u32) -> Result<LaunchOutcome, MirkError> {
        let Some(program) = find_viewer(&self.search_path) else {
            // Degraded and named as such: the program that runs cannot be asked for a page.
            return self.launch_default(path);
        };
        let viewer = program_name(&program);
        self.spawn(&program, &viewer_page_arguments(page, path))?;
        Ok(LaunchOutcome::Delegated { viewer })
    }

    fn spawn(&self, program: &Path, args: &[OsString]) -> Result<(), MirkError> {
        self.spawner.spawn(program, args).map_err(|error| {
            MirkError::Io(io::Error::other(format!(
                "could not start `{}`: {error}",
                program.display()
            )))
        })
    }
}

impl Default for XdgLauncher {
    fn default() -> Self {
        Self::new()
    }
}

impl Launcher for XdgLauncher {
    fn id(&self) -> &'static str {
        "linux-xdg-open"
    }

    fn platform(&self) -> Platform {
        PLATFORM
    }

    fn launch(&self, path: &Path, page: Option<u32>) -> Result<LaunchOutcome, MirkError> {
        match page {
            None => self.launch_default(path),
            Some(page) => self.launch_at_page(path, page),
        }
    }
}

/// The honest error for a program a launch needs but cannot find.
fn missing_program(program: &str, purpose: &str) -> MirkError {
    MirkError::Io(io::Error::new(
        io::ErrorKind::NotFound,
        format!("cannot {purpose}: `{program}` is not installed"),
    ))
}

/// The plain file name of a program path, for the [`LaunchOutcome`].
fn program_name(program: &Path) -> String {
    program
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.display().to_string())
}

/// The argument list for a page-capable viewer, with the **one and only** zero-to-one-based
/// conversion in the adapter.
///
/// `metadata::evince::page` is zero-based; `evince -i` (`--page-index`) is one-based. T1 confirmed
/// the mismatch three times, so the conversion happens here, at the command-line boundary, and the
/// round-trip test pins it. `u64` arithmetic keeps a corrupt `u32::MAX` page from overflowing and
/// panicking.
fn viewer_page_arguments(page: u32, path: &Path) -> Vec<OsString> {
    let one_based = u64::from(page) + 1;
    vec![
        OsString::from(PAGE_INDEX_FLAG),
        OsString::from(one_based.to_string()),
        path.as_os_str().to_os_string(),
    ]
}

/// Starts a program without waiting for it to finish.
///
/// Injected so a test can observe the exact program and arguments a launch would use without
/// starting a viewer. The real implementation reaps the child on a detached thread: a viewer such
/// as Evince stays alive for as long as the document is open, so waiting here would block the
/// caller for minutes.
trait ProcessSpawner: Send + Sync {
    fn spawn(&self, program: &Path, args: &[OsString]) -> io::Result<()>;
}

/// Spawns real processes with [`std::process::Command`].
struct SystemSpawner;

impl ProcessSpawner for SystemSpawner {
    fn spawn(&self, program: &Path, args: &[OsString]) -> io::Result<()> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        // Reap the child on a thread of its own, because waiting here would block until the viewer
        // exits — that is, until the reader closes the document. `Builder::spawn` rather than
        // `thread::spawn` because the latter panics when the OS cannot create a thread, and nothing
        // on this path may panic.
        //
        // A reaper that cannot be created is deliberately **not** an error: the document did open,
        // and reporting otherwise would be a lie about what the user sees. The only cost is that the
        // child is not reaped until this process exits.
        let _ = std::thread::Builder::new()
            .name("mirk-launch-reaper".to_string())
            .spawn(move || {
                let _ = child.wait();
            });
        Ok(())
    }
}

/// Portable identity on POSIX, from `st_dev` and `st_ino`.
///
/// The implementation lives in [`crate::posix`] so that every Unix backend shares it. Some
/// filesystems report unusable ids; the fallback there hashes content instead of guessing.
pub use crate::posix::PosixFileIdentity;

// --- Watched folders (T7a) --------------------------------------------------------------------

/// How long a burst of events for one path is merged before the path is reported.
///
/// Chosen at 250 ms: longer than the gap between the several `CLOSE_WRITE`/`MOVED_TO` events a
/// copy tool emits for a single file, so those collapse into one report; short enough that a new
/// document shows up without the user noticing a delay. It is also comfortably longer than this
/// project's measured ~10 ms per-document scan cost, so the report cannot race the scan it
/// triggers.
const COALESCE_WINDOW: Duration = Duration::from_millis(250);

/// How long the loop sleeps when no events are available.
///
/// The inotify API has no "read with timeout", and blocking in `read` would make the stop flag
/// unobservable until the next filesystem event. A short non-blocking read followed by this sleep
/// keeps the loop off the CPU while bounding how long a stop request waits. It is deliberately far
/// below [`COALESCE_WINDOW`] so polling never delays coalescing.
const POLL_INTERVAL: Duration = Duration::from_millis(25);

/// A comfortably large event buffer.
///
/// One inotify event is the kernel struct plus a name of at most `NAME_MAX` (255) bytes, so this
/// holds many events and can never be too small for a single one — an undersized buffer makes
/// `read` fail with `EINVAL`.
const EVENT_BUFFER_BYTES: usize = 64 * 1024;

/// `ENOSPC` on Linux. `inotify_add_watch` uses it for an exhausted watch-descriptor limit, which
/// is why it must never be presented to the user as "the disk is full".
const ENOSPC: i32 = 28;

/// The events the watcher subscribes to.
///
/// Deliberately **not** `MODIFY`: a file being written is not settled, and subscribing to
/// `IN_MODIFY` is exactly how a watcher ends up reporting a half-copied PDF. `CREATE` is needed to
/// notice new subdirectories and to tell an `Added` file from a `Changed` one, but is not itself
/// reported — the following `CLOSE_WRITE` is what settles the file.
const WATCH_MASK: WatchMask = WatchMask::CREATE
    .union(WatchMask::CLOSE_WRITE)
    .union(WatchMask::MOVED_TO)
    .union(WatchMask::MOVED_FROM)
    .union(WatchMask::DELETE)
    .union(WatchMask::DELETE_SELF)
    .union(WatchMask::MOVE_SELF);

/// Linux watched-folder watcher, backed by **inotify**.
///
/// One inotify instance watches every target directory. The value holds no state between calls:
/// [`FolderWatcher::watch`] builds the instance, runs the loop and drops it, so the same value can
/// be reused.
pub struct LinuxFolderWatcher;

impl FolderWatcher for LinuxFolderWatcher {
    fn id(&self) -> &'static str {
        "linux-inotify"
    }

    fn platform(&self) -> Platform {
        PLATFORM
    }

    fn watch(
        &self,
        targets: &[WatchTarget],
        stop: &AtomicBool,
        on_event: &mut dyn FnMut(WatchEvent),
    ) -> Result<(), MirkError> {
        WatchLoop::start(targets)?.run(stop, on_event)
    }
}

/// Maps a failed `inotify_add_watch` into the error `mirk` reports.
///
/// `inotify_add_watch` reports an exhausted per-user watch limit as `ENOSPC`, the same errno as a
/// genuinely full disk. Returning it unchanged would send the user looking at their disk; this
/// names the limit and the tunable that raises it instead. Every other failure keeps its original,
/// typed cause. Small and pure so it can be unit-tested without provoking a real kernel limit.
fn map_watch_error(path: &Path, error: io::Error) -> MirkError {
    if is_watch_limit(&error) {
        MirkError::Io(io::Error::other(format!(
            "cannot watch `{}`: the inotify watch limit is exhausted ({error}); raise \
             fs.inotify.max_user_watches and retry",
            path.display()
        )))
    } else {
        MirkError::Io(error)
    }
}

/// Whether an `inotify_add_watch` failure is the kernel refusing more watch descriptors.
///
/// `ENOSPC` from `inotify_add_watch` means the watch limit is exhausted, not that the disk is
/// full. Pure so it can be unit-tested without provoking the real limit.
fn is_watch_limit(error: &io::Error) -> bool {
    error.raw_os_error() == Some(ENOSPC)
}

/// Reads an inotify mask that describes the watcher's own health instead of a file change.
///
/// `IN_Q_OVERFLOW` is the only such signal today: it means the kernel dropped events and the
/// watcher's view may already be stale. Pure, so the classification is unit-testable without
/// provoking a real queue overflow, which userspace cannot do deterministically.
fn watcher_observation(mask: EventMask) -> Option<WatchEventKind> {
    mask.contains(EventMask::Q_OVERFLOW)
        .then_some(WatchEventKind::Lost)
}

/// Maps a target that cannot even be resolved into the error `mirk` reports.
///
/// A watched target is canonicalised so the watcher watches — and reports — the same path the
/// scanner stored, which also makes a symlinked target resolve to its real directory. If that
/// resolution fails, the target is unusable and the message names both the path and the cause.
fn map_target_error(path: &Path, error: io::Error) -> MirkError {
    MirkError::InvalidPath(format!(
        "watched target `{}` could not be resolved: {error}",
        path.display()
    ))
}

/// The decision the recursive walk makes about one directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryDecision {
    /// A directory: watch it and walk into it.
    Directory,
    /// Not a directory: nothing to watch.
    Ignore,
    /// The entry could not be read, or its type could not be determined: report `Unwatchable`.
    Unwatchable,
}

/// Classifies one `read_dir` item.
///
/// Extracted as a pure function because neither a per-entry iteration failure nor a `file_type`
/// failure can be produced on demand, so the error branches are tested directly rather than
/// through a reproduction that does not exist.
fn classify_entry(entry: &io::Result<fs::DirEntry>) -> EntryDecision {
    let Ok(entry) = entry else {
        return EntryDecision::Unwatchable;
    };
    match entry.file_type() {
        Ok(file_type) if file_type.is_dir() => EntryDecision::Directory,
        Ok(_) => EntryDecision::Ignore,
        Err(_) => EntryDecision::Unwatchable,
    }
}

/// One raw kernel event, owned so it can outlive the read buffer.
struct RawEvent {
    descriptor: WatchDescriptor,
    mask: EventMask,
    name: Option<OsString>,
}

/// An event waiting out the coalescing window.
struct PendingEvent {
    kind: WatchEventKind,
    first_seen: Instant,
}

/// The state of one [`FolderWatcher::watch`] call.
struct WatchLoop {
    inotify: Inotify,
    /// Watch descriptor to the directory it watches.
    watched: HashMap<WatchDescriptor, PathBuf>,
    /// The roots whose whole subtree is watched. A directory that appears later is only watched if
    /// it lies under one of these.
    recursive_roots: Vec<PathBuf>,
    /// Paths for which `IN_CREATE` was seen but the write has not settled, so the close is `Added`.
    created: HashSet<PathBuf>,
    /// Paths already reported as present during this call, so a later move-in is `Changed`.
    seen: HashSet<PathBuf>,
    /// Events waiting out the coalescing window, keyed by path. A `BTreeMap` makes delivery order
    /// deterministic at no cost at this size.
    pending: BTreeMap<PathBuf, PendingEvent>,
    buffer: Vec<u8>,
    /// The resolved (canonical) target paths the caller asked for. A queue overflow normally names
    /// no directory, so these are what `Lost` is reported against.
    roots: Vec<PathBuf>,
}

impl WatchLoop {
    fn start(targets: &[WatchTarget]) -> Result<Self, MirkError> {
        let inotify = Inotify::init().map_err(MirkError::Io)?;
        let mut state = Self {
            inotify,
            watched: HashMap::new(),
            recursive_roots: Vec::new(),
            created: HashSet::new(),
            seen: HashSet::new(),
            pending: BTreeMap::new(),
            buffer: vec![0; EVENT_BUFFER_BYTES],
            roots: Vec::new(),
        };
        for target in targets {
            // Watch and report the same path the scanner stored: canonicalising resolves a
            // symlinked target to the real directory, so a symlink can never make the watcher and
            // the scanner disagree about where a file lives. An explicit target that cannot be
            // resolved is fatal: the caller asked for it by name.
            let resolved = target
                .path
                .canonicalize()
                .map_err(|error| map_target_error(&target.path, error))?;
            // An explicit target that cannot be watched is fatal, whatever the reason: the caller
            // asked for it by name, so a partial watch would be a broken promise.
            state
                .add_directory(&resolved)
                .map_err(|error| map_watch_error(&resolved, error))?;
            state.roots.push(resolved.clone());
            if target.recursive {
                state.recursive_roots.push(resolved.clone());
                state.add_subdirectories(&resolved)?;
            }
        }
        Ok(state)
    }

    fn run(
        &mut self,
        stop: &AtomicBool,
        on_event: &mut dyn FnMut(WatchEvent),
    ) -> Result<(), MirkError> {
        while !stop.load(Ordering::Relaxed) {
            let events = match self.read_events() {
                Ok(events) => events,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => Vec::new(),
                Err(error) => return Err(MirkError::Io(error)),
            };
            for event in events {
                self.handle(event)?;
            }
            self.flush(on_event, false);
            if !stop.load(Ordering::Relaxed) {
                std::thread::sleep(POLL_INTERVAL);
            }
        }
        // Deliver whatever was observed but had not yet left the coalescing window, so stopping
        // does not silently drop a change that has already settled.
        self.flush(on_event, true);
        Ok(())
    }

    fn read_events(&mut self) -> io::Result<Vec<RawEvent>> {
        // Borrow the two fields separately: the event iterator borrows the buffer, and the borrow
        // checker cannot see through `self` for two disjoint field borrows in one expression.
        let inotify = &mut self.inotify;
        let buffer = &mut self.buffer;
        let mut raw = Vec::new();
        for event in inotify.read_events(buffer)? {
            raw.push(RawEvent {
                descriptor: event.wd.clone(),
                mask: event.mask,
                name: event.name.map(|name| name.to_os_string()),
            });
        }
        Ok(raw)
    }

    fn handle(&mut self, raw: RawEvent) -> Result<(), MirkError> {
        let mask = raw.mask;

        // A queue overflow describes the watcher, not a path, and usually carries no usable
        // descriptor. It must be classified before the normal path logic, which would otherwise
        // drop it as untracked and let the library go silently stale.
        if let Some(WatchEventKind::Lost) = watcher_observation(mask) {
            self.report_lost();
            return Ok(());
        }

        let path = match &raw.name {
            Some(name) => self.watched.get(&raw.descriptor).map(|dir| dir.join(name)),
            None => self.watched.get(&raw.descriptor).cloned(),
        };
        // A watch this loop no longer tracks; nothing to report.
        let Some(path) = path else {
            return Ok(());
        };

        if mask.contains(EventMask::IGNORED) {
            self.watched.remove(&raw.descriptor);
            return Ok(());
        }

        if mask.contains(EventMask::ISDIR) {
            if mask.intersects(EventMask::CREATE | EventMask::MOVED_TO) {
                self.watch_new_directory(&path)?;
            }
            if mask.intersects(
                EventMask::DELETE
                    | EventMask::MOVED_FROM
                    | EventMask::DELETE_SELF
                    | EventMask::MOVE_SELF,
            ) {
                // The directory left the tree, whether deleted or moved elsewhere. inotify keeps
                // watching the inode, so the watches under it must be released or the loop would
                // keep monitoring — and mislabelling — a path outside the nominated target.
                self.release_subtree(&path);
                self.enqueue(path, WatchEventKind::Removed);
            }
            return Ok(());
        }

        // A directory's own self-event is expected to carry `IN_ISDIR`. Handle it here anyway so a
        // kernel that omits the flag cannot leave a whole subtree watched after its root is gone.
        if mask.intersects(EventMask::MOVE_SELF | EventMask::DELETE_SELF) {
            self.release_subtree(&path);
            self.enqueue(path, WatchEventKind::Removed);
            return Ok(());
        }

        if mask.contains(EventMask::CLOSE_WRITE) {
            let kind = if self.created.remove(&path) {
                WatchEventKind::Added
            } else {
                WatchEventKind::Changed
            };
            self.seen.insert(path.clone());
            self.enqueue(path, kind);
        } else if mask.contains(EventMask::MOVED_TO) {
            let kind = if self.seen.insert(path.clone()) {
                WatchEventKind::Added
            } else {
                WatchEventKind::Changed
            };
            self.created.remove(&path);
            self.enqueue(path, kind);
        } else if mask.contains(EventMask::CREATE) {
            // A file that is still being written. Remember it so the close is `Added`, but do not
            // report it yet: a created file is not a settled file.
            self.created.insert(path);
        } else if mask.intersects(EventMask::DELETE | EventMask::MOVED_FROM) {
            self.seen.remove(&path);
            self.created.remove(&path);
            self.enqueue(path, WatchEventKind::Removed);
        }
        Ok(())
    }

    /// Adds one watch for `directory`, returning the kernel's own error on failure so callers can
    /// choose between fatal (`ENOSPC`) and degraded (`Unwatchable`).
    fn add_directory(&mut self, directory: &Path) -> io::Result<()> {
        let descriptor = self.inotify.watches().add(directory, WATCH_MASK)?;
        self.watched.insert(descriptor, directory.to_path_buf());
        Ok(())
    }

    /// Adds a watch for every directory under `root`.
    ///
    /// Symlinks are not followed: `DirEntry::file_type` does not resolve them, which also stops a
    /// symlink loop from hanging this walk. A subdirectory that cannot be read is reported as
    /// [`WatchEventKind::Unwatchable`] rather than skipped quietly, because its contents are
    /// invisible and the library must know that. The walk itself continues: one unreadable corner
    /// must not stop the rest of the tree from being watched.
    fn add_subdirectories(&mut self, root: &Path) -> Result<(), MirkError> {
        let mut stack = vec![root.to_path_buf()];
        while let Some(directory) = stack.pop() {
            let entries = match fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(_) => {
                    self.enqueue(directory, WatchEventKind::Unwatchable);
                    continue;
                }
            };
            for entry in entries {
                match classify_entry(&entry) {
                    EntryDecision::Directory => {
                        if let Ok(entry) = entry {
                            let subdirectory = entry.path();
                            self.try_watch_subdirectory(&subdirectory)?;
                            // Walk it even when its own watch could not be installed: its children
                            // may still be watchable, and the parent is already reported
                            // `Unwatchable`.
                            stack.push(subdirectory);
                        }
                    }
                    EntryDecision::Ignore => {}
                    EntryDecision::Unwatchable => {
                        // Prefer the entry's own path when it can be read; a per-entry read
                        // failure leaves only the directory that produced it to name.
                        let path = match entry {
                            Ok(entry) => entry.path(),
                            Err(_) => directory.clone(),
                        };
                        self.enqueue(path, WatchEventKind::Unwatchable);
                    }
                }
            }
        }
        Ok(())
    }

    /// Watches a discovered subdirectory, treating only the kernel's hard watch limit as fatal.
    ///
    /// `ENOSPC` stays fatal on purpose: the contract requires one honest error naming the limit
    /// rather than a watch that silently covers only part of the tree. Every other failure — a
    /// permission the kernel refuses, a directory that vanished between listing and watching — is
    /// reported as [`WatchEventKind::Unwatchable`] and the walk continues.
    fn try_watch_subdirectory(&mut self, path: &Path) -> Result<(), MirkError> {
        match self.add_directory(path) {
            Ok(()) => Ok(()),
            Err(error) if is_watch_limit(&error) => Err(map_watch_error(path, error)),
            Err(_) => {
                self.enqueue(path.to_path_buf(), WatchEventKind::Unwatchable);
                Ok(())
            }
        }
    }

    /// Releases every watch at or under `path` and forgets the bookkeeping for that subtree.
    ///
    /// inotify watches the inode, not the name: when a watched directory is moved elsewhere its
    /// watch follows it, and events would keep arriving composed with the old stored path — a
    /// monitor outside the tree the caller nominated. The kernel only sends `IN_IGNORED` for a
    /// watch it removed itself (a deletion); a move keeps the watch alive, and the `seen`/`created`
    /// bookkeeping for descendants is never covered by `IN_IGNORED` at all. This helper covers
    /// both, and is idempotent so the parent's `MOVED_FROM` and the child's `MOVE_SELF` may both
    /// call it.
    fn release_subtree(&mut self, path: &Path) {
        let stale: Vec<WatchDescriptor> = self
            .watched
            .iter()
            .filter(|(_, directory)| {
                let directory = directory.as_path();
                directory == path || directory.starts_with(path)
            })
            .map(|(descriptor, _)| descriptor.clone())
            .collect();
        for descriptor in stale {
            // A watch the kernel already dropped (the common case for a deleted directory, whose
            // `IN_IGNORED` is on its way) refuses removal; that is not an error to report.
            let _ = self.inotify.watches().remove(descriptor.clone());
            self.watched.remove(&descriptor);
        }
        self.seen
            .retain(|known| known.as_path() != path && !known.starts_with(path));
        self.created
            .retain(|created| created.as_path() != path && !created.starts_with(path));
        // A settled event already inside the coalescing window belongs to the tree that just left.
        // Delivering it afterwards would report a path outside the target — the very thing releasing
        // the watches exists to prevent — so the window is emptied for that subtree too.
        self.pending
            .retain(|queued, _| queued.as_path() != path && !queued.starts_with(path));
    }

    /// Watches a directory that appeared while watching, but only when it lies under a recursive
    /// target. A non-recursive target reports the new directory's contents not at all, which is the
    /// contract of `WatchTarget::recursive`.
    fn watch_new_directory(&mut self, path: &Path) -> Result<(), MirkError> {
        if !self
            .recursive_roots
            .iter()
            .any(|root| path.starts_with(root))
        {
            return Ok(());
        }
        if self.watched.values().any(|dir| dir.as_path() == path) {
            return Ok(());
        }
        self.try_watch_subdirectory(path)?;
        self.add_subdirectories(path)
    }

    /// Reports that events may have been dropped for a directory.
    ///
    /// A queue overflow normally carries no usable descriptor, so no single directory can be
    /// named. In that case every target root is reported: any of them may have lost events, and
    /// the consumer must rescan to be sure. When a descriptor is present, only that directory is
    /// reported.
    /// Reports that events may have been dropped.
    ///
    /// A queue overflow belongs to the inotify **instance**, not to one directory: the kernel sets
    /// the descriptor to `-1` for exactly that reason, because the loss is not attributable to a
    /// single watch. So every target is reported. Narrowing this to whichever directory a descriptor
    /// happened to name would leave the rest of the library silently stale — the failure this variant
    /// exists to prevent — and a consumer that rescans one directory would never learn otherwise.
    fn report_lost(&mut self) {
        for root in self.roots.clone() {
            self.enqueue(root, WatchEventKind::Lost);
        }
    }

    /// Queues an event, merging it into any event already waiting for the same path.
    ///
    /// The latest kind wins: a path created and then removed inside one window is reported as
    /// removed, which is its final observed state.
    fn enqueue(&mut self, path: PathBuf, kind: WatchEventKind) {
        match self.pending.get_mut(&path) {
            Some(pending) => pending.kind = kind,
            None => {
                self.pending.insert(
                    path,
                    PendingEvent {
                        kind,
                        first_seen: Instant::now(),
                    },
                );
            }
        }
    }

    /// Emits every queued event whose window has elapsed, or all of them when `force` is set.
    fn flush(&mut self, on_event: &mut dyn FnMut(WatchEvent), force: bool) {
        let now = Instant::now();
        let due: Vec<(PathBuf, WatchEventKind)> = self
            .pending
            .iter()
            .filter(|(_, pending)| {
                force || now.duration_since(pending.first_seen) >= COALESCE_WINDOW
            })
            .map(|(path, pending)| (path.clone(), pending.kind))
            .collect();
        for (path, kind) in due {
            self.pending.remove(&path);
            on_event(WatchEvent { path, kind });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mirk_core::{FileId, FileIdentity, FolderWatcher, WatchEvent, WatchEventKind, WatchTarget};
    use std::collections::HashSet;
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::{Duration, Instant};

    /// A unique temporary directory that removes itself on drop, including on panic.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "mirk-linux-{label}-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// The whole point of the stub convention: an unimplemented backend must say so, because
    /// `Ok(vec![])` would be indistinguishable from a genuinely empty library.
    #[test]
    fn the_recent_documents_stub_reports_not_supported_rather_than_empty_success() {
        let recent = XbelRecentDocuments.recent(10);
        assert!(matches!(recent, Err(MirkError::NotSupported { .. })));

        // The `/proc` detector is implemented (T8), so it must answer rather than claim the
        // capability is missing. Its behaviour is covered by the T8 tests below.
        assert!(ProcOpenDocuments.open_documents().is_ok());
    }

    // --- T9: the Evince GVFS progress provider and the Linux launcher -----------------------

    const EVINCE_ID: &str = "evince-gvfs";

    /// Writes an executable file named `name` into `dir`.
    fn make_executable(dir: &TempDir, name: &str, contents: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.path.join(name);
        std::fs::write(&path, contents).expect("write the fake program");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("make the fake program executable");
        path
    }

    /// A fake `gio` that answers with a record line, so `read` can be exercised end to end without
    /// writing to the user's GVFS metadata. Only `gio set` can create a real record, and the tests
    /// must never run it, so the record is produced by a stand-in program instead.
    fn fake_gio(dir: &TempDir, page: u32) -> PathBuf {
        make_executable(
            dir,
            "gio",
            &format!(
                "#!/bin/sh\n\
                 echo \"uri: file:///tmp/document.pdf\"\n\
                 echo \"local path: /tmp/document.pdf\"\n\
                 echo \"unix mount: /dev/x / ext4 rw,relatime\"\n\
                 echo \"attributes:\"\n\
                 echo \"  metadata::evince::page: {page}\"\n"
            ),
        )
    }

    /// The calls a recording spawner or launcher observed, shared with the test that asserts on
    /// them.
    type SpawnCalls = Arc<Mutex<Vec<(PathBuf, Vec<OsString>)>>>;
    type LaunchCalls = Arc<Mutex<Vec<(PathBuf, Option<u32>)>>>;

    #[derive(Clone, Default)]
    struct RecordingSpawner {
        calls: SpawnCalls,
    }
    impl ProcessSpawner for RecordingSpawner {
        fn spawn(&self, program: &Path, args: &[OsString]) -> io::Result<()> {
            self.calls
                .lock()
                .expect("recording spawner poisoned")
                .push((program.to_path_buf(), args.to_vec()));
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct RecordingLauncher {
        calls: LaunchCalls,
    }
    impl Launcher for RecordingLauncher {
        fn id(&self) -> &'static str {
            "recording-launcher"
        }

        fn platform(&self) -> Platform {
            PLATFORM
        }

        fn launch(&self, path: &Path, page: Option<u32>) -> Result<LaunchOutcome, MirkError> {
            self.calls
                .lock()
                .expect("recording launcher poisoned")
                .push((path.to_path_buf(), page));
            Ok(LaunchOutcome::Delegated {
                viewer: "recording".to_string(),
            })
        }
    }

    #[test]
    fn the_evince_provider_identity_and_capability_are_frozen() {
        let provider = EvinceGvfsProvider::new();
        assert_eq!(provider.id(), EVINCE_ID);
        assert_eq!(provider.platform(), Platform::Linux);
        assert_eq!(provider.capability(), ProgressCapability::Live);
        assert_eq!(
            provider.viewer_ids(),
            &["evince", "org.gnome.Evince", "papers", "org.gnome.Papers"][..]
        );
    }

    #[test]
    fn detect_is_missing_when_gio_is_absent() {
        let dir = TempDir::new("t9-no-gio");
        make_executable(&dir, "evince", "#!/bin/sh\n");
        let provider = EvinceGvfsProvider::with_launcher(
            Box::new(RecordingLauncher::default()),
            vec![dir.path.clone()],
        );
        assert_eq!(provider.detect(), Availability::Missing);
    }

    #[test]
    fn detect_is_missing_when_no_viewer_is_present() {
        let dir = TempDir::new("t9-no-viewer");
        fake_gio(&dir, 4);
        let provider = EvinceGvfsProvider::with_launcher(
            Box::new(RecordingLauncher::default()),
            vec![dir.path.clone()],
        );
        assert_eq!(provider.detect(), Availability::Missing);
    }

    #[test]
    fn detect_is_installed_when_both_gio_and_a_viewer_are_present() {
        let dir = TempDir::new("t9-installed");
        fake_gio(&dir, 4);
        make_executable(&dir, "evince", "#!/bin/sh\n");
        let provider = EvinceGvfsProvider::with_launcher(
            Box::new(RecordingLauncher::default()),
            vec![dir.path.clone()],
        );
        assert_eq!(provider.detect(), Availability::Installed);
    }

    #[test]
    fn detect_is_missing_for_an_empty_search_path() {
        let provider = EvinceGvfsProvider::with_launcher(
            Box::new(RecordingLauncher::default()),
            Vec::new(),
        );
        assert_eq!(provider.detect(), Availability::Missing);
    }

    #[test]
    fn the_gio_parser_reads_a_stored_page_and_its_absence() {
        let with_record = concat!(
            "uri: file:///home/u/libros/Comparaci%C3%B3n.pdf\n",
            "local path: /home/u/libros/Comparación.pdf\n",
            "unix mount: /dev/nvme0n1p2 / ext4 rw,relatime\n",
            "attributes:\n",
            "  metadata::evince::page: 72\n",
        );
        assert_eq!(parse_gio_page(with_record), Some(72));

        let without_record = concat!(
            "uri: file:///tmp/document.pdf\n",
            "local path: /tmp/document.pdf\n",
            "unix mount: /dev/nvme0n1p2 / ext4 rw,relatime\n",
            "attributes:\n",
        );
        assert_eq!(parse_gio_page(without_record), None);
    }

    #[test]
    fn the_gio_parser_refuses_anything_it_cannot_understand() {
        assert_eq!(parse_gio_page(""), None);
        assert_eq!(parse_gio_page("attributes:\n"), None);
        assert_eq!(parse_gio_page("attributes:\n  metadata::evince::page:\n"), None);
        assert_eq!(
            parse_gio_page("attributes:\n  metadata::evince::page: later\n"),
            None
        );
        assert_eq!(
            parse_gio_page("attributes:\n  metadata::evince::page: -1\n"),
            None
        );
        // A lookalike attribute must not be mistaken for the real one.
        assert_eq!(
            parse_gio_page("attributes:\n  metadata::evince::page_number: 5\n"),
            None
        );
    }

    #[test]
    fn read_returns_the_stored_zero_based_page_with_its_provenance() {
        let dir = TempDir::new("t9-read");
        fake_gio(&dir, 72);
        make_executable(&dir, "evince", "#!/bin/sh\n");
        let provider = EvinceGvfsProvider::with_launcher(
            Box::new(RecordingLauncher::default()),
            vec![dir.path.clone()],
        );
        let mut doc = Document::new("/home/u/libros/Comparación.pdf");
        doc.pages = Some(436);

        let progress = provider
            .read(&doc)
            .expect("read must not fail")
            .expect("a record exists");
        assert_eq!(
            progress.page, 72,
            "the stored page is zero-based and must not be shifted"
        );
        assert_eq!(progress.total, Some(436));
        assert_eq!(progress.source, ProgressSource::External(EVINCE_ID));
        assert_eq!(progress.confidence, Confidence::Exact);
    }

    #[test]
    fn read_returns_none_for_a_file_with_no_record() {
        // A real `gio` invocation against a real file with no metadata: the subprocess and the
        // parsing are exercised without writing anything anywhere.
        let dir = TempDir::new("t9-no-record");
        let path = dir.path.join("plain.pdf");
        std::fs::write(&path, b"%PDF-1.7 no metadata").expect("write the file");
        let doc = Document::new(&path);

        let progress = EvinceGvfsProvider::new()
            .read(&doc)
            .expect("a missing record is not an error");
        assert_eq!(progress, None, "no record must be Ok(None), never a zero");
    }

    #[test]
    fn resume_from_a_read_position_hands_the_cli_the_one_based_index() {
        // A full round trip through a real `gio` subprocess: the stand-in `gio` returns a stored
        // page, `read` turns it into a `StoredProgress`, and `resume` routes it through an
        // `XdgLauncher` whose spawner records the exact arguments the viewer CLI would receive.
        // This is the off-by-one round-trip test S3 demands, and it writes no metadata.
        let dir = TempDir::new("t9-round-trip");
        fake_gio(&dir, 72);
        make_executable(&dir, "evince", "#!/bin/sh\n");
        let spawner = RecordingSpawner::default();
        let launcher =
            XdgLauncher::with_spawner(Box::new(spawner.clone()), vec![dir.path.clone()]);
        let provider =
            EvinceGvfsProvider::with_launcher(Box::new(launcher), vec![dir.path.clone()]);

        let doc = Document::new("/home/u/libros/Comparación.pdf");
        let stored = provider
            .read(&doc)
            .expect("read must not fail")
            .expect("a record exists");
        provider.resume(&doc, stored.page).expect("resume");

        let calls = spawner.calls.lock().expect("spawner poisoned");
        assert_eq!(calls.len(), 1, "expected one viewer invocation: {calls:?}");
        let (program, args) = &calls[0];
        assert_eq!(program.file_name().and_then(|name| name.to_str()), Some("evince"));
        assert_eq!(args[0], OsString::from(PAGE_INDEX_FLAG));
        assert_eq!(
            args[1],
            OsString::from("73"),
            "stored page 72 must reach the CLI as 73"
        );
        assert_eq!(args[2], doc.path.as_os_str().to_os_string());
    }

    #[test]
    fn resume_forwards_the_zero_based_page_unchanged() {
        // The provider must not pre-convert: the one conversion belongs to the launcher's command
        // line, so handing a one-based page onward would resume one page past the target.
        let recorder = RecordingLauncher::default();
        let provider =
            EvinceGvfsProvider::with_launcher(Box::new(recorder.clone()), Vec::new());
        let doc = Document::new("/tmp/document.pdf");

        provider.resume(&doc, 72).expect("the recording launcher accepts");

        let calls = recorder.calls.lock().expect("launcher poisoned");
        assert_eq!(calls.as_slice(), &[(doc.path.clone(), Some(72))]);
    }

    #[test]
    fn launching_without_a_page_uses_xdg_open() {
        let dir = TempDir::new("t9-xdg-open");
        let xdg_open = make_executable(&dir, "xdg-open", "#!/bin/sh\n");
        let spawner = RecordingSpawner::default();
        let launcher =
            XdgLauncher::with_spawner(Box::new(spawner.clone()), vec![dir.path.clone()]);

        let path = Path::new("/home/u/libros/Comparación.pdf");
        let outcome = launcher.launch(path, None).expect("xdg-open exists");
        assert_eq!(
            outcome,
            LaunchOutcome::Delegated {
                viewer: "xdg-open".to_string()
            }
        );

        let calls = spawner.calls.lock().expect("spawner poisoned");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, xdg_open);
        assert_eq!(calls[0].1, vec![path.as_os_str().to_os_string()]);
    }

    #[test]
    fn launching_at_a_page_uses_the_viewer_cli_with_the_one_based_index() {
        let dir = TempDir::new("t9-page");
        let evince = make_executable(&dir, "evince", "#!/bin/sh\n");
        let spawner = RecordingSpawner::default();
        let launcher =
            XdgLauncher::with_spawner(Box::new(spawner.clone()), vec![dir.path.clone()]);

        let path = Path::new("/home/u/libros/Comparación.pdf");
        let outcome = launcher.launch(path, Some(71)).expect("evince exists");
        assert_eq!(
            outcome,
            LaunchOutcome::Delegated {
                viewer: "evince".to_string()
            }
        );

        let calls = spawner.calls.lock().expect("spawner poisoned");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, evince);
        assert_eq!(
            calls[0].1,
            vec![
                OsString::from("-i"),
                OsString::from("72"),
                path.as_os_str().to_os_string()
            ]
        );
    }

    #[test]
    fn a_page_launch_prefers_evince_over_papers() {
        let dir = TempDir::new("t9-prefer");
        make_executable(&dir, "evince", "#!/bin/sh\n");
        make_executable(&dir, "papers", "#!/bin/sh\n");
        let spawner = RecordingSpawner::default();
        let launcher =
            XdgLauncher::with_spawner(Box::new(spawner.clone()), vec![dir.path.clone()]);

        launcher
            .launch(Path::new("/tmp/doc.pdf"), Some(0))
            .expect("a viewer exists");
        let calls = spawner.calls.lock().expect("spawner poisoned");
        assert_eq!(
            calls[0].0.file_name().and_then(|name| name.to_str()),
            Some("evince")
        );
    }

    #[test]
    fn a_missing_program_is_an_honest_error_not_a_silent_success() {
        let dir = TempDir::new("t9-missing");
        let spawner = RecordingSpawner::default();
        let launcher =
            XdgLauncher::with_spawner(Box::new(spawner.clone()), vec![dir.path.clone()]);

        assert!(launcher.launch(Path::new("/tmp/doc.pdf"), None).is_err());
        // A page request is no different when *nothing* can open the document: the fallback to the
        // default handler is tried, and it fails too. That is the honest error — not "no page
        // viewer", which is a degraded success the trait's contract requires us to deliver.
        assert!(launcher.launch(Path::new("/tmp/doc.pdf"), Some(3)).is_err());
        assert!(
            spawner.calls.lock().expect("spawner poisoned").is_empty(),
            "a failed launch must not attempt to start a program"
        );
    }

    #[test]
    fn a_page_request_without_a_page_capable_viewer_still_opens_the_document() {
        // The `Launcher` contract is explicit: a launcher that cannot honour a page request must
        // still open the document, because landing on page one is a degraded success and refusing to
        // open anything is worse for the user. The outcome names the program that actually ran, so
        // the caller can tell which of the two happened.
        let dir = TempDir::new("t9-degraded");
        let xdg_open = make_executable(&dir, XDG_OPEN, "#!/bin/sh\n");
        let spawner = RecordingSpawner::default();
        let launcher =
            XdgLauncher::with_spawner(Box::new(spawner.clone()), vec![dir.path.clone()]);

        let path = Path::new("/home/u/libros/Comparación.pdf");
        let outcome = launcher
            .launch(path, Some(71))
            .expect("a missing page-capable viewer is a degraded success, not an error");
        assert_eq!(
            outcome,
            LaunchOutcome::Delegated {
                viewer: XDG_OPEN.to_string()
            }
        );

        let calls = spawner.calls.lock().expect("spawner poisoned");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, xdg_open);
        assert_eq!(
            calls[0].1,
            vec![path.as_os_str().to_os_string()],
            "the default handler gets the path alone: it cannot be asked for a page"
        );
    }

    #[test]
    fn the_page_argument_conversion_happens_once() {
        assert_eq!(
            viewer_page_arguments(0, Path::new("/tmp/doc.pdf")),
            vec![
                OsString::from("-i"),
                OsString::from("1"),
                OsString::from("/tmp/doc.pdf")
            ]
        );
        assert_eq!(
            viewer_page_arguments(72, Path::new("/tmp/doc.pdf"))[1],
            OsString::from("73")
        );
        // A corrupt maximum page must not overflow and panic.
        assert_eq!(
            viewer_page_arguments(u32::MAX, Path::new("/tmp/doc.pdf"))[1],
            OsString::from((u64::from(u32::MAX) + 1).to_string())
        );
    }

    #[test]
    fn a_non_executable_file_is_not_a_program() {
        let dir = TempDir::new("t9-not-exec");
        std::fs::write(dir.path.join("gio"), b"#!/bin/sh\n").expect("write non-executable");
        assert_eq!(find_program(GIO, std::slice::from_ref(&dir.path)), None);
    }

    #[test]
    fn progress_providers_registers_the_evince_adapter() {
        let providers = progress_providers();
        assert_eq!(providers.len(), 1, "exactly the Evince adapter is registered");
        assert_eq!(providers[0].id(), EVINCE_ID);
        assert_eq!(providers[0].capability(), ProgressCapability::Live);
    }

    #[test]
    fn file_identity_is_implemented_rather_than_stubbed() {
        // Replaces the old stub assertion: the capability now exists, so a real file must yield a
        // real identity rather than `NotSupported`.
        let dir = TempDir::new("identity");
        let path = dir.path.join("document.pdf");
        std::fs::write(&path, b"%PDF-1.7 real").unwrap();

        let identity = PosixFileIdentity.identity(&path).unwrap();
        assert!(matches!(identity, FileId::Posix { .. }));

        // The Linux backend must expose the real implementation, not a private copy.
        let backend = backends();
        assert_eq!(
            backend.file_identity.identity(&path).unwrap(),
            identity
        );
    }

    #[test]
    fn not_supported_errors_name_the_capability_and_the_platform() {
        let error = not_supported("open-document detection", PLATFORM);
        let rendered = error.to_string();
        assert!(rendered.contains("open-document detection"), "{rendered}");
        assert!(rendered.contains("Linux"), "{rendered}");
    }

    #[test]
    fn backends_are_linux_identified() {
        let backends = backends();
        assert_eq!(backends.launcher.platform(), Platform::Linux);
        assert_eq!(backends.recent_documents.id(), "linux-xbel-recent");
        assert_eq!(backends.open_documents.id(), "linux-proc-fd");
        assert_eq!(backends.file_identity.id(), "linux-posix-identity");
        assert_eq!(backends.folder_watcher.id(), "linux-inotify");
        assert_eq!(backends.folder_watcher.platform(), Platform::Linux);
    }

    // --- T8: the `/proc` open-document detector ----------------------------------------------

    /// A committed PDF fixture, used only to give the detector a real `.pdf` target. The detector
    /// never parses it: on this axis only the extension and the regular-file type matter.
    const PDF_FIXTURE: &[u8] =
        include_bytes!("../../mirk-scan/tests/fixtures/pages-in-object-stream.pdf");

    /// Serialises the detector tests in this binary.
    ///
    /// They hold files open in the *same* process, and `/proc/<pid>/fd` cannot tell one test's
    /// descriptor from another's. Running them one at a time keeps the determinism test from
    /// observing another test's close as a change. Poisoning is irrelevant to these assertions, so
    /// the guard is recovered rather than unwrapped.
    static DETECTOR_TESTS: Mutex<()> = Mutex::new(());

    fn detector_test_guard() -> std::sync::MutexGuard<'static, ()> {
        DETECTOR_TESTS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Writes the committed fixture into `dir` and returns its canonical path.
    fn write_fixture(dir: &TempDir, name: &str) -> PathBuf {
        let path = dir.path.join(name);
        std::fs::write(&path, PDF_FIXTURE).expect("write the PDF fixture");
        canonical(&path)
    }

    #[test]
    fn a_held_open_pdf_is_reported_with_our_pid_and_real_viewer() {
        let _guard = detector_test_guard();
        let dir = TempDir::new("proc-held");
        let path = write_fixture(&dir, "held.pdf");
        let _held = std::fs::File::open(&path).expect("hold the fixture open");

        let documents = ProcOpenDocuments
            .open_documents()
            .expect("the snapshot succeeds");

        // Containment, not equality: other processes on this machine may legitimately hold PDFs
        // open, so an exact-vector assertion would be flaky.
        let ours = documents
            .iter()
            .find(|document| document.path == path && document.pid == Some(std::process::id()))
            .unwrap_or_else(|| panic!("a held-open PDF was not reported: {documents:?}"));

        let expected = std::env::current_exe()
            .expect("current executable")
            .file_name()
            .expect("executable file name")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            ours.viewer.as_deref(),
            Some(expected.as_str()),
            "the viewer must come from `/proc/<pid>/cmdline`, not from the pid"
        );
    }

    #[test]
    fn a_held_open_non_pdf_is_not_reported() {
        let _guard = detector_test_guard();
        let dir = TempDir::new("proc-txt");
        let path = dir.path.join("notes.txt");
        std::fs::write(&path, b"not a PDF").expect("write the text file");
        let path = canonical(&path);
        let _held = std::fs::File::open(&path).expect("hold the text file open");

        let documents = ProcOpenDocuments
            .open_documents()
            .expect("the snapshot succeeds");

        assert!(
            !documents.iter().any(|document| document.path == path),
            "a non-PDF descriptor was reported: {documents:?}"
        );
    }

    #[test]
    fn a_held_open_but_unlinked_file_is_not_reported() {
        let _guard = detector_test_guard();
        let dir = TempDir::new("proc-unlinked");
        let path = write_fixture(&dir, "vanished.pdf");
        let _held = std::fs::File::open(&path).expect("hold the fixture open");
        std::fs::remove_file(&path).expect("unlink while the descriptor is still open");

        let documents = ProcOpenDocuments
            .open_documents()
            .expect("the snapshot succeeds");

        assert!(
            !documents.iter().any(|document| document.path == path),
            "an unlinked descriptor became a suggestion for a path that no longer exists: {documents:?}"
        );
    }

    #[test]
    fn a_pdf_closed_before_the_snapshot_is_not_reported() {
        let _guard = detector_test_guard();
        let dir = TempDir::new("proc-closed");
        let path = write_fixture(&dir, "closed.pdf");
        {
            let _held = std::fs::File::open(&path).expect("open the fixture");
        } // the descriptor is released here

        let documents = ProcOpenDocuments
            .open_documents()
            .expect("the snapshot succeeds");

        assert!(
            !documents.iter().any(|document| document.path == path),
            "a file that is not open was reported: {documents:?}"
        );
    }

    #[test]
    fn two_consecutive_snapshots_are_identical_sorted_and_deduplicated() {
        let _guard = detector_test_guard();
        let dir = TempDir::new("proc-stable");
        let path = write_fixture(&dir, "stable.pdf");
        let _held = std::fs::File::open(&path).expect("hold the fixture open");

        let first = ProcOpenDocuments
            .open_documents()
            .expect("the first snapshot succeeds");
        let second = ProcOpenDocuments
            .open_documents()
            .expect("the second snapshot succeeds");

        // Stability is asserted for the entries this test controls, not for the whole machine. Other
        // processes — a browser with a PDF open — and sibling tests in this binary, which hold PDFs
        // open while they run, can legitimately change the snapshot between two calls. Asserting that
        // the entire system stood still would be a flake dressed up as a test; the properties that
        // *are* the contract (deduplication and order) are asserted below over the whole snapshot.
        let root = std::fs::canonicalize(&dir.path).expect("canonicalise the temporary directory");
        let controlled = |documents: &[OpenDocument]| -> Vec<OpenDocument> {
            documents
                .iter()
                .filter(|document| document.path.starts_with(&root))
                .cloned()
                .collect()
        };
        assert_eq!(
            controlled(&first),
            controlled(&second),
            "two snapshots must agree about the paths this test controls"
        );

        let mut pairs: Vec<(PathBuf, Option<u32>)> = first
            .iter()
            .map(|document| (document.path.clone(), document.pid))
            .collect();
        let total = pairs.len();
        pairs.sort();
        pairs.dedup();
        assert_eq!(
            pairs.len(),
            total,
            "a (path, pid) pair was reported more than once: {first:?}"
        );

        let mut sorted = first.clone();
        sorted.sort_by(|left, right| left.path.cmp(&right.path).then(left.pid.cmp(&right.pid)));
        assert_eq!(
            first, sorted,
            "the snapshot must be sorted by path then pid"
        );

        assert!(
            first
                .iter()
                .any(|document| document.path == path && document.pid == Some(std::process::id())),
            "the held-open fixture is missing from the snapshot: {first:?}"
        );
    }

    #[test]
    fn a_deleted_descriptor_target_is_recognised_by_its_suffix() {
        assert!(is_deleted(Path::new("/tmp/report.pdf (deleted)")));
        assert!(!is_deleted(Path::new("/tmp/report.pdf")));
        // A file whose *name* contains the marker but does not end with it is not deleted.
        assert!(!is_deleted(Path::new("/tmp/report (deleted).pdf")));
    }

    #[test]
    fn only_a_pdf_extension_is_accepted_and_case_does_not_matter() {
        assert!(is_pdf(Path::new("/tmp/report.pdf")));
        assert!(is_pdf(Path::new("/tmp/REPORT.PDF")));
        assert!(is_pdf(Path::new("/tmp/report.Pdf")));
        assert!(!is_pdf(Path::new("/tmp/report.pdf.txt")));
        assert!(!is_pdf(Path::new("/tmp/pdf")));
        assert!(!is_pdf(Path::new("/tmp/report")));
        assert!(!is_pdf(Path::new("/")));
    }

    #[test]
    fn the_viewer_falls_back_from_cmdline_to_comm() {
        // The `cmdline` path wins: the basename of `argv[0]`.
        assert_eq!(
            viewer_name(Some(b"/usr/bin/evince\0--page-index\0five\0"), Some(b"evince\n")),
            Some("evince".to_owned())
        );
        // A missing or empty `cmdline` falls back to `comm`, with the kernel's newline removed.
        assert_eq!(
            viewer_name(None, Some(b"zathura\n")),
            Some("zathura".to_owned())
        );
        assert_eq!(
            viewer_name(Some(b"\0"), Some(b"okular\n")),
            Some("okular".to_owned())
        );
        // Neither readable: honest `None`, never the pid rendered as a string.
        assert_eq!(viewer_name(None, None), None);
        assert_eq!(viewer_name(Some(b""), None), None);
        assert_eq!(viewer_name(None, Some(b"\n")), None);
    }

    #[test]
    fn the_first_argument_basename_ignores_the_rest_of_the_command_line() {
        assert_eq!(
            first_argument_basename(b"/usr/bin/evince\0-i\0ten\0book.pdf\0"),
            Some("evince".to_owned())
        );
        assert_eq!(
            first_argument_basename(b"zathura\0"),
            Some("zathura".to_owned())
        );
        assert_eq!(first_argument_basename(b""), None);
        assert_eq!(first_argument_basename(b"\0--flag\0"), None);
    }

    // --- T7a: the watched-folder watcher -----------------------------------------------------

    /// How long a test waits after starting the watcher before touching the filesystem.
    ///
    /// `watch` installs its watches on the thread it runs on, and the test cannot observe that
    /// moment through the trait. This bounds the startup race; every wait after it is
    /// event-driven with a timeout, never a sleep-poll.
    const WATCH_STARTUP_WAIT: Duration = Duration::from_millis(500);

    /// A watcher running on its own thread, with a bounded channel for the events it reports.
    ///
    /// `watch` blocks by contract, so the tests run it off-thread and read events with a timeout.
    /// Nothing here waits unbounded: a missing event fails an assertion instead of hanging.
    struct RunningWatcher {
        stop_flag: Arc<AtomicBool>,
        events: mpsc::Receiver<WatchEvent>,
        done: mpsc::Receiver<()>,
        thread: Option<std::thread::JoinHandle<Result<(), MirkError>>>,
    }

    impl RunningWatcher {
        fn start(targets: Vec<WatchTarget>) -> Self {
            let stop_flag = Arc::new(AtomicBool::new(false));
            let (events_tx, events) = mpsc::channel();
            let (done_tx, done) = mpsc::channel();
            let thread_stop = Arc::clone(&stop_flag);
            let thread = std::thread::spawn(move || {
                let mut sink = move |event: WatchEvent| {
                    // A send failure only means the test ended early; it is not the
                    // watcher's problem.
                    let _ = events_tx.send(event);
                };
                let result = LinuxFolderWatcher.watch(&targets, &thread_stop, &mut sink);
                let _ = done_tx.send(());
                result
            });
            // Let the spawned thread install its watches before the test writes anything.
            std::thread::sleep(WATCH_STARTUP_WAIT);
            Self {
                stop_flag,
                events,
                done,
                thread: Some(thread),
            }
        }

        fn next_within(&self, timeout: Duration) -> Option<WatchEvent> {
            self.events.recv_timeout(timeout).ok()
        }

        /// Fails if any event arrives during `quiet`.
        fn assert_silent_for(&self, quiet: Duration) {
            match self.events.recv_timeout(quiet) {
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {}
                Ok(event) => panic!("expected no event, got {event:?}"),
            }
        }

        /// Stops the loop and returns its result, failing the test if it does not stop promptly.
        fn stop(mut self) -> Result<(), MirkError> {
            self.stop_flag.store(true, Ordering::SeqCst);
            self.done
                .recv_timeout(Duration::from_secs(5))
                .expect("the watcher did not stop after the stop flag was set");
            self.thread
                .take()
                .expect("thread handle taken twice")
                .join()
                .expect("the watcher thread panicked")
        }
    }

    impl Drop for RunningWatcher {
        fn drop(&mut self) {
            // Best effort, so a failing assertion cannot leave a loop running.
            self.stop_flag.store(true, Ordering::SeqCst);
        }
    }

    /// The canonical form of a temporary directory.
    ///
    /// The watcher canonicalises its targets, so tests must compare against canonical paths or
    /// they would assert the pre-fix behaviour on any host whose temporary directory is itself
    /// reached through a symlink.
    fn canonical(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).expect("canonicalise the temporary directory")
    }

    /// Receives until `wanted` events have arrived or the deadline passes. Never unbounded.
    fn collect_events(
        watcher: &RunningWatcher,
        wanted: usize,
        deadline: Duration,
    ) -> Vec<WatchEvent> {
        let cutoff = Instant::now() + deadline;
        let mut collected = Vec::new();
        while collected.len() < wanted && Instant::now() < cutoff {
            if let Some(event) = watcher.next_within(Duration::from_millis(250)) {
                collected.push(event);
            }
        }
        collected
    }

    #[test]
    fn a_written_file_is_reported_once_after_it_is_closed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = canonical(dir.path());
        let watcher = RunningWatcher::start(vec![WatchTarget {
            path: root.clone(),
            recursive: false,
        }]);

        let path = root.join("document.pdf");
        let mut file = std::fs::File::create(&path).expect("create the document");
        file.write_all(b"%PDF-1.7 first half").expect("write");
        file.flush().expect("flush");

        // The writer still holds the file open, so it has not settled: nothing may be reported.
        watcher.assert_silent_for(Duration::from_millis(400));

        file.write_all(b" and the rest").expect("write the rest");
        drop(file); // closing the writer is what settles the file

        let event = collect_events(&watcher, 1, Duration::from_secs(5))
            .into_iter()
            .next()
            .expect("the closed file was never reported");
        assert_eq!(event.path, path);
        assert_eq!(event.kind, WatchEventKind::Added);

        // Reported once, not once per write.
        watcher.assert_silent_for(Duration::from_millis(400));

        watcher.stop().expect("the watcher stops cleanly");
    }

    #[test]
    fn a_burst_of_settles_collapses_to_one_event_per_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = canonical(dir.path());
        let watcher = RunningWatcher::start(vec![WatchTarget {
            path: root.clone(),
            recursive: false,
        }]);

        // Four paths, each written twice in a tight burst: eight settle points for four paths.
        // Coalescing per path must yield one event each, not one per write session.
        let names: Vec<PathBuf> = (0..4)
            .map(|index| root.join(format!("doc-{index}.pdf")))
            .collect();
        for session in 0..2 {
            for (index, path) in names.iter().enumerate() {
                std::fs::write(path, format!("%PDF session {session} doc {index}")).expect("write");
            }
        }

        let reported = collect_events(&watcher, names.len(), Duration::from_secs(5));
        let reported_paths: HashSet<PathBuf> =
            reported.iter().map(|event| event.path.clone()).collect();

        // Every distinct path appears exactly once, and the eight writes did not become eight
        // events.
        assert_eq!(reported.len(), names.len(), "expected one event per path: {reported:?}");
        assert_eq!(reported_paths, names.iter().cloned().collect::<HashSet<_>>());
        assert!(
            reported.len() < names.len() * 2,
            "the second write of each path was not coalesced"
        );

        watcher.assert_silent_for(Duration::from_millis(400));
        watcher.stop().expect("the watcher stops cleanly");
    }

    #[test]
    fn a_recursive_target_watches_a_subdirectory_created_later() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = canonical(dir.path());
        let watcher = RunningWatcher::start(vec![WatchTarget {
            path: root.clone(),
            recursive: true,
        }]);

        let nested = root.join("nested");
        std::fs::create_dir(&nested).expect("create the subdirectory");
        // The watcher learns about the new directory from the parent's IN_CREATE and installs the
        // watch asynchronously. Give it a bounded moment first, otherwise the file's creation can
        // race ahead of the watch and be genuinely, correctly invisible. This is a fixed wait,
        // never an unbounded one.
        std::thread::sleep(Duration::from_millis(500));

        let file = nested.join("later.pdf");
        std::fs::write(&file, b"%PDF-1.7 created later").expect("write");

        let cutoff = Instant::now() + Duration::from_secs(5);
        let mut found = None;
        while Instant::now() < cutoff && found.is_none() {
            if let Some(event) = watcher.next_within(Duration::from_millis(250)) {
                if event.path == file {
                    found = Some(event);
                }
            }
        }

        let event = found.expect("a file created in a later subdirectory was never reported");
        assert_eq!(event.kind, WatchEventKind::Added);
        watcher.stop().expect("the watcher stops cleanly");
    }

    #[test]
    fn a_removed_file_is_reported_as_removed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = canonical(dir.path());
        let file = root.join("gone.pdf");
        std::fs::write(&file, b"%PDF-1.7 present").expect("write before watching");

        let watcher = RunningWatcher::start(vec![WatchTarget {
            path: root.clone(),
            recursive: false,
        }]);

        std::fs::remove_file(&file).expect("remove");

        let cutoff = Instant::now() + Duration::from_secs(5);
        let mut removed = None;
        while Instant::now() < cutoff && removed.is_none() {
            if let Some(event) = watcher.next_within(Duration::from_millis(250)) {
                if event.kind == WatchEventKind::Removed {
                    removed = Some(event);
                }
            }
        }

        let event = removed.expect("the removed file was never reported");
        assert_eq!(event.path, file);
        watcher.stop().expect("the watcher stops cleanly");
    }

    #[test]
    fn a_directory_that_disappears_mid_watch_is_reported_and_does_not_end_the_loop() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = canonical(dir.path());
        let watcher = RunningWatcher::start(vec![WatchTarget {
            path: root.clone(),
            recursive: true,
        }]);

        let nested = root.join("vanishing");
        std::fs::create_dir(&nested).expect("create the subdirectory");
        // Bound the wait for the new subdirectory's own watch to be installed (S2).
        std::thread::sleep(Duration::from_millis(500));
        std::fs::remove_dir(&nested).expect("remove the subdirectory");

        let cutoff = Instant::now() + Duration::from_secs(5);
        let mut removed = None;
        while Instant::now() < cutoff && removed.is_none() {
            if let Some(event) = watcher.next_within(Duration::from_millis(250)) {
                if event.path == nested && event.kind == WatchEventKind::Removed {
                    removed = Some(event);
                }
            }
        }
        let event = removed.expect("the disappeared directory was never reported");
        assert_eq!(event.path, nested);

        // The loop is still alive and stops cleanly: a disappearing directory is an event, not a
        // fatal error.
        watcher
            .stop()
            .expect("the watcher stops cleanly after a directory disappears");
    }

    #[test]
    fn setting_the_stop_flag_ends_the_loop_promptly() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = canonical(dir.path());
        let watcher = RunningWatcher::start(vec![WatchTarget {
            path: root,
            recursive: false,
        }]);
        // Let the loop reach its steady state before asking it to stop.
        std::thread::sleep(Duration::from_millis(100));

        let asked_at = Instant::now();
        watcher.stop().expect("the watcher stops cleanly");
        assert!(
            asked_at.elapsed() < Duration::from_secs(2),
            "stopping took {:?}",
            asked_at.elapsed()
        );
    }

    #[test]
    fn the_watch_limit_failure_is_mapped_to_an_error_that_names_the_limit() {
        // `inotify_add_watch` reports the exhausted watch-descriptor limit as ENOSPC. On Linux
        // that errno is 28. The mapping must name the limit rather than report "no space left on
        // device", which would send the user looking at their disk.
        let error = map_watch_error(Path::new("/library"), io::Error::from_raw_os_error(28));
        let rendered = error.to_string();
        assert!(rendered.contains("watch"), "{rendered}");
        assert!(rendered.contains("limit"), "{rendered}");
        assert!(rendered.contains("max_user_watches"), "{rendered}");
        assert!(rendered.contains("/library"), "{rendered}");
    }

    #[test]
    fn a_watch_failure_that_is_not_a_limit_keeps_its_underlying_cause() {
        let error = map_watch_error(Path::new("/library"), io::Error::from_raw_os_error(2));
        match error {
            MirkError::Io(inner) => assert_eq!(inner.raw_os_error(), Some(2)),
            other => panic!("expected the io error to pass through, got {other:?}"),
        }
    }

    #[test]
    fn a_queue_overflow_is_classified_as_a_lost_observation() {
        // A real queue overflow cannot be induced deterministically from userspace, so the
        // classification is a pure function and is tested directly. Nothing here fakes a kernel
        // overflow.
        assert_eq!(
            watcher_observation(EventMask::Q_OVERFLOW),
            Some(WatchEventKind::Lost)
        );
        assert_eq!(watcher_observation(EventMask::CLOSE_WRITE), None);
        assert_eq!(watcher_observation(EventMask::CREATE), None);
        assert_eq!(watcher_observation(EventMask::IGNORED), None);
    }

    #[test]
    fn a_queue_overflow_reports_every_target_not_just_one_directory() {
        // A real overflow cannot be produced from userspace, but its *consequence* is deterministic
        // and is the part that matters: events for any watched directory may have been dropped, so
        // every target must be reported. Reporting only the directory the descriptor happens to name
        // would leave the rest of the library silently stale.
        let first = TempDir::new("overflow-first");
        let second = TempDir::new("overflow-second");
        std::fs::create_dir_all(first.path.join("nested")).expect("create a nested directory");
        let targets = vec![
            WatchTarget {
                path: first.path.clone(),
                recursive: true,
            },
            WatchTarget {
                path: second.path.clone(),
                recursive: false,
            },
        ];

        let mut watcher = WatchLoop::start(&targets).expect("start the watch loop");
        // A descriptor that *is* tracked, which is the case that used to narrow the report.
        let descriptor = watcher
            .watched
            .keys()
            .next()
            .cloned()
            .expect("the loop watches at least one directory");
        watcher
            .handle(RawEvent {
                descriptor,
                mask: EventMask::Q_OVERFLOW,
                name: None,
            })
            .expect("an overflow is not fatal");

        let mut reported = Vec::new();
        watcher.flush(&mut |event| reported.push(event), true);

        let lost: Vec<PathBuf> = reported
            .iter()
            .filter(|event| event.kind == WatchEventKind::Lost)
            .map(|event| event.path.clone())
            .collect();
        assert!(
            lost.contains(&first.path),
            "the recursive target is reported: {lost:?}"
        );
        assert!(
            lost.contains(&second.path),
            "and so is the non-recursive one: {lost:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_subdirectory_is_reported_as_unwatchable_and_the_rest_keeps_working() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let root = canonical(dir.path());
        let locked = root.join("locked");
        let visible = root.join("visible");
        std::fs::create_dir(&locked).expect("create the locked subdirectory");
        std::fs::create_dir(&visible).expect("create the visible subdirectory");
        std::fs::write(locked.join("hidden.pdf"), b"%PDF-1.7 hidden").expect("write hidden");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .expect("lock the subdirectory");

        // Root and some filesystems ignore the mode bits, so the condition cannot be created
        // there. Restore and skip with an explicit reason, never a silent pass.
        if std::fs::read_dir(&locked).is_ok() {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
                .expect("restore");
            eprintln!(
                "skipping: this account can read a 0o000 directory, so Unwatchable cannot be \
                 produced"
            );
            return;
        }

        let watcher = RunningWatcher::start(vec![WatchTarget {
            path: root.clone(),
            recursive: true,
        }]);

        let cutoff = Instant::now() + Duration::from_secs(5);
        let mut unwatchable = None;
        while Instant::now() < cutoff && unwatchable.is_none() {
            if let Some(event) = watcher.next_within(Duration::from_millis(250)) {
                if event.path == locked && event.kind == WatchEventKind::Unwatchable {
                    unwatchable = Some(event);
                }
            }
        }

        // Restore before any assertion, so the temporary directory can always be cleaned up.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
            .expect("restore");

        // The loop must still be watching everything else: a file created in `visible` arrives
        // even though a corner of the tree is unwatchable.
        let later = visible.join("later.pdf");
        std::fs::write(&later, b"%PDF-1.7 visible").expect("write visible");
        let cutoff = Instant::now() + Duration::from_secs(5);
        let mut saw_later = false;
        while Instant::now() < cutoff && !saw_later {
            if let Some(event) = watcher.next_within(Duration::from_millis(250)) {
                if event.path == later && event.kind == WatchEventKind::Added {
                    saw_later = true;
                }
            }
        }

        let event = unwatchable.expect("the unreadable subdirectory was never reported");
        assert_eq!(event.path, locked);
        assert_eq!(event.kind, WatchEventKind::Unwatchable);
        assert!(saw_later, "the rest of the tree stopped being watched");

        watcher.stop().expect("the watcher stops cleanly");
    }

    #[test]
    fn the_recursive_walk_classifies_per_entry_failures_as_unwatchable() {
        // A per-entry `read_dir` failure cannot be induced on demand, so it is fed as a synthetic
        // `Err` here. The other branches use real `DirEntry` values from a temporary directory.
        let broken: io::Result<std::fs::DirEntry> =
            Err(io::Error::other("entry could not be read"));
        assert_eq!(classify_entry(&broken), EntryDecision::Unwatchable);

        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir(dir.path().join("sub")).expect("create a subdirectory");
        std::fs::write(dir.path().join("file.pdf"), b"%PDF-1.7").expect("create a file");
        let decisions: Vec<EntryDecision> = std::fs::read_dir(dir.path())
            .expect("read the temp dir")
            .map(|entry| classify_entry(&entry))
            .collect();

        assert!(decisions.contains(&EntryDecision::Directory));
        assert!(decisions.contains(&EntryDecision::Ignore));
        assert!(!decisions.contains(&EntryDecision::Unwatchable));
    }

    #[test]
    fn a_directory_moved_out_of_the_tree_is_released_and_not_reported() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = canonical(dir.path());
        let root = base.join("root");
        let outside = base.join("outside");
        std::fs::create_dir(&root).expect("create root");
        std::fs::create_dir(&outside).expect("create outside");
        let sub = root.join("sub");
        std::fs::create_dir(&sub).expect("create sub");

        let watcher = RunningWatcher::start(vec![WatchTarget {
            path: root.clone(),
            recursive: true,
        }]);

        let moved = outside.join("sub");
        std::fs::rename(&sub, &moved).expect("move the directory out of the tree");

        // The move itself is reported as `Removed` for the old path.
        let cutoff = Instant::now() + Duration::from_secs(5);
        let mut saw_removed = false;
        while Instant::now() < cutoff && !saw_removed {
            if let Some(event) = watcher.next_within(Duration::from_millis(250)) {
                if event.path == sub && event.kind == WatchEventKind::Removed {
                    saw_removed = true;
                }
            }
        }
        assert!(
            saw_removed,
            "the move out of the tree was not reported as Removed for {}",
            sub.display()
        );

        // A file created in the moved directory is now outside the target and must stay invisible.
        std::fs::write(moved.join("later.pdf"), b"%PDF-1.7 outside").expect("write outside");

        let cutoff = Instant::now() + Duration::from_secs(2);
        let mut leaked = None;
        while Instant::now() < cutoff && leaked.is_none() {
            if let Some(event) = watcher.next_within(Duration::from_millis(250)) {
                if event.path.starts_with(&sub) {
                    leaked = Some(event);
                }
            }
        }
        watcher.stop().expect("the watcher stops cleanly");
        assert!(
            leaked.is_none(),
            "an event leaked through for a directory moved outside the target: {leaked:?}"
        );
    }

    #[test]
    fn releasing_a_moved_out_directory_drops_its_watches_and_bookkeeping() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = canonical(dir.path());
        let sub = root.join("sub");
        let deeper = sub.join("deeper");
        std::fs::create_dir_all(&deeper).expect("create sub/deeper");

        let targets = [WatchTarget {
            path: root.clone(),
            recursive: true,
        }];
        let mut loop_state = WatchLoop::start(&targets).expect("start the watch loop");

        assert!(loop_state.watched.values().any(|dir| dir.as_path() == sub));
        assert!(loop_state.watched.values().any(|dir| dir.as_path() == deeper));

        // Pretend the loop had already reported bookkeeping for the subtree.
        loop_state.seen.insert(sub.clone());
        loop_state.seen.insert(deeper.clone());
        loop_state.created.insert(deeper.clone());

        loop_state.release_subtree(&sub);

        assert!(
            loop_state.watched.values().all(|dir| {
                let dir = dir.as_path();
                dir != sub && !dir.starts_with(&sub)
            }),
            "a moved-out directory or one of its descendants is still watched"
        );
        assert!(
            loop_state.watched.values().any(|dir| dir.as_path() == root),
            "the root watch was released along with the subtree"
        );
        assert!(loop_state.seen.iter().all(|path| !path.starts_with(&sub)));
        assert!(
            loop_state.created.is_empty(),
            "stale `created` bookkeeping survived the release"
        );
    }

    #[test]
    fn releasing_a_subtree_drops_its_pending_events() {
        // A settled event already inside the coalescing window belongs to the tree that just left.
        // Delivering it afterwards would report a path outside the target, which is exactly what
        // releasing the watches is meant to prevent.
        let dir = tempfile::tempdir().expect("temp dir");
        let root = canonical(dir.path());
        let sub = root.join("sub");
        std::fs::create_dir(&sub).expect("create sub");
        let child = sub.join("settled.pdf");
        let kept = root.join("kept.pdf");

        let targets = [WatchTarget {
            path: root.clone(),
            recursive: true,
        }];
        let mut loop_state = WatchLoop::start(&targets).expect("start the watch loop");

        loop_state.enqueue(child.clone(), WatchEventKind::Added);
        loop_state.enqueue(kept.clone(), WatchEventKind::Added);

        loop_state.release_subtree(&sub);

        let mut reported = Vec::new();
        loop_state.flush(&mut |event| reported.push(event), true);
        let paths: Vec<PathBuf> = reported.iter().map(|event| event.path.clone()).collect();

        assert!(
            !paths.contains(&child),
            "an event for a path inside the released subtree was still delivered: {paths:?}"
        );
        assert!(
            paths.contains(&kept),
            "an unrelated pending event was dropped along with the subtree: {paths:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_target_is_watched_at_its_canonical_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = canonical(dir.path());
        let real = base.join("real");
        std::fs::create_dir(&real).expect("create the real directory");
        let link = base.join("link");
        std::os::unix::fs::symlink(&real, &link).expect("create the symlink");

        let watcher = RunningWatcher::start(vec![WatchTarget {
            path: link.clone(),
            recursive: true,
        }]);

        let created = real.join("document.pdf");
        std::fs::write(&created, b"%PDF-1.7 through the symlink").expect("write");

        let cutoff = Instant::now() + Duration::from_secs(5);
        let mut events = Vec::new();
        let mut saw_canonical = false;
        while Instant::now() < cutoff && !saw_canonical {
            if let Some(event) = watcher.next_within(Duration::from_millis(250)) {
                if event.path == created && event.kind == WatchEventKind::Added {
                    saw_canonical = true;
                }
                events.push(event);
            }
        }
        watcher.stop().expect("the watcher stops cleanly");

        assert!(
            saw_canonical,
            "the file was not reported under the canonical path {}: {events:?}",
            created.display()
        );
        assert!(
            events.iter().all(|event| !event.path.starts_with(&link)),
            "an event was reported under the symlink path: {events:?}"
        );
    }
}
