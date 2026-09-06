//! Reading Markdown, once, for two front ends that draw it differently.
//!
//! The terminal build had all of this fused into its renderer: recognise a
//! heading and emit a styled ratatui line in the same breath. That works while
//! there is one way to draw, and the window is a second way — so the
//! recognising moved down here and the drawing stayed up there.
//!
//! **The alternative was a second parser**, and a second parser is a second
//! opinion about what `*a_b*` means. Two front ends of one program disagreeing
//! about their own README is a small thing that reads as carelessness.
//!
//! Deliberately not CommonMark. cian reads the Markdown people write in
//! READMEs and notes — headings, lists, fences, tables, task boxes, and the
//! four inline marks — and stops there. A full implementation is a large
//! dependency and most of it would never be reached.

/// A run of text with one meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    /// `` `code` `` — one span, never nested.
    Code(String),
    Bold(String),
    Italic(String),
    Strike(String),
    Link {
        text: String,
        url: String,
    },
    /// `<span style="color:#rrggbb">…</span>` — the one piece of HTML cian
    /// reads, because Markdown has no colour and this is the notation the
    /// most other tools already understand. **Only a validated hex colour
    /// ever gets through**, so the promise made at `html` — that everything
    /// from the file is escaped — still holds.
    Colored { text: String, color: String },
}

impl Inline {
    /// The characters, with the marks dropped. For a plain-text need — a
    /// width measurement, a search — where the emphasis does not matter.
    pub fn text(&self) -> &str {
        match self {
            Inline::Text(t)
            | Inline::Code(t)
            | Inline::Bold(t)
            | Inline::Italic(t)
            | Inline::Strike(t) => t,
            Inline::Link { text, .. } => text,
            Inline::Colored { text, .. } => text,
        }
    }
}

/// How a table column is lined up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// Split one line into its runs.
///
/// A scanner rather than a grammar: Markdown's inline marks are not nested in
/// practice — nobody writes bold inside a link inside italics — and a scanner
/// that gives up on an unclosed mark leaves it as text, which is what a reader
/// expects from a stray asterisk.
pub fn inline(text: &str) -> Vec<Inline> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<Inline> = Vec::new();
    let mut buf = String::new();
    let mut i = 0;

    let flush = |out: &mut Vec<Inline>, buf: &mut String| {
        if !buf.is_empty() {
            out.push(Inline::Text(std::mem::take(buf)));
        }
    };

    while i < chars.len() {
        let c = chars[i];

        // A colour span. Recognised by `note::spans`, so the window and the
        // phone cannot end up with two opinions about what a note says.
        if c == '<' {
            let rest: String = chars[i..].iter().collect();
            if let Some((inner, color, took)) = crate::note::first_color(&rest) {
                flush(&mut out, &mut buf);
                out.push(Inline::Colored { text: inner, color });
                i += took;
                continue;
            }
        }

        // Inline code first: everything inside a backtick pair is literal, so
        // a `*` in there is an asterisk and not the start of emphasis.
        if c == '`' {
            if let Some(end) = chars[i + 1..].iter().position(|&x| x == '`') {
                flush(&mut out, &mut buf);
                out.push(Inline::Code(chars[i + 1..i + 1 + end].iter().collect()));
                i += end + 2;
                continue;
            }
        }

        // Bold **…** or __…__
        if (c == '*' || c == '_') && i + 1 < chars.len() && chars[i + 1] == c {
            if let Some(end) = find_run(&chars, i + 2, [c, c]) {
                flush(&mut out, &mut buf);
                out.push(Inline::Bold(chars[i + 2..end].iter().collect()));
                i = end + 2;
                continue;
            }
        }

        // Strikethrough ~~…~~
        if c == '~' && i + 1 < chars.len() && chars[i + 1] == '~' {
            if let Some(end) = find_run(&chars, i + 2, ['~', '~']) {
                flush(&mut out, &mut buf);
                out.push(Inline::Strike(chars[i + 2..end].iter().collect()));
                i = end + 2;
                continue;
            }
        }

        // Italic *…* or _…_. A leading space rules it out, which is what stops
        // `a * b * c` from turning half a sum into emphasis.
        if c == '*' || c == '_' {
            if let Some(end) = chars[i + 1..].iter().position(|&x| x == c) {
                let inner: String = chars[i + 1..i + 1 + end].iter().collect();
                if !inner.is_empty() && !inner.starts_with(' ') {
                    flush(&mut out, &mut buf);
                    out.push(Inline::Italic(inner));
                    i += end + 2;
                    continue;
                }
            }
        }

        // Link [text](url)
        if c == '[' {
            if let Some(close) = chars[i + 1..].iter().position(|&x| x == ']') {
                let after = i + 1 + close + 1;
                if chars.get(after) == Some(&'(') {
                    if let Some(paren) = chars[after + 1..].iter().position(|&x| x == ')') {
                        flush(&mut out, &mut buf);
                        out.push(Inline::Link {
                            text: chars[i + 1..i + 1 + close].iter().collect(),
                            url: chars[after + 1..after + 1 + paren].iter().collect(),
                        });
                        i = after + 1 + paren + 1;
                        continue;
                    }
                }
            }
        }

        buf.push(c);
        i += 1;
    }
    flush(&mut out, &mut buf);
    out
}

/// The start of a two-character `marker` run at or after `from`.
fn find_run(chars: &[char], from: usize, marker: [char; 2]) -> Option<usize> {
    let mut i = from;
    while i + 1 < chars.len() {
        if chars[i] == marker[0] && chars[i + 1] == marker[1] {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ---- Block recognisers ----
//
// Each answers one question about one line and nothing else. They were already
// separate in the terminal build, which is why they could come down here
// unchanged.

/// `## Heading` → `(2, "Heading")`.
/// A heading's anchor, GitHub's way: lowercased, spaces to hyphens, and
/// punctuation dropped.
///
/// **Runs of hyphens are not collapsed, and that is deliberate.** GitHub's
/// slugger turns `v1.2 — notes` into `v12--notes` — the dash is dropped and
/// the two spaces around it each become a hyphen — and the links inside a
/// README were written against *that*. A tidier anchor would be a prettier
/// string that none of the document's own links point at.
///
/// Japanese is *kept*, not stripped. GitHub percent-encodes it in the href and
/// leaves the characters in the id — strip them and every heading in a
/// Japanese document collapses to the same empty anchor, which is worse than
/// no anchor at all. The window decodes the href before it looks the id up.
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.trim().chars() {
        if c.is_whitespace() {
            out.push('-');
        } else if c.is_alphanumeric() || c == '-' || c == '_' {
            out.extend(c.to_lowercase());
        }
        // Everything else — `.`, `(`, `:`, an emoji — is dropped, as GitHub
        // drops it.
    }
    out
}

pub fn heading(line: &str) -> Option<(usize, String)> {
    let t = line.trim_start();
    let hashes = t.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = t[hashes..].trim_start();
    // `#hashtag` is not a heading: a heading has a space after its hashes.
    if rest.len() == t.len() - hashes {
        return None;
    }
    Some((hashes, rest.to_string()))
}

/// `---`, `***`, `___` on their own.
pub fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

/// ` ```rust ` → `Some("rust")`; ` ``` ` → `Some("")`.
pub fn fence_lang(line: &str) -> Option<String> {
    let t = line.trim_start();
    t.strip_prefix("```").map(|rest| rest.trim().to_string())
}

/// `- item` / `1. item` → `(marker, text, indent)`.
pub fn list_item(raw: &str) -> Option<(String, String, usize)> {
    let indent = raw.len() - raw.trim_start().len();
    let t = raw.trim_start();
    for m in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(m) {
            return Some(("•".to_string(), rest.to_string(), indent));
        }
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let after = &t[digits..];
        for m in [". ", ") "] {
            if let Some(rest) = after.strip_prefix(m) {
                return Some((format!("{}{}", &t[..digits], m.trim_end()), rest.to_string(), indent));
            }
        }
    }
    None
}

/// `[ ] thing` / `[x] thing` → `(done, text)`.
pub fn task_item(text: &str) -> Option<(bool, String)> {
    let t = text.trim_start();
    for (mark, done) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
        if let Some(rest) = t.strip_prefix(mark) {
            return Some((done, rest.to_string()));
        }
    }
    None
}

/// `| --- | :-: |` — the line under a table's header.
pub fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    if !t.contains('-') || !t.starts_with('|') {
        return false;
    }
    t.trim_matches('|')
        .split('|')
        .all(|c| {
            let c = c.trim();
            !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')
        })
}

/// The cells of `| a | b |`.
pub fn split_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

/// `:-:` → centre, `--:` → right, anything else → left.
pub fn cell_align(sep: &str) -> Align {
    let s = sep.trim();
    match (s.starts_with(':'), s.ends_with(':')) {
        (true, true) => Align::Center,
        (false, true) => Align::Right,
        _ => Align::Left,
    }
}

// ---- Rendering to HTML ----
//
// The window's half. The terminal build draws the same parse as styled lines;
// this turns it into a document, which is the one thing a window can do that a
// terminal cannot — real proportional type, real tables, a real code block.
//
// **Every piece of text is escaped.** A README is a file from somewhere, and a
// preview that runs what it finds is a preview that runs whatever was in the
// repository somebody cloned.

/// Escape the five characters that would otherwise be markup.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A URL safe to put in an `href`.
///
/// `javascript:` in a link is the oldest trick there is, and a README is a
/// file from somewhere. Anything that is not plainly http, https, mailto or a
/// relative path becomes no link at all — shown as text, so nothing is hidden,
/// just not clickable.
fn safe_url(url: &str) -> Option<String> {
    let u = url.trim();
    let lower = u.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") {
        return Some(esc(u));
    }
    // A relative path: no scheme at all. `foo:bar` might be one, so anything
    // with a colon before the first slash is refused.
    let scheme_ish = u.split('/').next().unwrap_or("").contains(':');
    if !scheme_ish && !u.is_empty() {
        return Some(esc(u));
    }
    None
}

fn inline_html(text: &str) -> String {
    let mut out = String::new();
    for piece in inline(text) {
        match piece {
            Inline::Text(t) => out.push_str(&esc(&t)),
            Inline::Code(t) => {
                out.push_str("<code>");
                out.push_str(&esc(&t));
                out.push_str("</code>");
            }
            Inline::Bold(t) => {
                out.push_str("<strong>");
                out.push_str(&esc(&t));
                out.push_str("</strong>");
            }
            Inline::Italic(t) => {
                out.push_str("<em>");
                out.push_str(&esc(&t));
                out.push_str("</em>");
            }
            Inline::Strike(t) => {
                out.push_str("<del>");
                out.push_str(&esc(&t));
                out.push_str("</del>");
            }
            // The colour was validated as six hex digits before it got
            // here, so this is the one place a style attribute is written
            // and it cannot carry anything else.
            Inline::Colored { text, color } => {
                out.push_str(&format!(
                    "<span style=\"color:{color}\">{}</span>",
                    esc(&text)
                ));
            }
            Inline::Link { text, url } => match safe_url(&url) {
                Some(href) => {
                    out.push_str(&format!("<a href=\"{href}\">{}</a>", esc(&text)));
                }
                // Shown, not hidden — and not clickable.
                None => out.push_str(&esc(&text)),
            },
        }
    }
    out
}

/// 書く道具。**選んだところだけを渡してもらい、置き換えたものを返す。**
///
/// 位置（何文字目か）は受け取らない ── JS は UTF-16 の桁で数え、Rust は
/// 文字で数えるので、絵文字が一つ混ざるだけで境目がずれる。選んだ字そのものを
/// もらえば、その食い違いは起きようがない。
///
/// **意味はここにある。** iPhone の `Marks.deepen` と窓が別々に「見出しを
/// 深くする」を持つと、二つの前端で押し心地が分かれる。いまは窓だけが
/// ここを通っており、iPhone は Swift の写しを持ったままなので、**揃えるのは
/// これから**（`REQUESTS.ja.md` に置いた）。
pub mod marks {
    /// 挟む。**もう挟まっているなら外す。** 間違えて押した瞬間に欲しくなる。
    pub fn wrap(text: &str, mark: &str) -> String {
        if mark.is_empty() {
            return text.to_string();
        }
        // 選んでいないときは印だけ置く ── 中に入って打てるように。
        if text.is_empty() {
            return format!("{mark}{mark}");
        }
        if let Some(inner) = text
            .strip_prefix(mark)
            .and_then(|t| t.strip_suffix(mark))
            .filter(|_| text.len() >= mark.len() * 2)
        {
            return inner.to_string();
        }
        format!("{mark}{text}{mark}")
    }

    /// 行頭の印。**すべての行に付いていれば外し、一つでも無ければ付ける。**
    ///
    /// 付けるときは、先に**別の行頭の印を外す** ── 箇条書きを引用にすると
    /// `> - もの` になるのは、たいてい望んだことではない。
    pub fn prefix(text: &str, mark: &str) -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        let numbered = mark.starts_with(|c: char| c.is_ascii_digit());
        let has = |l: &str| -> bool {
            let t = l.trim_start();
            if numbered {
                let d = t.chars().take_while(|c| c.is_ascii_digit()).count();
                return d > 0 && t[d..].starts_with(". ");
            }
            t.starts_with(mark)
                // `- [ ] ` は `- [x] ` でも付いている。
                || (mark == "- [ ] " && (t.starts_with("- [x] ") || t.starts_with("- [X] ")))
        };
        // 空行は数に入れない ── 一行だけ空いているせいで外れない、を防ぐ。
        let live: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
        let all = !live.is_empty() && live.iter().all(|l| has(l));

        let mut n = 0usize;
        lines
            .iter()
            .map(|l| {
                if l.trim().is_empty() {
                    return l.to_string();
                }
                let indent = &l[..l.len() - l.trim_start().len()];
                let body = strip_any(l.trim_start());
                if all {
                    return format!("{indent}{body}");
                }
                n += 1;
                if numbered {
                    format!("{indent}{n}. {body}")
                } else {
                    format!("{indent}{mark}{body}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 行頭に付いている印を、どれでも外す。
    fn strip_any(t: &str) -> &str {
        for m in ["- [ ] ", "- [x] ", "- [X] ", "> ", "- ", "* ", "+ "] {
            if let Some(r) = t.strip_prefix(m) {
                return r;
            }
        }
        let d = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if d > 0 {
            if let Some(r) = t[d..].strip_prefix(". ") {
                return r;
            }
        }
        t
    }

    /// 見出し。**押すたびに深くなる** ── `#` → `##` → `###` → 無し。
    ///
    /// ボタンを三つ置くと、一つの考えに三つの名前が付く。
    pub fn deepen(text: &str) -> String {
        text.split('\n')
            .map(|l| {
                let t = l.trim_start();
                let indent = &l[..l.len() - t.len()];
                let n = t.chars().take_while(|c| *c == '#').count();
                let body = t[n..].trim_start();
                if body.is_empty() && n == 0 {
                    return l.to_string();
                }
                match n {
                    0 => format!("{indent}# {body}"),
                    1 => format!("{indent}## {body}"),
                    2 => format!("{indent}### {body}"),
                    _ => format!("{indent}{body}"),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// 置いた印を `data-line` / `data-span` に変える。
///
/// 印は `\u{1}` 行番号 `\u{2}`。**次の印までが、そのかたまりが食べた行**。
/// 何も出さなかった周（空行など）は印が続くだけなので、そのまま落とす。
///
/// 差す先は**印のすぐ後ろの開き札**。表や引用のように中に札を持つものでも、
/// 外側の一枚だけに差さる ── 中まで差すと、押した場所によって違う行が
/// 返ることになる。
fn stamps(raw: &str, total: usize) -> String {
    // まず (印の位置, 行番号, 印の長さ) を集める。
    let mut marks: Vec<(usize, usize, usize)> = Vec::new();
    let b = raw.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == 1 {
            if let Some(end) = raw[i..].find('\u{2}') {
                let n: usize = raw[i + 1..i + end].parse().unwrap_or(0);
                marks.push((i, n, end + 1));
                i += end + 1;
                continue;
            }
        }
        i += 1;
    }

    let mut out = String::with_capacity(raw.len());
    let mut cut = 0;
    for (k, &(at, line, len)) in marks.iter().enumerate() {
        out.push_str(&raw[cut..at]);
        cut = at + len;
        // 次の印の行番号までが、このかたまりの行数。
        let next = marks.get(k + 1).map(|m| m.1).unwrap_or(total);
        let span = next.saturating_sub(line).max(1);
        // **閉じ札は飛ばして、次の開き札に差す。**
        //
        // 箇条書きの二つ目以降は、前の項目を閉じてから始まる ── そこで
        // 最初に見つかる `>` は `</li>` のもので、そこに差すと
        // `</li data-line="10">` という札にならない札ができる（一度作った）。
        let rest = &raw[cut..];
        let mut j = 0;
        loop {
            let r = &rest[j..];
            j += r.len() - r.trim_start().len();
            let r = &rest[j..];
            if r.starts_with("</") {
                match r.find('>') {
                    Some(p) => {
                        j += p + 1;
                        continue;
                    }
                    None => break,
                }
            }
            break;
        }
        if rest[j..].starts_with('<') && !rest[j..].starts_with("</") {
            if let Some(p) = rest[j..].find('>') {
                out.push_str(&rest[..j + p]);
                out.push_str(&format!(" data-line=\"{line}\" data-span=\"{span}\""));
                cut += j + p;
            }
        }
    }
    out.push_str(&raw[cut..]);
    out
}

/// `> [!NOTE]` の一行を、注記の種類に。**GitHub が読める五つだけ。**
///
/// 増やすと、ここでだけ見える記法になり、同じノートが GitHub で壊れる。
pub fn alert_kind(t: &str) -> Option<String> {
    let rest = t.strip_prefix('>')?.trim_start();
    let inner = rest.strip_prefix("[!")?.strip_suffix(']')?;
    let k = inner.to_ascii_lowercase();
    matches!(k.as_str(), "note" | "tip" | "important" | "warning" | "caution").then_some(k)
}

/// Render Markdown as HTML.
///
/// Line-based, like the terminal renderer it shares a parser with: cian reads
/// the Markdown people write, not the Markdown a specification describes.
pub fn to_html(lines: &[String]) -> String {
    render(lines, true)
}

/// 各かたまりに、**元の何行目から何行ぶんか**を差す。
///
/// 読む面で直に書き換えるのに要る ── 押したかたまりの元の字が取れなければ、
/// 読む面は読むだけの面のままになる。チェックの升が `data-line` を積んで
/// いるのと同じ理由で、**何番目のかたまりかを数えると前書きのあるノートで
/// ずれる**。
///
/// 印はいったん制御文字で置き、最後に属性に変える ── かたまりが何行
/// 食べたかは、**次のかたまりが始まる場所**を見るまで分からない（枠も
/// 表も引用も、閉じるまで進む）。開始は毎周の頭で分かるので、印だけ先に
/// 置いておけば `continue` で抜ける枝も漏れない。
///
/// 引用や注記の中で呼び直すときは差さない（`stamp = false`）── 中の行番号は
/// 切り出したあとの数え方で、ファイルの行番号ではない。
fn render(lines: &[String], stamp: bool) -> String {
    let mut out = String::new();
    // The front matter goes, if there is one: it is how a note describes
    // itself, not something it says, and the title and tags are already on
    // screen wherever this is being drawn. `note::front` decides whether the
    // leading `---` is a front matter or a rule — one answer, so the phone
    // and the window agree about where a note starts.
    let mut i = crate::note::front(lines).lines;
    // Which list levels are open, by indent, and whether each is numbered.
    // Markdown's nesting is indentation and nothing else, so the indent is
    // the whole of the nesting; the flag is only about which tag closes it.
    //
    // **番号は `<ol>` で出す。** ここが `<ul>` のままだと、書いた `1. 2. 3.`
    // が黒丸で出る ── 書いたものと読めるものが違うので、Markdown が壊れて
    // いるように見える。何番から始めるかは `list_item` が返す印から取る。
    let mut open_lists: Vec<(usize, bool)> = Vec::new();

    // A list item stays open until something ends it, because a deeper list
    // belongs *inside* the item above it. Closing each `<li>` as it is written
    // put the nested `<ul>` next to its parent rather than in it — which
    // browsers tolerate and then indent wrongly.
    //
    // `li_open` means: the innermost open list has an item that has not been
    // closed. Going deeper leaves it open on purpose; coming back out closes
    // it, and closing a nested list re-opens the question for its parent,
    // whose own item was never closed either.
    let mut li_open = false;
    /// Close every open list. Anything that is not a list item ends all of
    /// them — a paragraph after a list is not inside it.
    fn close_all_lists(out: &mut String, open: &mut Vec<(usize, bool)>, li: &mut bool) {
        while let Some((_, ord)) = open.pop() {
            if *li {
                out.push_str("</li>\n");
            }
            out.push_str(if ord { "</ol>\n" } else { "</ul>\n" });
            *li = !open.is_empty();
        }
        *li = false;
    }

    fn close_lists_to(out: &mut String, open: &mut Vec<(usize, bool)>, li: &mut bool, indent: usize) {
        while open.last().is_some_and(|(d, _)| *d > indent) {
            if *li {
                out.push_str("</li>\n");
            }
            let (_, ord) = open.pop().unwrap();
            out.push_str(if ord { "</ol>\n" } else { "</ul>\n" });
            // The item this list was nested inside is still open.
            *li = !open.is_empty();
        }
        if *li && !open.is_empty() {
            out.push_str("</li>\n");
            *li = false;
        }
    }

    while i < lines.len() {
        if stamp {
            out.push('\u{1}');
            out.push_str(&i.to_string());
            out.push('\u{2}');
        }
        let raw = &lines[i];
        let t = raw.trim();

        // A fence takes everything to its close, verbatim.
        if let Some(lang) = fence_lang(raw) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            let mut body = String::new();
            i += 1;
            while i < lines.len() && fence_lang(&lines[i]).is_none() {
                body.push_str(&esc(&lines[i]));
                body.push('\n');
                i += 1;
            }
            i += 1; // the closing fence
            let class = if lang.is_empty() {
                String::new()
            } else {
                format!(" class=\"language-{}\"", esc(&lang))
            };
            out.push_str(&format!("<pre><code{class}>{body}</code></pre>\n"));
            continue;
        }

        if t.is_empty() {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            i += 1;
            continue;
        }

        if is_rule(raw) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            out.push_str("<hr>\n");
            i += 1;
            continue;
        }

        if let Some((level, text)) = heading(raw) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            // With an anchor, so `[…](#usage)` in the same file has somewhere
            // to land. A README is mostly links to itself and its neighbours,
            // and a preview that cannot follow either opens almost nothing
            // the document points at.
            out.push_str(&format!(
                "<h{level} id=\"{}\">{}</h{level}>\n",
                slug(&text),
                inline_html(&text)
            ));
            i += 1;
            continue;
        }

        // A table: a header row, a separator, then rows until they stop.
        if t.starts_with('|') && i + 1 < lines.len() && is_table_separator(&lines[i + 1]) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            let head = split_cells(raw);
            let aligns: Vec<Align> = split_cells(&lines[i + 1]).iter().map(|c| cell_align(c)).collect();
            let at = |n: usize| match aligns.get(n).copied().unwrap_or_default() {
                Align::Left => "",
                Align::Center => " style=\"text-align:center\"",
                Align::Right => " style=\"text-align:right\"",
            };
            out.push_str("<table>\n<thead><tr>");
            for (n, c) in head.iter().enumerate() {
                out.push_str(&format!("<th{}>{}</th>", at(n), inline_html(c)));
            }
            out.push_str("</tr></thead>\n<tbody>\n");
            i += 2;
            while i < lines.len() && lines[i].trim().starts_with('|') {
                out.push_str("<tr>");
                for (n, c) in split_cells(&lines[i]).iter().enumerate() {
                    out.push_str(&format!("<td{}>{}</td>", at(n), inline_html(c)));
                }
                out.push_str("</tr>\n");
                i += 1;
            }
            out.push_str("</tbody>\n</table>\n");
            continue;
        }

        // 行そのものが絵なら、絵として出す。**どこからが絵かは
        // `note::lone_image` の1か所** ── iPhone が絵として積む行を窓が
        // 字で出すと、同じノートが二つの見た目を持つ。
        if let Some(crate::note::Block::Image { alt, link }) = crate::note::lone_image(t) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            match safe_url(&link) {
                Some(src) => out.push_str(&format!(
                    "<img src=\"{src}\" alt=\"{}\">\n",
                    esc(&alt)
                )),
                // 出せない先なら、書いてあったものをそのまま字で。
                // **隠して失うより、出して残す。**
                None => out.push_str(&format!("<p>{}</p>\n", esc(t))),
            }
            i += 1;
            continue;
        }

        // GitHub 風の注記。`> [!NOTE]` に続く引用を、色の付いた枠にする。
        //
        // **引用の中の一行目でしか始まらない。** 本文に `[!NOTE]` と書いた
        // だけで枠になると、角括弧を書いただけの行が消える。GitHub が
        // 読めるものと同じ五つだけを受ける ── 増やすと、ここでだけ見える
        // 記法になり、ノートが GitHub で壊れる。
        if let Some(kind) = alert_kind(t) {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() {
                let q = lines[i].trim();
                let Some(rest) = q.strip_prefix('>') else { break };
                body.push(rest.trim_start().to_string());
                i += 1;
            }
            let name = match kind.as_str() {
                "note" => "ノート",
                "tip" => "こつ",
                "important" => "大事",
                "warning" => "注意",
                _ => "危険",
            };
            out.push_str(&format!(
                "<div class=\"alert {kind}\"><p class=\"alert-h\">{name}</p>\n"
            ));
            out.push_str(&render(&body, false));
            out.push_str("</div>\n");
            continue;
        }

        if t.starts_with("> ") || t == ">" {
            close_all_lists(&mut out, &mut open_lists, &mut li_open);
            let mut body = Vec::new();
            while i < lines.len() {
                let q = lines[i].trim();
                let Some(rest) = q.strip_prefix('>') else { break };
                body.push(rest.trim_start().to_string());
                i += 1;
            }
            out.push_str("<blockquote>\n");
            out.push_str(&render(&body, false));
            out.push_str("</blockquote>\n");
            continue;
        }

        if let Some((mark, text, indent)) = list_item(raw) {
            // `list_item` は印をそのまま返す ── 黒丸なら "•"、番号なら "1."。
            let ord = mark != "•";
            // 1 から始まらない番号は `start` で渡す。書いた番号で出ないと、
            // 途中から続ける箇条書き（手順の続き）が毎回 1 に戻る。
            let open_tag = |o: bool| -> String {
                if !o {
                    return "<ul>\n".to_string();
                }
                let n: usize = mark.trim_end_matches(['.', ')']).parse().unwrap_or(1);
                if n == 1 {
                    "<ol>\n".to_string()
                } else {
                    format!("<ol start=\"{n}\">\n")
                }
            };
            if open_lists.last().is_some_and(|(d, _)| indent > *d) {
                // Deeper: the parent's item stays open and this list goes in it.
                open_lists.push((indent, ord));
                out.push_str(&open_tag(ord));
            } else {
                close_lists_to(&mut out, &mut open_lists, &mut li_open, indent);
                // 同じ深さで印が変わったら、別のリスト ── 黒丸の続きに
                // 番号を混ぜると、片方の記法がもう片方の見た目で出る。
                if open_lists.last().is_some_and(|(_, o)| *o != ord) {
                    close_all_lists(&mut out, &mut open_lists, &mut li_open);
                }
                if open_lists.is_empty() {
                    open_lists.push((indent, ord));
                    out.push_str(&open_tag(ord));
                }
            }
            match task_item(&text) {
                // The line it came from travels with it. A checkbox you can
                // see and not press is a checkbox that makes you go and find
                // the line yourself — and `note::set_check` takes a line
                // number, so this is the whole of what a window needs to
                // make it work.
                // **`<button>` で出す。`<span>` ではない。** 押せるものは
                // 操作できるものとして名乗るべきで、そうでないと読み上げは
                // ただの字として読み、Tab では辿り着けず、キーボードだけの
                // 人には「無い」のと同じになる。押せる升が一つ出せない
                // だけで、ノートの半分が触れなくなる。
                Some((done, rest)) => out.push_str(&format!(
                    "<li class=\"task\"><button type=\"button\" class=\"box\" data-line=\"{}\" aria-pressed=\"{}\">{}</button>{}",
                    i,
                    done,
                    if done { "☑" } else { "☐" },
                    inline_html(&rest),
                )),
                None => out.push_str(&format!("<li>{}", inline_html(&text))),
            }
            li_open = true;
            i += 1;
            continue;
        }

        // A paragraph: this line and the ones after it that are not something
        // else. Joined with a space, because a hard-wrapped paragraph is one
        // paragraph and a window can wrap it itself.
        close_all_lists(&mut out, &mut open_lists, &mut li_open);
        let mut para = Vec::new();
        while i < lines.len() {
            let p = &lines[i];
            let pt = p.trim();
            if pt.is_empty()
                || heading(p).is_some()
                || is_rule(p)
                || fence_lang(p).is_some()
                || list_item(p).is_some()
                || pt.starts_with('|')
                || pt.starts_with('>')
            {
                break;
            }
            para.push(pt.to_string());
            i += 1;
        }
        out.push_str(&format!("<p>{}</p>\n", inline_html(&para.join(" "))));
    }
    close_all_lists(&mut out, &mut open_lists, &mut li_open);
    if stamp {
        return stamps(&out, lines.len());
    }
    out
}

#[cfg(test)]
mod tests {

    /// 組み方を確かめるときは、**行の印を外して見る。**
    ///
    /// `data-line` / `data-span` は「元の何行目か」という覚書で、組み方の
    /// 一部ではない。文字列でそのまま比べると、印を足した日に組み方の
    /// テストが全部落ちる ── 落ちたのは組み方ではないのに。
    ///
    /// `super::to_html` を覆っているので、この段のテストは自動でこちらを
    /// 通る。印そのものを確かめるテストだけ `super::to_html` を名指しする。
    fn to_html(lines: &[String]) -> String {
        let mut out = super::to_html(lines);
        for key in [" data-line=\"", " data-span=\""] {
            while let Some(at) = out.find(key) {
                let Some(end) = out[at + key.len()..].find('"') else { break };
                out.replace_range(at..at + key.len() + end + 1, "");
            }
        }
        out
    }

    #[test]
    fn two_coloured_words_on_one_line_both_survive() {
        // 一度これで壊れた: `find` はバイトを数え、走査は文字を数えていたので、
        // 日本語を挟むと span の**先まで**飛び越えて、次の span の途中から
        // 字が出ていた。
        let line = "ふつうの字と<span style=\"color:#D9822B\">だいだいの字</span>と、\
<span style=\"color:#0E93A8\">シアン</span>。";
        let out = to_html(&lines(line));
        assert!(out.contains("<span style=\"color:#d9822b\">だいだいの字</span>"), "{out}");
        assert!(out.contains("<span style=\"color:#0e93a8\">シアン</span>"), "{out}");
        assert!(!out.contains("e=&quot;color"), "span の途中から字が出ている: {out}");
        assert!(out.contains("と、"), "間の字が食われた: {out}");
    }

    #[test]
    fn front_matter_is_how_a_note_describes_itself_not_something_it_says() {
        let out = to_html(&lines("---\ntitle: 週報\ntags: [仕事]\n---\n\n# 見出し\n"));
        assert!(!out.contains("title:"), "{out}");
        assert!(out.contains("見出し"), "{out}");
        // 先頭の `---` が前書きでないなら、これまで通り区切り線。
        let rule = to_html(&lines("---\n\n本文。\n"));
        assert!(rule.contains("<hr"), "{rule}");
    }

    #[test]
    fn a_colour_survives_the_escaping_and_nothing_else_does() {
        let out = to_html(&lines("ふつうと<span style=\"color:#0E93A8\">シアン</span>。"));
        assert!(out.contains("<span style=\"color:#0e93a8\">シアン</span>"), "{out}");
        // 他の HTML は、これまで通り字にする。
        let out = to_html(&lines("<span onclick=\"x\">あ</span>"));
        assert!(out.contains("&lt;span"), "{out}");
        assert!(!out.contains("onclick=\"x\""), "{out}");
        // 色でない span も字のまま。
        let out = to_html(&lines("<span class=\"x\">あ</span>"));
        assert!(out.contains("&lt;span"), "{out}");
    }
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn inline_marks_are_read_once() {
        assert_eq!(
            inline("a **b** c `d` [e](http://x) ~~f~~"),
            vec![
                Inline::Text("a ".into()),
                Inline::Bold("b".into()),
                Inline::Text(" c ".into()),
                Inline::Code("d".into()),
                Inline::Text(" ".into()),
                Inline::Link { text: "e".into(), url: "http://x".into() },
                Inline::Text(" ".into()),
                Inline::Strike("f".into()),
            ]
        );
    }

    #[test]
    fn an_unclosed_mark_stays_text() {
        // A stray asterisk is an asterisk. Treating it as the start of
        // emphasis that never ends would swallow the rest of the line.
        assert_eq!(inline("2 * 3 = 6"), vec![Inline::Text("2 * 3 = 6".into())]);
    }

    #[test]
    fn code_wins_over_emphasis() {
        // Inside backticks a `*` is an asterisk, which is the whole reason to
        // write `*` in backticks.
        assert_eq!(inline("`a*b*c`"), vec![Inline::Code("a*b*c".into())]);
    }

    #[test]
    fn html_in_the_source_is_shown_not_run() {
        let html = to_html(&lines("<script>alert(1)</script>"));
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(!html.contains("<script>"), "{html}");
    }

    #[test]
    fn 押せる升は_押せるものとして名乗る() {
        // `<span>` で出していた頃、読み上げはただの字として読み、Tab では
        // 辿り着けなかった ── 押せる升が一つ出せないだけで、ノートの
        // 半分が触れなくなる。
        let out = super::to_html(&lines("- [ ] やること\n- [x] 済んだ\n"));
        assert!(out.contains("<button type=\"button\" class=\"box\""), "升が押せない: {out}");
        assert!(out.contains("aria-pressed=\"false\""), "入り切りが伝わらない: {out}");
        assert!(out.contains("aria-pressed=\"true\""), "入り切りが伝わらない: {out}");
        // 行番号は残す ── `note::set_check` が取るのはこれ。
        assert!(out.contains("data-line=\"0\"") && out.contains("data-line=\"1\""), "{out}");
    }

    #[test]
    fn github_の注記は五つだけ受ける() {
        let out = to_html(&lines("> [!NOTE]\n> 覚えておくこと。\n"));
        assert!(out.contains("<div class=\"alert note\">"), "枠にならない: {out}");
        assert!(out.contains("覚えておくこと。"), "中身が消えた: {out}");
        let out = to_html(&lines("> [!WARNING]\n> 気をつける。\n"));
        assert!(out.contains("alert warning"), "{out}");

        // ふつうの引用は、ふつうの引用のまま。
        let out = to_html(&lines("> ふつう\n"));
        assert!(out.contains("<blockquote>") && !out.contains("alert"), "{out}");
        // **引用の中の一行目でしか始まらない。** 本文に書いた角括弧が
        // 消えると、書いたものが読めるものと違う。
        let out = to_html(&lines("本文に [!NOTE] と書いた\n"));
        assert!(out.contains("[!NOTE]"), "本文の角括弧が消えた: {out}");
        // 知らない種類は、ふつうの引用。ここでだけ見える記法を増やさない。
        let out = to_html(&lines("> [!SPICY]\n> から\n"));
        assert!(!out.contains("alert"), "知らない種類が枠になった: {out}");
    }

    #[test]
    fn かたまりは元の行を持って出る() {
        // 読む面で直に書き換えるのに要る ── 押したかたまりの元の字が
        // 取れなければ、読む面は読むだけの面のままになる。
        let out = super::to_html(&lines("---\ntitle: t\n---\n\n# 題\n\n本文。\n続き。\n"));
        assert!(out.contains("<h1 id=\"題\" data-line=\"4\" data-span=\"1\">"), "{out}");
        // 折り返した段落は**一つのかたまりで二行ぶん**。
        assert!(out.contains("<p data-line=\"6\" data-span=\"2\">"), "{out}");

        // 枠は閉じるまでが一つ。
        let out = super::to_html(&lines("```\na\nb\n```\nあと\n"));
        assert!(out.contains("<pre data-line=\"0\" data-span=\"4\">"), "{out}");

        // **閉じ札には差さない。** 箇条書きの二つ目は前の項目を閉じてから
        // 始まるので、素朴に「次の `>`」を探すと `</li data-line=…>` に
        // なる（一度そうなった）。
        let out = super::to_html(&lines("- あ\n- い\n"));
        assert!(!out.contains("</li data-line"), "閉じ札に差さっている: {out}");
        assert!(out.contains("<li data-line=\"1\""), "二つ目に差さっていない: {out}");

        // 引用の中で数え直さない ── 中の行番号はファイルの行番号ではない。
        let out = super::to_html(&lines("> 引用\n> の中\n"));
        assert_eq!(out.matches("data-line").count(), 1, "中まで差さっている: {out}");
    }

    #[test]
    fn 番号つきは番号で出る() {
        // 書いた `1. 2.` が黒丸で出ると、書いたものと読めるものが違う。
        let out = to_html(&lines("1. 一つ\n2. 二つ\n"));
        assert!(out.contains("<ol>"), "番号つきが <ol> で出ていない: {out}");
        assert!(!out.contains("<ul>"), "黒丸が混ざっている: {out}");
        assert!(out.contains("</ol>"), "閉じていない: {out}");

        // 途中から続ける手順は、書いた番号から始まる。
        let out = to_html(&lines("3. 三つめから\n"));
        assert!(out.contains("<ol start=\"3\">"), "始まりが渡っていない: {out}");

        // 黒丸は黒丸のまま。
        let out = to_html(&lines("- 黒丸\n"));
        assert!(out.contains("<ul>") && !out.contains("<ol"), "{out}");

        // 同じ深さで印が変われば、別のリスト。
        let out = to_html(&lines("- 黒丸\n1. 番号\n"));
        assert!(out.contains("</ul>") && out.contains("<ol>"), "混ざっている: {out}");
    }

    #[test]
    fn 行そのものが絵なら絵で出る() {
        let out = to_html(&lines("![猫](cat.jpg)\n"));
        assert!(out.contains("<img src=\"cat.jpg\" alt=\"猫\">"), "絵になっていない: {out}");
        assert!(!out.contains("!<a"), "`!` が字のまま残っている: {out}");

        // 出せない先は、隠さずに字で残す。
        let out = to_html(&lines("![だめ](javascript:alert(1))\n"));
        assert!(!out.contains("<img"), "危ない絵が出ている: {out}");
        assert!(!out.contains("javascript:alert(1)</"), "そのまま href になっている: {out}");
        assert!(out.contains("だめ"), "書いてあったものが消えている: {out}");
    }

    #[test]
    fn a_javascript_link_is_not_a_link() {
        // The oldest trick there is, and a README is a file from somewhere.
        // The text still shows; it just does not go anywhere.
        let html = to_html(&lines("[click](javascript:alert(1))"));
        assert!(html.contains("click"), "{html}");
        assert!(!html.contains("<a "), "{html}");
    }

    #[test]
    fn relative_links_still_work() {
        let html = to_html(&lines("[readme](docs/README.md)"));
        assert!(html.contains(r#"<a href="docs/README.md">readme</a>"#), "{html}");
    }

    #[test]
    fn a_table_keeps_its_alignment() {
        let html = to_html(&lines("| a | b |\n| :- | --: |\n| 1 | 2 |"));
        assert!(html.contains("<table>"), "{html}");
        assert!(html.contains(r#"<th style="text-align:right">b</th>"#), "{html}");
    }

    #[test]
    fn nested_lists_close_in_order() {
        let html = to_html(&lines("- one\n  - deep\n- two"));
        assert_eq!(html.matches("<ul>").count(), 2, "{html}");
        assert_eq!(html.matches("</ul>").count(), 2, "{html}");
        assert_eq!(html.matches("<li>").count(), 3, "{html}");
        assert_eq!(html.matches("</li>").count(), 3, "{html}");
    }

    #[test]
    fn a_nested_list_sits_inside_its_parent_item() {
        // `<li>one</li><ul>…</ul>` is what the first version produced:
        // tolerated by browsers and then indented as though the nesting were
        // not there.
        let html = to_html(&lines("- one\n  - deep"));
        let li = html.find("<li>one").unwrap();
        let ul = html[li..].find("<ul>").unwrap();
        let close = html[li..].find("</li>").unwrap();
        assert!(ul < close, "nested <ul> must come before its parent's </li>\n{html}");
    }

    #[test]
    fn a_fence_is_verbatim() {
        let html = to_html(&lines("```rust\nlet x = *p;\n```"));
        assert!(html.contains(r#"<code class="language-rust">"#), "{html}");
        assert!(html.contains("let x = *p;"), "{html}");
        // Not turned into emphasis on the way past.
        assert!(!html.contains("<em>"), "{html}");
    }

    #[test]
    fn a_list_closes_before_what_follows_it() {
        // The first version dropped the record of the outermost list without
        // emitting its `</ul>`, so the paragraph after a list arrived inside
        // it — indented for ever, in every document that has a list.
        let html = to_html(&lines("- one\n  - deep\n- two\n\npara"));
        assert_eq!(html.matches("<ul>").count(), html.matches("</ul>").count(), "{html}");
        assert!(html.find("</ul>").unwrap() < html.find("<p>para").unwrap(), "{html}");
    }

    #[test]
    fn a_hard_wrapped_paragraph_is_one_paragraph() {
        let html = to_html(&lines("one\ntwo\n\nthree"));
        assert_eq!(html.matches("<p>").count(), 2, "{html}");
        assert!(html.contains("<p>one two</p>"), "{html}");
    }

    #[test]
    fn task_boxes_are_marked_and_carry_the_line_they_came_from() {
        let html = super::to_html(&lines("- [x] done\n- [ ] not"));
        assert!(html.contains("☑"), "{html}");
        assert!(html.contains("☐"), "{html}");
        // 押せるようにするのに要るのはこれだけ ── `note::set_check` は
        // 行番号を取る。前書きの分もちゃんと数える。
        assert!(html.contains("data-line=\"0\""), "{html}");
        assert!(html.contains("data-line=\"1\""), "{html}");
        let with_front = super::to_html(&lines("---\ntitle: x\n---\n\n- [ ] a\n"));
        assert!(with_front.contains("data-line=\"4\""), "{with_front}");
    }

    #[test]
    fn headings_get_an_anchor_to_link_to() {
        assert_eq!(slug("Usage"), "usage");
        assert_eq!(slug("Getting started!"), "getting-started");
        // Two spaces are two hyphens, and so is a dropped dash between them.
        // GitHub's own slugger does this, and a README's links were written
        // against GitHub's.
        assert_eq!(slug("v1.2 — notes (draft)"), "v12--notes-draft");
        // Japanese is kept: stripping it would collapse every heading in a
        // Japanese document to the same empty anchor.
        assert_eq!(slug("使い方"), "使い方");
        assert_eq!(slug("  trailing  "), "trailing", "the ends are trimmed first");
        assert_eq!(slug("###"), "");

        let html = to_html(&["# 使い方".to_string(), "## Getting started".to_string()]);
        assert!(html.contains("<h1 id=\"使い方\">"), "{html}");
        assert!(html.contains("<h2 id=\"getting-started\">"), "{html}");
    }
}
