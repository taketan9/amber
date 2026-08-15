//! Decoding an image down to a terminal-cell grid for the F3 preview.
//!
//! cian renders images with **half blocks** (`▀`): each character cell shows two
//! stacked pixels — the upper as the glyph's foreground, the lower as its
//! background. That needs nothing beyond 24-bit colour, so it works in every
//! modern terminal (Windows Terminal, iTerm2, …) without a graphics protocol or
//! a particular version — the fidelity is coarse, but the preview is universal.
//!
//! This module does the format-agnostic decode and the aspect-preserving resize
//! (pure Rust via the `image` crate); turning the grid into styled cells is the
//! UI layer's job.

use std::path::Path;

use anyhow::{Context, Result};

/// One 24-bit colour.
pub type Rgb = (u8, u8, u8);

/// An image reduced to a grid of terminal cells. Each cell carries the two
/// pixels a `▀` half-block draws (top = foreground, bottom = background).
#[derive(Debug, Clone)]
pub struct Thumb {
    pub cols: u16,
    pub rows: u16,
    /// Row-major, `rows * cols` cells of `(top, bottom)` pixels.
    pub cells: Vec<(Rgb, Rgb)>,
    /// The source image's pixel dimensions, for the caption.
    pub src_w: u32,
    pub src_h: u32,
}

/// File extensions cian previews as images.
pub fn is_image(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp")
    )
}

/// Decode `path` and fit it into a `max_cols` × `max_rows` cell box, preserving
/// aspect ratio. Because a cell is two pixels tall, the pixel box is
/// `max_cols` × `2*max_rows`.
pub fn thumbnail(path: &Path, max_cols: u16, max_rows: u16) -> Result<Thumb> {
    let img = image::open(path).with_context(|| format!("decode {}", path.display()))?;
    thumbnail_of(&img, max_cols, max_rows)
}

/// The same, from an image that has already been decoded.
///
/// Decoding is nearly all of the cost — a few megabytes of PNG has to be
/// unpacked whole before anything can be scaled — so a caller that has done
/// it once (off the drawing thread, say) does not do it again per resize.
pub fn thumbnail_of(img: &image::DynamicImage, max_cols: u16, max_rows: u16) -> Result<Thumb> {
    let rgb = img.to_rgb8();
    let (sw, sh) = rgb.dimensions();
    if sw == 0 || sh == 0 {
        anyhow::bail!("empty image");
    }

    let box_w = (max_cols.max(1)) as f64;
    let box_h = (max_rows.max(1) as f64) * 2.0;
    // Fit inside the box (scaling up small images so they are actually visible).
    let scale = (box_w / sw as f64).min(box_h / sh as f64);
    let tw = ((sw as f64 * scale).round() as u32).max(1);
    // The pixel height must be even so it packs into whole cells.
    let mut th = ((sh as f64 * scale).round() as u32).max(2);
    if th % 2 == 1 {
        th += 1;
    }

    let resized = image::imageops::resize(&rgb, tw, th, image::imageops::FilterType::Triangle);
    let cols = tw as u16;
    let rows = (th / 2) as u16;
    let mut cells = Vec::with_capacity(cols as usize * rows as usize);
    for ry in 0..rows as u32 {
        for cx in 0..cols as u32 {
            let top = resized.get_pixel(cx, ry * 2);
            let bot = resized.get_pixel(cx, ry * 2 + 1);
            cells.push(((top[0], top[1], top[2]), (bot[0], bot[1], bot[2])));
        }
    }
    Ok(Thumb { cols, rows, cells, src_w: sw, src_h: sh })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_image_extensions() {
        assert!(is_image(Path::new("a.PNG")));
        assert!(is_image(Path::new("a.jpg")));
        assert!(is_image(Path::new("a.webp")));
        assert!(!is_image(Path::new("a.txt")));
        assert!(!is_image(Path::new("a")));
    }

    #[test]
    fn fits_within_the_box_preserving_aspect() {
        // A 40×20 red PNG, previewed into an 8×8 cell box (16 px tall).
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("red.png");
        let mut img = image::RgbImage::new(40, 20);
        for px in img.pixels_mut() {
            *px = image::Rgb([200, 20, 20]);
        }
        img.save(&p).unwrap();

        let t = thumbnail(&p, 8, 8).unwrap();
        assert_eq!((t.src_w, t.src_h), (40, 20));
        // Landscape 2:1 → width-limited: scale 0.2 → 8×4 px → 8 cols, 2 rows.
        assert_eq!(t.cols, 8);
        assert_eq!(t.rows, 2);
        assert_eq!(t.cells.len(), 16);
        // A solid image comes back solid in that colour.
        assert_eq!(t.cells[0], ((200, 20, 20), (200, 20, 20)));
    }

    #[test]
    fn a_non_image_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("notimg.png");
        std::fs::write(&p, b"not really a png").unwrap();
        assert!(thumbnail(&p, 8, 8).is_err());
    }
}
