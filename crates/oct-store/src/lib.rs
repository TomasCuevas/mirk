//! Persistence for `oct`.
//!
//! Nothing is implemented yet — the schema, migrations and queries arrive with **T5**. This module
//! records what belongs here and, more importantly, *why* it is shaped that way, so the schema is
//! not reinvented from scratch.
//!
//! # Tables this crate will own
//!
//! | Table | Purpose |
//! | --- | --- |
//! | `documents` | One row per PDF: path, portable identity, size, mtime, page count, title, author, how it was discovered, when first and last seen. |
//! | `progress` | Reading position **with its provenance**: page, total, `source`, `confidence`, when recorded. |
//! | `open_events` | Intervals during which a document was open: document, process, viewer, start, end. |
//! | `viewers` | Known viewers and their declared capability, so it is not recomputed on every launch. |
//! | `folders` | Directories the user nominated as the source of truth, plus whether the watcher is enabled. |
//! | `suggestions` | Documents discovered outside watched folders, awaiting the user's accept-or-discard decision. |
//!
//! # The two decisions worth preserving
//!
//! **`progress` keeps `source` and `confidence` as separate columns.** A position read from
//! Evince's store, one typed in by a person, and one produced by `oct`'s own reader are not the
//! same kind of fact, and they do not deserve the same authority. Collapsing them into a single
//! integer produces an interface that cannot say *"page 12 of 80, recorded when Evince closed"*
//! and therefore silently presents stale data as current.
//!
//! **`documents` stores portable identity, never only a path.** Paths change when a file is moved
//! or renamed; identity is what lets the library follow a document instead of treating it as new
//! and losing its reading history. Because `(device, inode)` does not exist on Windows, identity
//! is the [`oct_core::FileId`] enum — and because some filesystems expose nothing usable, a content
//! hash is a first-class variant rather than a hidden fallback.
//!
//! # Where the database lives
//!
//! Per-platform user data directories, not the repository and not the current working directory:
//! `$XDG_DATA_HOME/oct` on Linux, `%APPDATA%` on Windows, `~/Library/Application Support` on
//! macOS.

// TODO(T5): schema, migrations, and the queries behind the tables described above.
