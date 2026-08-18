//! What a front end needs from cian, and nothing else.
//!
//! cian has two of them now. The terminal build owns a terminal, blocks on
//! `event::poll`, and lets crossterm decide what a keystroke was. The windowed
//! build owns a window, is called back by winit, and has to decide that itself.
//! Neither difference reaches the file panes.
//!
//! So the state stays here and the *loop* moves out. A front end owns three
//! things — a surface to draw on, a source of events, and the turn of the loop
//! — and asks [`Session`] for everything else:
//!
//! ```ignore
//! let mut cian = Session::start(left, right, StartupMacro::None)?;
//! loop {
//!     if redraw { terminal.draw(|f| cian.draw(f))?; }
//!     for ev in my_events() { redraw |= cian.handle_event(ev); }
//!     redraw |= cian.tick();
//!     if cian.should_quit() { break; }
//! }
//! cian.finish();
//! ```
//!
//! Events arrive as crossterm's [`Event`] even in the window, where no terminal
//! is involved. That is deliberate: crossterm's event types are plain enums
//! that need no terminal to exist, and cian's key handling — some five hundred
//! matches over `KeyCode` and `KeyModifiers` — is written against them. A
//! window that translates its own input into that shape gets all of it for
//! free. The alternative, an event type of cian's own, would have meant
//! rewriting every one of those matches to gain nothing.

use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{Event, KeyEventKind};
use ratatui::Frame;

use crate::{prepare_app, App, Host, Skin, StartupMacro};

/// A running cian, minus the screen and the keyboard.
pub struct Session {
    app: App,
    /// The theme in force before a skin swapped it, so switching back is a
    /// restore rather than a guess at what was there.
    saved_theme: Option<crate::theme::ResolvedTheme>,
    /// Whether the front end *can* draw system icons, as opposed to whether the
    /// current skin *wants* them.
    native_icons: bool,
}

impl Session {
    /// Load config, restore the last session, and build the application state.
    ///
    /// Does not touch a terminal, a window, or a screen — the caller owns all
    /// three by the time this returns.
    ///
    /// This is the *windowed* entry point, and says so: cian then keeps to
    /// itself the two things it would otherwise say about a terminal — advice
    /// about which one to use, and the plainer corners a console font needs.
    pub fn start(
        left: Option<PathBuf>,
        right: Option<PathBuf>,
        startup: StartupMacro,
    ) -> Result<Self> {
        Ok(Self {
            app: prepare_app(left, right, startup, Host::Window)?,
            saved_theme: None,
            native_icons: false,
        })
    }

    /// Tell cian that every key arrives distinguishable — that Ctrl+H is not
    /// Backspace, Ctrl+I is not Tab, and Ctrl+M is not Enter.
    ///
    /// In a terminal this is a request the emulator may refuse, which is why
    /// cian asks and then remembers the answer. A window has no such doubt: the
    /// key events are already distinct before anyone thinks about bytes. So the
    /// windowed front end simply says so.
    pub fn set_keys_distinguishable(&mut self, yes: bool) {
        self.app.kbd_enhanced = yes;
    }

    /// Paint the current state into `f`.
    pub fn draw(&mut self, f: &mut Frame) {
        crate::render::draw(f, &mut self.app);
    }

    /// Choose how the file panes are drawn. See [`Skin`].
    ///
    /// Switching to [`Skin::Finder`] brings its palette with it, because the
    /// borderless shape only reads on a light surface — a borderless dark pane
    /// is not a Finder, it is a pane with its edges missing. Switching back
    /// restores whatever theme was in force before.
    ///
    /// Not over a theme the user asked for, though. See
    /// [`crate::theme::theme_is_the_users`].
    pub fn set_skin(&mut self, skin: Skin) {
        if self.app.skin == skin {
            return;
        }
        // The colours in force: cian's, or ones somebody chose. A theme named in
        // init.lua or picked with `:theme` was being thrown away the moment the
        // view changed, so the same cian looked like two programs.
        let mine = crate::theme::theme_is_the_users();
        match skin {
            Skin::Finder if crate::theme::skin_may_swap_theme(mine) => {
                self.saved_theme = Some(crate::theme::theme());
                crate::set_theme(crate::theme::ResolvedTheme::FINDER);
            }
            Skin::Finder => {}
            Skin::Classic => {
                if let Some(t) = self.saved_theme.take() {
                    crate::set_theme(t);
                }
            }
        }
        self.app.skin = skin;
        self.app.icon_slots.clear();
    }

    /// The view named in `init.lua`, if one was.
    ///
    /// `cian.set_option("view", "classic")`. Read by the windowed front end at
    /// startup; the terminal build has no views to choose between.
    pub fn configured_view(&self) -> Option<&str> {
        self.app.config.options.view.as_deref()
    }

    /// Which skin is in force.
    pub fn skin(&self) -> Skin {
        self.app.skin
    }

    /// Say that this front end draws file icons itself.
    ///
    /// cian then leaves those cells blank and reports where they were, via
    /// [`icon_slots`](Self::icon_slots). Left off, it keeps drawing the Nerd
    /// Font glyph it always has — which is all a terminal can do, and which a
    /// window clips, because the ink of those glyphs runs past their advance.
    pub fn set_native_icons(&mut self, yes: bool) {
        self.native_icons = yes;
        // Every view, not just the desktop-shaped ones. Keeping the Nerd Font
        // glyph in the classic view was the plan, and the plan lost: the ink of
        // those glyphs runs past the cell they are given, so a window clips them
        // and the listing ends up full of half-drawn symbols. A picture has no
        // such limit.
        self.app.native_icons = yes;
        self.app.icon_slots.clear();
    }

    /// Show the left pane as a grid of pictures rather than two lists.
    ///
    /// Needs [`set_native_icons`](Self::set_native_icons): the grid draws names
    /// and a selection, and nothing else — the pictures are the caller's, and
    /// without them it is a grid of labels floating in space.
    pub fn set_icon_view(&mut self, yes: bool) {
        self.app.icon_view = yes;
        // One pane fills the window, so the focus has to be on a file pane and
        // not on the shell — otherwise the grid would be drawing one thing
        // while the keys and the mouse acted on another.
        if yes && self.app.focused == crate::FocusedPane::Shell {
            self.app.focus(crate::FocusedPane::Left);
        }
        // The grid draws its own pictures whatever the skin, because a grid
        // with no pictures in it is not a view of anything.
        self.app.icon_slots.clear();
    }

    /// Whether the grid is showing.
    pub fn icon_view(&self) -> bool {
        self.app.icon_view
    }

    /// Has the grid's ✕ been pressed? Reading it clears it.
    ///
    /// The grid cannot leave itself: which view is showing is the front end's
    /// to decide, and the button only asks.
    pub fn take_icon_view_close(&mut self) -> bool {
        std::mem::take(&mut self.app.icon_view_close)
    }

    /// A double click at this cell. Returns whether the grid took it.
    ///
    /// Double clicks have to be recognised by the caller: a window is told
    /// about presses and releases and nothing else, so the two-in-quick-
    /// succession judgement belongs to whoever owns the clock.
    pub fn double_click(&mut self, column: u16, row: u16) -> bool {
        self.app.grid_double_click(column, row)
    }

    /// What a press at this cell would pick up, if anything.
    pub fn drag_targets_at(&self, column: u16, row: u16) -> Vec<PathBuf> {
        self.app.drag_targets_at(column, row)
    }

    /// Put the cursor on whatever is at this cell. Returns whether it landed.
    ///
    /// For a front end about to show the *system's* menu about that file: the
    /// desktop moves its highlight to whatever was right-clicked, and cian's
    /// highlight is the only sign of which file the menu is about.
    pub fn point_at(&mut self, column: u16, row: u16) -> bool {
        self.app.point_at(column, row)
    }

    /// Where letting go at this cell would put them, if anywhere.
    pub fn drop_target_at(&self, column: u16, row: u16) -> Option<PathBuf> {
        self.app.drop_target_at(column, row)
    }

    /// Ask to put these files there. Opens cian's own confirmation — a drag is
    /// a new way to say "move", not a new way to move files unasked.
    pub fn drop_onto(&mut self, targets: Vec<PathBuf>, dest: PathBuf, move_it: bool) {
        self.app.drop_onto(targets, dest, move_it);
    }

    /// Read both file panes again.
    ///
    /// For a front end that has just let something *else* change the disk — the
    /// desktop's own context menu, which can rename, delete, extract or install
    /// while cian watches — and cannot wait for the next poll to notice.
    pub fn reload_panes(&mut self) {
        self.app.reload_both_panes();
    }

    /// Where icons belong, as of the last [`draw`](Self::draw). In cells.
    pub fn icon_slots(&self) -> &[crate::IconSlot] {
        &self.app.icon_slots
    }

    /// Where the open picture goes, if one is open and this front end draws
    /// its own. In cells, as of the last [`draw`](Self::draw).
    ///
    /// The popup leaves that rectangle empty rather than filling it with
    /// half-blocks. Decode the file to those pixels and put it there; see
    /// [`crate::ImageSlot`].
    pub fn image_slot(&self) -> Option<&crate::ImageSlot> {
        self.app.image_slot.as_ref()
    }

    /// Whether the surface must be wiped before the next paint.
    ///
    /// Terminal graphics live outside the cell grid, so a picture that stops
    /// being shown only goes away when the screen is cleared. A window draws
    /// every pixel it owns and never needs this, but it costs nothing to ask.
    pub fn take_full_clear(&mut self) -> bool {
        std::mem::take(&mut self.app.full_clear)
    }

    /// Feed cian one event. Returns whether the screen needs repainting.
    ///
    /// A key that fails never ends the session: navigation can fail for
    /// ordinary reasons — a directory vanished, a path turned out not to be one
    /// — and the answer to that is a message on the status line, not an exit.
    pub fn handle_event(&mut self, ev: Event) -> bool {
        match ev {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                // Input always wins over eye candy: land any transition
                // immediately rather than making the user wait for it.
                self.app.finish_anim();
                if let Err(e) = self.app.handle_key(key) {
                    self.app.message = Some(format!("✖ {}", crate::first_line(&e)));
                    cian_core::log::log(&format!("key error: {e:#}"));
                }
                true
            }
            Event::Mouse(m) => {
                self.app.handle_mouse(m);
                true
            }
            // A paste, or a file dropped onto the window — which arrives the
            // same way. `accept_drop` takes it only when every item really is a
            // file on disk.
            Event::Paste(text) => {
                if !self.app.accept_drop(&text) {
                    self.app.insert_into_active_text(&text);
                }
                true
            }
            Event::Resize(_, _) => true,
            _ => false,
        }
    }

    /// Advance everything that happens between keystrokes — background jobs,
    /// animations, shell output, the input method following the mode. Returns
    /// whether the screen needs repainting.
    pub fn tick(&mut self) -> bool {
        self.app.tick_background()
    }

    /// Put text in wherever cian is currently taking text, as if it had been
    /// typed. What a committed IME string is: not a keystroke, not a paste, but
    /// a finished piece of writing arriving all at once.
    pub fn insert_text(&mut self, text: &str) {
        self.app.insert_into_active_text(text);
    }

    /// Show text on the status line. The windowed front end uses it for 未確定
    /// characters until the panes learn to draw them in place.
    pub fn show_message(&mut self, text: &str) {
        self.app.message = Some(text.to_string());
    }

    /// How long the front end may sleep before calling [`tick`](Self::tick)
    /// again.
    ///
    /// Short while something is moving — a transition, a flash, a file
    /// operation or a reply in flight — so the motion stays smooth; longer
    /// otherwise, so an idle cian costs nothing. The terminal loop spends this
    /// as a poll timeout and the windowed one as a wake-up deadline, which is
    /// the same thing said twice.
    pub fn tick_interval(&self) -> std::time::Duration {
        let busy = self.app.anim.is_some()
            || self.app.flash.is_some()
            || self.app.op_job.is_some()
            || self.app.ai_job.is_some()
            || self.app.crmaine_rx.is_some();
        std::time::Duration::from_millis(if busy { 16 } else { 33 })
    }

    /// Has the user asked to leave?
    pub fn should_quit(&self) -> bool {
        self.app.should_quit
    }

    /// A file the user asked to open in an external editor, if one is waiting.
    ///
    /// The terminal build answers this by handing its terminal to vim. A window
    /// has no terminal to hand over, so the windowed build has to answer it
    /// some other way — and until it does, [`decline_edit`](Self::decline_edit)
    /// says so on the status line rather than leaving the request stuck.
    pub fn pending_edit(&self) -> Option<PathBuf> {
        self.app.pending_edit.as_ref().map(|e| e.path.clone())
    }

    /// Drop a pending edit request and explain why nothing happened.
    pub fn decline_edit(&mut self, why: &str) {
        self.app.pending_edit = None;
        self.app.message = Some(why.to_string());
    }

    /// What the window should be called: cian, plus the directory in front of
    /// the user. A window in the dock with three cians in it is otherwise three
    /// identical entries.
    /// What the window should be called.
    ///
    /// The name, and nothing else. It carried the current directory for a
    /// while — the reasoning was that three cians in a taskbar are otherwise
    /// three identical entries — and the reasoning was worse than the cost: a
    /// title bar showing a long path is a long path in the title bar, and the
    /// path is already on screen twice, in the address bar and the breadcrumb.
    pub fn title(&self) -> String {
        "cian".to_string()
    }

    /// Is the theme a light one?
    ///
    /// For a front end that has furniture of its own to colour — a title bar
    /// belongs to the desktop, and the desktop will paint it dark or light if
    /// it is told which. Measured off the surface cian is actually drawing on
    /// rather than off the theme's name, because a per-pane override or the
    /// Finder skin can make a "dark" theme light in practice.
    pub fn theme_is_light(&self) -> bool {
        let (r, g, b) = match crate::theme::theme().base_bg.unwrap_or(crate::theme::theme().popup_bg)
        {
            ratatui::style::Color::Rgb(r, g, b) => (r as u32, g as u32, b as u32),
            // A named colour means a terminal palette cian did not choose;
            // dark is the safer guess for a terminal-shaped theme.
            _ => return false,
        };
        // The usual weighting: green is most of what the eye calls brightness.
        (299 * r + 587 * g + 114 * b) / 1000 > 128
    }

    /// The colour cian is drawing its background in, if it is a real one.
    ///
    /// For a front end that can paint furniture the desktop owns — see
    /// [`theme_is_light`](Self::theme_is_light).
    pub fn theme_surface(&self) -> Option<(u8, u8, u8)> {
        match crate::theme::theme().base_bg.unwrap_or(crate::theme::theme().popup_bg) {
            ratatui::style::Color::Rgb(r, g, b) => Some((r, g, b)),
            _ => None,
        }
    }

    /// Save the session and give the input method back the way it was found.
    /// Call once, on the way out.
    pub fn finish(&mut self) {
        self.app.save_session();
        self.app.release_ime();
        // Before anything is dropped: a pseudo-console on Windows is closed by
        // waiting for the program inside it, and the program inside it may be
        // the reason cian is being closed. See `ShellPane::kill_all`.
        self.app.shell.kill_all();
    }
}
