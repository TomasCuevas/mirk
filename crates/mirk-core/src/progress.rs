//! Reading progress, and where it came from.
//!
//! # Why provenance is part of the type
//!
//! `mirk` learns a reading position from several places, and they are **not equally trustworthy**:
//!
//! - its own reader, which knows the page exactly and continuously;
//! - an external viewer, whose stored state is written at some moment `mirk` does not control;
//! - a human, typing a page number to correct a wrong value.
//!
//! If all three collapsed into a single `page: u32` column, the interface would have to present
//! them identically — and it would silently lie. A precise-looking "40%" that actually means "the
//! last time this file was closed, four months ago" is worse than no number at all, because the
//! user cannot tell the difference.
//!
//! So [`StoredProgress`] carries both a [`ProgressSource`] and a [`Confidence`], and the interface
//! is expected to say things like *"page 12 of 80 — recorded when Evince closed"*.

/// What a viewer is capable of telling `mirk` about reading position.
///
/// This is a property of the *viewer*, declared by its provider, and it is user-visible: `mirk`
/// warns before opening a document with a viewer whose stored progress cannot be tracked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProgressCapability {
    /// Position is observable while the document is open. Evince on Linux behaves this way.
    Live,
    /// Position is only written when the document is closed abruptly or normally.
    CloseTime,
    /// The viewer exposes nothing usable. Browsers are the common case.
    None,
}

impl ProgressCapability {
    /// A stable machine-readable identifier.
    pub const fn id(self) -> &'static str {
        match self {
            ProgressCapability::Live => "live",
            ProgressCapability::CloseTime => "close-time",
            ProgressCapability::None => "none",
        }
    }

    /// Whether this viewer can contribute any progress at all.
    pub const fn is_trackable(self) -> bool {
        !matches!(self, ProgressCapability::None)
    }
}

/// How much a recorded position should be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Confidence {
    /// Observed directly and currently accurate.
    Exact,
    /// Accurate as of the moment the viewer last persisted it; may be stale.
    CloseTime,
    /// Corrected deliberately by a human. Trusted over any automatic source.
    Manual,
    /// Recorded, but the provenance is unclear. Presented without a precision claim.
    Unknown,
}

/// Which mechanism produced a reading position.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProgressSource {
    /// `mirk`'s own reader.
    Native,
    /// Entered or corrected by the user.
    Manual,
    /// An external viewer, identified by its [`crate::ProgressProvider::id`].
    External(&'static str),
}

impl ProgressSource {
    /// A stable machine-readable identifier, suitable for persisting.
    pub const fn id(&self) -> &'static str {
        match self {
            ProgressSource::Native => "native",
            ProgressSource::Manual => "manual",
            ProgressSource::External(id) => id,
        }
    }
}

/// A reading position together with its provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProgress {
    /// Zero-based page index.
    ///
    /// **Zero-based deliberately.** Viewers disagree about this, and the difference is a silent
    /// off-by-one bug. `evince --page-index N` is one-based while its stored metadata is
    /// zero-based; adapters convert at the boundary and this type only ever holds the zero-based
    /// form.
    pub page: u32,
    /// Total pages, when known.
    pub total: Option<u32>,
    /// Which mechanism produced this value.
    pub source: ProgressSource,
    /// How much to trust it.
    pub confidence: Confidence,
    /// When the reader read this, in Unix seconds, as reported by the source that observed it.
    ///
    /// `None` is not "long ago": it is "no source said", and it must stay distinguishable from a
    /// time. This is a **different fact** from when the app wrote the row, which is why it travels
    /// as its own field rather than reusing the row's write time.
    pub read_at: Option<i64>,
}

impl StoredProgress {
    /// Creates a position with explicit provenance.
    ///
    /// No reading time is attached: a caller that does not know when something was read must not be
    /// taken to claim it does. Attach one with [`StoredProgress::with_read_at`].
    pub fn new(page: u32, total: Option<u32>, source: ProgressSource, confidence: Confidence) -> Self {
        Self {
            page,
            total,
            source,
            confidence,
            read_at: None,
        }
    }

    /// The same value with a reading time attached.
    ///
    /// The constructor keeps its current shape so existing callers do not grow an argument they do
    /// not have. `None` remains available and means "no source said" — see
    /// [`StoredProgress::read_at`].
    pub fn with_read_at(mut self, read_at: Option<i64>) -> Self {
        self.read_at = read_at;
        self
    }

    /// Fraction read, in `0.0..=1.0`, or [`None`] when the total page count is unknown.
    ///
    /// Returns [`None`] rather than guessing, because a made-up denominator is exactly the kind of
    /// fabricated precision this module exists to prevent.
    pub fn fraction(&self) -> Option<f64> {
        let total = self.total?;
        if total == 0 {
            return None;
        }
        Some((f64::from(self.page) / f64::from(total)).clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(page: u32, total: Option<u32>) -> StoredProgress {
        StoredProgress::new(page, total, ProgressSource::Native, Confidence::Exact)
    }

    #[test]
    fn fraction_is_none_without_a_total() {
        assert_eq!(progress(10, None).fraction(), None);
    }

    #[test]
    fn fraction_uses_the_page_total_ratio() {
        let fraction = progress(20, Some(80)).fraction();
        assert_eq!(fraction, Some(0.25));
    }

    #[test]
    fn fraction_is_clamped_when_the_total_is_wrong() {
        // A viewer can report a stale total; the result must stay usable rather than exceed 1.0.
        assert_eq!(progress(90, Some(80)).fraction(), Some(1.0));
    }

    #[test]
    fn fraction_is_none_for_a_zero_total() {
        assert_eq!(progress(0, Some(0)).fraction(), None);
    }

    #[test]
    fn only_none_is_untrackable() {
        assert!(ProgressCapability::Live.is_trackable());
        assert!(ProgressCapability::CloseTime.is_trackable());
        assert!(!ProgressCapability::None.is_trackable());
    }

    #[test]
    fn external_source_reports_the_provider_id() {
        assert_eq!(ProgressSource::External("evince-gvfs").id(), "evince-gvfs");
        assert_eq!(ProgressSource::Native.id(), "native");
        assert_eq!(ProgressSource::Manual.id(), "manual");
    }
}
