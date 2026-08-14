//! cian-pty: a single PTY-backed shell session.
//!
//! Each [`PtySession`] owns a pseudo-terminal running the user's shell, a
//! background reader thread that feeds raw output into a [`vt100::Parser`], and
//! the writer end for sending keystrokes back. The UI layer locks the parser to
//! render the current screen (via tui-term) and forwards input with
//! [`PtySession::write_input`].
//!
//! Threading model: one reader thread per session pushes bytes into a
//! `Mutex<Parser>` and flips an `AtomicBool` "dirty" flag. The UI's event loop
//! checks the flag to decide when to repaint, so output appears without busy
//! polling.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::Result;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use vt100::Parser;

mod log_sink;
use log_sink::LogSink;

/// Lines of output kept above the top of the panel, to scroll back through.
///
/// cian keeps this itself rather than using vt100's own scrollback, which in
/// the version tui-term pins cannot be scrolled further back than the height
/// of the screen without panicking. Text only: the colours are lost, and what
/// was being asked for is "what did that say", not "what colour was it".
const SCROLLBACK: usize = 10_000;

/// The last `rows` display rows of `lines`, each wrapped at `cols`.
///
/// A terminal wraps a long line when it writes it, so it occupied several
/// rows; a scrollback that put it back together as one would show a different
/// screen from the one that scrolled past.
pub(crate) fn wrap_rows<'a>(
    lines: impl Iterator<Item = &'a String>,
    rows: usize,
    cols: usize,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if l.is_empty() || cols == 0 {
            out.push(String::new());
            continue;
        }
        let mut cur = String::new();
        let mut w = 0usize;
        for ch in l.chars() {
            let cw = cian_core::textops::char_cols(ch, w);
            if w + cw > cols {
                out.push(std::mem::take(&mut cur));
                w = 0;
            }
            cur.push(ch);
            w += cw;
        }
        out.push(cur);
    }
    let keep = out.len().saturating_sub(rows);
    out.split_off(keep)
}

/// What has scrolled off the top, as plain lines.
#[derive(Default)]
pub(crate) struct History {
    lines: std::collections::VecDeque<String>,
    /// The line still being written, up to the next newline.
    partial: String,
    scrub: log_sink::Scrub,
}

impl History {
    fn feed(&mut self, bytes: &[u8]) {
        let (lines, partial) = (&mut self.lines, &mut self.partial);
        self.scrub.feed(bytes, |b| {
            if b == b'\n' {
                lines.push_back(std::mem::take(partial));
                while lines.len() > SCROLLBACK {
                    lines.pop_front();
                }
            } else {
                partial.push(b as char);
            }
        });
    }
}

/// A per-session output log, shared with the reader thread. `None` when not
/// logging.
type LogSlot = Arc<Mutex<Option<LogSink>>>;

pub use cian_core::viewer::TextEncoding;

/// The encoding the reader decodes PTY output with before feeding vt100 (which
/// speaks UTF-8). Shared so a menu can change it live.
type EncSlot = Arc<Mutex<TextEncoding>>;

/// The user's preferred shell, falling back to a sane default per platform.
pub fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

/// A live shell running inside a pseudo-terminal.
pub struct PtySession {
    parser: Arc<Mutex<Parser>>,
    dirty: Arc<AtomicBool>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    rows: u16,
    cols: u16,
    /// Optional session log, shared with the reader thread.
    log: LogSlot,
    history: Arc<Mutex<History>>,
    /// Lines the view is above the live output. Zero is the end.
    scrollback: Arc<std::sync::atomic::AtomicUsize>,
    /// Input encoding, shared with the reader thread.
    encoding: EncSlot,
    // Kept so the reader thread is owned by the session; it exits on EOF when
    // the child dies (or when the session is dropped and the master closes).
    _reader: JoinHandle<()>,
}

impl PtySession {
    /// Spawn `shell` inside a fresh PTY of `rows`×`cols`, starting in `cwd`.
    pub fn new(cwd: &Path, shell: &str, rows: u16, cols: u16) -> Result<Self> {
        let rows = rows.max(1);
        let cols = cols.max(1);

        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(shell);
        cmd.cwd(cwd);
        // Advertise a capable terminal so programs emit color/cursor sequences
        // that vt100 understands.
        cmd.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(cmd)?;
        // Drop the slave handle so the master observes EOF once the child exits.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let parser = Arc::new(Mutex::new(Parser::new(rows, cols, 0)));
        let dirty = Arc::new(AtomicBool::new(true));

        let log: LogSlot = Arc::new(Mutex::new(None));
        let encoding: EncSlot = Arc::new(Mutex::new(TextEncoding::Utf8));
        let reader_parser = Arc::clone(&parser);
        let reader_dirty = Arc::clone(&dirty);
        let reader_log = Arc::clone(&log);
        let history: Arc<Mutex<History>> = Arc::new(Mutex::new(History::default()));
        let reader_hist = Arc::clone(&history);
        let reader_enc = Arc::clone(&encoding);
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF: child closed the pty
                    Ok(n) => {
                        // Decode to UTF-8 when a non-UTF-8 encoding is chosen
                        // (e.g. a Shift_JIS shell); UTF-8 passes through as-is.
                        // Per-chunk decoding can split a multi-byte character at
                        // a read boundary, a rare and harmless single glyph.
                        let enc = reader_enc.lock().map(|e| *e).unwrap_or(TextEncoding::Utf8);
                        let owned;
                        let bytes: &[u8] = if enc == TextEncoding::Utf8 {
                            &buf[..n]
                        } else {
                            owned = enc.decode(&buf[..n]).into_bytes();
                            &owned
                        };
                        // Tee to the session log first, so a scrubbed
                        // transcript captures what was shown (already UTF-8).
                        if let Ok(mut slot) = reader_log.lock() {
                            if let Some(sink) = slot.as_mut() {
                                sink.write_bytes(bytes);
                            }
                        }
                        // …and to the scrollback, which is the same text kept
                        // in memory rather than written to a file.
                        if let Ok(mut h) = reader_hist.lock() {
                            h.feed(bytes);
                        }
                        match reader_parser.lock() {
                            Ok(mut p) => p.process(bytes),
                            // A poisoned parser never recovers: this pane will
                            // stop updating for the rest of the session, which
                            // looks exactly like a hang. Record it and stop.
                            Err(_) => {
                                cian_core::log::log(
                                    "pty reader: parser mutex poisoned; pane output stops here",
                                );
                                break;
                            }
                        }
                        reader_dirty.store(true, Ordering::Relaxed);
                    }
                    Err(e) => {
                        if cian_core::log::enabled() {
                            cian_core::log::log(&format!("pty reader: read error: {}", e));
                        }
                        break;
                    }
                }
            }
            // Final repaint so the UI reflects the closed/exited state.
            reader_dirty.store(true, Ordering::Relaxed);
        });

        Ok(Self {
            parser,
            dirty,
            writer,
            master: pair.master,
            child,
            rows,
            cols,
            log,
            history,
            scrollback: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            encoding,
            _reader: reader,
        })
    }

    /// The encoding the shell output is currently decoded with.
    pub fn encoding(&self) -> TextEncoding {
        self.encoding.lock().map(|e| *e).unwrap_or(TextEncoding::Utf8)
    }

    /// Set the encoding the shell output is decoded with. Takes effect on the
    /// next output; already-drawn cells keep their glyphs until overwritten.
    pub fn set_encoding(&self, enc: TextEncoding) {
        if let Ok(mut e) = self.encoding.lock() {
            *e = enc;
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Start writing a scrubbed transcript of this session's output to `path`,
    /// replacing any log already running. The reader thread picks it up on its
    /// next read.
    pub fn start_log(&self, path: &Path) -> Result<()> {
        let sink = LogSink::create(path)?;
        if let Ok(mut slot) = self.log.lock() {
            *slot = Some(sink);
        }
        Ok(())
    }

    /// Stop logging, flushing and closing the file.
    pub fn stop_log(&self) {
        if let Ok(mut slot) = self.log.lock() {
            *slot = None;
        }
    }

    pub fn is_logging(&self) -> bool {
        self.log.lock().map(|s| s.is_some()).unwrap_or(false)
    }

    /// The path currently being logged to, if any.
    pub fn log_path(&self) -> Option<PathBuf> {
        self.log.lock().ok().and_then(|s| s.as_ref().map(|k| k.path().to_path_buf()))
    }

    /// Shared parser handle. Lock it and call `.screen()` to render.
    /// Move the view `lines` back through the scrollback (negative goes
    /// forward again) and report where it ended up. Zero is live output.
    pub fn scroll_back(&self, lines: isize) -> usize {
        let have = self.history.lock().map(|h| h.lines.len()).unwrap_or(0);
        let at = self.scrollback.load(Ordering::Relaxed) as isize;
        let to = at.saturating_add(lines).clamp(0, have as isize) as usize;
        self.scrollback.store(to, Ordering::Relaxed);
        self.dirty.store(true, Ordering::Relaxed);
        to
    }

    /// Back to live output. Every keystroke does this, as a terminal does.
    pub fn scroll_to_bottom(&self) {
        if self.scrollback.swap(0, Ordering::Relaxed) != 0 {
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    /// How far back the view is, in lines. Zero while at live output.
    pub fn scrollback_pos(&self) -> usize {
        self.scrollback.load(Ordering::Relaxed)
    }

    /// The `rows` lines to draw at the current offset, wrapped to `cols` —
    /// a terminal wrapped them when it wrote them, so a scrollback that did
    /// not would put a long line back together and change what was on screen.
    pub fn history_rows(&self, rows: usize, cols: usize) -> Vec<String> {
        let at = self.scrollback.load(Ordering::Relaxed);
        let Ok(h) = self.history.lock() else { return Vec::new() };
        let end = h.lines.len().saturating_sub(at);
        // Walk back far enough that wrapping cannot leave the page short.
        let start = end.saturating_sub(rows);
        wrap_rows(h.lines.iter().take(end).skip(start), rows, cols)
    }

    pub fn parser(&self) -> &Arc<Mutex<Parser>> {
        &self.parser
    }

    /// Return whether new output has arrived since the last call, clearing the
    /// flag. Drives the UI's "should I repaint?" decision.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }

    /// Forward raw bytes (already encoded as terminal input) to the shell.
    pub fn write_input(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Resize the PTY and the parser's screen. No-op if unchanged.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        if let Ok(mut p) = self.parser.lock() {
            p.set_size(rows, cols);
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Current PTY size (rows, cols).
    pub fn size(&self) -> (u16, u16) {
        (self.rows, self.cols)
    }

    /// Whether the shell process is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        // Terminate the shell when its tab is closed. The reader thread then
        // sees EOF and exits on its own.
        let _ = self.child.kill();
    }
}


#[cfg(test)]
mod history_tests {
    use super::*;

    /// The scrollback is the output with the escape sequences taken out —
    /// the same filter the session log uses, so a colourful `ls` reads as
    /// names rather than as a wall of `[0m`.
    #[test]
    fn the_history_keeps_the_text_and_drops_the_noise() {
        let mut h = History::default();
        h.feed(b"plain\r\n");
        h.feed(b"\x1b[31mred\x1b[0m and \x1b[1mbold\x1b[0m\r\n");
        h.feed(b"\x1b]0;a title\x07titled\r\n");
        h.feed(b"half a li");
        assert_eq!(
            h.lines.iter().cloned().collect::<Vec<_>>(),
            vec!["plain", "red and bold", "titled"],
        );
        assert_eq!(h.partial, "half a li", "the unfinished line is not a line yet");

        // It is a ring: the oldest goes when it is full.
        let mut h = History::default();
        for i in 0..SCROLLBACK + 50 {
            h.feed(format!("line {i}\n").as_bytes());
        }
        assert_eq!(h.lines.len(), SCROLLBACK);
        assert_eq!(h.lines.front().unwrap(), "line 50", "the oldest 50 have gone");
    }

    /// A long line takes the rows it took when it was written.
    #[test]
    fn a_page_of_history_is_wrapped_to_the_width() {
        let lines = ["x".repeat(25), "short".to_string(), "y".repeat(10)];
        let page = wrap_rows(lines.iter(), 10, 10);
        assert_eq!(
            page,
            vec!["xxxxxxxxxx", "xxxxxxxxxx", "xxxxx", "short", "yyyyyyyyyy"],
        );
        // …and a page is a page: the tail of it, never more.
        let page = wrap_rows(lines.iter(), 2, 10);
        assert_eq!(page, vec!["short", "yyyyyyyyyy"]);
        // A full-width character counts as two columns, as it is drawn.
        let wide = ["あいうえお".to_string()];
        assert_eq!(wrap_rows(wide.iter(), 10, 4), vec!["あい", "うえ", "お"]);
    }
}
