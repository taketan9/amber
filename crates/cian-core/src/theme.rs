//! The named colour palettes, as data.
//!
//! Both front ends need these and neither should own them: the terminal build
//! turns a spec into ratatui colours, the window turns the same spec into CSS
//! custom properties, and a palette that existed twice would be two palettes
//! wearing one name the first time somebody adjusted one of them.
//!
//! A spec is the handful of colours a well-known theme actually publishes;
//! everything else each front end needs is derived from these. Named after the
//! ANSI-ish roles most palettes use, so a theme reads at a glance.

/// One palette. Every field is `0xrrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub bg: u32,
    pub fg: u32,
    pub dim: u32,
    pub border: u32,
    pub accent: u32,
    pub sel: u32,
    pub visual: u32,
    pub mark: u32,
    /// The surface dialogs and menus are drawn on. A shade off `bg`, in the
    /// same direction the theme itself goes: a light theme's dialogs are
    /// light. (They were all dark once, whatever the theme, which made every
    /// popup look like a different program had opened it.)
    pub popup: u32,
    pub status: u32,
    // File-type accents.
    pub blue: u32,
    pub yellow: u32,
    pub cyan: u32,
    pub magenta: u32,
    pub red: u32,
    pub green: u32,
    pub doc: u32,
}

impl Spec {
    /// Is this a light palette? Measured off the background's luminance, not
    /// declared per theme — a name can lie and a number cannot.
    pub fn is_light(&self) -> bool {
        let (r, g, b) = ((self.bg >> 16) & 0xff, (self.bg >> 8) & 0xff, self.bg & 0xff);
        // Rec. 601, which is close enough for "which way round is this".
        (299 * r + 587 * g + 114 * b) / 1000 > 128
    }
}

pub const SOLARIZED_LIGHT: Spec = Spec {
        bg: 0xfdf6e3, fg: 0x657b83, dim: 0x93a1a1, border: 0x93a1a1,
        accent: 0x268bd2, sel: 0xdcd5be, visual: 0xf7e4b0, mark: 0xcb4b16,
        popup: 0xf5efdc, status: 0xeee8d5,
        blue: 0x268bd2, yellow: 0xb58900, cyan: 0x2aa198, magenta: 0xd33682,
        red: 0xdc322f, green: 0x859900, doc: 0x586e75,
    };
pub const FINDER: Spec = Spec {
        bg: 0xffffff, fg: 0x1d1d1f, dim: 0x86868b, border: 0xd8d8dc,
        accent: 0x0a84ff, sel: 0x0a84ff, visual: 0xd6e9ff, mark: 0xff9500,
        popup: 0xf7f7f9, status: 0xececee,
        blue: 0x2f7de0, yellow: 0x9a6b00, cyan: 0x0a7f8c, magenta: 0xa63aa6,
        red: 0xc0392b, green: 0x2f8a3e, doc: 0x3a3a3c,
    };
pub const SOLARIZED_DARK: Spec = Spec {
        bg: 0x002b36, fg: 0x839496, dim: 0x586e75, border: 0x586e75,
        accent: 0x268bd2, sel: 0x073642, visual: 0x0a4a5a, mark: 0xcb4b16,
        popup: 0x073642, status: 0x073642,
        blue: 0x268bd2, yellow: 0xb58900, cyan: 0x2aa198, magenta: 0xd33682,
        red: 0xdc322f, green: 0x859900, doc: 0x93a1a1,
    };
pub const DRACULA: Spec = Spec {
        bg: 0x282a36, fg: 0xf8f8f2, dim: 0x6272a4, border: 0x6272a4,
        accent: 0xbd93f9, sel: 0x44475a, visual: 0x424458, mark: 0xffb86c,
        popup: 0x21222c, status: 0x191a21,
        blue: 0xbd93f9, yellow: 0xf1fa8c, cyan: 0x8be9fd, magenta: 0xff79c6,
        red: 0xff5555, green: 0x50fa7b, doc: 0xf8f8f2,
    };
pub const NORD: Spec = Spec {
        bg: 0x2e3440, fg: 0xd8dee9, dim: 0x4c566a, border: 0x4c566a,
        accent: 0x88c0d0, sel: 0x3b4252, visual: 0x434c5e, mark: 0xebcb8b,
        popup: 0x272c36, status: 0x3b4252,
        blue: 0x81a1c1, yellow: 0xebcb8b, cyan: 0x88c0d0, magenta: 0xb48ead,
        red: 0xbf616a, green: 0xa3be8c, doc: 0xe5e9f0,
    };
pub const GRUVBOX_DARK: Spec = Spec {
        bg: 0x282828, fg: 0xebdbb2, dim: 0x928374, border: 0x504945,
        accent: 0xfe8019, sel: 0x3c3836, visual: 0x504945, mark: 0xfabd2f,
        popup: 0x1d2021, status: 0x3c3836,
        blue: 0x83a598, yellow: 0xfabd2f, cyan: 0x8ec07c, magenta: 0xd3869b,
        red: 0xfb4934, green: 0xb8bb26, doc: 0xebdbb2,
    };
pub const GRUVBOX_LIGHT: Spec = Spec {
        bg: 0xfbf1c7, fg: 0x3c3836, dim: 0x7c6f64, border: 0xd5c4a1,
        accent: 0xaf3a03, sel: 0xebdbb2, visual: 0xd5c4a1, mark: 0xb57614,
        popup: 0xf2e5bc, status: 0xebdbb2,
        blue: 0x076678, yellow: 0xb57614, cyan: 0x427b58, magenta: 0x8f3f71,
        red: 0x9d0006, green: 0x79740e, doc: 0x3c3836,
    };
pub const TOKYO_NIGHT: Spec = Spec {
        bg: 0x1a1b26, fg: 0xc0caf5, dim: 0x565f89, border: 0x292e42,
        accent: 0x7aa2f7, sel: 0x292e42, visual: 0x33467c, mark: 0xe0af68,
        popup: 0x16161e, status: 0x16161e,
        blue: 0x7aa2f7, yellow: 0xe0af68, cyan: 0x7dcfff, magenta: 0xbb9af7,
        red: 0xf7768e, green: 0x9ece6a, doc: 0xc0caf5,
    };
pub const CATPPUCCIN_MOCHA: Spec = Spec {
        bg: 0x1e1e2e, fg: 0xcdd6f4, dim: 0x6c7086, border: 0x313244,
        accent: 0x89b4fa, sel: 0x313244, visual: 0x45475a, mark: 0xf9e2af,
        popup: 0x181825, status: 0x181825,
        blue: 0x89b4fa, yellow: 0xf9e2af, cyan: 0x94e2d5, magenta: 0xf5c2e7,
        red: 0xf38ba8, green: 0xa6e3a1, doc: 0xcdd6f4,
    };
pub const CATPPUCCIN_LATTE: Spec = Spec {
        bg: 0xeff1f5, fg: 0x4c4f69, dim: 0x6c6f85, border: 0xccd0da,
        accent: 0x1e66f5, sel: 0xccd0da, visual: 0xdce0e8, mark: 0xdf8e1d,
        popup: 0xe6e9ef, status: 0xccd0da,
        blue: 0x1e66f5, yellow: 0xdf8e1d, cyan: 0x179299, magenta: 0xea76cb,
        red: 0xd20f39, green: 0x40a02b, doc: 0x4c4f69,
    };
pub const MONOKAI: Spec = Spec {
        bg: 0x272822, fg: 0xf8f8f2, dim: 0x75715e, border: 0x3e3d32,
        accent: 0x66d9ef, sel: 0x3e3d32, visual: 0x49483e, mark: 0xfd971f,
        popup: 0x1e1f1c, status: 0x3e3d32,
        blue: 0x66d9ef, yellow: 0xe6db74, cyan: 0x66d9ef, magenta: 0xae81ff,
        red: 0xf92672, green: 0xa6e22e, doc: 0xf8f8f2,
    };
pub const ONE_DARK: Spec = Spec {
        bg: 0x282c34, fg: 0xabb2bf, dim: 0x5c6370, border: 0x3b4048,
        accent: 0x61afef, sel: 0x3b4048, visual: 0x3e4451, mark: 0xe5c07b,
        popup: 0x21252b, status: 0x21252b,
        blue: 0x61afef, yellow: 0xe5c07b, cyan: 0x56b6c2, magenta: 0xc678dd,
        red: 0xe06c75, green: 0x98c379, doc: 0xabb2bf,
    };
pub const GITHUB_LIGHT: Spec = Spec {
        bg: 0xffffff, fg: 0x24292e, dim: 0x6a737d, border: 0xd1d5da,
        accent: 0x0366d6, sel: 0xeef2f5, visual: 0xdbe9ff, mark: 0xe36209,
        popup: 0xf6f8fa, status: 0xeaeef2,
        blue: 0x0366d6, yellow: 0xb08800, cyan: 0x1b7c83, magenta: 0x6f42c1,
        red: 0xd73a49, green: 0x22863a, doc: 0x24292e,
    };
pub const MONOKAI_PRO: Spec = Spec {
        bg: 0x2d2a2e, fg: 0xfcfcfa, dim: 0x727072, border: 0x5b595c,
        accent: 0xffd866, sel: 0x423f42, visual: 0x5b595c, mark: 0xfc9867,
        popup: 0x221f22, status: 0x221f22,
        blue: 0x78dce8, yellow: 0xffd866, cyan: 0x78dce8, magenta: 0xab9df2,
        red: 0xff6188, green: 0xa9dc76, doc: 0xc1c0c0,
    };
pub const AYU_DARK: Spec = Spec {
        bg: 0x0d1017, fg: 0xbfbdb6, dim: 0x565b66, border: 0x1d2229,
        accent: 0xe6b450, sel: 0x1d2733, visual: 0x2d3640, mark: 0xff8f40,
        popup: 0x131721, status: 0x11151c,
        blue: 0x59c2ff, yellow: 0xe6b450, cyan: 0x95e6cb, magenta: 0xd2a6ff,
        red: 0xf26d78, green: 0xaad94c, doc: 0xacb6bf,
    };
pub const AYU_LIGHT: Spec = Spec {
        bg: 0xfcfcfc, fg: 0x5c6166, dim: 0x8a9199, border: 0xe7e8e9,
        accent: 0xf2ae49, sel: 0xeaeaeb, visual: 0xffe9b3, mark: 0xfa8d3e,
        popup: 0xf3f3f3, status: 0xf0f0f0,
        blue: 0x399ee6, yellow: 0xf2ae49, cyan: 0x4cbf99, magenta: 0xa37acc,
        red: 0xf07171, green: 0x86b300, doc: 0x787b80,
    };
pub const BLULOCO_LIGHT: Spec = Spec {
        bg: 0xf9f9f9, fg: 0x383a42, dim: 0xa0a1a7, border: 0xd4d4d4,
        accent: 0x275fe4, sel: 0xe5e5e6, visual: 0xd7e0f5, mark: 0xd52753,
        popup: 0xf0f0f0, status: 0xefefef,
        blue: 0x275fe4, yellow: 0xc18401, cyan: 0x0098dd, magenta: 0x823ff1,
        red: 0xd52753, green: 0x23974a, doc: 0x7a82da,
    };
pub const BEARDED: Spec = Spec {
        bg: 0x16161d, fg: 0xebebf0, dim: 0x6c6f93, border: 0x2a2a3c,
        accent: 0xa45fff, sel: 0x2c2c3f, visual: 0x3a2f55, mark: 0xff3e7b,
        popup: 0x1d1d28, status: 0x1d1d28,
        blue: 0x50b0f0, yellow: 0xffb86c, cyan: 0x21c7a8, magenta: 0xff3e7b,
        red: 0xff5f87, green: 0x7ddb8a, doc: 0xb9bacb,
    };

/// Every preset, in gallery order — the order the terminal build's `:theme`
/// picker walks, so the two builds offer the same list in the same sequence.
pub const PRESETS: &[(&str, Spec)] = &[
    ("solarized-light", SOLARIZED_LIGHT),
    ("solarized-dark", SOLARIZED_DARK),
    ("dracula", DRACULA),
    ("nord", NORD),
    ("gruvbox-dark", GRUVBOX_DARK),
    ("gruvbox-light", GRUVBOX_LIGHT),
    ("tokyo-night", TOKYO_NIGHT),
    ("catppuccin-mocha", CATPPUCCIN_MOCHA),
    ("catppuccin-latte", CATPPUCCIN_LATTE),
    ("monokai", MONOKAI),
    ("one-dark", ONE_DARK),
    ("github-light", GITHUB_LIGHT),
    ("monokai-pro", MONOKAI_PRO),
    ("ayu-dark", AYU_DARK),
    ("ayu-light", AYU_LIGHT),
    ("bluloco-light", BLULOCO_LIGHT),
    ("bearded", BEARDED),
    ("finder", FINDER),
];

/// The palette by name.
pub fn by_name(name: &str) -> Option<Spec> {
    PRESETS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// The relative luminance of `0xrrggbb`, the way WCAG defines it.
fn luminance(c: u32) -> f32 {
    let chan = |v: u32| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * chan((c >> 16) & 0xff) + 0.7152 * chan((c >> 8) & 0xff) + 0.0722 * chan(c & 0xff)
}

/// The contrast between two colours, 1.0 to 21.0.
pub fn contrast(a: u32, b: u32) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

/// Text that reads on `bg`.
///
/// The terminal build's rule, moved here so the window follows it too: prefer
/// the soft pair the rest of the interface is drawn in, and only reach for
/// pure black or white when a mid-tone ground — Catppuccin Latte's blue,
/// Dracula's selection — is too far from both for either to clear 4.5:1. The
/// eye notices the difference in tone long before it notices the extra
/// contrast.
pub fn readable_on(bg: u32) -> u32 {
    const DARK: u32 = 0x1e2028;
    const LIGHT: u32 = 0xe4e4f0;
    let soft = if contrast(DARK, bg) >= contrast(LIGHT, bg) { DARK } else { LIGHT };
    if contrast(soft, bg) >= 4.5 {
        return soft;
    }
    let hard = if contrast(0x000000, bg) >= contrast(0xffffff, bg) { 0x000000 } else { 0xffffff };
    if contrast(hard, bg) > contrast(soft, bg) { hard } else { soft }
}

/// A colour moved toward `bg` until it is a background rather than a mark —
/// what a highlight band or a soft accent panel is made of.
pub fn toward(c: u32, bg: u32, amount: f32) -> u32 {
    let mix = |sh: u32| {
        let (a, b) = (((c >> sh) & 0xff) as f32, ((bg >> sh) & 0xff) as f32);
        (a + (b - a) * amount).round().clamp(0.0, 255.0) as u32
    };
    (mix(16) << 16) | (mix(8) << 8) | mix(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_resolves() {
        for (name, _) in PRESETS {
            assert!(by_name(name).is_some(), "{name}");
        }
    }

    #[test]
    fn every_preset_can_be_written_on() {
        // The one thing a palette must not do is put text nobody can read on
        // its own accent — which is exactly where a name goes when a row is
        // selected.
        for (name, s) in PRESETS {
            for (what, ground) in [("accent", s.accent), ("sel", s.sel), ("status", s.status)] {
                let ink = readable_on(ground);
                assert!(
                    contrast(ink, ground) >= 4.5,
                    "{name}: {what} #{ground:06x} takes no readable text ({:.1}:1)",
                    contrast(ink, ground)
                );
            }
        }
    }

    #[test]
    fn light_and_dark_are_told_apart_by_the_number() {
        // The names say which way round these are; the luminance has to agree,
        // or the window will paint dark text on a dark ground.
        assert!(by_name("solarized-light").unwrap().is_light());
        assert!(by_name("github-light").unwrap().is_light());
        assert!(!by_name("dracula").unwrap().is_light());
        assert!(!by_name("tokyo-night").unwrap().is_light());
    }
}
