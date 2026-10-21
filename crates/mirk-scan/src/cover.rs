//! Rendering and caching a document's cover (R32).
//!
//! The cover of a document is the first page of its PDF, rendered 400 px wide and written as a PNG
//! beside the database in the application's data directory (`~/.local/share/mirk/covers/` on Linux,
//! decided in L20). The first time a cover is needed it is produced; afterwards it is read back from
//! that file.
//!
//! # Why a PNG file and not the `image` stack
//!
//! `pdfium-render` can rasterise a page without new bindings, but its PNG/JPEG conveniences live
//! behind the `image` feature the workspace deliberately disabled, and re-enabling it would pull a
//! whole image codec stack back in for one format. The PNG written here is exactly that format, via
//! the smallest encoder that produces it.
//!
//! # Why these functions take a path and not a document
//!
//! Naming the cached file is the caller's business: the document's row id is the stable thing a
//! cover belongs to (a document that moves keeps its row), and that id lives in the store, not here.
//! This module only knows how to turn a page of a PDF at a path into a PNG at a path — which is what
//! makes it testable in a temp directory and keeps the data-directory knowledge at the application
//! boundary.
//!
//! # Which page
//!
//! The page is a parameter, not an assumption (R32): the reader may choose any page, so this module
//! renders the index it is given. The **caller** owns the cache's file name, which is why the chosen
//! page must appear in that name — a cover cached under a name that does not carry the page would keep
//! serving the old page after the reader chose another one.
//!
//! # One at a time
//!
//! pdfium is single-threaded behind this binding (`thread_safe` serialises its own calls), and R32
//! requires covers to be produced one at a time so a screen full of cards cannot queue a library's
//! worth of concurrent 400 px bitmaps behind one core. [`cover_for`] therefore holds a process-wide
//! lock across the check-render-write sequence. The caller runs it off the interface's thread; this
//! module never touches a window.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use mirk_core::MirkError;
use pdfium_render::prelude::{PdfRenderConfig, Pdfium};

/// The width every cover is rendered at, in pixels (R32).
pub const COVER_WIDTH: u32 = 400;

/// What obtaining a cover produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverOutcome {
    /// The cover is on disk at this path: it was already cached, or it was just rendered and written.
    Ready {
        /// The cover file, ready to be read.
        path: PathBuf,
    },
    /// The document could not be read, so it has no cover.
    ///
    /// This is a fact about the document — encrypted, deleted, unreadable — and **not** an
    /// application error. It travels as an outcome so the caller can keep the placeholder and report
    /// the reason where the application reports such things, instead of raising it at a screen.
    Unavailable {
        /// Why the document could not be rendered, for the log.
        reason: String,
    },
}

/// The process-wide render lock. It guards ordering, not data.
static RENDER_LOCK: Mutex<()> = Mutex::new(());

/// Renders `page` (zero-based) of `document` and writes it as a PNG to `cover_path`, or returns the
/// existing file when it is already there.
///
/// The sequence is one at a time (see the module docs): the cache is checked, the page is rendered,
/// and the file is written while the lock is held, so two requests for the same document cannot both
/// render it and a render cannot interleave with another one's. **The caller is responsible for the
/// file name carrying `page`**, so a changed choice asks for a different file instead of being served
/// the old cover.
///
/// A document that cannot be read is [`CoverOutcome::Unavailable`] rather than an error. A failure to
/// create the covers directory or to write the file **is** an error: that is the application's own
/// storage, and it must not be reported as "this document has no cover".
pub fn cover_for(
    pdfium: &Pdfium,
    cover_path: &Path,
    document: &Path,
    page: u32,
) -> Result<CoverOutcome, MirkError> {
    let _render = render_lock();

    if cover_path.is_file() {
        return Ok(CoverOutcome::Ready {
            path: cover_path.to_path_buf(),
        });
    }

    if let Some(directory) = cover_path.parent() {
        if !directory.as_os_str().is_empty() {
            fs::create_dir_all(directory)?;
        }
    }

    let png = match render_cover(pdfium, document, page) {
        Ok(png) => png,
        Err(reason) => return Ok(CoverOutcome::Unavailable { reason }),
    };

    write_atomically(cover_path, &png)?;

    Ok(CoverOutcome::Ready {
        path: cover_path.to_path_buf(),
    })
}

/// Renders `page` (zero-based) of `document` 400 px wide and encodes it as PNG bytes.
///
/// Kept private: the only supported way to obtain a cover is [`cover_for`], which owns the cache and
/// the serialisation. A caller who needs just the bytes can be added when one exists.
fn render_cover(pdfium: &Pdfium, document: &Path, page: u32) -> Result<Vec<u8>, String> {
    let pdf = pdfium
        .load_pdf_from_file(document, None)
        .map_err(|error| format!("cannot parse the PDF: {error}"))?;

    let pages = pdf.pages();
    if pages.is_empty() {
        return Err("the PDF contains no pages".to_string());
    }

    // A page number the PDF does not have is a normal answer for a reader's choice, and pdfium's
    // refusal is the honest reason; it names the page rather than reporting a generic failure. The
    // index type is pdfium's own `u16`, so a value past its range is refused here by name instead of
    // panicking inside the binding.
    let index = u16::try_from(page).map_err(|_| format!("page {page} is beyond the supported range"))?;

    // R32's borrow trap, recorded so it is not discovered a second time: `pages.get(i)` returns a
    // page that borrows a temporary of its own, so the page is bound to a name before it is used.
    let page_handle = pages
        .get(index)
        .map_err(|error| format!("cannot read page {page}: {error}"))?;

    // R32's other trap: `PdfRenderConfig`'s setters CONSUME the config and return it. Setting only
    // the width keeps the page's aspect ratio, so the height follows from it.
    let config = PdfRenderConfig::new().set_target_width(COVER_WIDTH as i32);

    let bitmap = page_handle
        .render_with_config(&config)
        .map_err(|error| format!("cannot render page {page}: {error}"))?;

    // RGBA whatever the bitmap's own format is, then released with the bitmap: R32's measurement is
    // ~875 KiB per 400 px bitmap, so a cover is rendered, encoded and dropped rather than collected.
    let rgba = bitmap.as_rgba_bytes();
    encode_png(bitmap.width() as u32, bitmap.height() as u32, &rgba)
}

/// Encodes 8-bit RGBA pixels as a PNG.
///
/// This is the whole encoder the cover path needs: one colour type, one depth, no options. `png`
/// writes that one format; the `image` stack `pdfium-render`'s `image` feature would pull in stays
/// out of the build.
fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("cannot start the PNG: {error}"))?;
        writer
            .write_image_data(rgba)
            .map_err(|error| format!("cannot write the PNG: {error}"))?;
    }
    Ok(bytes)
}

/// Takes the process-wide render lock, recovering a poisoned one.
///
/// The lock guards ordering rather than data, so a panic in one render must not make every later
/// cover fail: there is nothing to be inconsistent about, and refusing to recover would turn one
/// broken document into a library with no covers.
fn render_lock() -> MutexGuard<'static, ()> {
    RENDER_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Writes `bytes` to `cover_path` through a sibling temporary file and a rename.
///
/// A cover is served from the moment its file exists, so a half-written cache would be served as a
/// broken image forever. The rename is atomic on one filesystem, which the sibling temporary
/// guarantees; a crash between the two leaves the `.tmp` file and no cover, so the next request
/// renders again rather than serving a torn one.
fn write_atomically(cover_path: &Path, bytes: &[u8]) -> Result<(), MirkError> {
    let temporary = cover_path.with_extension("png.tmp");
    fs::write(&temporary, bytes)?;
    if let Err(error) = fs::rename(&temporary, cover_path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bind_pdfium_library;

    /// The repository's own four-page fixture, with `Title: A Known Title` and a real page.
    const INFO_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/with-info-metadata.pdf");

    fn fixture(directory: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = directory.join(name);
        fs::write(&path, bytes).expect("write the fixture");
        path
    }

    /// The PNG's signature and the dimensions from its own IHDR chunk, or `None` when `bytes` is not
    /// a PNG. Deliberately reads the structure rather than trusting the extension: the test is about
    /// a real image, not about "some bytes".
    fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
        const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
        if bytes.get(..8)? != SIGNATURE {
            return None;
        }
        // The first chunk after the signature is IHDR: 4-byte length, "IHDR", then width and height,
        // each big-endian.
        if bytes.get(12..16)? != b"IHDR" {
            return None;
        }
        let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
        let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
        Some((width, height))
    }

    #[test]
    fn a_cover_is_rendered_as_a_png_and_then_served_from_the_file() {
        let directory = tempfile::tempdir().expect("temp dir");
        let document = fixture(directory.path(), "document.pdf", INFO_FIXTURE);
        let cover_path = directory.path().join("covers").join("1.png");
        let pdfium = bind_pdfium_library().expect("bind the repository's pdfium");

        let first = cover_for(&pdfium, &cover_path, &document, 0).expect("obtain a cover");
        match first {
            CoverOutcome::Ready { path } => assert_eq!(path, cover_path),
            CoverOutcome::Unavailable { reason } => {
                panic!("a readable document must produce a cover, but got: {reason}")
            }
        }

        let first_bytes = fs::read(&cover_path).expect("read the cover from disk");
        let (width, height) =
            png_dimensions(&first_bytes).expect("the cover must be a PNG, not some bytes");
        assert_eq!(width, COVER_WIDTH, "the cover is rendered 400 px wide");
        assert!(height > 0, "a rendered page has a height");

        // The second request is served from the FILE: the source document is gone, so a renderer
        // that ignored the cache would have nothing to read and could not return these bytes.
        fs::remove_file(&document).expect("remove the source document");
        let second = cover_for(&pdfium, &cover_path, &document, 0).expect("serve from the cache");
        match second {
            CoverOutcome::Ready { path } => assert_eq!(path, cover_path),
            CoverOutcome::Unavailable { reason } => {
                panic!("the cached cover must be served, but the renderer ran: {reason}")
            }
        }
        assert_eq!(
            fs::read(&cover_path).expect("read the cover again"),
            first_bytes,
            "the second request must return the very file written by the first"
        );
    }

    #[test]
    fn a_missing_pdf_yields_no_cover_rather_than_an_error() {
        let directory = tempfile::tempdir().expect("temp dir");
        let missing = directory.path().join("gone.pdf");
        let cover_path = directory.path().join("covers").join("2.png");
        let pdfium = bind_pdfium_library().expect("bind the repository's pdfium");

        // The name is the contract: an unreadable document is "no cover", never an error and never a
        // broken image. `Err` here would surface as a failed request the screen would have to explain.
        match cover_for(&pdfium, &cover_path, &missing, 0) {
            Ok(CoverOutcome::Unavailable { reason }) => {
                assert!(!reason.is_empty(), "the reason is what the log reports")
            }
            Ok(CoverOutcome::Ready { .. }) => panic!("a missing document cannot have a cover"),
            Err(error) => panic!("a missing document is no-cover, not an error: {error}"),
        }

        assert!(
            !cover_path.exists(),
            "nothing may be written when there is nothing to render"
        );
    }

    #[test]
    fn a_pdf_that_cannot_be_parsed_yields_no_cover_rather_than_an_error() {
        let directory = tempfile::tempdir().expect("temp dir");
        // A file with the right extension and the wrong bytes: the encrypted/unreadable case R32
        // names, down to the one thing that matters here — pdfium refuses it.
        let corrupt = fixture(directory.path(), "corrupt.pdf", b"not a PDF at all");
        let cover_path = directory.path().join("covers").join("3.png");
        let pdfium = bind_pdfium_library().expect("bind the repository's pdfium");

        match cover_for(&pdfium, &cover_path, &corrupt, 0) {
            Ok(CoverOutcome::Unavailable { reason }) => assert!(!reason.is_empty()),
            Ok(CoverOutcome::Ready { .. }) => panic!("a corrupt document cannot have a cover"),
            Err(error) => panic!("an unparseable document is no-cover, not an error: {error}"),
        }
        assert!(!cover_path.exists(), "a failed render leaves no cover behind");
    }

    #[test]
    fn a_chosen_page_is_the_page_that_is_rendered() {
        // R32: the page is a parameter, so choosing another page really changes the cover that comes
        // back rather than re-rendering page one under a new file name. The repository's fixture has
        // identical pages, so this test builds the smallest PDF that can show it: page one is a solid
        // blue rectangle and page two is a red square on white. The bytes must differ.
        let directory = tempfile::tempdir().expect("temp dir");
        let document = fixture(directory.path(), "two-pages.pdf", &two_page_pdf());
        let covers = directory.path().join("covers");
        let pdfium = bind_pdfium_library().expect("bind the repository's pdfium");

        let first_page = covers.join("1-p0.png");
        let second_page = covers.join("1-p1.png");

        for (path, page) in [(&first_page, 0u32), (&second_page, 1u32)] {
            match cover_for(&pdfium, path, &document, page) {
                Ok(CoverOutcome::Ready { path: ready }) => {
                    assert_eq!(&ready, path, "the cover is written where it was asked for")
                }
                Ok(CoverOutcome::Unavailable { reason }) => {
                    panic!("a readable page must produce a cover, but got: {reason}")
                }
                Err(error) => panic!("rendering a readable page must not fail: {error}"),
            }
        }

        let first_bytes = fs::read(&first_page).expect("read page one's cover");
        let second_bytes = fs::read(&second_page).expect("read page two's cover");
        assert_ne!(
            first_bytes, second_bytes,
            "page two must render page two, not page one again"
        );
    }

    /// A two-page PDF whose pages render differently, built here rather than committed as a fixture.
    ///
    /// The repository's fixtures cannot answer this question: `with-info-metadata.pdf`'s four pages
    /// carry identical content, so any page renders the same image. This is the smallest PDF that
    /// distinguishes them — no font, no compression, two rectangles of different colours.
    fn two_page_pdf() -> Vec<u8> {
        let content_one: &[u8] = b"0 0 1 rg 0 0 300 400 re f";
        let content_two: &[u8] = b"1 0 0 rg 50 50 100 100 re f";
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Contents 5 0 R >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Contents 6 0 R >>".to_vec(),
            [
                format!("<< /Length {} >>\nstream\n", content_one.len()).as_bytes(),
                content_one,
                b"\nendstream",
            ]
            .concat(),
            [
                format!("<< /Length {} >>\nstream\n", content_two.len()).as_bytes(),
                content_two,
                b"\nendstream",
            ]
            .concat(),
        ];

        let mut out: Vec<u8> = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend_from_slice(body);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    #[test]
    fn a_page_the_pdf_does_not_have_is_no_cover_rather_than_an_error() {
        // A reader can choose a page beyond the end by hand-editing or from a PDF whose count moved.
        // The honest answer is the same as any other unreadable cover: the placeholder, with the reason
        // written down — never a panic and never a broken image.
        let directory = tempfile::tempdir().expect("temp dir");
        let document = fixture(directory.path(), "document.pdf", INFO_FIXTURE);
        let cover_path = directory.path().join("covers").join("1-p99.png");
        let pdfium = bind_pdfium_library().expect("bind the repository's pdfium");

        match cover_for(&pdfium, &cover_path, &document, 99) {
            Ok(CoverOutcome::Unavailable { reason }) => assert!(!reason.is_empty()),
            Ok(CoverOutcome::Ready { .. }) => panic!("a missing page cannot have a cover"),
            Err(error) => panic!("a missing page is no-cover, not an error: {error}"),
        }
        assert!(!cover_path.exists(), "a failed render leaves no cover behind");
    }
}
