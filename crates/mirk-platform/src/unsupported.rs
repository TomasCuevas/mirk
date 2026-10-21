//! Backends for platforms `mirk` does not support yet.
//!
//! Windows and macOS are planned, and each needs its own module with real implementations. Until
//! then this module exists so the workspace **compiles everywhere** while still being honest: every
//! call reports [`MirkError::NotSupported`].
//!
//! The alternative — a `compile_error!` — would make cross-compilation and host-agnostic linting
//! impossible, and would be a worse signal than a runtime answer that names the missing capability.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use mirk_core::{
    FileId, FileIdentity, FolderWatcher, LaunchOutcome, Launcher, MirkError, OpenDocument,
    OpenDocumentDetector, Platform, RecentDocument, RecentDocumentsSource, WatchEvent, WatchTarget,
};

use crate::{not_supported, Backends};

/// The platform this binary was compiled for, which is not one mirk supports.
fn platform() -> Platform {
    Platform::current()
}

/// Backends for an unsupported platform. All five report `NotSupported`.
pub(super) fn backends() -> Backends {
    Backends {
        recent_documents: Box::new(UnsupportedRecentDocuments),
        open_documents: Box::new(UnsupportedOpenDocuments),
        launcher: Box::new(UnsupportedLauncher),
        file_identity: Box::new(UnsupportedFileIdentity),
        folder_watcher: Box::new(UnsupportedFolderWatcher),
    }
}

/// Axis A, unimplemented here.
pub struct UnsupportedRecentDocuments;

impl RecentDocumentsSource for UnsupportedRecentDocuments {
    fn id(&self) -> &'static str {
        "unsupported-recent"
    }

    fn platform(&self) -> Platform {
        platform()
    }

    fn recent(&self, _limit: usize) -> Result<Vec<RecentDocument>, MirkError> {
        Err(not_supported("recent-document ingestion", platform()))
    }
}

/// Axis B, unimplemented here.
pub struct UnsupportedOpenDocuments;

impl OpenDocumentDetector for UnsupportedOpenDocuments {
    fn id(&self) -> &'static str {
        "unsupported-open-documents"
    }

    fn platform(&self) -> Platform {
        platform()
    }

    fn open_documents(&self) -> Result<Vec<OpenDocument>, MirkError> {
        Err(not_supported("open-document detection", platform()))
    }
}

/// Axis C, unimplemented here.
pub struct UnsupportedLauncher;

impl Launcher for UnsupportedLauncher {
    fn id(&self) -> &'static str {
        "unsupported-launcher"
    }

    fn platform(&self) -> Platform {
        platform()
    }

    fn launch(&self, _path: &Path, _page: Option<u32>) -> Result<LaunchOutcome, MirkError> {
        Err(not_supported("launching documents", platform()))
    }
}

/// File identity, unimplemented here.
pub struct UnsupportedFileIdentity;

impl FileIdentity for UnsupportedFileIdentity {
    fn id(&self) -> &'static str {
        "unsupported-identity"
    }

    fn identity(&self, _path: &Path) -> Result<FileId, MirkError> {
        Err(not_supported("file identity", platform()))
    }
}

/// Watched folders, unimplemented here.
pub struct UnsupportedFolderWatcher;

impl FolderWatcher for UnsupportedFolderWatcher {
    fn id(&self) -> &'static str {
        "unsupported-folder-watcher"
    }

    fn platform(&self) -> Platform {
        platform()
    }

    fn watch(
        &self,
        _targets: &[WatchTarget],
        _stop: &AtomicBool,
        _on_event: &mut dyn FnMut(WatchEvent),
    ) -> Result<(), MirkError> {
        Err(not_supported("folder watching", platform()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same rule the other stubs follow: a non-Linux build must say it cannot watch, never
    /// return `Ok(())`. An empty success would be indistinguishable from "I watched and nothing
    /// happened", which is the failure mode this project exists to avoid.
    #[test]
    fn the_watcher_stub_reports_not_supported_rather_than_success() {
        let stop = AtomicBool::new(false);
        let mut sink = |_event: WatchEvent| {};
        let result = UnsupportedFolderWatcher.watch(&[], &stop, &mut sink);
        assert!(matches!(result, Err(MirkError::NotSupported { .. })));
    }
}
