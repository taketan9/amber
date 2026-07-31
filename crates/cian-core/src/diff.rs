//! Comparing the file under the cursor on the left with the one on the right.
//!
//! The two-pane layout is the whole point of this kind of file manager, and
//! "these two look the same, are they?" is the question it is best placed to
//! answer. The result is produced as aligned rows rather than a patch: the
//! panes are already side by side, so showing the two files side by side needs
//! no reading of `@@` headers to interpret.
//!
//! The algorithm is a plain longest-common-subsequence, with the common prefix
//! and suffix trimmed off first. That trimming is what makes it fast in the
//! case that actually happens — two versions of the same file, differing in a
//! few lines — and the quadratic table only ever covers the disagreeing middle.

use std::path::Path;

use anyhow::Result;

use crate::viewer::{view_file, ViewKind};

/// One line, with the number it has in its own file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// 1-based, as an editor would show it.
    pub no: usize,
    pub text: String,
}

/// One row of the side-by-side rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// Present and identical in both.
    Same { left: Line, right: Line },
    /// A line that was replaced: both sides exist but differ.
    Changed { left: Line, right: Line },
    /// Only in the left file.
    Removed { left: Line },
    /// Only in the right file.
    Added { right: Line },
    /// A run of identical lines that was folded away.
    Skipped { lines: usize },
}

impl Row {
    pub fn is_difference(&self) -> bool {
        !matches!(self, Row::Same { .. } | Row::Skipped { .. })
    }
}

/// The comparison of two files.
#[derive(Debug, Clone)]
pub struct Diff {
    pub rows: Vec<Row>,
    pub added: usize,
    pub removed: usize,
    pub changed: usize,
    /// Either file was longer than the viewer's read limit, so this compares
    /// only the part that was read.
    pub truncated: bool,
    /// Either file is binary; the comparison is then whole-file only.
    pub binary: bool,
    /// The two files' contents, as far as they were read, are the same.
    pub identical: bool,
    /// The differing middle was too large for the line-by-line algorithm and
    /// is reported as one wholesale replacement.
    pub too_large: bool,
}

/// Above this many cells the quadratic table costs more memory and time than
/// the answer is worth; 4M cells of `u32` is 16 MB, which is the most a
/// keypress should ever allocate.
const MAX_CELLS: usize = 4_000_000;

/// How many identical lines to keep either side of a difference before folding
/// the rest away. Three is what every diff tool has settled on.
pub const CONTEXT: usize = 3;

/// Compare two files line by line.
pub fn diff_files(left: &Path, right: &Path) -> Result<Diff> {
    let a = view_file(left)?;
    let b = view_file(right)?;
    let truncated = a.truncated || b.truncated;

    // A hex dump diffed line-by-line produces noise, and a shifted byte makes
    // every subsequent row differ. For binaries the honest answer is the only
    // one worth giving.
    if a.kind == ViewKind::Binary || b.kind == ViewKind::Binary {
        let identical = a.total_bytes == b.total_bytes && a.lines == b.lines;
        return Ok(Diff {
            rows: Vec::new(),
            added: 0,
            removed: 0,
            changed: 0,
            truncated,
            binary: true,
            identical,
            too_large: false,
        });
    }

    let mut d = diff_lines(&a.lines, &b.lines);
    d.truncated = truncated;
    Ok(d)
}

/// Like [`diff_files`], but decode both sides with an explicit encoding first —
/// for the diff viewer's "switch encoding" the same way the F3 viewer offers.
pub fn diff_files_with_encoding(
    left: &Path,
    right: &Path,
    enc: crate::viewer::TextEncoding,
) -> Result<Diff> {
    let mut a = view_file(left)?;
    let mut b = view_file(right)?;
    let truncated = a.truncated || b.truncated;
    if a.kind == ViewKind::Binary || b.kind == ViewKind::Binary {
        let identical = a.total_bytes == b.total_bytes && a.lines == b.lines;
        return Ok(Diff {
            rows: Vec::new(),
            added: 0,
            removed: 0,
            changed: 0,
            truncated,
            binary: true,
            identical,
            too_large: false,
        });
    }
    a.redecode(enc);
    b.redecode(enc);
    let mut d = diff_lines(&a.lines, &b.lines);
    d.truncated = truncated;
    Ok(d)
}

/// The line-level comparison, separated from any file reading so it can be
/// tested directly.
pub fn diff_lines(a: &[String], b: &[String]) -> Diff {
    // Trimming the shared head and tail is what keeps this cheap: two versions
    // of one file usually agree everywhere except a handful of lines, and only
    // that middle reaches the quadratic part.
    let mut head = 0;
    while head < a.len() && head < b.len() && a[head] == b[head] {
        head += 1;
    }
    let mut tail = 0;
    while tail < a.len() - head && tail < b.len() - head && a[a.len() - 1 - tail] == b[b.len() - 1 - tail]
    {
        tail += 1;
    }

    let mid_a = &a[head..a.len() - tail];
    let mid_b = &b[head..b.len() - tail];

    let mut rows = Vec::with_capacity(a.len().max(b.len()));
    let mut added = 0;
    let mut removed = 0;
    let mut changed = 0;
    let mut too_large = false;

    for i in 0..head {
        rows.push(Row::Same {
            left: Line { no: i + 1, text: a[i].clone() },
            right: Line { no: i + 1, text: b[i].clone() },
        });
    }

    let pairs = if mid_a.is_empty() || mid_b.is_empty() {
        Vec::new()
    } else if mid_a.len().saturating_mul(mid_b.len()) > MAX_CELLS {
        // Rather than refuse, report the middle as replaced wholesale. It is a
        // true statement about the files, just a coarse one, and the flag says
        // so on screen.
        too_large = true;
        Vec::new()
    } else {
        lcs(mid_a, mid_b)
    };

    // Walk both middles, emitting the hunks between successive matches.
    let mut ia = 0;
    let mut ib = 0;
    let emit = |rows: &mut Vec<Row>,
                added: &mut usize,
                removed: &mut usize,
                changed: &mut usize,
                dels: &[usize],
                adds: &[usize]| {
        // Pair a deletion with an addition where both are present: a modified
        // line then shows as one row with its before and after alongside,
        // which is the shape people read a side-by-side diff for.
        let paired = dels.len().min(adds.len());
        for k in 0..paired {
            rows.push(Row::Changed {
                left: Line { no: head + dels[k] + 1, text: mid_a[dels[k]].clone() },
                right: Line { no: head + adds[k] + 1, text: mid_b[adds[k]].clone() },
            });
            *changed += 1;
        }
        for &k in &dels[paired..] {
            rows.push(Row::Removed {
                left: Line { no: head + k + 1, text: mid_a[k].clone() },
            });
            *removed += 1;
        }
        for &k in &adds[paired..] {
            rows.push(Row::Added {
                right: Line { no: head + k + 1, text: mid_b[k].clone() },
            });
            *added += 1;
        }
    };

    for &(ma, mb) in &pairs {
        let dels: Vec<usize> = (ia..ma).collect();
        let adds: Vec<usize> = (ib..mb).collect();
        emit(&mut rows, &mut added, &mut removed, &mut changed, &dels, &adds);
        rows.push(Row::Same {
            left: Line { no: head + ma + 1, text: mid_a[ma].clone() },
            right: Line { no: head + mb + 1, text: mid_b[mb].clone() },
        });
        ia = ma + 1;
        ib = mb + 1;
    }
    let dels: Vec<usize> = (ia..mid_a.len()).collect();
    let adds: Vec<usize> = (ib..mid_b.len()).collect();
    emit(&mut rows, &mut added, &mut removed, &mut changed, &dels, &adds);

    for t in 0..tail {
        let i = a.len() - tail + t;
        let j = b.len() - tail + t;
        rows.push(Row::Same {
            left: Line { no: i + 1, text: a[i].clone() },
            right: Line { no: j + 1, text: b[j].clone() },
        });
    }

    Diff {
        identical: added == 0 && removed == 0 && changed == 0,
        rows,
        added,
        removed,
        changed,
        truncated: false,
        binary: false,
        too_large,
    }
}

/// Indices of a longest common subsequence, as `(index in a, index in b)`.
fn lcs(a: &[String], b: &[String]) -> Vec<(usize, usize)> {
    let n = a.len();
    let m = b.len();
    let w = m + 1;
    let mut t = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i * w + j] = if a[i] == b[j] {
                t[(i + 1) * w + j + 1] + 1
            } else {
                t[(i + 1) * w + j].max(t[i * w + j + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push((i, j));
            i += 1;
            j += 1;
        } else if t[(i + 1) * w + j] >= t[i * w + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// Replace long runs of identical lines with a [`Row::Skipped`] marker.
///
/// Without this, comparing two 2000-line files that differ in one place means
/// scrolling through 2000 rows to find it.
pub fn fold(rows: &[Row], context: usize) -> Vec<Row> {
    let keep: Vec<bool> = (0..rows.len())
        .map(|i| {
            let lo = i.saturating_sub(context);
            let hi = (i + context).min(rows.len().saturating_sub(1));
            (lo..=hi).any(|k| rows[k].is_difference())
        })
        .collect();

    let mut out = Vec::with_capacity(rows.len());
    let mut run = 0usize;
    for (i, row) in rows.iter().enumerate() {
        if keep[i] {
            if run > 0 {
                out.push(Row::Skipped { lines: run });
                run = 0;
            }
            out.push(row.clone());
        } else {
            run += 1;
        }
    }
    if run > 0 {
        out.push(Row::Skipped { lines: run });
    }
    out
}

/// A one-line summary for the popup title.
pub fn summary(d: &Diff) -> String {
    if d.binary {
        return if d.identical {
            "binary, identical".to_string()
        } else {
            "binary, differ".to_string()
        };
    }
    if d.identical {
        return "identical".to_string();
    }
    let mut parts = Vec::new();
    if d.changed > 0 {
        parts.push(format!("~{}", d.changed));
    }
    if d.added > 0 {
        parts.push(format!("+{}", d.added));
    }
    if d.removed > 0 {
        parts.push(format!("-{}", d.removed));
    }
    let mut s = parts.join(" ");
    if d.too_large {
        s.push_str("  (too large to align)");
    }
    if d.truncated {
        s.push_str("  (compared the first 4 MB)");
    }
    s
}

/// The identical run at the start and end of two changed lines, in **chars**.
///
/// A `Changed` row usually rewrites only part of the line. Showing which part —
/// the way WinMerge underlines the edited span and leaves the rest calm — reads
/// far better than repainting the whole line one flat color. Returns
/// `(prefix, suffix)` character counts common to both sides; they never overlap
/// (their sum never exceeds the shorter line's length).
pub fn common_affixes(a: &str, b: &str) -> (usize, usize) {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let mut p = 0;
    while p < ac.len() && p < bc.len() && ac[p] == bc[p] {
        p += 1;
    }
    // The suffix scan must stop at the shared prefix so the two runs can't
    // claim the same character on the shorter side.
    let max_s = ac.len().min(bc.len()) - p;
    let mut s = 0;
    while s < max_s && ac[ac.len() - 1 - s] == bc[bc.len() - 1 - s] {
        s += 1;
    }
    (p, s)
}

// ── Exporting a diff to a readable report ────────────────────────────────────
//
// The on-screen view is side-by-side, but the saved-to-disk form used to be a
// unified `-`/`+` dump that is hard to read after the fact. These render the
// same two-column view WinMerge shows, as a self-contained HTML page (colored,
// with the edited span within a changed line marked) or a Markdown table (which
// renders on GitHub and in cian's own viewer).

pub(crate) fn html_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}

/// A changed line as HTML with only the edited middle wrapped in `<mark>`.
fn changed_html(text: &str, prefix: usize, suffix: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let suffix = suffix.min(n.saturating_sub(prefix));
    let mid_end = n - suffix;
    if prefix >= mid_end {
        // Nothing distinct on this side (a pure insertion on the other) —
        // still mark an empty point so the eye lands on where it changed.
        return html_escape(text);
    }
    let seg = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
    format!(
        "{}<mark>{}</mark>{}",
        html_escape(&seg(0, prefix)),
        html_escape(&seg(prefix, mid_end)),
        html_escape(&seg(mid_end, n)),
    )
}

fn html_row(cls: &str, ln: Option<usize>, lt: &str, rn: Option<usize>, rt: &str) -> String {
    let cell = |no: Option<usize>, text: &str| -> String {
        match no {
            None => "<td class=\"num\"></td><td class=\"empty\"></td>".to_string(),
            Some(n) => format!(
                "<td class=\"num\">{}</td><td class=\"code\">{}</td>",
                n,
                if text.is_empty() { "&nbsp;" } else { text }
            ),
        }
    };
    format!("<tr class=\"{}\">{}{}</tr>\n", cls, cell(ln, lt), cell(rn, rt))
}

pub(crate) const REPORT_STYLE: &str = r#"<style>
  :root { color-scheme: light dark; }
  body { font: 13px/1.5 -apple-system, Segoe UI, Roboto, sans-serif; margin: 1.5rem; }
  h1 { font-size: 1.1rem; font-weight: 600; }
  h1 .arrow { color: #888; margin: 0 .4rem; }
  .summary { color: #666; margin: .2rem 0 1rem; }
  table { border-collapse: collapse; width: 100%; table-layout: fixed; }
  th { text-align: left; font-weight: 600; padding: .2rem .5rem;
       border-bottom: 2px solid #ccc; }
  td { padding: 0 .5rem; vertical-align: top; }
  td.code, td.empty { font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
       white-space: pre-wrap; word-break: break-word; }
  td.num { width: 3.5em; text-align: right; color: #999; user-select: none;
       font-family: ui-monospace, monospace; }
  td.empty { background: repeating-linear-gradient(45deg,
       transparent, transparent 6px, rgba(128,128,128,.08) 6px, rgba(128,128,128,.08) 12px); }
  tr.add td.code { background: #e6ffed; }
  tr.del td.code { background: #ffeef0; }
  tr.chg td.code { background: #fff8e5; }
  tr.chg mark { background: #ffd966; color: inherit; padding: 0 1px; border-radius: 2px; }
  @media (prefers-color-scheme: dark) {
    th { border-color: #444; }
    tr.add td.code { background: #123a1c; }
    tr.del td.code { background: #3a1417; }
    tr.chg td.code { background: #3a3313; }
    tr.chg mark { background: #7a5c12; color: #ffe9a8; }
  }
</style>
"#;

/// Render the two-file comparison as a self-contained WinMerge-style HTML page.
/// Shows every line (full context, not folded); the changed span within a line
/// is highlighted.
pub fn to_html(diff: &Diff, left: &str, right: &str) -> String {
    let mut s = String::new();
    s.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    s.push_str(&format!("<title>diff: {} \u{2194} {}</title>\n", html_escape(left), html_escape(right)));
    s.push_str(REPORT_STYLE);
    s.push_str("</head>\n<body>\n");
    s.push_str(&format!(
        "<h1>{} <span class=\"arrow\">\u{2194}</span> {}</h1>\n",
        html_escape(left),
        html_escape(right)
    ));
    s.push_str(&format!("<p class=\"summary\">{}</p>\n", html_escape(&summary(diff))));
    if diff.binary {
        s.push_str(&format!(
            "<p>{}</p>\n</body>\n</html>\n",
            if diff.identical { "Binary files, identical." } else { "Binary files differ." }
        ));
        return s;
    }
    s.push_str("<table>\n<thead><tr><th class=\"num\">#</th><th>");
    s.push_str(&html_escape(left));
    s.push_str("</th><th class=\"num\">#</th><th>");
    s.push_str(&html_escape(right));
    s.push_str("</th></tr></thead>\n<tbody>\n");
    for r in &diff.rows {
        match r {
            Row::Same { left: l, right: rr } => {
                s.push_str(&html_row("same", Some(l.no), &html_escape(&l.text), Some(rr.no), &html_escape(&rr.text)));
            }
            Row::Changed { left: l, right: rr } => {
                let (p, sfx) = common_affixes(&l.text, &rr.text);
                s.push_str(&html_row(
                    "chg",
                    Some(l.no),
                    &changed_html(&l.text, p, sfx),
                    Some(rr.no),
                    &changed_html(&rr.text, p, sfx),
                ));
            }
            Row::Removed { left: l } => {
                s.push_str(&html_row("del", Some(l.no), &html_escape(&l.text), None, ""));
            }
            Row::Added { right: rr } => {
                s.push_str(&html_row("add", None, "", Some(rr.no), &html_escape(&rr.text)));
            }
            Row::Skipped { .. } => {}
        }
    }
    s.push_str("</tbody>\n</table>\n</body>\n</html>\n");
    s
}

/// One cell's text for a Markdown table: pipes escaped, wrapped in code so
/// leading spaces survive, blank rendered as a space so the column stays.
pub(crate) fn md_code(text: &str) -> String {
    if text.is_empty() {
        return " ".to_string();
    }
    // Backticks inside inline code would end it early; swap for a look-alike.
    let t = text.replace('|', "\\|").replace('`', "\u{02bc}");
    format!("`{}`", t)
}

/// Render the two-file comparison as a side-by-side Markdown table. Renders on
/// GitHub and in cian's own viewer; the leading column marks each row
/// `~`/`+`/`-`/(blank) so a changed / added / removed / same line is obvious.
pub fn to_markdown(diff: &Diff, left: &str, right: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!("# diff: {} \u{2194} {}\n\n", left, right));
    s.push_str(&format!("`{}`\n\n", summary(diff)));
    if diff.binary {
        s.push_str(if diff.identical { "Binary files, identical.\n" } else { "Binary files differ.\n" });
        return s;
    }
    let h = |x: &str| x.replace('|', "\\|");
    s.push_str(&format!("|   | # | {} | # | {} |\n", h(left), h(right)));
    s.push_str("|:-:|--:|---|--:|---|\n");
    let num = |n: usize| n.to_string();
    for r in &diff.rows {
        let (st, ln, lt, rn, rt) = match r {
            Row::Same { left: l, right: rr } => (" ", num(l.no), md_code(&l.text), num(rr.no), md_code(&rr.text)),
            Row::Changed { left: l, right: rr } => ("~", num(l.no), md_code(&l.text), num(rr.no), md_code(&rr.text)),
            Row::Removed { left: l } => ("-", num(l.no), md_code(&l.text), String::new(), " ".to_string()),
            Row::Added { right: rr } => ("+", String::new(), " ".to_string(), num(rr.no), md_code(&rr.text)),
            Row::Skipped { .. } => continue,
        };
        s.push_str(&format!("| {} | {} | {} | {} | {} |\n", st, ln, lt, rn, rt));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    /// Compact rendering used by the tests, so an expectation reads like the
    /// screen does.
    fn render(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Same { left, .. } => format!("  {}", left.text),
                Row::Changed { left, right } => format!("~ {} | {}", left.text, right.text),
                Row::Removed { left } => format!("- {}", left.text),
                Row::Added { right } => format!("+ {}", right.text),
                Row::Skipped { lines } => format!("... {} lines", lines),
            })
            .collect()
    }

    #[test]
    fn common_affixes_isolates_the_edited_middle() {
        // Shared "let x = " ... ";" wrapping a changed value.
        assert_eq!(common_affixes("let x = 1;", "let x = 42;"), (8, 1));
        // No overlap: identical prefix and suffix can't double-count a char.
        assert_eq!(common_affixes("ab", "aXb"), (1, 1));
        // One side a prefix of the other — the whole shorter run is common.
        assert_eq!(common_affixes("foo", "foobar"), (3, 0));
        // Nothing in common: the entire line is the edit.
        assert_eq!(common_affixes("abc", "xyz"), (0, 0));
        // Char-based, so multibyte text splits on character boundaries.
        assert_eq!(common_affixes("あいう", "あXう"), (1, 1));
    }

    #[test]
    fn html_export_is_side_by_side_marks_the_edit_and_escapes() {
        let d = diff_lines(&["let x = 1;".to_string()], &["let x = 2;".to_string()]);
        let html = to_html(&d, "a.rs", "b.rs");
        assert!(html.contains("<table"), "has a table");
        // Only the edited character is wrapped, not the whole line.
        assert!(html.contains("<mark>1</mark>"), "left edit marked: {}", html);
        assert!(html.contains("<mark>2</mark>"), "right edit marked");
        assert!(html.contains("let x = "), "the common prefix stays plain");

        let d2 = diff_lines(&["a<b".to_string()], &["a>b".to_string()]);
        let html2 = to_html(&d2, "l", "r");
        assert!(html2.contains("&lt;") && html2.contains("&gt;"), "angle brackets escaped");
        assert!(!html2.contains("a<b"), "raw content is not emitted");
    }

    #[test]
    fn markdown_export_is_a_side_by_side_table() {
        let d = diff_lines(
            &["same".to_string(), "gone".to_string()],
            &["same".to_string(), "new".to_string(), "extra".to_string()],
        );
        let md = to_markdown(&d, "a", "b");
        assert!(md.starts_with("# diff: a \u{2194} b"), "titled: {}", md);
        assert!(md.contains("| # | a | # | b |"), "two numbered columns");
        // A pipe in content must be escaped so it cannot break the table.
        let dp = diff_lines(&["a|b".to_string()], &["a|c".to_string()]);
        let mdp = to_markdown(&dp, "l", "r");
        assert!(mdp.contains("a\\|b"), "pipe escaped: {}", mdp);
    }

    #[test]
    fn identical_files_produce_no_differences() {
        let a = lines(&["one", "two", "three"]);
        let d = diff_lines(&a, &a);
        assert!(d.identical);
        assert_eq!(d.added + d.removed + d.changed, 0);
        assert_eq!(d.rows.len(), 3, "every line still shown, just as context");
        assert!(d.rows.iter().all(|r| matches!(r, Row::Same { .. })));
    }

    #[test]
    fn a_replaced_line_shows_both_sides_on_one_row() {
        let a = lines(&["one", "two", "three"]);
        let b = lines(&["one", "TWO", "three"]);
        let d = diff_lines(&a, &b);
        assert_eq!(render(&d.rows), vec!["  one", "~ two | TWO", "  three"]);
        assert_eq!((d.changed, d.added, d.removed), (1, 0, 0));
        assert!(!d.identical);
    }

    #[test]
    fn an_inserted_line_is_added_on_the_right_only() {
        let a = lines(&["one", "three"]);
        let b = lines(&["one", "two", "three"]);
        let d = diff_lines(&a, &b);
        assert_eq!(render(&d.rows), vec!["  one", "+ two", "  three"]);
        assert_eq!((d.changed, d.added, d.removed), (0, 1, 0));
    }

    #[test]
    fn a_deleted_line_is_removed_on_the_left_only() {
        let a = lines(&["one", "two", "three"]);
        let b = lines(&["one", "three"]);
        let d = diff_lines(&a, &b);
        assert_eq!(render(&d.rows), vec!["  one", "- two", "  three"]);
        assert_eq!((d.changed, d.added, d.removed), (0, 0, 1));
    }

    /// Line numbers are each file's own, which is the whole use of showing
    /// them: they are what you type into an editor to go to the place.
    #[test]
    fn line_numbers_follow_each_file_separately() {
        let a = lines(&["a", "b", "c", "d"]);
        let b = lines(&["a", "c", "d"]);
        let d = diff_lines(&a, &b);
        let nums: Vec<(Option<usize>, Option<usize>)> = d
            .rows
            .iter()
            .map(|r| match r {
                Row::Same { left, right } => (Some(left.no), Some(right.no)),
                Row::Changed { left, right } => (Some(left.no), Some(right.no)),
                Row::Removed { left } => (Some(left.no), None),
                Row::Added { right } => (None, Some(right.no)),
                Row::Skipped { .. } => (None, None),
            })
            .collect();
        assert_eq!(
            nums,
            vec![(Some(1), Some(1)), (Some(2), None), (Some(3), Some(2)), (Some(4), Some(3))],
            "after the deletion the right side stays one behind"
        );
    }

    #[test]
    fn a_block_swapped_for_a_different_block_pairs_up_then_spills() {
        let a = lines(&["k", "x1", "x2", "x3", "z"]);
        let b = lines(&["k", "y1", "z"]);
        let d = diff_lines(&a, &b);
        assert_eq!(
            render(&d.rows),
            vec!["  k", "~ x1 | y1", "- x2", "- x3", "  z"],
            "one pairing, the surplus deletions after it"
        );
        assert_eq!((d.changed, d.added, d.removed), (1, 0, 2));
    }

    #[test]
    fn an_empty_file_against_a_full_one_is_all_additions() {
        let d = diff_lines(&[], &lines(&["a", "b"]));
        assert_eq!(render(&d.rows), vec!["+ a", "+ b"]);
        assert_eq!(d.added, 2);

        let d = diff_lines(&lines(&["a", "b"]), &[]);
        assert_eq!(d.removed, 2);
    }

    #[test]
    fn two_empty_files_are_identical() {
        let d = diff_lines(&[], &[]);
        assert!(d.identical);
        assert!(d.rows.is_empty());
    }

    /// Completely different files must not be aligned into nonsense pairings
    /// with an accidental shared line.
    #[test]
    fn nothing_in_common_is_a_clean_replacement() {
        let a = lines(&["aaa", "bbb"]);
        let b = lines(&["xxx", "yyy"]);
        let d = diff_lines(&a, &b);
        assert_eq!(render(&d.rows), vec!["~ aaa | xxx", "~ bbb | yyy"]);
        assert_eq!(d.changed, 2);
    }

    /// The trimming of the shared head and suffix must not lose or duplicate
    /// lines; every line of both files has to appear exactly once.
    #[test]
    fn every_line_of_both_files_appears_exactly_once() {
        let a = lines(&["h1", "h2", "a", "b", "c", "t1", "t2"]);
        let b = lines(&["h1", "h2", "a", "B", "c", "d", "t1", "t2"]);
        let d = diff_lines(&a, &b);

        let mut left = Vec::new();
        let mut right = Vec::new();
        for r in &d.rows {
            match r {
                Row::Same { left: l, right: rr } | Row::Changed { left: l, right: rr } => {
                    left.push(l.text.clone());
                    right.push(rr.text.clone());
                }
                Row::Removed { left: l } => left.push(l.text.clone()),
                Row::Added { right: rr } => right.push(rr.text.clone()),
                Row::Skipped { .. } => {}
            }
        }
        assert_eq!(left, a);
        assert_eq!(right, b);
    }

    /// Line numbers must stay correct on the far side of the trimmed suffix,
    /// which is the easy place to be off by the length of the trim.
    #[test]
    fn suffix_line_numbers_are_not_shifted_by_the_trim() {
        let a = lines(&["a", "x", "t1", "t2"]);
        let b = lines(&["a", "t1", "t2"]);
        let d = diff_lines(&a, &b);
        let last = d.rows.last().unwrap();
        match last {
            Row::Same { left, right } => {
                assert_eq!((left.no, right.no), (4, 3), "each file's own numbering");
                assert_eq!(left.text, "t2");
            }
            other => panic!("expected the shared tail last, got {:?}", other),
        }
    }

    #[test]
    fn folding_hides_long_agreeing_runs_but_keeps_context() {
        let mut a: Vec<String> = (0..20).map(|i| format!("line {}", i)).collect();
        let b = a.clone();
        a[10] = "changed".to_string();
        let d = diff_lines(&a, &b);
        let folded = fold(&d.rows, CONTEXT);

        assert_eq!(
            render(&folded),
            vec![
                "... 7 lines",
                "  line 7",
                "  line 8",
                "  line 9",
                "~ changed | line 10",
                "  line 11",
                "  line 12",
                "  line 13",
                "... 6 lines",
            ]
        );
    }

    #[test]
    fn folding_an_identical_pair_leaves_one_marker() {
        let a = lines(&["a", "b", "c"]);
        let d = diff_lines(&a, &a);
        assert_eq!(fold(&d.rows, CONTEXT), vec![Row::Skipped { lines: 3 }]);
    }

    #[test]
    fn folding_a_small_file_hides_nothing() {
        let a = lines(&["a", "b"]);
        let b = lines(&["a", "B"]);
        let d = diff_lines(&a, &b);
        assert_eq!(fold(&d.rows, CONTEXT), d.rows, "nothing is far enough away to fold");
    }

    #[test]
    fn summaries_say_what_happened() {
        let d = diff_lines(&lines(&["a"]), &lines(&["a"]));
        assert_eq!(summary(&d), "identical");

        let d = diff_lines(&lines(&["a", "b"]), &lines(&["A", "b", "c"]));
        assert_eq!(summary(&d), "~1 +1");
    }

    #[test]
    fn a_middle_too_big_to_align_still_reports_the_files_differ() {
        // Past MAX_CELLS the table is skipped; the answer must still be true.
        let a: Vec<String> = (0..2100).map(|i| format!("a{}", i)).collect();
        let b: Vec<String> = (0..2100).map(|i| format!("b{}", i)).collect();
        let d = diff_lines(&a, &b);
        assert!(d.too_large);
        assert!(!d.identical);
        assert_eq!(d.changed, 2100, "reported as a wholesale replacement");
        assert!(summary(&d).contains("too large"));
    }

    /// A shared head and tail get trimmed before the size check, so two big
    /// files differing in one line are aligned properly rather than written
    /// off as too large.
    #[test]
    fn big_files_with_a_small_difference_are_still_aligned() {
        let mut a: Vec<String> = (0..50_000).map(|i| format!("line {}", i)).collect();
        let b = a.clone();
        a[25_000] = "changed".to_string();
        let d = diff_lines(&a, &b);
        assert!(!d.too_large);
        assert_eq!((d.changed, d.added, d.removed), (1, 0, 0));
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;
    use std::fs;

    #[test]
    fn two_text_files_are_compared() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.txt");
        let b = d.path().join("b.txt");
        fs::write(&a, "one\ntwo\nthree\n").unwrap();
        fs::write(&b, "one\nTWO\nthree\n").unwrap();
        let r = diff_files(&a, &b).unwrap();
        assert_eq!(r.changed, 1);
        assert!(!r.binary);
    }

    /// A hex dump compared line by line is noise: one inserted byte makes
    /// every row differ. Saying "they differ" is the useful answer.
    #[test]
    fn a_binary_file_is_reported_as_binary_rather_than_hex_diffed() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.bin");
        let b = d.path().join("b.bin");
        fs::write(&a, b"\x00\x01\x02").unwrap();
        fs::write(&b, b"\x00\x01\x03").unwrap();
        let r = diff_files(&a, &b).unwrap();
        assert!(r.binary);
        assert!(!r.identical);
        assert!(r.rows.is_empty());
        assert_eq!(summary(&r), "binary, differ");

        let r = diff_files(&a, &a).unwrap();
        assert!(r.binary && r.identical);
    }

    #[test]
    fn comparing_a_directory_fails_rather_than_panicking() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.txt");
        fs::write(&f, "x").unwrap();
        assert!(diff_files(&f, d.path()).is_err());
        assert!(diff_files(d.path(), &f).is_err());
    }

    #[test]
    fn a_missing_file_fails_rather_than_panicking() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.txt");
        fs::write(&f, "x").unwrap();
        assert!(diff_files(&f, &d.path().join("nope")).is_err());
    }
}
