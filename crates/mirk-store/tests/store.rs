//! T5 acceptance tests, exercised only through the public interface of `mirk-store`.
//!
//! Every database these tests touch lives in a `tempfile` directory, never in the user's real
//! data directory or `$HOME`. Nothing here inspects `sqlite_master` directly: the schema is
//! observed through `Store`'s own API.

use std::path::{Path, PathBuf};

use mirk_core::{
    Confidence, CoverChoice, CoverSource, Document, FileId, MirkError, Platform, ProgressSource,
    StoredProgress,
};
use mirk_store::{
    adopt_legacy_data_dir, covering_folder, database_file_in, database_path, user_data_dir_for,
    AuthorEdit, PageVisit, ReadingSession, Store, StoredPageVisit, StoredReadingSession,
    SuggestionDecision, WatchedFolder,
};

fn temp_db() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("mirk.sqlite3");
    (dir, path)
}

/// Removes the V12 reading tables from a database rewound to an older version.
///
/// They did not exist at that version, so a fixture that rebuilds an older shape from this build's
/// current schema must take them away again: leaving them would make the collision preflight refuse
/// the open before the step under test runs.
fn drop_reading_tables(raw: &rusqlite::Connection) {
    raw.execute_batch("DROP TABLE page_visits; DROP TABLE reading_sessions;")
        .expect("remove the tables a later version creates");
}

/// How many rows `table` holds, read through SQLite directly.
///
/// No public read enumerates a whole table, and a refused write has to leave storage exactly as it
/// was: this is how a test proves that nothing was stored, rather than only that a call returned an
/// error.
fn row_count(path: &Path, table: &str) -> i64 {
    let connection = rusqlite::Connection::open(path).expect("open raw");
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .expect("count the rows")
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
            "authors",
            "document_authors",
            "documents",
            "folders",
            "open_events",
            "page_visits",
            "progress",
            "reading_sessions",
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
        rows[0].read_at,
        Some(read_at),
        "the reading time must survive the round trip"
    );

    let library = store.library().expect("list the library");
    assert_eq!(library.len(), 1);
    assert_eq!(
        library[0]
            .progress
            .as_ref()
            .and_then(|progress| progress.read_at),
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
        entry
            .progress
            .as_ref()
            .and_then(|progress| progress.read_at),
        None
    );
    assert_ne!(
        entry
            .progress
            .as_ref()
            .and_then(|progress| progress.read_at),
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
        entry
            .progress
            .as_ref()
            .and_then(|progress| progress.read_at),
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

// --- T6: a rename must not cost a reader the library they already have --------------------------

#[test]
fn adopts_a_legacy_directory_and_moves_its_contents_with_it() {
    let base = tempfile::tempdir().expect("temp dir");
    let legacy = base.path().join("oct");
    std::fs::create_dir(&legacy).expect("create legacy dir");
    let database = legacy.join("oct.sqlite3");
    std::fs::write(&database, b"not empty").expect("write a row-shaped payload");
    std::fs::write(legacy.join("cover-1.png"), b"png").expect("write a cover");

    let adopted = adopt_legacy_data_dir(base.path()).expect("the legacy directory is adopted");

    assert_eq!(adopted, base.path().join("mirk"));
    assert!(!legacy.exists(), "the legacy directory is gone, not copied");
    // The whole directory moves: the database AND the covers that live beside it.
    assert_eq!(
        std::fs::read(adopted.join("mirk.sqlite3")).expect("the database survived"),
        b"not empty"
    );
    assert!(adopted.join("cover-1.png").exists(), "the cover moved too");
    // A second launch has nothing left to adopt.
    assert!(adopt_legacy_data_dir(base.path()).is_none());
}

#[test]
fn leaves_the_legacy_directory_alone_when_the_new_one_exists() {
    let base = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir(base.path().join("oct")).expect("create legacy dir");
    std::fs::create_dir(base.path().join("mirk")).expect("create current dir");

    assert!(adopt_legacy_data_dir(base.path()).is_none());
    assert!(
        base.path().join("oct").exists(),
        "the legacy directory is untouched"
    );
    assert!(base.path().join("mirk").exists());
}

#[test]
fn adopts_nothing_when_neither_directory_exists() {
    let base = tempfile::tempdir().expect("temp dir");
    assert!(adopt_legacy_data_dir(base.path()).is_none());
}

// The rename chain: `mirk` replaced `loci`, which replaced `oct`. A reader may have opened the
// application under any of the three, and adoption walks the chain instead of knowing one predecessor.
#[test]
fn adopts_the_previous_name_as_well_as_the_oldest_one() {
    let base = tempfile::tempdir().expect("temp dir");
    let previous = base.path().join("loci");
    std::fs::create_dir(&previous).expect("create the previous directory");
    std::fs::write(previous.join("loci.sqlite3"), b"rows").expect("write its database");

    let adopted = adopt_legacy_data_dir(base.path()).expect("the previous name is adopted");

    assert_eq!(adopted, base.path().join("mirk"));
    assert!(
        adopted.join("mirk.sqlite3").exists(),
        "the file takes the new name"
    );
    assert!(!adopted.join("loci.sqlite3").exists());
}

#[test]
fn prefers_a_legacy_directory_that_holds_a_library_over_an_empty_newer_one() {
    let base = tempfile::tempdir().expect("temp dir");
    // The leftover of a launch that died before SQLite opened, under the newer name.
    std::fs::create_dir(base.path().join("loci")).expect("create the empty previous dir");
    let oldest = base.path().join("oct");
    std::fs::create_dir(&oldest).expect("create the oldest dir");
    std::fs::write(oldest.join("oct.sqlite3"), b"rows").expect("write its database");

    let adopted = adopt_legacy_data_dir(base.path()).expect("the library is the one adopted");

    assert_eq!(adopted, base.path().join("mirk"));
    assert_eq!(
        std::fs::read(adopted.join("mirk.sqlite3")).expect("the rows survived"),
        b"rows"
    );
}

#[test]
fn adopts_a_real_sqlite_database_that_still_opens_afterwards() {
    let base = tempfile::tempdir().expect("temp dir");
    let legacy = base.path().join("oct");
    std::fs::create_dir(&legacy).expect("create legacy dir");
    // A real database, built by the real store, not a byte string that merely looks like one.
    drop(Store::open(legacy.join("oct.sqlite3")).expect("create the database"));

    let adopted = adopt_legacy_data_dir(base.path()).expect("the directory is adopted");

    assert!(adopted.join("mirk.sqlite3").exists());
    assert!(!adopted.join("oct.sqlite3").exists());
    // The proof that matters: the file is still a database this build can open and read.
    let store = Store::open(database_file_in(&adopted)).expect("reopen the adopted database");
    assert_eq!(
        store
            .document_count()
            .expect("read through the adopted database"),
        0
    );
}

#[test]
fn moves_the_write_ahead_log_with_its_database() {
    let base = tempfile::tempdir().expect("temp dir");
    let legacy = base.path().join("oct");
    std::fs::create_dir(&legacy).expect("create legacy dir");
    for suffix in ["", "-wal", "-shm"] {
        std::fs::write(legacy.join(format!("oct.sqlite3{suffix}")), b"state").expect("write");
    }

    let adopted = adopt_legacy_data_dir(base.path()).expect("the directory is adopted");

    // The trio travels together: a database renamed without its WAL is a database that lost its last
    // transaction.
    for suffix in ["", "-wal", "-shm"] {
        assert!(
            adopted.join(format!("mirk.sqlite3{suffix}")).exists(),
            "{suffix} moved"
        );
        assert!(
            !adopted.join(format!("oct.sqlite3{suffix}")).exists(),
            "{suffix} left behind"
        );
    }
}

#[test]
fn merges_a_real_database_into_an_empty_modern_directory_and_it_still_opens() {
    let base = tempfile::tempdir().expect("temp dir");
    let legacy = base.path().join("oct");
    std::fs::create_dir(&legacy).expect("create legacy dir");
    // A real database, built by the real store.
    drop(Store::open(legacy.join("oct.sqlite3")).expect("create the database"));
    std::fs::create_dir(legacy.join("covers")).expect("create covers");
    std::fs::write(legacy.join("covers").join("cover-1.png"), b"png").expect("write a cover");
    // The leftover of a launch that died before SQLite opened: it exists, and it holds nothing.
    std::fs::create_dir(base.path().join("mirk")).expect("create empty current dir");

    let adopted = adopt_legacy_data_dir(base.path()).expect("the database is merged in");

    assert_eq!(adopted, base.path().join("mirk"));
    assert!(
        adopted.join("covers").join("cover-1.png").exists(),
        "the covers moved"
    );
    // The proof that matters: the merged file is a database this build can open and read.
    let store = Store::open(database_file_in(&adopted)).expect("reopen the merged database");
    assert_eq!(store.document_count().expect("read it"), 0);
    // The legacy directory is left in place rather than deleted: a half-finished merge stays
    // recoverable by hand, and nothing is ever removed on a rename's account.
    assert!(legacy.exists());
}

#[test]
fn refuses_to_merge_when_a_sidecar_would_collide() {
    let base = tempfile::tempdir().expect("temp dir");
    let legacy = base.path().join("oct");
    std::fs::create_dir(&legacy).expect("create legacy dir");
    std::fs::write(legacy.join("oct.sqlite3"), b"rows").expect("write the database");
    std::fs::write(legacy.join("oct.sqlite3-wal"), b"wal").expect("write its log");
    // A modern directory with a log but no database: a mixed state in which moving the legacy trio
    // would attach a log to a database it does not belong to.
    let current = base.path().join("mirk");
    std::fs::create_dir(&current).expect("create current dir");
    std::fs::write(current.join("oct.sqlite3-wal"), b"other wal").expect("write the modern log");

    // Adoption declines, and the caller keeps reading the legacy library in place.
    assert_eq!(
        adopt_legacy_data_dir(base.path()).expect("falls back to the legacy directory"),
        legacy
    );
    assert_eq!(
        std::fs::read(legacy.join("oct.sqlite3")).expect("legacy database untouched"),
        b"rows"
    );
    assert_eq!(
        std::fs::read(current.join("oct.sqlite3-wal")).expect("modern log untouched"),
        b"other wal"
    );
}

#[test]
fn leaves_both_directories_alone_when_the_modern_one_already_has_a_database() {
    let base = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir(base.path().join("oct")).expect("create legacy dir");
    std::fs::write(base.path().join("oct").join("oct.sqlite3"), b"old").expect("write legacy");
    std::fs::create_dir(base.path().join("mirk")).expect("create current dir");
    std::fs::write(base.path().join("mirk").join("mirk.sqlite3"), b"new").expect("write current");

    assert!(adopt_legacy_data_dir(base.path()).is_none());
    assert_eq!(
        std::fs::read(base.path().join("oct").join("oct.sqlite3")).expect("legacy untouched"),
        b"old"
    );
    assert_eq!(
        std::fs::read(base.path().join("mirk").join("mirk.sqlite3")).expect("current untouched"),
        b"new"
    );
}

#[test]
fn finds_the_legacy_database_file_when_the_modern_one_is_absent() {
    let dir = tempfile::tempdir().expect("temp dir");

    // Nothing there: the modern name, which the store will create.
    assert_eq!(
        database_file_in(dir.path()),
        dir.path().join("mirk.sqlite3")
    );

    // Only the legacy file: it wins, so a failed file rename cannot orphan a full library behind an
    // empty database created beside it.
    std::fs::write(dir.path().join("oct.sqlite3"), b"rows").expect("write legacy db");
    assert_eq!(database_file_in(dir.path()), dir.path().join("oct.sqlite3"));

    // Both: the modern one, because that is the one this build writes.
    std::fs::write(dir.path().join("mirk.sqlite3"), b"rows").expect("write modern db");
    assert_eq!(
        database_file_in(dir.path()),
        dir.path().join("mirk.sqlite3")
    );
}

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
        PathBuf::from("/xdg/data/mirk")
    );

    // XDG spec fallback when XDG_DATA_HOME is unset.
    assert_eq!(
        user_data_dir_for(Platform::Linux, None, Some(Path::new("/home/u")), None)
            .expect("linux fallback"),
        PathBuf::from("/home/u/.local/share/mirk")
    );

    assert_eq!(
        user_data_dir_for(
            Platform::Windows,
            None,
            Some(Path::new("C:/Users/u")),
            Some(Path::new("C:/Users/u/AppData/Roaming")),
        )
        .expect("windows"),
        PathBuf::from("C:/Users/u/AppData/Roaming/mirk")
    );

    assert_eq!(
        user_data_dir_for(Platform::MacOs, None, Some(Path::new("/Users/u")), None).expect("macos"),
        PathBuf::from("/Users/u/Library/Application Support/mirk")
    );

    // An unsupported platform says so rather than guessing a location.
    let unsupported = user_data_dir_for(Platform::Other, None, None, None);
    assert!(matches!(unsupported, Err(MirkError::NotSupported { .. })));

    match user_data_dir_for(Platform::Linux, None, None, None) {
        Err(MirkError::Storage(message)) => {
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
    assert!(resolved.ends_with("mirk/mirk.sqlite3"), "{resolved:?}");

    let cwd = std::env::current_dir().expect("current dir");
    assert!(
        !resolved.starts_with(&cwd),
        "database path must not be in the cwd"
    );
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
    assert_eq!(
        before, after,
        "reopening a current database must change nothing"
    );
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
        store
            .set_setting("language", "es")
            .expect("write a setting");
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
fn opening_a_current_database_takes_no_write_reservation() {
    // Opening a store that is already current takes no write reservation, and a second process
    // writing the same file must not stand in its way: only the upgrade needs a write lock. The lock
    // held here is the writer's RESERVED lock, which does not block readers; if the open tried to take
    // one it would wait out the connection's own busy timeout behind it and then fail. (A lock that
    // excludes readers, such as an exclusive lock, does block this read — as it blocks any read — but
    // that is a different question from the write reservation this test is about.)
    let (_dir, path) = temp_db();
    {
        let store = Store::open(&path).expect("migrate a fresh database");
        store
            .set_setting("language", "es")
            .expect("write a setting");
    }

    let competitor = rusqlite::Connection::open(&path).expect("open raw");
    competitor
        .execute_batch("BEGIN IMMEDIATE")
        .expect("take the database's write lock");

    let store = Store::open(&path)
        .expect("opening a current database must not need the write lock a writer holds");
    assert_eq!(
        store.schema_version().expect("schema version"),
        Store::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(
        store.get_setting("language").expect("read the setting"),
        Some("es".to_string())
    );
    drop(store);

    competitor
        .execute_batch("ROLLBACK")
        .expect("release the write lock");

    // And the writes the store exists for still happen once the lock is free.
    let store = Store::open(&path).expect("reopen");
    store
        .set_setting("language", "en")
        .expect("write after the lock is gone");
    assert_eq!(
        store.get_setting("language").expect("read the setting"),
        Some("en".to_string())
    );
}

#[test]
fn a_newer_database_is_refused_without_taking_the_write_lock() {
    // The refusal is the version check, which is a read: it must be reported as such, and not as the
    // busy error an open that took the write lock first would produce here.
    let (_dir, path) = temp_db();
    drop(Store::open(&path).expect("migrate a fresh database"));

    let competitor = rusqlite::Connection::open(&path).expect("open raw");
    competitor
        .pragma_update(None, "user_version", Store::CURRENT_SCHEMA_VERSION + 1)
        .expect("simulate a database a future build wrote");
    competitor
        .execute_batch("BEGIN IMMEDIATE")
        .expect("take the database's write lock");

    let error = Store::open(&path).expect_err("a newer schema must be refused");
    match error {
        MirkError::Storage(message) => assert!(
            message.contains("newer than this build supports"),
            "the refusal must be the version check, not a lock error: {message}"
        ),
        other => panic!("unexpected error: {other:?}"),
    }

    competitor
        .execute_batch("ROLLBACK")
        .expect("release the write lock");
}

#[test]
fn a_database_from_a_newer_build_is_refused_rather_than_downgraded() {
    let (_dir, path) = temp_db();
    {
        let store = Store::open(&path).expect("open");
        assert_eq!(
            store.schema_version().expect("version"),
            Store::CURRENT_SCHEMA_VERSION
        );
    }

    // Simulate a future build having written a higher version, then reopen with this build.
    {
        let connection = rusqlite::Connection::open(&path).expect("open raw");
        connection
            .pragma_update(None, "user_version", Store::CURRENT_SCHEMA_VERSION + 1)
            .expect("bump version");
    }

    let error = Store::open(&path).expect_err("a newer schema must be refused");
    assert!(
        matches!(error, MirkError::Storage(_)),
        "unexpected: {error:?}"
    );
}

#[test]
fn a_step_that_fails_on_the_way_to_v4_leaves_the_database_at_v3_untouched() {
    // The upgrade is one transaction, so a step that fails must leave the recorded version and every
    // existing row exactly as they were. The success path cannot show that: only a step that actually
    // fails can tell one transaction from "do the work, then stamp the version". The first pending
    // step fails here; the whole-upgrade unit is pinned by the rollback test beside the migration itself.
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
        store
            .set_setting("language", "es")
            .expect("write a setting");
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
        drop_reading_tables(&raw);
        raw.pragma_update(None, "user_version", 3i64)
            .expect("rewind the version stamp to the pre-V4 database shape");
    }

    // 1. The failing step reaches the caller rather than being swallowed or papered over.
    let error =
        Store::open(&path).expect_err("the V4 step must fail when `read_at` already exists");
    assert!(
        matches!(error, MirkError::Storage(_)),
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
        .query_row(
            "SELECT value FROM settings WHERE key = 'language'",
            [],
            |row| row.get(0),
        )
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
        columns
            .iter()
            .filter(|name| name.as_str() == "read_at")
            .count(),
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
            "authors",
            "document_authors",
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
            .set_user_naming(
                document_id,
                Some("The reader's title"),
                AuthorEdit::Untouched,
            )
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
        store
            .set_setting("language", "es")
            .expect("write a setting");
    }
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        drop_reading_tables(&raw);
        raw.pragma_update(None, "user_version", 4i64)
            .expect("rewind the version stamp to the pre-V5 database shape");
    }

    // 1. The failing step reaches the caller rather than being swallowed or papered over.
    let error =
        Store::open(&path).expect_err("the V5 step must fail when `user_title` already exists");
    assert!(
        matches!(error, MirkError::Storage(_)),
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
        .query_row(
            "SELECT value FROM settings WHERE key = 'language'",
            [],
            |row| row.get(0),
        )
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
        columns
            .iter()
            .filter(|name| name.as_str() == "user_title")
            .count(),
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
        // The fixture is a CURRENT database rewound to the old version and given back the column
        // that version had (`user_author`), so its count is the current shape plus one.
        23,
        "the documents table must keep the columns the fixture was built with, found {columns:?}"
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
            "authors",
            "document_authors",
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
            .set_user_naming(
                document_id,
                Some("The reader's title"),
                AuthorEdit::Untouched,
            )
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
        store
            .set_setting("language", "es")
            .expect("write a setting");
    }
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        drop_reading_tables(&raw);
        raw.pragma_update(None, "user_version", 5i64)
            .expect("rewind the version stamp to the pre-V6 database shape");
    }

    // 1. The failing step reaches the caller rather than being swallowed or papered over.
    let error =
        Store::open(&path).expect_err("the V6 step must fail when `cover_page` already exists");
    assert!(
        matches!(error, MirkError::Storage(_)),
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
        .query_row(
            "SELECT value FROM settings WHERE key = 'language'",
            [],
            |row| row.get(0),
        )
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
        columns
            .iter()
            .filter(|name| name.as_str() == "cover_page")
            .count(),
        1,
        "the failed ADD COLUMN must not have left a second `cover_page`; columns: {columns:?}"
    );
    assert_eq!(
        columns
            .iter()
            .filter(|name| name.as_str() == "cover_image")
            .count(),
        1,
        "the failed batch must not have added `cover_image` twice; columns: {columns:?}"
    );
    assert_eq!(
        columns.len(),
        // The fixture is a CURRENT database rewound to the old version and given back the column
        // that version had (`user_author`), so its count is the current shape plus one.
        23,
        "the documents table must keep the columns the fixture was built with, found {columns:?}"
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
            "authors",
            "document_authors",
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
fn a_step_that_fails_on_the_way_to_v7_leaves_the_database_at_v6_untouched() {
    // The same shape as the V4, V5 and V6 atomicity tests, one version later. A database is built with
    // the store's own current schema, its version stamp is rewound to 6, and `documents.user_author` is
    // put back **by hand** — V9 retires that column, so it is no longer part of a current schema, and
    // the V6 shape this test needs has to be rebuilt rather than inherited. With the column present,
    // reopening runs the V7 step, `ALTER TABLE documents ADD COLUMN user_author TEXT`, against a table
    // that already has it, which SQLite refuses with "duplicate column name". A step stamped before its
    // work would leave the database claiming version 7 with work that did not happen; the transaction
    // must leave the version at 6 and every row untouched.
    let (_dir, path) = temp_db();

    let document_id;
    {
        let store = Store::open(&path).expect("build a current database");
        let mut doc = document("/library/at-risk.pdf", FileId::Posix { dev: 3, ino: 20 });
        doc.pages = Some(120);
        doc.title = Some("The document's own title".to_string());
        doc.author = Some("The document's own author".to_string());
        document_id = store.upsert_document(&doc).expect("insert the document");
        store
            .set_user_naming(
                document_id,
                Some("The reader's title"),
                AuthorEdit::Decided(Some("The reader's author")),
            )
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
        store
            .set_setting("language", "es")
            .expect("write a setting");
    }
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        drop_reading_tables(&raw);
        raw.pragma_update(None, "user_version", 6i64)
            .expect("rewind the version stamp to the pre-V7 database shape");
        raw.execute_batch(
            "ALTER TABLE documents ADD COLUMN user_author TEXT;\n             UPDATE documents SET user_author = 'The reader''s author';",
        )
        .expect("rebuild the V6 documents table, which already has the reader's author column");
    }

    // 1. The failing step reaches the caller rather than being swallowed or papered over.
    let error =
        Store::open(&path).expect_err("the V7 step must fail when `user_author` already exists");
    assert!(
        matches!(error, MirkError::Storage(_)),
        "a failed migration must surface as a storage error, got {error:?}"
    );

    // 2. The version was not stamped ahead of work that did not happen.
    let raw = rusqlite::Connection::open(&path).expect("open raw");
    let version: i64 = raw
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read the version");
    assert_eq!(
        version, 6,
        "a failed V7 step must leave the database at version 6, not claim version 7"
    );

    // 3. Every pre-existing value is still readable: both titles, both authors, the cover choice, the
    //    page count, the position with its reading time, and the setting the previous steps recorded.
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
    let (own_author, user_author): (Option<String>, Option<String>) = raw
        .query_row(
            "SELECT author, user_author FROM documents WHERE id = ?1",
            rusqlite::params![document_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("both authors must survive the failed migration");
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
    assert_eq!(own_author.as_deref(), Some("The document's own author"));
    assert_eq!(user_author.as_deref(), Some("The reader's author"));
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
        .query_row(
            "SELECT value FROM settings WHERE key = 'language'",
            [],
            |row| row.get(0),
        )
        .expect("the setting must survive the failed migration");
    assert_eq!(language, "es");

    // 4. No half-applied state: the column the failing step wanted to add is still there exactly once,
    //    the table keeps its documented columns, and no table was created or dropped.
    let mut statement = raw
        .prepare("SELECT name FROM pragma_table_info('documents')")
        .expect("prepare");
    let columns: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        columns
            .iter()
            .filter(|name| name.as_str() == "user_author")
            .count(),
        1,
        "the failed ADD COLUMN must not have left a second `user_author`; columns: {columns:?}"
    );
    assert_eq!(
        columns.len(),
        // The fixture is a CURRENT database rewound to the old version and given back the column
        // that version had (`user_author`), so its count is the current shape plus one.
        24,
        "the documents table must keep the columns the fixture was built with, found {columns:?}"
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
            "authors",
            "document_authors",
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

// --- R42/S1: the V9 migration, and what it must never lose ---------------------------------------

#[test]
fn a_v8_database_keeps_every_readers_author_when_the_migration_gives_it_a_list() {
    // R42/S1: the migration preserves every existing reader's author. "A database written by the
    // previous code path" cannot be produced by this build any more — V9 retires the column V7 wrote —
    // so it is reconstructed exactly: the store's own current schema, its version stamp rewound to 8,
    // and `documents.user_author` put back with the values the old editor used to save. That column is
    // the previous code path's only trace, and putting it back is the only way to prove the backfill
    // reads the real thing rather than an imitation of it.
    let (_dir, path) = temp_db();

    let named;
    let lowercase;
    let blank;
    let absent;
    {
        let store = Store::open(&path).expect("build a current database");

        let mut cosmos = document("/library/cosmos.pdf", FileId::Posix { dev: 9, ino: 1 });
        cosmos.author = Some("Sagan, Carl; Druyan, Ann".to_string());
        named = store
            .upsert_document(&cosmos)
            .expect("insert the named document");
        store
            .set_user_naming(named, Some("Cosmos"), AuthorEdit::Untouched)
            .expect("the reader's title, which must survive too");

        let mut lowercase_doc = document("/library/contact.pdf", FileId::Posix { dev: 9, ino: 2 });
        lowercase_doc.author = Some("The document's own author".to_string());
        lowercase = store
            .upsert_document(&lowercase_doc)
            .expect("insert a second document by the same person");

        let mut blanked = document("/library/blanked.pdf", FileId::Posix { dev: 9, ino: 3 });
        blanked.author = Some("The document's own author".to_string());
        blank = store
            .upsert_document(&blanked)
            .expect("insert the document whose old author value was blank");

        absent = store
            .upsert_document(&document(
                "/library/absent.pdf",
                FileId::Posix { dev: 9, ino: 4 },
            ))
            .expect("insert the document whose old author value was NULL");
    }
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        raw.pragma_update(None, "user_version", 8i64)
            .expect("rewind the version stamp to the V8 shape");
        raw.execute_batch(
            "DROP TABLE document_authors;
             DROP TABLE authors;
             DROP TABLE page_visits;
             DROP TABLE reading_sessions;
             ALTER TABLE documents ADD COLUMN user_author TEXT;",
        )
        .expect("rebuild the column V7 wrote, which V9 retires, and remove the tables V9 creates");
        // Two spellings of ONE person, a blank that was never an author, and a row that never had one.
        for (id, value) in [
            (named, "Carl Sagan"),
            (lowercase, "carl sagan"),
            (blank, "   "),
        ] {
            raw.execute(
                "UPDATE documents SET user_author = ?1 WHERE id = ?2",
                rusqlite::params![value, id],
            )
            .expect("write the reader's author the old build stored");
        }
    }

    let store = Store::open(&path).expect("a V8 database migrates");
    assert_eq!(
        store.schema_version().expect("version"),
        Store::CURRENT_SCHEMA_VERSION
    );

    // The reader's author survives, as a one-entry list at the start of the order, and the rest of the
    // naming survives with it.
    let found = store.document_by_id(named).expect("read").expect("present");
    assert_eq!(found.document.authors, vec!["Carl Sagan"]);
    assert_eq!(
        found.document.user_title.as_deref(),
        Some("Cosmos"),
        "the reader's title must survive the migration too"
    );
    assert_eq!(
        found.document.author.as_deref(),
        Some("Sagan, Carl; Druyan, Ann"),
        "the document's own author is untouched, and still ONE string"
    );
    assert_eq!(
        found.document.display_authors(),
        vec!["Carl Sagan"],
        "and the reader's list is what is displayed"
    );

    // The other spelling of the same person is the SAME person, and the spelling that arrived first
    // wins — the migration deduplicates through the same key the write path uses.
    let second = store
        .document_by_id(lowercase)
        .expect("read")
        .expect("present");
    assert_eq!(second.document.authors, vec!["Carl Sagan"]);

    // A blank was never an author: no row, and the document falls back to its embedded string.
    let blanked = store.document_by_id(blank).expect("read").expect("present");
    assert!(blanked.document.authors.is_empty());
    assert_eq!(
        blanked.document.display_authors(),
        vec!["The document's own author"]
    );
    let absent = store
        .document_by_id(absent)
        .expect("read")
        .expect("present");
    assert!(absent.document.authors.is_empty());
    assert!(absent.document.display_authors().is_empty());

    // One `authors` row for one person, and one list row per document that actually had an author.
    let raw = rusqlite::Connection::open(&path).expect("open raw");
    let mut statement = raw
        .prepare("SELECT name FROM authors ORDER BY id")
        .expect("prepare");
    let names: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        names,
        vec!["Carl Sagan"],
        "one row per person, deduplicated case-insensitively, first spelling wins"
    );
    let list_rows: i64 = raw
        .query_row("SELECT COUNT(*) FROM document_authors", [], |row| {
            row.get(0)
        })
        .expect("count the list rows");
    assert_eq!(
        list_rows, 2,
        "one row per document that had a non-blank author, and none for a blank or an absent one"
    );
    let positions: Vec<i64> = raw
        .prepare("SELECT position FROM document_authors ORDER BY doc_id")
        .expect("prepare")
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        positions,
        vec![0, 0],
        "a backfilled list is one entry, at position 0"
    );

    // And the retired column is really gone: there is no second home for this fact to disagree from.
    let mut statement = raw
        .prepare("SELECT name FROM pragma_table_info('documents')")
        .expect("prepare");
    let columns: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert!(
        !columns.iter().any(|name| name == "user_author"),
        "V9 retires `documents.user_author` once its values have moved; columns: {columns:?}"
    );
}

#[test]
fn a_step_that_fails_on_the_way_to_v9_leaves_the_database_at_v8_untouched() {
    // The same shape as the V4–V8 atomicity tests, with one deliberate difference that makes it a
    // stronger test. The failure is arranged to happen at the **end** of the step: both tables are
    // created and every author is backfilled before `DROP COLUMN` refuses, so what is proven is that
    // the transaction takes the new tables and their rows back out again — not merely that the first
    // statement stopped before doing anything.
    //
    // The V8 shape has to be rebuilt rather than inherited, because a current database already has the
    // tables V9 creates and no longer has the column V9 retires: the stamp is rewound, the two tables
    // are dropped, `documents.user_author` is put back with a value, and an index over that column
    // makes the final `DROP COLUMN` fail the way a hand-edited database would.
    let (_dir, path) = temp_db();

    let document_id;
    {
        let store = Store::open(&path).expect("build a current database");
        let mut doc = document("/library/at-risk.pdf", FileId::Posix { dev: 3, ino: 21 });
        doc.pages = Some(120);
        doc.title = Some("The document's own title".to_string());
        doc.author = Some("The document's own author".to_string());
        document_id = store.upsert_document(&doc).expect("insert the document");
        store
            .set_user_naming(
                document_id,
                Some("The reader's title"),
                AuthorEdit::Decided(Some("The reader's author")),
            )
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
        store
            .set_setting("language", "es")
            .expect("write a setting");
    }
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        raw.pragma_update(None, "user_version", 8i64)
            .expect("rewind the version stamp to the V8 shape");
        raw.execute_batch(
            "DROP TABLE document_authors;
             DROP TABLE authors;
             DROP TABLE page_visits;
             DROP TABLE reading_sessions;
             ALTER TABLE documents ADD COLUMN user_author TEXT;
             UPDATE documents SET user_author = 'The reader''s author';
             CREATE INDEX idx_at_risk_user_author ON documents (user_author);",
        )
        .expect("rebuild the V8 shape, with an index over the column V9 retires");
    }

    // 1. The failing step reaches the caller rather than being swallowed or papered over.
    let error = Store::open(&path)
        .expect_err("the V9 step must fail when the retired column cannot be dropped");
    assert!(
        matches!(error, MirkError::Storage(_)),
        "a failed migration must surface as a storage error, got {error:?}"
    );

    // 2. The version was not stamped ahead of work that did not happen.
    let raw = rusqlite::Connection::open(&path).expect("open raw");
    let version: i64 = raw
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read the version");
    assert_eq!(
        version, 8,
        "a failed V9 step must leave the database at version 8, not claim version 9"
    );

    // 3. Every pre-existing value is still readable — including the reader's author the step had
    //    already moved into the new tables by the time it failed.
    let (path_value, own_author, user_author, user_title, pages): (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
    ) = raw
        .query_row(
            "SELECT path, author, user_author, user_title, pages FROM documents WHERE id = ?1",
            rusqlite::params![document_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("the document must survive the failed migration");
    assert_eq!(path_value, "/library/at-risk.pdf");
    assert_eq!(own_author.as_deref(), Some("The document's own author"));
    assert_eq!(
        user_author.as_deref(),
        Some("The reader's author"),
        "the column the step was moving must be exactly where it was"
    );
    assert_eq!(user_title.as_deref(), Some("The reader's title"));
    assert_eq!(pages, Some(120));

    let progress_count: i64 = raw
        .query_row("SELECT COUNT(*) FROM progress", [], |row| row.get(0))
        .expect("count the positions");
    assert_eq!(progress_count, 1, "no position may be lost");
    let language: String = raw
        .query_row(
            "SELECT value FROM settings WHERE key = 'language'",
            [],
            |row| row.get(0),
        )
        .expect("the setting must survive the failed migration");
    assert_eq!(language, "es");

    // 4. Nothing the step created survived it. The backfill had already run, so these tables existed
    //    and held rows; a step that was not one transaction would leave them behind.
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
        "the failed step must leave neither new table behind, though it had already written to them"
    );

    let mut statement = raw
        .prepare("SELECT name FROM pragma_table_info('documents')")
        .expect("prepare");
    let columns: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        columns
            .iter()
            .filter(|name| name.as_str() == "user_author")
            .count(),
        1,
        "the retired column must still be there exactly once; columns: {columns:?}"
    );
    assert_eq!(
        columns.len(),
        24,
        "the documents table must keep the columns the fixture was built with, found {columns:?}"
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
            .upsert_document(&document(
                "/library/read.pdf",
                FileId::Posix { dev: 7, ino: 7 },
            ))
            .expect("insert");

        // Neither choice exists until the reader makes one: the first page is the rule's default and
        // is not a stored value.
        let entries = store.library().expect("list");
        assert_eq!(entries[0].document.document.cover_page, None);
        assert_eq!(entries[0].document.document.cover_image, None);

        store
            .set_cover_page(document_id, Some(12))
            .expect("choose a page");
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
        mirk_core::domain::CoverChoice::Image("7-cover.jpg".to_string())
    );

    // Removing the image falls back to the page still on record, not to the default.
    store
        .set_cover_image(document_id, None)
        .expect("remove the image");
    let entries = store.library().expect("list after removal");
    assert_eq!(
        entries[0].document.document.cover_choice(),
        mirk_core::domain::CoverChoice::Page(12)
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
    store
        .set_cover_page(document_id, Some(3))
        .expect("choose a page");
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
    assert_eq!(
        document.cover_page,
        Some(3),
        "a rescan must not clear the page"
    );
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
        .upsert_document(&document(
            "/library/read.pdf",
            FileId::Posix { dev: 9, ino: 9 },
        ))
        .expect("insert");

    for refused in ["", "   ", "/etc/passwd", "a/../b.png", "dir\\x.png"] {
        let result = store.set_cover_image(id, Some(refused));
        assert!(
            matches!(result, Err(MirkError::InvalidInput { field, .. }) if field == "cover image"),
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
    assert!(
        matches!(error, MirkError::Storage(_)),
        "unexpected: {error:?}"
    );
}

// --- S1, again: the column contract, not just the table names ---------------------------------

#[test]
fn the_schema_has_the_documented_columns_of_every_table() {
    let (_dir, path) = temp_db();
    let _store = Store::open(&path).expect("open");

    // Read through SQLite's public catalog, so this asserts the *spec's* column list rather than
    // whatever the implementation happens to define.
    let connection = rusqlite::Connection::open(&path).expect("open raw");
    let expected: [(&str, &[&str]); 11] = [
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
        // R42/S1-S3: the readers' authors are facts in their own tables, and the order is a column.
        ("authors", &["id", "name", "name_key"]),
        ("document_authors", &["doc_id", "author_id", "position"]),
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
            &[
                "id",
                "exe",
                "desktop_id",
                "bundle_id",
                "platform",
                "capability",
                "detail",
            ],
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
        // F1 part 1: the facts only this application's own reader can honestly produce.
        (
            "reading_sessions",
            &["id", "doc_id", "started_at", "ended_at", "active_ms"],
        ),
        (
            "page_visits",
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
        .set_user_naming(id, Some("What the reader called it"), AuthorEdit::Untouched)
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
        .set_user_naming(id, None, AuthorEdit::Untouched)
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
        let result = store.set_user_naming(id, Some(blank), AuthorEdit::Untouched);
        match result {
            Err(MirkError::InvalidInput {
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
        .set_user_naming(id, Some("What the reader called it"), AuthorEdit::Untouched)
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
fn the_readers_author_is_a_separate_fact_and_is_the_one_displayed() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let identity = FileId::Posix { dev: 7, ino: 21 };
    let mut doc = document("/library/authored.pdf", identity.clone());
    doc.author = Some("The document's own author".to_string());
    let id = store.upsert_document(&doc).expect("insert the document");

    // Before anyone names it, there is only the document's own author and the reader's list is empty.
    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    assert!(found.document.authors.is_empty());
    assert_eq!(
        found.document.display_author().as_deref(),
        Some("The document's own author")
    );

    store
        .set_user_naming(
            id,
            None,
            AuthorEdit::Decided(Some("What the reader called them")),
        )
        .expect("name the author");

    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    // Both facts are stored, and the reader's is the one displayed.
    assert_eq!(
        found.document.author.as_deref(),
        Some("The document's own author"),
        "the reader's author must not overwrite the document's own"
    );
    assert_eq!(found.document.authors, vec!["What the reader called them"]);
    assert_eq!(
        found.document.display_author().as_deref(),
        Some("What the reader called them")
    );
}

#[test]
fn the_readers_naming_is_one_write_so_a_refused_write_leaves_both_facts_alone() {
    // R37's fix round, and R42/S5's rule that the naming write still carries everything. Save decides
    // two facts in ONE action — the reader's title and the reader's authors — so it must apply both or
    // neither. The defect this pins is precise: written as independent updates, the title landed even
    // when the author's write failed, and the editor then showed an error implying nothing had been
    // saved. The error message was a lie about a half-applied save.
    //
    // The V9 write makes that harder in one respect and easier to see in another: the list is
    // **replaced**, so a refused save has already deleted the reader's old rows by the time it fails.
    // The rollback is what brings them back, and this test starts from a naming that already exists so
    // that a dropped transaction and a surviving one read differently.
    //
    // The failure is made real from OUTSIDE the store: a trigger that refuses the authors' own table is
    // the same shape as a disk error or a constraint arriving mid-save, and it fails nothing that the
    // store's own write does not already do.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let identity = FileId::Posix { dev: 7, ino: 31 };
    let mut doc = document("/library/named.pdf", identity.clone());
    doc.title = Some("The document's own title".to_string());
    doc.author = Some("The document's own author".to_string());
    let id = store.upsert_document(&doc).expect("insert the document");

    // A naming that already exists: the save below must leave BOTH of these exactly as they are.
    store
        .set_user_naming(
            id,
            Some("The title the reader kept"),
            AuthorEdit::Decided(Some("The author the reader kept")),
        )
        .expect("save a naming the refused write must not disturb");

    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        raw.execute_batch(
            "CREATE TRIGGER refuse_the_readers_authors\n             BEFORE INSERT ON document_authors\n             BEGIN\n                 SELECT RAISE(ABORT, 'the last write of one save is refused');\n             END;",
        )
        .expect("install the trigger that refuses the reader's authors table");
    }

    let result = store.set_user_naming(
        id,
        Some("What the reader called it"),
        AuthorEdit::Decided(Some("What the reader called them")),
    );
    assert!(
        matches!(result, Err(MirkError::Storage(_))),
        "a refused write must be reported as a storage error, got {result:?}"
    );

    // NEITHER fact changed — and neither did the list the refused save had already deleted.
    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    assert_eq!(
        found.document.user_title.as_deref(),
        Some("The title the reader kept"),
        "a save whose author write was refused must not leave the new title behind"
    );
    assert_eq!(
        found.document.authors,
        vec!["The author the reader kept"],
        "the refused list must not have replaced the stored one, not even its delete"
    );
    assert_eq!(
        found.document.display_title(),
        "The title the reader kept",
        "the screen must keep showing what the store still holds"
    );
    assert_eq!(
        found.document.display_authors(),
        vec!["The author the reader kept"],
        "and the authors on screen are the ones that survived"
    );
}

#[test]
fn restoring_the_readers_title_leaves_the_readers_author_exactly_as_it_was() {
    // Restore decides the TITLE and nothing else: restoring the document's own title is not a
    // statement about who wrote the document. The combined write says so with
    // `AuthorEdit::Untouched`, and this is the evidence that "untouched" is not a quiet "clear it".
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let identity = FileId::Posix { dev: 7, ino: 32 };
    let mut doc = document("/library/named.pdf", identity.clone());
    doc.title = Some("The document's own title".to_string());
    doc.author = Some("The document's own author".to_string());
    let id = store.upsert_document(&doc).expect("insert the document");

    store
        .set_user_naming(
            id,
            Some("What the reader called it"),
            AuthorEdit::Decided(Some("What the reader called them")),
        )
        .expect("save the title and the author together");

    store
        .set_user_naming(id, None, AuthorEdit::Untouched)
        .expect("restore the document's own title");

    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    assert_eq!(found.document.user_title, None, "the title is restored");
    assert_eq!(
        found.document.authors,
        vec!["What the reader called them"],
        "restoring the title must not touch the reader's authors"
    );
    assert_eq!(
        found.document.display_author().as_deref(),
        Some("What the reader called them"),
        "and the author on screen stays the reader's"
    );
}

#[test]
fn an_empty_author_list_falls_back_to_the_documents_own() {
    // The deliberate difference from the title: the title refuses a blank because a document must have
    // one, while an author has a legitimate empty state — "no author of my own" (R42/S5). An empty list
    // is that decision, and so is a single emptied field in the one-author spelling, which the shipped
    // editor still sends: both restore the document's own author instead of being refused.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let identity = FileId::Posix { dev: 7, ino: 22 };
    let mut doc = document("/library/authored.pdf", identity.clone());
    doc.author = Some("The document's own author".to_string());
    let id = store.upsert_document(&doc).expect("insert the document");

    let empty_list: [&str; 0] = [];
    let clearings = [
        ("no name at all", AuthorEdit::Decided(None)),
        ("an emptied field", AuthorEdit::Decided(Some(""))),
        ("a field of spaces", AuthorEdit::Decided(Some("   "))),
        ("an empty list", AuthorEdit::DecidedList(&empty_list)),
    ];

    for (how, cleared) in clearings {
        store
            .set_user_naming(id, None, AuthorEdit::Decided(Some("A reader's author")))
            .expect("set the reader's author");
        store
            .set_user_naming(id, None, cleared)
            .expect("clearing the reader's authors is a legitimate action, not an error");

        let found = store
            .find_document_by_identity(&identity)
            .expect("lookup")
            .expect("present");
        assert!(
            found.document.authors.is_empty(),
            "clearing with {how} must leave no reader's author"
        );
        assert_eq!(
            found.document.display_authors(),
            vec!["The document's own author"],
            "an empty list must fall back to the document's own author, cleared with {how}"
        );
    }
}

#[test]
fn an_upsert_never_erases_the_readers_author() {
    // The twin of the title's rescan rule: the scanner's upsert refreshes the document's own metadata,
    // including its own author, but must not touch the author a person chose.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let identity = FileId::Posix { dev: 8, ino: 23 };
    let id = store
        .upsert_document(&document("/library/authored.pdf", identity.clone()))
        .expect("insert the document");
    store
        .set_user_naming(
            id,
            None,
            AuthorEdit::Decided(Some("What the reader called them")),
        )
        .expect("name the author");

    // The rescan knows the same identity and fresher metadata, and nothing about the reader's author.
    let mut rescan = document("/library/authored.pdf", identity.clone());
    rescan.author = Some("The document's own author, as the file embeds it".to_string());
    rescan.pages = Some(120);
    store.upsert_document(&rescan).expect("rescan the document");

    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    assert_eq!(
        found.document.authors,
        vec!["What the reader called them"],
        "a rescan must never rename an author a person chose"
    );
    assert_eq!(
        found.document.display_author().as_deref(),
        Some("What the reader called them"),
        "the reader's author must still be the one displayed after a rescan"
    );
    // The rescan's fresh metadata still landed: the rule is "do not touch the reader's author", not
    // "ignore the rescan".
    assert_eq!(found.document.pages, Some(120));
}

#[test]
fn the_readers_authors_are_one_row_per_person_in_the_readers_order() {
    // R42/S2 and R42/S3 on the write path. The order is the reader's and is never sorted; deduplication
    // is case-insensitive on the trimmed name; the spelling that arrives first wins, so an existing card
    // does not silently change; and the same person typed twice yields one row — in the list and in
    // `authors`.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let identity = FileId::Posix { dev: 7, ino: 41 };
    let id = store
        .upsert_document(&document("/library/coauthored.pdf", identity.clone()))
        .expect("insert the document");

    store
        .set_user_naming(
            id,
            None,
            AuthorEdit::DecidedList(&[
                "Carl Sagan",
                "Ann Druyan",
                "carl sagan",
                "  ANN DRUYAN  ",
                "Carl  Sagan",
            ]),
        )
        .expect("save a list with a case variant and a repeat");
    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    assert_eq!(
        found.document.authors,
        vec!["Carl Sagan", "Ann Druyan", "Carl  Sagan"],
        "trimmed, deduplicated case-insensitively, first spelling wins, order untouched — and inner \
         whitespace is not squashed, because only the ends are trimmed"
    );

    // One row per kept entry: the case variants collapsed into the spelling that arrived first, and the
    // double-space spelling stayed a separate entry because only the ENDS of a name are trimmed.
    let raw = rusqlite::Connection::open(&path).expect("open raw");
    let names: Vec<String> = raw
        .prepare("SELECT name FROM authors ORDER BY id")
        .expect("prepare")
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        names,
        vec!["Carl Sagan", "Ann Druyan", "Carl  Sagan"],
        "one row per person, and the spelling that arrived first is the one kept"
    );
    let list_rows: i64 = raw
        .query_row("SELECT COUNT(*) FROM document_authors", [], |row| {
            row.get(0)
        })
        .expect("count the list rows");
    assert_eq!(
        list_rows, 3,
        "three entries, and none of them the same person twice"
    );

    // The order is the reader's, so the reverse list is a different — and equally legitimate — answer.
    store
        .set_user_naming(
            id,
            None,
            AuthorEdit::DecidedList(&["Ann Druyan", "Carl Sagan"]),
        )
        .expect("save the reversed list");
    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    assert_eq!(
        found.document.authors,
        vec!["Ann Druyan", "Carl Sagan"],
        "the reader's order is preserved on read, never sorted"
    );
    let stored: Vec<(String, i64)> = raw
        .prepare(
            "SELECT a.name, da.position FROM document_authors da \
             JOIN authors a ON a.id = da.author_id ORDER BY da.position",
        )
        .expect("prepare")
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        stored,
        vec![("Ann Druyan".to_string(), 0), ("Carl Sagan".to_string(), 1)],
        "`position` is the reader's order, and it is what the read sorts by"
    );
    let names_after: i64 = raw
        .query_row("SELECT COUNT(*) FROM authors", [], |row| row.get(0))
        .expect("count the authors");
    assert_eq!(
        names_after, 3,
        "a list that only reuses known people adds no author rows"
    );

    // The list is a stored fact, not something a running process remembers: a fresh open reads the
    // same people in the same order back.
    let reopened = Store::open(&path).expect("reopen");
    let found = reopened.document_by_id(id).expect("read").expect("present");
    assert_eq!(found.document.authors, vec!["Ann Druyan", "Carl Sagan"]);
}

#[test]
fn a_blank_author_in_the_list_is_refused_and_nothing_is_written() {
    // R42/S2: a name that is only whitespace is refused, not stored as "". An **empty list** is a
    // different thing — the reader saying *no author of my own* — and the test above pins that it is
    // accepted. A blank entry inside a list is the mistake nobody meant.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let identity = FileId::Posix { dev: 7, ino: 42 };
    let id = store
        .upsert_document(&document("/library/coauthored.pdf", identity.clone()))
        .expect("insert the document");

    for blank in ["", "   ", "\t\n"] {
        let result = store.set_user_naming(
            id,
            Some("A title that must not land"),
            AuthorEdit::DecidedList(&["Carl Sagan", blank]),
        );
        match result {
            Err(MirkError::InvalidInput {
                field,
                value,
                expected,
            }) => {
                assert_eq!(field, "author");
                assert_eq!(value, blank);
                assert!(
                    expected.contains("non-empty"),
                    "the refusal must say what was wanted, got: {expected}"
                );
            }
            other => panic!("a blank author must be an invalid argument, got {other:?}"),
        }
    }

    // The refusal happens before the transaction opens, so nothing was written — not the valid name
    // beside the blank one, and not the title.
    let found = store
        .find_document_by_identity(&identity)
        .expect("lookup")
        .expect("present");
    assert!(found.document.authors.is_empty());
    assert_eq!(found.document.user_title, None);
    let raw = rusqlite::Connection::open(&path).expect("open raw");
    let authors: i64 = raw
        .query_row("SELECT COUNT(*) FROM authors", [], |row| row.get(0))
        .expect("count the authors");
    assert_eq!(authors, 0, "a refused list must not leave a person behind");
}

#[test]
fn the_listing_carries_the_readers_authors_and_the_one_rule_that_decides_what_is_displayed() {
    // R42/S4 at the store's own boundary. The displayed list is the core's rule
    // (`Document::display_authors`), and the listing and the single-document read both carry it — so the
    // card, the filter and the search cannot disagree. All three branches of the precedence are here,
    // and the embedded string is never split (R42/S6).
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    // 1. No reader's list: the embedded string, as ONE entry, commas and semicolons notwithstanding.
    let mut comma = document("/library/a-embedded.pdf", FileId::Posix { dev: 6, ino: 1 });
    comma.author = Some("Sagan, Carl; Druyan, Ann".to_string());
    let comma_id = store.upsert_document(&comma).expect("insert");

    // 2. A reader's list beside an embedded string: the reader's list wins, in the reader's order.
    let mut coauthored = document("/library/b-readers.pdf", FileId::Posix { dev: 6, ino: 2 });
    coauthored.author = Some("The document's own author".to_string());
    let readers_id = store.upsert_document(&coauthored).expect("insert");
    store
        .set_user_naming(
            readers_id,
            None,
            AuthorEdit::DecidedList(&["Ann Druyan", "Carl Sagan"]),
        )
        .expect("name the authors");

    // 3. Neither: no displayed author at all, and nothing invented in its place.
    let none_id = store
        .upsert_document(&document(
            "/library/c-none.pdf",
            FileId::Posix { dev: 6, ino: 3 },
        ))
        .expect("insert");

    // 4. An embedded string that looks like a list and is not one: it is not split on " and " either.
    let mut conjunction = document(
        "/library/d-embedded-and.pdf",
        FileId::Posix { dev: 6, ino: 4 },
    );
    conjunction.author = Some("Carl Sagan and Ann Druyan".to_string());
    let conjunction_id = store.upsert_document(&conjunction).expect("insert");

    let entries = store.library().expect("list the library");
    assert_eq!(entries.len(), 4);

    let embedded = &entries[0].document.document;
    assert_eq!(embedded.authors, Vec::<String>::new());
    assert_eq!(
        embedded.display_authors(),
        vec!["Sagan, Carl; Druyan, Ann"],
        "the embedded string is ONE author, never split on the comma or the semicolon"
    );

    let readers = &entries[1].document.document;
    assert_eq!(readers.authors, vec!["Ann Druyan", "Carl Sagan"]);
    assert_eq!(
        readers.display_authors(),
        vec!["Ann Druyan", "Carl Sagan"],
        "the reader's list wins over the embedded string, in the reader's order"
    );

    let nothing = &entries[2].document.document;
    assert!(nothing.authors.is_empty());
    assert!(
        nothing.display_authors().is_empty(),
        "neither the reader nor the file names an author, so nobody is displayed"
    );

    let and_string = &entries[3].document.document;
    assert_eq!(
        and_string.display_authors(),
        vec!["Carl Sagan and Ann Druyan"],
        "\" and \" is not a delimiter either: inventing two authors is the lie S6 refuses"
    );

    // The single-document read must agree with the listing for every one of them: one rule, one read.
    for (entry, expected_id) in entries
        .iter()
        .zip([comma_id, readers_id, none_id, conjunction_id])
    {
        assert_eq!(entry.document.id, expected_id);
        let by_id = store
            .document_by_id(entry.document.id)
            .expect("read the document")
            .expect("present");
        assert_eq!(
            by_id.document, entry.document.document,
            "a document read by id must be the same document the listing carries"
        );
    }
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

    let recorded = store
        .folders()
        .expect("list folders")
        .pop()
        .expect("one folder");
    assert_eq!(recorded.path, PathBuf::from("/library"));
    assert!(recorded.recursive);
    assert!(
        recorded.enabled,
        "the scanner does not touch the user's enabled flag"
    );

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
    let doomed = store
        .record_folder("/library/doomed", true)
        .expect("record");

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
    assert!(
        removal.kept.is_empty(),
        "no other watch covers this document"
    );

    let folders = store.folders().expect("list folders");
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].id, keep, "only the named watch goes");

    // The counts, not merely the absence of a row: the user asked for the watch's documents to
    // leave the library, so the document, its position and the listing entry all go with it.
    assert_eq!(
        store.document_count().expect("count"),
        0,
        "the document goes"
    );
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
    let outer = store
        .record_folder("/lib", true)
        .expect("record the outer watch");
    let inner = store
        .record_folder("/lib/inner", true)
        .expect("record the inner watch");

    store
        .upsert_document(&document(
            "/lib/direct.pdf",
            FileId::Posix { dev: 1, ino: 1 },
        ))
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
    let outer = store
        .record_folder("/lib", true)
        .expect("record the outer watch");
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
    assert!(store
        .progress_for_document(inside)
        .expect("progress")
        .is_empty());
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
    assert!(store
        .progress_for_document(inside)
        .expect("progress")
        .is_empty());
}

#[test]
fn the_removal_names_the_documents_that_go_and_the_ones_that_stay_sorted_by_path() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let outer = store
        .record_folder("/lib", true)
        .expect("record the outer watch");
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
        .upsert_document(&document(
            "/lib/kept/stays.pdf",
            FileId::Posix { dev: 5, ino: 2 },
        ))
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
    let covering =
        |candidate: &str| covering_folder(&folders, Path::new(candidate)).map(|folder| folder.id);

    // The trap this test exists for: `/library/pdfs-old` shares the entire character prefix
    // `/library/pdfs` and is an unrelated folder. `str::starts_with` says it is inside; comparing
    // whole path components says it is not.
    assert_eq!(covering("/library/pdfs-old"), None);
    // The same trap in a plainer spelling, so the rule cannot pass by special-casing one shape.
    assert_eq!(covering("/library/pdfs-2024"), None);

    // The answers that must be a refusal.
    assert_eq!(
        covering("/library/pdfs"),
        Some(1),
        "the watched folder itself"
    );
    assert_eq!(
        covering("/library/pdfs/book.pdf"),
        Some(1),
        "a real subfolder"
    );
    assert_eq!(
        covering("/library/pdfs/deep/inside"),
        Some(1),
        "a folder nested deeper"
    );

    // ... and the ones that must not be.
    assert_eq!(
        covering("/library"),
        None,
        "the parent is not inside its child"
    );
    assert_eq!(covering("/library/other"), None, "an unrelated sibling");
    assert_eq!(
        covering("/"),
        None,
        "the filesystem root is not inside anything"
    );
}

#[test]
fn a_trailing_separator_on_the_watched_path_changes_nothing() {
    let folders = vec![watched(1, "/library/pdfs/")];
    let covering =
        |candidate: &str| covering_folder(&folders, Path::new(candidate)).map(|folder| folder.id);

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
    let covering =
        |candidate: &str| covering_folder(&folders, Path::new(candidate)).map(|folder| folder.id);

    assert_eq!(covering("/library/pdfs/book"), Some(2));
    assert_eq!(covering("/library/other"), Some(1));
}

// --- UI slice: the library listing (S1) ------------------------------------------------------

#[test]
fn a_document_can_be_fetched_by_its_id_without_pulling_the_whole_library() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    let wanted = store
        .upsert_document(&document(
            "/library/wanted.pdf",
            FileId::Posix { dev: 1, ino: 21 },
        ))
        .expect("insert");
    store
        .upsert_document(&document(
            "/library/other.pdf",
            FileId::Posix { dev: 1, ino: 22 },
        ))
        .expect("insert");

    let found = store.document_by_id(wanted).expect("read");
    let found = found.expect("the document is in the library");
    assert_eq!(found.id, wanted);
    assert_eq!(found.document.path, PathBuf::from("/library/wanted.pdf"));
    assert_eq!(
        found.document.identity,
        Some(FileId::Posix { dev: 1, ino: 21 }),
        "the identity must be decoded exactly as the listing decodes it"
    );
}

#[test]
fn a_document_id_that_names_nothing_is_an_absence_and_not_a_failure() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    // `None` and an error are different answers: the first says "no such document", the second says
    // "the store could not be read". A caller that cannot tell them apart would report a missing
    // document as a broken database.
    assert_eq!(store.document_by_id(404).expect("read"), None);
}

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
        from_viewer
            .progress
            .as_ref()
            .map(|progress| &progress.source),
        Some(&ProgressSource::External("evince-gvfs"))
    );
    assert_eq!(
        from_viewer
            .progress
            .as_ref()
            .map(|progress| progress.confidence),
        Some(Confidence::Exact)
    );
    assert_eq!(
        by_hand.progress.as_ref().map(|progress| &progress.source),
        Some(&ProgressSource::Manual)
    );
    assert_eq!(
        by_hand
            .progress
            .as_ref()
            .map(|progress| progress.confidence),
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

// --- R4: the suggestion queue -----------------------------------------------------------------

#[test]
fn recording_the_same_suggestion_twice_counts_the_sightings_without_duplicating_it() {
    // The detector can see the same document open on every tick, so the queue must remember it once
    // and count the sightings rather than grow a row per sighting.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    let first = store
        .record_suggestion("/library/suggested.pdf")
        .expect("record the first sighting");
    let second = store
        .record_suggestion("/library/suggested.pdf")
        .expect("record the second sighting");
    assert_eq!(first, second, "one path is one suggestion, not two");

    let suggestions = store.suggestions().expect("list the queue");
    assert_eq!(suggestions.len(), 1);
    assert_eq!(suggestions[0].path, PathBuf::from("/library/suggested.pdf"));
    assert_eq!(suggestions[0].seen_count, 2);
    assert!(suggestions[0].decision.is_none(), "nothing decided it");
    assert!(suggestions[0].decided_at.is_none());
}

#[test]
fn a_suggestion_is_decided_once_and_a_second_decision_is_refused() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let id = store
        .record_suggestion("/library/decided.pdf")
        .expect("record");

    let decided = store
        .decide_suggestion(id, SuggestionDecision::Accepted)
        .expect("accept");
    assert_eq!(decided.decision, Some(SuggestionDecision::Accepted));
    assert!(
        decided.decided_at.is_some(),
        "the decision records when it happened"
    );

    // The queue offers only what still needs an answer, so the decided row is absent now.
    assert!(store.suggestions().expect("list").is_empty());

    // And the first answer is the one on record: a second write must be refused, never overwrite it.
    let error = store
        .decide_suggestion(id, SuggestionDecision::Discarded)
        .expect_err("a second decision must be refused");
    let MirkError::SuggestionAlreadyDecided { path, decision } = error else {
        panic!("expected a decided-suggestion error, got {error:?}");
    };
    assert_eq!(path, "/library/decided.pdf");
    assert_eq!(decision, "accepted");

    let existing = store.suggestion(id).expect("read").expect("present");
    assert_eq!(existing.decision, Some(SuggestionDecision::Accepted));
}

#[test]
fn re_detecting_a_decided_suggestion_cannot_erase_what_the_reader_decided() {
    // The load-bearing rule the queue shares with the reader's title, author and cover: observing
    // the same file again is not a reason to revisit a decision the reader already made. The row is
    // returned untouched — no new sighting, no cleared decision, no new decision time.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let id = store
        .record_suggestion("/library/dismissed.pdf")
        .expect("record");
    let decided_at = store
        .decide_suggestion(id, SuggestionDecision::Discarded)
        .expect("dismiss")
        .decided_at;

    let again = store
        .record_suggestion("/library/dismissed.pdf")
        .expect("re-detect");
    assert_eq!(again, id);

    let existing = store.suggestion(id).expect("read").expect("present");
    assert_eq!(existing.decision, Some(SuggestionDecision::Discarded));
    assert_eq!(existing.decided_at, decided_at);
    assert_eq!(
        existing.seen_count, 1,
        "a decided suggestion is not observed again"
    );
    assert!(
        store.suggestions().expect("list").is_empty(),
        "a decided suggestion is never offered again"
    );
}

#[test]
fn the_queue_is_ordered_by_when_each_document_was_first_seen() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    for name in ["/library/b.pdf", "/library/a.pdf", "/library/c.pdf"] {
        store.record_suggestion(name).expect("record");
    }

    let paths: Vec<PathBuf> = store
        .suggestions()
        .expect("list")
        .into_iter()
        .map(|suggestion| suggestion.path)
        .collect();
    assert_eq!(
        paths,
        vec![
            PathBuf::from("/library/b.pdf"),
            PathBuf::from("/library/a.pdf"),
            PathBuf::from("/library/c.pdf"),
        ],
        "insertion order, never the machine's locale"
    );
}

#[test]
fn deciding_a_suggestion_that_does_not_exist_is_a_storage_error() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");

    let error = store
        .decide_suggestion(404, SuggestionDecision::Accepted)
        .expect_err("an unknown id cannot be decided");
    assert!(matches!(error, MirkError::Storage(_)), "got {error:?}");
}

#[test]
fn a_stored_decision_this_build_does_not_know_is_an_error_not_an_undecided_row() {
    // A value a newer build wrote must not be presented as "still awaiting a decision": the reader
    // could then decide again and overwrite the answer already on record. The row is unreadable
    // rather than readable and wrong, exactly as an unknown confidence is.
    let (_dir, path) = temp_db();
    {
        let store = Store::open(&path).expect("open");
        store
            .record_suggestion("/library/future.pdf")
            .expect("record");
    }
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        raw.execute(
            "UPDATE suggestions SET decision = 'maybe' WHERE path = '/library/future.pdf'",
            [],
        )
        .expect("write a decision this build does not know");
    }

    let store = Store::open(&path).expect("reopen");
    let error = store
        .suggestion(1)
        .expect_err("an unknown decision must not read as undecided");
    assert!(matches!(error, MirkError::Storage(_)), "got {error:?}");
}

#[test]
fn a_step_that_fails_on_the_way_to_v8_leaves_the_database_at_v7_untouched() {
    // The same shape as the V4–V7 atomicity tests, one version later. A database is built with the
    // store's own current schema — so `suggestions` already has `decision` — and then only its version
    // stamp is rewound to 7. Reopening it runs the V8 step, `ALTER TABLE suggestions ADD COLUMN
    // decision TEXT`, against a table that already has that column, which SQLite refuses with
    // "duplicate column name". A step stamped before its work would leave the database claiming version
    // 8 with work that did not happen; the transaction must leave the version at 7 and every row
    // untouched, including the reader's decision.
    let (_dir, path) = temp_db();

    let suggestion_id;
    {
        let store = Store::open(&path).expect("build a current database");
        let document_id = store
            .upsert_document(&document(
                "/library/at-risk.pdf",
                FileId::Posix { dev: 7, ino: 70 },
            ))
            .expect("insert the document");
        store
            .record_progress(
                document_id,
                &StoredProgress::new(
                    12,
                    Some(120),
                    ProgressSource::External("evince-gvfs"),
                    Confidence::Exact,
                ),
            )
            .expect("record the position");
        suggestion_id = store
            .record_suggestion("/library/open-outside.pdf")
            .expect("record a suggestion");
        store
            .decide_suggestion(suggestion_id, SuggestionDecision::Discarded)
            .expect("dismiss it");
    }
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        drop_reading_tables(&raw);
        raw.pragma_update(None, "user_version", 7i64)
            .expect("rewind the version stamp to the pre-V8 database shape");
    }

    // 1. The failing step reaches the caller rather than being swallowed or papered over.
    let error =
        Store::open(&path).expect_err("the V8 step must fail when `decision` already exists");
    assert!(
        matches!(error, MirkError::Storage(_)),
        "a failed migration must surface as a storage error, got {error:?}"
    );

    // 2. The version was not stamped ahead of work that did not happen.
    let raw = rusqlite::Connection::open(&path).expect("open raw");
    let version: i64 = raw
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read the version");
    assert_eq!(
        version, 7,
        "a failed V8 step must leave the database at version 7, not claim version 8"
    );

    // 3. Every pre-existing value is still readable — the document, its position and, above all,
    //    the reader's decision, which is the fact this step exists to keep.
    let document_count: i64 = raw
        .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
        .expect("count the documents");
    let progress_count: i64 = raw
        .query_row("SELECT COUNT(*) FROM progress", [], |row| row.get(0))
        .expect("count the positions");
    assert_eq!((document_count, progress_count), (1, 1));

    let (decision, decided_at): (Option<String>, Option<i64>) = raw
        .query_row(
            "SELECT decision, decided_at FROM suggestions WHERE id = ?1",
            rusqlite::params![suggestion_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("the suggestion must survive the failed migration");
    assert_eq!(decision.as_deref(), Some("discarded"));
    assert!(decided_at.is_some(), "the decision time must survive too");

    // 4. No half-applied state: the column the failing step wanted to add is still there exactly
    //    once, and no table was created or dropped.
    let mut statement = raw
        .prepare("SELECT name FROM pragma_table_info('suggestions')")
        .expect("prepare");
    let columns: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect();
    assert_eq!(
        columns
            .iter()
            .filter(|name| name.as_str() == "decision")
            .count(),
        1,
        "the failed ADD COLUMN must not have left a second `decision`; columns: {columns:?}"
    );
}

// --- R19: a page and an image can both be kept, and the source decides -------------------------

#[test]
fn a_page_can_be_the_cover_while_the_image_is_kept() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let id = store
        .upsert_document(&document(
            "/library/cover.pdf",
            FileId::Posix { dev: 5, ino: 1 },
        ))
        .expect("insert");

    store.set_cover_page(id, Some(4)).expect("choose a page");
    store
        .set_cover_image(id, Some("cover.png"))
        .expect("choose an image");
    assert_eq!(
        store
            .document_by_id(id)
            .expect("read")
            .expect("row")
            .cover_choice(),
        CoverChoice::Image("cover.png".to_string()),
        "choosing an image makes it the cover"
    );

    // The user's defect: this used to change a column the choice ignored. Now it takes over, and the
    // image stays exactly where it is.
    store
        .set_cover_page(id, Some(2))
        .expect("choose a page again");
    let record = store.document_by_id(id).expect("read").expect("row");
    assert_eq!(record.cover_choice(), CoverChoice::Page(2));
    assert_eq!(
        record.document.cover_image.as_deref(),
        Some("cover.png"),
        "the image is kept, not deleted"
    );

    // And back to the image without choosing the file again.
    store
        .set_cover_source(id, CoverSource::Image)
        .expect("use the stored image");
    assert_eq!(
        store
            .document_by_id(id)
            .expect("read")
            .expect("row")
            .cover_choice(),
        CoverChoice::Image("cover.png".to_string())
    );

    // Removing the image hands the cover back to the page the reader had.
    store.set_cover_image(id, None).expect("remove the image");
    assert_eq!(
        store
            .document_by_id(id)
            .expect("read")
            .expect("row")
            .cover_choice(),
        CoverChoice::Page(2)
    );
}

#[test]
fn a_v9_database_gets_its_cover_source_from_the_image_it_had() {
    let (_dir, path) = temp_db();
    let (with_image, without) = {
        let store = Store::open(&path).expect("build a current database");
        let with_image = store
            .upsert_document(&document(
                "/library/with.png",
                FileId::Posix { dev: 6, ino: 1 },
            ))
            .expect("insert");
        store
            .set_cover_image(with_image, Some("chosen.png"))
            .expect("choose an image");
        let without = store
            .upsert_document(&document(
                "/library/without.pdf",
                FileId::Posix { dev: 6, ino: 2 },
            ))
            .expect("insert");
        store
            .set_cover_page(without, Some(3))
            .expect("choose a page");
        (with_image, without)
    };
    {
        // The pre-V10 shape: the column does not exist, and neither does the choice it records.
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        drop_reading_tables(&raw);
        raw.execute_batch("ALTER TABLE documents DROP COLUMN cover_source;")
            .expect("rebuild the V9 documents table");
        raw.pragma_update(None, "user_version", 9i64)
            .expect("rewind the version stamp");
    }

    let store = Store::open(&path).expect("a V9 database migrates");
    // The backfill reproduces the old rule exactly: an image means image, otherwise the page. A
    // library opens on the same cover it showed before the upgrade.
    assert_eq!(
        store
            .document_by_id(with_image)
            .expect("read")
            .expect("row")
            .cover_choice(),
        CoverChoice::Image("chosen.png".to_string())
    );
    assert_eq!(
        store
            .document_by_id(without)
            .expect("read")
            .expect("row")
            .cover_choice(),
        CoverChoice::Page(3)
    );
}

// --- F1 part 1: the schema for the facts only mirk's own reader can honestly produce -------------
//
// This build has no capture: nothing in the workspace records a session or a visit yet. What these
// tests pin is the storage contract those facts will be written under — the tables, the values they
// accept, what leaves with a document, and the guard that refuses a database whose shape is not the
// one this build writes. The facts are therefore written through SQLite directly: the constraints
// have to hold for a writer that bypassed `Store`, so testing them through a recording method would
// test the wrong boundary.

/// Opens the database file directly, with foreign keys enforced as the store's own connection does.
fn raw_connection(path: &Path) -> rusqlite::Connection {
    let connection = rusqlite::Connection::open(path).expect("open raw");
    connection
        .pragma_update(None, "foreign_keys", true)
        .expect("enable foreign keys");
    connection
}

/// Every user table of a database file, read without opening it through `Store`.
///
/// `Store::table_names` would migrate the file on the way in, which is exactly what a test of a
/// *refused* open must not do.
fn raw_table_names(path: &Path) -> Vec<String> {
    let connection = rusqlite::Connection::open(path).expect("open raw");
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .expect("prepare");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect()
}

/// The version stamp a database file carries, read without migrating it.
fn raw_version(path: &Path) -> i64 {
    let connection = rusqlite::Connection::open(path).expect("open raw");
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read the version stamp")
}

#[test]
fn the_schema_refuses_a_reading_fact_a_typed_read_could_not_represent() {
    // The constraints are what makes a stored row a fact even when the writer bypassed this crate.
    // Three groups: the numeric bounds (`u32` pages, non-negative times and durations), the SQLite
    // storage class an INTEGER column otherwise happily accepts (text, a real), and the
    // same-document rule between a visit and the session that gives it provenance.
    let (_dir, path) = temp_db();
    let (read, other);
    {
        let store = Store::open(&path).expect("open");
        read = store
            .upsert_document(&document(
                "/library/read.pdf",
                FileId::Posix { dev: 20, ino: 1 },
            ))
            .expect("insert");
        other = store
            .upsert_document(&document(
                "/library/other.pdf",
                FileId::Posix { dev: 20, ino: 2 },
            ))
            .expect("insert");
    }

    let raw = raw_connection(&path);
    raw.execute(
        "INSERT INTO reading_sessions (id, doc_id, started_at, ended_at, active_ms) \
         VALUES (1, ?1, 1700000000, 1700000400, 300000)",
        rusqlite::params![read],
    )
    .expect("record a session");
    raw.execute(
        "INSERT INTO reading_sessions (id, doc_id, started_at, ended_at, active_ms) \
         VALUES (2, ?1, 1700000000, 1700000100, 90000)",
        rusqlite::params![other],
    )
    .expect("record another document's session");

    let impossible_sessions = [
        (
            "INSERT INTO reading_sessions (doc_id, started_at, ended_at, active_ms) \
             VALUES (?1, -100, 100, 10)",
            "a start before the epoch",
        ),
        (
            "INSERT INTO reading_sessions (doc_id, started_at, ended_at, active_ms) \
             VALUES (?1, -100, -50, 10)",
            "an end before the epoch",
        ),
        (
            "INSERT INTO reading_sessions (doc_id, started_at, ended_at, active_ms) \
             VALUES (?1, 100, 200, -1)",
            "a negative active duration",
        ),
        (
            "INSERT INTO reading_sessions (doc_id, started_at, ended_at, active_ms) \
             VALUES (?1, 'abc', 200, 10)",
            "a text start time",
        ),
        (
            "INSERT INTO reading_sessions (doc_id, started_at, ended_at, active_ms) \
             VALUES (?1, 100, 200, 1.5)",
            "a fractional active duration",
        ),
    ];
    for (statement, what) in impossible_sessions {
        assert!(
            raw.execute(statement, rusqlite::params![read]).is_err(),
            "the schema must refuse {what}: `{statement}`"
        );
    }

    let impossible_visits = [
        (
            "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
             VALUES (?1, ?2, -1, 100, 10)",
            "a negative page",
        ),
        (
            "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
             VALUES (?1, ?2, 4294967296, 100, 10)",
            "a page beyond the u32 bound",
        ),
        (
            "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
             VALUES (?1, ?2, 0, -1, 10)",
            "an observation before the epoch",
        ),
        (
            "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
             VALUES (?1, ?2, 0, 100, -1)",
            "a negative visible duration",
        ),
        (
            "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
             VALUES (?1, ?2, 'abc', 100, 10)",
            "a text page",
        ),
        (
            "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
             VALUES (?1, ?2, 0, 'abc', 10)",
            "a text observation time",
        ),
        (
            "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
             VALUES (?1, ?2, 0, 100, 1.5)",
            "a fractional visible duration",
        ),
    ];
    for (statement, what) in impossible_visits {
        assert!(
            raw.execute(statement, rusqlite::params![read, 1i64])
                .is_err(),
            "the schema must refuse {what}: `{statement}`"
        );
    }

    // A visit whose session belongs to a different document claims provenance it does not have.
    assert!(
        raw.execute(
            "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
             VALUES (?1, ?2, 0, 100, 10)",
            rusqlite::params![read, 2i64],
        )
        .is_err(),
        "the schema must refuse a session of another document"
    );
    // And references to rows that do not exist.
    for (params, what) in [
        (rusqlite::params![read, 9_999i64], "an unknown session"),
        (rusqlite::params![9_999i64, 1i64], "an unknown document"),
    ] {
        assert!(
            raw.execute(
                "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
                 VALUES (?1, ?2, 0, 100, 10)",
                params,
            )
            .is_err(),
            "the schema must refuse {what}"
        );
    }

    // Nothing malformed was stored, so a typed read can never meet one of these rows.
    assert_eq!(row_count(&path, "reading_sessions"), 2);
    assert_eq!(row_count(&path, "page_visits"), 0);

    // The constraints are not a blanket refusal: the same row with the session of its own document
    // is exactly what the schema exists to hold, and it comes back with every value intact.
    raw.execute(
        "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
         VALUES (?1, ?2, 0, 100, 10)",
        rusqlite::params![read, 1i64],
    )
    .expect("the schema must accept a well-formed visit");
    let stored: (i64, i64, i64, i64) = raw
        .query_row(
            "SELECT session_id, page, observed_at, visible_ms FROM page_visits",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("read the visit back");
    assert_eq!(stored, (1, 0, 100, 10));
}

#[test]
fn a_session_whose_wall_clock_went_backwards_is_stored_as_given() {
    // `started_at` and `ended_at` are wall-clock readings, and the UTC clock can be corrected
    // backwards between them: an NTP correction, a manual change, a suspend and resume. What the
    // reader can honestly report is the two readings plus the separately measured `active_ms`, and
    // elapsed reading time is not `ended_at - started_at`, so the schema must not require the span to
    // be monotonic. The timestamps are still non-negative and the durations are still non-negative.
    let (_dir, path) = temp_db();
    let read;
    {
        let store = Store::open(&path).expect("open");
        read = store
            .upsert_document(&document(
                "/library/read.pdf",
                FileId::Posix { dev: 33, ino: 1 },
            ))
            .expect("insert");
    }

    let raw = raw_connection(&path);
    raw.execute(
        "INSERT INTO reading_sessions (doc_id, started_at, ended_at, active_ms) \
         VALUES (?1, 200, 100, 0)",
        rusqlite::params![read],
    )
    .expect("a span whose wall clock moved backwards is a fact, not a violation");
    let stored: (i64, i64, i64) = raw
        .query_row(
            "SELECT started_at, ended_at, active_ms FROM reading_sessions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("read the session back");
    assert_eq!(stored, (200, 100, 0));

    // A negative timestamp is still not a time, so the real bounds keep holding.
    assert!(
        raw.execute(
            "INSERT INTO reading_sessions (doc_id, started_at, ended_at, active_ms) \
             VALUES (?1, -1, 100, 0)",
            rusqlite::params![read],
        )
        .is_err(),
        "a negative start remains impossible"
    );
}

#[test]
fn deleting_a_document_or_a_session_takes_the_reading_facts_it_owns_with_it() {
    // Reading time that nothing can attribute is not a fact worth keeping: a document that leaves
    // the library takes its sessions and visits, and a session that goes takes the visits it gave
    // provenance to. Both rules are the schema's, so they hold for a writer that bypassed `Store`.
    let (_dir, path) = temp_db();
    let (first, second);
    {
        let store = Store::open(&path).expect("open");
        first = store
            .upsert_document(&document(
                "/library/first.pdf",
                FileId::Posix { dev: 22, ino: 1 },
            ))
            .expect("insert");
        second = store
            .upsert_document(&document(
                "/library/second.pdf",
                FileId::Posix { dev: 22, ino: 2 },
            ))
            .expect("insert");
    }

    let raw = raw_connection(&path);
    for (session_id, doc_id) in [(1i64, first), (2i64, second)] {
        raw.execute(
            "INSERT INTO reading_sessions (id, doc_id, started_at, ended_at, active_ms) \
             VALUES (?1, ?2, 1700000000, 1700000300, 240000)",
            rusqlite::params![session_id, doc_id],
        )
        .expect("record a session");
        raw.execute(
            "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
             VALUES (?1, ?2, 1, 1700000100, 1000)",
            rusqlite::params![doc_id, session_id],
        )
        .expect("record a visit");
    }
    assert_eq!(row_count(&path, "reading_sessions"), 2);
    assert_eq!(row_count(&path, "page_visits"), 2);

    // A document leaving the library takes its session and its visit, and nothing else.
    raw.execute(
        "DELETE FROM documents WHERE id = ?1",
        rusqlite::params![first],
    )
    .expect("remove the first document");
    let remaining_sessions: Vec<i64> = {
        let mut statement = raw
            .prepare("SELECT id FROM reading_sessions ORDER BY id")
            .expect("prepare");
        statement
            .query_map([], |row| row.get::<_, i64>(0))
            .expect("query")
            .map(|row| row.expect("row"))
            .collect()
    };
    assert_eq!(remaining_sessions, vec![2]);
    let remaining_visits: Vec<i64> = {
        let mut statement = raw
            .prepare("SELECT session_id FROM page_visits ORDER BY session_id")
            .expect("prepare");
        statement
            .query_map([], |row| row.get::<_, i64>(0))
            .expect("query")
            .map(|row| row.expect("row"))
            .collect()
    };
    assert_eq!(remaining_visits, vec![2]);

    // A session leaving takes its visits; the document it belonged to stays.
    raw.execute(
        "DELETE FROM reading_sessions WHERE id = ?1",
        rusqlite::params![2i64],
    )
    .expect("remove the second session");
    assert_eq!(row_count(&path, "page_visits"), 0);
    assert_eq!(row_count(&path, "documents"), 1);
    assert_eq!(
        raw.query_row("SELECT id FROM documents", [], |row| row.get::<_, i64>(0))
            .expect("the second document is untouched"),
        second
    );
}

#[test]
fn deleting_one_session_takes_only_its_visits_and_keeps_its_siblings_visits() {
    // Two sessions of the same document, each with its own visit. Deleting one session must take the
    // visits it gave provenance to and nothing else: the sibling session's visit, for the very same
    // document, is a different fact and must survive. The composite key `(session_id, doc_id)` is what
    // makes the cascade that precise — a cascade keyed only by `doc_id` would delete both visits.
    let (_dir, path) = temp_db();
    let read;
    {
        let store = Store::open(&path).expect("open");
        read = store
            .upsert_document(&document(
                "/library/read.pdf",
                FileId::Posix { dev: 34, ino: 1 },
            ))
            .expect("insert");
    }

    let raw = raw_connection(&path);
    for (session_id, page) in [(1i64, 2i64), (2i64, 7i64)] {
        raw.execute(
            "INSERT INTO reading_sessions (id, doc_id, started_at, ended_at, active_ms) \
             VALUES (?1, ?2, 1700000000, 1700000300, 240000)",
            rusqlite::params![session_id, read],
        )
        .expect("record a session of the same document");
        raw.execute(
            "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
             VALUES (?1, ?2, ?3, 1700000100, 1000)",
            rusqlite::params![read, session_id, page],
        )
        .expect("record a visit in that session");
    }
    assert_eq!(row_count(&path, "reading_sessions"), 2);
    assert_eq!(row_count(&path, "page_visits"), 2);

    raw.execute(
        "DELETE FROM reading_sessions WHERE id = ?1",
        rusqlite::params![1i64],
    )
    .expect("remove one session of the document");

    assert_eq!(row_count(&path, "reading_sessions"), 1);
    assert_eq!(row_count(&path, "page_visits"), 1);
    // The surviving facts are the sibling session and *its* visit, and they still name the same
    // document.
    let surviving: (i64, i64, i64, i64) = raw
        .query_row(
            "SELECT v.doc_id, v.session_id, v.page, v.visible_ms \
             FROM page_visits v JOIN reading_sessions s ON s.id = v.session_id",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .expect("the sibling session's visit must survive");
    assert_eq!(surviving, (read, 2, 7, 1000));
}

#[test]
fn removing_a_watched_folder_takes_the_reading_facts_of_its_documents_with_it() {
    // The same rule through the interface that actually removes documents: forgetting a watch
    // deletes the documents it covered, and the reading facts go with them through the store's own
    // foreign keys — while a document outside the watch keeps every fact.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let watched = store.record_folder("/library", true).expect("record");

    let inside = store
        .upsert_document(&document(
            "/library/inside.pdf",
            FileId::Posix { dev: 23, ino: 1 },
        ))
        .expect("insert");
    let outside = store
        .upsert_document(&document(
            "/elsewhere/outside.pdf",
            FileId::Posix { dev: 23, ino: 2 },
        ))
        .expect("insert");

    {
        let raw = raw_connection(&path);
        for (session_id, doc_id) in [(1i64, inside), (2i64, outside)] {
            raw.execute(
                "INSERT INTO reading_sessions (id, doc_id, started_at, ended_at, active_ms) \
                 VALUES (?1, ?2, 1700000000, 1700000300, 240000)",
                rusqlite::params![session_id, doc_id],
            )
            .expect("record a session");
            raw.execute(
                "INSERT INTO page_visits (doc_id, session_id, page, observed_at, visible_ms) \
                 VALUES (?1, ?2, 1, 1700000100, 1000)",
                rusqlite::params![doc_id, session_id],
            )
            .expect("record a visit");
        }
    }

    store
        .forget_folder(watched)
        .expect("forget")
        .expect("the watch was known");

    assert!(store.document_by_id(inside).expect("read").is_none());
    assert_eq!(row_count(&path, "reading_sessions"), 1);
    assert_eq!(row_count(&path, "page_visits"), 1);
    let raw = raw_connection(&path);
    assert_eq!(
        raw.query_row("SELECT doc_id FROM reading_sessions", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("the surviving session belongs to the document outside the watch"),
        outside
    );
    assert_eq!(
        raw.query_row("SELECT doc_id FROM page_visits", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("the surviving visit belongs to the document outside the watch"),
        outside
    );
}

// --- F1 part 1: the typed store API for first-party reading sessions --------------------------
//
// The schema tests above write through SQLite directly, because the constraints must hold for a
// writer that bypassed `Store`. These tests are the other boundary: the public store interface the
// reader will call, which must refuse what cannot be a fact before it reaches the database, and
// must store the two wall-clock readings exactly as reported.

#[test]
fn a_stored_session_returns_its_stable_id_and_facts_across_a_reopen() {
    // A session's id is the provenance a page visit will name, so the read must return it. It is
    // assigned on insert and never chosen by the caller; a close and reopen must not change it.
    let (_dir, path) = temp_db();
    let read;
    let other;
    let later_id;
    let earlier_id;
    let same_second_id;
    let other_id;
    let later = ReadingSession {
        doc_id: 0,
        started_at: 1_700_000_600,
        ended_at: 1_700_001_200,
        active_ms: 480_000,
    };
    let earlier = ReadingSession {
        doc_id: 0,
        started_at: 1_700_000_000,
        ended_at: 1_700_000_300,
        active_ms: 240_000,
    };
    let same_second = ReadingSession {
        doc_id: 0,
        started_at: 1_700_000_000,
        ended_at: 1_700_000_100,
        active_ms: 10_000,
    };
    let other_session = ReadingSession {
        doc_id: 0,
        started_at: 1_700_000_100,
        ended_at: 1_700_000_200,
        active_ms: 100_000,
    };
    {
        let store = Store::open(&path).expect("open");
        read = store
            .upsert_document(&document(
                "/library/read.pdf",
                FileId::Posix { dev: 44, ino: 1 },
            ))
            .expect("insert");
        other = store
            .upsert_document(&document(
                "/library/other.pdf",
                FileId::Posix { dev: 44, ino: 2 },
            ))
            .expect("insert");

        // Written newest first, and two sessions start in the same second, so the read order can
        // only be the documented (started_at, id) order.
        later_id = store
            .record_reading_session(&ReadingSession {
                doc_id: read,
                ..later.clone()
            })
            .expect("record the later session");
        earlier_id = store
            .record_reading_session(&ReadingSession {
                doc_id: read,
                ..earlier.clone()
            })
            .expect("record the earlier session");
        same_second_id = store
            .record_reading_session(&ReadingSession {
                doc_id: read,
                ..same_second.clone()
            })
            .expect("record the same-second session");
        other_id = store
            .record_reading_session(&ReadingSession {
                doc_id: other,
                ..other_session.clone()
            })
            .expect("record another document's session");
    }

    let store = Store::open(&path).expect("reopen");
    assert_eq!(
        store.reading_sessions_for_document(read).expect("read"),
        vec![
            StoredReadingSession {
                id: earlier_id,
                session: ReadingSession {
                    doc_id: read,
                    ..earlier
                },
            },
            StoredReadingSession {
                id: same_second_id,
                session: ReadingSession {
                    doc_id: read,
                    ..same_second
                },
            },
            StoredReadingSession {
                id: later_id,
                session: ReadingSession {
                    doc_id: read,
                    ..later
                },
            },
        ],
        "every returned id is the one insertion returned, the same-second tie keeps insertion order, \
         and each session keeps its own facts across a reopen"
    );
    assert_eq!(
        store.reading_sessions_for_document(other).expect("read"),
        vec![StoredReadingSession {
            id: other_id,
            session: ReadingSession {
                doc_id: other,
                ..other_session
            },
        }],
        "another document's session is filtered out and keeps its own id"
    );
    assert!(store
        .reading_sessions_for_document(9_999)
        .expect("read")
        .is_empty());

    // Ids are the database's to assign, are stable, and are distinct per row.
    let mut ids = vec![earlier_id, same_second_id, later_id, other_id];
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 4, "each stored session has its own stable id");
}

#[test]
fn a_reading_session_round_trips_in_a_deterministic_order() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let read = store
        .upsert_document(&document(
            "/library/read.pdf",
            FileId::Posix { dev: 40, ino: 1 },
        ))
        .expect("insert");
    let other = store
        .upsert_document(&document(
            "/library/other.pdf",
            FileId::Posix { dev: 40, ino: 2 },
        ))
        .expect("insert");

    // Written newest first, so the read order cannot be the write order. The two sessions that
    // start in the same second pin the tie-break: row id, so a repeated read never reorders them.
    let later = ReadingSession {
        doc_id: read,
        started_at: 1_700_000_600,
        ended_at: 1_700_001_200,
        active_ms: 480_000,
    };
    let earlier = ReadingSession {
        doc_id: read,
        started_at: 1_700_000_000,
        ended_at: 1_700_000_300,
        active_ms: 240_000,
    };
    let same_second = ReadingSession {
        doc_id: read,
        started_at: 1_700_000_000,
        ended_at: 1_700_000_100,
        active_ms: 10_000,
    };
    let other_session = ReadingSession {
        doc_id: other,
        started_at: 1_700_000_100,
        ended_at: 1_700_000_200,
        active_ms: 100_000,
    };
    let later_id = store
        .record_reading_session(&later)
        .expect("record the later session");
    let earlier_id = store
        .record_reading_session(&earlier)
        .expect("record the earlier session");
    let same_second_id = store
        .record_reading_session(&same_second)
        .expect("record the same-second session");
    let other_id = store
        .record_reading_session(&other_session)
        .expect("record another document's session");

    assert_eq!(
        store.reading_sessions_for_document(read).expect("read"),
        vec![
            StoredReadingSession {
                id: earlier_id,
                session: earlier,
            },
            StoredReadingSession {
                id: same_second_id,
                session: same_second,
            },
            StoredReadingSession {
                id: later_id,
                session: later,
            },
        ],
        "sessions are ordered by start time and then by row id, each keeping its own values"
    );
    assert_eq!(
        store.reading_sessions_for_document(other).expect("read"),
        vec![StoredReadingSession {
            id: other_id,
            session: other_session,
        }],
        "one document's sessions never appear under another"
    );
    // An id no document carries is an empty answer, not an error: nothing was ever recorded there.
    assert!(store
        .reading_sessions_for_document(9_999)
        .expect("read")
        .is_empty());
}

#[test]
fn a_session_whose_wall_clock_went_backwards_round_trips_through_the_store() {
    // The UTC clock can be corrected backwards between the two readings: an NTP correction, a
    // manual change, a suspend and resume. Elapsed reading time is not `ended_at - started_at`, so
    // the store must keep an honest backwards span rather than reject it or normalise it.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let read = store
        .upsert_document(&document(
            "/library/read.pdf",
            FileId::Posix { dev: 41, ino: 1 },
        ))
        .expect("insert");

    let corrected_back = ReadingSession {
        doc_id: read,
        started_at: 1_700_000_500,
        ended_at: 1_700_000_100,
        active_ms: 90_000,
    };
    let backwards_to_the_epoch = ReadingSession {
        doc_id: read,
        started_at: 200,
        ended_at: 100,
        active_ms: 0,
    };
    let corrected_back_id = store
        .record_reading_session(&corrected_back)
        .expect("a backwards correction is a fact, not a violation");
    let backwards_to_the_epoch_id = store
        .record_reading_session(&backwards_to_the_epoch)
        .expect("a span ending before it started is still two recorded readings");

    assert_eq!(
        store.reading_sessions_for_document(read).expect("read"),
        vec![
            StoredReadingSession {
                id: backwards_to_the_epoch_id,
                session: backwards_to_the_epoch,
            },
            StoredReadingSession {
                id: corrected_back_id,
                session: corrected_back,
            },
        ],
        "both readings are stored verbatim and the measured active time is never derived from them"
    );
}

#[test]
fn an_invalid_session_is_refused_without_touching_stored_facts() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let read = store
        .upsert_document(&document(
            "/library/read.pdf",
            FileId::Posix { dev: 42, ino: 1 },
        ))
        .expect("insert");

    let valid = ReadingSession {
        doc_id: read,
        started_at: 1_700_000_000,
        ended_at: 1_700_000_300,
        active_ms: 240_000,
    };
    let valid_id = store
        .record_reading_session(&valid)
        .expect("record a valid session");

    let refused_cases = [
        (
            ReadingSession {
                doc_id: read + 1,
                ..valid.clone()
            },
            "document",
        ),
        (
            ReadingSession {
                started_at: -1,
                ..valid.clone()
            },
            "started_at",
        ),
        (
            ReadingSession {
                ended_at: -1,
                ..valid.clone()
            },
            "ended_at",
        ),
        (
            ReadingSession {
                active_ms: -1,
                ..valid.clone()
            },
            "active_ms",
        ),
    ];
    for (session, field) in refused_cases {
        let refused = store.record_reading_session(&session);
        assert!(
            matches!(&refused, Err(MirkError::InvalidInput { field: named, .. }) if *named == field),
            "`{session:?}` must be refused as `{field}`, got {refused:?}"
        );
    }

    // Nothing refused was written, and the one valid row is exactly as it was.
    assert_eq!(row_count(&path, "reading_sessions"), 1);
    assert_eq!(
        store.reading_sessions_for_document(read).expect("read"),
        vec![StoredReadingSession {
            id: valid_id,
            session: valid,
        }]
    );
}

#[test]
fn recording_a_session_leaves_existing_document_and_progress_reads_unchanged() {
    let (_dir, path) = temp_db();
    let read;
    let progress;
    let session_id;
    {
        let store = Store::open(&path).expect("open");
        read = store
            .upsert_document(&document(
                "/library/read.pdf",
                FileId::Posix { dev: 43, ino: 1 },
            ))
            .expect("insert");
        progress = StoredProgress::new(3, Some(80), ProgressSource::Manual, Confidence::Manual);
        store
            .record_progress(read, &progress)
            .expect("record a reading position");
        session_id = store
            .record_reading_session(&ReadingSession {
                doc_id: read,
                started_at: 1_700_000_000,
                ended_at: 1_700_000_300,
                active_ms: 240_000,
            })
            .expect("record a session");

        // Recording reading time is a fact of its own: it neither rewrites the position nor
        // changes what the document and library reads report.
        assert_eq!(
            store.progress_for_document(read).expect("read progress"),
            vec![progress.clone()]
        );
        assert_eq!(store.document_count().expect("count"), 1);
        assert_eq!(store.library().expect("library").len(), 1);
        assert!(store.document_by_id(read).expect("read").is_some());
    }

    // A session is a stored fact, so reopening the database keeps it and its id.
    let store = Store::open(&path).expect("reopen");
    assert_eq!(
        store.reading_sessions_for_document(read).expect("read"),
        vec![StoredReadingSession {
            id: session_id,
            session: ReadingSession {
                doc_id: read,
                started_at: 1_700_000_000,
                ended_at: 1_700_000_300,
                active_ms: 240_000,
            },
        }]
    );
    assert_eq!(
        store.progress_for_document(read).expect("read progress"),
        vec![progress]
    );
}

// --- F1 part 1: the typed store API for page visits -------------------------------------------
//
// A visit is the per-document depth the roadmap asks for: which page was actually on screen, in
// which session, when it was observed, and for how long. These tests are the boundary the reader
// will call: the store refuses what cannot be a fact before reaching the database, stores the two
// recorded times exactly as reported, and returns the id the database assigned.

/// A read's rows as `(id, facts)`, so an assertion states both without repeating every literal.
fn visits(rows: Vec<StoredPageVisit>) -> Vec<(i64, PageVisit)> {
    rows.into_iter().map(|row| (row.id, row.visit)).collect()
}

/// The same projection for the session reads used to prove a visit did not restate its session.
fn sessions(rows: Vec<StoredReadingSession>) -> Vec<(i64, ReadingSession)> {
    rows.into_iter().map(|row| (row.id, row.session)).collect()
}

#[test]
fn a_stored_page_visit_returns_its_stable_id_and_facts_across_a_reopen() {
    // A visit's id is the row's own identity, so a read has to return it alongside the facts. It is
    // assigned on insert and never chosen by the caller; a close and reopen must not change it.
    let (_dir, path) = temp_db();
    let read;
    let other;
    let read_session;
    let earlier;
    let same_moment;
    let later;
    let other_visit;
    let earlier_id;
    let same_moment_id;
    let later_id;
    let other_visit_id;
    {
        let store = Store::open(&path).expect("open");
        read = store
            .upsert_document(&document(
                "/library/read.pdf",
                FileId::Posix { dev: 60, ino: 1 },
            ))
            .expect("insert");
        other = store
            .upsert_document(&document(
                "/library/other.pdf",
                FileId::Posix { dev: 60, ino: 2 },
            ))
            .expect("insert");
        read_session = store
            .record_reading_session(&ReadingSession {
                doc_id: read,
                started_at: 1_700_000_000,
                ended_at: 1_700_001_200,
                active_ms: 480_000,
            })
            .expect("record the reading session");
        let other_session = store
            .record_reading_session(&ReadingSession {
                doc_id: other,
                started_at: 1_700_000_000,
                ended_at: 1_700_000_600,
                active_ms: 120_000,
            })
            .expect("record another document's session");

        earlier = PageVisit {
            doc_id: read,
            session_id: read_session,
            page: 3,
            observed_at: 1_700_000_100,
            visible_ms: 40_000,
        };
        same_moment = PageVisit {
            doc_id: read,
            session_id: read_session,
            page: 4,
            observed_at: 1_700_000_100,
            visible_ms: 5_000,
        };
        later = PageVisit {
            doc_id: read,
            session_id: read_session,
            page: 12,
            observed_at: 1_700_000_900,
            visible_ms: 30_000,
        };
        // Page 0 is the first page of a zero-based document, exactly as `progress.page` is.
        other_visit = PageVisit {
            doc_id: other,
            session_id: other_session,
            page: 0,
            observed_at: 1_700_000_050,
            visible_ms: 1_500,
        };

        // Written newest first, with two pages observed in the same second, so the read order can
        // only be the documented (observed_at, id) order rather than the insertion order.
        later_id = store
            .record_page_visit(&later)
            .expect("record the later visit");
        earlier_id = store
            .record_page_visit(&earlier)
            .expect("record the earlier visit");
        same_moment_id = store
            .record_page_visit(&same_moment)
            .expect("record the visit observed in the same second");
        other_visit_id = store
            .record_page_visit(&other_visit)
            .expect("record another document's visit");
    }

    let store = Store::open(&path).expect("reopen");
    assert_eq!(
        visits(store.page_visits_for_document(read).expect("read visits")),
        vec![
            (earlier_id, earlier),
            (same_moment_id, same_moment),
            (later_id, later),
        ],
        "every returned id is the one insertion returned, the same-moment tie keeps insertion \
         order, and each visit keeps its own facts and session across a reopen"
    );
    assert_eq!(
        visits(store.page_visits_for_document(other).expect("read visits")),
        vec![(other_visit_id, other_visit)],
        "another document's visit is filtered out and keeps its own id"
    );
    assert!(
        store
            .page_visits_for_document(9_999)
            .expect("read visits")
            .is_empty(),
        "an id no document carries is an empty answer, not an error"
    );

    // Ids are the database's to assign, are stable, and are distinct per row.
    let mut ids = vec![later_id, earlier_id, same_moment_id, other_visit_id];
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 4, "each stored visit has its own stable id");
}

#[test]
fn a_visit_keeps_its_own_moment_and_may_name_an_earlier_session() {
    // The reader flushes visits late, so a visit can reach the store after a later session already
    // started and still belong to the earlier one. And `observed_at` is a UTC wall-clock reading:
    // the clock can be corrected or moved, so a visit is not required to fall inside its session's
    // span, and a session whose span runs backwards is no reason to refuse the moment it was seen.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let read = store
        .upsert_document(&document(
            "/library/read.pdf",
            FileId::Posix { dev: 61, ino: 1 },
        ))
        .expect("insert");

    let first = ReadingSession {
        doc_id: read,
        started_at: 1_700_000_000,
        ended_at: 1_700_000_300,
        active_ms: 240_000,
    };
    let second = ReadingSession {
        doc_id: read,
        started_at: 1_700_000_600,
        ended_at: 1_700_001_200,
        active_ms: 480_000,
    };
    let corrected_back = ReadingSession {
        doc_id: read,
        started_at: 1_700_010_000,
        ended_at: 1_700_009_000,
        active_ms: 60_000,
    };
    let first_session = store
        .record_reading_session(&first)
        .expect("record the first session");
    let second_session = store
        .record_reading_session(&second)
        .expect("record the second session");
    let corrected_back_session = store
        .record_reading_session(&corrected_back)
        .expect("a span whose clock was corrected backwards is a fact");

    let during_the_first = PageVisit {
        doc_id: read,
        session_id: first_session,
        page: 3,
        observed_at: 1_700_000_100,
        visible_ms: 40_000,
    };
    // Read in the first session and recorded only after the second one had already started.
    let flushed_late = PageVisit {
        doc_id: read,
        session_id: first_session,
        page: 4,
        observed_at: 1_700_000_200,
        visible_ms: 20_000,
    };
    // Before its session started and long after it ended: both are recorded readings, not errors.
    let before_its_session = PageVisit {
        doc_id: read,
        session_id: second_session,
        page: 5,
        observed_at: 1_700_000_400,
        visible_ms: 1_000,
    };
    let after_its_session = PageVisit {
        doc_id: read,
        session_id: second_session,
        page: 6,
        observed_at: 1_700_009_900,
        visible_ms: 2_000,
    };
    // Inside a span whose two readings run backwards: still a moment the reader observed.
    let inside_a_corrected_span = PageVisit {
        doc_id: read,
        session_id: corrected_back_session,
        page: 7,
        observed_at: 1_700_009_500,
        visible_ms: 3_000,
    };

    let during_the_first_id = store
        .record_page_visit(&during_the_first)
        .expect("record a visit of the first session");
    let flushed_late_id = store
        .record_page_visit(&flushed_late)
        .expect("a visit recorded after a later session began still names the earlier one");
    let before_its_session_id = store
        .record_page_visit(&before_its_session)
        .expect("an observation before its session started is a recorded reading");
    let after_its_session_id = store
        .record_page_visit(&after_its_session)
        .expect("an observation after its session ended is a recorded reading");
    let inside_a_corrected_span_id = store
        .record_page_visit(&inside_a_corrected_span)
        .expect("a corrected wall clock is not a reason to lose a visit");

    assert_eq!(
        visits(store.page_visits_for_document(read).expect("read visits")),
        vec![
            (during_the_first_id, during_the_first),
            (flushed_late_id, flushed_late),
            (before_its_session_id, before_its_session),
            (inside_a_corrected_span_id, inside_a_corrected_span),
            (after_its_session_id, after_its_session),
        ],
        "each visit keeps the session it names and the moment it was observed, ordered by that \
         moment rather than by insertion or by its session's start"
    );

    // Recording visits does not restate the sessions it names; they keep their own facts and ids.
    assert_eq!(
        sessions(store.reading_sessions_for_document(read).expect("read")),
        vec![
            (first_session, first),
            (second_session, second),
            (corrected_back_session, corrected_back),
        ]
    );
}

#[test]
fn an_invalid_page_visit_is_refused_without_touching_stored_facts() {
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let read = store
        .upsert_document(&document(
            "/library/read.pdf",
            FileId::Posix { dev: 62, ino: 1 },
        ))
        .expect("insert");
    let other = store
        .upsert_document(&document(
            "/library/other.pdf",
            FileId::Posix { dev: 62, ino: 2 },
        ))
        .expect("insert");

    let read_progress =
        StoredProgress::new(3, Some(80), ProgressSource::Manual, Confidence::Manual);
    let other_progress =
        StoredProgress::new(9, Some(20), ProgressSource::Manual, Confidence::Manual);
    store
        .record_progress(read, &read_progress)
        .expect("record a reading position");
    store
        .record_progress(other, &other_progress)
        .expect("record another reading position");
    let read_session_facts = ReadingSession {
        doc_id: read,
        started_at: 1_700_000_000,
        ended_at: 1_700_000_300,
        active_ms: 240_000,
    };
    let other_session_facts = ReadingSession {
        doc_id: other,
        started_at: 1_700_000_000,
        ended_at: 1_700_000_200,
        active_ms: 100_000,
    };
    let read_session = store
        .record_reading_session(&read_session_facts)
        .expect("record a session");
    let other_session = store
        .record_reading_session(&other_session_facts)
        .expect("record another document's session");

    let valid = PageVisit {
        doc_id: read,
        session_id: read_session,
        page: 7,
        observed_at: 1_700_000_500,
        visible_ms: 42_000,
    };
    let other_valid = PageVisit {
        doc_id: other,
        session_id: other_session,
        page: 1,
        observed_at: 1_700_000_400,
        visible_ms: 8_000,
    };
    let valid_id = store
        .record_page_visit(&valid)
        .expect("record a valid visit");
    let other_valid_id = store
        .record_page_visit(&other_valid)
        .expect("record another document's valid visit");

    let refused_cases = [
        (
            PageVisit {
                doc_id: 9_999,
                ..valid.clone()
            },
            "document",
        ),
        // A session of another document: refused, because a visit cannot be attributed across
        // documents. An id no session carries is refused the same way.
        (
            PageVisit {
                session_id: other_session,
                ..valid.clone()
            },
            "session_id",
        ),
        (
            PageVisit {
                session_id: 9_999,
                ..valid.clone()
            },
            "session_id",
        ),
        (
            PageVisit {
                observed_at: -1,
                ..valid.clone()
            },
            "observed_at",
        ),
        (
            PageVisit {
                visible_ms: -1,
                ..valid.clone()
            },
            "visible_ms",
        ),
    ];
    for (visit, field) in refused_cases {
        let refused = store.record_page_visit(&visit);
        assert!(
            matches!(&refused, Err(MirkError::InvalidInput { field: named, .. }) if *named == field),
            "`{visit:?}` must be refused as `{field}`, got {refused:?}"
        );
    }

    // Both documents are exactly as they were: a refusal wrote no visit and changed no session or
    // progress row, including the document the refused visit tried to borrow a session from.
    assert_eq!(row_count(&path, "page_visits"), 2);
    assert_eq!(row_count(&path, "reading_sessions"), 2);
    assert_eq!(row_count(&path, "progress"), 2);
    assert_eq!(
        visits(store.page_visits_for_document(read).expect("read visits")),
        vec![(valid_id, valid)]
    );
    assert_eq!(
        visits(store.page_visits_for_document(other).expect("read visits")),
        vec![(other_valid_id, other_valid)]
    );
    assert_eq!(
        store.progress_for_document(read).expect("read progress"),
        vec![read_progress]
    );
    assert_eq!(
        store.progress_for_document(other).expect("read progress"),
        vec![other_progress]
    );
    assert_eq!(
        sessions(store.reading_sessions_for_document(read).expect("read")),
        vec![(read_session, read_session_facts)],
        "the refused cross-document visit borrowed no session and changed none"
    );
    assert_eq!(
        sessions(store.reading_sessions_for_document(other).expect("read")),
        vec![(other_session, other_session_facts)]
    );
}

#[test]
fn recording_a_page_visit_leaves_progress_sessions_and_documents_unchanged() {
    // A visit is a fact of its own: it does not move the reading position, does not extend the
    // session it names, and never appears under another document.
    let (_dir, path) = temp_db();
    let store = Store::open(&path).expect("open");
    let read = store
        .upsert_document(&document(
            "/library/read.pdf",
            FileId::Posix { dev: 63, ino: 1 },
        ))
        .expect("insert");
    let other = store
        .upsert_document(&document(
            "/library/other.pdf",
            FileId::Posix { dev: 63, ino: 2 },
        ))
        .expect("insert");
    let session = ReadingSession {
        doc_id: read,
        started_at: 1_700_000_000,
        ended_at: 1_700_000_300,
        active_ms: 240_000,
    };

    let progress = StoredProgress::new(3, Some(80), ProgressSource::Manual, Confidence::Manual);
    store
        .record_progress(read, &progress)
        .expect("record a reading position");
    let session_id = store
        .record_reading_session(&session)
        .expect("record a session");
    store
        .record_page_visit(&PageVisit {
            doc_id: read,
            session_id,
            page: 7,
            observed_at: 1_700_000_500,
            visible_ms: 42_000,
        })
        .expect("record a visit");

    assert_eq!(
        store.progress_for_document(read).expect("read progress"),
        vec![progress],
        "a page on screen is not a reading position"
    );
    assert_eq!(
        sessions(store.reading_sessions_for_document(read).expect("read")),
        vec![(session_id, session)],
        "recording a visit does not restate or extend the session it names"
    );
    assert!(
        store
            .page_visits_for_document(other)
            .expect("read visits")
            .is_empty(),
        "a visit never appears under another document"
    );
    assert_eq!(store.document_count().expect("count"), 2);
    assert_eq!(store.library().expect("library").len(), 2);
}

// --- S8, again: an earlier, uncommitted schema 12 must not be adopted --------------------------

/// The two reading-table shapes an earlier, uncommitted build of schema 12 could have left behind in
/// a development database. Neither is reachable through this build's interface, so each is rebuilt
/// here from the statements that build wrote.
#[derive(Clone, Copy, Debug)]
enum EarlierV12 {
    /// The first shape: `page_visits` had neither the session it happened in nor the moment the page
    /// was observed.
    WithoutSessionAndObservationTime,
    /// The second shape: the columns exist, but the storage-class checks, the numeric bounds and the
    /// same-document key do not — and it still demands a monotonic wall-clock span, which this build
    /// deliberately does not: elapsed reading time is not `ended_at - started_at`.
    WithWeakerConstraints,
}

impl EarlierV12 {
    fn statements(self) -> &'static str {
        match self {
            EarlierV12::WithoutSessionAndObservationTime => {
                "CREATE TABLE reading_sessions (
                     id         INTEGER PRIMARY KEY,
                     doc_id     INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                     started_at INTEGER NOT NULL,
                     ended_at   INTEGER NOT NULL,
                     active_ms  INTEGER NOT NULL,
                     CHECK (ended_at >= started_at),
                     CHECK (active_ms >= 0)
                 );
                 CREATE INDEX idx_reading_sessions_doc ON reading_sessions (doc_id, started_at);
                 CREATE TABLE page_visits (
                     id         INTEGER PRIMARY KEY,
                     doc_id     INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                     page       INTEGER NOT NULL,
                     visible_ms INTEGER NOT NULL,
                     CHECK (page >= 0),
                     CHECK (visible_ms >= 0)
                 );
                 CREATE INDEX idx_page_visits_doc ON page_visits (doc_id, page);"
            }
            EarlierV12::WithWeakerConstraints => {
                "CREATE TABLE reading_sessions (
                     id         INTEGER PRIMARY KEY,
                     doc_id     INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                     started_at INTEGER NOT NULL,
                     ended_at   INTEGER NOT NULL,
                     active_ms  INTEGER NOT NULL,
                     CHECK (ended_at >= started_at),
                     CHECK (active_ms >= 0)
                 );
                 CREATE INDEX idx_reading_sessions_doc ON reading_sessions (doc_id, started_at);
                 CREATE TABLE page_visits (
                     id          INTEGER PRIMARY KEY,
                     doc_id      INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                     session_id  INTEGER NOT NULL REFERENCES reading_sessions(id) ON DELETE CASCADE,
                     page        INTEGER NOT NULL,
                     observed_at INTEGER NOT NULL,
                     visible_ms  INTEGER NOT NULL,
                     CHECK (page >= 0),
                     CHECK (visible_ms >= 0)
                 );
                 CREATE INDEX idx_page_visits_doc ON page_visits (doc_id, observed_at);
                 CREATE INDEX idx_page_visits_session ON page_visits (session_id, observed_at);"
            }
        }
    }

    fn rows(self) -> &'static str {
        match self {
            EarlierV12::WithoutSessionAndObservationTime => {
                "INSERT INTO reading_sessions (id, doc_id, started_at, ended_at, active_ms)
                 VALUES (1, 1, 1700000000, 1700000600, 480000);
                 INSERT INTO page_visits (id, doc_id, page, visible_ms)
                 VALUES (1, 1, 7, 42000);"
            }
            EarlierV12::WithWeakerConstraints => {
                "INSERT INTO reading_sessions (id, doc_id, started_at, ended_at, active_ms)
                 VALUES (1, 1, 1700000000, 1700000600, 480000);
                 INSERT INTO page_visits (id, doc_id, session_id, page, observed_at, visible_ms)
                 VALUES (1, 1, 1, 7, 1700000100, 42000);"
            }
        }
    }
}

/// Builds an isolated database carrying an earlier, uncommitted schema 12: the stamp says 12, the
/// reading tables have the older shape, and they hold representative session and page rows.
fn an_earlier_v12_database(shape: EarlierV12) -> (tempfile::TempDir, PathBuf) {
    let (dir, path) = temp_db();
    {
        let store = Store::open(&path).expect("build a current database");
        store
            .upsert_document(&document(
                "/library/read.pdf",
                FileId::Posix { dev: 24, ino: 1 },
            ))
            .expect("insert the document the old rows point at");
    }
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        drop_reading_tables(&raw);
        raw.execute_batch(shape.statements())
            .expect("rebuild the earlier shape");
        raw.execute_batch(shape.rows())
            .expect("record the rows the earlier build stored");
        raw.pragma_update(None, "user_version", 12i64)
            .expect("stamp the version the earlier build stamped");
    }
    (dir, path)
}

/// Everything a refused open must leave exactly as it was: the version stamp, the stored definition
/// of the reading tables and of the objects attached to them, and every row they hold.
///
/// Deliberately **logical** state, not the bytes of one file: committed rows can live in the
/// write-ahead log, and a connection closing can checkpoint them into the main file without changing
/// what the database contains. A byte comparison would either be flaky or claim something this crate
/// does not control.
#[derive(Debug, PartialEq, Eq)]
struct DatabaseSnapshot {
    version: i64,
    schema: Vec<(String, String, String)>,
    sessions: Vec<Vec<String>>,
    visits: Vec<Vec<String>>,
}

fn snapshot_database(path: &Path) -> DatabaseSnapshot {
    let connection = rusqlite::Connection::open(path).expect("open raw");
    snapshot_connection(&connection)
}

/// The same logical state, read through a connection the caller already holds.
///
/// A refused open must leave the committed state untouched while another connection still holds the
/// write-ahead log — that log belongs to the fixture connection, and only it can be asked what the
/// database contains before and after.
fn snapshot_connection(connection: &rusqlite::Connection) -> DatabaseSnapshot {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("read the version stamp");
    // Every object attached to the reading tables, so an unexpected index or trigger is part of what
    // a refused open must leave exactly as it was.
    let schema: Vec<(String, String, String)> = {
        let mut statement = connection
            .prepare(
                "SELECT type, name, COALESCE(sql, '') FROM sqlite_master \
                 WHERE name IN ('reading_sessions', 'page_visits', \
                                'idx_reading_sessions_doc', 'idx_page_visits_doc', \
                                'idx_page_visits_session') \
                    OR tbl_name IN ('reading_sessions', 'page_visits') \
                 ORDER BY type, name",
            )
            .expect("prepare");
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .expect("query")
            .map(|row| row.expect("row"))
            .collect()
    };
    DatabaseSnapshot {
        version,
        schema,
        sessions: dump_rows(connection, "SELECT * FROM reading_sessions ORDER BY id"),
        visits: dump_rows(connection, "SELECT * FROM page_visits ORDER BY id"),
    }
}

/// The path of one of a database's sidecar files, which belong to it and travel with it.
fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Every value of every row, with its SQLite storage class kept visible.
fn dump_rows(connection: &rusqlite::Connection, sql: &str) -> Vec<Vec<String>> {
    let mut statement = connection.prepare(sql).expect("prepare");
    let columns = statement.column_count();
    statement
        .query_map([], |row| {
            let mut values = Vec::with_capacity(columns);
            for index in 0..columns {
                let value: rusqlite::types::Value = row.get(index)?;
                values.push(format!("{value:?}"));
            }
            Ok(values)
        })
        .expect("query")
        .map(|row| row.expect("row"))
        .collect()
}

#[test]
fn an_earlier_v12_shape_without_the_session_and_time_facts_is_refused_and_left_untouched() {
    // The version stamp cannot tell this database apart from the corrected schema 12: it says 12
    // because an earlier pre-release build wrote it. Opening it must fail before any reading fact is
    // read or written, and must not touch a single byte — inventing the missing session and time
    // facts would be worse than refusing.
    let (_dir, path) = an_earlier_v12_database(EarlierV12::WithoutSessionAndObservationTime);
    let before = snapshot_database(&path);

    let error = Store::open(&path).expect_err("an earlier schema 12 must be refused");
    match &error {
        MirkError::Storage(message) => {
            assert!(
                message.contains("page_visits"),
                "the refusal must name the table it cannot use: {message}"
            );
            assert!(
                message.contains("session_id"),
                "the refusal must name the missing session fact: {message}"
            );
            assert!(
                message.contains("observed_at"),
                "the refusal must name the missing time fact: {message}"
            );
            assert!(
                message.contains("will not repair, rewrite, move or delete it"),
                "the refusal must say nothing was repaired: {message}"
            );
        }
        other => panic!("an incompatible schema must be a storage error, got {other:?}"),
    }

    assert_eq!(
        before,
        snapshot_database(&path),
        "a refused open must change nothing: not the version, not the tables, not a row"
    );
}

#[test]
fn an_earlier_v12_shape_with_weaker_constraints_is_refused_and_left_untouched() {
    // The same columns, but without the storage-class checks, the timestamp bounds, the `u32` page
    // bound or the same-document key. It reads and writes through this interface, so only the
    // definition itself can tell it apart from the corrected schema — and it must be refused, not
    // silently accepted as if the stronger contract were in force.
    let (_dir, path) = an_earlier_v12_database(EarlierV12::WithWeakerConstraints);
    let before = snapshot_database(&path);

    let error = Store::open(&path).expect_err("an earlier schema 12 must be refused");
    match &error {
        MirkError::Storage(message) => {
            assert!(
                message.contains("page_visits"),
                "the refusal must name the table it cannot use: {message}"
            );
            assert!(
                message.contains("the definition of `page_visits` differs"),
                "the refusal must say the definition differs, without diagnosing why: {message}"
            );
        }
        other => panic!("an incompatible schema must be a storage error, got {other:?}"),
    }

    assert_eq!(
        before,
        snapshot_database(&path),
        "a refused open must change nothing: not the version, not the tables, not a row"
    );
}

#[test]
fn an_earlier_v12_shape_in_wal_mode_is_refused_and_keeps_its_committed_state() {
    // Committed rows can live in the `-wal` sidecar instead of the main file, so what a refusal must
    // preserve is the logical committed state, not the bytes of one file. The fixture connection stays
    // open while the refused open runs, so the rows are still in the log when the refusal happens.
    let (_dir, path) = temp_db();
    {
        let store = Store::open(&path).expect("build a current database");
        store
            .upsert_document(&document(
                "/library/read.pdf",
                FileId::Posix { dev: 26, ino: 1 },
            ))
            .expect("insert");
    }

    let fixture = rusqlite::Connection::open(&path).expect("open raw");
    let mode: String = fixture
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .expect("switch the database to WAL");
    assert_eq!(mode, "wal");
    drop_reading_tables(&fixture);
    fixture
        .execute_batch(EarlierV12::WithoutSessionAndObservationTime.statements())
        .expect("rebuild the earlier shape");
    fixture
        .execute_batch(EarlierV12::WithoutSessionAndObservationTime.rows())
        .expect("record the rows the earlier build stored");
    fixture
        .pragma_update(None, "user_version", 12i64)
        .expect("stamp the version the earlier build stamped");

    let wal = sidecar(&path, "-wal");
    assert!(
        wal.exists(),
        "the fixture must leave its committed state in the write-ahead log"
    );
    let before = snapshot_connection(&fixture);

    let error = Store::open(&path).expect_err("an earlier schema 12 must be refused");
    match &error {
        MirkError::Storage(message) => {
            assert!(
                message.contains("committed contents"),
                "the refusal must speak about committed contents, not about physical files: {message}"
            );
            assert!(
                !message.contains("move the file"),
                "the refusal must not suggest moving only the main database file: {message}"
            );
        }
        other => panic!("an incompatible schema must be a storage error, got {other:?}"),
    }

    assert_eq!(
        before,
        snapshot_connection(&fixture),
        "the committed state must be unchanged while another connection still holds the log"
    );
    assert!(
        wal.exists(),
        "the refused open must not checkpoint away a log another connection still holds"
    );

    // Closing the fixture connection checkpoints the log; the committed state must survive it.
    drop(fixture);
    assert_eq!(
        before,
        snapshot_database(&path),
        "the committed state must survive the checkpoint"
    );
}

#[test]
fn an_unexpected_trigger_on_a_reading_table_is_refused_and_left_untouched() {
    // A trigger can delete or rewrite the facts as they are written, so it is not a harmless extra:
    // it is part of what the reading tables do. This build did not create it and cannot vouch for it,
    // so the database is refused rather than trusted.
    let (_dir, path) = temp_db();
    {
        let store = Store::open(&path).expect("build a current database");
        store
            .upsert_document(&document(
                "/library/read.pdf",
                FileId::Posix { dev: 27, ino: 1 },
            ))
            .expect("insert");
    }
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        raw.execute_batch(
            "CREATE TRIGGER unexpected_visit_cleanup AFTER INSERT ON page_visits \
             BEGIN DELETE FROM page_visits; END;",
        )
        .expect("attach an unexpected trigger");
    }
    let before = snapshot_database(&path);

    let error = Store::open(&path).expect_err("an unexpected trigger must be refused");
    match &error {
        MirkError::Storage(message) => assert!(
            message.contains("unexpected trigger") && message.contains("unexpected_visit_cleanup"),
            "the refusal must name the object it cannot vouch for: {message}"
        ),
        other => panic!("an unexpected trigger must be a storage error, got {other:?}"),
    }
    assert_eq!(before, snapshot_database(&path), "nothing may change");
}

#[test]
fn an_unexpected_unique_index_on_a_reading_table_is_refused_and_left_untouched() {
    // A unique index is a correctness constraint: it can reject a fact this build records, so it is
    // not a harmless addition to the shape this build writes.
    let (_dir, path) = temp_db();
    drop(Store::open(&path).expect("build a current database"));
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        raw.execute_batch(
            "CREATE UNIQUE INDEX idx_one_session_per_start \
             ON reading_sessions (doc_id, started_at);",
        )
        .expect("attach a unique index this build did not write");
    }
    let before = snapshot_database(&path);

    let error = Store::open(&path).expect_err("an unexpected index must be refused");
    match &error {
        MirkError::Storage(message) => assert!(
            message.contains("unexpected index") && message.contains("idx_one_session_per_start"),
            "the refusal must name the object it cannot vouch for: {message}"
        ),
        other => panic!("an unexpected index must be a storage error, got {other:?}"),
    }
    assert_eq!(before, snapshot_database(&path), "nothing may change");
}

#[test]
fn an_unexpected_index_named_like_an_autoindex_is_refused_and_left_untouched() {
    // SQLite's own autoindexes are exempt because they belong to the table definition that was
    // compared above. The exemption must match the reserved prefix exactly: `_` is a `LIKE`
    // wildcard, so a pattern would also exempt an object named `sqliteXautoindexY...`, which this
    // build did not create.
    let (_dir, path) = temp_db();
    drop(Store::open(&path).expect("build a current database"));
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        raw.execute_batch(
            "CREATE INDEX sqliteXautoindex_lookalike ON reading_sessions (started_at);",
        )
        .expect("attach an index whose name only looks reserved");
    }
    let before = snapshot_database(&path);

    let error = Store::open(&path).expect_err("a lookalike autoindex name must not be exempt");
    match &error {
        MirkError::Storage(message) => assert!(
            message.contains("unexpected index") && message.contains("sqliteXautoindex_lookalike"),
            "the refusal must name the object it cannot vouch for: {message}"
        ),
        other => panic!("an unexpected index must be a storage error, got {other:?}"),
    }
    assert_eq!(before, snapshot_database(&path), "nothing may change");
}

#[test]
fn a_current_database_on_a_read_only_file_opens_and_reads() {
    // The guard must not need the write lock: opening a current database is a read. A write-protected
    // file is the sharpest proof of that. A process that can still write after the permission change —
    // root, or a filesystem that ignores the mode — cannot prove this, and the test says so instead of
    // pretending.
    let (_dir, path) = temp_db();
    let document_id = {
        let store = Store::open(&path).expect("build a current database");
        store
            .upsert_document(&document(
                "/library/read.pdf",
                FileId::Posix { dev: 32, ino: 1 },
            ))
            .expect("insert")
    };

    let original = std::fs::metadata(&path).expect("metadata").permissions();
    let mut read_only = original.clone();
    read_only.set_readonly(true);
    std::fs::set_permissions(&path, read_only).expect("make the database read-only");

    let writable = std::fs::OpenOptions::new().write(true).open(&path).is_ok();
    if writable {
        std::fs::set_permissions(&path, original).expect("restore the mode");
        eprintln!("skipping: this process can still write a read-only file");
        return;
    }

    let store = Store::open(&path).expect("a read-only current database must open");
    assert_eq!(
        store.schema_version().expect("schema version"),
        Store::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(
        store
            .document_by_id(document_id)
            .expect("read")
            .expect("row")
            .document
            .path,
        PathBuf::from("/library/read.pdf")
    );
    drop(store);

    std::fs::set_permissions(&path, original).expect("restore the mode");
}

#[test]
fn a_v12_definition_that_differs_only_in_a_constraint_literal_is_refused() {
    // The comparison is byte-exact on purpose: `typeof(doc_id) = 'integer'` and
    // `typeof(doc_id) = 'INTEGER'` are different constraints — the second can never pass, so every
    // write would be rejected — and a comparison that folded case or tokenized the SQL could accept
    // a definition it cannot prove equivalent. This database differs from this build's shape in one
    // literal and nothing else.
    let (_dir, path) = temp_db();
    drop(Store::open(&path).expect("build a current database"));

    let raw = rusqlite::Connection::open(&path).expect("open raw");
    let table_sql: String = raw
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'page_visits'",
            [],
            |row| row.get(0),
        )
        .expect("read the table this build wrote");
    let index_sql: Vec<String> = {
        let mut statement = raw
            .prepare(
                "SELECT sql FROM sqlite_master WHERE type = 'index' \
                 AND tbl_name = 'page_visits' AND sql IS NOT NULL ORDER BY name",
            )
            .expect("prepare");
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query")
            .map(|row| row.expect("row"))
            .collect()
    };
    let weakened = table_sql.replace("typeof(doc_id) = 'integer'", "typeof(doc_id) = 'INTEGER'");
    assert_ne!(
        weakened, table_sql,
        "the fixture must change the constraint"
    );

    raw.execute_batch("DROP TABLE page_visits;")
        .expect("remove the corrected table");
    raw.execute_batch(&weakened)
        .expect("rebuild it with one literal changed");
    for statement in &index_sql {
        raw.execute_batch(statement).expect("recreate the index");
    }
    drop(raw);

    let before = snapshot_database(&path);
    let error = Store::open(&path).expect_err("a different constraint literal must be refused");
    match &error {
        MirkError::Storage(message) => assert!(
            message.contains("the definition of `page_visits` differs"),
            "the refusal must say the definition differs: {message}"
        ),
        other => panic!("a different definition must be a storage error, got {other:?}"),
    }
    assert_eq!(before, snapshot_database(&path), "nothing may change");
}

#[test]
fn a_v12_stamp_without_the_reading_objects_is_refused_and_left_untouched() {
    // A version stamp is a claim, not proof: a database can say 12 and not hold what 12 means. The
    // gate must report the missing objects by name rather than trusting the number, and it must not
    // create them — the stamp still says 12, so creating them silently would be a repair nobody
    // asked for and would leave the version claim indistinguishable from a real upgrade.
    let (_dir, path) = temp_db();
    drop(Store::open(&path).expect("build a current database"));
    {
        let raw = rusqlite::Connection::open(&path).expect("open raw");
        drop_reading_tables(&raw);
        raw.pragma_update(None, "user_version", 12i64)
            .expect("leave the current stamp behind the shape");
    }

    let error = Store::open(&path).expect_err("a stamp without its tables must be refused");
    match &error {
        MirkError::Storage(message) => {
            assert!(
                message.contains("the table `reading_sessions` is missing"),
                "the refusal must name the missing session table: {message}"
            );
            assert!(
                message.contains("the table `page_visits` is missing"),
                "the refusal must name the missing visit table: {message}"
            );
        }
        other => panic!("an incomplete schema must be a storage error, got {other:?}"),
    }
    assert_eq!(raw_version(&path), 12, "the stamp must not be rewritten");
    assert!(
        !raw_table_names(&path)
            .iter()
            .any(|name| name == "reading_sessions" || name == "page_visits"),
        "a refused open must not create the missing tables"
    );

    // The same for one missing index: the indexes are part of the shape the reads rely on.
    let (_index_dir, index_path) = temp_db();
    drop(Store::open(&index_path).expect("build a current database"));
    {
        let raw = rusqlite::Connection::open(&index_path).expect("open raw");
        raw.execute_batch("DROP INDEX idx_page_visits_session;")
            .expect("remove one index this build writes");
    }
    let error = Store::open(&index_path).expect_err("a missing index must be refused");
    match &error {
        MirkError::Storage(message) => assert!(
            message.contains("the index `idx_page_visits_session` is missing"),
            "the refusal must name the missing index: {message}"
        ),
        other => panic!("a missing index must be a storage error, got {other:?}"),
    }
    assert_eq!(raw_version(&index_path), i64::from(Store::CURRENT_SCHEMA_VERSION));
}
