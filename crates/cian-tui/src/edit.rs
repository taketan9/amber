//! Editing a file in an external editor. cian does not embed one — a usable
//! vim/nvim is tens of megabytes with its runtime, which would blow the
//! single-binary size for little gain — so `E` in the viewer (and `:edit`)
//! shells out to the user's editor: `cian.set_option("editor", …)` if set, then
//! `$VISUAL`/`$EDITOR`, then the first of nvim → vim → vi found on `PATH`.
//!
//! The main loop does the actual suspend/run/restore around the terminal (see
//! [`crate::run_loop`]); here we resolve *which* editor and expose the trigger.

use std::path::{Path, PathBuf};

use crate::{tr, App, Popup};

/// A queued edit: the file, a title to re-open the viewer with, and whether to
/// re-open it afterwards (edits launched from the viewer return to it).
pub(crate) struct PendingEdit {
    pub path: PathBuf,
    pub title: String,
    pub reopen_viewer: bool,
}

impl App {
    /// `E` in the viewer: edit the file being viewed, then return to the viewer.
    pub(crate) fn edit_viewed_file(&mut self) {
        if let Popup::Viewer { path, title, .. } = &self.popup {
            self.pending_edit = Some(PendingEdit {
                path: path.clone(),
                title: title.clone(),
                reopen_viewer: true,
            });
        }
    }

    /// `:edit` / `:e`: edit the file under the cursor in the active pane.
    pub(crate) fn edit_selected_file(&mut self) {
        let Some(p) = self.active_pane() else { return };
        match p.selected().filter(|e| !e.is_parent) {
            Some(e) if !e.is_dir => {
                self.pending_edit = Some(PendingEdit {
                    path: e.path.clone(),
                    title: e.name.clone(),
                    reopen_viewer: false,
                });
            }
            Some(_) => self.message = Some(tr(self.lang, "that is a directory", "ディレクトリです").into()),
            None => self.message = Some(tr(self.lang, "nothing to edit", "編集対象がありません").into()),
        }
    }

    /// `:edittab` / right-click "Edit in new tab": open the file under the cursor
    /// in the editor (the configured one, else nvim → vim → vi if installed) in a
    /// fresh shell tab, so cian keeps running. Does nothing but explain if no
    /// editor is on PATH.
    pub(crate) fn edit_in_new_tab(&mut self) {
        let path = match self.active_pane().and_then(|p| p.selected()) {
            Some(e) if e.is_parent => None,
            Some(e) if e.is_dir => {
                self.message = Some(tr(self.lang, "that is a directory", "ディレクトリです").into());
                return;
            }
            Some(e) => Some(e.path.clone()),
            None => None,
        };
        let Some(path) = path else {
            self.message = Some(tr(self.lang, "nothing to edit", "編集対象がありません").into());
            return;
        };
        let Some(words) = crate::edit::resolve_editor(&self.config) else {
            self.message = Some(tr(
                self.lang,
                "no editor found — install nvim/vim/vi, or set cian.set_option(\"editor\", …)",
                "エディタが見つかりません — nvim/vim/vi を入れるか cian.set_option(\"editor\", …) を設定",
            ).into());
            return;
        };
        // Quote the path so a name with spaces survives the shell.
        let cmd = format!("{} \"{}\"", words.join(" "), path.display());
        let cwd = self.shell_cwd();
        self.shell.new_tab_running(&cwd, cmd);
        self.focus(crate::FocusedPane::Shell);
        self.message = Some(format!("editing {} in a new tab", path.display()));
    }
}

/// Resolve the editor command (without the file argument) for `config`,
/// honouring `$VISUAL`/`$EDITOR` and finally the nvim → vim → vi fallback.
pub(crate) fn resolve_editor(config: &cian_lua::Config) -> Option<Vec<String>> {
    let visual = std::env::var("VISUAL").ok().filter(|s| !s.trim().is_empty());
    let editor = std::env::var("EDITOR").ok().filter(|s| !s.trim().is_empty());
    pick_editor(config.options.editor.as_deref(), visual.as_deref(), editor.as_deref(), on_path)
}

/// The resolution logic, split from the environment so it can be tested: an
/// explicit editor (config, then `$VISUAL`, then `$EDITOR`) is trusted as-is;
/// otherwise the first of nvim → vim → vi that `found` reports on `PATH`.
fn pick_editor(
    configured: Option<&str>,
    visual: Option<&str>,
    editor_env: Option<&str>,
    found: impl Fn(&str) -> bool,
) -> Option<Vec<String>> {
    if let Some(cmd) = configured.or(visual).or(editor_env) {
        let words: Vec<String> = cmd.split_whitespace().map(|s| s.to_string()).collect();
        if !words.is_empty() {
            return Some(words);
        }
    }
    ["nvim", "vim", "vi"].into_iter().find(|n| found(n)).map(|n| vec![n.to_string()])
}

/// Is `name` an executable on `PATH`? On Windows the usual executable
/// extensions are tried too.
fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
    std::env::split_paths(&path).any(|dir| {
        exts.iter().any(|ext| {
            let cand: PathBuf = dir.join(format!("{name}{ext}"));
            is_executable(&cand)
        })
    })
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_editor_is_trusted_verbatim() {
        // Even split into program + args, and even if "found" says no (the user
        // knows their own machine).
        let cmd = pick_editor(Some("code -w"), None, None, |_| false);
        assert_eq!(cmd, Some(vec!["code".into(), "-w".into()]));
    }

    #[test]
    fn falls_back_through_visual_then_editor() {
        assert_eq!(pick_editor(None, Some("hx"), Some("nano"), |_| false), Some(vec!["hx".into()]));
        assert_eq!(pick_editor(None, None, Some("nano"), |_| false), Some(vec!["nano".into()]));
    }

    #[test]
    fn path_search_prefers_nvim_then_vim_then_vi() {
        // Only vim + vi present → vim wins over vi.
        let have = |n: &str| n == "vim" || n == "vi";
        assert_eq!(pick_editor(None, None, None, have), Some(vec!["vim".into()]));
        // nvim present → it wins.
        assert_eq!(pick_editor(None, None, None, |n| n == "nvim" || n == "vim"), Some(vec!["nvim".into()]));
        // only vi.
        assert_eq!(pick_editor(None, None, None, |n| n == "vi"), Some(vec!["vi".into()]));
    }

    #[test]
    fn nothing_found_is_none() {
        assert_eq!(pick_editor(None, None, None, |_| false), None);
    }
}
