//! Reading a file for display without leaving the file manager.
//!
//! The point of an F3 viewer is to answer "what is in here" in a second, so
//! this reads a bounded prefix rather than the whole file: opening a 4 GB log
//! to look at its first page should not cost 4 GB of memory or any noticeable
//! wait. Binary files are rendered as hex instead of as mojibake.
//!
//! The raw prefix is kept so the text can be re-decoded in another encoding on
//! demand — a Shift_JIS or UTF-16 file opened as UTF-8 is mojibake until the
//! viewer is told which encoding it really is.

use std::fs;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};

/// How much of a file is read. Generous enough that ordinary files are shown
/// whole, small enough that a huge one is still instant.
pub const VIEW_LIMIT: u64 = 4 * 1024 * 1024;

/// How much of the prefix is inspected when deciding text vs binary.
const SNIFF: usize = 8000;

/// What the viewer is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    Text,
    /// Rendered as a hex dump because it is not text.
    Binary,
}

/// Text encodings the viewer can decode. Deliberately short — the ones that
/// actually turn up on a Japanese Windows machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    ShiftJis,
    Utf16Le,
    Utf16Be,
}

impl TextEncoding {
    /// All encodings, in the order a picker offers them.
    pub const ALL: [TextEncoding; 4] = [
        TextEncoding::Utf8,
        TextEncoding::ShiftJis,
        TextEncoding::Utf16Le,
        TextEncoding::Utf16Be,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TextEncoding::Utf8 => "UTF-8",
            TextEncoding::ShiftJis => "Shift_JIS",
            TextEncoding::Utf16Le => "UTF-16LE",
            TextEncoding::Utf16Be => "UTF-16BE",
        }
    }

    /// The next encoding in a cycle, for a "switch encoding" key.
    pub fn next(self) -> Self {
        match self {
            TextEncoding::Utf8 => TextEncoding::ShiftJis,
            TextEncoding::ShiftJis => TextEncoding::Utf16Le,
            TextEncoding::Utf16Le => TextEncoding::Utf16Be,
            TextEncoding::Utf16Be => TextEncoding::Utf8,
        }
    }

    fn engine(self) -> &'static encoding_rs::Encoding {
        match self {
            TextEncoding::Utf8 => encoding_rs::UTF_8,
            TextEncoding::ShiftJis => encoding_rs::SHIFT_JIS,
            TextEncoding::Utf16Le => encoding_rs::UTF_16LE,
            TextEncoding::Utf16Be => encoding_rs::UTF_16BE,
        }
    }

    /// Decode `bytes` in this encoding. Strips a leading BOM and never fails —
    /// invalid bytes become U+FFFD, which is what a viewer (or a terminal
    /// showing a mis-encoded stream) wants.
    pub fn decode(self, bytes: &[u8]) -> String {
        self.engine().decode(bytes).0.into_owned()
    }

    /// Encode `text` back to this encoding, for saving an edited file in the
    /// encoding it was read in. UTF-16 is not one of encoding_rs' output
    /// encodings, so those two are written by hand.
    pub fn encode(self, text: &str) -> Vec<u8> {
        match self {
            TextEncoding::Utf8 => text.as_bytes().to_vec(),
            TextEncoding::ShiftJis => self.engine().encode(text).0.into_owned(),
            TextEncoding::Utf16Le => text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect(),
            TextEncoding::Utf16Be => text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct View {
    pub kind: ViewKind,
    /// Display lines, already split and expanded, in the current encoding.
    pub lines: Vec<String>,
    /// Total size of the file, which may exceed what was read.
    pub total_bytes: u64,
    /// True when the file was longer than [`VIEW_LIMIT`].
    pub truncated: bool,
    /// The encoding `lines` were decoded with.
    pub encoding: TextEncoding,
    /// The raw prefix that was read, kept so [`View::redecode`] can rebuild
    /// `lines` in a different encoding.
    bytes: Vec<u8>,
}

impl View {
    /// Build a text view from already-extracted text (e.g. an Office/PDF
    /// document decoded by [`crate::office`]). The text itself becomes the kept
    /// bytes, so the encoding switch and everything else in the viewer behave
    /// exactly as for a real text file.
    pub fn from_text(text: String, total_bytes: u64, truncated: bool) -> View {
        let bytes = text.into_bytes();
        let lines = to_lines(&String::from_utf8_lossy(&bytes));
        View { kind: ViewKind::Text, lines, total_bytes, truncated, encoding: TextEncoding::Utf8, bytes }
    }

    /// Re-decode the kept bytes as `enc`, switching to text if it was showing
    /// hex (choosing an encoding is a deliberate "read this as text").
    pub fn redecode(&mut self, enc: TextEncoding) {
        self.encoding = enc;
        self.kind = ViewKind::Text;
        self.lines = to_lines(&enc.decode(&self.bytes));
    }
}

/// Split decoded text into display lines, expanding tabs so they do not
/// collapse to one cell and misalign everything after them.
fn to_lines(text: &str) -> Vec<String> {
    text.lines().map(|l| l.replace('\t', "    ")).collect()
}

/// Guess the encoding from a byte-order mark, if present.
fn bom_encoding(bytes: &[u8]) -> Option<TextEncoding> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        Some(TextEncoding::Utf8)
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        Some(TextEncoding::Utf16Le)
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        Some(TextEncoding::Utf16Be)
    } else {
        None
    }
}

/// Read the beginning of `path` for display.
pub fn view_file(path: &Path) -> Result<View> {
    let meta = fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
    if meta.is_dir() {
        anyhow::bail!("{} is a directory", path.display());
    }
    let total_bytes = meta.len();
    let truncated = total_bytes > VIEW_LIMIT;

    let mut f = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut buf = Vec::with_capacity(total_bytes.min(VIEW_LIMIT) as usize);
    f.by_ref()
        .take(VIEW_LIMIT)
        .read_to_end(&mut buf)
        .with_context(|| format!("read {}", path.display()))?;

    // A UTF-16 BOM means text even though the bytes are full of NULs; honour it
    // before the NUL sniff writes the file off as binary.
    if let Some(enc) = bom_encoding(&buf) {
        let lines = to_lines(&enc.decode(&buf));
        return Ok(View { kind: ViewKind::Text, lines, total_bytes, truncated, encoding: enc, bytes: buf });
    }

    // A NUL in the first few KB is the usual signal; UTF-8 text does not
    // contain one, and every binary format seems to within its header.
    let binary = buf[..buf.len().min(SNIFF)].contains(&0);
    if binary {
        let lines = hex_dump(&buf);
        return Ok(View {
            kind: ViewKind::Binary,
            lines,
            total_bytes,
            truncated,
            encoding: TextEncoding::Utf8,
            bytes: buf,
        });
    }

    let lines = to_lines(&TextEncoding::Utf8.decode(&buf));
    Ok(View { kind: ViewKind::Text, lines, total_bytes, truncated, encoding: TextEncoding::Utf8, bytes: buf })
}

/// `offset  hex bytes  |ascii|`, sixteen bytes to a line.
fn hex_dump(bytes: &[u8]) -> Vec<String> {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(i, chunk)| {
            let mut hex = String::with_capacity(48);
            for (j, b) in chunk.iter().enumerate() {
                if j == 8 {
                    hex.push(' ');
                }
                hex.push_str(&format!("{:02x} ", b));
            }
            let ascii: String = chunk
                .iter()
                .map(|b| if b.is_ascii_graphic() || *b == b' ' { *b as char } else { '.' })
                .collect();
            format!("{:08x}  {:<49}|{}|", i * 16, hex, ascii)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_split_into_lines_with_tabs_expanded() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.txt");
        fs::write(&f, "one\n\ttwo\nthree\n").unwrap();
        let v = view_file(&f).unwrap();
        assert_eq!(v.kind, ViewKind::Text);
        assert_eq!(v.lines, vec!["one", "    two", "three"]);
        assert!(!v.truncated);
        assert_eq!(v.encoding, TextEncoding::Utf8);
    }

    /// Showing a compiled file as text produces a screen of mojibake that
    /// answers nothing; hex at least shows the header.
    #[test]
    fn binary_becomes_a_hex_dump() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.bin");
        fs::write(&f, b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00").unwrap();
        let v = view_file(&f).unwrap();
        assert_eq!(v.kind, ViewKind::Binary);
        assert!(v.lines[0].starts_with("00000000  7f 45 4c 46"), "got {:?}", v.lines[0]);
        assert!(v.lines[0].contains("|.ELF"), "ascii column missing: {:?}", v.lines[0]);
    }

    #[test]
    fn a_huge_file_is_read_only_up_to_the_limit() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("big.log");
        let line = "x".repeat(999);
        let mut text = String::new();
        while (text.len() as u64) < VIEW_LIMIT + 4096 {
            text.push_str(&line);
            text.push('\n');
        }
        fs::write(&f, &text).unwrap();

        let v = view_file(&f).unwrap();
        assert!(v.truncated, "should say it stopped early");
        assert!(v.total_bytes > VIEW_LIMIT);
        let read: usize = v.lines.iter().map(|l| l.len() + 1).sum();
        assert!(read as u64 <= VIEW_LIMIT + 1024, "read {} bytes", read);
    }

    #[test]
    fn invalid_utf8_is_shown_rather_than_refused() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("latin.txt");
        // Not valid UTF-8, but no NUL either: still meant to be readable.
        fs::write(&f, b"caf\xe9 au lait\n").unwrap();
        let v = view_file(&f).unwrap();
        assert_eq!(v.kind, ViewKind::Text);
        assert!(v.lines[0].contains("caf"), "got {:?}", v.lines);
    }

    #[test]
    fn an_empty_file_views_as_nothing_rather_than_failing() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("empty");
        fs::write(&f, b"").unwrap();
        let v = view_file(&f).unwrap();
        assert!(v.lines.is_empty());
        assert_eq!(v.total_bytes, 0);
    }

    #[test]
    fn a_directory_is_refused() {
        let d = tempfile::tempdir().unwrap();
        assert!(view_file(d.path()).is_err());
    }

    /// A Shift_JIS file is mojibake as UTF-8 but readable once re-decoded.
    #[test]
    fn shift_jis_can_be_re_decoded() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("sjis.txt");
        // "日本語" in Shift_JIS.
        let sjis = [0x93, 0xfa, 0x96, 0x7b, 0x8c, 0xea, b'\n'];
        fs::write(&f, sjis).unwrap();
        let mut v = view_file(&f).unwrap();
        assert_eq!(v.kind, ViewKind::Text);
        assert_ne!(v.lines[0], "日本語", "as UTF-8 it is mojibake");
        v.redecode(TextEncoding::ShiftJis);
        assert_eq!(v.lines[0], "日本語", "Shift_JIS decodes correctly");
        assert_eq!(v.encoding, TextEncoding::ShiftJis);
    }

    #[test]
    fn encode_round_trips_each_encoding() {
        // What the in-viewer editor relies on to save in the file's own encoding.
        let text = "日本語 abc\n";
        for enc in TextEncoding::ALL {
            let bytes = enc.encode(text);
            assert_eq!(enc.decode(&bytes), text, "{:?} round-trips", enc);
        }
        // Shift_JIS and UTF-16 really change the bytes (not just UTF-8).
        assert_ne!(TextEncoding::ShiftJis.encode(text), text.as_bytes());
        assert_ne!(TextEncoding::Utf16Le.encode(text), text.as_bytes());
    }

    /// A UTF-16LE file has NULs, so it is hex by default; choosing UTF-16
    /// switches it to readable text.
    #[test]
    fn utf16_without_bom_is_hex_until_chosen() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("u16.txt");
        // "Hi" in UTF-16LE, no BOM.
        fs::write(&f, [0x48, 0x00, 0x69, 0x00]).unwrap();
        let mut v = view_file(&f).unwrap();
        assert_eq!(v.kind, ViewKind::Binary, "NULs make it look binary");
        v.redecode(TextEncoding::Utf16Le);
        assert_eq!(v.kind, ViewKind::Text);
        assert_eq!(v.lines[0], "Hi");
    }

    /// A UTF-16LE BOM is recognised as text straight away.
    #[test]
    fn a_utf16_bom_is_read_as_text() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("bom.txt");
        fs::write(&f, [0xFF, 0xFE, 0x48, 0x00, 0x69, 0x00]).unwrap();
        let v = view_file(&f).unwrap();
        assert_eq!(v.kind, ViewKind::Text);
        assert_eq!(v.encoding, TextEncoding::Utf16Le);
        assert_eq!(v.lines[0], "Hi");
    }
}
