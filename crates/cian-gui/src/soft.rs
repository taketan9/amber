//! Drawing the window without a graphics card.
//!
//! The windowed build renders through wgpu, which is the right answer on a
//! machine with a driver. This one does not have one:
//!
//!     gpu: Dx12 Microsoft Basic Render Driver (Cpu, driver 10.0.19041.7548)
//!
//! That is WARP, Windows' software rasteriser, and it is the *only* adapter
//! there — `vulkan` and `gl` both refuse to start. Every frame was being
//! composed by cian in one millisecond and then put on screen in a hundred and
//! thirty: seven frames a second, on a machine whose terminal build is, in its
//! owner's words, とんでもなく早い.
//!
//! Which is the clue worth following. A terminal draws text on the CPU, and it
//! is fast, because drawing text on a CPU is not hard: fill some rectangles,
//! stamp some glyphs, hand the pixels to the window. Going through a general
//! purpose 3D pipeline *emulated in software* to do that is the slowest
//! possible route to the same picture.
//!
//! So this is the other route. `softbuffer` gives a plain array of pixels that
//! the platform blits into the window — on Windows a DIB and one `BitBlt`, no
//! driver involved — and cian's cells are rasterised into it directly, with the
//! font it already carries. Two things make it quick enough to forget about:
//! every glyph is rasterised once and kept, and only the cells that changed are
//! painted again.
//!
//! It is chosen automatically when the only adapter is a software one, which
//! means nobody has to know any of the above.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;

use ab_glyph::{Font, FontRef, Glyph, ScaleFont};
use unicode_width::UnicodeWidthChar;
use cian_tui::ratatui::backend::{Backend, ClearType, WindowSize};
use cian_tui::ratatui::buffer::Cell;
use cian_tui::ratatui::layout::{Position, Size};
use cian_tui::ratatui::style::{Color, Modifier};
use winit::window::Window;

/// One picture to draw over the text, in physical pixels. The same shape the
/// wgpu layer takes, so the front end does not care which is underneath.
#[derive(Clone, Copy)]
pub struct Draw {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub alpha: f32,
}

/// A rasterised glyph: coverage values, and where they sit relative to the pen.
struct Stamp {
    w: usize,
    h: usize,
    /// How far above the baseline the ink starts — negative, in the usual case
    /// of a glyph that sits on the line rather than under it.
    top: i32,
    cover: Vec<u8>,
}

/// An icon, kept as straight RGBA at whatever size it arrived.
struct Picture {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

/// Never fails. The trait wants an error type; this one has nothing to say.
#[derive(Debug)]
pub struct Never;

impl std::fmt::Display for Never {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unreachable")
    }
}

impl core::error::Error for Never {}

pub struct SoftBackend {
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    font: FontRef<'static>,
    /// Pixels per cell, and where the baseline sits inside one.
    cell_w: u32,
    cell_h: u32,
    baseline: i32,
    px_w: u32,
    px_h: u32,
    cols: u16,
    rows: u16,

    /// What the screen holds, and what it held: only the difference is drawn.
    cells: Vec<Cell>,
    painted: Vec<Cell>,
    /// The pixels, kept between frames so an unchanged cell is not touched.
    pixels: Vec<u32>,
    /// Everything must be painted next time — after a resize, or a `clear`.
    all_dirty: bool,

    glyphs: HashMap<(char, bool), Stamp>,
    pictures: HashMap<u64, Picture>,
    frame: Vec<Draw>,
    /// The pictures drawn last time, so the cells beneath a picture that moved
    /// are repainted.
    drawn: Vec<Draw>,

    cursor: Position,
    cursor_visible: bool,
}

impl SoftBackend {
    /// Build one for this window at this font size, or `None` if the platform
    /// will not give us a surface to blit into.
    pub fn new(window: Arc<Window>, font: FontRef<'static>, size_px: u32) -> Option<Self> {
        let context = softbuffer::Context::new(window.clone()).ok()?;
        let surface = softbuffer::Surface::new(&context, window.clone()).ok()?;

        // The cell, measured from the font rather than guessed: cian's font is
        // monospaced, so one advance is every advance.
        let scaled = font.as_scaled(size_px as f32);
        let cell_w = scaled.h_advance(font.glyph_id('M')).ceil().max(1.0) as u32;
        let cell_h = (scaled.ascent() - scaled.descent() + scaled.line_gap()).ceil().max(1.0) as u32;
        let baseline = scaled.ascent().ceil() as i32;

        let mut out = Self {
            surface,
            font,
            cell_w,
            cell_h,
            baseline,
            px_w: 0,
            px_h: 0,
            cols: 0,
            rows: 0,
            cells: Vec::new(),
            painted: Vec::new(),
            pixels: Vec::new(),
            all_dirty: true,
            glyphs: HashMap::new(),
            pictures: HashMap::new(),
            frame: Vec::new(),
            drawn: Vec::new(),
            cursor: Position::new(0, 0),
            cursor_visible: false,
        };
        let px = window.inner_size();
        out.resize(px.width.max(1), px.height.max(1));
        Some(out)
    }

    /// The window changed size: work out the new grid and start again.
    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 || (w == self.px_w && h == self.px_h) {
            return;
        }
        self.px_w = w;
        self.px_h = h;
        let cols = (w / self.cell_w).max(1) as u16;
        let rows = (h / self.cell_h).max(1) as u16;
        // Only when the *grid* changes is what is on it forgotten.
        //
        // A window whose pixels changed by a few but whose cells did not is
        // still showing the same screen, and cian will not send it again: the
        // next draw carries only what differs from what it believes is up
        // there. Throwing the cells away here — which is what this did, on the
        // resize winit sends immediately after the first frame — left a window
        // holding one cell of text and nothing to refill it with.
        if cols != self.cols || rows != self.rows {
            let n = cols as usize * rows as usize;
            self.cols = cols;
            self.rows = rows;
            self.cells = vec![Cell::EMPTY; n];
            self.painted = vec![Cell::EMPTY; n];
        }
        self.pixels = vec![0u32; (w * h) as usize];
        // The pixels are new, so nothing on screen is what `painted` says.
        self.painted.iter_mut().for_each(|c| *c = Cell::EMPTY);
        self.all_dirty = true;
        if let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) {
            let _ = self.surface.resize(nw, nh);
        }
    }

    /// Hand over a picture to draw over the text later. Kept until replaced.
    pub fn upload(&mut self, id: u64, w: u32, h: u32, rgba: Vec<u8>) {
        if w == 0 || h == 0 {
            return;
        }
        self.pictures.insert(id, Picture { w, h, rgba });
    }

    /// What to draw over the text this frame.
    pub fn set_frame(&mut self, draws: Vec<Draw>) {
        self.frame = draws;
    }

    /// The stamp for this character, rasterising it the first time it is seen.
    ///
    /// This is the whole reason the CPU path is fast enough: a listing is a few
    /// dozen distinct characters repeated thousands of times, and each one is
    /// turned into coverage once for the life of the window.
    fn stamp(&mut self, c: char, bold: bool) -> Option<&Stamp> {
        let key = (c, bold);
        if !self.glyphs.contains_key(&key) {
            let scaled = self.font.as_scaled(self.cell_h as f32 * 0.82);
            let glyph: Glyph = self
                .font
                .glyph_id(c)
                .with_scale_and_position(scaled.scale(), ab_glyph::point(0.0, 0.0));
            let outlined = self.font.outline_glyph(glyph)?;
            let bounds = outlined.px_bounds();
            let (w, h) = (bounds.width().ceil() as usize, bounds.height().ceil() as usize);
            if w == 0 || h == 0 {
                return None;
            }
            let mut cover = vec![0u8; w * h];
            outlined.draw(|x, y, v| {
                let (x, y) = (x as usize, y as usize);
                if x < w && y < h {
                    // Bold without a bold face: the same coverage, harder. Not
                    // a second font, but enough that a bold row reads as one.
                    let v = if bold { (v * 1.35).min(1.0) } else { v };
                    cover[y * w + x] = (v * 255.0) as u8;
                }
            });
            self.glyphs.insert(
                key,
                Stamp { w, h, top: bounds.min.y as i32, cover },
            );
        }
        self.glyphs.get(&key)
    }

    /// A wide character owns the cell after it, and that cell owns nothing.
    ///
    /// ratatui writes `あ` into one cell and an empty symbol into the next; the
    /// glyph is drawn from the first and runs over both. Painting the second on
    /// its own puts its *default* background over the right half of the
    /// character — which on a light theme is a black box where half a letter
    /// should be, and is exactly what the first Japanese screen showed.
    fn owner_of(&self, col: u16, row: u16) -> u16 {
        if col == 0 {
            return col;
        }
        // Asked of the *left* neighbour, not of this cell. ratatui does not
        // mark the second half of a wide character in any way a backend can
        // see — it `reset`s it, which is to say it becomes a space with the
        // default colours — so "is this cell empty" was never the question.
        // "Does the cell before it own two" is.
        let left = row as usize * self.cols as usize + col as usize - 1;
        let wide = self
            .cells
            .get(left)
            .and_then(|c| c.symbol().chars().next())
            .map(|c| c.width().unwrap_or(1) > 1)
            .unwrap_or(false);
        if wide {
            col - 1
        } else {
            col
        }
    }

    /// Paint one cell's pixels: its background, then its character over it.
    fn paint_cell(&mut self, col: u16, row: u16) {
        let idx = row as usize * self.cols as usize + col as usize;
        let Some(cell) = self.cells.get(idx).cloned() else { return };
        // How many cells this character covers — a wide one is painted, and
        // cleared, across both of them.
        let span = cell
            .symbol()
            .chars()
            .next()
            .and_then(|c| c.width())
            .filter(|w| *w > 1)
            .map(|_| 2u32)
            .unwrap_or(1);

        let reverse = cell.modifier.contains(Modifier::REVERSED);
        let (mut fg, mut bg) = (rgb(cell.fg, FG), rgb(cell.bg, BG));
        if reverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.modifier.contains(Modifier::DIM) {
            fg = mix(fg, bg, 0.45);
        }
        // The caret, drawn as the block a terminal draws.
        if self.cursor_visible && self.cursor.x == col && self.cursor.y == row {
            std::mem::swap(&mut fg, &mut bg);
        }

        let x0 = col as u32 * self.cell_w;
        let y0 = row as u32 * self.cell_h;
        let packed = pack(bg);
        let fill_w = self.cell_w * span;
        for y in y0..(y0 + self.cell_h).min(self.px_h) {
            let row_start = (y * self.px_w) as usize;
            let from = row_start + x0 as usize;
            let to = (row_start + (x0 + fill_w).min(self.px_w) as usize).min(self.pixels.len());
            if from < to {
                self.pixels[from..to].fill(packed);
            }
        }

        let symbol = cell.symbol();
        let Some(c) = symbol.chars().next().filter(|c| !c.is_whitespace()) else { return };
        let bold = cell.modifier.contains(Modifier::BOLD);
        let (px_w, px_h, baseline, cell_w) =
            (self.px_w, self.px_h, self.baseline, self.cell_w);
        // Rasterised into the cache, then taken out of it: the painting below
        // writes to `self.pixels`, and a borrow of the cache would still be
        // live across it. Cloning the coverage would be a copy per glyph per
        // frame, which is the one thing this path cannot afford.
        if self.stamp(c, bold).is_none() {
            return;
        }
        let Some(stamp) = self.glyphs.remove(&(c, bold)) else { return };

        // Centred in the cell horizontally; on the baseline vertically.
        let pen_x = x0 as i32 + (((cell_w * span) as i32 - stamp.w as i32) / 2).max(0);
        let pen_y = y0 as i32 + baseline + stamp.top;
        for sy in 0..stamp.h {
            let y = pen_y + sy as i32;
            if y < 0 || y as u32 >= px_h {
                continue;
            }
            for sx in 0..stamp.w {
                let x = pen_x + sx as i32;
                if x < 0 || x as u32 >= px_w {
                    continue;
                }
                let a = stamp.cover[sy * stamp.w + sx];
                if a == 0 {
                    continue;
                }
                let at = (y as u32 * px_w + x as u32) as usize;
                let under = unpack(self.pixels[at]);
                self.pixels[at] = pack(mix(fg, under, a as f32 / 255.0));
            }
        }
        self.glyphs.insert((c, bold), stamp);
    }

    /// Alpha-blend the pictures over the text.
    fn paint_pictures(&mut self) {
        let frame = std::mem::take(&mut self.frame);
        for d in &frame {
            let Some(pic) = self.pictures.get(&d.id) else { continue };
            let (dw, dh) = (d.w.max(1.0), d.h.max(1.0));
            let x0 = d.x.max(0.0) as u32;
            let y0 = d.y.max(0.0) as u32;
            for oy in 0..dh as u32 {
                let y = y0 + oy;
                if y >= self.px_h {
                    break;
                }
                // Nearest neighbour: an icon is drawn at about the size it
                // arrived at, and the difference is not worth a filter written
                // to run on every pixel of every frame.
                let sy = (oy as f32 / dh * pic.h as f32) as u32;
                for ox in 0..dw as u32 {
                    let x = x0 + ox;
                    if x >= self.px_w {
                        break;
                    }
                    let sx = (ox as f32 / dw * pic.w as f32) as u32;
                    let si = ((sy.min(pic.h - 1) * pic.w + sx.min(pic.w - 1)) * 4) as usize;
                    let Some(px) = pic.rgba.get(si..si + 4) else { continue };
                    let a = px[3] as f32 / 255.0 * d.alpha;
                    if a <= 0.004 {
                        continue;
                    }
                    let at = (y * self.px_w + x) as usize;
                    let under = unpack(self.pixels[at]);
                    self.pixels[at] = pack(mix((px[0], px[1], px[2]), under, a));
                }
            }
        }
        self.frame = frame;
    }

    /// Which cells a picture covers, so they are repainted when it moves away.
    fn cells_under(&self, d: &Draw) -> (u16, u16, u16, u16) {
        let c0 = (d.x.max(0.0) as u32 / self.cell_w) as u16;
        let r0 = (d.y.max(0.0) as u32 / self.cell_h) as u16;
        let c1 = (((d.x + d.w).max(0.0) as u32) / self.cell_w) as u16;
        let r1 = (((d.y + d.h).max(0.0) as u32) / self.cell_h) as u16;
        (c0, r0, c1.min(self.cols.saturating_sub(1)), r1.min(self.rows.saturating_sub(1)))
    }
}

/// The colours a `Reset` means here. cian sets its own on nearly every cell;
/// these are what is left.
const FG: (u8, u8, u8) = (0xcd, 0xcd, 0xda);
const BG: (u8, u8, u8) = (0x1a, 0x1b, 0x26);

fn pack((r, g, b): (u8, u8, u8)) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

fn unpack(v: u32) -> (u8, u8, u8) {
    (((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8)
}

/// `a` over `b`, by `t`.
fn mix(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> (u8, u8, u8) {
    let f = |x: u8, y: u8| (x as f32 * t + y as f32 * (1.0 - t)) as u8;
    (f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

/// A ratatui colour as bytes. cian's themes are truecolor throughout; the named
/// ones are what a stray `Color::Red` in a message would use.
fn rgb(c: Color, fallback: (u8, u8, u8)) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Reset => fallback,
        Color::Black => (0x1a, 0x1b, 0x26),
        Color::Red => (0xf7, 0x76, 0x8e),
        Color::Green => (0x9e, 0xce, 0x6a),
        Color::Yellow => (0xe0, 0xaf, 0x68),
        Color::Blue => (0x7a, 0xa2, 0xf7),
        Color::Magenta => (0xbb, 0x9a, 0xf7),
        Color::Cyan => (0x7d, 0xcf, 0xff),
        Color::Gray => (0xa9, 0xb1, 0xd6),
        Color::DarkGray => (0x41, 0x48, 0x68),
        Color::LightRed => (0xff, 0x9e, 0x9e),
        Color::LightGreen => (0xb9, 0xf2, 0x7c),
        Color::LightYellow => (0xff, 0xc7, 0x77),
        Color::LightBlue => (0x9a, 0xbd, 0xf5),
        Color::LightMagenta => (0xd2, 0xa6, 0xff),
        Color::LightCyan => (0xa4, 0xda, 0xff),
        Color::White => (0xc0, 0xca, 0xf5),
        Color::Indexed(i) => {
            // The 6×6×6 cube and the greys, which is all anything reaches for.
            let i = i as u32;
            if i >= 232 {
                let v = (8 + (i - 232) * 10) as u8;
                (v, v, v)
            } else if i >= 16 {
                let i = i - 16;
                let step = |v: u32| if v == 0 { 0 } else { (55 + v * 40) as u8 };
                (step(i / 36), step((i / 6) % 6), step(i % 6))
            } else {
                fallback
            }
        }
    }
}

impl Backend for SoftBackend {
    type Error = Never;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            if x >= self.cols || y >= self.rows {
                continue;
            }
            let idx = y as usize * self.cols as usize + x as usize;
            if let Some(slot) = self.cells.get_mut(idx) {
                *slot = cell.clone();
            }
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.cursor_visible = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        Ok(self.cursor)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.cursor = position.into();
        Ok(())
    }

    /// Repaint everything next time — and do *not* forget what is on screen.
    ///
    /// For a terminal backend, clearing means erasing what the terminal is
    /// showing, and the caller redraws afterwards. Here the pixels are cian's
    /// own and the cells are the only record of them: emptying that record
    /// leaves a blank window that nothing refills, because the next draw only
    /// sends what *changed* — and against a screen cian believes it has already
    /// drawn, nothing has. One frame of text, then darkness, which is what the
    /// first run of this renderer did.
    fn clear(&mut self) -> Result<(), Self::Error> {
        self.all_dirty = true;
        Ok(())
    }

    fn clear_region(&mut self, _clear_type: ClearType) -> Result<(), Self::Error> {
        self.clear()
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(Size::new(self.cols, self.rows))
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        Ok(WindowSize {
            columns_rows: Size::new(self.cols, self.rows),
            pixels: Size::new(self.px_w as u16, self.px_h as u16),
        })
    }

    /// Paint what changed, then hand the pixels to the window.
    fn flush(&mut self) -> Result<(), Self::Error> {
        if self.pixels.is_empty() {
            return Ok(());
        }
        // Cells whose contents differ from what is on screen — plus, when a
        // picture has moved, the cells it used to be over and the ones it is
        // over now, because those pixels are not the cells' own.
        let mut dirty: Vec<usize> = Vec::new();
        if self.all_dirty {
            dirty.extend(0..self.cells.len());
        } else {
            for (i, (now, was)) in self.cells.iter().zip(self.painted.iter()).enumerate() {
                if now != was {
                    dirty.push(i);
                }
            }
            if self.frame.len() != self.drawn.len()
                || self.frame.iter().zip(self.drawn.iter()).any(|(a, b)| {
                    a.id != b.id || a.x != b.x || a.y != b.y || a.w != b.w || a.alpha != b.alpha
                })
            {
                let boxes: Vec<Draw> =
                    self.frame.iter().chain(self.drawn.iter()).copied().collect();
                for d in boxes {
                    let (c0, r0, c1, r1) = self.cells_under(&d);
                    for r in r0..=r1 {
                        for c in c0..=c1 {
                            dirty.push(r as usize * self.cols as usize + c as usize);
                        }
                    }
                }
                dirty.sort_unstable();
                dirty.dedup();
            }
        }

        if dirty.is_empty() {
            return Ok(());
        }
        if std::env::var_os("CIAN_SOFT_DEBUG").is_some() {
            let filled = self.cells.iter().filter(|c| c.symbol() != " ").count();
            cian_core::log::log(&format!(
                "soft: {}x{} cells ({}px/{}px each), {} dirty, {} with a symbol, {} glyphs cached",
                self.cols,
                self.rows,
                self.cell_w,
                self.cell_h,
                dirty.len(),
                filled,
                self.glyphs.len(),
            ));
        }
        let mut owners: Vec<(u16, u16)> = dirty
            .into_iter()
            .map(|i| {
                let (col, row) = ((i % self.cols as usize) as u16, (i / self.cols as usize) as u16);
                (self.owner_of(col, row), row)
            })
            .collect();
        owners.sort_unstable();
        owners.dedup();
        for (col, row) in owners {
            self.paint_cell(col, row);
        }
        self.paint_pictures();

        self.painted.clone_from(&self.cells);
        self.drawn.clone_from(&self.frame);
        self.all_dirty = false;

        if let Ok(mut buf) = self.surface.buffer_mut() {
            let n = buf.len().min(self.pixels.len());
            buf[..n].copy_from_slice(&self.pixels[..n]);
            let _ = buf.present();
        }
        Ok(())
    }
}
