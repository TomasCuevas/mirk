//! The commands the screens call, with their logic in plain functions.
//!
//! The Tauri wrappers in `main.rs` are deliberately thin: they open the production database, call
//! one of the functions here, and render an [`oct_core::OctError`] into a string an IPC caller can
//! read. Everything that decides *what* the screen shows lives in this module, takes its
//! collaborators as arguments, and is therefore testable without starting a Tauri application —
//! the same injection pattern the scanner and the provider adapters already use.
//!
//! Each command exists because a pixel of a screen needs it: the library, a folder scan, the
//! progress-provider registry, the watched folders Configuración lists and now removes (R20, R27),
//! the card action that opens a document's containing folder, and the watched-folder action that
//! opens the folder itself (R24). Anything else is a later task.

use std::fs;
use std::path::{Path, PathBuf};

use oct_core::domain::CoverChoice;
use oct_core::{
    Document, FileIdentity, LaunchOutcome, Launcher, OctError, OpenDocumentDetector, StoredProgress,
};
use oct_scan::{cover_for, CoverOutcome, ScanReport, Scanner};
use oct_store::{
    covering_folder, AuthorEdit, FolderRemoval, LibraryEntry, Store, Suggestion,
    SuggestionDecision, WatchedFolder,
};
use serde::Serialize;

use crate::shell;

/// A reading position serialised for the interface, provenance included.
///
/// `source` and `confidence` stay two fields, mirroring the two store columns: the screen renders
/// them separately and must never be handed a single flattened value. `recorded_at` is when the app
/// wrote the row; `read_at` is when a source observed the reading, and serialises as `readAt` to
/// match the interface's own spelling. They are deliberately two fields, because a row the app
/// wrote is not evidence of a reading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressView {
    pub page: u32,
    pub total: Option<u32>,
    pub source: String,
    pub confidence: String,
    pub recorded_at: Option<i64>,
    pub read_at: Option<i64>,
}

impl ProgressView {
    fn new(progress: &StoredProgress, recorded_at: Option<i64>) -> Self {
        Self {
            page: progress.page,
            total: progress.total,
            source: progress.source.id().to_string(),
            confidence: confidence_id(progress.confidence),
            recorded_at,
            read_at: progress.read_at,
        }
    }
}

/// One library row as the list library command returns it.
///
/// `title` is already the **displayed** title (R31): the core resolves the reader's title over the
/// document's own, so no screen has to decide whose title it is rendering. `user_title` carries the
/// reader's title on its own — `null` when they have not set one — so the interface can say "this
/// one is yours" without guessing. It is deliberately not a `title_source` column: with the two facts
/// kept separate, no source is needed to tell them apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntryView {
    pub id: i64,
    pub path: String,
    /// The title to show, resolved by the core: the reader's when set, otherwise the document's own.
    pub title: String,
    /// The reader's own title, or `null` when they have not set one (`userTitle` over IPC).
    pub user_title: Option<String>,
    /// The author to show, resolved by the core: the reader's when set, otherwise the document's own
    /// (`author` over IPC). `null` when neither the document nor the reader has an author.
    pub author: Option<String>,
    /// The reader's own author, or `null` when they have not set one (`userAuthor` over IPC).
    pub user_author: Option<String>,
    pub pages: Option<u32>,
    pub progress: Option<ProgressView>,
    /// The page the reader chose for the cover, zero-based, or `null` for the first page (R32).
    pub cover_page: Option<u32>,
    /// The file name of an image the reader supplied, inside the covers directory, or `null` (R32).
    pub cover_image: Option<String>,
}

impl From<LibraryEntry> for LibraryEntryView {
    fn from(entry: LibraryEntry) -> Self {
        let LibraryEntry {
            document,
            progress,
            progress_recorded_at,
        } = entry;

        Self {
            id: document.id,
            path: display_path(&document.document.path),
            title: document.document.display_title(),
            user_title: document.document.user_title.clone(),
            author: document.document.display_author(),
            user_author: document.document.user_author.clone(),
            pages: document.document.pages,
            cover_page: document.document.cover_page,
            cover_image: document.document.cover_image.clone(),
            progress: progress
                .as_ref()
                .map(|progress| ProgressView::new(progress, progress_recorded_at)),
        }
    }
}

/// A document the scan counted and persisted, as the scan command returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CountedView {
    pub path: String,
    pub pages: u32,
    pub title: Option<String>,
    pub author: Option<String>,
}

/// A path the scan could not turn into a counted document, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FailureView {
    pub path: String,
    pub reason: String,
}

/// What the scan command returns. Mirrors [`oct_scan::ScanReport`] with paths as strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReportView {
    pub root: String,
    pub discovered: Vec<String>,
    pub counted: Vec<CountedView>,
    pub skipped: Vec<String>,
    pub failures: Vec<FailureView>,
}

impl From<ScanReport> for ScanReportView {
    fn from(report: ScanReport) -> Self {
        let ScanReport {
            root,
            discovered,
            counted,
            skipped,
            failures,
        } = report;

        Self {
            root: display_path(&root),
            discovered: discovered.iter().map(|path| display_path(path)).collect(),
            counted: counted
                .into_iter()
                .map(|document| CountedView {
                    path: display_path(&document.path),
                    pages: document.pages,
                    title: document.title,
                    author: document.author,
                })
                .collect(),
            skipped: skipped.iter().map(|path| display_path(path)).collect(),
            failures: failures
                .into_iter()
                .map(|failure| FailureView {
                    path: display_path(&failure.path),
                    reason: failure.reason,
                })
                .collect(),
        }
    }
}

/// A progress provider's identity, capability and availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderView {
    pub id: String,
    /// `"live"`, `"close-time"` or `"none"` — what the viewer can report and the UI can promise.
    pub capability: String,
    /// `"installed"`, `"missing"` or `"unsupported"`.
    pub availability: String,
}

/// A watched folder as Configuración lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolderView {
    pub id: i64,
    pub path: String,
    pub recursive: bool,
    pub enabled: bool,
}

impl From<WatchedFolder> for WatchedFolderView {
    fn from(folder: WatchedFolder) -> Self {
        Self {
            id: folder.id,
            path: display_path(&folder.path),
            recursive: folder.recursive,
            enabled: folder.enabled,
        }
    }
}

/// What removing one watch would take with it, as Configuración's confirmation reads it (R27).
///
/// The store type never crosses this boundary, exactly like every other view here: the screen sees
/// paths as strings and never a [`Store`] type, and `folder` is a [`WatchedFolderView`] built by
/// the same `display_path` the folder list uses, so the two cannot spell a path differently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderRemovalView {
    pub folder: WatchedFolderView,
    /// Paths of the documents that leave the library.
    pub documents: Vec<String>,
    /// Paths of the documents that stay because another remaining watch still covers them.
    pub kept: Vec<String>,
}

impl From<FolderRemoval> for FolderRemovalView {
    fn from(removal: FolderRemoval) -> Self {
        Self {
            folder: WatchedFolderView::from(removal.folder),
            documents: removal.documents,
            kept: removal.kept,
        }
    }
}

/// The folder a card action opened, and the program that opened it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenedFolderView {
    pub folder: String,
    pub viewer: String,
}

/// One document awaiting the reader's accept-or-discard decision (R4).
///
/// Only the **undecided** queue travels here: a decision is history, read back by id when it needs
/// to be stated. The path is shown as a path rather than a title, and that is correct here — a
/// suggestion is not a library document, so it has no title to show and inventing one from the file
/// name would be the screen guessing at data the application never read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionView {
    pub id: i64,
    pub path: String,
    /// How many times the application has observed it open since the reader last acted.
    pub seen_count: u32,
    /// Unix seconds when it was first observed, which is what orders the queue.
    pub first_seen_at: i64,
}

impl From<Suggestion> for SuggestionView {
    fn from(suggestion: Suggestion) -> Self {
        Self {
            id: suggestion.id,
            path: display_path(&suggestion.path),
            seen_count: suggestion.seen_count,
            first_seen_at: suggestion.first_seen_at,
        }
    }
}

/// The library listing: every document with its latest progress and provenance.
pub fn library_entries(store: &Store) -> Result<Vec<LibraryEntryView>, OctError> {
    Ok(store
        .library()?
        .into_iter()
        .map(LibraryEntryView::from)
        .collect())
}

/// Saves the reader's naming of one document in ONE write (R31, R37).
///
/// This is the command the editor's Save and its Restore control both call, and it is one command for
/// one reason: Save decides the reader's title *and* the reader's author, and an action that decides
/// two facts must apply both or neither. Two commands, each opening the store and each writing one
/// fact, is how a failed author write used to leave the title saved while the screen said nothing was.
///
/// `title` is a `String` on the IPC boundary, so the interface sends either the reader's text or
/// nothing at all: an absent value means "restore the document's own title" and is a legitimate
/// action, while a blank value is refused as the invalid argument it is. The refusal — the variant,
/// the field and the words — is decided in [`Store::set_user_naming`], so this command and any future
/// caller cannot disagree about what a blank title means.
///
/// `author` is an [`AuthorEdit`], not a plain value, because the editor's two controls decide different
/// things. Save decides the author — a value, or `Decided(None)` for *no author of my own*, which falls
/// back to the document's own author and is a legitimate action rather than an error. Restore decides
/// the title and nothing else, and says so with `Untouched`; that is how "leave the reader's author
/// exactly as it was" survives into the one write instead of being a missing second call.
///
/// Nothing here touches the document's own title or author: both reader's facts are separate, and a
/// document can always be restored to what the file itself carries.
pub fn set_document_naming(
    store: &Store,
    id: i64,
    title: Option<&str>,
    author: AuthorEdit<'_>,
) -> Result<(), OctError> {
    store.set_user_naming(id, title, author)
}

/// Sets or clears the page the reader chose for one document's cover (R32).
///
/// `page` is zero-based, exactly as pages are stored everywhere in this workspace; the interface
/// converts from the one-based number it shows. `None` restores the default, the first page, and is a
/// legitimate action rather than an error. The decision itself — a `NULL` page meaning the first —
/// lives in [`Document::cover_choice`], so this command only carries the reader's answer there.
pub fn set_cover_page(store: &Store, id: i64, page: Option<u32>) -> Result<(), OctError> {
    store.set_cover_page(id, page)
}

/// Copies the reader's chosen image into the covers directory and records its name (R32).
///
/// `source` is a path the native dialog returned; `None` removes the image the reader had set. The
/// file is **copied**, never referenced: the image is the reader's data, and it must survive a rescan,
/// the original being moved, or the original being deleted. The copy's name is derived from the
/// document's row id and the image's own format, so it cannot collide with another document's cover,
/// and the value recorded is that name alone — never a path outside the covers directory.
///
/// The image is judged by its first bytes before anything is written. A file that is not an image is
/// refused as [`OctError::InvalidInput`] — the variant this project added for exactly this kind of lie
/// (R35) — and never as a storage failure: the database was never involved, and reporting it as one
/// would send whoever reads the log to look at the disk. Checking signatures needs no dependency, and
/// the browser's `<img>` remains the thing that actually decodes the image.
pub fn set_cover_image(
    store: &Store,
    id: i64,
    data_directory: &Path,
    source: Option<&Path>,
) -> Result<(), OctError> {
    let Some(source) = source else {
        // Removing the image leaves the file on disk until another image replaces it. That is
        // deliberate: the file is inert once nothing references it, and deleting user data on an
        // action shaped like "undo" is how a reader loses an image they meant to keep.
        return store.set_cover_image(id, None);
    };

    let bytes = fs::read(source)?;
    let Some(mime) = image_mime(&bytes) else {
        return Err(OctError::InvalidInput {
            field: "cover image",
            value: display_path(source),
            expected: "a PNG, JPEG, GIF, WebP or BMP image".to_string(),
        });
    };

    let covers = covers_directory(data_directory);
    fs::create_dir_all(&covers)?;
    let file_name = format!("{id}-cover.{}", image_extension(mime));
    write_atomically(&covers.join(&file_name), &bytes)?;
    store.set_cover_image(id, Some(&file_name))
}

/// The MIME type of an image, judged by its first bytes.
///
/// A dependency-free signature check: enough to refuse a file that is not an image at all, and to
/// label the data URL for the formats people actually have. It is deliberately not a decoder — the
/// application copies the reader's bytes and the browser's `<img>` decides whether they render, so
/// nothing here needs to understand the image.
fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    const PNG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.starts_with(PNG) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    None
}

/// The file extension a copy of an image with this MIME type is stored under.
fn image_extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        _ => "img",
    }
}

/// Writes `bytes` to `path` through a sibling temporary file and a rename.
///
/// The same shape the cover cache uses: a half-written image must never be served, and the rename is
/// atomic on one filesystem, which the sibling guarantees.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), OctError> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

// --- R32: a document's cover, rendered on demand and cached in the application's data directory ---

/// The directory covers live in: `<data directory>/covers` (L20).
///
/// The data directory is passed in rather than read here, exactly like every other collaborator in
/// this module: `main.rs` derives it from `oct_store::database_path`, so the covers and the database
/// live in one place by construction instead of by two functions agreeing.
pub fn covers_directory(data_directory: &Path) -> PathBuf {
    data_directory.join("covers")
}

/// The cached cover file for one page of the document with `id`.
///
/// Named by the document's row id AND the page, and that second half is the point (R32). A cover is
/// cached on disk and served from that file forever, so a name without the page would keep serving
/// the old page after the reader chose another one — a stale cover presented as the current one.
/// Carrying the page means a changed choice simply asks for a different file: the cache is never
/// invalidated, it is outgrown, and the old file stops being reachable rather than becoming wrong.
/// The row id still anchors the cover to a document (a document that moves keeps its row).
fn cover_file(covers_directory: &Path, id: i64, page: u32) -> PathBuf {
    covers_directory.join(format!("{id}-p{page}.png"))
}

/// A document's cover as a data URL, or `None` when it cannot be shown (R32).
///
/// The choice is [`Document::cover_choice`]'s: the reader's own image when there is one, otherwise the
/// page they chose, otherwise the first page. This function only ACTS on that answer; it does not
/// decide it, so the card and any future screen cannot disagree about which cover a document has.
///
/// `None` is reserved for facts about the **document**: it has no readable PDF (encrypted, deleted,
/// unreadable), the reader's image file is missing or is not an image, or the id names no row. Those
/// keep the card's placeholder and the reason goes to stderr, where this application reports
/// everything it cannot do. A failure of the application's own storage — the covers directory, the
/// write, a missing pdfium — is returned as an error instead, because reporting "this document has no
/// cover" when the disk refused the write would be a lie about the document. Either way nothing
/// reaches a screen as an error: the card turns a rejected request into its placeholder too.
pub fn document_cover(
    store: &Store,
    id: i64,
    data_directory: &Path,
) -> Result<Option<String>, OctError> {
    let Some(document) = document_by_id(store, id)? else {
        // An id that names nothing is "no cover", the same answer as a document with no readable
        // PDF. The interface only ever asks about documents it is showing, so this is a race at
        // worst, not a caller error worth a message on screen.
        return Ok(None);
    };

    let covers = covers_directory(data_directory);
    match document.cover_choice() {
        CoverChoice::Image(name) => cover_from_image(&covers, &name, &document.path),
        CoverChoice::Page(page) => cover_from_page(&covers, id, page, &document.path),
    }
}

/// The reader's own image, read from the covers directory as a data URL.
///
/// Only the file's own name is ever joined: a stored value carrying a directory part is not a name,
/// and this is the second line of defence against a hand-edited database pointing outside the covers
/// directory. A file that is gone, or whose first bytes are not an image, is a fact about that file —
/// no cover and a reason in the log, never a broken image on screen.
fn cover_from_image(
    covers: &Path,
    name: &str,
    document: &Path,
) -> Result<Option<String>, OctError> {
    let Some(file_name) = Path::new(name).file_name() else {
        return Ok(None);
    };
    let path = covers.join(file_name);

    match fs::read(&path) {
        Ok(bytes) => match image_mime(&bytes) {
            Some(mime) => Ok(Some(data_url(mime, &bytes))),
            None => {
                eprintln!(
                    "no cover for {}: the supplied image is not an image: {}",
                    display_path(document),
                    path.display()
                );
                Ok(None)
            }
        },
        Err(error) => {
            eprintln!(
                "no cover for {}: the supplied image could not be read: {error}",
                display_path(document)
            );
            Ok(None)
        }
    }
}

/// The chosen page of the document, rendered (or served cached) as a PNG data URL.
fn cover_from_page(
    covers: &Path,
    id: i64,
    page: u32,
    document: &Path,
) -> Result<Option<String>, OctError> {
    let cover_path = cover_file(covers, id, page);

    let pdfium = oct_scan::bind_pdfium_library().map_err(|error| {
        eprintln!(
            "the cover of {} could not be produced: {error}",
            display_path(document)
        );
        error
    })?;

    let cached = match cover_for(&pdfium, &cover_path, document, page) {
        Ok(CoverOutcome::Ready { path }) => path,
        Ok(CoverOutcome::Unavailable { reason }) => {
            // A fact about the document: no cover, no error, and the reason is written down.
            eprintln!("no cover for {}: {reason}", display_path(document));
            return Ok(None);
        }
        Err(error) => {
            eprintln!(
                "the cover of {} could not be produced: {error}",
                display_path(document)
            );
            return Err(error);
        }
    };

    match fs::read(&cached) {
        Ok(bytes) => Ok(Some(png_data_url(&bytes))),
        Err(error) => {
            eprintln!(
                "the cover of {} could not be read back: {error}",
                display_path(document)
            );
            Err(error.into())
        }
    }
}

/// The document with `id`, cover choice included, or `None` when the id names nothing.
///
/// The cover belongs to the document row, not to a path the interface hands back, so the row is
/// resolved from the store. The listing is the only lookup `oct-store` exposes today; it is ordered
/// by path, which is irrelevant to a `find` by id.
fn document_by_id(store: &Store, id: i64) -> Result<Option<Document>, OctError> {
    Ok(store
        .library()?
        .into_iter()
        .find(|entry| entry.document.id == id)
        .map(|entry| entry.document.document))
}

/// `bytes` as a PNG `data:` URL for an `<img src>`.
///
/// A data URL is deliberate: Tauri's asset protocol would need a new configuration entry plus a
/// capability grant, and a 400 px cover is small, so the extra protocol surface buys nothing. The
/// base64 is written here rather than pulled in as a dependency — the standard alphabet is twenty
/// lines, and this workspace adds a crate only when it earns its place.
fn png_data_url(bytes: &[u8]) -> String {
    data_url("image/png", bytes)
}

/// `bytes` as a `data:` URL of `mime`, for an `<img src>`.
fn data_url(mime: &str, bytes: &[u8]) -> String {
    let prefix = format!("data:{mime};base64,");
    let mut url = String::with_capacity(prefix.len() + bytes.len().div_ceil(3) * 4);
    url.push_str(&prefix);
    base64_encode(bytes, &mut url);
    url
}

/// The RFC 4648 standard alphabet.
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Appends the Base64 encoding of `bytes` to `out`.
fn base64_encode(bytes: &[u8], out: &mut String) {
    for chunk in bytes.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = u32::from(chunk.get(1).copied().unwrap_or(0));
        let third = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (first << 16) | (second << 8) | third;

        out.push(BASE64_ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(BASE64_ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            BASE64_ALPHABET[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_ALPHABET[triple as usize & 0x3f] as char
        } else {
            '='
        });
    }
}

/// Scans `folder` into `store` and returns what happened.
///
/// `identity` is injected so the scan can be driven with a real or fake provider, exactly as
/// [`Scanner`] documents.
///
/// A folder that a watched folder already covers is refused here, before anything is walked or
/// recorded: the same tree under two watches discovers every document in it twice, and the subfolder
/// adds nothing the watch does not already reach. The refusal carries the covering folder's path,
/// because "it was blocked" with no "by which one" leaves the reader with nothing to do about it.
///
/// The candidate is resolved with the same `canonicalize` the scanner uses on the root it records,
/// so a folder the user selected through a symlinked ancestor is still recognised as one the store
/// has already recorded. Canonicalising first also means a folder that does not exist fails here
/// rather than after a pdfium bind, which is the same honest `Io` error arriving one step earlier.
pub fn scan_path(
    store: &Store,
    identity: &dyn FileIdentity,
    folder: &Path,
) -> Result<ScanReportView, OctError> {
    let root = folder.canonicalize()?;

    if let Some(covering) = covering_folder(&store.folders()?, &root) {
        return Err(OctError::PathCoveredByWatchedFolder {
            path: display_path(&root),
            covering: display_path(&covering.path),
        });
    }

    let scanner = Scanner::new(store, identity);
    Ok(ScanReportView::from(scanner.scan(&root)?))
}

/// The progress providers this build knows, with their declared capability and live availability.
pub fn provider_registry() -> Vec<ProviderView> {
    oct_platform::progress_providers()
        .iter()
        .map(|provider| ProviderView {
            id: provider.id().to_string(),
            capability: provider.capability().id().to_string(),
            availability: provider.detect().id().to_string(),
        })
        .collect()
}

// --- The suggestion queue (R4) ---------------------------------------------------------------
//
// The queue is the one place the application observes documents the reader did not nominate. It is
// deliberately inert: `observe_open_documents` records what the detector saw, `list_suggestions`
// reads the undecided rows, and only the reader's own `accept_suggestion` / `discard_suggestion`
// turn a suggestion into library data or dismiss it. Nothing here may decide on the reader's behalf.

/// Records a suggestion for every PDF the detector reports open that is neither in the library nor
/// inside a watched folder (R4).
///
/// This is the producer half of the queue, and it decides nothing: it only records what the
/// application observed. Accepting or dismissing is the reader's ([`accept_suggestion`],
/// [`discard_suggestion`]). A watched folder is the source of truth, so a document under one is
/// skipped: if it is not in the library, the scan failed to count it, which is a scan failure to
/// report and never a suggestion to review. A document already in the library is likewise nothing to
/// suggest.
///
/// [`OctError::NotSupported`] from the detector is propagated rather than swallowed: on a platform
/// with no detector the caller must be able to say the capability is missing instead of showing an
/// empty queue that merely looks like good news.
///
/// Returns how many distinct paths were recorded. The detector may report one path under more than
/// one process, and this counts a document, not its readers.
pub fn observe_open_documents(
    store: &Store,
    detector: &dyn OpenDocumentDetector,
) -> Result<usize, OctError> {
    let open = detector.open_documents()?;
    let entries = store.library()?;
    let folders = store.folders()?;

    let mut recorded = std::collections::HashSet::new();
    for document in &open {
        if recorded.contains(&document.path) {
            continue;
        }
        if entries
            .iter()
            .any(|entry| entry.document.document.path == document.path)
        {
            continue;
        }
        if covering_folder(&folders, &document.path).is_some() {
            continue;
        }
        store.record_suggestion(&document.path)?;
        recorded.insert(document.path.clone());
    }
    Ok(recorded.len())
}

/// The suggestions still awaiting the reader's decision, oldest first.
pub fn list_suggestions(store: &Store) -> Result<Vec<SuggestionView>, OctError> {
    Ok(store
        .suggestions()?
        .into_iter()
        .map(SuggestionView::from)
        .collect())
}

/// Accepts the suggestion with `id`: the document joins the library, and the decision is recorded.
///
/// The order is deliberate. The file is counted and persisted **first**; only then is the suggestion
/// marked accepted. A file that has since disappeared, or a pdfium that cannot be loaded, therefore
/// leaves the suggestion exactly where it was — still awaiting a decision — instead of marking it
/// accepted while adding nothing. Once the document is in the library the producer can no longer
/// offer it, because its path is known; the decision write is the record that closes the queue item.
///
/// A suggestion whose file is gone is refused before anything is written. The scanner deliberately
/// persists a path it could not read so a watched folder keeps knowing about a broken file, and that
/// is right for a folder the reader nominated; it is wrong for a one-off suggestion, whose whole
/// point is that the reader chose a real document.
pub fn accept_suggestion(
    store: &Store,
    identity: &dyn FileIdentity,
    id: i64,
) -> Result<(), OctError> {
    let suggestion = store
        .suggestion(id)?
        .ok_or_else(|| OctError::Storage(format!("no suggestion with id {id} to accept")))?;
    if let Some(decision) = suggestion.decision {
        return Err(OctError::SuggestionAlreadyDecided {
            path: display_path(&suggestion.path),
            decision: decision.id(),
        });
    }

    if !suggestion.path.is_file() {
        return Err(OctError::InvalidPath(format!(
            "the suggested document no longer exists: {}",
            display_path(&suggestion.path)
        )));
    }

    let scanner = Scanner::new(store, identity);
    scanner.count_and_persist(std::slice::from_ref(&suggestion.path))?;

    let added = store
        .library()?
        .iter()
        .any(|entry| entry.document.document.path == suggestion.path);
    if !added {
        return Err(OctError::Storage(format!(
            "the suggested document could not be added: {}",
            display_path(&suggestion.path)
        )));
    }

    store.decide_suggestion(id, SuggestionDecision::Accepted)?;
    Ok(())
}

/// Dismisses the suggestion with `id`, recording that the reader declined it.
///
/// The row stays, with its decision and its time: a suggestion the reader dismissed is a fact about
/// their library, and removing the row would let the next detection pass offer it again. The
/// producer stops at a decided row, so this is the one write that closes the item for good.
pub fn discard_suggestion(store: &Store, id: i64) -> Result<(), OctError> {
    store.decide_suggestion(id, SuggestionDecision::Discarded)?;
    Ok(())
}

/// Every folder the store has recorded, oldest first.
pub fn watched_folders(store: &Store) -> Result<Vec<WatchedFolderView>, OctError> {
    Ok(store
        .folders()?
        .into_iter()
        .map(WatchedFolderView::from)
        .collect())
}

/// Computes what removing the watch with `id` would take with it, changing nothing (R27).
///
/// The confirmation reads this first, so the count it states is the store's own answer rather
/// than a guess the screen makes. `None` means the id names no watch.
pub fn folder_removal(store: &Store, id: i64) -> Result<Option<FolderRemovalView>, OctError> {
    Ok(store.folder_removal(id)?.map(FolderRemovalView::from))
}

/// Stops watching the folder with `id`, taking the documents it covers and their positions with it.
///
/// The decision is [`Store::forget_folder`]'s, including the guard that keeps a document another
/// remaining watch still covers. The view it returns is the same plan [`folder_removal`] shows, so
/// what the confirmation said and what actually left cannot disagree. All this adds is the store
/// the production command opens.
pub fn remove_folder(store: &Store, id: i64) -> Result<Option<FolderRemovalView>, OctError> {
    Ok(store.forget_folder(id)?.map(FolderRemovalView::from))
}

/// The `settings` key the interface language is stored under. The interface writes it and the
/// desktop process reads it to word the tray (R1), so both halves use this one name.
const LANGUAGE_SETTING: &str = "language";

/// The stored interface language, but only when it is a locale this build knows.
///
/// The database accepts any string under `settings`; a value this build does not recognise is not a
/// language and must not reach a caller as one. It is reported as absent — the same answer as "never
/// chosen" — so the interface keeps its default instead of rendering keys with no words behind them.
pub fn get_language(store: &Store) -> Result<Option<String>, OctError> {
    Ok(store
        .get_setting(LANGUAGE_SETTING)?
        .filter(|language| shell::is_known_language(language)))
}

/// Stores the interface language, refusing a locale this build does not know.
///
/// Refusing rather than storing keeps the single source honest: a typo written once would otherwise
/// be read back as "no language stored" forever, and a newer build's locale would be silently
/// downgraded by an older one. The tray is rebuilt by the command wrapper in `main.rs`, not here, so
/// this decision stays testable without a display or a Tauri application.
pub fn set_language(store: &Store, locale: &str) -> Result<(), OctError> {
    if !shell::is_known_language(locale) {
        // `R35`: a bad argument answers as a bad argument. This used to be `Storage`, which was a small
        // lie in a log — the database was never involved, and anyone reading a failure later would look
        // in the wrong place. Not reachable from the interface today: the selector offers what
        // `shell::LANGUAGES` holds.
        return Err(OctError::InvalidInput {
            field: "locale",
            value: locale.to_string(),
            expected: shell::LANGUAGES.join(" or "),
        });
    }
    store.set_setting(LANGUAGE_SETTING, locale)
}

/// Opens the folder that contains `path` with the platform launcher.
///
/// The document's own path is deliberately not opened: the reader asked for its folder. The launcher
/// is injected so this can be tested without starting a file manager, exactly as the scanner takes
/// its identity and the adapters take their spawner.
pub fn open_containing_folder(
    launcher: &dyn Launcher,
    path: &Path,
) -> Result<OpenedFolderView, OctError> {
    let folder = containing_folder(path)?;
    open_directory(launcher, &folder)
}

/// Opens `path`, which must itself be the folder the caller wants on screen (R24).
///
/// This is the watched-folder action, and it opens the folder **itself** rather than its parent:
/// [`open_containing_folder`] answers "where does this document live", and this answers "show me
/// this folder". Both go through [`open_directory`], so there is one place that decides what an
/// openable folder is and one place that reports a failure to open it.
pub fn open_folder(launcher: &dyn Launcher, path: &Path) -> Result<OpenedFolderView, OctError> {
    open_directory(launcher, path)
}

/// Opens `folder` with `launcher`, refusing anything that is not a folder on disk right now.
///
/// A folder that is gone is an honest error rather than a silent no-op: the store can hold a
/// document — or a watch — whose directory was deleted or moved since it was last seen, and
/// "nothing happened" would be the least useful report of that. The launcher's own failure (no
/// `xdg-open`, say) travels back unchanged.
fn open_directory(launcher: &dyn Launcher, folder: &Path) -> Result<OpenedFolderView, OctError> {
    match fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(OctError::InvalidPath(format!(
                "the path is not a folder: {}",
                display_path(folder)
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(OctError::InvalidPath(format!(
                "the folder no longer exists: {}",
                display_path(folder)
            )))
        }
        Err(error) => return Err(OctError::Io(error)),
    }

    let viewer = match launcher.launch(folder, None)? {
        LaunchOutcome::Delegated { viewer } => viewer,
        // `oct`'s own reader is a legitimate answer rather than a failure, once it exists.
        LaunchOutcome::OpenedInternally => "oct".to_string(),
    };

    Ok(OpenedFolderView {
        folder: display_path(folder),
        viewer,
    })
}

/// The directory a document lives in.
///
/// `Path::parent` returns `None` only for a path with no parent (a bare `/`), and an empty parent
/// for a bare file name; neither is a path a stored document can meaningfully have.
fn containing_folder(path: &Path) -> Result<PathBuf, OctError> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Ok(parent.to_path_buf()),
        _ => Err(OctError::InvalidPath(format!(
            "the document path has no folder: {}",
            display_path(path)
        ))),
    }
}

/// The stable string a [`oct_core::Confidence`] is persisted under.
fn confidence_id(confidence: oct_core::Confidence) -> String {
    match confidence {
        oct_core::Confidence::Exact => "exact",
        oct_core::Confidence::CloseTime => "close-time",
        oct_core::Confidence::Manual => "manual",
        oct_core::Confidence::Unknown => "unknown",
    }
    .to_string()
}

/// A path rendered for display. The store only ever holds valid UTF-8 paths, and a scan reports a
/// genuinely undecodable name as a failure rather than a document, so this is lossless in practice.
fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use oct_core::{
        Confidence, Document, FileId, LaunchOutcome, Launcher, OpenDocument, Platform,
        ProgressSource, StoredProgress,
    };
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn in_memory_store() -> Store {
        Store::open_in_memory().expect("open an in-memory store")
    }

    fn document(path: &str, dev: u64, ino: u64) -> Document {
        let mut doc = Document::new(path);
        doc.identity = Some(FileId::Posix { dev, ino });
        doc
    }

    /// A detector that reports exactly the paths a test hands it, so the producer's own rules can
    /// be exercised without walking the real `/proc` or depending on what this machine has open.
    struct FixedDetector {
        documents: Vec<OpenDocument>,
    }

    impl FixedDetector {
        fn seeing(paths: &[&str]) -> Self {
            Self {
                documents: paths
                    .iter()
                    .map(|path| OpenDocument {
                        path: PathBuf::from(*path),
                        pid: Some(4242),
                        viewer: Some("evince".to_string()),
                    })
                    .collect(),
            }
        }
    }

    impl OpenDocumentDetector for FixedDetector {
        fn id(&self) -> &'static str {
            "test-detector"
        }

        fn platform(&self) -> Platform {
            Platform::current()
        }

        fn open_documents(&self) -> Result<Vec<OpenDocument>, OctError> {
            Ok(self.documents.clone())
        }
    }

    #[test]
    fn the_library_view_carries_the_latest_progress_and_its_provenance() {
        let store = in_memory_store();
        let read_id = store
            .upsert_document(&document("/library/read.pdf", 1, 1))
            .expect("insert");
        let unread_id = store
            .upsert_document(&document("/library/unread.pdf", 1, 2))
            .expect("insert");

        // Two rows; the later one must win and the earlier one must not leak into the view.
        store
            .record_progress(
                read_id,
                &StoredProgress::new(
                    3,
                    Some(80),
                    ProgressSource::External("evince-gvfs"),
                    Confidence::Exact,
                ),
            )
            .expect("record");
        store
            .record_progress(
                read_id,
                &StoredProgress::new(
                    11,
                    Some(80),
                    ProgressSource::External("evince-gvfs"),
                    Confidence::Exact,
                ),
            )
            .expect("record");

        let entries = library_entries(&store).expect("list the library");
        assert_eq!(entries.len(), 2);

        let read = entries
            .iter()
            .find(|entry| entry.id == read_id)
            .expect("the read document");
        let progress = read.progress.as_ref().expect("progress present");
        assert_eq!(progress.page, 11);
        assert_eq!(progress.total, Some(80));
        assert_eq!(progress.source, "evince-gvfs");
        assert_eq!(progress.confidence, "exact");
        assert!(progress.recorded_at.is_some());

        let unread = entries
            .iter()
            .find(|entry| entry.id == unread_id)
            .expect("the unread document");
        assert_eq!(unread.progress, None, "no progress must stay absent, not become zero");
    }

    #[test]
    fn the_library_view_carries_the_reading_time_apart_from_the_write_time() {
        let store = in_memory_store();
        let doc_id = store
            .upsert_document(&document("/library/read.pdf", 1, 3))
            .expect("insert");

        // A reading time a year old, written now. Both facts must reach the view, and the write
        // time must not be allowed to stand in for the reading time.
        let old = 1_600_000_000;
        store
            .record_progress(
                doc_id,
                &StoredProgress::new(
                    4,
                    Some(80),
                    ProgressSource::External("evince-gvfs"),
                    Confidence::Exact,
                )
                .with_read_at(Some(old)),
            )
            .expect("record");

        let entries = library_entries(&store).expect("list the library");
        let progress = entries[0].progress.as_ref().expect("progress present");
        // `ProgressView` derives `Serialize` with `rename_all = "camelCase"`, so `read_at` reaches
        // the interface as `readAt`, exactly as `recorded_at` reaches it as `recordedAt`.
        assert_eq!(progress.read_at, Some(old), "the reading time must reach the view");
        assert!(
            progress.recorded_at.is_some(),
            "the row still records when the app wrote it"
        );
        assert_ne!(
            progress.recorded_at,
            Some(old),
            "the write time must not be mistaken for the reading time"
        );
    }

    // --- R31: the reader's title travels beside the document's own -------------------------------

    #[test]
    fn the_library_view_displays_the_readers_title_and_marks_it_as_theirs() {
        let store = in_memory_store();
        let mut doc = document("/library/name.pdf", 5, 5);
        doc.title = Some("The document's own title".to_string());
        let id = store.upsert_document(&doc).expect("insert");

        // Before anyone names it, the view shows the document's own title and no reader's title.
        let entries = library_entries(&store).expect("list");
        assert_eq!(entries[0].title, "The document's own title");
        assert_eq!(entries[0].user_title, None);

        set_document_naming(
            &store,
            id,
            Some("What the reader called it"),
            AuthorEdit::Untouched,
        )
        .expect("name it");

        let entries = library_entries(&store).expect("list again");
        // `title` is the displayed value, and `userTitle` marks it as the reader's.
        assert_eq!(entries[0].title, "What the reader called it");
        assert_eq!(
            entries[0].user_title.as_deref(),
            Some("What the reader called it")
        );
    }

    // --- the reader's author travels beside the document's own -----------------------------------

    #[test]
    fn the_library_view_displays_the_readers_author_and_marks_it_as_theirs() {
        let store = in_memory_store();
        let mut doc = document("/library/authored.pdf", 5, 9);
        doc.author = Some("The document's own author".to_string());
        let id = store.upsert_document(&doc).expect("insert");

        // Before anyone names it, the view shows the document's own author and no reader's author.
        let entries = library_entries(&store).expect("list");
        assert_eq!(entries[0].author.as_deref(), Some("The document's own author"));
        assert_eq!(entries[0].user_author, None);

        set_document_naming(
            &store,
            id,
            None,
            AuthorEdit::Decided(Some("What the reader called them")),
        )
        .expect("name it");

        let entries = library_entries(&store).expect("list again");
        // `author` is the displayed value, and `userAuthor` marks it as the reader's.
        assert_eq!(entries[0].author.as_deref(), Some("What the reader called them"));
        assert_eq!(entries[0].user_author.as_deref(), Some("What the reader called them"));
    }

    #[test]
    fn clearing_the_readers_author_restores_the_documents_own() {
        let store = in_memory_store();
        let mut doc = document("/library/authored.pdf", 5, 10);
        doc.author = Some("The document's own author".to_string());
        let id = store.upsert_document(&doc).expect("insert");

        set_document_naming(
            &store,
            id,
            None,
            AuthorEdit::Decided(Some("The reader's author")),
        )
        .expect("name it");
        // An empty author — `None` here, a blank field in the interface — is the legitimate "no author
        // of my own" action, not an error, and restores the document's own author.
        set_document_naming(&store, id, None, AuthorEdit::Decided(None))
            .expect("clear the reader's author");
        let entries = library_entries(&store).expect("list again");
        assert_eq!(entries[0].author.as_deref(), Some("The document's own author"));
        assert_eq!(entries[0].user_author, None);
    }

    #[test]
    fn a_document_with_no_title_at_all_reaches_the_view_as_the_files_own_name() {
        // The rule L18 shipped, now resolved in the core instead of the interface: a card shows the
        // file's name rather than its path, and the interface never has to decide that.
        let store = in_memory_store();
        store
            .upsert_document(&document("/library/Deep Work.pdf", 5, 6))
            .expect("insert");

        let entries = library_entries(&store).expect("list");
        assert_eq!(entries[0].title, "Deep Work");
        assert!(!entries[0].title.contains('/'), "a path is not a title");
        assert_eq!(entries[0].user_title, None);
    }

    #[test]
    fn restoring_the_documents_own_title_clears_the_readers() {
        let store = in_memory_store();
        let mut doc = document("/library/name.pdf", 5, 7);
        doc.title = Some("The document's own title".to_string());
        let id = store.upsert_document(&doc).expect("insert");

        set_document_naming(
            &store,
            id,
            Some("The reader's title"),
            AuthorEdit::Untouched,
        )
        .expect("name it");
        assert_eq!(
            library_entries(&store).expect("list")[0].title,
            "The reader's title"
        );

        // `None` is the legitimate "restore" action, not an error, and it says nothing about the
        // reader's author.
        set_document_naming(&store, id, None, AuthorEdit::Untouched)
            .expect("restore the document's own title");
        let entries = library_entries(&store).expect("list again");
        assert_eq!(entries[0].title, "The document's own title");
        assert_eq!(entries[0].user_title, None);
    }

    #[test]
    fn one_save_carries_both_facts_and_one_restore_touches_only_the_title() {
        // The shape of the fix at the command layer: Save is ONE call that decides the title and the
        // author, and Restore is one call that decides the title and says nothing about the author. The
        // store's own test pins that the two facts apply together or not at all; this pins what each
        // control asks for.
        let store = in_memory_store();
        let mut doc = document("/library/named.pdf", 5, 11);
        doc.title = Some("The document's own title".to_string());
        doc.author = Some("The document's own author".to_string());
        let id = store.upsert_document(&doc).expect("insert");

        set_document_naming(
            &store,
            id,
            Some("What the reader called it"),
            AuthorEdit::Decided(Some("What the reader called them")),
        )
        .expect("save the title and the author in one command");

        let saved = library_entries(&store).expect("list");
        assert_eq!(saved[0].title, "What the reader called it");
        assert_eq!(saved[0].author.as_deref(), Some("What the reader called them"));

        set_document_naming(&store, id, None, AuthorEdit::Untouched)
            .expect("restore the document's own title");

        let restored = library_entries(&store).expect("list again");
        assert_eq!(restored[0].title, "The document's own title");
        assert_eq!(restored[0].user_title, None);
        assert_eq!(
            restored[0].user_author.as_deref(),
            Some("What the reader called them"),
            "restoring the title must leave the reader's author exactly as it was"
        );
    }

    #[test]
    fn a_blank_title_is_refused_by_the_command_as_an_invalid_argument() {
        let store = in_memory_store();
        let mut doc = document("/library/name.pdf", 5, 8);
        doc.title = Some("The document's own title".to_string());
        let id = store.upsert_document(&doc).expect("insert");

        match set_document_naming(
            &store,
            id,
            Some("   "),
            // The author is decided by the same call, and must not land while the title is refused:
            // the refusal happens before the write opens, so there is nothing to roll back.
            AuthorEdit::Decided(Some("An author that must not be written")),
        ) {
            Err(OctError::InvalidInput {
                field,
                value,
                expected,
            }) => {
                assert_eq!(field, "title");
                assert_eq!(value, "   ");
                assert!(expected.contains("non-empty"), "expected: {expected}");
            }
            other => panic!("a blank title must be an invalid argument, got {other:?}"),
        }

        // Nothing was written, so the document still displays its own title and its own author.
        let entries = library_entries(&store).expect("list");
        assert_eq!(entries[0].title, "The document's own title");
        assert_eq!(entries[0].user_title, None);
        assert_eq!(entries[0].user_author, None);
    }

    #[test]
    fn scanning_a_real_folder_populates_the_library_view() {
        let store = in_memory_store();
        let identity = oct_platform::backends().file_identity;
        // The scanner's own committed fixtures: two real PDFs, read-only, never the maintainer's
        // library. Using them here exercises the same code path the command wraps.
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/oct-scan/tests/fixtures");

        let report = scan_path(&store, identity.as_ref(), &fixtures).expect("scan the fixtures");
        assert_eq!(report.discovered.len(), 2, "two fixtures: {report:?}");
        assert_eq!(
            report.counted.len(),
            report.discovered.len(),
            "both fixtures are countable: {report:?}"
        );
        assert!(report.failures.is_empty(), "no fixture fails: {report:?}");

        let entries = library_entries(&store).expect("list the library");
        assert_eq!(entries.len(), report.counted.len());
        assert!(
            entries.iter().all(|entry| entry.pages.is_some()),
            "a counted document must reach the library with its page count: {entries:?}"
        );
    }

    #[test]
    fn scanning_a_missing_folder_is_an_error_not_an_empty_success() {
        let store = in_memory_store();
        let identity = oct_platform::backends().file_identity;
        let missing = std::env::temp_dir().join("oct-desktop-folder-that-is-not-there-9f3c1a");

        let result = scan_path(&store, identity.as_ref(), &missing);
        assert!(
            result.is_err(),
            "a folder that does not exist must surface as an error, got {result:?}"
        );
    }

    #[test]
    fn the_provider_registry_reports_capability_and_availability() {
        let providers = provider_registry();
        for provider in &providers {
            assert!(!provider.id.is_empty(), "every provider has an id");
            assert!(
                ["live", "close-time", "none"].contains(&provider.capability.as_str()),
                "unexpected capability `{}`",
                provider.capability
            );
            assert!(
                ["installed", "missing", "unsupported"].contains(&provider.availability.as_str()),
                "unexpected availability `{}`",
                provider.availability
            );
        }
        #[cfg(target_os = "linux")]
        assert_eq!(
            providers.len(),
            1,
            "the Linux build registers exactly the Evince adapter"
        );
    }

    // --- The UI shell's two new commands -----------------------------------------------------

    /// A launcher that records what it was asked to open instead of opening anything.
    struct RecordingLauncher {
        calls: Mutex<Vec<(PathBuf, Option<u32>)>>,
    }

    impl RecordingLauncher {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl Launcher for RecordingLauncher {
        fn id(&self) -> &'static str {
            "test-recorder"
        }

        fn platform(&self) -> Platform {
            Platform::current()
        }

        fn launch(&self, path: &Path, page: Option<u32>) -> Result<LaunchOutcome, OctError> {
            self.calls
                .lock()
                .expect("the recording lock")
                .push((path.to_path_buf(), page));
            Ok(LaunchOutcome::Delegated {
                viewer: "test-file-manager".to_string(),
            })
        }
    }

    /// A unique temporary directory that removes itself on drop, including on panic.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "oct-desktop-{label}-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create the temporary directory");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn watched_folders_lists_what_the_store_recorded() {
        let store = in_memory_store();
        assert!(
            watched_folders(&store).expect("list folders").is_empty(),
            "a fresh store has recorded nothing"
        );

        store.record_folder("/library", true).expect("record");
        store.record_folder("/library/short", false).expect("record");

        let folders = watched_folders(&store).expect("list folders");
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].path, "/library");
        assert!(folders[0].recursive);
        assert!(folders[0].enabled);
        assert_eq!(folders[1].path, "/library/short");
        assert!(!folders[1].recursive);
    }

    // --- R27: removal computes its plan first, then carries it out ------------------------------

    #[test]
    fn folder_removal_plans_before_removing_and_names_the_documents() {
        let store = in_memory_store();
        let doomed = store.record_folder("/lib/doomed", true).expect("record");
        let keep = store.record_folder("/lib/keep", true).expect("record");
        store
            .upsert_document(&document("/lib/doomed/gone.pdf", 8, 1))
            .expect("insert");
        store
            .upsert_document(&document("/lib/keep/stays.pdf", 8, 2))
            .expect("insert");

        let planned = folder_removal(&store, doomed)
            .expect("plan")
            .expect("the watch is known");
        assert_eq!(planned.folder.id, doomed);
        assert_eq!(planned.documents, vec!["/lib/doomed/gone.pdf".to_string()]);
        assert!(planned.kept.is_empty());
        // The plan changed nothing, so the document is still there to be removed.
        assert_eq!(store.document_count().expect("count"), 2);

        let removed = remove_folder(&store, doomed)
            .expect("remove")
            .expect("the watch was known");
        assert_eq!(removed, planned, "the removal carries out the plan it showed");
        assert_eq!(store.document_count().expect("count"), 1);
        let folders = watched_folders(&store).expect("list folders");
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].id, keep);
    }

    #[test]
    fn removing_an_unknown_folder_is_none_and_deletes_nothing() {
        let store = in_memory_store();
        let watched = store.record_folder("/lib/doomed", true).expect("record");
        store
            .upsert_document(&document("/lib/doomed/gone.pdf", 9, 1))
            .expect("insert");

        assert!(
            folder_removal(&store, 404).expect("plan").is_none(),
            "an unknown id plans nothing"
        );
        assert!(
            remove_folder(&store, 404).expect("remove").is_none(),
            "an unknown id removes nothing"
        );
        assert_eq!(store.folders().expect("list folders").len(), 1);
        assert_eq!(store.document_count().expect("count"), 1);
        // The watch that does exist is untouched and still removable.
        assert!(folder_removal(&store, watched).expect("plan").is_some());
    }

    #[test]
    fn opening_the_containing_folder_asks_the_launcher_for_the_folder() {
        let dir = TempDir::new("open-folder");
        let folder = dir.path.join("libros");
        let document = folder.join("Cosmos.pdf");
        std::fs::create_dir_all(&folder).expect("create the document's folder");
        std::fs::write(&document, b"not really a pdf").expect("write the file");

        let launcher = RecordingLauncher::new();
        let opened = open_containing_folder(&launcher, &document).expect("open the folder");

        assert_eq!(opened.folder, display_path(&folder));
        assert_eq!(opened.viewer, "test-file-manager");

        let calls = launcher.calls.lock().expect("the recording lock");
        assert_eq!(calls.len(), 1, "exactly one launch");
        assert_eq!(calls[0].0, folder, "the folder, not the document");
        assert_eq!(calls[0].1, None, "opening a folder is not a page request");
    }

    #[test]
    fn a_folder_that_no_longer_exists_is_an_honest_error() {
        let dir = TempDir::new("gone-folder");
        let folder = dir.path.join("libros");
        let document = folder.join("Cosmos.pdf");
        std::fs::create_dir_all(&folder).expect("create the document's folder");
        std::fs::write(&document, b"not really a pdf").expect("write the file");
        // The library keeps a document whose folder was deleted after the last scan.
        std::fs::remove_dir_all(&folder).expect("remove the folder");

        let launcher = RecordingLauncher::new();
        let result = open_containing_folder(&launcher, &document);

        match result {
            Err(OctError::InvalidPath(message)) => {
                assert!(message.contains("libros"), "names the folder: {message}");
            }
            other => panic!("expected an InvalidPath error, got {other:?}"),
        }
        assert!(
            launcher.calls.lock().expect("the recording lock").is_empty(),
            "a folder that is gone must not be launched"
        );
    }

    #[test]
    fn a_path_with_no_folder_is_an_honest_error() {
        let launcher = RecordingLauncher::new();
        let result = open_containing_folder(&launcher, Path::new("/"));

        assert!(
            matches!(result, Err(OctError::InvalidPath(_))),
            "got {result:?}"
        );
        assert!(launcher.calls.lock().expect("the recording lock").is_empty());
    }

    #[test]
    fn a_launcher_failure_travels_back_unchanged() {
        struct FailingLauncher;

        impl Launcher for FailingLauncher {
            fn id(&self) -> &'static str {
                "test-failing"
            }

            fn platform(&self) -> Platform {
                Platform::current()
            }

            fn launch(&self, _path: &Path, _page: Option<u32>) -> Result<LaunchOutcome, OctError> {
                Err(OctError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no xdg-open",
                )))
            }
        }

        let dir = TempDir::new("launcher-failure");
        let file = dir.path.join("Cosmos.pdf");
        std::fs::write(&file, b"not really a pdf").expect("write the file");

        let result = open_containing_folder(&FailingLauncher, &file);
        match result {
            Err(OctError::Io(error)) => assert!(error.to_string().contains("no xdg-open")),
            other => panic!("expected an io error, got {other:?}"),
        }
    }

    // --- R22: a subfolder of a watched folder is refused, and the refusal says why ------------

    #[test]
    fn scanning_a_subfolder_of_a_watched_folder_is_refused_by_name() {
        let store = in_memory_store();
        let identity = oct_platform::backends().file_identity;
        let dir = TempDir::new("nested-scan");
        let library = dir.path.join("library");
        let subfolder = library.join("pdfs");
        std::fs::create_dir_all(&subfolder).expect("create the folders");
        // The store holds the canonical root, because that is what the scanner records (T6), and
        // the chosen path is resolved the same way so the two spellings can be compared at all.
        let watched_root = library.canonicalize().expect("canonical root");
        store.record_folder(&watched_root, true).expect("record");

        let result = scan_path(&store, identity.as_ref(), &subfolder);

        match result {
            Err(OctError::PathCoveredByWatchedFolder { path, covering }) => {
                assert!(path.ends_with("pdfs"), "names the refused folder: {path}");
                assert!(
                    covering.ends_with("library"),
                    "names the folder that already covers it: {covering}"
                );
            }
            other => panic!("expected the nesting refusal, got {other:?}"),
        }
        assert_eq!(
            store.folders().expect("list folders").len(),
            1,
            "a refused folder must not be recorded"
        );
    }

    #[test]
    fn scanning_a_folder_a_watch_already_covers_is_refused_for_the_folder_itself_too() {
        let store = in_memory_store();
        let identity = oct_platform::backends().file_identity;
        let dir = TempDir::new("same-scan");
        let library = dir.path.join("library");
        std::fs::create_dir_all(&library).expect("create the folder");
        let watched_root = library.canonicalize().expect("canonical root");
        store.record_folder(&watched_root, true).expect("record");

        let result = scan_path(&store, identity.as_ref(), &library);
        assert!(
            matches!(result, Err(OctError::PathCoveredByWatchedFolder { .. })),
            "re-adding the watched folder is the same refusal, not a second watch: {result:?}"
        );
        assert_eq!(store.folders().expect("list folders").len(), 1);
    }

    #[test]
    fn the_refusal_reads_as_a_sentence_naming_the_covering_folder() {
        // The interface renders `OctError`'s own message, so the message — not only the two fields —
        // has to carry the answer: "it was blocked" without "by which one" is a half-answer.
        let error = OctError::PathCoveredByWatchedFolder {
            path: "/library/pdfs/book".to_string(),
            covering: "/library/pdfs".to_string(),
        };
        let message = error.to_string();

        assert!(message.contains("/library/pdfs/book"), "names the choice: {message}");
        assert!(
            message.contains("/library/pdfs"),
            "names the covering folder: {message}"
        );
        assert!(
            message.contains("already covered") && message.contains("not added"),
            "states what happened and why: {message}"
        );
    }

    // --- R24: a watched folder opens itself, not its parent ------------------------------------

    #[test]
    fn opening_a_watched_folder_asks_the_launcher_for_that_folder_itself() {
        let dir = TempDir::new("open-watched");
        let folder = dir.path.join("libros");
        std::fs::create_dir_all(&folder).expect("create the folder");
        std::fs::write(folder.join("Cosmos.pdf"), b"not really a pdf").expect("write a file");

        let launcher = RecordingLauncher::new();
        let opened = open_folder(&launcher, &folder).expect("open the watched folder");

        assert_eq!(opened.folder, display_path(&folder));
        assert_eq!(opened.viewer, "test-file-manager");

        let calls = launcher.calls.lock().expect("the recording lock");
        assert_eq!(calls.len(), 1, "exactly one launch");
        assert_eq!(calls[0].0, folder, "the folder itself, not its parent");
        assert_eq!(calls[0].1, None, "opening a folder is not a page request");
    }

    #[test]
    fn opening_a_watched_folder_that_is_gone_is_an_honest_error() {
        let dir = TempDir::new("gone-watched");
        let folder = dir.path.join("libros");
        std::fs::create_dir_all(&folder).expect("create the folder");
        // The watch outlives the folder it names: nothing re-checks the disk until someone asks.
        std::fs::remove_dir_all(&folder).expect("remove the folder");

        let launcher = RecordingLauncher::new();
        let result = open_folder(&launcher, &folder);

        match result {
            Err(OctError::InvalidPath(message)) => {
                assert!(message.contains("libros"), "names the folder: {message}");
            }
            other => panic!("expected an InvalidPath error, got {other:?}"),
        }
        assert!(
            launcher.calls.lock().expect("the recording lock").is_empty(),
            "a folder that is gone must not be launched"
        );
    }

    // --- R1: the language the interface and the tray share ------------------------------------

    #[test]
    fn the_language_is_stored_and_read_back() {
        let store = in_memory_store();
        assert_eq!(get_language(&store).expect("read"), None);

        set_language(&store, "es").expect("store Spanish");
        assert_eq!(get_language(&store).expect("read"), Some("es".to_string()));

        set_language(&store, "en").expect("store English");
        assert_eq!(get_language(&store).expect("read"), Some("en".to_string()));
    }

    #[test]
    fn an_unknown_locale_is_rejected_and_not_stored() {
        let store = in_memory_store();
        let result = set_language(&store, "fr");
        // `R35`. The variant matters, and so do its three fields: the whole reason this is not `Storage`
        // is that whoever reads the failure must be able to see WHICH value arrived, in which field, and
        // what the build accepts — so the test asserts all three rather than only the variant's name.
        match result.expect_err("an unknown locale must be refused") {
            OctError::InvalidInput {
                field,
                value,
                expected,
            } => {
                assert_eq!(field, "locale");
                assert_eq!(value, "fr");
                assert_eq!(expected, "en or es");
            }
            other => panic!("a bad locale must be an invalid argument, not {other:?}"),
        }
        assert_eq!(
            store.get_setting("language").expect("read the raw setting"),
            None,
            "a refused locale must never reach the store"
        );
    }

    #[test]
    fn a_stored_value_that_is_not_a_language_never_reaches_a_caller() {
        let store = in_memory_store();
        // A newer build, a hand-edited database or a typo can leave anything under this key. A
        // caller must see "no language" rather than a value it cannot word the tray with.
        store
            .set_setting("language", "fr")
            .expect("write a value the build does not know");
        assert_eq!(
            get_language(&store).expect("read"),
            None,
            "an unknown stored value is reported as no language, not as one"
        );
    }

    // --- R32: the cover's data URL and its file name ------------------------------------------

    #[test]
    fn the_cover_is_a_png_data_url_in_the_rfc_4648_encoding() {
        // RFC 4648's own vectors, so the encoder is pinned against the standard rather than against
        // itself. The three padding lengths are the cases that can go wrong.
        let encoded = |input: &[u8]| {
            let mut out = String::new();
            base64_encode(input, &mut out);
            out
        };
        assert_eq!(encoded(b""), "");
        assert_eq!(encoded(b"f"), "Zg==");
        assert_eq!(encoded(b"fo"), "Zm8=");
        assert_eq!(encoded(b"foo"), "Zm9v");
        assert_eq!(encoded(b"foob"), "Zm9vYg==");
        assert_eq!(encoded(b"fooba"), "Zm9vYmE=");
        assert_eq!(encoded(b"foobar"), "Zm9vYmFy");

        // The full URL: the prefix a browser needs, then the PNG signature's own encoding, so the
        // bytes the interface receives are an image and not text pretending to be one.
        let url = png_data_url(b"\x89PNG\r\n\x1a\n");
        assert!(url.starts_with("data:image/png;base64,"), "{url}");
        assert_eq!(&url["data:image/png;base64,".len()..], "iVBORw0KGgo=");
    }

    #[test]
    fn the_cover_file_carries_the_page_inside_the_covers_directory() {
        assert_eq!(
            covers_directory(Path::new("/data/oct")),
            PathBuf::from("/data/oct/covers")
        );
        // The page is part of the name (R32): a name without it would keep serving the old page after
        // the reader chose another one. Two pages, two files, neither one stale.
        assert_eq!(
            cover_file(Path::new("/data/oct/covers"), 42, 0),
            PathBuf::from("/data/oct/covers/42-p0.png")
        );
        assert_eq!(
            cover_file(Path::new("/data/oct/covers"), 42, 7),
            PathBuf::from("/data/oct/covers/42-p7.png")
        );
    }

    #[test]
    fn the_library_view_carries_the_readers_cover_choice() {
        let store = in_memory_store();
        let id = store
            .upsert_document(&document("/library/read.pdf", 4, 4))
            .expect("insert");

        // No choice yet: `null` means the first page, and the view says so rather than inventing `0`.
        let entries = library_entries(&store).expect("list");
        assert_eq!(entries[0].cover_page, None);
        assert_eq!(entries[0].cover_image, None);

        set_cover_page(&store, id, Some(4)).expect("choose a page");
        let entries = library_entries(&store).expect("list again");
        assert_eq!(entries[0].cover_page, Some(4));
        assert_eq!(entries[0].cover_image, None);
    }

    #[test]
    fn the_reader_supplied_image_wins_over_the_page_they_chose() {
        // No pdfium is bound and the document's path does not exist: if the page were consulted at all
        // this test could not produce a cover. It succeeds precisely because the image short-circuits
        // the page, which is the precedence R32 freezes.
        let store = in_memory_store();
        let id = store
            .upsert_document(&document("/library/gone.pdf", 5, 5))
            .expect("insert");
        set_cover_page(&store, id, Some(3)).expect("choose a page");

        let directory = TempDir::new("cover-image");
        let source = directory.path.join("chosen.png");
        fs::write(&source, b"\x89PNG\r\n\x1a\nnot really a full png, but the bytes say image")
            .expect("write the reader's image");
        set_cover_image(&store, id, &directory.path, Some(&source)).expect("copy the image");

        let cover = document_cover(&store, id, &directory.path)
            .expect("the image cover is produced")
            .expect("the image cover exists");
        assert!(cover.starts_with("data:image/png;base64,"), "{cover}");
    }

    #[test]
    fn the_page_the_reader_chose_is_the_page_the_renderer_uses() {
        // The whole path, pdfium included: the same document rendered at a chosen page comes back as a
        // cover, while a page the PDF does not have comes back as no cover. If the page were ignored,
        // the second call would have served page one's cached cover instead of reporting none. That the
        // two valid pages render different BYTES is pinned in `oct-scan`'s cover tests, where a two-page
        // PDF with visibly different pages can be built; the repository's fixture has identical pages.
        let directory = TempDir::new("cover-page");
        let pdf = directory.path.join("document.pdf");
        fs::write(
            &pdf,
            include_bytes!("../../oct-scan/tests/fixtures/with-info-metadata.pdf"),
        )
        .expect("write the fixture");

        let store = in_memory_store();
        let id = store
            .upsert_document(&document(
                pdf.to_str().expect("utf-8 path"),
                6,
                6,
            ))
            .expect("insert");

        set_cover_page(&store, id, Some(1)).expect("choose page two");
        let chosen = document_cover(&store, id, &directory.path)
            .expect("render the chosen page")
            .expect("page two has a cover");
        assert!(chosen.starts_with("data:image/png;base64,"), "{chosen}");

        // A page beyond the end is the reader's choice too, and it is no cover rather than an error or
        // a silently substituted page one.
        set_cover_page(&store, id, Some(99)).expect("choose a page the PDF does not have");
        assert_eq!(
            document_cover(&store, id, &directory.path).expect("no cover, no error"),
            None
        );
    }

    #[test]
    fn a_file_that_is_not_an_image_is_refused_as_an_invalid_argument() {
        let store = in_memory_store();
        let id = store
            .upsert_document(&document("/library/read.pdf", 7, 7))
            .expect("insert");
        let directory = TempDir::new("cover-bad-image");
        let not_an_image = directory.path.join("notes.txt");
        fs::write(&not_an_image, b"this is text, not a picture").expect("write the text file");

        let result = set_cover_image(&store, id, &directory.path, Some(&not_an_image));
        match result.expect_err("a non-image must be refused") {
            OctError::InvalidInput {
                field,
                value,
                expected,
            } => {
                // R35: the variant and all three of its fields. A bad argument reported as `Storage`
                // would send whoever reads the log to look at the disk, which was never involved.
                assert_eq!(field, "cover image");
                assert!(value.ends_with("notes.txt"), "{value}");
                assert!(expected.contains("image"), "{expected}");
            }
            other => panic!("a non-image must be an invalid argument, not {other:?}"),
        }

        // Nothing was written and nothing was recorded: the document still has no image.
        let entries = library_entries(&store).expect("list");
        assert_eq!(entries[0].cover_image, None);
    }

    #[test]
    fn an_id_that_names_no_document_has_no_cover_and_no_error() {
        let store = in_memory_store();
        assert_eq!(
            document_cover(&store, 999, Path::new("/data/oct")).expect("no document, no cover"),
            None
        );
    }

    // --- R4: the suggestion queue's producer and its decisions -----------------------------

    #[test]
    fn a_document_inside_a_watched_folder_is_never_suggested() {
        // The watched folder is the source of truth. A PDF under one that is not in the library is a
        // scan that has not counted it yet, not a document the reader should be asked to review.
        let store = in_memory_store();
        store
            .record_folder("/library/pdfs", true)
            .expect("watch the folder");

        let detector =
            FixedDetector::seeing(&["/library/pdfs/inside.pdf", "/elsewhere/open.pdf"]);
        let recorded = observe_open_documents(&store, &detector).expect("observe");

        assert_eq!(recorded, 1, "only the document outside the watch is a suggestion");
        let suggestions = list_suggestions(&store).expect("list");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].path, "/elsewhere/open.pdf");
    }

    #[test]
    fn a_document_already_in_the_library_is_not_suggested() {
        let store = in_memory_store();
        store
            .upsert_document(&document("/library/known.pdf", 1, 1))
            .expect("insert");

        let detector = FixedDetector::seeing(&["/library/known.pdf"]);
        assert_eq!(observe_open_documents(&store, &detector).expect("observe"), 0);
        assert!(list_suggestions(&store).expect("list").is_empty());
    }

    #[test]
    fn the_same_document_open_on_two_processes_is_one_suggestion_per_observation() {
        // The detector reports one entry per `(path, pid)`; the queue counts documents, not readers.
        // One observation bumps the sighting once — a file held by two processes is still one file —
        // and the next observation is what makes it a second sighting.
        let store = in_memory_store();
        let detector = FixedDetector {
            documents: vec![
                OpenDocument {
                    path: PathBuf::from("/elsewhere/open.pdf"),
                    pid: Some(1),
                    viewer: None,
                },
                OpenDocument {
                    path: PathBuf::from("/elsewhere/open.pdf"),
                    pid: Some(2),
                    viewer: None,
                },
            ],
        };

        assert_eq!(observe_open_documents(&store, &detector).expect("observe"), 1);
        let suggestions = list_suggestions(&store).expect("list");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].seen_count, 1, "one observation, one sighting");

        observe_open_documents(&store, &detector).expect("observe again");
        let suggestions = list_suggestions(&store).expect("list");
        assert_eq!(suggestions.len(), 1, "still one row");
        assert_eq!(suggestions[0].seen_count, 2, "the next observation is the second");
    }

    #[test]
    fn accepting_a_suggestion_adds_the_document_then_records_the_decision() {
        // The whole feature: the reader's accept is the only thing that turns a suggestion into
        // library data, and it does so by persisting the document and only then recording the
        // decision. The repository's committed fixture is scanned read-only.
        let store = in_memory_store();
        let identity = oct_platform::backends().file_identity;
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/oct-scan/tests/fixtures/with-info-metadata.pdf");
        let id = store
            .record_suggestion(&fixture)
            .expect("record the suggestion");

        accept_suggestion(&store, identity.as_ref(), id).expect("accept");

        let entries = library_entries(&store).expect("list");
        assert_eq!(entries.len(), 1, "the document joined the library");
        assert_eq!(entries[0].path, fixture.to_string_lossy());
        assert!(entries[0].pages.is_some(), "the fixture is countable");

        let suggestion = store.suggestion(id).expect("read").expect("present");
        assert_eq!(suggestion.decision, Some(SuggestionDecision::Accepted));
        assert!(suggestion.decided_at.is_some());
        assert!(
            list_suggestions(&store).expect("list").is_empty(),
            "a decided suggestion leaves the queue"
        );
    }

    #[test]
    fn a_failed_accept_leaves_the_suggestion_undecided() {
        // The load-bearing ordering rule: the decision is written only after the document is
        // persisted. A file that has since disappeared fails the scan, so a decision written first
        // would leave the queue claiming an accepted document that does not exist.
        let store = in_memory_store();
        let identity = oct_platform::backends().file_identity;
        let missing = PathBuf::from("/library/gone-before-accept-7f31.pdf");
        let id = store.record_suggestion(&missing).expect("record");

        let error =
            accept_suggestion(&store, identity.as_ref(), id).expect_err("the file is gone");
        assert!(matches!(error, OctError::InvalidPath(_)), "got {error:?}");

        let suggestion = store.suggestion(id).expect("read").expect("present");
        assert!(
            suggestion.decision.is_none(),
            "no decision may exist without its document"
        );
        assert!(
            list_suggestions(&store).expect("list").iter().any(|s| s.id == id),
            "the undecided suggestion is still offered"
        );
    }

    #[test]
    fn discarding_a_suggestion_records_the_decision() {
        let store = in_memory_store();
        let id = store
            .record_suggestion("/elsewhere/open.pdf")
            .expect("record");

        discard_suggestion(&store, id).expect("discard");

        let suggestion = store.suggestion(id).expect("read").expect("present");
        assert_eq!(suggestion.decision, Some(SuggestionDecision::Discarded));
        assert!(list_suggestions(&store).expect("list").is_empty());
    }

    #[test]
    fn a_suggestion_cannot_be_decided_twice_through_the_commands() {
        let store = in_memory_store();
        let id = store
            .record_suggestion("/elsewhere/open.pdf")
            .expect("record");
        discard_suggestion(&store, id).expect("the first decision");

        let error = discard_suggestion(&store, id).expect_err("a second decision must be refused");
        assert!(
            matches!(error, OctError::SuggestionAlreadyDecided { .. }),
            "got {error:?}"
        );
    }
}
