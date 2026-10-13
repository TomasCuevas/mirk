//! Dev harness: reads the recorded reading position of every PDF under a directory.
//!
//! Exists for the same reason `oct-scan`'s `scan_folder` example does — so the maintainer's real
//! library can be used as evidence instead of a fixture pretending to be one. It prints one line per
//! document that has a recorded position and writes nothing anywhere.
//!
//! Usage: `cargo run -q -p oct-platform --example read_progress -- <directory>`

use std::path::{Path, PathBuf};

use oct_core::Document;
use oct_platform::progress_providers;

fn main() {
    let root = match std::env::args().nth(1) {
        Some(root) => root,
        None => {
            eprintln!("usage: read_progress <directory>");
            std::process::exit(2);
        }
    };

    let providers = progress_providers();
    let Some(provider) = providers.first() else {
        eprintln!("# no progress provider is registered on this platform");
        std::process::exit(1);
    };
    println!(
        "# provider {} platform={:?} capability={:?} detect={:?}",
        provider.id(),
        provider.platform(),
        provider.capability(),
        provider.detect()
    );

    let mut documents = Vec::new();
    walk(Path::new(&root), &mut documents);
    documents.sort();

    let mut with_record = 0;
    let mut errors = 0;
    for path in &documents {
        let document = Document::new(path.clone());
        match provider.read(&document) {
            Ok(Some(progress)) => {
                with_record += 1;
                println!("{}\t{}", path.display(), progress.page);
            }
            Ok(None) => {}
            Err(error) => {
                errors += 1;
                println!("# error reading {}: {error}", path.display());
            }
        }
    }
    println!(
        "# {} PDFs, {with_record} with a recorded position, {errors} errors",
        documents.len()
    );
}

/// Collects `.pdf` files, without following symlinks and without shelling out to anything.
fn walk(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            walk(&path, found);
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
        {
            found.push(path);
        }
    }
}
