//! Value types shared by every layer of `oct`.

use std::fmt;
use std::path::PathBuf;

/// The operating system a build targets.
///
/// [`Platform::Other`] exists so that unsupported systems are represented honestly rather than
/// being silently classified as one of the three supported ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Platform {
    Linux,
    Windows,
    MacOs,
    Other,
}

impl Platform {
    /// The platform this binary was compiled for.
    pub const fn current() -> Self {
        #[cfg(target_os = "linux")]
        {
            Platform::Linux
        }
        #[cfg(target_os = "windows")]
        {
            Platform::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Platform::MacOs
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            Platform::Other
        }
    }

    /// A stable machine-readable identifier, suitable for storage and for adapter metadata.
    pub const fn id(self) -> &'static str {
        match self {
            Platform::Linux => "linux",
            Platform::Windows => "windows",
            Platform::MacOs => "macos",
            Platform::Other => "other",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Platform::Linux => "Linux",
            Platform::Windows => "Windows",
            Platform::MacOs => "macOS",
            Platform::Other => "an unsupported platform",
        };
        f.write_str(label)
    }
}

/// A portable identity for a file on disk.
///
/// This exists because the obvious identity on POSIX — `(device, inode)` — simply does not exist
/// on Windows, which reports a `(volume serial number, file index)` pair instead. Neither is
/// available on every filesystem (network shares and FAT/exFAT often report useless values), which
/// is why [`FileId::ContentHash`] is part of the type rather than a fallback hidden behind it.
///
/// Identity is what lets `oct` recognise that a document was moved or renamed instead of treating
/// it as a brand new PDF, so the distinction between these variants is a correctness concern.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FileId {
    /// POSIX: `st_dev` plus `st_ino`.
    Posix { dev: u64, ino: u64 },
    /// Windows: `dwVolumeSerialNumber` plus `nFileIndexHigh`/`Low`.
    Windows {
        volume_serial: u64,
        file_index: u64,
    },
    /// Fallback for filesystems with no usable identity: a hash of the file's contents.
    ContentHash(String),
}

/// What the reader asked a document's cover to be (R32).
///
/// This is the **choice**, not the image: the covers directory is the application's business, and
/// [`Document`] knows nothing about it. The name held by [`CoverChoice::Image`] is the file's name
/// inside that directory, never a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverChoice {
    /// An image the reader supplied, copied into the covers directory under this file name.
    Image(String),
    /// A page of the document, zero-based exactly like every other page in this workspace.
    Page(u32),
}

/// A PDF known to the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    /// Absolute path on disk.
    ///
    /// **Treat paths as opaque.** They legitimately contain spaces, non-ASCII characters, and
    /// macOS-normalised (NFD) Unicode, so no layer may split, trim or re-encode this value.
    pub path: PathBuf,
    /// Portable identity, when it could be determined.
    pub identity: Option<FileId>,
    /// Total page count, once counted. Counting requires a real PDF parser; a naive byte scan
    /// undercounts documents that store page objects in compressed object streams.
    pub pages: Option<u32>,
    /// Document title from the PDF metadata dictionary, when present.
    ///
    /// This is the document's **own** title, exactly as the scanner found it. It is never written by
    /// the reader: a typed title is a separate fact ([`Document::user_title`], R31), so "restore the
    /// document's own title" is a decision the interface can take rather than an act of data loss.
    pub title: Option<String>,
    /// Document author from the PDF metadata dictionary, when present.
    ///
    /// This is the document's **own** author, exactly as the scanner found it — **one opaque string**.
    /// It is never written by the reader, never split, and never rewritten: the readers' own authors
    /// are a separate fact ([`Document::authors`], R42). `"Sagan, Carl; Druyan, Ann"` is what one
    /// document's metadata happened to hold, and splitting it on commas, semicolons or " and " would
    /// invent authors nobody wrote (R42/S6), so nobody does.
    pub author: Option<String>,
    /// The title the reader typed, or [`None`] when they have not set one (R31).
    ///
    /// **`None` means only "the reader has not set a title"** — it is not a blank title and not an
    /// instruction. A blank title is refused before it reaches here. Keeping the reader's title in
    /// its own field, instead of overwriting [`Document::title`], is what lets the interface say
    /// whose title it is showing and what makes restoring the document's own title possible at all.
    pub user_title: Option<String>,
    /// The authors the reader chose, **in the reader's order**, or empty when they have said nothing
    /// of their own (R42/S3, R42/S5).
    ///
    /// **An empty list means "no author of my own"**, which is legitimate because many documents have
    /// none, and it is what makes the fallback to [`Document::author`] a decision rather than data
    /// loss. Keeping the reader's authors in their own field, instead of overwriting `author`, is also
    /// what keeps the document's own author available to be shown again. This is the deliberate
    /// difference from [`Document::user_title`], whose field has no legitimate blank.
    ///
    /// A stored list is already normalised by the write boundary — trimmed, blank-free and
    /// deduplicated case-insensitively, first spelling wins — so `"Carl Sagan"` and `"carl sagan"`
    /// are one person here too. Order is the reader's and is preserved on read: "Carl Sagan, Ann
    /// Druyan" and "Ann Druyan, Carl Sagan" are different lists and both are legitimate.
    pub authors: Vec<String>,
    /// The page the reader chose for the cover, zero-based, or [`None`] for the first page (R32).
    ///
    /// `None` is the **default**, not "unknown": the first page is what a document without a choice
    /// shows. Storing `0` and storing `None` display the same cover, and the distinction exists so
    /// that "restore the default" is a real action rather than a value the reader has to guess.
    pub cover_page: Option<u32>,
    /// The file name of an image the reader supplied, inside the covers directory, or [`None`] (R32).
    ///
    /// Only the **name** is kept, never a path: the application copies the reader's file into its own
    /// covers directory, so the image survives a rescan, a move of the original, or the original
    /// being deleted. A path outside that directory would be a reference to somebody else's data that
    /// this application does not own.
    pub cover_image: Option<String>,
}

/// A trimmed value, or [`None`] when it held only whitespace.
fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|trimmed| !trimmed.is_empty())
}

/// The file's own name without its extension, the title of last resort.
///
/// Everything after the last path separator, minus the extension. A leading dot is kept, so a name
/// like `.hidden` is a name rather than an empty title with an extension. `Path::file_name` is used
/// rather than scanning the rendered string for `/`, so a Windows path spelled with `\` behaves the
/// same as a Unix one — the interface's old JavaScript copy of this rule got that wrong.
fn title_from_path(path: &std::path::Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    match name.rfind('.') {
        Some(extension) if extension > 0 => name[..extension].to_owned(),
        _ => name,
    }
}

impl Document {
    /// A document identified only by its path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            identity: None,
            pages: None,
            title: None,
            author: None,
            user_title: None,
            authors: Vec::new(),
            cover_page: None,
            cover_image: None,
        }
    }

    /// The title to show a person (R31).
    ///
    /// The reader's title wins when they have set one; otherwise it is the document's own title —
    /// the embedded one, or the file's own name when the PDF carries no title. **This is the one
    /// place the rule lives**, so the interface never has to decide whose title it is rendering:
    /// the value it receives is already the displayed one, and the separate
    /// [`Document::user_title`] field tells it whether that title is the reader's.
    ///
    /// A blank value counts as no value in either field: PDF metadata can hold an empty string, and
    /// a stored title is never blank because [`crate::OctError::InvalidInput`] refuses one. The
    /// file's name is the last resort, never the path — a path where a title belongs reads as a
    /// mistake and tells the reader nothing they cannot already see.
    pub fn display_title(&self) -> String {
        if let Some(user) = non_blank(self.user_title.as_deref()) {
            return user.to_owned();
        }
        if let Some(own) = non_blank(self.title.as_deref()) {
            return own.to_owned();
        }
        title_from_path(&self.path)
    }

    /// The authors to show a person, in the order they should be read (R42/S4).
    ///
    /// The reader's list wins when it has any entry; otherwise the document's **embedded** author —
    /// the whole string, as one entry, never split (R42/S6) — and otherwise none at all. **This is the
    /// one place that precedence lives**, exactly like [`Document::display_title`]: the card, the
    /// author filter and the free-text search all read what this returns, so no screen decides whose
    /// author it is rendering and two screens cannot disagree about who wrote a document.
    ///
    /// A blank entry counts as no entry, the same way a blank title counts as no title: a stored list
    /// is already blank-free, but PDF metadata and hand-edited databases can hold an empty string, and
    /// `NULL`/empty already mean "not set" — the two must not be made to mean two different things.
    /// Unlike the title there is no last-resort fallback to the file's name: an author is optional, and
    /// inventing one would be the lie this rule exists to make unnecessary.
    pub fn display_authors(&self) -> Vec<String> {
        let reader: Vec<String> = self
            .authors
            .iter()
            .filter_map(|name| non_blank(Some(name.as_str())).map(str::to_owned))
            .collect();
        if !reader.is_empty() {
            return reader;
        }
        non_blank(self.author.as_deref())
            .map(|own| vec![own.to_owned()])
            .unwrap_or_default()
    }

    /// The first of [`Document::display_authors`], for the callers that can still express only one
    /// author.
    ///
    /// The store and the domain model work in a list (R42); the one author by which a single card
    /// line and the current IPC view are worded is simply the first of that list, and it comes from
    /// the same rule rather than from a second copy of it. It is deliberately **not** a second
    /// precedence rule: with no displayed author at all it answers [`None`].
    pub fn display_author(&self) -> Option<String> {
        self.display_authors().into_iter().next()
    }

    /// The document's percent-encoded `file://` URI.
    ///
    /// This is the key format used by GVFS metadata and by several viewer state stores, so the
    /// encoding has to be exact. See [`crate::uri`].
    pub fn uri(&self) -> String {
        crate::uri::file_uri_from_path(&self.path)
    }

    /// The cover to show (R32).
    ///
    /// **This is the one place the rule lives**, exactly like [`Document::display_title`]: the
    /// reader's own image wins when there is one, otherwise the page they chose, otherwise the first
    /// page. A caller renders what this returns rather than deciding the precedence again, so the
    /// card, the cached file's name and any future screen cannot disagree about which cover a
    /// document has.
    ///
    /// A blank image name counts as no image, the same way a blank title counts as no title: PDF
    /// metadata and hand-edited databases can hold an empty string, and `NULL` already means "not
    /// set", so the two must not be made to mean the same thing in two different ways.
    pub fn cover_choice(&self) -> CoverChoice {
        if let Some(image) = non_blank(self.cover_image.as_deref()) {
            return CoverChoice::Image(image.to_owned());
        }
        CoverChoice::Page(self.cover_page.unwrap_or(0))
    }
}

/// A document reported by an OS-level recent-documents list (platform axis A).
///
/// These lists are curated by the desktop itself, which makes them a far better signal than
/// watching process file handles: they record documents a human actually opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentDocument {
    /// Absolute path, when the source could resolve one.
    pub path: PathBuf,
    /// Last time the source observed this document being used, if it records one.
    pub last_used_unix: Option<i64>,
}

/// A document currently open in some application (platform axis B).
///
/// Detection on this axis is heuristic on every platform `oct` targets. Thumbnail indexers,
/// backups and antivirus software all open PDFs without a human reading them, so results must be
/// treated as *suggestions* to be reviewed rather than as facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenDocument {
    /// Absolute path of the open file.
    pub path: PathBuf,
    /// Process id of the application holding it open, when known.
    pub pid: Option<u32>,
    /// Executable name of that application, when known.
    pub viewer: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R31: one function decides whose title is shown, and it is the only place that decides.
    #[test]
    fn the_readers_title_wins_over_the_documents_own() {
        let mut document = Document::new("/library/report.pdf");
        document.title = Some("Embedded title".to_string());
        document.user_title = Some("The reader's name".to_string());

        assert_eq!(document.display_title(), "The reader's name");
    }

    #[test]
    fn without_a_readers_title_the_documents_own_title_is_shown() {
        let mut document = Document::new("/library/report.pdf");
        document.title = Some("Embedded title".to_string());

        assert_eq!(document.display_title(), "Embedded title");
    }

    #[test]
    fn a_document_with_no_title_at_all_falls_back_to_the_files_own_name() {
        // The rule L18 shipped, now in the one place instead of the interface: never the path.
        let document = Document::new("/home/u/libros/Cosmos - Carl Sagan.pdf");

        let title = document.display_title();
        assert_eq!(title, "Cosmos - Carl Sagan");
        assert!(!title.contains('/'), "a path is not a title: {title}");
    }

    #[test]
    fn a_blank_embedded_title_is_treated_as_no_title() {
        let mut document = Document::new("/u/report.pdf");
        document.title = Some("   ".to_string());

        assert_eq!(document.display_title(), "report");
    }

    /// R32: one function decides which cover is shown, and it is the only place that decides.
    #[test]
    fn the_readers_image_wins_over_the_page_they_chose() {
        let mut document = Document::new("/library/report.pdf");
        document.cover_page = Some(7);
        document.cover_image = Some("9-cover.png".to_string());

        assert_eq!(
            document.cover_choice(),
            CoverChoice::Image("9-cover.png".to_string())
        );
    }

    #[test]
    fn without_an_image_the_page_the_reader_chose_is_shown() {
        let mut document = Document::new("/library/report.pdf");
        document.cover_page = Some(7);

        assert_eq!(document.cover_choice(), CoverChoice::Page(7));
    }

    #[test]
    fn a_document_with_no_choice_shows_the_first_page() {
        // The first page is the default, and this is the one place that default lives.
        assert_eq!(Document::new("/library/report.pdf").cover_choice(), CoverChoice::Page(0));
    }

    #[test]
    fn a_blank_image_name_is_treated_as_no_image() {
        let mut document = Document::new("/library/report.pdf");
        document.cover_page = Some(4);
        document.cover_image = Some("   ".to_string());

        assert_eq!(document.cover_choice(), CoverChoice::Page(4));
    }

    /// R42/S4: one function decides which authors are shown, and it is the only place that decides.
    #[test]
    fn the_readers_authors_win_over_the_documents_own() {
        let mut document = Document::new("/library/report.pdf");
        document.author = Some("Embedded author".to_string());
        document.authors = vec!["Carl Sagan".to_string(), "Ann Druyan".to_string()];

        assert_eq!(document.display_authors(), vec!["Carl Sagan", "Ann Druyan"]);
        assert_eq!(document.display_author().as_deref(), Some("Carl Sagan"));
    }

    /// R42/S3: the reader's order is the one that is shown.
    #[test]
    fn the_readers_order_is_the_order_displayed() {
        let mut document = Document::new("/library/report.pdf");
        document.authors = vec!["Ann Druyan".to_string(), "Carl Sagan".to_string()];

        assert_eq!(document.display_authors(), vec!["Ann Druyan", "Carl Sagan"]);
    }

    /// R42/S6: the embedded string is ONE author, never a list the metadata did not mean.
    #[test]
    fn an_embedded_author_string_is_never_split_on_commas_semicolons_or_and() {
        for embedded in [
            "Sagan, Carl; Druyan, Ann",
            "Carl Sagan and Ann Druyan",
            "Sagan, Carl",
        ] {
            let mut document = Document::new("/library/report.pdf");
            document.author = Some(embedded.to_string());

            assert_eq!(
                document.display_authors(),
                vec![embedded],
                "the embedded string is one entry, exactly as the file holds it: {embedded}"
            );
        }
    }

    /// R42/S5: an empty reader's list is "no author of my own", which falls back to the embedded one.
    #[test]
    fn an_empty_readers_list_falls_back_to_the_embedded_author_as_one_entry() {
        let mut document = Document::new("/library/report.pdf");
        document.author = Some("Sagan, Carl; Druyan, Ann".to_string());
        document.authors = Vec::new();

        assert_eq!(
            document.display_authors(),
            vec!["Sagan, Carl; Druyan, Ann"],
            "one entry, not two"
        );
    }

    #[test]
    fn without_a_readers_author_the_documents_own_is_shown() {
        let mut document = Document::new("/library/report.pdf");
        document.author = Some("Embedded author".to_string());

        assert_eq!(document.display_author().as_deref(), Some("Embedded author"));
        assert_eq!(document.display_authors(), vec!["Embedded author"]);
    }

    #[test]
    fn a_document_with_no_author_at_all_shows_none() {
        // An author is optional, and there is no file-name fallback: inventing one would be a lie.
        assert_eq!(Document::new("/library/report.pdf").display_author(), None);
        assert!(Document::new("/library/report.pdf").display_authors().is_empty());
    }

    #[test]
    fn a_blank_embedded_author_is_treated_as_no_author() {
        let mut document = Document::new("/u/report.pdf");
        document.author = Some("   ".to_string());

        assert_eq!(document.display_author(), None);
        assert!(document.display_authors().is_empty());
    }

    #[test]
    fn a_blank_readers_author_falls_back_to_the_documents_own() {
        let mut document = Document::new("/u/report.pdf");
        document.author = Some("Embedded author".to_string());
        // A stored list is blank-free, so this is a hand-edited database rather than a write of ours;
        // it must still be treated as no entry rather than as an author named "  ".
        document.authors = vec!["  ".to_string()];

        assert_eq!(document.display_author().as_deref(), Some("Embedded author"));
        assert_eq!(document.display_authors(), vec!["Embedded author"]);
    }

    #[test]
    fn the_file_name_fallback_keeps_a_leading_dot_and_a_middle_dot() {
        assert_eq!(Document::new("/u/.hidden").display_title(), ".hidden");
        assert_eq!(Document::new("/u/report.v2.pdf").display_title(), "report.v2");
        assert_eq!(Document::new("/u/no-extension").display_title(), "no-extension");
        // The separator comparison is a path operation, not a character search: a dot in a
        // directory's name must not be mistaken for the file's extension.
        assert_eq!(Document::new("/u/libros.v2/report.pdf").display_title(), "report");
    }
}
