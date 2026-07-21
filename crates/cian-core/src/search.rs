//! Recursive search from a directory downwards.
//!
//! Written to stream and to stop: a search over a deep tree can take a long
//! time and may be started by mistake, so results are handed back as they are
//! found and a cancel flag is checked at every step. Nothing here blocks on a
//! whole traversal before producing anything.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// One match.
#[derive(Debug, Clone)]
pub struct Hit {
    /// Absolute path to the entry.
    pub path: PathBuf,
    /// Path relative to the search root, which is what a result list wants to
    /// show — the absolute one is mostly the root repeated.
    pub rel: PathBuf,
    pub is_dir: bool,
    /// For a content match: the 1-based line number and the line itself.
    pub line: Option<(usize, String)>,
}

/// What a search looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Match entry names.
    Name,
    /// Match the text inside files.
    Content,
}

/// What to look for.
#[derive(Debug, Clone)]
pub struct Query {
    /// Matched case-insensitively.
    pub needle: String,
    /// Descend into directories whose name starts with a dot.
    pub include_hidden: bool,
    pub mode: Mode,
}

impl Query {
    pub fn new(needle: impl Into<String>) -> Self {
        Self { needle: needle.into().to_lowercase(), include_hidden: false, mode: Mode::Name }
    }

    pub fn content(needle: impl Into<String>) -> Self {
        Self { mode: Mode::Content, ..Self::new(needle) }
    }

    fn matches(&self, name: &str) -> bool {
        self.needle.is_empty() || name.to_lowercase().contains(&self.needle)
    }
}

/// Files larger than this are not read looking for text. A grep that stalls on
/// a database dump or a disk image is worse than one that admits it skipped it.
pub const MAX_GREP_BYTES: u64 = 8 * 1024 * 1024;

/// How much of a file to sniff for NUL bytes before deciding it is binary.
const SNIFF: usize = 8000;

/// Report every line of `path` containing the needle.
///
/// Skips files that are too big, and files that look binary — matching inside
/// a compiled object produces unreadable "lines" and no useful answer.
fn grep_file(
    path: &Path,
    needle: &str,
    cancel: &AtomicBool,
    mut on_line: impl FnMut(usize, String),
) {
    let Ok(meta) = fs::metadata(path) else { return };
    if meta.len() > MAX_GREP_BYTES {
        return;
    }
    let Ok(bytes) = fs::read(path) else { return };
    if bytes[..bytes.len().min(SNIFF)].contains(&0) {
        return;
    }
    let Ok(text) = String::from_utf8(bytes) else { return };
    for (i, line) in text.lines().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if line.to_lowercase().contains(needle) {
            // Long lines are useless in a result list and expensive to carry.
            let shown: String = line.trim().chars().take(300).collect();
            on_line(i + 1, shown);
        }
    }
}

/// Stop before the result list becomes useless to scroll and the search
/// pointless to continue.
pub const MAX_HITS: usize = 5000;

/// How a search ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Complete,
    Cancelled,
    /// Stopped at [`MAX_HITS`].
    Truncated,
}

/// Walk `root` breadth-first, reporting matches through `on_hit`.
///
/// Breadth-first on purpose: shallow matches are usually the wanted ones, and
/// they arrive first, so a search that is going to be cancelled early still
/// produces the useful results.
pub fn search(
    root: &Path,
    query: &Query,
    cancel: &AtomicBool,
    on_hit: &mut dyn FnMut(Hit),
) -> Outcome {
    let mut queue = std::collections::VecDeque::from([root.to_path_buf()]);
    let mut found = 0usize;
    while let Some(dir) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            return Outcome::Cancelled;
        }
        // An unreadable directory is normal (permissions); skipping it beats
        // aborting the whole search.
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if cancel.load(Ordering::Relaxed) {
                return Outcome::Cancelled;
            }
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let hidden = name.starts_with('.');
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);

            let visible = query.include_hidden || !hidden;
            match query.mode {
                Mode::Name => {
                    if visible && query.matches(&name) {
                        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                        on_hit(Hit { path: path.clone(), rel, is_dir, line: None });
                        found += 1;
                        if found >= MAX_HITS {
                            return Outcome::Truncated;
                        }
                    }
                }
                Mode::Content => {
                    if visible && !is_dir && !query.needle.is_empty() {
                        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                        let mut stop = false;
                        grep_file(&path, &query.needle, cancel, |n, text| {
                            if found < MAX_HITS {
                                on_hit(Hit {
                                    path: path.clone(),
                                    rel: rel.clone(),
                                    is_dir: false,
                                    line: Some((n, text)),
                                });
                                found += 1;
                            } else {
                                stop = true;
                            }
                        });
                        if stop {
                            return Outcome::Truncated;
                        }
                    }
                }
            }
            // Symlinked directories are not followed: a link back up the tree
            // would loop forever.
            if is_dir && !path.is_symlink() && (query.include_hidden || !hidden) {
                queue.push_back(path);
            }
        }
    }
    Outcome::Complete
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("src/deep/deeper")).unwrap();
        fs::create_dir_all(d.path().join(".hidden")).unwrap();
        fs::write(d.path().join("readme.md"), b"").unwrap();
        fs::write(d.path().join("src/main.rs"), b"").unwrap();
        fs::write(d.path().join("src/deep/notes.md"), b"").unwrap();
        fs::write(d.path().join("src/deep/deeper/main.rs"), b"").unwrap();
        fs::write(d.path().join(".hidden/secret.md"), b"").unwrap();
        d
    }

    fn run(root: &Path, q: Query) -> (Vec<String>, Outcome) {
        let cancel = AtomicBool::new(false);
        let mut names = Vec::new();
        let out = search(root, &q, &cancel, &mut |h| {
            names.push(h.rel.display().to_string().replace('\\', "/"))
        });
        (names, out)
    }

    #[test]
    fn finds_matches_at_every_depth() {
        let d = tree();
        let (mut names, out) = run(d.path(), Query::new("main"));
        names.sort();
        assert_eq!(out, Outcome::Complete);
        assert_eq!(names, vec!["src/deep/deeper/main.rs", "src/main.rs"]);
    }

    #[test]
    fn matching_is_case_insensitive() {
        let d = tree();
        let (names, _) = run(d.path(), Query::new("README"));
        assert_eq!(names, vec!["readme.md"]);
    }

    /// Shallow results arrive first, so cancelling early still leaves the
    /// most likely matches on screen.
    #[test]
    fn results_come_back_shallowest_first() {
        let d = tree();
        let (names, _) = run(d.path(), Query::new(".md"));
        let depth = |s: &String| s.matches('/').count();
        assert!(
            names.windows(2).all(|w| depth(&w[0]) <= depth(&w[1])),
            "not breadth-first: {:?}",
            names
        );
    }

    #[test]
    fn hidden_directories_are_skipped_unless_asked_for() {
        let d = tree();
        let (names, _) = run(d.path(), Query::new("secret"));
        assert!(names.is_empty(), "should not descend into .hidden: {:?}", names);

        let q = Query { include_hidden: true, ..Query::new("secret") };
        let (names, _) = run(d.path(), q);
        assert_eq!(names, vec![".hidden/secret.md"]);
    }

    #[test]
    fn directories_are_matched_too() {
        let d = tree();
        let (names, _) = run(d.path(), Query::new("deeper"));
        assert_eq!(names, vec!["src/deep/deeper"]);
    }

    #[test]
    fn cancelling_stops_the_walk() {
        let d = tree();
        let cancel = AtomicBool::new(true);
        let mut n = 0;
        let out = search(d.path(), &Query::new(""), &cancel, &mut |_| n += 1);
        assert_eq!(out, Outcome::Cancelled);
        assert_eq!(n, 0, "already cancelled: nothing should be walked");
    }

    #[test]
    fn an_unreadable_directory_does_not_abort_the_search() {
        let d = tree();
        // A path that vanishes mid-walk behaves like an unreadable one.
        let ghost = d.path().join("ghost");
        fs::create_dir(&ghost).unwrap();
        fs::remove_dir(&ghost).unwrap();
        let (names, out) = run(d.path(), Query::new("main"));
        assert_eq!(out, Outcome::Complete);
        assert_eq!(names.len(), 2);
    }
}

#[cfg(test)]
mod grep_tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("src")).unwrap();
        fs::write(d.path().join("a.txt"), "alpha\nTODO: fix this\nomega\n").unwrap();
        fs::write(d.path().join("src/b.rs"), "fn main() {}\n// todo later\n").unwrap();
        fs::write(d.path().join("clean.txt"), "nothing here\n").unwrap();
        // A binary file that contains the needle as bytes.
        fs::write(d.path().join("blob.bin"), b"\x00\x01todo\x00\xff").unwrap();
        d
    }

    fn run(root: &Path, needle: &str) -> Vec<(String, usize, String)> {
        let cancel = AtomicBool::new(false);
        let mut out = Vec::new();
        search(root, &Query::content(needle), &cancel, &mut |h| {
            let (n, text) = h.line.clone().unwrap();
            out.push((h.rel.display().to_string().replace('\\', "/"), n, text));
        });
        out.sort();
        out
    }

    #[test]
    fn finds_matching_lines_with_their_numbers() {
        let d = tree();
        let hits = run(d.path(), "todo");
        assert_eq!(
            hits,
            vec![
                ("a.txt".to_string(), 2, "TODO: fix this".to_string()),
                ("src/b.rs".to_string(), 2, "// todo later".to_string()),
            ],
            "case-insensitive, line-numbered, and recursive"
        );
    }

    /// Matching inside a compiled artefact yields unreadable "lines" and never
    /// answers the question that was asked.
    #[test]
    fn binary_files_are_skipped() {
        let d = tree();
        let hits = run(d.path(), "todo");
        assert!(!hits.iter().any(|(p, _, _)| p.contains("blob")), "{:?}", hits);
    }

    #[test]
    fn files_over_the_size_limit_are_skipped() {
        let d = tempfile::tempdir().unwrap();
        let big = d.path().join("huge.log");
        let mut text = String::with_capacity(MAX_GREP_BYTES as usize + 64);
        while text.len() < MAX_GREP_BYTES as usize + 32 {
            text.push_str("filler line\n");
        }
        text.push_str("needle here\n");
        fs::write(&big, &text).unwrap();
        assert!(fs::metadata(&big).unwrap().len() > MAX_GREP_BYTES);
        assert!(run(d.path(), "needle").is_empty(), "should not read a huge file");
    }

    #[test]
    fn an_empty_needle_matches_nothing_rather_than_every_line() {
        let d = tree();
        assert!(run(d.path(), "").is_empty());
    }

    #[test]
    fn a_name_search_still_reports_no_line() {
        let d = tree();
        let cancel = AtomicBool::new(false);
        let mut lines = Vec::new();
        search(d.path(), &Query::new("a.txt"), &cancel, &mut |h| lines.push(h.line.clone()));
        assert_eq!(lines, vec![None]);
    }
}
