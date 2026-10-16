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

    /// A value a caller supplied is not something this build can act on.
    ///
    /// Deliberately **not** [`OctError::Storage`]. A storage failure means the database or the disk
    /// refused an operation, so reporting a bad argument as one sends whoever reads a log looking at the
    /// wrong thing entirely. It is not [`OctError::InvalidPath`] either: that variant is about a path
    /// that could not be understood, while this one is any argument, named as the interface names it.
    ///
    /// The three fields are the point of the variant: an "invalid input" that does not say which value
    /// arrived, in which field, and what the build accepts instead cannot be acted on by anybody —
    /// neither by the person who triggered it nor by the person reading yesterday's log.
    #[error("{field} `{value}` is not valid: expected {expected}")]
    InvalidInput {
        /// The value's name, as the interface names it, for example `"locale"`.
        field: &'static str,
        /// What actually arrived, kept verbatim so a misbehaving caller is identifiable.
        value: String,
        /// What this build accepts, in the words the interface offers it.
        expected: String,
    },

    /// A path could not be interpreted for the operation being attempted.
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// A folder was refused because a watched folder already covers it.
    ///
    /// Deliberately **not** [`OctError::InvalidPath`]: that variant means the path itself could not be
    /// understood, while here the path is well formed and the folder exists — the request is refused
    /// because watching it would put the same tree under two watches and discover every document in
    /// it twice. Both paths travel as fields rather than baked into one sentence, so the interface
    /// can name the folder that already covers the user's choice and word the alert in its own
    /// language. "It was blocked", with no "by which one", is a half-answer.
    #[error("`{path}` is already covered by the watched folder `{covering}`, so it was not added")]
    PathCoveredByWatchedFolder {
        /// The folder whose addition was refused.
        path: String,
        /// The watched folder that already covers it.
        covering: String,
    },

    /// A native component this build needs is missing or could not be loaded.
    ///
    /// Deliberately **not** [`OctError::NotSupported`], which means the capability is not implemented
    /// for this platform at all. Here the capability exists and the *component* is absent: a setup
    /// problem the user can fix, not a missing feature, and reporting it as unsupported would tell a
    /// Linux user that Linux is unsupported. The underlying cause is kept verbatim so a wrong
    /// architecture is distinguishable from a missing file.
    #[error("{component} could not be loaded: {message}")]
    ComponentUnavailable {
        /// The component's name, for example `"pdfium"`.
        component: &'static str,
        /// The underlying cause, as reported by the loader.
        message: String,
    },

    /// A persistence operation failed.
    ///
    /// Carries a description rather than a typed source, because `oct-core` deliberately has no
    /// database dependency; `oct-store` renders the underlying failure into this string at the
    /// boundary. Storage failures are ordinary outcomes — a full disk, a locked file, a database
    /// written by a newer build — and are surfaced rather than hidden.
    #[error("storage error: {0}")]
    Storage(String),
}
