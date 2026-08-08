//! Whole-buffer text transforms — the "整形" half of an editor: sorting and
//! de-duplicating lines, converting between full- and half-width forms,
//! and normalising indentation.
//!
//! Every function here takes lines and returns lines, so the viewer can apply
//! one to a visual selection or to the whole file with the same call, and so
//! the rules are testable without a screen.
//!
//! The width conversions are the reason this module is not just a couple of
//! `sort()` calls. Japanese documents arrive with ASCII written full-width
//! (`ＡＢＣ１２３`) because some other tool did it, and with katakana written
//! half-width (`ｱｲｳ`) because a mainframe did — and the two want converting in
//! *opposite* directions. So "to half-width" means ASCII only and "to
//! full-width" means kana only, which is what a person actually wants when
//! they reach for either.

/// Sort `lines`, optionally descending. Comparison is by the text as written:
/// a "natural" order that understands embedded numbers is a different feature
/// and a surprising default.
pub fn sort(lines: &[String], descending: bool) -> Vec<String> {
    let mut out = lines.to_vec();
    out.sort();
    if descending {
        out.reverse();
    }
    out
}

/// Drop repeated lines, keeping the first of each. Unlike `uniq(1)` this does
/// not require the input to be sorted — in a document the duplicates are
/// scattered, and demanding a sort first would reorder something the user
/// wanted left alone.
pub fn uniq(lines: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    lines.iter().filter(|l| seen.insert((*l).clone())).cloned().collect()
}

/// Full-width ASCII → half-width, and the half-width katakana forms → their
/// full-width equivalents. In other words: make the Latin text normal, and
/// make the kana normal. Both are "the half-width one is wrong" in practice.
pub fn to_halfwidth(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // A half-width kana followed by a (semi-)voiced mark is one character.
        if let Some(base) = halfwidth_kana_base(c) {
            let next = chars.get(i + 1).copied();
            if let Some(comb) = next.and_then(|n| combine_kana(base, n)) {
                out.push(comb);
                i += 2;
                continue;
            }
            out.push(base);
            i += 1;
            continue;
        }
        out.push(match c {
            // The full-width ASCII block sits exactly 0xFEE0 above ASCII.
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ', // ideographic space
            other => other,
        });
        i += 1;
    }
    out
}

/// Half-width ASCII → full-width. The mirror of [`to_halfwidth`]'s ASCII half,
/// for the times a form or a fixed-width report wants full-width digits.
pub fn to_fullwidth(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '!'..='~' => char::from_u32(c as u32 + 0xFEE0).unwrap_or(c),
            ' ' => '\u{3000}',
            other => other,
        })
        .collect()
}

/// The full-width kana a half-width one maps to, ignoring voice marks.
fn halfwidth_kana_base(c: char) -> Option<char> {
    const TABLE: &[(char, char)] = &[
        ('｡', '。'), ('｢', '「'), ('｣', '」'), ('､', '、'), ('･', '・'),
        ('ｦ', 'ヲ'), ('ｧ', 'ァ'), ('ｨ', 'ィ'), ('ｩ', 'ゥ'), ('ｪ', 'ェ'), ('ｫ', 'ォ'),
        ('ｬ', 'ャ'), ('ｭ', 'ュ'), ('ｮ', 'ョ'), ('ｯ', 'ッ'), ('ｰ', 'ー'),
        ('ｱ', 'ア'), ('ｲ', 'イ'), ('ｳ', 'ウ'), ('ｴ', 'エ'), ('ｵ', 'オ'),
        ('ｶ', 'カ'), ('ｷ', 'キ'), ('ｸ', 'ク'), ('ｹ', 'ケ'), ('ｺ', 'コ'),
        ('ｻ', 'サ'), ('ｼ', 'シ'), ('ｽ', 'ス'), ('ｾ', 'セ'), ('ｿ', 'ソ'),
        ('ﾀ', 'タ'), ('ﾁ', 'チ'), ('ﾂ', 'ツ'), ('ﾃ', 'テ'), ('ﾄ', 'ト'),
        ('ﾅ', 'ナ'), ('ﾆ', 'ニ'), ('ﾇ', 'ヌ'), ('ﾈ', 'ネ'), ('ﾉ', 'ノ'),
        ('ﾊ', 'ハ'), ('ﾋ', 'ヒ'), ('ﾌ', 'フ'), ('ﾍ', 'ヘ'), ('ﾎ', 'ホ'),
        ('ﾏ', 'マ'), ('ﾐ', 'ミ'), ('ﾑ', 'ム'), ('ﾒ', 'メ'), ('ﾓ', 'モ'),
        ('ﾔ', 'ヤ'), ('ﾕ', 'ユ'), ('ﾖ', 'ヨ'),
        ('ﾗ', 'ラ'), ('ﾘ', 'リ'), ('ﾙ', 'ル'), ('ﾚ', 'レ'), ('ﾛ', 'ロ'),
        ('ﾜ', 'ワ'), ('ﾝ', 'ン'),
    ];
    TABLE.iter().find(|(h, _)| *h == c).map(|(_, f)| *f)
}

/// Fold a voiced (ﾞ) or semi-voiced (ﾟ) mark into the kana before it.
fn combine_kana(base: char, mark: char) -> Option<char> {
    let voiced = "カキクケコサシスセソタチツテトハヒフヘホ";
    let semi = "ハヒフヘホ";
    match mark {
        'ﾞ' | '\u{3099}' | '\u{309B}' => voiced
            .chars()
            .position(|c| c == base)
            .and_then(|_| char::from_u32(base as u32 + 1))
            .or(if base == 'ウ' { Some('ヴ') } else { None }),
        'ﾟ' | '\u{309A}' | '\u{309C}' => {
            semi.chars().position(|c| c == base).and_then(|_| char::from_u32(base as u32 + 2))
        }
        _ => None,
    }
}

/// Leading tabs → `width` spaces each. Only leading whitespace is touched: a
/// tab inside a line is usually a column separator in data, and expanding it
/// would silently change the file's meaning.
pub fn expand_tabs(lines: &[String], width: usize) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            let indent: String = l.chars().take_while(|c| *c == '\t' || *c == ' ').collect();
            let rest = &l[indent.len()..];
            let expanded: String = indent
                .chars()
                .map(|c| if c == '\t' { " ".repeat(width) } else { " ".to_string() })
                .collect();
            format!("{expanded}{rest}")
        })
        .collect()
}

/// Leading runs of `width` spaces → tabs. The inverse of [`expand_tabs`], with
/// the same "leading only" rule and for the same reason.
pub fn unexpand_tabs(lines: &[String], width: usize) -> Vec<String> {
    if width == 0 {
        return lines.to_vec();
    }
    lines
        .iter()
        .map(|l| {
            let spaces = l.chars().take_while(|c| *c == ' ').count();
            let rest: String = l.chars().skip(spaces).collect();
            format!("{}{}{}", "\t".repeat(spaces / width), " ".repeat(spaces % width), rest)
        })
        .collect()
}

/// Re-indent to a consistent step — VS Code's "reformat the indentation"
/// without knowing the language.
///
/// The nesting is read from the *existing* indentation: each distinct depth
/// that appears becomes one level, and levels are re-emitted at `width`
/// spaces each. That fixes a document indented 3-and-5 and 2 by different
/// hands without needing to parse it, and leaves blank lines blank.
pub fn reindent(lines: &[String], width: usize) -> Vec<String> {
    // The ladder of indent widths actually used, smallest first.
    let mut steps: Vec<usize> = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_width(l))
        .collect();
    steps.sort_unstable();
    steps.dedup();
    lines
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                return String::new();
            }
            let depth = steps.iter().position(|w| *w == indent_width(l)).unwrap_or(0);
            format!("{}{}", " ".repeat(depth * width), l.trim_start())
        })
        .collect()
}

/// The visual width of a line's leading whitespace, counting a tab as 8 —
/// the width every terminal and printer has agreed on for long enough that
/// mixed-indent files were laid out against it.
fn indent_width(line: &str) -> usize {
    let mut w = 0;
    for c in line.chars() {
        match c {
            ' ' => w += 1,
            '\t' => w = (w / 8 + 1) * 8,
            _ => break,
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blk(top: usize, bottom: usize, left: usize, right: usize) -> Block {
        Block { top, bottom, left, right }
    }

    /// The rectangle comes out of every line it covers, and a line too short
    /// to reach into it is left exactly as it was.
    #[test]
    fn block_delete_cuts_a_column_and_spares_short_lines() {
        let src = v(&["abcdef", "abcdef", "ab", "abcdef"]);
        assert_eq!(
            block_delete(&src, blk(0, 3, 2, 4)),
            v(&["abef", "abef", "ab", "abef"]),
        );
        assert_eq!(block_text(&src, blk(0, 2, 2, 4)), v(&["cd", "cd", ""]));
    }

    /// Inserting down a column pads the short lines out, because the whole
    /// point is that the column lines up afterwards.
    #[test]
    fn block_insert_pads_short_lines_so_the_column_aligns() {
        let src = v(&["abcdef", "ab", ""]);
        assert_eq!(block_insert(&src, blk(0, 2, 3, 4), "# "), v(&["abc# def", "ab # ", "   # "]));
        // Appending works at the right edge, padding the same way.
        assert_eq!(block_append(&v(&["ab", "abcd"]), blk(0, 1, 0, 3), "|"), v(&["ab |", "abc|d"]));
    }

    /// Replace is a delete and an insert in one step — rewriting a column of
    /// values without doing it twice.
    #[test]
    fn block_replace_swaps_the_rectangle() {
        let src = v(&["id=001 x", "id=002 y", "id=0"]);
        assert_eq!(
            block_replace(&src, blk(0, 2, 3, 6), "999"),
            v(&["id=999 x", "id=999 y", "id=999"]),
        );
    }

    /// The rectangle is built from two cursor positions in any order, and
    /// includes the cell the anchor sits on.
    #[test]
    fn a_block_spans_both_cursors_inclusively() {
        assert_eq!(Block::between((3, 5), (1, 2)), blk(1, 3, 2, 6));
        assert_eq!(Block::between((1, 2), (3, 5)), blk(1, 3, 2, 6));
        assert_eq!(block_text(&v(&["abcdef"]), Block::between((0, 1), (0, 3))), v(&["bcd"]));
    }

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn sort_and_uniq_do_the_obvious_thing() {
        assert_eq!(sort(&v(&["c", "a", "b"]), false), v(&["a", "b", "c"]));
        assert_eq!(sort(&v(&["c", "a", "b"]), true), v(&["c", "b", "a"]));
        // Duplicates need not be adjacent, and the first of each survives in
        // place — a document's repeats are scattered, and sorting first would
        // reorder what the user wanted left alone.
        assert_eq!(uniq(&v(&["b", "a", "b", "c", "a"])), v(&["b", "a", "c"]));
    }

    /// "To half-width" means the Latin text; "to full-width" means the kana.
    /// Both directions are what someone means when they reach for either.
    #[test]
    fn width_conversion_goes_the_way_people_mean() {
        assert_eq!(to_halfwidth("ＡＢＣ１２３（ｘ）"), "ABC123(x)");
        assert_eq!(to_halfwidth("全角\u{3000}空白"), "全角 空白");
        assert_eq!(to_fullwidth("ABC123"), "ＡＢＣ１２３");
        // Kana: half-width in, full-width out — including the voice marks,
        // which are separate characters half-width and joined full-width.
        assert_eq!(to_halfwidth("ｱｲｳ"), "アイウ");
        assert_eq!(to_halfwidth("ｶﾞｷﾞﾊﾟﾋﾟ"), "ガギパピ");
        assert_eq!(to_halfwidth("ｳﾞ"), "ヴ");
        // Text already in the right form is left exactly as it is.
        assert_eq!(to_halfwidth("すでに正しい text"), "すでに正しい text");
    }

    #[test]
    fn tab_conversion_touches_only_the_indent() {
        // A tab inside the line is a column separator in data; expanding it
        // would silently change what the file means.
        assert_eq!(expand_tabs(&v(&["\ta\tb"]), 4), v(&["    a\tb"]));
        assert_eq!(unexpand_tabs(&v(&["    a b"]), 4), v(&["\ta b"]));
        // A partial step keeps its leftover spaces rather than rounding.
        assert_eq!(unexpand_tabs(&v(&["      x"]), 4), v(&["\t  x"]));
    }

    /// Re-indent reads the nesting from what is there, so a file indented by
    /// three different hands comes out on one ladder.
    #[test]
    fn reindent_rebuilds_a_consistent_ladder() {
        let src = v(&["top", "   three", "        eight", "   three again", "", "top again"]);
        assert_eq!(
            reindent(&src, 2),
            v(&["top", "  three", "    eight", "  three again", "", "top again"]),
        );
        // A tab counts as eight columns, which is the width the mixed file
        // was laid out against in the first place.
        assert_eq!(reindent(&v(&["a", "\tb"]), 4), v(&["a", "    b"]));
    }
}

/// A rectangle over the buffer: whole lines `top..=bottom`, columns
/// `left..right` (end-exclusive) within each.
///
/// Lines shorter than `left` are the awkward case every block editor has to
/// answer. cian's answer: a delete leaves them alone (there is nothing inside
/// the rectangle to remove) and an insert pads them out with spaces to reach
/// the column, because the point of inserting down a column is that the
/// column lines up afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    pub top: usize,
    pub bottom: usize,
    pub left: usize,
    pub right: usize,
}

impl Block {
    /// The rectangle between two cursor positions, in any order.
    pub fn between(a: (usize, usize), b: (usize, usize)) -> Block {
        Block {
            top: a.0.min(b.0),
            bottom: a.0.max(b.0),
            left: a.1.min(b.1),
            right: a.1.max(b.1) + 1, // the anchor cell is inside the block
        }
    }
}

/// Cut the rectangle out of each line. Short lines keep what they have.
pub fn block_delete(lines: &[String], b: Block) -> Vec<String> {
    edit_block(lines, b, |chars, left, right| {
        if left >= chars.len() {
            return; // nothing of this line lies inside the rectangle
        }
        chars.drain(left..right.min(chars.len()));
    })
}

/// Insert `text` at the rectangle's left edge on every line, padding short
/// lines with spaces so the inserted column actually lines up.
pub fn block_insert(lines: &[String], b: Block, text: &str) -> Vec<String> {
    edit_block(lines, b, |chars, left, _| {
        while chars.len() < left {
            chars.push(' ');
        }
        for (i, c) in text.chars().enumerate() {
            chars.insert(left + i, c);
        }
    })
}

/// Append `text` at the rectangle's right edge on every line, padding to
/// reach it. The block equivalent of vim's `A`, for adding a trailing column.
pub fn block_append(lines: &[String], b: Block, text: &str) -> Vec<String> {
    edit_block(lines, b, |chars, _, right| {
        while chars.len() < right {
            chars.push(' ');
        }
        for (i, c) in text.chars().enumerate() {
            chars.insert(right + i, c);
        }
    })
}

/// Replace the rectangle's contents with `text` on every line: a delete and
/// an insert in one step, which is how a column of values gets rewritten.
pub fn block_replace(lines: &[String], b: Block, text: &str) -> Vec<String> {
    edit_block(lines, b, |chars, left, right| {
        if left < chars.len() {
            chars.drain(left..right.min(chars.len()));
        }
        while chars.len() < left {
            chars.push(' ');
        }
        for (i, c) in text.chars().enumerate() {
            chars.insert(left + i, c);
        }
    })
}

/// The rectangle's contents, one line per row — what a block yank copies.
pub fn block_text(lines: &[String], b: Block) -> Vec<String> {
    (b.top..=b.bottom)
        .filter_map(|i| lines.get(i))
        .map(|l| {
            let chars: Vec<char> = l.chars().collect();
            if b.left >= chars.len() {
                String::new()
            } else {
                chars[b.left..b.right.min(chars.len())].iter().collect()
            }
        })
        .collect()
}

/// Run `f` over each line the block covers, as chars, and rebuild.
fn edit_block(
    lines: &[String],
    b: Block,
    f: impl Fn(&mut Vec<char>, usize, usize),
) -> Vec<String> {
    let mut out = lines.to_vec();
    for i in b.top..=b.bottom.min(out.len().saturating_sub(1)) {
        let mut chars: Vec<char> = out[i].chars().collect();
        f(&mut chars, b.left, b.right);
        out[i] = chars.into_iter().collect();
    }
    out
}
