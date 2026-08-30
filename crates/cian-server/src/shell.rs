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
    /// Which shell this is, carried on every screen it sends.
    ///
    /// A hidden tab keeps running — that is the reason to have tabs — and it
    /// keeps producing screens. Without a name on them the window would draw
    /// whichever one moved last, and a build scrolling in tab two would keep
    /// stamping itself over tab one.
    pub id: u64,
    pub rows: u16,
    pub cols: u16,
}

impl Shell {
    pub fn start(
        id: u64,
        cwd: &std::path::Path,
        rows: u16,
        cols: u16,
        out: Out,
    ) -> anyhow::Result<Self> {
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
            let mut ticks: u32 = 0;
            while !watch_stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(TICK_MS));
                // Has the shell exited on its own — `exit`, Ctrl+D, a crash?
                // Nothing used to ask, so a shell the person closed by typing
                // `exit` left a frozen screen that looked alive. Asked twice a
                // second, which is often enough to feel immediate and rare
                // enough not to matter.
                ticks += 1;
                if ticks % 8 == 0 && !watch.lock().map(|mut s| s.is_alive()).unwrap_or(false) {
                    // A pane whose shell has ended, not a panel to take down:
                    // exactly what Shift+F10 does, which is why one word says
                    // both. The old event said "gone" and the window heard
                    // "the shell is over", so closing one split pane closed
                    // every other pane with it.
                    out.event("shellexit", serde_json::json!({ "id": id }));
                    return;
                }
                let dirty = watch.lock().map(|s| s.take_dirty()).unwrap_or(false);
                if !dirty {
                    continue;
                }
                let Ok(s) = watch.lock() else { continue };
                if let Some(mut grid) = render(&s) {
                    grid["id"] = serde_json::json!(id);
                    out.event("shell", grid);
                }
            }
            // Stopped because the Shell was dropped — the engine already knows,
            // having done it. Saying so would be the engine telling the window
            // about the window's own request.
        });

        Ok(Shell { session, stop, id, rows, cols })
    }

    /// A handle a worker can hold: writing and waiting, without the Shell
    /// itself (which lives in the session and cannot cross a thread).
    pub fn handle(&self) -> Handle {
        Handle { session: Arc::clone(&self.session) }
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
        self.session.lock().ok().and_then(|s| render(&s)).map(|mut g| {
            g["id"] = serde_json::json!(self.id);
            g
        })
    }

    /// The visible screen as plain text, for the AI to read.
    pub fn contents(&self) -> Option<String> {
        self.session
            .lock()
            .ok()
            .and_then(|s| s.parser().lock().ok().map(|p| p.screen().contents()))
    }

    pub fn is_logging(&self) -> bool {
        self.session.lock().map(|s| s.is_logging()).unwrap_or(false)
    }

    pub fn log_path(&self) -> Option<std::path::PathBuf> {
        self.session.lock().ok().and_then(|s| s.log_path())
    }

    pub fn start_log(&self, at: &std::path::Path) -> anyhow::Result<()> {
        self.session
            .lock()
            .map_err(|_| anyhow::anyhow!("シェルに触れません"))?
            .start_log(at)
    }

    pub fn stop_log(&self) {
        if let Ok(s) = self.session.lock() {
            s.stop_log();
        }
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

/// What a macro's worker holds while it types into a shell it does not own.
pub struct Handle {
    session: Arc<Mutex<PtySession>>,
}

impl Handle {
    pub fn write(&self, bytes: &[u8]) {
        if let Ok(mut s) = self.session.lock() {
            s.write_input(bytes);
        }
    }

    /// Wait until `text` shows up on the screen, or `secs` pass.
    ///
    /// What makes a login macro possible: `sqlplus /nolog` is not ready when
    /// the process starts, it is ready when it says so. Case-insensitive,
    /// because a prompt's capitalisation is not something anyone should have
    /// to get right in a config file.
    pub fn wait_for(&self, text: &str, secs: f64) {
        let want = text.to_lowercase();
        let until = std::time::Instant::now() + std::time::Duration::from_secs_f64(secs);
        while std::time::Instant::now() < until {
            let seen = self
                .session
                .lock()
                .ok()
                .and_then(|s| s.parser().lock().ok().map(|p| p.screen().contents()))
                .map(|c| c.to_lowercase().contains(&want))
                .unwrap_or(false);
            if seen {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
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

/// How a tab's shells are arranged.
///
/// A tree rather than a list, because that is what splitting *is*: every split
/// takes one pane and makes it two, and the pane it takes might itself be half
/// of an earlier split. A flat list of panes with one direction covers "two
/// side by side" and falls apart the moment somebody splits the right-hand one
/// downwards — which is the second thing anybody does.
pub enum Node {
    Leaf(u64),
    Split {
        down: bool,
        /// What fraction the first half keeps, 0.05–0.95.
        ratio: f32,
        a: Box<Node>,
        b: Box<Node>,
    },
}

impl Node {
    /// Replace the leaf `id` with a split holding it and `fresh`.
    pub fn split_at(&mut self, id: u64, fresh: u64, down: bool) -> bool {
        match self {
            Node::Leaf(mine) if *mine == id => {
                *self = Node::Split {
                    down,
                    ratio: 0.5,
                    a: Box::new(Node::Leaf(id)),
                    b: Box::new(Node::Leaf(fresh)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { a, b, .. } => a.split_at(id, fresh, down) || b.split_at(id, fresh, down),
        }
    }

    /// Remove the leaf `id`, collapsing the split it was half of.
    ///
    /// Returns false when `id` is the only leaf: a tab with no panes is not a
    /// tab, and closing the last pane means closing the tab.
    pub fn close(&mut self, id: u64) -> bool {
        let replacement = match self {
            Node::Leaf(_) => return false,
            Node::Split { a, b, .. } => match (a.as_ref(), b.as_ref()) {
                (Node::Leaf(x), _) if *x == id => Some(std::mem::replace(b.as_mut(), Node::Leaf(0))),
                (_, Node::Leaf(y)) if *y == id => Some(std::mem::replace(a.as_mut(), Node::Leaf(0))),
                _ => None,
            },
        };
        if let Some(kept) = replacement {
            *self = kept;
            return true;
        }
        match self {
            Node::Split { a, b, .. } => a.close(id) || b.close(id),
            Node::Leaf(_) => false,
        }
    }

    /// Nudge the split that `id` sits under, in the given direction.
    ///
    /// The *nearest* split of the right orientation, walking out from the
    /// leaf: dragging a border means moving the one you can see, and the one
    /// you can see is the innermost that runs the right way.
    pub fn resize(&mut self, id: u64, wider: bool, down_axis: bool) -> bool {
        let Node::Split { down, ratio, a, b } = self else { return false };
        // Inner splits first: the border you can see is the innermost one.
        if a.resize(id, wider, down_axis) || b.resize(id, wider, down_axis) {
            return true;
        }
        if *down != down_axis {
            return false;
        }
        let mut mine = Vec::new();
        a.leaves(&mut mine);
        let mut theirs = Vec::new();
        b.leaves(&mut theirs);
        let first = if mine.contains(&id) {
            true
        } else if theirs.contains(&id) {
            false
        } else {
            return false;
        };
        // Which half the leaf is in decides which way "wider" goes: widening
        // the right-hand pane means giving the first half less, not more.
        let step = if first == wider { 0.05 } else { -0.05 };
        *ratio = (*ratio + step).clamp(0.1, 0.9);
        true
    }

    pub fn leaves(&self, out: &mut Vec<u64>) {
        match self {
            Node::Leaf(id) => out.push(*id),
            Node::Split { a, b, .. } => {
                a.leaves(out);
                b.leaves(out);
            }
        }
    }

    /// Where each pane sits, as fractions of the panel: `(id, x, y, w, h)`.
    ///
    /// Worked out here rather than in the window because the tree is here.
    /// The window places boxes; it does not need to know what a split is.
    pub fn places(&self, x: f32, y: f32, w: f32, h: f32, out: &mut Vec<(u64, f32, f32, f32, f32)>) {
        match self {
            Node::Leaf(id) => out.push((*id, x, y, w, h)),
            Node::Split { down, ratio, a, b } => {
                let r = ratio.clamp(0.05, 0.95);
                if *down {
                    a.places(x, y, w, h * r, out);
                    b.places(x, y + h * r, w, h * (1.0 - r), out);
                } else {
                    a.places(x, y, w * r, h, out);
                    b.places(x + w * r, y, w * (1.0 - r), h, out);
                }
            }
        }
    }
}
