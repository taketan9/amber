//! Asking the system for an icon, off the thread that draws.
//!
//! The lookup is one call, and on this Mac it takes twenty-five microseconds.
//! That number is why it sat in the middle of the frame for so long, and it is
//! the wrong number: it was measured on a local disk, on the machine cian is
//! written on, and every listing that matters to the person reporting the
//! window as slow lives in OneDrive. `SHGetFileInfoW` on a file the sync engine
//! owns is not a lookup, it is a conversation — and six of those per frame,
//! which is what a listing being scrolled through asks for, is a loop that
//! falls behind the keyboard. That is what "the cursor keeps moving after I let
//! go" is: not a slow frame, but key repeats queuing behind one.
//!
//! Microsoft's own guidance on `SHGetFileInfo` says to call it from a
//! background thread for exactly this reason.
//!
//! So the frame never asks the system anything. It asks *here*, and here either
//! has the answer already or will have it in a moment; the row draws cian's own
//! glyph until then, and the picture arrives on a later frame. Nothing waits.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

use crate::sysicon::Icon;

/// What to ask the system about.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Ask {
    /// This file, whose own icon may differ from every other file's.
    Path(PathBuf),
    /// Anything with this extension — one answer for a directory of a hundred.
    Kind(String),
    /// A directory, which the system answers for by kind.
    Directory,
    /// The picture *in* this file, decoded to fit a box this many pixels
    /// across and down. Not an icon at all — the image popup's own content —
    /// but it belongs on this thread for the same reason the icons do: a
    /// photograph takes tens of milliseconds to decode and scale, which is
    /// several frames, and the frame must not wait for it.
    Picture { path: PathBuf, w: u32, h: u32 },
}

struct Request {
    ask: Ask,
    px: u32,
}

/// A finished lookup: what was asked, and what came back.
pub struct Answer {
    pub ask: Ask,
    pub icon: Option<Icon>,
}

/// The worker, and the queue of things it has been asked.
pub struct Icons {
    tx: Sender<Request>,
    rx: Receiver<Answer>,
    /// What has been asked for and not yet answered, so the same file is not
    /// queued once per frame for as long as it is on screen.
    asked: HashSet<Ask>,
}

impl Icons {
    pub fn start() -> Self {
        let (tx, requests) = std::sync::mpsc::channel::<Request>();
        let (answers, rx) = std::sync::mpsc::channel::<Answer>();
        std::thread::spawn(move || {
            // Ends when the window does: the channel closes and the loop with
            // it. Nothing is joined, because nothing here needs finishing.
            while let Ok(req) = requests.recv() {
                let icon = match &req.ask {
                    Ask::Path(p) => crate::sysicon::icon_for(p, req.px),
                    Ask::Kind(ext) => crate::sysicon::icon_for_type(ext, false, req.px),
                    Ask::Directory => crate::sysicon::icon_for_type("", true, req.px),
                    Ask::Picture { path, w, h } => crate::picture::decode(path, *w, *h),
                };
                if answers.send(Answer { ask: req.ask, icon }).is_err() {
                    break;
                }
            }
        });
        Self { tx, rx, asked: HashSet::new() }
    }

    /// Ask for one, unless it has already been asked for.
    ///
    /// Returns immediately, always. There is no version of this that waits.
    pub fn want(&mut self, ask: Ask, px: u32) {
        if !self.asked.insert(ask.clone()) {
            return;
        }
        let _ = self.tx.send(Request { ask, px });
    }

    /// Everything that has come back since this was last asked.
    pub fn collect(&mut self) -> Vec<Answer> {
        let mut out = Vec::new();
        while let Ok(answer) = self.rx.try_recv() {
            self.asked.remove(&answer.ask);
            out.push(answer);
        }
        out
    }

    /// Is anything still out with the worker? The loop keeps painting while
    /// there is, so icons appear as they land rather than at the next keypress.
    pub fn waiting(&self) -> bool {
        !self.asked.is_empty()
    }
}
