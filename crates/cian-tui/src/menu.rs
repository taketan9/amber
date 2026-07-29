//! The right-click / `M` context menu and clipboard paste: building the menu
//! for the focused surface, drilling into submenus, and running an item — plus
//! clip_targets/paste_clip/reload_both. Split out of lib.rs.
use super::*;

impl App {
    pub(crate) fn open_context_menu(&mut self, col: u16, row: u16) {
        // Whether the AI helper is usable, checked (and cached) up front so the
        // AI entries only appear when they will work.
        let ai = self.ai.is_some() && self.ai_ready();
        let mut items = Vec::new();
        // Launchers lead both menus, in the agreed order: crmaine ▸, snippets,
        // macros. Each appears only when it has something to offer.
        if ai {
            items.push(MenuItem::AiMenu);
        }
        if !self.config.snippets.is_empty() {
            items.push(MenuItem::Snippets);
        }
        if !self.macros.is_empty() {
            items.push(MenuItem::Macros);
        }
        let has_hosts = !self.config.ssh_hosts.is_empty();
        if self.focused == FocusedPane::Shell {
            // A PTY owns its own screen, so file operations make no sense here.
            items.push(MenuItem::Paste);
            if has_hosts {
                items.push(MenuItem::SendMenu); // Transfer ▸
            }
            // SSH leads: keys never reach the picker while the shell has focus,
            // so this menu is the only way to open it without leaving the shell.
            items.push(MenuItem::Ssh);
            items.push(MenuItem::SessionMenu); // log ▸ / encoding
            // Synchronize input — only when there's more than one pane.
            if self.shell.active_pane_count() > 1 {
                if self.shell.is_broadcasting() {
                    items.push(MenuItem::SyncStop);
                    // Narrow (or widen) the group to just the chosen panes.
                    items.push(MenuItem::SyncMember);
                } else {
                    items.push(MenuItem::SyncStart);
                }
            }
            items.push(MenuItem::WindowMenu);
            items.push(MenuItem::Background);
            items.push(MenuItem::ThemePick);
            items.push(MenuItem::Lang);
        } else {
            // Frequent file ops sit at the top level; the rest fold into groups.
            items.push(MenuItem::Copy);
            items.push(MenuItem::CopyPathText); // directly under Copy
            items.push(MenuItem::Cut);
            items.push(MenuItem::Paste);
            items.push(MenuItem::Rename);
            items.push(MenuItem::Delete);
            items.push(MenuItem::EditTab); // open in the editor in a new shell tab
            items.push(MenuItem::FileMenu); // to-other / copy-to / bulk rename
            // Archive ▸ when there is something to pack, or the cursor is on one.
            let has_targets = self.active_pane().map(|p| !p.target_paths().is_empty()).unwrap_or(false);
            let on_archive = self
                .active_pane()
                .and_then(|p| p.selected())
                .map(|e| !e.is_dir && cian_core::archive::is_archive(&e.path))
                .unwrap_or(false);
            if has_targets || on_archive {
                items.push(MenuItem::ArchiveMenu);
            }
            items.push(MenuItem::InspectMenu); // attributes / hash / compare / count / dupes
            if has_hosts {
                items.push(MenuItem::SendMenu); // Transfer ▸
            }
            match self.vcs_kind() {
                Some(Vcs::Git) => items.push(MenuItem::GitMenu),
                Some(Vcs::Svn) => items.push(MenuItem::SvnMenu),
                None => {}
            }
            items.push(MenuItem::Shortcuts);
            items.push(MenuItem::Ssh);
            items.push(MenuItem::Background);
            items.push(MenuItem::ViewMenu); // show hidden / language / copy path
            // The OS-native group sits last, just above Quit / the manual, so
            // "Show in Explorer / Finder" is always in the same easy-to-reach
            // spot (third from the bottom).
            items.push(MenuItem::OsMenu); // open / open-with / reveal / properties
        }
        // Quit and the manual close every menu — reachable by mouse alone
        // (quitting otherwise needs `q`, which the shell eats).
        items.push(MenuItem::Quit);
        items.push(MenuItem::Manual);
        self.menu_stack.clear();
        self.popup = Popup::ContextMenu { items, cursor: 0, at: (col, row) };
    }

    /// The children a group item drills into (context-dependent). `None` for a
    /// leaf item.
    pub(crate) fn submenu_children(&self, item: MenuItem) -> Option<Vec<MenuItem>> {
        match item {
            MenuItem::AiMenu => {
                let mut v = vec![MenuItem::AiChat];
                if self.focused == FocusedPane::Shell {
                    v.insert(0, MenuItem::AiExplainError);
                    v.insert(0, MenuItem::AiShellCmd);
                } else {
                    // In a file pane the AI can draft a commit for its repo,
                    // scan the folder for junk, and suggest a structure.
                    v.insert(0, MenuItem::AiCommit);
                    v.insert(0, MenuItem::AiRename);
                    v.insert(0, MenuItem::AiSearch);
                    v.insert(0, MenuItem::AiStructure);
                    v.insert(0, MenuItem::AiJunk);
                }
                v.push(MenuItem::Back);
                Some(v)
            }
            MenuItem::SendMenu => {
                Some(vec![MenuItem::ScpUpload, MenuItem::ScpDownload, MenuItem::Back])
            }
            MenuItem::GitMenu => Some(vec![
                MenuItem::GitStage,
                MenuItem::GitUnstage,
                MenuItem::GitDiscard,
                MenuItem::GitDiff,
                MenuItem::GitHistory,
                MenuItem::Back,
            ]),
            MenuItem::SvnMenu => Some(vec![
                MenuItem::SvnAdd,
                MenuItem::SvnRevert,
                MenuItem::SvnResolve,
                MenuItem::SvnDiff,
                MenuItem::SvnLog,
                MenuItem::SvnUpdate,
                MenuItem::SvnCommit,
                MenuItem::Back,
            ]),
            MenuItem::WindowMenu => {
                let mut v = vec![
                    MenuItem::ShellSplitLR,
                    MenuItem::ShellSplitTB,
                    MenuItem::ShellNewTab,
                ];
                // Offer the close that matches what is active: a split pane if
                // this tab is split, otherwise the tab itself.
                if self.shell.active_pane_count() > 1 {
                    v.push(MenuItem::ShellCloseSplit);
                } else {
                    v.push(MenuItem::ShellCloseTab);
                }
                v.push(MenuItem::ShellZoom);
                v.push(MenuItem::Back);
                Some(v)
            }
            MenuItem::CompressMenu => Some(vec![
                MenuItem::CompressZip,
                MenuItem::CompressZipEnc,
                MenuItem::CompressTarGz,
                MenuItem::Back,
            ]),
            MenuItem::FileMenu => Some(vec![
                MenuItem::CopyToOther,
                MenuItem::MoveToOther,
                MenuItem::CopyToPath,
                MenuItem::BulkRename,
                MenuItem::Back,
            ]),
            MenuItem::ArchiveMenu => {
                let mut v = vec![MenuItem::CompressMenu];
                // "Extract here" only when the cursor is on an archive.
                let on_archive = self
                    .active_pane()
                    .and_then(|p| p.selected())
                    .map(|e| !e.is_dir && cian_core::archive::is_archive(&e.path))
                    .unwrap_or(false);
                if on_archive {
                    v.push(MenuItem::Extract);
                }
                v.push(MenuItem::Back);
                Some(v)
            }
            MenuItem::InspectMenu => Some(vec![
                MenuItem::Attributes,
                MenuItem::Hash,
                MenuItem::Compare,
                MenuItem::Count,
                MenuItem::FindDupes,
                MenuItem::Back,
            ]),
            // The OS-native actions (#9): hand the selected file to the shell's
            // own verbs rather than reimplementing them.
            MenuItem::OsMenu => Some(vec![
                MenuItem::OpenDefault,
                MenuItem::OpenWithOs,
                MenuItem::RevealInOs,
                MenuItem::PropertiesOs,
                MenuItem::Back,
            ]),
            MenuItem::ViewMenu => Some(vec![
                MenuItem::HiddenToggle,
                MenuItem::ThemePick,
                MenuItem::ThemePickPane,
                MenuItem::Lang,
                MenuItem::Back,
            ]),
            MenuItem::SessionMenu => {
                let mut v = Vec::new();
                if self.shell.active_session().map(|s| s.is_logging()).unwrap_or(false) {
                    v.push(MenuItem::StopLog);
                } else {
                    v.push(MenuItem::StartLog);
                }
                v.push(MenuItem::Encoding);
                v.push(MenuItem::Back);
                Some(v)
            }
            _ => None,
        }
    }

    /// Open a submenu, stashing the current menu so `Back`/Esc returns to it.
    pub(crate) fn open_submenu(&mut self, items: Vec<MenuItem>) {
        let at = match &self.popup {
            Popup::ContextMenu { at, .. } => *at,
            _ => (0, 0),
        };
        let parent = std::mem::replace(&mut self.popup, Popup::None);
        if matches!(parent, Popup::ContextMenu { .. }) {
            self.menu_stack.push(parent);
        }
        self.popup = Popup::ContextMenu { items, cursor: 0, at };
    }

    /// Go back up one menu level, or close the menu when at the top.
    pub(crate) fn menu_back(&mut self) {
        match self.menu_stack.pop() {
            Some(parent) => self.popup = parent,
            None => self.popup = Popup::None,
        }
    }

    /// Put the pane's targets into the file clipboard.
    pub(crate) fn clip_targets(&mut self, op: ClipOp) {
        let paths = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        let verb = match op {
            ClipOp::Copy => "copied",
            ClipOp::Cut => "cut",
        };
        self.message = Some(format!("{} {} item(s)", verb, paths.len()));
        self.file_clip = Some(FileClipboard { paths, op });
    }

    /// Paste the file clipboard into the focused pane's directory.
    pub(crate) fn paste_clip(&mut self) -> Result<()> {
        // cian's own register wins when set — it was filled deliberately, from
        // here. Otherwise fall back to the OS clipboard, so a file copied in
        // Explorer or Finder pastes as you would expect.
        let (clip, from_os) = match self.file_clip.clone() {
            Some(c) => (c, false),
            None => {
                let paths = os_clipboard_files();
                if paths.is_empty() {
                    self.message = Some("clipboard has no files".into());
                    return Ok(());
                }
                (FileClipboard { paths, op: ClipOp::Copy }, true)
            }
        };
        let dest = match self.focused {
            FocusedPane::Shell => self.shell_cwd(),
            _ => match self.active_pane() {
                Some(p) => p.cwd.clone(),
                None => return Ok(()),
            },
        };
        // Pasting into the directory the files already live in would be a
        // no-op at best and a self-overwrite at worst.
        if clip.paths.iter().any(|p| p.parent() == Some(dest.as_path())) {
            self.message = Some("already in this directory".into());
            return Ok(());
        }
        let report = match clip.op {
            ClipOp::Copy => ops::copy_many(&clip.paths, &dest, Conflict::Skip),
            ClipOp::Cut => ops::move_many(&clip.paths, &dest, Conflict::Skip),
        };
        // A cut is consumed by its paste; a copy stays available.
        if clip.op == ClipOp::Cut {
            self.file_clip = None;
        }
        self.reload_both();
        self.flash(self.focused);
        if from_os && report.errors.is_empty() {
            // Say where they came from, so "paste" is never ambiguous about
            // which of the two clipboards it just used.
            self.message =
                Some(format!("pasted {} item(s) from the system clipboard", report.ok));
            return Ok(());
        }
        self.show_op_report(&report);
        Ok(())
    }

    /// Reload both file panes; a paste can change either of them.
    pub(crate) fn reload_both(&mut self) {
        let _ = self.left.active_mut().reload();
        let _ = self.right.active_mut().reload();
        // A file op or refresh may have changed the working tree.
        self.invalidate_git();
    }

    pub(crate) fn run_menu_item(&mut self, item: MenuItem) -> Result<()> {
        // A group drills into its submenu; Back climbs out — neither acts.
        if let Some(children) = self.submenu_children(item) {
            self.open_submenu(children);
            return Ok(());
        }
        if item == MenuItem::Back {
            self.menu_back();
            return Ok(());
        }
        // A leaf action closes the whole (possibly nested) menu.
        self.menu_stack.clear();
        self.popup = Popup::None;
        match item {
            MenuItem::AiMenu | MenuItem::SendMenu | MenuItem::WindowMenu | MenuItem::GitMenu | MenuItem::SvnMenu | MenuItem::CompressMenu | MenuItem::FileMenu | MenuItem::ArchiveMenu | MenuItem::InspectMenu | MenuItem::OsMenu | MenuItem::ViewMenu | MenuItem::SessionMenu | MenuItem::Back => {} // handled above
            MenuItem::OpenDefault => self.open_externally(),
            MenuItem::OpenWithOs => self.open_with_os(),
            MenuItem::RevealInOs => self.reveal_in_os(),
            MenuItem::PropertiesOs => self.properties_os(),
            MenuItem::CopyPathText => self.copy_paths_to_clipboard(),
            MenuItem::ShellSplitLR => {
                let cwd = self.shell_cwd();
                self.shell.split_active(&cwd, SplitDir::LeftRight);
                self.focus(FocusedPane::Shell);
            }
            MenuItem::ShellSplitTB => {
                let cwd = self.shell_cwd();
                self.shell.split_active(&cwd, SplitDir::TopBottom);
                self.focus(FocusedPane::Shell);
            }
            MenuItem::ShellNewTab => {
                let cwd = self.shell_cwd();
                self.shell.new_tab(&cwd);
                self.focus(FocusedPane::Shell);
            }
            MenuItem::ShellCloseSplit => {
                self.popup = Popup::ConfirmClose { target: CloseTarget::ShellPane };
            }
            MenuItem::ShellCloseTab => {
                if self.shell.close_active() {
                    self.focus(self.last_file_pane);
                }
            }
            MenuItem::ShellZoom => {
                self.focus(FocusedPane::Shell);
                // Don't fight a full-screen TUI running in the pane.
                if !self.shell.active_modes().0 {
                    self.toggle_zoom();
                }
            }
            MenuItem::SyncStart | MenuItem::SyncStop => {
                self.focus(FocusedPane::Shell);
                let on = self.shell.set_broadcast(matches!(item, MenuItem::SyncStart));
                self.message = Some(if on {
                    tr(self.lang, "⇄ synchronize ON — input goes to all panes", "⇄ 同時入力 ON — 全ペインに入力").into()
                } else {
                    tr(self.lang, "synchronize off", "同時入力 OFF").into()
                });
            }
            MenuItem::SyncMember => {
                self.focus(FocusedPane::Shell);
                let n = self.shell.toggle_sync_member();
                let total = self.shell.active_pane_count();
                self.message = Some(if n == 0 {
                    tr(self.lang, "⇄ sync group cleared — all panes", "⇄ 同時入力グループ解除 — 全ペイン").into()
                } else {
                    format!("⇄ sync group: {n}/{total}")
                });
            }
            MenuItem::Copy => self.clip_targets(ClipOp::Copy),
            MenuItem::Cut => self.clip_targets(ClipOp::Cut),
            // In the shell, "Paste" means the text on the clipboard goes to
            // the running program — the terminal sense of paste. Only in a file
            // pane does it mean the file clipboard.
            MenuItem::Paste if self.focused == FocusedPane::Shell => self.paste_text_to_shell(),
            MenuItem::Paste => return self.paste_clip(),
            MenuItem::CopyToOther => self.start_transfer(PendingOp::Copy),
            MenuItem::MoveToOther => self.start_transfer(PendingOp::Move),
            MenuItem::CopyToPath => self.start_dest_picker(PendingOp::Copy),
            MenuItem::Rename => self.start_rename(),
            MenuItem::Delete => self.start_delete(),
            MenuItem::Manual => self.open_manual(),
            MenuItem::Ssh => self.start_ssh(),
            MenuItem::ScpUpload => self.start_scp(ScpDir::Upload),
            MenuItem::ScpDownload => self.start_scp(ScpDir::Download),
            MenuItem::StartLog => self.start_log_prompt(),
            MenuItem::StopLog => self.stop_session_log(),
            MenuItem::AiChat => self.open_ai_chat(),
            MenuItem::AiShellCmd => self.start_ai_shell_prompt(),
            MenuItem::AiExplainError => self.explain_shell_error(),
            MenuItem::AiCommit => self.start_ai_commit_message(),
            MenuItem::AiJunk => self.start_ai_junk(),
            MenuItem::AiStructure => self.start_ai_structure(),
            MenuItem::AiRename => self.start_ai_rename_prompt(),
            MenuItem::AiSearch => self.start_ai_search_prompt(),
            MenuItem::GitStage => self.git_stage(),
            MenuItem::GitUnstage => self.git_unstage(),
            MenuItem::GitDiscard => self.git_discard_prompt(),
            MenuItem::GitHistory => self.start_git_log(),
            MenuItem::GitDiff => self.git_diff_file(),
            MenuItem::SvnAdd => self.git_stage(),
            MenuItem::SvnRevert => self.git_discard_prompt(),
            MenuItem::SvnResolve => self.svn_resolve(),
            MenuItem::SvnDiff => self.git_diff_file(),
            MenuItem::SvnLog => self.start_git_log(),
            MenuItem::SvnUpdate => self.svn_update(),
            MenuItem::SvnCommit => self.svn_commit_prompt(),
            MenuItem::BulkRename => self.start_bulk_rename(),
            MenuItem::Snippets => self.start_snippets(),
            MenuItem::Macros => self.start_macros(),
            MenuItem::EditTab => self.edit_in_new_tab(),
            MenuItem::ThemePick => self.start_theme_picker(),
            MenuItem::ThemePickPane => {
                let side = matches!(self.focused, FocusedPane::Right) as usize;
                self.start_pane_theme_picker(side);
            }
            MenuItem::Shortcuts => self.start_shortcuts(),
            MenuItem::Lang => {
                // Flip the interface language; every localized string reads
                // `self.lang` at draw time, so the next frame is fully in the
                // new language.
                self.lang = self.lang.toggled();
                self.message = Some(match self.lang {
                    Lang::En => "language: English".into(),
                    Lang::Ja => "言語: 日本語".into(),
                });
            }
            MenuItem::Encoding => {
                match self.shell.active_session() {
                    Some(s) => {
                        let cur = cian_core::viewer::TextEncoding::ALL
                            .iter()
                            .position(|e| *e == s.encoding())
                            .unwrap_or(0);
                        self.popup =
                            Popup::EncodingPicker { cursor: cur, target: EncTarget::Shell };
                    }
                    None => self.message = Some("no shell here".into()),
                }
            }
            MenuItem::Quit => self.start_quit_confirm(),
            MenuItem::HiddenToggle => self.toggle_hidden(),
            MenuItem::Attributes => self.show_attributes(),
            MenuItem::Hash => self.start_hash(cian_core::attrs::HashKind::Sha256),
            MenuItem::Compare => self.open_diff(),
            MenuItem::FindDupes => self.start_dupes(),
            MenuItem::Count => self.start_count(),
            MenuItem::Extract => self.extract_selected(),
            MenuItem::CompressZip => self.prompt_compress(CompressKind::Zip),
            MenuItem::CompressZipEnc => self.prompt_compress(CompressKind::ZipEnc),
            MenuItem::CompressTarGz => self.prompt_compress(CompressKind::TarGz),
            MenuItem::Background => {
                let pane = self.focused;
                let current = match pane {
                    FocusedPane::Shell => self.shell.active_pane_bg(),
                    _ => Self::bg_slot(pane).and_then(|s| self.pane_bg[s]),
                };
                let cur = current
                    .and_then(|c| PANE_BG_PRESETS.iter().position(|(_, p)| *p == Some(c)))
                    .unwrap_or(0);
                self.popup = Popup::ColorPicker { pane, cursor: cur };
            }
        }
        Ok(())
    }
}
