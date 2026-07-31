//! Comparing two directory trees and listing what differs.
//!
//! The question this answers is "which files and folders are not the same
//! between these two trees" — not *how* their contents differ. Two files are
//! the same when their sizes match and, if they do, their bytes match: an
//! accurate compare that never depends on timestamps. Reading same-sized files
//! makes it heavier than a stat-only scan, so it reports progress and can be
//! cancelled.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use crate::progress::Progress;

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
///
/// `on_progress` is called as the walk advances, with `files_done`/`files_total`
/// counting entries examined — a first stat-only pass counts the total so the
/// bar can show a real percentage.
pub fn compare(
    left: &Path,
    right: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
) -> DirDiff {
    let total = count_entries(left, right, Path::new(""), cancel);
    let mut out = DirDiff::default();
    let mut done = 0usize;
    let mut last_report = 0usize;
    walk(left, right, Path::new(""), cancel, total, &mut done, &mut last_report, on_progress, &mut out);
    out.entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// Stat-only first pass: how many union entries the comparison will examine.
fn count_entries(left: &Path, right: &Path, rel: &Path, cancel: &AtomicBool) -> usize {
    if cancel.load(Ordering::Relaxed) {
        return 0;
    }
    let ln = names(&left.join(rel));
    let rn = names(&right.join(rel));
    let all: BTreeSet<&String> = ln.iter().chain(rn.iter()).collect();
    let mut n = 0;
    for name in all {
        n += 1;
        let child = rel.join(name);
        let l = fs::symlink_metadata(left.join(&child)).ok();
        let r = fs::symlink_metadata(right.join(&child)).ok();
        if matches!((&l, &r), (Some(a), Some(b)) if a.is_dir() && b.is_dir()) {
            n += count_entries(left, right, &child, cancel);
        }
    }
    n
}

#[allow(clippy::too_many_arguments)]
fn walk(
    left: &Path,
    right: &Path,
    rel: &Path,
    cancel: &AtomicBool,
    total: usize,
    done: &mut usize,
    last_report: &mut usize,
    on_progress: &mut dyn FnMut(&Progress),
    out: &mut DirDiff,
) {
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
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            return;
        }
        if out.entries.len() >= MAX_ENTRIES {
            out.truncated = true;
            return;
        }
        let child = rel.join(name);
        let lp = left.join(&child);
        let rp = right.join(&child);
        let lm = fs::symlink_metadata(&lp).ok();
        let rm = fs::symlink_metadata(&rp).ok();

        *done += 1;
        // Report at most every 16 entries so the channel is not flooded.
        if *done - *last_report >= 16 {
            *last_report = *done;
            let p = Progress {
                files_done: *done,
                files_total: total,
                current: child.display().to_string(),
                ..Default::default()
            };
            on_progress(&p);
        }

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
                    walk(left, right, &child, cancel, total, done, last_report, on_progress, out);
                } else if l.is_dir() != r.is_dir() {
                    // One a directory, the other a file: a real difference.
                    out.entries.push(Entry { rel: child, status: Status::Differ, is_dir: false });
                } else if files_differ(&lp, &rp, &l, &r, cancel) {
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

/// Two files differ when their sizes differ, or — at equal size — when their
/// bytes do. A file that cannot be read is treated as differing rather than
/// silently reported as identical.
fn files_differ(lp: &Path, rp: &Path, l: &fs::Metadata, r: &fs::Metadata, cancel: &AtomicBool) -> bool {
    if l.len() != r.len() {
        return true;
    }
    // An unreadable file is treated as differing rather than silently "same".
    contents_differ(lp, rp, cancel).unwrap_or(true)
}

/// Compare two same-sized files byte for byte, short-circuiting on the first
/// mismatch and checking the cancel flag between chunks.
fn contents_differ(lp: &Path, rp: &Path, cancel: &AtomicBool) -> std::io::Result<bool> {
    let mut lf = fs::File::open(lp)?;
    let mut rf = fs::File::open(rp)?;
    let mut lb = vec![0u8; 64 * 1024];
    let mut rb = vec![0u8; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(false); // caller will notice the cancel and discard
        }
        let ln = read_full(&mut lf, &mut lb)?;
        let rn = read_full(&mut rf, &mut rb)?;
        if ln != rn {
            return Ok(true);
        }
        if ln == 0 {
            return Ok(false); // both reached EOF together, all bytes equal
        }
        if lb[..ln] != rb[..rn] {
            return Ok(true);
        }
    }
}

/// Fill `buf` as far as possible, returning how many bytes were read (0 at EOF).
/// A short read from one file must not be mistaken for a difference.
fn read_full(f: &mut fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// Set a path's modification time (test helper, and handy for callers).
#[doc(hidden)]
pub fn set_mtime(path: &Path, t: SystemTime) -> std::io::Result<()> {
    fs::OpenOptions::new().write(true).open(path)?.set_modified(t)
}

// ── Exporting a folder comparison to a readable report ───────────────────────

fn counts(entries: &[Entry]) -> (usize, usize, usize) {
    let (mut m, mut a, mut d) = (0, 0, 0);
    for e in entries {
        match e.status {
            Status::Differ => m += 1,
            Status::OnlyRight => a += 1,
            Status::OnlyLeft => d += 1,
        }
    }
    (m, a, d)
}

fn rel_name(e: &Entry) -> String {
    let mut n = e.rel.display().to_string().replace('\\', "/");
    if e.is_dir {
        n.push('/');
    }
    n
}

/// Render the folder comparison as a self-contained HTML page: a left column
/// and a right column, so it is obvious which side each path is on — present on
/// the left only, the right only, or on both but differing.
pub fn to_html(entries: &[Entry], left: &str, right: &str, truncated: bool) -> String {
    use crate::diff::{html_escape, REPORT_STYLE};
    let (m, a, d) = counts(entries);
    let mut s = String::new();
    s.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    s.push_str(&format!("<title>compare: {} \u{2194} {}</title>\n", html_escape(left), html_escape(right)));
    s.push_str(REPORT_STYLE);
    s.push_str("</head>\n<body>\n");
    s.push_str(&format!(
        "<h1>{} <span class=\"arrow\">\u{2194}</span> {}</h1>\n",
        html_escape(left),
        html_escape(right)
    ));
    let cut = if truncated { "  (stopped at 5000)" } else { "" };
    s.push_str(&format!("<p class=\"summary\">~{} differ · +{} right-only · -{} left-only{}</p>\n", m, a, d, cut));
    s.push_str("<table>\n<thead><tr><th>");
    s.push_str(&html_escape(left));
    s.push_str("</th><th class=\"num\">\u{0394}</th><th>");
    s.push_str(&html_escape(right));
    s.push_str("</th></tr></thead>\n<tbody>\n");
    for e in entries {
        let name = html_escape(&rel_name(e));
        let (cls, l, mid, r) = match e.status {
            Status::OnlyLeft => ("del", name.as_str(), "\u{25c0}", ""),
            Status::OnlyRight => ("add", "", "\u{25b6}", name.as_str()),
            Status::Differ => ("chg", name.as_str(), "\u{2260}", name.as_str()),
        };
        let cell = |t: &str| if t.is_empty() {
            "<td class=\"empty\"></td>".to_string()
        } else {
            format!("<td class=\"code\">{}</td>", t)
        };
        s.push_str(&format!(
            "<tr class=\"{}\">{}<td class=\"num\">{}</td>{}</tr>\n",
            cls, cell(l), mid, cell(r)
        ));
    }
    s.push_str("</tbody>\n</table>\n</body>\n</html>\n");
    s
}

/// Render the folder comparison as a Markdown table: a left column and a right
/// column, the middle marking `-` left-only, `+` right-only, `~` differ.
pub fn to_markdown(entries: &[Entry], left: &str, right: &str, truncated: bool) -> String {
    use crate::diff::md_code;
    let (m, a, d) = counts(entries);
    let mut s = String::new();
    s.push_str(&format!("# compare: {} \u{2194} {}\n\n", left, right));
    let cut = if truncated { "  (stopped at 5000)" } else { "" };
    s.push_str(&format!("`~{} differ  +{} right-only  -{} left-only{}`\n\n", m, a, d, cut));
    let h = |x: &str| x.replace('|', "\\|");
    s.push_str(&format!("| {} |   | {} |\n", h(left), h(right)));
    s.push_str("|---|:-:|---|\n");
    for e in entries {
        let name = rel_name(e);
        let (st, l, r) = match e.status {
            Status::OnlyLeft => ("-", md_code(&name), " ".to_string()),
            Status::OnlyRight => ("+", " ".to_string(), md_code(&name)),
            Status::Differ => ("~", md_code(&name), md_code(&name)),
        };
        s.push_str(&format!("| {} | {} | {} |\n", l, st, r));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn run(l: &Path, r: &Path) -> Vec<(String, Status)> {
        let cancel = AtomicBool::new(false);
        compare(l, r, &cancel, &mut |_| {})
            .entries
            .into_iter()
            .map(|e| (e.rel.display().to_string().replace('\\', "/"), e.status))
            .collect()
    }

    fn sample_entries() -> Vec<Entry> {
        vec![
            Entry { rel: PathBuf::from("only_left.txt"), status: Status::OnlyLeft, is_dir: false },
            Entry { rel: PathBuf::from("only_right.txt"), status: Status::OnlyRight, is_dir: false },
            Entry { rel: PathBuf::from("changed.txt"), status: Status::Differ, is_dir: false },
        ]
    }

    #[test]
    fn html_export_puts_each_path_on_its_side() {
        let html = to_html(&sample_entries(), "LEFT", "RIGHT", false);
        assert!(html.contains("<table"));
        assert!(html.contains("only_left.txt") && html.contains("only_right.txt"));
        // A left-only row is styled as a deletion, a right-only as an addition,
        // a differing one as a change — and the changed path appears twice
        // (once per side).
        assert!(html.contains("class=\"del\"") && html.contains("class=\"add\"") && html.contains("class=\"chg\""));
        assert_eq!(html.matches("changed.txt").count(), 2, "differing path on both sides");
    }

    #[test]
    fn markdown_export_is_a_left_right_table() {
        let md = to_markdown(&sample_entries(), "LEFT", "RIGHT", false);
        assert!(md.starts_with("# compare: LEFT \u{2194} RIGHT"));
        assert!(md.contains("| LEFT |   | RIGHT |"), "header names both sides: {}", md);
        assert!(md.contains("only_left.txt") && md.contains("only_right.txt"));
    }

    #[test]
    fn identical_trees_report_nothing_regardless_of_mtime() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        for d in [l.path(), r.path()] {
            fs::create_dir(d.join("sub")).unwrap();
            fs::write(d.join("a.txt"), b"hello").unwrap();
            fs::write(d.join("sub/b.txt"), b"world").unwrap();
        }
        // Deliberately give one side a different mtime: content compare ignores
        // timestamps, so the trees are still identical.
        set_mtime(&l.path().join("a.txt"), SystemTime::UNIX_EPOCH + Duration::from_secs(5)).unwrap();
        let cancel = AtomicBool::new(false);
        assert!(compare(l.path(), r.path(), &cancel, &mut |_| {}).is_identical());
    }

    #[test]
    fn same_size_but_different_bytes_is_a_diff() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        fs::write(l.path().join("f.txt"), b"aaaaa").unwrap();
        fs::write(r.path().join("f.txt"), b"aaaab").unwrap(); // same length, last byte differs
        assert_eq!(run(l.path(), r.path()), vec![("f.txt".to_string(), Status::Differ)]);
    }

    #[test]
    fn progress_is_reported() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        for i in 0..40 {
            fs::write(l.path().join(format!("f{i}")), b"x").unwrap();
            fs::write(r.path().join(format!("f{i}")), b"x").unwrap();
        }
        let cancel = AtomicBool::new(false);
        let mut seen_total = 0;
        compare(l.path(), r.path(), &cancel, &mut |p| seen_total = p.files_total);
        assert!(seen_total >= 40, "a total was reported: {}", seen_total);
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
    fn same_size_same_bytes_different_mtime_is_not_a_diff() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        fs::write(l.path().join("f.txt"), b"12345").unwrap();
        fs::write(r.path().join("f.txt"), b"12345").unwrap();
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        set_mtime(&l.path().join("f.txt"), base).unwrap();
        set_mtime(&r.path().join("f.txt"), base + Duration::from_secs(60)).unwrap();
        assert!(run(l.path(), r.path()).is_empty(), "content is equal, timestamps ignored");
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
        let d = compare(l.path(), r.path(), &cancel, &mut |_| {});
        assert!(d.cancelled);
    }
}
