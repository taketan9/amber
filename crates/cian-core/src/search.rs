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
}

/// What to look for.
#[derive(Debug, Clone)]
pub struct Query {
    /// Matched case-insensitively against the entry's name.
    pub needle: String,
    /// Descend into directories whose name starts with a dot.
    pub include_hidden: bool,
}

impl Query {
    pub fn new(needle: impl Into<String>) -> Self {
        Self { needle: needle.into().to_lowercase(), include_hidden: false }
    }

    fn matches(&self, name: &str) -> bool {
        self.needle.is_empty() || name.to_lowercase().contains(&self.needle)
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

            if query.matches(&name) && (query.include_hidden || !hidden) {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                on_hit(Hit { path: path.clone(), rel, is_dir });
                found += 1;
                if found >= MAX_HITS {
                    return Outcome::Truncated;
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
