//! Pattern-based bulk rename — the offline counterpart to the AI rename.
//!
//! Two modes, chosen by the shape of the pattern:
//!
//! * **Substitution** `s/regex/replacement/flags` — a regular-expression search
//!   and replace over the whole filename. `$1` / `${name}` capture references
//!   work in the replacement; flags are `g` (replace every match, not just the
//!   first) and `i` (case-insensitive).
//!
//! * **Template** (anything else) — the pattern becomes the new name, with
//!   placeholders filled per file:
//!     - `{name}` the original stem (filename without its extension)
//!     - `{ext}`  the original extension (no dot), empty if none
//!     - `{n}`    a sequence number; `{n3}` zero-pads to width 3
//!
//! A template is the *whole* new name, so include `.{ext}` (or a literal
//! extension) yourself — nothing is appended for you, which keeps it honest.
//!
//! The engine only computes strings; the caller shows them for review and does
//! the actual on-disk renames, so a bad pattern never touches a file.

use anyhow::{bail, Result};
use regex::Regex;

/// How the counter in `{n}` / template numbering advances.
#[derive(Debug, Clone, Copy)]
pub struct Numbering {
    pub start: i64,
    pub step: i64,
}

impl Default for Numbering {
    fn default() -> Self {
        Numbering { start: 1, step: 1 }
    }
}

/// A parsed pattern, ready to apply to each name.
pub enum Plan {
    Subst { re: Regex, rep: String, all: bool },
    Template(String),
}

/// Parse a pattern string into a [`Plan`]. Errors on a malformed `s///` or a
/// bad regex, so the UI can report it before showing anything.
pub fn parse(pattern: &str) -> Result<Plan> {
    if let Some(rest) = pattern.strip_prefix("s/") {
        // Split on unescaped `/`. We keep it simple: `\/` is a literal slash.
        let parts = split_unescaped(rest, '/');
        if parts.len() < 2 || parts.len() > 3 {
            bail!("substitution must be s/regex/replacement/[flags]");
        }
        let flags = parts.get(2).map(String::as_str).unwrap_or("");
        let all = flags.contains('g');
        let ci = flags.contains('i');
        for f in flags.chars() {
            if f != 'g' && f != 'i' {
                bail!("unknown flag '{}' (only g and i)", f);
            }
        }
        let pat = if ci { format!("(?i){}", parts[0]) } else { parts[0].clone() };
        let re = Regex::new(&pat).map_err(|e| anyhow::anyhow!("bad regex: {}", e))?;
        Ok(Plan::Subst { re, rep: parts[1].clone(), all })
    } else {
        if pattern.trim().is_empty() {
            bail!("empty rename pattern");
        }
        Ok(Plan::Template(pattern.to_string()))
    }
}

/// The new filename for `name` (the current filename, extension included) at
/// zero-based position `index` in the batch.
pub fn apply(plan: &Plan, name: &str, index: usize, num: Numbering) -> String {
    match plan {
        Plan::Subst { re, rep, all } => {
            if *all {
                re.replace_all(name, rep.as_str()).into_owned()
            } else {
                re.replace(name, rep.as_str()).into_owned()
            }
        }
        Plan::Template(t) => {
            let (stem, ext) = split_ext(name);
            let counter = num.start + (index as i64) * num.step;
            expand_template(t, stem, ext, counter)
        }
    }
}

/// Convenience: parse then map a whole batch. Returns one new name per input,
/// in order.
pub fn plan_batch(pattern: &str, names: &[String], num: Numbering) -> Result<Vec<String>> {
    let plan = parse(pattern)?;
    Ok(names.iter().enumerate().map(|(i, n)| apply(&plan, n, i, num)).collect())
}

/// Split a filename into (stem, extension-without-dot). A leading dot (dotfile)
/// is part of the stem, matching how the rest of cian treats `.bashrc`.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    }
}

/// Expand `{name}`, `{ext}`, `{n}` / `{n<width>}` in a template.
fn expand_template(t: &str, stem: &str, ext: &str, counter: i64) -> String {
    let mut out = String::with_capacity(t.len() + 8);
    let bytes = t.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(close) = t[i..].find('}') {
                let token = &t[i + 1..i + close];
                if let Some(rep) = expand_token(token, stem, ext, counter) {
                    out.push_str(&rep);
                    i += close + 1;
                    continue;
                }
            }
        }
        // Not a recognised placeholder: copy the byte through.
        let ch = t[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// A single `{…}` token, or `None` if it is not one we know (left literal).
fn expand_token(token: &str, stem: &str, ext: &str, counter: i64) -> Option<String> {
    match token {
        "name" => Some(stem.to_string()),
        "ext" => Some(ext.to_string()),
        "n" => Some(counter.to_string()),
        _ => {
            // `n<width>` → zero-padded counter.
            let width = token.strip_prefix('n')?.parse::<usize>().ok()?;
            if counter < 0 {
                Some(format!("-{:0>width$}", (-counter).to_string(), width = width))
            } else {
                Some(format!("{:0>width$}", counter, width = width))
            }
        }
    }
}

/// Split `s` on `sep`, treating `\<sep>` as an escaped literal separator.
fn split_unescaped(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if next == sep {
                    cur.push(sep);
                    chars.next();
                    continue;
                }
            }
            cur.push('\\');
        } else if c == sep {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn template_numbering_and_parts() {
        let out = plan_batch("{name}_{n3}.{ext}", &names(&["a.txt", "b.log"]), Numbering::default()).unwrap();
        assert_eq!(out, vec!["a_001.txt", "b_002.log"]);
    }

    #[test]
    fn template_custom_start_and_step() {
        let out = plan_batch("img{n}.jpg", &names(&["x.png", "y.png", "z.png"]), Numbering { start: 10, step: 5 }).unwrap();
        assert_eq!(out, vec!["img10.jpg", "img15.jpg", "img20.jpg"]);
    }

    #[test]
    fn dotfile_has_no_extension() {
        let out = plan_batch("{name}-{ext}", &names(&[".bashrc"]), Numbering::default()).unwrap();
        assert_eq!(out, vec![".bashrc-"]);
    }

    #[test]
    fn substitution_first_and_global() {
        let first = plan_batch("s/o/0/", &names(&["foo.txt"]), Numbering::default()).unwrap();
        assert_eq!(first, vec!["f0o.txt"]);
        let all = plan_batch("s/o/0/g", &names(&["foo.txt"]), Numbering::default()).unwrap();
        assert_eq!(all, vec!["f00.txt"]);
    }

    #[test]
    fn substitution_captures_and_case_insensitive() {
        // Capture refs use the braced form so `${2}_` is not read as group "2_".
        let out = plan_batch(r"s/(\d+)-(\w+)/${2}_${1}/", &names(&["12-report.pdf"]), Numbering::default()).unwrap();
        assert_eq!(out, vec!["report_12.pdf"]);
        let ci = plan_batch("s/img/photo/i", &names(&["IMG_1.jpg"]), Numbering::default()).unwrap();
        assert_eq!(ci, vec!["photo_1.jpg"]);
    }

    #[test]
    fn bad_patterns_error() {
        assert!(parse("s/[/x/").is_err(), "bad regex");
        assert!(parse("s/only-two/").is_ok(), "two fields ok");
        assert!(parse("s/a/b/z").is_err(), "unknown flag");
        assert!(parse("").is_err(), "empty");
    }

    #[test]
    fn escaped_slash_in_substitution() {
        let out = plan_batch(r"s/a/x\/y/", &names(&["a.txt"]), Numbering::default()).unwrap();
        assert_eq!(out, vec!["x/y.txt"]);
    }
}
