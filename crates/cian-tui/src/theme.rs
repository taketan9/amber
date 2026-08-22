//! The colour theme, border style, interface language (i18n via `tr`), and the
//! remappable `Action` enum — resolved from init.lua and installed into
//! process-wide statics at startup. Split out of lib.rs.

use std::sync::{OnceLock, RwLock};

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
        Self::DARK
    }
}

/// `0xRRGGBB` → a ratatui truecolor. `const` so whole palettes are compile-time
/// constants.
const fn rgb(v: u32) -> Color {
    Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// A compact palette spec: the handful of colors a well-known theme actually
/// defines, from which [`from_spec`] derives every [`ResolvedTheme`] slot. Named
/// after the ANSI-ish roles most palettes publish, so a theme reads at a glance.
struct Spec {
    bg: u32,
    fg: u32,
    dim: u32,
    border: u32,
    accent: u32,
    sel: u32,
    visual: u32,
    mark: u32,
    /// The surface dialogs and menus are drawn on. A shade off `bg`, in the
    /// same direction the theme itself goes: a light theme's dialogs are
    /// light. (They were all dark once, whatever the theme, which made every
    /// popup look like a different program had opened it.) Text on them is
    /// `readable_on`, so either way round reads.
    popup: u32,
    status: u32,
    // File-type accents.
    blue: u32,
    yellow: u32,
    cyan: u32,
    magenta: u32,
    red: u32,
    green: u32,
    doc: u32,
}

/// Expand a [`Spec`] into the full resolved palette. `const` so every preset is
/// a `const ResolvedTheme`.
const fn from_spec(s: Spec) -> ResolvedTheme {
    ResolvedTheme {
        accent: rgb(s.accent),
        status_bg: rgb(s.status),
        selected_bg: rgb(s.sel),
        visual_bg: rgb(s.visual),
        mark_fg: rgb(s.mark),
        base_bg: Some(rgb(s.bg)),
        dim: rgb(s.dim),
        border: rgb(s.border),
        popup_bg: rgb(s.popup),
        file: FilePalette {
            directory: rgb(s.blue),
            code: rgb(s.yellow),
            config: rgb(s.cyan),
            document: rgb(s.doc),
            image: rgb(s.magenta),
            media: rgb(s.cyan),
            archive: rgb(s.red),
            executable: rgb(s.green),
            muted: rgb(s.dim),
            plain: rgb(s.fg),
        },
    }
}

impl ResolvedTheme {
    /// The original built-in dark theme. Unlike the named presets it leaves
    /// `base_bg` as `None`, so the terminal's own background shows through.
    pub(crate) const DARK: ResolvedTheme = ResolvedTheme {
        accent: Color::Cyan, // cian-blue, kept consistent across the app
        status_bg: rgb(0x282837),
        selected_bg: rgb(0x3c3c5a),
        visual_bg: rgb(0x503c1e),
        mark_fg: Color::Yellow,
        base_bg: None,
        dim: rgb(0x82829b),
        border: Color::DarkGray,
        popup_bg: rgb(0x181822),
        file: FilePalette {
            directory: rgb(0x60a5fa),
            code: rgb(0xfacc15),
            config: rgb(0x94bed2),
            document: rgb(0xe2e2ec),
            image: rgb(0xd882dc),
            media: rgb(0x78c8be),
            archive: rgb(0xf08278),
            executable: rgb(0x7ed982),
            muted: rgb(0x808094),
            plain: rgb(0xcdcdda),
        },
    };

    // Ethan Schoonover's Solarized, light and dark. Popups stay on Solarized's
    // dark base02 so their light body text reads over the light surface.
    pub(crate) const SOLARIZED_LIGHT: ResolvedTheme = from_spec(Spec {
        bg: 0xfdf6e3, fg: 0x657b83, dim: 0x93a1a1, border: 0x93a1a1,
        accent: 0x268bd2, sel: 0xdcd5be, visual: 0xf7e4b0, mark: 0xcb4b16,
        popup: 0xf5efdc, status: 0xeee8d5,
        blue: 0x268bd2, yellow: 0xb58900, cyan: 0x2aa198, magenta: 0xd33682,
        red: 0xdc322f, green: 0x859900, doc: 0x586e75,
    });
    /// The palette a desktop file manager is drawn in: near-white, one strong
    /// blue for the selection, and greys quiet enough that the eye goes to the
    /// names. Paired with [`crate::Skin::Finder`], which is what takes the
    /// borders away — the colours alone are just another light theme.
    pub(crate) const FINDER: ResolvedTheme = from_spec(Spec {
        bg: 0xffffff, fg: 0x1d1d1f, dim: 0x86868b, border: 0xd8d8dc,
        accent: 0x0a84ff, sel: 0x0a84ff, visual: 0xd6e9ff, mark: 0xff9500,
        popup: 0xf7f7f9, status: 0xececee,
        blue: 0x2f7de0, yellow: 0x9a6b00, cyan: 0x0a7f8c, magenta: 0xa63aa6,
        red: 0xc0392b, green: 0x2f8a3e, doc: 0x3a3a3c,
    });
    pub(crate) const SOLARIZED_DARK: ResolvedTheme = from_spec(Spec {
        bg: 0x002b36, fg: 0x839496, dim: 0x586e75, border: 0x586e75,
        accent: 0x268bd2, sel: 0x073642, visual: 0x0a4a5a, mark: 0xcb4b16,
        popup: 0x073642, status: 0x073642,
        blue: 0x268bd2, yellow: 0xb58900, cyan: 0x2aa198, magenta: 0xd33682,
        red: 0xdc322f, green: 0x859900, doc: 0x93a1a1,
    });
    pub(crate) const DRACULA: ResolvedTheme = from_spec(Spec {
        bg: 0x282a36, fg: 0xf8f8f2, dim: 0x6272a4, border: 0x6272a4,
        accent: 0xbd93f9, sel: 0x44475a, visual: 0x424458, mark: 0xffb86c,
        popup: 0x21222c, status: 0x191a21,
        blue: 0xbd93f9, yellow: 0xf1fa8c, cyan: 0x8be9fd, magenta: 0xff79c6,
        red: 0xff5555, green: 0x50fa7b, doc: 0xf8f8f2,
    });
    pub(crate) const NORD: ResolvedTheme = from_spec(Spec {
        bg: 0x2e3440, fg: 0xd8dee9, dim: 0x4c566a, border: 0x4c566a,
        accent: 0x88c0d0, sel: 0x3b4252, visual: 0x434c5e, mark: 0xebcb8b,
        popup: 0x272c36, status: 0x3b4252,
        blue: 0x81a1c1, yellow: 0xebcb8b, cyan: 0x88c0d0, magenta: 0xb48ead,
        red: 0xbf616a, green: 0xa3be8c, doc: 0xe5e9f0,
    });
    pub(crate) const GRUVBOX_DARK: ResolvedTheme = from_spec(Spec {
        bg: 0x282828, fg: 0xebdbb2, dim: 0x928374, border: 0x504945,
        accent: 0xfe8019, sel: 0x3c3836, visual: 0x504945, mark: 0xfabd2f,
        popup: 0x1d2021, status: 0x3c3836,
        blue: 0x83a598, yellow: 0xfabd2f, cyan: 0x8ec07c, magenta: 0xd3869b,
        red: 0xfb4934, green: 0xb8bb26, doc: 0xebdbb2,
    });
    pub(crate) const GRUVBOX_LIGHT: ResolvedTheme = from_spec(Spec {
        bg: 0xfbf1c7, fg: 0x3c3836, dim: 0x7c6f64, border: 0xd5c4a1,
        accent: 0xaf3a03, sel: 0xebdbb2, visual: 0xd5c4a1, mark: 0xb57614,
        popup: 0xf2e5bc, status: 0xebdbb2,
        blue: 0x076678, yellow: 0xb57614, cyan: 0x427b58, magenta: 0x8f3f71,
        red: 0x9d0006, green: 0x79740e, doc: 0x3c3836,
    });
    pub(crate) const TOKYO_NIGHT: ResolvedTheme = from_spec(Spec {
        bg: 0x1a1b26, fg: 0xc0caf5, dim: 0x565f89, border: 0x292e42,
        accent: 0x7aa2f7, sel: 0x292e42, visual: 0x33467c, mark: 0xe0af68,
        popup: 0x16161e, status: 0x16161e,
        blue: 0x7aa2f7, yellow: 0xe0af68, cyan: 0x7dcfff, magenta: 0xbb9af7,
        red: 0xf7768e, green: 0x9ece6a, doc: 0xc0caf5,
    });
    pub(crate) const CATPPUCCIN_MOCHA: ResolvedTheme = from_spec(Spec {
        bg: 0x1e1e2e, fg: 0xcdd6f4, dim: 0x6c7086, border: 0x313244,
        accent: 0x89b4fa, sel: 0x313244, visual: 0x45475a, mark: 0xf9e2af,
        popup: 0x181825, status: 0x181825,
        blue: 0x89b4fa, yellow: 0xf9e2af, cyan: 0x94e2d5, magenta: 0xf5c2e7,
        red: 0xf38ba8, green: 0xa6e3a1, doc: 0xcdd6f4,
    });
    pub(crate) const CATPPUCCIN_LATTE: ResolvedTheme = from_spec(Spec {
        bg: 0xeff1f5, fg: 0x4c4f69, dim: 0x6c6f85, border: 0xccd0da,
        accent: 0x1e66f5, sel: 0xccd0da, visual: 0xdce0e8, mark: 0xdf8e1d,
        popup: 0xe6e9ef, status: 0xccd0da,
        blue: 0x1e66f5, yellow: 0xdf8e1d, cyan: 0x179299, magenta: 0xea76cb,
        red: 0xd20f39, green: 0x40a02b, doc: 0x4c4f69,
    });
    pub(crate) const MONOKAI: ResolvedTheme = from_spec(Spec {
        bg: 0x272822, fg: 0xf8f8f2, dim: 0x75715e, border: 0x3e3d32,
        accent: 0x66d9ef, sel: 0x3e3d32, visual: 0x49483e, mark: 0xfd971f,
        popup: 0x1e1f1c, status: 0x3e3d32,
        blue: 0x66d9ef, yellow: 0xe6db74, cyan: 0x66d9ef, magenta: 0xae81ff,
        red: 0xf92672, green: 0xa6e22e, doc: 0xf8f8f2,
    });
    pub(crate) const ONE_DARK: ResolvedTheme = from_spec(Spec {
        bg: 0x282c34, fg: 0xabb2bf, dim: 0x5c6370, border: 0x3b4048,
        accent: 0x61afef, sel: 0x3b4048, visual: 0x3e4451, mark: 0xe5c07b,
        popup: 0x21252b, status: 0x21252b,
        blue: 0x61afef, yellow: 0xe5c07b, cyan: 0x56b6c2, magenta: 0xc678dd,
        red: 0xe06c75, green: 0x98c379, doc: 0xabb2bf,
    });
    pub(crate) const GITHUB_LIGHT: ResolvedTheme = from_spec(Spec {
        bg: 0xffffff, fg: 0x24292e, dim: 0x6a737d, border: 0xd1d5da,
        accent: 0x0366d6, sel: 0xeef2f5, visual: 0xdbe9ff, mark: 0xe36209,
        popup: 0xf6f8fa, status: 0xeaeef2,
        blue: 0x0366d6, yellow: 0xb08800, cyan: 0x1b7c83, magenta: 0x6f42c1,
        red: 0xd73a49, green: 0x22863a, doc: 0x24292e,
    });
    /// Monokai Pro — the paid Monokai's own palette, not the classic one
    /// above: warmer greys, and the amber that everything is keyed to.
    pub(crate) const MONOKAI_PRO: ResolvedTheme = from_spec(Spec {
        bg: 0x2d2a2e, fg: 0xfcfcfa, dim: 0x727072, border: 0x5b595c,
        accent: 0xffd866, sel: 0x423f42, visual: 0x5b595c, mark: 0xfc9867,
        popup: 0x221f22, status: 0x221f22,
        blue: 0x78dce8, yellow: 0xffd866, cyan: 0x78dce8, magenta: 0xab9df2,
        red: 0xff6188, green: 0xa9dc76, doc: 0xc1c0c0,
    });
    /// Ayu Dark — near-black with one amber accent, which is the whole idea.
    pub(crate) const AYU_DARK: ResolvedTheme = from_spec(Spec {
        bg: 0x0d1017, fg: 0xbfbdb6, dim: 0x565b66, border: 0x1d2229,
        accent: 0xe6b450, sel: 0x1d2733, visual: 0x2d3640, mark: 0xff8f40,
        popup: 0x131721, status: 0x11151c,
        blue: 0x59c2ff, yellow: 0xe6b450, cyan: 0x95e6cb, magenta: 0xd2a6ff,
        red: 0xf26d78, green: 0xaad94c, doc: 0xacb6bf,
    });
    /// Ayu Light — the same palette on paper.
    pub(crate) const AYU_LIGHT: ResolvedTheme = from_spec(Spec {
        bg: 0xfcfcfc, fg: 0x5c6166, dim: 0x8a9199, border: 0xe7e8e9,
        accent: 0xf2ae49, sel: 0xeaeaeb, visual: 0xffe9b3, mark: 0xfa8d3e,
        popup: 0xf3f3f3, status: 0xf0f0f0,
        blue: 0x399ee6, yellow: 0xf2ae49, cyan: 0x4cbf99, magenta: 0xa37acc,
        red: 0xf07171, green: 0x86b300, doc: 0x787b80,
    });
    /// Bluloco Light — a light theme with saturated syntax rather than pastel.
    pub(crate) const BLULOCO_LIGHT: ResolvedTheme = from_spec(Spec {
        bg: 0xf9f9f9, fg: 0x383a42, dim: 0xa0a1a7, border: 0xd4d4d4,
        accent: 0x275fe4, sel: 0xe5e5e6, visual: 0xd7e0f5, mark: 0xd52753,
        popup: 0xf0f0f0, status: 0xefefef,
        blue: 0x275fe4, yellow: 0xc18401, cyan: 0x0098dd, magenta: 0x823ff1,
        red: 0xd52753, green: 0x23974a, doc: 0x7a82da,
    });
    /// Bearded — the family's dark, vivid look: a near-black violet ground
    /// with pink, amethyst and teal on it. Approximated from the family's
    /// signature colours rather than copied from one variant, since Bearded
    /// ships dozens; `cian.set_theme{...}` takes exact values if you have a
    /// particular one in mind.
    pub(crate) const BEARDED: ResolvedTheme = from_spec(Spec {
        bg: 0x16161d, fg: 0xebebf0, dim: 0x6c6f93, border: 0x2a2a3c,
        accent: 0xa45fff, sel: 0x2c2c3f, visual: 0x3a2f55, mark: 0xff3e7b,
        popup: 0x1d1d28, status: 0x1d1d28,
        blue: 0x50b0f0, yellow: 0xffb86c, cyan: 0x21c7a8, magenta: 0xff3e7b,
        red: 0xff5f87, green: 0x7ddb8a, doc: 0xb9bacb,
    });
}

/// The named presets, in gallery order. `default` is the transparent-background
/// built-in; the rest paint their own surface.
pub(crate) const THEME_NAMES: &[&str] = &[
    "default",
    "solarized-light",
    "solarized-dark",
    "dracula",
    "nord",
    "gruvbox-dark",
    "gruvbox-light",
    "tokyo-night",
    "catppuccin-mocha",
    "catppuccin-latte",
    "monokai",
    "one-dark",
    "github-light",
    "monokai-pro",
    "ayu-dark",
    "ayu-light",
    "bluloco-light",
    "bearded",
];

/// Process-wide active theme. Unlike the old set-once global this is swappable
/// so `:theme` can change the look live; the stateless draw helpers read it
/// through [`theme`] without threading a palette through every call. Reads take
/// a copy — `ResolvedTheme` is `Copy` and small.
static THEME: RwLock<ResolvedTheme> = RwLock::new(ResolvedTheme::DARK);

pub(crate) fn theme() -> ResolvedTheme {
    *THEME.read().unwrap_or_else(|e| e.into_inner())
}

/// Whether these colours were chosen by the user rather than by cian.
///
/// The detail and icon views bring the Finder palette with them, because the
/// borderless shape they use only reads on a light surface — a borderless dark
/// pane is not a Finder, it is a pane with its edges missing. That is the right
/// default and the wrong override: someone who set `solarized-light` in
/// init.lua, or picked a theme with `:theme`, watched it apply in the classic
/// view and be replaced the moment they pressed Ctrl+Shift+G. A choice made out
/// loud holds in every view.
static ASKED_FOR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Note that a theme was named — in init.lua, by `:theme`, or in the gallery.
pub(crate) fn theme_was_asked_for() {
    ASKED_FOR.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn theme_is_the_users() -> bool {
    ASKED_FOR.load(std::sync::atomic::Ordering::Relaxed)
}

/// May a change of skin replace the colours in force?
///
/// Split from the flag so the rule can be asserted without a process-wide
/// switch that, once flipped, stays flipped for every other test in the run.
pub(crate) fn skin_may_swap_theme(theme_is_the_users: bool) -> bool {
    !theme_is_the_users
}

/// Which colours a view should be wearing.
///
/// The desktop skins only read on a light surface — a borderless dark pane is
/// not a Finder, it is a pane with its edges missing — so switching to one
/// brings its palette with it. A theme the user asked for by name outranks
/// that, everywhere it comes up.
///
/// Written as a function of what it depends on, not of the process-wide flag,
/// so the rule can be asserted without one test's `:theme` deciding another
/// test's colours.
pub(crate) fn theme_for_skin(
    configured: ResolvedTheme,
    finder_skin: bool,
    theme_is_the_users: bool,
) -> ResolvedTheme {
    if finder_skin && skin_may_swap_theme(theme_is_the_users) {
        ResolvedTheme::FINDER
    } else {
        configured
    }
}

/// Swap the active theme (from `:theme`, the picker preview, or `:reload`).
pub(crate) fn set_theme(t: ResolvedTheme) {
    let mut w = THEME.write().unwrap_or_else(|e| e.into_inner());
    if *w != t {
        THEME_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    *w = t;
}

/// Bumped whenever the theme actually changes.
///
/// Anything that caches *styles* rather than recomputing them each frame — the
/// Markdown preview's grid, the syntax highlighter — has to know when the
/// colours underneath it moved. Without this, a preview opened on a light
/// theme kept its near-black text after `:theme` switched to a dark one, and
/// the page went black on black.
static THEME_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn theme_generation() -> u64 {
    THEME_GEN.load(std::sync::atomic::Ordering::Relaxed)
}

/// A concrete surface colour that follows the theme's light/dark identity —
/// the theme's own `base_bg` when it paints one (light themes go light), else
/// the dark popup background. Surfaces that want to honour a light theme (the
/// right-click menu, the F3 viewer) use this with `readable_on` for their text,
/// instead of the always-dark `popup_bg`.
pub(crate) fn surface() -> Color {
    let t = theme();
    t.base_bg.unwrap_or(t.popup_bg)
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

/// Set once by the windowed front end, before the theme is resolved.
///
/// There is no terminal in that build at all, and no environment variable says
/// so — a window started from Explorer looks to every test below exactly like
/// the legacy console, and was being treated as one.
static WINDOWED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Say that cian owns its own window, and with it its own font.
pub(crate) fn host_is_a_window() {
    WINDOWED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Is cian drawing into a window of its own?
///
/// Asked by the few things that are decided by how the host draws a cell rather
/// than by anything cian means — see [`crate::render::cloud_mark_for`].
pub(crate) fn in_a_window() -> bool {
    WINDOWED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the host can be trusted with the glyphs cian would rather use.
///
/// A window can, always: the font is cian's own and it is a Nerd Font. A
/// terminal can if it says which one it is — the legacy Windows console sets
/// none of these.
pub(crate) fn modern_terminal() -> bool {
    WINDOWED.load(std::sync::atomic::Ordering::Relaxed)
        || std::env::var_os("WT_SESSION").is_some()
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
    /// From the `lang` option. Japanese unless English is asked for: cian is
    /// written in Japanese first, and an unset `lang` should give the people
    /// it was written for their own language without a config file. (The Lua
    /// layer already rejects values other than "ja"/"en".)
    pub(crate) fn from_opt(opt: Option<&str>) -> Lang {
        match opt {
            Some("en") => Lang::En,
            _ => Lang::Ja,
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
///
/// # How cian talks
///
/// Every string that reaches a person goes through here, so the house style
/// belongs here too. It was arrived at by measuring what the six hundred
/// existing messages already did and settling the exceptions, not by decree.
///
/// * **English begins lower-case**, unless the first word is a name: `nothing
///   to operate on`, but `AI returned no command`. A terminal tool's voice,
///   the same as `ls` and `git`.
/// * **Japanese is 敬体** — 「〜ます」「〜ません」. Never 常体.
/// * **No full stop at the end**, in either language. Between two sentences,
///   yes: 「未保存の変更があります。Ctrl+S で保存できます」, `unsaved changes.
///   Ctrl+S saves` — the reader needs the break, but the line does not need a
///   stop it will never be followed past. A test holds both languages to this.
/// * **Two sentences, not a dash.** State what happened, then what can be done
///   about it. `unsaved changes — Ctrl+S saves` reads as one breathless
///   thought; `unsaved changes. Ctrl+S saves` is two clear ones.
/// * **Never "for now", "not yet", "temporarily".** A limit is a fact about
///   the tool, not an apology for it: `archives are read-only. copy extracts.`
///   Saying nothing at all about a limit is worse — silence reads as a bug.
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
    /// Paste the file clipboard (Windows-style; also on Ctrl+V).
    Paste,
    /// Cut the selection to the file clipboard (also on Ctrl+X).
    Cut,
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
    /// Mark every file in this listing — or, in the viewer, select the whole
    /// file. Which of the two is simply which is in front of you.
    MarkAll,
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
        "paste" => Action::Paste,
        "cut" => Action::Cut,
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
        "mark_all" | "select_all" => Action::MarkAll,
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

/// Parse a key spec from `cian.set_keymap` — `"x"`, `"alt+g"`, `"ctrl+f"`,
/// `"shift+s"` — into the character and the modifiers to match on.
///
/// Shift is folded into the character rather than kept as a modifier: a
/// terminal may or may not report Shift alongside an uppercase letter, and the
/// uppercase letter already says everything the binding needs. Only Ctrl and
/// Alt survive as modifiers, which are the two a terminal reports reliably.
pub(crate) fn parse_key_spec(spec: &str) -> Option<(char, crossterm::event::KeyModifiers)> {
    use crossterm::event::KeyModifiers;
    let spec = spec.trim();
    let mut parts: Vec<&str> = spec.split('+').collect();
    let key = parts.pop()?;
    let mut c = key.chars().next()?;
    if key.chars().count() != 1 {
        return None;
    }
    let mut mods = KeyModifiers::NONE;
    for m in parts {
        match m.trim().to_lowercase().as_str() {
            "ctrl" | "control" | "c" => mods |= KeyModifiers::CONTROL,
            "alt" | "opt" | "option" | "meta" | "m" => mods |= KeyModifiers::ALT,
            "shift" | "s" => c = c.to_ascii_uppercase(),
            _ => return None,
        }
    }
    Some((c, mods))
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
    Some(match name.trim().to_lowercase().replace([' ', '_'], "-").as_str() {
        "default" | "dark" => ResolvedTheme::DARK,
        "solarized-light" | "solarized" => ResolvedTheme::SOLARIZED_LIGHT,
        "solarized-dark" => ResolvedTheme::SOLARIZED_DARK,
        "dracula" => ResolvedTheme::DRACULA,
        "nord" => ResolvedTheme::NORD,
        "gruvbox-dark" | "gruvbox" => ResolvedTheme::GRUVBOX_DARK,
        "gruvbox-light" => ResolvedTheme::GRUVBOX_LIGHT,
        "tokyo-night" | "tokyonight" => ResolvedTheme::TOKYO_NIGHT,
        "catppuccin-mocha" | "catppuccin" | "mocha" => ResolvedTheme::CATPPUCCIN_MOCHA,
        "catppuccin-latte" | "latte" => ResolvedTheme::CATPPUCCIN_LATTE,
        "monokai" => ResolvedTheme::MONOKAI,
        "one-dark" | "onedark" => ResolvedTheme::ONE_DARK,
        "github-light" | "github" => ResolvedTheme::GITHUB_LIGHT,
        "monokai-pro" | "monokaipro" => ResolvedTheme::MONOKAI_PRO,
        "ayu-dark" | "ayu" => ResolvedTheme::AYU_DARK,
        "ayu-light" => ResolvedTheme::AYU_LIGHT,
        "bluloco-light" | "bluloco" => ResolvedTheme::BLULOCO_LIGHT,
        "bearded" | "bearded-theme" => ResolvedTheme::BEARDED,
        "finder" => ResolvedTheme::FINDER,
        _ => return None,
    })
}

/// The preset name whose palette matches `t`, if any (so the picker and status
/// bar can name the active theme). Compares by value since presets are `Copy`.
pub(crate) fn theme_name_of(t: &ResolvedTheme) -> Option<&'static str> {
    THEME_NAMES.iter().copied().find(|n| theme_preset(n).as_ref() == Some(t))
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
    // Did anyone actually ask for these colours, or are they the ones cian
    // picked? Only the asked-for kind survives a change of view.
    if theme.preset.is_some()
        || theme.accent.is_some()
        || theme.status_bg.is_some()
        || theme.selected_bg.is_some()
        || theme.visual_bg.is_some()
        || theme.mark_fg.is_some()
    {
        theme_was_asked_for();
    }
    let (resolved, errs) = resolve_theme(theme);
    set_theme(resolved);
    let _ = BORDERS.set(resolve_border_type(borders));
    let _ = NERD.set(nerd);
    errs
}
