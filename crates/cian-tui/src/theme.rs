//! The colour theme, border style, interface language (i18n via `tr`), and the
//! remappable `Action` enum — resolved from init.lua and installed into
//! process-wide statics at startup. Split out of lib.rs.

use std::sync::OnceLock;

use ratatui::style::Color;
use ratatui::widgets::BorderType;

/// Resolved color palette. Defaults match the original built-in theme; a
/// `~/.config/cian/init.lua` calling `cian.set_theme{...}` overrides any field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResolvedTheme {
    pub(crate) accent: Color,
    pub(crate) status_bg: Color,
    pub(crate) selected_bg: Color,
    pub(crate) visual_bg: Color,
    pub(crate) mark_fg: Color,
    /// The surface behind panes and the shell. `None` leaves the terminal's own
    /// background showing (the dark default's behaviour); a light theme paints
    /// it so the look holds up on any terminal.
    pub(crate) base_bg: Option<Color>,
    /// Quieter greys for secondary text and borders.
    pub(crate) dim: Color,
    pub(crate) border: Color,
    /// Background of menus and dialogs.
    pub(crate) popup_bg: Color,
    /// File-type accents, indexed by [`FileKind`].
    pub(crate) file: FilePalette,
}

/// The eight file-type accents plus the two neutral tones, kept together so a
/// theme swaps them as a set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FilePalette {
    pub(crate) directory: Color,
    pub(crate) code: Color,
    pub(crate) config: Color,
    pub(crate) document: Color,
    pub(crate) image: Color,
    pub(crate) media: Color,
    pub(crate) archive: Color,
    pub(crate) executable: Color,
    pub(crate) muted: Color,
    pub(crate) plain: Color,
}

impl Default for ResolvedTheme {
    fn default() -> Self {
        Self {
            accent: Color::Cyan, // cian-blue, kept consistent across the app
            status_bg: Color::Rgb(40, 40, 55),
            selected_bg: Color::Rgb(60, 60, 90),
            visual_bg: Color::Rgb(80, 60, 30),
            mark_fg: Color::Yellow,
            base_bg: None,
            dim: Color::Rgb(130, 130, 155),
            border: Color::DarkGray,
            popup_bg: Color::Rgb(24, 24, 34),
            file: FilePalette {
                directory: Color::Rgb(96, 165, 250),
                code: Color::Rgb(250, 204, 21),
                config: Color::Rgb(148, 190, 210),
                document: Color::Rgb(226, 226, 236),
                image: Color::Rgb(216, 130, 220),
                media: Color::Rgb(120, 200, 190),
                archive: Color::Rgb(240, 130, 120),
                executable: Color::Rgb(126, 217, 130),
                muted: Color::Rgb(128, 128, 148),
                plain: Color::Rgb(205, 205, 218),
            },
        }
    }
}

impl ResolvedTheme {
    /// Ethan Schoonover's Solarized Light. The eight accents are unchanged from
    /// the palette (they are tuned to read on the light *and* dark bases); the
    /// neutrals map to base00/base1 for text and base2 for surfaces.
    fn solarized_light() -> Self {
        let base00 = Color::Rgb(0x65, 0x7b, 0x83); // body text
        let base01 = Color::Rgb(0x58, 0x6e, 0x75); // emphasized text
        let base1 = Color::Rgb(0x93, 0xa1, 0xa1); // comments / secondary
        let base2 = Color::Rgb(0xee, 0xe8, 0xd5); // highlighted surface
        let base3 = Color::Rgb(0xfd, 0xf6, 0xe3); // background
        let blue = Color::Rgb(0x26, 0x8b, 0xd2);
        let yellow = Color::Rgb(0xb5, 0x89, 0x00);
        let orange = Color::Rgb(0xcb, 0x4b, 0x16);
        let red = Color::Rgb(0xdc, 0x32, 0x2f);
        let magenta = Color::Rgb(0xd3, 0x36, 0x82);
        let cyan = Color::Rgb(0x2a, 0xa1, 0x98);
        let green = Color::Rgb(0x85, 0x99, 0x00);
        Self {
            accent: blue,
            status_bg: base2,
            selected_bg: Color::Rgb(0xdc, 0xd5, 0xbe), // a touch darker than base2
            visual_bg: Color::Rgb(0xf7, 0xe4, 0xb0), // warm highlight
            mark_fg: orange,
            base_bg: Some(base3),
            dim: base1,
            border: base1,
            // Menus and dialogs stay on Solarized's dark base (base02): a dark
            // panel over the light surface, so their existing light text reads
            // without recoloring every popup.
            popup_bg: Color::Rgb(0x07, 0x36, 0x42),
            file: FilePalette {
                directory: blue,
                code: yellow,
                config: cyan,
                document: base01,
                image: magenta,
                media: cyan,
                archive: red,
                executable: green,
                muted: base1,
                plain: base00,
            },
        }
    }
}

/// Process-wide resolved theme. Set once at startup from the Lua config so the
/// stateless draw helpers can read it without threading it through every call.
static THEME: OnceLock<ResolvedTheme> = OnceLock::new();

pub(crate) fn theme() -> &'static ResolvedTheme {
    THEME.get_or_init(ResolvedTheme::default)
}

/// Which corner glyphs the borders use. Set once at startup; see
/// [`resolve_border_type`].
static BORDERS: OnceLock<BorderType> = OnceLock::new();

pub(crate) fn border_type() -> BorderType {
    *BORDERS.get_or_init(|| resolve_border_type(None))
}

/// Whether Nerd Font glyphs may be used (file icons, branch/disk symbols). Set
/// once at startup from `cian.set_option("nerd_fonts", …)`; defaults to true.
static NERD: OnceLock<bool> = OnceLock::new();

pub(crate) fn nerd_fonts() -> bool {
    *NERD.get_or_init(|| true)
}

/// Pick rounded or square corners.
///
/// Rounded corners are `╭╮╯╰` (U+256D–U+2570), which plenty of console fonts —
/// Consolas and Lucida Console among them — simply do not contain, while the
/// straight `─│` (U+2500, U+2502) are in almost all of them. Windows then
/// font-links just the corners to some other face, whose metrics differ, and
/// the frame looks a few pixels out at each corner while its sides stay put.
///
/// So: square corners in the legacy Windows console, rounded where the
/// terminal is known to cope, and an explicit `borders` option to override.
pub(crate) fn resolve_border_type(configured: Option<&str>) -> BorderType {
    match configured.map(|s| s.trim().to_lowercase()).as_deref() {
        Some("plain") | Some("square") => return BorderType::Plain,
        Some("rounded") => return BorderType::Rounded,
        _ => {}
    }
    if cfg!(windows) && !modern_terminal() {
        BorderType::Plain
    } else {
        BorderType::Rounded
    }
}

/// Whether the host terminal advertises itself as a modern one. The legacy
/// Windows console sets none of these.
pub(crate) fn modern_terminal() -> bool {
    std::env::var_os("WT_SESSION").is_some()
        || std::env::var_os("WEZTERM_PANE").is_some()
        || std::env::var_os("TERM_PROGRAM").is_some()
}

/// Interface language for the key manual / help text. Japanese is the default;
/// `cian.set_option("lang", "en")` switches to English.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ja,
    En,
}

impl Lang {
    /// From the `lang` option; anything but an explicit "ja" is English (the
    /// Lua layer already rejects values other than "ja"/"en").
    pub(crate) fn from_opt(opt: Option<&str>) -> Lang {
        match opt {
            Some("ja") => Lang::Ja,
            _ => Lang::En,
        }
    }

    /// Toggle to the other language.
    pub(crate) fn toggled(self) -> Lang {
        match self {
            Lang::En => Lang::Ja,
            Lang::Ja => Lang::En,
        }
    }
}

/// Pick the English or Japanese form of a fixed UI string.
pub(crate) fn tr(lang: Lang, en: &'static str, ja: &'static str) -> &'static str {
    match lang {
        Lang::En => en,
        Lang::Ja => ja,
    }
}

/// Localize the known progress-operation labels (`start_op`'s first argument).
/// Anything unrecognised (e.g. a directory path) is shown unchanged.
pub(crate) fn tr_op_label(lang: Lang, label: &str) -> String {
    if lang == Lang::En {
        return label.to_string();
    }
    match label {
        "copying" => "コピー中",
        "moving" => "移動中",
        "uploading" => "アップロード中",
        "downloading" => "ダウンロード中",
        "hashing" => "チェックサム計算中",
        "elevating" => "管理者権限で実行中",
        "comparing" => "比較中",
        other => return other.to_string(),
    }
    .to_string()
}

/// The "... and N more" overflow line, localized.
pub(crate) fn tr_count(lang: Lang, more: usize) -> String {
    match lang {
        Lang::En => format!("  ... and {} more", more),
        Lang::Ja => format!("  ... 他 {} 件", more),
    }
}

/// Remappable normal-mode actions. Keys the user binds via `cian.set_keymap`
/// resolve to one of these; the default key handling is otherwise untouched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    CursorDown,
    CursorUp,
    CursorTop,
    CursorBottom,
    PageUp,
    PageDown,
    Parent,
    EnterDir,
    Quit,
    Search,
    SearchNext,
    SearchPrev,
    History,
    Shortcuts,
    Copy,
    Move,
    Delete,
    Rename,
    NewFile,
    NewDir,
    OpenOther,
    OpenOtherTab,
    /// Make the active pane show the other pane's directory (pull).
    SyncFromOther,
    /// Make the other pane show the active pane's directory (push).
    SyncToOther,
    OpenExternal,
    CopyPath,
    CopyFileRef,
    MarkDown,
    MarkUp,
    InvertMarks,
    Visual,
    Command,
    Filter,
    FindRecursive,
    GrepRecursive,
    Sort,
    JumpPath,
    View,
    Diff,
    Refresh,
    Menu,
    Ssh,
    NewTab,
    CloseTab,
    Manual,
    /// Bound to a key to disable it — the key does nothing, shadowing whatever
    /// default it would otherwise trigger.
    Nop,
}

/// Map a Lua action name to an [`Action`]. Unknown names are reported as
/// config errors rather than silently ignored.
pub(crate) fn action_from_name(name: &str) -> Option<Action> {
    Some(match name {
        "cursor_down" => Action::CursorDown,
        "cursor_up" => Action::CursorUp,
        "cursor_top" => Action::CursorTop,
        "cursor_bottom" => Action::CursorBottom,
        "page_up" => Action::PageUp,
        "page_down" => Action::PageDown,
        "parent" => Action::Parent,
        "enter" => Action::EnterDir,
        "quit" => Action::Quit,
        "search" => Action::Search,
        "search_next" => Action::SearchNext,
        "search_prev" => Action::SearchPrev,
        "history" => Action::History,
        "shortcuts" => Action::Shortcuts,
        "copy" => Action::Copy,
        "move" => Action::Move,
        "delete" => Action::Delete,
        "rename" => Action::Rename,
        "new_file" => Action::NewFile,
        "new_dir" => Action::NewDir,
        "open_other" => Action::OpenOther,
        "open_other_tab" => Action::OpenOtherTab,
        "sync_from_other" => Action::SyncFromOther,
        "sync_to_other" => Action::SyncToOther,
        "open_external" => Action::OpenExternal,
        "copy_path" => Action::CopyPath,
        "copy_file_ref" => Action::CopyFileRef,
        "mark_down" => Action::MarkDown,
        "mark_up" => Action::MarkUp,
        "invert_marks" => Action::InvertMarks,
        "visual" => Action::Visual,
        "command" => Action::Command,
        "filter" => Action::Filter,
        "find_recursive" => Action::FindRecursive,
        "grep_recursive" => Action::GrepRecursive,
        "sort" => Action::Sort,
        "jump_path" => Action::JumpPath,
        "view" => Action::View,
        "diff" => Action::Diff,
        "refresh" => Action::Refresh,
        "menu" => Action::Menu,
        "ssh" => Action::Ssh,
        "new_tab" => Action::NewTab,
        "close_tab" => Action::CloseTab,
        "manual" => Action::Manual,
        "none" | "nop" | "unbind" => Action::Nop,
        _ => return None,
    })
}

/// Parse a user color spec: `#rrggbb`, `r,g,b`, or a named color.
pub(crate) fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        }
        return None;
    }
    if s.contains(',') {
        let parts: Vec<&str> = s.split(',').map(|x| x.trim()).collect();
        if parts.len() == 3 {
            let r = parts[0].parse::<u8>().ok()?;
            let g = parts[1].parse::<u8>().ok()?;
            let b = parts[2].parse::<u8>().ok()?;
            return Some(Color::Rgb(r, g, b));
        }
        return None;
    }
    match s.to_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "darkgrey" => Some(Color::DarkGray),
        "lightred" => Some(Color::LightRed),
        "lightgreen" => Some(Color::LightGreen),
        "lightyellow" => Some(Color::LightYellow),
        "lightblue" => Some(Color::LightBlue),
        "lightmagenta" => Some(Color::LightMagenta),
        "lightcyan" => Some(Color::LightCyan),
        _ => None,
    }
}

/// Resolve a Lua [`Theme`] into a concrete palette, collecting any invalid
/// color specs as human-readable errors (the default is kept for those).
/// Named palettes selectable with `cian.set_theme "<name>"`.
pub(crate) fn theme_preset(name: &str) -> Option<ResolvedTheme> {
    match name.trim().to_lowercase().replace([' ', '_'], "-").as_str() {
        "solarized-light" | "solarized" => Some(ResolvedTheme::solarized_light()),
        "default" | "dark" => Some(ResolvedTheme::default()),
        _ => None,
    }
}

pub(crate) fn resolve_theme(t: &cian_lua::Theme) -> (ResolvedTheme, Vec<String>) {
    let mut errors = Vec::new();
    // Start from the named preset if one was chosen, else the dark default.
    let mut c = match &t.preset {
        Some(name) => theme_preset(name).unwrap_or_else(|| {
            errors.push(format!(
                "theme.preset: unknown preset {:?} (try \"solarized-light\")",
                name
            ));
            ResolvedTheme::default()
        }),
        None => ResolvedTheme::default(),
    };
    let mut apply = |spec: &Option<String>, slot: &mut Color, label: &str| {
        if let Some(s) = spec {
            match parse_color(s) {
                Some(col) => *slot = col,
                None => errors.push(format!("theme.{}: invalid color {:?}", label, s)),
            }
        }
    };
    apply(&t.accent, &mut c.accent, "accent");
    apply(&t.status_bg, &mut c.status_bg, "status_bg");
    apply(&t.selected_bg, &mut c.selected_bg, "selected_bg");
    apply(&t.visual_bg, &mut c.visual_bg, "visual_bg");
    apply(&t.mark_fg, &mut c.mark_fg, "mark_fg");
    (c, errors)
}

/// Resolve and install the theme + border style into the process-wide statics
/// (call once, before drawing). Returns the non-fatal theme errors to report.
pub(crate) fn install(theme: &cian_lua::Theme, borders: Option<&str>, nerd: bool) -> Vec<String> {
    let (resolved, errs) = resolve_theme(theme);
    let _ = THEME.set(resolved);
    let _ = BORDERS.set(resolve_border_type(borders));
    let _ = NERD.set(nerd);
    errs
}
