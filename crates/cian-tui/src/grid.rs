//! Steering the icon grid.
//!
//! The grid does not read like a list, so it is not driven like one. In every
//! desktop file manager a letter key means *go to the file that starts with
//! it*, and the arrows walk the grid — and that is what people arriving at an
//! icon view already know. So in this view, and only in this view, cian gives
//! the letters up.
//!
//! Repeating a letter walks the files that begin with it, one press per file,
//! wrapping at the end. Typing different letters builds a prefix instead, so
//! `re` finds `README.md` without stopping at `report.pdf` on the way. A pause
//! ends the word: the next letter starts a fresh search rather than extending
//! one the user has forgotten about.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::render::clamp_list_scroll;
use crate::{App, FocusedPane, Mode, PendingOp, Popup};

/// How long a typed prefix stays live. Long enough to finish a word, short
/// enough that a letter pressed a moment later is obviously a new search.
const PATIENCE: Duration = Duration::from_millis(900);

impl App {
    /// Handle a key the grid claims for itself. Returns whether it did.
    ///
    /// Only plain letters and the arrows are taken. Everything else — `:`, `/`,
    /// Enter, Backspace, Space, the digits that pick a tab, every combination
    /// with a modifier — falls through to the keys cian has always had, so the
    /// grid is a different way of *moving*, not a different program.
    pub(crate) fn grid_key(&mut self, key: KeyEvent) -> bool {
        if !self.icon_view
            || !matches!(self.popup, Popup::None)
            || self.mode != Mode::Normal
            || self.focused == FocusedPane::Shell
        {
            return false;
        }
        // A modifier means a command, not typing.
        if key.modifiers.intersects(
            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
        ) {
            return false;
        }

        let cols = self.icon_cols.max(1);
        match key.code {
            KeyCode::Up => self.grid_move(-(cols as isize)),
            KeyCode::Down => self.grid_move(cols as isize),
            KeyCode::Left => self.grid_move(-1),
            KeyCode::Right => self.grid_move(1),
            // Digits are left alone: they pick a tab, and no one looks for a
            // file by typing its leading digit often enough to be worth that.
            KeyCode::Char(c) if c.is_alphanumeric() && !c.is_ascii_digit() => {
                self.type_ahead(c)
            }
            _ => return false,
        }
        true
    }

    /// Step the cursor by `by` entries, stopping at the ends rather than
    /// wrapping — a grid has corners, and walking off one should feel like it.
    fn grid_move(&mut self, by: isize) {
        let Some(pane) = self.active_pane_mut() else { return };
        let last = pane.entries.len().saturating_sub(1);
        let want = pane.cursor as isize + by;
        pane.cursor = want.clamp(0, last as isize) as usize;
        // A letter typed after moving starts a new search, not a continuation.
        self.type_ahead.clear();
    }

    /// Go to the file this letter names.
    /// The same jump, for a view that is not the grid — see the `q` arm in
    /// `keys.rs`.
    pub(crate) fn type_ahead_jump(&mut self, c: char) {
        self.type_ahead(c);
    }

    fn type_ahead(&mut self, c: char) {
        let now = Instant::now();
        if now.duration_since(self.type_ahead_at) > PATIENCE {
            self.type_ahead.clear();
        }
        self.type_ahead_at = now;

        // The same letter again walks the files beginning with it rather than
        // looking for a name with the letter twice — `jj` is how one asks for
        // the second `j`, not for `jjson`.
        let repeat = self.type_ahead.chars().count() == 1
            && self.type_ahead.chars().next().map(lower) == Some(lower(c));
        if !repeat {
            self.type_ahead.push(c);
        }
        let prefix: String = self.type_ahead.to_lowercase();

        let Some(pane) = self.active_pane() else { return };
        let names: Vec<String> = pane.entries.iter().map(|e| e.name.to_lowercase()).collect();
        let from = if repeat { pane.cursor + 1 } else { 0 };
        let total = names.len();

        // From `from`, all the way round, so a repeat wraps back to the first.
        let found = (0..total)
            .map(|i| (from + i) % total)
            .find(|&i| names[i].starts_with(&prefix));

        match found {
            Some(i) => {
                if let Some(p) = self.active_pane_mut() {
                    p.cursor = i;
                }
            }
            // Nothing starts with what has been typed. Rather than sit on a
            // dead prefix — where every further letter also fails — drop back
            // to just this letter and try again.
            None if prefix.chars().count() > 1 => {
                self.type_ahead.clear();
                self.type_ahead.push(c);
                let one = self.type_ahead.to_lowercase();
                if let Some(i) = names.iter().position(|n| n.starts_with(&one)) {
                    if let Some(p) = self.active_pane_mut() {
                        p.cursor = i;
                    }
                }
            }
            None => {}
        }
    }
}

/// Lowercase a char for comparison. Only the first of a multi-char lowering is
/// kept, which is enough: this compares single keystrokes.
fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// The mouse, in the grid.
///
/// The list panes have their own hit-testing in [`crate::mouse`]; none of it
/// applies here, because the grid puts entries in two dimensions and cian's
/// panes have only ever had one. So the grid answers for its own rectangle.
impl App {
    /// Which entry is under this cell, if any.
    ///
    /// Only tiles that actually hold an entry answer — the empty space after
    /// the last file is not the last file, and clicking it should do nothing
    /// rather than jump the cursor to the end.
    pub(crate) fn grid_entry_at(&self, col: u16, row: u16) -> Option<usize> {
        let area = self.grid_area?;
        if col < area.x || row < area.y || col >= area.x + area.width || row >= area.y + area.height
        {
            return None;
        }
        let cols = self.icon_cols.max(1);
        let cx = ((col - area.x) / crate::render::TILE_W) as usize;
        let cy = ((row - area.y) / crate::render::TILE_H) as usize;
        if cx >= cols {
            return None;
        }
        let pane = self.active_pane()?;
        let per_page = cols * (area.height / crate::render::TILE_H).max(1) as usize;
        let start = pane.cursor.checked_div(per_page).map_or(0, |page| page * per_page);
        let i = start + cy * cols + cx;
        (i < pane.entries.len()).then_some(i)
    }

    /// Which place in the sidebar is on this row, if any.
    pub(crate) fn sidebar_at(&self, row: u16) -> Option<std::path::PathBuf> {
        self.sidebar_rows.iter().find(|(_, y)| *y == row).map(|(p, _)| p.clone())
    }

    /// Which toolbar button is under this cell, if any.
    pub(crate) fn grid_button_at(&self, col: u16, row: u16) -> Option<crate::GridButton> {
        self.grid_buttons
            .iter()
            .find(|(_, r)| {
                col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
            })
            .map(|(b, _)| *b)
    }
}

/// What the grid does when it is clicked.
impl App {
    /// A single click: put the cursor on what was clicked, or press a button.
    /// Returns whether the click belonged to the grid.
    ///
    /// Held down, Ctrl (or Command — both, because on a Mac with the modifiers
    /// swapped there is no telling which key the user thinks of as Ctrl) adds
    /// to the selection instead of replacing it, which is what every desktop
    /// does. In cian's terms that is a mark, the same one `Space` sets, so a
    /// selection built with the mouse can be operated on with the keyboard.
    pub(crate) fn grid_click_mods(&mut self, col: u16, row: u16, adding: bool) -> bool {
        // Both desktop-shaped views, not just the grid: the detail view has the
        // same address bar, the same buttons and the same sidebar drawn down
        // its left — and none of them answered a click, because this whole
        // function began by asking whether the grid was showing. The places in
        // the sidebar are the reason a sidebar is there.
        if !self.single_pane_view() {
            return false;
        }
        // The address bar, before the buttons: it spans the width, so a click
        // anywhere along it is a click on it. A crumb goes to that ancestor;
        // anywhere else opens the prompt to type a path.
        if self.grid_address.is_some_and(|r| {
            row == r.y && col >= r.x && col < r.x + r.width
        }) {
            let crumb = self
                .grid_crumbs
                .iter()
                .find(|(_, r)| col >= r.x && col < r.x + r.width)
                .map(|(p, _)| p.clone());
            match crumb {
                Some(path) => {
                    if let Some(p) = self.active_pane_mut() {
                        p.marks.clear();
                        let _ = p.jump_to(path);
                    }
                    self.type_ahead.clear();
                }
                None => self.start_jump_path(),
            }
            return true;
        }
        if let Some(b) = self.grid_button_at(col, row) {
            self.grid_button(b);
            return true;
        }
        // The sidebar is one click to a place, which is the whole reason it
        // is there.
        if col < crate::render::SIDEBAR_W + 1 {
            // "＋ 追加" keeps where you are. The bookmark list `b` opens can do
            // this too, and nobody arriving at a sidebar knows that.
            if self.sidebar_add.is_some_and(|r| row == r.y) {
                self.start_shortcut_add(Vec::new(), false);
                return true;
            }
            if let Some(path) = self.sidebar_at(row) {
                if let Some(p) = self.active_pane_mut() {
                    p.marks.clear();
                    let _ = p.jump_to(path);
                }
                self.type_ahead.clear();
                return true;
            }
        }
        // Below here is the grid's own: tiles, and the empty space between
        // them. The detail view's rows are the listing's to answer.
        if !self.icon_view {
            return false;
        }
        if let Some(i) = self.grid_entry_at(col, row) {
            if let Some(p) = self.active_pane_mut() {
                // Ctrl+click *adds* to a selection, so there has to be one to
                // add to. The file already under the cursor is what the eye
                // says is selected — it is drawn selected — so the first
                // Ctrl+click makes that true rather than starting from nothing
                // and quietly dropping the file the user thought they had.
                if adding && p.marks.is_empty() {
                    let was = p.cursor;
                    if was != i {
                        p.set_mark_at(was);
                    }
                }
                p.cursor = i;
                if adding {
                    p.toggle_mark_at(i);
                }
            }
            // Clicking is pointing, and the next letter typed starts a new
            // search rather than continuing one from before the click.
            self.type_ahead.clear();
            return true;
        }
        // Inside the grid but on nothing. Swallowed, so a stray click on the
        // background does not fall through to the list panes underneath — and
        // it empties the selection, which is what clicking the empty part of a
        // window means everywhere else.
        let inside = self.grid_area.is_some_and(|a| {
            col >= a.x && row >= a.y && col < a.x + a.width && row < a.y + a.height
        });
        if inside {
            if let Some(p) = self.active_pane_mut() {
                p.marks.clear();
            }
            self.type_ahead.clear();
        }
        inside
    }

    /// A double click: a directory is entered, a file is handed to whichever
    /// application owns it.
    ///
    /// Not what Enter does. Enter *reads* — cian's rule is that looking at a
    /// file is the common case and launching another program is the rare one.
    /// But a double click in a grid of icons means "open" to everyone who has
    /// ever used a desktop, and this view is for those people.
    pub(crate) fn grid_double_click(&mut self, col: u16, row: u16) -> bool {
        if !self.icon_view {
            return false;
        }
        let Some(i) = self.grid_entry_at(col, row) else { return false };
        if let Some(p) = self.active_pane_mut() {
            p.cursor = i;
        }
        // Over SFTP the row's path belongs to the server, so neither entering it
        // as a local directory nor handing it to the desktop can work. The
        // remote pane's own Enter does both jobs — descend, or fetch and read.
        if self.active_pane().map(|p| p.is_remote()).unwrap_or(false) {
            self.remote_pane_enter();
            return true;
        }
        let is_dir = self.active_pane().and_then(|p| p.selected()).map(|e| e.is_dir);
        match is_dir {
            Some(true) => {
                if let Some(p) = self.active_pane_mut() {
                    p.marks.clear();
                    let _ = p.enter_selected();
                }
            }
            Some(false) => self.open_externally(),
            None => {}
        }
        true
    }

    /// A toolbar button.
    fn grid_button(&mut self, which: crate::GridButton) {
        use crate::GridButton::*;
        match which {
            Back => {
                self.clear_marks();
                self.pane_go_back();
            }
            Forward => {
                self.clear_marks();
                self.pane_go_forward();
            }
            Up => {
                if let Some(p) = self.active_pane_mut() {
                    p.marks.clear();
                    let _ = p.go_parent();
                }
            }
            // Leaving the grid is the front end's business — it owns the view —
            // so this only says so, and the window notices.
            Close => self.icon_view_close = true,
        }
    }
}

impl App {
    /// A plain click, with nothing held down. Used by the tests, which are
    /// about where a click lands rather than about what is held while it does.
    #[cfg(test)]
    pub(crate) fn grid_click(&mut self, col: u16, row: u16) -> bool {
        self.grid_click_mods(col, row, false)
    }
}

impl App {
    /// Drop the selection. Leaving a directory ends what was chosen in it:
    /// marks name paths, and carrying a set of them somewhere else is how a
    /// delete lands on something nobody was looking at.
    fn clear_marks(&mut self) {
        if let Some(p) = self.active_pane_mut() {
            p.marks.clear();
        }
    }
}

/// Dragging files with the mouse.
///
/// cian has never had this: a terminal cannot report a drag as anything but a
/// stream of motion events, and it cannot be a drag source for the desktop at
/// all. A window can do both, so the pieces live here — what was picked up, and
/// where letting go would put it — with the drawing left to whoever owns the
/// surface and the *doing* left to cian's existing confirmation.
impl App {
    /// What a press at this cell would pick up, if anything.
    ///
    /// The marked files when the one under the pointer is among them, so a
    /// selection built up with `Space` or Ctrl-click drags as a group; just the
    /// one file otherwise. `..` is never picked up — it is a way out, not a
    /// thing.
    pub(crate) fn drag_targets_at(&self, col: u16, row: u16) -> Vec<std::path::PathBuf> {
        let i = match self.icon_view {
            true => self.grid_entry_at(col, row),
            false => self.row_entry_at(col, row),
        };
        let Some(i) = i else { return Vec::new() };
        let Some(pane) = self.active_pane() else { return Vec::new() };
        let Some(entry) = pane.entries.get(i) else { return Vec::new() };
        if entry.is_parent {
            return Vec::new();
        }
        if pane.is_marked(i) {
            let marked = pane.target_paths();
            if !marked.is_empty() {
                return marked;
            }
        }
        vec![entry.path.clone()]
    }

    /// Which entry a cell falls on in a list pane, if any.
    fn row_entry_at(&self, col: u16, row: u16) -> Option<usize> {
        let area = self.layout_rects.for_pane(self.focused);
        if area.width == 0
            || col < area.x
            || row < area.y
            || col >= area.x + area.width
            || row >= area.y + area.height
        {
            return None;
        }
        // One row for the border, one for the column headings.
        let first = area.y + 2;
        if row < first {
            return None;
        }
        let pane = self.active_pane()?;
        let list_h = area.height.saturating_sub(3) as usize;
        let start = clamp_list_scroll(pane.scroll, pane.cursor, list_h, pane.entries.len());
        let i = start + (row - first) as usize;
        (i < pane.entries.len()).then_some(i)
    }

    /// Where letting go at this cell would put the files, if anywhere.
    ///
    /// A folder under the pointer, a place in the sidebar, or the other pane.
    /// Anywhere else is not a destination, and the drag is simply abandoned —
    /// a drop that lands on nothing should do nothing.
    pub(crate) fn drop_target_at(&self, col: u16, row: u16) -> Option<std::path::PathBuf> {
        if let Some(path) = self.sidebar_at(row) {
            if col < crate::render::SIDEBAR_W + 1 {
                return Some(path);
            }
        }
        let over = match self.icon_view {
            true => self.grid_entry_at(col, row),
            false => self.row_entry_at(col, row),
        };
        if let Some(i) = over {
            if let Some(e) = self.active_pane().and_then(|p| p.entries.get(i)) {
                if e.is_dir {
                    return Some(e.path.clone());
                }
            }
        }
        // The other pane, in the two-pane view.
        if !self.icon_view {
            for (which, rect) in [
                (FocusedPane::Left, self.layout_rects.left),
                (FocusedPane::Right, self.layout_rects.right),
            ] {
                if which != self.focused
                    && rect.width > 0
                    && col >= rect.x
                    && row >= rect.y
                    && col < rect.x + rect.width
                    && row < rect.y + rect.height
                {
                    let tabs = if which == FocusedPane::Left { &self.left } else { &self.right };
                    return Some(tabs.active_ref().cwd.clone());
                }
            }
        }
        None
    }

    /// Ask to put `targets` in `dest`. Opens the confirmation cian already
    /// uses for every copy and move — a drag is a new way to *say* it, not a
    /// new way to move files without being asked.
    pub(crate) fn drop_onto(
        &mut self,
        targets: Vec<std::path::PathBuf>,
        dest: std::path::PathBuf,
        move_it: bool,
    ) {
        if targets.is_empty() {
            return;
        }
        // Dropping a folder into itself, or into where it already is, is a
        // gesture that missed rather than a request.
        if targets.iter().any(|t| *t == dest || t.parent() == Some(dest.as_path())) {
            return;
        }
        let op = if move_it { PendingOp::Move } else { PendingOp::Copy };
        self.popup = Popup::ConfirmTransfer { op, targets, dest };
    }
}
