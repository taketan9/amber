//! A thin layer over the `git` command line.
//!
//! cian shows git state next to files and can stage/unstage/discard them. It
//! shells out to `git` rather than linking libgit2 (a C dependency that would
//! break the single self-contained binary) or pulling in a large pure-Rust
//! implementation: every developer already has `git` on PATH, and a missing or
//! non-repo directory simply yields `None`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// One file's two-letter porcelain status: the index (staged) side and the
/// worktree (unstaged) side, each a git status code (`M`, `A`, `D`, `?`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileState {
    pub index: char,
    pub worktree: char,
}

/// A coarse category for colouring, collapsing the XY codes into what a glance
/// needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitMark {
    /// Staged (index has a change): green.
    Staged,
    /// Modified in the worktree (unstaged): yellow.
    Modified,
    /// Not tracked: blue-grey.
    Untracked,
    /// Merge conflict: red.
    Conflict,
    /// A directory that contains changes below it.
    DirDirty,
}

impl GitMark {
    /// The one/two-character badge shown before the name.
    pub fn badge(self) -> &'static str {
        match self {
            GitMark::Staged => "●",
            GitMark::Modified => "✚",
            GitMark::Untracked => "?",
            GitMark::Conflict => "‼",
            GitMark::DirDirty => "~",
        }
    }
}

impl FileState {
    /// The category to colour by. Conflict and staged win over a plain worktree
    /// change, matching how you think about a file's state.
    pub fn mark(self) -> GitMark {
        match (self.index, self.worktree) {
            ('?', '?') => GitMark::Untracked,
            ('U', _) | (_, 'U') | ('D', 'D') | ('A', 'A') => GitMark::Conflict,
            (i, _) if i != ' ' && i != '?' => GitMark::Staged,
            _ => GitMark::Modified,
        }
    }

    /// Synthesize codes that yield `mark` — so a backend that speaks GitMark
    /// (svn) can build a [`RepoStatus`].
    fn from_mark(mark: GitMark) -> Self {
        let (index, worktree) = match mark {
            GitMark::Staged | GitMark::DirDirty => ('M', ' '),
            GitMark::Modified => (' ', 'M'),
            GitMark::Untracked => ('?', '?'),
            GitMark::Conflict => ('U', 'U'),
        };
        FileState { index, worktree }
    }
}

/// Everything cian shows about a repository the pane sits in.
#[derive(Debug, Clone)]
pub struct RepoStatus {
    pub root: PathBuf,
    /// Branch name, or a short hash / "(detached)" when there is no branch.
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    /// Normalised-path → its status. Directories that contain changes are keyed
    /// in `dirty_dirs`.
    files: HashMap<String, FileState>,
    /// Normalised paths of directories that contain a change below them.
    dirty_dirs: HashSet<String>,
}

/// Normalise an absolute path for cross-platform map keys: forward slashes, and
/// lower-cased on Windows (its filesystem is case-insensitive). This absorbs the
/// separator and case differences between `git`'s output and cian's paths.
fn norm(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('\\', "/");
    // Drop the Windows verbatim prefix (`\\?\C:\…` → `C:/…`) so a canonicalised
    // path lines up with git's plain output.
    if let Some(rest) = s.strip_prefix("//?/") {
        s = rest.to_string();
    }
    let s = s.trim_end_matches('/').to_string();
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

impl RepoStatus {
    /// The mark for `path` (a file's own state, or "contains changes" for a
    /// directory), if any.
    pub fn mark_for(&self, path: &Path) -> Option<GitMark> {
        let key = norm(path);
        if let Some(st) = self.files.get(&key) {
            return Some(st.mark());
        }
        if self.dirty_dirs.contains(&key) {
            return Some(GitMark::DirDirty);
        }
        None
    }

    /// Number of files with any change (staged or not, excluding untracked).
    pub fn changed_count(&self) -> usize {
        self.files
            .values()
            .filter(|s| s.mark() != GitMark::Untracked)
            .count()
    }

    /// Build a status from precomputed per-file marks — used by the SVN backend,
    /// which shares GitMark and this display type. `label` fills the "branch"
    /// slot (e.g. `"svn r123"`). Containing directories are tainted automatically.
    pub fn from_marks(
        root: PathBuf,
        label: String,
        marks: impl IntoIterator<Item = (PathBuf, GitMark)>,
    ) -> Self {
        let mut files = HashMap::new();
        let mut dirty_dirs = HashSet::new();
        for (abs, mark) in marks {
            let mut cur = abs.parent();
            while let Some(d) = cur {
                if !d.starts_with(&root) && d != root {
                    break;
                }
                dirty_dirs.insert(norm(d));
                if d == root {
                    break;
                }
                cur = d.parent();
            }
            files.insert(norm(&abs), FileState::from_mark(mark));
        }
        RepoStatus { root, branch: label, ahead: 0, behind: 0, files, dirty_dirs }
    }
}

/// Run `git` in `dir`, returning its stdout as bytes on a zero exit. `None` on
/// any failure (git missing, not a repo, non-zero exit) so callers can treat
/// "no git here" uniformly.
fn git_output(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = crate::proc::quiet("git")
        .arg("-C")
        .arg(dir)
        // Never let a user's pager or prompts hang a captured command.
        .env("GIT_PAGER", "cat")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .output()
        .ok()?;
    if out.status.success() {
        Some(out.stdout)
    } else {
        None
    }
}

/// The status of the repository `dir` sits in, or `None` if it is not in one.
pub fn status(dir: &Path) -> Option<RepoStatus> {
    let root_out = git_output(dir, &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(String::from_utf8_lossy(&root_out).trim());

    let raw = git_output(dir, &["status", "--porcelain=v1", "--branch", "-z"])?;
    let text = String::from_utf8_lossy(&raw);
    let mut fields = text.split('\0');

    let mut branch = String::from("(unknown)");
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut files: HashMap<String, FileState> = HashMap::new();
    let mut dirty_dirs: HashSet<String> = HashSet::new();

    while let Some(field) = fields.next() {
        if field.is_empty() {
            continue;
        }
        if let Some(rest) = field.strip_prefix("## ") {
            let (b, a, be) = parse_branch_line(rest);
            branch = b;
            ahead = a;
            behind = be;
            continue;
        }
        // Ordinary entry: "XY <path>". A rename/copy (X in R/C) is followed by
        // the original path in the next NUL field, which we skip.
        let bytes = field.as_bytes();
        if bytes.len() < 4 {
            continue;
        }
        let x = field.chars().next().unwrap_or(' ');
        let y = field.chars().nth(1).unwrap_or(' ');
        let path = field[3..].to_string();
        if x == 'R' || x == 'C' {
            let _ = fields.next(); // consume the original path
        }
        let abs = root.join(&path);
        // A change taints every directory between the repo root and the file,
        // so a folder in the listing can flag that it holds changes.
        let mut cur = abs.parent();
        while let Some(d) = cur {
            if !d.starts_with(&root) && d != root {
                break;
            }
            dirty_dirs.insert(norm(d));
            if d == root {
                break;
            }
            cur = d.parent();
        }
        files.insert(norm(&abs), FileState { index: x, worktree: y });
    }

    Some(RepoStatus { root, branch, ahead, behind, files, dirty_dirs })
}

/// Parse the porcelain branch header body (everything after `## `).
/// Examples: `main...origin/main [ahead 2, behind 1]`, `main`,
/// `No commits yet on trunk` (an unborn branch), `HEAD (no branch)`.
fn parse_branch_line(rest: &str) -> (String, u32, u32) {
    // A brand-new repo with no commits: "No commits yet on <branch>".
    if let Some(name) = rest.strip_prefix("No commits yet on ") {
        let branch = name.split_whitespace().next().unwrap_or("").to_string();
        return (if branch.is_empty() { "(unborn)".into() } else { branch }, 0, 0);
    }
    let name_part = rest.split("...").next().unwrap_or(rest);
    let branch = name_part.split_whitespace().next().unwrap_or("").to_string();
    let mut ahead = 0;
    let mut behind = 0;
    if let Some(inside) = rest.split_once('[').and_then(|(_, r)| r.split_once(']')).map(|(a, _)| a) {
        for part in inside.split(',') {
            let p = part.trim();
            if let Some(n) = p.strip_prefix("ahead ") {
                ahead = n.trim().parse().unwrap_or(0);
            } else if let Some(n) = p.strip_prefix("behind ") {
                behind = n.trim().parse().unwrap_or(0);
            }
        }
    }
    let branch = if branch.is_empty() { "(detached)".to_string() } else { branch };
    (branch, ahead, behind)
}

/// The staged diff (`git diff --cached`), or `None` if not in a repo. An empty
/// string means "in a repo, but nothing staged".
pub fn staged_diff(dir: &Path) -> Option<String> {
    // `--no-color` so the AI sees plain text, not ANSI escapes.
    let out = git_output(dir, &["diff", "--cached", "--no-color"])?;
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// A short summary of what is staged (`git diff --cached --stat`), for showing
/// the user which files a generated message covers.
pub fn staged_stat(dir: &Path) -> Option<String> {
    let out = git_output(dir, &["diff", "--cached", "--stat", "--no-color"])?;
    Some(String::from_utf8_lossy(&out).trim_end().to_string())
}

/// One commit in a log listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Short hash.
    pub hash: String,
    /// Author date, `YYYY-MM-DD`.
    pub date: String,
    pub author: String,
    pub subject: String,
}

/// Recent commits, newest first (up to `limit`). `path`, when given, limits the
/// log to commits that touched that file.
pub fn log(dir: &Path, path: Option<&Path>, limit: usize) -> Vec<Commit> {
    let n = format!("-{}", limit.max(1));
    // Unit-separator (0x1f) between fields survives any subject text.
    let mut args: Vec<String> = vec![
        "log".into(),
        "--no-color".into(),
        "--date=short".into(),
        "--pretty=format:%h\u{1f}%ad\u{1f}%an\u{1f}%s".into(),
        n,
    ];
    let path_str;
    if let Some(path) = path {
        args.push("--".into());
        path_str = path.to_string_lossy().into_owned();
        args.push(path_str);
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let Some(out) = git_output(dir, &arg_refs) else { return Vec::new() };
    String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|line| {
            let mut f = line.split('\u{1f}');
            Some(Commit {
                hash: f.next()?.to_string(),
                date: f.next()?.to_string(),
                author: f.next()?.to_string(),
                subject: f.next().unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// The working-tree changes of `file` versus HEAD, as a unified diff (empty when
/// there are none).
pub fn file_diff(dir: &Path, file: &Path) -> Option<String> {
    let out = git_output(dir, &["diff", "HEAD", "--no-color", "--", &file.to_string_lossy()])?;
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// A commit shown as a unified diff (`git show <hash>`).
pub fn show(dir: &Path, hash: &str) -> Option<String> {
    let out = git_output(dir, &["show", "--no-color", hash])?;
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// One line's blame: short commit hash, author, and date.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlameLine {
    pub hash: String,
    pub author: String,
    pub date: String,
}

/// Per-line blame for `file`, one entry per line in order. `None` when the file
/// is not tracked or git failed. Uncommitted lines blame to a zero hash and
/// "Not Committed Yet".
pub fn blame(dir: &Path, file: &Path) -> Option<Vec<BlameLine>> {
    let out = git_output(dir, &["blame", "--line-porcelain", "--", &file.to_string_lossy()])?;
    let text = String::from_utf8_lossy(&out);
    let mut lines = Vec::new();
    let (mut hash, mut author, mut date) = (String::new(), String::new(), String::new());
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("author ") {
            author = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("author-time ") {
            if let Some(d) = rest.trim().parse::<i64>().ok().and_then(epoch_ymd) {
                date = d;
            }
        } else if line.starts_with('\t') {
            // The content line closes a group; commit the accumulated blame.
            lines.push(BlameLine { hash: hash.clone(), author: author.clone(), date: date.clone() });
        } else {
            // A group header: "<40-hex> <orig> <final> [count]".
            let first = line.split(' ').next().unwrap_or("");
            if first.len() >= 7 && first.bytes().all(|b| b.is_ascii_hexdigit()) {
                hash = first[..7].to_string(); // match `git log %h`
            }
        }
    }
    Some(lines)
}

/// A unix timestamp as `YYYY-MM-DD`.
fn epoch_ymd(ts: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(ts, 0).map(|d| d.format("%Y-%m-%d").to_string())
}

/// Commit the staged changes with `message`. Fails if nothing is staged or the
/// commit is rejected (e.g. a hook, or a missing identity); the git error text
/// is returned so it can be shown in-app. Output is captured, never printed, so
/// it cannot corrupt the TUI.
pub fn commit(dir: &Path, message: &str) -> Result<()> {
    let out = crate::proc::quiet("git")
        .arg("-C")
        .arg(dir)
        .args(["commit", "-m", message])
        .output()
        .context("run git commit")?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        let msg = if err.is_empty() {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            err.to_string()
        };
        anyhow::bail!("{}", if msg.is_empty() { "git commit failed".into() } else { msg })
    }
}

/// How a working-file line differs from its committed (HEAD) version, for the
/// viewer's change gutter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineChange {
    /// A line with no counterpart in HEAD.
    Added,
    /// A line that replaced a different one.
    Modified,
    /// One or more lines were deleted immediately before this one.
    DeletedBefore,
}

/// Per-line change status of `file` versus its committed version, keyed by
/// 0-based working-file line index. `None` when not in a repo or git is absent;
/// an empty map means the file matches HEAD (or is unmodified). An untracked or
/// brand-new file reports every line as [`LineChange::Added`].
pub fn line_changes(dir: &Path, file: &Path) -> Option<std::collections::HashMap<usize, LineChange>> {
    // Locate the file within its repo (git wants a repo-relative path).
    let root_out = git_output(dir, &["rev-parse", "--show-toplevel"])?;
    let root = PathBuf::from(String::from_utf8_lossy(&root_out).trim());
    let rel = norm_rel(&root, file)?;

    let work = std::fs::read_to_string(file).ok()?;
    let work_lines: Vec<String> = work.lines().map(|s| s.to_string()).collect();

    // The committed version. Absent (new/untracked file) → the whole file is new.
    let spec = format!("HEAD:{}", rel);
    let head = git_output(dir, &["show", &spec]);
    let mut map = std::collections::HashMap::new();
    let Some(head_bytes) = head else {
        for i in 0..work_lines.len() {
            map.insert(i, LineChange::Added);
        }
        return Some(map);
    };
    let head_text = String::from_utf8_lossy(&head_bytes);
    let head_lines: Vec<String> = head_text.lines().map(|s| s.to_string()).collect();
    changes_from_lines(&head_lines, &work_lines, &mut map);
    Some(map)
}

/// Diff `base` against `work` and record each working line's change into `map`.
/// Shared by the git and svn gutters (they differ only in how `base` is fetched).
pub(crate) fn changes_from_lines(
    base: &[String],
    work: &[String],
    map: &mut std::collections::HashMap<usize, LineChange>,
) {
    let d = crate::diff::diff_lines(base, work);
    let mut pending_del = false;
    for row in &d.rows {
        use crate::diff::Row;
        match row {
            Row::Changed { right, .. } => {
                map.insert(right.no.saturating_sub(1), LineChange::Modified);
                pending_del = false;
            }
            Row::Added { right } => {
                let mark = if pending_del { LineChange::Modified } else { LineChange::Added };
                map.insert(right.no.saturating_sub(1), mark);
                pending_del = false;
            }
            Row::Removed { .. } => pending_del = true,
            Row::Same { right, .. } => {
                if pending_del {
                    map.entry(right.no.saturating_sub(1)).or_insert(LineChange::DeletedBefore);
                    pending_del = false;
                }
            }
            Row::Skipped { .. } => {}
        }
    }
}

/// A repo-relative, forward-slashed path for `file` under `root`, or `None` if
/// `file` is not inside `root`.
fn norm_rel(root: &Path, file: &Path) -> Option<String> {
    let r = norm(root);
    let f = norm(file);
    let rest = f.strip_prefix(&r)?.trim_start_matches('/');
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

/// `git add` the given paths.
pub fn stage(dir: &Path, paths: &[PathBuf]) -> Result<()> {
    run_git(dir, "add", paths)
}

/// `git reset HEAD` the given paths (unstage, keeping worktree changes).
pub fn unstage(dir: &Path, paths: &[PathBuf]) -> Result<()> {
    let mut args: Vec<String> = vec!["reset".into(), "HEAD".into(), "--".into()];
    args.extend(paths.iter().map(|p| p.display().to_string()));
    run_git_args(dir, &args)
}

/// Throw away worktree changes to the given tracked paths (`git checkout --`).
/// Untracked files are left alone (git would not touch them anyway).
pub fn discard(dir: &Path, paths: &[PathBuf]) -> Result<()> {
    run_git(dir, "checkout", paths)
}

fn run_git(dir: &Path, verb: &str, paths: &[PathBuf]) -> Result<()> {
    let mut args: Vec<String> = vec![verb.into(), "--".into()];
    args.extend(paths.iter().map(|p| p.display().to_string()));
    run_git_args(dir, &args)
}

fn run_git_args(dir: &Path, args: &[String]) -> Result<()> {
    // Capture output rather than inheriting it: git writes progress to the
    // terminal (`git reset` prints "Unstaged changes after reset…"), which would
    // corrupt the TUI. On failure the captured stderr becomes the error text.
    let out = crate::proc::quiet("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .context("run git")?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        if err.is_empty() {
            anyhow::bail!("git {} failed", args.first().cloned().unwrap_or_default())
        } else {
            anyhow::bail!("{}", err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_line_parses_ahead_behind() {
        assert_eq!(parse_branch_line("main"), ("main".into(), 0, 0));
        assert_eq!(
            parse_branch_line("main...origin/main [ahead 2, behind 1]"),
            ("main".into(), 2, 1)
        );
        assert_eq!(
            parse_branch_line("feature/x...origin/x [ahead 3]"),
            ("feature/x".into(), 3, 0)
        );
        assert_eq!(parse_branch_line("HEAD (no branch)"), ("HEAD".into(), 0, 0));
        // A fresh repo before its first commit.
        assert_eq!(parse_branch_line("No commits yet on trunk"), ("trunk".into(), 0, 0));
    }

    #[test]
    fn norm_strips_the_windows_verbatim_prefix() {
        // A canonicalised Windows path (`\\?\C:\…`) must key the same as git's
        // plain `C:/…` output.
        assert_eq!(norm(Path::new(r"\\?\C:\a\b")), norm(Path::new(r"C:\a\b")));
        assert_eq!(norm(Path::new("/a/b/")), norm(Path::new("/a/b")));
    }

    #[test]
    fn file_state_categories() {
        assert_eq!(FileState { index: '?', worktree: '?' }.mark(), GitMark::Untracked);
        assert_eq!(FileState { index: 'M', worktree: ' ' }.mark(), GitMark::Staged);
        assert_eq!(FileState { index: ' ', worktree: 'M' }.mark(), GitMark::Modified);
        assert_eq!(FileState { index: 'U', worktree: 'U' }.mark(), GitMark::Conflict);
        // Staged AND then modified again: the staged side wins the colour.
        assert_eq!(FileState { index: 'M', worktree: 'M' }.mark(), GitMark::Staged);
    }

    /// End-to-end against a real throwaway repo, skipped when git is absent.
    /// Deliberately needs no commit — `git commit` depends on an identity and
    /// signing config that vary across CI runners, whereas add/untracked do not.
    #[test]
    fn status_of_a_real_repo() {
        let d = tempfile::tempdir().unwrap();
        // Canonicalise so paths match `git rev-parse --show-toplevel` (which
        // resolves symlinks like macOS's /var → /private/var).
        let dir = &std::fs::canonicalize(d.path()).unwrap();
        let init_ok = crate::proc::quiet("git")
            .arg("-C")
            .arg(dir)
            .args(["init", "-q"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !init_ok {
            eprintln!("git not available; skipping");
            return;
        }
        std::fs::write(dir.join("staged.txt"), "x\n").unwrap();
        std::fs::write(dir.join("untracked.txt"), "y\n").unwrap();
        assert!(stage(dir, &[dir.join("staged.txt")]).is_ok(), "git add");

        let st = status(dir).expect("in a repo");
        assert_eq!(st.mark_for(&dir.join("staged.txt")), Some(GitMark::Staged));
        assert_eq!(st.mark_for(&dir.join("untracked.txt")), Some(GitMark::Untracked));
        assert_eq!(st.mark_for(&dir.join("does-not-exist")), None);

        // Unstage puts it back to untracked (it was never committed).
        unstage(dir, &[dir.join("staged.txt")]).unwrap();
        let st = status(dir).unwrap();
        assert_eq!(st.mark_for(&dir.join("staged.txt")), Some(GitMark::Untracked));
    }

    /// Per-line change status against HEAD, for the F3 viewer's git lens.
    #[test]
    fn line_changes_classify_added_modified_and_deletions() {
        let d = tempfile::tempdir().unwrap();
        let dir = &std::fs::canonicalize(d.path()).unwrap();
        let init_ok = crate::proc::quiet("git").arg("-C").arg(dir).args(["init", "-q"]).status()
            .map(|s| s.success()).unwrap_or(false);
        if !init_ok {
            eprintln!("git not available; skipping");
            return;
        }
        for kv in [["user.email", "t@e.com"], ["user.name", "T"], ["core.autocrlf", "false"]] {
            let _ = crate::proc::quiet("git").arg("-C").arg(dir).args(["config", kv[0], kv[1]]).status();
        }
        let f = dir.join("f.txt");
        std::fs::write(&f, "a\nb\nc\nd\n").unwrap();
        crate::proc::quiet("git").arg("-C").arg(dir).args(["add", "."]).status().unwrap();
        crate::proc::quiet("git").arg("-C").arg(dir).args(["commit", "-qm", "init"]).status().unwrap();

        // Modify line 2, delete line 3, append a new line.
        std::fs::write(&f, "a\nB\nd\ne\n").unwrap();
        let m = line_changes(dir, &f).expect("in a repo");
        assert_eq!(m.get(&0), None, "line 1 unchanged");
        assert_eq!(m.get(&1), Some(&LineChange::Modified), "line 2 modified");
        assert_eq!(m.get(&3), Some(&LineChange::Added), "the appended line is new");
        // A brand-new untracked file: every line is added.
        let n = dir.join("new.txt");
        std::fs::write(&n, "x\ny\n").unwrap();
        let mn = line_changes(dir, &n).unwrap();
        assert_eq!(mn.get(&0), Some(&LineChange::Added));
        assert_eq!(mn.get(&1), Some(&LineChange::Added));
    }

    #[test]
    fn log_blame_and_file_diff() {
        let d = tempfile::tempdir().unwrap();
        let dir = &std::fs::canonicalize(d.path()).unwrap();
        let init_ok = crate::proc::quiet("git").arg("-C").arg(dir).args(["init", "-q"]).status()
            .map(|s| s.success()).unwrap_or(false);
        if !init_ok {
            eprintln!("git not available; skipping");
            return;
        }
        for kv in [["user.email", "t@e.com"], ["user.name", "Alice"], ["core.autocrlf", "false"]] {
            let _ = crate::proc::quiet("git").arg("-C").arg(dir).args(["config", kv[0], kv[1]]).status();
        }
        let f = dir.join("f.txt");
        std::fs::write(&f, "one\ntwo\n").unwrap();
        crate::proc::quiet("git").arg("-C").arg(dir).args(["add", "."]).status().unwrap();
        crate::proc::quiet("git").arg("-C").arg(dir).args(["commit", "-qm", "first commit"]).status().unwrap();

        // log lists the commit.
        let commits = log(dir, None, 10);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "first commit");
        assert_eq!(commits[0].author, "Alice");
        assert!(commits[0].date.len() == 10, "YYYY-MM-DD");

        // blame attributes both committed lines to Alice.
        let b = blame(dir, &f).expect("blame");
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].author, "Alice");
        assert_eq!(b[0].hash, commits[0].hash);

        // file_diff shows a working-tree change vs HEAD.
        std::fs::write(&f, "one\nTWO\n").unwrap();
        let diff = file_diff(dir, &f).expect("diff");
        assert!(diff.contains("-two") && diff.contains("+TWO"), "unified diff: {diff}");
        // Uncommitted line now blames to the zero hash.
        let b2 = blame(dir, &f).unwrap();
        assert!(b2[1].hash.starts_with("0000"), "line 2 is not committed yet: {:?}", b2[1]);
    }

    /// The staged diff feeds the AI commit-message feature; commit clears it.
    /// Sets an identity locally so the test does not depend on the runner's.
    #[test]
    fn staged_diff_and_commit_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let dir = &std::fs::canonicalize(d.path()).unwrap();
        let init_ok = crate::proc::quiet("git")
            .arg("-C")
            .arg(dir)
            .args(["init", "-q"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !init_ok {
            eprintln!("git not available; skipping");
            return;
        }
        // A repo-local identity, so `commit` does not depend on global config.
        for kv in [["user.email", "t@example.com"], ["user.name", "Test"]] {
            let _ = crate::proc::quiet("git").arg("-C").arg(dir).args(["config", kv[0], kv[1]]).status();
        }

        // Nothing staged yet: an empty (but Some) diff.
        assert_eq!(staged_diff(dir).as_deref(), Some(""));

        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
        stage(dir, &[dir.join("a.txt")]).unwrap();
        let diff = staged_diff(dir).expect("in a repo");
        assert!(diff.contains("a.txt") && diff.contains("+hello"), "diff:\n{diff}");

        commit(dir, "add a.txt").unwrap();
        // Post-commit the stage is empty again.
        assert_eq!(staged_diff(dir).as_deref(), Some(""));
        // The message landed.
        let log = git_output(dir, &["log", "-1", "--pretty=%s"]).unwrap();
        assert_eq!(String::from_utf8_lossy(&log).trim(), "add a.txt");
    }
}
