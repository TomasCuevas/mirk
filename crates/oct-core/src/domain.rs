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
    pub title: Option<String>,
    /// Document author from the PDF metadata dictionary, when present.
    pub author: Option<String>,
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
        }
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
