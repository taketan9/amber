//! Where a frame's time went, in parts.
//!
//! `CIAN_GUI_PROF=1` already says what a frame cost in total. That was enough
//! while the answer was "the rows", and not enough the moment it was not: a
//! Windows build reported sixteen milliseconds a frame against this Mac's half
//! a millisecond, and the only way to tell which *part* had grown was to guess,
//! change something, and ship a build to the one machine that could see it.
//!
//! So the frame is timed in pieces, and the pieces go out with the total. Off
//! unless asked for — the check is one atomic read per phase — and the numbers
//! are per-thread, because drawing happens on one thread and nothing else
//! writes here.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The parts of a frame worth telling apart. Anything not named here lands in
/// the remainder the caller computes from the total.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// The places down the left of the desktop views.
    Sidebar,
    /// Both file listings.
    Panes,
    /// The shell panel, or the preview that replaces it.
    Shell,
}

const N: usize = 3;

static ON: AtomicBool = AtomicBool::new(false);

thread_local! {
    static SPENT: RefCell<[Duration; N]> = const { RefCell::new([Duration::ZERO; N]) };
    /// The worst single visit to each phase since the last report.
    ///
    /// An average is the wrong statistic for a stutter. Profiling paints
    /// continuously, so most frames have nothing to do and cost almost
    /// nothing; the one frame in fifty that re-lays a whole listing, or asks
    /// the shell for six icons that live in OneDrive, is divided by fifty
    /// before anyone sees it. What is felt is that one frame. So it is kept.
    static WORST: RefCell<[Duration; N]> = const { RefCell::new([Duration::ZERO; N]) };
}

/// Start timing frames in parts. Called by a front end that means to report it.
pub fn enable() {
    ON.store(true, Ordering::Relaxed);
}

/// Time `f` as part of `phase`, if anyone is counting.
pub(crate) fn timed<T>(phase: Phase, f: impl FnOnce() -> T) -> T {
    if !ON.load(Ordering::Relaxed) {
        return f();
    }
    let t = Instant::now();
    let out = f();
    let spent = t.elapsed();
    SPENT.with(|s| s.borrow_mut()[phase as usize] += spent);
    WORST.with(|w| {
        let w = &mut w.borrow_mut()[phase as usize];
        if spent > *w {
            *w = spent;
        }
    });
    out
}

/// What the parts have added up to since the last time this was asked, as a
/// line to put next to the total. Empty when nothing is being counted.
pub fn take_report(frames: u32) -> String {
    if !ON.load(Ordering::Relaxed) || frames == 0 {
        return String::new();
    }
    let spent = SPENT.with(|s| std::mem::replace(&mut *s.borrow_mut(), [Duration::ZERO; N]));
    let worst = WORST.with(|w| std::mem::replace(&mut *w.borrow_mut(), [Duration::ZERO; N]));
    let us = |d: Duration| d.as_micros();
    format!(
        " [sidebar {}/{}us, panes {}/{}us, shell {}/{}us — avg/worst]",
        us(spent[Phase::Sidebar as usize] / frames),
        us(worst[Phase::Sidebar as usize]),
        us(spent[Phase::Panes as usize] / frames),
        us(worst[Phase::Panes as usize]),
        us(spent[Phase::Shell as usize] / frames),
        us(worst[Phase::Shell as usize]),
    )
}
