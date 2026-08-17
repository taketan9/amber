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

#[cfg(windows)]
mod platform {
    use super::Icon;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BI_RGB,
        DIB_RGB_COLORS,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL,
    };
    use windows_sys::Win32::UI::Shell::{
        SHDefExtractIconW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_ICONLOCATION,
        SHGFI_LARGEICON, SHGFI_SMALLICON, SHGFI_USEFILEATTRIBUTES,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    /// A Rust string as the NUL-terminated UTF-16 every `…W` call wants.
    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Above this, ask for the large icon rather than the small one. The shell
    /// has exactly two sizes to offer through this call — 16 and 32 logical
    /// pixels, scaled by the display — and the bigger one is right for
    /// everything cian draws except a tiny row.
    const SMALL_ABOVE: u32 = 20;

    pub fn icon_for(path: &Path, size: u32) -> Option<Icon> {
        let path = wide(path.as_os_str());
        fetch(path.as_ptr(), 0, size, false)
    }

    pub fn icon_for_type(ext: &str, is_dir: bool, size: u32) -> Option<Icon> {
        // `SHGFI_USEFILEATTRIBUTES` means "answer for a name of this shape,
        // never mind whether it exists" — which is the only question that can
        // be asked about a file on a server. A bare extension is not a name, so
        // it is made into one.
        let name = if is_dir {
            "cian-remote-directory".to_string()
        } else if ext.is_empty() {
            "cian-remote-file".to_string()
        } else {
            format!("cian-remote-file.{ext}")
        };
        let attrs = if is_dir { FILE_ATTRIBUTE_DIRECTORY } else { FILE_ATTRIBUTE_NORMAL };
        let name = wide(std::ffi::OsStr::new(&name));
        fetch(name.as_ptr(), attrs, size, true)
    }

    /// Ask the shell for an icon, and take its pixels.
    ///
    /// Two routes, in this order:
    ///
    /// 1. **Where the icon lives, then the icon at the size wanted.** The shell
    ///    will say which file and which index a kind of document takes its icon
    ///    from, and `SHDefExtractIconW` will then extract *that* at any size.
    ///    This is the one that matters for the grid, whose tiles are a hundred
    ///    pixels across.
    /// 2. **The system image list.** `SHGFI_ICON` hands back a ready-made icon,
    ///    but only in the two sizes the shell keeps — 32 and 16 logical pixels.
    ///    Right for a listing row, blown up in a grid, and the only answer for a
    ///    file whose icon is not a resource in some other file at all.
    fn fetch(path: *const u16, attrs: u32, size: u32, by_type: bool) -> Option<Icon> {
        if let Some(icon) = at_size(path, attrs, size, by_type) {
            return Some(icon);
        }
        let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
        let mut flags = SHGFI_ICON
            | if size > SMALL_ABOVE { SHGFI_LARGEICON } else { SHGFI_SMALLICON };
        if by_type {
            flags |= SHGFI_USEFILEATTRIBUTES;
        }
        // SAFETY: `path` is a NUL-terminated wide string owned by the caller
        // for the length of this call, and `info` is the struct this call is
        // documented to fill, with its own size passed alongside.
        let ok = unsafe {
            SHGetFileInfoW(
                path,
                attrs,
                &mut info,
                std::mem::size_of::<SHFILEINFOW>() as u32,
                flags,
            )
        };
        if ok == 0 || info.hIcon.is_null() {
            return None;
        }
        let out = unsafe { pixels_of(info.hIcon) };
        // The icon is ours now, and leaking one per file would be a leak per
        // *row of every listing*.
        unsafe { DestroyIcon(info.hIcon) };
        out
    }

    /// Route 1: the icon this file takes its picture from, drawn at `size`.
    ///
    /// `None` whenever the shell has no icon *location* to give — a file whose
    /// icon is generated rather than stored, which the caller then asks for the
    /// ready-made way.
    fn at_size(path: *const u16, attrs: u32, size: u32, by_type: bool) -> Option<Icon> {
        let size = size.clamp(16, 256);
        let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
        let mut flags = SHGFI_ICONLOCATION;
        if by_type {
            flags |= SHGFI_USEFILEATTRIBUTES;
        }
        // SAFETY: as in `fetch` — a NUL-terminated wide string the caller owns,
        // and a struct whose size is passed with it.
        let ok = unsafe {
            SHGetFileInfoW(path, attrs, &mut info, std::mem::size_of::<SHFILEINFOW>() as u32, flags)
        };
        if ok == 0 || info.szDisplayName[0] == 0 {
            return None;
        }
        let mut icon: HICON = std::ptr::null_mut();
        // Both halves of `niconsize` are set to what is wanted: the low one is
        // the large icon's size and the high one the small icon's, and only the
        // large one is being asked for here.
        let want = size | (size << 16);
        // SAFETY: `szDisplayName` is the NUL-terminated path the call above
        // wrote into it, and `icon` is a handle slot this frame owns.
        let hr = unsafe {
            SHDefExtractIconW(
                info.szDisplayName.as_ptr(),
                info.iIcon,
                0,
                &mut icon,
                std::ptr::null_mut(),
                want,
            )
        };
        if hr != 0 || icon.is_null() {
            return None;
        }
        let out = unsafe { pixels_of(icon) };
        unsafe { DestroyIcon(icon) };
        out
    }

    /// The pixels behind an `HICON`, as tightly-packed RGBA8.
    ///
    /// An icon is two bitmaps — colour and mask — and the colour one is a DDB,
    /// which is to say a handle to something the driver owns rather than bytes
    /// that can be read. `GetDIBits` is the way across: it is asked for a
    /// top-down 32-bit image and writes one, converting whatever the icon
    /// actually is.
    ///
    /// # Safety
    /// `icon` must be a live icon handle.
    unsafe fn pixels_of(icon: HICON) -> Option<Icon> {
        let mut ii: ICONINFO = std::mem::zeroed();
        if GetIconInfo(icon, &mut ii) == 0 {
            return None;
        }
        let out = decode(&ii);
        // `GetIconInfo` hands over copies of both bitmaps, and they are the
        // caller's to delete.
        if !ii.hbmColor.is_null() {
            DeleteObject(ii.hbmColor);
        }
        if !ii.hbmMask.is_null() {
            DeleteObject(ii.hbmMask);
        }
        out
    }

    /// The colour bitmap of an icon, alpha and all.
    ///
    /// # Safety
    /// Both bitmaps in `ii` must be live handles.
    unsafe fn decode(ii: &ICONINFO) -> Option<Icon> {
        let mut bm: BITMAP = std::mem::zeroed();
        let got = GetObjectW(
            ii.hbmColor,
            std::mem::size_of::<BITMAP>() as i32,
            &mut bm as *mut BITMAP as *mut core::ffi::c_void,
        );
        if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
            // A monochrome icon has no colour bitmap at all. Rare enough
            // (nothing the shell has shipped this century) to leave to the
            // caller's fallback rather than to grow a second decoder for.
            return None;
        }
        let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);
        let mut rgba = read_bits(ii.hbmColor, w, h)?;
        // BGRA on the wire, RGBA in the texture.
        for px in rgba.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        // A 32-bit icon carries its own alpha and that is the end of it. An
        // older one has none — every byte zero, which would draw as nothing at
        // all — and says what is transparent in its mask instead: set where the
        // background shows through.
        if rgba.chunks_exact(4).all(|px| px[3] == 0) {
            let mask = read_bits(ii.hbmMask, w, h);
            for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
                px[3] = match &mask {
                    Some(m) if m[i * 4] > 127 => 0,
                    _ => 255,
                };
            }
        }
        Some(Icon { width: w, height: h, rgba })
    }

    /// A GDI bitmap's pixels as top-down 32-bit BGRA.
    ///
    /// # Safety
    /// `bitmap` must be a live bitmap handle of at least `w`×`h`.
    unsafe fn read_bits(
        bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP,
        w: u32,
        h: u32,
    ) -> Option<Vec<u8>> {
        if bitmap.is_null() {
            return None;
        }
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader.biSize = std::mem::size_of::<
            windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER,
        >() as u32;
        bi.bmiHeader.biWidth = w as i32;
        // Negative: rows top to bottom, the way every other picture in this
        // program is stored. A positive height would hand back the icon
        // upside down.
        bi.bmiHeader.biHeight = -(h as i32);
        bi.bmiHeader.biPlanes = 1;
        bi.bmiHeader.biBitCount = 32;
        bi.bmiHeader.biCompression = BI_RGB;
        let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
        let dc = GetDC(std::ptr::null_mut());
        if dc.is_null() {
            return None;
        }
        let lines = GetDIBits(
            dc,
            bitmap,
            0,
            h,
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            &mut bi,
            DIB_RGB_COLORS,
        );
        ReleaseDC(std::ptr::null_mut(), dc);
        (lines != 0).then_some(buf)
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use super::Icon;
    use std::path::Path;

    /// No system icons here. On Linux there is no single answer — the icon a
    /// file gets depends on the desktop, its theme and its index — so the front
    /// end's own Nerd Font glyph stays the right call, and saying `None` is how
    /// it is asked for.
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
