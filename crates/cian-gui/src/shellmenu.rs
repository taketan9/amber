//! The desktop's own right-click menu, for the files cian is showing.
//!
//! cian has always had a menu of its own, and it carries the actions cian knows
//! how to do — including the ones it borrows from the system, like "open with"
//! and "reveal". What it cannot carry is everything *else* the desktop puts
//! there: the entries every installed program adds for itself. On Windows those
//! are the point of the menu — 7-Zip, the version control client, the antivirus,
//! "Open in Terminal", "Edit with…" — and a file manager whose right-click does
//! not have them is a file manager one has to leave.
//!
//! So in the desktop-shaped views, the desktop's menu is what right-click
//! opens. cian's own is still one key away (Shift+right-click), and is still
//! what the classic view shows: that view is a terminal that happens to be a
//! file manager, and the shell's menu would be a stranger in it.
//!
//! Only Windows has this. macOS has no API for "give me the Finder's menu for
//! this file" — the Finder builds it privately — and on Linux it belongs to a
//! desktop environment rather than to the system, so both keep cian's own.

use std::path::Path;

/// Show the system's context menu for `paths` at a point in *screen* pixels,
/// and carry out whatever is chosen.
///
/// Returns whether a menu was shown. `false` means this platform has none to
/// show and the caller should open cian's own instead.
pub fn show(window: &winit::window::Window, paths: &[std::path::PathBuf], at: (i32, i32)) -> bool {
    if paths.is_empty() {
        return false;
    }
    platform::show(window, paths, at)
}

/// Whether this platform can show a system menu at all. Asked before a click is
/// routed, so the decision does not depend on a menu failing to appear.
pub fn available() -> bool {
    cfg!(windows)
}

/// Whether a path is one the system can be asked about — the same rule the
/// drag out follows, and for the same reason.
pub fn addressable(path: &Path) -> bool {
    path.exists()
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        IContextMenu, IShellFolder, ILCreateFromPathW, ILFree, SHBindToParent, CMF_NORMAL,
        CMINVOKECOMMANDINFO,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreatePopupMenu, DestroyMenu, SetForegroundWindow, TrackPopupMenuEx, SW_SHOWNORMAL,
        TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    };

    /// Where the shell's own command ids start. It is handed a range to number
    /// its entries within, and the number that comes back out of the menu is
    /// relative to the bottom of that range — which is the one subtraction in
    /// this file that has to be right.
    const FIRST_ID: u32 = 1;
    const LAST_ID: u32 = 0x7fff;

    pub fn show(window: &winit::window::Window, paths: &[PathBuf], at: (i32, i32)) -> bool {
        let Some(hwnd) = hwnd(window) else { return false };
        // One folder's worth at a time: a shell menu is built by the folder the
        // items live in, and items from two folders have no single one to ask.
        // The first path decides, and anything elsewhere is left out rather
        // than being quietly attributed to the wrong parent.
        let Some(first) = paths.first() else { return false };
        let parent = first.parent().map(|p| p.to_path_buf());
        let together: Vec<&PathBuf> =
            paths.iter().filter(|p| p.parent().map(|q| q.to_path_buf()) == parent).collect();

        let mut ids: Vec<*const ITEMIDLIST> = Vec::with_capacity(together.len());
        for p in &together {
            let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
            // SAFETY: a NUL-terminated wide string that outlives the call.
            let id = unsafe { ILCreateFromPathW(PCWSTR(wide.as_ptr())) };
            if id.is_null() {
                for id in ids {
                    unsafe { ILFree(Some(id)) };
                }
                return false;
            }
            ids.push(id);
        }

        let shown = unsafe { run(hwnd, &ids, at) };
        for id in ids {
            // SAFETY: each came from `ILCreateFromPathW` and is freed once.
            unsafe { ILFree(Some(id)) };
        }
        shown
    }

    /// Build the menu, show it, and invoke what was picked.
    ///
    /// # Safety
    /// `ids` must be live absolute id lists, all with the same parent.
    unsafe fn run(hwnd: HWND, ids: &[*const ITEMIDLIST], at: (i32, i32)) -> bool {
        let Some(&first) = ids.first() else { return false };
        // The folder the items are in, and each item's id *within* it: a
        // context menu is asked of the parent, about its children.
        let mut child: *mut ITEMIDLIST = std::ptr::null_mut();
        let Ok(folder) = SHBindToParent::<IShellFolder>(first, Some(&mut child)) else {
            return false;
        };
        if child.is_null() {
            return false;
        }
        // The remaining items' child ids, found the same way. Each is owned by
        // its parent id list, so none of these are freed here.
        let mut children: Vec<*const ITEMIDLIST> = vec![child];
        for &id in ids.iter().skip(1) {
            let mut c: *mut ITEMIDLIST = std::ptr::null_mut();
            if SHBindToParent::<IShellFolder>(id, Some(&mut c)).is_ok() && !c.is_null() {
                children.push(c);
            }
        }

        let Ok(menu) = folder.GetUIObjectOf::<IContextMenu>(hwnd, &children, None) else {
            return false;
        };
        let Ok(hmenu) = CreatePopupMenu() else { return false };

        let filled = menu.QueryContextMenu(hmenu, 0, FIRST_ID, LAST_ID, CMF_NORMAL);
        if filled.is_err() {
            let _ = DestroyMenu(hmenu);
            return false;
        }

        // A popup menu belongs to the foreground window, and a menu whose owner
        // is behind something else never goes away when clicked off.
        let _ = SetForegroundWindow(hwnd);
        let picked = TrackPopupMenuEx(
            hmenu,
            (TPM_RETURNCMD | TPM_LEFTALIGN | TPM_RIGHTBUTTON).0,
            at.0,
            at.1,
            hwnd,
            None,
        );

        if picked.as_bool() {
            // `TPM_RETURNCMD` hands back the id rather than posting it, so the
            // command is invoked here — with the id made relative to the range
            // the shell was given, which is what `InvokeCommand` expects.
            let id = picked.0 as u32;
            let verb = (id - FIRST_ID) as usize;
            let mut info: CMINVOKECOMMANDINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32;
            info.hwnd = hwnd;
            info.lpVerb = windows::core::PCSTR(verb as *const u8);
            info.nShow = SW_SHOWNORMAL.0;
            let _ = menu.InvokeCommand(&info);
        }
        let _ = DestroyMenu(hmenu);
        true
    }

    /// Where in screen pixels a point in the window is.
    pub fn to_screen(window: &winit::window::Window, x: f64, y: f64) -> (i32, i32) {
        let origin = window
            .inner_position()
            .map(|p| (p.x, p.y))
            .unwrap_or((0, 0));
        (origin.0 + x as i32, origin.1 + y as i32)
    }

    fn hwnd(window: &winit::window::Window) -> Option<HWND> {
        let handle = window.window_handle().ok()?;
        match handle.as_ref() {
            RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut core::ffi::c_void)),
            _ => None,
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::PathBuf;

    pub fn show(_window: &winit::window::Window, _paths: &[PathBuf], _at: (i32, i32)) -> bool {
        false
    }

    pub fn to_screen(_window: &winit::window::Window, x: f64, y: f64) -> (i32, i32) {
        (x as i32, y as i32)
    }
}

/// A point inside the window, in screen pixels — where a popup menu wants it.
pub fn to_screen(window: &winit::window::Window, x: f64, y: f64) -> (i32, i32) {
    platform::to_screen(window, x, y)
}
