//! Filesystem operations used by the file panes.
//!
//! Every routine here is non-interactive: it succeeds, fails, or returns a
//! conflict so the UI layer can decide how to react (overwrite / skip / etc).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use fs_extra::dir::{self, CopyOptions as DirCopyOptions};
use fs_extra::file::{self, CopyOptions as FileCopyOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// Skip a single destination if it already exists.
    Skip,
    /// Overwrite the destination unconditionally.
    Overwrite,
}

#[derive(Debug, Default, Clone)]
pub struct OpReport {
    pub ok: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

impl OpReport {
    pub fn merge(&mut self, other: OpReport) {
        self.ok += other.ok;
        self.skipped += other.skipped;
        self.errors.extend(other.errors);
    }
    pub fn note_error(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }
}

fn dest_for(src: &Path, dest_dir: &Path) -> PathBuf {
    let name = src
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    dest_dir.join(name)
}

pub fn copy_one(src: &Path, dest_dir: &Path, on_conflict: Conflict) -> Result<bool> {
    let target = dest_for(src, dest_dir);
    if target.exists() && on_conflict == Conflict::Skip {
        return Ok(false);
    }
    if src.is_dir() {
        let mut opts = DirCopyOptions::new();
        opts.overwrite = on_conflict == Conflict::Overwrite;
        opts.copy_inside = false;
        dir::copy(src, dest_dir, &opts)
            .with_context(|| format!("copy dir {} -> {}", src.display(), dest_dir.display()))?;
    } else {
        let mut opts = FileCopyOptions::new();
        opts.overwrite = on_conflict == Conflict::Overwrite;
        file::copy(src, &target, &opts)
            .with_context(|| format!("copy file {} -> {}", src.display(), target.display()))?;
    }
    Ok(true)
}

pub fn move_one(src: &Path, dest_dir: &Path, on_conflict: Conflict) -> Result<bool> {
    let target = dest_for(src, dest_dir);
    if target.exists() && on_conflict == Conflict::Skip {
        return Ok(false);
    }
    if src.is_dir() {
        let mut opts = DirCopyOptions::new();
        opts.overwrite = on_conflict == Conflict::Overwrite;
        opts.copy_inside = false;
        dir::move_dir(src, dest_dir, &opts)
            .with_context(|| format!("move dir {} -> {}", src.display(), dest_dir.display()))?;
    } else {
        let mut opts = FileCopyOptions::new();
        opts.overwrite = on_conflict == Conflict::Overwrite;
        file::move_file(src, &target, &opts)
            .with_context(|| format!("move file {} -> {}", src.display(), target.display()))?;
    }
    Ok(true)
}

/// How a delete disposes of its targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteMode {
    /// Move to the OS trash (Finder's Trash / the Windows Recycle Bin), so the
    /// user can undo a mistake. The default: `d` is one keystroke away from
    /// destroying work, and a file manager used daily must be forgiving.
    Trash,
    /// Unlink immediately. Unrecoverable.
    Permanent,
}

pub fn delete_one(src: &Path, mode: DeleteMode) -> Result<()> {
    match mode {
        DeleteMode::Trash => trash::delete(src)
            .with_context(|| format!("move to trash: {}", src.display()))?,
        DeleteMode::Permanent => {
            if src.is_dir() {
                fs::remove_dir_all(src).with_context(|| format!("rm -r {}", src.display()))?;
            } else {
                fs::remove_file(src).with_context(|| format!("rm {}", src.display()))?;
            }
        }
    }
    Ok(())
}

pub fn rename_in_place(src: &Path, new_name: &str) -> Result<PathBuf> {
    let parent = src
        .parent()
        .with_context(|| format!("no parent for {}", src.display()))?;
    let dest = parent.join(new_name);
    fs::rename(src, &dest)
        .with_context(|| format!("rename {} -> {}", src.display(), dest.display()))?;
    Ok(dest)
}

pub fn create_file(parent: &Path, name: &str) -> Result<PathBuf> {
    let p = parent.join(name);
    if p.exists() {
        anyhow::bail!("already exists: {}", p.display());
    }
    fs::File::create(&p).with_context(|| format!("touch {}", p.display()))?;
    Ok(p)
}

pub fn create_dir(parent: &Path, name: &str) -> Result<PathBuf> {
    let p = parent.join(name);
    if p.exists() {
        anyhow::bail!("already exists: {}", p.display());
    }
    fs::create_dir(&p).with_context(|| format!("mkdir {}", p.display()))?;
    Ok(p)
}

/// `mkdir`, optionally `-p`.
///
/// `spec` may contain path separators (`a/b/c`); without `parents` every
/// component but the last must already exist, matching plain `mkdir`. With
/// `parents` the whole chain is made and an existing target is not an error,
/// matching `mkdir -p`.
pub fn make_dir(parent: &Path, spec: &str, parents: bool) -> Result<PathBuf> {
    let p = parent.join(spec);
    if parents {
        fs::create_dir_all(&p).with_context(|| format!("mkdir -p {}", p.display()))?;
    } else {
        if p.exists() {
            anyhow::bail!("already exists: {} (use -p to ignore)", p.display());
        }
        fs::create_dir(&p).with_context(|| format!("mkdir {}", p.display()))?;
    }
    Ok(p)
}

/// `touch`: create the file if missing, otherwise bump its modification time.
pub fn touch(parent: &Path, name: &str) -> Result<PathBuf> {
    let p = parent.join(name);
    let existed = p.exists();
    let f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .with_context(|| format!("touch {}", p.display()))?;
    if existed {
        // Only worth moving the clock on a file that was already there; a
        // fresh one is already stamped now.
        f.set_modified(std::time::SystemTime::now())
            .with_context(|| format!("touch {}", p.display()))?;
    }
    Ok(p)
}

/// Bulk copy with a single conflict policy applied to every source.
pub fn copy_many(srcs: &[PathBuf], dest_dir: &Path, on_conflict: Conflict) -> OpReport {
    let mut report = OpReport::default();
    for src in srcs {
        match copy_one(src, dest_dir, on_conflict) {
            Ok(true) => report.ok += 1,
            Ok(false) => report.skipped += 1,
            Err(e) => report.note_error(format!("{}: {}", src.display(), e)),
        }
    }
    report
}

pub fn move_many(srcs: &[PathBuf], dest_dir: &Path, on_conflict: Conflict) -> OpReport {
    let mut report = OpReport::default();
    for src in srcs {
        match move_one(src, dest_dir, on_conflict) {
            Ok(true) => report.ok += 1,
            Ok(false) => report.skipped += 1,
            Err(e) => report.note_error(format!("{}: {}", src.display(), e)),
        }
    }
    report
}

pub fn delete_many(srcs: &[PathBuf], mode: DeleteMode) -> OpReport {
    let mut report = OpReport::default();
    for src in srcs {
        match delete_one(src, mode) {
            Ok(()) => report.ok += 1,
            Err(e) => report.note_error(format!("{}: {}", src.display(), e)),
        }
    }
    report
}

#[cfg(test)]
mod make_touch_tests {
    use super::*;

    #[test]
    fn mkdir_p_creates_a_chain_and_tolerates_existing() {
        let d = tempfile::tempdir().unwrap();
        let made = make_dir(d.path(), "a/b/c", true).unwrap();
        assert!(made.is_dir());
        assert!(d.path().join("a/b/c").is_dir());
        // -p run twice is not an error.
        assert!(make_dir(d.path(), "a/b/c", true).is_ok());
    }

    #[test]
    fn plain_mkdir_needs_the_parent_and_refuses_an_existing_dir() {
        let d = tempfile::tempdir().unwrap();
        // No parent yet: plain mkdir fails.
        assert!(make_dir(d.path(), "x/y", false).is_err());
        make_dir(d.path(), "x", false).unwrap();
        make_dir(d.path(), "x/y", false).unwrap();
        // Existing: refused without -p.
        assert!(make_dir(d.path(), "x", false).is_err());
    }

    #[test]
    fn touch_creates_then_bumps_the_mtime() {
        let d = tempfile::tempdir().unwrap();
        let p = touch(d.path(), "note.txt").unwrap();
        assert!(p.is_file());
        // Content is preserved when touched again (append mode, nothing written).
        fs::write(&p, b"keep me").unwrap();
        let before = fs::metadata(&p).unwrap().modified().unwrap();
        // Force a distinctly older stamp, then touch and confirm it advanced.
        // The handle must be writable to set times on Windows, so open for
        // write rather than read.
        let old = before - std::time::Duration::from_secs(120);
        fs::OpenOptions::new().write(true).open(&p).unwrap().set_modified(old).unwrap();
        touch(d.path(), "note.txt").unwrap();
        let after = fs::metadata(&p).unwrap().modified().unwrap();
        assert!(after > old, "mtime moved forward");
        assert_eq!(fs::read(&p).unwrap(), b"keep me", "contents untouched");
    }
}
