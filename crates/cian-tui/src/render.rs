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

/// The local model's colour. Every "AI - simple" window — the chat, the prompts
/// it asks first, and the review lists its answers become — wears this cyan, so
/// a glance says the answer came from the model configured in `cian.ai`.
const AI_SIMPLE: Color = Color::Rgb(0, 190, 205);
/// crmaine's signature carmine, worn by the crmaine-backed chats (and by the
/// remote pane), so the two assistants never look like one.
const CRMAINE: Color = Color::Rgb(214, 45, 70);

/// True when this popup belongs to the AI - simple family, and so wears
/// [`AI_SIMPLE`] rather than the theme accent.
fn is_ai_simple(popup: &Popup) -> bool {
    match popup {
        Popup::AiChat { skin, .. } => skin.simple,
        Popup::AiShellConfirm { .. }
        | Popup::CommitMessage { .. }
        | Popup::JunkReview { .. }
        | Popup::StructureReview { .. } => true,
        // `:brename` and `:find` share their result lists with the AI; only the
        // AI side of each belongs to the family.
        Popup::RenameReview { by_ai, .. } | Popup::FindResults { by_ai, .. } => *by_ai,
        // The AI prompts; every other text input is a plain file operation.
        Popup::TextInput { kind, .. } => matches!(
            kind,
            InputKind::AiShellCmd | InputKind::AiRename | InputKind::AiSearch
        ),
        _ => false,
    }
}

/// The frame colour for a popup: cyan for the AI - simple family, the theme's
/// own accent for everything else.
fn popup_accent(popup: &Popup) -> Color {
    if is_ai_simple(popup) {
        AI_SIMPLE
    } else {
        theme().accent
    }
}

/// Normal three-surface layout: left/right file panes on top, shell below.
/// Apply a file pane's theme override (if any) to the active-theme global
/// before it draws, returning the palette to restore once it has. Per-pane
/// themes (#8) let the two columns wear different palettes; the swap is scoped
/// to that single `draw_file_pane` call so the shell and bars keep the app
/// theme. `side` is 0 = left, 1 = right.
/// Returns `Some(previous theme)` only when this pane actually has an override
/// and the global was swapped — the caller restores it afterward. `None` means
/// the pane follows the app theme and the global was left untouched (so a frame
/// with no per-pane themes does no theme writes at all).
fn push_pane_theme(app: &App, side: usize) -> Option<ResolvedTheme> {
    let t = app.pane_theme[side].as_deref().and_then(theme_preset)?;
    let prev = theme();
    set_theme(t);
    Some(prev)
}

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
    let mut sort_rects = Vec::new();
    let mut crumb_rects = Vec::new();
    let mut nav_rects = Vec::new();
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
    let restore = push_pane_theme(app, 0);
    draw_file_pane(f, panes_split[0], &app.left, app.focused == FocusedPane::Left, visual_for_left, app.mode, bg_l, fl_l, FocusedPane::Left, &mut tab_rects, app.git_for(FocusedPane::Left), app.lang, &mut sort_rects, &mut crumb_rects, &mut nav_rects);
    if let Some(prev) = restore { set_theme(prev); }
    let restore = push_pane_theme(app, 1);
    draw_file_pane(f, panes_split[1], &app.right, app.focused == FocusedPane::Right, visual_for_right, app.mode, bg_r, fl_r, FocusedPane::Right, &mut tab_rects, app.git_for(FocusedPane::Right), app.lang, &mut sort_rects, &mut crumb_rects, &mut nav_rects);
    if let Some(prev) = restore { set_theme(prev); }
    // With preview on and a file pane focused, the shell panel's area shows
    // the file under the cursor instead; the PTY runs on underneath, and
    // focusing the shell (Shift+J / click) gets its pixels back.
    let log_border = recording_pulse(app.started.elapsed());
    if app.preview_on && app.focused != FocusedPane::Shell {
        draw_preview_panel(f, shell_area, app);
    } else {
        // draw_shell sizes each pane's PTY to its computed sub-rect.
        draw_shell(f, shell_area, &mut app.shell, app.focused == FocusedPane::Shell, &mut dividers, &mut leaves, ov, &mut tab_rects, log_border);
    }
    app.dividers = dividers;
    app.shell_leaves = leaves;
    app.tab_rects = tab_rects;
    app.sort_rects = sort_rects;
    app.crumb_rects = crumb_rects;
    app.nav_rects = nav_rects;
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
            let restore = push_pane_theme(app, 0);
            draw_file_pane(f, rect, &app.left, true, va, app.mode, bg, fl, FocusedPane::Left, &mut Vec::new(), app.git_for(FocusedPane::Left), app.lang, &mut Vec::new(), &mut Vec::new(), &mut Vec::new());
            if let Some(prev) = restore { set_theme(prev); }
        }
        FocusedPane::Right => {
            let (bg, fl) = (app.pane_bg[1], app.flash_level(FocusedPane::Right));
            let va = app.visual_anchor;
            let restore = push_pane_theme(app, 1);
            draw_file_pane(f, rect, &app.right, true, va, app.mode, bg, fl, FocusedPane::Right, &mut Vec::new(), app.git_for(FocusedPane::Right), app.lang, &mut Vec::new(), &mut Vec::new(), &mut Vec::new());
            if let Some(prev) = restore { set_theme(prev); }
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
    let mut sort_rects = Vec::new();
    let mut crumb_rects = Vec::new();
    let mut nav_rects = Vec::new();
    match app.focused {
        FocusedPane::Left => {
            rects.left = area;
            app.layout_rects = rects;
            let va = app.visual_anchor;
            let (bg, fl) = (app.pane_bg[0], app.flash_level(FocusedPane::Left));
            let restore = push_pane_theme(app, 0);
            draw_file_pane(f, area, &app.left, true, va, app.mode, bg, fl, FocusedPane::Left, &mut tab_rects, app.git_for(FocusedPane::Left), app.lang, &mut sort_rects, &mut crumb_rects, &mut nav_rects);
            if let Some(prev) = restore { set_theme(prev); }
        }
        FocusedPane::Right => {
            rects.right = area;
            app.layout_rects = rects;
            let va = app.visual_anchor;
            let (bg, fl) = (app.pane_bg[1], app.flash_level(FocusedPane::Right));
            let restore = push_pane_theme(app, 1);
            draw_file_pane(f, area, &app.right, true, va, app.mode, bg, fl, FocusedPane::Right, &mut tab_rects, app.git_for(FocusedPane::Right), app.lang, &mut sort_rects, &mut crumb_rects, &mut nav_rects);
            if let Some(prev) = restore { set_theme(prev); }
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
    app.sort_rects = sort_rects;
    app.crumb_rects = crumb_rects;
    app.nav_rects = nav_rects;
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

    if app.op_job.is_some() && !app.op_bar_hidden {
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
    if matches!(app.popup, Popup::AiHistory { .. }) {
        draw_ai_history(f, area, app);
        return;
    }
    if matches!(app.popup, Popup::Toggles { .. }) {
        draw_toggles(f, area, app);
        return;
    }
    if matches!(app.popup, Popup::OpQueue { .. }) {
        draw_op_queue(f, area, app);
        return;
    }
    // The image preview decodes to fit its box and caches by size, so it takes
    // `&mut app` too.
    if matches!(app.popup, Popup::ImageView { .. }) {
        draw_image(f, area, app);
        return;
    }
    // The F3 image popup closed: drop its protocol state, and wipe the
    // terminal once so the picture does not linger over whatever is now
    // underneath (see `App::full_clear`).
    if app.img_proto.take().is_some() {
        app.full_clear = true;
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
            app.menu_rect = context_menu_rect(items, *at, area, app.menu_lang);
        }
        // And the viewer's text body, so a drag maps to a line — plus the
        // line-number gutter width, so it maps to a char column too.
        if let Popup::Viewer { view, preview, blame, editing, shape, .. } = &app.popup {
            let inner_w = centered_rect(area.width.saturating_sub(4), area.height.saturating_sub(2), area)
                .inner(Margin { vertical: 1, horizontal: 2 })
                .width;
            let ow = shape.as_deref().map_or(0, |s| outline_width(inner_w, s.shown, s.items.len()));
            app.viewer_frame = viewer_frame_rect(area);
            app.viewer_rect = viewer_body_rect(area, ow);
            app.outline_rect = Rect::new(
                app.viewer_rect.x.saturating_sub(ow),
                app.viewer_rect.y,
                ow.saturating_sub(1),
                app.viewer_rect.height,
            );
            app.viewer_gutter = if !blame.is_empty() && !*preview && !*editing {
                BLAME_W as u16
            } else if !*preview && view.kind == cian_core::viewer::ViewKind::Text {
                let fold_col = u16::from(shape.as_deref().is_some_and(|s| !s.items.is_empty()));
                (format!("{}", view.lines.len()).len().max(3) + 1) as u16 + fold_col
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
        let menu_lang = app.menu_lang;
        let show_ws = app.show_ws;
        let ruler = app.show_ruler;
        app.popup_zones.clear();
        // The status line lives outside the viewer's border, on the very last
        // row, which is not where anyone is looking while reading a file. A
        // message raised by the viewer itself — "saved", "nothing to fold
        // here" — is shown on its own footer instead.
        let msg_for_viewer = app.message.clone().filter(|_| app.message_fresh);
        // Every open file's name, in order, with the one on screen back in its
        // place — the strip has to name them all, and the active one is not in
        // the list while it is being read.
        let names: Vec<String> = {
            let mut v: Vec<String> = app
                .viewer_tabs
                .iter()
                .map(|p| match p {
                    Popup::Viewer { title, .. } => title.clone(),
                    _ => String::new(),
                })
                .collect();
            if let Popup::Viewer { title, .. } = &app.popup {
                let at = app.viewer_tab_idx.min(v.len());
                v.insert(at, title.clone());
            }
            v
        };
        let mut tab_rects: Vec<(Rect, usize)> = Vec::new();
        // A menu — or a chat, or the theme gallery — opened *from* the viewer
        // is drawn on top of it, not instead of it. The file is what the
        // question is about; losing sight of it while answering is the wrong
        // way round.
        if let Some(behind) = app.viewer_return.take() {
            let mut behind = behind;
            if let Some(other) = app.viewer_split.take() {
                let (first, second) = split_viewer_areas(area, app.viewer_split_lr);
                let (mine, theirs) = if app.viewer_split_focus {
                    (second, first)
                } else {
                    (first, second)
                };
                let mut other = other;
                draw_viewer(f, theirs, &mut other, lang, (show_ws, ruler), None, (0, &[], &[]));
                draw_viewer(f, mine, &mut behind, lang, (show_ws, ruler), None, (0, &[], &[]));
                app.viewer_split = Some(other);
            } else {
                draw_viewer(f, area, &mut behind, lang, (show_ws, ruler), None, (0, &[], &[]));
            }
            app.viewer_return = Some(behind);
        }
        // A split viewer is two viewers. The half not in focus is drawn first
        // and dimmed, exactly as the unfocused file pane is, so which one the
        // keyboard is pointed at is never a guess.
        // Only while a viewer is what is on screen. A menu, a confirm dialog
        // or a chat is a different popup entirely, and letting the split
        // branch draw it meant drawing nothing at all — the dialog was there,
        // invisible, quietly taking the next Enter.
        if matches!(app.popup, Popup::Viewer { .. }) && app.viewer_split.is_some() {
            let other = app.viewer_split.take().expect("just checked");
            let full = area;
            let (first, second) = split_viewer_areas(area, app.viewer_split_lr);
            // Which half each file occupies is fixed; crossing over moves the
            // focus, not the files. Drawing the focused one always on the left
            // made the two look as though they had traded places.
            let (mine, theirs) = if app.viewer_split_focus {
                (second, first)
            } else {
                (first, second)
            };
            // Where each half ended up, so a click in the one the keyboard is
            // not on can cross to it.
            app.viewer_half_rects = [mine, theirs];
            let mut other = other;
            // Either buffer moved on: work the marks out again, so the
            // comparison keeps telling the truth while both are edited. This
            // is the whole reason for doing it in place rather than in a
            // window that would have gone stale the moment you typed.
            if app.viewer_diff.is_some() {
                let now = {
                    let f = |p: &Popup| match p {
                        Popup::Viewer { view, .. } => crate::content_key(&view.lines),
                        _ => 0,
                    };
                    (f(&app.popup), f(&other))
                };
                if app.viewer_diff.as_deref().is_some_and(|d| d.fp != now) {
                    app.viewer_split = Some(other);
                    app.recompute_viewer_diff();
                    other = app.viewer_split.take().expect("put back just above");
                }
            }
            let (dm, dt) = match app.viewer_diff.as_deref() {
                Some(d) => (d.mine.as_slice(), d.theirs.as_slice()),
                None => (&[][..], &[][..]),
            };
            draw_viewer(f, theirs, &mut other, lang, (show_ws, ruler), None, (0, &[], dt));
            f.render_widget(
                Block::default().style(Style::default().fg(Color::Rgb(90, 90, 105))),
                theirs,
            );
            app.viewer_tab_rects = draw_viewer(
                f,
                mine,
                &mut app.popup,
                lang,
                (show_ws, ruler),
                msg_for_viewer.as_deref(),
                (app.viewer_tab_idx, &names, dm),
            );
            app.viewer_split = Some(other);
            app.popup_zones.clear();
            // Everything the mouse needs, for the half the keyboard is on —
            // without this the clicks were being measured against a viewer
            // that filled the whole screen, which is not where anything was.
            let ow = if let Popup::Viewer { shape, .. } = &app.popup {
                let inner_w = viewer_frame_rect(mine)
                    .inner(Margin { vertical: 1, horizontal: 2 })
                    .width;
                shape.as_deref().map_or(0, |s| outline_width(inner_w, s.shown, s.items.len()))
            } else {
                0
            };
            app.viewer_frame = viewer_frame_rect(mine);
            app.viewer_rect = viewer_body_rect(mine, ow);
            app.outline_rect = Rect::new(
                app.viewer_rect.x.saturating_sub(ow),
                app.viewer_rect.y,
                ow.saturating_sub(1),
                app.viewer_rect.height,
            );
            let _ = full;
            return;
        }
        draw_popup(
            f,
            area,
            &mut app.popup,
            &app.config.ssh_hosts,
            &app.config.snippets,
            find_state,
            &dests,
            &mut app.popup_zones,
            lang,
            menu_lang,
            show_ws,
            ruler,
            msg_for_viewer,
            app.viewer_tab_idx,
            &names,
            &mut tab_rects,
        );
        app.viewer_tab_rects = tab_rects;
    } else {
        app.popup_zones.clear();
    }
    if !matches!(app.popup, Popup::Viewer { .. }) {
        app.viewer_tab_rects.clear();
    }
    if app.viewer_split.is_none() {
        app.viewer_half_rects = [Rect::new(0, 0, 0, 0); 2];
    }

    // A brief "starting up" splash while the AI probe runs — non-blocking (it
    // never intercepts input, and yields the moment a popup opens), just so the
    // first couple of seconds don't feel dead.
    if matches!(app.popup, Popup::None) && app.is_starting_up() {
        draw_startup_splash(f, area, app.startup_at.elapsed().as_millis());
    }
}

/// A centered, animated "starting up" card. Drawn over the UI; purely cosmetic.
fn draw_startup_splash(f: &mut Frame, area: Rect, elapsed_ms: u128) {
    const SPIN: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let frame = SPIN[((elapsed_ms / 90) % SPIN.len() as u128) as usize];
    let w = 34u16.min(area.width);
    let h = 5u16.min(area.height);
    let rect = centered_rect(w, h, area);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(theme().popup_bg));
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);
    let lines = vec![
        Line::from(vec![
            Span::styled(format!("{}  ", frame), Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)),
            Span::styled("cian", Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)),
            Span::styled("  starting up…", Style::default().fg(Color::Rgb(180, 180, 200))),
        ]),
        Line::from(Span::styled(
            "  checking AI helper (crmaine)…",
            Style::default().fg(Color::Rgb(140, 140, 165)).add_modifier(Modifier::ITALIC),
        )),
    ];
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
}

/// The viewer's text body rect, mirroring its renderer's geometry so a mouse
/// click maps to the right line.
/// The viewer's frame within `area` — the bordered box, whose top row carries
/// the title and the tab arrows.
pub(crate) fn viewer_frame_rect(area: Rect) -> Rect {
    centered_rect(area.width.saturating_sub(4), area.height.saturating_sub(2), area)
}

fn viewer_body_rect(area: Rect, outline_w: u16) -> Rect {
    let rect = viewer_frame_rect(area);
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    let body_h = inner.height.saturating_sub(1);
    Rect::new(inner.x + outline_w, inner.y, inner.width - outline_w, body_h)
}

/// How wide the outline column is, or 0 when it is not showing.
///
/// Shared by the renderer and the mouse handler: a click has to land on the
/// entry that was drawn there, and two copies of this arithmetic would drift.
pub(crate) fn outline_width(inner_w: u16, show: bool, items: usize) -> u16 {
    if show && items > 0 && inner_w >= 60 { 28u16.min(inner_w / 3) } else { 0 }
}

/// The source line a viewer line stands for.
///
/// In source mode they are the same number. In the Markdown preview they are
/// not, and everything that reads the *file* (the outline) has to be told
/// which of the two it is holding before it compares it with anything that
/// reads the *screen* (the cursor).
pub(crate) fn src_line(md_map: &[usize], line: usize) -> usize {
    md_map.get(line).copied().unwrap_or(line)
}

/// The viewer line showing source line `src` — the trip back.
///
/// A rendered block often opens with a blank line for spacing, and that blank
/// belongs to the same source line as the heading under it. Landing on the
/// blank is landing one line short of what was asked for, so the first line
/// with something on it wins.
pub(crate) fn disp_line(md_map: &[usize], lines: &[String], src: usize) -> usize {
    if md_map.is_empty() {
        return src;
    }
    let first = md_map.iter().position(|s| *s >= src).unwrap_or(md_map.len().saturating_sub(1));
    let mut i = first;
    while i + 1 < md_map.len()
        && md_map[i + 1] == md_map[first]
        && lines.get(i).is_some_and(|l| l.trim().is_empty())
    {
        i += 1;
    }
    i
}

/// The first outline entry drawn, given where the cursor is and how many rows
/// there is room for.
pub(crate) fn outline_top(items: &[cian_core::outline::Item], line: usize, h: usize) -> usize {
    match items.iter().rposition(|i| i.line <= line) {
        Some(i) if h > 0 && i >= h => i + 1 - h,
        _ => 0,
    }
}

/// The rect the context menu occupies, from its anchor and item count. Shared
/// by the renderer and the mouse handler so a click lands where the row is
/// drawn.
/// Split a menu label into (name, hint), where the hint is a trailing
/// `(…)`-style key/command annotation preceded by two spaces (e.g.
/// `"Bulk rename…  (:brename)"` → `("Bulk rename…", "(:brename)")`). No hint
/// yields an empty second element.
pub(crate) fn menu_label_parts(label: &str) -> (&str, &str) {
    if label.ends_with(')') {
        if let Some(pos) = label.rfind("  (") {
            return (label[..pos].trim_end(), &label[pos + 2..]);
        }
    }
    (label, "")
}

/// The widest name and widest hint across a menu's items — so names left-align
/// and hints right-align in a common column.
fn menu_dims(items: &[MenuItem], lang: Lang) -> (usize, usize) {
    let mut name_w = 0;
    let mut hint_w = 0;
    for i in items {
        let (n, h) = menu_label_parts(i.label(lang));
        name_w = name_w.max(width(n));
        hint_w = hint_w.max(width(h));
    }
    (name_w.max(6), hint_w)
}

/// A text-input field line with the cursor shown as a highlighted character
/// (reverse video), so moving the cursor never shifts the text. A password is
/// masked; a cursor at the end highlights a trailing space (a block cursor).
fn caret_line(buffer: &str, cursor: usize, secret: bool) -> Line<'static> {
    let shown: String = if secret { "•".repeat(buffer.chars().count()) } else { buffer.to_string() };
    let chars: Vec<char> = shown.chars().collect();
    let cur = cursor.min(chars.len());
    let before: String = chars[..cur].iter().collect();
    let at: String = chars.get(cur).map(|c| c.to_string()).unwrap_or_else(|| " ".to_string());
    let after: String = chars.get(cur + 1..).map(|s| s.iter().collect()).unwrap_or_default();
    Line::from(vec![
        Span::raw(">"),
        Span::raw(before),
        Span::styled(at, Style::default().fg(Color::Black).bg(theme().accent)),
        Span::raw(after),
    ])
}


fn context_menu_rect(items: &[MenuItem], at: (u16, u16), area: Rect, lang: Lang) -> Rect {
    // marker(2) + name + gap(2, if any hint) + hint + right gutter(2) + borders(2).
    let (name_w, hint_w) = menu_dims(items, lang);
    let hint_col = if hint_w > 0 { hint_w + 2 } else { 0 };
    let w = (2 + name_w + hint_col + 2 + 2) as u16;
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
) -> (Line<'a>, String) {
    fn label_for(i: usize, tab: &Pane, is_active: bool) -> String {
        // A remote pane shows "⇅ user@host:/path" so it reads as a server.
        if let Some((host, path)) = tab.remote_view() {
            return format!(" {} ⇅ {}:{} ", i + 1, host, path);
        }
        // Inside an archive: "⊞ report.zip/sub/" so the pane reads as a
        // place inside a file, not a directory.
        if let Some((arc, sub)) = tab.archive_view() {
            let name = arc.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            return format!(" {} ⊞ {}/{} ", i + 1, name, sub);
        }
        // A flat / search listing names the view (e.g. "⌥ branch", "⌥ grep: x")
        // rather than a directory, so it is obvious the pane is not a folder and
        // that `b` / Esc leaves it.
        if let Some(lbl) = tab.flat_label() {
            // …and says how, because "the pane is not a folder any more" is
            // easy to notice and "Esc puts it back" is not.
            return format!(" {} ⌥ {}  ⏎Esc/⌫ ", i + 1, lbl);
        }
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
    // Display cells, not characters: a Japanese directory name is two cells a
    // character, and the char count both overflowed the layout budget and
    // misplaced the click map.
    let width_of = |s: &str| width(s) as u16;

    // The two history arrows eat four cells at the head of the title, so the
    // tabs get that much less to lay out in. Forgetting this is how the long
    // path started being clipped at the right edge again.
    const NAV_W: u16 = 4;
    let max_width = max_width.saturating_sub(NAV_W);
    // First, lay out tabs starting from the active one outward so it never gets cut.
    let active = tabs.active.min(tabs.tabs.len().saturating_sub(1));
    let total = tabs.tabs.len();
    let mut shown: Vec<usize> = vec![active];
    // A long path is shortened from the middle — its tail is the part that
    // identifies it, and clipping at the border loses exactly that end.
    let active_label = truncate_middle(
        &label_for(active, &tabs.tabs[active], true),
        max_width.saturating_sub(2) as usize,
    );
    let mut used: u16 = width_of(&active_label);
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
    // Browser arrows, before the tabs: lit when there is somewhere to go.
    // Their rects are pushed by the caller, which knows the pane's origin.
    {
        let active = &tabs.tabs[tabs.active.min(tabs.tabs.len().saturating_sub(1))];
        let lit = Style::default().fg(theme().accent).add_modifier(Modifier::BOLD);
        let out = Style::default().fg(theme().dim);
        spans.push(Span::styled(
            "◀",
            if active.history.is_empty() { out } else { lit },
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            "▶",
            if active.forward.is_empty() { out } else { lit },
        ));
        spans.push(Span::raw(" "));
        col += NAV_W;
    }
    if hidden_left > 0 {
        let s = format!("+{} ", hidden_left);
        col += width_of(&s);
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
        let label = if is_active {
            active_label.clone()
        } else {
            label_for(i, &tabs.tabs[i], is_active)
        };
        let w = width_of(&label);
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
    (Line::from(spans), active_label)
}

/// Pick a Nerd Font glyph based on the entry name/extension.
pub(crate) fn icon_for(entry: &cian_core::Entry) -> &'static str {
    // Without a Nerd Font, drop the icons entirely — directory colour still
    // marks folders, and no glyph mojibakes on a plain terminal.
    if !crate::theme::nerd_fonts() {
        return "";
    }
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
        let p = theme().file;
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
    lang: Lang,
    sort_rects: &mut Vec<(FocusedPane, cian_core::SortKey, Rect)>,
    crumb_rects: &mut Vec<(FocusedPane, usize, Rect)>,
    nav_rects: &mut Vec<(FocusedPane, bool, Rect)>,
) {
    // Read the active theme once — `theme()` now takes a lock, and the row loop
    // below would otherwise hit it thousands of times per frame.
    let th = theme();
    let focus_bg = focus_badge_color(mode);
    let bg = bg.or(th.base_bg);
    let mut border_style = if focused {
        Style::default().fg(focus_bg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(th.border)
    };
    // An operation that just landed here lights the border, fading out.
    if flash > 0.0 {
        border_style = Style::default().fg(fade(th.accent, flash)).add_modifier(Modifier::BOLD);
    }
    // A remote (SFTP) pane wears a carmine frame, so "this is a server, not the
    // local disk" is unmistakable regardless of focus.
    if tabs.active_ref().is_remote() {
        border_style = Style::default().fg(CRMAINE).add_modifier(Modifier::BOLD);
    }
    let max_title_w = area.width.saturating_sub(2);
    let mut offsets = Vec::new();
    let (title, active_title) = tabs_title(tabs, focused, focus_bg, max_title_w, &mut offsets);
    // The two history arrows sit at columns 1 and 3 of the title.
    nav_rects.push((pane_id, false, Rect::new(area.x + 2, area.y, 1, 1)));
    nav_rects.push((pane_id, true, Rect::new(area.x + 4, area.y, 1, 1)));
    // The title is drawn on the top border row, one cell in from the corner.
    for (i, off, w) in &offsets {
        tab_rects.push((pane_id, *i, Rect::new(area.x + 1 + off, area.y, *w, 1)));
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
    // Likewise a ☁ column, only where a sync client actually put placeholders:
    // an ordinary folder never pays a cell for it.
    let cloud_w: u16 = if pane.has_cloud() { 2 } else { 0 };
    let meta_w = if show_time { SIZE_COL_W + TIME_COL_W + 2 } else if show_size { SIZE_COL_W + 1 } else { 0 };
    // 2 mark + icon + 2 spaces
    let name_w = inner_w.saturating_sub(meta_w + 5 + git_w + cloud_w) as usize;

    // Build ListItems only for the rows the viewport can actually show. ratatui
    // renders a fresh `ListState` (offset 0) by scrolling just enough to keep the
    // selected row visible, which for uniform 1-row items lands the window at
    // `[cursor+1-height, cursor+1)`. Replicating that here turns per-frame work
    // from O(entries) into O(visible) — the difference between a snappy and a
    // sluggish pane on a directory with thousands of files.
    let total = pane.entries.len();
    // Borders top+bottom, plus the column-header row under the top border.
    let list_h = area.height.saturating_sub(3) as usize;
    let start = if list_h == 0 { pane.cursor } else { pane.cursor.saturating_sub(list_h - 1) };
    let end = start.saturating_add(list_h).min(total);
    let mark_style = Style::default().fg(th.mark_fg).add_modifier(Modifier::BOLD);
    let meta_style = Style::default().fg(th.dim);

    let items: Vec<ListItem> = pane.entries[start..end].iter().enumerate().map(|(vi, e)| {
        let i = start + vi; // absolute index for marks / visual range / git
        let marked = pane.is_marked(i);
        let in_visual = visual_range.map(|(a, b)| i >= a && i <= b).unwrap_or(false);
        let mark_symbol = if marked { "● " } else { "  " };
        let kind = kind_for(e);
        let kind_color = kind.color();
        let mut name_style = Style::default().fg(kind_color);
        if kind.bold() {
            name_style = name_style.add_modifier(Modifier::BOLD);
        }
        // The icon carries the same color so the row reads as one unit.
        let icon_style = Style::default().fg(kind_color);

        let name = fit(&e.name, name_w);
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
        if cloud_w > 0 {
            // ☁ = listed but not downloaded; a space keeps local files aligned.
            spans.push(Span::styled(
                if e.cloud { "☁ " } else { "  " },
                Style::default().fg(Color::Rgb(130, 175, 210)),
            ));
        }
        spans.extend([
            Span::styled(mark_symbol, mark_style),
            Span::styled(format!("{}  ", icon_for(e)), icon_style),
            Span::styled(name, name_style),
        ]);
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
        if in_visual { item = item.style(Style::default().bg(th.visual_bg)); }
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
    // The frame and the rows render separately so the column-header row can
    // sit between them: block on the full area, header on the first inner
    // row, the list below it.
    f.render_widget(block, area);
    let inner = area.inner(Margin { vertical: 1, horizontal: 1 });
    if inner.height > 0 {
        let header = Rect::new(inner.x, inner.y, inner.width, 1);
        draw_pane_header(
            f, header, pane, git_w, cloud_w, name_w, show_size, show_time, list_style, pane_id,
            lang, sort_rects,
        );
    }
    let list_area =
        Rect::new(inner.x, inner.y + 1, inner.width, inner.height.saturating_sub(1));
    let list = List::new(items)
        .style(list_style)
        .highlight_style(
            Style::default().bg(th.selected_bg).add_modifier(Modifier::BOLD),
        );

    // The items are already the visible slice, so the selection is addressed
    // relative to `start` and the state's own offset stays at 0.
    let mut state = ListState::default();
    if !pane.entries.is_empty() { state.select(Some(pane.cursor - start)); }
    f.render_stateful_widget(list, list_area, &mut state);

    draw_list_scrollbar(f, area, pane.entries.len(), pane.cursor, focused, border_style);

    // The active tab's path segments are click targets (a breadcrumb): the
    // rects live on the title row and are resolved before tab selection.
    if let Some((ix, tab_col, _)) =
        offsets.iter().copied().find(|(i, _, _)| *i == tabs.active)
    {
        // Labels start one cell in from the corner, like the tab rects above.
        push_breadcrumb_rects(&active_title, ix, area, tab_col + 1, pane, pane_id, crumb_rects);
    }
}

/// The column-header row: `Name`, `Size`, `Date` over their columns, the
/// active sort key carrying a direction arrow. Each label's rect is pushed to
/// `sort_rects` so a click sorts by that column (repeat flips the direction,
/// as column headers behave everywhere else — `apply_sort_key` does the flip).
#[allow(clippy::too_many_arguments)]
fn draw_pane_header(
    f: &mut Frame,
    header: Rect,
    pane: &Pane,
    git_w: u16,
    cloud_w: u16,
    name_w: usize,
    show_size: bool,
    show_time: bool,
    base: Style,
    pane_id: FocusedPane,
    lang: Lang,
    sort_rects: &mut Vec<(FocusedPane, cian_core::SortKey, Rect)>,
) {
    use cian_core::SortKey;
    let style = base.fg(theme().dim);
    let label = |key: SortKey| -> String {
        let name = match key {
            SortKey::Name => tr(lang, "Name", "名前"),
            SortKey::Size => tr(lang, "Size", "サイズ"),
            SortKey::Modified => tr(lang, "Date", "日時"),
            SortKey::Extension => "",
        };
        if pane.sort.key == key {
            format!("{} {}", name, if pane.sort.reverse { "▼" } else { "▲" })
        } else {
            name.to_string()
        }
    };
    // Mirror the row layout: git badge, mark, icon columns, then the fields.
    // The icon column exists only with Nerd Fonts on (one cell + two spaces;
    // two spaces alone otherwise).
    let prefix = 4 + usize::from(nerd_fonts()) + git_w as usize + cloud_w as usize;
    let name_lbl = label(SortKey::Name);
    let size_lbl = label(SortKey::Size);
    let time_lbl = label(SortKey::Modified);
    let mut text = format!("{}{}", " ".repeat(prefix), pad_to(&name_lbl, name_w));
    if show_size {
        text.push_str(&pad_left(&size_lbl, SIZE_COL_W as usize + 1));
    }
    if show_time {
        text.push_str(&format!(" {}", time_lbl));
    }
    f.render_widget(Paragraph::new(text).style(style), header);

    // Click zones, in the same geometry the text was laid out in.
    let x = header.x + prefix as u16;
    sort_rects.push((pane_id, SortKey::Name, Rect::new(x, header.y, width(&name_lbl) as u16, 1)));
    if show_size {
        let sx = header.x + (prefix + name_w) as u16;
        sort_rects.push((pane_id, SortKey::Size, Rect::new(sx, header.y, SIZE_COL_W + 1, 1)));
    }
    if show_time {
        let tx = header.x + (prefix + name_w + SIZE_COL_W as usize + 2) as u16;
        sort_rects.push((
            pane_id,
            SortKey::Modified,
            Rect::new(tx, header.y, width(&time_lbl) as u16, 1),
        ));
    }
}

/// Map the displayed path segments of the active tab to click rects, counted
/// from the path's end. Counting from the end keeps the mapping exact even
/// when the head was middle-truncated: the tail of `truncate_middle` is
/// verbatim, so everything right of the `…` is trustworthy — and the segment
/// holding the `…` itself is ambiguous, so it gets no rect.
fn push_breadcrumb_rects(
    label: &str,
    active_ix: usize,
    area: Rect,
    tab_col: u16,
    pane: &Pane,
    pane_id: FocusedPane,
    crumb_rects: &mut Vec<(FocusedPane, usize, Rect)>,
) {
    // Only a plain directory listing has a browsable path.
    if pane.remote_view().is_some() || pane.flat_label().is_some() || pane.archive_view().is_some() {
        return;
    }
    // The label opens with " N " (the tab number) — that part is a tab click,
    // not a path segment, so parsing starts after it.
    let prefix = format!(" {} ", active_ix + 1);
    let Some(path_part) = label.strip_prefix(&prefix) else { return };
    let mut col = width(&prefix); // display cells from the label start
    let mut seg_start = col;
    let mut segs: Vec<(usize, usize, bool)> = Vec::new(); // (start, end, clean)
    let mut clean = true; // no `…` seen inside this segment
    for ch in path_part.chars() {
        let w = width(&ch.to_string());
        if ch == '/' || ch == '\\' {
            if col > seg_start {
                segs.push((seg_start, col, clean));
            }
            clean = true;
            seg_start = col + w;
        } else if ch == '…' {
            clean = false;
        }
        col += w;
    }
    if col > seg_start {
        segs.push((seg_start, col, clean));
    }
    // The label's trailing " " rides along in the last segment; harmless.
    let n = segs.len();
    for (i, (s, e, clean)) in segs.into_iter().enumerate() {
        if !clean {
            continue;
        }
        // Segments count up from the end: the last is the cwd itself (0 to
        // strip), the one before it 1, and so on.
        let strip = n - 1 - i;
        let x = area.x + tab_col + s as u16;
        crumb_rects.push((pane_id, strip, Rect::new(x, area.y, (e - s) as u16, 1)));
    }
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
    // Two rows in: the top border and the column-header row.
    let view_h = area.height.saturating_sub(3);
    if view_h == 0 || total <= view_h as usize {
        return;
    }
    let track = Rect::new(area.x + area.width.saturating_sub(1), area.y + 2, 1, view_h);
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
    let broadcast = shell.is_broadcasting();
    let sync_members = shell.sync_members.clone();
    let tab = &shell.tabs[active];
    render_node(f, tab, active, root, inner, tab.active, focused, false, dividers, leaves, ov, log_border, broadcast, &sync_members);
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
    broadcast: bool,
    sync_members: &std::collections::BTreeSet<usize>,
) {
    match tab.nodes.get(i).and_then(|n| n.as_ref()) {
        Some(Node::Leaf { session, bg }) => {
            let target = if bordered {
                let is_active = focused && i == active_leaf;
                // A pane is a live sync target when broadcast is on AND either the
                // member set is empty (all panes) or it lists this leaf.
                let sync_here = broadcast && (sync_members.is_empty() || sync_members.contains(&i));
                // Broadcast/synchronize is the loudest state (input hits every
                // pane), so it wins the border colour — a bright amber with a
                // `⇄` badge on each pane it targets.
                let bs = if sync_here {
                    Style::default().fg(Color::Rgb(255, 176, 32)).add_modifier(Modifier::BOLD)
                } else if session.is_logging() {
                    Style::default().fg(log_border).add_modifier(Modifier::BOLD)
                } else if is_active {
                    Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let mut blk = Block::default().borders(Borders::ALL)
        .border_type(border_type()).border_style(bs);
                if sync_here {
                    // Show the group size (n/total) only when it is a real subset.
                    let title = if sync_members.is_empty() {
                        " ⇄ SYNC ".to_string()
                    } else {
                        let all = tab.leaves();
                        let live = all.iter().filter(|l| sync_members.contains(l)).count();
                        format!(" ⇄ SYNC {}/{} ", live, all.len())
                    };
                    blk = blk.title(title);
                }
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
            render_node(f, tab, tab_idx, *first, rects.0, active_leaf, focused, true, dividers, leaves, ov, log_border, broadcast, sync_members);
            render_node(f, tab, tab_idx, *second, rects.1, active_leaf, focused, true, dividers, leaves, ov, log_border, broadcast, sync_members);
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

/// Colour a status message by what kind of news it carries, so a failure never
/// wears the same clothes as a success. Classified from the text itself —
/// messages come from a hundred call sites and most already start with a glyph
/// (✔/⚠) or contain an unambiguous failure word; the rest stay accent-neutral.
pub(crate) fn message_color(msg: &str) -> Color {
    const GOOD: Color = Color::Rgb(110, 200, 130);
    const WARN: Color = Color::Rgb(235, 200, 100);
    const BAD: Color = Color::Rgb(235, 110, 110);
    if msg.starts_with('✔') || msg.starts_with("saved") || msg.starts_with("copied")
        || msg.starts_with("renamed") || msg.starts_with("created")
    {
        return GOOD;
    }
    if msg.starts_with('⚠') || msg.contains("cancelled") || msg.contains("中止")
        || msg.contains("unsaved") || msg.contains("未保存")
    {
        return WARN;
    }
    let lower = msg.to_lowercase();
    if lower.contains("fail") || lower.contains("error") || lower.contains("cannot")
        || lower.contains("not found") || lower.contains("denied")
        || msg.contains("できません") || msg.contains("失敗") || msg.contains("ありません")
    {
        return BAD;
    }
    theme().accent
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
        // When the last output looks like an error, nudge toward asking Carmine
        // to explain it — the action lives at the top of the shell menu
        // (Shift+Enter), which works everywhere a modifier-combo might not.
        if app.shell_error_detected() {
            v.push(("⚠ S-Enter", d("explain error", "エラーを説明")));
        }
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
        // A flat / search listing is a mode of its own: the one thing that must
        // be obvious is how to get out of it, then that marks and file ops work
        // on the results just like a normal listing.
        // Inside an archive the keys mean archive things — and there is
        // nothing else on screen to say so, which is exactly when the bar
        // earns its row.
        _ if app.active_pane().map(|p| p.archive_view().is_some()).unwrap_or(false) => {
            let mut v = vec![
                ("Enter/l", d("in", "入る")),
                ("-/h", d("out", "戻る")),
                ("F3", d("view member", "メンバー閲覧")),
                ("Space", d("mark", "マーク")),
                ("c", d("extract →", "展開 →")),
            ];
            // The write half exists for zip only; saying so beats a key that
            // answers "read-only for now".
            let zip = app
                .active_pane()
                .and_then(|p| p.archive_view())
                .map(|(a, _)| {
                    a.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.eq_ignore_ascii_case("zip") || e.eq_ignore_ascii_case("jar"))
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            if zip {
                v.extend([("F2", d("rename", "リネーム")), ("d", d("delete", "削除"))]);
            } else {
                v.push(("", d("(read-only)", "（読取専用）")));
            }
            v.push(("?", d("help", "ヘルプ")));
            v
        }
        _ if app.active_pane().map(|p| p.is_flat()).unwrap_or(false) => vec![
            ("b/Esc", d("leave", "戻る")),
            ("Space", d("mark", "マーク")),
            ("/", d("filter", "絞込")),
            ("Enter", d("open", "開く")),
            ("F3", d("view", "閲覧")),
            ("?", d("help", "ヘルプ")),
        ],
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
            ("b", d("branch", "ブランチ")),
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
    // The badge names the mode once it leaves Normal (helix-style): the pane
    // letter alone said *where* keys go but not *what* they currently mean.
    let mode_word = match app.mode {
        Mode::Normal | Mode::Shell => "",
        Mode::Visual => " VISUAL",
        Mode::Search => " SEARCH",
        Mode::Command => " CMD",
        Mode::Filter => " FILTER",
    };
    let mut spans: Vec<Span> = vec![
        Span::styled(
            format!(" {}{} ", focus_label, mode_word),
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
        let branch_glyph = if nerd_fonts() { "\u{e0a0} " } else { "" };
        let mut label = format!("{}{}", branch_glyph, git.branch);
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

    // Free space on the active pane's mount — always in view, since a copy or
    // an extract of a huge tree is a glance away from "will this fit". Amber
    // past 80% used, red past 95%, so a filling disk announces itself.
    if let Some(u) = app.disk_for(app.focused) {
        spans.push(dim_sep.clone());
        let frac = u.used_fraction();
        let color = if frac >= 0.95 {
            Color::Rgb(230, 110, 110)
        } else if frac >= 0.80 {
            Color::Rgb(240, 210, 120)
        } else {
            Color::Rgb(130, 175, 210)
        };
        let label = format!(
            "{}{} free / {}",
            if nerd_fonts() { "\u{f0a0} " } else { "" },
            cian_core::disk::human_size(u.free),
            cian_core::disk::human_size(u.total),
        );
        spans.push(chip(label, color));
    }

    if app.zoomed {
        spans.push(dim_sep.clone());
        spans.push(chip("[zoom]".to_string(), theme().accent));
    }

    // A running operation keeps a chip here — the whole story once the
    // progress popup is tucked away, a heartbeat even while it shows.
    if let Some(job) = &app.op_job {
        spans.push(dim_sep.clone());
        let p = &job.latest;
        let pct = if let Some(f) = (p.bytes_done * 100).checked_div(p.bytes_total) {
            format!(" {}%", f.min(100))
        } else if p.files_total > 0 {
            format!(" {}/{}", p.files_done, p.files_total)
        } else {
            String::new()
        };
        let queued = if app.op_queue.is_empty() {
            String::new()
        } else {
            format!(" +{}", app.op_queue.len())
        };
        if app.op_stalled() {
            let secs = job.last_progress.elapsed().as_secs();
            spans.push(chip(
                format!("⚠ {}{} — stalled {}s{}", job.label, pct, secs, queued),
                Color::Rgb(235, 200, 100),
            ));
        } else {
            spans.push(chip(format!("⏳ {}{}{}", job.label, pct, queued), theme().accent));
        }
    }

    let has_msg = app.message.as_ref().is_some_and(|m| !m.is_empty());
    if let Some(msg) = app.message.as_ref() {
        if !msg.is_empty() {
            spans.push(dim_sep.clone());
            spans.push(Span::styled(
                format!("◂ {}", msg),
                Style::default()
                    .fg(message_color(msg))
                    .bg(theme().status_bg)
                    .add_modifier(Modifier::ITALIC | Modifier::BOLD),
            ));
        }
    }

    // Everything on this row except the message is also on screen somewhere
    // else — the path is in the pane title, the branch in its header. The
    // message is the only thing here that is news, and it was last in the
    // queue for space: on a real terminal with a long path and a git chip it
    // was pushed off the right-hand edge and simply never seen. So when they
    // do not all fit, the chips give way, one at a time, from the left —
    // keeping the mode chip, which is what says whether a keystroke will be
    // read as a command.
    if has_msg {
        let total = |v: &[Span]| v.iter().map(|s| width(&s.content)).sum::<usize>();
        let room = area.width as usize;
        // The message is the last two spans (separator + text); never drop it.
        while total(&spans) > room && spans.len() > 3 {
            spans.remove(1);
        }
    }

    let line = Line::from(spans);
    let p = Paragraph::new(line).style(Style::default().bg(theme().status_bg));
    f.render_widget(p, area);

    // The active shell pane's title (its `user@host: cwd`), right-aligned so it
    // sits in the bottom-right and tracks whichever split/tab is active —
    // rather than staying on the first pane. Drawn as its own right-aligned
    // paragraph over the same row.
    // …but not over a message. The title is the same every frame; the message
    // is the answer to what was just pressed.
    if let Some(title) = app.shell.active_title().filter(|_| !has_msg) {
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
        Paragraph::new(tr(lang, " Esc = stop   b = background ", " Esc = 中止   b = バックグラウンドへ ")).style(
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
/// The image preview. On a terminal that answered the startup graphics query
/// (kitty / iTerm2 / sixel), the picture renders as real pixels; everywhere
/// else it falls back to half-block (`▀`) cells — top pixel the glyph's
/// foreground, bottom pixel its background — which any 24-bit terminal can
/// show. Both paths decode to fit and cache.
fn draw_image(f: &mut Frame, area: Rect, app: &mut App) {
    let lang = app.lang;
    let rect = centered_rect(area.width.saturating_sub(2), area.height.saturating_sub(2), area);
    f.render_widget(Clear, rect);
    let inner = rect.inner(Margin { vertical: 1, horizontal: 1 });
    let body_w = inner.width;
    let body_h = inner.height.saturating_sub(1); // leave a row for the footer

    if app.gfx_picker.is_some() {
        draw_image_gfx(f, rect, inner, app);
        return;
    }

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

/// The `:preview` panel, borrowing the shell's area: what the cursor is on,
/// rendered with the F3 assets — syntax colour for code, pixels for images
/// where the terminal can, listings for folders and archives.
fn draw_preview_panel(f: &mut Frame, area: Rect, app: &mut App) {
    let lang = app.lang;
    // Resolve what to show; a reason-not-to is shown as a note.
    let target = crate::preview::preview_target(app);
    let (title_name, note) = match &target {
        Ok(p) => {
            app.ensure_preview(p);
            (p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), None)
        }
        Err(e) => (String::new(), Some(e.clone())),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().border))
        .title(Line::from(vec![
            Span::styled(
                " ⌥ preview ",
                Style::default().fg(theme().accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(truncate_middle(&title_name, 48), Style::default().fg(theme().dim)),
            Span::raw(" "),
        ]))
        .title_bottom(tr(
            lang,
            " :preview off   Shift+J = shell ",
            " :preview で解除   シェルは Shift+J ",
        ));
    let inner = area.inner(Margin { vertical: 1, horizontal: 1 });
    // Wipe the panel before drawing into it. A `Paragraph` writes only the
    // characters it has, and a `Block`'s style recolours cells without
    // replacing them — so the tail of a longer previous file stayed on screen
    // underneath the shorter new one, and the two read as one garbled
    // document. The preview changes contents on every cursor move, which is
    // the worst case for leaving anything behind.
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default().style(Style::default().bg(surface())),
        area,
    );
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    // Two rows is the floor for anything readable. Below it, say why rather
    // than drawing a one-line sliver that looks like a failure.
    if inner.height < 2 {
        f.render_widget(
            Paragraph::new(tr(lang, "(drag the border down for a preview)", "（境界線を下げるとプレビューが出ます）"))
                .style(Style::default().fg(theme().dim)),
            inner,
        );
        return;
    }

    if let Some(msg) = note {
        // A note can be several lines (the cloud explanation is), so it wraps
        // rather than being clipped to the first row.
        f.render_widget(
            Paragraph::new(msg)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(theme().dim)),
            inner,
        );
        return;
    }

    // Image: pixels when the terminal can, half-blocks otherwise. Cached like
    // the F3 popup, but in preview-owned state.
    if matches!(app.preview.as_ref().map(|p| &p.body), Some(crate::preview::PreviewBody::Image)) {
        let path = app.preview.as_ref().map(|p| p.path.clone()).unwrap_or_default();
        if app.gfx_picker.is_some() {
            if app.preview_gfx.as_ref().map(|(p, _)| p != &path).unwrap_or(true) {
                app.preview_gfx = None;
                if let (Ok(img), Some(picker)) = (image::open(&path), app.gfx_picker.as_ref()) {
                    app.preview_gfx = Some((path.clone(), picker.new_resize_protocol(img)));
                }
            }
            if let Some((_, proto)) = app.preview_gfx.as_mut() {
                f.render_stateful_widget(ratatui_image::StatefulImage::default(), inner, proto);
                return;
            }
        }
        let mut drew = false;
        if let Some(state) = app.preview.as_mut() {
            if state.thumb.as_ref().map(|(c, r, _)| (*c, *r)) != Some((inner.width, inner.height)) {
                state.thumb = cian_core::image::thumbnail(&path, inner.width, inner.height)
                    .ok()
                    .map(|t| (inner.width, inner.height, t));
            }
            if let Some((_, _, t)) = &state.thumb {
                drew = true;
                let mut rows: Vec<Line> = Vec::new();
                for ry in 0..t.rows as usize {
                    let mut spans = Vec::with_capacity(t.cols as usize);
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
                let left = inner.x + (inner.width.saturating_sub(t.cols)) / 2;
                let pic = Rect::new(left, inner.y, t.cols.min(inner.width), (t.rows).min(inner.height));
                f.render_widget(Paragraph::new(rows), pic);
            }
        }
        // Never leave the panel blank: an empty box reads as "the feature is
        // broken", when the honest answer is that this image could not be
        // decoded (or the panel has no room for it).
        if !drew {
            f.render_widget(
                Paragraph::new(tr(lang, "(cannot render this image here)", "（この画像はここに描画できません）"))
                    .style(Style::default().fg(theme().dim)),
                inner,
            );
        }
        return;
    }

    let Some(state) = app.preview.as_ref() else { return };
    let body_fg = readable_on(theme().base_bg.unwrap_or(Color::Black));
    match &state.body {
        crate::preview::PreviewBody::Text { lines, hl } => {
            let mut shown: Vec<Line> = Vec::with_capacity(inner.height as usize);
            for (i, l) in lines.iter().take(inner.height as usize).enumerate() {
                let clipped = truncate(l, inner.width as usize);
                match hl.get(i) {
                    Some(cats) if !cats.is_empty() => {
                        let spans: Vec<Span> = clipped
                            .chars()
                            .enumerate()
                            .map(|(ci, ch)| {
                                let style = cats
                                    .get(ci)
                                    .map(|c| hl_style(*c))
                                    .unwrap_or(Style::default().fg(body_fg));
                                Span::styled(ch.to_string(), style)
                            })
                            .collect();
                        shown.push(Line::from(spans));
                    }
                    _ => shown.push(Line::from(Span::styled(
                        clipped,
                        Style::default().fg(body_fg),
                    ))),
                }
            }
            if shown.is_empty() {
                shown.push(Line::from(Span::styled(
                    tr(lang, "(empty file)", "（空のファイル）"),
                    Style::default().fg(theme().dim),
                )));
            }
            f.render_widget(Paragraph::new(shown), inner);
        }
        crate::preview::PreviewBody::List { rows, truncated } => {
            let mut shown: Vec<Line> = rows
                .iter()
                .take(inner.height as usize)
                .map(|r| Line::from(Span::styled(truncate(r, inner.width as usize), Style::default().fg(body_fg))))
                .collect();
            if *truncated && shown.len() == inner.height as usize {
                if let Some(last) = shown.last_mut() {
                    *last = Line::from(Span::styled("…", Style::default().fg(theme().dim)));
                }
            }
            f.render_widget(Paragraph::new(shown), inner);
        }
        crate::preview::PreviewBody::Note(msg) => {
            f.render_widget(
                Paragraph::new(msg.clone()).style(Style::default().fg(theme().dim)),
                inner,
            );
        }
        crate::preview::PreviewBody::Image => unreachable!("handled above"),
    }
}

/// The terminal-graphics image path: decode once per file (cached on `App`,
/// keyed by path), then let ratatui-image resize/encode for the box each
/// frame in whatever protocol the terminal offered at startup.
fn draw_image_gfx(f: &mut Frame, rect: Rect, inner: Rect, app: &mut App) {
    let lang = app.lang;
    let body_h = inner.height.saturating_sub(1); // the footer keeps its row
    let (path, title) = if let Popup::ImageView { path, title, .. } = &app.popup {
        (path.clone(), title.clone())
    } else {
        return;
    };
    // (Re)decode when a different image opens.
    if app.img_proto.as_ref().map(|(p, _)| p != &path).unwrap_or(true) {
        app.img_proto = None;
        if let (Ok(img), Some(picker)) = (image::open(&path), app.gfx_picker.as_ref()) {
            app.img_proto = Some((path.clone(), picker.new_resize_protocol(img)));
        }
    }
    let caption = image::image_dimensions(&path)
        .map(|(w, h)| format!("{}×{}px", w, h))
        .unwrap_or_default();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(theme().popup_bg))
        .title(format!(" {}  —  {} ", title, caption));
    f.render_widget(block, rect);

    let pic = Rect::new(inner.x, inner.y, inner.width, body_h);
    match app.img_proto.as_mut() {
        Some((_, proto)) => {
            f.render_stateful_widget(
                ratatui_image::StatefulImage::default(),
                pic,
                proto,
            );
        }
        None => {
            f.render_widget(
                Paragraph::new(tr(lang, "cannot show image", "画像を表示できません"))
                    .style(Style::default().fg(Color::Rgb(230, 120, 120))),
                pic,
            );
        }
    }

    let footer_area = Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
    f.render_widget(
        Paragraph::new(tr(lang, " S-Enter reveal   E edit   Esc close ", " S-Enter 場所へ   E 編集   Esc 閉じる "))
            .style(Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)),
        footer_area,
    );
}

/// Width of the viewer's blame gutter: `hash(7) + " " + author(11) + " "`.
const BLAME_W: usize = 20;

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

/// A text color that reads clearly on `bg`: near-black on a light background,
/// near-white on a dark one. Keeps popup text legible under any theme — a light
/// theme (e.g. Solarized Light) would otherwise show pale text on a pale ground.
pub(crate) fn readable_on(bg: Color) -> Color {
    let (r, g, b) = match bg {
        Color::Rgb(r, g, b) => (r as f32, g as f32, b as f32),
        _ => return Color::Rgb(225, 225, 240), // unknown → assume a dark ground
    };
    let lum = 0.299 * r + 0.587 * g + 0.114 * b;
    if lum > 140.0 {
        Color::Rgb(30, 32, 40)
    } else {
        Color::Rgb(228, 228, 240)
    }
}

/// Inline Markdown within one text run: `**bold**` and `` `code` ``. Anything
/// that would cross a wrap boundary is simply left as plain text.
fn md_inline(text: &str, base: Style, code_c: Color) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut buf = String::new();
    let mut i = 0;
    while i < chars.len() {
        // Inline code: `...`
        if chars[i] == '`' {
            if let Some(rel) = chars[i + 1..].iter().position(|&c| c == '`') {
                if !buf.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut buf), base));
                }
                let code: String = chars[i + 1..i + 1 + rel].iter().collect();
                spans.push(Span::styled(code, Style::default().fg(code_c)));
                i = i + rel + 2;
                continue;
            }
        }
        // Bold: **...**
        if chars[i] == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(rel) = chars[i + 2..].windows(2).position(|w| w == ['*', '*']) {
                let end = i + 2 + rel;
                if !buf.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut buf), base));
                }
                let b: String = chars[i + 2..end].iter().collect();
                spans.push(Span::styled(b, base.add_modifier(Modifier::BOLD)));
                i = end + 2;
                continue;
            }
        }
        buf.push(chars[i]);
        i += 1;
    }
    if !buf.is_empty() {
        spans.push(Span::styled(buf, base));
    }
    if spans.is_empty() {
        spans.push(Span::styled(String::new(), base));
    }
    spans
}

/// Render one raw line of an assistant answer as Markdown into wrapped, styled
/// lines paired with their plain text (for copy / scroll mapping). `in_code`
/// carries fenced-code-block state between lines; `gutter` is the speaker bar.
fn md_body_line(
    raw: &str,
    width: usize,
    gutter: Color,
    body_c: Color,
    in_code: &mut bool,
) -> Vec<(String, Line<'static>)> {
    let code_c = Color::Rgb(206, 145, 120);
    let head_c = Color::Rgb(120, 190, 255);
    let quote_c = Color::Rgb(150, 150, 170);
    let w = width.saturating_sub(2).max(1);
    let bar = || Span::styled("▏ ", Style::default().fg(gutter));
    let mut out: Vec<(String, Line)> = Vec::new();
    let trimmed = raw.trim_start();

    // A ``` fence toggles code mode and draws a faint rule.
    if trimmed.starts_with("```") {
        *in_code = !*in_code;
        out.push((
            raw.to_string(),
            Line::from(vec![bar(), Span::styled("─".repeat(w.min(40)), Style::default().fg(quote_c))]),
        ));
        return out;
    }
    if *in_code {
        for chunk in wrap_str(raw, w) {
            let line = Line::from(vec![bar(), Span::styled(chunk.clone(), Style::default().fg(code_c))]);
            out.push((chunk, line));
        }
        return out;
    }
    // Heading: one-to-three leading '#'.
    let hashes = trimmed.chars().take_while(|&c| c == '#').count();
    if (1..=3).contains(&hashes) && trimmed.chars().nth(hashes) == Some(' ') {
        let text = trimmed[hashes + 1..].trim();
        for chunk in wrap_str(text, w) {
            let line = Line::from(vec![
                bar(),
                Span::styled(chunk.clone(), Style::default().fg(head_c).add_modifier(Modifier::BOLD)),
            ]);
            out.push((chunk, line));
        }
        return out;
    }
    // Bullet: "- " / "* " → "• "
    if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
        let mut first = true;
        for chunk in wrap_str(rest, w.saturating_sub(2)) {
            let marker = if first { "• " } else { "  " };
            let mut spans = vec![bar(), Span::styled(marker, Style::default().fg(gutter))];
            spans.extend(md_inline(&chunk, Style::default().fg(body_c), code_c));
            out.push((format!("{marker}{chunk}"), Line::from(spans)));
            first = false;
        }
        return out;
    }
    // Blockquote: "> "
    if let Some(rest) = trimmed.strip_prefix("> ") {
        for chunk in wrap_str(rest, w.saturating_sub(2)) {
            let line = Line::from(vec![
                bar(),
                Span::styled("│ ", Style::default().fg(quote_c)),
                Span::styled(chunk.clone(), Style::default().fg(quote_c).add_modifier(Modifier::ITALIC)),
            ]);
            out.push((chunk, line));
        }
        return out;
    }
    // Plain paragraph with inline styling.
    for chunk in wrap_str(raw, w) {
        let mut spans = vec![bar()];
        spans.extend(md_inline(&chunk, Style::default().fg(body_c), code_c));
        out.push((chunk, Line::from(spans)));
    }
    out
}

fn draw_ai_chat(f: &mut Frame, area: Rect, app: &mut App) {
    let lang = app.lang;
    // How this window presents itself: the action that opened it, and whether
    // the local model or crmaine is answering.
    let skin = if let Popup::AiChat { skin, .. } = &app.popup {
        skin.clone()
    } else {
        ChatSkin::of(ChatMode::Ai)
    };
    let width: u16 = 76u16.min(area.width.saturating_sub(2));
    let height = area.height.saturating_sub(2).max(8);
    let rect = centered_rect(width, height, area);
    f.render_widget(Clear, rect);
    // Each backend wears its own colour, so the frame alone says who is
    // answering: crmaine's signature carmine (the same frame the remote pane
    // wears), and cyan for the local model.
    let accent = if skin.simple { AI_SIMPLE } else { CRMAINE };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
        .style(Style::default().bg(theme().popup_bg))
        .title(Line::from(vec![
            Span::styled(" ✦ ", Style::default().fg(accent).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{} ", skin.title),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
        ]))
        .title_bottom(tr(
            lang,
            " Enter=send  Shift+Enter=newline  Ctrl+V=paste (image too)  Ctrl+R=history  Esc=stop/close ",
            " Enter=送信  Shift+Enter=改行  Ctrl+V=貼付（画像も）  Ctrl+R=履歴  Esc=中断/閉じる ",
        ));
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);
    let body_w = inner.width.max(1) as usize;

    // The current pipeline stage (if any), read before the popup is borrowed.
    let stage = app.crmaine_stage.clone();
    // The input can be several lines (Alt+Enter); the transcript gives up a row
    // per extra input line, capped so a huge paste can't swallow the answer.
    let input_rows = if let Popup::AiChat { input, .. } = &app.popup {
        input.split('\n').count().clamp(1, 6)
    } else {
        1
    };
    // Pasted images get their own row above the input, so the count is visible
    // before sending rather than only in the transient status message.
    let attach_n = app.chat_attachments.len();
    let attach_rows = u16::from(attach_n > 0);
    let view_h = inner.height.saturating_sub(input_rows as u16 + attach_rows) as usize;

    let mut flat: Vec<String> = Vec::new();
    let mut shown: Vec<Line> = Vec::new();
    let mut input_str = String::new();
    let mut off = 0usize;
    if let Popup::AiChat { input, log, scroll, pending, sel, .. } = &mut app.popup {
        // Flat plain-text lines (for copying) and their styled counterparts.
        // Each turn is a speaker header line followed by the wrapped body,
        // indented — the "crmaine - Ajent" name is too long to sit inline.
        let mut styled: Vec<Line> = Vec::new();
        // Message text must contrast with the popup ground under any theme.
        let body_c = readable_on(theme().popup_bg);
        let source_c = Color::Rgb(150, 175, 205);
        let dim_c = Color::Rgb(150, 150, 170);
        for m in log.iter() {
            // The assistant signs with the backend that actually answered — a
            // reply from the local model must not read as crmaine's work.
            let (glyph, name, name_c) = if m.user {
                ("▍", tr(lang, "you", "あなた"), theme().accent)
            } else if skin.simple {
                ("◆", "AI - simple", accent)
            } else {
                ("◆", tr(lang, "crmaine", "カーマイン"), accent)
            };
            styled.push(Line::from(vec![
                Span::styled(format!("{glyph} "), Style::default().fg(name_c).add_modifier(Modifier::BOLD)),
                Span::styled(name.to_string(), Style::default().fg(name_c).add_modifier(Modifier::BOLD)),
            ]));
            flat.push(name.to_string());
            // Once crmaine's "— sources —" rule appears, the rest of the turn is
            // its citation list; render those quietly and in a link-ish blue.
            let mut in_sources = false;
            let mut in_code = false;
            for raw in m.text.split('\n') {
                if raw.trim() == "— sources —" {
                    in_sources = true;
                    styled.push(Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(
                            tr(lang, "sources", "参照元"),
                            Style::default().fg(dim_c).add_modifier(Modifier::BOLD),
                        ),
                    ]));
                    flat.push(raw.to_string());
                    continue;
                }
                // The assistant's prose is Markdown; the user's text and the
                // citation list stay literal.
                if !m.user && !in_sources {
                    for (plain, line) in md_body_line(raw, body_w, name_c, body_c, &mut in_code) {
                        styled.push(line);
                        flat.push(plain);
                    }
                    continue;
                }
                let text_c = if in_sources { source_c } else { body_c };
                for chunk in wrap_str(raw, body_w.saturating_sub(2)) {
                    styled.push(Line::from(vec![
                        // A thin gutter in the speaker's colour gives the thread
                        // a chat feel without boxing every message.
                        Span::styled("▏ ", Style::default().fg(name_c)),
                        Span::styled(chunk.clone(), Style::default().fg(text_c)),
                    ]));
                    flat.push(chunk);
                }
            }
            styled.push(Line::from(""));
            flat.push(String::new());
        }
        if *pending {
            // A braille spinner in the backend's colour, driven off the wall clock
            // so it turns while the answer is in flight (the loop force-repaints
            // meanwhile).
            const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let fi = (app.startup_at.elapsed().as_millis() / 90) as usize % FRAMES.len();
            let label = stage.clone().unwrap_or_else(|| {
                if skin.simple {
                    tr(lang, "AI - simple is thinking…", "AI - simple が考えています…").to_string()
                } else {
                    tr(lang, "crmaine is thinking…", "カーマイン が考えています…").to_string()
                }
            });
            styled.push(Line::from(vec![
                Span::styled(
                    format!("{} ", FRAMES[fi]),
                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(label, Style::default().fg(dim_c).add_modifier(Modifier::ITALIC)),
            ]));
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
    if attach_rows > 0 {
        let label = match lang {
            Lang::Ja => format!("画像 {attach_n} 枚"),
            Lang::En if attach_n == 1 => "1 image".to_string(),
            Lang::En => format!("{attach_n} images"),
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("📎 {label}"),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ))),
            Rect::new(inner.x, inner.y + view_h as u16, inner.width, 1),
        );
    }
    // The input, possibly several lines. A "> " prompt on the first row, aligned
    // continuation on the rest, and a block caret at the very end.
    let in_style = Style::default()
        .fg(readable_on(theme().selected_bg))
        .add_modifier(Modifier::BOLD)
        .bg(theme().selected_bg);
    let raw_lines: Vec<&str> = input_str.split('\n').collect();
    let last = raw_lines.len().saturating_sub(1);
    let mut in_lines: Vec<Line> = Vec::with_capacity(input_rows);
    for (i, seg) in raw_lines.iter().enumerate().take(input_rows) {
        let prefix = if i == 0 { "> " } else { "  " };
        let caret = if i == last { "\u{2588}" } else { "" };
        in_lines.push(Line::from(Span::styled(format!("{prefix}{seg}{caret}"), in_style)));
    }
    f.render_widget(
        Paragraph::new(in_lines).style(Style::default().bg(theme().selected_bg)),
        Rect::new(inner.x, inner.y + view_h as u16 + attach_rows, inner.width, input_rows as u16),
    );
}

/// The operation queue (`:queue`): the running op with its progress and
/// stall age, then everything waiting its turn.
fn draw_op_queue(f: &mut Frame, area: Rect, app: &mut App) {
    let lang = app.lang;
    let cursor = match &app.popup {
        Popup::OpQueue { cursor } => *cursor,
        _ => return,
    };
    let w = 60u16.min(area.width.saturating_sub(2));
    let n_rows = 1 + app.op_queue.len();
    let h = (n_rows as u16 + 4).clamp(6, area.height.saturating_sub(2));
    let inner = popup_frame(
        f,
        area,
        w,
        h,
        tr(lang, " operation queue ", " 操作キュー "),
        tr(lang, " x=stop/remove (x again=abandon)  Esc ", " x=停止/削除（再度x=見捨て）  Esc "),
    );
    let body_c = readable_on(theme().popup_bg);
    let mut lines: Vec<Line> = Vec::new();
    // Row 0: the runner.
    match &app.op_job {
        Some(job) => {
            let p = &job.latest;
            let pct = if let Some(f) = (p.bytes_done * 100).checked_div(p.bytes_total) {
                format!("{}%", f.min(100))
            } else {
                format!("{}/{}", p.files_done, p.files_total)
            };
            let stalled = app.op_stalled();
            let state = if job.cancel_requested.is_some() {
                tr(lang, "stopping…", "停止中…").to_string()
            } else if stalled {
                let s = job.last_progress.elapsed().as_secs();
                if lang == Lang::Ja { format!("⚠ 停滞 {}秒", s) } else { format!("⚠ stalled {}s", s) }
            } else {
                tr(lang, "running", "実行中").to_string()
            };
            let c = if stalled { Color::Rgb(235, 200, 100) } else { Color::Rgb(130, 205, 150) };
            lines.push(Line::from(vec![
                Span::styled(if cursor == 0 { "▶ " } else { "  " }, Style::default().fg(theme().accent)),
                Span::styled(format!("{} {} ", job.label, pct), Style::default().fg(body_c).add_modifier(Modifier::BOLD)),
                Span::styled(state, Style::default().fg(c)),
            ]));
        }
        None => lines.push(Line::from(Span::styled(
            tr(lang, "  (nothing running)", "  （実行中なし）"),
            Style::default().fg(theme().dim),
        ))),
    }
    // The waiting line.
    for (i, q) in app.op_queue.iter().enumerate() {
        let sel = cursor == i + 1;
        lines.push(Line::from(vec![
            Span::styled(if sel { "▶ " } else { "  " }, Style::default().fg(theme().accent)),
            Span::styled(
                format!("{}. {}", i + 1, q.label),
                Style::default().fg(if sel { body_c } else { theme().dim }),
            ),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The UI-toggles menu: each switch with its current state, cursor-highlighted.
fn draw_toggles(f: &mut Frame, area: Rect, app: &App) {
    let lang = app.lang;
    let Popup::Toggles { cursor } = &app.popup else { return };
let cursor = *cursor;
let rows = app.toggle_rows();
let width: u16 = 42u16.min(area.width.saturating_sub(2));
let height = (rows.len() as u16 + 3).clamp(5, area.height.saturating_sub(2));
let rect = centered_rect(width, height, area);
f.render_widget(Clear, rect);
let block = Block::default()
    .borders(Borders::ALL)
    .border_type(border_type())
    .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
    .style(Style::default().bg(theme().popup_bg))
    .title(tr(lang, " toggles ", " トグル "))
    .title_bottom(tr(lang, " Enter/Space=flip  ↑↓  Esc ", " Enter/Space=切替  ↑↓  Esc "));
let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
f.render_widget(block, rect);

let body_c = readable_on(theme().popup_bg);
let dim_c = Color::Rgb(150, 150, 170);
let on_c = Color::Rgb(130, 205, 150);
let w = inner.width as usize;
let mut lines: Vec<Line> = Vec::new();
for (i, (_, label, state, on)) in rows.iter().enumerate() {
    let sel = i == cursor;
    let marker = if sel { "▶ " } else { "  " };
    // Right-align the state text on the row.
    let pad = w.saturating_sub(2 + label.chars().count() + state.chars().count()).max(1);
    let label_style = if sel {
        Style::default().fg(readable_on(theme().selected_bg)).bg(theme().selected_bg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(body_c)
    };
    let state_style = if *on {
        Style::default().fg(on_c).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(dim_c)
    };
    lines.push(Line::from(vec![
        Span::styled(marker, Style::default().fg(theme().accent)),
        Span::styled(label.clone(), label_style),
        Span::raw(" ".repeat(pad)),
        Span::styled(state.clone(), state_style),
    ]));
}
f.render_widget(Paragraph::new(lines), inner);
}

/// The chat history picker: past conversations this session, newest first.
fn draw_ai_history(f: &mut Frame, area: Rect, app: &App) {
    let lang = app.lang;
    let Popup::AiHistory { cursor } = &app.popup else { return };
let cursor = *cursor;
// This list mixes both backends' conversations, so it wears neither one's
// colour — each row carries its own badge instead.
let frame_c = theme().accent;
let dim_c = Color::Rgb(150, 150, 170);
let width: u16 = 72u16.min(area.width.saturating_sub(2));
let height = (app.ai_history.len() as u16 + 3).clamp(6, area.height.saturating_sub(2));
let rect = centered_rect(width, height, area);
f.render_widget(Clear, rect);
let block = Block::default()
    .borders(Borders::ALL)
    .border_type(border_type())
    .border_style(Style::default().fg(frame_c).add_modifier(Modifier::BOLD))
    .style(Style::default().bg(theme().popup_bg))
    .title(tr(lang, " chat history ", " チャット履歴 "))
    .title_bottom(tr(lang, " Enter=open  d=delete  ↑↓  Esc ", " Enter=開く  d=削除  ↑↓  Esc "));
let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
f.render_widget(block, rect);

let body_c = readable_on(theme().popup_bg);
let view_h = inner.height as usize;
let first = if cursor >= view_h { cursor + 1 - view_h } else { 0 };
let mut lines: Vec<Line> = Vec::new();
for (i, c) in app.ai_history.iter().enumerate().skip(first).take(view_h) {
    let sel = i == cursor;
    let log = c.log();
    let title = App::ai_history_title(log);
    let turns = log.iter().filter(|m| m.user).count();
    let marker = if sel { "▶ " } else { "  " };
    let badge = format!("{:<6} ", c.mode().badge());
    let title_style = if sel {
        Style::default()
            .fg(readable_on(theme().selected_bg))
            .bg(theme().selected_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(body_c)
    };
    lines.push(Line::from(vec![
        Span::styled(marker, Style::default().fg(frame_c)),
        Span::styled(badge, Style::default().fg(dim_c)),
        Span::styled(title, title_style),
        Span::styled(format!("  ({turns})"), Style::default().fg(dim_c)),
    ]));
}
f.render_widget(Paragraph::new(lines), inner);
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
    tr(lang, " Draft commit message — editing ", " コミットメッセージ生成 — 編集中 ")
} else {
    tr(lang, " Draft commit message ", " コミットメッセージ生成 ")
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
    .border_style(Style::default().fg(AI_SIMPLE).add_modifier(Modifier::BOLD))
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
        format!(" ゴミファイル検出  {}/{} 選択 ", checked, n)
    } else {
        format!(" Detect junk files  {}/{} checked ", checked, n)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(AI_SIMPLE).add_modifier(Modifier::BOLD))
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
        format!(" フォルダ構成を提案  {}/{} 選択 ", checked, n)
    } else {
        format!(" Suggest folder structure  {}/{} checked ", checked, n)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(AI_SIMPLE).add_modifier(Modifier::BOLD))
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
    let (n, checked, by_ai) = if let Popup::RenameReview { items, by_ai, .. } = &app.popup {
        (items.len(), items.iter().filter(|i| i.selected).count(), *by_ai)
    } else {
        (0, 0, false)
    };
    // Named for whichever side proposed the renames: the AI menu item, or the
    // `:brename` pattern.
    let head = match (by_ai, lang) {
        (true, Lang::Ja) => "AIリネーム",
        (true, Lang::En) => "AI rename",
        (false, Lang::Ja) => "リネーム候補",
        (false, Lang::En) => "proposed renames",
    };
    let title = if lang == Lang::Ja {
        format!(" {}  {}/{} 選択 ", head, checked, n)
    } else {
        format!(" {}  {}/{} checked ", head, checked, n)
    };
    let accent = if by_ai { AI_SIMPLE } else { theme().accent };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
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
    if let Popup::RenameReview { items, cursor, scroll, .. } = &mut app.popup {
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

/// The frame nearly every popup wears: a centred `w`×`h` box, cleared, bordered
/// in the accent colour, with `title` along the top and `footer` along the
/// bottom. Returns the inner area to draw into.
///
/// Pass `""` for a title or footer the popup does not want — an empty one draws
/// nothing. The handful of popups that need something else (their own anchor
/// rect, a filled background, a tighter margin) still build their own block;
/// this is the common case, not a mandate.
fn popup_frame<'a>(
    f: &mut Frame,
    area: Rect,
    w: u16,
    h: u16,
    title: impl Into<Line<'a>>,
    footer: impl Into<Line<'a>>,
) -> Rect {
    popup_frame_in(f, area, w, h, title, footer, theme().accent)
}

/// The same frame in a chosen colour — for the AI - simple windows, which wear
/// [`AI_SIMPLE`] instead of the theme accent.
#[allow(clippy::too_many_arguments)]
fn popup_frame_in<'a>(
    f: &mut Frame,
    area: Rect,
    w: u16,
    h: u16,
    title: impl Into<Line<'a>>,
    footer: impl Into<Line<'a>>,
    accent: Color,
) -> Rect {
    let rect = centered_rect(w, h, area);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(accent).add_modifier(Modifier::BOLD))
        .title(title)
        .title_bottom(footer);
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);
    inner
}

#[allow(clippy::too_many_arguments)]
fn draw_popup(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    hosts: &[cian_lua::SshHost],
    snippets: &[cian_lua::Snippet],
    find: Option<(&str, &str, Option<cian_core::search::Outcome>, cian_core::search::Mode)>,
    dests: &[(String, PathBuf)],
    zones: &mut Vec<PopupZone>,
    lang: Lang,
    menu_lang: Lang,
    show_ws: bool,
    ruler: bool,
    msg: Option<String>,
    tab_at: usize,
    tab_names: &[String],
    tab_rects: &mut Vec<(Rect, usize)>,
) {
    // Every popup with a shape of its own draws itself. The rest — the
    // confirm/notice dialogs, which differ only in their wording — fall through
    // to the one renderer they share.
    match popup {
        Popup::ThemePicker { .. } => draw_theme_picker(f, area, popup, lang),
        Popup::Manual { .. } => draw_manual(f, area, popup, lang),
        Popup::ContextMenu { .. } => draw_context_menu(f, area, popup, menu_lang),
        Popup::SshHosts { .. } => draw_ssh_hosts(f, area, popup, hosts, zones, lang),
        Popup::Snippets { .. } => draw_snippets(f, area, popup, snippets, zones, lang),
        Popup::RemoteBrowser { .. } => draw_remote_browser(f, area, popup, zones, lang),
        Popup::LocalDest { .. } => draw_local_dest(f, area, popup, zones, lang),
        Popup::SshUsers { .. } => draw_ssh_users(f, area, popup, hosts, zones, lang),
        Popup::FindResults { .. } => draw_find_results(f, area, popup, find, zones, lang),
        Popup::GrepReplace(_) => draw_grep_replace(f, area, popup, zones, lang),
        Popup::Shortcuts { .. } => draw_shortcuts(f, area, popup, zones, lang),
        Popup::History { .. } => draw_history(f, area, popup, zones, lang),
        Popup::DestPicker { .. } => draw_dest_picker(f, area, popup, dests, zones, lang),
        Popup::Viewer { .. } => {
            *tab_rects = draw_viewer(f, area, popup, lang, (show_ws, ruler), msg.as_deref(), (tab_at, tab_names, &[]));
        }
        Popup::DirCompare { .. } => draw_dir_compare(f, area, popup, zones, lang),
        Popup::Diff { .. } => draw_diff(f, area, popup, lang),
        Popup::Archive { .. } => draw_archive(f, area, popup, zones, lang),
        Popup::Palette { .. } => draw_palette(f, area, popup, lang),
        Popup::DiskUsage { .. } => draw_disk_usage(f, area, popup, zones, lang),
        Popup::GitLog { .. } => draw_git_log(f, area, popup, zones, lang),
        Popup::Macros { .. } => draw_macros(f, area, popup, zones, lang),
        Popup::SortPicker { .. } => draw_sort_picker(f, area, popup, zones, lang),
        Popup::EncodingPicker { .. } => draw_encoding_picker(f, area, popup, zones, lang),
        Popup::ColorPicker { .. } => draw_color_picker(f, area, popup, zones, lang),
        _ => draw_simple_dialog(f, area, popup, zones, lang),
    }
}

/// The confirm/notice dialogs, which differ only in their text: each supplies
/// a title, body lines and a footer hint, and they share one frame, one body
/// paragraph and one button row.
fn draw_simple_dialog(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
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
        Popup::ConfirmNoBom { targets } => {
            let title = tr(lang, " strip BOM ", " BOM除去 ").to_string();
            let head = if lang == Lang::Ja {
                format!("{} 件から UTF-8 BOM を除去します:", targets.len())
            } else {
                format!("strip the UTF-8 BOM from {} file(s):", targets.len())
            };
            let mut lines = vec![head, String::new()];
            for p in targets.iter().take(8) {
                lines.push(format!("  {}", p.display()));
            }
            if targets.len() > 8 {
                lines.push(tr_count(lang, targets.len() - 8));
            }
            lines.push(String::new());
            lines.push(
                tr(lang,
                   "UTF-16 files are detected and left alone (their BOM is load-bearing).",
                   "UTF-16 のファイルは検出してスキップします（BOM が必須のため）。")
                .to_string(),
            );
            let foot = tr(lang, " y/Enter=strip  n/Esc=cancel ", " y/Enter=除去  n/Esc=取消 ");
            (title, lines, foot.to_string())
        }
        Popup::ConfirmZipAdd { archive, sub, sources } => {
            let title = tr(lang, " add to zip ", " zipへ追加 ").to_string();
            let where_ = format!(
                "{}{}{}",
                archive.file_name().map(|s| s.to_string_lossy()).unwrap_or_default(),
                if sub.is_empty() { "" } else { "/" },
                sub
            );
            let head = if lang == Lang::Ja {
                format!("{} 件 → {}:", sources.len(), where_)
            } else {
                format!("{} item(s) → {}:", sources.len(), where_)
            };
            let mut lines = vec![head, String::new()];
            for p in sources.iter().take(8) {
                lines.push(format!("  {}", p.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()));
            }
            if sources.len() > 8 {
                lines.push(tr_count(lang, sources.len() - 8));
            }
            lines.push(String::new());
            lines.push(tr(lang, "same names inside the zip are replaced", "zip内の同名メンバーは置き換えられます").to_string());
            let foot = tr(lang, " y/Enter=add  n/Esc=cancel ", " y/Enter=追加  n/Esc=取消 ");
            (title, lines, foot.to_string())
        }
        Popup::ConfirmZipDelete { archive, members, shown } => {
            let title = tr(lang, " delete from zip ", " zipから削除 ").to_string();
            let head = if lang == Lang::Ja {
                format!(
                    "{} 件（メンバー {} 個）を {} から削除:",
                    shown.len(),
                    members.len(),
                    archive.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()
                )
            } else {
                format!(
                    "{} item(s) ({} member(s)) from {}:",
                    shown.len(),
                    members.len(),
                    archive.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()
                )
            };
            let mut lines = vec![head, String::new()];
            for m in shown.iter().take(8) {
                lines.push(format!("  {}", m));
            }
            if shown.len() > 8 {
                lines.push(tr_count(lang, shown.len() - 8));
            }
            lines.push(String::new());
            lines.push(tr(
                lang,
                "the zip is rewritten — there is no trash for this",
                "zipを書き直します — ゴミ箱には行きません",
            ).to_string());
            let foot = tr(lang, " y/Enter=delete  n/Esc=cancel ", " y/Enter=削除  n/Esc=取消 ");
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
        Popup::TextInput { title, prompt, .. } => {
            // The field line is filled in below as a styled Line (the cursor
            // highlights a character rather than inserting one, so nothing
            // shifts as it moves).
            let body = vec![prompt.clone(), String::new()];
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
                tr(lang, " Command from description ", " 説明からコマンド生成 ").to_string(),
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
        Popup::ConfirmDiffCopy { src, dst, is_dir, .. } => {
            let what = if *is_dir {
                tr(lang, "directory", "ディレクトリ")
            } else {
                tr(lang, "file", "ファイル")
            };
            let head = if lang == Lang::Ja {
                format!("既存の{}を上書きします:", what)
            } else {
                format!("overwrite the existing {}:", what)
            };
            let lines = vec![
                head,
                String::new(),
                format!("  {} {}", tr(lang, "from", "元"), src.display()),
                format!("  {}   {}", tr(lang, "to", "先"), dst.display()),
                String::new(),
                tr(lang, "The destination will be replaced.", "コピー先は置き換えられます。").to_string(),
            ];
            (
                tr(lang, " copy across ", " 反対側へコピー ").to_string(),
                lines,
                tr(lang, " y/Enter = overwrite   n/Esc = cancel ", " y/Enter = 上書き   n/Esc = 取消 ").to_string(),
            )
        }
        Popup::ConfirmDirSync { to_right, ops, extra, .. } => {
            let arrow = if *to_right { "left → right" } else { "right → left" };
            let arrow_ja = if *to_right { "左 → 右" } else { "右 → 左" };
            let n = ops.len();
            let head = if lang == Lang::Ja {
                format!("フォルダを一方向に同期（{}）", arrow_ja)
            } else {
                format!("one-way folder sync ({})", arrow)
            };
            let mut lines = vec![
                head,
                String::new(),
                format!("  {} {}", tr(lang, "copy / overwrite:", "コピー／上書き:"), n),
            ];
            if *extra > 0 {
                lines.push(format!(
                    "  {} {}",
                    tr(lang, "destination-only, kept:", "コピー先のみ・保持:"),
                    extra
                ));
            }
            lines.push(String::new());
            lines.push(
                tr(
                    lang,
                    "Nothing is deleted; the source's files are copied over.",
                    "削除は行いません。コピー元のファイルで置き換えます。",
                )
                .to_string(),
            );
            (
                tr(lang, " synchronize ", " 同期 ").to_string(),
                lines,
                tr(lang, " y/Enter = sync   n/Esc = cancel ", " y/Enter = 同期   n/Esc = 取消 ").to_string(),
            )
        }
        Popup::ConfirmRemoteDelete { name, is_dir, .. } => {
            let head = if *is_dir {
                tr(lang, "delete this folder and everything inside it, on the server:",
                      "このフォルダを中身ごとサーバ上で削除します:").to_string()
            } else {
                tr(lang, "delete this file on the server:", "このファイルをサーバ上で削除します:").to_string()
            };
            let lines = vec![
                head,
                String::new(),
                format!("  {}", name),
                String::new(),
                tr(lang, "This is permanent — the server has no trash.", "取り消せません（サーバにゴミ箱はありません）。").to_string(),
            ];
            (
                tr(lang, " remote delete ", " リモート削除 ").to_string(),
                lines,
                tr(lang, " y/Enter = delete   n/Esc = cancel ", " y/Enter = 削除   n/Esc = 取消 ").to_string(),
            )
        }
        Popup::ConfirmRemoteMove { plan, from, to } => {
            let n = plan.files.len();
            let head = if lang == Lang::Ja {
                format!("{} 個をホスト間で移動します:", n)
            } else {
                format!("move {} item(s) across hosts:", n)
            };
            let lines = vec![
                head,
                String::new(),
                format!("  {}  →  {}", from, to),
                String::new(),
                tr(lang, "Each file is copied, then deleted from the source.", "各ファイルをコピー後、コピー元から削除します。").to_string(),
            ];
            (
                tr(lang, " move across hosts ", " ホスト間の移動 ").to_string(),
                lines,
                tr(lang, " y/Enter = move   n/Esc = cancel ", " y/Enter = 移動   n/Esc = 取消 ").to_string(),
            )
        }
        Popup::ConfirmSnippet { name, cmd, .. } => {
            let head = if lang == Lang::Ja {
                format!("スニペットを送信しますか？  「{}」", name)
            } else {
                format!("send this snippet?  \"{}\"", name)
            };
            let lines = vec![head, String::new(), format!("  $ {}", cmd)];
            (
                tr(lang, " send snippet ", " スニペット送信 ").to_string(),
                lines,
                tr(lang, " y/Enter = send   n/Esc = cancel ", " y/Enter = 送信   n/Esc = 取消 ").to_string(),
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
        | Popup::GitLog { .. }
        | Popup::EncodingPicker { .. }
        | Popup::SshHosts { .. }
        | Popup::SshUsers { .. }
        | Popup::Snippets { .. }
        | Popup::ThemePicker { .. }
        | Popup::RemoteBrowser { .. }
        | Popup::LocalDest { .. }
        | Popup::Shortcuts { .. }
        | Popup::History { .. }
        | Popup::FindResults { .. }
        | Popup::GrepReplace(_)
        | Popup::DestPicker { .. }
        | Popup::Viewer { .. }
        | Popup::Diff { .. }
        | Popup::DirCompare { .. }
        | Popup::Archive { .. }
        | Popup::DiskUsage { .. }
        | Popup::Palette { .. }
        | Popup::AiChat { .. }
        | Popup::AiHistory { .. }
        | Popup::Toggles { .. }
        | Popup::ImageView { .. }
        | Popup::CommitMessage { .. }
        | Popup::JunkReview { .. }
        | Popup::StructureReview { .. }
        | Popup::RenameReview { .. }
        | Popup::DupeReview { .. }
        | Popup::OpQueue { .. }
        | Popup::None => return,
    };

    // A text-input box is wider (long descriptions and pasted paths need room)
    // and grows taller as the value wraps, so nothing you type is cut off.
    let width: u16 = match popup {
        Popup::TextInput { .. } => 96u16.min(area.width.saturating_sub(2)),
        // A notice can be a key list, whose lines are a key and a sentence; at
        // seventy columns every one of them wrapped, which turns a list into a
        // wall. It takes what the longest line asks for, within reason.
        Popup::Notice { lines } => {
            let longest = lines.iter().map(|l| width(l)).max().unwrap_or(0) as u16;
            longest.saturating_add(6).clamp(40, 110).min(area.width.saturating_sub(2))
        }
        _ => 70u16.min(area.width.saturating_sub(2)),
    };
    let extra_rows = if let Popup::TextInput { buffer, .. } = popup {
        let inner_w = width.saturating_sub(4).max(1) as usize;
        (buffer.chars().count() / inner_w) as u16
    } else {
        0
    };
    let height = (body.len() as u16 + 4 + extra_rows).max(6).min(area.height.saturating_sub(2));
    let rect = centered_rect(width, height, area);

    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        // The AI - simple dialogs (the command confirm, the rename/search
        // prompts) wear the local model's cyan; the rest keep the theme accent.
        .border_style(Style::default().fg(popup_accent(popup)).add_modifier(Modifier::BOLD))
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

    let mut body_text: Vec<Line> = body.into_iter().map(Line::from).collect();
    // The text-input field renders the cursor as a highlighted character so
    // moving it never shifts the surrounding text (was inserting a caret glyph).
    // Not a popup renderer of its own: it rewrites the line the shared body
    // above already laid out.
    if let Popup::TextInput { buffer, cursor, kind, .. } = popup {
        if body_text.len() >= 2 {
            body_text[1] = caret_line(buffer, *cursor, kind.is_secret());
        }
    }
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

/// The theme gallery. The active theme is already applied live (the global
/// was swapped as the cursor moved), so the popup itself renders in the
/// previewed palette; a swatch row lets palettes be compared at a glance.
fn draw_theme_picker(f: &mut Frame, area: Rect, popup: &mut Popup, lang: Lang) {
    let Popup::ThemePicker { cursor, scope } = popup else { return };
    let names = crate::theme::THEME_NAMES;
    let pane_scope = matches!(scope, ThemeScope::Pane { .. });
    let w = 46u16.min(area.width);
    let h = (names.len() as u16 + 4).min(area.height.saturating_sub(2)).max(8);
    let rect = centered_rect(w, h, area);
    f.render_widget(Clear, rect);
    f.render_widget(Block::default().style(Style::default().bg(theme().popup_bg)), rect);
    let title = match scope {
        ThemeScope::App { .. } => tr(lang, " theme — whole app ", " テーマ — 全体 "),
        ThemeScope::Pane { side, .. } if *side == 0 => tr(lang, " theme — left pane ", " テーマ — 左ペイン "),
        ThemeScope::Pane { .. } => tr(lang, " theme — right pane ", " テーマ — 右ペイン "),
    };
    let footer = if pane_scope {
        tr(lang, " j/k=preview  Enter=keep  x=follow app  Esc=cancel ",
                 " j/k=プレビュー  Enter=決定  x=全体に従う  Esc=取消 ")
    } else {
        tr(lang, " j/k=preview  Enter=keep  Esc=cancel ",
                 " j/k=プレビュー  Enter=決定  Esc=取消 ")
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .title(title)
        .title_bottom(footer);
    let inner = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);
    let view_h = inner.height as usize;
    let scroll = cursor.saturating_sub(view_h.saturating_sub(1)).min(*cursor);
    let mut lines: Vec<Line> = Vec::new();
    for (i, name) in names.iter().enumerate().skip(scroll).take(view_h) {
        let sel = i == *cursor;
        let pal = crate::theme::theme_preset(name).unwrap_or_default();
        // A compact swatch: directory / code / archive / executable accents.
        let sw = |c: Color| Span::styled("█", Style::default().fg(c));
        let name_style = if sel {
            Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme().file.plain)
        };
        lines.push(Line::from(vec![
            Span::styled(if sel { "▸ " } else { "  " }, name_style),
            Span::styled(format!("{:<20}", name), name_style),
            sw(pal.file.directory), sw(pal.file.code), sw(pal.file.archive),
            sw(pal.file.executable), sw(pal.accent),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The manual is taller than any terminal, so it renders as a scrolling
/// viewport rather than the fixed block the other popups use.
fn draw_manual(f: &mut Frame, area: Rect, popup: &mut Popup, lang: Lang) {
    let Popup::Manual { lines, scroll } = popup else { return };
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
}

/// The context menu is anchored at the pointer rather than centred, so it
/// sizes and positions itself.
fn draw_context_menu(f: &mut Frame, area: Rect, popup: &mut Popup, menu_lang: Lang) {
    let Popup::ContextMenu { items, cursor, at } = popup else { return };
    // The context menu follows `menu_lang` (which may differ from the rest
    // of the UI) so it can be pinned to Japanese on an English interface.
    let lang = menu_lang;
    let (name_w, hint_w) = menu_dims(items, lang);
    let rect = context_menu_rect(items, *at, area, lang);

    f.render_widget(Clear, rect);
    // Follow the theme's own surface (light on a light theme) with readable
    // text, rather than the always-dark popup background.
    let surf = surface();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent))
        .style(Style::default().bg(surf));
    let inner = rect.inner(Margin { vertical: 1, horizontal: 1 });
    f.render_widget(block, rect);

    let rows: Vec<Line> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let sel = i == *cursor;
            let style = if sel {
                Style::default().bg(theme().selected_bg).fg(readable_on(theme().selected_bg)).add_modifier(Modifier::BOLD)
            } else {
                Style::default().bg(surf).fg(readable_on(surf))
            };
            // "▸ name … (hint)": name left-aligned, hint right-aligned in a
            // shared column, with even 2-cell gutters on both sides.
            let (name, hint) = menu_label_parts(item.label(lang));
            let marker = if sel { "▸ " } else { "  " };
            let body = if hint_w > 0 {
                format!("{}{}  {}  ", marker, pad_to(name, name_w), pad_left(hint, hint_w))
            } else {
                format!("{}{}  ", marker, pad_to(name, name_w))
            };
            Line::from(Span::styled(body, style))
        })
        .collect();
    f.render_widget(Paragraph::new(rows), inner);
}

fn draw_ssh_hosts(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    hosts: &[cian_lua::SshHost],
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::SshHosts { cursor, filter } = popup else { return };
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
    let footer = tr(lang, " Enter=select  F2=type by hand  Esc ", " Enter=選択  F2=手入力  Esc ");
    let inner = popup_frame(f, area, w, h, tr(lang, " ssh — host ", " SSH — ホスト "), footer);

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
}

fn draw_snippets(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    snippets: &[cian_lua::Snippet],
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::Snippets { cursor, filter } = popup else { return };
    let needle = filter.to_lowercase();
    let matches: Vec<&cian_lua::Snippet> = snippets
        .iter()
        .filter(|s| {
            needle.is_empty()
                || s.name.to_lowercase().contains(&needle)
                || s.cmd.to_lowercase().contains(&needle)
        })
        .collect();
    let w = 64u16.min(area.width);
    let h = (matches.len() as u16 + 5).min(area.height.saturating_sub(2)).max(6);
    let inner = popup_frame(f, area, w, h, tr(lang, " snippets → shell ", " スニペット → シェル "), "");

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
    for (i, s) in matches.iter().enumerate() {
        let sel = i == *cursor;
        let style = if sel {
            Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(205, 205, 218))
        };
        // A tag shows what will happen: run, type-only, or confirm-first.
        let tag = if s.confirm { "?" } else if s.enter { "↵" } else { "…" };
        lines.push(Line::from(vec![
            Span::styled(format!("{}{} ", if sel { "▸ " } else { "  " }, tag), style),
            Span::styled(format!("{:<20}", truncate(&s.name, 20)), style),
            Span::styled(
                format!("  {}", truncate(&s.cmd, (inner.width as usize).saturating_sub(26))),
                Style::default().fg(Color::Rgb(140, 140, 165)),
            ),
        ]));
        push_row_zone(zones, inner, inner.y + 1 + i as u16, i);
    }
    let body_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(1));
    f.render_widget(Paragraph::new(lines), body_area);
    let footer_area =
        Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1);
    f.render_widget(
        Paragraph::new(tr(lang, " type to filter  ↑↓ select  Enter send  Esc cancel ", " 入力で絞込  ↑↓ 選択  Enter 送信  Esc 取消 ")).style(
            Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
        ),
        footer_area,
    );
}

fn draw_remote_browser(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::RemoteBrowser { label, cwd, entries, cursor, scroll, marked, loading, purpose } = popup else { return };
    let uploading = *purpose == BrowsePurpose::Upload;
    let th = theme();
    let w = 70u16.min(area.width);
    let h = area.height.saturating_sub(4).clamp(8, 30);
    let title = if uploading {
        format!(" upload → {}  :  {} ", label, cwd)
    } else {
        format!(" download ← {}  :  {} ", label, cwd)
    };
    let footer = if uploading {
        tr(
            lang,
            " Enter=open  -=up  u=upload here  Esc ",
            " Enter=開く  -=上  u=ここへアップロード  Esc ",
        )
    } else {
        tr(
            lang,
            " Enter=open/mark  Space=mark  -=up  d=download  Esc ",
            " Enter=開く/選択  Space=選択  -=上  d=ダウンロード  Esc ",
        )
    };
    let inner = popup_frame(f, area, w, h, title, footer);
    let view_h = inner.height as usize;
    if *loading {
        f.render_widget(
            Paragraph::new(tr(lang, "  …listing", "  …取得中"))
                .style(Style::default().fg(Color::Rgb(150, 150, 170)).add_modifier(Modifier::ITALIC)),
            inner,
        );
        return;
    }
    *scroll = (*scroll).min(cursor.saturating_sub(view_h.saturating_sub(1)));
    if *cursor < *scroll {
        *scroll = *cursor;
    }
    let mut lines: Vec<Line> = Vec::new();
    if entries.is_empty() {
        lines.push(Line::from(Span::styled("  (empty)", Style::default().fg(Color::Rgb(150, 150, 170)))));
    }
    for (i, e) in entries.iter().enumerate().skip(*scroll).take(view_h) {
        let sel = i == *cursor;
        let checked = marked.contains(&e.name);
        let mark = if checked { "◉ " } else if sel { "▸ " } else { "  " };
        let (icon, name_c) = if e.is_dir {
            ("▸ ", th.file.directory)
        } else {
            ("  ", th.file.plain)
        };
        let size = if e.is_dir { String::new() } else { cian_core::disk::human_size(e.size) };
        // Base fg per row; the selected row also gets a full-width background
        // below so it reads as the focused row, like the file panes.
        let base = if sel {
            Style::default().fg(th.accent).add_modifier(Modifier::BOLD)
        } else if checked {
            Style::default().fg(Color::Rgb(130, 205, 150))
        } else {
            Style::default().fg(name_c)
        };
        let mut line = Line::from(vec![
            Span::styled(format!("{}{}", mark, icon), base),
            Span::styled(format!("{:<40}", truncate(&e.name, 40)), base),
            Span::styled(format!("{:>10}", size), Style::default().fg(th.dim)),
        ]);
        if sel {
            line = line.style(Style::default().bg(th.selected_bg));
        }
        lines.push(line);
        push_row_zone(zones, inner, inner.y + (i - *scroll) as u16, i);
    }
    // Paint the selected row's background across the full inner width first,
    // then the text on top (the spans carry no bg, so it shows through).
    if !entries.is_empty() && *cursor >= *scroll {
        let sel_y = inner.y + (*cursor - *scroll) as u16;
        if sel_y < inner.y + inner.height {
            f.render_widget(
                Block::default().style(Style::default().bg(th.selected_bg)),
                Rect::new(inner.x, sel_y, inner.width, 1),
            );
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_local_dest(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::LocalDest { files, cursor } = popup else { return };
    let opts_len = 4usize;
    let w = 56u16.min(area.width);
    let h = (opts_len as u16 + 4).min(area.height);
    let inner = popup_frame(f, area, w, h, format!(" download {} file(s) to… ", files.len()), "");
    // Labels only; the actual dirs are resolved when a row is chosen.
    let labels = [
        tr(lang, "Left pane", "左ペイン"),
        tr(lang, "Right pane", "右ペイン"),
        tr(lang, "Desktop", "デスクトップ"),
        tr(lang, "Type a path…", "パスを入力…"),
    ];
    let rows: Vec<Line> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let sel = i == *cursor;
            let style = if sel {
                Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(205, 205, 218))
            };
            push_row_zone(zones, inner, inner.y + i as u16, i);
            Line::from(Span::styled(format!("{}{}", if sel { "▸ " } else { "  " }, l), style))
        })
        .collect();
    f.render_widget(Paragraph::new(rows), inner);
}

fn draw_ssh_users(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    hosts: &[cian_lua::SshHost],
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::SshUsers { host, cursor } = popup else { return };
    let Some(hst) = hosts.get(*host) else { return };
    let w = 40u16.min(area.width);
    let h = (hst.users.len() as u16 + 4).min(area.height.saturating_sub(2)).max(6);
    let inner = popup_frame(f, area, w, h, format!(" {} — {} ", tr(lang, "ssh", "SSH"), hst.name), "");

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
}

/// The grep-replace preview: every line that would change, before / after,
/// with a checkbox. Unchecked rows are dimmed rather than hidden — the point
/// of the list is to see what you decided *not* to do as well as what you did.
fn draw_grep_replace(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::GrepReplace(plan) = popup else { return };
    let w = 110u16.min(area.width.saturating_sub(2));
    let h = area.height.saturating_sub(4).max(8);
    let picked = plan.changes.iter().filter(|c| c.picked).count();
    let files = {
        let mut seen: Vec<&std::path::Path> = Vec::new();
        for c in plan.changes.iter().filter(|c| c.picked) {
            if !seen.contains(&c.path.as_path()) {
                seen.push(c.path.as_path());
            }
        }
        seen.len()
    };
    let title = format!(
        " replace  {}  —  {}/{} line(s) in {} file(s) ",
        plan.what,
        picked,
        plan.changes.len(),
        files
    );
    let inner = popup_frame(f, area, w, h, truncate_middle(&title, w.saturating_sub(4) as usize), "");

    // Bottom-up: the hint bar, the "before" text of the line under the cursor,
    // and — when there is one — a note about files that could not be read,
    // because a silently ignored file is the thing most likely to be mistaken
    // for "already correct".
    let note = (!plan.skipped.is_empty()) as u16;
    let body_h = inner.height.saturating_sub(2 + note) as usize;
    if plan.cursor < plan.scroll {
        plan.scroll = plan.cursor;
    } else if body_h > 0 && plan.cursor >= plan.scroll + body_h {
        plan.scroll = plan.cursor + 1 - body_h;
    }

    let dim = Color::Rgb(120, 120, 140);
    let mut last_file: Option<&std::path::Path> = None;
    if plan.scroll > 0 {
        last_file = plan.changes.get(plan.scroll - 1).map(|c| c.path.as_path());
    }
    for (row, (i, c)) in plan.changes.iter().enumerate().skip(plan.scroll).take(body_h).enumerate() {
        let sel = i == plan.cursor;
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
        // The file name is printed once per run of lines from that file: with
        // twenty hits in one file, repeating the path twenty times crowds out
        // the text that is actually being decided on.
        let same_file = last_file == Some(c.path.as_path());
        last_file = Some(c.path.as_path());
        let loc = if same_file {
            format!("{:>8}: ", c.line + 1)
        } else {
            format!("{}:{}: ", c.path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(), c.line + 1)
        };
        let mark = if c.picked { "[x] " } else { "[ ] " };
        let loc_w = width(&loc).min(inner.width as usize / 3);
        let rest = (inner.width as usize).saturating_sub(4 + loc_w);
        let text_style = if c.picked {
            base.fg(Color::Rgb(225, 225, 240))
        } else {
            base.fg(dim).add_modifier(Modifier::CROSSED_OUT)
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(mark, if c.picked { base.fg(theme().accent) } else { base.fg(dim) }),
                Span::styled(truncate_middle(&loc, loc_w), base.fg(Color::Rgb(135, 135, 160))),
                Span::styled(truncate(&crate::util::plain(&c.after.replace('\n', "⏎")), rest), text_style),
            ])),
            line_area,
        );
    }

    // The rows show what each line becomes. The one under the cursor is the
    // one being decided, so show what it is now too — a diff of one, exactly
    // where it is needed and nowhere it is not.
    if let Some(c) = plan.changes.get(plan.cursor) {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" now ", Style::default().fg(dim)),
                Span::styled(
                    truncate(&crate::util::plain(&c.before), inner.width.saturating_sub(5) as usize),
                    Style::default().fg(Color::Rgb(200, 160, 160)),
                ),
            ])),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(2 + note), inner.width, 1),
        );
    }

    if note == 1 {
        let why = plan
            .skipped
            .iter()
            .take(2)
            .map(|s| {
                format!("{} ({})", s.path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(), s.why)
            })
            .collect::<Vec<_>>()
            .join(", ");
        let more = if plan.skipped.len() > 2 { format!(" +{}", plan.skipped.len() - 2) } else { String::new() };
        f.render_widget(
            Paragraph::new(truncate(
                &format!(" {} not read: {why}{more}", plan.skipped.len()),
                inner.width as usize,
            ))
            .style(Style::default().fg(Color::Rgb(220, 180, 120))),
            Rect::new(inner.x, inner.y + inner.height.saturating_sub(2), inner.width, 1),
        );
    }

    f.render_widget(
        Paragraph::new(tr(
            lang,
            " Space=toggle  a=all  f=this file  Enter=write  Esc=cancel ",
            " Space=切替  a=全部  f=このファイル  Enter=書き込み  Esc=取消 ",
        ))
        .style(Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)),
        Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
    );
}

fn draw_find_results(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    find: Option<(&str, &str, Option<cian_core::search::Outcome>, cian_core::search::Mode)>,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let accent = popup_accent(popup);
    let Popup::FindResults { hits, cursor, scroll, by_ai } = popup else { return };
    let by_ai = *by_ai;
    let w = 96u16.min(area.width.saturating_sub(2));
    let h = area.height.saturating_sub(4).max(8);
    // The AI's semantic search lands in this same list; name it for the menu
    // item that produced it rather than for the `:find` state, which belongs to
    // whatever sweep ran last.
    let title = if by_ai {
        if lang == Lang::Ja {
            format!(" セマンティック検索 — {} 件 ", hits.len())
        } else {
            format!(" Semantic search — {} found ", hits.len())
        }
    } else {
        match find {
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
        }
    };
    let inner = popup_frame_in(
        f,
        area,
        w,
        h,
        truncate_middle(&title, w.saturating_sub(4) as usize),
        "",
        accent,
    );

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
                    truncate(&crate::util::plain(text), avail.saturating_sub(loc_w)),
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
        Paragraph::new(tr(lang, " Enter=go  r=replace all  p=panelize  j/k=move  Esc=close ", " Enter=移動  r=一括置換  p=ペイン化  j/k=カーソル  Esc=閉じる ")).style(
            Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD),
        ),
        Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
    );
}

fn draw_shortcuts(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::Shortcuts { entries, cursor, path } = popup else { return };
    let level = sc_level(entries, path);
    // Wide, because these are paths and URLs; the generic 70-column popup
    // wrapped them across lines, which made the list unreadable.
    let w = 96u16.min(area.width.saturating_sub(2));
    let h = (level.len() as u16 + 5).max(8).min(area.height.saturating_sub(2));
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
    let inner = popup_frame(f, area, w, h, title, "");

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
}

fn draw_history(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::History { entries, cursor } = popup else { return };
    // Its own renderer rather than the plain-text popup, so the selected
    // row gets the same highlight bar the shortcuts list has.
    let w = 96u16.min(area.width.saturating_sub(2));
    let h = (entries.len() as u16 + 5).max(6).min(area.height.saturating_sub(2));
    let inner = popup_frame(f, area, w, h, format!(" {} ({}) ", tr(lang, "history", "履歴"), entries.len()), "");

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
}

fn draw_dest_picker(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    dests: &[(String, PathBuf)],
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::DestPicker { op, targets, cursor } = popup else { return };
    let rows = dests.len();
    let w = 84u16.min(area.width.saturating_sub(2));
    let h = (rows as u16 + 6).min(area.height.saturating_sub(2));
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
    let inner = popup_frame(f, area, w, h, dp_title, "");

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
}

/// The outline column down the left of the viewer.
///
/// The highlighted entry is the one the cursor is *inside*, not the one it is
/// on: scrolling through a function body should keep saying which function
/// that is, which is the whole reason to give up the screen width.
fn draw_outline_column(
    f: &mut Frame,
    area: Rect,
    items: &[cian_core::outline::Item],
    line: usize,
) {
    use cian_core::outline::Kind;
    if area.width == 0 || area.height == 0 {
        return;
    }
    // Paint the column's own background first. A `Paragraph` writes only the
    // characters it has, so a row that is now short — or empty — would keep
    // whatever the last frame left in the cells beyond it.
    f.render_widget(Block::default().style(Style::default().bg(surface())), area);
    let here = items.iter().rposition(|i| i.line <= line);
    // Scroll the list so the current entry stays visible in a long file.
    let h = area.height as usize;
    let top = outline_top(items, line, h);
    for (row, (i, item)) in items.iter().enumerate().skip(top).take(h).enumerate() {
        let y = area.y + row as u16;
        let cur = here == Some(i);
        let colour = match item.kind {
            Kind::Heading => Color::Rgb(150, 190, 250),
            Kind::Type => Color::Rgb(230, 200, 140),
            Kind::Function => Color::Rgb(170, 220, 175),
            Kind::Section => Color::Rgb(190, 175, 220),
        };
        let indent = "  ".repeat(item.level.min(4));
        let text = format!("{indent}{}", item.text);
        let mut style = Style::default().fg(if cur { colour } else { dim_of(colour) });
        if cur {
            style = style.add_modifier(Modifier::BOLD);
        }
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(if cur { "▎" } else { " " }, Style::default().fg(colour)),
                Span::styled(truncate(&text, area.width.saturating_sub(1) as usize), style),
            ])),
            Rect::new(area.x, y, area.width, 1),
        );
    }
}

/// Pull a colour back towards the background, for the entries that are not
/// the current one — the same hue, so the kind is still readable at a glance.
fn dim_of(c: Color) -> Color {
    match c {
        Color::Rgb(r, g, b) => Color::Rgb(r / 2 + 30, g / 2 + 30, b / 2 + 35),
        other => other,
    }
}


/// A shade of the current surface, `amount` steps away from it.
///
/// Away, not darker: on a dark theme that means lighter and on a light theme
/// darker, so a tint meant to be *noticed* stays noticeable and a tint meant
/// to sit *under* the text never swallows it. Fixed dark values did the second
/// of those on a light theme, which is where this came from.
pub(crate) fn shade_of_surface(amount: i16) -> Color {
    let (r, g, b) = match surface() {
        Color::Rgb(r, g, b) => (r as i16, g as i16, b as i16),
        _ => (30, 30, 40),
    };
    // In i32: 299 × 255 alone overflows an i16, and a panic here would take
    // the program down every frame.
    let lum = (299 * r as i32 + 587 * g as i32 + 114 * b as i32) / 1000;
    let step = if lum > 140 { -amount } else { amount };
    let clamp = |v: i16| v.saturating_add(step).clamp(0, 255) as u8;
    Color::Rgb(clamp(r), clamp(g), clamp(b))
}

/// Tint the cursor's line.
///
/// A background rather than a colour change, so it sits under the syntax
/// highlighting instead of arguing with it — the tint says which line you are
/// on, and the colours go on saying what the text is. There is no matching
/// stripe down the column: the ruler already marks it, and a full-height bar
/// through the text costs more reading than it repays.
fn cross(base: Style, on_line: bool) -> Style {
    if on_line {
        base.bg(shade_of_surface(28))
    } else {
        base
    }
}

/// The two halves of a split viewer. The focused one comes first.
fn split_viewer_areas(area: Rect, left_right: bool) -> (Rect, Rect) {
    if left_right {
        let w = area.width / 2;
        (
            Rect::new(area.x, area.y, w, area.height),
            Rect::new(area.x + w, area.y, area.width - w, area.height),
        )
    } else {
        let h = area.height / 2;
        (
            Rect::new(area.x, area.y, area.width, h),
            Rect::new(area.x, area.y + h, area.width, area.height - h),
        )
    }
}

fn draw_viewer(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    lang: Lang,
    // The two things the viewer draws over the text but does not hold itself:
    // the invisible-character marks and the ruler with its crosshair.
    marks: (bool, bool),
    msg: Option<&str>,
    // Which of the viewer's open files this is, what they are all called, and
    // — when a comparison is running — what each line of *this* half is.
    tab: (usize, &[String], &[cian_core::diff::Mark]),
) -> Vec<(Rect, usize)> {
    let (show_ws, ruler) = marks;
    let (tab_at, tab_names, diff_marks) = tab;
    let tabs = tab_names.len();
    let mut tab_rects: Vec<(Rect, usize)> = Vec::new();
    let Popup::Viewer { title, view, scroll, line, col, visual, anchor, find_input, find_query, sub_input, sub_walk, block_input, git_lines, markdown, preview, source, md_styles, md_map, md_width, editing, dirty, editable, hl, hl_lang, blame, shape, path, .. } = popup else { return tab_rects };
    let rect = viewer_frame_rect(area);
    f.render_widget(Clear, rect);

    // The preview owns `view.lines`: render the source to plain text plus a
    // parallel per-character style grid at the current width and swap it in;
    // leaving preview (or a width change) restores/re-wraps. Everything below
    // — cursor, visual selection, `/` search, the mouse — then works over
    // whichever text is on screen.
    let inner_w = rect.width.saturating_sub(4).max(1);
    if *preview {
        if md_styles.is_empty() || *md_width != inner_w {
            let (plain, styles, map) = crate::markdown::render_styled(source, inner_w as usize);
            view.lines = plain;
            *md_styles = styles;
            *md_map = map;
            *md_width = inner_w;
        }
    } else if !md_styles.is_empty() {
        view.lines = source.clone();
        md_styles.clear();
        md_map.clear();
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

    // An edit changes the file's shape. Re-read it when the buffer has moved
    // on — but not while typing, where the outline would flicker on every
    // keystroke and the fold under the cursor could vanish mid-word.
    if !*editing && !*preview {
        let now = crate::fingerprint(&view.lines);
        if shape.as_deref().is_some_and(|s| s.fp != now) {
            *shape = crate::Shape::read(path, &view.lines, shape.as_deref());
        }
    }

    // Where the cursor is drawn, in columns — what the ruler measures and what
    // the crosshair has to agree with. Characters and columns part company the
    // moment a line has Japanese in it.
    let cur_col = view
        .lines
        .get(*line)
        .map(|l| {
            l.chars()
                .take(*col)
                .fold(0usize, |at, c| at + cian_core::textops::char_cols(c, at))
        })
        .unwrap_or(0);

    let kind = match view.kind {
        cian_core::viewer::ViewKind::Text => view.encoding.label(),
        cian_core::viewer::ViewKind::Binary => "binary",
    };
    let size = cian_core::human_size(view.total_bytes);
    let cut = if view.truncated { "  (first 4M shown)" } else { "" };
    // A little mode badge in the title, so which visual mode is active — and
    // where the cursor sits — is never a guess.
    // The viewer says what mode it is in the way the file panes do: a word and
    // a colour, on the border as well as in the chip. Reading, selecting and
    // editing are three quite different things to have a keyboard pointed at,
    // and a badge alone is easy to have not looked at.
    // Typing at a prompt is its own mode and takes the frame, exactly as it
    // does in the file panes and in the same colours — otherwise `:` and `i`
    // begin the same way on screen while meaning opposite things.
    let (mode, mode_color) = if sub_input.is_some() {
        ("COMMAND", Color::Rgb(200, 100, 200))
    } else if find_input.is_some() {
        ("SEARCH", Color::Rgb(80, 200, 120))
    } else if *editing {
        // Not orange: the selecting modes are orange, and "the next key goes
        // into the file" is the one state worth never mistaking.
        ("EDIT", Color::Rgb(235, 105, 105))
    } else {
        match visual {
            None => ("READ", theme().accent),
            Some(ViewVisual::Char) => ("VISUAL", Color::Rgb(255, 140, 0)),
            Some(ViewVisual::Line) => ("V-LINE", Color::Rgb(255, 140, 0)),
            Some(ViewVisual::Block) => ("V-BLOCK", Color::Rgb(255, 175, 60)),
        }
    };
    let dirty_mark = if *dirty { " ●" } else { "" };
    // The BOM is invisible in the text, which is exactly why it gets a badge:
    // three unseen bytes at the top of a script are a classic breakage.
    let bom_mark = if view.bom {
        match view.encoding {
            cian_core::viewer::TextEncoding::Utf8 => " · UTF-8 BOM",
            _ => " · BOM",
        }
    } else {
        ""
    };
    // The line ending is as invisible as the BOM and just as easy to convert
    // by accident, so it gets the same treatment: shown, and only changed on
    // purpose (`:lf` / `:crlf`).
    // The line ending, with the arrow the marks would draw, so the badge and
    // the text agree about which is which.
    let eol_mark = if view.kind == cian_core::viewer::ViewKind::Text {
        let arrow = match view.eol {
            cian_core::viewer::Eol::Crlf => "↵",
            cian_core::viewer::Eol::Cr => "←",
            cian_core::viewer::Eol::Lf => "↓",
        };
        format!(" · {} {}", view.eol.label(), arrow)
    } else {
        String::new()
    };
    let head = if *preview {
        tr(lang, "Markdown preview", "Markdown プレビュー").to_string()
    } else {
        format!("{}, {}{}{}{}", kind, size, cut, bom_mark, eol_mark)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(mode_color).add_modifier(Modifier::BOLD))
        // The viewer takes the theme's own surface (light on a light theme),
        // so it truly follows the theme; its text uses readable_on below.
        .style(Style::default().bg(surface()))
        .title(if tabs > 1 {
            // With several files open, which one this is matters more than how
            // big it is: the count replaces the size, and the name keeps its
            // dirty mark.
            // The two arrows come first, at a fixed column, so the mouse can
            // find them without the file name's length coming into it — the
            // same shape the file panes' history arrows have.
            // A strip, as the shell panel has: every open file named, the one
            // being read picked out. The two arrows come first at a fixed
            // column so the mouse can find them whatever the names are.
            let mut spans = vec![Span::styled(
                " ◂ ▸ ".to_string(),
                Style::default().fg(theme().accent).add_modifier(Modifier::BOLD),
            )];
            let mut at = rect.x + 1 + 5;
            for (i, name) in tab_names.iter().enumerate() {
                let label = format!(" {} {} ", i + 1, truncate(name, 18));
                let w = width(&label) as u16;
                tab_rects.push((Rect::new(at, rect.y, w, 1), i));
                at += w;
                spans.push(Span::styled(
                    label,
                    if i == tab_at {
                        Style::default().fg(Color::Black).bg(mode_color).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(150, 150, 170))
                    },
                ));
            }
            if *dirty {
                spans.push(Span::styled(" ●".to_string(), Style::default().fg(Color::Rgb(240, 200, 120))));
            }
            Line::from(spans)
        } else {
            Line::from(format!(" {}{}  —  {} ", title, dirty_mark, head))
        })
        .title_bottom(Line::from(vec![
            Span::styled(
                format!(" {} ", mode),
                Style::default().fg(Color::Black).bg(mode_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                // The column, counted as the screen counts it — the same
                // number the ruler marks. A character count would disagree
                // with the ruler on any line with Japanese in it, and the
                // column is what a fixed-width record is about anyway.
                format!(" {}:{} ", *line + 1, cur_col + 1),
                Style::default().fg(mode_color),
            ),
        ]));
    let whole = rect.inner(Margin { vertical: 1, horizontal: 2 });
    f.render_widget(block, rect);

    // The outline takes a column off the left, but only when there is room
    // for both it and a usable amount of text — on a narrow terminal the file
    // is what you came for.
    let outline_w = shape.as_deref().map_or(0, |s| outline_width(whole.width, s.shown, s.items.len()));
    let inner = Rect::new(whole.x + outline_w, whole.y, whole.width - outline_w, whole.height);

    // The ruler is only for reading a fixed-width record, which the rendered
    // Markdown is not, and it costs a row.
    let show_ruler = ruler && !*preview && view.kind == cian_core::viewer::ViewKind::Text;
    let body_h = inner.height.saturating_sub(1 + u16::from(show_ruler)) as usize;
    // Closed folds take their lines out of the picture entirely: `visible` is
    // the buffer as it is actually shown, and everything below — scrolling,
    // the cursor, the mouse — works over that rather than over raw line
    // numbers, so a fold cannot leave a gap or a cursor stranded off screen.
    // Not while editing: folding is a reading aid, and hiding lines from
    // someone who is typing into the file is a good way to lose an edit into
    // a region they cannot see.
    let folded = shape
        .as_deref()
        .filter(|_| !*preview && !*editing)
        .map(|s| s.hidden(view.lines.len()))
        .unwrap_or_default();
    // The cursor never sits inside a closed fold; it sits on the heading that
    // closed. Doing it here catches every way the cursor can move — a search
    // hit, a `G`, a grep jump — instead of one arm at a time.
    if !folded.is_empty() && folded.get(*line).copied().unwrap_or(false) {
        if let Some(h) = shape.as_deref().and_then(|s| s.enclosing_fold(*line, view.lines.len())) {
            *line = h;
            *col = 0;
        }
    }
    let visible: Vec<usize> = if folded.is_empty() {
        (0..view.lines.len()).collect()
    } else {
        (0..view.lines.len()).filter(|i| !folded[*i]).collect()
    };
    // `scroll` stays a real line number — it is the file's position, and the
    // percentage in the corner would otherwise lie — so it is converted to and
    // from an index into `visible` around the clamping.
    let vpos = |l: usize| visible.partition_point(|v| *v < l);
    let cur_v = vpos(*line);
    let mut top_v = vpos(*scroll);
    let max_top = visible.len().saturating_sub(body_h);
    top_v = top_v.min(max_top);
    if cur_v < top_v {
        top_v = cur_v;
    } else if cur_v >= top_v + body_h.max(1) {
        top_v = cur_v + 1 - body_h.max(1);
    }
    *scroll = visible.get(top_v).copied().unwrap_or(0);
    let max_scroll = max_top;

    // Line numbers and the git change bar belong to the source only; the
    // rendered preview is a document, not a file listing. The blame gutter,
    // when on, takes the left column instead of line numbers.
    let show_blame = !blame.is_empty() && !*preview && !*editing;
    let numbered = !*preview && !show_blame && view.kind == cian_core::viewer::ViewKind::Text;
    // One column for the fold markers, present on every numbered line whether
    // or not that line folds: a gutter that changes width per line would
    // stagger the text and break the mouse's column mapping.
    let fold_col = usize::from(numbered && shape.as_deref().is_some_and(|s| !s.items.is_empty()));
    let gutter = if show_blame {
        BLAME_W
    } else if numbered {
        format!("{}", view.lines.len()).len().max(3) + 1 + fold_col
    } else {
        0
    };
    let avail = (inner.width as usize).saturating_sub(gutter);

    // Ordered selection endpoints, for the highlight geometry.
    let (s0, e0) = order_pos(*anchor, (*line, *col));
    let sel_bg = Style::default().bg(theme().selected_bg);
    // The page's own two colours, swapped. Not `REVERSED`, which swaps
    // whatever is underneath — on a tinted line that is the tint, and the
    // cursor came out as a smudge the same shade as the line it was on.
    let cursor_style = Style::default().fg(surface()).bg(readable_on(surface()));
    let search_bg = Style::default().bg(Color::Rgb(120, 100, 0)).fg(Color::Rgb(255, 240, 190));
    // Body text adapts to the (themed) surface so it reads on light themes.
    let text_fg = readable_on(surface());
    // Character columns matched by the active search, per line, for highlight.
    // Compiled once per frame; the same `/re/`-or-literal language as n/N uses
    // (util::viewer_find), so what glows is exactly what n lands on.
    let matcher = find_query
        .as_ref()
        .filter(|q| !q.is_empty())
        .and_then(|q| cian_core::search::Matcher::parse(q).ok());
    let match_cols = |l: &str| -> Vec<(usize, usize)> {
        let Some(m) = matcher.as_ref() else { return Vec::new() };
        // find_ranges is end-exclusive; the highlight loop below wants
        // inclusive ends.
        m.find_ranges(l).into_iter().map(|(s, e)| (s, e.saturating_sub(1).max(s))).collect()
    };

    // The inclusive selected column range on absolute line `i`, if any.
    let sel_cols = |i: usize, len: usize| -> Option<(usize, usize)> {
        match visual {
            None => None,
            Some(ViewVisual::Line) => {
                if i >= s0.0 && i <= e0.0 { Some((0, len)) } else { None }
            }
            // The block is a rectangle in *columns*, so which characters of
            // this line it covers depends on how wide this line's characters
            // are. Asking the block itself keeps the highlight and the edit
            // agreeing about where the rectangle is.
            Some(ViewVisual::Block) => {
                if i >= s0.0 && i <= e0.0 {
                    let b = cian_core::textops::Block::between(&view.lines, *anchor, (*line, *col));
                    let (from, to) = b.char_range(view.lines.get(i).map(|s| s.as_str()).unwrap_or(""));
                    (to > from).then(|| (from, to - 1))
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

    let rows: Vec<Line> = visible
        .iter()
        .skip(top_v)
        .take(body_h)
        .map(|i| (*i, &view.lines[*i]))
        .map(|(i, l)| {
            // The buffer keeps real tabs; the screen cannot show one. Each
            // buffer character is drawn as whatever it looks like — a tab as
            // the spaces up to the next stop — while every column reckoning
            // below (the cursor, the selection, the highlighter, a search hit)
            // stays in buffer characters. Expanding the string first, as this
            // used to, meant the file was saved back with its tabs already
            // spent.
            let marks = show_ws && !*preview;
            let trail_from = if marks {
                l.chars().count() - l.chars().rev().take_while(|c| *c == ' ').count()
            } else {
                usize::MAX
            };
            // `w` is how many columns this character will take, worked out by
            // the same function the block selection and the mouse use.
            let shown = |j: usize, ch: char, w: usize| -> String {
                match ch {
                    '\t' if marks => format!("→{}", " ".repeat(w.saturating_sub(1))),
                    '\t' => " ".repeat(w),
                    // An ideographic space is the one that breaks YAML and
                    // shell scripts while looking exactly like nothing.
                    '\u{3000}' if marks => "□".to_string(),
                    // Only the *trailing* half-width spaces. Dotting every
                    // gap between words makes prose unreadable, and the ones
                    // that matter — the invisible difference between a line
                    // that ends cleanly and one that does not — are at the end.
                    ' ' if j >= trail_from => "·".to_string(),
                    other => other.to_string(),
                }
            };
            // Take buffer characters until their *drawn* width fills the row.
            let mut chars: Vec<char> = Vec::new();
            let mut drawn = 0usize;
            for ch in l.chars() {
                let w = cian_core::textops::char_cols(ch, drawn);
                if drawn + w > avail {
                    break;
                }
                drawn += w;
                chars.push(ch);
            }
            let len = chars.len();
            let sel = sel_cols(i, len);
            // While hex-editing, `col` holds a nibble index (0..32); map it to
            // the dump's on-screen column: offset(8) + 2 spaces, 3 cells per
            // byte, one extra gap after byte 8.
            let cur = if i == *line {
                if *editing && view.kind == cian_core::viewer::ViewKind::Binary {
                    let nib = (*col).min(31);
                    let byte = nib / 2;
                    Some(10 + byte * 3 + usize::from(byte >= 8) + nib % 2)
                } else {
                    Some(*col)
                }
            } else {
                None
            };
            let matches = match_cols(l);
            // The line the cursor is on, and the column it is in, tinted so
            // both can be followed across a wide record without a finger on
            // the screen. Underneath everything that says more than "you are
            // here" — a selection, a search hit, the cursor itself.
            let cross_line = ruler && !*preview && i == *line;
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
                    let base = hl
                        .get(i)
                        .and_then(|s| s.get(j))
                        .copied()
                        .unwrap_or(Style::default().fg(text_fg));
                    cross(base, cross_line)
                } else {
                    cross(Style::default().fg(text_fg), cross_line)
                }
            };
            // Build the body char-by-char, merging same-styled runs.
            let mut spans: Vec<Span> = Vec::new();
            if show_blame {
                // "hash author……" per line, dimmed; a run of the same commit
                // reads as one block.
                let (hash, who) = blame
                    .get(i)
                    .map(|b| (b.hash.as_str(), b.author.as_str()))
                    .unwrap_or(("", ""));
                let who: String = who.chars().take(11).collect();
                let same_as_prev = i > 0 && blame.get(i - 1).map(|p| p.hash.as_str()) == Some(hash);
                let (shown_hash, shown_who) = if same_as_prev {
                    (String::new(), String::new()) // repeat block: leave blank
                } else {
                    (hash.to_string(), who)
                };
                spans.push(Span::styled(
                    format!("{:<7} {:<11} ", shown_hash, shown_who),
                    Style::default().fg(Color::Rgb(120, 120, 145)),
                ));
            }
            if numbered {
                // The line number, then a 1-column separator that doubles as
                // the git change bar (green added / amber modified / red for
                // a deletion just above). Keeping the width fixed means the
                // mouse column mapping is unaffected.
                spans.push(Span::styled(
                    format!("{:>w$}", i + 1, w = gutter.saturating_sub(1 + fold_col)),
                    Style::default().fg(Color::Rgb(110, 110, 135)),
                ));
                // A heading with something under it says so, and says whether
                // it is open. The marker is also the click target.
                if fold_col == 1 {
                    let sh = shape.as_deref();
                    let foldable = sh.is_some_and(|s| s.extent_at(i, view.lines.len()).is_some());
                    let shut = foldable && sh.is_some_and(|s| s.folds.contains(&i));
                    spans.push(Span::styled(
                        if !foldable { " " } else if shut { "▸" } else { "▾" },
                        Style::default().fg(if shut { theme().accent } else { Color::Rgb(110, 110, 135) }),
                    ));
                }
                // The 1-column separator (previously a plain space) is the
                // change bar.
                // A live comparison takes this column while it is running: it
                // is the more urgent of the two answers, and they are both
                // "how does this line differ from something".
                let (bar, bar_c) = match diff_marks.get(i) {
                    Some(cian_core::diff::Mark::Changed) => ("▌", Color::Rgb(240, 210, 120)),
                    Some(cian_core::diff::Mark::Only) => ("▌", Color::Rgb(130, 205, 150)),
                    _ => match git_lines.get(&i) {
                        Some(cian_core::git::LineChange::Added) => ("▏", Color::Rgb(130, 205, 150)),
                        Some(cian_core::git::LineChange::Modified) => ("▏", Color::Rgb(240, 210, 120)),
                        Some(cian_core::git::LineChange::DeletedBefore) => ("▁", Color::Rgb(230, 120, 120)),
                        None => (" ", Color::Reset),
                    },
                };
                spans.push(Span::styled(bar.to_string(), Style::default().fg(bar_c)));
            }
            let mut run = String::new();
            let mut run_style = cell_style(0);
            let mut at = 0usize; // drawn column, so a tab knows its own stop
            for (j, ch) in chars.iter().enumerate() {
                let st = cell_style(j);
                if st != run_style && !run.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut run), run_style));
                }
                run_style = st;
                let w = cian_core::textops::char_cols(*ch, at);
                let text = shown(j, *ch, w);
                at += w;
                run.push_str(&text);
            }
            if !run.is_empty() {
                spans.push(Span::styled(run, run_style));
            }
            // The cursor can sit just past the last char (empty line, or end
            // of line): show it as a reversed space so it stays visible.
            if cur == Some(len) {
                spans.push(Span::styled(" ".to_string(), cursor_style));
            }
            // A tint that stops where the text stops is not a line highlight;
            // it is a highlight on some words. Carry it to the edge.
            if cross_line {
                let used = at + usize::from(cur == Some(len));
                if used < avail {
                    spans.push(Span::styled(
                        " ".repeat(avail - used),
                        cross(Style::default(), true),
                    ));
                }
            }
            // With the marks on, the line ending is drawn too, and the two
            // kinds look different — which is the point. A file that is CRLF
            // except for three lines is not something a badge in the title can
            // tell you, and it is exactly the file that causes trouble.
            if marks && len == l.chars().count() {
                spans.push(Span::styled(
                    match view.eol {
                        // One glyph each, the way Sakura draws them: a bent
                        // arrow for a carriage return, a straight one for a
                        // line feed. Two glyphs for CRLF said the same thing
                        // twice and cost a column.
                        cian_core::viewer::Eol::Crlf => "↵",
                        cian_core::viewer::Eol::Cr => "←",
                        cian_core::viewer::Eol::Lf => "↓",
                    },
                    Style::default().fg(Color::Rgb(110, 140, 175)),
                ));
            }
            Line::from(spans)
        })
        .collect();
    let top = inner.y + u16::from(show_ruler);
    let body_area = Rect::new(inner.x, top, inner.width, body_h as u16);
    f.render_widget(Paragraph::new(rows), body_area);
    if show_ruler {
        // A scale over the text, starting where the text starts: every tenth
        // column numbered, every fifth marked. Counting characters by eye is
        // exactly what it is here to stop.
        let mut scale = String::with_capacity(avail);
        while scale.chars().count() < avail {
            let c = scale.chars().count() + 1; // 1-based, as the corner reads
            scale.push(match c {
                _ if c % 10 == 0 => char::from_digit((c / 10 % 10) as u32, 10).unwrap_or('|'),
                _ if c % 5 == 0 => '+',
                _ => '·',
            });
        }
        // Split by characters, not bytes: the scale is made of `·`, which is
        // two bytes wide, so a column number used as a byte offset lands
        // inside one and takes the program with it.
        let marks: Vec<char> = scale.chars().collect();
        // The scale counts *display* columns, so the mark has to be where the
        // cursor is drawn rather than how many characters precede it. On a
        // line of Japanese those are different numbers, and the ruler was
        // pointing at neither the right column nor a useful one.
        let cur = cur_col.min(marks.len().saturating_sub(1));
        let before: String = marks[..cur.min(marks.len())].iter().collect();
        let at: String = marks.get(cur).into_iter().collect();
        let after: String = marks[(cur + 1).min(marks.len())..].iter().collect();
        let dim = Style::default().fg(Color::Rgb(105, 105, 130));
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ".repeat(gutter), dim),
                Span::styled(before, dim),
                // Where the cursor is, in the scale as well as in the text.
                Span::styled(at, Style::default().fg(Color::Black).bg(mode_color)),
                Span::styled(after, dim),
            ])),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
    }
    if outline_w > 0 {
        if let Some(sh) = shape.as_deref() {
            draw_outline_column(
                f,
                Rect::new(whole.x, whole.y, outline_w.saturating_sub(1), body_h as u16),
                &sh.items,
                src_line(md_map, *line),
            );
        }
    }
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
    } else if let Some(w) = sub_walk {
        // The decision prompt names the change and the progress, so neither
        // has to be held in the head while answering.
        let h = &w.hits[w.idx.min(w.hits.len().saturating_sub(1))];
        let shorten = |s: &str| truncate(s, 24);
        format!(
            "{}  {} → {}   [{}/{}]",
            tr(lang, " replace?  y yes   n no   a all   q stop ", " 置換?  y はい   n いいえ   a 残り全部   q 中止 "),
            shorten(&h.from),
            shorten(&h.to),
            w.idx + 1,
            w.hits.len(),
        )
    } else if let Some(b) = block_input {
        let what = match b.kind {
            crate::BlockEdit::Insert => tr(lang, "insert ▏", "左端に挿入 ▏"),
            crate::BlockEdit::Append => tr(lang, "append ▕", "右端に追記 ▕"),
            crate::BlockEdit::Replace => tr(lang, "replace ▊", "矩形を置換 ▊"),
            crate::BlockEdit::LineStart => tr(lang, "line start ▏", "各行の先頭 ▏"),
            crate::BlockEdit::LineEnd => tr(lang, "line end ▕", "各行の末尾 ▕"),
        };
        // A line selection has no column to report; a rectangle does.
        let ragged = matches!(b.kind, crate::BlockEdit::LineStart | crate::BlockEdit::LineEnd);
        let rows = b.block.bottom - b.block.top + 1;
        format!(
            "{} {}_   {}",
            what,
            b.text,
            match (ragged, lang == Lang::Ja) {
                (true, true) => format!("({rows} 行)"),
                (true, false) => format!("({rows} lines)"),
                (false, true) => format!("({rows} 行, {} 桁目)", b.block.left + 1),
                (false, false) => format!("({rows} lines, col {})", b.block.left + 1),
            }
        )
    } else if let Some(cmd) = sub_input {
        // What the prompt takes, shown rather than assumed: the replace form
        // first, then the word commands, because a blank prompt with no menu
        // is a prompt you have to have read the manual to use.
        // The menu is for someone who has not started yet. Once there is
        // something typed, it is the typed text that has to be readable, and
        // a wall of vocabulary beside it is only in the way.
        if cmd.is_empty() {
            format!(
                ":_   {}",
                tr(lang,
                   "s/old/new/[gci] · w wq q q! · preview block outline ws sort uniq han zen expand[ all] unexpand reindent lf crlf",
                   "s/old/new/[gci] · w wq q q! · preview block outline ws sort uniq han zen expand[ all] unexpand reindent lf crlf"),
            )
        } else if cmd.starts_with('s') {
            // Mid-replace, the flags are the part still to be decided — and
            // the whole reason `r` seeded the prompt was so they would be.
            format!(
                ":{}_   {}",
                cmd,
                tr(lang,
                   "flags: g all on a line · c confirm each · i ignore case",
                   "フラグ: g 行内すべて · c 1件ずつ確認 · i 大小無視"),
            )
        } else {
            format!(":{}_", cmd)
        }
    } else {
        match find_input {
            Some(q) => format!("/{}_", q),
            None => {
                let mmd = source.iter().any(|l| {
                    let t = l.trim_start();
                    (t.starts_with("```") || t.starts_with("~~~"))
                        && t.trim_start_matches(['`', '~']).trim().eq_ignore_ascii_case("mermaid")
                });
                // `]]` and `[[` only mean something when the file has a shape,
                // and folding only in the source; offering either otherwise is
                // a hint that answers a question nobody can ask.
                let has_shape = shape.as_deref().is_some_and(|s| !s.items.is_empty());
                let shape_hint = match (has_shape, *preview) {
                    (false, _) => "",
                    (true, true) => tr(lang, " ]] [[ section ", " ]] [[ 見出し "),
                    (true, false) => tr(lang, " ]] [[ section  Space fold  zA all ", " ]] [[ 見出し  Space 折りたたみ  zA 全部 "),
                };
                // `r` only means something once there is something to replace.
                let after_find = if find_query.is_some() {
                    tr(lang, " r replace ", " r 置換 ")
                } else {
                    ""
                };
                let hints = if *preview {
                    format!("{}{}{}{}{}{}",
                        tr(lang, " / f search  n/N  v/V select  y copy ", " / f 検索  n/N  v/V 選択  y コピー "),
                        after_find,
                        ed,
                        shape_hint,
                        if mmd { tr(lang, " m diagram ", " m 図 ") } else { "" },
                        tr(lang, " :preview source  ", " :preview ソース  "))
                } else if *markdown {
                    format!("{}{}{}{}{}",
                        tr(lang, " / f search  n/N  v/V select  y copy ", " / f 検索  n/N  v/V 選択  y コピー "),
                        after_find,
                        ed,
                        shape_hint,
                        tr(lang, " p paste  :preview  ", " p 貼付  :preview  "))
                } else {
                    format!("{}{}{}{}{}",
                        tr(lang, " / f search  n/N  v/V select  y copy  p paste ", " / f 検索  n/N  v/V 選択  y コピー  p 貼付 "),
                        after_find,
                        ed,
                        shape_hint,
                        tr(lang, " e enc  ", " e 文字コード  "))
                };
                format!("{}{} ", hints, pos)
            }
        }
    };
    // A message the viewer itself raised takes the footer while it lasts. The
    // hints are always one keystroke away; "nothing to fold here" answers a
    // key that was just pressed, and belongs where that key was aimed.
    let prompt_up = sub_input.is_some() || find_input.is_some() || block_input.is_some();
    let (footer, footer_style) = match msg.filter(|_| !*editing && sub_walk.is_none() && !prompt_up) {
        Some(m) => (
            format!(" {m} "),
            Style::default().fg(Color::Black).bg(Color::Rgb(240, 210, 120)).add_modifier(Modifier::BOLD),
        ),
        None => (
            footer,
            Style::default().fg(Color::Black).bg(mode_color).add_modifier(Modifier::BOLD),
        ),
    };
    f.render_widget(
        Paragraph::new(truncate(&footer, inner.width as usize)).style(footer_style),
        Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
    );
    tab_rects
}

fn draw_dir_compare(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::DirCompare { left, right, entries, cursor, scroll, truncated, .. } = popup else { return };
    use cian_core::dirdiff::Status;
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
    let title = format!(" {}  ↔  {}   —   {} ", left, right, counts);
    let (w, h) = (area.width.saturating_sub(2), area.height.saturating_sub(2));
    let inner = popup_frame(f, area, w, h, title, "");

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
    // Two columns with a marker between them, mirroring the file diff: a
    // path sits on the side(s) it exists, so which tree has (or differs on)
    // an entry is read straight down either column.
    let mid = 3usize;
    let col = (inner.width as usize).saturating_sub(mid) / 2;
    for (row, (i, e)) in entries.iter().enumerate().skip(first).take(body_h).enumerate() {
        let sel = i == *cursor;
        let y = inner.y + row as u16;
        let line = Rect::new(inner.x, y, inner.width, 1);
        push_row_zone(zones, inner, y, i);
        if sel {
            f.render_widget(Block::default().style(Style::default().bg(theme().selected_bg)), line);
        }
        let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
        let mut name = e.rel.display().to_string().replace('\\', "/");
        if e.is_dir {
            name.push('/');
        }
        let shown = truncate_middle(&name, col);
        let blank = " ".repeat(col);
        let (mark, mcol, left_txt, right_txt) = match e.status {
            Status::OnlyLeft => ("◀", del, shown.clone(), blank.clone()),
            Status::OnlyRight => ("▶", add, blank.clone(), shown.clone()),
            Status::Differ => ("≠", chg, shown.clone(), shown.clone()),
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(pad_to(&left_txt, col), base.fg(mcol)),
                Span::styled(format!(" {} ", mark), base.fg(mcol).add_modifier(Modifier::BOLD)),
                Span::styled(pad_to(&right_txt, col), base.fg(mcol)),
            ])),
            line,
        );
    }
    f.render_widget(
        Paragraph::new(tr(lang,
            " ◀ left  ▶ right  ≠ differ   Enter=go  </> copy one  [/] sync all  w save  Esc ",
            " ◀ 左  ▶ 右  ≠ 相違   Enter=移動  </> 1件コピー  [/] 一括同期  w 保存  Esc ",
        ))
        .style(Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)),
        Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
    );
}

fn draw_diff(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    lang: Lang,
) {
    let Popup::Diff { left, right, result, folded, fold, scroll, encoding, find, find_input, .. } = popup else { return };
    use cian_core::diff::Row;

    let title = format!(" {} ↔ {}  —  {} ", left, right, cian_core::diff::summary(result));
    let (w, h) = (area.width.saturating_sub(2), area.height.saturating_sub(2));
    let inner = popup_frame(f, area, w, h, title, "");

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
    // The exact edited span within a changed line: a solid bar, the way
    // WinMerge marks the characters that actually differ.
    let chg_hot = Style::default()
        .fg(Color::Black)
        .bg(Color::Rgb(240, 210, 120))
        .add_modifier(Modifier::BOLD);

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

    // A changed line, with its common prefix/suffix left calm and only the
    // edited middle painted as a bar. `prefix`/`suffix` are the shared char
    // counts from `common_affixes`; each side clamps `suffix` to its own
    // length so an insertion (empty middle on one side) stays in bounds.
    let emph_cell = |line: &cian_core::diff::Line, prefix: usize, suffix: usize| -> Vec<Span<'static>> {
        let chars: Vec<char> = line.text.chars().collect();
        let n = chars.len();
        let suffix = suffix.min(n.saturating_sub(prefix));
        let mid_end = n - suffix;
        // Match `cell`'s truncation: keep at most `col` chars, ellipsis when cut.
        let fits = n <= col;
        let budget = if fits { col } else { col.saturating_sub(1) };
        let mut spans = vec![Span::styled(format!("{:>w$} ", line.no, w = gutter - 1), num)];
        let mut buf = String::new();
        let mut buf_hot = false;
        let mut shown = String::new();
        for (i, &c) in chars.iter().take(budget).enumerate() {
            let is_hot = i >= prefix && i < mid_end;
            if !buf.is_empty() && is_hot != buf_hot {
                spans.push(Span::styled(std::mem::take(&mut buf), if buf_hot { chg_hot } else { chg }));
            }
            buf_hot = is_hot;
            buf.push(c);
            shown.push(c);
        }
        if !buf.is_empty() {
            spans.push(Span::styled(buf, if buf_hot { chg_hot } else { chg }));
        }
        if !fits {
            spans.push(Span::styled("…".to_string(), chg));
            shown.push('…');
        }
        let pad = col.saturating_sub(crate::util::width(&shown));
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        spans
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
                    let (p, sfx) = cian_core::diff::common_affixes(&l.text, &rr.text);
                    let mut s = emph_cell(l, p, sfx);
                    s.push(Span::styled(" ~ ", chg.add_modifier(Modifier::BOLD)));
                    s.extend(emph_cell(rr, p, sfx));
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
            tr(lang, "c copy  w save(.html/.md)  e enc  x explain  g/G  Esc",
                  "c コピー  w 保存(.html/.md)  e 文字コード  x 説明  g/G  Esc"),
            encoding.label(),
            pos
        )
    };
    f.render_widget(
        Paragraph::new(footer)
        .style(Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)),
        Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
    );
}

fn draw_archive(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::Archive { path, members, cursor, scroll } = popup else { return };
    let w = 96u16.min(area.width.saturating_sub(2));
    let h = area.height.saturating_sub(4).max(8);
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let total: u64 = members.iter().map(|m| m.size).sum();
    let title =
        format!(" {}  —  {} entries, {} unpacked ", name, members.len(), cian_core::human_size(total));
    let inner = popup_frame(f, area, w, h, title, "");

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
}

fn draw_palette(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    lang: Lang,
) {
    let Popup::Palette { kind, query, items, shown, cursor, scroll } = popup else { return };
    let w = 84u16.min(area.width.saturating_sub(2));
    let h = (area.height.saturating_sub(4)).clamp(6, 22);
    let title = match kind {
        PaletteKind::Commands => tr(lang, " command palette ", " コマンドパレット "),
        PaletteKind::Jump => tr(lang, " jump to ", " ジャンプ "),
        PaletteKind::File => tr(lang, " find file ", " ファイル検索 "),
    };
    let inner = popup_frame(f, area, w, h, title, "");

    // Row 0 is the live query; the list fills the rest above the footer.
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("› ", Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}_", query), Style::default().fg(Color::Rgb(230, 230, 245))),
        ])),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let list_top = inner.y + 1;
    let body_h = inner.height.saturating_sub(2) as usize;
    if *cursor < *scroll {
        *scroll = *cursor;
    } else if body_h > 0 && *cursor >= *scroll + body_h {
        *scroll = *cursor + 1 - body_h;
    }
    for (row, si) in (*scroll..shown.len().min(*scroll + body_h)).enumerate() {
        let idx = shown[si];
        let it = &items[idx];
        let sel = si == *cursor;
        let y = list_top + row as u16;
        let line = Rect::new(inner.x, y, inner.width, 1);
        if sel {
            f.render_widget(Block::default().style(Style::default().bg(theme().selected_bg)), line);
        }
        let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
        let label_w = (inner.width as usize * 2 / 5).max(10);
        let detail_w = (inner.width as usize).saturating_sub(label_w + 4);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(if sel { " ▸ " } else { "   " }, base),
                Span::styled(
                    format!("{:<w$}", truncate(&it.label, label_w), w = label_w),
                    base.fg(if sel { Color::Rgb(235, 235, 250) } else { Color::Rgb(210, 210, 225) })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(truncate_middle(&it.detail, detail_w), base.fg(Color::Rgb(140, 140, 165))),
            ])),
            line,
        );
    }
    if shown.is_empty() {
        f.render_widget(
            Paragraph::new(tr(lang, "  (no matches)", "  （一致なし）")).style(Style::default().fg(Color::Rgb(150, 150, 170))),
            Rect::new(inner.x, list_top, inner.width, 1),
        );
    }
    f.render_widget(
        Paragraph::new(tr(lang, " type to filter   ↑/↓ move   Enter run   Esc close ", " 入力で絞込   ↑/↓ 移動   Enter 実行   Esc 閉じる "))
            .style(Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)),
        Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
    );
}

fn draw_disk_usage(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::DiskUsage { dir, entries, total, cursor, scroll } = popup else { return };
    let w = 96u16.min(area.width.saturating_sub(2));
    let h = area.height.saturating_sub(4).max(8);
    let title = format!(
        " {}  —  {}  ({} items) ",
        truncate_middle(&dir.display().to_string(), 60),
        cian_core::human_size(*total),
        entries.len()
    );
    let inner = popup_frame(f, area, w, h, title, "");

    let body_h = inner.height.saturating_sub(1) as usize;
    if *cursor < *scroll {
        *scroll = *cursor;
    } else if body_h > 0 && *cursor >= *scroll + body_h {
        *scroll = *cursor + 1 - body_h;
    }
    // Bars scale to the biggest child, so the space hog fills the bar.
    let max = entries.first().map(|e| e.size).unwrap_or(0).max(1);
    let bar_w = 18usize;
    for (row, (i, e)) in entries.iter().enumerate().skip(*scroll).take(body_h).enumerate() {
        let sel = i == *cursor;
        let y = inner.y + row as u16;
        let line = Rect::new(inner.x, y, inner.width, 1);
        push_row_zone(zones, inner, y, i);
        if sel {
            f.render_widget(Block::default().style(Style::default().bg(theme().selected_bg)), line);
        }
        let base = if sel { Style::default().bg(theme().selected_bg) } else { Style::default() };
        let filled = ((e.size as u128 * bar_w as u128) / max as u128) as usize;
        let bar: String = "█".repeat(filled) + &"░".repeat(bar_w.saturating_sub(filled));
        let pct = if *total > 0 { e.size as f64 * 100.0 / *total as f64 } else { 0.0 };
        let mut name = e.name.clone();
        if e.is_dir {
            name.push('/');
        }
        let name_w = (inner.width as usize).saturating_sub(bar_w + 24);
        let name_style = if e.is_dir {
            base.fg(FileKind::Directory.color()).add_modifier(Modifier::BOLD)
        } else {
            base.fg(Color::Rgb(225, 225, 240))
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(if sel { " ▸ " } else { "   " }, base),
                Span::styled(format!("{:<w$}", truncate_middle(&name, name_w), w = name_w), name_style),
                Span::styled(bar, base.fg(theme().accent)),
                Span::styled(format!(" {:>8}", cian_core::human_size(e.size)), base.fg(Color::Rgb(210, 210, 225))),
                Span::styled(format!(" {:>4.0}%", pct), base.fg(Color::Rgb(140, 140, 165))),
            ])),
            line,
        );
    }
    if entries.is_empty() {
        f.render_widget(
            Paragraph::new(tr(lang, "  (empty)", "  （空）")).style(Style::default().fg(Color::Rgb(150, 150, 170))),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
    }
    f.render_widget(
        Paragraph::new(tr(lang,
            " Enter=into folder   -=up   j/k move   Esc=close ",
            " Enter=フォルダへ   -=上へ   j/k 移動   Esc=閉じる ",
        ))
        .style(Style::default().fg(Color::Black).bg(theme().accent).add_modifier(Modifier::BOLD)),
        Rect::new(inner.x, inner.y + inner.height.saturating_sub(1), inner.width, 1),
    );
}

fn draw_git_log(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::GitLog { title, commits, cursor, scroll, .. } = popup else { return };
    let rect = centered_rect(area.width.saturating_sub(4), area.height.saturating_sub(4), area);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type())
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::BOLD))
        .title(format!(" {} ", title))
        .title_bottom(tr(lang, " Enter=show diff  j/k  g/G  Esc ", " Enter=差分表示  j/k  g/G  Esc "));
    let inner = rect.inner(Margin { vertical: 1, horizontal: 1 });
    f.render_widget(block, rect);
    let body_h = inner.height as usize;
    if *cursor < *scroll {
        *scroll = *cursor;
    } else if *cursor >= *scroll + body_h {
        *scroll = *cursor + 1 - body_h;
    }
    let hash_w = 8usize;
    let date_w = 10usize;
    let author_w = 14usize;
    let subj_w = (inner.width as usize).saturating_sub(hash_w + date_w + author_w + 3);
    let rows: Vec<Line> = commits
        .iter()
        .enumerate()
        .skip(*scroll)
        .take(body_h)
        .map(|(i, c)| {
            let sel = i == *cursor;
            let author: String = c.author.chars().take(author_w).collect();
            let subject: String = c.subject.chars().take(subj_w).collect();
            let line = format!(
                "{:<hw$} {:<dw$} {:<aw$} {}",
                c.hash, c.date, author, subject,
                hw = hash_w, dw = date_w, aw = author_w,
            );
            let style = if sel {
                Style::default().fg(Color::Black).bg(theme().accent)
            } else {
                Style::default().fg(Color::Rgb(200, 200, 215))
            };
            Line::from(Span::styled(line, style))
        })
        .collect();
    for i in 0..commits.len().min(body_h) {
        push_row_zone(zones, inner, inner.y + i as u16, *scroll + i);
    }
    f.render_widget(Paragraph::new(rows), inner);
}

fn draw_macros(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::Macros { cursor, names } = popup else { return };
    let widest = names.iter().map(|n| n.chars().count()).max().unwrap_or(10);
    let w = (widest as u16 + 8).clamp(28, area.width);
    let h = (names.len() as u16 + 3).min(area.height);
    let inner = popup_frame(f, area, w, h, tr(lang, " run a macro ", " マクロを実行 "), "");

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
}

fn draw_sort_picker(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::SortPicker { cursor } = popup else { return };
    let w = 34u16.min(area.width);
    let h = SortKey::ALL.len() as u16 + 3;
    let inner = popup_frame(f, area, w, h.min(area.height), tr(lang, " sort by ", " 並び替え "), "");

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
}

fn draw_encoding_picker(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::EncodingPicker { cursor, .. } = popup else { return };
    use cian_core::viewer::TextEncoding;
    let w = 34u16.min(area.width);
    let h = TextEncoding::ALL.len() as u16 + 3;
    let inner = popup_frame(f, area, w, h.min(area.height), tr(lang, " text encoding ", " 文字コード "), "");
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
}

fn draw_color_picker(
    f: &mut Frame,
    area: Rect,
    popup: &mut Popup,
    zones: &mut Vec<PopupZone>,
    lang: Lang,
) {
    let Popup::ColorPicker { cursor, .. } = popup else { return };
    let w = 26u16.min(area.width);
    let h = PANE_BG_PRESETS.len() as u16 + 3;
    let inner = popup_frame(f, area, w, h.min(area.height), tr(lang, " background ", " 背景色 "), "");

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
}

#[cfg(test)]
mod md_tests {
    use super::*;

    /// Concatenate a styled run's text back to a plain string.
    fn plain(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn inline_splits_bold_and_code_but_keeps_text() {
        let base = Style::default();
        let spans = md_inline("run `ls -l` then **stop**", base, Color::Rgb(1, 2, 3));
        assert_eq!(plain(&spans), "run ls -l then stop");
        // The code span carries the code colour; the bold span the bold modifier.
        assert!(spans.iter().any(|s| s.style.fg == Some(Color::Rgb(1, 2, 3)) && s.content == "ls -l"));
        assert!(spans.iter().any(|s| s.style.add_modifier.contains(Modifier::BOLD) && s.content == "stop"));
    }

    #[test]
    fn unterminated_markers_stay_literal() {
        let spans = md_inline("a `b and **c", Style::default(), Color::Rgb(0, 0, 0));
        assert_eq!(plain(&spans), "a `b and **c");
    }

    #[test]
    fn body_line_handles_headings_bullets_and_code_fences() {
        let g = Color::Rgb(9, 9, 9);
        let b = Color::Rgb(8, 8, 8);
        let mut in_code = false;

        let head = md_body_line("## Title", 40, g, b, &mut in_code);
        assert_eq!(head.len(), 1);
        assert_eq!(head[0].0, "Title"); // hashes stripped

        let bullet = md_body_line("- item", 40, g, b, &mut in_code);
        assert!(bullet[0].0.starts_with("• "));

        // A fence flips code mode; the line inside is verbatim.
        let _fence = md_body_line("```", 40, g, b, &mut in_code);
        assert!(in_code, "opening fence enters code mode");
        let code = md_body_line("x = **not bold** here", 40, g, b, &mut in_code);
        assert_eq!(code[0].0, "x = **not bold** here");
        let _close = md_body_line("```", 40, g, b, &mut in_code);
        assert!(!in_code, "closing fence leaves code mode");
    }
}
