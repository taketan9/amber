//! A readable transcript of a shell session.
//!
//! The PTY carries the raw byte stream a terminal would render: text
//! interleaved with escape sequences that move the cursor, set colors, retitle
//! the window and so on. Written to a file verbatim that is unreadable. This
//! strips the escape sequences and keeps the text, the way TeraTerm's
//! plain-text log does — a faithful-enough record of what scrolled past for
//! reviewing a session afterwards.
//!
//! It is a streaming filter: escape sequences can straddle two reads, so the
//! partial-sequence state is carried between calls.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Where in an escape sequence the filter currently is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Ordinary text.
    Text,
    /// Just saw ESC; the next byte decides the kind of sequence.
    Escape,
    /// Inside `ESC [ … final` (a CSI sequence); ends on a byte in `0x40..=0x7e`.
    Csi,
    /// Inside `ESC ] … (BEL | ST)` (an OSC string, e.g. a window title).
    Osc,
    /// Saw ESC inside an OSC while looking for the `ESC \` string terminator.
    OscEsc,
}

/// An open log that scrubs escape sequences as bytes arrive.
pub struct LogSink {
    path: PathBuf,
    out: BufWriter<File>,
    state: State,
}

impl LogSink {
    /// Create (or truncate) the log file at `path`.
    pub fn create(path: &Path) -> Result<Self> {
        let file = File::create(path).with_context(|| format!("open log {}", path.display()))?;
        Ok(Self { path: path.to_path_buf(), out: BufWriter::new(file), state: State::Text })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Feed a chunk of raw PTY output, appending its readable text.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.step(b);
        }
        // Flush per chunk: a session log wants to survive a crash of the very
        // process it is recording, and terminal output is not high-volume
        // enough for the syscalls to matter.
        let _ = self.out.flush();
    }

    fn step(&mut self, b: u8) {
        match self.state {
            State::Text => match b {
                0x1b => self.state = State::Escape,
                b'\n' => self.emit(b'\n'),
                b'\t' => self.emit(b'\t'),
                // Drop CR so CRLF becomes LF; drop other C0 controls (BEL,
                // backspace and friends produce noise, not text).
                b'\r' => {}
                0x00..=0x1f => {}
                other => self.emit(other),
            },
            State::Escape => match b {
                b'[' => self.state = State::Csi,
                b']' => self.state = State::Osc,
                // A two-byte escape (charset selection and the like): the byte
                // after ESC is the whole thing, so we are done with it.
                _ => self.state = State::Text,
            },
            State::Csi => {
                // Parameter and intermediate bytes continue the sequence; a
                // final byte (0x40..=0x7e) ends it.
                if (0x40..=0x7e).contains(&b) {
                    self.state = State::Text;
                }
            }
            State::Osc => match b {
                0x07 => self.state = State::Text,      // BEL terminates
                0x1b => self.state = State::OscEsc,    // maybe ESC \ (ST)
                _ => {}
            },
            State::OscEsc => {
                // `ESC \` is the string terminator; anything else was a stray
                // ESC, and we resume scanning the OSC.
                self.state = if b == b'\\' { State::Text } else { State::Osc };
            }
        }
    }

    fn emit(&mut self, b: u8) {
        let _ = self.out.write_all(&[b]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn scrub(input: &[u8]) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.log");
        {
            let mut s = LogSink::create(&path).unwrap();
            s.write_bytes(input);
        }
        let mut out = String::new();
        File::open(&path).unwrap().read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(scrub(b"hello world\n"), "hello world\n");
    }

    #[test]
    fn csi_color_sequences_are_stripped() {
        // `ESC[31mred ESC[0m` around the word.
        assert_eq!(scrub(b"\x1b[31mred\x1b[0m done\n"), "red done\n");
    }

    #[test]
    fn an_osc_title_is_stripped() {
        // `ESC]0;user@host: ~ BEL` — a window-title set, then text.
        assert_eq!(scrub(b"\x1b]0;user@host: ~\x07$ ls\n"), "$ ls\n");
        // OSC ended with the ST form (ESC \) instead of BEL.
        assert_eq!(scrub(b"\x1b]0;title\x1b\\next\n"), "next\n");
    }

    #[test]
    fn cr_is_dropped_so_crlf_becomes_lf() {
        assert_eq!(scrub(b"line one\r\nline two\r\n"), "line one\nline two\n");
    }

    #[test]
    fn a_sequence_split_across_two_writes_is_still_stripped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.log");
        {
            let mut s = LogSink::create(&path).unwrap();
            s.write_bytes(b"a\x1b[3"); // CSI started but not finished
            s.write_bytes(b"1mb\n"); // finishes the CSI, then text
        }
        let mut out = String::new();
        File::open(&path).unwrap().read_to_string(&mut out).unwrap();
        assert_eq!(out, "ab\n");
    }

    #[test]
    fn tabs_survive_but_other_controls_do_not() {
        assert_eq!(scrub(b"a\tb\x07c\x08\n"), "a\tbc\n");
    }
}
