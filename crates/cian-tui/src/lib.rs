use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use cian_core::ops::{self, Conflict, DeleteMode, OpReport};
use cian_core::{Pane, Sort, SortKey};
use cian_lua::Config;
use cian_pty::PtySession;
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags, MouseButton,
    MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{SetTitle, 
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::{Frame, Terminal};
use serde::{Deserialize, Serialize};
use tui_term::widget::PseudoTerminal;

/// Resolved color palette. Defaults match the original built-in theme; a
/// `~/.config/cian/init.lua` calling `cian.set_theme{...}` overrides any field.
#[derive(Debug, Clone, Copy)]
struct ResolvedTheme {
    accent: Color,
    status_bg: Color,
    selected_bg: Color,
    visual_bg: Color,
    mark_fg: Color,
}

impl Default for ResolvedTheme {
    fn default() -> Self {
        Self {
            accent: Color::Cyan, // cian-blue, kept consistent across the app
            status_bg: Color::Rgb(40, 40, 55),
            selected_bg: Color::Rgb(60, 60, 90),
            visual_bg: Color::Rgb(80, 60, 30),
            mark_fg: Color::Yellow,
        }
    }
}

/// Process-wide resolved theme. Set once at startup from the Lua config so the
/// stateless draw helpers can read it without threading it through every call.
static THEME: OnceLock<ResolvedTheme> = OnceLock::new();

fn theme() -> &'static ResolvedTheme {
    THEME.get_or_init(ResolvedTheme::default)
}

/// Which corner glyphs the borders use. Set once at startup; see
/// [`resolve_border_type`].
static BORDERS: OnceLock<BorderType> = OnceLock::new();

fn border_type() -> BorderType {
    *BORDERS.get_or_init(|| resolve_border_type(None))
}

/// Pick rounded or square corners.
///
/// Rounded corners are `╭╮╯╰` (U+256D–U+2570), which plenty of console fonts —
/// Consolas and Lucida Console among them — simply do not contain, while the
/// straight `─│` (U+2500, U+2502) are in almost all of them. Windows then
/// font-links just the corners to some other face, whose metrics differ, and
/// the frame looks a few pixels out at each corner while its sides stay put.
///
/// So: square corners in the legacy Windows console, rounded where the
/// terminal is known to cope, and an explicit `borders` option to override.
fn resolve_border_type(configured: Option<&str>) -> BorderType {
    match configured.map(|s| s.trim().to_lowercase()).as_deref() {
        Some("plain") | Some("square") => return BorderType::Plain,
        Some("rounded") => return BorderType::Rounded,
        _ => {}
    }
    if cfg!(windows) && !modern_terminal() {
        BorderType::Plain
    } else {
        BorderType::Rounded
    }
}

/// Whether the host terminal advertises itself as a modern one. The legacy
/// Windows console sets none of these.
fn modern_terminal() -> bool {
    std::env::var_os("WT_SESSION").is_some()
        || std::env::var_os("WEZTERM_PANE").is_some()
        || std::env::var_os("TERM_PROGRAM").is_some()
}

/// Remappable normal-mode actions. Keys the user binds via `cian.set_keymap`
/// resolve to one of these; the default key handling is otherwise untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    CursorDown,
    CursorUp,
    CursorBottom,
    PageUp,
    PageDown,
    Parent,
    EnterDir,
    Quit,
    Search,
    SearchNext,
    SearchPrev,
    History,
    Shortcuts,
    Copy,
    Move,
    Delete,
    Rename,
    NewFile,
    NewDir,
    OpenOther,
    OpenOtherTab,
    OpenExternal,
    CopyPath,
    CopyFileRef,
    MarkDown,
    MarkUp,
    InvertMarks,
    Visual,
    Command,
}

/// Map a Lua action name to an [`Action`]. Unknown names are reported as
/// config errors rather than silently ignored.
fn action_from_name(name: &str) -> Option<Action> {
    Some(match name {
        "cursor_down" => Action::CursorDown,
        "cursor_up" => Action::CursorUp,
        "cursor_bottom" => Action::CursorBottom,
        "page_up" => Action::PageUp,
        "page_down" => Action::PageDown,
        "parent" => Action::Parent,
        "enter" => Action::EnterDir,
        "quit" => Action::Quit,
        "search" => Action::Search,
        "search_next" => Action::SearchNext,
        "search_prev" => Action::SearchPrev,
        "history" => Action::History,
        "shortcuts" => Action::Shortcuts,
        "copy" => Action::Copy,
        "move" => Action::Move,
        "delete" => Action::Delete,
        "rename" => Action::Rename,
        "new_file" => Action::NewFile,
        "new_dir" => Action::NewDir,
        "open_other" => Action::OpenOther,
        "open_other_tab" => Action::OpenOtherTab,
        "open_external" => Action::OpenExternal,
        "copy_path" => Action::CopyPath,
        "copy_file_ref" => Action::CopyFileRef,
        "mark_down" => Action::MarkDown,
        "mark_up" => Action::MarkUp,
        "invert_marks" => Action::InvertMarks,
        "visual" => Action::Visual,
        "command" => Action::Command,
        _ => return None,
    })
}

/// Parse a user color spec: `#rrggbb`, `r,g,b`, or a named color.
fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
        return None;
    }
    if s.contains(',') {
        let parts: Vec<&str> = s.split(',').map(|x| x.trim()).collect();
        if parts.len() == 3 {
            let r = parts[0].parse::<u8>().ok()?;
            let g = parts[1].parse::<u8>().ok()?;
            let b = parts[2].parse::<u8>().ok()?;
            return Some(Color::Rgb(r, g, b));
        }
        return None;
    }
    match s.to_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "darkgrey" => Some(Color::DarkGray),
        "lightred" => Some(Color::LightRed),
        "lightgreen" => Some(Color::LightGreen),
        "lightyellow" => Some(Color::LightYellow),
        "lightblue" => Some(Color::LightBlue),
        "lightmagenta" => Some(Color::LightMagenta),
        "lightcyan" => Some(Color::LightCyan),
        _ => None,
    }
}

/// Resolve a Lua [`Theme`] into a concrete palette, collecting any invalid
/// color specs as human-readable errors (the default is kept for those).
fn resolve_theme(t: &cian_lua::Theme) -> (ResolvedTheme, Vec<String>) {
    let mut c = ResolvedTheme::default();
    let mut errors = Vec::new();
    let mut apply = |spec: &Option<String>, slot: &mut Color, label: &str| {
        if let Some(s) = spec {
            match parse_color(s) {
                Some(col) => *slot = col,
                None => errors.push(format!("theme.{}: invalid color {:?}", label, s)),
            }
        }
    };
    apply(&t.accent, &mut c.accent, "accent");
    apply(&t.status_bg, &mut c.status_bg, "status_bg");
    apply(&t.selected_bg, &mut c.selected_bg, "selected_bg");
    apply(&t.visual_bg, &mut c.visual_bg, "visual_bg");
    apply(&t.mark_fg, &mut c.mark_fg, "mark_fg");
    (c, errors)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPane {
    Left,
    Right,
    Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Visual,
    Search,
    Command,
    /// Incremental filter: the listing narrows as the user types.
    Filter,
    Shell,
}


pub struct PaneTabs {
    pub tabs: Vec<Pane>,
    pub active: usize,
}

impl PaneTabs {
    pub fn single(p: Pane) -> Self {
        Self { tabs: vec![p], active: 0 }
    }
    pub fn active_ref(&self) -> &Pane { &self.tabs[self.active] }
    pub fn active_mut(&mut self) -> &mut Pane { &mut self.tabs[self.active] }
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }
    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
        }
    }
    pub fn select(&mut self, idx: usize) {
        if idx < self.tabs.len() { self.active = idx; }
    }
    pub fn add_clone(&mut self) -> Result<()> {
        let cwd = self.active_ref().cwd.clone();
        self.tabs.push(Pane::new(cwd)?);
        self.active = self.tabs.len() - 1;
        Ok(())
    }
    pub fn close_active(&mut self) {
        if self.tabs.len() > 1 {
            self.tabs.remove(self.active);
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len() - 1;
            }
        }
    }
}

/// How the panes inside one shell tab are arranged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitDir {
    /// Panes side by side (vertical dividers).
    LeftRight,
    /// Panes stacked (horizontal dividers).
    TopBottom,
}

/// A node in a shell tab's split tree: a leaf PTY pane, or a binary split of
/// two child nodes (referenced by slab index).
enum Node {
    /// A live pane. `bg` tints only this pane, so split panes can be told
    /// apart at a glance — the whole point of colouring them.
    Leaf { session: PtySession, bg: Option<Color> },
    /// `ratio` is the percentage of the split's area given to `first`; it is
    /// what dragging the border between the two children adjusts.
    Split { dir: SplitDir, first: usize, second: usize, ratio: u16 },
}

/// One shell tab: a binary tree of PTY panes supporting nested splits. Nodes
/// live in a slab indexed by `usize`; `None` slots are free for reuse.
struct ShellTab {
    nodes: Vec<Option<Node>>,
    root: usize,
    /// Index of the active leaf node.
    active: usize,
}

impl ShellTab {
    fn new(session: PtySession) -> Self {
        Self { nodes: vec![Some(Node::Leaf { session, bg: None })], root: 0, active: 0 }
    }

    fn alloc(&mut self, node: Node) -> usize {
        if let Some(i) = self.nodes.iter().position(|n| n.is_none()) {
            self.nodes[i] = Some(node);
            i
        } else {
            self.nodes.push(Some(node));
            self.nodes.len() - 1
        }
    }

    fn active_pane(&self) -> Option<&PtySession> {
        match self.nodes.get(self.active).and_then(|n| n.as_ref()) {
            Some(Node::Leaf { session, .. }) => Some(session),
            _ => None,
        }
    }
    fn active_pane_mut(&mut self) -> Option<&mut PtySession> {
        match self.nodes.get_mut(self.active).and_then(|n| n.as_mut()) {
            Some(Node::Leaf { session, .. }) => Some(session),
            _ => None,
        }
    }

    fn collect_leaves(&self, i: usize, out: &mut Vec<usize>) {
        match self.nodes.get(i).and_then(|n| n.as_ref()) {
            Some(Node::Leaf { .. }) => out.push(i),
            Some(Node::Split { first, second, .. }) => {
                self.collect_leaves(*first, out);
                self.collect_leaves(*second, out);
            }
            None => {}
        }
    }
    fn leaves(&self) -> Vec<usize> {
        let mut v = Vec::new();
        if self.nodes.get(self.root).map(|n| n.is_some()).unwrap_or(false) {
            self.collect_leaves(self.root, &mut v);
        }
        v
    }

    fn first_leaf(&self, i: usize) -> usize {
        match self.nodes.get(i).and_then(|n| n.as_ref()) {
            Some(Node::Split { first, .. }) => self.first_leaf(*first),
            _ => i,
        }
    }

    fn parent_of(&self, child: usize) -> Option<(usize, bool)> {
        for (i, n) in self.nodes.iter().enumerate() {
            if let Some(Node::Split { first, second, .. }) = n {
                if *first == child {
                    return Some((i, true));
                }
                if *second == child {
                    return Some((i, false));
                }
            }
        }
        None
    }

    /// Split the active leaf into (old, new) along `dir`; new becomes active.
    fn split(&mut self, dir: SplitDir, new_session: PtySession) {
        let old = self.active;
        if !matches!(self.nodes.get(old).and_then(|n| n.as_ref()), Some(Node::Leaf { .. })) {
            return;
        }
        let new_leaf = self.alloc(Node::Leaf { session: new_session, bg: None });
        let split_idx = self.alloc(Node::Split { dir, first: old, second: new_leaf, ratio: 50 });
        if old == self.root {
            self.root = split_idx;
        } else if let Some((p, is_first)) = self.parent_of(old) {
            if let Some(Node::Split { first, second, .. }) = self.nodes[p].as_mut() {
                if is_first {
                    *first = split_idx;
                } else {
                    *second = split_idx;
                }
            }
        }
        self.active = new_leaf;
    }

    fn focus_next(&mut self, forward: bool) {
        let leaves = self.leaves();
        if leaves.is_empty() {
            return;
        }
        let pos = leaves.iter().position(|&l| l == self.active).unwrap_or(0);
        let n = leaves.len();
        let np = if forward { (pos + 1) % n } else { (pos + n - 1) % n };
        self.active = leaves[np];
    }

    /// Close the active leaf; its sibling takes the parent's place. Returns true
    /// if the tab is now empty.
    fn close_active(&mut self) -> bool {
        let leaf = self.active;
        if !matches!(self.nodes.get(leaf).and_then(|n| n.as_ref()), Some(Node::Leaf { .. })) {
            return self.leaves().is_empty();
        }
        if leaf == self.root {
            self.nodes[leaf] = None;
            return true;
        }
        let (p, leaf_is_first) = match self.parent_of(leaf) {
            Some(x) => x,
            None => {
                self.nodes[leaf] = None;
                return self.leaves().is_empty();
            }
        };
        let sib = match self.nodes[p].as_ref() {
            Some(Node::Split { first, second, .. }) => {
                if leaf_is_first { *second } else { *first }
            }
            _ => return false,
        };
        if p == self.root {
            self.root = sib;
        } else if let Some((gp, p_is_first)) = self.parent_of(p) {
            if let Some(Node::Split { first, second, .. }) = self.nodes[gp].as_mut() {
                if p_is_first {
                    *first = sib;
                } else {
                    *second = sib;
                }
            }
        }
        self.nodes[leaf] = None;
        self.nodes[p] = None;
        self.active = self.first_leaf(sib);
        false
    }

    fn for_each_leaf_mut(&mut self, f: &mut dyn FnMut(&mut PtySession)) {
        for n in self.nodes.iter_mut() {
            if let Some(Node::Leaf { session: s, .. }) = n {
                f(s);
            }
        }
    }
}

/// The bottom shell panel: a set of tabs, each holding one or more split panes.
///
/// The first tab is spawned lazily on first focus.
pub struct ShellPane {
    tabs: Vec<ShellTab>,
    active: usize,
    /// Toggle (Shift+F12): show only the active split pane, filling the panel.
    zoom_pane: bool,
    /// Inner size of the whole shell panel, refreshed each frame; used as the
    /// initial size for newly-spawned panes before the next layout pass.
    rows: u16,
    cols: u16,
    shell_cmd: String,
    error: Option<String>,
    /// Spawns currently in flight on background threads; polled each tick by
    /// [`ShellPane::poll_pending`]. See [`ShellPane::spawn_async`].
    pending: Vec<PendingSpawn>,
    /// `(tab, split node)` for a split that was just created, so the UI can
    /// animate the new pane growing in. Consumed by whoever reads it.
    just_split: Option<(usize, usize)>,
}

/// A PTY spawn running on a background thread, plus what to do with the
/// session once it arrives.
struct PendingSpawn {
    rx: std::sync::mpsc::Receiver<std::result::Result<PtySession, String>>,
    kind: PendingKind,
}

/// Where a pending session should be installed once it is ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingKind {
    /// The lazily-started first tab (see [`ShellPane::ensure`]).
    FirstTab,
    /// An additional tab (F9).
    NewTab,
    /// A split of tab `tab`. Applies to whichever leaf is active in that tab
    /// when the session lands — normally the same one the user asked from,
    /// since spawns complete in well under a frame.
    Split { tab: usize, dir: SplitDir },
}

impl ShellPane {
    fn new(shell_cmd: String) -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            zoom_pane: false,
            rows: 24,
            cols: 80,
            shell_cmd,
            error: None,
            pending: Vec::new(),
            just_split: None,
        }
    }

    fn toggle_pane_zoom(&mut self) {
        self.zoom_pane = !self.zoom_pane;
    }

    fn count(&self) -> usize {
        self.tabs.len()
    }

    fn active_tab(&self) -> Option<&ShellTab> {
        self.tabs.get(self.active)
    }

    /// How many split panes the active tab has.
    fn active_pane_count(&self) -> usize {
        self.active_tab().map(|t| t.leaves().len()).unwrap_or(0)
    }

    /// Set the active pane's background. Per pane, not per panel: the point is
    /// to tell one split from another.
    fn set_active_pane_bg(&mut self, color: Option<Color>) {
        let active = self.active;
        if let Some(t) = self.tabs.get_mut(active) {
            let leaf = t.active;
            if let Some(Node::Leaf { bg, .. }) = t.nodes.get_mut(leaf).and_then(|n| n.as_mut()) {
                *bg = color;
            }
        }
    }

    /// The active pane's background, for pre-selecting it in the picker.
    fn active_pane_bg(&self) -> Option<Color> {
        let t = self.active_tab()?;
        match t.nodes.get(t.active).and_then(|n| n.as_ref()) {
            Some(Node::Leaf { bg, .. }) => *bg,
            _ => None,
        }
    }

    fn active_session(&self) -> Option<&PtySession> {
        self.active_tab().and_then(|t| t.active_pane())
    }

    fn active_session_mut(&mut self) -> Option<&mut PtySession> {
        self.tabs.get_mut(self.active).and_then(|t| t.active_pane_mut())
    }

    /// Start a PTY spawn on a background thread.
    ///
    /// Spawning (openpty + fork/exec of the shell) must never run on the UI
    /// thread: the event loop is single-threaded, so a slow shell startup
    /// (heavy rc files, a hung `$SHELL`, a stalled network home directory)
    /// would block *all* input until it returned — the app looked frozen.
    /// Every spawn path goes through here; results are installed by
    /// [`ShellPane::poll_pending`].
    fn spawn_async(&mut self, cwd: &Path, kind: PendingKind) {
        let cwd = cwd.to_path_buf();
        let shell_cmd = self.shell_cmd.clone();
        let rows = self.rows.max(1);
        let cols = self.cols.max(1);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            if cian_core::log::enabled() {
                cian_core::log::log(&format!("spawning shell {:?} in {}", shell_cmd, cwd.display()));
            }
            let result = PtySession::new(&cwd, &shell_cmd, rows, cols).map_err(|e| e.to_string());
            if cian_core::log::enabled() {
                match &result {
                    Ok(_) => cian_core::log::log("shell spawned"),
                    Err(e) => cian_core::log::log(&format!("shell spawn failed: {}", e)),
                }
            }
            let _ = tx.send(result);
        });
        self.pending.push(PendingSpawn { rx, kind });
        self.error = None;
    }

    /// Whether a spawn of this kind is already in flight.
    fn is_pending(&self, kind: PendingKind) -> bool {
        self.pending.iter().any(|p| p.kind == kind)
    }

    /// True while the panel has no pane yet but one is on its way.
    fn is_starting(&self) -> bool {
        self.tabs.is_empty() && !self.pending.is_empty()
    }

    /// Spawn the first tab if none exists yet (lazy start on first focus).
    fn ensure(&mut self, cwd: &Path) {
        if self.tabs.is_empty() && !self.is_pending(PendingKind::FirstTab) {
            self.spawn_async(cwd, PendingKind::FirstTab);
        }
    }

    /// Install any background spawns that have completed. Returns true if the
    /// panel's state changed (so the caller should repaint).
    fn poll_pending(&mut self) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        let mut changed = false;
        let mut still_pending = Vec::with_capacity(self.pending.len());
        for p in std::mem::take(&mut self.pending) {
            match p.rx.try_recv() {
                Ok(Ok(session)) => {
                    self.install(session, p.kind);
                    changed = true;
                }
                Ok(Err(e)) => {
                    self.error = Some(e);
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => still_pending.push(p),
                // The worker vanished without sending (it panicked). Drop it
                // rather than waiting forever.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("shell spawn failed unexpectedly".to_string());
                    changed = true;
                }
            }
        }
        self.pending = still_pending;
        changed
    }

    /// Place a freshly-spawned session according to what asked for it.
    fn install(&mut self, session: PtySession, kind: PendingKind) {
        match kind {
            PendingKind::FirstTab => {
                self.tabs.push(ShellTab::new(session));
                self.active = self.tabs.len() - 1;
            }
            PendingKind::NewTab => {
                self.tabs.push(ShellTab::new(session));
                self.active = self.tabs.len() - 1;
                self.zoom_pane = false;
            }
            PendingKind::Split { tab, dir } => match self.tabs.get_mut(tab) {
                Some(t) => {
                    t.split(dir, session);
                    // `split` makes the new leaf active, so its parent is the
                    // split node that was just created.
                    self.just_split = t.parent_of(t.active).map(|(p, _)| (tab, p));
                    // A split must be visible, so leave single-pane zoom.
                    self.zoom_pane = false;
                }
                // The target tab was closed while we were spawning; the
                // session is dropped here, which kills the shell.
                None => return,
            },
        }
        self.error = None;
    }

    /// Open an additional shell tab.
    fn new_tab(&mut self, cwd: &Path) {
        self.spawn_async(cwd, PendingKind::NewTab);
    }

    /// Split the active tab's active pane in `dir`, spawning a new pane.
    fn split_active(&mut self, cwd: &Path, dir: SplitDir) {
        if self.tabs.get(self.active).is_none() {
            return;
        }
        let kind = PendingKind::Split { tab: self.active, dir };
        self.spawn_async(cwd, kind);
    }

    fn next_pane(&mut self) {
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.focus_next(true);
        }
        self.zoom_pane = false;
    }

    fn prev_pane(&mut self) {
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.focus_next(false);
        }
        self.zoom_pane = false;
    }

    /// Close the active pane. If its tab becomes empty the tab is removed.
    /// Returns true if no tabs remain (caller should leave the shell).
    fn close_active_pane(&mut self) -> bool {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            if tab.close_active() {
                self.tabs.remove(self.active);
                if self.active >= self.tabs.len() && self.active > 0 {
                    self.active -= 1;
                }
            }
        }
        self.zoom_pane = false;
        self.tabs.is_empty()
    }

    /// Switch to shell tab `i` (no-op if out of range).
    fn select(&mut self, i: usize) {
        if i < self.tabs.len() {
            self.active = i;
            self.zoom_pane = false;
        }
    }

    /// Close the whole active tab. Returns true if no tabs remain.
    fn close_active(&mut self) -> bool {
        if self.active < self.tabs.len() {
            self.tabs.remove(self.active);
            if self.active >= self.tabs.len() && self.active > 0 {
                self.active -= 1;
            }
        }
        self.zoom_pane = false;
        self.tabs.is_empty()
    }

    /// Clear and report whether any pane in the active tab produced new output.
    fn take_active_tab_dirty(&mut self) -> bool {
        let mut dirty = false;
        if let Some(t) = self.tabs.get_mut(self.active) {
            t.for_each_leaf_mut(&mut |p| {
                if p.take_dirty() {
                    dirty = true;
                }
            });
        }
        dirty
    }

    /// `(alternate_screen, application_cursor)` for the active pane.
    fn active_modes(&self) -> (bool, bool) {
        if let Some(s) = self.active_session() {
            if let Ok(p) = s.parser().lock() {
                let scr = p.screen();
                return (scr.alternate_screen(), scr.application_cursor());
            }
        }
        (false, false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingOp {
    Copy,
    Move,
}

#[derive(Debug, Clone)]
enum Popup {
    None,
    ConfirmDelete { targets: Vec<PathBuf> },
    ConfirmTransfer { op: PendingOp, targets: Vec<PathBuf>, dest: PathBuf },
    TextInput {
        title: String,
        prompt: String,
        buffer: String,
        kind: InputKind,
        /// Caret position, as a char index into `buffer`, so the middle of a
        /// name can be edited rather than only its end.
        cursor: usize,
    },
    Notice { lines: Vec<String> },
    /// The key manual. Unlike `Notice` it is far taller than any terminal, so
    /// it carries a scroll offset (in lines from the top).
    Manual { lines: Vec<String>, scroll: usize },
    /// Right-click menu, anchored near the pointer.
    ContextMenu { items: Vec<MenuItem>, cursor: usize, at: (u16, u16) },
    /// Background-color picker for the pane that was right-clicked.
    ColorPicker { pane: FocusedPane, cursor: usize },
    /// Sort-order picker for the focused pane.
    SortPicker { cursor: usize },
    /// A file's contents, scrollable.
    Viewer { title: String, view: cian_core::viewer::View, scroll: usize },
    /// The left pane's file against the right pane's, side by side.
    ///
    /// Both the full row list and the folded one are kept: folding is a toggle
    /// people flick back and forth, and recomputing it belongs nowhere near
    /// the render path.
    Diff {
        left: String,
        right: String,
        result: cian_core::diff::Diff,
        folded: Vec<cian_core::diff::Row>,
        fold: bool,
        scroll: usize,
    },
    /// An archive's members, with extraction from the list.
    Archive {
        path: PathBuf,
        members: Vec<cian_core::archive::Member>,
        cursor: usize,
        scroll: usize,
    },
    /// Where to send a copy or move: recent destinations plus a way to type
    /// somewhere new.
    DestPicker { op: PendingOp, targets: Vec<PathBuf>, cursor: usize },
    /// Results of a recursive search, filling in as they are found.
    FindResults { hits: Vec<cian_core::search::Hit>, cursor: usize, scroll: usize },
    /// SSH: pick a host, then a user on it.
    SshHosts { cursor: usize, filter: String },
    SshUsers { host: usize, cursor: usize },
    Search { buffer: String },
    History { entries: Vec<PathBuf>, cursor: usize },
    Shortcuts { entries: Vec<Shortcut>, cursor: usize },
    ConfirmQuit,
    ConfirmClose { target: CloseTarget },
}

/// An entry in the right-click menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuItem {
    Copy,
    Cut,
    Paste,
    CopyToOther,
    MoveToOther,
    CopyToPath,
    Delete,
    Rename,
    Background,
    HiddenToggle,
    Attributes,
    Hash,
    Compare,
    Ssh,
    Manual,
}

impl MenuItem {
    fn label(self) -> &'static str {
        match self {
            MenuItem::Copy => "Copy",
            MenuItem::Cut => "Cut",
            MenuItem::Paste => "Paste",
            MenuItem::CopyToOther => "Copy to other pane",
            MenuItem::MoveToOther => "Move to other pane",
            MenuItem::CopyToPath => "Copy to…  (recent / typed)",
            MenuItem::Delete => "Delete (to trash)",
            MenuItem::Rename => "Rename",
            MenuItem::Background => "Background color…",
            MenuItem::HiddenToggle => "Show / hide dotfiles",
            MenuItem::Attributes => "Attributes…",
            MenuItem::Hash => "Checksum…",
            MenuItem::Compare => "Compare left ↔ right",
            MenuItem::Ssh => "SSH connect…",
            MenuItem::Manual => "Key manual  (?)",
        }
    }
}

/// A password held until ssh asks for it.
///
/// ssh reads the password from its controlling terminal rather than stdin, so
/// it cannot be piped in — but cian *owns* that terminal, so writing to the PTY
/// when the prompt appears works. This is the same approach TeraTerm's `.ttl`
/// macros take (`wait 'password:'` / `sendln`), and expect(1) before them.
///
/// Waiting for the prompt rather than sending blindly is what keeps this from
/// breaking everything else: a host on key auth never prompts, so the secret is
/// simply never sent and the deadline quietly expires.
struct PendingAuth {
    secret: String,
    /// Give up after this; the connection was probably keyed, refused, or is
    /// asking something else entirely (a host-key confirmation, an MFA code).
    deadline: Instant,
}

/// Never let a secret reach a log line or a panic message.
impl std::fmt::Debug for PendingAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingAuth").field("secret", &"<redacted>").finish()
    }
}

/// How long to watch for a password prompt before giving up.
const AUTH_WINDOW: Duration = Duration::from_secs(20);

/// Does this screen end in something asking for a password?
///
/// Deliberately narrow: only a prompt on the last non-empty line counts, so
/// the word "password" scrolling past in a log cannot trigger a send.
fn looks_like_password_prompt(screen: &str) -> bool {
    let Some(last) = screen.lines().map(|l| l.trim_end()).rfind(|l| !l.is_empty())
    else {
        return false;
    };
    let l = last.to_lowercase();
    // A host-key question also ends in a colon but must not be answered with a
    // password; it is handled by the user.
    if l.contains("yes/no") || l.contains("fingerprint") {
        return false;
    }
    (l.contains("password") || l.contains("passphrase")) && l.trim_end().ends_with(':')
}

/// A file operation running on a worker thread.
///
/// Copies and moves used to run inline: a 700 MB file locked the UI for
/// fourteen seconds with nothing on screen explaining why. The work now runs
/// off the event loop, reports progress back over a channel, and watches a
/// flag it can be told to stop by.
struct OpJob {
    rx: std::sync::mpsc::Receiver<OpMsg>,
    cancel: Arc<AtomicBool>,
    /// What to call it in the popup.
    label: &'static str,
    latest: cian_core::progress::Progress,
    started: Instant,
}

enum OpMsg {
    Tick(cian_core::progress::Progress),
    Done(OpReport),
}

/// A recursive search running on a worker thread.
///
/// Kept separate from [`OpJob`] because results stream in rather than a single
/// report arriving at the end: a search over a big tree should be usable while
/// it is still going.
struct FindJob {
    rx: std::sync::mpsc::Receiver<FindMsg>,
    cancel: Arc<AtomicBool>,
    /// Pre-rendered for the popup title; the borrow checker objects to
    /// formatting it while `popup` is mutably borrowed for drawing.
    root_label: String,
    query: String,
    mode: cian_core::search::Mode,
    done: Option<cian_core::search::Outcome>,
}

enum FindMsg {
    Hit(cian_core::search::Hit),
    Done(cian_core::search::Outcome),
}

/// Work deferred until a shrink transition finishes.
#[derive(Debug, Clone, Copy)]
enum PendingClose {
    /// Remove the shell's active split pane.
    ShellPane,
}

/// Whether a clipboard entry will be copied or moved when pasted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipOp {
    Copy,
    Cut,
}

/// Files held for a later paste, Explorer-style: copy or cut here, navigate
/// somewhere else, paste there. Independent of the system clipboard, which
/// `p`/`Shift+P` still drive.
#[derive(Debug, Clone)]
struct FileClipboard {
    paths: Vec<PathBuf>,
    op: ClipOp,
}

/// Preset pane backgrounds.
///
/// These exist to answer "which pane am I typing into?", so they are pitched
/// to be unmistakable at a glance rather than tasteful — an earlier, subtler
/// set failed at exactly that. Still dark enough to keep normal terminal
/// foreground colors readable on top.
/// Evenly spaced around the hue wheel at a fixed, low brightness, so no two
/// are confusable and all keep foreground text readable. Verified by
/// `the_palette_is_distinct_enough_to_tell_panes_apart`.
const PANE_BG_PRESETS: [(&str, Option<Color>); 9] = [
    ("default", None),
    ("navy", Some(Color::Rgb(17, 45, 87))),
    ("teal", Some(Color::Rgb(17, 87, 69))),
    ("forest", Some(Color::Rgb(25, 87, 17))),
    ("olive", Some(Color::Rgb(85, 87, 17))),
    ("rust", Some(Color::Rgb(87, 29, 17))),
    ("wine", Some(Color::Rgb(87, 17, 65))),
    ("plum", Some(Color::Rgb(49, 17, 87))),
    ("slate", Some(Color::Rgb(62, 69, 87))),
];

/// What a close-confirmation popup will close when accepted.
#[derive(Debug, Clone, Copy)]
enum CloseTarget {
    /// The active split pane in the shell.
    ShellPane,
    /// The active tab of a file pane.
    FileTab(FocusedPane),
}

#[derive(Debug, Clone)]
enum InputKind {
    Rename { original: PathBuf },
    NewFile { parent: PathBuf },
    NewDir { parent: PathBuf },
    ShortcutName { editing_index: Option<usize> },
    ShortcutTarget { editing_index: Option<usize>, name: String },
    /// A path typed to jump to (or a file to open).
    JumpPath,
    /// A name to search for, recursively from the current directory.
    FindRecursive,
    /// Text to look for inside the files below the current directory.
    GrepRecursive,
    /// A directory typed as the destination of a pending copy or move.
    DestPath { op: PendingOp, targets: Vec<PathBuf> },
    /// A password for a zip about to be created. Rendered masked.
    ZipPassword { dest: PathBuf, sources: Vec<PathBuf> },
    /// A new name for a single file being copied/moved into `dest_dir`.
    TransferAs { op: PendingOp, src: PathBuf, dest_dir: PathBuf },
}

impl InputKind {
    /// Whether the field holds a secret and should be shown as dots.
    fn is_secret(&self) -> bool {
        matches!(self, InputKind::ZipPassword { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shortcut {
    pub name: String,
    pub target: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct ShortcutsFile {
    #[serde(default)]
    shortcuts: Vec<Shortcut>,
}

pub struct ShortcutStore {
    pub entries: Vec<Shortcut>,
    pub path: PathBuf,
}

impl ShortcutStore {
    fn default_path() -> PathBuf {
        home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("cian")
            .join("shortcuts.toml")
    }

    pub fn load_or_default() -> Self {
        let path = Self::default_path();
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str::<ShortcutsFile>(&s).ok())
            .map(|f| f.shortcuts)
            .unwrap_or_default();
        Self { entries, path }
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = ShortcutsFile { shortcuts: self.entries.clone() };
        let s = toml::to_string_pretty(&file)?;
        std::fs::write(&self.path, s)?;
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct LayoutRects {
    left: Rect,
    right: Rect,
    shell: Rect,
}

/// Which split a draggable border adjusts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DividerTarget {
    /// The horizontal border between the file panes and the shell panel.
    Main,
    /// The vertical border between the left and right file panes.
    Panes,
    /// A border inside the shell panel's split tree.
    ShellSplit { tab: usize, node: usize },
}

/// A border the user can grab and drag to re-proportion a split. Rebuilt every
/// frame during rendering, since it depends on the current geometry.
#[derive(Debug, Clone, Copy)]
struct Divider {
    /// The band of cells that counts as grabbing this border.
    zone: Rect,
    /// The area being divided; the drag position is mapped into this.
    parent: Rect,
    /// Whether the border moves horizontally or vertically.
    dir: Direction,
    target: DividerTarget,
}

impl Divider {
    /// Convert an absolute mouse position into a percentage for the first
    /// child, clamped so neither side can be squeezed out of existence.
    fn ratio_at(&self, col: u16, row: u16) -> u16 {
        let (pos, start, len) = match self.dir {
            Direction::Horizontal => (col, self.parent.x, self.parent.width),
            Direction::Vertical => (row, self.parent.y, self.parent.height),
        };
        if len == 0 {
            return 50;
        }
        let offset = pos.saturating_sub(start).min(len);
        let pct = (offset as u32 * 100 / len as u32) as u16;
        pct.clamp(MIN_SPLIT_PCT, 100 - MIN_SPLIT_PCT)
    }
}

/// Neither side of a split may shrink below this share of its parent, so a
/// border can never be dragged far enough to make a pane unusable.
const MIN_SPLIT_PCT: u16 = 15;

/// How often the panes are checked against the filesystem. Long enough to be
/// invisible in cost, short enough that a file appearing feels immediate.
const WATCH_INTERVAL: Duration = Duration::from_millis(1200);

/// How many copy/move destinations to remember.
const DEST_HISTORY_CAP: usize = 15;

/// How long an operation flash stays visible.
const FLASH_SECS: f32 = 0.45;

/// Default transition length. Long enough to read as motion, short enough that
/// it never gets in the way of fast keyboard work.
const DEFAULT_ANIM_MS: u64 = 150;

/// A layout transition in flight.
///
/// Transitions are *purely visual*: PTYs keep their old size for the duration
/// and are resized exactly once, when the transition lands. Resizing a PTY per
/// frame would send a SIGWINCH storm to the shell and make it reflow a dozen
/// times, which looks far worse than the animation looks good.
#[derive(Debug, Clone, Copy)]
struct Anim {
    kind: AnimKind,
    start: Instant,
    dur: Duration,
}

#[derive(Debug, Clone, Copy)]
enum AnimKind {
    /// A surface growing to fill the window, or shrinking back out of it.
    Zoom { from: Rect, to: Rect },
    /// A split's ratio easing between two values — used both when a split is
    /// created (the new pane grows in) and when one is closed (it shrinks away).
    Ratio { target: DividerTarget, from: u16, to: u16 },
}

impl Anim {
    /// Eased 0.0..=1.0 position through the transition.
    fn progress(&self) -> f32 {
        if self.dur.is_zero() {
            return 1.0;
        }
        let t = (self.start.elapsed().as_secs_f32() / self.dur.as_secs_f32()).clamp(0.0, 1.0);
        // Ease-out cubic: quick to start, settling gently. Reads as "snappy"
        // rather than "slow" at these durations.
        1.0 - (1.0 - t).powi(3)
    }

    fn done(&self) -> bool {
        self.start.elapsed() >= self.dur
    }
}

/// The smallest rect containing both. Zero-sized inputs are ignored so an
/// absent surface (e.g. while zoomed) does not drag the union to the origin.
fn union_rect(a: Rect, b: Rect) -> Rect {
    if a.width == 0 || a.height == 0 {
        return b;
    }
    if b.width == 0 || b.height == 0 {
        return a;
    }
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let r = (a.x + a.width).max(b.x + b.width);
    let bo = (a.y + a.height).max(b.y + b.height);
    Rect { x, y, width: r - x, height: bo - y }
}

/// Linear interpolation between two rects at eased position `t`.
fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    let f = |x: u16, y: u16| -> u16 {
        (x as f32 + (y as f32 - x as f32) * t).round().max(0.0) as u16
    };
    Rect {
        x: f(a.x, b.x),
        y: f(a.y, b.y),
        width: f(a.width, b.width).max(1),
        height: f(a.height, b.height).max(1),
    }
}

/// Rendering overrides applied while a transition is in flight.
#[derive(Debug, Default, Clone, Copy)]
struct AnimOverride {
    /// Use this ratio instead of the divider's stored one.
    ratio: Option<(DividerTarget, u16)>,
    /// Leave PTY sizes alone; they are applied once the transition lands.
    freeze_pty: bool,
}

impl AnimOverride {
    /// The ratio to render `target` at: the override if it applies, else the
    /// stored value clamped to a usable range.
    fn ratio_for(&self, target: DividerTarget, stored: u16) -> u16 {
        match self.ratio {
            Some((t, r)) if t == target => r,
            _ => stored.clamp(MIN_SPLIT_PCT, 100 - MIN_SPLIT_PCT),
        }
    }
}

/// Files being dragged from one pane to another.
///
/// cian cannot take part in the OS's drag and drop — a console application has
/// no window to be a drag source or target — but it owns the mouse events
/// inside its own surface, so dragging between its panes works.
#[derive(Debug, Clone)]
struct FileDrag {
    from: FocusedPane,
    paths: Vec<PathBuf>,
    /// Where the pointer is now, so the drop target can be highlighted.
    over: Option<FocusedPane>,
    /// True once the pointer has actually moved; a press and release without
    /// motion is a click, not a drag.
    moved: bool,
}

pub struct App {
    pub left: PaneTabs,
    pub right: PaneTabs,
    pub shell: ShellPane,
    pub focused: FocusedPane,
    pub mode: Mode,
    pub command_buffer: String,
    /// In-progress text for [`Mode::Filter`].
    pub filter_buffer: String,
    pub message: Option<String>,
    pub last_file_pane: FocusedPane,
    pub should_quit: bool,
    pub visual_anchor: Option<usize>,
    pub clipboard_on_copy: bool,
    clipboard: Option<arboard::Clipboard>,
    popup: Popup,
    layout_rects: LayoutRects,
    /// Percentage of the window given to the file panes; the shell gets the
    /// rest. Adjusted by dragging the border between them.
    main_pct: u16,
    /// Percentage of the file-pane area given to the left pane.
    panes_pct: u16,
    /// Draggable borders for the current frame, rebuilt during rendering.
    dividers: Vec<Divider>,
    /// `(tab, leaf, rect)` for each shell split pane on screen, so a click can
    /// land on the pane under the pointer rather than whichever was active.
    shell_leaves: Vec<(usize, usize, Rect)>,
    /// The border currently being dragged, if any.
    drag: Option<Divider>,
    /// Files picked up by the mouse and not yet dropped.
    file_drag: Option<FileDrag>,
    /// Directories recently copied or moved into, most recent first.
    dest_history: Vec<PathBuf>,
    /// Files awaiting a paste (see [`FileClipboard`]).
    file_clip: Option<FileClipboard>,
    /// Pane to briefly highlight after an operation landed there, and when it
    /// started. Makes it obvious *where* a copy/move/delete took effect.
    flash: Option<(FocusedPane, Instant)>,
    /// Layout transition in flight, if any.
    anim: Option<Anim>,
    /// Work to run when the current transition finishes (e.g. actually closing
    /// the pane that just finished shrinking away).
    anim_then: Option<PendingClose>,
    /// Transition length; zero disables animation.
    anim_dur: Duration,
    /// The focused surface's rect from before it was zoomed.
    ///
    /// While zoomed, `layout_rects` describes the zoomed layout — the focused
    /// surface fills the window and the others are empty — so the rect to
    /// shrink back into is not recoverable from it and has to be kept.
    zoom_return: Option<Rect>,
    /// Show the contextual key-hint bar.
    show_key_hints: bool,
    /// A command to type into the shell once it is ready. Needed because the
    /// PTY spawns on a background thread, so the shell may not exist yet at
    /// the moment the user picks a connection.
    pending_shell_input: Option<String>,
    /// A target path chosen for a shortcut being added from somewhere other
    /// than the file cursor (e.g. the history list), consumed by the name step.
    pending_shortcut_target: Option<String>,
    /// A password waiting for ssh to ask for it. See [`PendingAuth`].
    pending_auth: Option<PendingAuth>,
    /// A copy/move/delete running on a worker thread.
    op_job: Option<OpJob>,
    /// A recursive search running on a worker thread.
    find_job: Option<FindJob>,
    /// When the panes were last checked against the filesystem.
    last_watch: Instant,
    /// Per-pane background overrides, indexed by [`Self::bg_slot`].
    /// Session-only: deliberately not persisted.
    pane_bg: [Option<Color>; 2],
    last_search_query: Option<String>,
    pub shortcuts: ShortcutStore,
    pending_g: bool,
    /// When true, only the focused surface is drawn, filling the window.
    pub zoomed: bool,
    /// When CIAN_DEBUG_KEYS is set, show each shell keypress in the status bar.
    debug_keys: bool,
    config: Config,
    /// User keymap overrides: plain character keys (no Ctrl) the user bound via
    /// `cian.set_keymap`. Only contains entries the user set; everything else
    /// falls through to the built-in defaults.
    keymap: HashMap<char, Action>,
}

impl App {
    pub fn new(left: PathBuf, right: PathBuf, config: Config) -> Result<Self> {
        // Build the keymap from user overrides (invalid action names are
        // validated and reported separately in `run`).
        let mut keymap: HashMap<char, Action> = HashMap::new();
        for (c, name) in &config.keymaps {
            if let Some(a) = action_from_name(name) {
                keymap.insert(*c, a);
            }
        }
        let clipboard_on_copy = config.options.clipboard_on_copy.unwrap_or(true);
        let shell_cmd = config
            .options
            .shell
            .clone()
            .unwrap_or_else(cian_pty::default_shell);
        Ok(Self {
            left: PaneTabs::single(Pane::new(left)?),
            right: PaneTabs::single(Pane::new(right)?),
            shell: ShellPane::new(shell_cmd),
            focused: FocusedPane::Left,
            mode: Mode::Normal,
            command_buffer: String::new(),
            filter_buffer: String::new(),
            message: None,
            last_file_pane: FocusedPane::Left,
            should_quit: false,
            visual_anchor: None,
            clipboard_on_copy,
            clipboard: arboard::Clipboard::new().ok(),
            popup: Popup::None,
            layout_rects: LayoutRects::default(),
            main_pct: 60,
            panes_pct: 50,
            dividers: Vec::new(),
            shell_leaves: Vec::new(),
            drag: None,
            file_drag: None,
            dest_history: Vec::new(),
            file_clip: None,
            flash: None,
            anim: None,
            anim_then: None,
            anim_dur: Duration::from_millis(
                config.options.animation_ms.unwrap_or(DEFAULT_ANIM_MS),
            ),
            show_key_hints: config.options.key_hints.unwrap_or(true),
            zoom_return: None,
            pending_shell_input: None,
            pending_shortcut_target: None,
            pending_auth: None,
            op_job: None,
            find_job: None,
            last_watch: Instant::now(),
            pane_bg: [None, None],
            last_search_query: None,
            shortcuts: ShortcutStore::load_or_default(),
            pending_g: false,
            zoomed: false,
            debug_keys: std::env::var("CIAN_DEBUG_KEYS").is_ok(),
            config,
            keymap,
        })
    }

    /// The directory a newly-spawned shell should start in: the cwd of the
    /// file pane we were last on.
    fn shell_cwd(&self) -> PathBuf {
        let tabs = match self.last_file_pane {
            FocusedPane::Right => &self.right,
            _ => &self.left,
        };
        tabs.active_ref().cwd.clone()
    }

    fn active_file_tabs(&self) -> Option<&PaneTabs> {
        match self.focused {
            FocusedPane::Left => Some(&self.left),
            FocusedPane::Right => Some(&self.right),
            FocusedPane::Shell => None,
        }
    }
    fn active_file_tabs_mut(&mut self) -> Option<&mut PaneTabs> {
        match self.focused {
            FocusedPane::Left => Some(&mut self.left),
            FocusedPane::Right => Some(&mut self.right),
            FocusedPane::Shell => None,
        }
    }
    fn active_pane(&self) -> Option<&Pane> { self.active_file_tabs().map(|t| t.active_ref()) }
    fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        self.active_file_tabs_mut().map(|t| t.active_mut())
    }

    fn opposite_pane_cwd(&self) -> Option<PathBuf> {
        let other = match self.focused {
            FocusedPane::Left => &self.right,
            FocusedPane::Right => &self.left,
            FocusedPane::Shell => return None,
        };
        Some(other.active_ref().cwd.clone())
    }

    fn focus(&mut self, target: FocusedPane) {
        if matches!(self.focused, FocusedPane::Left | FocusedPane::Right) {
            self.last_file_pane = self.focused;
        }
        if target == FocusedPane::Shell {
            // Lazily start a shell in the directory we're coming from.
            let cwd = self
                .active_pane()
                .map(|p| p.cwd.clone())
                .unwrap_or_else(|| self.left.active_ref().cwd.clone());
            self.shell.ensure(&cwd);
        }
        self.focused = target;
        self.mode = match target {
            FocusedPane::Shell => Mode::Shell,
            _ => Mode::Normal,
        };
        self.visual_anchor = None;
    }

    fn focus_direction(&mut self, dir: char) {
        let next = match (self.focused, dir) {
            (FocusedPane::Left, 'l') => FocusedPane::Right,
            (FocusedPane::Right, 'h') => FocusedPane::Left,
            (FocusedPane::Left | FocusedPane::Right, 'j') => FocusedPane::Shell,
            // From shell: H and K both go left, L goes right.
            (FocusedPane::Shell, 'h') | (FocusedPane::Shell, 'k') => FocusedPane::Left,
            (FocusedPane::Shell, 'l') => FocusedPane::Right,
            _ => self.focused,
        };
        if next != self.focused {
            self.focus(next);
        }
    }

    fn run_command(&mut self) {
        let raw = self.command_buffer.trim().to_string();
        self.command_buffer.clear();
        self.mode = Mode::Normal;
        if raw.is_empty() {
            return;
        }
        // `!cmd` is a shell escape: everything after the bang is the command,
        // so it is split off before tokenising (the command has its own
        // quoting, and `%`-substitution happens inside).
        if let Some(rest) = raw.strip_prefix('!') {
            self.run_bang(rest);
            return;
        }

        // Split into a verb and its arguments. Whitespace-separated is enough:
        // the commands that take a path accept it as the whole remainder, and
        // the ones that take flags take single tokens.
        let mut parts = raw.split_whitespace();
        let verb = parts.next().unwrap_or("");
        let args: Vec<&str> = parts.collect();
        let rest = raw[verb.len()..].trim(); // the arguments as one string

        match verb {
            "q" | "quit" => self.should_quit = true,
            "shell" => self.focus(FocusedPane::Shell),
            "man" | "help" | "h" => self.open_manual(),
            "paste" => { let _ = self.paste_clip(); }
            "hidden" => self.toggle_hidden(),
            "view" | "look" => self.look_inside(),
            "diff" | "compare" => self.open_diff(),
            "copyto" => self.start_dest_picker(PendingOp::Copy),
            "moveto" => self.start_dest_picker(PendingOp::Move),
            "grep" => self.start_grep_prompt(),
            "find" => self.start_find_prompt(),
            "menu" => self.open_menu_at_cursor(),
            "ssh" => self.start_ssh(),

            // Navigation.
            "cd" | "goto" => {
                if rest.is_empty() {
                    self.start_jump_path();
                } else {
                    let _ = self.cmd_cd(rest);
                }
            }
            "pwd" => self.cmd_pwd(),

            // Creation.
            "mkdir" | "md" => self.cmd_mkdir(&args),
            "touch" => self.cmd_touch(&args),

            // Transfers: no argument means "to the other pane", matching the
            // y/m keys; an argument is an explicit destination.
            "cp" | "copy" => self.cmd_transfer(PendingOp::Copy, rest),
            "mv" | "move" => self.cmd_transfer(PendingOp::Move, rest),
            "rm" | "del" | "delete" => self.start_delete(),

            // Inspection.
            "ls" | "dir" => self.cmd_ls(&args),
            "stat" | "attr" | "attrs" => self.show_attributes(),
            "file" => self.cmd_file(),
            "wc" => self.cmd_wc(),
            "head" => self.cmd_peek(cian_core::inspect::End::Head, &args),
            "tail" => self.cmd_peek(cian_core::inspect::End::Tail, &args),
            "df" => self.cmd_df(&args),

            // Attributes and integrity.
            "chmod" => self.set_attr_command(rest),
            "readonly" => match rest {
                "on" | "true" | "1" => self.set_readonly_command(true),
                "off" | "false" | "0" => self.set_readonly_command(false),
                _ => self.message = Some("usage: :readonly on|off".into()),
            },
            "hash" | "sha256" | "md5" => {
                // `:hash md5` or `:md5` both work.
                let spec = if verb == "hash" { rest } else { verb };
                match cian_core::attrs::HashKind::parse(spec) {
                    Some(k) => self.start_hash(k),
                    None => self.message = Some(format!("unknown hash: {} (md5 or sha256)", spec)),
                }
            }

            // Archiving.
            "zip" => self.cmd_zip(&args),

            other => self.message = Some(format!("unknown command: :{}", other)),
        }
    }

    /// `pwd`: show the focused pane's directory and put it on the clipboard,
    /// since the usual reason to ask is to paste it somewhere.
    fn cmd_pwd(&mut self) {
        let Some(p) = self.active_pane() else {
            self.message = Some("no active pane".into());
            return;
        };
        let path = p.cwd.display().to_string();
        if let Some(cb) = self.clipboard.as_mut() {
            let _ = cb.set_text(path.clone());
        }
        self.message = Some(format!("{}  (copied)", path));
    }

    /// `cd <path>`: enter a directory directly, without the prompt.
    fn cmd_cd(&mut self, arg: &str) -> Result<()> {
        // `cd -` / `cd ..` / `cd ~` are worth honouring since the muscle memory
        // is universal; everything else is a path.
        let target = match arg {
            "-" => self.active_pane().and_then(|p| p.history.get(1).cloned()),
            _ => Some(expand_path(arg)),
        };
        let Some(target) = target else {
            self.message = Some("no previous directory".into());
            return Ok(());
        };
        if !target.is_dir() {
            self.message = Some(format!("not a directory: {}", target.display()));
            return Ok(());
        }
        if let Some(p) = self.active_pane_mut() {
            p.jump_to(target.clone())?;
        }
        self.message = Some(format!("→ {}", target.display()));
        Ok(())
    }

    /// `mkdir <name>` / `mkdir -p a/b/c`, created in the focused directory.
    fn cmd_mkdir(&mut self, args: &[&str]) {
        let parents = args.contains(&"-p");
        let names: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with('-')).collect();
        if names.is_empty() {
            self.message = Some("usage: :mkdir [-p] <name>".into());
            return;
        }
        let Some(cwd) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        let mut made = 0;
        for name in &names {
            match cian_core::ops::make_dir(&cwd, name, parents) {
                Ok(_) => made += 1,
                Err(e) => {
                    self.message = Some(format!("mkdir: {}", e));
                    break;
                }
            }
        }
        if made > 0 {
            self.reload_active();
            if self.message.is_none() {
                self.message = Some(format!("mkdir: created {}", made));
            }
        }
    }

    /// `touch <name>...`: create empty files, or bump the mtime of existing ones.
    fn cmd_touch(&mut self, args: &[&str]) {
        let names: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with('-')).collect();
        if names.is_empty() {
            self.message = Some("usage: :touch <name>".into());
            return;
        }
        let Some(cwd) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        let mut n = 0;
        for name in &names {
            match cian_core::ops::touch(&cwd, name) {
                Ok(_) => n += 1,
                Err(e) => {
                    self.message = Some(format!("touch: {}", e));
                    break;
                }
            }
        }
        if n > 0 {
            self.reload_active();
            if self.message.is_none() {
                self.message = Some(format!("touch: {}", n));
            }
        }
    }

    /// `cp`/`mv`: no argument moves the selection to the other pane; an
    /// argument is an explicit destination directory (or, for a single item, a
    /// new path).
    fn cmd_transfer(&mut self, op: PendingOp, arg: &str) {
        if arg.is_empty() {
            self.start_transfer(op);
            return;
        }
        let targets = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if targets.is_empty() {
            self.message = Some("nothing to operate on".into());
            return;
        }
        let dest = expand_path(arg);
        if dest.is_dir() {
            self.popup = Popup::ConfirmTransfer { op, targets, dest };
            return;
        }
        // Not an existing directory: only meaningful as a rename/copy of a
        // single item to that exact path, and only if its parent exists.
        if targets.len() != 1 {
            self.message = Some(format!("not a directory: {}", dest.display()));
            return;
        }
        let parent_ok = dest.parent().map(|p| p.as_os_str().is_empty() || p.is_dir()).unwrap_or(false);
        if !parent_ok {
            self.message = Some(format!("no such directory: {}", dest.display()));
            return;
        }
        let src = &targets[0];
        let res = match op {
            PendingOp::Move => std::fs::rename(src, &dest).map_err(anyhow::Error::from),
            PendingOp::Copy => cian_core::ops::copy_one(src, dest.parent().unwrap_or(&dest), Conflict::Overwrite)
                .and_then(|_| {
                    // copy_one lands it under the parent with the source name;
                    // if a different name was asked for, put it right.
                    let landed = dest.parent().unwrap_or(&dest).join(src.file_name().unwrap_or_default());
                    if landed != dest { std::fs::rename(&landed, &dest)?; }
                    Ok(())
                }),
        };
        match res {
            Ok(_) => {
                self.reload_both();
                self.message = Some(format!("{} → {}", if op == PendingOp::Move { "mv" } else { "cp" }, dest.display()));
            }
            Err(e) => self.message = Some(format!("{}: {}", if op == PendingOp::Move { "mv" } else { "cp" }, e)),
        }
    }

    /// `ls`: refresh the listing. `ls -a` toggles hidden files, which is the
    /// one flag that makes sense when the pane already *is* the listing.
    fn cmd_ls(&mut self, args: &[&str]) {
        if args.iter().any(|a| a.contains('a')) {
            self.toggle_hidden();
        } else {
            self.reload_active();
            self.message = Some("refreshed".into());
        }
    }

    /// `file`: name what the selection is, by magic number and content.
    fn cmd_file(&mut self) {
        let paths = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        let mut lines = Vec::new();
        for path in paths.iter().take(30) {
            let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let desc = cian_core::inspect::classify(path).unwrap_or_else(|e| e.to_string());
            lines.push(format!("{:<28} {}", truncate(&name, 28), desc));
        }
        if paths.len() > 30 {
            lines.push(format!("... and {} more", paths.len() - 30));
        }
        self.popup = Popup::Notice { lines };
    }

    /// `wc`: line, word and byte counts for the selection, with a total.
    fn cmd_wc(&mut self) {
        let paths = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        let mut lines = vec![format!("{:>9} {:>9} {:>11}  name", "lines", "words", "bytes"), String::new()];
        let mut tot = cian_core::inspect::Counts::default();
        let mut shown = 0;
        for path in paths.iter().take(30) {
            let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match cian_core::inspect::count(path) {
                Ok(c) => {
                    tot.lines += c.lines;
                    tot.words += c.words;
                    tot.bytes += c.bytes;
                    lines.push(format!("{:>9} {:>9} {:>11}  {}", c.lines, c.words, c.bytes, truncate(&name, 30)));
                    shown += 1;
                }
                Err(e) => lines.push(format!("{:>31}  {}: {}", "", truncate(&name, 20), e)),
            }
        }
        if shown > 1 {
            lines.push(String::new());
            lines.push(format!("{:>9} {:>9} {:>11}  total", tot.lines, tot.words, tot.bytes));
        }
        self.popup = Popup::Notice { lines };
    }

    /// `head`/`tail [-n N]`: the first or last N lines of the selected file.
    fn cmd_peek(&mut self, end: cian_core::inspect::End, args: &[&str]) {
        let n = parse_dash_n(args).unwrap_or(10);
        let Some(path) = self.active_pane().and_then(|p| p.selected().map(|e| e.path.clone())) else {
            self.message = Some("nothing selected".into());
            return;
        };
        match cian_core::inspect::peek(&path, end, n) {
            Ok(rows) => {
                let which = if end == cian_core::inspect::End::Head { "head" } else { "tail" };
                let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let mut lines = vec![format!("{} -n {}  {}", which, n, name), String::new()];
                lines.extend(rows.into_iter().map(|l| truncate(&l, 200)));
                self.popup = Popup::Notice { lines };
            }
            Err(e) => self.message = Some(format!("{}", e)),
        }
    }

    /// `df [-h|-k|-m|-g]`: free space on the focused pane's filesystem.
    fn cmd_df(&mut self, args: &[&str]) {
        let unit = match args.iter().find(|a| a.starts_with('-')) {
            Some(flag) => match cian_core::inspect::Unit::parse(flag) {
                Some(u) => u,
                None => {
                    self.message = Some(format!("df: unknown flag {} (try -h -k -m -g)", flag));
                    return;
                }
            },
            None => cian_core::inspect::Unit::Human,
        };
        let Some(cwd) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        match cian_core::inspect::disk_space(&cwd) {
            Ok(s) => {
                let lines = vec![
                    format!("filesystem holding  {}", cwd.display()),
                    String::new(),
                    format!("total      {}", unit.format(s.total)),
                    format!("used       {}   ({}%)", unit.format(s.used()), s.percent_used()),
                    format!("available  {}", unit.format(s.available)),
                ];
                self.popup = Popup::Notice { lines };
            }
            Err(e) => self.message = Some(format!("df: {}", e)),
        }
    }

    /// `zip [-e] <name>`: bundle the selection. `-e` asks for a password and
    /// AES-encrypts the result.
    fn cmd_zip(&mut self, args: &[&str]) {
        let encrypt = args.contains(&"-e") || args.contains(&"-p");
        let name = args.iter().copied().find(|a| !a.starts_with('-'));
        let Some(name) = name else {
            self.message = Some("usage: :zip [-e] <name.zip>".into());
            return;
        };
        let sources = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if sources.is_empty() {
            self.message = Some("nothing selected to zip".into());
            return;
        }
        let Some(cwd) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        let mut fname = name.to_string();
        if !fname.to_lowercase().ends_with(".zip") {
            fname.push_str(".zip");
        }
        let dest = cwd.join(&fname);
        if dest.exists() {
            self.message = Some(format!("already exists: {}", fname));
            return;
        }
        if encrypt {
            // Collect the password on a masked prompt, then build the zip when
            // it is submitted.
            self.popup = text_input(
                "zip password",
                "password (AES-256; Explorer cannot open — use 7-Zip):",
                String::new(),
                InputKind::ZipPassword { dest, sources },
            );
        } else {
            self.start_zip(dest, sources, None);
        }
    }

    /// Kick off zip creation on a worker, with progress and cancel like the
    /// other bulk operations.
    fn start_zip(&mut self, dest: PathBuf, sources: Vec<PathBuf>, password: Option<String>) {
        self.start_op("zipping", move |ctl| {
            cian_core::archive::create_zip(&sources, &dest, password.as_deref(), ctl)
        });
    }

    /// `!cmd`: run a shell command in the shell panel, with `%` substituted by
    /// the selected paths, `%f` by the current file, `%d` by the directory.
    fn run_bang(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            self.message = Some("usage: :!<command>   (% = selection, %f = file, %d = dir)".into());
            return;
        }
        let pane = self.active_pane();
        let cwd = pane.map(|p| p.cwd.display().to_string()).unwrap_or_default();
        let file = pane
            .and_then(|p| p.selected().map(|e| e.path.display().to_string()))
            .unwrap_or_default();
        let sel: Vec<String> = pane
            .map(|p| p.target_paths())
            .unwrap_or_default()
            .iter()
            .map(|p| shell_quote(&p.display().to_string()))
            .collect();
        let sel = sel.join(" ");

        // Longer tokens first so `%f`/`%d` are not eaten by `%`.
        let expanded = cmd
            .replace("%f", &shell_quote(&file))
            .replace("%d", &shell_quote(&cwd))
            .replace('%', &sel);
        self.run_in_shell(expanded);
    }

    fn reload_active(&mut self) {
        if let Some(p) = self.active_pane_mut() {
            let _ = p.reload();
        }
    }

    fn open_in_other_pane(&mut self, new_tab: bool) -> Result<()> {
        let target = match self.active_pane().and_then(|p| p.selected()) {
            Some(e) if e.is_dir => e.path.clone(),
            _ => { self.message = Some("not a directory".into()); return Ok(()); }
        };
        let other = match self.focused {
            FocusedPane::Left => &mut self.right,
            FocusedPane::Right => &mut self.left,
            FocusedPane::Shell => return Ok(()),
        };
        if new_tab {
            let pane = Pane::new(target.clone())?;
            other.tabs.push(pane);
            other.active = other.tabs.len() - 1;
        } else {
            other.active_mut().jump_to(target.clone())?;
        }
        // focus stays on the active pane
        self.message = Some(format!(
            "{} other pane → {}",
            if new_tab { "new tab in" } else { "opened in" },
            target.display()
        ));
        Ok(())
    }

    fn open_externally(&mut self) {
        let Some(pane) = self.active_pane() else { return };
        let Some(entry) = pane.selected() else { return };
        let path = entry.path.clone();
        // Extension-dispatch execution: if the user registered an `on_open`
        // handler for this extension in init.lua, run it instead of the OS open.
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !ext.is_empty() && self.config.has_ext_open(&ext) {
            match self.config.run_ext_open(&ext, &path) {
                Some(Ok(())) => {
                    self.message = Some(format!("opened via lua: {}", path.display()));
                    return;
                }
                Some(Err(e)) => {
                    self.message = Some(format!("on_open({}) error: {}", ext, e));
                    return;
                }
                None => {}
            }
        }
        match os_open(&path) {
            Ok(()) => self.message = Some(format!("opened: {}", path.display())),
            Err(e) => self.message = Some(format!("open failed: {}", e)),
        }
    }

    /// Text on the system clipboard, if any.
    fn clipboard_text(&mut self) -> Option<String> {
        self.clipboard.as_mut()?.get_text().ok().filter(|t| !t.is_empty())
    }

    /// Send the clipboard's text to the shell, as typing it would. Raw, with
    /// newlines: pasting a command line into a shell is meant to run it, and
    /// this does not know whether the child enabled bracketed paste, so it
    /// adds no wrapper (a stray `\x1b[200~` would otherwise print as garbage).
    fn paste_text_to_shell(&mut self) {
        match self.clipboard_text() {
            Some(t) => {
                if let Some(s) = self.shell.active_session_mut() {
                    s.write_input(t.as_bytes());
                } else {
                    self.message = Some("no shell to paste into".into());
                }
            }
            None => self.message = Some("clipboard has no text".into()),
        }
    }

    fn push_clipboard(&mut self, paths: &[PathBuf]) {
        if !self.clipboard_on_copy { return; }
        let Some(cb) = self.clipboard.as_mut() else { return; };
        let text = paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n");
        let _ = cb.set_text(text);
    }

    // ------- Visual mode -------
    fn visual_start(&mut self) {
        if let Some(p) = self.active_pane() {
            self.visual_anchor = Some(p.cursor);
            self.mode = Mode::Visual;
        }
    }
    fn visual_commit(&mut self) {
        let anchor = match self.visual_anchor.take() {
            Some(a) => a,
            None => { self.mode = Mode::Normal; return; }
        };
        if let Some(p) = self.active_pane_mut() {
            let cur = p.cursor;
            let (a, b) = if anchor <= cur { (anchor, cur) } else { (cur, anchor) };
            for i in a..=b { p.set_mark_at(i); }
        }
        self.mode = Mode::Normal;
    }
    fn visual_cancel_and_clear_all(&mut self) {
        self.visual_anchor = None;
        if let Some(p) = self.active_pane_mut() { p.clear_marks(); }
        self.mode = Mode::Normal;
    }

    // ------- Confirmation flows -------
    fn start_transfer(&mut self, op: PendingOp) {
        let Some(dest) = self.opposite_pane_cwd() else { return };
        let targets = match self.active_pane() {
            Some(p) => p.target_paths(),
            None => return,
        };
        if targets.is_empty() { self.message = Some("nothing to operate on".into()); return; }
        self.popup = Popup::ConfirmTransfer { op, targets, dest };
    }
    fn start_delete(&mut self) {
        let targets = match self.active_pane() {
            Some(p) => p.target_paths(),
            None => return,
        };
        if targets.is_empty() { self.message = Some("nothing to delete".into()); return; }
        self.popup = Popup::ConfirmDelete { targets };
    }
    fn start_rename(&mut self) {
        let Some(p) = self.active_pane() else { return };
        let Some(e) = p.selected() else { return };
        self.popup = text_input(
                "rename",
                "new name:",
                e.name.clone(),
                InputKind::Rename { original: e.path.clone() },
            );
    }
    fn start_new_file(&mut self) {
        let Some(p) = self.active_pane() else { return };
        self.popup = text_input(
                "new file",
                "name:",
                String::new(),
                InputKind::NewFile { parent: p.cwd.clone() },
            );
    }
    fn start_new_dir(&mut self) {
        let Some(p) = self.active_pane() else { return };
        self.popup = text_input(
                "new directory",
                "name:",
                String::new(),
                InputKind::NewDir { parent: p.cwd.clone() },
            );
    }

    // ------- Search -------
    fn start_search(&mut self) {
        self.popup = Popup::Search { buffer: String::new() };
        self.mode = Mode::Search;
    }

    fn finish_search(&mut self) {
        let popup = std::mem::replace(&mut self.popup, Popup::None);
        let buffer = if let Popup::Search { buffer } = popup { buffer } else { return };
        self.mode = Mode::Normal;
        let q = buffer.trim().to_string();
        if q.is_empty() { return; }
        self.last_search_query = Some(q.clone());
        let ql = q.to_lowercase();
        if let Some(p) = self.active_pane_mut() {
            if let Some(i) = p.entries.iter().position(|e| e.name.to_lowercase().contains(&ql)) {
                p.cursor = i;
            } else {
                self.message = Some(format!("pattern not found: {}", q));
            }
        }
    }

    // ------- Shortcuts -------
    fn start_shortcuts(&mut self) {
        self.popup = Popup::Shortcuts {
            entries: self.shortcuts.entries.clone(),
            cursor: 0,
        };
    }

    fn start_shortcut_add(&mut self) {
        self.popup = text_input(
                "new shortcut — name",
                "name:",
                String::new(),
                InputKind::ShortcutName { editing_index: None },
            );
    }

    fn start_shortcut_edit(&mut self, idx: usize) {
        let Some(s) = self.shortcuts.entries.get(idx).cloned() else { return };
        self.popup = text_input(
                "edit shortcut — name",
                "name:",
                s.name,
                InputKind::ShortcutName { editing_index: Some(idx) },
            );
    }

    fn copy_paths_to_clipboard(&mut self) {
        let paths = match self.active_pane() {
            Some(p) => p.target_paths(),
            None => return,
        };
        if paths.is_empty() {
            self.message = Some("nothing to copy".into());
            return;
        }
        let Some(cb) = self.clipboard.as_mut() else {
            self.message = Some("clipboard unavailable".into());
            return;
        };
        let text = paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n");
        match cb.set_text(text) {
            Ok(()) => self.message = Some(format!("◂ copied {} path(s) to clipboard", paths.len())),
            Err(e) => self.message = Some(format!("clipboard error: {}", e)),
        }
    }

    fn copy_file_refs_to_clipboard(&mut self) {
        let paths = match self.active_pane() {
            Some(p) => p.target_paths(),
            None => return,
        };
        if paths.is_empty() {
            self.message = Some("nothing to copy".into());
            return;
        }
        match os_clipboard_file_refs(&paths) {
            Ok(()) => self.message = Some(format!("◂ copied {} file ref(s) to clipboard", paths.len())),
            Err(e) => self.message = Some(format!("file-ref clipboard failed: {}", e)),
        }
    }

    fn copy_shortcut_target_to_clipboard(&mut self, idx: usize) {
        let Some(entry) = self.shortcuts.entries.get(idx).cloned() else { return };
        let Some(cb) = self.clipboard.as_mut() else {
            self.message = Some("clipboard unavailable".into());
            return;
        };
        match cb.set_text(entry.target.clone()) {
            Ok(()) => self.message = Some(format!("◂ copied: {}", truncate(&entry.target, 50))),
            Err(e) => self.message = Some(format!("clipboard error: {}", e)),
        }
    }

    fn execute_shortcut(&mut self, idx: usize) -> Result<()> {
        let Some(entry) = self.shortcuts.entries.get(idx).cloned() else { return Ok(()) };
        let target = entry.target.clone();

        // URL?
        if target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("file://")
        {
            let _ = os_open_string(&target);
            self.message = Some(format!("◂ {}", entry.name));
            return Ok(());
        }

        let path = expand_tilde(Path::new(&target));

        // macOS .app bundles are technically directories. Always hand them to
        // `open` so the app launches instead of cd-ing into the package.
        let is_app_bundle = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("app"))
            .unwrap_or(false);
        if is_app_bundle && path.exists() {
            match os_open(&path) {
                Ok(()) => self.message = Some(format!("◂ {}", entry.name)),
                Err(e) => self.message = Some(format!("shortcut failed: {}", e)),
            }
            return Ok(());
        }

        // Plain directory → navigate.
        if path.is_dir() {
            if let Some(p) = self.active_pane_mut() {
                p.jump_to(path)?;
            }
            self.message = Some(format!("◂ {}", entry.name));
            return Ok(());
        }

        // File or other existing entity → OS default.
        if path.exists() {
            let _ = os_open(&path);
            self.message = Some(format!("◂ {}", entry.name));
            return Ok(());
        }

        // Fallback: hand off the raw string to the OS opener (e.g. unknown protocols).
        match os_open_string(&target) {
            Ok(()) => self.message = Some(format!("◂ {}", entry.name)),
            Err(e) => self.message = Some(format!("shortcut failed: {}", e)),
        }
        Ok(())
    }

    // ------- History -------
    fn start_history(&mut self) {
        let entries = self.active_pane().map(|p| p.history.clone()).unwrap_or_default();
        if entries.is_empty() {
            self.message = Some("no history yet".into());
            return;
        }
        self.popup = Popup::History { entries, cursor: 0 };
    }

    fn finish_history(&mut self) -> Result<()> {
        let popup = std::mem::replace(&mut self.popup, Popup::None);
        let (entries, cursor) = if let Popup::History { entries, cursor } = popup {
            (entries, cursor)
        } else { return Ok(()) };
        let Some(target) = entries.get(cursor).cloned() else { return Ok(()) };
        if let Some(p) = self.active_pane_mut() {
            p.jump_to(target)?;
        }
        Ok(())
    }

    // ------- Incremental filter -------
    /// Start filtering, seeded with the pane's current filter so `/` reopens
    /// and edits an existing narrowing rather than discarding it.
    fn start_filter(&mut self) {
        self.filter_buffer = self.active_pane().map(|p| p.filter.clone()).unwrap_or_default();
        self.mode = Mode::Filter;
    }

    /// Push the buffer into the pane, narrowing the listing as the user types.
    fn apply_filter_buffer(&mut self) {
        let buf = self.filter_buffer.clone();
        if let Some(p) = self.active_pane_mut() {
            p.set_filter(buf);
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            // Esc abandons the narrowing entirely and restores the full list.
            KeyCode::Esc => {
                self.filter_buffer.clear();
                if let Some(p) = self.active_pane_mut() {
                    p.clear_filter();
                }
                self.mode = Mode::Normal;
            }
            // Enter keeps the filter applied and returns to normal keys, so the
            // narrowed list can be marked and operated on.
            KeyCode::Enter => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                self.filter_buffer.pop();
                self.apply_filter_buffer();
            }
            KeyCode::Up => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(-1); }
            }
            KeyCode::Down => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(1); }
            }
            KeyCode::Char(c) => {
                self.filter_buffer.push(c);
                self.apply_filter_buffer();
            }
            _ => {}
        }
        Ok(())
    }

    // ------- SSH -------

    /// Hosts matching the picker's current filter, as `(index, host)`.
    fn ssh_matches(&self, filter: &str) -> Vec<(usize, &cian_lua::SshHost)> {
        let needle = filter.to_lowercase();
        self.config
            .ssh_hosts
            .iter()
            .enumerate()
            .filter(|(_, h)| {
                needle.is_empty()
                    || h.name.to_lowercase().contains(&needle)
                    || h.host.to_lowercase().contains(&needle)
            })
            .collect()
    }

    fn start_ssh(&mut self) {
        if self.config.ssh_hosts.is_empty() {
            self.popup = Popup::Notice {
                lines: vec![
                    "No SSH hosts configured.".to_string(),
                    String::new(),
                    "Declare them in init.lua:".to_string(),
                    String::new(),
                    "  cian.ssh({".to_string(),
                    "    users = { \"root\", \"deploy\" },".to_string(),
                    "    hosts = {".to_string(),
                    "      { name = \"web1\", host = \"10.0.1.11\" },".to_string(),
                    "    },".to_string(),
                    "  })".to_string(),
                ],
            };
            return;
        }
        self.popup = Popup::SshHosts { cursor: 0, filter: String::new() };
    }

    /// Connect as `user` to host index `idx`, by typing the command into the
    /// shell panel.
    ///
    /// Typing it into a shell rather than spawning `ssh` directly means the
    /// user's own shell config and agent apply, and when the session ends the
    /// tab drops back to a local prompt instead of closing.
    fn ssh_connect(&mut self, idx: usize, user: &str) {
        let Some(h) = self.config.ssh_hosts.get(idx) else { return };
        let Some(u) = h.users.iter().find(|u| u.name == user) else { return };
        let mut cmd = format!("ssh {}@{}", u.name, h.host);
        if let Some(p) = h.port {
            cmd.push_str(&format!(" -p {}", p));
        }
        let label = format!("{}@{}", u.name, h.name);
        // Resolved before the command is sent so a slow `password_cmd` cannot
        // make us miss the prompt.
        let secret = u.secret();
        self.run_in_shell(cmd);
        match secret {
            Some(s) => {
                self.pending_auth =
                    Some(PendingAuth { secret: s, deadline: Instant::now() + AUTH_WINDOW });
                self.message = Some(format!("→ {} (sending password on prompt)", label));
            }
            None => self.message = Some(format!("→ {}", label)),
        }
    }

    /// Send the held password if ssh is now asking for one.
    ///
    /// Returns true if the UI should repaint. The secret is written straight to
    /// the PTY and never logged, echoed, or put in `message`.
    fn poll_pending_auth(&mut self) -> bool {
        let Some(auth) = &self.pending_auth else { return false };
        if Instant::now() > auth.deadline {
            // Expired: keyed host, refused login, or a prompt we do not answer.
            self.pending_auth = None;
            return false;
        }
        // Nothing to look at until the command has actually been delivered.
        if self.pending_shell_input.is_some() {
            return false;
        }
        let asking = match self.shell.active_session() {
            Some(s) => match s.parser().lock() {
                Ok(p) => looks_like_password_prompt(&p.screen().contents()),
                Err(_) => false,
            },
            None => false,
        };
        if !asking {
            return false;
        }
        let Some(auth) = self.pending_auth.take() else { return false };
        if let Some(s) = self.shell.active_session_mut() {
            let mut line = auth.secret;
            line.push('\n');
            s.write_input(line.as_bytes());
        }
        true
    }

    /// Send a command line to the shell panel, starting the shell if needed.
    fn run_in_shell(&mut self, mut cmd: String) {
        cmd.push('\n');
        let cwd = self.shell_cwd();
        self.shell.ensure(&cwd);
        self.focus(FocusedPane::Shell);
        match self.shell.active_session_mut() {
            Some(s) => s.write_input(cmd.as_bytes()),
            // Still spawning: hand it to `poll_pending`'s follow-up.
            None => self.pending_shell_input = Some(cmd),
        }
    }

    /// Deliver a command queued while the shell was still starting.
    fn flush_pending_shell_input(&mut self) {
        let Some(cmd) = self.pending_shell_input.take() else { return };
        match self.shell.active_session_mut() {
            Some(s) => s.write_input(cmd.as_bytes()),
            // Not ready yet — put it back and try again next tick.
            None => self.pending_shell_input = Some(cmd),
        }
    }

    // ------- Sorting -------
    fn start_sort_picker(&mut self) {
        // Open on the pane's current key, so the picker shows where you are.
        let cur = self
            .active_pane()
            .and_then(|p| SortKey::ALL.iter().position(|k| *k == p.sort.key))
            .unwrap_or(0);
        self.popup = Popup::SortPicker { cursor: cur };
    }

    /// Apply a sort key. Choosing the key that is already active flips the
    /// direction, which is how column headers behave everywhere else.
    fn apply_sort_key(&mut self, key: SortKey) {
        let Some(p) = self.active_pane_mut() else { return };
        let reverse = if p.sort.key == key { !p.sort.reverse } else { false };
        p.set_sort(Sort { key, reverse });
        let arrow = if reverse { "descending" } else { "ascending" };
        self.message = Some(format!("sorted by {} ({})", key.label(), arrow));
    }

    /// Note a directory as a copy/move destination.
    ///
    /// Most transfers go to the other pane, but the ones that do not tend to
    /// repeat — a build output, a share, a scratch folder — and retyping the
    /// path each time is the tedious part.
    fn remember_dest(&mut self, dest: &Path) {
        self.dest_history.retain(|p| p != dest);
        self.dest_history.insert(0, dest.to_path_buf());
        self.dest_history.truncate(DEST_HISTORY_CAP);
    }

    /// Offer somewhere other than the opposite pane to send the selection.
    fn start_dest_picker(&mut self, op: PendingOp) {
        let targets = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if targets.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        self.popup = Popup::DestPicker { op, targets, cursor: 0 };
    }

    /// Rows of the destination picker: the opposite pane first, then history.
    fn dest_choices(&self) -> Vec<(String, PathBuf)> {
        let mut out = Vec::new();
        if let Some(other) = self.opposite_pane_cwd() {
            out.push(("other pane".to_string(), other));
        }
        for p in &self.dest_history {
            if out.iter().any(|(_, q)| q == p) {
                continue;
            }
            out.push(("recent".to_string(), p.clone()));
        }
        out
    }

    // ------- Looking inside things -------

    /// F3: show what is in the highlighted entry.
    ///
    /// One key for both because the question is the same — "what is in here" —
    /// and the answer's shape follows from the file: an archive lists its
    /// members, anything else is read.
    fn look_inside(&mut self) {
        let Some(entry) = self.active_pane().and_then(|p| p.selected().cloned()) else {
            self.message = Some("nothing selected".into());
            return;
        };
        if entry.is_dir {
            self.message = Some("that is a directory — Enter to go in".into());
            return;
        }
        if cian_core::archive::is_archive(&entry.path) {
            match cian_core::archive::list(&entry.path) {
                Ok(members) => {
                    self.popup = Popup::Archive {
                        path: entry.path,
                        members,
                        cursor: 0,
                        scroll: 0,
                    };
                    return;
                }
                // Named like an archive but unreadable as one: fall through to
                // the viewer rather than refusing outright.
                Err(e) => self.message = Some(format!("not a readable archive: {}", e)),
            }
        }
        match cian_core::viewer::view_file(&entry.path) {
            Ok(view) => {
                self.popup = Popup::Viewer {
                    title: entry.name.clone(),
                    view,
                    scroll: 0,
                }
            }
            Err(e) => self.message = Some(format!("cannot view: {}", e)),
        }
    }

    /// Compare the file under the left pane's cursor with the right pane's.
    ///
    /// Deliberately not "the focused pane against the other one": the whole
    /// gesture is to put A on the left and B on the right, and which pane the
    /// cursor happens to be in at the moment of pressing the key should not
    /// silently swap the two sides of the result.
    fn open_diff(&mut self) {
        let pick = |t: &PaneTabs| t.active_ref().selected().cloned();
        let (Some(a), Some(b)) = (pick(&self.left), pick(&self.right)) else {
            self.message = Some("select a file in each pane to compare".into());
            return;
        };
        if a.is_dir || b.is_dir {
            self.message = Some("directories cannot be compared, only files".into());
            return;
        }
        match cian_core::diff::diff_files(&a.path, &b.path) {
            Ok(result) => {
                let folded = cian_core::diff::fold(&result.rows, cian_core::diff::CONTEXT);
                self.popup = Popup::Diff {
                    left: a.name.clone(),
                    right: b.name.clone(),
                    result,
                    folded,
                    // Folded to begin with: the differences are what was asked
                    // for, and on two near-identical files the unfolded view
                    // opens on a screen of agreement.
                    fold: true,
                    scroll: 0,
                };
            }
            Err(e) => self.message = Some(format!("cannot compare: {}", e)),
        }
    }

    /// Pull members out of the open archive into the opposite pane.
    fn extract_from_archive(&mut self, all: bool) {
        let Popup::Archive { path, members, cursor, .. } = &self.popup else { return };
        let (path, chosen) = (
            path.clone(),
            if all {
                Vec::new()
            } else {
                match members.get(*cursor) {
                    Some(m) => vec![m.name.clone()],
                    None => return,
                }
            },
        );
        let Some(dest) = self.opposite_pane_cwd() else {
            self.message = Some("no destination pane".into());
            return;
        };
        self.popup = Popup::None;
        self.remember_dest(&dest);
        self.start_op("extracting", move |ctl| {
            cian_core::archive::extract(&path, &chosen, &dest, ctl)
        });
    }

    // ------- Hidden files, attributes, checksums -------
    fn toggle_hidden(&mut self) {
        let Some(p) = self.active_pane_mut() else { return };
        let show = !p.show_hidden;
        p.set_show_hidden(show);
        self.message =
            Some(if show { "showing dotfiles".into() } else { "hiding dotfiles".to_string() });
    }

    fn show_attributes(&mut self) {
        let paths = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        let mut lines = Vec::new();
        for path in paths.iter().take(20) {
            let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match cian_core::attrs::read_attrs(path) {
                Ok(a) => {
                    let owner = a.owner.as_ref().map(|o| format!("   owner {}", o)).unwrap_or_default();
                    lines.push(format!("{:<28} {}{}", truncate(&name, 28), a.describe(), owner));
                }
                Err(e) => lines.push(format!("{:<28} {}", truncate(&name, 28), e)),
            }
        }
        if paths.len() > 20 {
            lines.push(format!("... and {} more", paths.len() - 20));
        }
        lines.push(String::new());
        lines.push("change with  :chmod 644   or  :readonly on|off".to_string());
        self.popup = Popup::Notice { lines };
    }

    /// Checksum the selection on a worker thread — the files worth hashing are
    /// the big ones, which is exactly when doing it inline would freeze.
    fn start_hash(&mut self, kind: cian_core::attrs::HashKind) {
        let paths: Vec<PathBuf> = self
            .active_pane()
            .map(|p| p.target_paths())
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p.is_file())
            .collect();
        if paths.is_empty() {
            self.message = Some("no files selected".into());
            return;
        }
        self.start_op("hashing", move |ctl| {
            let mut report = OpReport::default();
            let total = paths.len();
            for (i, path) in paths.iter().enumerate() {
                if ctl.cancel.load(Ordering::Relaxed) {
                    break;
                }
                let p = cian_core::progress::Progress {
                    files_done: i,
                    files_total: total,
                    current: path.display().to_string(),
                    ..Default::default()
                };
                (ctl.on_progress)(&p);
                match cian_core::attrs::hash_file(path, kind, ctl.cancel) {
                    Ok(Some(sum)) => {
                        let name = path
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        // Carried on the report so the result survives to be
                        // shown; there is no other channel back.
                        report.note_error(format!("{}  {}  {}", kind.label(), sum, name));
                    }
                    Ok(None) => break,
                    Err(e) => report.note_error(format!("{}: {}", path.display(), e)),
                }
            }
            report
        });
    }

    fn set_attr_command(&mut self, arg: &str) {
        let paths = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        let mut ok = 0;
        let mut err = None;
        for path in &paths {
            match cian_core::attrs::set_mode(path, arg) {
                Ok(()) => ok += 1,
                Err(e) => {
                    err = Some(e.to_string());
                    break;
                }
            }
        }
        self.reload_both();
        self.message = Some(match err {
            Some(e) => format!("chmod failed: {}", e),
            None => format!("chmod {} on {} item(s)", arg, ok),
        });
    }

    fn set_readonly_command(&mut self, on: bool) {
        let paths = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        let mut ok = 0;
        for path in &paths {
            if cian_core::attrs::set_readonly(path, on).is_ok() {
                ok += 1;
            }
        }
        self.reload_both();
        self.message = Some(format!("read-only {} on {} item(s)", if on { "set" } else { "cleared" }, ok));
    }

    // ------- Recursive search -------
    fn start_find_prompt(&mut self) {
        self.popup = text_input(
                "find (recursive)",
                "name contains   (Ctrl+V paste, Ctrl+U clear):",
                String::new(),
                InputKind::FindRecursive,
            );
    }

    fn start_grep_prompt(&mut self) {
        self.popup = text_input(
                "grep (recursive)",
                "text inside files   (Ctrl+V paste, Ctrl+U clear):",
                String::new(),
                InputKind::GrepRecursive,
            );
    }

    /// Walk the tree below the focused pane on a worker thread.
    fn start_find(&mut self, needle: &str, mode: cian_core::search::Mode) {
        let Some(root) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        let mut query = cian_core::search::Query::new(needle);
        query.mode = mode;
        query.include_hidden =
            self.active_pane().map(|p| p.show_hidden).unwrap_or(false);
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let worker_cancel = Arc::clone(&cancel);
        let worker_root = root.clone();
        std::thread::spawn(move || {
            let mut on_hit = |h: cian_core::search::Hit| {
                let _ = tx.send(FindMsg::Hit(h));
            };
            let outcome =
                cian_core::search::search(&worker_root, &query, &worker_cancel, &mut on_hit);
            let _ = tx.send(FindMsg::Done(outcome));
        });
        self.find_job = Some(FindJob {
            rx,
            cancel,
            root_label: root
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| root.display().to_string()),
            query: needle.to_string(),
            mode,
            done: None,
        });
        self.popup = Popup::FindResults { hits: Vec::new(), cursor: 0, scroll: 0 };
    }

    /// Collect whatever the search has produced. Returns true to repaint.
    fn poll_find_job(&mut self) -> bool {
        let Some(job) = &mut self.find_job else { return false };
        let mut changed = false;
        let mut batch = Vec::new();
        loop {
            match job.rx.try_recv() {
                Ok(FindMsg::Hit(h)) => batch.push(h),
                Ok(FindMsg::Done(o)) => {
                    job.done = Some(o);
                    changed = true;
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if job.done.is_none() {
                        job.done = Some(cian_core::search::Outcome::Complete);
                        changed = true;
                    }
                    break;
                }
            }
        }
        if !batch.is_empty() {
            changed = true;
            if let Popup::FindResults { hits, .. } = &mut self.popup {
                hits.extend(batch);
            }
        }
        changed
    }

    /// Go to the highlighted result: into the directory, or onto the file.
    fn open_find_hit(&mut self) -> Result<()> {
        let Popup::FindResults { hits, cursor, .. } = &self.popup else { return Ok(()) };
        let Some(hit) = hits.get(*cursor).cloned() else { return Ok(()) };
        self.popup = Popup::None;
        self.stop_find();

        let (dir, name) = if hit.is_dir {
            (hit.path.clone(), None)
        } else {
            match hit.path.parent() {
                Some(p) => (p.to_path_buf(), Some(hit.path.clone())),
                None => return Ok(()),
            }
        };
        if let Some(p) = self.active_pane_mut() {
            p.jump_to(dir)?;
            if let Some(target) = name {
                if let Some(i) = p.entries.iter().position(|e| e.path == target) {
                    p.cursor = i;
                }
            }
        }
        self.message = Some(format!("→ {}", hit.rel.display()));
        Ok(())
    }

    fn stop_find(&mut self) {
        if let Some(job) = self.find_job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    // ------- Jump to a typed path -------
    fn start_jump_path(&mut self) {
        // Seed with the current directory: most jumps are edits of where you
        // already are, and it doubles as a reminder of the expected form.
        let here = self.active_pane().map(|p| p.cwd.display().to_string()).unwrap_or_default();
        self.popup = text_input(
                "go to path",
                "directory to enter, or file to open:",
                here,
                InputKind::JumpPath,
            );
    }

    /// Enter a typed directory, or open a typed file with its usual program.
    fn finish_jump_path(&mut self, raw: &str) -> Result<()> {
        let raw = raw.trim();
        if raw.is_empty() {
            self.message = Some("cancelled".into());
            return Ok(());
        }
        let path = expand_path(raw);
        if !path.exists() {
            self.message = Some(format!("no such path: {}", path.display()));
            return Ok(());
        }
        if path.is_dir() {
            if let Some(p) = self.active_pane_mut() {
                p.jump_to(path.clone())?;
            }
            self.message = Some(format!("→ {}", path.display()));
            return Ok(());
        }
        // A file: put the cursor on it in its own directory, then open it the
        // same way Enter would — including any init.lua on_open handler.
        if let Some(parent) = path.parent().map(|p| p.to_path_buf()) {
            if let Some(p) = self.active_pane_mut() {
                let _ = p.jump_to(parent);
                if let Some(i) = p.entries.iter().position(|e| e.path == path) {
                    p.cursor = i;
                }
            }
        }
        self.open_externally();
        Ok(())
    }

    /// Open the context menu beside the highlighted entry, as though it had
    /// been right-clicked.
    fn open_menu_at_cursor(&mut self) {
        let rect = match self.focused {
            FocusedPane::Left => self.layout_rects.left,
            FocusedPane::Right => self.layout_rects.right,
            FocusedPane::Shell => self.layout_rects.shell,
        };
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        // Anchor on the cursor's row so the menu appears next to what it acts
        // on, the same as a right-click would.
        let view_h = rect.height.saturating_sub(2);
        let offset = self
            .active_pane()
            .map(|p| {
                let first = p.cursor.saturating_sub(view_h.saturating_sub(1) as usize);
                (p.cursor - first) as u16
            })
            .unwrap_or(0);
        let row = (rect.y + 1 + offset).min(rect.y + rect.height.saturating_sub(1));
        self.open_context_menu(rect.x + 4, row);
    }

    // ------- Manual -------
    fn open_manual(&mut self) {
        self.popup = Popup::Manual { lines: manual_lines(&self.keymap), scroll: 0 };
    }

    // ------- Quit confirmation -------
    fn start_quit_confirm(&mut self) {
        self.popup = Popup::ConfirmQuit;
    }

    /// Perform a confirmed close (shell split pane or file tab).
    fn execute_close(&mut self, target: CloseTarget) {
        match target {
            // Shrink the pane away first; the removal happens when the
            // transition lands (or immediately if animation is off).
            CloseTarget::ShellPane => self.close_shell_pane_animated(),
            CloseTarget::FileTab(pane) => {
                let tabs = match pane {
                    FocusedPane::Left => &mut self.left,
                    FocusedPane::Right => &mut self.right,
                    FocusedPane::Shell => return,
                };
                tabs.close_active();
            }
        }
    }

    fn jump_to_next_match(&mut self, forward: bool) {
        let Some(query) = self.last_search_query.clone() else {
            self.message = Some("no previous search".into());
            return;
        };
        let ql = query.to_lowercase();
        let Some(p) = self.active_pane_mut() else { return };
        let n = p.entries.len();
        if n == 0 { return; }
        let start = p.cursor;
        let mut i = if forward { (start + 1) % n } else { (start + n - 1) % n };
        for _ in 0..n {
            if p.entries[i].name.to_lowercase().contains(&ql) {
                p.cursor = i;
                return;
            }
            i = if forward { (i + 1) % n } else { (i + n - 1) % n };
        }
        self.message = Some(format!("pattern not found: {}", query));
    }

    /// Run a file operation on a worker thread, showing a progress popup.
    fn start_op<F>(&mut self, label: &'static str, work: F)
    where
        F: FnOnce(&mut cian_core::progress::Ctl) -> OpReport + Send + 'static,
    {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let worker_cancel = Arc::clone(&cancel);
        let worker_tx = tx.clone();
        std::thread::spawn(move || {
            // Rate-limit the updates: a chunked copy calls back on every
            // megabyte, and forwarding all of that would flood the channel
            // and repaint far more often than a screen can show.
            let mut last = Instant::now() - Duration::from_secs(1);
            let mut on_progress = |p: &cian_core::progress::Progress| {
                if last.elapsed() >= Duration::from_millis(60) {
                    last = Instant::now();
                    let _ = worker_tx.send(OpMsg::Tick(p.clone()));
                }
            };
            let mut ctl = cian_core::progress::Ctl {
                cancel: &worker_cancel,
                on_progress: &mut on_progress,
            };
            let report = work(&mut ctl);
            let _ = tx.send(OpMsg::Done(report));
        });
        self.op_job = Some(OpJob {
            rx,
            cancel,
            label,
            latest: cian_core::progress::Progress::default(),
            started: Instant::now(),
        });
        self.popup = Popup::None;
    }

    /// Drain worker updates. Returns true if the UI should repaint.
    fn poll_op_job(&mut self) -> bool {
        let Some(job) = &mut self.op_job else { return false };
        let mut changed = false;
        let mut finished = None;
        loop {
            match job.rx.try_recv() {
                Ok(OpMsg::Tick(p)) => {
                    job.latest = p;
                    changed = true;
                }
                Ok(OpMsg::Done(r)) => {
                    finished = Some(r);
                    changed = true;
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                // The worker vanished without reporting; do not wait forever.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finished = Some(OpReport::default());
                    changed = true;
                    break;
                }
            }
        }
        if let Some(report) = finished {
            let cancelled = self.op_job.as_ref().map(|j| j.cancel.load(Ordering::Relaxed));
            self.op_job = None;
            self.reload_both();
            if let Some(p) = self.active_pane_mut() {
                p.clear_marks();
            }
            self.flash(self.focused);
            if cancelled == Some(true) {
                self.message = Some(format!(
                    "cancelled — {} done before stopping",
                    report.ok
                ));
            } else {
                self.show_op_report(&report);
            }
        }
        changed
    }

    /// Reload any pane whose directory changed underneath it.
    ///
    /// cian only ever reloaded after its own actions, so a file created by
    /// something else — a build, a download, a colleague's sync — simply never
    /// appeared. Returns true if anything was refreshed.
    fn poll_external_changes(&mut self) -> bool {
        if self.last_watch.elapsed() < WATCH_INTERVAL {
            return false;
        }
        self.last_watch = Instant::now();
        // Not while an operation runs: it will reload at the end anyway, and
        // re-reading a directory being written to would just fight it.
        if self.op_job.is_some() {
            return false;
        }
        let mut changed = false;
        for tabs in [&mut self.left, &mut self.right] {
            let pane = tabs.active_mut();
            if pane.is_stale() {
                let _ = pane.reload();
                changed = true;
            }
        }
        changed
    }

    fn cancel_op_job(&mut self) {
        if let Some(job) = &self.op_job {
            job.cancel.store(true, Ordering::Relaxed);
            self.message = Some("stopping…".into());
        }
    }

    fn finish_transfer(&mut self, conflict: Conflict) -> Result<()> {
        let popup = std::mem::replace(&mut self.popup, Popup::None);
        let Popup::ConfirmTransfer { op, targets, dest } = popup else { return Ok(()) };
        self.push_clipboard(&targets);
        self.remember_dest(&dest);
        let label = match op {
            PendingOp::Copy => "copying",
            PendingOp::Move => "moving",
        };
        self.start_op(label, move |ctl| match op {
            PendingOp::Copy => cian_core::progress::copy_many(&targets, &dest, conflict, ctl),
            PendingOp::Move => cian_core::progress::move_many(&targets, &dest, conflict, ctl),
        });
        Ok(())
    }

    fn finish_delete(&mut self, mode: DeleteMode) -> Result<()> {
        let popup = std::mem::replace(&mut self.popup, Popup::None);
        let Popup::ConfirmDelete { targets } = popup else { return Ok(()) };
        if cian_core::log::enabled() {
            cian_core::log::log(&format!("delete {:?}: {} target(s)", mode, targets.len()));
        }
        self.start_op("deleting", move |ctl| {
            cian_core::progress::delete_many(&targets, mode, ctl)
        });
        Ok(())
    }

    fn show_op_report(&mut self, report: &OpReport) {
        if !report.errors.is_empty() {
            let mut lines = vec![format!(
                "{} ok · {} skipped · {} errors", report.ok, report.skipped, report.errors.len()
            )];
            lines.extend(report.errors.iter().take(8).cloned());
            if report.errors.len() > 8 {
                lines.push(format!("... and {} more", report.errors.len() - 8));
            }
            self.popup = Popup::Notice { lines };
        } else {
            self.message = Some(format!("done — {} ok · {} skipped", report.ok, report.skipped));
        }
    }

    fn finish_text_input(&mut self) -> Result<()> {
        let popup = std::mem::replace(&mut self.popup, Popup::None);
        let Popup::TextInput { buffer, kind, .. } = popup else { return Ok(()) };
        let name = buffer.trim().to_string();
        if name.is_empty() {
            self.message = Some("cancelled (empty name)".into());
            return Ok(());
        }
        let result = match &kind {
            InputKind::Rename { original } => {
                ops::rename_in_place(original, &name).map(|p| format!("renamed: {}", p.display()))
            }
            InputKind::NewFile { parent } => {
                ops::create_file(parent, &name).map(|p| format!("created: {}", p.display()))
            }
            InputKind::NewDir { parent } => {
                ops::create_dir(parent, &name).map(|p| format!("mkdir: {}", p.display()))
            }
            InputKind::JumpPath => return self.finish_jump_path(&name),
            InputKind::FindRecursive => {
                self.start_find(&name, cian_core::search::Mode::Name);
                return Ok(());
            }
            InputKind::GrepRecursive => {
                self.start_find(&name, cian_core::search::Mode::Content);
                return Ok(());
            }
            InputKind::DestPath { op, targets } => {
                let dest = expand_path(&name);
                if !dest.is_dir() {
                    self.message = Some(format!("not a directory: {}", dest.display()));
                    return Ok(());
                }
                self.popup =
                    Popup::ConfirmTransfer { op: *op, targets: targets.clone(), dest };
                return Ok(());
            }
            InputKind::ZipPassword { dest, sources } => {
                // An empty password here means "never mind the encryption".
                if name.is_empty() {
                    self.message = Some("zip cancelled".into());
                    return Ok(());
                }
                self.start_zip(dest.clone(), sources.clone(), Some(name));
                return Ok(());
            }
            InputKind::TransferAs { op, src, dest_dir } => {
                let target = dest_dir.join(&name);
                let verb = if *op == PendingOp::Move { "mv" } else { "cp" };
                let res = match op {
                    PendingOp::Move => std::fs::rename(src, &target).map_err(anyhow::Error::from),
                    PendingOp::Copy => cian_core::ops::copy_one(src, dest_dir, Conflict::Overwrite)
                        .and_then(|_| {
                            let landed =
                                dest_dir.join(src.file_name().unwrap_or_default());
                            if landed != target {
                                std::fs::rename(&landed, &target)?;
                            }
                            Ok(())
                        }),
                };
                match res {
                    Ok(_) => {
                        self.reload_both();
                        self.message = Some(format!("{} → {}", verb, target.display()));
                    }
                    Err(e) => self.message = Some(format!("{}: {}", verb, e)),
                }
                return Ok(());
            }
            InputKind::ShortcutName { editing_index } => {
                // chain into the next step: target input. A new shortcut
                // defaults to the entry under the cursor, which is what you
                // are almost always bookmarking — and saves typing a path that
                // is already on screen.
                // A target chosen elsewhere (the history list) wins; otherwise
                // default to the entry under the cursor, which is what you are
                // almost always bookmarking.
                let here = self
                    .pending_shortcut_target
                    .take()
                    .or_else(|| {
                        self.active_pane()
                            .and_then(|p| p.selected().map(|e| e.path.display().to_string()))
                    })
                    .unwrap_or_default();
                let prev_target = editing_index
                    .and_then(|i| self.shortcuts.entries.get(i).map(|s| s.target.clone()))
                    .unwrap_or(here);
                self.popup = text_input(
                    "shortcut — target",
                    "URL / path / app   (Ctrl+V paste, Ctrl+U clear):",
                    prev_target,
                    InputKind::ShortcutTarget { editing_index: *editing_index, name },
                );
                return Ok(());
            }
            InputKind::ShortcutTarget { editing_index, name: stored_name } => {
                let target = name; // `name` here is actually the trimmed buffer
                if target.is_empty() {
                    self.message = Some("cancelled (empty target)".into());
                    return Ok(());
                }
                let entry = Shortcut { name: stored_name.clone(), target };
                match editing_index {
                    Some(i) => {
                        if let Some(s) = self.shortcuts.entries.get_mut(*i) { *s = entry; }
                    }
                    None => self.shortcuts.entries.push(entry),
                }
                match self.shortcuts.save() {
                    Ok(()) => self.message = Some("shortcut saved".into()),
                    Err(e) => self.popup = Popup::Notice { lines: vec![format!("save failed: {}", e)] },
                }
                return Ok(());
            }
        };
        if let Some(t) = self.active_file_tabs_mut() { let _ = t.active_mut().reload(); }
        match result {
            Ok(msg) => self.message = Some(msg),
            Err(e) => self.popup = Popup::Notice { lines: vec![e.to_string()] },
        }
        Ok(())
    }

    // ------- Mouse -------
    fn handle_mouse(&mut self, ev: MouseEvent) {
        let (col, row) = (ev.column, ev.row);

        // A drag in progress owns the mouse until the button comes back up,
        // even if the pointer strays outside the border's grab zone.
        if let Some(d) = self.drag {
            match ev.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    self.set_divider_ratio(d, d.ratio_at(col, row));
                    return;
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.drag = None;
                    return;
                }
                _ => {}
            }
        }

        // Ignore everything else while a popup owns the screen.
        if !matches!(self.popup, Popup::None) {
            return;
        }

        let in_rect = |r: Rect| {
            r.width > 0 && r.height > 0
                && col >= r.x && col < r.x + r.width
                && row >= r.y && row < r.y + r.height
        };

        // Right-click focuses what was clicked, puts the cursor on the row
        // under the pointer, and opens the context menu there.
        if matches!(ev.kind, MouseEventKind::Down(MouseButton::Right)) {
            let target = if in_rect(self.layout_rects.left) {
                Some(FocusedPane::Left)
            } else if in_rect(self.layout_rects.right) {
                Some(FocusedPane::Right)
            } else if in_rect(self.layout_rects.shell) {
                Some(FocusedPane::Shell)
            } else {
                None
            };
            if let Some(t) = target {
                if self.focused != t {
                    self.focus(t);
                }
                match t {
                    // Act on the split pane under the pointer, not whichever
                    // happened to be active — otherwise a right-click on the
                    // left half colours the right one.
                    FocusedPane::Shell => self.select_shell_leaf_at(col, row),
                    _ => self.cursor_to_row(t, row),
                }
                self.open_context_menu(col, row);
            }
            return;
        }

        let pane_at = |col: u16, row: u16| -> Option<FocusedPane> {
            let hit = |r: Rect| {
                r.width > 0 && r.height > 0
                    && col >= r.x && col < r.x + r.width
                    && row >= r.y && row < r.y + r.height
            };
            if hit(self.layout_rects.left) {
                Some(FocusedPane::Left)
            } else if hit(self.layout_rects.right) {
                Some(FocusedPane::Right)
            } else if hit(self.layout_rects.shell) {
                Some(FocusedPane::Shell)
            } else {
                None
            }
        };

        // A file drag in progress owns the mouse until release.
        if self.file_drag.is_some() {
            match ev.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    let over = pane_at(col, row);
                    if let Some(d) = &mut self.file_drag {
                        d.moved = true;
                        d.over = over;
                    }
                    return;
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    let over = pane_at(col, row);
                    self.finish_file_drag(over, ev.modifiers);
                    return;
                }
                _ => {}
            }
        }

        if !matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }

        // Grabbing a border starts a resize instead of moving focus. Checked
        // first because the seam overlaps the panes' own border cells.
        if let Some(d) = self.dividers.iter().copied().find(|d| in_rect(d.zone)) {
            self.drag = Some(d);
            return;
        }

        match pane_at(col, row) {
            Some(FocusedPane::Shell) => {
                self.focus(FocusedPane::Shell);
                // Clicking a split should focus that split, as in any multiplexer.
                self.select_shell_leaf_at(col, row);
            }
            Some(pane) => {
                self.focus(pane);
                // Put the cursor on the row that was clicked, then arm a drag
                // from it. Whether it becomes a drag or stays a click is
                // decided on release.
                self.cursor_to_row(pane, row);
                let paths = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
                if !paths.is_empty() {
                    self.file_drag =
                        Some(FileDrag { from: pane, paths, over: Some(pane), moved: false });
                }
            }
            None => {}
        }
    }

    /// Resolve a finished drag.
    ///
    /// Dropping onto the other file pane transfers; onto the shell it types
    /// the paths, which is the closest thing to dragging a file into a
    /// terminal. Anything else — including a press and release in place, which
    /// is just a click — does nothing.
    fn finish_file_drag(&mut self, over: Option<FocusedPane>, mods: KeyModifiers) {
        let Some(drag) = self.file_drag.take() else { return };
        if !drag.moved {
            return;
        }
        let Some(target) = over else { return };
        if target == drag.from {
            return;
        }
        match target {
            FocusedPane::Shell => {
                let quoted: Vec<String> = drag
                    .paths
                    .iter()
                    .map(|p| {
                        let s = p.display().to_string();
                        // Quote only when needed, so the common case stays
                        // something you would have typed yourself.
                        if s.contains(' ') { format!("\"{}\"", s) } else { s }
                    })
                    .collect();
                let text = quoted.join(" ");
                self.focus(FocusedPane::Shell);
                let cwd = self.shell_cwd();
                self.shell.ensure(&cwd);
                match self.shell.active_session_mut() {
                    Some(s) => s.write_input(text.as_bytes()),
                    None => self.pending_shell_input = Some(text),
                }
                self.message = Some(format!("{} path(s) → shell", drag.paths.len()));
            }
            dest_pane => {
                let dest = match dest_pane {
                    FocusedPane::Left => self.left.active_ref().cwd.clone(),
                    FocusedPane::Right => self.right.active_ref().cwd.clone(),
                    FocusedPane::Shell => return,
                };
                // Shift means move, matching what every other file manager
                // does with a modifier on a drag.
                let op = if mods.contains(KeyModifiers::SHIFT) {
                    PendingOp::Move
                } else {
                    PendingOp::Copy
                };
                self.popup = Popup::ConfirmTransfer { op, targets: drag.paths, dest };
            }
        }
    }

    // ------- Transitions -------

    fn anim_enabled(&self) -> bool {
        !self.anim_dur.is_zero()
    }

    /// Toggle full-window zoom of the focused surface, animating between the
    /// surface's pane rect and the whole layout area.
    fn toggle_zoom(&mut self) {
        // The full area is the union of everything currently laid out; derived
        // rather than stored so it stays right at any window size. While
        // zoomed this is just the focused surface, which already fills it.
        let full = union_rect(
            union_rect(self.layout_rects.left, self.layout_rects.right),
            self.layout_rects.shell,
        );
        if self.zoomed {
            // Shrink back into where the surface came from. Taken from
            // `zoom_return` because `layout_rects` now describes the zoomed
            // layout; reading the focused pane's rect here would give the full
            // area again, making the transition a no-op.
            let back = self.zoom_return.take();
            self.zoomed = false;
            if let Some(back) = back {
                // A resize while zoomed can leave the remembered rect outside
                // the window; snapping is better than flying in from nowhere.
                let fits = back.x + back.width <= full.x + full.width
                    && back.y + back.height <= full.y + full.height;
                if fits && back.width > 0 && full.width > 0 {
                    self.start_anim(AnimKind::Zoom { from: full, to: back });
                }
            }
        } else {
            let pane_rect = match self.focused {
                FocusedPane::Left => self.layout_rects.left,
                FocusedPane::Right => self.layout_rects.right,
                FocusedPane::Shell => self.layout_rects.shell,
            };
            self.zoomed = true;
            self.zoom_return = Some(pane_rect);
            if pane_rect.width > 0 && full.width > 0 {
                self.start_anim(AnimKind::Zoom { from: pane_rect, to: full });
            }
        }
    }

    fn start_anim(&mut self, kind: AnimKind) {
        if !self.anim_enabled() {
            return;
        }
        self.anim = Some(Anim { kind, start: Instant::now(), dur: self.anim_dur });
    }

    /// What the renderer should override this frame.
    fn anim_override(&self) -> AnimOverride {
        match self.anim {
            Some(a) => match a.kind {
                AnimKind::Ratio { target, from, to } => {
                    let t = a.progress();
                    let r = (from as f32 + (to as f32 - from as f32) * t).round() as u16;
                    AnimOverride { ratio: Some((target, r)), freeze_pty: true }
                }
                AnimKind::Zoom { .. } => AnimOverride { ratio: None, freeze_pty: true },
            },
            None => AnimOverride::default(),
        }
    }

    /// Land the current transition now, applying any deferred work. Called
    /// when the timer expires and whenever the user presses a key, so input is
    /// never held up waiting for an animation.
    fn finish_anim(&mut self) {
        if self.anim.take().is_none() {
            return;
        }
        if let Some(close) = self.anim_then.take() {
            self.apply_pending_close(close);
        }
    }

    /// Perform a close that was deferred until its shrink animation finished.
    fn apply_pending_close(&mut self, close: PendingClose) {
        match close {
            PendingClose::ShellPane => {
                let empty = self.shell.close_active_pane();
                if empty {
                    let back = self.last_file_pane;
                    self.focus(back);
                }
            }
        }
    }

    /// Begin closing the active shell split pane, shrinking it away first.
    /// Falls back to closing immediately when animation is off or the pane is
    /// the only one in its tab (nothing to shrink into).
    fn close_shell_pane_animated(&mut self) {
        let parent = self
            .shell
            .tabs
            .get(self.shell.active)
            .and_then(|t| t.parent_of(t.active));
        match (self.anim_enabled(), parent) {
            (true, Some((p, is_first))) => {
                let stored = match self
                    .shell
                    .tabs
                    .get(self.shell.active)
                    .and_then(|t| t.nodes.get(p))
                    .and_then(|n| n.as_ref())
                {
                    Some(Node::Split { ratio, .. }) => *ratio,
                    _ => 50,
                };
                // Drive the closing child's share to nothing.
                let to = if is_first { 0 } else { 100 };
                self.anim_then = Some(PendingClose::ShellPane);
                self.start_anim(AnimKind::Ratio {
                    target: DividerTarget::ShellSplit { tab: self.shell.active, node: p },
                    from: stored,
                    to,
                });
            }
            _ => self.apply_pending_close(PendingClose::ShellPane),
        }
    }

    /// Briefly highlight `pane` to show an operation landed there.
    fn flash(&mut self, pane: FocusedPane) {
        self.flash = Some((pane, Instant::now()));
    }

    /// How lit `pane` currently is, 1.0 right after a flash fading to 0.0.
    /// Returns 0.0 once the flash has expired.
    fn flash_level(&self, pane: FocusedPane) -> f32 {
        let Some((p, at)) = self.flash else { return 0.0 };
        if p != pane {
            return 0.0;
        }
        let e = at.elapsed().as_secs_f32();
        if e >= FLASH_SECS {
            0.0
        } else {
            1.0 - e / FLASH_SECS
        }
    }

    /// Whether a flash is still running and the UI should keep repainting.
    fn flash_active(&self) -> bool {
        self.flash.map(|(_, at)| at.elapsed().as_secs_f32() < FLASH_SECS).unwrap_or(false)
    }

    /// Which `pane_bg` slot a file pane uses.
    fn bg_slot(pane: FocusedPane) -> Option<usize> {
        match pane {
            FocusedPane::Left => Some(0),
            FocusedPane::Right => Some(1),
            // The shell's background lives on the split pane itself.
            FocusedPane::Shell => None,
        }
    }

    /// Make the shell split pane under the pointer the active one.
    ///
    /// Without this, clicking a pane focuses the panel but leaves the previous
    /// pane active, so anything acting on "the active pane" targets the wrong
    /// half of a split.
    fn select_shell_leaf_at(&mut self, col: u16, row: u16) {
        let hit = self.shell_leaves.iter().copied().find(|(_, _, r)| {
            r.width > 0 && r.height > 0
                && col >= r.x && col < r.x + r.width
                && row >= r.y && row < r.y + r.height
        });
        if let Some((tab, leaf, _)) = hit {
            self.shell.active = tab;
            if let Some(t) = self.shell.tabs.get_mut(tab) {
                t.active = leaf;
            }
        }
    }

    /// Move a file pane's cursor to the entry drawn at absolute screen `row`.
    /// Out-of-range rows (the border, or empty space past the last entry) leave
    /// the cursor alone rather than jumping somewhere arbitrary.
    fn cursor_to_row(&mut self, pane: FocusedPane, row: u16) {
        let rect = match pane {
            FocusedPane::Left => self.layout_rects.left,
            FocusedPane::Right => self.layout_rects.right,
            FocusedPane::Shell => return,
        };
        // The list starts one row in, past the top border.
        let Some(offset) = row.checked_sub(rect.y + 1) else { return };
        if offset >= rect.height.saturating_sub(2) {
            return;
        }
        let Some(p) = self.active_pane_mut() else { return };
        // The list scrolls, so the first visible row is not always entry 0.
        let view_h = rect.height.saturating_sub(2) as usize;
        let first = p.cursor.saturating_sub(view_h.saturating_sub(1)).min(
            p.entries.len().saturating_sub(view_h.min(p.entries.len())),
        );
        let idx = first + offset as usize;
        if idx < p.entries.len() {
            p.cursor = idx;
        }
    }

    fn open_context_menu(&mut self, col: u16, row: u16) {
        let mut items = Vec::new();
        if self.focused == FocusedPane::Shell {
            // A PTY owns its own screen, so the file operations make no sense
            // here. SSH leads: keys never reach the picker while the shell has
            // focus, so this menu is the only way to open it without first
            // leaving the shell — which is exactly where you want it.
            items.push(MenuItem::Ssh);
            items.push(MenuItem::Paste);
            items.push(MenuItem::Background);
        } else {
            items.push(MenuItem::Copy);
            items.push(MenuItem::Cut);
            // Always offered: it can also paste from the system clipboard, and
            // hiding it made a file just copied in Explorer look unpasteable.
            items.push(MenuItem::Paste);
            items.push(MenuItem::CopyToOther);
            items.push(MenuItem::MoveToOther);
            items.push(MenuItem::CopyToPath);
            items.push(MenuItem::Rename);
            items.push(MenuItem::Delete);
            items.push(MenuItem::Attributes);
            items.push(MenuItem::Hash);
            items.push(MenuItem::Compare);
            items.push(MenuItem::HiddenToggle);
            items.push(MenuItem::Ssh);
            items.push(MenuItem::Background);
        }
        // Always last, and present in every menu: the point of it is to be
        // findable when you cannot remember anything else — including in the
        // shell, where it is otherwise the only entry worth showing.
        items.push(MenuItem::Manual);
        self.popup = Popup::ContextMenu { items, cursor: 0, at: (col, row) };
    }

    /// Put the pane's targets into the file clipboard.
    fn clip_targets(&mut self, op: ClipOp) {
        let paths = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        let verb = match op {
            ClipOp::Copy => "copied",
            ClipOp::Cut => "cut",
        };
        self.message = Some(format!("{} {} item(s)", verb, paths.len()));
        self.file_clip = Some(FileClipboard { paths, op });
    }

    /// Paste the file clipboard into the focused pane's directory.
    fn paste_clip(&mut self) -> Result<()> {
        // cian's own register wins when set — it was filled deliberately, from
        // here. Otherwise fall back to the OS clipboard, so a file copied in
        // Explorer or Finder pastes as you would expect.
        let (clip, from_os) = match self.file_clip.clone() {
            Some(c) => (c, false),
            None => {
                let paths = os_clipboard_files();
                if paths.is_empty() {
                    self.message = Some("clipboard has no files".into());
                    return Ok(());
                }
                (FileClipboard { paths, op: ClipOp::Copy }, true)
            }
        };
        let dest = match self.focused {
            FocusedPane::Shell => self.shell_cwd(),
            _ => match self.active_pane() {
                Some(p) => p.cwd.clone(),
                None => return Ok(()),
            },
        };
        // Pasting into the directory the files already live in would be a
        // no-op at best and a self-overwrite at worst.
        if clip.paths.iter().any(|p| p.parent() == Some(dest.as_path())) {
            self.message = Some("already in this directory".into());
            return Ok(());
        }
        let report = match clip.op {
            ClipOp::Copy => ops::copy_many(&clip.paths, &dest, Conflict::Skip),
            ClipOp::Cut => ops::move_many(&clip.paths, &dest, Conflict::Skip),
        };
        // A cut is consumed by its paste; a copy stays available.
        if clip.op == ClipOp::Cut {
            self.file_clip = None;
        }
        self.reload_both();
        self.flash(self.focused);
        if from_os && report.errors.is_empty() {
            // Say where they came from, so "paste" is never ambiguous about
            // which of the two clipboards it just used.
            self.message =
                Some(format!("pasted {} item(s) from the system clipboard", report.ok));
            return Ok(());
        }
        self.show_op_report(&report);
        Ok(())
    }

    /// Reload both file panes; a paste can change either of them.
    fn reload_both(&mut self) {
        let _ = self.left.active_mut().reload();
        let _ = self.right.active_mut().reload();
    }

    fn run_menu_item(&mut self, item: MenuItem) -> Result<()> {
        self.popup = Popup::None;
        match item {
            MenuItem::Copy => self.clip_targets(ClipOp::Copy),
            MenuItem::Cut => self.clip_targets(ClipOp::Cut),
            // In the shell, "Paste" means the text on the clipboard goes to
            // the running program — the terminal sense of paste. Only in a file
            // pane does it mean the file clipboard.
            MenuItem::Paste if self.focused == FocusedPane::Shell => self.paste_text_to_shell(),
            MenuItem::Paste => return self.paste_clip(),
            MenuItem::CopyToOther => self.start_transfer(PendingOp::Copy),
            MenuItem::MoveToOther => self.start_transfer(PendingOp::Move),
            MenuItem::CopyToPath => self.start_dest_picker(PendingOp::Copy),
            MenuItem::Rename => self.start_rename(),
            MenuItem::Delete => self.start_delete(),
            MenuItem::Manual => self.open_manual(),
            MenuItem::Ssh => self.start_ssh(),
            MenuItem::HiddenToggle => self.toggle_hidden(),
            MenuItem::Attributes => self.show_attributes(),
            MenuItem::Hash => self.start_hash(cian_core::attrs::HashKind::Sha256),
            MenuItem::Compare => self.open_diff(),
            MenuItem::Background => {
                let pane = self.focused;
                let current = match pane {
                    FocusedPane::Shell => self.shell.active_pane_bg(),
                    _ => Self::bg_slot(pane).and_then(|s| self.pane_bg[s]),
                };
                let cur = current
                    .and_then(|c| PANE_BG_PRESETS.iter().position(|(_, p)| *p == Some(c)))
                    .unwrap_or(0);
                self.popup = Popup::ColorPicker { pane, cursor: cur };
            }
        }
        Ok(())
    }

    /// Apply a dragged border's new position to whichever split it divides.
    fn set_divider_ratio(&mut self, d: Divider, pct: u16) {
        match d.target {
            DividerTarget::Main => self.main_pct = pct,
            DividerTarget::Panes => self.panes_pct = pct,
            DividerTarget::ShellSplit { tab, node } => {
                if let Some(Node::Split { ratio, .. }) =
                    self.shell.tabs.get_mut(tab).and_then(|t| t.nodes.get_mut(node)).and_then(|n| n.as_mut())
                {
                    *ratio = pct;
                }
            }
        }
    }

    // ------- Key dispatch -------
    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // A running operation owns Esc: stopping it is the only thing anyone
        // wants from the keyboard while it is on screen.
        if self.op_job.is_some() {
            if key.code == KeyCode::Esc {
                self.cancel_op_job();
            }
            return Ok(());
        }
        if !matches!(self.popup, Popup::None) {
            return self.handle_popup_key(key);
        }
        // Ctrl+. shows the key manual. `?` does the same without needing the
        // kitty keyboard protocol (plain terminals cannot encode Ctrl+.), and
        // `:man` works everywhere. Full-screen shell apps keep both keys.
        if self.focused != FocusedPane::Shell
            && ((key.code == KeyCode::Char('.') && key.modifiers.contains(KeyModifiers::CONTROL))
                || (key.code == KeyCode::Char('?') && !key.modifiers.contains(KeyModifiers::CONTROL)))
        {
            self.open_manual();
            return Ok(());
        }
        // F12 toggles full-window zoom of the focused surface; Shift+F12 zooms
        // only the active split pane within the shell. While a full-screen app
        // runs in the shell, both are passed through to it.
        if key.code == KeyCode::F(12) {
            let shell_fullscreen =
                self.focused == FocusedPane::Shell && self.shell.active_modes().0;
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                if self.focused == FocusedPane::Shell && !shell_fullscreen {
                    self.shell.toggle_pane_zoom();
                    return Ok(());
                }
            } else if !shell_fullscreen {
                self.toggle_zoom();
                return Ok(());
            }
        }
        if self.mode == Mode::Command {
            return self.handle_command_key(key);
        }
        if self.mode == Mode::Filter {
            return self.handle_filter_key(key);
        }
        if self.focused == FocusedPane::Shell {
            return self.handle_shell_key(key);
        }
        if self.mode == Mode::Visual {
            return self.handle_visual_key(key);
        }
        self.handle_normal_key(key)
    }

    fn handle_popup_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(self.popup, Popup::TextInput { .. }) {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            // Ctrl+V pastes. Handled before the buffer is borrowed because it
            // needs the clipboard, which lives on `self`.
            if ctrl && matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V')) {
                let text = self.clipboard_text();
                if let Popup::TextInput { buffer, cursor, .. } = &mut self.popup {
                    match text {
                        // Paths and URLs are what get pasted here, and a
                        // trailing newline from `pwd` or a browser would
                        // otherwise end up inside the value. Inserted at the
                        // caret, not always the end.
                        Some(t) => insert_str_at(buffer, cursor, t.trim_end_matches(['\r', '\n'])),
                        None => self.message = Some("clipboard has no text".into()),
                    }
                }
                return Ok(());
            }
            let Popup::TextInput { buffer, cursor, .. } = &mut self.popup else { return Ok(()) };
            let len = buffer.chars().count();
            match key.code {
                KeyCode::Esc => { self.popup = Popup::None; return Ok(()); }
                KeyCode::Enter => { return self.finish_text_input(); }
                // Caret movement, so the middle of a name can be reached.
                KeyCode::Left => { *cursor = cursor.saturating_sub(1); return Ok(()); }
                KeyCode::Right => { *cursor = (*cursor + 1).min(len); return Ok(()); }
                KeyCode::Home => { *cursor = 0; return Ok(()); }
                KeyCode::End => { *cursor = len; return Ok(()); }
                KeyCode::Char('a') if ctrl => { *cursor = 0; return Ok(()); }
                KeyCode::Char('e') if ctrl => { *cursor = len; return Ok(()); }
                KeyCode::Backspace => { backspace_at(buffer, cursor); return Ok(()); }
                KeyCode::Delete => { delete_at(buffer, cursor); return Ok(()); }
                // Clear the line, as in any readline prompt.
                KeyCode::Char('u') | KeyCode::Char('U') if ctrl => {
                    buffer.clear();
                    *cursor = 0;
                    return Ok(());
                }
                // Without this guard every Ctrl+<key> inserted its bare letter,
                // so Ctrl+V typed a "v" instead of pasting.
                KeyCode::Char(_) if ctrl => return Ok(()),
                KeyCode::Char(c) => { insert_char_at(buffer, cursor, c); return Ok(()); }
                _ => return Ok(()),
            }
        }
        if let Popup::Search { buffer } = &mut self.popup {
            match key.code {
                KeyCode::Esc => {
                    self.popup = Popup::None;
                    self.mode = Mode::Normal;
                    return Ok(());
                }
                KeyCode::Enter => { self.finish_search(); return Ok(()); }
                KeyCode::Backspace => { buffer.pop(); return Ok(()); }
                // Up/Down walk the matches while the box is still open, so
                // several files with the same substring can be visited without
                // closing and reopening the search.
                KeyCode::Down | KeyCode::Up => {
                    let forward = key.code == KeyCode::Down;
                    let q = buffer.trim().to_string();
                    if !q.is_empty() {
                        self.last_search_query = Some(q);
                        self.jump_to_next_match(forward);
                    }
                    return Ok(());
                }
                KeyCode::Char(c) => { buffer.push(c); return Ok(()); }
                _ => return Ok(()),
            }
        }
        if let Popup::ContextMenu { items, cursor, .. } = &mut self.popup {
            let n = items.len();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => *cursor = (*cursor + 1) % n,
                KeyCode::Char('k') | KeyCode::Up => *cursor = (*cursor + n - 1) % n,
                KeyCode::Enter => {
                    let item = items[*cursor];
                    return self.run_menu_item(item);
                }
                _ => {}
            }
            return Ok(());
        }
        if let Popup::SshHosts { cursor, filter } = &mut self.popup {
            match key.code {
                KeyCode::Esc => self.popup = Popup::None,
                KeyCode::Down => *cursor += 1,
                KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Backspace => {
                    filter.pop();
                    *cursor = 0;
                }
                KeyCode::Enter => {
                    let (cursor, filter) = (*cursor, filter.clone());
                    let picked = self.ssh_matches(&filter).get(cursor).map(|(i, _)| *i);
                    if let Some(i) = picked {
                        // A host with one user needs no second stage.
                        let users = self.config.ssh_hosts[i].users.clone();
                        if users.len() == 1 {
                            let only = users[0].name.clone();
                            self.popup = Popup::None;
                            self.ssh_connect(i, &only);
                        } else {
                            self.popup = Popup::SshUsers { host: i, cursor: 0 };
                        }
                    }
                }
                // Typing filters; there is no other use for plain characters
                // here, so no modifier is needed to start searching.
                KeyCode::Char(c) => {
                    filter.push(c);
                    *cursor = 0;
                }
                _ => {}
            }
            // Keep the cursor inside the filtered list.
            if let Popup::SshHosts { cursor, filter } = &mut self.popup {
                let n = self.config.ssh_hosts.iter().filter(|h| {
                    let needle = filter.to_lowercase();
                    needle.is_empty()
                        || h.name.to_lowercase().contains(&needle)
                        || h.host.to_lowercase().contains(&needle)
                }).count();
                *cursor = (*cursor).min(n.saturating_sub(1));
            }
            return Ok(());
        }
        if let Popup::SshUsers { host, cursor } = &mut self.popup {
            let n = self.config.ssh_hosts.get(*host).map(|h| h.users.len()).unwrap_or(0);
            if n == 0 {
                self.popup = Popup::None;
                return Ok(());
            }
            match key.code {
                // Esc steps back to the host list rather than closing outright.
                KeyCode::Esc => self.popup = Popup::SshHosts { cursor: 0, filter: String::new() },
                KeyCode::Char('j') | KeyCode::Down => *cursor = (*cursor + 1) % n,
                KeyCode::Char('k') | KeyCode::Up => *cursor = (*cursor + n - 1) % n,
                KeyCode::Enter => {
                    let (h, c) = (*host, *cursor);
                    let user = self.config.ssh_hosts[h].users[c].name.clone();
                    self.popup = Popup::None;
                    self.ssh_connect(h, &user);
                }
                _ => {}
            }
            return Ok(());
        }
        if let Popup::FindResults { hits, cursor, .. } = &mut self.popup {
            let n = hits.len();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.popup = Popup::None;
                    self.stop_find();
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if n > 0 {
                        *cursor = (*cursor + 1).min(n - 1);
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Char('d') | KeyCode::PageDown => {
                    if n > 0 {
                        *cursor = (*cursor + 10).min(n - 1);
                    }
                }
                KeyCode::Char('u') | KeyCode::PageUp => *cursor = cursor.saturating_sub(10),
                KeyCode::Char('g') | KeyCode::Home => *cursor = 0,
                KeyCode::Char('G') | KeyCode::End => *cursor = n.saturating_sub(1),
                KeyCode::Enter => return self.open_find_hit(),
                _ => {}
            }
            return Ok(());
        }
        if matches!(self.popup, Popup::DestPicker { .. }) {
            let n = self.dest_choices().len();
            let Popup::DestPicker { cursor, .. } = &mut self.popup else { return Ok(()) };
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => {
                    if n > 0 {
                        *cursor = (*cursor + 1).min(n - 1);
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                // Somewhere not in the list yet.
                KeyCode::Char('n') => {
                    let Popup::DestPicker { op, targets, .. } =
                        std::mem::replace(&mut self.popup, Popup::None)
                    else {
                        return Ok(());
                    };
                    self.popup = text_input(
                "destination",
                "copy/move to which directory:",
                self
                            .opposite_pane_cwd()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default(),
                InputKind::DestPath { op, targets },
            );
                }
                KeyCode::Enter => {
                    let c = *cursor;
                    let Popup::DestPicker { op, targets, .. } =
                        std::mem::replace(&mut self.popup, Popup::None)
                    else {
                        return Ok(());
                    };
                    if let Some((_, dest)) = self.dest_choices().into_iter().nth(c) {
                        self.popup = Popup::ConfirmTransfer { op, targets, dest };
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        if let Popup::Viewer { view, scroll, .. } = &mut self.popup {
            let last = view.lines.len().saturating_sub(1);
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => *scroll = (*scroll + 1).min(last),
                KeyCode::Char('k') | KeyCode::Up => *scroll = scroll.saturating_sub(1),
                KeyCode::Char('d') | KeyCode::PageDown => *scroll = (*scroll + 20).min(last),
                KeyCode::Char('u') | KeyCode::PageUp => *scroll = scroll.saturating_sub(20),
                KeyCode::Char('g') | KeyCode::Home => *scroll = 0,
                KeyCode::Char('G') | KeyCode::End => *scroll = last,
                _ => {}
            }
            return Ok(());
        }
        if let Popup::Diff { result, folded, fold, scroll, .. } = &mut self.popup {
            let rows: &[cian_core::diff::Row] = if *fold { folded } else { &result.rows };
            let last = rows.len().saturating_sub(1);
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => *scroll = (*scroll + 1).min(last),
                KeyCode::Char('k') | KeyCode::Up => *scroll = scroll.saturating_sub(1),
                KeyCode::Char('d') | KeyCode::PageDown => *scroll = (*scroll + 20).min(last),
                KeyCode::Char('u') | KeyCode::PageUp => *scroll = scroll.saturating_sub(20),
                KeyCode::Char('g') | KeyCode::Home => *scroll = 0,
                KeyCode::Char('G') | KeyCode::End => *scroll = last,
                // Jumping between differences is the reason to open this at
                // all; scrolling to hunt for the next one is the thing every
                // diff viewer exists to save you from.
                KeyCode::Char('n') => {
                    if let Some(i) =
                        rows.iter().enumerate().skip(*scroll + 1).find(|(_, r)| r.is_difference())
                    {
                        *scroll = i.0;
                    }
                }
                KeyCode::Char('N') => {
                    if let Some(i) = rows[..*scroll].iter().rposition(|r| r.is_difference()) {
                        *scroll = i;
                    }
                }
                KeyCode::Char('f') => {
                    *fold = !*fold;
                    // The row lists differ in length, so a kept offset would
                    // land somewhere unrelated.
                    *scroll = 0;
                }
                _ => {}
            }
            return Ok(());
        }
        if let Popup::Archive { members, cursor, .. } = &mut self.popup {
            let n = members.len();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => {
                    if n > 0 {
                        *cursor = (*cursor + 1).min(n - 1);
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Char('g') | KeyCode::Home => *cursor = 0,
                KeyCode::Char('G') | KeyCode::End => *cursor = n.saturating_sub(1),
                KeyCode::Char('a') => self.extract_from_archive(true),
                KeyCode::Enter => self.extract_from_archive(false),
                _ => {}
            }
            return Ok(());
        }
        if let Popup::SortPicker { cursor } = &mut self.popup {
            let n = SortKey::ALL.len();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => *cursor = (*cursor + 1) % n,
                KeyCode::Char('k') | KeyCode::Up => *cursor = (*cursor + n - 1) % n,
                KeyCode::Enter => {
                    let key = SortKey::ALL[*cursor];
                    self.popup = Popup::None;
                    self.apply_sort_key(key);
                }
                // Direct picks, so the picker is skippable once memorised.
                KeyCode::Char('n') => { self.popup = Popup::None; self.apply_sort_key(SortKey::Name); }
                KeyCode::Char('s') => { self.popup = Popup::None; self.apply_sort_key(SortKey::Size); }
                KeyCode::Char('d') => { self.popup = Popup::None; self.apply_sort_key(SortKey::Modified); }
                KeyCode::Char('e') => { self.popup = Popup::None; self.apply_sort_key(SortKey::Extension); }
                _ => {}
            }
            return Ok(());
        }
        if let Popup::ColorPicker { pane, cursor } = &mut self.popup {
            let n = PANE_BG_PRESETS.len();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => *cursor = (*cursor + 1) % n,
                KeyCode::Char('k') | KeyCode::Up => *cursor = (*cursor + n - 1) % n,
                KeyCode::Enter => {
                    let (pane, idx) = (*pane, *cursor);
                    let color = PANE_BG_PRESETS[idx].1;
                    match pane {
                        // Only the split pane that was clicked, not the panel.
                        FocusedPane::Shell => self.shell.set_active_pane_bg(color),
                        _ => {
                            if let Some(slot) = Self::bg_slot(pane) {
                                self.pane_bg[slot] = color;
                            }
                        }
                    }
                    self.popup = Popup::None;
                }
                _ => {}
            }
            return Ok(());
        }
        if let Popup::Manual { lines, scroll } = &mut self.popup {
            // The renderer clamps `scroll` to the last full page each frame, so
            // saturating at the line count here is safe.
            let last = lines.len().saturating_sub(1);
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => *scroll = (*scroll + 1).min(last),
                KeyCode::Char('k') | KeyCode::Up => *scroll = scroll.saturating_sub(1),
                KeyCode::Char('d') | KeyCode::PageDown => *scroll = (*scroll + 10).min(last),
                KeyCode::Char('u') | KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                KeyCode::Char('g') | KeyCode::Home => *scroll = 0,
                KeyCode::Char('G') | KeyCode::End => *scroll = last,
                _ => {}
            }
            return Ok(());
        }
        if let Popup::History { cursor, entries } = &mut self.popup {
            match key.code {
                KeyCode::Esc => { self.popup = Popup::None; return Ok(()); }
                KeyCode::Enter => { return self.finish_history(); }
                KeyCode::Char('j') | KeyCode::Down => {
                    if *cursor + 1 < entries.len() { *cursor += 1; }
                    return Ok(());
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if *cursor > 0 { *cursor -= 1; }
                    return Ok(());
                }
                // `a` bookmarks the highlighted path as a shortcut: the target
                // is filled in for you, and you just type a name.
                KeyCode::Char('a') => {
                    let target = entries.get(*cursor).map(|p| p.display().to_string());
                    self.popup = Popup::None;
                    if let Some(t) = target {
                        self.pending_shortcut_target = Some(t);
                        self.start_shortcut_add();
                    }
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }
        if let Popup::Shortcuts { cursor, entries } = &mut self.popup {
            match key.code {
                KeyCode::Esc => { self.popup = Popup::None; return Ok(()); }
                KeyCode::Enter => {
                    let idx = *cursor;
                    self.popup = Popup::None;
                    return self.execute_shortcut(idx);
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if !entries.is_empty() && *cursor + 1 < entries.len() { *cursor += 1; }
                    return Ok(());
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if *cursor > 0 { *cursor -= 1; }
                    return Ok(());
                }
                KeyCode::Char('a') => {
                    self.popup = Popup::None;
                    self.start_shortcut_add();
                    return Ok(());
                }
                KeyCode::Char('d') => {
                    if !entries.is_empty() {
                        let idx = *cursor;
                        entries.remove(idx);
                        self.shortcuts.entries = entries.clone();
                        let _ = self.shortcuts.save();
                        if *cursor >= entries.len() && *cursor > 0 { *cursor -= 1; }
                        if entries.is_empty() { self.popup = Popup::None; }
                    }
                    return Ok(());
                }
                KeyCode::Char('r') => {
                    if !entries.is_empty() {
                        let idx = *cursor;
                        self.popup = Popup::None;
                        self.start_shortcut_edit(idx);
                    }
                    return Ok(());
                }
                KeyCode::Char('p') => {
                    if !entries.is_empty() {
                        let idx = *cursor;
                        self.copy_shortcut_target_to_clipboard(idx);
                    }
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }
        if matches!(self.popup, Popup::ConfirmQuit) {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.popup = Popup::None;
                    self.should_quit = true;
                }
                KeyCode::Char('n') | KeyCode::Esc => { self.popup = Popup::None; }
                _ => {}
            }
            return Ok(());
        }
        if let Popup::ConfirmClose { target } = &self.popup {
            let target = *target;
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.popup = Popup::None;
                    self.execute_close(target);
                }
                KeyCode::Char('n') | KeyCode::Esc => { self.popup = Popup::None; }
                _ => {}
            }
            return Ok(());
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => { self.popup = Popup::None; Ok(()) }
            // Enter is the same as `y` here: the plain "yes" everyone reaches
            // for. (Overwrite/permanent stays on its own key so it is never
            // the accidental default.)
            KeyCode::Char('y') | KeyCode::Enter => match &self.popup {
                Popup::ConfirmDelete { .. } => self.finish_delete(DeleteMode::Trash),
                Popup::ConfirmTransfer { .. } => self.finish_transfer(Conflict::Skip),
                Popup::Notice { .. } => { self.popup = Popup::None; Ok(()) }
                _ => Ok(()),
            },
            // `a` is the "I really mean it" variant: overwrite for transfers,
            // unrecoverable delete instead of a trip through the trash.
            KeyCode::Char('a') => match &self.popup {
                Popup::ConfirmDelete { .. } => self.finish_delete(DeleteMode::Permanent),
                Popup::ConfirmTransfer { .. } => self.finish_transfer(Conflict::Overwrite),
                _ => Ok(()),
            },
            // A single-item move/copy can be renamed on the way: `r` opens an
            // editable name seeded with the destination filename.
            KeyCode::Char('r') => {
                if let Popup::ConfirmTransfer { op, targets, dest } = &self.popup {
                    if targets.len() == 1 {
                        let op = *op;
                        let src = targets[0].clone();
                        let dest = dest.clone();
                        let name =
                            src.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                        self.popup = text_input(
                            match op { PendingOp::Move => "move as", PendingOp::Copy => "copy as" },
                            "new name in the destination:",
                            name,
                            InputKind::TransferAs { op, src, dest_dir: dest },
                        );
                    } else {
                        self.message = Some("rename applies to a single item".into());
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) -> Result<()> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        // Paths are what get typed here, and they are the thing most worth
        // pasting rather than retyping. Ctrl+V mirrors the text-input prompt.
        if ctrl && matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V')) {
            if let Some(t) = self.clipboard_text() {
                self.insert_into_active_text(&t);
            } else {
                self.message = Some("clipboard has no text".into());
            }
            return Ok(());
        }
        match key.code {
            KeyCode::Esc => { self.command_buffer.clear(); self.mode = Mode::Normal; }
            KeyCode::Enter => self.run_command(),
            KeyCode::Backspace => { self.command_buffer.pop(); }
            // Clear the line, as in any readline prompt.
            KeyCode::Char('u') | KeyCode::Char('U') if ctrl => self.command_buffer.clear(),
            // Otherwise a bare Ctrl+<key> would type its letter into the line.
            KeyCode::Char(_) if ctrl => {}
            KeyCode::Char(c) => self.command_buffer.push(c),
            _ => {}
        }
        Ok(())
    }

    /// Insert pasted or typed text into whichever text field currently has the
    /// focus. Used by Ctrl+V and by a terminal bracketed-paste event, so a
    /// paste lands in the same place a keystroke would.
    ///
    /// Newlines are stripped: a path copied from `pwd`, a browser, or another
    /// pane carries a trailing one, and a bracketed paste of several lines
    /// would otherwise smuggle an Enter into a single-line prompt.
    fn insert_into_active_text(&mut self, text: &str) {
        let clean: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        if clean.is_empty() {
            return;
        }
        match &mut self.popup {
            Popup::TextInput { buffer, cursor, .. } => {
                insert_str_at(buffer, cursor, &clean);
                return;
            }
            Popup::Search { buffer } => {
                buffer.push_str(&clean);
                return;
            }
            Popup::SshHosts { filter, .. } => {
                filter.push_str(&clean);
                return;
            }
            _ => {}
        }
        match self.mode {
            Mode::Command => self.command_buffer.push_str(&clean),
            Mode::Filter => {
                self.filter_buffer.push_str(&clean);
                self.apply_filter_buffer();
            }
            // A paste into the shell belongs to whatever is running there —
            // the prompt, or a full-screen editor. The original text is sent,
            // newlines and all, since a multi-line paste into a shell is a
            // deliberate thing. The raw bytes go through unwrapped: this does
            // not track whether the child turned on bracketed paste, and a
            // `\x1b[200~` wrapper sent to a program that did not would show up
            // as literal garbage.
            _ if self.focused == FocusedPane::Shell => {
                if let Some(s) = self.shell.active_session_mut() {
                    s.write_input(text.as_bytes());
                }
            }
            _ => {}
        }
    }

    fn handle_shell_key(&mut self, key: KeyEvent) -> Result<()> {
        let (alt_screen, app_cursor) = self.shell.active_modes();
        if self.debug_keys {
            self.message = Some(format!(
                "key={:?} mods={:?} alt_screen={}",
                key.code, key.modifiers, alt_screen
            ));
        }
        // Esc returns to the file pane — unless a full-screen app (alternate
        // screen) is running, in which case Esc belongs to that app (e.g. vim).
        if key.code == KeyCode::Esc && !alt_screen {
            self.focus(self.last_file_pane);
            return Ok(());
        }
        // Tab and split controls via F-keys — reserved only at a normal prompt.
        // The Ctrl modifier is swallowed before reaching the app on some setups
        // (IME/OS), so F-keys (independent escape sequences) are used instead.
        // Shift+F drives splits. While a full-screen app runs (alternate screen)
        // they pass through, like F12.
        if !alt_screen {
            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
            match key.code {
                // Pane navigation (parallels file-pane tab nav): Shift+F1/F2.
                KeyCode::F(1) if shift => {
                    self.shell.next_pane();
                    return Ok(());
                }
                KeyCode::F(2) if shift => {
                    self.shell.prev_pane();
                    return Ok(());
                }
                // Splits within the active tab: Shift+F8 left/right, F9 top/bottom.
                KeyCode::F(8) if shift => {
                    let cwd = self.shell_cwd();
                    self.shell.split_active(&cwd, SplitDir::LeftRight);
                    return Ok(());
                }
                KeyCode::F(9) if shift => {
                    let cwd = self.shell_cwd();
                    self.shell.split_active(&cwd, SplitDir::TopBottom);
                    return Ok(());
                }
                // Close the active split pane, with confirmation.
                KeyCode::F(10) if shift => {
                    self.popup = Popup::ConfirmClose { target: CloseTarget::ShellPane };
                    return Ok(());
                }
                // Tab controls: plain F-keys.
                KeyCode::F(n @ 1..=8) if !shift => {
                    self.shell.select((n - 1) as usize);
                    return Ok(());
                }
                KeyCode::F(9) if !shift => {
                    let cwd = self.shell_cwd();
                    self.shell.new_tab(&cwd);
                    return Ok(());
                }
                KeyCode::F(10) if !shift => {
                    if self.shell.close_active() {
                        self.focus(self.last_file_pane);
                    }
                    return Ok(());
                }
                _ => {}
            }
        }
        // Everything else is forwarded to the shell.
        if let Some(bytes) = encode_key(key, app_cursor) {
            if let Some(s) = self.shell.active_session_mut() {
                s.write_input(&bytes);
            }
        }
        Ok(())
    }

    fn handle_visual_key(&mut self, mut key: KeyEvent) -> Result<()> {
        normalize_jp_key(&mut key);
        // `gg` works here too, so `gg` then visual then `G` selects everything.
        if self.pending_g {
            self.pending_g = false;
            if matches!(key.code, KeyCode::Char('g')) {
                if let Some(p) = self.active_pane_mut() { p.cursor = 0; }
                return Ok(());
            }
        }
        match key.code {
            KeyCode::Esc => self.visual_cancel_and_clear_all(),
            KeyCode::Enter | KeyCode::Char('v') => self.visual_commit(),
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(1); }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(-1); }
            }
            // Stretch the selection over the whole listing in one keystroke.
            KeyCode::Char('a') => self.visual_select_all(),
            KeyCode::Char('g') => self.pending_g = true,
            KeyCode::Char('G') | KeyCode::End => {
                if let Some(p) = self.active_pane_mut() {
                    if !p.entries.is_empty() { p.cursor = p.entries.len() - 1; }
                }
            }
            KeyCode::Home => {
                if let Some(p) = self.active_pane_mut() { p.cursor = 0; }
            }
            KeyCode::Char('D') | KeyCode::PageDown => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(10); }
            }
            KeyCode::Char('U') | KeyCode::PageUp => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(-10); }
            }
            _ => {}
        }
        Ok(())
    }

    /// Anchor at the top and put the cursor at the bottom, so the whole
    /// listing is selected and Enter marks it.
    fn visual_select_all(&mut self) {
        let last = match self.active_pane() {
            Some(p) if !p.entries.is_empty() => p.entries.len() - 1,
            _ => return,
        };
        self.visual_anchor = Some(0);
        if let Some(p) = self.active_pane_mut() {
            p.cursor = last;
        }
    }

    fn handle_normal_key(&mut self, mut key: KeyEvent) -> Result<()> {
        // Full-width input (全角英数) → ASCII so commands work without leaving
        // the Japanese IME. Kana can be bound per-key via init.lua.
        normalize_jp_key(&mut key);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        // `gg` chord → jump to top
        if self.pending_g {
            self.pending_g = false;
            if matches!(key.code, KeyCode::Char('g')) && !ctrl {
                if let Some(p) = self.active_pane_mut() { p.cursor = 0; }
                return Ok(());
            }
            // anything else: fall through to normal handling
        }

        // User keymap overrides: plain character keys (no Ctrl). Only keys the
        // user explicitly bound appear here, so default behaviour is untouched
        // for everything else.
        if !ctrl {
            if let KeyCode::Char(c) = key.code {
                if let Some(action) = self.keymap.get(&c).copied() {
                    return self.execute_action(action);
                }
            }
        }

        match (ctrl, shift, key.code) {
            (false, _, KeyCode::Char('q')) => self.start_quit_confirm(),
            // `_` for shift, not `false`: `:` is Shift+; on most layouts, and a
            // terminal with the kitty keyboard protocol (WezTerm, kitty, foot)
            // reports that Shift, so `(false, false, …)` never matched there and
            // `:` did nothing. The character already encodes the shift; whether
            // the modifier is also set is irrelevant. Same for the punctuation
            // bindings below.
            (false, _, KeyCode::Char(':')) => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
            }
            (false, _, KeyCode::Esc) => {
                if let Some(p) = self.active_pane_mut() {
                    p.clear_marks();
                    p.clear_filter();
                }
            }
            // Pane navigation: Shift + H/J/K/L (universally works, no terminal config needed).
            // Ctrl+H/J/K/L is the alternative — needs `enable_kitty_keyboard = true` in WezTerm.
            (false, _, KeyCode::Char('H')) => self.focus_direction('h'),
            (false, _, KeyCode::Char('J')) => self.focus_direction('j'),
            (false, _, KeyCode::Char('K')) => self.focus_direction('k'),
            (false, _, KeyCode::Char('L')) => self.focus_direction('l'),
            (true, _, KeyCode::Char('h')) => self.focus_direction('h'),
            (true, _, KeyCode::Char('j')) => self.focus_direction('j'),
            (true, _, KeyCode::Char('k')) => self.focus_direction('k'),
            (true, _, KeyCode::Char('l')) => self.focus_direction('l'),
            (false, false, KeyCode::Tab) => {
                if let Some(t) = self.active_file_tabs_mut() { t.next_tab(); }
            }
            (_, _, KeyCode::BackTab) => {
                if let Some(t) = self.active_file_tabs_mut() { t.prev_tab(); }
            }
            (true, _, KeyCode::Char(c)) if c.is_ascii_digit() => {
                if let Some(d) = c.to_digit(10) {
                    if d >= 1 {
                        if let Some(t) = self.active_file_tabs_mut() { t.select(d as usize - 1); }
                    }
                }
            }
            // Tab management: t = new tab, w = close active tab (Ctrl variants intentionally absent).
            (false, false, KeyCode::Char('t')) => {
                if let Some(t) = self.active_file_tabs_mut() { t.add_clone()?; }
            }
            (false, false, KeyCode::Char('w')) => {
                if let Some(t) = self.active_file_tabs_mut() { t.close_active(); }
            }
            // Consistent F-key tab controls (parallel the shell's pane controls):
            // Shift+F1/F2 = next/prev tab, Shift+F10 = close tab (with confirm).
            (false, true, KeyCode::F(1)) => {
                if let Some(t) = self.active_file_tabs_mut() { t.next_tab(); }
            }
            (false, true, KeyCode::F(2)) => {
                if let Some(t) = self.active_file_tabs_mut() { t.prev_tab(); }
            }
            (false, true, KeyCode::F(10)) => {
                self.popup = Popup::ConfirmClose { target: CloseTarget::FileTab(self.focused) };
            }
            // search, filter, history, shortcuts
            (false, false, KeyCode::Char('f')) => self.start_search(),
            (false, _, KeyCode::Char('/')) => self.start_filter(),
            (false, true, KeyCode::Char('F')) => self.start_find_prompt(),
            (true, _, KeyCode::Char('f')) => self.start_grep_prompt(),
            (false, _, KeyCode::Char(',')) => self.start_sort_picker(),
            (false, false, KeyCode::Char('z')) => self.start_jump_path(),
            (false, false, KeyCode::F(3)) => self.look_inside(),
            // `=` for "are these equal": free, mnemonic, and next to the
            // keys already used for the two panes.
            (false, _, KeyCode::Char('=')) => self.open_diff(),
            // Manual refresh, for the cases the timer cannot see — a file
            // whose contents changed without the directory being touched.
            (true, _, KeyCode::Char('r')) | (false, false, KeyCode::F(5)) => {
                self.reload_both();
                self.message = Some("refreshed".into());
            }
            // Shift+Enter opens the same menu the right mouse button does, for
            // the entry under the cursor. Needs a terminal that distinguishes
            // it from plain Enter (the Windows console does; on Unix it wants
            // the kitty keyboard protocol) — `:menu` always works.
            (false, true, KeyCode::Enter) => self.open_menu_at_cursor(),
            (false, true, KeyCode::Char('S')) => self.start_ssh(),
            (false, _, KeyCode::Char('n')) => self.jump_to_next_match(true),
            (false, _, KeyCode::Char('N')) => self.jump_to_next_match(false),
            (false, false, KeyCode::Char('h')) => self.start_history(),
            (false, false, KeyCode::Char('s')) => self.start_shortcuts(),
            // navigation: gg/G + Shift+U/D for fast cursor moves
            (false, false, KeyCode::Char('g')) => { self.pending_g = true; }
            (false, _, KeyCode::Char('G')) => {
                if let Some(p) = self.active_pane_mut() {
                    if !p.entries.is_empty() { p.cursor = p.entries.len() - 1; }
                }
            }
            (false, _, KeyCode::Char('U')) => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(-10); }
            }
            (false, _, KeyCode::Char('D')) => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(10); }
            }
            // p = copy path strings; P = copy file references (Finder/Explorer-style)
            (false, false, KeyCode::Char('p')) => self.copy_paths_to_clipboard(),
            (false, true, KeyCode::Char('P')) => self.copy_file_refs_to_clipboard(),
            (false, false, KeyCode::Char(' ')) => {
                if let Some(p) = self.active_pane_mut() {
                    let i = p.cursor; p.toggle_mark_at(i); p.move_cursor(1);
                }
            }
            (false, true, KeyCode::Char(' ')) => {
                if let Some(p) = self.active_pane_mut() {
                    let i = p.cursor; p.toggle_mark_at(i); p.move_cursor(-1);
                }
            }
            (false, false, KeyCode::Char('v')) => self.visual_start(),
            (false, true, KeyCode::Char('V')) => {
                if let Some(p) = self.active_pane_mut() {
                    for i in 0..p.entries.len() { p.toggle_mark_at(i); }
                }
            }
            (false, false, KeyCode::Char('y')) | (false, false, KeyCode::Char('c')) => {
                self.start_transfer(PendingOp::Copy);
            }
            (false, false, KeyCode::Char('m')) => self.start_transfer(PendingOp::Move),
            (false, false, KeyCode::Char('d')) => self.start_delete(),
            (false, false, KeyCode::Char('r')) => self.start_rename(),
            (false, false, KeyCode::Char('a')) => self.start_new_file(),
            (false, true, KeyCode::Char('A')) => self.start_new_dir(),
            (false, false, KeyCode::Char('j')) | (_, _, KeyCode::Down) => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(1); }
            }
            (false, false, KeyCode::Char('k')) | (_, _, KeyCode::Up) => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(-1); }
            }
            // Parent: `-` or Backspace. The arrows move between panes instead,
            // which is what a two-pane layout makes people reach for — using
            // them for up/into a directory was reported as confusing.
            (false, false, KeyCode::Char('-'))
            | (_, _, KeyCode::Backspace) => {
                if let Some(p) = self.active_pane_mut() { p.go_parent()?; }
            }
            (_, _, KeyCode::Left) => self.focus(FocusedPane::Left),
            (_, _, KeyCode::Right) => self.focus(FocusedPane::Right),
            // `l` only enters directories; never opens files.
            (false, false, KeyCode::Char('l')) => {
                if let Some(p) = self.active_pane_mut() {
                    let is_dir = p.selected().map(|e| e.is_dir).unwrap_or(false);
                    if is_dir { p.enter_selected()?; }
                }
            }
            // Ctrl+Enter / Ctrl+Shift+Enter need kitty keyboard protocol to be distinguished.
            (true, false, KeyCode::Enter) => { self.open_in_other_pane(false)?; }
            (true, true, KeyCode::Enter) => { self.open_in_other_pane(true)?; }
            // Universal aliases (always work, no terminal config needed).
            (false, false, KeyCode::Char('o')) => { self.open_in_other_pane(false)?; }
            (false, true, KeyCode::Char('O')) => { self.open_in_other_pane(true)?; }
            // Enter alone keeps the OS-open behavior until viewer ships in sprint 5.
            (false, _, KeyCode::Enter) => {
                let is_dir = self.active_pane()
                    .and_then(|p| p.selected())
                    .map(|e| e.is_dir)
                    .unwrap_or(false);
                if is_dir {
                    if let Some(p) = self.active_pane_mut() { p.enter_selected()?; }
                } else {
                    self.open_externally();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Run a remappable action (dispatched from a user keymap override). The
    /// bodies mirror the default key handlers exactly so behaviour is identical
    /// whether triggered by a default key or a user-bound one.
    fn execute_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::CursorDown => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(1); }
            }
            Action::CursorUp => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(-1); }
            }
            Action::CursorBottom => {
                if let Some(p) = self.active_pane_mut() {
                    if !p.entries.is_empty() { p.cursor = p.entries.len() - 1; }
                }
            }
            Action::PageUp => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(-10); }
            }
            Action::PageDown => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(10); }
            }
            Action::Parent => {
                if let Some(p) = self.active_pane_mut() { p.go_parent()?; }
            }
            Action::EnterDir => {
                if let Some(p) = self.active_pane_mut() {
                    let is_dir = p.selected().map(|e| e.is_dir).unwrap_or(false);
                    if is_dir { p.enter_selected()?; }
                }
            }
            Action::Quit => self.start_quit_confirm(),
            Action::Search => self.start_search(),
            Action::SearchNext => self.jump_to_next_match(true),
            Action::SearchPrev => self.jump_to_next_match(false),
            Action::History => self.start_history(),
            Action::Shortcuts => self.start_shortcuts(),
            Action::Copy => self.start_transfer(PendingOp::Copy),
            Action::Move => self.start_transfer(PendingOp::Move),
            Action::Delete => self.start_delete(),
            Action::Rename => self.start_rename(),
            Action::NewFile => self.start_new_file(),
            Action::NewDir => self.start_new_dir(),
            Action::OpenOther => self.open_in_other_pane(false)?,
            Action::OpenOtherTab => self.open_in_other_pane(true)?,
            Action::OpenExternal => self.open_externally(),
            Action::CopyPath => self.copy_paths_to_clipboard(),
            Action::CopyFileRef => self.copy_file_refs_to_clipboard(),
            Action::MarkDown => {
                if let Some(p) = self.active_pane_mut() {
                    let i = p.cursor; p.toggle_mark_at(i); p.move_cursor(1);
                }
            }
            Action::MarkUp => {
                if let Some(p) = self.active_pane_mut() {
                    let i = p.cursor; p.toggle_mark_at(i); p.move_cursor(-1);
                }
            }
            Action::InvertMarks => {
                if let Some(p) = self.active_pane_mut() {
                    for i in 0..p.entries.len() { p.toggle_mark_at(i); }
                }
            }
            Action::Visual => self.visual_start(),
            Action::Command => {
                self.mode = Mode::Command;
                self.command_buffer.clear();
            }
        }
        Ok(())
    }
}

/// Map a full-width character to its ASCII equivalent, if it has one.
///
/// Covers the full-width ASCII block (U+FF01–U+FF5E → U+0021–U+007E) and the
/// ideographic space, so commands work while a Japanese IME is in full-width
/// alphanumeric (全角英数) mode without switching back to ASCII input.
fn jp_to_ascii(c: char) -> Option<char> {
    let u = c as u32;
    if (0xFF01..=0xFF5E).contains(&u) {
        char::from_u32(u - 0xFEE0)
    } else if c == '\u{3000}' {
        Some(' ')
    } else {
        None
    }
}

/// Normalise a key in place: full-width characters become their ASCII command
/// key, with SHIFT synthesised for upper-case letters so the existing
/// shift-gated bindings (A, V, P, O, …) still match.
fn normalize_jp_key(key: &mut KeyEvent) {
    if let KeyCode::Char(c) = key.code {
        if let Some(a) = jp_to_ascii(c) {
            key.code = KeyCode::Char(a);
            if a.is_ascii_uppercase() {
                key.modifiers.insert(KeyModifiers::SHIFT);
            }
        }
    }
}

/// Translate a key event into the byte sequence a terminal would send to the
/// shell. `app_cursor` selects between normal (`ESC [`) and application
/// (`ESC O`) cursor-key encodings, mirroring the active DECCKM mode.
fn encode_key(key: KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let cursor = |c: u8| -> Vec<u8> {
        let intro = if app_cursor { b"\x1bO" } else { b"\x1b[" };
        let mut v = intro.to_vec();
        v.push(c);
        v
    };

    let mut out: Vec<u8> = Vec::new();
    match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                let ctl = match c {
                    ' ' | '@' => Some(0u8),
                    'a'..='z' => Some(c as u8 - b'a' + 1),
                    'A'..='Z' => Some(c as u8 - b'A' + 1),
                    '[' => Some(27),
                    '\\' => Some(28),
                    ']' => Some(29),
                    '^' => Some(30),
                    '_' => Some(31),
                    '?' => Some(127),
                    _ => None,
                };
                if alt {
                    out.push(0x1b);
                }
                match ctl {
                    Some(b) => out.push(b),
                    None => {
                        let mut buf = [0u8; 4];
                        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    }
                }
            } else {
                if alt {
                    out.push(0x1b);
                }
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
        KeyCode::Enter => out.push(b'\r'),
        KeyCode::Tab => out.push(b'\t'),
        KeyCode::BackTab => out.extend_from_slice(b"\x1b[Z"),
        KeyCode::Backspace => out.push(0x7f),
        KeyCode::Esc => out.push(0x1b),
        KeyCode::Up => out = cursor(b'A'),
        KeyCode::Down => out = cursor(b'B'),
        KeyCode::Right => out = cursor(b'C'),
        KeyCode::Left => out = cursor(b'D'),
        KeyCode::Home => out = cursor(b'H'),
        KeyCode::End => out = cursor(b'F'),
        KeyCode::PageUp => out.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => out.extend_from_slice(b"\x1b[6~"),
        KeyCode::Delete => out.extend_from_slice(b"\x1b[3~"),
        KeyCode::Insert => out.extend_from_slice(b"\x1b[2~"),
        _ => return None,
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn os_open(path: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(target_os = "linux")]
    let mut cmd = Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg("start").arg("");
        c
    };
    cmd.arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

fn os_open_string(target: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(target_os = "linux")]
    let mut cmd = Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg("start").arg("");
        c
    };
    cmd.arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

/// The user's home directory: `$HOME`, or `$USERPROFILE` on Windows.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Expand `~`, `$VAR`/`${VAR}` and `%VAR%` in a typed path.
///
/// A path is usually typed after copying it from somewhere, and the somewhere
/// is often a shell or an Explorer address bar, where these forms are normal.
/// Parse a `-n N` argument (or the bare `-N` shorthand `head`/`tail` accept).
fn parse_dash_n(args: &[&str]) -> Option<usize> {
    let mut it = args.iter().copied();
    while let Some(a) = it.next() {
        if a == "-n" {
            return it.next().and_then(|v| v.parse().ok());
        }
        // `-20` shorthand.
        if let Some(num) = a.strip_prefix('-') {
            if let Ok(n) = num.parse::<usize>() {
                return Some(n);
            }
        }
    }
    None
}

/// Quote a path for a POSIX shell (single quotes, with the usual `'\''`
/// escape). On Windows the shell is usually PowerShell or cmd, whose quoting
/// differs, but a path with no odd characters passes through either way and
/// this at least keeps spaces together.
fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    if s.chars().all(|c| c.is_alphanumeric() || "._-/:\\".contains(c)) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Build a text-input popup with the caret at the end of the seeded text —
/// where you want it for editing an existing name or path.
fn text_input(
    title: impl Into<String>,
    prompt: impl Into<String>,
    buffer: String,
    kind: InputKind,
) -> Popup {
    let cursor = buffer.chars().count();
    Popup::TextInput { title: title.into(), prompt: prompt.into(), buffer, kind, cursor }
}

/// Byte offset of the `n`-th char, or the string's length past the end. Used to
/// edit a `String` at a caret expressed as a char index (so CJK is handled).
fn char_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

fn insert_str_at(buffer: &mut String, cursor: &mut usize, s: &str) {
    let b = char_byte(buffer, *cursor);
    buffer.insert_str(b, s);
    *cursor += s.chars().count();
}

fn insert_char_at(buffer: &mut String, cursor: &mut usize, c: char) {
    let b = char_byte(buffer, *cursor);
    buffer.insert(b, c);
    *cursor += 1;
}

/// Delete the char before the caret (Backspace).
fn backspace_at(buffer: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    let start = char_byte(buffer, *cursor - 1);
    let end = char_byte(buffer, *cursor);
    buffer.replace_range(start..end, "");
    *cursor -= 1;
}

/// Delete the char at the caret (Delete).
fn delete_at(buffer: &mut String, cursor: &mut usize) {
    let n = buffer.chars().count();
    if *cursor >= n {
        return;
    }
    let start = char_byte(buffer, *cursor);
    let end = char_byte(buffer, *cursor + 1);
    buffer.replace_range(start..end, "");
}

/// Render a single-line field with a visible caret at `cursor`, masking the
/// text with dots when it is a secret.
fn field_with_caret(buffer: &str, cursor: usize, secret: bool) -> String {
    let shown: String = if secret {
        "•".repeat(buffer.chars().count())
    } else {
        buffer.to_string()
    };
    let split = char_byte(&shown, cursor);
    format!(">{}▏{}", &shown[..split], &shown[split..])
}

fn expand_path(input: &str) -> PathBuf {
    let mut out = String::with_capacity(input.len());
    let b: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            // %VAR% (Windows)
            '%' => {
                if let Some(end) = b[i + 1..].iter().position(|c| *c == '%') {
                    let name: String = b[i + 1..i + 1 + end].iter().collect();
                    if let Some(v) = std::env::var_os(&name) {
                        out.push_str(&v.to_string_lossy());
                        i += end + 2;
                        continue;
                    }
                }
                out.push('%');
                i += 1;
            }
            // $VAR and ${VAR} (Unix)
            '$' => {
                let (name, adv) = if b.get(i + 1) == Some(&'{') {
                    match b[i + 2..].iter().position(|c| *c == '}') {
                        Some(end) => (b[i + 2..i + 2 + end].iter().collect::<String>(), end + 3),
                        None => (String::new(), 1),
                    }
                } else {
                    let end = b[i + 1..]
                        .iter()
                        .position(|c| !c.is_alphanumeric() && *c != '_')
                        .unwrap_or(b.len() - i - 1);
                    (b[i + 1..i + 1 + end].iter().collect::<String>(), end + 1)
                };
                match (name.is_empty(), std::env::var_os(&name)) {
                    (false, Some(v)) => {
                        out.push_str(&v.to_string_lossy());
                        i += adv;
                    }
                    _ => {
                        out.push('$');
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    // Quotes survive copy-paste from shells and Explorer; strip a matched pair.
    let t = out.trim();
    let unquoted = |q: char| t.strip_prefix(q).and_then(|x| x.strip_suffix(q));
    let t = unquoted('"').or_else(|| unquoted('\'')).unwrap_or(t);
    expand_tilde(Path::new(t))
}

fn expand_tilde(p: &Path) -> PathBuf {
    if let Some(s) = p.to_str() {
        if let Some(rest) = s.strip_prefix("~/") {
            if let Some(home) = home_dir() {
                return home.join(rest);
            }
        }
        if s == "~" {
            if let Some(home) = home_dir() {
                return home;
            }
        }
    }
    p.to_path_buf()
}

/// Put native file references on the clipboard so Finder/Explorer can paste
/// the actual files (not just the path string).
/// Files currently on the OS clipboard, e.g. copied in Explorer or Finder.
///
/// Every candidate is checked against the filesystem before being returned:
/// the platform queries happily hand back plain clipboard *text* interpreted
/// as a path (copying the word "hello" yields `/hello` on macOS), and acting
/// on that would be at best a confusing error.
fn os_clipboard_files() -> Vec<PathBuf> {
    keep_existing(os_clipboard_files_raw())
}

/// Drop anything that is not actually a file or directory on disk.
fn keep_existing(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.into_iter().filter(|p| p.exists()).collect()
}

#[cfg(target_os = "macos")]
fn os_clipboard_files_raw() -> Vec<PathBuf> {
    // `the clipboard as «class furl»` only ever yields one file; coercing to a
    // list handles both the single- and multi-file cases.
    const SCRIPT: &str = r#"set out to ""
try
  set items_ to the clipboard as list
  repeat with i in items_
    set out to out & POSIX path of i & linefeed
  end repeat
end try
return out"#;
    let out = match Command::new("osascript").args(["-e", SCRIPT]).output() {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&out)
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(target_os = "macos")]
fn os_clipboard_file_refs(paths: &[PathBuf]) -> Result<()> {
    let escape = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let parts: Vec<String> = paths
        .iter()
        .map(|p| format!("POSIX file \"{}\"", escape(&p.display().to_string())))
        .collect();
    let script = if parts.len() == 1 {
        format!("set the clipboard to {}", parts[0])
    } else {
        format!("set the clipboard to {{{}}}", parts.join(", "))
    };
    let status = Command::new("osascript")
        .args(["-e", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        anyhow::bail!("osascript exited with status {}", status);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn os_clipboard_files_raw() -> Vec<PathBuf> {
    let read = |cmd: &str, args: &[&str]| -> Option<String> {
        let o = Command::new(cmd).args(args).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    // Wayland first, then X11, mirroring the write side.
    let text = read("wl-paste", &["--type", "text/uri-list"])
        .or_else(|| read("xclip", &["-selection", "clipboard", "-t", "text/uri-list", "-o"]))
        .unwrap_or_default();
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| PathBuf::from(percent_decode(l.strip_prefix("file://").unwrap_or(l))))
        .collect()
}

/// Turn `%20`-style escapes in a `file://` URI back into bytes.
#[cfg(target_os = "linux")]
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(target_os = "linux")]
fn os_clipboard_file_refs(paths: &[PathBuf]) -> Result<()> {
    use std::io::Write;
    let uris = paths
        .iter()
        .map(|p| format!("file://{}", p.display()))
        .collect::<Vec<_>>()
        .join("\n");
    // try wl-copy first (wayland), then xclip
    if let Ok(mut child) = Command::new("wl-copy")
        .args(["--type", "text/uri-list"])
        .stdin(Stdio::piped())
        .spawn()
    {
        if let Some(s) = child.stdin.as_mut() {
            s.write_all(uris.as_bytes())?;
        }
        if child.wait()?.success() {
            return Ok(());
        }
    }
    let mut child = Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "text/uri-list"])
        .stdin(Stdio::piped())
        .spawn()?;
    if let Some(s) = child.stdin.as_mut() {
        s.write_all(uris.as_bytes())?;
    }
    if !child.wait()?.success() {
        anyhow::bail!("xclip failed");
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn os_clipboard_files_raw() -> Vec<PathBuf> {
    let out = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-Clipboard -Format FileDropList | ForEach-Object { $_.FullName }",
        ])
        .output();
    let out = match out {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&out)
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

#[cfg(target_os = "windows")]
fn os_clipboard_file_refs(paths: &[PathBuf]) -> Result<()> {
    // Was a stub that always failed, so Shift+P did nothing on the platform
    // where Explorer interop matters most.
    if paths.is_empty() {
        return Ok(());
    }
    // Single-quoted PowerShell literals: the only escape needed is a doubled
    // quote, which leaves spaces and backslashes alone.
    let list = paths
        .iter()
        .map(|p| format!("'{}'", p.display().to_string().replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &format!("Set-Clipboard -Path {}", list)])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        anyhow::bail!("Set-Clipboard exited with status {}", status);
    }
    Ok(())
}

fn shortcut_icon(target: &str) -> &'static str {
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("file://")
    {
        return "\u{f0ac}"; // globe
    }
    let lower = target.to_lowercase();
    if lower.ends_with(".app") {
        return "\u{f179}"; // apple
    }
    let path = expand_tilde(Path::new(target));
    if path.is_dir() {
        return "\u{f07b}"; // folder
    }
    if path.exists() {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        // Only the name matters here: this is used to pick an icon by extension.
        let entry =
            cian_core::Entry { name, path: path.clone(), is_dir: false, len: 0, modified: None };
        return icon_for(&entry);
    }
    "\u{f15b}" // default file
}

/// One row of the manual: the built-in key(s), the remappable action they run
/// (if any), and what it does.
struct ManualEntry {
    keys: &'static str,
    action: Option<Action>,
    desc: &'static str,
}

const fn entry(keys: &'static str, action: Option<Action>, desc: &'static str) -> ManualEntry {
    ManualEntry { keys, action, desc }
}

/// The manual's contents, grouped into sections. Entries carrying an [`Action`]
/// are remappable, so [`manual_lines`] can append whatever extra keys the user
/// bound to them in `init.lua`.
fn manual_sections() -> Vec<(&'static str, Vec<ManualEntry>)> {
    use Action::*;
    vec![
        (
            "General",
            vec![
                entry("q", Some(Quit), "quit (confirms)"),
                entry(":", Some(Command), "command mode (:q, :shell, :man)"),
                entry("?, Ctrl+.", None, "show this manual (also right-click)"),
                entry("Esc", None, "clear marks and filter / leave shell"),
            ],
        ),
        (
            "Navigation",
            vec![
                entry("j, Down", Some(CursorDown), "cursor down"),
                entry("k, Up", Some(CursorUp), "cursor up"),
                entry("Shift+D", Some(PageDown), "move 10 lines down"),
                entry("Shift+U", Some(PageUp), "move 10 lines up"),
                entry("gg", None, "jump to top"),
                entry("G", Some(CursorBottom), "jump to bottom"),
                entry("l, Enter", Some(EnterDir), "enter folder / open file"),
                entry("F3", None, "look inside: view a file, list an archive"),
                entry("=", None, "compare the left pane's file with the right pane's"),
                entry("-, Bksp", Some(Parent), "parent folder"),
                entry("Left / Right", None, "focus the left / right pane"),
                entry("h", Some(History), "history popup"),
                entry("z", None, "go to a typed path (also :cd)"),
                entry("Ctrl+R, F5", None, "refresh now"),
                entry("f", Some(Search), "search in this folder"),
                entry("Shift+F", None, "find by name, whole tree below here"),
                entry("Ctrl+F", None, "grep inside files, whole tree below here"),
                entry("n", Some(SearchNext), "next match"),
                entry("N", Some(SearchPrev), "previous match"),
                entry("/", None, "filter list as you type"),
                entry(",", None, "sort by name / size / date / ext"),
                entry("Shift+S", None, "ssh picker (also :ssh, or right-click)"),
                entry("Enter, Esc", None, "while filtering: keep / clear it"),
            ],
        ),
        (
            "Marks and file operations",
            vec![
                entry("Space", Some(MarkDown), "toggle mark, move down"),
                entry("Shift+Space", Some(MarkUp), "toggle mark, move up"),
                entry("v", Some(Visual), "visual select"),
                entry("  a", None, "  in visual: select all (or gg v G)"),
                entry("  gg / G", None, "  in visual: extend to top / bottom"),
                entry("V", Some(InvertMarks), "invert all marks"),
                entry("y, c", Some(Copy), "copy to opposite pane"),
                entry("m", Some(Move), "move to opposite pane"),
                entry("d", Some(Delete), "delete (to trash)"),
                entry("r", Some(Rename), "rename"),
                entry("a", Some(NewFile), "new file"),
                entry("A", Some(NewDir), "new directory"),
                entry("o", Some(OpenOther), "open in opposite pane"),
                entry("O", Some(OpenOtherTab), "open in opposite pane's new tab"),
                entry("p", Some(CopyPath), "copy path text to clipboard"),
                entry("Shift+P", Some(CopyFileRef), "copy file(s) to clipboard"),
                entry("s", Some(Shortcuts), "shortcuts menu"),
                entry(":hidden", None, "show / hide dotfiles (also right-click)"),
                entry(":attr", None, "attributes;  :chmod 644,  :readonly on|off"),
                entry(":hash", None, "checksum;  :hash md5  /  :hash sha256"),
                entry("Shift+Enter", None, "context menu for the entry (also :menu)"),
            ],
        ),
        (
            "Panes and tabs",
            vec![
                entry("Shift+H/J/K/L", None, "move focus between panes"),
                entry("drag a border", None, "resize any split (mouse)"),
                entry("drag an entry", None, "to the other pane: copy (Shift: move)"),
                entry("  ", None, "  onto the shell: type its path there"),
                entry(":copyto", None, "copy to a recent or typed directory"),
                entry("right-click", None, "context menu (copy/cut/paste, color)"),
                entry("Ctrl+H/J/K/L", None, "same (needs kitty keyboard support)"),
                entry("t", None, "new tab"),
                entry("w", None, "close tab"),
                entry("Tab, Shift+Tab", None, "next / previous tab"),
                entry("Ctrl+1..9", None, "jump to tab N"),
                entry("Shift+F1/F2", None, "next / previous tab"),
                entry("Shift+F10", None, "close tab (confirms)"),
            ],
        ),
        (
            "Commands (type : then the name — Linux-style)",
            vec![
                entry(":mkdir", None, "make a directory;  :mkdir -p a/b/c"),
                entry(":touch", None, "create a file, or bump its mtime"),
                entry(":cp / :mv", None, "no arg → other pane;  or  :mv <dest>"),
                entry(":rm", None, "delete the selection (to trash)"),
                entry(":cd", None, ":cd <path>  /  :cd ..  /  :cd -  /  :cd ~"),
                entry(":pwd", None, "show the directory, copy it to the clipboard"),
                entry(":ls", None, "refresh;  :ls -a  toggles dotfiles"),
                entry(":stat", None, "attributes (same as :attr)"),
                entry(":file", None, "what the selection is, by content"),
                entry(":wc", None, "line / word / byte counts"),
                entry(":head / :tail", None, "first / last lines;  :tail -n 40"),
                entry(":df", None, "free disk space;  :df -h -k -m -g"),
                entry(":zip", None, "bundle selection;  :zip -e  for a password"),
                entry(":!cmd", None, "run in shell;  % = selection, %f file, %d dir"),
            ],
        ),
        (
            "Shell panel (focus: click, Shift+J, or :shell)",
            vec![
                entry("F1-F8", None, "switch to shell tab 1-8"),
                entry("F9", None, "new shell tab"),
                entry("F10", None, "close shell tab"),
                entry("Shift+F1/F2", None, "focus next / previous split pane"),
                entry("Shift+F8", None, "v-split (panes side by side)"),
                entry("Shift+F9", None, "h-split (panes stacked)"),
                entry("Shift+F10", None, "close split pane (confirms)"),
                entry("F12", None, "zoom focused surface (toggle)"),
                entry("Shift+F12", None, "zoom active split pane (toggle)"),
                entry("Shift+drag", None, "select text to copy (the terminal's own selection)"),
                entry("right-click → Paste", None, "paste clipboard text into the shell"),
                entry("Esc", None, "back to files (full-screen apps keep it)"),
            ],
        ),
    ]
}

/// Render the manual, folding in the user's `init.lua` key overrides.
///
/// `cian.set_keymap` is additive — a user-bound key runs the action *in
/// addition to* the built-in key — so extra keys are appended rather than
/// replacing the defaults, which is exactly what the running app does.
pub fn manual_lines(keymap: &HashMap<char, Action>) -> Vec<String> {
    let mut out = vec!["cian — key manual".to_string()];
    for (title, entries) in manual_sections() {
        out.push(String::new());
        out.push(title.to_string());
        for e in entries {
            let mut keys = e.keys.to_string();
            if let Some(action) = e.action {
                // Extra keys the user bound to this action, sorted for stability.
                let mut extra: Vec<char> = keymap
                    .iter()
                    .filter(|(_, a)| **a == action)
                    .map(|(c, _)| *c)
                    .collect();
                extra.sort_unstable();
                for c in extra {
                    keys.push_str(&format!(", {}", c));
                }
            }
            out.push(format!("  {:<17} {}", keys, e.desc));
        }
    }
    out
}

/// Plain-text manual for `cian -man`, using the user's own config so the keys
/// it lists match the keys that will actually work.
pub fn manual_text() -> String {
    let config = cian_lua::load();
    let mut keymap: HashMap<char, Action> = HashMap::new();
    for (c, name) in &config.keymaps {
        if let Some(a) = action_from_name(name) {
            keymap.insert(*c, a);
        }
    }
    manual_lines(&keymap).join("\n")
}

/// Version line for `cian --version`.
///
/// Includes the commit because "which build am I running?" is otherwise
/// unanswerable, and an old exe left on PATH looks exactly like missing
/// features.
pub fn version_text() -> String {
    format!("cian {} ({})", env!("CARGO_PKG_VERSION"), env!("CIAN_COMMIT"))
}

/// One-screen usage synopsis for `cian -h`.
pub fn usage_text() -> String {
    // Report the paths this build actually resolves rather than the Unix
    // spelling: on Windows `~/.config/...` is not something the user can paste
    // anywhere, and "where does my config go?" is the first thing they need.
    let cfg = cian_lua::config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(could not resolve a home directory)".into());
    let shortcuts = ShortcutStore::default_path().display().to_string();

    [
        "cian — a two-pane terminal file manager".to_string(),
        String::new(),
        "USAGE:".to_string(),
        "    cian [LEFT_PATH] [RIGHT_PATH]".to_string(),
        String::new(),
        "ARGS:".to_string(),
        "    LEFT_PATH     directory for the left pane  (default: current dir)".to_string(),
        "    RIGHT_PATH    directory for the right pane (default: current dir)".to_string(),
        String::new(),
        "OPTIONS:".to_string(),
        "    -h, --help    show this help".to_string(),
        "    -V, --version show the version and commit".to_string(),
        "    -man, --man   show the full key manual (also ? or Ctrl+. in-app)".to_string(),
        String::new(),
        "CONFIG:".to_string(),
        format!("    {}", cfg),
        format!("    {}", shortcuts),
        "    (override the config directory with $CIAN_CONFIG_DIR)".to_string(),
        String::new(),
        "ENVIRONMENT:".to_string(),
        "    CIAN_LOG      append diagnostics to this file (debugging)".to_string(),
    ]
    .join("\n")
}

/// Note when the host terminal will not do cian justice.
///
/// cian cannot restyle the console it was launched into — the font and colors
/// belong to the host. Running `cian.exe` straight from Explorer or cmd lands
/// in the legacy console, where Nerd Font icons become boxes. Saying so once
/// at startup beats leaving it looking broken.
fn terminal_advice() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    if modern_terminal() {
        return Vec::new();
    }
    vec![
        "This looks like the legacy Windows console.".to_string(),
        "cian works, but file-type icons need a Nerd Font, which that console".to_string(),
        "cannot use. For the intended look, start it from Windows Terminal:".to_string(),
        String::new(),
        "    wt cian".to_string(),
        String::new(),
        "or from WezTerm. (This notice only appears in the legacy console.)".to_string(),
    ]
}

/// Restore the terminal before a panic unwinds out of the TUI.
///
/// Without this, a panic leaves the terminal in raw mode inside the alternate
/// screen: the panic message is invisible, the shell prompt is unusable, and
/// the user has to run `reset`. The hook puts the terminal back first, so the
/// backtrace lands on a normal screen (and in `$CIAN_LOG` if enabled).
fn install_panic_hook() {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut out = io::stdout();
        let _ = execute!(out, PopKeyboardEnhancementFlags);
        let _ = execute!(out, DisableBracketedPaste);
        let _ = execute!(out, DisableMouseCapture);
        let _ = disable_raw_mode();
        let _ = execute!(out, LeaveAlternateScreen);
        cian_core::log::log(&format!("PANIC: {}", info));
        original(info);
    }));
}

pub fn run(left: PathBuf, right: PathBuf) -> Result<()> {
    // Load user config (never fails; problems are reported below).
    let config = cian_lua::load();

    // Resolve and install the color theme before any drawing happens.
    let (resolved, theme_errors) = resolve_theme(&config.theme);
    let _ = THEME.set(resolved);
    let _ = BORDERS.set(resolve_border_type(config.options.borders.as_deref()));

    // Collect all non-fatal config issues for a single startup notice.
    let mut startup_errors = config.errors.clone();
    startup_errors.extend(theme_errors);
    for (c, name) in &config.keymaps {
        if action_from_name(name).is_none() {
            startup_errors.push(format!("keymap: unknown action {:?} (key '{}')", name, c));
        }
    }

    startup_errors.extend(terminal_advice());

    let mut app = App::new(left, right, config)?;
    if !startup_errors.is_empty() {
        let mut lines = vec!["config loaded with issues:".to_string(), String::new()];
        let total = startup_errors.len();
        lines.extend(startup_errors.into_iter().take(10));
        if total > 10 {
            lines.push(format!("... and {} more", total - 10));
        }
        app.popup = Popup::Notice { lines };
    }

    install_panic_hook();
    cian_core::log::log("cian starting");

    // Name the window. Costs nothing and stops a bare `cian.exe` from sitting
    // in a console still labelled with whatever launched it.
    let _ = execute!(io::stdout(), SetTitle("cian"));

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    // Ask the terminal to disambiguate Ctrl-h / Ctrl-i / Ctrl-m from Backspace/Tab/Enter.
    // Supported by WezTerm, kitty, foot, etc. Silently ignored elsewhere.
    let kbd_enhanced = execute!(
        stdout,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    )
    .is_ok();

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app);

    if kbd_enhanced {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(terminal.backend_mut(), DisableBracketedPaste);
    let _ = execute!(terminal.backend_mut(), DisableMouseCapture);
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    let mut needs_redraw = true;
    loop {
        if needs_redraw {
            terminal.draw(|f| draw(f, app))?;
            needs_redraw = false;
        }
        // Short timeout so live shell output is picked up promptly; we only
        // actually repaint when something changed (input, resize, or new
        // shell output), so the loop stays cheap when idle. While a transition
        // or flash is running we tick faster so the motion stays smooth.
        let tick =
            if app.anim.is_some() || app.flash.is_some() || app.op_job.is_some() { 16 } else { 33 };
        if event::poll(Duration::from_millis(tick))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    // Input always wins over eye candy: land any transition
                    // immediately rather than making the user wait for it.
                    app.finish_anim();
                    app.handle_key(key)?;
                    needs_redraw = true;
                }
                Event::Mouse(m) => {
                    app.handle_mouse(m);
                    needs_redraw = true;
                }
                // A terminal paste (Cmd/Ctrl+V, right-click, middle-click)
                // arrives whole rather than as keystrokes, so it lands in the
                // active field atomically and its newlines are stripped.
                Event::Paste(text) => {
                    app.insert_into_active_text(&text);
                    needs_redraw = true;
                }
                Event::Resize(_, _) => needs_redraw = true,
                _ => {}
            }
        }
        // Repaint when any pane in the active shell tab produced new output.
        if app.shell.take_active_tab_dirty() {
            needs_redraw = true;
        }
        // Install the shell tab once its background spawn (see `ensure`) lands.
        if app.shell.poll_pending() {
            needs_redraw = true;
        }
        // A connection picked before the shell finished starting.
        if app.pending_shell_input.is_some() {
            app.flush_pending_shell_input();
            needs_redraw = true;
        }
        // ssh asks for a password on its own schedule, so watch for the prompt
        // rather than sending blindly.
        if app.pending_auth.is_some() {
            needs_redraw |= app.poll_pending_auth();
        }
        // A freshly-created split grows in from nothing.
        if let Some((tab, node)) = app.shell.just_split.take() {
            app.start_anim(AnimKind::Ratio {
                target: DividerTarget::ShellSplit { tab, node },
                from: 100,
                to: 50,
            });
        }
        // Drive any transition in flight, landing it when its time is up.
        if let Some(a) = app.anim {
            needs_redraw = true;
            if a.done() {
                app.finish_anim();
            }
        }
        // Search results stream in while the walk continues.
        if app.find_job.is_some() {
            needs_redraw |= app.poll_find_job();
        }
        // Catch changes made by anything other than cian.
        if app.poll_external_changes() {
            needs_redraw = true;
        }
        // A running file operation reports in over a channel.
        if app.op_job.is_some() {
            needs_redraw |= app.poll_op_job();
        }
        // A fading flash needs frames of its own; clear it once it expires so
        // the loop can go back to sleep.
        if app.flash.is_some() {
            needs_redraw = true;
            if !app.flash_active() {
                app.flash = None;
            }
        }
        // If the focused pane's shell has exited (e.g. the user typed `exit`),
        // close that pane; if its tab (and the whole panel) empties, return to
        // the files so we never strand the user typing into a dead shell.
        if app.focused == FocusedPane::Shell {
            let exited = app
                .shell
                .active_session_mut()
                .map(|s| !s.is_alive())
                .unwrap_or(false);
            if exited {
                let empty = app.shell.close_active_pane();
                if empty {
                    let back = app.last_file_pane;
                    app.focus(back);
                }
                app.message = Some("shell exited".into());
                needs_redraw = true;
            }
        }
        if app.should_quit {
            return Ok(());
        }
    }
}

/// Normal three-surface layout: left/right file panes on top, shell below.
fn draw_split(f: &mut Frame, main_area: Rect, app: &mut App, ov: AnimOverride) {
    let main_pct = ov.ratio_for(DividerTarget::Main, app.main_pct);
    let main_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(main_pct), Constraint::Percentage(100 - main_pct)])
        .split(main_area);
    let panes_area = main_split[0];
    let shell_area = main_split[1];

    let panes_pct = ov.ratio_for(DividerTarget::Panes, app.panes_pct);
    let panes_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(panes_pct), Constraint::Percentage(100 - panes_pct)])
        .split(panes_area);

    app.layout_rects = LayoutRects {
        left: panes_split[0],
        right: panes_split[1],
        shell: shell_area,
    };

    let mut leaves = Vec::new();
    let mut dividers = vec![
        Divider {
            zone: seam_zone(Direction::Vertical, panes_area, shell_area),
            parent: main_area,
            dir: Direction::Vertical,
            target: DividerTarget::Main,
        },
        Divider {
            zone: seam_zone(Direction::Horizontal, panes_split[0], panes_split[1]),
            parent: panes_area,
            dir: Direction::Horizontal,
            target: DividerTarget::Panes,
        },
    ];

    let visual_for_left = if app.focused == FocusedPane::Left { app.visual_anchor } else { None };
    let visual_for_right = if app.focused == FocusedPane::Right { app.visual_anchor } else { None };

    let (bg_l, bg_r) = (app.pane_bg[0], app.pane_bg[1]);
    let (fl_l, fl_r) = (app.flash_level(FocusedPane::Left), app.flash_level(FocusedPane::Right));
    draw_file_pane(f, panes_split[0], &app.left, app.focused == FocusedPane::Left, visual_for_left, app.mode, bg_l, fl_l);
    draw_file_pane(f, panes_split[1], &app.right, app.focused == FocusedPane::Right, visual_for_right, app.mode, bg_r, fl_r);
    // draw_shell sizes each pane's PTY to its computed sub-rect.
    draw_shell(f, shell_area, &mut app.shell, app.focused == FocusedPane::Shell, &mut dividers, &mut leaves, ov);
    app.dividers = dividers;
    app.shell_leaves = leaves;
}

/// The focused surface drawn at an arbitrary rect, used as the floating layer
/// of a zoom transition. Deliberately does not touch `app.layout_rects`: the
/// backdrop already set those, and hit-testing should follow the resting
/// layout rather than a rect that is still moving.
fn draw_zoom_overlay(f: &mut Frame, rect: Rect, app: &mut App, ov: AnimOverride) {
    let mut sink = Vec::new();
    match app.focused {
        FocusedPane::Left => {
            let (bg, fl) = (app.pane_bg[0], app.flash_level(FocusedPane::Left));
            let va = app.visual_anchor;
            draw_file_pane(f, rect, &app.left, true, va, app.mode, bg, fl);
        }
        FocusedPane::Right => {
            let (bg, fl) = (app.pane_bg[1], app.flash_level(FocusedPane::Right));
            let va = app.visual_anchor;
            draw_file_pane(f, rect, &app.right, true, va, app.mode, bg, fl);
        }
        FocusedPane::Shell => {
            draw_shell(f, rect, &mut app.shell, true, &mut sink, &mut Vec::new(), ov);
        }
    }
}

/// Zoomed layout: only the focused surface, filling the available area.
fn draw_zoomed(f: &mut Frame, area: Rect, app: &mut App, ov: AnimOverride) {
    let mut rects = LayoutRects::default();
    // Only the shell's internal splits are draggable while zoomed; the
    // main/panes borders are not on screen.
    let mut dividers = Vec::new();
    let mut leaves = Vec::new();
    match app.focused {
        FocusedPane::Left => {
            rects.left = area;
            app.layout_rects = rects;
            let va = app.visual_anchor;
            let (bg, fl) = (app.pane_bg[0], app.flash_level(FocusedPane::Left));
            draw_file_pane(f, area, &app.left, true, va, app.mode, bg, fl);
        }
        FocusedPane::Right => {
            rects.right = area;
            app.layout_rects = rects;
            let va = app.visual_anchor;
            let (bg, fl) = (app.pane_bg[1], app.flash_level(FocusedPane::Right));
            draw_file_pane(f, area, &app.right, true, va, app.mode, bg, fl);
        }
        FocusedPane::Shell => {
            rects.shell = area;
            app.layout_rects = rects;
            draw_shell(f, area, &mut app.shell, true, &mut dividers, &mut leaves, ov);
        }
    }
    app.dividers = dividers;
    app.shell_leaves = leaves;
}

fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    // Command and filter modes add a prompt line above the status bar; the key
    // hints take another. A very short window drops the hints rather than the
    // listing.
    let prompt_line = matches!(app.mode, Mode::Command | Mode::Filter);
    let hint_line = app.show_key_hints && area.height >= 12;
    let bottom_lines = 1 + u16::from(prompt_line) + u16::from(hint_line);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(bottom_lines)])
        .split(area);
    let main_area = vertical[0];
    let bottom_area = vertical[1];

    let ov = app.anim_override();
    // A zoom transition draws the normal layout as a backdrop and floats the
    // zooming surface above it, so it visibly grows out of (or shrinks back
    // into) its own pane.
    if let Some(Anim { kind: AnimKind::Zoom { from, to }, .. }) = app.anim {
        let t = app.anim.map(|a| a.progress()).unwrap_or(1.0);
        draw_split(f, main_area, app, ov);
        let rect = lerp_rect(from, to, t);
        f.render_widget(Clear, rect);
        draw_zoom_overlay(f, rect, app, ov);
    } else if app.zoomed {
        draw_zoomed(f, main_area, app, ov);
    } else {
        draw_split(f, main_area, app, ov);
    }

    // Stack the bottom rows: [prompt] [hints] [status]. Each is claimed only if
    // the strip actually has room — a window can be short enough that Layout
    // hands back fewer rows than were asked for, and writing past the buffer
    // panics.
    let end = bottom_area.y.saturating_add(bottom_area.height);
    let mut row = bottom_area.y;
    let claim = |row: &mut u16| -> Option<Rect> {
        if *row >= end {
            return None;
        }
        let r = Rect::new(bottom_area.x, *row, bottom_area.width, 1);
        *row += 1;
        Some(r)
    };

    // Note: `claim` must only be called for rows that are actually drawn, so
    // each branch guards its flag *before* claiming.
    if prompt_line {
        if let Some(cmd_area) = claim(&mut row) {
            if app.mode == Mode::Filter {
                let matched = app.active_pane().map(|p| p.entries.len()).unwrap_or(0);
                let total = app.active_pane().map(|p| p.all_entries.len()).unwrap_or(0);
                draw_prompt_line(
                    f,
                    cmd_area,
                    &format!("filter /{}_", app.filter_buffer),
                    &format!("{}/{} match  Enter=keep  Esc=clear", matched, total),
                );
            } else {
                draw_command_line(f, cmd_area, &app.command_buffer);
            }
        }
    }
    if hint_line {
        if let Some(r) = claim(&mut row) {
            draw_key_hints(f, r, app);
        }
    }
    if let Some(r) = claim(&mut row) {
        draw_status(f, r, app);
    }

    if app.op_job.is_some() {
        draw_op_progress(f, area, app);
    }
    if !matches!(app.popup, Popup::None) {
        let find_state = app
            .find_job
            .as_ref()
            .map(|j| (j.query.as_str(), j.root_label.as_str(), j.done, j.mode));
        let dests = app.dest_choices();
        draw_popup(f, area, &mut app.popup, &app.config.ssh_hosts, find_state, &dests);
    }
}

/// Build a tab strip. Active tab uses full path; inactive tabs use just the
/// directory name. If the labels overflow `max_width`, the rest collapse into
/// a `+N` marker so the active tab stays visible.
fn tabs_title<'a>(tabs: &'a PaneTabs, focused: bool, focus_bg: Color, max_width: u16) -> Line<'a> {
    fn label_for(i: usize, tab: &Pane, is_active: bool) -> String {
        let main = if is_active {
            tab.cwd.display().to_string()
        } else {
            tab.cwd
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| tab.cwd.display().to_string())
        };
        format!(" {} {} ", i + 1, main)
    }
    let width_of = |s: &str| s.chars().count() as u16;

    // First, lay out tabs starting from the active one outward so it never gets cut.
    let active = tabs.active.min(tabs.tabs.len().saturating_sub(1));
    let total = tabs.tabs.len();
    let mut shown: Vec<usize> = vec![active];
    let mut used: u16 = width_of(&label_for(active, &tabs.tabs[active], true));
    let sep_w: u16 = 1;
    let reserve: u16 = 5; // for " +N "

    let (mut left, mut right) = (active, active);
    loop {
        let try_right = right + 1 < total;
        let try_left = left > 0;
        if !try_right && !try_left { break; }
        // prefer expanding right first (chronological order)
        if try_right {
            let i = right + 1;
            let w = width_of(&label_for(i, &tabs.tabs[i], false)) + sep_w;
            let need_reserve = if i + 1 < total || left > 0 { reserve } else { 0 };
            if used + w + need_reserve <= max_width {
                shown.push(i);
                used += w;
                right = i;
                continue;
            }
        }
        if try_left {
            let i = left - 1;
            let w = width_of(&label_for(i, &tabs.tabs[i], false)) + sep_w;
            let need_reserve = if i > 0 || right + 1 < total { reserve } else { 0 };
            if used + w + need_reserve <= max_width {
                shown.insert(0, i);
                used += w;
                left = i;
                continue;
            }
        }
        break;
    }
    let hidden_left = left;
    let hidden_right = total.saturating_sub(right + 1);

    let mut spans: Vec<Span<'a>> = Vec::new();
    spans.push(Span::raw(" "));
    if hidden_left > 0 {
        spans.push(Span::styled(
            format!("+{} ", hidden_left),
            Style::default().fg(Color::DarkGray),
        ));
    }
    for (pos, &i) in shown.iter().enumerate() {
        let is_active = i == active;
        let style = if is_active {
            if focused {
                Style::default().fg(Color::Black).bg(focus_bg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).bg(Color::DarkGray)
            }
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let label = label_for(i, &tabs.tabs[i], is_active);
        spans.push(Span::styled(label, style));
        if pos + 1 < shown.len() {
            spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));
        }
    }
    if hidden_right > 0 {
        spans.push(Span::styled(
            format!(" +{}", hidden_right),
            Style::default().fg(Color::DarkGray),
        ));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// Pick a Nerd Font glyph based on the entry name/extension.
fn icon_for(entry: &cian_core::Entry) -> &'static str {
    if entry.is_dir {
        return match entry.name.as_str() {
            ".git" => "\u{e702}",
            ".github" => "\u{f408}",
            "node_modules" => "\u{e5fa}",
            "src" => "\u{f121}",
            "tests" | "test" => "\u{f0c3}",
            "docs" | "doc" => "\u{f02d}",
            "target" | "build" | "dist" | "out" => "\u{f1c6}",
            ".vscode" | ".idea" => "\u{e7c5}",
            _ => "\u{f07b}",
        };
    }
    let lower = entry.name.to_lowercase();
    match lower.as_str() {
        "cargo.toml" | "cargo.lock" => return "\u{e7a8}",
        "dockerfile" | ".dockerignore" => return "\u{f308}",
        "makefile" => return "\u{e779}",
        "readme.md" | "readme" => return "\u{f48a}",
        "license" | "license.md" => return "\u{f02d}",
        ".gitignore" | ".gitattributes" | ".gitmodules" => return "\u{f1d3}",
        ".env" | ".env.local" => return "\u{f462}",
        "package.json" | "package-lock.json" | "yarn.lock" => return "\u{e60b}",
        _ => {}
    }
    let ext = std::path::Path::new(&entry.name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "rs" => "\u{e7a8}",
        "py" => "\u{e73c}",
        "js" | "mjs" | "cjs" => "\u{f2ee}",
        "ts" | "tsx" | "jsx" => "\u{e628}",
        "go" => "\u{e627}",
        "c" | "h" => "\u{e61e}",
        "cpp" | "cc" | "cxx" | "hpp" => "\u{e61d}",
        "java" => "\u{e738}",
        "rb" => "\u{e21e}",
        "php" => "\u{e608}",
        "lua" => "\u{e620}",
        "swift" => "\u{e755}",
        "kt" | "kts" => "\u{e634}",
        "md" | "markdown" => "\u{f48a}",
        "json" | "jsonc" => "\u{e60b}",
        "yaml" | "yml" => "\u{f481}",
        "toml" | "ini" | "conf" | "cfg" => "\u{f013}",
        "xml" => "\u{f72d}",
        "html" | "htm" => "\u{f13b}",
        "css" | "scss" | "sass" | "less" => "\u{f13c}",
        "vue" => "\u{fd42}",
        "svelte" => "\u{e697}",
        "sh" | "bash" | "zsh" | "fish" => "\u{f489}",
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" | "tif" | "tiff" => "\u{f1c5}",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => "\u{f001}",
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "wmv" => "\u{f03d}",
        "pdf" => "\u{f1c1}",
        "zip" | "tar" | "gz" | "7z" | "rar" | "bz2" | "xz" => "\u{f1c6}",
        "txt" | "log" => "\u{f0f6}",
        "exe" | "dll" | "so" | "dylib" => "\u{f013}",
        _ => "\u{f15c}",
    }
}

/// Broad kinds of file, used to color the listing.
///
/// Deliberately coarse: the point is that a glance separates "code" from
/// "archive" from "image", not that every extension gets its own hue. Too many
/// colors read as noise rather than structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileKind {
    Directory,
    Code,
    Config,
    Document,
    Image,
    Media,
    Archive,
    Executable,
    /// Dotfiles and other things that are usually background noise.
    Muted,
    Plain,
}

impl FileKind {
    fn color(self) -> Color {
        match self {
            // Not `Color::Blue`: the terminal's ANSI blue is #0000ee, which is
            // close to unreadable on a dark background.
            FileKind::Directory => Color::Rgb(96, 165, 250),
            FileKind::Code => Color::Rgb(250, 204, 21),
            FileKind::Config => Color::Rgb(148, 190, 210),
            FileKind::Document => Color::Rgb(226, 226, 236),
            FileKind::Image => Color::Rgb(216, 130, 220),
            FileKind::Media => Color::Rgb(120, 200, 190),
            FileKind::Archive => Color::Rgb(240, 130, 120),
            FileKind::Executable => Color::Rgb(126, 217, 130),
            FileKind::Muted => Color::Rgb(128, 128, 148),
            FileKind::Plain => Color::Rgb(205, 205, 218),
        }
    }

    fn bold(self) -> bool {
        matches!(self, FileKind::Directory | FileKind::Executable)
    }
}

/// Classify an entry for coloring. Mirrors the categories [`icon_for`] draws
/// from, so a file's icon and its color always agree.
fn kind_for(entry: &cian_core::Entry) -> FileKind {
    if entry.is_dir {
        return FileKind::Directory;
    }
    // Dotfiles recede: they are rarely the thing being looked for.
    if entry.name.starts_with('.') {
        return FileKind::Muted;
    }
    let ext = std::path::Path::new(&entry.name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "rs" | "py" | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "go" | "c" | "h" | "cpp"
        | "cc" | "cxx" | "hpp" | "java" | "rb" | "php" | "lua" | "swift" | "kt" | "kts"
        | "vue" | "svelte" | "html" | "htm" | "css" | "scss" | "sass" | "less" => FileKind::Code,
        "toml" | "ini" | "conf" | "cfg" | "yaml" | "yml" | "json" | "jsonc" | "xml" | "env" => {
            FileKind::Config
        }
        "md" | "markdown" | "txt" | "log" | "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt"
        | "pptx" | "rtf" | "csv" | "tsv" => FileKind::Document,
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" | "tif" | "tiff" => {
            FileKind::Image
        }
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "mp4" | "mov" | "mkv" | "avi" | "webm"
        | "wmv" => FileKind::Media,
        "zip" | "tar" | "gz" | "7z" | "rar" | "bz2" | "xz" | "zst" | "tgz" => FileKind::Archive,
        "exe" | "msi" | "bat" | "cmd" | "ps1" | "sh" | "bash" | "zsh" | "fish" | "app"
        | "dll" | "so" | "dylib" => FileKind::Executable,
        _ => FileKind::Plain,
    }
}

fn shell_tabs_title<'a>(tabs: &'a ShellPane, focused: bool) -> Line<'a> {
    let mut spans: Vec<Span<'a>> = Vec::new();
    spans.push(Span::raw(" "));
    for i in 0..tabs.count().max(1) {
        let label = format!(" shell {} ", i + 1);
        let style = if i == tabs.active {
            if focused {
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).bg(Color::DarkGray)
            }
        } else {
            // Readable medium grey for inactive tabs (DarkGray was too dim).
            Style::default().fg(Color::Gray)
        };
        spans.push(Span::styled(label, style));
        if i + 1 < tabs.count() {
            spans.push(Span::styled("│", Style::default().fg(Color::Gray)));
        }
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

#[allow(clippy::too_many_arguments)]
fn draw_file_pane(
    f: &mut Frame,
    area: Rect,
    tabs: &PaneTabs,
    focused: bool,
    visual_anchor: Option<usize>,
    mode: Mode,
    bg: Option<Color>,
    flash: f32,
) {
    let focus_bg = focus_badge_color(mode);
    let mut border_style = if focused {
        Style::default().fg(focus_bg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    // An operation that just landed here lights the border, fading out.
    if flash > 0.0 {
        border_style = Style::default().fg(fade(theme().accent, flash)).add_modifier(Modifier::BOLD);
    }
    let max_title_w = area.width.saturating_sub(2);
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(border_style)
        .title(tabs_title(tabs, focused, focus_bg, max_title_w));
    if let Some(c) = bg {
        block = block.style(Style::default().bg(c));
    }

    let pane = tabs.active_ref();
    let visual_range = visual_anchor.map(|a| {
        if a <= pane.cursor { (a, pane.cursor) } else { (pane.cursor, a) }
    });

    // Columns are dropped progressively on narrow panes so the name always
    // keeps a usable amount of room.
    let inner_w = area.width.saturating_sub(2);
    let show_time = inner_w >= 52;
    let show_size = inner_w >= 34;
    let meta_w = if show_time { SIZE_COL_W + TIME_COL_W + 2 } else if show_size { SIZE_COL_W + 1 } else { 0 };
    // 2 mark + icon + 2 spaces
    let name_w = inner_w.saturating_sub(meta_w + 5) as usize;

    let items: Vec<ListItem> = pane.entries.iter().enumerate().map(|(i, e)| {
        let marked = pane.is_marked(i);
        let in_visual = visual_range.map(|(a, b)| i >= a && i <= b).unwrap_or(false);
        let mark_symbol = if marked { "● " } else { "  " };
        let mark_style = Style::default().fg(theme().mark_fg).add_modifier(Modifier::BOLD);
        let kind = kind_for(e);
        let mut name_style = Style::default().fg(kind.color());
        if kind.bold() {
            name_style = name_style.add_modifier(Modifier::BOLD);
        }
        // The icon carries the same color so the row reads as one unit.
        let icon_style = Style::default().fg(kind.color());

        let name = truncate(&e.name, name_w);
        let mut spans = vec![
            Span::styled(mark_symbol, mark_style),
            Span::styled(format!("{}  ", icon_for(e)), icon_style),
            Span::styled(format!("{:<w$}", name, w = name_w), name_style),
        ];
        let meta_style = Style::default().fg(Color::Rgb(130, 130, 155));
        if show_size {
            // Directories have no meaningful byte count of their own.
            let s = if e.is_dir { "—".to_string() } else { cian_core::human_size(e.len) };
            spans.push(Span::styled(
                format!(" {:>w$}", s, w = SIZE_COL_W as usize),
                meta_style,
            ));
        }
        if show_time {
            let t = e.modified.map(cian_core::format_time).unwrap_or_else(|| "-".into());
            spans.push(Span::styled(format!(" {}", t), meta_style));
        }

        let mut item = ListItem::new(Line::from(spans));
        if in_visual { item = item.style(Style::default().bg(theme().visual_bg)); }
        item
    }).collect();

    // An unfocused pane recedes so the focused one reads as the active surface.
    let mut list_style = if focused {
        Style::default()
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    if let Some(c) = bg {
        list_style = list_style.bg(c);
    }
    let list = List::new(items)
        .block(block)
        .style(list_style)
        .highlight_style(
            Style::default().bg(theme().selected_bg).add_modifier(Modifier::BOLD),
        );

    let mut state = ListState::default();
    if !pane.entries.is_empty() { state.select(Some(pane.cursor)); }
    f.render_stateful_widget(list, area, &mut state);

    draw_list_scrollbar(f, area, pane.entries.len(), pane.cursor, focused, border_style);
}

/// Fixed widths so the columns line up between the two panes.
const SIZE_COL_W: u16 = 5;
const TIME_COL_W: u16 = 16;

/// Draw a scrollbar on a pane's right border when the listing overflows.
fn draw_list_scrollbar(
    f: &mut Frame,
    area: Rect,
    total: usize,
    cursor: usize,
    focused: bool,
    border: Style,
) {
    let view_h = area.height.saturating_sub(2);
    if view_h == 0 || total <= view_h as usize {
        return;
    }
    let track = Rect::new(area.x + area.width.saturating_sub(1), area.y + 1, 1, view_h);
    let mut state = ScrollbarState::new(total).position(cursor);
    // The bar sits *on* the pane's right border, so the track has to be the
    // border: same glyph, same style. Drawing it in its own dimmer color made
    // the right edge look broken — bright where the thumb was, faded
    // elsewhere, while the other three sides stayed the border color.
    let thumb = if focused {
        border.add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::Rgb(120, 120, 145))
    };
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_symbol("│")
            .thumb_style(thumb)
            .track_symbol(Some("│"))
            .track_style(border)
            .begin_symbol(None)
            .end_symbol(None),
        track,
        &mut state,
    );
}

/// Draw the shell panel, then apply its background tint.
///
/// The tint has to be a post-pass. The PTY widget writes an explicit `Reset`
/// background into every cell the shell left uncolored, which would clobber
/// any background set on the block underneath. Recoloring only the cells
/// that are still `Reset` tints the panel while leaving alone every color
/// the shell chose for itself (ls colors, a vim theme, and so on).
#[allow(clippy::too_many_arguments)]
fn draw_shell(
    f: &mut Frame,
    area: Rect,
    shell: &mut ShellPane,
    focused: bool,
    dividers: &mut Vec<Divider>,
    leaves: &mut Vec<(usize, usize, Rect)>,
    ov: AnimOverride,
) {
    draw_shell_inner(f, area, shell, focused, dividers, leaves, ov);
}

/// Repaint every still-uncolored cell in `area` with `bg`.
fn tint_default_cells(f: &mut Frame, area: Rect, bg: Color) {
    let buf = f.buffer_mut();
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            if let Some(cell) = buf.cell_mut((x, y)) {
                if cell.bg == Color::Reset {
                    cell.set_bg(bg);
                }
            }
        }
    }
}

fn draw_shell_inner(
    f: &mut Frame,
    area: Rect,
    shell: &mut ShellPane,
    focused: bool,
    dividers: &mut Vec<Divider>,
    leaves: &mut Vec<(usize, usize, Rect)>,
    ov: AnimOverride,
) {
    let border_style = if focused {
        Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(border_style)
        .title(shell_tabs_title(shell, focused));
    let inner = area.inner(Margin { vertical: 1, horizontal: 1 });
    f.render_widget(block, area);

    // Remember the inner size for sizing newly-spawned panes.
    shell.rows = inner.height.max(1);
    shell.cols = inner.width.max(1);

    let active = shell.active;
    if shell.tabs.get(active).is_none() {
        let body = if let Some(err) = &shell.error {
            format!("shell failed to start: {}", err)
        } else if shell.is_starting() {
            "starting shell…".to_string()
        } else {
            "shell pane — focus here (Shift+J / click / :shell) to start a shell. \
             Esc returns to the files."
                .to_string()
        };
        f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), inner);
        return;
    }

    // Shift+F12: show only the active leaf, filling the panel.
    if shell.zoom_pane {
        let leaf = shell.tabs[active].active;
        if let Some(tab) = shell.tabs.get_mut(active) {
            if let Some(Node::Leaf { session: s, .. }) = tab.nodes.get_mut(leaf).and_then(|n| n.as_mut()) {
                s.resize(inner.height.max(1), inner.width.max(1));
            }
        }
        if let Some(Node::Leaf { session: s, bg }) = shell.tabs[active].nodes.get(leaf).and_then(|n| n.as_ref()) {
            if let Ok(parser) = s.parser().lock() {
                f.render_widget(PseudoTerminal::new(parser.screen()), inner);
            }
            if let Some(c) = bg {
                tint_default_cells(f, inner, *c);
            }
        }
        return;
    }

    let root = shell.tabs[active].root;
    // While a transition runs the PTYs keep their old size; the real resize
    // happens on the frame after it lands.
    if !ov.freeze_pty {
        if let Some(tab) = shell.tabs.get_mut(active) {
            resize_node(tab, active, root, inner, false, ov);
        }
    }
    let tab = &shell.tabs[active];
    render_node(f, tab, active, root, inner, tab.active, focused, false, dividers, leaves, ov);
}

/// Recursively size each leaf's PTY to its rect. `bordered` is true for leaves
/// inside a split (which draw a 1-cell border), false for a lone root leaf.
fn resize_node(tab: &mut ShellTab, tab_idx: usize, i: usize, area: Rect, bordered: bool, ov: AnimOverride) {
    let split = match tab.nodes.get(i).and_then(|n| n.as_ref()) {
        Some(Node::Split { dir, first, second, ratio }) => Some((*dir, *first, *second, *ratio)),
        Some(Node::Leaf { .. }) => None,
        None => return,
    };
    match split {
        None => {
            let (h, w) = if bordered {
                (area.height.saturating_sub(2).max(1), area.width.saturating_sub(2).max(1))
            } else {
                (area.height.max(1), area.width.max(1))
            };
            if let Some(Node::Leaf { session: s, .. }) = tab.nodes[i].as_mut() {
                s.resize(h, w);
            }
        }
        Some((dir, first, second, ratio)) => {
            let r = ov.ratio_for(DividerTarget::ShellSplit { tab: tab_idx, node: i }, ratio);
            let rects = split_rects(dir, area, r);
            resize_node(tab, tab_idx, first, rects.0, true, ov);
            resize_node(tab, tab_idx, second, rects.1, true, ov);
        }
    }
}

/// Recursively render the split tree. Leaves inside a split get a border (the
/// active one highlighted); a lone root leaf fills its area without one.
#[allow(clippy::too_many_arguments)]
fn render_node(
    f: &mut Frame,
    tab: &ShellTab,
    tab_idx: usize,
    i: usize,
    area: Rect,
    active_leaf: usize,
    focused: bool,
    bordered: bool,
    dividers: &mut Vec<Divider>,
    leaves: &mut Vec<(usize, usize, Rect)>,
    ov: AnimOverride,
) {
    match tab.nodes.get(i).and_then(|n| n.as_ref()) {
        Some(Node::Leaf { session, bg }) => {
            leaves.push((tab_idx, i, area));
            let target = if bordered {
                let is_active = focused && i == active_leaf;
                let bs = if is_active {
                    Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let blk = Block::default().borders(Borders::ALL)
        .border_type(border_type()).border_style(bs);
                let pinner = area.inner(Margin { vertical: 1, horizontal: 1 });
                f.render_widget(blk, area);
                pinner
            } else {
                area
            };
            if let Ok(parser) = session.parser().lock() {
                f.render_widget(PseudoTerminal::new(parser.screen()), target);
            }
            // Tint after the PTY has drawn: it writes an explicit Reset
            // background into every cell the shell left uncolored, which would
            // otherwise clobber anything set underneath.
            if let Some(c) = bg {
                tint_default_cells(f, area, *c);
            }
        }
        Some(Node::Split { dir, first, second, ratio }) => {
            let target = DividerTarget::ShellSplit { tab: tab_idx, node: i };
            let rects = split_rects(*dir, area, ov.ratio_for(target, *ratio));
            let d = match dir {
                SplitDir::LeftRight => Direction::Horizontal,
                SplitDir::TopBottom => Direction::Vertical,
            };
            dividers.push(Divider {
                zone: seam_zone(d, rects.0, rects.1),
                parent: area,
                dir: d,
                target,
            });
            render_node(f, tab, tab_idx, *first, rects.0, active_leaf, focused, true, dividers, leaves, ov);
            render_node(f, tab, tab_idx, *second, rects.1, active_leaf, focused, true, dividers, leaves, ov);
        }
        None => {}
    }
}

/// Split a rect along `dir`, giving `ratio` percent of it to the first child.
fn split_rects(dir: SplitDir, area: Rect, ratio: u16) -> (Rect, Rect) {
    let direction = match dir {
        SplitDir::LeftRight => Direction::Horizontal,
        SplitDir::TopBottom => Direction::Vertical,
    };
    let first = ratio.min(100);
    let rects = Layout::default()
        .direction(direction)
        .constraints([Constraint::Percentage(first), Constraint::Percentage(100 - first)])
        .split(area);
    (rects[0], rects[1])
}

/// The band of cells that counts as grabbing the border between `a` and `b`.
/// The two rects are adjacent, so the seam is the last row/column of `a` plus
/// the first of `b` — two cells, which is a comfortable grab target.
fn seam_zone(dir: Direction, a: Rect, b: Rect) -> Rect {
    match dir {
        Direction::Horizontal => Rect {
            x: a.x + a.width.saturating_sub(1),
            y: a.y,
            width: 2.min(b.x + b.width - (a.x + a.width.saturating_sub(1))),
            height: a.height,
        },
        Direction::Vertical => Rect {
            x: a.x,
            y: a.y + a.height.saturating_sub(1),
            width: a.width,
            height: 2.min(b.y + b.height - (a.y + a.height.saturating_sub(1))),
        },
    }
}

/// A prompt line with a right-aligned hint, used by filter mode.
fn draw_prompt_line(f: &mut Frame, area: Rect, left: &str, right: &str) {
    let style = Style::default()
        .bg(Color::Rgb(20, 20, 30))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    f.render_widget(Paragraph::new(left).style(style), area);
    let w = right.chars().count() as u16 + 1;
    if area.width > w {
        let hint = Rect::new(area.x + area.width - w, area.y, w, 1);
        f.render_widget(
            Paragraph::new(right).style(style.fg(Color::DarkGray).remove_modifier(Modifier::BOLD)),
            hint,
        );
    }
}

fn draw_command_line(f: &mut Frame, area: Rect, buf: &str) {
    let text = format!(":{}", buf);
    let p = Paragraph::new(text).style(
        Style::default().bg(Color::Rgb(20, 20, 30)).fg(Color::White).add_modifier(Modifier::BOLD),
    );
    f.render_widget(p, area);
}

/// Blend `c` toward white by `t` (0 = unchanged, 1 = fully lit). Used for the
/// operation flash, which fades a border back to its resting color.
fn fade(c: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = match c {
        Color::Rgb(r, g, b) => (r, g, b),
        // Named colors have no components to blend; approximate with a light
        // neutral so the flash still reads.
        _ => (200, 220, 255),
    };
    let mix = |v: u8| (v as f32 + (255.0 - v as f32) * t) as u8;
    Color::Rgb(mix(r), mix(g), mix(b))
}

fn focus_badge_color(mode: Mode) -> Color {
    match mode {
        Mode::Normal => theme().accent,
        Mode::Visual => Color::Rgb(255, 140, 0),
        Mode::Search => Color::Rgb(80, 200, 120),
        Mode::Command => Color::Rgb(200, 100, 200),
        Mode::Filter => Color::Rgb(80, 200, 120),
        Mode::Shell => Color::Rgb(200, 160, 60),
    }
}

/// The keys worth advertising in the current context.
///
/// Deliberately short and mode-specific: a bar listing everything is wallpaper
/// that stops being read. `?` is always last so the full manual is reachable
/// from whatever state the user is stuck in.
fn key_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    if app.focused == FocusedPane::Shell {
        let mut v = vec![("Esc", "files")];
        // Moving between split panes only exists once there is a split, and it
        // is the hint most worth showing then — the key is easy to forget and
        // there is nothing on screen otherwise to suggest it.
        if app.shell.active_pane_count() > 1 {
            v.push(("S-F1/S-F2", "prev/next pane"));
        }
        v.extend([
            ("F9", "new tab"),
            // Named per key rather than as a pair. "S-F8/F9" read as
            // "Shift+F8 or F9" — with plain F9 (new tab) sitting right beside
            // it — and gave no clue which key gave which orientation.
            ("S-F8", "v-split"),
            ("S-F9", "h-split"),
            ("S-F10", "close"),
            ("F12", "zoom"),
            ("?", "help"),
        ]);
        return v;
    }
    match app.mode {
        Mode::Visual => vec![
            ("j/k", "extend"),
            ("a", "all"),
            ("gg/G", "top/bottom"),
            ("Enter", "confirm"),
            ("Esc", "cancel"),
        ],
        Mode::Filter => vec![
            ("type", "narrow"),
            ("Enter", "keep"),
            ("Esc", "clear"),
        ],
        Mode::Command => vec![("Enter", "run"), ("Esc", "cancel")],
        // Ordered by how often each is reached for: a narrow window drops
        // from the end, and `? help` is reserved separately. Kept short on
        // purpose — a bar listing everything becomes wallpaper, and the
        // manual is one keystroke away.
        _ => vec![
            ("l/-", "in/out"),
            ("Space", "mark"),
            ("y/m", "copy/mv"),
            ("d", "delete"),
            ("/", "filter"),
            (",", "sort"),
            ("S-F", "find"),
            ("C-F", "grep"),
            ("F3", "view"),
            ("S-J", "shell"),
            // Last, so it is the first to drop on a narrow window: comparing
            // two files is the rarest of these by some distance.
            ("=", "diff"),
            ("?", "help"),
        ],
    }
}

fn draw_key_hints(f: &mut Frame, area: Rect, app: &App) {
    let key_style = Style::default()
        .fg(theme().accent)
        .bg(theme().status_bg)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(Color::Rgb(150, 150, 170)).bg(theme().status_bg);
    let gap = Span::styled("   ", desc_style);

    let hints = key_hints(app);
    // +4 for the space between key and label plus the trailing gap.
    let width_of = |(k, d): &(&str, &str)| k.chars().count() as u16 + d.chars().count() as u16 + 4;

    // The last hint is always `? help`. It is the way out of not knowing any
    // of the others, so it must never be the entry that a narrow window drops
    // — reserve its width and truncate the middle instead.
    let (body, tail) = hints.split_at(hints.len().saturating_sub(1));
    let reserved: u16 = tail.iter().map(width_of).sum();

    let mut spans = vec![Span::styled(" ", desc_style)];
    let mut used = 1u16;
    for h in body {
        let w = width_of(h);
        if used + w + reserved > area.width {
            break;
        }
        used += w;
        spans.push(Span::styled(h.0, key_style));
        spans.push(Span::styled(format!(" {}", h.1), desc_style));
        spans.push(gap.clone());
    }
    for h in tail {
        if used + width_of(h) <= area.width {
            spans.push(Span::styled(h.0, key_style));
            spans.push(Span::styled(format!(" {}", h.1), desc_style));
        }
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(theme().status_bg)),
        area,
    );
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let focus_label = match app.focused {
        FocusedPane::Left => "L",
        FocusedPane::Right => "R",
        FocusedPane::Shell => "S",
    };
    let badge_bg = focus_badge_color(app.mode);
    let (item_count, mark_count) = match app.active_pane() {
        Some(p) => (p.entries.len(), p.mark_count()),
        None => (0, 0),
    };
    let dim_sep = Span::styled(
        "  ▏  ",
        Style::default().fg(Color::Rgb(90, 90, 110)).bg(theme().status_bg),
    );
    let pad = Span::styled(" ", Style::default().bg(theme().status_bg));
    let chip = |label: String, fg: Color| {
        Span::styled(
            label,
            Style::default().fg(fg).bg(theme().status_bg).add_modifier(Modifier::BOLD),
        )
    };

    let mut spans: Vec<Span> = vec![
        Span::styled(
            format!(" {} ", focus_label),
            Style::default().fg(Color::Black).bg(badge_bg).add_modifier(Modifier::BOLD),
        ),
        pad.clone(),
        chip(format!("{} items", item_count), Color::White),
        dim_sep.clone(),
        chip(
            format!("marks {}", mark_count),
            if mark_count > 0 { theme().mark_fg } else { Color::Rgb(140, 140, 160) },
        ),
        dim_sep.clone(),
        chip(
            match app.active_pane() {
                Some(p) => format!("{} {}", p.sort.key.label(), if p.sort.reverse { "▼" } else { "▲" }),
                None => "—".to_string(),
            },
            Color::Rgb(180, 180, 220),
        ),
    ];

    // A narrowed listing must never look like a complete one, so the active
    // filter stays visible after leaving filter mode.
    if let Some(filter) = app.active_pane().map(|p| p.filter.clone()).filter(|f| !f.is_empty()) {
        let total = app.active_pane().map(|p| p.all_entries.len()).unwrap_or(0);
        spans.push(dim_sep.clone());
        spans.push(chip(
            format!("filter /{} ({} of {})", filter, item_count, total),
            Color::Rgb(80, 200, 120),
        ));
    }

    if app.zoomed {
        spans.push(dim_sep.clone());
        spans.push(chip("[zoom]".to_string(), theme().accent));
    }

    if let Some(msg) = app.message.as_ref() {
        if !msg.is_empty() {
            spans.push(dim_sep.clone());
            spans.push(Span::styled(
                format!("◂ {}", msg),
                Style::default()
                    .fg(theme().accent)
                    .bg(theme().status_bg)
                    .add_modifier(Modifier::ITALIC | Modifier::BOLD),
            ));
        }
    }

    let line = Line::from(spans);
    let p = Paragraph::new(line).style(Style::default().bg(theme().status_bg));
    f.render_widget(p, area);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", cut)
    }
}

/// Display width of a string in terminal cells.
///
/// Not `chars().count()`: CJK characters occupy two cells, so a Japanese
/// shortcut name padded by character count pushes everything after it out of
/// alignment and off the right edge.
fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Pad to `w` display cells, accounting for wide characters.
fn pad_to(s: &str, w: usize) -> String {
    let mut out = s.to_string();
    for _ in width(s)..w {
        out.push(' ');
    }
    out
}

/// Shorten from the middle, keeping both ends.
///
/// Paths and URLs carry their meaning at opposite ends — the final directory
/// of one, the host of the other — so cutting either end loses what identifies
/// it. Removing the middle keeps both.
fn truncate_middle(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max <= 3 {
        return truncate(s, max);
    }
    // Budget in display cells from each end, so wide characters cost two.
    let keep = max - 1;
    let (head_budget, tail_budget) = (keep.div_ceil(2), keep / 2);
    let take_from = |it: &mut dyn Iterator<Item = char>, budget: usize| -> String {
        let (mut out, mut used) = (String::new(), 0usize);
        for c in it {
            let cw = UnicodeWidthStr::width(c.to_string().as_str());
            if used + cw > budget {
                break;
            }
            used += cw;
            out.push(c);
        }
        out
    };
    let h = take_from(&mut s.chars(), head_budget);
    let t: String = take_from(&mut s.chars().rev(), tail_budget).chars().rev().collect();
    format!("{}…{}", h, t)
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// A progress bar for the running file operation, and the way to stop it.
fn draw_op_progress(f: &mut Frame, area: Rect, app: &App) {
    let Some(job) = &app.op_job else { return };
    let p = &job.latest;

    let w = 74u16.min(area.width.saturating_sub(2));
    let rect = centered_rect(w, 8, area);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .title(format!(" {} ", job.label));
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);

    // Which entry, shortened from the middle so the directory and the filename
    // both stay legible.
    f.render_widget(
        Paragraph::new(truncate_middle(&p.current, inner.width as usize))
            .style(Style::default().fg(Color::Rgb(190, 190, 210))),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    let frac = p.fraction().clamp(0.0, 1.0);
    let bar_y = inner.y + 2;
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(50, 50, 66))),
        Rect::new(inner.x, bar_y, inner.width, 1),
    );
    let filled = ((inner.width as f32) * frac).round() as u16;
    if filled > 0 {
        f.render_widget(
            Block::default().style(Style::default().bg(theme().accent)),
            Rect::new(inner.x, bar_y, filled.min(inner.width), 1),
        );
    }

    let counts = if p.bytes_total > 0 {
        format!(
            "{} / {}   ({} of {} files)",
            cian_core::human_size(p.bytes_done),
            cian_core::human_size(p.bytes_total),
            p.files_done,
            p.files_total
        )
    } else {
        format!("{} of {} files", p.files_done, p.files_total)
    };
    // Elapsed time, so a slow volume looks slow rather than stuck.
    let secs = job.started.elapsed().as_secs();
    let elapsed = if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    };
    f.render_widget(
        Paragraph::new(format!("{:>3}%   {}   ·  {}", (frac * 100.0) as u16, counts, elapsed)),
        Rect::new(inner.x, bar_y + 2, inner.width, 1),
    );
    f.render_widget(
        Paragraph::new(" Esc = stop ").style(
            Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
        ),
        Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
    );
}

#[allow(clippy::type_complexity)]
fn draw_popup(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    hosts: &[cian_lua::SshHost],
    find: Option<(&str, &str, Option<cian_core::search::Outcome>, cian_core::search::Mode)>,
    dests: &[(String, PathBuf)],
) {
    // The manual is taller than any terminal, so it renders as a scrolling
    // viewport rather than the fixed block the other popups use.
    if let Popup::Manual { lines, scroll } = popup {
        let height = area.height.saturating_sub(2).max(6);
        let width: u16 = 70u16.min(area.width.saturating_sub(2));
        let rect = centered_rect(width, height, area);
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        let view_h = inner.height.saturating_sub(1) as usize;

        // Clamp so the last page sits flush with the bottom; this also
        // normalises an over-scrolled offset from the key handler.
        let max_scroll = lines.len().saturating_sub(view_h);
        *scroll = (*scroll).min(max_scroll);
        let offset = *scroll;

        f.render_widget(Clear, rect);
        let pos = match (offset * 100).checked_div(max_scroll) {
            Some(pct) => format!(" {}% ", pct),
            // Everything fits; there is nothing to scroll.
            None => " all ".to_string(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
        .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(" manual ")
            .title_bottom(pos);
        f.render_widget(block, rect);

        let body: Vec<Line> = lines
            .iter()
            .skip(offset)
            .take(view_h)
            .map(|l| Line::from(l.clone()))
            .collect();
        let body_area = Rect::new(inner.x, inner.y, inner.width, view_h as u16);
        f.render_widget(Paragraph::new(body), body_area);

        let footer_area =
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
        let footer = Paragraph::new(" j/k scroll  u/d page  g/G top/bottom  Esc close ").style(
            Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
        );
        f.render_widget(footer, footer_area);
        return;
    }
    // The context menu is anchored at the pointer rather than centred, so it
    // sizes and positions itself.
    if let Popup::ContextMenu { items, cursor, at } = popup {
        let w = items.iter().map(|i| i.label().len()).max().unwrap_or(10) as u16 + 4;
        let h = items.len() as u16 + 2;
        // Keep the whole menu on screen when clicking near an edge.
        let x = at.0.min(area.width.saturating_sub(w));
        let y = at.1.min(area.height.saturating_sub(h));
        let rect = Rect::new(x, y, w.min(area.width), h.min(area.height));

        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent))
            .style(Style::default().bg(Color::Rgb(24, 24, 34)));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 1 });
        f.render_widget(block, rect);

        let rows: Vec<Line> = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let sel = i == *cursor;
                let style = if sel {
                    Style::default().bg(theme().selected_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(210, 210, 225))
                };
                Line::from(Span::styled(
                    format!("{}{:<w$}", if sel { "▸ " } else { "  " }, item.label(), w = (w - 4) as usize),
                    style,
                ))
            })
            .collect();
        f.render_widget(Paragraph::new(rows), inner);
        return;
    }

    if let Popup::SshHosts { cursor, filter } = popup {
        let needle = filter.to_lowercase();
        let matches: Vec<&cian_lua::SshHost> = hosts
            .iter()
            .filter(|h| {
                needle.is_empty()
                    || h.name.to_lowercase().contains(&needle)
                    || h.host.to_lowercase().contains(&needle)
            })
            .collect();
        let w = 56u16.min(area.width);
        let h = (matches.len() as u16 + 5).min(area.height.saturating_sub(2)).max(6);
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(" ssh — host ");
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let mut lines = vec![Line::from(Span::styled(
            format!("/{}_", filter),
            Style::default().fg(theme().accent).add_modifier(Modifier::BOLD),
        ))];
        if matches.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no match)",
                Style::default().fg(Color::Rgb(150, 150, 170)),
            )));
        }
        for (i, hst) in matches.iter().enumerate() {
            let sel = i == *cursor;
            let style = if sel {
                Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(205, 205, 218))
            };
            let users = if hst.users.len() == 1 {
                hst.users[0].name.clone()
            } else {
                format!("{} users", hst.users.len())
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{}{:<16}", if sel { "▸ " } else { "  " }, hst.name), style),
                Span::styled(
                    format!("{:<22} {}", hst.host, users),
                    Style::default().fg(Color::Rgb(140, 140, 165)),
                ),
            ]));
        }
        let body_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
        f.render_widget(Paragraph::new(lines), body_area);
        let footer_area =
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
        f.render_widget(
            Paragraph::new(" type to filter  ↑↓ select  Enter next  Esc cancel ").style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            footer_area,
        );
        return;
    }

    if let Popup::SshUsers { host, cursor } = popup {
        let Some(hst) = hosts.get(*host) else { return };
        let w = 40u16.min(area.width);
        let h = (hst.users.len() as u16 + 4).min(area.height.saturating_sub(2)).max(6);
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(format!(" ssh — {} ", hst.name));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let lines: Vec<Line> = hst
            .users
            .iter()
            .enumerate()
            .map(|(i, u)| {
                let sel = i == *cursor;
                let style = if sel {
                    Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(205, 205, 218))
                };
                // A key marks logins that will authenticate without typing.
                let mark = if u.has_secret() { "  🔑" } else { "" };
                Line::from(Span::styled(
                    format!("{}{}@{}{}", if sel { "▸ " } else { "  " }, u.name, hst.host, mark),
                    style,
                ))
            })
            .collect();
        let body_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
        f.render_widget(Paragraph::new(lines), body_area);
        let footer_area =
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
        f.render_widget(
            Paragraph::new(" Enter connect   Esc back ").style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            footer_area,
        );
        return;
    }

    if let Popup::FindResults { hits, cursor, scroll } = popup {
        let w = 96u16.min(area.width.saturating_sub(2));
        let h = area.height.saturating_sub(4).max(8);
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let title = match find {
            Some((query, root, done, mode)) => {
                let verb = match mode {
                    cian_core::search::Mode::Name => "find",
                    cian_core::search::Mode::Content => "grep",
                };
                let state = match done {
                    None => "searching…".to_string(),
                    Some(cian_core::search::Outcome::Complete) => format!("{} found", hits.len()),
                    Some(cian_core::search::Outcome::Cancelled) => {
                        format!("{} found (stopped)", hits.len())
                    }
                    Some(cian_core::search::Outcome::Truncated) => {
                        format!("{} found (too many, stopped)", hits.len())
                    }
                };
                format!(" {} \"{}\" in {} — {} ", verb, query, root, state)
            }
            None => " find ".to_string(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(truncate_middle(&title, w.saturating_sub(4) as usize));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = inner.height.saturating_sub(1) as usize;
        // Keep the cursor on screen as results stream in beneath it.
        if *cursor < *scroll {
            *scroll = *cursor;
        } else if body_h > 0 && *cursor >= *scroll + body_h {
            *scroll = *cursor + 1 - body_h;
        }

        if hits.is_empty() {
            f.render_widget(
                Paragraph::new("(nothing yet)").style(Style::default().fg(Color::Rgb(150, 150, 170))),
                Rect::new(inner.x, inner.y, inner.width, 1),
            );
        }
        for (row, (i, hit)) in hits.iter().enumerate().skip(*scroll).take(body_h).enumerate() {
            let sel = i == *cursor;
            let y = inner.y + row as u16;
            let line_area = Rect::new(inner.x, y, inner.width, 1);
            if sel {
                f.render_widget(
                    Block::default().style(Style::default().bg(theme().selected_bg)),
                    line_area,
                );
            }
            let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            // The directory part is context; the name is the answer.
            let rel = hit.rel.display().to_string();
            let (dir, name) = match rel.rfind(std::path::MAIN_SEPARATOR) {
                Some(i) => (rel[..=i].to_string(), rel[i + 1..].to_string()),
                None => (String::new(), rel.clone()),
            };
            let avail = inner.width.saturating_sub(4) as usize;
            let mut spans = vec![Span::styled(if sel { " ▸ " } else { "   " }, base)];
            match &hit.line {
                // A content match: the location is a prefix, the matched text
                // is the answer, so give the text the room and the emphasis.
                Some((n, text)) => {
                    let loc = format!("{}:{}  ", rel, n);
                    let loc_w = width(&loc).min(avail / 2);
                    spans.push(Span::styled(
                        truncate_middle(&loc, loc_w),
                        base.fg(Color::Rgb(135, 135, 160)),
                    ));
                    spans.push(Span::styled(
                        truncate(text, avail.saturating_sub(loc_w)),
                        base.fg(Color::Rgb(225, 225, 240)),
                    ));
                }
                None => {
                    spans.push(Span::styled(
                        truncate_middle(&dir, avail.saturating_sub(width(&name))),
                        base.fg(Color::Rgb(135, 135, 160)),
                    ));
                    spans.push(Span::styled(
                        name.clone(),
                        if hit.is_dir {
                            base.fg(FileKind::Directory.color()).add_modifier(Modifier::BOLD)
                        } else {
                            base.fg(Color::Rgb(225, 225, 240))
                        },
                    ));
                }
            }
            f.render_widget(Paragraph::new(Line::from(spans)), line_area);
        }
        f.render_widget(
            Paragraph::new(" Enter=go  j/k=move  Esc=close ").style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::Shortcuts { entries, cursor } = popup {
        // Wide, because these are paths and URLs; the generic 70-column popup
        // wrapped them across lines, which made the list unreadable.
        let w = 96u16.min(area.width.saturating_sub(2));
        let h = (entries.len() as u16 + 5).max(8).min(area.height.saturating_sub(2));
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(" shortcuts ");
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = inner.height.saturating_sub(1);
        let footer_area =
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);

        if entries.is_empty() {
            let hint = vec![
                Line::from(Span::styled(
                    "(no shortcuts yet)",
                    Style::default().fg(Color::Rgb(150, 150, 170)),
                )),
                Line::from(""),
                Line::from("Press `a` to add one. Targets can be URLs, paths, or apps."),
            ];
            f.render_widget(
                Paragraph::new(hint),
                Rect::new(inner.x, inner.y, inner.width, body_h),
            );
        } else {
            // Name column sized to the longest name, within reason, so the
            // targets line up in a column of their own.
            let name_w = entries
                .iter()
                .map(|s| width(&s.name))
                .max()
                .unwrap_or(8)
                .clamp(8, 24);
            let target_w = (inner.width as usize).saturating_sub(name_w + 8);

            // Keep the selected row visible once the list outgrows the popup.
            let view = body_h as usize;
            let first = cursor.saturating_sub(view.saturating_sub(1));
            for (row, (i, sc)) in entries.iter().enumerate().skip(first).take(view).enumerate() {
                let sel = i == *cursor;
                let y = inner.y + row as u16;
                let line_area = Rect::new(inner.x, y, inner.width, 1);
                if sel {
                    // A full-width bar, not just a marker: which row is active
                    // has to be obvious at a glance.
                    f.render_widget(
                        Block::default().style(Style::default().bg(theme().selected_bg)),
                        line_area,
                    );
                }
                let base = if sel {
                    Style::default().bg(theme().selected_bg)
                } else {
                    Style::default()
                };
                let name_style = if sel {
                    base.fg(theme().accent).add_modifier(Modifier::BOLD)
                } else {
                    base.fg(Color::Rgb(225, 225, 240)).add_modifier(Modifier::BOLD)
                };
                // The target is reference material: same row, quieter, so the
                // name is what the eye lands on.
                let target_style = base.fg(Color::Rgb(140, 140, 165));
                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(if sel { " ▸ " } else { "   " }, name_style),
                        Span::styled(format!("{}  ", shortcut_icon(&sc.target)), base),
                        Span::styled(
                            format!("{}  ", pad_to(&truncate_middle(&sc.name, name_w), name_w)),
                            name_style,
                        ),
                        Span::styled(truncate_middle(&sc.target, target_w), target_style),
                    ])),
                    line_area,
                );
            }
        }
        f.render_widget(
            Paragraph::new(" Enter=open  a=add  d=delete  r=edit  p=copy target  Esc=close ")
                .style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(theme().accent)
                        .add_modifier(Modifier::BOLD),
                ),
            footer_area,
        );
        return;
    }

    if let Popup::History { entries, cursor } = popup {
        // Its own renderer rather than the plain-text popup, so the selected
        // row gets the same highlight bar the shortcuts list has.
        let w = 96u16.min(area.width.saturating_sub(2));
        let h = (entries.len() as u16 + 5).max(6).min(area.height.saturating_sub(2));
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(format!(" history ({}) ", entries.len()));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = inner.height.saturating_sub(1) as usize;
        let first = cursor.saturating_sub(body_h.saturating_sub(1));
        for (row, (i, p)) in entries.iter().enumerate().skip(first).take(body_h).enumerate() {
            let sel = i == *cursor;
            let line_area = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
            if sel {
                f.render_widget(
                    Block::default().style(Style::default().bg(theme().selected_bg)),
                    line_area,
                );
            }
            let base =
                if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            let text_style = if sel {
                base.fg(theme().accent).add_modifier(Modifier::BOLD)
            } else {
                base.fg(Color::Rgb(215, 215, 230))
            };
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(if sel { " ▸ " } else { "   " }, text_style),
                    Span::styled(
                        truncate_middle(&p.display().to_string(), inner.width as usize - 4),
                        text_style,
                    ),
                ])),
                line_area,
            );
        }
        f.render_widget(
            Paragraph::new(" ↑↓/jk select  Enter jump  a add shortcut  Esc cancel ").style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::DestPicker { op, targets, cursor } = popup {
        let rows = dests.len();
        let w = 84u16.min(area.width.saturating_sub(2));
        let h = (rows as u16 + 6).min(area.height.saturating_sub(2));
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let verb = match op {
            PendingOp::Copy => "copy",
            PendingOp::Move => "move",
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(format!(" {} {} item(s) to ", verb, targets.len()));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        for (i, (kind, path)) in dests.iter().enumerate().take(inner.height.saturating_sub(2) as usize) {
            let sel = i == *cursor;
            let y = inner.y + i as u16;
            let line = Rect::new(inner.x, y, inner.width, 1);
            if sel {
                f.render_widget(
                    Block::default().style(Style::default().bg(theme().selected_bg)),
                    line,
                );
            }
            let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(if sel { " ▸ " } else { "   " }, base),
                    Span::styled(
                        format!("{:<11}", kind),
                        base.fg(Color::Rgb(135, 135, 160)),
                    ),
                    Span::styled(
                        truncate_middle(&path.display().to_string(), inner.width as usize - 16),
                        base.fg(Color::Rgb(225, 225, 240)),
                    ),
                ])),
                line,
            );
        }
        f.render_widget(
            Paragraph::new(" Enter=send here   n=type a path   Esc=cancel ").style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::Viewer { title, view, scroll } = popup {
        let w = area.width.saturating_sub(4);
        let h = area.height.saturating_sub(2);
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let kind = match view.kind {
            cian_core::viewer::ViewKind::Text => "text",
            cian_core::viewer::ViewKind::Binary => "binary",
        };
        let size = cian_core::human_size(view.total_bytes);
        let cut = if view.truncated { "  (first 4M shown)" } else { "" };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(format!(" {}  —  {}, {}{} ", title, kind, size, cut));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = inner.height.saturating_sub(1) as usize;
        let max_scroll = view.lines.len().saturating_sub(body_h);
        *scroll = (*scroll).min(max_scroll);

        let numbered = view.kind == cian_core::viewer::ViewKind::Text;
        let gutter = if numbered {
            format!("{}", view.lines.len()).len().max(3) + 1
        } else {
            0
        };
        let rows: Vec<Line> = view
            .lines
            .iter()
            .enumerate()
            .skip(*scroll)
            .take(body_h)
            .map(|(i, l)| {
                let body = truncate(l, inner.width as usize - gutter);
                if numbered {
                    Line::from(vec![
                        Span::styled(
                            format!("{:>w$} ", i + 1, w = gutter - 1),
                            Style::default().fg(Color::Rgb(110, 110, 135)),
                        ),
                        Span::raw(body),
                    ])
                } else {
                    Line::from(Span::styled(body, Style::default().fg(Color::Rgb(200, 200, 215))))
                }
            })
            .collect();
        f.render_widget(
            Paragraph::new(rows),
            Rect::new(inner.x, inner.y, inner.width, body_h as u16),
        );
        let pos = match max_scroll {
            0 => "all".to_string(),
            m => format!("{}%", *scroll * 100 / m),
        };
        f.render_widget(
            Paragraph::new(format!(" j/k scroll  u/d page  g/G ends  Esc close      {} ", pos))
                .style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(theme().accent)
                        .add_modifier(Modifier::BOLD),
                ),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::Diff { left, right, result, folded, fold, scroll } = popup {
        use cian_core::diff::Row;

        let rect = centered_rect(area.width.saturating_sub(2), area.height.saturating_sub(2), area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(format!(
                " {} ↔ {}  —  {} ",
                left,
                right,
                cian_core::diff::summary(result)
            ));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = inner.height.saturating_sub(1) as usize;
        let rows: &[Row] = if *fold { folded } else { &result.rows };
        let max_scroll = rows.len().saturating_sub(body_h);
        *scroll = (*scroll).min(max_scroll);

        // Two equal columns with a marker between them, so the eye can run
        // straight down either file.
        let gutter = 5usize;
        let col = (inner.width as usize).saturating_sub(3 + gutter * 2) / 2;

        let dim = Style::default().fg(Color::Rgb(150, 150, 168));
        let num = Style::default().fg(Color::Rgb(105, 105, 130));
        let del = Style::default().fg(Color::Rgb(255, 140, 145));
        let add = Style::default().fg(Color::Rgb(130, 225, 150));
        let chg = Style::default().fg(Color::Rgb(240, 210, 120));

        let cell = |line: Option<&cian_core::diff::Line>, style: Style| -> Vec<Span<'static>> {
            match line {
                Some(l) => vec![
                    Span::styled(format!("{:>w$} ", l.no, w = gutter - 1), num),
                    Span::styled(pad_to(&truncate(&l.text, col), col), style),
                ],
                // An absent side is left blank rather than filled, so the gap
                // itself shows which file the line is missing from.
                None => vec![Span::raw(" ".repeat(gutter + col))],
            }
        };

        let body: Vec<Line> = rows
            .iter()
            .skip(*scroll)
            .take(body_h)
            .map(|r| match r {
                Row::Skipped { lines } => Line::from(Span::styled(
                    format!("{:^w$}", format!("⋯ {} identical lines", lines), w = inner.width as usize),
                    Style::default().fg(Color::Rgb(95, 95, 120)),
                )),
                Row::Same { left: l, right: rr } => {
                    let mut s = cell(Some(l), dim);
                    s.push(Span::styled(" │ ", num));
                    s.extend(cell(Some(rr), dim));
                    Line::from(s)
                }
                Row::Changed { left: l, right: rr } => {
                    let mut s = cell(Some(l), chg);
                    s.push(Span::styled(" ~ ", chg.add_modifier(Modifier::BOLD)));
                    s.extend(cell(Some(rr), chg));
                    Line::from(s)
                }
                Row::Removed { left: l } => {
                    let mut s = cell(Some(l), del);
                    s.push(Span::styled(" - ", del.add_modifier(Modifier::BOLD)));
                    s.extend(cell(None, del));
                    Line::from(s)
                }
                Row::Added { right: rr } => {
                    let mut s = cell(None, add);
                    s.push(Span::styled(" + ", add.add_modifier(Modifier::BOLD)));
                    s.extend(cell(Some(rr), add));
                    Line::from(s)
                }
            })
            .collect();

        // A binary comparison has no rows; say why rather than showing a void.
        let body = if result.binary {
            vec![Line::from(Span::styled(
                if result.identical {
                    "  These are binary files, and they are byte-for-byte the same."
                } else {
                    "  These are binary files, and their contents differ."
                },
                dim,
            ))]
        } else if result.identical {
            vec![Line::from(Span::styled("  The two files are identical.", add))]
        } else {
            body
        };

        f.render_widget(
            Paragraph::new(body),
            Rect::new(inner.x, inner.y, inner.width, body_h as u16),
        );
        let pos = match max_scroll {
            0 => "all".to_string(),
            m => format!("{}%", *scroll * 100 / m),
        };
        f.render_widget(
            Paragraph::new(format!(
                " n/N next/prev change  f {}  j/k scroll  u/d page  Esc close      {} ",
                if *fold { "show all" } else { "fold" },
                pos
            ))
            .style(Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::Archive { path, members, cursor, scroll } = popup {
        let w = 96u16.min(area.width.saturating_sub(2));
        let h = area.height.saturating_sub(4).max(8);
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let total: u64 = members.iter().map(|m| m.size).sum();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(format!(
                " {}  —  {} entries, {} unpacked ",
                name,
                members.len(),
                cian_core::human_size(total)
            ));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = inner.height.saturating_sub(1) as usize;
        if *cursor < *scroll {
            *scroll = *cursor;
        } else if body_h > 0 && *cursor >= *scroll + body_h {
            *scroll = *cursor + 1 - body_h;
        }
        for (row, (i, m)) in members.iter().enumerate().skip(*scroll).take(body_h).enumerate() {
            let sel = i == *cursor;
            let line = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
            if sel {
                f.render_widget(
                    Block::default().style(Style::default().bg(theme().selected_bg)),
                    line,
                );
            }
            let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            let size = if m.is_dir { "—".to_string() } else { cian_core::human_size(m.size) };
            let name_w = inner.width as usize - 14;
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(if sel { " ▸ " } else { "   " }, base),
                    Span::styled(
                        format!("{:<w$}", truncate_middle(&m.name, name_w), w = name_w),
                        if m.is_dir {
                            base.fg(FileKind::Directory.color()).add_modifier(Modifier::BOLD)
                        } else {
                            base.fg(Color::Rgb(225, 225, 240))
                        },
                    ),
                    Span::styled(format!("{:>6}", size), base.fg(Color::Rgb(140, 140, 165))),
                ])),
                line,
            );
        }
        f.render_widget(
            Paragraph::new(" Enter=extract this   a=extract all   Esc=close ").style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::SortPicker { cursor } = popup {
        let w = 34u16.min(area.width);
        let h = SortKey::ALL.len() as u16 + 3;
        let rect = centered_rect(w, h.min(area.height), area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(" sort by ");
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let rows: Vec<Line> = SortKey::ALL
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let sel = i == *cursor;
                let style = if sel {
                    Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(200, 200, 215))
                };
                // The shortcut letter doubles as the mnemonic.
                let hint = match k {
                    SortKey::Name => "n",
                    SortKey::Size => "s",
                    SortKey::Modified => "d",
                    SortKey::Extension => "e",
                };
                Line::from(Span::styled(
                    format!("{}{}  ({})", if sel { "▸ " } else { "  " }, k.label(), hint),
                    style,
                ))
            })
            .collect();
        let body_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
        f.render_widget(Paragraph::new(rows), body_area);
        let footer_area =
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
        f.render_widget(
            Paragraph::new(" Enter=apply (again = reverse)  Esc ").style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            footer_area,
        );
        return;
    }

    if let Popup::ColorPicker { cursor, .. } = popup {
        let w = 26u16.min(area.width);
        let h = PANE_BG_PRESETS.len() as u16 + 3;
        let rect = centered_rect(w, h.min(area.height), area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(" background ");
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let rows: Vec<Line> = PANE_BG_PRESETS
            .iter()
            .enumerate()
            .map(|(i, (name, color))| {
                let sel = i == *cursor;
                // A swatch of the actual color, so the name is not the only cue.
                let swatch = Span::styled(
                    "  ",
                    Style::default().bg(color.unwrap_or(Color::Rgb(16, 16, 20))),
                );
                let label = Span::styled(
                    format!(" {}{}", if sel { "▸ " } else { "  " }, name),
                    if sel {
                        Style::default().add_modifier(Modifier::BOLD).fg(theme().accent)
                    } else {
                        Style::default().fg(Color::Rgb(200, 200, 215))
                    },
                );
                Line::from(vec![swatch, label])
            })
            .collect();
        let body_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
        f.render_widget(Paragraph::new(rows), body_area);
        let footer_area =
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
        f.render_widget(
            Paragraph::new(" Enter=apply  Esc=cancel ").style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            footer_area,
        );
        return;
    }

    let popup: &Popup = popup;
    let (title, body, footer) = match popup {
        Popup::ConfirmDelete { targets } => {
            let title = " delete ".to_string();
            let head = format!("{} item(s) → trash:", targets.len());
            let mut lines = vec![head, String::new()];
            for p in targets.iter().take(8) { lines.push(format!("  {}", p.display())); }
            if targets.len() > 8 { lines.push(format!("  ... and {} more", targets.len() - 8)); }
            (title, lines, " y=trash  a=delete permanently  n/Esc=cancel ".to_string())
        }
        Popup::ConfirmTransfer { op, targets, dest } => {
            let verb = match op { PendingOp::Copy => "copy", PendingOp::Move => "move" };
            let title = format!(" {} ", verb);
            let head = format!("{} item(s) → {}", targets.len(), dest.display());
            let mut lines = vec![head, String::new()];
            for p in targets.iter().take(8) { lines.push(format!("  {}", p.display())); }
            if targets.len() > 8 { lines.push(format!("  ... and {} more", targets.len() - 8)); }
            let foot = if targets.len() == 1 {
                " y/Enter=Yes  a=overwrite  r=rename  n/Esc=cancel "
            } else {
                " y/Enter=Yes(skip)  a=overwrite  n/Esc=cancel "
            };
            (title, lines, foot.to_string())
        }
        Popup::TextInput { title, prompt, buffer, kind, cursor } => {
            // The caret is drawn where editing will happen; a password is shown
            // as dots so it does not sit in plain sight.
            let body = vec![prompt.clone(), field_with_caret(buffer, *cursor, kind.is_secret())];
            (format!(" {} ", title), body, " Enter=ok  ←→ move  Esc=cancel ".to_string())
        }
        Popup::Notice { lines } => {
            (" notice ".to_string(), lines.clone(), " Enter / Esc = close ".to_string())
        }
        Popup::Search { buffer } => {
            (
                " search ".to_string(),
                vec!["find (substring, case-insensitive):".into(), format!("/{}_", buffer)],
                " ↑↓ step matches  Enter=jump  Esc=cancel  (then n/N) ".to_string(),
            )
        }
        Popup::ConfirmQuit => {
            (
                " quit cian? ".to_string(),
                vec!["Are you sure you want to quit?".into()],
                " y / Enter = yes   n / Esc = no ".to_string(),
            )
        }
        Popup::ConfirmClose { target } => {
            let what = match target {
                CloseTarget::ShellPane => "this shell pane",
                CloseTarget::FileTab(_) => "this tab",
            };
            (
                " close? ".to_string(),
                vec![format!("Close {}?", what)],
                " y / Enter = yes   n / Esc = no ".to_string(),
            )
        }
        // All handled above, before this match.
        Popup::Manual { .. }
        | Popup::ContextMenu { .. }
        | Popup::ColorPicker { .. }
        | Popup::SortPicker { .. }
        | Popup::SshHosts { .. }
        | Popup::SshUsers { .. }
        | Popup::Shortcuts { .. }
        | Popup::History { .. }
        | Popup::FindResults { .. }
        | Popup::DestPicker { .. }
        | Popup::Viewer { .. }
        | Popup::Diff { .. }
        | Popup::Archive { .. }
        | Popup::None => return,
    };

    let height = (body.len() as u16 + 4).max(6).min(area.height.saturating_sub(2));
    let width: u16 = 70u16.min(area.width.saturating_sub(2));
    let rect = centered_rect(width, height, area);

    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .title(title);
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);

    let body_text: Vec<Line> = body.into_iter().map(Line::from).collect();
    let body_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
    let footer_area = Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);

    let p = Paragraph::new(body_text).wrap(Wrap { trim: false });
    f.render_widget(p, body_area);

    let footer_p = Paragraph::new(footer).style(
        Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
    );
    f.render_widget(footer_p, footer_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn code(k: KeyCode) -> KeyEvent {
        KeyEvent::new(k, KeyModifiers::NONE)
    }

    /// An app rooted at a temp dir containing `names`.
    fn app_with(names: &[&str]) -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        for n in names {
            std::fs::write(dir.path().join(n), b"").unwrap();
        }
        let p = dir.path().to_path_buf();
        let app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        (dir, app)
    }

    /// Render and hand back the raw buffer, for checking colors.
    fn render_buf(app: &mut App, w: u16, h: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// Render `app` onto a `w`x`h` test terminal and return the text of each row.
    fn render(app: &mut App, w: u16, h: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn slash_filters_the_listing_incrementally() {
        let (_d, mut app) = app_with(&["alpha.rs", "beta.rs", "gamma.txt"]);
        assert_eq!(app.active_pane().unwrap().entries.len(), 3);

        app.handle_key(key('/')).unwrap();
        assert_eq!(app.mode, Mode::Filter);

        app.handle_key(key('r')).unwrap();
        app.handle_key(key('s')).unwrap();
        assert_eq!(app.active_pane().unwrap().entries.len(), 2);

        // Backspace widens the match: "r" still excludes gamma.txt.
        app.handle_key(code(KeyCode::Backspace)).unwrap();
        assert_eq!(app.active_pane().unwrap().entries.len(), 2);

        // Emptying the buffer restores the full listing.
        app.handle_key(code(KeyCode::Backspace)).unwrap();
        assert_eq!(app.filter_buffer, "");
        assert_eq!(app.active_pane().unwrap().entries.len(), 3);
    }

    #[test]
    fn enter_keeps_the_filter_and_esc_clears_it() {
        let (_d, mut app) = app_with(&["a.txt", "b.md"]);

        app.handle_key(key('/')).unwrap();
        app.handle_key(key('m')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.active_pane().unwrap().entries.len(), 1, "filter should survive Enter");

        // Esc in normal mode drops the narrowing.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(app.active_pane().unwrap().entries.len(), 2);
    }

    #[test]
    fn esc_while_filtering_restores_the_full_list() {
        let (_d, mut app) = app_with(&["a.txt", "b.md"]);
        app.handle_key(key('/')).unwrap();
        app.handle_key(key('m')).unwrap();
        assert_eq!(app.active_pane().unwrap().entries.len(), 1);
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.active_pane().unwrap().entries.len(), 2);
    }

    #[test]
    fn question_mark_opens_the_manual() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.handle_key(key('?')).unwrap();
        assert!(matches!(app.popup, Popup::Manual { .. }));
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn ctrl_dot_opens_the_manual() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.handle_key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::CONTROL)).unwrap();
        assert!(matches!(app.popup, Popup::Manual { .. }));
    }

    /// Regression: the manual is ~50 lines, far taller than a normal terminal.
    /// Every line must be reachable by scrolling rather than silently clipped.
    #[test]
    fn manual_scrolls_to_reveal_its_last_section() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.handle_key(key('?')).unwrap();

        let top = render(&mut app, 100, 24).join("\n");
        assert!(top.contains("key manual"), "manual header should be visible");
        assert!(
            !top.contains("zoom active split pane"),
            "the last section cannot already fit on a 24-row terminal"
        );

        // G jumps to the bottom; the final section must now be on screen.
        app.handle_key(key('G')).unwrap();
        let bottom = render(&mut app, 100, 24).join("\n");
        assert!(
            bottom.contains("zoom active split pane"),
            "scrolling to the end must reveal the last section; got:\n{}",
            bottom
        );
    }

    #[test]
    fn manual_scroll_is_clamped_at_both_ends() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.handle_key(key('?')).unwrap();

        // Scrolling up at the top is a no-op, not an underflow panic.
        for _ in 0..5 {
            app.handle_key(key('k')).unwrap();
        }
        let Popup::Manual { scroll, .. } = &app.popup else { panic!("expected manual") };
        assert_eq!(*scroll, 0);

        // Paging past the end settles on the last page after a render.
        for _ in 0..50 {
            app.handle_key(key('d')).unwrap();
        }
        let _ = render(&mut app, 100, 24);
        let Popup::Manual { scroll, lines } = &app.popup else { panic!("expected manual") };
        assert!(*scroll < lines.len(), "scroll must stay inside the document");
    }

    /// The manual reflects `init.lua` overrides rather than a hardcoded list.
    #[test]
    fn manual_lists_user_bound_keys() {
        let mut keymap = HashMap::new();
        keymap.insert('x', Action::Delete);
        let text = manual_lines(&keymap).join("\n");
        assert!(text.contains("d, x"), "user-bound key missing from manual:\n{}", text);
    }

    fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent { kind, column: col, row, modifiers: KeyModifiers::NONE }
    }

    /// Grab a divider, drag it, release. Returns the app for further asserts.
    fn drag_divider(app: &mut App, target: DividerTarget, to: (u16, u16)) {
        let d = app
            .dividers
            .iter()
            .copied()
            .find(|d| d.target == target)
            .unwrap_or_else(|| panic!("no divider for {:?} in {:?}", target, app.dividers));
        let grab = (d.zone.x, d.zone.y);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), grab.0, grab.1));
        assert!(app.drag.is_some(), "grabbing the seam should start a drag");
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), to.0, to.1));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), to.0, to.1));
        assert!(app.drag.is_none(), "releasing should end the drag");
    }

    #[test]
    fn dragging_the_vertical_seam_resizes_the_file_panes() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        assert_eq!(app.panes_pct, 50);

        // Drag the left/right seam to roughly a quarter of the width.
        drag_divider(&mut app, DividerTarget::Panes, (25, 10));
        assert!(
            (20..=30).contains(&app.panes_pct),
            "expected ~25%, got {}",
            app.panes_pct
        );

        // The rendered rects must follow.
        let _ = render(&mut app, 100, 40);
        assert!(
            app.layout_rects.left.width < app.layout_rects.right.width,
            "left pane should now be the narrow one"
        );
    }

    #[test]
    fn dragging_the_horizontal_seam_resizes_the_shell_panel() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        assert_eq!(app.main_pct, 60);

        drag_divider(&mut app, DividerTarget::Main, (50, 10));
        assert!(app.main_pct < 60, "shell should have grown, got {}", app.main_pct);

        let before = app.layout_rects.shell.height;
        let _ = render(&mut app, 100, 40);
        assert!(app.layout_rects.shell.height > before / 2, "shell rect should follow the drag");
    }

    #[test]
    fn a_split_cannot_be_dragged_past_its_minimum() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);

        // Drag far past the left edge; the pane must keep a usable width.
        drag_divider(&mut app, DividerTarget::Panes, (0, 10));
        assert_eq!(app.panes_pct, MIN_SPLIT_PCT);

        drag_divider(&mut app, DividerTarget::Panes, (999, 10));
        assert_eq!(app.panes_pct, 100 - MIN_SPLIT_PCT);
    }

    #[test]
    fn grabbing_a_seam_does_not_change_focus() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);

        let d = app.dividers.iter().copied().find(|d| d.target == DividerTarget::Main).unwrap();
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), d.zone.x, d.zone.y));
        assert_eq!(app.focused, FocusedPane::Left, "grabbing a border must not steal focus");
    }

    #[test]
    fn clicking_inside_a_pane_still_moves_focus() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);

        let r = app.layout_rects.right;
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), r.x + 5, r.y + 3));
        assert_eq!(app.focused, FocusedPane::Right);
        assert!(app.drag.is_none());
    }

    /// An app with two *different* directories, one per pane.
    fn app_two_dirs(
        left: &[&str],
        right: &[&str],
    ) -> (tempfile::TempDir, tempfile::TempDir, App) {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        for n in left {
            std::fs::write(l.path().join(n), b"x").unwrap();
        }
        for n in right {
            std::fs::write(r.path().join(n), b"y").unwrap();
        }
        let app = App::new(
            l.path().to_path_buf(),
            r.path().to_path_buf(),
            cian_lua::Config::default(),
        )
        .unwrap();
        (l, r, app)
    }

    #[test]
    fn copy_then_paste_duplicates_into_the_other_directory() {
        let (_l, r, mut app) = app_two_dirs(&["doc.txt"], &[]);
        app.focus(FocusedPane::Left);
        app.run_menu_item(MenuItem::Copy).unwrap();
        assert!(app.file_clip.is_some());

        app.focus(FocusedPane::Right);
        app.run_menu_item(MenuItem::Paste).unwrap();

        assert!(r.path().join("doc.txt").exists(), "file should have been pasted");
        // A copy stays on the clipboard for pasting again elsewhere.
        assert!(app.file_clip.is_some(), "copy should survive its paste");
    }

    #[test]
    fn cut_then_paste_moves_and_empties_the_clipboard() {
        let (l, r, mut app) = app_two_dirs(&["move_me.txt"], &[]);
        app.focus(FocusedPane::Left);
        app.run_menu_item(MenuItem::Cut).unwrap();

        app.focus(FocusedPane::Right);
        app.run_menu_item(MenuItem::Paste).unwrap();

        assert!(r.path().join("move_me.txt").exists(), "should exist at destination");
        assert!(!l.path().join("move_me.txt").exists(), "should be gone from source");
        assert!(app.file_clip.is_none(), "a cut is consumed by its paste");
    }

    #[test]
    fn pasting_into_the_source_directory_is_refused() {
        let (l, _r, mut app) = app_two_dirs(&["a.txt"], &[]);
        app.focus(FocusedPane::Left);
        app.run_menu_item(MenuItem::Copy).unwrap();
        // Paste straight back where it came from.
        app.run_menu_item(MenuItem::Paste).unwrap();

        let n = std::fs::read_dir(l.path()).unwrap().count();
        assert_eq!(n, 1, "must not duplicate into the same directory");
        assert!(app.message.as_deref().unwrap_or("").contains("already"));
    }

    /// Paste is always offered, because it can also take files from the system
    /// clipboard. Hiding it until cian's own register was filled made a file
    /// just copied in Explorer look unpasteable.
    #[test]
    fn paste_is_always_offered() {
        let (_l, _r, mut app) = app_two_dirs(&["a.txt"], &[]);
        let _ = render(&mut app, 100, 40);

        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::Paste), "offered with nothing held");
        app.popup = Popup::None;

        app.clip_targets(ClipOp::Copy);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::Paste), "and still offered once held");
    }

    /// Plain text on the clipboard must never be treated as a path: the
    /// platform queries return the text coerced into one (copying "hello"
    /// yields `/hello` on macOS), and acting on that would be nonsense.
    #[test]
    fn clipboard_candidates_that_do_not_exist_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.txt");
        std::fs::write(&real, b"x").unwrap();

        let kept = keep_existing(vec![
            real.clone(),
            PathBuf::from("/just some copied text"),
            dir.path().to_path_buf(),
            PathBuf::from(""),
        ]);
        assert_eq!(kept, vec![real, dir.path().to_path_buf()], "only real entries survive");
        assert!(keep_existing(Vec::new()).is_empty());
    }

    #[test]
    fn right_click_focuses_the_pane_and_opens_the_menu() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);

        let r = app.layout_rects.right;
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), r.x + 5, r.y + 2));
        assert_eq!(app.focused, FocusedPane::Right, "right-click should move focus");
        assert!(matches!(app.popup, Popup::ContextMenu { .. }));
    }

    #[test]
    fn the_shell_menu_omits_file_operations() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.clip_targets(ClipOp::Copy);
        app.focus(FocusedPane::Shell);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::Paste));
        assert!(!items.contains(&MenuItem::Delete), "delete makes no sense in a PTY");
        assert!(!items.contains(&MenuItem::Rename));
    }

    /// The manual has to be reachable from the menu everywhere — that is the
    /// whole point of putting it there.
    /// Keys never reach the picker while the shell has focus, so the menu is
    /// the only route to SSH from there. It must lead the shell's menu.
    #[test]
    fn the_shell_menu_leads_with_ssh() {
        let (_d, mut app) = app_with_ssh();
        app.focus(FocusedPane::Shell);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert_eq!(items.first(), Some(&MenuItem::Ssh), "got {:?}", items);
    }

    #[test]
    fn the_menu_reaches_the_ssh_picker_from_the_shell() {
        let (_d, mut app) = app_with_ssh();
        app.focus(FocusedPane::Shell);
        app.run_menu_item(MenuItem::Ssh).unwrap();
        assert!(matches!(app.popup, Popup::SshHosts { .. }), "should open the picker");
    }

    /// Both panes offer it, since the picker is useful from either.
    #[test]
    fn the_file_menu_offers_ssh_too() {
        let (_d, mut app) = app_with_ssh();
        app.focus(FocusedPane::Left);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::Ssh), "got {:?}", items);
    }

    #[test]
    fn every_context_menu_offers_the_manual() {
        let (_d, mut app) = app_with(&["a.txt"]);
        for pane in [FocusedPane::Left, FocusedPane::Right, FocusedPane::Shell] {
            app.focus(pane);
            app.open_context_menu(5, 5);
            let Popup::ContextMenu { items, .. } = &app.popup else {
                panic!("no menu for {:?}", pane)
            };
            assert_eq!(
                items.last(),
                Some(&MenuItem::Manual),
                "manual should be the last entry for {:?}",
                pane
            );
            app.popup = Popup::None;
        }
    }

    /// Right-clicking the shell with an empty clipboard used to open nothing
    /// at all; the manual entry means there is always something to show.
    #[test]
    fn the_shell_menu_has_its_own_reduced_set() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        assert!(app.file_clip.is_none());
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert_eq!(
            items,
            &vec![MenuItem::Ssh, MenuItem::Paste, MenuItem::Background, MenuItem::Manual]
        );
    }

    #[test]
    fn choosing_the_manual_from_the_menu_opens_it() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.open_context_menu(5, 5);

        // Walk to the last entry and activate it with the keyboard.
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        let steps = items.len() - 1;
        for _ in 0..steps {
            app.handle_key(key('j')).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        assert!(matches!(app.popup, Popup::Manual { .. }), "expected the manual");
        let screen = render(&mut app, 100, 40).join("\n");
        assert!(screen.contains("key manual"), "manual should be on screen");
    }

    #[test]
    fn the_color_picker_sets_only_the_chosen_pane() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Right);
        app.run_menu_item(MenuItem::Background).unwrap();
        assert!(matches!(app.popup, Popup::ColorPicker { .. }));

        // Move off "default" and apply.
        app.handle_key(key('j')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();

        assert!(app.pane_bg[1].is_some(), "right pane should be tinted");
        assert!(app.pane_bg[0].is_none(), "left pane must be untouched");
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn a_flash_fades_out_and_then_expires() {
        let (_d, mut app) = app_with(&["a.txt"]);
        assert_eq!(app.flash_level(FocusedPane::Left), 0.0);

        app.flash(FocusedPane::Left);
        assert!(app.flash_level(FocusedPane::Left) > 0.9, "should start near full");
        assert_eq!(app.flash_level(FocusedPane::Right), 0.0, "only the named pane lights");
        assert!(app.flash_active());

        // Pretend the flash started long ago.
        app.flash = Some((FocusedPane::Left, Instant::now() - Duration::from_secs(2)));
        assert_eq!(app.flash_level(FocusedPane::Left), 0.0);
        assert!(!app.flash_active());
    }

    #[test]
    fn easing_stays_in_range_and_hits_both_ends() {
        let a = Anim {
            kind: AnimKind::Zoom { from: Rect::new(0, 0, 10, 10), to: Rect::new(0, 0, 20, 20) },
            start: Instant::now(),
            dur: Duration::from_millis(100),
        };
        assert!(a.progress() < 0.2, "should start near zero");
        assert!(!a.done());

        let ended = Anim { start: Instant::now() - Duration::from_secs(1), ..a };
        assert_eq!(ended.progress(), 1.0);
        assert!(ended.done());

        // A zero-length transition is already over.
        let instant = Anim { dur: Duration::ZERO, ..a };
        assert_eq!(instant.progress(), 1.0);
        assert!(instant.done());
    }

    #[test]
    fn lerp_rect_interpolates_between_its_endpoints() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(10, 20, 30, 40);
        assert_eq!(lerp_rect(a, b, 0.0), a);
        assert_eq!(lerp_rect(a, b, 1.0), b);
        let mid = lerp_rect(a, b, 0.5);
        assert_eq!((mid.x, mid.y, mid.width, mid.height), (5, 10, 20, 25));
        // Never collapses to nothing, which would make a widget panic.
        let z = lerp_rect(Rect::new(0, 0, 0, 0), Rect::new(0, 0, 0, 0), 0.5);
        assert!(z.width >= 1 && z.height >= 1);
    }

    #[test]
    fn union_rect_ignores_empty_inputs() {
        let a = Rect::new(0, 0, 10, 5);
        let b = Rect::new(10, 0, 10, 5);
        assert_eq!(union_rect(a, b), Rect::new(0, 0, 20, 5));
        assert_eq!(union_rect(a, Rect::new(0, 0, 0, 0)), a);
        assert_eq!(union_rect(Rect::new(0, 0, 0, 0), b), b);
    }

    /// Both directions must actually travel. The un-zoom used to read the
    /// focused pane's rect out of `layout_rects`, which by then described the
    /// *zoomed* layout — so `from` and `to` were both the full window and the
    /// transition, while running, moved nothing.
    #[test]
    fn zoom_animates_in_both_directions() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        assert!(!app.zoomed);
        let pane = app.layout_rects.left;

        app.toggle_zoom();
        assert!(app.zoomed);
        let Some(Anim { kind: AnimKind::Zoom { from, to }, .. }) = app.anim else {
            panic!("expected a zoom")
        };
        assert_eq!(from, pane, "should grow out of the pane it was in");
        assert!(to.width > from.width && to.height > from.height, "{:?} -> {:?}", from, to);
        app.finish_anim();

        // Rendering while zoomed overwrites layout_rects with the zoomed
        // layout — the exact condition that broke the way back.
        let _ = render(&mut app, 100, 40);

        app.toggle_zoom();
        assert!(!app.zoomed);
        let Some(Anim { kind: AnimKind::Zoom { from, to }, .. }) = app.anim else {
            panic!("expected a zoom back")
        };
        assert_ne!(from, to, "the way back must travel, not sit still");
        assert!(to.width < from.width && to.height < from.height, "{:?} -> {:?}", from, to);
        assert_eq!(to, pane, "should shrink into the pane it came from");
    }

    #[test]
    fn zooming_the_shell_returns_to_the_shell_rect() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Shell);
        let shell = app.layout_rects.shell;

        app.toggle_zoom();
        app.finish_anim();
        let _ = render(&mut app, 100, 40);
        app.toggle_zoom();

        let Some(Anim { kind: AnimKind::Zoom { to, .. }, .. }) = app.anim else {
            panic!("expected a zoom back")
        };
        assert_eq!(to, shell, "each surface returns to its own rect");
    }

    #[test]
    fn animation_can_be_switched_off_by_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        let mut config = cian_lua::Config::default();
        config.options.animation_ms = Some(0);
        let p = dir.path().to_path_buf();
        let mut app = App::new(p.clone(), p, config).unwrap();
        let _ = render(&mut app, 100, 40);

        app.toggle_zoom();
        assert!(app.zoomed, "the zoom itself must still happen");
        assert!(app.anim.is_none(), "but with no transition");
    }

    #[test]
    fn the_ratio_override_only_applies_to_its_own_divider() {
        let ov = AnimOverride {
            ratio: Some((DividerTarget::Panes, 90)),
            freeze_pty: true,
        };
        assert_eq!(ov.ratio_for(DividerTarget::Panes, 50), 90);
        // Other dividers fall through to their stored value.
        assert_eq!(ov.ratio_for(DividerTarget::Main, 60), 60);
        // Stored values are clamped; overrides are not, so a close animation
        // can drive a pane all the way to zero.
        assert_eq!(ov.ratio_for(DividerTarget::Main, 99), 100 - MIN_SPLIT_PCT);
        let zero = AnimOverride { ratio: Some((DividerTarget::Main, 0)), freeze_pty: true };
        assert_eq!(zero.ratio_for(DividerTarget::Main, 50), 0);
    }

    #[test]
    fn a_deferred_close_runs_when_its_transition_lands() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Nothing to close, but the deferral machinery should still fire
        // exactly once and then clear itself.
        app.anim_then = Some(PendingClose::ShellPane);
        app.start_anim(AnimKind::Ratio {
            target: DividerTarget::Main,
            from: 50,
            to: 0,
        });
        assert!(app.anim.is_some());

        app.finish_anim();
        assert!(app.anim.is_none());
        assert!(app.anim_then.is_none(), "deferred work must be consumed");
    }

    #[test]
    fn split_ratio_survives_a_render_round_trip() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.panes_pct = 30;
        let _ = render(&mut app, 100, 40);
        // 30% of a 100-wide window, give or take rounding.
        assert!(
            (28..=32).contains(&app.layout_rects.left.width),
            "got {}",
            app.layout_rects.left.width
        );
    }

    /// Right-clicking a row must select the file actually drawn on that row,
    /// including after the list has scrolled.
    #[test]
    fn right_click_selects_the_row_under_the_pointer_when_scrolled() {
        let names: Vec<String> = (0..60).map(|i| format!("f{:02}.txt", i)).collect();
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let (_d, mut app) = app_with(&refs);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);

        let rect = app.layout_rects.left;
        let view_h = rect.height.saturating_sub(2);

        // Every combination of scroll position and clicked row must agree.
        for cursor in [0usize, 5, 20, 45, 59] {
            for off in 0..view_h.min(8) {
                if let Some(p) = app.active_pane_mut() {
                    p.cursor = cursor;
                }
                let before = render(&mut app, 100, 40);
                let row = rect.y + 1 + off;
                let lo = rect.x as usize;
                let hi = (rect.x + rect.width) as usize;
                let drawn: String =
                    before[row as usize].chars().skip(lo).take(hi - lo).collect();
                app.handle_mouse(mouse(
                    MouseEventKind::Down(MouseButton::Right),
                    rect.x + 3,
                    row,
                ));
                let sel = app.active_pane().unwrap().selected().unwrap().name.clone();
                assert!(
                    drawn.contains(&sel),
                    "cursor {} row-offset {}: screen showed {:?}, selected {:?}",
                    cursor,
                    off,
                    drawn.trim(),
                    sel
                );
                app.popup = Popup::None;
            }
        }
    }

    #[test]
    fn right_click_on_a_single_screenful_selects_correctly() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);
        let rect = app.layout_rects.left;
        // Clicking past the last entry must leave the cursor where it was
        // rather than jumping somewhere arbitrary.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), rect.x + 3, rect.y + 1));
        assert_eq!(app.active_pane().unwrap().cursor, 0);
        app.popup = Popup::None;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), rect.x + 3, rect.y + 2));
        assert_eq!(app.active_pane().unwrap().cursor, 1);
        app.popup = Popup::None;

        // A row inside the pane but past the last entry: stay put.
        let before = app.active_pane().unwrap().cursor;
        let blank = rect.y + rect.height - 3;
        assert!(blank > rect.y + 3, "test needs a pane taller than the listing");
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), rect.x + 3, blank));
        assert_eq!(app.focused, FocusedPane::Left, "still inside the pane");
        assert_eq!(app.active_pane().unwrap().cursor, before, "empty space must not move it");
        app.popup = Popup::None;

        // The pane's own border row is not a list row either.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), rect.x + 3, rect.y));
        assert_eq!(app.active_pane().unwrap().cursor, before, "the border must not move it");
    }

    /// Degenerate geometry must not panic (u16 underflow in seam maths).
    #[test]
    fn rendering_survives_a_tiny_terminal() {
        let (_d, mut app) = app_with(&["a.txt"]);
        for (w, h) in [(1u16, 1u16), (2, 2), (4, 3), (10, 4), (1, 40), (40, 1)] {
            let _ = render(&mut app, w, h);
        }
        // And with a popup open, which does its own rect maths.
        app.open_manual();
        for (w, h) in [(1u16, 1u16), (3, 3), (12, 5)] {
            let _ = render(&mut app, w, h);
        }
    }

    #[test]
    fn the_shell_menu_offers_a_background_color() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(
            items.contains(&MenuItem::Background),
            "the shell pane should be tintable too, got {:?}",
            items
        );
    }

    #[test]
    fn the_color_picker_tints_only_the_active_split_pane() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        app.run_menu_item(MenuItem::Background).unwrap();
        app.handle_key(key('j')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();

        // The file panes keep their own (unset) backgrounds.
        assert!(app.pane_bg[0].is_none() && app.pane_bg[1].is_none());
        // With no shell running there is no pane to color, and nothing panics.
        assert!(app.shell.active_pane_bg().is_none());
    }

    /// A pane's color must stop at that pane. This used to be stored per
    /// panel, so coloring one split painted every split and every tab —
    /// including ones meant to keep the terminal's own background.
    #[test]
    fn a_pane_tint_stops_at_that_pane() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let tint = Color::Rgb(17, 45, 87);
        app.pane_bg[0] = Some(tint);

        let buf = render_buf(&mut app, 100, 40);
        let left = app.layout_rects.left;
        let right = app.layout_rects.right;
        assert!(left.height > 2 && right.height > 2, "need a real layout");

        assert_eq!(
            buf[(left.x + 5, left.y + left.height / 2)].bg,
            tint,
            "the colored pane should be tinted"
        );
        assert_ne!(
            buf[(right.x + 5, right.y + right.height / 2)].bg,
            tint,
            "the tint must not reach the other pane"
        );
    }

    /// Two split panes, each with its own background — the case that was
    /// impossible when the color lived on the panel.
    #[test]
    fn split_panes_hold_separate_backgrounds() {
        let dir = tempfile::tempdir().unwrap();
        let sh = cian_pty::default_shell();
        let mk = || cian_pty::PtySession::new(dir.path(), &sh, 24, 80).unwrap();

        let mut tab = ShellTab::new(mk());
        let first = tab.active;
        tab.split(SplitDir::LeftRight, mk());
        let second = tab.active;
        assert_ne!(first, second, "split should make a second leaf");

        let set = |t: &mut ShellTab, leaf: usize, c: Color| {
            if let Some(Node::Leaf { bg, .. }) = t.nodes.get_mut(leaf).and_then(|n| n.as_mut()) {
                *bg = Some(c);
            }
        };
        let get = |t: &ShellTab, leaf: usize| match t.nodes.get(leaf).and_then(|n| n.as_ref()) {
            Some(Node::Leaf { bg, .. }) => *bg,
            _ => None,
        };

        set(&mut tab, first, Color::Rgb(17, 45, 87));
        assert_eq!(get(&tab, first), Some(Color::Rgb(17, 45, 87)));
        assert_eq!(get(&tab, second), None, "the sibling must stay on the default");

        set(&mut tab, second, Color::Rgb(87, 29, 17));
        assert_eq!(get(&tab, first), Some(Color::Rgb(17, 45, 87)), "unchanged by its sibling");
        assert_eq!(get(&tab, second), Some(Color::Rgb(87, 29, 17)));
    }

    /// Clicking a split must act on the pane under the pointer. Without this,
    /// right-clicking the left half of a split colored the right half —
    /// whichever happened to be active.
    #[test]
    fn clicking_a_split_selects_the_pane_under_the_pointer() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);

        // Two leaves side by side, standing in for a real split.
        let shell = app.layout_rects.shell;
        let half = shell.width / 2;
        app.shell_leaves = vec![
            (0, 7, Rect::new(shell.x, shell.y, half, shell.height)),
            (0, 9, Rect::new(shell.x + half, shell.y, half, shell.height)),
        ];
        app.shell.tabs.push(ShellTab { nodes: Vec::new(), root: 0, active: 9 });

        app.select_shell_leaf_at(shell.x + 2, shell.y + 2);
        assert_eq!(app.shell.tabs[0].active, 7, "should pick the left pane");

        app.select_shell_leaf_at(shell.x + half + 2, shell.y + 2);
        assert_eq!(app.shell.tabs[0].active, 9, "should pick the right pane");

        // A point outside every pane leaves the selection alone.
        app.select_shell_leaf_at(0, 0);
        assert_eq!(app.shell.tabs[0].active, 9);
    }

    #[test]
    fn the_shell_hints_mention_pane_switching_only_when_split() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        // No panes yet: the key would do nothing, so it is not advertised.
        assert!(!key_hints(&app).iter().any(|(k, _)| *k == "S-F1/F2"));
    }

    #[test]
    fn the_palette_is_distinct_enough_to_tell_panes_apart() {
        // The first entry is "no color"; the rest must be visibly different
        // from one another, which an earlier too-subtle set was not.
        let colors: Vec<(u8, u8, u8)> = PANE_BG_PRESETS
            .iter()
            .filter_map(|(_, c)| match c {
                Some(Color::Rgb(r, g, b)) => Some((*r, *g, *b)),
                _ => None,
            })
            .collect();
        assert_eq!(colors.len(), PANE_BG_PRESETS.len() - 1);
        for (i, a) in colors.iter().enumerate() {
            for b in colors.iter().skip(i + 1) {
                let d = (a.0 as i32 - b.0 as i32).abs()
                    + (a.1 as i32 - b.1 as i32).abs()
                    + (a.2 as i32 - b.2 as i32).abs();
                assert!(d >= 60, "{:?} and {:?} are too close to tell apart", a, b);
            }
            // Dark enough that normal foreground text stays readable.
            let lum = 0.299 * a.0 as f32 + 0.587 * a.1 as f32 + 0.114 * a.2 as f32;
            assert!(lum < 90.0, "{:?} is too light for text on top (lum {})", a, lum);
        }
    }

    /// Cells the shell colored for itself must survive the tint, or ls
    /// colors and vim themes would be flattened.
    #[test]
    fn the_tint_leaves_explicitly_colored_cells_alone() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Give a file pane a background so there are non-Reset cells to guard,
        // then tint the whole screen area and check they are preserved.
        let painted = Color::Rgb(40, 0, 0);
        app.pane_bg[0] = Some(painted);
        let tint = Color::Rgb(0, 0, 40);

        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        terminal
            .draw(|f| {
                draw(f, &mut app);
                tint_default_cells(f, f.area(), tint);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();

        let left = app.layout_rects.left;
        let cell = buf[(left.x + 5, left.y + left.height / 2)].bg;
        assert_eq!(cell, painted, "an already-colored cell must not be repainted");

        // And a cell that was Reset did get the tint.
        let right = app.layout_rects.right;
        assert_eq!(buf[(right.x + 5, right.y + right.height / 2)].bg, tint);
    }

    #[test]
    fn comma_opens_the_sort_picker_and_enter_applies_it() {
        let (_d, mut app) = app_with(&["b.rs", "a.rs", "c.md"]);
        app.handle_key(key(',')).unwrap();
        assert!(matches!(app.popup, Popup::SortPicker { .. }));

        // Jump straight to extension with its mnemonic.
        app.handle_key(key('e')).unwrap();
        assert!(matches!(app.popup, Popup::None));
        let p = app.active_pane().unwrap();
        assert_eq!(p.sort.key, SortKey::Extension);
        assert!(!p.sort.reverse);
    }

    /// Picking the key that is already active flips the direction, the way a
    /// column header does.
    #[test]
    fn choosing_the_active_key_again_reverses_it() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt"]);
        app.apply_sort_key(SortKey::Size);
        assert!(!app.active_pane().unwrap().sort.reverse);
        app.apply_sort_key(SortKey::Size);
        assert!(app.active_pane().unwrap().sort.reverse, "second pick should reverse");
        app.apply_sort_key(SortKey::Name);
        assert!(!app.active_pane().unwrap().sort.reverse, "a different key resets direction");
    }

    #[test]
    fn sorting_is_per_pane() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt"]);
        app.focus(FocusedPane::Left);
        app.apply_sort_key(SortKey::Size);
        assert_eq!(app.left.active_ref().sort.key, SortKey::Size);
        assert_eq!(app.right.active_ref().sort.key, SortKey::Name, "other pane untouched");
    }

    #[test]
    fn the_status_bar_shows_the_active_order() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let screen = render(&mut app, 100, 40).join("\n");
        assert!(screen.contains("name ▲"), "ascending indicator missing:\n{}", screen);

        app.apply_sort_key(SortKey::Modified);
        app.apply_sort_key(SortKey::Modified);
        let screen = render(&mut app, 100, 40).join("\n");
        assert!(screen.contains("date ▼"), "descending indicator missing:\n{}", screen);
    }

    #[test]
    fn the_key_hint_bar_is_contextual() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let normal = render(&mut app, 110, 40).join("\n");
        assert!(normal.contains("sort"), "normal hints missing:\n{}", normal);
        assert!(normal.contains("filter"));

        // Visual mode advertises a different, shorter set.
        app.visual_start();
        let visual = render(&mut app, 110, 40).join("\n");
        assert!(visual.contains("extend"), "visual hints missing:\n{}", visual);
        assert!(!visual.contains("rename"), "normal-mode hints should be gone");
    }

    #[test]
    fn the_key_hint_bar_can_be_switched_off() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        let mut config = cian_lua::Config::default();
        config.options.key_hints = Some(false);
        let p = dir.path().to_path_buf();
        let mut app = App::new(p.clone(), p, config).unwrap();

        let screen = render(&mut app, 110, 40).join("\n");
        assert!(!screen.contains("? help"), "hints should be hidden");
        // The row it would have used goes back to the listing.
        assert!(screen.contains("a.txt"));
    }

    /// The bottom rows are claimed one at a time, so a row must only be
    /// consumed by a bar that is actually drawn. Getting that wrong shifts
    /// everything below it down by one and blanks the last line.
    #[test]
    fn the_status_bar_sits_on_the_last_row_in_every_mode() {
        let (_d, mut app) = app_with(&["a.txt"]);

        let normal = render(&mut app, 110, 40);
        assert!(normal[39].contains("items"), "status row: {:?}", normal[39]);
        assert!(normal[38].contains("help"), "hints above it: {:?}", normal[38]);

        // Filter mode adds a prompt row above the hints; the status bar must
        // still be the bottom line.
        app.handle_key(key('/')).unwrap();
        let filtering = render(&mut app, 110, 40);
        assert!(filtering[39].contains("items"), "status row: {:?}", filtering[39]);
        assert!(filtering[37].contains("filter /"), "prompt row: {:?}", filtering[37]);
    }

    /// `? help` is the way out of not knowing any other key, so a narrow
    /// window must drop something else. Adding one hint used to push it off
    /// the end.
    #[test]
    fn the_help_hint_survives_a_narrow_window() {
        let (_d, mut app) = app_with(&["a.txt"]);
        for w in [40u16, 60, 80, 110, 200] {
            let screen = render(&mut app, w, 40).join("\n");
            assert!(screen.contains("? help"), "lost at width {}:\n{}", w, screen);
        }
    }

    /// A short window drops the hints rather than squeezing the listing out.
    #[test]
    fn a_short_window_drops_the_hints() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let tall = render(&mut app, 110, 40).join("\n");
        assert!(tall.contains("? help"));
        let short = render(&mut app, 110, 10).join("\n");
        assert!(!short.contains("? help"), "hints should yield on a short window");
    }

    fn app_with_ssh() -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        let mut config = cian_lua::Config::default();
        config.ssh_hosts = vec![
            cian_lua::SshHost {
                name: "web1".into(),
                host: "10.0.1.11".into(),
                users: vec![cian_lua::SshUser::plain("root"), cian_lua::SshUser::plain("deploy")],
                port: None,
            },
            cian_lua::SshHost {
                name: "db1".into(),
                host: "10.0.2.31".into(),
                users: vec![cian_lua::SshUser {
                    name: "postgres".into(),
                    password: Some("hunter2".into()),
                    password_cmd: None,
                }],
                port: Some(2222),
            },
        ];
        let p = dir.path().to_path_buf();
        let app = App::new(p.clone(), p, config).unwrap();
        (dir, app)
    }

    #[test]
    fn the_ssh_picker_filters_hosts_as_you_type() {
        let (_d, mut app) = app_with_ssh();
        app.start_ssh();
        assert_eq!(app.ssh_matches("").len(), 2);

        app.handle_key(key('d')).unwrap();
        app.handle_key(key('b')).unwrap();
        let Popup::SshHosts { filter, .. } = &app.popup else { panic!("no picker") };
        assert_eq!(filter, "db");
        assert_eq!(app.ssh_matches("db").len(), 1);

        // Backspace widens it again.
        app.handle_key(code(KeyCode::Backspace)).unwrap();
        let Popup::SshHosts { filter, .. } = &app.popup else { panic!("no picker") };
        assert_eq!(filter, "d");
    }

    /// A host with several users needs the second stage; one with a single
    /// user should connect straight away.
    #[test]
    fn a_single_user_host_skips_the_second_stage() {
        let (_d, mut app) = app_with_ssh();
        app.start_ssh();
        app.handle_key(key('d')).unwrap();
        app.handle_key(key('b')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::None), "should have connected already");
        assert!(app.message.as_deref().unwrap_or("").contains("postgres@db1"));
    }

    #[test]
    fn a_multi_user_host_offers_its_users() {
        let (_d, mut app) = app_with_ssh();
        app.start_ssh();
        app.handle_key(key('w')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let Popup::SshUsers { host, .. } = &app.popup else { panic!("expected the user stage") };
        assert_eq!(app.config.ssh_hosts[*host].name, "web1");

        // Esc steps back to the host list rather than closing outright.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::SshHosts { .. }));
    }

    #[test]
    fn connecting_types_the_command_into_the_shell() {
        let (_d, mut app) = app_with_ssh();
        // No shell yet, so the command has to be queued for the spawn.
        assert_eq!(app.shell.count(), 0);
        app.ssh_connect(1, "postgres");
        assert_eq!(app.focused, FocusedPane::Shell, "should hand over to the shell");
        assert_eq!(
            app.pending_shell_input.as_deref(),
            Some("ssh postgres@10.0.2.31 -p 2222\n"),
            "port should be carried through"
        );
    }

    #[test]
    fn the_picker_explains_itself_when_nothing_is_configured() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.start_ssh();
        let Popup::Notice { lines } = &app.popup else { panic!("expected a notice") };
        let text = lines.join("\n");
        assert!(text.contains("cian.ssh"), "should show how to configure it:\n{}", text);
    }

    #[test]
    fn a_password_prompt_is_recognised_only_at_the_end_of_the_screen() {
        assert!(looks_like_password_prompt("root@10.0.2.31's password:"));
        assert!(looks_like_password_prompt("Password:"));
        assert!(looks_like_password_prompt("Enter passphrase for key '/x/id_ed25519':"));
        // Trailing blank lines are ignored.
        assert!(looks_like_password_prompt("Password:\n\n  \n"));
    }

    #[test]
    fn things_that_must_not_be_mistaken_for_a_password_prompt() {
        // The word scrolling past in output is not a prompt.
        assert!(!looks_like_password_prompt("password rotation done\n$ "));
        assert!(!looks_like_password_prompt("Failed password for root\n$ "));
        // A host-key question ends in a colon but must be answered by a human.
        assert!(!looks_like_password_prompt(
            "The authenticity of host 'x' can't be established.\n\
             ED25519 key fingerprint is SHA256:abc.\n\
             Are you sure you want to continue connecting (yes/no)?:"
        ));
        assert!(!looks_like_password_prompt(""));
        assert!(!looks_like_password_prompt("$ "));
    }

    #[test]
    fn connecting_as_a_user_with_a_secret_arms_the_prompt_watcher() {
        let (_d, mut app) = app_with_ssh();
        app.ssh_connect(1, "postgres");
        assert!(app.pending_auth.is_some(), "should be waiting for the prompt");
        // The secret must not appear in anything the user or a log can see.
        let msg = app.message.clone().unwrap_or_default();
        assert!(!msg.contains("hunter2"), "secret leaked into the status message: {}", msg);
        assert!(!format!("{:?}", app.pending_auth).contains("hunter2"), "secret leaked via Debug");
    }

    #[test]
    fn a_user_without_a_secret_does_not_arm_it() {
        let (_d, mut app) = app_with_ssh();
        app.ssh_connect(0, "root");
        assert!(app.pending_auth.is_none(), "key-auth logins must not wait to type anything");
    }

    #[test]
    fn the_watcher_gives_up_after_its_window() {
        let (_d, mut app) = app_with_ssh();
        app.ssh_connect(1, "postgres");
        // Pretend the window has passed with no prompt — a keyed host, say.
        app.pending_auth = Some(PendingAuth {
            secret: "hunter2".into(),
            deadline: Instant::now() - Duration::from_secs(1),
        });
        app.pending_shell_input = None;
        assert!(!app.poll_pending_auth());
        assert!(app.pending_auth.is_none(), "should have expired rather than waiting forever");
    }

    /// The command is queued while the PTY spawns; the password must not be
    /// sent before the command it answers has even been delivered.
    #[test]
    fn nothing_is_sent_while_the_command_is_still_queued() {
        let (_d, mut app) = app_with_ssh();
        app.ssh_connect(1, "postgres");
        assert!(app.pending_shell_input.is_some(), "command should be queued");
        assert!(!app.poll_pending_auth());
        assert!(app.pending_auth.is_some(), "still armed, just not fired");
    }

    #[test]
    fn a_secret_can_come_from_a_command_instead_of_the_file() {
        let u = cian_lua::SshUser {
            name: "deploy".into(),
            password: None,
            password_cmd: Some("printf 'from-store'".into()),
        };
        assert!(u.has_secret());
        assert_eq!(u.secret().as_deref(), Some("from-store"));
    }

    #[test]
    fn z_prompts_for_a_path_seeded_with_the_current_directory() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let here = app.active_pane().unwrap().cwd.clone();
        app.handle_key(key('z')).unwrap();
        let Popup::TextInput { buffer, kind, .. } = &app.popup else { panic!("no prompt") };
        assert!(matches!(kind, InputKind::JumpPath));
        assert_eq!(buffer, &here.display().to_string(), "seeded with where you are");
    }

    #[test]
    fn a_typed_directory_is_entered() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("inner.txt"), b"x").unwrap();
        let p = dir.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        let target = dir.path().join("sub");
        app.finish_jump_path(&target.display().to_string()).unwrap();
        // jump_to canonicalises, so compare on the final component.
        assert_eq!(app.active_pane().unwrap().cwd.file_name().unwrap(), "sub");
        assert_eq!(app.active_pane().unwrap().entries[0].name, "inner.txt");
    }

    /// Naming a file should land the cursor on it, so the pane is left
    /// somewhere useful rather than wherever it happened to be.
    #[test]
    fn a_typed_file_moves_the_cursor_to_it() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(dir.path().join(n), b"x").unwrap();
        }
        let p = dir.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        let target = dir.path().join("c.txt");
        app.finish_jump_path(&target.display().to_string()).unwrap();
        let pane = app.active_pane().unwrap();
        assert_eq!(pane.selected().unwrap().name, "c.txt");
    }

    #[test]
    fn a_path_that_does_not_exist_says_so_and_stays_put() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let before = app.active_pane().unwrap().cwd.clone();
        app.finish_jump_path("/no/such/place/at/all").unwrap();
        assert_eq!(app.active_pane().unwrap().cwd, before, "must not move");
        assert!(app.message.as_deref().unwrap_or("").contains("no such path"));
    }

    /// Paths get typed after copying them out of a shell or an address bar,
    /// which is where these forms come from.
    #[test]
    fn typed_paths_expand_env_vars_tildes_and_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::env::set_var("CIAN_TEST_BASE", dir.path());

        for form in [
            "$CIAN_TEST_BASE/sub",
            "${CIAN_TEST_BASE}/sub",
            "%CIAN_TEST_BASE%/sub",
        ] {
            assert_eq!(expand_path(form), sub, "failed to expand {:?}", form);
        }
        // Surrounding quotes, as pasted from a shell.
        let quoted = format!("\"{}\"", sub.display());
        assert_eq!(expand_path(&quoted), sub);

        // An unset variable is left alone rather than silently becoming empty.
        assert_eq!(expand_path("$CIAN_NOT_SET_ANYWHERE"), PathBuf::from("$CIAN_NOT_SET_ANYWHERE"));
        std::env::remove_var("CIAN_TEST_BASE");
    }

    #[test]
    fn shift_enter_opens_the_context_menu_by_the_cursor() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);
        if let Some(p) = app.active_pane_mut() {
            p.cursor = 2;
        }

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        let Popup::ContextMenu { at, items, .. } = &app.popup else {
            panic!("expected the context menu")
        };
        assert!(items.contains(&MenuItem::Delete), "the file-pane menu");
        let left = app.layout_rects.left;
        assert!(at.0 >= left.x && at.0 < left.x + left.width, "anchored in the pane");
        assert_eq!(at.1, left.y + 1 + 2, "on the cursor's row");
    }

    /// Rounded corners are missing from several stock console fonts, so
    /// Windows font-links only the corners and the frame looks a few pixels
    /// out at each one. Square corners are in every font.
    #[test]
    fn border_corners_fall_back_to_square_where_fonts_lack_the_rounded_ones() {
        // An explicit setting always wins, on every platform.
        assert_eq!(resolve_border_type(Some("plain")), BorderType::Plain);
        assert_eq!(resolve_border_type(Some("square")), BorderType::Plain);
        assert_eq!(resolve_border_type(Some("rounded")), BorderType::Rounded);
        assert_eq!(resolve_border_type(Some("  Rounded  ")), BorderType::Rounded);
        // An unrecognised value falls through to the automatic choice rather
        // than failing; a bad config should not cost you your borders.
        let auto = resolve_border_type(None);
        assert_eq!(resolve_border_type(Some("nonsense")), auto);

        // Unix terminals handle the rounded set.
        #[cfg(not(windows))]
        assert_eq!(auto, BorderType::Rounded);
    }

    #[test]
    fn the_rendered_frame_uses_the_chosen_corner_glyphs() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let screen = render(&mut app, 100, 40).join("\n");
        let (round, square) = (
            screen.contains('\u{256d}'),
            screen.contains('\u{250c}'),
        );
        assert!(round ^ square, "exactly one corner style should be on screen");
        assert_eq!(round, border_type() == BorderType::Rounded);
    }

    /// Names are often Japanese here, and CJK characters take two cells. Using
    /// the character count to pad pushed everything after a Japanese name two
    /// columns right and off the edge.
    #[test]
    fn width_and_padding_count_cells_not_characters() {
        assert_eq!(width("work"), 4);
        assert_eq!(width("社内Wiki"), 8, "two cells per CJK character");
        assert_eq!("社内Wiki".chars().count(), 6, "which is not the character count");

        assert_eq!(width(&pad_to("社内Wiki", 12)), 12);
        assert_eq!(width(&pad_to("work", 12)), 12);
        // Already at or past the target: left alone rather than truncated.
        assert_eq!(pad_to("work", 2), "work");
    }

    /// Paths identify themselves at the end, URLs at the start. Cutting either
    /// end loses what tells them apart, so the middle goes.
    #[test]
    fn middle_truncation_keeps_both_ends() {
        assert_eq!(truncate_middle("short", 20), "short");
        let long = "/var/log/application/deploy/current/output.log";
        let cut = truncate_middle(long, 20);
        assert!(width(&cut) <= 20, "must fit: {:?} is {}", cut, width(&cut));
        assert!(cut.starts_with("/var"), "keeps the head: {:?}", cut);
        assert!(cut.ends_with(".log"), "keeps the tail: {:?}", cut);
        assert!(cut.contains('…'));

        // Wide characters cost two cells here too.
        let jp = truncate_middle("社内ドキュメント一覧ページ", 10);
        assert!(width(&jp) <= 10, "{:?} is {} cells", jp, width(&jp));

        // Degenerate widths must not panic or overrun.
        for w in 0..6 {
            let out = truncate_middle("/some/path/file.txt", w);
            assert!(width(&out) <= w.max(1), "w={} gave {:?}", w, out);
        }
    }

    #[test]
    fn visual_a_selects_the_whole_listing() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt", "d.txt"]);
        app.handle_key(key('v')).unwrap();
        assert_eq!(app.mode, Mode::Visual);
        app.handle_key(key('a')).unwrap();

        assert_eq!(app.visual_anchor, Some(0), "anchored at the top");
        assert_eq!(app.active_pane().unwrap().cursor, 3, "cursor at the bottom");

        // Enter commits the range to marks.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.active_pane().unwrap().mark_count(), 4);
    }

    /// The other route the user asked for: gg, visual, G.
    #[test]
    fn gg_then_visual_then_g_selects_everything() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt", "d.txt"]);
        if let Some(p) = app.active_pane_mut() {
            p.cursor = 2;
        }
        app.handle_key(key('g')).unwrap();
        app.handle_key(key('g')).unwrap();
        assert_eq!(app.active_pane().unwrap().cursor, 0);

        app.handle_key(key('v')).unwrap();
        app.handle_key(key('G')).unwrap();
        assert_eq!(app.active_pane().unwrap().cursor, 3, "G must move in visual mode too");

        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.active_pane().unwrap().mark_count(), 4);
    }

    #[test]
    fn gg_works_inside_visual_mode() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt"]);
        if let Some(p) = app.active_pane_mut() {
            p.cursor = 2;
        }
        app.handle_key(key('v')).unwrap();
        app.handle_key(key('g')).unwrap();
        app.handle_key(key('g')).unwrap();
        assert_eq!(app.active_pane().unwrap().cursor, 0);
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.active_pane().unwrap().mark_count(), 3);
    }

    /// Ctrl+<key> used to fall through to the plain-character arm, so every
    /// Ctrl combination typed its bare letter into the field.
    ///
    /// Checked with a binding that does nothing rather than Ctrl+V: that one
    /// really does paste, and asserting on the result would depend on whatever
    /// happened to be on the machine's clipboard.
    #[test]
    fn unbound_ctrl_keys_do_not_type_their_letter_into_a_text_field() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.start_shortcut_add();
        app.handle_key(key('w')).unwrap();
        for c in ['x', 'a', 'k'] {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)).unwrap();
        }
        let Popup::TextInput { buffer, .. } = &app.popup else { panic!("no field") };
        assert_eq!(buffer, "w", "a Ctrl combination leaked its letter");
    }

    #[test]
    fn ctrl_u_clears_the_field() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.start_shortcut_add();
        for c in "typo".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)).unwrap();
        let Popup::TextInput { buffer, .. } = &app.popup else { panic!("no field") };
        assert!(buffer.is_empty());
    }

    /// A new shortcut is nearly always for the thing under the cursor, so the
    /// target starts filled in rather than blank.
    #[test]
    fn a_new_shortcut_defaults_its_target_to_the_current_entry() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt"]);
        if let Some(p) = app.active_pane_mut() {
            p.cursor = 1;
        }
        let expected = app.active_pane().unwrap().selected().unwrap().path.clone();

        app.start_shortcut_add();
        for c in "mine".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        let Popup::TextInput { buffer, kind, .. } = &app.popup else { panic!("no target step") };
        assert!(matches!(kind, InputKind::ShortcutTarget { .. }));
        assert_eq!(buffer, &expected.display().to_string());
    }

    /// Wait for the search worker to finish, draining as it goes.
    fn drain_find(app: &mut App) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            app.poll_find_job();
            if app.find_job.as_ref().and_then(|j| j.done).is_some() {
                app.poll_find_job();
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("search did not finish");
    }

    fn find_tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("src/deep")).unwrap();
        std::fs::create_dir_all(d.path().join("build")).unwrap();
        std::fs::write(d.path().join("readme.md"), b"").unwrap();
        std::fs::write(d.path().join("src/main.rs"), b"").unwrap();
        std::fs::write(d.path().join("src/deep/main.rs"), b"").unwrap();
        std::fs::write(d.path().join("build/main.o"), b"").unwrap();
        d
    }

    #[test]
    fn shift_f_searches_the_tree_below_the_pane() {
        let d = find_tree();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.handle_key(KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT)).unwrap();
        let Popup::TextInput { kind, .. } = &app.popup else { panic!("no prompt") };
        assert!(matches!(kind, InputKind::FindRecursive));

        for c in "main".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        drain_find(&mut app);

        let Popup::FindResults { hits, .. } = &app.popup else { panic!("no results") };
        assert_eq!(hits.len(), 3, "got {:?}", hits.iter().map(|h| &h.rel).collect::<Vec<_>>());
    }

    /// Choosing a result should leave the pane somewhere useful: in the file's
    /// directory, with the cursor on it.
    #[test]
    fn choosing_a_result_navigates_to_it() {
        let d = find_tree();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.start_find("main.rs", cian_core::search::Mode::Name);
        drain_find(&mut app);
        // Pick the deepest hit, whichever position it landed in.
        let idx = match &app.popup {
            Popup::FindResults { hits, .. } => hits
                .iter()
                .position(|h| h.rel.to_string_lossy().contains("deep"))
                .expect("expected a hit under src/deep"),
            _ => panic!("no results"),
        };
        if let Popup::FindResults { cursor, .. } = &mut app.popup {
            *cursor = idx;
        }
        app.open_find_hit().unwrap();

        assert!(matches!(app.popup, Popup::None), "the popup should close");
        let pane = app.active_pane().unwrap();
        assert_eq!(pane.cwd.file_name().unwrap(), "deep");
        assert_eq!(pane.selected().unwrap().name, "main.rs");
        assert!(app.find_job.is_none(), "the worker should be released");
    }

    #[test]
    fn a_search_with_no_matches_says_so_rather_than_hanging() {
        let d = find_tree();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.start_find("nothing-matches-this", cian_core::search::Mode::Name);
        drain_find(&mut app);
        let Popup::FindResults { hits, .. } = &app.popup else { panic!("no results popup") };
        assert!(hits.is_empty());
        assert_eq!(app.find_job.as_ref().unwrap().done, Some(cian_core::search::Outcome::Complete));
    }

    #[test]
    fn closing_the_results_stops_the_worker() {
        let d = find_tree();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.start_find("main", cian_core::search::Mode::Name);
        assert!(app.find_job.is_some());
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None));
        assert!(app.find_job.is_none(), "Esc must release the search");
    }

    #[test]
    fn ctrl_f_greps_inside_files_and_reports_the_line() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "one\nTODO: fix\nthree\n").unwrap();
        std::fs::write(d.path().join("b.txt"), "nothing\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)).unwrap();
        let Popup::TextInput { kind, .. } = &app.popup else { panic!("no prompt") };
        assert!(matches!(kind, InputKind::GrepRecursive));
        for c in "todo".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        drain_find(&mut app);

        let Popup::FindResults { hits, .. } = &app.popup else { panic!("no results") };
        assert_eq!(hits.len(), 1);
        let (n, text) = hits[0].line.clone().expect("a content hit carries its line");
        assert_eq!(n, 2, "1-based line number");
        assert_eq!(text, "TODO: fix");
    }

    #[test]
    fn the_menu_offers_the_new_entries() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        for want in [MenuItem::HiddenToggle, MenuItem::Attributes, MenuItem::Hash] {
            assert!(items.contains(&want), "{:?} missing from {:?}", want, items);
        }
    }

    #[test]
    fn the_menu_toggles_dotfiles_for_the_focused_pane_only() {
        let (_d, mut app) = app_with(&["a.txt", ".hidden"]);
        app.focus(FocusedPane::Left);
        assert_eq!(app.left.active_ref().entries.len(), 2);

        app.run_menu_item(MenuItem::HiddenToggle).unwrap();
        assert_eq!(app.left.active_ref().entries.len(), 1, "dotfile hidden here");
        assert_eq!(app.right.active_ref().entries.len(), 2, "and not in the other pane");
    }

    /// Dragging from one pane to the other should raise the transfer
    /// confirmation, not act silently.
    #[test]
    fn dragging_between_panes_offers_a_transfer() {
        let (_l, r, mut app) = app_two_dirs(&["doc.txt"], &[]);
        let _ = render(&mut app, 100, 40);
        let (left, right) = (app.layout_rects.left, app.layout_rects.right);

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 1));
        assert!(app.file_drag.is_some(), "pressing on an entry arms a drag");

        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            right.x + 5,
            right.y + 1,
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), right.x + 5, right.y + 1));

        let Popup::ConfirmTransfer { op, targets, dest } = &app.popup else {
            panic!("expected a transfer confirmation, got {:?}", app.popup)
        };
        assert_eq!(*op, PendingOp::Copy, "a plain drag copies");
        assert_eq!(targets.len(), 1);
        assert_eq!(dest.file_name(), r.path().file_name());
        assert!(app.file_drag.is_none(), "the drag is released");
    }

    #[test]
    fn shift_dragging_moves_instead_of_copying() {
        let (_l, _r, mut app) = app_two_dirs(&["doc.txt"], &[]);
        let _ = render(&mut app, 100, 40);
        let (left, right) = (app.layout_rects.left, app.layout_rects.right);

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 1));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), right.x + 5, right.y + 1));
        let mut up = mouse(MouseEventKind::Up(MouseButton::Left), right.x + 5, right.y + 1);
        up.modifiers = KeyModifiers::SHIFT;
        app.handle_mouse(up);

        let Popup::ConfirmTransfer { op, .. } = &app.popup else { panic!("no confirmation") };
        assert_eq!(*op, PendingOp::Move);
    }

    /// Press and release without moving is a click. It must not transfer
    /// anything, or every click would raise a dialog.
    #[test]
    fn a_click_without_movement_is_not_a_drag() {
        let (_l, _r, mut app) = app_two_dirs(&["doc.txt"], &[]);
        let _ = render(&mut app, 100, 40);
        let left = app.layout_rects.left;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 1));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), left.x + 5, left.y + 1));
        assert!(matches!(app.popup, Popup::None), "a click must not start a transfer");
        assert!(app.file_drag.is_none());
    }

    #[test]
    fn dropping_back_on_the_same_pane_does_nothing() {
        let (_l, _r, mut app) = app_two_dirs(&["a.txt", "b.txt"], &[]);
        let _ = render(&mut app, 100, 40);
        let left = app.layout_rects.left;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 1));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), left.x + 5, left.y + 2));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), left.x + 5, left.y + 2));
        assert!(matches!(app.popup, Popup::None));
    }

    /// The nearest thing to dragging a file into a terminal.
    #[test]
    fn dragging_onto_the_shell_types_the_paths() {
        let (_l, _r, mut app) = app_two_dirs(&["doc.txt"], &[]);
        let _ = render(&mut app, 100, 40);
        let (left, shell) = (app.layout_rects.left, app.layout_rects.shell);

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 1));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), shell.x + 5, shell.y + 2));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), shell.x + 5, shell.y + 2));

        assert_eq!(app.focused, FocusedPane::Shell);
        let queued = app.pending_shell_input.clone().unwrap_or_default();
        assert!(queued.contains("doc.txt"), "got {:?}", queued);
        assert!(!queued.ends_with('\n'), "paths are typed, not run");
    }

    #[test]
    fn destinations_are_remembered_most_recent_first_and_deduped() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.remember_dest(Path::new("/tmp/one"));
        app.remember_dest(Path::new("/tmp/two"));
        app.remember_dest(Path::new("/tmp/one"));
        assert_eq!(
            app.dest_history,
            vec![PathBuf::from("/tmp/one"), PathBuf::from("/tmp/two")],
            "re-using a destination promotes it rather than duplicating it"
        );

        for i in 0..DEST_HISTORY_CAP + 5 {
            app.remember_dest(&PathBuf::from(format!("/tmp/d{}", i)));
        }
        assert_eq!(app.dest_history.len(), DEST_HISTORY_CAP, "the list is capped");
    }

    #[test]
    fn the_destination_picker_leads_with_the_other_pane() {
        let (_l, r, mut app) = app_two_dirs(&["a.txt"], &[]);
        app.remember_dest(Path::new("/tmp/somewhere"));
        app.focus(FocusedPane::Left);
        app.start_dest_picker(PendingOp::Copy);

        assert!(matches!(app.popup, Popup::DestPicker { .. }));
        let choices = app.dest_choices();
        assert_eq!(choices[0].0, "other pane");
        assert_eq!(choices[0].1.file_name(), r.path().file_name());
        assert!(choices.iter().any(|(k, p)| k == "recent" && p == Path::new("/tmp/somewhere")));
    }

    /// Two panes, one file each, both cursors on the first entry.
    fn two_panes_with(
        a: &str,
        b: &str,
    ) -> (tempfile::TempDir, tempfile::TempDir, App) {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(l.path().join("a.txt"), a).unwrap();
        std::fs::write(r.path().join("b.txt"), b).unwrap();
        let app = App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
            .unwrap();
        (l, r, app)
    }

    #[test]
    fn equals_compares_the_two_panes_files() {
        let (_l, _r, mut app) = two_panes_with("one\ntwo\nthree\n", "one\nTWO\nthree\n");
        app.handle_key(code(KeyCode::Char('='))).unwrap();

        let Popup::Diff { left, right, result, .. } = &app.popup else {
            panic!("expected the diff, got {:?}", app.popup)
        };
        assert_eq!((left.as_str(), right.as_str()), ("a.txt", "b.txt"));
        assert_eq!(result.changed, 1);
        assert!(!result.identical);
    }

    /// Which pane holds the focus must not decide which file is the "before".
    #[test]
    fn the_left_pane_is_always_the_left_side() {
        let (_l, _r, mut app) = two_panes_with("old\n", "new\n");
        app.focus(FocusedPane::Right);
        app.handle_key(code(KeyCode::Char('='))).unwrap();

        let Popup::Diff { result, left, .. } = &app.popup else { panic!("no diff") };
        assert_eq!(left, "a.txt");
        match &result.rows[0] {
            cian_core::diff::Row::Changed { left, right } => {
                assert_eq!((left.text.as_str(), right.text.as_str()), ("old", "new"));
            }
            other => panic!("expected a change, got {:?}", other),
        }
    }

    #[test]
    fn comparing_a_directory_says_so_instead_of_opening() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::create_dir(l.path().join("adir")).unwrap();
        std::fs::write(r.path().join("b.txt"), "x").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();

        app.handle_key(code(KeyCode::Char('='))).unwrap();
        assert!(matches!(app.popup, Popup::None));
        assert!(app.message.as_deref().unwrap().contains("directories"));
    }

    #[test]
    fn an_empty_pane_reports_rather_than_opening_an_empty_diff() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(r.path().join("b.txt"), "x").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();

        app.handle_key(code(KeyCode::Char('='))).unwrap();
        assert!(matches!(app.popup, Popup::None));
        assert!(app.message.as_deref().unwrap().contains("select a file"));
    }

    #[test]
    fn n_jumps_to_the_next_difference_and_f_unfolds() {
        // Two differences far enough apart that folding hides the gap.
        let mut a: Vec<String> = (0..40).map(|i| format!("line {}", i)).collect();
        let b = a.clone();
        a[5] = "first change".into();
        a[30] = "second change".into();
        let (_l, _r, mut app) =
            two_panes_with(&(a.join("\n") + "\n"), &(b.join("\n") + "\n"));
        app.handle_key(code(KeyCode::Char('='))).unwrap();

        let Popup::Diff { folded, scroll, fold, .. } = &app.popup else { panic!("no diff") };
        assert!(*fold, "opens folded");
        assert_eq!(*scroll, 0);
        let folded_len = folded.len();

        app.handle_key(code(KeyCode::Char('n'))).unwrap();
        let Popup::Diff { folded, scroll, .. } = &app.popup else { panic!("no diff") };
        assert!(folded[*scroll].is_difference(), "n landed on a change");
        let first = *scroll;

        app.handle_key(code(KeyCode::Char('n'))).unwrap();
        let Popup::Diff { folded, scroll, .. } = &app.popup else { panic!("no diff") };
        assert!(*scroll > first && folded[*scroll].is_difference(), "and on to the next");
        let second = *scroll;

        app.handle_key(code(KeyCode::Char('N'))).unwrap();
        let Popup::Diff { scroll, .. } = &app.popup else { panic!("no diff") };
        assert_eq!(*scroll, first, "N goes back");
        assert!(second > first);

        app.handle_key(code(KeyCode::Char('f'))).unwrap();
        let Popup::Diff { fold, result, scroll, .. } = &app.popup else { panic!("no diff") };
        assert!(!*fold);
        assert_eq!(*scroll, 0, "the row lists differ in length; the old offset is meaningless");
        assert!(result.rows.len() > folded_len, "unfolding shows more");
    }

    #[test]
    fn esc_closes_the_diff() {
        let (_l, _r, mut app) = two_panes_with("a\n", "b\n");
        app.handle_key(code(KeyCode::Char('='))).unwrap();
        assert!(matches!(app.popup, Popup::Diff { .. }));
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn the_diff_renders_without_panicking_at_any_size() {
        let (_l, _r, mut app) = two_panes_with("one\ntwo\n", "one\nTWO\nthree\n");
        app.handle_key(code(KeyCode::Char('='))).unwrap();
        let wide = render(&mut app, 120, 30).join("\n");
        assert!(wide.contains("a.txt ↔ b.txt"), "both names in the title:\n{}", wide);
        assert!(wide.contains("two") && wide.contains("TWO"), "both sides shown:\n{}", wide);
        assert!(wide.contains("three"), "the added line too:\n{}", wide);

        // Narrow enough that the column arithmetic would underflow if it were
        // not saturating.
        for (w, h) in [(80u16, 24u16), (24, 8), (10, 5)] {
            render(&mut app, w, h);
        }
    }

    /// Wait for a background file operation to finish.
    fn drain_op(app: &mut App) {
        for _ in 0..200 {
            if app.op_job.is_none() { break; }
            app.poll_op_job();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Run a `:`-command as if it were typed and Enter pressed.
    fn run_cmd(app: &mut App, line: &str) {
        app.command_buffer = line.to_string();
        app.mode = Mode::Command;
        app.run_command();
    }

    /// A terminal with the kitty keyboard protocol (WezTerm, kitty) reports the
    /// Shift held to type `:`, so the binding must not require Shift to be
    /// absent — otherwise `:` does nothing there and command mode is unreachable.
    #[test]
    fn colon_opens_command_mode_even_with_shift_reported() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.handle_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(app.mode, Mode::Command, "Shift+: must still enter command mode");
        // And it still works without the modifier (a plain-PTY terminal).
        app.mode = Mode::Normal;
        app.handle_key(code(KeyCode::Char(':'))).unwrap();
        assert_eq!(app.mode, Mode::Command);
    }

    /// The other shifted-punctuation bindings, likewise reachable with the
    /// modifier set.
    #[test]
    fn punctuation_bindings_ignore_the_shift_modifier() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt"]);
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(app.mode, Mode::Filter, "/ opens the filter regardless of shift");
        app.handle_key(code(KeyCode::Esc)).unwrap();

        app.handle_key(KeyEvent::new(KeyCode::Char(','), KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::SortPicker { .. }), ", opens the sort picker");
    }

    #[test]
    fn mkdir_makes_a_directory_and_dash_p_makes_the_chain() {
        let (d, mut app) = app_with(&["existing.txt"]);
        run_cmd(&mut app, "mkdir fresh");
        assert!(d.path().join("fresh").is_dir());
        // Plain mkdir into a missing parent fails and says so.
        run_cmd(&mut app, "mkdir a/b/c");
        assert!(!d.path().join("a/b/c").exists());
        assert!(app.message.as_deref().unwrap().to_lowercase().contains("mkdir"));
        // -p builds the whole path.
        run_cmd(&mut app, "mkdir -p a/b/c");
        assert!(d.path().join("a/b/c").is_dir());
        // The new entries show up without an explicit refresh.
        assert!(app.active_pane().unwrap().all_entries.iter().any(|e| e.name == "fresh"));
    }

    #[test]
    fn touch_creates_a_file_that_appears_in_the_listing() {
        let (d, mut app) = app_with(&["a.txt"]);
        run_cmd(&mut app, "touch new.log");
        assert!(d.path().join("new.log").is_file());
        assert!(app.active_pane().unwrap().all_entries.iter().any(|e| e.name == "new.log"));
    }

    #[test]
    fn pwd_reports_and_copies_the_directory() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Compare against the pane's canonicalised cwd, which is what pwd prints.
        let cwd = app.active_pane().unwrap().cwd.display().to_string();
        run_cmd(&mut app, "pwd");
        let msg = app.message.clone().unwrap();
        assert!(msg.contains(&cwd), "msg {:?} should contain {:?}", msg, cwd);
        assert!(msg.contains("copied"));
    }

    #[test]
    fn cp_with_no_argument_targets_the_other_pane() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(l.path().join("doc.txt"), b"hi").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();
        // The pane canonicalises its cwd (differently per platform), so compare
        // against the pane's own path rather than the raw tempdir.
        let right_cwd = app.right.active_ref().cwd.clone();
        run_cmd(&mut app, "cp");
        // Opens the confirm-transfer popup aimed at the right pane.
        match &app.popup {
            Popup::ConfirmTransfer { op, dest, targets } => {
                assert_eq!(*op, PendingOp::Copy);
                assert_eq!(*dest, right_cwd);
                assert_eq!(targets.len(), 1);
            }
            other => panic!("expected a transfer confirm, got {:?}", other),
        }
    }

    #[test]
    fn mv_with_a_path_renames_a_single_file() {
        let (d, mut app) = app_with(&["old.txt", "z.txt"]);
        // Cursor on the first entry (sorted): old.txt.
        app.active_pane_mut().unwrap().cursor = 0;
        let first = app.active_pane().unwrap().selected().unwrap().name.clone();
        run_cmd(&mut app, &format!("mv {}", d.path().join("renamed.txt").display()));
        assert!(d.path().join("renamed.txt").is_file(), "moved to the new name");
        assert!(!d.path().join(&first).exists(), "original is gone");
    }

    #[test]
    fn rm_asks_before_deleting() {
        let (_d, mut app) = app_with(&["a.txt"]);
        run_cmd(&mut app, "rm");
        assert!(matches!(app.popup, Popup::ConfirmDelete { .. }), "rm confirms first");
    }

    #[test]
    fn ls_dash_a_toggles_hidden() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let before = app.active_pane().unwrap().show_hidden;
        run_cmd(&mut app, "ls -a");
        assert_ne!(app.active_pane().unwrap().show_hidden, before);
    }

    #[test]
    fn file_and_wc_open_a_notice() {
        let (d, mut app) = app_with(&["notes.txt"]);
        std::fs::write(d.path().join("notes.txt"), "one two three\nsecond line\n").unwrap();
        app.reload_active();
        app.active_pane_mut().unwrap().cursor = 0;

        run_cmd(&mut app, "file");
        let Popup::Notice { lines } = &app.popup else { panic!("file → notice") };
        assert!(lines.iter().any(|l| l.contains("text")), "{:?}", lines);

        run_cmd(&mut app, "wc");
        let Popup::Notice { lines } = &app.popup else { panic!("wc → notice") };
        // 2 newlines, 5 words.
        assert!(lines.iter().any(|l| l.contains(" 2 ") && l.contains(" 5 ")), "{:?}", lines);
    }

    #[test]
    fn head_and_tail_show_the_right_ends() {
        let (d, mut app) = app_with(&["log.txt"]);
        let text: String = (1..=50).map(|i| format!("line {}\n", i)).collect();
        std::fs::write(d.path().join("log.txt"), text).unwrap();
        app.reload_active();
        app.active_pane_mut().unwrap().cursor = 0;

        run_cmd(&mut app, "head -n 2");
        let Popup::Notice { lines } = &app.popup else { panic!("head → notice") };
        assert!(lines.iter().any(|l| l == "line 1"));
        assert!(!lines.iter().any(|l| l == "line 3"), "only 2 asked for: {:?}", lines);

        run_cmd(&mut app, "tail -n 2");
        let Popup::Notice { lines } = &app.popup else { panic!("tail → notice") };
        assert!(lines.iter().any(|l| l == "line 50"));
        assert!(lines.iter().any(|l| l == "line 49"));
    }

    #[test]
    fn df_reports_free_space() {
        let (_d, mut app) = app_with(&["a.txt"]);
        run_cmd(&mut app, "df -h");
        let Popup::Notice { lines } = &app.popup else { panic!("df → notice") };
        assert!(lines.iter().any(|l| l.starts_with("total")));
        assert!(lines.iter().any(|l| l.starts_with("available")));

        run_cmd(&mut app, "df -z");
        assert!(app.message.as_deref().unwrap().contains("unknown flag"), "bad flag reported");
    }

    #[test]
    fn zip_bundles_the_selection() {
        let (d, mut app) = app_with(&["one.txt", "two.txt"]);
        std::fs::write(d.path().join("one.txt"), b"1").unwrap();
        // Mark both so the whole selection is zipped.
        app.reload_active();
        let paths: Vec<PathBuf> =
            app.active_pane().unwrap().all_entries.iter().map(|e| e.path.clone()).collect();
        for p in paths {
            app.active_pane_mut().unwrap().marks.insert(p);
        }
        run_cmd(&mut app, "zip bundle");
        drain_op(&mut app);
        assert!(d.path().join("bundle.zip").is_file(), "zip created");
        let names: Vec<String> = cian_core::archive::list(&d.path().join("bundle.zip"))
            .unwrap()
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert!(names.contains(&"one.txt".to_string()), "{:?}", names);
    }

    #[test]
    fn zip_dash_e_asks_for_a_password_which_is_masked() {
        let (d, mut app) = app_with(&["secret.txt"]);
        app.active_pane_mut().unwrap().cursor = 0;
        run_cmd(&mut app, "zip -e locked");
        match &app.popup {
            Popup::TextInput { kind, .. } => {
                assert!(kind.is_secret(), "the password field is a secret");
            }
            other => panic!("expected a password prompt, got {:?}", other),
        }
        // The masked field renders as dots, not the typed text.
        app.handle_key(code(KeyCode::Char('p'))).unwrap();
        app.handle_key(code(KeyCode::Char('w'))).unwrap();
        let shown = render(&mut app, 80, 20).join("\n");
        assert!(shown.contains("••"), "password shown masked:\n{}", shown);
        assert!(!shown.contains(">pw"), "the literal password must not appear");
        let _ = d;
    }

    #[test]
    fn bang_runs_in_the_shell_with_substitutions() {
        let (d, mut app) = app_with(&["target file.txt"]);
        app.active_pane_mut().unwrap().cursor = 0;
        run_cmd(&mut app, "!echo %f");
        assert_eq!(app.focused, FocusedPane::Shell, "hands over to the shell");
        // No shell spawned in tests, so the command is queued verbatim.
        let queued = app.pending_shell_input.clone().unwrap_or_default();
        assert!(queued.starts_with("echo "), "got {:?}", queued);
        // The filename has a space, so it must be quoted as one argument.
        assert!(queued.contains("target file.txt"), "the file path is substituted: {:?}", queued);
        assert!(queued.contains('\''), "quoted because of the space: {:?}", queued);
        let _ = d;
    }

    #[test]
    fn an_unknown_command_says_so() {
        let (_d, mut app) = app_with(&["a.txt"]);
        run_cmd(&mut app, "frobnicate");
        assert!(app.message.as_deref().unwrap().contains("unknown command"));
    }

    #[test]
    fn paste_lands_in_the_command_line() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.mode = Mode::Command;
        app.command_buffer = "cd ".into();
        // A bracketed-paste event carrying a path, with a stray newline.
        app.insert_into_active_text("/some/path\n");
        assert_eq!(app.command_buffer, "cd /some/path", "newline stripped, text appended");
    }

    // ---- editing, confirms, search, history refinements ----

    #[test]
    fn the_text_field_edits_at_the_caret_not_only_the_end() {
        let (_d, mut app) = app_with(&["report.txt"]);
        app.active_pane_mut().unwrap().cursor = 0;
        app.handle_key(code(KeyCode::Char('r'))).unwrap(); // rename prompt
        // Seeded with the name, caret at the end.
        {
            let Popup::TextInput { buffer, cursor, .. } = &app.popup else { panic!("no prompt") };
            assert_eq!(buffer, "report.txt");
            assert_eq!(*cursor, "report.txt".chars().count());
        }
        // Move left past ".txt" (4 chars) and insert.
        for _ in 0..4 { app.handle_key(code(KeyCode::Left)).unwrap(); }
        for c in "_v2".chars() { app.handle_key(code(KeyCode::Char(c))).unwrap(); }
        let Popup::TextInput { buffer, .. } = &app.popup else { panic!("no prompt") };
        assert_eq!(buffer, "report_v2.txt", "inserted before the extension");

        // Home, then Delete removes the first char.
        app.handle_key(code(KeyCode::Home)).unwrap();
        app.handle_key(code(KeyCode::Delete)).unwrap();
        let Popup::TextInput { buffer, cursor, .. } = &app.popup else { panic!("no prompt") };
        assert_eq!(buffer, "eport_v2.txt");
        assert_eq!(*cursor, 0);

        // Backspace at the start is a no-op, not a panic.
        app.handle_key(code(KeyCode::Backspace)).unwrap();
        let Popup::TextInput { buffer, .. } = &app.popup else { panic!("no prompt") };
        assert_eq!(buffer, "eport_v2.txt");
    }

    #[test]
    fn caret_editing_handles_multibyte_characters() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.popup = text_input("t", "p", "あい".to_string(), InputKind::JumpPath);
        // Caret at end (2 chars). Left once → between あ and い. Insert 'X'.
        app.handle_key(code(KeyCode::Left)).unwrap();
        app.handle_key(code(KeyCode::Char('X'))).unwrap();
        let Popup::TextInput { buffer, .. } = &app.popup else { panic!("no prompt") };
        assert_eq!(buffer, "あXい", "insert respects char boundaries");
    }

    #[test]
    fn enter_is_yes_on_a_transfer_confirm() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(l.path().join("doc.txt"), b"hi").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();
        run_cmd(&mut app, "cp"); // ConfirmTransfer to the right pane
        app.handle_key(code(KeyCode::Enter)).unwrap();
        drain_op(&mut app);
        assert!(r.path().join("doc.txt").is_file(), "Enter confirmed the copy");
    }

    #[test]
    fn r_on_a_move_confirm_renames_into_the_destination() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(l.path().join("old.txt"), b"data").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();
        app.active_pane_mut().unwrap().cursor = 0;
        app.handle_key(code(KeyCode::Char('m'))).unwrap(); // move confirm
        app.handle_key(code(KeyCode::Char('r'))).unwrap(); // rename & move
        // Seeded with the source name; clear it and type a new one.
        let Popup::TextInput { kind: InputKind::TransferAs { .. }, .. } = &app.popup else {
            panic!("expected the rename prompt, got {:?}", app.popup)
        };
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)).unwrap();
        for c in "new.txt".chars() { app.handle_key(code(KeyCode::Char(c))).unwrap(); }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        assert!(r.path().join("new.txt").is_file(), "moved under the new name");
        assert!(!l.path().join("old.txt").exists(), "and gone from the source");
    }

    #[test]
    fn search_arrows_step_through_the_matches() {
        let (_d, mut app) = app_with(&["a1.txt", "a2.txt", "zzz.txt"]);
        // Sorted: a1, a2, zzz.
        app.handle_key(code(KeyCode::Char('f'))).unwrap(); // search
        app.handle_key(code(KeyCode::Char('a'))).unwrap(); // matches a1, a2
        app.handle_key(code(KeyCode::Down)).unwrap();
        let first = app.active_pane().unwrap().cursor;
        assert!(app.active_pane().unwrap().entries[first].name.contains('a'));
        app.handle_key(code(KeyCode::Down)).unwrap();
        let second = app.active_pane().unwrap().cursor;
        assert_ne!(first, second, "Down moved to the other match");
        assert!(app.active_pane().unwrap().entries[second].name.contains('a'));
    }

    #[test]
    fn history_a_bookmarks_the_selected_path_as_a_shortcut() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Seed some history and open it.
        app.active_pane_mut().unwrap().history =
            vec![PathBuf::from("/tmp/one"), PathBuf::from("/tmp/two")];
        app.handle_key(code(KeyCode::Char('h'))).unwrap();
        assert!(matches!(app.popup, Popup::History { .. }));
        app.handle_key(code(KeyCode::Down)).unwrap(); // select /tmp/two
        app.handle_key(code(KeyCode::Char('a'))).unwrap(); // add shortcut

        // Now on the name step; type a name and continue.
        let Popup::TextInput { kind: InputKind::ShortcutName { .. }, .. } = &app.popup else {
            panic!("expected the shortcut-name prompt, got {:?}", app.popup)
        };
        for c in "mydir".chars() { app.handle_key(code(KeyCode::Char(c))).unwrap(); }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        // The target step must be pre-filled with the chosen history path.
        let Popup::TextInput { buffer, kind: InputKind::ShortcutTarget { .. }, .. } = &app.popup
        else {
            panic!("expected the target step, got {:?}", app.popup)
        };
        assert_eq!(buffer, "/tmp/two", "target seeded from the history selection");
    }

    #[test]
    fn the_history_popup_highlights_the_selection() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.active_pane_mut().unwrap().history =
            vec![PathBuf::from("/tmp/alpha"), PathBuf::from("/tmp/beta")];
        app.handle_key(code(KeyCode::Char('h'))).unwrap();
        let shown = render(&mut app, 100, 20).join("\n");
        assert!(shown.contains("▸"), "the selected row has a marker:\n{}", shown);
        assert!(shown.contains("/tmp/alpha") && shown.contains("/tmp/beta"), "{}", shown);
    }

    /// Right-click Paste in the shell must send text to the terminal, not try
    /// to paste files as it does in a file pane.
    #[test]
    fn shell_paste_sends_text_not_files() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        app.file_clip = None;
        app.run_menu_item(MenuItem::Paste).unwrap();
        // Whatever the clipboard held, this took the shell text path — never
        // the file path, whose messages talk about "files".
        let msg = app.message.clone().unwrap_or_default();
        assert!(!msg.contains("files"), "should not paste files in the shell: {:?}", msg);
    }

    #[test]
    fn f3_views_a_text_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.rs"), "fn main() {}\nsecond\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.handle_key(code(KeyCode::F(3))).unwrap();
        let Popup::Viewer { view, title, .. } = &app.popup else {
            panic!("expected the viewer, got {:?}", app.popup)
        };
        assert_eq!(title, "a.rs");
        assert_eq!(view.kind, cian_core::viewer::ViewKind::Text);
        assert_eq!(view.lines, vec!["fn main() {}", "second"]);
    }

    #[test]
    fn f3_on_an_archive_lists_it_instead() {
        let d = tempfile::tempdir().unwrap();
        {
            use std::io::Write as _;
            let f = std::fs::File::create(d.path().join("a.zip")).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let o: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            w.start_file("inside.txt", o).unwrap();
            w.write_all(b"hi").unwrap();
            w.finish().unwrap();
        }
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.handle_key(code(KeyCode::F(3))).unwrap();
        let Popup::Archive { members, .. } = &app.popup else {
            panic!("expected the archive list, got {:?}", app.popup)
        };
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "inside.txt");
    }

    #[test]
    fn extracting_sends_the_members_to_the_other_pane() {
        let src = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        {
            use std::io::Write as _;
            let f = std::fs::File::create(src.path().join("a.zip")).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let o: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            w.start_file("inside.txt", o).unwrap();
            w.write_all(b"hi").unwrap();
            w.finish().unwrap();
        }
        let mut app = App::new(
            src.path().to_path_buf(),
            out.path().to_path_buf(),
            cian_lua::Config::default(),
        )
        .unwrap();

        app.handle_key(code(KeyCode::F(3))).unwrap();
        app.extract_from_archive(true);
        assert!(app.op_job.is_some(), "extraction runs on the worker");

        let start = Instant::now();
        while app.op_job.is_some() && start.elapsed() < Duration::from_secs(5) {
            app.poll_op_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(std::fs::read_to_string(out.path().join("inside.txt")).unwrap(), "hi");
        // The destination is worth remembering like any other transfer target.
        assert!(app.dest_history.iter().any(|p| p.file_name() == out.path().file_name()));
    }

    #[test]
    fn f3_on_a_directory_says_so_rather_than_opening_a_blank_viewer() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        assert!(matches!(app.popup, Popup::None));
        assert!(app.message.as_deref().unwrap_or("").contains("directory"));
    }

    #[test]
    fn shell_panel_starts_empty_and_focusing_it_does_not_block() {
        let (_d, mut app) = app_with(&["a.txt"]);
        assert_eq!(app.shell.count(), 0);

        // Focusing the shell must return immediately, leaving the spawn in
        // flight rather than blocking the event loop on fork/exec.
        app.focus(FocusedPane::Shell);
        assert!(app.shell.is_starting(), "spawn should be pending, not resolved inline");

        // The placeholder renders without a session present.
        let out = render(&mut app, 100, 24).join("\n");
        assert!(out.contains("starting shell"), "expected placeholder; got:\n{}", out);
    }
}
