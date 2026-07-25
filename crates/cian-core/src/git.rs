//! A thin layer over the `git` command line.
//!
//! cian shows git state next to files and can stage/unstage/discard them. It
//! shells out to `git` rather than linking libgit2 (a C dependency that would
//! break the single self-contained binary) or pulling in a large pure-Rust
//! implementation: every developer already has `git` on PATH, and a missing or
//! non-repo directory simply yields `None`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

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
}

/// Run `git` in `dir`, returning its stdout as bytes on a zero exit. `None` on
/// any failure (git missing, not a repo, non-zero exit) so callers can treat
/// "no git here" uniformly.
fn git_output(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
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
/// Examples: `main...origin/main [ahead 2, behind 1]`, `main`, `HEAD (no branch)`.
fn parse_branch_line(rest: &str) -> (String, u32, u32) {
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
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .context("run git")?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("git {} failed", args.first().cloned().unwrap_or_default())
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
        let init_ok = Command::new("git")
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
}
