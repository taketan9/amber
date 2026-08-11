//! Search-and-replace over a buffer of lines — the `:s/old/new/` behind the
//! F3 viewer's replace.
//!
//! Deliberately pure: it takes lines and hands back the edited lines plus what
//! it changed, so the confirm-each-one flow can walk the same list the
//! replace-all flow applies in one go, and so every rule below is testable
//! without a screen.
//!
//! The pattern language is the one the rest of cian already speaks
//! ([`crate::search::Matcher`]): a bare needle is a case-insensitive
//! substring, `/re/` is a regex, `/re/i` an insensitive one. The replacement
//! understands the escapes people actually reach for — `\n`, `\t`, `\r`,
//! `\\` — because "turn every `\r\n` into `\n`" is the single most common
//! reason to open replace at all, and typing a literal CR is not possible.

use crate::search::Matcher;

/// One replacement the caller may accept or skip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Index into the lines that were handed in.
    pub line: usize,
    /// Char range within that line, end-exclusive.
    pub start: usize,
    pub end: usize,
    /// The text being replaced, and what it becomes — so a confirm prompt can
    /// show both without re-deriving them.
    pub from: String,
    pub to: String,
}

/// A parsed `:s/old/new/flags` command.
#[derive(Debug, Clone)]
pub struct Substitution {
    pub matcher: Matcher,
    pub replacement: String,
    /// Ask before each replacement (`c`).
    pub confirm: bool,
    /// Replace every occurrence on a line, not just the first (`g`).
    pub global: bool,
}

/// Parse `s/old/new/flags`, or the bare `/old/new/flags` the prompt shows.
///
/// The delimiter is whatever follows `s`, so a pattern full of slashes can use
/// another one — `s#/usr/local#/opt#` — which is exactly the case where
/// escaping every slash is unbearable.
pub fn parse(input: &str) -> Result<Substitution, String> {
    let body = input.strip_prefix('s').unwrap_or(input);
    let mut chars = body.chars();
    let Some(delim) = chars.next() else {
        return Err("nothing to replace — try s/old/new/".to_string());
    };
    if delim.is_alphanumeric() || delim.is_whitespace() {
        return Err(format!("{delim:?} cannot separate the parts — try s/old/new/"));
    }
    // Split on unescaped delimiters, keeping `\<delim>` as a literal.
    let mut parts: Vec<String> = vec![String::new()];
    let mut escaped = false;
    for c in chars {
        if escaped {
            // A backslash before the delimiter escapes it; anything else keeps
            // the backslash so the replacement's own escapes survive.
            if c != delim {
                parts.last_mut().unwrap().push('\\');
            }
            parts.last_mut().unwrap().push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == delim {
            parts.push(String::new());
        } else {
            parts.last_mut().unwrap().push(c);
        }
    }
    if parts.len() < 2 {
        return Err(format!("missing the replacement — try s{delim}old{delim}new{delim}"));
    }
    let pattern = parts[0].clone();
    let replacement = parts[1].clone();
    let flags = parts.get(2).cloned().unwrap_or_default();
    if pattern.is_empty() {
        return Err("nothing to search for".to_string());
    }
    let mut confirm = false;
    let mut global = false;
    let mut insensitive = false;
    for f in flags.chars() {
        match f {
            'c' => confirm = true,
            'g' => global = true,
            'i' => insensitive = true,
            other => return Err(format!("unknown flag {other:?} — only c, g and i")),
        }
    }
    // A pattern counts as a regex only when it is wrapped in slashes at *both*
    // ends — `/ORA-\d+/`, `/x/i`. Without the closing slash it is a literal,
    // which is what makes `s#/usr/local#/opt#` mean the path it looks like
    // rather than a malformed regex.
    let looks_regex = pattern.len() > 1
        && pattern.starts_with('/')
        && (pattern.ends_with('/') || pattern.ends_with("/i"));
    let matcher = if looks_regex {
        let pat = if insensitive && !pattern.ends_with("/i") {
            format!("{}i", pattern)
        } else {
            pattern
        };
        Matcher::parse(&pat)?
    } else {
        // A literal needle carries the same escapes the replacement does —
        // without this there is no way to name a carriage return, and
        // `s/\r//` (the reason replace gets opened at all) would search for a
        // backslash followed by an r.
        Matcher::Literal(unescape(&pattern).to_lowercase())
    };
    Ok(Substitution { matcher, replacement: unescape(&replacement), confirm, global })
}

/// How the pattern half of a replace is read.
///
/// Three states rather than a regex on/off switch, because the thing people
/// actually reach for `*` to say — "anything at all, here" — is neither. In a
/// regex `*` repeats the character before it; `crm*ne` is a perfectly good
/// pattern that does not match `crmaine`. Redefining it there would break
/// every honest `\d*`, `[a-z]*` and `(ab)*` in the same stroke, so the glob
/// reading gets a mode of its own instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pattern {
    /// Matched as typed, syntax and all.
    #[default]
    Plain,
    /// `*` is any run of characters, `?` is any one. Everything else is
    /// itself — the shell's reading, and the one a `*` in a search box is
    /// nearly always meant to have.
    Wildcard,
    /// A real regular expression.
    Regex,
}

/// `a*b?c` → `a.*b.c`, with everything else escaped. `*` and `?` are the only
/// two: adding `[...]` would be halfway to a regex without being one, and the
/// mode next door is a regex already.
fn wildcard_to_regex(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() * 2);
    for c in pattern.chars() {
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            other => out.push_str(&regex::escape(&other.to_string())),
        }
    }
    out
}

/// The one regex mistake worth naming: `*` repeats the character before it,
/// and everyone who has typed a shell glob expects it to mean "anything".
/// `crm*ne` compiles, matches nothing in a file full of `crmaine`, and looks
/// like it should have worked.
///
/// Returns the suggested pattern when the `*` follows a plain character —
/// not when it follows `.`, a class or a group, where the repeat is what was
/// meant. Advice, never a silent rewrite: matching something other than what
/// was typed is worse than finding nothing.
pub fn star_hint(pattern: &str) -> Option<String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut found = false;
    let mut escaped = false;
    for (i, &c) in chars.iter().enumerate() {
        if escaped {
            escaped = false;
            out.push(c);
            continue;
        }
        if c == '\\' {
            escaped = true;
            out.push(c);
            continue;
        }
        if c == '*' && i > 0 {
            let before = chars[i - 1];
            // `.` is already the wildcard; `]` and `)` close something the
            // author built on purpose; an escaped character was spelled out.
            let deliberate = matches!(before, '.' | ']' | ')' | '?' | '+' | '*')
                || (i >= 2 && chars[i - 2] == '\\');
            if !deliberate {
                found = true;
                out.push('.');
            }
        }
        out.push(c);
    }
    found.then_some(out)
}

/// The same substitution, said with fields instead of with `s/old/new/`.
///
/// This is what the replace bar hands in: a pattern, a replacement, and the
/// three switches every editor's replace dialog has. It always builds a regex,
/// even for a plain needle, because the switches are exactly the things a
/// literal cannot express — `Matcher::Literal` is case-insensitive by
/// definition and knows nothing about word boundaries.
///
/// `word` wraps the pattern in `\b…\b`, which is what "whole word only"
/// means; wrapping a user's own regex is deliberate, since someone who asks
/// for both wants their pattern matched at word boundaries.
pub fn from_fields(
    find: &str,
    with: &str,
    pattern: Pattern,
    case_sensitive: bool,
    word: bool,
) -> Result<Substitution, String> {
    if find.is_empty() {
        return Err("nothing to search for".to_string());
    }
    // A plain needle carries the same escapes the replacement does, so `\r`
    // means a carriage return here too — then it is escaped, so that the
    // characters a regex would read as syntax are matched as themselves.
    let body = match pattern {
        Pattern::Regex => find.to_string(),
        Pattern::Plain => regex::escape(&unescape(find)),
        Pattern::Wildcard => wildcard_to_regex(&unescape(find)),
    };
    let body = if word { format!(r"\b(?:{body})\b") } else { body };
    let re = regex::RegexBuilder::new(&body)
        .case_insensitive(!case_sensitive)
        .build()
        .map_err(|e| crate::search::shorten_regex_error(&e))?;
    Ok(Substitution {
        matcher: Matcher::Regex(re),
        replacement: unescape(with),
        confirm: false,
        // Every occurrence on a line: a replace dialog that stopped at the
        // first one per line would be a surprise nobody asked for.
        global: true,
    })
}

/// Turn the escapes a replacement may carry into real characters. Without this
/// there is no way to type a newline or a tab into the prompt at all.
pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            // An unknown escape keeps both characters: better to insert what
            // was typed than to silently eat a backslash.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Every replacement `sub` would make in `lines`, in order.
///
/// `range` limits it to a line range (a visual selection); `None` is the whole
/// buffer. Without `global`, only the first match on each line counts —
/// vim's rule, and the one that makes "fix the first column" possible.
pub fn find(sub: &Substitution, lines: &[String], range: Option<(usize, usize)>) -> Vec<Hit> {
    let (lo, hi) = range.unwrap_or((0, lines.len().saturating_sub(1)));
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if i < lo || i > hi {
            continue;
        }
        let ranges = sub.matcher.find_ranges(line);
        let chars: Vec<char> = line.chars().collect();
        for (start, end) in ranges {
            let from: String = chars[start..end.min(chars.len())].iter().collect();
            out.push(Hit {
                line: i,
                start,
                end,
                to: expand(sub, &from),
                from,
            });
            if !sub.global {
                break;
            }
        }
    }
    out
}

/// The replacement text for one match, with regex capture groups (`$1`,
/// `$name`) filled in. A literal search has no groups, so its replacement is
/// used as-is.
///
/// Note for anyone writing one: a group reference runs to the end of the
/// word, so `$2_$1` asks for a group *named* `2_`. Brace it — `${2}_${1}` —
/// whenever a letter, digit or underscore follows the number.
fn expand(sub: &Substitution, matched: &str) -> String {
    match &sub.matcher {
        Matcher::Literal(_) => sub.replacement.clone(),
        Matcher::Regex(re) => match re.captures(matched) {
            Some(caps) => {
                let mut out = String::new();
                caps.expand(&sub.replacement, &mut out);
                out
            }
            None => sub.replacement.clone(),
        },
    }
}

/// Apply `hits` to `lines`, returning the new lines.
///
/// Applied last-first within each line so an earlier replacement never shifts
/// the offsets of a later one — the classic way this kind of code goes wrong.
/// A replacement containing a newline splits the line, which is what makes
/// `s/;/;\n/g` (and its opposite) work.
pub fn apply(lines: &[String], hits: &[Hit]) -> Vec<String> {
    let mut out: Vec<String> = lines.to_vec();
    let mut by_line: std::collections::BTreeMap<usize, Vec<&Hit>> = Default::default();
    for h in hits {
        by_line.entry(h.line).or_default().push(h);
    }
    for (line, mut group) in by_line {
        let Some(text) = out.get(line) else { continue };
        group.sort_by_key(|h| std::cmp::Reverse(h.start));
        let mut chars: Vec<char> = text.chars().collect();
        for h in group {
            if h.start > chars.len() || h.end > chars.len() {
                continue;
            }
            let tail: Vec<char> = chars[h.end..].to_vec();
            chars.truncate(h.start);
            chars.extend(h.to.chars());
            chars.extend(tail);
        }
        out[line] = chars.into_iter().collect();
    }
    // A replacement may have introduced newlines; re-split so the buffer stays
    // one string per line.
    if out.iter().any(|l| l.contains('\n')) {
        out = out.iter().flat_map(|l| l.split('\n').map(str::to_string)).collect();
    }
    out
}

#[cfg(test)]
mod tests {
    /// The replace bar's three switches, which are the things `s/old/new/`
    /// cannot say: a literal is case-insensitive by definition and knows
    /// nothing about word boundaries.
    #[test]
    fn from_fields_says_what_the_switches_mean() {
        let run = |pat: &str, with: &str, re: Pattern, cs: bool, w: bool, lines: &[&str]| {
            let lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
            let sub = from_fields(pat, with, re, cs, w).unwrap();
            let hits = find(&sub, &lines, None);
            apply(&lines, &hits)
        };
        // Case-insensitive unless asked, every occurrence on a line.
        assert_eq!(run("cat", "dog", Pattern::Plain, false, false, &["cat CAT"]), vec!["dog dog"]);
        assert_eq!(run("cat", "dog", Pattern::Plain, true, false, &["cat CAT"]), vec!["dog CAT"]);
        // Whole words only.
        assert_eq!(run("cat", "dog", Pattern::Plain, false, true, &["cat cattle"]), vec!["dog cattle"]);
        // A plain needle is matched as itself, syntax and all.
        assert_eq!(run("a.c", "x", Pattern::Plain, false, false, &["a.c abc"]), vec!["x abc"]);
        // …and a regex is a regex.
        assert_eq!(run(r"a.c", "x", Pattern::Regex, false, false, &["a.c abc"]), vec!["x x"]);
        // The escapes, in both halves.
        assert_eq!(run(r"\t", " ", Pattern::Plain, false, false, &["a\tb"]), vec!["a b"]);
        assert_eq!(run("b", r"\tb", Pattern::Plain, false, false, &["ab"]), vec!["a\tb"]);
        // An empty pattern is a mistake, not a match on everything.
        assert!(from_fields("", "x", Pattern::Plain, false, false).is_err());
        // A broken regex is reported rather than quietly matched literally.
        assert!(from_fields("(", "x", Pattern::Regex, false, false).is_err());
    }

    /// `*` means "the character before it, repeated" — it is not a wildcard.
    /// `crm*ne` therefore does not match `crmaine`, and reporting that as a
    /// find is the only honest answer; `crm.*ne` is the pattern that means
    /// what people reach for `*` to say.
    #[test]
    fn a_star_repeats_the_thing_before_it() {
        let lines = vec!["crmaine".to_string()];
        let hit = |pat: &str| {
            let sub = from_fields(pat, "x", Pattern::Regex, false, false).unwrap();
            !find(&sub, &lines, None).is_empty()
        };
        assert!(!hit("crm*ne"), "zero-or-more m, then ne — crmaine has aine");
        assert!(hit("crm.*ne"));
        assert!(hit("crm.+ne"));
        assert!(hit("crmm*aine"), "and the star does what it says on the tin");
        // The hint that says so, offered only where it would help.
        assert!(star_hint("crm*ne").is_some());
        assert!(star_hint("crm.*ne").is_none(), "already a wildcard");
        assert!(star_hint(r"crm\w*ne").is_none(), "a class, repeated on purpose");
        assert!(star_hint("(ab)*c").is_none(), "a group, repeated on purpose");
        assert!(star_hint("plain").is_none());
    }

    /// The wildcard mode is where `*` means what a search box makes people
    /// expect it to mean. It is a separate mode rather than a change to the
    /// regex one, because `\d*` and `(ab)*` have to keep meaning what they
    /// say.
    #[test]
    fn wildcards_mean_what_a_shell_means_by_them() {
        let lines: Vec<String> =
            ["crmaine", "crmne", "crmXne", "crm*ne"].iter().map(|s| s.to_string()).collect();
        let hit = |pat: &str, mode: Pattern| {
            let sub = from_fields(pat, "x", mode, false, false).unwrap();
            find(&sub, &lines, None).iter().map(|h| h.line).collect::<Vec<_>>()
        };
        // `*` is any run, including an empty one.
        assert_eq!(hit("crm*ne", Pattern::Wildcard), vec![0, 1, 2, 3]);
        // `?` is exactly one.
        assert_eq!(hit("crm?ne", Pattern::Wildcard), vec![2, 3]);
        // Regex reads the same text quite differently, and still does.
        assert_eq!(hit("crm*ne", Pattern::Regex), vec![1]);
        // Everything else is itself: a dot is a dot here.
        let dots: Vec<String> = vec!["a.c".to_string(), "abc".to_string()];
        let sub = from_fields("a.c", "x", Pattern::Wildcard, false, false).unwrap();
        assert_eq!(find(&sub, &dots, None).len(), 1, "the literal a.c only");
    }

    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    fn run(cmd: &str, src: &[&str]) -> Vec<String> {
        let sub = parse(cmd).expect("parses");
        let l = lines(src);
        let hits = find(&sub, &l, None);
        apply(&l, &hits)
    }

    #[test]
    fn literal_replace_honours_the_global_flag() {
        // Without `g`, the first match on each line — vim's rule.
        assert_eq!(run("s/a/X/", &["banana", "apple"]), lines(&["bXnana", "Xpple"]));
        assert_eq!(run("s/a/X/g", &["banana"]), lines(&["bXnXnX"]));
        // A bare needle is case-insensitive, like every other search in cian.
        assert_eq!(run("s/ABC/x/", &["abcabc"]), lines(&["xabc"]));
    }

    #[test]
    fn regex_replace_expands_capture_groups() {
        assert_eq!(
            run(r"s#/(\w+)-(\d+)/#${2}_${1}#g", &["/ORA-600/ and /ORA-7445/"]),
            lines(&["/600_ORA/ and /7445_ORA/"]),
        );
        // Unbraced, a group reference swallows the following word characters
        // — `$2_` names a group that does not exist, and yields nothing.
        assert_eq!(run(r"s#/(a)(b)/#$2$1#", &["ab"]), lines(&["ba"]), "plain refs still work");
        // `/…/i` for an insensitive regex, or the `i` flag on the command.
        assert_eq!(run("s#/error/i#OK#g", &["Error ERROR error"]), lines(&["OK OK OK"]));
        assert_eq!(run("s#/error/#OK#gi", &["Error ERROR"]), lines(&["OK OK"]));
    }

    /// The reason replace exists: normalising line endings. A `\r` is not
    /// typeable, so the escape has to carry it.
    #[test]
    fn crlf_becomes_lf() {
        let src = lines(&["one\r", "two\r", "three"]);
        let sub = parse(r"s/\r//g").unwrap();
        let hits = find(&sub, &src, None);
        assert_eq!(hits.len(), 2, "only the two carriage returns");
        assert_eq!(apply(&src, &hits), lines(&["one", "two", "three"]));
    }

    /// A replacement holding a newline splits the line — and its opposite,
    /// joining, is just the same machinery.
    #[test]
    fn a_newline_in_the_replacement_splits_the_line() {
        assert_eq!(run(r"s/;/;\n/g", &["a;b;c"]), lines(&["a;", "b;", "c"]));
    }

    /// Several matches on one line must not shift each other's offsets, and a
    /// selection limits the damage to the chosen lines.
    #[test]
    fn overlapping_offsets_and_ranges_stay_correct() {
        // The replacement is longer than the match, so a left-to-right apply
        // would corrupt every later offset on the line.
        assert_eq!(run("s/x/LONGER/g", &["x-x-x"]), lines(&["LONGER-LONGER-LONGER"]));

        let src = lines(&["a", "a", "a"]);
        let sub = parse("s/a/b/g").unwrap();
        let hits = find(&sub, &src, Some((1, 1)));
        assert_eq!(apply(&src, &hits), lines(&["a", "b", "a"]), "only the selected line");
    }

    #[test]
    fn a_custom_delimiter_saves_escaping_every_slash() {
        assert_eq!(
            run("s#/usr/local#/opt#", &["/usr/local/bin"]),
            lines(&["/opt/bin"]),
        );
        // …and an escaped delimiter still works inside the pattern.
        assert_eq!(run(r"s/a\/b/X/", &["a/b/c"]), lines(&["X/c"]));
    }

    #[test]
    fn bad_input_is_refused_with_a_reason() {
        for bad in ["s/only-one-part", "s", "sxaxbx", "s/a/b/z"] {
            assert!(parse(bad).is_err(), "{bad:?} should be refused");
        }
        assert!(parse("s#/[unclosed/#x#").is_err(), "a broken regex is refused");
        // …but the same characters unwrapped are a literal, not an error:
        // a bracket in a filename should not need escaping.
        assert!(parse("s/[unclosed/x/").is_ok(), "an unwrapped pattern is literal");
        // The flags that do exist all parse.
        for good in ["s/a/b/", "s/a/b/g", "s/a/b/c", "s/a/b/gci", "s#a#b#g"] {
            assert!(parse(good).is_ok(), "{good:?} should parse");
        }
    }
}
