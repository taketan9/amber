//! Finding a font that can draw Japanese, icons and box drawing at one width.
//!
//! The renderer sizes its grid from a single font's `m` advance, so every
//! glyph it draws has to agree with that: ASCII one cell, 全角 two, box drawing
//! flush to the cell edges. One monospaced Japanese Nerd Font satisfies all of
//! it. Two fonts stitched together do not — a fallback chain whose faces
//! disagree on metrics puts the dingbats at the wrong scale and loses the ones
//! neither face has (`spikes/gui-spike/README.md` has the pictures).
//!
//! So: exactly one font, and the search stops at the first that loads.
//!
//! The intended answer is `bundled-font`, which compiles it in and makes the
//! binary self-contained — the whole point of a window is that there is nothing
//! left to install. Until a font is committed the search falls back to disk,
//! which is also how a different font gets tried without a rebuild.

use std::path::PathBuf;

/// `CIAN_FONT=/path/to/Font.ttf` beats everything below it.
const ENV: &str = "CIAN_FONT";

/// Where a Japanese Nerd Font tends to be once someone has downloaded one.
/// Checked in order; the first that parses wins.
const SEARCH: &[&str] = &[
    "~/Library/Fonts/HackGenConsoleNF-Regular.ttf",
    "~/Library/Fonts/UDEVGothicNF-Regular.ttf",
    "~/Library/Fonts/PlemolJPConsoleNF-Regular.ttf",
    "~/Downloads/HackGenConsoleNF-Regular.ttf",
    "C:/Windows/Fonts/HackGenConsoleNF-Regular.ttf",
    "/usr/share/fonts/truetype/hackgen/HackGenConsoleNF-Regular.ttf",
];

/// The font compiled into this binary, when there is one.
#[cfg(feature = "bundled-font")]
const BUNDLED: Option<&[u8]> =
    Some(include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/fonts/cian.ttf")));
#[cfg(not(feature = "bundled-font"))]
const BUNDLED: Option<&[u8]> = None;

fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}

/// The font's bytes and where they came from.
///
/// Leaked deliberately: the renderer's faces borrow this for as long as the
/// process draws anything, which is until it exits. A font read once and held
/// forever is not a leak worth an `Arc`.
pub fn load() -> Result<(&'static [u8], String), String> {
    if let Some(bytes) = BUNDLED {
        return Ok((bytes, "(bundled)".to_string()));
    }

    let mut tried = Vec::new();
    let named = std::env::var_os(ENV).map(PathBuf::from);
    let candidates = named.into_iter().chain(SEARCH.iter().map(|p| expand(p)));

    for path in candidates {
        match std::fs::read(&path) {
            Ok(bytes) => {
                let name = path.display().to_string();
                return Ok((Box::leak(bytes.into_boxed_slice()), name));
            }
            Err(_) => tried.push(path.display().to_string()),
        }
    }

    Err(format!(
        "cian-gui could not find a font.\n\n\
         It needs one monospaced Japanese Nerd Font — ASCII one cell wide, 全角 two,\n\
         with the Nerd Font icons in it. HackGen Console NF, UDEV Gothic NF and\n\
         PlemolJP Console NF are all of that shape.\n\n\
         Point it at one:\n    {ENV}=/path/to/Font.ttf cian-gui\n\n\
         or put it where it looks:\n{}\n",
        tried.iter().map(|p| format!("    {p}")).collect::<Vec<_>>().join("\n"),
    ))
}
