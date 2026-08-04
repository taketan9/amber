//! Disk-usage analysis — "what is eating the space under here", biggest first.
//!
//! For each immediate child of a directory it totals the bytes below it (a
//! directory is summed recursively), then sorts largest first — the ncdu /
//! WinDirStat view, but built from plain `read_dir` + `metadata` so it never
//! reads a file's contents. Symlinks are not followed, so a link back up the
//! tree can neither loop nor double-count.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// One immediate child of the analysed directory, with its total size.
#[derive(Debug, Clone)]
pub struct DuEntry {
    pub name: String,
    pub path: PathBuf,
    /// Total bytes at or below this entry (recursive for a directory).
    pub size: u64,
    pub is_dir: bool,
}

/// Total the size of each immediate child of `dir`, largest first. `on_progress`
/// is called with the running count of entries visited so a long walk can show
/// life; `cancel` stops it early (returning whatever was summed so far).
pub fn analyze(dir: &Path, cancel: &AtomicBool, on_progress: &mut dyn FnMut(u64)) -> Vec<DuEntry> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else { return out };
    let mut visited = 0u64;
    for e in rd.flatten() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let path = e.path();
        let ft = e.file_type();
        // A symlink (even to a directory) is listed at its own link size, never
        // followed — same rule as the recursive walk below.
        let is_symlink = ft.as_ref().map(|t| t.is_symlink()).unwrap_or(false);
        let is_dir = !is_symlink && ft.as_ref().map(|t| t.is_dir()).unwrap_or(false);
        let size = if is_dir {
            dir_size(&path, cancel, &mut visited, on_progress)
        } else {
            visited += 1;
            on_progress(visited);
            e.metadata().map(|m| m.len()).unwrap_or(0)
        };
        out.push(DuEntry { name: e.file_name().to_string_lossy().into_owned(), path, size, is_dir });
    }
    out.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Sum every regular file beneath `dir` (iterative, so a deep tree can't blow
/// the stack). Symlinks are skipped.
fn dir_size(dir: &Path, cancel: &AtomicBool, visited: &mut u64, on_progress: &mut dyn FnMut(u64)) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            *visited += 1;
            // Report every so often — a per-entry callback on a huge tree would
            // itself dominate the walk.
            if *visited % 512 == 0 {
                on_progress(*visited);
            }
            let ft = e.file_type();
            let is_symlink = ft.as_ref().map(|t| t.is_symlink()).unwrap_or(false);
            if is_symlink {
                continue;
            }
            if ft.as_ref().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push(e.path());
            } else {
                total += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_each_child_recursively_and_sorts_largest_first() {
        let d = tempfile::tempdir().unwrap();
        // a small file, and a directory holding a bigger one.
        fs::write(d.path().join("small.txt"), vec![0u8; 100]).unwrap();
        fs::create_dir(d.path().join("big")).unwrap();
        fs::write(d.path().join("big/data.bin"), vec![0u8; 5000]).unwrap();
        fs::create_dir(d.path().join("big/nested")).unwrap();
        fs::write(d.path().join("big/nested/more.bin"), vec![0u8; 2000]).unwrap();

        let cancel = AtomicBool::new(false);
        let mut seen = 0u64;
        let result = analyze(d.path(), &cancel, &mut |n| seen = n);

        assert_eq!(result.len(), 2);
        // "big" (7000, recursive) sorts before "small.txt" (100).
        assert_eq!(result[0].name, "big");
        assert!(result[0].is_dir);
        assert_eq!(result[0].size, 7000);
        assert_eq!(result[1].name, "small.txt");
        assert_eq!(result[1].size, 100);
    }

    #[test]
    fn an_unreadable_or_empty_directory_gives_an_empty_list() {
        let d = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        assert!(analyze(d.path(), &cancel, &mut |_| {}).is_empty());
        assert!(analyze(&d.path().join("nope"), &cancel, &mut |_| {}).is_empty());
    }
}
