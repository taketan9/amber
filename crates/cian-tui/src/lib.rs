use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::Result;
use cian_core::ops::{self, Conflict, DeleteMode, OpReport};
use cian_core::{Pane, Sort, SortKey};
use cian_lua::Config;
use cian_pty::PtySession;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, KeyboardEnhancementFlags, MouseButton, MouseEvent, MouseEventKind,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::{Frame, Terminal};
use serde::{Deserialize, Serialize};
use tui_term::widget::PseudoTerminal;

/// Resolved colour palette. Defaults match the original built-in theme; a
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

/// Parse a user colour spec: `#rrggbb`, `r,g,b`, or a named colour.
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
/// colour specs as human-readable errors (the default is kept for those).
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
    Leaf(PtySession),
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
        Self { nodes: vec![Some(Node::Leaf(session))], root: 0, active: 0 }
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
            Some(Node::Leaf(s)) => Some(s),
            _ => None,
        }
    }
    fn active_pane_mut(&mut self) -> Option<&mut PtySession> {
        match self.nodes.get_mut(self.active).and_then(|n| n.as_mut()) {
            Some(Node::Leaf(s)) => Some(s),
            _ => None,
        }
    }

    fn collect_leaves(&self, i: usize, out: &mut Vec<usize>) {
        match self.nodes.get(i).and_then(|n| n.as_ref()) {
            Some(Node::Leaf(_)) => out.push(i),
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
        if !matches!(self.nodes.get(old).and_then(|n| n.as_ref()), Some(Node::Leaf(_))) {
            return;
        }
        let new_leaf = self.alloc(Node::Leaf(new_session));
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
        if !matches!(self.nodes.get(leaf).and_then(|n| n.as_ref()), Some(Node::Leaf(_))) {
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
            if let Some(Node::Leaf(s)) = n {
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

#[derive(Debug, Clone)]
enum PendingOp {
    Copy,
    Move,
}

#[derive(Debug, Clone)]
enum Popup {
    None,
    ConfirmDelete { targets: Vec<PathBuf> },
    ConfirmTransfer { op: PendingOp, targets: Vec<PathBuf>, dest: PathBuf },
    TextInput { title: String, prompt: String, buffer: String, kind: InputKind },
    Notice { lines: Vec<String> },
    /// The key manual. Unlike `Notice` it is far taller than any terminal, so
    /// it carries a scroll offset (in lines from the top).
    Manual { lines: Vec<String>, scroll: usize },
    /// Right-click menu, anchored near the pointer.
    ContextMenu { items: Vec<MenuItem>, cursor: usize, at: (u16, u16) },
    /// Background-colour picker for the pane that was right-clicked.
    ColorPicker { pane: FocusedPane, cursor: usize },
    /// Sort-order picker for the focused pane.
    SortPicker { cursor: usize },
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
    Delete,
    Rename,
    Background,
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
            MenuItem::Delete => "Delete (to trash)",
            MenuItem::Rename => "Rename",
            MenuItem::Background => "Background colour…",
            MenuItem::Manual => "Key manual  (?)",
        }
    }
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

/// Preset pane backgrounds. Deliberately dark and low-saturation: these sit
/// behind a full pane of text, so anything vivid would hurt legibility.
const PANE_BG_PRESETS: [(&str, Option<Color>); 9] = [
    ("default", None),
    ("slate", Some(Color::Rgb(28, 32, 42))),
    ("ink", Some(Color::Rgb(22, 24, 38))),
    ("forest", Some(Color::Rgb(24, 38, 30))),
    ("moss", Some(Color::Rgb(32, 40, 28))),
    ("wine", Some(Color::Rgb(42, 26, 32))),
    ("rust", Some(Color::Rgb(44, 32, 24))),
    ("plum", Some(Color::Rgb(38, 28, 44))),
    ("steel", Some(Color::Rgb(26, 34, 40))),
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
    /// The border currently being dragged, if any.
    drag: Option<Divider>,
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
    /// Show the contextual key-hint bar.
    show_key_hints: bool,
    /// Per-pane background overrides, indexed by [`Self::bg_slot`].
    /// Session-only: deliberately not persisted.
    pane_bg: [Option<Color>; 3],
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
            drag: None,
            file_clip: None,
            flash: None,
            anim: None,
            anim_then: None,
            anim_dur: Duration::from_millis(
                config.options.animation_ms.unwrap_or(DEFAULT_ANIM_MS),
            ),
            show_key_hints: config.options.key_hints.unwrap_or(true),
            pane_bg: [None, None, None],
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
        match raw.as_str() {
            "" => {}
            "q" | "quit" => self.should_quit = true,
            "shell" => self.focus(FocusedPane::Shell),
            "man" | "help" | "h" => self.open_manual(),
            other => self.message = Some(format!("unknown command: :{}", other)),
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
        self.popup = Popup::TextInput {
            title: "rename".into(),
            prompt: "new name:".into(),
            buffer: e.name.clone(),
            kind: InputKind::Rename { original: e.path.clone() },
        };
    }
    fn start_new_file(&mut self) {
        let Some(p) = self.active_pane() else { return };
        self.popup = Popup::TextInput {
            title: "new file".into(),
            prompt: "name:".into(),
            buffer: String::new(),
            kind: InputKind::NewFile { parent: p.cwd.clone() },
        };
    }
    fn start_new_dir(&mut self) {
        let Some(p) = self.active_pane() else { return };
        self.popup = Popup::TextInput {
            title: "new directory".into(),
            prompt: "name:".into(),
            buffer: String::new(),
            kind: InputKind::NewDir { parent: p.cwd.clone() },
        };
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
        self.popup = Popup::TextInput {
            title: "new shortcut — name".into(),
            prompt: "name:".into(),
            buffer: String::new(),
            kind: InputKind::ShortcutName { editing_index: None },
        };
    }

    fn start_shortcut_edit(&mut self, idx: usize) {
        let Some(s) = self.shortcuts.entries.get(idx).cloned() else { return };
        self.popup = Popup::TextInput {
            title: "edit shortcut — name".into(),
            prompt: "name:".into(),
            buffer: s.name,
            kind: InputKind::ShortcutName { editing_index: Some(idx) },
        };
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

    fn finish_transfer(&mut self, conflict: Conflict) -> Result<()> {
        let popup = std::mem::replace(&mut self.popup, Popup::None);
        let Popup::ConfirmTransfer { op, targets, dest } = popup else { return Ok(()) };
        let report = match op {
            PendingOp::Copy => {
                self.push_clipboard(&targets);
                ops::copy_many(&targets, &dest, conflict)
            }
            PendingOp::Move => {
                self.push_clipboard(&targets);
                ops::move_many(&targets, &dest, conflict)
            }
        };
        if let Some(t) = self.active_file_tabs_mut() { let _ = t.active_mut().reload(); }
        let other_focus = match self.focused {
            FocusedPane::Left => FocusedPane::Right,
            FocusedPane::Right => FocusedPane::Left,
            FocusedPane::Shell => FocusedPane::Left,
        };
        let other = match other_focus {
            FocusedPane::Left => &mut self.left,
            FocusedPane::Right => &mut self.right,
            FocusedPane::Shell => &mut self.left,
        };
        let _ = other.active_mut().reload();
        // The destination is where the files appeared, so light that pane.
        self.flash(other_focus);
        self.show_op_report(&report);
        Ok(())
    }

    fn finish_delete(&mut self, mode: DeleteMode) -> Result<()> {
        let popup = std::mem::replace(&mut self.popup, Popup::None);
        let Popup::ConfirmDelete { targets } = popup else { return Ok(()) };
        if cian_core::log::enabled() {
            cian_core::log::log(&format!("delete {:?}: {} target(s)", mode, targets.len()));
        }
        let report = ops::delete_many(&targets, mode);
        if let Some(t) = self.active_file_tabs_mut() { let _ = t.active_mut().reload(); }
        if let Some(p) = self.active_pane_mut() { p.clear_marks(); }
        self.flash(self.focused);
        self.show_op_report(&report);
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
            InputKind::ShortcutName { editing_index } => {
                // chain into the next step: target input
                let prev_target = editing_index
                    .and_then(|i| self.shortcuts.entries.get(i).map(|s| s.target.clone()))
                    .unwrap_or_default();
                self.popup = Popup::TextInput {
                    title: "shortcut — target".into(),
                    prompt: "URL / path (~ ok) / app:".into(),
                    buffer: prev_target,
                    kind: InputKind::ShortcutTarget {
                        editing_index: *editing_index,
                        name,
                    },
                };
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
                if t != FocusedPane::Shell {
                    self.cursor_to_row(t, row);
                }
                self.open_context_menu(col, row);
            }
            return;
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

        if in_rect(self.layout_rects.left) {
            self.focus(FocusedPane::Left);
        } else if in_rect(self.layout_rects.right) {
            self.focus(FocusedPane::Right);
        } else if in_rect(self.layout_rects.shell) {
            self.focus(FocusedPane::Shell);
        }
    }

    // ------- Transitions -------

    fn anim_enabled(&self) -> bool {
        !self.anim_dur.is_zero()
    }

    /// Toggle full-window zoom of the focused surface, animating between the
    /// surface's pane rect and the whole layout area.
    fn toggle_zoom(&mut self) {
        let pane_rect = match self.focused {
            FocusedPane::Left => self.layout_rects.left,
            FocusedPane::Right => self.layout_rects.right,
            FocusedPane::Shell => self.layout_rects.shell,
        };
        // The full area is the union of everything currently laid out; derived
        // rather than stored so it stays right at any window size.
        let full = union_rect(
            union_rect(self.layout_rects.left, self.layout_rects.right),
            self.layout_rects.shell,
        );
        self.zoomed = !self.zoomed;
        if pane_rect.width > 0 && full.width > 0 {
            let (from, to) = if self.zoomed { (pane_rect, full) } else { (full, pane_rect) };
            self.start_anim(AnimKind::Zoom { from, to });
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
            FocusedPane::Shell => Some(2),
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
            // here — but pasting and appearance still do.
            if self.file_clip.is_some() {
                items.push(MenuItem::Paste);
            }
            items.push(MenuItem::Background);
        } else {
            items.push(MenuItem::Copy);
            items.push(MenuItem::Cut);
            if self.file_clip.is_some() {
                items.push(MenuItem::Paste);
            }
            items.push(MenuItem::CopyToOther);
            items.push(MenuItem::MoveToOther);
            items.push(MenuItem::Rename);
            items.push(MenuItem::Delete);
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
        let Some(clip) = self.file_clip.clone() else {
            self.message = Some("clipboard is empty".into());
            return Ok(());
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
            MenuItem::Paste => return self.paste_clip(),
            MenuItem::CopyToOther => self.start_transfer(PendingOp::Copy),
            MenuItem::MoveToOther => self.start_transfer(PendingOp::Move),
            MenuItem::Rename => self.start_rename(),
            MenuItem::Delete => self.start_delete(),
            MenuItem::Manual => self.open_manual(),
            MenuItem::Background => {
                let pane = self.focused;
                let cur = Self::bg_slot(pane)
                    .and_then(|s| self.pane_bg[s])
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
        if let Popup::TextInput { buffer, .. } = &mut self.popup {
            match key.code {
                KeyCode::Esc => { self.popup = Popup::None; return Ok(()); }
                KeyCode::Enter => { return self.finish_text_input(); }
                KeyCode::Backspace => { buffer.pop(); return Ok(()); }
                KeyCode::Char(c) => { buffer.push(c); return Ok(()); }
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
                    if let Some(slot) = Self::bg_slot(pane) {
                        self.pane_bg[slot] = PANE_BG_PRESETS[idx].1;
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
            KeyCode::Char('y') => match &self.popup {
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
            KeyCode::Enter => {
                if matches!(self.popup, Popup::Notice { .. }) { self.popup = Popup::None; }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => { self.command_buffer.clear(); self.mode = Mode::Normal; }
            KeyCode::Enter => self.run_command(),
            KeyCode::Backspace => { self.command_buffer.pop(); }
            KeyCode::Char(c) => self.command_buffer.push(c),
            _ => {}
        }
        Ok(())
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
        match key.code {
            KeyCode::Esc => self.visual_cancel_and_clear_all(),
            KeyCode::Enter | KeyCode::Char('v') => self.visual_commit(),
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(1); }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(p) = self.active_pane_mut() { p.move_cursor(-1); }
            }
            _ => {}
        }
        Ok(())
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
            (false, false, KeyCode::Char(':')) => {
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
            (false, false, KeyCode::Char('/')) => self.start_filter(),
            (false, false, KeyCode::Char(',')) => self.start_sort_picker(),
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
            // Parent: h was reassigned to history; use -, Backspace, or Left arrow instead.
            (false, false, KeyCode::Char('-'))
            | (_, _, KeyCode::Left)
            | (_, _, KeyCode::Backspace) => {
                if let Some(p) = self.active_pane_mut() { p.go_parent()?; }
            }
            // FIX: l / Right only enters directories; never opens files.
            (false, false, KeyCode::Char('l')) | (_, _, KeyCode::Right) => {
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
fn os_clipboard_file_refs(_paths: &[PathBuf]) -> Result<()> {
    anyhow::bail!("file-reference clipboard not yet implemented on Windows");
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
                entry("l, Right, Enter", Some(EnterDir), "enter folder / open file"),
                entry("-, Left, Bksp", Some(Parent), "parent folder"),
                entry("h", Some(History), "history popup"),
                entry("f", Some(Search), "search"),
                entry("n", Some(SearchNext), "next match"),
                entry("N", Some(SearchPrev), "previous match"),
                entry("/", None, "filter list as you type"),
                entry(",", None, "sort by name / size / date / ext"),
                entry("Enter, Esc", None, "while filtering: keep / clear it"),
            ],
        ),
        (
            "Marks and file operations",
            vec![
                entry("Space", Some(MarkDown), "toggle mark, move down"),
                entry("Shift+Space", Some(MarkUp), "toggle mark, move up"),
                entry("v", Some(Visual), "visual select"),
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
            ],
        ),
        (
            "Panes and tabs",
            vec![
                entry("Shift+H/J/K/L", None, "move focus between panes"),
                entry("drag a border", None, "resize any split (mouse)"),
                entry("right-click", None, "context menu (copy/cut/paste, colour)"),
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
            "Shell panel (focus: click, Shift+J, or :shell)",
            vec![
                entry("F1-F8", None, "switch to shell tab 1-8"),
                entry("F9", None, "new shell tab"),
                entry("F10", None, "close shell tab"),
                entry("Shift+F1/F2", None, "focus next / previous split pane"),
                entry("Shift+F8", None, "split pane left/right"),
                entry("Shift+F9", None, "split pane top/bottom"),
                entry("Shift+F10", None, "close split pane (confirms)"),
                entry("F12", None, "zoom focused surface (toggle)"),
                entry("Shift+F12", None, "zoom active split pane (toggle)"),
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

    // Resolve and install the colour theme before any drawing happens.
    let (resolved, theme_errors) = resolve_theme(&config.theme);
    let _ = THEME.set(resolved);

    // Collect all non-fatal config issues for a single startup notice.
    let mut startup_errors = config.errors.clone();
    startup_errors.extend(theme_errors);
    for (c, name) in &config.keymaps {
        if action_from_name(name).is_none() {
            startup_errors.push(format!("keymap: unknown action {:?} (key '{}')", name, c));
        }
    }

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

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
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
        let tick = if app.anim.is_some() || app.flash.is_some() { 16 } else { 33 };
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
    draw_shell(f, shell_area, &mut app.shell, app.focused == FocusedPane::Shell, &mut dividers, ov, app.pane_bg[2]);
    app.dividers = dividers;
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
            draw_shell(f, rect, &mut app.shell, true, &mut sink, ov, app.pane_bg[2]);
        }
    }
}

/// Zoomed layout: only the focused surface, filling the available area.
fn draw_zoomed(f: &mut Frame, area: Rect, app: &mut App, ov: AnimOverride) {
    let mut rects = LayoutRects::default();
    // Only the shell's internal splits are draggable while zoomed; the
    // main/panes borders are not on screen.
    let mut dividers = Vec::new();
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
            draw_shell(f, area, &mut app.shell, true, &mut dividers, ov, app.pane_bg[2]);
        }
    }
    app.dividers = dividers;
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

    if !matches!(app.popup, Popup::None) {
        draw_popup(f, area, &mut app.popup);
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

/// Broad kinds of file, used to colour the listing.
///
/// Deliberately coarse: the point is that a glance separates "code" from
/// "archive" from "image", not that every extension gets its own hue. Too many
/// colours read as noise rather than structure.
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

/// Classify an entry for colouring. Mirrors the categories [`icon_for`] draws
/// from, so a file's icon and its colour always agree.
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
        .border_type(BorderType::Rounded)
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
        // The icon carries the same colour so the row reads as one unit.
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

    draw_list_scrollbar(f, area, pane.entries.len(), pane.cursor, focused);
}

/// Fixed widths so the columns line up between the two panes.
const SIZE_COL_W: u16 = 5;
const TIME_COL_W: u16 = 16;

/// Draw a scrollbar on a pane's right border when the listing overflows.
fn draw_list_scrollbar(f: &mut Frame, area: Rect, total: usize, cursor: usize, focused: bool) {
    let view_h = area.height.saturating_sub(2);
    if view_h == 0 || total <= view_h as usize {
        return;
    }
    let track = Rect::new(area.x + area.width.saturating_sub(1), area.y + 1, 1, view_h);
    let mut state = ScrollbarState::new(total).position(cursor);
    let style = if focused {
        Style::default().fg(theme().accent)
    } else {
        Style::default().fg(Color::Rgb(90, 90, 110))
    };
    // The bar sits on the pane's right border, so the track keeps drawing the
    // border line and only the thumb thickens. Without this the border looks
    // broken wherever the thumb happens to be.
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_symbol("┃")
            .thumb_style(style)
            .track_symbol(Some("│"))
            .track_style(Style::default().fg(Color::DarkGray))
            .begin_symbol(None)
            .end_symbol(None),
        track,
        &mut state,
    );
}

/// Draw the shell panel, then apply its background tint.
///
/// The tint has to be a post-pass. The PTY widget writes an explicit `Reset`
/// background into every cell the shell left uncoloured, which would clobber
/// any background set on the block underneath. Recolouring only the cells
/// that are still `Reset` tints the panel while leaving alone every colour
/// the shell chose for itself (ls colours, a vim theme, and so on).
#[allow(clippy::too_many_arguments)]
fn draw_shell(
    f: &mut Frame,
    area: Rect,
    shell: &mut ShellPane,
    focused: bool,
    dividers: &mut Vec<Divider>,
    ov: AnimOverride,
    bg: Option<Color>,
) {
    draw_shell_inner(f, area, shell, focused, dividers, ov);
    if let Some(c) = bg {
        tint_default_cells(f, area, c);
    }
}

/// Repaint every still-uncoloured cell in `area` with `bg`.
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
    ov: AnimOverride,
) {
    let border_style = if focused {
        Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
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
            if let Some(Node::Leaf(s)) = tab.nodes.get_mut(leaf).and_then(|n| n.as_mut()) {
                s.resize(inner.height.max(1), inner.width.max(1));
            }
        }
        if let Some(Node::Leaf(s)) = shell.tabs[active].nodes.get(leaf).and_then(|n| n.as_ref()) {
            if let Ok(parser) = s.parser().lock() {
                f.render_widget(PseudoTerminal::new(parser.screen()), inner);
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
    render_node(f, tab, active, root, inner, tab.active, focused, false, dividers, ov);
}

/// Recursively size each leaf's PTY to its rect. `bordered` is true for leaves
/// inside a split (which draw a 1-cell border), false for a lone root leaf.
fn resize_node(tab: &mut ShellTab, tab_idx: usize, i: usize, area: Rect, bordered: bool, ov: AnimOverride) {
    let split = match tab.nodes.get(i).and_then(|n| n.as_ref()) {
        Some(Node::Split { dir, first, second, ratio }) => Some((*dir, *first, *second, *ratio)),
        Some(Node::Leaf(_)) => None,
        None => return,
    };
    match split {
        None => {
            let (h, w) = if bordered {
                (area.height.saturating_sub(2).max(1), area.width.saturating_sub(2).max(1))
            } else {
                (area.height.max(1), area.width.max(1))
            };
            if let Some(Node::Leaf(s)) = tab.nodes[i].as_mut() {
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
    ov: AnimOverride,
) {
    match tab.nodes.get(i).and_then(|n| n.as_ref()) {
        Some(Node::Leaf(session)) => {
            let target = if bordered {
                let is_active = focused && i == active_leaf;
                let bs = if is_active {
                    Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let blk = Block::default().borders(Borders::ALL)
        .border_type(BorderType::Rounded).border_style(bs);
                let pinner = area.inner(Margin { vertical: 1, horizontal: 1 });
                f.render_widget(blk, area);
                pinner
            } else {
                area
            };
            if let Ok(parser) = session.parser().lock() {
                f.render_widget(PseudoTerminal::new(parser.screen()), target);
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
            render_node(f, tab, tab_idx, *first, rects.0, active_leaf, focused, true, dividers, ov);
            render_node(f, tab, tab_idx, *second, rects.1, active_leaf, focused, true, dividers, ov);
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
/// operation flash, which fades a border back to its resting colour.
fn fade(c: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = match c {
        Color::Rgb(r, g, b) => (r, g, b),
        // Named colours have no components to blend; approximate with a light
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
        return vec![
            ("Esc", "files"),
            ("F9", "new tab"),
            ("S-F8/F9", "split"),
            ("S-F10", "close"),
            ("F12", "zoom"),
            ("?", "help"),
        ];
    }
    match app.mode {
        Mode::Visual => vec![
            ("j/k", "extend"),
            ("Enter", "confirm"),
            ("Esc", "cancel"),
        ],
        Mode::Filter => vec![
            ("type", "narrow"),
            ("Enter", "keep"),
            ("Esc", "clear"),
        ],
        Mode::Command => vec![("Enter", "run"), ("Esc", "cancel")],
        _ => vec![
            ("l/-", "in/out"),
            ("Space", "mark"),
            ("y/m", "copy/move"),
            ("d", "delete"),
            ("r", "rename"),
            ("/", "filter"),
            (",", "sort"),
            ("Shift+J", "shell"),
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

    let mut spans = vec![Span::styled(" ", desc_style)];
    let mut used = 1u16;
    for (k, d) in key_hints(app) {
        // +4 for the space between key and label plus the trailing gap.
        let w = k.chars().count() as u16 + d.chars().count() as u16 + 4;
        if used + w > area.width {
            break;
        }
        used += w;
        spans.push(Span::styled(k, key_style));
        spans.push(Span::styled(format!(" {}", d), desc_style));
        spans.push(gap.clone());
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

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

fn draw_popup(f: &mut Frame, area: Rect, popup: &mut Popup) {
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
        .border_type(BorderType::Rounded)
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
            .border_type(BorderType::Rounded)
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

    if let Popup::SortPicker { cursor } = popup {
        let w = 34u16.min(area.width);
        let h = SortKey::ALL.len() as u16 + 3;
        let rect = centered_rect(w, h.min(area.height), area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
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
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(" background ");
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let rows: Vec<Line> = PANE_BG_PRESETS
            .iter()
            .enumerate()
            .map(|(i, (name, color))| {
                let sel = i == *cursor;
                // A swatch of the actual colour, so the name is not the only cue.
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
            (title, lines, " y=Yes(skip on conflict)  a=Yes(overwrite)  n/Esc=cancel ".to_string())
        }
        Popup::TextInput { title, prompt, buffer, .. } => {
            let body = vec![prompt.clone(), format!(">{}_", buffer)];
            (format!(" {} ", title), body, " Enter=ok  Esc=cancel ".to_string())
        }
        Popup::Notice { lines } => {
            (" notice ".to_string(), lines.clone(), " Enter / Esc = close ".to_string())
        }
        Popup::Search { buffer } => {
            (
                " search ".to_string(),
                vec!["find (substring, case-insensitive):".into(), format!("/{}_", buffer)],
                " Enter=jump  Esc=cancel  (then n/N for next/prev) ".to_string(),
            )
        }
        Popup::History { entries, cursor } => {
            let mut lines: Vec<String> =
                vec![format!("recent paths ({} entries):", entries.len()), String::new()];
            for (i, p) in entries.iter().enumerate() {
                let marker = if i == *cursor { "▸ " } else { "  " };
                lines.push(format!("{}{}", marker, p.display()));
            }
            (" history ".to_string(), lines, " ↑↓/jk select  Enter jump  Esc cancel ".to_string())
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
        Popup::Shortcuts { entries, cursor } => {
            let title = " shortcuts ".to_string();
            let mut lines: Vec<String> = if entries.is_empty() {
                vec![
                    "(no shortcuts yet)".to_string(),
                    String::new(),
                    "Press `a` to add your first one.".to_string(),
                    String::new(),
                    "Targets can be URLs (https://...), paths (~/foo),".to_string(),
                    "or apps (e.g. /Applications/Safari.app).".to_string(),
                ]
            } else {
                let mut lines = vec![format!("{} entries:", entries.len()), String::new()];
                for (i, s) in entries.iter().enumerate() {
                    let marker = if i == *cursor { "▸ " } else { "  " };
                    let icon = shortcut_icon(&s.target);
                    lines.push(format!(
                        "{}{}  {:<20} {}",
                        marker,
                        icon,
                        truncate(&s.name, 20),
                        s.target
                    ));
                }
                lines
            };
            lines.push(String::new());
            lines.push(format!("(file: {})", ShortcutStore::default_path().display()));
            (
                title,
                lines,
                " Enter=open  a=add  d=delete  r=edit  p=copy target  Esc=close ".to_string(),
            )
        }
        // All handled above, before this match.
        Popup::Manual { .. }
        | Popup::ContextMenu { .. }
        | Popup::ColorPicker { .. }
        | Popup::SortPicker { .. }
        | Popup::None => return,
    };

    let height = (body.len() as u16 + 4).max(6).min(area.height.saturating_sub(2));
    let width: u16 = 70u16.min(area.width.saturating_sub(2));
    let rect = centered_rect(width, height, area);

    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
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

    /// Render and hand back the raw buffer, for checking colours.
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

    #[test]
    fn paste_only_appears_in_the_menu_once_something_is_held() {
        let (_l, _r, mut app) = app_two_dirs(&["a.txt"], &[]);
        let _ = render(&mut app, 100, 40);

        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(!items.contains(&MenuItem::Paste), "nothing held yet");
        app.popup = Popup::None;

        app.clip_targets(ClipOp::Copy);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::Paste));
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
    fn the_shell_menu_opens_even_with_an_empty_clipboard() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        assert!(app.file_clip.is_none());
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert_eq!(items, &vec![MenuItem::Background, MenuItem::Manual]);
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
    fn the_colour_picker_sets_only_the_chosen_pane() {
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

    #[test]
    fn zoom_toggles_and_animates() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        assert!(!app.zoomed);

        app.toggle_zoom();
        assert!(app.zoomed);
        assert!(app.anim.is_some(), "zoom should start a transition");
        // The overlay must be growing toward something larger than the pane.
        let Some(Anim { kind: AnimKind::Zoom { from, to }, .. }) = app.anim else {
            panic!("expected a zoom")
        };
        assert!(to.width > from.width, "{:?} -> {:?}", from, to);

        app.finish_anim();
        assert!(app.anim.is_none());

        // Zooming back out reverses the direction.
        app.toggle_zoom();
        let Some(Anim { kind: AnimKind::Zoom { from, to }, .. }) = app.anim else {
            panic!("expected a zoom")
        };
        assert!(to.width < from.width);
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
    fn the_shell_menu_offers_a_background_colour() {
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
    fn the_colour_picker_tints_the_shell_slot() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        app.run_menu_item(MenuItem::Background).unwrap();
        app.handle_key(key('j')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();

        assert!(app.pane_bg[2].is_some(), "shell slot should be set");
        assert!(app.pane_bg[0].is_none() && app.pane_bg[1].is_none(), "file panes untouched");
    }

    /// The tint is a post-pass over the rendered cells, so prove it actually
    /// reaches the shell panel and stops at its edge.
    #[test]
    fn the_shell_tint_covers_the_panel_and_nothing_else() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let tint = Color::Rgb(24, 38, 30);
        app.pane_bg[2] = Some(tint);

        let buf = render_buf(&mut app, 100, 40);
        let shell = app.layout_rects.shell;
        let left = app.layout_rects.left;
        assert!(shell.height > 2 && left.height > 2, "need a real layout");

        let mid = buf[(shell.x + 5, shell.y + shell.height / 2)].bg;
        assert_eq!(mid, tint, "shell interior should be tinted");

        let in_files = buf[(left.x + 5, left.y + left.height / 2)].bg;
        assert_ne!(in_files, tint, "the tint must not leak into the file panes");
    }

    /// Cells the shell coloured for itself must survive the tint, or ls
    /// colours and vim themes would be flattened.
    #[test]
    fn the_tint_leaves_explicitly_coloured_cells_alone() {
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
        assert_eq!(cell, painted, "an already-coloured cell must not be repainted");

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

    /// A short window drops the hints rather than squeezing the listing out.
    #[test]
    fn a_short_window_drops_the_hints() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let tall = render(&mut app, 110, 40).join("\n");
        assert!(tall.contains("? help"));
        let short = render(&mut app, 110, 10).join("\n");
        assert!(!short.contains("? help"), "hints should yield on a short window");
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
