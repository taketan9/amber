//! cian GUI spike — does a window-owning cian actually look right in Japanese?
//!
//! This answers the questions that decide the whole project, and nothing else.
//! It does not touch cian: it draws a fake cian screen with ratatui so the
//! *rendering* can be judged on its own.
//!
//! What it is checking, in order of how badly it would hurt to get wrong:
//!
//! 1. **全角 alignment.** Every row inside the ruler block is padded to the
//!    same display width. If wide characters advance by two cells, the right
//!    border is a straight line. If they do not, it is a staircase — and the
//!    whole idea is dead.
//! 2. **Nerd Font icons.** They come from a different font than the Japanese
//!    text, so this is really a test of the fallback chain: three fonts, one
//!    grid, one baseline.
//! 3. **Font size at runtime.** `+` / `-` re-sizes. This is the thing cian
//!    cannot do today without asking init.lua for a terminal-specific command.
//! 4. **What the keyboard delivers.** Every key press is dumped to a panel with
//!    its modifiers. The question is whether Ctrl+H and friends arrive intact,
//!    since on the terminal side they do not.
//! 5. **Cost.** Frame time is measured with a full repaint every frame, which
//!    is far more than cian asks for.
//!
//! Keys: `+`/`-` font size · `t` cycle test page · `Esc` quit.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use ratatui_wgpu::{Builder, Dimensions, Font, Fonts, WgpuBackend};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

/// Where the fonts come from.
///
/// Loaded from disk rather than embedded, because the point of the spike is to
/// try several and see which survives. The real thing embeds one file.
///
/// The order given here is the order they are handed over; the backend sorts
/// its own fallback chain by width, which is itself something to watch.
/// The answer this spike arrived at: one Japanese Nerd Font, ASCII 1 : 全角 2,
/// covering kana, kanji, box drawing, icons and dingbats at a single set of
/// metrics. When it is there, nothing else is used — mixing in a second font
/// is what broke ✔ and ✖ in the first place.
const FONT_PREFERRED: &str = "~/Downloads/HackGenConsoleNF-Regular.ttf";

/// The two-font chain, used only when the preferred font is missing. Kept so
/// the difference between one font and a fallback chain stays reproducible.
const FONT_FALLBACK_CHAIN: &[&str] = &[
    // ASCII, box drawing, and the Nerd Font private-use icons.
    "~/Library/Fonts/HackNerdFontMono-Regular.ttf",
    // Japanese. A .ttc collection — ttf-parser reads index 0 out of one.
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
];

const SIZE_MIN: u32 = 8;
const SIZE_MAX: u32 = 64;
const RULER_W: usize = 46;

fn expand(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var("HOME") {
            Ok(home) => format!("{home}/{rest}"),
            Err(_) => path.to_string(),
        },
        None => path.to_string(),
    }
}

/// Read every font that exists, leaking the bytes so the faces can borrow them
/// for the life of the process. A spike may leak; it runs once and exits.
fn load_fonts() -> Vec<(String, Font<'static>)> {
    // An explicit list on the command line beats the built-in one, so trying a
    // different font is a re-run rather than a re-edit.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths: Vec<String> = if !args.is_empty() {
        args.iter().map(|p| expand(p)).collect()
    } else if std::path::Path::new(&expand(FONT_PREFERRED)).exists() {
        vec![expand(FONT_PREFERRED)]
    } else {
        FONT_FALLBACK_CHAIN.iter().map(|p| expand(p)).collect()
    };

    let mut out = Vec::new();
    for full in paths {
        let Ok(bytes) = std::fs::read(&full) else {
            eprintln!("skip (not found):    {full}");
            continue;
        };
        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        match Font::new(leaked) {
            Some(f) => {
                let name = std::path::Path::new(&full)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| full.clone());
                eprintln!("loaded:              {name}");
                out.push((name, f));
            }
            // A .ttc that ttf-parser will not open is worth saying out loud —
            // silently falling back would look like a rendering bug later.
            None => eprintln!("skip (unparseable):  {full}"),
        }
    }
    if out.is_empty() {
        eprintln!("\nno font loaded. pass one or more font files as arguments:");
        eprintln!("    cargo run --release -- /path/to/Font.ttf /path/to/Japanese.ttf");
        std::process::exit(1);
    }
    out
}

/// Which sample screen is on show.
#[derive(Clone, Copy, PartialEq)]
enum Page {
    /// The alignment ruler — the one that decides the font question.
    Ruler,
    /// cian as it looks today: borders, dense rows, a function-key bar.
    Panes,
    /// The same listing drawn as a desktop file manager would: no box drawing,
    /// a breadcrumb, column headers, banded rows, a full-width selection.
    Finder,
    /// The same again with a blank line between rows. Half as many files fit;
    /// the question is whether that trade is the one that lowers the barrier.
    FinderRoomy,
}

impl Page {
    fn next(self) -> Self {
        match self {
            Page::Ruler => Page::Panes,
            Page::Panes => Page::Finder,
            Page::Finder => Page::FinderRoomy,
            Page::FinderRoomy => Page::Ruler,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Page::Ruler => "幅の定規",
            Page::Panes => "今のcian",
            Page::Finder => "Finder風（密）",
            Page::FinderRoomy => "Finder風（ゆったり）",
        }
    }
}

/// The palette a desktop file manager reads as: quiet greys, one accent, and
/// no lines where whitespace will do.
mod skin {
    use ratatui::style::Color;
    pub const TEXT: Color = Color::Rgb(0x1d, 0x1d, 0x1f);
    pub const DIM: Color = Color::Rgb(0x86, 0x86, 0x8b);
    pub const BG: Color = Color::Rgb(0xff, 0xff, 0xff);
    pub const BAND: Color = Color::Rgb(0xf5, 0xf5, 0xf7);
    pub const CHROME: Color = Color::Rgb(0xec, 0xec, 0xee);
    pub const RULE: Color = Color::Rgb(0xd8, 0xd8, 0xdc);
    pub const ACCENT: Color = Color::Rgb(0x0a, 0x84, 0xff);
    pub const ON_ACCENT: Color = Color::Rgb(0xff, 0xff, 0xff);
    pub const FOLDER: Color = Color::Rgb(0x54, 0xa0, 0xff);
}

/// Everything a frame needs, held apart from the terminal so the draw closure
/// does not fight the backend for a borrow of `App`.
struct Ui {
    page: Page,
    size_px: u32,
    fps: f64,
    worst_ms: f64,
    fonts: String,
    keylog: Vec<String>,
    /// Text the IME has finished with, as a real input field would keep it.
    committed: String,
    /// Text the IME is still composing. Drawn underlined, the way every editor
    /// draws 未確定文字 — if this shows かな rather than romaji, the platform is
    /// doing the conversion and cian only has to draw the result.
    preedit: String,
    /// Whether the IME says it is switched on for this window.
    ime_on: bool,
}

/// Pad `s` out to `width` display columns, the way cian's panes do.
///
/// This is the measurement the whole alignment question turns on: `unicode-width`
/// says a kanji is two columns, and the renderer has to agree.
fn pad(s: &str, width: usize) -> String {
    let have = Span::raw(s).width();
    let mut out = s.to_string();
    for _ in have..width {
        out.push(' ');
    }
    out
}

fn draw_ruler(f: &mut Frame, area: Rect, ui: &Ui) {
    // Every one of these is padded to exactly RULER_W columns. Anything that
    // measures differently than it draws shows up as a bent right border.
    let rows: &[(&str, &str)] = &[
        ("ASCII", "abcdefghij 0123456789 !@#$%^&*()"),
        ("かな", "あいうえお かきくけこ さしすせそ"),
        ("漢字", "日本語表示確認 全角文字幅検査"),
        ("混在", "src/日本語.rs → 読み込み OK"),
        ("カナ", "ﾊﾝｶｸｶﾀｶﾅ と 全角カタカナ"),
        ("記号", "「」『』【】（）〈〉…、。"),
        // A grid rather than a row of parts. Corner pieces laid side by side
        // never join — reading that as breakage is a mistake made once.
        ("罫線", "┌───┬───┐   ┏━━━┳━━━┓"),
        ("", "│ A │ 日│   ┃ A ┃ 日┃"),
        ("", "├───┼───┤   ┣━━━╋━━━┫"),
        ("", "└───┴───┘   ┗━━━┻━━━┛"),
        ("ブロック", "░▒▓█ ▁▂▃▄▅▆▇ ◢◣◤◥"),
        // Cell markers around each icon: anything drawn wider than one cell
        // eats the marker next to it.
        ("Nerd", "[\u{f07b}][\u{f15b}][\u{e7a8}][\u{e73c}][\u{f1c1}][\u{e0b0}][\u{f09b}]"),
        ("矢印", "[→][←][↑][↓][⇒][⇔][▶][◀][✔][✖][⚠][●]"),
    ];

    let mut lines: Vec<Line> = Vec::new();
    // A column ruler, so a drift of one cell is countable rather than a feeling.
    let mut tens = String::new();
    for i in 0..RULER_W {
        tens.push(if i % 10 == 0 {
            char::from_digit((i / 10) as u32, 10).unwrap_or('+')
        } else if i % 5 == 0 {
            '+'
        } else {
            '.'
        });
    }
    lines.push(Line::from(Span::styled(tens, Style::default().fg(Color::DarkGray))));

    for (label, text) in rows {
        let body = format!("{} {}", pad(label, 8), text);
        lines.push(Line::from(pad(&body, RULER_W)));
    }
    lines.push(Line::from(pad("", RULER_W)));
    // The same sentence under every attribute the renderer has to find or fake.
    for (label, style) in [
        ("  太字 bold — 同じ幅で並ぶこと", Style::default().add_modifier(Modifier::BOLD)),
        ("  斜体 italic — 同じ幅で並ぶこと", Style::default().add_modifier(Modifier::ITALIC)),
        ("  下線 underline — 同じ幅で並ぶ", Style::default().add_modifier(Modifier::UNDERLINED)),
        // Reverse video is how cian draws the cursor row, so the highlight has
        // to cover exactly the cells the text claims.
        ("  反転 cursor — 幅と一致すること", Style::default().add_modifier(Modifier::REVERSED)),
    ] {
        lines.push(Line::from(Span::styled(pad(label, RULER_W), style)));
    }
    lines.push(Line::from(Span::styled(
        pad("  カラー colour truecolor 表示", RULER_W),
        Style::default().fg(Color::Rgb(0xd3, 0x36, 0x82)).bg(Color::Rgb(0x1c, 0x1c, 0x1c)),
    )));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" 幅の定規 — {}px ", ui.size_px));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_panes(f: &mut Frame, area: Rect, ui: &Ui) {
    // A stand-in for cian's own screen: two file panes over a key-report strip.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(3), Constraint::Length(9)])
        .split(area);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);

    let files: &[(&str, &str, &str)] = &[
        ("\u{f07b}", "..", ""),
        ("\u{f07b}", "画像フォルダ", "<DIR>"),
        ("\u{f07b}", "ドキュメント", "<DIR>"),
        ("\u{e7a8}", "main.rs", "12,340"),
        ("\u{e7a8}", "日本語ファイル名.rs", "3,201"),
        ("\u{e73c}", "script.py", "890"),
        ("\u{f1c1}", "報告書_2026年度.pdf", "1,204,880"),
        ("\u{f15b}", "メモ 覚書.txt", "77"),
        ("\u{f09b}", "README.md", "4,096"),
    ];

    for (i, col) in cols.iter().enumerate() {
        let inner_w = col.width.saturating_sub(2) as usize;
        let mut lines: Vec<Line> = Vec::new();
        for (n, (icon, name, size)) in files.iter().enumerate() {
            // Right-align the size the way a file pane does — this only lands
            // if the name's measured width matches its drawn width.
            let left = format!(" {icon} {name}");
            let gap = inner_w.saturating_sub(Span::raw(&left).width() + size.len() + 1);
            let text = format!("{left}{}{size} ", " ".repeat(gap));
            let style = if n == 4 && i == 0 {
                Style::default().add_modifier(Modifier::REVERSED)
            } else if *size == "<DIR>" {
                Style::default().fg(Color::Rgb(0x26, 0x8b, 0xd2)).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(text, style)));
        }
        let title = if i == 0 { " ~/作業/cian " } else { " /Volumes/バックアップ " };
        f.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(title),
            ),
            *col,
        );
    }

    // A live input field, so the IME can be judged by what it *produces* rather
    // than by whether events merely arrive. Underlined text is 未確定 — if it
    // shows かな rather than romaji, the platform did the conversion and cian
    // would only have to draw the result.
    let mut field = vec![Span::raw("> "), Span::raw(ui.committed.as_str())];
    if ui.preedit.is_empty() {
        field.push(Span::styled(" ", Style::default().add_modifier(Modifier::REVERSED)));
    } else {
        field.push(Span::styled(
            ui.preedit.as_str(),
            Style::default().add_modifier(Modifier::UNDERLINED),
        ));
    }
    f.render_widget(
        Paragraph::new(Line::from(field)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(if ui.ime_on {
                    " 入力欄 — IME ON（下線が未確定） "
                } else {
                    " 入力欄 "
                }),
        ),
        rows[1],
    );

    // What the keyboard actually handed over. This is the other half of the
    // spike: a terminal cannot deliver some of these at all.
    let mut log: Vec<Line> =
        ui.keylog.iter().rev().take(7).map(|s| Line::from(s.as_str())).collect();
    if log.is_empty() {
        log.push(Line::from("このMacは Control と Command を入れ替えてあります。"));
        log.push(Line::from("  「Control」と刻印されたキー → Super（＝Command）"));
        log.push(Line::from("  右の「⌘」キー              → Control（＝本物のCtrl）"));
        log.push(Line::from(""));
        log.push(Line::from("入れ替えはHID層で起きるので [物理 …] も入れ替え後です。"));
        log.push(Line::from("日本語入力ONで打つと、上の入力欄に未確定文字が出ます。"));
    }
    f.render_widget(
        Paragraph::new(log).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" 受け取ったキー（新しいものが上） "),
        ),
        rows[2],
    );
}

/// The sample listing, shared by every page so the pages differ only in how
/// they are drawn.
const SAMPLE: &[(&str, &str, &str, &str)] = &[
    ("\u{f07b}", "画像フォルダ", "", "2026-08-16 20:38"),
    ("\u{f07b}", "ドキュメント", "", "2026-08-09 23:18"),
    ("\u{f07b}", "crates", "", "2026-08-16 22:17"),
    ("\u{e7a8}", "main.rs", "12,340", "2026-08-16 22:21"),
    ("\u{e7a8}", "日本語ファイル名.rs", "3,201", "2026-08-15 21:17"),
    ("\u{e73c}", "script.py", "890", "2026-07-29 01:10"),
    ("\u{f1c1}", "報告書_2026年度.pdf", "1,204,880", "2026-06-03 11:14"),
    ("\u{f15b}", "メモ 覚書.txt", "77", "2026-06-03 00:24"),
    ("\u{f09b}", "README.md", "4,096", "2026-08-15 21:17"),
];

/// Cut `s` down to `width` display columns, ending in `…` when something was
/// lost. Counts in columns, not bytes or chars, so a kanji costs two.
fn fit(s: &str, width: usize) -> String {
    if Span::raw(s).width() <= width {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = Span::raw(c.to_string()).width();
        if w + cw > width.saturating_sub(1) {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

/// The columns that fit in `width`, in the order a file manager drops them.
///
/// This is the part the mock-up exists to expose: two panes side by side leave
/// each one about forty columns, and name + size + date needs thirty-six of
/// them before a single letter of the name is drawn. Something has to go, and
/// every desktop file manager answers the same way — drop the columns from the
/// right and let the name have the room.
fn finder_columns(width: usize) -> (bool, bool) {
    (width >= 34, width >= 52) // (size, date)
}

/// One row of the desktop-style listing, padded to the full width so its
/// background reaches both edges. Nothing here is a border; the shape comes
/// from colour alone.
fn finder_row(
    (icon, name, size, date): &(&str, &str, &str, &str),
    width: usize,
    selected: bool,
    banded: bool,
) -> Line<'static> {
    let is_dir = size.is_empty();
    let (with_size, with_date) = finder_columns(width);
    let size_w = if with_size { 11 } else { 0 };
    let date_w = if with_date { 18 } else { 0 };
    let name_w = width.saturating_sub(7 + size_w + date_w);

    let mut body = format!("   {icon}  {}", pad(&fit(name, name_w), name_w));
    if with_size {
        let shown = if is_dir { "—" } else { size };
        // Right-aligned in its column, the way a number wants to be read.
        let w = Span::raw(shown).width();
        body.push_str(&format!("{}{shown}  ", " ".repeat(size_w.saturating_sub(w + 2))));
    }
    if with_date {
        body.push_str(&format!("{date}  "));
    }
    let body = pad(&body, width);

    let (fg, bg) = if selected {
        (skin::ON_ACCENT, skin::ACCENT)
    } else if banded {
        (skin::TEXT, skin::BAND)
    } else {
        (skin::TEXT, skin::BG)
    };
    let mut style = Style::default().fg(fg).bg(bg);
    if is_dir && !selected {
        style = style.fg(skin::FOLDER).add_modifier(Modifier::BOLD);
    }
    Line::from(Span::styled(body, style))
}

/// A desktop file manager's chrome: a breadcrumb above, column headings below
/// it, then the listing. No box drawing anywhere — the panes are told apart by
/// a gutter and a single hairline, which is what the real ones do.
fn draw_finder(f: &mut Frame, area: Rect, roomy: bool) {
    // Paint the whole surface first so the bands and the gutter sit on white
    // rather than on the terminal's idea of a background.
    f.render_widget(
        Paragraph::new("").style(Style::default().bg(skin::BG)),
        area,
    );

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    // Toolbar and breadcrumb.
    let crumb = Line::from(vec![
        Span::styled("  ‹  ›   ", Style::default().fg(skin::DIM).bg(skin::CHROME)),
        Span::styled("\u{f07b}  ", Style::default().fg(skin::FOLDER).bg(skin::CHROME)),
        Span::styled("dateshimakaya", Style::default().fg(skin::DIM).bg(skin::CHROME)),
        Span::styled("  ›  ", Style::default().fg(skin::RULE).bg(skin::CHROME)),
        Span::styled("workspace", Style::default().fg(skin::DIM).bg(skin::CHROME)),
        Span::styled("  ›  ", Style::default().fg(skin::RULE).bg(skin::CHROME)),
        Span::styled("cian", Style::default().fg(skin::TEXT).bg(skin::CHROME).add_modifier(Modifier::BOLD)),
        Span::styled(
            pad("", area.width as usize),
            Style::default().bg(skin::CHROME),
        ),
    ]);
    f.render_widget(Paragraph::new(crumb), rows[0]);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Length(1), Constraint::Min(10)])
        .split(rows[2]);
    let heads = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Length(1), Constraint::Min(10)])
        .split(rows[1]);

    for (side, (head, pane)) in [(0usize, (heads[0], panes[0])), (1, (heads[2], panes[2]))] {
        let w = pane.width as usize;
        // Column headings, as a strip of colour rather than a ruled row —
        // named by the same rule that lays the rows out, so they cannot drift
        // apart.
        let (with_size, with_date) = finder_columns(w);
        let size_w = if with_size { 11 } else { 0 };
        let date_w = if with_date { 18 } else { 0 };
        let name_w = w.saturating_sub(7 + size_w + date_w);
        let mut cols = format!("   \u{2004} 名前 ▲{}", " ".repeat(name_w.saturating_sub(6)));
        if with_size {
            cols.push_str(&format!("{}サイズ  ", " ".repeat(size_w.saturating_sub(8))));
        }
        if with_date {
            cols.push_str("更新日時");
        }
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                pad(&cols, w),
                Style::default().fg(skin::DIM).bg(skin::BG),
            ))),
            head,
        );

        let mut lines: Vec<Line> = Vec::new();
        for (n, entry) in SAMPLE.iter().enumerate() {
            let selected = side == 0 && n == 4;
            lines.push(finder_row(entry, w, selected, !roomy && n % 2 == 1));
            if roomy {
                // The airy variant: a blank line of the row's own colour, so a
                // selected row reads as one tall block rather than two thin
                // ones with a white seam.
                let bg = if selected { skin::ACCENT } else { skin::BG };
                lines.push(Line::from(Span::styled(pad("", w), Style::default().bg(bg))));
            }
        }
        f.render_widget(Paragraph::new(lines).style(Style::default().bg(skin::BG)), pane);
    }

    // The hairline between the panes — one column, no corners, no joins.
    let gutter: Vec<Line> = (0..panes[1].height)
        .map(|_| Line::from(Span::styled("│", Style::default().fg(skin::RULE).bg(skin::BG))))
        .collect();
    f.render_widget(Paragraph::new(gutter), panes[1]);

    let status = format!(
        "   9 項目、うち 1 個を選択      42.3 GB / 233.5 GB 空き{}",
        " ".repeat(4),
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            pad(&status, area.width as usize),
            Style::default().fg(skin::DIM).bg(skin::CHROME),
        ))),
        rows[3],
    );
}

fn draw(f: &mut Frame, ui: &Ui) {
    let area = f.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    match ui.page {
        Page::Ruler => draw_ruler(f, rows[0], ui),
        Page::Panes => draw_panes(f, rows[0], ui),
        Page::Finder => draw_finder(f, rows[0], false),
        Page::FinderRoomy => draw_finder(f, rows[0], true),
    }

    let status = format!(
        " {} · {}x{} cells · {}px · {:.0} fps · t で切替 · +/- 拡縮 · Esc 終了 ",
        ui.page.label(),
        area.width,
        area.height,
        ui.size_px,
        ui.fps,
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            pad(&status, area.width as usize),
            Style::default().fg(Color::Rgb(0xfd, 0xf6, 0xe3)).bg(Color::Rgb(0x07, 0x36, 0x42)),
        ))),
        rows[1],
    );
}

struct App {
    window: Option<Arc<Window>>,
    terminal: Option<Terminal<WgpuBackend<'static, 'static>>>,
    faces: Vec<(String, Font<'static>)>,
    ui: Ui,
    mods: ModifiersState,
    frames: u64,
    last_report: Instant,
}

impl App {
    fn new() -> Self {
        let faces = load_fonts();
        let names: Vec<&str> = faces.iter().map(|(n, _)| n.as_str()).collect();
        let fonts = names.join(" + ");
        Self {
            window: None,
            terminal: None,
            faces,
            ui: Ui {
                page: match std::env::var("SPIKE_PAGE").as_deref() {
                    Ok("panes") => Page::Panes,
                    Ok("finder") => Page::Finder,
                    Ok("roomy") => Page::FinderRoomy,
                    _ => Page::Ruler,
                },
                size_px: std::env::var("SPIKE_SIZE")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(22),
                fps: 0.0,
                worst_ms: 0.0,
                fonts,
                keylog: Vec::new(),
                committed: String::new(),
                preedit: String::new(),
                ime_on: false,
            },
            mods: ModifiersState::empty(),
            frames: 0,
            last_report: Instant::now(),
        }
    }

    /// Add a line to the on-screen report, keeping the list bounded.
    fn log(&mut self, line: String) {
        self.ui.keylog.push(line);
        if self.ui.keylog.len() > 32 {
            self.ui.keylog.remove(0);
        }
    }

    /// The font collection at the current size: the first face is the
    /// last-resort one, and every face (including it) joins the fallback chain.
    fn build_fonts(&self) -> Fonts<'static> {
        let mut iter = self.faces.iter().map(|(_, f)| f.clone());
        let first = iter.next().expect("checked in load_fonts");
        let mut fonts = Fonts::new(first.clone(), self.ui.size_px);
        fonts.add_fonts(std::iter::once(first).chain(iter));
        fonts
    }

    fn step_size(&mut self, by: i32) {
        let want = (self.ui.size_px as i32 + by).clamp(SIZE_MIN as i32, SIZE_MAX as i32) as u32;
        if want == self.ui.size_px {
            return;
        }
        self.ui.size_px = want;
        let fonts = self.build_fonts();
        if let Some(t) = self.terminal.as_mut() {
            t.backend_mut().update_fonts(fonts);
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // `SPIKE_PIN=1` nails the window to a known corner and keeps it on top,
        // which is how a screenshot finds it. Off by default: a window that
        // will not go behind anything is no way to try a file manager.
        let mut attrs = WindowAttributes::default()
            .with_title("cian gui spike")
            .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 780.0));
        if std::env::var_os("SPIKE_PIN").is_some() {
            attrs = attrs
                .with_position(winit::dpi::LogicalPosition::new(40.0, 60.0))
                .with_window_level(winit::window::WindowLevel::AlwaysOnTop);
        }
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));

        // Without this, winit sends no `Ime` events at all — the default is
        // off, and an IME-off window looks exactly like a platform that cannot
        // do IME. cian's whole reason to want a window of its own includes
        // seeing preedit text, so it has to be asked for.
        window.set_ime_allowed(true);
        // Where the candidate list should appear. A real cian would move this
        // to the cell the caret is in; a fixed spot is enough to prove the
        // events arrive.
        window.set_ime_cursor_area(
            winit::dpi::LogicalPosition::new(40.0, 600.0),
            winit::dpi::LogicalSize::new(400.0, 40.0),
        );
        let size = window.inner_size();

        let mut faces = self.faces.iter().map(|(_, f)| f.clone());
        let first = faces.next().expect("checked in load_fonts");
        let backend = pollster::block_on(
            Builder::from_font(first.clone())
                .with_font_size_px(self.ui.size_px)
                .with_fonts(std::iter::once(first).chain(faces))
                .with_width_and_height(Dimensions {
                    width: NonZeroU32::new(size.width.max(1)).unwrap(),
                    height: NonZeroU32::new(size.height.max(1)).unwrap(),
                })
                .build_with_target(window.clone()),
        )
        .expect("build wgpu backend");

        self.terminal = Some(Terminal::new(backend).expect("terminal"));
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
                return;
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::Resized(size) => {
                if let Some(t) = self.terminal.as_mut() {
                    t.backend_mut().resize(size.width.max(1), size.height.max(1));
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                // Report the key the way a translation layer would have to read
                // it: logical key plus the modifier state, plus whatever text
                // the platform decided it produced.
                let mut parts = Vec::new();
                for (on, name) in [
                    (self.mods.control_key(), "Ctrl"),
                    (self.mods.alt_key(), "Alt"),
                    (self.mods.shift_key(), "Shift"),
                    (self.mods.super_key(), "Super"),
                ] {
                    if on {
                        parts.push(name.to_string());
                    }
                }
                parts.push(match &event.logical_key {
                    Key::Character(c) => format!("{c:?}"),
                    Key::Named(n) => format!("{n:?}"),
                    other => format!("{other:?}"),
                });
                // "Physical" here is winit's word, not the keyboard's. macOS
                // applies the System Settings modifier swap down at the HID
                // level, below everything winit can see, so *both* fields are
                // already post-swap: the key labelled Control reports as
                // SuperLeft. Which is itself the finding — a program cannot see
                // the swap, only live with it.
                let phys = match event.physical_key {
                    winit::keyboard::PhysicalKey::Code(c) => format!("{c:?}"),
                    winit::keyboard::PhysicalKey::Unidentified(_) => "?".to_string(),
                };
                let text = event.text.as_ref().map(|t| format!(" text={t:?}")).unwrap_or_default();
                self.log(format!("{}{}   [物理 {}]", parts.join("+"), text, phys));

                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        event_loop.exit();
                        return;
                    }
                    Key::Character(c) if matches!(c.as_str(), "+" | "=" | ";") => self.step_size(2),
                    Key::Character(c) if c.as_str() == "-" => self.step_size(-2),
                    Key::Character(c) if c.as_str() == "t" => self.ui.page = self.ui.page.next(),
                    _ => {}
                }
            }
            // The one thing the terminal build can never see. Driven into a
            // real input field rather than only logged, because "the events
            // arrive" and "Japanese can be typed" are different claims.
            WindowEvent::Ime(ime) => {
                use winit::event::Ime;
                match &ime {
                    Ime::Enabled => self.ui.ime_on = true,
                    Ime::Disabled => {
                        self.ui.ime_on = false;
                        self.ui.preedit.clear();
                    }
                    Ime::Preedit(text, _) => self.ui.preedit = text.clone(),
                    Ime::Commit(text) => {
                        self.ui.committed.push_str(text);
                        self.ui.preedit.clear();
                    }
                }
                self.log(format!("IME {ime:?}"));
            }
            WindowEvent::RedrawRequested => {
                let start = Instant::now();
                if let Some(t) = self.terminal.as_mut() {
                    // Clear first so every frame is a full repaint — a harder
                    // job than cian's, which only redraws on change.
                    let _ = t.clear();
                    let ui = &self.ui;
                    let _ = t.draw(|f| draw(f, ui));
                }
                let ms = start.elapsed().as_secs_f64() * 1000.0;
                self.ui.worst_ms = self.ui.worst_ms.max(ms);
                self.frames += 1;
                let since = self.last_report.elapsed().as_secs_f64();
                if since >= 0.5 {
                    self.ui.fps = self.frames as f64 / since;
                    self.frames = 0;
                    self.last_report = Instant::now();
                    self.ui.worst_ms = ms;
                }
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
                return;
            }
            _ => {}
        }
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new();
    event_loop.run_app(&mut app).expect("run");
}
