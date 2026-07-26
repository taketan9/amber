//! Actions on the App that don't belong to a bigger feature module: transfers,
//! delete/rename/create, shortcuts (bookmarks), filter, sort, destination
//! picker, opening the viewer/diff/archive/attributes/hash, recursive
//! find/grep, jump-to-path, the manual, config reload, and the worker-backed
//! op job (progress, elevation retry, external-change polling). Split out of
//! lib.rs as an `impl App` block.
use super::*;

impl App {
    /// Send the clipboard's text to the shell, as typing it would. Raw, with
    /// newlines: pasting a command line into a shell is meant to run it, and
    /// this does not know whether the child enabled bracketed paste, so it
    /// adds no wrapper (a stray `\x1b[200~` would otherwise print as garbage).
    pub(crate) fn paste_text_to_shell(&mut self) {
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

    pub(crate) fn push_clipboard(&mut self, paths: &[PathBuf]) {
        if !self.clipboard_on_copy { return; }
        let Some(cb) = self.clipboard.as_mut() else { return; };
        let text = paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n");
        let _ = cb.set_text(text);
    }

    // ------- Visual mode -------
    pub(crate) fn visual_start(&mut self) {
        if let Some(p) = self.active_pane() {
            self.visual_anchor = Some(p.cursor);
            self.mode = Mode::Visual;
        }
    }
    pub(crate) fn visual_commit(&mut self) {
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
    pub(crate) fn visual_cancel_and_clear_all(&mut self) {
        self.visual_anchor = None;
        if let Some(p) = self.active_pane_mut() { p.clear_marks(); }
        self.mode = Mode::Normal;
    }

    // ------- Confirmation flows -------
    pub(crate) fn start_transfer(&mut self, op: PendingOp) {
        let Some(dest) = self.opposite_pane_cwd() else { return };
        let targets = match self.active_pane() {
            Some(p) => p.target_paths(),
            None => return,
        };
        if targets.is_empty() { self.message = Some("nothing to operate on".into()); return; }
        self.popup = Popup::ConfirmTransfer { op, targets, dest };
    }
    pub(crate) fn start_delete(&mut self) {
        let targets = match self.active_pane() {
            Some(p) => p.target_paths(),
            None => return,
        };
        if targets.is_empty() { self.message = Some("nothing to delete".into()); return; }
        self.popup = Popup::ConfirmDelete { targets };
    }
    pub(crate) fn start_rename(&mut self) {
        let Some(p) = self.active_pane() else { return };
        let Some(e) = p.selected() else { return };
        self.popup = text_input(
                "rename",
                "new name:",
                e.name.clone(),
                InputKind::Rename { original: e.path.clone() },
            );
    }
    pub(crate) fn start_new_file(&mut self) {
        let Some(p) = self.active_pane() else { return };
        self.popup = text_input(
                "new file",
                "name:",
                String::new(),
                InputKind::NewFile { parent: p.cwd.clone() },
            );
    }
    pub(crate) fn start_new_dir(&mut self) {
        let Some(p) = self.active_pane() else { return };
        self.popup = text_input(
                "new directory",
                "name:",
                String::new(),
                InputKind::NewDir { parent: p.cwd.clone() },
            );
    }

    // ------- Search -------
    pub(crate) fn start_search(&mut self) {
        self.popup = Popup::Search { buffer: String::new() };
        self.mode = Mode::Search;
    }

    pub(crate) fn finish_search(&mut self) {
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
    pub(crate) fn start_shortcuts(&mut self) {
        self.popup = Popup::Shortcuts {
            entries: self.shortcuts.entries.clone(),
            cursor: 0,
            path: Vec::new(),
        };
    }

    /// Re-open the shortcuts popup at `path`/`cursor` from the saved store (used
    /// after an add/edit/delete so the view reflects the change).
    pub(crate) fn reopen_shortcuts(&mut self, path: Vec<usize>, cursor: usize) {
        let n = sc_level(&self.shortcuts.entries, &path).len();
        self.popup = Popup::Shortcuts {
            entries: self.shortcuts.entries.clone(),
            cursor: cursor.min(n.saturating_sub(1)),
            path,
        };
    }

    /// Prompt for a new shortcut's name in the group at `path`. `group` makes a
    /// folder (name only, no target step).
    pub(crate) fn start_shortcut_add(&mut self, path: Vec<usize>, group: bool) {
        let title = if group { "new folder — name" } else { "new shortcut — name" };
        self.popup = text_input(
            title,
            "name:",
            String::new(),
            InputKind::ShortcutName { path, edit_idx: None, group },
        );
    }

    pub(crate) fn start_shortcut_edit(&mut self, path: Vec<usize>, idx: usize) {
        let Some(s) = sc_level(&self.shortcuts.entries, &path).get(idx).cloned() else { return };
        let group = s.is_group();
        self.popup = text_input(
            "edit shortcut — name",
            "name:",
            s.name,
            InputKind::ShortcutName { path, edit_idx: Some(idx), group },
        );
    }

    pub(crate) fn copy_paths_to_clipboard(&mut self) {
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

    pub(crate) fn copy_file_refs_to_clipboard(&mut self) {
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

    pub(crate) fn copy_shortcut_target_to_clipboard(&mut self, path: &[usize], idx: usize) {
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

    pub(crate) fn execute_shortcut(&mut self, path: &[usize], idx: usize) -> Result<()> {
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
    pub(crate) fn start_history(&mut self) {
        let entries = self.active_pane().map(|p| p.history.clone()).unwrap_or_default();
        if entries.is_empty() {
            self.message = Some("no history yet".into());
            return;
        }
        self.popup = Popup::History { entries, cursor: 0 };
    }

    pub(crate) fn finish_history(&mut self) -> Result<()> {
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
    pub(crate) fn start_filter(&mut self) {
        self.filter_buffer = self.active_pane().map(|p| p.filter.clone()).unwrap_or_default();
        self.mode = Mode::Filter;
    }

    /// Push the buffer into the pane, narrowing the listing as the user types.
    pub(crate) fn apply_filter_buffer(&mut self) {
        let buf = self.filter_buffer.clone();
        if let Some(p) = self.active_pane_mut() {
            p.set_filter(buf);
        }
    }

    pub(crate) fn handle_filter_key(&mut self, key: KeyEvent) -> Result<()> {
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

    /// Type an AI-suggested command at the shell prompt WITHOUT running it —
    /// the user reviews it and presses Enter. Focuses the shell.
    pub(crate) fn insert_ai_command_at_prompt(&mut self, cmd: &str) {
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
    pub(crate) fn run_in_shell(&mut self, mut cmd: String) {
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
    pub(crate) fn flush_pending_shell_input(&mut self) {
        let Some(cmd) = self.pending_shell_input.take() else { return };
        match self.shell.active_session_mut() {
            Some(s) => s.write_input(cmd.as_bytes()),
            // Not ready yet — put it back and try again next tick.
            None => self.pending_shell_input = Some(cmd),
        }
    }

    // ------- Sorting -------
    pub(crate) fn start_sort_picker(&mut self) {
        // Open on the pane's current key, so the picker shows where you are.
        let cur = self
            .active_pane()
            .and_then(|p| SortKey::ALL.iter().position(|k| *k == p.sort.key))
            .unwrap_or(0);
        self.popup = Popup::SortPicker { cursor: cur };
    }

    /// Apply a sort key. Choosing the key that is already active flips the
    /// direction, which is how column headers behave everywhere else.
    pub(crate) fn apply_sort_key(&mut self, key: SortKey) {
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
    pub(crate) fn remember_dest(&mut self, dest: &Path) {
        self.dest_history.retain(|p| p != dest);
        self.dest_history.insert(0, dest.to_path_buf());
        self.dest_history.truncate(DEST_HISTORY_CAP);
    }

    /// Offer somewhere other than the opposite pane to send the selection.
    pub(crate) fn start_dest_picker(&mut self, op: PendingOp) {
        let targets = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if targets.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        self.popup = Popup::DestPicker { op, targets, cursor: 0 };
    }

    /// Rows of the destination picker: the opposite pane first, then history.
    pub(crate) fn dest_choices(&self) -> Vec<(String, PathBuf)> {
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
    pub(crate) fn look_inside(&mut self) {
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
    pub(crate) fn open_viewer_at(&mut self, path: &Path, title: &str, line0: usize) {
        // Office/PDF documents are extracted to text first (fully in-process, no
        // external converter), then shown in the ordinary viewer so search,
        // selection and copy all work over them.
        if cian_core::office::classify(path).is_some() {
            self.open_document_viewer(path, title, line0);
            return;
        }
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
                // Markdown files open in rendered preview; `p` toggles the source.
                let markdown = matches!(
                    path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(),
                    Some("md") | Some("markdown") | Some("mkd") | Some("mdown")
                );
                let source = view.lines.clone();
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
                    markdown,
                    preview: markdown,
                    source,
                    md_styles: Vec::new(),
                    md_width: 0,
                }
            }
            Err(e) => self.message = Some(format!("cannot view: {}", e)),
        }
    }

    /// Open an Office/PDF document as extracted text in the viewer.
    pub(crate) fn open_document_viewer(&mut self, path: &Path, title: &str, line0: usize) {
        let total_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        match cian_core::office::extract(path) {
            Ok((doc, mut lines)) => {
                // A one-line header naming the format, and — for the legacy
                // binary formats — an honest note that the text is approximate.
                let mut header = vec![format!("── {} ──", doc.label())];
                if doc.is_best_effort() {
                    header.push(tr(
                        self.lang,
                        "(legacy binary — best-effort text; re-save as the modern format for a faithful view)",
                        "(旧バイナリ形式 — テキスト抽出は簡易です。正確な表示には新形式で保存し直してください)",
                    ).to_string());
                }
                header.push(String::new());
                lines.splice(0..0, header);

                let text = lines.join("\n");
                let view = cian_core::viewer::View::from_text(text, total_bytes, false);
                let last = view.lines.len().saturating_sub(1);
                let line = line0.min(last);
                let source = view.lines.clone();
                self.popup = Popup::Viewer {
                    title: format!("{}  ·  {}", title, doc.label()),
                    path: path.to_path_buf(),
                    view,
                    scroll: line.saturating_sub(4),
                    line,
                    col: 0,
                    goal: 0,
                    visual: None,
                    anchor: (0, 0),
                    find_input: None,
                    find_query: None,
                    count: None,
                    git_lines: std::collections::HashMap::new(),
                    markdown: false,
                    preview: false,
                    source,
                    md_styles: Vec::new(),
                    md_width: 0,
                };
            }
            Err(e) => self.message = Some(format!("cannot read document: {}", e)),
        }
    }

    /// Compare the file under the left pane's cursor with the right pane's.
    ///
    /// Deliberately not "the focused pane against the other one": the whole
    /// gesture is to put A on the left and B on the right, and which pane the
    /// cursor happens to be in at the moment of pressing the key should not
    /// silently swap the two sides of the result.
    pub(crate) fn open_diff(&mut self) {
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
                // Identical files get a clear notice rather than a diff of
                // nothing — the same feedback the folder compare now gives.
                if !result.rows.iter().any(|r| r.is_difference()) {
                    self.popup = Popup::Notice {
                        lines: vec![
                            tr(self.lang, "The two files are identical.", "2つのファイルは同一です。").to_string(),
                            String::new(),
                            format!("{}  ↔  {}", a.name, b.name),
                        ],
                    };
                    return;
                }
                let folded = cian_core::diff::fold(&result.rows, cian_core::diff::CONTEXT);
                self.popup = Popup::Diff {
                    left: a.name.clone(),
                    right: b.name.clone(),
                    left_path: a.path.clone(),
                    right_path: b.path.clone(),
                    encoding: cian_core::viewer::TextEncoding::Utf8,
                    result,
                    folded,
                    // Folded to begin with: the differences are what was asked
                    // for, and on two near-identical files the unfolded view
                    // opens on a screen of agreement.
                    fold: true,
                    scroll: 0,
                    find: None,
                    find_input: None,
                };
            }
            Err(e) => self.message = Some(format!("cannot compare: {}", e)),
        }
    }

    /// Compare two directory trees on a worker thread, showing the differing
    /// paths when it finishes. Esc cancels a long walk.
    pub(crate) fn start_dir_compare(&mut self, left: PathBuf, right: PathBuf, ln: String, rn: String) {
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
    pub(crate) fn poll_diff_job(&mut self) -> bool {
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
            // A clear notice, not just a status line — the compare felt
            // unresponsive when identical folders only whispered a message.
            self.popup = Popup::Notice {
                lines: vec![
                    tr(self.lang, "The two folders are identical.", "2つのフォルダは同一です。").to_string(),
                    String::new(),
                    format!("{}  ↔  {}", job.left, job.right),
                ],
            };
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
    pub(crate) fn dir_compare_goto(&mut self) {
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

    /// The open diff (file or folder) rendered as plain text, for copy/save.
    fn diff_as_text(&self) -> Option<String> {
        use cian_core::diff::Row;
        match &self.popup {
            Popup::Diff { left, right, result, .. } => {
                let mut out = format!("--- {}\n+++ {}\n", left, right);
                if result.binary {
                    out.push_str(if result.identical {
                        "(binary files, identical)\n"
                    } else {
                        "(binary files differ)\n"
                    });
                    return Some(out);
                }
                for r in &result.rows {
                    match r {
                        Row::Same { left: l, .. } => out.push_str(&format!("  {}\n", l.text)),
                        Row::Removed { left: l } => out.push_str(&format!("- {}\n", l.text)),
                        Row::Added { right: rr } => out.push_str(&format!("+ {}\n", rr.text)),
                        Row::Changed { left: l, right: rr } => {
                            out.push_str(&format!("- {}\n+ {}\n", l.text, rr.text));
                        }
                        Row::Skipped { lines } => out.push_str(&format!("  … {} identical lines\n", lines)),
                    }
                }
                Some(out)
            }
            Popup::DirCompare { left, right, entries, .. } => {
                use cian_core::dirdiff::Status;
                let mut out = format!("# compare  {}  ↔  {}\n", left, right);
                for e in entries {
                    let mark = match e.status {
                        Status::OnlyLeft => "-",
                        Status::OnlyRight => "+",
                        Status::Differ => "~",
                    };
                    out.push_str(&format!("{} {}\n", mark, e.rel.display()));
                }
                Some(out)
            }
            _ => None,
        }
    }

    /// Move the diff view to the next/previous row whose text matches the
    /// active search (case-insensitive). `from_here` includes the current row
    /// (used right after confirming a search).
    pub(crate) fn diff_search_jump(&mut self, forward: bool, from_here: bool) {
        use cian_core::diff::Row;
        let Popup::Diff { result, folded, fold, scroll, find, .. } = &mut self.popup else { return };
        let Some(q) = find.as_ref().map(|s| s.to_lowercase()) else { return };
        let rows: &[Row] = if *fold { folded } else { &result.rows };
        let hit = |r: &Row| -> bool {
            let txt = |o: Option<&cian_core::diff::Line>| o.map(|l| l.text.to_lowercase()).unwrap_or_default();
            match r {
                Row::Same { left, right } => txt(Some(left)).contains(&q) || txt(Some(right)).contains(&q),
                Row::Changed { left, right } => txt(Some(left)).contains(&q) || txt(Some(right)).contains(&q),
                Row::Removed { left } => txt(Some(left)).contains(&q),
                Row::Added { right } => txt(Some(right)).contains(&q),
                Row::Skipped { .. } => false,
            }
        };
        let n = rows.len();
        if n == 0 {
            return;
        }
        let found = if forward {
            let start = if from_here { *scroll } else { *scroll + 1 };
            (start..n).find(|&i| hit(&rows[i]))
        } else {
            (0..*scroll).rev().find(|&i| hit(&rows[i]))
        };
        match found {
            Some(i) => *scroll = i,
            None => self.message = Some(tr(self.lang, "no more matches", "一致なし").into()),
        }
    }

    /// Copy the diff/compare result to the clipboard.
    pub(crate) fn copy_diff(&mut self) {
        let Some(text) = self.diff_as_text() else { return };
        match self.clipboard.as_mut() {
            Some(cb) => {
                let _ = cb.set_text(text);
                self.message = Some("◂ diff copied".into());
            }
            None => self.message = Some("clipboard unavailable".into()),
        }
    }

    /// Prompt for a filename and save the diff/compare result into the active
    /// pane's directory.
    pub(crate) fn start_diff_save_as(&mut self) {
        let Some(text) = self.diff_as_text() else { return };
        self.popup = text_input(
            "save diff as",
            "filename (in the active pane's directory):",
            "diff.txt".to_string(),
            InputKind::DiffSaveAs { text },
        );
    }

    /// Stash the current file diff and open the encoding picker for it.
    pub(crate) fn open_diff_encoding_picker(&mut self) {
        if !matches!(self.popup, Popup::Diff { .. }) {
            return;
        }
        let cur = if let Popup::Diff { encoding, .. } = &self.popup {
            cian_core::viewer::TextEncoding::ALL.iter().position(|e| e == encoding).unwrap_or(0)
        } else {
            0
        };
        let diff = std::mem::replace(&mut self.popup, Popup::None);
        self.popup = Popup::EncodingPicker { cursor: cur, target: EncTarget::Diff(Box::new(diff)) };
    }

    /// Pull members out of the open archive into the opposite pane.
    pub(crate) fn extract_from_archive(&mut self, all: bool) {
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

    /// `:unzip` / `:extract` (and the right-click menu): extract the archive
    /// under the cursor into a fresh sub-folder of the active pane, named after
    /// the archive. Works for zip and tar/tar.gz.
    pub(crate) fn extract_selected(&mut self) {
        let Some(p) = self.active_pane() else { return };
        let Some(e) = p.selected().filter(|e| !e.is_parent) else {
            self.message = Some(tr(self.lang, "select an archive to extract", "解凍する書庫を選択してください").into());
            return;
        };
        if e.is_dir || !cian_core::archive::is_archive(&e.path) {
            self.message = Some(format!("{}: {}", tr(self.lang, "not an archive", "書庫ではありません"), e.name));
            return;
        }
        let archive = e.path.clone();
        let dest = unique_dir(&p.cwd, &archive_stem(&e.name));
        self.start_op("extracting", move |ctl| {
            let _ = std::fs::create_dir_all(&dest);
            cian_core::archive::extract(&archive, &[], &dest, ctl)
        });
    }

    // ------- Hidden files, attributes, checksums -------
    pub(crate) fn toggle_hidden(&mut self) {
        let Some(p) = self.active_pane_mut() else { return };
        let show = !p.show_hidden;
        p.set_show_hidden(show);
        self.message =
            Some(if show { "showing dotfiles".into() } else { "hiding dotfiles".to_string() });
    }

    pub(crate) fn show_attributes(&mut self) {
        let paths = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        self.popup = Popup::Notice { lines: self.attributes_lines(&paths, 40) };
    }

    /// Build the Attributes listing (permissions, size, owner) for `paths`,
    /// capped at `limit` rows. Shared by the Attributes menu/`:attr` and `:ls`.
    pub(crate) fn attributes_lines(&self, paths: &[PathBuf], limit: usize) -> Vec<String> {
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
    pub(crate) fn start_hash(&mut self, kind: cian_core::attrs::HashKind) {
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

    pub(crate) fn set_attr_command(&mut self, arg: &str) {
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

    pub(crate) fn set_readonly_command(&mut self, on: bool) {
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
    pub(crate) fn start_find_prompt(&mut self) {
        self.popup = text_input(
                "find (recursive)",
                "name contains   (Ctrl+V paste, Ctrl+U clear):",
                String::new(),
                InputKind::FindRecursive,
            );
    }

    pub(crate) fn start_grep_prompt(&mut self) {
        self.popup = text_input(
                "grep (recursive)",
                "text inside files   (Ctrl+V paste, Ctrl+U clear):",
                String::new(),
                InputKind::GrepRecursive,
            );
    }

    /// Walk the tree below the focused pane on a worker thread.
    pub(crate) fn start_find(&mut self, needle: &str, mode: cian_core::search::Mode) {
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
    pub(crate) fn poll_find_job(&mut self) -> bool {
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
    pub(crate) fn open_find_hit(&mut self) -> Result<()> {
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

    pub(crate) fn stop_find(&mut self) {
        if let Some(job) = self.find_job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    // ------- Jump to a typed path -------
    pub(crate) fn start_jump_path(&mut self) {
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
    pub(crate) fn finish_jump_path(&mut self, raw: &str) -> Result<()> {
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
    pub(crate) fn open_menu_at_cursor(&mut self) {
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
    pub(crate) fn open_manual(&mut self) {
        self.popup = Popup::Manual { lines: manual_lines(&self.keymap, self.lang), scroll: 0 };
    }

    // ------- AI -------

    /// Re-read `init.lua` and apply everything that can change without a
    /// restart: keymaps, options, SSH hosts and open handlers. The colour theme
    /// and border style are installed once at startup (into set-once globals),
    /// so a change to those is reported as needing a restart rather than being
    /// silently ignored.
    pub(crate) fn reload_config(&mut self) {
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

        // Re-read macro.lua, count.lua and shortcuts too, so `:reload` picks them up.
        let (macros, macro_error) = crate::macro_run::load_macros();
        self.macros = macros;
        self.macro_error = macro_error;
        self.count_opts = crate::count::load_count_opts();
        self.shortcuts = ShortcutStore::load_or_default();

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
    pub(crate) fn start_quit_confirm(&mut self) {
        self.popup = Popup::ConfirmQuit;
    }

    /// Perform a confirmed close (shell split pane or file tab).
    pub(crate) fn execute_close(&mut self, target: CloseTarget) {
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

    pub(crate) fn jump_to_next_match(&mut self, forward: bool) {
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
    pub(crate) fn start_op<F>(&mut self, label: &'static str, work: F)
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
    pub(crate) fn poll_op_job(&mut self) -> bool {
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
    pub(crate) fn poll_external_changes(&mut self) -> bool {
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

    pub(crate) fn cancel_op_job(&mut self) {
        if let Some(job) = &self.op_job {
            job.cancel.store(true, Ordering::Relaxed);
            self.message = Some("stopping…".into());
        }
    }

    pub(crate) fn finish_transfer(&mut self, conflict: Conflict) -> Result<()> {
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
    pub(crate) fn run_elevated_transfer(&mut self) {
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

    pub(crate) fn finish_delete(&mut self, mode: DeleteMode) -> Result<()> {
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

    pub(crate) fn show_op_report(&mut self, report: &OpReport) {
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

    pub(crate) fn finish_text_input(&mut self) -> Result<()> {
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
            InputKind::DiffSaveAs { text } => {
                let dir = self.active_pane().map(|p| p.cwd.clone());
                if let Some(dir) = dir {
                    let path = dir.join(&name);
                    match std::fs::write(&path, text) {
                        Ok(()) => {
                            self.message = Some(format!("saved diff → {}", path.display()));
                            self.reload_active();
                        }
                        Err(e) => self.message = Some(format!("save failed: {}", e)),
                    }
                }
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
            InputKind::CompressName { kind, sources } => {
                if name.is_empty() {
                    self.message = Some("compress cancelled".into());
                    return Ok(());
                }
                let Some(cwd) = self.active_pane().map(|p| p.cwd.clone()) else { return Ok(()) };
                let (ext, gz) = match kind {
                    CompressKind::Zip => (".zip", None),
                    CompressKind::TarGz => (".tar.gz", Some(true)),
                };
                let mut fname = name.clone();
                let low = fname.to_lowercase();
                let has_ext = match kind {
                    CompressKind::TarGz => low.ends_with(".tar.gz") || low.ends_with(".tgz"),
                    _ => low.ends_with(ext),
                };
                if !has_ext {
                    fname.push_str(ext);
                }
                let dest = cwd.join(&fname);
                if dest.exists() {
                    self.message = Some(format!("already exists: {}", fname));
                    return Ok(());
                }
                match gz {
                    None => self.start_zip(dest, sources.clone(), None),
                    Some(g) => self.start_tar(dest, sources.clone(), g),
                }
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
}

/// The base name of an archive without its extension, handling the two-part
/// `.tar.gz` / `.tar.bz2` / `.tar.xz` / `.tgz` cases: `proj.tar.gz` → `proj`.
fn archive_stem(name: &str) -> String {
    let low = name.to_lowercase();
    for suf in [".tar.gz", ".tar.bz2", ".tar.xz", ".tgz"] {
        if low.ends_with(suf) {
            return name[..name.len() - suf.len()].to_string();
        }
    }
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
}

/// A directory path under `parent` named `stem`, made unique by appending
/// `-1`, `-2`, … so extracting never merges into an existing folder.
fn unique_dir(parent: &Path, stem: &str) -> PathBuf {
    let base = parent.join(stem);
    if !base.exists() {
        return base;
    }
    for n in 1.. {
        let cand = parent.join(format!("{stem}-{n}"));
        if !cand.exists() {
            return cand;
        }
    }
    base
}
