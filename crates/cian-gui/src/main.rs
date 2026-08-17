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
//! * Pictures still draw as half-blocks. Drawing them as pixels is the next
//!   thing the window makes possible.

mod appkit;
mod dragout;
mod font;
mod glyph;
mod input;
mod pixels;
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

/// How many icons may be fetched from the system in one frame.
///
/// Low on purpose. The cost is in the first ask for a kind of file, and a
/// listing that fills in over three frames is invisible where one that
/// stalls for a fifth of a second is not.
const ICONS_PER_FRAME: usize = 6;

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

struct Gui {
    cian: Session,
    window: Option<Arc<Window>>,
    terminal: Option<Terminal<WgpuBackend<'static, 'static, PixelLayer>>>,
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
    /// Print every key to stderr — see `CIAN_GUI_KEYLOG`.
    keylog: bool,
    /// Which texture holds which file's icon. Never emptied: an icon costs a
    /// few kilobytes and the same files are looked at again and again.
    icon_ids: std::collections::HashMap<std::path::PathBuf, u64>,
    next_icon_id: u64,
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
    /// A drag in progress: what was picked up, and where it started.
    ///
    /// Held from the press, but not a drag until the pointer has moved a cell
    /// away — otherwise every click would be a one-pixel drag and the ghost
    /// would flicker on each one.
    drag: Option<Drag>,
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
            keylog: std::env::var_os("CIAN_GUI_KEYLOG").is_some(),
            icon_ids: std::collections::HashMap::new(),
            // Ids start above the smoke test's, which uses 1.
            next_icon_id: 100,
            last_draws: 0,
            pending_view_change: false,
            last_click: None,
            zoom_rest: 0.0,
            drag: None,
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
        let Ok(grid) = t.size() else { return false };
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
        self.needs_redraw = true;
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    /// One character cell, in physical pixels. Derived from the surface rather
    /// than asked of the renderer: the grid and the window agree on it by
    /// construction, so dividing one by the other is exact.
    fn cell_size(&self) -> (u32, u32) {
        let (Some(w), Some(t)) = (self.window.as_ref(), self.terminal.as_ref()) else {
            return (0, 0);
        };
        let px = w.inner_size();
        let Ok(grid) = t.size() else { return (0, 0) };
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
        let want = (self.size_px as i32 + by).clamp(SIZE_MIN as i32, SIZE_MAX as i32) as u32;
        if want == self.size_px {
            return;
        }
        let Some(window) = self.window.clone() else { return };
        self.size_px = want;

        match Self::build_terminal(&window, &self.face, self.size_px) {
            Some(t) => {
                self.terminal = Some(t);
                // The new renderer has no textures in it, so nothing may claim
                // to have uploaded one. Left behind, every icon would be a
                // draw call against a texture that does not exist.
                self.icon_ids.clear();
                self.next_icon_id = 100;
                self.last_draws = 0;
                self.cian.show_message(&format!("文字サイズ {}px", self.size_px));
            }
            None => {
                self.size_px = (self.size_px as i32 - by) as u32;
                self.cian.show_message("文字サイズを変更できませんでした");
            }
        }
        self.redraw_now();
    }

    /// Build a renderer for this window at this size. Shared by startup and by
    /// every font change, so the two can never drift apart.
    fn build_terminal(
        window: &Arc<Window>,
        face: &Font<'static>,
        size_px: u32,
    ) -> Option<Terminal<WgpuBackend<'static, 'static, PixelLayer>>> {
        let size = window.inner_size();
        let backend = pollster::block_on(
            Builder::<PixelLayer>::from_font(face.clone())
                .with_font_size_px(size_px)
                .with_width_and_height(Dimensions {
                    width: NonZeroU32::new(size.width.max(1)).unwrap(),
                    height: NonZeroU32::new(size.height.max(1)).unwrap(),
                })
                .build_with_target(window.clone()),
        )
        .ok()?;
        Terminal::new(backend).ok()
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
            t.backend_mut().post_processor_mut().set_frame(Vec::new());
        }
        self.last_draws = 0;
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
            let (Some(w), Ok(grid)) = (self.window.as_ref(), t.size()) else { return };
            let px = w.inner_size();
            if grid.width == 0 || grid.height == 0 {
                return;
            }
            (px.width as f32 / grid.width as f32, px.height as f32 / grid.height as f32)
        };

        let mut draws = Vec::new();
        let mut fetched = 0;
        for slot in self.cian.icon_slots() {
            let id = match self.icon_ids.get(slot.path.as_path()) {
                Some(&NO_ICON) => continue,
                Some(id) => *id,
                None => {
                    // The budget is per frame, not per directory: a row whose
                    // icon has not been asked for yet simply draws nothing this
                    // time round and is picked up on the next.
                    if fetched >= ICONS_PER_FRAME {
                        continue;
                    }
                    fetched += 1;
                    // Twice the size it will be drawn at, so the sampler is
                    // always shrinking a picture rather than stretching one.
                    // Asking for exactly the drawn size leaves it to macOS to
                    // pick a representation, and the one it picks for an
                    // in-between size is the smaller — which is what made every
                    // folder look like a blown-up thumbnail.
                    let want = (slot.h as f32 * ch).ceil() as u32;
                    let px = (want * 2).clamp(32, 256);
                    if let Some((c, rgb)) = slot.glyph {
                        let Some(font) = self.glyphs.as_ref() else { continue };
                        let Some(rgba) = glyph::render(font, c, px, rgb) else {
                            self.icon_ids.insert(slot.path.clone(), NO_ICON);
                            continue;
                        };
                        let id = self.next_icon_id;
                        self.next_icon_id += 1;
                        self.icon_ids.insert(slot.path.clone(), id);
                        t.backend_mut().post_processor_mut().upload(id, px, px, rgba);
                        id
                    } else {
                    // A file on a server has a path this disk knows nothing
                    // about, so it is asked for by type instead. Cached under
                    // the same path key, which is what makes a directory of a
                    // hundred `.log` files cost one lookup.
                    let icon = if slot.local {
                        sysicon::icon_for(&slot.path, px)
                    } else {
                        let ext = slot
                            .path
                            .extension()
                            .map(|e| e.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        sysicon::icon_for_type(&ext, slot.is_dir, px)
                    };
                    let Some(icon) = icon else {
                        if self.keylog {
                            eprintln!("icon: none for {}", slot.path.display());
                        }
                        // Remembered as "no icon" so it is not asked for again
                        // every frame for the rest of the session.
                        self.icon_ids.insert(slot.path.clone(), NO_ICON);
                        continue;
                    };
                    let id = self.next_icon_id;
                    self.next_icon_id += 1;
                    self.icon_ids.insert(slot.path.clone(), id);
                    t.backend_mut().post_processor_mut().upload(
                        id,
                        icon.width,
                        icon.height,
                        icon.rgba,
                    );
                    id
                    }
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
        // The ghost. Same picture as the file's icon, drawn faintly where the
        // pointer is — which is what a desktop shows, and what makes a drag
        // feel like carrying something rather than like nothing happening.
        if let Some(d) = self.drag.as_ref().filter(|d| d.moved && !d.handed_over) {
            if let Some(&id) = d.paths.first().and_then(|p| self.icon_ids.get(p.as_path())) {
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
                fetched,
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
        let changed = draws.len() != self.last_draws
            || fetched > 0
            || self.pending_view_change;
        self.last_draws = draws.len();
        self.pending_view_change = false;
        if changed {
            self.needs_redraw = true;
        }
        t.backend_mut().post_processor_mut().set_frame(draws);
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

        match Self::build_terminal(&window, &self.face, self.size_px) {
            Some(t) => self.terminal = Some(t),
            None => {
                eprintln!("cian: could not start the renderer");
                event_loop.exit();
                return;
            }
        }

        // After the window exists, not before: setting it earlier applies it
        // to an NSApplication that has not finished launching, and finishing
        // launching puts it back.
        appkit::announce();
        cian_core::log::log(&format!("cian starting in a window, font {}", self.font_name));
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),

            WindowEvent::Resized(size) => {
                if let Some(t) = self.terminal.as_mut() {
                    t.backend_mut().resize(size.width.max(1), size.height.max(1));
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

            // Files dropped on the window. cian already understands a list of
            // paths arriving as text, because that is how a terminal delivers a
            // drop too.
            WindowEvent::DroppedFile(path) => {
                self.feed(Event::Paste(path.display().to_string()));
            }

            // Rendering stays inside the callback: on macOS the window server
            // expects the frame to be finished before this returns.
            WindowEvent::RedrawRequested => {
                let Some(t) = self.terminal.as_mut() else { return };
                if self.cian.take_full_clear() {
                    let _ = t.clear();
                }
                let cian = &mut self.cian;
                if let Err(e) = t.draw(|f| cian.draw(f)) {
                    cian_core::log::log(&format!("cian-gui draw failed: {e}"));
                }
                self.needs_redraw = false;
                self.place_icons();
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
            if self.needs_redraw {
                w.request_redraw();
            }
        }

    }

    /// Block until something happens. The heartbeat is one of the things that
    /// happens, so nothing is lost by sleeping between them.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.cian.finish();
    }
}

fn main() -> anyhow::Result<()> {
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
