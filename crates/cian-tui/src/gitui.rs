//! Git actions on the App: refreshing/caching per-pane status, stage, unstage,
//! and discard (with its confirm), plus the accessors the rest of the UI reads.
//! The git plumbing itself lives in cian-core::git.
use super::*;

impl App {
    /// The directory a newly-spawned shell should start in: the cwd of the
    /// file pane we were last on.
    /// Recompute each pane's git status when its directory has changed since the
    /// last cache. Called once per frame; the `git` shell-out only runs on an
    /// actual directory change, so it costs nothing while browsing one folder.
    pub(crate) fn ensure_git(&mut self) {
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
    pub(crate) fn invalidate_git(&mut self) {
        self.git = [None, None];
    }

    /// The active file pane's directory, if it sits in a git repository. Uses
    /// the cached status when it is warm, and falls back to a direct check (the
    /// cache is cold right after a git action invalidates it).
    pub(crate) fn git_repo_dir(&self) -> Option<PathBuf> {
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
    pub(crate) fn git_targets(&self) -> Vec<PathBuf> {
        self.active_pane().map(|p| p.target_paths()).unwrap_or_default()
    }

    /// `git add` the selection.
    pub(crate) fn git_stage(&mut self) {
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
    pub(crate) fn git_unstage(&mut self) {
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
    pub(crate) fn git_discard_prompt(&mut self) {
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
    pub(crate) fn git_discard(&mut self) {
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
    pub(crate) fn git_for(&self, pane: FocusedPane) -> Option<&cian_core::git::RepoStatus> {
        let idx = match pane {
            FocusedPane::Left => 0,
            FocusedPane::Right => 1,
            FocusedPane::Shell => return None,
        };
        self.git[idx].as_ref().and_then(|g| g.status.as_ref())
    }

    /// Show read-only `text` in the viewer (used for git diffs / commit views).
    /// A synthetic view: no file on disk, not editable.
    pub(crate) fn open_text_viewer(&mut self, title: &str, text: String) {
        let total = text.len() as u64;
        let view = cian_core::viewer::View::from_text(text, total, false);
        let source = view.lines.clone();
        self.popup = Popup::Viewer {
            title: title.to_string(),
            path: PathBuf::new(),
            view,
            scroll: 0,
            line: 0,
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
            blame: Vec::new(),
            hl_lang: None,
            hl: Vec::new(),
            editable: false,
            editing: false,
            dirty: false,
        };
    }

    /// `:gitdiff` / right-click: show the selected file's working-tree changes
    /// versus HEAD, in the viewer.
    pub(crate) fn git_diff_file(&mut self) {
        let Some(dir) = self.git_repo_dir() else {
            self.message = Some("not a git repository".into());
            return;
        };
        let Some(entry) = self.active_pane().and_then(|p| p.selected()).filter(|e| !e.is_parent) else {
            self.message = Some("select a file to diff".into());
            return;
        };
        let (path, name) = (entry.path.clone(), entry.name.clone());
        match cian_core::git::file_diff(&dir, &path) {
            Some(d) if !d.trim().is_empty() => self.open_text_viewer(&format!("git diff HEAD — {}", name), d),
            Some(_) => self.message = Some(format!("{}: no changes vs HEAD", name)),
            None => self.message = Some("git diff failed".into()),
        }
    }

    /// `@`/`:gitlog` / right-click: the commit log — one file's history when the
    /// cursor is on a tracked file, otherwise the whole repository.
    pub(crate) fn start_git_log(&mut self) {
        let Some(dir) = self.git_repo_dir() else {
            self.message = Some("not a git repository".into());
            return;
        };
        let file = self
            .active_pane()
            .and_then(|p| p.selected())
            .filter(|e| !e.is_parent && !e.is_dir)
            .map(|e| (e.path.clone(), e.name.clone()));
        let commits = cian_core::git::log(&dir, file.as_ref().map(|(p, _)| p.as_path()), 300);
        if commits.is_empty() {
            self.message = Some("no commits".into());
            return;
        }
        let title = match &file {
            Some((_, name)) => format!("git log — {}", name),
            None => "git log".to_string(),
        };
        self.popup = Popup::GitLog { title, dir, commits, cursor: 0, scroll: 0 };
    }

    /// Show a commit's diff (`git show`) in the viewer.
    pub(crate) fn git_show_commit(&mut self, hash: &str, dir: &Path) {
        match cian_core::git::show(dir, hash) {
            Some(s) => self.open_text_viewer(&format!("commit {}", hash), s),
            None => self.message = Some("git show failed".into()),
        }
    }

    /// `B` in the viewer: toggle the git blame gutter for the viewed file.
    pub(crate) fn toggle_viewer_blame(&mut self) {
        let path = if let Popup::Viewer { path, blame, .. } = &self.popup {
            if !blame.is_empty() {
                // Already on → turn it off.
                if let Popup::Viewer { blame, .. } = &mut self.popup {
                    blame.clear();
                }
                self.message = Some(tr(self.lang, "blame off", "blame オフ").into());
                return;
            }
            path.clone()
        } else {
            return;
        };
        let Some(dir) = path.parent() else { return };
        match cian_core::git::blame(dir, &path) {
            Some(b) if !b.is_empty() => {
                if let Popup::Viewer { blame, .. } = &mut self.popup {
                    *blame = b;
                }
                self.message = Some(tr(self.lang, "blame on", "blame オン").into());
            }
            _ => self.message = Some(tr(self.lang, "no blame (untracked or not a repo)", "blame不可（未追跡/非リポジトリ）").into()),
        }
    }
}
