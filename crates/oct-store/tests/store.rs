//! T5 acceptance tests, exercised only through the public interface of `oct-store`.
//!
//! Every database these tests touch lives in a `tempfile` directory, never in the user's real
//! data directory or `$HOME`. Nothing here inspects `sqlite_master` directly: the schema is
//! observed through `Store`'s own API.

use std::path::{Path, PathBuf};

use oct_core::{Confidence, Document, FileId, OctError, Platform, ProgressSource, StoredProgress};
use oct_store::{covering_folder, database_path, user_data_dir_for, Store, WatchedFolder};

fn temp_db() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("oct.sqlite3");
    (dir, path)
}

fn document(path: &str, identity: FileId) -> Document {
    let mut doc = Document::new(path);
    doc.identity = Some(identity);
    doc
}

/// Unix seconds, the unit both `progress.read_at` and `progress.updated_at` are stored in. A local
/// mirror of the store's own clock so the tests can bracket a write rather than trust one instant.
fn unix_now() -> i64 {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch");
    i64::try_from(duration.as_secs()).expect("Unix seconds fit an i64")
}

// --- S1: the documented tables -----------------------------------------------------------------

#[test]
fn schema_contains_exactly_the_documented_tables() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let tables = store.table_names().expect("table names");
    assert_eq!(
        tables,
        vec![
            "documents",
            "folders",
            "open_events",
            "progress",
            "settings",
            "suggestions",
            "viewers",
        ]
    );
}

// --- S2: provenance survives storage ---------------------------------------------------------

#[test]
fn progress_keeps_source_and_confidence_distinct_after_a_round_trip() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let doc_id = store
        .upsert_document(&Document::new("/library/document.pdf"))
        .expect("insert document");

    // Same page, same total, different provenance: the two rows must not collapse into one.
    let from_viewer = StoredProgress::new(
        5,
        Some(80),
        ProgressSource::External("evince-gvfs"),
        Confidence::Exact,
    );
    let by_hand = StoredProgress::new(5, Some(80), ProgressSource::Manual, Confidence::Manual);
    assert_ne!(from_viewer, by_hand);

    store
        .record_progress(doc_id, &from_viewer)
        .expect("record viewer progress");
    store
        .record_progress(doc_id, &by_hand)
        .expect("record manual progress");

    let rows = store.progress_for_document(doc_id).expect("read progress");
    assert_eq!(rows, vec![from_viewer, by_hand]);
}

#[test]
fn a_reading_time_round_trips_through_library_and_per_document_reads() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let doc_id = store
        .upsert_document(&Document::new("/library/read.pdf"))
        .expect("insert document");

    let read_at = 1_700_000_000;
    store
        .record_progress(
            doc_id,
            &StoredProgress::new(
                5,
                Some(80),
                ProgressSource::External("evince-gvfs"),
                Confidence::Exact,
            )
            .with_read_at(Some(read_at)),
        )
        .expect("record progress");

    let rows = store.progress_for_document(doc_id).expect("read progress");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].read_at, Some(read_at),
        "the reading time must survive the round trip"
    );

    let library = store.library().expect("list the library");
    assert_eq!(library.len(), 1);
    assert_eq!(
        library[0].progress.as_ref().and_then(|progress| progress.read_at),
        Some(read_at),
        "the listing must carry the reading time with the rest of the row"
    );

    // A row read back can be written again unchanged: `read_at` travels with the value, so a caller
    // that re-records what it read does not silently drop the reading time.
    let read_back = rows[0].clone();
    store
        .record_progress(doc_id, &read_back)
        .expect("re-record what was read");
    let again = store.progress_for_document(doc_id).expect("read progress");
    assert_eq!(again.len(), 2);
    assert_eq!(
        again[1], read_back,
        "a stored row must survive a read-and-write round trip unchanged"
    );
}

#[test]
fn progress_written_without_a_reading_time_reads_back_as_unknown() {
    // The defect this slice removes: the write time was standing in for the reading time. A caller
    // that knows only where the reader is must not be taken to claim when they read it.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let doc_id = store
        .upsert_document(&Document::new("/library/read.pdf"))
        .expect("insert document");

    store
        .record_progress(
            doc_id,
            &StoredProgress::new(5, Some(80), ProgressSource::Native, Confidence::Exact),
        )
        .expect("record progress");

    let rows = store.progress_for_document(doc_id).expect("read progress");
    assert_eq!(
        rows[0].read_at, None,
        "no source said when this was read, so the answer is `None`, not a fabricated time"
    );
    // Explicitly not the two tempting substitutes: the row's own write time, and zero.
    assert_ne!(rows[0].read_at, Some(0), "unknown is not the epoch");

    let library = store.library().expect("list the library");
    let entry = &library[0];
    assert_eq!(
        entry.progress.as_ref().and_then(|progress| progress.read_at),
        None
    );
    assert_ne!(
        entry.progress.as_ref().and_then(|progress| progress.read_at),
        entry.progress_recorded_at,
        "the reading time must not be filled in from the row's write time"
    );
    assert!(
        entry.progress_recorded_at.is_some(),
        "the app still records when it wrote the row"
    );
}

#[test]
fn writing_a_row_stamps_its_own_time_even_when_the_reading_time_is_old() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let doc_id = store
        .upsert_document(&Document::new("/library/read.pdf"))
        .expect("insert document");

    let a_year: i64 = 365 * 24 * 60 * 60;
    let before = unix_now();
    let old = before - a_year;
    store
        .record_progress(
            doc_id,
            &StoredProgress::new(
                5,
                Some(80),
                ProgressSource::External("evince-gvfs"),
                Confidence::Exact,
            )
            .with_read_at(Some(old)),
        )
        .expect("record progress");
    let after = unix_now();

    let library = store.library().expect("list the library");
    let entry = &library[0];
    let recorded_at = entry.progress_recorded_at.expect("a write time");
    assert!(
        recorded_at >= before && recorded_at <= after,
        "the write time must be now ({before}..={after}), got {recorded_at}"
    );
    assert_ne!(
        recorded_at, old,
        "the write time must not be the year-old reading time"
    );
    assert_eq!(
        entry.progress.as_ref().and_then(|progress| progress.read_at),
        Some(old),
        "the reading time must stay what the source reported"
    );
}

// --- S3: portable identity, never only a path -------------------------------------------------

#[test]
fn every_file_id_variant_round_trips_and_is_found_by_identity_alone() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    let cases = [
        (
            FileId::Posix {
                dev: 2049,
                ino: 9001,
            },
            "/library/posix.pdf",
        ),
        (
            FileId::Windows {
                volume_serial: 0xDEAD_BEEF_1234_5678,
                file_index: 123_456_789,
            },
            "/library/windows.pdf",
        ),
        (
            FileId::ContentHash("sha256:0f1e2d3c".to_string()),
            "/library/hash.pdf",
        ),
        // The full `u64` range must survive; a naive `i64` cast would wrap.
        (
            FileId::Posix {
                dev: u64::MAX,
                ino: u64::MAX,
            },
            "/library/large-ids.pdf",
        ),
    ];

    for (identity, path_str) in &cases {
        let mut doc = document(path_str, identity.clone());
        doc.pages = Some(12);
        doc.title = Some("A title".to_string());
        store.upsert_document(&doc).expect("insert document");
    }

    for (identity, path_str) in &cases {
        let found = store
            .find_document_by_identity(identity)
            .expect("lookup")
            .unwrap_or_else(|| panic!("identity {identity:?} was not found by itself"));
        assert_eq!(found.document.identity.as_ref(), Some(identity));
        assert_eq!(found.document.path, PathBuf::from(path_str));
    }

    // An identity that was never stored is absent, not fabricated.
    let missing = store
        .find_document_by_identity(&FileId::Posix { dev: 1, ino: 2 })
        .expect("lookup");
    assert!(missing.is_none());
}

// --- S4: pages are zero-based in storage ------------------------------------------------------

#[test]
fn page_zero_is_stored_and_read_back_as_zero() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let doc_id = store
        .upsert_document(&Document::new("/library/first-page.pdf"))
        .expect("insert document");

    store
        .record_progress(
            doc_id,
            &StoredProgress::new(0, Some(80), ProgressSource::Native, Confidence::Exact),
        )
        .expect("record progress");

    let rows = store.progress_for_document(doc_id).expect("read progress");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].page, 0, "page 0 must not be converted to page 1");
    assert_ne!(rows[0].page, 1);
}

// --- S5: the database lives in the user data directory ----------------------------------------

#[test]
fn user_data_directory_follows_the_platform_conventions() {
    assert_eq!(
        user_data_dir_for(
            Platform::Linux,
            Some(Path::new("/xdg/data")),
            Some(Path::new("/home/u")),
            None,
        )
        .expect("linux with XDG_DATA_HOME"),
        PathBuf::from("/xdg/data/oct")
    );

    // XDG spec fallback when XDG_DATA_HOME is unset.
    assert_eq!(
        user_data_dir_for(Platform::Linux, None, Some(Path::new("/home/u")), None)
            .expect("linux fallback"),
        PathBuf::from("/home/u/.local/share/oct")
    );

    assert_eq!(
        user_data_dir_for(
            Platform::Windows,
            None,
            Some(Path::new("C:/Users/u")),
            Some(Path::new("C:/Users/u/AppData/Roaming")),
        )
        .expect("windows"),
        PathBuf::from("C:/Users/u/AppData/Roaming/oct")
    );

    assert_eq!(
        user_data_dir_for(Platform::MacOs, None, Some(Path::new("/Users/u")), None)
            .expect("macos"),
        PathBuf::from("/Users/u/Library/Application Support/oct")
    );

    // An unsupported platform says so rather than guessing a location.
    let unsupported = user_data_dir_for(Platform::Other, None, None, None);
    assert!(matches!(unsupported, Err(OctError::NotSupported { .. })));

    match user_data_dir_for(Platform::Linux, None, None, None) {
        Err(OctError::Storage(message)) => {
            assert!(message.contains("XDG_DATA_HOME"), "{message}");
            assert!(message.contains("HOME"), "{message}");
        }
        other => panic!("expected a storage error, got {other:?}"),
    }
}

#[test]
fn database_path_is_under_the_xdg_data_home_and_never_the_working_directory() {
    if Platform::current() != Platform::Linux {
        // The variable consulted differs per platform; this assertion is Linux-specific by design.
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let previous = std::env::var_os("XDG_DATA_HOME");
    std::env::set_var("XDG_DATA_HOME", dir.path());
    let resolved = database_path();
    match previous {
        Some(value) => std::env::set_var("XDG_DATA_HOME", value),
        None => std::env::remove_var("XDG_DATA_HOME"),
    }

    let resolved = resolved.expect("resolve database path");
    assert!(
        resolved.starts_with(dir.path()),
        "database path {resolved:?} is not under the XDG data home {dir:?}"
    );
    assert!(resolved.ends_with("oct/oct.sqlite3"), "{resolved:?}");

    let cwd = std::env::current_dir().expect("current dir");
    assert!(!resolved.starts_with(&cwd), "database path must not be in the cwd");
}

// --- S6: versioned, forward-only, idempotent migrations ---------------------------------------

#[test]
fn a_fresh_database_is_migrated_and_reopening_it_changes_nothing() {
    let (_dir, path) = temp_db();
    assert!(!path.exists(), "the database file must not exist yet");

    {
        let store = Store::open(&path).expect("open a fresh database");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );
    }
    assert!(path.exists(), "opening must create the file");

    let before = std::fs::read(&path).expect("read database bytes");

    {
        let store = Store::open(&path).expect("reopen a current database");
        assert_eq!(
            store.schema_version().expect("schema version"),
            Store::CURRENT_SCHEMA_VERSION
        );
    }

    let after = std::fs::read(&path).expect("read database bytes");
    assert_eq!(before, after, "reopening a current database must change nothing");
}

// --- S9: settings, the single source the interface and the tray share -------------------------

#[test]
fn a_setting_round_trips_and_an_absent_key_reads_as_none() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    // Never chosen and chosen as nothing are the same honest answer: `None`, not an empty string.
    assert_eq!(store.get_setting("language").expect("read"), None);

    store.set_setting("language", "es").expect("write");
    assert_eq!(
        store.get_setting("language").expect("read"),
        Some("es".to_string())
    );

    // Writing the same key again replaces the value in place rather than adding a second row.
    store.set_setting("language", "en").expect("overwrite");
    assert_eq!(
        store.get_setting("language").expect("read"),
        Some("en".to_string())
    );
}

#[test]
fn reopening_a_current_database_changes_nothing() {
    let (_dir, path) = temp_db();
    {
        let store = Store::open(&path).expect("migrate a fresh database");
        assert_eq!(
            store.schema_version().expect("version"),
            Store::CURRENT_SCHEMA_VERSION
        );
        store.set_setting("language", "es").expect("write a setting");
    }
    let before = std::fs::read(&path).expect("read the database bytes");

    {
        let store = Store::open(&path).expect("reopen a current database");
        assert_eq!(
            store.schema_version().expect("version"),
            Store::CURRENT_SCHEMA_VERSION
        );
        assert_eq!(
            store.get_setting("language").expect("read"),
            Some("es".to_string()),
            "the setting must survive the reopen"
        );
    }

    let after = std::fs::read(&path).expect("read the database bytes");
    assert_eq!(
        before, after,
        "reopening a current database must not rewrite it"
    );
}

#[test]
fn a_database_from_a_newer_build_is_refused_rather_than_downgraded() {
    let (_dir, path) = temp_db();
    {
        let store = Store::open(&path).expect("open");
        assert_eq!(store.schema_version().expect("version"), Store::CURRENT_SCHEMA_VERSION);
    }

    // Simulate a future build having written a higher version, then reopen with this build.
    {
        let connection = rusqlite::Connection::open(&path).expect("open raw");
        connection
            .pragma_update(None, "user_version", Store::CURRENT_SCHEMA_VERSION + 1)
            .expect("bump version");
    }

    let error = Store::open(&path).expect_err("a newer schema must be refused");
    assert!(matches!(error, OctError::Storage(_)), "unexpected: {error:?}");
}

#[test]
fn a_step_that_fails_on_the_way_to_v4_leaves_the_database_at_v3_untouched() {
    // Every migration step is one transaction, so a step that fails must leave the recorded version
    // and every existing row exactly as they were. The success path cannot show that: only a step
    // that actually fails can tell one transaction from "do the work, then stamp the version".
    //
    // The failure needs no seam and no mock. A database is built with real rows and the store's own
    // current schema — so `progress` already has `read_at` — and then only its version stamp is
    // rewound to 3. Reopening it runs the V4 step, `ALTER TABLE progress ADD COLUMN read_at`, against
    // a table that already has that column, which SQLite refuses with "duplicate column name".
    let (_dir, path) = temp_db();

    let document_id;
    let read_at = 1_700_000_000;
    {
        let store = Store::open(&path).expect("build a current database");
        let mut doc = document("/library/at-risk.pdf", FileId::Posix { dev: 3, ino: 17 });
        doc.pages = Some(120);
        doc.title = Some("At risk".to_string());
        document_id = store.upsert_document(&doc).expect("insert the document");
        store
            .record_progress(
                document_id,
                &StoredProgress::new(
                    12,
                    Some(120),
                    ProgressSource::External("evince-gvfs"),
                    Confidence::Exact,
                )
                .with_read_at(Some(read_at)),
            )
            .expect("record the position");
        store.set_setting("language", "es").expect("write a setting");
    }

    // The write time is stamped by `unix_now`, so it is captured before the failure rather than
    // guessed afterwards; the assertion below is that it does not move.
    let recorded_before: i64 = {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        raw.query_row(
            "SELECT updated_at FROM progress WHERE doc_id = ?1",
            rusqlite::params![document_id],
            |row| row.get(0),
        )
        .expect("read the write time")
    };
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        raw.pragma_update(None, "user_version", 3i64)
            .expect("rewind the version stamp to the pre-V4 database shape");
    }

    // 1. The failing step reaches the caller rather than being swallowed or papered over.
    let error = Store::open(&path).expect_err("the V4 step must fail when `read_at` already exists");
    assert!(
        matches!(error, OctError::Storage(_)),
        "a failed migration must surface as a storage error, got {error:?}"
    );

    // 2. The version was not stamped ahead of work that did not happen.
    let raw = rusqlite::Connection::open(&path).expect("open raw");
    let version: i64 = raw
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read the version");
    assert_eq!(
        version, 3,
        "a failed V4 step must leave the database at version 3, not claim version 4"
    );

    // 3. Every pre-existing value is still readable: the document, its position with its provenance
    //    and reading time, the row's own write time, and the setting the previous step recorded.
    let (path_value, title, pages): (String, Option<String>, Option<i64>) = raw
        .query_row(
            "SELECT path, title, pages FROM documents WHERE id = ?1",
            rusqlite::params![document_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the document must survive the failed migration");
    assert_eq!(path_value, "/library/at-risk.pdf");
    assert_eq!(title.as_deref(), Some("At risk"));
    assert_eq!(pages, Some(120));

    let (page, total, source, confidence, stored_read_at, updated_at): (
        i64,
        Option<i64>,
        String,
        String,
        Option<i64>,
        i64,
    ) = raw
        .query_row(
            "SELECT page, total, source, confidence, read_at, updated_at \
             FROM progress WHERE doc_id = ?1",
            rusqlite::params![document_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .expect("the position must survive the failed migration");
    assert_eq!(page, 12);
    assert_eq!(total, Some(120));
    assert_eq!(source, "evince-gvfs");
    assert_eq!(confidence, "exact");
    assert_eq!(stored_read_at, Some(read_at));
    assert_eq!(
        updated_at, recorded_before,
        "the row's write time must not move under a failed migration"
    );

    let language: String = raw
        .query_row("SELECT value FROM settings WHERE key = 'language'", [], |row| {
            row.get(0)
        })
        .expect("the setting must survive the failed migration");
    assert_eq!(language, "es");

    // 4. No half-applied state: the column the failing step wanted to add is still there exactly
    //    once, the table keeps its documented columns, and no table was created or dropped.
    let mut statement = raw
        .prepare("SELECT name FROM pragma_table_info('progress')")
        .expect("prepare");
    let columns: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        columns.iter().filter(|name| name.as_str() == "read_at").count(),
        1,
        "the failed ADD COLUMN must not have left a second `read_at`; columns: {columns:?}"
    );
    for documented in [
        "id",
        "doc_id",
        "page",
        "total",
        "source",
        "confidence",
        "read_at",
        "updated_at",
    ] {
        assert!(
            columns.iter().any(|name| name == documented),
            "the failed step must not have removed `{documented}`; columns: {columns:?}"
        );
    }
    assert_eq!(
        columns.len(),
        8,
        "the progress table must keep its eight columns, found {columns:?}"
    );

    let tables: Vec<String> = raw
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("prepare")
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        tables,
        vec![
            "documents",
            "folders",
            "open_events",
            "progress",
            "settings",
            "suggestions",
            "viewers",
        ],
        "the failed migration must not have created or dropped a table"
    );

    let document_count: i64 = raw
        .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
        .expect("count the documents");
    let progress_count: i64 = raw
        .query_row("SELECT COUNT(*) FROM progress", [], |row| row.get(0))
        .expect("count the positions");
    assert_eq!(
        (document_count, progress_count),
        (1, 1),
        "no row may be lost or duplicated by a failed migration"
    );
}

#[test]
fn a_step_that_fails_on_the_way_to_v5_leaves_the_database_at_v4_untouched() {
    // The same shape as the V4 atomicity test above: a database is built with the store's own current
    // schema — so `documents` already has `user_title` — and then only its version stamp is rewound to
    // 4. Reopening it runs the V5 step, `ALTER TABLE documents ADD COLUMN user_title TEXT`, against a
    // table that already has that column, which SQLite refuses with "duplicate column name". A step
    // stamped before its work would leave the database claiming version 5 with work that did not
    // happen; the transaction must leave the version at 4 and every row untouched.
    let (_dir, path) = temp_db();

    let document_id;
    {
        let store = Store::open(&path).expect("build a current database");
        let mut doc = document("/library/at-risk.pdf", FileId::Posix { dev: 3, ino: 18 });
        doc.pages = Some(120);
        doc.title = Some("The document's own title".to_string());
        document_id = store.upsert_document(&doc).expect("insert the document");
        store
            .set_user_title(document_id, Some("The reader's title"))
            .expect("name the document");
        store
            .record_progress(
                document_id,
                &StoredProgress::new(
                    12,
                    Some(120),
                    ProgressSource::External("evince-gvfs"),
                    Confidence::Exact,
                )
                .with_read_at(Some(1_700_000_000)),
            )
            .expect("record the position");
        store.set_setting("language", "es").expect("write a setting");
    }
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        raw.pragma_update(None, "user_version", 4i64)
            .expect("rewind the version stamp to the pre-V5 database shape");
    }

    // 1. The failing step reaches the caller rather than being swallowed or papered over.
    let error =
        Store::open(&path).expect_err("the V5 step must fail when `user_title` already exists");
    assert!(
        matches!(error, OctError::Storage(_)),
        "a failed migration must surface as a storage error, got {error:?}"
    );

    // 2. The version was not stamped ahead of work that did not happen.
    let raw = rusqlite::Connection::open(&path).expect("open raw");
    let version: i64 = raw
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read the version");
    assert_eq!(
        version, 4,
        "a failed V5 step must leave the database at version 4, not claim version 5"
    );

    // 3. Every pre-existing value is still readable: both titles, the page count, the position with
    //    its reading time, and the setting the previous steps recorded.
    let (path_value, own_title, user_title, pages): (
        String,
        Option<String>,
        Option<String>,
        Option<i64>,
    ) = raw
        .query_row(
            "SELECT path, title, user_title, pages FROM documents WHERE id = ?1",
            rusqlite::params![document_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("the document must survive the failed migration");
    assert_eq!(path_value, "/library/at-risk.pdf");
    assert_eq!(own_title.as_deref(), Some("The document's own title"));
    assert_eq!(user_title.as_deref(), Some("The reader's title"));
    assert_eq!(pages, Some(120));

    let (page, source, stored_read_at): (i64, String, Option<i64>) = raw
        .query_row(
            "SELECT page, source, read_at FROM progress WHERE doc_id = ?1",
            rusqlite::params![document_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the position must survive the failed migration");
    assert_eq!(page, 12);
    assert_eq!(source, "evince-gvfs");
    assert_eq!(stored_read_at, Some(1_700_000_000));

    let language: String = raw
        .query_row("SELECT value FROM settings WHERE key = 'language'", [], |row| {
            row.get(0)
        })
        .expect("the setting must survive the failed migration");
    assert_eq!(language, "es");

    // 4. No half-applied state: the column the failing step wanted to add is still there exactly
    //    once, the table keeps its documented columns, and no table was created or dropped.
    let mut statement = raw
        .prepare("SELECT name FROM pragma_table_info('documents')")
        .expect("prepare");
    let columns: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        columns.iter().filter(|name| name.as_str() == "user_title").count(),
        1,
        "the failed ADD COLUMN must not have left a second `user_title`; columns: {columns:?}"
    );
    for documented in [
        "id",
        "path",
        "path_normalized",
        "uri",
        "file_id_kind",
        "volume_id",
        "file_index",
        "content_hash",
        "size",
        "mtime",
        "title",
        "author",
        "pages",
        "pages_source",
        "discovered_via",
        "platform",
        "first_seen_at",
        "last_opened_at",
        "status",
        "user_title",
        "cover_page",
        "cover_image",
    ] {
        assert!(
            columns.iter().any(|name| name == documented),
            "the failed step must not have removed `{documented}`; columns: {columns:?}"
        );
    }
    assert_eq!(
        columns.len(),
        22,
        "the documents table must keep its twenty-two columns, found {columns:?}"
    );

    let tables: Vec<String> = raw
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("prepare")
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        tables,
        vec![
            "documents",
            "folders",
            "open_events",
            "progress",
            "settings",
            "suggestions",
            "viewers",
        ],
        "the failed migration must not have created or dropped a table"
    );

    let document_count: i64 = raw
        .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
        .expect("count the documents");
    let progress_count: i64 = raw
        .query_row("SELECT COUNT(*) FROM progress", [], |row| row.get(0))
        .expect("count the positions");
    assert_eq!(
        (document_count, progress_count),
        (1, 1),
        "no row may be lost or duplicated by a failed migration"
    );
}

#[test]
fn a_step_that_fails_on_the_way_to_v6_leaves_the_database_at_v5_untouched() {
    // The same shape as the V4 and V5 atomicity tests, one version later. A database is built with the
    // store's own current schema — so `documents` already has `cover_page` — and then only its version
    // stamp is rewound to 5. Reopening it runs the V6 step, `ALTER TABLE documents ADD COLUMN
    // cover_page INTEGER`, against a table that already has that column, which SQLite refuses with
    // "duplicate column name". A step stamped before its work would leave the database claiming version
    // 6 with one column added and the other not; the transaction must leave the version at 5 and every
    // row untouched.
    let (_dir, path) = temp_db();

    let document_id;
    {
        let store = Store::open(&path).expect("build a current database");
        let mut doc = document("/library/at-risk.pdf", FileId::Posix { dev: 3, ino: 19 });
        doc.pages = Some(120);
        doc.title = Some("The document's own title".to_string());
        document_id = store.upsert_document(&doc).expect("insert the document");
        store
            .set_user_title(document_id, Some("The reader's title"))
            .expect("name the document");
        store
            .set_cover_page(document_id, Some(7))
            .expect("choose a cover page");
        store
            .set_cover_image(document_id, Some("9-cover.png"))
            .expect("choose a cover image");
        store
            .record_progress(
                document_id,
                &StoredProgress::new(
                    12,
                    Some(120),
                    ProgressSource::External("evince-gvfs"),
                    Confidence::Exact,
                )
                .with_read_at(Some(1_700_000_000)),
            )
            .expect("record the position");
        store.set_setting("language", "es").expect("write a setting");
    }
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        raw.pragma_update(None, "user_version", 5i64)
            .expect("rewind the version stamp to the pre-V6 database shape");
    }

    // 1. The failing step reaches the caller rather than being swallowed or papered over.
    let error =
        Store::open(&path).expect_err("the V6 step must fail when `cover_page` already exists");
    assert!(
        matches!(error, OctError::Storage(_)),
        "a failed migration must surface as a storage error, got {error:?}"
    );

    // 2. The version was not stamped ahead of work that did not happen.
    let raw = rusqlite::Connection::open(&path).expect("open raw");
    let version: i64 = raw
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read the version");
    assert_eq!(
        version, 5,
        "a failed V6 step must leave the database at version 5, not claim version 6"
    );

    // 3. Every pre-existing value is still readable: both titles, the cover choice, the page count, the
    //    position with its reading time, and the setting the previous steps recorded.
    let (path_value, own_title, user_title, pages): (
        String,
        Option<String>,
        Option<String>,
        Option<i64>,
    ) = raw
        .query_row(
            "SELECT path, title, user_title, pages FROM documents WHERE id = ?1",
            rusqlite::params![document_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("the document must survive the failed migration");
    let (cover_page, cover_image): (Option<i64>, Option<String>) = raw
        .query_row(
            "SELECT cover_page, cover_image FROM documents WHERE id = ?1",
            rusqlite::params![document_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the cover choice must survive the failed migration");
    assert_eq!(path_value, "/library/at-risk.pdf");
    assert_eq!(own_title.as_deref(), Some("The document's own title"));
    assert_eq!(user_title.as_deref(), Some("The reader's title"));
    assert_eq!(cover_page, Some(7));
    assert_eq!(cover_image.as_deref(), Some("9-cover.png"));
    assert_eq!(pages, Some(120));

    let (page, source, stored_read_at): (i64, String, Option<i64>) = raw
        .query_row(
            "SELECT page, source, read_at FROM progress WHERE doc_id = ?1",
            rusqlite::params![document_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the position must survive the failed migration");
    assert_eq!(page, 12);
    assert_eq!(source, "evince-gvfs");
    assert_eq!(stored_read_at, Some(1_700_000_000));

    let language: String = raw
        .query_row("SELECT value FROM settings WHERE key = 'language'", [], |row| {
            row.get(0)
        })
        .expect("the setting must survive the failed migration");
    assert_eq!(language, "es");

    // 4. No half-applied state: the column the failing step wanted to add is still there exactly once,
    //    the other V6 column was not left behind twice, the table keeps its documented columns, and no
    //    table was created or dropped.
    let mut statement = raw
        .prepare("SELECT name FROM pragma_table_info('documents')")
        .expect("prepare");
    let columns: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        columns.iter().filter(|name| name.as_str() == "cover_page").count(),
        1,
        "the failed ADD COLUMN must not have left a second `cover_page`; columns: {columns:?}"
    );
    assert_eq!(
        columns.iter().filter(|name| name.as_str() == "cover_image").count(),
        1,
        "the failed batch must not have added `cover_image` twice; columns: {columns:?}"
    );
    assert_eq!(
        columns.len(),
        22,
        "the documents table must keep its twenty-two columns, found {columns:?}"
    );

    let tables: Vec<String> = raw
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("prepare")
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        tables,
        vec![
            "documents",
            "folders",
            "open_events",
            "progress",
            "settings",
            "suggestions",
            "viewers",
        ],
        "the failed migration must not have created or dropped a table"
    );

    let document_count: i64 = raw
        .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
        .expect("count the documents");
    let progress_count: i64 = raw
        .query_row("SELECT COUNT(*) FROM progress", [], |row| row.get(0))
        .expect("count the positions");
    assert_eq!(
        (document_count, progress_count),
        (1, 1),
        "no row may be lost or duplicated by a failed migration"
    );
}

// --- R32: the reader's cover choice -------------------------------------------------------------

#[test]
fn the_readers_cover_choice_round_trips_and_survives_a_restart() {
    let (_dir, path) = temp_db();
    let document_id;
    {
        let store = Store::open(&path).expect("open");
        document_id = store
            .upsert_document(&document("/library/read.pdf", FileId::Posix { dev: 7, ino: 7 }))
            .expect("insert");

        // Neither choice exists until the reader makes one: the first page is the rule's default and
        // is not a stored value.
        let entries = store.library().expect("list");
        assert_eq!(entries[0].document.document.cover_page, None);
        assert_eq!(entries[0].document.document.cover_image, None);

        store.set_cover_page(document_id, Some(12)).expect("choose a page");
        store
            .set_cover_image(document_id, Some("7-cover.jpg"))
            .expect("choose an image");
    }

    // The choice is a stored fact, not something a running process remembers: a fresh open reads the
    // very page and image the reader picked.
    let store = Store::open(&path).expect("reopen");
    let entries = store.library().expect("list after restart");
    let document = &entries[0].document.document;
    assert_eq!(document.cover_page, Some(12));
    assert_eq!(document.cover_image.as_deref(), Some("7-cover.jpg"));
    // And the one rule reads them back in the right order: the image the reader supplied wins over the
    // page they chose.
    assert_eq!(
        document.cover_choice(),
        oct_core::domain::CoverChoice::Image("7-cover.jpg".to_string())
    );

    // Removing the image falls back to the page still on record, not to the default.
    store
        .set_cover_image(document_id, None)
        .expect("remove the image");
    let entries = store.library().expect("list after removal");
    assert_eq!(
        entries[0].document.document.cover_choice(),
        oct_core::domain::CoverChoice::Page(12)
    );
}

#[test]
fn a_rescan_does_not_clear_the_readers_cover_choice() {
    // The same rule the reader's title already has (R31): the scanner refreshes the document's own
    // metadata and must never be able to erase a choice a person made. `upsert_document` simply does
    // not write the cover columns, so re-scanning a document that gained a page count cannot reset it.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let identity = FileId::Posix { dev: 8, ino: 8 };
    let document_id = store
        .upsert_document(&document("/library/read.pdf", identity.clone()))
        .expect("insert");
    store.set_cover_page(document_id, Some(3)).expect("choose a page");
    store
        .set_cover_image(document_id, Some("8-cover.png"))
        .expect("choose an image");

    // The rescan: the same document, this time with a page count and a title, exactly what the scanner
    // supplies. The identity matches, so it updates the row in place.
    let mut rescanned = document("/library/read.pdf", identity);
    rescanned.pages = Some(240);
    rescanned.title = Some("Counted at last".to_string());
    let same_id = store.upsert_document(&rescanned).expect("rescan");
    assert_eq!(same_id, document_id, "the rescan updates the same row");

    let entries = store.library().expect("list");
    let document = &entries[0].document.document;
    assert_eq!(document.pages, Some(240), "the scan's own fact still lands");
    assert_eq!(document.cover_page, Some(3), "a rescan must not clear the page");
    assert_eq!(
        document.cover_image.as_deref(),
        Some("8-cover.png"),
        "a rescan must not clear the image"
    );
}

#[test]
fn a_blank_or_pathlike_cover_image_name_is_refused_as_an_invalid_argument() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let id = store
        .upsert_document(&document("/library/read.pdf", FileId::Posix { dev: 9, ino: 9 }))
        .expect("insert");

    for refused in ["", "   ", "/etc/passwd", "a/../b.png", "dir\\x.png"] {
        let result = store.set_cover_image(id, Some(refused));
        assert!(
            matches!(result, Err(OctError::InvalidInput { field, .. }) if field == "cover image"),
            "`{refused}` must be an invalid argument, got {result:?}"
        );
    }

    // Nothing was written by any refusal: `NULL` still means "no image".
    let entries = store.library().expect("list");
    assert_eq!(entries[0].document.document.cover_image, None);
}

// --- S7: paths are opaque UTF-8 and are not Unicode-normalised --------------------------------

#[test]
fn paths_survive_byte_for_byte_and_nfc_is_not_merged_with_nfd() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    let decorated = "/library/Comparación de Métodos de Strings entre JavaScript y Python.pdf";
    let nfc = "/library/Comparaci\u{00F3}n.pdf";
    let nfd = "/library/Comparacio\u{0301}n.pdf";
    assert_ne!(nfc, nfd, "the test fixture must actually differ in bytes");

    let entries = [
        (FileId::ContentHash("a".to_string()), decorated),
        (FileId::ContentHash("b".to_string()), nfc),
        (FileId::ContentHash("c".to_string()), nfd),
    ];

    for (identity, path_str) in &entries {
        store
            .upsert_document(&document(path_str, identity.clone()))
            .expect("insert document");
    }

    for (identity, path_str) in &entries {
        let found = store
            .find_document_by_identity(identity)
            .expect("lookup")
            .expect("present");
        assert_eq!(
            found.document.path.as_os_str().to_str().expect("utf-8"),
            *path_str,
            "path must survive byte for byte"
        );
    }

    // A normalising store would have collapsed the NFC and NFD rows into one.
    let nfc_row = store
        .find_document_by_identity(&FileId::ContentHash("b".to_string()))
        .expect("lookup")
        .expect("nfc present");
    let nfd_row = store
        .find_document_by_identity(&FileId::ContentHash("c".to_string()))
        .expect("lookup")
        .expect("nfd present");
    assert_ne!(nfc_row.document.path, nfd_row.document.path);
}

// --- S8: honest failures ----------------------------------------------------------------------

#[test]
fn opening_something_that_is_not_a_database_is_an_honest_storage_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("not-a-database.sqlite3");
    std::fs::write(&path, b"this is definitely not SQLite").expect("write junk");

    let error = Store::open(&path).expect_err("junk must not open as a database");
    assert!(matches!(error, OctError::Storage(_)), "unexpected: {error:?}");
}

// --- S1, again: the column contract, not just the table names ---------------------------------

#[test]
fn the_schema_has_the_documented_columns_of_every_table() {
    let (_dir, path) = temp_db();
    let _store = Store::open(&path).expect("open");

    // Read through SQLite's public catalog, so this asserts the *spec's* column list rather than
    // whatever the implementation happens to define.
    let connection = rusqlite::Connection::open(&path).expect("open raw");
    let expected: [(&str, &[&str]); 7] = [
        (
            "documents",
            &[
                "id",
                "path",
                "path_normalized",
                "uri",
                "file_id_kind",
                "volume_id",
                "file_index",
                "content_hash",
                "size",
                "mtime",
                "title",
                "author",
                "pages",
                "pages_source",
                "discovered_via",
                "platform",
                "first_seen_at",
                "last_opened_at",
                "status",
                "user_title",
            ],
        ),
        (
            "progress",
            &[
                "id",
                "doc_id",
                "page",
                "total",
                "source",
                "confidence",
                "read_at",
                "updated_at",
            ],
        ),
        (
            "open_events",
            &["id", "doc_id", "pid", "viewer_id", "started_at", "ended_at"],
        ),
        (
            "viewers",
            &["id", "exe", "desktop_id", "bundle_id", "platform", "capability", "detail"],
        ),
        (
            "folders",
            &["id", "path", "recursive", "enabled", "added_at"],
        ),
        (
            "suggestions",
            &["id", "path", "seen_count", "first_seen_at", "decided_at"],
        ),
        ("settings", &["key", "value"]),
    ];

    for (table, columns) in expected {
        let mut statement = connection
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .expect("prepare");
        let actual: Vec<String> = statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .map(|row| row.expect("row"))
            .collect();

        for column in columns {
            assert!(
                actual.iter().any(|name| name == column),
                "table `{table}` is missing the documented column `{column}`; found {actual:?}"
            );
        }
    }
}

// --- S3, again: one row per document, whatever order knowledge arrives in ----------------------

#[test]
fn a_document_that_gains_an_identity_keeps_its_row_and_its_progress() {
    // The scanner may not know a document's identity on the first pass (an unavailable filesystem, a
    // failed hash) and learn it later. That must attach the identity to the existing row, not fork
    // the document into two rows with two reading histories.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    let doc_path = "/library/identity-learned-later.pdf";
    let doc_id = store
        .upsert_document(&Document::new(doc_path))
        .expect("insert without identity");
    store
        .record_progress(
            doc_id,
            &StoredProgress::new(7, Some(80), ProgressSource::Manual, Confidence::Manual),
        )
        .expect("record progress");

    let identity = FileId::Posix { dev: 42, ino: 4242 };
    let again = store
        .upsert_document(&document(doc_path, identity.clone()))
        .expect("upsert with identity");

    assert_eq!(
        again, doc_id,
        "the same document must not gain a second row when its identity becomes known"
    );
    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("the identity must now resolve");
    assert_eq!(found.id, doc_id);
    assert_eq!(
        store
            .progress_for_document(doc_id)
            .expect("read progress")
            .len(),
        1,
        "the reading history must follow the document, not be stranded on the old row"
    );
}

#[test]
fn a_path_that_now_holds_a_different_file_gets_its_own_row() {
    // Overwriting a file changes its identity. The older row keeps its own progress: attributing it
    // to the replacement would be the silent lie the provenance columns exist to prevent.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    let doc_path = "/library/overwritten.pdf";
    let original = FileId::Posix { dev: 1, ino: 1 };
    let replacement = FileId::Posix { dev: 2, ino: 2 };

    let original_id = store
        .upsert_document(&document(doc_path, original.clone()))
        .expect("insert the original");
    store
        .record_progress(
            original_id,
            &StoredProgress::new(50, Some(80), ProgressSource::Native, Confidence::Exact),
        )
        .expect("record progress");

    let replacement_id = store
        .upsert_document(&document(doc_path, replacement.clone()))
        .expect("insert the replacement");
    assert_ne!(replacement_id, original_id);

    // Each identity still resolves to its own row and its own progress.
    assert_eq!(
        store
            .find_document_by_identity(&original)
            .expect("lookup")
            .expect("original present")
            .id,
        original_id
    );
    assert_eq!(
        store
            .find_document_by_identity(&replacement)
            .expect("lookup")
            .expect("replacement present")
            .id,
        replacement_id
    );
    assert_eq!(
        store
            .progress_for_document(original_id)
            .expect("read progress")
            .len(),
        1
    );
    assert!(store
        .progress_for_document(replacement_id)
        .expect("read progress")
        .is_empty());

    // A caller that still has no identity resolves by path to the newest row for that path.
    let by_path = store
        .upsert_document(&Document::new(doc_path))
        .expect("upsert by path alone");
    assert_eq!(
        by_path, replacement_id,
        "a path lookup must resolve to the newest row, not the superseded one"
    );
}

#[test]
fn an_upsert_never_erases_known_values_with_unknown_ones() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    let doc_path = "/library/known-values.pdf";
    let identity = FileId::ContentHash("sha256:abc".to_string());
    let mut known = document(doc_path, identity.clone());
    known.pages = Some(12);
    known.title = Some("A known title".to_string());
    known.author = Some("A known author".to_string());
    store.upsert_document(&known).expect("insert what we know");

    // A later caller that knows less must not destroy what the earlier one established.
    store
        .upsert_document(&Document::new(doc_path))
        .expect("upsert knowing nothing");

    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("the document must still be findable by its identity");
    assert_eq!(found.document.identity, Some(identity));
    assert_eq!(found.document.pages, Some(12));
    assert_eq!(found.document.title.as_deref(), Some("A known title"));
    assert_eq!(found.document.author.as_deref(), Some("A known author"));
}

// --- R31: the reader's title is a separate fact ------------------------------------------------

#[test]
fn the_readers_title_is_a_separate_fact_and_is_the_one_displayed() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let identity = FileId::Posix { dev: 7, ino: 11 };
    let mut doc = document("/library/named.pdf", identity.clone());
    doc.title = Some("The document's own title".to_string());
    let id = store.upsert_document(&doc).expect("insert the document");

    // Before anyone names it, there is only the document's own title and `user_title` is `None`.
    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    assert_eq!(found.document.user_title, None);
    assert_eq!(found.document.display_title(), "The document's own title");

    store
        .set_user_title(id, Some("What the reader called it"))
        .expect("name the document");

    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    // Both facts are stored, and the reader's is the one displayed.
    assert_eq!(
        found.document.title.as_deref(),
        Some("The document's own title"),
        "the reader's title must not overwrite the document's own"
    );
    assert_eq!(
        found.document.user_title.as_deref(),
        Some("What the reader called it")
    );
    assert_eq!(found.document.display_title(), "What the reader called it");

    // Clearing it restores the document's own title, which never left.
    store
        .set_user_title(id, None)
        .expect("restore the document's own title");
    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    assert_eq!(found.document.user_title, None);
    assert_eq!(found.document.display_title(), "The document's own title");
}

#[test]
fn a_blank_title_is_refused_as_an_invalid_argument_and_nothing_is_written() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let identity = FileId::Posix { dev: 7, ino: 12 };
    let id = store
        .upsert_document(&document("/library/named.pdf", identity.clone()))
        .expect("insert the document");

    for blank in ["", "   ", "\t\n"] {
        let result = store.set_user_title(id, Some(blank));
        match result {
            Err(OctError::InvalidInput {
                field,
                value,
                expected,
            }) => {
                assert_eq!(field, "title");
                assert_eq!(value, blank);
                assert!(expected.contains("non-empty"), "expected: {expected}");
            }
            other => panic!("a blank title must be an invalid argument, got {other:?}"),
        }
    }

    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    assert_eq!(
        found.document.user_title, None,
        "a refused title must never reach the column"
    );
}

#[test]
fn an_upsert_never_erases_the_readers_title() {
    // R31's load-bearing rule: the scanner's upsert (and every rescan after it) refreshes the
    // document's own metadata but must not touch the reader's title. A rescan happening after a
    // person named a document must not silently rename it.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let identity = FileId::Posix { dev: 8, ino: 13 };
    let id = store
        .upsert_document(&document("/library/named.pdf", identity.clone()))
        .expect("insert the document");
    store
        .set_user_title(id, Some("What the reader called it"))
        .expect("name the document");

    // The rescan knows the same identity and fresher metadata, and nothing about the reader's title.
    let mut rescan = document("/library/named.pdf", identity.clone());
    rescan.title = Some("The document's own title, as the file embeds it".to_string());
    rescan.pages = Some(120);
    store.upsert_document(&rescan).expect("rescan the document");

    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    assert_eq!(
        found.document.user_title.as_deref(),
        Some("What the reader called it"),
        "a rescan must never rename a document a person named"
    );
    assert_eq!(
        found.document.display_title(),
        "What the reader called it",
        "the reader's title must still be the one displayed after a rescan"
    );
    // The rescan's fresh metadata still landed: the rule is "do not touch the reader's title", not
    // "ignore the rescan".
    assert_eq!(found.document.pages, Some(120));
}

#[test]
fn the_schema_refuses_a_second_row_for_a_known_identity() {
    // "One row per known identity" is a schema invariant, not only a rule in `upsert_document`:
    // a second writer that loses the race must fail honestly rather than silently forking a
    // document into two rows. Identity-less rows stay unconstrained, because they are resolved by
    // path and a path may legitimately hold more than one row.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    store
        .upsert_document(&document(
            "/library/unique.pdf",
            FileId::Posix { dev: 7, ino: 7 },
        ))
        .expect("insert the first row");

    let connection = rusqlite::Connection::open(&path).expect("open raw");
    let duplicate = connection.execute(
        "INSERT INTO documents \
            (path, path_normalized, uri, file_id_kind, volume_id, file_index, first_seen_at) \
         VALUES (?1, ?1, 'file:///library/other.pdf', 'posix', '7', '7', 0)",
        rusqlite::params!["/library/other.pdf"],
    );
    assert!(
        duplicate.is_err(),
        "the schema must reject a duplicate known identity, got {duplicate:?}"
    );

    // The same identity on a *different* path is not refused when it goes through the store: it is
    // the documented move/rename case, which updates the existing row in place.
    let moved = store
        .upsert_document(&document(
            "/library/moved.pdf",
            FileId::Posix { dev: 7, ino: 7 },
        ))
        .expect("a rename must update, not fail");
    let rows = store
        .find_document_by_identity(&FileId::Posix { dev: 7, ino: 7 })
        .expect("lookup")
        .expect("present");
    assert_eq!(rows.id, moved);
    assert_eq!(rows.document.path, PathBuf::from("/library/moved.pdf"));
}

// --- T6: the folders API ---------------------------------------------------------------------

#[test]
fn recording_the_same_folder_twice_keeps_one_row_and_the_first_added_time() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    let first = store
        .record_folder("/library", true)
        .expect("record the folder");
    let second = store
        .record_folder("/library", true)
        .expect("record the same folder again");
    assert_eq!(first, second, "re-recording must update, not insert");
    assert_eq!(store.folders().expect("list folders").len(), 1);

    let recorded = store.folders().expect("list folders").pop().expect("one folder");
    assert_eq!(recorded.path, PathBuf::from("/library"));
    assert!(recorded.recursive);
    assert!(recorded.enabled, "the scanner does not touch the user's enabled flag");

    // A different recursion flag updates the existing row rather than forking it.
    store
        .record_folder("/library", false)
        .expect("update recursion");
    let folders = store.folders().expect("list folders");
    assert_eq!(folders.len(), 1);
    assert!(!folders[0].recursive);
    assert_eq!(folders[0].id, first);
}

#[test]
fn document_count_tracks_the_documents_table() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    assert_eq!(store.document_count().expect("count"), 0);

    store
        .upsert_document(&document(
            "/library/one.pdf",
            FileId::Posix { dev: 1, ino: 1 },
        ))
        .expect("insert");
    store
        .upsert_document(&document(
            "/library/two.pdf",
            FileId::Posix { dev: 1, ino: 2 },
        ))
        .expect("insert");
    assert_eq!(store.document_count().expect("count"), 2);

    // An update is not a new row.
    store
        .upsert_document(&document(
            "/library/one.pdf",
            FileId::Posix { dev: 1, ino: 1 },
        ))
        .expect("update");
    assert_eq!(store.document_count().expect("count"), 2);
}

#[test]
fn the_schema_refuses_a_second_row_for_the_same_folder() {
    // "A folder is recorded once" is a schema invariant, not only a rule in `record_folder`: a
    // second writer that loses the race must fail honestly rather than making the watcher walk the
    // same folder twice.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let folder = "/library/watched";
    let first = store.record_folder(folder, true).expect("record once");
    let again = store.record_folder(folder, true).expect("record twice");
    assert_eq!(first, again, "recording the same path must reuse the row");
    assert_eq!(store.folders().expect("list").len(), 1);

    let connection = rusqlite::Connection::open(&path).expect("open raw");
    let duplicate = connection.execute(
        "INSERT INTO folders (path, recursive, enabled, added_at) VALUES (?1, 1, 1, 0)",
        rusqlite::params![folder],
    );
    assert!(
        duplicate.is_err(),
        "the schema must reject a duplicate folder path, got {duplicate:?}"
    );

    // Re-recording with a different `recursive` updates in place and never touches `enabled`,
    // which is the user's setting rather than the scanner's.
    let updated = store.record_folder(folder, false).expect("update");
    assert_eq!(updated, first);
    let folders = store.folders().expect("list");
    assert_eq!(folders.len(), 1);
    assert!(!folders[0].recursive);
    assert!(folders[0].enabled, "the user's enabled flag must survive");
}

// --- R27 (reverses R20): removing a watch takes its documents with it -------------------------

#[test]
fn forgetting_a_folder_takes_the_documents_and_positions_it_covers_with_it() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let keep = store.record_folder("/library/keep", true).expect("record");
    let doomed = store.record_folder("/library/doomed", true).expect("record");

    let doc_id = store
        .upsert_document(&document(
            "/library/doomed/Cosmos.pdf",
            FileId::Posix { dev: 3, ino: 7 },
        ))
        .expect("insert the document");
    store
        .record_progress(
            doc_id,
            &StoredProgress::new(
                72,
                Some(575),
                ProgressSource::External("evince-gvfs"),
                Confidence::Exact,
            ),
        )
        .expect("record its position");

    let removal = store
        .forget_folder(doomed)
        .expect("forget the watch")
        .expect("the row was there, so the removal must report what it took");

    // The plan names the document by the stored path, and nothing was left behind.
    assert_eq!(removal.folder.id, doomed);
    assert_eq!(
        removal.documents,
        vec!["/library/doomed/Cosmos.pdf".to_string()]
    );
    assert!(removal.kept.is_empty(), "no other watch covers this document");

    let folders = store.folders().expect("list folders");
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, keep, "only the named watch goes");

    // The counts, not merely the absence of a row: the user asked for the watch's documents to
    // leave the library, so the document, its position and the listing entry all go with it.
    assert_eq!(store.document_count().expect("count"), 0, "the document goes");
    assert!(
        store
            .progress_for_document(doc_id)
            .expect("read progress")
            .is_empty(),
        "the reading history goes with the document it describes"
    );
    assert!(
        store.library().expect("list the library").is_empty(),
        "the library no longer lists it"
    );
}

#[test]
fn removing_the_outer_watch_keeps_documents_the_inner_watch_still_covers() {
    // The guard R27 exists for: the user's own store holds a nested pair, and removing the outer
    // watch must not empty the folder that is still watched.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let outer = store.record_folder("/lib", true).expect("record the outer watch");
    let inner = store
        .record_folder("/lib/inner", true)
        .expect("record the inner watch");

    store
        .upsert_document(&document("/lib/direct.pdf", FileId::Posix { dev: 1, ino: 1 }))
        .expect("insert a document directly under the outer watch");
    let nested = store
        .upsert_document(&document(
            "/lib/inner/nested.pdf",
            FileId::Posix { dev: 1, ino: 2 },
        ))
        .expect("insert a document under the inner watch");

    let removal = store
        .forget_folder(outer)
        .expect("forget the outer watch")
        .expect("the row was there");

    assert_eq!(removal.documents, vec!["/lib/direct.pdf".to_string()]);
    assert_eq!(removal.kept, vec!["/lib/inner/nested.pdf".to_string()]);

    assert_eq!(store.document_count().expect("count"), 1);
    let library = store.library().expect("list the library");
    assert_eq!(library.len(), 1);
    assert_eq!(library[0].document.id, nested);

    let folders = store.folders().expect("list folders");
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, inner, "only the named watch goes");
}

#[test]
fn removing_the_inner_watch_keeps_its_documents_because_the_outer_watch_still_covers_them() {
    // The guard is "another remaining watch still covers it", and the outer watch is exactly that:
    // a document inside `/lib/inner` is also inside `/lib`. So removing the inner watch removes
    // nothing — the outer watch still lists those documents, and a later scan would bring them back
    // anyway. (The work order phrased this direction as "removes only its own"; the frozen
    // contract's `covering_folder` guard keeps them, and this test pins that contract. See risks.)
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let outer = store.record_folder("/lib", true).expect("record the outer watch");
    let inner = store
        .record_folder("/lib/inner", true)
        .expect("record the inner watch");

    let nested = store
        .upsert_document(&document(
            "/lib/inner/nested.pdf",
            FileId::Posix { dev: 2, ino: 1 },
        ))
        .expect("insert");

    let removal = store
        .forget_folder(inner)
        .expect("forget the inner watch")
        .expect("the row was there");

    assert!(
        removal.documents.is_empty(),
        "the outer watch still covers this document, so it does not leave"
    );
    assert_eq!(removal.kept, vec!["/lib/inner/nested.pdf".to_string()]);

    assert_eq!(store.document_count().expect("count"), 1);
    assert_eq!(
        store.library().expect("list the library")[0].document.id,
        nested
    );

    let folders = store.folders().expect("list folders");
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, outer);
}

#[test]
fn a_document_outside_every_watch_is_never_touched() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let watched = store.record_folder("/lib/watched", true).expect("record");

    let inside = store
        .upsert_document(&document(
            "/lib/watched/gone.pdf",
            FileId::Posix { dev: 3, ino: 1 },
        ))
        .expect("insert a document the watch covers");
    let outside = store
        .upsert_document(&document(
            "/lib/elsewhere/kept.pdf",
            FileId::Posix { dev: 3, ino: 2 },
        ))
        .expect("insert a document no watch covers");

    store
        .forget_folder(watched)
        .expect("forget the watch")
        .expect("the row was there");

    assert_eq!(store.document_count().expect("count"), 1);
    let library = store.library().expect("list the library");
    assert_eq!(library.len(), 1);
    assert_eq!(
        library[0].document.id, outside,
        "a document outside every watch stays"
    );
    assert!(
        store
            .progress_for_document(inside)
            .expect("progress")
            .is_empty()
    );
}

#[test]
fn a_document_in_a_sibling_folder_sharing_a_string_prefix_is_never_touched() {
    // `/lib/pdfs-old` shares the whole string prefix `/lib/pdfs` and is an unrelated folder. The
    // "inside" test compares path components, so the sibling stays where a string prefix would
    // take it.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let watched = store.record_folder("/lib/pdfs", true).expect("record");

    let inside = store
        .upsert_document(&document(
            "/lib/pdfs/inside.pdf",
            FileId::Posix { dev: 4, ino: 1 },
        ))
        .expect("insert");
    let sibling = store
        .upsert_document(&document(
            "/lib/pdfs-old/sibling.pdf",
            FileId::Posix { dev: 4, ino: 2 },
        ))
        .expect("insert");

    let removal = store
        .forget_folder(watched)
        .expect("forget the watch")
        .expect("the row was there");

    assert_eq!(removal.documents, vec!["/lib/pdfs/inside.pdf".to_string()]);
    assert!(removal.kept.is_empty());

    assert_eq!(store.document_count().expect("count"), 1);
    assert_eq!(
        store.library().expect("list the library")[0].document.id,
        sibling
    );
    assert!(
        store
            .progress_for_document(inside)
            .expect("progress")
            .is_empty()
    );
}

#[test]
fn the_removal_names_the_documents_that_go_and_the_ones_that_stay_sorted_by_path() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let outer = store.record_folder("/lib", true).expect("record the outer watch");
    store
        .record_folder("/lib/inner", true)
        .expect("record the inner watch");

    // Inserted out of path order, so the sorted answer cannot be an accident of insertion order.
    for (name, dev, ino) in [
        ("/lib/inner/z-stays.pdf", 7, 1),
        ("/lib/b-goes.pdf", 7, 2),
        ("/lib/inner/a-stays.pdf", 7, 3),
        ("/lib/a-goes.pdf", 7, 4),
    ] {
        store
            .upsert_document(&document(name, FileId::Posix { dev, ino }))
            .expect("insert");
    }

    let removal = store
        .folder_removal(outer)
        .expect("plan")
        .expect("the watch is known");

    assert_eq!(
        removal.documents,
        vec!["/lib/a-goes.pdf".to_string(), "/lib/b-goes.pdf".to_string()],
        "`documents` is exactly what leaves, sorted by path"
    );
    assert_eq!(
        removal.kept,
        vec![
            "/lib/inner/a-stays.pdf".to_string(),
            "/lib/inner/z-stays.pdf".to_string(),
        ],
        "`kept` is exactly what another watch still covers, sorted by path"
    );
}

#[test]
fn folder_removal_plans_exactly_what_forget_folder_does_and_changes_nothing() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let watched = store.record_folder("/lib/watched", true).expect("record");
    store
        .record_folder("/lib/kept", true)
        .expect("record a second watch");

    store
        .upsert_document(&document(
            "/lib/watched/gone.pdf",
            FileId::Posix { dev: 5, ino: 1 },
        ))
        .expect("insert");
    store
        .upsert_document(&document("/lib/kept/stays.pdf", FileId::Posix { dev: 5, ino: 2 }))
        .expect("insert");

    // The plan is read-only: two consecutive calls agree, and neither changes the store.
    let folders_before = store.folders().expect("list folders");
    let count_before = store.document_count().expect("count");
    let library_before = store.library().expect("list the library");

    let first = store.folder_removal(watched).expect("plan");
    let second = store.folder_removal(watched).expect("plan again");
    assert_eq!(first, second, "two consecutive plans must agree");
    assert_eq!(store.folders().expect("list folders"), folders_before);
    assert_eq!(store.document_count().expect("count"), count_before);
    assert_eq!(store.library().expect("list the library"), library_before);

    // And the plan is exactly what the removal carries out.
    let planned = first.expect("the watch is known");
    let performed = store
        .forget_folder(watched)
        .expect("forget")
        .expect("the watch was known");
    assert_eq!(planned, performed);

    assert_eq!(store.document_count().expect("count"), count_before - 1);
    let library = store.library().expect("list the library");
    assert_eq!(library.len(), 1);
    assert_eq!(
        library[0].document.document.path,
        PathBuf::from("/lib/kept/stays.pdf")
    );
}

#[test]
fn an_unknown_id_plans_and_removes_nothing() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let watched = store.record_folder("/lib/watched", true).expect("record");
    let doc_id = store
        .upsert_document(&document(
            "/lib/watched/book.pdf",
            FileId::Posix { dev: 6, ino: 1 },
        ))
        .expect("insert");
    store
        .record_progress(
            doc_id,
            &StoredProgress::new(1, Some(2), ProgressSource::Manual, Confidence::Manual),
        )
        .expect("record progress");

    // R27 makes the promise sharper than the earlier `Ok(false)`: an id that names no watch must
    // delete nothing at all, not even by accident.
    assert!(
        store.folder_removal(404).expect("plan").is_none(),
        "an unknown id plans nothing"
    );
    assert!(
        store.forget_folder(404).expect("forget").is_none(),
        "an unknown id removes nothing"
    );

    let folders = store.folders().expect("list folders");
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, watched, "the watch survives");
    assert_eq!(
        store.document_count().expect("count"),
        1,
        "the document survives"
    );
    assert_eq!(
        store.progress_for_document(doc_id).expect("progress").len(),
        1,
        "the position survives"
    );
}

#[test]
fn forgetting_an_id_the_store_never_held_is_none_rather_than_a_failure() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    // A request that is already satisfied is not an error: `None` says nothing was there, and
    // inventing a failure here would make a double click look broken.
    assert!(store
        .forget_folder(404)
        .expect("an unknown id must be answered, not refused")
        .is_none());

    let id = store.record_folder("/library", true).expect("record");
    assert!(store.forget_folder(id).expect("first removal").is_some());
    assert!(
        store.forget_folder(id).expect("second removal").is_none(),
        "removing the same watch twice must report nothing to remove, not panic"
    );
    assert!(store.folders().expect("list").is_empty());
}

// --- R22: coverage is a component question, never a string one --------------------------------

fn watched(id: i64, path: &str) -> WatchedFolder {
    WatchedFolder {
        id,
        path: PathBuf::from(path),
        recursive: true,
        enabled: true,
    }
}

#[test]
fn a_watched_folder_covers_itself_and_its_subfolders_but_not_a_lookalike_sibling() {
    let folders = vec![watched(1, "/library/pdfs")];
    let covering = |candidate: &str| {
        covering_folder(&folders, Path::new(candidate)).map(|folder| folder.id)
    };

    // The trap this test exists for: `/library/pdfs-old` shares the entire character prefix
    // `/library/pdfs` and is an unrelated folder. `str::starts_with` says it is inside; comparing
    // whole path components says it is not.
    assert_eq!(covering("/library/pdfs-old"), None);
    // The same trap in a plainer spelling, so the rule cannot pass by special-casing one shape.
    assert_eq!(covering("/library/pdfs-2024"), None);

    // The answers that must be a refusal.
    assert_eq!(covering("/library/pdfs"), Some(1), "the watched folder itself");
    assert_eq!(covering("/library/pdfs/book.pdf"), Some(1), "a real subfolder");
    assert_eq!(
        covering("/library/pdfs/deep/inside"),
        Some(1),
        "a folder nested deeper"
    );

    // ... and the ones that must not be.
    assert_eq!(covering("/library"), None, "the parent is not inside its child");
    assert_eq!(covering("/library/other"), None, "an unrelated sibling");
    assert_eq!(covering("/"), None, "the filesystem root is not inside anything");
}

#[test]
fn a_trailing_separator_on_the_watched_path_changes_nothing() {
    let folders = vec![watched(1, "/library/pdfs/")];
    let covering = |candidate: &str| {
        covering_folder(&folders, Path::new(candidate)).map(|folder| folder.id)
    };

    assert_eq!(covering("/library/pdfs/book"), Some(1));
    assert_eq!(covering("/library/pdfs"), Some(1));
    assert_eq!(covering("/library/pdfs-old"), None);
}

#[test]
fn the_tightest_covering_folder_is_the_one_that_gets_named() {
    // Two watches can cover one candidate when a folder inside a watched tree was recorded before
    // the rule existed. The refusal names the folder that actually bounds the choice, so the reader
    // is handed the narrowest true answer rather than a technically-correct ancestor.
    let folders = vec![watched(1, "/library"), watched(2, "/library/pdfs")];
    let covering = |candidate: &str| {
        covering_folder(&folders, Path::new(candidate)).map(|folder| folder.id)
    };

    assert_eq!(covering("/library/pdfs/book"), Some(2));
    assert_eq!(covering("/library/other"), Some(1));
}

// --- UI slice: the library listing (S1) ------------------------------------------------------

#[test]
fn the_library_returns_the_latest_progress_with_its_provenance() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let doc_id = store
        .upsert_document(&document(
            "/library/read.pdf",
            FileId::Posix { dev: 1, ino: 10 },
        ))
        .expect("insert");

    // Three rows, deliberately out of page order, so "latest" cannot be confused with "largest".
    let opened = StoredProgress::new(
        3,
        Some(80),
        ProgressSource::External("evince-gvfs"),
        Confidence::Exact,
    );
    let corrected = StoredProgress::new(40, Some(80), ProgressSource::Manual, Confidence::Manual);
    let latest = StoredProgress::new(
        12,
        Some(80),
        ProgressSource::External("evince-gvfs"),
        Confidence::Exact,
    );
    store.record_progress(doc_id, &opened).expect("record");
    store.record_progress(doc_id, &corrected).expect("record");
    store.record_progress(doc_id, &latest).expect("record");

    let library = store.library().expect("list the library");
    assert_eq!(library.len(), 1);
    let entry = &library[0];
    assert_eq!(entry.document.id, doc_id);
    assert_eq!(
        entry.progress.as_ref(),
        Some(&latest),
        "the listing must return the most recently recorded row, provenance intact"
    );
    assert!(
        entry.progress_recorded_at.is_some(),
        "the listing must carry when the latest progress was recorded"
    );
}

#[test]
fn the_library_keeps_two_documents_with_the_same_page_distinguishable_by_provenance() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    let viewer_doc = store
        .upsert_document(&document(
            "/library/by-viewer.pdf",
            FileId::Posix { dev: 1, ino: 11 },
        ))
        .expect("insert");
    let manual_doc = store
        .upsert_document(&document(
            "/library/by-hand.pdf",
            FileId::Posix { dev: 1, ino: 12 },
        ))
        .expect("insert");

    // Same page, same total, different provenance: the listing must not flatten the distinction.
    store
        .record_progress(
            viewer_doc,
            &StoredProgress::new(
                5,
                Some(80),
                ProgressSource::External("evince-gvfs"),
                Confidence::Exact,
            ),
        )
        .expect("record");
    store
        .record_progress(
            manual_doc,
            &StoredProgress::new(5, Some(80), ProgressSource::Manual, Confidence::Manual),
        )
        .expect("record");

    let library = store.library().expect("list the library");
    let from_viewer = library
        .iter()
        .find(|entry| entry.document.id == viewer_doc)
        .expect("viewer document");
    let by_hand = library
        .iter()
        .find(|entry| entry.document.id == manual_doc)
        .expect("manual document");

    assert_eq!(
        from_viewer.progress.as_ref().map(|progress| &progress.source),
        Some(&ProgressSource::External("evince-gvfs"))
    );
    assert_eq!(
        from_viewer.progress.as_ref().map(|progress| progress.confidence),
        Some(Confidence::Exact)
    );
    assert_eq!(
        by_hand.progress.as_ref().map(|progress| &progress.source),
        Some(&ProgressSource::Manual)
    );
    assert_eq!(
        by_hand.progress.as_ref().map(|progress| progress.confidence),
        Some(Confidence::Manual)
    );
}

#[test]
fn a_document_with_no_progress_is_listed_without_inventing_one() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let doc_id = store
        .upsert_document(&Document::new("/library/unread.pdf"))
        .expect("insert");

    let library = store.library().expect("list the library");
    assert_eq!(library.len(), 1);
    assert_eq!(library[0].document.id, doc_id);
    assert_eq!(
        library[0].progress, None,
        "no progress is `None`, never a zero that would render as \"0% read\""
    );
    assert_eq!(library[0].progress_recorded_at, None);
}

#[test]
fn an_empty_store_has_an_empty_library() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    assert!(store.library().expect("list the library").is_empty());
}

#[test]
fn the_library_is_ordered_deterministically_by_path() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    for (index, name) in ["/library/b.pdf", "/library/a.pdf", "/library/c.pdf"]
        .into_iter()
        .enumerate()
    {
        store
            .upsert_document(&document(
                name,
                FileId::Posix {
                    dev: 1,
                    ino: index as u64 + 20,
                },
            ))
            .expect("insert");
    }

    let paths: Vec<PathBuf> = store
        .library()
        .expect("list the library")
        .into_iter()
        .map(|entry| entry.document.document.path)
        .collect();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("/library/a.pdf"),
            PathBuf::from("/library/b.pdf"),
            PathBuf::from("/library/c.pdf"),
        ]
    );
}
