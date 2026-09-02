//! A note: a Markdown file that says a little about itself.
//!
//! The whole of cian mode rests on one decision — **the notes are plain files
//! and there is no database.** Everything else follows from it:
//!
//!   * a OneNote migration is a script that writes files, not an import format
//!   * a SharePoint library, a synced OneDrive folder or a Dropbox folder is a
//!     notes folder with nothing added
//!   * crmaine can index them, because they are text on a disk
//!   * and nothing here is a lock-in: the exit is `ls`
//!
//! What a note knows about itself lives in YAML front matter, the convention
//! every static-site generator and every notes app already reads:
//!
//! ```text
//! ---
//! title: 移行の段取り
//! tags: [onenote, 2026]
//! created: 2026-09-02
//! ---
//! # 移行の段取り
//! ```
//!
//! **This module is in `cian-core` on purpose.** Electron does not run on iOS,
//! so an iPhone build would be a third front end and the only thing that could
//! cross is what lives here — pure Rust, no I/O beyond reading a file, no UI.
//! Putting note logic in a pane or a renderer would be cheap today and
//! expensive exactly once.
//!
//! **The YAML read here is a subset, deliberately.** `key: value`, and lists
//! written either `[a, b]` or as `- a` lines. Anchors, nested maps, multi-line
//! scalars and flow maps are not understood — a note that uses them keeps its
//! front matter as text and simply reports no tags, rather than a parser
//! written for a fifth of YAML guessing at the other four.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A note's front matter, and where the body starts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Front {
    /// `key: value` pairs, in the order YAML happens to give them (sorted, so
    /// two reads of one file cannot differ).
    pub fields: BTreeMap<String, String>,
    /// `tags:` read as a list, whichever of the two spellings was used.
    pub tags: Vec<String>,
    /// How many lines the block occupied, fences included. `0` when there was
    /// none — which is the common case and must not be an error.
    pub lines: usize,
}

impl Front {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }
}

/// Read the front matter off the top of a file's lines.
///
/// A block only counts at the very top, and only when it closes. A stray `---`
/// in the middle of a document is a horizontal rule, and an unclosed one at the
/// top is a document that happens to start with a rule — treating either as
/// front matter would silently swallow the beginning of somebody's note.
pub fn front(lines: &[String]) -> Front {
    let mut out = Front::default();
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return out;
    }
    let Some(end) = lines.iter().skip(1).position(|l| {
        let t = l.trim_end();
        t == "---" || t == "..."
    }) else {
        return out;
    };
    let end = end + 1;
    out.lines = end + 1;
    let mut list_key: Option<String> = None;
    for raw in &lines[1..end] {
        let line = raw.trim_end();
        // `  - value` continues whichever key opened the list.
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            if let Some(k) = &list_key {
                if k == "tags" {
                    out.tags.push(unquote(item.trim()).to_string());
                }
            }
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let key = k.trim().to_ascii_lowercase();
        let val = v.trim();
        if val.is_empty() {
            list_key = Some(key);
            continue;
        }
        list_key = None;
        if key == "tags" {
            out.tags = split_list(val);
            continue;
        }
        out.fields.insert(key, unquote(val).to_string());
    }
    out
}

/// `[a, b]` or `a, b` → the items. Quotes come off; empties are dropped.
fn split_list(v: &str) -> Vec<String> {
    let inner = v.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|x| unquote(x.trim()).to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// One note, as a list needs to show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub path: PathBuf,
    /// The front matter's `title`, else the first heading, else the file name
    /// without its extension. Never empty — a list of blanks is not a list.
    pub title: String,
    /// The first few lines of body text, flattened, for the second line of a
    /// row. Headings, fences and front matter are left out: they say what the
    /// note is *made of* rather than what it is about.
    pub excerpt: String,
    pub tags: Vec<String>,
    /// `updated` from the front matter if it has one, else the file's mtime as
    /// seconds since the epoch. Formatting belongs to whoever is drawing.
    pub updated: Option<u64>,
    pub bytes: u64,
}

/// Read one note. Only the head of the file is looked at — a list of two
/// hundred notes must not read two hundred whole files to draw itself.
pub fn read(path: &Path, head_lines: usize) -> Option<Note> {
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<String> = text.lines().take(head_lines.max(8)).map(str::to_string).collect();
    let meta = std::fs::metadata(path).ok();
    let f = front(&lines);
    let body = &lines[f.lines.min(lines.len())..];
    let title = f
        .get("title")
        .map(str::to_string)
        .filter(|t| !t.trim().is_empty())
        .or_else(|| heading(body))
        .or_else(|| {
            path.file_stem().map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "(no name)".to_string());
    let updated = f
        .get("updated")
        .and_then(date_secs)
        .or_else(|| {
            meta.as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
        });
    Some(Note {
        path: path.to_path_buf(),
        title,
        excerpt: excerpt(body),
        tags: f.tags,
        updated,
        bytes: meta.map(|m| m.len()).unwrap_or(0),
    })
}

/// The first `# heading`, if the note leads with one.
fn heading(body: &[String]) -> Option<String> {
    body.iter()
        .map(|l| l.trim())
        .find(|l| l.starts_with('#'))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .filter(|t| !t.is_empty())
}

/// A line of body text, flattened.
fn excerpt(body: &[String]) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for line in body {
        let t = line.trim();
        if t.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || t.is_empty() || t.starts_with('#') || t.starts_with("---") {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(t);
        if out.chars().count() >= 120 {
            break;
        }
    }
    out.chars().take(120).collect()
}

/// `2026-09-02` or `2026-09-02T10:00:00` → seconds since the epoch.
///
/// Days from the civil calendar, no time zone. A note's `updated` is a date
/// somebody typed; pretending to know which hour of it they meant, or in whose
/// zone, would be inventing precision.
fn date_secs(s: &str) -> Option<u64> {
    let d = s.trim();
    let d = d.split(['T', ' ']).next()?;
    let mut it = d.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let day: i64 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days.checked_mul(86_400)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ls(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn front_matter_is_read_both_ways_round() {
        let f = front(&ls("---\ntitle: 段取り\ntags: [onenote, 2026]\n---\n# 段取り\n"));
        assert_eq!(f.get("title"), Some("段取り"));
        assert_eq!(f.tags, ["onenote", "2026"]);
        assert_eq!(f.lines, 4, "the block, fences included");

        let f = front(&ls("---\ntags:\n  - a\n  - b\nstatus: done\n---\nbody\n"));
        assert_eq!(f.tags, ["a", "b"]);
        assert_eq!(f.get("status"), Some("done"));
    }

    /// **A `---` that is not front matter must not be treated as some.** A
    /// horizontal rule partway down, and a document that opens with one and
    /// never closes it, are both ordinary Markdown — swallowing either would
    /// eat the start of somebody's note.
    #[test]
    fn a_rule_is_not_front_matter() {
        assert_eq!(front(&ls("# title\n\n---\n\nbody\n")).lines, 0);
        assert_eq!(front(&ls("---\nnot closed\nbody\n")).lines, 0);
        assert_eq!(front(&ls("")).lines, 0);
    }

    #[test]
    fn a_title_falls_back_until_it_finds_one() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("kickoff.md");

        std::fs::write(&p, "---\ntitle: 名前\n---\n# 見出し\n").unwrap();
        assert_eq!(read(&p, 40).unwrap().title, "名前");

        std::fs::write(&p, "# 見出し\n本文\n").unwrap();
        assert_eq!(read(&p, 40).unwrap().title, "見出し", "then the heading");

        std::fs::write(&p, "本文だけ\n").unwrap();
        assert_eq!(read(&p, 40).unwrap().title, "kickoff", "then the file name");
    }

    /// The second line of a row says what the note is *about*, so what it is
    /// made of — headings, fences, front matter — is left out.
    #[test]
    fn the_excerpt_skips_the_scaffolding() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.md");
        std::fs::write(&p, "---\ntitle: t\n---\n# 見出し\n\n本文の一行目。\n```\ncode\n```\n二行目。\n").unwrap();
        let n = read(&p, 40).unwrap();
        assert_eq!(n.excerpt, "本文の一行目。 二行目。");
    }

    #[test]
    fn a_typed_date_beats_the_mtime() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.md");
        std::fs::write(&p, "---\nupdated: 2020-01-02\n---\nx\n").unwrap();
        // 2020-01-02T00:00:00Z
        assert_eq!(read(&p, 40).unwrap().updated, Some(1_577_923_200));
        std::fs::write(&p, "x\n").unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let got = read(&p, 40).unwrap().updated.unwrap();
        assert!(got.abs_diff(now) < 60, "otherwise the file's own time");
    }

    #[test]
    fn the_epoch_arithmetic_is_right() {
        assert_eq!(date_secs("1970-01-01"), Some(0));
        assert_eq!(date_secs("2026-09-02"), Some(1_788_307_200));
        assert_eq!(date_secs("2026-09-02T10:11:12"), Some(1_788_307_200), "the day only");
        assert_eq!(date_secs("nonsense"), None);
        assert_eq!(date_secs("2026-13-01"), None, "no thirteenth month");
    }
}
