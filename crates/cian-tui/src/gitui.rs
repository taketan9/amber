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
}
