//! Backends for platforms `oct` does not support yet.
//!
//! Windows and macOS are planned, and each needs its own module with real implementations. Until
//! then this module exists so the workspace **compiles everywhere** while still being honest: every
//! call reports [`OctError::NotSupported`].
//!
//! The alternative — a `compile_error!` — would make cross-compilation and host-agnostic linting
//! impossible, and would be a worse signal than a runtime answer that names the missing capability.

use std::path::Path;

use oct_core::{
    FileId, FileIdentity, LaunchOutcome, Launcher, OctError, OpenDocument, OpenDocumentDetector,
    Platform, RecentDocument, RecentDocumentsSource,
};

use crate::{not_supported, Backends};

/// The platform this binary was compiled for, which is not one oct supports.
fn platform() -> Platform {
    Platform::current()
}

/// Backends for an unsupported platform. All four report `NotSupported`.
pub(super) fn backends() -> Backends {
    Backends {
        recent_documents: Box::new(UnsupportedRecentDocuments),
        open_documents: Box::new(UnsupportedOpenDocuments),
        launcher: Box::new(UnsupportedLauncher),
        file_identity: Box::new(UnsupportedFileIdentity),
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

    fn recent(&self, _limit: usize) -> Result<Vec<RecentDocument>, OctError> {
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

    fn open_documents(&self) -> Result<Vec<OpenDocument>, OctError> {
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

    fn launch(&self, _path: &Path, _page: Option<u32>) -> Result<LaunchOutcome, OctError> {
        Err(not_supported("launching documents", platform()))
    }
}

/// File identity, unimplemented here.
pub struct UnsupportedFileIdentity;

impl FileIdentity for UnsupportedFileIdentity {
    fn id(&self) -> &'static str {
        "unsupported-identity"
    }

    fn identity(&self, _path: &Path) -> Result<FileId, OctError> {
        Err(not_supported("file identity", platform()))
    }
}
