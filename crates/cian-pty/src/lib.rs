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

/// A per-session output log, shared with the reader thread. `None` when not
/// logging.
type LogSlot = Arc<Mutex<Option<LogSink>>>;

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
        let reader_parser = Arc::clone(&parser);
        let reader_dirty = Arc::clone(&dirty);
        let reader_log = Arc::clone(&log);
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF: child closed the pty
                    Ok(n) => {
                        // Tee to the session log first, so a scrubbed
                        // transcript captures exactly what was received.
                        if let Ok(mut slot) = reader_log.lock() {
                            if let Some(sink) = slot.as_mut() {
                                sink.write_bytes(&buf[..n]);
                            }
                        }
                        match reader_parser.lock() {
                            Ok(mut p) => p.process(&buf[..n]),
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
            _reader: reader,
        })
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
