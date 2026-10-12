//! T6 acceptance tests, exercised only through the public interfaces of `oct-scan` and
//! `oct-store`.
//!
//! Every test works inside a `tempfile` directory. No test reads a real library and none writes
//! outside its temporary directory. The identity provider is a deterministic fake, so these tests
//! never depend on inode numbers or on `oct-platform`.
//!
//! **These tests need the native pdfium library**, because proving real page counting means parsing
//! real PDFs. Run `scripts/fetch-pdfium.sh` once per clone (or set `OCT_PDFIUM_LIB`) before
//! `cargo test`. Without it they fail with a message naming both remedies — deliberately not skipped,
//! because a test that silently checks nothing is worse than one that says it cannot run.

use std::path::{Path, PathBuf};

use oct_core::{FileId, FileIdentity, OctError};
use oct_scan::{resolve_pdfium_library, PdfiumLibrary, Scanner};
use oct_store::Store;

/// The R2 fixture: three page objects hidden inside a FlateDecode-compressed object stream.
const OBJECT_STREAM_FIXTURE: &[u8] = include_bytes!("fixtures/pages-in-object-stream.pdf");
/// A classic-xref PDF with four pages, `Title: A Known Title` and `Author: A Known Author`.
const INFO_FIXTURE: &[u8] = include_bytes!("fixtures/with-info-metadata.pdf");

/// A deterministic identity derived from the path bytes, never from inode numbers.
///
/// `oct-scan` cannot tell how identity is produced — that is the point of the injected
/// [`FileIdentity`] — so the tests drive it with this fake instead of `oct-platform`.
struct FakeIdentity;

impl FileIdentity for FakeIdentity {
    fn id(&self) -> &'static str {
        "fake-path-hash"
    }

    fn identity(&self, path: &Path) -> Result<FileId, OctError> {
        Ok(FileId::ContentHash(format!(
            "fnv1a64:{:016x}",
            fnv1a64(path.as_os_str().as_encoded_bytes())
        )))
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn temp_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path().join("library");
    std::fs::create_dir(&root).expect("create library dir");
    (dir, root)
}

fn temp_store(dir: &tempfile::TempDir) -> Store {
    Store::open(dir.path().join("oct.sqlite3")).expect("open store")
}

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directory");
    }
    std::fs::write(path, bytes).expect("write file");
}

/// A naive `/Type /Page` byte scan, written the way risk R2 says people write it.
///
/// It matches the token only at a boundary so `/Type /Pages` (the page-tree node) is not counted.
/// On the R2 fixture it returns `0`, which is exactly why a real parser is mandatory.
fn naive_page_object_count(bytes: &[u8]) -> usize {
    let needle = b"/Type /Page";
    let mut count = 0;
    for start in 0..bytes.len() {
        let Some(window) = bytes.get(start..start + needle.len()) else {
            break;
        };
        if window != needle {
            continue;
        }
        let boundary = !matches!(
            bytes.get(start + needle.len()),
            Some(byte) if byte.is_ascii_alphanumeric()
        );
        if boundary {
            count += 1;
        }
    }
    count
}

fn count_substring(bytes: &[u8], needle: &[u8]) -> usize {
    bytes
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

fn counted_pages(report: &oct_scan::ScanReport, path: &Path) -> Option<u32> {
    report
        .counted
        .iter()
        .find(|document| document.path == path)
        .map(|document| document.pages)
}

// --- S1: discovery ---------------------------------------------------------------------------

#[test]
fn discovers_nested_pdfs_case_insensitively_and_keeps_paths_byte_for_byte() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;

    let nested = root.join("sub dir").join("nested.pdf");
    let accented = root
        .join("Comparación de Métodos entre JavaScript y Python.pdf");
    let multibyte = root.join("café ☕").join("日本語.PDF");
    let ignored = root.join("notes.txt");

    write_file(&nested, OBJECT_STREAM_FIXTURE);
    write_file(&accented, OBJECT_STREAM_FIXTURE);
    write_file(&multibyte, OBJECT_STREAM_FIXTURE);
    write_file(&ignored, b"not a pdf, and not named like one either");

    let report = Scanner::new(&store, &identity)
        .scan(&root)
        .expect("scan completes");

    assert_eq!(report.discovered_count(), 3, "the non-PDF file is not discovered");
    assert_eq!(report.failure_count(), 0);
    for path in [&nested, &accented, &multibyte] {
        assert!(
            report.discovered.contains(path),
            "{} was not discovered",
            path.display()
        );
    }

    // The exact bytes that reached the store, not a normalised approximation (R7).
    for expected in [&nested, &accented, &multibyte] {
        let identity = identity
            .identity(expected)
            .expect("fake identity never fails");
        let stored = store
            .find_document_by_identity(&identity)
            .expect("lookup succeeds")
            .expect("the discovered PDF was stored");
        assert_eq!(
            stored.document.path.as_os_str(),
            expected.as_os_str(),
            "the stored path must survive byte-for-byte"
        );
    }

    // Guard the fixture itself: the multi-byte name is not silently transliterated.
    let stored_multibyte = report
        .discovered
        .iter()
        .find(|path| path.extension().is_some_and(|ext| ext == "PDF"))
        .expect("the .PDF file was discovered");
    assert_eq!(stored_multibyte, &multibyte);
}

// --- S2: real page counting (R2) -------------------------------------------------------------

#[test]
fn counts_object_stream_pages_and_proves_the_naive_scan_is_wrong_on_the_same_file() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    let path = root.join("pages-in-object-stream.pdf");
    write_file(&path, OBJECT_STREAM_FIXTURE);

    let report = Scanner::new(&store, &identity)
        .scan(&root)
        .expect("scan completes");

    // The real parser follows the page tree through the object stream.
    assert_eq!(counted_pages(&report, &path), Some(3));

    // The naive methods are wrong on this very file, so the fixture cannot become vacuous.
    assert_eq!(
        naive_page_object_count(OBJECT_STREAM_FIXTURE),
        0,
        "the naive byte scan must count 0 page objects here, or this fixture proves nothing"
    );
    assert_eq!(
        count_substring(OBJECT_STREAM_FIXTURE, b"/Type /Page"),
        1,
        "the only literal match is the /Type /Pages tree node"
    );
}

// --- S3: metadata ----------------------------------------------------------------------------

#[test]
fn reads_title_and_author_and_never_invents_absent_metadata() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    let with_info = root.join("with-info-metadata.pdf");
    let without_info = root.join("pages-in-object-stream.pdf");
    write_file(&with_info, INFO_FIXTURE);
    write_file(&without_info, OBJECT_STREAM_FIXTURE);

    let report = Scanner::new(&store, &identity)
        .scan(&root)
        .expect("scan completes");

    let known = report
        .counted
        .iter()
        .find(|document| document.path == with_info)
        .expect("the metadata fixture was counted");
    assert_eq!(known.title.as_deref(), Some("A Known Title"));
    assert_eq!(known.author.as_deref(), Some("A Known Author"));

    let unknown = report
        .counted
        .iter()
        .find(|document| document.path == without_info)
        .expect("the object-stream fixture was counted");
    assert_eq!(unknown.title, None, "an absent title is None, not an empty string");
    assert_eq!(unknown.author, None, "an absent author is None, not an empty string");
}

// --- S4: honest failures ---------------------------------------------------------------------

fn write_encrypted_fixture(source: &Path, destination: &Path) {
    // Encryption derives its key from the trailer `/ID`, which the fixtures do not carry; add one
    // so the encrypted copy can be produced at all.
    let file_id = lopdf::Object::Array(vec![
        lopdf::Object::String(
            b"0123456789abcdef".to_vec(),
            lopdf::StringFormat::Literal,
        ),
        lopdf::Object::String(
            b"0123456789abcdef".to_vec(),
            lopdf::StringFormat::Literal,
        ),
    ]);
    let mut borrowed = lopdf::Document::load(source).expect("load fixture for encryption");
    borrowed.trailer.set(b"ID", file_id.clone());
    let version = lopdf::EncryptionVersion::V2 {
        document: &borrowed,
        owner_password: "owner",
        user_password: "user",
        key_length: 128,
        permissions: lopdf::Permissions::all(),
    };
    let state = lopdf::EncryptionState::try_from(version).expect("build encryption state");
    let mut document = lopdf::Document::load(source).expect("load fixture again");
    document.trailer.set(b"ID", file_id);
    document.encrypt(&state).expect("encrypt");
    document.save(destination).expect("save encrypted fixture");
}

#[test]
fn reports_non_pdf_corrupt_and_encrypted_files_as_failures_with_none_pages() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;

    let not_a_pdf = root.join("not-a-pdf.pdf");
    let truncated = root.join("truncated.pdf");
    let encrypted = root.join("encrypted.pdf");
    write_file(&not_a_pdf, b"this file is named .pdf but is plain text\n");
    write_file(&truncated, &INFO_FIXTURE[..200]);
    // The source for the encrypted copy lives outside the watched root, so it is not scanned.
    let plain_source = dir.path().join("plain-source.pdf");
    write_file(&plain_source, INFO_FIXTURE);
    write_encrypted_fixture(&plain_source, &encrypted);

    let report = Scanner::new(&store, &identity)
        .scan(&root)
        .expect("the scan itself does not fail");

    assert_eq!(report.counted_count(), 0);
    for path in [&not_a_pdf, &truncated, &encrypted] {
        let failure = report
            .failures
            .iter()
            .find(|failure| &failure.path == path)
            .unwrap_or_else(|| panic!("{} is reported as a failure", path.display()));
        assert!(
            !failure.reason.is_empty(),
            "a failure must carry a reason, not just a path"
        );

        let identity = identity.identity(path).expect("fake identity never fails");
        let stored = store
            .find_document_by_identity(&identity)
            .expect("lookup succeeds")
            .expect("a discovered PDF is stored even when it cannot be read");
        assert_eq!(
            stored.document.pages, None,
            "unreadable pages stay None, never 0"
        );
        assert_ne!(stored.document.pages, Some(0));
    }
}

// --- S5: filesystem robustness ---------------------------------------------------------------

#[cfg(unix)]
#[test]
fn a_symlink_loop_does_not_hang_the_scan() {
    let (dir, root) = temp_root();
    write_file(&root.join("real.pdf"), OBJECT_STREAM_FIXTURE);
    let sub = root.join("sub");
    std::fs::create_dir(&sub).expect("create sub dir");
    // Two ways to loop forever if symlinks were followed: back to the root, and onto itself.
    std::os::unix::fs::symlink(&root, sub.join("back-to-root")).expect("symlink to root");
    std::os::unix::fs::symlink(&sub, sub.join("self")).expect("symlink to self");

    // Bound the test, so a regression fails the assertion instead of hanging the whole suite.
    let (sender, receiver) = std::sync::mpsc::channel();
    let thread_root = root.clone();
    let thread_dir = dir.path().to_path_buf();
    std::thread::spawn(move || {
        let store = Store::open(thread_dir.join("oct.sqlite3")).expect("open store");
        let identity = FakeIdentity;
        let discovered = Scanner::new(&store, &identity)
            .scan(&thread_root)
            .map(|report| report.discovered_count());
        let _ = sender.send(discovered);
    });

    let discovered = receiver
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("scan did not finish: a symlink loop was probably followed");
    assert_eq!(discovered.expect("scan completes"), 1);
}

#[cfg(unix)]
#[test]
fn a_permission_denied_subdirectory_is_reported_and_does_not_abort_the_scan() {
    use std::os::unix::fs::PermissionsExt;

    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    write_file(&root.join("readable.pdf"), OBJECT_STREAM_FIXTURE);
    let locked = root.join("locked");
    write_file(&locked.join("hidden.pdf"), OBJECT_STREAM_FIXTURE);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
        .expect("lock the directory");

    // Root (and some filesystems) ignore the mode bits, so the condition cannot be created there.
    let unreadable = std::fs::read_dir(&locked).is_err();

    let report = Scanner::new(&store, &identity).scan(&root);

    // Restore before asserting so the temporary directory can always be cleaned up.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755))
        .expect("restore the directory mode");

    let report = report.expect("the scan completes despite the unreadable directory");
    if unreadable {
        assert_eq!(report.counted_count(), 1);
        assert!(
            report.failures.iter().any(|failure| failure.path == locked),
            "the unreadable directory is reported"
        );
    } else {
        eprintln!(
            "skipping the permission assertion: this account can read a 0o000 directory \
             (probably running as root), so the condition cannot be created"
        );
    }
}

#[test]
fn a_file_that_disappears_between_discovery_and_parsing_is_reported_not_fatal() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    let keeps = root.join("keeps.pdf");
    let vanishes = root.join("vanishes.pdf");
    write_file(&keeps, OBJECT_STREAM_FIXTURE);
    write_file(&vanishes, INFO_FIXTURE);

    let scanner = Scanner::new(&store, &identity);
    let discovery = scanner.discover(&root).expect("discovery completes");
    assert_eq!(discovery.pdfs.len(), 2);

    // The file is in the discovered list; it is gone by the time it would be parsed.
    std::fs::remove_file(&vanishes).expect("remove the discovered file");

    let outcome = scanner
        .count_and_persist(&discovery.pdfs)
        .expect("pdfium loads and page counting runs");
    assert_eq!(outcome.counted.len(), 1);
    assert_eq!(outcome.counted[0].path, keeps);
    assert_eq!(outcome.failures.len(), 1);
    assert_eq!(outcome.failures[0].path, vanishes);
    assert!(!outcome.failures[0].reason.is_empty());

    let identity = identity.identity(&vanishes).expect("fake identity never fails");
    let stored = store
        .find_document_by_identity(&identity)
        .expect("lookup succeeds")
        .expect("the vanished file is still recorded");
    assert_eq!(stored.document.pages, None);
}

// --- S6: persistence and idempotence ---------------------------------------------------------

#[test]
fn scanning_twice_leaves_the_same_rows_and_records_the_folder_once() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    let nested = root.join("sub").join("nested.pdf");
    let flat = root.join("flat.pdf");
    write_file(&nested, INFO_FIXTURE);
    write_file(&flat, OBJECT_STREAM_FIXTURE);

    let scanner = Scanner::new(&store, &identity);
    let first = scanner.scan(&root).expect("first scan");
    assert_eq!(first.counted_count(), 2);
    let rows_after_first = store.document_count().expect("count documents");

    let second = scanner.scan(&root).expect("second scan");
    assert_eq!(second.counted_count(), 2);
    assert_eq!(
        store.document_count().expect("count documents"),
        rows_after_first,
        "a rescan must not duplicate documents"
    );

    let flat_identity = identity.identity(&flat).expect("fake identity never fails");
    let flat_record = store
        .find_document_by_identity(&flat_identity)
        .expect("lookup succeeds")
        .expect("flat.pdf is stored");
    assert_eq!(flat_record.document.pages, Some(3));
    let nested_identity = identity.identity(&nested).expect("fake identity never fails");
    let nested_record = store
        .find_document_by_identity(&nested_identity)
        .expect("lookup succeeds")
        .expect("nested.pdf is stored");
    assert_eq!(nested_record.document.pages, Some(4));

    let folders = store.folders().expect("list folders");
    assert_eq!(folders.len(), 1, "the watched folder is recorded exactly once");
    assert_eq!(
        folders[0].path,
        root.canonicalize().expect("canonical root")
    );
    assert!(folders[0].recursive);
}

// --- S4: a known count is preserved, and the failure says so ---------------------------------

#[test]
fn a_previously_counted_document_that_becomes_unreadable_keeps_its_count_and_is_reported() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    let path = root.join("counted.pdf");
    write_file(&path, OBJECT_STREAM_FIXTURE);

    let scanner = Scanner::new(&store, &identity);
    let first = scanner.scan(&root).expect("first scan");
    assert_eq!(counted_pages(&first, &path), Some(3));

    // The file is damaged after having been counted. Its count is the last known one, not a claim
    // about a fresh reading — the failure entry is what says the count was not re-verified.
    write_file(&path, b"this is no longer a PDF at all");

    let second = scanner.scan(&root).expect("second scan");
    assert_eq!(second.counted_count(), 0);
    assert!(
        second.failures.iter().any(|failure| failure.path == path),
        "the damaged file is reported as a failure"
    );

    let stored = store
        .find_document_by_identity(&identity.identity(&path).expect("fake identity never fails"))
        .expect("lookup succeeds")
        .expect("the document is still stored");
    assert_eq!(
        stored.document.pages,
        Some(3),
        "an unknown page count must not erase a known one, and must never become 0"
    );
}

// --- S5: a deliberate skip is reported, not invisible -----------------------------------------

#[cfg(unix)]
#[test]
fn a_symlinked_folder_is_reported_as_skipped_not_silently_ignored() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    write_file(&root.join("real.pdf"), OBJECT_STREAM_FIXTURE);

    // A folder of PDFs linked into the library from outside it.
    let elsewhere = dir.path().join("elsewhere");
    write_file(&elsewhere.join("linked.pdf"), INFO_FIXTURE);
    let link = root.join("linked-folder");
    std::os::unix::fs::symlink(&elsewhere, &link).expect("create the symlink");

    let report = Scanner::new(&store, &identity)
        .scan(&root)
        .expect("scan completes");

    assert_eq!(report.counted_count(), 1, "only the real file is counted");
    assert_eq!(
        report.skipped,
        vec![root
            .canonicalize()
            .expect("canonical root")
            .join("linked-folder")],
        "the symlink is reported as skipped"
    );
    assert_eq!(report.skipped_count(), 1);
    assert!(
        report.failures.is_empty(),
        "a deliberate skip is not a failure: nothing went wrong"
    );
    assert_eq!(store.document_count().expect("count documents"), 1);
}

// --- S1/S4: an unrepresentable name is reported, never silently dropped -----------------------

#[cfg(unix)]
#[test]
fn a_pdf_whose_extension_is_not_valid_utf8_is_reported_not_silently_skipped() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    write_file(&root.join("fine.pdf"), OBJECT_STREAM_FIXTURE);

    // The store cannot represent this path at all, so the honest outcome is a reported failure.
    // Skipping it would make an invisible file indistinguishable from a file that is not there.
    let undecodable = root.join(OsStr::from_bytes(b"broken.\xff\xfe"));
    write_file(&undecodable, OBJECT_STREAM_FIXTURE);

    let report = Scanner::new(&store, &identity)
        .scan(&root)
        .expect("the scan completes");

    assert_eq!(
        report.counted_count(),
        1,
        "only the representable PDF is counted"
    );
    let failure = report
        .failures
        .iter()
        .find(|failure| failure.path == undecodable)
        .expect("the unrepresentable name is reported as a failure");
    assert!(
        !failure.reason.is_empty(),
        "a failure must carry a reason, not just a path"
    );
    assert_eq!(store.document_count().expect("count documents"), 1);
}

// --- S7: layering is asserted by the manifest and by this fake identity driving every test ----

// --- pdfium library resolution (the native library is loaded at runtime) -----------------------

#[test]
fn resolves_an_explicit_library_file_first() {
    let (dir, _root) = temp_root();
    let library = dir.path().join("libpdfium.so");
    write_file(&library, b"a stand-in; resolution never loads it");
    let vendor = dir.path().join("vendor/lib");

    let resolved = resolve_pdfium_library(Some(&library), &vendor)
        .expect("an existing explicit file resolves");
    assert_eq!(resolved, PdfiumLibrary::File(library));
}

#[test]
fn resolves_an_explicit_library_directory_first() {
    let (dir, _root) = temp_root();
    let library = dir.path().join("pdfium-lib");
    std::fs::create_dir_all(&library).expect("create the library directory");
    let vendor = dir.path().join("vendor/lib");

    let resolved = resolve_pdfium_library(Some(&library), &vendor)
        .expect("an existing explicit directory resolves");
    assert_eq!(resolved, PdfiumLibrary::Directory(library));
}

#[test]
fn falls_back_to_the_vendor_directory() {
    let (dir, _root) = temp_root();
    let vendor = dir.path().join("vendor/lib");
    std::fs::create_dir_all(&vendor).expect("create the vendor directory");

    let resolved = resolve_pdfium_library(None, &vendor).expect("the vendor directory resolves");
    assert_eq!(resolved, PdfiumLibrary::Directory(vendor));
}

#[test]
fn falls_back_to_the_system_library_when_nothing_local_exists() {
    let (dir, _root) = temp_root();
    let vendor = dir.path().join("vendor/absent");

    let resolved = resolve_pdfium_library(None, &vendor)
        .expect("the system library is always the last resort");
    assert_eq!(resolved, PdfiumLibrary::System);
}

#[test]
fn an_explicit_path_that_does_not_exist_is_an_honest_error() {
    let (dir, _root) = temp_root();
    let vendor = dir.path().join("vendor/lib");
    std::fs::create_dir_all(&vendor).expect("create the vendor directory");
    let stale = dir.path().join("stale/libpdfium.so");

    let error = resolve_pdfium_library(Some(&stale), &vendor)
        .expect_err("a stale explicit path must not silently fall back to the vendor library");
    let OctError::ComponentUnavailable { component, message } = error else {
        panic!("expected OctError::ComponentUnavailable, got {error:?}");
    };
    assert_eq!(component, "pdfium");
    assert!(
        message.contains("does not exist"),
        "the error names the cause: {message}"
    );
    assert!(
        message.contains("fetch-pdfium.sh"),
        "names the fetch script: {message}"
    );
    assert!(
        message.contains("OCT_PDFIUM_LIB"),
        "names the environment variable: {message}"
    );
}

// --- a missing native library fails the scan, not the documents -------------------------------

#[test]
fn a_missing_pdfium_library_fails_the_scan_once_instead_of_every_document() {
    let (dir, root) = temp_root();
    let store = temp_store(&dir);
    let identity = FakeIdentity;
    write_file(&root.join("first.pdf"), OBJECT_STREAM_FIXTURE);
    write_file(&root.join("second.pdf"), INFO_FIXTURE);

    // Point the scanner at a library that does not exist. `with_pdfium_library` takes the explicit
    // value directly, so the test mutates no process state — the review flagged in-process
    // environment mutation as a race.
    let stale = dir.path().join("stale/libpdfium.so");
    let scanner = Scanner::with_pdfium_library(&store, &identity, Some(stale));

    // Every entry point that parses surfaces the same single, actionable error.
    let outcomes = [
        scanner
            .count_and_persist(&[root.join("first.pdf")])
            .map(|_| ()),
        scanner.scan(&root).map(|_| ()),
    ];
    for outcome in outcomes {
        let error =
            outcome.expect_err("a missing pdfium library must fail the call, not the files");
        let OctError::ComponentUnavailable { component, message } = error else {
            panic!("expected OctError::ComponentUnavailable, got {error:?}");
        };
        assert_eq!(component, "pdfium");
        assert!(message.contains("fetch-pdfium.sh"), "{message}");
        assert!(message.contains("OCT_PDFIUM_LIB"), "{message}");
    }

    // Discovery is a filesystem walk and must not depend on a native parser: listing a folder has to
    // keep working when page counting cannot run at all.
    let discovery = scanner
        .discover(&root)
        .expect("discovery must not need the pdfium library");
    assert_eq!(discovery.pdfs.len(), 2, "both PDFs are still discovered");

    // One error, not one per document: nothing is counted, stored or recorded, and a missing
    // library never turns into a page count of 0.
    assert_eq!(
        store.document_count().expect("count documents"),
        0,
        "a missing library must not store documents with a page count of 0"
    );
    assert!(
        store.folders().expect("list folders").is_empty(),
        "the scan failed before recording the folder"
    );
}
