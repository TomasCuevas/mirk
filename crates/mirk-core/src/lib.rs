//! Domain types and platform-facing traits for `mirk`.
//!
//! This crate is deliberately **dependency-free apart from `thiserror`**. It must not depend on
//! Tauri, SQLite, GTK, or any OS API. Everything here is a value type, a trait, or an error — the
//! vocabulary the rest of the workspace speaks.
//!
//! # Why the traits live here
//!
//! `mirk` has to run on Linux, Windows and macOS, and each one exposes reading progress through a
//! completely different mechanism. Rather than scattering `#[cfg]` through the codebase, every
//! OS-specific capability is expressed as a trait in [`traits`], and `mirk-platform` supplies the
//! implementation the compiler selects for the target.
//!
//! The traits in this crate are a **public interface**. Contributors will implement
//! [`ProgressProvider`] to teach `mirk` about their favourite PDF viewer, so the contracts here are
//! documented and stable-by-intent.
//!
//! # The three platform axes
//!
//! | Axis | Trait | Question it answers |
//! | --- | --- | --- |
//! | A. Recent documents | [`RecentDocumentsSource`] | Which PDFs did the user open recently? |
//! | B. Live detection | [`OpenDocumentDetector`] | What is open *right now*? |
//! | C. Launch | [`Launcher`] | How do I open a document at a given page? |
//! | D. Watched folders | [`FolderWatcher`] | Which PDFs changed, and when are they settled? |
//!
//! Reading progress itself comes from [`ProgressProvider`], and file identity from
//! [`FileIdentity`] — identity is an axis of its own because `(device, inode)` does not exist on
//! Windows. `FolderWatcher` is the sixth trait: watching a folder is an OS-specific capability
//! like the others, so it lives behind a trait rather than scattering `#[cfg]` through the
//! product.

pub mod domain;
pub mod error;
pub mod progress;
pub mod traits;
pub mod uri;

pub use domain::{
    CoverChoice, CoverSource, Document, FileId, OpenDocument, Platform, RecentDocument,
};
pub use error::MirkError;
pub use progress::{Confidence, ProgressCapability, ProgressSource, StoredProgress};
pub use traits::{
    Availability, FileIdentity, FolderWatcher, LaunchOutcome, Launcher, OpenDocumentDetector,
    ProgressProvider, RecentDocumentsSource, WatchEvent, WatchEventKind, WatchTarget,
};
pub use uri::file_uri_from_path;
