//! A shell, in the engine.
//!
//! **Not in the window.** Electron's usual answer is `node-pty`: a native
//! module that wants a C++ toolchain and a rebuild against Electron's own ABI
//! — the same several gigabytes this project already refused once, for SFTP.
//! `cian-pty` is `portable-pty` and `vt100`, both plain Rust, and it is the
//! very emulator the terminal build reads its shell through. So there is one
//! emulator rather than two, and nothing to compile on the machine that runs
//! the GUI.
//!
//! The window sends keystrokes and draws a grid. It does not know what an
//! escape sequence is, which is right: interpreting them is a job with twenty
//! years of edge cases in it, and having a second answer to any of them is how
//! two front ends stop looking like one program.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cian_pty::PtySession;

use crate::wire::Out;

/// How often the screen is looked at.
///
/// A shell is the one thing here that changes without being asked, so it is
/// polled — but only the *dirty* flag is polled. Serialising a grid nobody
/// changed is the wasteful part, not the looking.
const TICK_MS: u64 = 30;

pub struct Shell {
    session: Arc<Mutex<PtySession>>,
    stop: Arc<AtomicBool>,
    pub rows: u16,
    pub cols: u16,
}

impl Shell {
    pub fn start(cwd: &std::path::Path, rows: u16, cols: u16, out: Out) -> anyhow::Result<Self> {
        let shell = cian_pty::default_shell();
        // `start` rather than `new`: it falls back to something that will run
        // and says which. A panel with cmd.exe in it and a line saying why
        // beats an empty panel every time — which is exactly what `new` alone
        // produced the first time this was tried.
        let (session, note) = PtySession::start(cwd, &shell, rows, cols)?;
        if let Some(note) = note {
            out.event("shellnote", serde_json::json!({ "note": note }));
        }
        let session = Arc::new(Mutex::new(session));
        let stop = Arc::new(AtomicBool::new(false));

        let watch = Arc::clone(&session);
        let watch_stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            while !watch_stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(TICK_MS));
                let dirty = watch.lock().map(|s| s.take_dirty()).unwrap_or(false);
                if !dirty {
                    continue;
                }
                let Ok(s) = watch.lock() else { continue };
                if let Some(grid) = render(&s) {
                    out.event("shell", grid);
                }
            }
            // Say so once it has gone, so the window can take the panel down
            // rather than leaving a frozen screen that looks alive.
            out.event("shellgone", serde_json::json!({}));
        });

        Ok(Shell { session, stop, rows, cols })
    }

    pub fn write(&self, bytes: &[u8]) {
        if let Ok(mut s) = self.session.lock() {
            s.write_input(bytes);
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.rows = rows;
        self.cols = cols;
        if let Ok(mut s) = self.session.lock() {
            s.resize(rows, cols);
        }
    }

    pub fn scroll(&self, lines: isize) {
        if let Ok(s) = self.session.lock() {
            s.scroll_back(lines);
        }
    }

    pub fn to_bottom(&self) {
        if let Ok(s) = self.session.lock() {
            s.scroll_to_bottom();
        }
    }

    /// The screen as it stands, for a window that has just opened the panel and
    /// has nothing to draw yet.
    pub fn screen(&self) -> Option<serde_json::Value> {
        self.session.lock().ok().and_then(|s| render(&s))
    }

    pub fn alive(&self) -> bool {
        self.session.lock().map(|mut s| s.is_alive()).unwrap_or(false)
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut s) = self.session.lock() {
            s.kill_now();
        }
    }
}

/// The screen, as runs of same-looking cells.
///
/// Per cell would be honest and enormous: eighty columns of separate objects,
/// thirty times a second. A run holds while the colours and the emphasis hold,
/// which on a real shell screen means a line is usually one or two of them.
fn render(session: &PtySession) -> Option<serde_json::Value> {
    let parser = session.parser().lock().ok()?;
    let screen = parser.screen();
    let (rows, cols) = screen.size();
    let mut lines = Vec::with_capacity(rows as usize);

    for row in 0..rows {
        let mut runs: Vec<serde_json::Value> = Vec::new();
        let mut text = String::new();
        let mut style: Option<Style> = None;
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else { continue };
            // A wide character occupies two columns and reports its text in
            // the first; the second is a continuation and must not be drawn
            // again or every 全角 character doubles.
            if cell.is_wide_continuation() {
                continue;
            }
            let here = Style::of(cell);
            if style.as_ref() != Some(&here) {
                if let Some(was) = style.take() {
                    runs.push(was.json(&text));
                }
                text.clear();
                style = Some(here);
            }
            let c = cell.contents();
            text.push_str(if c.is_empty() { " " } else { c });
        }
        if let Some(was) = style {
            runs.push(was.json(&text));
        }
        lines.push(serde_json::Value::Array(runs));
    }

    let (cr, cc) = screen.cursor_position();
    Some(serde_json::json!({
        "rows": rows,
        "cols": cols,
        "cursor": { "row": cr, "col": cc },
        "hidden": screen.hide_cursor(),
        "scrollback": screen.scrollback(),
        "title": session.window_title(),
        "lines": lines,
    }))
}

/// What a run of cells looks like.
#[derive(PartialEq, Eq)]
struct Style {
    fg: Option<String>,
    bg: Option<String>,
    bold: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

impl Style {
    fn of(cell: &vt100::Cell) -> Style {
        Style {
            fg: colour(cell.fgcolor()),
            bg: colour(cell.bgcolor()),
            bold: cell.bold(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
        }
    }

    fn json(&self, text: &str) -> serde_json::Value {
        let mut o = serde_json::Map::new();
        o.insert("t".into(), serde_json::Value::String(text.to_string()));
        if let Some(f) = &self.fg {
            o.insert("f".into(), serde_json::Value::String(f.clone()));
        }
        if let Some(b) = &self.bg {
            o.insert("b".into(), serde_json::Value::String(b.clone()));
        }
        for (k, v) in [
            ("bold", self.bold),
            ("it", self.italic),
            ("ul", self.underline),
            ("inv", self.inverse),
        ] {
            if v {
                o.insert(k.into(), serde_json::Value::Bool(true));
            }
        }
        serde_json::Value::Object(o)
    }
}

/// A vt100 colour, as something CSS can use.
///
/// The sixteen named ones stay named — they are the palette the window's theme
/// gets to choose, and freezing them to hex here would mean a shell that
/// ignores 白磁 and 陰翳 alike. Only the 256-colour and true-colour cases,
/// which a program picked deliberately, become fixed values.
fn colour(c: vt100::Color) -> Option<String> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) if i < 16 => Some(format!("c{i}")),
        vt100::Color::Idx(i) => {
            let (r, g, b) = xterm256(i);
            Some(format!("#{r:02x}{g:02x}{b:02x}"))
        }
        vt100::Color::Rgb(r, g, b) => Some(format!("#{r:02x}{g:02x}{b:02x}")),
    }
}

/// The xterm 256-colour cube and its grey ramp, which are arithmetic rather
/// than a table anybody needs to read.
fn xterm256(i: u8) -> (u8, u8, u8) {
    if i < 232 {
        let i = i - 16;
        let step = |n: u8| if n == 0 { 0 } else { 55 + n * 40 };
        (step(i / 36), step((i / 6) % 6), step(i % 6))
    } else {
        let v = 8 + (i - 232) * 10;
        (v, v, v)
    }
}
