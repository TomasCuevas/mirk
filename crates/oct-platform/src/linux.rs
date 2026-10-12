//! Linux backends.
//!
//! This module is the shape of the platform layer; the behaviour arrives with its own tasks. Every
//! unimplemented method returns [`OctError::NotSupported`] rather than an empty success, so that a
//! missing capability is visible instead of looking like an empty library.
//!
//! Planned sources, for the implementer who picks this up:
//!
//! | Backend | Source of truth | Task |
//! | --- | --- | --- |
//! | Axis A | `~/.local/share/recently-used.xbel` (freedesktop Recent files, GTK) | T12 |
//! | Axis B | `/proc/<pid>/fd` plus `/proc/<pid>/cmdline` | T8 |
//! | Axis C | `xdg-open`, or a viewer CLI such as `evince -i <N>` | T7 |
//! | Identity | `st_dev` plus `st_ino` from `std::os::unix::fs::MetadataExt` | T11 |
//!
//! Two facts from the project's own measurements are worth carrying into that work:
//!
//! - `evince --page-index N` is **one-based** while the page Evince stores in GVFS metadata is
//!   **zero-based**. Converting at the wrong boundary resumes the reader one page off, silently.
//! - Metadata reads cost roughly 10 ms per document on the reference machine, so a scan over a
//!   large library must be incremental rather than a full re-read on every pass.

use std::path::Path;

use oct_core::{
    LaunchOutcome, Launcher, OctError, OpenDocument, OpenDocumentDetector, Platform,
    ProgressProvider, RecentDocument, RecentDocumentsSource,
};

use crate::{not_supported, Backends};

const PLATFORM: Platform = Platform::Linux;

/// Every Linux backend, in the state this build provides it.
pub(super) fn backends() -> Backends {
    Backends {
        recent_documents: Box::new(XbelRecentDocuments),
        open_documents: Box::new(ProcOpenDocuments),
        launcher: Box::new(XdgLauncher),
        file_identity: Box::new(PosixFileIdentity),
    }
}

/// Linux progress providers.
///
/// Empty until T9. The first real entry is the Evince adapter, reading
/// `metadata::evince::page` from the GVFS store at `~/.local/share/gvfs-metadata/`. It is
/// `ProgressCapability::Live` — measured, not assumed: the store updates within roughly three
/// seconds of a page change while the document is still open.
pub(super) fn progress_providers() -> Vec<Box<dyn ProgressProvider>> {
    // TODO(T9): register EvinceGvfsProvider here.
    Vec::new()
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

    fn recent(&self, _limit: usize) -> Result<Vec<RecentDocument>, OctError> {
        // TODO(T12): parse `~/.local/share/recently-used.xbel`, keep `application/pdf` entries,
        // skip entries whose file no longer exists, and return them newest first.
        Err(not_supported("recent-document ingestion", PLATFORM))
    }
}

/// Axis B — documents currently open, discovered by inspecting process file descriptors.
pub struct ProcOpenDocuments;

impl OpenDocumentDetector for ProcOpenDocuments {
    fn id(&self) -> &'static str {
        "linux-proc-fd"
    }

    fn platform(&self) -> Platform {
        PLATFORM
    }

    fn open_documents(&self) -> Result<Vec<OpenDocument>, OctError> {
        // TODO(T8): walk `/proc/<pid>/fd`, resolve symlinks, keep `.pdf` targets, read
        // `/proc/<pid>/comm` for the viewer name. Expect noise from thumbnailers and indexers —
        // the output feeds a review queue, not the library.
        Err(not_supported("open-document detection", PLATFORM))
    }
}

/// Axis C — opening documents through the desktop's own association mechanism.
pub struct XdgLauncher;

impl Launcher for XdgLauncher {
    fn id(&self) -> &'static str {
        "linux-xdg-open"
    }

    fn platform(&self) -> Platform {
        PLATFORM
    }

    fn launch(&self, _path: &Path, _page: Option<u32>) -> Result<LaunchOutcome, OctError> {
        // TODO(T7): `xdg-open` for the plain case; a viewer CLI (`evince -i N`, remembering the
        // one-based conversion) when a page is requested and that viewer is known.
        Err(not_supported("launching documents", PLATFORM))
    }
}

/// Portable identity on POSIX, from `st_dev` and `st_ino`.
///
/// The implementation lives in [`crate::posix`] so that every Unix backend shares it. Some
/// filesystems report unusable ids; the fallback there hashes content instead of guessing.
pub use crate::posix::PosixFileIdentity;

#[cfg(test)]
mod tests {
    use super::*;
    use oct_core::{FileId, FileIdentity};
    use std::path::PathBuf;

    /// A unique temporary directory that removes itself on drop, including on panic.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "oct-linux-{label}-{}",
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
    fn unimplemented_backends_report_not_supported_rather_than_empty_success() {
        let recent = XbelRecentDocuments.recent(10);
        assert!(matches!(recent, Err(OctError::NotSupported { .. })));

        let open = ProcOpenDocuments.open_documents();
        assert!(matches!(open, Err(OctError::NotSupported { .. })));
    }

    #[test]
    fn the_launcher_reports_not_supported_instead_of_pretending_to_open() {
        let outcome = XdgLauncher.launch(Path::new("/tmp/does-not-matter.pdf"), Some(0));
        assert!(matches!(outcome, Err(OctError::NotSupported { .. })));
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
    }
}
