//! Who the window says it is.
//!
//! A bare executable has no identity on macOS beyond its filename: the menu bar
//! calls it `cian-gui` and the Dock shows the blank rocket every unbundled
//! binary gets. Both are answerable at runtime — the name through
//! `NSProcessInfo`, the icon through `NSApplication` — so cian looks like
//! itself the moment it opens, without waiting on a `.app` bundle.
//!
//! What still needs the bundle: the icon Finder draws on the *file*. That one
//! lives in the bundle's resources, not in the running process, and no call
//! from inside can change it.

/// cian's own icon, compiled in so the window has it wherever it runs from.
const ICON: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../cian.ico"));

/// The name the desktop should use, rather than the executable's filename.
const NAME: &str = "cian";

/// How much of the canvas a macOS app icon's artwork is supposed to fill.
///
/// Every icon in the Dock sits on the same invisible grid, and the artwork
/// occupies about four fifths of it — the rest is transparent margin. cian's
/// `.ico` fills its square edge to edge, which is correct for a Windows icon
/// and makes it a size larger than everything beside it on a Mac.
const ARTWORK: f32 = 0.80;

/// Put the artwork back inside the margin macOS expects.
fn inset(src: &image::DynamicImage) -> image::DynamicImage {
    use image::GenericImageView;
    let (w, h) = src.dimensions();
    let side = w.max(h);
    let art = ((side as f32) * ARTWORK).round() as u32;
    let scaled = src.resize(art, art, image::imageops::FilterType::Lanczos3);
    let mut canvas = image::RgbaImage::new(side, side);
    let (aw, ah) = (scaled.width(), scaled.height());
    image::imageops::overlay(
        &mut canvas,
        &scaled.to_rgba8(),
        ((side - aw) / 2) as i64,
        ((side - ah) / 2) as i64,
    );
    image::DynamicImage::ImageRgba8(canvas)
}

/// Give the running application cian's name and icon. Does nothing anywhere
/// the platform has no such notion.
pub fn announce() {
    platform::announce();
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::ClassType;
    use objc2_foundation::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{NSData, NSProcessInfo, NSString};

    pub fn announce() {
        // The menu bar reads this, and for an unbundled binary it starts out as
        // the executable's filename.
        let name = NSString::from_str(super::NAME);
        unsafe { NSProcessInfo::processInfo().setProcessName(&name) };

        let Some(mtm) = MainThreadMarker::new() else { return };

        // NSImage will not read an .ico, so it is turned into a PNG on the way
        // — decoding is something this binary can already do, and the icon is
        // small enough that doing it at startup costs nothing worth measuring.
        let Ok(decoded) = image::load_from_memory(super::ICON) else { return };
        let decoded = super::inset(&decoded);
        let mut png = Vec::new();
        if decoded
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .is_err()
        {
            return;
        }

        let data = NSData::with_bytes(&png);
        let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else { return };
        // SAFETY: on the main thread (the marker proves it), with an image this
        // function has just been handed sole ownership of.
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    /// Windows takes its name and icon from the executable's own resources,
    /// which is a build-time matter rather than a runtime one; on Linux it is
    /// the `.desktop` file's job. Neither is answerable from here.
    pub fn announce() {}
}
