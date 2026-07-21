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
