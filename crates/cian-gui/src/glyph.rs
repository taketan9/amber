//! Drawing one of cian's own icons as a picture.
//!
//! The classic view's file-type icons come from a Nerd Font, and they cannot be
//! drawn in a cell without losing their right-hand edge: the ink of those
//! glyphs runs up to two thirds past the advance they were given, and the text
//! renderer rasterises each one into a tile exactly one cell wide. There is no
//! Japanese Nerd Font with the icons scaled to fit, so the glyph itself cannot
//! be made narrower.
//!
//! A picture has no cell to be clipped by. So the same glyph, from the same
//! font, is rasterised here at whatever size the row is and handed to the
//! picture layer — the look cian was built around, drawn whole.

use ab_glyph::{Font, FontRef, Glyph, Point, ScaleFont};

/// Rasterise `ch` into an RGBA square of `size` pixels, in `rgb`.
///
/// Returns `None` when the font has no such glyph, or when it has one with no
/// outline (a space). The caller then draws nothing, which is the same as what
/// the cell would have shown.
pub fn render(font: &FontRef<'static>, ch: char, size: u32, rgb: (u8, u8, u8)) -> Option<Vec<u8>> {
    let size = size.max(1);
    let id = font.glyph_id(ch);

    // Scale so the glyph's *ink* fits the square, rather than so its em does.
    // These icons are drawn to overflow their advance; asking for "one em tall"
    // would put them straight back over the edge.
    let probe = font.as_scaled(size as f32);
    let outline = probe.outline_glyph(Glyph {
        id,
        scale: (size as f32).into(),
        position: Point { x: 0.0, y: 0.0 },
    })?;
    let bounds = outline.px_bounds();
    let (w, h) = (bounds.width(), bounds.height());
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let fit = (size as f32 * 0.92) / w.max(h);
    let scale = size as f32 * fit;

    let outline = font.as_scaled(scale).outline_glyph(Glyph {
        id,
        scale: scale.into(),
        position: Point { x: 0.0, y: 0.0 },
    })?;
    let bounds = outline.px_bounds();

    // Centre what was drawn inside the square: the bounds are where the glyph
    // put its ink relative to the origin, which for these is nowhere near the
    // middle of anything.
    let ox = (size as f32 - bounds.width()) / 2.0 - bounds.min.x;
    let oy = (size as f32 - bounds.height()) / 2.0 - bounds.min.y;

    let mut rgba = vec![0u8; (size * size * 4) as usize];
    outline.draw(|gx, gy, coverage| {
        let x = gx as f32 + bounds.min.x + ox;
        let y = gy as f32 + bounds.min.y + oy;
        if x < 0.0 || y < 0.0 || x >= size as f32 || y >= size as f32 {
            return;
        }
        let i = ((y as u32 * size + x as u32) * 4) as usize;
        rgba[i] = rgb.0;
        rgba[i + 1] = rgb.1;
        rgba[i + 2] = rgb.2;
        // Coverage is how much of the pixel the glyph covers, which is exactly
        // what alpha means here — the colour is flat.
        rgba[i + 3] = (coverage.clamp(0.0, 1.0) * 255.0) as u8;
    });
    Some(rgba)
}
