//! Proves the contributor-facing traits are **object-safe**.
//!
//! This is not a formality. The provider registry holds `Box<dyn ProgressProvider>`, and the whole
//! extensibility story — a contributor adding an adapter for a viewer `mirk` has never heard of —
//! depends on these traits being usable behind a trait object. A trait that silently stops being
//! object-safe breaks the architecture at the point where it matters most, so it is asserted here
//! rather than discovered later.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use mirk_core::{
    Availability, Confidence, Document, FileId, FileIdentity, FolderWatcher, LaunchOutcome,
    Launcher, MirkError, OpenDocument, OpenDocumentDetector, Platform, ProgressCapability,
    ProgressProvider, ProgressSource, RecentDocument, RecentDocumentsSource, StoredProgress,
    WatchEvent, WatchTarget,
};

/// A provider that implements every method with the cheapest honest answer.
///
/// Its existence doubles as documentation: this is exactly how little an adapter has to do, and
/// how a real one should behave before it knows anything.
struct MinimalProvider;

impl ProgressProvider for MinimalProvider {
    fn id(&self) -> &'static str {
        "minimal"
    }

    fn platform(&self) -> Platform {
        Platform::current()
    }

    fn viewer_ids(&self) -> &'static [&'static str] {
        &[]
    }

    fn capability(&self) -> ProgressCapability {
        ProgressCapability::None
    }

    fn detect(&self) -> Availability {
        Availability::Missing
    }

    fn read(&self, _doc: &Document) -> Result<Option<StoredProgress>, MirkError> {
        Ok(None)
    }

    fn resume(&self, doc: &Document, page: u32) -> Result<LaunchOutcome, MirkError> {
        let _ = (doc, page);
        Err(MirkError::NotSupported {
            capability: "resume",
            platform: self.platform(),
        })
    }
}

struct MinimalRecent;

impl RecentDocumentsSource for MinimalRecent {
    fn id(&self) -> &'static str {
        "minimal-recent"
    }

    fn platform(&self) -> Platform {
        Platform::current()
    }

    fn recent(&self, _limit: usize) -> Result<Vec<RecentDocument>, MirkError> {
        Ok(Vec::new())
    }
}

struct MinimalDetector;

impl OpenDocumentDetector for MinimalDetector {
    fn id(&self) -> &'static str {
        "minimal-detector"
    }

    fn platform(&self) -> Platform {
        Platform::current()
    }

    fn open_documents(&self) -> Result<Vec<OpenDocument>, MirkError> {
        Ok(Vec::new())
    }
}

struct MinimalLauncher;

impl Launcher for MinimalLauncher {
    fn id(&self) -> &'static str {
        "minimal-launcher"
    }

    fn platform(&self) -> Platform {
        Platform::current()
    }

    fn launch(&self, _path: &Path, _page: Option<u32>) -> Result<LaunchOutcome, MirkError> {
        Err(MirkError::NotSupported {
            capability: "launching documents",
            platform: self.platform(),
        })
    }
}

struct MinimalIdentity;

impl FileIdentity for MinimalIdentity {
    fn id(&self) -> &'static str {
        "minimal-identity"
    }

    fn identity(&self, _path: &Path) -> Result<FileId, MirkError> {
        Err(MirkError::NotSupported {
            capability: "file identity",
            platform: Platform::current(),
        })
    }
}

struct MinimalWatcher;

impl FolderWatcher for MinimalWatcher {
    fn id(&self) -> &'static str {
        "minimal-watcher"
    }

    fn platform(&self) -> Platform {
        Platform::current()
    }

    fn watch(
        &self,
        _targets: &[WatchTarget],
        _stop: &AtomicBool,
        _on_event: &mut dyn FnMut(WatchEvent),
    ) -> Result<(), MirkError> {
        Ok(())
    }
}

#[test]
fn every_platform_trait_is_object_safe() {
    // If any of these traits lost object safety, this test would not compile — which is the point.
    let providers: Vec<Box<dyn ProgressProvider>> = vec![Box::new(MinimalProvider)];
    let recent: Box<dyn RecentDocumentsSource> = Box::new(MinimalRecent);
    let detector: Box<dyn OpenDocumentDetector> = Box::new(MinimalDetector);
    let launcher: Box<dyn Launcher> = Box::new(MinimalLauncher);
    let identity: Box<dyn FileIdentity> = Box::new(MinimalIdentity);
    let watcher: Box<dyn FolderWatcher> = Box::new(MinimalWatcher);

    assert_eq!(providers.len(), 1);
    assert_eq!(providers[0].id(), "minimal");
    assert_eq!(recent.id(), "minimal-recent");
    assert_eq!(detector.id(), "minimal-detector");
    assert_eq!(launcher.id(), "minimal-launcher");
    assert_eq!(identity.id(), "minimal-identity");
    assert_eq!(watcher.id(), "minimal-watcher");
}

#[test]
fn a_registry_of_heterogeneous_providers_can_be_built() {
    // The realistic shape: several adapters, each contributing its own provenance, held together
    // without the caller knowing any concrete type.
    struct Second {
        id: &'static str,
    }

    impl ProgressProvider for Second {
        fn id(&self) -> &'static str {
            self.id
        }
        fn platform(&self) -> Platform {
            Platform::Linux
        }
        fn viewer_ids(&self) -> &'static [&'static str] {
            &["evince"]
        }
        fn capability(&self) -> ProgressCapability {
            ProgressCapability::Live
        }
        fn detect(&self) -> Availability {
            Availability::Installed
        }
        fn read(&self, _doc: &Document) -> Result<Option<StoredProgress>, MirkError> {
            Ok(Some(StoredProgress::new(
                72,
                Some(436),
                ProgressSource::External("evince-gvfs"),
                Confidence::Exact,
            )))
        }
        fn resume(&self, _doc: &Document, _page: u32) -> Result<LaunchOutcome, MirkError> {
            Ok(LaunchOutcome::Delegated {
                viewer: "evince".to_string(),
            })
        }
    }

    let registry: Vec<Box<dyn ProgressProvider>> = vec![
        Box::new(MinimalProvider),
        Box::new(Second { id: "evince-gvfs" }),
    ];

    let trackable: Vec<&str> = registry
        .iter()
        .filter(|p| p.capability().is_trackable())
        .map(|p| p.id())
        .collect();

    assert_eq!(trackable, vec!["evince-gvfs"]);

    let doc = Document::new("/tmp/x.pdf");
    let progress = registry[1]
        .read(&doc)
        .expect("read should not fail")
        .expect("value");
    assert_eq!(progress.fraction(), Some(72.0 / 436.0));
    assert_eq!(progress.source.id(), "evince-gvfs");
}

#[test]
fn adapters_are_send_and_sync_so_scanning_can_leave_the_ui_thread() {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}

    assert_send_sync::<dyn ProgressProvider>();
    assert_send_sync::<dyn RecentDocumentsSource>();
    assert_send_sync::<dyn OpenDocumentDetector>();
    assert_send_sync::<dyn Launcher>();
    assert_send_sync::<dyn FileIdentity>();
    assert_send_sync::<dyn FolderWatcher>();
}

#[test]
fn the_three_capabilities_are_distinguishable_in_the_interface() {
    // The capability is user-visible: it decides whether mirk warns before opening a document.
    assert_eq!(ProgressCapability::Live.id(), "live");
    assert_eq!(ProgressCapability::CloseTime.id(), "close-time");
    assert_eq!(ProgressCapability::None.id(), "none");
    assert_eq!(Availability::Installed.id(), "installed");
    assert_eq!(Availability::Unsupported.id(), "unsupported");
}
