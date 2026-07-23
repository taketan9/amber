//! Comparing two directory trees and listing what differs.
//!
//! The question this answers is "which files and folders are not the same
//! between these two trees" — not *how* their contents differ. So it never
//! reads a file: two files are judged the same when their size and
//! modification time match, the way rsync's and robocopy's quick scans do.
//! That keeps it instant even on a large tree, at the cost of missing an edit
//! that somehow preserved both size and mtime.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

/// Why an entry is listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Present on the left only.
    OnlyLeft,
    /// Present on the right only.
    OnlyRight,
    /// Present on both, but they differ (size/mtime, or file-vs-directory).
    Differ,
}

/// One differing path, relative to the two roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub rel: PathBuf,
    pub status: Status,
    /// True when the entry is a directory on the side(s) it exists.
    pub is_dir: bool,
}

/// The comparison of two trees.
#[derive(Debug, Clone, Default)]
pub struct DirDiff {
    /// Differing entries, sorted by path.
    pub entries: Vec<Entry>,
    /// True if the walk hit [`MAX_ENTRIES`] and stopped early.
    pub truncated: bool,
    /// True if it was cancelled before finishing.
    pub cancelled: bool,
}

impl DirDiff {
    pub fn is_identical(&self) -> bool {
        self.entries.is_empty() && !self.truncated && !self.cancelled
    }
}

/// Stop before the result list becomes useless to scroll.
pub const MAX_ENTRIES: usize = 5000;

/// Compare `left` and `right` recursively, reporting every path that differs.
pub fn compare(left: &Path, right: &Path, cancel: &AtomicBool) -> DirDiff {
    let mut out = DirDiff::default();
    walk(left, right, Path::new(""), cancel, &mut out);
    out.entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

fn walk(left: &Path, right: &Path, rel: &Path, cancel: &AtomicBool, out: &mut DirDiff) {
    if cancel.load(Ordering::Relaxed) {
        out.cancelled = true;
        return;
    }
    if out.entries.len() >= MAX_ENTRIES {
        out.truncated = true;
        return;
    }

    // Names present on each side. An unreadable directory is treated as empty
    // rather than aborting the whole comparison.
    let ln = names(&left.join(rel));
    let rn = names(&right.join(rel));
    let all: BTreeSet<&String> = ln.iter().chain(rn.iter()).collect();

    for name in all {
        if out.entries.len() >= MAX_ENTRIES {
            out.truncated = true;
            return;
        }
        let child = rel.join(name);
        let lp = left.join(&child);
        let rp = right.join(&child);
        let lm = fs::symlink_metadata(&lp).ok();
        let rm = fs::symlink_metadata(&rp).ok();

        match (lm, rm) {
            (Some(l), None) => {
                out.entries.push(Entry { rel: child, status: Status::OnlyLeft, is_dir: l.is_dir() });
            }
            (None, Some(r)) => {
                out.entries.push(Entry { rel: child, status: Status::OnlyRight, is_dir: r.is_dir() });
            }
            (Some(l), Some(r)) => {
                let both_dirs = l.is_dir() && r.is_dir();
                if both_dirs {
                    // Recurse; the directory itself is "the same" — only its
                    // differing contents get listed.
                    walk(left, right, &child, cancel, out);
                } else if l.is_dir() != r.is_dir() {
                    // One a directory, the other a file: a real difference.
                    out.entries.push(Entry { rel: child, status: Status::Differ, is_dir: false });
                } else if files_differ(&l, &r) {
                    out.entries.push(Entry { rel: child, status: Status::Differ, is_dir: false });
                }
            }
            (None, None) => {}
        }
    }
}

/// The entry names directly inside `dir`, or empty if it cannot be read.
fn names(dir: &Path) -> Vec<String> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect()
}

/// Two non-directory entries differ when their size or modification time does.
fn files_differ(l: &fs::Metadata, r: &fs::Metadata) -> bool {
    if l.len() != r.len() {
        return true;
    }
    match (l.modified().ok(), r.modified().ok()) {
        (Some(a), Some(b)) => a != b,
        // No timestamps to compare and equal sizes: call them the same.
        _ => false,
    }
}

/// Set a path's modification time (test helper, and handy for callers).
#[doc(hidden)]
pub fn set_mtime(path: &Path, t: SystemTime) -> std::io::Result<()> {
    fs::OpenOptions::new().write(true).open(path)?.set_modified(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn run(l: &Path, r: &Path) -> Vec<(String, Status)> {
        let cancel = AtomicBool::new(false);
        compare(l, r, &cancel)
            .entries
            .into_iter()
            .map(|e| (e.rel.display().to_string().replace('\\', "/"), e.status))
            .collect()
    }

    #[test]
    fn identical_trees_report_nothing() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        for d in [l.path(), r.path()] {
            fs::create_dir(d.join("sub")).unwrap();
            fs::write(d.join("a.txt"), b"hello").unwrap();
            fs::write(d.join("sub/b.txt"), b"world").unwrap();
        }
        // Match the mtimes so the quick check sees them as equal.
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        for d in [l.path(), r.path()] {
            set_mtime(&d.join("a.txt"), t).unwrap();
            set_mtime(&d.join("sub/b.txt"), t).unwrap();
        }
        let cancel = AtomicBool::new(false);
        assert!(compare(l.path(), r.path(), &cancel).is_identical());
    }

    #[test]
    fn only_on_one_side_is_reported_without_descending() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        // A whole directory only on the left, with contents.
        fs::create_dir_all(l.path().join("onlyL/deep")).unwrap();
        fs::write(l.path().join("onlyL/deep/x.txt"), b"x").unwrap();
        // A file only on the right.
        fs::write(r.path().join("onlyR.txt"), b"r").unwrap();

        let hits = run(l.path(), r.path());
        assert!(hits.contains(&("onlyL".to_string(), Status::OnlyLeft)), "{:?}", hits);
        assert!(hits.contains(&("onlyR.txt".to_string(), Status::OnlyRight)), "{:?}", hits);
        // The only-on-left directory is named, but its contents are not.
        assert!(!hits.iter().any(|(p, _)| p.contains("deep")), "should not descend: {:?}", hits);
    }

    #[test]
    fn a_size_difference_is_a_diff() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        fs::write(l.path().join("f.txt"), b"short").unwrap();
        fs::write(r.path().join("f.txt"), b"a much longer body").unwrap();
        assert_eq!(run(l.path(), r.path()), vec![("f.txt".to_string(), Status::Differ)]);
    }

    #[test]
    fn a_newer_mtime_at_equal_size_is_a_diff() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        fs::write(l.path().join("f.txt"), b"12345").unwrap();
        fs::write(r.path().join("f.txt"), b"12345").unwrap(); // same size
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        set_mtime(&l.path().join("f.txt"), base).unwrap();
        set_mtime(&r.path().join("f.txt"), base + Duration::from_secs(60)).unwrap();
        assert_eq!(run(l.path(), r.path()), vec![("f.txt".to_string(), Status::Differ)]);
    }

    #[test]
    fn a_file_versus_a_directory_of_the_same_name_differs() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        fs::write(l.path().join("thing"), b"file").unwrap();
        fs::create_dir(r.path().join("thing")).unwrap();
        assert_eq!(run(l.path(), r.path()), vec![("thing".to_string(), Status::Differ)]);
    }

    #[test]
    fn cancelling_stops_the_walk() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        fs::write(l.path().join("a"), b"x").unwrap();
        let cancel = AtomicBool::new(true);
        let d = compare(l.path(), r.path(), &cancel);
        assert!(d.cancelled);
    }
}
