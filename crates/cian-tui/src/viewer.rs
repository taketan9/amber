//! The F3 viewer's own key handling and actions: vim-style motion and visual
//! selection, incremental search stepping, encoding re-decode, reveal-in-pane,
//! grep-hit stepping, and copying the selection. Split out of lib.rs as an
//! `impl App` block.
use super::*;

impl App {
    /// Copy the viewer's selected lines (or the whole file when nothing is
    /// selected) to the clipboard.
    /// The vim-flavoured keymap for the F3 viewer: a cursor that moves with
    /// h/j/k/l and friends, and v / V / Ctrl-v visual selection with y/c to
    /// copy. The rendered body height (from `viewer_rect`) sizes the page moves
    /// and keeps the cursor on screen.
    pub(crate) fn handle_viewer_key(&mut self, key: KeyEvent) -> Result<()> {
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

        // Markdown preview mode: a simpler, scroll-only view of the rendered
        // document (the styled line count differs from the source, so scrolling
        // is bounded by the count the renderer stashed last frame).
        if matches!(self.popup, Popup::Viewer { preview: true, .. }) {
            let body_h = (self.viewer_rect.height as usize).max(1);
            let last = self.viewer_preview_lines.saturating_sub(1);
            if let Popup::Viewer { scroll, preview, title, .. } = &mut self.popup {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => { self.popup = Popup::None; return Ok(()); }
                    KeyCode::Char('p') => {
                        *preview = false;
                        self.message = Some(tr(self.lang, "markdown: source", "Markdown: ソース").into());
                        return Ok(());
                    }
                    KeyCode::Char('j') | KeyCode::Down => *scroll = (*scroll + 1).min(last),
                    KeyCode::Char('k') | KeyCode::Up => *scroll = scroll.saturating_sub(1),
                    KeyCode::Char('d') | KeyCode::PageDown => *scroll = (*scroll + body_h / 2).min(last),
                    KeyCode::Char('u') | KeyCode::PageUp => *scroll = scroll.saturating_sub(body_h / 2),
                    KeyCode::Char('g') | KeyCode::Home => *scroll = 0,
                    KeyCode::Char('G') | KeyCode::End => *scroll = last,
                    KeyCode::Char('S') => { let _ = title; self.summarize_viewer(); return Ok(()); }
                    KeyCode::Char('y') | KeyCode::Char('c') => { self.copy_viewer_selection(); return Ok(()); }
                    KeyCode::Enter if shift => { self.viewer_reveal_in_pane(); return Ok(()); }
                    _ => {}
                }
                return Ok(());
            }
        }

        // `p` toggles into the rendered Markdown preview (source-mode only, since
        // preview mode handles `p` above).
        if !ctrl && key.code == KeyCode::Char('p') {
            if let Popup::Viewer { preview, markdown: true, .. } = &mut self.popup {
                *preview = true;
                self.message = Some(tr(self.lang, "markdown: preview", "Markdown: プレビュー").into());
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
    pub(crate) fn viewer_search_jump(&mut self, forward: bool) {
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
    pub(crate) fn finish_encoding_pick(&mut self, chosen: Option<cian_core::viewer::TextEncoding>) {
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
            EncTarget::Diff(mut diff) => {
                if let (Some(enc), Popup::Diff { left_path, right_path, result, folded, encoding, scroll, .. }) =
                    (chosen, diff.as_mut())
                {
                    match cian_core::diff::diff_files_with_encoding(left_path, right_path, enc) {
                        Ok(d) => {
                            *folded = cian_core::diff::fold(&d.rows, cian_core::diff::CONTEXT);
                            *result = d;
                            *encoding = enc;
                            *scroll = 0;
                            self.message = Some(format!("encoding: {}", enc.label()));
                        }
                        Err(e) => self.message = Some(format!("re-diff failed: {}", e)),
                    }
                }
                self.popup = *diff;
            }
        }
    }

    /// Shift+Enter in the viewer: close it and move the active pane to the
    /// viewed file's directory, cursor on the file.
    pub(crate) fn viewer_reveal_in_pane(&mut self) {
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
    pub(crate) fn viewer_grep_step(&mut self, forward: bool) {
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

    pub(crate) fn copy_viewer_selection(&mut self) {
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
}
