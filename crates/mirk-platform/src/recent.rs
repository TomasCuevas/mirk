//! The desktop's own recent-files history, and what it recorded about reading.
//!
//! `mirk`'s database knows when *it* wrote a row, and the launch sweep writes a whole library at
//! once, so every document looks freshly read. The one place where the **viewer** wrote down that a
//! person opened a document, and when, is the freedesktop Recent files list that GTK maintains at
//! `~/.local/share/recently-used.xbel`. This module reads it.
//!
//! # Tolerant by contract
//!
//! The file is an XML stream written by another application, and it is allowed to be truncated,
//! reordered, or in a dialect this parser has never seen. A parse therefore never fails on content:
//! it scans for `<bookmark>` elements, keeps the `file://` ones it can decode, and ignores the
//! rest. That is also why [`RecentDocuments::len`] exists — a caller can report how much was read
//! rather than presenting an empty history as an empty library.
//!
//! # The timestamp is not "now"
//!
//! The bookmark carries the moment the opener wrote, not the moment this process looked. Using it
//! is the whole point: the project measured that after a launch sweep every row's timestamp was the
//! sweep's, never the reader's, which is why a "not opened in a long time" category could never
//! contain anything.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use mirk_core::MirkError;

/// The file a GNOME/GTK desktop keeps its recent-files history in, relative to `$XDG_DATA_HOME`.
const RECENTLY_USED_FILE: &str = "recently-used.xbel";

/// The desktop's recent-files history: what the system's own applications recorded about the files
/// a person opened, and when.
///
/// Only `file://` entries that name a local path are represented. A path maps to the newest time
/// any bookmark recorded for it; an entry whose timestamps are missing entirely is counted by
/// [`len`](Self::len) but answers `None` from [`read_at`](Self::read_at), because "the history has
/// no time for this" and "the history does not know this" must not be confused with epoch.
pub struct RecentDocuments {
    /// Newest observed time per decoded local path. Absent means no bookmark carried a usable time.
    entries: HashMap<PathBuf, i64>,
    /// Every `file://` bookmark that decoded to a local path, with or without a timestamp.
    parsed: usize,
}

impl RecentDocuments {
    /// Parses the history at `path`.
    ///
    /// A file that is not there is an EMPTY history rather than an error: a machine with no history
    /// is a machine where nothing was opened through this mechanism. A file that exists and cannot
    /// be read is a real error and is reported as one.
    pub fn read(path: &Path) -> Result<Self, MirkError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            // "No history" is a fact, not a failure: it is what every fresh profile looks like.
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::empty()),
            // A directory, a permission denial, or an I/O fault is a real error and must surface.
            Err(error) => return Err(MirkError::Io(error)),
        };

        // Lossy on purpose: a history with one bad byte is still worth the other 988 entries.
        Ok(Self::parse(&String::from_utf8_lossy(&bytes)))
    }

    /// When the history last recorded opening `path`, in Unix seconds. `None` means the history
    /// does not know this document, which is a different fact from "opened long ago".
    pub fn read_at(&self, path: &Path) -> Option<i64> {
        self.entries.get(path).copied()
    }

    /// How many `file://` entries were parsed. Callers report it; a silent empty history and a
    /// well-populated one must not look the same from outside.
    pub fn len(&self) -> usize {
        self.parsed
    }

    /// Whether nothing was parsed. `len` is a measurement, so this is the companion it wants.
    pub fn is_empty(&self) -> bool {
        self.parsed == 0
    }

    /// The history a machine with no readable recent-files list has: nothing, with no error.
    fn empty() -> Self {
        Self {
            entries: HashMap::new(),
            parsed: 0,
        }
    }

    /// Scans `contents` for bookmarks, keeping the `file://` ones and the newest time each carries.
    fn parse(contents: &str) -> Self {
        let mut history = Self::empty();

        for tag in bookmark_tags(contents) {
            let attributes = parse_attributes(tag);

            // Attribute order is not guaranteed, so every lookup is by name.
            let Some(href) = attribute(&attributes, "href") else {
                continue;
            };
            // A non-empty host is a remote file; pretending to know its local path would invent a
            // document. Anything that is not `file://` is simply not this module's business.
            let Some(path) = local_path_from_href(href) else {
                continue;
            };

            history.parsed += 1;

            // `added` is ignored: when GTK first saw the document is not when a person read it.
            let modified = attribute(&attributes, "modified").and_then(parse_iso8601_utc);
            let visited = attribute(&attributes, "visited").and_then(parse_iso8601_utc);
            let Some(time) = newest_of(modified, visited) else {
                // Neither attribute was present or usable. Leaving the path out would lose the
                // fact that it was parsed; storing `0` would read as 1970 and make a document
                // look ancient. It is counted, and answers `None`.
                continue;
            };

            history
                .entries
                .entry(path)
                .and_modify(|existing| *existing = (*existing).max(time))
                .or_insert(time);
        }

        history
    }
}

/// Where the history lives, following XDG: `$XDG_DATA_HOME/recently-used.xbel`, falling back to
/// `$HOME/.local/share/recently-used.xbel`.
///
/// This reuses `mirk-store`'s `user_data_dir_for` convention — read the environment here, then
/// resolve in a testable helper that takes the variables as parameters — rather than a second,
/// divergent reading of `$XDG_DATA_HOME`.
pub fn recently_used_path() -> Result<PathBuf, MirkError> {
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);

    recently_used_path_for(xdg_data_home.as_deref(), home.as_deref())
}

/// Resolves the history path for an explicit environment.
///
/// An empty `XDG_DATA_HOME` is treated as unset, matching `mirk-store`: GTK does the same, and a
/// blank value is a configuration mistake rather than a request to look in the root directory.
/// Missing both variables is [`MirkError::Storage`], because guessing `~` when `$HOME` is unknown
/// would read the wrong user's history.
fn recently_used_path_for(
    xdg_data_home: Option<&Path>,
    home: Option<&Path>,
) -> Result<PathBuf, MirkError> {
    let base = match xdg_data_home {
        Some(directory) if !directory.as_os_str().is_empty() => directory.to_path_buf(),
        _ => home
            .ok_or_else(|| {
                MirkError::Storage(
                    "cannot locate the recent-files history: neither XDG_DATA_HOME nor HOME is set"
                        .to_string(),
                )
            })?
            .join(".local/share"),
    };

    Ok(base.join(RECENTLY_USED_FILE))
}

/// The `<bookmark ...>` tag bodies in `contents`, without the leading `<bookmark` or the closing
/// `>`.
///
/// The scan deliberately does not validate XML: it finds the marker, ends the tag at the first `>`
/// outside quotes, and moves on. Malformed content costs the entries it contains and nothing else.
fn bookmark_tags(contents: &str) -> Vec<&str> {
    const MARKER: &str = "<bookmark";

    let mut tags = Vec::new();
    let mut rest = contents;

    while let Some(position) = rest.find(MARKER) {
        rest = &rest[position + MARKER.len()..];

        // `<bookmarks>` and `<bookmarkfoo` are not `<bookmark>` elements.
        let Some(first) = rest.chars().next() else {
            break;
        };
        if !(first.is_ascii_whitespace() || first == '/' || first == '>') {
            continue;
        }

        let Some(end) = tag_end(rest) else {
            // Truncated: no closing `>` remains, so the tail has no complete tag.
            break;
        };
        tags.push(&rest[..end]);
        rest = &rest[end..];
    }

    tags
}

/// The byte offset of the `>` that closes a tag, respecting double-quoted attribute values.
fn tag_end(tag: &str) -> Option<usize> {
    let mut quoted = false;

    for (index, byte) in tag.bytes().enumerate() {
        match byte {
            b'"' => quoted = !quoted,
            b'>' if !quoted => return Some(index),
            _ => {}
        }
    }

    None
}

/// Every `name="value"` in a tag body, with the value entity-decoded.
///
/// Names are returned as written and matched case-sensitively by the caller, which is correct for
/// XBEL: the specification spells them lower-case.
fn parse_attributes(tag: &str) -> Vec<(String, String)> {
    let bytes = tag.as_bytes();
    let mut attributes = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        let progress = index;

        // Skip whitespace and the `/` of a self-closing tag.
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b'/') {
            index += 1;
        }

        let name_start = index;
        while index < bytes.len() && bytes[index] != b'=' && !bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let name = &tag[name_start..index];

        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }

        if index < bytes.len() && bytes[index] == b'=' {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }

            if let Some((value, next)) = attribute_value(tag, index) {
                if !name.is_empty() {
                    attributes.push((name.to_string(), value));
                }
                index = next;
            }
        }

        // A malformed fragment must still consume a byte, or the scan never terminates.
        if index == progress {
            index += 1;
        }
    }

    attributes
}

/// One attribute value starting at `start`, plus the offset just past it.
///
/// A quoted value ends at its matching quote; an unterminated one runs to the end of the tag. An
/// unquoted value ends at the next whitespace. All three are accepted, because the goal is to
/// extract what is there rather than to reject what is not canonical.
fn attribute_value(tag: &str, start: usize) -> Option<(String, usize)> {
    let bytes = tag.as_bytes();

    if let Some(quote) = bytes
        .get(start)
        .copied()
        .filter(|byte| *byte == b'"' || *byte == b'\'')
    {
        let value_start = start + 1;
        let mut index = value_start;
        while index < bytes.len() && bytes[index] != quote {
            index += 1;
        }
        let value = decode_entities(&tag[value_start..index]);
        let next = if index < bytes.len() {
            index + 1
        } else {
            index
        };
        return Some((value, next));
    }

    if start >= bytes.len() {
        return None;
    }

    let mut index = start;
    while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
        index += 1;
    }

    Some((decode_entities(&tag[start..index]), index))
}

/// The value of the attribute called `name`, if the tag carries it.
fn attribute<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(attribute_name, _)| attribute_name == name)
        .map(|(_, value)| value.as_str())
}

/// Expands the XML entities GTK emits in `href` values, leaving unknown ones verbatim.
///
/// The five predefined entities and numeric character references are handled; anything else is
/// copied through unchanged so a novel entity cannot silently delete part of a path.
fn decode_entities(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut rest = value;

    while let Some(ampersand) = rest.find('&') {
        decoded.push_str(&rest[..ampersand]);
        let tail = &rest[ampersand..];

        let Some(semicolon) = tail.find(';') else {
            decoded.push_str(tail);
            return decoded;
        };

        let entity = &tail[1..semicolon];
        match entity {
            "amp" => decoded.push('&'),
            "lt" => decoded.push('<'),
            "gt" => decoded.push('>'),
            "quot" => decoded.push('"'),
            "apos" => decoded.push('\''),
            _ => match decode_numeric_entity(entity) {
                Some(character) => decoded.push(character),
                None => decoded.push_str(&tail[..=semicolon]),
            },
        }

        rest = &tail[semicolon + 1..];
    }

    decoded.push_str(rest);
    decoded
}

/// Decodes `#123` or `#x1F` into its character, or `None` when it is not a numeric reference.
fn decode_numeric_entity(entity: &str) -> Option<char> {
    let digits = entity.strip_prefix('#')?;
    let (radix, digits) = match digits.strip_prefix(['x', 'X']) {
        Some(hex) => (16, hex),
        None => (10, digits),
    };

    char::from_u32(u32::from_str_radix(digits, radix).ok()?)
}

/// Turns a `file://` href into a local path, decoding percent escapes. `None` for anything else.
///
/// A `file://` URI is `file://` + authority + path. Empty authority (the `file:///` form GTK
/// writes) is this machine; a non-empty one — including `localhost` — is not a path this module
/// can vouch for, so it is dropped rather than guessed at.
fn local_path_from_href(href: &str) -> Option<PathBuf> {
    let rest = strip_prefix_ignore_ascii_case(href, "file://")?;
    let slash = rest.find('/')?;
    let (authority, path) = rest.split_at(slash);

    if !authority.is_empty() {
        return None;
    }

    Some(path_from_bytes(percent_decode(path)))
}

/// Strips `prefix` when it matches `value` case-insensitively. ASCII-only, like URI schemes.
fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        Some(&value[prefix.len()..])
    } else {
        None
    }
}

/// Decodes `%XX` escapes byte-wise, the way URIs encode them, and copies every other byte.
fn percent_decode(value: &str) -> Vec<u8> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
            {
                decoded.push(high << 4 | low);
                index += 3;
                continue;
            }
        }

        decoded.push(bytes[index]);
        index += 1;
    }

    decoded
}

/// The value of one hexadecimal digit, or `None`.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// A path from decoded URI bytes.
///
/// On Unix a filename is an arbitrary byte sequence, and that is exactly what percent-decoding
/// produced, so the bytes become an [`std::ffi::OsStr`] without a UTF-8 round trip. Elsewhere the
/// platform requires UTF-8, and lossy decoding is the tolerant fallback.
#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    PathBuf::from(OsStr::from_bytes(&bytes))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

/// The larger of two optional times, where `None` carries no information rather than zero.
fn newest_of(first: Option<i64>, second: Option<i64>) -> Option<i64> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.max(second)),
        (Some(time), None) | (None, Some(time)) => Some(time),
        (None, None) => None,
    }
}

/// Parses an ISO-8601 UTC timestamp such as `2026-09-08T07:06:05.654321Z` into Unix seconds.
///
/// The fields are fixed-width, so they are read by position. The conversion uses the standard
/// days-from-civil algorithm rather than a `Date`-like type, and the tests pin it against values
/// derived outside this codebase (`date -u` and Python's `calendar.timegm`), because a hand-rolled
/// calendar that agrees only with itself is the classic silent wrong answer.
///
/// Anything that is not the `Z` form — an offset, a missing fraction, a bad digit — returns
/// `None` for that attribute rather than a made-up time.
fn parse_iso8601_utc(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20 {
        return None;
    }

    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }

    let year = digits(&bytes[0..4])?;
    let month = digits(&bytes[5..7])?;
    let day = digits(&bytes[8..10])?;
    let hour = digits(&bytes[11..13])?;
    let minute = digits(&bytes[14..16])?;
    let second = digits(&bytes[17..19])?;

    // Optional fractional seconds, ignored: Unix seconds have no sub-second component.
    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let fraction_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == fraction_start {
            return None;
        }
    }

    if !matches!(bytes.get(index), Some(&b'Z') | Some(&b'z')) {
        return None;
    }

    // Ranges are checked so a garbage field cannot overflow the arithmetic below.
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }

    let days = days_from_civil(i64::from(year), i64::from(month), i64::from(day));
    Some(days * 86_400 + i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second))
}

/// The unsigned number spelled by `bytes`, or `None` if any byte is not an ASCII digit.
fn digits(bytes: &[u8]) -> Option<u32> {
    let mut value = 0u32;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(byte - b'0');
    }

    Some(value)
}

/// Days from 1970-01-01 to `year-month-day`, by Howard Hinnant's `days_from_civil`.
///
/// This is the standard shift of the Gregorian calendar to a March-based epoch, which makes the
/// leap-day rule a single division and avoids a table of month lengths.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes `contents` to a `recently-used.xbel` in a fresh temp directory, returning both so the
    /// directory outlives the test.
    fn write_history(contents: impl AsRef<[u8]>) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join(RECENTLY_USED_FILE);
        std::fs::write(&path, contents).expect("write fixture");
        (directory, path)
    }

    #[test]
    fn modified_yields_the_time_and_read_at_finds_it_by_path() {
        // 2026-09-08T07:06:05Z. Derived independently: `date -u -d '2026-09-08T07:06:05Z' +%s`
        // and `calendar.timegm((2026, 9, 8, 7, 6, 5))` both give 1788851165.
        let (_directory, path) = write_history(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <xbel version=\"1.0\">\n\
             <bookmark href=\"file:///home/someone/Documents/Novela.pdf\" \
             added=\"2024-03-01T10:11:12.123456Z\" \
             modified=\"2026-09-08T07:06:05.654321Z\" \
             visited=\"2026-09-08T07:06:05.654321Z\">\n\
             </bookmark>\n\
             </xbel>\n",
        );

        let history = RecentDocuments::read(&path).expect("read the fixture");
        assert_eq!(
            history.read_at(Path::new("/home/someone/Documents/Novela.pdf")),
            Some(1_788_851_165)
        );
    }

    #[test]
    fn attribute_order_does_not_matter() {
        // The measured file has no guaranteed order, so `href` must be found by name in any
        // position — here it is last, which a positional reader would miss entirely.
        let (_directory, path) = write_history(
            "<xbel>\n\
             <bookmark modified=\"2026-09-08T07:06:05.000000Z\" \
             visited=\"2026-09-08T07:06:05.000000Z\" \
             added=\"2024-03-01T10:11:12.123456Z\" \
             href=\"file:///data/ordered.pdf\"/>\n\
             </xbel>\n",
        );

        let history = RecentDocuments::read(&path).expect("read the fixture");
        assert_eq!(
            history.read_at(Path::new("/data/ordered.pdf")),
            Some(1_788_851_165)
        );
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn the_newest_of_modified_and_visited_wins() {
        // `visited` is three hours later than `modified`; the later read is the one that matters.
        let (_directory, path) = write_history(
            "<xbel>\n\
             <bookmark href=\"file:///data/later.pdf\" \
             modified=\"2026-09-08T07:06:05.000000Z\" \
             visited=\"2026-09-08T10:06:05.000000Z\"/>\n\
             <bookmark href=\"file:///data/earlier.pdf\" \
             modified=\"2026-09-08T10:06:05.000000Z\" \
             visited=\"2026-09-08T07:06:05.000000Z\"/>\n\
             </xbel>\n",
        );

        let history = RecentDocuments::read(&path).expect("read the fixture");
        assert_eq!(
            history.read_at(Path::new("/data/later.pdf")),
            Some(1_788_851_165 + 3 * 3_600)
        );
        assert_eq!(
            history.read_at(Path::new("/data/earlier.pdf")),
            Some(1_788_851_165 + 3 * 3_600)
        );
    }

    #[test]
    fn a_single_timestamp_is_used_and_missing_timestamps_are_not_epoch() {
        let (_directory, path) = write_history(
            "<xbel>\n\
             <bookmark href=\"file:///data/only-visited.pdf\" \
             visited=\"2026-09-08T07:06:05.000000Z\"/>\n\
             <bookmark href=\"file:///data/only-modified.pdf\" \
             modified=\"2026-09-08T07:06:05.000000Z\"/>\n\
             <bookmark href=\"file:///data/no-time.pdf\" added=\"2024-03-01T10:11:12.123456Z\"/>\n\
             </xbel>\n",
        );

        let history = RecentDocuments::read(&path).expect("read the fixture");
        assert_eq!(
            history.read_at(Path::new("/data/only-visited.pdf")),
            Some(1_788_851_165)
        );
        assert_eq!(
            history.read_at(Path::new("/data/only-modified.pdf")),
            Some(1_788_851_165)
        );

        // The entry is known and counted, but its time is unknown — not 1970, which would render
        // as "opened in 1970" and put a fresh document in the "long ago" category.
        assert_eq!(history.read_at(Path::new("/data/no-time.pdf")), None);
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn percent_and_entity_escapes_are_decoded() {
        // `%20` is a space and `&amp;` is the XML-escaped ampersand a real filename would carry.
        let (_directory, path) = write_history(
            "<xbel>\n\
             <bookmark \
             href=\"file:///home/someone/Libros/Una%20Novela%20%26%20M%C3%A1s.pdf\" \
             modified=\"2026-09-08T07:06:05.000000Z\"/>\n\
             </xbel>\n",
        );

        let history = RecentDocuments::read(&path).expect("read the fixture");
        assert_eq!(
            history.read_at(Path::new("/home/someone/Libros/Una Novela & Más.pdf")),
            Some(1_788_851_165)
        );
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn non_file_hrefs_are_ignored_and_not_counted() {
        let (_directory, path) = write_history(
            "<xbel>\n\
             <bookmark href=\"https://example.com/paper.pdf\" \
             visited=\"2026-09-08T07:06:05.000000Z\"/>\n\
             <bookmark href=\"trash:///gone.pdf\" visited=\"2026-09-08T07:06:05.000000Z\"/>\n\
             <bookmark href=\"file://remote-host/share/doc.pdf\" \
             visited=\"2026-09-08T07:06:05.000000Z\"/>\n\
             <bookmark href=\"file:///data/kept.pdf\" \
             visited=\"2026-09-08T07:06:05.000000Z\"/>\n\
             </xbel>\n",
        );

        let history = RecentDocuments::read(&path).expect("read the fixture");
        assert_eq!(history.len(), 1, "only the local file:// entry counts");
        assert_eq!(
            history.read_at(Path::new("/data/kept.pdf")),
            Some(1_788_851_165)
        );
        assert_eq!(history.read_at(Path::new("/share/doc.pdf")), None);
    }

    #[test]
    fn a_missing_file_is_an_empty_history() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("does-not-exist.xbel");

        let history = RecentDocuments::read(&path).expect("a missing history is not an error");
        assert!(history.is_empty());
        assert_eq!(history.len(), 0);
    }

    #[test]
    fn a_directory_is_an_honest_error() {
        let directory = tempfile::tempdir().expect("temp dir");

        // The path exists but is not a readable file, so this is a real fault rather than "no
        // history": reporting it as empty would hide a broken environment.
        let result = RecentDocuments::read(directory.path());
        assert!(matches!(result, Err(MirkError::Io(_))));
    }

    #[test]
    fn iso_timestamps_convert_to_unix_seconds() {
        // Every expectation below was derived outside this codebase, with `date -u -d '<ts>' +%s`
        // and confirmed against Python's `calendar.timegm`:
        //   2026-09-08T07:06:05Z -> 1788851165
        //   2024-02-29T00:00:00Z -> 1709164800  (leap-year boundary)
        //   2024-03-01T00:00:00Z -> 1709251200  (day after the leap day)
        //   2000-01-01T00:00:00Z -> 946684800   (a leap year divisible by 400)
        //   1970-01-01T00:00:00Z -> 0
        assert_eq!(
            parse_iso8601_utc("2026-09-08T07:06:05.654321Z"),
            Some(1_788_851_165)
        );
        assert_eq!(
            parse_iso8601_utc("2024-02-29T00:00:00.000000Z"),
            Some(1_709_164_800)
        );
        assert_eq!(
            parse_iso8601_utc("2024-03-01T00:00:00.000000Z"),
            Some(1_709_251_200)
        );
        assert_eq!(
            parse_iso8601_utc("2000-01-01T00:00:00.000000Z"),
            Some(946_684_800)
        );
        assert_eq!(parse_iso8601_utc("1970-01-01T00:00:00Z"), Some(0));
    }

    #[test]
    fn the_iso_parser_rejects_shapes_it_cannot_convert() {
        // No fraction and no `Z`; an offset form; a bad field; a truncated string.
        assert_eq!(parse_iso8601_utc("2026-09-08T07:06:05"), None);
        assert_eq!(parse_iso8601_utc("2026-09-08T07:06:05+02:00"), None);
        assert_eq!(parse_iso8601_utc("2026-13-08T07:06:05.000000Z"), None);
        assert_eq!(parse_iso8601_utc("2026-09-08T07:06:05."), None);
        assert_eq!(parse_iso8601_utc("not a time"), None);
    }

    #[test]
    fn garbage_content_yields_an_empty_history_without_panicking() {
        // The last case is not valid UTF-8, which `read` must decode lossily rather than fail on.
        let cases: [&[u8]; 4] = [
            b"this is not xml at all\n",
            b"<bookmark href=file:///half-open\n",
            b"<bookmark href=\"file:///no/close\n",
            b"<?xml version=\"1.0\"?>\xff\xfe binary\n",
        ];

        for garbage in cases {
            let (_directory, path) = write_history(garbage);
            let history = RecentDocuments::read(&path).expect("garbage is not an error");
            assert!(
                history.is_empty(),
                "garbage parsed as {len} entries: {garbage:?}",
                len = history.len()
            );
        }
    }

    #[test]
    fn recently_used_path_prefers_xdg_data_home() {
        let path = recently_used_path_for(
            Some(Path::new("/xdg/data")),
            Some(Path::new("/home/someone")),
        )
        .expect("resolve");

        assert_eq!(path, Path::new("/xdg/data/recently-used.xbel"));
    }

    #[test]
    fn recently_used_path_falls_back_to_home_and_ignores_an_empty_xdg() {
        let fallback =
            recently_used_path_for(None, Some(Path::new("/home/someone"))).expect("resolve");
        assert_eq!(
            fallback,
            Path::new("/home/someone/.local/share/recently-used.xbel")
        );

        // An empty `XDG_DATA_HOME` is a blank configuration, not "look in /".
        let empty = recently_used_path_for(Some(Path::new("")), Some(Path::new("/home/someone")))
            .expect("resolve");
        assert_eq!(
            empty,
            Path::new("/home/someone/.local/share/recently-used.xbel")
        );
    }

    #[test]
    fn recently_used_path_errors_when_the_environment_cannot_say() {
        let error = recently_used_path_for(None, None).expect_err("no HOME and no XDG");
        assert!(matches!(error, MirkError::Storage(_)), "got {error:?}");
    }
}
