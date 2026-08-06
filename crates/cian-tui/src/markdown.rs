//! A small Markdown → styled-terminal-lines renderer for the F3 viewer's
//! preview mode. It is a pragmatic, line-based parser (headings, emphasis,
//! inline/fenced code, blockquotes, lists, rules, links) — not a full CommonMark
//! engine. Pipe tables render as bordered, aligned boxes, task lists as
//! checkboxes, and ```mermaid``` `graph`/`flowchart` blocks become a readable
//! arrow-list "flow" (a terminal cannot draw the diagram itself); other mermaid
//! diagram types fall back to a clearly-boxed source block.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::render::readable_on;
use crate::theme;
use crate::theme::surface;
use crate::util::{pad_left, pad_to, truncate, wrap_str};

/// Column alignment for a pipe-table, from its `:---:` separator row.
#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    Center,
    Right,
}

/// Is `line` a table separator row (`|---|:--:|---:|`)? Every cell must be dashes
/// with optional leading/trailing colons, and there must be at least one dash.
fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    if !t.contains('-') || !t.contains('|') {
        return false;
    }
    let cells = split_cells(t);
    !cells.is_empty()
        && cells.iter().all(|c| {
            let c = c.trim();
            !c.is_empty() && c.contains('-') && c.chars().all(|ch| ch == '-' || ch == ':')
        })
}

/// Split a table row into trimmed cells, dropping the outer pipes.
fn split_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

/// Alignment from a separator cell: `:--` left, `--:` right, `:-:` centre.
fn cell_align(sep: &str) -> Align {
    let c = sep.trim();
    match (c.starts_with(':'), c.ends_with(':')) {
        (true, true) => Align::Center,
        (false, true) => Align::Right,
        _ => Align::Left,
    }
}

/// The plain text of an inline-formatted string (markers stripped), for a table
/// cell — so `**x**` measures and shows as `x`.
fn plain(text: &str) -> String {
    inline(text, Style::default(), usize::MAX)
        .iter()
        .map(|s| s.content.as_ref())
        .collect()
}

/// Lighten an RGB colour by `n` per channel — used to lift code blocks and
/// blockquotes off the viewer's (themed) background so they stand out.
fn elevate(c: Color, n: u8) -> Color {
    match c {
        Color::Rgb(r, g, b) => Color::Rgb(r.saturating_add(n), g.saturating_add(n), b.saturating_add(n)),
        _ => Color::Rgb(45, 45, 62),
    }
}

/// Pad `s` to `w` display columns per `align` (assumes `s.width() <= w`).
fn pad_align(s: &str, w: usize, align: Align) -> String {
    match align {
        Align::Left => pad_to(s, w),
        Align::Right => pad_left(s, w),
        Align::Center => {
            let gap = w.saturating_sub(s.width());
            let l = gap / 2;
            format!("{}{s}{}", " ".repeat(l), " ".repeat(gap - l))
        }
    }
}

/// Render a pipe-table as a bordered, column-aligned box.
fn render_table(
    header: &[String],
    aligns: &[Align],
    rows: &[Vec<String>],
    width: usize,
) -> Vec<Line<'static>> {
    let ncols = header.len().max(1);
    let cell = |row: &[String], i: usize| plain(row.get(i).map(String::as_str).unwrap_or(""));
    let align = |i: usize| aligns.get(i).copied().unwrap_or(Align::Left);

    // Natural column widths from the content.
    let mut colw = vec![0usize; ncols];
    for (i, w) in colw.iter_mut().enumerate() {
        *w = plain(header.get(i).map(String::as_str).unwrap_or("")).width();
    }
    for row in rows {
        for (i, w) in colw.iter_mut().enumerate() {
            *w = (*w).max(cell(row, i).width());
        }
    }
    // Shrink the widest columns until the whole table fits `width`. Frame cost is
    // `3*ncols + 1` (a `│`, two padding spaces per column, plus the last `│`).
    let frame = 3 * ncols + 1;
    let budget = width.saturating_sub(frame).max(ncols); // ≥ 1 col each
    while colw.iter().sum::<usize>() > budget {
        let (idx, _) = colw.iter().enumerate().max_by_key(|(_, w)| **w).unwrap();
        if colw[idx] <= 1 {
            break;
        }
        colw[idx] -= 1;
    }

    let border = Style::default().fg(Color::Rgb(90, 90, 110));
    let head_style = Style::default().fg(theme().accent).add_modifier(Modifier::BOLD);
    let body_style = Style::default().fg(readable_on(surface()));

    // Border rows: left/mid/right corners joined by `fill` across each column.
    let rule = |left: &str, mid: &str, right: &str, fill: &str| {
        let mut s = String::from(left);
        for (i, w) in colw.iter().enumerate() {
            s.push_str(&fill.repeat(w + 2));
            s.push_str(if i + 1 == ncols { right } else { mid });
        }
        Line::from(Span::styled(s, border))
    };
    let data_row = |cells: &dyn Fn(usize) -> String, style: Style| {
        let mut spans = vec![Span::styled("│".to_string(), border)];
        for (i, w) in colw.iter().enumerate() {
            let text = truncate(&cells(i), *w);
            spans.push(Span::styled(format!(" {} ", pad_align(&text, *w, align(i))), style));
            spans.push(Span::styled("│".to_string(), border));
        }
        Line::from(spans)
    };

    let mut out = Vec::new();
    out.push(rule("┌", "┬", "┐", "─"));
    let hdr: Vec<String> = (0..ncols).map(|i| plain(header.get(i).map(String::as_str).unwrap_or(""))).collect();
    out.push(data_row(&|i| hdr[i].clone(), head_style));
    out.push(rule("├", "┼", "┤", "─"));
    for row in rows {
        let r: Vec<String> = (0..ncols).map(|i| cell(row, i)).collect();
        out.push(data_row(&|i| r[i].clone(), body_style));
    }
    out.push(rule("└", "┴", "┘", "─"));
    out
}

/// Render Markdown to a plain-text grid plus a parallel per-character style
/// grid. The viewer drives its cursor / selection / search over the plain text
/// and paints each character with the matching base style, so all the viewer's
/// machinery works over the rendered document unchanged.
pub(crate) fn render_styled(source: &[String], width: usize) -> (Vec<String>, Vec<Vec<Style>>) {
    let lines = render(source, width);
    let mut plain = Vec::with_capacity(lines.len());
    let mut styles = Vec::with_capacity(lines.len());
    for line in &lines {
        let mut text = String::new();
        let mut st = Vec::new();
        for span in &line.spans {
            for ch in span.content.chars() {
                text.push(ch);
                st.push(span.style);
            }
        }
        plain.push(text);
        styles.push(st);
    }
    (plain, styles)
}

/// Render Markdown `source` lines into styled, width-wrapped display lines.
pub(crate) fn render(source: &[String], width: usize) -> Vec<Line<'static>> {
    let width = width.max(8);
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut i = 0;
    while i < source.len() {
        let raw = &source[i];
        let trimmed = raw.trim_start();

        // Fenced code block: ``` or ~~~ (optionally with a language).
        if let Some(lang) = fence_lang(trimmed) {
            i += 1;
            let mut code = Vec::new();
            while i < source.len() && fence_lang(source[i].trim_start()).is_none() {
                code.push(source[i].clone());
                i += 1;
            }
            i += 1; // consume the closing fence (if any)
            out.extend(code_block(&lang, &code, width));
            continue;
        }

        // Horizontal rule.
        if is_rule(trimmed) {
            out.push(Line::from(Span::styled(
                "─".repeat(width),
                Style::default().fg(Color::Rgb(90, 90, 110)),
            )));
            i += 1;
            continue;
        }

        // ATX heading (# .. ######).
        if let Some((level, text)) = heading(trimmed) {
            if !out.is_empty() {
                out.push(Line::from(""));
            }
            let color = theme().accent;
            let prefix = match level {
                1 => "█ ",
                2 => "▊ ",
                _ => "▎ ",
            };
            let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
            let mut spans = vec![Span::styled(prefix.to_string(), style)];
            spans.extend(inline(&text, style, width.saturating_sub(2)));
            out.push(Line::from(spans));
            if level <= 2 {
                out.push(Line::from(Span::styled(
                    "─".repeat(width),
                    Style::default().fg(Color::Rgb(70, 70, 90)),
                )));
            }
            i += 1;
            continue;
        }

        // Blockquote — a coloured left bar over a subtly raised background band
        // so it reads as a quote against the themed viewer surface.
        if let Some(rest) = trimmed.strip_prefix('>') {
            let qbg = elevate(surface(), 14);
            let bar = Style::default().fg(theme().accent).bg(qbg).add_modifier(Modifier::BOLD);
            let body = Style::default().fg(readable_on(qbg)).bg(qbg).add_modifier(Modifier::ITALIC);
            for chunk in wrap_str(rest.trim_start(), width.saturating_sub(2)) {
                let mut spans = vec![Span::styled("▎ ".to_string(), bar)];
                spans.extend(inline(&chunk, body, width));
                // Pad the band to full width so the background is a solid block.
                let used: usize = spans.iter().map(|s| s.content.width()).sum();
                if used < width {
                    spans.push(Span::styled(" ".repeat(width - used), Style::default().bg(qbg)));
                }
                out.push(Line::from(spans));
            }
            i += 1;
            continue;
        }

        // Pipe table: a header row, a `|---|:--:|` separator, then body rows.
        if raw.contains('|')
            && i + 1 < source.len()
            && is_table_separator(&source[i + 1])
        {
            let header = split_cells(raw);
            let aligns = split_cells(&source[i + 1]).iter().map(|c| cell_align(c)).collect::<Vec<_>>();
            i += 2;
            let mut rows = Vec::new();
            while i < source.len() {
                let r = source[i].trim();
                if r.is_empty() || !r.contains('|') {
                    break;
                }
                rows.push(split_cells(&source[i]));
                i += 1;
            }
            out.extend(render_table(&header, &aligns, &rows, width));
            continue;
        }

        // Unordered / ordered list item.
        if let Some((marker, text, indent)) = list_item(raw) {
            let pad = " ".repeat(indent);
            // GitHub task list: `- [ ]` / `- [x]` becomes a checkbox glyph in
            // place of the bullet, with the marker stripped from the text.
            let (marker, text, mstyle) = if let Some(r) = task_item(&text) {
                match r {
                    (true, rest) => (
                        "☑".to_string(),
                        rest,
                        Style::default().fg(Color::Rgb(126, 211, 133)).add_modifier(Modifier::BOLD),
                    ),
                    (false, rest) => (
                        "☐".to_string(),
                        rest,
                        Style::default().fg(theme().dim).add_modifier(Modifier::BOLD),
                    ),
                }
            } else {
                (marker, text, Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            };
            let avail = width.saturating_sub(indent + marker.chars().count() + 1);
            let wrapped = inline(&text, Style::default().fg(readable_on(surface())), avail);
            let mut spans = vec![
                Span::styled(pad.clone(), Style::default()),
                Span::styled(format!("{} ", marker), mstyle),
            ];
            spans.extend(wrapped);
            out.push(Line::from(spans));
            i += 1;
            continue;
        }

        // Blank line.
        if trimmed.is_empty() {
            out.push(Line::from(""));
            i += 1;
            continue;
        }

        // Plain paragraph text, wrapped.
        let base = Style::default().fg(readable_on(surface()));
        for chunk in wrap_str(trimmed, width) {
            out.push(Line::from(inline(&chunk, base, width)));
        }
        i += 1;
    }
    out
}

/// If `line` opens/closes a fenced code block, return its language tag (empty
/// string when none). ` ``` `, ` ```rust `, `~~~`.
fn fence_lang(line: &str) -> Option<String> {
    let t = line.trim_end();
    t.strip_prefix("```")
        .or_else(|| t.strip_prefix("~~~"))
        .map(|rest| rest.trim().to_lowercase())
}

fn is_rule(t: &str) -> bool {
    let t = t.trim();
    (t.len() >= 3) && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

fn heading(t: &str) -> Option<(usize, String)> {
    let hashes = t.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && t.chars().nth(hashes) == Some(' ') {
        Some((hashes, t[hashes + 1..].trim().to_string()))
    } else {
        None
    }
}

/// `(bullet-or-number, text, indent)` for a list line, else None.
fn list_item(raw: &str) -> Option<(String, String, usize)> {
    let indent = raw.len() - raw.trim_start().len();
    let t = raw.trim_start();
    for m in ['-', '*', '+'] {
        if let Some(rest) = t.strip_prefix(m) {
            if rest.starts_with(' ') {
                return Some(("•".to_string(), rest.trim_start().to_string(), indent));
            }
        }
    }
    // Ordered: "1." / "12)".
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        let after = &t[digits.len()..];
        if let Some(rest) = after.strip_prefix('.').or_else(|| after.strip_prefix(')')) {
            if rest.starts_with(' ') {
                return Some((format!("{}.", digits), rest.trim_start().to_string(), indent));
            }
        }
    }
    None
}

/// One token of a flowchart line: a node reference (raw text incl. any brackets)
/// or an arrow with its optional `|label|`.
enum Tok {
    Node(String),
    Arrow(String),
}

/// Collect `id[label]` / `id(label)` / `id{label}` / `id((label))` declarations
/// from one line into `map` (id → display label), so a later bare `id` in an
/// edge can be shown by its label.
fn collect_node_labels(line: &str, map: &mut std::collections::HashMap<String, String>) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_alphanumeric() || chars[i] == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let id: String = chars[start..i].iter().collect();
            if let Some((label, next)) = read_bracket_label(&chars, i) {
                map.entry(id).or_insert(label);
                i = next;
            }
        } else {
            i += 1;
        }
    }
}

/// If `chars[i]` opens a node label (`[`, `(`, `{`, possibly doubled like `((`),
/// return the inner label (quotes stripped) and the index past the close.
fn read_bracket_label(chars: &[char], i: usize) -> Option<(String, usize)> {
    let open = *chars.get(i)?;
    let close = match open {
        '[' => ']',
        '(' => ')',
        '{' => '}',
        _ => return None,
    };
    let mut j = i;
    let mut depth = 0; // count leading opens (handles (( )) / [[ ]])
    while chars.get(j) == Some(&open) {
        depth += 1;
        j += 1;
    }
    let text_start = j;
    let mut closes = 0;
    while j < chars.len() && closes < depth {
        if chars[j] == close {
            closes += 1;
        } else {
            closes = 0;
        }
        j += 1;
    }
    let label: String = chars[text_start..j.saturating_sub(depth)].iter().collect();
    let label = label.trim().trim_matches('"').trim().to_string();
    Some((label, j))
}

/// The display label for a node reference token like `A`, `A[X]`, `A(( Y ))`.
fn node_label(token: &str, map: &std::collections::HashMap<String, String>) -> String {
    let chars: Vec<char> = token.trim().chars().collect();
    let mut i = 0;
    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
        i += 1;
    }
    let id: String = chars[..i].iter().collect();
    if let Some((label, _)) = read_bracket_label(&chars, i) {
        if !label.is_empty() {
            return label;
        }
    }
    map.get(&id).cloned().unwrap_or(id)
}

/// Tokenise a flowchart line into nodes and arrows.
fn tokenize_flow(line: &str) -> Vec<Tok> {
    let s = line.trim().trim_end_matches(';');
    let chars: Vec<char> = s.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    let mut buf = String::new();
    let mut depth = 0i32; // inside [] () {}
    while i < chars.len() {
        let c = chars[i];
        if depth == 0 && (c == '-' || c == '=' || c == '<') {
            // Start of an arrow.
            if !buf.trim().is_empty() {
                toks.push(Tok::Node(std::mem::take(&mut buf)));
            } else {
                buf.clear();
            }
            while i < chars.len() && matches!(chars[i], '-' | '=' | '.' | '<' | '>') {
                i += 1;
            }
            if matches!(chars.get(i), Some('x') | Some('o')) {
                i += 1; // --x / --o arrowheads
            }
            let mut label = String::new();
            if chars.get(i) == Some(&'|') {
                i += 1;
                while i < chars.len() && chars[i] != '|' {
                    label.push(chars[i]);
                    i += 1;
                }
                i += 1; // closing |
            }
            toks.push(Tok::Arrow(label.trim().to_string()));
            continue;
        }
        if matches!(c, '[' | '(' | '{') {
            depth += 1;
        } else if matches!(c, ']' | ')' | '}') {
            depth -= 1;
        }
        buf.push(c);
        i += 1;
    }
    if !buf.trim().is_empty() {
        toks.push(Tok::Node(buf));
    }
    toks
}

/// Render a `graph` / `flowchart` mermaid block as a readable list of edges
/// (`from ──label──▶ to`) with node labels resolved. `None` for non-flow diagram
/// types (sequence, class, …), which fall back to the source box.
fn mermaid_flow(lines: &[String], width: usize) -> Option<Vec<Line<'static>>> {
    let mut idx = 0;
    while idx < lines.len() && lines[idx].trim().is_empty() {
        idx += 1;
    }
    let header = lines.get(idx)?.trim();
    if !(header.starts_with("graph") || header.starts_with("flowchart")) {
        return None;
    }

    let mut map = std::collections::HashMap::new();
    for l in lines {
        collect_node_labels(l, &mut map);
    }

    // Edges may sit after the direction on the header line (`graph TD; A-->B`)
    // as well as on their own lines, so parse the header's tail too.
    let head_tail = header.split_once(';').map(|(_, t)| t.to_string()).unwrap_or_default();
    let mut edges: Vec<(String, String, String)> = Vec::new();
    for l in std::iter::once(&head_tail).chain(lines[idx + 1..].iter()) {
        let lt = l.trim();
        if lt.is_empty() || lt.starts_with("%%") || lt.starts_with("subgraph") || lt == "end" {
            continue;
        }
        let toks = tokenize_flow(l);
        let mut k = 0;
        while k + 2 < toks.len() + 1 {
            if let (Some(Tok::Node(a)), Some(Tok::Arrow(lbl)), Some(Tok::Node(b))) =
                (toks.get(k), toks.get(k + 1), toks.get(k + 2))
            {
                edges.push((node_label(a, &map), lbl.clone(), node_label(b, &map)));
                k += 2; // chain: reuse b as the next `from`
            } else {
                break;
            }
        }
    }
    if edges.is_empty() {
        return None;
    }

    let base = surface();
    let bg = elevate(base, 14);
    let node = Style::default().fg(readable_on(bg)).bg(bg).add_modifier(Modifier::BOLD);
    let arrow = Style::default().fg(theme().accent).bg(bg);
    let lbl = Style::default().fg(Color::Rgb(180, 205, 150)).bg(bg).add_modifier(Modifier::ITALIC);
    let fill = Style::default().bg(bg);

    let mut out = Vec::new();
    out.push(Line::from(Span::styled(
        format!("{:<w$}", " mermaid flow ", w = width),
        Style::default().bg(elevate(base, 30)).fg(theme().accent).add_modifier(Modifier::BOLD),
    )));
    for (from, elabel, to) in &edges {
        let mut spans = vec![Span::styled("  ".to_string(), fill), Span::styled(from.clone(), node)];
        if elabel.is_empty() {
            spans.push(Span::styled("  ──▶  ".to_string(), arrow));
        } else {
            spans.push(Span::styled("  ──".to_string(), arrow));
            spans.push(Span::styled(elabel.clone(), lbl));
            spans.push(Span::styled("──▶  ".to_string(), arrow));
        }
        spans.push(Span::styled(to.clone(), node));
        let used: usize = spans.iter().map(|s| s.content.width()).sum();
        if used < width {
            spans.push(Span::styled(" ".repeat(width - used), fill));
        }
        out.push(Line::from(spans));
    }
    Some(out)
}

/// A task-list item `[ ] rest` / `[x] rest` → `(checked, rest)`.
fn task_item(text: &str) -> Option<(bool, String)> {
    let t = text.trim_start();
    if let Some(r) = t.strip_prefix("[ ]") {
        return Some((false, r.trim_start().to_string()));
    }
    for mark in ["[x]", "[X]"] {
        if let Some(r) = t.strip_prefix(mark) {
            return Some((true, r.trim_start().to_string()));
        }
    }
    None
}

/// A fenced code block as boxed, monospaced lines, on a theme-derived surface
/// raised off the viewer background. A ```mermaid``` block is first parsed into a
/// readable flow (arrows between node labels); only if that fails does it fall
/// back to the labelled source box (a terminal cannot draw the diagram itself).
fn code_block(lang: &str, lines: &[String], width: usize) -> Vec<Line<'static>> {
    if lang == "mermaid" {
        if let Some(flow) = mermaid_flow(lines, width) {
            return flow;
        }
    }
    let base = surface();
    let code_bg = elevate(base, 18);
    let bg = Style::default().bg(code_bg);
    let mut out = Vec::new();
    let label = if lang == "mermaid" {
        " mermaid (source) ".to_string()
    } else if lang.is_empty() {
        " code ".to_string()
    } else {
        format!(" {} ", lang)
    };
    out.push(Line::from(Span::styled(
        format!("{:<w$}", label, w = width),
        Style::default().bg(elevate(base, 34)).fg(theme().accent).add_modifier(Modifier::BOLD),
    )));
    let code_fg = readable_on(code_bg);
    for l in lines {
        let shown = format!("  {:<w$}", l, w = width.saturating_sub(2));
        out.push(Line::from(Span::styled(shown, bg.fg(code_fg))));
    }
    out
}

/// Parse inline emphasis / code / links in `text` into styled spans, on top of
/// `base`. Wrapping is left to the caller (the text is already a chunk).
fn inline(text: &str, base: Style, _width: usize) -> Vec<Span<'static>> {
    let code_style = Style::default().bg(Color::Rgb(45, 45, 62)).fg(Color::Rgb(240, 210, 150));
    let link_style = base.fg(theme().accent).add_modifier(Modifier::UNDERLINED);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let flush = |spans: &mut Vec<Span<'static>>, buf: &mut String| {
        if !buf.is_empty() {
            spans.push(Span::styled(std::mem::take(buf), base));
        }
    };
    while i < chars.len() {
        let c = chars[i];
        // Inline code `...`. The coloured background is the marker, so the text
        // is not padded — padding it looked wrong hugged against punctuation,
        // e.g. `(`meso`)` rendering as `( meso )`.
        if c == '`' {
            if let Some(end) = chars[i + 1..].iter().position(|&x| x == '`') {
                flush(&mut spans, &mut buf);
                let inner: String = chars[i + 1..i + 1 + end].iter().collect();
                spans.push(Span::styled(inner, code_style));
                i += end + 2;
                continue;
            }
        }
        // Bold **...** or __...__
        if (c == '*' || c == '_') && i + 1 < chars.len() && chars[i + 1] == c {
            let marker = [c, c];
            if let Some(end) = find_run(&chars, i + 2, &marker) {
                flush(&mut spans, &mut buf);
                let inner: String = chars[i + 2..end].iter().collect();
                spans.push(Span::styled(inner, base.add_modifier(Modifier::BOLD)));
                i = end + 2;
                continue;
            }
        }
        // Italic *...* or _..._
        if c == '*' || c == '_' {
            if let Some(end) = chars[i + 1..].iter().position(|&x| x == c) {
                let inner: String = chars[i + 1..i + 1 + end].iter().collect();
                if !inner.is_empty() && !inner.starts_with(' ') {
                    flush(&mut spans, &mut buf);
                    spans.push(Span::styled(inner, base.add_modifier(Modifier::ITALIC)));
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
                        flush(&mut spans, &mut buf);
                        let label: String = chars[i + 1..i + 1 + close].iter().collect();
                        spans.push(Span::styled(label, link_style));
                        i = after + 1 + paren + 1;
                        continue;
                    }
                }
            }
        }
        buf.push(c);
        i += 1;
    }
    flush(&mut spans, &mut buf);
    if spans.is_empty() {
        spans.push(Span::styled(String::new(), base));
    }
    spans
}

/// Index of the start of a two-char `marker` run at or after `from`.
fn find_run(chars: &[char], from: usize, marker: &[char; 2]) -> Option<usize> {
    let mut i = from;
    while i + 1 < chars.len() {
        if chars[i] == marker[0] && chars[i + 1] == marker[1] {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(|l| l.to_string()).collect()
    }

    #[test]
    fn headings_lists_and_code_render_to_lines() {
        let src = lines("# Title\n\nSome **bold** and `code`.\n\n- one\n- two\n\n```mermaid\ngraph TD; A-->B\n```\n");
        let out = render(&src, 40);
        // The title text survives (styling aside), and the mermaid flow renders
        // the A → B edge with node labels and an arrow.
        let flat: Vec<String> = out.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect();
        assert!(flat.iter().any(|l| l.contains("Title")));
        assert!(flat.iter().any(|l| l.contains("mermaid flow")));
        assert!(flat.iter().any(|l| l.contains("A") && l.contains("▶") && l.contains("B")));
        assert!(flat.iter().any(|l| l.contains("• one")));
    }

    #[test]
    fn a_pipe_table_renders_bordered_and_aligned() {
        let src = lines("| Name | Qty | Note |\n|:-----|----:|:----:|\n| apple | 3 | ok |\n| pear | 12 | **hi** |\n");
        let out = render(&src, 60);
        let flat: Vec<String> = out
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect();
        // Box-drawing frame present.
        assert!(flat.iter().any(|l| l.starts_with('┌') && l.ends_with('┐')), "top border: {flat:?}");
        assert!(flat.iter().any(|l| l.starts_with('├')), "header separator");
        assert!(flat.iter().any(|l| l.starts_with('└')), "bottom border");
        // Header and cells appear; emphasis markers are stripped in a cell.
        assert!(flat.iter().any(|l| l.contains("Name") && l.contains("Qty")));
        assert!(flat.iter().any(|l| l.contains("apple")));
        assert!(flat.iter().any(|l| l.contains("hi") && !l.contains("**hi**")), "markers stripped");
        // Right-aligned Qty column: "12" hugs the right padding.
        assert!(flat.iter().any(|l| l.contains("12 │")), "right-aligned qty: {flat:?}");
    }

    #[test]
    fn mermaid_flow_and_tasklist_render() {
        let src = lines("```mermaid\ngraph TD\n    A[ファイラー] -->|F3| B(ビューア)\n    B --> C{Markdown?}\n    C -->|Yes| D[プレビュー]\n    C -->|No| E[プレーン]\n```\n\n- [ ] todo\n- [x] done\n");
        let out = render(&src, 60);
        let flat: Vec<String> = out
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect();
        // Node labels resolved (not raw ids), arrows and edge labels shown.
        assert!(flat.iter().any(|l| l.contains("ファイラー") && l.contains("ビューア") && l.contains("F3")));
        assert!(flat.iter().any(|l| l.contains("Markdown?") && l.contains("Yes") && l.contains("プレビュー")));
        assert!(!flat.iter().any(|l| l.contains("A[ファイラー]")), "raw node syntax gone");
        // Task list: checkboxes, not bullets with literal [ ].
        assert!(flat.iter().any(|l| l.contains("☐") && l.contains("todo") && !l.contains("[ ]")));
        assert!(flat.iter().any(|l| l.contains("☑") && l.contains("done") && !l.contains("[x]")));
    }

    #[test]
    fn inline_emphasis_splits_into_spans() {
        let sp = inline("a **b** c", Style::default(), 40);
        // "a ", "b" (bold), " c" — the bold word is its own span.
        assert!(sp.iter().any(|s| s.content == "b" && s.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn inline_code_in_parens_is_not_padded() {
        // Regression: `(`meso`)` must render the code tight, not "( meso )".
        let sp = inline("hoge(`meso`)", Style::default(), 40);
        assert!(sp.iter().any(|s| s.content == "meso"), "code content is exactly `meso`: {:?}",
            sp.iter().map(|s| s.content.as_ref()).collect::<Vec<_>>());
        let flat: String = sp.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(flat, "hoge(meso)", "no stray spaces around the code");
    }
}
