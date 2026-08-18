//! The picture inside a file, at the size it is about to be shown.
//!
//! cian's image popup draws with half-blocks — two pixels to a cell, the most
//! a terminal can do without a graphics protocol — and in a window there is no
//! reason for it: the window already composites real pictures over the cells
//! for the file icons, and a photograph is the same job at a larger size.
//!
//! Decoding happens on the icon worker (see [`crate::iconjob`]), because a
//! twelve-megapixel JPEG takes far longer to decode than a frame lasts, and a
//! frame that waits for one is a window that stops.

use std::path::Path;

use crate::sysicon::Icon;

/// Decode `path` and scale it to fit a `w`×`h` pixel box, keeping its shape.
///
/// Fitted, not filled: a picture cropped to the box would be showing less of
/// the file than the file has, and this is a viewer. Lanczos on the way down,
/// which is where the quality question actually is — a photograph reduced by a
/// factor of six with a box filter looks like a photograph of a photograph.
pub fn decode(path: &Path, w: u32, h: u32) -> Option<Icon> {
    if w == 0 || h == 0 {
        return None;
    }
    let img = image::open(path).ok()?;
    let fitted = img.resize(w, h, image::imageops::FilterType::Lanczos3);
    let rgba = fitted.to_rgba8();
    Some(Icon { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w`×`h` PNG in a temporary directory.
    fn png(dir: &Path, name: &str, w: u32, h: u32) -> std::path::PathBuf {
        let mut img = image::RgbImage::new(w, h);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]);
        }
        let path = dir.join(name);
        img.save(&path).unwrap();
        path
    }

    #[test]
    fn a_wide_picture_fits_the_width_and_keeps_its_shape() {
        let d = tempfile::tempdir().unwrap();
        let p = png(d.path(), "wide.png", 400, 100);
        let out = decode(&p, 200, 200).expect("decoded");
        assert_eq!((out.width, out.height), (200, 50), "fitted, not stretched");
        assert_eq!(out.rgba.len(), (200 * 50 * 4) as usize, "four bytes a pixel");
    }

    #[test]
    fn a_tall_one_fits_the_height() {
        let d = tempfile::tempdir().unwrap();
        let p = png(d.path(), "tall.png", 100, 400);
        let out = decode(&p, 200, 200).expect("decoded");
        assert_eq!((out.width, out.height), (50, 200));
    }

    #[test]
    fn a_small_one_is_scaled_up_to_the_box() {
        let d = tempfile::tempdir().unwrap();
        let p = png(d.path(), "small.png", 20, 10);
        let out = decode(&p, 400, 400).expect("decoded");
        assert_eq!((out.width, out.height), (400, 200), "a viewer fills what it is given");
    }

    #[test]
    fn a_box_with_no_room_in_it_is_not_decoded() {
        let d = tempfile::tempdir().unwrap();
        let p = png(d.path(), "any.png", 20, 10);
        assert!(decode(&p, 0, 100).is_none());
    }

    #[test]
    fn something_that_is_not_a_picture_says_so_rather_than_panicking() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("notes.txt");
        std::fs::write(&p, b"this is not a png").unwrap();
        assert!(decode(&p, 100, 100).is_none());
    }
}
