//! Reading a file *in* the pane — what `Enter` does.
//!
//! `F3` and Shift+Tab open the editor, which takes the screen because that is
//! what editing wants. `Enter` is the other half of reading: the file appears
//! where its listing was, the other pane stays where it is, and Esc puts the
//! listing back. Nothing is opened over anything.
//!
//! It is deliberately a pager and not a second editor: motions, and the way
//! out. `F3` from here promotes the same file to the real thing.

use super::*;

/// A file being read inside a pane.
pub(crate) struct PaneFile {
    pub path: PathBuf,
    pub title: String,
    pub lines: Vec<String>,
    /// First visible line.
    pub scroll: usize,
    /// Which row of the listing to put the cursor back on when it closes.
    pub back_to: usize,
}

impl App {
    fn side(pane: FocusedPane) -> Option<usize> {
        match pane {
            FocusedPane::Left => Some(0),
            FocusedPane::Right => Some(1),
            FocusedPane::Shell => None,
        }
    }

    /// The file open in `pane`, if any.
    pub(crate) fn pane_file(&self, pane: FocusedPane) -> Option<&PaneFile> {
        Self::side(pane).and_then(|i| self.pane_files[i].as_ref())
    }

    /// `Enter` on a file: read it here.
    ///
    /// Text as text, anything else as the hex dump the viewer would show —
    /// the same decoding, so a Shift_JIS log reads the same way it does in
    /// the editor.
    pub(crate) fn open_file_in_pane(&mut self, path: &Path) {
        let Some(i) = Self::side(self.focused) else { return };
        let view = match cian_core::viewer::view_file(path) {
            Ok(v) => v,
            Err(e) => {
                self.message = Some(format!("{e}"));
                return;
            }
        };
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let back_to = self.active_pane().map(|p| p.cursor).unwrap_or(0);
        self.pane_files[i] = Some(PaneFile {
            path: path.to_path_buf(),
            title,
            lines: view.lines,
            scroll: 0,
            back_to,
        });
        self.note_recent_file(path);
    }

    /// Esc — the listing comes back, on the row it was left on.
    pub(crate) fn close_pane_file(&mut self) {
        let Some(i) = Self::side(self.focused) else { return };
        let Some(f) = self.pane_files[i].take() else { return };
        if let Some(p) = self.active_pane_mut() {
            let last = p.entries.len().saturating_sub(1);
            p.cursor = f.back_to.min(last);
        }
    }

    /// `F3` from a pane that is reading a file: the same file, in the editor.
    pub(crate) fn promote_pane_file(&mut self) -> bool {
        let Some(i) = Self::side(self.focused) else { return false };
        let Some(f) = self.pane_files[i].as_ref() else { return false };
        let (path, title) = (f.path.clone(), f.title.clone());
        self.close_pane_file();
        self.open_viewer_at(&path, &title, 0);
        true
    }

    /// The pager's keys: motions, and the way out. Returns true when the key
    /// was one of them.
    pub(crate) fn pane_file_key(&mut self, key: KeyEvent) -> bool {
        let Some(i) = Self::side(self.focused) else { return false };
        if self.pane_files[i].is_none() {
            return false;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        // How much of the file is on screen — the pane's rows, less its frame
        // and header.
        let page = (self.layout_rects.for_pane(self.focused).height as usize).saturating_sub(4).max(1);
        match key.code {
            // Out, and back to the listing.
            KeyCode::Esc
            | KeyCode::Backspace
            | KeyCode::Left
            | KeyCode::Char('q')
            | KeyCode::Char('h') => {
                self.close_pane_file();
                true
            }
            // Up to the real thing.
            KeyCode::F(3) => {
                self.promote_pane_file();
                true
            }
            _ => {
                let Some(f) = self.pane_files[i].as_mut() else { return false };
                let last = f.lines.len().saturating_sub(1);
                let step = |s: &mut usize, by: usize, down: bool| {
                    *s = if down { (*s + by).min(last) } else { s.saturating_sub(by) };
                };
                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => step(&mut f.scroll, 1, true),
                    KeyCode::Char('k') | KeyCode::Up => step(&mut f.scroll, 1, false),
                    KeyCode::Char('d') if ctrl => step(&mut f.scroll, page / 2, true),
                    KeyCode::Char('u') if ctrl => step(&mut f.scroll, page / 2, false),
                    KeyCode::PageDown | KeyCode::Char(' ') => step(&mut f.scroll, page, true),
                    KeyCode::PageUp => step(&mut f.scroll, page, false),
                    KeyCode::Char('g') | KeyCode::Home => f.scroll = 0,
                    KeyCode::Char('G') | KeyCode::End => f.scroll = last,
                    // Anything else belongs to the pane, not to the pager —
                    // switching panes, the menu, `:` and so on still work.
                    _ => return false,
                }
                true
            }
        }
    }

    /// The wheel over a pane that is reading a file.
    pub(crate) fn pane_file_scroll(&mut self, pane: FocusedPane, down: bool, by: usize) -> bool {
        let Some(i) = Self::side(pane) else { return false };
        let Some(f) = self.pane_files[i].as_mut() else { return false };
        let last = f.lines.len().saturating_sub(1);
        f.scroll = if down { (f.scroll + by).min(last) } else { f.scroll.saturating_sub(by) };
        true
    }
}
