//! `:du` — disk-usage analysis of a directory, on a worker thread.
//!
//! Sums each immediate child of the target directory (recursively for folders)
//! and shows them biggest-first with a bar and percentage, so "what is eating
//! the space" is a glance away. Enter drills into a folder; `-` climbs back.

use std::path::PathBuf;

use super::*;

impl App {
    /// Analyse `dir` (default: the active pane's directory) on a worker thread.
    pub(crate) fn start_du(&mut self, dir: PathBuf) {
        if self.du_job.is_some() {
            self.message = Some(tr(self.lang, "already analysing…", "解析中です…").into());
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let worker_dir = dir.clone();
        std::thread::spawn(move || {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let mut on_progress = |_n: u64| {};
            let entries = cian_core::du::analyze(&worker_dir, &cancel, &mut on_progress);
            let _ = tx.send((worker_dir, entries));
        });
        self.du_job = Some(rx);
        self.message = Some(tr(self.lang, "analysing disk usage…", "容量を解析中…").into());
    }

    /// `:du` on the active pane's directory (or the folder under the cursor).
    pub(crate) fn start_du_here(&mut self) {
        let dir = self
            .active_pane()
            .map(|p| {
                // A folder under the cursor is the natural target; otherwise the
                // whole pane directory.
                match p.selected().filter(|e| e.is_dir && !e.is_parent) {
                    Some(e) => e.path.clone(),
                    None => p.cwd.clone(),
                }
            })
            .unwrap_or_else(|| PathBuf::from("."));
        self.start_du(dir);
    }

    /// Install a finished analysis into the disk-usage popup. Returns true if a
    /// result landed.
    pub(crate) fn poll_du(&mut self) -> bool {
        let Some(rx) = &self.du_job else { return false };
        match rx.try_recv() {
            Ok((dir, entries)) => {
                self.du_job = None;
                let total: u64 = entries.iter().map(|e| e.size).sum();
                self.popup = Popup::DiskUsage { dir, entries, total, cursor: 0, scroll: 0 };
                self.message = None;
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.du_job = None;
                self.message = Some(tr(self.lang, "disk-usage analysis failed", "容量解析に失敗").into());
                true
            }
        }
    }

    /// Enter the highlighted disk-usage row: drill into a directory (re-analyse),
    /// or jump the file panes to the entry's location.
    pub(crate) fn du_enter(&mut self) {
        let Popup::DiskUsage { entries, cursor, .. } = &self.popup else { return };
        let Some(e) = entries.get(*cursor) else { return };
        if e.is_dir {
            let dir = e.path.clone();
            self.popup = Popup::None;
            self.start_du(dir);
        }
    }

    /// Climb to the parent directory and re-analyse it.
    pub(crate) fn du_parent(&mut self) {
        let Popup::DiskUsage { dir, .. } = &self.popup else { return };
        let Some(parent) = dir.parent().map(|p| p.to_path_buf()) else { return };
        self.popup = Popup::None;
        self.start_du(parent);
    }
}
