//! Dragging files out of cian, into the desktop's own file manager.
//!
//! Dropping *into* cian is free: the window system tells winit about a drop and
//! winit passes it on. Dragging *out* is not. It means being a drag source —
//! announcing to the desktop that a gesture has begun and that these files are
//! what it carries — and neither platform lets a program do that by accident.
//!
//! There is nothing to draw here. Once the session has begun the desktop owns
//! the gesture and draws the ghost itself, in its own house style, with its own
//! snap-back animation when a drop is refused. cian's own ghost is for drags
//! that stay inside the window; this is for the ones that leave.

use std::path::Path;

/// Begin dragging these files out of the window.
///
/// Returns whether a session actually started. `false` means the platform
/// declined or is not implemented, and the caller should keep treating the
/// gesture as an ordinary in-window drag.
pub fn begin(window: &winit::window::Window, paths: &[std::path::PathBuf]) -> bool {
    if paths.is_empty() {
        return false;
    }
    platform::begin(window, paths)
}

#[cfg(target_os = "macos")]
mod platform {
    use std::path::PathBuf;

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
    use objc2_app_kit::{
        NSApplication, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession,
        NSDraggingSource, NSPasteboardWriting, NSView,
    };
    use objc2_foundation::{
        MainThreadMarker, NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSURL,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    declare_class!(
        /// The thing the desktop asks "what may be done with what you are
        /// dragging?".
        ///
        /// A drag source has to be an Objective-C object because AppKit calls
        /// back into it, so there is a class here whether or not it has any
        /// state to keep. It has none: the answer is the same every time.
        struct Source;

        unsafe impl ClassType for Source {
            type Super = NSObject;
            // Main-thread-only, because `NSDraggingSource` is: AppKit calls
            // back on the thread that started the drag and objc2 insists the
            // type say so.
            type Mutability = mutability::MainThreadOnly;
            const NAME: &'static str = "CianDragSource";
        }

        impl DeclaredClass for Source {}

        unsafe impl NSObjectProtocol for Source {}

        unsafe impl NSDraggingSource for Source {
            #[method(draggingSession:sourceOperationMaskForDraggingContext:)]
            unsafe fn source_operation(
                &self,
                _session: &NSDraggingSession,
                context: NSDraggingContext,
            ) -> NSDragOperation {
                // Outside the window: move, which is what dragging a file to
                // another folder means. Inside it: nothing, because cian
                // answers its own drags itself and a doubled gesture would run
                // the move twice.
                match context {
                    NSDraggingContext::OutsideApplication => NSDragOperation::Move,
                    _ => NSDragOperation::None,
                }
            }
        }
    );

    impl Source {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = mtm.alloc::<Self>();
            unsafe { msg_send_id![this, init] }
        }
    }

    pub fn begin(window: &winit::window::Window, paths: &[PathBuf]) -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };

        // The view winit drew into. Everything below hangs off it: a drag
        // belongs to a view, not to a window.
        let Ok(handle) = window.window_handle() else { return false };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return false };
        let view: &NSView = unsafe { &*(handle.ns_view.as_ptr() as *const NSView) };

        // A drag has to be attached to the event that started it — the press.
        // Without one AppKit has no idea where the gesture came from and
        // refuses to begin.
        let app = NSApplication::sharedApplication(mtm);
        let Some(event) = app.currentEvent() else { return false };

        let mut items = Vec::new();
        for (n, path) in paths.iter().enumerate() {
            let Some(s) = path.to_str() else { continue };
            let url = unsafe { NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(s)) };
            let writer: &ProtocolObject<dyn NSPasteboardWriting> =
                ProtocolObject::from_ref(&*url);
            let item =
                unsafe { NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), writer) };
            // Where the ghost starts from. Spread a little so several files
            // read as a stack rather than as one; the desktop takes over the
            // drawing from here.
            let at = unsafe { event.locationInWindow() };
            let offset = (n as f64) * 4.0;
            unsafe {
                item.setDraggingFrame_contents(
                    NSRect::new(
                        NSPoint::new(at.x - 16.0 + offset, at.y - 16.0 - offset),
                        NSSize::new(32.0, 32.0),
                    ),
                    None,
                );
            }
            items.push(item);
        }
        if items.is_empty() {
            return false;
        }

        let source = Source::new(mtm);
        let source: &ProtocolObject<dyn NSDraggingSource> = ProtocolObject::from_ref(&*source);
        let array = NSArray::from_vec(items);
        unsafe { view.beginDraggingSessionWithItems_event_source(&array, &event, source) };
        true
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{IBindCtx, IDataObject};
    use windows::Win32::System::Ole::{
        IDropSource, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE,
    };
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        ILCreateFromPathW, ILFree, SHCreateShellItemArrayFromIDLists, SHDoDragDrop, BHID_DataObject,
    };

    /// Nothing here implements a COM interface, and that is the point.
    ///
    /// The textbook way to be a drag source on Windows is to write an
    /// `IDataObject` (nine methods), an `IEnumFORMATETC` (four) and an
    /// `IDropSource` (two), hand-rolling the vtables and the reference counts —
    /// several hundred lines of unsafe code that cannot be compiled on the
    /// machine cian is written on, let alone run there. A mistake in one of
    /// those tables is not a wrong answer, it is a crash on someone else's
    /// desktop.
    ///
    /// The shell will make all three: an item array knows how to hand over a
    /// data object for what it holds, and `SHDoDragDrop` supplies the default
    /// drop source — the one every Explorer window uses, with the right cursors
    /// and the right drag image — when it is given none. What is left is
    /// turning paths into the ids the shell speaks in.
    pub fn begin(window: &winit::window::Window, paths: &[PathBuf]) -> bool {
        let Some(hwnd) = hwnd(window) else { return false };

        // An id list per file. These are absolute, so the array may hold files
        // from different folders — which is what a marked selection is.
        let mut ids: Vec<*const ITEMIDLIST> = Vec::with_capacity(paths.len());
        for p in paths {
            let wide: Vec<u16> =
                p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
            // SAFETY: a NUL-terminated wide string that outlives the call.
            let id = unsafe { ILCreateFromPathW(windows::core::PCWSTR(wide.as_ptr())) };
            if id.is_null() {
                // A path the shell does not recognise: let go of what has been
                // built and leave the gesture to cian's own drag.
                for id in ids {
                    unsafe { ILFree(Some(id)) };
                }
                return false;
            }
            ids.push(id);
        }

        let started = drag(hwnd, &ids);

        // The ids belong to this function whatever happened.
        for id in ids {
            // SAFETY: each was handed back by `ILCreateFromPathW` and has not
            // been freed; the shell copied what it needed.
            unsafe { ILFree(Some(id)) };
        }
        started
    }

    /// The drag itself. Blocks until the gesture ends, as every drag source on
    /// Windows does — the desktop owns the mouse until the button comes up.
    fn drag(hwnd: HWND, ids: &[*const ITEMIDLIST]) -> bool {
        // SAFETY: `ids` are live id lists owned by the caller for the length of
        // this call, and the interfaces are dropped before it returns.
        unsafe {
            let Ok(items) = SHCreateShellItemArrayFromIDLists(ids) else {
                return false;
            };
            let Ok(data) = items.BindToHandler::<_, IDataObject>(None::<&IBindCtx>, &BHID_DataObject)
            else {
                return false;
            };
            // What cian is willing to let happen. The desktop decides which of
            // them it is, from where the file lands and which keys are held —
            // exactly as it does for a drag out of Explorer.
            let allowed = DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK;
            SHDoDragDrop(Some(hwnd), &data, None::<&IDropSource>, allowed).is_ok()
        }
    }

    /// The window behind winit's, which is what a drag has to be started from.
    fn hwnd(window: &winit::window::Window) -> Option<HWND> {
        let handle = window.window_handle().ok()?;
        match handle.as_ref() {
            RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut core::ffi::c_void)),
            _ => None,
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod platform {
    use std::path::PathBuf;

    pub fn begin(_window: &winit::window::Window, _paths: &[PathBuf]) -> bool {
        false
    }
}

/// Whether a path is worth offering to the desktop at all.
///
/// A file on a server or inside an archive has a path this machine cannot open,
/// and handing one over would drop a broken reference into someone's folder.
pub fn draggable(path: &Path) -> bool {
    path.exists()
}
