//! Walking the last few things back.
//!
//! The shape is the terminal build's, arrived at again rather than invented:
//! four things are undoable, and two deliberately are not.
//!
//! **A copy is not undone**, because undoing one means deleting files that now
//! exist, and a key that sometimes deletes is not a key anyone can trust.
//! **A delete is not undone either** — it went to the trash, which is the
//! system's own undo and already has a window for it.
//!
//! Where you *are* is on the same stack as what you did, in the order things
//! happened. Walking into the wrong folder is the commonest thing to want
//! back, and keeping it on a second stack meant `u` did not cover it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// One step back.
#[derive(Debug, Clone)]
pub enum Undo {
    /// Rename `to` back to `from`.
    Rename { from: PathBuf, to: PathBuf },
    /// Remove what was just made. **Not redoable** — undoing it destroys it,
    /// and nothing here remembers what was inside.
    Created { path: PathBuf },
    /// Move each `.0` (where it is now) back to `.1` (where it was).
    Moved { pairs: Vec<(PathBuf, PathBuf)> },
    /// Take this pane back to `from`.
    Navigated { pane: String, from: PathBuf },
}

impl Undo {
    /// What just happened, for the person who pressed the key.
    ///
    /// Naming it is the point: `u` that says "done" leaves you wondering which
    /// of the last three things it took back. And the direction is a parameter
    /// rather than assumed, because Ctrl+R applies the very same step and
    /// saying "戻しました" there would describe the opposite of what happened.
    pub fn describe(&self, undoing: bool) -> String {
        let verb = if undoing { "戻しました" } else { "やり直しました" };
        match self {
            Undo::Rename { from, to } => {
                format!("{} → {} を{}", name_of(to), name_of(from), verb)
            }
            Undo::Created { path } => format!("{} を取り消しました", name_of(path)),
            Undo::Moved { pairs } => format!("{} 件の移動を{}", pairs.len(), verb),
            Undo::Navigated { from, .. } => format!("{} に{}", from.display(), verb),
        }
    }
}

fn name_of(p: &std::path::Path) -> String {
    p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// The stack, shared with whatever thread finishes a move.
///
/// A move reports what it actually shifted only once it is over, and that
/// happens on a worker — so the stack cannot live behind the session's own
/// borrow.
#[derive(Clone, Default)]
pub struct Stack(Arc<Mutex<Vec<Undo>>>);

/// How far back `u` reaches. Deep enough to cover a wrong turn and the two
/// things before it, shallow enough that it is never a substitute for a
/// backup — which is a promise this cannot make.
const DEPTH: usize = 32;

impl Stack {
    pub fn push(&self, step: Undo) {
        let mut v = self.0.lock().unwrap();
        v.push(step);
        if v.len() > DEPTH {
            v.remove(0);
        }
    }

    pub fn pop(&self) -> Option<Undo> {
        self.0.lock().unwrap().pop()
    }
}

/// The other direction.
///
/// Not a second kind of step: what `u` undoes is described by the step it took
/// off the stack, and putting it back is the same description read the other
/// way — a rename swaps its two names, a move swaps its two places. Only the
/// two that cannot be undone at all have nothing to redo, and they never reach
/// here.
impl Undo {
    /// This step inverted, for the redo stack. `None` where undoing it
    /// destroyed what redoing it would need — a created file that has just
    /// been removed cannot be brought back with what is remembered here.
    pub fn inverted(&self) -> Option<Undo> {
        match self {
            Undo::Rename { from, to } => Some(Undo::Rename { from: to.clone(), to: from.clone() }),
            Undo::Moved { pairs } => Some(Undo::Moved {
                pairs: pairs.iter().map(|(now, was)| (was.clone(), now.clone())).collect(),
            }),
            Undo::Navigated { pane, from } => {
                Some(Undo::Navigated { pane: pane.clone(), from: from.clone() })
            }
            Undo::Created { .. } => None,
        }
    }
}
