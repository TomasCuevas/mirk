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
//! - writes and reads for `open_events`, `viewers` and `suggestions`; the `folders` table has only
//!   what its callers need: [`Store::record_folder`], [`Store::folders`], [`Store::forget_folder`],
//!   [`Store::folder_removal`], [`WatchedFolder`] and the pure [`covering_folder`] rule that
//!   decides whether a folder is already covered by a watch (R20, R22, R27);
//! - metadata writes (`size`, `mtime`, `discovered_via`, `platform`, `status`) — the
//!   [`oct_core::Document`] value type does not carry them yet, so [`Store::upsert_document`]
//!   leaves those columns unset for the scanner (T6) to fill;
//! - path normalisation: paths are stored as opaque UTF-8 and are **never** Unicode-normalised
//!   (risk R7), so `path_normalized` deliberately holds the same bytes as `path`.

use std::collections::HashSet;
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
    pub const CURRENT_SCHEMA_VERSION: u32 = 4;

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
    /// Only the columns [`Document`] can supply are written; the scanner fills metadata such as
    /// `size`, `mtime` and `discovered_via` when it exists.
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
                "SELECT id, path, file_id_kind, volume_id, file_index, content_hash, pages, title, author \
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
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?;

        let Some((id, path, kind, volume, index, hash, pages, title, author)) = row else {
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

        Ok(Some(DocumentRecord {
            id,
            document: Document {
                path: PathBuf::from(path),
                identity,
                pages,
                title,
                author,
            },
        }))
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
    pub fn library(&self) -> Result<Vec<LibraryEntry>, OctError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT d.id, d.path, d.file_id_kind, d.volume_id, d.file_index, d.content_hash, \
                        d.pages, d.title, d.author, \
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
                // The LEFT JOIN leaves every `progress` column NULL for a document with none, so
                // `p.page` is the presence check: it is NOT NULL whenever the row exists.
                let progress: Option<LatestProgressColumns> =
                    match row.get::<_, Option<i64>>(9)? {
                        Some(page) => Some((
                            page,
                            row.get(10)?,
                            row.get(11)?,
                            row.get(12)?,
                            row.get(13)?,
                            row.get(14)?,
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
                    progress,
                ))
            })
            .map_err(storage_error)?;

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
                progress,
            ) = row.map_err(storage_error)?;

            let identity = decode_identity(
                kind.as_deref(),
                volume.as_deref(),
                index.as_deref(),
                hash.as_deref(),
            )?;
            let pages = pages.map(u32::try_from).transpose().map_err(|_| {
                OctError::Storage("stored page count is not a valid page number".into())
            })?;

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
                    document: Document {
                        path: PathBuf::from(path),
                        identity,
                        pages,
                        title,
                        author,
                    },
                },
                progress,
                progress_recorded_at,
            });
        }
        Ok(entries)
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
    }

    Ok(())
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

    #[test]
    fn a_version_3_database_gains_read_at_and_keeps_its_rows() {
        let (_dir, path) = a_version_3_database();

        let store = Store::open(&path).expect("opening migrates the database");
        assert_eq!(store.schema_version().expect("schema version"), 4);

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
        assert_eq!(store.schema_version().expect("schema version"), 4);

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
        assert_eq!(store.schema_version().expect("schema version"), 4);

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
        assert_eq!(store.schema_version().expect("schema version"), 4);
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
