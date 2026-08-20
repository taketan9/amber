//! The UI-toggles menu (`T` / `:toggles`): one place to flip the handful of
//! live on/off settings, inspired by AstroNvim's `<Leader>u`. Each row shows its
//! current state and toggles in place, so the menu stays open for several flips.

use super::*;

/// One switch in the toggles menu.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ToggleId {
    /// Show dotfiles in the panes.
    Dotfiles,
    /// Broadcast keyboard input to every shell pane in the tab.
    Sync,
    /// Ring the bell / post a desktop toast when a long job finishes.
    Notify,
    /// Re-read and checksum-verify files after an SFTP transfer.
    Verify,
    /// Cursor-follow preview in the shell panel's area.
    Preview,
    /// Let sweeps read cloud placeholders (and so download them).
    ReadCloud,
    /// Interface language (English ↔ Japanese).
    Lang,
    /// Which grammar the built-in editor answers to: vim ↔ notepad.
    EditStyle,
}

impl App {
    /// Open the toggles menu (`T` / `:toggles`).
    pub(crate) fn start_toggles(&mut self) {
        self.popup = Popup::Toggles { cursor: 0 };
    }

    /// The rows shown in the menu: `(id, label, state text, on?)`. `on` drives
    /// the accent so an enabled switch reads at a glance.
    pub(crate) fn toggle_rows(&self) -> Vec<(ToggleId, String, String, bool)> {
        let ja = self.lang == Lang::Ja;
        let onoff = |b: bool| {
            if b {
                tr(self.lang, "on", "ON").to_string()
            } else {
                tr(self.lang, "off", "OFF").to_string()
            }
        };
        let dotfiles = self.active_pane().map(|p| p.show_hidden).unwrap_or(false);
        let sync = self.shell.is_broadcasting();
        let notify = self.notify_runtime.or(self.config.options.notify).unwrap_or(true);
        let verify = self.verify_runtime.or(self.config.options.verify_transfers).unwrap_or(false);
        vec![
            (ToggleId::Dotfiles, tr(self.lang, "Dotfiles", "隠しファイル").into(), onoff(dotfiles), dotfiles),
            (ToggleId::Sync, tr(self.lang, "Input sync (all shells)", "入力同期（全シェル）").into(), onoff(sync), sync),
            (ToggleId::Notify, tr(self.lang, "Task-done notification", "完了通知").into(), onoff(notify), notify),
            (ToggleId::Verify, tr(self.lang, "Verify transfers", "転送後ベリファイ").into(), onoff(verify), verify),
            (
                ToggleId::Preview,
                tr(self.lang, "Cursor preview (shell panel)", "カーソル追従プレビュー").into(),
                onoff(self.preview_on),
                self.preview_on,
            ),
            (
                ToggleId::ReadCloud,
                tr(self.lang, "Read ☁ cloud-only files", "☁ クラウド上のファイルも読む").into(),
                onoff(cian_core::cloud::include()),
                cian_core::cloud::include(),
            ),
            (
                ToggleId::Lang,
                tr(self.lang, "Language", "言語").into(),
                if ja { "日本語".into() } else { "English".into() },
                ja,
            ),
            // Named by what it *is*, not by on/off: neither of the two is the
            // absence of the other, and "Editor: off" would say nothing. The
            // accent goes to vim because that is the one this switch is a
            // departure from.
            (
                ToggleId::EditStyle,
                tr(self.lang, "Editor keys", "エディタのキー操作").into(),
                match self.edit_style {
                    crate::EditStyle::Vim => tr(self.lang, "vim", "vim").to_string(),
                    crate::EditStyle::Notepad => {
                        tr(self.lang, "notepad", "メモ帳ふう").to_string()
                    }
                },
                self.edit_style == crate::EditStyle::Vim,
            ),
        ]
    }

    /// Move the menu cursor by `delta`, clamped.
    pub(crate) fn toggles_move(&mut self, delta: isize) {
        let n = self.toggle_rows().len();
        if let Popup::Toggles { cursor } = &mut self.popup {
            if n > 0 {
                *cursor = (*cursor as isize + delta).clamp(0, n as isize - 1) as usize;
            }
        }
    }

    /// Flip the highlighted switch, leaving the menu open.
    pub(crate) fn toggles_apply(&mut self) {
        let cursor = match self.popup {
            Popup::Toggles { cursor } => cursor,
            _ => return,
        };
        let Some((id, ..)) = self.toggle_rows().into_iter().nth(cursor) else { return };
        match id {
            ToggleId::Dotfiles => self.toggle_hidden(),
            ToggleId::Sync => {
                let on = self.shell.toggle_broadcast();
                self.message = Some(if on {
                    tr(self.lang, "⇄ input synchronize ON", "⇄ 入力同期 ON").into()
                } else {
                    tr(self.lang, "input synchronize off", "入力同期 OFF").into()
                });
            }
            ToggleId::Notify => {
                let cur = self.notify_runtime.or(self.config.options.notify).unwrap_or(true);
                self.notify_runtime = Some(!cur);
            }
            ToggleId::Verify => {
                let cur =
                    self.verify_runtime.or(self.config.options.verify_transfers).unwrap_or(false);
                self.verify_runtime = Some(!cur);
            }
            ToggleId::Preview => self.toggle_preview(),
            ToggleId::ReadCloud => {
                let on = !cian_core::cloud::include();
                cian_core::cloud::set_include(on);
                self.message = Some(if on {
                    tr(self.lang,
                       "⚠ sweeps will now download cloud-only files",
                       "⚠ 一括処理がクラウド上のファイルをダウンロードします").into()
                } else {
                    tr(self.lang, "sweeps skip cloud-only files", "一括処理はクラウド上のファイルを飛ばします").into()
                });
            }
            // The whole interface, menus included — see `MenuItem::Lang`.
            ToggleId::Lang => {
                self.lang = self.lang.toggled();
                if !self.menu_lang_pinned {
                    self.menu_lang = self.lang;
                }
            }
            ToggleId::EditStyle => {
                self.edit_style = match self.edit_style {
                    crate::EditStyle::Vim => crate::EditStyle::Notepad,
                    crate::EditStyle::Notepad => crate::EditStyle::Vim,
                };
                // A file already open changes grammar under the cursor, which
                // is the point of flipping this while one is: the switch is
                // reached for *because* the editor is not behaving the way the
                // hands expect. Notepad is always typing, so a panel that was
                // sitting in normal mode starts taking text.
                self.sync_edit_style();
                self.message = Some(match self.edit_style {
                    crate::EditStyle::Vim => tr(
                        self.lang,
                        "editor: vim keys — i to type, Esc to stop",
                        "エディタ: vim キー操作 — i で入力、Esc で抜ける",
                    )
                    .into(),
                    crate::EditStyle::Notepad => tr(
                        self.lang,
                        "editor: notepad keys — just type; Shift+arrows select",
                        "エディタ: メモ帳ふう — そのまま入力、Shift+矢印で選択",
                    )
                    .into(),
                });
            }
        }
    }
}
