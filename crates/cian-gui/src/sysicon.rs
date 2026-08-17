//! The icon the desktop already uses for a file.
//!
//! cian has drawn file-type icons from a Nerd Font since it began, which is the
//! only thing a terminal can do. A window can ask the system instead, and the
//! system knows things a font never will: which application owns a `.sketch`,
//! what that application's icon looks like, that this particular folder is the
//! Downloads folder, that this `.app` has a face of its own.
//!
//! Answers are cached by the caller, not here: this is the slow part (the first
//! call for an unseen file type touches Launch Services) and it must never run
//! on the frame path more than once per kind of file.
//!
//! Anywhere the system has no opinion — and on any platform not implemented
//! here — the answer is `None` and cian falls back to the font glyph it has
//! always used.

use std::path::Path;

/// A decoded icon: width, height, and tightly-packed RGBA8.
pub struct Icon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The system's icon for `path`, rendered at `size` pixels square.
///
/// `size` should be the size it will actually be drawn at: an icon is a set of
/// pictures at several resolutions plus rules about which to use, and asking
/// for the size wanted lets the system apply them.
pub fn icon_for(path: &Path, size: u32) -> Option<Icon> {
    platform::icon_for(path, size)
}

/// The system's icon for a *kind* of file, named by extension.
///
/// For anything cian is showing that is not on this disk — a remote pane over
/// SFTP, a listing from inside an archive — there is no path to ask about. The
/// type is still known, and the type is what the icon is mostly about anyway:
/// a `.pdf` on a server should look like a `.pdf`.
pub fn icon_for_type(ext: &str, is_dir: bool, size: u32) -> Option<Icon> {
    platform::icon_for_type(ext, is_dir, size)
}

#[cfg(target_os = "macos")]
mod platform {
    use super::Icon;
    use std::path::Path;

    use objc2::ClassType;
    use objc2_app_kit::{
        NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace, NSGraphicsContext,
        NSWorkspace,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

    pub fn icon_for_type(ext: &str, is_dir: bool, size: u32) -> Option<Icon> {
        let ws = unsafe { NSWorkspace::sharedWorkspace() };
        // A folder has no extension to ask about, so it is asked about by name:
        // every system has one somewhere, and this is where macOS keeps it.
        let icon = if is_dir {
            // A folder has no extension to ask about, so it is asked about by
            // name: the root is a folder, and its icon is the folder icon.
            unsafe { ws.iconForFile(&NSString::from_str("/")) }
        } else {
            // `iconForFileType` is deprecated in favour of `iconForContentType`,
            // and the objc2 bindings this build uses do not expose the
            // replacement at all — the whole `UTType` surface is absent from
            // objc2-app-kit 0.2. Deprecated and present beats modern and
            // missing; swap it when the bindings catch up.
            #[allow(deprecated)]
            unsafe {
                ws.iconForFileType(&NSString::from_str(if ext.is_empty() {
                    "public.data"
                } else {
                    ext
                }))
            }
        };
        draw(&icon, size.max(1))
    }

    pub fn icon_for(path: &Path, size: u32) -> Option<Icon> {
        let size = size.max(1);
        // NSWorkspace answers for a path that exists; for one that does not it
        // still answers, with the generic document icon, which is the right
        // answer for a file that vanished between listing and drawing.
        let ns_path = NSString::from_str(path.to_str()?);
        let icon = unsafe { NSWorkspace::sharedWorkspace().iconForFile(&ns_path) };
        draw(&icon, size)
    }

    /// Render an `NSImage` into RGBA at the size it will be drawn.
    fn draw(icon: &objc2_app_kit::NSImage, size: u32) -> Option<Icon> {
        // Tell the image how big it is about to be.
        //
        // An icon is a set of pictures at several resolutions, and `NSImage`
        // chooses between them by its own `size` — not by the rectangle it is
        // asked to draw into. What `iconForFile:` hands back is 32×32 points,
        // so drawing it into a hundred-pixel square picked the 32-pixel picture
        // and stretched it. The folders that looked sharp were the ones with
        // custom artwork big enough to survive that; every plain blue folder
        // was a blown-up thumbnail.
        unsafe { icon.setSize(NSSize::new(size as f64, size as f64)) };

        // Draw it, rather than asking for a representation and decoding that.
        //
        // The obvious route — `TIFFRepresentation` into an image decoder — is
        // what this used to do, and it silently produced *transparent* pictures
        // for exactly the files whose icons the system builds rather than
        // stores: documents with a thumbnail, anything Launch Services renders
        // on demand. Their TIFF has the representations but not the pixels.
        // Asking AppKit to draw the icon at the size wanted makes it do that
        // work, and there is nothing left to decode or guess about.
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                size as isize,
                size as isize,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                (size * 4) as isize,
                32,
            )
        }?;

        let ctx = unsafe { NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep) }?;
        unsafe {
            NSGraphicsContext::saveGraphicsState_class();
            NSGraphicsContext::setCurrentContext(Some(&ctx));
            let rect = NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(size as f64, size as f64),
            );
            // `Copy` rather than `SourceOver`: the bitmap starts as whatever
            // memory it was handed, and blending onto uninitialised pixels is
            // how an icon comes out haunted.
            icon.drawInRect_fromRect_operation_fraction(
                rect,
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
                NSCompositingOperation::Copy,
                1.0,
            );
            NSGraphicsContext::restoreGraphicsState_class();
        }

        let data = unsafe { rep.bitmapData() };
        if data.is_null() {
            return None;
        }
        let len = (size * size * 4) as usize;
        // SAFETY: the bitmap was created with these exact dimensions, four
        // 8-bit samples per pixel and a row stride of `size * 4`, so this is the
        // buffer AppKit just drew into. Copied out before `rep` is dropped.
        let rgba = unsafe { std::slice::from_raw_parts(data, len) }.to_vec();

        // A fully transparent result means the draw produced nothing. Better to
        // say so and let the caller fall back than to draw an empty square.
        if rgba.chunks_exact(4).all(|px| px[3] == 0) {
            return None;
        }
        Some(Icon { width: size, height: size, rgba })
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::Icon;
    use std::path::Path;

    /// No system icons here yet. Windows has `SHGetFileInfo`, which is the next
    /// one to write; on Linux there is no single answer, so the font glyph may
    /// stay the right call there.
    pub fn icon_for(_path: &Path, _size: u32) -> Option<Icon> {
        None
    }

    pub fn icon_for_type(_ext: &str, _is_dir: bool, _size: u32) -> Option<Icon> {
        None
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// Write the system icon out at a few sizes so it can be looked at.
    ///
    /// Not an assertion — "does this look sharp" is not something a test can
    /// answer. It exists because the alternative was guessing at the cause of a
    /// soft icon from a screenshot, and guessing was already wrong once.
    #[test]
    #[ignore = "writes files for a human to look at: cargo test -p cian-gui -- --ignored"]
    fn dump_icons_for_inspection() {
        let out = std::path::Path::new("/tmp/cian-icons");
        std::fs::create_dir_all(out).unwrap();
        for size in [32u32, 64, 88, 128, 256] {
            let icon = icon_for(std::path::Path::new("/Users"), size)
                .unwrap_or_else(|| panic!("no icon at {size}"));
            let buf = image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba).unwrap();
            buf.save(out.join(format!("folder-{size}.png"))).unwrap();
        }
        eprintln!("wrote {}", out.display());
    }
}
