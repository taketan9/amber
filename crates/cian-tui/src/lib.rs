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
use ratatui::layout::{Direction, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::BorderType;
use ratatui::{Frame, Terminal};
use serde::{Deserialize, Serialize};

mod util;
use util::{
    centered_rect, glob_match, order_pos, pad_to, truncate, truncate_middle, union_rect,
    viewer_charwise, viewer_find, viewer_match_bracket, viewer_paragraph, viewer_word_back,
    viewer_word_forward, vlen, width, wrap_str,
};

mod ai;
mod render;
use render::{draw, icon_for};
// Exercised only by the test module.
#[cfg(test)]
use render::{key_hints, tint_default_cells};

mod ai_parse;
use ai_parse::{
    clean_ai_command, clean_ai_commit_message, parse_junk_reply, parse_rename_reply,
    parse_sem_search_reply, parse_structure_reply, truncate_diff_for_ai, truncate_text_for_ai,
};
// `clean_dest_folder` / `clean_filename` are only exercised directly by tests;
// the library reaches them through the parse_* functions above.
#[cfg(test)]
use ai_parse::{clean_dest_folder, clean_filename};

/// Resolved color palette. Defaults match the original built-in theme; a
/// `~/.config/cian/init.lua` calling `cian.set_theme{...}` overrides any field.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ResolvedTheme {
    accent: Color,
    status_bg: Color,
    selected_bg: Color,
    visual_bg: Color,
    mark_fg: Color,
    /// The surface behind panes and the shell. `None` leaves the terminal's own
    /// background showing (the dark default's behaviour); a light theme paints
    /// it so the look holds up on any terminal.
    base_bg: Option<Color>,
    /// Quieter greys for secondary text and borders.
    dim: Color,
    border: Color,
    /// Background of menus and dialogs.
    popup_bg: Color,
    /// File-type accents, indexed by [`FileKind`].
    file: FilePalette,
}

/// The eight file-type accents plus the two neutral tones, kept together so a
/// theme swaps them as a set.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FilePalette {
    directory: Color,
    code: Color,
    config: Color,
    document: Color,
    image: Color,
    media: Color,
    archive: Color,
    executable: Color,
    muted: Color,
    plain: Color,
}

impl Default for ResolvedTheme {
    fn default() -> Self {
        Self {
            accent: Color::Cyan, // cian-blue, kept consistent across the app
            status_bg: Color::Rgb(40, 40, 55),
            selected_bg: Color::Rgb(60, 60, 90),
            visual_bg: Color::Rgb(80, 60, 30),
            mark_fg: Color::Yellow,
            base_bg: None,
            dim: Color::Rgb(130, 130, 155),
            border: Color::DarkGray,
            popup_bg: Color::Rgb(24, 24, 34),
            file: FilePalette {
                directory: Color::Rgb(96, 165, 250),
                code: Color::Rgb(250, 204, 21),
                config: Color::Rgb(148, 190, 210),
                document: Color::Rgb(226, 226, 236),
                image: Color::Rgb(216, 130, 220),
                media: Color::Rgb(120, 200, 190),
                archive: Color::Rgb(240, 130, 120),
                executable: Color::Rgb(126, 217, 130),
                muted: Color::Rgb(128, 128, 148),
                plain: Color::Rgb(205, 205, 218),
            },
        }
    }
}

impl ResolvedTheme {
    /// Ethan Schoonover's Solarized Light. The eight accents are unchanged from
    /// the palette (they are tuned to read on the light *and* dark bases); the
    /// neutrals map to base00/base1 for text and base2 for surfaces.
    fn solarized_light() -> Self {
        let base00 = Color::Rgb(0x65, 0x7b, 0x83); // body text
        let base01 = Color::Rgb(0x58, 0x6e, 0x75); // emphasized text
        let base1 = Color::Rgb(0x93, 0xa1, 0xa1); // comments / secondary
        let base2 = Color::Rgb(0xee, 0xe8, 0xd5); // highlighted surface
        let base3 = Color::Rgb(0xfd, 0xf6, 0xe3); // background
        let blue = Color::Rgb(0x26, 0x8b, 0xd2);
        let yellow = Color::Rgb(0xb5, 0x89, 0x00);
        let orange = Color::Rgb(0xcb, 0x4b, 0x16);
        let red = Color::Rgb(0xdc, 0x32, 0x2f);
        let magenta = Color::Rgb(0xd3, 0x36, 0x82);
        let cyan = Color::Rgb(0x2a, 0xa1, 0x98);
        let green = Color::Rgb(0x85, 0x99, 0x00);
        Self {
            accent: blue,
            status_bg: base2,
            selected_bg: Color::Rgb(0xdc, 0xd5, 0xbe), // a touch darker than base2
            visual_bg: Color::Rgb(0xf7, 0xe4, 0xb0), // warm highlight
            mark_fg: orange,
            base_bg: Some(base3),
            dim: base1,
            border: base1,
            // Menus and dialogs stay on Solarized's dark base (base02): a dark
            // panel over the light surface, so their existing light text reads
            // without recoloring every popup.
            popup_bg: Color::Rgb(0x07, 0x36, 0x42),
            file: FilePalette {
                directory: blue,
                code: yellow,
                config: cyan,
                document: base01,
                image: magenta,
                media: cyan,
                archive: red,
                executable: green,
                muted: base1,
                plain: base00,
            },
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

/// Interface language for the key manual / help text. Japanese is the default;
/// `cian.set_option("lang", "en")` switches to English.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ja,
    En,
}

impl Lang {
    /// From the `lang` option; anything but an explicit "ja" is English (the
    /// Lua layer already rejects values other than "ja"/"en").
    fn from_opt(opt: Option<&str>) -> Lang {
        match opt {
            Some("ja") => Lang::Ja,
            _ => Lang::En,
        }
    }

    /// Toggle to the other language.
    fn toggled(self) -> Lang {
        match self {
            Lang::En => Lang::Ja,
            Lang::Ja => Lang::En,
        }
    }
}

/// Pick the English or Japanese form of a fixed UI string.
fn tr(lang: Lang, en: &'static str, ja: &'static str) -> &'static str {
    match lang {
        Lang::En => en,
        Lang::Ja => ja,
    }
}

/// Localize the known progress-operation labels (`start_op`'s first argument).
/// Anything unrecognised (e.g. a directory path) is shown unchanged.
fn tr_op_label(lang: Lang, label: &str) -> String {
    if lang == Lang::En {
        return label.to_string();
    }
    match label {
        "copying" => "コピー中",
        "moving" => "移動中",
        "uploading" => "アップロード中",
        "downloading" => "ダウンロード中",
        "hashing" => "チェックサム計算中",
        "elevating" => "管理者権限で実行中",
        "comparing" => "比較中",
        other => return other.to_string(),
    }
    .to_string()
}

/// The "... and N more" overflow line, localized.
fn tr_count(lang: Lang, more: usize) -> String {
    match lang {
        Lang::En => format!("  ... and {} more", more),
        Lang::Ja => format!("  ... 他 {} 件", more),
    }
}

/// Remappable normal-mode actions. Keys the user binds via `cian.set_keymap`
/// resolve to one of these; the default key handling is otherwise untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    CursorDown,
    CursorUp,
    CursorTop,
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
    /// Make the active pane show the other pane's directory (pull).
    SyncFromOther,
    /// Make the other pane show the active pane's directory (push).
    SyncToOther,
    OpenExternal,
    CopyPath,
    CopyFileRef,
    MarkDown,
    MarkUp,
    InvertMarks,
    Visual,
    Command,
    Filter,
    FindRecursive,
    GrepRecursive,
    Sort,
    JumpPath,
    View,
    Diff,
    Refresh,
    Menu,
    Ssh,
    NewTab,
    CloseTab,
    Manual,
    /// Bound to a key to disable it — the key does nothing, shadowing whatever
    /// default it would otherwise trigger.
    Nop,
}

/// Map a Lua action name to an [`Action`]. Unknown names are reported as
/// config errors rather than silently ignored.
fn action_from_name(name: &str) -> Option<Action> {
    Some(match name {
        "cursor_down" => Action::CursorDown,
        "cursor_up" => Action::CursorUp,
        "cursor_top" => Action::CursorTop,
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
        "sync_from_other" => Action::SyncFromOther,
        "sync_to_other" => Action::SyncToOther,
        "open_external" => Action::OpenExternal,
        "copy_path" => Action::CopyPath,
        "copy_file_ref" => Action::CopyFileRef,
        "mark_down" => Action::MarkDown,
        "mark_up" => Action::MarkUp,
        "invert_marks" => Action::InvertMarks,
        "visual" => Action::Visual,
        "command" => Action::Command,
        "filter" => Action::Filter,
        "find_recursive" => Action::FindRecursive,
        "grep_recursive" => Action::GrepRecursive,
        "sort" => Action::Sort,
        "jump_path" => Action::JumpPath,
        "view" => Action::View,
        "diff" => Action::Diff,
        "refresh" => Action::Refresh,
        "menu" => Action::Menu,
        "ssh" => Action::Ssh,
        "new_tab" => Action::NewTab,
        "close_tab" => Action::CloseTab,
        "manual" => Action::Manual,
        "none" | "nop" | "unbind" => Action::Nop,
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
/// Named palettes selectable with `cian.set_theme "<name>"`.
fn theme_preset(name: &str) -> Option<ResolvedTheme> {
    match name.trim().to_lowercase().replace([' ', '_'], "-").as_str() {
        "solarized-light" | "solarized" => Some(ResolvedTheme::solarized_light()),
        "default" | "dark" => Some(ResolvedTheme::default()),
        _ => None,
    }
}

fn resolve_theme(t: &cian_lua::Theme) -> (ResolvedTheme, Vec<String>) {
    let mut errors = Vec::new();
    // Start from the named preset if one was chosen, else the dark default.
    let mut c = match &t.preset {
        Some(name) => theme_preset(name).unwrap_or_else(|| {
            errors.push(format!(
                "theme.preset: unknown preset {:?} (try \"solarized-light\")",
                name
            ));
            ResolvedTheme::default()
        }),
        None => ResolvedTheme::default(),
    };
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
    /// Every tab's pane, for settings (like show-hidden) that apply to all.
    pub fn all_mut(&mut self) -> impl Iterator<Item = &mut Pane> {
        self.tabs.iter_mut()
    }
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

    /// Walk up from the active leaf to the nearest split laid out along `want`,
    /// returning its node index. Used to pick which boundary a resize key moves
    /// — a Left/Right key resizes the nearest side-by-side split, Up/Down the
    /// nearest stacked one.
    fn nearest_split(&self, want: SplitDir) -> Option<usize> {
        let mut child = self.active;
        while let Some((parent, _)) = self.parent_of(child) {
            if let Some(Node::Split { dir, .. }) = self.nodes.get(parent).and_then(|n| n.as_ref()) {
                if *dir == want {
                    return Some(parent);
                }
            }
            child = parent;
        }
        None
    }

    /// Nudge a split's ratio by `delta`, clamped so neither child vanishes.
    fn nudge_split(&mut self, node: usize, delta: i16) {
        if let Some(Node::Split { ratio, .. }) =
            self.nodes.get_mut(node).and_then(|n| n.as_mut())
        {
            let next = (*ratio as i16 + delta).clamp(MIN_SPLIT_PCT as i16, 100 - MIN_SPLIT_PCT as i16);
            *ratio = next as u16;
        }
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
    /// The configured shell program (path/name), for prompts that need it.
    fn command(&self) -> &str {
        &self.shell_cmd
    }

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

    /// The active pane's terminal title (what the shell/program set via OSC) —
    /// usually `user@host: cwd`. Empty titles return None.
    fn active_title(&self) -> Option<String> {
        let t = self.active_tab()?;
        let s = match t.nodes.get(t.active).and_then(|n| n.as_ref()) {
            Some(Node::Leaf { session, .. }) => session,
            _ => return None,
        };
        let title = s.parser().lock().ok()?.screen().title().trim().to_string();
        if title.is_empty() { None } else { Some(title) }
    }

    /// The active pane's position among the tab's panes, `(index, total)`,
    /// 1-based — for the "1 of 3" hint while one pane is maximized.
    fn active_pane_position(&self) -> (usize, usize) {
        match self.active_tab() {
            Some(t) => {
                let leaves = t.leaves();
                let pos = leaves.iter().position(|&l| l == t.active).map(|i| i + 1).unwrap_or(1);
                (pos, leaves.len())
            }
            None => (1, 1),
        }
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

/// A drag-selection inside a shell pane, in that pane's grid coordinates
/// (row/col relative to the PTY area), used to copy terminal text.
#[derive(Debug, Clone, Copy)]
struct ShellSel {
    tab: usize,
    leaf: usize,
    /// The PTY area on screen, to map cells for highlighting.
    inner: Rect,
    /// Anchor and moving end, as `(grid_row, grid_col)`.
    anchor: (u16, u16),
    end: (u16, u16),
    /// True once the pointer moved — a bare click just focuses.
    dragged: bool,
}

/// Which way an SFTP transfer goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScpDir {
    /// Local files → remote directory.
    Upload,
    /// A remote file → local directory.
    Download,
}

/// A transfer waiting on the remote path being typed. Held on `App` rather than
/// in the popup so the resolved password never reaches a `Debug`-formatted
/// `Popup`.
struct ScpPending {
    target: cian_scp::Target,
    label: String,
    dir: ScpDir,
    /// Upload: the local files to send. Download: unused.
    locals: Vec<PathBuf>,
    /// Download: the local directory to save into. Upload: unused.
    local_dir: PathBuf,
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
    /// Choose the encoding the active shell pane's output is decoded with.
    EncodingPicker { cursor: usize, target: EncTarget },
    /// A file's contents, scrollable.
    Viewer {
        title: String,
        /// The file on disk, so `Shift+Enter` can reveal it in the pane.
        path: PathBuf,
        view: cian_core::viewer::View,
        /// First visible line.
        scroll: usize,
        /// Cursor line (absolute).
        line: usize,
        /// Cursor column, as a char index into that line.
        col: usize,
        /// Remembered column for vertical motion (vim's "goal column");
        /// `usize::MAX` means "end of line" (as after `$`).
        goal: usize,
        /// Active visual selection mode; `None` in normal mode.
        visual: Option<ViewVisual>,
        /// Selection anchor `(line, col)`, meaningful while `visual` is `Some`.
        anchor: (usize, usize),
        /// While typing a `/` search, the text entered so far; `None` otherwise.
        find_input: Option<String>,
        /// The confirmed search pattern, kept for `n`/`N` and match highlight.
        find_query: Option<String>,
        /// A pending numeric count typed before a motion (vim's `42G`).
        count: Option<usize>,
        /// Per-line git change status vs HEAD (the change gutter), keyed by
        /// 0-based line index. Empty when not tracked or unchanged.
        git_lines: std::collections::HashMap<usize, cian_core::git::LineChange>,
    },
    /// The recursive comparison of two directories: a list of differing paths.
    DirCompare {
        left: String,
        right: String,
        left_root: PathBuf,
        right_root: PathBuf,
        entries: Vec<cian_core::dirdiff::Entry>,
        cursor: usize,
        scroll: usize,
        truncated: bool,
    },
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
    /// Bookmarks. `entries` is the whole tree; `path` is the group currently
    /// open (a breadcrumb of indices), and `cursor` indexes within that level.
    Shortcuts { entries: Vec<Shortcut>, cursor: usize, path: Vec<usize> },
    ConfirmQuit,
    ConfirmClose { target: CloseTarget },
    /// An AI-generated shell command awaiting review before it goes to the
    /// prompt (never auto-run).
    AiShellConfirm { command: String },
    /// The AI chat: a transcript, an input line, and whether a reply is pending.
    /// `sel` is a selected range of wrapped transcript lines `(anchor, cursor)`,
    /// for copying, mirroring the F3 viewer's line selection.
    AiChat {
        input: String,
        log: Vec<ChatMsg>,
        scroll: usize,
        pending: bool,
        sel: Option<(usize, usize)>,
    },
    /// A copy/move failed because the destination needs administrator rights.
    /// Offers to redo it elevated (Windows only).
    ConfirmElevate { op: PendingOp, targets: Vec<PathBuf>, dest: PathBuf },
    /// An AI-drafted commit message, shown editable before it is committed.
    /// `dir` is the repo the staged diff came from; `stat` summarises the files;
    /// `editing` toggles between preview and typing into `buffer`.
    CommitMessage { buffer: String, stat: String, dir: PathBuf, editing: bool },
    /// The AI's junk-file suggestions, each toggleable, before deletion. Nothing
    /// is deleted from here directly — approving hands the checked paths to the
    /// normal delete confirmation.
    JunkReview { items: Vec<JunkItem>, cursor: usize, scroll: usize },
    /// The AI's proposed folder structure: a set of moves (file → subfolder),
    /// each toggleable. Approving runs the checked moves, creating folders as
    /// needed. `dir` is the folder the moves are relative to.
    StructureReview { items: Vec<MoveItem>, cursor: usize, scroll: usize, dir: PathBuf },
    /// The AI's proposed renames (old → new), each toggleable. Approving renames
    /// the checked files in place.
    RenameReview { items: Vec<RenameItem>, cursor: usize, scroll: usize },
    /// Confirm discarding (reverting) worktree changes to tracked files. This
    /// throws away uncommitted work, so it is gated behind its own dialog.
    ConfirmDiscard { targets: Vec<PathBuf>, dir: PathBuf },
    /// Duplicate files found by content, grouped, each toggleable. Approving
    /// hands the checked copies to the normal delete confirmation.
    DupeReview { items: Vec<DupeItem>, cursor: usize, scroll: usize },
}

/// One file in a duplicate group. `group` is its 0-based group index (files in
/// the same group are byte-identical); `keeper` marks the one row per group left
/// unchecked by default, so approving deletes the redundant copies, not all of
/// them.
#[derive(Debug, Clone)]
struct DupeItem {
    path: PathBuf,
    group: usize,
    keeper: bool,
    selected: bool,
}

/// One candidate the junk detector flagged: a path, why it thinks so, and
/// whether it is currently checked for deletion.
#[derive(Debug, Clone)]
struct JunkItem {
    path: PathBuf,
    reason: String,
    selected: bool,
}

/// One proposed move in a structure suggestion: take `path` (its name shown as
/// `name`) into the sub-folder `dest` (relative to the pane's directory,
/// created if missing), with the AI's short rationale.
#[derive(Debug, Clone)]
struct MoveItem {
    path: PathBuf,
    name: String,
    dest: String,
    reason: String,
    selected: bool,
}

/// One proposed rename: `path` (currently named `old`) becomes `new` (a bare
/// filename in the same directory).
#[derive(Debug, Clone)]
struct RenameItem {
    path: PathBuf,
    old: String,
    new: String,
    selected: bool,
}

/// What the encoding picker applies its choice to.
#[derive(Debug, Clone)]
enum EncTarget {
    /// The active shell pane's live output decoding.
    Shell,
    /// A stashed F3 viewer to re-decode and restore when the pick is made.
    Viewer(Box<Popup>),
}

/// The F3 viewer's visual-selection mode, matching vim's three flavours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewVisual {
    /// `v`: character-wise, from the anchor cell to the cursor cell.
    Char,
    /// `V`: line-wise, whole lines between anchor and cursor.
    Line,
    /// `Ctrl-v`: block-wise, the rectangle of columns between them.
    Block,
}

/// A clickable region of the on-screen popup, registered by `draw_popup` and
/// consumed by the mouse handler. Rather than duplicate every popup's layout in
/// the mouse code, the draw side (which owns the geometry) records what each
/// rect means, and clicks are turned back into the popup's own key actions.
#[derive(Debug, Clone, Copy)]
struct PopupZone {
    rect: Rect,
    kind: ZoneKind,
}

/// What clicking a [`PopupZone`] does, expressed as the keystroke it stands in
/// for so the existing popup key handlers do the actual work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZoneKind {
    /// Put the list cursor on this index, then confirm (Enter).
    SelectRow(usize),
    /// Stand in for a character key (a confirm dialog's y/n/a/r button).
    Char(char),
    /// Stand in for Enter / Esc (dialog OK / cancel).
    Enter,
    Esc,
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
    /// Send the selected file(s) to a server over SFTP.
    ScpUpload,
    /// Fetch a file from a server over SFTP into this pane.
    ScpDownload,
    /// Begin recording this shell pane's output to a log file.
    StartLog,
    /// Stop the recording running on this shell pane.
    StopLog,
    /// Cycle the encoding the shell output is decoded with.
    Encoding,
    /// Toggle the interface language (English ↔ Japanese).
    Lang,
    /// Open the AI chat.
    AiChat,
    /// Generate a shell command from a description (shell pane).
    AiShellCmd,
    /// Explain the error shown in the shell pane.
    AiExplainError,
    /// Draft a git commit message from the staged diff.
    AiCommit,
    /// Detect junk files in the current directory.
    AiJunk,
    /// Find duplicate files by content (not AI).
    FindDupes,
    /// Suggest an organised folder structure for the current directory.
    AiStructure,
    /// Bulk-rename the marked files (or the whole listing) by an instruction.
    AiRename,
    /// Semantic search over the tree from a natural-language query.
    AiSearch,
    /// A submenu grouping the git actions (stage / unstage / discard).
    GitMenu,
    /// `git add` the selection.
    GitStage,
    /// `git reset HEAD` the selection.
    GitUnstage,
    /// `git checkout --` the selection (discard worktree changes).
    GitDiscard,
    /// Open the shortcuts / bookmarks menu (the `s` key).
    Shortcuts,
    /// A submenu grouping the AI actions (drills down when chosen).
    AiMenu,
    /// A submenu grouping the file-transfer actions.
    SendMenu,
    /// A submenu grouping the shell window actions (splits, tabs, zoom).
    WindowMenu,
    /// Split the active shell tab left/right (S-F8).
    ShellSplitLR,
    /// Split the active shell tab top/bottom (S-F9).
    ShellSplitTB,
    /// Open a new shell tab (F9).
    ShellNewTab,
    /// Close the active shell split pane (S-F10).
    ShellCloseSplit,
    /// Close the active shell tab (F10).
    ShellCloseTab,
    /// Zoom the shell surface (F12).
    ShellZoom,
    /// Goes back up from a submenu to its parent.
    Back,
    Quit,
    Manual,
}

impl MenuItem {
    /// Group items open a submenu instead of acting; this is their marker.
    fn is_group(self) -> bool {
        matches!(self, MenuItem::AiMenu | MenuItem::SendMenu | MenuItem::WindowMenu | MenuItem::GitMenu)
    }
}

impl MenuItem {
    fn label(self, lang: Lang) -> &'static str {
        match self {
            MenuItem::Copy => tr(lang, "Copy", "コピー"),
            MenuItem::Cut => tr(lang, "Cut", "カット"),
            MenuItem::Paste => tr(lang, "Paste", "貼り付け"),
            MenuItem::CopyToOther => tr(lang, "Copy to other pane", "反対ペインへコピー"),
            MenuItem::MoveToOther => tr(lang, "Move to other pane", "反対ペインへ移動"),
            MenuItem::CopyToPath => tr(lang, "Copy to  (recent / typed)", "指定先へコピー  (履歴/入力)"),
            MenuItem::Delete => tr(lang, "Delete (to trash)", "削除（ゴミ箱へ）"),
            MenuItem::Rename => tr(lang, "Rename", "リネーム"),
            MenuItem::Background => tr(lang, "Background color", "背景色"),
            MenuItem::HiddenToggle => tr(lang, "Show / hide dotfiles", "ドットファイルの表示切替"),
            MenuItem::Attributes => tr(lang, "Attributes", "属性"),
            MenuItem::Hash => tr(lang, "Checksum", "チェックサム"),
            MenuItem::Compare => tr(lang, "Compare left ↔ right", "左右を比較"),
            MenuItem::Ssh => tr(lang, "SSH connect", "SSH接続"),
            MenuItem::ScpUpload => tr(lang, "Upload → server", "アップロード → サーバ"),
            MenuItem::ScpDownload => tr(lang, "Download ← server", "ダウンロード ← サーバ"),
            MenuItem::StartLog => tr(lang, "Start session log", "セッションログ開始"),
            MenuItem::StopLog => tr(lang, "Stop session log  ●", "セッションログ停止  ●"),
            MenuItem::Encoding => tr(lang, "Text encoding", "文字コード"),
            MenuItem::Quit => tr(lang, "Quit cian  (q)", "cian を終了  (q)"),
            // Labelled with the language it switches *to*, so the action is
            // clear whichever language the menu is currently in.
            MenuItem::Lang => match lang {
                Lang::En => "日本語に切替",
                Lang::Ja => "Switch to English",
            },
            MenuItem::AiChat => tr(lang, "Chat", "チャット"),
            MenuItem::AiShellCmd => tr(lang, "Command from description", "説明からコマンド生成"),
            MenuItem::AiExplainError => tr(lang, "Explain the last error", "直近のエラーを説明"),
            MenuItem::AiCommit => tr(lang, "Draft commit message", "コミットメッセージ生成"),
            MenuItem::AiJunk => tr(lang, "Detect junk files", "ゴミファイル検出"),
            MenuItem::FindDupes => tr(lang, "Find duplicate files", "重複ファイルを検出"),
            MenuItem::AiStructure => tr(lang, "Suggest folder structure", "フォルダ構成を提案"),
            MenuItem::AiRename => tr(lang, "Bulk rename", "一括リネーム"),
            MenuItem::AiSearch => tr(lang, "Semantic search", "セマンティック検索"),
            MenuItem::GitMenu => tr(lang, "Git ▸", "Git ▸"),
            MenuItem::GitStage => tr(lang, "Stage  (git add)", "ステージ  (git add)"),
            MenuItem::GitUnstage => tr(lang, "Unstage  (git reset)", "アンステージ  (git reset)"),
            MenuItem::GitDiscard => tr(lang, "Discard changes  (git checkout)", "変更を破棄  (git checkout)"),
            MenuItem::Shortcuts => tr(lang, "Shortcuts  (s)", "ショートカット  (s)"),
            MenuItem::AiMenu => tr(lang, "AI ▸", "AI ▸"),
            MenuItem::SendMenu => tr(lang, "Transfer ▸", "転送 ▸"),
            MenuItem::WindowMenu => tr(lang, "Window ▸", "ウィンドウ ▸"),
            MenuItem::ShellSplitLR => tr(lang, "Split left / right  (S-F8)", "左右に分割  (S-F8)"),
            MenuItem::ShellSplitTB => tr(lang, "Split top / bottom  (S-F9)", "上下に分割  (S-F9)"),
            MenuItem::ShellNewTab => tr(lang, "New tab  (F9)", "新規タブ  (F9)"),
            MenuItem::ShellCloseSplit => tr(lang, "Close split pane  (S-F10)", "分割パネルを閉じる  (S-F10)"),
            MenuItem::ShellCloseTab => tr(lang, "Close tab  (F10)", "タブを閉じる  (F10)"),
            MenuItem::ShellZoom => tr(lang, "Zoom  (F12)", "ズーム  (F12)"),
            MenuItem::Back => tr(lang, "◂ Back", "◂ 戻る"),
            MenuItem::Manual => tr(lang, "Key manual  (?)", "キー一覧  (?)"),
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

/// Two clicks closer together than this on the same row count as a double-click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

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
/// A directory comparison running on a worker thread. It streams progress and
/// delivers the whole result when the walk finishes.
struct DiffJob {
    rx: std::sync::mpsc::Receiver<DiffMsg>,
    cancel: Arc<AtomicBool>,
    left_root: PathBuf,
    right_root: PathBuf,
    left: String,
    right: String,
    /// Latest progress, for the bar.
    latest: cian_core::progress::Progress,
    label: &'static str,
    started: Instant,
}

enum DiffMsg {
    Tick(cian_core::progress::Progress),
    Done(cian_core::dirdiff::DirDiff),
}

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

/// What a finished AI reply should be used for, so one job plumbing serves
/// every AI feature.
#[derive(Debug, Clone)]
enum AiPurpose {
    /// Append to the chat transcript.
    Chat,
    /// A shell command to review and insert at the prompt.
    ShellCommand,
    /// A git commit message drafted from the staged diff. `dir`/`stat` are
    /// carried through so the editable preview can commit into the right repo.
    CommitMessage { dir: PathBuf, stat: String },
    /// Junk-file detection over a directory listing. `names` is the name→path
    /// list the model was shown, so its answer can be validated back to real,
    /// absolute paths (a hallucinated name simply matches nothing).
    Junk { names: Vec<(String, PathBuf)> },
    /// Structure suggestion over a directory listing. `names` validates the
    /// reply back to real paths; `dir` is the folder moves are relative to.
    Structure { names: Vec<(String, PathBuf)>, dir: PathBuf },
    /// Bulk rename over a chosen set of files. `names` validates the reply back
    /// to real paths.
    Rename { names: Vec<(String, PathBuf)> },
    /// Semantic search: the model picks relevant paths from a catalog. `hits`
    /// is the catalog it was shown, so the reply validates back to real hits.
    SemSearch { hits: Vec<cian_core::search::Hit> },
}

/// A pending AI request; the worker sends the assistant's reply (or an error
/// message) back over the channel, tagged with what to do with it.
struct AiJob {
    rx: std::sync::mpsc::Receiver<Result<String, String>>,
    purpose: AiPurpose,
}

/// One line of an AI chat transcript.
#[derive(Debug, Clone)]
struct ChatMsg {
    /// True for the user's turn, false for the assistant's.
    user: bool,
    text: String,
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
/// Saturated hues spaced around the wheel, pushed as strong as they can go
/// while foreground text stays readable (luminance kept under 90). Blue carries
/// little luminance, so blues can run brightest; greens are held back most.
/// Verified by `the_palette_is_distinct_enough_to_tell_panes_apart`.
const PANE_BG_PRESETS: [(&str, Option<Color>); 9] = [
    ("default", None),
    ("navy", Some(Color::Rgb(10, 40, 140))),
    ("teal", Some(Color::Rgb(10, 105, 105))),
    ("forest", Some(Color::Rgb(20, 110, 20))),
    ("olive", Some(Color::Rgb(108, 88, 10))),
    ("rust", Some(Color::Rgb(140, 45, 15))),
    ("wine", Some(Color::Rgb(135, 15, 80))),
    ("plum", Some(Color::Rgb(75, 15, 140))),
    ("slate", Some(Color::Rgb(70, 85, 120))),
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
    /// Naming a shortcut. `path` is the group it lives in; `edit_idx` is set
    /// when renaming an existing one; `group` makes it a folder (no target).
    ShortcutName { path: Vec<usize>, edit_idx: Option<usize>, group: bool },
    ShortcutTarget { path: Vec<usize>, edit_idx: Option<usize>, name: String },
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
    /// A directory to write a session log into; the file name is generated.
    LogDir,
    /// The remote path for a pending SFTP transfer (details on `App`).
    ScpRemote,
    /// A natural-language description to turn into a shell command via AI.
    AiShellCmd,
    /// A natural-language instruction for how to bulk-rename the chosen files.
    AiRename,
    /// A natural-language query for semantic search over the tree.
    AiSearch,
}

impl InputKind {
    /// Whether the field holds a secret and should be shown as dots.
    fn is_secret(&self) -> bool {
        matches!(self, InputKind::ZipPassword { .. })
    }
}

/// A bookmark: either a leaf (`target` set) or a group/folder (`children` set)
/// that drills into more shortcuts. The two are mutually exclusive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shortcut {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<Shortcut>>,
}

impl Shortcut {
    fn leaf(name: String, target: String) -> Self {
        Self { name, target: Some(target), children: None }
    }
    fn group(name: String) -> Self {
        Self { name, target: None, children: Some(Vec::new()) }
    }
    fn is_group(&self) -> bool {
        self.children.is_some()
    }
    fn target_str(&self) -> &str {
        self.target.as_deref().unwrap_or("")
    }
}

/// The list of shortcuts at `path` (indices to descend through groups). Empty if
/// the path does not resolve.
fn sc_level<'a>(entries: &'a [Shortcut], path: &[usize]) -> &'a [Shortcut] {
    let mut cur = entries;
    for &i in path {
        match cur.get(i).and_then(|s| s.children.as_deref()) {
            Some(ch) => cur = ch,
            None => return &[],
        }
    }
    cur
}

/// Mutable variant of [`sc_level`].
fn sc_level_mut<'a>(entries: &'a mut Vec<Shortcut>, path: &[usize]) -> Option<&'a mut Vec<Shortcut>> {
    let mut cur = entries;
    for &i in path {
        cur = cur.get_mut(i)?.children.as_mut()?;
    }
    Some(cur)
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
    fn config_dir() -> PathBuf {
        home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".config")
            .join("cian")
    }

    /// The YAML file bookmarks are stored in now.
    fn default_path() -> PathBuf {
        Self::config_dir().join("shortcuts.yaml")
    }

    /// The old TOML file, read once to migrate anyone who has one.
    fn legacy_toml_path() -> PathBuf {
        Self::config_dir().join("shortcuts.toml")
    }

    pub fn load_or_default() -> Self {
        let path = Self::default_path();
        // Prefer the YAML file; fall back to a legacy TOML one and migrate it.
        if let Some(entries) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_yml::from_str::<ShortcutsFile>(&s).ok())
            .map(|f| f.shortcuts)
        {
            return Self { entries, path };
        }
        let legacy = Self::legacy_toml_path();
        if let Some(entries) = std::fs::read_to_string(&legacy)
            .ok()
            .and_then(|s| toml::from_str::<ShortcutsFile>(&s).ok())
            .map(|f| f.shortcuts)
        {
            // One-time migration: write the YAML copy and leave the old file in
            // place (harmless, and a safety net if something goes wrong).
            let store = Self { entries, path };
            let _ = store.save();
            return store;
        }
        Self { entries: Vec::new(), path }
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = ShortcutsFile { shortcuts: self.entries.clone() };
        let s = serde_yml::to_string(&file)?;
        std::fs::write(&self.path, s)?;
        Ok(())
    }
}

/// A pane's cached git status and the directory it was computed for.
struct GitState {
    cwd: PathBuf,
    status: Option<cian_core::git::RepoStatus>,
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
    /// One shell split pane growing to fill the shell panel (Shift+F12), or
    /// shrinking back into its slot. Like `Zoom`, but the backdrop keeps the
    /// splits so the pane visibly grows out of them.
    PaneZoom { from: Rect, to: Rect },
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
    /// Draw the shell's split panes even when one is flagged maximized — used
    /// while a pane-zoom transition floats the growing pane above the splits.
    show_splits: bool,
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
    /// The entry index the drag started on. A drag that stays inside the origin
    /// pane rubber-band-selects from here to the row under the pointer.
    anchor: usize,
    /// True once the pointer has reached a row other than the anchor, i.e. a
    /// real rubber-band selection has begun. A press-and-release on one row —
    /// even if the terminal reports a stray same-cell Drag — must stay a click
    /// and never touch the marks.
    rubber: bool,
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
    /// `(tab, leaf, outer rect, inner PTY rect)` for each shell split pane on
    /// screen, so a click can land on the pane under the pointer and a drag can
    /// map to a terminal cell.
    shell_leaves: Vec<(usize, usize, Rect, Rect)>,
    /// Clickable tab-label rects, rebuilt each frame: which pane's strip, the
    /// tab index, and where it sits, so a tab can be switched with the mouse.
    tab_rects: Vec<(FocusedPane, usize, Rect)>,
    /// The context menu's on-screen rect (inner area), for clicking its items.
    menu_rect: Rect,
    /// Parent context menus stashed while a submenu is open, so Esc/← drills
    /// back up instead of closing everything.
    menu_stack: Vec<Popup>,
    /// The viewer's text body rect, for mapping a mouse click to a line.
    viewer_rect: Rect,
    /// The viewer's line-number gutter width, so a click maps to a char column.
    viewer_gutter: u16,
    /// Clickable regions of whatever popup is on screen, rebuilt every frame by
    /// `draw_popup`, so dialogs and pickers can be driven entirely by mouse.
    popup_zones: Vec<PopupZone>,
    /// The last copy/move that failed on a permission error, kept so a Windows
    /// user can retry it elevated. `(op, sources, destination dir)`.
    pending_elevation: Option<(PendingOp, Vec<PathBuf>, PathBuf)>,
    /// The active shell pane's slot rect, stashed on pane-zoom so the shrink
    /// back knows where to land (the split rects are gone while zoomed).
    pane_zoom_return: Option<Rect>,
    /// Wall-clock start, used to phase the slow "recording" border pulse.
    started: Instant,
    /// The local side of an SFTP transfer being set up, carried through the SSH
    /// host/user picker: `(direction, files to upload, local save dir)`.
    scp_dir: Option<(ScpDir, Vec<PathBuf>, PathBuf)>,
    /// An SFTP transfer whose remote path is being entered, if any.
    scp_pending: Option<ScpPending>,
    /// Time and row of the last left-click in a file pane, to detect a
    /// double-click (which activates the entry).
    last_click: Option<(Instant, u16)>,
    /// An in-progress or finished text selection in a shell pane (its own
    /// selection, since cian holds the mouse the terminal would otherwise use).
    shell_sel: Option<ShellSel>,
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
    /// Interface language for the key manual (Japanese by default).
    lang: Lang,
    /// Cached git status per file pane `[left, right]`, recomputed when the
    /// pane's directory changes or on an explicit refresh.
    git: [Option<GitState>; 2],
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
    /// The grep-results popup stashed while viewing one hit in F3, so Esc from
    /// the viewer returns to the list rather than closing everything.
    find_return: Option<Box<Popup>>,
    /// AI helper config from `cian.ai{...}`; `None` disables every AI feature.
    ai: Option<cian_ai::AiConfig>,
    /// Whether the AI helper actually works (python + packages + sign-in),
    /// checked lazily on first use and cached. `None` until checked.
    ai_ready: Option<bool>,
    /// A pending AI request running on a worker thread.
    ai_job: Option<AiJob>,
    /// The chat transcript's on-screen body rect, the effective scroll offset,
    /// and the flat wrapped lines — rebuilt each frame so a mouse drag can map
    /// to a line range and copy it.
    ai_rect: Rect,
    ai_scroll: usize,
    ai_lines: Vec<String>,
    /// The junk-review list body rect, stashed so a click can map to a row.
    junk_rect: Rect,
    /// The structure-review list body rect, for the same reason.
    struct_rect: Rect,
    /// The rename-review list body rect, for the same reason.
    rename_rect: Rect,
    /// The dupe-review list body rect, for the same reason.
    dupe_rect: Rect,
    /// A running duplicate scan, delivering its groups when finished.
    dupes_job: Option<std::sync::mpsc::Receiver<Vec<Vec<PathBuf>>>>,
    diff_job: Option<DiffJob>,
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
        // Honour the show_hidden option on the initial panes (it defaults to
        // true, cian's long-standing behaviour).
        let show_hidden = config.options.show_hidden.unwrap_or(true);
        let mut left_pane = Pane::new(left)?;
        let mut right_pane = Pane::new(right)?;
        left_pane.set_show_hidden(show_hidden);
        right_pane.set_show_hidden(show_hidden);
        Ok(Self {
            left: PaneTabs::single(left_pane),
            right: PaneTabs::single(right_pane),
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
            last_click: None,
            shell_sel: None,
            tab_rects: Vec::new(),
            menu_rect: Rect::new(0, 0, 0, 0),
            menu_stack: Vec::new(),
            viewer_rect: Rect::new(0, 0, 0, 0),
            viewer_gutter: 0,
            popup_zones: Vec::new(),
            pending_elevation: None,
            pane_zoom_return: None,
            started: Instant::now(),
            scp_dir: None,
            scp_pending: None,
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
            lang: Lang::from_opt(config.options.lang.as_deref()),
            git: [None, None],
            ai: config.ai.as_ref().map(|a| cian_ai::AiConfig {
                python: a.python.clone(),
                endpoint: a.endpoint.clone(),
                model: a.model.clone(),
                api_version: a.api_version.clone(),
                auth_mode: a.auth_mode.clone(),
                api_key: a.api_key.clone(),
                api_base_url: a.api_base_url.clone(),
            }),
            ai_ready: None,
            ai_job: None,
            ai_rect: Rect::new(0, 0, 0, 0),
            junk_rect: Rect::new(0, 0, 0, 0),
            struct_rect: Rect::new(0, 0, 0, 0),
            rename_rect: Rect::new(0, 0, 0, 0),
            dupe_rect: Rect::new(0, 0, 0, 0),
            dupes_job: None,
            ai_scroll: 0,
            ai_lines: Vec::new(),
            zoom_return: None,
            pending_shell_input: None,
            pending_shortcut_target: None,
            pending_auth: None,
            op_job: None,
            find_job: None,
            find_return: None,
            diff_job: None,
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
    /// Recompute each pane's git status when its directory has changed since the
    /// last cache. Called once per frame; the `git` shell-out only runs on an
    /// actual directory change, so it costs nothing while browsing one folder.
    fn ensure_git(&mut self) {
        for (idx, tabs) in [&self.left, &self.right].into_iter().enumerate() {
            let cwd = tabs.active_ref().cwd.clone();
            let stale = self.git[idx].as_ref().map(|g| g.cwd != cwd).unwrap_or(true);
            if stale {
                let status = cian_core::git::status(&cwd);
                self.git[idx] = Some(GitState { cwd, status });
            }
        }
    }

    /// Drop the git cache so the next frame recomputes it — after a git action
    /// or a file operation that may have changed the working tree.
    fn invalidate_git(&mut self) {
        self.git = [None, None];
    }

    /// The active file pane's directory, if it sits in a git repository. Uses
    /// the cached status when it is warm, and falls back to a direct check (the
    /// cache is cold right after a git action invalidates it).
    fn git_repo_dir(&self) -> Option<PathBuf> {
        match self.focused {
            FocusedPane::Left | FocusedPane::Right => {
                let cwd = self.active_pane()?.cwd.clone();
                if self.git_for(self.focused).is_some() || cian_core::git::status(&cwd).is_some() {
                    Some(cwd)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// The selection to act on for a git command: marked files, else the entry
    /// under the cursor (never the `..` row).
    fn git_targets(&self) -> Vec<PathBuf> {
        self.active_pane().map(|p| p.target_paths()).unwrap_or_default()
    }

    /// `git add` the selection.
    fn git_stage(&mut self) {
        let Some(dir) = self.git_repo_dir() else {
            self.message = Some("not a git repository".into());
            return;
        };
        let paths = self.git_targets();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        match cian_core::git::stage(&dir, &paths) {
            Ok(()) => {
                self.message = Some(format!("● staged {} path(s)", paths.len()));
                self.invalidate_git();
                self.reload_active();
            }
            Err(e) => self.message = Some(format!("git add: {}", e)),
        }
    }

    /// `git reset HEAD` the selection (unstage, keeping worktree changes).
    fn git_unstage(&mut self) {
        let Some(dir) = self.git_repo_dir() else {
            self.message = Some("not a git repository".into());
            return;
        };
        let paths = self.git_targets();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        match cian_core::git::unstage(&dir, &paths) {
            Ok(()) => {
                self.message = Some(format!("unstaged {} path(s)", paths.len()));
                self.invalidate_git();
                self.reload_active();
            }
            Err(e) => self.message = Some(format!("git reset: {}", e)),
        }
    }

    /// Open the confirm dialog for discarding worktree changes to the selection.
    fn git_discard_prompt(&mut self) {
        let Some(dir) = self.git_repo_dir() else {
            self.message = Some("not a git repository".into());
            return;
        };
        let paths = self.git_targets();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        self.popup = Popup::ConfirmDiscard { targets: paths, dir };
    }

    /// `git checkout --` the selection: throw away worktree changes to tracked
    /// files (untracked files are left alone). Called after the confirm.
    fn git_discard(&mut self) {
        let popup = std::mem::replace(&mut self.popup, Popup::None);
        let Popup::ConfirmDiscard { targets, dir } = popup else { return };
        match cian_core::git::discard(&dir, &targets) {
            Ok(()) => {
                self.message = Some(format!("discarded changes to {} path(s)", targets.len()));
                self.invalidate_git();
                self.reload_active();
            }
            Err(e) => self.message = Some(format!("git checkout: {}", e)),
        }
    }

    /// The git status for a file pane, if it sits in a repo.
    fn git_for(&self, pane: FocusedPane) -> Option<&cian_core::git::RepoStatus> {
        let idx = match pane {
            FocusedPane::Left => 0,
            FocusedPane::Right => 1,
            FocusedPane::Shell => return None,
        };
        self.git[idx].as_ref().and_then(|g| g.status.as_ref())
    }

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
            "ai" | "chat" => self.open_ai_chat(),
            "aicmd" => {
                if rest.is_empty() {
                    self.start_ai_shell_prompt();
                } else {
                    self.start_ai_shell_cmd(rest);
                }
            }
            "stage" | "add" => self.git_stage(),
            "unstage" | "reset" => self.git_unstage(),
            "discard" | "revert" | "checkout" => self.git_discard_prompt(),
            "aicommit" | "commitmsg" => self.start_ai_commit_message(),
            "aijunk" | "junk" => self.start_ai_junk(),
            "aiorganize" | "aistructure" | "organize" => self.start_ai_structure(),
            "airename" | "rename" => {
                if rest.is_empty() {
                    self.start_ai_rename_prompt();
                } else {
                    self.start_ai_rename(rest);
                }
            }
            "aisearch" | "semsearch" | "ask" => {
                if rest.is_empty() {
                    self.start_ai_search_prompt();
                } else {
                    self.start_ai_search(rest);
                }
            }
            "aierror" | "explain" => self.explain_shell_error(),
            "dupes" | "dup" | "duplicates" => self.start_dupes(),
            "reload" | "source" => self.reload_config(),
            // Mark / unmark entries whose name matches a glob (`:mark *.rs`).
            "mark" | "select" => self.cmd_mark(rest, true),
            "unmark" | "deselect" => self.cmd_mark(rest, false),

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
    /// `:mark <glob>` / `:unmark <glob>` — (un)mark every entry whose name
    /// matches the wildcard pattern (`*`, `?`; case-insensitive). No pattern
    /// acts on all entries.
    fn cmd_mark(&mut self, pattern: &str, mark: bool) {
        let pat = pattern.trim();
        let Some(p) = self.active_pane_mut() else { return };
        let mut n = 0usize;
        for i in 0..p.entries.len() {
            let name = p.entries[i].name.to_lowercase();
            if pat.is_empty() || glob_match(&pat.to_lowercase(), &name) {
                let was = p.is_marked(i);
                if mark && !was {
                    p.set_mark_at(i);
                    n += 1;
                } else if !mark && was {
                    p.toggle_mark_at(i);
                    n += 1;
                }
            }
        }
        self.message = Some(format!(
            "{} {} entr{}",
            if mark { "marked" } else { "unmarked" },
            n,
            if n == 1 { "y" } else { "ies" }
        ));
    }

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
        // `:ls -a` still toggles dotfiles (the long-standing behaviour). A plain
        // `:ls` now shows the same Attributes window as the menu, but for every
        // entry in the listing — a detailed `ls -l`-style view.
        if args.iter().any(|a| a.starts_with('-') && a.contains('a')) {
            self.toggle_hidden();
            return;
        }
        let paths: Vec<PathBuf> = match self.active_pane() {
            Some(p) => p.entries.iter().filter(|e| !e.is_parent).map(|e| e.path.clone()).collect(),
            None => Vec::new(),
        };
        if paths.is_empty() {
            self.message = Some("empty directory".into());
            return;
        }
        // Same cap as the Attributes window — the popup is not scrollable, and a
        // longer list would clip; the trailing "… and N more" says so.
        self.popup = Popup::Notice { lines: self.attributes_lines(&paths, 40) };
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
        // A directory opens the other pane on it; anything else (a file, or an
        // empty pane) opens the other pane on *this* directory, so the two
        // panes line up on the same folder.
        let target = match self.active_pane() {
            Some(p) => match p.selected() {
                Some(e) if e.is_dir => e.path.clone(),
                _ => p.cwd.clone(),
            },
            None => return Ok(()),
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

    /// `o` — make the ACTIVE pane show the same directory as the other pane
    /// (pull). E.g. on the right pane, the right pane jumps to the left's cwd.
    fn sync_active_from_other(&mut self) -> Result<()> {
        let other_cwd = match self.focused {
            FocusedPane::Left => self.right.active_ref().cwd.clone(),
            FocusedPane::Right => self.left.active_ref().cwd.clone(),
            FocusedPane::Shell => return Ok(()),
        };
        if let Some(p) = self.active_pane_mut() {
            if p.cwd == other_cwd {
                self.message = Some("panes already in the same directory".into());
                return Ok(());
            }
            p.jump_to(other_cwd.clone())?;
        }
        self.message = Some(format!("this pane → {}", other_cwd.display()));
        Ok(())
    }

    /// `O` — make the OTHER pane show the same directory as the active pane
    /// (push). E.g. on the right pane, the left pane jumps to the right's cwd.
    fn sync_other_from_active(&mut self) -> Result<()> {
        let cwd = match self.active_pane() {
            Some(p) => p.cwd.clone(),
            None => return Ok(()),
        };
        let other = match self.focused {
            FocusedPane::Left => &mut self.right,
            FocusedPane::Right => &mut self.left,
            FocusedPane::Shell => return Ok(()),
        };
        if other.active_ref().cwd == cwd {
            self.message = Some("panes already in the same directory".into());
            return Ok(());
        }
        other.active_mut().jump_to(cwd.clone())?;
        // Focus stays on the active pane.
        self.message = Some(format!("other pane → {}", cwd.display()));
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

    /// Ask where to write this shell pane's session log. The file name is
    /// generated on submit from the time and the pane's host.
    fn start_log_prompt(&mut self) {
        if self.shell.active_session().is_none() {
            self.message = Some("no shell here to log".into());
            return;
        }
        // Seed with a sensible directory: the focused file pane's, else home.
        let seed = self
            .last_file_pane_cwd()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        self.popup = text_input(
            "session log — folder",
            "directory to save the log in  (Ctrl+V paste):",
            seed,
            InputKind::LogDir,
        );
    }

    /// Start logging the active shell pane into `dir`, building the file name
    /// from the timestamp and the pane's host (e.g. `20260723_140501_myhost.log`).
    fn start_session_log(&mut self, dir: &str) {
        let dir = expand_path(dir.trim());
        if !dir.is_dir() {
            self.message = Some(format!("not a directory: {}", dir.display()));
            return;
        }
        // Host from the pane's title (`user@host: cwd`), sanitized; a plain
        // "shell" when the shell set no title.
        let host = self
            .shell
            .active_title()
            .and_then(|t| host_from_title(&t))
            .unwrap_or_else(|| "shell".to_string());
        let name = format!("{}_{}.log", cian_core::timestamp_compact(), host);
        let path = dir.join(&name);
        match self.shell.active_session() {
            Some(s) => match s.start_log(&path) {
                Ok(()) => self.message = Some(format!("● logging to {}", path.display())),
                Err(e) => self.message = Some(format!("log failed: {}", e)),
            },
            None => self.message = Some("no shell here to log".into()),
        }
    }

    fn stop_session_log(&mut self) {
        match self.shell.active_session() {
            Some(s) if s.is_logging() => {
                let where_ = s.log_path().map(|p| p.display().to_string()).unwrap_or_default();
                s.stop_log();
                self.message = Some(format!("log saved: {}", where_));
            }
            _ => self.message = Some("this pane is not logging".into()),
        }
    }

    /// Any shell pane currently recording, across all tabs? Drives the pulsing
    /// border and the keep-repainting-while-logging tick.
    fn any_logging(&self) -> bool {
        self.shell.tabs.iter().any(|t| {
            t.nodes.iter().any(|n| {
                matches!(n, Some(Node::Leaf { session, .. }) if session.is_logging())
            })
        })
    }

    /// The file pane a file-oriented action should use: the focused one, or —
    /// when the shell has focus — the last file pane that did.
    fn effective_file_pane(&self) -> &Pane {
        let tabs = match self.focused {
            FocusedPane::Left => &self.left,
            FocusedPane::Right => &self.right,
            FocusedPane::Shell => match self.last_file_pane {
                FocusedPane::Right => &self.right,
                _ => &self.left,
            },
        };
        tabs.active_ref()
    }

    /// The last-focused file pane's directory, for seeding prompts.
    fn last_file_pane_cwd(&self) -> Option<PathBuf> {
        let tabs = match self.last_file_pane {
            FocusedPane::Right => &self.right,
            _ => &self.left,
        };
        Some(tabs.active_ref().cwd.clone())
    }

    /// Copy the viewer's selected lines (or the whole file when nothing is
    /// selected) to the clipboard.
    /// The vim-flavoured keymap for the F3 viewer: a cursor that moves with
    /// h/j/k/l and friends, and v / V / Ctrl-v visual selection with y/c to
    /// copy. The rendered body height (from `viewer_rect`) sizes the page moves
    /// and keeps the cursor on screen.
    fn handle_viewer_key(&mut self, key: KeyEvent) -> Result<()> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        // While typing a `/` search, keys build the query; Enter runs it.
        if matches!(self.popup, Popup::Viewer { find_input: Some(_), .. }) {
            match key.code {
                KeyCode::Esc => {
                    if let Popup::Viewer { find_input, .. } = &mut self.popup {
                        *find_input = None;
                    }
                }
                KeyCode::Enter => {
                    let q = if let Popup::Viewer { find_input, .. } = &mut self.popup {
                        find_input.take().unwrap_or_default()
                    } else {
                        String::new()
                    };
                    if !q.is_empty() {
                        if let Popup::Viewer { find_query, .. } = &mut self.popup {
                            *find_query = Some(q);
                        }
                        self.viewer_search_jump(true);
                    }
                }
                KeyCode::Backspace => {
                    if let Popup::Viewer { find_input: Some(s), .. } = &mut self.popup {
                        s.pop();
                    }
                }
                KeyCode::Char(c) if !ctrl => {
                    if let Popup::Viewer { find_input: Some(s), .. } = &mut self.popup {
                        s.push(c);
                    }
                }
                _ => {}
            }
            return Ok(());
        }

        // Shift+Enter re-decodes the same bytes under the next text encoding.
        // Shift+Enter reveals the viewed file in the pane: jump there, cursor
        // on it, and close the viewer.
        if key.code == KeyCode::Enter && shift {
            self.viewer_reveal_in_pane();
            return Ok(());
        }
        // `e` opens the encoding picker; the choice re-decodes this file.
        if !ctrl && key.code == KeyCode::Char('e') {
            let cur = if let Popup::Viewer { view, .. } = &self.popup {
                cian_core::viewer::TextEncoding::ALL
                    .iter()
                    .position(|enc| *enc == view.encoding)
                    .unwrap_or(0)
            } else {
                0
            };
            let viewer = std::mem::replace(&mut self.popup, Popup::None);
            self.popup = Popup::EncodingPicker {
                cursor: cur,
                target: EncTarget::Viewer(Box::new(viewer)),
            };
            return Ok(());
        }
        // y / c copy the selection (or the whole file when nothing is selected).
        if !ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('y')) {
            self.copy_viewer_selection();
            return Ok(());
        }
        // `/` opens the search prompt.
        if !ctrl && key.code == KeyCode::Char('/') {
            if let Popup::Viewer { find_input, .. } = &mut self.popup {
                *find_input = Some(String::new());
            }
            return Ok(());
        }
        // Ctrl+n / Ctrl+N step to the next / previous grep hit's preview,
        // without returning to the results list.
        if ctrl && matches!(key.code, KeyCode::Char('n') | KeyCode::Char('N')) {
            let forward = key.code == KeyCode::Char('n') && !shift;
            self.viewer_grep_step(forward);
            return Ok(());
        }
        // n / N jump to the next / previous match of the in-file search.
        if !ctrl && matches!(key.code, KeyCode::Char('n') | KeyCode::Char('N')) {
            let forward = key.code == KeyCode::Char('n');
            self.viewer_search_jump(forward);
            return Ok(());
        }
        // A numeric prefix builds a count for the next motion (vim's `42G`).
        if !ctrl {
            if let KeyCode::Char(c @ '0'..='9') = key.code {
                if let Popup::Viewer { count, .. } = &mut self.popup {
                    if c != '0' || count.is_some() {
                        let d = c as usize - '0' as usize;
                        *count = Some(count.unwrap_or(0).saturating_mul(10) + d);
                        return Ok(());
                    }
                }
            }
        }

        let body_h = (self.viewer_rect.height as usize).max(1);
        let half = (body_h / 2).max(1);
        let mut close = false;
        let mut summarize = false;
        if let Popup::Viewer { view, scroll, line, col, goal, visual, anchor, count, .. } = &mut self.popup {
            let cnt = count.take();
            let n = view.lines.len();
            let last = n.saturating_sub(1);
            // Move the cursor to a line, landing at the goal column (clamped).
            let to_line = |ln: usize, line: &mut usize, col: &mut usize, goal: usize| {
                *line = ln.min(last);
                let len = vlen(view, *line);
                *col = if goal == usize::MAX { len } else { goal.min(len) };
            };
            let start_visual = |mode: ViewVisual, visual: &mut Option<ViewVisual>, anchor: &mut (usize, usize), line: usize, col: usize| {
                if *visual == Some(mode) {
                    *visual = None; // pressing the same key again leaves visual
                } else {
                    if visual.is_none() {
                        *anchor = (line, col);
                    }
                    *visual = Some(mode);
                }
            };

            // Modifier+arrow selects like an editor and the motion arm below
            // extends it: Alt+arrow is block-wise (a rectangle), Shift+arrow is
            // character-wise. Both begin at the cursor; a plain arrow keeps vim's
            // behaviour. Home/End/PageUp/Dn extend the same way.
            let is_arrow = matches!(
                key.code,
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
                    | KeyCode::Home | KeyCode::End | KeyCode::PageUp | KeyCode::PageDown
            );
            if is_arrow {
                if alt {
                    if visual.is_none() {
                        *anchor = (*line, *col);
                    }
                    *visual = Some(ViewVisual::Block);
                } else if shift && visual.is_none() {
                    *anchor = (*line, *col);
                    *visual = Some(ViewVisual::Char);
                }
            }

            match (ctrl, key.code) {
                (false, KeyCode::Esc) | (false, KeyCode::Char('q')) => {
                    // Esc first drops out of visual mode, then closes.
                    if visual.is_some() {
                        *visual = None;
                    } else {
                        close = true;
                    }
                }
                (false, KeyCode::Char('v')) => start_visual(ViewVisual::Char, visual, anchor, *line, *col),
                (false, KeyCode::Char('V')) => start_visual(ViewVisual::Line, visual, anchor, *line, *col),
                (true, KeyCode::Char('v')) => start_visual(ViewVisual::Block, visual, anchor, *line, *col),
                // `S` summarises the file with the AI (sends its text). Handled
                // after the borrow ends, below.
                (false, KeyCode::Char('S')) => summarize = true,
                (false, KeyCode::Char('o')) if visual.is_some() => {
                    // Swap the cursor and the anchor.
                    let a = *anchor;
                    *anchor = (*line, *col);
                    *line = a.0;
                    *col = a.1;
                    *goal = *col;
                }

                // Vertical motion keeps the goal column.
                (false, KeyCode::Char('j')) | (_, KeyCode::Down) => to_line(*line + 1, line, col, *goal),
                (false, KeyCode::Char('k')) | (_, KeyCode::Up) => to_line(line.saturating_sub(1), line, col, *goal),
                (false, KeyCode::Char('d')) | (true, KeyCode::Char('d')) | (_, KeyCode::PageDown) => {
                    to_line(*line + half, line, col, *goal)
                }
                (false, KeyCode::Char('u')) | (true, KeyCode::Char('u')) | (_, KeyCode::PageUp) => {
                    to_line(line.saturating_sub(half), line, col, *goal)
                }
                (true, KeyCode::Char('f')) => to_line(*line + body_h, line, col, *goal),
                (true, KeyCode::Char('b')) => to_line(line.saturating_sub(body_h), line, col, *goal),
                (false, KeyCode::Char('g')) => to_line(0, line, col, *goal),
                // `G` goes to the bottom, or to line N when a count was typed.
                (false, KeyCode::Char('G')) => {
                    let target = cnt.map(|c| c.saturating_sub(1)).unwrap_or(last);
                    to_line(target, line, col, *goal);
                }
                // `%` jumps to the matching bracket.
                (false, KeyCode::Char('%')) => {
                    if let Some((nl, nc)) = viewer_match_bracket(view, *line, *col) {
                        *line = nl;
                        *col = nc;
                        *goal = nc;
                    }
                }
                // `{` / `}` jump between paragraph (blank-line) boundaries.
                (false, KeyCode::Char('{')) => {
                    *line = viewer_paragraph(view, *line, false);
                    *col = 0;
                    *goal = 0;
                }
                (false, KeyCode::Char('}')) => {
                    *line = viewer_paragraph(view, *line, true);
                    *col = 0;
                    *goal = 0;
                }

                // Horizontal motion resets the goal to the real column.
                (false, KeyCode::Char('h')) | (_, KeyCode::Left) => {
                    *col = col.saturating_sub(1);
                    *goal = *col;
                }
                (false, KeyCode::Char('l')) | (_, KeyCode::Right) => {
                    let len = vlen(view, *line);
                    if *col < len {
                        *col += 1;
                    }
                    *goal = *col;
                }
                (false, KeyCode::Char('0')) | (_, KeyCode::Home) => {
                    *col = 0;
                    *goal = 0;
                }
                (false, KeyCode::Char('$')) | (_, KeyCode::End) => {
                    *col = vlen(view, *line);
                    *goal = usize::MAX;
                }
                (false, KeyCode::Char('w')) => {
                    let (nl, nc) = viewer_word_forward(view, *line, *col, last);
                    *line = nl;
                    *col = nc;
                    *goal = *col;
                }
                (false, KeyCode::Char('b')) => {
                    let (nl, nc) = viewer_word_back(view, *line, *col);
                    *line = nl;
                    *col = nc;
                    *goal = *col;
                }
                _ => {}
            }

            // Keep the cursor on screen.
            *line = (*line).min(last);
            if *line < *scroll {
                *scroll = *line;
            } else if *line >= *scroll + body_h {
                *scroll = *line + 1 - body_h;
            }
            *scroll = (*scroll).min(n.saturating_sub(body_h));
        }
        if summarize {
            self.summarize_viewer();
            return Ok(());
        }
        if close {
            // If this viewer was opened from a grep hit, go back to the results
            // list so the next hit is one keystroke away; otherwise just close.
            match self.find_return.take() {
                Some(back) => self.popup = *back,
                None => self.popup = Popup::None,
            }
        }
        Ok(())
    }

    /// Jump the viewer cursor to the next/previous match of the active search,
    /// scrolling it into view. Wraps around the file.
    fn viewer_search_jump(&mut self, forward: bool) {
        let body_h = (self.viewer_rect.height as usize).max(1);
        let mut no_query = false;
        let mut not_found = false;
        if let Popup::Viewer { view, scroll, line, col, goal, find_query, .. } = &mut self.popup {
            match find_query.clone() {
                None => no_query = true,
                Some(q) => match viewer_find(view, (*line, *col), &q, forward) {
                    Some((nl, nc)) => {
                        *line = nl;
                        *col = nc;
                        *goal = nc;
                        let n = view.lines.len();
                        if *line < *scroll {
                            *scroll = *line;
                        } else if *line >= *scroll + body_h {
                            *scroll = *line + 1 - body_h;
                        }
                        *scroll = (*scroll).min(n.saturating_sub(body_h));
                    }
                    None => not_found = true,
                },
            }
        }
        if no_query {
            self.message = Some("no search — press / first".into());
        } else if not_found {
            self.message = Some("no match".into());
        }
    }

    /// Apply (or cancel, with `None`) an encoding-picker choice to whatever it
    /// targeted, then close it — restoring a stashed viewer when it came from F3.
    fn finish_encoding_pick(&mut self, chosen: Option<cian_core::viewer::TextEncoding>) {
        let target = match std::mem::replace(&mut self.popup, Popup::None) {
            Popup::EncodingPicker { target, .. } => target,
            other => {
                self.popup = other;
                return;
            }
        };
        match target {
            EncTarget::Shell => {
                if let Some(enc) = chosen {
                    if let Some(s) = self.shell.active_session() {
                        s.set_encoding(enc);
                        self.message = Some(format!("shell encoding: {}", enc.label()));
                    }
                }
            }
            EncTarget::Viewer(mut viewer) => {
                if let Some(enc) = chosen {
                    if let Popup::Viewer { view, visual, .. } = viewer.as_mut() {
                        view.redecode(enc);
                        *visual = None;
                        self.message = Some(format!("encoding: {}", enc.label()));
                    }
                }
                self.popup = *viewer;
            }
        }
    }

    /// Shift+Enter in the viewer: close it and move the active pane to the
    /// viewed file's directory, cursor on the file.
    fn viewer_reveal_in_pane(&mut self) {
        let path = if let Popup::Viewer { path, .. } = &self.popup {
            path.clone()
        } else {
            return;
        };
        let Some(dir) = path.parent().map(|p| p.to_path_buf()) else { return };
        self.popup = Popup::None;
        self.find_return = None;
        self.stop_find();
        if let Some(p) = self.active_pane_mut() {
            if p.jump_to(dir).is_ok() {
                if let Some(i) = p.entries.iter().position(|e| e.path == path) {
                    p.cursor = i;
                }
            }
        }
        self.message = Some(format!("→ {}", path.display()));
    }

    /// Ctrl+n / Ctrl+N in the viewer: preview the next/previous grep hit
    /// directly, keeping the stashed results in step. A no-op unless the viewer
    /// was opened from a grep result.
    fn viewer_grep_step(&mut self, forward: bool) {
        let hit = {
            let Some(back) = self.find_return.as_mut() else {
                self.message = Some("not viewing a grep hit".into());
                return;
            };
            let Popup::FindResults { hits, cursor, .. } = back.as_mut() else { return };
            let n = hits.len();
            if n == 0 {
                return;
            }
            // Step to the next/previous hit that has a line (a content match).
            let mut idx = *cursor;
            let mut found = None;
            for _ in 0..n {
                idx = if forward { (idx + 1) % n } else { (idx + n - 1) % n };
                if hits[idx].line.is_some() {
                    found = Some(idx);
                    break;
                }
            }
            match found {
                Some(i) => {
                    *cursor = i;
                    hits[i].clone()
                }
                None => return,
            }
        };
        if let Some((lineno, _)) = &hit.line {
            let name = hit
                .path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| hit.rel.display().to_string());
            // open_viewer_at replaces the popup but leaves `find_return` intact.
            self.open_viewer_at(&hit.path, &name, lineno.saturating_sub(1));
            self.message = Some(format!("{}  (Ctrl+n/N next/prev · Esc list)", hit.rel.display()));
        }
    }

    fn copy_viewer_selection(&mut self) {
        let text = if let Popup::Viewer { view, line, col, visual, anchor, .. } = &self.popup {
            let lines = &view.lines;
            let n = lines.len();
            if n == 0 {
                String::new()
            } else {
                match visual {
                    // No selection: copy the whole file, as before.
                    None => lines.join("\n"),
                    Some(ViewVisual::Line) => {
                        let (a, b) = (anchor.0.min(*line), anchor.0.max(*line).min(n - 1));
                        lines[a..=b].join("\n")
                    }
                    Some(ViewVisual::Char) => {
                        // Order the two endpoints, then take an inclusive
                        // char-wise span across the lines between them.
                        let (s, e) = order_pos((anchor.0, anchor.1), (*line, *col));
                        viewer_charwise(lines, s, e)
                    }
                    Some(ViewVisual::Block) => {
                        let (l0, l1) = (anchor.0.min(*line), anchor.0.max(*line).min(n - 1));
                        let (c0, c1) = (anchor.1.min(*col), anchor.1.max(*col));
                        (l0..=l1)
                            .map(|l| {
                                let chars: Vec<char> = lines[l].chars().collect();
                                let hi = (c1 + 1).min(chars.len());
                                if c0 >= hi {
                                    String::new()
                                } else {
                                    chars[c0..hi].iter().collect()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                }
            }
        } else {
            return;
        };
        if text.is_empty() {
            self.message = Some("nothing to copy".into());
            return;
        }
        if let Some(cb) = self.clipboard.as_mut() {
            let _ = cb.set_text(text);
        }
        self.message = Some("copied".into());
        // A copy ends the visual gesture; leave the viewer open.
        if let Popup::Viewer { visual, .. } = &mut self.popup {
            *visual = None;
        }
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
            path: Vec::new(),
        };
    }

    /// Re-open the shortcuts popup at `path`/`cursor` from the saved store (used
    /// after an add/edit/delete so the view reflects the change).
    fn reopen_shortcuts(&mut self, path: Vec<usize>, cursor: usize) {
        let n = sc_level(&self.shortcuts.entries, &path).len();
        self.popup = Popup::Shortcuts {
            entries: self.shortcuts.entries.clone(),
            cursor: cursor.min(n.saturating_sub(1)),
            path,
        };
    }

    /// Prompt for a new shortcut's name in the group at `path`. `group` makes a
    /// folder (name only, no target step).
    fn start_shortcut_add(&mut self, path: Vec<usize>, group: bool) {
        let title = if group { "new folder — name" } else { "new shortcut — name" };
        self.popup = text_input(
            title,
            "name:",
            String::new(),
            InputKind::ShortcutName { path, edit_idx: None, group },
        );
    }

    fn start_shortcut_edit(&mut self, path: Vec<usize>, idx: usize) {
        let Some(s) = sc_level(&self.shortcuts.entries, &path).get(idx).cloned() else { return };
        let group = s.is_group();
        self.popup = text_input(
            "edit shortcut — name",
            "name:",
            s.name,
            InputKind::ShortcutName { path, edit_idx: Some(idx), group },
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

    fn copy_shortcut_target_to_clipboard(&mut self, path: &[usize], idx: usize) {
        let Some(entry) = sc_level(&self.shortcuts.entries, path).get(idx).cloned() else { return };
        let target = entry.target_str().to_string();
        let Some(cb) = self.clipboard.as_mut() else {
            self.message = Some("clipboard unavailable".into());
            return;
        };
        match cb.set_text(target.clone()) {
            Ok(()) => self.message = Some(format!("◂ copied: {}", truncate(&target, 50))),
            Err(e) => self.message = Some(format!("clipboard error: {}", e)),
        }
    }

    fn execute_shortcut(&mut self, path: &[usize], idx: usize) -> Result<()> {
        let Some(entry) = sc_level(&self.shortcuts.entries, path).get(idx).cloned() else { return Ok(()) };
        // Groups are descended in the key handler, not executed.
        if entry.is_group() {
            return Ok(());
        }
        let target = entry.target_str().to_string();

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

    /// Begin an SFTP transfer: capture the local side, then reuse the SSH
    /// host/user picker to choose the server. `ssh_pick` routes back here once
    /// a user is chosen because [`App::scp_dir`] is set.
    fn start_scp(&mut self, dir: ScpDir) {
        if self.config.ssh_hosts.is_empty() {
            self.start_ssh(); // shows the "configure a host" notice
            return;
        }
        // Works from the shell too, acting on the last-focused file pane.
        let pane = self.effective_file_pane();
        let (locals, local_dir) = match dir {
            ScpDir::Upload => {
                let files: Vec<PathBuf> =
                    pane.target_paths().into_iter().filter(|p| p.is_file()).collect();
                if files.is_empty() {
                    self.message = Some("select a file to upload".into());
                    return;
                }
                (files, PathBuf::new())
            }
            ScpDir::Download => (Vec::new(), pane.cwd.clone()),
        };
        self.scp_dir = Some((dir, locals, local_dir));
        self.popup = Popup::SshHosts { cursor: 0, filter: String::new() };
    }

    /// After a host+user is picked for a transfer, resolve the connection and
    /// ask for the remote path.
    fn scp_after_pick(&mut self, host_idx: usize, user: &str) {
        let Some((dir, locals, local_dir)) = self.scp_dir.take() else { return };
        let Some(h) = self.config.ssh_hosts.get(host_idx) else { return };
        let Some(u) = h.users.iter().find(|u| u.name == user) else { return };
        let Some(password) = u.secret() else {
            self.message = Some(format!(
                "no password set for {}@{} — a transfer needs one in init.lua",
                u.name, h.name
            ));
            return;
        };
        let target = cian_scp::Target {
            host: h.host.clone(),
            port: h.port.unwrap_or(22),
            user: u.name.clone(),
            password,
        };
        let label = format!("{}@{}", u.name, h.name);
        let (title, prompt, seed) = match dir {
            ScpDir::Upload => (
                "SFTP upload — remote folder",
                "remote directory to upload into:",
                String::new(),
            ),
            ScpDir::Download => (
                "SFTP download — remote file",
                "remote file path to download:",
                String::new(),
            ),
        };
        self.scp_pending = Some(ScpPending { target, label, dir, locals, local_dir });
        self.popup = text_input(title, prompt, seed, InputKind::ScpRemote);
    }

    /// Run the pending transfer against `remote` (a directory for upload, a file
    /// for download), on a worker thread with the shared progress popup.
    fn start_scp_transfer(&mut self, remote: &str) {
        let Some(p) = self.scp_pending.take() else { return };
        let remote = remote.trim().to_string();
        if remote.is_empty() {
            self.message = Some("cancelled (no remote path)".into());
            return;
        }
        let ScpPending { target, label, dir, locals, local_dir } = p;
        let verb = if dir == ScpDir::Upload { "uploading" } else { "downloading" };
        self.message = Some(format!("{} {} …", verb, label));
        self.start_op(if dir == ScpDir::Upload { "uploading" } else { "downloading" }, move |ctl| {
            let mut report = OpReport::default();
            // Bridge cian-scp's byte progress into the shared op progress.
            let cancel = ctl.cancel;
            match dir {
                ScpDir::Upload => {
                    let total = locals.len();
                    for (i, local) in locals.iter().enumerate() {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let fname = local.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                        let dest = format!("{}/{}", remote.trim_end_matches('/'), fname);
                        let cur = fname.clone();
                        let mut fwd = |done: u64, tot: u64| {
                            (ctl.on_progress)(&cian_core::progress::Progress {
                                bytes_done: done,
                                bytes_total: tot,
                                files_done: i,
                                files_total: total,
                                current: cur.clone(),
                            });
                        };
                        let mut sctl = cian_scp::Ctl { cancel, on_progress: &mut fwd };
                        match cian_scp::upload(&target, local, &dest, &mut sctl) {
                            Ok(via) => {
                                report.ok += 1;
                                report.note = Some(format!("via {}", via.label()));
                            }
                            Err(e) => report.note_error(format!("{}: {}", fname, e)),
                        }
                    }
                }
                ScpDir::Download => {
                    let fname = remote.rsplit('/').next().unwrap_or("download").to_string();
                    let dest = local_dir.join(&fname);
                    let cur = fname.clone();
                    let mut fwd = |done: u64, tot: u64| {
                        (ctl.on_progress)(&cian_core::progress::Progress {
                            bytes_done: done,
                            bytes_total: tot,
                            files_done: 0,
                            files_total: 1,
                            current: cur.clone(),
                        });
                    };
                    let mut sctl = cian_scp::Ctl { cancel, on_progress: &mut fwd };
                    match cian_scp::download(&target, &remote, &dest, &mut sctl) {
                        Ok(via) => {
                            report.ok += 1;
                            report.note = Some(format!("via {}", via.label()));
                        }
                        Err(e) => report.note_error(format!("{}: {}", fname, e)),
                    }
                }
            }
            report
        });
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

    /// Type an AI-suggested command at the shell prompt WITHOUT running it —
    /// the user reviews it and presses Enter. Focuses the shell.
    fn insert_ai_command_at_prompt(&mut self, cmd: &str) {
        let cwd = self.shell_cwd();
        self.shell.ensure(&cwd);
        self.focus(FocusedPane::Shell);
        match self.shell.active_session_mut() {
            Some(s) => s.write_input(cmd.as_bytes()),
            None => self.pending_shell_input = Some(cmd.to_string()),
        }
        self.message = Some("command at prompt — review and press Enter".into());
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
        self.open_viewer_at(&entry.path, &entry.name, 0);
    }

    /// Open the F3 viewer on `path`, with the cursor on `line0` (0-based). Used
    /// by F3 (line 0) and by "open a grep hit at its line".
    fn open_viewer_at(&mut self, path: &Path, title: &str, line0: usize) {
        match cian_core::viewer::view_file(path) {
            Ok(view) => {
                // The git change gutter: which lines differ from HEAD. Best
                // effort — empty when the file is not in a repo or is unchanged.
                let git_lines = path
                    .parent()
                    .and_then(|dir| cian_core::git::line_changes(dir, path))
                    .unwrap_or_default();
                let last = view.lines.len().saturating_sub(1);
                let line = line0.min(last);
                self.popup = Popup::Viewer {
                    title: title.to_string(),
                    path: path.to_path_buf(),
                    view,
                    scroll: line.saturating_sub(4), // show a little context above
                    line,
                    col: 0,
                    goal: 0,
                    visual: None,
                    anchor: (0, 0),
                    find_input: None,
                    find_query: None,
                    count: None,
                    git_lines,
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
        // The `..` row is never a comparison subject; treat it as no selection.
        let pick = |t: &PaneTabs| t.active_ref().selected().filter(|e| !e.is_parent).cloned();
        let (Some(a), Some(b)) = (pick(&self.left), pick(&self.right)) else {
            self.message = Some("select a file (or a folder) in each pane to compare".into());
            return;
        };
        // Two directories: a recursive tree comparison. Two files: a line diff.
        if a.is_dir && b.is_dir {
            self.start_dir_compare(a.path.clone(), b.path.clone(), a.name.clone(), b.name.clone());
            return;
        }
        if a.is_dir || b.is_dir {
            self.message = Some("compare two files, or two folders — not one of each".into());
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

    /// Compare two directory trees on a worker thread, showing the differing
    /// paths when it finishes. Esc cancels a long walk.
    fn start_dir_compare(&mut self, left: PathBuf, right: PathBuf, ln: String, rn: String) {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        let worker_cancel = Arc::clone(&cancel);
        let (l, r) = (left.clone(), right.clone());
        std::thread::spawn(move || {
            // Rate-limit the ticks: the core already reports every 16 entries,
            // but a huge tree still produces plenty; forward at most ~30/s.
            let mut last = Instant::now() - Duration::from_secs(1);
            let mut on_progress = |p: &cian_core::progress::Progress| {
                if last.elapsed() >= Duration::from_millis(33) {
                    last = Instant::now();
                    let _ = tx.send(DiffMsg::Tick(p.clone()));
                }
            };
            let diff = cian_core::dirdiff::compare(&l, &r, &worker_cancel, &mut on_progress);
            let _ = tx.send(DiffMsg::Done(diff));
        });
        self.diff_job = Some(DiffJob {
            rx,
            cancel,
            left_root: left,
            right_root: right,
            left: ln,
            right: rn,
            latest: cian_core::progress::Progress::default(),
            label: "comparing folders",
            started: Instant::now(),
        });
    }

    /// Drain progress and install the result when the worker finishes.
    fn poll_diff_job(&mut self) -> bool {
        let Some(job) = &mut self.diff_job else { return false };
        let mut done = None;
        let mut changed = false;
        loop {
            match job.rx.try_recv() {
                Ok(DiffMsg::Tick(p)) => {
                    job.latest = p;
                    changed = true;
                }
                Ok(DiffMsg::Done(d)) => {
                    done = Some(d);
                    changed = true;
                    break;
                }
                Err(_) => break,
            }
        }
        let Some(diff) = done else { return changed };
        let job = self.diff_job.take().unwrap();
        if diff.cancelled {
            self.message = Some("comparison cancelled".into());
            return true;
        }
        if diff.is_identical() {
            self.message = Some(format!("{} and {} match — no differences", job.left, job.right));
            return true;
        }
        self.popup = Popup::DirCompare {
            left: job.left,
            right: job.right,
            left_root: job.left_root,
            right_root: job.right_root,
            entries: diff.entries,
            cursor: 0,
            scroll: 0,
            truncated: diff.truncated,
        };
        true
    }

    /// Jump both panes to the highlighted diff entry (whichever side has it),
    /// putting the cursor on it, and close the comparison.
    fn dir_compare_goto(&mut self) {
        let Popup::DirCompare { entries, cursor, left_root, right_root, .. } = &self.popup else {
            return;
        };
        let Some(e) = entries.get(*cursor) else { return };
        use cian_core::dirdiff::Status;
        let rel = e.rel.clone();
        let (status, lr, rr) = (e.status, left_root.clone(), right_root.clone());
        self.popup = Popup::None;
        let go = |pane: &mut PaneTabs, root: &Path, rel: &Path| {
            let full = root.join(rel);
            let dir = if full.is_dir() { full.clone() } else { full.parent().map(|p| p.to_path_buf()).unwrap_or(full.clone()) };
            let p = pane.active_mut();
            if p.jump_to(dir).is_ok() {
                if let Some(i) = p.entries.iter().position(|x| x.path == full) {
                    p.cursor = i;
                }
            }
        };
        if status != Status::OnlyRight {
            go(&mut self.left, &lr, &rel);
        }
        if status != Status::OnlyLeft {
            go(&mut self.right, &rr, &rel);
        }
        self.message = Some(format!("→ {}", rel.display()));
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
        self.popup = Popup::Notice { lines: self.attributes_lines(&paths, 40) };
    }

    /// Build the Attributes listing (permissions, size, owner) for `paths`,
    /// capped at `limit` rows. Shared by the Attributes menu/`:attr` and `:ls`.
    fn attributes_lines(&self, paths: &[PathBuf], limit: usize) -> Vec<String> {
        let ja = self.lang == Lang::Ja;
        let mut lines = Vec::new();
        for path in paths.iter().take(limit) {
            let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match cian_core::attrs::read_attrs(path) {
                Ok(a) => {
                    // A folder is labelled as such; a file shows its byte size,
                    // right-aligned so the sizes form a readable column.
                    let size = if a.is_dir {
                        format!("{:>10}", tr(self.lang, "<dir>", "<フォルダ>"))
                    } else {
                        format!("{:>10}", cian_core::human_size(a.size.unwrap_or(0)))
                    };
                    let owner = a.owner.as_ref().map(|o| format!("  owner {}", o)).unwrap_or_default();
                    lines.push(format!("{:<28} {}  {}{}", truncate(&name, 28), a.describe(), size, owner));
                }
                Err(e) => lines.push(format!("{:<28} {}", truncate(&name, 28), e)),
            }
        }
        if paths.len() > limit {
            lines.push(if ja {
                format!("... 他 {} 件", paths.len() - limit)
            } else {
                format!("... and {} more", paths.len() - limit)
            });
        }
        lines.push(String::new());
        lines.push(tr(self.lang,
            "change with  :chmod 644   or  :readonly on|off",
            "変更:  :chmod 644   または  :readonly on|off").to_string());
        lines
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
        self.find_return = None; // a fresh search invalidates any stashed list
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

        // A grep hit (content match) opens the viewer right on the matched
        // line — the whole reason you grepped. The results list is stashed so
        // Esc from the viewer returns to it, for scanning hit after hit. A name
        // match just navigates to the file.
        if let Some((lineno, _)) = &hit.line {
            let results = std::mem::replace(&mut self.popup, Popup::None);
            self.find_return = Some(Box::new(results));
            self.stop_find(); // freeze the list; the stash already holds the hits
            let name = hit
                .path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| hit.rel.display().to_string());
            self.open_viewer_at(&hit.path, &name, lineno.saturating_sub(1));
            self.message = Some("Esc → back to results".into());
            return Ok(());
        }
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
        self.popup = Popup::Manual { lines: manual_lines(&self.keymap, self.lang), scroll: 0 };
    }

    // ------- AI -------

    /// Re-read `init.lua` and apply everything that can change without a
    /// restart: keymaps, options, SSH hosts and open handlers. The colour theme
    /// and border style are installed once at startup (into set-once globals),
    /// so a change to those is reported as needing a restart rather than being
    /// silently ignored.
    fn reload_config(&mut self) {
        let config = cian_lua::load();

        // Rebuild the user keymap, validating action names as at startup.
        let mut keymap: HashMap<char, Action> = HashMap::new();
        let mut problems: Vec<String> = config.errors.clone();
        for (c, name) in &config.keymaps {
            match action_from_name(name) {
                Some(a) => {
                    keymap.insert(*c, a);
                }
                None => problems.push(format!("keymap: unknown action {:?} (key '{}')", name, c)),
            }
        }
        self.keymap = keymap;

        // Live-applicable options.
        self.lang = Lang::from_opt(config.options.lang.as_deref());
        self.show_key_hints = config.options.key_hints.unwrap_or(true);
        self.clipboard_on_copy = config.options.clipboard_on_copy.unwrap_or(true);
        self.anim_dur =
            Duration::from_millis(config.options.animation_ms.unwrap_or(DEFAULT_ANIM_MS));
        let show_hidden = config.options.show_hidden.unwrap_or(true);
        for tabs in [&mut self.left, &mut self.right] {
            for pane in tabs.all_mut() {
                pane.set_show_hidden(show_hidden);
            }
        }

        // Theme and borders live in set-once globals; note if the file now asks
        // for something different, since we cannot swap them in place.
        let (resolved, theme_errors) = resolve_theme(&config.theme);
        problems.extend(theme_errors);
        let theme_changed = resolved != *theme();
        let borders_changed =
            resolve_border_type(config.options.borders.as_deref()) != border_type();

        // ssh hosts and on_open handlers come along with the replaced config.
        self.config = config;

        if !problems.is_empty() {
            let mut lines = vec!["reloaded with issues:".to_string(), String::new()];
            let total = problems.len();
            lines.extend(problems.into_iter().take(10));
            if total > 10 {
                lines.push(format!("... and {} more", total - 10));
            }
            self.popup = Popup::Notice { lines };
        } else if theme_changed || borders_changed {
            self.message = Some("config reloaded — restart to apply theme/border changes".into());
        } else {
            self.message = Some("config reloaded".into());
        }
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
            let label = self.op_job.as_ref().map(|j| j.label).unwrap_or("");
            self.op_job = None;
            self.reload_both();
            if let Some(p) = self.active_pane_mut() {
                p.clear_marks();
            }
            self.flash(self.focused);
            // A permission failure on a copy/move is the one case with a real
            // way out on Windows: offer to redo it with administrator rights.
            // On other platforms just fall through to the friendlier report.
            let elevate = report.permission_denied
                && cfg!(windows)
                && self.pending_elevation.is_some();
            if !report.permission_denied {
                self.pending_elevation = None;
            }
            if cancelled == Some(true) {
                self.pending_elevation = None;
                self.message = Some(format!(
                    "cancelled — {} done before stopping",
                    report.ok
                ));
            } else if elevate {
                let (op, targets, dest) = self.pending_elevation.take().unwrap();
                self.popup = Popup::ConfirmElevate { op, targets, dest };
            } else {
                self.show_op_report(&report);
                // A checksum is worth pasting into a verify field, so put the
                // digest(s) straight onto the clipboard when hashing finishes.
                if label == "hashing" {
                    let sums: Vec<String> = report
                        .errors
                        .iter()
                        .filter_map(|l| l.split_whitespace().nth(1).map(str::to_string))
                        .collect();
                    if !sums.is_empty() {
                        if let Some(cb) = self.clipboard.as_mut() {
                            let _ = cb.set_text(sums.join("\n"));
                        }
                        if let Popup::Notice { lines } = &mut self.popup {
                            lines.push(String::new());
                            lines.push("→ copied to the clipboard".to_string());
                        }
                    }
                }
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
        // Remembered so a permission failure can offer an elevated retry; the
        // op-completion handler clears this unless it actually hit that wall.
        self.pending_elevation = Some((op, targets.clone(), dest.clone()));
        self.start_op(label, move |ctl| match op {
            PendingOp::Copy => cian_core::progress::copy_many(&targets, &dest, conflict, ctl),
            PendingOp::Move => cian_core::progress::move_many(&targets, &dest, conflict, ctl),
        });
        Ok(())
    }

    /// Redo the remembered copy/move with administrator rights (Windows UAC).
    /// The elevated process runs the transfer itself, so there is no in-app
    /// progress — cian just waits on the worker and reports the outcome.
    fn run_elevated_transfer(&mut self) {
        let popup = std::mem::replace(&mut self.popup, Popup::None);
        let Popup::ConfirmElevate { op, targets, dest } = popup else { return };
        let move_after = op == PendingOp::Move;
        let n = targets.len();
        let items: Vec<cian_core::elevate::CopyItem> = targets
            .into_iter()
            .map(|src| cian_core::elevate::CopyItem { src, dest_dir: dest.clone() })
            .collect();
        self.message = Some("waiting for the administrator prompt…".into());
        self.start_op("elevating", move |_ctl| {
            let mut report = OpReport::default();
            match cian_core::elevate::elevated_copy(&items, move_after) {
                Ok(()) => {
                    report.ok = n;
                    report.note = Some("as administrator".into());
                }
                Err(e) => report.note_error(e.to_string()),
            }
            report
        });
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
            // Turn the raw "Access is denied (os error 5)" into something that
            // says what to do about it.
            if report.permission_denied {
                lines.push(String::new());
                lines.push("Permission denied — this location needs administrator rights.".into());
                if cfg!(windows) {
                    lines.push("Run cian as administrator, or copy to a writable folder.".into());
                } else {
                    lines.push("Copy to a folder you can write to, or fix its permissions.".into());
                }
                lines.push(String::new());
            }
            lines.extend(report.errors.iter().take(8).cloned());
            if report.errors.len() > 8 {
                lines.push(format!("... and {} more", report.errors.len() - 8));
            }
            self.popup = Popup::Notice { lines };
        } else {
            let mut msg = format!("done — {} ok · {} skipped", report.ok, report.skipped);
            if let Some(note) = &report.note {
                msg.push_str(&format!(" ({})", note));
            }
            self.message = Some(msg);
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
            InputKind::AiShellCmd => {
                self.start_ai_shell_cmd(&name);
                return Ok(());
            }
            InputKind::AiRename => {
                self.start_ai_rename(&name);
                return Ok(());
            }
            InputKind::AiSearch => {
                self.start_ai_search(&name);
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
            InputKind::LogDir => {
                self.start_session_log(&name);
                return Ok(());
            }
            InputKind::ScpRemote => {
                self.start_scp_transfer(&name);
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
            InputKind::ShortcutName { path, edit_idx, group } => {
                if *group {
                    // A folder needs no target: create/rename it and reopen.
                    let p = path.clone();
                    if let Some(lvl) = sc_level_mut(&mut self.shortcuts.entries, &p) {
                        match edit_idx {
                            Some(i) if *i < lvl.len() => lvl[*i].name = name,
                            _ => lvl.push(Shortcut::group(name)),
                        }
                    }
                    let cursor = edit_idx.unwrap_or(sc_level(&self.shortcuts.entries, &p).len().saturating_sub(1));
                    let _ = self.shortcuts.save();
                    self.reopen_shortcuts(p, cursor);
                    return Ok(());
                }
                // A leaf chains into the target step. New shortcuts default to a
                // target picked elsewhere (history) or the entry under the cursor.
                let here = self
                    .pending_shortcut_target
                    .take()
                    .or_else(|| {
                        self.active_pane()
                            .and_then(|p| p.selected().map(|e| e.path.display().to_string()))
                    })
                    .unwrap_or_default();
                let prev_target = edit_idx
                    .and_then(|i| sc_level(&self.shortcuts.entries, path).get(i).map(|s| s.target_str().to_string()))
                    .filter(|t| !t.is_empty())
                    .unwrap_or(here);
                self.popup = text_input(
                    "shortcut — target",
                    "URL / path / app   (Ctrl+V paste, Ctrl+U clear):",
                    prev_target,
                    InputKind::ShortcutTarget { path: path.clone(), edit_idx: *edit_idx, name },
                );
                return Ok(());
            }
            InputKind::ShortcutTarget { path, edit_idx, name: stored_name } => {
                let target = name; // `name` here is actually the trimmed buffer
                if target.is_empty() {
                    self.message = Some("cancelled (empty target)".into());
                    return Ok(());
                }
                let entry = Shortcut::leaf(stored_name.clone(), target);
                let p = path.clone();
                let cursor = if let Some(lvl) = sc_level_mut(&mut self.shortcuts.entries, &p) {
                    match edit_idx {
                        Some(i) if *i < lvl.len() => {
                            lvl[*i] = entry;
                            *i
                        }
                        _ => {
                            lvl.push(entry);
                            lvl.len() - 1
                        }
                    }
                } else {
                    0
                };
                match self.shortcuts.save() {
                    Ok(()) => {
                        self.message = Some("shortcut saved".into());
                        self.reopen_shortcuts(p, cursor);
                    }
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

        // In the viewer: a click places the cursor on that line, a drag selects
        // whole lines (line-wise visual), the wheel scrolls, and right-click
        // copies. Handled before the blanket popup guard below.
        if matches!(self.popup, Popup::Viewer { .. }) {
            let body = self.viewer_rect;
            let body_h = (body.height as usize).max(1);
            // The clicked column, offset past the line-number gutter, so a click
            // lands on the character under the pointer (not just its line).
            let text_x = body.x + self.viewer_gutter;
            let ecol = col;
            let line_at = |row: u16, scroll: usize, n: usize| -> usize {
                let rel = row.saturating_sub(body.y) as usize;
                (scroll + rel).min(n.saturating_sub(1))
            };
            let col_at = |view: &cian_core::viewer::View, l: usize| -> usize {
                let rel = ecol.saturating_sub(text_x) as usize;
                rel.min(vlen(view, l))
            };
            match ev.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Popup::Viewer { view, scroll, line, col, goal, visual, anchor, .. } =
                        &mut self.popup
                    {
                        let l = line_at(row, *scroll, view.lines.len());
                        let c = col_at(view, l);
                        *line = l;
                        *col = c;
                        *goal = c;
                        *anchor = (l, c);
                        *visual = None; // a bare click just moves the cursor
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    // Holding Alt while dragging makes a block (rectangular)
                    // selection; otherwise it is character-wise.
                    let mode = if ev.modifiers.contains(KeyModifiers::ALT) {
                        ViewVisual::Block
                    } else {
                        ViewVisual::Char
                    };
                    if let Popup::Viewer { view, scroll, line, col, goal, visual, .. } = &mut self.popup {
                        let l = line_at(row, *scroll, view.lines.len());
                        let c = col_at(view, l);
                        *line = l;
                        *col = c;
                        *goal = c;
                        *visual = Some(mode);
                    }
                }
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                    if let Popup::Viewer { view, scroll, line, col, goal, .. } = &mut self.popup {
                        let n = view.lines.len();
                        let last = n.saturating_sub(1);
                        if matches!(ev.kind, MouseEventKind::ScrollDown) {
                            *line = (*line + 3).min(last);
                        } else {
                            *line = line.saturating_sub(3);
                        }
                        *col = (*goal).min(vlen(view, *line));
                        if *line < *scroll {
                            *scroll = *line;
                        } else if *line >= *scroll + body_h {
                            *scroll = *line + 1 - body_h;
                        }
                        *scroll = (*scroll).min(n.saturating_sub(body_h));
                    }
                }
                MouseEventKind::Down(MouseButton::Right) => self.copy_viewer_selection(),
                _ => {}
            }
            return;
        }

        // In the AI chat, drag selects transcript lines and copies on release;
        // the wheel scrolls; right-click copies. Same feel as the viewer.
        if matches!(self.popup, Popup::AiChat { .. }) {
            let body = self.ai_rect;
            let n = self.ai_lines.len();
            let scroll = self.ai_scroll;
            let line_at = |row: u16| -> usize {
                let rel = row.saturating_sub(body.y) as usize;
                (scroll + rel).min(n.saturating_sub(1))
            };
            let in_body = body.width > 0
                && col >= body.x
                && col < body.x + body.width
                && row >= body.y
                && row < body.y + body.height;
            match ev.kind {
                MouseEventKind::Down(MouseButton::Left) if in_body && n > 0 => {
                    let l = line_at(row);
                    if let Popup::AiChat { sel, .. } = &mut self.popup {
                        *sel = Some((l, l));
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) if n > 0 => {
                    let l = line_at(row);
                    if let Popup::AiChat { sel: Some(s), .. } = &mut self.popup {
                        s.1 = l;
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    // A drag that actually spanned lines copies; a bare click clears.
                    let dragged = matches!(self.popup, Popup::AiChat { sel: Some((a, b)), .. } if a != b);
                    if dragged {
                        self.copy_ai_text();
                    } else if let Popup::AiChat { sel, .. } = &mut self.popup {
                        *sel = None;
                    }
                }
                MouseEventKind::ScrollDown => {
                    if let Popup::AiChat { scroll, .. } = &mut self.popup {
                        *scroll = scroll.saturating_add(3);
                    }
                }
                MouseEventKind::ScrollUp => {
                    if let Popup::AiChat { scroll, .. } = &mut self.popup {
                        *scroll = scroll.saturating_sub(3);
                    }
                }
                MouseEventKind::Down(MouseButton::Right) => self.copy_ai_text(),
                _ => {}
            }
            return;
        }

        // Junk review: a click toggles the row's checkbox (and moves the cursor
        // to it); the wheel scrolls. Approval is still Enter/the button.
        if matches!(self.popup, Popup::JunkReview { .. }) {
            let body = self.junk_rect;
            let row_at = |row: u16, scroll: usize, n: usize| -> Option<usize> {
                if row < body.y || row >= body.y + body.height { return None; }
                let idx = scroll + (row - body.y) as usize;
                if idx < n { Some(idx) } else { None }
            };
            if let Popup::JunkReview { items, cursor, scroll } = &mut self.popup {
                let n = items.len();
                match ev.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(idx) = row_at(row, *scroll, n) {
                            *cursor = idx;
                            items[idx].selected = !items[idx].selected;
                        }
                    }
                    MouseEventKind::ScrollDown => *scroll = (*scroll + 1).min(n.saturating_sub(1)),
                    MouseEventKind::ScrollUp => *scroll = scroll.saturating_sub(1),
                    _ => {}
                }
            }
            return;
        }

        // Dupe review: same feel — click a row to toggle it.
        if matches!(self.popup, Popup::DupeReview { .. }) {
            let body = self.dupe_rect;
            let row_at = |row: u16, scroll: usize, n: usize| -> Option<usize> {
                if row < body.y || row >= body.y + body.height { return None; }
                let idx = scroll + (row - body.y) as usize;
                if idx < n { Some(idx) } else { None }
            };
            if let Popup::DupeReview { items, cursor, scroll } = &mut self.popup {
                let n = items.len();
                match ev.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(idx) = row_at(row, *scroll, n) {
                            *cursor = idx;
                            items[idx].selected = !items[idx].selected;
                        }
                    }
                    MouseEventKind::ScrollDown => *scroll = (*scroll + 1).min(n.saturating_sub(1)),
                    MouseEventKind::ScrollUp => *scroll = scroll.saturating_sub(1),
                    _ => {}
                }
            }
            return;
        }

        // Structure review: same feel as junk review — click a row to toggle it.
        if matches!(self.popup, Popup::StructureReview { .. }) {
            let body = self.struct_rect;
            let row_at = |row: u16, scroll: usize, n: usize| -> Option<usize> {
                if row < body.y || row >= body.y + body.height { return None; }
                let idx = scroll + (row - body.y) as usize;
                if idx < n { Some(idx) } else { None }
            };
            if let Popup::StructureReview { items, cursor, scroll, .. } = &mut self.popup {
                let n = items.len();
                match ev.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(idx) = row_at(row, *scroll, n) {
                            *cursor = idx;
                            items[idx].selected = !items[idx].selected;
                        }
                    }
                    MouseEventKind::ScrollDown => *scroll = (*scroll + 1).min(n.saturating_sub(1)),
                    MouseEventKind::ScrollUp => *scroll = scroll.saturating_sub(1),
                    _ => {}
                }
            }
            return;
        }

        // Rename review: same feel — click a row to toggle it.
        if matches!(self.popup, Popup::RenameReview { .. }) {
            let body = self.rename_rect;
            let row_at = |row: u16, scroll: usize, n: usize| -> Option<usize> {
                if row < body.y || row >= body.y + body.height { return None; }
                let idx = scroll + (row - body.y) as usize;
                if idx < n { Some(idx) } else { None }
            };
            if let Popup::RenameReview { items, cursor, scroll } = &mut self.popup {
                let n = items.len();
                match ev.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(idx) = row_at(row, *scroll, n) {
                            *cursor = idx;
                            items[idx].selected = !items[idx].selected;
                        }
                    }
                    MouseEventKind::ScrollDown => *scroll = (*scroll + 1).min(n.saturating_sub(1)),
                    MouseEventKind::ScrollUp => *scroll = scroll.saturating_sub(1),
                    _ => {}
                }
            }
            return;
        }

        // The context menu is mouse-navigable: hovering a row highlights it,
        // clicking it runs it. Handled before the blanket popup guard below.
        if matches!(self.popup, Popup::ContextMenu { .. }) {
            let m = self.menu_rect;
            let top = m.y + 1; // first row inside the border
            let in_cols = col >= m.x && col < m.x + m.width;
            if let Popup::ContextMenu { items, cursor, .. } = &mut self.popup {
                let n = items.len();
                let idx = row.saturating_sub(top) as usize;
                let on_row = in_cols && row >= top && idx < n;
                match ev.kind {
                    MouseEventKind::Moved | MouseEventKind::Drag(MouseButton::Left) => {
                        if on_row {
                            *cursor = idx;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        if on_row {
                            let item = items[idx];
                            let _ = self.run_menu_item(item);
                        } else {
                            // A click off the menu dismisses it entirely, as
                            // menus do — including any parent levels.
                            self.menu_stack.clear();
                            self.popup = Popup::None;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        // Right-click inside a submenu backs out one level.
                        self.menu_back();
                    }
                    _ => {}
                }
            }
            return;
        }

        // Every other popup — confirm dialogs and list pickers — is driven
        // through the hit zones the renderer registered, so it is fully
        // clickable. The wheel scrolls whatever is on screen.
        if !matches!(self.popup, Popup::None) {
            let _ = self.handle_popup_mouse(ev);
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
                    let (from, anchor) =
                        self.file_drag.as_ref().map(|d| (d.from, d.anchor)).unwrap();
                    if let Some(d) = &mut self.file_drag {
                        d.moved = true;
                        d.over = over;
                    }
                    // Dragging inside the origin pane rubber-band-selects rows
                    // between the anchor and the pointer, like a file manager.
                    // Dragging onto the other pane stays a copy/move gesture.
                    if over == Some(from) && from != FocusedPane::Shell {
                        self.cursor_to_row(from, row);
                        // Only start marking once the pointer has actually left
                        // the anchor row: a click that jitters within one cell
                        // reports a same-row Drag, and that must not mark. Once a
                        // real rubber-band has begun, keep updating it (even back
                        // onto the anchor row).
                        let cur = self.active_pane().map(|p| p.cursor).unwrap_or(anchor);
                        let rubber = self.file_drag.as_ref().map(|d| d.rubber).unwrap_or(false)
                            || cur != anchor;
                        if let Some(d) = &mut self.file_drag {
                            d.rubber = rubber;
                        }
                        if rubber {
                            if let Some(p) = self.active_pane_mut() {
                                let (lo, hi) = (anchor.min(cur), anchor.max(cur));
                                p.clear_marks();
                                for i in lo..=hi {
                                    p.set_mark_at(i);
                                }
                            }
                        }
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

        // The mouse wheel scrolls the file pane under the pointer.
        if matches!(ev.kind, MouseEventKind::ScrollDown | MouseEventKind::ScrollUp) {
            if let Some(pane @ (FocusedPane::Left | FocusedPane::Right)) = pane_at(col, row) {
                self.focus(pane);
                let delta: isize = if matches!(ev.kind, MouseEventKind::ScrollDown) { 3 } else { -3 };
                if let Some(p) = self.active_pane_mut() {
                    p.move_cursor(delta);
                }
            }
            return;
        }

        // A shell-pane selection in progress: extend on drag, copy on release.
        if self.shell_sel.is_some() {
            match ev.kind {
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some(sel) = &mut self.shell_sel {
                        sel.end = grid_pos(sel.inner, col, row);
                        sel.dragged = true;
                    }
                    return;
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    let dragged = self.shell_sel.map(|s| s.dragged).unwrap_or(false);
                    if dragged {
                        self.copy_shell_selection(); // copy-on-select; keep the highlight
                    } else {
                        self.shell_sel = None; // a bare click, not a selection
                    }
                    return;
                }
                _ => {}
            }
        }

        if !matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }

        // Clicking a tab label switches to that tab. Checked before the border
        // drag, because the shell's tab bar sits on the files|shell seam row —
        // divider-first would swallow every shell-tab click as a drag.
        if let Some((pane, idx, _)) = self.tab_rects.iter().copied().find(|(_, _, r)| in_rect(*r)) {
            self.focus(pane);
            match pane {
                FocusedPane::Shell => self.shell.select(idx),
                _ => {
                    if let Some(t) = self.active_file_tabs_mut() {
                        t.select(idx);
                    }
                }
            }
            return;
        }

        // Grabbing a border (away from any tab label) starts a resize.
        if let Some(d) = self.dividers.iter().copied().find(|d| in_rect(d.zone)) {
            self.drag = Some(d);
            return;
        }

        match pane_at(col, row) {
            Some(FocusedPane::Shell) => {
                self.focus(FocusedPane::Shell);
                // Clicking a split should focus that split, as in any multiplexer.
                self.select_shell_leaf_at(col, row);
                // Begin a text selection anchored here, if the click landed on a
                // pane's terminal area. A plain drag then selects (no Shift), and
                // release copies.
                self.shell_sel = self
                    .shell_leaves
                    .iter()
                    .copied()
                    .find(|(_, _, _, inner)| {
                        inner.width > 0
                            && inner.height > 0
                            && col >= inner.x
                            && col < inner.x + inner.width
                            && row >= inner.y
                            && row < inner.y + inner.height
                    })
                    .map(|(tab, leaf, _, inner)| {
                        let a = grid_pos(inner, col, row);
                        ShellSel { tab, leaf, inner, anchor: a, end: a, dragged: false }
                    });
            }
            Some(pane) => {
                self.focus(pane);
                // Put the cursor on the row that was clicked.
                self.cursor_to_row(pane, row);
                // The `..` row has no other purpose, so a single click on it
                // steps up a level immediately rather than waiting for a
                // double-click — it can be neither marked nor dragged.
                if self.active_pane().and_then(|p| p.selected()).map(|e| e.is_parent).unwrap_or(false) {
                    self.last_click = None;
                    let _ = self.activate_selected();
                    return;
                }
                // A second click on the same row in quick succession is a
                // double-click: enter a directory, or open a file with its OS
                // default program — the same as Enter / the open key.
                let now = Instant::now();
                let is_double = self
                    .last_click
                    .map(|(t, r)| r == row && now.duration_since(t) < DOUBLE_CLICK)
                    .unwrap_or(false);
                if is_double {
                    self.last_click = None;
                    let _ = self.activate_selected();
                    return;
                }
                self.last_click = Some((now, row));
                // Otherwise arm a drag from here; whether it becomes a drag or
                // stays a click is decided on release. The cursor was just put
                // on the clicked row, so that is the selection anchor.
                let anchor = self.active_pane().map(|p| p.cursor).unwrap_or(0);
                let paths = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
                if !paths.is_empty() {
                    self.file_drag = Some(FileDrag {
                        from: pane,
                        paths,
                        over: Some(pane),
                        moved: false,
                        anchor,
                        rubber: false,
                    });
                }
            }
            None => {}
        }
    }

    /// The zone under the pointer, if any. Later zones win, so a small button
    /// drawn on top of a wider row is reachable.
    fn zone_at(&self, col: u16, row: u16) -> Option<ZoneKind> {
        self.popup_zones
            .iter()
            .rev()
            .find(|z| {
                let r = z.rect;
                r.width > 0
                    && r.height > 0
                    && col >= r.x
                    && col < r.x + r.width
                    && row >= r.y
                    && row < r.y + r.height
            })
            .map(|z| z.kind)
    }

    /// Point the active popup's list cursor at `i`. A no-op for popups that have
    /// no cursor (confirm dialogs, notices).
    fn set_popup_cursor(&mut self, i: usize) {
        match &mut self.popup {
            Popup::ContextMenu { cursor, .. }
            | Popup::ColorPicker { cursor, .. }
            | Popup::SortPicker { cursor, .. }
            | Popup::EncodingPicker { cursor, .. }
            | Popup::DirCompare { cursor, .. }
            | Popup::Archive { cursor, .. }
            | Popup::DestPicker { cursor, .. }
            | Popup::FindResults { cursor, .. }
            | Popup::SshHosts { cursor, .. }
            | Popup::SshUsers { cursor, .. }
            | Popup::History { cursor, .. }
            | Popup::Shortcuts { cursor, .. } => *cursor = i,
            _ => {}
        }
    }

    /// Drive the on-screen popup with the mouse: the wheel scrolls, a click on a
    /// registered zone replays the keystroke it stands for so all the existing
    /// popup key handling does the real work.
    fn handle_popup_mouse(&mut self, ev: MouseEvent) -> Result<()> {
        let (col, row) = (ev.column, ev.row);
        let synth = |code| KeyEvent::new(code, KeyModifiers::NONE);
        match ev.kind {
            // The wheel moves the cursor / scroll of whatever is open; every
            // list and scroll popup accepts Down/Up.
            MouseEventKind::ScrollDown => return self.handle_popup_key(synth(KeyCode::Down)),
            MouseEventKind::ScrollUp => return self.handle_popup_key(synth(KeyCode::Up)),
            // Hovering (or dragging over) a row highlights it, as the menu does.
            MouseEventKind::Moved | MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(ZoneKind::SelectRow(i)) = self.zone_at(col, row) {
                    self.set_popup_cursor(i);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => match self.zone_at(col, row) {
                Some(ZoneKind::SelectRow(i)) => {
                    self.set_popup_cursor(i);
                    return self.handle_popup_key(synth(KeyCode::Enter));
                }
                Some(ZoneKind::Char(c)) => return self.handle_popup_key(synth(KeyCode::Char(c))),
                Some(ZoneKind::Enter) => return self.handle_popup_key(synth(KeyCode::Enter)),
                Some(ZoneKind::Esc) => return self.handle_popup_key(synth(KeyCode::Esc)),
                // A click in dead space inside the popup does nothing; a click
                // right outside it is ignored too, so a mis-aimed click never
                // silently confirms a destructive dialog.
                None => {}
            },
            _ => {}
        }
        Ok(())
    }

    /// Act on the selected entry as Enter would: enter a directory, or open a
    /// file with its OS default program (or an init.lua `on_open` handler).
    fn activate_selected(&mut self) -> Result<()> {
        let is_dir = self.active_pane().and_then(|p| p.selected()).map(|e| e.is_dir);
        match is_dir {
            Some(true) => {
                if let Some(p) = self.active_pane_mut() {
                    p.enter_selected()?;
                }
            }
            Some(false) => self.open_externally(),
            None => {}
        }
        Ok(())
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

    /// Maximize the active shell pane, or restore it, animating the pane out of
    /// (or back into) its slot the way full-window zoom animates.
    fn toggle_pane_zoom_animated(&mut self) {
        // The shell panel's inner area (inside its border).
        let s = self.layout_rects.shell;
        let full = Rect::new(
            s.x.saturating_add(1),
            s.y.saturating_add(1),
            s.width.saturating_sub(2),
            s.height.saturating_sub(2),
        );
        if self.shell.zoom_pane {
            // Restoring: shrink back into the slot stashed on the way in.
            let back = self.pane_zoom_return.take();
            self.shell.zoom_pane = false;
            if let Some(back) = back {
                if back != full && back.width > 0 {
                    self.start_anim(AnimKind::PaneZoom { from: full, to: back });
                }
            }
        } else {
            // Maximizing: grow from the active pane's current slot.
            let slot = self.active_shell_leaf_rect();
            self.shell.zoom_pane = true;
            if let Some(slot) = slot {
                self.pane_zoom_return = Some(slot);
                if slot != full && slot.width > 0 {
                    self.start_anim(AnimKind::PaneZoom { from: slot, to: full });
                }
            }
        }
    }

    /// The on-screen rect of the active shell split pane, from the last frame's
    /// captured leaf rects.
    fn active_shell_leaf_rect(&self) -> Option<Rect> {
        let tab = self.shell.active;
        let leaf = self.shell.tabs.get(tab).map(|t| t.active)?;
        self.shell_leaves.iter().find(|(t, l, _, _)| *t == tab && *l == leaf).map(|(_, _, r, _)| *r)
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
                    AnimOverride { ratio: Some((target, r)), freeze_pty: true, show_splits: false }
                }
                AnimKind::Zoom { .. } => {
                    AnimOverride { ratio: None, freeze_pty: true, show_splits: false }
                }
                AnimKind::PaneZoom { .. } => {
                    AnimOverride { ratio: None, freeze_pty: true, show_splits: true }
                }
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
    /// Copy the current shell selection's text to the clipboard, reading it
    /// from the pane's terminal grid.
    fn copy_shell_selection(&mut self) {
        let Some(sel) = self.shell_sel else { return };
        let Some(session) = self
            .shell
            .tabs
            .get(sel.tab)
            .and_then(|t| t.nodes.get(sel.leaf))
            .and_then(|n| n.as_ref())
            .and_then(|n| match n {
                Node::Leaf { session, .. } => Some(session),
                _ => None,
            })
        else {
            return;
        };
        // Order the two ends so start is before end in reading order.
        let (a, b) = (sel.anchor, sel.end);
        let (start, endp) = if (a.0, a.1) <= (b.0, b.1) { (a, b) } else { (b, a) };
        let text = match session.parser().lock() {
            Ok(p) => p.screen().contents_between(start.0, start.1, endp.0, endp.1),
            Err(_) => return,
        };
        let text = text.trim_end_matches(['\n', ' ']).to_string();
        if text.is_empty() {
            return;
        }
        if let Some(cb) = self.clipboard.as_mut() {
            let _ = cb.set_text(text);
        }
        self.message = Some("copied".into());
    }

    fn select_shell_leaf_at(&mut self, col: u16, row: u16) {
        let hit = self.shell_leaves.iter().copied().find(|(_, _, r, _)| {
            r.width > 0 && r.height > 0
                && col >= r.x && col < r.x + r.width
                && row >= r.y && row < r.y + r.height
        });
        if let Some((tab, leaf, _, _)) = hit {
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
        // Whether the AI helper is usable, checked (and cached) up front so the
        // AI entries only appear when they will work.
        let ai = self.ai.is_some() && self.ai_ready();
        let mut items = Vec::new();
        if self.focused == FocusedPane::Shell {
            // A PTY owns its own screen, so the file operations make no sense
            // here. SSH leads: keys never reach the picker while the shell has
            // focus, so this menu is the only way to open it without first
            // leaving the shell — which is exactly where you want it.
            items.push(MenuItem::Ssh);
            items.push(MenuItem::Paste);
            // Session logging, per pane: offer start or stop depending on
            // whether this pane is already recording.
            if self.shell.active_session().map(|s| s.is_logging()).unwrap_or(false) {
                items.push(MenuItem::StopLog);
            } else {
                items.push(MenuItem::StartLog);
            }
            // Re-decode the shell's output (Shift_JIS, UTF-16, …).
            items.push(MenuItem::Encoding);
            // SFTP to/from a configured host, acting on the last file pane.
            if !self.config.ssh_hosts.is_empty() {
                items.push(MenuItem::ScpUpload);
                items.push(MenuItem::ScpDownload);
            }
            items.push(MenuItem::Background);
            // Window operations (splits, tabs, zoom) that otherwise live only on
            // the F-keys, so they are reachable by mouse.
            items.push(MenuItem::WindowMenu);
            if ai {
                items.push(MenuItem::AiMenu);
            }
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
            items.push(MenuItem::FindDupes);
            items.push(MenuItem::HiddenToggle);
            // Git actions, only when this pane sits in a repository.
            if self.git_for(self.focused).is_some() {
                items.push(MenuItem::GitMenu);
            }
            // The bookmarks menu, reachable by mouse as well as the `s` key.
            items.push(MenuItem::Shortcuts);
            // SFTP transfer, offered only when servers are configured.
            if !self.config.ssh_hosts.is_empty() {
                items.push(MenuItem::SendMenu);
            }
            items.push(MenuItem::Ssh);
            items.push(MenuItem::Background);
            if ai {
                items.push(MenuItem::AiMenu);
            }
        }
        // Language toggle, quit and the manual are in every menu, so all are
        // reachable by mouse alone (quitting otherwise needs `q`, which the
        // shell eats).
        items.push(MenuItem::Lang);
        items.push(MenuItem::Quit);
        items.push(MenuItem::Manual);
        self.menu_stack.clear();
        self.popup = Popup::ContextMenu { items, cursor: 0, at: (col, row) };
    }

    /// The children a group item drills into (context-dependent). `None` for a
    /// leaf item.
    fn submenu_children(&self, item: MenuItem) -> Option<Vec<MenuItem>> {
        match item {
            MenuItem::AiMenu => {
                let mut v = vec![MenuItem::AiChat];
                if self.focused == FocusedPane::Shell {
                    v.insert(0, MenuItem::AiExplainError);
                    v.insert(0, MenuItem::AiShellCmd);
                } else {
                    // In a file pane the AI can draft a commit for its repo,
                    // scan the folder for junk, and suggest a structure.
                    v.insert(0, MenuItem::AiCommit);
                    v.insert(0, MenuItem::AiRename);
                    v.insert(0, MenuItem::AiSearch);
                    v.insert(0, MenuItem::AiStructure);
                    v.insert(0, MenuItem::AiJunk);
                }
                v.push(MenuItem::Back);
                Some(v)
            }
            MenuItem::SendMenu => {
                Some(vec![MenuItem::ScpUpload, MenuItem::ScpDownload, MenuItem::Back])
            }
            MenuItem::GitMenu => Some(vec![
                MenuItem::GitStage,
                MenuItem::GitUnstage,
                MenuItem::GitDiscard,
                MenuItem::Back,
            ]),
            MenuItem::WindowMenu => {
                let mut v = vec![
                    MenuItem::ShellSplitLR,
                    MenuItem::ShellSplitTB,
                    MenuItem::ShellNewTab,
                ];
                // Offer the close that matches what is active: a split pane if
                // this tab is split, otherwise the tab itself.
                if self.shell.active_pane_count() > 1 {
                    v.push(MenuItem::ShellCloseSplit);
                } else {
                    v.push(MenuItem::ShellCloseTab);
                }
                v.push(MenuItem::ShellZoom);
                v.push(MenuItem::Back);
                Some(v)
            }
            _ => None,
        }
    }

    /// Open a submenu, stashing the current menu so `Back`/Esc returns to it.
    fn open_submenu(&mut self, items: Vec<MenuItem>) {
        let at = match &self.popup {
            Popup::ContextMenu { at, .. } => *at,
            _ => (0, 0),
        };
        let parent = std::mem::replace(&mut self.popup, Popup::None);
        if matches!(parent, Popup::ContextMenu { .. }) {
            self.menu_stack.push(parent);
        }
        self.popup = Popup::ContextMenu { items, cursor: 0, at };
    }

    /// Go back up one menu level, or close the menu when at the top.
    fn menu_back(&mut self) {
        match self.menu_stack.pop() {
            Some(parent) => self.popup = parent,
            None => self.popup = Popup::None,
        }
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
        // A file op or refresh may have changed the working tree.
        self.invalidate_git();
    }

    fn run_menu_item(&mut self, item: MenuItem) -> Result<()> {
        // A group drills into its submenu; Back climbs out — neither acts.
        if let Some(children) = self.submenu_children(item) {
            self.open_submenu(children);
            return Ok(());
        }
        if item == MenuItem::Back {
            self.menu_back();
            return Ok(());
        }
        // A leaf action closes the whole (possibly nested) menu.
        self.menu_stack.clear();
        self.popup = Popup::None;
        match item {
            MenuItem::AiMenu | MenuItem::SendMenu | MenuItem::WindowMenu | MenuItem::GitMenu | MenuItem::Back => {} // handled above
            MenuItem::ShellSplitLR => {
                let cwd = self.shell_cwd();
                self.shell.split_active(&cwd, SplitDir::LeftRight);
                self.focus(FocusedPane::Shell);
            }
            MenuItem::ShellSplitTB => {
                let cwd = self.shell_cwd();
                self.shell.split_active(&cwd, SplitDir::TopBottom);
                self.focus(FocusedPane::Shell);
            }
            MenuItem::ShellNewTab => {
                let cwd = self.shell_cwd();
                self.shell.new_tab(&cwd);
                self.focus(FocusedPane::Shell);
            }
            MenuItem::ShellCloseSplit => {
                self.popup = Popup::ConfirmClose { target: CloseTarget::ShellPane };
            }
            MenuItem::ShellCloseTab => {
                if self.shell.close_active() {
                    self.focus(self.last_file_pane);
                }
            }
            MenuItem::ShellZoom => {
                self.focus(FocusedPane::Shell);
                // Don't fight a full-screen TUI running in the pane.
                if !self.shell.active_modes().0 {
                    self.toggle_zoom();
                }
            }
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
            MenuItem::ScpUpload => self.start_scp(ScpDir::Upload),
            MenuItem::ScpDownload => self.start_scp(ScpDir::Download),
            MenuItem::StartLog => self.start_log_prompt(),
            MenuItem::StopLog => self.stop_session_log(),
            MenuItem::AiChat => self.open_ai_chat(),
            MenuItem::AiShellCmd => self.start_ai_shell_prompt(),
            MenuItem::AiExplainError => self.explain_shell_error(),
            MenuItem::AiCommit => self.start_ai_commit_message(),
            MenuItem::AiJunk => self.start_ai_junk(),
            MenuItem::AiStructure => self.start_ai_structure(),
            MenuItem::AiRename => self.start_ai_rename_prompt(),
            MenuItem::AiSearch => self.start_ai_search_prompt(),
            MenuItem::GitStage => self.git_stage(),
            MenuItem::GitUnstage => self.git_unstage(),
            MenuItem::GitDiscard => self.git_discard_prompt(),
            MenuItem::Shortcuts => self.start_shortcuts(),
            MenuItem::Lang => {
                // Flip the interface language; every localized string reads
                // `self.lang` at draw time, so the next frame is fully in the
                // new language.
                self.lang = self.lang.toggled();
                self.message = Some(match self.lang {
                    Lang::En => "language: English".into(),
                    Lang::Ja => "言語: 日本語".into(),
                });
            }
            MenuItem::Encoding => {
                match self.shell.active_session() {
                    Some(s) => {
                        let cur = cian_core::viewer::TextEncoding::ALL
                            .iter()
                            .position(|e| *e == s.encoding())
                            .unwrap_or(0);
                        self.popup =
                            Popup::EncodingPicker { cursor: cur, target: EncTarget::Shell };
                    }
                    None => self.message = Some("no shell here".into()),
                }
            }
            MenuItem::Quit => self.start_quit_confirm(),
            MenuItem::HiddenToggle => self.toggle_hidden(),
            MenuItem::Attributes => self.show_attributes(),
            MenuItem::Hash => self.start_hash(cian_core::attrs::HashKind::Sha256),
            MenuItem::Compare => self.open_diff(),
            MenuItem::FindDupes => self.start_dupes(),
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

    /// Grow or shrink a pane from the keyboard, moving the relevant divider in
    /// the arrow's direction (Right pushes the divider right, so the pane on
    /// its left grows, and so on). Which divider depends on where the focus is:
    ///
    /// - a file pane: Left/Right move the left|right split, Up/Down the
    ///   files|shell split;
    /// - the shell: Left/Right resize the nearest side-by-side split; Up/Down
    ///   resize the nearest stacked split, or the files|shell split when the
    ///   shell has none.
    fn resize_split(&mut self, dir: KeyCode) {
        const STEP: i16 = 4;
        let clamp = |v: i16| v.clamp(MIN_SPLIT_PCT as i16, 100 - MIN_SPLIT_PCT as i16) as u16;
        // The files|shell divider is `main_pct` = height given to the files;
        // Down grows the files, Up grows the shell.
        let main = |s: &mut Self, delta: i16| s.main_pct = clamp(s.main_pct as i16 + delta);

        match self.focused {
            FocusedPane::Left | FocusedPane::Right => match dir {
                // `panes_pct` is the left pane's width: Right grows the left.
                KeyCode::Right => self.panes_pct = clamp(self.panes_pct as i16 + STEP),
                KeyCode::Left => self.panes_pct = clamp(self.panes_pct as i16 - STEP),
                KeyCode::Down => main(self, STEP),
                KeyCode::Up => main(self, -STEP),
                _ => {}
            },
            FocusedPane::Shell => {
                let active = self.shell.active;
                // Resolve which inner split (if any) the key targets before any
                // mutable borrow, so the fallback can touch `self.main_pct`.
                let want = match dir {
                    KeyCode::Left | KeyCode::Right => Some(SplitDir::LeftRight),
                    KeyCode::Up | KeyCode::Down => Some(SplitDir::TopBottom),
                    _ => None,
                };
                let node = want.and_then(|w| {
                    self.shell.tabs.get(active).and_then(|t| t.nearest_split(w))
                });
                let delta = match dir {
                    KeyCode::Right | KeyCode::Down => STEP,
                    KeyCode::Left | KeyCode::Up => -STEP,
                    _ => 0,
                };
                match node {
                    Some(n) => {
                        if let Some(t) = self.shell.tabs.get_mut(active) {
                            t.nudge_split(n, delta);
                        }
                    }
                    // No inner split along this axis: Up/Down still move the
                    // files|shell divider so the whole shell grows or shrinks.
                    None if matches!(dir, KeyCode::Up | KeyCode::Down) => main(self, delta),
                    None => {}
                }
            }
        }
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
        // A directory comparison is likewise interruptible with Esc.
        if self.diff_job.is_some() {
            if key.code == KeyCode::Esc {
                if let Some(j) = &self.diff_job {
                    j.cancel.store(true, Ordering::Relaxed);
                }
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
        // Ctrl+Shift+Arrow resizes panes from the keyboard, the counterpart to
        // dragging a border. Global (works from the shell too); the modifier
        // combination is not one a shell program expects on an arrow key.
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && key.modifiers.contains(KeyModifiers::SHIFT)
            && matches!(
                key.code,
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down
            )
        {
            self.resize_split(key.code);
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
                    self.toggle_pane_zoom_animated();
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
        if matches!(self.popup, Popup::AiChat { .. }) {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Esc => self.popup = Popup::None,
                KeyCode::Enter => self.send_ai_message(),
                KeyCode::PageUp | KeyCode::Up => {
                    if let Popup::AiChat { scroll, .. } = &mut self.popup {
                        *scroll = scroll.saturating_sub(3);
                    }
                }
                KeyCode::PageDown | KeyCode::Down => {
                    if let Popup::AiChat { scroll, .. } = &mut self.popup {
                        *scroll = scroll.saturating_add(3);
                    }
                }
                KeyCode::Char('u') if ctrl => {
                    if let Popup::AiChat { input, .. } = &mut self.popup {
                        input.clear();
                    }
                }
                // Ctrl+V pastes the clipboard into the input.
                KeyCode::Char('v') if ctrl => {
                    let text = self.clipboard_text();
                    if let (Some(t), Popup::AiChat { input, .. }) = (text, &mut self.popup) {
                        input.push_str(t.trim_end_matches(['\r', '\n']));
                    }
                }
                // Ctrl+Y copies the current selection, or the last reply if none.
                KeyCode::Char('y') if ctrl => self.copy_ai_text(),
                KeyCode::Char('c') if ctrl => self.copy_ai_text(),
                KeyCode::Char(_) if ctrl => {}
                KeyCode::Backspace => {
                    if let Popup::AiChat { input, sel, .. } = &mut self.popup {
                        input.pop();
                        *sel = None;
                    }
                }
                KeyCode::Char(c) => {
                    if let Popup::AiChat { input, sel, .. } = &mut self.popup {
                        input.push(c);
                        *sel = None; // typing dismisses a selection
                    }
                }
                _ => {}
            }
            return Ok(());
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
                // Esc / q / ← climb out of a submenu, or close at the top.
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Left | KeyCode::Char('h') => {
                    self.menu_back()
                }
                KeyCode::Char('j') | KeyCode::Down => *cursor = (*cursor + 1) % n,
                KeyCode::Char('k') | KeyCode::Up => *cursor = (*cursor + n - 1) % n,
                // → / l drills into a group.
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    let item = items[*cursor];
                    if key.code != KeyCode::Enter && !item.is_group() {
                        return Ok(()); // →/l only acts on groups
                    }
                    return self.run_menu_item(item);
                }
                _ => {}
            }
            return Ok(());
        }
        if let Popup::SshHosts { cursor, filter } = &mut self.popup {
            match key.code {
                // Cancelling the picker abandons any transfer being set up, so
                // a later plain :ssh does not get routed into SFTP.
                KeyCode::Esc => {
                    self.popup = Popup::None;
                    self.scp_dir = None;
                }
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
                            if self.scp_dir.is_some() {
                                self.scp_after_pick(i, &only);
                            } else {
                                self.ssh_connect(i, &only);
                            }
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
                    if self.scp_dir.is_some() {
                        self.scp_after_pick(h, &user);
                    } else {
                        self.ssh_connect(h, &user);
                    }
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
        if matches!(self.popup, Popup::Viewer { .. }) {
            return self.handle_viewer_key(key);
        }
        // A notice (op results, attributes, checksums, wc…) can be copied
        // whole with `y`, so a hash or a path can be lifted out of it.
        if let Popup::Notice { lines } = &self.popup {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('c') => {
                    let text = lines.join("\n");
                    if let Some(cb) = self.clipboard.as_mut() {
                        let _ = cb.set_text(text);
                    }
                    self.message = Some("copied".into());
                    self.popup = Popup::None;
                    return Ok(());
                }
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => {
                    self.popup = Popup::None;
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }
        if let Popup::DirCompare { entries, cursor, scroll, .. } = &mut self.popup {
            let n = entries.len();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => {
                    if n > 0 { *cursor = (*cursor + 1).min(n - 1); }
                }
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Char('d') | KeyCode::PageDown => *cursor = (*cursor + 20).min(n.saturating_sub(1)),
                KeyCode::Char('u') | KeyCode::PageUp => *cursor = cursor.saturating_sub(20),
                KeyCode::Char('g') | KeyCode::Home => *cursor = 0,
                KeyCode::Char('G') | KeyCode::End => *cursor = n.saturating_sub(1),
                // Jump both panes to the highlighted difference.
                KeyCode::Enter => self.dir_compare_goto(),
                _ => { let _ = scroll; }
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
        if let Popup::EncodingPicker { cursor, .. } = &mut self.popup {
            let n = cian_core::viewer::TextEncoding::ALL.len();
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    *cursor = (*cursor + 1) % n;
                    return Ok(());
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    *cursor = (*cursor + n - 1) % n;
                    return Ok(());
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    // Cancel: restore a stashed viewer unchanged, else just close.
                    self.finish_encoding_pick(None);
                    return Ok(());
                }
                KeyCode::Enter => {
                    let enc = cian_core::viewer::TextEncoding::ALL[*cursor];
                    self.finish_encoding_pick(Some(enc));
                    return Ok(());
                }
                _ => return Ok(()),
            }
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
                        self.start_shortcut_add(Vec::new(), false);
                    }
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }
        if let Popup::Shortcuts { cursor, entries, path } = &mut self.popup {
            let level = sc_level(entries, path);
            let n = level.len();
            let cur_is_group = level.get(*cursor).map(|s| s.is_group()).unwrap_or(false);
            match key.code {
                // Esc / q / ← climb out of a group, or close at the top.
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Left | KeyCode::Char('h') => {
                    if path.pop().is_some() {
                        *cursor = 0;
                    } else {
                        self.popup = Popup::None;
                    }
                }
                // Enter/→ descend into a group; Enter on a leaf runs it.
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                    if n == 0 {
                        return Ok(());
                    }
                    if cur_is_group {
                        path.push(*cursor);
                        *cursor = 0;
                    } else if key.code == KeyCode::Enter {
                        let (p, idx) = (path.clone(), *cursor);
                        self.popup = Popup::None;
                        return self.execute_shortcut(&p, idx);
                    }
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if n > 0 && *cursor + 1 < n {
                        *cursor += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    *cursor = cursor.saturating_sub(1);
                }
                // `a` adds a shortcut in this group; `A` adds a subfolder. The
                // uppercase char is the signal — the SHIFT modifier bit is not
                // reliably reported for letters across terminals.
                KeyCode::Char('a') => {
                    let p = path.clone();
                    self.popup = Popup::None;
                    self.start_shortcut_add(p, false);
                }
                KeyCode::Char('A') => {
                    let p = path.clone();
                    self.popup = Popup::None;
                    self.start_shortcut_add(p, true);
                }
                KeyCode::Char('d') => {
                    if n > 0 {
                        let (p, idx) = (path.clone(), *cursor);
                        if let Some(lvl) = sc_level_mut(&mut self.shortcuts.entries, &p) {
                            if idx < lvl.len() {
                                lvl.remove(idx);
                            }
                        }
                        let _ = self.shortcuts.save();
                        self.reopen_shortcuts(p, idx);
                    }
                }
                KeyCode::Char('r') => {
                    if n > 0 {
                        let (p, idx) = (path.clone(), *cursor);
                        self.popup = Popup::None;
                        self.start_shortcut_edit(p, idx);
                    }
                }
                KeyCode::Char('p') if n > 0 && !cur_is_group => {
                    let (p, idx) = (path.clone(), *cursor);
                    self.copy_shortcut_target_to_clipboard(&p, idx);
                }
                _ => {}
            }
            return Ok(());
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
        if matches!(self.popup, Popup::ConfirmElevate { .. }) {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => self.run_elevated_transfer(),
                KeyCode::Char('n') | KeyCode::Esc => { self.popup = Popup::None; }
                _ => {}
            }
            return Ok(());
        }
        if let Popup::AiShellConfirm { command } = &self.popup {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    let cmd = command.clone();
                    self.popup = Popup::None;
                    self.insert_ai_command_at_prompt(&cmd);
                }
                KeyCode::Char('n') | KeyCode::Esc => self.popup = Popup::None,
                _ => {}
            }
            return Ok(());
        }
        if let Popup::CommitMessage { buffer, editing, .. } = &mut self.popup {
            if *editing {
                // Typing mode: edit the message freely; Esc returns to preview.
                match key.code {
                    KeyCode::Esc => *editing = false,
                    KeyCode::Enter => buffer.push('\n'),
                    KeyCode::Backspace => { buffer.pop(); }
                    KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        buffer.push(c);
                    }
                    _ => {}
                }
                return Ok(());
            }
            // Preview mode: commit, edit, or cancel.
            match key.code {
                KeyCode::Enter | KeyCode::Char('c') => self.commit_with_drafted_message(),
                KeyCode::Char('e') => {
                    if let Popup::CommitMessage { editing, .. } = &mut self.popup {
                        *editing = true;
                    }
                }
                KeyCode::Esc | KeyCode::Char('n') => self.popup = Popup::None,
                _ => {}
            }
            return Ok(());
        }
        if let Popup::JunkReview { items, cursor, .. } = &mut self.popup {
            let n = items.len();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => {
                    if n > 0 { *cursor = (*cursor + 1).min(n - 1); }
                }
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Char('g') | KeyCode::Home => *cursor = 0,
                KeyCode::Char('G') | KeyCode::End => *cursor = n.saturating_sub(1),
                // Space toggles the item under the cursor; `a` toggles all.
                KeyCode::Char(' ') => {
                    if let Some(it) = items.get_mut(*cursor) { it.selected = !it.selected; }
                }
                KeyCode::Char('a') => {
                    let all_on = items.iter().all(|it| it.selected);
                    for it in items.iter_mut() { it.selected = !all_on; }
                }
                // Enter/d hands the checked paths to the normal delete confirm.
                KeyCode::Enter | KeyCode::Char('d') => self.confirm_junk_deletion(),
                _ => {}
            }
            return Ok(());
        }
        if let Popup::DupeReview { items, cursor, .. } = &mut self.popup {
            let n = items.len();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => {
                    if n > 0 { *cursor = (*cursor + 1).min(n - 1); }
                }
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Char('g') | KeyCode::Home => *cursor = 0,
                KeyCode::Char('G') | KeyCode::End => *cursor = n.saturating_sub(1),
                KeyCode::Char(' ') => {
                    if let Some(it) = items.get_mut(*cursor) { it.selected = !it.selected; }
                }
                KeyCode::Char('a') => {
                    let all_on = items.iter().all(|it| it.selected);
                    for it in items.iter_mut() { it.selected = !all_on; }
                }
                KeyCode::Enter | KeyCode::Char('d') => self.confirm_dupe_deletion(),
                _ => {}
            }
            return Ok(());
        }
        if let Popup::StructureReview { items, cursor, .. } = &mut self.popup {
            let n = items.len();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => {
                    if n > 0 { *cursor = (*cursor + 1).min(n - 1); }
                }
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Char('g') | KeyCode::Home => *cursor = 0,
                KeyCode::Char('G') | KeyCode::End => *cursor = n.saturating_sub(1),
                KeyCode::Char(' ') => {
                    if let Some(it) = items.get_mut(*cursor) { it.selected = !it.selected; }
                }
                KeyCode::Char('a') => {
                    let all_on = items.iter().all(|it| it.selected);
                    for it in items.iter_mut() { it.selected = !all_on; }
                }
                // Enter/m runs the checked moves (creating folders as needed).
                KeyCode::Enter | KeyCode::Char('m') => self.apply_structure_plan(),
                _ => {}
            }
            return Ok(());
        }
        if let Popup::RenameReview { items, cursor, .. } = &mut self.popup {
            let n = items.len();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => {
                    if n > 0 { *cursor = (*cursor + 1).min(n - 1); }
                }
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Char('g') | KeyCode::Home => *cursor = 0,
                KeyCode::Char('G') | KeyCode::End => *cursor = n.saturating_sub(1),
                KeyCode::Char(' ') => {
                    if let Some(it) = items.get_mut(*cursor) { it.selected = !it.selected; }
                }
                KeyCode::Char('a') => {
                    let all_on = items.iter().all(|it| it.selected);
                    for it in items.iter_mut() { it.selected = !all_on; }
                }
                // Enter/r renames the checked files.
                KeyCode::Enter | KeyCode::Char('r') => self.apply_rename_plan(),
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
                Popup::ConfirmDiscard { .. } => { self.git_discard(); Ok(()) }
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
            Popup::AiChat { input, .. } => {
                input.push_str(&clean);
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
        // Any keypress ends a mouse selection's highlight — the screen is about
        // to change under it anyway.
        self.shell_sel = None;
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
        // Shift+Enter opens the shell's context menu, the way it does in a file
        // pane — the shell cannot type `:menu`, so this is its way in by
        // keyboard (right-click is the other). Passed through to a full-screen
        // app, which may want it.
        if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT) && !alt_screen {
            self.open_menu_at_cursor();
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
            // Tab management: t = new tab, w = close active tab.
            (false, false, KeyCode::Char('t')) => {
                if let Some(t) = self.active_file_tabs_mut() { t.add_clone()?; }
            }
            (false, false, KeyCode::Char('w')) => {
                if let Some(t) = self.active_file_tabs_mut() { t.close_active(); }
            }
            // F-key tab controls, matching the shell panel: F9 = new tab,
            // F1/F2 = previous/next tab, F10 = close tab (with confirm). Plain
            // and shifted both work, so the muscle memory carries over.
            (false, _, KeyCode::F(9)) => {
                if let Some(t) = self.active_file_tabs_mut() { t.add_clone()?; }
            }
            (false, _, KeyCode::F(1)) => {
                if let Some(t) = self.active_file_tabs_mut() { t.prev_tab(); }
            }
            (false, _, KeyCode::F(2)) => {
                if let Some(t) = self.active_file_tabs_mut() { t.next_tab(); }
            }
            (false, _, KeyCode::F(10)) => {
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
            // `M` (and Shift+Enter, where the terminal can report it) opens the
            // same menu the right mouse button does, for the entry under the
            // cursor. Shift+Enter needs a terminal that distinguishes it from
            // plain Enter — the Windows console does, and Unix terminals with
            // the kitty keyboard protocol (kitty, WezTerm, foot). macOS
            // Terminal.app cannot, so `M` is the reliable key there; `:menu`
            // always works too.
            (false, _, KeyCode::Char('M')) => self.open_menu_at_cursor(),
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
            // `o` pulls the other pane's directory into this one; `O` pushes
            // this pane's directory onto the other. (Open-into-other-pane lives
            // on Ctrl+Enter above.)
            (false, false, KeyCode::Char('o')) => { self.sync_active_from_other()?; }
            (false, true, KeyCode::Char('O')) => { self.sync_other_from_active()?; }
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
            Action::CursorTop => {
                if let Some(p) = self.active_pane_mut() { p.cursor = 0; }
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
            Action::SyncFromOther => self.sync_active_from_other()?,
            Action::SyncToOther => self.sync_other_from_active()?,
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
            Action::Filter => self.start_filter(),
            Action::FindRecursive => self.start_find_prompt(),
            Action::GrepRecursive => self.start_grep_prompt(),
            Action::Sort => self.start_sort_picker(),
            Action::JumpPath => self.start_jump_path(),
            Action::View => self.look_inside(),
            Action::Diff => self.open_diff(),
            Action::Refresh => {
                self.reload_both();
                self.message = Some("refreshed".into());
            }
            Action::Menu => self.open_menu_at_cursor(),
            Action::Ssh => self.start_ssh(),
            Action::NewTab => {
                if let Some(t) = self.active_file_tabs_mut() {
                    t.add_clone()?;
                }
            }
            Action::CloseTab => {
                if let Some(t) = self.active_file_tabs_mut() {
                    t.close_active();
                }
            }
            Action::Manual => self.open_manual(),
            Action::Nop => {}
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

/// Pull a host name out of a terminal title like `user@host: ~/dir`, for a
/// log file name. Returns a filesystem-safe token, or None if there's no `@`.
fn host_from_title(title: &str) -> Option<String> {
    let after_at = title.split('@').nth(1)?;
    // The host runs up to the first `:`, space, or slash.
    let host: String = after_at
        .chars()
        .take_while(|c| !matches!(c, ':' | ' ' | '/' | '\t'))
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

/// Reverse the cells covered by a shell selection, so the drag is visible. The
/// selection is linear (like a terminal's): from the anchor to the end in
/// reading order, whole rows in between.
fn highlight_shell_selection(f: &mut Frame, sel: &ShellSel) {
    let inner = sel.inner;
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let (a, b) = (sel.anchor, sel.end);
    let (start, end) = if (a.0, a.1) <= (b.0, b.1) { (a, b) } else { (b, a) };
    let buf = f.buffer_mut();
    for gr in start.0..=end.0 {
        let first = if gr == start.0 { start.1 } else { 0 };
        let last = if gr == end.0 { end.1 } else { inner.width.saturating_sub(1) };
        for gc in first..=last {
            let x = inner.x + gc;
            let y = inner.y + gr;
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_style(Style::default().add_modifier(Modifier::REVERSED));
            }
        }
    }
}

/// Map an on-screen `(col, row)` to a `(grid_row, grid_col)` inside `inner`,
/// clamped to the area — for translating a mouse position to a terminal cell.
fn grid_pos(inner: Rect, col: u16, row: u16) -> (u16, u16) {
    let gr = row.saturating_sub(inner.y).min(inner.height.saturating_sub(1));
    let gc = col.saturating_sub(inner.x).min(inner.width.saturating_sub(1));
    (gr, gc)
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
        let entry = cian_core::Entry {
            name,
            path: path.clone(),
            is_dir: false,
            len: 0,
            modified: None,
            is_parent: false,
        };
        return icon_for(&entry);
    }
    "\u{f15b}" // default file
}

/// One row of the manual: the built-in key(s), the remappable action they run
/// (if any), and what it does, in English and Japanese.
struct ManualEntry {
    keys: &'static str,
    action: Option<Action>,
    en: &'static str,
    ja: &'static str,
}

impl ManualEntry {
    fn desc(&self, lang: Lang) -> &'static str {
        match lang {
            Lang::En => self.en,
            Lang::Ja => self.ja,
        }
    }
}

const fn entry(
    keys: &'static str,
    action: Option<Action>,
    en: &'static str,
    ja: &'static str,
) -> ManualEntry {
    ManualEntry { keys, action, en, ja }
}

/// The manual's contents, grouped into sections. Entries carrying an [`Action`]
/// are remappable, so [`manual_lines`] can append whatever extra keys the user
/// bound to them in `init.lua`.
fn manual_sections() -> Vec<((&'static str, &'static str), Vec<ManualEntry>)> {
    use Action::*;
    vec![
        (
            ("General", "基本"),
            vec![
                entry("q", Some(Quit), "quit (confirms)", "終了（確認あり）"),
                entry(":", Some(Command), "command mode (:q, :shell, :man)", "コマンドモード（:q, :shell, :man）"),
                entry("?, Ctrl+.", None, "show this manual (also right-click)", "このマニュアルを表示（右クリックでも）"),
                entry("Esc", None, "clear marks and filter / leave shell", "マーク・フィルタ解除／シェルを抜ける"),
            ],
        ),
        (
            ("Navigation", "移動"),
            vec![
                entry("j, Down", Some(CursorDown), "cursor down", "カーソルを下へ"),
                entry("k, Up", Some(CursorUp), "cursor up", "カーソルを上へ"),
                entry("Shift+D", Some(PageDown), "move 10 lines down", "10行下へ"),
                entry("Shift+U", Some(PageUp), "move 10 lines up", "10行上へ"),
                entry("gg", None, "jump to top", "先頭へジャンプ"),
                entry("G", Some(CursorBottom), "jump to bottom", "末尾へジャンプ"),
                entry("l, Enter", Some(EnterDir), "enter folder / open file", "フォルダに入る／ファイルを開く"),
                entry("F3", None, "look inside: view a file, list an archive", "中身を見る：ファイル閲覧・書庫の一覧"),
                entry("  in viewer", None, "hjkl move, /n/N search, %/{/}/NG jump, v/V/C-v select y copy", "ビューア内：hjkl移動, /n/N検索, %/{/}/NG移動, v/V/C-v選択 yコピー"),
                entry("  from a grep hit", None, "Ctrl+n/N next/prev hit, Shift+Enter reveal in pane, e encoding", "grepヒットから：Ctrl+n/N 次/前, Shift+Enter 場所へ, e 文字コード"),
                entry("=", None, "compare left ↔ right: two files (line diff), or two folders (recursive)", "左右を比較：ファイル同士（行差分）／フォルダ同士（再帰）"),
                entry("-, Bksp", Some(Parent), "parent folder", "親フォルダへ"),
                entry("Left / Right", None, "focus the left / right pane", "左／右のペインにフォーカス"),
                entry("h", Some(History), "history popup", "履歴ポップアップ"),
                entry("z", None, "go to a typed path (also :cd)", "入力したパスへ移動（:cd でも）"),
                entry("Ctrl+R, F5", None, "refresh now", "今すぐ再読み込み"),
                entry("f", Some(Search), "search in this folder", "このフォルダ内を検索"),
                entry("Shift+F", None, "find by name, whole tree below here", "名前で検索（ここ以下のツリー全体）"),
                entry("Ctrl+F", None, "grep inside files, whole tree below here", "ファイル内をgrep（ここ以下のツリー全体）"),
                entry("n", Some(SearchNext), "next match", "次のマッチ"),
                entry("N", Some(SearchPrev), "previous match", "前のマッチ"),
                entry("/", None, "filter list as you type", "入力に応じて一覧を絞り込み"),
                entry(",", None, "sort by name / size / date / ext", "ソート：名前／サイズ／日付／拡張子"),
                entry("Shift+S", None, "ssh picker (also :ssh, or right-click)", "SSHピッカー（:ssh・右クリックでも）"),
                entry("Enter, Esc", None, "while filtering: keep / clear it", "フィルタ中：適用したまま／解除"),
            ],
        ),
        (
            ("Marks and file operations", "マークとファイル操作"),
            vec![
                entry("Space", Some(MarkDown), "toggle mark, move down", "マーク切替して下へ"),
                entry("Shift+Space", Some(MarkUp), "toggle mark, move up", "マーク切替して上へ"),
                entry("v", Some(Visual), "visual select", "ビジュアル選択"),
                entry("  a", None, "  in visual: select all (or gg v G)", "  ビジュアル中：全選択（gg v G でも）"),
                entry("  gg / G", None, "  in visual: extend to top / bottom", "  ビジュアル中：先頭／末尾まで伸ばす"),
                entry("V", Some(InvertMarks), "invert all marks", "全マークを反転"),
                entry("y, c", Some(Copy), "copy to opposite pane", "反対ペインへコピー"),
                entry("m", Some(Move), "move to opposite pane", "反対ペインへ移動"),
                entry("d", Some(Delete), "delete (to trash)", "削除（ゴミ箱へ）"),
                entry("r", Some(Rename), "rename", "リネーム"),
                entry("a", Some(NewFile), "new file", "新規ファイル"),
                entry("A", Some(NewDir), "new directory", "新規ディレクトリ"),
                entry("o", Some(SyncFromOther), "this pane → other pane's directory", "このペインを反対ペインと同じ場所に"),
                entry("O", Some(SyncToOther), "other pane → this pane's directory", "反対ペインをこのペインと同じ場所に"),
                entry("Ctrl+Enter", Some(OpenOther), "open in the opposite pane", "反対ペインで開く"),
                entry("p", Some(CopyPath), "copy path text to clipboard", "パス文字列をクリップボードにコピー"),
                entry("Shift+P", Some(CopyFileRef), "copy file(s) to clipboard", "ファイルをクリップボードにコピー"),
                entry("s", Some(Shortcuts), "shortcuts menu", "ショートカットメニュー"),
                entry(":hidden", None, "show / hide dotfiles (also right-click)", "ドットファイルの表示切替（右クリックでも）"),
                entry(":attr", None, "attributes;  :chmod 644,  :readonly on|off", "属性；  :chmod 644,  :readonly on|off"),
                entry(":hash", None, "checksum;  :hash md5  /  :hash sha256", "チェックサム；  :hash md5  /  :hash sha256"),
                entry(":stage / :unstage", None, "git add / git reset the selection (in a repo)", "選択を git add / git reset（リポジトリ内）"),
                entry(":discard", None, "git checkout -- : throw away worktree changes", "git checkout -- ：作業ツリーの変更を破棄"),
                entry("right-click", None, "upload/download to a configured host (SFTP or SCP)", "設定したホストへアップ／ダウンロード（SFTP/SCP）"),
                entry("M / Shift+Enter", Some(Menu), "context menu for the entry (also :menu)", "エントリのコンテキストメニュー（:menu でも）"),
            ],
        ),
        (
            ("Panes and tabs", "ペインとタブ"),
            vec![
                entry("Shift+H/J/K/L", None, "move focus between panes", "ペイン間でフォーカス移動"),
                entry("Ctrl+Shift+←→↑↓", None, "resize panes (border follows the arrow)", "ペインのリサイズ（境界が矢印方向へ）"),
                entry("drag a border", None, "resize any split (mouse)", "境界をドラッグで分割をリサイズ（マウス）"),
                entry("double-click", None, "enter a folder, or open a file (OS default)", "フォルダに入る／ファイルを開く（OS標準）"),
                entry("drag an entry", None, "to the other pane: copy (Shift: move)", "反対ペインへ：コピー（Shift で移動）"),
                entry("  ", None, "  onto the shell: type its path there", "  シェルへ：パスをそこに入力"),
                entry(":copyto", None, "copy to a recent or typed directory", "最近使った／入力したディレクトリへコピー"),
                entry("right-click", None, "context menu (copy/cut/paste, color)", "コンテキストメニュー（コピー/カット/貼付、色）"),
                entry("Ctrl+H/J/K/L", None, "same (needs kitty keyboard support)", "同上（kittyキーボード対応が必要）"),
                entry("t, F9", None, "new tab", "新規タブ"),
                entry("w", None, "close tab", "タブを閉じる"),
                entry("F1 / F2", None, "previous / next tab", "前／次のタブ"),
                entry("Tab, Shift+Tab", None, "next / previous tab", "次／前のタブ"),
                entry("click a tab", None, "switch to it (mouse)", "クリックで切替（マウス）"),
                entry("F10", None, "close tab (confirms)", "タブを閉じる（確認あり）"),
            ],
        ),
        (
            ("Commands (type : then the name — Linux-style)", "コマンド（: に続けて名前を入力 — Linux風）"),
            vec![
                entry(":mkdir", None, "make a directory;  :mkdir -p a/b/c", "ディレクトリ作成；  :mkdir -p a/b/c"),
                entry(":touch", None, "create a file, or bump its mtime", "ファイル作成／mtimeを更新"),
                entry(":cp / :mv", None, "no arg → other pane;  or  :mv <dest>", "引数なし→反対ペイン；  または  :mv <宛先>"),
                entry(":rm", None, "delete the selection (to trash)", "選択物を削除（ゴミ箱へ）"),
                entry(":cd", None, ":cd <path>  /  :cd ..  /  :cd -  /  :cd ~", ":cd <パス>  /  :cd ..  /  :cd -  /  :cd ~"),
                entry(":pwd", None, "show the directory, copy it to the clipboard", "ディレクトリを表示しクリップボードにコピー"),
                entry(":ls", None, "refresh;  :ls -a  toggles dotfiles", "再読み込み；  :ls -a でドットファイル切替"),
                entry(":stat", None, "attributes (same as :attr)", "属性（:attr と同じ）"),
                entry(":file", None, "what the selection is, by content", "選択物の種別を内容から判定"),
                entry(":wc", None, "line / word / byte counts", "行／単語／バイト数"),
                entry(":head / :tail", None, "first / last lines;  :tail -n 40", "先頭／末尾の行；  :tail -n 40"),
                entry(":df", None, "free disk space;  :df -h -k -m -g", "ディスク空き容量；  :df -h -k -m -g"),
                entry(":reload", None, "re-read init.lua (theme/border need a restart)", "init.luaを再読込（テーマ/枠は再起動が必要）"),
                entry(":mark", None, "mark by wildcard;  :mark *.rs   :unmark *", "ワイルドカードでマーク；  :mark *.rs   :unmark *"),
                entry(":ai", None, "AI chat  (needs cian.ai in init.lua)", "AIチャット  (init.luaのcian.aiが必要)"),
                entry(":aicmd", None, "AI: shell command from a description", "AI: 説明からシェルコマンド生成"),
                entry(":zip", None, "bundle selection;  :zip -e  for a password", "選択物をまとめる；  :zip -e でパスワード付き"),
                entry(":!cmd", None, "run in shell;  % = selection, %f file, %d dir", "シェルで実行；  % =選択, %f ファイル, %d ディレクトリ"),
            ],
        ),
        (
            ("Shell panel (focus: click, Shift+J, or :shell)", "シェルパネル（フォーカス：クリック・Shift+J・:shell）"),
            vec![
                entry("F1-F8", None, "switch to shell tab 1-8", "シェルタブ 1-8 に切替"),
                entry("F9", None, "new shell tab", "新規シェルタブ"),
                entry("F10", None, "close shell tab", "シェルタブを閉じる"),
                entry("Shift+F1/F2", None, "focus next / previous split pane", "次／前の分割ペインにフォーカス"),
                entry("Shift+F8", None, "v-split (panes side by side)", "左右分割（ペインを横に並べる）"),
                entry("Shift+F9", None, "h-split (panes stacked)", "上下分割（ペインを縦に積む）"),
                entry("Shift+F10", None, "close split pane (confirms)", "分割ペインを閉じる（確認あり）"),
                entry("F12", None, "zoom focused surface (toggle)", "フォーカス中の面をズーム（トグル）"),
                entry("Shift+F12", None, "zoom active split pane (toggle)", "アクティブな分割ペインをズーム（トグル）"),
                entry("drag", None, "select text; it is copied to the clipboard on release", "テキスト選択；離すとクリップボードにコピー"),
                entry("right-click", None, "menu: paste, log, SFTP, text encoding, color", "メニュー：貼付、ログ、SFTP、文字コード、色"),
                entry("Esc", None, "back to files (full-screen apps keep it)", "ファイルに戻る（全画面アプリはEscを保持）"),
            ],
        ),
    ]
}

/// Render the manual in `lang`, folding in the user's `init.lua` key overrides.
///
/// A user-bound key is appended to the action's built-in keys, matching what
/// the running app does (a binding replaces its default; extra aliases show up
/// here so the manual and the keyboard agree).
pub fn manual_lines(keymap: &HashMap<char, Action>, lang: Lang) -> Vec<String> {
    let header = match lang {
        Lang::En => "cian — key manual",
        Lang::Ja => "cian — キー一覧",
    };
    let mut out = vec![header.to_string()];
    for ((en_title, ja_title), entries) in manual_sections() {
        let title = match lang {
            Lang::En => en_title,
            Lang::Ja => ja_title,
        };
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
            out.push(format!("  {:<17} {}", keys, e.desc(lang)));
        }
    }
    out
}

/// Plain-text manual for `cian -man`, using the user's own config so the keys
/// it lists match the keys that will actually work — and its `lang` option.
pub fn manual_text() -> String {
    let config = cian_lua::load();
    let mut keymap: HashMap<char, Action> = HashMap::new();
    for (c, name) in &config.keymaps {
        if let Some(a) = action_from_name(name) {
            keymap.insert(*c, a);
        }
    }
    let lang = Lang::from_opt(config.options.lang.as_deref());
    manual_lines(&keymap, lang).join("\n")
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
        "    a fully-commented starter init.lua is in examples/init.lua".to_string(),
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

/// The directory to open when no path was given on the command line: the
/// configured `home`, else the Desktop, else the home directory, else `.`.
fn default_home(config: &cian_lua::Config) -> PathBuf {
    if let Some(h) = &config.options.home {
        let p = expand_path(h);
        if p.is_dir() {
            return p;
        }
    }
    if let Some(home) = home_dir() {
        let desktop = home.join("Desktop");
        if desktop.is_dir() {
            return desktop;
        }
        if home.is_dir() {
            return home;
        }
    }
    PathBuf::from(".")
}

pub fn run(left: Option<PathBuf>, right: Option<PathBuf>) -> Result<()> {
    // Load user config (never fails; problems are reported below).
    let config = cian_lua::load();

    // Fill in either pane not given a path on the command line.
    let fallback = default_home(&config);
    let left = left.unwrap_or_else(|| fallback.clone());
    let right = right.unwrap_or(fallback);

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
    let mut last_pulse = Instant::now();
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
        // While a pane is recording, keep the frame alive so its carmine
        // border can pulse — throttled to ~8 fps, which is plenty for a
        // 10-second cycle and stays cheap.
        if app.any_logging() && last_pulse.elapsed() >= Duration::from_millis(125) {
            last_pulse = Instant::now();
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
        // A directory comparison lands its whole result at once.
        if app.diff_job.is_some() {
            needs_redraw |= app.poll_diff_job();
        }
        // Catch changes made by anything other than cian.
        if app.poll_external_changes() {
            needs_redraw = true;
        }
        // A running file operation reports in over a channel.
        if app.op_job.is_some() {
            needs_redraw |= app.poll_op_job();
        }
        // A pending AI reply lands over its own channel.
        if app.ai_job.is_some() {
            needs_redraw |= app.poll_ai_job();
        }
        // A running duplicate scan reports its groups when done.
        if app.dupes_job.is_some() {
            needs_redraw |= app.poll_dupes_job();
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
            // `anim_then.is_none()` guards against re-firing every tick while
            // the closing animation runs (the dead pane is still active until
            // it lands). The animated close shrinks the pane away and merges
            // its sibling back in, the same as Shift+F10 does.
            if exited && app.anim_then.is_none() {
                app.close_shell_pane_animated();
                app.message = Some("shell exited".into());
                needs_redraw = true;
            }
        }
        if app.should_quit {
            return Ok(());
        }
    }
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

    #[test]
    fn the_solarized_light_preset_paints_a_light_base() {
        let t = cian_lua::Theme { preset: Some("solarized-light".into()), ..Default::default() };
        let (c, errors) = resolve_theme(&t);
        assert!(errors.is_empty(), "{:?}", errors);
        assert_eq!(c.base_bg, Some(Color::Rgb(0xfd, 0xf6, 0xe3)), "base3 background");
        assert_eq!(c.accent, Color::Rgb(0x26, 0x8b, 0xd2), "solarized blue accent");
        assert_eq!(c.file.directory, Color::Rgb(0x26, 0x8b, 0xd2));
    }

    #[test]
    fn the_default_theme_keeps_the_dark_look() {
        let (c, errors) = resolve_theme(&cian_lua::Theme::default());
        assert!(errors.is_empty());
        assert_eq!(c.base_bg, None, "no painted background — the terminal shows through");
        assert_eq!(c.accent, Color::Cyan);
    }

    #[test]
    fn per_key_overrides_apply_on_top_of_a_preset() {
        let t = cian_lua::Theme {
            preset: Some("solarized-light".into()),
            accent: Some("#ff0000".into()),
            ..Default::default()
        };
        let (c, errors) = resolve_theme(&t);
        assert!(errors.is_empty());
        assert_eq!(c.accent, Color::Rgb(255, 0, 0), "override wins");
        assert_eq!(c.base_bg, Some(Color::Rgb(0xfd, 0xf6, 0xe3)), "rest stays solarized");
    }

    #[test]
    fn an_unknown_preset_reports_and_falls_back_to_dark() {
        let t = cian_lua::Theme { preset: Some("nope".into()), ..Default::default() };
        let (c, errors) = resolve_theme(&t);
        assert!(errors.iter().any(|e| e.contains("unknown preset")), "{:?}", errors);
        assert_eq!(c.base_bg, None);
    }

    /// An app rooted at a temp dir containing `names`.
    fn app_with(names: &[&str]) -> (tempfile::TempDir, App) {
        app_with_keymaps(names, Vec::new())
    }

    /// Like `app_with`, but with the `lang` option set.
    fn app_with_lang(names: &[&str], lang: &str) -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        for n in names {
            std::fs::write(dir.path().join(n), b"").unwrap();
        }
        let p = dir.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.options.lang = Some(lang.to_string());
        let app = App::new(p.clone(), p, config).unwrap();
        (dir, app)
    }

    /// Like `app_with`, but with `cian.set_keymap` overrides applied.
    fn app_with_keymaps(names: &[&str], keymaps: Vec<(char, String)>) -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        for n in names {
            std::fs::write(dir.path().join(n), b"").unwrap();
        }
        let p = dir.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.keymaps = keymaps;
        let app = App::new(p.clone(), p, config).unwrap();
        (dir, app)
    }

    #[test]
    fn shortcuts_save_as_yaml_and_legacy_toml_still_parses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shortcuts.yaml");
        let store = ShortcutStore {
            entries: vec![
                Shortcut::leaf("home".into(), "~/".into()),
                Shortcut::leaf("docs".into(), "https://example.com".into()),
            ],
            path: path.clone(),
        };
        store.save().unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("name: home"), "written as YAML:\n{text}");
        // Round-trips through the YAML parser the loader uses.
        let back: ShortcutsFile = serde_yml::from_str(&text).unwrap();
        assert_eq!(back.shortcuts.len(), 2);
        assert_eq!(back.shortcuts[0].name, "home");

        // A pre-existing TOML file must still parse, so migration keeps entries.
        let legacy = "[[shortcuts]]\nname = \"srv\"\ntarget = \"/srv\"\n";
        let parsed: ShortcutsFile = toml::from_str(legacy).unwrap();
        assert_eq!(parsed.shortcuts[0].target.as_deref(), Some("/srv"));
    }

    #[test]
    fn ai_chat_round_trips_a_mock_reply() {
        let have_py = std::process::Command::new("python3")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !have_py {
            eprintln!("no python3; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(),
            auth_mode: "mock".into(),
            ..Default::default()
        });
        let mut app = App::new(p.clone(), p, config).unwrap();
        assert!(app.ai.is_some(), "AI configured");

        app.open_ai_chat();
        assert!(matches!(app.popup, Popup::AiChat { .. }), "chat opened (mock is available)");
        if let Popup::AiChat { input, .. } = &mut app.popup {
            *input = "hello".into();
        }
        app.send_ai_message();
        // Wait for the worker's reply.
        let start = Instant::now();
        while app.ai_job.is_some() && start.elapsed() < Duration::from_secs(10) {
            app.poll_ai_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        match &app.popup {
            Popup::AiChat { log, .. } => {
                assert!(log.iter().any(|m| m.user && m.text == "hello"), "user turn recorded");
                assert!(
                    log.iter().any(|m| !m.user && m.text.contains("[mock] hello")),
                    "assistant echoed via the mock helper: {:?}",
                    log
                );
            }
            other => panic!("expected the chat, got {:?}", other),
        }
    }

    #[test]
    fn ai_chat_copy_uses_selection_then_last_reply() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.popup = Popup::AiChat {
            input: String::new(),
            log: vec![
                ChatMsg { user: true, text: "hi".into() },
                ChatMsg { user: false, text: "the answer\nline two".into() },
            ],
            scroll: 0,
            pending: false,
            sel: Some((0, 1)),
        };
        // A selection copies those flat lines (as the draw would have populated).
        app.ai_lines = vec!["one".into(), "two".into(), "three".into()];
        app.copy_ai_text();
        assert_eq!(app.message.as_deref(), Some("copied"));
        assert!(matches!(app.popup, Popup::AiChat { sel: None, .. }), "selection cleared");

        // With no selection, it copies the last assistant reply.
        app.copy_ai_text();
        assert_eq!(app.message.as_deref(), Some("copied"));
    }

    #[test]
    fn clean_ai_command_strips_fences_and_prose() {
        assert_eq!(clean_ai_command("ls -la"), "ls -la");
        assert_eq!(clean_ai_command("```sh\nls -la\n```"), "ls -la");
        assert_eq!(clean_ai_command("`git status`"), "git status");
        assert_eq!(clean_ai_command("\n\n  find . -name '*.log'  \n"), "find . -name '*.log'");
    }

    /// The F3 viewer shows a git change bar for lines that differ from HEAD.
    #[test]
    fn the_viewer_shows_a_git_change_bar() {
        let d = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(d.path()).unwrap();
        let ok = std::process::Command::new("git")
            .arg("-C").arg(&dir).args(["init", "-q"]).status()
            .map(|s| s.success()).unwrap_or(false);
        if !ok {
            eprintln!("no git; skipping");
            return;
        }
        for kv in [["user.email", "t@e.com"], ["user.name", "T"], ["core.autocrlf", "false"]] {
            let _ = std::process::Command::new("git").arg("-C").arg(&dir).args(["config", kv[0], kv[1]]).status();
        }
        let f = dir.join("code.txt");
        std::fs::write(&f, "keep\nold\nkeep2\n").unwrap();
        std::process::Command::new("git").arg("-C").arg(&dir).args(["add", "."]).status().unwrap();
        std::process::Command::new("git").arg("-C").arg(&dir).args(["commit", "-qm", "init"]).status().unwrap();
        std::fs::write(&f, "keep\nNEW\nkeep2\n").unwrap();

        let mut app = App::new(dir.clone(), dir.clone(), cian_lua::Config::default()).unwrap();
        app.open_viewer_at(&f, "code.txt", 0);
        // The map was computed for the modified file.
        let Popup::Viewer { git_lines, .. } = &app.popup else { panic!("no viewer") };
        assert_eq!(git_lines.get(&1), Some(&cian_core::git::LineChange::Modified), "line 2 modified");
        // And the change bar renders on screen.
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains('▏'), "change bar shown:\n{screen}");
    }

    /// The status line shows the repo's branch when the pane is in one.
    #[test]
    fn the_status_line_shows_the_git_branch() {
        let d = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(d.path()).unwrap();
        let ok = std::process::Command::new("git")
            .arg("-C").arg(&dir).args(["init", "-q", "-b", "trunk"]).status()
            .map(|s| s.success()).unwrap_or(false);
        if !ok {
            eprintln!("no git (or too old for -b); skipping");
            return;
        }
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::new(dir.clone(), dir, cian_lua::Config::default()).unwrap();
        let screen = render(&mut app, 120, 30).join("\n");
        assert!(screen.contains("trunk"), "branch shown in the status line:\n{screen}");
    }

    /// Stage / unstage / discard through the app on a real throwaway repo.
    #[test]
    fn git_stage_unstage_and_discard_operate_on_the_selection() {
        let d = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(d.path()).unwrap();
        let git_ok = std::process::Command::new("git")
            .arg("-C").arg(&dir).args(["init", "-q"]).status()
            .map(|s| s.success()).unwrap_or(false);
        if !git_ok {
            eprintln!("no git; skipping");
            return;
        }
        for kv in [["user.email", "t@example.com"], ["user.name", "Test"], ["core.autocrlf", "false"]] {
            let _ = std::process::Command::new("git").arg("-C").arg(&dir).args(["config", kv[0], kv[1]]).status();
        }
        // Commit an initial file so we have a tracked file to modify/discard.
        std::fs::write(dir.join("tracked.txt"), "one\n").unwrap();
        std::process::Command::new("git").arg("-C").arg(&dir).args(["add", "."]).status().unwrap();
        std::process::Command::new("git").arg("-C").arg(&dir).args(["commit", "-qm", "init"]).status().unwrap();
        std::fs::write(dir.join("tracked.txt"), "one\ntwo\n").unwrap();

        let mut app = App::new(dir.clone(), dir.clone(), cian_lua::Config::default()).unwrap();
        let _ = render(&mut app, 100, 40); // computes git status
        // Cursor onto tracked.txt (index 0 is `..`).
        let idx = app.active_pane().unwrap().entries.iter()
            .position(|e| e.name == "tracked.txt").unwrap();
        app.active_pane_mut().unwrap().cursor = idx;

        // Stage: the worktree change becomes staged.
        app.git_stage();
        let st = cian_core::git::status(&dir).unwrap();
        assert_eq!(st.mark_for(&dir.join("tracked.txt")), Some(cian_core::git::GitMark::Staged));

        // Unstage: back to a plain worktree modification.
        app.git_stage(); // (re-stage to ensure state)
        app.git_unstage();
        let st = cian_core::git::status(&dir).unwrap();
        assert_eq!(st.mark_for(&dir.join("tracked.txt")), Some(cian_core::git::GitMark::Modified));

        // Discard: confirm dialog, then the change is gone.
        let _ = render(&mut app, 100, 40);
        app.active_pane_mut().unwrap().cursor = idx;
        app.git_discard_prompt();
        assert!(matches!(app.popup, Popup::ConfirmDiscard { .. }), "discard confirms first");
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("tracked.txt")).unwrap(), "one\n",
            "worktree change reverted");
    }

    #[test]
    fn parse_junk_reply_validates_names_and_strips_prose() {
        let names = vec![
            ("target".to_string(), PathBuf::from("/p/target")),
            ("main.rs".to_string(), PathBuf::from("/p/main.rs")),
            (".DS_Store".to_string(), PathBuf::from("/p/.DS_Store")),
        ];
        // Fenced, with prose around it, and a hallucinated name that must be dropped.
        let raw = "Here is the junk:\n```json\n[\
            {\"name\":\"target\",\"reason\":\"build output\"},\
            {\"name\":\".DS_Store\",\"reason\":\"macOS cruft\"},\
            {\"name\":\"nonexistent\",\"reason\":\"made up\"}\
            ]\n```\n";
        let items = parse_junk_reply(raw, &names);
        let got: Vec<&str> = items.iter().map(|i| i.path.file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(got, vec!["target", ".DS_Store"], "only shown names survive");
        assert!(items.iter().all(|i| i.selected), "candidates start checked");
        assert_eq!(items[0].reason, "build output");
        // Never flags source — it just isn't in the reply, and couldn't be added.
        assert!(!got.contains(&"main.rs"));
    }

    #[test]
    fn parse_junk_reply_empty_or_garbage_is_no_items() {
        let names = vec![("x".to_string(), PathBuf::from("/p/x"))];
        assert!(parse_junk_reply("[]", &names).is_empty());
        assert!(parse_junk_reply("I could not find any junk.", &names).is_empty());
    }

    /// The whole duplicate flow: scan a dir with two identical files, wait for
    /// the worker, and check the review pre-selects the redundant copy.
    #[test]
    fn dupe_scan_finds_copies_and_preselects_all_but_one() {
        let (d, mut app) = app_with(&["one.txt", "two.txt", "unique.txt"]);
        std::fs::write(d.path().join("one.txt"), b"same bytes here").unwrap();
        std::fs::write(d.path().join("two.txt"), b"same bytes here").unwrap();
        std::fs::write(d.path().join("unique.txt"), b"different").unwrap();
        app.reload_active();

        app.start_dupes();
        assert!(app.dupes_job.is_some(), "scan running on a worker");
        let start = Instant::now();
        while app.dupes_job.is_some() && start.elapsed() < Duration::from_secs(10) {
            app.poll_dupes_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        let Popup::DupeReview { items, .. } = &app.popup else {
            panic!("expected the dupe review, got {:?}", app.popup)
        };
        // Two identical files → one group of two; exactly one is pre-checked.
        assert_eq!(items.len(), 2, "the duplicate pair (unique.txt omitted)");
        assert_eq!(items.iter().filter(|i| i.selected).count(), 1, "keep one, check the other");
        assert_eq!(items.iter().filter(|i| i.keeper).count(), 1);

        // Approving hands the checked copy to the delete confirmation.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        match &app.popup {
            Popup::ConfirmDelete { targets } => assert_eq!(targets.len(), 1),
            other => panic!("expected delete confirm, got {:?}", other),
        }
    }

    #[test]
    fn junk_review_approval_routes_checked_paths_to_delete_confirm() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.popup = Popup::JunkReview {
            items: vec![
                JunkItem { path: PathBuf::from("/p/target"), reason: "build".into(), selected: true },
                JunkItem { path: PathBuf::from("/p/keep"), reason: "".into(), selected: false },
                JunkItem { path: PathBuf::from("/p/cache"), reason: "cache".into(), selected: true },
            ],
            cursor: 0,
            scroll: 0,
        };
        // Enter approves: only the checked ones go to the delete confirmation.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        match &app.popup {
            Popup::ConfirmDelete { targets } => {
                assert_eq!(targets, &vec![PathBuf::from("/p/target"), PathBuf::from("/p/cache")]);
            }
            other => panic!("expected the delete confirm, got {:?}", other),
        }
    }

    #[test]
    fn junk_review_space_toggles_and_a_selects_all() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.popup = Popup::JunkReview {
            items: vec![
                JunkItem { path: PathBuf::from("/p/1"), reason: String::new(), selected: true },
                JunkItem { path: PathBuf::from("/p/2"), reason: String::new(), selected: true },
            ],
            cursor: 0,
            scroll: 0,
        };
        // Space unchecks the first.
        app.handle_key(code(KeyCode::Char(' '))).unwrap();
        // `a` toggles all: since not all are on, it turns all on.
        app.handle_key(code(KeyCode::Char('a'))).unwrap();
        if let Popup::JunkReview { items, .. } = &app.popup {
            assert!(items.iter().all(|i| i.selected), "a turned everything on");
        } else {
            panic!("popup changed");
        }
    }

    #[test]
    fn parse_sem_search_reply_matches_orders_and_folds_reasons() {
        let hit = |rel: &str| cian_core::search::Hit {
            path: PathBuf::from("/root").join(rel),
            rel: PathBuf::from(rel),
            is_dir: false,
            line: None,
        };
        let catalog = vec![hit("src/db.rs"), hit("README.md"), hit("src/ui.rs")];
        // Ranked: ui first, then db; a made-up path is dropped.
        let raw = "```json\n[\
            {\"path\":\"src/ui.rs\",\"reason\":\"UI code\"},\
            {\"path\":\"src/db.rs\",\"reason\":\"database layer\"},\
            {\"path\":\"nope.rs\",\"reason\":\"invented\"}\
            ]\n```";
        let out = parse_sem_search_reply(raw, &catalog);
        let rels: Vec<String> = out.iter().map(|h| h.rel.display().to_string()).collect();
        assert_eq!(rels, vec!["src/ui.rs", "src/db.rs"], "kept order, dropped the invented path");
        // The reason is folded into the line so the list shows it and Enter previews.
        assert_eq!(out[0].line.as_ref().map(|(n, t)| (*n, t.as_str())), Some((1, "UI code")));
    }

    #[test]
    fn ai_search_builds_a_catalog_and_fires_a_request() {
        let have_py = std::process::Command::new("python3")
            .arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
        if !have_py {
            eprintln!("no python3; skipping");
            return;
        }
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("src")).unwrap();
        std::fs::write(d.path().join("src/db.rs"), b"x").unwrap();
        std::fs::write(d.path().join("README.md"), b"x").unwrap();
        let p = d.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(), auth_mode: "mock".into(), ..Default::default()
        });
        let mut app = App::new(p.clone(), p, config).unwrap();

        app.start_ai_search("the database code");
        assert!(app.ai_job.is_some(), "a request was fired over the catalog");
        // The mock echoes (not JSON), so the pipeline reports no matches.
        let start = Instant::now();
        while app.ai_job.is_some() && start.elapsed() < Duration::from_secs(10) {
            app.poll_ai_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.message.as_deref().unwrap_or("").contains("no relevant"),
            "mock reply parses to no matches: {:?}", app.message);
    }

    #[test]
    fn clean_filename_rejects_paths_and_specials() {
        assert_eq!(clean_filename(" report_v2.txt "), Some("report_v2.txt".to_string()));
        assert_eq!(clean_filename("a/b.txt"), None);
        assert_eq!(clean_filename("a\\b.txt"), None);
        assert_eq!(clean_filename(".."), None);
        assert_eq!(clean_filename("."), None);
        assert_eq!(clean_filename(""), None);
        assert_eq!(clean_filename("C:evil"), None);
    }

    #[test]
    fn parse_rename_reply_validates_and_dedupes() {
        let names = vec![
            ("IMG_1.jpg".to_string(), PathBuf::from("/p/IMG_1.jpg")),
            ("IMG_2.jpg".to_string(), PathBuf::from("/p/IMG_2.jpg")),
            ("keep.txt".to_string(), PathBuf::from("/p/keep.txt")),
        ];
        let raw = "[\
            {\"name\":\"IMG_1.jpg\",\"new_name\":\"photo_01.jpg\"},\
            {\"name\":\"IMG_2.jpg\",\"new_name\":\"../escape.jpg\"},\
            {\"name\":\"keep.txt\",\"new_name\":\"keep.txt\"},\
            {\"name\":\"ghost\",\"new_name\":\"x.jpg\"}\
            ]";
        let items = parse_rename_reply(raw, &names);
        // Only IMG_1 survives: IMG_2's target escapes, keep is a no-op, ghost unknown.
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].old, "IMG_1.jpg");
        assert_eq!(items[0].new, "photo_01.jpg");
    }

    /// The whole rename flow: build the review popup and approve — the checked
    /// file is renamed in place, the unchecked left alone.
    #[test]
    fn rename_plan_renames_checked_files() {
        let (d, mut app) = app_with(&["IMG_1.jpg", "keep.txt"]);
        app.popup = Popup::RenameReview {
            items: vec![
                RenameItem { path: d.path().join("IMG_1.jpg"), old: "IMG_1.jpg".into(),
                    new: "photo_01.jpg".into(), selected: true },
                RenameItem { path: d.path().join("keep.txt"), old: "keep.txt".into(),
                    new: "notes.txt".into(), selected: false },
            ],
            cursor: 0,
            scroll: 0,
        };
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(d.path().join("photo_01.jpg").is_file(), "renamed");
        assert!(!d.path().join("IMG_1.jpg").exists(), "old name gone");
        assert!(d.path().join("keep.txt").is_file(), "unchecked untouched");
        assert!(!d.path().join("notes.txt").exists());
    }

    #[test]
    fn truncate_text_for_ai_caps_and_handles_one_long_line() {
        let short = "a\nb\nc\n";
        assert_eq!(truncate_text_for_ai(short, 1000), short, "short text is unchanged");
        // A single line longer than the cap is cut on a char boundary.
        let long = "x".repeat(5000);
        let out = truncate_text_for_ai(&long, 100);
        assert!(out.len() < long.len() && out.contains("truncated"));
        // Multibyte: cutting must not split a char.
        let multi = "あ".repeat(2000);
        let out = truncate_text_for_ai(&multi, 100);
        assert!(out.starts_with("あ") && out.contains("truncated"));
    }

    /// Pressing `S` in the viewer sends the file's text and opens the chat with
    /// the reply (mock: an echo of the body).
    #[test]
    fn viewer_summarize_opens_the_chat_with_a_reply() {
        let have_py = std::process::Command::new("python3")
            .arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
        if !have_py {
            eprintln!("no python3; skipping");
            return;
        }
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("readme.txt"), "hello world\nsecond line\n").unwrap();
        let p = d.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(), auth_mode: "mock".into(), ..Default::default()
        });
        let mut app = App::new(p.clone(), p, config).unwrap();
        app.active_pane_mut().unwrap().cursor = 1; // readme.txt (index 0 is `..`)
        let _ = render(&mut app, 100, 40);
        app.look_inside(); // open the F3 viewer
        assert!(matches!(app.popup, Popup::Viewer { .. }), "viewer open");
        let _ = render(&mut app, 100, 40);

        app.handle_key(code(KeyCode::Char('S'))).unwrap();
        assert!(matches!(app.popup, Popup::AiChat { .. }), "summarise opened the chat");
        let start = Instant::now();
        while app.ai_job.is_some() && start.elapsed() < Duration::from_secs(10) {
            app.poll_ai_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        let Popup::AiChat { log, .. } = &app.popup else { panic!("chat closed") };
        assert!(log.iter().any(|m| !m.user && m.text.contains("hello world")),
            "the mock echoed the file text back as the summary: {log:?}");
    }

    #[test]
    fn clean_dest_folder_rejects_escapes() {
        assert_eq!(clean_dest_folder("images"), Some("images".to_string()));
        assert_eq!(clean_dest_folder(" docs/2023 "), Some("docs/2023".to_string()));
        assert_eq!(clean_dest_folder("a\\b"), Some("a/b".to_string()));
        // Anything that could escape the current directory is refused.
        assert_eq!(clean_dest_folder("../evil"), None);
        assert_eq!(clean_dest_folder("/abs"), None);
        assert_eq!(clean_dest_folder("C:/x"), None);
        assert_eq!(clean_dest_folder("a/../b"), None);
        assert_eq!(clean_dest_folder(""), None);
    }

    #[test]
    fn parse_structure_reply_validates_names_and_folders() {
        let names = vec![
            ("cat.jpg".to_string(), PathBuf::from("/p/cat.jpg")),
            ("notes.md".to_string(), PathBuf::from("/p/notes.md")),
        ];
        let raw = "```json\n[\
            {\"name\":\"cat.jpg\",\"folder\":\"images\",\"reason\":\"an image\"},\
            {\"name\":\"notes.md\",\"folder\":\"../escape\",\"reason\":\"bad folder\"},\
            {\"name\":\"ghost.txt\",\"folder\":\"docs\",\"reason\":\"not shown\"}\
            ]\n```";
        let items = parse_structure_reply(raw, &names);
        assert_eq!(items.len(), 1, "only the valid, real-name move survives");
        assert_eq!(items[0].name, "cat.jpg");
        assert_eq!(items[0].dest, "images");
        assert!(items[0].selected);
    }

    /// The whole structure flow: build a review popup by hand and approve it —
    /// the checked file is moved into a freshly created sub-folder.
    #[test]
    fn structure_plan_moves_checked_files_into_new_folders() {
        let (d, mut app) = app_with(&["cat.jpg", "keep.txt"]);
        let dir = app.active_pane().unwrap().cwd.clone();
        app.popup = Popup::StructureReview {
            items: vec![
                MoveItem { path: d.path().join("cat.jpg"), name: "cat.jpg".into(),
                    dest: "images".into(), reason: "image".into(), selected: true },
                MoveItem { path: d.path().join("keep.txt"), name: "keep.txt".into(),
                    dest: "docs".into(), reason: String::new(), selected: false },
            ],
            cursor: 0,
            scroll: 0,
            dir,
        };
        app.handle_key(code(KeyCode::Enter)).unwrap(); // run the checked moves
        drain_op(&mut app);
        assert!(d.path().join("images/cat.jpg").is_file(), "moved into the new folder");
        assert!(!d.path().join("cat.jpg").exists(), "gone from the root");
        // The unchecked one is left where it was, and its folder not created.
        assert!(d.path().join("keep.txt").is_file(), "unchecked stays put");
        assert!(!d.path().join("docs").exists(), "no folder for an unchecked move");
    }

    #[test]
    fn clean_ai_commit_message_strips_a_wrapping_fence() {
        assert_eq!(clean_ai_commit_message("feat: add x\n\n- why"), "feat: add x\n\n- why");
        assert_eq!(clean_ai_commit_message("```\nfix: bug\n```"), "fix: bug");
        assert_eq!(clean_ai_commit_message("\n\n  chore: tidy  \n\n"), "chore: tidy");
    }

    #[test]
    fn truncate_diff_for_ai_caps_on_a_line_boundary() {
        let big = "line one\nline two\nline three\n".repeat(100);
        let out = truncate_diff_for_ai(&big, 40);
        assert!(out.len() < big.len());
        assert!(out.contains("truncated"), "marks the cut: {out:?}");
        // Only whole lines are kept before the marker.
        let before_marker = out.split("\n\n[").next().unwrap();
        assert!(before_marker.split('\n').all(|l| l.is_empty() || big.contains(l)));
    }

    /// The whole commit-message flow with a throwaway repo: draft (mock), edit,
    /// and commit — then the message is in the log and the stage is clean.
    #[test]
    fn ai_commit_message_flow_drafts_edits_and_commits() {
        let have_py = std::process::Command::new("python3")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !have_py {
            eprintln!("no python3; skipping");
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(d.path()).unwrap();
        let git_ok = std::process::Command::new("git")
            .arg("-C").arg(&dir).args(["init", "-q"]).status()
            .map(|s| s.success()).unwrap_or(false);
        if !git_ok {
            eprintln!("no git; skipping");
            return;
        }
        for kv in [["user.email", "t@example.com"], ["user.name", "Test"]] {
            let _ = std::process::Command::new("git").arg("-C").arg(&dir).args(["config", kv[0], kv[1]]).status();
        }
        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
        cian_core::git::stage(&dir, &[dir.join("a.txt")]).unwrap();

        let mut config = cian_lua::Config::default();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(),
            auth_mode: "mock".into(),
            ..Default::default()
        });
        let mut app = App::new(dir.clone(), dir.clone(), config).unwrap();

        app.start_ai_commit_message();
        let start = Instant::now();
        while app.ai_job.is_some() && start.elapsed() < Duration::from_secs(10) {
            app.poll_ai_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(app.popup, Popup::CommitMessage { .. }), "draft popup, got {:?}", app.popup);

        // Replace the drafted text with our own: e → edit, clear, type.
        app.handle_key(key('e')).unwrap();
        if let Popup::CommitMessage { buffer, .. } = &mut app.popup {
            buffer.clear();
        }
        for c in "add a.txt".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Esc)).unwrap(); // leave edit mode
        app.handle_key(code(KeyCode::Enter)).unwrap(); // commit

        assert!(matches!(app.popup, Popup::None), "committed, popup closed: {:?}", app.popup);
        assert_eq!(cian_core::git::staged_diff(&dir).as_deref(), Some(""), "stage is clean");
        let log = std::process::Command::new("git").arg("-C").arg(&dir).args(["log", "-1", "--pretty=%s"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "add a.txt");
    }

    #[test]
    fn ai_shell_command_flow_yields_a_confirm_popup() {
        let have_py = std::process::Command::new("python3")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !have_py {
            eprintln!("no python3; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(),
            auth_mode: "mock".into(),
            ..Default::default()
        });
        let mut app = App::new(p.clone(), p, config).unwrap();

        app.start_ai_shell_cmd("compress the logs");
        // Wait for the worker; the mock echoes the request as the "command".
        let start = Instant::now();
        while app.ai_job.is_some() && start.elapsed() < Duration::from_secs(10) {
            app.poll_ai_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        match &app.popup {
            Popup::AiShellConfirm { command } => {
                assert!(command.contains("compress the logs"), "got {command:?}");
            }
            other => panic!("expected the command-confirm popup, got {:?}", other),
        }
    }

    #[test]
    fn the_context_menu_drills_into_submenus_and_back() {
        // With SSH hosts, the file menu offers a "Transfer ▸" group.
        let (_d, mut app) = app_with_ssh();
        app.open_context_menu(5, 5);
        let has_group = matches!(&app.popup, Popup::ContextMenu { items, .. } if items.contains(&MenuItem::SendMenu));
        assert!(has_group, "file menu has a Transfer group");

        // Drill in: the submenu shows the SFTP actions and a Back item.
        app.run_menu_item(MenuItem::SendMenu).unwrap();
        match &app.popup {
            Popup::ContextMenu { items, .. } => {
                assert!(items.contains(&MenuItem::ScpUpload));
                assert!(items.contains(&MenuItem::Back));
            }
            other => panic!("expected the submenu, got {:?}", other),
        }
        assert_eq!(app.menu_stack.len(), 1, "parent stashed");

        // Back returns to the parent menu, not to nothing.
        app.run_menu_item(MenuItem::Back).unwrap();
        let back_at_parent = matches!(&app.popup, Popup::ContextMenu { items, .. } if items.contains(&MenuItem::SendMenu));
        assert!(back_at_parent, "Back climbed to the parent");
        assert!(app.menu_stack.is_empty());
    }

    #[test]
    fn ai_chat_is_silent_without_config() {
        let (_d, mut app) = app_with(&["a.txt"]);
        assert!(app.ai.is_none());
        app.open_ai_chat();
        assert!(matches!(app.popup, Popup::None), "no chat without cian.ai config");
        assert!(app.message.as_deref().unwrap_or("").contains("not configured"));
    }

    #[test]
    fn glob_match_handles_stars_and_question_marks() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", ".rs"));
        assert!(!glob_match("*.rs", "main.rst"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("test_*", "test_foo"));
        assert!(!glob_match("test_*", "footest"));
        assert!(glob_match("a*b*c", "axxbyyc"));
    }

    #[test]
    fn mark_command_marks_matching_entries() {
        let (_d, mut app) = app_with(&["a.rs", "b.rs", "c.txt", "readme.md"]);
        app.command_buffer = "mark *.rs".into();
        app.run_command();
        assert_eq!(app.active_pane().unwrap().mark_count(), 2, "two .rs marked");
        // Unmark one class, then all.
        app.command_buffer = "unmark *.rs".into();
        app.run_command();
        assert_eq!(app.active_pane().unwrap().mark_count(), 0);
    }

    #[test]
    fn a_permission_error_explains_admin_rights() {
        let (_d, mut app) = app_with(&["a.rs"]);
        let mut report = OpReport { permission_denied: true, ..Default::default() };
        report.note_error("C:/Program Files/x: Access is denied (os error 5)");
        app.show_op_report(&report);
        let Popup::Notice { lines } = &app.popup else { panic!("expected a notice") };
        assert!(
            lines.iter().any(|l| l.contains("administrator rights")),
            "the notice names the cause: {lines:?}"
        );
    }

    #[test]
    fn a_user_keymap_rebinds_and_disables_keys() {
        let (_d, mut app) = app_with_keymaps(
            &["a.rs", "b.rs"],
            vec![
                ('x', "delete".into()), // bind a new key to an action
                ('d', "none".into()),   // and turn the default off
            ],
        );
        // `x` now opens the delete confirm…
        app.handle_key(key('x')).unwrap();
        assert!(matches!(app.popup, Popup::ConfirmDelete { .. }), "x deletes");
        app.handle_key(code(KeyCode::Esc)).unwrap();
        // …while the disabled `d` does nothing.
        app.handle_key(key('d')).unwrap();
        assert!(matches!(app.popup, Popup::None), "d is unbound");
    }

    #[test]
    fn every_action_named_in_the_example_config_resolves() {
        // Guards against the docs drifting from the code: each
        // `set_keymap("k", "action")` in examples/init.lua must name a real
        // action, so a user copying a line always gets a working binding.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/init.lua");
        let text = std::fs::read_to_string(path).expect("read examples/init.lua");
        let mut checked = 0;
        for line in text.lines() {
            // Only the real binding lines (`cian.set_keymap("k", "action")`),
            // not the "key"/"action" placeholder in the section header or the
            // prose examples that have text before the call.
            let trimmed = line.trim_start_matches(['-', ' ']);
            if !trimmed.starts_with("cian.set_keymap(") {
                continue;
            }
            let Some(rest) = trimmed.split_once("set_keymap(").map(|(_, r)| r) else { continue };
            // The action is the second quoted string on the line.
            let quoted: Vec<&str> = rest.split('"').collect();
            if quoted.len() >= 4 {
                let action = quoted[3];
                assert!(
                    action_from_name(action).is_some(),
                    "examples/init.lua names unknown action {:?}",
                    action
                );
                checked += 1;
            }
        }
        assert!(checked > 20, "expected to have checked the documented bindings, got {checked}");
    }

    #[test]
    fn reload_reapplies_the_keymap_live() {
        let (_d, mut app) = app_with(&["a.rs"]);
        // No user binding yet: `x` is not delete.
        assert!(!app.keymap.contains_key(&'x'));
        // Point CIAN_CONFIG_DIR at a temp config that binds x -> delete, then
        // reload — the running app should pick it up without a restart.
        let cfgdir = tempfile::tempdir().unwrap();
        std::fs::write(
            cfgdir.path().join("init.lua"),
            "cian.set_keymap(\"x\", \"delete\")\n",
        )
        .unwrap();
        std::env::set_var("CIAN_CONFIG_DIR", cfgdir.path());
        app.command_buffer = "reload".into();
        app.run_command();
        std::env::remove_var("CIAN_CONFIG_DIR");
        assert_eq!(app.keymap.get(&'x'), Some(&Action::Delete), "reload bound x live");
    }

    #[test]
    fn a_newly_named_action_is_bindable() {
        // `sort` had no bindable name before; confirm it now resolves and works.
        assert_eq!(action_from_name("sort"), Some(Action::Sort));
        let (_d, mut app) = app_with_keymaps(&["a.rs"], vec![('S', "sort".into())]);
        app.handle_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::SortPicker { .. }), "S opens the sort picker");
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

    /// Click the centre of the first popup zone matching `want`, after a render
    /// has registered the zones. Returns false if no such zone exists.
    fn click_zone(app: &mut App, want: ZoneKind) -> bool {
        let hit = app.popup_zones.iter().find(|z| z.kind == want).map(|z| z.rect);
        match hit {
            Some(r) => {
                app.handle_mouse(mouse(
                    MouseEventKind::Down(MouseButton::Left),
                    r.x + r.width / 2,
                    r.y,
                ));
                true
            }
            None => false,
        }
    }

    #[test]
    fn the_wheel_scrolls_the_file_pane_under_the_pointer() {
        let names: Vec<String> = (0..40).map(|i| format!("f{:02}.txt", i)).collect();
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let (_d, mut app) = app_with(&refs);
        let _ = render(&mut app, 100, 40);
        let start = app.active_pane().unwrap().cursor;
        let left = app.layout_rects.left;
        app.handle_mouse(mouse(MouseEventKind::ScrollDown, left.x + 3, left.y + 3));
        let after = app.active_pane().unwrap().cursor;
        assert!(after > start, "wheel down moved the cursor down: {start} -> {after}");
        app.handle_mouse(mouse(MouseEventKind::ScrollUp, left.x + 3, left.y + 3));
        assert!(app.active_pane().unwrap().cursor < after, "wheel up moved it back up");
    }

    #[test]
    fn dragging_inside_a_pane_rubber_band_selects() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt", "d.txt"]);
        let _ = render(&mut app, 100, 40);
        let left = app.layout_rects.left;
        // Row 1 is the `..` row; the files start on row 2. Press on the first
        // file, drag down two more, release inside the pane.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 3, left.y + 2));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), left.x + 3, left.y + 4));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), left.x + 3, left.y + 4));
        // The dragged-over range is now marked (3 files), not a copy to elsewhere.
        assert_eq!(app.active_pane().unwrap().mark_count(), 3, "range is marked");
        assert!(app.file_drag.is_none(), "drag released");
    }

    #[test]
    fn clicking_a_sort_picker_row_applies_it() {
        let (_d, mut app) = app_with(&["a.rs", "b.rs"]);
        app.start_sort_picker();
        assert!(matches!(app.popup, Popup::SortPicker { .. }));
        // Render so the row hit-zones are registered, then click the 3rd entry.
        let _ = render(&mut app, 100, 40);
        assert!(click_zone(&mut app, ZoneKind::SelectRow(2)), "row zone present");
        // A pick closes the picker and applies that key.
        assert!(matches!(app.popup, Popup::None), "picker closed after a click");
        assert_eq!(app.active_pane().unwrap().sort.key, SortKey::ALL[2]);
    }

    #[test]
    fn clicking_a_confirm_dialog_button_answers_it() {
        let (_d, mut app) = app_with(&["a.rs"]);
        app.start_quit_confirm();
        assert!(matches!(app.popup, Popup::ConfirmQuit));
        let _ = render(&mut app, 100, 40);
        // The "No" button cancels without quitting.
        assert!(click_zone(&mut app, ZoneKind::Esc), "No button present");
        assert!(matches!(app.popup, Popup::None));
        assert!(!app.should_quit);

        app.start_quit_confirm();
        let _ = render(&mut app, 100, 40);
        assert!(click_zone(&mut app, ZoneKind::Enter), "Yes button present");
        assert!(app.should_quit, "clicking Yes quits");
    }

    #[test]
    fn the_mouse_wheel_scrolls_the_manual() {
        let (_d, mut app) = app_with(&["a.rs"]);
        app.open_manual();
        let _ = render(&mut app, 100, 40);
        app.handle_mouse(mouse(MouseEventKind::ScrollDown, 50, 20));
        app.handle_mouse(mouse(MouseEventKind::ScrollDown, 50, 20));
        let Popup::Manual { scroll, .. } = &app.popup else { panic!("expected manual") };
        assert_eq!(*scroll, 2, "two wheel notches scrolled two lines");
    }

    #[test]
    fn slash_filters_the_listing_incrementally() {
        let (_d, mut app) = app_with(&["alpha.rs", "beta.rs", "gamma.txt"]);
        // Counts include the synthetic `..` row, so a 3-file dir lists 4.
        assert_eq!(app.active_pane().unwrap().entries.len(), 4);

        app.handle_key(key('/')).unwrap();
        assert_eq!(app.mode, Mode::Filter);

        app.handle_key(key('r')).unwrap();
        app.handle_key(key('s')).unwrap();
        assert_eq!(app.active_pane().unwrap().entries.len(), 3);

        // Backspace widens the match: "r" still excludes gamma.txt.
        app.handle_key(code(KeyCode::Backspace)).unwrap();
        assert_eq!(app.active_pane().unwrap().entries.len(), 3);

        // Emptying the buffer restores the full listing.
        app.handle_key(code(KeyCode::Backspace)).unwrap();
        assert_eq!(app.filter_buffer, "");
        assert_eq!(app.active_pane().unwrap().entries.len(), 4);
    }

    #[test]
    fn enter_keeps_the_filter_and_esc_clears_it() {
        let (_d, mut app) = app_with(&["a.txt", "b.md"]);

        app.handle_key(key('/')).unwrap();
        app.handle_key(key('m')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.mode, Mode::Normal);
        // `..` plus the one match survives the filter.
        assert_eq!(app.active_pane().unwrap().entries.len(), 2, "filter should survive Enter");

        // Esc in normal mode drops the narrowing.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(app.active_pane().unwrap().entries.len(), 3);
    }

    #[test]
    fn esc_while_filtering_restores_the_full_list() {
        let (_d, mut app) = app_with(&["a.txt", "b.md"]);
        app.handle_key(key('/')).unwrap();
        app.handle_key(key('m')).unwrap();
        assert_eq!(app.active_pane().unwrap().entries.len(), 2, "`..` plus the match");
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.active_pane().unwrap().entries.len(), 3);
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
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");
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
        let text = manual_lines(&keymap, Lang::En).join("\n");
        assert!(text.contains("d, x"), "user-bound key missing from manual:\n{}", text);
    }

    #[test]
    fn the_status_and_hints_default_to_english_and_switch_to_japanese() {
        // Default is English.
        let (_d, mut app) = app_with(&["a.txt"]);
        let en = render(&mut app, 110, 40).join("\n");
        assert!(en.contains("items") && en.contains("help"), "English chrome:\n{en}");

        // lang=ja renders the chrome in Japanese. A wide (CJK) glyph occupies
        // two cells, so the row reconstruction inserts a space after each; strip
        // spaces before matching the words.
        let flat = |app: &mut App| render(app, 110, 40).join("\n").replace(' ', "");
        let (_d2, mut ja) = app_with_lang(&["a.txt"], "ja");
        let screen = flat(&mut ja);
        assert!(screen.contains("件"), "status counts in Japanese:\n{screen}");
        assert!(screen.contains("ヘルプ"), "help hint in Japanese");
        ja.open_context_menu(5, 5);
        let menu = flat(&mut ja);
        assert!(menu.contains("コピー"), "menu in Japanese:\n{menu}");
    }

    #[test]
    fn the_manual_defaults_to_english_and_switches_to_japanese() {
        let keymap = HashMap::new();
        let en = manual_lines(&keymap, Lang::En).join("\n");
        assert!(en.contains("key manual"), "English header");
        assert!(en.contains("delete (to trash)"), "English description present");
        let ja = manual_lines(&keymap, Lang::Ja).join("\n");
        assert!(ja.contains("キー一覧"), "Japanese header:\n{ja}");
        assert!(ja.contains("削除（ゴミ箱へ）"), "Japanese description present");

        // The `lang` option drives which one an App shows.
        let (_d, app_en) = app_with(&["a.rs"]);
        assert_eq!(app_en.lang, Lang::En, "default is English");
        let (_d2, app_ja) = app_with_lang(&["a.rs"], "ja");
        assert_eq!(app_ja.lang, Lang::Ja, "lang=ja switches to Japanese");
    }

    #[test]
    fn the_menu_language_toggle_flips_the_interface() {
        let (_d, mut app) = app_with(&["a.txt"]);
        assert_eq!(app.lang, Lang::En, "starts English");
        app.run_menu_item(MenuItem::Lang).unwrap();
        assert_eq!(app.lang, Lang::Ja, "toggled to Japanese");
        // The label reflects the language it switches *to*.
        assert_eq!(MenuItem::Lang.label(Lang::Ja), "Switch to English");
        assert_eq!(MenuItem::Lang.label(Lang::En), "日本語に切替");
        app.run_menu_item(MenuItem::Lang).unwrap();
        assert_eq!(app.lang, Lang::En, "toggled back to English");
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
        // Grab the middle of the seam, not its very corner — the corner shares
        // a cell with a tab label, which now wins the click.
        let grab = (d.zone.x + d.zone.width / 2, d.zone.y + d.zone.height / 2);
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

    /// `o` pulls the other pane's directory into the active one; `O` pushes the
    /// active pane's directory onto the other. Focus never moves.
    #[test]
    fn o_and_shift_o_sync_the_two_panes_directories() {
        let (l, r, mut app) = app_two_dirs(&["a.txt"], &["b.txt"]);
        let (ldir, rdir) = (l.path().to_path_buf(), r.path().to_path_buf());
        assert_ne!(app.left.active_ref().cwd, app.right.active_ref().cwd);

        // On the right pane, `o` makes the right pane show the left's directory.
        app.focus(FocusedPane::Right);
        app.handle_key(key('o')).unwrap();
        assert!(app.right.active_ref().cwd.ends_with(ldir.file_name().unwrap()),
            "right pulled the left's dir");
        assert_eq!(app.focused, FocusedPane::Right, "focus stays put");

        // Reset the right pane, then `O` pushes the right's dir onto the left.
        app.right.active_mut().jump_to(rdir.clone()).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('O'), KeyModifiers::SHIFT)).unwrap();
        assert!(app.left.active_ref().cwd.ends_with(rdir.file_name().unwrap()),
            "left received the right's dir");
        assert_eq!(app.focused, FocusedPane::Right, "focus still on the right");
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
        // No SSH hosts configured here, so SFTP entries are omitted.
        assert_eq!(
            items,
            &vec![
                MenuItem::Ssh,
                MenuItem::Paste,
                MenuItem::StartLog,
                MenuItem::Encoding,
                MenuItem::Background,
                MenuItem::WindowMenu,
                MenuItem::Lang,
                MenuItem::Quit,
                MenuItem::Manual
            ]
        );
    }

    #[test]
    fn explain_error_without_a_shell_reports_it() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Force AI on (mock) so we get past the config gate to the shell check.
        app.ai = Some(cian_ai::AiConfig { auth_mode: "mock".into(), ..Default::default() });
        app.focus(FocusedPane::Shell);
        app.explain_shell_error();
        assert!(app.message.as_deref().unwrap_or("").contains("no shell"),
            "reports the absence of a shell: {:?}", app.message);
    }

    #[test]
    fn shell_window_submenu_offers_splits_tabs_and_zoom() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        app.open_context_menu(3, 3);
        // Drill into Window ▸.
        app.run_menu_item(MenuItem::WindowMenu).unwrap();
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no submenu") };
        assert!(items.contains(&MenuItem::ShellSplitLR));
        assert!(items.contains(&MenuItem::ShellSplitTB));
        assert!(items.contains(&MenuItem::ShellNewTab));
        assert!(items.contains(&MenuItem::ShellZoom));
        // A single (unsplit) tab offers "close tab", not "close split".
        assert!(items.contains(&MenuItem::ShellCloseTab));
        assert!(!items.contains(&MenuItem::ShellCloseSplit));
        assert!(items.contains(&MenuItem::Back));
    }

    #[test]
    fn attributes_lines_show_a_size_for_a_file() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("data.bin");
        std::fs::write(&f, vec![0u8; 2048]).unwrap();
        let (_d2, app) = app_with(&["a.txt"]);
        let lines = app.attributes_lines(&[f], 40);
        // Human-readable size appears on the entry's row.
        assert!(lines.iter().any(|l| l.contains("data.bin") && (l.contains("2.0K") || l.contains("2K") || l.contains("2048"))),
            "size shown: {lines:?}");
    }

    #[test]
    fn scp_upload_walks_picker_then_asks_for_the_remote_path() {
        let (_d, mut app) = app_with_ssh();
        app.active_pane_mut().unwrap().cursor = 1; // a.txt (index 0 is the `..` row)
        app.start_scp(ScpDir::Upload);
        assert!(matches!(app.popup, Popup::SshHosts { .. }), "opens the host picker");
        assert!(app.scp_dir.is_some());

        // Pick db1 (single user, has a password) → straight to the remote prompt.
        app.command_buffer.clear();
        // Filter to db1 then Enter.
        for c in "db1".chars() {
            app.handle_key(code(KeyCode::Char(c))).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        match &app.popup {
            Popup::TextInput { kind: InputKind::ScpRemote, .. } => {}
            other => panic!("expected the remote-path prompt, got {:?}", other),
        }
        let p = app.scp_pending.as_ref().expect("a pending transfer");
        assert_eq!(p.target.host, "10.0.2.31");
        assert_eq!(p.target.port, 2222);
        assert_eq!(p.target.user, "postgres");
        assert_eq!(p.dir, ScpDir::Upload);
        assert_eq!(p.locals.len(), 1);
    }

    #[test]
    fn scp_upload_without_a_selected_file_is_refused() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("onlydir")).unwrap();
        let mut config = cian_lua::Config::default();
        config.ssh_hosts = vec![cian_lua::SshHost {
            name: "web1".into(),
            host: "10.0.1.11".into(),
            users: vec![cian_lua::SshUser::plain("root")],
            port: None,
        }];
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, config).unwrap();
        app.active_pane_mut().unwrap().cursor = 0; // the directory
        app.start_scp(ScpDir::Upload);
        assert!(matches!(app.popup, Popup::None));
        assert!(app.message.as_deref().unwrap().contains("select a file"));
    }

    #[test]
    fn scp_needs_a_password_for_the_user() {
        let (_d, mut app) = app_with_ssh();
        app.active_pane_mut().unwrap().cursor = 1; // a real file, not the `..` row
        app.start_scp(ScpDir::Upload);
        // web1 / root has no password configured.
        for c in "web1".chars() {
            app.handle_key(code(KeyCode::Char(c))).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap(); // host web1 → user list
        app.handle_key(code(KeyCode::Enter)).unwrap(); // first user (root)
        assert!(app.scp_pending.is_none());
        assert!(app.message.as_deref().unwrap().contains("no password"));
    }

    #[test]
    fn a_host_name_is_pulled_from_a_terminal_title() {
        assert_eq!(host_from_title("taketan@web01: ~/proj"), Some("web01".into()));
        assert_eq!(host_from_title("root@db-server:/var"), Some("db-server".into()));
        // No `@` — nothing to take.
        assert_eq!(host_from_title("just a title"), None);
    }

    #[test]
    fn the_log_prompt_asks_for_a_folder_when_a_shell_exists() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // No shell yet → it declines rather than opening a prompt.
        app.start_log_prompt();
        assert!(matches!(app.popup, Popup::None));
        assert!(app.message.as_deref().unwrap().contains("no shell"));
    }

    #[test]
    fn starting_a_log_in_a_bad_directory_is_refused() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.start_session_log("/no/such/directory/anywhere");
        assert!(app.message.as_deref().unwrap().contains("not a directory"));
    }

    #[test]
    fn choosing_the_manual_from_the_menu_opens_it() {
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");
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
            show_splits: false,
        };
        assert_eq!(ov.ratio_for(DividerTarget::Panes, 50), 90);
        // Other dividers fall through to their stored value.
        assert_eq!(ov.ratio_for(DividerTarget::Main, 60), 60);
        // Stored values are clamped; overrides are not, so a close animation
        // can drive a pane all the way to zero.
        assert_eq!(ov.ratio_for(DividerTarget::Main, 99), 100 - MIN_SPLIT_PCT);
        let zero =
            AnimOverride { ratio: Some((DividerTarget::Main, 0)), freeze_pty: true, show_splits: false };
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
        let l0 = Rect::new(shell.x, shell.y, half, shell.height);
        let l1 = Rect::new(shell.x + half, shell.y, half, shell.height);
        app.shell_leaves = vec![(0, 7, l0, l0), (0, 9, l1, l1)];
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
    fn the_status_bar_drops_the_sort_indicator_but_keeps_the_counts() {
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");
        let screen = render(&mut app, 100, 40).join("\n");
        // The sort chip was removed; the item/mark counts stay.
        assert!(!screen.contains("name ▲"), "the sort indicator should be gone:\n{}", screen);
        assert!(screen.contains("items"));
        assert!(screen.contains("marks"));

        // Sorting still works even though it is no longer shown here.
        app.apply_sort_key(SortKey::Modified);
        assert_eq!(app.active_pane().unwrap().sort.key, SortKey::Modified);
    }

    #[test]
    fn the_key_hint_bar_is_contextual() {
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");
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
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");

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
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");
        for w in [40u16, 60, 80, 110, 200] {
            let screen = render(&mut app, w, 40).join("\n");
            assert!(screen.contains("? help"), "lost at width {}:\n{}", w, screen);
        }
    }

    /// A short window drops the hints rather than squeezing the listing out.
    #[test]
    fn a_short_window_drops_the_hints() {
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");
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
        // entries[0] is the `..` row; the first real entry follows it.
        assert_eq!(app.active_pane().unwrap().entries[1].name, "inner.txt");
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
        // 4 files plus the `..` row → last index is 4.
        assert_eq!(app.active_pane().unwrap().cursor, 4, "cursor at the bottom");

        // Enter commits the range to marks; `..` is never marked, so 4 files.
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
        // 4 files plus the `..` row → last index is 4.
        assert_eq!(app.active_pane().unwrap().cursor, 4, "G must move in visual mode too");

        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.active_pane().unwrap().mark_count(), 4);
    }

    #[test]
    fn gg_works_inside_visual_mode() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt"]);
        // Start on the last file (index 3, after the `..` row) so the range up
        // to the top covers all three files.
        if let Some(p) = app.active_pane_mut() {
            p.cursor = 3;
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
        app.start_shortcut_add(Vec::new(), false);
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
        app.start_shortcut_add(Vec::new(), false);
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

        app.start_shortcut_add(Vec::new(), false);
        for c in "mine".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        let Popup::TextInput { buffer, kind, .. } = &app.popup else { panic!("no target step") };
        assert!(matches!(kind, InputKind::ShortcutTarget { .. }));
        assert_eq!(buffer, &expected.display().to_string());
    }

    /// `A` makes a folder in the current level; Enter steps in; `A` again nests;
    /// Esc/← climbs back out. The tree is what gets saved.
    #[test]
    fn shortcuts_menu_creates_and_navigates_nested_folders() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Bookmarks live in a temp dir so the test never touches the real config,
        // and start empty so indices are predictable regardless of the dev's own.
        let sd = tempfile::tempdir().unwrap();
        app.shortcuts.path = sd.path().join("shortcuts.yaml");
        app.shortcuts.entries.clear();

        // Open the menu and add a top-level folder "Projects" with `A`.
        app.start_shortcuts();
        app.handle_key(key('A')).unwrap();
        for c in "Projects".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        // Back in the menu, the folder is there; step into it with Enter.
        assert!(matches!(&app.popup, Popup::Shortcuts { path, .. } if path.is_empty()));
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let Popup::Shortcuts { path, .. } = &app.popup else { panic!("menu closed") };
        assert_eq!(path, &vec![0], "stepped into the folder");

        // Add a leaf shortcut inside it: name then target.
        app.handle_key(key('a')).unwrap();
        for c in "cian".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap(); // name -> target step
        // Clear the auto-filled target and type our own.
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)).unwrap();
        for c in "~/workspace/cian".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        // The store now holds Projects/cian.
        assert_eq!(app.shortcuts.entries.len(), 1);
        let projects = &app.shortcuts.entries[0];
        assert_eq!(projects.name, "Projects");
        assert!(projects.is_group());
        let kids = projects.children.as_ref().unwrap();
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].name, "cian");
        assert_eq!(kids[0].target.as_deref(), Some("~/workspace/cian"));

        // Esc climbs back to the top rather than closing.
        assert!(matches!(&app.popup, Popup::Shortcuts { path, .. } if path == &vec![0]));
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(&app.popup, Popup::Shortcuts { path, .. } if path.is_empty()));
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None), "Esc at the top closes the menu");
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

    #[test]
    fn choosing_a_grep_hit_opens_the_viewer_at_that_line() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("code.txt"),
            "first line\nsecond has TARGET here\nthird line\n",
        )
        .unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.start_find("TARGET", cian_core::search::Mode::Content);
        drain_find(&mut app);
        let has_hit = matches!(&app.popup, Popup::FindResults { hits, .. } if !hits.is_empty());
        assert!(has_hit, "grep found the line");

        app.open_find_hit().unwrap();
        // The viewer opened on the matched line (line 2 → 0-based index 1).
        match &app.popup {
            Popup::Viewer { line, view, .. } => {
                assert_eq!(*line, 1, "cursor on the matched line");
                assert!(view.lines[*line].contains("TARGET"));
            }
            other => panic!("expected the viewer, got {:?}", other),
        }

        // Esc from the viewer returns to the grep results, not to nothing.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(
            matches!(app.popup, Popup::FindResults { .. }),
            "Esc returns to the results list, got {:?}",
            app.popup
        );
        // A second Esc closes the results.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None));
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
        for want in [MenuItem::HiddenToggle, MenuItem::Attributes, MenuItem::Hash, MenuItem::Shortcuts] {
            assert!(items.contains(&want), "{:?} missing from {:?}", want, items);
        }
    }

    /// `M` opens the context menu on every terminal (Shift+Enter can't be
    /// distinguished from Enter on e.g. macOS Terminal.app).
    #[test]
    fn m_key_opens_the_context_menu() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);
        app.handle_key(KeyEvent::new(KeyCode::Char('M'), KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::ContextMenu { .. }), "M opened the menu");
        // Also works when the terminal doesn't tag the uppercase char with SHIFT.
        app.popup = Popup::None;
        app.handle_key(KeyEvent::new(KeyCode::Char('M'), KeyModifiers::NONE)).unwrap();
        assert!(matches!(app.popup, Popup::ContextMenu { .. }), "M works without a SHIFT tag too");
    }

    #[test]
    fn the_menu_shortcuts_entry_opens_the_bookmarks() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        app.run_menu_item(MenuItem::Shortcuts).unwrap();
        assert!(matches!(app.popup, Popup::Shortcuts { .. }), "opened the shortcuts menu");
    }

    #[test]
    fn the_menu_toggles_dotfiles_for_the_focused_pane_only() {
        let (_d, mut app) = app_with(&["a.txt", ".hidden"]);
        app.focus(FocusedPane::Left);
        // Counts include the `..` row: 2 files + `..` = 3.
        assert_eq!(app.left.active_ref().entries.len(), 3);

        app.run_menu_item(MenuItem::HiddenToggle).unwrap();
        assert_eq!(app.left.active_ref().entries.len(), 2, "dotfile hidden here");
        assert_eq!(app.right.active_ref().entries.len(), 3, "and not in the other pane");
    }

    /// Dragging from one pane to the other should raise the transfer
    /// confirmation, not act silently.
    #[test]
    fn dragging_between_panes_offers_a_transfer() {
        let (_l, r, mut app) = app_two_dirs(&["doc.txt"], &[]);
        let _ = render(&mut app, 100, 40);
        let (left, right) = (app.layout_rects.left, app.layout_rects.right);

        // Row 1 is `..`; press on the file on row 2.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 2));
        assert!(app.file_drag.is_some(), "pressing on an entry arms a drag");

        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            right.x + 5,
            right.y + 2,
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), right.x + 5, right.y + 2));

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

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 2));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), right.x + 5, right.y + 2));
        let mut up = mouse(MouseEventKind::Up(MouseButton::Left), right.x + 5, right.y + 2);
        up.modifiers = KeyModifiers::SHIFT;
        app.handle_mouse(up);

        let Popup::ConfirmTransfer { op, .. } = &app.popup else { panic!("no confirmation") };
        assert_eq!(*op, PendingOp::Move);
    }

    /// Regression: a click that the terminal reported with a stray same-row
    /// Drag used to mark that row. Clicking file A then file B then A must
    /// leave the marks untouched — a bare click is not a mark.
    #[test]
    fn clicking_files_never_marks_them() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt"]);
        let _ = render(&mut app, 100, 40);
        let left = app.layout_rects.left;
        // Rows: 1 = `..`, 2 = a.txt, 3 = b.txt, 4 = c.txt.
        for cy in [left.y + 2, left.y + 3, left.y + 2] {
            // A press, a same-row drag (the terminal's jitter), then release.
            app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 3, cy));
            app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), left.x + 3, cy));
            app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), left.x + 3, cy));
        }
        assert_eq!(app.active_pane().unwrap().mark_count(), 0, "clicks must not mark");
    }

    /// The `..` row navigates up on a single click, and can never be marked.
    #[test]
    fn the_parent_row_navigates_up_and_is_never_marked() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("sub/inner.txt"), b"x").unwrap();
        let start = d.path().join("sub");
        let mut app = App::new(start.clone(), start, cian_lua::Config::default()).unwrap();
        let _ = render(&mut app, 100, 40);
        let left = app.layout_rects.left;
        // The first row is `..`; a single click steps up to the parent.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 3, left.y + 1));
        assert!(!app.left.active_ref().cwd.ends_with("sub"), "left sub via ..");
        // Marking the `..` row (e.g. via Space on it) is a no-op.
        if let Some(p) = app.active_pane_mut() {
            p.cursor = 0; // back onto `..`
            p.toggle_mark_at(0);
        }
        assert_eq!(app.active_pane().unwrap().mark_count(), 0, "`..` is never marked");
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

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 2));
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
    fn comparing_a_directory_against_a_file_is_refused() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::create_dir(l.path().join("adir")).unwrap();
        std::fs::write(r.path().join("b.txt"), "x").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();

        app.handle_key(code(KeyCode::Char('='))).unwrap();
        assert!(matches!(app.popup, Popup::None));
        assert!(app.message.as_deref().unwrap().contains("not one of each"));
    }

    #[test]
    fn comparing_two_directories_lists_the_differences() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::create_dir(l.path().join("proj")).unwrap();
        std::fs::create_dir(r.path().join("proj")).unwrap();
        std::fs::write(l.path().join("proj/same.txt"), b"xy").unwrap();
        std::fs::write(r.path().join("proj/same.txt"), b"xy").unwrap();
        // Equal size AND mtime, so the quick compare treats them as identical.
        let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        cian_core::dirdiff::set_mtime(&l.path().join("proj/same.txt"), t).unwrap();
        cian_core::dirdiff::set_mtime(&r.path().join("proj/same.txt"), t).unwrap();
        std::fs::write(l.path().join("proj/only_left.txt"), b"l").unwrap();
        std::fs::write(r.path().join("proj/changed.txt"), b"aaaa").unwrap();
        std::fs::write(l.path().join("proj/changed.txt"), b"a").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();
        // Cursor on "proj" in each pane (index 0 is the `..` row).
        app.left.active_mut().cursor = 1;
        app.right.active_mut().cursor = 1;

        app.handle_key(code(KeyCode::Char('='))).unwrap();
        assert!(app.diff_job.is_some(), "comparison started on a worker");
        // Drain the worker.
        for _ in 0..200 {
            if app.diff_job.is_none() { break; }
            app.poll_diff_job();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let Popup::DirCompare { entries, .. } = &app.popup else {
            panic!("expected the comparison, got {:?}", app.popup)
        };
        let paths: Vec<String> =
            entries.iter().map(|e| e.rel.display().to_string().replace('\\', "/")).collect();
        // Paths are relative to the compared folders (proj), not the roots.
        assert!(paths.contains(&"only_left.txt".to_string()), "{:?}", paths);
        assert!(paths.contains(&"changed.txt".to_string()), "{:?}", paths);
        assert!(!paths.contains(&"same.txt".to_string()), "identical file omitted: {:?}", paths);
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
        // Cursor on the first file (index 0 is the `..` row): old.txt.
        app.active_pane_mut().unwrap().cursor = 1;
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
        app.active_pane_mut().unwrap().cursor = 1; // notes.txt (index 0 is `..`)

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
        app.active_pane_mut().unwrap().cursor = 1; // log.txt (index 0 is `..`)

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
        app.active_pane_mut().unwrap().cursor = 1; // secret.txt (index 0 is `..`)
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
        app.active_pane_mut().unwrap().cursor = 1; // the file (index 0 is `..`)
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

    #[test]
    fn o_on_a_file_mirrors_the_directory_to_the_other_pane() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(l.path().join("doc.txt"), b"x").unwrap();
        std::fs::create_dir(r.path().join("elsewhere")).unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();
        app.focus(FocusedPane::Left);
        app.active_pane_mut().unwrap().cursor = 1; // doc.txt (a file; index 0 is `..`)
        app.open_in_other_pane(false).unwrap();
        assert_eq!(
            app.right.active_ref().cwd,
            app.left.active_ref().cwd,
            "the other pane lines up on this directory"
        );
    }

    #[test]
    fn f_keys_manage_file_tabs() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        assert_eq!(app.left.tabs.len(), 1);
        app.handle_key(code(KeyCode::F(9))).unwrap(); // new tab
        assert_eq!(app.left.tabs.len(), 2);
        assert_eq!(app.left.active, 1);
        app.handle_key(code(KeyCode::F(1))).unwrap(); // previous
        assert_eq!(app.left.active, 0);
        app.handle_key(code(KeyCode::F(2))).unwrap(); // next
        assert_eq!(app.left.active, 1);
    }

    #[test]
    fn ctrl_digit_no_longer_jumps_file_tabs() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        app.left.add_clone().unwrap(); // now 2 tabs, active 1
        // Ctrl+1 used to select tab 0; it must not any more.
        app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(app.left.active, 1, "Ctrl+1 is no longer a tab jump");
    }

    #[test]
    fn the_default_home_prefers_config_then_desktop() {
        // A configured home directory wins when it exists.
        let d = tempfile::tempdir().unwrap();
        let mut config = cian_lua::Config::default();
        config.options.home = Some(d.path().display().to_string());
        assert_eq!(default_home(&config), d.path());

        // A configured but missing directory falls through (to Desktop/home/.).
        let mut config = cian_lua::Config::default();
        config.options.home = Some("/definitely/not/here".into());
        let fallback = default_home(&config);
        assert_ne!(fallback, PathBuf::from("/definitely/not/here"));
    }

    #[test]
    fn a_notice_can_be_copied_then_closes() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.popup = Popup::Notice { lines: vec!["abc123".into()] };
        app.handle_key(code(KeyCode::Char('y'))).unwrap();
        assert!(matches!(app.popup, Popup::None));
        assert_eq!(app.message.as_deref(), Some("copied"));
    }

    #[test]
    fn double_clicking_a_directory_enters_it() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("sub/inner.txt"), b"x").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p.clone(), cian_lua::Config::default()).unwrap();
        let _ = render(&mut app, 100, 40);
        let r = app.layout_rects.left;
        // Row 1 is the `..` row; "sub" (dirs first) is on row 2.
        let (cx, cy) = (r.x + 3, r.y + 2);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), cx, cy));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), cx, cy));
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), cx, cy));

        // Compare by the final component (the pane canonicalises differently
        // per platform than std::fs::canonicalize).
        assert!(
            app.left.active_ref().cwd.ends_with("sub"),
            "double-click entered the directory: {:?}",
            app.left.active_ref().cwd
        );
    }

    #[test]
    fn a_slow_second_click_is_not_a_double_click() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p.clone(), cian_lua::Config::default()).unwrap();
        let _ = render(&mut app, 100, 40);
        let root = app.left.active_ref().cwd.clone();
        let r = app.layout_rects.left;
        // Row 2 is "sub"; row 1 is the `..` row (which would navigate up).
        let (cx, cy) = (r.x + 3, r.y + 2);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), cx, cy));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), cx, cy));
        // Age the first click past the double-click window.
        app.last_click = Some((Instant::now() - Duration::from_secs(2), cy));
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), cx, cy));
        assert_eq!(app.left.active_ref().cwd, root,
            "a slow second click just selects, does not enter");
    }

    #[test]
    fn clicking_a_tab_switches_to_it() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        app.left.add_clone().unwrap(); // second tab, now active
        // Wide enough that the first tab is not collapsed into a +N marker.
        let _ = render(&mut app, 300, 40);
        assert_eq!(app.left.active, 1);

        let (_, _, r) = app
            .tab_rects
            .iter()
            .copied()
            .find(|(p, i, _)| *p == FocusedPane::Left && *i == 0)
            .expect("a rect for the left pane's first tab");
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), r.x + 1, r.y));
        assert_eq!(app.left.active, 0, "clicking the first tab selected it");
        assert_eq!(app.focused, FocusedPane::Left);
    }

    #[test]
    fn the_context_menu_runs_the_item_that_was_clicked() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        let _ = render(&mut app, 100, 40);
        // Open the menu at a known spot, then render so menu_rect is set.
        app.open_context_menu(10, 10);
        let _ = render(&mut app, 100, 40);
        let m = app.menu_rect;
        // The Quit item is second-to-last; click its row.
        let (quit_idx, _) = {
            let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
            items.iter().enumerate().find(|(_, it)| **it == MenuItem::Quit).expect("quit item")
        };
        let row = m.y + 1 + quit_idx as u16;
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), m.x + 2, row));
        assert!(matches!(app.popup, Popup::ConfirmQuit), "clicking Quit opened the confirm");
    }

    #[test]
    fn clicking_off_the_context_menu_dismisses_it() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.open_context_menu(10, 10);
        let _ = render(&mut app, 100, 40);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 1, 1));
        assert!(matches!(app.popup, Popup::None));
    }

    // ---- keyboard pane resize ----

    fn ctrl_shift(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL | KeyModifiers::SHIFT)
    }

    #[test]
    fn ctrl_shift_arrows_resize_the_file_panes() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        assert_eq!(app.panes_pct, 50);
        assert_eq!(app.main_pct, 60);

        // Right pushes the left|right divider right → left pane grows.
        app.handle_key(ctrl_shift(KeyCode::Right)).unwrap();
        assert!(app.panes_pct > 50, "left grew: {}", app.panes_pct);
        let wider = app.panes_pct;
        app.handle_key(ctrl_shift(KeyCode::Left)).unwrap();
        assert!(app.panes_pct < wider, "left shrank back");

        // Down grows the file area (files|shell divider moves down).
        app.handle_key(ctrl_shift(KeyCode::Down)).unwrap();
        assert!(app.main_pct > 60, "files grew: {}", app.main_pct);
        app.handle_key(ctrl_shift(KeyCode::Up)).unwrap();
        app.handle_key(ctrl_shift(KeyCode::Up)).unwrap();
        assert!(app.main_pct < 60, "and shrank past the start");
    }

    #[test]
    fn resize_is_clamped_so_a_pane_never_vanishes() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        for _ in 0..50 {
            app.handle_key(ctrl_shift(KeyCode::Left)).unwrap();
        }
        assert_eq!(app.panes_pct, MIN_SPLIT_PCT, "cannot shrink below the floor");
        for _ in 0..50 {
            app.handle_key(ctrl_shift(KeyCode::Right)).unwrap();
        }
        assert_eq!(app.panes_pct, 100 - MIN_SPLIT_PCT, "nor grow past the ceiling");
    }

    #[test]
    fn from_the_shell_up_down_resizes_the_shell_area() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        assert_eq!(app.main_pct, 60);
        // With no inner split, Up grows the shell (files|shell divider up).
        app.handle_key(ctrl_shift(KeyCode::Up)).unwrap();
        assert!(app.main_pct < 60, "shell grew: {}", app.main_pct);
    }

    // ---- editing, confirms, search, history refinements ----

    #[test]
    fn the_text_field_edits_at_the_caret_not_only_the_end() {
        let (_d, mut app) = app_with(&["report.txt"]);
        app.active_pane_mut().unwrap().cursor = 1; // report.txt (index 0 is `..`)
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
        app.active_pane_mut().unwrap().cursor = 1; // old.txt (index 0 is `..`)
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
    fn the_viewer_line_visual_selects_and_copies_a_range() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30); // size viewer_rect so motion works

        // Move to line 1 (two), start line-visual, extend to line 2 (three).
        app.handle_key(key('j')).unwrap();
        app.handle_key(key('V')).unwrap();
        app.handle_key(key('j')).unwrap();
        assert!(
            matches!(app.popup, Popup::Viewer { visual: Some(ViewVisual::Line), .. }),
            "line-visual is active"
        );
        app.handle_key(key('y')).unwrap();
        assert_eq!(app.message.as_deref(), Some("copied"));
        // Visual ends after the copy; the viewer stays open.
        assert!(matches!(app.popup, Popup::Viewer { visual: None, .. }));
    }

    #[test]
    fn the_viewer_shift_arrow_selects_characters() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "hello world\nsecond\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);

        // Shift+Right three times: a character-wise selection begins at col 0
        // and the cursor advances, extending it.
        for _ in 0..3 {
            app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT)).unwrap();
        }
        match &app.popup {
            Popup::Viewer { visual: Some(ViewVisual::Char), anchor, col, .. } => {
                assert_eq!(*anchor, (0, 0), "anchored where selection began");
                assert_eq!(*col, 3, "cursor advanced three chars");
            }
            other => panic!("expected a char selection, got {:?}", other),
        }
        // A plain motion keeps the vim-style selection; `y` copies and ends it.
        app.handle_key(key('y')).unwrap();
        assert_eq!(app.message.as_deref(), Some("copied"));
        assert!(matches!(app.popup, Popup::Viewer { visual: None, .. }));
    }

    #[test]
    fn the_viewer_alt_arrow_and_alt_drag_select_a_block() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "hello world\nsecond line\nthird row!!\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);

        // Alt+Down then Alt+Right builds a rectangle from the cursor.
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT)).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)).unwrap();
        match &app.popup {
            Popup::Viewer { visual: Some(ViewVisual::Block), anchor, line, col, .. } => {
                assert_eq!(*anchor, (0, 0));
                assert_eq!((*line, *col), (1, 2), "block cursor advanced down 1, right 2");
            }
            other => panic!("expected a block selection, got {:?}", other),
        }
        app.handle_key(code(KeyCode::Esc)).unwrap(); // drop the selection

        // Alt+drag also makes a block selection.
        let body = app.viewer_rect;
        let x0 = body.x + app.viewer_gutter;
        let mut down = mouse(MouseEventKind::Down(MouseButton::Left), x0 + 1, body.y);
        down.modifiers = KeyModifiers::ALT;
        app.handle_mouse(down);
        let mut drag = mouse(MouseEventKind::Drag(MouseButton::Left), x0 + 4, body.y + 2);
        drag.modifiers = KeyModifiers::ALT;
        app.handle_mouse(drag);
        assert!(matches!(app.popup, Popup::Viewer { visual: Some(ViewVisual::Block), .. }),
            "alt-drag makes a block selection");
    }

    #[test]
    fn the_viewer_mouse_drag_selects_characters() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "hello world\nsecond line\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);
        let body = app.viewer_rect;
        let x0 = body.x + app.viewer_gutter;
        // Press on (line 0, char 2), drag to (line 0, char 8): a char selection.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x0 + 2, body.y));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), x0 + 8, body.y));
        match &app.popup {
            Popup::Viewer { visual: Some(ViewVisual::Char), anchor, line, col, .. } => {
                assert_eq!(*anchor, (0, 2), "anchored at the press char");
                assert_eq!((*line, *col), (0, 8), "cursor at the drag char");
            }
            other => panic!("expected a char selection, got {:?}", other),
        }
        // Right-click copies the selection.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), x0 + 8, body.y));
        assert_eq!(app.message.as_deref(), Some("copied"));
    }

    /// Drive the viewer with a sequence of plain-char keys.
    fn vkeys(app: &mut App, s: &str) {
        for c in s.chars() {
            app.handle_key(key(c)).unwrap();
        }
    }

    #[test]
    fn the_viewer_searches_and_jumps_between_matches() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "alpha\nbeta needle\ngamma\nneedle again\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);

        // /needle<CR> jumps to the first match (line 1, col 5).
        app.handle_key(key('/')).unwrap();
        vkeys(&mut app, "needle");
        app.handle_key(code(KeyCode::Enter)).unwrap();
        if let Popup::Viewer { line, col, .. } = &app.popup {
            assert_eq!((*line, *col), (1, 5), "first match");
        } else {
            panic!("viewer");
        }
        // n advances to the next match (line 3, col 0).
        app.handle_key(key('n')).unwrap();
        if let Popup::Viewer { line, col, .. } = &app.popup {
            assert_eq!((*line, *col), (3, 0), "second match");
        } else {
            panic!("viewer");
        }
    }

    #[test]
    fn the_viewer_goto_line_and_bracket_match() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "fn f() {\n    body\n}\nfour\nfive\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);

        // 4G jumps to line 4 (0-based index 3).
        vkeys(&mut app, "4");
        app.handle_key(key('G')).unwrap();
        if let Popup::Viewer { line, .. } = &app.popup {
            assert_eq!(*line, 3, "goto line 4");
        } else {
            panic!("viewer");
        }
        // Back to the top, move onto the `{` (col 7 of "fn f() {"), then % to
        // its matching `}` on line 2.
        app.handle_key(key('g')).unwrap();
        vkeys(&mut app, "lllllll"); // 7 × l → col 7 = '{'
        if let Popup::Viewer { col, .. } = &app.popup {
            assert_eq!(*col, 7, "cursor on the brace");
        }
        vkeys(&mut app, "%");
        if let Popup::Viewer { line, .. } = &app.popup {
            assert_eq!(*line, 2, "matching brace is on line 2");
        } else {
            panic!("viewer");
        }
    }

    #[test]
    fn the_viewer_char_visual_yanks_across_lines() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "abcd\nefgh\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);
        // From (0,1)=b, char-visual to (1,1)=f → "bcd\nef".
        app.handle_key(key('l')).unwrap();
        app.handle_key(key('v')).unwrap();
        app.handle_key(key('j')).unwrap();
        // cursor col follows the goal (1) on line 1.
        let text = if let Popup::Viewer { view, line, col, visual, anchor, .. } = &app.popup {
            assert_eq!((*line, *col), (1, 1));
            let (s, e) = order_pos(*anchor, (*line, *col));
            assert!(visual.is_some());
            viewer_charwise(&view.lines, s, e)
        } else {
            panic!("viewer")
        };
        assert_eq!(text, "bcd\nef");
    }

    #[test]
    fn e_opens_the_encoding_picker_and_applies_the_choice() {
        let d = tempfile::tempdir().unwrap();
        // "日本語" in Shift_JIS: mojibake as UTF-8 until switched.
        std::fs::write(d.path().join("s.txt"), [0x93u8, 0xfa, 0x96, 0x7b, 0x8c, 0xea, b'\n']).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();

        // `e` opens the picker (a list), not an immediate cycle.
        app.handle_key(key('e')).unwrap();
        assert!(
            matches!(app.popup, Popup::EncodingPicker { target: EncTarget::Viewer(_), .. }),
            "e opens the picker targeting the viewer"
        );
        // Move to Shift_JIS and confirm; the viewer comes back re-decoded.
        let sjis = cian_core::viewer::TextEncoding::ALL
            .iter()
            .position(|e| *e == cian_core::viewer::TextEncoding::ShiftJis)
            .unwrap();
        if let Popup::EncodingPicker { cursor, .. } = &mut app.popup {
            *cursor = sjis;
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let Popup::Viewer { view, .. } = &app.popup else { panic!("viewer restored") };
        assert_eq!(view.encoding, cian_core::viewer::TextEncoding::ShiftJis);
        assert_eq!(view.lines[0], "日本語");
    }

    #[test]
    fn cancelling_the_encoding_picker_restores_the_viewer_unchanged() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("s.txt"), b"plain\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        app.handle_key(key('e')).unwrap();
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "Esc returns to the viewer");
    }

    #[test]
    fn shift_enter_reveals_the_viewed_file_in_the_pane() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("sub").join("deep.txt"), "content\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        // Open the file directly in the viewer, then Shift+Enter to reveal it.
        app.open_viewer_at(&d.path().join("sub").join("deep.txt"), "deep.txt", 0);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::None), "viewer closed");
        let pane = app.active_pane().unwrap();
        assert!(pane.cwd.ends_with("sub"), "pane moved into the file's dir: {:?}", pane.cwd);
        assert_eq!(pane.selected().map(|e| e.name.as_str()), Some("deep.txt"));
    }

    #[test]
    fn ctrl_n_steps_through_grep_hits_in_the_viewer() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "NEEDLE one\n").unwrap();
        std::fs::write(d.path().join("b.txt"), "two NEEDLE\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.start_find("NEEDLE", cian_core::search::Mode::Content);
        drain_find(&mut app);
        // Sort of results is by rel path, so a.txt is first. Open it.
        if let Popup::FindResults { cursor, .. } = &mut app.popup {
            *cursor = 0;
        }
        app.open_find_hit().unwrap();
        let first = match &app.popup {
            Popup::Viewer { title, .. } => title.clone(),
            _ => panic!("viewer"),
        };
        // Ctrl+n → the other file's hit.
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)).unwrap();
        let second = match &app.popup {
            Popup::Viewer { title, .. } => title.clone(),
            other => panic!("expected viewer, got {:?}", other),
        };
        assert_ne!(first, second, "Ctrl+n moved to the other hit");
        // Esc still returns to the (stepped) results list.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::FindResults { .. }));
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
