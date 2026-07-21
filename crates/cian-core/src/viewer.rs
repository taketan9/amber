//! Reading a file for display without leaving the file manager.
//!
//! The point of an F3 viewer is to answer "what is in here" in a second, so
//! this reads a bounded prefix rather than the whole file: opening a 4 GB log
//! to look at its first page should not cost 4 GB of memory or any noticeable
//! wait. Binary files are rendered as hex instead of as mojibake.

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

#[derive(Debug, Clone)]
pub struct View {
    pub kind: ViewKind,
    /// Display lines, already split and expanded.
    pub lines: Vec<String>,
    /// Total size of the file, which may exceed what was read.
    pub total_bytes: u64,
    /// True when the file was longer than [`VIEW_LIMIT`].
    pub truncated: bool,
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

    // A NUL in the first few KB is the usual signal; UTF-8 text does not
    // contain one, and every binary format seems to within its header.
    let binary = buf[..buf.len().min(SNIFF)].contains(&0);
    if binary {
        return Ok(View { kind: ViewKind::Binary, lines: hex_dump(&buf), total_bytes, truncated });
    }

    let text = String::from_utf8_lossy(&buf);
    let lines = text
        .lines()
        // Tabs are common in source and would otherwise render as a single
        // cell, misaligning everything after them.
        .map(|l| l.replace('\t', "    "))
        .collect();
    Ok(View { kind: ViewKind::Text, lines, total_bytes, truncated })
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
}
