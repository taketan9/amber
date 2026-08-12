//! Mouse handling: routing clicks/drags/scroll to the viewer, AI chat, review
//! popups, the context menu, panes and borders, plus the popup hit-zone and
//! row-cursor helpers. Split out of lib.rs as an `impl App` block.
use super::*;

impl App {
    // ------- Mouse -------
    pub(crate) fn handle_mouse(&mut self, ev: MouseEvent) {
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

        // A border is a border whatever is drawn beside it: a click on one is
        // a resize, not a click on a pane or on the panel.
        let on_divider = self.dividers.iter().any(|d| {
            let r = d.zone;
            col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
        });
        // A docked panel only owns the mouse inside its own frame. A click on
        // the listing beside it, or on the shell below, moves the focus there
        // — the panel is one surface among the window's, not a dialog over
        // them.
        if !on_divider && matches!(self.popup, Popup::Viewer { .. }) && self.viewer_dock.is_some() {
            let hit = |r: Rect| {
                r.width > 0
                    && r.height > 0
                    && col >= r.x
                    && col < r.x + r.width
                    && row >= r.y
                    && row < r.y + r.height
            };
            let inside = hit(self.viewer_frame)
                || (self.viewer_split.is_some()
                    && (hit(self.viewer_half_rects[0]) || hit(self.viewer_half_rects[1])));
            if matches!(ev.kind, MouseEventKind::Down(_)) {
                let to = if inside {
                    // Clicking the panel focuses it, the same way clicking a
                    // listing focuses that pane. Without this the click was
                    // swallowed by the panel's own handling, which only runs
                    // for the focused pane — so the panel could be clicked
                    // *away from* but never *to*.
                    self.viewer_dock
                } else if hit(self.layout_rects.left) {
                    Some(FocusedPane::Left)
                } else if hit(self.layout_rects.right) {
                    Some(FocusedPane::Right)
                } else if hit(self.layout_rects.shell) {
                    Some(FocusedPane::Shell)
                } else {
                    None
                };
                if let Some(to) = to {
                    if to != self.focused {
                        self.focus(to);
                        // A click on the panel goes on to do what it came for
                        // — place the caret, hit a tab, hit the ✕ — now that
                        // the panel is the focused surface.
                        if !inside {
                            return;
                        }
                    }
                }
            }
        }
        // In the viewer: a click places the cursor on that line, a drag selects
        // whole lines (line-wise visual), the wheel scrolls, and right-click
        // copies. Handled before the blanket popup guard below.
        // …and it only handles the mouse *inside its own frame* when it is
        // docked: outside it, the click belongs to the window — a border to
        // drag, a pane to focus.
        let inside_panel = {
            let hit = |r: Rect| {
                r.width > 0
                    && r.height > 0
                    && col >= r.x
                    && col < r.x + r.width
                    && row >= r.y
                    && row < r.y + r.height
            };
            // Split in two, the panel is both halves — `viewer_frame` only
            // describes the one the keyboard is on.
            hit(self.viewer_frame)
                || (self.viewer_split.is_some()
                    && (hit(self.viewer_half_rects[0]) || hit(self.viewer_half_rects[1])))
        };
        // The seam between two panes runs along the panel's own border, so a
        // click there is a resize even though it is "inside" the frame.
        if matches!(self.popup, Popup::Viewer { .. })
            && (self.viewer_dock.is_none()
                || (inside_panel && !on_divider && self.viewer_dock == Some(self.focused)))
        {
            // The tab strip lives in the top border. A title starts one column
            // inside the frame and opens with " ◂ ▸ ", which puts the arrows
            // at the third and fifth columns of the box.
            let frame = self.viewer_frame;
            // The ✕ in the corner. Since Esc no longer closes the file, this
            // is the way out that does not have to be known about.
            let x_rect = self.viewer_close_rect;
            if matches!(ev.kind, MouseEventKind::Down(MouseButton::Left))
                && x_rect.width > 0
                && row == x_rect.y
                && col >= x_rect.x
                && col < x_rect.x + x_rect.width
            {
                if matches!(self.popup, Popup::Viewer { dirty: true, .. }) {
                    self.message = Some(
                        tr(
                            self.lang,
                            "unsaved changes — :w to save, :q! to discard",
                            "未保存の変更があります — :w で保存、:q! で破棄",
                        )
                        .into(),
                    );
                } else {
                    self.close_viewer_file();
                }
                return;
            }
            if self.viewer_tab_count() > 1
                && matches!(ev.kind, MouseEventKind::Down(MouseButton::Left))
                && row == frame.y
            {
                if col == frame.x + 2 {
                    self.viewer_switch_tab(false);
                    return;
                }
                if col == frame.x + 4 {
                    self.viewer_switch_tab(true);
                    return;
                }
                // …or the name of the file itself, which is what a tab strip
                // is for.
                if let Some((_, i)) = self
                    .viewer_tab_rects
                    .iter()
                    .copied()
                    .find(|(r, _)| col >= r.x && col < r.x + r.width)
                {
                    self.viewer_goto_tab(i);
                    return;
                }
            }
            // A click in the half that is not in focus crosses to it, rather
            // than moving a cursor in a file the keyboard is not pointed at.
            if self.viewer_split.is_some()
                && matches!(ev.kind, MouseEventKind::Down(MouseButton::Left))
            {
                let theirs = self.viewer_half_rects[1];
                if theirs.width > 0
                    && col >= theirs.x
                    && col < theirs.x + theirs.width
                    && row >= theirs.y
                    && row < theirs.y + theirs.height
                {
                    self.swap_viewer_split();
                    self.full_clear = true;
                    return;
                }
            }
            // A click in the outline column jumps to that entry — the reason
            // the column is worth its width.
            let ol = self.outline_rect;
            if ol.width > 0
                && matches!(ev.kind, MouseEventKind::Down(MouseButton::Left))
                && col >= ol.x
                && col < ol.x + ol.width
                && row >= ol.y
                && row < ol.y + ol.height
            {
                if let Popup::Viewer { shape, line, col: c, goal, visual, md_map, view, .. } = &mut self.popup {
                    let items = shape.as_deref().map(|s| s.items.as_slice()).unwrap_or(&[]);
                    let here = crate::render::src_line(md_map, *line);
                    let top = crate::render::outline_top(items, here, ol.height as usize);
                    let idx = top + (row - ol.y) as usize;
                    if let Some(item) = items.get(idx).cloned() {
                        *line = crate::render::disp_line(md_map, &view.lines, item.line);
                        *c = 0;
                        *goal = 0;
                        *visual = None;
                    }
                }
                return;
            }
            let body = self.viewer_rect;
            let body_h = (body.height as usize).max(1);
            // The clicked column, offset past the line-number gutter, so a click
            // lands on the character under the pointer (not just its line).
            let text_x = body.x + self.viewer_gutter;
            let ecol = col;
            // Closed folds mean screen rows and line numbers are not the same
            // thing: the rows drawn from `scroll` down, resolved once, so a
            // click over folded text lands on what is under the pointer.
            let rows: Vec<usize> = if let Popup::Viewer { view, scroll, shape, preview, .. } = &self.popup {
                let hid = shape
                    .as_deref()
                    .filter(|_| !*preview)
                    .map(|sh| sh.hidden(view.lines.len()))
                    .unwrap_or_default();
                (*scroll..view.lines.len())
                    .filter(|i| hid.is_empty() || !hid[*i])
                    .take(body_h)
                    .collect()
            } else {
                Vec::new()
            };
            let line_at = |row: u16, scroll: usize, n: usize| -> usize {
                let rel = row.saturating_sub(body.y) as usize;
                match rows.get(rel) {
                    Some(l) => *l,
                    None => rows.last().copied().unwrap_or((scroll + rel).min(n.saturating_sub(1))),
                }
            };
            // A click on the fold marker in the gutter opens or closes it,
            // rather than moving the cursor there — the marker is drawn to be
            // clicked, and a cursor move is what the text itself is for.
            // The gutter is [line number][fold marker][git change bar], so the
            // marker is two columns left of where the text starts.
            if matches!(ev.kind, MouseEventKind::Down(MouseButton::Left))
                && self.viewer_gutter > 1
                && col + 2 == text_x
                && row >= body.y
                && ((row - body.y) as usize) < rows.len()
            {
                let l = line_at(row, 0, 0);
                self.toggle_viewer_fold(Some(l));
                return;
            }
            // A clicked column is not a character index: a tab is one buffer
            // character but several drawn columns, and a Japanese character is
            // one buffer character but two. Both have to be walked back
            // through the same widths the renderer used — counting every
            // character as one column put the cursor a character further left
            // for every wide one before it, which is most of a line of
            // Japanese.
            let col_at = |view: &cian_core::viewer::View, l: usize| -> usize {
                let rel = ecol.saturating_sub(text_x) as usize;
                let Some(text) = view.lines.get(l) else { return 0 };
                let mut drawn = 0usize;
                for (j, ch) in text.chars().enumerate() {
                    let w = cian_core::textops::char_cols(ch, drawn);
                    if rel < drawn + w {
                        return j;
                    }
                    drawn += w;
                }
                vlen(view, l)
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
                // Right-click opens the menu — the same gesture as in the
                // file panes. Copying moved into it, where it can be seen.
                MouseEventKind::Down(MouseButton::Right) => self.open_viewer_menu(col, row),
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
            if let Popup::RenameReview { items, cursor, scroll, .. } = &mut self.popup {
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
        // A border drag belongs to the window, not to whatever is drawn in
        // it. With the panel docked in a pane, dragging the seam between the
        // panes — or the one above the shell — resizes them as ever.
        let panel_docked = matches!(self.popup, Popup::Viewer { .. }) && self.viewer_dock.is_some();
        let border_gesture = panel_docked
            && (self.drag.is_some()
                || (on_divider && matches!(ev.kind, MouseEventKind::Down(MouseButton::Left))));
        if !matches!(self.popup, Popup::None) && !border_gesture {
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

        // Copied out, so the closure below borrows nothing of `self` — the
        // wheel needs to reach for the pane mutably right after asking which
        // pane it is over.
        let rects = self.layout_rects;
        let pane_at = move |col: u16, row: u16| -> Option<FocusedPane> {
            let hit = |r: Rect| {
                r.width > 0 && r.height > 0
                    && col >= r.x && col < r.x + r.width
                    && row >= r.y && row < r.y + r.height
            };
            if hit(rects.left) {
                Some(FocusedPane::Left)
            } else if hit(rects.right) {
                Some(FocusedPane::Right)
            } else if hit(rects.shell) {
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
                    // Dragging within the origin pane just moves the cursor.
                    // It used to rubber-band-select rows, which fought the
                    // deliberate marking `Space` and visual mode already do,
                    // and made every slightly-shaky click reshuffle the marks.
                    let _ = anchor;
                    if over == Some(from) && from != FocusedPane::Shell {
                        self.cursor_to_row(from, row);
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

        // The ◀ / ▶ arrows at the head of the title, before anything else on
        // that row can claim the click.
        if let Some((pane, fwd, _)) = self.nav_rects.iter().copied().find(|(_, _, r)| in_rect(*r)) {
            self.focus(pane);
            if fwd {
                self.pane_go_forward();
            } else {
                self.pane_go_back();
            }
            return;
        }
        // A breadcrumb click navigates to that ancestor of the pane's cwd.
        // Checked before tab selection: these rects sit inside the active
        // tab's label, and the tab click would otherwise swallow them.
        if let Some((pane, strip, _)) =
            self.crumb_rects.iter().copied().find(|(_, _, r)| in_rect(*r))
        {
            self.focus(pane);
            let target = self.active_pane().map(|p| {
                let mut t = p.cwd.clone();
                for _ in 0..strip {
                    if let Some(parent) = t.parent() {
                        t = parent.to_path_buf();
                    }
                }
                t
            });
            if let (Some(t), Some(p)) = (target, self.active_pane_mut()) {
                if t != p.cwd {
                    let _ = p.jump_to(t);
                }
            }
            return;
        }
        // A column-header click sorts by that column (repeat = flip).
        if let Some((pane, key, _)) =
            self.sort_rects.iter().copied().find(|(_, _, r)| in_rect(*r))
        {
            self.focus(pane);
            self.apply_sort_key(key);
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
                    });
                }
            }
            None => {}
        }
    }

    /// The zone under the pointer, if any. Later zones win, so a small button
    /// drawn on top of a wider row is reachable.
    pub(crate) fn zone_at(&self, col: u16, row: u16) -> Option<ZoneKind> {
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
    pub(crate) fn set_popup_cursor(&mut self, i: usize) {
        match &mut self.popup {
            Popup::GrepReplace(plan) => plan.cursor = i,
            Popup::ContextMenu { cursor, .. }
            | Popup::ColorPicker { cursor, .. }
            | Popup::SortPicker { cursor, .. }
            | Popup::Macros { cursor, .. }
            | Popup::GitLog { cursor, .. }
            | Popup::EncodingPicker { cursor, .. }
            | Popup::DirCompare { cursor, .. }
            | Popup::Archive { cursor, .. }
            | Popup::DiskUsage { cursor, .. }
            | Popup::Palette { cursor, .. }
            | Popup::DestPicker { cursor, .. }
            | Popup::FindResults { cursor, .. }
            | Popup::SshHosts { cursor, .. }
            | Popup::SshUsers { cursor, .. }
            | Popup::Snippets { cursor, .. }
            | Popup::RemoteBrowser { cursor, .. }
            | Popup::LocalDest { cursor, .. }
            | Popup::History { cursor, .. }
            | Popup::Shortcuts { cursor, .. } => *cursor = i,
            _ => {}
        }
    }

    /// Drive the on-screen popup with the mouse: the wheel scrolls, a click on a
    /// registered zone replays the keystroke it stands for so all the existing
    /// popup key handling does the real work.
    pub(crate) fn handle_popup_mouse(&mut self, ev: MouseEvent) -> Result<()> {
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

    /// Act on the selected entry as Enter would: enter a directory, or read a
    /// file in the viewer. Inside an archive the rows are members, and Enter
    /// navigates or views them; on an archive file, Enter goes in.
    ///
    /// Enter reads rather than launching. Looking at a file is what one does
    /// with it a hundred times a day and handing it to another program is what
    /// one does occasionally — and the viewer can be left with Esc, while an
    /// application that opens by accident has to be found and closed.
    /// Ctrl+Enter is the launch, and `x` where a terminal keeps Ctrl.
    pub(crate) fn activate_selected(&mut self) -> Result<()> {
        if self.active_pane().map(|p| p.archive_view().is_some()).unwrap_or(false) {
            self.archive_activate();
            return Ok(());
        }
        let sel = self.active_pane().and_then(|p| p.selected()).map(|e| (e.is_dir, e.path.clone()));
        match sel {
            Some((true, _)) => {
                if let Some(p) = self.active_pane_mut() {
                    p.enter_selected()?;
                }
            }
            Some((false, path)) if cian_core::archive::is_archive(&path) => {
                self.enter_archive(path, String::new());
            }
            // Enter reads it *here*: the same viewer, docked in the pane
            // whose listing it replaces, with everything it can do. F3 and
            // Shift+Tab open the same file over the whole window instead.
            Some((false, _)) => {
                let here = self.focused;
                self.look_inside();
                if matches!(self.popup, Popup::Viewer { .. }) {
                    self.viewer_dock = Some(here);
                    self.full_clear = true;
                }
            }
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
    pub(crate) fn finish_file_drag(&mut self, over: Option<FocusedPane>, mods: KeyModifiers) {
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

    pub(crate) fn anim_enabled(&self) -> bool {
        !self.anim_dur.is_zero()
    }

    /// Toggle full-window zoom of the focused surface, animating between the
    /// surface's pane rect and the whole layout area.
    pub(crate) fn toggle_zoom(&mut self) {
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
    pub(crate) fn toggle_pane_zoom_animated(&mut self) {
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
    pub(crate) fn active_shell_leaf_rect(&self) -> Option<Rect> {
        let tab = self.shell.active;
        let leaf = self.shell.tabs.get(tab).map(|t| t.active)?;
        self.shell_leaves.iter().find(|(t, l, _, _)| *t == tab && *l == leaf).map(|(_, _, r, _)| *r)
    }

    pub(crate) fn start_anim(&mut self, kind: AnimKind) {
        if !self.anim_enabled() {
            return;
        }
        self.anim = Some(Anim { kind, start: Instant::now(), dur: self.anim_dur });
    }

    /// What the renderer should override this frame.
    pub(crate) fn anim_override(&self) -> AnimOverride {
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
    pub(crate) fn finish_anim(&mut self) {
        if self.anim.take().is_none() {
            return;
        }
        if let Some(close) = self.anim_then.take() {
            self.apply_pending_close(close);
        }
    }

    /// Perform a close that was deferred until its shrink animation finished.
    pub(crate) fn apply_pending_close(&mut self, close: PendingClose) {
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
    pub(crate) fn close_shell_pane_animated(&mut self) {
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
    pub(crate) fn flash(&mut self, pane: FocusedPane) {
        self.flash = Some((pane, Instant::now()));
    }

    /// How lit `pane` currently is, 1.0 right after a flash fading to 0.0.
    /// Returns 0.0 once the flash has expired.
    pub(crate) fn flash_level(&self, pane: FocusedPane) -> f32 {
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
    pub(crate) fn flash_active(&self) -> bool {
        self.flash.map(|(_, at)| at.elapsed().as_secs_f32() < FLASH_SECS).unwrap_or(false)
    }

    /// Which `pane_bg` slot a file pane uses.
    pub(crate) fn bg_slot(pane: FocusedPane) -> Option<usize> {
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
    pub(crate) fn copy_shell_selection(&mut self) {
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
        // `contents_between` stops *before* its end column, while the
        // highlight covers the cell the pointer is on — so the last character
        // of a selection was shown as taken and then not copied. One past it
        // is what the eye was promised.
        let text = match session.parser().lock() {
            Ok(p) => p.screen().contents_between(
                start.0,
                start.1,
                endp.0,
                endp.1.saturating_add(1),
            ),
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

    pub(crate) fn select_shell_leaf_at(&mut self, col: u16, row: u16) {
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
    pub(crate) fn cursor_to_row(&mut self, pane: FocusedPane, row: u16) {
        let rect = match pane {
            FocusedPane::Left => self.layout_rects.left,
            FocusedPane::Right => self.layout_rects.right,
            FocusedPane::Shell => return,
        };
        // The list starts two rows in: the top border, then the column header.
        let Some(offset) = row.checked_sub(rect.y + 2) else { return };
        if offset >= rect.height.saturating_sub(3) {
            return;
        }
        let Some(p) = self.active_pane_mut() else { return };
        // The list scrolls, so the first visible row is not always entry 0.
        let view_h = rect.height.saturating_sub(3) as usize;
        let first = p.cursor.saturating_sub(view_h.saturating_sub(1)).min(
            p.entries.len().saturating_sub(view_h.min(p.entries.len())),
        );
        let idx = first + offset as usize;
        if idx < p.entries.len() {
            p.cursor = idx;
        }
    }
}
