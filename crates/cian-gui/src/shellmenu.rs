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
//! So in the window, the desktop's menu is what the right button opens — in
//! every view. It was cian's own here, and the desktop's only in the two views
//! that look like a desktop; one button meaning two different menus depending
//! on the view is one menu too many, and the button that opens the system's
//! menu everywhere else in the machine should open the system's menu here.
//!
//! cian's own is a key away rather than a mode away: `Shift+Enter`, `M`,
//! `:menu` — and Shift+right-click, for a hand already on the mouse. In a
//! terminal, where there is no system menu to be had, the right button still
//! opens cian's.
//!
//! Windows hands its whole menu over: `IContextMenu` is the same object
//! Explorer asks, so what appears is exactly what Explorer would have shown,
//! third-party entries and all. There are *two* of those objects, and both are
//! asked here — see [`About`]. The one for a selection comes from the parent
//! folder (`GetUIObjectOf`) and carries copy, cut, rename, delete, properties;
//! the one for the folder you are looking at comes from the folder itself
//! (`CreateViewObject`) and is the only one that has ever carried **paste**.
//! A file manager that could copy and never paste would be half a file
//! manager, which is why the empty space between the files answers too.
//!
//! What is *not* forwarded is the menu's own window messages. A shell menu
//! whose submenus are built on demand — "New ▸", "Send to ▸", some
//! third-party ones — fills them in response to `WM_INITMENUPOPUP`, which
//! reaches an `IContextMenu2`/`IContextMenu3` only through a window
//! subclass. Without one, those submenus can come up empty. Every top-level
//! entry, paste included, works regardless.
//!
//! macOS has no such call. The Finder builds its menu privately and there is no
//! API that returns it — so what happens here is the next true thing: a real
//! AppKit menu, drawn and driven by macOS, carrying the actions the system
//! itself provides for a file. Open, and Open With listing the applications
//! Launch Services actually names for that file; reveal it in the Finder;
//! Quick Look; copy the path; move it to the Trash. What is missing, and cannot
//! be had, is the entries other applications install into the Finder's menu.
//!
//! Linux keeps cian's own: there the menu belongs to a desktop environment
//! rather than to the system, and there is no one thing to ask.

use std::path::{Path, PathBuf};

/// What the menu is to be about.
///
/// The distinction is Windows', and it is not a nicety: the shell builds *two*
/// different menus for a folder. The one for a folder as an **item** — the one
/// you get by right-clicking its icon — carries open, copy, cut, rename,
/// delete, properties. The one for a folder as a **place**, which is what a
/// right-click on the empty space in a window gives you, carries **paste**,
/// **new ▸** and refresh. They come from different COM calls, and the second
/// one is the one a file manager cannot do without: a copy with nowhere to be
/// pasted is half an operation.
pub enum About {
    /// These files and folders, as a selection.
    Items(Vec<PathBuf>),
    /// This folder, as the place you are looking at.
    // Read on the platforms that have a menu to show. On the ones that do not,
    // the caller still says what it *would* have asked about, and the stub
    // still answers "there is nothing to show" — so the payload is dead there
    // and only there.
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    Folder(PathBuf),
}

/// Show the system's context menu at a point in *screen* pixels, and carry out
/// whatever is chosen.
///
/// Returns whether a menu was shown. `false` means this platform has none to
/// show and the caller should open cian's own instead.
pub fn show(window: &winit::window::Window, about: &About, at: (i32, i32)) -> bool {
    if matches!(about, About::Items(paths) if paths.is_empty()) {
        return false;
    }
    platform::show(window, about, at)
}

/// Whether this platform can show a system menu at all. Asked before a click is
/// routed, so the decision does not depend on a menu failing to appear.
pub fn available() -> bool {
    cfg!(windows) || cfg!(target_os = "macos")
}

/// Whether a path is one the system can be asked about — the same rule the
/// drag out follows, and for the same reason.
pub fn addressable(path: &Path) -> bool {
    path.exists()
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::IBindCtx;
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        IContextMenu, IShellFolder, ILCreateFromPathW, ILFree, SHBindToObject, SHBindToParent,
        CMF_NORMAL, CMINVOKECOMMANDINFO,
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

    pub fn show(window: &winit::window::Window, about: &super::About, at: (i32, i32)) -> bool {
        match about {
            super::About::Items(paths) => items(window, paths, at),
            super::About::Folder(dir) => background(window, dir, at),
        }
    }

    /// The menu for a folder *as a place*: paste, new, refresh.
    ///
    /// A different object to the one below, from a different call. The items
    /// menu is asked of the parent folder about its children; this one is asked
    /// of the folder itself, and it is the only one that has ever carried
    /// paste — which is why right-clicking the empty space in Explorer gives
    /// you something else than right-clicking a file does.
    fn background(window: &winit::window::Window, dir: &Path, at: (i32, i32)) -> bool {
        let Some(hwnd) = hwnd(window) else { return false };
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        // SAFETY: a NUL-terminated wide string that outlives the call.
        //
        // Typed `*const` where it is bound rather than where it is used: the
        // pointer coercion happens at a `let` with a type on it, and does not
        // happen inside the `Some(..)` that frees it.
        let id: *const ITEMIDLIST = unsafe { ILCreateFromPathW(PCWSTR(wide.as_ptr())) };
        if id.is_null() {
            return false;
        }
        // SAFETY: `id` is a live absolute id list, freed once below.
        let shown = unsafe {
            // A null folder means "relative to the desktop", which is what an
            // absolute id list is relative to.
            match SHBindToObject::<_, _, IShellFolder>(
                None::<&IShellFolder>,
                id,
                None::<&IBindCtx>,
            ) {
                Ok(folder) => match folder.CreateViewObject::<IContextMenu>(hwnd) {
                    Ok(menu) => track(hwnd, &menu, at),
                    Err(_) => false,
                },
                Err(_) => false,
            }
        };
        // SAFETY: came from `ILCreateFromPathW` and is freed once.
        unsafe { ILFree(Some(id)) };
        shown
    }

    fn items(window: &winit::window::Window, paths: &[PathBuf], at: (i32, i32)) -> bool {
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
        track(hwnd, &menu, at)
    }

    /// Put a shell menu on screen, wait for it, and invoke what was picked.
    ///
    /// The same for both kinds of menu: where the `IContextMenu` came from is
    /// the only thing that differs, and by here it is just a menu.
    ///
    /// # Safety
    /// `menu` must be a live shell context menu for this window.
    unsafe fn track(hwnd: HWND, menu: &IContextMenu, at: (i32, i32)) -> bool {
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

#[cfg(target_os = "macos")]
mod platform {
    use std::cell::Cell;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{declare_class, msg_send_id, mutability, sel, ClassType, DeclaredClass};
    use objc2_app_kit::{
        NSMenu, NSMenuItem, NSPasteboard, NSPasteboardTypeString, NSView, NSWorkspace,
    };
    use objc2_foundation::{
        MainThreadMarker, NSArray, NSFileManager, NSObject, NSObjectProtocol, NSString, NSURL,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// Nothing chosen. The menu can be dismissed, and dismissal is the common
    /// case, so it needs a value of its own rather than a flag beside one.
    const NOTHING: isize = -1;

    thread_local! {
        /// Which item was picked, written by the callback and read once the
        /// menu has closed.
        ///
        /// A menu is modal: `popUpContextMenu` does not return until the menu
        /// is gone, and AppKit sends the action from inside that call, on this
        /// thread. So the answer can simply be left here and collected after —
        /// no channel, no lock, no lifetime that has to outlive the menu.
        static CHOSEN: Cell<isize> = const { Cell::new(NOTHING) };
    }

    declare_class!(
        /// What the menu items are wired to.
        ///
        /// An `NSMenuItem` sends its action to an Objective-C object, so there
        /// has to be one. It holds nothing: the item's tag says which entry it
        /// was, and that is the whole message.
        struct Picker;

        unsafe impl ClassType for Picker {
            type Super = NSObject;
            type Mutability = mutability::MainThreadOnly;
            const NAME: &'static str = "CianMenuPicker";
        }

        impl DeclaredClass for Picker {}

        unsafe impl NSObjectProtocol for Picker {}

        unsafe impl Picker {
            #[method(cianPick:)]
            fn pick(&self, sender: &NSMenuItem) {
                let tag = unsafe { sender.tag() };
                CHOSEN.with(|c| c.set(tag));
            }
        }
    );

    impl Picker {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = mtm.alloc::<Self>();
            unsafe { msg_send_id![this, init] }
        }
    }

    /// The fixed entries. Anything at or above [`OPEN_WITH`] is the nth
    /// application Launch Services named.
    const OPEN: isize = 0;
    const REVEAL: isize = 1;
    const QUICK_LOOK: isize = 2;
    const COPY_PATH: isize = 3;
    const TRASH: isize = 4;
    const OPEN_WITH: isize = 100;

    fn url(path: &Path) -> Option<Retained<NSURL>> {
        let s = path.to_str()?;
        Some(unsafe { NSURL::fileURLWithPath(&NSString::from_str(s)) })
    }

    /// Add an entry, wired to `picker` and tagged `tag`.
    fn item(
        mtm: MainThreadMarker,
        menu: &NSMenu,
        picker: &Picker,
        title: &str,
        tag: isize,
    ) {
        let it = NSMenuItem::new(mtm);
        unsafe {
            it.setTitle(&NSString::from_str(title));
            it.setTarget(Some(&*(picker as *const Picker as *const AnyObject)));
            it.setAction(Some(sel!(cianPick:)));
            it.setTag(tag);
            menu.addItem(&it);
        }
    }

    pub fn show(window: &winit::window::Window, about: &super::About, _at: (i32, i32)) -> bool {
        // macOS has one menu for a folder, not two: the Finder's "paste item"
        // lives on its own window's background and is not something another
        // application can ask for. So a folder is shown as what it is — one
        // item, with the same actions any other item gets.
        let owned;
        let paths: &[PathBuf] = match about {
            super::About::Items(paths) => paths,
            super::About::Folder(dir) => {
                owned = [dir.clone()];
                &owned
            }
        };
        let Some(mtm) = MainThreadMarker::new() else { return false };
        let Ok(handle) = window.window_handle() else { return false };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return false };
        // SAFETY: winit's own view, alive for as long as the window is, and
        // borrowed only for the length of this call.
        let view: &NSView = unsafe { &*(handle.ns_view.as_ptr() as *const NSView) };

        // The menu is placed by the event that asked for it — the right-mouse
        // press AppKit has just delivered — so there is no arithmetic here to
        // get wrong about which corner a screen counts from.
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        let Some(event) = app.currentEvent() else { return false };

        let urls: Vec<_> = paths.iter().filter_map(|p| url(p)).collect();
        if urls.is_empty() {
            return false;
        }
        let one = urls.len() == 1;

        let picker = Picker::new(mtm);
        let menu = NSMenu::new(mtm);
        item(mtm, &menu, &picker, "Open", OPEN);

        // The applications the system itself would offer. Only for a single
        // file: Launch Services answers about one URL, and merging the answers
        // for a selection would be cian's opinion rather than the system's.
        let apps: Vec<Retained<NSURL>> = if one {
            let list = unsafe { NSWorkspace::sharedWorkspace().URLsForApplicationsToOpenURL(&urls[0]) };
            list.iter().map(|u| u.retain()).collect()
        } else {
            Vec::new()
        };
        if !apps.is_empty() {
            let with = NSMenu::new(mtm);
            for (n, app_url) in apps.iter().enumerate() {
                let name = unsafe { app_url.lastPathComponent() }
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let name = name.strip_suffix(".app").unwrap_or(&name).to_string();
                item(mtm, &with, &picker, &name, OPEN_WITH + n as isize);
            }
            let holder = NSMenuItem::new(mtm);
            unsafe { holder.setTitle(&NSString::from_str("Open With")) };
            holder.setSubmenu(Some(&with));
            menu.addItem(&holder);
        }

        menu.addItem(&NSMenuItem::separatorItem(mtm));
        item(mtm, &menu, &picker, "Show in Finder", REVEAL);
        item(mtm, &menu, &picker, "Quick Look", QUICK_LOOK);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        item(mtm, &menu, &picker, if one { "Copy Path" } else { "Copy Paths" }, COPY_PATH);
        item(mtm, &menu, &picker, "Move to Trash", TRASH);

        CHOSEN.with(|c| c.set(NOTHING));
        // Modal: this returns when the menu is gone, and the pick (if there was
        // one) has already been recorded by then.
        unsafe { NSMenu::popUpContextMenu_withEvent_forView(&menu, &event, view) };
        let chosen = CHOSEN.with(|c| c.replace(NOTHING));
        if chosen == NOTHING {
            // Dismissed. The menu was still shown, which is what the caller
            // asked about — it must not now open cian's own on top.
            return true;
        }
        run(chosen, paths, &urls, &apps);
        true
    }

    fn run(chosen: isize, paths: &[PathBuf], urls: &[Retained<NSURL>], apps: &[Retained<NSURL>]) {
        let ws = unsafe { NSWorkspace::sharedWorkspace() };
        match chosen {
            OPEN => {
                for u in urls {
                    unsafe { ws.openURL(u) };
                }
            }
            REVEAL => {
                let array = NSArray::from_vec(urls.to_vec());
                unsafe { ws.activateFileViewerSelectingURLs(&array) };
            }
            // Quick Look's own panel belongs to a view controller, which cian
            // does not have; `qlmanage -p` is the system's own way in from
            // outside one, and it is what the shortcut in the Finder ends up
            // doing.
            QUICK_LOOK => {
                let _ = Command::new("/usr/bin/qlmanage")
                    .arg("-p")
                    .args(paths)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
            COPY_PATH => {
                let text = paths
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                let pb = unsafe { NSPasteboard::generalPasteboard() };
                unsafe {
                    pb.clearContents();
                    pb.setString_forType(&NSString::from_str(&text), NSPasteboardTypeString);
                }
            }
            // The Trash, not deletion: this is the desktop's menu, and what the
            // desktop's menu does here is reversible.
            TRASH => {
                let fm = unsafe { NSFileManager::defaultManager() };
                for u in urls {
                    let _ = unsafe { fm.trashItemAtURL_resultingItemURL_error(u, None) };
                }
            }
            n if n >= OPEN_WITH => {
                let Some(app) = apps.get((n - OPEN_WITH) as usize) else { return };
                let Some(app) = (unsafe { app.path() }) else { return };
                let _ = Command::new("/usr/bin/open")
                    .arg("-a")
                    .arg(app.to_string())
                    .args(paths)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
            _ => {}
        }
    }

    pub fn to_screen(_window: &winit::window::Window, x: f64, y: f64) -> (i32, i32) {
        // Unused: the menu is placed by the event that opened it.
        (x as i32, y as i32)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    pub fn show(
        _window: &winit::window::Window,
        _about: &super::About,
        _at: (i32, i32),
    ) -> bool {
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
