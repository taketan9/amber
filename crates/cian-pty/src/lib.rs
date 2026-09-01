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

/// The window title the shell sets, kept for the tab label.
///
/// vt100 0.16 stopped storing it on the screen and started handing it to a
/// callback instead, so cian keeps it. Shared rather than owned by the
/// parser, because the reader thread owns the parser and the UI thread is
/// what asks for the title.
#[derive(Clone, Default)]
pub struct TitleSink(Arc<Mutex<String>>);

impl vt100::Callbacks for TitleSink {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        if let Ok(mut t) = self.0.lock() {
            *t = String::from_utf8_lossy(title).into_owned();
        }
    }
}

/// The parser, with cian's callbacks attached.
pub type ShellParser = Parser<TitleSink>;

mod log_sink;
use log_sink::LogSink;

/// Rows of output kept above the top of the panel, to scroll back through.
///
/// vt100 keeps them, with their colours, exactly as they were on screen. It
/// could not before 0.16 — an offset past the height of the screen panicked
/// inside `visible_rows` — which is why cian briefly kept a plain-text ring
/// of its own instead.
const SCROLLBACK: usize = 10_000;


/// A per-session output log, shared with the reader thread. `None` when not
/// logging.
type LogSlot = Arc<Mutex<Option<LogSink>>>;

pub use cian_core::viewer::TextEncoding;

/// The encoding the reader decodes PTY output with before feeding vt100 (which
/// speaks UTF-8). Shared so a menu can change it live.
type EncSlot = Arc<Mutex<TextEncoding>>;

/// The user's preferred shell, falling back to a sane default per platform.
///
/// On Windows this is **Windows PowerShell, not `cmd.exe`**. `COMSPEC` is
/// what the platform answers when asked for "the shell", and what it names is
/// the command interpreter from 1987 — which is the shell you get when nobody
/// chose, not the shell anybody would choose. Asked for by name after the
/// first Windows session had a `cmd.exe` in the panel.
///
/// `cian.set_option("shell", …)` still wins over this, and `fallback_shells()`
/// still catches the machine where PowerShell has been removed or locked down
/// — so this is a preference, not a requirement.
pub fn default_shell() -> String {
    if cfg!(windows) {
        // The absolute path first: `powershell.exe` resolves through PATH,
        // and PATH on a managed machine is somebody else's decision.
        if let Ok(root) = std::env::var("SystemRoot") {
            let full = format!("{root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe");
            if std::path::Path::new(&full).exists() {
                return full;
            }
        }
        "powershell.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

/// Split a configured shell into the program and its arguments.
///
/// `cian.set_option("shell", …)` is written the way one would type it, and
/// "powershell.exe -NoLogo" is a perfectly ordinary way to type it. Handed
/// whole to the spawner it becomes the *filename* `powershell.exe -NoLogo`,
/// which does not exist — and the shell panel then stays empty for a reason
/// nothing on screen could explain.
///
/// Double quotes group, because a Windows path with a space in it has to be
/// writable: `"C:\Program Files\PowerShell\7\pwsh.exe" -NoLogo`.
pub fn split_command(spec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in spec.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut current));
                    any = false;
                }
            }
            c => {
                current.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(current);
    }
    out
}

/// The shells to try, in order, when the configured one cannot be started.
///
/// A shell panel that stays empty is the least useful thing cian can do. If
/// what was asked for will not start — a PowerShell that is not installed, a
/// path that moved, a name with a typo in it — the next best thing is a shell,
/// plus a note saying which one and why.
pub fn fallback_shells() -> Vec<String> {
    if cfg!(windows) {
        // PowerShell first, then its absolute path, and only then the command
        // interpreter. `COMSPEC` used to be inserted at the front, which made
        // the *fallback* order disagree with `default_shell()`'s preference:
        // a PowerShell that failed to start dropped straight to `cmd.exe`
        // without trying the copy that is always at the same address.
        let mut out = vec!["powershell.exe".to_string()];
        if let Ok(root) = std::env::var("SystemRoot") {
            out.push(format!("{root}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"));
        }
        if let Ok(comspec) = std::env::var("COMSPEC") {
            out.push(comspec);
        }
        out.push("cmd.exe".to_string());
        out
    } else {
        vec!["/bin/sh".to_string()]
    }
}

/// A live shell running inside a pseudo-terminal.
pub struct PtySession {
    parser: Arc<Mutex<ShellParser>>,
    dirty: Arc<AtomicBool>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    rows: u16,
    cols: u16,
    /// When the shell was started, so a panel that is still blank can say how
    /// long it has been blank for.
    started: std::time::Instant,
    /// Whether the shell's exit has already been written to the log.
    reported_exit: bool,
    /// Optional session log, shared with the reader thread.
    log: LogSlot,
    title: TitleSink,
    /// Input encoding, shared with the reader thread.
    encoding: EncSlot,
    // Kept so the reader thread is owned by the session; it exits on EOF when
    // the child dies (or when the session is dropped and the master closes).
    _reader: JoinHandle<()>,
}

impl PtySession {
    /// Spawn `shell` in a PTY, falling back to something that will start.
    ///
    /// Returns the session and, when the configured shell was not the one that
    /// started, a note for the caller to show. A panel with `cmd.exe` in it and
    /// a line saying why beats an empty panel every time.
    pub fn start(
        cwd: &Path,
        shell: &str,
        rows: u16,
        cols: u16,
    ) -> Result<(Self, Option<String>)> {
        let first = match Self::new(cwd, shell, rows, cols) {
            Ok(s) => return Ok((s, None)),
            Err(e) => e,
        };
        let asked = split_command(shell).first().cloned().unwrap_or_default();
        for candidate in fallback_shells() {
            if candidate == shell || candidate == asked {
                continue;
            }
            if let Ok(s) = Self::new(cwd, &candidate, rows, cols) {
                return Ok((
                    s,
                    Some(format!("{shell} が起動できないので {candidate} を使います（{first}）")),
                ));
            }
        }
        Err(first)
    }

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

        // Split rather than handed over whole: see [`split_command`].
        let argv = split_command(shell);
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| anyhow::anyhow!("no shell configured"))?;
        let mut cmd = CommandBuilder::new(program);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.cwd(cwd);
        // Advertise a capable terminal so programs emit color/cursor sequences
        // that vt100 understands.
        cmd.env("TERM", "xterm-256color");
        // And a UTF-8 locale, when the process has none to pass on.
        //
        // A terminal always has one, so the terminal build inherited it and
        // never had to think about this. **A window started from Finder or
        // Explorer has none**, and without it zsh's line editor treats every
        // byte over 0x7f as unprintable: typing 日本語 at the prompt showed
        // `<0083><0086>…` while the command's own output came out perfectly,
        // which reads as a font problem and is not one.
        //
        // Not on Windows, where the console works from a code page and these
        // variables mean nothing.
        #[cfg(not(windows))]
        if std::env::var_os("LC_ALL").is_none() && std::env::var_os("LANG").is_none() {
            cmd.env("LANG", "en_US.UTF-8");
            cmd.env("LC_CTYPE", "en_US.UTF-8");
        }

        let child = pair.slave.spawn_command(cmd)?;
        cian_core::log::log(&format!(
            "pty: {program:?} started in a {cols}x{rows} pty at {}",
            cwd.display(),
        ));
        // Drop the slave handle so the master observes EOF once the child exits.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let title = TitleSink::default();
        let parser = Arc::new(Mutex::new(ShellParser::new_with_callbacks(
            rows,
            cols,
            SCROLLBACK,
            title.clone(),
        )));
        let dirty = Arc::new(AtomicBool::new(true));

        let log: LogSlot = Arc::new(Mutex::new(None));
        let encoding: EncSlot = Arc::new(Mutex::new(TextEncoding::Utf8));
        let reader_parser = Arc::clone(&parser);
        let reader_dirty = Arc::clone(&dirty);
        let reader_log = Arc::clone(&log);
        let reader_enc = Arc::clone(&encoding);
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            // Whether anything has been heard from the shell yet.
            //
            // "It spawned and the panel stayed empty" is a report that cannot
            // be acted on: a shell that never starts, one that starts and says
            // nothing, and one whose output cian fails to display all look the
            // same from the outside. The first thing it says is logged, with
            // how long it took to say it — a corporate PowerShell profile can
            // take half a minute, and that is a different problem from silence.
            let mut heard = false;
            let started = std::time::Instant::now();
            let mut last_told = started;
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        cian_core::log::log(&format!(
                            "pty reader: end of output after {:?}{}",
                            started.elapsed(),
                            if heard { "" } else { ", having heard nothing at all" },
                        ));
                        break; // EOF: child closed the pty
                    }
                    Ok(n) => {
                        // Every arrival is worth a line, but not more than one
                        // a second: what a diagnosis needs is *when* the shell
                        // spoke and roughly what it said, not a transcript. A
                        // prompt that turns up forty seconds in is the whole
                        // answer to "why is the panel empty", and the first
                        // sixteen bytes alone could never show it.
                        let now = std::time::Instant::now();
                        let due = !heard
                            || now.duration_since(last_told) >= std::time::Duration::from_secs(1);
                        if due && cian_core::log::enabled() {
                            last_told = now;
                            let taste: String = String::from_utf8_lossy(&buf[..n.min(48)])
                                .chars()
                                .map(|c| if c.is_control() { '.' } else { c })
                                .collect();
                            cian_core::log::log(&format!(
                                "pty reader: {n} bytes at {:?}: {taste}",
                                started.elapsed(),
                            ));
                        }
                        heard = true;
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
            started: std::time::Instant::now(),
            reported_exit: false,
            log,
            title,
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
    ///
    /// The offset is measured from the bottom, so while a command is still
    /// producing output the view drifts with it, as it would in a terminal
    /// that scrolls under you. Reading back over something that has finished
    /// — what this is for — holds still.
    pub fn scroll_back(&self, lines: isize) -> usize {
        let Ok(mut p) = self.parser.lock() else { return 0 };
        let at = p.screen().scrollback() as isize;
        let to = at.saturating_add(lines).clamp(0, SCROLLBACK as isize) as usize;
        p.screen_mut().set_scrollback(to);
        self.dirty.store(true, Ordering::Relaxed);
        // It clamps to what there actually is, so ask rather than assume: a
        // fresh shell has nothing to go back through.
        p.screen().scrollback()
    }

    /// Back to live output. `write_input` calls it, so every keystroke does
    /// this, as a terminal does — which this comment claimed for months while
    /// nothing called it.
    pub fn scroll_to_bottom(&self) {
        if let Ok(mut p) = self.parser.lock() {
            if p.screen().scrollback() != 0 {
                p.screen_mut().set_scrollback(0);
                self.dirty.store(true, Ordering::Relaxed);
            }
        }
    }

    /// How far back the view is, in rows. Zero while at live output.
    pub fn scrollback_pos(&self) -> usize {
        self.parser.lock().map(|p| p.screen().scrollback()).unwrap_or(0)
    }

    /// The window title the shell last set, if any.
    pub fn window_title(&self) -> String {
        self.title.0.lock().map(|t| t.clone()).unwrap_or_default()
    }

    pub fn parser(&self) -> &Arc<Mutex<ShellParser>> {
        &self.parser
    }

    /// Return whether new output has arrived since the last call, clearing the
    /// flag. Drives the UI's "should I repaint?" decision.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Relaxed)
    }

    /// Forward raw bytes (already encoded as terminal input) to the shell.
    pub fn write_input(&mut self, bytes: &[u8]) {
        // **Typing returns you to live output.** The comment on
        // `scroll_to_bottom` has said "every keystroke does this, as a
        // terminal does" since it was written, and nothing did it: the wheel
        // over the panel walks the scrollback, and once back there the view
        // stayed there for ever.
        //
        // That is worse than it sounds, because `vt100` answers the two
        // halves of the question from different places — `cell()` gives the
        // scrolled-back grid and `cursor_position()` gives the live cursor.
        // So the panel showed old text with a cursor blinking and *moving* in
        // it: a shell that looks broken rather than scrolled, which is
        // exactly how it was reported ("文字を入力しても記載されている文字が
        // かわらない。ただ、ぴこぴこしているカーソル位置は変わっている").
        //
        // Here rather than in the one caller, because every write is somebody
        // typing or a program answering a prompt, and both mean "look at what
        // is happening now".
        self.scroll_to_bottom();
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
            p.screen_mut().set_size(rows, cols);
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Current PTY size (rows, cols).
    pub fn size(&self) -> (u16, u16) {
        (self.rows, self.cols)
    }

    /// Whether the shell process is still running.
    /// End this shell now, without waiting for it.
    ///
    /// Closing a pseudo-console on Windows *waits for the program inside it to
    /// exit*, and a shell that has wedged — one stuck loading a profile, say —
    /// never does. That wait happens inside `drop`, which is to say while cian
    /// is trying to close its window: the window stops answering, and the only
    /// way out is the task manager.
    ///
    /// So the child is killed first, deliberately and early, and the tidying up
    /// that follows has nothing left to wait for.
    pub fn kill_now(&mut self) {
        let _ = self.child.kill();
    }

    /// How long since this shell was started.
    pub fn age(&self) -> std::time::Duration {
        self.started.elapsed()
    }

    /// Is there nothing on the screen at all?
    ///
    /// Not the same question as [`has_spoken`](Self::has_spoken): a shell can
    /// speak without saying anything visible. What arrives first from ConPTY is
    /// the terminal-mode sequences it sets for itself, and a shell that stops
    /// there — no banner, no prompt — has started and then gone away to do
    /// something. On Windows that something is usually a profile script, and on
    /// a machine whose home directory is OneDrive's, a profile script can take
    /// a very long time.
    pub fn screen_is_blank(&self) -> bool {
        match self.parser.lock() {
            Ok(p) => p.screen().contents().trim().is_empty(),
            Err(_) => false,
        }
    }

    pub fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            // Still running.
            Ok(None) => true,
            // Finished, with a status. The panel closes on this — and the
            // status is said once, because "the shell went away" and "the shell
            // went away with code 1" are different problems and only one of
            // them is cian's.
            Ok(Some(status)) => {
                if !self.reported_exit {
                    self.reported_exit = true;
                    cian_core::log::log(&format!(
                        "pty: the shell exited after {:?} with {status:?}",
                        self.started.elapsed(),
                    ));
                }
                false
            }
            // Asked and could not be told. "I do not know" is not "it exited",
            // and treating it as one closes the panel the moment after it
            // opened — which looks exactly like a shell that would not start.
            Err(_) => true,
        }
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
mod tests {
    use super::*;

    /// `cian.set_option("shell", …)` is written the way one would type it.
    /// Handed to the spawner whole, "powershell.exe -NoLogo" is the *name of a
    /// file* — one that does not exist — and the panel stays empty.
    #[test]
    fn a_shell_with_arguments_is_split_into_program_and_arguments() {
        assert_eq!(split_command("powershell.exe -NoLogo"), ["powershell.exe", "-NoLogo"]);
        assert_eq!(split_command("pwsh"), ["pwsh"]);
        assert_eq!(split_command("  /bin/zsh   -l  "), ["/bin/zsh", "-l"]);
    }

    /// A Windows path has spaces in it, and has to be writable.
    #[test]
    fn quotes_group_a_path_with_spaces() {
        assert_eq!(
            split_command("\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\" -NoLogo"),
            ["C:\\Program Files\\PowerShell\\7\\pwsh.exe", "-NoLogo"]
        );
    }

    #[test]
    fn nothing_configured_is_no_arguments_rather_than_one_empty_one() {
        assert!(split_command("").is_empty());
        assert!(split_command("   ").is_empty());
    }

    /// Something has to start. The ladder ends somewhere that exists on a bare
    /// install of the platform.
    #[test]
    fn the_fallbacks_end_somewhere_that_exists() {
        let last = fallback_shells();
        assert!(!last.is_empty());
        if cfg!(windows) {
            assert!(last.iter().any(|s| s.to_lowercase().contains("cmd.exe")));
            assert!(last.iter().any(|s| s.to_lowercase().contains("powershell")));
        } else {
            assert!(last.iter().any(|s| s == "/bin/sh"));
        }
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;

    /// Typing while scrolled back must bring the view down.
    ///
    /// Without it the panel shows the old grid with a cursor blinking and
    /// *moving* in it, because `vt100` answers `cell()` from the scrollback
    /// and `cursor_position()` from the live screen. That reads as a frozen
    /// shell, not a scrolled one — which is how it was reported.
    #[test]
    fn writing_returns_the_view_to_the_bottom() {
        let cwd = std::env::temp_dir();
        let Ok((session, _note)) = PtySession::start(&cwd, &default_shell(), 24, 80) else {
            // No pty on this runner: the assertion below has nothing to say.
            return;
        };
        let session = std::sync::Arc::new(std::sync::Mutex::new(session));
        // Enough output to have something to scroll back through.
        {
            let mut s = session.lock().unwrap();
            for _ in 0..80 {
                s.write_input(b"echo cian-scroll-test\n");
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(900));
        {
            let s = session.lock().unwrap();
            s.scroll_back(20);
        }
        let back = session.lock().unwrap().scrollback_pos();
        if back == 0 {
            // The shell produced less than a screen; nothing was scrolled and
            // there is nothing to assert about coming back.
            return;
        }
        session.lock().unwrap().write_input(b"\n");
        assert_eq!(session.lock().unwrap().scrollback_pos(), 0,
                   "a keystroke must return the view to live output");
    }
}
