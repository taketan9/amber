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
    /// The label of the flat listing showing here, if one is — a branch view
    /// or a panelized search. The window needs it to know that Esc means
    /// "back to the directory" rather than "nothing to cancel".
    flat: Option<String>,
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
            flat: pane.flat_label().map(str::to_string),
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
    /// The file the viewer has open, as it was read.
    ///
    /// A save writes back through this rather than through anything the front
    /// end says, so it cannot land on another path and cannot lose the
    /// encoding, BOM or line ending the file arrived with. Getting those wrong
    /// turns a one-line edit into a diff on every line — and on a Shift_JIS
    /// log, into a file the tool that wrote it can no longer read.
    open: Option<(std::path::PathBuf, cian_core::grepedit::TextFile)>,
    /// What `u` has taken back, waiting for Ctrl+R.
    ///
    /// Cleared by anything else that pushes onto the undo stack: once you have
    /// done something new, the branch you undid is gone. A redo stack that
    /// survives that puts files back on top of work done since.
    redo: Stack,
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
            open: None,
            redo: Stack::default(),
        })
    }

    /// The paths an operation acts on: the marked rows, or the one under the
    /// cursor when nothing is marked. Never the `..` row — it is navigable but
    /// is not a thing to copy.
    /// Where the front end says its cursor is.
    ///
    /// **The front end owns the cursor.** It moves on every `j` without asking,
    /// because a round trip per keystroke to redraw one highlighted row would
    /// be absurd — but that left the engine's own idea of it only being
    /// updated by `enter` and `mark`. Three presses of `j` and then `r`
    /// renamed whatever had been under the cursor three rows ago.
    ///
    /// So every request that names a pane states the cursor too, and it is
    /// taken here, once, rather than in each of the handlers that consult it.
    fn take_cursor(&mut self, req: &Request) {
        // Both panes, every time. `compare` needs the row under each cursor —
        // `=` is one key and the answer is what the two of them are pointing
        // at — and a request that could only state one of them made that
        // impossible to ask for.
        for which in ["left", "right"] {
            let Some(at) = req.params["cursors"][which].as_u64() else { continue };
            if let Ok(pane) = self.pane_mut(which) {
                // Clamped rather than trusted: the listing can have changed
                // under the front end between its last draw and this request.
                pane.cursor = (at as usize).min(pane.entries.len().saturating_sub(1));
            }
        }
    }

    /// The row under the cursor, which is never `..`.
    ///
    /// Four handlers had written this out, and the parent guard is the whole
    /// point of it: without it `r` renames the directory you are standing in
    /// and `view` tries to read it. One place, so the guard cannot be the one
    /// thing a fifth handler forgets.
    fn selected(&mut self, which: &str) -> anyhow::Result<(std::path::PathBuf, String, bool)> {
        let pane = self.pane_mut(which)?;
        let Some(e) = pane.entries.get(pane.cursor).filter(|e| !e.is_parent) else {
            anyhow::bail!("対象がありません");
        };
        Ok((e.path.clone(), e.name.clone(), e.is_dir))
    }

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
        self.take_cursor(req);
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
            // Hand the file to whatever the desktop opens it with. A
            // directory goes to the other pane instead, which is what the
            // terminal build's Ctrl+Enter does — one key, and the answer
            // depends on what is under the cursor.
            "open" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let other = if which == "left" { "right" } else { "left" };
                let (path, name, is_dir) = self.selected(&which)?;
                if is_dir {
                    let there = self.pane_mut(other)?;
                    *there = Pane::new(path)?;
                    let view = serde_json::to_value(PaneView::of(there))?;
                    return Ok(serde_json::json!({ "pane": other, "view": view, "name": name }));
                }
                cian_core::proc::open_with_desktop(&path)?;
                Ok(serde_json::json!({ "opened": name }))
            }
            // Read a file for the viewer.
            //
            // Decoding is the engine's job, not the window's. A browser reads
            // UTF-8 and nothing else, and half of what this meets on a
            // Japanese Windows machine is Shift_JIS — a log, a batch file,
            // something out of an old tool. Handing over raw bytes would mean
            // writing that detection a second time, in JavaScript.
            "view" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let (path, name, is_dir) = self.selected(&which)?;
                if is_dir {
                    anyhow::bail!("{name} はディレクトリです");
                }
                let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let file = cian_core::grepedit::read_text(&path)?;
                let reply = serde_json::json!({
                    "name": name,
                    "path": path.display().to_string(),
                    "lines": file.lines,
                    "bytes": len,
                    "encoding": format!("{:?}", file.encoding),
                    "eol": format!("{:?}", file.eol),
                    "bom": file.bom,
                    "lang": cian_core::highlight::detect(&path).map(|l| format!("{l:?}")),
                });
                self.open = Some((path, file));
                Ok(reply)
            }
            // Write the open file back, in the encoding it arrived in.
            "save" => {
                let Some((path, original)) = self.open.as_ref() else {
                    anyhow::bail!("開いているファイルがありません");
                };
                let lines: Vec<String> = req.params["lines"]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect())
                    .unwrap_or_default();
                let file = cian_core::grepedit::TextFile { lines, ..original.clone() };
                cian_core::grepedit::write_text(path, &file)?;
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let lines = file.lines.len();
                self.open = Some((path.clone(), file));
                Ok(serde_json::json!({ "saved": name, "lines": lines }))
            }
            // ---- What is here, measured rather than felt ----
            //
            // Every one of these already exists in cian-core, written and
            // tested for the terminal build. The engine's whole job is to let
            // the window ask; none of the answering happens here.
            "count" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let paths = self.targets(&which)?;
                let o = cian_core::count::Options::default();
                let r = cian_core::count::count(&paths, &o);
                Ok(serde_json::json!({
                    "files": r.total.files,
                    "steps": r.total.steps(&o),
                    "lines": r.total.total,
                    "blank": r.total.blank,
                    "comments": r.total.comment,
                    "truncated": r.truncated,
                    "by_ext": r.by_ext.iter().take(20).map(|(e, c)| serde_json::json!({
                        "ext": if e.is_empty() { "(拡張子なし)" } else { e },
                        "files": c.files,
                        "steps": c.steps(&o),
                    })).collect::<Vec<_>>(),
                }))
            }
            "attr" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let (path, name, _) = self.selected(&which)?;
                let a = cian_core::attrs::read_attrs(&path)?;
                Ok(serde_json::json!({
                    "name": name,
                    "path": path.display().to_string(),
                    "mode": a.mode.map(|m| format!("{:o}", m & 0o7777)),
                    "readonly": a.readonly,
                    "owner": a.owner,
                    "size": a.size,
                    "is_dir": a.is_dir,
                }))
            }
            "chmod" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let spec = req.params["spec"].as_str().unwrap_or("").trim().to_string();
                if spec.is_empty() {
                    anyhow::bail!("モードを指定してください（例: 644）");
                }
                let paths = self.targets(&which)?;
                for p in &paths {
                    cian_core::attrs::set_mode(p, &spec)?;
                }
                let pane = self.pane_mut(&which)?;
                pane.reload()?;
                Ok(serde_json::json!({ "changed": paths.len(), "spec": spec }))
            }
            "readonly" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let on = req.params["on"].as_bool().unwrap_or(true);
                let paths = self.targets(&which)?;
                for p in &paths {
                    cian_core::attrs::set_readonly(p, on)?;
                }
                let pane = self.pane_mut(&which)?;
                pane.reload()?;
                Ok(serde_json::json!({ "changed": paths.len(), "on": on }))
            }
            // Checksums. Cancellable because a checksum of something large is
            // the one "quick look" in here that is not quick.
            "hash" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let kind = match req.params["kind"].as_str() {
                    Some("md5") => cian_core::attrs::HashKind::Md5,
                    _ => cian_core::attrs::HashKind::Sha256,
                };
                let paths = self.targets(&which)?;
                let stop = std::sync::atomic::AtomicBool::new(false);
                let mut rows = Vec::new();
                for p in paths.iter().take(200) {
                    // A directory has no checksum, and saying so beats the
                    // read error the caller would otherwise be handed.
                    if p.is_dir() {
                        rows.push(serde_json::json!({
                            "name": p.file_name().map(|s| s.to_string_lossy().into_owned()),
                            "sum": "(ディレクトリ)",
                        }));
                        continue;
                    }
                    let sum = cian_core::attrs::hash_file(p, kind, &stop)?;
                    rows.push(serde_json::json!({
                        "name": p.file_name().map(|s| s.to_string_lossy().into_owned()),
                        "sum": sum,
                    }));
                }
                Ok(serde_json::json!({ "kind": req.params["kind"].as_str().unwrap_or("sha256"), "rows": rows }))
            }
            // What is biggest here. On a worker with a cancel flag, because
            // pointed at a home directory it is minutes rather than seconds.
            "du" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let dir = match req.params["path"].as_str() {
                    Some(p) => std::path::PathBuf::from(p),
                    None => self.pane_mut(&which)?.cwd.clone(),
                };
                let stop = std::sync::atomic::AtomicBool::new(false);
                let rows = cian_core::du::analyze(&dir, &stop, &mut |_| {});
                Ok(serde_json::json!({
                    "cwd": dir.display().to_string(),
                    "rows": rows.iter().take(500).map(|e| serde_json::json!({
                        "name": e.name,
                        "path": e.path.display().to_string(),
                        "size": e.size,
                        "is_dir": e.is_dir,
                    })).collect::<Vec<_>>(),
                }))
            }
            // Find by name, or grep inside files. One method: the two differ
            // by a mode, and the pattern language — bare text is a literal,
            // /re/ is a regex, /re/i ignores case — is the same for both, so
            // splitting them would be two doors onto one room.
            "search" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let needle = req.params["needle"].as_str().unwrap_or("").to_string();
                if needle.is_empty() {
                    anyhow::bail!("探す文字列がありません");
                }
                let mode = match req.params["mode"].as_str() {
                    Some("content") => cian_core::search::Mode::Content,
                    _ => cian_core::search::Mode::Name,
                };
                let root = self.pane_mut(&which)?.cwd.clone();
                let query = cian_core::search::Query::parse(&needle, mode)
                    .map_err(|e| anyhow::anyhow!(e))?;
                let stop = std::sync::atomic::AtomicBool::new(false);
                let mut hits = Vec::new();
                let outcome = cian_core::search::search(&root, &query, &stop, &mut |h| {
                    if hits.len() < 2000 {
                        hits.push(serde_json::json!({
                            "path": h.path.display().to_string(),
                            "rel": h.rel.display().to_string(),
                            "is_dir": h.is_dir,
                            "line": h.line.as_ref().map(|(n, t)| serde_json::json!({
                                "n": n,
                                "text": t.chars().take(400).collect::<String>(),
                            })),
                        }));
                    }
                });
                Ok(serde_json::json!({
                    "root": root.display().to_string(),
                    "needle": needle,
                    "mode": if matches!(mode, cian_core::search::Mode::Content) { "content" } else { "name" },
                    "truncated": matches!(outcome, cian_core::search::Outcome::Truncated),
                    "hits": hits,
                }))
            }
            // Load a set of paths into a pane as if it were a listing.
            //
            // The terminal build calls it panelizing, and it is what makes a
            // search result useful rather than merely informative: the matches
            // become rows to mark and operate on with the keys already known.
            "panelize" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let paths: Vec<std::path::PathBuf> = req.params["paths"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).map(std::path::PathBuf::from).collect())
                    .unwrap_or_default();
                if paths.is_empty() {
                    anyhow::bail!("読み込むものがありません");
                }
                let label = req.params["label"].as_str().unwrap_or("結果").to_string();
                let pane = self.pane_mut(&which)?;
                let root = pane.cwd.clone();
                let entries: Vec<cian_core::Entry> = paths
                    .iter()
                    .map(|p| {
                        let rel = p.strip_prefix(&root).unwrap_or(p);
                        cian_core::Entry::flat(rel, p.clone(), p.is_dir())
                    })
                    .collect();
                pane.enter_flat(label, entries);
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            // Everything below here, one row per file.
            //
            // Its own method rather than a search for nothing: a search wants
            // something to look for and is right to refuse an empty needle,
            // and "show me all of it" is a different question with a different
            // answer — directories are rows in a listing and noise in a branch.
            "branch" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let root = self.pane_mut(&which)?.cwd.clone();
                let stop = std::sync::atomic::AtomicBool::new(false);
                let query = cian_core::search::Query::new("");
                let mut entries = Vec::new();
                cian_core::search::search(&root, &query, &stop, &mut |h| {
                    if !h.is_dir && entries.len() < 20_000 {
                        entries.push(cian_core::Entry::flat(&h.rel, h.path, false));
                    }
                });
                let found = entries.len();
                let pane = self.pane_mut(&which)?;
                pane.enter_flat("ブランチ", entries);
                Ok(serde_json::json!({
                    "found": found,
                    "pane": serde_json::to_value(PaneView::of(pane))?,
                }))
            }
            // ---- Left against right ----
            //
            // One method for both, because `=` is one key. What is under the
            // two cursors decides: two files are compared line by line, two
            // directories recursively. Asking the window to work out which
            // would put the decision where the files are not.
            "compare" => {
                let (lp, ln, ld) = self.selected("left")?;
                let (rp, rn, rd) = self.selected("right")?;
                if ld != rd {
                    anyhow::bail!("{ln} と {rn} は種類が違います");
                }
                let stop = std::sync::atomic::AtomicBool::new(false);
                if ld {
                    let d = cian_core::dirdiff::compare(&lp, &rp, &stop, &mut |_| {});
                    let rows: Vec<_> = d.entries.iter().take(5000).map(|e| serde_json::json!({
                        "rel": e.rel.display().to_string(),
                        "is_dir": e.is_dir,
                        "status": match e.status {
                            cian_core::dirdiff::Status::OnlyLeft => "left",
                            cian_core::dirdiff::Status::OnlyRight => "right",
                            cian_core::dirdiff::Status::Differ => "differ",
                        },
                    })).collect();
                    return Ok(serde_json::json!({
                        "kind": "dirs",
                        "left": lp.display().to_string(),
                        "right": rp.display().to_string(),
                        "truncated": d.truncated,
                        "rows": rows,
                    }));
                }
                let d = cian_core::diff::diff_files(&lp, &rp)?;
                // Folded to three lines of context. The whole file is right
                // for a file being read and wrong for a difference being
                // looked at: the point is what changed, and pages of identical
                // lines between two changes hide it.
                let folded = cian_core::diff::fold(&d.rows, 3);
                let rows: Vec<_> = folded.iter().take(20_000).map(|r| match r {
                    cian_core::diff::Row::Same { left, right } => serde_json::json!({
                        "kind": "same", "ln": left.no, "rn": right.no,
                        "left": left.text, "right": right.text,
                    }),
                    cian_core::diff::Row::Changed { left, right } => serde_json::json!({
                        "kind": "changed", "ln": left.no, "rn": right.no,
                        "left": left.text, "right": right.text,
                    }),
                    cian_core::diff::Row::Removed { left } => serde_json::json!({
                        "kind": "removed", "ln": left.no, "left": left.text,
                    }),
                    cian_core::diff::Row::Added { right } => serde_json::json!({
                        "kind": "added", "rn": right.no, "right": right.text,
                    }),
                    cian_core::diff::Row::Skipped { lines } => serde_json::json!({
                        "kind": "skipped", "lines": lines,
                    }),
                }).collect();
                Ok(serde_json::json!({
                    "kind": "files",
                    "left": ln, "right": rn,
                    "added": d.added, "removed": d.removed, "changed": d.changed,
                    "truncated": d.truncated,
                    "summary": cian_core::diff::summary(&d),
                    "rows": rows,
                }))
            }
            // ---- Bulk rename ----
            //
            // The plan first, always. `:renamepattern` can rename a hundred
            // files, and the one thing that makes that safe is seeing the
            // hundred new names before any of them exists.
            "renameplan" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pattern = req.params["pattern"].as_str().unwrap_or("").to_string();
                let paths = self.targets(&which)?;
                let names: Vec<String> = paths
                    .iter()
                    .map(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())
                    .collect();
                let planned = cian_core::rename::plan_batch(&pattern, &names, Default::default())
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let rows: Vec<_> = names.iter().zip(planned.iter()).zip(paths.iter())
                    .map(|((from, to), p)| serde_json::json!({
                        "from": from, "to": to,
                        "path": p.display().to_string(),
                        "same": from == to,
                        "clash": p.with_file_name(to).exists() && from != to,
                    }))
                    .collect();
                Ok(serde_json::json!({ "pattern": pattern, "rows": rows }))
            }
            "renameapply" => {
                let pairs = req.params["rows"].as_array().cloned().unwrap_or_default();
                let mut done = 0usize;
                let mut errors: Vec<String> = Vec::new();
                for row in &pairs {
                    let (Some(path), Some(to)) = (row["path"].as_str(), row["to"].as_str()) else {
                        continue;
                    };
                    let from = std::path::PathBuf::from(path);
                    match cian_core::ops::rename_in_place(&from, to) {
                        Ok(_) => {
                            self.undo.push(Undo::Rename { from: from.clone(), to: from.with_file_name(to) });
                            done += 1;
                        }
                        Err(e) => errors.push(format!("{}: {e}", from.display())),
                    }
                }
                for which in ["left", "right"] {
                    let _ = self.pane_mut(which).map(|p| p.reload());
                }
                Ok(serde_json::json!({ "renamed": done, "errors": errors }))
            }
            // ---- Archives ----
            "archivelist" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let (path, name, _) = self.selected(&which)?;
                if !cian_core::archive::is_archive(&path) {
                    anyhow::bail!("{name} はアーカイブではありません");
                }
                let members = cian_core::archive::list(&path)?;
                Ok(serde_json::json!({
                    "name": name,
                    "path": path.display().to_string(),
                    "members": members.iter().take(5000).map(|m| serde_json::json!({
                        "name": m.name, "is_dir": m.is_dir,
                        "size": m.size, "compressed": m.compressed,
                    })).collect::<Vec<_>>(),
                }))
            }
            "compress" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let kind = req.params["kind"].as_str().unwrap_or("zip").to_string();
                let paths = self.targets(&which)?;
                if paths.is_empty() {
                    anyhow::bail!("対象がありません");
                }
                let cwd = self.pane_mut(&which)?.cwd.clone();
                let stem = req.params["name"].as_str().map(str::to_string).unwrap_or_else(|| {
                    paths[0].file_stem().map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "archive".into())
                });
                let ext = match kind.as_str() {
                    "tar" => "tar",
                    "targz" => "tar.gz",
                    _ => "zip",
                };
                let dest = cwd.join(format!("{stem}.{ext}"));
                if dest.exists() {
                    anyhow::bail!("{} はすでにあります", dest.display());
                }
                let stop = std::sync::atomic::AtomicBool::new(false);
                let mut noop = |_: &cian_core::progress::Progress| {};
                let mut ctl = cian_core::progress::Ctl { cancel: &stop, on_progress: &mut noop };
                let report = match kind.as_str() {
                    "tar" => cian_core::archive::create_tar(&paths, &dest, false, &mut ctl),
                    "targz" => cian_core::archive::create_tar(&paths, &dest, true, &mut ctl),
                    _ => cian_core::archive::create_zip(
                        &paths, &dest, req.params["password"].as_str(), &mut ctl),
                };
                self.undo.push(Undo::Created { path: dest.clone() });
                let pane = self.pane_mut(&which)?;
                pane.reload()?;
                Ok(serde_json::json!({
                    "made": dest.file_name().map(|s| s.to_string_lossy().into_owned()),
                    "ok": report.ok, "errors": report.errors,
                    "pane": serde_json::to_value(PaneView::of(pane))?,
                }))
            }
            "extract" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let (path, name, _) = self.selected(&which)?;
                if !cian_core::archive::is_archive(&path) {
                    anyhow::bail!("{name} はアーカイブではありません");
                }
                let cwd = self.pane_mut(&which)?.cwd.clone();
                let stop = std::sync::atomic::AtomicBool::new(false);
                let mut noop = |_: &cian_core::progress::Progress| {};
                let mut ctl = cian_core::progress::Ctl { cancel: &stop, on_progress: &mut noop };
                let report = cian_core::archive::extract(
                    &path, &[], &cwd, req.params["password"].as_str(), "", &mut ctl);
                let pane = self.pane_mut(&which)?;
                pane.reload()?;
                Ok(serde_json::json!({
                    "from": name, "ok": report.ok, "errors": report.errors,
                    "pane": serde_json::to_value(PaneView::of(pane))?,
                }))
            }
            // ---- Version control ----
            //
            // git and svn behind one set of methods, because a person standing
            // in a working copy wants "the history of this file" and not "the
            // git history of this file". Which one it is, is a property of the
            // directory rather than a question worth asking.
            "vcs" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let dir = self.pane_mut(&which)?.cwd.clone();
                let git = cian_core::git::status(&dir);
                let svn = cian_core::svn::is_working_copy(&dir);
                Ok(serde_json::json!({
                    "kind": if git.is_some() { Some("git") } else if svn { Some("svn") } else { None },
                    "branch": git.as_ref().map(|g| g.branch.clone()),
                    "root": git.as_ref().map(|g| g.root.display().to_string()),
                }))
            }
            "log" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let only_this = req.params["file"].as_bool().unwrap_or(false);
                let dir = self.pane_mut(&which)?.cwd.clone();
                let file = if only_this { self.selected(&which).ok().map(|(p, _, _)| p) } else { None };
                let (kind, commits) = if cian_core::git::status(&dir).is_some() {
                    ("git", cian_core::git::log(&dir, file.as_deref(), 200))
                } else if cian_core::svn::is_working_copy(&dir) {
                    ("svn", cian_core::svn::log(&dir, file.as_deref(), 200))
                } else {
                    anyhow::bail!("git でも svn でもありません");
                };
                Ok(serde_json::json!({
                    "kind": kind,
                    "of": file.as_ref().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned()),
                    "commits": commits.iter().map(|c| serde_json::json!({
                        "hash": c.hash, "date": c.date, "author": c.author, "subject": c.subject,
                    })).collect::<Vec<_>>(),
                }))
            }
            // The diff of one file against what is committed, or of one commit.
            "vcsdiff" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let dir = self.pane_mut(&which)?.cwd.clone();
                let is_git = cian_core::git::status(&dir).is_some();
                let text = match req.params["hash"].as_str() {
                    Some(h) => if is_git {
                        cian_core::git::show(&dir, h)
                    } else {
                        cian_core::svn::show(&dir, h)
                    },
                    None => {
                        let (path, _, _) = self.selected(&which)?;
                        if is_git {
                            cian_core::git::file_diff(&dir, &path)
                        } else {
                            cian_core::svn::file_diff(&dir, &path)
                        }
                    }
                };
                let Some(text) = text else {
                    anyhow::bail!("差分がありません");
                };
                Ok(serde_json::json!({
                    "lines": text.lines().take(20_000).map(str::to_string).collect::<Vec<_>>(),
                }))
            }
            "stage" | "unstage" | "discard" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let dir = self.pane_mut(&which)?.cwd.clone();
                if cian_core::git::status(&dir).is_none() {
                    anyhow::bail!("git リポジトリではありません");
                }
                let paths = self.targets(&which)?;
                match req.method.as_str() {
                    "stage" => cian_core::git::stage(&dir, &paths)?,
                    "unstage" => cian_core::git::unstage(&dir, &paths)?,
                    _ => cian_core::git::discard(&dir, &paths)?,
                }
                let pane = self.pane_mut(&which)?;
                pane.reload()?;
                Ok(serde_json::json!({
                    "did": req.method,
                    "count": paths.len(),
                    "pane": serde_json::to_value(PaneView::of(pane))?,
                }))
            }
            // Files with the same contents. Compared by content, not by name —
            // which is the whole reason to ask, since two copies of a photo
            // rarely share a name.
            "dedup" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let paths = self.targets(&which)?;
                let stop = std::sync::atomic::AtomicBool::new(false);
                let groups = cian_core::dedup::find_duplicates(&paths, &stop);
                Ok(serde_json::json!({
                    "groups": groups.iter().map(|g| g.iter()
                        .map(|p| p.display().to_string()).collect::<Vec<_>>())
                        .collect::<Vec<_>>(),
                }))
            }
            // Leave a flat listing and go back to the directory it came from.
            "leaveflat" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                if !pane.is_flat() {
                    anyhow::bail!("一覧はもともとのディレクトリです");
                }
                pane.leave_flat()?;
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            // Where this pane has been. Its own history, not a shared one —
            // the two panes are two places at once, which is the point of two.
            "back" | "forward" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                let moved = if req.method == "back" { pane.go_back()? } else { pane.go_forward()? };
                if !moved {
                    anyhow::bail!(
                        "{}に履歴がありません",
                        if req.method == "back" { "前" } else { "先" }
                    );
                }
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            "history" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                Ok(serde_json::json!({
                    "cwd": pane.cwd.display().to_string(),
                    "back": pane.history.iter().take(40)
                        .map(|p| p.display().to_string()).collect::<Vec<_>>(),
                    "forward": pane.forward.iter().take(40)
                        .map(|p| p.display().to_string()).collect::<Vec<_>>(),
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
                let (from, _, _) = self.selected(&which)?;
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
            "undo" | "redo" => {
                let taking = if req.method == "undo" { &self.undo } else { &self.redo };
                let Some(step) = taking.pop() else {
                    anyhow::bail!(
                        "{}操作はありません",
                        if req.method == "undo" { "取り消せる" } else { "やり直せる" }
                    );
                };
                if req.method == "undo" {
                    if let Some(back) = step.inverted() {
                        self.redo.push(back);
                    }
                } else if let Some(back) = step.inverted() {
                    self.undo.push(back);
                }
                let said = step.describe(req.method == "undo");
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
