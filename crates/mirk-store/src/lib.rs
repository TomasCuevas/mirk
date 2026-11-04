//! Persistence for `mirk`.
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
//! | `reading_sessions` | One row per reading session in `mirk`'s own reader: document, start, end, and the visible-and-focused milliseconds actually spent reading. |
//! | `page_visits` | One row per page that was actually on screen in the reader: the session it happened in, when it was observed, and how long it stayed there. |
//!
//! # The two decisions worth preserving
//!
//! **`progress` keeps `source` and `confidence` as separate columns.** A position read from
//! Evince's store, one typed in by a person, and one produced by `mirk`'s own reader are not the
//! same kind of fact, and they do not deserve the same authority. Collapsing them into a single
//! integer produces an interface that cannot say *"page 12 of 80, recorded when Evince closed"*
//! and therefore silently presents stale data as current. Two rows with the same page but
//! different provenance remain two rows.
//!
//! **A document's authors are rows, not a string.** `documents.author` is the PDF's own embedded
//! author — one opaque string, read-only, never split (R42/S6) — while the authors a reader chose are
//! their own ordered list across `authors` and `document_authors`, so "several authors per work" is
//! stored as facts instead of as a delimiter somebody would have to guess at. The rule that turns the
//! two into what a screen shows lives in one place, [`mirk_core::Document::display_authors`], so the
//! card, the filter and the search cannot disagree about who wrote a document (R42/S4).
//!
//! **`documents` stores portable identity, never only a path.** Paths change when a file is moved
//! or renamed; identity is what lets the library follow a document instead of treating it as new
//! and losing its reading history. Because `(device, inode)` does not exist on Windows, identity
//! is the [`mirk_core::FileId`] enum — and because some filesystems expose nothing usable, a content
//! hash is a first-class variant rather than a hidden fallback. All three variants are stored in
//! the `file_id_kind` / `volume_id` / `file_index` / `content_hash` columns and can be looked up
//! without a path. The numeric ids are stored as decimal TEXT so the full `u64` range survives
//! SQLite's signed 64-bit `INTEGER`.
//!
//! # Reading facts are first-party, and durations are given rather than computed
//!
//! `reading_sessions` and `page_visits` are the two facts only `mirk`'s own reader can honestly
//! produce (F1 part 1). `open_events` already records that *some* viewer had a document open; it
//! cannot say whether anybody read, and `mirk` cannot vouch for time it did not observe (D3), so no
//! external-viewer session is stored here.
//!
//! **The schema measures nothing; this build only stores what the reader measures.** `active_ms`
//! and `visible_ms` are durations the reader will hand over already measured under the owner's rule
//! (L4): only visible-and-focused time counts, paused after 20 minutes without interaction. The
//! tables exist as the home for those facts, and [`Store::record_reading_session`] writes a session
//! exactly as given. Capture is deliberately absent from this build: no reader observes a session
//! yet and nothing calls the recording method, so no reading fact is written in production and none
//! of the timing rules above is implemented.
//!
//! **A page visit keeps its own moment and its session.** `observed_at` is when the page was on
//! screen and `session_id` is the session it happened in, so the read order is chronology rather
//! than insertion order: a writer that flushes visits late cannot make an earlier page look later,
//! and a visit cannot be attributed to a session of another document.
//!
//! **Time is stored, not adjudicated.** `started_at`, `ended_at` and `observed_at` are the reader's
//! UTC wall-clock readings, stored verbatim. The UTC clock can move — an NTP correction, a manual
//! change, a suspend and resume — and a write can arrive after the session it belongs to, so the
//! schema does not require a visit to fall inside its session's span, does not require `ended_at` to
//! be at or after `started_at`, and claims nothing beyond the recorded values: ordering by
//! `observed_at` orders by the clock the reader reported, not by a monotonic timeline whose physical
//! truth is known. What it does require is that every timestamp is non-negative and every measured
//! duration is non-negative — elapsed reading time is not a wall-clock subtraction.
//!
//! # Migrations
//!
//! The schema version is recorded inside the database with SQLite's own `PRAGMA user_version`.
//! Opening a database brings it to [`Store::CURRENT_SCHEMA_VERSION`] and records that version;
//! reopening an already-current database is a no-op and takes **no write reservation** — it can be
//! opened while another process is writing, and it does not stop that write. It is a read like any
//! other, so a lock that excludes readers (a writer mid-commit) still blocks it for as long as the
//! connection's own busy timeout allows; what it never does is queue behind the write lock an upgrade
//! takes. A fresh file is created at the current version. Migrations are forward-only: a database
//! written by a newer build is refused rather than silently downgraded. A database that does need
//! migrating is upgraded as **one serialized write**: the write lock is taken first, the version is
//! re-read under it, every missing step is applied in one transaction, and the connection's own busy
//! timeout — raised only for that wait — is put back afterwards, so competing opens cannot both apply
//! a step and a failure leaves the database at the version it was opened with. See `upgrade` for the
//! rollback and contention consequences of that.
//!
//! **A database stamped with the current version is still checked before it is trusted.** Schema 12
//! was revised in place before it shipped, so a development database can carry an earlier pre-release
//! shape under the same `user_version` stamp. Opening a current database therefore compares the
//! stored definition of the reading tables and their indexes with the statements this build writes,
//! and refuses a database whose shape differs before any reading fact is read or written. The
//! comparison is byte-exact on purpose (see `verify_reading_tables`): a semantically equivalent but
//! differently formatted definition is refused rather than accepted on a guess. An older database
//! that already holds an object the V12 step would create is likewise refused before any step runs,
//! so a collision cannot leave it partially upgraded.
//!
//! **Refusal is not repair.** A database whose reading tables are not this build's shape is left
//! exactly as it was — no table, no row and not the version stamp changes — because the session and
//! time facts a missing column would hold cannot be recovered by guessing. The same refusal names the
//! database, the version, and the objects or columns that differ, so the reader can tell which build
//! wrote it instead of reading a storage error that blames the disk.
//!
//! # Where the database lives
//!
//! Per-platform user data directories, not the repository and not the current working directory:
//! `$XDG_DATA_HOME/mirk` on Linux (falling back to `~/.local/share/mirk`), `%APPDATA%\mirk` on
//! Windows, and `~/Library/Application Support/mirk` on macOS. [`database_path`] resolves it.
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
//! - the `suggestions` table is written and read only through the queue's own methods —
//!   [`Store::record_suggestion`], [`Store::record_suggestion_with_history`],
//!   [`Store::suggestions`], [`Store::suggestion`] and [`Store::decide_suggestion`] — because the
//!   suggestion queue is the only caller, and a decision is the reader's (R4);
//! - metadata writes (`size`, `mtime`, `discovered_via`, `platform`, `status`) — the
//!   [`mirk_core::Document`] value type does not carry them yet, so [`Store::upsert_document`]
//!   leaves those columns unset for the scanner (T6) to fill;
//! - reading facts: the `reading_sessions` and `page_visits` schema and its compatibility guard are
//!   present, and [`Store::record_reading_session`] / [`Store::reading_sessions_for_document`] store
//!   and return first-party sessions through [`ReadingSession`] and [`StoredReadingSession`]. Capture
//!   is still absent: nothing observes a session, and no page visit is written or read. Capture
//!   arrives with the reader that observes it, and with it the `source`, `viewer_id` and
//!   `ended_reason` columns the roadmap sketched: 0.2.0 stores only first-party reader sessions, so
//!   a `source` column could hold one constant, `viewer_id` would presume the still-open answer to
//!   D3, and an `ended_reason`
//!   vocabulary has no owner decision behind it yet;
//! - path normalisation: paths are stored as opaque UTF-8 and are **never** Unicode-normalised
//!   (risk R7), so `path_normalized` deliberately holds the same bytes as `path`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mirk_core::{
    Confidence, CoverChoice, CoverSource, Document, FileId, MirkError, Platform, ProgressSource,
    StoredProgress,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

/// The application directory name appended to each platform's user data directory.
const APP_DIR: &str = "mirk";

/// The application directories this build replaced, newest first.
///
/// `loci` was the product before `mirk`; `oct` was the one before that. A reader may have opened the
/// application under either name, and a rename must not cost them the library — so adoption walks the
/// chain rather than knowing one predecessor.
const LEGACY_APP_DIRS: [&str; 2] = ["loci", "oct"];

/// The database file name inside [`APP_DIR`].
const DATABASE_FILE: &str = "mirk.sqlite3";

/// The database file names those builds wrote, in the same order as [`LEGACY_APP_DIRS`]. Read by
/// [`adopt_legacy_data_dir`] and by [`database_file_in`], so that a file that could not be renamed is
/// still found instead of being shadowed by a newly created empty database.
const LEGACY_DATABASE_FILES: [&str; 2] = ["loci.sqlite3", "oct.sqlite3"];

/// The files that belong to one database besides its main file. SQLite's write-ahead log and
/// shared-memory index are part of the database's state: renaming the main file without them — or the
/// reverse — is how a migration loses the last transaction written before the rename.
const DATABASE_SIDECARS: [&str; 2] = ["-wal", "-shm"];

/// The covers directory, which lives beside the database (see `covers_directory` in the desktop
/// crate). Adoption moves its files when a legacy directory has to be merged into an existing one.
const COVERS_DIR: &str = "covers";

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
/// it: it runs only on the V3 -> V4 step, once, and the step is committed with the rest of the
/// upgrade it belongs to (see `upgrade`).
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
/// rule in [`mirk_core::Document::cover_choice`] and not here: this crate stores the facts and does not
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
/// leave that database without the column forever while claiming otherwise. The step is committed
/// with the rest of the upgrade it belongs to, so a failure leaves the version behind and no
/// half-added column (the V7 atomicity test pins exactly that).
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

/// V10: which of the two cover sources is in force (R19).
///
/// Before this column the rule was "an image wins if there is one", which is a consequence rather
/// than a choice: a reader with an image could not go back to a page without deleting the image, and
/// could not return to the image afterwards. The source is now a stored fact, and both the image and
/// the page can be kept at once.
///
/// The backfill reproduces the old rule exactly — an image means `image`, otherwise `page` — so an
/// existing library opens on the same cover it showed before the upgrade.
const SCHEMA_V10_ADD: &str = "ALTER TABLE documents ADD COLUMN cover_source TEXT;";

/// The backfill, separate from the column so it can run on its own: a database that already has the
/// column (an interrupted upgrade, or a fixture built from the current shape) still gets every row
/// filled in, and the step is idempotent from the outside.
const SCHEMA_V10_BACKFILL: &str = "
UPDATE documents
   SET cover_source = CASE
       WHEN cover_image IS NOT NULL AND TRIM(cover_image) <> '' THEN 'image'
       ELSE 'page'
   END
 WHERE cover_source IS NULL;
";

/// Version 11: when the **reader** opened a suggested document, according to the desktop's own
/// history, as a fact of its own.
///
/// The queue already knew when the application first noticed a file (`first_seen_at`), and that is a
/// different fact: it is when *mirk* looked. L46 already paid for putting the two in one column —
/// every row looked freshly read, and a "not opened in a long time" category could never contain
/// anything — so the history's time is a separate nullable column and `first_seen_at` keeps its
/// meaning untouched. `NULL` means no source said, which is not the same as 1970 and not the same as
/// "opened long ago"; a source that has no time to offer (`/proc` has none) writes `NULL`.
///
/// Added as a migration rather than by editing [`SCHEMA_V1`], because V1 is **frozen**: a database an
/// earlier build already created reports its version, and editing the frozen text would leave that
/// database without the column forever while claiming otherwise. Every existing suggestion row keeps
/// its path, its sighting count and its decision, and reads back with no history time.
const SCHEMA_V11: &str = "
ALTER TABLE suggestions ADD COLUMN history_at INTEGER;
";

/// Version 12: the two facts the first-party reader is the only honest source of (F1 part 1).
///
/// `reading_sessions` is a fact about **reading**, not about a process: `open_events` already records
/// that a viewer had a document open, and it cannot say whether anybody read. `page_visits` is the
/// per-document depth the roadmap asks for — which pages were actually on screen, when, and for how
/// long — keyed by document and naming the session it happened in, because two visits to one page are
/// two facts and a visit without its session has no provenance.
///
/// **Durations are stored, never derived.** `active_ms` is the visible-and-focused time the reader
/// will measure (L4: paused after 20 minutes without interaction), so it is deliberately smaller
/// than the `started_at`/`ended_at` span when the session idles; nothing here compares the two.
/// `CHECK` constraints keep the values a fact at the schema level as well as at the interface, the
/// same discipline `progress` and `document_authors` already follow: an impossible duration is
/// refused by the database even to a writer that bypassed [`Store`].
///
/// **The wall clock is recorded, not trusted to be monotonic.** `started_at` and `ended_at` are the
/// reader's UTC readings, and the clock can be corrected backwards between them — an NTP correction,
/// a manual change, a suspend and resume. Elapsed reading time is not `ended_at - started_at`, so no
/// ordering constraint relates the two: what is required is that each is a non-negative timestamp and
/// that the separately measured `active_ms` is non-negative. A schema that demanded
/// `ended_at >= started_at` would reject an honest session and lose a fact that cannot be recorded
/// again.
///
/// **The schema enforces exactly what a typed read expects.** Every integer column carries a
/// `typeof(...) = 'integer'` check, because an INTEGER column otherwise stores text or a real and a
/// typed read would then fail on a row the schema had already accepted. Timestamps and durations are
/// non-negative, `page` is bounded to a `u32`, and a visit names its session through a composite key
/// `(session_id, doc_id)`, so it cannot be attributed to a session of another document even by a
/// writer that bypassed [`Store`].
///
/// **`ON DELETE CASCADE` is the deletion rule.** A document that leaves the library — the reader
/// removed its watch, and [`Store::forget_folder`] deleted the row — must not leave reading time
/// behind that nothing can attribute, exactly as its `progress` and `open_events` already go with
/// it. A session that goes takes the visits it attributes with it, through the same composite key.
///
/// Added as a migration rather than by editing [`SCHEMA_V1`], because V1 is **frozen**: a database an
/// earlier build already created reports its version, and editing the frozen text would leave that
/// database without these tables forever while claiming otherwise. The step only creates homes for
/// facts; it invents none, so every existing row is untouched.
///
/// **Plain `CREATE TABLE`, deliberately, not `IF NOT EXISTS`.** A table that is already there under
/// one of these names is not this schema's table: this step has never run against it, so the shape is
/// unknown and could be missing the columns and constraints every read depends on. Skipping it would
/// create the rest, stamp 12, and leave a database that claims a schema it does not have — the exact
/// lie [`Store::open`]'s refusal of a newer version exists to prevent. The whole upgrade is one
/// transaction, so refusing here takes back the tables it already created and leaves the recorded
/// version where it was, where the next open retries.
///
/// **The tables are compared on open.** This version was revised in place before it shipped, so a
/// development database can carry an earlier pre-release shape under the same version stamp.
/// [`verify_reading_tables`] therefore compares the stored definition of both tables with the
/// statements below, which is why each table is its own constant: the text the migration executes
/// and the text the check expects cannot drift apart.
const SCHEMA_V12_READING_SESSIONS: &str = "CREATE TABLE reading_sessions (
    id         INTEGER PRIMARY KEY,
    doc_id     INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    started_at INTEGER NOT NULL,
    ended_at   INTEGER NOT NULL,
    active_ms  INTEGER NOT NULL,
    -- The parent key of `page_visits`' composite foreign key: a visit and its session must name
    -- the same document, and SQLite can only enforce that against a unique parent key.
    UNIQUE (id, doc_id),
    -- The storage class is part of the fact: an INTEGER column otherwise accepts text and reals,
    -- and a typed read would then fail on a row the schema had already accepted.
    CHECK (typeof(doc_id) = 'integer'),
    CHECK (typeof(started_at) = 'integer'),
    CHECK (typeof(ended_at) = 'integer'),
    CHECK (typeof(active_ms) = 'integer'),
    CHECK (started_at >= 0),
    CHECK (ended_at >= 0),
    CHECK (active_ms >= 0)
)";

/// The index a document's sessions are read through, newest start last.
const SCHEMA_V12_READING_SESSIONS_INDEX: &str =
    "CREATE INDEX idx_reading_sessions_doc ON reading_sessions (doc_id, started_at)";

/// The statement that creates `page_visits` (F1 part 1).
const SCHEMA_V12_PAGE_VISITS: &str = "CREATE TABLE page_visits (
    id          INTEGER PRIMARY KEY,
    doc_id      INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    session_id  INTEGER NOT NULL,
    page        INTEGER NOT NULL,
    observed_at INTEGER NOT NULL,
    visible_ms  INTEGER NOT NULL,
    -- A visit's session must belong to the same document. One composite key instead of two loose
    -- references, so a raw writer cannot attribute reading time to another document's session.
    FOREIGN KEY (session_id, doc_id) REFERENCES reading_sessions(id, doc_id) ON DELETE CASCADE,
    CHECK (typeof(doc_id) = 'integer'),
    CHECK (typeof(session_id) = 'integer'),
    CHECK (typeof(page) = 'integer'),
    CHECK (typeof(observed_at) = 'integer'),
    CHECK (typeof(visible_ms) = 'integer'),
    -- `u32`: a page outside this range is one a typed read cannot represent.
    CHECK (page BETWEEN 0 AND 4294967295),
    CHECK (observed_at >= 0),
    CHECK (visible_ms >= 0)
)";

/// The index a document's visits are read through, in observation order.
const SCHEMA_V12_PAGE_VISITS_DOC_INDEX: &str =
    "CREATE INDEX idx_page_visits_doc ON page_visits (doc_id, observed_at)";

/// The index that makes deleting a session and its visits cheap.
const SCHEMA_V12_PAGE_VISITS_SESSION_INDEX: &str =
    "CREATE INDEX idx_page_visits_session ON page_visits (session_id, observed_at)";

/// The indexes V12 creates, with the statement this build writes for each.
///
/// Kept as their own list because both the collision preflight and [`verify_reading_tables`] need
/// them by name, and neither should be able to drift away from what the step executes.
const READING_INDEXES: [(&str, &str); 3] = [
    (
        "idx_reading_sessions_doc",
        SCHEMA_V12_READING_SESSIONS_INDEX,
    ),
    ("idx_page_visits_doc", SCHEMA_V12_PAGE_VISITS_DOC_INDEX),
    (
        "idx_page_visits_session",
        SCHEMA_V12_PAGE_VISITS_SESSION_INDEX,
    ),
];

/// Version 12's whole step, statement by statement, in the order it is applied.
///
/// The two `CREATE TABLE` statements are the ones [`verify_reading_tables`] compares a database
/// against, so a change to either of them is a change to what this build accepts on open.
const SCHEMA_V12: [&str; 5] = [
    SCHEMA_V12_READING_SESSIONS,
    SCHEMA_V12_READING_SESSIONS_INDEX,
    SCHEMA_V12_PAGE_VISITS,
    SCHEMA_V12_PAGE_VISITS_DOC_INDEX,
    SCHEMA_V12_PAGE_VISITS_SESSION_INDEX,
];

/// A document `mirk` observed open outside the watched folders, awaiting the reader's decision (R4).
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
    /// Unix seconds when the desktop's own history recorded the reader opening this document, or
    /// [`None`] when no source knows.
    ///
    /// This is the reader's own moment — what the GTK recent-files list wrote down — and it is a
    /// **separate column** from `first_seen_at`, which is the moment the application noticed the
    /// file. L46 already paid for that lesson once: one column for "when I looked" and "when the
    /// reader opened it" made every row look freshly read. `None` says no source said, which is a
    /// different fact from a document opened long ago, and it is what a detector that records no
    /// time writes.
    pub history_at: Option<i64>,
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
    /// Which of the two cover sources is in force (R19), or [`None`] for a row written before the
    /// choice existed — [`DocumentRecord::cover_choice`] then applies the old rule.
    pub cover_source: Option<CoverSource>,
}

impl DocumentRecord {
    /// The cover this document shows, honouring the stored source (R19).
    pub fn cover_choice(&self) -> CoverChoice {
        self.document.cover_choice_from(self.cover_source)
    }
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

/// One reading session in `mirk`'s own reader, as storage keeps it (F1 part 1).
///
/// A session is a fact about **reading**, not about a process: `open_events` already records that
/// some viewer had a document open, and it cannot say whether anybody read. Only the reader writes
/// this type — an external viewer's session would be time `mirk` cannot vouch for (D3), and there is
/// no such row here.
///
/// `started_at` and `ended_at` are the session's wall-clock span in Unix seconds; `active_ms` is the
/// **visible-and-focused** milliseconds inside that span, as the reader measured them. The store
/// derives neither from the other: the owner's rule (L4 — pause after 20 minutes without
/// interaction) is a measurement rule, so a session that idled keeps its active time, which is
/// smaller than the span.
///
/// Both timestamps are UTC wall-clock readings stored verbatim. The clock can be corrected or moved
/// between them, so the store claims nothing about the span beyond the two numbers: it is not a
/// monotonic interval, and `active_ms` is not required to fit inside it. See the crate documentation
/// for the time policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingSession {
    /// The `documents.id` the session read.
    pub doc_id: i64,
    /// Unix seconds when the session started, as the reader's clock read it.
    pub started_at: i64,
    /// Unix seconds when it ended, as the reader's clock read it. The clock can be corrected
    /// backwards in between, so this may be earlier than [`ReadingSession::started_at`].
    pub ended_at: i64,
    /// Visible-and-focused milliseconds actually spent reading; never negative.
    pub active_ms: i64,
}

/// One reading session as it was stored: the row's stable id plus the facts it holds (F1 part 1).
///
/// The id is the provenance a page visit names when it says which session it happened in, so a read
/// has to return it: a caller cannot attribute a visit to a session it can only re-insert. It is
/// assigned by the database on insert and is not a field of [`ReadingSession`], so a caller cannot
/// choose or forge it, and the same row keeps the same id across a close and reopen. It is stable
/// for the life of the row: deleting the document takes its sessions with it, and a deleted
/// session's id may be reused by a later row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredReadingSession {
    /// The `reading_sessions.id` primary key, assigned by the database on insert.
    pub id: i64,
    /// The session facts exactly as they were recorded.
    pub session: ReadingSession,
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
            documents: self.documents.into_iter().map(|(_, path)| path).collect(),
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
fn normalize_user_title(title: Option<&str>) -> Result<Option<&str>, MirkError> {
    match title {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err(MirkError::InvalidInput {
                    field: "title",
                    value: value.to_string(),
                    expected: "a non-empty title, or no value to restore the document's own"
                        .to_string(),
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
fn normalize_user_authors<'a>(names: &[&'a str]) -> Result<Vec<&'a str>, MirkError> {
    let mut kept: Vec<&'a str> = Vec::with_capacity(names.len());
    let mut seen: HashSet<String> = HashSet::with_capacity(names.len());
    for name in names {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(MirkError::InvalidInput {
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

/// A SQLite connection to `mirk`'s database.
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
    pub const CURRENT_SCHEMA_VERSION: u32 = 12;

    /// Opens the database at `path`, creating parent directories and the file when needed, and
    /// brings it to [`Self::CURRENT_SCHEMA_VERSION`].
    ///
    /// Pass [`database_path`] for the production location; tests pass a temporary file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MirkError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(MirkError::Io)?;
            }
        }

        let mut connection = Connection::open(path).map_err(storage_error)?;
        Self::prepare(&mut connection, Some(path))?;
        Ok(Self { connection })
    }

    /// Opens an ephemeral in-memory database, migrated like a file. Used by callers that want a
    /// throwaway store; the production path is [`Self::open`].
    pub fn open_in_memory() -> Result<Self, MirkError> {
        let mut connection = Connection::open_in_memory().map_err(storage_error)?;
        Self::prepare(&mut connection, None)?;
        Ok(Self { connection })
    }

    /// Prepares `connection` for use, with `origin` naming it in the refusals below.
    ///
    /// `origin` is the file the database lives in when there is one, so a refusal can say *which*
    /// database it is about instead of pointing at an anonymous connection. An in-memory database has
    /// no path and reads as "this database".
    fn prepare(connection: &mut Connection, origin: Option<&Path>) -> Result<(), MirkError> {
        // Foreign keys are off by default in SQLite and are per-connection, so they are enabled
        // here rather than assumed.
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(storage_error)?;
        migrate(connection, origin)
    }

    /// The schema version currently recorded in the database.
    pub fn schema_version(&self) -> Result<u32, MirkError> {
        read_schema_version(&self.connection)
    }

    /// The names of the user tables in the database, sorted.
    ///
    /// Exposed so migrations can be diagnosed and the schema contract verified through the public
    /// interface instead of by querying SQLite's metadata tables from outside.
    pub fn table_names(&self) -> Result<Vec<String>, MirkError> {
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
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, MirkError> {
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
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), MirkError> {
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
    /// [`MirkError::Storage`] constraint error. Serialising writers with an immediate transaction and
    /// retrying is deliberately deferred — `mirk` currently opens one store per process, so the race
    /// requires a second process writing the same database, which no code does yet.
    ///
    /// Only the columns the scanner can supply are written; the reader's title and the reader's
    /// authors are deliberately not among them, because they are separate facts with their own write
    /// path ([`Store::set_user_naming`]). `documents.author` in particular is the file's own embedded
    /// string and is written from the scanner's metadata only — never from anything a reader typed,
    /// and never split into names.
    /// The scanner fills metadata such as `size`, `mtime` and `discovered_via` when it exists.
    pub fn upsert_document(&self, document: &Document) -> Result<i64, MirkError> {
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
    ) -> Result<(), MirkError> {
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
            return Err(MirkError::Storage(format!(
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
    pub fn set_cover_page(&self, id: i64, page: Option<u32>) -> Result<(), MirkError> {
        let affected = self
            .connection
            .execute(
                "UPDATE documents SET cover_page = ?1, cover_source = 'page' WHERE id = ?2",
                params![page.map(i64::from), id],
            )
            .map_err(storage_error)?;

        if affected == 0 {
            return Err(MirkError::Storage(format!(
                "no document with id {id} to set a cover page on"
            )));
        }
        Ok(())
    }

    /// Makes one of the two stored sources the cover, without touching either value (R19).
    ///
    /// This is how a reader returns to an image they already supplied, or to the page they had
    /// chosen, without deleting anything: both facts stay in the row and only the source moves.
    pub fn set_cover_source(&self, id: i64, source: CoverSource) -> Result<(), MirkError> {
        let affected = self
            .connection
            .execute(
                "UPDATE documents SET cover_source = ?1 WHERE id = ?2",
                params![source.as_str(), id],
            )
            .map_err(storage_error)?;
        if affected == 0 {
            return Err(MirkError::Storage(format!(
                "no document with id {id} to set a cover source on"
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
    pub fn set_cover_image(&self, id: i64, file_name: Option<&str>) -> Result<(), MirkError> {
        let normalized: Option<&str> = match file_name {
            Some(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Err(MirkError::InvalidInput {
                        field: "cover image",
                        value: value.to_string(),
                        expected:
                            "a file name inside the covers directory, or no value to remove it"
                                .to_string(),
                    });
                }
                if trimmed.contains('/') || trimmed.contains('\\') {
                    return Err(MirkError::InvalidInput {
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
                // Choosing an image makes it the cover; removing it hands the cover back to the page.
                // The page column is untouched either way, so the page the reader had is still there
                // when the image goes away — and the image file itself is not deleted by this method.
                "UPDATE documents SET cover_image = ?1, cover_source = CASE WHEN ?1 IS NULL THEN 'page' ELSE 'image' END WHERE id = ?2",
                params![normalized, id],
            )
            .map_err(storage_error)?;

        if affected == 0 {
            return Err(MirkError::Storage(format!(
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
    ) -> Result<Option<DocumentRecord>, MirkError> {
        let (kind, volume, index, hash) = encode_identity(identity);

        let row = self
            .connection
            .query_row(
                "SELECT id, path, file_id_kind, volume_id, file_index, content_hash, pages, title, author, user_title, cover_page, cover_image, cover_source \
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
                        row.get::<_, Option<String>>(12)?,
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
            cover_source,
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
        let pages = pages.map(u32::try_from).transpose().map_err(|_| {
            MirkError::Storage("stored page count is not a valid page number".into())
        })?;
        let cover_page = cover_page.map(u32::try_from).transpose().map_err(|_| {
            MirkError::Storage("stored cover page is not a valid page number".into())
        })?;

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
            cover_source: decode_cover_source(cover_source),
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
    fn reader_authors(&self) -> Result<HashMap<i64, Vec<String>>, MirkError> {
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
    ) -> Result<i64, MirkError> {
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
    pub fn progress_for_document(&self, doc_id: i64) -> Result<Vec<StoredProgress>, MirkError> {
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
            let page = u32::try_from(page).map_err(|_| {
                MirkError::Storage(format!("stored page {page} is not a page number"))
            })?;
            let total = total
                .map(u32::try_from)
                .transpose()
                .map_err(|_| MirkError::Storage("stored total is not a valid page count".into()))?;

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

    /// Records one completed reading session and returns the new row id (F1 part 1).
    ///
    /// **The store validates, it does not measure.** `active_ms` is stored exactly as given: the
    /// visible-and-focused rule (L4) belongs to the reader, and a store that recomputed it would
    /// disagree with the thing that observed it. What is refused is what cannot be a fact: a
    /// `doc_id` no document carries (nothing to attach the session to), a negative time (Unix
    /// seconds begin at the epoch), or a negative `active_ms`. A refusal writes nothing.
    ///
    /// **A backwards wall-clock span is stored as given.** `started_at` and `ended_at` are two UTC
    /// readings, and the clock can be corrected backwards between them (an NTP correction, a manual
    /// change, a suspend and resume). Elapsed reading time is not `ended_at - started_at`; it is the
    /// separately measured `active_ms`, so this build deliberately does not require `ended_at` to be
    /// at or after `started_at`, does not normalise the span, and never compares `active_ms` with
    /// it. Both readings are kept verbatim, because refusing an honest session would lose a fact
    /// that cannot be recorded again.
    ///
    /// **The id is the database's to assign.** [`ReadingSession`] carries no id field, so a caller
    /// cannot choose one; the returned id is the row's stable primary key, and
    /// [`Store::reading_sessions_for_document`] returns it with the facts.
    ///
    /// **The document check is not atomic with the insert.** When the document is already absent,
    /// the pre-check refuses with [`MirkError::InvalidInput`] naming `document`. A concurrent
    /// deletion between that check and the insert — a [`Store::forget_folder`] running in another
    /// connection — is instead refused by the foreign key, which surfaces as
    /// [`MirkError::Storage`]. This build does not promise `InvalidInput` for that race: the two
    /// refusals are different events, and the storage error is SQLite's constraint failure, not a
    /// stable typed outcome. Either way the refused write stores nothing.
    pub fn record_reading_session(&self, session: &ReadingSession) -> Result<i64, MirkError> {
        self.require_document(session.doc_id)?;
        if session.started_at < 0 {
            return Err(invalid_input(
                "started_at",
                session.started_at,
                "Unix seconds at or after the epoch",
            ));
        }
        if session.ended_at < 0 {
            return Err(invalid_input(
                "ended_at",
                session.ended_at,
                "Unix seconds at or after the epoch",
            ));
        }
        if session.active_ms < 0 {
            return Err(invalid_input(
                "active_ms",
                session.active_ms,
                "a non-negative duration in milliseconds",
            ));
        }

        self.connection
            .execute(
                "INSERT INTO reading_sessions (doc_id, started_at, ended_at, active_ms) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    session.doc_id,
                    session.started_at,
                    session.ended_at,
                    session.active_ms
                ],
            )
            .map_err(storage_error)?;
        Ok(self.connection.last_insert_rowid())
    }

    /// Every reading session stored for a document, in the order of their recorded start times.
    ///
    /// Each item carries the row's stable [`StoredReadingSession::id`] together with its facts, so a
    /// caller can attribute a later page visit to a session that already exists instead of having to
    /// insert it again. Ordering is by `started_at` and then by row id, so two sessions that began
    /// in the same second still come back in a stable order. That is ordering by the recorded wall
    /// clock, not a claim that the clock was monotonic. An id no document carries is an empty answer
    /// rather than an error: nothing was ever recorded for it.
    pub fn reading_sessions_for_document(
        &self,
        doc_id: i64,
    ) -> Result<Vec<StoredReadingSession>, MirkError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, doc_id, started_at, ended_at, active_ms FROM reading_sessions \
                 WHERE doc_id = ?1 ORDER BY started_at, id",
            )
            .map_err(storage_error)?;

        let rows = statement
            .query_map(params![doc_id], |row| {
                Ok(StoredReadingSession {
                    id: row.get(0)?,
                    session: ReadingSession {
                        doc_id: row.get(1)?,
                        started_at: row.get(2)?,
                        ended_at: row.get(3)?,
                        active_ms: row.get(4)?,
                    },
                })
            })
            .map_err(storage_error)?;

        let mut sessions = Vec::new();
        for row in rows {
            sessions.push(row.map_err(storage_error)?);
        }
        Ok(sessions)
    }

    /// Refuses a document id this store does not hold, as an invalid argument naming the field.
    ///
    /// The foreign key would refuse the row anyway; asking first is what turns the refusal into an
    /// invalid argument that names the field, instead of a storage failure that sends whoever reads
    /// the log looking at the disk. It also keeps the refusal free of side effects.
    ///
    /// **This check is not a guarantee against concurrent deletion.** It answers for the moment it
    /// runs. A document deleted before the insert — by a [`Store::forget_folder`] in another
    /// connection — is caught here as [`MirkError::InvalidInput`]; a deletion that lands between
    /// this check and the insert is caught by the foreign key as [`MirkError::Storage`]. Neither
    /// refusal writes a row, and this build does not claim one stable error type across that race.
    fn require_document(&self, doc_id: i64) -> Result<(), MirkError> {
        let known = self
            .connection
            .query_row(
                "SELECT 1 FROM documents WHERE id = ?1",
                params![doc_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(storage_error)?;

        if known.is_none() {
            return Err(invalid_input(
                "document",
                doc_id,
                "the id of a document this store holds",
            ));
        }
        Ok(())
    }

    /// Records a watched folder and returns its row id.
    ///
    /// Recording the same path again updates `recursive` in place rather than inserting a second
    /// row, so the scanner can record its folder on every run without duplicating it. `added_at`
    /// keeps the time the folder was first recorded, and `enabled` is left untouched: it is the
    /// user's setting, not the scanner's.
    ///
    /// Like [`Store::upsert_document`], the read-then-write below is not a transaction; the same
    /// accepted residual applies, because `mirk` opens one store per process today. A second writer
    /// that loses the race cannot duplicate the folder: `folders.path` is UNIQUE, so it fails with
    /// a [`MirkError::Storage`] constraint error instead.
    pub fn record_folder(&self, path: impl AsRef<Path>, recursive: bool) -> Result<i64, MirkError> {
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
    pub fn folder_removal(&self, id: i64) -> Result<Option<FolderRemoval>, MirkError> {
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
    pub fn forget_folder(&self, id: i64) -> Result<Option<FolderRemoval>, MirkError> {
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
    fn plan_folder_removal(&self, id: i64) -> Result<Option<FolderRemovalPlan>, MirkError> {
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
    /// ([`mirk_core::Document::display_authors`], R42/S4). The precedence is not decided here: this
    /// listing and [`Store::document_by_id`] both hand the same decoded [`Document`] to the one rule
    /// that lives in the core, so the card, the author filter and the free-text search cannot disagree
    /// about who wrote a document.
    pub fn library(&self) -> Result<Vec<LibraryEntry>, MirkError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT d.id, d.path, d.file_id_kind, d.volume_id, d.file_index, d.content_hash, \
                        d.pages, d.title, d.author, d.user_title, d.cover_page, d.cover_image, \
                        p.page, p.total, p.source, p.confidence, p.read_at, p.updated_at, \
                        d.cover_source \
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
                let cover_source: Option<String> = row.get(18)?;
                // The LEFT JOIN leaves every `progress` column NULL for a document with none, so
                // `p.page` is the presence check: it is NOT NULL whenever the row exists.
                let progress: Option<LatestProgressColumns> = match row.get::<_, Option<i64>>(12)? {
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
                    cover_source,
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
                cover_source,
            ) = row.map_err(storage_error)?;

            let (progress, progress_recorded_at) = match progress {
                Some((page, total, source, confidence, read_at, updated_at)) => {
                    let page = u32::try_from(page).map_err(|_| {
                        MirkError::Storage(format!("stored page {page} is not a page number"))
                    })?;
                    let total = total.map(u32::try_from).transpose().map_err(|_| {
                        MirkError::Storage("stored total is not a valid page count".into())
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
                    cover_source: decode_cover_source(cover_source),
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
    pub fn document_by_id(&self, id: i64) -> Result<Option<DocumentRecord>, MirkError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, path, file_id_kind, volume_id, file_index, content_hash, \
                        pages, title, author, user_title, cover_page, cover_image, cover_source \
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
                    row.get::<_, Option<String>>(12)?,
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
            cover_source,
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
        Ok(Some(DocumentRecord {
            id,
            document,
            cover_source: decode_cover_source(cover_source),
        }))
    }

    /// Every watched folder, oldest first.
    pub fn folders(&self) -> Result<Vec<WatchedFolder>, MirkError> {
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
    pub fn document_count(&self) -> Result<usize, MirkError> {
        let count: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
            .map_err(storage_error)?;
        usize::try_from(count)
            .map_err(|_| MirkError::Storage(format!("document count {count} is not a usize")))
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
    /// This writes no history time, because the detector that calls it has none: `/proc` says a file
    /// is open now, not when a person opened it. [`Store::record_suggestion_with_history`] is the
    /// same write for a source that does know.
    ///
    /// The lookup and the write are not one transaction, the same accepted residual as
    /// [`Store::record_folder`] and [`Store::upsert_document`]: `mirk` opens one store per process
    /// today. A racing second writer cannot duplicate the path, because `suggestions.path` is
    /// UNIQUE — it fails with a constraint error instead of forking the queue.
    pub fn record_suggestion(&self, path: impl AsRef<Path>) -> Result<i64, MirkError> {
        self.record_suggestion_with_history(path, None)
    }

    /// Records one observation of `path`, together with when the desktop's history says the reader
    /// opened it.
    ///
    /// The behaviour is exactly [`Store::record_suggestion`]'s — decided rows are never touched —
    /// with one addition: an undecided row keeps the **newest** history time any observation
    /// reported. It is a high-water mark rather than last-write-wins for the reason L46 recorded
    /// when it made the same choice for `progress.read_at`: a later pass that knew an older moment
    /// would otherwise walk the fact backwards and make a document opened today look older than the
    /// store already knew it was. `None` never erases a known time, and two unknowns stay unknown.
    ///
    /// `first_seen_at` is not involved: it is when the application noticed the file, and the
    /// history's time is a different fact about a different actor.
    pub fn record_suggestion_with_history(
        &self,
        path: impl AsRef<Path>,
        history_at: Option<i64>,
    ) -> Result<i64, MirkError> {
        let path = path_to_string(path.as_ref())?;

        let existing: Option<(i64, Option<String>, Option<i64>)> = self
            .connection
            .query_row(
                "SELECT id, decision, history_at FROM suggestions WHERE path = ?1 ORDER BY id LIMIT 1",
                params![path],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(storage_error)?;

        match existing {
            // A decided suggestion is history: it is returned as it stands, never touched.
            Some((id, Some(_), _)) => Ok(id),
            Some((id, None, known)) => {
                self.connection
                    .execute(
                        "UPDATE suggestions SET seen_count = seen_count + 1, history_at = ?1 \
                         WHERE id = ?2",
                        params![newest_history_at(known, history_at), id],
                    )
                    .map_err(storage_error)?;
                Ok(id)
            }
            None => {
                self.connection
                    .execute(
                        "INSERT INTO suggestions \
                         (path, seen_count, first_seen_at, decided_at, decision, history_at) \
                         VALUES (?1, 1, ?2, NULL, NULL, ?3)",
                        params![path, unix_now(), history_at],
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
    pub fn suggestions(&self) -> Result<Vec<Suggestion>, MirkError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, path, seen_count, first_seen_at, decision, decided_at, history_at \
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
    pub fn suggestion(&self, id: i64) -> Result<Option<Suggestion>, MirkError> {
        let columns: Option<SuggestionColumns> = self
            .connection
            .query_row(
                "SELECT id, path, seen_count, first_seen_at, decision, decided_at, history_at \
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
    /// [`MirkError::SuggestionAlreadyDecided`], naming which suggestion and what is on record, rather
    /// than being overwritten: the first answer is the reader's, and a second write would erase it.
    /// The `WHERE decision IS NULL` on the statement is what makes the guard atomic, so a second
    /// caller between the read and the write cannot slip a decision past it.
    pub fn decide_suggestion(
        &self,
        id: i64,
        decision: SuggestionDecision,
    ) -> Result<Suggestion, MirkError> {
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
                Some(existing) => Err(MirkError::SuggestionAlreadyDecided {
                    path: existing.path.to_string_lossy().into_owned(),
                    decision: existing
                        .decision
                        .map(SuggestionDecision::id)
                        .unwrap_or("decided"),
                }),
                None => Err(MirkError::Storage(format!(
                    "no suggestion with id {id} to decide"
                ))),
            };
        }

        self.suggestion(id)?.ok_or_else(|| {
            MirkError::Storage(format!(
                "suggestion {id} disappeared after its decision was recorded"
            ))
        })
    }

    fn find_id_by_identity(&self, identity: &FileId) -> Result<Option<i64>, MirkError> {
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
    fn find_id_by_path(&self, path: &str) -> Result<Option<(i64, Option<String>)>, MirkError> {
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
/// On Linux this is `$XDG_DATA_HOME/mirk/mirk.sqlite3` (or `~/.local/share/mirk/mirk.sqlite3` when
/// `XDG_DATA_HOME` is unset), on Windows `%APPDATA%\mirk\mirk.sqlite3`, and on macOS
/// `~/Library/Application Support/mirk/mirk.sqlite3`. It is never the repository and never the
/// current working directory.
pub fn database_path() -> Result<PathBuf, MirkError> {
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);

    let base = user_data_base_for(
        Platform::current(),
        xdg_data_home.as_deref(),
        home.as_deref(),
        appdata.as_deref(),
    )?;
    // The directory this build writes to, unless a `oct` directory from before the rename is the only
    // one that exists — in which case it is adopted rather than orphaned.
    let directory = adopt_legacy_data_dir(&base).unwrap_or_else(|| base.join(APP_DIR));
    Ok(database_file_in(&directory))
}

/// The database file to open inside an already-chosen data directory.
///
/// The modern name wins; the legacy one is used only when the modern file does not exist and the
/// legacy one does. That second chance is deliberate and independent of the rename: if renaming the
/// file had failed after the directory moved, `mirk.sqlite3` would not exist and opening it would
/// create an empty database beside a full one — the rename would have cost the reader their library
/// while claiming success.
pub fn database_file_in(directory: &Path) -> PathBuf {
    let modern = directory.join(DATABASE_FILE);
    if modern.exists() {
        return modern;
    }
    // The newest legacy file that exists; `loci.sqlite3` before `oct.sqlite3`.
    for name in LEGACY_DATABASE_FILES {
        let legacy = directory.join(name);
        if legacy.exists() {
            return legacy;
        }
    }
    modern
}

/// Takes over the data directory of the pre-rename build, and answers which directory to use.
///
/// The rule is **keep the library, never delete**:
///
/// - `mirk` exists: nothing to adopt, the answer is `None` and that directory is used.
/// - `mirk` is absent and `oct` exists: the whole directory is renamed, covers included (they live
///   beside the database), and the new path is returned.
/// - the rename fails: the **legacy** path is returned, so the application keeps working in place
///   instead of starting with an empty library. A cosmetic rename must never be able to lose a
///   reader's progress, and a failure here is the caller's to report, not the reader's to discover.
/// - neither exists: `None`, and a fresh `mirk` directory is created later by the store.
///
/// When both directories exist, the modern one wins **unless** it holds no database at all — the
/// leftover of a launch that died before SQLite opened — in which case the legacy database and covers
/// are moved into it. Nothing is ever deleted: a merge that cannot complete leaves the legacy
/// directory in place and the application keeps reading from it.
///
/// Residual, stated rather than hidden: if a cover exists under the same name in both directories,
/// the legacy one stays in `oct/` rather than overwriting the modern one. A cover is a rendering of a
/// page, not the reading record, so the merge prefers losing sight of one image to replacing a file.
pub fn adopt_legacy_data_dir(base: &Path) -> Option<PathBuf> {
    let current = base.join(APP_DIR);

    if !current.exists() {
        // A legacy directory holding a database beats an older one left empty by an interrupted
        // launch; only then does mere existence count. Newest name first, so `loci` wins over `oct`.
        let legacies = legacy_directories(base);
        let legacy = legacies
            .iter()
            .find(|path| holds_database(path))
            .or_else(|| legacies.iter().find(|path| path.exists()))?
            .clone();

        if std::fs::rename(&legacy, &current).is_err() {
            // The directory could not move (a lock, a permission, a full disk): keep using it in
            // place. The library is still readable, which is the only outcome that matters here.
            return Some(legacy);
        }
        rename_database_files(&current);
        return Some(current);
    }

    // The modern directory already exists. It may be the leftover of an interrupted first launch, in
    // which case it holds no database and the reader's library is still in a legacy directory.
    // Adopting means moving that database (and the covers) into it — never deleting either.
    if holds_database(&current) {
        return None;
    }
    let legacy = legacy_directories(base)
        .into_iter()
        .find(|path| holds_database(path))?;
    if !move_contents(&legacy, &current) {
        // The database could not be moved (a lock, a permission): keep reading it where it is
        // instead of pointing the application at a directory with no database in it.
        return Some(legacy);
    }
    rename_database_files(&current);
    Some(current)
}

/// Every directory name this build replaced, newest first.
fn legacy_directories(base: &Path) -> Vec<PathBuf> {
    LEGACY_APP_DIRS.iter().map(|name| base.join(name)).collect()
}

/// Whether `directory` holds a database under either name. "Exists" is not enough: a directory
/// created by a launch that died before SQLite opened contains nothing worth keeping.
fn holds_database(directory: &Path) -> bool {
    std::iter::once(&DATABASE_FILE)
        .chain(LEGACY_DATABASE_FILES.iter())
        .any(|name| directory.join(name).exists())
}

/// Renames the database trio inside `directory`, all or nothing.
///
/// A partial rename is worse than none: SQLite would open the legacy-named database without its
/// write-ahead log and could silently lose the last transaction, or the modern-named one without the
/// log that still holds the reader's most recent pages. So a failure undoes what was already renamed,
/// in reverse order, leaving the trio together under its original names — which `database_file_in`
/// still finds.
fn rename_database_files(directory: &Path) {
    if directory.join(DATABASE_FILE).exists() {
        // A modern database already exists: it is authoritative and no file may be touched. Renaming
        // a sidecar here would attach a legacy write-ahead log to a database it does not belong to,
        // which is corruption, not migration.
        return;
    }
    let Some(source) = LEGACY_DATABASE_FILES
        .iter()
        .find(|name| directory.join(name).exists())
    else {
        return;
    };

    // Plan the whole trio before moving anything: a sidecar whose destination already exists means
    // the state is mixed, and in a mixed state no rename is safe.
    let mut plan: Vec<(PathBuf, PathBuf)> = Vec::new();
    for suffix in std::iter::once("").chain(DATABASE_SIDECARS) {
        let from = directory.join(format!("{source}{suffix}"));
        if !from.exists() {
            continue;
        }
        let to = directory.join(format!("{DATABASE_FILE}{suffix}"));
        if to.exists() {
            return;
        }
        plan.push((from, to));
    }

    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (from, to) in plan {
        match std::fs::rename(&from, &to) {
            Ok(()) => done.push((from, to)),
            Err(_) => {
                for (from, to) in done.iter().rev() {
                    let _ = std::fs::rename(to, from);
                }
                return;
            }
        }
    }
}

/// Moves a legacy database and its covers into an existing directory, file by file, and only where
/// the destination does not already hold something. Nothing is deleted: the legacy directory is left
/// in place, so a half-finished merge is still recoverable by hand.
fn move_contents(legacy: &Path, current: &Path) -> bool {
    // The database trio is planned first and moved all or nothing, for the same reason the rename is:
    // a database that arrives without its write-ahead log, or a log that arrives without its
    // database, is worse than a merge that did not happen.
    let Some(source) = LEGACY_DATABASE_FILES
        .iter()
        .find(|name| legacy.join(name).exists())
    else {
        // Nothing to move but the covers; that is still worth doing.
        move_covers(legacy, current);
        return true;
    };

    let mut plan: Vec<(PathBuf, PathBuf)> = Vec::new();
    for suffix in std::iter::once("").chain(DATABASE_SIDECARS) {
        let from = legacy.join(format!("{source}{suffix}"));
        if !from.exists() {
            continue;
        }
        let to = current.join(format!("{source}{suffix}"));
        if to.exists() {
            return false;
        }
        plan.push((from, to));
    }

    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (from, to) in plan {
        match std::fs::rename(&from, &to) {
            Ok(()) => done.push((from, to)),
            Err(_) => {
                for (from, to) in done.iter().rev() {
                    let _ = std::fs::rename(to, from);
                }
                return false;
            }
        }
    }

    move_covers(legacy, current);
    true
}

/// Moves the covers best effort: they are decoration beside the reading record, and a collision
/// leaves the legacy image in place rather than replacing the modern one.
fn move_covers(legacy: &Path, current: &Path) {
    let covers = legacy.join(COVERS_DIR);
    if covers.is_dir() {
        let target = current.join(COVERS_DIR);
        let _ = std::fs::create_dir_all(&target);
        if let Ok(entries) = std::fs::read_dir(&covers) {
            for entry in entries.flatten() {
                let to = target.join(entry.file_name());
                if !to.exists() {
                    let _ = std::fs::rename(entry.path(), &to);
                }
            }
        }
    }
}

/// Resolves `mirk`'s user data directory for an explicit platform and environment.
///
/// This is the testable core of [`database_path`]: it takes the platform and the three
/// environment variables that are consulted, so the per-OS rules can be exercised without mutating
/// the process environment or touching a real home directory.
///
/// Returns [`MirkError::NotSupported`] for a platform `mirk` does not target, and
/// [`MirkError::Storage`] when the required environment variable is missing.
pub fn user_data_dir_for(
    platform: Platform,
    xdg_data_home: Option<&Path>,
    home: Option<&Path>,
    appdata: Option<&Path>,
) -> Result<PathBuf, MirkError> {
    Ok(user_data_base_for(platform, xdg_data_home, home, appdata)?.join(APP_DIR))
}

/// The platform's user data directory *before* the application's own directory is appended.
///
/// Split out of [`user_data_dir_for`] because [`database_path`] has to inspect the base to adopt a
/// legacy directory, and re-reading the three environment variables in two functions is how the two
/// would eventually disagree.
fn user_data_base_for(
    platform: Platform,
    xdg_data_home: Option<&Path>,
    home: Option<&Path>,
    appdata: Option<&Path>,
) -> Result<PathBuf, MirkError> {
    match platform {
        Platform::Linux => {
            let base = match xdg_data_home {
                Some(directory) if !directory.as_os_str().is_empty() => directory.to_path_buf(),
                _ => home
                    .ok_or_else(|| {
                        MirkError::Storage(
                            "cannot locate the user data directory: neither XDG_DATA_HOME nor HOME is set"
                                .to_string(),
                        )
                    })?
                    .join(".local/share"),
            };
            Ok(base)
        }
        Platform::Windows => {
            let appdata = appdata.ok_or_else(|| {
                MirkError::Storage(
                    "cannot locate the user data directory: APPDATA is not set".to_string(),
                )
            })?;
            Ok(appdata.to_path_buf())
        }
        Platform::MacOs => {
            let home = home.ok_or_else(|| {
                MirkError::Storage(
                    "cannot locate the user data directory: HOME is not set".to_string(),
                )
            })?;
            Ok(home.join("Library/Application Support"))
        }
        Platform::Other => Err(MirkError::NotSupported {
            capability: "user data directory",
            platform,
        }),
    }
}

/// Whether `table` has a column with this name, asked of SQLite itself rather than assumed.
fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool, MirkError> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(storage_error)?;
    let mut rows = statement.query([]).map_err(storage_error)?;
    while let Some(row) = rows.next().map_err(storage_error)? {
        let name: String = row.get(1).map_err(storage_error)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Reads the stored cover source. An absent or unknown value is [`None`], which asks
/// [`Document::cover_choice_from`] for the pre-V10 rule instead of inventing a source.
fn decode_cover_source(value: Option<String>) -> Option<CoverSource> {
    value.and_then(|text| CoverSource::parse(text.trim()))
}

/// How long an open that has migrations to apply waits for a competing writer.
///
/// Applying a migration is a write, so the open has to wait for whoever is already writing — in
/// practice another `mirk` process upgrading the same file after an update, or a scanner mid-save.
/// The wait is bounded on purpose: a writer that never finishes is a fault worth reporting, and this
/// reports it as an honest [`MirkError::Storage`] instead of a launch that never returns.
///
/// This is a **floor, not a replacement.** [`upgrade`] raises the connection's own busy timeout to at
/// least this value while it takes the lock and puts the original back afterwards, on every path. The
/// original is read from the connection ([`current_busy_timeout`]) and never assumed: `rusqlite`
/// installs a five-second timeout when it opens a connection, so restoring "the default" from a guess
/// is how a store that was migrated ends up contending differently from one that was only opened.
const MIGRATION_LOCK_WAIT: Duration = Duration::from_secs(10);

/// The busy timeout `connection` currently has, in the units SQLite reports it in.
///
/// The query form of `PRAGMA busy_timeout` reads the value without changing it, which is what makes it
/// usable here: the upgrade has to put back exactly what it found, not what it believes the driver's
/// default to be. The one prior state SQLite does not report is a handler installed directly with
/// `sqlite3_busy_handler`; nothing in this crate installs one, and a connection this crate opens never
/// has one.
fn current_busy_timeout(connection: &Connection) -> Result<Duration, MirkError> {
    let millis: i64 = connection
        .pragma_query_value(None, "busy_timeout", |row| row.get(0))
        .map_err(storage_error)?;
    let millis = u64::try_from(millis).map_err(|_| {
        MirkError::Storage(format!("database reports an invalid busy timeout {millis}"))
    })?;
    Ok(Duration::from_millis(millis))
}

/// Refuses a database written by a newer build instead of pretending to read it.
fn refuse_newer_version(version: u32) -> Result<(), MirkError> {
    if version > Store::CURRENT_SCHEMA_VERSION {
        return Err(MirkError::Storage(format!(
            "database schema version {version} is newer than this build supports \
             ({}); refusing to downgrade",
            Store::CURRENT_SCHEMA_VERSION
        )));
    }
    Ok(())
}

/// Brings `connection` to [`Store::CURRENT_SCHEMA_VERSION`], forward-only.
///
/// **Opening a current database takes no write reservation.** The version is read once and, when it
/// is already the version this build writes, the open is validated inside one consistent read
/// snapshot (a deferred transaction takes no write reservation) and returns: another process may be
/// writing the same file, and this open neither queues behind that writer nor stops it. It is still a
/// read, so a lock that excludes readers — an exclusive lock, a writer in the middle of a commit —
/// blocks it until this connection's own busy timeout runs out, exactly as it would block any other
/// read. The same read is what refuses a database written by a newer build, so that refusal takes no
/// write either.
///
/// **A current database is checked, not assumed.** Schema 12 was revised in place before it shipped,
/// so its version stamp alone cannot say which shape wrote a database. [`verify_reading_tables`] runs
/// inside the read snapshot above, before any reading fact can be read or written.
///
/// **Upgrading is one serialized write** ([`upgrade`]): the connection's own busy timeout is raised
/// while the write lock is taken and put back afterwards, the version is re-read under the lock, and
/// every missing step is applied in the same transaction.
fn migrate(connection: &mut Connection, origin: Option<&Path>) -> Result<(), MirkError> {
    let version = read_schema_version(connection)?;
    // Test builds only: the counter the migration tests watch to force their interleaving. It is
    // compiled out of every build that ships, so no caller can observe it.
    #[cfg(test)]
    observations::note_version_read(connection.path());
    refuse_newer_version(version)?;
    if version == Store::CURRENT_SCHEMA_VERSION {
        let snapshot = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(storage_error)?;
        verify_reading_tables(&snapshot, origin)?;
        snapshot.commit().map_err(storage_error)?;
        return Ok(());
    }
    upgrade(connection, origin)
}

/// Applies every missing migration step, serialized against competing writers.
///
/// The version check and the steps are **one unit and one transaction**. Two opens of the same
/// database at the same time cannot both apply a step: the second waits for the first and then finds
/// the work done. A step that fails undoes the steps this open already applied, instead of leaving a
/// database half-upgraded for the next launch to trip over.
///
/// **The re-read is the other half of that promise.** The version that decides what to apply is read
/// again after the write lock is taken, never the one read before it: while this open waited,
/// another process may have finished the whole upgrade, and replaying steps from the stale version
/// would apply an `ALTER TABLE` twice and fail — or, worse, succeed and write the same facts twice.
///
/// **What this changes about rollback.** Each step used to be committed as it was applied. Now they
/// are committed together at the end, so a failure in the last step leaves the database at the
/// version it was opened with and **no** step from this open applied, where the old shape kept the
/// earlier ones. That trade is deliberate: "some steps committed" is the state this rewrite exists
/// to make impossible, the recorded version still says where to resume, and the retry on the next
/// open redoes the same work from the same starting point. The per-version tests in this crate still
/// pass because each of them opens a database that needs exactly one step; the whole-upgrade test is
/// the one that pins the new unit.
///
/// **What it does not change is how the store contends for the database.** The longer wait belongs to
/// this function and to nothing else: the connection's own busy timeout is read before it is raised and
/// restored on every path, so a store that was upgraded locks exactly like one that was already current.
fn upgrade(connection: &mut Connection, origin: Option<&Path>) -> Result<(), MirkError> {
    // Whatever this connection was opened with is what it must still have afterwards, on every path.
    let previous = current_busy_timeout(connection)?;
    connection
        .busy_timeout(previous.max(MIGRATION_LOCK_WAIT))
        .map_err(storage_error)?;
    let outcome = upgrade_locked(connection, origin);
    // Back to the value this connection already had, so a store that was upgraded contends exactly
    // like one that was not when restoration succeeds. If both operations fail, report the upgrade
    // error; the restoration error is not returned and the original timeout may not have been restored.
    let restored = connection.busy_timeout(previous).map_err(storage_error);
    outcome.and(restored)
}

/// The upgrade itself, in one immediate transaction: the collision check and the version check under
/// the write lock, every missing step, the final definition check, and the stamp.
fn upgrade_locked(connection: &mut Connection, origin: Option<&Path>) -> Result<(), MirkError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage_error)?;

    // Read again now that the write lock is ours. The value read before the lock describes a moment
    // that has passed; this one describes the database this transaction is about to change.
    let version = read_schema_version(&transaction)?;
    refuse_newer_version(version)?;
    if version == Store::CURRENT_SCHEMA_VERSION {
        // A competing writer finished the upgrade while this open waited for the lock. Its output is
        // still checked, exactly as a database opened directly at this version would be: a writer
        // that stamped the version without writing this build's tables must not be trusted. Dropping
        // the transaction rolls back a transaction that wrote nothing.
        verify_reading_tables(&transaction, origin)?;
        return Ok(());
    }

    // A table or index the last step would create must not already exist. The check runs before any
    // step, inside the same transaction, so a collision cannot leave the database partially upgraded.
    preflight_v12_collisions(&transaction, origin)?;

    // One step per version, applied in order to a database known to be at `version`. Every statement
    // runs on `transaction`, so a failure anywhere undoes the whole upgrade and the recorded version
    // stays where it was.
    if version < 1 {
        transaction
            .execute_batch(SCHEMA_V1)
            .map_err(storage_error)?;
    }

    if version < 2 {
        transaction
            .execute_batch(SCHEMA_V2)
            .map_err(storage_error)?;
    }

    if version < 3 {
        transaction
            .execute_batch(SCHEMA_V3)
            .map_err(storage_error)?;
    }

    if version < 4 {
        transaction
            .execute_batch(SCHEMA_V4)
            .map_err(storage_error)?;
    }

    if version < 5 {
        transaction
            .execute_batch(SCHEMA_V5)
            .map_err(storage_error)?;
    }

    if version < 6 {
        transaction
            .execute_batch(SCHEMA_V6)
            .map_err(storage_error)?;
    }

    if version < 7 {
        transaction
            .execute_batch(SCHEMA_V7)
            .map_err(storage_error)?;
    }

    if version < 8 {
        transaction
            .execute_batch(SCHEMA_V8)
            .map_err(storage_error)?;
    }

    if version < 9 {
        // The three parts of this step stay ONE unit: the two tables, the backfill that moves the
        // reader's existing author into them, and the retirement of the column it came from. A
        // failure anywhere — including in the `DROP COLUMN` at the end, after the tables exist and
        // the rows are written — leaves the database at version 8 with every row where it was, so
        // the next open retries the whole step instead of finding half a schema (the V9 atomicity
        // test pins it).
        transaction
            .execute_batch(SCHEMA_V9)
            .map_err(storage_error)?;
        backfill_reader_authors(&transaction)?;
        transaction
            .execute_batch(SCHEMA_V9_RETIRE_USER_AUTHOR)
            .map_err(storage_error)?;
    }

    if version < 10 {
        // `ADD COLUMN` has no `IF NOT EXISTS` in SQLite, and a run that already added it — an
        // interrupted upgrade, or a database built from the current shape and stamped older — must
        // not fail the whole open. The column is asked for, then added only if it is missing.
        if !column_exists(&transaction, "documents", "cover_source")? {
            transaction
                .execute_batch(SCHEMA_V10_ADD)
                .map_err(storage_error)?;
        }
        transaction
            .execute_batch(SCHEMA_V10_BACKFILL)
            .map_err(storage_error)?;
    }

    if version < 11 {
        // `ADD COLUMN` has no `IF NOT EXISTS` in SQLite, and a run that already added it — an
        // interrupted upgrade, or a database built from the current shape and stamped older — must
        // not fail the whole open.
        if !column_exists(&transaction, "suggestions", "history_at")? {
            transaction
                .execute_batch(SCHEMA_V11)
                .map_err(storage_error)?;
        }
    }

    if version < 12 {
        // The step only creates the two tables and their indexes; it rewrites no existing row, so a
        // library that upgrades keeps every document, position and suggestion exactly as it was. A
        // failure here takes the whole upgrade back with the caller's transaction.
        for statement in SCHEMA_V12 {
            transaction
                .execute_batch(statement)
                .map_err(storage_error)?;
        }
    }

    // The step's own output is checked before the version stamp claims it: a database this build
    // would refuse to reopen must not be stamped as if it were current.
    verify_reading_tables(&transaction, origin)?;

    // One stamp for the whole upgrade, written in the same transaction as the work it describes.
    transaction
        .pragma_update(
            None,
            "user_version",
            i64::from(Store::CURRENT_SCHEMA_VERSION),
        )
        .map_err(storage_error)?;
    transaction.commit().map_err(storage_error)?;
    Ok(())
}

/// Refuses a database older than the current version that already holds an object the V12 step
/// would create, before this connection runs any migration step.
///
/// The V12 step uses plain `CREATE TABLE`, so a collision there would fail only after the earlier
/// steps had already run, leaving the database partially upgraded at an intermediate version. Asking
/// first keeps the database exactly where it was: the recorded version, every table and every row are
/// untouched, and the next open can retry once the obstacle is gone. A genuine older database holds
/// none of these names — they did not exist before V12 — so this never refuses an upgrade that has
/// any chance of succeeding.
///
/// Names are looked up across every object type and **case-insensitively**, as SQLite itself resolves
/// them: a table called `READING_SESSIONS` is the name `CREATE TABLE reading_sessions` collides with,
/// and a view or trigger that happens to take one of the names would stop the step just as surely.
/// The refusal names the object with the spelling the database stores.
///
/// **This check is atomic with the upgrade.** The version is re-read and this check, every migration
/// step and the final definition check run inside one `BEGIN IMMEDIATE` transaction, so a competing
/// writer cannot commit a colliding object between the check and the V12 step: writers are serialized
/// for the whole upgrade. Readers are unaffected — they see the database as it was before the
/// transaction — and a writer that arrives while the upgrade is in progress waits for SQLite's busy
/// timeout and then fails with a storage error instead of interleaving. A current database never
/// reaches this path: it is validated in a read snapshot that takes no write reservation.
fn preflight_v12_collisions(
    connection: &Connection,
    origin: Option<&Path>,
) -> Result<(), MirkError> {
    let version = read_schema_version(connection)?;
    if version >= Store::CURRENT_SCHEMA_VERSION {
        return Ok(());
    }

    for name in v12_object_names() {
        let existing: Option<(String, String)> = connection
            .query_row(
                "SELECT type, name FROM sqlite_master WHERE name = ?1 COLLATE NOCASE LIMIT 1",
                params![name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some((kind, stored_name)) = existing {
            // The refusal names the object as the database stores it, not as this build would write
            // it: `READING_SESSIONS` and `reading_sessions` are the same name to SQLite, and a reader
            // told about the wrong spelling would look for an object that is not there.
            return Err(incompatible_older_schema(
                origin,
                version,
                &kind,
                &stored_name,
            ));
        }
    }
    Ok(())
}

/// Every schema object the V12 step creates, by name.
///
/// Derived from the same constants the step executes, so a name cannot be added to one list and
/// forgotten in the other.
fn v12_object_names() -> Vec<&'static str> {
    READING_TABLES
        .iter()
        .map(|(name, _, _)| *name)
        .chain(READING_INDEXES.iter().map(|(name, _)| *name))
        .collect()
}

/// The refusal a pre-existing V12 object earns on an older database.
fn incompatible_older_schema(
    origin: Option<&Path>,
    version: u32,
    kind: &str,
    name: &str,
) -> MirkError {
    let database = match origin {
        Some(path) => format!("the database at `{}`", path.display()),
        None => "this database".to_string(),
    };
    MirkError::Storage(format!(
        "{database} reports schema version {version} but already holds {kind} `{name}`, which the \
         upgrade to schema version {current} would create. The upgrade was refused before any step \
         ran, so the database's committed contents are exactly as they were — no table, no row and \
         not the version stamp changed — and this build will not repair, rewrite, move or delete it. \
         Open it with the build that wrote it.",
        current = Store::CURRENT_SCHEMA_VERSION
    ))
}

/// The reading tables V12 owns: the statement this build writes for each, and the columns that
/// statement declares, in declaration order.
///
/// The column list is what lets a refusal name the fact an earlier shape is missing. A drift between
/// it and the statement cannot go unnoticed: it would make this build refuse the tables it just
/// created, which every fresh-database test would fail on immediately.
const READING_TABLES: [(&str, &str, &[&str]); 2] = [
    (
        "reading_sessions",
        SCHEMA_V12_READING_SESSIONS,
        &["id", "doc_id", "started_at", "ended_at", "active_ms"],
    ),
    (
        "page_visits",
        SCHEMA_V12_PAGE_VISITS,
        &[
            "id",
            "doc_id",
            "session_id",
            "page",
            "observed_at",
            "visible_ms",
        ],
    ),
];

/// Refuses a database that claims schema 12 but holds reading tables this build did not write.
///
/// Schema 12 was revised in place before it shipped, so a development database can carry an earlier
/// pre-release shape under the same stamp: `page_visits` without `session_id`/`observed_at`, or the
/// same columns without the storage-class checks, the timestamp bounds, the `u32` page bound and the
/// same-document key. `PRAGMA user_version` cannot tell those apart from this build's shape, so the
/// tables' own stored definitions are compared with the statements the migration executes.
///
/// A mismatch fails the open **before any reading fact is read or written**, and changes nothing in
/// the database's committed contents: no table, no row, and not the version stamp. There is
/// deliberately no repair. Rebuilding the tables would have to invent the session and time facts of
/// every existing visit, and a fabricated provenance is exactly the lie this crate exists to prevent.
///
/// The comparison is **exact**: a database is accepted only if its reading tables and indexes carry
/// the bytes this build's statements write. SQLite stores the statement text verbatim, so the
/// migration's own output matches by construction; the only thing dropped is the statement terminator,
/// which SQLite does not store. A semantically equivalent definition that is formatted differently —
/// keyword case, spacing, comments, a quoted identifier, a reordered constraint — is **refused**. That
/// is deliberate: SQLite's tokenizer is not Rust's (U+00A0 is whitespace to Rust but an identifier
/// character to SQLite), so a comparison that ignored formatting could accept a definition it cannot
/// prove equivalent. The cost is that a harmless reformat is refused; the benefit is that only the
/// bytes this build wrote are trusted.
///
/// The three indexes the step creates are part of that shape: each must be present and identical, and
/// any other index or trigger attached to a reading table is refused. A unique index is a correctness
/// constraint — it can reject a fact this build records — and a trigger can rewrite or delete one, so
/// neither is treated as a harmless addition.
///
/// What this does **not** verify is the rest of the database: triggers on other tables that might
/// reach these rows, views, and a file whose schema was edited through `PRAGMA writable_schema`. That
/// is the explicit trust boundary — the gate checks the reading facts' own objects, not the whole file
/// — and it is the same trust this crate already places in the V1–V11 schema. The check runs inside
/// the transaction the open already uses — the read snapshot for a current database, the immediate
/// upgrade transaction when one is needed — so it cannot race a competing writer during the open;
/// once the open has finished, a writer could still change the schema, and this build does not claim
/// to police the file afterwards.
fn verify_reading_tables(connection: &Connection, origin: Option<&Path>) -> Result<(), MirkError> {
    // Every difference is collected before refusing, so the error names the whole gap rather than
    // whichever table happened to be checked first.
    let mut differences: Vec<String> = Vec::new();
    for (table, expected_statement, expected_columns) in READING_TABLES {
        let stored: Option<String> = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1 COLLATE NOCASE",
                params![table],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;

        let Some(stored) = stored else {
            differences.push(format!("the table `{table}` is missing"));
            continue;
        };
        if stored_definition_matches(&stored, expected_statement) {
            continue;
        }

        let actual_columns = table_columns(connection, table)?;
        let missing: Vec<&str> = expected_columns
            .iter()
            .copied()
            .filter(|column| !actual_columns.iter().any(|actual| actual == column))
            .collect();
        if missing.is_empty() {
            differences.push(format!(
                "the definition of `{table}` differs from the one this build writes"
            ));
        } else {
            differences.push(format!(
                "the table `{table}` is missing the column(s) {}",
                quoted_list(&missing)
            ));
        }
    }

    for (index, expected_statement) in READING_INDEXES {
        let stored: Option<String> = connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?1 COLLATE NOCASE",
                params![index],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        match stored {
            None => differences.push(format!("the index `{index}` is missing")),
            Some(stored) if !stored_definition_matches(&stored, expected_statement) => {
                differences.push(format!(
                    "the index `{index}` is not the one this build writes"
                ));
            }
            Some(_) => {}
        }
    }

    // Any other index or trigger attached to a reading table is an object this build did not create
    // and cannot vouch for. An autoindex belongs to the table's own definition, which was compared
    // above, so it is not a separate object here.
    let mut statement = connection
        .prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_master \
             WHERE type IN ('index', 'trigger') \
             ORDER BY type, name",
        )
        .map_err(storage_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(storage_error)?;
    for row in rows {
        let (kind, name, table, sql) = row.map_err(storage_error)?;
        // Only SQLite's own autoindexes are not separate objects: they carry no SQL and the exact
        // reserved prefix. A `LIKE` pattern would be wrong here — `_` is a wildcard, so it would also
        // exempt an object named `sqliteXautoindexY...`, which this build did not create.
        let is_autoindex = kind == "index"
            && sql.is_none()
            && name.to_ascii_lowercase().starts_with("sqlite_autoindex_");
        if is_autoindex {
            continue;
        }
        if !READING_TABLES
            .iter()
            .any(|(expected, _, _)| expected.eq_ignore_ascii_case(&table))
        {
            continue;
        }
        // Object names are case-insensitive in SQLite, so an index written with different case is the
        // same index.
        let expected_index = kind == "index"
            && READING_INDEXES
                .iter()
                .any(|(expected, _)| expected.eq_ignore_ascii_case(&name));
        if !expected_index {
            differences.push(format!(
                "the unexpected {kind} `{name}` is attached to a reading table"
            ));
        }
    }

    if differences.is_empty() {
        return Ok(());
    }
    Err(incompatible_reading_schema(origin, differences.join("; ")))
}

/// The refusal an earlier schema 12 earns, worded so it can be acted on without guessing.
fn incompatible_reading_schema(origin: Option<&Path>, detail: String) -> MirkError {
    let database = match origin {
        Some(path) => format!("the database at `{}`", path.display()),
        None => "this database".to_string(),
    };
    MirkError::Storage(format!(
        "{database} reports schema version {version} but its reading facts are not the shape this \
         build wrote: {detail}. Schema version {version} was revised before it shipped, so this is \
         most likely a development database written by an earlier pre-release build. The database's \
         committed contents are exactly as they were — no table, no row, not the version stamp \
         changed — and this build will not repair, rewrite, move or delete it, because the missing \
         session and time facts cannot be recovered by guessing. Open it with the build that wrote it.",
        version = Store::CURRENT_SCHEMA_VERSION
    ))
}

/// Whether a database's stored definition is exactly the one this build writes.
///
/// SQLite stores the text of the `CREATE` statement it executed, so a table this build created
/// carries the bytes of the constant the migration ran — spacing, comments, keyword case, quoted text
/// and all. The guard's tests pin that all five definitions come back byte-identical; the only
/// transformation applied here is dropping the statement terminator, which SQLite does not store and
/// this build's statements do not carry.
///
/// Everything else must match exactly. A definition that is semantically equivalent but formatted
/// differently is **refused**: SQLite's tokenizer is not Rust's — U+00A0 is whitespace to Rust but an
/// identifier character to SQLite, so `INTEGER\u{A0}NOT NULL` is a different column — and any
/// normalization rule would be a guess this build cannot prove safe. Refusing is the conservative
/// direction; the cost is that a harmless reformat is refused too, and only the bytes this build wrote
/// are trusted.
fn stored_definition_matches(stored: &str, canonical: &str) -> bool {
    stored == canonical.strip_suffix(';').unwrap_or(canonical)
}

/// The column names of one table, in declaration order.
fn table_columns(connection: &Connection, table: &str) -> Result<Vec<String>, MirkError> {
    let mut statement = connection
        .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
        .map_err(storage_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(storage_error)?;

    let mut columns = Vec::new();
    for row in rows {
        columns.push(row.map_err(storage_error)?);
    }
    Ok(columns)
}

/// A backticked, comma-separated list, for naming several columns in one sentence.
fn quoted_list(values: &[&str]) -> String {
    values
        .iter()
        .map(|value| format!("`{value}`"))
        .collect::<Vec<_>>()
        .join(", ")
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
fn backfill_reader_authors(transaction: &rusqlite::Transaction<'_>) -> Result<(), MirkError> {
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
fn upsert_author(connection: &Connection, name: &str) -> Result<i64, MirkError> {
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

fn read_schema_version(connection: &Connection) -> Result<u32, MirkError> {
    let raw: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage_error)?;
    u32::try_from(raw).map_err(|_| {
        MirkError::Storage(format!("database reports an invalid schema version {raw}"))
    })
}

/// Splits a [`FileId`] into its storage columns: `(kind, volume_id, file_index, content_hash)`.
fn encode_identity(
    identity: &FileId,
) -> (&'static str, Option<String>, Option<String>, Option<String>) {
    match identity {
        FileId::Posix { dev, ino } => ("posix", Some(dev.to_string()), Some(ino.to_string()), None),
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
) -> Result<Option<FileId>, MirkError> {
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
                MirkError::Storage("stored content-hash identity has no hash".to_string())
            })?;
            Ok(Some(FileId::ContentHash(hash.to_string())))
        }
        Some(other) => Err(MirkError::Storage(format!(
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
) -> Result<Document, MirkError> {
    let identity = decode_identity(
        kind.as_deref(),
        volume.as_deref(),
        index.as_deref(),
        hash.as_deref(),
    )?;
    let pages = pages
        .map(u32::try_from)
        .transpose()
        .map_err(|_| MirkError::Storage("stored page count is not a valid page number".into()))?;
    let cover_page = cover_page
        .map(u32::try_from)
        .transpose()
        .map_err(|_| MirkError::Storage("stored cover page is not a valid page number".into()))?;

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

fn parse_u64(value: Option<&str>, column: &str) -> Result<u64, MirkError> {
    let value = value
        .ok_or_else(|| MirkError::Storage(format!("identity column `{column}` is missing")))?;
    value.parse::<u64>().map_err(|_| {
        MirkError::Storage(format!(
            "identity column `{column}` is not a valid u64: `{value}`"
        ))
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

fn parse_confidence(value: &str) -> Result<Confidence, MirkError> {
    match value {
        "exact" => Ok(Confidence::Exact),
        "close-time" => Ok(Confidence::CloseTime),
        "manual" => Ok(Confidence::Manual),
        "unknown" => Ok(Confidence::Unknown),
        other => Err(MirkError::Storage(format!(
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
type SuggestionColumns = (
    i64,
    String,
    i64,
    i64,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

/// Reads one `suggestions` row into its raw columns.
fn suggestion_columns(row: &rusqlite::Row<'_>) -> rusqlite::Result<SuggestionColumns> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

/// The newer of two history times, where `None` carries no information rather than zero.
///
/// A known time is never lowered and never erased by an observation that has none: the history's
/// moment is a fact a source recorded, and a later pass that cannot see it is not evidence that it
/// did not happen.
fn newest_history_at(known: Option<i64>, observed: Option<i64>) -> Option<i64> {
    match (known, observed) {
        (Some(known), Some(observed)) => Some(known.max(observed)),
        (Some(known), None) => Some(known),
        (None, observed) => observed,
    }
}

/// Interprets a raw row, refusing a decision id this build does not know.
///
/// An unrecognised id is an error rather than an undecided row, exactly as [`parse_confidence`]
/// treats an unknown confidence: a value a newer build wrote must not be silently presented as
/// "still awaiting a decision", because the reader could then decide it again and overwrite the
/// answer already on record.
fn suggestion_from_columns(columns: SuggestionColumns) -> Result<Suggestion, MirkError> {
    let (id, path, seen_count, first_seen_at, decision, decided_at, history_at) = columns;
    let seen_count = u32::try_from(seen_count).map_err(|_| {
        MirkError::Storage(format!(
            "suggestion {id} has an invalid seen count {seen_count}"
        ))
    })?;
    let decision = match decision.as_deref() {
        None => None,
        Some("accepted") => Some(SuggestionDecision::Accepted),
        Some("discarded") => Some(SuggestionDecision::Discarded),
        Some(other) => {
            return Err(MirkError::Storage(format!(
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
        history_at,
    })
}

fn path_to_string(path: &Path) -> Result<String, MirkError> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        MirkError::InvalidPath(format!("path is not valid UTF-8: {}", path.display()))
    })
}

fn unix_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

fn storage_error(error: rusqlite::Error) -> MirkError {
    MirkError::Storage(error.to_string())
}

/// A caller-supplied value this build cannot act on, named as the interface names it.
///
/// One spelling for the refusals in this crate, so the field, the value that arrived and what is
/// accepted cannot drift apart between callers — and so an invalid argument is never reported as a
/// storage failure ([`MirkError::InvalidInput`] explains why that distinction matters).
fn invalid_input(
    field: &'static str,
    value: impl std::fmt::Display,
    expected: &'static str,
) -> MirkError {
    MirkError::InvalidInput {
        field,
        value: value.to_string(),
        expected: expected.to_string(),
    }
}

/// The one seam the migration-race tests have, compiled into test builds only.
///
/// The race those tests need cannot be forced from outside the process: a competing connection has to
/// commit its version change in the window between an open's first version read and its write lock,
/// and nothing outside [`migrate`] can see that window. So the window is *counted* here and the tests
/// commit only once the count says an open has passed through it. A counter and not a pause, because a
/// pause would have to be handed out to a caller to release, and the point of the tests is the
/// behaviour around the real lock rather than around a controllable one.
#[cfg(test)]
mod observations {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn reads() -> &'static Mutex<HashMap<String, usize>> {
        static READS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
        READS.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Counts one pre-lock version read of the database at `path`.
    ///
    /// `None` is an in-memory database, which no second connection can reach, so there is nothing to
    /// race with and nothing to count.
    pub(super) fn note_version_read(path: Option<&str>) {
        let Some(path) = path else {
            return;
        };
        let mut reads = reads().lock().expect("the read counter is never poisoned");
        *reads.entry(path.to_string()).or_default() += 1;
    }

    /// How many pre-lock version reads `path` has recorded so far.
    pub(super) fn version_reads(path: &std::path::Path) -> usize {
        let path = path
            .to_str()
            .expect("the temporary database paths are UTF-8");
        let reads = reads().lock().expect("the read counter is never poisoned");
        reads.get(path).copied().unwrap_or_default()
    }

    /// Forgets the reads recorded for `path`, so a test can count from its own fixture onwards.
    pub(super) fn reset(path: &std::path::Path) {
        let path = path
            .to_str()
            .expect("the temporary database paths are UTF-8");
        let mut reads = reads().lock().expect("the read counter is never poisoned");
        reads.remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a database exactly as version 1 left it: the V1 schema, a recorded folder, and the
    /// version stamp. Built from `SCHEMA_V1` itself rather than a copy, so the fixture cannot drift
    /// away from the schema it claims to reproduce.
    fn a_version_1_database() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("mirk.sqlite3");
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
        let path = dir.path().join("mirk.sqlite3");
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
        let path = dir.path().join("mirk.sqlite3");
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
        let path = dir.path().join("mirk.sqlite3");
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

    /// Builds a database exactly as version 10 left it: V1..V10 applied, one suggestion awaiting the
    /// reader's decision, and the version stamp. The V11 migration must add the history column
    /// without touching the suggestion it exists for.
    fn a_version_10_database() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("mirk.sqlite3");
        let connection = Connection::open(&path).expect("open raw");
        for schema in [
            SCHEMA_V1,
            SCHEMA_V2,
            SCHEMA_V3,
            SCHEMA_V4,
            SCHEMA_V5,
            SCHEMA_V6,
            SCHEMA_V7,
            SCHEMA_V8,
            SCHEMA_V9,
            SCHEMA_V9_RETIRE_USER_AUTHOR,
            SCHEMA_V10_ADD,
        ] {
            connection
                .execute_batch(schema)
                .expect("apply a schema a version 10 database has");
        }
        // One suggestion waiting for its decision, recorded before the history column existed.
        connection
            .execute(
                "INSERT INTO suggestions (path, seen_count, first_seen_at, decided_at, decision) \
                 VALUES (?1, 3, 1700000000, NULL, NULL)",
                params!["/elsewhere/suggested.pdf"],
            )
            .expect("record a suggestion the way version 10 did");
        connection
            .pragma_update(None, "user_version", 10i64)
            .expect("stamp version 10");
        drop(connection);
        (dir, path)
    }

    #[test]
    fn a_version_10_database_gains_the_history_time_without_losing_a_suggestion() {
        // The column the migration exists for: the reader's own moment from the desktop's history,
        // which must never be confused with `first_seen_at` — the moment the application noticed the
        // file. L46 already paid for putting those two in one column.
        let (_dir, path) = a_version_10_database();

        let store = Store::open(&path).expect("opening migrates the database");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );

        // What the older build recorded survives whole, and it reads back with no history time:
        // the migration adds a fact, it does not invent one.
        let suggestion = store.suggestion(1).expect("read").expect("still present");
        assert_eq!(suggestion.path, PathBuf::from("/elsewhere/suggested.pdf"));
        assert_eq!(suggestion.seen_count, 3);
        assert_eq!(suggestion.first_seen_at, 1_700_000_000);
        assert_eq!(suggestion.decision, None);
        assert_eq!(suggestion.history_at, None);

        // And the column is live afterwards, as its own fact: recording the history's time leaves
        // the application's own sighting time exactly where it was.
        store
            .record_suggestion_with_history("/elsewhere/suggested.pdf", Some(1_600_000_000))
            .expect("record the history's time");
        let suggestion = store.suggestion(1).expect("read").expect("present");
        assert_eq!(suggestion.history_at, Some(1_600_000_000));
        assert_eq!(suggestion.first_seen_at, 1_700_000_000);
        assert_eq!(suggestion.seen_count, 4);
    }

    #[test]
    fn the_history_column_migration_is_idempotent() {
        // Reopening a migrated database must not run the step again, and must not rewrite anything.
        let (_dir, path) = a_version_10_database();
        drop(Store::open(&path).expect("migrate"));

        let store = Store::open(&path).expect("reopen a migrated database");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );
        let suggestion = store.suggestion(1).expect("read").expect("present");
        assert_eq!(suggestion.seen_count, 3);
        assert_eq!(suggestion.history_at, None);
        drop(store);

        // A database that already carries the column and is still stamped 10 is what an interrupted
        // upgrade leaves behind: `ADD COLUMN` has no `IF NOT EXISTS`, so the step has to ask first
        // instead of failing the whole open.
        let (_dir, path) = a_version_10_database();
        let connection = Connection::open(&path).expect("open raw");
        connection
            .execute_batch(SCHEMA_V11)
            .expect("add the column without stamping the version");
        drop(connection);

        let store = Store::open(&path).expect("a half-applied step must not fail the open");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );
        let suggestion = store.suggestion(1).expect("read").expect("present");
        assert_eq!(suggestion.path, PathBuf::from("/elsewhere/suggested.pdf"));
        assert_eq!(suggestion.seen_count, 3, "the rows are intact");
        assert_eq!(suggestion.history_at, None);
    }

    #[test]
    fn a_suggestion_keeps_the_histories_time_apart_and_never_moves_it_backwards() {
        let store = Store::open_in_memory().expect("open in memory");

        // The `/proc` detector says a file is open now and has no moment to offer, so the row says
        // honestly that no source said when it was opened.
        let untyped = store
            .record_suggestion("/elsewhere/open.pdf")
            .expect("record");
        let suggestion = store.suggestion(untyped).expect("read").expect("present");
        assert_eq!(suggestion.history_at, None);

        // A source that does know writes the reader's moment, in its own column.
        let history = store
            .record_suggestion_with_history("/elsewhere/opened.pdf", Some(1_600_000_000))
            .expect("record");
        let suggestion = store.suggestion(history).expect("read").expect("present");
        assert_eq!(suggestion.history_at, Some(1_600_000_000));
        assert_ne!(
            suggestion.first_seen_at, 1_600_000_000,
            "when mirk looked is not when the reader opened it"
        );

        // A later pass that knows an older moment cannot walk the fact backwards, and one that knows
        // nothing cannot erase it — the same high-water mark `progress.read_at` uses (L46).
        store
            .record_suggestion_with_history("/elsewhere/opened.pdf", Some(1_500_000_000))
            .expect("record an older moment");
        store
            .record_suggestion("/elsewhere/opened.pdf")
            .expect("record an observation with no moment");
        let suggestion = store.suggestion(history).expect("read").expect("present");
        assert_eq!(suggestion.history_at, Some(1_600_000_000));
        assert_eq!(suggestion.seen_count, 3);

        // A decided suggestion is history: nothing about it is touched, its time included.
        store
            .decide_suggestion(history, SuggestionDecision::Discarded)
            .expect("discard");
        store
            .record_suggestion_with_history("/elsewhere/opened.pdf", Some(1_700_000_000))
            .expect("record a decided path again");
        let suggestion = store.suggestion(history).expect("read").expect("present");
        assert_eq!(suggestion.decision, Some(SuggestionDecision::Discarded));
        assert_eq!(suggestion.history_at, Some(1_600_000_000));
        assert_eq!(suggestion.seen_count, 3);
    }

    #[test]
    fn a_version_4_database_gains_the_readers_title_without_touching_the_documents() {
        let (_dir, path) = a_version_4_database();

        let store = Store::open(&path).expect("opening migrates the database");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );

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
        assert_eq!(
            library[0].document.document.display_title(),
            "The reader's title"
        );
    }

    #[test]
    fn a_version_3_database_gains_read_at_and_keeps_its_rows() {
        let (_dir, path) = a_version_3_database();

        let store = Store::open(&path).expect("opening migrates the database");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );

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
        assert_eq!(
            rows[1].read_at,
            Some(1234),
            "the new time survives the migration"
        );
    }

    #[test]
    fn a_version_2_database_gains_settings_and_keeps_its_rows() {
        let (_dir, path) = a_version_2_database();

        let store = Store::open(&path).expect("opening migrates the database");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );

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
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );

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
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );
        drop(store);

        let after = std::fs::read(&path).expect("read the database bytes");
        assert_eq!(
            after, before,
            "a database already current must not be rewritten"
        );
    }

    // --- The upgrade as one serialized write ---------------------------------------------------

    /// Builds a database exactly as version 1 left it, **with a library in it**: a watch, a document
    /// and the position the older build recorded for it. Every fixture here is built from the real
    /// schema constant, so it cannot drift away from the version it claims to reproduce.
    fn a_version_1_database_with_a_position() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("mirk.sqlite3");
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
            .execute(
                "INSERT INTO documents (path, path_normalized, uri, title, pages, first_seen_at) \
                 VALUES (?1, ?1, ?2, ?3, 120, 9)",
                params![
                    "/library/watched/read.pdf",
                    "file:///library/watched/read.pdf",
                    "A document the older build knew"
                ],
            )
            .expect("record a document the way version 1 did");
        connection
            .execute(
                "INSERT INTO progress (doc_id, page, total, source, confidence, updated_at) \
                 VALUES (1, 5, 120, ?1, ?2, 99)",
                params!["native", "exact"],
            )
            .expect("record a position the way version 1 did");
        connection
            .pragma_update(None, "user_version", 1i64)
            .expect("stamp version 1");
        drop(connection);
        (dir, path)
    }

    /// Builds a database this build created, stamped with the version it created: an ordinary launch
    /// after the upgrade, with one setting of the reader's already in it.
    fn a_current_database() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("mirk.sqlite3");
        {
            let store = Store::open(&path).expect("create a current database");
            store
                .set_setting("language", "es")
                .expect("write a setting");
        }
        (dir, path)
    }

    /// A path that holds no database yet: opening it is the fresh-install path, which creates the file
    /// and applies every migration.
    fn a_fresh_database_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("mirk.sqlite3");
        assert!(!path.exists(), "the fixture must not create the file");
        (dir, path)
    }

    /// Builds a current-shaped database stamped as version 8 to detect replay of the V9 step.
    fn a_current_database_stamped_8() -> (tempfile::TempDir, PathBuf) {
        let (dir, path) = a_current_database();
        {
            let connection = Connection::open(&path).expect("open raw");
            connection
                .pragma_update(None, "user_version", 8i64)
                .expect("leave the stamp behind the shape");
        }
        (dir, path)
    }

    /// A connection's busy timeout in milliseconds, as SQLite reports it.
    fn busy_timeout_millis(connection: &Connection) -> i64 {
        connection
            .pragma_query_value(None, "busy_timeout", |row| row.get(0))
            .expect("read the busy timeout")
    }

    /// Joins `handle`, failing instead of waiting forever if the thread never finishes.
    ///
    /// The opener threads below wait on another connection's lock, so a regression that made them wait
    /// for good would otherwise hang the test run instead of reporting itself. The bound is well above
    /// the migration's own wait, so a legitimate wait is never turned into a failure.
    fn join_bounded<T>(handle: std::thread::JoinHandle<T>, what: &str) -> T {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !handle.is_finished() {
            assert!(
                std::time::Instant::now() < deadline,
                "{what} never finished; it is still waiting on a lock"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        handle.join().expect("the opener thread must not panic")
    }

    /// Whether `name` exists as an object of `kind` (`table`, `index`, ...). Unit tests may look at
    /// SQLite's own catalogue; the crate's integration tests deliberately do not.
    fn schema_object_exists(connection: &Connection, kind: &str, name: &str) -> bool {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = ?1 AND name = ?2",
                params![kind, name],
                |row| row.get(0),
            )
            .expect("count the schema objects");
        count > 0
    }

    /// Waits, bounded, until `path` has recorded `count` pre-lock version reads.
    ///
    /// This is the synchronization the race tests use: it proves the openers they spawned have read
    /// the version *before* the competing writer commits, so the interleaving under test is forced
    /// rather than hoped for. The bound turns "the open never got that far" into a failure instead of
    /// a hang.
    fn wait_for_version_reads(path: &Path, count: usize) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while observations::version_reads(path) < count {
            assert!(
                std::time::Instant::now() < deadline,
                "no open read the schema version of {} {count} time(s); the race could not be forced",
                path.display()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn a_failed_late_step_rolls_the_whole_upgrade_back() {
        // The unit of rollback is the upgrade, not the step. The fixture is a version 1 database with
        // a real library in it, and the failure is planted where a hand-edited database would trip:
        // V9 creates `authors`, so a table already holding that name makes the step that reaches it
        // fail — after V2 to V8 have already been applied in this same open.
        let (_dir, path) = a_version_1_database_with_a_position();
        {
            let connection = Connection::open(&path).expect("open raw");
            connection
                .execute_batch("CREATE TABLE authors (id INTEGER PRIMARY KEY)")
                .expect("plant the collision the V9 step will hit");
        }

        let error = Store::open(&path).expect_err("the upgrade must fail on the planted collision");
        assert!(
            matches!(error, MirkError::Storage(_)),
            "a failed migration must surface as a storage error, got {error:?}"
        );

        let connection = Connection::open(&path).expect("open raw");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("read the version");
        assert_eq!(
            version, 1,
            "a failed upgrade must leave the version it was opened with"
        );

        // Not one step of the upgrade survives, including the ones that had already run when V9
        // failed. Each assertion names the step it belongs to.
        assert!(
            !schema_object_exists(&connection, "index", "idx_folders_path_unique"),
            "the V2 folder index was applied before the failure and must be rolled back"
        );
        assert!(
            !schema_object_exists(&connection, "table", "settings"),
            "the V3 settings table was applied before the failure and must be rolled back"
        );
        assert!(
            !column_exists(&connection, "progress", "read_at").expect("ask about read_at"),
            "the V4 column must be rolled back"
        );
        assert!(
            !column_exists(&connection, "documents", "user_title").expect("ask about user_title"),
            "the V5 column must be rolled back"
        );
        assert!(
            !column_exists(&connection, "documents", "cover_page").expect("ask about cover_page"),
            "the V6 column must be rolled back"
        );
        assert!(
            !column_exists(&connection, "documents", "user_author").expect("ask about user_author"),
            "the V7 column must be rolled back"
        );
        assert!(
            !column_exists(&connection, "suggestions", "decision").expect("ask about decision"),
            "the V8 column must be rolled back"
        );
        assert!(
            !schema_object_exists(&connection, "table", "document_authors"),
            "the V9 tables must not survive the step that failed"
        );

        // Every row the older build recorded is still exactly where it was.
        let (folder, recursive, enabled): (String, i64, i64) = connection
            .query_row("SELECT path, recursive, enabled FROM folders", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .expect("the watch must survive the failed upgrade");
        assert_eq!(folder, "/library/watched");
        assert_eq!((recursive, enabled), (1, 1));
        let (document, title, pages): (String, Option<String>, Option<i64>) = connection
            .query_row("SELECT path, title, pages FROM documents", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .expect("the document must survive the failed upgrade");
        assert_eq!(document, "/library/watched/read.pdf");
        assert_eq!(title.as_deref(), Some("A document the older build knew"));
        assert_eq!(pages, Some(120));
        let (page, total): (i64, Option<i64>) = connection
            .query_row("SELECT page, total FROM progress", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .expect("the position must survive the failed upgrade");
        assert_eq!((page, total), (5, Some(120)));

        // The collision itself is outside the transaction — it was planted by someone else — and the
        // rollback must not have taken it with it.
        assert!(
            schema_object_exists(&connection, "table", "authors"),
            "the planted table is not part of the upgrade and must be left alone"
        );
        drop(connection);

        // And the rolled-back database is still a database an upgrade can be retried on: with the
        // obstacle gone, the very next open applies every step and keeps the library.
        {
            let connection = Connection::open(&path).expect("open raw");
            connection
                .execute_batch("DROP TABLE authors")
                .expect("remove the obstacle");
        }
        let store = Store::open(&path).expect("the retry must succeed");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );
        let library = store.library().expect("list the library");
        assert_eq!(library.len(), 1);
        assert_eq!(
            library[0].document.document.path,
            PathBuf::from("/library/watched/read.pdf")
        );
        assert_eq!(
            library[0]
                .progress
                .as_ref()
                .expect("the position survived")
                .page,
            5
        );
    }

    #[test]
    fn two_openers_at_once_migrate_an_old_database_exactly_once() {
        // Two real opens of the same version 1 database, serialized by SQLite's own write lock. A
        // third connection holds that lock and writes nothing, which is what makes the test
        // deterministic: both openers read version 1 and then queue behind it, and only once both
        // have read is the lock released. The winner migrates; the loser must find the work done
        // rather than replay it — a replay fails on the V2 index it would create twice.
        let (_dir, path) = a_version_1_database_with_a_position();

        let gate = Connection::open(&path).expect("open raw");
        gate.execute_batch("BEGIN IMMEDIATE")
            .expect("take the write lock");

        let openers: Vec<_> = (0..2)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || Store::open(&path))
            })
            .collect();

        wait_for_version_reads(&path, 2);
        gate.execute_batch("COMMIT")
            .expect("release the write lock");

        for opener in openers {
            let store = join_bounded(opener, "one of the competing openers").expect(
                "both openers must succeed: the second must re-read the version, not replay",
            );
            assert_eq!(
                store.schema_version().expect("schema version"),
                Store::CURRENT_SCHEMA_VERSION
            );
        }

        // Exactly one upgrade happened, and it kept everything the older build recorded.
        let store = Store::open(&path).expect("reopen the migrated database");
        let folders = store.folders().expect("list folders");
        assert_eq!(folders.len(), 1, "the watch was recorded once, not twice");
        let library = store.library().expect("list the library");
        assert_eq!(library.len(), 1);
        assert_eq!(
            library[0]
                .progress
                .as_ref()
                .expect("the position survived")
                .page,
            5
        );
    }

    #[test]
    fn an_open_that_waited_for_the_write_lock_uses_the_version_it_finds_there() {
        // The version a waiting open read before the lock is stale by the time it gets in. Here the
        // shape is already current and only the stamp is behind, so a replay of V9 fails on the
        // `user_author` column that is already gone: an open that did not re-read cannot finish.
        let (_dir, path) = a_current_database_stamped_8();
        // The fixture's own open read the version once; the test counts from here.
        observations::reset(&path);

        let competitor = Connection::open(&path).expect("open raw");
        competitor
            .execute_batch("BEGIN IMMEDIATE")
            .expect("take the write lock");

        let opener = {
            let path = path.clone();
            std::thread::spawn(move || Store::open(&path))
        };

        wait_for_version_reads(&path, 1);

        // The competing writer finishes the upgrade it was already running and records the version
        // while the opener is queued behind it.
        competitor
            .pragma_update(
                None,
                "user_version",
                i64::from(Store::CURRENT_SCHEMA_VERSION),
            )
            .expect("record the version the upgrade reached");
        competitor
            .execute_batch("COMMIT")
            .expect("release the lock");

        let store = join_bounded(opener, "the waiting opener").expect(
            "the waiting open must use the version it finds under the lock, not the stale one",
        );
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );
        // Nothing was rewritten: the reopen found the database current and left it alone.
        assert_eq!(
            store.get_setting("language").expect("read the setting"),
            Some("es".to_string())
        );
        assert_eq!(
            observations::version_reads(&path),
            1,
            "the fast path took no further version read: the re-read happens inside the lock"
        );
    }

    #[test]
    fn a_current_database_can_be_opened_while_a_writer_holds_the_lock() {
        // Opening a current store takes no write reservation. If it took one it would have to wait out
        // the connection's own busy timeout behind the lock below and then fail instead of opening.
        let (_dir, path) = a_current_database_stamped_8();
        {
            let connection = Connection::open(&path).expect("open raw");
            connection
                .pragma_update(
                    None,
                    "user_version",
                    i64::from(Store::CURRENT_SCHEMA_VERSION),
                )
                .expect("restore the current stamp");
        }

        let writer = Connection::open(&path).expect("open raw");
        writer
            .execute_batch("BEGIN IMMEDIATE")
            .expect("take the write lock");

        let store =
            Store::open(&path).expect("a current database must open without the write lock");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );
        assert_eq!(
            store.get_setting("language").expect("read the setting"),
            Some("es".to_string())
        );
        drop(store);

        writer.execute_batch("ROLLBACK").expect("release the lock");

        // And the store the write is meant for still writes, once the lock is free.
        let store = Store::open(&path).expect("reopen");
        store
            .set_setting("language", "en")
            .expect("write after the lock is gone");
        assert_eq!(
            store.get_setting("language").expect("read the setting"),
            Some("en".to_string())
        );
    }

    // --- The connection's own contention settings survive an upgrade ---------------------------

    #[test]
    fn a_successful_upgrade_restores_the_busy_timeout_it_found() {
        // The upgrade lengthens the connection's busy timeout so it can wait for a competing writer.
        // What it must not do is leave a different value behind: a store that had to be migrated has
        // to contend exactly like one that was already current, and the value is read from the
        // connection rather than assumed to be any particular default.
        let (_dir, path) = a_version_1_database_with_a_position();
        let mut connection = Connection::open(&path).expect("open raw");
        connection
            .busy_timeout(Duration::from_millis(1234))
            .expect("set a timeout that is neither zero nor the driver's default");

        upgrade(&mut connection, None).expect("the upgrade must succeed");

        assert_eq!(
            busy_timeout_millis(&connection),
            1234,
            "a successful upgrade must put the connection's own busy timeout back"
        );
    }

    #[test]
    fn a_failed_upgrade_restores_the_busy_timeout_it_found() {
        // The same promise on the path that fails: the open is refused, and the connection it was
        // refused on is left as it was found rather than holding the upgrade's longer wait.
        let (_dir, path) = a_version_1_database_with_a_position();
        {
            let connection = Connection::open(&path).expect("open raw");
            connection
                .execute_batch("CREATE TABLE authors (id INTEGER PRIMARY KEY)")
                .expect("plant the collision the V9 step will hit");
        }
        let mut connection = Connection::open(&path).expect("open raw");
        connection
            .busy_timeout(Duration::from_millis(1234))
            .expect("set a timeout that is neither zero nor the driver's default");

        let error =
            upgrade(&mut connection, None).expect_err("the upgrade must fail on the collision");
        assert!(
            matches!(error, MirkError::Storage(_)),
            "the failure must reach the caller as a storage error, got {error:?}"
        );

        assert_eq!(
            busy_timeout_millis(&connection),
            1234,
            "a failed upgrade must put the connection's own busy timeout back"
        );
    }

    #[test]
    fn an_upgrade_that_finds_the_work_done_restores_the_busy_timeout_it_found() {
        // The no-op path: an upgrade whose write lock finds the database already current — what an
        // open sees when a competing writer finished while it waited. It applies nothing and still
        // owes the connection its own timeout back.
        let (_dir, path) = a_current_database();
        let mut connection = Connection::open(&path).expect("open raw");
        connection
            .busy_timeout(Duration::from_millis(1234))
            .expect("set a timeout that is neither zero nor the driver's default");

        upgrade(&mut connection, None).expect("an upgrade with nothing to do succeeds");

        assert_eq!(
            busy_timeout_millis(&connection),
            1234,
            "an upgrade that applied nothing must still put the connection's timeout back"
        );
    }

    #[test]
    fn an_upgraded_store_contends_exactly_like_a_current_one() {
        // The regression this pins: the upgrade overrides the busy timeout and used to restore a
        // guessed value, so a fresh or upgraded store ended up contending differently from a store
        // that was already current. Both are compared with an untouched connection, so the assertion
        // is "the driver's value survived", not "the value is any particular constant".
        let (_fresh_dir, fresh_path) = a_fresh_database_path();
        let fresh = Store::open(&fresh_path).expect("migrate a fresh database");
        let (_current_dir, current_path) = a_current_database();
        let current = Store::open(&current_path).expect("open a current database");
        let untouched = Connection::open_in_memory().expect("open an untouched connection");

        assert_eq!(
            busy_timeout_millis(&fresh.connection),
            busy_timeout_millis(&current.connection),
            "an upgraded store and a current store must contend the same way"
        );
        assert_eq!(
            busy_timeout_millis(&fresh.connection),
            busy_timeout_millis(&untouched),
            "the upgrade must keep the busy timeout the driver opened the connection with"
        );

        // And the write path still reports contention instead of retrying it away: with another
        // connection holding the write lock, the store's own write is refused. The wait is shortened
        // for this check alone; what the assertions above pin is that no migration wait was left
        // behind to lengthen it.
        for (what, store, path) in [
            ("the upgraded store", &fresh, &fresh_path),
            ("the current store", &current, &current_path),
        ] {
            store
                .connection
                .busy_timeout(Duration::from_millis(10))
                .expect("shorten the wait so the check is quick");
            let gate = Connection::open(path).expect("open raw");
            gate.execute_batch("BEGIN IMMEDIATE")
                .expect("hold the write lock");

            let outcome = store.set_setting("language", "en");

            assert!(
                matches!(outcome, Err(MirkError::Storage(_))),
                "{what} must report a busy database rather than retrying it away: {outcome:?}"
            );
            gate.execute_batch("ROLLBACK").expect("release the lock");
        }
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

    // --- A genuine version 11 database, built from the frozen statements ------------------------

    /// Builds a database exactly as version 11 left it, from the **frozen** V1–V11 statements the
    /// migrations execute — not by building the current shape and rewinding the stamp, which would
    /// only prove that this build can open its own output.
    ///
    /// Every step is applied as the migration applies it, including the V9 backfill that moves the
    /// reader's `user_author` into `authors`/`document_authors` and the V10 cover-source backfill. The
    /// fixture therefore carries representative existing data: a watched folder, a document with its
    /// own and the reader's title and author, a position with provenance and reading time, an
    /// undecided suggestion with its history time, and the interface language.
    ///
    /// `pre_existing` — when given — is created after V11, as the incompatible object the V12 step
    /// would meet.
    fn a_genuine_version_11_database(pre_existing: Option<&str>) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("mirk.sqlite3");
        let connection = Connection::open(&path).expect("open raw");
        for schema in [
            SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5, SCHEMA_V6, SCHEMA_V7, SCHEMA_V8,
        ] {
            connection
                .execute_batch(schema)
                .expect("apply a schema a version 11 database has");
        }
        connection
            .execute(
                "INSERT INTO folders (path, recursive, enabled, added_at) VALUES (?1, 1, 1, 7)",
                params!["/library/watched"],
            )
            .expect("record a folder the way version 8 did");
        connection
            .execute(
                "INSERT INTO documents \
                 (path, path_normalized, uri, title, author, pages, first_seen_at, user_title, user_author) \
                 VALUES (?1, ?1, ?2, ?3, ?4, 80, 9, ?5, ?6)",
                params![
                    "/library/watched/read.pdf",
                    "file:///library/watched/read.pdf",
                    "The document's own title",
                    "Sagan, Carl; Druyan, Ann",
                    "The reader's title",
                    "Carl Sagan"
                ],
            )
            .expect("record a document the way version 8 did");

        // V9 exactly as the migration applies it: the tables, the backfill that moves the reader's
        // author into them, and the retirement of the column it came from.
        let transaction = connection
            .unchecked_transaction()
            .expect("open the V9 unit");
        transaction
            .execute_batch(SCHEMA_V9)
            .expect("apply the V9 schema");
        backfill_reader_authors(&transaction).expect("run the V9 backfill");
        transaction
            .execute_batch(SCHEMA_V9_RETIRE_USER_AUTHOR)
            .expect("retire the column V9 replaces");
        transaction.commit().expect("commit the V9 unit");

        // V10 and V11, as the migration applies them.
        connection
            .execute_batch(SCHEMA_V10_ADD)
            .expect("add the V10 column");
        connection
            .execute_batch(SCHEMA_V10_BACKFILL)
            .expect("run the V10 backfill");

        // A suggestion that exists **before** the V11 step. It has no `history_at` column yet, so the
        // value this row ends up with is the one `ALTER TABLE ... ADD COLUMN history_at` supplies:
        // NULL, meaning "no source said when this was seen", not 1970 and not the first sighting.
        connection
            .execute(
                "INSERT INTO suggestions (path, seen_count, first_seen_at, decided_at, decision) \
                 VALUES (?1, 2, 1650000000, NULL, NULL)",
                params!["/elsewhere/pending.pdf"],
            )
            .expect("record a suggestion the way version 10 did");
        connection
            .execute_batch(SCHEMA_V11)
            .expect("add the V11 column");

        // The rest of the library a version 11 reader already had, including a suggestion recorded
        // after V11 with the history time the desktop knows.
        connection
            .execute(
                "INSERT INTO progress (doc_id, page, total, source, confidence, read_at, updated_at) \
                 VALUES (1, 12, 80, ?1, ?2, 1600000000, 99)",
                params!["native", "exact"],
            )
            .expect("record a position the way version 11 did");
        connection
            .execute(
                "INSERT INTO suggestions (path, seen_count, first_seen_at, decided_at, decision, history_at) \
                 VALUES (?1, 3, 1700000000, NULL, NULL, 1600000000)",
                params!["/elsewhere/suggested.pdf"],
            )
            .expect("record a suggestion the way version 11 did");
        connection
            .execute(
                "INSERT INTO settings (key, value) VALUES ('language', 'es')",
                [],
            )
            .expect("record a setting the way version 11 did");

        if let Some(statement) = pre_existing {
            connection
                .execute_batch(statement)
                .expect("create the incompatible object the step will meet");
        }
        connection
            .pragma_update(None, "user_version", 11i64)
            .expect("stamp version 11");
        drop(connection);
        (dir, path)
    }

    #[test]
    fn a_genuine_version_11_database_upgrades_to_12_without_losing_a_row() {
        // The fixture is a real version 11 database: frozen statements, the real V9 and V10
        // backfills, and rows a reader already had. The upgrade must add the reading tables and
        // change nothing else — a row, a column value or a relation.
        let (_dir, path) = a_genuine_version_11_database(None);
        {
            let raw = Connection::open(&path).expect("open raw");
            assert_eq!(read_schema_version(&raw).expect("version"), 11);
            assert!(
                !schema_object_exists(&raw, "table", "reading_sessions")
                    && !schema_object_exists(&raw, "table", "page_visits"),
                "a version 11 database cannot already hold the tables V12 adds"
            );
            // The suggestion that existed before the V11 step carries the V11 column's default: NULL,
            // meaning "no source said when this was seen". The fixture records the other suggestion
            // after V11 precisely so the two states are distinguishable.
            let pending_history: Option<i64> = raw
                .query_row(
                    "SELECT history_at FROM suggestions WHERE path = '/elsewhere/pending.pdf'",
                    [],
                    |row| row.get(0),
                )
                .expect("read the pre-V11 suggestion's history time");
            assert_eq!(
                pending_history, None,
                "a suggestion present before the V11 step must get the column default, not a time"
            );
        }

        let store = Store::open(&path).expect("a genuine version 11 database migrates");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );
        let tables = store.table_names().expect("table names");
        for added in ["reading_sessions", "page_visits"] {
            assert!(
                tables.iter().any(|name| name == added),
                "the migration must create `{added}`; found {tables:?}"
            );
        }
        assert_eq!(
            store
                .connection
                .query_row("SELECT COUNT(*) FROM reading_sessions", [], |row| row
                    .get::<_, i64>(0))
                .expect("count the sessions"),
            0,
            "the migration creates a home for facts, it does not invent any"
        );
        assert_eq!(
            store
                .connection
                .query_row("SELECT COUNT(*) FROM page_visits", [], |row| row
                    .get::<_, i64>(0))
                .expect("count the visits"),
            0
        );

        let stored = store
            .document_by_id(1)
            .expect("read")
            .expect("the document is still there");
        assert_eq!(
            stored.document.path,
            PathBuf::from("/library/watched/read.pdf")
        );
        assert_eq!(stored.document.pages, Some(80));
        assert_eq!(
            stored.document.title.as_deref(),
            Some("The document's own title")
        );
        assert_eq!(
            stored.document.author.as_deref(),
            Some("Sagan, Carl; Druyan, Ann")
        );
        assert_eq!(
            stored.document.user_title.as_deref(),
            Some("The reader's title")
        );
        assert_eq!(stored.document.authors, vec!["Carl Sagan".to_string()]);

        let progress = store.progress_for_document(1).expect("read the position");
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0].page, 12);
        assert_eq!(progress[0].total, Some(80));
        assert_eq!(progress[0].source, ProgressSource::Native);
        assert_eq!(progress[0].confidence, Confidence::Exact);
        assert_eq!(progress[0].read_at, Some(1_600_000_000));

        let suggestion = store.suggestion(1).expect("read").expect("still present");
        assert_eq!(suggestion.seen_count, 2);
        assert_eq!(suggestion.first_seen_at, 1_650_000_000);
        assert_eq!(
            suggestion.history_at, None,
            "the V11 default survives the upgrade: no history time was ever recorded"
        );

        let suggestion = store.suggestion(2).expect("read").expect("still present");
        assert_eq!(suggestion.seen_count, 3);
        assert_eq!(suggestion.first_seen_at, 1_700_000_000);
        assert_eq!(suggestion.history_at, Some(1_600_000_000));

        assert_eq!(store.folders().expect("list folders").len(), 1);
        assert_eq!(
            store.get_setting("language").expect("read the setting"),
            Some("es".to_string())
        );
    }

    #[test]
    fn the_upgraded_genuine_version_11_database_reopens_without_losing_a_row() {
        // The upgrade's output has to survive the next open: the guard compares the tables again and
        // the version 11 data must still read back exactly.
        let (_dir, path) = a_genuine_version_11_database(None);
        drop(Store::open(&path).expect("migrate the genuine version 11 database"));

        let store = Store::open(&path).expect("the upgraded database reopens");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );
        let stored = store.document_by_id(1).expect("read").expect("row");
        assert_eq!(
            stored.document.user_title.as_deref(),
            Some("The reader's title")
        );
        assert_eq!(stored.document.authors, vec!["Carl Sagan".to_string()]);
        assert_eq!(
            store
                .progress_for_document(1)
                .expect("read the position")
                .first()
                .expect("the position is still there")
                .page,
            12
        );
        assert_eq!(
            store.suggestion(1).expect("read").expect("row").history_at,
            None,
            "the V11 column default survives the upgrade and the reopen"
        );
        assert_eq!(
            store.suggestion(2).expect("read").expect("row").history_at,
            Some(1_600_000_000),
            "a history time recorded after V11 survives too"
        );
        assert_eq!(
            store.get_setting("language").expect("read the setting"),
            Some("es".to_string())
        );
    }

    #[test]
    fn a_pre_existing_reading_sessions_table_stops_the_upgrade_before_any_step_runs() {
        // A table the upgrade must create already exists with a shape this build cannot use. The
        // collision is checked before the migration starts, so the database is left exactly at
        // version 11 with nothing the step would create and nothing rewritten.
        let (_dir, path) = a_genuine_version_11_database(Some(
            "CREATE TABLE reading_sessions (id INTEGER PRIMARY KEY, doc_id INTEGER, started_at INTEGER);",
        ));

        let error = Store::open(&path).expect_err("a collision must stop the upgrade");
        match &error {
            MirkError::Storage(message) => {
                assert!(
                    message.contains("reading_sessions"),
                    "the refusal must name the object in the way: {message}"
                );
                assert!(
                    message.contains("version 11") && message.contains("version 12"),
                    "the refusal must name the version the database is at and the one it would claim: {message}"
                );
                assert!(
                    message.contains(&path.display().to_string()),
                    "the refusal must name the database it is about: {message}"
                );
                assert!(
                    message.contains("refused before any step ran"),
                    "the refusal must say nothing was migrated: {message}"
                );
            }
            other => panic!("a collision must be a storage error, got {other:?}"),
        }

        let raw = Connection::open(&path).expect("open raw");
        assert_eq!(read_schema_version(&raw).expect("version"), 11);
        assert!(!schema_object_exists(&raw, "table", "page_visits"));
        assert_eq!(
            table_columns(&raw, "reading_sessions").expect("columns"),
            vec!["id", "doc_id", "started_at"],
            "the pre-existing table must be untouched"
        );
        let (page, read_at): (i64, Option<i64>) = raw
            .query_row(
                "SELECT page, read_at FROM progress WHERE doc_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("the old position must still be there");
        assert_eq!((page, read_at), (12, Some(1_600_000_000)));
        let language: String = raw
            .query_row(
                "SELECT value FROM settings WHERE key = 'language'",
                [],
                |row| row.get(0),
            )
            .expect("the old setting must still be there");
        assert_eq!(language, "es");

        // The fixture — and only the fixture — removes the obstacle it planted. This build offers no
        // automated repair; the point is that the refusal left a database a later open can migrate
        // normally, with every version 11 row preserved.
        raw.execute_batch("DROP TABLE reading_sessions;")
            .expect("the fixture removes the obstacle it planted");
        drop(raw);

        let store = Store::open(&path).expect("the retry must migrate successfully");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );
        let tables = store.table_names().expect("table names");
        for added in ["reading_sessions", "page_visits"] {
            assert!(
                tables.iter().any(|name| name == added),
                "the retry must create `{added}`; found {tables:?}"
            );
        }
        assert_eq!(
            store
                .connection
                .query_row("SELECT COUNT(*) FROM reading_sessions", [], |row| row
                    .get::<_, i64>(0))
                .expect("count the sessions"),
            0
        );
        assert_eq!(
            store
                .connection
                .query_row("SELECT COUNT(*) FROM page_visits", [], |row| row
                    .get::<_, i64>(0))
                .expect("count the visits"),
            0
        );

        // The old data is still exactly what the version 11 build recorded.
        let stored = store
            .document_by_id(1)
            .expect("read")
            .expect("the document survived the refusal and the retry");
        assert_eq!(stored.document.pages, Some(80));
        assert_eq!(
            stored.document.user_title.as_deref(),
            Some("The reader's title")
        );
        assert_eq!(stored.document.authors, vec!["Carl Sagan".to_string()]);
        let progress = store.progress_for_document(1).expect("read the position");
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0].page, 12);
        assert_eq!(progress[0].read_at, Some(1_600_000_000));
        assert_eq!(
            store.suggestion(1).expect("read").expect("row").history_at,
            None
        );
        assert_eq!(
            store.suggestion(2).expect("read").expect("row").history_at,
            Some(1_600_000_000)
        );
        assert_eq!(store.folders().expect("list folders").len(), 1);
        assert_eq!(
            store.get_setting("language").expect("read the setting"),
            Some("es".to_string())
        );
    }

    #[test]
    fn a_pre_existing_page_visits_table_stops_the_upgrade_before_reading_sessions_is_created() {
        // The same promise for the second table, and in the other order: nothing the step creates may
        // exist after the refusal.
        let (_dir, path) = a_genuine_version_11_database(Some(
            "CREATE TABLE page_visits (id INTEGER PRIMARY KEY, note TEXT);",
        ));

        let error = Store::open(&path).expect_err("a collision must stop the upgrade");
        match &error {
            MirkError::Storage(message) => assert!(
                message.contains("page_visits"),
                "the refusal must name the object in the way: {message}"
            ),
            other => panic!("a collision must be a storage error, got {other:?}"),
        }

        let raw = Connection::open(&path).expect("open raw");
        assert_eq!(read_schema_version(&raw).expect("version"), 11);
        assert!(
            !schema_object_exists(&raw, "table", "reading_sessions"),
            "the V12 step must not have created `reading_sessions`"
        );
        assert_eq!(
            table_columns(&raw, "page_visits").expect("columns"),
            vec!["id", "note"],
            "the pre-existing table must be untouched"
        );
    }

    #[test]
    fn a_pre_existing_v12_object_named_in_another_case_still_stops_the_upgrade() {
        // SQLite resolves object names case-insensitively, so a table called `READING_SESSIONS` is
        // the name the V12 step's `CREATE TABLE reading_sessions` collides with. The preflight must
        // use the same rule, or the step would run and fail only after earlier steps had committed.
        let (_dir, path) = a_genuine_version_11_database(Some(
            "CREATE TABLE READING_SESSIONS (id INTEGER PRIMARY KEY, doc_id INTEGER, started_at INTEGER);",
        ));

        let error = Store::open(&path).expect_err("a case-variant collision must stop the upgrade");
        match &error {
            MirkError::Storage(message) => assert!(
                message.contains("READING_SESSIONS") && message.contains("version 11"),
                "the refusal must name the object as stored and the version: {message}"
            ),
            other => panic!("a collision must be a storage error, got {other:?}"),
        }
        let raw = Connection::open(&path).expect("open raw");
        assert_eq!(read_schema_version(&raw).expect("version"), 11);
        assert!(!schema_object_exists(&raw, "table", "page_visits"));
    }

    #[test]
    fn a_genuine_v11_upgrade_creates_reading_tables_this_build_reopens() {
        // The guard must not refuse the migration's own output: a genuine version 11 database is
        // upgraded, given facts, closed, and opened again — and the facts are still there.
        let (_dir, path) = a_genuine_version_11_database(None);
        {
            let store = Store::open(&path).expect("a version 11 database migrates");
            assert_eq!(
                store.schema_version().expect("schema version"),
                Store::CURRENT_SCHEMA_VERSION
            );
            let raw = Connection::open(&path).expect("open raw");
            raw.execute(
                "INSERT INTO reading_sessions (id, doc_id, started_at, ended_at, active_ms) \
                 VALUES (1, 1, 1700000000, 1700000600, 480000)",
                [],
            )
            .expect("record a session");
            raw.execute(
                "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
                 VALUES (1, 1, 4, 1700000100, 90000)",
                [],
            )
            .expect("record a visit");
        }

        let store = Store::open(&path).expect("the upgraded database reopens");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );
        let session: (i64, i64, i64, i64) = store
            .connection
            .query_row(
                "SELECT doc_id, started_at, ended_at, active_ms FROM reading_sessions",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("the session survived the reopen");
        assert_eq!(session, (1, 1_700_000_000, 1_700_000_600, 480_000));
        let visit: (i64, i64, i64, i64) = store
            .connection
            .query_row(
                "SELECT session_id, page, observed_at, visible_ms FROM page_visits",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("the visit survived the reopen");
        assert_eq!(visit, (1, 4, 1_700_000_100, 90_000));
    }
}
