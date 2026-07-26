//! The rendering layer: every `draw_*` function plus the colour/geometry
//! helpers they use. Split out of lib.rs. These take `&App` / `&mut App` and
//! never mutate domain state beyond stashing layout rects for the mouse code.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::Frame;
use tui_term::widget::PseudoTerminal;

use super::*;

/// Normal three-surface layout: left/right file panes on top, shell below.
fn draw_split(f: &mut Frame, main_area: Rect, app: &mut App, ov: AnimOverride) {
    app.ensure_git();
    let main_pct = ov.ratio_for(DividerTarget::Main, app.main_pct);
    let main_split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(main_pct), Constraint::Percentage(100 - main_pct)])
        .split(main_area);
    let panes_area = main_split[0];
    let shell_area = main_split[1];

    let panes_pct = ov.ratio_for(DividerTarget::Panes, app.panes_pct);
    let panes_split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(panes_pct), Constraint::Percentage(100 - panes_pct)])
        .split(panes_area);

    app.layout_rects = LayoutRects {
        left: panes_split[0],
        right: panes_split[1],
        shell: shell_area,
    };

    let mut leaves = Vec::new();
    let mut tab_rects = Vec::new();
    let mut dividers = vec![
        Divider {
            zone: seam_zone(Direction::Vertical, panes_area, shell_area),
            parent: main_area,
            dir: Direction::Vertical,
            target: DividerTarget::Main,
        },
        Divider {
            zone: seam_zone(Direction::Horizontal, panes_split[0], panes_split[1]),
            parent: panes_area,
            dir: Direction::Horizontal,
            target: DividerTarget::Panes,
        },
    ];

    let visual_for_left = if app.focused == FocusedPane::Left { app.visual_anchor } else { None };
    let visual_for_right = if app.focused == FocusedPane::Right { app.visual_anchor } else { None };

    let (bg_l, bg_r) = (app.pane_bg[0], app.pane_bg[1]);
    let (fl_l, fl_r) = (app.flash_level(FocusedPane::Left), app.flash_level(FocusedPane::Right));
    draw_file_pane(f, panes_split[0], &app.left, app.focused == FocusedPane::Left, visual_for_left, app.mode, bg_l, fl_l, FocusedPane::Left, &mut tab_rects, app.git_for(FocusedPane::Left));
    draw_file_pane(f, panes_split[1], &app.right, app.focused == FocusedPane::Right, visual_for_right, app.mode, bg_r, fl_r, FocusedPane::Right, &mut tab_rects, app.git_for(FocusedPane::Right));
    // draw_shell sizes each pane's PTY to its computed sub-rect.
    let log_border = recording_pulse(app.started.elapsed());
    draw_shell(f, shell_area, &mut app.shell, app.focused == FocusedPane::Shell, &mut dividers, &mut leaves, ov, &mut tab_rects, log_border);
    app.dividers = dividers;
    app.shell_leaves = leaves;
    app.tab_rects = tab_rects;
}

/// The focused surface drawn at an arbitrary rect, used as the floating layer
/// of a zoom transition. Deliberately does not touch `app.layout_rects`: the
/// backdrop already set those, and hit-testing should follow the resting
/// layout rather than a rect that is still moving.
fn draw_zoom_overlay(f: &mut Frame, rect: Rect, app: &mut App, ov: AnimOverride) {
    let mut sink = Vec::new();
    match app.focused {
        FocusedPane::Left => {
            let (bg, fl) = (app.pane_bg[0], app.flash_level(FocusedPane::Left));
            let va = app.visual_anchor;
            draw_file_pane(f, rect, &app.left, true, va, app.mode, bg, fl, FocusedPane::Left, &mut Vec::new(), app.git_for(FocusedPane::Left));
        }
        FocusedPane::Right => {
            let (bg, fl) = (app.pane_bg[1], app.flash_level(FocusedPane::Right));
            let va = app.visual_anchor;
            draw_file_pane(f, rect, &app.right, true, va, app.mode, bg, fl, FocusedPane::Right, &mut Vec::new(), app.git_for(FocusedPane::Right));
        }
        FocusedPane::Shell => {
            let log_border = recording_pulse(app.started.elapsed());
            draw_shell(f, rect, &mut app.shell, true, &mut sink, &mut Vec::new(), ov, &mut Vec::new(), log_border);
        }
    }
}

/// Float the active shell pane's terminal at `rect`, for the pane-zoom
/// transition. Just the one pane's screen, bordered, so it reads as that pane
/// growing rather than the whole panel.
fn draw_pane_zoom_overlay(f: &mut Frame, rect: Rect, app: &mut App) {
    if rect.width < 2 || rect.height < 2 {
        return;
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD));
    let inner = rect.inner(Margin { vertical: 1, horizontal: 1 });
    f.render_widget(block, rect);

    let tab = app.shell.active;
    let leaf = app.shell.tabs.get(tab).map(|t| t.active);
    if let Some(leaf) = leaf {
        if let Some(Node::Leaf { session, bg }) =
            app.shell.tabs.get(tab).and_then(|t| t.nodes.get(leaf)).and_then(|n| n.as_ref())
        {
            if let Ok(parser) = session.parser().lock() {
                f.render_widget(PseudoTerminal::new(parser.screen()), inner);
            }
            if let Some(c) = bg {
                tint_default_cells(f, inner, *c);
            }
        }
    }
    if let Some(base) = theme().base_bg {
        tint_shell_base(f, inner, base, theme().file.plain);
    }
}

/// Zoomed layout: only the focused surface, filling the available area.
fn draw_zoomed(f: &mut Frame, area: Rect, app: &mut App, ov: AnimOverride) {
    let mut rects = LayoutRects::default();
    // Only the shell's internal splits are draggable while zoomed; the
    // main/panes borders are not on screen.
    let mut dividers = Vec::new();
    let mut leaves = Vec::new();
    let mut tab_rects = Vec::new();
    match app.focused {
        FocusedPane::Left => {
            rects.left = area;
            app.layout_rects = rects;
            let va = app.visual_anchor;
            let (bg, fl) = (app.pane_bg[0], app.flash_level(FocusedPane::Left));
            draw_file_pane(f, area, &app.left, true, va, app.mode, bg, fl, FocusedPane::Left, &mut tab_rects, app.git_for(FocusedPane::Left));
        }
        FocusedPane::Right => {
            rects.right = area;
            app.layout_rects = rects;
            let va = app.visual_anchor;
            let (bg, fl) = (app.pane_bg[1], app.flash_level(FocusedPane::Right));
            draw_file_pane(f, area, &app.right, true, va, app.mode, bg, fl, FocusedPane::Right, &mut tab_rects, app.git_for(FocusedPane::Right));
        }
        FocusedPane::Shell => {
            rects.shell = area;
            app.layout_rects = rects;
            let log_border = recording_pulse(app.started.elapsed());
            draw_shell(f, area, &mut app.shell, true, &mut dividers, &mut leaves, ov, &mut tab_rects, log_border);
        }
    }
    app.dividers = dividers;
    app.shell_leaves = leaves;
    app.tab_rects = tab_rects;
}

pub(crate) fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    // A light theme paints the whole surface so gaps, the shell panel and the
    // bottom bars share one background rather than showing the terminal's own.
    if let Some(bg) = theme().base_bg {
        f.render_widget(Block::default().style(Style::default().bg(bg)), area);
    }
    // Command and filter modes add a prompt line above the status bar; the key
    // hints take another. A very short window drops the hints rather than the
    // listing.
    let prompt_line = matches!(app.mode, Mode::Command | Mode::Filter);
    let hint_line = app.show_key_hints && area.height >= 12;
    let bottom_lines = 1 + u16::from(prompt_line) + u16::from(hint_line);
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(bottom_lines)])
        .split(area);
    let main_area = vertical[0];
    let bottom_area = vertical[1];

    let ov = app.anim_override();
    // A zoom transition draws the normal layout as a backdrop and floats the
    // zooming surface above it, so it visibly grows out of (or shrinks back
    // into) its own pane.
    if let Some(Anim { kind: AnimKind::Zoom { from, to }, .. }) = app.anim {
        let t = app.anim.map(|a| a.progress()).unwrap_or(1.0);
        draw_split(f, main_area, app, ov);
        let rect = lerp_rect(from, to, t);
        f.render_widget(Clear, rect);
        draw_zoom_overlay(f, rect, app, ov);
    } else if let Some(Anim { kind: AnimKind::PaneZoom { from, to }, .. }) = app.anim {
        // Backdrop keeps the shell's splits (ov.show_splits); the active pane
        // floats above them, growing out of or shrinking into its slot.
        let t = app.anim.map(|a| a.progress()).unwrap_or(1.0);
        draw_split(f, main_area, app, ov);
        let rect = lerp_rect(from, to, t);
        f.render_widget(Clear, rect);
        draw_pane_zoom_overlay(f, rect, app);
    } else if app.zoomed {
        draw_zoomed(f, main_area, app, ov);
    } else {
        draw_split(f, main_area, app, ov);
    }

    // Reverse the cells of a shell text selection, over whatever was drawn.
    if let Some(sel) = app.shell_sel {
        highlight_shell_selection(f, &sel);
    }

    // Stack the bottom rows: [prompt] [hints] [status]. Each is claimed only if
    // the strip actually has room — a window can be short enough that Layout
    // hands back fewer rows than were asked for, and writing past the buffer
    // panics.
    let end = bottom_area.y.saturating_add(bottom_area.height);
    let mut row = bottom_area.y;
    let claim = |row: &mut u16| -> Option<Rect> {
        if *row >= end {
            return None;
        }
        let r = Rect::new(bottom_area.x, *row, bottom_area.width, 1);
        *row += 1;
        Some(r)
    };

    // Note: `claim` must only be called for rows that are actually drawn, so
    // each branch guards its flag *before* claiming.
    if prompt_line {
        if let Some(cmd_area) = claim(&mut row) {
            if app.mode == Mode::Filter {
                let matched = app.active_pane().map(|p| p.entries.len()).unwrap_or(0);
                let total = app.active_pane().map(|p| p.all_entries.len()).unwrap_or(0);
                draw_prompt_line(
                    f,
                    cmd_area,
                    &format!("filter /{}_", app.filter_buffer),
                    &format!("{}/{} match  Enter=keep  Esc=clear", matched, total),
                );
            } else {
                draw_command_line(f, cmd_area, &app.command_buffer);
            }
        }
    }
    if hint_line {
        if let Some(r) = claim(&mut row) {
            draw_key_hints(f, r, app);
        }
    }
    if let Some(r) = claim(&mut row) {
        draw_status(f, r, app);
    }

    if app.op_job.is_some() {
        draw_op_progress(f, area, app);
    }
    // The directory comparison shows the same bar while it runs.
    if let Some(job) = &app.diff_job {
        draw_progress_bar(f, area, job.label, &job.latest, job.started, app.lang);
    }
    // The chat has its own renderer so it can stash the transcript geometry on
    // `app` for mouse selection.
    if matches!(app.popup, Popup::AiChat { .. }) {
        draw_ai_chat(f, area, app);
        return;
    }
    // The image preview decodes to fit its box and caches by size, so it takes
    // `&mut app` too.
    if matches!(app.popup, Popup::ImageView { .. }) {
        draw_image(f, area, app);
        return;
    }
    if matches!(app.popup, Popup::CommitMessage { .. }) {
        draw_commit_message(f, area, app);
        return;
    }
    if matches!(app.popup, Popup::JunkReview { .. }) {
        draw_junk_review(f, area, app);
        return;
    }
    if matches!(app.popup, Popup::DupeReview { .. }) {
        draw_dupe_review(f, area, app);
        return;
    }
    if matches!(app.popup, Popup::StructureReview { .. }) {
        draw_structure_review(f, area, app);
        return;
    }
    if matches!(app.popup, Popup::RenameReview { .. }) {
        draw_rename_review(f, area, app);
        return;
    }
    if !matches!(app.popup, Popup::None) {
        // Remember where the context menu landed so a click can hit its rows.
        if let Popup::ContextMenu { items, at, .. } = &app.popup {
            app.menu_rect = context_menu_rect(items, *at, area, app.lang);
        }
        // And the viewer's text body, so a drag maps to a line — plus the
        // line-number gutter width, so it maps to a char column too.
        if let Popup::Viewer { view, preview, .. } = &app.popup {
            app.viewer_rect = viewer_body_rect(area);
            app.viewer_gutter = if !*preview && view.kind == cian_core::viewer::ViewKind::Text {
                (format!("{}", view.lines.len()).len().max(3) + 1) as u16
            } else {
                0
            };
        }
        let find_state = app
            .find_job
            .as_ref()
            .map(|j| (j.query.as_str(), j.root_label.as_str(), j.done, j.mode));
        let dests = app.dest_choices();
        let lang = app.lang;
        app.popup_zones.clear();
        draw_popup(
            f,
            area,
            &mut app.popup,
            &app.config.ssh_hosts,
            find_state,
            &dests,
            &mut app.popup_zones,
            lang,
        );
    } else {
        app.popup_zones.clear();
    }
}

/// The viewer's text body rect, mirroring its renderer's geometry so a mouse
/// click maps to the right line.
fn viewer_body_rect(area: Rect) -> Rect {
    let w = area.width.saturating_sub(4);
    let h = area.height.saturating_sub(2);
    let rect = centered_rect(w, h, area);
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    let body_h = inner.height.saturating_sub(1);
    Rect::new(inner.x, inner.y, inner.width, body_h)
}

/// The rect the context menu occupies, from its anchor and item count. Shared
/// by the renderer and the mouse handler so a click lands where the row is
/// drawn.
fn context_menu_rect(items: &[MenuItem], at: (u16, u16), area: Rect, lang: Lang) -> Rect {
    let w = items.iter().map(|i| width(i.label(lang))).max().unwrap_or(10) as u16 + 4;
    let h = items.len() as u16 + 2;
    let x = at.0.min(area.width.saturating_sub(w));
    let y = at.1.min(area.height.saturating_sub(h));
    Rect::new(x, y, w.min(area.width), h.min(area.height))
}

/// Build a tab strip. Active tab uses full path; inactive tabs use just the
/// directory name. If the labels overflow `max_width`, the rest collapse into
/// a `+N` marker so the active tab stays visible.
fn tabs_title<'a>(
    tabs: &'a PaneTabs,
    focused: bool,
    focus_bg: Color,
    max_width: u16,
    // Filled with (tab index, column offset from the title's start, width) for
    // each visible tab, so a click can be mapped back to a tab.
    offsets: &mut Vec<(usize, u16, u16)>,
) -> Line<'a> {
    fn label_for(i: usize, tab: &Pane, is_active: bool) -> String {
        let main = if is_active {
            tab.cwd.display().to_string()
        } else {
            tab.cwd
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| tab.cwd.display().to_string())
        };
        format!(" {} {} ", i + 1, main)
    }
    let width_of = |s: &str| s.chars().count() as u16;

    // First, lay out tabs starting from the active one outward so it never gets cut.
    let active = tabs.active.min(tabs.tabs.len().saturating_sub(1));
    let total = tabs.tabs.len();
    let mut shown: Vec<usize> = vec![active];
    let mut used: u16 = width_of(&label_for(active, &tabs.tabs[active], true));
    let sep_w: u16 = 1;
    let reserve: u16 = 5; // for " +N "

    let (mut left, mut right) = (active, active);
    loop {
        let try_right = right + 1 < total;
        let try_left = left > 0;
        if !try_right && !try_left { break; }
        // prefer expanding right first (chronological order)
        if try_right {
            let i = right + 1;
            let w = width_of(&label_for(i, &tabs.tabs[i], false)) + sep_w;
            let need_reserve = if i + 1 < total || left > 0 { reserve } else { 0 };
            if used + w + need_reserve <= max_width {
                shown.push(i);
                used += w;
                right = i;
                continue;
            }
        }
        if try_left {
            let i = left - 1;
            let w = width_of(&label_for(i, &tabs.tabs[i], false)) + sep_w;
            let need_reserve = if i > 0 || right + 1 < total { reserve } else { 0 };
            if used + w + need_reserve <= max_width {
                shown.insert(0, i);
                used += w;
                left = i;
                continue;
            }
        }
        break;
    }
    let hidden_left = left;
    let hidden_right = total.saturating_sub(right + 1);

    let mut spans: Vec<Span<'a>> = Vec::new();
    // Track the running column offset so each tab's on-screen span is known.
    let mut col: u16 = 1; // the leading space below
    spans.push(Span::raw(" "));
    if hidden_left > 0 {
        let s = format!("+{} ", hidden_left);
        col += s.chars().count() as u16;
        spans.push(Span::styled(s, Style::default().fg(Color::DarkGray)));
    }
    for (pos, &i) in shown.iter().enumerate() {
        let is_active = i == active;
        let style = if is_active {
            if focused {
                Style::default().fg(Color::Black).bg(focus_bg).add_modifier(Modifier::BOLD)
            } else {
                // Active but unfocused: an accent-tinted bar so it stays legible
                // whatever the pane background is (DarkGray vanished on some).
                Style::default().fg(Color::Black).bg(theme().border).add_modifier(Modifier::BOLD)
            }
        } else {
            // Inactive tabs: a readable mid grey from the theme, not DarkGray,
            // which was the same tone as some backgrounds.
            Style::default().fg(theme().dim).add_modifier(Modifier::BOLD)
        };
        let label = label_for(i, &tabs.tabs[i], is_active);
        let w = label.chars().count() as u16;
        offsets.push((i, col, w));
        col += w;
        spans.push(Span::styled(label, style));
        if pos + 1 < shown.len() {
            spans.push(Span::styled("│", Style::default().fg(theme().dim)));
            col += 1;
        }
    }
    if hidden_right > 0 {
        spans.push(Span::styled(
            format!(" +{}", hidden_right),
            Style::default().fg(Color::DarkGray),
        ));
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// Pick a Nerd Font glyph based on the entry name/extension.
pub(crate) fn icon_for(entry: &cian_core::Entry) -> &'static str {
    // The synthetic `..` row gets an up-level arrow so it reads as navigation,
    // not as a folder that happens to be called "..".
    if entry.is_parent {
        return "\u{f062}"; // arrow-up
    }
    if entry.is_dir {
        return match entry.name.as_str() {
            ".git" => "\u{e702}",
            ".github" => "\u{f408}",
            "node_modules" => "\u{e5fa}",
            "src" => "\u{f121}",
            "tests" | "test" => "\u{f0c3}",
            "docs" | "doc" => "\u{f02d}",
            "target" | "build" | "dist" | "out" => "\u{f1c6}",
            ".vscode" | ".idea" => "\u{e7c5}",
            _ => "\u{f07b}",
        };
    }
    let lower = entry.name.to_lowercase();
    match lower.as_str() {
        "cargo.toml" | "cargo.lock" => return "\u{e7a8}",
        "dockerfile" | ".dockerignore" => return "\u{f308}",
        "makefile" => return "\u{e779}",
        "readme.md" | "readme" => return "\u{f48a}",
        "license" | "license.md" => return "\u{f02d}",
        ".gitignore" | ".gitattributes" | ".gitmodules" => return "\u{f1d3}",
        ".env" | ".env.local" => return "\u{f462}",
        "package.json" | "package-lock.json" | "yarn.lock" => return "\u{e60b}",
        _ => {}
    }
    let ext = std::path::Path::new(&entry.name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "rs" => "\u{e7a8}",
        "py" => "\u{e73c}",
        "js" | "mjs" | "cjs" => "\u{f2ee}",
        "ts" | "tsx" | "jsx" => "\u{e628}",
        "go" => "\u{e627}",
        "c" | "h" => "\u{e61e}",
        "cpp" | "cc" | "cxx" | "hpp" => "\u{e61d}",
        "java" => "\u{e738}",
        "rb" => "\u{e21e}",
        "php" => "\u{e608}",
        "lua" => "\u{e620}",
        "swift" => "\u{e755}",
        "kt" | "kts" => "\u{e634}",
        "md" | "markdown" => "\u{f48a}",
        "json" | "jsonc" => "\u{e60b}",
        "yaml" | "yml" => "\u{f481}",
        "toml" | "ini" | "conf" | "cfg" => "\u{f013}",
        "xml" => "\u{f72d}",
        "html" | "htm" => "\u{f13b}",
        "css" | "scss" | "sass" | "less" => "\u{f13c}",
        "vue" => "\u{fd42}",
        "svelte" => "\u{e697}",
        "sh" | "bash" | "zsh" | "fish" => "\u{f489}",
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" | "tif" | "tiff" => "\u{f1c5}",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => "\u{f001}",
        "mp4" | "mov" | "mkv" | "avi" | "webm" | "wmv" => "\u{f03d}",
        "pdf" => "\u{f1c1}",
        "zip" | "tar" | "gz" | "7z" | "rar" | "bz2" | "xz" => "\u{f1c6}",
        "txt" | "log" => "\u{f0f6}",
        "exe" | "dll" | "so" | "dylib" => "\u{f013}",
        _ => "\u{f15c}",
    }
}

/// Broad kinds of file, used to color the listing.
///
/// Deliberately coarse: the point is that a glance separates "code" from
/// "archive" from "image", not that every extension gets its own hue. Too many
/// colors read as noise rather than structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileKind {
    Directory,
    Code,
    Config,
    Document,
    Image,
    Media,
    Archive,
    Executable,
    /// Dotfiles and other things that are usually background noise.
    Muted,
    Plain,
}

impl FileKind {
    fn color(self) -> Color {
        // From the active theme's palette, so a light theme recolors the whole
        // set at once rather than fighting these fixed values.
        let p = &theme().file;
        match self {
            FileKind::Directory => p.directory,
            FileKind::Code => p.code,
            FileKind::Config => p.config,
            FileKind::Document => p.document,
            FileKind::Image => p.image,
            FileKind::Media => p.media,
            FileKind::Archive => p.archive,
            FileKind::Executable => p.executable,
            FileKind::Muted => p.muted,
            FileKind::Plain => p.plain,
        }
    }

    fn bold(self) -> bool {
        matches!(self, FileKind::Directory | FileKind::Executable)
    }
}

/// Classify an entry for coloring. Mirrors the categories [`icon_for`] draws
/// from, so a file's icon and its color always agree.
fn kind_for(entry: &cian_core::Entry) -> FileKind {
    if entry.is_dir {
        return FileKind::Directory;
    }
    // Dotfiles recede: they are rarely the thing being looked for.
    if entry.name.starts_with('.') {
        return FileKind::Muted;
    }
    let ext = std::path::Path::new(&entry.name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "rs" | "py" | "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "go" | "c" | "h" | "cpp"
        | "cc" | "cxx" | "hpp" | "java" | "rb" | "php" | "lua" | "swift" | "kt" | "kts"
        | "vue" | "svelte" | "html" | "htm" | "css" | "scss" | "sass" | "less" => FileKind::Code,
        "toml" | "ini" | "conf" | "cfg" | "yaml" | "yml" | "json" | "jsonc" | "xml" | "env" => {
            FileKind::Config
        }
        "md" | "markdown" | "txt" | "log" | "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt"
        | "pptx" | "rtf" | "csv" | "tsv" => FileKind::Document,
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" | "tif" | "tiff" => {
            FileKind::Image
        }
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "mp4" | "mov" | "mkv" | "avi" | "webm"
        | "wmv" => FileKind::Media,
        "zip" | "tar" | "gz" | "7z" | "rar" | "bz2" | "xz" | "zst" | "tgz" => FileKind::Archive,
        "exe" | "msi" | "bat" | "cmd" | "ps1" | "sh" | "bash" | "zsh" | "fish" | "app"
        | "dll" | "so" | "dylib" => FileKind::Executable,
        _ => FileKind::Plain,
    }
}

fn shell_tabs_title<'a>(
    tabs: &'a ShellPane,
    focused: bool,
    offsets: &mut Vec<(usize, u16, u16)>,
) -> Line<'a> {
    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut col: u16 = 1; // the leading space below
    spans.push(Span::raw(" "));
    for i in 0..tabs.count().max(1) {
        let label = format!(" shell {} ", i + 1);
        let style = if i == tabs.active {
            if focused {
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).bg(Color::DarkGray)
            }
        } else {
            // Readable medium grey for inactive tabs (DarkGray was too dim).
            Style::default().fg(Color::Gray)
        };
        let w = label.chars().count() as u16;
        offsets.push((i, col, w));
        col += w;
        spans.push(Span::styled(label, style));
        if i + 1 < tabs.count() {
            spans.push(Span::styled("│", Style::default().fg(Color::Gray)));
            col += 1;
        }
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

#[allow(clippy::too_many_arguments)]
fn draw_file_pane(
    f: &mut Frame,
    area: Rect,
    tabs: &PaneTabs,
    focused: bool,
    visual_anchor: Option<usize>,
    mode: Mode,
    bg: Option<Color>,
    flash: f32,
    pane_id: FocusedPane,
    tab_rects: &mut Vec<(FocusedPane, usize, Rect)>,
    git: Option<&cian_core::git::RepoStatus>,
) {
    let focus_bg = focus_badge_color(mode);
    let bg = bg.or(theme().base_bg);
    let mut border_style = if focused {
        Style::default().fg(focus_bg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme().border)
    };
    // An operation that just landed here lights the border, fading out.
    if flash > 0.0 {
        border_style = Style::default().fg(fade(theme().accent, flash)).add_modifier(Modifier::BOLD);
    }
    let max_title_w = area.width.saturating_sub(2);
    let mut offsets = Vec::new();
    let title = tabs_title(tabs, focused, focus_bg, max_title_w, &mut offsets);
    // The title is drawn on the top border row, one cell in from the corner.
    for (i, off, w) in offsets {
        tab_rects.push((pane_id, i, Rect::new(area.x + 1 + off, area.y, w, 1)));
    }
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(border_style)
        .title(title);
    if let Some(c) = bg {
        block = block.style(Style::default().bg(c));
    }

    let pane = tabs.active_ref();
    let visual_range = visual_anchor.map(|a| {
        if a <= pane.cursor { (a, pane.cursor) } else { (pane.cursor, a) }
    });

    // Columns are dropped progressively on narrow panes so the name always
    // keeps a usable amount of room.
    let inner_w = area.width.saturating_sub(2);
    let show_time = inner_w >= 52;
    let show_size = inner_w >= 34;
    // A git badge column (badge + space) only when the pane sits in a repo.
    let git_w: u16 = if git.is_some() { 2 } else { 0 };
    let meta_w = if show_time { SIZE_COL_W + TIME_COL_W + 2 } else if show_size { SIZE_COL_W + 1 } else { 0 };
    // 2 mark + icon + 2 spaces
    let name_w = inner_w.saturating_sub(meta_w + 5 + git_w) as usize;

    let items: Vec<ListItem> = pane.entries.iter().enumerate().map(|(i, e)| {
        let marked = pane.is_marked(i);
        let in_visual = visual_range.map(|(a, b)| i >= a && i <= b).unwrap_or(false);
        let mark_symbol = if marked { "● " } else { "  " };
        let mark_style = Style::default().fg(theme().mark_fg).add_modifier(Modifier::BOLD);
        let kind = kind_for(e);
        let mut name_style = Style::default().fg(kind.color());
        if kind.bold() {
            name_style = name_style.add_modifier(Modifier::BOLD);
        }
        // The icon carries the same color so the row reads as one unit.
        let icon_style = Style::default().fg(kind.color());

        let name = truncate(&e.name, name_w);
        let mut spans = Vec::new();
        if git.is_some() {
            let (badge, color) = git
                .and_then(|g| g.mark_for(&e.path))
                .map(|m| (m.badge(), git_mark_color(m)))
                .unwrap_or(("", Color::Reset));
            spans.push(Span::styled(
                format!("{:<1} ", badge),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
        }
        spans.extend([
            Span::styled(mark_symbol, mark_style),
            Span::styled(format!("{}  ", icon_for(e)), icon_style),
            Span::styled(format!("{:<w$}", name, w = name_w), name_style),
        ]);
        let meta_style = Style::default().fg(theme().dim);
        if show_size {
            // Directories have no meaningful byte count; the `..` row shows none.
            let s = if e.is_parent {
                String::new()
            } else if e.is_dir {
                "—".to_string()
            } else {
                cian_core::human_size(e.len)
            };
            spans.push(Span::styled(
                format!(" {:>w$}", s, w = SIZE_COL_W as usize),
                meta_style,
            ));
        }
        if show_time {
            let t = if e.is_parent {
                String::new()
            } else {
                e.modified.map(cian_core::format_time).unwrap_or_else(|| "-".into())
            };
            spans.push(Span::styled(format!(" {}", t), meta_style));
        }

        let mut item = ListItem::new(Line::from(spans));
        if in_visual { item = item.style(Style::default().bg(theme().visual_bg)); }
        item
    }).collect();

    // An unfocused pane recedes so the focused one reads as the active surface.
    let mut list_style = if focused {
        Style::default()
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    if let Some(c) = bg {
        list_style = list_style.bg(c);
    }
    let list = List::new(items)
        .block(block)
        .style(list_style)
        .highlight_style(
            Style::default().bg(theme().selected_bg).add_modifier(Modifier::BOLD),
        );

    let mut state = ListState::default();
    if !pane.entries.is_empty() { state.select(Some(pane.cursor)); }
    f.render_stateful_widget(list, area, &mut state);

    draw_list_scrollbar(f, area, pane.entries.len(), pane.cursor, focused, border_style);
}

/// The colour of a git status badge.
fn git_mark_color(m: cian_core::git::GitMark) -> Color {
    use cian_core::git::GitMark::*;
    match m {
        Staged => Color::Rgb(130, 225, 150),   // green
        Modified => Color::Rgb(240, 210, 120),  // yellow
        Untracked => Color::Rgb(130, 170, 210), // blue-grey
        Conflict => Color::Rgb(255, 130, 135),  // red
        DirDirty => Color::Rgb(180, 165, 110),  // muted yellow
    }
}

/// Fixed widths so the columns line up between the two panes.
const SIZE_COL_W: u16 = 5;
const TIME_COL_W: u16 = 16;

/// Draw a scrollbar on a pane's right border when the listing overflows.
fn draw_list_scrollbar(
    f: &mut Frame,
    area: Rect,
    total: usize,
    cursor: usize,
    focused: bool,
    border: Style,
) {
    let view_h = area.height.saturating_sub(2);
    if view_h == 0 || total <= view_h as usize {
        return;
    }
    let track = Rect::new(area.x + area.width.saturating_sub(1), area.y + 1, 1, view_h);
    let mut state = ScrollbarState::new(total).position(cursor);
    // The bar sits *on* the pane's right border, so the track has to be the
    // border: same glyph, same style. Drawing it in its own dimmer color made
    // the right edge look broken — bright where the thumb was, faded
    // elsewhere, while the other three sides stayed the border color.
    let thumb = if focused {
        border.add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::Rgb(120, 120, 145))
    };
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .thumb_symbol("│")
            .thumb_style(thumb)
            .track_symbol(Some("│"))
            .track_style(border)
            .begin_symbol(None)
            .end_symbol(None),
        track,
        &mut state,
    );
}

/// Draw the shell panel, then apply its background tint.
///
/// The tint has to be a post-pass. The PTY widget writes an explicit `Reset`
/// background into every cell the shell left uncolored, which would clobber
/// any background set on the block underneath. Recoloring only the cells
/// that are still `Reset` tints the panel while leaving alone every color
/// the shell chose for itself (ls colors, a vim theme, and so on).
#[allow(clippy::too_many_arguments)]
fn draw_shell(
    f: &mut Frame,
    area: Rect,
    shell: &mut ShellPane,
    focused: bool,
    dividers: &mut Vec<Divider>,
    leaves: &mut Vec<(usize, usize, Rect, Rect)>,
    ov: AnimOverride,
    tab_rects: &mut Vec<(FocusedPane, usize, Rect)>,
    log_border: Color,
) {
    draw_shell_inner(f, area, shell, focused, dividers, leaves, ov, tab_rects, log_border);
}

/// Repaint every still-uncolored cell in `area` with `bg`.
pub(crate) fn tint_default_cells(f: &mut Frame, area: Rect, bg: Color) {
    let buf = f.buffer_mut();
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            if let Some(cell) = buf.cell_mut((x, y)) {
                if cell.bg == Color::Reset {
                    cell.set_bg(bg);
                }
            }
        }
    }
}

/// Like [`tint_default_cells`], but also recolors the *foreground* of cells the
/// shell left at the terminal default. On a light theme the shell's own default
/// text is otherwise a pale terminal color on the pale base — the letters you
/// type look washed out. Colors the shell chose for itself are left alone.
fn tint_shell_base(f: &mut Frame, area: Rect, bg: Color, fg: Color) {
    let buf = f.buffer_mut();
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            if let Some(cell) = buf.cell_mut((x, y)) {
                if cell.bg == Color::Reset {
                    cell.set_bg(bg);
                }
                if cell.fg == Color::Reset {
                    cell.set_fg(fg);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_shell_inner(
    f: &mut Frame,
    area: Rect,
    shell: &mut ShellPane,
    focused: bool,
    dividers: &mut Vec<Divider>,
    leaves: &mut Vec<(usize, usize, Rect, Rect)>,
    ov: AnimOverride,
    tab_rects: &mut Vec<(FocusedPane, usize, Rect)>,
    log_border: Color,
) {
    // The panel border turns to the pulsing carmine when the pane it frames
    // (a lone leaf, or a maximized one) is recording.
    let panel_logs = shell
        .active_tab()
        .map(|t| {
            let single = t.leaves().len() == 1;
            (single || shell.zoom_pane)
                && matches!(
                    t.nodes.get(t.active).and_then(|n| n.as_ref()),
                    Some(Node::Leaf { session, .. }) if session.is_logging()
                )
        })
        .unwrap_or(false);
    let border_style = if panel_logs {
        Style::default().fg(log_border).add_modifier(Modifier::BOLD)
    } else if focused {
        Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme().border)
    };
    let mut offsets = Vec::new();
    let title = shell_tabs_title(shell, focused, &mut offsets);
    for (i, off, w) in offsets {
        tab_rects.push((FocusedPane::Shell, i, Rect::new(area.x + 1 + off, area.y, w, 1)));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(border_style)
        .title(title);
    let inner = area.inner(Margin { vertical: 1, horizontal: 1 });
    f.render_widget(block, area);

    // Remember the inner size for sizing newly-spawned panes.
    shell.rows = inner.height.max(1);
    shell.cols = inner.width.max(1);

    let active = shell.active;
    if shell.tabs.get(active).is_none() {
        let body = if let Some(err) = &shell.error {
            format!("shell failed to start: {}", err)
        } else if shell.is_starting() {
            "starting shell…".to_string()
        } else {
            "shell pane — focus here (Shift+J / click / :shell) to start a shell. \
             Esc returns to the files."
                .to_string()
        };
        f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), inner);
        return;
    }

    // Shift+F12: show only the active leaf, filling the panel. Suppressed
    // while a pane-zoom transition runs, so the splits show as the backdrop
    // the pane grows out of.
    if shell.zoom_pane && !ov.show_splits {
        let leaf = shell.tabs[active].active;
        if let Some(tab) = shell.tabs.get_mut(active) {
            if let Some(Node::Leaf { session: s, .. }) = tab.nodes.get_mut(leaf).and_then(|n| n.as_mut()) {
                s.resize(inner.height.max(1), inner.width.max(1));
            }
        }
        if let Some(Node::Leaf { session: s, bg }) = shell.tabs[active].nodes.get(leaf).and_then(|n| n.as_ref()) {
            if let Ok(parser) = s.parser().lock() {
                f.render_widget(PseudoTerminal::new(parser.screen()), inner);
            }
            if let Some(c) = bg {
                tint_default_cells(f, inner, *c);
            }
        }
        // A maximized pane hides its siblings; say how many, so it is clear
        // this is one of several and not the whole tab.
        let (pos, total) = shell.active_pane_position();
        if total > 1 {
            let badge = format!(" ▣ pane {}/{}  ({} hidden) ", pos, total, total - 1);
            let bw = badge.chars().count() as u16;
            if bw < inner.width {
                let at = Rect::new(inner.x + inner.width - bw, inner.y, bw, 1);
                f.render_widget(
                    Paragraph::new(badge).style(
                        Style::default()
                            .fg(Color::Black)
                            .bg(theme().accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    at,
                );
            }
        }
        if let Some(bg) = theme().base_bg {
            tint_shell_base(f, inner, bg, theme().file.plain);
        }
        return;
    }

    let root = shell.tabs[active].root;
    // While a transition runs the PTYs keep their old size; the real resize
    // happens on the frame after it lands.
    if !ov.freeze_pty {
        if let Some(tab) = shell.tabs.get_mut(active) {
            resize_node(tab, active, root, inner, false, ov);
        }
    }
    let tab = &shell.tabs[active];
    render_node(f, tab, active, root, inner, tab.active, focused, false, dividers, leaves, ov, log_border);
    // Fill any cell the shell left at the terminal default with the theme's
    // base, so a light theme's shell panel matches the rest.
    if let Some(bg) = theme().base_bg {
        tint_shell_base(f, inner, bg, theme().file.plain);
    }
}

/// Recursively size each leaf's PTY to its rect. `bordered` is true for leaves
/// inside a split (which draw a 1-cell border), false for a lone root leaf.
fn resize_node(tab: &mut ShellTab, tab_idx: usize, i: usize, area: Rect, bordered: bool, ov: AnimOverride) {
    let split = match tab.nodes.get(i).and_then(|n| n.as_ref()) {
        Some(Node::Split { dir, first, second, ratio }) => Some((*dir, *first, *second, *ratio)),
        Some(Node::Leaf { .. }) => None,
        None => return,
    };
    match split {
        None => {
            let (h, w) = if bordered {
                (area.height.saturating_sub(2).max(1), area.width.saturating_sub(2).max(1))
            } else {
                (area.height.max(1), area.width.max(1))
            };
            if let Some(Node::Leaf { session: s, .. }) = tab.nodes[i].as_mut() {
                s.resize(h, w);
            }
        }
        Some((dir, first, second, ratio)) => {
            let r = ov.ratio_for(DividerTarget::ShellSplit { tab: tab_idx, node: i }, ratio);
            let rects = split_rects(dir, area, r);
            resize_node(tab, tab_idx, first, rects.0, true, ov);
            resize_node(tab, tab_idx, second, rects.1, true, ov);
        }
    }
}

/// Recursively render the split tree. Leaves inside a split get a border (the
/// active one highlighted); a lone root leaf fills its area without one.
#[allow(clippy::too_many_arguments)]
fn render_node(
    f: &mut Frame,
    tab: &ShellTab,
    tab_idx: usize,
    i: usize,
    area: Rect,
    active_leaf: usize,
    focused: bool,
    bordered: bool,
    dividers: &mut Vec<Divider>,
    leaves: &mut Vec<(usize, usize, Rect, Rect)>,
    ov: AnimOverride,
    log_border: Color,
) {
    match tab.nodes.get(i).and_then(|n| n.as_ref()) {
        Some(Node::Leaf { session, bg }) => {
            let target = if bordered {
                let is_active = focused && i == active_leaf;
                let bs = if session.is_logging() {
                    Style::default().fg(log_border).add_modifier(Modifier::BOLD)
                } else if is_active {
                    Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let blk = Block::default().borders(Borders::ALL)
        .border_type(border_type()).border_style(bs);
                let pinner = area.inner(Margin { vertical: 1, horizontal: 1 });
                f.render_widget(blk, area);
                pinner
            } else {
                area
            };
            // (tab, leaf, outer area for focus, inner PTY area for selection).
            leaves.push((tab_idx, i, area, target));
            if let Ok(parser) = session.parser().lock() {
                f.render_widget(PseudoTerminal::new(parser.screen()), target);
            }
            // Tint after the PTY has drawn: it writes an explicit Reset
            // background into every cell the shell left uncolored, which would
            // otherwise clobber anything set underneath.
            if let Some(c) = bg {
                tint_default_cells(f, area, *c);
            }
        }
        Some(Node::Split { dir, first, second, ratio }) => {
            let target = DividerTarget::ShellSplit { tab: tab_idx, node: i };
            let rects = split_rects(*dir, area, ov.ratio_for(target, *ratio));
            let d = match dir {
                SplitDir::LeftRight => Direction::Horizontal,
                SplitDir::TopBottom => Direction::Vertical,
            };
            dividers.push(Divider {
                zone: seam_zone(d, rects.0, rects.1),
                parent: area,
                dir: d,
                target,
            });
            render_node(f, tab, tab_idx, *first, rects.0, active_leaf, focused, true, dividers, leaves, ov, log_border);
            render_node(f, tab, tab_idx, *second, rects.1, active_leaf, focused, true, dividers, leaves, ov, log_border);
        }
        None => {}
    }
}

/// Split a rect along `dir`, giving `ratio` percent of it to the first child.
fn split_rects(dir: SplitDir, area: Rect, ratio: u16) -> (Rect, Rect) {
    let direction = match dir {
        SplitDir::LeftRight => Direction::Horizontal,
        SplitDir::TopBottom => Direction::Vertical,
    };
    let first = ratio.min(100);
    let rects = Layout::default()
        .direction(direction)
        .constraints([Constraint::Percentage(first), Constraint::Percentage(100 - first)])
        .split(area);
    (rects[0], rects[1])
}

/// The band of cells that counts as grabbing the border between `a` and `b`.
/// The two rects are adjacent, so the seam is the last row/column of `a` plus
/// the first of `b` — two cells, which is a comfortable grab target.
fn seam_zone(dir: Direction, a: Rect, b: Rect) -> Rect {
    match dir {
        Direction::Horizontal => Rect {
            x: a.x + a.width.saturating_sub(1),
            y: a.y,
            width: 2.min(b.x + b.width - (a.x + a.width.saturating_sub(1))),
            height: a.height,
        },
        Direction::Vertical => Rect {
            x: a.x,
            y: a.y + a.height.saturating_sub(1),
            width: a.width,
            height: 2.min(b.y + b.height - (a.y + a.height.saturating_sub(1))),
        },
    }
}

/// A prompt line with a right-aligned hint, used by filter mode.
fn draw_prompt_line(f: &mut Frame, area: Rect, left: &str, right: &str) {
    let style = Style::default()
        .bg(Color::Rgb(20, 20, 30))
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    f.render_widget(Paragraph::new(left).style(style), area);
    let w = right.chars().count() as u16 + 1;
    if area.width > w {
        let hint = Rect::new(area.x + area.width - w, area.y, w, 1);
        f.render_widget(
            Paragraph::new(right).style(style.fg(Color::DarkGray).remove_modifier(Modifier::BOLD)),
            hint,
        );
    }
}

fn draw_command_line(f: &mut Frame, area: Rect, buf: &str) {
    let text = format!(":{}", buf);
    let p = Paragraph::new(text).style(
        Style::default().bg(Color::Rgb(20, 20, 30)).fg(Color::White).add_modifier(Modifier::BOLD),
    );
    f.render_widget(p, area);
}

/// Blend `c` toward white by `t` (0 = unchanged, 1 = fully lit). Used for the
/// operation flash, which fades a border back to its resting color.
/// The recording-border color at time `elapsed`: carmine that pulses between a
/// deep and a bright shade on a ~10-second cycle, so a logging pane reads as
/// "● recording" without ever disappearing.
fn recording_pulse(elapsed: std::time::Duration) -> Color {
    let period = 10.0_f32;
    let phase = (elapsed.as_secs_f32() % period) / period;
    // Smooth 0→1→0 over the cycle (cosine), never reaching either extreme.
    let level = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * phase).cos();
    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * level) as u8;
    // Deep carmine → bright carmine.
    Color::Rgb(lerp(120, 214), lerp(0, 45), lerp(20, 70))
}

fn fade(c: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let (r, g, b) = match c {
        Color::Rgb(r, g, b) => (r, g, b),
        // Named colors have no components to blend; approximate with a light
        // neutral so the flash still reads.
        _ => (200, 220, 255),
    };
    let mix = |v: u8| (v as f32 + (255.0 - v as f32) * t) as u8;
    Color::Rgb(mix(r), mix(g), mix(b))
}

fn focus_badge_color(mode: Mode) -> Color {
    match mode {
        Mode::Normal => theme().accent,
        Mode::Visual => Color::Rgb(255, 140, 0),
        Mode::Search => Color::Rgb(80, 200, 120),
        Mode::Command => Color::Rgb(200, 100, 200),
        Mode::Filter => Color::Rgb(80, 200, 120),
        Mode::Shell => Color::Rgb(200, 160, 60),
    }
}

/// The keys worth advertising in the current context.
///
/// Deliberately short and mode-specific: a bar listing everything is wallpaper
/// that stops being read. `?` is always last so the full manual is reachable
/// from whatever state the user is stuck in.
pub(crate) fn key_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    // Pick the English or Japanese label; the key column is the same either way.
    let ja = app.lang == Lang::Ja;
    let d = move |en: &'static str, jp: &'static str| -> &'static str {
        if ja {
            jp
        } else {
            en
        }
    };
    if app.focused == FocusedPane::Shell {
        let mut v = vec![("Esc", d("files", "ファイル"))];
        // Moving between split panes only exists once there is a split, and it
        // is the hint most worth showing then — the key is easy to forget and
        // there is nothing on screen otherwise to suggest it.
        if app.shell.active_pane_count() > 1 {
            v.push(("S-F1/S-F2", d("prev/next pane", "前/次のペイン")));
        }
        v.extend([
            // F1..F8 jump straight to tab N; naming F1/F2 stands in for the row.
            ("F1/F2", d("tab 1/2", "タブ1/2")),
            ("F9", d("new tab", "新規タブ")),
            ("F10", d("close tab", "タブを閉じる")),
            // Named per key rather than as a pair. "S-F8/F9" read as
            // "Shift+F8 or F9" — with plain F9 (new tab) sitting right beside
            // it — and gave no clue which key gave which orientation.
            ("S-F8", d("v-split", "左右分割")),
            ("S-F9", d("h-split", "上下分割")),
            ("S-F10", d("close split", "分割を閉じる")),
            ("F12", d("zoom", "ズーム")),
            // No `? help` here: in the shell `?` is a literal character that
            // goes to the running program, so advertising it would be a lie.
            // Shift+Enter opens the menu, which leads to the manual.
            ("S-Enter", d("menu", "メニュー")),
        ]);
        return v;
    }
    match app.mode {
        Mode::Visual => vec![
            ("j/k", d("extend", "伸ばす")),
            ("a", d("all", "全選択")),
            ("gg/G", d("top/bottom", "先頭/末尾")),
            ("Enter", d("confirm", "確定")),
            ("Esc", d("cancel", "取消")),
        ],
        Mode::Filter => vec![
            ("type", d("narrow", "絞込")),
            ("Enter", d("keep", "適用")),
            ("Esc", d("clear", "解除")),
        ],
        Mode::Command => vec![("Enter", d("run", "実行")), ("Esc", d("cancel", "取消"))],
        // Ordered by how often each is reached for: a narrow window drops
        // from the end, and `? help` is reserved separately. Kept short on
        // purpose — a bar listing everything becomes wallpaper, and the
        // manual is one keystroke away.
        _ => vec![
            // Switching focus between the two file panes and the shell is the
            // core two-pane move, so it leads the bar.
            ("←→", d("panes", "ペイン")),
            ("S-J", d("shell", "シェル")),
            ("Space", d("mark", "マーク")),
            ("/", d("filter", "絞込")),
            (",", d("sort", "並替")),
            ("S-F", d("find", "検索")),
            ("C-F", d("grep", "grep")),
            ("F3", d("view", "閲覧")),
            ("M", d("menu", "メニュー")),
            // The tab F-keys, which are otherwise invisible: F1/F2 step tabs,
            // F9 opens one, F10 closes one.
            ("F1/F2", d("prev/next tab", "前/次タブ")),
            ("F9", d("new tab", "新規タブ")),
            ("F10", d("close tab", "タブを閉じる")),
            // Last, so it is the first to drop on a narrow window: comparing
            // two files is the rarest of these by some distance.
            ("=", d("diff", "差分")),
            ("?", d("help", "ヘルプ")),
        ],
    }
}

fn draw_key_hints(f: &mut Frame, area: Rect, app: &App) {
    let key_style = Style::default()
        .fg(theme().accent)
        .bg(theme().status_bg)
        .add_modifier(Modifier::BOLD);
    let desc_style = Style::default().fg(Color::Rgb(150, 150, 170)).bg(theme().status_bg);
    let gap = Span::styled("   ", desc_style);

    let hints = key_hints(app);
    // +4 for the space between key and label plus the trailing gap. Display
    // width, not char count, so wide (CJK) labels don't overflow the row.
    let width_of = |(k, d): &(&str, &str)| width(k) as u16 + width(d) as u16 + 4;

    // The last hint is always `? help`. It is the way out of not knowing any
    // of the others, so it must never be the entry that a narrow window drops
    // — reserve its width and truncate the middle instead.
    let (body, tail) = hints.split_at(hints.len().saturating_sub(1));
    let reserved: u16 = tail.iter().map(width_of).sum();

    let mut spans = vec![Span::styled(" ", desc_style)];
    let mut used = 1u16;
    for h in body {
        let w = width_of(h);
        if used + w + reserved > area.width {
            break;
        }
        used += w;
        spans.push(Span::styled(h.0, key_style));
        spans.push(Span::styled(format!(" {}", h.1), desc_style));
        spans.push(gap.clone());
    }
    for h in tail {
        if used + width_of(h) <= area.width {
            spans.push(Span::styled(h.0, key_style));
            spans.push(Span::styled(format!(" {}", h.1), desc_style));
        }
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(theme().status_bg)),
        area,
    );
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let focus_label = match app.focused {
        FocusedPane::Left => "L",
        FocusedPane::Right => "R",
        FocusedPane::Shell => "S",
    };
    let badge_bg = focus_badge_color(app.mode);
    let (item_count, mark_count) = match app.active_pane() {
        Some(p) => (p.entries.len(), p.mark_count()),
        None => (0, 0),
    };
    let dim_sep = Span::styled(
        "  ▏  ",
        Style::default().fg(Color::Rgb(90, 90, 110)).bg(theme().status_bg),
    );
    let pad = Span::styled(" ", Style::default().bg(theme().status_bg));
    let chip = |label: String, fg: Color| {
        Span::styled(
            label,
            Style::default().fg(fg).bg(theme().status_bg).add_modifier(Modifier::BOLD),
        )
    };

    let ja = app.lang == Lang::Ja;
    let items_chip = if ja {
        format!("{} 件", item_count)
    } else {
        format!("{} items", item_count)
    };
    let marks_chip = if ja {
        format!("マーク {}", mark_count)
    } else {
        format!("marks {}", mark_count)
    };
    let mut spans: Vec<Span> = vec![
        Span::styled(
            format!(" {} ", focus_label),
            Style::default().fg(Color::Black).bg(badge_bg).add_modifier(Modifier::BOLD),
        ),
        pad.clone(),
        chip(items_chip, Color::White),
        dim_sep.clone(),
        chip(
            marks_chip,
            if mark_count > 0 { theme().mark_fg } else { Color::Rgb(140, 140, 160) },
        ),
    ];

    // A narrowed listing must never look like a complete one, so the active
    // filter stays visible after leaving filter mode.
    if let Some(filter) = app.active_pane().map(|p| p.filter.clone()).filter(|f| !f.is_empty()) {
        let total = app.active_pane().map(|p| p.all_entries.len()).unwrap_or(0);
        spans.push(dim_sep.clone());
        let filter_chip = if ja {
            format!("フィルタ /{} ({}/{} 件)", filter, item_count, total)
        } else {
            format!("filter /{} ({} of {})", filter, item_count, total)
        };
        spans.push(chip(filter_chip, Color::Rgb(80, 200, 120)));
    }

    // The git branch of the active pane's repository, with ahead/behind and a
    // changed-file count — the "branch bar" every developer glances at.
    if let Some(git) = app.git_for(app.focused) {
        spans.push(dim_sep.clone());
        let mut label = format!("\u{e0a0} {}", git.branch); //
        if git.ahead > 0 {
            label.push_str(&format!(" ↑{}", git.ahead));
        }
        if git.behind > 0 {
            label.push_str(&format!(" ↓{}", git.behind));
        }
        let changed = git.changed_count();
        if changed > 0 {
            label.push_str(&format!("  ✚{}", changed));
        }
        // Green when clean, amber when there are uncommitted changes.
        let color = if changed > 0 { Color::Rgb(240, 210, 120) } else { Color::Rgb(130, 205, 150) };
        spans.push(chip(label, color));
    }

    if app.zoomed {
        spans.push(dim_sep.clone());
        spans.push(chip("[zoom]".to_string(), theme().accent));
    }

    if let Some(msg) = app.message.as_ref() {
        if !msg.is_empty() {
            spans.push(dim_sep.clone());
            spans.push(Span::styled(
                format!("◂ {}", msg),
                Style::default()
                    .fg(theme().accent)
                    .bg(theme().status_bg)
                    .add_modifier(Modifier::ITALIC | Modifier::BOLD),
            ));
        }
    }

    let line = Line::from(spans);
    let p = Paragraph::new(line).style(Style::default().bg(theme().status_bg));
    f.render_widget(p, area);

    // The active shell pane's title (its `user@host: cwd`), right-aligned so it
    // sits in the bottom-right and tracks whichever split/tab is active —
    // rather than staying on the first pane. Drawn as its own right-aligned
    // paragraph over the same row.
    if let Some(title) = app.shell.active_title() {
        let shown = format!(" {} ", truncate(&title, (area.width / 2).max(8) as usize));
        f.render_widget(
            Paragraph::new(shown)
                .alignment(Alignment::Right)
                .style(
                    Style::default()
                        .fg(Color::Rgb(150, 200, 235))
                        .bg(theme().status_bg)
                        .add_modifier(Modifier::BOLD),
                ),
            area,
        );
    }
}

/// A progress bar for the running file operation, and the way to stop it.
fn draw_op_progress(f: &mut Frame, area: Rect, app: &App) {
    let Some(job) = &app.op_job else { return };
    draw_progress_bar(f, area, job.label, &job.latest, job.started, app.lang);
}

/// A centered progress dialog: label, current item, a bar, counts and elapsed.
/// Shared by file operations and the directory comparison.
fn draw_progress_bar(
    f: &mut Frame,
    area: Rect,
    label: &str,
    p: &cian_core::progress::Progress,
    started: Instant,
    lang: Lang,
) {
    let w = 74u16.min(area.width.saturating_sub(2));
    let rect = centered_rect(w, 8, area);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .title(format!(" {} ", tr_op_label(lang, label)));
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);

    // Which entry, shortened from the middle so the directory and the filename
    // both stay legible.
    f.render_widget(
        Paragraph::new(truncate_middle(&p.current, inner.width as usize))
            .style(Style::default().fg(Color::Rgb(190, 190, 210))),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    let frac = p.fraction().clamp(0.0, 1.0);
    let bar_y = inner.y + 2;
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(50, 50, 66))),
        Rect::new(inner.x, bar_y, inner.width, 1),
    );
    let filled = ((inner.width as f32) * frac).round() as u16;
    if filled > 0 {
        f.render_widget(
            Block::default().style(Style::default().bg(theme().accent)),
            Rect::new(inner.x, bar_y, filled.min(inner.width), 1),
        );
    }

    let counts = if p.bytes_total > 0 {
        format!(
            "{} / {}   ({} of {} files)",
            cian_core::human_size(p.bytes_done),
            cian_core::human_size(p.bytes_total),
            p.files_done,
            p.files_total
        )
    } else {
        format!("{} of {} files", p.files_done, p.files_total)
    };
    // Elapsed time, so a slow volume looks slow rather than stuck.
    let secs = started.elapsed().as_secs();
    let elapsed = if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    };
    f.render_widget(
        Paragraph::new(format!("{:>3}%   {}   ·  {}", (frac * 100.0) as u16, counts, elapsed)),
        Rect::new(inner.x, bar_y + 2, inner.width, 1),
    );
    f.render_widget(
        Paragraph::new(tr(lang, " Esc = stop ", " Esc = 中止 ")).style(
            Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
        ),
        Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
    );
}

#[allow(clippy::type_complexity)]
/// Register one clickable row spanning `inner`'s width at `y`, standing in for
/// selecting list index `idx`.
fn push_row_zone(zones: &mut Vec<PopupZone>, inner: Rect, y: u16, idx: usize) {
    zones.push(PopupZone {
        rect: Rect::new(inner.x, y, inner.width, 1),
        kind: ZoneKind::SelectRow(idx),
    });
}

/// The AI chat, rendered with `&mut App` so it can stash the transcript's rect,
/// scroll and flat lines for mouse selection.
/// The image preview: the picture as half-block (`▀`) cells — top pixel is the
/// glyph's foreground, bottom pixel its background — so it renders in any 24-bit
/// terminal without a graphics protocol. Decoded to fit and cached by size.
fn draw_image(f: &mut Frame, area: Rect, app: &mut App) {
    let lang = app.lang;
    let rect = centered_rect(area.width.saturating_sub(2), area.height.saturating_sub(2), area);
    f.render_widget(Clear, rect);
    let inner = rect.inner(Margin { vertical: 1, horizontal: 1 });
    let body_w = inner.width;
    let body_h = inner.height.saturating_sub(1); // leave a row for the footer

    // (Re)decode when first shown or after a resize.
    let (title, caption, rows, err) = if let Popup::ImageView { path, title, shown, error } = &mut app.popup {
        if error.is_none() && shown.as_ref().map(|(c, r, _)| (*c, *r)) != Some((body_w, body_h)) {
            match cian_core::image::thumbnail(path, body_w, body_h) {
                Ok(t) => *shown = Some((body_w, body_h, t)),
                Err(e) => *error = Some(e.to_string()),
            }
        }
        let mut rows: Vec<Line> = Vec::new();
        let mut caption = String::new();
        if let Some((_, _, t)) = shown {
            caption = format!("{}×{}px", t.src_w, t.src_h);
            for ry in 0..t.rows as usize {
                let mut spans: Vec<Span> = Vec::with_capacity(t.cols as usize);
                for cx in 0..t.cols as usize {
                    let (top, bot) = t.cells[ry * t.cols as usize + cx];
                    spans.push(Span::styled(
                        "▀",
                        Style::default()
                            .fg(Color::Rgb(top.0, top.1, top.2))
                            .bg(Color::Rgb(bot.0, bot.1, bot.2)),
                    ));
                }
                rows.push(Line::from(spans));
            }
        }
        (title.clone(), caption, rows, error.clone())
    } else {
        (String::new(), String::new(), Vec::new(), None)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(theme().popup_bg))
        .title(format!(" {}  —  {} ", title, caption));
    f.render_widget(block, rect);

    if let Some(e) = err {
        f.render_widget(
            Paragraph::new(format!("cannot show image: {}", e)).style(Style::default().fg(Color::Rgb(230, 120, 120))),
            inner,
        );
    } else {
        // Centre the picture in its box, vertically and horizontally.
        let img_h = rows.len() as u16;
        let img_w = rows.first().map(|l| l.spans.len() as u16).unwrap_or(0);
        let top = inner.y + (body_h.saturating_sub(img_h)) / 2;
        let left = inner.x + (body_w.saturating_sub(img_w)) / 2;
        let pic = Rect::new(left, top, img_w.min(body_w), img_h.min(body_h));
        f.render_widget(Paragraph::new(rows), pic);
    }

    let footer_area = Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
    f.render_widget(
        Paragraph::new(tr(lang, " S-Enter reveal   E edit   Esc close ", " S-Enter 場所へ   E 編集   Esc 閉じる "))
            .style(Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)),
        footer_area,
    );
}

/// Colour for a syntax-highlight category (a VS Code-dark-ish palette).
fn hl_style(cat: cian_core::highlight::Category) -> Style {
    use cian_core::highlight::Category as C;
    let c = match cat {
        C::Plain => Color::Rgb(210, 210, 222),
        C::Keyword => Color::Rgb(197, 134, 192), // mauve
        C::Type => Color::Rgb(78, 201, 176),      // teal
        C::Str => Color::Rgb(206, 145, 120),      // salmon
        C::Comment => Color::Rgb(106, 153, 85),   // green
        C::Number => Color::Rgb(181, 206, 168),   // pale green
        C::Tag => Color::Rgb(86, 156, 214),       // blue
        C::Attr => Color::Rgb(156, 220, 254),     // light blue
    };
    Style::default().fg(c)
}

fn draw_ai_chat(f: &mut Frame, area: Rect, app: &mut App) {
    let lang = app.lang;
    let width: u16 = 76u16.min(area.width.saturating_sub(2));
    let height = area.height.saturating_sub(2).max(8);
    let rect = centered_rect(width, height, area);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(theme().popup_bg))
        .title(tr(lang, " AI chat ", " AI チャット "))
        .title_bottom(tr(
            lang,
            " Enter=send  drag/Ctrl+Y=copy  Ctrl+V=paste  ↑↓  Esc ",
            " Enter=送信  ドラッグ/Ctrl+Y=コピー  Ctrl+V=貼付  ↑↓  Esc ",
        ));
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);
    let body_w = inner.width.max(1) as usize;
    let view_h = inner.height.saturating_sub(1) as usize;

    let mut flat: Vec<String> = Vec::new();
    let mut shown: Vec<Line> = Vec::new();
    let mut input_str = String::new();
    let mut off = 0usize;
    if let Popup::AiChat { input, log, scroll, pending, sel } = &mut app.popup {
        // Flat plain-text lines (for copying) and their styled counterparts.
        let mut styled: Vec<Line> = Vec::new();
        let push = |flat: &mut Vec<String>, styled: &mut Vec<Line>, prefix: &str, prefix_c: Color, body: String, body_c: Color| {
            styled.push(Line::from(vec![
                Span::styled(prefix.to_string(), Style::default().fg(prefix_c).add_modifier(Modifier::BOLD)),
                Span::styled(body.clone(), Style::default().fg(body_c)),
            ]));
            flat.push(body);
        };
        for m in log.iter() {
            let (tag, tag_c, body_c) = if m.user {
                ("you ", theme().accent, Color::Rgb(225, 225, 240))
            } else {
                ("ai  ", Color::Rgb(130, 205, 150), Color::Rgb(205, 210, 220))
            };
            let mut first = true;
            for raw in m.text.split('\n') {
                for chunk in wrap_str(raw, body_w.saturating_sub(4)) {
                    let prefix = if first { tag } else { "    " };
                    push(&mut flat, &mut styled, prefix, tag_c, chunk, body_c);
                    first = false;
                }
            }
            styled.push(Line::from(""));
            flat.push(String::new());
        }
        if *pending {
            styled.push(Line::from(Span::styled(
                tr(lang, "ai  …thinking", "ai  …考え中"),
                Style::default().fg(Color::Rgb(150, 150, 170)).add_modifier(Modifier::ITALIC),
            )));
            flat.push(String::new());
        }
        let max_scroll = flat.len().saturating_sub(view_h);
        off = (*scroll).min(max_scroll);
        *scroll = off; // usize::MAX means "stick to bottom"; clamp it here
        input_str = input.clone();

        let sel_range = sel.map(|(a, b)| (a.min(b), a.max(b)));
        for (i, line) in styled.into_iter().enumerate().skip(off).take(view_h) {
            let selected = sel_range.map(|(a, b)| i >= a && i <= b).unwrap_or(false);
            shown.push(if selected {
                line.style(Style::default().bg(theme().selected_bg))
            } else {
                line
            });
        }
    }

    // Stash the geometry so a mouse drag can map to a line range and copy it.
    app.ai_rect = Rect::new(inner.x, inner.y, inner.width, view_h as u16);
    app.ai_scroll = off;
    app.ai_lines = flat;

    f.render_widget(Paragraph::new(shown), app.ai_rect);
    f.render_widget(
        Paragraph::new(format!("> {}", input_str))
            .style(Style::default().fg(Color::Rgb(240, 240, 250)).bg(theme().selected_bg)),
        Rect::new(inner.x, inner.y + view_h as u16, inner.width, 1),
    );
}

/// The editable commit-message preview. `editing` shows a caret and a different
/// footer; otherwise it is a read-only preview with commit / edit / cancel keys.
fn draw_commit_message(f: &mut Frame, area: Rect, app: &mut App) {
    let lang = app.lang;
    let Popup::CommitMessage { buffer, stat, editing, .. } = &app.popup else { return };
    let editing = *editing;
    let width: u16 = 80u16.min(area.width.saturating_sub(2));
    let height = area.height.saturating_sub(2).clamp(10, 30);
    let rect = centered_rect(width, height, area);
    f.render_widget(Clear, rect);
    let title = if editing {
        tr(lang, " commit message — editing ", " コミットメッセージ — 編集中 ")
    } else {
        tr(lang, " commit message ", " コミットメッセージ ")
    };
    let footer = if editing {
        tr(lang, " type to edit   Enter=newline   Esc=done editing ",
              " 入力で編集   Enter=改行   Esc=編集終了 ")
    } else {
        tr(lang, " Enter/c=commit   e=edit   Esc=cancel ",
              " Enter/c=コミット   e=編集   Esc=取消 ")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(theme().popup_bg))
        .title(title)
        .title_bottom(footer);
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);

    let body_w = inner.width.max(1) as usize;
    let mut lines: Vec<Line> = Vec::new();
    // The staged-files summary, quietly, so the reviewer sees what it covers.
    if !stat.is_empty() {
        for raw in stat.lines() {
            for chunk in wrap_str(raw, body_w) {
                lines.push(Line::from(Span::styled(
                    chunk,
                    Style::default().fg(Color::Rgb(140, 140, 165)),
                )));
            }
        }
        lines.push(Line::from(Span::styled(
            "─".repeat(body_w.min(60)),
            Style::default().fg(Color::Rgb(90, 90, 110)),
        )));
    }
    // The message itself. A trailing block marks the edit point when editing.
    let subject_c = Color::Rgb(235, 235, 245);
    let body_c = Color::Rgb(205, 210, 220);
    let shown = if editing { format!("{}\u{2588}", buffer) } else { buffer.clone() };
    for (i, raw) in shown.split('\n').enumerate() {
        let c = if i == 0 { subject_c } else { body_c };
        let modifier = if i == 0 { Modifier::BOLD } else { Modifier::empty() };
        let wrapped = wrap_str(raw, body_w);
        if wrapped.is_empty() {
            lines.push(Line::from(""));
        }
        for chunk in wrapped {
            lines.push(Line::from(Span::styled(chunk, Style::default().fg(c).add_modifier(modifier))));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The junk-review list: a checkbox per candidate, its name, size and the
/// reason the AI gave. Nothing is deleted here — Enter hands the checked ones
/// to the normal delete confirmation.
fn draw_junk_review(f: &mut Frame, area: Rect, app: &mut App) {
    let lang = app.lang;
    let width: u16 = 88u16.min(area.width.saturating_sub(2));
    let height = area.height.saturating_sub(2).clamp(8, 30);
    let rect = centered_rect(width, height, area);
    f.render_widget(Clear, rect);
    let (n, checked) = if let Popup::JunkReview { items, .. } = &app.popup {
        (items.len(), items.iter().filter(|i| i.selected).count())
    } else {
        (0, 0)
    };
    let title = if lang == Lang::Ja {
        format!(" ゴミ候補  {}/{} 選択 ", checked, n)
    } else {
        format!(" junk candidates  {}/{} checked ", checked, n)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(theme().popup_bg))
        .title(title)
        .title_bottom(tr(lang,
            " Space/click=toggle  a=all  Enter/d=delete checked  Esc=cancel ",
            " Space/クリック=切替  a=全て  Enter/d=選択を削除  Esc=取消 "));
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);

    let body_h = inner.height as usize;
    let body_w = inner.width as usize;
    let mut rows: Vec<Line> = Vec::new();
    if let Popup::JunkReview { items, cursor, scroll } = &mut app.popup {
        // Keep the cursor in view.
        if *cursor < *scroll {
            *scroll = *cursor;
        } else if *cursor >= *scroll + body_h {
            *scroll = *cursor + 1 - body_h;
        }
        for (i, it) in items.iter().enumerate().skip(*scroll).take(body_h) {
            let sel = i == *cursor;
            let checkbox = if it.selected { "[x] " } else { "[ ] " };
            let box_c = if it.selected { theme().mark_fg } else { Color::Rgb(120, 120, 140) };
            let name = it.path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let name_c = if sel { theme().accent } else { Color::Rgb(230, 230, 245) };
            let reason = if it.reason.is_empty() { String::new() } else { format!("— {}", it.reason) };
            let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            rows.push(Line::from(vec![
                Span::styled(checkbox, base.fg(box_c).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{}  ", pad_to(&truncate_middle(&name, 28), 28)),
                    base.fg(name_c).add_modifier(Modifier::BOLD)),
                Span::styled(truncate(&reason, body_w.saturating_sub(36)),
                    base.fg(Color::Rgb(150, 150, 170))),
            ]));
        }
        app.junk_rect = Rect::new(inner.x, inner.y, inner.width, body_h.min(items.len().saturating_sub(*scroll)) as u16);
    }
    f.render_widget(Paragraph::new(rows), inner);
}

/// The duplicate-file review: files grouped by identical content, a checkbox
/// per copy (the keeper of each group left unchecked). Enter deletes the checked.
fn draw_dupe_review(f: &mut Frame, area: Rect, app: &mut App) {
    let lang = app.lang;
    let width: u16 = 96u16.min(area.width.saturating_sub(2));
    let height = area.height.saturating_sub(2).clamp(8, 30);
    let rect = centered_rect(width, height, area);
    f.render_widget(Clear, rect);
    let (n, checked) = if let Popup::DupeReview { items, .. } = &app.popup {
        (items.len(), items.iter().filter(|i| i.selected).count())
    } else {
        (0, 0)
    };
    let title = if lang == Lang::Ja {
        format!(" 重複ファイル  {}/{} 選択 ", checked, n)
    } else {
        format!(" duplicate files  {}/{} checked ", checked, n)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(theme().popup_bg))
        .title(title)
        .title_bottom(tr(lang,
            " Space/click=toggle  a=all  Enter/d=delete checked  Esc=cancel ",
            " Space/クリック=切替  a=全て  Enter/d=選択を削除  Esc=取消 "));
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);

    let body_h = inner.height as usize;
    let body_w = inner.width as usize;
    let mut rows: Vec<Line> = Vec::new();
    if let Popup::DupeReview { items, cursor, scroll } = &mut app.popup {
        if *cursor < *scroll {
            *scroll = *cursor;
        } else if *cursor >= *scroll + body_h {
            *scroll = *cursor + 1 - body_h;
        }
        for (i, it) in items.iter().enumerate().skip(*scroll).take(body_h) {
            let sel = i == *cursor;
            // A group-change gets a subtle "#N" tag so the groups read apart.
            let group_start = i == 0 || items.get(i.wrapping_sub(1)).map(|p| p.group != it.group).unwrap_or(true);
            let checkbox = if it.selected { "[x] " } else { "[ ] " };
            let box_c = if it.selected { theme().mark_fg } else { Color::Rgb(120, 120, 140) };
            let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            let tag = if group_start { format!("#{} ", it.group + 1) } else { "   ".to_string() };
            let path_c = if it.keeper { Color::Rgb(130, 205, 150) } else { Color::Rgb(220, 220, 235) };
            let suffix = if it.keeper { tr(lang, "  (keep)", "  (残す)") } else { "" };
            let shown = it.path.display().to_string();
            rows.push(Line::from(vec![
                Span::styled(checkbox, base.fg(box_c).add_modifier(Modifier::BOLD)),
                Span::styled(tag, base.fg(Color::Rgb(150, 150, 170))),
                Span::styled(truncate_middle(&shown, body_w.saturating_sub(14)), base.fg(path_c)),
                Span::styled(suffix, base.fg(Color::Rgb(130, 205, 150))),
            ]));
        }
        app.dupe_rect = Rect::new(inner.x, inner.y, inner.width, body_h.min(items.len().saturating_sub(*scroll)) as u16);
    }
    f.render_widget(Paragraph::new(rows), inner);
}

/// The structure-suggestion review: a checkbox per proposed move showing
/// `name → folder/`, with the AI's reason. Enter runs the checked moves.
fn draw_structure_review(f: &mut Frame, area: Rect, app: &mut App) {
    let lang = app.lang;
    let width: u16 = 92u16.min(area.width.saturating_sub(2));
    let height = area.height.saturating_sub(2).clamp(8, 30);
    let rect = centered_rect(width, height, area);
    f.render_widget(Clear, rect);
    let (n, checked) = if let Popup::StructureReview { items, .. } = &app.popup {
        (items.len(), items.iter().filter(|i| i.selected).count())
    } else {
        (0, 0)
    };
    let title = if lang == Lang::Ja {
        format!(" 構成の提案  {}/{} 選択 ", checked, n)
    } else {
        format!(" suggested structure  {}/{} checked ", checked, n)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(theme().popup_bg))
        .title(title)
        .title_bottom(tr(lang,
            " Space/click=toggle  a=all  Enter/m=move checked  Esc=cancel ",
            " Space/クリック=切替  a=全て  Enter/m=選択を移動  Esc=取消 "));
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);

    let body_h = inner.height as usize;
    let body_w = inner.width as usize;
    let mut rows: Vec<Line> = Vec::new();
    if let Popup::StructureReview { items, cursor, scroll, .. } = &mut app.popup {
        if *cursor < *scroll {
            *scroll = *cursor;
        } else if *cursor >= *scroll + body_h {
            *scroll = *cursor + 1 - body_h;
        }
        for (i, it) in items.iter().enumerate().skip(*scroll).take(body_h) {
            let sel = i == *cursor;
            let checkbox = if it.selected { "[x] " } else { "[ ] " };
            let box_c = if it.selected { theme().mark_fg } else { Color::Rgb(120, 120, 140) };
            let name_c = if sel { theme().accent } else { Color::Rgb(230, 230, 245) };
            let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            // `name  →  folder/`, then the reason quietly at the end.
            let arrow = format!("{}  →  {}/", pad_to(&truncate_middle(&it.name, 26), 26), it.dest);
            let reason = if it.reason.is_empty() { String::new() } else { format!("   — {}", it.reason) };
            rows.push(Line::from(vec![
                Span::styled(checkbox, base.fg(box_c).add_modifier(Modifier::BOLD)),
                Span::styled(truncate(&arrow, body_w.saturating_sub(6)),
                    base.fg(name_c).add_modifier(Modifier::BOLD)),
                Span::styled(truncate(&reason, body_w.saturating_sub(4)),
                    base.fg(Color::Rgb(150, 150, 170))),
            ]));
        }
        app.struct_rect = Rect::new(inner.x, inner.y, inner.width, body_h.min(items.len().saturating_sub(*scroll)) as u16);
    }
    f.render_widget(Paragraph::new(rows), inner);
}

/// The bulk-rename review: a checkbox per proposed rename showing `old → new`.
/// Enter renames the checked files in place.
fn draw_rename_review(f: &mut Frame, area: Rect, app: &mut App) {
    let lang = app.lang;
    let width: u16 = 92u16.min(area.width.saturating_sub(2));
    let height = area.height.saturating_sub(2).clamp(8, 30);
    let rect = centered_rect(width, height, area);
    f.render_widget(Clear, rect);
    let (n, checked) = if let Popup::RenameReview { items, .. } = &app.popup {
        (items.len(), items.iter().filter(|i| i.selected).count())
    } else {
        (0, 0)
    };
    let title = if lang == Lang::Ja {
        format!(" リネーム候補  {}/{} 選択 ", checked, n)
    } else {
        format!(" proposed renames  {}/{} checked ", checked, n)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(theme().popup_bg))
        .title(title)
        .title_bottom(tr(lang,
            " Space/click=toggle  a=all  Enter/r=rename checked  Esc=cancel ",
            " Space/クリック=切替  a=全て  Enter/r=選択をリネーム  Esc=取消 "));
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);

    let body_h = inner.height as usize;
    let body_w = inner.width as usize;
    let half = body_w.saturating_sub(8) / 2;
    let mut rows: Vec<Line> = Vec::new();
    if let Popup::RenameReview { items, cursor, scroll } = &mut app.popup {
        if *cursor < *scroll {
            *scroll = *cursor;
        } else if *cursor >= *scroll + body_h {
            *scroll = *cursor + 1 - body_h;
        }
        for (i, it) in items.iter().enumerate().skip(*scroll).take(body_h) {
            let sel = i == *cursor;
            let checkbox = if it.selected { "[x] " } else { "[ ] " };
            let box_c = if it.selected { theme().mark_fg } else { Color::Rgb(120, 120, 140) };
            let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            let old_c = if sel { Color::Rgb(230, 230, 245) } else { Color::Rgb(200, 200, 215) };
            rows.push(Line::from(vec![
                Span::styled(checkbox, base.fg(box_c).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{}  →  ", pad_to(&truncate_middle(&it.old, half), half)),
                    base.fg(old_c)),
                Span::styled(truncate_middle(&it.new, half),
                    base.fg(theme().accent).add_modifier(Modifier::BOLD)),
            ]));
        }
        app.rename_rect = Rect::new(inner.x, inner.y, inner.width, body_h.min(items.len().saturating_sub(*scroll)) as u16);
    }
    f.render_widget(Paragraph::new(rows), inner);
}

#[allow(clippy::too_many_arguments)]
fn draw_popup(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    hosts: &[cian_lua::SshHost],
    find: Option<(&str, &str, Option<cian_core::search::Outcome>, cian_core::search::Mode)>,
    dests: &[(String, PathBuf)],
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    // The manual is taller than any terminal, so it renders as a scrolling
    // viewport rather than the fixed block the other popups use.
    if let Popup::Manual { lines, scroll } = popup {
        let height = area.height.saturating_sub(2).max(6);
        let width: u16 = 70u16.min(area.width.saturating_sub(2));
        let rect = centered_rect(width, height, area);
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        let view_h = inner.height.saturating_sub(1) as usize;

        // Clamp so the last page sits flush with the bottom; this also
        // normalises an over-scrolled offset from the key handler.
        let max_scroll = lines.len().saturating_sub(view_h);
        *scroll = (*scroll).min(max_scroll);
        let offset = *scroll;

        f.render_widget(Clear, rect);
        let pos = match (offset * 100).checked_div(max_scroll) {
            Some(pct) => format!(" {}% ", pct),
            // Everything fits; there is nothing to scroll.
            None => " all ".to_string(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
        .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(tr(lang, " manual ", " キー一覧 "))
            .title_bottom(pos);
        f.render_widget(block, rect);

        let body: Vec<Line> = lines
            .iter()
            .skip(offset)
            .take(view_h)
            .map(|l| Line::from(l.clone()))
            .collect();
        let body_area = Rect::new(inner.x, inner.y, inner.width, view_h as u16);
        f.render_widget(Paragraph::new(body), body_area);

        let footer_area =
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
        let footer_text = match lang {
            Lang::En => " j/k scroll  u/d page  g/G  Esc close ",
            Lang::Ja => " j/k スクロール  u/d ページ  g/G  Esc 閉じる ",
        };
        let footer = Paragraph::new(footer_text).style(
            Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
        );
        f.render_widget(footer, footer_area);
        return;
    }
    // The context menu is anchored at the pointer rather than centred, so it
    // sizes and positions itself.
    if let Popup::ContextMenu { items, cursor, at } = popup {
        let w = items.iter().map(|i| width(i.label(lang))).max().unwrap_or(10) as u16 + 4;
        let rect = context_menu_rect(items, *at, area, lang);

        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent))
            .style(Style::default().bg(theme().popup_bg));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 1 });
        f.render_widget(block, rect);

        let rows: Vec<Line> = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let sel = i == *cursor;
                let style = if sel {
                    Style::default().bg(theme().selected_bg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(210, 210, 225))
                };
                Line::from(Span::styled(
                    format!("{}{}", if sel { "▸ " } else { "  " }, pad_to(item.label(lang), (w - 4) as usize)),
                    style,
                ))
            })
            .collect();
        f.render_widget(Paragraph::new(rows), inner);
        return;
    }

    if let Popup::SshHosts { cursor, filter } = popup {
        let needle = filter.to_lowercase();
        let matches: Vec<&cian_lua::SshHost> = hosts
            .iter()
            .filter(|h| {
                needle.is_empty()
                    || h.name.to_lowercase().contains(&needle)
                    || h.host.to_lowercase().contains(&needle)
            })
            .collect();
        let w = 56u16.min(area.width);
        let h = (matches.len() as u16 + 5).min(area.height.saturating_sub(2)).max(6);
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(tr(lang, " ssh — host ", " SSH — ホスト "));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let mut lines = vec![Line::from(Span::styled(
            format!("/{}_", filter),
            Style::default().fg(theme().accent).add_modifier(Modifier::BOLD),
        ))];
        if matches.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no match)",
                Style::default().fg(Color::Rgb(150, 150, 170)),
            )));
        }
        for (i, hst) in matches.iter().enumerate() {
            let sel = i == *cursor;
            let style = if sel {
                Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(205, 205, 218))
            };
            let users = if hst.users.len() == 1 {
                hst.users[0].name.clone()
            } else {
                format!("{} users", hst.users.len())
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{}{:<16}", if sel { "▸ " } else { "  " }, hst.name), style),
                Span::styled(
                    format!("{:<22} {}", hst.host, users),
                    Style::default().fg(Color::Rgb(140, 140, 165)),
                ),
            ]));
            // Row 0 is the filter line, so host `i` sits one below it.
            push_row_zone(zones, inner, inner.y + 1 + i as u16, i);
        }
        let body_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
        f.render_widget(Paragraph::new(lines), body_area);
        let footer_area =
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
        f.render_widget(
            Paragraph::new(tr(lang, " type to filter  ↑↓ select  Enter next  Esc cancel ", " 入力で絞込  ↑↓ 選択  Enter 次へ  Esc 取消 ")).style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            footer_area,
        );
        return;
    }

    if let Popup::SshUsers { host, cursor } = popup {
        let Some(hst) = hosts.get(*host) else { return };
        let w = 40u16.min(area.width);
        let h = (hst.users.len() as u16 + 4).min(area.height.saturating_sub(2)).max(6);
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(format!(" {} — {} ", tr(lang, "ssh", "SSH"), hst.name));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let lines: Vec<Line> = hst
            .users
            .iter()
            .enumerate()
            .map(|(i, u)| {
                let sel = i == *cursor;
                let style = if sel {
                    Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(205, 205, 218))
                };
                // A key marks logins that will authenticate without typing.
                let mark = if u.has_secret() { "  🔑" } else { "" };
                Line::from(Span::styled(
                    format!("{}{}@{}{}", if sel { "▸ " } else { "  " }, u.name, hst.host, mark),
                    style,
                ))
            })
            .collect();
        for i in 0..hst.users.len() {
            push_row_zone(zones, inner, inner.y + i as u16, i);
        }
        let body_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
        f.render_widget(Paragraph::new(lines), body_area);
        let footer_area =
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
        f.render_widget(
            Paragraph::new(tr(lang, " Enter connect   Esc back ", " Enter 接続   Esc 戻る ")).style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            footer_area,
        );
        return;
    }

    if let Popup::FindResults { hits, cursor, scroll } = popup {
        let w = 96u16.min(area.width.saturating_sub(2));
        let h = area.height.saturating_sub(4).max(8);
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let title = match find {
            Some((query, root, done, mode)) => {
                let verb = match mode {
                    cian_core::search::Mode::Name => "find",
                    cian_core::search::Mode::Content => "grep",
                };
                let state = match done {
                    None => "searching…".to_string(),
                    Some(cian_core::search::Outcome::Complete) => format!("{} found", hits.len()),
                    Some(cian_core::search::Outcome::Cancelled) => {
                        format!("{} found (stopped)", hits.len())
                    }
                    Some(cian_core::search::Outcome::Truncated) => {
                        format!("{} found (too many, stopped)", hits.len())
                    }
                };
                format!(" {} \"{}\" in {} — {} ", verb, query, root, state)
            }
            None => " find ".to_string(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(truncate_middle(&title, w.saturating_sub(4) as usize));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = inner.height.saturating_sub(1) as usize;
        // Keep the cursor on screen as results stream in beneath it.
        if *cursor < *scroll {
            *scroll = *cursor;
        } else if body_h > 0 && *cursor >= *scroll + body_h {
            *scroll = *cursor + 1 - body_h;
        }

        if hits.is_empty() {
            f.render_widget(
                Paragraph::new("(nothing yet)").style(Style::default().fg(Color::Rgb(150, 150, 170))),
                Rect::new(inner.x, inner.y, inner.width, 1),
            );
        }
        for (row, (i, hit)) in hits.iter().enumerate().skip(*scroll).take(body_h).enumerate() {
            let sel = i == *cursor;
            let y = inner.y + row as u16;
            let line_area = Rect::new(inner.x, y, inner.width, 1);
            push_row_zone(zones, inner, y, i);
            if sel {
                f.render_widget(
                    Block::default().style(Style::default().bg(theme().selected_bg)),
                    line_area,
                );
            }
            let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            // The directory part is context; the name is the answer.
            let rel = hit.rel.display().to_string();
            let (dir, name) = match rel.rfind(std::path::MAIN_SEPARATOR) {
                Some(i) => (rel[..=i].to_string(), rel[i + 1..].to_string()),
                None => (String::new(), rel.clone()),
            };
            let avail = inner.width.saturating_sub(4) as usize;
            let mut spans = vec![Span::styled(if sel { " ▸ " } else { "   " }, base)];
            match &hit.line {
                // A content match: the location is a prefix, the matched text
                // is the answer, so give the text the room and the emphasis.
                Some((n, text)) => {
                    let loc = format!("{}:{}  ", rel, n);
                    let loc_w = width(&loc).min(avail / 2);
                    spans.push(Span::styled(
                        truncate_middle(&loc, loc_w),
                        base.fg(Color::Rgb(135, 135, 160)),
                    ));
                    spans.push(Span::styled(
                        truncate(text, avail.saturating_sub(loc_w)),
                        base.fg(Color::Rgb(225, 225, 240)),
                    ));
                }
                None => {
                    spans.push(Span::styled(
                        truncate_middle(&dir, avail.saturating_sub(width(&name))),
                        base.fg(Color::Rgb(135, 135, 160)),
                    ));
                    spans.push(Span::styled(
                        name.clone(),
                        if hit.is_dir {
                            base.fg(FileKind::Directory.color()).add_modifier(Modifier::BOLD)
                        } else {
                            base.fg(Color::Rgb(225, 225, 240))
                        },
                    ));
                }
            }
            f.render_widget(Paragraph::new(Line::from(spans)), line_area);
        }
        f.render_widget(
            Paragraph::new(tr(lang, " Enter=go  j/k=move  Esc=close ", " Enter=移動  j/k=カーソル  Esc=閉じる ")).style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::Shortcuts { entries, cursor, path } = popup {
        let level = sc_level(entries, path);
        // Wide, because these are paths and URLs; the generic 70-column popup
        // wrapped them across lines, which made the list unreadable.
        let w = 96u16.min(area.width.saturating_sub(2));
        let h = (level.len() as u16 + 5).max(8).min(area.height.saturating_sub(2));
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        // Breadcrumb of the current group path in the title.
        let mut crumb = String::new();
        let mut walk: &[Shortcut] = entries;
        for &i in path.iter() {
            if let Some(s) = walk.get(i) {
                crumb.push_str(&format!(" / {}", s.name));
                walk = s.children.as_deref().unwrap_or(&[]);
            }
        }
        let title = format!("{}{} ", tr(lang, " shortcuts", " ショートカット"), crumb);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(title);
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = inner.height.saturating_sub(1);
        let footer_area =
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);

        if level.is_empty() {
            let hint = vec![
                Line::from(Span::styled(
                    tr(lang, "(empty)", "（空）"),
                    Style::default().fg(Color::Rgb(150, 150, 170)),
                )),
                Line::from(""),
                Line::from(tr(lang, "a = add a shortcut,  A = add a folder.", "a = ショートカット追加,  A = フォルダ追加。")),
            ];
            f.render_widget(
                Paragraph::new(hint),
                Rect::new(inner.x, inner.y, inner.width, body_h),
            );
        } else {
            // Name column sized to the longest name, within reason, so the
            // targets line up in a column of their own.
            let name_w = level
                .iter()
                .map(|s| width(&s.name))
                .max()
                .unwrap_or(8)
                .clamp(8, 24);
            let target_w = (inner.width as usize).saturating_sub(name_w + 8);

            // Keep the selected row visible once the list outgrows the popup.
            let view = body_h as usize;
            let first = cursor.saturating_sub(view.saturating_sub(1));
            for (row, (i, sc)) in level.iter().enumerate().skip(first).take(view).enumerate() {
                let sel = i == *cursor;
                let y = inner.y + row as u16;
                let line_area = Rect::new(inner.x, y, inner.width, 1);
                push_row_zone(zones, inner, y, i);
                if sel {
                    // A full-width bar, not just a marker: which row is active
                    // has to be obvious at a glance.
                    f.render_widget(
                        Block::default().style(Style::default().bg(theme().selected_bg)),
                        line_area,
                    );
                }
                let base = if sel {
                    Style::default().bg(theme().selected_bg)
                } else {
                    Style::default()
                };
                let name_style = if sel {
                    base.fg(theme().accent).add_modifier(Modifier::BOLD)
                } else {
                    base.fg(Color::Rgb(225, 225, 240)).add_modifier(Modifier::BOLD)
                };
                // The target is reference material: same row, quieter, so the
                // name is what the eye lands on.
                let target_style = base.fg(Color::Rgb(140, 140, 165));
                // A folder shows a ▸ and its child count instead of a target.
                let (icon, tail) = if sc.is_group() {
                    ("▸".to_string(), format!("{} items", sc.children.as_ref().map(|c| c.len()).unwrap_or(0)))
                } else {
                    (shortcut_icon(sc.target_str()).to_string(), truncate_middle(sc.target_str(), target_w))
                };
                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(if sel { " ▸ " } else { "   " }, name_style),
                        Span::styled(format!("{}  ", icon), base),
                        Span::styled(
                            format!("{}  ", pad_to(&truncate_middle(&sc.name, name_w), name_w)),
                            name_style,
                        ),
                        Span::styled(tail, target_style),
                    ])),
                    line_area,
                );
            }
        }
        f.render_widget(
            Paragraph::new(tr(lang, " Enter=open/into  a=add  A=folder  d=del  r=edit  ←=back  Esc ", " Enter=開く/入る  a=追加  A=フォルダ  d=削除  r=編集  ←=戻る  Esc "))
                .style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(theme().accent)
                        .add_modifier(Modifier::BOLD),
                ),
            footer_area,
        );
        return;
    }

    if let Popup::History { entries, cursor } = popup {
        // Its own renderer rather than the plain-text popup, so the selected
        // row gets the same highlight bar the shortcuts list has.
        let w = 96u16.min(area.width.saturating_sub(2));
        let h = (entries.len() as u16 + 5).max(6).min(area.height.saturating_sub(2));
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(format!(" {} ({}) ", tr(lang, "history", "履歴"), entries.len()));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = inner.height.saturating_sub(1) as usize;
        let first = cursor.saturating_sub(body_h.saturating_sub(1));
        for (row, (i, p)) in entries.iter().enumerate().skip(first).take(body_h).enumerate() {
            let sel = i == *cursor;
            let line_area = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
            push_row_zone(zones, inner, inner.y + row as u16, i);
            if sel {
                f.render_widget(
                    Block::default().style(Style::default().bg(theme().selected_bg)),
                    line_area,
                );
            }
            let base =
                if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            let text_style = if sel {
                base.fg(theme().accent).add_modifier(Modifier::BOLD)
            } else {
                base.fg(Color::Rgb(215, 215, 230))
            };
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(if sel { " ▸ " } else { "   " }, text_style),
                    Span::styled(
                        truncate_middle(&p.display().to_string(), inner.width as usize - 4),
                        text_style,
                    ),
                ])),
                line_area,
            );
        }
        f.render_widget(
            Paragraph::new(tr(lang, " ↑↓/jk select  Enter jump  a add shortcut  Esc cancel ", " ↑↓/jk 選択  Enter 移動  a ショートカット追加  Esc 取消 ")).style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::DestPicker { op, targets, cursor } = popup {
        let rows = dests.len();
        let w = 84u16.min(area.width.saturating_sub(2));
        let h = (rows as u16 + 6).min(area.height.saturating_sub(2));
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let verb = match (op, lang) {
            (PendingOp::Copy, Lang::En) => "copy",
            (PendingOp::Move, Lang::En) => "move",
            (PendingOp::Copy, Lang::Ja) => "コピー",
            (PendingOp::Move, Lang::Ja) => "移動",
        };
        let dp_title = if lang == Lang::Ja {
            format!(" {} 件を{} ", targets.len(), verb)
        } else {
            format!(" {} {} item(s) to ", verb, targets.len())
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(dp_title);
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        for (i, (kind, path)) in dests.iter().enumerate().take(inner.height.saturating_sub(2) as usize) {
            let sel = i == *cursor;
            let y = inner.y + i as u16;
            let line = Rect::new(inner.x, y, inner.width, 1);
            push_row_zone(zones, inner, y, i);
            if sel {
                f.render_widget(
                    Block::default().style(Style::default().bg(theme().selected_bg)),
                    line,
                );
            }
            let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(if sel { " ▸ " } else { "   " }, base),
                    Span::styled(
                        format!("{:<11}", kind),
                        base.fg(Color::Rgb(135, 135, 160)),
                    ),
                    Span::styled(
                        truncate_middle(&path.display().to_string(), inner.width as usize - 16),
                        base.fg(Color::Rgb(225, 225, 240)),
                    ),
                ])),
                line,
            );
        }
        f.render_widget(
            Paragraph::new(tr(lang, " Enter=send here   n=type a path   Esc=cancel ", " Enter=ここへ   n=パス入力   Esc=取消 ")).style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::Viewer { title, view, scroll, line, col, visual, anchor, find_input, find_query, git_lines, markdown, preview, source, md_styles, md_width, editing, dirty, editable, hl, hl_lang, .. } = popup {
        let w = area.width.saturating_sub(4);
        let h = area.height.saturating_sub(2);
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);

        // The preview owns `view.lines`: render the source to plain text plus a
        // parallel per-character style grid at the current width and swap it in;
        // leaving preview (or a width change) restores/re-wraps. Everything below
        // — cursor, visual selection, `/` search, the mouse — then works over
        // whichever text is on screen.
        let inner_w = rect.width.saturating_sub(4).max(1);
        if *preview {
            if md_styles.is_empty() || *md_width != inner_w {
                let (plain, styles) = crate::markdown::render_styled(source, inner_w as usize);
                view.lines = plain;
                *md_styles = styles;
                *md_width = inner_w;
            }
        } else if !md_styles.is_empty() {
            view.lines = source.clone();
            md_styles.clear();
            *md_width = 0;
        }
        *line = (*line).min(view.lines.len().saturating_sub(1));
        *col = (*col).min(view.lines.get(*line).map(|l| l.chars().count()).unwrap_or(0));

        // Syntax highlight source code (not the Markdown preview, not while
        // editing). Computed once and cached; the cache is cleared on an edit
        // or re-decode so it refreshes. Colours come from the per-char category.
        if !*preview && !*editing {
            if let Some(lang) = hl_lang {
                if hl.is_empty() {
                    *hl = cian_core::highlight::highlight(&view.lines, *lang)
                        .into_iter()
                        .map(|cats| cats.into_iter().map(hl_style).collect())
                        .collect();
                }
            }
        }

        let kind = match view.kind {
            cian_core::viewer::ViewKind::Text => view.encoding.label(),
            cian_core::viewer::ViewKind::Binary => "binary",
        };
        let size = cian_core::human_size(view.total_bytes);
        let cut = if view.truncated { "  (first 4M shown)" } else { "" };
        // A little mode badge in the title, so which visual mode is active — and
        // where the cursor sits — is never a guess.
        let mode = if *editing {
            "  [EDIT]".to_string()
        } else {
            match visual {
                None => String::new(),
                Some(ViewVisual::Char) => "  [VISUAL]".into(),
                Some(ViewVisual::Line) => "  [V-LINE]".into(),
                Some(ViewVisual::Block) => "  [V-BLOCK]".into(),
            }
        };
        let dirty_mark = if *dirty { " ●" } else { "" };
        let head = if *preview {
            tr(lang, "Markdown preview", "Markdown プレビュー").to_string()
        } else {
            format!("{}, {}{}", kind, size, cut)
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(format!(" {}{}  —  {} ", title, dirty_mark, head))
            .title_bottom(format!(" {}:{}{} ", *line + 1, *col + 1, mode));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = inner.height.saturating_sub(1) as usize;
        let max_scroll = view.lines.len().saturating_sub(body_h);
        *scroll = (*scroll).min(max_scroll);
        // Keep the cursor on screen (preview scrolls by moving the cursor too).
        if *line < *scroll {
            *scroll = *line;
        } else if *line >= *scroll + body_h.max(1) {
            *scroll = *line + 1 - body_h.max(1);
        }

        // Line numbers and the git change bar belong to the source only; the
        // rendered preview is a document, not a file listing.
        let numbered = !*preview && view.kind == cian_core::viewer::ViewKind::Text;
        let gutter = if numbered {
            format!("{}", view.lines.len()).len().max(3) + 1
        } else {
            0
        };
        let avail = (inner.width as usize).saturating_sub(gutter);

        // Ordered selection endpoints, for the highlight geometry.
        let (s0, e0) = order_pos(*anchor, (*line, *col));
        let sel_bg = Style::default().bg(theme().selected_bg);
        let cursor_style = Style::default().add_modifier(Modifier::REVERSED);
        let search_bg = Style::default().bg(Color::Rgb(120, 100, 0)).fg(Color::Rgb(255, 240, 190));
        let text_fg = if numbered { Color::Rgb(210, 210, 222) } else { Color::Rgb(200, 200, 215) };
        // Character columns matched by the active search, per line, for highlight.
        let needle = find_query.as_ref().map(|q| q.to_lowercase()).filter(|q| !q.is_empty());
        let match_cols = |l: &str| -> Vec<(usize, usize)> {
            let Some(nd) = needle.as_ref() else { return Vec::new() };
            let hay = l.to_lowercase();
            let nlen = nd.chars().count();
            let mut out = Vec::new();
            let mut from = 0usize;
            while let Some(rel) = hay[from..].find(nd.as_str()) {
                let byte = from + rel;
                let start = hay[..byte].chars().count();
                out.push((start, start + nlen.saturating_sub(1)));
                from = byte + nd.len().max(1);
            }
            out
        };

        // The inclusive selected column range on absolute line `i`, if any.
        let sel_cols = |i: usize, len: usize| -> Option<(usize, usize)> {
            match visual {
                None => None,
                Some(ViewVisual::Line) => {
                    if i >= s0.0 && i <= e0.0 { Some((0, len)) } else { None }
                }
                Some(ViewVisual::Block) => {
                    if i >= s0.0 && i <= e0.0 {
                        Some((anchor.1.min(*col), anchor.1.max(*col)))
                    } else {
                        None
                    }
                }
                Some(ViewVisual::Char) => {
                    if i < s0.0 || i > e0.0 {
                        None
                    } else if s0.0 == e0.0 {
                        Some((s0.1, e0.1))
                    } else if i == s0.0 {
                        Some((s0.1, len))
                    } else if i == e0.0 {
                        Some((0, e0.1))
                    } else {
                        Some((0, len))
                    }
                }
            }
        };

        let rows: Vec<Line> = view
            .lines
            .iter()
            .enumerate()
            .skip(*scroll)
            .take(body_h)
            .map(|(i, l)| {
                let chars: Vec<char> = l.chars().take(avail).collect();
                let len = chars.len();
                let sel = sel_cols(i, len);
                let cur = if i == *line { Some(*col) } else { None };
                let matches = match_cols(l);
                let cell_style = |j: usize| -> Style {
                    // Priority: cursor over selection over a search match; the
                    // resting style is the Markdown colour in preview, else plain.
                    if cur == Some(j) {
                        cursor_style.fg(text_fg)
                    } else if sel.map(|(a, b)| j >= a && j <= b).unwrap_or(false) {
                        sel_bg.fg(text_fg)
                    } else if matches.iter().any(|(a, b)| j >= *a && j <= *b) {
                        search_bg
                    } else if *preview {
                        md_styles.get(i).and_then(|s| s.get(j)).copied().unwrap_or_default()
                    } else if !*editing && !hl.is_empty() {
                        hl.get(i).and_then(|s| s.get(j)).copied().unwrap_or(Style::default().fg(text_fg))
                    } else {
                        Style::default().fg(text_fg)
                    }
                };
                // Build the body char-by-char, merging same-styled runs.
                let mut spans: Vec<Span> = Vec::new();
                if numbered {
                    // The line number, then a 1-column separator that doubles as
                    // the git change bar (green added / amber modified / red for
                    // a deletion just above). Keeping the width fixed means the
                    // mouse column mapping is unaffected.
                    spans.push(Span::styled(
                        format!("{:>w$}", i + 1, w = gutter.saturating_sub(1)),
                        Style::default().fg(Color::Rgb(110, 110, 135)),
                    ));
                    // The 1-column separator (previously a plain space) is the
                    // change bar.
                    let (bar, bar_c) = match git_lines.get(&i) {
                        Some(cian_core::git::LineChange::Added) => ("▏", Color::Rgb(130, 205, 150)),
                        Some(cian_core::git::LineChange::Modified) => ("▏", Color::Rgb(240, 210, 120)),
                        Some(cian_core::git::LineChange::DeletedBefore) => ("▁", Color::Rgb(230, 120, 120)),
                        None => (" ", Color::Reset),
                    };
                    spans.push(Span::styled(bar.to_string(), Style::default().fg(bar_c)));
                }
                let mut run = String::new();
                let mut run_style = cell_style(0);
                for (j, ch) in chars.iter().enumerate() {
                    let st = cell_style(j);
                    if st != run_style && !run.is_empty() {
                        spans.push(Span::styled(std::mem::take(&mut run), run_style));
                    }
                    run_style = st;
                    run.push(*ch);
                }
                if !run.is_empty() {
                    spans.push(Span::styled(run, run_style));
                }
                // The cursor can sit just past the last char (empty line, or end
                // of line): show it as a reversed space so it stays visible.
                if cur == Some(len) {
                    spans.push(Span::styled(" ".to_string(), cursor_style));
                }
                Line::from(spans)
            })
            .collect();
        let body_area = Rect::new(inner.x, inner.y, inner.width, body_h as u16);
        f.render_widget(Paragraph::new(rows), body_area);
        let pos = match max_scroll {
            0 => "all".to_string(),
            m => format!("{}%", *scroll * 100 / m),
        };
        // While editing, the footer shows the editor keys; while typing a
        // search, the `/` prompt; otherwise the usual hints.
        let ed = if *editable { tr(lang, " i edit ", " i 編集 ") } else { " " };
        let footer = if *editing {
            tr(lang,
                " EDIT — type to insert   Ctrl+S save   Esc leave   Shift+Q discard ",
                " 編集中 — 入力で挿入   Ctrl+S 保存   Esc 終了   Shift+Q 破棄 ").to_string()
        } else {
            match find_input {
                Some(q) => format!("/{}_", q),
                None => {
                    let hints = if *preview {
                        format!("{}{}{}",
                            tr(lang, " / f search  n/N  v/V select  y copy ", " / f 検索  n/N  v/V 選択  y コピー "),
                            ed,
                            tr(lang, " E ext-edit  p source  ", " E 外部編集  p ソース  "))
                    } else if *markdown {
                        format!("{}{}{}",
                            tr(lang, " / f search  n/N  v/V select  y copy ", " / f 検索  n/N  v/V 選択  y コピー "),
                            ed,
                            tr(lang, " e enc  p preview  ", " e 文字コード  p プレビュー  "))
                    } else {
                        format!("{}{}{}",
                            tr(lang, " / f search  n/N  v/V select  y copy ", " / f 検索  n/N  v/V 選択  y コピー "),
                            ed,
                            tr(lang, " E ext-edit  S-Enter reveal  e enc  ", " E 外部編集  S-Enter 場所へ  e 文字コード  "))
                    };
                    format!("{}{} ", hints, pos)
                }
            }
        };
        f.render_widget(
            Paragraph::new(footer)
                .style(Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::DirCompare { left, right, entries, cursor, scroll, truncated, .. } = popup {
        use cian_core::dirdiff::Status;
        let rect = centered_rect(area.width.saturating_sub(2), area.height.saturating_sub(2), area);
        f.render_widget(Clear, rect);
        let counts = {
            let (mut a, mut d, mut m) = (0, 0, 0);
            for e in entries.iter() {
                match e.status {
                    Status::OnlyRight => a += 1,
                    Status::OnlyLeft => d += 1,
                    Status::Differ => m += 1,
                }
            }
            let cut = if *truncated { "  (stopped at 5000)" } else { "" };
            format!("~{} +{} -{}{}", m, a, d, cut)
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(format!(" {}  ↔  {}   —   {} ", left, right, counts));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = (inner.height.saturating_sub(1) as usize).max(1);
        // Keep the cursor on screen.
        if *cursor < *scroll {
            *scroll = *cursor;
        } else if *cursor >= *scroll + body_h {
            *scroll = *cursor + 1 - body_h;
        }
        let first = *scroll;
        let add = Color::Rgb(130, 225, 150);
        let del = Color::Rgb(255, 140, 145);
        let chg = Color::Rgb(240, 210, 120);
        for (row, (i, e)) in entries.iter().enumerate().skip(first).take(body_h).enumerate() {
            let sel = i == *cursor;
            let y = inner.y + row as u16;
            let line = Rect::new(inner.x, y, inner.width, 1);
            push_row_zone(zones, inner, y, i);
            if sel {
                f.render_widget(Block::default().style(Style::default().bg(theme().selected_bg)), line);
            }
            let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            let (mark, col) = match e.status {
                Status::OnlyRight => ("+ ", add),
                Status::OnlyLeft => ("- ", del),
                Status::Differ => ("~ ", chg),
            };
            let mut name = e.rel.display().to_string().replace('\\', "/");
            if e.is_dir {
                name.push('/');
            }
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(format!(" {}", mark), base.fg(col).add_modifier(Modifier::BOLD)),
                    Span::styled(truncate_middle(&name, inner.width as usize - 4), base.fg(col)),
                ])),
                line,
            );
        }
        f.render_widget(
            Paragraph::new(tr(lang,
                " + right-only   - left-only   ~ differ    Enter=go to   j/k  Esc close ",
                " + 右のみ   - 左のみ   ~ 相違    Enter=移動   j/k  Esc 閉じる ",
            ))
            .style(Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::Diff { left, right, result, folded, fold, scroll, encoding, find, find_input, .. } = popup {
        use cian_core::diff::Row;

        let rect = centered_rect(area.width.saturating_sub(2), area.height.saturating_sub(2), area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(format!(
                " {} ↔ {}  —  {} ",
                left,
                right,
                cian_core::diff::summary(result)
            ));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = inner.height.saturating_sub(1) as usize;
        let rows: &[Row] = if *fold { folded } else { &result.rows };
        let max_scroll = rows.len().saturating_sub(body_h);
        *scroll = (*scroll).min(max_scroll);

        // Two equal columns with a marker between them, so the eye can run
        // straight down either file.
        let gutter = 5usize;
        let col = (inner.width as usize).saturating_sub(3 + gutter * 2) / 2;

        let dim = Style::default().fg(Color::Rgb(150, 150, 168));
        let num = Style::default().fg(Color::Rgb(105, 105, 130));
        let del = Style::default().fg(Color::Rgb(255, 140, 145));
        let add = Style::default().fg(Color::Rgb(130, 225, 150));
        let chg = Style::default().fg(Color::Rgb(240, 210, 120));

        let cell = |line: Option<&cian_core::diff::Line>, style: Style| -> Vec<Span<'static>> {
            match line {
                Some(l) => vec![
                    Span::styled(format!("{:>w$} ", l.no, w = gutter - 1), num),
                    Span::styled(pad_to(&truncate(&l.text, col), col), style),
                ],
                // An absent side is left blank rather than filled, so the gap
                // itself shows which file the line is missing from.
                None => vec![Span::raw(" ".repeat(gutter + col))],
            }
        };

        // Rows whose text matches the active search get a highlight bar.
        let needle = find.as_ref().map(|s| s.to_lowercase());
        let row_matches = |r: &Row| -> bool {
            let Some(q) = &needle else { return false };
            let has = |o: Option<&cian_core::diff::Line>| o.map(|l| l.text.to_lowercase().contains(q)).unwrap_or(false);
            match r {
                Row::Same { left, right } | Row::Changed { left, right } => has(Some(left)) || has(Some(right)),
                Row::Removed { left } => has(Some(left)),
                Row::Added { right } => has(Some(right)),
                Row::Skipped { .. } => false,
            }
        };
        let search_bg = Style::default().bg(Color::Rgb(80, 70, 20));
        let body: Vec<Line> = rows
            .iter()
            .skip(*scroll)
            .take(body_h)
            .map(|r| {
                let line = match r {
                    Row::Skipped { lines } => Line::from(Span::styled(
                        format!("{:^w$}", format!("⋯ {} identical lines", lines), w = inner.width as usize),
                        Style::default().fg(Color::Rgb(95, 95, 120)),
                    )),
                    Row::Same { left: l, right: rr } => {
                        let mut s = cell(Some(l), dim);
                        s.push(Span::styled(" │ ", num));
                        s.extend(cell(Some(rr), dim));
                        Line::from(s)
                    }
                    Row::Changed { left: l, right: rr } => {
                        let mut s = cell(Some(l), chg);
                        s.push(Span::styled(" ~ ", chg.add_modifier(Modifier::BOLD)));
                        s.extend(cell(Some(rr), chg));
                        Line::from(s)
                    }
                    Row::Removed { left: l } => {
                        let mut s = cell(Some(l), del);
                        s.push(Span::styled(" - ", del.add_modifier(Modifier::BOLD)));
                        s.extend(cell(None, del));
                        Line::from(s)
                    }
                    Row::Added { right: rr } => {
                        let mut s = cell(None, add);
                        s.push(Span::styled(" + ", add.add_modifier(Modifier::BOLD)));
                        s.extend(cell(Some(rr), add));
                        Line::from(s)
                    }
                };
                if row_matches(r) { line.style(search_bg) } else { line }
            })
            .collect();

        // A binary comparison has no rows; say why rather than showing a void.
        let body = if result.binary {
            vec![Line::from(Span::styled(
                if result.identical {
                    "  These are binary files, and they are byte-for-byte the same."
                } else {
                    "  These are binary files, and their contents differ."
                },
                dim,
            ))]
        } else if result.identical {
            vec![Line::from(Span::styled("  The two files are identical.", add))]
        } else {
            body
        };

        f.render_widget(
            Paragraph::new(body),
            Rect::new(inner.x, inner.y, inner.width, body_h as u16),
        );
        let pos = match max_scroll {
            0 => "all".to_string(),
            m => format!("{}%", *scroll * 100 / m),
        };
        let fold_word = if *fold { tr(lang, "show all", "全表示") } else { tr(lang, "fold", "畳む") };
        // A live `/` search prompt takes over the footer while typing.
        let footer = if let Some(q) = find_input {
            format!(" /{}_ ", q)
        } else {
            format!(
                "{}{}  {}  [{}] {} ",
                tr(lang, " n/N change  / find  f ", " n/N 変更  / 検索  f "),
                fold_word,
                tr(lang, "c copy  w save  e enc  g/G  Esc",
                      "c コピー  w 保存  e 文字コード  g/G  Esc"),
                encoding.label(),
                pos
            )
        };
        f.render_widget(
            Paragraph::new(footer)
            .style(Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::Archive { path, members, cursor, scroll } = popup {
        let w = 96u16.min(area.width.saturating_sub(2));
        let h = area.height.saturating_sub(4).max(8);
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let total: u64 = members.iter().map(|m| m.size).sum();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(format!(
                " {}  —  {} entries, {} unpacked ",
                name,
                members.len(),
                cian_core::human_size(total)
            ));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let body_h = inner.height.saturating_sub(1) as usize;
        if *cursor < *scroll {
            *scroll = *cursor;
        } else if body_h > 0 && *cursor >= *scroll + body_h {
            *scroll = *cursor + 1 - body_h;
        }
        for (row, (i, m)) in members.iter().enumerate().skip(*scroll).take(body_h).enumerate() {
            let sel = i == *cursor;
            let line = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
            push_row_zone(zones, inner, inner.y + row as u16, i);
            if sel {
                f.render_widget(
                    Block::default().style(Style::default().bg(theme().selected_bg)),
                    line,
                );
            }
            let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
            let size = if m.is_dir { "—".to_string() } else { cian_core::human_size(m.size) };
            let name_w = inner.width as usize - 14;
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(if sel { " ▸ " } else { "   " }, base),
                    Span::styled(
                        format!("{:<w$}", truncate_middle(&m.name, name_w), w = name_w),
                        if m.is_dir {
                            base.fg(FileKind::Directory.color()).add_modifier(Modifier::BOLD)
                        } else {
                            base.fg(Color::Rgb(225, 225, 240))
                        },
                    ),
                    Span::styled(format!("{:>6}", size), base.fg(Color::Rgb(140, 140, 165))),
                ])),
                line,
            );
        }
        f.render_widget(
            Paragraph::new(tr(lang, " Enter=extract this   a=extract all   Esc=close ", " Enter=これを展開   a=全展開   Esc=閉じる ")).style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::Macros { cursor, names } = popup {
        let widest = names.iter().map(|n| n.chars().count()).max().unwrap_or(10);
        let w = (widest as u16 + 8).clamp(28, area.width);
        let h = (names.len() as u16 + 3).min(area.height);
        let rect = centered_rect(w, h, area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(tr(lang, " run a macro ", " マクロを実行 "));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let rows: Vec<Line> = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let sel = i == *cursor;
                let style = if sel {
                    Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(200, 200, 215))
                };
                Line::from(Span::styled(
                    format!("{}{}", if sel { "▸ " } else { "  " }, name),
                    style,
                ))
            })
            .collect();
        let body_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
        f.render_widget(Paragraph::new(rows), body_area);
        for i in 0..names.len() {
            push_row_zone(zones, inner, inner.y + i as u16, i);
        }
        let footer_area = Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
        f.render_widget(
            Paragraph::new(tr(lang, " Enter=run  j/k  Esc ", " Enter=実行  j/k  Esc ")).style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            footer_area,
        );
        return;
    }

    if let Popup::SortPicker { cursor } = popup {
        let w = 34u16.min(area.width);
        let h = SortKey::ALL.len() as u16 + 3;
        let rect = centered_rect(w, h.min(area.height), area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(tr(lang, " sort by ", " 並び替え "));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let rows: Vec<Line> = SortKey::ALL
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let sel = i == *cursor;
                let style = if sel {
                    Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(200, 200, 215))
                };
                // The shortcut letter doubles as the mnemonic.
                let hint = match k {
                    SortKey::Name => "n",
                    SortKey::Size => "s",
                    SortKey::Modified => "d",
                    SortKey::Extension => "e",
                };
                Line::from(Span::styled(
                    format!("{}{}  ({})", if sel { "▸ " } else { "  " }, k.label(), hint),
                    style,
                ))
            })
            .collect();
        let body_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
        f.render_widget(Paragraph::new(rows), body_area);
        for i in 0..SortKey::ALL.len() {
            push_row_zone(zones, inner, inner.y + i as u16, i);
        }
        let footer_area =
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
        f.render_widget(
            Paragraph::new(tr(lang, " Enter=apply (again = reverse)  Esc ", " Enter=適用（再度で逆順）  Esc ")).style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            footer_area,
        );
        return;
    }

    if let Popup::EncodingPicker { cursor, .. } = popup {
        use cian_core::viewer::TextEncoding;
        let w = 34u16.min(area.width);
        let h = TextEncoding::ALL.len() as u16 + 3;
        let rect = centered_rect(w, h.min(area.height), area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(tr(lang, " text encoding ", " 文字コード "));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);
        let rows: Vec<Line> = TextEncoding::ALL
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let sel = i == *cursor;
                let style = if sel {
                    Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(200, 200, 215))
                };
                Line::from(Span::styled(
                    format!("{}{}", if sel { "▸ " } else { "  " }, e.label()),
                    style,
                ))
            })
            .collect();
        f.render_widget(
            Paragraph::new(rows),
            Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1)),
        );
        for i in 0..TextEncoding::ALL.len() {
            push_row_zone(zones, inner, inner.y + i as u16, i);
        }
        f.render_widget(
            Paragraph::new(tr(lang, " Enter=apply  Esc=cancel ", " Enter=適用  Esc=取消 ")).style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
        );
        return;
    }

    if let Popup::ColorPicker { cursor, .. } = popup {
        let w = 26u16.min(area.width);
        let h = PANE_BG_PRESETS.len() as u16 + 3;
        let rect = centered_rect(w, h.min(area.height), area);
        f.render_widget(Clear, rect);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type())
            .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
            .title(tr(lang, " background ", " 背景色 "));
        let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
        f.render_widget(block, rect);

        let rows: Vec<Line> = PANE_BG_PRESETS
            .iter()
            .enumerate()
            .map(|(i, (name, color))| {
                let sel = i == *cursor;
                // A swatch of the actual color, so the name is not the only cue.
                let swatch = Span::styled(
                    "  ",
                    Style::default().bg(color.unwrap_or(Color::Rgb(16, 16, 20))),
                );
                let label = Span::styled(
                    format!(" {}{}", if sel { "▸ " } else { "  " }, name),
                    if sel {
                        Style::default().add_modifier(Modifier::BOLD).fg(theme().accent)
                    } else {
                        Style::default().fg(Color::Rgb(200, 200, 215))
                    },
                );
                Line::from(vec![swatch, label])
            })
            .collect();
        let body_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
        f.render_widget(Paragraph::new(rows), body_area);
        for i in 0..PANE_BG_PRESETS.len() {
            push_row_zone(zones, inner, inner.y + i as u16, i);
        }
        let footer_area =
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
        f.render_widget(
            Paragraph::new(tr(lang, " Enter=apply  Esc=cancel ", " Enter=適用  Esc=取消 ")).style(
                Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            footer_area,
        );
        return;
    }

    let popup: &Popup = popup;
    let (title, body, footer) = match popup {
        Popup::ConfirmDelete { targets } => {
            let title = tr(lang, " delete ", " 削除 ").to_string();
            let head = if lang == Lang::Ja {
                format!("{} 件 → ゴミ箱:", targets.len())
            } else {
                format!("{} item(s) → trash:", targets.len())
            };
            let mut lines = vec![head, String::new()];
            for p in targets.iter().take(8) { lines.push(format!("  {}", p.display())); }
            if targets.len() > 8 {
                lines.push(tr_count(lang, targets.len() - 8));
            }
            let foot = tr(lang, " y/Enter=trash  a=delete permanently  n/Esc=cancel ",
                " y/Enter=ゴミ箱  a=完全削除  n/Esc=取消 ");
            (title, lines, foot.to_string())
        }
        Popup::ConfirmTransfer { op, targets, dest } => {
            let title = match (op, lang) {
                (PendingOp::Copy, Lang::Ja) => " コピー ",
                (PendingOp::Move, Lang::Ja) => " 移動 ",
                (PendingOp::Copy, Lang::En) => " copy ",
                (PendingOp::Move, Lang::En) => " move ",
            }.to_string();
            let head = format!("{} {} → {}", targets.len(), tr(lang, "item(s)", "件"), dest.display());
            let mut lines = vec![head, String::new()];
            for p in targets.iter().take(8) { lines.push(format!("  {}", p.display())); }
            if targets.len() > 8 {
                lines.push(tr_count(lang, targets.len() - 8));
            }
            let foot = if targets.len() == 1 {
                tr(lang, " y/Enter=Yes  a=overwrite  r=rename  n/Esc=cancel ",
                    " y/Enter=実行  a=上書き  r=改名  n/Esc=取消 ")
            } else {
                tr(lang, " y/Enter=Yes(skip)  a=overwrite  n/Esc=cancel ",
                    " y/Enter=実行(重複はスキップ)  a=上書き  n/Esc=取消 ")
            };
            (title, lines, foot.to_string())
        }
        Popup::TextInput { title, prompt, buffer, kind, cursor } => {
            // The caret is drawn where editing will happen; a password is shown
            // as dots so it does not sit in plain sight.
            let body = vec![prompt.clone(), field_with_caret(buffer, *cursor, kind.is_secret())];
            let foot = tr(lang, " Enter=ok  ←→ move  Esc=cancel ", " Enter=決定  ←→ 移動  Esc=取消 ");
            (format!(" {} ", title), body, foot.to_string())
        }
        Popup::Notice { lines } => {
            let title = tr(lang, " notice ", " お知らせ ").to_string();
            let foot = tr(lang, " y = copy   Enter / Esc = close ", " y = コピー   Enter / Esc = 閉じる ");
            (title, lines.clone(), foot.to_string())
        }
        Popup::Search { buffer } => {
            (
                tr(lang, " search ", " 検索 ").to_string(),
                vec![
                    tr(lang, "find (substring, case-insensitive):", "検索（部分一致・大小無視）:").to_string(),
                    format!("/{}_", buffer),
                ],
                tr(lang, " ↑↓ step matches  Enter=jump  Esc=cancel  (then n/N) ",
                    " ↑↓ マッチ移動  Enter=ジャンプ  Esc=取消  (後で n/N) ").to_string(),
            )
        }
        Popup::ConfirmQuit => {
            (
                tr(lang, " quit cian? ", " cian を終了？ ").to_string(),
                vec![tr(lang, "Are you sure you want to quit?", "本当に終了しますか？").to_string()],
                tr(lang, " y / Enter = yes   n / Esc = no ", " y / Enter = はい   n / Esc = いいえ ").to_string(),
            )
        }
        Popup::ConfirmClose { target } => {
            let what = match (target, lang) {
                (CloseTarget::ShellPane, Lang::Ja) => "このシェルペイン",
                (CloseTarget::FileTab(_), Lang::Ja) => "このタブ",
                (CloseTarget::ShellPane, Lang::En) => "this shell pane",
                (CloseTarget::FileTab(_), Lang::En) => "this tab",
            };
            let head = if lang == Lang::Ja { format!("{}を閉じますか？", what) } else { format!("Close {}?", what) };
            (
                tr(lang, " close? ", " 閉じる？ ").to_string(),
                vec![head],
                tr(lang, " y / Enter = yes   n / Esc = no ", " y / Enter = はい   n / Esc = いいえ ").to_string(),
            )
        }
        Popup::AiShellConfirm { command } => {
            (
                tr(lang, " AI command ", " AI コマンド ").to_string(),
                vec![
                    tr(lang, "Insert this command at the shell prompt?", "このコマンドをシェルのプロンプトに入力しますか？").to_string(),
                    String::new(),
                    format!("  {}", command),
                ],
                tr(lang, " y/Enter = insert   n/Esc = cancel ", " y/Enter = 入力   n/Esc = 取消 ").to_string(),
            )
        }
        Popup::ConfirmDiscard { targets, .. } => {
            let head = if lang == Lang::Ja {
                format!("{} 件の変更を破棄（元に戻す）:", targets.len())
            } else {
                format!("discard changes to {} path(s):", targets.len())
            };
            let mut lines = vec![
                head,
                String::new(),
            ];
            for p in targets.iter().take(8) {
                lines.push(format!("  {}", p.display()));
            }
            if targets.len() > 8 {
                lines.push(tr_count(lang, targets.len() - 8));
            }
            lines.push(String::new());
            lines.push(tr(lang,
                "This throws away uncommitted changes and cannot be undone.",
                "コミットしていない変更は失われ、元に戻せません。").to_string());
            (
                tr(lang, " discard changes ", " 変更を破棄 ").to_string(),
                lines,
                tr(lang, " y/Enter = discard   n/Esc = cancel ", " y/Enter = 破棄   n/Esc = 取消 ").to_string(),
            )
        }
        Popup::ConfirmElevate { op, targets, dest } => {
            let verb = match (op, lang) {
                (PendingOp::Copy, Lang::Ja) => "コピー",
                (PendingOp::Move, Lang::Ja) => "移動",
                (PendingOp::Copy, Lang::En) => "copy",
                (PendingOp::Move, Lang::En) => "move",
            };
            let body = if lang == Lang::Ja {
                vec![
                    format!("{} への書き込みには管理者権限が必要です", dest.display()),
                    String::new(),
                    format!("{} 件の{}を昇格して再試行しますか？ UACの確認が出ます。", targets.len(), verb),
                ]
            } else {
                vec![
                    format!("{} needs administrator rights to write to", dest.display()),
                    String::new(),
                    format!("Retry the {} of {} item(s) elevated? A UAC prompt will appear.", verb, targets.len()),
                ]
            };
            (
                tr(lang, " administrator rights ", " 管理者権限 ").to_string(),
                body,
                tr(lang, " y/Enter = retry as admin   n/Esc = cancel ",
                    " y/Enter = 管理者として再試行   n/Esc = 取消 ").to_string(),
            )
        }
        // All handled above, before this match.
        Popup::Manual { .. }
        | Popup::ContextMenu { .. }
        | Popup::ColorPicker { .. }
        | Popup::SortPicker { .. }
        | Popup::Macros { .. }
        | Popup::EncodingPicker { .. }
        | Popup::SshHosts { .. }
        | Popup::SshUsers { .. }
        | Popup::Shortcuts { .. }
        | Popup::History { .. }
        | Popup::FindResults { .. }
        | Popup::DestPicker { .. }
        | Popup::Viewer { .. }
        | Popup::Diff { .. }
        | Popup::DirCompare { .. }
        | Popup::Archive { .. }
        | Popup::AiChat { .. }
        | Popup::ImageView { .. }
        | Popup::CommitMessage { .. }
        | Popup::JunkReview { .. }
        | Popup::StructureReview { .. }
        | Popup::RenameReview { .. }
        | Popup::DupeReview { .. }
        | Popup::None => return,
    };

    let height = (body.len() as u16 + 4).max(6).min(area.height.saturating_sub(2));
    let width: u16 = 70u16.min(area.width.saturating_sub(2));
    let rect = centered_rect(width, height, area);

    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(theme().popup_bg))
        .title(title);
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);

    // Clickable buttons for the dialogs. Each stands in for the key it mirrors,
    // so the keyboard shortcuts in the footer keep working unchanged.
    let buttons: Vec<(&str, ZoneKind)> = match popup {
        Popup::ConfirmDelete { .. } => vec![
            (tr(lang, "Trash", "ゴミ箱"), ZoneKind::Enter),
            (tr(lang, "Delete!", "完全削除"), ZoneKind::Char('a')),
            (tr(lang, "Cancel", "取消"), ZoneKind::Esc),
        ],
        Popup::ConfirmTransfer { targets, .. } => {
            let mut b = vec![
                (tr(lang, "Yes", "実行"), ZoneKind::Enter),
                (tr(lang, "Overwrite", "上書き"), ZoneKind::Char('a')),
            ];
            if targets.len() == 1 {
                b.push((tr(lang, "Rename", "改名"), ZoneKind::Char('r')));
            }
            b.push((tr(lang, "Cancel", "取消"), ZoneKind::Esc));
            b
        }
        Popup::Notice { .. } => vec![
            (tr(lang, "Copy", "コピー"), ZoneKind::Char('y')),
            (tr(lang, "Close", "閉じる"), ZoneKind::Enter),
        ],
        Popup::TextInput { .. } => vec![
            (tr(lang, "OK", "決定"), ZoneKind::Enter),
            (tr(lang, "Cancel", "取消"), ZoneKind::Esc),
        ],
        Popup::Search { .. } => vec![
            (tr(lang, "Jump", "ジャンプ"), ZoneKind::Enter),
            (tr(lang, "Cancel", "取消"), ZoneKind::Esc),
        ],
        Popup::ConfirmQuit | Popup::ConfirmClose { .. } => vec![
            (tr(lang, "Yes", "はい"), ZoneKind::Enter),
            (tr(lang, "No", "いいえ"), ZoneKind::Esc),
        ],
        Popup::ConfirmElevate { .. } => vec![
            (tr(lang, "Retry as admin", "管理者として再試行"), ZoneKind::Enter),
            (tr(lang, "Cancel", "取消"), ZoneKind::Esc),
        ],
        Popup::AiShellConfirm { .. } => vec![
            (tr(lang, "Insert", "入力"), ZoneKind::Enter),
            (tr(lang, "Cancel", "取消"), ZoneKind::Esc),
        ],
        Popup::ConfirmDiscard { .. } => vec![
            (tr(lang, "Discard", "破棄"), ZoneKind::Enter),
            (tr(lang, "Cancel", "取消"), ZoneKind::Esc),
        ],
        _ => vec![],
    };

    let body_text: Vec<Line> = body.into_iter().map(Line::from).collect();
    // A dialog gets a dedicated button row above the hint footer; everything
    // else keeps the single hint line.
    let button_row = !buttons.is_empty() && inner.height >= 3;
    let body_h = inner.height.saturating_sub(if button_row { 2 } else { 1 });
    let body_area = Rect::new(inner.x, inner.y, inner.width, body_h);
    let footer_area = Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);

    let p = Paragraph::new(body_text).wrap(Wrap { trim: false });
    f.render_widget(p, body_area);

    if button_row {
        let btn_area = Rect::new(inner.x, inner.y + inner.height.saturating_sub(2), inner.width, 1);
        let mut x = btn_area.x;
        for (label, kind) in &buttons {
            let text = format!("[ {} ]", label);
            let w = text.chars().count() as u16;
            if x + w > btn_area.x + btn_area.width {
                break;
            }
            let r = Rect::new(x, btn_area.y, w, 1);
            f.render_widget(
                Paragraph::new(text).style(
                    Style::default()
                        .fg(theme().accent)
                        .add_modifier(Modifier::BOLD),
                ),
                r,
            );
            zones.push(PopupZone { rect: r, kind: *kind });
            x += w + 2; // a gap so adjacent buttons are visually distinct
        }
    }

    let footer_p = Paragraph::new(footer).style(
        Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
    );
    f.render_widget(footer_p, footer_area);
}
