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
//! - library listing, filtering and sorting over `documents`;
//! - writes and reads for `open_events`, `viewers`, `folders` and `suggestions`;
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

/// A stored document, together with its database id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentRecord {
    /// The `documents.id` primary key, needed to attach progress to the document.
    pub id: i64,
    /// The document as stored.
    pub document: Document,
}

/// A SQLite connection to `oct`'s database.
///
/// `Store` is intentionally the only handle: callers never see a `rusqlite::Connection`, so the
/// schema and its migration stay an implementation detail.
#[derive(Debug)]
pub struct Store {
    connection: Connection,
}

impl Store {
    /// The schema version this build creates and migrates to.
    pub const CURRENT_SCHEMA_VERSION: u32 = 1;

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
    pub fn record_progress(
        &self,
        doc_id: i64,
        progress: &StoredProgress,
    ) -> Result<i64, OctError> {
        self.connection
            .execute(
                "INSERT INTO progress (doc_id, page, total, source, confidence, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    doc_id,
                    i64::from(progress.page),
                    progress.total.map(i64::from),
                    progress.source.id(),
                    confidence_id(progress.confidence),
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
                "SELECT page, total, source, confidence FROM progress \
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
                ))
            })
            .map_err(storage_error)?;

        let mut progress = Vec::new();
        for row in rows {
            let (page, total, source, confidence) = row.map_err(storage_error)?;
            let page = u32::try_from(page)
                .map_err(|_| OctError::Storage(format!("stored page {page} is not a page number")))?;
            let total = total
                .map(u32::try_from)
                .transpose()
                .map_err(|_| {
                    OctError::Storage("stored total is not a valid page count".into())
                })?;

            progress.push(StoredProgress::new(
                page,
                total,
                parse_progress_source(&source),
                parse_confidence(&confidence)?,
            ));
        }
        Ok(progress)
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
    let version = read_schema_version(connection)?;

    if version > Store::CURRENT_SCHEMA_VERSION {
        return Err(OctError::Storage(format!(
            "database schema version {version} is newer than this build supports \
             ({}); refusing to downgrade",
            Store::CURRENT_SCHEMA_VERSION
        )));
    }

    if version < 1 {
        let transaction = connection.transaction().map_err(storage_error)?;
        transaction.execute_batch(SCHEMA_V1).map_err(storage_error)?;
        transaction
            .pragma_update(None, "user_version", i64::from(Store::CURRENT_SCHEMA_VERSION))
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
