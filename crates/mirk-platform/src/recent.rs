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

/// The mime type a bookmark records when its document is a PDF.
const PDF_MIME_TYPE: &str = "application/pdf";

/// The name of the attribute, or of the element, GTK writes a document's mime type into.
///
/// Real histories carry it as a child element — `<mime:mime-type type="application/pdf"/>` inside
/// `<info><metadata>` — but the name is used for both spellings, because the rule this module
/// answers is which mime type was recorded and not which XML shape carried it.
const MIME_TYPE: &str = "mime:mime-type";

/// One `<bookmark>` that named a local path, with everything the suggestion queue needs to know
/// about it.
///
/// This is the shape [`RecentDocuments::documents`] answers with, and it exists because
/// [`RecentDocuments::read_at`] answers a narrower question — *when* was this read? — and can only
/// say "no moment" for a document it knows perfectly well. A caller that has to decide whether a
/// document is worth suggesting needs the path even when its moment is unknown, and needs the
/// recorded type on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// The decoded local path the bookmark named.
    pub path: PathBuf,
    /// The newest time this path's bookmarks recorded, or `None` when none of them carried a usable
    /// one. `None` is "the history has no moment for this document", never 1970.
    pub last_used_unix: Option<i64>,
    /// Whether the bookmark recorded [`PDF_MIME_TYPE`] as the document's type.
    ///
    /// The extension rule is deliberately **not** applied here: this module reports what the history
    /// said, and the platform layer takes the union with the `.pdf` test it already uses, so the two
    /// axes cannot disagree about what a PDF is.
    pub recorded_as_pdf: bool,
}

/// The desktop's recent-files history: what the system's own applications recorded about the files
/// a person opened, and when.
///
/// Only `file://` entries that name a local path are represented. A path maps to the newest time
/// any bookmark recorded for it; an entry whose timestamps are missing entirely is counted by
/// [`len`](Self::len) but answers `None` from [`read_at`](Self::read_at), because "the history has
/// no time for this" and "the history does not know this" must not be confused with epoch. It is
/// nevertheless present in [`documents`](Self::documents), because a document the reader opened is
/// still a document when no moment was recorded for it.
pub struct RecentDocuments {
    /// Every distinct local path the history names, in the order each was first seen, with the
    /// newest time its bookmarks recorded and whether any of them typed it as a PDF.
    documents: Vec<HistoryEntry>,
    /// Where each path's entry lives in `documents`, so a bookmark repeated for one path merges into
    /// one entry instead of appearing twice. A lookup, not a second copy of the fact.
    index: HashMap<PathBuf, usize>,
    /// Every `file://` bookmark that decoded to a local path, with or without a timestamp and
    /// counted once per bookmark. This is what [`len`](Self::len) reports.
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
        let position = *self.index.get(path)?;
        self.documents[position].last_used_unix
    }

    /// Every distinct local path the history names, in the order each was first seen.
    ///
    /// Unlike [`read_at`](Self::read_at), this keeps a document whose bookmark recorded no time at
    /// all: the caller that decides whether to suggest it needs the path, and
    /// [`HistoryEntry::last_used_unix`] says honestly that its moment is unknown.
    pub fn documents(&self) -> &[HistoryEntry] {
        &self.documents
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
            documents: Vec::new(),
            index: HashMap::new(),
            parsed: 0,
        }
    }

    /// Scans `contents` for bookmarks, keeping the `file://` ones and the newest time each carries.
    fn parse(contents: &str) -> Self {
        let mut history = Self::empty();

        for bookmark in bookmarks(contents) {
            // Attribute order is not guaranteed, so every lookup is by name.
            let attributes = parse_attributes(bookmark.tag);

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
            // Neither attribute present or usable leaves this `None`. Leaving the path out would
            // lose the fact that it was parsed; storing `0` would read as 1970 and make a document
            // look ancient. It is kept as a document with no known moment.
            let time = newest_of(modified, visited);
            let recorded_as_pdf = recorded_pdf_mime(&attributes, bookmark.body);

            match history.index.get(&path) {
                Some(&position) => {
                    let entry = &mut history.documents[position];
                    entry.last_used_unix = newest_of(entry.last_used_unix, time);
                    entry.recorded_as_pdf |= recorded_as_pdf;
                }
                None => {
                    history.index.insert(path.clone(), history.documents.len());
                    history.documents.push(HistoryEntry {
                        path,
                        last_used_unix: time,
                        recorded_as_pdf,
                    });
                }
            }
        }

        history
    }
}

/// Whether a bookmark recorded [`PDF_MIME_TYPE`] as the document's type.
///
/// Both spellings are honoured — the child element GTK actually writes, and the same name on the
/// bookmark's own tag — because the question is which type was recorded, and a history from a
/// writer nobody has seen yet should not lose a PDF to a detail of the dialect.
fn recorded_pdf_mime(attributes: &[(String, String)], body: &str) -> bool {
    if attribute(attributes, MIME_TYPE)
        .map(is_pdf_mime_value)
        .unwrap_or(false)
    {
        return true;
    }

    mime_element_records_pdf(body)
}

/// Whether any `<mime:mime-type>` inside an element body names a PDF.
fn mime_element_records_pdf(body: &str) -> bool {
    const MARKER: &str = "<mime:mime-type";

    let mut rest = body;
    while let Some(position) = rest.find(MARKER) {
        rest = &rest[position + MARKER.len()..];
        let Some(end) = tag_end(rest) else {
            // Truncated: no closing `>` remains, so this element has no attributes to read.
            return false;
        };

        if attribute(&parse_attributes(&rest[..end]), "type")
            .map(is_pdf_mime_value)
            .unwrap_or(false)
        {
            return true;
        }

        rest = &rest[end..];
    }

    false
}

/// Whether a mime-type value names a PDF, ignoring parameters and ASCII case.
fn is_pdf_mime_value(value: &str) -> bool {
    value
        .split(';')
        .next()
        .map(|kind| kind.trim().eq_ignore_ascii_case(PDF_MIME_TYPE))
        .unwrap_or(false)
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

/// One `<bookmark>` element found in a history.
struct Bookmark<'a> {
    /// The opening tag's body, without the leading `<bookmark` and without the closing `>`.
    tag: &'a str,
    /// The element's own content, from just after the opening tag to before `</bookmark>`. Empty for
    /// a self-closing tag or for an element that was never closed.
    body: &'a str,
}

/// The `<bookmark>` elements in `contents`.
///
/// The scan deliberately does not validate XML: it finds the marker, ends the tag at the first `>`
/// outside quotes, and moves on. Malformed content costs the entries it contains and nothing else.
/// The element's body is taken as well, because the mime type GTK records lives inside it rather
/// than on the tag.
fn bookmarks(contents: &str) -> Vec<Bookmark<'_>> {
    const MARKER: &str = "<bookmark";

    let mut found = Vec::new();
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
        let tag = &rest[..end];
        let after_tag = &rest[end + 1..];

        // A self-closing tag has no content of its own.
        let (body, consumed) = if tag.trim_end().ends_with('/') {
            ("", 0)
        } else {
            element_body(after_tag)
        };

        found.push(Bookmark { tag, body });
        rest = &after_tag[consumed..];
    }

    found
}

/// The content of a bookmark element, plus how much of `after_tag` it consumed.
///
/// The body ends at the closing tag, and never runs past the next opening one: a bookmark that was
/// never closed must not borrow the next bookmark's mime type, which would make one document's
/// facts another document's.
fn element_body(after_tag: &str) -> (&str, usize) {
    const CLOSE: &str = "</bookmark";
    const OPEN: &str = "<bookmark";

    match (after_tag.find(CLOSE), after_tag.find(OPEN)) {
        // Truncated: no closing tag remains, so the tail is whatever this element holds.
        (None, None) => (after_tag, after_tag.len()),
        (None, Some(next)) => (&after_tag[..next], next),
        (Some(close), None) => (&after_tag[..close], close + CLOSE.len()),
        (Some(close), Some(next)) if next < close => (&after_tag[..next], next),
        (Some(close), _) => (&after_tag[..close], close + CLOSE.len()),
    }
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
    fn the_parsed_documents_include_a_bookmark_without_a_time() {
        // The map `read_at` reads keeps only paths that carry a time, and its caller only asks for a
        // date. The suggestion producer asks a different question — is this a document I should
        // offer? — so the parsed history has to answer with the path even when no moment was
        // recorded for it, and say honestly that the moment is unknown.
        let (_directory, path) = write_history(
            "<xbel>\n\
             <bookmark href=\"file:///data/dated.pdf\" \
             visited=\"2026-09-08T07:06:05.000000Z\"/>\n\
             <bookmark href=\"file:///data/undated.pdf\" added=\"2024-03-01T10:11:12.123456Z\"/>\n\
             </xbel>\n",
        );

        let history = RecentDocuments::read(&path).expect("read the fixture");
        let documents = history.documents();
        assert_eq!(documents.len(), 2, "both bookmarks are documents");
        assert_eq!(documents[0].path, Path::new("/data/dated.pdf"));
        assert_eq!(documents[0].last_used_unix, Some(1_788_851_165));
        assert_eq!(documents[1].path, Path::new("/data/undated.pdf"));
        assert_eq!(
            documents[1].last_used_unix, None,
            "unknown is None, never 1970"
        );

        // And the narrower answer is unchanged by the wider one existing.
        assert_eq!(history.read_at(Path::new("/data/undated.pdf")), None);
    }

    #[test]
    fn a_bookmark_records_whether_it_typed_the_document_as_a_pdf() {
        // GTK writes the type one level down, inside the bookmark element, which is why the parser
        // has to keep the element body and not only the opening tag. A writer that puts the same
        // name on the tag is honoured too.
        let (_directory, path) = write_history(
            "<xbel>\n\
             <bookmark href=\"file:///data/typed-by-element.bin\" visited=\"2026-09-08T07:06:05.000000Z\">\n\
             <info><metadata owner=\"http://freedesktop.org\">\n\
             <mime:mime-type type=\"application/pdf\"/>\n\
             </metadata></info>\n\
             </bookmark>\n\
             <bookmark href=\"file:///data/typed-by-attribute.pdf\" \
             mime:mime-type=\"Application/PDF; charset=binary\" \
             visited=\"2026-09-08T07:06:05.000000Z\"/>\n\
             <bookmark href=\"file:///data/a-text-file.txt\" visited=\"2026-09-08T07:06:05.000000Z\">\n\
             <info><metadata owner=\"http://freedesktop.org\">\n\
             <mime:mime-type type=\"text/plain\"/>\n\
             </metadata></info>\n\
             </bookmark>\n\
             <bookmark href=\"file:///data/untyped.pdf\" visited=\"2026-09-08T07:06:05.000000Z\"/>\n\
             </xbel>\n",
        );

        let history = RecentDocuments::read(&path).expect("read the fixture");
        let recorded: Vec<(PathBuf, bool)> = history
            .documents()
            .iter()
            .map(|entry| (entry.path.clone(), entry.recorded_as_pdf))
            .collect();

        assert_eq!(
            recorded,
            vec![
                (PathBuf::from("/data/typed-by-element.bin"), true),
                (PathBuf::from("/data/typed-by-attribute.pdf"), true),
                (PathBuf::from("/data/a-text-file.txt"), false),
                // The extension is the platform layer's half of the rule: the parser reports what
                // was recorded and nothing else.
                (PathBuf::from("/data/untyped.pdf"), false),
            ]
        );
    }

    #[test]
    fn one_path_named_twice_is_one_document_with_the_newest_time_and_the_pdf_type() {
        // The same file opened once through its mime-typed bookmark and once through a plain one is
        // still one document: two entries would make the queue offer it twice.
        let (_directory, path) = write_history(
            "<xbel>\n\
             <bookmark href=\"file:///data/twice.pdf\" visited=\"2026-09-08T07:06:05.000000Z\">\n\
             <info><metadata owner=\"http://freedesktop.org\">\n\
             <mime:mime-type type=\"application/pdf\"/>\n\
             </metadata></info>\n\
             </bookmark>\n\
             <bookmark href=\"file:///data/twice.pdf\" visited=\"2026-09-09T07:06:05.000000Z\"/>\n\
             </xbel>\n",
        );

        let history = RecentDocuments::read(&path).expect("read the fixture");
        assert_eq!(history.documents().len(), 1);
        assert_eq!(history.documents()[0].last_used_unix, Some(1_788_937_565));
        assert!(history.documents()[0].recorded_as_pdf);
        assert_eq!(history.len(), 2, "both bookmarks were parsed");
    }

    #[test]
    fn a_bookmark_that_was_never_closed_does_not_borrow_the_next_ones_type() {
        // Malformed content costs the entries it contains, and must not label one document with
        // another's recorded type: that would send the reader a `.bin` file the history only ever
        // typed on the next line.
        let (_directory, path) = write_history(
            "<xbel>\n\
             <bookmark href=\"file:///data/unclosed.bin\" visited=\"2026-09-08T07:06:05.000000Z\">\n\
             <bookmark href=\"file:///data/typed.pdf\" visited=\"2026-09-08T07:06:05.000000Z\">\n\
             <info><metadata owner=\"http://freedesktop.org\">\n\
             <mime:mime-type type=\"application/pdf\"/>\n\
             </metadata></info>\n\
             </bookmark>\n\
             </xbel>\n",
        );

        let history = RecentDocuments::read(&path).expect("read the fixture");
        let recorded: Vec<(PathBuf, bool)> = history
            .documents()
            .iter()
            .map(|entry| (entry.path.clone(), entry.recorded_as_pdf))
            .collect();

        assert_eq!(
            recorded,
            vec![
                (PathBuf::from("/data/unclosed.bin"), false),
                (PathBuf::from("/data/typed.pdf"), true),
            ]
        );
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
