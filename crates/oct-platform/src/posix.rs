//! Portable file identity for POSIX systems.
//!
//! `oct` recognises a moved or renamed document by its identity, so the value returned here must be
//! stable across renames and unique per file. On POSIX the native answer is the `(device, inode)`
//! pair from [`std::os::unix::fs::MetadataExt`], which follows the file rather than the path and
//! therefore survives moves and hard links.
//!
//! Some filesystems cannot provide one: network shares and FUSE mounts commonly report an inode of
//! zero, and FAT/exFAT can report values that are not unique. Trusting those values would silently
//! merge unrelated documents into a single reading history, which is exactly the failure this
//! capability exists to prevent. When the native pair is unusable this module falls back to a
//! streamed SHA-256 content hash instead.

use std::fmt::Write as _;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use oct_core::{FileId, FileIdentity, OctError};
use sha2::{Digest, Sha256};

/// Read size for the streaming content hash.
///
/// Documents in this project reach hundreds of megabytes, so the file is never read into memory.
const HASH_CHUNK_SIZE: usize = 64 * 1024;

/// Portable file identity on POSIX, from `st_dev` and `st_ino`.
///
/// The reported identifier is deliberately the value the Linux backend has always advertised: it is
/// persisted in the database, so it must not change between releases. macOS gets this
/// implementation for free once its backend is wired up.
pub struct PosixFileIdentity;

impl FileIdentity for PosixFileIdentity {
    fn id(&self) -> &'static str {
        "linux-posix-identity"
    }

    fn identity(&self, path: &Path) -> Result<FileId, OctError> {
        let metadata = path.metadata()?;
        let dev = metadata.dev();
        let ino = metadata.ino();

        if native_id_is_usable(dev, ino) {
            Ok(FileId::Posix { dev, ino })
        } else {
            content_hash(path).map(FileId::ContentHash)
        }
    }
}

/// Whether a `(device, inode)` pair is stable enough to identify a file.
///
/// Zero is the sentinel the kernel and common network/FUSE filesystems use when they cannot supply a
/// real value, so a pair containing a zero is treated as unusable rather than trusted. This decision
/// is factored out to be testable on its own, without needing a real exotic mount.
pub fn native_id_is_usable(dev: u64, ino: u64) -> bool {
    dev != 0 && ino != 0
}

/// Streams `path` through SHA-256 and returns the lowercase hex digest.
fn content_hash(path: &Path) -> Result<String, OctError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; HASH_CHUNK_SIZE];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest.as_slice() {
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A unique temporary directory that removes itself on drop, including on panic.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "oct-identity-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
            let path = self.path.join(name);
            fs::write(&path, contents).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn identity_is_stable_across_repeated_calls() {
        let dir = TempDir::new("stable");
        let path = dir.file("document.pdf", b"%PDF-1.7 stable");

        let first = PosixFileIdentity.identity(&path).unwrap();
        let second = PosixFileIdentity.identity(&path).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn different_files_have_different_identities() {
        let dir = TempDir::new("distinct");
        let alpha = dir.file("alpha.pdf", b"%PDF-1.7 alpha");
        let beta = dir.file("beta.pdf", b"%PDF-1.7 beta");

        assert_ne!(
            PosixFileIdentity.identity(&alpha).unwrap(),
            PosixFileIdentity.identity(&beta).unwrap()
        );
    }

    #[test]
    fn hard_links_to_the_same_inode_share_an_identity() {
        let dir = TempDir::new("hardlink");
        let original = dir.file("original.pdf", b"%PDF-1.7 hard link");
        let linked = dir.path.join("linked.pdf");
        fs::hard_link(&original, &linked).unwrap();

        let first = PosixFileIdentity.identity(&original).unwrap();
        let second = PosixFileIdentity.identity(&linked).unwrap();

        // This is the case that proves identity follows the file, not the path.
        assert_eq!(first, second);
        assert!(matches!(first, FileId::Posix { .. }));
    }

    #[test]
    fn a_missing_file_returns_an_io_error_without_panicking() {
        let dir = TempDir::new("missing");
        let missing = dir.path.join("absent.pdf");

        let result = PosixFileIdentity.identity(&missing);
        assert!(matches!(result, Err(OctError::Io(_))));
    }

    #[test]
    fn the_fallback_decision_rejects_zero_native_ids() {
        assert!(native_id_is_usable(1, 42));
        assert!(!native_id_is_usable(0, 42));
        assert!(!native_id_is_usable(1, 0));
        assert!(!native_id_is_usable(0, 0));
    }

    #[test]
    fn the_fallback_hash_is_the_sha256_of_the_contents() {
        let dir = TempDir::new("hash");
        let path = dir.file("document.pdf", b"abc");

        assert_eq!(
            content_hash(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn file_ids_are_comparable_and_hashable() {
        let dir = TempDir::new("eq");
        let path = dir.file("document.pdf", b"%PDF-1.7 comparable");

        let id = PosixFileIdentity.identity(&path).unwrap();
        let clone = id.clone();
        assert_eq!(id, clone);
        assert_ne!(id, FileId::ContentHash("unrelated".to_string()));

        let mut set = HashSet::new();
        set.insert(id);
        assert!(set.contains(&clone));
    }
}
