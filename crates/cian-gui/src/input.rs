//! winit's idea of a keystroke, said in crossterm's words.
//!
//! cian decides what a key means in some five hundred matches over
//! `KeyCode` and `KeyModifiers`. None of that is about terminals — it is about
//! `j` moving down and `Ctrl+H` focusing left — so the windowed build keeps all
//! of it and translates its input into the same shape instead.
//!
//! The translation is not quite a renaming. Two things genuinely differ:
//!
//! * **A window knows what a terminal has to guess.** In a terminal, Ctrl+H,
//!   Ctrl+I and Ctrl+M arrive as the same bytes as Backspace, Tab and Enter,
//!   which is why cian asks the emulator for the kitty keyboard protocol and
//!   remembers whether it got it. Here they are simply different events, and
//!   are reported as such.
//! * **A window is told about keys a terminal never sees.** Command on macOS
//!   and the Windows key never reach a terminal application at all. They arrive
//!   here, so they are carried through as `SUPER` and can be bound.

use cian_tui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use winit::event::{ElementState, MouseButton as WinitButton, MouseScrollDelta};
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Modifier state, said in crossterm's words.
pub fn modifiers(m: ModifiersState) -> KeyModifiers {
    let mut out = KeyModifiers::empty();
    if m.shift_key() {
        out |= KeyModifiers::SHIFT;
    }
    if m.control_key() {
        out |= KeyModifiers::CONTROL;
    }
    if m.alt_key() {
        out |= KeyModifiers::ALT;
    }
    // Carried rather than dropped: cian cannot be given a Super binding through
    // a terminal, because a terminal is never told about one.
    if m.super_key() {
        out |= KeyModifiers::SUPER;
    }
    out
}

/// Which key was pressed, for the purpose of matching a binding.
///
/// Two different questions hide behind "what key is this", and the answer has
/// to change with which one is being asked:
///
/// * **Typing.** The platform has already folded Shift and the layout into the
///   character — `A`, `!`, `é` — and that folded character is the answer.
/// * **A shortcut.** The platform has also folded *Ctrl* in, and there the
///   folding destroys the question. macOS turns Ctrl+H into `U+0008` and
///   Option+G into `©`; asked what key that was, "backspace" and "copyright
///   sign" are both wrong. `key_without_modifiers` unfolds it back to `h` and
///   `g` while still respecting the keyboard layout, which is exactly what a
///   binding wants.
///
/// So: the unfolded key when a modifier that means "shortcut" is down, the
/// folded one otherwise.
pub fn base_key(ev: &winit::event::KeyEvent, mods: ModifiersState) -> Key {
    let shortcut = mods.control_key() || mods.alt_key() || mods.super_key();
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if shortcut {
        use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
        return ev.key_without_modifiers();
    }
    let _ = shortcut;
    ev.logical_key.clone()
}

/// A winit key press as a crossterm [`KeyEvent`], or `None` for a key cian has
/// nothing to say about (a bare modifier, a media key, a dead key).
pub fn key(ev: &winit::event::KeyEvent, mods: ModifiersState) -> Option<KeyEvent> {
    let logical = base_key(ev, mods);
    let shortcut = mods.control_key() || mods.alt_key() || mods.super_key();

    let code = match &logical {
        Key::Named(n) => named(*n)?,
        Key::Character(s) => {
            // One `char` per key. A `Key::Character` holding more than one is a
            // dead-key composition or an IME artefact, and that path belongs to
            // the IME handler, not here.
            let mut chars = s.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            // Unfolding the modifiers took the capital off with them, so put it
            // back: cian reads a letter's case as the Shift, deliberately —
            // "the uppercase char is the signal, the SHIFT modifier bit is not
            // reliably reported for letters across terminals".
            if shortcut && mods.shift_key() {
                KeyCode::Char(c.to_ascii_uppercase())
            } else {
                KeyCode::Char(c)
            }
        }
        // Dead keys and unidentified keys mean nothing on their own.
        _ => return None,
    };

    // ...and for the same reason, never report it twice. Shift lives in the
    // character for letters and in the modifier for everything else — arrows,
    // Enter, the function keys — which is where cian looks for it.
    let mut m = modifiers(mods);
    if matches!(code, KeyCode::Char(_)) {
        m.remove(KeyModifiers::SHIFT);
    }

    // Shift+Tab is its own key everywhere else, so it is here too.
    let code = if code == KeyCode::Tab && mods.shift_key() { KeyCode::BackTab } else { code };

    Some(KeyEvent {
        code,
        modifiers: m,
        kind: match ev.state {
            ElementState::Pressed => KeyEventKind::Press,
            ElementState::Released => KeyEventKind::Release,
        },
        state: KeyEventState::NONE,
    })
}

/// The named keys cian actually handles. Everything else is deliberately
/// dropped rather than invented: a key cian has no branch for is better as
/// nothing than as a wrong guess.
fn named(n: NamedKey) -> Option<KeyCode> {
    Some(match n {
        NamedKey::Enter => KeyCode::Enter,
        NamedKey::Tab => KeyCode::Tab,
        NamedKey::Space => KeyCode::Char(' '),
        NamedKey::Backspace => KeyCode::Backspace,
        NamedKey::Delete => KeyCode::Delete,
        NamedKey::Escape => KeyCode::Esc,
        NamedKey::Insert => KeyCode::Insert,
        NamedKey::Home => KeyCode::Home,
        NamedKey::End => KeyCode::End,
        NamedKey::PageUp => KeyCode::PageUp,
        NamedKey::PageDown => KeyCode::PageDown,
        NamedKey::ArrowUp => KeyCode::Up,
        NamedKey::ArrowDown => KeyCode::Down,
        NamedKey::ArrowLeft => KeyCode::Left,
        NamedKey::ArrowRight => KeyCode::Right,
        NamedKey::F1 => KeyCode::F(1),
        NamedKey::F2 => KeyCode::F(2),
        NamedKey::F3 => KeyCode::F(3),
        NamedKey::F4 => KeyCode::F(4),
        NamedKey::F5 => KeyCode::F(5),
        NamedKey::F6 => KeyCode::F(6),
        NamedKey::F7 => KeyCode::F(7),
        NamedKey::F8 => KeyCode::F(8),
        NamedKey::F9 => KeyCode::F(9),
        NamedKey::F10 => KeyCode::F(10),
        NamedKey::F11 => KeyCode::F(11),
        NamedKey::F12 => KeyCode::F(12),
        _ => return None,
    })
}

/// Where the pointer is, in cells rather than pixels.
#[derive(Clone, Copy, Default)]
pub struct CellPos {
    pub column: u16,
    pub row: u16,
}

/// Pixels to cells. `cell` is the size of one character cell in physical
/// pixels; a zero there would mean the surface has not been sized yet, so the
/// pointer is reported at the origin rather than dividing by nothing.
pub fn to_cell(x: f64, y: f64, cell: (u32, u32)) -> CellPos {
    let (cw, ch) = cell;
    if cw == 0 || ch == 0 {
        return CellPos::default();
    }
    CellPos {
        column: (x.max(0.0) as u32 / cw).min(u16::MAX as u32) as u16,
        row: (y.max(0.0) as u32 / ch).min(u16::MAX as u32) as u16,
    }
}

fn button(b: WinitButton) -> Option<MouseButton> {
    Some(match b {
        WinitButton::Left => MouseButton::Left,
        WinitButton::Right => MouseButton::Right,
        WinitButton::Middle => MouseButton::Middle,
        _ => return None,
    })
}

/// A press or release, as a crossterm [`MouseEvent`].
pub fn mouse_button(
    b: WinitButton,
    state: ElementState,
    at: CellPos,
    mods: ModifiersState,
) -> Option<MouseEvent> {
    let b = button(b)?;
    Some(MouseEvent {
        kind: match state {
            ElementState::Pressed => MouseEventKind::Down(b),
            ElementState::Released => MouseEventKind::Up(b),
        },
        column: at.column,
        row: at.row,
        modifiers: modifiers(mods),
    })
}

/// Pointer movement. `held` is the button being dragged, if any — a terminal
/// reports a drag and a hover as different things, so this does too.
pub fn mouse_move(held: Option<WinitButton>, at: CellPos, mods: ModifiersState) -> MouseEvent {
    let kind = match held.and_then(button) {
        Some(b) => MouseEventKind::Drag(b),
        None => MouseEventKind::Moved,
    };
    MouseEvent { kind, column: at.column, row: at.row, modifiers: modifiers(mods) }
}

/// A wheel turn as `(horizontal, vertical)` notches, positive meaning right and
/// down — the one-notch-at-a-time unit a terminal reports in.
///
/// A trackpad reports fractional pixels by the hundred, so pixel deltas are
/// divided by the cell height and the remainder is the caller's to keep;
/// otherwise a gentle two-finger scroll would fire hundreds of line-scrolls.
pub fn wheel(delta: MouseScrollDelta, cell_h: u32) -> (f64, f64) {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => (x as f64, -(y as f64)),
        MouseScrollDelta::PixelDelta(p) => {
            let h = cell_h.max(1) as f64;
            (p.x / h, -(p.y / h))
        }
    }
}

/// Build the scroll events for `notches` in one direction.
pub fn scroll_events(
    horizontal: i32,
    vertical: i32,
    at: CellPos,
    mods: ModifiersState,
) -> Vec<MouseEvent> {
    let m = modifiers(mods);
    let mut out = Vec::new();
    let mut push = |kind| out.push(MouseEvent { kind, column: at.column, row: at.row, modifiers: m });
    for _ in 0..vertical.abs() {
        push(if vertical > 0 { MouseEventKind::ScrollDown } else { MouseEventKind::ScrollUp });
    }
    for _ in 0..horizontal.abs() {
        push(if horizontal > 0 { MouseEventKind::ScrollRight } else { MouseEventKind::ScrollLeft });
    }
    out
}
