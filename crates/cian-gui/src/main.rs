//! cian with a window of its own.
//!
//! The same cian — the same panes, the same keys, the same Lua — with the
//! terminal taken out from under it. What changes is only who owns the loop:
//! the terminal build blocks on `event::poll` and paints when it wakes, while
//! here winit does the waking and cian is asked what it wants between times.
//! Both drive the same [`cian_tui::Session`].
//!
//! Three things a terminal could not give it:
//!
//! * **Every key, distinguishable.** Ctrl+H is not Backspace here, and Command
//!   arrives instead of being swallowed by the OS on its way to a terminal.
//! * **The font is cian's own.** `Ctrl` `+`/`-` resizes it directly rather than
//!   asking init.lua for a command that only one emulator understands.
//! * **Japanese input.** The IME talks to the window, so 未確定文字 exist at all.
//!
//! What is not here yet, and is known to be missing:
//!
//! * `E` (external editor) has no terminal to hand to vim, and says so rather
//!   than hanging. It wants running inside the shell pane instead.
//! * 未確定文字 land on the status line rather than under the caret, because the
//!   panes have no notion of a preedit yet.

// A windowed program, and Windows should be told so. Left unsaid, the linker
// builds a console application: double-clicking cian opened a black console
// window *and* the cian window, and closing the black one killed the program.
// The console is not a fallback either — nothing is ever printed to it.
//
// What is lost is `cian --version` from a terminal, because a windowed program
// starts with no streams at all. `attach_console` below takes the terminal's
// own back, so the answer lands where it was asked for.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod appkit;
mod dragout;
mod font;
mod glyph;
mod iconjob;
mod input;
mod picture;
mod pixels;
mod shellmenu;
mod soft;
mod sysicon;

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Instant;

use cian_tui::crossterm::event::Event;
use cian_tui::ratatui::Terminal;
use cian_tui::{Session, Skin, StartupMacro};
use pixels::PixelLayer;
use ratatui_wgpu::{Builder, Dimensions, Font, WgpuBackend};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState};
use winit::window::{Window, WindowAttributes, WindowId};

/// Font size in pixels, and how far it may be pushed.
const SIZE_DEFAULT: u32 = 22;
const SIZE_MIN: u32 = 8;
const SIZE_MAX: u32 = 72;

/// Recorded against a path the system had no icon for, so it is asked once
/// and never again.
const NO_ICON: u64 = 0;

/// How close together two clicks have to be to count as one double click.
/// macOS's own default is half a second; matching it means the grid feels the
/// same as everything else on the desktop.
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(500);

/// The three ways the panes can look. `Ctrl+Shift+G` walks round them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    /// Bordered panes, dense rows, Nerd Font glyphs — cian as it has always
    /// looked.
    Classic,
    /// Borderless, banded, light, with the system's own icons.
    Finder,
    /// The left pane alone, as a grid of pictures.
    Icons,
}

/// The loop's heartbeat, sent from a thread of its own.
///
/// `ControlFlow::WaitUntil` was the obvious way to ask for one and it does not
/// work here: measured on this Mac it woke the loop about once every one and a
/// half seconds instead of thirty times a second, which starved everything that
/// happens between keystrokes — shell output, transitions, spinners, the icons
/// filling in. A thread that sends an event is the one thing every backend has
/// to answer, because an event is what an event loop is for.
#[derive(Debug, Clone, Copy)]
struct Tick;

/// How often that thread pokes it. Matches the terminal build's own poll, so
/// both front ends give cian a turn at the same rate.
const HEARTBEAT: std::time::Duration = std::time::Duration::from_millis(16);

/// Paint the title bar itself, where the desktop allows it.
///
/// `set_theme` above gets it to dark or light, which is most of the way. This
/// is the rest: Windows 11 will take an exact colour for the caption, so the
/// bar matches the theme rather than merely agreeing with it about the time of
/// day. Anything older ignores it, which is why the result is not checked.
#[cfg(windows)]
fn caption_colour(window: &Window, rgb: Option<(u8, u8, u8)>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Some((r, g, b)) = rgb else { return };
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::Win32(h) = handle.as_ref() else { return };
    let hwnd = windows::Win32::Foundation::HWND(h.hwnd.get() as *mut core::ffi::c_void);
    // COLORREF is 0x00BBGGRR — the other way round from everything else here.
    let colour: u32 = (b as u32) << 16 | (g as u32) << 8 | r as u32;
    // SAFETY: a live window handle, and a four-byte value whose size is passed
    // with it. The call is documented to fail harmlessly on older Windows.
    unsafe {
        let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd,
            windows::Win32::Graphics::Dwm::DWMWA_CAPTION_COLOR,
            &colour as *const u32 as *const core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );
    }
}

#[cfg(not(windows))]
fn caption_colour(_window: &Window, _rgb: Option<(u8, u8, u8)>) {}

/// Should cian draw the pixels itself?
///
/// `CIAN_GUI_RENDER=cpu|gpu` decides it outright. Left alone, the machine
/// decides: if every adapter that answers is a *software* one, then wgpu would
/// be a software rasteriser pretending to be a graphics card, and drawing the
/// cells directly is both simpler and — measured on the machine that reported
/// this — two orders of magnitude quicker.
fn want_cpu_renderer() -> bool {
    match std::env::var("CIAN_GUI_RENDER").ok().as_deref() {
        Some("cpu") => return true,
        Some("gpu") => return false,
        _ => {}
    }
    use ratatui_wgpu::wgpu::{Backends, DeviceType, Instance, InstanceDescriptor};
    let instance =
        Instance::new(&InstanceDescriptor { backends: Backends::default(), ..Default::default() });
    let adapters = pollster::block_on(instance.enumerate_adapters(Backends::default()));
    // Nothing at all answered: there is no wgpu path to take, so take the other
    // one rather than failing to open a window.
    if adapters.is_empty() {
        return true;
    }
    adapters
        .iter()
        .all(|a| matches!(a.get_info().device_type, DeviceType::Cpu | DeviceType::Other))
}

/// Which graphics API to draw through, and what is available.
///
/// The default was "whatever wgpu picks", and on the machine that reported the
/// window as unusable it picked something that takes a hundred and thirty
/// milliseconds to put one frame on screen — against one millisecond for cian
/// to compose it. Seven frames a second, and not one of them cian's fault.
///
/// A locked-down Windows machine can have no usable hardware driver at all, in
/// which case the choice is a software rasteriser and the only question is
/// which one. So: every adapter is written to the log with what it is, and
/// `CIAN_GUI_BACKEND=dx12|vulkan|gl` picks between them when the automatic
/// choice is a bad one. Naming what is wrong is most of fixing it.
fn gpu_instance() -> ratatui_wgpu::wgpu::Instance {
    use ratatui_wgpu::wgpu::{Backends, Instance, InstanceDescriptor};
    let backends = match std::env::var("CIAN_GUI_BACKEND").ok().as_deref() {
        Some("dx12") => Backends::DX12,
        Some("vulkan") => Backends::VULKAN,
        Some("gl") => Backends::GL,
        Some("metal") => Backends::METAL,
        _ => Backends::default(),
    };
    let instance = Instance::new(&InstanceDescriptor { backends, ..Default::default() });
    // Said out loud, always, and to both places. This is the one fact that
    // decides whether the window is slow because of something cian does or
    // because there is no graphics driver under it, and it must not depend on
    // anyone having remembered to turn logging on first.
    let mut said = 0;
    for adapter in pollster::block_on(instance.enumerate_adapters(backends)) {
        let info = adapter.get_info();
        let line = format!(
            "gpu: {:?} {} ({:?}, driver {} {})",
            info.backend, info.name, info.device_type, info.driver, info.driver_info,
        );
        eprintln!("{line}");
        cian_core::log::log(&line);
        said += 1;
    }
    if said == 0 {
        let line = format!("gpu: nothing at all answered for {backends:?}");
        eprintln!("{line}");
        cian_core::log::log(&line);
    }
    // And which of them would be chosen, asked the same way the renderer asks.
    if let Ok(a) = pollster::block_on(instance.request_adapter(&Default::default())) {
        let info = a.get_info();
        let line = format!(
            "gpu: chosen — {:?} {} ({:?})",
            info.backend, info.name, info.device_type,
        );
        eprintln!("{line}");
        cian_core::log::log(&line);
    }
    instance
}

/// How finished frames reach the screen.
///
/// `CIAN_GUI_PRESENT=vsync|immediate|mailbox`. The renderer's own default is
/// immediate; on a machine where presenting is the slow part, which one is
/// being used is worth being able to change without a new build.
fn present_mode() -> ratatui_wgpu::wgpu::PresentMode {
    use ratatui_wgpu::wgpu::PresentMode;
    match std::env::var("CIAN_GUI_PRESENT").ok().as_deref() {
        Some("vsync") => PresentMode::AutoVsync,
        Some("mailbox") => PresentMode::Mailbox,
        Some("immediate") => PresentMode::Immediate,
        _ => PresentMode::AutoNoVsync,
    }
}

/// A duration in microseconds, written in ASCII.
///
/// Everything cian prints for a person to read may end up in a Windows
/// console, which is not UTF-8 — `µs` arrives there as `ﾂｵs`.
fn us(d: std::time::Duration) -> String {
    format!("{}us", d.as_micros())
}

/// How many frames `CIAN_GUI_PROF` averages over before it says anything.
const PROF_EVERY: u32 = 120;

/// The two ways cian can put a frame on screen.
///
/// wgpu where there is a driver, and pixels drawn by hand where there is not.
/// The loop asks the same questions of either; the difference lives here and
/// nowhere else. See [`soft`] for why the second one exists.
#[allow(clippy::large_enum_variant)] // One of these exists, for the life of the window.
enum Screen {
    Gpu(Terminal<WgpuBackend<'static, 'static, PixelLayer>>),
    Cpu(Terminal<soft::SoftBackend>),
}

impl Screen {
    fn size(&self) -> Option<cian_tui::ratatui::layout::Size> {
        match self {
            Screen::Gpu(t) => t.size().ok(),
            Screen::Cpu(t) => t.size().ok(),
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        match self {
            Screen::Gpu(t) => t.backend_mut().resize(w, h),
            Screen::Cpu(t) => t.backend_mut().resize(w, h),
        }
    }

    fn clear(&mut self) {
        match self {
            Screen::Gpu(t) => {
                let _ = t.clear();
            }
            Screen::Cpu(t) => {
                let _ = t.clear();
            }
        }
    }

    /// Draw a frame, and say how long the *composing* half of it took — the
    /// part that is cian's rather than the renderer's.
    fn draw(&mut self, cian: &mut Session) -> Result<std::time::Duration, String> {
        let mut build = std::time::Duration::ZERO;
        let out = match self {
            Screen::Gpu(t) => t
                .draw(|f| {
                    let b0 = Instant::now();
                    cian.draw(f);
                    build = b0.elapsed();
                })
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Screen::Cpu(t) => t
                .draw(|f| {
                    let b0 = Instant::now();
                    cian.draw(f);
                    build = b0.elapsed();
                })
                .map(|_| ())
                .map_err(|e| e.to_string()),
        };
        out.map(|()| build)
    }

    fn upload(&mut self, id: u64, w: u32, h: u32, rgba: Vec<u8>) {
        match self {
            Screen::Gpu(t) => t.backend_mut().post_processor_mut().upload(id, w, h, rgba),
            Screen::Cpu(t) => t.backend_mut().upload(id, w, h, rgba),
        }
    }

    fn evict(&mut self, id: u64) {
        match self {
            Screen::Gpu(t) => t.backend_mut().post_processor_mut().evict(id),
            Screen::Cpu(t) => t.backend_mut().evict(id),
        }
    }

    fn set_frame(&mut self, draws: Vec<pixels::Draw>) {
        match self {
            Screen::Gpu(t) => t.backend_mut().post_processor_mut().set_frame(draws),
            Screen::Cpu(t) => t.backend_mut().set_frame(
                draws
                    .into_iter()
                    .map(|d| soft::Draw {
                        id: d.id,
                        x: d.x,
                        y: d.y,
                        w: d.w,
                        h: d.h,
                        alpha: d.alpha,
                    })
                    .collect(),
            ),
        }
    }

    /// Which one this is, for the log and for `:version`.
    fn name(&self) -> &'static str {
        match self {
            Screen::Gpu(_) => "wgpu",
            Screen::Cpu(_) => "cpu",
        }
    }
}

struct Gui {
    cian: Session,
    window: Option<Arc<Window>>,
    terminal: Option<Screen>,
    face: Font<'static>,
    /// The same font, parsed for drawing single glyphs as pictures.
    glyphs: Option<ab_glyph::FontRef<'static>>,
    font_name: String,
    size_px: u32,

    mods: ModifiersState,
    /// Which button is down, so a move can be reported as a drag.
    held: Option<MouseButton>,
    /// Where the pointer is, in cells.
    at: input::CellPos,
    /// Sub-notch scroll left over from a trackpad, kept so a slow drag still
    /// eventually scrolls instead of rounding to nothing every frame.
    scroll_rest: (f64, f64),

    needs_redraw: bool,
    /// The earliest the next background turn may happen.
    ///
    /// This is the whole throttle. The terminal build gets one for free — its
    /// `event::poll(33ms)` blocks for that long every turn, so the loop can
    /// never run faster than the timeout no matter how much wants redrawing.
    /// Nothing here blocks, so without a deadline of its own the loop draws
    /// flat out: cian asks for a frame, winit serves it immediately, cian asks
    /// again. That is a busy loop wearing a frame rate as a disguise.
    next_tick: Instant,
    /// The title last given to the window, so it is only set when it changes.
    title: String,
    /// Whether the desktop was last told the theme is a light one. `None`
    /// until it has been told anything.
    told_light: Option<bool>,
    /// Print every key to stderr — see `CIAN_GUI_KEYLOG`.
    keylog: bool,
    /// Which texture holds which icon, under the key it is shared by. Never
    /// emptied: an icon costs a few kilobytes and the same kinds of file are
    /// looked at again and again.
    icon_ids: std::collections::HashMap<IconKey, u64>,
    /// Textures for cian's own glyphs, kept apart from the system's icons:
    /// a row can want one because the view asked for it, or because the system
    /// had nothing, and those are different answers to the same key.
    glyph_ids: std::collections::HashMap<IconKey, u64>,
    /// The thread that asks the system about icons. See [`iconjob`].
    icons: iconjob::Icons,
    next_icon_id: u64,
    /// The decoded size of each picture, which is not the size of the box it
    /// was asked to fit: it keeps its shape, so one of the two dimensions comes
    /// back short and the picture is centred in what is left.
    picture_size: std::collections::HashMap<IconKey, (f32, f32)>,
    /// The picture textures now on the layer — at most one, and dropped as soon
    /// as another arrives. A window-sized photograph is megabytes of texture,
    /// and unlike an icon it will not be wanted again.
    picture_ids: Vec<(IconKey, u64)>,
    /// How many pictures the layer was last told to draw, so a change can
    /// be noticed and repainted. See `place_icons`.
    last_draws: usize,
    /// Set when the view changes, to force one more frame even if the count
    /// happens to come out the same on both sides of the switch.
    pending_view_change: bool,
    /// When and where the last left press landed, for spotting a double.
    last_click: Option<(Instant, (u16, u16))>,
    /// Sub-step wheel left over while zooming, so a trackpad still adds up.
    zoom_rest: f64,
    /// A font size asked for but not yet built. See `apply_font_size`.
    want_size: Option<u32>,
    /// When the last frame was painted. Kept for the profiler and for anyone
    /// asking how long a frame took.
    last_frame: Instant,
    /// `CIAN_GUI_PROF=1`: report what a frame costs, to stderr and the log.
    ///
    /// Here rather than in a profiler because the machine that feels slow is
    /// not this one — it is a Windows machine on the other side of a report,
    /// and "it is sluggish" and "a frame takes 9ms, 7 of them in cian" are
    /// different conversations.
    prof: bool,
    prof_total: std::time::Duration,
    prof_build: std::time::Duration,
    prof_icons: std::time::Duration,
    prof_frames: u32,
    /// The worst single frame since the last report, and its parts.
    prof_worst: std::time::Duration,
    prof_worst_build: std::time::Duration,
    /// How many events were handled between one frame and the next.
    ///
    /// This is the number that says whether cian is *behind*. One or two is a
    /// program keeping up with a person; thirty is a keyboard whose repeats
    /// have been queuing while cian painted, which is what "it keeps moving
    /// after I let go" is made of.
    prof_events: u32,
    prof_worst_events: u32,
    /// Files dropped on the window since the last turn of the loop. See
    /// `WindowEvent::DroppedFile`.
    dropped: Vec<std::path::PathBuf>,
    /// A drag in progress: what was picked up, and where it started.
    ///
    /// Held from the press, but not a drag until the pointer has moved a cell
    /// away — otherwise every click would be a one-pixel drag and the ghost
    /// would flicker on each one.
    drag: Option<Drag>,
}

/// What an icon is looked up by.
///
/// Almost every file's icon is its *kind's* icon: a folder of two thousand
/// `.log` files has one icon in it, not two thousand. Asking the system per
/// path meant two thousand shell calls and two thousand textures for one
/// picture — invisible on a Mac, and on Windows a per-file trip through the
/// shell (and whatever a virus scanner has hooked into it) on the thread that
/// draws.
///
/// The exceptions are the files that really do carry their own picture:
/// directories (a folder can be given one), programs and shortcuts, and the
/// icon formats themselves.
#[derive(Clone, PartialEq, Eq, Hash)]
enum IconKey {
    /// This file, and only this file.
    Path(std::path::PathBuf),
    /// Everything with this extension, lowercased. Empty for "no extension".
    Kind(String),
    /// The picture in this file, decoded for a box this many pixels across and
    /// down. The size is part of the key: resize the window and it is a
    /// different picture, decoded again rather than stretched.
    Picture(std::path::PathBuf, u32, u32),
}

/// Extensions whose icon belongs to the file rather than to its kind.
const OWN_ICON: &[&str] =
    &["exe", "lnk", "app", "ico", "icns", "url", "msi", "scr", "cpl", "cur", "ani"];

impl IconKey {
    fn of(slot: &cian_tui::IconSlot) -> Self {
        let ext = slot
            .path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if slot.is_dir || OWN_ICON.contains(&ext.as_str()) {
            Self::Path(slot.path.clone())
        } else {
            Self::Kind(ext)
        }
    }
}

/// Files being dragged with the mouse.
struct Drag {
    paths: Vec<std::path::PathBuf>,
    from: (u16, u16),
    /// False until the pointer leaves the cell it was pressed in.
    moved: bool,
    /// True once the desktop has taken the gesture over. cian stops drawing its
    /// own ghost then, and stops expecting a drop of its own.
    handed_over: bool,
}

impl Gui {
    fn new(
        cian: Session,
        face: Font<'static>,
        bytes: &'static [u8],
        font_name: String,
    ) -> Self {
        Self {
            cian,
            glyphs: ab_glyph::FontRef::try_from_slice(bytes).ok(),
            window: None,
            terminal: None,
            face,
            font_name,
            size_px: SIZE_DEFAULT,
            mods: ModifiersState::empty(),
            held: None,
            at: input::CellPos::default(),
            scroll_rest: (0.0, 0.0),
            needs_redraw: true,
            next_tick: Instant::now(),
            title: String::new(),
            told_light: None,
            keylog: std::env::var_os("CIAN_GUI_KEYLOG").is_some(),
            icon_ids: std::collections::HashMap::new(),
            glyph_ids: std::collections::HashMap::new(),
            icons: iconjob::Icons::start(),
            // Ids start above the smoke test's, which uses 1.
            next_icon_id: 100,
            picture_size: std::collections::HashMap::new(),
            picture_ids: Vec::new(),
            last_draws: 0,
            pending_view_change: false,
            last_click: None,
            zoom_rest: 0.0,
            want_size: None,
            last_frame: Instant::now(),
            prof: {
                let on = std::env::var_os("CIAN_GUI_PROF").is_some();
                if on {
                    // Time the frame in parts too: the total alone cannot say
                    // which machine's frame is the slow one, and this one is
                    // not the machine cian is developed on.
                    cian_tui::prof::enable();
                }
                on
            },
            prof_total: std::time::Duration::ZERO,
            prof_build: std::time::Duration::ZERO,
            prof_icons: std::time::Duration::ZERO,
            prof_frames: 0,
            prof_worst: std::time::Duration::ZERO,
            prof_worst_build: std::time::Duration::ZERO,
            prof_events: 0,
            prof_worst_events: 0,
            drag: None,
            dropped: Vec::new(),
        }
    }

    /// Is the pointer past the edge of the window?
    ///
    /// Only true once it has properly left — a drag that grazes the border on
    /// its way across the window should not be handed to the desktop.
    fn pointer_outside(&self) -> bool {
        let (Some(t), Some(_)) = (self.terminal.as_ref(), self.window.as_ref()) else {
            return false;
        };
        let Some(grid) = t.size() else { return false };
        self.at.column == 0
            || self.at.row == 0
            || self.at.column + 1 >= grid.width
            || self.at.row + 1 >= grid.height
    }

    /// Ask for a frame now rather than at the next tick.
    ///
    /// Input takes this path: a keystroke should land on screen immediately,
    /// not up to a tick later. Everything else waits its turn.
    fn redraw_now(&mut self) {
        // Only says that a frame is wanted. Asking for it happens once the
        // events waiting behind this one have all been handled — see
        // `about_to_wait`, which is where a window gets what the terminal
        // build has for free.
        self.needs_redraw = true;
    }

    /// One character cell, in physical pixels. Derived from the surface rather
    /// than asked of the renderer: the grid and the window agree on it by
    /// construction, so dividing one by the other is exact.
    fn cell_size(&self) -> (u32, u32) {
        let (Some(w), Some(t)) = (self.window.as_ref(), self.terminal.as_ref()) else {
            return (0, 0);
        };
        let px = w.inner_size();
        let Some(grid) = t.size() else { return (0, 0) };
        if grid.width == 0 || grid.height == 0 {
            return (0, 0);
        }
        (px.width / grid.width as u32, px.height / grid.height as u32)
    }

    /// Change the font size, by rebuilding the renderer around the new one.
    ///
    /// `update_fonts` exists and is the obvious call, and it does not work here.
    /// A different size means a different number of cells across, and the
    /// backend grows its cell buffer with `Vec::resize` — which keeps what was
    /// already in it, at the linear positions the *old* width gave them. Every
    /// row after the first is then offset by the difference, and the screen
    /// shears. Clearing first and nudging the surface size got it right
    /// sometimes, which is the worst kind of right.
    ///
    /// So the whole thing is built again. It is the same path as startup, which
    /// is known to work, and the cost — a new surface and a lost icon cache —
    /// is paid only when someone asks for a different size.
    fn step_font(&mut self, by: i32) {
        let from = self.want_size.unwrap_or(self.size_px);
        let want = (from as i32 + by).clamp(SIZE_MIN as i32, SIZE_MAX as i32) as u32;
        if want == from {
            return;
        }
        // Only the number changes here. The renderer is rebuilt once, on the
        // next frame — see `apply_font_size`.
        self.want_size = Some(want);
        self.cian.show_message(&format!("文字サイズ {want}px"));
        self.redraw_now();
    }

    /// Rebuild the renderer around a newly asked-for font size, at most once
    /// per frame.
    ///
    /// Both halves of that matter, and both were crashes waiting to happen.
    ///
    /// **Once per frame.** A wheel notch with Ctrl held used to rebuild the
    /// whole renderer inside the wheel event — and one flick of a trackpad is
    /// not one notch, it is dozens. Dozens of devices, queues and surfaces
    /// created in a single event, each one costing a tenth of a second.
    ///
    /// **The old one goes first.** The new renderer was built while the old was
    /// still held, which means two wgpu surfaces existing at once for one
    /// window. Metal tolerates it; DX12 has one swap chain per window and does
    /// not, which is why resizing the font killed the Windows build and not
    /// this one.
    fn apply_font_size(&mut self) {
        let Some(want) = self.want_size.take() else { return };
        if want == self.size_px {
            return;
        }
        let Some(window) = self.window.clone() else { return };
        let was = self.size_px;

        // Dropped, not replaced: the surface it owns has to be gone before
        // another can be made for the same window.
        self.terminal = None;
        // Nothing may claim to have uploaded a texture into a renderer that no
        // longer exists — every icon would be a draw call against nothing.
        self.icon_ids.clear();
        self.glyph_ids.clear();
        self.next_icon_id = 100;
        self.last_draws = 0;

        match Self::build_terminal(&window, &self.face, self.glyphs.as_ref(), want) {
            Some(t) => {
                self.size_px = want;
                self.terminal = Some(t);
            }
            None => {
                // Put back the size that was working. If even that cannot be
                // built the window has no renderer at all, and saying so beats
                // a window that stays black for the rest of the session.
                self.terminal = Self::build_terminal(&window, &self.face, self.glyphs.as_ref(), was);
                if self.terminal.is_none() {
                    cian_core::log::log("cian-gui: lost the renderer while resizing the font");
                }
                self.cian.show_message("文字サイズを変更できませんでした");
            }
        }
        self.needs_redraw = true;
    }

    /// Build a renderer for this window at this size. Shared by startup and by
    /// every font change, so the two can never drift apart.
    fn build_terminal(
        window: &Arc<Window>,
        face: &Font<'static>,
        glyphs: Option<&ab_glyph::FontRef<'static>>,
        size_px: u32,
    ) -> Option<Screen> {
        // Pixels drawn by hand where there is no driver to draw them.
        //
        // A software rasteriser pretending to be a graphics card is the slowest
        // way to put text on a screen: a hundred and thirty milliseconds a
        // frame, measured, against one for cian to compose it. Drawing the
        // cells directly is what a terminal does on the same machine, and that
        // machine's terminal is fast. See [`soft`].
        if want_cpu_renderer() {
            cian_core::log::log("renderer: drawing on the cpu (no graphics driver worth using)");
            if let Some(parsed) = glyphs.cloned() {
                if let Some(b) = soft::SoftBackend::new(window.clone(), parsed, size_px) {
                    if let Ok(t) = Terminal::new(b) {
                        return Some(Screen::Cpu(t));
                    }
                }
            }
            cian_core::log::log("renderer: the cpu path would not start; falling back to wgpu");
        }
        let size = window.inner_size();
        let backend = pollster::block_on(
            Builder::<PixelLayer>::from_font(face.clone())
                .with_font_size_px(size_px)
                .with_instance(gpu_instance())
                .with_present_mode(present_mode())
                .with_width_and_height(Dimensions {
                    width: NonZeroU32::new(size.width.max(1)).unwrap(),
                    height: NonZeroU32::new(size.height.max(1)).unwrap(),
                })
                .build_with_target(window.clone()),
        )
        .ok()?;
        Terminal::new(backend).ok().map(Screen::Gpu)
    }

    /// Which of the three views is showing.
    fn view(&self) -> View {
        if self.cian.icon_view() {
            View::Icons
        } else if self.cian.skin() == Skin::Finder {
            View::Finder
        } else {
            View::Classic
        }
    }

    /// Show a view. The grid brings the desktop palette with it: a grid of
    /// pictures on the bordered dark look is neither one thing nor the other.
    fn set_view(&mut self, view: View) {
        self.cian.set_icon_view(view == View::Icons);
        self.cian.set_skin(match view {
            View::Classic => Skin::Classic,
            View::Finder | View::Icons => Skin::Finder,
        });

        self.pending_view_change = true;
        // Empty the picture list here, not on the next turn. The list is always
        // one frame behind the text, so the first frame of the new view would
        // otherwise carry the old view's pictures — twenty full-size icons over
        // a file listing, with nothing scheduled to take them away again.
        if let Some(t) = self.terminal.as_mut() {
            t.set_frame(Vec::new());
        }
        self.last_draws = 0;
        // What a path's picture *is* changes with the view — the classic list
        // draws cian's own glyph where the detail view asks the system — so a
        // cache keyed on the path alone is stale the moment the view changes.
        // Left in place, switching to the detail view kept every glyph.
        self.icon_ids.clear();
        self.glyph_ids.clear();
        self.cian.show_message(match view {
            View::Finder => "表示: 詳細（Ctrl+Shift+G でアイコン）",
            View::Icons => "表示: アイコン（Ctrl+Shift+G でクラシック）",
            View::Classic => "表示: クラシック（Ctrl+Shift+G で詳細）",
        });
        self.redraw_now();
    }

    /// `Ctrl+Shift+G` — round the three views: details, icons, classic.
    fn toggle_icons(&mut self) {
        let next = match self.view() {
            View::Finder => View::Icons,
            View::Icons => View::Classic,
            View::Classic => View::Finder,
        };
        self.set_view(next);
    }

    /// Keys the window answers itself, before cian sees them.
    ///
    /// Both of these are things a terminal build cannot do at all — resize a
    /// font it does not own, and offer a look that only exists in a window — so
    /// there is nothing in cian's tables for them to collide with.
    ///
    /// It reads the key through the same unfolding as everything else. Asking
    /// `logical_key` directly is the mistake this is a note about: with Ctrl
    /// held, macOS folds the letter into a control character first, so
    /// Ctrl+Shift+G arrives as `U+0007` and never equals `"g"`.
    fn intercept(&mut self, ev: &winit::event::KeyEvent) -> bool {
        // F11 fills the screen, which is what F11 does everywhere. A terminal
        // build cannot answer it — the window belongs to the emulator — so
        // there is nothing in cian's tables for it to collide with.
        if matches!(ev.logical_key, Key::Named(winit::keyboard::NamedKey::F11)) {
            if let Some(w) = self.window.as_ref() {
                // Maximised rather than borderless-fullscreen: this is a file
                // manager, and the title bar and the taskbar are still wanted.
                w.set_maximized(!w.is_maximized());
            }
            return true;
        }
        if !self.mods.control_key() {
            return false;
        }
        let logical = input::base_key(ev, self.mods);

        // Size first, and without asking about Shift. On most layouts `+` *is*
        // Shift and `=`, so testing the shift state before looking at the key
        // meant `Ctrl` `+` — the obvious way to ask for bigger — fell into the
        // branch below and did nothing at all.
        match &logical {
            Key::Character(c) if matches!(c.as_str(), "+" | "=" | ";" | "^") => {
                self.step_font(2);
                return true;
            }
            Key::Character(c) if matches!(c.as_str(), "-" | "_") => {
                self.step_font(-2);
                return true;
            }
            Key::Character(c) if c.as_str() == "0" => {
                let back = SIZE_DEFAULT as i32 - self.size_px as i32;
                self.step_font(back);
                return true;
            }
            _ => {}
        }

        if self.mods.shift_key() {
            match &logical {
                Key::Character(c) if matches!(c.as_str(), "g" | "G" | "i" | "I") => {
                    self.toggle_icons()
                }
                _ => return false,
            }
            return true;
        }
        false
    }

    /// Put the system's icons where cian left room for them.
    ///
    /// Fetching one is not free — the first ask for an unseen kind of file
    /// walks Launch Services — so only a few new ones are fetched per frame and
    /// the rest arrive over the next few. A directory of two thousand files
    /// fills in visibly rather than stopping the world once.
    fn place_icons(&mut self) {
        let Some(t) = self.terminal.as_mut() else { return };
        let (cw, ch) = {
            let (Some(w), Some(grid)) = (self.window.as_ref(), t.size()) else { return };
            let px = w.inner_size();
            if grid.width == 0 || grid.height == 0 {
                return;
            }
            (px.width as f32 / grid.width as f32, px.height as f32 / grid.height as f32)
        };

        // Anything the worker has finished since the last frame, made into
        // textures here — uploading is the renderer's business and the renderer
        // belongs to this thread.
        for answer in self.icons.collect() {
            let key = match &answer.ask {
                iconjob::Ask::Path(p) => IconKey::Path(p.clone()),
                iconjob::Ask::Kind(e) => IconKey::Kind(e.clone()),
                iconjob::Ask::Directory => IconKey::Kind(String::new()),
                iconjob::Ask::Picture { path, w, h } => IconKey::Picture(path.clone(), *w, *h),
            };
            match answer.icon {
                Some(icon) => {
                    let id = self.next_icon_id;
                    self.next_icon_id += 1;
                    if matches!(key, IconKey::Picture(..)) {
                        self.picture_size
                            .insert(key.clone(), (icon.width as f32, icon.height as f32));
                        // One picture at a time. A window-sized photograph is
                        // several megabytes of texture, and the last one is of
                        // no further use the moment this one exists.
                        for (old, id) in std::mem::take(&mut self.picture_ids) {
                            self.icon_ids.remove(&old);
                            self.picture_size.remove(&old);
                            t.evict(id);
                        }
                        self.picture_ids.push((key.clone(), id));
                    }
                    self.icon_ids.insert(key, id);
                    t.upload(id, icon.width, icon.height, icon.rgba);
                }
                // The system has no picture for this one. Remembered as such,
                // so the row falls back to cian's own glyph and nothing asks
                // again.
                None => {
                    self.icon_ids.insert(key, NO_ICON);
                }
            }
        }

        let mut draws = Vec::new();
        for slot in self.cian.icon_slots() {
            let key = IconKey::of(slot);
            let id = match self.icon_ids.get(&key) {
                Some(&NO_ICON) => {
                    // No system icon: cian's own glyph, drawn here because
                    // rasterising one is arithmetic rather than a question for
                    // the operating system.
                    match self.glyph_ids.get(&key) {
                        Some(&NO_ICON) => continue,
                        Some(id) => *id,
                        None => {
                            let want = (slot.h as f32 * ch).ceil() as u32;
                            let px = (want * 2).clamp(32, 256);
                            let made = slot
                                .glyph
                                .zip(self.glyphs.as_ref())
                                .and_then(|((c, rgb), font)| glyph::render(font, c, px, rgb));
                            match made {
                                Some(rgba) => {
                                    let id = self.next_icon_id;
                                    self.next_icon_id += 1;
                                    self.glyph_ids.insert(key.clone(), id);
                                    t.upload(id, px, px, rgba);
                                    id
                                }
                                None => {
                                    self.glyph_ids.insert(key.clone(), NO_ICON);
                                    continue;
                                }
                            }
                        }
                    }
                }
                Some(id) => *id,
                None => {
                    // Not known yet. Ask — which returns at once — and draw
                    // nothing here this time round. The frame never waits for
                    // an answer from the system: see [`iconjob`].
                    let want = (slot.h as f32 * ch).ceil() as u32;
                    let px = (want * 2).clamp(32, 256);
                    if slot.prefer_glyph {
                        // The view asked for cian's own glyph, so the system is
                        // not asked at all.
                        self.icon_ids.insert(key, NO_ICON);
                    } else {
                        let ask = match (&key, slot.local, slot.is_dir) {
                            (IconKey::Path(p), true, _) => iconjob::Ask::Path(p.clone()),
                            (IconKey::Path(_), false, true) => iconjob::Ask::Directory,
                            (IconKey::Path(_), false, false) => iconjob::Ask::Kind(String::new()),
                            (IconKey::Kind(e), _, _) => iconjob::Ask::Kind(e.clone()),
                            // A row asks for an icon, never for a photograph:
                            // `IconKey::of` cannot produce this one.
                            (IconKey::Picture(..), _, _) => continue,
                        };
                        self.icons.want(ask, px);
                    }
                    continue;
                }
            };
            // Square, centred in the cells cian set aside, and inset a little
            // so neighbouring rows do not touch.
            let h = slot.h as f32 * ch * 0.86;
            let w = h;
            let cell_w = slot.w as f32 * cw;
            draws.push(pixels::Draw {
                id,
                x: slot.x as f32 * cw + (cell_w - w) / 2.0,
                y: slot.y as f32 * ch + (slot.h as f32 * ch - h) / 2.0,
                w,
                h,
                alpha: 1.0,
            });
        }
        // The picture in the image popup, which is not an icon and is drawn the
        // same way: the popup left its middle empty and said, in cells, where
        // it goes. Decoded to the pixels that rectangle really is — so what is
        // on screen is the file, not an impression of it in half-blocks.
        if let Some(slot) = self.cian.image_slot().cloned() {
            let box_w = (slot.w as f32 * cw).floor().max(1.0);
            let box_h = (slot.h as f32 * ch).floor().max(1.0);
            let key = IconKey::Picture(slot.path.clone(), box_w as u32, box_h as u32);
            match self.icon_ids.get(&key).copied() {
                Some(NO_ICON) => {}
                Some(id) => {
                    // Centred, at whatever shape came back — the decoder fitted
                    // it to the box and the box is rarely the same shape.
                    let (pw, ph) = self.picture_size.get(&key).copied().unwrap_or((box_w, box_h));
                    draws.push(pixels::Draw {
                        id,
                        x: slot.x as f32 * cw + (box_w - pw) / 2.0,
                        y: slot.y as f32 * ch + (box_h - ph) / 2.0,
                        w: pw,
                        h: ph,
                        alpha: 1.0,
                    });
                }
                None => self.icons.want(
                    iconjob::Ask::Picture {
                        path: slot.path.clone(),
                        w: box_w as u32,
                        h: box_h as u32,
                    },
                    0,
                ),
            }
        }

        // The ghost. Same picture as the file's icon, drawn faintly where the
        // pointer is — which is what a desktop shows, and what makes a drag
        // feel like carrying something rather than like nothing happening.
        if let Some(d) = self.drag.as_ref().filter(|d| d.moved && !d.handed_over) {
            let ghost = d.paths.first().and_then(|p| {
                let slot = self.cian.icon_slots().iter().find(|s| s.path == *p)?;
                self.icon_ids.get(&IconKey::of(slot)).copied()
            });
            if let Some(id) = ghost {
                if id != NO_ICON {
                    let side = ch * 1.6;
                    draws.push(pixels::Draw {
                        id,
                        x: self.at.column as f32 * cw - side / 2.0,
                        y: self.at.row as f32 * ch - side / 2.0,
                        w: side,
                        h: side,
                        alpha: 0.65,
                    });
                }
            }
        }

        if self.keylog {
            eprintln!(
                "icons: {} slots, {} known, {} fetched, {} drawn",
                self.cian.icon_slots().len(),
                self.icon_ids.len(),
                usize::from(self.icons.waiting()),
                draws.len(),
            );
        }

        // The pictures are always one frame behind the text, and there is no
        // way round it: the slots are produced *by* the draw, so the list can
        // only be handed over after it. Normally invisible. Not invisible at
        // all when the view changes — the frame that first draws the lists is
        // still carrying the grid's list, and twenty huge icons sit on top of
        // the file names until something else happens to ask for a repaint.
        // Nothing does, so they stay there.
        //
        // So: whenever the list is not the one already on the layer, ask for
        // the frame that will put it right.
        // Keep painting while the worker still owes an answer, so pictures
        // appear as they land rather than at the next keypress.
        let changed = draws.len() != self.last_draws
            || self.icons.waiting()
            || self.pending_view_change;
        self.last_draws = draws.len();
        self.pending_view_change = false;
        if changed {
            self.needs_redraw = true;
        }
        t.set_frame(draws);
    }

    fn feed(&mut self, ev: Event) {
        if self.cian.handle_event(ev) {
            self.redraw_now();
        }
    }
}

impl ApplicationHandler<Tick> for Gui {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // `CIAN_GUI_PIN=1` nails the window to a known corner and keeps it in
        // front. Only useful for taking a picture of it, which is otherwise a
        // fight with whatever else is on screen.
        let mut attrs = WindowAttributes::default()
            .with_title("cian")
            .with_window_icon(appkit::window_icon())
            .with_inner_size(winit::dpi::LogicalSize::new(1200.0, 800.0));
        if std::env::var_os("CIAN_GUI_PIN").is_some() {
            attrs = attrs
                .with_position(winit::dpi::LogicalPosition::new(40.0, 60.0))
                .with_window_level(winit::window::WindowLevel::AlwaysOnTop);
        }
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("cian-gui: could not open a window: {e}");
                event_loop.exit();
                return;
            }
        };

        // Without this the IME never speaks to the window, and a window that
        // has not asked looks exactly like a platform that cannot.
        window.set_ime_allowed(true);

        match Self::build_terminal(&window, &self.face, self.glyphs.as_ref(), self.size_px) {
            Some(t) => {
                cian_core::log::log(&format!("renderer: {}", t.name()));
                self.terminal = Some(t);
            }
            None => {
                // With the reason, and with the size it was asked for. "Could
                // not start the renderer" sent someone away with nothing to
                // report but the sentence itself.
                let px = window.inner_size();
                let line = format!(
                    "cian: could not start the renderer at {}x{} — see the gpu: lines above. \
                     CIAN_GUI_BACKEND=dx12|vulkan|gl chooses a different one.",
                    px.width, px.height,
                );
                eprintln!("{line}");
                cian_core::log::log(&line);
                event_loop.exit();
                return;
            }
        }

        // After the window exists, not before: setting it earlier applies it
        // to an NSApplication that has not finished launching, and finishing
        // launching puts it back.
        appkit::announce();
        let px = window.inner_size();
        cian_core::log::log(&format!(
            "cian starting in a window {}x{} px, font {}, present {:?}",
            px.width,
            px.height,
            self.font_name,
            present_mode(),
        ));
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.prof && !matches!(event, WindowEvent::RedrawRequested) {
            self.prof_events += 1;
        }
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),

            WindowEvent::Resized(size) => {
                if let Some(t) = self.terminal.as_mut() {
                    t.resize(size.width.max(1), size.height.max(1));
                }
                // cian re-lays-out on resize the same as in a terminal; the
                // numbers in the event are pixels and it wants cells, which it
                // reads off the frame it is about to draw.
                self.feed(Event::Resize(0, 0));
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Released {
                    return;
                }
                // `CIAN_GUI_KEYLOG=1` prints what the window was handed and
                // what cian was told, side by side. Guessing which of the two
                // is wrong has cost this project two rounds already.
                if self.keylog {
                    let sent = input::key(&event, self.mods);
                    eprintln!(
                        "key: winit {:?} / unfolded {:?} / mods {:?}  ->  cian {:?}",
                        event.logical_key,
                        input::base_key(&event, self.mods),
                        self.mods,
                        sent.map(|k| (k.code, k.modifiers)),
                    );
                }
                if self.intercept(&event) {
                    return;
                }
                // A key the IME is composing with belongs to the IME, not to
                // cian: `j` while 未確定 text is being built is part of a word.
                if let Some(k) = input::key(&event, self.mods) {
                    self.feed(Event::Key(k));
                }
            }

            WindowEvent::Ime(ime) => {
                use winit::event::Ime;
                match ime {
                    // The finished string arrives whole. Not a keystroke and
                    // not a paste — a piece of writing, landing at once.
                    Ime::Commit(text) => {
                        self.cian.insert_text(&text);
                        self.redraw_now();
                    }
                    // 未確定 characters. The panes cannot draw them under the
                    // caret yet, so they go where they can at least be read.
                    Ime::Preedit(text, _) if !text.is_empty() => {
                        self.cian.show_message(&format!("[{text}]"));
                        self.redraw_now();
                    }
                    _ => {}
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let at = input::to_cell(position.x, position.y, self.cell_size());
                // Only when it changes cell: a terminal reports movement in
                // cells, and a mouse crosses hundreds of pixels inside one.
                if at.column != self.at.column || at.row != self.at.row {
                    self.at = at;
                    if let Some(d) = self.drag.as_mut() {
                        if (at.column, at.row) != d.from {
                            d.moved = true;
                        }
                    }
                    // Leaving the window hands the gesture to the desktop.
                    // From there it is the desktop's drag: its ghost, its rules,
                    // its snap-back when a drop is refused — and cian's own
                    // ghost stops, because two of them would be two drags.
                    if self.drag.as_ref().is_some_and(|d| d.moved && !d.handed_over)
                        && self.pointer_outside()
                    {
                        let Some(window) = self.window.clone() else { return };
                        let out: Vec<_> = self
                            .drag
                            .as_ref()
                            .map(|d| d.paths.clone())
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|p| dragout::draggable(p))
                            .collect();
                        if !out.is_empty() && dragout::begin(&window, &out) {
                            if let Some(d) = self.drag.as_mut() {
                                d.handed_over = true;
                            }
                            self.redraw_now();
                            return;
                        }
                    }
                    if self.drag.as_ref().is_some_and(|d| d.moved) {
                        // While dragging, the pointer is carrying something
                        // rather than pointing at things; the panes should not
                        // follow it.
                        self.redraw_now();
                        return;
                    }
                    self.feed(Event::Mouse(input::mouse_move(self.held, at, self.mods)));
                }
            }

            // The right button belongs to the desktop, in every view. cian's own
            // menu is on `Shift+Enter`, on `M`, and on `:menu` — and on
            // Shift+right-click, for a hand already holding the mouse.
            //
            // It was cian's menu here, and the desktop's only in the desktop
            // views. Two menus on one button is one menu too many: the button
            // that opens the system's menu everywhere else in the machine
            // should open the system's menu here too, and cian's own is a key
            // away rather than a mode away.
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } if !self.mods.shift_key() && shellmenu::available() => {
                // Point at it first: the menu is about the file under the
                // pointer, and the highlight is what says which that is.
                self.cian.point_at(self.at.column, self.at.row);
                let mut paths: Vec<_> = self
                    .cian
                    .drag_targets_at(self.at.column, self.at.row)
                    .into_iter()
                    .filter(|p| shellmenu::addressable(p))
                    .collect();
                // Empty space is not nothing: it is the folder being looked at,
                // which is what the Finder answers for when a click lands
                // between the files.
                if paths.is_empty() {
                    paths.extend(
                        self.cian
                            .drop_target_at(self.at.column, self.at.row)
                            .filter(|p| shellmenu::addressable(p)),
                    );
                }
                let Some(window) = self.window.clone() else { return };
                let (cw, ch) = self.cell_size();
                let at = shellmenu::to_screen(
                    &window,
                    (self.at.column as u32 * cw) as f64,
                    ((self.at.row + 1) as u32 * ch) as f64,
                );
                if paths.is_empty() || !shellmenu::show(&window, &paths, at) {
                    // Nothing under the pointer, or the shell declined: cian's
                    // own menu rather than no menu at all.
                    self.feed(Event::Mouse(input::mouse_button(
                        MouseButton::Right,
                        ElementState::Pressed,
                        self.at,
                        self.mods,
                    ).unwrap()));
                    return;
                }
                // Whatever was chosen may have changed the directory under us.
                self.cian.reload_panes();
                self.redraw_now();
            }

            WindowEvent::MouseInput { state, button, .. } => {
                self.held = match state {
                    ElementState::Pressed => Some(button),
                    ElementState::Released => None,
                };

                // A double click is two presses of the same button, close in
                // time and on the same cell. The window is told about presses;
                // deciding that two of them are one gesture is this end's job.
                if state == ElementState::Pressed && button == MouseButton::Left {
                    let now = Instant::now();
                    let again = self.last_click.is_some_and(|(at, cell)| {
                        cell == (self.at.column, self.at.row)
                            && now.duration_since(at) < DOUBLE_CLICK
                    });
                    if again {
                        self.last_click = None;
                        if self.cian.double_click(self.at.column, self.at.row) {
                            self.redraw_now();
                            return;
                        }
                    } else {
                        self.last_click = Some((now, (self.at.column, self.at.row)));
                    }
                }

                if state == ElementState::Pressed && button == MouseButton::Left {
                    let paths = self.cian.drag_targets_at(self.at.column, self.at.row);
                    self.drag = if paths.is_empty() {
                        None
                    } else {
                        Some(Drag {
                            paths,
                            from: (self.at.column, self.at.row),
                            moved: false,
                            handed_over: false,
                        })
                    };
                }
                // ...and let go on release, onto whatever is under the pointer.
                if state == ElementState::Released && button == MouseButton::Left {
                    if let Some(d) = self.drag.take().filter(|d| d.moved) {
                        if let Some(dest) = self.cian.drop_target_at(self.at.column, self.at.row) {
                            // A drag moves. Not "moves if a modifier is held" —
                            // picking a thing up and putting it somewhere else
                            // is one gesture with one meaning, and cian has `c`
                            // for the times you meant to leave a copy behind.
                            self.cian.drop_onto(d.paths, dest, true);
                        }
                        self.redraw_now();
                        return;
                    }
                }
                if let Some(m) = input::mouse_button(button, state, self.at, self.mods) {
                    self.feed(Event::Mouse(m));
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (_, ch) = self.cell_size();
                let (dx, dy) = input::wheel(delta, ch);

                // Held down, either modifier turns the wheel into a zoom —
                // browsers, Explorer and Finder all agree on this one.
                if self.mods.control_key() || self.mods.super_key() {
                    self.zoom_rest += dy;
                    // A trackpad reports in fractions of a line; a notch of
                    // wheel is one whole one. Either way, a step per unit.
                    while self.zoom_rest <= -1.0 {
                        self.zoom_rest += 1.0;
                        self.step_font(2);
                    }
                    while self.zoom_rest >= 1.0 {
                        self.zoom_rest -= 1.0;
                        self.step_font(-2);
                    }
                    return;
                }
                // Whole notches only; the remainder is kept so a slow trackpad
                // drag still adds up to a scroll instead of rounding away.
                self.scroll_rest.0 += dx;
                self.scroll_rest.1 += dy;
                let h = self.scroll_rest.0.trunc();
                let v = self.scroll_rest.1.trunc();
                self.scroll_rest.0 -= h;
                self.scroll_rest.1 -= v;
                for m in input::scroll_events(h as i32, v as i32, self.at, self.mods) {
                    self.feed(Event::Mouse(m));
                }
            }

            // Files dropped on the window. One event per file, so a drop of
            // three arrives as three — and cian answers the first with a
            // confirmation dialog, which makes the other two land in whatever
            // that dialog is typing into. They are collected here and handed
            // over as one drop on the next turn of the loop, which is what the
            // gesture was.
            WindowEvent::DroppedFile(path) => {
                if self.keylog {
                    eprintln!("drop: {}", path.display());
                }
                cian_core::log::log(&format!("dropped on the window: {}", path.display()));
                self.dropped.push(path);
                self.redraw_now();
            }

            // Rendering stays inside the callback: on macOS the window server
            // expects the frame to be finished before this returns.
            WindowEvent::RedrawRequested => {
                // Before the frame, never during one: rebuilding the renderer
                // in the middle of drawing with it is the one order that
                // cannot work.
                self.apply_font_size();
                let t0 = Instant::now();
                let Some(t) = self.terminal.as_mut() else { return };
                // A frame can be asked for while the window is minimised — the
                // heartbeat does not know — and a grid with no rows or columns
                // in it is not a frame: the backend walks its cells in chunks
                // of `width`, and a chunk of nothing panics. See `Resized`.
                match t.size() {
                    Some(grid) if grid.width > 0 && grid.height > 0 => {}
                    _ => return,
                }
                if self.cian.take_full_clear() {
                    t.clear();
                }
                // Timed from inside, so "what cian cost" and "what the
                // renderer cost" stay separable whichever renderer it is.
                let cian = &mut self.cian;
                let build = match t.draw(cian) {
                    Ok(build) => build,
                    Err(e) => {
                        cian_core::log::log(&format!("cian-gui draw failed: {e}"));
                        std::time::Duration::ZERO
                    }
                };
                let t1 = Instant::now();
                self.last_frame = t1;
                self.needs_redraw = false;
                self.place_icons();
                if self.prof {
                    let t2 = Instant::now();
                    self.prof_total += t1 - t0;
                    self.prof_build += build;
                    if t1 - t0 > self.prof_worst {
                        self.prof_worst = t1 - t0;
                        self.prof_worst_build = build;
                    }
                    if self.prof_events > self.prof_worst_events {
                        self.prof_worst_events = self.prof_events;
                    }
                    self.prof_events = 0;
                    self.prof_icons += t2 - t1;
                    self.prof_frames += 1;
                    if self.prof_frames == PROF_EVERY {
                        let n = PROF_EVERY;
                        let line = format!(
                            // Microseconds spelled `us`. A `Duration`'s own
                            // `{:?}` writes `µs`, and this line is read in a
                            // Windows console — where it arrived as `ﾂｵs`.
                            "frame x{n}: {} total = cian {} + renderer {}, icons {} \
                             | WORST {} (cian {}), {} events waited{}",
                            us(self.prof_total / n),
                            us(self.prof_build / n),
                            us((self.prof_total - self.prof_build) / n),
                            us(self.prof_icons / n),
                            us(self.prof_worst),
                            us(self.prof_worst_build),
                            self.prof_worst_events,
                            // Which part of cian's own time, when the parts are
                            // being counted. See `cian_tui::prof`.
                            cian_tui::prof::take_report(n),
                        );
                        eprintln!("{line}");
                        cian_core::log::log(&line);
                        self.prof_frames = 0;
                        self.prof_worst = std::time::Duration::ZERO;
                        self.prof_worst_build = std::time::Duration::ZERO;
                        self.prof_worst_events = 0;
                        self.prof_total = std::time::Duration::ZERO;
                        self.prof_build = std::time::Duration::ZERO;
                        self.prof_icons = std::time::Duration::ZERO;
                    }
                }
            }

            _ => {}
        }
    }

    /// The heartbeat: give cian a turn at everything that happens between
    /// keystrokes — background jobs, transitions, shell output, icons filling
    /// in — and repaint if any of it changed the screen.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, _tick: Tick) {
        let now = Instant::now();
        if now < self.next_tick {
            // The thread pokes faster than cian always needs; when it is idle
            // it asks for a longer gap and the extra pokes are dropped here.
            return;
        }
        self.next_tick = now + self.cian.tick_interval();

        // A drop that has finished arriving: one gesture, however many files
        // winit reported it as.
        if !self.dropped.is_empty() {
            let paths = std::mem::take(&mut self.dropped);
            let text = paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            self.feed(Event::Paste(text));
        }

        // An edit request would hand a terminal to vim. There isn't one.
        if self.cian.pending_edit().is_some() {
            self.cian.decline_edit(
                "外部エディタは窓版では未対応です（シェル枠で開く予定）— cian --tui で使えます",
            );
            self.needs_redraw = true;
        }

        if self.cian.take_icon_view_close() {
            self.set_view(View::Classic);
        }
        self.needs_redraw |= self.cian.tick();
        // Profiling paints continuously, on purpose: the question it answers is
        // what a frame costs, and frames only happen when something changes.
        if self.prof {
            self.needs_redraw = true;
        }

        if self.cian.should_quit() {
            event_loop.exit();
            return;
        }

        if let Some(w) = self.window.as_ref() {
            let title = self.cian.title();
            if title != self.title {
                w.set_title(&title);
                self.title = title;
            }
            // The title bar belongs to the desktop, and the desktop will paint
            // it to match — dark or light — if it is told which. Without this
            // a dark cian wore a white title bar, which is the one piece of
            // the window cian cannot draw itself.
            let light = self.cian.theme_is_light();
            if self.told_light != Some(light) {
                self.told_light = Some(light);
                w.set_theme(Some(if light {
                    winit::window::Theme::Light
                } else {
                    winit::window::Theme::Dark
                }));
                caption_colour(w, self.cian.theme_surface());
            }
            if self.needs_redraw {
                w.request_redraw();
            }
        }

    }

    /// Block until something happens. The heartbeat is one of the things that
    /// happens, so nothing is lost by sleeping between them.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Every event that was waiting has now been handled, and *this* is the
        // moment to paint.
        //
        // The terminal build has always done it this way without having to
        // think about it: `event::poll` blocks, and when it wakes it drains
        // everything the terminal has — every key of a repeat, every notch of a
        // wheel — runs them all, and paints once. The window was painting once
        // per event instead. Hold the up arrow and the key repeats faster than
        // a frame can be drawn and presented, so the events queue, and the
        // cursor goes on climbing for a second *after* the key is released —
        // which is exactly what was reported, and is the clearest possible
        // description of a backlog.
        //
        // Asking here costs no latency: the queue is empty, so the frame is
        // drawn immediately. It simply cannot be asked for more often than
        // there is input to justify it.
        if self.needs_redraw {
            if let Some(w) = self.window.as_ref() {
                w.request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.cian.finish();
        // And then stop, rather than unwinding out through everything that was
        // built on the way in.
        //
        // Closing the window was hanging. The session is saved and the shells
        // are killed by `finish` above; what is left is teardown — a GPU
        // device, a surface, a pseudo-console, a font atlas — and every one of
        // those is a thing that can decide to wait. None of it needs doing:
        // the process is ending, and the operating system reclaims all of it
        // whether or not the destructors run. What a person wants when they
        // press the close button is for the window to go away.
        //
        // Everything with a reason to be flushed has been flushed first. If
        // that ever stops being true, it belongs in `finish`, not here.
        cian_core::log::log("cian closing");
        std::process::exit(0);
    }
}

/// Take back the console cian was started from, if it was started from one.
///
/// A windowed Windows program is given no standard streams. That is right for
/// the double-click case — the whole point of the subsystem line above — and
/// wrong for `cian --version` typed at a prompt, where the answer would go
/// nowhere. Attaching to the parent's console puts it back; when there is no
/// parent console (the double-click case) the call simply fails and nothing
/// changes.
#[cfg(windows)]
fn attach_console() {
    // SAFETY: no arguments but a constant, and the only failure mode is "there
    // was no console", which is reported by the return value.
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
}

#[cfg(not(windows))]
fn attach_console() {}

fn main() -> anyhow::Result<()> {
    // Before anything can be printed, and whatever the arguments: a startup
    // that fails says why, and "why" belongs in the terminal it was typed in.
    attach_console();
    // A window with no console swallows a panic whole: the process simply
    // vanishes, and the report is "it closed". `CIAN_LOG` is the only place a
    // crash can leave a note, so it leaves one there.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        cian_core::log::log(&format!("PANIC: {info}"));
        previous(info);
    }));
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| matches!(a.as_str(), "-v" | "-V" | "--version")) {
        println!("{}", cian_tui::version_text());
        return Ok(());
    }
    if args.iter().any(|a| matches!(a.as_str(), "-h" | "--help")) {
        println!("{}", cian_tui::usage_text());
        return Ok(());
    }
    let mut paths = args.iter().filter(|a| !a.starts_with('-'));
    let left = paths.next().map(std::path::PathBuf::from);
    let right = paths.next().map(std::path::PathBuf::from);

    let (bytes, font_name) = match font::load() {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    let Some(face) = Font::new(bytes) else {
        eprintln!("cian-gui: {font_name} is not a font this renderer can read.");
        std::process::exit(2);
    };

    let mut cian = Session::start(left, right, StartupMacro::None)?;
    // `--finder` opens straight into the desktop look. Someone meeting cian for
    // the first time should not have to find a keystroke to stop it looking
    // like a terminal.
    // Which view to open in. The default is the details list: it is the one
    // that looks least like a terminal, and the window exists for the person
    // who did not choose a terminal. `cian.set_option("view", "classic")` in
    // init.lua is how someone who knows what they want says so once.
    let want = args
        .iter()
        .find_map(|a| match a.as_str() {
            "--classic" => Some("classic"),
            "--finder" | "--details" => Some("details"),
            "--icons" => Some("icons"),
            _ => None,
        })
        .or_else(|| cian.configured_view())
        .unwrap_or("details");
    match want {
        "classic" => cian.set_skin(Skin::Classic),
        "icons" => {
            cian.set_icon_view(true);
            cian.set_skin(Skin::Finder);
        }
        _ => cian.set_skin(Skin::Finder),
    }
    // No handshake, no doubt: in a window the keys are distinct before anyone
    // thinks about bytes.
    cian.set_keys_distinguishable(true);
    // A window can draw a real icon, so cian should stop drawing a clipped
    // glyph where one goes.
    cian.set_native_icons(true);

    let event_loop = EventLoop::<Tick>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        // Ends on its own when the loop closes and `send_event` starts failing.
        while proxy.send_event(Tick).is_ok() {
            std::thread::sleep(HEARTBEAT);
        }
    });

    let mut gui = Gui::new(cian, face, bytes, font_name);
    event_loop.run_app(&mut gui)?;
    Ok(())
}
