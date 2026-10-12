//! T5 acceptance tests, exercised only through the public interface of `oct-store`.
//!
//! Every database these tests touch lives in a `tempfile` directory, never in the user's real
//! data directory or `$HOME`. Nothing here inspects `sqlite_master` directly: the schema is
//! observed through `Store`'s own API.

use std::path::{Path, PathBuf};

use oct_core::{Confidence, Document, FileId, OctError, Platform, ProgressSource, StoredProgress};
use oct_store::{database_path, user_data_dir_for, Store};

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

// --- S1: the six documented tables -----------------------------------------------------------

#[test]
fn schema_contains_exactly_the_six_documented_tables() {
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
    let expected: [(&str, &[&str]); 6] = [
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
            ],
        ),
        (
            "progress",
            &["id", "doc_id", "page", "total", "source", "confidence", "updated_at"],
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
