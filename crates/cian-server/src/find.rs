//! The file finder: everything under here, ranked as you type.
//!
//! **The walk starts before the picker has anything in it.** The terminal
//! build got this wrong once — it read the whole tree on the main loop before
//! drawing a single row, so opening the finder on a deep tree or a network
//! drive was a freeze with nothing on screen to say why. That is a mistake
//! worth not making twice, so this is a worker from the first line.
//!
//! Ranking stays here rather than in the front end. A fuzzy matcher is a pile
//! of small judgements — a run of letters beats scattered ones, a match at a
//! word boundary beats one in the middle — and two implementations of it drift
//! apart within a week. One, in Rust, answered over the pipe: it is a local
//! pipe, and a keystroke's round trip costs less than the ranking itself.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::wire::Out;

/// Where the walk gives up.
///
/// Not there to keep anything responsive — the walk is off the main loop — but
/// ranking every row against every keystroke does cost something, and a tree
/// this size is one to narrow rather than to scroll.
const CAP: usize = 50_000;

/// How many are sent back at a time. Small enough that the first batch lands
/// while the picker is still opening.
const BATCH: usize = 512;

/// One row of the finder.
#[derive(Clone)]
pub struct Hit {
    /// Shown and matched against: the path relative to where the walk began.
    pub rel: String,
    pub full: PathBuf,
    pub is_dir: bool,
}

/// A walk, and what it has found so far.
#[derive(Clone, Default)]
pub struct Find {
    hits: Arc<Mutex<Vec<Hit>>>,
    cancel: Arc<AtomicBool>,
}

impl Find {
    /// Begin again under `root`. Any walk already running is called off — a
    /// second `//` means the first one's answers are not wanted.
    pub fn start(&mut self, root: PathBuf, out: Out) {
        self.cancel.store(true, Ordering::Relaxed);
        *self = Find {
            hits: Arc::new(Mutex::new(Vec::new())),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let hits = Arc::clone(&self.hits);
        let cancel = Arc::clone(&self.cancel);

        std::thread::spawn(move || {
            let mut batch: Vec<Hit> = Vec::with_capacity(BATCH);
            let mut count = 0usize;
            let mut stopped_early = false;

            walk(&root, &mut |hit| {
                if cancel.load(Ordering::Relaxed) {
                    return false;
                }
                batch.push(hit);
                count += 1;
                if batch.len() >= BATCH {
                    hits.lock().unwrap().extend(batch.drain(..));
                    out.event("finding", serde_json::json!({ "found": count }));
                }
                if count >= CAP {
                    stopped_early = true;
                    return false;
                }
                true
            });

            if !batch.is_empty() {
                hits.lock().unwrap().extend(batch.drain(..));
            }
            if !cancel.load(Ordering::Relaxed) {
                out.event(
                    "found",
                    serde_json::json!({ "total": count, "capped": stopped_early }),
                );
            }
        });
    }

    /// The best `limit` of what has been found, for this query.
    ///
    /// Answered against whatever the walk has reached, so an early keystroke
    /// ranks a partial tree and the next one ranks more of it. That is the
    /// right behaviour: the alternative is a picker that will not answer until
    /// the disk has.
    pub fn rank(&self, query: &str, limit: usize) -> Vec<Hit> {
        let hits = self.hits.lock().unwrap();
        if query.is_empty() {
            return hits.iter().take(limit).cloned().collect();
        }
        let order = cian_core::fuzzy::rank(query, hits.iter().map(|h| h.rel.as_str()));
        order.into_iter().take(limit).filter_map(|i| hits.get(i).cloned()).collect()
    }

    pub fn found(&self) -> usize {
        self.hits.lock().unwrap().len()
    }
}

/// Walk from `root` outwards, handing each entry to `cb`, until `cb` says stop.
///
/// **Breadth first, not depth first.** The cap has to fall somewhere, and the
/// depth-first version spent all fifty thousand of it inside `target/` before
/// it ever reached `gui/` — so searching a source tree for "renderer" returned
/// build fingerprints and not the file. Nothing was wrong with the ranking;
/// the file simply had not been looked at yet.
///
/// Going outwards in rings fixes it without a list of directories to ignore,
/// which would have to be guessed per project and would be wrong for the next
/// one. What is near is found first, what is deep is what gets cut, and near
/// is a good guess at what was meant.
fn walk(root: &Path, cb: &mut dyn FnMut(Hit) -> bool) {
    let mut ring: Vec<PathBuf> = vec![root.to_path_buf()];
    while !ring.is_empty() {
        let mut next: Vec<PathBuf> = Vec::new();
        for dir in ring {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for entry in rd.flatten() {
                let path = entry.path();
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                // Nobody is looking for a file inside `.git`, and under a
                // repository it is most of what there is.
                if is_dir && matches!(entry.file_name().to_str(), Some(".git" | ".svn" | ".hg")) {
                    continue;
                }
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                if !cb(Hit { rel, full: path.clone(), is_dir }) {
                    return;
                }
                if is_dir {
                    next.push(path);
                }
            }
        }
        ring = next;
    }
}
