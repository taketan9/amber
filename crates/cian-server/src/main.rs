//! The engine, spoken over a pipe.
//!
//! cian's file manager lives in [`cian_core`] and knows nothing about how it is
//! drawn — the terminal front end proves that, and this is the second caller.
//! One JSON object per line in, one per line out, over stdin and stdout.
//!
//! **A line each way, not a stream.** The protocol has to be readable in a log
//! and typeable by hand when something is wrong at a customer's desk, and both
//! of those rule out anything framed by byte counts.
//!
//! ```text
//! → {"id":1,"method":"list","params":{"pane":"left","path":"/tmp"}}
//! ← {"id":1,"ok":{"cwd":"/tmp","entries":[…]}}
//! ← {"id":1,"error":"no such directory: /nope"}
//! ```
//!
//! Every reply carries the `id` it answers, so the caller may have several in
//! flight. A long operation also speaks unasked, on lines carrying `event`
//! instead of `id` — see [`wire`].

use std::io::BufRead;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use cian_core::Pane;

mod find;
mod jobs;
mod undo;
mod wire;

use find::Find;
use jobs::{Jobs, Kind};
use undo::{Stack, Undo};
use wire::Out;

/// One call from the front end.
#[derive(Deserialize)]
struct Request {
    id: u64,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

/// What a pane looks like to whoever is drawing it.
///
/// Deliberately not `cian_core::Pane` itself: that carries the undo stacks, the
/// history and the marks as a set, and a front end needs none of it to paint a
/// listing. Sending the whole thing would make every field of the engine part
/// of the protocol.
#[derive(Serialize)]
struct PaneView {
    cwd: String,
    entries: Vec<Row>,
    cursor: usize,
    /// How many rows are marked. The front end could count them, but this is
    /// the number it puts on the status line and counting is the engine's job.
    marked: usize,
    /// Whether dotfiles are showing. The switches menu puts the current value
    /// beside the name, so it has to come from the engine rather than from
    /// whatever the front end last remembered asking for.
    hidden_shown: bool,
}

/// One line of a listing.
#[derive(Serialize)]
struct Row {
    name: String,
    path: String,
    is_dir: bool,
    len: u64,
    /// Seconds since the epoch, or `null` where the filesystem has no opinion.
    modified: Option<u64>,
    /// Listed but not downloaded — reading it would pull it over the network.
    cloud: bool,
    /// The synthetic `..` row: navigable, never a target.
    parent: bool,
    marked: bool,
}

impl PaneView {
    fn of(pane: &Pane) -> Self {
        PaneView {
            cwd: pane.cwd.display().to_string(),
            cursor: pane.cursor,
            marked: pane.mark_count(),
            hidden_shown: pane.show_hidden,
            entries: pane
                .entries
                .iter()
                .map(|e| Row {
                    name: e.name.clone(),
                    path: e.path.display().to_string(),
                    is_dir: e.is_dir,
                    len: e.len,
                    modified: e.modified.and_then(|t| {
                        t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
                    }),
                    cloud: e.cloud,
                    parent: e.is_parent,
                    marked: pane.marks.contains(&e.path),
                })
                .collect(),
        }
    }
}

/// The two panes and whatever is running over them.
struct Session {
    left: Pane,
    right: Pane,
    jobs: Jobs,
    out: Out,
    undo: Stack,
    find: Find,
    /// Held for a later paste. Independent of the system clipboard, and of
    /// which pane is focused — that is the point of it.
    clip: Option<cian_core::clip::Clipboard>,
}

impl Session {
    fn new(dir: std::path::PathBuf, out: Out) -> anyhow::Result<Self> {
        Ok(Session {
            left: Pane::new(dir.clone())?,
            right: Pane::new(dir)?,
            jobs: Jobs::default(),
            out,
            undo: Stack::default(),
            find: Find::default(),
            clip: None,
        })
    }

    /// The paths an operation acts on: the marked rows, or the one under the
    /// cursor when nothing is marked. Never the `..` row — it is navigable but
    /// is not a thing to copy.
    fn targets(&self, which: &str) -> anyhow::Result<Vec<std::path::PathBuf>> {
        let pane = match which {
            "left" => &self.left,
            "right" => &self.right,
            other => anyhow::bail!("no such pane: {other}"),
        };
        let marked: Vec<_> = pane
            .entries
            .iter()
            .filter(|e| !e.is_parent && pane.marks.contains(&e.path))
            .map(|e| e.path.clone())
            .collect();
        if !marked.is_empty() {
            return Ok(marked);
        }
        match pane.entries.get(pane.cursor).filter(|e| !e.is_parent) {
            Some(e) => Ok(vec![e.path.clone()]),
            None => Ok(Vec::new()),
        }
    }

    /// Where a transfer goes: the directory the other pane is showing. Two
    /// panes side by side, and you copy between them — that is the whole idea,
    /// and it is why the destination never has to be typed.
    fn other_cwd(&self, which: &str) -> std::path::PathBuf {
        match which {
            "left" => self.right.cwd.clone(),
            _ => self.left.cwd.clone(),
        }
    }

    fn pane_mut(&mut self, which: &str) -> anyhow::Result<&mut Pane> {
        match which {
            "left" => Ok(&mut self.left),
            "right" => Ok(&mut self.right),
            other => Err(anyhow::anyhow!("no such pane: {other}")),
        }
    }

    /// Answer one call. The error is a string because it is going to a person,
    /// through a dialog, not to code that will match on it.
    fn handle(&mut self, req: &Request) -> anyhow::Result<serde_json::Value> {
        match req.method.as_str() {
            // Both panes as they stand. What the front end asks for on startup
            // and after anything that could have changed the world.
            "state" => Ok(serde_json::json!({
                "left": PaneView::of(&self.left),
                "right": PaneView::of(&self.right),
            })),
            // Read a directory into a pane.
            "list" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let path = req.params["path"].as_str().map(std::path::PathBuf::from);
                let pane = self.pane_mut(&which)?;
                if let Some(p) = path {
                    if !p.is_dir() {
                        anyhow::bail!("not a directory: {}", p.display());
                    }
                    *pane = Pane::new(p)?;
                } else {
                    pane.reload()?;
                }
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            // Step into whatever the cursor is on, or out to the parent.
            "enter" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let at = req.params["cursor"].as_u64().map(|n| n as usize);
                let pane = self.pane_mut(&which)?;
                if let Some(n) = at {
                    pane.cursor = n.min(pane.entries.len().saturating_sub(1));
                }
                let was = pane.cwd.clone();
                pane.enter_selected()?;
                // Only if it actually went somewhere: `Enter` on a file will
                // one day open it, and that is not a step to walk back.
                if pane.cwd != was {
                    self.undo.push(Undo::Navigated { pane: which.clone(), from: was });
                }
                let pane = self.pane_mut(&which)?;
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            "parent" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                let was = pane.cwd.clone();
                pane.go_parent()?;
                if pane.cwd != was {
                    self.undo.push(Undo::Navigated { pane: which.clone(), from: was });
                }
                let pane = self.pane_mut(&which)?;
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            // Marking. `at` is a row; without it the cursor's row is meant.
            "mark" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let at = req.params["at"].as_u64().map(|n| n as usize);
                let pane = self.pane_mut(&which)?;
                let row = at.unwrap_or(pane.cursor);
                pane.toggle_mark_at(row);
                // Marking walks down the list, the way it does everywhere else:
                // one keystroke marks and moves on.
                if at.is_none() && pane.cursor + 1 < pane.entries.len() {
                    pane.cursor += 1;
                }
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            "markall" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                // A second press clears, which is what every "select all" does
                // once everything is already selected.
                if pane.mark_count() > 0 {
                    pane.clear_marks();
                } else {
                    for i in 0..pane.entries.len() {
                        pane.set_mark_at(i);
                    }
                }
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            "invert" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                for i in 0..pane.entries.len() {
                    pane.toggle_mark_at(i);
                }
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            "unmarkall" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                pane.clear_marks();
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            // The operations. Each answers with the number it will report
            // under, before it has touched anything.
            "copy" | "move" | "delete" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let paths = self.targets(&which)?;
                if paths.is_empty() {
                    anyhow::bail!("nothing to operate on");
                }
                let (kind, dest) = match req.method.as_str() {
                    "copy" => (Kind::Copy, Some(self.other_cwd(&which))),
                    "move" => (Kind::Move, Some(self.other_cwd(&which))),
                    _ => (Kind::Delete, None),
                };
                let count = paths.len();
                let op = self.jobs.start(kind, paths, dest, self.out.clone(), self.undo.clone());
                Ok(serde_json::json!({ "op": op, "count": count }))
            }
            // Hold the selection for a later paste, and drop it somewhere
            // else. `c`/`m` go straight to the other pane; this is the other
            // half of the pair, for when the destination is not on screen yet.
            "clip" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let op = match req.params["op"].as_str() {
                    Some("cut") => cian_core::clip::Op::Cut,
                    _ => cian_core::clip::Op::Copy,
                };
                let paths = self.targets(&which)?;
                if paths.is_empty() {
                    anyhow::bail!("対象がありません");
                }
                let count = paths.len();
                self.clip = Some(cian_core::clip::Clipboard { paths, op });
                Ok(serde_json::json!({
                    "held": count,
                    "op": if op == cian_core::clip::Op::Cut { "cut" } else { "copy" },
                }))
            }
            "paste" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let dest = self.pane_mut(&which)?.cwd.clone();
                // The engine has no system clipboard of its own — the window
                // is the only thing here that can see one, and it has not been
                // asked for yet. `plan` takes the fallback as a closure for
                // exactly this: the day it can, it is one argument.
                let (paths, op) =
                    match cian_core::clip::plan(self.clip.as_ref(), Vec::new, &dest) {
                        cian_core::clip::Paste::Empty => {
                            anyhow::bail!("クリップボードは空です")
                        }
                        cian_core::clip::Paste::AlreadyHere => {
                            anyhow::bail!("既にこのディレクトリです")
                        }
                        cian_core::clip::Paste::Go { paths, op, .. } => (paths, op),
                    };
                let kind = if op == cian_core::clip::Op::Cut { Kind::Move } else { Kind::Copy };
                if !cian_core::clip::survives(op) {
                    self.clip = None;
                }
                let count = paths.len();
                let job = self.jobs.start(
                    kind, paths, Some(dest), self.out.clone(), self.undo.clone());
                // Which it is, said back: the key pressed was "paste" either
                // way, and only the register knew whether that meant a copy.
                Ok(serde_json::json!({
                    "op": job,
                    "count": count,
                    "kind": if matches!(kind, Kind::Move) { "move" } else { "copy" },
                }))
            }
            // Rename in place. The name is a bare filename, never a path —
            // moving something is what `move` is for, and a rename that could
            // also move would make one confirm dialog have to explain two
            // things.
            "rename" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let to = req.params["name"].as_str().unwrap_or("").trim().to_string();
                if to.is_empty() {
                    anyhow::bail!("名前が空です");
                }
                if to.contains('/') || to.contains('\\') {
                    anyhow::bail!("名前に区切り文字は使えません: {to}");
                }
                let pane = self.pane_mut(&which)?;
                let Some(entry) = pane.entries.get(pane.cursor).filter(|e| !e.is_parent) else {
                    anyhow::bail!("対象がありません");
                };
                let from = entry.path.clone();
                let dest = from.with_file_name(&to);
                if dest.exists() {
                    anyhow::bail!("{to} はすでにあります");
                }
                cian_core::ops::rename_in_place(&from, &to)?;
                self.undo.push(Undo::Rename { from: from.clone(), to: dest });
                let pane = self.pane_mut(&which)?;
                pane.reload()?;
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            // A new file or a new directory, in the pane being looked at.
            "create" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let name = req.params["name"].as_str().unwrap_or("").trim().to_string();
                let dir = req.params["dir"].as_bool().unwrap_or(false);
                if name.is_empty() {
                    anyhow::bail!("名前が空です");
                }
                let pane = self.pane_mut(&which)?;
                let at = pane.cwd.clone();
                let made = if dir {
                    cian_core::ops::create_dir(&at, &name)?
                } else {
                    cian_core::ops::create_file(&at, &name)?
                };
                self.undo.push(Undo::Created { path: made });
                let pane = self.pane_mut(&which)?;
                pane.reload()?;
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            // One step back, whatever it was.
            "undo" => {
                let Some(step) = self.undo.pop() else {
                    anyhow::bail!("取り消せる操作はありません");
                };
                let said = step.describe();
                match &step {
                    Undo::Rename { from, to } => {
                        let name = from
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        cian_core::ops::rename_in_place(to, &name)?;
                    }
                    Undo::Created { path } => {
                        // Straight off the disk rather than into the trash. It
                        // was made a moment ago and never had anything in it;
                        // putting it in the bin would leave litter to explain.
                        if path.is_dir() {
                            std::fs::remove_dir(path)?;
                        } else {
                            std::fs::remove_file(path)?;
                        }
                    }
                    Undo::Moved { pairs } => {
                        for (now, was) in pairs {
                            if let Some(parent) = was.parent() {
                                cian_core::ops::move_one(
                                    now,
                                    parent,
                                    cian_core::ops::Conflict::Skip,
                                )?;
                            }
                        }
                    }
                    Undo::Navigated { pane, from } => {
                        let p = self.pane_mut(pane)?;
                        *p = Pane::new(from.clone())?;
                    }
                }
                self.left.reload()?;
                self.right.reload()?;
                Ok(serde_json::json!({
                    "said": said,
                    "left": PaneView::of(&self.left),
                    "right": PaneView::of(&self.right),
                }))
            }
            // Narrow the listing to names containing this. Case-insensitive,
            // and it scopes everything downstream — marks, operations, the
            // count on the status line — because they all work off what is
            // shown rather than off what is there.
            "filter" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let text = req.params["text"].as_str().unwrap_or("").to_string();
                let pane = self.pane_mut(&which)?;
                if text.is_empty() {
                    pane.clear_filter();
                } else {
                    pane.set_filter(text);
                }
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            "hidden" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                let now = !pane.show_hidden;
                pane.set_show_hidden(now);
                Ok(serde_json::json!({
                    "pane": PaneView::of(pane),
                    "showing": now,
                }))
            }
            "sort" => {
                use cian_core::{Sort, SortKey};
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let key = match req.params["key"].as_str().unwrap_or("name") {
                    "size" => SortKey::Size,
                    "date" | "modified" => SortKey::Modified,
                    "ext" | "extension" => SortKey::Extension,
                    _ => SortKey::Name,
                };
                let pane = self.pane_mut(&which)?;
                // The same key twice turns it round, which is what a column
                // heading does everywhere and what the hand expects.
                let reverse = pane.sort.key == key && !pane.sort.reverse;
                pane.set_sort(Sort { key, reverse });
                Ok(serde_json::json!({
                    "pane": PaneView::of(pane),
                    "by": key.label(),
                    "reverse": reverse,
                }))
            }
            // Everything under the pane's directory, walked on a worker.
            // The picker opens on nothing and fills in; it never waits.
            "find" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let root = match which.as_str() {
                    "left" => self.left.cwd.clone(),
                    _ => self.right.cwd.clone(),
                };
                self.find.start(root.clone(), self.out.clone());
                Ok(serde_json::json!({ "root": root.display().to_string() }))
            }
            // The best of what has been found so far, for what has been typed
            // so far. Ranked here so there is one fuzzy matcher and not two.
            "rank" => {
                let query = req.params["query"].as_str().unwrap_or("");
                let limit = req.params["limit"].as_u64().unwrap_or(200) as usize;
                let rows: Vec<_> = self
                    .find
                    .rank(query, limit)
                    .into_iter()
                    .map(|h| {
                        serde_json::json!({
                            "rel": h.rel,
                            "path": h.full.display().to_string(),
                            "is_dir": h.is_dir,
                        })
                    })
                    .collect();
                Ok(serde_json::json!({ "rows": rows, "of": self.find.found() }))
            }
            // Take a pane to a found path — into it if it is a directory, to
            // its folder with the cursor on it if it is a file.
            "reveal" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let path = PathBuf::from(req.params["path"].as_str().unwrap_or(""));
                let (dir, name) = if path.is_dir() {
                    (path.clone(), None)
                } else {
                    (
                        path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
                        path.file_name().map(|s| s.to_string_lossy().into_owned()),
                    )
                };
                let pane = self.pane_mut(&which)?;
                let was = pane.cwd.clone();
                *pane = Pane::new(dir)?;
                if let Some(n) = name {
                    if let Some(i) = pane.entries.iter().position(|e| e.name == n) {
                        pane.cursor = i;
                    }
                }
                if pane.cwd != was {
                    self.undo.push(Undo::Navigated { pane: which.clone(), from: was });
                }
                let pane = self.pane_mut(&which)?;
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            "cancel" => {
                let op = req.params["op"].as_u64().unwrap_or(0);
                Ok(serde_json::json!({ "stopping": self.jobs.cancel(op) }))
            }
            other => Err(anyhow::anyhow!("no such method: {other}")),
        }
    }
}

fn main() -> anyhow::Result<()> {
    let start = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    let out = Out::start();
    let mut session = Session::new(start, out.clone())?;

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        // A line that is not a request at all still gets an answer, because a
        // front end waiting on an id it will never be sent is the worst way
        // for this to go wrong.
        match serde_json::from_str::<Request>(&line) {
            Ok(req) => match session.handle(&req) {
                Ok(ok) => out.reply(req.id, ok),
                Err(e) => out.fail(req.id, e),
            },
            Err(e) => out.send(serde_json::json!({
                "id": serde_json::Value::Null,
                "error": format!("bad request: {e}"),
            })),
        }
    }
    Ok(())
}
