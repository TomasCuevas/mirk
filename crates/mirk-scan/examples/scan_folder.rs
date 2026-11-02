//! Dev harness: scan a folder into a throwaway database.
//!
//! ```text
//! cargo run -p mirk-scan --example scan_folder -- <folder> <db-path>
//! ```
//!
//! It prints how many documents were discovered, counted and failed, with a reason for each
//! failure, so a real library can be scanned into a database nobody depends on (`/tmp/...`).
//!
//! The identity provider is a **harness-only** implementation: on Unix it uses the same
//! `(device, inode)` pair as production, and falls back to a 64-bit FNV-1a of the file's contents
//! on filesystems with no usable native identity. Production code uses `mirk-platform`'s provider;
//! `mirk-scan` itself never depends on it — identity arrives by injection.

use std::path::Path;
use std::process::ExitCode;

use mirk_core::{FileId, FileIdentity, MirkError};
use mirk_scan::Scanner;
use mirk_store::Store;

/// The harness's portable-identity provider. See the module docs for why it is not `mirk-platform`.
struct HarnessIdentity;

impl FileIdentity for HarnessIdentity {
    fn id(&self) -> &'static str {
        "scan-folder-harness"
    }

    fn identity(&self, path: &Path) -> Result<FileId, MirkError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::metadata(path)?;
            if metadata.dev() != 0 && metadata.ino() != 0 {
                return Ok(FileId::Posix {
                    dev: metadata.dev(),
                    ino: metadata.ino(),
                });
            }
        }
        Ok(FileId::ContentHash(format!(
            "fnv1a64:{:016x}",
            fnv1a64(path)?
        )))
    }
}

fn fnv1a64(path: &Path) -> Result<u64, MirkError> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut buffer = [0u8; 64 * 1024];
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        for byte in &buffer[..read] {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    Ok(hash)
}

fn main() -> ExitCode {
    let mut arguments = std::env::args_os();
    let _program = arguments.next();
    let (Some(folder), Some(database)) = (arguments.next(), arguments.next()) else {
        eprintln!("usage: scan_folder <folder> <db-path>");
        return ExitCode::from(2);
    };

    let store = match Store::open(&database) {
        Ok(store) => store,
        Err(error) => {
            eprintln!(
                "cannot open the database {}: {error}",
                Path::new(&database).display()
            );
            return ExitCode::FAILURE;
        }
    };
    let identity = HarnessIdentity;
    let scanner = Scanner::new(&store, &identity);

    let report = match scanner.scan(&folder) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("cannot scan {}: {error}", Path::new(&folder).display());
            return ExitCode::FAILURE;
        }
    };

    println!("watched folder: {}", report.root.display());
    println!("discovered: {}", report.discovered_count());
    println!("counted:    {}", report.counted_count());
    println!("failed:     {}", report.failure_count());
    for failure in &report.failures {
        println!("  failed: {} — {}", failure.path.display(), failure.reason);
    }

    ExitCode::SUCCESS
}
