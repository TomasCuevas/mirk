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
    pub author: Option<String>,
    /// The title the reader typed, or [`None`] when they have not set one (R31).
    ///
    /// **`None` means only "the reader has not set a title"** — it is not a blank title and not an
    /// instruction. A blank title is refused before it reaches here. Keeping the reader's title in
    /// its own field, instead of overwriting [`Document::title`], is what lets the interface say
    /// whose title it is showing and what makes restoring the document's own title possible at all.
    pub user_title: Option<String>,
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

    /// The document's percent-encoded `file://` URI.
    ///
    /// This is the key format used by GVFS metadata and by several viewer state stores, so the
    /// encoding has to be exact. See [`crate::uri`].
    pub fn uri(&self) -> String {
        crate::uri::file_uri_from_path(&self.path)
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
