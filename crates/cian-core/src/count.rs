//! Counting files and lines under a tree — cian's built-in "kazoechao": how
//! many files, how many lines, and how many *steps* (source lines, i.e. not
//! blank and not comment). What counts as a step is configurable, because the
//! answer people want depends on the project — see [`Options`], driven from
//! `count.lua`.

use std::path::{Path, PathBuf};

/// What to count and how. Loaded from `count.lua`; sensible defaults otherwise.
#[derive(Debug, Clone)]
pub struct Options {
    /// Extensions to include (lower-case, no dot). Empty = every text file.
    pub extensions: Vec<String>,
    /// Count blank lines as steps too (off = kazoechao-style SLOC).
    pub count_blank: bool,
    /// Count comment lines as steps too.
    pub count_comments: bool,
    /// Line-comment markers; a line whose first non-space run starts with one
    /// is a comment. Block comments are not tracked (a deliberate simplification).
    pub comment_prefixes: Vec<String>,
    /// Safety cap on how many files are visited.
    pub max_files: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            extensions: Vec::new(),
            count_blank: false,
            count_comments: false,
            comment_prefixes: ["//", "#", "--", ";", "%", "'", "*", "<!--", "rem "]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            max_files: 200_000,
        }
    }
}

/// Line tallies for a file or a group of files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub files: usize,
    /// Physical lines.
    pub total: usize,
    pub blank: usize,
    pub comment: usize,
    /// Lines that are neither blank nor comment.
    pub code: usize,
}

impl Counts {
    fn add(&mut self, other: Counts) {
        self.files += other.files;
        self.total += other.total;
        self.blank += other.blank;
        self.comment += other.comment;
        self.code += other.code;
    }

    /// The headline "steps" number under `o`: code lines, plus blanks and/or
    /// comments if the options fold them in.
    pub fn steps(&self, o: &Options) -> usize {
        self.code
            + if o.count_blank { self.blank } else { 0 }
            + if o.count_comments { self.comment } else { 0 }
    }
}

/// A full count: the grand total plus a per-extension breakdown.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub total: Counts,
    /// `(extension, counts)`, sorted by step count descending. `""` is used for
    /// files with no extension.
    pub by_ext: Vec<(String, Counts)>,
    /// True if the file cap was hit and counting stopped early.
    pub truncated: bool,
}

/// Directory names never worth counting (VCS metadata — never source).
const SKIP_DIRS: [&str; 3] = [".git", ".svn", ".hg"];
/// How much of each file to read (matches the viewer's feel; huge generated
/// files should not dominate a step count anyway).
const READ_LIMIT: u64 = 8 * 1024 * 1024;

/// Count every matching file under `paths` (files counted directly, directories
/// walked). Order-independent; the breakdown is sorted for display.
pub fn count(paths: &[PathBuf], o: &Options) -> Report {
    use std::collections::HashMap;
    let mut by_ext: HashMap<String, Counts> = HashMap::new();
    let mut total = Counts::default();
    let mut truncated = false;

    let mut stack: Vec<PathBuf> = paths.to_vec();
    while let Some(p) = stack.pop() {
        if total.files >= o.max_files {
            truncated = true;
            break;
        }
        let meta = match std::fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| SKIP_DIRS.contains(&n)) {
                continue;
            }
            if let Ok(rd) = std::fs::read_dir(&p) {
                for e in rd.flatten() {
                    stack.push(e.path());
                }
            }
            continue;
        }
        if !meta.is_file() {
            continue; // symlink, socket, etc.
        }
        if !ext_matches(&p, o) {
            continue;
        }
        if let Some(c) = count_file(&p, o) {
            total.add(c);
            by_ext.entry(ext_of(&p)).or_default().add(c);
        }
    }

    let mut by_ext: Vec<(String, Counts)> = by_ext.into_iter().collect();
    by_ext.sort_by(|a, b| b.1.steps(o).cmp(&a.1.steps(o)).then_with(|| a.0.cmp(&b.0)));
    Report { total, by_ext, truncated }
}

fn ext_of(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).unwrap_or_default()
}

fn ext_matches(p: &Path, o: &Options) -> bool {
    if o.extensions.is_empty() {
        return true;
    }
    let ext = ext_of(p);
    o.extensions.iter().any(|e| e.trim_start_matches('.').to_lowercase() == ext)
}

/// Count one file's lines. `None` if it can't be read or looks binary.
fn count_file(p: &Path, o: &Options) -> Option<Counts> {
    use std::io::Read;
    let f = std::fs::File::open(p).ok()?;
    let mut buf = Vec::new();
    f.take(READ_LIMIT).read_to_end(&mut buf).ok()?;
    // A NUL byte in the head means binary — don't count it as source.
    if buf.iter().take(8000).any(|&b| b == 0) {
        return None;
    }
    let text = String::from_utf8_lossy(&buf);
    let mut c = Counts { files: 1, ..Counts::default() };
    for line in text.lines() {
        c.total += 1;
        let t = line.trim_start();
        if t.is_empty() {
            c.blank += 1;
        } else if is_comment(t, o) {
            c.comment += 1;
        } else {
            c.code += 1;
        }
    }
    Some(c)
}

fn is_comment(trimmed: &str, o: &Options) -> bool {
    let lower = trimmed.to_lowercase();
    o.comment_prefixes.iter().any(|pfx| {
        // `rem ` (batch) is word-ish; others are punctuation prefixes.
        if pfx.ends_with(' ') {
            lower.starts_with(pfx)
        } else {
            !pfx.is_empty() && trimmed.starts_with(pfx.as_str())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn steps_exclude_blanks_and_comments_by_default() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.rs", "fn main() {}\n\n// a comment\nlet x = 1;\n");
        let o = Options { extensions: vec!["rs".into()], ..Default::default() };
        let r = count(&[d.path().to_path_buf()], &o);
        assert_eq!(r.total.files, 1);
        assert_eq!(r.total.total, 4);
        assert_eq!(r.total.blank, 1);
        assert_eq!(r.total.comment, 1);
        assert_eq!(r.total.code, 2);
        assert_eq!(r.total.steps(&o), 2, "blank + comment excluded");
    }

    #[test]
    fn options_can_fold_blanks_and_comments_in() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.rs", "code\n\n// c\n");
        let o = Options {
            extensions: vec!["rs".into()],
            count_blank: true,
            count_comments: true,
            ..Default::default()
        };
        let r = count(&[d.path().to_path_buf()], &o);
        assert_eq!(r.total.steps(&o), 3, "all physical lines count");
    }

    #[test]
    fn extension_filter_and_breakdown() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.rs", "let a = 1;\nlet b = 2;\n");
        write(d.path(), "b.py", "x = 1\n");
        write(d.path(), "note.txt", "ignored\n");
        let o = Options { extensions: vec!["rs".into(), "py".into()], ..Default::default() };
        let r = count(&[d.path().to_path_buf()], &o);
        assert_eq!(r.total.files, 2, "txt excluded");
        assert_eq!(r.total.code, 3);
        // rs (2 steps) sorts before py (1 step).
        assert_eq!(r.by_ext[0].0, "rs");
        assert_eq!(r.by_ext[0].1.code, 2);
        assert_eq!(r.by_ext[1].0, "py");
    }

    #[test]
    fn binary_and_vcs_are_skipped() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join(".git")).unwrap();
        write(d.path(), ".git/config.rs", "should not be counted\n");
        std::fs::write(d.path().join("bin.rs"), [0u8, 1, 2, 3]).unwrap();
        let o = Options { extensions: vec!["rs".into()], ..Default::default() };
        let r = count(&[d.path().to_path_buf()], &o);
        assert_eq!(r.total.files, 0, "vcs dir and binary file skipped");
    }
}
