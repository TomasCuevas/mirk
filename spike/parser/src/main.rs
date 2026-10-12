//! One measurement: parse one PDF with one parser and report what it cost.
//!
//! Usage: `parser-spike <lopdf|pdfium> <path-to-pdf>`
//!
//! Prints exactly one JSON line on stdout, so a driver can collect results without parsing prose.
//! Self-measured rather than externally sampled: wall time, CPU time and peak RSS are read from the
//! process itself, which is the only way to attribute them to *this* run when several are queued.
//!
//! This is a spike, not product code. It exists to answer one question with numbers: is `lopdf`
//! viable for counting pages in a real library, or does the job need an FFI parser like `pdfium`?

use std::time::Instant;

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() != 3 {
        eprintln!("usage: parser-spike <lopdf|pdfium> <path-to-pdf>");
        std::process::exit(2);
    }
    let arm = arguments[1].as_str();
    let path = arguments[2].as_str();

    let started = Instant::now();
    let outcome = match arm {
        "lopdf" => lopdf_pages(path),
        "pdfium" => pdfium_pages(path),
        other => {
            eprintln!("unknown arm `{other}`");
            std::process::exit(2);
        }
    };
    let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
    let (cpu_ms, peak_rss_kb) = self_usage();

    match outcome {
        Ok(pages) => println!(
            "{{\"arm\":\"{arm}\",\"pages\":{pages},\"wall_ms\":{wall_ms:.0},\
             \"cpu_ms\":{cpu_ms:.0},\"peak_rss_kb\":{peak_rss_kb}}}"
        ),
        Err(error) => {
            let error = error.replace('\\', "\\\\").replace('"', "\\\"");
            println!(
                "{{\"arm\":\"{arm}\",\"error\":\"{error}\",\"wall_ms\":{wall_ms:.0},\
                 \"cpu_ms\":{cpu_ms:.0},\"peak_rss_kb\":{peak_rss_kb}}}"
            );
            std::process::exit(1);
        }
    }
}

/// Counts pages by loading the whole document into `lopdf`'s object graph.
fn lopdf_pages(path: &str) -> Result<usize, String> {
    let document = lopdf::Document::load(path).map_err(|error| error.to_string())?;
    if document.is_encrypted() {
        return Err("encrypted".to_string());
    }
    Ok(document.get_pages().len())
}

/// Counts pages through `pdfium`, the parser Chrome renders with.
fn pdfium_pages(path: &str) -> Result<usize, String> {
    use pdfium_render::prelude::*;

    let library_directory =
        std::env::var("PDFIUM_LIB_DIR").unwrap_or_else(|_| "vendor/lib".to_string());
    let library_name = Pdfium::pdfium_platform_library_name_at_path(&library_directory);
    let bindings = Pdfium::bind_to_library(library_name).map_err(|error| format!("{error:?}"))?;
    let pdfium = Pdfium::new(bindings);

    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|error| format!("{error:?}"))?;
    Ok(document.pages().len() as usize)
}

/// This process's CPU time and peak resident memory, from `/proc`.
///
/// Linux-only, which is all this spike claims. Peak RSS is `VmHWM` — the kernel's own high-water
/// mark — rather than a sample, so a short-lived allocation spike cannot be missed.
fn self_usage() -> (f64, u64) {
    let ticks_per_second = 100.0; // USER_HZ on Linux; the spike is Linux-only by design.
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    // Fields 14 and 15 are utime and stime, counted from after the comm field, which may itself
    // contain spaces and parentheses.
    let cpu_ms = stat
        .rfind(')')
        .and_then(|end| {
            let fields: Vec<&str> = stat[end + 1..].split_whitespace().collect();
            let user = fields.get(11)?.parse::<f64>().ok()?;
            let system = fields.get(12)?.parse::<f64>().ok()?;
            Some((user + system) / ticks_per_second * 1000.0)
        })
        .unwrap_or(0.0);

    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let peak_rss_kb = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);

    (cpu_ms, peak_rss_kb)
}
