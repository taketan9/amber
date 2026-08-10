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
    /// Every viewer key goes through here so the keys of a *change* can be
    /// kept for `.` — which is what makes `.` work for anything, including a
    /// `cw` and everything typed after it.
    pub(crate) fn handle_viewer_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.vim_replaying {
            return self.handle_viewer_key_inner(key);
        }
        let dirty_before = matches!(self.popup, Popup::Viewer { dirty: true, .. });
        let r = self.handle_viewer_key_inner(key);
        let editing = matches!(self.popup, Popup::Viewer { editing: true, .. });
        let op_pending = matches!(
            &self.popup,
            Popup::Viewer { pending: Some(p), .. } if matches!(p, 'd' | 'c')
        );
        let dirty_now = matches!(self.popup, Popup::Viewer { dirty: true, .. });
        if self.vim_recording.is_some() {
            self.viewer_record(key, false);
        } else if editing || op_pending || (dirty_now && !dirty_before) {
            // This key began a change.
            self.viewer_record(key, true);
        }
        // At rest again — nothing half-typed and not in the editor — so the
        // command is over and its keys are what `.` will replay.
        let at_rest = !editing
            && !op_pending
            && self.vim_obj.is_none()
            && self.vim_wait.is_none()
            && self.vim_mark_wait.is_none();
        if at_rest {
            self.viewer_end_record();
        }
        r
    }

    fn handle_viewer_key_inner(&mut self, key: KeyEvent) -> Result<()> {
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
        // Typing the text for a rectangular edit.
        if matches!(self.popup, Popup::Viewer { block_input: Some(_), .. }) {
            match key.code {
                KeyCode::Esc => {
                    if let Popup::Viewer { block_input, .. } = &mut self.popup {
                        *block_input = None;
                    }
                }
                KeyCode::Enter => self.finish_block_edit(),
                KeyCode::Backspace => {
                    if let Popup::Viewer { block_input: Some(b), .. } = &mut self.popup {
                        b.text.pop();
                    }
                }
                KeyCode::Char(c) if !ctrl => {
                    if let Popup::Viewer { block_input: Some(b), .. } = &mut self.popup {
                        b.text.push(c);
                    }
                }
                _ => {}
            }
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
        // `:` opens the command line. It used to arrive with `s/` already
        // typed, to show the shape of a replace — but the same prompt takes a
        // dozen word commands (`outline`, `ws`, `sort`, `crlf`…) and every one
        // of them had to begin by deleting two characters nobody asked for.
        // The shape lives in the hint under the prompt instead.
        //
        // It works in the Markdown preview too, so `:outline` is reachable
        // without dropping to the source first.
        // On every file, not only the ones that can be edited: `:q` is how the
        // viewer closes now, and a document that cannot be written is exactly
        // as much in need of being closed as one that can. The commands that
        // *write* check for themselves.
        if !ctrl && !alt && key.code == KeyCode::Char(':')
            && matches!(self.popup, Popup::Viewer { .. })
        {
            if let Popup::Viewer { sub_input, .. } = &mut self.popup {
                *sub_input = Some(String::new());
            }
            return Ok(());
        }
        // While editing, the built-in plain-text editor owns every key.
        if matches!(self.popup, Popup::Viewer { editing: true, .. }) {
            return self.handle_editor_key(key);
        }
        // `]c` / `[c` — the next and previous difference, vimdiff's own keys,
        // while a comparison is running.
        if !ctrl && key.code == KeyCode::Char('c') && self.viewer_diff.is_some() {
            let armed = match &self.popup {
                Popup::Viewer { pending: Some(p), .. } if matches!(p, ']' | '[') => Some(*p),
                _ => None,
            };
            if let Some(p) = armed {
                if let Popup::Viewer { pending, .. } = &mut self.popup {
                    *pending = None;
                }
                self.viewer_diff_step(p == ']');
                return Ok(());
            }
        }
        // F3 on a file docked in a pane gives it the whole window — the same
        // file, the same cursor, more room. (From the panes, F3 opens one;
        // here it is the only thing left for it to mean.)
        if key.code == KeyCode::F(3) && self.viewer_dock.take().is_some() {
            self.full_clear = true;
            return Ok(());
        }
        // Marks: `ma` here, `'a` back. Ahead of the grammar, which would read
        // the `a` of `ma` as a text object.
        if self.viewer_mark_key(key) {
            return Ok(());
        }
        // Ctrl+O / Ctrl+I walk back and forward through the places a jump
        // came from.
        if ctrl && matches!(key.code, KeyCode::Char('o') | KeyCode::Char('i')) {
            self.viewer_jump_list(key.code == KeyCode::Char('o'));
            return Ok(());
        }
        // `.` repeats the last change — the keys of it, replayed.
        if !ctrl && !alt && key.code == KeyCode::Char('.')
            && matches!(self.popup, Popup::Viewer { editing: false, .. })
        {
            self.viewer_repeat_change();
            return Ok(());
        }
        // vi's grammar, ahead of the keys it shares letters with: `i` is
        // insert on its own and the start of a text object after an operator,
        // and only the grammar knows which of the two this is.
        if self.viewer_vim_key(key) {
            return Ok(());
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
        // Tab steps to the next difference while a comparison is running —
        // and `]c` / `[c` do it either way, which is what vimdiff calls them.
        // Shift+Tab used to be "the previous one" and is the window's now.
        if !ctrl && !alt
            && key.code == KeyCode::Tab
            && self.viewer_diff.is_some()
            && matches!(self.popup, Popup::Viewer { editing: false, .. })
        {
            self.viewer_diff_step(true);
            return Ok(());
        }
        // `z` folds: `za` toggles the one at the cursor, `zR` opens every fold,
        // `zM` closes them all. Same prefix trick as the brackets, and for the
        // same reason it has to come before the edit operators — which
        // claim `a` and would otherwise swallow both halves.
        // Space is the one-handed way to fold: it is what the eye reaches for
        // over a collapsed outline, and it was doing nothing here.
        if !ctrl && !alt && key.code == KeyCode::Char(' ')
            && matches!(self.popup, Popup::Viewer { editing: false, sub_walk: None, .. })
        {
            self.toggle_viewer_fold(None);
            return Ok(());
        }
        if !ctrl && matches!(key.code, KeyCode::Char('z' | 'a' | 'A' | 'R' | 'M' | 't' | 'b')) {
            let c = match key.code {
                KeyCode::Char(c) => c,
                _ => unreachable!(),
            };
            let armed = matches!(self.popup, Popup::Viewer { pending: Some('z'), .. });
            // `b` and `t` only mean anything here after `z`; on their own they
            // are the word motion and (for now) nothing.
            if !armed && matches!(c, 'b' | 't') {
                // fall through to the motion handler
            } else if c == 'z' && !armed {
                if let Popup::Viewer { pending, .. } = &mut self.popup {
                    *pending = Some('z');
                }
                return Ok(());
            }
            if armed {
                if let Popup::Viewer { pending, .. } = &mut self.popup {
                    *pending = None;
                }
                match c {
                    // zz / zt / zb — the cursor line to the middle, the top or
                    // the bottom of the window, without moving the cursor.
                    'z' | 't' | 'b' => {
                        let body = (self.viewer_rect.height as usize).max(1);
                        if let Popup::Viewer { scroll, line, .. } = &mut self.popup {
                            *scroll = match c {
                                't' => *line,
                                'b' => line.saturating_sub(body.saturating_sub(1)),
                                _ => line.saturating_sub(body / 2),
                            };
                        }
                    }
                    'a' => self.toggle_viewer_fold(None),
                    // `zA` is the whole file as one switch: anything still
                    // open means "close it all", everything closed means
                    // "open it all". One key instead of remembering which of
                    // zR and zM is which.
                    'A' => {
                        let any_open = if let Popup::Viewer { shape, view, .. } = &self.popup {
                            match shape.as_deref() {
                                Some(sh) => sh
                                    .items
                                    .iter()
                                    .filter(|i| sh.extent_at(i.line, view.lines.len()).is_some())
                                    .any(|i| !sh.folds.contains(&i.line)),
                                None => false,
                            }
                        } else {
                            false
                        };
                        self.fold_all(any_open);
                    }
                    'R' => self.fold_all(false),
                    'M' => self.fold_all(true),
                    _ => {}
                }
                return Ok(());
            }
        }
        // Splits, on the keys the shell panel already uses for its own: the
        // reflex is the same and so should be the reach.
        if shift && matches!(key.code, KeyCode::F(8) | KeyCode::F(9) | KeyCode::F(10)) {
            match key.code {
                KeyCode::F(8) => self.split_viewer(true),
                KeyCode::F(9) => self.split_viewer(false),
                _ => self.close_viewer_split(),
            }
            return Ok(());
        }
        // Shift+H / Shift+L cross between the two halves — the same keys that
        // cross between the file panes.
        if self.viewer_split.is_some()
            && matches!(key.code, KeyCode::Char('H') | KeyCode::Char('L'))
        {
            let want = key.code == KeyCode::Char('L');
            if self.viewer_split_focus != want {
                self.swap_viewer_split();
            }
            return Ok(());
        }
        // F2 / Shift+F2 walk the open files, as they do in the shell panel.
        if matches!(key.code, KeyCode::F(2)) {
            self.viewer_switch_tab(!shift);
            return Ok(());
        }
        // `]]` / `[[` step to the next / previous outline entry — vim's section
        // motion, over the shape the outline column is showing. Doubled, like
        // vim, so a single bracket stays free.
        //
        // Ahead of the edit operators: they clear the pending key on every
        // keystroke, which would eat the first bracket of the pair.
        if !ctrl && matches!(key.code, KeyCode::Char(']') | KeyCode::Char('[')) {
            let c = if key.code == KeyCode::Char(']') { ']' } else { '[' };
            let mut empty = false;
            // Arm it and wait for the second half — `]` alone means nothing.
            if let Popup::Viewer { pending, shape, line, col, goal, md_map, view, .. } = &mut self.popup {
                if *pending == Some(c) {
                    *pending = None;
                    let items = shape.as_deref().map(|s| s.items.as_slice()).unwrap_or(&[]);
                    // The comparison happens in the file's line numbers and the
                    // jump lands in the screen's, which are the same thing only
                    // when the Markdown preview is off.
                    let here = crate::render::src_line(md_map, *line);
                    if items.is_empty() {
                        empty = true;
                    } else if let Some(t) = if c == ']' {
                        items.iter().find(|i| i.line > here).map(|i| i.line)
                    } else {
                        items.iter().rev().find(|i| i.line < here).map(|i| i.line)
                    } {
                        *line = crate::render::disp_line(md_map, &view.lines, t);
                        *col = 0;
                        *goal = 0;
                    }
                } else {
                    *pending = Some(c);
                }
            }
            if empty {
                self.message = Some(tr(self.lang,
                    "no outline for this kind of file",
                    "この種類のファイルにはアウトラインがない").into());
            }
            return Ok(());
        }
        // The vim change set (x, dd, o, u, …) works from normal mode on an
        // editable file; a consumed key stops here.
        if self.viewer_edit_operator(key) {
            return Ok(());
        }



        // Shift+Enter opens the viewer's menu — the keyboard's version of the
        // right-click, as it is in the file panes. Revealing the file in the
        // pane, which used to be here, is an item in it.
        if key.code == KeyCode::Enter && shift {
            let (c, r) = (self.viewer_rect.x + 4, self.viewer_rect.y + 2);
            self.open_viewer_menu(c, r);
            return Ok(());
        }
        // `y` copies the selection. (`c` used to as well; it is the change
        // operator now, and copy has never been what `c` means in vi.)
        if !ctrl && key.code == KeyCode::Char('y')
            && matches!(self.popup, Popup::Viewer { visual: Some(_), .. })
        {
            self.copy_viewer_selection();
            return Ok(());
        }
        // `p` / `P` paste, where vi puts them: after the cursor and before it.
        // The Markdown preview, which used to hold `p`, moved to Ctrl+E and
        // `:preview` — paste is the more general key and belongs on the
        // more general finger.
        if !ctrl && !alt && matches!(key.code, KeyCode::Char('p') | KeyCode::Char('P'))
            && matches!(self.popup, Popup::Viewer { editable: true, editing: false, .. })
        {
            self.paste_into_viewer(key.code == KeyCode::Char('P'));
            return Ok(());
        }
        // Ctrl+E toggles the Markdown preview. `:preview` does the same, and
        // is the one that works on a terminal keeping Ctrl for itself.
        if ctrl && key.code == KeyCode::Char('e') {
            self.toggle_markdown_preview();
            return Ok(());
        }
        // `m` opens any mermaid blocks as a real diagram in the browser (the
        // terminal shows the readable flow; this is the crisp picture).
        if false {
            self.open_mermaid_in_browser();
            return Ok(());
        }
        // Ctrl+A selects the whole file. The pane's own Ctrl+A never reaches
        // here — a popup owns the keyboard — so it is bound in both places,
        // and `mark_all` decides which of the two it means.
        if ctrl && key.code == KeyCode::Char('a') {
            self.mark_all();
            return Ok(());
        }
        // `=` compares the two halves, in place. Both stay editable and the
        // marks follow every edit — the difference between reading a diff and
        // working inside one.
        if !ctrl && !alt && key.code == KeyCode::Char('=') {
            self.toggle_viewer_diff();
            return Ok(());
        }
        // `?` here answers "what can I do in this window", not "what can cian
        // do" — the whole manual buries the one in the other.
        if !ctrl && !alt && key.code == KeyCode::Char('?') {
            // As a scrolling report, not a notice: the viewer's keys do not
            // fit a fixed block, and half a key list is worse than none. The
            // file itself waits behind it and comes back on Esc.
            let lines = crate::viewer_manual_lines(self.lang);
            let back = std::mem::replace(&mut self.popup, Popup::None);
            self.popup = Popup::Report {
                title: tr(self.lang, " the viewer ", " ビューア ").to_string(),
                lines,
                scroll: 0,
                back: Box::new(back),
            };
            return Ok(());
        }
        // `r` after a search: replace what was found, without typing the
        // pattern a second time. The prompt arrives as `s/<what you searched
        // for>/`, so all that is left is the replacement — and the `c` flag,
        // which walks the hits one at a time.
        if !ctrl && !alt && key.code == KeyCode::Char('r')
            && matches!(self.popup, Popup::Viewer { editable: true, .. })
        {
            let q = if let Popup::Viewer { find_query, .. } = &self.popup {
                find_query.clone().filter(|q| !q.is_empty())
            } else {
                None
            };
            match q {
                Some(q) => {
                    // The delimiter has to be one the pattern does not
                    // contain, or the prompt would arrive already broken —
                    // `/re/` patterns are full of slashes by definition.
                    let d = ['/', '#', '@', '!', '%', ',']
                        .into_iter()
                        .find(|c| !q.contains(*c))
                        .unwrap_or('/');
                    if let Popup::Viewer { sub_input, .. } = &mut self.popup {
                        *sub_input = Some(format!("s{d}{q}{d}"));
                    }
                    self.message = Some(tr(self.lang,
                        "type the replacement — add c before Enter to confirm each one",
                        "置換後の文字を入力 — 末尾に c を足すと1件ずつ確認").into());
                }
                None => {
                    self.message = Some(tr(self.lang,
                        "search with / first, then r replaces what it found",
                        "先に / で検索してから r で置換").into());
                }
            }
            return Ok(());
        }
        // `/`, `f` and `Shift+F` all open the search prompt (the pane's own
        // find keys, so the reflex carries over into the viewer and preview).
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
        // Esc abandons a half-typed command — the `48` of `48G`, the `d` of
        // `dd` — before it means anything else. vi does the same, and it is
        // the way out of "what have I pressed?" now that the prompt row shows
        // what that is.
        if key.code == KeyCode::Esc {
            if let Popup::Viewer { count, pending, .. } = &mut self.popup {
                if count.is_some() || pending.is_some() {
                    *count = None;
                    *pending = None;
                    return Ok(());
                }
            }
        }
        // `*` / `#` — search for the word under the cursor, forward or back.
        // The word is taken literally, so a name full of `.` or `(` is not
        // read as a pattern.
        if !ctrl && !alt && matches!(key.code, KeyCode::Char('*') | KeyCode::Char('#')) {
            let forward = key.code == KeyCode::Char('*');
            let word = if let Popup::Viewer { view, line, col, .. } = &self.popup {
                crate::util::word_under_cursor(view.lines.get(*line).map(String::as_str).unwrap_or(""), *col)
            } else {
                None
            };
            match word {
                Some(w) => {
                    if let Popup::Viewer { find_query, .. } = &mut self.popup {
                        *find_query = Some(w);
                    }
                    self.viewer_search_jump(forward);
                }
                None => {
                    self.message =
                        Some(tr(self.lang, "no word under the cursor", "カーソル位置に語がありません").into())
                }
            }
            return Ok(());
        }
        // The places worth being able to come back from: a jump to a line, and
        // a search landing. Noted before the cursor moves.
        if !ctrl && !alt
            && matches!(key.code, KeyCode::Char('G') | KeyCode::Char('n') | KeyCode::Char('N'))
            && matches!(self.popup, Popup::Viewer { editing: false, .. })
        {
            self.viewer_note_jump();
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
        let mut say_how_to_close = false;
        if let Popup::Viewer { view, scroll, line, col, goal, visual, anchor, count, find_query, .. } = &mut self.popup {
            let cnt = count.take();
            // How many times a motion repeats: the count, or once.
            let times = cnt.unwrap_or(1).max(1);
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
                    // Esc peels state off one layer at a time — a half-typed
                    // command (handled above), a visual selection, an active
                    // search and its highlights — and then stops. It does not
                    // close the file: in vi that is `:q`, and a key that
                    // sometimes means "never mind" and sometimes means "put
                    // this away" is a key you cannot press quickly. The ✕ in
                    // the corner is the mouse's way out.
                    if visual.is_some() {
                        *visual = None;
                    } else if find_query.is_some() {
                        *find_query = None;
                    } else {
                        say_how_to_close = true;
                    }
                }
                // Alt+v too, since a terminal that keeps Ctrl+V usually keeps
                // Ctrl+Q as well. Ahead of plain `v`, which this match would
                // otherwise claim first. `:block` is the one route nothing can
                // intercept.
                (false, KeyCode::Char('v')) if alt => {
                    start_visual(ViewVisual::Block, visual, anchor, *line, *col)
                }
                (false, KeyCode::Char('v')) => start_visual(ViewVisual::Char, visual, anchor, *line, *col),
                (false, KeyCode::Char('V')) => start_visual(ViewVisual::Line, visual, anchor, *line, *col),
                // Ctrl+Q is vim's own synonym for Ctrl+V, and exists for this
                // exact reason: plenty of terminals keep Ctrl+V for
                // themselves and never pass it on.
                (true, KeyCode::Char('v')) | (true, KeyCode::Char('q')) => {
                    start_visual(ViewVisual::Block, visual, anchor, *line, *col)
                }

                (false, KeyCode::Char('o')) if visual.is_some() => {
                    // Swap the cursor and the anchor.
                    let a = *anchor;
                    *anchor = (*line, *col);
                    *line = a.0;
                    *col = a.1;
                    *goal = *col;
                }

                // Vertical motion keeps the goal column. A count repeats the
                // motion, as it does in vi: `3j`, `5}`, `2Ctrl-d`.
                (false, KeyCode::Char('j')) | (_, KeyCode::Down) => to_line(*line + times, line, col, *goal),
                (false, KeyCode::Char('k')) | (_, KeyCode::Up) => to_line(line.saturating_sub(times), line, col, *goal),
                (false, KeyCode::Char('d')) | (true, KeyCode::Char('d')) | (_, KeyCode::PageDown) => {
                    to_line(*line + half * times, line, col, *goal)
                }
                (false, KeyCode::Char('u')) | (true, KeyCode::Char('u')) | (_, KeyCode::PageUp) => {
                    to_line(line.saturating_sub(half * times), line, col, *goal)
                }
                (true, KeyCode::Char('f')) => to_line(*line + body_h * times, line, col, *goal),
                (true, KeyCode::Char('b')) => to_line(line.saturating_sub(body_h * times), line, col, *goal),
                // `gg` is the top, `5gg` is line 5 — the other half of `G`.
                (false, KeyCode::Char('g')) => {
                    to_line(cnt.map(|c| c.saturating_sub(1)).unwrap_or(0), line, col, *goal)
                }
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
                    for _ in 0..times {
                        *line = viewer_paragraph(view, *line, false);
                    }
                    *col = 0;
                    *goal = 0;
                }
                (false, KeyCode::Char('}')) => {
                    for _ in 0..times {
                        *line = viewer_paragraph(view, *line, true);
                    }
                    *col = 0;
                    *goal = 0;
                }

                // Horizontal motion resets the goal to the real column.
                (false, KeyCode::Char('h')) | (_, KeyCode::Left) => {
                    *col = col.saturating_sub(times);
                    *goal = *col;
                }
                (false, KeyCode::Char('l')) | (_, KeyCode::Right) => {
                    *col = (*col + times).min(vlen(view, *line));
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
                    for _ in 0..times {
                        let (nl, nc) = viewer_word_forward(view, *line, *col, last);
                        *line = nl;
                        *col = nc;
                    }
                    *goal = *col;
                }
                (false, KeyCode::Char('b')) => {
                    for _ in 0..times {
                        let (nl, nc) = viewer_word_back(view, *line, *col);
                        *line = nl;
                        *col = nc;
                    }
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
        if say_how_to_close {
            self.message = Some(
                tr(
                    self.lang,
                    ":q closes this file  (:q! discards edits) — or the ✕ in the corner",
                    ":q で閉じます（:q! で変更を破棄） — 右上の ✕ でも",
                )
                .into(),
            );
        }
        Ok(())
    }

    /// Close the file being read — not the viewer.
    ///
    /// With other files open, in the tab strip or in the other half of a
    /// split, this closes *that one*: the rest are still being read and may
    /// hold unsaved edits. Only the last file closes the viewer itself. Every
    /// way out goes through here — `q`, Esc, `:q`, `:q!`, `:wq` — because a
    /// `:q` that closed the whole viewer took the other half of the split
    /// down with it.
    pub(crate) fn close_viewer_file(&mut self) {
        if !self.viewer_tabs.is_empty() || self.viewer_split.is_some() {
            self.close_viewer_tab();
            return;
        }
        // A docked file leaves the pane it was sitting in; the listing under
        // it never went anywhere.
        self.viewer_dock = None;
        // The last file: the viewer is done, so nothing of it may be left
        // behind. A stale split half would go on hijacking the screen from
        // whatever opened next.
        self.viewer_split = None;
        self.viewer_split_focus = false;
        // If this viewer was opened from a grep hit, go back to the results
        // list so the next hit is one keystroke away; otherwise just close.
        match self.find_return.take() {
            Some(back) => self.popup = *back,
            None => self.popup = Popup::None,
        }
    }

    /// Shift+F8 / Shift+F9: read two files at once, side by side or stacked.
    ///
    /// The other half shows the next open file, or a second view of this one
    /// when it is the only file open — which is the case for a long
    /// configuration file whose top and bottom have to be read together.
    pub(crate) fn split_viewer(&mut self, left_right: bool) {
        if !matches!(self.popup, Popup::Viewer { .. }) {
            return;
        }
        if self.viewer_split.is_some() {
            // Already split: this just changes which way round.
            self.viewer_split_lr = left_right;
            return;
        }
        let other = if self.viewer_tabs.is_empty() {
            self.popup.clone()
        } else {
            // The next tab along, taken out of the strip: it is on screen now,
            // so it is not one of the ones waiting behind.
            let n = self.viewer_tab_count();
            let cur = self.viewer_tab_idx.min(n - 1);
            let mut all = self.viewer_all_tabs();
            let next = (cur + 1) % all.len();
            let other = all.remove(next);
            let back = if next < cur { cur - 1 } else { cur };
            self.viewer_make_active(&mut all, back);
            other
        };
        self.viewer_split = Some(Box::new(other));
        self.viewer_split_lr = left_right;
        self.viewer_split_focus = false;
        self.full_clear = true;
        self.message = Some(tr(self.lang,
            "split — Shift+H/L crosses over, Shift+F10 closes it",
            "分割 — Shift+H/L で行き来、Shift+F10 で解除").into());
    }

    /// Shift+F10: back to one file, keeping the one being read.
    pub(crate) fn close_viewer_split(&mut self) {
        let Some(other) = self.viewer_split.take() else { return };
        // The half not in focus goes back to the tab strip rather than being
        // thrown away — it may hold unsaved edits.
        let keep_other = self.viewer_split_focus;
        let (shown, stashed) = if keep_other {
            (*other, std::mem::replace(&mut self.popup, Popup::None))
        } else {
            (std::mem::replace(&mut self.popup, Popup::None), *other)
        };
        self.popup = shown;
        let at = self.viewer_tab_idx.min(self.viewer_tabs.len());
        self.viewer_tabs.insert(at, stashed);
        self.viewer_split_focus = false;
        self.full_clear = true;
        self.message = Some(tr(self.lang, "one file again", "分割を解除しました").into());
    }

    /// Point the keyboard at the other half.
    pub(crate) fn swap_viewer_split(&mut self) {
        let Some(other) = self.viewer_split.as_mut() else { return };
        std::mem::swap(&mut self.popup, other.as_mut());
        self.viewer_split_focus = !self.viewer_split_focus;
    }

    /// How many files the viewer has open, the active one included.
    pub(crate) fn viewer_tab_count(&self) -> usize {
        self.viewer_tabs.len() + usize::from(matches!(self.popup, Popup::Viewer { .. }))
    }

    /// Every open viewer in order, with the active one put back where it
    /// belongs. Taking `self.popup` out is what makes the ordering work: the
    /// active tab is not in the list while it is on screen.
    fn viewer_all_tabs(&mut self) -> Vec<Popup> {
        let mut all = std::mem::take(&mut self.viewer_tabs);
        let at = self.viewer_tab_idx.min(all.len());
        all.insert(at, std::mem::replace(&mut self.popup, Popup::None));
        all
    }

    fn viewer_make_active(&mut self, all: &mut Vec<Popup>, idx: usize) {
        let idx = idx.min(all.len().saturating_sub(1));
        self.popup = all.remove(idx);
        self.viewer_tabs = std::mem::take(all);
        self.viewer_tab_idx = idx;
    }

    /// F2 / Shift+F2: the next or previous open file, wrapping — the same keys
    /// the shell panel uses for its tabs.
    pub(crate) fn viewer_switch_tab(&mut self, forward: bool) {
        let n = self.viewer_tab_count();
        if n < 2 {
            return;
        }
        let cur = self.viewer_tab_idx.min(n - 1);
        let next = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
        let mut all = self.viewer_all_tabs();
        self.viewer_make_active(&mut all, next);
        self.viewer_note_tab();
    }

    /// `=` in a split: mark what differs between the two halves, or stop.
    pub(crate) fn toggle_viewer_diff(&mut self) {
        if self.viewer_diff.take().is_some() {
            self.message = Some(tr(self.lang, "comparison off", "差分表示を解除").into());
            return;
        }
        if self.viewer_split.is_none() {
            self.message = Some(tr(self.lang,
                "split first — Shift+F8 puts two files side by side",
                "先に分割してください — Shift+F8 で2つ並びます").into());
            return;
        }
        self.recompute_viewer_diff();
        let n = self
            .viewer_diff
            .as_deref()
            .map(|d| d.mine.iter().filter(|m| **m != cian_core::diff::Mark::Same).count())
            .unwrap_or(0);
        self.message = Some(if self.lang == Lang::Ja {
            format!("差分 {n} 行（Tab / Shift+Tab で移動、= で解除）")
        } else {
            format!("{n} line(s) differ — Tab / Shift+Tab to step, = to stop")
        });
    }

    /// Work the marks out again. Cheap enough to do whenever either buffer has
    /// moved on, which is what keeps the comparison honest while editing.
    pub(crate) fn recompute_viewer_diff(&mut self) {
        let lines_of = |p: &Popup| match p {
            Popup::Viewer { view, .. } => Some(view.lines.clone()),
            _ => None,
        };
        let (Some(a), Some(b)) = (
            lines_of(&self.popup),
            self.viewer_split.as_deref().and_then(lines_of),
        ) else {
            self.viewer_diff = None;
            return;
        };
        let fp = (crate::content_key(&a), crate::content_key(&b));
        let (mine, theirs) = cian_core::diff::marks(&a, &b);
        self.viewer_diff = Some(Box::new(crate::ViewerDiff { mine, theirs, fp }));
    }

    /// `]c` / `[c`: the next or previous line that differs, in this half.
    pub(crate) fn viewer_diff_step(&mut self, forward: bool) {
        let Some(d) = self.viewer_diff.as_deref() else {
            self.message = Some(tr(self.lang, "no comparison — = starts one", "差分表示していません — = で開始").into());
            return;
        };
        let marks = d.mine.clone();
        let mut moved = None;
        if let Popup::Viewer { line, col, goal, .. } = &mut self.popup {
            let here = *line;
            let found = if forward {
                (here + 1..marks.len()).find(|i| marks[*i] != cian_core::diff::Mark::Same)
            } else {
                (0..here).rev().find(|i| marks[*i] != cian_core::diff::Mark::Same)
            };
            if let Some(t) = found {
                *line = t;
                *col = 0;
                *goal = 0;
                moved = Some(t);
            }
        }
        if moved.is_none() {
            self.message = Some(tr(self.lang, "no more differences that way", "その方向に差分はありません").into());
        }
    }

    /// Show the `i`-th open file — the tab strip's click.
    pub(crate) fn viewer_goto_tab(&mut self, i: usize) {
        let n = self.viewer_tab_count();
        if n < 2 || i >= n || i == self.viewer_tab_idx {
            return;
        }
        let mut all = self.viewer_all_tabs();
        self.viewer_make_active(&mut all, i);
        self.viewer_note_tab();
    }

    /// Close the file on screen and show the next one along.
    pub(crate) fn close_viewer_tab(&mut self) {
        if self.viewer_tabs.is_empty() {
            // Nothing waiting, but the other half of a split is still on
            // screen: that one becomes the file being read.
            if let Some(other) = self.viewer_split.take() {
                self.popup = *other;
                self.viewer_split_focus = false;
                self.full_clear = true;
                return;
            }
            self.popup = Popup::None;
            return;
        }
        let mut all = self.viewer_all_tabs();
        let at = self.viewer_tab_idx.min(all.len().saturating_sub(1));
        all.remove(at);
        let next = at.min(all.len().saturating_sub(1));
        self.viewer_make_active(&mut all, next);
        self.viewer_note_tab();
    }

    fn viewer_note_tab(&mut self) {
        let n = self.viewer_tab_count();
        let i = self.viewer_tab_idx + 1;
        let name = if let Popup::Viewer { title, .. } = &self.popup {
            title.clone()
        } else {
            String::new()
        };
        self.message = Some(format!("{name}   [{i}/{n}]"));
    }

    /// Open every marked file at once, as tabs. The first is on screen and the
    /// rest are a keystroke away, which is the point of having marked them.
    pub(crate) fn open_viewer_tabs(&mut self, paths: &[std::path::PathBuf]) {
        let mut all: Vec<Popup> = Vec::new();
        for p in paths {
            let title = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            self.open_viewer_at(p, &title, 0);
            if matches!(self.popup, Popup::Viewer { .. }) {
                all.push(std::mem::replace(&mut self.popup, Popup::None));
            }
        }
        if all.is_empty() {
            return;
        }
        self.viewer_make_active(&mut all, 0);
        if self.viewer_tab_count() > 1 {
            self.viewer_note_tab();
        }
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

    /// Open or close a fold. `at` names the heading line; `None` means the one
    /// the cursor is on or, failing that, the heading it sits under — so `za`
    /// in the middle of a function closes that function.
    pub(crate) fn toggle_viewer_fold(&mut self, at: Option<usize>) {
        let mut nothing = true;
        if let Popup::Viewer { shape, view, line, col, goal, visual, .. } = &mut self.popup {
            let total = view.lines.len();
            if let Some(sh) = shape.as_deref_mut() {
                let head = at
                    .or_else(|| sh.items.iter().rev().find(|i| i.line <= *line).map(|i| i.line))
                    .filter(|h| sh.extent_at(*h, total).is_some());
                if let Some(head) = head {
                    nothing = false;
                    if !sh.folds.remove(&head) {
                        sh.folds.insert(head);
                        // The cursor cannot stay inside something it can no
                        // longer see; it lands on the heading that closed.
                        if *line > head {
                            *line = head;
                            *col = 0;
                            *goal = 0;
                            *visual = None;
                        }
                    }
                }
            }
        }
        if nothing {
            self.message = Some(tr(self.lang, "nothing to fold here", "ここには折りたためるものがない").into());
        }
    }

    /// `zR` / `zM`: open every fold, or close every one that has anything in it.
    pub(crate) fn fold_all(&mut self, close: bool) {
        let mut n = 0usize;
        if let Popup::Viewer { shape, view, line, col, goal, visual, .. } = &mut self.popup {
            let total = view.lines.len();
            let Some(sh) = shape.as_deref_mut() else { return };
            sh.folds.clear();
            if close {
                let heads: Vec<usize> = sh
                    .items
                    .iter()
                    .map(|i| i.line)
                    .filter(|l| sh.extent_at(*l, total).is_some())
                    .collect();
                n = heads.len();
                sh.folds.extend(heads);
                if let Some(h) = sh.enclosing_fold(*line, total) {
                    *line = h;
                    *col = 0;
                    *goal = 0;
                    *visual = None;
                }
            }
        }
        self.message = Some(if close {
            format!("{n} fold(s) closed")
        } else {
            tr(self.lang, "all folds open", "折りたたみを全部展開").into()
        });
    }

    /// Run a `:s/old/new/flags` typed at the viewer's replace prompt.
    /// Run a `:s/old/new/flags` typed at the viewer's replace prompt.
    ///
    /// Without `c` every replacement lands at once, as one undo step. With
    /// `c` the hits are walked one at a time — see [`Self::sub_walk_key`].
    /// A visual selection limits the range, which is how "just this block"
    /// is expressed without a separate syntax.
    pub(crate) fn run_substitute(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
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
        // The line-transform verbs. Each acts on a v/V selection when there is
        // one, else the whole buffer, and lands as one undo step.
        let words: Vec<&str> = cmd.split_whitespace().collect();
        let arg_usize = |i: usize, d: usize| words.get(i).and_then(|w| w.parse().ok()).unwrap_or(d);
        use cian_core::textops as tx;
        let transform: Option<LineTransform> = match words.first().copied() {
            Some("sort") => Some(Box::new(|l: &[String]| tx::sort(l, false))),
            Some("rsort") => Some(Box::new(|l: &[String]| tx::sort(l, true))),
            Some("uniq") => Some(Box::new(tx::uniq)),
            Some("han") => Some(Box::new(|l: &[String]| {
                l.iter().map(|s| tx::to_halfwidth(s)).collect()
            })),
            Some("zen") => Some(Box::new(|l: &[String]| {
                l.iter().map(|s| tx::to_fullwidth(s)).collect()
            })),
            // `expand` converts the indent only, which is what protects the
            // tabs inside a tab-separated file. `expand all` is the other
            // thing people want, and has to be asked for by name because it
            // destroys those separators.
            Some("expand") => {
                let all = words.get(1) == Some(&"all");
                let w = arg_usize(if all { 2 } else { 1 }, cian_core::viewer::tab_width());
                Some(Box::new(move |l: &[String]| {
                    if all {
                        tx::expand_all_tabs(l, w)
                    } else {
                        tx::expand_tabs(l, w)
                    }
                }))
            }
            Some("unexpand") => {
                let w = arg_usize(1, cian_core::viewer::tab_width());
                Some(Box::new(move |l: &[String]| tx::unexpand_tabs(l, w)))
            }
            Some("reindent") => {
                let w = arg_usize(1, 2);
                Some(Box::new(move |l: &[String]| tx::reindent(l, w)))
            }
            _ => None,
        };
        if let Some(f) = transform {
            self.apply_line_transform(&f, words[0]);
            return;
        }
        // vi's own file commands. They exist here because Ctrl+S is not a key
        // every terminal is willing to hand over — iTerm2 keeps Ctrl+F for its
        // find bar and macOS takes Ctrl+Q for zoom — and a viewer you can edit
        // but cannot save is worse than one you cannot edit.
        match words.first().copied().unwrap_or("") {
            "w" | "write" | "saveas" | "wa" => {
                // `:w <name>` writes it there and adopts the name — which is
                // how a file that started empty gets one.
                let rest = words[1..].join(" ");
                if !rest.is_empty() {
                    self.save_viewer_file_as(&rest);
                    return;
                }
                if matches!(&self.popup, Popup::Viewer { path, .. } if path.as_os_str().is_empty()) {
                    self.message = Some(
                        tr(
                            self.lang,
                            "this file has no name yet — :w <name>",
                            "まだ名前がありません — :w <名前>",
                        )
                        .into(),
                    );
                    return;
                }
                if matches!(self.popup, Popup::Viewer { editable: false, .. }) {
                    self.message = Some(
                        tr(
                            self.lang,
                            "this one is read-only — it is shown as text, not held as it is",
                            "これは読み取り専用です（テキストとして表示しているだけ）",
                        )
                        .into(),
                    );
                    return;
                }
                self.save_viewer_file();
                return;
            }
            "wq" | "x" => {
                self.save_viewer_file();
                // Only close if the save actually took: a failed write that
                // silently shut the file would lose the edit.
                if matches!(self.popup, Popup::Viewer { dirty: false, .. }) {
                    self.close_viewer_file();
                }
                return;
            }
            "q" => {
                if matches!(self.popup, Popup::Viewer { dirty: true, .. }) {
                    self.message = Some(tr(self.lang,
                        "unsaved changes — :w to save, :q! to discard",
                        "未保存の変更があります — :w で保存、:q! で破棄").into());
                } else {
                    self.close_viewer_file();
                }
                return;
            }
            "q!" => {
                self.close_viewer_file();
                return;
            }
            // What used to be `S`, `A`, `B`, `e`, `E`, `m` — vi's letters,
            // handed back. Each is also on the right-click menu.
            "summary" | "summarise" | "summarize" => {
                self.summarize_viewer();
                return;
            }
            "blame" => {
                self.toggle_viewer_blame();
                return;
            }
            "enc" | "encoding" => {
                self.start_viewer_encoding_pick();
                return;
            }
            "mermaid" | "diagram" => {
                self.open_mermaid_in_browser();
                return;
            }
            // From here `:edit` means *this* file, not the one under the
            // pane's cursor.
            _ if cmd.starts_with('g') || cmd.starts_with('v') => {
                // `:g/re/d` deletes every line that matches, `:v/re/d` every
                // line that does not — the two halves of a log triage, and
                // one undo step either way.
                if let Some(rest) = cmd.strip_prefix('g').or_else(|| cmd.strip_prefix('v')) {
                    let keep_matching = cmd.starts_with('v');
                    let Some(spec) = rest.strip_prefix('/') else { return };
                    let Some((pat, action)) = spec.rsplit_once('/') else {
                        self.message = Some(
                            tr(self.lang, "usage: :g/pattern/d", "使い方: :g/パターン/d").into(),
                        );
                        return;
                    };
                    if action.trim() != "d" {
                        self.message = Some(
                            tr(self.lang, "only :g/pattern/d for now", "今のところ :g/パターン/d のみ").into(),
                        );
                        return;
                    }
                    self.viewer_global_delete(pat, keep_matching);
                    return;
                }
            }
            "edit" | "e" => {
                self.edit_viewer_file_externally();
                return;
            }
            // The block selection, for terminals that keep Ctrl+V and Ctrl+Q
            // to themselves.
            "block" => {
                if let Popup::Viewer { visual, anchor, line, col, .. } = &mut self.popup {
                    *anchor = (*line, *col);
                    *visual = Some(ViewVisual::Block);
                }
                return;
            }
            _ => {}
        }
        // The Markdown preview, by name — the key that used to do it (`p`)
        // now pastes, and Ctrl+E is not a key every terminal will hand over.
        if matches!(cmd, "preview" | "source" | "md") {
            self.toggle_markdown_preview();
            return;
        }
        // `outline` flips the shape column. Like `ws` it only changes what is
        // drawn, so it is not an undo step.
        if cmd == "outline" {
            if let Popup::Viewer { shape, .. } = &mut self.popup {
                let Some(sh) = shape.as_deref_mut() else {
                    self.message = Some(tr(self.lang,
                        "no outline rules for this kind of file",
                        "この種類のファイルにはアウトラインの規則がない").into());
                    return;
                };
                sh.shown = !sh.shown;
                let on = sh.shown;
                self.message = Some(if on {
                    tr(self.lang, "outline shown", "アウトラインを表示").into()
                } else {
                    tr(self.lang, "outline hidden", "アウトラインを非表示").into()
                });
            }
            return;
        }
        // `ws` flips the invisible-character marks. It reads the buffer
        // rather than changing it, so it is not an undo step.
        // `ruler` flips the column scale and the crosshair.
        if matches!(cmd, "ruler" | "cross") {
            self.show_ruler = !self.show_ruler;
            self.message = Some(if self.show_ruler {
                tr(self.lang, "ruler and crosshair on", "ルーラーと十字を表示").into()
            } else {
                tr(self.lang, "ruler and crosshair off", "ルーラーと十字を非表示").into()
            });
            return;
        }
        if cmd == "ws" {
            self.show_ws = !self.show_ws;
            self.message = Some(if self.show_ws {
                tr(self.lang, "showing spaces, tabs and ideographic spaces",
                              "空白・TAB・全角スペースを表示").into()
            } else {
                tr(self.lang, "hiding whitespace marks", "空白の表示をやめました").into()
            });
            return;
        }
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

    /// Run a whole-line transform over the selection (or the whole file) as
    /// one undo step. Sorting and de-duplicating change the line count, so a
    /// selection is spliced back rather than assigned in place.
    fn apply_line_transform(&mut self, f: &dyn Fn(&[String]) -> Vec<String>, verb: &str) {
        let range = if let Popup::Viewer { visual: Some(_), anchor, line, .. } = &self.popup {
            Some((anchor.0.min(*line), anchor.0.max(*line)))
        } else {
            None
        };
        let mut before = 0usize;
        let mut after = 0usize;
        let mut untouched = false;
        if let Popup::Viewer { view, undo, dirty, hl, line, visual, .. } = &mut self.popup {
            push_viewer_undo(undo, &view.lines, *line, 0);
            before = view.lines.len();
            let was = view.lines.clone();
            view.lines = match range {
                Some((lo, hi)) => {
                    let hi = hi.min(view.lines.len().saturating_sub(1));
                    let mut out = view.lines[..lo].to_vec();
                    out.extend(f(&view.lines[lo..=hi]));
                    out.extend_from_slice(&view.lines[hi + 1..]);
                    out
                }
                None => f(&view.lines),
            };
            after = view.lines.len();
            // A verb that finds nothing to do looks exactly like a verb that
            // did not run — `:expand` on a file whose tabs are all mid-line
            // being the case that prompted this.
            untouched = was == view.lines;
            *line = (*line).min(after.saturating_sub(1));
            *visual = None;
            *dirty = true;
            hl.clear();
        }
        let scope = match range {
            Some(_) => tr(self.lang, "selection", "選択範囲"),
            None => tr(self.lang, "whole file", "ファイル全体"),
        };
        self.message = Some(if untouched {
            if self.lang == Lang::Ja {
                format!("{verb}: {scope} — 変わるものがなかった")
            } else {
                format!("{verb}: {scope} — nothing to change")
            }
        } else if before == after {
            format!("{verb}: {scope}")
        } else if self.lang == Lang::Ja {
            format!("{verb}: {scope} — {} 行 → {} 行", before, after)
        } else {
            format!("{verb}: {scope} — {before} lines → {after}")
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
        let mut block_prompt: Option<crate::BlockInput> = None;
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
                // `~` — swap the case of the character under the cursor and
                // step over it, so holding it walks a word.
                KeyCode::Char('~') if visual.is_none() => {
                    push_viewer_undo(undo, lines, *line, *col);
                    let n = cnt(count);
                    let mut chars: Vec<char> = lines[*line].chars().collect();
                    for _ in 0..n {
                        let Some(c) = chars.get(*col).copied() else { break };
                        let swapped: String = if c.is_uppercase() {
                            c.to_lowercase().collect()
                        } else {
                            c.to_uppercase().collect()
                        };
                        // A case change can be more than one character (ß), so
                        // splice rather than assign.
                        chars.splice(*col..=*col, swapped.chars());
                        *col = (*col + swapped.chars().count()).min(chars.len());
                    }
                    lines[*line] = chars.into_iter().collect();
                    *goal = *col;
                    *dirty = true;
                    consumed = true;
                }
                // `>>` / `<<` — shift lines by one tab stop. In a `v`/`V`
                // selection a single `>` or `<` does the whole range, which is
                // the shape everyone actually uses it in.
                KeyCode::Char('>') | KeyCode::Char('<') => {
                    let out = key.code == KeyCode::Char('>');
                    let (from, to) = match visual.take() {
                        Some(_) => {
                            let (s, e) = crate::util::order_pos(*anchor, (*line, *col));
                            (s.0, e.0.min(lines.len() - 1))
                        }
                        None => {
                            let n = cnt(count);
                            (*line, (*line + n - 1).min(lines.len() - 1))
                        }
                    };
                    push_viewer_undo(undo, lines, *line, *col);
                    let width = cian_core::viewer::tab_width().max(1);
                    let pad = " ".repeat(width);
                    for l in lines[from..=to].iter_mut() {
                        if out {
                            if !l.is_empty() {
                                l.insert_str(0, &pad);
                            }
                        } else if l.starts_with('\t') {
                            // Take back one stop, whether it is a tab or spaces.
                            l.remove(0);
                        } else {
                            let take = l.chars().take(width).take_while(|c| *c == ' ').count();
                            l.drain(..take);
                        }
                    }
                    *line = from;
                    *col = 0;
                    *goal = 0;
                    *dirty = true;
                    consumed = true;
                }
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
                        ViewVisual::Block => {
                            let b = cian_core::textops::Block::between(lines, *anchor, (*line, *col));
                            *lines = cian_core::textops::block_delete(lines, b);
                            *line = b.top;
                            *col = b.left;
                        }
                    }
                    *goal = *col;
                    *dirty = true;
                    consumed = true;
                }
                // I / A / c on a rectangle: type once, land on every line.
                KeyCode::Char(k @ ('I' | 'A' | 'c'))
                    if *visual == Some(ViewVisual::Block) =>
                {
                    let b = cian_core::textops::Block::between(lines, *anchor, (*line, *col));
                    let kind = match k {
                        'I' => crate::BlockEdit::Insert,
                        'A' => crate::BlockEdit::Append,
                        _ => crate::BlockEdit::Replace,
                    };
                    *visual = None;
                    block_prompt = Some(crate::BlockInput { block: b, kind, text: String::new() });
                    consumed = true;
                }
                // …and on a line selection, where they mean the start of each
                // line and the end of each line. Vim reserves these for the
                // rectangle, but "put a comma on the end of all of these" is
                // asked for far more often than a column is, and V is the
                // easier selection to make.
                KeyCode::Char(k @ ('I' | 'A')) if *visual == Some(ViewVisual::Line) => {
                    let b = cian_core::textops::Block {
                        top: (*line).min(anchor.0),
                        bottom: (*line).max(anchor.0),
                        left: 0,
                        right: 0,
                    };
                    let kind = if k == 'I' {
                        crate::BlockEdit::LineStart
                    } else {
                        crate::BlockEdit::LineEnd
                    };
                    *visual = None;
                    block_prompt = Some(crate::BlockInput { block: b, kind, text: String::new() });
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
        if let Some(b) = block_prompt {
            let kind = b.kind;
            if let Popup::Viewer { block_input, .. } = &mut self.popup {
                *block_input = Some(Box::new(b));
            }
            self.message = Some(match kind {
                crate::BlockEdit::Insert => tr(self.lang,
                    "type the text to insert down the left edge, Enter to apply",
                    "左端に差し込む文字を入力、Enter で適用").into(),
                crate::BlockEdit::Append => tr(self.lang,
                    "type the text to append at the right edge, Enter to apply",
                    "右端に追記する文字を入力、Enter で適用").into(),
                crate::BlockEdit::LineStart => tr(self.lang,
                    "type the text to put at the start of every line, Enter to apply",
                    "各行の先頭に入れる文字を入力、Enter で適用").into(),
                crate::BlockEdit::LineEnd => tr(self.lang,
                    "type the text to put at the end of every line, Enter to apply",
                    "各行の末尾に付ける文字を入力、Enter で適用").into(),
                crate::BlockEdit::Replace => tr(self.lang,
                    "type what replaces the rectangle, Enter to apply",
                    "矩形を置き換える文字を入力、Enter で適用").into(),
            });
        }
        if entered_editing {
            self.entered_editing_message();
        }
        consumed
    }

    /// Apply the rectangular edit whose text was just typed.
    fn finish_block_edit(&mut self) {
        let Some(b) = (if let Popup::Viewer { block_input, .. } = &mut self.popup {
            block_input.take()
        } else {
            None
        }) else {
            return;
        };
        use cian_core::textops as tx;
        let rows = b.block.bottom - b.block.top + 1;
        if let Popup::Viewer { view, undo, dirty, hl, line, col, .. } = &mut self.popup {
            push_viewer_undo(undo, &view.lines, *line, *col);
            view.lines = match b.kind {
                crate::BlockEdit::Insert => tx::block_insert(&view.lines, b.block, &b.text),
                crate::BlockEdit::Append => tx::block_append(&view.lines, b.block, &b.text),
                crate::BlockEdit::Replace => tx::block_replace(&view.lines, b.block, &b.text),
                crate::BlockEdit::LineStart => {
                    tx::line_affix(&view.lines, b.block.top, b.block.bottom, &b.text, false)
                }
                crate::BlockEdit::LineEnd => {
                    tx::line_affix(&view.lines, b.block.top, b.block.bottom, &b.text, true)
                }
            };
            *line = b.block.top;
            *col = b.block.left;
            *dirty = true;
            hl.clear();
        }
        self.message = Some(if self.lang == Lang::Ja {
            format!("{} 行に適用しました", rows)
        } else {
            format!("applied to {rows} line(s)")
        });
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
                let mut bytes = Vec::new();
                // …and with the byte-order mark it arrived with. Dropping one
                // is a real edit to the file — it is exactly what `:nobom`
                // exists to do on purpose — so a save must not do it by
                // accident.
                if view.bom {
                    bytes.extend_from_slice(match view.encoding {
                        cian_core::viewer::TextEncoding::Utf8 => &[0xEF, 0xBB, 0xBF][..],
                        cian_core::viewer::TextEncoding::Utf16Le => &[0xFF, 0xFE][..],
                        cian_core::viewer::TextEncoding::Utf16Be => &[0xFE, 0xFF][..],
                        cian_core::viewer::TextEncoding::ShiftJis => &[][..],
                    });
                }
                bytes.extend_from_slice(&view.encoding.encode(&text));
                (path.clone(), bytes)
            }
        } else {
            return;
        };
        match std::fs::write(&path, bytes) {
            Ok(()) => {
                // A member opened from inside an archive is a temp file; the
                // save the user asked for is into the archive.
                if let Some(err) = self.write_back_to_archive(&path) {
                    self.message = Some(err);
                    return;
                }
                if let Popup::Viewer { dirty, source, view, .. } = &mut self.popup {
                    *dirty = false;
                    // Keep the preview's source copy in step with what's on disk.
                    *source = view.lines.clone();
                }
                self.message = Some(match self.arc_edits.get(&path) {
                    Some((a, m)) => format!(
                        "saved into {}: {m}",
                        a.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()
                    ),
                    None => format!("saved: {}", path.display()),
                });
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
                    // With the trailing newline, because whole lines were
                    // taken: it is what tells a paste to put them back as
                    // whole lines, and what every other program expects of a
                    // line copied to the clipboard.
                    Some(ViewVisual::Line) => {
                        let (a, b) = (anchor.0.min(*line), anchor.0.max(*line).min(n - 1));
                        lines[a..=b].join("\n") + "\n"
                    }
                    Some(ViewVisual::Char) => {
                        // Order the two endpoints, then take an inclusive
                        // char-wise span across the lines between them.
                        let (s, e) = order_pos((anchor.0, anchor.1), (*line, *col));
                        viewer_charwise(lines, s, e)
                    }
                    // Through the block itself, so a copy takes exactly what
                    // the highlight showed and `d` would have cut — this used
                    // to count characters while they counted columns.
                    Some(ViewVisual::Block) => {
                        let b = cian_core::textops::Block::between(lines, *anchor, (*line, *col));
                        cian_core::textops::block_text(lines, b).join("\n")
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
            let _ = cb.set_text(text.clone());
        }
        self.yank = Some(text);
        self.message = Some("copied".into());
        // A copy ends the visual gesture; leave the viewer open.
        if let Popup::Viewer { visual, .. } = &mut self.popup {
            *visual = None;
        }
    }

    /// Toggle the rendered Markdown preview.
    ///
    /// The preview is a *full* viewer — cursor, visual selection, `/` search
    /// and the mouse all work over the rendered document — so this only flips
    /// the flag and resets the cursor; the render swaps `view.lines` in and
    /// out around it.
    pub(crate) fn toggle_markdown_preview(&mut self) {
        let mut is_md = false;
        if let Popup::Viewer { preview, markdown: true, line, col, scroll, visual, .. } =
            &mut self.popup
        {
            is_md = true;
            *preview = !*preview;
            let on = *preview;
            (*line, *col, *scroll, *visual) = (0, 0, 0, None);
            self.message = Some(if on {
                tr(self.lang, "markdown: preview", "Markdown: プレビュー").into()
            } else {
                tr(self.lang, "markdown: source", "Markdown: ソース").into()
            });
        }
        if !is_md {
            self.message = Some(tr(self.lang,
                "not a Markdown file — nothing to preview",
                "Markdown ではないのでプレビューはありません").into());
        }
    }

    /// `p` / `P`: put the clipboard into the buffer, after the cursor or
    /// before it.
    ///
    /// Text that ends in a newline came from whole lines, so it goes back as
    /// whole lines — below the cursor for `p`, above it for `P`. That is vi's
    /// distinction, and the one that makes "copy these three lines, paste them
    /// there" land where meant rather than in the middle of a word.
    pub(crate) fn paste_into_viewer(&mut self, before: bool) {
        // The system clipboard first, so something copied in another program
        // pastes here; cian's own yank when there is none, so copy-and-paste
        // within a file still works where no clipboard service exists.
        let from_os = self.clipboard_text().filter(|t| !t.is_empty());
        let Some(text) = from_os.or_else(|| self.yank.clone()) else {
            self.message = Some(tr(self.lang,
                "nothing has been copied yet",
                "まだ何もコピーしていません").into());
            return;
        };
        self.put_text_in_viewer(&text, before);
    }

    /// Put `text` into the file at the cursor, as `p` / `P` would.
    ///
    /// Also where a terminal paste lands (Cmd/Ctrl+V): it arrives as one event
    /// carrying the whole text, and going through here means one edit and one
    /// repaint rather than the text being typed in a character at a time.
    pub(crate) fn put_text_in_viewer(&mut self, text: &str, before: bool) {
        let text = text.replace("\r\n", "\n");
        // Whole lines or a run of characters? `p` on a copied *line* puts it on
        // its own line, which is the distinction that makes vi's paste land
        // where meant. In the editor there is no such question: the caret is
        // what everything is relative to, and a trailing newline is a line
        // break to type, not an instruction to push whole lines above.
        let editing = matches!(self.popup, Popup::Viewer { editing: true, .. });
        let linewise = !editing && text.ends_with('\n');
        let parts: Vec<String> = if linewise {
            text.trim_end_matches('\n').split('\n').map(str::to_string).collect()
        } else {
            text.split('\n').map(str::to_string).collect()
        };
        // Not into a binary file. What is on screen there is a hex *rendering*
        // of the bytes, not the bytes; text pushed into it would be saved as
        // whatever that rendering parses back to. Hex editing is overwrite-
        // only for the same reason — the size must not change.
        if matches!(&self.popup, Popup::Viewer { view, .. } if view.kind == cian_core::viewer::ViewKind::Binary)
        {
            self.message = Some(
                tr(
                    self.lang,
                    "a binary file takes hex edits, not pasted text",
                    "バイナリファイルには貼り付けできません（16進で上書き編集）",
                )
                .into(),
            );
            return;
        }
        let mut n = 0usize;
        if let Popup::Viewer { view, undo, dirty, hl, line, col, goal, .. } = &mut self.popup {
            push_viewer_undo(undo, &view.lines, *line, *col);
            if view.lines.is_empty() {
                view.lines.push(String::new());
            }
            // A file with nothing in it has no line to paste *after*: the text
            // becomes the file, rather than landing under a blank first line.
            let blank_file = view.lines.len() == 1 && view.lines[0].is_empty();
            let at = (*line).min(view.lines.len() - 1);
            if linewise {
                let put = if before || blank_file { at } else { at + 1 };
                for (i, p) in parts.iter().enumerate() {
                    view.lines.insert(put + i, p.clone());
                }
                if blank_file {
                    view.lines.pop(); // the line that was standing in for nothing
                }
                *line = put;
                *col = 0;
                *goal = 0;
            } else {
                let chars: Vec<char> = view.lines[at].chars().collect();
                // `p` goes after the character the cursor is on; `P` at it.
                let cut = (*col + usize::from(!before && !chars.is_empty())).min(chars.len());
                let head: String = chars[..cut].iter().collect();
                let tail: String = chars[cut..].iter().collect();
                if parts.len() == 1 {
                    view.lines[at] = format!("{head}{}{tail}", parts[0]);
                    *col = cut + parts[0].chars().count();
                } else {
                    view.lines[at] = format!("{head}{}", parts[0]);
                    for (i, p) in parts[1..].iter().enumerate() {
                        view.lines.insert(at + 1 + i, p.clone());
                    }
                    let last = at + parts.len() - 1;
                    let end = view.lines[last].chars().count();
                    view.lines[last].push_str(&tail);
                    *line = last;
                    *col = end;
                }
                *goal = *col;
            }
            n = parts.len();
            *dirty = true;
            hl.clear();
        }
        self.message = Some(if self.lang == Lang::Ja {
            format!("{n} 行を貼り付けました")
        } else {
            format!("pasted {n} line(s)")
        });
    }

    /// vi's operator grammar, ahead of every other key in the viewer.
    ///
    /// `{count}{d|c|y}{count}{motion}` and `{d|c|y}{i|a}{object}`, plus the
    /// `f` family and the motions on their own. Returns true when the key was
    /// part of a command and has been dealt with — everything else falls
    /// through to the keys the viewer had before.
    ///
    /// It lives in front because the operators claim letters the viewer used
    /// for its own features (`c`, `y`, `e`, `f`); those moved to the `:` line
    /// and the menu, where a file manager's features belong and a text
    /// editor's keys do not.
    pub(crate) fn viewer_vim_key(&mut self, key: KeyEvent) -> bool {
        let m = key.modifiers;
        if m.contains(KeyModifiers::CONTROL) || m.contains(KeyModifiers::ALT) {
            return false;
        }
        let KeyCode::Char(c) = key.code else { return false };
        // Not while something is being typed into, and not over a selection —
        // there `d`, `c` and `y` act on what is selected, which the viewer
        // already does.
        let ready = matches!(
            &self.popup,
            Popup::Viewer {
                editing: false,
                find_input: None,
                sub_input: None,
                block_input: None,
                sub_walk: None,
                visual: None,
                ..
            }
        );
        if !ready {
            return false;
        }

        // A `f`/`t` waiting for its character — as a motion on its own, or as
        // the tail of `df,`.
        if let Some(fkey) = self.vim_wait.take() {
            self.vim_last_find = Some((fkey, c));
            return self.viewer_run_motion(fkey, Some(c));
        }
        // `i` / `a` waiting for the object it belongs to.
        if let Some(around) = self.vim_obj.take() {
            let op = self.viewer_pending_op();
            let (at, lines) = self.viewer_where();
            let span = crate::vim::text_object(&lines, at, around == 'a', c);
            self.viewer_clear_op();
            return match (op, span) {
                (Some(op), Some(span)) => {
                    self.viewer_apply_op(op, span);
                    true
                }
                // An object with no operator in front of it is not a command;
                // an operator with no object simply stops.
                _ => true,
            };
        }

        let op = self.viewer_pending_op();
        // The count between the operator and its motion — the `2` of `d2w`.
        // `0` is a motion until a count has been started, and a digit after
        // that — the one ambiguity vi's counts have.
        if op.is_some() && c.is_ascii_digit() && (c != '0' || self.viewer_count_started()) {
            if let Popup::Viewer { count, .. } = &mut self.popup {
                let d = c as usize - '0' as usize;
                *count = Some(count.unwrap_or(0).saturating_mul(10) + d);
            }
            return true;
        }
        // Starting an operator. `dd`, `cc`, `yy` are the doubled forms.
        if matches!(c, 'd' | 'c' | 'y') {
            if op == Some(c) {
                let n = self.viewer_take_count().max(1);
                let (at, lines) = self.viewer_where();
                let last = (at.0 + n - 1).min(lines.len().saturating_sub(1));
                self.viewer_clear_op();
                self.viewer_apply_op(c, crate::vim::Span::Lines { first: at.0, last });
                return true;
            }
            if op.is_none() {
                if let Popup::Viewer { pending, .. } = &mut self.popup {
                    *pending = Some(c);
                }
                return true;
            }
        }
        // `i` / `a` begin a text object only while an operator waits; on their
        // own they are insert and append.
        if op.is_some() && matches!(c, 'i' | 'a') {
            self.vim_obj = Some(c);
            return true;
        }
        // The `f` family always wants a character next.
        if matches!(c, 'f' | 'F' | 't' | 'T') {
            self.vim_wait = Some(c);
            return true;
        }
        // `;` and `,` repeat the last one, forwards and backwards.
        if matches!(c, ';' | ',') {
            let Some((fkey, arg)) = self.vim_last_find else { return false };
            let fkey = if c == ',' {
                match fkey {
                    'f' => 'F',
                    'F' => 'f',
                    't' => 'T',
                    _ => 't',
                }
            } else {
                fkey
            };
            return self.viewer_run_motion(fkey, Some(arg));
        }
        // With an operator waiting, this key has to be a motion or the command
        // is abandoned — vi's rule, and it stops a stray key from deleting
        // something.
        if op.is_some() {
            let handled = self.viewer_run_motion(c, None);
            if !handled {
                self.viewer_clear_op();
            }
            return true;
        }
        false
    }

    /// Is a count already being typed? `0` is a motion until then.
    fn viewer_count_started(&self) -> bool {
        matches!(&self.popup, Popup::Viewer { count: Some(_), .. })
    }

    /// The operator waiting for a motion, if any.
    fn viewer_pending_op(&self) -> Option<char> {
        match &self.popup {
            Popup::Viewer { pending: Some(p), .. } if matches!(p, 'd' | 'c' | 'y') => Some(*p),
            _ => None,
        }
    }

    fn viewer_clear_op(&mut self) {
        self.vim_obj = None;
        self.vim_wait = None;
        if let Popup::Viewer { pending, count, .. } = &mut self.popup {
            if matches!(pending, Some('d') | Some('c') | Some('y')) {
                *pending = None;
            }
            *count = None;
        }
    }

    fn viewer_take_count(&mut self) -> usize {
        if let Popup::Viewer { count, .. } = &mut self.popup {
            count.take().unwrap_or(1).max(1)
        } else {
            1
        }
    }

    /// The cursor and the buffer, copied out so the borrow ends here.
    fn viewer_where(&self) -> ((usize, usize), Vec<String>) {
        match &self.popup {
            Popup::Viewer { line, col, view, .. } => ((*line, *col), view.lines.clone()),
            _ => ((0, 0), Vec::new()),
        }
    }

    /// Run `key` as a motion: move the cursor, or hand the span it covers to
    /// the operator that is waiting. False when it is not a motion at all.
    fn viewer_run_motion(&mut self, key: char, arg: Option<char>) -> bool {
        let op = self.viewer_pending_op();
        let n = self.viewer_take_count();
        let (at, lines) = self.viewer_where();
        // vi's one special case: `cw` on a word changes the word, not the
        // space after it — it behaves like `ce`. Everyone relies on it
        // without knowing it is a special case.
        let key = if op == Some('c')
            && key == 'w'
            && lines
                .get(at.0)
                .and_then(|l| l.chars().nth(at.1))
                .map(|c| !c.is_whitespace())
                .unwrap_or(false)
        {
            'e'
        } else {
            key
        };
        let Some(mo) = crate::vim::motion(&lines, at, key, arg, n) else {
            return false;
        };
        match op {
            Some(op) => {
                self.viewer_clear_op();
                self.viewer_apply_op(op, crate::vim::span_of(at, mo));
            }
            None => {
                if let Popup::Viewer { line, col, goal, view, .. } = &mut self.popup {
                    *line = mo.to.0.min(view.lines.len().saturating_sub(1));
                    let len = view.lines.get(*line).map(|l| l.chars().count()).unwrap_or(0);
                    *col = mo.to.1.min(len);
                    *goal = *col;
                }
            }
        }
        true
    }

    /// Delete, change or yank what `span` covers. `c` leaves the editor open
    /// where the text was, which is the whole difference between it and `d`.
    fn viewer_apply_op(&mut self, op: char, span: crate::vim::Span) {
        use crate::vim::Span;
        let text = self.viewer_span_text(span);
        if op == 'y' {
            self.yank = Some(text.clone());
            if let Some(cb) = self.clipboard.as_mut() {
                let _ = cb.set_text(text);
            }
            self.message = Some(match span {
                Span::Lines { first, last } => {
                    let n = last - first + 1;
                    if self.lang == Lang::Ja {
                        format!("{n} 行コピー")
                    } else {
                        format!("yanked {n} line(s)")
                    }
                }
                Span::Chars { .. } => tr(self.lang, "yanked", "コピーしました").into(),
            });
            return;
        }
        if !matches!(self.popup, Popup::Viewer { editable: true, .. }) {
            self.message = Some(
                tr(self.lang, "this one is read-only", "これは読み取り専用です").into(),
            );
            return;
        }
        self.yank = Some(text);
        let change = op == 'c';
        if let Popup::Viewer { view, undo, line, col, goal, dirty, hl, .. } = &mut self.popup {
            let lines = &mut view.lines;
            if lines.is_empty() {
                lines.push(String::new());
            }
            push_viewer_undo(undo, lines, *line, *col);
            match span {
                Span::Lines { first, last } => {
                    let last = last.min(lines.len() - 1);
                    if change {
                        // `cc` empties the lines rather than removing them:
                        // the point is to type a replacement where they were.
                        lines.drain(first..=last);
                        lines.insert(first, String::new());
                        *line = first;
                    } else {
                        lines.drain(first..=last);
                        if lines.is_empty() {
                            lines.push(String::new());
                        }
                        *line = first.min(lines.len() - 1);
                    }
                    *col = 0;
                }
                Span::Chars { start, end } => {
                    let (sl, sc) = start;
                    let (el, ec) = end;
                    let el = el.min(lines.len() - 1);
                    let head: String = lines[sl].chars().take(sc).collect();
                    let tail_from = ec.saturating_add(1);
                    let tail: String = lines[el].chars().skip(tail_from).collect();
                    lines[sl] = format!("{head}{tail}");
                    if el > sl {
                        lines.drain(sl + 1..=el);
                    }
                    *line = sl;
                    *col = sc;
                }
            }
            *goal = *col;
            *dirty = true;
            hl.clear();
        }
        if change {
            if let Popup::Viewer { editing, .. } = &mut self.popup {
                *editing = true;
            }
        }
    }

    /// The text `span` covers, for the yank register.
    fn viewer_span_text(&self, span: crate::vim::Span) -> String {
        use crate::vim::Span;
        let Popup::Viewer { view, .. } = &self.popup else { return String::new() };
        let lines = &view.lines;
        match span {
            Span::Lines { first, last } => {
                let last = last.min(lines.len().saturating_sub(1));
                let mut s = lines[first..=last].join("\n");
                s.push('\n');
                s
            }
            Span::Chars { start, end } => {
                let (sl, sc) = start;
                let (el, ec) = end;
                let el = el.min(lines.len().saturating_sub(1));
                if sl == el {
                    lines[sl].chars().skip(sc).take(ec.saturating_sub(sc) + 1).collect()
                } else {
                    let mut out: String = lines[sl].chars().skip(sc).collect();
                    for l in lines.iter().take(el).skip(sl + 1) {
                        out.push('\n');
                        out.push_str(l);
                    }
                    out.push('\n');
                    out.extend(lines[el].chars().take(ec + 1));
                    out
                }
            }
        }
    }

    /// `:g/re/d` — drop every line that matches (or, for `:v`, every line
    /// that does not). One undo step, and it says how many went.
    pub(crate) fn viewer_global_delete(&mut self, pattern: &str, invert: bool) {
        if pattern.is_empty() {
            self.message = Some(tr(self.lang, "no pattern", "パターンがありません").into());
            return;
        }
        let matcher = match cian_core::search::Matcher::parse(pattern) {
            Ok(m) => m,
            Err(e) => {
                self.message = Some(e.to_string());
                return;
            }
        };
        if !matches!(self.popup, Popup::Viewer { editable: true, .. }) {
            self.message =
                Some(tr(self.lang, "this one is read-only", "これは読み取り専用です").into());
            return;
        }
        let mut gone = 0usize;
        if let Popup::Viewer { view, undo, line, col, goal, dirty, hl, .. } = &mut self.popup {
            push_viewer_undo(undo, &view.lines, *line, *col);
            let before = view.lines.len();
            view.lines.retain(|l| matcher.find_ranges(l).is_empty() != invert);
            gone = before - view.lines.len();
            if view.lines.is_empty() {
                view.lines.push(String::new());
            }
            *line = (*line).min(view.lines.len() - 1);
            *col = 0;
            *goal = 0;
            if gone > 0 {
                *dirty = true;
                hl.clear();
            }
        }
        self.message = Some(if self.lang == Lang::Ja {
            format!("{gone} 行削除")
        } else {
            format!("{gone} line(s) deleted")
        });
    }

    /// `.` — do the last change again.
    ///
    /// The keys are replayed rather than the *effect* re-applied: vi's `.`
    /// means "that command, here", and a command is what was typed — the
    /// operator, its count, its motion, and for `c` everything typed into the
    /// editor before Esc. Replaying the keys gets all of that for free and
    /// cannot drift from what the keys actually do.
    pub(crate) fn viewer_repeat_change(&mut self) {
        let Some(keys) = self.vim_last_change.clone() else {
            self.message = Some(tr(self.lang, "nothing to repeat yet", "繰り返す変更がありません").into());
            return;
        };
        // Not recorded again while it runs, or `.` would rewrite itself with
        // its own replay.
        self.vim_replaying = true;
        for k in keys {
            let _ = self.handle_viewer_key_inner(k);
        }
        self.vim_replaying = false;
    }

    /// Start (or continue) recording the keys of a change.
    fn viewer_record(&mut self, key: KeyEvent, start: bool) {
        if start && self.vim_recording.is_none() {
            self.vim_recording = Some(Vec::new());
        }
        if let Some(rec) = self.vim_recording.as_mut() {
            rec.push(key);
        }
    }

    /// The change is over: keep its keys for `.`.
    fn viewer_end_record(&mut self) {
        if let Some(rec) = self.vim_recording.take() {
            if !rec.is_empty() {
                self.vim_last_change = Some(rec);
            }
        }
    }

    /// `m{a-z}` sets a mark, `'{a-z}` and `` `{a-z} `` jump to it.
    ///
    /// Per file: a mark set in one file is not somewhere to land in another,
    /// and silently jumping to line 200 of a different document is worse than
    /// saying there is no such mark here.
    fn viewer_mark_key(&mut self, key: KeyEvent) -> bool {
        let KeyCode::Char(c) = key.code else { return false };
        if key.modifiers.contains(KeyModifiers::CONTROL)
            || key.modifiers.contains(KeyModifiers::ALT)
        {
            return false;
        }
        let ready = matches!(
            &self.popup,
            Popup::Viewer { editing: false, find_input: None, sub_input: None, .. }
        );
        if !ready {
            return false;
        }
        // Waiting for the letter after `m`, `'` or a backtick.
        if let Some(what) = self.vim_mark_wait.take() {
            if !c.is_ascii_alphabetic() {
                return true; // an abandoned mark command, not a stray edit
            }
            let path = match &self.popup {
                Popup::Viewer { path, .. } => path.clone(),
                _ => return true,
            };
            match what {
                'm' => {
                    if let Popup::Viewer { line, col, .. } = &self.popup {
                        self.vim_marks.insert((path, c), (*line, *col));
                    }
                    self.message =
                        Some(if self.lang == Lang::Ja { format!("マーク {c}") } else { format!("mark {c}") });
                }
                _ => {
                    let exact = what == '`';
                    match self.vim_marks.get(&(path, c)).copied() {
                        Some((l, col)) => {
                            self.viewer_note_jump();
                            if let Popup::Viewer { line, col: cc, goal, view, .. } = &mut self.popup {
                                *line = l.min(view.lines.len().saturating_sub(1));
                                let len =
                                    view.lines.get(*line).map(|s| s.chars().count()).unwrap_or(0);
                                // `'a` is the line, backtick-a the exact spot.
                                *cc = if exact { col.min(len) } else { 0 };
                                *goal = *cc;
                            }
                        }
                        None => {
                            self.message = Some(if self.lang == Lang::Ja {
                                format!("マーク {c} はこのファイルにありません")
                            } else {
                                format!("no mark {c} in this file")
                            })
                        }
                    }
                }
            }
            return true;
        }
        if matches!(c, 'm' | '\'' | '`') {
            self.vim_mark_wait = Some(c);
            return true;
        }
        false
    }

    /// Remember where the cursor is, before a jump that could be hard to find
    /// the way back from. `Ctrl+O` walks back through these, `Ctrl+I` forward.
    pub(crate) fn viewer_note_jump(&mut self) {
        let Popup::Viewer { path, line, col, .. } = &self.popup else { return };
        let here = (path.clone(), *line, *col);
        // A new jump discards anything Ctrl+O had walked back past, as vi does.
        self.vim_jumps.truncate(self.vim_jump_at);
        if self.vim_jumps.last() == Some(&here) {
            return;
        }
        self.vim_jumps.push(here);
        const KEEP: usize = 100;
        if self.vim_jumps.len() > KEEP {
            self.vim_jumps.remove(0);
        }
        self.vim_jump_at = self.vim_jumps.len();
    }

    /// `Ctrl+O` / `Ctrl+I` — back and forward through those places.
    pub(crate) fn viewer_jump_list(&mut self, back: bool) {
        let Popup::Viewer { path, line, col, .. } = &self.popup else { return };
        let (path, here) = (path.clone(), (*line, *col));
        if back {
            if self.vim_jump_at == 0 {
                self.message = Some(tr(self.lang, "no older place", "これ以上戻れません").into());
                return;
            }
            // Stepping back for the first time keeps where we are, so Ctrl+I
            // has somewhere to return to.
            if self.vim_jump_at == self.vim_jumps.len() {
                self.vim_jumps.push((path.clone(), here.0, here.1));
            }
            self.vim_jump_at -= 1;
        } else {
            if self.vim_jump_at + 1 >= self.vim_jumps.len() {
                self.message = Some(tr(self.lang, "no newer place", "これ以上進めません").into());
                return;
            }
            self.vim_jump_at += 1;
        }
        let Some((p, l, c)) = self.vim_jumps.get(self.vim_jump_at).cloned() else { return };
        if p != path {
            self.message = Some(tr(self.lang, "that place is in another file", "別のファイルの位置です").into());
            return;
        }
        if let Popup::Viewer { line, col, goal, view, .. } = &mut self.popup {
            *line = l.min(view.lines.len().saturating_sub(1));
            let len = view.lines.get(*line).map(|s| s.chars().count()).unwrap_or(0);
            *col = c.min(len);
            *goal = *col;
        }
    }

    /// Shift+Tab: step between the file being edited and the panes behind it.
    ///
    /// The viewer is not a dialog you finish with — it is the other half of
    /// the window. So it parks rather than closes, keeping its cursor, its
    /// folds and its unsaved edits, and the same key brings it back. With
    /// nothing to come back to it opens an empty file, which is what makes
    /// this an editor you can start typing into rather than only a reader.
    pub(crate) fn toggle_viewer_park(&mut self) {
        if matches!(self.popup, Popup::Viewer { .. }) {
            self.viewer_dock = None;
            let v = std::mem::replace(&mut self.popup, Popup::None);
            self.viewer_parked = Some(Box::new(v));
            self.message = Some(
                tr(self.lang, "Shift+Tab returns to the file", "Shift+Tab で戻ります").into(),
            );
            return;
        }
        if !matches!(self.popup, Popup::None) {
            return;
        }
        match self.viewer_parked.take() {
            Some(v) => {
                self.popup = *v;
                self.full_clear = true;
            }
            None => self.open_scratch_viewer(),
        }
    }

    /// An empty file to type into, with no name yet. `:w <name>` gives it one.
    pub(crate) fn open_scratch_viewer(&mut self) {
        let mut view = cian_core::viewer::View::from_text(String::new(), 0, false);
        // One empty line rather than none: the cursor has to be somewhere, and
        // "a file with nothing in it" is a file with one blank line.
        if view.lines.is_empty() {
            view.lines.push(String::new());
        }
        self.popup = Popup::Viewer {
            title: tr(self.lang, "untitled", "無題").to_string(),
            // No path: `:w` asks for a name rather than writing somewhere it
            // was never told about.
            path: PathBuf::new(),
            view: Box::new(view),
            scroll: 0,
            line: 0,
            col: 0,
            goal: 0,
            visual: None,
            anchor: (0, 0),
            find_input: None,
            sub_input: None,
            block_input: None,
            shape: None,
            sub_walk: None,
            find_query: None,
            count: None,
            pending: None,
            git_lines: Default::default(),
            markdown: false,
            preview: false,
            source: Vec::new(),
            md_styles: Vec::new(),
            md_map: Vec::new(),
            md_width: 0,
            blame: Vec::new(),
            hl_lang: None,
            hl: Vec::new(),
            editable: true,
            editing: false,
            dirty: false,
            undo: Vec::new(),
        };
        self.full_clear = true;
        self.message = Some(
            tr(
                self.lang,
                "empty file — i to type, :w <name> to save it somewhere",
                "空のファイル — i で入力、:w <名前> で保存",
            )
            .into(),
        );
    }

    /// `:w <name>` — write this buffer to `name` and go on editing *it*.
    ///
    /// The name is taken relative to the pane you came from, because that is
    /// the folder you were looking at when you started typing.
    pub(crate) fn save_viewer_file_as(&mut self, name: &str) {
        // The folder you were looking at — the active pane's, then the last
        // file pane's. Never the process's own directory: that is wherever
        // cian happened to be started from, which is nobody's intention.
        let base = self
            .active_pane()
            .map(|p| p.cwd.clone())
            .or_else(|| self.last_file_pane_cwd())
            .unwrap_or_default();
        let want = crate::expand_path(name);
        let path = if want.is_absolute() { want } else { base.join(want) };
        if path.is_dir() {
            self.message = Some(format!("{} is a folder", path.display()));
            return;
        }
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                self.message = Some(format!("no such folder: {}", parent.display()));
                return;
            }
        }
        // Adopt the name first, then save through the ordinary path so the
        // encoding, the line ending and the BOM are written the way that one
        // writes them.
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.to_string());
        if let Popup::Viewer { path: p, title: t, editable, .. } = &mut self.popup {
            *p = path.clone();
            *t = title;
            *editable = true;
        } else {
            return;
        }
        self.save_viewer_file();
        // A new file changes what the pane is showing.
        self.reload_both();
        self.note_recent_file(&path);
    }

    /// `:enc` — pick the encoding this file is decoded with. Only in source
    /// mode; the rendered preview owns `view.lines`.
    pub(crate) fn start_viewer_encoding_pick(&mut self) {
        if matches!(self.popup, Popup::Viewer { preview: true, .. }) {
            self.message =
                Some(tr(self.lang, ":preview shows the source first", "先に :preview でソース表示に").into());
            return;
        }
        if !matches!(self.popup, Popup::Viewer { .. }) {
            return;
        }
        let cur = if let Popup::Viewer { view, .. } = &self.popup {
            cian_core::viewer::TextEncoding::ALL
                .iter()
                .position(|enc| *enc == view.encoding)
                .unwrap_or(0)
        } else {
            0
        };
        let viewer = std::mem::replace(&mut self.popup, Popup::None);
        self.popup =
            Popup::EncodingPicker { cursor: cur, target: EncTarget::Viewer(Box::new(viewer)) };
    }

    /// `:edit` from the viewer — hand the file to `$VISUAL` / `$EDITOR`.
    pub(crate) fn edit_viewer_file_externally(&mut self) {
        if !matches!(self.popup, Popup::Viewer { .. }) {
            return;
        }
        self.edit_viewed_file();
        self.popup = Popup::None;
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
