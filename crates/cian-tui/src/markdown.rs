//! A small Markdown → styled-terminal-lines renderer for the F3 viewer's
//! preview mode. It is a pragmatic, line-based parser (headings, emphasis,
//! inline/fenced code, blockquotes, lists, rules, links) — not a full CommonMark
//! engine — plus special handling for ```mermaid``` blocks, which are shown as a
//! clearly-boxed source block (a terminal cannot draw the diagram itself).

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::theme;
use crate::util::wrap_str;

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

/// Truncate `s` to `w` display columns, ending with `…` if it was cut.
fn clip_width(s: &str, w: usize) -> String {
    if s.width() <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in s.chars() {
        let cw = ch.to_string().width();
        if used + cw > w.saturating_sub(1) {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    out
}

/// Pad `s` to `w` display columns per `align` (assumes `s.width() <= w`).
fn pad_align(s: &str, w: usize, align: Align) -> String {
    let gap = w.saturating_sub(s.width());
    match align {
        Align::Left => format!("{s}{}", " ".repeat(gap)),
        Align::Right => format!("{}{s}", " ".repeat(gap)),
        Align::Center => {
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
    let body_style = Style::default().fg(Color::Rgb(210, 210, 224));

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
            let text = clip_width(&cells(i), *w);
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

        // Blockquote.
        if let Some(rest) = trimmed.strip_prefix('>') {
            let bar = Style::default().fg(Color::Rgb(120, 170, 210));
            let body = Style::default().fg(Color::Rgb(170, 185, 205)).add_modifier(Modifier::ITALIC);
            for chunk in wrap_str(rest.trim_start(), width.saturating_sub(2)) {
                let mut spans = vec![Span::styled("▏ ".to_string(), bar)];
                spans.extend(inline(&chunk, body, width));
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
            let bullet = Style::default().fg(theme().accent).add_modifier(Modifier::BOLD);
            let avail = width.saturating_sub(indent + marker.chars().count() + 1);
            let wrapped = inline(&text, Style::default().fg(Color::Rgb(210, 210, 224)), avail);
            let mut spans = vec![
                Span::styled(pad.clone(), Style::default()),
                Span::styled(format!("{} ", marker), bullet),
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
        let base = Style::default().fg(Color::Rgb(205, 205, 218));
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

/// A fenced code block as boxed, monospaced lines. `mermaid` gets a labelled
/// header so the notation is clearly visible even though the diagram isn't drawn.
fn code_block(lang: &str, lines: &[String], width: usize) -> Vec<Line<'static>> {
    let bg = Style::default().bg(Color::Rgb(30, 30, 42));
    let mut out = Vec::new();
    let label = if lang == "mermaid" {
        " mermaid diagram (source) ".to_string()
    } else if lang.is_empty() {
        " code ".to_string()
    } else {
        format!(" {} ", lang)
    };
    out.push(Line::from(Span::styled(
        format!("{:<w$}", label, w = width),
        Style::default().bg(Color::Rgb(45, 45, 62)).fg(theme().accent).add_modifier(Modifier::BOLD),
    )));
    let code_fg = if lang == "mermaid" { Color::Rgb(150, 205, 235) } else { Color::Rgb(200, 205, 215) };
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
        // The title text survives (styling aside), the mermaid label appears,
        // and the diagram source line is kept.
        let flat: Vec<String> = out.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect();
        assert!(flat.iter().any(|l| l.contains("Title")));
        assert!(flat.iter().any(|l| l.contains("mermaid diagram")));
        assert!(flat.iter().any(|l| l.contains("A-->B")));
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
