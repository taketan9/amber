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

/// What a box-drawing or block character covers, in fractions of its cell.
///
/// These are not letters and drawing them as letters is why the borders looked
/// like they had been drawn freehand. A font's `─` is an outline scaled to
/// 0.82 of the cell and *centred* in it, so it stops short of both edges and
/// the next cell's `─` starts short of its own: a border comes out as a dashed,
/// wandering line. `│` lands wherever rounding puts it, so a vertical border
/// wobbles by a pixel from row to row — which is exactly the "wavy" that was
/// reported. And `█`, drawn the same way, is a block with gaps around it, which
/// is why the scrollbar could not be seen.
///
/// Every terminal emulator worth using draws these itself instead, and this is
/// that: rectangles in cell-relative coordinates, snapped to whole pixels, so a
/// line meets its neighbour exactly and a block fills its cell exactly.
#[derive(Clone, Copy)]
struct Part {
    /// Left, top, right, bottom, each 0.0–1.0 of the cell.
    l: f32,
    t: f32,
    r: f32,
    b: f32,
    /// How much of the foreground to mix in. The shades (░▒▓) are the only
    /// ones that are not solid.
    ink: f32,
}

const fn part(l: f32, t: f32, r: f32, b: f32) -> Part {
    Part { l, t, r, b, ink: 1.0 }
}

/// A line's thickness, as a fraction of the cell. Light strokes are one
/// device pixel at ordinary sizes and grow with the font; heavy ones are twice
/// that, which is the distinction the characters themselves make.
const LIGHT: f32 = 0.09;
const HEAVY: f32 = 0.18;

/// Half a cell, give or take the stroke: where a line stops when it is a
/// corner or a tee rather than a crossing.
const MID: f32 = 0.5;

/// The rectangles for one character, or `None` if it is an ordinary glyph.
///
/// The lines are built from four arms — left, up, right, down — so a corner, a
/// tee and a crossing are the same code with different arms.
fn cell_art(c: char) -> Option<[Option<Part>; 4]> {
    // (left, up, right, down) as stroke widths; 0.0 means "no arm".
    let l = LIGHT;
    let h = HEAVY;
    let arms: (f32, f32, f32, f32) = match c {
        // Straight lines.
        '─' | '╌' | '╍' => (l, 0.0, l, 0.0),
        '━' => (h, 0.0, h, 0.0),
        '│' | '╎' | '╏' => (0.0, l, 0.0, l),
        '┃' => (0.0, h, 0.0, h),
        // Corners, square and rounded. A rounded corner is one or two pixels of
        // curve at these sizes; the join is what matters and the join is square.
        '┌' | '╭' => (0.0, 0.0, l, l),
        '┐' | '╮' => (l, 0.0, 0.0, l),
        '└' | '╰' => (0.0, l, l, 0.0),
        '┘' | '╯' => (l, l, 0.0, 0.0),
        '┏' => (0.0, 0.0, h, h),
        '┓' => (h, 0.0, 0.0, h),
        '┗' => (0.0, h, h, 0.0),
        '┛' => (h, h, 0.0, 0.0),
        // Tees and the crossing.
        '├' => (0.0, l, l, l),
        '┤' => (l, l, 0.0, l),
        '┬' => (l, 0.0, l, l),
        '┴' => (l, l, l, 0.0),
        '┼' => (l, l, l, l),
        '┣' => (0.0, h, h, h),
        '┫' => (h, h, 0.0, h),
        '┳' => (h, 0.0, h, h),
        '┻' => (h, h, h, 0.0),
        '╋' => (h, h, h, h),
        _ => return blocks(c),
    };
    let (la, ua, ra, da) = arms;
    let mut out = [None; 4];
    // Each arm reaches the cell edge and stops in the middle, so two arms of
    // the same width meeting across a cell boundary make one unbroken line.
    if la > 0.0 {
        let half = la / 2.0;
        out[0] = Some(part(0.0, MID - half, MID + half.max(ra / 2.0), MID + half));
    }
    if ra > 0.0 {
        let half = ra / 2.0;
        out[1] = Some(part(MID - half.max(la / 2.0), MID - half, 1.0, MID + half));
    }
    if ua > 0.0 {
        let half = ua / 2.0;
        out[2] = Some(part(MID - half, 0.0, MID + half, MID + half.max(da / 2.0)));
    }
    if da > 0.0 {
        let half = da / 2.0;
        out[3] = Some(part(MID - half, MID - half.max(ua / 2.0), MID + half, 1.0));
    }
    Some(out)
}

/// The blocks and shades, which are rectangles by definition.
fn blocks(c: char) -> Option<[Option<Part>; 4]> {
    let one = |p: Part| Some([Some(p), None, None, None]);
    let eighth = |n: f32| n / 8.0;
    match c {
        '█' => one(part(0.0, 0.0, 1.0, 1.0)),
        '▀' => one(part(0.0, 0.0, 1.0, 0.5)),
        '▄' => one(part(0.0, 0.5, 1.0, 1.0)),
        '▌' => one(part(0.0, 0.0, 0.5, 1.0)),
        '▐' => one(part(0.5, 0.0, 1.0, 1.0)),
        // Eighths, up from the bottom and in from the left.
        '▁'..='▇' => {
            let n = c as u32 - '▁' as u32 + 1;
            one(part(0.0, 1.0 - eighth(n as f32), 1.0, 1.0))
        }
        '▏' => one(part(0.0, 0.0, eighth(1.0), 1.0)),
        '▎' => one(part(0.0, 0.0, eighth(2.0), 1.0)),
        '▍' => one(part(0.0, 0.0, eighth(3.0), 1.0)),
        '▋' => one(part(0.0, 0.0, eighth(5.0), 1.0)),
        '▊' => one(part(0.0, 0.0, eighth(6.0), 1.0)),
        '▉' => one(part(0.0, 0.0, eighth(7.0), 1.0)),
        '▕' => one(part(1.0 - eighth(1.0), 0.0, 1.0, 1.0)),
        // The shades are the whole cell at a fraction of the ink. Dithering
        // them would be more faithful to a VGA font and worse to look at.
        '░' => one(Part { ink: 0.25, ..part(0.0, 0.0, 1.0, 1.0) }),
        '▒' => one(Part { ink: 0.5, ..part(0.0, 0.0, 1.0, 1.0) }),
        '▓' => one(Part { ink: 0.75, ..part(0.0, 0.0, 1.0, 1.0) }),
        _ => None,
    }
}

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
    /// Where the caret was when the screen was last painted, if it was showing.
    ///
    /// The caret is not in the cells — it is a swap of two colours done while
    /// painting — so moving it changes two cells' pixels without changing
    /// either cell. Nothing marked them for repainting, and the block stayed
    /// where it had been: a trail of them along a shell prompt, which is the
    /// "old text left on screen" that survived every other fix here. `:redraw`
    /// cleared it, which is exactly what a missed repaint looks like.
    painted_cursor: Option<Position>,
    /// When the debug line was last written. See `CIAN_SOFT_DEBUG`.
    said: Option<std::time::Instant>,
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
            painted_cursor: None,
            said: None,
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

    /// Forget a picture. For the ones that are megabytes rather than a
    /// thumbnail, and are not wanted twice.
    pub fn evict(&mut self, id: u64) {
        self.pictures.remove(&id);
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
        // Lines and blocks are drawn, not typeset. See [`cell_art`].
        if let Some(parts) = cell_art(c) {
            for p in parts.into_iter().flatten() {
                let x1 = x0 + (p.l * fill_w as f32).round() as u32;
                let x2 = x0 + (p.r * fill_w as f32).round().max(1.0) as u32;
                let y1 = y0 + (p.t * self.cell_h as f32).round() as u32;
                let y2 = y0 + (p.b * self.cell_h as f32).round().max(1.0) as u32;
                // A stroke that rounds to nothing is still a stroke: a hairline
                // border is thin, not absent.
                let x2 = x2.max(x1 + 1);
                let y2 = y2.max(y1 + 1);
                let ink = if p.ink >= 1.0 { pack(fg) } else { pack(mix(fg, bg, p.ink)) };
                for y in y1..y2.min(self.px_h) {
                    let row_start = (y * self.px_w) as usize;
                    let from = row_start + x1 as usize;
                    let to = (row_start + x2.min(self.px_w) as usize).min(self.pixels.len());
                    if from < to {
                        self.pixels[from..to].fill(ink);
                    }
                }
            }
            return;
        }
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
        // Clipped to the cell it belongs to, not merely to the window.
        //
        // A glyph whose ink is wider than its advance — a Nerd Font icon, a
        // box character in a face that draws them long, some CJK forms — was
        // painted straight over its neighbours' pixels. Those pixels belong to
        // cells that were not redrawn, so the overspill stayed on screen after
        // the character that made it had gone: the leftovers reported in the
        // shell panel, under a preview, and along the hint bar.
        let (clip_x0, clip_x1) = (x0, (x0 + fill_w).min(px_w));
        let (clip_y0, clip_y1) = (y0, (y0 + self.cell_h).min(px_h));
        for sy in 0..stamp.h {
            let y = pen_y + sy as i32;
            if y < 0 || (y as u32) < clip_y0 || y as u32 >= clip_y1 {
                continue;
            }
            for sx in 0..stamp.w {
                let x = pen_x + sx as i32;
                if x < 0 || (x as u32) < clip_x0 || x as u32 >= clip_x1 {
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
                // Bilinear, and it is not a luxury: the shell keeps ready-made
                // icons at 32 pixels, the grid draws its tiles at a hundred, and
                // nearest neighbour turns a 32-pixel icon into visible squares —
                // "とてつもなく粗い", as reported. Four taps a pixel is nothing
                // beside the decode that produced the picture.
                let fy = (oy as f32 + 0.5) / dh * pic.h as f32 - 0.5;
                for ox in 0..dw as u32 {
                    let x = x0 + ox;
                    if x >= self.px_w {
                        break;
                    }
                    let fx = (ox as f32 + 0.5) / dw * pic.w as f32 - 0.5;
                    let Some((rgb, a)) = sample(pic, fx, fy) else { continue };
                    let a = a * d.alpha;
                    if a <= 0.004 {
                        continue;
                    }
                    let at = (y * self.px_w + x) as usize;
                    let under = unpack(self.pixels[at]);
                    self.pixels[at] = pack(mix(rgb, under, a));
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

/// The colour of a picture at a fractional position, blended from the four
/// texels around it.
///
/// Alpha is blended too, and the colour is weighted *by* alpha — otherwise the
/// transparent border around an icon drags its (arbitrary) colour into the
/// edge, and every icon comes out with a dark halo.
fn sample(pic: &Picture, fx: f32, fy: f32) -> Option<((u8, u8, u8), f32)> {
    if pic.w == 0 || pic.h == 0 {
        return None;
    }
    let x0 = fx.floor();
    let y0 = fy.floor();
    let tx = fx - x0;
    let ty = fy - y0;
    let cx = |v: f32| (v.max(0.0) as u32).min(pic.w - 1);
    let cy = |v: f32| (v.max(0.0) as u32).min(pic.h - 1);
    let (x0, x1) = (cx(x0), cx(x0 + 1.0));
    let (y0, y1) = (cy(y0), cy(y0 + 1.0));
    let texel = |x: u32, y: u32| -> (f32, f32, f32, f32) {
        let i = ((y * pic.w + x) * 4) as usize;
        match pic.rgba.get(i..i + 4) {
            Some(p) => (p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32 / 255.0),
            None => (0.0, 0.0, 0.0, 0.0),
        }
    };
    let (w00, w10, w01, w11) = (
        (1.0 - tx) * (1.0 - ty),
        tx * (1.0 - ty),
        (1.0 - tx) * ty,
        tx * ty,
    );
    let c = [
        (texel(x0, y0), w00),
        (texel(x1, y0), w10),
        (texel(x0, y1), w01),
        (texel(x1, y1), w11),
    ];
    let alpha: f32 = c.iter().map(|((_, _, _, a), w)| a * w).sum();
    if alpha <= 0.0 {
        return None;
    }
    // Premultiplied on the way in, divided back out on the way to a colour.
    let mut rgb = [0.0f32; 3];
    for ((r, g, b), w) in c.iter().map(|((r, g, b, a), w)| ((*r, *g, *b), a * w)) {
        rgb[0] += r * w;
        rgb[1] += g * w;
        rgb[2] += b * w;
    }
    let out = (
        (rgb[0] / alpha).round().clamp(0.0, 255.0) as u8,
        (rgb[1] / alpha).round().clamp(0.0, 255.0) as u8,
        (rgb[2] / alpha).round().clamp(0.0, 255.0) as u8,
    );
    Some((out, alpha.clamp(0.0, 1.0)))
}

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
        let caret = self.cursor_visible.then_some(self.cursor);
        if self.all_dirty {
            dirty.extend(0..self.cells.len());
        } else {
            for (i, (now, was)) in self.cells.iter().zip(self.painted.iter()).enumerate() {
                if now != was {
                    dirty.push(i);
                }
            }
            // Where the caret is, and where it was. Neither cell changed, and
            // both of them look different because of it.
            if caret != self.painted_cursor {
                for at in [caret, self.painted_cursor].into_iter().flatten() {
                    if at.x < self.cols && at.y < self.rows {
                        dirty.push(at.y as usize * self.cols as usize + at.x as usize);
                    }
                }
            }
            if self.frame.len() != self.drawn.len()
                || self.frame.iter().zip(self.drawn.iter()).any(|(a, b)| {
                    // …including its *height*, which was left out — so a
                    // preview replaced by a shorter picture kept the bottom of
                    // the old one, because the cells under it were never
                    // marked for repainting.
                    a.id != b.id
                        || a.x != b.x
                        || a.y != b.y
                        || a.w != b.w
                        || a.h != b.h
                        || a.alpha != b.alpha
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
        // `CIAN_SOFT_DEBUG=1`, at most once a second. Per frame it is sixty
        // lines a second of nearly identical text, which is not a diagnostic,
        // it is a way to lose one.
        if std::env::var_os("CIAN_SOFT_DEBUG").is_some()
            && self.said.map(|t| t.elapsed() >= std::time::Duration::from_secs(1)).unwrap_or(true)
        {
            self.said = Some(std::time::Instant::now());
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
        self.painted_cursor = caret;
        self.all_dirty = false;

        if let Ok(mut buf) = self.surface.buffer_mut() {
            let n = buf.len().min(self.pixels.len());
            buf[..n].copy_from_slice(&self.pixels[..n]);
            let _ = buf.present();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character cian frames a pane with, plus the ones the bars are
    /// made of. If one of these falls back to the font, it is drawn as a letter
    /// — centred, short of the edges, and wandering by a pixel per row.
    const DRAWN: &[char] = &[
        // Plain and rounded borders — the two `resolve_border_type` can pick.
        '─', '│', '┌', '┐', '└', '┘', '╭', '╮', '╰', '╯',
        // Tees and crossings, for a frame with a divider in it.
        '├', '┤', '┬', '┴', '┼',
        // The bars: a solid thumb, a hairline track, a gutter, and the
        // half-block the picture preview is made of.
        '█', '▏', '▌', '▐', '▀', '▄', '░', '▒', '▓',
    ];

    #[test]
    fn everything_cian_draws_a_frame_with_is_drawn_rather_than_typeset() {
        for &c in DRAWN {
            assert!(cell_art(c).is_some(), "{c:?} would fall back to the font");
        }
    }

    #[test]
    fn a_letter_is_still_a_letter() {
        for c in ['a', 'Z', '0', '?', 'あ', '漢', '☂'] {
            assert!(cell_art(c).is_none(), "{c:?} should be typeset");
        }
    }

    /// A line has to reach both edges of its cell, or the next cell's line
    /// starts after this one stopped and the border comes out dashed.
    #[test]
    fn a_line_reaches_the_edges_of_its_cell() {
        let across = cell_art('─').unwrap();
        assert!(across.iter().flatten().any(|p| p.l == 0.0), "reaches the left edge");
        assert!(across.iter().flatten().any(|p| p.r == 1.0), "reaches the right edge");
        let down = cell_art('│').unwrap();
        assert!(down.iter().flatten().any(|p| p.t == 0.0), "reaches the top");
        assert!(down.iter().flatten().any(|p| p.b == 1.0), "reaches the bottom");
    }

    /// …and a corner reaches exactly the two edges it turns between, so it
    /// meets its neighbours and does not stick out into the ones it does not.
    #[test]
    fn a_corner_reaches_two_edges_and_no_others() {
        // ╭ turns from the right edge down to the bottom.
        let parts: Vec<Part> = cell_art('╭').unwrap().into_iter().flatten().collect();
        assert!(parts.iter().any(|p| p.r == 1.0), "joins the line to its right");
        assert!(parts.iter().any(|p| p.b == 1.0), "joins the line below it");
        assert!(!parts.iter().any(|p| p.l == 0.0), "nothing to its left");
        assert!(!parts.iter().any(|p| p.t == 0.0), "nothing above it");
    }

    #[test]
    fn the_crossing_has_all_four_arms() {
        assert_eq!(cell_art('┼').unwrap().iter().flatten().count(), 4);
    }

    #[test]
    fn a_full_block_fills_its_cell_exactly() {
        let parts: Vec<Part> = cell_art('█').unwrap().into_iter().flatten().collect();
        assert_eq!(parts.len(), 1);
        let p = parts[0];
        assert_eq!((p.l, p.t, p.r, p.b, p.ink), (0.0, 0.0, 1.0, 1.0, 1.0));
    }

    #[test]
    fn the_halves_and_eighths_are_the_fractions_they_are_named_after() {
        let only = |c: char| cell_art(c).unwrap().into_iter().flatten().next().unwrap();
        assert_eq!(only('▀').b, 0.5, "the top half stops halfway down");
        assert_eq!(only('▄').t, 0.5, "the bottom half starts halfway down");
        assert_eq!(only('▌').r, 0.5, "the left half stops halfway across");
        assert_eq!(only('▏').r, 0.125, "one eighth");
        assert_eq!(only('▄').b, 1.0, "and reaches the bottom");
        // ▁ is one eighth up from the bottom, ▇ is seven.
        assert!((only('▁').t - 0.875).abs() < 1e-6, "{}", only('▁').t);
        assert!((only('▇').t - 0.125).abs() < 1e-6, "{}", only('▇').t);
    }

    #[test]
    fn the_shades_are_the_whole_cell_at_less_than_full_ink() {
        for (c, want) in [('░', 0.25), ('▒', 0.5), ('▓', 0.75)] {
            let p = cell_art(c).unwrap().into_iter().flatten().next().unwrap();
            assert_eq!((p.l, p.t, p.r, p.b), (0.0, 0.0, 1.0, 1.0), "{c:?} covers the cell");
            assert_eq!(p.ink, want, "{c:?} is {want} ink");
        }
    }

    /// The two line weights are different, and both are thin enough to be a
    /// line rather than a bar.
    #[test]
    fn a_heavy_line_is_thicker_than_a_light_one() {
        let width = |c: char| {
            let p = cell_art(c).unwrap().into_iter().flatten().next().unwrap();
            p.b - p.t
        };
        assert!(width('━') > width('─'), "heavier");
        assert!(width('─') > 0.0 && width('━') < 0.5, "still a line");
    }
}

#[cfg(test)]
mod picture_tests {
    use super::*;

    fn checker(w: u32, h: u32) -> Picture {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let on = (x + y) % 2 == 0;
                let v = if on { 255 } else { 0 };
                rgba.extend_from_slice(&[v, v, v, 255]);
            }
        }
        Picture { w, h, rgba }
    }

    #[test]
    fn between_two_texels_is_between_two_colours() {
        let pic = checker(2, 1);
        // Dead centre of the pair: half of each, which nearest neighbour can
        // never produce and which is the whole difference in a scaled-up icon.
        let ((r, _, _), a) = sample(&pic, 0.5, 0.0).unwrap();
        assert!((100..=155).contains(&r), "blended, not snapped: {r}");
        assert_eq!(a, 1.0);
    }

    #[test]
    fn on_a_texel_is_that_texel() {
        let pic = checker(2, 1);
        assert_eq!(sample(&pic, 0.0, 0.0).unwrap().0, (255, 255, 255));
        assert_eq!(sample(&pic, 1.0, 0.0).unwrap().0, (0, 0, 0));
    }

    #[test]
    fn outside_the_picture_clamps_rather_than_wraps() {
        let pic = checker(2, 1);
        assert_eq!(sample(&pic, -3.0, -3.0).unwrap().0, (255, 255, 255));
        assert_eq!(sample(&pic, 9.0, 9.0).unwrap().0, (0, 0, 0));
    }

    /// A transparent border must not drag its colour into the edge — that is
    /// where the dark halo round a scaled icon comes from.
    #[test]
    fn a_transparent_neighbour_does_not_tint_the_edge() {
        let pic = Picture {
            w: 2,
            h: 1,
            // A white pixel beside a fully transparent black one.
            rgba: vec![255, 255, 255, 255, 0, 0, 0, 0],
        };
        let ((r, g, b), a) = sample(&pic, 0.5, 0.0).unwrap();
        assert_eq!((r, g, b), (255, 255, 255), "the colour stays white");
        assert!((a - 0.5).abs() < 0.01, "and it is the alpha that fades: {a}");
    }

    #[test]
    fn a_wholly_transparent_spot_is_not_drawn_at_all() {
        let pic = Picture { w: 1, h: 1, rgba: vec![9, 9, 9, 0] };
        assert!(sample(&pic, 0.0, 0.0).is_none());
    }
}
