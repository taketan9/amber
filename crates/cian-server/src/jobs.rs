//! Work that outlives the call that asked for it.
//!
//! Copying four thousand files takes long enough that the front end must stay
//! answerable while it runs — the cursor still moves, the other pane still
//! reads. So the call returns an operation number at once and the work goes to
//! a thread, which reports against that number until it is finished.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use cian_core::ops::{self, Conflict, DeleteMode};

use crate::undo::{Stack, Undo};
use crate::wire::Out;

/// How often a running operation is allowed to speak.
///
/// A copy of ten thousand files that reported each one would put ten thousand
/// lines through the pipe for a bar that moves in pixels. Once every this many
/// milliseconds is enough to look continuous and cheap enough to ignore.
const REPORT_EVERY_MS: u128 = 80;

/// What a running operation is doing, for the front end's benefit.
#[derive(Clone, Copy)]
pub enum Kind {
    Copy,
    Move,
    Delete,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Copy => "copy",
            Kind::Move => "move",
            Kind::Delete => "delete",
        }
    }
}

/// The operations in flight, so they can be numbered and called off.
#[derive(Default)]
pub struct Jobs {
    next: AtomicU64,
    running: std::sync::Mutex<Vec<(u64, Arc<AtomicBool>)>>,
}

impl Jobs {
    /// Start one. Returns the number it will report under, immediately —
    /// before any file has been touched.
    pub fn start(
        &self,
        kind: Kind,
        paths: Vec<PathBuf>,
        dest: Option<PathBuf>,
        out: Out,
        undo: Stack,
    ) -> u64 {
        let op = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.running.lock().unwrap().push((op, Arc::clone(&cancel)));

        std::thread::spawn(move || {
            let total = paths.len();
            out.event(
                "started",
                serde_json::json!({ "op": op, "kind": kind.name(), "total": total }),
            );

            let mut ok = 0usize;
            let mut skipped = 0usize;
            // Where each moved thing ended up, and where it came from. Only a
            // move records this: a copy is not undone (undoing one deletes
            // files that now exist) and a delete went to the trash, which has
            // its own way back.
            let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
            let mut errors: Vec<String> = Vec::new();
            let mut last = std::time::Instant::now();
            let began = std::time::Instant::now();

            // One at a time rather than through `copy_many`, which cannot say
            // where it has got to. The per-file calls are the same ones it
            // makes; what is added here is the counting and the way out.
            for (done, path) in paths.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    out.event(
                        "done",
                        serde_json::json!({
                            "op": op, "ok": ok, "skipped": skipped,
                            "errors": errors, "cancelled": true,
                        }),
                    );
                    return;
                }
                let result = match (kind, dest.as_ref()) {
                    (Kind::Copy, Some(d)) => ops::copy_one(path, d, Conflict::Overwrite),
                    (Kind::Move, Some(d)) => ops::move_one(path, d, Conflict::Overwrite),
                    // Trash, never unlink. `d` is one keystroke from destroying
                    // work, and this front end has no undo yet at all.
                    (Kind::Delete, _) => {
                        ops::delete_one(path, DeleteMode::Trash).map(|()| true)
                    }
                    // A copy or move with nowhere to go. Caught before starting,
                    // so this is only here to make the match total.
                    _ => Err(anyhow::anyhow!("no destination")),
                };
                match result {
                    Ok(true) => {
                        ok += 1;
                        if let (Kind::Move, Some(d)) = (kind, dest.as_ref()) {
                            if let Some(name) = path.file_name() {
                                moved.push((d.join(name), path.clone()));
                            }
                        }
                    }
                    Ok(false) => skipped += 1,
                    Err(e) => errors.push(format!("{}: {}", path.display(), e)),
                }
                // Throttled, and always truthful about which file it is on.
                if last.elapsed().as_millis() >= REPORT_EVERY_MS {
                    last = std::time::Instant::now();
                    out.event(
                        "progress",
                        serde_json::json!({
                            "op": op, "done": done + 1, "total": total,
                            "path": path.display().to_string(),
                        }),
                    );
                }
            }
            if !moved.is_empty() {
                undo.push(Undo::Moved { pairs: moved });
            }
            out.event(
                "done",
                serde_json::json!({
                    "op": op, "ok": ok, "skipped": skipped,
                    "errors": errors, "cancelled": false,
                    // How long it took, so the front end can say so and so
                    // that "no progress was reported" can be told apart from
                    // "it was over before there was anything to report".
                    "ms": began.elapsed().as_millis() as u64,
                }),
            );
        });
        op
    }

    /// Ask an operation to stop. It stops between files, never inside one —
    /// a half-copied file is worse than a slow cancel.
    pub fn cancel(&self, op: u64) -> bool {
        let running = self.running.lock().unwrap();
        match running.iter().find(|(n, _)| *n == op) {
            Some((_, flag)) => {
                flag.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }
}
