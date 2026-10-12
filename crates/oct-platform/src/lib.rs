//! Operating-system backends for `oct`.
//!
//! Each implementation of the traits in [`oct_core::traits`] lives in a module gated by
//! `#[cfg(target_os = ...)]`, and the compiler picks exactly one. Nothing above this crate has to
//! know which platform it is running on.
//!
//! # Honest stubs
//!
//! Not every backend is implemented yet. Unimplemented ones return
//! [`oct_core::OctError::NotSupported`] rather than an empty success, and that distinction is
//! deliberate:
//!
//! - `Err(NotSupported)` means *"this build cannot answer that question"*;
//! - `Ok(vec![])` would mean *"I looked, and there are none"*.
//!
//! Returning the second when the first is true makes the interface show an empty library that is
//! indistinguishable from a real one, which is the failure mode this project exists to avoid.
//! Callers are therefore expected to surface `NotSupported` as a missing capability.

#[cfg(target_os = "linux")]
mod linux;

#[cfg(not(target_os = "linux"))]
mod unsupported;

use oct_core::{
    FileIdentity, Launcher, OctError, OpenDocumentDetector, ProgressProvider,
    RecentDocumentsSource,
};

/// The set of backends selected for this build.
///
/// Held as trait objects so the caller never names a concrete platform type, and so a platform can
/// swap an implementation without touching anything upstream.
pub struct Backends {
    /// Axis A: the desktop's list of recently opened documents.
    pub recent_documents: Box<dyn RecentDocumentsSource>,
    /// Axis B: what is open right now.
    pub open_documents: Box<dyn OpenDocumentDetector>,
    /// Axis C: opening a document, optionally at a page.
    pub launcher: Box<dyn Launcher>,
    /// Portable file identity, used to recognise documents across moves and renames.
    pub file_identity: Box<dyn FileIdentity>,
}

/// Every backend for the platform this binary was compiled for.
pub fn backends() -> Backends {
    #[cfg(target_os = "linux")]
    {
        linux::backends()
    }
    #[cfg(not(target_os = "linux"))]
    {
        unsupported::backends()
    }
}

/// Reading-progress providers available on this platform.
///
/// Empty until T9 lands the first real adapter. The registry built on top of this is the product's
/// extensibility point: one adapter per viewer, added without touching the core.
pub fn progress_providers() -> Vec<Box<dyn ProgressProvider>> {
    #[cfg(target_os = "linux")]
    {
        linux::progress_providers()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

/// Builds the standard "this capability is not implemented on this platform" error.
pub(crate) fn not_supported(capability: &'static str, platform: oct_core::Platform) -> OctError {
    OctError::NotSupported {
        capability,
        platform,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oct_core::Platform;

    #[test]
    fn the_selected_backends_report_the_compiled_platform() {
        let backends = backends();
        let expected = Platform::current();

        assert_eq!(backends.recent_documents.platform(), expected);
        assert_eq!(backends.open_documents.platform(), expected);
        assert_eq!(backends.launcher.platform(), expected);
    }

    #[test]
    fn backend_identifiers_are_unique() {
        // The identifiers are persisted and used to attribute progress, so a collision would
        // silently misattribute data.
        let backends = backends();
        let ids = [
            backends.recent_documents.id(),
            backends.open_documents.id(),
            backends.launcher.id(),
            backends.file_identity.id(),
        ];

        let mut sorted = ids.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "backend ids collided: {ids:?}");
    }

    #[test]
    fn progress_providers_are_unique_and_trackable() {
        // Empty today (T9 adds the first one). The invariant has to hold from the start, because
        // a duplicate id would corrupt stored provenance once real adapters arrive.
        let providers = progress_providers();
        let mut ids: Vec<&str> = providers.iter().map(|provider| provider.id()).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "provider ids collided: {ids:?}");
    }
}
