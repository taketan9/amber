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
                        // Validate a `/re/` pattern now, at the prompt, where
                        // the error can point at what was typed.
                        if let Err(e) = cian_core::search::Matcher::parse(&q) {
                            self.message = Some(e);
                            return Ok(());
                        }
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

        // Ctrl+S saves from normal mode too. It used to live only inside the
        // insert-mode handler, so a change made with `dd` or `:s` could not be
        // written without first entering insert mode for no reason.
        if ctrl && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S'))
            && matches!(self.popup, Popup::Viewer { editable: true, .. })
        {
            self.save_viewer_file();
            return Ok(());
        }
        // A confirm-each-one replace owns the keyboard while it walks.
        if matches!(self.popup, Popup::Viewer { sub_walk: Some(_), .. }) {
            return self.sub_walk_key(key);
        }
        // While typing `:s/old/new/`, keys build the command; Enter runs it.
        if matches!(self.popup, Popup::Viewer { sub_input: Some(_), .. }) {
            match key.code {
                KeyCode::Esc => {
                    if let Popup::Viewer { sub_input, .. } = &mut self.popup {
                        *sub_input = None;
                    }
                }
                KeyCode::Enter => {
                    let cmd = if let Popup::Viewer { sub_input, .. } = &mut self.popup {
                        sub_input.take().unwrap_or_default()
                    } else {
                        String::new()
                    };
                    self.run_substitute(&cmd);
                }
                KeyCode::Backspace => {
                    if let Popup::Viewer { sub_input: Some(s), .. } = &mut self.popup {
                        s.pop();
                    }
                }
                KeyCode::Char(c) if !ctrl => {
                    if let Popup::Viewer { sub_input: Some(s), .. } = &mut self.popup {
                        s.push(c);
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        // `:` opens replace, seeded with `s/` so the shape is on screen.
        if !ctrl && !alt && key.code == KeyCode::Char(':')
            && matches!(self.popup, Popup::Viewer { editable: true, preview: false, .. })
        {
            if let Popup::Viewer { sub_input, .. } = &mut self.popup {
                *sub_input = Some("s/".to_string());
            }
            return Ok(());
        }
        // While editing, the built-in plain-text editor owns every key.
        if matches!(self.popup, Popup::Viewer { editing: true, .. }) {
            return self.handle_editor_key(key);
        }
        // `i` enters the editor on an editable text file. A Markdown preview
        // drops to its source first, since edits belong on the raw file.
        if !ctrl && !alt && key.code == KeyCode::Char('i')
            && matches!(self.popup, Popup::Viewer { editable: true, .. })
        {
            let mut binary = false;
            if let Popup::Viewer { preview, editing, line, col, scroll, visual, view, undo, .. } =
                &mut self.popup
            {
                if *preview {
                    *preview = false;
                    (*line, *col, *scroll, *visual) = (0, 0, 0, None);
                }
                binary = view.kind == cian_core::viewer::ViewKind::Binary;
                if binary {
                    // The hex editor stores the nibble index in `col`.
                    *col = 0;
                } else {
                    // One insert session = one undo unit (vim's coarse model).
                    push_viewer_undo(undo, &view.lines, *line, *col);
                }
                *editing = true;
            }
            if binary {
                self.message = Some(tr(
                    self.lang,
                    "hex edit — 0-9a-f overwrites, Ctrl+S saves (.bak kept), Esc leaves",
                    "hex編集 — 0-9a-f で上書き、Ctrl+S 保存（.bak を残す）、Esc で終了",
                ).into());
            } else {
                self.entered_editing_message();
            }
            return Ok(());
        }
        // The vim change set (x, dd, o, u, …) works from normal mode on an
        // editable file; a consumed key stops here.
        if self.viewer_edit_operator(key) {
            return Ok(());
        }

        // `p` toggles between the raw source and the rendered Markdown preview.
        // The preview is a full viewer — cursor, visual selection, `/` search and
        // the mouse all work over the rendered document — so this only flips the
        // flag and resets the cursor; the render swaps `view.lines` in and out.
        if !ctrl && key.code == KeyCode::Char('p') {
            if let Popup::Viewer { preview, markdown: true, line, col, scroll, visual, .. } = &mut self.popup {
                *preview = !*preview;
                let on = *preview;
                (*line, *col, *scroll, *visual) = (0, 0, 0, None);
                self.message = Some(if on {
                    tr(self.lang, "markdown: preview", "Markdown: プレビュー").into()
                } else {
                    tr(self.lang, "markdown: source", "Markdown: ソース").into()
                });
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
        // `e` opens the encoding picker; the choice re-decodes this file. Only
        // in source mode — the rendered preview owns `view.lines`.
        if !ctrl && key.code == KeyCode::Char('e')
            && !matches!(self.popup, Popup::Viewer { preview: true, .. })
        {
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
        // `E` opens the file in the external editor (nvim → vim → vi, or the
        // configured one); the viewer re-opens on it afterwards.
        if !ctrl && key.code == KeyCode::Char('E') {
            self.edit_viewed_file();
            self.popup = Popup::None;
            return Ok(());
        }
        // `B` toggles the git blame gutter.
        if !ctrl && key.code == KeyCode::Char('B') {
            self.toggle_viewer_blame();
            return Ok(());
        }
        // `m` opens any mermaid blocks as a real diagram in the browser (the
        // terminal shows the readable flow; this is the crisp picture).
        if !ctrl && key.code == KeyCode::Char('m') {
            self.open_mermaid_in_browser();
            return Ok(());
        }
        // `/`, `f` and `Shift+F` all open the search prompt (the pane's own
        // find keys, so the reflex carries over into the viewer and preview).
        if !ctrl && matches!(key.code, KeyCode::Char('/') | KeyCode::Char('f') | KeyCode::Char('F')) {
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
        let mut coding = false;
        let mut warn_unsaved = false;
        if let Popup::Viewer { view, scroll, line, col, goal, visual, anchor, count, find_query, dirty, .. } = &mut self.popup {
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
                (false, KeyCode::Esc) => {
                    // Esc peels state off one layer at a time: leave visual
                    // selection, then clear an active search (its highlights),
                    // then refuse to drop unsaved edits, and only then close.
                    // `q` behaves the same; `Q` discards and closes.
                    if visual.is_some() {
                        *visual = None;
                    } else if find_query.is_some() {
                        *find_query = None;
                    } else if *dirty {
                        warn_unsaved = true;
                    } else {
                        close = true;
                    }
                }
                (false, KeyCode::Char('q')) => {
                    if *dirty {
                        warn_unsaved = true;
                    } else {
                        close = true;
                    }
                }
                // Discard unsaved edits and close.
                (false, KeyCode::Char('Q')) => close = true,
                (false, KeyCode::Char('v')) => start_visual(ViewVisual::Char, visual, anchor, *line, *col),
                (false, KeyCode::Char('V')) => start_visual(ViewVisual::Line, visual, anchor, *line, *col),
                (true, KeyCode::Char('v')) => start_visual(ViewVisual::Block, visual, anchor, *line, *col),
                // `S` summarises the file with the AI (sends its text). Handled
                // after the borrow ends, below.
                (false, KeyCode::Char('S')) => summarize = true,
                // `A` hands this code to crmaine Coding (Ajent). Also below.
                (false, KeyCode::Char('A')) => coding = true,
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
        if warn_unsaved {
            self.message = Some(tr(
                self.lang,
                "unsaved edits — Ctrl+S to save, or Shift+Q to discard & close",
                "未保存の編集 — Ctrl+S で保存、Shift+Q で破棄して閉じる",
            ).into());
            return Ok(());
        }
        if summarize {
            self.summarize_viewer();
            return Ok(());
        }
        if coding {
            self.start_coding("");
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
                    if let Popup::Viewer { view, visual, source, hl, .. } = viewer.as_mut() {
                        view.redecode(enc);
                        // Keep the preview's source in step with the new decode,
                        // and drop the highlight cache so it recomputes.
                        *source = view.lines.clone();
                        hl.clear();
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
        self.reveal_path_in_pane(&path);
    }

    /// Run a `:s/old/new/flags` typed at the viewer's replace prompt.
    ///
    /// Without `c` every replacement lands at once, as one undo step. With
    /// `c` the hits are walked one at a time — see [`Self::sub_walk_key`].
    /// A visual selection limits the range, which is how "just this block"
    /// is expressed without a separate syntax.
    pub(crate) fn run_substitute(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() || cmd == "s/" {
            return;
        }
        // The same prompt carries the line-ending conversions: they are the
        // other thing people open replace for, and `s/\r//` cannot express
        // them (the viewer's lines never hold the ending).
        let eol = match cmd {
            "lf" => Some(cian_core::viewer::Eol::Lf),
            "crlf" => Some(cian_core::viewer::Eol::Crlf),
            "cr" => Some(cian_core::viewer::Eol::Cr),
            _ => None,
        };
        if let Some(want) = eol {
            let changed = if let Popup::Viewer { view, dirty, .. } = &mut self.popup {
                let was = view.eol;
                view.eol = want;
                *dirty = *dirty || was != want;
                was != want
            } else {
                false
            };
            self.message = Some(if changed {
                format!("line endings → {} (Ctrl+S to write)", want.label())
            } else {
                format!("already {}", want.label())
            });
            return;
        }
        let sub = match cian_core::substitute::parse(cmd.trim()) {
            Ok(s) => s,
            Err(e) => {
                self.message = Some(format!("✖ {e}"));
                return;
            }
        };
        // A visual selection is the range; otherwise the whole file.
        let range = if let Popup::Viewer { visual: Some(_), anchor, line, .. } = &self.popup {
            let (a, b) = (anchor.0.min(*line), anchor.0.max(*line));
            Some((a, b))
        } else {
            None
        };
        let Popup::Viewer { view, .. } = &self.popup else { return };
        let hits = cian_core::substitute::find(&sub, &view.lines, range);
        if hits.is_empty() {
            self.message = Some(tr(self.lang, "no matches", "該当なし").into());
            return;
        }
        if sub.confirm {
            // Land on the first hit and hand the keyboard to the walk.
            let first = hits[0].line;
            if let Popup::Viewer { sub_walk, line, col, visual, .. } = &mut self.popup {
                *visual = None;
                *line = first;
                *col = hits[0].start;
                *sub_walk = Some(Box::new(crate::SubWalk { hits, idx: 0, replaced: 0, skipped: 0 }));
            }
            self.scroll_viewer_to_cursor();
            return;
        }
        let n = hits.len();
        self.apply_substitution(&hits);
        self.message = Some(if self.lang == Lang::Ja {
            format!("{} 件置換しました", n)
        } else {
            format!("replaced {} occurrence(s)", n)
        });
    }

    /// Apply `hits` to the buffer as one undo step.
    fn apply_substitution(&mut self, hits: &[cian_core::substitute::Hit]) {
        if let Popup::Viewer { view, undo, dirty, hl, line, visual, .. } = &mut self.popup {
            push_viewer_undo(undo, &view.lines, *line, 0);
            view.lines = cian_core::substitute::apply(&view.lines, hits);
            *line = (*line).min(view.lines.len().saturating_sub(1));
            *visual = None;
            *dirty = true;
            hl.clear();
        }
    }

    /// The confirm-each-one walk: `y` replaces, `n` skips, `a` takes all that
    /// remain, `q`/Esc stops. Each acceptance is applied straight away so the
    /// change is visible while deciding the next one; the later hits on that
    /// same line shift by the length difference.
    fn sub_walk_key(&mut self, key: KeyEvent) -> Result<()> {
        let answer = match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => 'y',
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char(' ') => 'n',
            KeyCode::Char('a') | KeyCode::Char('A') => 'a',
            KeyCode::Char('q') | KeyCode::Esc => 'q',
            _ => return Ok(()),
        };
        let Popup::Viewer { sub_walk: Some(w), .. } = &self.popup else { return Ok(()) };
        let (hits, idx, replaced, skipped) =
            (w.hits.clone(), w.idx, w.replaced, w.skipped);

        if answer == 'q' {
            self.finish_sub_walk(replaced, skipped, true);
            return Ok(());
        }
        if answer == 'a' {
            // Everything from here on, in one go.
            let rest: Vec<_> = hits[idx..].to_vec();
            let n = rest.len();
            self.apply_substitution(&rest);
            self.finish_sub_walk(replaced + n, skipped, false);
            return Ok(());
        }

        let mut hits = hits;
        let mut replaced = replaced;
        let mut skipped = skipped;
        if answer == 'y' {
            let hit = hits[idx].clone();
            self.apply_substitution(std::slice::from_ref(&hit));
            replaced += 1;
            // The line just changed length; every later hit on it moves.
            let delta = hit.to.chars().count() as isize - hit.from.chars().count() as isize;
            for h in hits.iter_mut().skip(idx + 1).filter(|h| h.line == hit.line) {
                h.start = (h.start as isize + delta).max(0) as usize;
                h.end = (h.end as isize + delta).max(0) as usize;
            }
        } else {
            skipped += 1;
        }
        let next = idx + 1;
        if next >= hits.len() {
            self.finish_sub_walk(replaced, skipped, false);
            return Ok(());
        }
        let (nl, nc) = (hits[next].line, hits[next].start);
        if let Popup::Viewer { sub_walk, line, col, .. } = &mut self.popup {
            *line = nl;
            *col = nc;
            *sub_walk = Some(Box::new(crate::SubWalk { hits, idx: next, replaced, skipped }));
        }
        self.scroll_viewer_to_cursor();
        Ok(())
    }

    /// End a confirm walk and report what it did.
    fn finish_sub_walk(&mut self, replaced: usize, skipped: usize, stopped: bool) {
        if let Popup::Viewer { sub_walk, .. } = &mut self.popup {
            *sub_walk = None;
        }
        let ja = self.lang == Lang::Ja;
        let head = if stopped {
            if ja { "中断 — " } else { "stopped — " }
        } else {
            ""
        };
        self.message = Some(if ja {
            format!("{head}{replaced} 件置換, {skipped} 件スキップ")
        } else {
            format!("{head}replaced {replaced}, skipped {skipped}")
        });
    }

    /// Keep the cursor line on screen after a jump.
    fn scroll_viewer_to_cursor(&mut self) {
        let body_h = (self.viewer_rect.height as usize).max(1);
        if let Popup::Viewer { line, scroll, view, .. } = &mut self.popup {
            let n = view.lines.len();
            if *line < *scroll {
                *scroll = *line;
            } else if *line >= *scroll + body_h {
                *scroll = *line + 1 - body_h;
            }
            *scroll = (*scroll).min(n.saturating_sub(body_h));
        }
    }

    /// The "editing — …" status line, shared by every way into the editor.
    fn entered_editing_message(&mut self) {
        self.message = Some(tr(
            self.lang,
            "editing — type to insert, Ctrl+S save, Esc leave",
            "編集中 — 入力で挿入, Ctrl+S 保存, Esc 終了",
        ).into());
    }

    /// vim's small change set from the viewer's normal mode: `x`, `dd`, `D`,
    /// `J`, `o`/`O`, `a`, `I` and `u` (undo), on an editable non-preview file.
    /// Returns true when the key was consumed.
    ///
    /// Two deliberate deviations, because existing bindings win: `A` stays
    /// crmaine Coding (append-at-end is `$` then `a`), and on non-editable
    /// views `d`/`u` keep their pager scrolling — here they are operator and
    /// undo, with Ctrl+d/Ctrl+u still scrolling as in vim.
    fn viewer_edit_operator(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::ALT)
        {
            return false;
        }
        let body_h = (self.viewer_rect.height as usize).max(1);
        let mut entered_editing = false;
        let mut consumed = false;
        if let Popup::Viewer {
            view,
            line,
            col,
            goal,
            scroll,
            visual,
            anchor,
            count,
            pending,
            dirty,
            editing,
            undo,
            hl,
            editable: true,
            preview: false,
            ..
        } = &mut self.popup
        {
            // The change set edits text. A binary view's "lines" are its hex
            // dump — text operators there would edit the rendering, not the
            // file. Hex editing has its own mode behind `i`.
            if view.kind != cian_core::viewer::ViewKind::Text {
                return false;
            }
            let lines = &mut view.lines;
            if lines.is_empty() {
                lines.push(String::new());
            }
            // An interrupted `dd` cancels the operator; the key then acts
            // normally (vim's behaviour for an abandoned operator).
            let was_pending = pending.take();
            let cnt = |count: &mut Option<usize>| count.take().unwrap_or(1).max(1);

            match key.code {
                // dd — delete N whole lines. First d only arms the operator.
                KeyCode::Char('d') if visual.is_none() => {
                    if was_pending != Some('d') {
                        *pending = Some('d');
                        return true;
                    }
                    push_viewer_undo(undo, lines, *line, *col);
                    let n = cnt(count).min(lines.len() - *line);
                    lines.drain(*line..*line + n);
                    if lines.is_empty() {
                        lines.push(String::new());
                    }
                    *line = (*line).min(lines.len() - 1);
                    *col = 0;
                    *goal = 0;
                    *dirty = true;
                    consumed = true;
                }
                // d / x in visual mode — delete the selection.
                KeyCode::Char('d') | KeyCode::Char('x') if visual.is_some() => {
                    let mode = visual.take().unwrap();
                    push_viewer_undo(undo, lines, *line, *col);
                    let (s, e) = crate::util::order_pos(*anchor, (*line, *col));
                    match mode {
                        ViewVisual::Line => {
                            lines.drain(s.0..=e.0.min(lines.len() - 1));
                            if lines.is_empty() {
                                lines.push(String::new());
                            }
                            *line = s.0.min(lines.len() - 1);
                            *col = 0;
                        }
                        ViewVisual::Char => {
                            let head: String =
                                lines[s.0].chars().take(s.1).collect();
                            let tail: String =
                                lines[e.0.min(lines.len() - 1)].chars().skip(e.1 + 1).collect();
                            lines.drain(s.0..=e.0.min(lines.len() - 1));
                            lines.insert(s.0, head + &tail);
                            *line = s.0;
                            *col = s.1.min(lines[s.0].chars().count());
                        }
                        // A block delete would need per-line column splicing;
                        // not worth the code until someone misses it.
                        ViewVisual::Block => {
                            *visual = Some(mode);
                            self.message = Some(tr(
                                self.lang,
                                "block delete is not supported",
                                "矩形削除は未対応",
                            ).into());
                            return true;
                        }
                    }
                    *goal = *col;
                    *dirty = true;
                    consumed = true;
                }
                // x — delete N characters under the cursor.
                KeyCode::Char('x') => {
                    let len = lines[*line].chars().count();
                    if *col < len {
                        push_viewer_undo(undo, lines, *line, *col);
                        let n = cnt(count).min(len - *col);
                        let chs: Vec<char> = lines[*line].chars().collect();
                        lines[*line] =
                            chs[..*col].iter().chain(chs[*col + n..].iter()).collect();
                        let new_len = len - n;
                        *col = (*col).min(new_len.saturating_sub(1));
                        *goal = *col;
                        *dirty = true;
                    }
                    consumed = true;
                }
                // D — delete to the end of the line.
                KeyCode::Char('D') => {
                    push_viewer_undo(undo, lines, *line, *col);
                    let head: String = lines[*line].chars().take(*col).collect();
                    lines[*line] = head;
                    *col = (*col).min(lines[*line].chars().count().saturating_sub(1));
                    *goal = *col;
                    *dirty = true;
                    consumed = true;
                }
                // J — join the next line onto this one with a single space.
                KeyCode::Char('J') => {
                    if *line + 1 < lines.len() {
                        push_viewer_undo(undo, lines, *line, *col);
                        let next = lines.remove(*line + 1);
                        let cur = lines[*line].trim_end().to_string();
                        *col = cur.chars().count();
                        let joined = if cur.is_empty() {
                            next.trim_start().to_string()
                        } else if next.trim_start().is_empty() {
                            cur
                        } else {
                            format!("{} {}", cur, next.trim_start())
                        };
                        lines[*line] = joined;
                        *goal = *col;
                        *dirty = true;
                    }
                    consumed = true;
                }
                // o / O — open a line below/above and start typing.
                KeyCode::Char('o') if visual.is_none() => {
                    push_viewer_undo(undo, lines, *line, *col);
                    lines.insert(*line + 1, String::new());
                    *line += 1;
                    (*col, *goal) = (0, 0);
                    (*editing, *dirty) = (true, true);
                    entered_editing = true;
                    consumed = true;
                }
                KeyCode::Char('O') => {
                    push_viewer_undo(undo, lines, *line, *col);
                    lines.insert(*line, String::new());
                    (*col, *goal) = (0, 0);
                    (*editing, *dirty) = (true, true);
                    entered_editing = true;
                    consumed = true;
                }
                // a — insert after the cursor (i's sibling; A stays Coding).
                KeyCode::Char('a') if visual.is_none() => {
                    push_viewer_undo(undo, lines, *line, *col);
                    *col = (*col + 1).min(lines[*line].chars().count());
                    *goal = *col;
                    *editing = true;
                    entered_editing = true;
                    consumed = true;
                }
                // I — insert at the first non-blank of the line.
                KeyCode::Char('I') => {
                    push_viewer_undo(undo, lines, *line, *col);
                    *col = lines[*line]
                        .chars()
                        .position(|c| !c.is_whitespace())
                        .unwrap_or(0);
                    *goal = *col;
                    *editing = true;
                    entered_editing = true;
                    consumed = true;
                }
                // u — undo the last change (Ctrl+u still scrolls).
                KeyCode::Char('u') => {
                    match undo.pop() {
                        Some(snap) => {
                            view.lines = snap.lines;
                            *line = snap.line.min(view.lines.len().saturating_sub(1));
                            *col = snap.col;
                            *goal = *col;
                            // The stack bottom is the buffer as loaded, so an
                            // emptied stack means we are back at the original.
                            *dirty = !undo.is_empty();
                        }
                        None => {
                            self.message = Some(tr(
                                self.lang,
                                "already at oldest change",
                                "これ以上戻れません",
                            ).into());
                            return true;
                        }
                    }
                    consumed = true;
                }
                _ => return false,
            }
            if consumed {
                // The buffer changed shape: stale highlight, cursor may be
                // off-screen.
                hl.clear();
                let last = view.lines.len().saturating_sub(1);
                *line = (*line).min(last);
                if *line < *scroll {
                    *scroll = *line;
                } else if *line >= *scroll + body_h {
                    *scroll = *line + 1 - body_h;
                }
            }
        }
        if entered_editing {
            self.entered_editing_message();
        }
        consumed
    }

    /// The built-in plain-text editor: modeless while `editing` — printable keys
    /// insert, the usual editing/motion keys apply, Ctrl+S saves, Esc leaves.
    fn handle_editor_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(
            &self.popup,
            Popup::Viewer { view, .. } if view.kind == cian_core::viewer::ViewKind::Binary
        ) {
            return self.handle_hex_editor_key(key);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S')) {
            self.save_viewer_file();
            return Ok(());
        }
        if key.code == KeyCode::Esc {
            if let Popup::Viewer { editing, .. } = &mut self.popup {
                *editing = false;
            }
            self.message = Some(tr(self.lang, "left edit mode", "編集モード終了").into());
            return Ok(());
        }
        let body_h = (self.viewer_rect.height as usize).max(1).saturating_sub(1).max(1);
        if let Popup::Viewer { view, line, col, scroll, goal, dirty, hl, .. } = &mut self.popup {
            // Any edit invalidates the cached highlight; it recomputes on exit.
            hl.clear();
            let lines = &mut view.lines;
            if lines.is_empty() {
                lines.push(String::new());
            }
            let line_chars = |lines: &[String], l: usize| -> Vec<char> { lines[l].chars().collect() };
            let last_line = lines.len().saturating_sub(1);
            *line = (*line).min(last_line);
            let cur_len = lines[*line].chars().count();
            *col = (*col).min(cur_len);
            match key.code {
                KeyCode::Char(c) if !ctrl => {
                    let mut chs = line_chars(lines, *line);
                    chs.insert(*col, c);
                    lines[*line] = chs.into_iter().collect();
                    *col += 1;
                    *dirty = true;
                }
                KeyCode::Tab => {
                    let mut chs = line_chars(lines, *line);
                    for _ in 0..4 {
                        chs.insert(*col, ' ');
                        *col += 1;
                    }
                    lines[*line] = chs.into_iter().collect();
                    *dirty = true;
                }
                KeyCode::Enter => {
                    let chs = line_chars(lines, *line);
                    let head: String = chs[..*col].iter().collect();
                    let tail: String = chs[*col..].iter().collect();
                    lines[*line] = head;
                    lines.insert(*line + 1, tail);
                    *line += 1;
                    *col = 0;
                    *dirty = true;
                }
                KeyCode::Backspace => {
                    if *col > 0 {
                        let mut chs = line_chars(lines, *line);
                        chs.remove(*col - 1);
                        lines[*line] = chs.into_iter().collect();
                        *col -= 1;
                    } else if *line > 0 {
                        let cur = lines.remove(*line);
                        *line -= 1;
                        *col = lines[*line].chars().count();
                        lines[*line].push_str(&cur);
                    }
                    *dirty = true;
                }
                KeyCode::Delete => {
                    let chs = line_chars(lines, *line);
                    if *col < chs.len() {
                        let mut chs = chs;
                        chs.remove(*col);
                        lines[*line] = chs.into_iter().collect();
                        *dirty = true;
                    } else if *line + 1 < lines.len() {
                        let next = lines.remove(*line + 1);
                        lines[*line].push_str(&next);
                        *dirty = true;
                    }
                }
                KeyCode::Left => {
                    if *col > 0 {
                        *col -= 1;
                    } else if *line > 0 {
                        *line -= 1;
                        *col = lines[*line].chars().count();
                    }
                }
                KeyCode::Right => {
                    if *col < cur_len {
                        *col += 1;
                    } else if *line < last_line {
                        *line += 1;
                        *col = 0;
                    }
                }
                KeyCode::Up => {
                    if *line > 0 {
                        *line -= 1;
                        *col = (*col).min(lines[*line].chars().count());
                    }
                }
                KeyCode::Down => {
                    if *line < last_line {
                        *line += 1;
                        *col = (*col).min(lines[*line].chars().count());
                    }
                }
                KeyCode::Home => *col = 0,
                KeyCode::End => *col = cur_len,
                KeyCode::PageUp => {
                    *line = line.saturating_sub(body_h);
                    *col = (*col).min(lines[*line].chars().count());
                }
                KeyCode::PageDown => {
                    *line = (*line + body_h).min(last_line);
                    *col = (*col).min(lines[*line].chars().count());
                }
                _ => {}
            }
            *goal = *col;
            // Keep the cursor on screen (the render also follows it).
            if *line < *scroll {
                *scroll = *line;
            } else if *line >= *scroll + body_h {
                *scroll = *line + 1 - body_h;
            }
        }
        Ok(())
    }

    /// The hex editor: overwrite-only byte edits on a binary view. Hex digits
    /// rewrite the nibble under the cursor and advance; arrows/hjkl move by
    /// nibble and row; Ctrl+S saves (a `.bak` of the original is written
    /// first); u undoes; Esc leaves. No insert, no delete — offsets never
    /// shift, the file size cannot change, and that is what makes patching a
    /// binary safe enough to offer.
    fn handle_hex_editor_key(&mut self, key: KeyEvent) -> Result<()> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S')) {
            self.save_viewer_file();
            return Ok(());
        }
        if key.code == KeyCode::Esc {
            if let Popup::Viewer { editing, .. } = &mut self.popup {
                *editing = false;
            }
            self.message = Some(tr(self.lang, "left hex edit", "hex編集終了").into());
            return Ok(());
        }
        let body_h = (self.viewer_rect.height as usize).max(1).saturating_sub(1).max(1);
        if let Popup::Viewer { view, line, col, scroll, dirty, undo, .. } = &mut self.popup {
            let total = view.raw_bytes().len();
            if total == 0 {
                return Ok(());
            }
            // The cursor walks nibbles: 2 per byte, 16 bytes per dump line.
            let last_byte = total - 1;
            let byte_of = |line: usize, nib: usize| (line * 16 + nib / 2).min(last_byte);
            // Recover the nibble index from the stored (line, col) pair; col
            // holds the nibble index directly (0..32) while hex-editing.
            let mut nib = (*col).min(31);
            let mut cur_byte = byte_of(*line, nib);

            match key.code {
                KeyCode::Char(c @ ('0'..='9' | 'a'..='f' | 'A'..='F')) if !ctrl => {
                    let val = c.to_digit(16).unwrap() as u8;
                    // One undo unit per visit; whole-buffer snapshots would be
                    // heavy here, so the hex path snapshots bytes on demand.
                    if undo.is_empty() {
                        undo.push(crate::ViewerSnap {
                            lines: Vec::new(),
                            line: *line,
                            col: *col,
                            bytes: Some(view.raw_bytes().to_vec()),
                        });
                    }
                    let old = view.raw_bytes()[cur_byte];
                    let new = if nib % 2 == 0 { (old & 0x0F) | (val << 4) } else { (old & 0xF0) | val };
                    view.hex_set_byte(cur_byte, new);
                    *dirty = true;
                    // Advance to the next nibble, wrapping to the next row.
                    if nib < 31 && byte_of(*line, nib + 1) == *line * 16 + nib.div_ceil(2) {
                        nib += 1;
                    } else if cur_byte < last_byte {
                        *line += 1;
                        nib = 0;
                    }
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    if nib < 31 && byte_of(*line, nib + 1) == *line * 16 + nib.div_ceil(2) {
                        nib += 1;
                    } else if cur_byte < last_byte {
                        *line += 1;
                        nib = 0;
                    }
                }
                KeyCode::Char('h') | KeyCode::Left => {
                    if nib > 0 {
                        nib -= 1;
                    } else if *line > 0 {
                        *line -= 1;
                        nib = 31;
                    }
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if (*line + 1) * 16 < total {
                        *line += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => *line = line.saturating_sub(1),
                KeyCode::PageDown => *line = (*line + body_h).min(total.saturating_sub(1) / 16),
                KeyCode::PageUp => *line = line.saturating_sub(body_h),
                KeyCode::Char('u') => {
                    if let Some(snap) = undo.pop() {
                        if let Some(bytes) = snap.bytes {
                            view.set_raw_bytes(bytes);
                            *line = snap.line;
                            nib = snap.col.min(31);
                            *dirty = false;
                        }
                    } else {
                        self.message = Some(tr(
                            self.lang,
                            "already at oldest change",
                            "これ以上戻れません",
                        ).into());
                        return Ok(());
                    }
                }
                _ => {}
            }
            // Clamp the line to the data and stash the nibble back in `col`.
            *line = (*line).min(total.saturating_sub(1) / 16);
            cur_byte = byte_of(*line, nib);
            let _ = cur_byte;
            *col = nib;
            if *line < *scroll {
                *scroll = *line;
            } else if *line >= *scroll + body_h {
                *scroll = *line + 1 - body_h;
            }
        }
        Ok(())
    }

    /// Write the edited buffer back to disk in the file's own encoding.
    fn save_viewer_file(&mut self) {
        let (path, bytes) = if let Popup::Viewer { path, view, .. } = &self.popup {
            if view.kind == cian_core::viewer::ViewKind::Binary {
                // Hex edits write the raw bytes back — after keeping a `.bak`
                // of the original, because a binary has no undo once written.
                let bak = path.with_extension(format!(
                    "{}bak",
                    path.extension().map(|e| format!("{}.", e.to_string_lossy())).unwrap_or_default()
                ));
                if let Err(e) = std::fs::copy(path, &bak) {
                    self.message = Some(format!("backup failed, not saving: {e}"));
                    return;
                }
                (path.clone(), view.raw_bytes().to_vec())
            } else {
                // Written back with the line ending the file arrived with:
                // opening a CRLF file to read it must not quietly convert it.
                let sep = view.eol.as_str();
                let text = view.lines.join(sep) + sep;
                (path.clone(), view.encoding.encode(&text))
            }
        } else {
            return;
        };
        match std::fs::write(&path, bytes) {
            Ok(()) => {
                if let Popup::Viewer { dirty, source, view, .. } = &mut self.popup {
                    *dirty = false;
                    // Keep the preview's source copy in step with what's on disk.
                    *source = view.lines.clone();
                }
                self.message = Some(format!("saved: {}", path.display()));
                if let Some(t) = self.active_file_tabs_mut() {
                    let _ = t.active_mut().reload();
                }
                // If this file was opened from a remote pane, push the edit back.
                self.reupload_remote(&path);
            }
            Err(e) => self.message = Some(format!("save failed: {}", e)),
        }
    }

    /// Close whatever is open, jump the active pane to `path`'s folder, and put
    /// the cursor on it. Shared by the viewer and image preview's Shift+Enter.
    pub(crate) fn reveal_path_in_pane(&mut self, path: &std::path::Path) {
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

    /// `m` in the viewer: write the document's mermaid blocks into a self-
    /// contained HTML page and open it in the OS default browser, so the actual
    /// diagram is rendered (the terminal only shows the readable flow). Uses a
    /// local `mermaid.min.js` (config dir / beside the exe) when present so it
    /// works offline; otherwise the CDN, so it just works when online.
    pub(crate) fn open_mermaid_in_browser(&mut self) {
        let Popup::Viewer { source, .. } = &self.popup else { return };
        let blocks = extract_mermaid_blocks(source);
        if blocks.is_empty() {
            self.message = Some(tr(self.lang, "no mermaid blocks here", "mermaid ブロックがありません").into());
            return;
        }
        let dir = std::env::temp_dir().join("cian-mermaid");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.message = Some(format!("cannot create temp dir: {e}"));
            return;
        }
        // Prefer a local mermaid.min.js (offline); copy it next to the page.
        let local = cian_lua::config_read_path("mermaid.min.js").filter(|p| p.exists());
        let script = match &local {
            Some(js) => {
                let _ = std::fs::copy(js, dir.join("mermaid.min.js"));
                "<script src=\"mermaid.min.js\"></script>\n<script>mermaid.initialize({startOnLoad:true});</script>"
                    .to_string()
            }
            None => "<script type=\"module\">import mermaid from \"https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs\";mermaid.initialize({startOnLoad:true});</script>".to_string(),
        };
        let html = mermaid_html(&blocks, &script);
        let page = dir.join("diagram.html");
        if let Err(e) = std::fs::write(&page, html) {
            self.message = Some(format!("cannot write page: {e}"));
            return;
        }
        match os_open(&page) {
            Ok(()) => {
                let how = if local.is_some() { "offline" } else { "via CDN" };
                self.message = Some(format!("opened {} mermaid block(s) in the browser ({how})", blocks.len()));
            }
            Err(e) => self.message = Some(format!("open failed: {e}")),
        }
    }
}

/// Pull the contents of each ```mermaid ...``` fenced block out of `source`.
/// Push an undo snapshot, bounding what the stack can hold: 100 steps, and
/// oldest-first eviction once the retained buffers pass ~32MB (a viewer file
/// is capped at 4MB, so a burst of whole-file edits cannot pile up memory).
fn push_viewer_undo(undo: &mut Vec<ViewerSnap>, lines: &[String], line: usize, col: usize) {
    undo.push(ViewerSnap { lines: lines.to_vec(), line, col, bytes: None });
    let bytes = |s: &ViewerSnap| s.lines.iter().map(|l| l.len() + 1).sum::<usize>();
    let mut total: usize = undo.iter().map(bytes).sum();
    while undo.len() > 100 || (total > 32 * 1024 * 1024 && undo.len() > 1) {
        total -= bytes(&undo[0]);
        undo.remove(0);
    }
}

fn extract_mermaid_blocks(source: &[String]) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < source.len() {
        let t = source[i].trim_start();
        let is_mermaid_fence = (t.starts_with("```") || t.starts_with("~~~"))
            && t.trim_start_matches(['`', '~']).trim().eq_ignore_ascii_case("mermaid");
        if is_mermaid_fence {
            i += 1;
            let mut body = String::new();
            while i < source.len()
                && !(source[i].trim_start().starts_with("```") || source[i].trim_start().starts_with("~~~"))
            {
                body.push_str(&source[i]);
                body.push('\n');
                i += 1;
            }
            i += 1; // consume the closing fence
            if !body.trim().is_empty() {
                blocks.push(body);
            }
        } else {
            i += 1;
        }
    }
    blocks
}

/// A self-contained HTML page rendering `blocks` as mermaid diagrams.
fn mermaid_html(blocks: &[String], script: &str) -> String {
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut body = String::new();
    for b in blocks {
        body.push_str("<pre class=\"mermaid\">\n");
        body.push_str(&escape(b));
        body.push_str("</pre>\n");
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>cian — mermaid</title>\
<style>body{{background:#0f1116;color:#cdd0d8;font-family:system-ui,sans-serif;margin:0;padding:20px}}\
h3{{margin:0 0 12px;font-weight:600}}\
.mermaid{{background:#fff;border-radius:10px;padding:16px;margin:16px 0;overflow:auto}}</style>\
</head><body><h3>cian — mermaid</h3>{body}{script}</body></html>"
    )
}

#[cfg(test)]
mod mermaid_tests {
    use super::{extract_mermaid_blocks, mermaid_html};

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn extracts_only_mermaid_fences() {
        let src = lines(
            "# t\n\n```rust\nlet x=1;\n```\n\n```mermaid\ngraph TD\nA-->B\n```\n\ntext\n\n```mermaid\nsequenceDiagram\nA->>B: hi\n```\n",
        );
        let blocks = extract_mermaid_blocks(&src);
        assert_eq!(blocks.len(), 2, "two mermaid blocks, the rust one skipped");
        assert!(blocks[0].contains("graph TD") && blocks[0].contains("A-->B"));
        assert!(blocks[1].contains("sequenceDiagram"));
        assert!(!blocks.iter().any(|b| b.contains("let x=1")));
    }

    #[test]
    fn html_escapes_and_embeds_script() {
        let html = mermaid_html(&["graph TD\nA-->B & <C>\n".into()], "<script>1</script>");
        assert!(html.contains("class=\"mermaid\""));
        assert!(html.contains("A--&gt;B &amp; &lt;C&gt;"), "angle brackets & amp escaped: {html}");
        assert!(html.contains("<script>1</script>"), "the script tag is embedded");
    }
}
