//! Persistence for `oct`.
//!
//! This crate owns the SQLite database: its schema, its forward-only migrations, and the small
//! query surface the rest of the workspace currently calls. It is deliberately not a rich data
//! access layer yet — see "What is deliberately deferred" below.
//!
//! # Tables this crate owns
//!
//! | Table | Purpose |
//! | --- | --- |
//! | `documents` | One row per PDF: path, portable identity, size, mtime, page count, title, author, how it was discovered, when first and last seen. |
//! | `authors` | One row per person the readers named, deduplicated by the trimmed, lowercased name (R42/S2). |
//! | `document_authors` | A document's readers' authors, **ordered**: `position` is the reader's order (R42/S3). |
//! | `progress` | Reading position **with its provenance**: page, total, `source`, `confidence`, when read, when recorded. |
//! | `open_events` | Intervals during which a document was open: document, process, viewer, start, end. |
//! | `viewers` | Known viewers and their declared capability, so it is not recomputed on every launch. |
//! | `folders` | Directories the user nominated as the source of truth, plus whether the watcher is enabled. |
//! | `settings` | A small key/value store for choices the interface and the desktop process share; the first is the interface language, which the tray follows (R1). |
//! | `suggestions` | Documents discovered outside watched folders, awaiting the user's accept-or-discard decision. |
//!
//! # The two decisions worth preserving
//!
//! **`progress` keeps `source` and `confidence` as separate columns.** A position read from
//! Evince's store, one typed in by a person, and one produced by `oct`'s own reader are not the
//! same kind of fact, and they do not deserve the same authority. Collapsing them into a single
//! integer produces an interface that cannot say *"page 12 of 80, recorded when Evince closed"*
//! and therefore silently presents stale data as current. Two rows with the same page but
//! different provenance remain two rows.
//!
//! **A document's authors are rows, not a string.** `documents.author` is the PDF's own embedded
//! author — one opaque string, read-only, never split (R42/S6) — while the authors a reader chose are
//! their own ordered list across `authors` and `document_authors`, so "several authors per work" is
//! stored as facts instead of as a delimiter somebody would have to guess at. The rule that turns the
//! two into what a screen shows lives in one place, [`oct_core::Document::display_authors`], so the
//! card, the filter and the search cannot disagree about who wrote a document (R42/S4).
//!
//! **`documents` stores portable identity, never only a path.** Paths change when a file is moved
//! or renamed; identity is what lets the library follow a document instead of treating it as new
//! and losing its reading history. Because `(device, inode)` does not exist on Windows, identity
//! is the [`oct_core::FileId`] enum — and because some filesystems expose nothing usable, a content
//! hash is a first-class variant rather than a hidden fallback. All three variants are stored in
//! the `file_id_kind` / `volume_id` / `file_index` / `content_hash` columns and can be looked up
//! without a path. The numeric ids are stored as decimal TEXT so the full `u64` range survives
//! SQLite's signed 64-bit `INTEGER`.
//!
//! # Migrations
//!
//! The schema version is recorded inside the database with SQLite's own `PRAGMA user_version`.
//! Opening a database brings it to [`Store::CURRENT_SCHEMA_VERSION`] and records that version;
//! reopening an already-current database is a no-op; a fresh file is created at the current
//! version. Migrations are forward-only: a database written by a newer build is refused rather
//! than silently downgraded.
//!
//! # Where the database lives
//!
//! Per-platform user data directories, not the repository and not the current working directory:
//! `$XDG_DATA_HOME/oct` on Linux (falling back to `~/.local/share/oct`), `%APPDATA%\oct` on
//! Windows, and `~/Library/Application Support/oct` on macOS. [`database_path`] resolves it.
//!
//! # What is deliberately deferred
//!
//! T5 implements the schema, the migrations and only the queries needed to prove the schema
//! contract, portable identity, provenance round-tripping and zero-based pages. The following are
//! **intentionally absent because nothing calls them yet**; they arrive with their callers rather
//! than as speculative SQL:
//!
//! - filtering and sorting over `documents`: [`Store::library`] returns every document with its
//!   latest progress, ordered by path, and nothing narrower — the first screen needed exactly that
//!   and no more;
//! - writes and reads for `open_events` and `viewers`; the `folders` table has only what its callers
//!   need: [`Store::record_folder`], [`Store::folders`], [`Store::forget_folder`],
//!   [`Store::folder_removal`], [`WatchedFolder`] and the pure [`covering_folder`] rule that
//!   decides whether a folder is already covered by a watch (R20, R22, R27);
//! - the `suggestions` table is written and read only through the queue's own four methods —
//!   [`Store::record_suggestion`], [`Store::suggestions`], [`Store::suggestion`] and
//!   [`Store::decide_suggestion`] — because the suggestion queue is the only caller, and a decision
//!   is the reader's (R4);
//! - metadata writes (`size`, `mtime`, `discovered_via`, `platform`, `status`) — the
//!   [`oct_core::Document`] value type does not carry them yet, so [`Store::upsert_document`]
//!   leaves those columns unset for the scanner (T6) to fill;
//! - path normalisation: paths are stored as opaque UTF-8 and are **never** Unicode-normalised
//!   (risk R7), so `path_normalized` deliberately holds the same bytes as `path`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use oct_core::{Confidence, Document, FileId, OctError, Platform, ProgressSource, StoredProgress};
use rusqlite::{params, Connection, OptionalExtension};

/// The application directory name appended to each platform's user data directory.
const APP_DIR: &str = "oct";

/// The database file name inside [`APP_DIR`].
const DATABASE_FILE: &str = "oct.sqlite3";

/// The version-1 schema. Later migrations are added below it, never edited in place.
const SCHEMA_V1: &str = "
CREATE TABLE documents (
    id              INTEGER PRIMARY KEY,
    path            TEXT    NOT NULL,
    path_normalized TEXT    NOT NULL,
    uri             TEXT    NOT NULL,
    file_id_kind    TEXT,
    volume_id       TEXT,
    file_index      TEXT,
    content_hash    TEXT,
    size            INTEGER,
    mtime           INTEGER,
    title           TEXT,
    author          TEXT,
    pages           INTEGER,
    pages_source    TEXT,
    discovered_via  TEXT,
    platform        TEXT,
    first_seen_at   INTEGER NOT NULL,
    last_opened_at  INTEGER,
    status          TEXT
);

-- Identity index used by the lookups.
CREATE INDEX idx_documents_identity
    ON documents (file_id_kind, volume_id, file_index, content_hash);

-- And the same invariant as a constraint: one row per *known* identity, so a second writer that
-- races the read-then-write in `upsert_document` fails honestly instead of silently forking a
-- document into two rows with two reading histories.
--
-- COALESCE is load-bearing, not decoration. A plain composite unique index over these columns
-- constrains nothing: every row of a given kind leaves the other kinds' columns NULL, and SQLite
-- treats each NULL as distinct, so no two rows would ever be considered equal. Normalising NULL to
-- an empty string inside the index makes the comparison total. The partial predicate keeps rows
-- with no identity out of the constraint entirely, which they must be: they are resolved by path,
-- and a path may legitimately have held more than one document.
CREATE UNIQUE INDEX idx_documents_identity_unique
    ON documents (
        COALESCE(file_id_kind, ''),
        COALESCE(volume_id, ''),
        COALESCE(file_index, ''),
        COALESCE(content_hash, '')
    )
    WHERE file_id_kind IS NOT NULL;

CREATE TABLE viewers (
    id          INTEGER PRIMARY KEY,
    exe         TEXT,
    desktop_id  TEXT,
    bundle_id   TEXT,
    platform    TEXT    NOT NULL,
    capability  TEXT    NOT NULL,
    detail      TEXT
);

CREATE TABLE progress (
    id          INTEGER PRIMARY KEY,
    doc_id      INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    page        INTEGER NOT NULL,
    total       INTEGER,
    source      TEXT    NOT NULL,
    confidence  TEXT    NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX idx_progress_doc ON progress (doc_id);

CREATE TABLE open_events (
    id          INTEGER PRIMARY KEY,
    doc_id      INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    pid         INTEGER,
    viewer_id   INTEGER REFERENCES viewers(id),
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER
);

CREATE TABLE folders (
    id          INTEGER PRIMARY KEY,
    path        TEXT    NOT NULL,
    recursive   INTEGER NOT NULL DEFAULT 0,
    enabled     INTEGER NOT NULL DEFAULT 1,
    added_at    INTEGER NOT NULL
);

CREATE TABLE suggestions (
    id            INTEGER PRIMARY KEY,
    path          TEXT    NOT NULL,
    seen_count    INTEGER NOT NULL DEFAULT 0,
    first_seen_at INTEGER NOT NULL,
    decided_at    INTEGER
);
";

/// Version 2: folders are recorded once, enforced by the database and not only by the read-then-write
/// in [`Store::record_folder`].
///
/// Added as a migration rather than by editing [`SCHEMA_V1`], because V1 is **frozen**: a database
/// created by an earlier build already reports version 1, and editing the V1 text would leave it
/// without the constraint forever while claiming otherwise. `folders.path` is `NOT NULL`, so unlike
/// the identity index on `documents` there is no NULL-distinct trap to work around here. The scanner
/// canonicalises the root before recording it, so two spellings of one folder are still possible
/// from a careless caller; that is a documented limitation this constraint cannot see.
const SCHEMA_V2: &str = "
CREATE UNIQUE INDEX idx_folders_path_unique ON folders (path);
";

/// Version 3: one place for the settings the interface and the desktop process share.
///
/// Added as a migration rather than by editing [`SCHEMA_V1`] or [`SCHEMA_V2`], because both are
/// **frozen**: a database an earlier build already created reports their versions, and editing the
/// text would leave that database without the table forever while claiming otherwise. The first
/// setting is the interface language — the desktop process reads it to word the tray (R1) — so the
/// table exists before any value does, and stays generic rather than growing a column per setting.
const SCHEMA_V3: &str = "
CREATE TABLE settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
";

/// Version 4: `progress` records **when the reader read**, separately from when the app wrote the
/// row.
///
/// `updated_at` keeps its meaning untouched — when the app wrote the row — and gains a sibling,
/// `read_at`, which only a source that observed a reading may fill. A `NULL` says the app **does
/// not know**, and that is a distinct fact from "read long ago": the launch sweep writes a position
/// for every known document, so a whole library answers with one moment, and inferring a reading
/// time from it would fabricate evidence the app never had.
///
/// Added as a migration rather than by editing an earlier schema, because all of them are
/// **frozen**: a database an earlier build already created reports their versions, and editing the
/// text would leave that database without the column forever while claiming otherwise. `ALTER TABLE
/// ... ADD COLUMN` is idempotent in the sense that matters here because the recorded version gates
/// it: it runs only on the V3 -> V4 step, once, and the step is stamped in its own transaction.
const SCHEMA_V4: &str = "
ALTER TABLE progress ADD COLUMN read_at INTEGER;
";

/// Version 5: the reader's own title, kept apart from the document's (R31).
///
/// A title extracted from a PDF and a title a person typed are **different facts**, exactly like
/// progress from a viewer versus progress typed by hand. So the reader's title does not overwrite
/// `documents.title`: that column keeps the document's own title, exactly as the scanner wrote it,
/// and this new nullable column holds the reader's. `NULL` means only "the reader has not set a
/// title", and the two together make it possible to say whose title is on screen — and to restore
/// the document's own title without having destroyed it.
///
/// Added as a migration rather than by editing an earlier schema, because all of them are
/// **frozen**: a database an earlier build already created reports their versions, and editing the
/// text would leave that database without the column forever while claiming otherwise. Every
/// existing row keeps its title untouched and reads back with no reader's title.
const SCHEMA_V5: &str = "
ALTER TABLE documents ADD COLUMN user_title TEXT;
";

/// Version 6: the reader's cover choice, as two nullable facts on the document (R32).
///
/// The default cover is the first page, so `cover_page` is `NULL` for "the first page" — the same
/// shape `user_title` uses, where `NULL` means "not set" rather than a value. `cover_image` holds the
/// **file name** of an image the reader supplied, inside the covers directory; a name and not a path,
/// because the application copies the reader's image into that directory (L20), so the cover survives
/// a rescan and the original file being moved or deleted.
///
/// The precedence between the two — image first, otherwise the page, otherwise the first — is a single
/// rule in [`oct_core::Document::cover_choice`] and not here: this crate stores the facts and does not
/// decide what they mean.
///
/// Added as a migration rather than by editing an earlier schema, because all of them are **frozen**:
/// a database an earlier build already created reports their versions, and editing the text would
/// leave that database without the columns forever while claiming otherwise. Both `ALTER TABLE`
/// statements run in one transaction with the version stamp, so a step that fails halfway leaves the
/// version behind and no half-added column (the V6 atomicity test pins exactly that).
const SCHEMA_V6: &str = "
ALTER TABLE documents ADD COLUMN cover_page INTEGER;
ALTER TABLE documents ADD COLUMN cover_image TEXT;
";

/// Version 7: the reader's own author, kept apart from the document's own.
///
/// A twin of V5's `user_title`, with one deliberate difference that lives at the WRITE boundary rather
/// than here: the title refuses a blank because a document must have a title, while an author has a
/// legitimate empty state — *no author of my own* — and clearing the reader's value restores the
/// document's own author. Exactly like `user_title`, `NULL` means only "the reader has not set one",
/// and the value never overwrites `documents.author`, so a rescan cannot rename an author a person
/// chose and the document's own author is never destroyed.
///
/// Added as a migration rather than by editing an earlier schema, because all of them are **frozen**:
/// a database an earlier build already created reports their versions, and editing the text would
/// leave that database without the column forever while claiming otherwise. The step is stamped in
/// its own transaction, so a failure leaves the version behind and no half-added column (the V7
/// atomicity test pins exactly that).
///
/// **This column does not survive the schema.** [`SCHEMA_V9`] moves every non-blank value here into
/// `document_authors` and then drops it, because a reader's single author became the reader's ordered
/// list (R42/S1) and one fact deserves one home. Kept here as written, because a migration is the
/// history of the schema and not a description of it.
const SCHEMA_V7: &str = "
ALTER TABLE documents ADD COLUMN user_author TEXT;
";

/// Version 8: the reader's decision about a suggestion, as its own fact (R4).
///
/// The table already had `decided_at`, which says **when** a decision happened but not **what** was
/// decided: a timestamp alone cannot distinguish an accepted suggestion from a dismissed one. This
/// step adds `decision`, holding `'accepted'` or `'discarded'`, and `NULL` means only "the reader
/// has not decided yet". It is deliberately a separate column rather than a value written into
/// `decided_at`, exactly as the reader's title is a column beside the document's own: a re-detection
/// bumps `seen_count` and stops at a decided row, so the reader's decision is never overwritten by
/// the application observing the same file again.
///
/// The unique index is what makes "one suggestion per path" a database invariant rather than a
/// promise the read-then-write in [`Store::record_suggestion`] makes and might break. It is safe to
/// add here because no shipped build ever wrote a suggestion row, so the table is empty of
/// duplicates; a hand-edited database with two rows for one path will fail the step honestly rather
/// than silently pick one.
///
/// Added as a migration rather than by editing [`SCHEMA_V1`], because V1 is **frozen**: a database an
/// earlier build already created reports version 1, and editing the text would leave it without the
/// column and the index forever while claiming otherwise. Both statements run in one transaction with
/// the version stamp, so a step that fails halfway leaves the version behind (the V8 atomicity test
/// pins exactly that).
const SCHEMA_V8: &str = "
ALTER TABLE suggestions ADD COLUMN decision TEXT;
CREATE UNIQUE INDEX idx_suggestions_path_unique ON suggestions (path);
";

/// Version 9, first half: a document's authors are **facts in their own tables**, not a delimited
/// string and not JSON (R42/S1).
///
/// `authors` holds one row per person and `document_authors` holds the reader's ordered list for a
/// document: `position` is the reader's order (R42/S3), and the `(doc_id, author_id)` primary key says
/// a document cannot name the same person twice. `ON DELETE CASCADE` on `doc_id` gives the list the
/// same lifetime as the document it describes; deleting a document already cascades its progress and
/// its open events, and a list that outlived its document would be an author nobody wrote.
///
/// **Deduplication is a normalised key column, and this is the decision S2 asked for.** `name` keeps
/// the spelling that arrived first, so an existing card does not silently change; `name_key` holds the
/// trimmed name lowercased, and `name_key` is `UNIQUE`, which makes "one row per person" a database
/// invariant instead of a promise the write path makes and might break. It is not `COLLATE NOCASE`
/// on `name`, because SQLite's `NOCASE` folds ASCII only: `"Émile Zola"` and `"ÉMILE ZOLA"` would be
/// two people, and this library's authors are mostly not ASCII. It is not a lookup either, for the
/// same class of reason — a comparison computed in Rust and a constraint that does not exist cannot
/// both be true of every row the next writer inserts. The residual is written down rather than
/// hidden: `str::to_lowercase` is Unicode lowercase and not full case folding, so a pair that
/// lowercasing does not unify (German `ß` against `SS`, Greek final sigma against `σ`) is still two
/// people. Nothing in this workspace can tell those apart without a dependency and without guessing
/// at a person's name.
///
/// The uniqueness lives on `name_key` and **not** on `name` as well, deliberately: an exact duplicate
/// name is already an equal key, so a second unique index would be redundant, and it would make every
/// upsert ambiguous about which constraint it conflicted with — SQLite only applies the `ON CONFLICT`
/// target to the constraint it actually detected.
///
/// The index is the read path: a document's list is always read *by document, ordered by position*,
/// and the primary key orders by `author_id` instead.
///
/// Added as a migration rather than by editing [`SCHEMA_V1`], because V1 is **frozen**: a database an
/// earlier build already created reports version 1, and editing the text would leave it without these
/// tables forever while claiming otherwise.
const SCHEMA_V9: &str = "
CREATE TABLE authors (
    id       INTEGER PRIMARY KEY,
    name     TEXT NOT NULL,
    name_key TEXT NOT NULL UNIQUE
);

CREATE TABLE document_authors (
    doc_id    INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES authors(id),
    position  INTEGER NOT NULL,
    PRIMARY KEY (doc_id, author_id)
);

CREATE INDEX idx_document_authors_doc ON document_authors (doc_id, position);
";

/// Version 9, second half: the reader's single author column **retires**, and this is the decision the
/// task asked for.
///
/// The alternative was to keep `documents.user_author` in place and document it as unread. It was
/// rejected because "the reader's author must never be able to disagree with the new list" is a
/// property of the schema or it is not a property at all: a column that still holds a name is a second
/// home for one fact, and the next build that reads it — or an older build still running beside this
/// one — resurrects a value the list has already replaced. V9 is the only version that can drop it
/// while the value is provably in hand, so nothing is lost: the backfill ([`backfill_reader_authors`])
/// reads every non-blank `user_author` into a one-entry `document_authors` row **first**, in the same
/// transaction, and only then does this statement run.
///
/// `documents.author` is untouched by all of this: it is the PDF's embedded string, read-only, never
/// split and never rewritten (R42/S6).
///
/// `ALTER TABLE ... DROP COLUMN` needs SQLite 3.35 or newer; this crate builds the bundled SQLite
/// (3.46), and the workspace's `rusqlite` feature set is what makes that true. A database a person
/// hand-edited into an index over `user_author` fails this statement honestly, and the transaction
/// leaves the whole step unapplied (the V9 atomicity test pins exactly that).
const SCHEMA_V9_RETIRE_USER_AUTHOR: &str = "ALTER TABLE documents DROP COLUMN user_author;";

/// A document `oct` observed open outside the watched folders, awaiting the reader's decision (R4).
///
/// A suggestion is **not** library data: it carries a path and no document foreign key, so a
/// suggestion whose file is gone is a row that names nothing, not a document with a missing file.
/// What turns it into data is [`Store::decide_suggestion`], and only the reader can ask for that.
///
/// `seen_count` is how many times the application has observed the document open since the reader
/// last acted on it, and it is `first_seen_at` — not the latest sighting — that orders the queue, so
/// a document that keeps being opened does not jump around. `decision` is `None` until the reader
/// decides, and is never cleared afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// The `suggestions.id` primary key.
    pub id: i64,
    /// Absolute path of the document, exactly as the detector reported it.
    pub path: PathBuf,
    /// How many times the application has observed it open while it was undecided.
    pub seen_count: u32,
    /// Unix seconds when the application first observed it.
    pub first_seen_at: i64,
    /// The reader's decision, or [`None`] while it is still awaiting one.
    pub decision: Option<SuggestionDecision>,
    /// Unix seconds when the reader decided, or [`None`] while no decision exists.
    pub decided_at: Option<i64>,
}

/// Which decision the reader took on a suggestion (R4).
///
/// The two variants are persisted by their stable ids (`"accepted"` / `"discarded"`), so a row
/// written by this build means the same thing to the next one. The decision is the reader's and
/// nobody else's: nothing in this workspace may accept or discard a suggestion on its own, which is
/// why this type has no `Default` and no automatic source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuggestionDecision {
    /// The reader accepted it: the document joined the library.
    Accepted,
    /// The reader dismissed it: it must not be offered again.
    Discarded,
}

impl SuggestionDecision {
    /// A stable machine-readable identifier, suitable for persisting and for the interface.
    pub const fn id(self) -> &'static str {
        match self {
            SuggestionDecision::Accepted => "accepted",
            SuggestionDecision::Discarded => "discarded",
        }
    }
}

/// A stored document, together with its database id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRecord {
    /// The `documents.id` primary key, needed to attach progress to the document.
    pub id: i64,
    /// The document as stored.
    pub document: Document,
}

/// One row of the library view: a document and its most recent reading position.
///
/// This is the contract T5 deferred until a caller existed. It deliberately carries
/// [`StoredProgress`] whole rather than flattening it to a page number, so the screen that
/// renders it can state the position *with its provenance* — `p. 12 of 80, recorded by Evince` —
/// instead of a percentage that hides where the number came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryEntry {
    /// The document row, including the database id needed to record progress against it.
    pub document: DocumentRecord,
    /// The most recent reading position, with `source` and `confidence` intact, or [`None`] when
    /// the document has never held one. It is never a zero standing in for "unknown".
    pub progress: Option<StoredProgress>,
    /// Unix seconds when `progress` was recorded, or [`None`] alongside a [`None`] progress.
    pub progress_recorded_at: Option<i64>,
}

/// A watched folder as stored.
///
/// A folder is a source of truth for the library: the scanner walks it, and the live watcher
/// (T7) reacts to changes inside it. It is stored separately from `documents` because a folder
/// exists whether or not a scan has found anything in it yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchedFolder {
    /// The `folders.id` primary key.
    pub id: i64,
    /// Absolute path of the watched folder, as recorded.
    pub path: PathBuf,
    /// Whether the scanner descends into subdirectories.
    pub recursive: bool,
    /// Whether the watcher is enabled. User-controlled; [`Store::record_folder`] leaves it alone.
    pub enabled: bool,
}

/// What removing one watch takes with it (R27).
///
/// Removing a watched folder removes the documents that live inside it and their reading positions:
/// the user asked for exactly that, because a folder they stopped watching should leave their
/// library. Two guards keep it from being a trap. A document **another remaining watch still
/// covers** is kept, so removing an outer folder cannot empty a folder that is still watched. And a
/// position is recoverable from the viewer's own metadata — re-watching the folder brings it back
/// through the launch sweep — which is why the confirmation may say so; nothing in this crate
/// enforces that, it is a fact about the provider layer.
///
/// Both vectors hold stored `documents.path` values, sorted by path so two consecutive calls agree
/// and a caller can diff them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRemoval {
    /// The watch being removed.
    pub folder: WatchedFolder,
    /// Documents inside `folder` that no other remaining watch covers: these leave the library.
    pub documents: Vec<String>,
    /// Documents inside `folder` that another remaining watch still covers: these stay.
    pub kept: Vec<String>,
}

/// The result of planning a removal: [`FolderRemoval`]'s paths plus the ids needed to delete them.
///
/// One computation feeds both the read-only plan and the removal itself, so the count the interface
/// states and the rows that actually leave cannot disagree.
struct FolderRemovalPlan {
    folder: WatchedFolder,
    /// `(documents.id, documents.path)`, sorted by path.
    documents: Vec<(i64, String)>,
    kept: Vec<String>,
}

impl FolderRemovalPlan {
    /// Drops the ids, leaving the caller-facing [`FolderRemoval`].
    fn into_removal(self) -> FolderRemoval {
        FolderRemoval {
            folder: self.folder,
            documents: self
                .documents
                .into_iter()
                .map(|(_, path)| path)
                .collect(),
            kept: self.kept,
        }
    }
}

/// What one action decided about the reader's authors, in the write that saves the document editor.
///
/// The editor has two controls and they do not decide the same facts. **Save** decides the title *and*
/// the authors. **Restore** decides the title and nothing else: restoring the document's own title is
/// not a statement about who wrote the document. A two-valued parameter cannot say which, so "nothing"
/// and "no author of my own" — which are different actions — would arrive looking the same.
///
/// This type exists so the difference is spelled out at the one write that has to make it, instead of
/// being inferred from which statements a caller happens to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorEdit<'a> {
    /// The action decided the author, as the one name the shipped interface can still write: the name,
    /// or none for *no author of my own*, which is a legitimate action that falls back to the
    /// document's own author.
    ///
    /// A blank name means the same as none, because an author field the reader emptied is them saying
    /// they have no author of their own — not a person named by spaces. This is the one-author spelling
    /// of [`AuthorEdit::DecidedList`] and it means exactly a zero- or one-entry list; it stays because
    /// the IPC command and its web side still speak in one author (R42's interface half is a later
    /// unit), and a one-element list has to behave exactly like the old single author.
    Decided(Option<&'a str>),
    /// The action decided the authors, as the reader's ordered list (R42/S5).
    ///
    /// An **empty** list is *no author of my own*: it is a decision, and it falls back to the
    /// document's own author rather than meaning "said nothing" — that is
    /// [`AuthorEdit::Untouched`]. Entries are trimmed and deduplicated case-insensitively in the order
    /// given; an entry that is only whitespace is refused rather than stored as `""` (R42/S2), because
    /// a blank chip is a mistake and `NULL` already means "not set".
    DecidedList(&'a [&'a str]),
    /// The action said nothing about the authors, so the reader's list stays exactly as it is.
    Untouched,
}

/// The reader's title as the column stores it, or the refusal a blank deserves.
///
/// `None` stays `None` — restore the document's own title — while a blank `Some` is the invalid
/// argument it is: a title that holds only whitespace is not a title, and storing it would make `NULL`
/// mean two different things. One normalisation for every write that carries a title, so no caller can
/// disagree with another about what a blank title means.
fn normalize_user_title(title: Option<&str>) -> Result<Option<&str>, OctError> {
    match title {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(OctError::InvalidInput {
                    field: "title",
                    value: value.to_string(),
                    expected: "a non-empty title, or no value to restore the document's own".to_string(),
                });
            }
            Ok(Some(trimmed))
        }
        None => Ok(None),
    }
}

/// The reader's author as the one-author spelling stores it: a blank value is no value at all.
///
/// **This is the deliberate difference from [`normalize_user_title`], which refuses a blank because a
/// document must have a title.** Many documents have no author, so an empty field is exactly the
/// reader's way of clearing their own author, and no author at all stays the only spelling of "not
/// set". It is the single-value form of [`normalize_user_authors`], and it means a zero- or one-entry
/// list: a blank here is the reader emptying one field, which is not the same act as inserting a
/// blank chip into a list — that one is refused, below.
fn normalize_user_author(author: Option<&str>) -> Option<&str> {
    author.map(str::trim).filter(|trimmed| !trimmed.is_empty())
}

/// The key one person is deduplicated by: the trimmed name, lowercased (R42/S2).
///
/// **One function, so the write and the migration cannot disagree about who is the same person.** It
/// is also what `authors.name_key` is compared against by the database's own `UNIQUE` constraint, so
/// the rule that decides "same person" is not a query in one place and a promise in another.
///
/// It is Unicode lowercase rather than SQLite's `COLLATE NOCASE`, because `NOCASE` folds ASCII only
/// and `"Émile Zola"` / `"ÉMILE ZOLA"` would be two people. The residual is honest and small: this is
/// lowercasing, not full case folding, so `ß` against `SS` and Greek final sigma against `σ` stay two
/// people.
fn author_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// The reader's authors as the tables store them: trimmed, blank-free, deduplicated in the order given.
///
/// **Order is the reader's and is never sorted** (R42/S3): "Carl Sagan, Ann Druyan" and "Ann Druyan,
/// Carl Sagan" are different lists and both are legitimate, so this keeps the first occurrence of each
/// person and drops only a repeat. Deduplication is by [`author_key`], case-insensitively, and **the
/// spelling that arrives first wins** (R42/S2) — the same rule the `authors` upsert applies to a name
/// the library already knows, so typing `"Carl Sagan"` and `"carl sagan"` in one save yields one row
/// named "Carl Sagan".
///
/// An entry that is only whitespace is refused rather than dropped: a chip the reader left empty is a
/// mistake to report, not a person to forget (R42/S2). An **empty list** is not a mistake — it is the
/// reader saying they have no author of their own.
fn normalize_user_authors<'a>(names: &[&'a str]) -> Result<Vec<&'a str>, OctError> {
    let mut kept: Vec<&'a str> = Vec::with_capacity(names.len());
    let mut seen: HashSet<String> = HashSet::with_capacity(names.len());
    for name in names {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(OctError::InvalidInput {
                field: "author",
                value: (*name).to_string(),
                expected: "a non-empty author name, or no entry at all".to_string(),
            });
        }
        if seen.insert(author_key(trimmed)) {
            kept.push(trimmed);
        }
    }
    Ok(kept)
}

/// A SQLite connection to `oct`'s database.
///
/// `Store` is intentionally the only handle: callers never see a `rusqlite::Connection`, so the
/// schema and its migration stay an implementation detail.
#[derive(Debug)]
pub struct Store {
    connection: Connection,
}

/// The `progress` columns [`Store::library`] reads for a document's latest row, in select order:
/// page, total, source, confidence, read time, write time. Named so the tuple does not turn every
/// call site into a puzzle, and so the read and the write cannot drift apart silently.
type LatestProgressColumns = (i64, Option<i64>, String, String, Option<i64>, i64);

/// The watched folder that covers `path`, when one does, preferring the tightest one.
///
/// "Covers" means `path` is the watched folder itself or a folder inside it, and the question is
/// asked of **path components**, never of the characters they spell. `/library/pdfs-old` shares the
/// whole string prefix `/library/pdfs` and is a different folder; `Path::starts_with` compares whole
/// components, so it answers that correctly where a `starts_with` on the rendered path would not. A
/// trailing separator on the watched path changes nothing either, because `Path` drops it before
/// comparing.
///
/// The tightest covering folder wins so a refusal can name the folder that actually bounds the
/// user's choice: with both `/library` and `/library/pdfs` watched, a candidate inside `pdfs` is
/// covered by both, and `pdfs` is the answer that explains itself.
pub fn covering_folder<'a>(folders: &'a [WatchedFolder], path: &Path) -> Option<&'a WatchedFolder> {
    folders
        .iter()
        .filter(|folder| path.starts_with(&folder.path))
        .max_by_key(|folder| folder.path.components().count())
}

impl Store {
    /// The schema version this build creates and migrates to.
    pub const CURRENT_SCHEMA_VERSION: u32 = 9;

    /// Opens the database at `path`, creating parent directories and the file when needed, and
    /// brings it to [`Self::CURRENT_SCHEMA_VERSION`].
    ///
    /// Pass [`database_path`] for the production location; tests pass a temporary file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, OctError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(OctError::Io)?;
            }
        }

        let mut connection = Connection::open(path).map_err(storage_error)?;
        Self::prepare(&mut connection)?;
        Ok(Self { connection })
    }

    /// Opens an ephemeral in-memory database, migrated like a file. Used by callers that want a
    /// throwaway store; the production path is [`Self::open`].
    pub fn open_in_memory() -> Result<Self, OctError> {
        let mut connection = Connection::open_in_memory().map_err(storage_error)?;
        Self::prepare(&mut connection)?;
        Ok(Self { connection })
    }

    fn prepare(connection: &mut Connection) -> Result<(), OctError> {
        // Foreign keys are off by default in SQLite and are per-connection, so they are enabled
        // here rather than assumed.
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(storage_error)?;
        migrate(connection)
    }

    /// The schema version currently recorded in the database.
    pub fn schema_version(&self) -> Result<u32, OctError> {
        read_schema_version(&self.connection)
    }

    /// The names of the user tables in the database, sorted.
    ///
    /// Exposed so migrations can be diagnosed and the schema contract verified through the public
    /// interface instead of by querying SQLite's metadata tables from outside.
    pub fn table_names(&self) -> Result<Vec<String>, OctError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage_error)?;

        let mut names = Vec::new();
        for row in rows {
            names.push(row.map_err(storage_error)?);
        }
        Ok(names)
    }

    /// Reads a `settings` value, or `None` when the key was never written.
    ///
    /// An absent key and a key that holds nothing are the same answer on purpose: callers decide
    /// what a missing setting means (the interface falls back to its default), and `None` is the
    /// honest report that nothing was ever chosen.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, OctError> {
        self.connection
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage_error)
    }

    /// Writes a `settings` value, replacing any previous value for `key`.
    ///
    /// The upsert is in SQL rather than a read-then-write so a second writer cannot race this one
    /// into a duplicate-key failure; the primary key on `settings.key` is the same invariant.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), OctError> {
        self.connection
            .execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    /// Inserts a document, or updates the existing row that describes it. Returns the row id.
    ///
    /// **Matching order.** Identity wins, because it survives renames and moves: a document whose
    /// identity is already stored is updated in place even when its path changed. Only when the
    /// identity is unknown to the caller, or unseen, does the path decide — and a row found by path
    /// that has no identity yet **adopts** the one being supplied.
    ///
    /// A path that now holds a *different* identity is a different file, and therefore gets its own
    /// row: attaching the older row's reading history to the replacement would be exactly the silent
    /// lie the provenance columns exist to prevent. Path lookups then resolve to the newest row for
    /// that path.
    ///
    /// **Nothing known is erased by something unknown.** A `None` never overwrites a stored value,
    /// so a caller that does not know the identity, the page count or the metadata yet cannot
    /// destroy what an earlier caller already established.
    ///
    /// **Concurrency.** The read-then-write above is not itself a transaction, but the database
    /// enforces the same invariant with a partial unique index over the identity columns, so a second
    /// writer that races this one cannot create a duplicate row for a known identity: it fails with a
    /// [`OctError::Storage`] constraint error. Serialising writers with an immediate transaction and
    /// retrying is deliberately deferred — `oct` currently opens one store per process, so the race
    /// requires a second process writing the same database, which no code does yet.
    ///
    /// Only the columns the scanner can supply are written; the reader's title and the reader's
    /// authors are deliberately not among them, because they are separate facts with their own write
    /// path ([`Store::set_user_naming`]). `documents.author` in particular is the file's own embedded
    /// string and is written from the scanner's metadata only — never from anything a reader typed,
    /// and never split into names.
    /// The scanner fills metadata such as `size`, `mtime` and `discovered_via` when it exists.
    pub fn upsert_document(&self, document: &Document) -> Result<i64, OctError> {
        let path = path_to_string(&document.path)?;
        let uri = document.uri();
        let now = unix_now();
        let pages = document.pages.map(i64::from);

        let (file_id_kind, volume_id, file_index, content_hash) = match &document.identity {
            Some(identity) => {
                let (kind, volume, index, hash) = encode_identity(identity);
                (Some(kind), volume, index, hash)
            }
            None => (None, None, None, None),
        };

        let existing = match &document.identity {
            Some(identity) => match self.find_id_by_identity(identity)? {
                Some(id) => Some(id),
                None => match self.find_id_by_path(&path)? {
                    // The row predates knowing this document's identity: adopt it rather than fork.
                    Some((id, None)) => Some(id),
                    // The path holds a different file; the replacement gets its own row.
                    Some((_, Some(_))) | None => None,
                },
            },
            None => self.find_id_by_path(&path)?.map(|(id, _)| id),
        };

        match existing {
            Some(id) => {
                self.connection
                    .execute(
                        "UPDATE documents SET \
                            path = ?1, path_normalized = ?1, uri = ?2, \
                            file_id_kind = COALESCE(?3, file_id_kind), \
                            volume_id = COALESCE(?4, volume_id), \
                            file_index = COALESCE(?5, file_index), \
                            content_hash = COALESCE(?6, content_hash), \
                            pages = COALESCE(?7, pages), \
                            title = COALESCE(?8, title), \
                            author = COALESCE(?9, author) \
                         WHERE id = ?10",
                        params![
                            path,
                            uri,
                            file_id_kind,
                            volume_id.as_deref(),
                            file_index.as_deref(),
                            content_hash.as_deref(),
                            pages,
                            document.title.as_deref(),
                            document.author.as_deref(),
                            id
                        ],
                    )
                    .map_err(storage_error)?;
                Ok(id)
            }
            None => {
                self.connection
                    .execute(
                        "INSERT INTO documents (\
                            path, path_normalized, uri, file_id_kind, volume_id, file_index, \
                            content_hash, pages, title, author, first_seen_at\
                         ) VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            path,
                            uri,
                            file_id_kind,
                            volume_id.as_deref(),
                            file_index.as_deref(),
                            content_hash.as_deref(),
                            pages,
                            document.title.as_deref(),
                            document.author.as_deref(),
                            now
                        ],
                    )
                    .map_err(storage_error)?;
                Ok(self.connection.last_insert_rowid())
            }
        }
    }

    /// Saves the reader's naming of the document with `id`: its title, and its authors when this action
    /// decided them (R31, R37, R42/S5).
    ///
    /// **It is one write, because the action that decides both facts is one action.** The editor's Save
    /// stores the title and the authors together, and an action that decides two facts must apply both
    /// or neither: writing them as two independent updates left a window where the title had landed,
    /// the author had not, and the interface reported an error that implied nothing was saved. A
    /// half-applied save is not a save, so every statement below runs inside one transaction and a
    /// failure anywhere rolls the whole thing back — including the `document_authors` rows a refused
    /// write had already deleted and re-inserted (the one-write test pins exactly that).
    ///
    /// `title` follows [`normalize_user_title`]'s rule — `None` restores the document's own title, a
    /// blank `Some` is refused — and it is validated **before** the transaction opens, so a refused
    /// title never reaches the database. `author` is an [`AuthorEdit`], not a plain value: Save decides
    /// the authors, while Restore decides the title and nothing else, and `AuthorEdit::Untouched` is how
    /// this write is told to leave the reader's list alone instead of that decision being inferred from
    /// a missing statement. `AuthorEdit::DecidedList(&[])` is the reader saying *no author of my own*,
    /// which is a decision and not a silence, so it clears the list rather than keeping it.
    ///
    /// The list is normalised once ([`normalize_user_authors`]) and then written as the reader's order:
    /// `position` is the index in that list, and each name is upserted into `authors` **once**, so two
    /// documents by the same person share one row and typing the same name twice in one save yields
    /// one entry. A name the library already knows keeps the spelling it already had (R42/S2).
    ///
    /// Nothing here touches the document's own title or `documents.author`. Both reader's facts are
    /// separate from the file's, and [`Store::upsert_document`] never writes them, so a rescan cannot
    /// rename a document a person named.
    ///
    /// **What this cannot express**: an action that changed the authors and said nothing about the
    /// title. Nothing in the application has one — Save decides both and Restore decides the title —
    /// and a title the caller wants to keep is simply passed as it is, which is also what makes the
    /// pair atomic.
    pub fn set_user_naming(
        &self,
        id: i64,
        title: Option<&str>,
        author: AuthorEdit<'_>,
    ) -> Result<(), OctError> {
        let title = normalize_user_title(title)?;
        // `None` is "this action said nothing about the authors"; `Some(list)` is a decision, and an
        // empty list inside it is the legitimate *no author of my own*.
        let authors: Option<Vec<&str>> = match author {
            AuthorEdit::Untouched => None,
            AuthorEdit::Decided(single) => Some(
                normalize_user_author(single)
                    .into_iter()
                    .collect::<Vec<&str>>(),
            ),
            AuthorEdit::DecidedList(list) => Some(normalize_user_authors(list)?),
        };

        // One transaction around every statement, which is the whole point: a later write failing drops
        // the transaction uncommitted, and the title goes with it. Everything below is on the
        // transaction handle rather than on the connection, so no statement can escape it.
        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(storage_error)?;

        let affected = transaction
            .execute(
                "UPDATE documents SET user_title = ?1 WHERE id = ?2",
                params![title, id],
            )
            .map_err(storage_error)?;
        if affected == 0 {
            return Err(OctError::Storage(format!(
                "no document with id {id} to name"
            )));
        }

        if let Some(authors) = authors {
            // The list is replaced, not merged: the reader's list is what they said it is, and a name
            // they removed must not survive as an orphan row nobody reads.
            transaction
                .execute(
                    "DELETE FROM document_authors WHERE doc_id = ?1",
                    params![id],
                )
                .map_err(storage_error)?;
            for (position, name) in authors.iter().enumerate() {
                let author_id = upsert_author(&transaction, name)?;
                transaction
                    .execute(
                        "INSERT INTO document_authors (doc_id, author_id, position) \
                         VALUES (?1, ?2, ?3)",
                        params![id, author_id, position as i64],
                    )
                    .map_err(storage_error)?;
            }
        }

        transaction.commit().map_err(storage_error)?;
        Ok(())
    }

    /// Sets or clears the page the reader chose for the document's cover (R32).
    ///
    /// `None` restores the default — the first page — and is a legitimate action, not an error. A
    /// page is stored zero-based exactly like a reading position, so the interface converts once at
    /// its own boundary. A page beyond the end is **not** refused here: it is a cover that cannot be
    /// rendered, which the cover path already reports as "no cover" without an invented message.
    /// Refusing it in the store would put a second copy of that rule where it does not belong.
    pub fn set_cover_page(&self, id: i64, page: Option<u32>) -> Result<(), OctError> {
        let affected = self
            .connection
            .execute(
                "UPDATE documents SET cover_page = ?1 WHERE id = ?2",
                params![page.map(i64::from), id],
            )
            .map_err(storage_error)?;

        if affected == 0 {
            return Err(OctError::Storage(format!(
                "no document with id {id} to set a cover page on"
            )));
        }
        Ok(())
    }

    /// Sets or clears the reader's supplied image for the document's cover (R32).
    ///
    /// `file_name` is the **name** of a file inside the covers directory, never a path. `None`
    /// removes the image, so the document shows whichever page it shows. A blank name is refused as
    /// the invalid argument it is: `NULL` already means "no image", and an empty string would make
    /// the column mean two different things. A value carrying a directory part is refused too — the
    /// application copies the reader's file into its own directory and stores the name it gave it, so
    /// a caller handing over a path has misunderstood the contract, and joining that path would let a
    /// hand-edited database point outside the covers directory.
    pub fn set_cover_image(&self, id: i64, file_name: Option<&str>) -> Result<(), OctError> {
        let normalized: Option<&str> = match file_name {
            Some(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Err(OctError::InvalidInput {
                        field: "cover image",
                        value: value.to_string(),
                        expected: "a file name inside the covers directory, or no value to remove it"
                            .to_string(),
                    });
                }
                if trimmed.contains('/') || trimmed.contains('\\') {
                    return Err(OctError::InvalidInput {
                        field: "cover image",
                        value: value.to_string(),
                        expected: "a plain file name with no directory part".to_string(),
                    });
                }
                Some(trimmed)
            }
            None => None,
        };

        let affected = self
            .connection
            .execute(
                "UPDATE documents SET cover_image = ?1 WHERE id = ?2",
                params![normalized, id],
            )
            .map_err(storage_error)?;

        if affected == 0 {
            return Err(OctError::Storage(format!(
                "no document with id {id} to set a cover image on"
            )));
        }
        Ok(())
    }

    /// Finds a document by its portable identity alone, without a path.
    ///
    /// Returns `Ok(None)` when no document carries that identity — an absent document is a normal
    /// answer, not an error.
    pub fn find_document_by_identity(
        &self,
        identity: &FileId,
    ) -> Result<Option<DocumentRecord>, OctError> {
        let (kind, volume, index, hash) = encode_identity(identity);

        let row = self
            .connection
            .query_row(
                "SELECT id, path, file_id_kind, volume_id, file_index, content_hash, pages, title, author, user_title, cover_page, cover_image \
                 FROM documents \
                 WHERE file_id_kind = ?1 AND volume_id IS ?2 AND file_index IS ?3 AND content_hash IS ?4 \
                 LIMIT 1",
                params![kind, volume.as_deref(), index.as_deref(), hash.as_deref()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<i64>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, Option<i64>>(10)?,
                        row.get::<_, Option<String>>(11)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?;

        let Some((
            id,
            path,
            kind,
            volume,
            index,
            hash,
            pages,
            title,
            author,
            user_title,
            cover_page,
            cover_image,
        )) = row
        else {
            return Ok(None);
        };

        let identity = decode_identity(
            kind.as_deref(),
            volume.as_deref(),
            index.as_deref(),
            hash.as_deref(),
        )?;
        let pages = pages
            .map(u32::try_from)
            .transpose()
            .map_err(|_| OctError::Storage("stored page count is not a valid page number".into()))?;
        let cover_page = cover_page
            .map(u32::try_from)
            .transpose()
            .map_err(|_| OctError::Storage("stored cover page is not a valid page number".into()))?;

        Ok(Some(DocumentRecord {
            id,
            document: Document {
                path: PathBuf::from(path),
                identity,
                pages,
                title,
                author,
                user_title,
                authors: self.reader_authors()?.remove(&id).unwrap_or_default(),
                cover_page,
                cover_image,
            },
        }))
    }

    /// The reader's ordered author list for **every** document that has one, keyed by
    /// `documents.id` (R42/S1, R42/S3).
    ///
    /// One query and one code path, used by the listing and by both single-document lookups, for the
    /// same reason [`decode_document`] is shared: a document fetched by id and the same document
    /// fetched in the listing must not be able to disagree about who wrote it. Reading the whole table
    /// for a single-document lookup is the price of that, and it is small — one row per (document,
    /// author) pair in a personal library.
    ///
    /// A document with no reader's authors is simply absent from the map, and its caller uses the empty
    /// list: *no author of my own*, never an invented one. The order is `position`, which is the
    /// reader's order; `a.id` breaks a tie no write of ours can create, so two reads of the same
    /// database cannot come back in different orders.
    fn reader_authors(&self) -> Result<HashMap<i64, Vec<String>>, OctError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT da.doc_id, a.name FROM document_authors da \
                 JOIN authors a ON a.id = da.author_id \
                 ORDER BY da.doc_id ASC, da.position ASC, a.id ASC",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(storage_error)?;

        let mut by_document: HashMap<i64, Vec<String>> = HashMap::new();
        for row in rows {
            let (doc_id, name) = row.map_err(storage_error)?;
            by_document.entry(doc_id).or_default().push(name);
        }
        Ok(by_document)
    }

    /// Records a reading position with its provenance and returns the new row id.
    ///
    /// [`StoredProgress::page`] is zero-based and is stored unchanged.
    ///
    /// **Two times, deliberately separate.** `updated_at` is stamped with `unix_now()` — when the
    /// app wrote this row, which is a fact the app always has. `read_at` is written from
    /// [`StoredProgress::read_at`] and is left `NULL` when the caller does not know, so a caller
    /// cannot accidentally present the write time as a reading time.
    pub fn record_progress(
        &self,
        doc_id: i64,
        progress: &StoredProgress,
    ) -> Result<i64, OctError> {
        self.connection
            .execute(
                "INSERT INTO progress (doc_id, page, total, source, confidence, read_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    doc_id,
                    i64::from(progress.page),
                    progress.total.map(i64::from),
                    progress.source.id(),
                    confidence_id(progress.confidence),
                    progress.read_at,
                    unix_now()
                ],
            )
            .map_err(storage_error)?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Every progress row for a document, oldest first. Provenance is returned intact, so rows
    /// that share a page but differ in `source` or `confidence` stay distinguishable here.
    pub fn progress_for_document(&self, doc_id: i64) -> Result<Vec<StoredProgress>, OctError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT page, total, source, confidence, read_at FROM progress \
                 WHERE doc_id = ?1 ORDER BY id",
            )
            .map_err(storage_error)?;

        let rows = statement
            .query_map(params![doc_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            })
            .map_err(storage_error)?;

        let mut progress = Vec::new();
        for row in rows {
            let (page, total, source, confidence, read_at) = row.map_err(storage_error)?;
            let page = u32::try_from(page)
                .map_err(|_| OctError::Storage(format!("stored page {page} is not a page number")))?;
            let total = total
                .map(u32::try_from)
                .transpose()
                .map_err(|_| {
                    OctError::Storage("stored total is not a valid page count".into())
                })?;

            progress.push(
                StoredProgress::new(
                    page,
                    total,
                    parse_progress_source(&source),
                    parse_confidence(&confidence)?,
                )
                .with_read_at(read_at),
            );
        }
        Ok(progress)
    }

    /// Records a watched folder and returns its row id.
    ///
    /// Recording the same path again updates `recursive` in place rather than inserting a second
    /// row, so the scanner can record its folder on every run without duplicating it. `added_at`
    /// keeps the time the folder was first recorded, and `enabled` is left untouched: it is the
    /// user's setting, not the scanner's.
    ///
    /// Like [`Store::upsert_document`], the read-then-write below is not a transaction; the same
    /// accepted residual applies, because `oct` opens one store per process today. A second writer
    /// that loses the race cannot duplicate the folder: `folders.path` is UNIQUE, so it fails with
    /// a [`OctError::Storage`] constraint error instead.
    pub fn record_folder(
        &self,
        path: impl AsRef<Path>,
        recursive: bool,
    ) -> Result<i64, OctError> {
        let path = path_to_string(path.as_ref())?;
        let recursive = if recursive { 1 } else { 0 };

        let existing: Option<(i64, i64)> = self
            .connection
            .query_row(
                "SELECT id, recursive FROM folders WHERE path = ?1 ORDER BY id LIMIT 1",
                params![path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;

        match existing {
            Some((id, current)) => {
                if current != recursive {
                    self.connection
                        .execute(
                            "UPDATE folders SET recursive = ?1 WHERE id = ?2",
                            params![recursive, id],
                        )
                        .map_err(storage_error)?;
                }
                Ok(id)
            }
            None => {
                self.connection
                    .execute(
                        "INSERT INTO folders (path, recursive, enabled, added_at) \
                         VALUES (?1, ?2, 1, ?3)",
                        params![path, recursive, unix_now()],
                    )
                    .map_err(storage_error)?;
                Ok(self.connection.last_insert_rowid())
            }
        }
    }

    /// Computes what removing the watch with `id` would take with it, changing nothing (R27).
    ///
    /// This is what lets the interface state a real count before doing something irreversible: the
    /// number the confirmation shows and the number [`Store::forget_folder`] removes come from the
    /// same computation, so they cannot disagree. `None` means the id names no watch — and a
    /// caller that proceeds anyway still deletes nothing.
    pub fn folder_removal(&self, id: i64) -> Result<Option<FolderRemoval>, OctError> {
        Ok(self
            .plan_folder_removal(id)?
            .map(FolderRemovalPlan::into_removal))
    }

    /// Removes the watched folder with `id` and the documents it covers, returning what it took.
    ///
    /// **The watch and its documents go together (R27), and that reverses an earlier rule.** The
    /// first design left the library alone, arguing that a folder *finds* documents rather than
    /// makes them exist. The user has since said what they actually meant — *"Elimine la carpeta
    /// de 'Borges' pero el PDF que allí estaba, sigue apareciendo en mi biblioteca"* — so a folder
    /// they stopped watching takes its documents and their positions with it.
    ///
    /// Two guards keep that from being a trap. A document **another remaining watch still covers**
    /// is kept, so removing an outer folder cannot empty a folder that is still watched. And an id
    /// the table does not hold answers `None` and deletes **nothing at all**, where the earlier
    /// signature said `Ok(false)` for the same request.
    ///
    /// The deletes and the folder row are one transaction, so an interrupted removal cannot leave
    /// documents whose watch is gone (or a watch whose documents are gone); `ON DELETE CASCADE` on
    /// `progress.doc_id` and `open_events.doc_id` carries the rest. [`Store::folder_removal`]
    /// computes exactly this without changing anything.
    pub fn forget_folder(&self, id: i64) -> Result<Option<FolderRemoval>, OctError> {
        let Some(plan) = self.plan_folder_removal(id)? else {
            return Ok(None);
        };

        let transaction = self
            .connection
            .unchecked_transaction()
            .map_err(storage_error)?;
        for (document_id, _) in &plan.documents {
            transaction
                .execute("DELETE FROM documents WHERE id = ?1", params![document_id])
                .map_err(storage_error)?;
        }
        transaction
            .execute("DELETE FROM folders WHERE id = ?1", params![id])
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;

        Ok(Some(plan.into_removal()))
    }

    /// Works out which documents a removal would take and which a remaining watch still covers.
    ///
    /// "Inside the folder" is [`Path::starts_with`] on whole components, never a string prefix —
    /// the same rule [`covering_folder`]'s own doc-comment explains, so `/library/pdfs-old` is
    /// not inside `/library/pdfs` — and the coverage guard reuses [`covering_folder`] over the
    /// **remaining** watches rather than reimplementing it.
    fn plan_folder_removal(&self, id: i64) -> Result<Option<FolderRemovalPlan>, OctError> {
        let folders = self.folders()?;
        let Some(folder) = folders.iter().find(|folder| folder.id == id).cloned() else {
            return Ok(None);
        };
        let remaining: Vec<WatchedFolder> = folders
            .into_iter()
            .filter(|candidate| candidate.id != id)
            .collect();

        let mut documents = Vec::new();
        let mut kept = Vec::new();
        for entry in self.library()? {
            let document_id = entry.document.id;
            let document_path = path_to_string(&entry.document.document.path)?;
            if !Path::new(&document_path).starts_with(&folder.path) {
                continue;
            }
            if covering_folder(&remaining, Path::new(&document_path)).is_some() {
                kept.push(document_path);
            } else {
                documents.push((document_id, document_path));
            }
        }

        documents.sort_by(|left, right| left.1.cmp(&right.1));
        kept.sort();

        Ok(Some(FolderRemovalPlan {
            folder,
            documents,
            kept,
        }))
    }

    /// Every document in the library, each with its most recent reading position and provenance.
    ///
    /// Ordered deterministically by path (then row id, to break a tie when two rows briefly share
    /// a path — see [`Store::upsert_document`]), so two consecutive calls return the same order.
    /// The newest progress row is the one with the highest id, matching
    /// [`Store::progress_for_document`] and the order rows are written in.
    ///
    /// A document that has never held progress is listed with `progress: None`; a document with
    /// several progress rows is listed once, with the latest. Filtering and sorting are
    /// deliberately absent: the first screen needs the whole library in a stable order and
    /// nothing narrower.
    ///
    /// Every entry carries the reader's ordered authors **and** the displayed ones
    /// ([`oct_core::Document::display_authors`], R42/S4). The precedence is not decided here: this
    /// listing and [`Store::document_by_id`] both hand the same decoded [`Document`] to the one rule
    /// that lives in the core, so the card, the author filter and the free-text search cannot disagree
    /// about who wrote a document.
    pub fn library(&self) -> Result<Vec<LibraryEntry>, OctError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT d.id, d.path, d.file_id_kind, d.volume_id, d.file_index, d.content_hash, \
                        d.pages, d.title, d.author, d.user_title, d.cover_page, d.cover_image, \
                        p.page, p.total, p.source, p.confidence, p.read_at, p.updated_at \
                 FROM documents d \
                 LEFT JOIN progress p ON p.id = (\
                     SELECT p2.id FROM progress p2 WHERE p2.doc_id = d.id \
                     ORDER BY p2.id DESC LIMIT 1\
                 ) \
                 ORDER BY d.path ASC, d.id ASC",
            )
            .map_err(storage_error)?;

        let rows = statement
            .query_map([], |row| {
                let kind: Option<String> = row.get(2)?;
                let volume: Option<String> = row.get(3)?;
                let index: Option<String> = row.get(4)?;
                let hash: Option<String> = row.get(5)?;
                let pages: Option<i64> = row.get(6)?;
                let title: Option<String> = row.get(7)?;
                let author: Option<String> = row.get(8)?;
                let user_title: Option<String> = row.get(9)?;
                let cover_page: Option<i64> = row.get(10)?;
                let cover_image: Option<String> = row.get(11)?;
                // The LEFT JOIN leaves every `progress` column NULL for a document with none, so
                // `p.page` is the presence check: it is NOT NULL whenever the row exists.
                let progress: Option<LatestProgressColumns> =
                    match row.get::<_, Option<i64>>(12)? {
                        Some(page) => Some((
                            page,
                            row.get(13)?,
                            row.get(14)?,
                            row.get(15)?,
                            row.get(16)?,
                            row.get(17)?,
                        )),
                        None => None,
                    };
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    kind,
                    volume,
                    index,
                    hash,
                    pages,
                    title,
                    author,
                    user_title,
                    cover_page,
                    cover_image,
                    progress,
                ))
            })
            .map_err(storage_error)?;

        // One read for every document's list, shared with `document_by_id` and
        // `find_document_by_identity` so all three cannot disagree about who wrote a document.
        let mut reader_authors = self.reader_authors()?;

        let mut entries = Vec::new();
        for row in rows {
            let (
                id,
                path,
                kind,
                volume,
                index,
                hash,
                pages,
                title,
                author,
                user_title,
                cover_page,
                cover_image,
                progress,
            ) = row.map_err(storage_error)?;

            let (progress, progress_recorded_at) = match progress {
                Some((page, total, source, confidence, read_at, updated_at)) => {
                    let page = u32::try_from(page).map_err(|_| {
                        OctError::Storage(format!("stored page {page} is not a page number"))
                    })?;
                    let total = total.map(u32::try_from).transpose().map_err(|_| {
                        OctError::Storage("stored total is not a valid page count".into())
                    })?;
                    (
                        Some(
                            StoredProgress::new(
                                page,
                                total,
                                parse_progress_source(&source),
                                parse_confidence(&confidence)?,
                            )
                            .with_read_at(read_at),
                        ),
                        Some(updated_at),
                    )
                }
                None => (None, None),
            };

            entries.push(LibraryEntry {
                document: DocumentRecord {
                    id,
                    document: decode_document(
                        path,
                        kind,
                        volume,
                        index,
                        hash,
                        pages,
                        title,
                        author,
                        user_title,
                        reader_authors.remove(&id).unwrap_or_default(),
                        cover_page,
                        cover_image,
                    )?,
                },
                progress,
                progress_recorded_at,
            });
        }
        Ok(entries)
    }

    /// One document, by its row id, or `None` when no row carries that id.
    ///
    /// The document columns are read exactly as [`Store::library`] reads them, through the same
    /// [`decode_document`] helper, so a document fetched by id and the same document fetched in the
    /// listing can never disagree about a field. A caller that needs one document therefore does
    /// not have to pull the whole library to find it.
    pub fn document_by_id(&self, id: i64) -> Result<Option<DocumentRecord>, OctError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, path, file_id_kind, volume_id, file_index, content_hash, \
                        pages, title, author, user_title, cover_page, cover_image \
                 FROM documents WHERE id = ?1",
            )
            .map_err(storage_error)?;

        let mut rows = statement
            .query_map(params![id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                ))
            })
            .map_err(storage_error)?;

        let Some(row) = rows.next() else {
            return Ok(None);
        };
        let (
            id,
            path,
            kind,
            volume,
            index,
            hash,
            pages,
            title,
            author,
            user_title,
            cover_page,
            cover_image,
        ) = row.map_err(storage_error)?;

        let document = decode_document(
            path,
            kind,
            volume,
            index,
            hash,
            pages,
            title,
            author,
            user_title,
            self.reader_authors()?.remove(&id).unwrap_or_default(),
            cover_page,
            cover_image,
        )?;
        Ok(Some(DocumentRecord { id, document }))
    }

    /// Every watched folder, oldest first.
    pub fn folders(&self) -> Result<Vec<WatchedFolder>, OctError> {
        let mut statement = self
            .connection
            .prepare("SELECT id, path, recursive, enabled FROM folders ORDER BY id")
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(WatchedFolder {
                    id: row.get(0)?,
                    path: PathBuf::from(row.get::<_, String>(1)?),
                    recursive: row.get::<_, i64>(2)? != 0,
                    enabled: row.get::<_, i64>(3)? != 0,
                })
            })
            .map_err(storage_error)?;

        let mut folders = Vec::new();
        for row in rows {
            folders.push(row.map_err(storage_error)?);
        }
        Ok(folders)
    }

    /// The number of rows in `documents`.
    ///
    /// Added for the scanner's idempotence contract (T6): "scanning the same folder twice leaves
    /// no duplicates" cannot be proven by looking known identities up one at a time, because a
    /// duplicate row is exactly what such a lookup would not reveal.
    pub fn document_count(&self) -> Result<usize, OctError> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
            .map_err(storage_error)?;
        usize::try_from(count)
            .map_err(|_| OctError::Storage(format!("document count {count} is not a usize")))
    }

    // --- The suggestion queue (R4) --------------------------------------------------------------

    /// Records one observation of `path` outside the watched folders and returns the row id.
    ///
    /// The first observation inserts a row with `seen_count = 1` and `first_seen_at` at [`unix_now`];
    /// every later one while the row is **undecided** bumps `seen_count`. A row the reader has
    /// already decided is left exactly as it is: not bumped, not re-opened, and above all not
    /// cleared. That is what "a re-detection cannot erase what the reader decided" means here — the
    /// application observing the same file again must never put a dismissed document back in front
    /// of the reader, and accepting one twice is not a decision that exists.
    ///
    /// The lookup and the write are not one transaction, the same accepted residual as
    /// [`Store::record_folder`] and [`Store::upsert_document`]: `oct` opens one store per process
    /// today. A racing second writer cannot duplicate the path, because `suggestions.path` is
    /// UNIQUE — it fails with a constraint error instead of forking the queue.
    pub fn record_suggestion(&self, path: impl AsRef<Path>) -> Result<i64, OctError> {
        let path = path_to_string(path.as_ref())?;

        let existing: Option<(i64, Option<String>)> = self
            .connection
            .query_row(
                "SELECT id, decision FROM suggestions WHERE path = ?1 ORDER BY id LIMIT 1",
                params![path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;

        match existing {
            // A decided suggestion is history: it is returned as it stands, never touched.
            Some((id, Some(_))) => Ok(id),
            Some((id, None)) => {
                self.connection
                    .execute(
                        "UPDATE suggestions SET seen_count = seen_count + 1 WHERE id = ?1",
                        params![id],
                    )
                    .map_err(storage_error)?;
                Ok(id)
            }
            None => {
                self.connection
                    .execute(
                        "INSERT INTO suggestions (path, seen_count, first_seen_at, decided_at, decision) \
                         VALUES (?1, 1, ?2, NULL, NULL)",
                        params![path, unix_now()],
                    )
                    .map_err(storage_error)?;
                Ok(self.connection.last_insert_rowid())
            }
        }
    }

    /// Every suggestion still awaiting the reader's decision, oldest first.
    ///
    /// Ordered by `first_seen_at` and then by id, so the queue is a function of the data and not of
    /// the machine's clock granularity or of when the caller asked. Decided rows are deliberately
    /// absent: the screen shows what still needs an answer, and the record of a decision is read
    /// back by id through [`Store::suggestion`].
    pub fn suggestions(&self) -> Result<Vec<Suggestion>, OctError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, path, seen_count, first_seen_at, decision, decided_at \
                 FROM suggestions WHERE decision IS NULL ORDER BY first_seen_at ASC, id ASC",
            )
            .map_err(storage_error)?;

        let rows = statement
            .query_map([], suggestion_columns)
            .map_err(storage_error)?;

        let mut suggestions = Vec::new();
        for row in rows {
            suggestions.push(suggestion_from_columns(row.map_err(storage_error)?)?);
        }
        Ok(suggestions)
    }

    /// One suggestion by id, decided or not.
    pub fn suggestion(&self, id: i64) -> Result<Option<Suggestion>, OctError> {
        let columns: Option<SuggestionColumns> = self
            .connection
            .query_row(
                "SELECT id, path, seen_count, first_seen_at, decision, decided_at \
                 FROM suggestions WHERE id = ?1",
                params![id],
                suggestion_columns,
            )
            .optional()
            .map_err(storage_error)?;

        columns.map(suggestion_from_columns).transpose()
    }

    /// Records the reader's decision on a suggestion, once.
    ///
    /// A row that already carries a decision is refused with
    /// [`OctError::SuggestionAlreadyDecided`], naming which suggestion and what is on record, rather
    /// than being overwritten: the first answer is the reader's, and a second write would erase it.
    /// The `WHERE decision IS NULL` on the statement is what makes the guard atomic, so a second
    /// caller between the read and the write cannot slip a decision past it.
    pub fn decide_suggestion(
        &self,
        id: i64,
        decision: SuggestionDecision,
    ) -> Result<Suggestion, OctError> {
        let affected = self
            .connection
            .execute(
                "UPDATE suggestions SET decision = ?1, decided_at = ?2 \
                 WHERE id = ?3 AND decision IS NULL",
                params![decision.id(), unix_now(), id],
            )
            .map_err(storage_error)?;

        if affected == 0 {
            return match self.suggestion(id)? {
                Some(existing) => Err(OctError::SuggestionAlreadyDecided {
                    path: existing.path.to_string_lossy().into_owned(),
                    decision: existing
                        .decision
                        .map(SuggestionDecision::id)
                        .unwrap_or("decided"),
                }),
                None => Err(OctError::Storage(format!(
                    "no suggestion with id {id} to decide"
                ))),
            };
        }

        self.suggestion(id)?.ok_or_else(|| {
            OctError::Storage(format!(
                "suggestion {id} disappeared after its decision was recorded"
            ))
        })
    }

    fn find_id_by_identity(&self, identity: &FileId) -> Result<Option<i64>, OctError> {
        let (kind, volume, index, hash) = encode_identity(identity);
        self.connection
            .query_row(
                "SELECT id FROM documents \
                 WHERE file_id_kind = ?1 AND volume_id IS ?2 AND file_index IS ?3 AND content_hash IS ?4 \
                 LIMIT 1",
                params![kind, volume.as_deref(), index.as_deref(), hash.as_deref()],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(storage_error)
    }

    /// Finds the newest row for `path`, together with the identity kind recorded on it.
    ///
    /// A path can legitimately hold more than one row, because a file replaced by a different one at
    /// the same path is a different document (see [`Store::upsert_document`]). The newest row is the
    /// one that currently describes what lives at that path, so it is the one a path lookup must
    /// resolve to.
    fn find_id_by_path(&self, path: &str) -> Result<Option<(i64, Option<String>)>, OctError> {
        self.connection
            .query_row(
                "SELECT id, file_id_kind FROM documents WHERE path = ?1 ORDER BY id DESC LIMIT 1",
                params![path],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()
            .map_err(storage_error)
    }
}

/// The database file this build uses on the current platform.
///
/// On Linux this is `$XDG_DATA_HOME/oct/oct.sqlite3` (or `~/.local/share/oct/oct.sqlite3` when
/// `XDG_DATA_HOME` is unset), on Windows `%APPDATA%\oct\oct.sqlite3`, and on macOS
/// `~/Library/Application Support/oct/oct.sqlite3`. It is never the repository and never the
/// current working directory.
pub fn database_path() -> Result<PathBuf, OctError> {
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);

    let directory = user_data_dir_for(
        Platform::current(),
        xdg_data_home.as_deref(),
        home.as_deref(),
        appdata.as_deref(),
    )?;
    Ok(directory.join(DATABASE_FILE))
}

/// Resolves `oct`'s user data directory for an explicit platform and environment.
///
/// This is the testable core of [`database_path`]: it takes the platform and the three
/// environment variables that are consulted, so the per-OS rules can be exercised without mutating
/// the process environment or touching a real home directory.
///
/// Returns [`OctError::NotSupported`] for a platform `oct` does not target, and
/// [`OctError::Storage`] when the required environment variable is missing.
pub fn user_data_dir_for(
    platform: Platform,
    xdg_data_home: Option<&Path>,
    home: Option<&Path>,
    appdata: Option<&Path>,
) -> Result<PathBuf, OctError> {
    match platform {
        Platform::Linux => {
            let base = match xdg_data_home {
                Some(directory) if !directory.as_os_str().is_empty() => directory.to_path_buf(),
                _ => home
                    .ok_or_else(|| {
                        OctError::Storage(
                            "cannot locate the user data directory: neither XDG_DATA_HOME nor HOME is set"
                                .to_string(),
                        )
                    })?
                    .join(".local/share"),
            };
            Ok(base.join(APP_DIR))
        }
        Platform::Windows => {
            let appdata = appdata.ok_or_else(|| {
                OctError::Storage(
                    "cannot locate the user data directory: APPDATA is not set".to_string(),
                )
            })?;
            Ok(appdata.join(APP_DIR))
        }
        Platform::MacOs => {
            let home = home.ok_or_else(|| {
                OctError::Storage(
                    "cannot locate the user data directory: HOME is not set".to_string(),
                )
            })?;
            Ok(home.join("Library/Application Support").join(APP_DIR))
        }
        Platform::Other => Err(OctError::NotSupported {
            capability: "user data directory",
            platform,
        }),
    }
}

/// Brings `connection` to [`Store::CURRENT_SCHEMA_VERSION`], forward-only.
fn migrate(connection: &mut Connection) -> Result<(), OctError> {
    let mut version = read_schema_version(connection)?;

    if version > Store::CURRENT_SCHEMA_VERSION {
        return Err(OctError::Storage(format!(
            "database schema version {version} is newer than this build supports \
             ({}); refusing to downgrade",
            Store::CURRENT_SCHEMA_VERSION
        )));
    }

    // One step per version, each recorded as it is applied, so a database left at any version by an
    // older build is brought forward by exactly the steps it is missing. Every step is applied in
    // its own transaction: an interrupted migration leaves the recorded version untouched, so the
    // next open retries the same step instead of finding half a schema.
    if version < 1 {
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction.execute_batch(SCHEMA_V1).map_err(storage_error)?;
        transaction
            .pragma_update(None, "user_version", 1i64)
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        version = 1;
    }

    if version < 2 {
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction.execute_batch(SCHEMA_V2).map_err(storage_error)?;
        transaction
            .pragma_update(None, "user_version", 2i64)
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
    }

    if version < 3 {
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction.execute_batch(SCHEMA_V3).map_err(storage_error)?;
        transaction
            .pragma_update(None, "user_version", 3i64)
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
    }

    if version < 4 {
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction.execute_batch(SCHEMA_V4).map_err(storage_error)?;
        transaction
            .pragma_update(None, "user_version", 4i64)
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        version = 4;
    }

    if version < 5 {
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction.execute_batch(SCHEMA_V5).map_err(storage_error)?;
        transaction
            .pragma_update(None, "user_version", 5i64)
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        version = 5;
    }

    if version < 6 {
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction.execute_batch(SCHEMA_V6).map_err(storage_error)?;
        transaction
            .pragma_update(None, "user_version", 6i64)
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
    }

    if version < 7 {
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction.execute_batch(SCHEMA_V7).map_err(storage_error)?;
        transaction
            .pragma_update(None, "user_version", 7i64)
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
    }

    if version < 8 {
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction.execute_batch(SCHEMA_V8).map_err(storage_error)?;
        transaction
            .pragma_update(None, "user_version", 8i64)
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        version = 8;
    }

    if version < 9 {
        // The three parts of this step are deliberately ONE transaction: the two tables, the backfill
        // that moves the reader's existing author into them, and the retirement of the column it came
        // from. A failure anywhere — including in the `DROP COLUMN` at the end, after the tables exist
        // and the rows are written — leaves the version at 8 with every row where it was, so the next
        // open retries the whole step instead of finding half a schema (the V9 atomicity test pins it).
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction.execute_batch(SCHEMA_V9).map_err(storage_error)?;
        backfill_reader_authors(&transaction)?;
        transaction
            .execute_batch(SCHEMA_V9_RETIRE_USER_AUTHOR)
            .map_err(storage_error)?;
        transaction
            .pragma_update(None, "user_version", 9i64)
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
    }

    Ok(())
}

/// Moves every non-blank `documents.user_author` into a one-entry `document_authors` row (R42/S1).
///
/// This is the migration's promise that **no reader's author is lost**: the value the V7 column held
/// becomes the reader's list, at `position = 0`, before [`SCHEMA_V9_RETIRE_USER_AUTHOR`] drops the
/// column it came from. A blank or absent value becomes no row at all, because it never was an author
/// — `NULL` meant "the reader has not set one", and a blank string was already normalised to "not
/// set" on the way in.
///
/// It runs in Rust rather than as `INSERT ... SELECT` because the `name_key` that makes "one row per
/// person" a database invariant is [`author_key`] — trimmed, Unicode-lowercased — and SQLite's own
/// `lower()` folds ASCII only. Two documents written by the same person in different cases therefore
/// share one `authors` row after the migration, exactly as they would after a save.
fn backfill_reader_authors(transaction: &rusqlite::Transaction<'_>) -> Result<(), OctError> {
    let mut statement = transaction
        .prepare("SELECT id, user_author FROM documents WHERE user_author IS NOT NULL ORDER BY id")
        .map_err(storage_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(storage_error)?;

    let mut moved: Vec<(i64, String)> = Vec::new();
    for row in rows {
        let (doc_id, author) = row.map_err(storage_error)?;
        if let Some(name) = normalize_user_author(Some(&author)) {
            moved.push((doc_id, name.to_string()));
        }
    }
    // The statement borrows the transaction, so it is dropped before anything is written.
    drop(statement);

    for (doc_id, name) in moved {
        let author_id = upsert_author(transaction, &name)?;
        transaction
            .execute(
                "INSERT INTO document_authors (doc_id, author_id, position) VALUES (?1, ?2, 0)",
                params![doc_id, author_id],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

/// The `authors.id` of `name`, inserting the person the first time the library meets them (R42/S2).
///
/// **The spelling that arrives first wins.** The statement is an upsert whose update branch assigns the
/// existing name to itself: it writes nothing, and exists only so that one statement returns the row id
/// whether the person was already known or is being inserted now. That is also why it cannot be a
/// read-then-insert — a second writer racing this one would fork the person into two rows, and the
/// database's `UNIQUE(name_key)` is what stops it.
fn upsert_author(connection: &Connection, name: &str) -> Result<i64, OctError> {
    connection
        .query_row(
            "INSERT INTO authors (name, name_key) VALUES (?1, ?2) \
             ON CONFLICT(name_key) DO UPDATE SET name = authors.name \
             RETURNING id",
            params![name, author_key(name)],
            |row| row.get::<_, i64>(0),
        )
        .map_err(storage_error)
}

fn read_schema_version(connection: &Connection) -> Result<u32, OctError> {
    let raw: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage_error)?;
    u32::try_from(raw)
        .map_err(|_| OctError::Storage(format!("database reports an invalid schema version {raw}")))
}

/// Splits a [`FileId`] into its storage columns: `(kind, volume_id, file_index, content_hash)`.
fn encode_identity(
    identity: &FileId,
) -> (&'static str, Option<String>, Option<String>, Option<String>) {
    match identity {
        FileId::Posix { dev, ino } => (
            "posix",
            Some(dev.to_string()),
            Some(ino.to_string()),
            None,
        ),
        FileId::Windows {
            volume_serial,
            file_index,
        } => (
            "windows",
            Some(volume_serial.to_string()),
            Some(file_index.to_string()),
            None,
        ),
        FileId::ContentHash(hash) => ("content-hash", None, None, Some(hash.clone())),
    }
}

/// Rebuilds a [`FileId`] from its storage columns. A missing kind means "no identity".
fn decode_identity(
    kind: Option<&str>,
    volume_id: Option<&str>,
    file_index: Option<&str>,
    content_hash: Option<&str>,
) -> Result<Option<FileId>, OctError> {
    match kind {
        None => Ok(None),
        Some("posix") => Ok(Some(FileId::Posix {
            dev: parse_u64(volume_id, "volume_id")?,
            ino: parse_u64(file_index, "file_index")?,
        })),
        Some("windows") => Ok(Some(FileId::Windows {
            volume_serial: parse_u64(volume_id, "volume_id")?,
            file_index: parse_u64(file_index, "file_index")?,
        })),
        Some("content-hash") => {
            let hash = content_hash.ok_or_else(|| {
                OctError::Storage("stored content-hash identity has no hash".to_string())
            })?;
            Ok(Some(FileId::ContentHash(hash.to_string())))
        }
        Some(other) => Err(OctError::Storage(format!(
            "unknown file identity kind `{other}`"
        ))),
    }
}

/// The document columns of a row, decoded into a [`Document`].
///
/// Shared by the listing and the single-document lookup, so a document fetched by id and the same
/// document fetched in the listing are decoded by the same code and cannot disagree about a field.
/// `authors` is the reader's ordered list, read by [`Store::reader_authors`] rather than by a join
/// here, because a joined list would have to be re-grouped at every call site.
#[allow(clippy::too_many_arguments)]
fn decode_document(
    path: String,
    kind: Option<String>,
    volume: Option<String>,
    index: Option<String>,
    hash: Option<String>,
    pages: Option<i64>,
    title: Option<String>,
    author: Option<String>,
    user_title: Option<String>,
    authors: Vec<String>,
    cover_page: Option<i64>,
    cover_image: Option<String>,
) -> Result<Document, OctError> {
    let identity = decode_identity(
        kind.as_deref(),
        volume.as_deref(),
        index.as_deref(),
        hash.as_deref(),
    )?;
    let pages = pages.map(u32::try_from).transpose().map_err(|_| {
        OctError::Storage("stored page count is not a valid page number".into())
    })?;
    let cover_page = cover_page.map(u32::try_from).transpose().map_err(|_| {
        OctError::Storage("stored cover page is not a valid page number".into())
    })?;

    Ok(Document {
        path: PathBuf::from(path),
        identity,
        pages,
        title,
        author,
        user_title,
        authors,
        cover_page,
        cover_image,
    })
}

fn parse_u64(value: Option<&str>, column: &str) -> Result<u64, OctError> {
    let value =
        value.ok_or_else(|| OctError::Storage(format!("identity column `{column}` is missing")))?;
    value.parse::<u64>().map_err(|_| {
        OctError::Storage(format!("identity column `{column}` is not a valid u64: `{value}`"))
    })
}

fn confidence_id(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::Exact => "exact",
        Confidence::CloseTime => "close-time",
        Confidence::Manual => "manual",
        Confidence::Unknown => "unknown",
    }
}

fn parse_confidence(value: &str) -> Result<Confidence, OctError> {
    match value {
        "exact" => Ok(Confidence::Exact),
        "close-time" => Ok(Confidence::CloseTime),
        "manual" => Ok(Confidence::Manual),
        "unknown" => Ok(Confidence::Unknown),
        other => Err(OctError::Storage(format!(
            "unknown confidence value `{other}`"
        ))),
    }
}

/// Rebuilds a [`ProgressSource`] from its stored id.
///
/// `native` and `manual` are the two built-in sources; anything else is an external provider id.
/// [`ProgressSource::External`] holds a `&'static str`, so the id is interned rather than leaked
/// on every read.
fn parse_progress_source(value: &str) -> ProgressSource {
    match value {
        "native" => ProgressSource::Native,
        "manual" => ProgressSource::Manual,
        other => ProgressSource::External(intern(other)),
    }
}

fn intern(value: &str) -> &'static str {
    static INTERNED: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();

    let mut interned = INTERNED
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(existing) = interned.get(value) {
        return existing;
    }

    let leaked: &'static str = Box::leak(value.to_owned().into_boxed_str());
    interned.insert(leaked);
    leaked
}

/// The `suggestions` columns every read shares, before they are interpreted.
type SuggestionColumns = (i64, String, i64, i64, Option<String>, Option<i64>);

/// Reads one `suggestions` row into its raw columns.
fn suggestion_columns(row: &rusqlite::Row<'_>) -> rusqlite::Result<SuggestionColumns> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
    ))
}

/// Interprets a raw row, refusing a decision id this build does not know.
///
/// An unrecognised id is an error rather than an undecided row, exactly as [`parse_confidence`]
/// treats an unknown confidence: a value a newer build wrote must not be silently presented as
/// "still awaiting a decision", because the reader could then decide it again and overwrite the
/// answer already on record.
fn suggestion_from_columns(columns: SuggestionColumns) -> Result<Suggestion, OctError> {
    let (id, path, seen_count, first_seen_at, decision, decided_at) = columns;
    let seen_count = u32::try_from(seen_count).map_err(|_| {
        OctError::Storage(format!("suggestion {id} has an invalid seen count {seen_count}"))
    })?;
    let decision = match decision.as_deref() {
        None => None,
        Some("accepted") => Some(SuggestionDecision::Accepted),
        Some("discarded") => Some(SuggestionDecision::Discarded),
        Some(other) => {
            return Err(OctError::Storage(format!(
                "suggestion {id} carries an unknown decision `{other}`"
            )))
        }
    };

    Ok(Suggestion {
        id,
        path: PathBuf::from(path),
        seen_count,
        first_seen_at,
        decision,
        decided_at,
    })
}

fn path_to_string(path: &Path) -> Result<String, OctError> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        OctError::InvalidPath(format!("path is not valid UTF-8: {}", path.display()))
    })
}

fn unix_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

fn storage_error(error: rusqlite::Error) -> OctError {
    OctError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a database exactly as version 1 left it: the V1 schema, a recorded folder, and the
    /// version stamp. Built from `SCHEMA_V1` itself rather than a copy, so the fixture cannot drift
    /// away from the schema it claims to reproduce.
    fn a_version_1_database() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("oct.sqlite3");
        let connection = Connection::open(&path).expect("open raw");
        connection
            .execute_batch(SCHEMA_V1)
            .expect("create the version 1 schema");
        connection
            .execute(
                "INSERT INTO folders (path, recursive, enabled, added_at) VALUES (?1, 1, 1, 7)",
                params!["/library/watched"],
            )
            .expect("record a folder the way version 1 did");
        connection
            .pragma_update(None, "user_version", 1i64)
            .expect("stamp version 1");
        drop(connection);
        (dir, path)
    }

    /// Builds a database exactly as version 2 left it: the V1 schema plus the V2 folder index, a
    /// recorded folder, and the version stamp. Both schema constants are used directly so the
    /// fixture cannot drift away from the schemas it reproduces.
    fn a_version_2_database() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("oct.sqlite3");
        let connection = Connection::open(&path).expect("open raw");
        connection
            .execute_batch(SCHEMA_V1)
            .expect("create the version 1 schema");
        connection
            .execute_batch(SCHEMA_V2)
            .expect("apply the version 2 migration");
        connection
            .execute(
                "INSERT INTO folders (path, recursive, enabled, added_at) VALUES (?1, 1, 1, 7)",
                params!["/library/watched"],
            )
            .expect("record a folder the way version 2 did");
        connection
            .pragma_update(None, "user_version", 2i64)
            .expect("stamp version 2");
        drop(connection);
        (dir, path)
    }

    /// Builds a database exactly as version 3 left it: the V1 schema plus the V2 folder index and
    /// the V3 settings table, a watched folder, a document with one position, and the version
    /// stamp. All three schema constants are used directly so the fixture cannot drift away from
    /// the schemas it reproduces.
    fn a_version_3_database() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("oct.sqlite3");
        let connection = Connection::open(&path).expect("open raw");
        connection
            .execute_batch(SCHEMA_V1)
            .expect("create the version 1 schema");
        connection
            .execute_batch(SCHEMA_V2)
            .expect("apply the version 2 migration");
        connection
            .execute_batch(SCHEMA_V3)
            .expect("apply the version 3 migration");
        connection
            .execute(
                "INSERT INTO folders (path, recursive, enabled, added_at) VALUES (?1, 1, 1, 7)",
                params!["/library/watched"],
            )
            .expect("record a folder the way version 3 did");
        // A document with a position, both written before `read_at` existed, so the fixture is
        // exactly the shape a real pre-V4 database has when the migration reaches it.
        connection
            .execute(
                "INSERT INTO documents (path, path_normalized, uri, first_seen_at) \
                 VALUES (?1, ?1, ?2, 9)",
                params!["/library/read.pdf", "file:///library/read.pdf"],
            )
            .expect("record a document the way version 3 did");
        connection
            .execute(
                "INSERT INTO progress (doc_id, page, total, source, confidence, updated_at) \
                 VALUES (1, 5, 80, ?1, ?2, 99)",
                params!["native", "exact"],
            )
            .expect("record a position the way version 3 did");
        connection
            .pragma_update(None, "user_version", 3i64)
            .expect("stamp version 3");
        drop(connection);
        (dir, path)
    }

    /// Builds a database exactly as version 4 left it: V1..V4 applied, a watched folder, a document
    /// that carries its own title, and a position with a reading time. The V5 migration must add the
    /// reader's-title column without touching the document's own title.
    fn a_version_4_database() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("oct.sqlite3");
        let connection = Connection::open(&path).expect("open raw");
        connection
            .execute_batch(SCHEMA_V1)
            .expect("create the version 1 schema");
        connection
            .execute_batch(SCHEMA_V2)
            .expect("apply the version 2 migration");
        connection
            .execute_batch(SCHEMA_V3)
            .expect("apply the version 3 migration");
        connection
            .execute_batch(SCHEMA_V4)
            .expect("apply the version 4 migration");
        connection
            .execute(
                "INSERT INTO folders (path, recursive, enabled, added_at) VALUES (?1, 1, 1, 7)",
                params!["/library/watched"],
            )
            .expect("record a folder the way version 4 did");
        // The document's own title, written before the reader's-title column existed.
        connection
            .execute(
                "INSERT INTO documents (path, path_normalized, uri, title, first_seen_at) \
                 VALUES (?1, ?1, ?2, ?3, 9)",
                params![
                    "/library/read.pdf",
                    "file:///library/read.pdf",
                    "The document's own title"
                ],
            )
            .expect("record a document the way version 4 did");
        connection
            .execute(
                "INSERT INTO progress (doc_id, page, total, source, confidence, read_at, updated_at) \
                 VALUES (1, 5, 80, ?1, ?2, 1234, 99)",
                params!["native", "exact"],
            )
            .expect("record a position the way version 4 did");
        connection
            .pragma_update(None, "user_version", 4i64)
            .expect("stamp version 4");
        drop(connection);
        (dir, path)
    }

    #[test]
    fn a_version_4_database_gains_the_readers_title_without_touching_the_documents() {
        let (_dir, path) = a_version_4_database();

        let store = Store::open(&path).expect("opening migrates the database");
        assert_eq!(store.schema_version().expect("schema version"), Store::CURRENT_SCHEMA_VERSION);

        // The document's own title survives the V4 -> V5 step untouched, and it reads back with no
        // reader's title: the migration adds a fact, it does not invent one.
        let library = store.library().expect("list the library");
        assert_eq!(library.len(), 1);
        assert_eq!(
            library[0].document.document.title.as_deref(),
            Some("The document's own title")
        );
        assert_eq!(library[0].document.document.user_title, None);

        // The V6 columns exist as well, and they start empty: no cover choice was ever made for a
        // document that predates them, and `None` is the rule's own default (the first page).
        assert_eq!(library[0].document.document.cover_page, None);
        assert_eq!(library[0].document.document.cover_image, None);

        // The column the migration exists for is live afterwards, and the reader's title wins for
        // display while the document's own title stays stored exactly as it was.
        let doc_id = library[0].document.id;
        store
            .set_user_naming(doc_id, Some("The reader's title"), AuthorEdit::Untouched)
            .expect("set the reader's title");
        let library = store.library().expect("list the library again");
        assert_eq!(
            library[0].document.document.user_title.as_deref(),
            Some("The reader's title")
        );
        assert_eq!(
            library[0].document.document.title.as_deref(),
            Some("The document's own title"),
            "the reader's title must not overwrite the document's own"
        );
        assert_eq!(library[0].document.document.display_title(), "The reader's title");
    }

    #[test]
    fn a_version_3_database_gains_read_at_and_keeps_its_rows() {
        let (_dir, path) = a_version_3_database();

        let store = Store::open(&path).expect("opening migrates the database");
        assert_eq!(store.schema_version().expect("schema version"), Store::CURRENT_SCHEMA_VERSION);

        // The watch, the document and its position all survive the V3 -> V4 step.
        let folders = store.folders().expect("list folders");
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, PathBuf::from("/library/watched"));

        let library = store.library().expect("list the library");
        assert_eq!(library.len(), 1);
        let progress = library[0]
            .progress
            .as_ref()
            .expect("the position must survive the migration");
        assert_eq!(progress.page, 5);
        assert_eq!(progress.total, Some(80));
        // A row written before the column existed has no reading time, and the migration must not
        // fabricate one out of `updated_at`: "the app does not know" is the honest answer.
        assert_eq!(
            progress.read_at, None,
            "a pre-V4 row must read back as unknown, not as its own write time"
        );

        // And the column the migration exists for is live afterwards: a reading time round-trips.
        let doc_id = library[0].document.id;
        store
            .record_progress(
                doc_id,
                &StoredProgress::new(6, Some(80), ProgressSource::Native, Confidence::Exact)
                    .with_read_at(Some(1234)),
            )
            .expect("record a position with a reading time");
        let rows = store.progress_for_document(doc_id).expect("read positions");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].read_at, None, "the old row keeps its unknown time");
        assert_eq!(rows[1].read_at, Some(1234), "the new time survives the migration");
    }

    #[test]
    fn a_version_2_database_gains_settings_and_keeps_its_rows() {
        let (_dir, path) = a_version_2_database();

        let store = Store::open(&path).expect("opening migrates the database");
        assert_eq!(store.schema_version().expect("schema version"), Store::CURRENT_SCHEMA_VERSION);

        // What the older build recorded survives, including the flags the user owns.
        let folders = store.folders().expect("list folders");
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, PathBuf::from("/library/watched"));
        assert!(folders[0].recursive);
        assert!(folders[0].enabled);

        // The table the V3 migration exists for is usable afterwards. This is the language the
        // desktop process reads to word the tray, so the migration is what R1 depends on.
        store
            .set_setting("language", "es")
            .expect("the settings table must exist after the V2 -> 3 migration");
        assert_eq!(
            store.get_setting("language").expect("read"),
            Some("es".to_string())
        );
    }

    #[test]
    fn a_version_1_database_gains_the_folder_constraint_when_it_migrates() {
        // The reason SCHEMA_V1 is never edited in place: a database an earlier build already created
        // must be brought forward by a migration, not silently left behind while the code claims a
        // constraint it does not have.
        let (_dir, path) = a_version_1_database();

        let store = Store::open(&path).expect("opening migrates the database");
        assert_eq!(store.schema_version().expect("schema version"), Store::CURRENT_SCHEMA_VERSION);

        // What the older build recorded survives, including the flags the user owns.
        let folders = store.folders().expect("list folders");
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, PathBuf::from("/library/watched"));
        assert!(folders[0].recursive);
        assert!(folders[0].enabled);

        // V1 arrives at the same current version as everything else, settings table included.
        store
            .set_setting("language", "es")
            .expect("the settings table must exist after the V1 -> 3 migration");
        assert_eq!(
            store.get_setting("language").expect("read"),
            Some("es".to_string())
        );

        // And the constraint the migration exists for is live afterwards.
        let connection = Connection::open(&path).expect("open raw");
        let duplicate = connection.execute(
            "INSERT INTO folders (path, recursive, enabled, added_at) VALUES (?1, 0, 1, 0)",
            params!["/library/watched"],
        );
        assert!(
            duplicate.is_err(),
            "a migrated database must reject a duplicate folder, got {duplicate:?}"
        );
    }

    #[test]
    fn a_migrated_database_is_not_rewritten_on_the_next_open() {
        let (_dir, path) = a_version_1_database();
        drop(Store::open(&path).expect("migrate"));
        let before = std::fs::read(&path).expect("read the database bytes");

        let store = Store::open(&path).expect("reopen");
        assert_eq!(store.schema_version().expect("schema version"), Store::CURRENT_SCHEMA_VERSION);
        drop(store);

        let after = std::fs::read(&path).expect("read the database bytes");
        assert_eq!(after, before, "a database already current must not be rewritten");
    }

    /// A watched folder whose single document has a position and an open event, for the removal tests
    /// below. It returns the store, the folder's id and the document's id.
    fn a_watch_with_one_read_document() -> (Store, i64, i64) {
        let store = Store::open_in_memory().expect("open in memory");
        let folder = store
            .record_folder("/library/watched", true)
            .expect("record the folder");
        let document_id = store
            .upsert_document(&Document::new("/library/watched/book.pdf"))
            .expect("insert the document");
        store
            .record_progress(
                document_id,
                &StoredProgress::new(7, Some(80), ProgressSource::Manual, Confidence::Manual),
            )
            .expect("record a position");
        store
            .connection
            .execute(
                "INSERT INTO open_events (doc_id, pid, viewer_id, started_at) VALUES (?1, 1, NULL, 0)",
                params![document_id],
            )
            .expect("record an open event");
        (store, folder, document_id)
    }

    /// R27's atomicity claim: the documents and the watch row go together. A test that only exercises a
    /// SUCCESSFUL removal cannot tell one transaction from two statements in a row, so this one makes
    /// the second statement fail and asserts the first was undone rather than left behind. The failure
    /// is injected with a trigger because the store exposes no seam for an artificial error, and adding
    /// one would test the seam instead of the transaction.
    #[test]
    fn an_interrupted_removal_leaves_the_watch_and_its_documents_alone() {
        let (store, folder, document_id) = a_watch_with_one_read_document();
        store
            .connection
            .execute_batch(
                "CREATE TRIGGER refuse_to_forget BEFORE DELETE ON folders \
                 BEGIN SELECT RAISE(ABORT, 'the second statement fails'); END;",
            )
            .expect("install the trigger");

        let outcome = store.forget_folder(folder);

        assert!(
            outcome.is_err(),
            "the injected failure has to reach the caller, got {outcome:?}"
        );
        assert_eq!(
            store.document_count().expect("count the documents"),
            1,
            "the document must come back when the watch could not be removed"
        );
        assert_eq!(
            store.folders().expect("read the folders").len(),
            1,
            "the watch must survive its own failed removal"
        );
        assert_eq!(
            store
                .progress_for_document(document_id)
                .expect("read the position")
                .len(),
            1,
            "the position must come back with the document"
        );
        let events: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM open_events", [], |row| row.get(0))
            .expect("count the open events");
        assert_eq!(events, 1, "the open event must come back with the document");
    }

    /// The cascade R27 relies on for the rows that hang off a document. `progress` is asserted through
    /// the public interface by the removal tests in `tests/store.rs`; `open_events` has no public
    /// reader yet, so its rows are counted here — which is also what would catch the foreign-key
    /// pragma being dropped, since without it nothing deletes them and the count stays at one.
    #[test]
    fn removing_a_watch_takes_the_open_events_of_its_documents() {
        let (store, folder, _document_id) = a_watch_with_one_read_document();

        let removed = store.forget_folder(folder).expect("remove the watch");

        assert!(removed.is_some(), "the watch was there to remove");
        assert_eq!(store.document_count().expect("count the documents"), 0);
        let events: i64 = store
            .connection
            .query_row("SELECT COUNT(*) FROM open_events", [], |row| row.get(0))
            .expect("count the open events");
        assert_eq!(events, 0, "an open event cannot outlive its document");
    }
}
