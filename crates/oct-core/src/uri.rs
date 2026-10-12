//! Percent-encoded `file://` URIs, the key format several platforms use for per-document state.
//!
//! This matters more than it looks. GNOME's GVFS metadata — where Evince stores the current page —
//! is keyed by URI, not by path, and the encoding has to match byte for byte or the lookup simply
//! finds nothing.
//!
//! Two traps this module exists to contain:
//!
//! - **Encoding is byte-wise, not character-wise.** A non-ASCII character expands to one `%XX`
//!   escape per UTF-8 byte: `ó` becomes `%C3%B3`, not `%F3`.
//! - **Unicode normalisation differs per platform in the same characters.** macOS stores
//!   filenames decomposed (NFD) while Linux and Windows generally use composed (NFC) forms, so
//!   `Comparación` has two different byte sequences that *look* identical. Nothing in this module
//!   normalises, because normalising would break the lookup on the platform whose filesystem
//!   actually holds the file: the caller must pass the path exactly as the OS reported it.

/// The bytes that are never percent-encoded, per RFC 3986's `unreserved` set, plus `/`.
const fn is_unescaped(byte: u8) -> bool {
    matches!(byte,
        b'A'..=b'Z'
        | b'a'..=b'z'
        | b'0'..=b'9'
        | b'-' | b'_' | b'.' | b'~'
        | b'/'
    )
}

const HEX: [u8; 16] = *b"0123456789ABCDEF";

/// Builds the `file://` URI for an absolute path.
///
/// The input is treated as an opaque byte sequence taken from the OS, which is why this takes a
/// [`std::path::Path`] rather than a `&str`: on Unix a filename is not required to be valid UTF-8,
/// and `as_encoded_bytes` gives us exactly what the filesystem holds.
///
/// # Platform coverage
///
/// This produces the **POSIX** form, which is the one GNOME, KDE and the freedesktop metadata
/// stores use. Windows drive letters and UNC shares need a separate translation and are not
/// handled yet; that belongs with the Windows backends rather than here.
pub fn file_uri_from_path(path: &std::path::Path) -> String {
    let bytes = path.as_os_str().as_encoded_bytes();

    let mut out = String::with_capacity(bytes.len() + 8);
    out.push_str("file://");

    for &byte in bytes {
        if is_unescaped(byte) {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push(HEX[usize::from(byte >> 4)] as char);
            out.push(HEX[usize::from(byte & 0x0f)] as char);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn absolute_path_gets_the_three_slash_form() {
        assert_eq!(
            file_uri_from_path(Path::new("/home/user/doc.pdf")),
            "file:///home/user/doc.pdf"
        );
    }

    #[test]
    fn spaces_become_percent_escapes() {
        assert_eq!(
            file_uri_from_path(Path::new("/a/b c.pdf")),
            "file:///a/b%20c.pdf"
        );
    }

    #[test]
    fn non_ascii_escapes_per_utf8_byte() {
        // `ó` is U+00F3, two UTF-8 bytes: C3 B3. A single `%F3` would be wrong.
        assert_eq!(
            file_uri_from_path(Path::new("/libros/Comparación.pdf")),
            "file:///libros/Comparaci%C3%B3n.pdf"
        );
    }

    #[test]
    fn unreserved_characters_are_left_alone() {
        assert_eq!(
            file_uri_from_path(Path::new("/a/A-Za_z0.9~x.pdf")),
            "file:///a/A-Za_z0.9~x.pdf"
        );
    }

    #[test]
    fn uri_delimiters_are_escaped_so_they_cannot_split_the_uri() {
        // A literal `#` in a filename would otherwise start a fragment.
        assert_eq!(
            file_uri_from_path(Path::new("/a/b#c?d.pdf")),
            "file:///a/b%23c%3Fd.pdf"
        );
    }

    #[test]
    fn uppercase_hex_is_used() {
        // Matches what `gio info` and GVFS produce; lowercase would fail to match keys.
        assert!(file_uri_from_path(Path::new("/a/ñ.pdf")).contains("%C3%B1"));
    }

    #[test]
    fn document_uri_delegates_to_the_encoder() {
        let doc = crate::Document::new("/tmp/Informe Anual.pdf");
        assert_eq!(doc.uri(), "file:///tmp/Informe%20Anual.pdf");
    }
}
