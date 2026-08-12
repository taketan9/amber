//! vi's grammar: `{count}{operator}{count}{motion}` and `{operator}{i|a}{object}`.
//!
//! The point of vi is that motions and operators are separate words that
//! multiply: learn `w`, `}`, `f,` and `iw` once, and `d`, `c` and `y` each
//! gain all of them. This module is the half that can be worked out from the
//! text alone — where a motion lands, and what a text object covers — as pure
//! functions over the buffer, so the viewer is left with the small job of
//! remembering which operator is waiting and applying the result.
//!
//! Positions are `(line, column)` in *characters*, matching the viewer's own
//! cursor. Nothing here touches the screen: `H`, `M`, `L` and the `z` family
//! are viewport motions and live where the viewport is known.

/// What a motion covers when an operator is waiting on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sweep {
    /// From the cursor up to, but not including, the target — `w`, `0`, `%`
    /// backwards. The everyday case.
    Exclusive,
    /// Up to and including the target — `e`, `f`, `$`. The difference is one
    /// character and it is the one everybody notices.
    Inclusive,
    /// Whole lines, whatever the columns were — `j`, `k`, `G`, `}`.
    Linewise,
}

/// Where a motion lands, and what it would sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Motion {
    pub to: (usize, usize),
    pub sweep: Sweep,
}

/// A range of text, as an operator sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Span {
    /// `start` up to and including `end`, both `(line, column)`.
    Chars { start: (usize, usize), end: (usize, usize) },
    /// Whole lines, inclusive.
    Lines { first: usize, last: usize },
}

fn chars_of(lines: &[String], l: usize) -> Vec<char> {
    lines.get(l).map(|s| s.chars().collect()).unwrap_or_default()
}

fn len_of(lines: &[String], l: usize) -> usize {
    lines.get(l).map(|s| s.chars().count()).unwrap_or(0)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where `key` takes the cursor from `at`, repeated `count` times.
///
/// `arg` is the character a `f` / `t` family motion was given. Returns `None`
/// for a key that is not a motion, so the caller can treat it as "not part of
/// this command" and put the operator down.
pub(crate) fn motion(
    lines: &[String],
    at: (usize, usize),
    key: char,
    arg: Option<char>,
    count: usize,
) -> Option<Motion> {
    let last_line = lines.len().saturating_sub(1);
    let (mut l, mut c) = at;
    let n = count.max(1);
    let m = |to, sweep| Some(Motion { to, sweep });
    match key {
        'h' => m((l, c.saturating_sub(n)), Sweep::Exclusive),
        'l' => m((l, (c + n).min(len_of(lines, l))), Sweep::Exclusive),
        'j' => m(((l + n).min(last_line), c), Sweep::Linewise),
        'k' => m((l.saturating_sub(n), c), Sweep::Linewise),
        '0' => m((l, 0), Sweep::Exclusive),
        '^' => {
            let cs = chars_of(lines, l);
            let first = cs.iter().position(|ch| !ch.is_whitespace()).unwrap_or(0);
            m((l, first), Sweep::Exclusive)
        }
        // `$` is inclusive: `d$` takes the last character with it.
        '$' => m((l, len_of(lines, l).saturating_sub(1)), Sweep::Inclusive),
        'G' => m((last_line, 0), Sweep::Linewise),
        // The capitals are the WORD forms: a word stops at punctuation, a
        // WORD runs to the next space. One pair of functions, one flag.
        'w' | 'W' => {
            let big = key == 'W';
            for _ in 0..n {
                let (nl, nc) =
                    crate::util::viewer_word_forward_big(lines, l, c, last_line, big);
                l = nl;
                c = nc;
            }
            m((l, c), Sweep::Exclusive)
        }
        'b' | 'B' => {
            let big = key == 'B';
            for _ in 0..n {
                let (nl, nc) = crate::util::viewer_word_back_big(lines, l, c, big);
                l = nl;
                c = nc;
            }
            m((l, c), Sweep::Exclusive)
        }
        // `e` / `E` — the end of this word, or of the next one when already
        // sitting on it.
        'e' | 'E' => {
            let big = key == 'E';
            for _ in 0..n {
                let (nl, nc) = crate::util::viewer_word_end_big(lines, l, c, big);
                l = nl;
                c = nc;
            }
            m((l, c), Sweep::Inclusive)
        }
        '{' | '}' => {
            let forward = key == '}';
            for _ in 0..n {
                l = paragraph(lines, l, forward);
            }
            m((l, 0), Sweep::Linewise)
        }
        'f' | 'F' | 't' | 'T' => {
            let want = arg?;
            let cs = chars_of(lines, l);
            let forward = key == 'f' || key == 't';
            let till = key == 't' || key == 'T';
            let mut pos = c;
            for _ in 0..n {
                pos = if forward {
                    let from = if till { pos + 2 } else { pos + 1 };
                    (from..cs.len()).find(|i| cs[*i] == want)?
                } else {
                    let upto = if till { pos.checked_sub(1)? } else { pos };
                    (0..upto).rev().find(|i| cs[*i] == want)?
                };
            }
            if till {
                pos = if forward { pos - 1 } else { pos + 1 };
            }
            // Forward is inclusive (`dfx` eats the `x`), backward is not.
            m((l, pos), if forward { Sweep::Inclusive } else { Sweep::Exclusive })
        }
        '%' => {
            let (nl, nc) = match_bracket(lines, l, c)?;
            m((nl, nc), Sweep::Inclusive)
        }
        _ => None,
    }
}

/// The line a paragraph motion lands on: the next (or previous) blank line,
/// or the end of the file.
pub(crate) fn paragraph(lines: &[String], from: usize, forward: bool) -> usize {
    let blank = |i: usize| lines.get(i).map(|l| l.trim().is_empty()).unwrap_or(true);
    let last = lines.len().saturating_sub(1);
    let mut i = from;
    if forward {
        i = (i + 1).min(last);
        while i < last && blank(i) {
            i += 1;
        }
        while i < last && !blank(i) {
            i += 1;
        }
    } else {
        i = i.saturating_sub(1);
        while i > 0 && blank(i) {
            i -= 1;
        }
        while i > 0 && !blank(i) {
            i -= 1;
        }
    }
    i
}

/// `%` — the bracket matching the one at or after the cursor.
fn match_bracket(lines: &[String], line: usize, col: usize) -> Option<(usize, usize)> {
    const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
    let cs = chars_of(lines, line);
    let mut start = col;
    while start < cs.len()
        && !PAIRS.iter().any(|(o, c)| *o == cs[start] || *c == cs[start])
    {
        start += 1;
    }
    let br = *cs.get(start)?;
    if let Some((_, close)) = PAIRS.iter().find(|(o, _)| *o == br) {
        let (mut depth, mut l, mut c) = (0i32, line, start);
        loop {
            let cs = chars_of(lines, l);
            while c < cs.len() {
                if cs[c] == br {
                    depth += 1;
                } else if cs[c] == *close {
                    depth -= 1;
                    if depth == 0 {
                        return Some((l, c));
                    }
                }
                c += 1;
            }
            l += 1;
            c = 0;
            if l >= lines.len() {
                return None;
            }
        }
    }
    let (open, _) = PAIRS.iter().find(|(_, c)| *c == br)?;
    let (mut depth, mut l, mut c) = (0i32, line, start as isize);
    loop {
        let cs = chars_of(lines, l);
        while c >= 0 {
            let ch = cs[c as usize];
            if ch == br {
                depth += 1;
            } else if ch == *open {
                depth -= 1;
                if depth == 0 {
                    return Some((l, c as usize));
                }
            }
            c -= 1;
        }
        l = l.checked_sub(1)?;
        c = chars_of(lines, l).len() as isize - 1;
    }
}

/// What `iw`, `a"`, `i(` … cover from `at`. `around` is the `a` form, which
/// takes the delimiters (or the trailing whitespace) with it.
pub(crate) fn text_object(
    lines: &[String],
    at: (usize, usize),
    around: bool,
    obj: char,
) -> Option<Span> {
    let (l, c) = at;
    let cs = chars_of(lines, l);
    match obj {
        // A word, or the run of whitespace the cursor is sitting in.
        'w' | 'W' => {
            if cs.is_empty() {
                return None;
            }
            let c = c.min(cs.len() - 1);
            let same = |a: char, b: char| {
                if obj == 'W' {
                    a.is_whitespace() == b.is_whitespace()
                } else {
                    is_word(a) == is_word(b) && a.is_whitespace() == b.is_whitespace()
                }
            };
            let here = cs[c];
            let mut s = c;
            while s > 0 && same(cs[s - 1], here) {
                s -= 1;
            }
            let mut e = c;
            while e + 1 < cs.len() && same(cs[e + 1], here) {
                e += 1;
            }
            if around {
                // `aw` takes the whitespace after the word, or before it when
                // there is none after — vi's rule, and the one that makes
                // `daw` in a list leave a tidy line.
                let mut e2 = e;
                while e2 + 1 < cs.len() && cs[e2 + 1].is_whitespace() {
                    e2 += 1;
                }
                if e2 == e {
                    while s > 0 && cs[s - 1].is_whitespace() {
                        s -= 1;
                    }
                } else {
                    e = e2;
                }
            }
            Some(Span::Chars { start: (l, s), end: (l, e) })
        }
        // A quoted run, on this line: the quotes are not nested, so the pair
        // is simply the one the cursor is between.
        '"' | '\'' | '`' => {
            let q = obj;
            let mut opens: Vec<usize> = Vec::new();
            for (i, ch) in cs.iter().enumerate() {
                if *ch == q && (i == 0 || cs[i - 1] != '\\') {
                    opens.push(i);
                }
            }
            let pair = opens.chunks(2).find(|p| p.len() == 2 && c <= p[1])?;
            let (a, b) = (pair[0], pair[1]);
            if around {
                Some(Span::Chars { start: (l, a), end: (l, b) })
            } else if b > a + 1 {
                Some(Span::Chars { start: (l, a + 1), end: (l, b - 1) })
            } else {
                None
            }
        }
        // A bracketed run, across lines, honouring nesting.
        '(' | ')' | 'b' | '[' | ']' | '{' | '}' | 'B' | '<' | '>' => {
            let (open, close) = match obj {
                '(' | ')' | 'b' => ('(', ')'),
                '[' | ']' => ('[', ']'),
                '<' | '>' => ('<', '>'),
                _ => ('{', '}'),
            };
            let (sl, sc) = enclosing(lines, at, open, close, false)?;
            let (el, ec) = enclosing(lines, at, open, close, true)?;
            if around {
                return Some(Span::Chars { start: (sl, sc), end: (el, ec) });
            }
            // Inside: step in from each bracket, and give up when there is
            // nothing between them.
            let start = if sc + 1 < len_of(lines, sl) {
                (sl, sc + 1)
            } else {
                (sl + 1, 0)
            };
            let end = if ec > 0 {
                (el, ec - 1)
            } else {
                (el.checked_sub(1)?, len_of(lines, el.saturating_sub(1)).saturating_sub(1))
            };
            // A block whose braces are alone at the ends of their lines is
            // whole lines, as it is in vi: `di{` on a function body leaves the
            // braces on adjacent lines rather than an empty one between them.
            if start.0 > sl && end.0 < el && start.0 <= end.0 {
                return Some(Span::Lines { first: start.0, last: end.0 });
            }
            (start.0 < end.0 || (start.0 == end.0 && start.1 <= end.1))
                .then_some(Span::Chars { start, end })
        }
        _ => None,
    }
}

/// The `open` before the cursor or the `close` after it, at nesting depth 0.
fn enclosing(
    lines: &[String],
    at: (usize, usize),
    open: char,
    close: char,
    forward: bool,
) -> Option<(usize, usize)> {
    let (mut l, mut c) = (at.0 as isize, at.1 as isize);
    let mut depth = 0i32;
    loop {
        let cs = chars_of(lines, l as usize);
        while c >= 0 && (c as usize) < cs.len() {
            let ch = cs[c as usize];
            if forward {
                if ch == open && (l, c) != (at.0 as isize, at.1 as isize) {
                    depth += 1;
                } else if ch == close {
                    if depth == 0 {
                        return Some((l as usize, c as usize));
                    }
                    depth -= 1;
                }
                c += 1;
            } else {
                if ch == close && (l, c) != (at.0 as isize, at.1 as isize) {
                    depth += 1;
                } else if ch == open {
                    if depth == 0 {
                        return Some((l as usize, c as usize));
                    }
                    depth -= 1;
                }
                c -= 1;
            }
        }
        if forward {
            l += 1;
            if l as usize >= lines.len() {
                return None;
            }
            c = 0;
        } else {
            l -= 1;
            if l < 0 {
                return None;
            }
            c = chars_of(lines, l as usize).len() as isize - 1;
        }
    }
}

/// The span an operator covers, given where the cursor is and where the
/// motion landed.
pub(crate) fn span_of(at: (usize, usize), mo: Motion) -> Span {
    match mo.sweep {
        Sweep::Linewise => Span::Lines {
            first: at.0.min(mo.to.0),
            last: at.0.max(mo.to.0),
        },
        Sweep::Inclusive | Sweep::Exclusive => {
            let (mut s, mut e) = (at, mo.to);
            let backwards = e.0 < s.0 || (e.0 == s.0 && e.1 < s.1);
            if backwards {
                std::mem::swap(&mut s, &mut e);
            }
            // An exclusive motion stops one short of where it landed — unless
            // it went backwards, where the cursor's own character is the one
            // left out instead.
            if mo.sweep == Sweep::Exclusive {
                if backwards {
                    if e.1 > 0 {
                        e.1 -= 1;
                    } else {
                        return Span::Chars { start: s, end: e };
                    }
                } else if e.1 > 0 {
                    e.1 -= 1;
                } else if e.0 > s.0 {
                    // Landed at the start of a later line: stop at the end of
                    // the previous one.
                    e = (e.0 - 1, usize::MAX);
                }
            }
            Span::Chars { start: s, end: e }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn motions_land_where_vi_lands() {
        let b = buf("alpha beta gamma\nsecond line\n");
        // `w` is exclusive, `e` inclusive — the difference `dw` and `de` show.
        assert_eq!(motion(&b, (0, 0), 'w', None, 1).unwrap().to, (0, 6));
        assert_eq!(motion(&b, (0, 0), 'w', None, 1).unwrap().sweep, Sweep::Exclusive);
        assert_eq!(motion(&b, (0, 0), 'e', None, 1).unwrap().to, (0, 4));
        assert_eq!(motion(&b, (0, 0), 'e', None, 1).unwrap().sweep, Sweep::Inclusive);
        // A count multiplies it.
        assert_eq!(motion(&b, (0, 0), 'w', None, 2).unwrap().to, (0, 11));
        // `$` includes the last character, `0` does not include the cursor's.
        assert_eq!(motion(&b, (0, 3), '$', None, 1).unwrap().to, (0, 15));
        assert_eq!(motion(&b, (0, 3), '0', None, 1).unwrap().to, (0, 0));
        // `f` lands on the character, `t` before it.
        assert_eq!(motion(&b, (0, 0), 'f', Some('g'), 1).unwrap().to, (0, 11));
        assert_eq!(motion(&b, (0, 0), 't', Some('g'), 1).unwrap().to, (0, 10));
        assert_eq!(motion(&b, (0, 8), 'F', Some('a'), 1).unwrap().to, (0, 4));
        // A `f` for a character that is not there is not a motion at all.
        assert!(motion(&b, (0, 0), 'f', Some('Z'), 1).is_none());
        // j/k are linewise however far along the line the cursor is.
        assert_eq!(motion(&b, (0, 4), 'j', None, 1).unwrap().sweep, Sweep::Linewise);
        // A key that is not a motion says so.
        assert!(motion(&b, (0, 0), 'Z', None, 1).is_none());
    }

    #[test]
    fn a_word_object_is_the_word_and_a_around_takes_the_space() {
        let b = buf("alpha beta gamma\n");
        assert_eq!(
            text_object(&b, (0, 7), false, 'w'),
            Some(Span::Chars { start: (0, 6), end: (0, 9) }),
            "iw from inside `beta`",
        );
        assert_eq!(
            text_object(&b, (0, 7), true, 'w'),
            Some(Span::Chars { start: (0, 6), end: (0, 10) }),
            "aw takes the space after it",
        );
    }

    #[test]
    fn quotes_and_brackets_are_found_around_the_cursor() {
        let b = buf("value = \"some text\";\n");
        assert_eq!(
            text_object(&b, (0, 12), false, '"'),
            Some(Span::Chars { start: (0, 9), end: (0, 17) }),
            "i\" is what is between them",
        );
        assert_eq!(
            text_object(&b, (0, 12), true, '"'),
            Some(Span::Chars { start: (0, 8), end: (0, 18) }),
            "a\" takes the quotes",
        );

        let b = buf("call(one, two)\n");
        assert_eq!(
            text_object(&b, (0, 6), false, '(',),
            Some(Span::Chars { start: (0, 5), end: (0, 12) }),
        );
        assert_eq!(
            text_object(&b, (0, 6), true, ')'),
            Some(Span::Chars { start: (0, 4), end: (0, 13) }),
        );

        // Nesting, across lines.
        let b = buf("fn f() {\n    if x {\n        y();\n    }\n}\n");
        assert_eq!(
            text_object(&b, (2, 8), true, '{'),
            Some(Span::Chars { start: (1, 9), end: (3, 4) }),
            "the inner block, not the outer one",
        );
    }

    #[test]
    fn a_span_is_what_the_operator_takes() {
        // `dw` on "alpha beta" leaves "beta": exclusive, so up to but not
        // including where `w` landed.
        let b = buf("alpha beta\n");
        let mo = motion(&b, (0, 0), 'w', None, 1).unwrap();
        assert_eq!(span_of((0, 0), mo), Span::Chars { start: (0, 0), end: (0, 5) });
        // `de` takes the word itself.
        let mo = motion(&b, (0, 0), 'e', None, 1).unwrap();
        assert_eq!(span_of((0, 0), mo), Span::Chars { start: (0, 0), end: (0, 4) });
        // `dj` is two whole lines.
        let b = buf("one\ntwo\nthree\n");
        let mo = motion(&b, (0, 1), 'j', None, 1).unwrap();
        assert_eq!(span_of((0, 1), mo), Span::Lines { first: 0, last: 1 });
        // A backwards motion sweeps back to the cursor.
        let mo = motion(&b, (1, 2), 'b', None, 1).unwrap();
        assert_eq!(span_of((1, 2), mo), Span::Chars { start: (1, 0), end: (1, 1) });
    }
}
