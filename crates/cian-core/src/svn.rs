//! A thin layer over the `svn` command line, mirroring [`crate::git`].
//!
//! Many shops (Taketan's included) manage code in Subversion. cian shows the
//! same status marks, change gutter, log and blame for an svn working copy as it
//! does for git — reusing git's [`GitMark`], [`RepoStatus`], [`Commit`],
//! [`BlameLine`] and [`LineChange`] display types so the UI is identical. Like
//! git it shells out to `svn` rather than linking a library, and a directory
//! that is not a working copy simply yields `None`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Result};

use crate::git::{BlameLine, Commit, GitMark, LineChange, RepoStatus};

/// The working-copy root at or above `dir` (the ancestor holding `.svn`), or
/// `None` if `dir` is not in a working copy. Cheap — no `svn` invocation.
pub fn wc_root(dir: &Path) -> Option<PathBuf> {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        if d.join(".svn").is_dir() {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

/// Whether `dir` is inside an svn working copy.
pub fn is_working_copy(dir: &Path) -> bool {
    wc_root(dir).is_some()
}

/// Run `svn` in `dir`, capturing stdout on a zero exit. `--non-interactive`
/// keeps a missing credential or a prompt from hanging the captured command.
fn svn_output(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("svn")
        .current_dir(dir)
        .arg("--non-interactive")
        .args(args)
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// Status of the working copy `dir` sits in, in git's display shape. Marks come
/// from `svn status`; the "branch" slot is filled with `svn r<rev>`.
pub fn status(dir: &Path) -> Option<RepoStatus> {
    let root = wc_root(dir)?;
    let raw = svn_output(dir, &["status"])?;
    let text = String::from_utf8_lossy(&raw);

    let mut marks: Vec<(PathBuf, GitMark)> = Vec::new();
    for line in text.lines() {
        if line.len() < 8 {
            continue;
        }
        let code = line.chars().next().unwrap_or(' ');
        let Some(mark) = code_to_mark(code) else { continue };
        // `svn status` prints paths relative to the invocation directory.
        let rel = line[8..].trim();
        if rel.is_empty() {
            continue;
        }
        marks.push((dir.join(rel), mark));
    }

    let label = match revision(dir) {
        Some(r) => format!("svn r{}", r),
        None => "svn".to_string(),
    };
    Some(RepoStatus::from_marks(root, label, marks))
}

/// Map an `svn status` first-column code to a display mark.
fn code_to_mark(code: char) -> Option<GitMark> {
    match code {
        'M' | '!' => Some(GitMark::Modified), // modified, or missing
        'A' | 'D' | 'R' => Some(GitMark::Staged), // scheduled add / delete / replace
        'C' | '~' => Some(GitMark::Conflict), // conflicted, or obstructed
        '?' | 'I' => Some(GitMark::Untracked),
        _ => None,
    }
}

/// The working copy's current revision, from `svn info`.
fn revision(dir: &Path) -> Option<String> {
    let out = svn_output(dir, &["info"])?;
    String::from_utf8_lossy(&out)
        .lines()
        .find_map(|l| l.strip_prefix("Revision:").map(|r| r.trim().to_string()))
}

/// Per-line change status of `file` versus its pristine base (git's gutter type),
/// by diffing the working file against `svn cat -r BASE`.
pub fn line_changes(dir: &Path, file: &Path) -> Option<HashMap<usize, LineChange>> {
    wc_root(dir)?;
    let work = std::fs::read_to_string(file).ok()?;
    let work_lines: Vec<String> = work.lines().map(|s| s.to_string()).collect();
    let mut map = HashMap::new();
    let base = svn_output(dir, &["cat", "-r", "BASE", &file.to_string_lossy()]);
    let Some(base_bytes) = base else {
        // No base (a newly-added / untracked file): every line is new.
        for i in 0..work_lines.len() {
            map.insert(i, LineChange::Added);
        }
        return Some(map);
    };
    let base_text = String::from_utf8_lossy(&base_bytes);
    let base_lines: Vec<String> = base_text.lines().map(|s| s.to_string()).collect();
    crate::git::changes_from_lines(&base_lines, &work_lines, &mut map);
    Some(map)
}

/// The working-tree changes of `file` versus BASE, as a unified diff.
pub fn file_diff(dir: &Path, file: &Path) -> Option<String> {
    let out = svn_output(dir, &["diff", &file.to_string_lossy()])?;
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// A revision shown as a diff (`svn diff -c <rev>`).
pub fn show(dir: &Path, rev: &str) -> Option<String> {
    let r = rev.trim_start_matches('r');
    let out = svn_output(dir, &["diff", "-c", r])?;
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// Recent commits (newest first). `path` limits to that file's history.
pub fn log(dir: &Path, path: Option<&Path>, limit: usize) -> Vec<Commit> {
    let n = limit.max(1).to_string();
    let mut args: Vec<String> = vec!["log".into(), "-l".into(), n];
    let p;
    if let Some(path) = path {
        p = path.to_string_lossy().into_owned();
        args.push(p);
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let Some(out) = svn_output(dir, &arg_refs) else { return Vec::new() };
    parse_log(&String::from_utf8_lossy(&out))
}

/// Parse `svn log` text into commits. Blocks are separated by a dashed rule; the
/// header line is `r<rev> | <author> | <date …> | N line(s)`.
fn parse_log(text: &str) -> Vec<Commit> {
    let mut out = Vec::new();
    for block in text.split("------------------------------------------------------------------------") {
        let mut lines = block.trim_matches('\n').lines();
        let Some(header) = lines.next() else { continue };
        let parts: Vec<&str> = header.split(" | ").collect();
        if parts.len() < 3 || !parts[0].starts_with('r') {
            continue;
        }
        let hash = parts[0].trim().to_string();
        let author = parts[1].trim().to_string();
        // "2026-07-26 10:00:00 +0900 (…)" → keep the date only.
        let date = parts[2].split_whitespace().next().unwrap_or("").to_string();
        // Skip the blank line, take the first non-empty message line as subject.
        let subject = lines.find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        out.push(Commit { hash, date, author, subject });
    }
    out
}

/// Per-line blame for `file` (`svn blame -v`), one entry per line in order.
pub fn blame(dir: &Path, file: &Path) -> Option<Vec<BlameLine>> {
    let out = svn_output(dir, &["blame", "-v", &file.to_string_lossy()])?;
    let text = String::from_utf8_lossy(&out);
    let mut lines = Vec::new();
    for line in text.lines() {
        // "   123    alice 2026-07-26 10:00:00 +0900 (…) content"
        let mut it = line.split_whitespace();
        let rev = it.next().unwrap_or("");
        let author = it.next().unwrap_or("").to_string();
        let date = it.next().unwrap_or("").to_string();
        lines.push(BlameLine { hash: format!("r{}", rev), author, date });
    }
    Some(lines)
}

// ─────────────────────────────── mutations ───────────────────────────────

fn run_svn(dir: &Path, args: &[String]) -> Result<()> {
    let out = Command::new("svn")
        .current_dir(dir)
        .arg("--non-interactive")
        .args(args)
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
}

fn with_paths(verb: &str, paths: &[PathBuf]) -> Vec<String> {
    let mut a = vec![verb.to_string()];
    a.extend(paths.iter().map(|p| p.display().to_string()));
    a
}

/// `svn add` the given paths (schedule for addition).
pub fn add(dir: &Path, paths: &[PathBuf]) -> Result<()> {
    run_svn(dir, &with_paths("add", paths))
}

/// `svn revert` the given paths (undo local changes / scheduling).
pub fn revert(dir: &Path, paths: &[PathBuf]) -> Result<()> {
    run_svn(dir, &with_paths("revert", paths))
}

/// `svn resolve --accept working` the given paths (mark conflicts resolved,
/// keeping the working copy's merged content).
pub fn resolve(dir: &Path, paths: &[PathBuf]) -> Result<()> {
    let mut a = vec!["resolve".to_string(), "--accept".into(), "working".into()];
    a.extend(paths.iter().map(|p| p.display().to_string()));
    run_svn(dir, &a)
}

/// `svn update` the working copy (touches the network).
pub fn update(dir: &Path) -> Result<()> {
    run_svn(dir, &["update".to_string()])
}

/// `svn commit -m <message>` the given paths (touches the network).
pub fn commit(dir: &Path, paths: &[PathBuf], message: &str) -> Result<()> {
    let mut a = vec!["commit".to_string(), "-m".into(), message.to_string()];
    a.extend(paths.iter().map(|p| p.display().to_string()));
    run_svn(dir, &a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_mapping_and_log_parse() {
        assert_eq!(code_to_mark('M'), Some(GitMark::Modified));
        assert_eq!(code_to_mark('A'), Some(GitMark::Staged));
        assert_eq!(code_to_mark('C'), Some(GitMark::Conflict));
        assert_eq!(code_to_mark('?'), Some(GitMark::Untracked));
        assert_eq!(code_to_mark(' '), None);

        let sample = "\
------------------------------------------------------------------------
r42 | alice | 2026-07-26 10:00:00 +0900 (Fri, 26 Jul 2026) | 1 line

fix the thing
------------------------------------------------------------------------
r41 | bob | 2026-07-20 09:00:00 +0900 (Mon, 20 Jul 2026) | 2 lines

add the thing
more detail
------------------------------------------------------------------------";
        let commits = parse_log(sample);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "r42");
        assert_eq!(commits[0].author, "alice");
        assert_eq!(commits[0].date, "2026-07-26");
        assert_eq!(commits[0].subject, "fix the thing");
        assert_eq!(commits[1].subject, "add the thing");
    }

    #[test]
    fn not_a_working_copy_yields_none() {
        let d = tempfile::tempdir().unwrap();
        assert!(wc_root(d.path()).is_none());
        assert!(status(d.path()).is_none());
        assert!(line_changes(d.path(), &d.path().join("x")).is_none());
    }
}
