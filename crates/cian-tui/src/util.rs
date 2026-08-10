//! Pure, self-contained helpers shared across the TUI: display wrapping and
//! padding, and the F3 viewer's cursor/selection geometry. Nothing here touches
//! `App` state, so it lives apart from the main file for readability.

use unicode_width::UnicodeWidthStr;
use ratatui::layout::Rect;

/// Break `s` into chunks no wider than `width` display columns, on the char
/// boundary (no hyphenation). A blank string yields one empty chunk so the line
/// still takes a row.
pub(crate) fn wrap_str(s: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    if s.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = UnicodeWidthStr::width(ch.to_string().as_str()).max(1);
        if w + cw > width && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            w = 0;
        }
        cur.push(ch);
        w += cw;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The ASCII key a Japanese IME sends in place of `c`, if any.
///
/// With the IME on, a letter never reaches cian at all — it is held in the
/// terminal's own composition until it is committed, which is why single-key
/// commands go dead. Punctuation is different: it commits as it is typed, and
/// arrives as its full-width or kana form. `：` *is* the colon key being
/// pressed, so where a keystroke is a command rather than text, it is read as
/// one. Text fields never go through this — a file may legitimately be named
/// with a full-width colon, and on Windows it must be.
pub(crate) fn fold_ime_key(c: char) -> Option<char> {
    // The full-width block sits exactly 0xFEE0 above its ASCII twin.
    if ('！'..='～').contains(&c) {
        return char::from_u32(c as u32 - 0xFEE0);
    }
    Some(match c {
        '　' => ' ',
        '。' => '.',
        '、' => ',',
        // The kana layout's own punctuation: these keys carry no other
        // meaning in a command context, so reading them as the key that was
        // pressed costs nothing.
        '・' => '/',
        '「' => '[',
        '」' => ']',
        'ー' => '-',
        _ => return None,
    })
}

/// Fold a whole word to ASCII the same way. For the `:` command line's verb,
/// which is ASCII by definition — its arguments are left exactly as typed.
pub(crate) fn fold_ime_word(s: &str) -> String {
    s.chars().map(|c| fold_ime_key(c).unwrap_or(c)).collect()
}

/// The word under `col` in `line`, as `*` and `#` take it: a run of letters,
/// digits and underscores. `None` when the cursor is not on one — searching
/// for the space you are standing in has no meaning.
pub(crate) fn word_under_cursor(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    if chars.is_empty() {
        return None;
    }
    // On a non-word character, vi looks forward to the next word on the line.
    let mut i = col.min(chars.len() - 1);
    while i < chars.len() && !is_word(chars[i]) {
        i += 1;
    }
    if i >= chars.len() {
        return None;
    }
    let mut s = i;
    while s > 0 && is_word(chars[s - 1]) {
        s -= 1;
    }
    let mut e = i;
    while e + 1 < chars.len() && is_word(chars[e + 1]) {
        e += 1;
    }
    Some(chars[s..=e].iter().collect())
}

/// The length in characters of viewer line `l` (0 if out of range).
pub(crate) fn vlen(view: &cian_core::viewer::View, l: usize) -> usize {
    view.lines.get(l).map(|s| s.chars().count()).unwrap_or(0)
}

/// `w` / `b` over a plain buffer, for [`crate::vim`], which works on the
/// lines rather than on a `View`.
pub(crate) fn viewer_word_forward_view(
    lines: &[String],
    line: usize,
    col: usize,
    last: usize,
) -> (usize, usize) {
    let chars: Vec<char> = lines.get(line).map(|s| s.chars().collect()).unwrap_or_default();
    let mut i = col;
    while i < chars.len() && !chars[i].is_whitespace() {
        i += 1;
    }
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i < chars.len() {
        return (line, i);
    }
    let mut l = line + 1;
    while l <= last {
        let c: Vec<char> = lines[l].chars().collect();
        let first = c.iter().position(|ch| !ch.is_whitespace()).unwrap_or(0);
        if !c.is_empty() {
            return (l, first);
        }
        l += 1;
    }
    (line, chars.len())
}

pub(crate) fn viewer_word_back_view(lines: &[String], line: usize, col: usize) -> (usize, usize) {
    let chars: Vec<char> = lines.get(line).map(|s| s.chars().collect()).unwrap_or_default();
    let mut i = col;
    if i == 0 {
        if line == 0 {
            return (0, 0);
        }
        let prev = line - 1;
        let c: Vec<char> = lines[prev].chars().collect();
        let mut j = c.len();
        while j > 0 && c[j - 1].is_whitespace() {
            j -= 1;
        }
        while j > 0 && !c[j - 1].is_whitespace() {
            j -= 1;
        }
        return (prev, j);
    }
    i -= 1;
    while i > 0 && chars[i].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !chars[i - 1].is_whitespace() {
        i -= 1;
    }
    (line, i)
}

/// `w`: the start of the next word, moving onto the next line when the current
/// one runs out. Words are runs of non-whitespace (a simplification of vim's
/// word/WORD split that reads naturally for code).
pub(crate) fn viewer_word_forward(
    view: &cian_core::viewer::View,
    line: usize,
    col: usize,
    last: usize,
) -> (usize, usize) {
    let chars: Vec<char> = view.lines.get(line).map(|s| s.chars().collect()).unwrap_or_default();
    let mut i = col;
    // Skip the rest of the current word, then any whitespace.
    while i < chars.len() && !chars[i].is_whitespace() {
        i += 1;
    }
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i < chars.len() {
        return (line, i);
    }
    // Fell off the end: first non-blank of the next non-empty line.
    let mut l = line + 1;
    while l <= last {
        let c: Vec<char> = view.lines[l].chars().collect();
        let first = c.iter().position(|ch| !ch.is_whitespace()).unwrap_or(0);
        if !c.is_empty() {
            return (l, first);
        }
        l += 1;
    }
    (line, chars.len())
}

/// `b`: the start of the current or previous word.
pub(crate) fn viewer_word_back(view: &cian_core::viewer::View, line: usize, col: usize) -> (usize, usize) {
    let chars: Vec<char> = view.lines.get(line).map(|s| s.chars().collect()).unwrap_or_default();
    let mut i = col;
    if i == 0 {
        // At the line start: end of the previous line's last word.
        if line == 0 {
            return (0, 0);
        }
        let prev = line - 1;
        let c: Vec<char> = view.lines[prev].chars().collect();
        let mut j = c.len();
        while j > 0 && c[j - 1].is_whitespace() {
            j -= 1;
        }
        while j > 0 && !c[j - 1].is_whitespace() {
            j -= 1;
        }
        return (prev, j);
    }
    i -= 1;
    while i > 0 && chars[i].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !chars[i - 1].is_whitespace() {
        i -= 1;
    }
    (line, i)
}

/// `%`: from the bracket at or after the cursor on its line, the matching
/// bracket, scanning across lines and honouring nesting. `None` if there is no
/// bracket to jump from or its pair is unbalanced.
pub(crate) fn viewer_match_bracket(
    view: &cian_core::viewer::View,
    line: usize,
    col: usize,
) -> Option<(usize, usize)> {
    const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
    let opener = |c: char| PAIRS.iter().find(|(o, _)| *o == c).map(|(_, cl)| *cl);
    let closer = |c: char| PAIRS.iter().find(|(_, cl)| *cl == c).map(|(o, _)| *o);

    let chars: Vec<char> = view.lines.get(line)?.chars().collect();
    // Find the bracket to jump from: the one under the cursor, else the next on
    // the line.
    let mut start = col;
    while start < chars.len() && opener(chars[start]).is_none() && closer(chars[start]).is_none() {
        start += 1;
    }
    let br = *chars.get(start)?;

    if let Some(want_close) = opener(br) {
        // Scan forward for the matching closer.
        let mut depth = 0i32;
        let mut l = line;
        let mut c = start;
        loop {
            let cs: Vec<char> = view.lines.get(l).map(|s| s.chars().collect()).unwrap_or_default();
            while c < cs.len() {
                if cs[c] == br {
                    depth += 1;
                } else if cs[c] == want_close {
                    depth -= 1;
                    if depth == 0 {
                        return Some((l, c));
                    }
                }
                c += 1;
            }
            l += 1;
            c = 0;
            if l >= view.lines.len() {
                return None;
            }
        }
    } else if let Some(want_open) = closer(br) {
        // Scan backward for the matching opener.
        let mut depth = 0i32;
        let mut l = line as isize;
        let mut c = start as isize;
        loop {
            let cs: Vec<char> = view.lines.get(l as usize).map(|s| s.chars().collect()).unwrap_or_default();
            while c >= 0 {
                let ch = cs[c as usize];
                if ch == br {
                    depth += 1;
                } else if ch == want_open {
                    depth -= 1;
                    if depth == 0 {
                        return Some((l as usize, c as usize));
                    }
                }
                c -= 1;
            }
            l -= 1;
            if l < 0 {
                return None;
            }
            c = view.lines.get(l as usize).map(|s| s.chars().count() as isize - 1).unwrap_or(-1);
        }
    } else {
        None
    }
}

/// `{` / `}`: the previous/next blank line (paragraph boundary).
pub(crate) fn viewer_paragraph(view: &cian_core::viewer::View, line: usize, forward: bool) -> usize {
    let blank = |l: usize| view.lines.get(l).map(|s| s.trim().is_empty()).unwrap_or(true);
    let last = view.lines.len().saturating_sub(1);
    if forward {
        let mut l = line + 1;
        while l < last && !blank(l) {
            l += 1;
        }
        l.min(last)
    } else {
        if line == 0 {
            return 0;
        }
        let mut l = line - 1;
        while l > 0 && !blank(l) {
            l -= 1;
        }
        l
    }
}

/// The next/previous match of `query` (case-insensitive substring) from `from`,
/// wrapping around the file. Returns the match's start `(line, col)`.
pub(crate) fn viewer_find(
    view: &cian_core::viewer::View,
    from: (usize, usize),
    query: &str,
    forward: bool,
) -> Option<(usize, usize)> {
    if query.is_empty() || view.lines.is_empty() {
        return None;
    }
    // Same pattern language as `:find`/`:grep`: `/re/` is a regex, anything
    // else a case-insensitive substring. A pattern that no longer parses
    // (should not happen — the prompt rejects it) simply matches nothing.
    let matcher = cian_core::search::Matcher::parse(query).ok()?;
    let n = view.lines.len();

    if forward {
        // Current line after the cursor, then following lines, then wrap.
        for step in 0..=n {
            let l = (from.0 + step) % n;
            let first = if step == 0 { from.1 + 1 } else { 0 };
            if let Some((s, _)) =
                matcher.find_ranges(&view.lines[l]).into_iter().find(|(s, _)| *s >= first)
            {
                return Some((l, s));
            }
        }
    } else {
        for step in 0..=n {
            let l = (from.0 + n - (step % n)) % n;
            // On the cursor line, only matches strictly before the cursor.
            let limit = if step == 0 { from.1 } else { usize::MAX };
            if let Some((s, _)) = matcher
                .find_ranges(&view.lines[l])
                .into_iter()
                .rev()
                .find(|(s, _)| *s < limit)
            {
                return Some((l, s));
            }
        }
    }
    None
}

/// Order two `(line, col)` positions so the earlier one comes first.
pub(crate) fn order_pos(a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Char-wise text between two ordered positions, end-inclusive (vim `v` yank).
pub(crate) fn viewer_charwise(lines: &[String], s: (usize, usize), e: (usize, usize)) -> String {
    let take = |l: usize, from: usize, to_incl: Option<usize>| -> String {
        let chars: Vec<char> = lines.get(l).map(|x| x.chars().collect()).unwrap_or_default();
        let end = match to_incl {
            Some(c) => (c + 1).min(chars.len()),
            None => chars.len(),
        };
        let start = from.min(end);
        chars[start..end].iter().collect()
    };
    if s.0 == e.0 {
        return take(s.0, s.1, Some(e.1));
    }
    let mut out = vec![take(s.0, s.1, None)];
    for l in (s.0 + 1)..e.0 {
        out.push(lines.get(l).cloned().unwrap_or_default());
    }
    out.push(take(e.0, 0, Some(e.1)));
    out.join("\n")
}

/// Shorten to `max` display cells, ending with `…` if anything was cut.
///
/// Cells, not characters, for the reason [`width`] gives: a 28-character
/// Japanese name is 56 columns wide, so cutting by `chars().count()` leaves
/// whatever follows it in the row misaligned and pushed off the right edge.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    // One cell goes to the ellipsis.
    let (mut out, mut used) = (String::new(), 0usize);
    for c in s.chars() {
        let cw = width(c.to_string().as_str());
        if used + cw > max - 1 {
            break;
        }
        used += cw;
        out.push(c);
    }
    out.push('…');
    out
}

/// Truncate to `w` display cells and pad back out to exactly `w`.
///
/// The column idiom. `format!("{:<w$}", …)` pads by character count, so pairing
/// it with a width-aware truncate still misaligns wide text — the two halves
/// must agree on the unit, and this keeps them together.
pub(crate) fn fit(s: &str, w: usize) -> String {
    pad_to(&truncate(s, w), w)
}

/// Display width of a string in terminal cells.
///
/// Not `chars().count()`: CJK characters occupy two cells, so a Japanese
/// shortcut name padded by character count pushes everything after it out of
/// alignment and off the right edge.
/// Make a line of a file safe to draw somewhere that is not the viewer.
///
/// A tab written into a cell is sent to the terminal as a tab: the cursor
/// jumps and the cells it skipped keep whatever was in them, which is how the
/// tail of a Makefile stayed on screen underneath the next file previewed.
/// Only the viewer expands tabs properly, against its own column arithmetic;
/// everywhere else — previews, result lists, diff rows — wants this.
pub(crate) fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\t' => {
                let at = width(&out);
                let stop = cian_core::viewer::tab_width();
                out.push_str(&" ".repeat(stop - (at % stop)));
            }
            // Any other control character would move the cursor or change the
            // colours; a visible stand-in says something is there.
            c if c.is_control() => out.push('·'),
            c => out.push(c),
        }
    }
    out
}

pub(crate) fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Pad to `w` display cells, accounting for wide characters.
pub(crate) fn pad_to(s: &str, w: usize) -> String {
    let mut out = s.to_string();
    for _ in width(s)..w {
        out.push(' ');
    }
    out
}

/// Right-align within `w` display cells (pad on the left).
pub(crate) fn pad_left(s: &str, w: usize) -> String {
    format!("{}{}", " ".repeat(w.saturating_sub(width(s))), s)
}

/// Shorten from the middle, keeping both ends.
///
/// Paths and URLs carry their meaning at opposite ends — the final directory
/// of one, the host of the other — so cutting either end loses what identifies
/// it. Removing the middle keeps both.
pub(crate) fn truncate_middle(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    if max <= 3 {
        return truncate(s, max);
    }
    // Budget in display cells from each end, so wide characters cost two.
    let keep = max - 1;
    let (head_budget, tail_budget) = (keep.div_ceil(2), keep / 2);
    let take_from = |it: &mut dyn Iterator<Item = char>, budget: usize| -> String {
        let (mut out, mut used) = (String::new(), 0usize);
        for c in it {
            let cw = UnicodeWidthStr::width(c.to_string().as_str());
            if used + cw > budget {
                break;
            }
            used += cw;
            out.push(c);
        }
        out
    };
    let h = take_from(&mut s.chars(), head_budget);
    let t: String = take_from(&mut s.chars().rev(), tail_budget).chars().rev().collect();
    format!("{}…{}", h, t)
}

pub(crate) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// The smallest rect containing both. Zero-sized inputs are ignored so an
/// absent surface (e.g. while zoomed) does not drag the union to the origin.
pub(crate) fn union_rect(a: Rect, b: Rect) -> Rect {
    if a.width == 0 || a.height == 0 {
        return b;
    }
    if b.width == 0 || b.height == 0 {
        return a;
    }
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let r = (a.x + a.width).max(b.x + b.width);
    let bo = (a.y + a.height).max(b.y + b.height);
    Rect { x, y, width: r - x, height: bo - y }
}
