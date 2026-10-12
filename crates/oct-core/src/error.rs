//! The error type shared by every fallible operation in `oct`.
//!
//! Library code in this workspace does not panic. Anything that can fail returns
//! [`Result<_, OctError>`], because most of what `oct` does is read state written by *other*
//! applications: files that vanish, formats that change between releases, and sandboxes that deny
//! access are all ordinary outcomes rather than exceptional ones.

use crate::domain::Platform;

/// Everything that can go wrong.
///
/// The variants are chosen so callers can decide *how to degrade* rather than only *that
/// something failed*. In particular [`OctError::NotSupported`] is a normal, expected answer on a
/// platform that cannot provide a capability, and must never be presented to the user as a bug.
#[derive(Debug, thiserror::Error)]
pub enum OctError {
    /// The capability exists in the abstraction but has no implementation here.
    ///
    /// This is expected, not exceptional: `oct-platform` returns it honestly instead of
    /// fabricating a plausible-looking value.
    #[error("{capability} is not supported on {platform}")]
    NotSupported {
        /// The capability that is unavailable, for example `"open-document detection"`.
        capability: &'static str,
        /// The platform that cannot provide it.
        platform: Platform,
    },

    /// A filesystem or process operation failed.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// An individual provider failed while reading a viewer's state.
    ///
    /// Carries the provider id so a failing adapter can be identified, disabled and reported
    /// without taking the rest of the registry down with it.
    #[error("provider `{provider}` failed: {message}")]
    Adapter {
        /// The [`crate::ProgressProvider::id`] of the adapter that failed.
        provider: &'static str,
        /// A human-readable, non-fatal description of what went wrong.
        message: String,
    },

    /// A path could not be interpreted for the operation being attempted.
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// A persistence operation failed.
    ///
    /// Carries a description rather than a typed source, because `oct-core` deliberately has no
    /// database dependency; `oct-store` renders the underlying failure into this string at the
    /// boundary. Storage failures are ordinary outcomes — a full disk, a locked file, a database
    /// written by a newer build — and are surfaced rather than hidden.
    #[error("storage error: {0}")]
    Storage(String),
}
