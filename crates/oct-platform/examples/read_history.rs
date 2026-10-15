//! Prints what the desktop's recent-files history knows about the paths it is given.
//!
//! The diagnostic for U14. The carousels read a document's *reading* time from this history, and a wrong
//! answer here is invisible on screen — a document that is not found simply ages as if it had never been
//! opened. Running it against real paths is the only way to tell "the reader is wrong" apart from "the
//! viewer never recorded that document".
//!
//! It prints the aggregate count and one line per requested path, and never the history itself: the file
//! lists documents a person opened, and a diagnostic has no business dumping it.
//!
//! ```text
//! cargo run -p oct-platform --example read_history -- /path/to/one.pdf /path/to/another.pdf
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use oct_platform::recent::{RecentDocuments, recently_used_path};

fn main() -> ExitCode {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    let history = match recently_used_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("cannot say where the history lives: {error}");
            return ExitCode::FAILURE;
        }
    };
    let documents = match RecentDocuments::read(&history) {
        Ok(documents) => documents,
        Err(error) => {
            eprintln!("cannot read {}: {error}", history.display());
            return ExitCode::FAILURE;
        }
    };

    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => elapsed.as_secs() as i64,
        Err(error) => {
            eprintln!("the clock is before the Unix epoch: {error}");
            return ExitCode::FAILURE;
        }
    };

    println!("{}: {} entries", history.display(), documents.len());
    let mut known = 0;
    for path in &paths {
        match documents.read_at(path) {
            Some(read_at) => {
                known += 1;
                let days = (now - read_at) as f64 / 86_400.0;
                println!("{days:>8.1} days   {}", path.display());
            }
            None => println!("  unknown      {}", path.display()),
        }
    }
    println!();
    println!("{known} of {} given paths are in the history", paths.len());
    ExitCode::SUCCESS
}
