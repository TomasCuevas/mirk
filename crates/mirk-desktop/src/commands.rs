//! The commands the screens call, with their logic in plain functions.
//!
//! The Tauri wrappers in `main.rs` are deliberately thin: they open the production database, call
//! one of the functions here, and render an [`mirk_core::MirkError`] into a string an IPC caller can
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
use std::sync::atomic::{AtomicU64, Ordering};

use mirk_core::domain::CoverChoice;
use mirk_core::{
    CoverSource, FileIdentity, LaunchOutcome, Launcher, MirkError, OpenDocumentDetector,
    RecentDocumentsSource, StoredProgress,
};
use mirk_scan::{cover_for, CoverOutcome, ScanReport, Scanner};
use mirk_store::{
    covering_folder, AuthorEdit, DocumentRecord, FolderRemoval, LibraryEntry, Store, Suggestion,
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
    /// The authors to show, resolved by the core (R42/S4): the reader's ordered list when it has any
    /// entry, otherwise the document's embedded author as a single entry, otherwise empty (`authors`
    /// over IPC).
    ///
    /// The precedence is **not** decided here: [`Document::display_authors`] is the one place it
    /// lives, so the card, the filter and the search cannot disagree about who wrote a document. The
    /// embedded string is never split on commas, semicolons or "and" (R42/S6) — an author the file's
    /// metadata happened to hold is one entry, and a reader who wants a real list writes one.
    ///
    /// This replaces the single `author` field the card used to read: it is the same value, whole.
    pub authors: Vec<String>,
    /// **Temporary**: the single author the card reads until the web app moves to [`Self::authors`].
    ///
    /// It exists so this commit can land without breaking the interface that is still reading it — the
    /// list is the fact and this is a projection of its first entry. It is removed in the commit that
    /// moves the web app to the list, and nothing may start reading it in the meantime. It is
    /// serialized as an explicit `null` when absent, exactly as the field it stands in for always was:
    /// omitting the key would be an output change a reader of the old shape could not distinguish from
    /// "the property was never there".
    pub author: Option<String>,
    /// **Temporary**: the reader's single author, for the same reason, and `null` for the same reason.
    pub user_author: Option<String>,
    /// The reader's own authors, in their order, or empty when they have set none
    /// (`readerAuthors` over IPC).
    ///
    /// A separate fact from [`Self::authors`], exactly as `user_title` is separate from `title`: it
    /// is what tells the editor whether the names on screen are the reader's, and what the editor's
    /// suggestion list draws on beside the displayed ones. It is read from the same rows in the same
    /// order, so it cannot disagree with what `authors` resolved.
    pub reader_authors: Vec<String>,
    pub pages: Option<u32>,
    pub progress: Option<ProgressView>,
    /// The page the reader chose for the cover, zero-based, or `null` for the first page (R32).
    pub cover_page: Option<u32>,
    /// The file name of an image the reader supplied, inside the covers directory, or `null` (R32).
    pub cover_image: Option<String>,
    /// Which of the two sources is in force (R19): `"page"`, `"image"`, or `null` for a library written
    /// before the choice existed. The interface needs it to say what the cover *is* and to offer "use
    /// the image you saved" without deleting anything.
    pub cover_source: Option<String>,
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
            authors: document.document.display_authors(),
            // The temporary single-author projection, from the same list so the two cannot disagree.
            author: document.document.display_author(),
            user_author: document.document.authors.first().cloned(),
            reader_authors: document.document.authors.clone(),
            pages: document.document.pages,
            cover_page: document.document.cover_page,
            cover_image: document.document.cover_image.clone(),
            cover_source: document
                .cover_source
                .map(|source| source.as_str().to_string()),
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

/// What the scan command returns. Mirrors [`mirk_scan::ScanReport`] with paths as strings.
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

/// The document a card action opened, the program that opened it, and the page it was opened at.
///
/// `page` is the zero-based page the document was resumed at, or `None` when the document was
/// opened at its beginning — which is what happens for a document no source has ever reported a
/// position for. It travels back so a caller can tell the two apart: "opened" and "opened where I
/// left it" are different promises, and only the second one is a promise about the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenedDocumentView {
    pub path: String,
    pub viewer: String,
    pub page: Option<u32>,
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
    /// Unix seconds when the desktop's history recorded the reader opening it, or `null` when no
    /// source knows (`historyAt` over IPC).
    ///
    /// The reader's own moment, kept apart from `first_seen_at` — when the application noticed the
    /// file — for the reason L46 recorded: one column for both made every row look freshly read. The
    /// screen orders by this, with the undated rows after every dated one, and `null` is "no source
    /// said" rather than 1970.
    pub history_at: Option<i64>,
}

impl From<Suggestion> for SuggestionView {
    fn from(suggestion: Suggestion) -> Self {
        Self {
            id: suggestion.id,
            path: display_path(&suggestion.path),
            seen_count: suggestion.seen_count,
            first_seen_at: suggestion.first_seen_at,
            history_at: suggestion.history_at,
        }
    }
}

/// The library listing: every document with its latest progress and provenance.
pub fn library_entries(store: &Store) -> Result<Vec<LibraryEntryView>, MirkError> {
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
/// `authors` is the reader's ordered **list** (R42/S5), and the shape carries the same distinction the
/// store keeps: `None` means this action said nothing about the authors — Restore decides the title and
/// nothing else — while `Some(vec![])` is *no author of my own*, a decision that clears the reader's
/// list so the document's own author shows again. `Some(vec![..])` is the reader's list, trimmed,
/// blank-free and deduplicated in the order given by the store, in one write with the title. The
/// one-author spelling ([`AuthorEdit::Decided`]) stays available in the store, but the IPC shape this
/// build speaks is a list, because the interface must be able to send several names.
///
/// Nothing here touches the document's own title or author: both reader's facts are separate, and a
/// document can always be restored to what the file itself carries. `documents.author` is never split
/// on commas, semicolons or "and" (R42/S6): a reader who wants a real list types one.
pub fn set_document_naming(
    store: &Store,
    id: i64,
    title: Option<&str>,
    authors: Option<Vec<String>>,
) -> Result<(), MirkError> {
    // The store speaks in `AuthorEdit`, whose two spellings are "a decision" and "a silence". The
    // `Option` here is exactly that distinction, so the conversion is a match and nothing else — the
    // decisions about trimming, blanks and duplicates stay in the store, where every caller gets them.
    let borrowed: Vec<&str> = authors
        .as_ref()
        .map(|list| list.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let author = match &authors {
        Some(_) => AuthorEdit::DecidedList(&borrowed),
        None => AuthorEdit::Untouched,
    };
    store.set_user_naming(id, title, author)
}

/// Sets or clears the page the reader chose for one document's cover (R32).
///
/// `page` is zero-based, exactly as pages are stored everywhere in this workspace; the interface
/// converts from the one-based number it shows. `None` restores the default, the first page, and is a
/// legitimate action rather than an error. The decision itself — a `NULL` page meaning the first —
/// lives in [`Document::cover_choice`], so this command only carries the reader's answer there.
pub fn set_cover_page(store: &Store, id: i64, page: Option<u32>) -> Result<(), MirkError> {
    store.set_cover_page(id, page)
}

/// Makes the image the reader already supplied the cover again, without touching the file (R19).
///
/// This is the other half of "a page can take over": once the page is the cover, the image is still
/// stored, and this brings it back. It refuses when there is no image to return to — a source saying
/// `image` over no file would be a broken cover, which is worse than a refusal the screen can explain.
pub fn use_cover_image(store: &Store, id: i64) -> Result<(), MirkError> {
    let Some(record) = store.document_by_id(id)? else {
        return Err(MirkError::Storage(format!(
            "no document with id {id} to use a cover image on"
        )));
    };
    match record.document.cover_image.as_deref() {
        Some(name) if !name.trim().is_empty() => store.set_cover_source(id, CoverSource::Image),
        _ => Err(MirkError::InvalidInput {
            field: "cover image",
            value: "<none>".to_string(),
            expected: "a document with an image already supplied".to_string(),
        }),
    }
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
/// refused as [`MirkError::InvalidInput`] — the variant this project added for exactly this kind of lie
/// (R35) — and never as a storage failure: the database was never involved, and reporting it as one
/// would send whoever reads the log to look at the disk. Checking signatures needs no dependency, and
/// the browser's `<img>` remains the thing that actually decodes the image.
pub fn set_cover_image(
    store: &Store,
    id: i64,
    data_directory: &Path,
    source: Option<&Path>,
) -> Result<(), MirkError> {
    let Some(source) = source else {
        // Removing the image leaves the file on disk until another image replaces it. That is
        // deliberate: the file is inert once nothing references it, and deleting user data on an
        // action shaped like "undo" is how a reader loses an image they meant to keep.
        return store.set_cover_image(id, None);
    };

    let bytes = fs::read(source)?;
    let Some(mime) = image_mime(&bytes) else {
        return Err(MirkError::InvalidInput {
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
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), MirkError> {
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
/// this module: `main.rs` derives it from `mirk_store::database_path`, so the covers and the database
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
) -> Result<Option<String>, MirkError> {
    let Some(record) = document_by_id(store, id)? else {
        // An id that names nothing is "no cover", the same answer as a document with no readable
        // PDF. The interface only ever asks about documents it is showing, so this is a race at
        // worst, not a caller error worth a message on screen.
        return Ok(None);
    };

    let covers = covers_directory(data_directory);
    // R19: the RECORD decides, because the choice is a stored fact — an image and a page can both be
    // kept, and only the source says which one is the cover.
    match record.cover_choice() {
        CoverChoice::Image(name) => cover_from_image(&covers, &name, &record.document.path),
        CoverChoice::Page(page) => cover_from_page(&covers, id, page, &record.document.path),
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
) -> Result<Option<String>, MirkError> {
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
) -> Result<Option<String>, MirkError> {
    let cover_path = cover_file(covers, id, page);

    let pdfium = mirk_scan::bind_pdfium_library().map_err(|error| {
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

// --- L60: the preview of a suggestion, which has no row to key a cache on ---

/// A unique temporary path for one preview render.
///
/// The process id separates concurrent runs of the application and the serial separates concurrent
/// renders inside one run. Both are only names: nothing is ever found again by this name, which is
/// exactly why the preview needs no cache key it could collide on.
fn preview_file() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("mirk-preview-{}-{serial}.png", std::process::id()))
}

/// The first page of a document that is not in the library yet, as a PNG data URL (L60).
///
/// A suggestion is a path the application observed open, not a row: it has no id, so it has no name
/// in the cover cache document_cover keeps (a row id and a page). Inventing one from the path would
/// mean either a hash — which can one day serve one document's page for another — or a sanitised
/// path, which has no useful length bound. So the preview is rendered to a temporary file, read
/// back and discarded: nothing is cached, nothing can go stale, and nothing can be attributed to the
/// wrong document. The cost is one render per card, and a card asks once.
///
/// Page zero is the FIRST page, the same default []CoverChoice] gives a document whose reader has
/// chosen nothing — which is every suggested document, since nobody can have chosen a page for a
/// file that is not in the library (R32).
///
/// `None` is the answer document_cover already gives for a document it cannot read: gone, not a PDF,
/// encrypted, unreadable. The card keeps its placeholder and the reason goes to stderr. A failure of
/// the application itself — no pdfium — stays an error, because reporting "no cover" when the
/// renderer could not even be loaded would be a lie about the document.
pub fn suggestion_cover(document: &Path) -> Result<Option<String>, MirkError> {
    let pdfium = mirk_scan::bind_pdfium_library().map_err(|error| {
        eprintln!(
            "the preview of {} could not be produced: {error}",
            display_path(document)
        );
        error
    })?;

    let preview = preview_file();
    // Read FIRST, then clean up. The first version of this deleted the temporary files before
    // reading the page it had just rendered, and the test said so in one line: "could not be read
    // back: No such file or directory".
    let result = match cover_for(&pdfium, &preview, document, 0) {
        Ok(CoverOutcome::Ready { path }) => match fs::read(&path) {
            Ok(bytes) => Ok(Some(png_data_url(&bytes))),
            Err(error) => {
                eprintln!(
                    "the preview of {} could not be read back: {error}",
                    display_path(document)
                );
                Err(error.into())
            }
        },
        Ok(CoverOutcome::Unavailable { reason }) => {
            eprintln!("no preview for {}: {reason}", display_path(document));
            Ok(None)
        }
        Err(error) => {
            eprintln!(
                "the preview of {} could not be produced: {error}",
                display_path(document)
            );
            Err(error)
        }
    };

    // Whatever happened, the temporary files do not outlive the request.
    let _ = fs::remove_file(&preview);
    let _ = fs::remove_file(preview.with_extension("tmp"));

    result
}

/// The document with `id`, cover choice included, or `None` when the id names nothing.
///
/// The cover belongs to the document row, not to a path the interface hands back, so the row is
/// resolved from the store. The listing is the only lookup `mirk-store` exposes today; it is ordered
/// by path, which is irrelevant to a `find` by id.
fn document_by_id(store: &Store, id: i64) -> Result<Option<DocumentRecord>, MirkError> {
    Ok(store
        .library()?
        .into_iter()
        .find(|entry| entry.document.id == id)
        .map(|entry| entry.document))
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
) -> Result<ScanReportView, MirkError> {
    let root = folder.canonicalize()?;

    if let Some(covering) = covering_folder(&store.folders()?, &root) {
        return Err(MirkError::PathCoveredByWatchedFolder {
            path: display_path(&root),
            covering: display_path(&covering.path),
        });
    }

    let scanner = Scanner::new(store, identity);
    Ok(ScanReportView::from(scanner.scan(&root)?))
}

/// The progress providers this build knows, with their declared capability and live availability.
pub fn provider_registry() -> Vec<ProviderView> {
    mirk_platform::progress_providers()
        .iter()
        .map(|provider| ProviderView {
            id: provider.id().to_string(),
            capability: provider.capability().id().to_string(),
            availability: provider.detect().id().to_string(),
        })
        .collect()
}

// --- L61: whether this machine can say which documents are open at all ---

/// Whether the machine can report the documents that are open, as the interface must see it (R4).
///
/// Without this, an empty queue and a machine with no detector are the same screen: "nothing is
/// waiting" would be a claim the application cannot support, and the reader would have no reason to
/// look again. The producer propagates the same fact as MirkError::NotSupported, and this is its
/// reader-facing half.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectorView {
    /// The detector's own identifier, so the screen can name which one answered.
    pub id: String,
    /// "installed", "unsupported" or "unavailable" — whether an empty queue can be believed.
    pub availability: String,
}

/// Asks the detector once and reports what it said.
///
/// The answer comes from asking rather than from a declaration, because OpenDocumentDetector has no
/// way to declare itself — its trait offers an id, a platform and the documents it sees, and nothing
/// else. Asking also answers the only question the screen actually has: can an empty queue be
/// believed on this machine?
///
/// The failure is classified rather than flattened: a capability the platform does not implement is
/// unsupported and stays empty forever, while a detector that failed this time is unavailable and may
/// answer on the next ask. Both must keep the screen from saying the queue is empty. Neither is an
/// error to raise, because neither is a defect of this application.
pub fn suggestion_detector(detector: &dyn OpenDocumentDetector) -> DetectorView {
    let availability = match detector.open_documents() {
        Ok(_) => "installed",
        Err(MirkError::NotSupported { .. }) => "unsupported",
        Err(_) => "unavailable",
    };

    DetectorView {
        id: detector.id().to_string(),
        availability: availability.to_string(),
    }
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
/// [`MirkError::NotSupported`] from the detector is propagated rather than swallowed: on a platform
/// with no detector the caller must be able to say the capability is missing instead of showing an
/// empty queue that merely looks like good news.
///
/// Returns how many distinct paths were recorded. The detector may report one path under more than
/// one process, and this counts a document, not its readers.
pub fn observe_open_documents(
    store: &Store,
    detector: &dyn OpenDocumentDetector,
) -> Result<usize, MirkError> {
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

/// How many suggestions one pass records.
///
/// This bounds what the pass **records**, not what it asks the source for, and it keeps the newest
/// entries because the source answers newest first. The two are not interchangeable: asking the
/// source for this many entries would spend the window on entries the pass then rejects, so a
/// history longer than the bound — or one whose newest entries are all already in the library or
/// inside a watched folder — would hide the eligible documents behind them for good, which is the
/// opposite of S10's "every eligible PDF". The source reads the whole history file either way, so
/// the only thing an early limit bought was the omission.
const RECENT_SUGGESTION_LIMIT: usize = 1_000;

/// Records a suggestion for every PDF the desktop's history names that is neither in the library nor
/// inside a watched folder (S10 of `suggestions-screen.md`).
///
/// The sibling producer of [`observe_open_documents`], over the other axis: that one asks what is
/// open right now and records the moment the application noticed; this one asks what the reader
/// opened at some point in the past, and the history's own moment travels with the suggestion. It is
/// a separate column from `first_seen_at`, because "when the reader opened it" and "when mirk
/// looked" are different facts (L46 of `pdf-progress-tracker.md`).
///
/// Like its sibling it decides nothing: a watched folder is the source of truth, a document already
/// in the library is nothing to suggest, and accepting or dismissing stays the reader's
/// ([`accept_suggestion`], [`discard_suggestion`]). Decided rows are left untouched by the store, so
/// a dismissed document cannot come back through this path.
///
/// The history is read on demand — when the interface asks — rather than on the ingestion tick: the
/// tick runs every few seconds and would bump every suggestion's sighting count on each pass, which
/// is neither what the counter means nor a reason for a disk read that often.
///
/// Returns how many distinct paths this pass recorded. A source that cannot answer — no backend on
/// this platform — is propagated instead of being reported as an empty queue.
pub fn observe_recent_documents(
    store: &Store,
    source: &dyn RecentDocumentsSource,
) -> Result<usize, MirkError> {
    record_recent_documents(store, source, RECENT_SUGGESTION_LIMIT)
}

/// [`observe_recent_documents`] with the pass's window as a parameter.
///
/// Separate only so the window's rule can be tested with a bound small enough to read: "the newest
/// entries first" and "the bound applies to what is recorded" are one rule, and a test needs only
/// a window of one and a history whose newest entry is already known to tell it from the other one.
fn record_recent_documents(
    store: &Store,
    source: &dyn RecentDocumentsSource,
    limit: usize,
) -> Result<usize, MirkError> {
    // Everything the source has, rather than the window: the three exclusions below are this pass's
    // own judgement, and a limit here would drop eligible documents before they were even looked
    // at. The window is applied to the entries actually recorded.
    let recent = source.recent(usize::MAX)?;
    let entries = store.library()?;
    let folders = store.folders()?;

    let mut recorded = std::collections::HashSet::new();
    for document in &recent {
        // The window is spent by recordings, and the source is newest first, so what survives it is
        // the newest eligible entries rather than the newest entries.
        if recorded.len() >= limit {
            break;
        }
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
        store.record_suggestion_with_history(&document.path, document.last_used_unix)?;
        recorded.insert(document.path.clone());
    }
    Ok(recorded.len())
}

/// The suggestions still awaiting the reader's decision, oldest first.
pub fn list_suggestions(store: &Store) -> Result<Vec<SuggestionView>, MirkError> {
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
) -> Result<(), MirkError> {
    let suggestion = store
        .suggestion(id)?
        .ok_or_else(|| MirkError::Storage(format!("no suggestion with id {id} to accept")))?;
    if let Some(decision) = suggestion.decision {
        return Err(MirkError::SuggestionAlreadyDecided {
            path: display_path(&suggestion.path),
            decision: decision.id(),
        });
    }

    if !suggestion.path.is_file() {
        return Err(MirkError::InvalidPath(format!(
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
        return Err(MirkError::Storage(format!(
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
pub fn discard_suggestion(store: &Store, id: i64) -> Result<(), MirkError> {
    store.decide_suggestion(id, SuggestionDecision::Discarded)?;
    Ok(())
}

/// Every folder the store has recorded, oldest first.
pub fn watched_folders(store: &Store) -> Result<Vec<WatchedFolderView>, MirkError> {
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
pub fn folder_removal(store: &Store, id: i64) -> Result<Option<FolderRemovalView>, MirkError> {
    Ok(store.folder_removal(id)?.map(FolderRemovalView::from))
}

/// Stops watching the folder with `id`, taking the documents it covers and their positions with it.
///
/// The decision is [`Store::forget_folder`]'s, including the guard that keeps a document another
/// remaining watch still covers. The view it returns is the same plan [`folder_removal`] shows, so
/// what the confirmation said and what actually left cannot disagree. All this adds is the store
/// the production command opens.
pub fn remove_folder(store: &Store, id: i64) -> Result<Option<FolderRemovalView>, MirkError> {
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
pub fn get_language(store: &Store) -> Result<Option<String>, MirkError> {
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
pub fn set_language(store: &Store, locale: &str) -> Result<(), MirkError> {
    if !shell::is_known_language(locale) {
        // `R35`: a bad argument answers as a bad argument. This used to be `Storage`, which was a small
        // lie in a log — the database was never involved, and anyone reading a failure later would look
        // in the wrong place. Not reachable from the interface today: the selector offers what
        // `shell::LANGUAGES` holds.
        return Err(MirkError::InvalidInput {
            field: "locale",
            value: locale.to_string(),
            expected: shell::LANGUAGES.join(" or "),
        });
    }
    store.set_setting(LANGUAGE_SETTING, locale)
}

/// The stored window layout, but only when it is one this build knows.
///
/// The same rule as [`get_language`], for the same reason: the database accepts any string under
/// `settings`, and a value this build cannot size a window with must not reach a caller as a choice. It
/// is reported as absent, which the interface and the desktop process both read as "keep the default".
pub fn get_window_layout(store: &Store) -> Result<Option<String>, MirkError> {
    Ok(store
        .get_setting(crate::layout::LAYOUT_SETTING)?
        .filter(|layout| crate::layout::WindowLayout::parse(layout).is_some()))
}

/// Stores the chosen window layout, refusing a value this build does not know.
///
/// Refusing rather than storing is what keeps a restart honest: a typo written once would otherwise be
/// read back as "no layout stored" forever, and the reader would wonder why their choice vanished. The
/// window is sized by the command wrapper in `main.rs`, not here, so this decision stays testable
/// without a display — and a refused value never reaches the store, so a stored choice survives it.
pub fn set_window_layout(store: &Store, layout: &str) -> Result<(), MirkError> {
    if crate::layout::WindowLayout::parse(layout).is_none() {
        return Err(MirkError::InvalidInput {
            field: "layout",
            value: layout.to_string(),
            expected: crate::layout::LAYOUT_KEYS.join(", "),
        });
    }
    store.set_setting(crate::layout::LAYOUT_SETTING, layout)
}

/// Opens a document with the platform launcher, at the page the reader last reached.
///
/// This is the card's own action: the reader asks for a document and the document goes to the
/// system's application for it — Evince or Papers on Linux when one of them is installed, the
/// desktop's default handler otherwise. `mirk` does not render it: the first-party reader is a
/// later task, and until it exists this is the honest way to open a PDF.
///
/// # The page
///
/// The page is the **newest recorded position**, the very same row the listing shows, so the card
/// and the document can never disagree: a card that says "page 12" opens at page 12. A document no
/// source has ever reported a position for is opened at its beginning, which is the honest answer
/// — there is no page to resume.
///
/// The viewer's own capability is the platform's business, not this function's: the launcher's
/// contract says a viewer that cannot be asked for a page still opens the document, and the
/// returned `viewer` names the program that actually ran. That is why the fallback is a degraded
/// success rather than an error, and why the `viewer` field is worth having.
///
/// The stored position is used rather than a live read of the viewer's state on purpose: the live
/// state belongs to the viewer, and the page the card shows is the stored one. A reader who sees
/// "page 12" on the card and lands on page 12 has been told the truth.
pub fn open_document(
    store: &Store,
    launcher: &dyn Launcher,
    id: i64,
) -> Result<OpenedDocumentView, MirkError> {
    let Some(record) = store.document_by_id(id)? else {
        // An id that names no document is an invalid argument, not a storage failure: the caller
        // asked for something the library does not hold, and a log that says otherwise sends
        // whoever reads it looking in the wrong place (R35).
        return Err(MirkError::InvalidInput {
            field: "id",
            value: id.to_string(),
            expected: "a document in the library".to_string(),
        });
    };
    let path = record.document.path.clone();

    // The file has to still be there. A document whose file was moved or deleted since the last
    // scan is an honest error, not a silent no-op: handing a path to a viewer that will complain
    // in a window the reader is not looking at is the failure this project refuses everywhere else.
    match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            return Err(MirkError::InvalidPath(format!(
                "the path is not a file: {}",
                display_path(&path)
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(MirkError::InvalidPath(format!(
                "the document no longer exists: {}",
                display_path(&path)
            )))
        }
        Err(error) => return Err(MirkError::Io(error)),
    }

    // The newest recorded position, the same row the listing shows. `progress_for_document` returns
    // the rows oldest first, so the last one is the newest — the same rule the listing's join
    // applies, which is what keeps the card and the document in agreement.
    let page = store
        .progress_for_document(id)?
        .last()
        .map(|progress| progress.page);

    let viewer = match launcher.launch(&path, page)? {
        LaunchOutcome::Delegated { viewer } => viewer,
        // `mirk`'s own reader is a legitimate answer rather than a failure, once it exists.
        LaunchOutcome::OpenedInternally => "mirk".to_string(),
    };

    Ok(OpenedDocumentView {
        path: display_path(&path),
        viewer,
        page,
    })
}

/// Opens the folder that contains `path` with the platform launcher.
///
/// The document's own path is deliberately not opened: the reader asked for its folder. The launcher
/// is injected so this can be tested without starting a file manager, exactly as the scanner takes
/// its identity and the adapters take their spawner.
pub fn open_containing_folder(
    launcher: &dyn Launcher,
    path: &Path,
) -> Result<OpenedFolderView, MirkError> {
    let folder = containing_folder(path)?;
    open_directory(launcher, &folder)
}

/// Opens `path`, which must itself be the folder the caller wants on screen (R24).
///
/// This is the watched-folder action, and it opens the folder **itself** rather than its parent:
/// [`open_containing_folder`] answers "where does this document live", and this answers "show me
/// this folder". Both go through [`open_directory`], so there is one place that decides what an
/// openable folder is and one place that reports a failure to open it.
pub fn open_folder(launcher: &dyn Launcher, path: &Path) -> Result<OpenedFolderView, MirkError> {
    open_directory(launcher, path)
}

/// Opens `folder` with `launcher`, refusing anything that is not a folder on disk right now.
///
/// A folder that is gone is an honest error rather than a silent no-op: the store can hold a
/// document — or a watch — whose directory was deleted or moved since it was last seen, and
/// "nothing happened" would be the least useful report of that. The launcher's own failure (no
/// `xdg-open`, say) travels back unchanged.
fn open_directory(launcher: &dyn Launcher, folder: &Path) -> Result<OpenedFolderView, MirkError> {
    match fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            return Err(MirkError::InvalidPath(format!(
                "the path is not a folder: {}",
                display_path(folder)
            )))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(MirkError::InvalidPath(format!(
                "the folder no longer exists: {}",
                display_path(folder)
            )))
        }
        Err(error) => return Err(MirkError::Io(error)),
    }

    let viewer = match launcher.launch(folder, None)? {
        LaunchOutcome::Delegated { viewer } => viewer,
        // `mirk`'s own reader is a legitimate answer rather than a failure, once it exists.
        LaunchOutcome::OpenedInternally => "mirk".to_string(),
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
fn containing_folder(path: &Path) -> Result<PathBuf, MirkError> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Ok(parent.to_path_buf()),
        _ => Err(MirkError::InvalidPath(format!(
            "the document path has no folder: {}",
            display_path(path)
        ))),
    }
}

/// The stable string a [`mirk_core::Confidence`] is persisted under.
fn confidence_id(confidence: mirk_core::Confidence) -> String {
    match confidence {
        mirk_core::Confidence::Exact => "exact",
        mirk_core::Confidence::CloseTime => "close-time",
        mirk_core::Confidence::Manual => "manual",
        mirk_core::Confidence::Unknown => "unknown",
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
    use mirk_core::{
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

        fn open_documents(&self) -> Result<Vec<OpenDocument>, MirkError> {
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
        assert_eq!(
            unread.progress, None,
            "no progress must stay absent, not become zero"
        );
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
        assert_eq!(
            progress.read_at,
            Some(old),
            "the reading time must reach the view"
        );
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

        set_document_naming(&store, id, Some("What the reader called it"), None).expect("name it");

        let entries = library_entries(&store).expect("list again");
        // `title` is the displayed value, and `userTitle` marks it as the reader's.
        assert_eq!(entries[0].title, "What the reader called it");
        assert_eq!(
            entries[0].user_title.as_deref(),
            Some("What the reader called it")
        );
    }

    // --- the reader's authors travel beside the document's own -----------------------------------

    #[test]
    fn the_library_view_displays_the_readers_authors_and_marks_them_as_theirs() {
        let store = in_memory_store();
        let mut doc = document("/library/authored.pdf", 5, 9);
        doc.author = Some("The document's own author".to_string());
        let id = store.upsert_document(&doc).expect("insert");

        // Before anyone names it, the displayed list is the document's own author as a single entry,
        // and the reader has none of their own.
        let entries = library_entries(&store).expect("list");
        assert_eq!(entries[0].authors, vec!["The document's own author"]);
        assert!(entries[0].reader_authors.is_empty());

        set_document_naming(
            &store,
            id,
            None,
            Some(vec!["What the reader called them".to_string()]),
        )
        .expect("name it");

        let entries = library_entries(&store).expect("list again");
        // `authors` is the displayed list, and `readerAuthors` marks it as the reader's.
        assert_eq!(entries[0].authors, vec!["What the reader called them"]);
        assert_eq!(
            entries[0].reader_authors,
            vec!["What the reader called them"]
        );
    }

    #[test]
    fn the_view_carries_both_the_displayed_and_the_readers_own_list() {
        // R42/S4 and S7 reaching the interface: the view carries the whole displayed list — resolved
        // by the core, in the reader's order — and the reader's own list beside it, so the card can
        // state several authors and the filter can offer them. Neither field is a first-element
        // narrowing any more: the narrowing was what the packed single author used to be.
        let store = in_memory_store();
        let mut doc = document("/library/coauthored.pdf", 5, 14);
        doc.author = Some("The document's own author".to_string());
        let id = store.upsert_document(&doc).expect("insert");

        set_document_naming(
            &store,
            id,
            None,
            Some(vec!["Carl Sagan".to_string(), "Ann Druyan".to_string()]),
        )
        .expect("save the reader's authors");

        let entries = library_entries(&store).expect("list");
        assert_eq!(
            entries[0].authors,
            vec!["Carl Sagan", "Ann Druyan"],
            "the document's own author is not displayed while the reader has a list of their own"
        );
        assert_eq!(entries[0].reader_authors, vec!["Carl Sagan", "Ann Druyan"]);

        // The whole list is what the store kept, in the reader's order, and the view is that list
        // rather than a copy with an opinion of its own.
        let stored = store.document_by_id(id).expect("read").expect("present");
        assert_eq!(stored.document.authors, entries[0].reader_authors);
        assert_eq!(stored.document.display_authors(), entries[0].authors);
    }

    #[test]
    fn clearing_the_readers_authors_restores_the_documents_own() {
        let store = in_memory_store();
        let mut doc = document("/library/authored.pdf", 5, 10);
        doc.author = Some("The document's own author".to_string());
        let id = store.upsert_document(&doc).expect("insert");

        set_document_naming(
            &store,
            id,
            None,
            Some(vec!["The reader's author".to_string()]),
        )
        .expect("name it");
        // An empty list — `[]` here, a blank field in the interface — is the legitimate "no author of
        // my own" action, not an error, and restores the document's own author. An absent value is a
        // different thing: it says nothing about the authors at all.
        set_document_naming(&store, id, None, Some(Vec::new()))
            .expect("clear the reader's authors");
        let entries = library_entries(&store).expect("list again");
        assert_eq!(entries[0].authors, vec!["The document's own author"]);
        assert!(entries[0].reader_authors.is_empty());
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

        set_document_naming(&store, id, Some("The reader's title"), None).expect("name it");
        assert_eq!(
            library_entries(&store).expect("list")[0].title,
            "The reader's title"
        );

        // `None` is the legitimate "restore" action, not an error, and it says nothing about the
        // reader's authors.
        set_document_naming(&store, id, None, None).expect("restore the document's own title");
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
            Some(vec!["What the reader called them".to_string()]),
        )
        .expect("save the title and the authors in one command");

        let saved = library_entries(&store).expect("list");
        assert_eq!(saved[0].title, "What the reader called it");
        assert_eq!(saved[0].authors, vec!["What the reader called them"]);

        set_document_naming(&store, id, None, None).expect("restore the document's own title");

        let restored = library_entries(&store).expect("list again");
        assert_eq!(restored[0].title, "The document's own title");
        assert_eq!(restored[0].user_title, None);
        assert_eq!(
            restored[0].reader_authors,
            vec!["What the reader called them"],
            "restoring the title must leave the reader's authors exactly as they were"
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
            // The authors are decided by the same call, and must not land while the title is refused:
            // the refusal happens before the write opens, so there is nothing to roll back.
            Some(vec!["An author that must not be written".to_string()]),
        ) {
            Err(MirkError::InvalidInput {
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
        assert!(entries[0].reader_authors.is_empty());
    }

    #[test]
    fn scanning_a_real_folder_populates_the_library_view() {
        let store = in_memory_store();
        let identity = mirk_platform::backends().file_identity;
        // The scanner's own committed fixtures: two real PDFs, read-only, never the maintainer's
        // library. Using them here exercises the same code path the command wraps.
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/mirk-scan/tests/fixtures");

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
        let identity = mirk_platform::backends().file_identity;
        let missing = std::env::temp_dir().join("mirk-desktop-folder-that-is-not-there-9f3c1a");

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

        fn launch(&self, path: &Path, page: Option<u32>) -> Result<LaunchOutcome, MirkError> {
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
            let path =
                std::env::temp_dir().join(format!("mirk-desktop-{label}-{}", std::process::id()));
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
        store
            .record_folder("/library/short", false)
            .expect("record");

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
        assert_eq!(
            removed, planned,
            "the removal carries out the plan it showed"
        );
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
            Err(MirkError::InvalidPath(message)) => {
                assert!(message.contains("libros"), "names the folder: {message}");
            }
            other => panic!("expected an InvalidPath error, got {other:?}"),
        }
        assert!(
            launcher
                .calls
                .lock()
                .expect("the recording lock")
                .is_empty(),
            "a folder that is gone must not be launched"
        );
    }

    #[test]
    fn a_path_with_no_folder_is_an_honest_error() {
        let launcher = RecordingLauncher::new();
        let result = open_containing_folder(&launcher, Path::new("/"));

        assert!(
            matches!(result, Err(MirkError::InvalidPath(_))),
            "got {result:?}"
        );
        assert!(launcher
            .calls
            .lock()
            .expect("the recording lock")
            .is_empty());
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

            fn launch(&self, _path: &Path, _page: Option<u32>) -> Result<LaunchOutcome, MirkError> {
                Err(MirkError::Io(std::io::Error::new(
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
            Err(MirkError::Io(error)) => assert!(error.to_string().contains("no xdg-open")),
            other => panic!("expected an io error, got {other:?}"),
        }
    }

    // --- the card opens the document (R37) ------------------------------------------------------

    /// A store holding one document whose file is really on disk, so the open action can be
    /// exercised the way the interface exercises it.
    fn stored_document(label: &str) -> (Store, i64, PathBuf, TempDir) {
        let store = in_memory_store();
        let dir = TempDir::new(label);
        let file = dir.path.join("Cosmos.pdf");
        std::fs::write(&file, b"not really a pdf").expect("write the file");
        let id = store
            .upsert_document(&document(&display_path(&file), 3, 1))
            .expect("insert");
        (store, id, file, dir)
    }

    #[test]
    fn opening_a_document_resumes_it_at_the_page_the_reader_reached() {
        let (store, id, file, _dir) = stored_document("open-document");

        // Two rows, deliberately out of page order, so "the newest" cannot be confused with "the
        // furthest": the card shows the newest one, and the document has to open there too.
        store
            .record_progress(
                id,
                &StoredProgress::new(
                    40,
                    Some(300),
                    ProgressSource::External("evince-gvfs"),
                    Confidence::Exact,
                ),
            )
            .expect("record");
        store
            .record_progress(
                id,
                &StoredProgress::new(
                    11,
                    Some(300),
                    ProgressSource::External("evince-gvfs"),
                    Confidence::Exact,
                ),
            )
            .expect("record");

        let launcher = RecordingLauncher::new();
        let opened = open_document(&store, &launcher, id).expect("open the document");

        assert_eq!(opened.path, display_path(&file));
        assert_eq!(opened.viewer, "test-file-manager");
        assert_eq!(
            opened.page,
            Some(11),
            "the page reported is the one the document was opened at"
        );

        let calls = launcher.calls.lock().expect("the recording lock");
        assert_eq!(calls.len(), 1, "exactly one launch");
        assert_eq!(calls[0].0, file, "the document itself, not its folder");
        assert_eq!(
            calls[0].1,
            Some(11),
            "the newest recorded position, the same one the card shows"
        );
    }

    #[test]
    fn a_document_nobody_has_read_opens_at_its_beginning() {
        let (store, id, _file, _dir) = stored_document("open-unread");

        let launcher = RecordingLauncher::new();
        let opened = open_document(&store, &launcher, id).expect("open the document");

        assert_eq!(
            opened.page, None,
            "a document with no recorded position has no page to resume at"
        );
        let calls = launcher.calls.lock().expect("the recording lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].1, None,
            "no page is asked for, so the viewer is free to open it at the beginning"
        );
    }

    #[test]
    fn a_document_whose_file_is_gone_is_an_honest_error_and_is_not_launched() {
        let (store, id, file, dir) = stored_document("open-gone");
        // The library keeps a document whose file was deleted after the last scan.
        std::fs::remove_dir_all(&dir.path).expect("remove the file");

        let launcher = RecordingLauncher::new();
        let result = open_document(&store, &launcher, id);

        match result {
            Err(MirkError::InvalidPath(message)) => {
                assert!(
                    message.contains("Cosmos.pdf"),
                    "names the document: {message}"
                );
            }
            other => panic!("expected an InvalidPath error, got {other:?}"),
        }
        assert!(
            launcher
                .calls
                .lock()
                .expect("the recording lock")
                .is_empty(),
            "a document that is gone must not be launched"
        );
        assert!(!file.exists());
    }

    #[test]
    fn an_id_that_names_no_document_is_an_invalid_argument_and_is_not_launched() {
        let store = in_memory_store();
        let launcher = RecordingLauncher::new();

        let result = open_document(&store, &launcher, 404);

        match result {
            Err(MirkError::InvalidInput {
                field,
                value,
                expected,
            }) => {
                assert_eq!(field, "id");
                assert_eq!(value, "404");
                assert!(
                    !expected.is_empty(),
                    "the error says what is accepted instead"
                );
            }
            other => panic!("expected an InvalidInput error, got {other:?}"),
        }
        assert!(launcher
            .calls
            .lock()
            .expect("the recording lock")
            .is_empty());
    }

    #[test]
    fn a_launcher_failure_travels_back_unchanged_when_opening_a_document() {
        struct FailingLauncher;

        impl Launcher for FailingLauncher {
            fn id(&self) -> &'static str {
                "test-failing"
            }

            fn platform(&self) -> Platform {
                Platform::current()
            }

            fn launch(&self, _path: &Path, _page: Option<u32>) -> Result<LaunchOutcome, MirkError> {
                Err(MirkError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no xdg-open",
                )))
            }
        }

        let (store, id, _file, _dir) = stored_document("open-failure");
        let result = open_document(&store, &FailingLauncher, id);

        match result {
            Err(MirkError::Io(error)) => assert!(error.to_string().contains("no xdg-open")),
            other => panic!("expected an io error, got {other:?}"),
        }
    }

    // --- R22: a subfolder of a watched folder is refused, and the refusal says why ------------

    #[test]
    fn scanning_a_subfolder_of_a_watched_folder_is_refused_by_name() {
        let store = in_memory_store();
        let identity = mirk_platform::backends().file_identity;
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
            Err(MirkError::PathCoveredByWatchedFolder { path, covering }) => {
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
        let identity = mirk_platform::backends().file_identity;
        let dir = TempDir::new("same-scan");
        let library = dir.path.join("library");
        std::fs::create_dir_all(&library).expect("create the folder");
        let watched_root = library.canonicalize().expect("canonical root");
        store.record_folder(&watched_root, true).expect("record");

        let result = scan_path(&store, identity.as_ref(), &library);
        assert!(
            matches!(result, Err(MirkError::PathCoveredByWatchedFolder { .. })),
            "re-adding the watched folder is the same refusal, not a second watch: {result:?}"
        );
        assert_eq!(store.folders().expect("list folders").len(), 1);
    }

    #[test]
    fn the_refusal_reads_as_a_sentence_naming_the_covering_folder() {
        // The interface renders `MirkError`'s own message, so the message — not only the two fields —
        // has to carry the answer: "it was blocked" without "by which one" is a half-answer.
        let error = MirkError::PathCoveredByWatchedFolder {
            path: "/library/pdfs/book".to_string(),
            covering: "/library/pdfs".to_string(),
        };
        let message = error.to_string();

        assert!(
            message.contains("/library/pdfs/book"),
            "names the choice: {message}"
        );
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
            Err(MirkError::InvalidPath(message)) => {
                assert!(message.contains("libros"), "names the folder: {message}");
            }
            other => panic!("expected an InvalidPath error, got {other:?}"),
        }
        assert!(
            launcher
                .calls
                .lock()
                .expect("the recording lock")
                .is_empty(),
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
            MirkError::InvalidInput {
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

    // --- S2/S3: the stored window layout -------------------------------------------------------

    #[test]
    fn the_window_layout_is_stored_and_read_back() {
        let store = in_memory_store();
        assert_eq!(
            get_window_layout(&store).expect("read"),
            None,
            "a fresh store has no layout, so the interface keeps the default"
        );

        set_window_layout(&store, "columns-3").expect("store three per row");
        assert_eq!(
            get_window_layout(&store).expect("read"),
            Some("columns-3".to_string())
        );

        set_window_layout(&store, "auto").expect("store full screen");
        assert_eq!(
            get_window_layout(&store).expect("read"),
            Some("auto".to_string())
        );
    }

    #[test]
    fn a_layout_this_build_does_not_know_is_refused_and_not_stored() {
        let store = in_memory_store();
        set_window_layout(&store, "columns-3").expect("store a known layout");

        match set_window_layout(&store, "columns-5") {
            Err(MirkError::InvalidInput {
                field,
                value,
                expected,
            }) => {
                assert_eq!(field, "layout");
                assert_eq!(value, "columns-5");
                assert!(
                    expected.contains("columns-2") && expected.contains("auto"),
                    "the refusal names what is accepted instead: {expected}"
                );
            }
            other => panic!("an unknown layout must be an invalid argument, got {other:?}"),
        }

        assert_eq!(
            get_window_layout(&store).expect("read"),
            Some("columns-3".to_string()),
            "a refused choice must leave the stored one in force"
        );
    }

    #[test]
    fn a_stored_layout_this_build_does_not_know_is_reported_as_no_choice() {
        // A newer build, a hand-edited database or a typo can leave anything under this key. A caller
        // must see "no choice" rather than a value it cannot size a window with.
        let store = in_memory_store();
        store
            .set_setting(crate::layout::LAYOUT_SETTING, "columns-9")
            .expect("write a value the build does not know");

        assert_eq!(get_window_layout(&store).expect("read"), None);
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
            covers_directory(Path::new("/data/mirk")),
            PathBuf::from("/data/mirk/covers")
        );
        // The page is part of the name (R32): a name without it would keep serving the old page after
        // the reader chose another one. Two pages, two files, neither one stale.
        assert_eq!(
            cover_file(Path::new("/data/mirk/covers"), 42, 0),
            PathBuf::from("/data/mirk/covers/42-p0.png")
        );
        assert_eq!(
            cover_file(Path::new("/data/mirk/covers"), 42, 7),
            PathBuf::from("/data/mirk/covers/42-p7.png")
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

    // R19: the choice is a stored fact, so a page can take the cover while the image stays, and the
    // image can come back without the reader choosing the file again — the user's report was that
    // neither direction was possible once an image existed.
    #[test]
    fn a_page_takes_over_and_the_image_comes_back_without_choosing_it_again() {
        let store = in_memory_store();
        let id = store
            .upsert_document(&document("/library/gone.pdf", 5, 9))
            .expect("insert");
        store
            .set_cover_image(id, Some("chosen.png"))
            .expect("store an image name");

        assert_eq!(
            store
                .document_by_id(id)
                .expect("read")
                .expect("row")
                .cover_choice(),
            CoverChoice::Image("chosen.png".to_string())
        );

        set_cover_page(&store, id, Some(2)).expect("choose a page");
        let record = store.document_by_id(id).expect("read").expect("row");
        assert_eq!(record.cover_choice(), CoverChoice::Page(2));
        assert_eq!(
            record.document.cover_image.as_deref(),
            Some("chosen.png"),
            "the image is kept while the page is the cover"
        );

        use_cover_image(&store, id).expect("use the stored image");
        assert_eq!(
            store
                .document_by_id(id)
                .expect("read")
                .expect("row")
                .cover_choice(),
            CoverChoice::Image("chosen.png".to_string())
        );
    }

    #[test]
    fn the_library_view_says_which_cover_source_is_in_force() {
        let store = in_memory_store();
        let id = store
            .upsert_document(&document("/library/src.pdf", 7, 7))
            .expect("insert");

        // A library written before the choice existed says nothing: the interface falls back to the
        // old rule rather than inventing a source.
        assert_eq!(library_entries(&store).expect("list")[0].cover_source, None);

        set_cover_page(&store, id, Some(2)).expect("page");
        assert_eq!(
            library_entries(&store).expect("list")[0]
                .cover_source
                .as_deref(),
            Some("page")
        );

        store.set_cover_image(id, Some("mine.png")).expect("image");
        assert_eq!(
            library_entries(&store).expect("list")[0]
                .cover_source
                .as_deref(),
            Some("image")
        );
    }

    #[test]
    fn using_an_image_that_was_never_supplied_is_refused() {
        let store = in_memory_store();
        let id = store
            .upsert_document(&document("/library/none.pdf", 8, 8))
            .expect("insert");

        assert!(matches!(
            use_cover_image(&store, id),
            Err(MirkError::InvalidInput { .. })
        ));
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
        fs::write(
            &source,
            b"\x89PNG\r\n\x1a\nnot really a full png, but the bytes say image",
        )
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
        // two valid pages render different BYTES is pinned in `mirk-scan`'s cover tests, where a two-page
        // PDF with visibly different pages can be built; the repository's fixture has identical pages.
        let directory = TempDir::new("cover-page");
        let pdf = directory.path.join("document.pdf");
        fs::write(
            &pdf,
            include_bytes!("../../mirk-scan/tests/fixtures/with-info-metadata.pdf"),
        )
        .expect("write the fixture");

        let store = in_memory_store();
        let id = store
            .upsert_document(&document(pdf.to_str().expect("utf-8 path"), 6, 6))
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
    fn a_suggested_document_shows_its_first_page() {
        // L60: a suggestion is a path, not a library row — the reader has not accepted it yet — so its
        // cover cannot come from the row-keyed cache document_cover uses. This test never opens a
        // store at all, which is the point: the preview must work for a document the library has
        // never seen.
        let directory = TempDir::new("suggestion-cover");
        let pdf = directory.path.join("suggested.pdf");
        fs::write(
            &pdf,
            include_bytes!("../../mirk-scan/tests/fixtures/with-info-metadata.pdf"),
        )
        .expect("write the fixture");

        let cover = suggestion_cover(&pdf)
            .expect("the preview is produced")
            .expect("a readable PDF has a first page");
        assert!(cover.starts_with("data:image/png;base64,"), "{cover}");
    }

    #[test]
    fn a_suggested_path_that_cannot_be_read_is_no_cover_and_not_an_error() {
        // The file is not a PDF, or is no longer there: the card keeps its placeholder. An error here
        // would put a message on screen for a fact about the file, which is what document_cover
        // already refuses to do for a library document.
        let directory = TempDir::new("suggestion-cover-none");
        let not_a_pdf = directory.path.join("notes.txt");
        fs::write(&not_a_pdf, b"this is not a PDF").expect("write the file");

        assert_eq!(
            suggestion_cover(&not_a_pdf).expect("no cover, no error"),
            None
        );
        assert_eq!(
            suggestion_cover(&directory.path.join("gone.pdf")).expect("no cover, no error"),
            None
        );
    }

    /// The three answers a detector can give, as three collaborators rather than one flag.
    struct DetectorAnswering;
    impl OpenDocumentDetector for DetectorAnswering {
        fn id(&self) -> &'static str {
            "answering"
        }
        fn platform(&self) -> Platform {
            Platform::Linux
        }
        fn open_documents(&self) -> Result<Vec<OpenDocument>, MirkError> {
            Ok(Vec::new())
        }
    }

    struct DetectorUnsupported;
    impl OpenDocumentDetector for DetectorUnsupported {
        fn id(&self) -> &'static str {
            "unsupported"
        }
        fn platform(&self) -> Platform {
            Platform::Linux
        }
        fn open_documents(&self) -> Result<Vec<OpenDocument>, MirkError> {
            Err(MirkError::NotSupported {
                capability: "open-document detection",
                platform: Platform::Linux,
            })
        }
    }

    struct DetectorFailing;
    impl OpenDocumentDetector for DetectorFailing {
        fn id(&self) -> &'static str {
            "failing"
        }
        fn platform(&self) -> Platform {
            Platform::Linux
        }
        fn open_documents(&self) -> Result<Vec<OpenDocument>, MirkError> {
            Err(std::io::Error::other("the process table could not be read").into())
        }
    }

    #[test]
    fn a_detector_that_answers_is_reported_as_installed() {
        let view = suggestion_detector(&DetectorAnswering);
        assert_eq!(view.id, "answering");
        assert_eq!(view.availability, "installed");
    }

    #[test]
    fn a_machine_without_a_detector_is_unsupported_rather_than_empty() {
        // The case the producer's own comment names: the capability is missing here, so the queue
        // cannot fill. "There is nothing to decide" would be a claim the application cannot support,
        // and the reader would have no reason to look again.
        assert_eq!(
            suggestion_detector(&DetectorUnsupported).availability,
            "unsupported"
        );
    }

    #[test]
    fn a_detector_that_failed_is_not_reported_as_working() {
        // A failure is not an empty queue either. Presenting a transient failure as good news is the
        // same defect one step further away.
        assert_eq!(
            suggestion_detector(&DetectorFailing).availability,
            "unavailable"
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
            MirkError::InvalidInput {
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
            document_cover(&store, 999, Path::new("/data/mirk")).expect("no document, no cover"),
            None
        );
    }

    // --- R4: the suggestion queue's producer and its decisions -----------------------------

    /// A source that reports exactly the documents a test hands it, so the producer's own rules can
    /// be exercised without reading this machine's real recent-files history.
    struct FixedRecent {
        documents: Vec<mirk_core::RecentDocument>,
    }

    impl FixedRecent {
        fn naming(entries: &[(&str, Option<i64>)]) -> Self {
            Self {
                documents: entries
                    .iter()
                    .map(|(path, time)| mirk_core::RecentDocument {
                        path: PathBuf::from(*path),
                        last_used_unix: *time,
                    })
                    .collect(),
            }
        }
    }

    impl RecentDocumentsSource for FixedRecent {
        fn id(&self) -> &'static str {
            "test-recent"
        }

        fn platform(&self) -> Platform {
            Platform::current()
        }

        fn recent(&self, limit: usize) -> Result<Vec<mirk_core::RecentDocument>, MirkError> {
            let mut documents = self.documents.clone();
            documents.truncate(limit);
            Ok(documents)
        }
    }

    /// A source that cannot answer, so the producer's failure path can be observed.
    struct BrokenRecent;

    impl RecentDocumentsSource for BrokenRecent {
        fn id(&self) -> &'static str {
            "broken-recent"
        }

        fn platform(&self) -> Platform {
            Platform::current()
        }

        fn recent(&self, _limit: usize) -> Result<Vec<mirk_core::RecentDocument>, MirkError> {
            Err(MirkError::NotSupported {
                capability: "recent-document ingestion",
                platform: Platform::current(),
            })
        }
    }

    #[test]
    fn a_document_inside_a_watched_folder_is_never_suggested() {
        // The watched folder is the source of truth. A PDF under one that is not in the library is a
        // scan that has not counted it yet, not a document the reader should be asked to review.
        let store = in_memory_store();
        store
            .record_folder("/library/pdfs", true)
            .expect("watch the folder");

        let detector = FixedDetector::seeing(&["/library/pdfs/inside.pdf", "/elsewhere/open.pdf"]);
        let recorded = observe_open_documents(&store, &detector).expect("observe");

        assert_eq!(
            recorded, 1,
            "only the document outside the watch is a suggestion"
        );
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
        assert_eq!(
            observe_open_documents(&store, &detector).expect("observe"),
            0
        );
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

        assert_eq!(
            observe_open_documents(&store, &detector).expect("observe"),
            1
        );
        let suggestions = list_suggestions(&store).expect("list");
        assert_eq!(suggestions.len(), 1);
        assert_eq!(
            suggestions[0].seen_count, 1,
            "one observation, one sighting"
        );

        observe_open_documents(&store, &detector).expect("observe again");
        let suggestions = list_suggestions(&store).expect("list");
        assert_eq!(suggestions.len(), 1, "still one row");
        assert_eq!(
            suggestions[0].seen_count, 2,
            "the next observation is the second"
        );
    }

    #[test]
    fn the_history_becomes_suggestions_with_the_readers_own_time() {
        // S10 of `suggestions-screen.md`: every PDF the history names that is neither in the library
        // nor inside a watched folder is a suggestion, and it carries the moment the history
        // recorded — not the moment mirk noticed the file.
        let store = in_memory_store();
        store
            .upsert_document(&document("/library/known.pdf", 1, 1))
            .expect("insert a document");
        store
            .record_folder("/library/pdfs", true)
            .expect("watch the folder");

        let source = FixedRecent::naming(&[
            ("/library/known.pdf", Some(1_700_000_000)),
            ("/library/pdfs/inside.pdf", Some(1_700_000_000)),
            ("/elsewhere/reading.pdf", Some(1_600_000_000)),
            // S3 of the inherited spec: a document with no recorded moment is still a document.
            ("/elsewhere/undated.pdf", None),
        ]);

        let recorded = observe_recent_documents(&store, &source).expect("observe");
        assert_eq!(
            recorded, 2,
            "only the two outside the library and the watch"
        );

        let mut suggestions = list_suggestions(&store).expect("list");
        suggestions.sort_by(|left, right| left.path.cmp(&right.path));
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].path, "/elsewhere/reading.pdf");
        assert_eq!(suggestions[0].history_at, Some(1_600_000_000));
        assert_ne!(
            suggestions[0].first_seen_at, 1_600_000_000,
            "when the app noticed is not when the reader opened it"
        );
        assert_eq!(suggestions[1].path, "/elsewhere/undated.pdf");
        assert_eq!(
            suggestions[1].history_at, None,
            "no moment is null, never 1970"
        );
    }

    #[test]
    fn the_window_keeps_the_newest_eligible_entries_not_the_newest_entries() {
        // S10: every eligible PDF is a suggestion. The bound therefore applies to what the pass
        // RECORDS, in the newest-first order the source guarantees. Applying it to what the source
        // is asked for would hide the eligible documents behind the newest entries, permanently,
        // whenever those newest entries are already known. A history needs no thousand entries to
        // show that: the same rule with a window of one is the same rule.
        let store = in_memory_store();
        store
            .upsert_document(&document("/library/newest.pdf", 1, 1))
            .expect("insert");
        store
            .upsert_document(&document("/library/second.pdf", 2, 2))
            .expect("insert");

        let source = FixedRecent::naming(&[
            // The newest entries are all already in the library, which is the case that used to
            // consume the whole window.
            ("/library/newest.pdf", Some(1_700_000_002)),
            ("/library/second.pdf", Some(1_700_000_001)),
            ("/elsewhere/eligible.pdf", Some(1_600_000_000)),
            ("/elsewhere/older.pdf", Some(1_500_000_000)),
        ]);

        let recorded = record_recent_documents(&store, &source, 1).expect("observe");
        assert_eq!(recorded, 1, "the bound counts what was recorded");

        let suggestions = list_suggestions(&store).expect("list");
        let paths: Vec<&str> = suggestions
            .iter()
            .map(|suggestion| suggestion.path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec!["/elsewhere/eligible.pdf"],
            "the newest eligible entry is kept, not the newest entry"
        );
    }

    #[test]
    fn a_decided_history_document_is_not_offered_again() {
        // The producer decides nothing: a document the reader dismissed stays dismissed even though
        // the history still names it and the file is still there.
        let store = in_memory_store();
        let source = FixedRecent::naming(&[("/elsewhere/dismissed.pdf", Some(1_600_000_000))]);

        assert_eq!(
            observe_recent_documents(&store, &source).expect("observe"),
            1
        );
        let id = list_suggestions(&store).expect("list")[0].id;
        discard_suggestion(&store, id).expect("discard");

        assert_eq!(
            observe_recent_documents(&store, &source).expect("observe again"),
            1,
            "the path is still recorded; the decision is what stops it being offered"
        );
        assert!(
            list_suggestions(&store).expect("list").is_empty(),
            "a dismissed document must not come back"
        );
        let suggestion = store.suggestion(id).expect("read").expect("present");
        assert_eq!(suggestion.decision, Some(SuggestionDecision::Discarded));
        assert_eq!(
            suggestion.history_at,
            Some(1_600_000_000),
            "a decided row is never touched"
        );
    }

    #[test]
    fn a_history_that_cannot_be_read_is_reported_rather_than_shown_as_an_empty_queue() {
        // "No backend on this platform" must not look like "nothing is waiting": the producer
        // propagates the capability error instead of recording zero suggestions.
        let store = in_memory_store();
        let error = observe_recent_documents(&store, &BrokenRecent).expect_err("no source");
        assert!(
            matches!(error, MirkError::NotSupported { .. }),
            "got {error:?}"
        );
        assert!(list_suggestions(&store).expect("list").is_empty());
    }

    #[test]
    fn accepting_a_suggestion_adds_the_document_then_records_the_decision() {
        // The whole feature: the reader's accept is the only thing that turns a suggestion into
        // library data, and it does so by persisting the document and only then recording the
        // decision. The repository's committed fixture is scanned read-only.
        let store = in_memory_store();
        let identity = mirk_platform::backends().file_identity;
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/mirk-scan/tests/fixtures/with-info-metadata.pdf");
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
        let identity = mirk_platform::backends().file_identity;
        let missing = PathBuf::from("/library/gone-before-accept-7f31.pdf");
        let id = store.record_suggestion(&missing).expect("record");

        let error = accept_suggestion(&store, identity.as_ref(), id).expect_err("the file is gone");
        assert!(matches!(error, MirkError::InvalidPath(_)), "got {error:?}");

        let suggestion = store.suggestion(id).expect("read").expect("present");
        assert!(
            suggestion.decision.is_none(),
            "no decision may exist without its document"
        );
        assert!(
            list_suggestions(&store)
                .expect("list")
                .iter()
                .any(|s| s.id == id),
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
            matches!(error, MirkError::SuggestionAlreadyDecided { .. }),
            "got {error:?}"
        );
    }
}
