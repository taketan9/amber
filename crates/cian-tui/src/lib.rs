use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
use ratatui::{Frame, Terminal};
use serde::{Deserialize, Serialize};

mod panes;
mod theme;
use theme::*;

mod util;
use util::{
    centered_rect, glob_match, order_pos, pad_to, truncate, truncate_middle, union_rect,
    viewer_charwise, viewer_find, viewer_match_bracket, viewer_paragraph, viewer_word_back,
    viewer_word_forward, vlen, width, wrap_str,
};

mod ai;
mod markdown;
mod viewer;
mod ssh;
mod gitui;
mod commands;
mod actions;
mod count;
mod edit;
mod macro_run;
mod session;
mod mouse;
mod menu;
mod keys;
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
    /// Synchronize/broadcast input: keystrokes go to every pane in the active
    /// tab at once. Only meaningful with more than one pane.
    broadcast: bool,
}

/// A PTY spawn running on a background thread, plus what to do with the
/// session once it arrives.
/// The result channel for an async remote directory listing: `(cwd, entries)`
/// on success, an error string otherwise.
type RemoteLsRx = std::sync::mpsc::Receiver<Result<(String, Vec<cian_scp::RemoteEntry>), String>>;

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
    /// A split of tab `tab`. `leaf` is the specific leaf node to split (so a
    /// macro's `from = N` targets the intended pane regardless of what is active
    /// when the spawn lands); `None` splits whatever is active at install time.
    /// `ratio` is the percentage the source pane keeps (for even grid thirds).
    Split { tab: usize, dir: SplitDir, leaf: Option<usize>, ratio: u16 },
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
    /// Overwrite confirmation for a copy-across from a comparison view (`<`/`>`
    /// in the file diff or the folder compare). `back` is the comparison popup
    /// to restore whether the copy is confirmed or cancelled.
    ConfirmDiffCopy { src: PathBuf, dst: PathBuf, is_dir: bool, back: Box<Popup> },
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
    /// The macro launcher: pick a macro from `macro.lua` to run. Names are held
    /// here so the renderer stays independent of `App`.
    Macros { cursor: usize, names: Vec<String> },
    /// A git commit log (repo-wide or one file's history). Enter shows the
    /// selected commit's diff in the viewer.
    GitLog {
        title: String,
        dir: PathBuf,
        commits: Vec<cian_core::git::Commit>,
        cursor: usize,
        scroll: usize,
        /// Which VCS produced the log — decides how Enter shows a commit.
        vcs: Vcs,
    },
    /// An image shown as half-block cells (works in any 24-bit terminal). The
    /// decoded grid is cached for the size it was last drawn at; a resize or a
    /// decode failure updates it in the render.
    ImageView {
        path: PathBuf,
        title: String,
        /// `(cols, rows, thumbnail)` cached for the last drawn inner size.
        shown: Option<(u16, u16, cian_core::image::Thumb)>,
        /// Why the image could not be decoded, if it could not.
        error: Option<String>,
    },
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
        /// True for a Markdown file, so `p` can toggle a rendered preview.
        markdown: bool,
        /// Showing the rendered Markdown preview rather than the raw source.
        ///
        /// The preview is a *full* viewer: `view.lines` is swapped for the
        /// rendered plain text (so the cursor, visual selection, `/` search and
        /// mouse all work over the rendered document), and `md_styles` carries
        /// the per-character colour applied underneath. The render owns the
        /// swap: it re-renders when `preview` flips or the width changes.
        preview: bool,
        /// The original source lines, kept so leaving preview can restore them
        /// (and so the preview can be re-wrapped when the width changes).
        source: Vec<String>,
        /// Per-character base style parallel to `view.lines` while previewing;
        /// empty in source mode.
        md_styles: Vec<Vec<Style>>,
        /// The inner width the preview was last wrapped to, so the render can
        /// tell when a resize means it must re-render.
        md_width: u16,
        /// Per-line git blame, shown as a left gutter when non-empty. Toggled
        /// with `B`; empty means off.
        blame: Vec<cian_core::git::BlameLine>,
        /// Syntax-highlight language for this file, if recognised. Drives the
        /// per-character colours in source (non-preview) mode.
        hl_lang: Option<cian_core::highlight::Lang>,
        /// Cached per-character highlight styles, parallel to `view.lines`.
        /// Empty until computed (and cleared on edit / re-decode so it refreshes).
        hl: Vec<Vec<Style>>,
        /// True for a real text file that can be edited and saved in place
        /// (false for a hex dump, an extracted Office document, etc).
        editable: bool,
        /// In the built-in plain-text editor: keys insert/delete instead of
        /// navigating. Toggled with `i`; left with `Esc`.
        editing: bool,
        /// Unsaved edits are present.
        dirty: bool,
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
        /// The two files on disk, so the encoding can be switched (re-diff) and
        /// the result saved.
        left_path: PathBuf,
        right_path: PathBuf,
        /// The encoding both sides were decoded with.
        encoding: cian_core::viewer::TextEncoding,
        result: cian_core::diff::Diff,
        folded: Vec<cian_core::diff::Row>,
        fold: bool,
        scroll: usize,
        /// A confirmed text search; rows containing it are highlighted and
        /// `n`/`N` step between them. `None` when no search is active.
        find: Option<String>,
        /// While typing a `/` search, the text entered so far.
        find_input: Option<String>,
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
    /// Browse a remote directory over SFTP to choose files to download. `cwd` is
    /// the remote directory; `marked` are file names selected for download.
    RemoteBrowser {
        label: String,
        cwd: String,
        entries: Vec<cian_scp::RemoteEntry>,
        cursor: usize,
        scroll: usize,
        marked: std::collections::BTreeSet<String>,
        loading: bool,
    },
    /// Pick where a set of remote files download to: the left/right pane, the
    /// Desktop, or a typed path. `files` are the chosen remote file paths.
    LocalDest { files: Vec<String>, cursor: usize },
    /// The command-snippet launcher: pick one to send to the active shell.
    /// Items come from `config.snippets`, filtered by `filter`.
    Snippets { cursor: usize, filter: String },
    /// Confirm sending a snippet flagged `confirm = true` (a destructive one).
    ConfirmSnippet { name: String, cmd: String, enter: bool },
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
    /// A stashed file diff to re-run under the chosen encoding.
    Diff(Box<Popup>),
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
    /// The commit log (repo, or the selected file's history).
    GitHistory,
    /// The selected file's working-tree diff vs HEAD.
    GitDiff,
    /// A submenu grouping the svn actions (add / revert / update / commit …).
    SvnMenu,
    /// `svn add` the selection.
    SvnAdd,
    /// `svn revert` the selection (discard local changes).
    SvnRevert,
    /// `svn resolve --accept working` the selection.
    SvnResolve,
    /// The selected file's working-copy diff vs BASE.
    SvnDiff,
    /// The commit log (working copy, or the selected file's history).
    SvnLog,
    /// `svn update` the working copy.
    SvnUpdate,
    /// `svn commit` the selection (prompts for a message).
    SvnCommit,
    /// Pattern-based bulk rename of the marked files (`:brename`).
    BulkRename,
    /// Open the command-snippet launcher (`:snip`).
    Snippets,
    /// Open the layout-macro launcher (`@` / `:macros`).
    Macros,
    /// Open the shortcuts / bookmarks menu (the `s` key).
    Shortcuts,
    /// A submenu grouping the compress-to-archive actions.
    CompressMenu,
    /// Compress the selection to a `.zip`.
    CompressZip,
    /// Compress the selection to a password-protected `.zip`.
    CompressZipEnc,
    /// Compress the selection to a `.tar.gz`.
    CompressTarGz,
    /// Extract the archive under the cursor into a fresh sub-folder.
    Extract,
    /// Count files/steps under the selection (`:count`).
    Count,
    /// A submenu grouping the AI actions (drills down when chosen).
    AiMenu,
    /// A submenu grouping the file-transfer actions.
    SendMenu,
    /// A submenu grouping the shell window actions (splits, tabs, zoom).
    WindowMenu,
    /// A submenu grouping the less-common file actions (copy/move to other
    /// pane, copy to a path, bulk rename).
    FileMenu,
    /// A submenu grouping archive actions (compress ▸, extract here).
    ArchiveMenu,
    /// A submenu grouping the read-only "inspect" actions (attributes, hash,
    /// compare, count, find duplicates).
    InspectMenu,
    /// A submenu grouping view/misc actions (show hidden, language, copy path).
    ViewMenu,
    /// A submenu grouping the shell session actions (logging, encoding).
    SessionMenu,
    /// Copy the selection's path text to the system clipboard (the `p` key).
    CopyPathText,
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
    /// Start broadcasting input to every pane in the active shell tab.
    SyncStart,
    /// Stop broadcasting input.
    SyncStop,
    /// Goes back up from a submenu to its parent.
    Back,
    Quit,
    Manual,
}

impl MenuItem {
    /// Group items open a submenu instead of acting; this is their marker.
    fn is_group(self) -> bool {
        matches!(
            self,
            MenuItem::AiMenu
                | MenuItem::SendMenu
                | MenuItem::WindowMenu
                | MenuItem::FileMenu
                | MenuItem::ArchiveMenu
                | MenuItem::InspectMenu
                | MenuItem::ViewMenu
                | MenuItem::SessionMenu
                | MenuItem::GitMenu
                | MenuItem::SvnMenu
                | MenuItem::CompressMenu
        )
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
            MenuItem::CompressMenu => tr(lang, "Compress ▸", "圧縮 ▸"),
            MenuItem::CompressZip => tr(lang, "→ .zip", "→ .zip"),
            MenuItem::CompressZipEnc => tr(lang, "→ .zip  (password)", "→ .zip  (パスワード)"),
            MenuItem::CompressTarGz => tr(lang, "→ .tar.gz", "→ .tar.gz"),
            MenuItem::Extract => tr(lang, "Extract here", "ここに解凍"),
            MenuItem::Count => tr(lang, "Count files & steps", "ファイル・ステップ数を数える"),
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
            MenuItem::AiRename => tr(lang, "AI rename", "AIリネーム"),
            MenuItem::AiSearch => tr(lang, "Semantic search", "セマンティック検索"),
            MenuItem::GitMenu => tr(lang, "Git ▸", "Git ▸"),
            MenuItem::GitStage => tr(lang, "Stage  (git add)", "ステージ  (git add)"),
            MenuItem::GitUnstage => tr(lang, "Unstage  (git reset)", "アンステージ  (git reset)"),
            MenuItem::GitDiscard => tr(lang, "Discard changes  (git checkout)", "変更を破棄  (git checkout)"),
            MenuItem::GitHistory => tr(lang, "History / log  (git log)", "履歴 / ログ  (git log)"),
            MenuItem::GitDiff => tr(lang, "Diff vs HEAD  (git diff)", "HEADとの差分  (git diff)"),
            MenuItem::SvnMenu => tr(lang, "SVN ▸", "SVN ▸"),
            MenuItem::SvnAdd => tr(lang, "Add  (svn add)", "追加  (svn add)"),
            MenuItem::SvnRevert => tr(lang, "Revert changes  (svn revert)", "変更を破棄  (svn revert)"),
            MenuItem::SvnResolve => tr(lang, "Resolve conflict  (svn resolve)", "競合を解決  (svn resolve)"),
            MenuItem::SvnDiff => tr(lang, "Diff vs BASE  (svn diff)", "BASEとの差分  (svn diff)"),
            MenuItem::SvnLog => tr(lang, "History / log  (svn log)", "履歴 / ログ  (svn log)"),
            MenuItem::SvnUpdate => tr(lang, "Update  (svn update)", "更新  (svn update)"),
            MenuItem::SvnCommit => tr(lang, "Commit…  (svn commit)", "コミット…  (svn commit)"),
            MenuItem::BulkRename => tr(lang, "Bulk rename…  (:brename)", "一括リネーム…  (:brename)"),
            MenuItem::Snippets => tr(lang, "Snippets → shell  (:snip)", "スニペット → シェル  (:snip)"),
            MenuItem::Macros => tr(lang, "Macros → layout  (@)", "マクロ → レイアウト  (@)"),
            MenuItem::Shortcuts => tr(lang, "Shortcuts  (s)", "ショートカット  (s)"),
            // Ⓒ stands in for crmaine's icon — a terminal menu cannot embed the
            // PNG/SVG, which is itself just the "CRMAINE" wordmark as text, so a
            // circled C echoes it most closely.
            MenuItem::AiMenu => tr(lang, "Ⓒ crmaine - Ajent ▸", "Ⓒ crmaine - Ajent ▸"),
            MenuItem::SendMenu => tr(lang, "Transfer ▸", "転送 ▸"),
            MenuItem::WindowMenu => tr(lang, "Window ▸", "ウィンドウ ▸"),
            MenuItem::FileMenu => tr(lang, "File ▸", "ファイル操作 ▸"),
            MenuItem::ArchiveMenu => tr(lang, "Archive ▸", "書庫 ▸"),
            MenuItem::InspectMenu => tr(lang, "Inspect ▸", "調べる ▸"),
            MenuItem::ViewMenu => tr(lang, "View ▸", "表示 ▸"),
            MenuItem::SessionMenu => tr(lang, "Session ▸", "セッション ▸"),
            MenuItem::CopyPathText => tr(lang, "Copy path text  (p)", "パスをコピー  (p)"),
            MenuItem::ShellSplitLR => tr(lang, "Split left / right  (S-F8)", "左右に分割  (S-F8)"),
            MenuItem::ShellSplitTB => tr(lang, "Split top / bottom  (S-F9)", "上下に分割  (S-F9)"),
            MenuItem::ShellNewTab => tr(lang, "New tab  (F9)", "新規タブ  (F9)"),
            MenuItem::ShellCloseSplit => tr(lang, "Close split pane  (S-F10)", "分割パネルを閉じる  (S-F10)"),
            MenuItem::ShellCloseTab => tr(lang, "Close tab  (F10)", "タブを閉じる  (F10)"),
            MenuItem::ShellZoom => tr(lang, "Zoom  (F12)", "ズーム  (F12)"),
            MenuItem::SyncStart => tr(lang, "Synchronize input  ⇄", "同時入力を開始  ⇄"),
            MenuItem::SyncStop => tr(lang, "Stop synchronize  ⇄", "同時入力を停止  ⇄"),
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
    /// Pushed onto the undo stack if the op finishes cleanly (set for moves).
    undo: Option<UndoAction>,
}

/// A reversible file operation, for the `u` undo stack. Deletes are excluded —
/// they go to the OS trash, which has its own restore.
#[derive(Debug, Clone)]
enum UndoAction {
    /// Undo by renaming `to` back to `from`.
    Rename { from: PathBuf, to: PathBuf },
    /// Undo by removing what was just created.
    Created { path: PathBuf },
    /// Undo by moving each `.0` (where it is now) back to `.1` (where it was).
    Moved { pairs: Vec<(PathBuf, PathBuf)> },
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
/// Per-pane background tints: dark enough for foreground text to stay readable
/// (luminance < 90), and pairwise distinct enough to tell two panes apart at a
/// glance — both enforced by a test. A richer, more saturated spread than the
/// original set, and more of them.
const PANE_BG_PRESETS: [(&str, Option<Color>); 14] = [
    ("default", None),
    ("navy", Some(Color::Rgb(10, 40, 140))),
    ("ocean", Some(Color::Rgb(15, 95, 160))),
    ("teal", Some(Color::Rgb(10, 110, 110))),
    ("forest", Some(Color::Rgb(25, 120, 25))),
    ("moss", Some(Color::Rgb(60, 100, 40))),
    ("olive", Some(Color::Rgb(110, 90, 10))),
    ("mocha", Some(Color::Rgb(95, 60, 35))),
    ("rust", Some(Color::Rgb(150, 50, 15))),
    ("crimson", Some(Color::Rgb(160, 25, 45))),
    // Named for Taketan's own project, crmaine — the emoticon marks it as a nod.
    ("crmaine (^_-)", Some(Color::Rgb(140, 15, 85))),
    ("plum", Some(Color::Rgb(85, 20, 150))),
    ("steel", Some(Color::Rgb(40, 60, 90))),
    ("slate", Some(Color::Rgb(70, 85, 120))),
];

/// Resolve a macro's `bg = "…"`: a preset name (matched on its first word, so
/// `"crmaine"` finds `"crmaine (^_-)"`), else a `#rrggbb` / named / `"r,g,b"`
/// spec. `None` for an unknown spec or the "default" preset.
pub(crate) fn resolve_bg(spec: &str) -> Option<Color> {
    let key = spec.trim().to_lowercase();
    for (name, color) in PANE_BG_PRESETS {
        let first = name.split_whitespace().next().unwrap_or(name).to_lowercase();
        if first == key {
            return color;
        }
    }
    theme::parse_color(spec)
}

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
    /// A filename to save the diff/compare result into (the text is carried
    /// here because the source popup is replaced by the prompt).
    DiffSaveAs { text: String },
    /// A name for an archive about to be created from `sources`, in the given
    /// format. The extension is appended if missing.
    CompressName { kind: CompressKind, sources: Vec<PathBuf> },
    /// The password for an encrypted zip about to be extracted. Rendered masked.
    ExtractPassword { archive: PathBuf, members: Vec<String>, dest: PathBuf },
    /// The log message for an `svn commit` of the given paths.
    SvnCommit { paths: Vec<PathBuf> },
    /// A typed local directory to download the given remote files into.
    LocalDestPath { files: Vec<String> },
    /// The chmod mode (octal, e.g. 777; blank = keep) for an upload to `remote`.
    UploadChmod { remote: String },
    /// The chmod mode for files just downloaded into `dir` (local, Unix only).
    DownloadChmod { files: Vec<String>, dir: PathBuf },
    /// A bulk-rename pattern (template or `s/re/rep/flags`) for these files.
    BulkRenamePattern { targets: Vec<PathBuf> },
}

/// The archive format chosen from the right-click "Compress" submenu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompressKind {
    Zip,
    /// A password-protected (AES-256) zip.
    ZipEnc,
    TarGz,
}

impl InputKind {
    /// Whether the field holds a secret and should be shown as dots.
    fn is_secret(&self) -> bool {
        matches!(self, InputKind::ZipPassword { .. } | InputKind::ExtractPassword { .. })
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

    /// Convert from the UI-agnostic node the Lua store round-trips.
    fn from_node(n: &cian_lua::shortcuts::Node) -> Self {
        Self {
            name: n.name.clone(),
            target: n.target.clone(),
            children: n.children.as_ref().map(|ch| ch.iter().map(Shortcut::from_node).collect()),
        }
    }

    fn to_node(&self) -> cian_lua::shortcuts::Node {
        cian_lua::shortcuts::Node {
            name: self.name.clone(),
            target: self.target.clone(),
            children: self.children.as_ref().map(|ch| ch.iter().map(Shortcut::to_node).collect()),
        }
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

/// Build the request-side [`cian_ai::AiConfig`] from the parsed Lua config.
/// Shared by startup and `:reload` so AI settings can be tuned live.
pub(crate) fn ai_config_from(config: &cian_lua::Config) -> Option<cian_ai::AiConfig> {
    config.ai.as_ref().map(|a| cian_ai::AiConfig {
        python: a.python.clone(),
        endpoint: a.endpoint.clone(),
        model: a.model.clone(),
        api_version: a.api_version.clone(),
        auth_mode: a.auth_mode.clone(),
        api_key: a.api_key.clone(),
        api_base_url: a.api_base_url.clone(),
    })
}

impl ShortcutStore {
    /// The Lua file bookmarks are stored in now. Portable-aware: a copy next to
    /// the executable wins for both reading and writing (see [`cian_lua`]).
    pub fn default_path() -> PathBuf {
        cian_lua::config_write_path("shortcuts.lua")
            .unwrap_or_else(|| PathBuf::from("shortcuts.lua"))
    }

    /// A legacy `shortcuts.<ext>` to migrate, resolved the same portable-aware
    /// way as the Lua file so a carried-along old file is still found.
    fn legacy_path(ext: &str) -> Option<PathBuf> {
        cian_lua::config_read_path(&format!("shortcuts.{ext}"))
            .filter(|p| p.exists())
    }

    pub fn load_or_default() -> Self {
        // Prefer the Lua file (portable copy first, then the user dir).
        if let Some(lua) = cian_lua::config_read_path("shortcuts.lua").filter(|p| p.exists()) {
            if let Ok(nodes) = cian_lua::shortcuts::load(&lua) {
                return Self { entries: nodes.iter().map(Shortcut::from_node).collect(), path: Self::default_path() };
            }
        }
        // Otherwise migrate a legacy YAML, then a legacy TOML, writing the Lua
        // copy and leaving the old file in place (a harmless safety net).
        let path = Self::default_path();
        for ext in ["yaml", "toml"] {
            let Some(legacy) = Self::legacy_path(ext) else { continue };
            let Ok(text) = std::fs::read_to_string(&legacy) else { continue };
            let parsed = if ext == "yaml" {
                serde_yml::from_str::<ShortcutsFile>(&text).ok()
            } else {
                toml::from_str::<ShortcutsFile>(&text).ok()
            };
            if let Some(file) = parsed {
                let store = Self { entries: file.shortcuts, path };
                let _ = store.save();
                return store;
            }
        }
        Self { entries: Vec::new(), path }
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let nodes: Vec<cian_lua::shortcuts::Node> = self.entries.iter().map(Shortcut::to_node).collect();
        std::fs::write(&self.path, cian_lua::shortcuts::to_lua(&nodes))?;
        Ok(())
    }
}

/// Which version-control system a pane's directory belongs to. Both report the
/// same [`cian_core::git::RepoStatus`] display type, so the UI is identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vcs {
    Git,
    Svn,
}

/// A pane's cached VCS status and the directory it was computed for.
struct GitState {
    cwd: PathBuf,
    /// Which VCS the status came from (`None` when the directory is in neither).
    kind: Option<Vcs>,
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
    /// The server connection being browsed for a download, reused for the
    /// directory listings and the transfer itself.
    scp_target: Option<(cian_scp::Target, String)>,
    /// A pending remote directory listing: the worker sends `(cwd, entries)` or
    /// an error message. Polled from the main loop.
    remote_ls: Option<RemoteLsRx>,
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
    /// Language for the key manual and the right-click menu specifically —
    /// `menu_lang` overrides `lang` for those two surfaces; else follows `lang`.
    menu_lang: Lang,
    /// Cached git status per file pane `[left, right]`, recomputed when the
    /// pane's directory changes or on an explicit refresh.
    git: [Option<GitState>; 2],
    /// Cached free/total disk space of each file pane's mount `[left, right]`,
    /// refreshed alongside `git` when the pane's directory changes or after a
    /// file operation. `Some(cwd, None)` remembers a mount that could not be
    /// queried, so we don't re-probe it every frame.
    disk: [Option<(PathBuf, Option<cian_core::disk::Usage>)>; 2],
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
    /// User-defined macros loaded from `macro.lua` (portable-aware).
    macros: Vec<cian_lua::macros::Macro>,
    /// Why `macro.lua` failed to load, if it did — shown when the menu is empty.
    macro_error: Option<String>,
    /// A layout macro currently building itself out across ticks.
    macro_run: Option<macro_run::MacroRun>,
    /// File/step-counter settings from `count.lua` (portable-aware).
    count_opts: cian_core::count::Options,
    /// A running count, delivering its report when finished.
    count_job: Option<std::sync::mpsc::Receiver<cian_core::count::Report>>,
    /// A file the user asked to edit; the main loop suspends the TUI, runs the
    /// external editor, and restores. See [`crate::edit`].
    pending_edit: Option<edit::PendingEdit>,
    /// Reversible operations, newest last; `u` undoes the last one.
    undo_stack: Vec<UndoAction>,
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
        let (macros, macro_error) = macro_run::load_macros();
        let count_opts = count::load_count_opts();
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
            scp_target: None,
            remote_ls: None,
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
            menu_lang: match config.options.menu_lang.as_deref() {
                Some(s) => Lang::from_opt(Some(s)),
                None => Lang::from_opt(config.options.lang.as_deref()),
            },
            git: [None, None],
            disk: [None, None],
            ai: ai_config_from(&config),
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
            macros,
            macro_error,
            macro_run: None,
            count_opts,
            count_job: None,
            pending_edit: None,
            undo_stack: Vec::new(),
            pending_g: false,
            zoomed: false,
            debug_keys: std::env::var("CIAN_DEBUG_KEYS").is_ok(),
            config,
            keymap,
        })
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

/// Parse a chmod field: blank → no change `(None, None)`; a valid octal mode →
/// `(Some(mode), None)`; anything else → `(None, Some(error))`.
fn parse_chmod(s: &str) -> (Option<u32>, Option<String>) {
    let t = s.trim();
    if t.is_empty() {
        return (None, None);
    }
    match u32::from_str_radix(t, 8) {
        Ok(m) if m <= 0o7777 => (Some(m), None),
        _ => (None, Some(format!("invalid chmod {:?} — use an octal mode like 777", t))),
    }
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
                entry("F3", None, "look inside: text/hex, an image, or an archive's list", "中身を見る：テキスト/16進・画像・書庫の一覧"),
                entry("  edit in viewer", None, "i = built-in editor (Ctrl+S save, Esc/Q leave), E = external", "ビューア内編集：i 内蔵（Ctrl+S 保存, Esc/Q 終了）／ E 外部エディタ"),
                entry(":edit", None, "edit the file in your external editor (E in the viewer)", "外部エディタで編集（ビューア内は E）"),
                entry("  in viewer", None, "hjkl move, /n/N search, %/{/}/NG jump, v/V/C-v select y copy", "ビューア内：hjkl移動, /n/N検索, %/{/}/NG移動, v/V/C-v選択 yコピー"),
                entry("  B in viewer", None, "toggle the git blame gutter (who last changed each line)", "ビューア内：git blame ガター切替（各行の最終変更者）"),
                entry("  from a grep hit", None, "Ctrl+n/N next/prev hit, Shift+Enter reveal in pane, e encoding", "grepヒットから：Ctrl+n/N 次/前, Shift+Enter 場所へ, e 文字コード"),
                entry("=", None, "compare left ↔ right: two files (line diff), or two folders (recursive)", "左右を比較：ファイル同士（行差分）／フォルダ同士（再帰）"),
                entry("  > / <", None, "  in a comparison: copy the entry across to the other side (confirms overwrite)", "  比較画面：エントリを反対側へコピー（上書きは確認）"),
                entry("  c / w", None, "  in a comparison: copy result to clipboard / save it to the active pane", "  比較画面：結果をクリップボードへ／アクティブペインに保存"),
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
                entry("u", None, "undo the last rename / create / move (also :undo)", "直前のリネーム／作成／移動を取り消し（:undo でも）"),
                entry("y, c", Some(Copy), "copy to opposite pane", "反対ペインへコピー"),
                entry("m", Some(Move), "move to opposite pane", "反対ペインへ移動"),
                entry("d", Some(Delete), "delete (to trash)", "削除（ゴミ箱へ）"),
                entry("r", Some(Rename), "rename", "リネーム"),
                entry(":brename", None, "bulk rename by pattern: {name}_{n3}.{ext} or s/re/rep/gi (preview first)", "パターン一括リネーム：{name}_{n3}.{ext} / s/re/rep/gi（先にプレビュー）"),
                entry("a", Some(NewFile), "new file", "新規ファイル"),
                entry("A", Some(NewDir), "new directory", "新規ディレクトリ"),
                entry("o", Some(SyncFromOther), "this pane → other pane's directory", "このペインを反対ペインと同じ場所に"),
                entry("O", Some(SyncToOther), "other pane → this pane's directory", "反対ペインをこのペインと同じ場所に"),
                entry("Ctrl+Enter", Some(OpenOther), "open in the opposite pane", "反対ペインで開く"),
                entry("p", Some(CopyPath), "copy path text to clipboard", "パス文字列をクリップボードにコピー"),
                entry("Shift+P", Some(CopyFileRef), "copy file(s) to clipboard", "ファイルをクリップボードにコピー"),
                entry("s", Some(Shortcuts), "shortcuts menu", "ショートカットメニュー"),
                entry("@", None, "run a macro (layout builder; also :macros / right-click)", "マクロを実行（レイアウト構築；:macros／右クリックでも）"),
                entry(":count", None, "count files & steps (marked, or the whole tree)", "ファイル・ステップ数を数える（マーク or ツリー全体）"),
                entry(":hidden", None, "show / hide dotfiles (also right-click)", "ドットファイルの表示切替（右クリックでも）"),
                entry(":attr", None, "attributes;  :chmod 644,  :readonly on|off", "属性；  :chmod 644,  :readonly on|off"),
                entry(":hash", None, "checksum;  :hash md5  /  :hash sha256", "チェックサム；  :hash md5  /  :hash sha256"),
                entry(":stage / :unstage", None, "git add / git reset the selection (in a repo)", "選択を git add / git reset（リポジトリ内）"),
                entry(":discard", None, "git/svn: throw away worktree changes (git checkout / svn revert)", "作業ツリーの変更を破棄（git checkout / svn revert）"),
                entry(":gitlog", None, "commit log / a file's history — git or svn (also right-click)", "コミットログ／ファイル履歴 — git・svn（右クリックでも）"),
                entry(":gitdiff", None, "the selected file's diff vs HEAD/BASE — git or svn", "選択ファイルの HEAD／BASE との差分 — git・svn"),
                entry(":svnupdate", None, "svn update the working copy (also right-click SVN ▸)", "svn update で作業コピーを更新（右クリック SVN ▸ でも）"),
                entry(":svncommit", None, "svn commit the selection (prompts for a message)", "選択を svn commit（メッセージ入力）"),
                entry(":svnresolve", None, "svn resolve --accept working (mark conflicts resolved)", "svn resolve --accept working（競合を解決）"),
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
                entry(":where", None, "which config files cian reads/writes (portable vs ~/.config)", "cianが読み書きする設定ファイルの場所（ポータブル/~/.config）"),
                entry(":mark", None, "mark by wildcard;  :mark *.rs   :unmark *", "ワイルドカードでマーク；  :mark *.rs   :unmark *"),
                entry(":ai", None, "AI chat  (needs cian.ai in init.lua)", "AIチャット  (init.luaのcian.aiが必要)"),
                entry(":aicmd", None, "AI: shell command from a description", "AI: 説明からシェルコマンド生成"),
                entry(":zip", None, "bundle selection;  :zip -e  for a password", "選択物をまとめる；  :zip -e でパスワード付き"),
                entry(":tar / :targz", None, "make a .tar / .tar.gz (also right-click ▸ Compress)", ".tar / .tar.gz を作成（右クリック▸圧縮でも）"),
                entry(":unzip", None, "extract the archive here (also right-click ▸ Extract)", "書庫をここに解凍（右クリック▸解凍でも）"),
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
                entry(":sync", None, "synchronize: type into all panes at once (also right-click)", "同時入力：全ペインへ一括入力（右クリックでも）"),
                entry("Ctrl+Shift+Enter / :snip", None, "snippet launcher → send a saved command to the shell; works from the shell too (cian.snippets)", "スニペットランチャー → 定型コマンドをシェルへ送信；シェルからも可（cian.snippets）"),
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
        "    cian --macro <FILE.lua>        run a macro file once at startup".to_string(),
        "    cian <FILE.lua>                same (a *.lua argument is a macro)".to_string(),
        "    cian --macro-name <NAME>       run a named macro from your config".to_string(),
        String::new(),
        "ARGS:".to_string(),
        "    LEFT_PATH     directory for the left pane  (default: current dir)".to_string(),
        "    RIGHT_PATH    directory for the right pane (default: current dir)".to_string(),
        String::new(),
        "OPTIONS:".to_string(),
        "    -h, --help    show this help".to_string(),
        "    -V, --version show the version and commit".to_string(),
        "    -man, --man   show the full key manual (also ? or Ctrl+. in-app)".to_string(),
        "    -m, --macro <FILE.lua>   build a macro's layout at startup".to_string(),
        "    --macro-name <NAME>      build a named macro from macro.lua / macro/".to_string(),
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

/// What to run once, automatically, at startup — driven by the command line.
/// This is cian's TeraTerm-`.ttl`-style hook: point it at a macro and cian comes
/// up with that layout already built.
pub enum StartupMacro {
    /// Nothing — the normal interactive start.
    None,
    /// `--macro <file>` (or a `*.lua` argument): load this file and run its
    /// first macro once.
    File(PathBuf),
    /// `--macro-name <name>`: run a macro of this name from the loaded config.
    Named(String),
}

pub fn run(left: Option<PathBuf>, right: Option<PathBuf>, startup: StartupMacro) -> Result<()> {
    // Load user config (never fails; problems are reported below).
    let config = cian_lua::load();

    // With no paths on the command line, pick up where the last session left
    // off; an explicit path always wins over the remembered one.
    let session = if left.is_none() && right.is_none() {
        session::restore()
    } else {
        None
    };
    let fallback = default_home(&config);
    let left = left
        .or_else(|| session.as_ref().and_then(|s| s.left_dir()))
        .unwrap_or_else(|| fallback.clone());
    let right = right
        .or_else(|| session.as_ref().and_then(|s| s.right_dir()))
        .unwrap_or(fallback);

    // Resolve and install the color theme before any drawing happens.
    let theme_errors = theme::install(&config.theme, config.options.borders.as_deref());

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
    // Restore which pane had focus, if a session set it.
    if session.as_ref().map(|s| s.focused_right()).unwrap_or(false) {
        app.focus(FocusedPane::Right);
    }
    if !startup_errors.is_empty() {
        let mut lines = vec!["config loaded with issues:".to_string(), String::new()];
        let total = startup_errors.len();
        lines.extend(startup_errors.into_iter().take(10));
        if total > 10 {
            lines.push(format!("... and {} more", total - 10));
        }
        app.popup = Popup::Notice { lines };
    }

    // A startup macro (from `--macro` / `--macro-name` / a `*.lua` argument):
    // queue it so it builds as soon as the shell is up, like a TeraTerm `.ttl`.
    match startup {
        StartupMacro::None => {}
        StartupMacro::Named(name) => {
            if !app.start_macro_by_name(&name) {
                app.message = Some(format!("no macro named {:?} (check macro.lua / macro/)", name));
            }
        }
        StartupMacro::File(path) => match cian_lua::macros::load(&path) {
            Ok(ms) if !ms.is_empty() => app.begin_macro(&ms[0]),
            Ok(_) => app.message = Some(format!("{}: no macro found in file", path.display())),
            Err(e) => app.message = Some(format!("macro {}: {}", path.display(), e)),
        },
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

    // Remember where the panes were for next launch.
    app.save_session();

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

/// Suspend the TUI, run the external editor attached to the real terminal on
/// the queued file, then restore the alternate screen and reload. cian owns the
/// terminal here, so this is where the leave/enter dance belongs.
fn suspend_and_edit<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    let Some(edit) = app.pending_edit.take() else { return Ok(()) };
    let Some(cmd) = edit::resolve_editor(&app.config) else {
        app.message = Some(tr(
            app.lang,
            "no editor found — install nvim/vim/vi, or set cian.set_option(\"editor\", …)",
            "エディタが見つかりません — nvim/vim/vi を入れるか cian.set_option(\"editor\", …) を設定してください",
        ).into());
        return Ok(());
    };

    // Hand the terminal back to a normal cooked state for the editor.
    let mut out = io::stdout();
    disable_raw_mode()?;
    let _ = execute!(out, PopKeyboardEnhancementFlags);
    execute!(out, DisableBracketedPaste, DisableMouseCapture, LeaveAlternateScreen)?;

    let status = Command::new(&cmd[0]).args(&cmd[1..]).arg(&edit.path).status();

    // Take it back and rebuild the screen.
    enable_raw_mode()?;
    execute!(out, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    let _ = execute!(
        out,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    terminal.clear()?;

    match status {
        Ok(s) if s.success() || s.code().is_some() => {}
        Ok(_) => app.message = Some("editor exited abnormally".into()),
        Err(e) => app.message = Some(format!("could not launch editor: {}", e)),
    }

    // The file may have changed on disk; refresh the panes and, if the edit came
    // from the viewer, re-open it on the (possibly changed) file.
    app.reload_both();
    if edit.reopen_viewer {
        app.open_viewer_at(&edit.path, &edit.title, 0);
    }
    Ok(())
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
        // Advance a running layout macro (splits, colours, commands) once the
        // shell is idle between spawns.
        if app.macro_run.is_some() && app.tick_macro() {
            needs_redraw = true;
        }
        // Install a finished remote directory listing into the download browser.
        if app.remote_ls.is_some() && app.poll_remote_ls() {
            needs_redraw = true;
        }
        // A finished file/step count shows its report.
        if app.count_job.is_some() && app.poll_count() {
            needs_redraw = true;
        }
        // An edit request suspends the TUI, runs the editor, and restores.
        if app.pending_edit.is_some() {
            suspend_and_edit(terminal, app)?;
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
mod tests;
