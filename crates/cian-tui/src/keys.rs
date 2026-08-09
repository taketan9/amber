//! Keyboard handling: the top-level key router, popup and shell key handling,
//! the remappable-action dispatch, and split-resize. Split out of lib.rs as an
//! `impl App` block.
use super::*;

impl App {
    /// Grow or shrink a pane from the keyboard, moving the relevant divider in
    /// the arrow's direction (Right pushes the divider right, so the pane on
    /// its left grows, and so on). Which divider depends on where the focus is:
    ///
    /// - a file pane: Left/Right move the left|right split, Up/Down the
    ///   files|shell split;
    /// - the shell: Left/Right resize the nearest side-by-side split; Up/Down
    ///   resize the nearest stacked split, or the files|shell split when the
    ///   shell has none.
    pub(crate) fn resize_split(&mut self, dir: KeyCode) {
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
    pub(crate) fn set_divider_ratio(&mut self, d: Divider, pct: u16) {
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
    /// Handle a key, and — while `:keys` is on — say what the key actually
    /// was.
    ///
    /// A terminal sits between the keyboard and cian and is free to keep,
    /// rewrite or drop any combination it likes. When a binding "does not
    /// work", the first question is whether the keystroke arrived at all, and
    /// there was no way to answer it from inside cian. Reported after the key
    /// has been handled, so it is the last word rather than the first thing
    /// overwritten.
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // Whether the message on screen is an answer to *this* keystroke.
        // Messages have no expiry — they sit there until something replaces
        // them — so anywhere that gives a message the floor has to know the
        // difference between news and a leftover, or it keeps the floor
        // forever. The viewer's footer does exactly that.
        // Cleared first, so "did this key raise a message" is a question about
        // whether one was *set* rather than whether the words changed. Pressing
        // a key that refuses twice has to say so twice; comparing the text made
        // the second refusal silent.
        let before = self.message.take();
        let r = self.handle_key_inner(key);
        self.message_fresh = self.message.is_some();
        if self.message.is_none() {
            self.message = before;
        }
        if !self.key_probe {
            return r;
        }
        if self.key_probe {
            let mut mods: Vec<&str> = Vec::new();
            for (m, name) in [
                (KeyModifiers::CONTROL, "Ctrl"),
                (KeyModifiers::ALT, "Alt"),
                (KeyModifiers::SHIFT, "Shift"),
                (KeyModifiers::SUPER, "Super"),
            ] {
                if key.modifiers.contains(m) {
                    mods.push(name);
                }
            }
            let what = match key.code {
                KeyCode::Char(c) => format!("Char({c:?})"),
                other => format!("{other:?}"),
            };
            let mods = if mods.is_empty() { "no modifier".to_string() } else { mods.join("+") };
            self.message = Some(format!("key: {what}  {mods}   (:keys to stop)"));
        }
        r
    }

    fn handle_key_inner(&mut self, key: KeyEvent) -> Result<()> {
        // A copy/move/delete or a directory-compare runs on a worker thread and
        // stays flagged in-flight until its result is polled in. Those polls
        // otherwise run only *after* this key is handled, and `event::poll`
        // blocks for up to a tick — so a job can finish in that window and the
        // "Esc is the only key" gates just below would silently swallow the very
        // next keypress (a second copy right after the first appeared to need two
        // presses). Land any finished job here first, so a gate is closed only
        // while its op is genuinely still running. Both polls no-op when idle.
        self.poll_op_job();
        self.poll_diff_job();
        // While the progress popup is up, it owns the keyboard: Esc stops the
        // op, and `b`/Enter tucks the popup away so work can continue while
        // the op runs (a status-line chip keeps showing it; `:queue` manages
        // the line). Once tucked away, every key means what it always does.
        if self.op_job.is_some() && !self.op_bar_hidden {
            match key.code {
                KeyCode::Esc => self.cancel_op_job(),
                KeyCode::Char('b') | KeyCode::Enter => {
                    self.op_bar_hidden = true;
                    self.message = Some(tr(
                        self.lang,
                        "running in the background — :queue to manage",
                        "バックグラウンドで実行中 — :queue で管理",
                    ).into());
                }
                _ => {}
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
        // Ctrl+Shift+Enter opens the snippet launcher from anywhere — crucially
        // while the SHELL pane is focused, where a plain key would just be typed
        // into the terminal. cian sees the keystroke before the PTY does, so
        // this global intercept works in either pane. (Needs a terminal that
        // reports the modifiers with Enter — the Windows console and kitty-
        // protocol terminals do; `:snip` and the top of the right-click menu
        // are the fallbacks elsewhere.)
        if key.code == KeyCode::Enter
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && key.modifiers.contains(KeyModifiers::SHIFT)
        {
            self.start_snippets();
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

    pub(crate) fn handle_popup_key(&mut self, key: KeyEvent) -> Result<()> {
        // Every popup with a keymap of its own handles the key. The rest — the
        // confirm and notice dialogs — fall through to the one keymap they
        // share. Mirrors how `draw_popup` dispatches the same set.
        match self.popup {
            Popup::Palette { .. } => self.palette_key(key),
            Popup::AiChat { .. } => self.ai_chat_key(key),
            Popup::AiHistory { .. } => self.ai_history_key(key),
            Popup::Toggles { .. } => self.toggles_key(key),
            Popup::OpQueue { .. } => self.op_queue_key(key),
            Popup::ContextMenu { .. } => self.context_menu_key(key),
            Popup::SshHosts { .. } => self.ssh_hosts_key(key),
            Popup::SshUsers { .. } => self.ssh_users_key(key),
            Popup::Snippets { .. } => self.snippets_key(key),
            Popup::RemoteBrowser { .. } => self.remote_browser_key(key),
            Popup::LocalDest { .. } => self.local_dest_key(key),
            Popup::ThemePicker { .. } => self.theme_picker_key(key),
            Popup::FindResults { .. } => self.find_results_key(key),
            Popup::GrepReplace(_) => self.grep_replace_key(key),
            Popup::DestPicker { .. } => self.dest_picker_key(key),
            Popup::ImageView { .. } => self.image_view_key(key),
            Popup::DirCompare { .. } => self.dir_compare_key(key),
            Popup::Diff { .. } => self.diff_key(key),
            Popup::DiskUsage { .. } => self.disk_usage_key(key),
            Popup::Archive { .. } => self.archive_key(key),
            Popup::GitLog { .. } => self.git_log_key(key),
            Popup::Macros { .. } => self.macros_key(key),
            Popup::SortPicker { .. } => self.sort_picker_key(key),
            Popup::ColorPicker { .. } => self.color_picker_key(key),
            Popup::Manual { .. } => self.manual_key(key),
            Popup::Report { .. } => self.report_key(key),
            Popup::Shortcuts { .. } => self.shortcuts_key(key),
            Popup::ConfirmClose { .. } => self.confirm_close_key(key),
            Popup::ConfirmElevate { .. } => self.confirm_elevate_key(key),
            Popup::AiShellConfirm { .. } => self.ai_shell_confirm_key(key),
            Popup::CommitMessage { .. } => self.commit_message_key(key),
            Popup::JunkReview { .. } => self.junk_review_key(key),
            Popup::DupeReview { .. } => self.dupe_review_key(key),
            Popup::StructureReview { .. } => self.structure_review_key(key),
            Popup::RenameReview { .. } => self.rename_review_key(key),
            Popup::TextInput { .. } => self.text_input_key(key),
            Popup::Search { .. } => self.search_key(key),
            Popup::Viewer { .. } => self.handle_viewer_key(key),
            Popup::Notice { .. } => self.notice_key(key),
            Popup::EncodingPicker { .. } => self.encoding_picker_key(key),
            Popup::History { .. } => self.history_key(key),
            _ => self.confirm_dialog_key(key),
        }
    }

    /// The confirm and notice dialogs: they differ only in their wording,
    /// so they share one y / n / Enter keymap rather than each repeating it.
    fn confirm_dialog_key(&mut self, key: KeyEvent) -> Result<()> {
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
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') => {
                // A cancelled copy-across restores the comparison it came from;
                // everything else just closes.
                if matches!(self.popup, Popup::ConfirmDiffCopy { .. }) {
                    self.cancel_diff_copy();
                } else if matches!(self.popup, Popup::ConfirmDirSync { .. }) {
                    self.cancel_dir_sync();
                } else {
                    self.popup = Popup::None;
                }
                Ok(())
            }
            // Enter is the same as `y` here: the plain "yes" everyone reaches
            // for. (Overwrite/permanent stays on its own key so it is never
            // the accidental default.)
            KeyCode::Char('y') | KeyCode::Enter => match &self.popup {
                Popup::ConfirmDelete { .. } => self.finish_delete(DeleteMode::Trash),
                Popup::ConfirmZipAdd { .. } => self.confirm_zip_add(),
                Popup::ConfirmZipDelete { .. } => self.confirm_zip_delete(),
                Popup::ConfirmNoBom { .. } => self.confirm_nobom(),
                Popup::ConfirmTransfer { .. } => self.finish_transfer(Conflict::Skip),
                Popup::ConfirmDiscard { .. } => { self.git_discard(); Ok(()) }
                Popup::ConfirmDiffCopy { .. } => { self.confirm_diff_copy(); Ok(()) }
                Popup::ConfirmDirSync { .. } => { self.confirm_dir_sync(); Ok(()) }
                Popup::ConfirmRemoteDelete { .. } => { self.confirm_remote_delete(); Ok(()) }
                Popup::ConfirmRemoteMove { .. } => { self.confirm_remote_move(); Ok(()) }
                Popup::ConfirmSnippet { cmd, enter, .. } => {
                    let (cmd, enter) = (cmd.clone(), *enter);
                    self.popup = Popup::None;
                    self.deliver_snippet(&cmd, enter);
                    Ok(())
                }
                Popup::Notice { .. } => {
                    self.popup = Popup::None;
                    self.restore_viewer();
                    Ok(())
                }
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

    fn text_input_key(&mut self, key: KeyEvent) -> Result<()> {
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
                KeyCode::Esc => { self.popup = Popup::None; Ok(())}
                KeyCode::Enter => { self.finish_text_input()}
                // Caret movement, so the middle of a name can be reached.
                KeyCode::Left => { *cursor = cursor.saturating_sub(1); Ok(())}
                KeyCode::Right => { *cursor = (*cursor + 1).min(len); Ok(())}
                KeyCode::Home => { *cursor = 0; Ok(())}
                KeyCode::End => { *cursor = len; Ok(())}
                KeyCode::Char('a') if ctrl => { *cursor = 0; Ok(())}
                KeyCode::Char('e') if ctrl => { *cursor = len; Ok(())}
                KeyCode::Backspace => { backspace_at(buffer, cursor); Ok(())}
                KeyCode::Delete => { delete_at(buffer, cursor); Ok(())}
                // Clear the line, as in any readline prompt.
                KeyCode::Char('u') | KeyCode::Char('U') if ctrl => {
                    buffer.clear();
                    *cursor = 0;
                    Ok(())
                }
                // Without this guard every Ctrl+<key> inserted its bare letter,
                // so Ctrl+V typed a "v" instead of pasting.
                KeyCode::Char(_) if ctrl => Ok(()),
                KeyCode::Char(c) => { insert_char_at(buffer, cursor, c); Ok(())}
                _ => Ok(()),
            }
    }

    fn search_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::Search { buffer } = &mut self.popup else { return Ok(()) };
            match key.code {
                KeyCode::Esc => {
                    self.popup = Popup::None;
                    self.mode = Mode::Normal;
                    Ok(())
                }
                KeyCode::Enter => { self.finish_search(); Ok(())}
                KeyCode::Backspace => { buffer.pop(); Ok(())}
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
                    Ok(())
                }
                KeyCode::Char(c) => { buffer.push(c); Ok(())}
                _ => Ok(()),
            }
    }

        /// A notice (op results, attributes, checksums, wc…) can be copied
        /// whole with `y`, so a hash or a path can be lifted out of it.
    fn notice_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::Notice { lines } = &self.popup else { return Ok(()) };
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('c') => {
                    let text = lines.join("\n");
                    if let Some(cb) = self.clipboard.as_mut() {
                        let _ = cb.set_text(text);
                    }
                    self.message = Some("copied".into());
                    self.popup = Popup::None;
                    self.restore_viewer();
                    Ok(())
                }
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => {
                    self.popup = Popup::None;
                    // Back to the file, when the notice was raised over one —
                    // `?` in the viewer being the case that matters.
                    self.restore_viewer();
                    Ok(())
                }
                _ => Ok(()),
            }
    }

    fn encoding_picker_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::EncodingPicker { cursor, .. } = &mut self.popup else { return Ok(()) };
            let n = cian_core::viewer::TextEncoding::ALL.len();
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    *cursor = (*cursor + 1) % n;
                    Ok(())
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    *cursor = (*cursor + n - 1) % n;
                    Ok(())
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    // Cancel: restore a stashed viewer unchanged, else just close.
                    self.finish_encoding_pick(None);
                    Ok(())
                }
                KeyCode::Enter => {
                    let enc = cian_core::viewer::TextEncoding::ALL[*cursor];
                    self.finish_encoding_pick(Some(enc));
                    Ok(())
                }
                _ => Ok(()),
            }
    }

    fn history_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::History { cursor, entries } = &mut self.popup else { return Ok(()) };
            match key.code {
                KeyCode::Esc => { self.popup = Popup::None; Ok(())}
                KeyCode::Enter => { self.finish_history()}
                KeyCode::Char('j') | KeyCode::Down => {
                    if *cursor + 1 < entries.len() { *cursor += 1; }
                    Ok(())
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if *cursor > 0 { *cursor -= 1; }
                    Ok(())
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
                    Ok(())
                }
                _ => Ok(()),
            }
    }

        /// The fuzzy picker (command palette / jump): type to filter, move with
        /// arrows or Ctrl+n/p, Enter runs/jumps, Esc closes.
    fn palette_key(&mut self, key: KeyEvent) -> Result<()> {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Esc => self.popup = Popup::None,
                KeyCode::Enter => self.palette_accept(),
                KeyCode::Down => self.palette_move(1),
                KeyCode::Up => self.palette_move(-1),
                KeyCode::Char('n') if ctrl => self.palette_move(1),
                KeyCode::Char('p') if ctrl => self.palette_move(-1),
                KeyCode::PageDown => self.palette_move(10),
                KeyCode::PageUp => self.palette_move(-10),
                KeyCode::Backspace => {
                    if let Popup::Palette { query, .. } = &mut self.popup {
                        query.pop();
                    }
                    self.palette_refilter();
                }
                KeyCode::Char(c) if !ctrl => {
                    if let Popup::Palette { query, .. } = &mut self.popup {
                        query.push(c);
                    }
                    self.palette_refilter();
                }
                _ => {}
            }
        Ok(())
    }

    fn ai_chat_key(&mut self, key: KeyEvent) -> Result<()> {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            let alt = key.modifiers.contains(KeyModifiers::ALT);
            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
            match key.code {
                KeyCode::Esc => {
                    // A streaming answer: first Esc stops it (and leaves the chat
                    // open); a second Esc then closes. Otherwise close, keeping the
                    // conversation recoverable from the history picker.
                    if !self.cancel_ai_pending() {
                        self.archive_current_ai_chat();
                        self.popup = Popup::None;
                        // Back to the file the question was about, if it was
                        // asked from the viewer.
                        self.restore_viewer();
                    }
                }
                // Ctrl+R opens the conversation history; Ctrl+N starts a fresh
                // one (tucking the current away first).
                KeyCode::Char('r') if ctrl => self.open_ai_history(),
                KeyCode::Char('n') if ctrl => self.new_ai_chat(),
                // Shift+Enter inserts a newline (multi-line questions); plain
                // Enter sends. Alt+Enter and Ctrl+J do the same, as fallbacks for
                // terminals that don't report Shift with Enter (macOS Terminal;
                // iTerm2 needs the kitty protocol, Windows console reports it).
                KeyCode::Enter if shift || alt => {
                    if let Popup::AiChat { input, sel, .. } = &mut self.popup {
                        input.push('\n');
                        *sel = None;
                    }
                }
                KeyCode::Char('j') if ctrl => {
                    if let Popup::AiChat { input, sel, .. } = &mut self.popup {
                        input.push('\n');
                        *sel = None;
                    }
                }
                KeyCode::Enter => self.send_ai_message(),
                // Ctrl+D asks the retriever what it handed the model for this
                // question — the "the answer is wrong, whose fault is it?" key.
                // Not while an answer streams: the report takes the popup, and
                // the rest of the answer would have nowhere to land.
                KeyCode::Char('d') if ctrl => {
                    if self.crmaine_rx.is_some() {
                        self.message =
                            Some(tr(self.lang, "wait for the answer to finish", "回答の完了を待ってください").into());
                    } else {
                        // No argument: the last question `/query` was sent —
                        // which, in a RAG chat, is the one on screen.
                        self.start_debug_search("");
                    }
                }
                // Ctrl+↑ / Ctrl+↓ rate the last RAG answer (thumbs up / down).
                KeyCode::Up if ctrl => self.send_crmaine_feedback(true),
                KeyCode::Down if ctrl => self.send_crmaine_feedback(false),
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
                // Ctrl+V pastes whatever is on the clipboard: a screenshot
                // becomes an attachment, text becomes text. One key, because
                // "paste" is one idea — the user should not have to know which
                // kind of thing they copied.
                KeyCode::Char('v') if ctrl => self.paste_into_chat(),
                // Alt+V is the same paste, for terminals that swallow Ctrl+V
                // to do their own (Windows Terminal binds it by default, and
                // then the key never reaches us).
                KeyCode::Char('v') if alt => self.paste_into_chat(),
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
        Ok(())
    }

    fn ai_history_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::AiHistory { cursor } = &mut self.popup else { return Ok(()) };
            let n = self.ai_history.len();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => {
                    if n > 0 { *cursor = (*cursor + 1).min(n - 1); }
                }
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Char('g') | KeyCode::Home => *cursor = 0,
                KeyCode::Char('G') | KeyCode::End => *cursor = n.saturating_sub(1),
                KeyCode::Enter | KeyCode::Char('l') => {
                    let i = *cursor;
                    self.load_ai_conversation(i);
                }
                KeyCode::Char('d') | KeyCode::Char('x') => {
                    let i = *cursor;
                    self.delete_ai_conversation(i);
                    // Keep the cursor in range after a removal.
                    if let Popup::AiHistory { cursor } = &mut self.popup {
                        let len = self.ai_history.len();
                        *cursor = (*cursor).min(len.saturating_sub(1));
                    }
                }
                _ => {}
            }
        Ok(())
    }

    /// The `:queue` popup: j/k move, x stops/removes (x again abandons a
    /// deaf worker), Esc closes. Rows shift under it as ops finish; the
    /// cursor is clamped at render time by the row count.
    fn op_queue_key(&mut self, key: KeyEvent) -> Result<()> {
        let rows = 1 + self.op_queue.len();
        let Popup::OpQueue { cursor } = &mut self.popup else { return Ok(()) };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
            KeyCode::Char('j') | KeyCode::Down => *cursor = (*cursor + 1).min(rows.saturating_sub(1)),
            KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
            KeyCode::Char('x') | KeyCode::Delete => {
                let row = *cursor;
                self.op_queue_kill(row);
            }
            _ => {}
        }
        Ok(())
    }

    fn toggles_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::Toggles { .. } = &self.popup else { return Ok(()) };
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => self.toggles_move(1),
                KeyCode::Char('k') | KeyCode::Up => self.toggles_move(-1),
                // Enter / Space flip the highlighted switch, keeping the menu up.
                KeyCode::Enter | KeyCode::Char(' ') => self.toggles_apply(),
                _ => {}
            }
        Ok(())
    }

    fn context_menu_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::ContextMenu { items, cursor, .. } = &mut self.popup else { return Ok(()) };
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
        Ok(())
    }

    fn ssh_hosts_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::SshHosts { cursor, filter } = &mut self.popup else { return Ok(()) };
            match key.code {
                // Cancelling the picker abandons any transfer being set up, so
                // a later plain :ssh does not get routed into SFTP.
                KeyCode::Esc => {
                    self.popup = Popup::None;
                    self.scp_dir = None;
                }
                KeyCode::Down => *cursor += 1,
                KeyCode::Up => *cursor = cursor.saturating_sub(1),
                // F2: skip the configured list and type server/user/password by
                // hand (#2). Works for both a plain login and a transfer.
                KeyCode::F(2) => {
                    self.start_manual_ssh();
                    return Ok(());
                }
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
        Ok(())
    }

    fn ssh_users_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::SshUsers { host, cursor } = &mut self.popup else { return Ok(()) };
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
        Ok(())
    }

    fn snippets_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::Snippets { cursor, filter } = &mut self.popup else { return Ok(()) };
            match key.code {
                KeyCode::Esc => self.popup = Popup::None,
                KeyCode::Down => *cursor += 1,
                KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Backspace => { filter.pop(); *cursor = 0; }
                KeyCode::Enter => {
                    let (cursor, filter) = (*cursor, filter.clone());
                    if let Some(i) = self.snippet_matches(&filter).get(cursor).map(|(i, _)| *i) {
                        // send_snippet either opens a confirm (leave it up) or
                        // delivers to the shell (the picker has done its job).
                        self.send_snippet(i);
                        if matches!(self.popup, Popup::Snippets { .. }) {
                            self.popup = Popup::None;
                        }
                    }
                }
                // Typing filters the list.
                KeyCode::Char(c) => { filter.push(c); *cursor = 0; }
                _ => {}
            }
            // Keep the cursor inside the filtered list. Count via `config`
            // (a disjoint field) so it does not clash with the `popup` borrow.
            if let Popup::Snippets { cursor, filter } = &mut self.popup {
                let needle = filter.to_lowercase();
                let n = self.config.snippets.iter().filter(|s| {
                    needle.is_empty()
                        || s.name.to_lowercase().contains(&needle)
                        || s.cmd.to_lowercase().contains(&needle)
                }).count();
                *cursor = (*cursor).min(n.saturating_sub(1));
            }
        Ok(())
    }

    fn remote_browser_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::RemoteBrowser { entries, cursor, purpose, .. } = &mut self.popup else { return Ok(()) };
            let n = entries.len();
            let uploading = *purpose == BrowsePurpose::Upload;
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.popup = Popup::None;
                    self.scp_target = None;
                    self.scp_pending = None;
                    self.remote_ls = None;
                }
                KeyCode::Char('j') | KeyCode::Down => { if n > 0 { *cursor = (*cursor + 1).min(n - 1); } }
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Char('g') | KeyCode::Home => *cursor = 0,
                KeyCode::Char('G') | KeyCode::End => *cursor = n.saturating_sub(1),
                KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => self.remote_browser_enter(),
                // Space marks a file to fetch — only meaningful when downloading.
                KeyCode::Char(' ') if !uploading => self.remote_browser_mark(),
                KeyCode::Char('-') | KeyCode::Backspace | KeyCode::Char('h') | KeyCode::Left => {
                    self.remote_browser_parent()
                }
                // `d` fetches the marked files; `u` drops the pending uploads here.
                KeyCode::Char('d') if !uploading => self.remote_browser_download(),
                KeyCode::Char('u') if uploading => self.remote_browser_upload_here(),
                _ => {}
            }
        Ok(())
    }

    fn local_dest_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::LocalDest { cursor, .. } = &mut self.popup else { return Ok(()) };
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => { self.popup = Popup::None; self.scp_target = None; }
                KeyCode::Char('j') | KeyCode::Down => *cursor = (*cursor + 1).min(3),
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Enter => {
                    let c = *cursor;
                    self.local_dest_pick(c);
                }
                _ => {}
            }
        Ok(())
    }

    fn theme_picker_key(&mut self, key: KeyEvent) -> Result<()> {
            match key.code {
                // Esc / q restore the theme we opened with; Enter keeps the
                // previewed one. j/k (and arrows) move and preview live.
                KeyCode::Esc | KeyCode::Char('q') => self.theme_picker_cancel(),
                KeyCode::Enter | KeyCode::Char('l') => self.theme_picker_commit(),
                KeyCode::Char('j') | KeyCode::Down => self.theme_picker_move(1),
                KeyCode::Char('k') | KeyCode::Up => self.theme_picker_move(-1),
                // Pane-scoped only: drop the override, follow the app theme.
                KeyCode::Char('x') => self.theme_picker_clear_pane(),
                _ => {}
            }
        Ok(())
    }

    fn find_results_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::FindResults { hits, cursor, .. } = &mut self.popup else { return Ok(()) };
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
                // `r` replaces across everything the grep found — the bulk half
                // of the viewer's `:s`. Only for a content grep: a name search
                // matched filenames, and rewriting those is what `R` is for.
                KeyCode::Char('r') => return self.start_grep_replace(),
                // `p` panelizes: load the matches into the pane as a flat listing
                // so they can be marked and bulk-operated on, not just jumped to.
                KeyCode::Char('p') => {
                    let hits = if let Popup::FindResults { hits, .. } = &self.popup {
                        hits.clone()
                    } else {
                        Vec::new()
                    };
                    let label = self
                        .find_job
                        .as_ref()
                        .map(|j| match j.mode {
                            cian_core::search::Mode::Content => format!("grep: {}", j.query),
                            cian_core::search::Mode::Name => format!("find: {}", j.query),
                        })
                        .unwrap_or_else(|| tr(self.lang, "results", "検索結果").to_string());
                    self.popup = Popup::None;
                    self.stop_find();
                    self.panelize_active(label, &hits, false);
                }
                _ => {}
            }
        Ok(())
    }

    /// The grep-replace preview: walk the list, uncheck what the pattern got
    /// wrong, then Enter to write. Nothing has been written yet at this point,
    /// so Esc is always free.
    fn grep_replace_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::GrepReplace(plan) = &mut self.popup else { return Ok(()) };
        let n = plan.changes.len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.popup = Popup::None;
                self.message = Some(tr(self.lang, "replace cancelled — nothing written", "置換を取り消した（書き込みなし）").into());
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    plan.cursor = (plan.cursor + 1).min(n - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => plan.cursor = plan.cursor.saturating_sub(1),
            KeyCode::Char('d') | KeyCode::PageDown => {
                if n > 0 {
                    plan.cursor = (plan.cursor + 10).min(n - 1);
                }
            }
            KeyCode::Char('u') | KeyCode::PageUp => plan.cursor = plan.cursor.saturating_sub(10),
            KeyCode::Char('g') | KeyCode::Home => plan.cursor = 0,
            KeyCode::Char('G') | KeyCode::End => plan.cursor = n.saturating_sub(1),
            KeyCode::Char(' ') => {
                if let Some(c) = plan.changes.get_mut(plan.cursor) {
                    c.picked = !c.picked;
                }
                if n > 0 {
                    plan.cursor = (plan.cursor + 1).min(n - 1);
                }
            }
            // `a` flips the lot: off when everything is on, on otherwise, so
            // one key both clears the list and puts it back.
            KeyCode::Char('a') => {
                let all_on = plan.changes.iter().all(|c| c.picked);
                for c in &mut plan.changes {
                    c.picked = !all_on;
                }
            }
            // `f` unchecks the rest of the file under the cursor — the usual
            // shape of "not this one, it's generated".
            KeyCode::Char('f') => {
                let Some(path) = plan.changes.get(plan.cursor).map(|c| c.path.clone()) else {
                    return Ok(());
                };
                let on = plan.changes.iter().any(|c| c.path == path && c.picked);
                for c in plan.changes.iter_mut().filter(|c| c.path == path) {
                    c.picked = !on;
                }
            }
            KeyCode::Enter => return self.commit_grep_replace(),
            _ => {}
        }
        Ok(())
    }

    fn dest_picker_key(&mut self, key: KeyEvent) -> Result<()> {
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
        Ok(())
    }

        /// The image preview: Esc/q close, Shift+Enter reveals it in the pane,
        /// `E` opens it in the external editor.
    fn image_view_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::ImageView { path, title, .. } = &self.popup else { return Ok(()) };
            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Enter if shift => {
                    let path = path.clone();
                    self.popup = Popup::None;
                    self.reveal_path_in_pane(&path);
                }
                KeyCode::Char('E') => {
                    self.pending_edit = Some(crate::edit::PendingEdit {
                        path: path.clone(),
                        kind: crate::edit::EditKind::File {
                            title: title.clone(),
                            reopen_viewer: false,
                        },
                    });
                    self.popup = Popup::None;
                }
                _ => {}
            }
        Ok(())
    }

    fn dir_compare_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::DirCompare { entries, cursor, scroll, .. } = &mut self.popup else { return Ok(()) };
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
                // c/p copy the list; w saves it to the active pane.
                KeyCode::Char('c') | KeyCode::Char('p') => self.copy_diff(),
                KeyCode::Char('w') => self.start_diff_save_as(),
                // >/< copy the highlighted entry across to the other tree.
                KeyCode::Char('>') | KeyCode::Char('.') => self.dir_compare_copy(true),
                KeyCode::Char('<') | KeyCode::Char(',') => self.dir_compare_copy(false),
                // ]/[ sync the whole tree one way (make the other side match).
                KeyCode::Char(']') => self.dir_compare_sync(true),
                KeyCode::Char('[') => self.dir_compare_sync(false),
                _ => { let _ = scroll; }
            }
        Ok(())
    }

    fn diff_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::Diff { result, folded, fold, scroll, find, find_input, .. } = &mut self.popup else { return Ok(()) };
            // While typing a `/` search, keys build the query.
            if let Some(buf) = find_input {
                match key.code {
                    KeyCode::Esc => { *find_input = None; }
                    KeyCode::Enter => {
                        let q = buf.trim().to_string();
                        *find_input = None;
                        *find = if q.is_empty() { None } else { Some(q) };
                        self.diff_search_jump(true, true);
                    }
                    KeyCode::Backspace => { buf.pop(); }
                    KeyCode::Char(c) => buf.push(c),
                    _ => {}
                }
                return Ok(());
            }
            let rows: &[cian_core::diff::Row] = if *fold { folded } else { &result.rows };
            let last = rows.len().saturating_sub(1);
            let searching = find.is_some();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    // Esc clears an active search first, then closes.
                    if find.is_some() { *find = None; } else { self.popup = Popup::None; }
                }
                KeyCode::Char('j') | KeyCode::Down => *scroll = (*scroll + 1).min(last),
                KeyCode::Char('k') | KeyCode::Up => *scroll = scroll.saturating_sub(1),
                KeyCode::Char('d') | KeyCode::PageDown => *scroll = (*scroll + 20).min(last),
                KeyCode::Char('u') | KeyCode::PageUp => *scroll = scroll.saturating_sub(20),
                KeyCode::Char('g') | KeyCode::Home => *scroll = 0,
                KeyCode::Char('G') | KeyCode::End => *scroll = last,
                // `/` searches the diff text; n/N step matches while a search is
                // active, otherwise they step between differences (the default).
                KeyCode::Char('/') => { *find_input = Some(String::new()); }
                KeyCode::Char('n') if searching => self.diff_search_jump(true, false),
                KeyCode::Char('N') if searching => self.diff_search_jump(false, false),
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
                // c/p copy the diff as text; w saves it; e switches encoding.
                KeyCode::Char('c') | KeyCode::Char('p') => self.copy_diff(),
                KeyCode::Char('w') => self.start_diff_save_as(),
                KeyCode::Char('e') => self.open_diff_encoding_picker(),
                // x asks Carmine to explain what changed and why.
                KeyCode::Char('x') => self.explain_diff(),
                // >/< copy one side's file over the other (confirms).
                KeyCode::Char('>') | KeyCode::Char('.') => self.diff_copy(true),
                KeyCode::Char('<') | KeyCode::Char(',') => self.diff_copy(false),
                _ => {}
            }
        Ok(())
    }

    fn disk_usage_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::DiskUsage { entries, cursor, scroll, .. } = &mut self.popup else { return Ok(()) };
            let n = entries.len();
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => {
                    if n > 0 { *cursor = (*cursor + 1).min(n - 1); }
                }
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Char('d') | KeyCode::PageDown => *cursor = (*cursor + 15).min(n.saturating_sub(1)),
                KeyCode::Char('u') | KeyCode::PageUp => *cursor = cursor.saturating_sub(15),
                KeyCode::Char('g') | KeyCode::Home => *cursor = 0,
                KeyCode::Char('G') | KeyCode::End => *cursor = n.saturating_sub(1),
                // Enter/l/→ drill into a folder; -/←/Backspace climb up.
                KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => self.du_enter(),
                KeyCode::Char('-') | KeyCode::Backspace | KeyCode::Left => self.du_parent(),
                _ => { let _ = scroll; }
            }
        Ok(())
    }

    fn archive_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::Archive { members, cursor, .. } = &mut self.popup else { return Ok(()) };
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
        Ok(())
    }

    fn git_log_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::GitLog { commits, cursor, scroll, dir, vcs, .. } = &mut self.popup else { return Ok(()) };
            let n = commits.len().max(1);
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => *cursor = (*cursor + 1).min(n - 1),
                KeyCode::Char('k') | KeyCode::Up => *cursor = cursor.saturating_sub(1),
                KeyCode::Char('g') | KeyCode::Home => *cursor = 0,
                KeyCode::Char('G') | KeyCode::End => *cursor = n - 1,
                KeyCode::Enter => {
                    let hash = commits.get(*cursor).map(|c| c.hash.clone());
                    let dir = dir.clone();
                    let vcs = *vcs;
                    if let Some(h) = hash {
                        self.git_show_commit(&h, &dir, vcs);
                    }
                }
                _ => {
                    let _ = scroll;
                }
            }
        Ok(())
    }

    fn macros_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::Macros { cursor, names } = &mut self.popup else { return Ok(()) };
            let n = names.len().max(1);
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.popup = Popup::None,
                KeyCode::Char('j') | KeyCode::Down => *cursor = (*cursor + 1) % n,
                KeyCode::Char('k') | KeyCode::Up => *cursor = (*cursor + n - 1) % n,
                KeyCode::Enter => {
                    let idx = *cursor;
                    self.run_macro(idx);
                }
                _ => {}
            }
        Ok(())
    }

    fn sort_picker_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::SortPicker { cursor } = &mut self.popup else { return Ok(()) };
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
        Ok(())
    }

    fn color_picker_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::ColorPicker { pane, cursor } = &mut self.popup else { return Ok(()) };
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
        Ok(())
    }

    fn manual_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::Manual { lines, scroll } = &mut self.popup else { return Ok(()) };
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
        Ok(())
    }

    /// A scrolling report reads like the manual, except Esc puts back whatever
    /// it was raised over (the chat, for a `Ctrl+D` retrieval trace).
    fn report_key(&mut self, key: KeyEvent) -> Result<()> {
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
            if let Popup::Report { back, .. } = &mut self.popup {
                let back = std::mem::replace(&mut **back, Popup::None);
                self.popup = back;
            }
            return Ok(());
        }
        let Popup::Report { lines, scroll, .. } = &mut self.popup else { return Ok(()) };
        let last = lines.len().saturating_sub(1);
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => *scroll = (*scroll + 1).min(last),
            KeyCode::Char('k') | KeyCode::Up => *scroll = scroll.saturating_sub(1),
            KeyCode::Char('d') | KeyCode::PageDown => *scroll = (*scroll + 10).min(last),
            KeyCode::Char('u') | KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
            KeyCode::Char('g') | KeyCode::Home => *scroll = 0,
            KeyCode::Char('G') | KeyCode::End => *scroll = last,
            _ => {}
        }
        Ok(())
    }

    fn shortcuts_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::Shortcuts { cursor, entries, path } = &mut self.popup else { return Ok(()) };
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
        Ok(())
    }

    fn confirm_close_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::ConfirmClose { target } = &self.popup else { return Ok(()) };
            let target = *target;
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.popup = Popup::None;
                    self.execute_close(target);
                }
                KeyCode::Char('n') | KeyCode::Esc => { self.popup = Popup::None; }
                _ => {}
            }
        Ok(())
    }

    fn confirm_elevate_key(&mut self, key: KeyEvent) -> Result<()> {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => self.run_elevated_transfer(),
                KeyCode::Char('n') | KeyCode::Esc => { self.popup = Popup::None; }
                _ => {}
            }
        Ok(())
    }

    fn ai_shell_confirm_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::AiShellConfirm { command } = &self.popup else { return Ok(()) };
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    let cmd = command.clone();
                    self.popup = Popup::None;
                    self.insert_ai_command_at_prompt(&cmd);
                }
                KeyCode::Char('n') | KeyCode::Esc => self.popup = Popup::None,
                _ => {}
            }
        Ok(())
    }

    fn commit_message_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::CommitMessage { buffer, editing, .. } = &mut self.popup else { return Ok(()) };
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
        Ok(())
    }

    fn junk_review_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::JunkReview { items, cursor, .. } = &mut self.popup else { return Ok(()) };
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
        Ok(())
    }

    fn dupe_review_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::DupeReview { items, cursor, .. } = &mut self.popup else { return Ok(()) };
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
        Ok(())
    }

    fn structure_review_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::StructureReview { items, cursor, .. } = &mut self.popup else { return Ok(()) };
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
        Ok(())
    }

    fn rename_review_key(&mut self, key: KeyEvent) -> Result<()> {
        let Popup::RenameReview { items, cursor, .. } = &mut self.popup else { return Ok(()) };
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
        Ok(())
    }

    pub(crate) fn handle_command_key(&mut self, key: KeyEvent) -> Result<()> {
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
    pub(crate) fn insert_into_active_text(&mut self, text: &str) {
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
                if self.shell.is_broadcasting() {
                    self.shell.write_all_panes(text.as_bytes());
                } else if let Some(s) = self.shell.active_session_mut() {
                    s.write_input(text.as_bytes());
                }
            }
            _ => {}
        }
    }

    pub(crate) fn handle_shell_key(&mut self, key: KeyEvent) -> Result<()> {
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
        // Ctrl+Enter opens cian's `:` command line — typing `:` here would just
        // go to the terminal, so this is the shell's keyboard way in (the
        // "Command…" menu item is the fallback where the terminal cannot report
        // the modifier). Passed through to a full-screen app.
        if key.code == KeyCode::Enter
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::SHIFT)
            && !alt_screen
        {
            self.enter_command_mode();
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
        // Everything else is forwarded to the shell — to every pane in the tab
        // when broadcast/synchronize is on, otherwise just the active one.
        if let Some(bytes) = encode_key(key, app_cursor) {
            if self.shell.is_broadcasting() {
                self.shell.write_all_panes(&bytes);
            } else if let Some(s) = self.shell.active_session_mut() {
                s.write_input(&bytes);
            }
        }
        Ok(())
    }

    pub(crate) fn handle_visual_key(&mut self, mut key: KeyEvent) -> Result<()> {
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
    pub(crate) fn visual_select_all(&mut self) {
        let last = match self.active_pane() {
            Some(p) if !p.entries.is_empty() => p.entries.len() - 1,
            _ => return,
        };
        self.visual_anchor = Some(0);
        if let Some(p) = self.active_pane_mut() {
            p.cursor = last;
        }
    }

    pub(crate) fn handle_normal_key(&mut self, mut key: KeyEvent) -> Result<()> {
        // Full-width input (全角英数) → ASCII so commands work without leaving
        // the Japanese IME. Kana can be bound per-key via init.lua.
        normalize_jp_key(&mut key);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        // `gg` chord → jump to top
        if self.pending_g {
            self.pending_g = false;
            if matches!(key.code, KeyCode::Char('g')) && !ctrl {
                if let Some(p) = self.active_pane_mut() { p.cursor = 0; }
                return Ok(());
            }
            // anything else: fall through to normal handling
        }

        // User keymap overrides. Only keys the user explicitly bound appear
        // here, so default behaviour is untouched for everything else. Shift
        // is not part of the lookup — the uppercase character already carries
        // it, and terminals disagree about whether to report both.
        if let KeyCode::Char(c) = key.code {
            let mods = key.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT);
            if let Some(action) = self.keymap.get(&(c, mods)).copied() {
                return self.execute_action(action);
            }
        }

        // A remote (SFTP) pane drives navigation over the network and does not
        // (yet) do local file operations. Handle its keys before the local ones.
        if self.active_pane().map(|p| p.is_remote()).unwrap_or(false) {
            match key.code {
                // Match the local panes exactly: Enter / `l` go into a folder,
                // `-` / Backspace go up. The ←/→ arrows are NOT navigation here —
                // they switch panes, so they fall through to the shared handler
                // below (using them to go up/into was reported as confusing).
                KeyCode::Enter | KeyCode::Char('l') => {
                    self.remote_pane_enter();
                    return Ok(());
                }
                KeyCode::Char('-') | KeyCode::Backspace => {
                    self.remote_pane_parent();
                    return Ok(());
                }
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.leave_remote_pane();
                    return Ok(());
                }
                // `c` copies across the boundary, `m` moves (copy then delete the
                // source, after a confirm).
                KeyCode::Char('c') if !ctrl => {
                    self.try_remote_pane_transfer(false);
                    return Ok(());
                }
                KeyCode::Char('m') if !ctrl => {
                    self.try_remote_pane_transfer(true);
                    return Ok(());
                }
                // Write ops on the server, same keys as the local pane:
                // A = new folder, a = new file, r = rename, d = delete.
                KeyCode::Char('A') if !ctrl => {
                    self.remote_pane_mkdir();
                    return Ok(());
                }
                KeyCode::Char('a') if !ctrl => {
                    self.remote_pane_touch();
                    return Ok(());
                }
                KeyCode::Char('r') if !ctrl => {
                    self.remote_pane_rename();
                    return Ok(());
                }
                KeyCode::Char('d') if !ctrl => {
                    self.remote_pane_delete();
                    return Ok(());
                }
                // Not wired for remote yet: checksum, branch, compare.
                KeyCode::Char('x' | 'b' | '=') if !ctrl => {
                    self.message = Some(tr(
                        self.lang,
                        "remote pane: not available (c copy, m move, A/a/r/d write)",
                        "リモートペイン: 未対応（c コピー, m 移動, A/a/r/d 書込）",
                    ).into());
                    return Ok(());
                }
                _ => {} // j/k, marks, filter, sort still work on the fetched list
            }
        }

        match (ctrl, shift, key.code) {
            // The browser's arrows over this pane's history. First in the
            // match on purpose: the unmodified `h`, `l`, ← and → arms below
            // would otherwise claim these before the Alt guard is reached.
            (_, _, KeyCode::Left) if alt => self.pane_go_back(),
            (_, _, KeyCode::Right) if alt => self.pane_go_forward(),
            (_, _, KeyCode::Char('h')) if alt => self.pane_go_back(),
            (_, _, KeyCode::Char('l')) if alt => self.pane_go_forward(),
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
                // In a branch / search listing, Esc is the way out of the view
                // first; only once back in a normal directory does it clear
                // marks and the filter.
                if self.active_pane().map(|p| p.is_flat()).unwrap_or(false) {
                    if let Some(p) = self.active_pane_mut() {
                        let _ = p.leave_flat();
                    }
                } else if let Some(p) = self.active_pane_mut() {
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
            // Ctrl+A marks the lot. Only where the terminal hands Ctrl over —
            // bind `mark_all` to something else in keymap.lua where it does
            // not, or use `:markall`.
            (true, _, KeyCode::Char('a')) => self.mark_all(),
            (true, _, KeyCode::Char('f')) => self.start_grep_prompt(),
            // `C` = command palette (mnemonic: Commands), `Z` = fuzzy-jump to a
            // recent dir (complements `z`, jump-to-typed-path). Letter keys, not
            // Ctrl (macOS terminals steal Ctrl+P/O) nor `;` (too easily confused
            // with `:` command mode on a US keyboard). The char already encodes
            // the shift, so `_` matches whether or not the modifier is reported.
            (false, _, KeyCode::Char('C')) => self.start_command_palette(),
            (false, _, KeyCode::Char('Z')) => self.start_fuzzy_jump(),
            // `T` = the UI-toggles menu (dotfiles, input sync, notifications…).
            (false, _, KeyCode::Char('T')) => self.start_toggles(),
            (false, _, KeyCode::Char(',')) => self.start_sort_picker(),
            (false, false, KeyCode::Char('z')) => self.start_jump_path(),
            // `b` flattens the subtree into this pane (branch view); again to leave.
            (false, false, KeyCode::Char('b')) => self.toggle_branch_view(),
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
            // `@` — vim's play-a-macro key — opens the macro launcher.
            (false, _, KeyCode::Char('@')) => self.start_macros(),
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
            // `c` copies the selection to the other pane; `m` moves it. `y`
            // pastes (an alias for Ctrl+V), so the vim-style transfer keys and
            // the Windows-style clipboard keys coexist.
            (false, false, KeyCode::Char('c')) => self.start_transfer(PendingOp::Copy),
            (false, false, KeyCode::Char('m')) => self.start_transfer(PendingOp::Move),
            (false, false, KeyCode::Char('y')) => { let _ = self.paste_clip(); }
            // Windows-style file clipboard (file panes only — in the shell these
            // keys keep their terminal meaning: Ctrl+C is interrupt, Ctrl+V is
            // handled by the terminal emulator).
            (true, false, KeyCode::Char('c')) => self.clip_targets(ClipOp::Copy),
            (true, false, KeyCode::Char('x')) => self.clip_targets(ClipOp::Cut),
            (true, false, KeyCode::Char('v')) => { let _ = self.paste_clip(); }
            (false, false, KeyCode::Char('d')) => self.start_delete(),
            // Undo the last rename / create / move (also `:undo`).
            (false, false, KeyCode::Char('u')) => self.undo_last(),
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
            (_, _, KeyCode::Backspace) => {
                if self.active_pane().map(|p| p.archive_view().is_some()).unwrap_or(false) {
                    self.archive_go_up();
                } else if self.active_pane().map(|p| p.is_flat()).unwrap_or(false) {
                    // A search listing has no parent to climb to: "up" from a
                    // set of results means back to the folder they came from,
                    // which is what Esc does. Wandering off to a parent
                    // directory instead is a surprise nobody asked for.
                    if let Some(p) = self.active_pane_mut() {
                        let _ = p.leave_flat();
                    }
                } else if let Some(p) = self.active_pane_mut() { p.go_parent()?; }
            }
            (_, _, KeyCode::Left) => self.focus(FocusedPane::Left),
            (_, _, KeyCode::Right) => self.focus(FocusedPane::Right),
            // `l` is deliberately unbound. Entering a folder is Enter; the
            // vim-shaped `l` kept firing when a hand rested on the home row.
            // Ctrl+Enter hands the file to its own program — the thing Enter
            // used to do, moved off the key one presses by reflex. On a
            // directory it keeps its old job of opening that folder in the
            // other pane, which is not something a launcher could mean.
            // (Ctrl+Shift+Enter is the global snippet launcher.) Needs a
            // terminal that reports the modifier with Enter; `x` does the same
            // where one does not.
            (true, false, KeyCode::Enter) => {
                let on_dir = self
                    .active_pane()
                    .and_then(|p| p.selected())
                    .map(|e| e.is_dir)
                    .unwrap_or(true);
                if on_dir {
                    self.open_in_other_pane(false)?;
                } else {
                    self.open_externally();
                }
            }
            // `o` pulls the other pane's directory into this one; `O` pushes
            // this pane's directory onto the other. (Open-into-other-pane lives
            // on Ctrl+Enter above.)
            (false, false, KeyCode::Char('o')) => { self.sync_active_from_other()?; }
            (false, true, KeyCode::Char('O')) => { self.sync_other_from_active()?; }
            // Enter and a double-click must be the same action, so both go
            // through activate_selected — which also knows about archives
            // (browse in, not OS-open) and archive members. The keyboard used
            // to have its own copy of the logic here, which is exactly how
            // Enter on a zip kept opening Finder after the mouse learned
            // better.
            (false, _, KeyCode::Enter) => self.activate_selected()?,
            _ => {}
        }
        Ok(())
    }

    /// Run a remappable action (dispatched from a user keymap override). The
    /// bodies mirror the default key handlers exactly so behaviour is identical
    /// whether triggered by a default key or a user-bound one.
    pub(crate) fn execute_action(&mut self, action: Action) -> Result<()> {
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
                if self.active_pane().map(|p| p.archive_view().is_some()).unwrap_or(false) {
                    self.archive_go_up();
                } else if let Some(p) = self.active_pane_mut() { p.go_parent()?; }
            }
            Action::EnterDir => {
                if self.active_pane().map(|p| p.archive_view().is_some()).unwrap_or(false) {
                    // Inside an archive `l` behaves like Enter on directories.
                    let on_dir = self
                        .active_pane()
                        .and_then(|p| p.selected())
                        .map(|e| e.is_dir)
                        .unwrap_or(false);
                    if on_dir {
                        self.archive_activate();
                    }
                } else if let Some(p) = self.active_pane_mut() {
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
            Action::Paste => { let _ = self.paste_clip(); }
            Action::Cut => self.clip_targets(ClipOp::Cut),
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
            Action::MarkAll => self.mark_all(),
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
