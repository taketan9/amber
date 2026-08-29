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
mod shell;
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
    /// This side's tabs — one crumb each — and which is showing.
    tabs: Vec<String>,
    tab: usize,
    /// `user@host` when this pane is showing a server. The window needs it to
    /// know that Enter, `..` and `c` all mean something over the network.
    remote: Option<String>,
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
    /// The active tab of a side, with the side's tab strip attached.
    fn of_side(side: &Side) -> PaneView {
        let mut v = PaneView::of(side.get());
        v.tabs = side
            .tabs
            .iter()
            .map(|p| {
                p.flat_label()
                    .map(str::to_string)
                    .or_else(|| p.remote_view().map(|(h, _)| h.to_string()))
                    .unwrap_or_else(|| {
                        p.cwd
                            .file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| p.cwd.display().to_string())
                    })
            })
            .collect();
        v.tab = side.at;
        v
    }

    fn of(pane: &Pane) -> Self {
        PaneView {
            cwd: pane.cwd.display().to_string(),
            cursor: pane.cursor,
            marked: pane.mark_count(),
            hidden_shown: pane.show_hidden,
            flat: pane.flat_label().map(str::to_string),
            // Filled in by `of_side`; a bare pane does not know its siblings.
            tabs: Vec::new(),
            tab: 0,
            remote: pane.remote_view().map(|(host, _)| host.to_string()),
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
/// One tab of the shell panel: its layout, and which pane has the keyboard.
struct ShellTab {
    root: shell::Node,
    focus: u64,
    /// Type into every pane of this tab at once.
    ///
    /// The reason splits exist for a lot of people: four servers side by side
    /// and the same command on all four. Per tab rather than global, because
    /// the tab you built for that is not the tab you keep a shell in.
    sync: bool,
}

/// One side of the window, and the tabs it holds.
///
/// The active tab *is* the pane as far as everything else is concerned —
/// `pane_mut` hands back a `&mut Pane` exactly as it always did, so the forty
/// call sites that operate on "this pane" did not have to learn what a tab is.
/// Only the handful that switch or close one know there is a list.
struct Side {
    tabs: Vec<Pane>,
    at: usize,
}

impl Side {
    fn new(pane: Pane) -> Self {
        Side { tabs: vec![pane], at: 0 }
    }

    fn now(&mut self) -> &mut Pane {
        let at = self.at.min(self.tabs.len().saturating_sub(1));
        &mut self.tabs[at]
    }

    fn get(&self) -> &Pane {
        &self.tabs[self.at.min(self.tabs.len().saturating_sub(1))]
    }
}

struct Session {
    left: Side,
    right: Side,
    jobs: Jobs,
    out: Out,
    undo: Stack,
    find: Find,
    /// Held for a later paste. Independent of the system clipboard, and of
    /// which pane is focused — that is the point of it.
    clip: Option<cian_core::clip::Clipboard>,
    /// The file open in the viewer, as `view_file` read it — kept whether it
    /// is text or a binary, because re-decoding needs the raw bytes and a
    /// second read would be a second answer to "what is in this file".
    shown: Option<(std::path::PathBuf, cian_core::viewer::View)>,
    /// A binary open in the hex editor, as it was read plus whatever has been
    /// overwritten. Separate from `open` because the two save differently:
    /// text goes back through its encoding, bytes go back as bytes.
    hex: Option<(std::path::PathBuf, cian_core::viewer::View)>,
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
    /// The shell panel's tabs, and which one is showing.
    ///
    /// Started on demand rather than at launch: most sessions never open the
    /// panel, and a shell process per window that nobody asked for is a
    /// process nobody accounts for. More than one because a long build in tab
    /// one is the reason you want tab two.
    /// Every shell alive, whichever tab or split it belongs to.
    shells: Vec<shell::Shell>,
    /// One tree per tab: how that tab's shells are arranged, and which of them
    /// has the keyboard.
    tabs: Vec<ShellTab>,
    shell_at: usize,
    shell_next: u64,
    /// Where a remote pane is connected, and how.
    ///
    /// Held rather than asked for each time: SFTP wants a password, and a file
    /// manager that asks again for every directory you walk into is one nobody
    /// uses twice. Kept only in memory — never written anywhere.
    remotes: std::collections::HashMap<String, cian_scp::Target>,
}

impl Session {
    fn new(dir: std::path::PathBuf, out: Out) -> anyhow::Result<Self> {
        Ok(Session {
            left: Side::new(Pane::new(dir.clone())?),
            right: Side::new(Pane::new(dir)?),
            jobs: Jobs::default(),
            out,
            undo: Stack::default(),
            find: Find::default(),
            clip: None,
            open: None,
            shown: None,
            hex: None,
            redo: Stack::default(),
            shells: Vec::new(),
            tabs: Vec::new(),
            shell_at: 0,
            shell_next: 1,
            remotes: std::collections::HashMap::new(),
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

    /// Climb one level inside an archive; past the root, leave it and land on
    /// the archive file, which is where you were when you went in.
    fn archive_up(
        &mut self,
        which: &str,
        archive: &std::path::Path,
        sub: &str,
    ) -> anyhow::Result<serde_json::Value> {
        if sub.is_empty() {
            let dir = archive.parent().unwrap_or(archive).to_path_buf();
            let name = archive.file_name().map(|s| s.to_string_lossy().into_owned());
            let pane = self.pane_mut(which)?;
            *pane = Pane::new(dir)?;
            if let Some(name) = name {
                if let Some(i) = pane.entries.iter().position(|e| e.name == name) {
                    pane.cursor = i;
                }
            }
            return self.view(which);
        }
        // "a/b/" → "a/"; "a/" → "".
        let parent = sub
            .trim_end_matches('/')
            .rsplit_once('/')
            .map(|(head, _)| format!("{head}/"))
            .unwrap_or_default();
        let child = sub.trim_end_matches('/').rsplit('/').next().map(str::to_string);
        let members = cian_core::archive::list(archive)?;
        let rows = cian_core::archive::archive_rows(archive, &members, &parent);
        let pane = self.pane_mut(which)?;
        pane.enter_archive(archive.to_path_buf(), parent, rows);
        // Land on the directory just left, as real navigation does.
        if let Some(child) = child {
            if let Some(i) = pane.entries.iter().position(|e| e.name == child) {
                pane.cursor = i;
            }
        }
        self.view(which)
    }

    fn targets(&self, which: &str) -> anyhow::Result<Vec<std::path::PathBuf>> {
        let pane = match which {
            "left" => self.left.get(),
            "right" => self.right.get(),
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
            "left" => self.right.get().cwd.clone(),
            _ => self.left.get().cwd.clone(),
        }
    }

    fn pane_mut(&mut self, which: &str) -> anyhow::Result<&mut Pane> {
        Ok(self.side_mut(which)?.now())
    }

    /// This side, as the window wants it: the active tab's listing *and* the
    /// tab strip.
    ///
    /// Every reply that hands back a pane goes through here. Returning a bare
    /// pane worked until there were tabs, and then every operation would have
    /// quietly dropped the strip — a tab bar that vanishes when you rename a
    /// file is worse than no tab bar.
    fn view(&mut self, which: &str) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::to_value(PaneView::of_side(self.side_mut(which)?))?)
    }

    /// The pane with the keyboard, in the tab that is showing.
    fn shell_now(&mut self) -> Option<&mut shell::Shell> {
        let id = self.tabs.get(self.shell_at)?.focus;
        self.shells.iter_mut().find(|s| s.id == id).filter(|s| s.alive())
    }

    /// The panel as the window needs it: every pane of the showing tab, where
    /// it sits, and its screen.
    ///
    /// Places are worked out here because the tree is here. The window puts
    /// boxes where it is told; it does not need to know what a split is.
    fn shell_reply(&self) -> serde_json::Value {
        let Some(tab) = self.tabs.get(self.shell_at) else {
            return serde_json::json!({ "gone": true });
        };
        let mut places = Vec::new();
        tab.root.places(0.0, 0.0, 1.0, 1.0, &mut places);
        let panes: Vec<_> = places
            .iter()
            .filter_map(|(id, x, y, w, h)| {
                let sh = self.shells.iter().find(|s| s.id == *id)?;
                Some(serde_json::json!({
                    "id": id,
                    "x": x, "y": y, "w": w, "h": h,
                    "focused": *id == tab.focus,
                    "screen": sh.screen(),
                }))
            })
            .collect();
        serde_json::json!({
            "panes": panes,
            "tabs": self.tabs.len(),
            "tab": self.shell_at,
            "showing": tab.focus,
            "sync": tab.sync,
        })
    }

    /// Start a shell and hand back its id.
    fn new_shell(&mut self, cwd: &std::path::Path, rows: u16, cols: u16) -> anyhow::Result<u64> {
        let id = self.shell_next;
        self.shell_next += 1;
        let sh = shell::Shell::start(id, cwd, rows, cols, self.out.clone())?;
        self.shells.push(sh);
        Ok(id)
    }

    /// Forget every shell no tab still points at.
    fn prune_shells(&mut self) {
        let mut alive = Vec::new();
        for tab in &self.tabs {
            tab.root.leaves(&mut alive);
        }
        self.shells.retain(|s| alive.contains(&s.id));
    }

    fn side_mut(&mut self, which: &str) -> anyhow::Result<&mut Side> {
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
                "left": PaneView::of_side(&self.left),
                "right": PaneView::of_side(&self.right),
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
                self.view(&which)
            }
            // Step into whatever the cursor is on, or out to the parent.
            "enter" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let at = req.params["cursor"].as_u64().map(|n| n as usize);
                let pane = self.pane_mut(&which)?;
                if let Some(n) = at {
                    pane.cursor = n.min(pane.entries.len().saturating_sub(1));
                }
                // Inside an archive the rows are synthetic: their paths do
                // not exist on the disk, so the ordinary descent would look
                // for a directory that is not there. A directory row means
                // "list this prefix instead".
                if let Some((archive, sub)) = pane.archive_view() {
                    let (archive, sub) = (archive.to_path_buf(), sub.to_string());
                    let Some(row) = pane.entries.get(pane.cursor) else {
                        anyhow::bail!("対象がありません");
                    };
                    if row.is_parent {
                        return self.archive_up(&which, &archive, &sub);
                    }
                    if !row.is_dir {
                        anyhow::bail!("アーカイブ内のファイルはまだ開けません");
                    }
                    let deeper = format!("{sub}{}/", row.name);
                    let members = cian_core::archive::list(&archive)?;
                    let rows = cian_core::archive::archive_rows(&archive, &members, &deeper);
                    let pane = self.pane_mut(&which)?;
                    pane.enter_archive(archive, deeper, rows);
                    return self.view(&which);
                }
                let was = pane.cwd.clone();
                pane.enter_selected()?;
                // Only if it actually went somewhere: `Enter` on a file will
                // one day open it, and that is not a step to walk back.
                if pane.cwd != was {
                    self.undo.push(Undo::Navigated { pane: which.clone(), from: was });
                }
                self.view(&which)
            }
            "parent" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                if let Some((archive, sub)) = pane.archive_view() {
                    let (archive, sub) = (archive.to_path_buf(), sub.to_string());
                    return self.archive_up(&which, &archive, &sub);
                }
                let was = pane.cwd.clone();
                pane.go_parent()?;
                if pane.cwd != was {
                    self.undo.push(Undo::Navigated { pane: which.clone(), from: was });
                }
                self.view(&which)
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
                self.view(&which)
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
                self.view(&which)
            }
            "invert" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                for i in 0..pane.entries.len() {
                    pane.toggle_mark_at(i);
                }
                self.view(&which)
            }
            // Exactly these, and nothing else, marked.
            //
            // Visual selection re-marks from its anchor on every move, so it
            // needs to *state* the set rather than toggle towards it —
            // toggling would make an overshoot permanent instead of something
            // you correct by moving back.
            "setmarks" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let want: std::collections::HashSet<String> = req.params["paths"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).map(str::to_string).collect())
                    .unwrap_or_default();
                let pane = self.pane_mut(&which)?;
                pane.clear_marks();
                for i in 0..pane.entries.len() {
                    if want.contains(&pane.entries[i].path.display().to_string()) {
                        pane.set_mark_at(i);
                    }
                }
                self.view(&which)
            }
            "unmarkall" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                pane.clear_marks();
                self.view(&which)
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
                // `view_file` rather than `read_text`, because it answers for
                // everything: text in whatever encoding, a hex dump for a
                // binary, extracted text for an Office file or a PDF. The
                // narrower read refused a binary with "looks binary", which is
                // true and leaves the person with nothing.
                let shown = cian_core::viewer::view_file(&path)?;
                let binary = matches!(shown.kind, cian_core::viewer::ViewKind::Binary);
                // The editable copy is only fetched for real text: a hex dump
                // is a rendering, and saving one back would write the dump.
                let file = if binary {
                    None
                } else {
                    cian_core::grepedit::read_text(&path).ok()
                };
                let reply = serde_json::json!({
                    "name": name,
                    "path": path.display().to_string(),
                    "lines": shown.lines,
                    "bytes": len,
                    "binary": binary,
                    "truncated": shown.truncated,
                    "encoding": format!("{:?}", shown.encoding),
                    "eol": format!("{:?}", shown.eol),
                    "bom": shown.bom,
                    "lang": if binary {
                        None
                    } else {
                        cian_core::highlight::detect(&path).map(|l| format!("{l:?}"))
                    },
                });
                // Text and binary are remembered separately: one saves back
                // through its encoding, the other as bytes, and a single slot
                // would make `save` guess which it was holding.
                if binary {
                    self.open = None;
                    self.hex = Some((path.clone(), shown.clone()));
                } else {
                    self.hex = None;
                    self.open = file.map(|f| (path.clone(), f));
                }
                self.shown = Some((path, shown));
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
                self.view(&which)
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
                    "pane": self.view(&which)?,
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
                    "pane": self.view(&which)?,
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
                    "pane": self.view(&which)?,
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
                    "pane": self.view(&which)?,
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
            // ---- The shell ----
            "shellopen" | "shelltab" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let rows = req.params["rows"].as_u64().unwrap_or(24) as u16;
                let cols = req.params["cols"].as_u64().unwrap_or(80) as u16;
                if req.method == "shellopen" && !self.tabs.is_empty() {
                    for sh in &mut self.shells {
                        sh.resize(rows, cols);
                    }
                    return Ok(self.shell_reply());
                }
                let cwd = self.pane_mut(&which)?.cwd.clone();
                let id = self.new_shell(&cwd, rows, cols)?;
                self.tabs.push(ShellTab { root: shell::Node::Leaf(id), focus: id, sync: false });
                self.shell_at = self.tabs.len() - 1;
                Ok(self.shell_reply())
            }
            // Split the focused pane. Shift+F8 side by side, Shift+F9 stacked
            // — the terminal build's two keys.
            "shellsplit" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let down = req.params["down"].as_bool().unwrap_or(false);
                let rows = req.params["rows"].as_u64().unwrap_or(24) as u16;
                let cols = req.params["cols"].as_u64().unwrap_or(80) as u16;
                let Some(tab) = self.tabs.get(self.shell_at) else {
                    anyhow::bail!("シェルが開いていません");
                };
                let at = tab.focus;
                let cwd = self.pane_mut(&which)?.cwd.clone();
                let fresh = self.new_shell(&cwd, rows, cols)?;
                let tab = &mut self.tabs[self.shell_at];
                if !tab.root.split_at(at, fresh, down) {
                    anyhow::bail!("分割できませんでした");
                }
                tab.focus = fresh;
                Ok(self.shell_reply())
            }
            // Drag the border the focused pane sits against.
            "shellresizepane" => {
                let wider = req.params["wider"].as_bool().unwrap_or(true);
                let down = req.params["down"].as_bool().unwrap_or(false);
                let Some(tab) = self.tabs.get_mut(self.shell_at) else {
                    anyhow::bail!("シェルが開いていません");
                };
                let id = tab.focus;
                if !tab.root.resize(id, wider, down) {
                    anyhow::bail!("その向きに動かせる境界がありません");
                }
                Ok(self.shell_reply())
            }
            // Move the keyboard to the next pane of this tab.
            "shellfocus" => {
                let step = req.params["step"].as_i64().unwrap_or(1);
                let at = req.params["at"].as_u64();
                let Some(tab) = self.tabs.get_mut(self.shell_at) else {
                    anyhow::bail!("シェルが開いていません");
                };
                let mut ids = Vec::new();
                tab.root.leaves(&mut ids);
                match at.and_then(|n| ids.get(n as usize).copied()) {
                    Some(id) => tab.focus = id,
                    None if at.is_some() => anyhow::bail!("そのペインはありません"),
                    None => {
                        if let Some(now) = ids.iter().position(|id| *id == tab.focus) {
                            let n = ids.len() as i64;
                            tab.focus = ids[((now as i64 + step).rem_euclid(n)) as usize];
                        }
                    }
                }
                Ok(self.shell_reply())
            }
            // Close the focused pane; the last one closes the tab.
            "shellpaneclose" => {
                let Some(tab) = self.tabs.get_mut(self.shell_at) else {
                    anyhow::bail!("シェルが開いていません");
                };
                let going = tab.focus;
                if tab.root.close(going) {
                    let mut ids = Vec::new();
                    tab.root.leaves(&mut ids);
                    tab.focus = ids[0];
                    self.prune_shells();
                    return Ok(self.shell_reply());
                }
                // It was the only pane: closing it closes the tab.
                self.tabs.remove(self.shell_at);
                self.prune_shells();
                if self.tabs.is_empty() {
                    return Ok(serde_json::json!({ "gone": true }));
                }
                self.shell_at = self.shell_at.min(self.tabs.len() - 1);
                Ok(self.shell_reply())
            }
            "shellinput" => {
                let text = req.params["text"].as_str().unwrap_or("").to_string();
                let Some(tab) = self.tabs.get(self.shell_at) else {
                    anyhow::bail!("シェルが開いていません");
                };
                // With sync on, every pane of this tab hears it.
                let targets: Vec<u64> = if tab.sync {
                    let mut ids = Vec::new();
                    tab.root.leaves(&mut ids);
                    ids
                } else {
                    vec![tab.focus]
                };
                for id in targets {
                    if let Some(sh) = self.shells.iter().find(|s| s.id == id) {
                        sh.write(text.as_bytes());
                    }
                }
                Ok(serde_json::json!({}))
            }
            // Type into every pane of this tab at once, or stop.
            "shellsync" => {
                let Some(tab) = self.tabs.get_mut(self.shell_at) else {
                    anyhow::bail!("シェルが開いていません");
                };
                tab.sync = req.params["on"].as_bool().unwrap_or(!tab.sync);
                let on = tab.sync;
                let mut reply = self.shell_reply();
                reply["sync"] = serde_json::json!(on);
                Ok(reply)
            }
            "shellresize" => {
                // Each pane gets the size of *its* box, not the panel's — a
                // pane that thinks it is full width wraps its output at the
                // wrong column, which is the classic broken-split look.
                let (rows, cols) = (
                    req.params["rows"].as_u64().unwrap_or(24) as f32,
                    req.params["cols"].as_u64().unwrap_or(80) as f32,
                );
                for tab in &self.tabs {
                    let mut places = Vec::new();
                    tab.root.places(0.0, 0.0, 1.0, 1.0, &mut places);
                    for (id, _, _, w, h) in places {
                        if let Some(sh) = self.shells.iter_mut().find(|s| s.id == id) {
                            sh.resize(
                                ((rows * h) as u16).max(2),
                                ((cols * w) as u16).max(20),
                            );
                        }
                    }
                }
                Ok(serde_json::json!({}))
            }
            "shellscroll" => {
                let lines = req.params["lines"].as_i64();
                let Some(sh) = self.shell_now() else {
                    anyhow::bail!("シェルが開いていません");
                };
                match lines {
                    Some(n) => sh.scroll(n as isize),
                    None => sh.to_bottom(),
                }
                Ok(self.shell_reply())
            }
            "shellgo" => {
                if self.tabs.is_empty() {
                    anyhow::bail!("シェルが開いていません");
                }
                let n = self.tabs.len() as i64;
                self.shell_at = match req.params["at"].as_i64() {
                    Some(at) => at.rem_euclid(n) as usize,
                    None => (self.shell_at as i64 + req.params["step"].as_i64().unwrap_or(1))
                        .rem_euclid(n) as usize,
                };
                Ok(self.shell_reply())
            }
            "shellclose" => {
                if self.tabs.is_empty() {
                    return Ok(serde_json::json!({ "gone": true }));
                }
                self.tabs.remove(self.shell_at);
                self.prune_shells();
                if self.tabs.is_empty() {
                    return Ok(serde_json::json!({ "gone": true }));
                }
                self.shell_at = self.shell_at.min(self.tabs.len() - 1);
                Ok(self.shell_reply())
            }
            // Run a command in the shell, in this pane's directory.
            //
            // `%` is the selection, `%f` the file, `%d` the directory — the
            // terminal build's substitutions, so a command that works there
            // works here.
            "run" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let line = req.params["line"].as_str().unwrap_or("").to_string();
                if line.trim().is_empty() {
                    anyhow::bail!("コマンドがありません");
                }
                let cwd = self.pane_mut(&which)?.cwd.clone();
                let paths = self.targets(&which).unwrap_or_default();
                let quoted: Vec<String> = paths.iter().map(|p| quote(&p.display().to_string())).collect();
                let file = paths.first().map(|p| quote(&p.display().to_string())).unwrap_or_default();
                let text = line
                    .replace("%d", &quote(&cwd.display().to_string()))
                    .replace("%f", &file)
                    .replace('%', &quoted.join(" "));
                let Some(sh) = self.shell_now() else {
                    anyhow::bail!("シェルが開いていません");
                };
                sh.write(format!("{text}\n").as_bytes());
                Ok(serde_json::json!({ "sent": text }))
            }
            // One command per marked file. `{}` is the path — the terminal
            // build's `:each`.
            "each" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let line = req.params["line"].as_str().unwrap_or("").to_string();
                if !line.contains("{}") {
                    anyhow::bail!("{{}} がありません（例: :each grep -l foo {{}}）");
                }
                let paths = self.targets(&which)?;
                let Some(sh) = self.shell_now() else {
                    anyhow::bail!("シェルが開いていません");
                };
                for p in &paths {
                    let one = line.replace("{}", &quote(&p.display().to_string()));
                    sh.write(format!("{one}\n").as_bytes());
                }
                Ok(serde_json::json!({ "ran": paths.len() }))
            }
            // Walk into an archive as though it were a directory.
            //
            // The rows come from cian-core, which is where they came from for
            // the terminal build too — two front ends disagreeing about what
            // is inside one zip would be a strange thing to ship.
            "enterarchive" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let sub = req.params["sub"].as_str().unwrap_or("").to_string();
                let archive = match req.params["archive"].as_str() {
                    Some(p) => std::path::PathBuf::from(p),
                    None => self.selected(&which)?.0,
                };
                if !cian_core::archive::is_archive(&archive) {
                    anyhow::bail!("アーカイブではありません");
                }
                let members = cian_core::archive::list(&archive)?;
                let rows = cian_core::archive::archive_rows(&archive, &members, &sub);
                let pane = self.pane_mut(&which)?;
                pane.enter_archive(archive.clone(), sub.clone(), rows);
                Ok(serde_json::json!({
                    "archive": archive.display().to_string(),
                    "sub": sub,
                    "pane": self.view(&which)?,
                }))
            }
            // The file's bytes, for something the window can draw but not read
            // — an image, mostly. Capped, and refused outright above the cap
            // rather than truncated: half a PNG is not a smaller PNG.
            "bytes" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let (path, name, is_dir) = self.selected(&which)?;
                if is_dir {
                    anyhow::bail!("{name} はディレクトリです");
                }
                const CAP: u64 = 24 * 1024 * 1024;
                let len = std::fs::metadata(&path)?.len();
                if len > CAP {
                    anyhow::bail!("{name} は大きすぎます（{} MB）", len / 1024 / 1024);
                }
                let bytes = std::fs::read(&path)?;
                Ok(serde_json::json!({
                    "name": name,
                    "kind": mime_of(&path),
                    "len": len,
                    "b64": b64(&bytes),
                }))
            }
            // Strip UTF-8 byte-order marks. UTF-16 ones are left alone:
            // without one, a UTF-16 file's byte order is guesswork.
            "nobom" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let paths: Vec<_> = self.targets(&which)?.into_iter().filter(|p| p.is_file()).collect();
                if paths.is_empty() {
                    anyhow::bail!("対象がありません");
                }
                let (mut stripped, mut none, mut utf16, mut failed) = (0, 0, 0, 0);
                for p in &paths {
                    match cian_core::ops::strip_utf8_bom(p) {
                        Ok(Some(true)) => stripped += 1,
                        Ok(Some(false)) => none += 1,
                        Ok(None) => utf16 += 1,
                        Err(_) => failed += 1,
                    }
                }
                let pane = self.pane_mut(&which)?;
                pane.reload()?;
                Ok(serde_json::json!({
                    "stripped": stripped, "none": none, "utf16": utf16, "failed": failed,
                    "pane": self.view(&which)?,
                }))
            }
            // The headings and definitions in the open file, for jumping.
            "outline" => {
                let Some((path, file)) = self.open.as_ref() else {
                    anyhow::bail!("開いているファイルがありません");
                };
                let items = cian_core::outline::outline(path, &file.lines);
                Ok(serde_json::json!({
                    "items": items.iter().map(|i| serde_json::json!({
                        "line": i.line, "level": i.level, "text": i.text,
                        "kind": format!("{:?}", i.kind),
                    })).collect::<Vec<_>>(),
                }))
            }
            // Files dropped onto a pane from the desktop. A move, like a drag
            // between two folders anywhere else.
            "drop" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let paths: Vec<std::path::PathBuf> = req.params["paths"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).map(std::path::PathBuf::from).collect())
                    .unwrap_or_default();
                if paths.is_empty() {
                    anyhow::bail!("落とされたものがありません");
                }
                let dest = self.pane_mut(&which)?.cwd.clone();
                let count = paths.len();
                let op = self.jobs.start(
                    Kind::Move, paths, Some(dest), self.out.clone(), self.undo.clone());
                Ok(serde_json::json!({ "op": op, "count": count }))
            }
            // ---- Line operations on the open file ----
            //
            // Done here rather than in the window because cian-core already
            // does them, correctly, for the terminal build. `:han` and `:zen`
            // in particular are a table of Japanese width mappings that nobody
            // should own two copies of.
            "textop" => {
                let Some((_, file)) = self.open.as_ref() else {
                    anyhow::bail!("開いているファイルがありません");
                };
                let lines: Vec<String> = req.params["lines"]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect())
                    .unwrap_or_else(|| file.lines.clone());
                let width = req.params["width"].as_u64().unwrap_or(4) as usize;
                use cian_core::textops as t;
                let out = match req.params["op"].as_str().unwrap_or("") {
                    "sort" => t::sort(&lines, false),
                    "rsort" => t::sort(&lines, true),
                    "uniq" => t::uniq(&lines),
                    "han" => lines.iter().map(|l| t::to_halfwidth(l)).collect(),
                    "zen" => lines.iter().map(|l| t::to_fullwidth(l)).collect(),
                    "expand" => t::expand_tabs(&lines, width),
                    "expandall" => t::expand_all_tabs(&lines, width),
                    "unexpand" => t::unexpand_tabs(&lines, width),
                    "reindent" => t::reindent(&lines, width),
                    other => anyhow::bail!("知らない操作: {other}"),
                };
                Ok(serde_json::json!({ "lines": out }))
            }
            // Change the line endings the open file will be written with.
            "eol" => {
                let Some((_, file)) = self.open.as_mut() else {
                    anyhow::bail!("開いているファイルがありません");
                };
                file.eol = match req.params["kind"].as_str() {
                    Some("crlf") => cian_core::viewer::Eol::Crlf,
                    _ => cian_core::viewer::Eol::Lf,
                };
                Ok(serde_json::json!({ "eol": format!("{:?}", file.eol) }))
            }
            // ---- Replace across every file a grep matched ----
            //
            // The plan first, and every line of it: this writes to files that
            // are not open and cannot be undone with `u`. Seeing each line
            // before and after is the only thing that makes it safe.
            "replaceplan" => {
                let paths: Vec<std::path::PathBuf> = req.params["paths"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str()).map(std::path::PathBuf::from).collect())
                    .unwrap_or_default();
                let spec = req.params["spec"].as_str().unwrap_or("");
                let sub = cian_core::substitute::parse(spec).map_err(|e| anyhow::anyhow!(e))?;
                let (changes, skipped) = cian_core::grepedit::plan(&paths, &sub);
                Ok(serde_json::json!({
                    "changes": changes.iter().map(|c| serde_json::json!({
                        "path": c.path.display().to_string(),
                        "line": c.line, "before": c.before, "after": c.after,
                    })).collect::<Vec<_>>(),
                    "skipped": skipped.iter().map(|s| serde_json::json!({
                        "path": s.path.display().to_string(), "why": s.why,
                    })).collect::<Vec<_>>(),
                }))
            }
            "replaceapply" => {
                let changes: Vec<cian_core::grepedit::Change> = req.params["changes"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| Some(cian_core::grepedit::Change {
                        path: std::path::PathBuf::from(v["path"].as_str()?),
                        line: v["line"].as_u64()? as usize,
                        before: v["before"].as_str()?.to_string(),
                        after: v["after"].as_str()?.to_string(),
                        picked: true,
                    })).collect())
                    .unwrap_or_default();
                if changes.is_empty() {
                    anyhow::bail!("置換する行がありません");
                }
                let r = cian_core::grepedit::apply(&changes);
                for which in ["left", "right"] {
                    let _ = self.pane_mut(which).map(|p| p.reload());
                }
                Ok(serde_json::json!({
                    "files": r.files, "lines": r.lines, "stale": r.stale, "errors": r.errors,
                }))
            }
            // ---- svn, the three that are not shared with git ----
            "svn" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let dir = self.pane_mut(&which)?.cwd.clone();
                if !cian_core::svn::is_working_copy(&dir) {
                    anyhow::bail!("svn の作業コピーではありません");
                }
                let paths = self.targets(&which).unwrap_or_default();
                let what = req.params["what"].as_str().unwrap_or("");
                // These three answer with `()`; what to say is this end's job.
                let said = match what {
                    "update" => {
                        cian_core::svn::update(&dir)?;
                        "svn update しました".to_string()
                    }
                    "commit" => {
                        let msg = req.params["message"].as_str().unwrap_or("");
                        if msg.trim().is_empty() {
                            anyhow::bail!("コミットメッセージがありません");
                        }
                        cian_core::svn::commit(&dir, &paths, msg)?;
                        format!("{} 件を svn commit しました", paths.len())
                    }
                    "resolve" => {
                        cian_core::svn::resolve(&dir, &paths)?;
                        format!("{} 件を解決済みにしました", paths.len())
                    }
                    other => anyhow::bail!("知らない svn 操作: {other}"),
                };
                let pane = self.pane_mut(&which)?;
                pane.reload()?;
                Ok(serde_json::json!({
                    "said": said,
                    "pane": self.view(&which)?,
                }))
            }
            // ---- A server, in this pane ----
            //
            // Not a separate window or a transfer dialog: the rows are rows,
            // and `c`/`m` across to the other pane are an upload or a
            // download. That is the terminal build's arrangement and the
            // reason it is worth having at all.
            "connect" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let target = cian_scp::Target {
                    host: req.params["host"].as_str().unwrap_or("").to_string(),
                    port: req.params["port"].as_u64().unwrap_or(22) as u16,
                    user: req.params["user"].as_str().unwrap_or("").to_string(),
                    password: req.params["password"].as_str().unwrap_or("").to_string(),
                };
                if target.host.is_empty() || target.user.is_empty() {
                    anyhow::bail!("ホストとユーザが要ります");
                }
                let start = req.params["path"].as_str().unwrap_or(".").to_string();
                let (resolved, entries) = cian_scp::list_dir(&target, &start)?;
                let label = format!("{}@{}", target.user, target.host);
                self.remotes.insert(which.clone(), target);
                let rows = remote_rows(&resolved, &entries);
                let pane = self.pane_mut(&which)?;
                pane.enter_remote(label.clone(), resolved.clone(), rows);
                Ok(serde_json::json!({
                    "host": label,
                    "path": resolved,
                    "pane": self.view(&which)?,
                }))
            }
            "remotelist" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let Some(target) = self.remotes.get(&which).cloned() else {
                    anyhow::bail!("このペインはサーバに繋がっていません");
                };
                let (label, here) = {
                    let pane = self.pane_mut(&which)?;
                    let Some((h, p)) = pane.remote_view() else {
                        anyhow::bail!("このペインはサーバを表示していません");
                    };
                    (h.to_string(), p.to_string())
                };
                // A named path, or the row under the cursor, or one level up.
                let want = match req.params["path"].as_str() {
                    Some(p) => p.to_string(),
                    None if req.params["up"].as_bool().unwrap_or(false) => {
                        cian_scp::remote_parent(&here)
                    }
                    None => {
                        let (path, _, is_dir) = self.selected(&which)?;
                        if !is_dir {
                            anyhow::bail!("ディレクトリではありません");
                        }
                        path.display().to_string()
                    }
                };
                let (resolved, entries) = cian_scp::list_dir(&target, &want)?;
                let rows = remote_rows(&resolved, &entries);
                let pane = self.pane_mut(&which)?;
                pane.enter_remote(label, resolved.clone(), rows);
                Ok(serde_json::json!({
                    "path": resolved,
                    "pane": self.view(&which)?,
                }))
            }
            // Copy across when one of the two panes is a server.
            //
            // `c` is `c` either way: the difference between a copy and an
            // upload is which pane you are standing in, and making it a
            // separate command would be asking the person to know something
            // the program already knows.
            "transfer" => {
                let from = req.params["pane"].as_str().unwrap_or("left").to_string();
                let to = if from == "left" { "right" } else { "left" };
                let paths = self.targets(&from)?;
                if paths.is_empty() {
                    anyhow::bail!("対象がありません");
                }
                let up = self.remotes.contains_key(to);
                let down = self.remotes.contains_key(&from);
                let target = self.remotes.get(if up { to } else { &from }).cloned();
                let Some(target) = target else {
                    anyhow::bail!("どちらのペインもサーバではありません");
                };
                if up && down {
                    anyhow::bail!("サーバ同士の転送はできません");
                }
                let dest = if up {
                    self.pane_mut(to)?.remote_view().map(|(_, p)| p.to_string())
                        .ok_or_else(|| anyhow::anyhow!("転送先が分かりません"))?
                } else {
                    self.pane_mut(to)?.cwd.display().to_string()
                };
                let stop = std::sync::atomic::AtomicBool::new(false);
                let (mut ok, mut errors) = (0usize, Vec::new());
                for p in &paths {
                    let name = p.file_name().map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.display().to_string());
                    let mut noop = |_: u64, _: u64| {};
                    let mut ctl = cian_scp::Ctl {
                        cancel: &stop,
                        on_progress: &mut noop,
                        limit_bps: None,
                    };
                    let r = if up {
                        cian_scp::upload(&target, p, &cian_scp::remote_join(&dest, &name), None, &mut ctl)
                            .map(|_| ())
                    } else {
                        // A remote row's `path` is the remote absolute path.
                        cian_scp::download(
                            &target,
                            &p.display().to_string(),
                            &std::path::Path::new(&dest).join(&name),
                            &mut ctl,
                        ).map(|_| ())
                    };
                    match r {
                        Ok(()) => ok += 1,
                        Err(e) => errors.push(format!("{name}: {e}")),
                    }
                }
                // Both sides may have changed; re-read whichever is local.
                for which in ["left", "right"] {
                    if !self.remotes.contains_key(which) {
                        let _ = self.pane_mut(which).map(|p| p.reload());
                    }
                }
                Ok(serde_json::json!({
                    "direction": if up { "up" } else { "down" },
                    "ok": ok,
                    "errors": errors,
                    "left": PaneView::of(self.left.get()),
                    "right": PaneView::of(self.right.get()),
                }))
            }
            // Making, renaming and removing on the server.
            //
            // The same keys as here — `a`, `A`, `r`, `d` — because the rows
            // look the same and behave the same. The one difference is stated
            // rather than hidden: a remote delete is a delete, since SFTP has
            // no trash to put anything in.
            "remoteop" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let Some(target) = self.remotes.get(&which).cloned() else {
                    anyhow::bail!("このペインはサーバに繋がっていません");
                };
                let here = {
                    let pane = self.pane_mut(&which)?;
                    let Some((_, p)) = pane.remote_view() else {
                        anyhow::bail!("このペインはサーバを表示していません");
                    };
                    p.to_string()
                };
                let what = req.params["what"].as_str().unwrap_or("");
                let name = req.params["name"].as_str().unwrap_or("").trim().to_string();
                let said = match what {
                    "mkdir" | "touch" => {
                        if name.is_empty() {
                            anyhow::bail!("名前が空です");
                        }
                        let at = cian_scp::remote_join(&here, &name);
                        if what == "mkdir" {
                            cian_scp::make_dir(&target, &at)?;
                        } else {
                            cian_scp::make_file(&target, &at)?;
                        }
                        format!("{name} を作りました")
                    }
                    "rename" => {
                        if name.is_empty() {
                            anyhow::bail!("名前が空です");
                        }
                        let (from, was, _) = self.selected(&which)?;
                        let from = from.display().to_string();
                        cian_scp::rename(&target, &from, &cian_scp::remote_join(&here, &name))?;
                        format!("{was} → {name}")
                    }
                    "delete" => {
                        let paths = self.targets(&which)?;
                        if paths.is_empty() {
                            anyhow::bail!("対象がありません");
                        }
                        let mut gone = 0usize;
                        for p in &paths {
                            let path = p.display().to_string();
                            // The row knows whether it was a directory; SFTP
                            // needs telling, because rmdir and unlink are two
                            // calls and picking the wrong one just fails.
                            let is_dir = self
                                .pane_mut(&which)?
                                .entries
                                .iter()
                                .find(|e| e.path == *p)
                                .map(|e| e.is_dir)
                                .unwrap_or(false);
                            cian_scp::remove(&target, &path, is_dir)?;
                            gone += 1;
                        }
                        format!("{gone} 件を消しました")
                    }
                    other => anyhow::bail!("知らないリモート操作: {other}"),
                };
                let (_, entries) = cian_scp::list_dir(&target, &here)?;
                let label = format!("{}@{}", target.user, target.host);
                let rows = remote_rows(&here, &entries);
                let pane = self.pane_mut(&which)?;
                pane.enter_remote(label, here, rows);
                let mut reply = self.view(&which)?;
                reply["said"] = serde_json::json!(said);
                Ok(reply)
            }
            "disconnect" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                self.remotes.remove(&which);
                let pane = self.pane_mut(&which)?;
                let home = pane.cwd.clone();
                *pane = Pane::new(home)?;
                self.view(&which)
            }
            // ---- What is remembered between sessions ----
            //
            // The terminal build's own state file, not a second one. A look
            // chosen in the window and not in the terminal would be two
            // programs wearing one name. `init.lua` stays untouched: it is
            // written by hand and read as code, and a program that rewrote it
            // would be reformatting somebody's comments.
            "settings" => Ok(serde_json::json!({
                "look": cian_lua::state_get("gui_look"),
                "style": cian_lua::state_get("gui_editor"),
                "theme": cian_lua::state_get("theme"),
                "where": cian_lua::config_read_path("state.toml")
                    .map(|p| p.display().to_string()),
            })),
            "remember" => {
                let key = req.params["key"].as_str().unwrap_or("");
                let value = req.params["value"].as_str().unwrap_or("");
                if !matches!(key, "gui_look" | "gui_editor") {
                    anyhow::bail!("覚えられない項目です: {key}");
                }
                cian_lua::state_set(key, value);
                Ok(serde_json::json!({ "key": key, "value": value }))
            }
            // ---- The AI, where a site has configured one ----
            //
            // The prompts are the terminal build's, word for word. Two front
            // ends asking the same model differently would give two different
            // answers to the same question, which is the kind of difference
            // nobody can debug.
            "ai" => {
                let cfg = cian_ai::AiConfig::from_lua(&cian_lua::load());
                let Some(cfg) = cfg else {
                    anyhow::bail!("AI が設定されていません（init.lua の cian.ai{{…}}）");
                };
                if !cian_ai::available(&cfg) {
                    anyhow::bail!("AI を利用できません（python・パッケージ・サインインのいずれか）");
                }
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let (system, user) = match req.params["what"].as_str().unwrap_or("") {
                    "cmd" => {
                        let want = req.params["text"].as_str().unwrap_or("");
                        if want.trim().is_empty() {
                            anyhow::bail!("やりたいことを書いてください");
                        }
                        let cwd = self.pane_mut(&which)?.cwd.display().to_string();
                        (
                            "You write a single shell command for the platform named.                              Answer with the command only — no explanation, no code                              fence, no leading prompt character.".to_string(),
                            format!(
                                "Platform: {}\nDirectory: {cwd}\nTask: {want}",
                                std::env::consts::OS
                            ),
                        )
                    }
                    "log" => {
                        let (path, name, is_dir) = self.selected(&which)?;
                        if is_dir {
                            anyhow::bail!("ログファイルを選んでください");
                        }
                        // A log's meaning is at its end — read the tail.
                        let tail = read_tail(&path, 16_000);
                        if tail.trim().is_empty() {
                            anyhow::bail!("{name} は空です");
                        }
                        (
                            "You triage a log file for an operator (often RHEL/AIX or                              Oracle). From the tail below: list the errors and warnings                              that matter, each with its key line; note a rough timeline                              if the timestamps show one; then give the single most                              likely cause and the next thing to check. Ignore routine                              INFO noise. Be concise; plain text, no markdown headings."
                                .to_string(),
                            tail,
                        )
                    }
                    "text" => (
                        req.params["system"].as_str().unwrap_or("Answer concisely.").to_string(),
                        req.params["text"].as_str().unwrap_or("").to_string(),
                    ),
                    other => anyhow::bail!("知らない AI 依頼: {other}"),
                };
                // On a worker, and answered by an event.
                //
                // `chat` waits on a python subprocess talking to a server on
                // somebody else's network. Run here it would hold the whole
                // engine — every keystroke in the listing queued behind a
                // question about a log file. The first attempt did exactly
                // that and looked like a freeze.
                let out = self.out.clone();
                std::thread::spawn(move || match cian_ai::chat(&cfg, &system, &user, &[]) {
                    Ok(answer) => out.event("ai", serde_json::json!({ "answer": answer })),
                    Err(e) => out.event("ai", serde_json::json!({ "error": e.to_string() })),
                });
                Ok(serde_json::json!({ "asked": true }))
            }
            // ---- Bookmarks ----
            //
            // The terminal build's own `shortcuts.lua`, read the same way and
            // written back through the same renderer. A second bookmark list
            // would be the worst of the two-programs problems: the folders you
            // saved would depend on which one you saved them from.
            "shortcuts" => {
                let path = cian_lua::config_read_path("shortcuts.lua");
                let nodes = path
                    .as_ref()
                    .filter(|p| p.exists())
                    .and_then(|p| cian_lua::shortcuts::load(p).ok())
                    .unwrap_or_default();
                fn flatten(nodes: &[cian_lua::shortcuts::Node], depth: usize, out: &mut Vec<serde_json::Value>) {
                    for n in nodes {
                        out.push(serde_json::json!({
                            "name": n.name,
                            "target": n.target,
                            "depth": depth,
                            "group": n.children.is_some(),
                        }));
                        if let Some(kids) = &n.children {
                            flatten(kids, depth + 1, out);
                        }
                    }
                }
                let mut rows = Vec::new();
                flatten(&nodes, 0, &mut rows);
                Ok(serde_json::json!({
                    "where": path.map(|p| p.display().to_string()),
                    "rows": rows,
                }))
            }
            "bookmark" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let cwd = self.pane_mut(&which)?.cwd.clone();
                let name = req.params["name"].as_str().unwrap_or("").trim().to_string();
                let name = if name.is_empty() {
                    cwd.file_name().map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| cwd.display().to_string())
                } else {
                    name
                };
                let path = cian_lua::config_write_path("shortcuts.lua")
                    .ok_or_else(|| anyhow::anyhow!("設定の置き場所が分かりません"))?;
                let mut nodes = if path.exists() {
                    cian_lua::shortcuts::load(&path).unwrap_or_default()
                } else {
                    Vec::new()
                };
                let target = cwd.display().to_string();
                if nodes.iter().any(|n| n.target.as_deref() == Some(target.as_str())) {
                    anyhow::bail!("すでに登録されています");
                }
                nodes.push(cian_lua::shortcuts::Node::leaf(name.clone(), target.clone()));
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                std::fs::write(&path, cian_lua::shortcuts::to_lua(&nodes))?;
                Ok(serde_json::json!({ "name": name, "target": target }))
            }
            // ---- Tabs ----
            //
            // Each side keeps a list, and the active one *is* the pane
            // everywhere else. A new tab opens where you are standing, which
            // is what makes it useful: the reason to open one is almost always
            // "keep this, and go somewhere else for a moment".
            "tabnew" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let here = self.pane_mut(&which)?.cwd.clone();
                let fresh = Pane::new(here)?;
                let side = self.side_mut(&which)?;
                side.tabs.insert(side.at + 1, fresh);
                side.at += 1;
                Ok(serde_json::to_value(PaneView::of_side(side))?)
            }
            "tabclose" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let side = self.side_mut(&which)?;
                if side.tabs.len() <= 1 {
                    anyhow::bail!("最後のタブは閉じられません");
                }
                side.tabs.remove(side.at);
                side.at = side.at.min(side.tabs.len() - 1);
                Ok(serde_json::to_value(PaneView::of_side(side))?)
            }
            "tabgo" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let side = self.side_mut(&which)?;
                let n = side.tabs.len();
                side.at = match req.params["at"].as_i64() {
                    Some(at) => (at.rem_euclid(n as i64)) as usize,
                    None => {
                        let step = req.params["step"].as_i64().unwrap_or(1);
                        ((side.at as i64 + step).rem_euclid(n as i64)) as usize
                    }
                };
                Ok(serde_json::to_value(PaneView::of_side(side))?)
            }
            // What is running, and a way to stop one of them.
            "queue" => Ok(serde_json::json!({ "jobs": self.jobs.listing() })),
            // The files themselves onto the OS clipboard, so Finder or
            // Explorer pastes the files rather than their names. `p` puts the
            // path text there; conflating the two is how you end up pasting a
            // path into a folder.
            "clipfiles" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let paths = self.targets(&which)?;
                if paths.is_empty() {
                    anyhow::bail!("対象がありません");
                }
                cian_core::fileclip::put_files(&paths)?;
                Ok(serde_json::json!({ "count": paths.len() }))
            }
            // ---- Macros ----
            //
            // A layout macro in the terminal build builds a *grid* of shell
            // panes. There are no splits here yet, so each pane becomes a
            // tab. That is a real reduction and it is said out loud rather
            // than papered over: the shells, their commands and their scripted
            // steps all run, and only the arrangement is lost.
            "macros" => {
                let path = cian_lua::config_read_path("macro.lua");
                let list = path
                    .as_ref()
                    .filter(|p| p.exists())
                    .and_then(|p| cian_lua::macros::load(p).ok())
                    .unwrap_or_default();
                Ok(serde_json::json!({
                    "where": path.map(|p| p.display().to_string()),
                    "macros": list.iter().map(|m| serde_json::json!({
                        "name": m.name,
                        "panes": m.panes.len(),
                        "script": m.is_script(),
                        "sync": m.sync,
                    })).collect::<Vec<_>>(),
                }))
            }
            "macrorun" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let want = req.params["name"].as_str().unwrap_or("").to_string();
                let rows = req.params["rows"].as_u64().unwrap_or(24) as u16;
                let cols = req.params["cols"].as_u64().unwrap_or(80) as u16;
                let path = cian_lua::config_read_path("macro.lua")
                    .filter(|p| p.exists())
                    .ok_or_else(|| anyhow::anyhow!("macro.lua がありません"))?;
                let list = cian_lua::macros::load(&path).map_err(|e| anyhow::anyhow!(e))?;
                let Some(mac) = list.into_iter().find(|m| m.name == want) else {
                    anyhow::bail!("{want} というマクロはありません");
                };
                if mac.is_script() {
                    anyhow::bail!("スクリプトマクロはまだ動かせません（レイアウトのみ）");
                }
                let cwd = self.pane_mut(&which)?.cwd.clone();
                // The layout the macro actually asked for. `from` names an
                // earlier pane to split off (1-based), so a macro can build a
                // grid rather than a row — which is the whole reason the field
                // exists.
                let mut made: Vec<u64> = Vec::new();
                let mut root: Option<shell::Node> = None;
                let mut opened = 0usize;
                for step in &mac.panes {
                    let id = self.new_shell(&cwd, rows, cols)?;
                    match &mut root {
                        None => root = Some(shell::Node::Leaf(id)),
                        Some(tree) => {
                            let from = step
                                .from
                                .and_then(|n| made.get(n.saturating_sub(1)).copied())
                                .unwrap_or_else(|| *made.last().unwrap());
                            let down = matches!(step.dir, cian_lua::macros::Split::Down);
                            tree.split_at(from, id, down);
                        }
                    }
                    made.push(id);
                    let sh = self.shells.last().unwrap();
                    // The command and its scripted steps, on a worker: an
                    // `expect` waits for a prompt, and waiting here would hold
                    // the engine for as long as the login takes.
                    let cmd = step.cmd.clone();
                    let steps = step.steps.clone();
                    let handle = sh.handle();
                    std::thread::spawn(move || {
                        if let Some(cmd) = cmd {
                            handle.write(format!("{cmd}\n").as_bytes());
                        }
                        for s in &steps {
                            match s {
                                cian_lua::macros::Step::Send(line) => {
                                    handle.write(format!("{line}\n").as_bytes());
                                }
                                cian_lua::macros::Step::Wait(secs) => {
                                    std::thread::sleep(std::time::Duration::from_secs_f64(*secs));
                                }
                                cian_lua::macros::Step::Expect { text, timeout } => {
                                    handle.wait_for(text, *timeout);
                                }
                            }
                        }
                    });
                    opened += 1;
                }
                let Some(root) = root else {
                    anyhow::bail!("{want} にはペインがありません");
                };
                let focus = made[0];
                self.tabs.push(ShellTab { root, focus, sync: mac.sync });
                self.shell_at = self.tabs.len() - 1;
                let mut reply = self.shell_reply();
                reply["name"] = serde_json::json!(mac.name);
                reply["opened"] = serde_json::json!(opened);
                Ok(reply)
            }
            // Free space on the disk this pane is on.
            "df" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let at = self.pane_mut(&which)?.cwd.clone();
                let d = cian_core::inspect::disk_space(&at)?;
                Ok(serde_json::json!({
                    "where": at.display().to_string(),
                    "total": d.total,
                    "available": d.available,
                    "used": d.used(),
                }))
            }
            // Lines, words and bytes, for the selection.
            "wc" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let paths = self.targets(&which)?;
                let mut rows = Vec::new();
                for p in paths.iter().take(500) {
                    if p.is_dir() {
                        continue;
                    }
                    if let Ok(c) = cian_core::inspect::count(p) {
                        rows.push(serde_json::json!({
                            "name": p.file_name().map(|s| s.to_string_lossy().into_owned()),
                            "lines": c.lines, "words": c.words, "bytes": c.bytes,
                        }));
                    }
                }
                Ok(serde_json::json!({ "rows": rows }))
            }
            // Where cian reads and writes its settings — the question `:where`
            // exists to answer, because a portable copy beside the executable
            // wins and that is not where anybody looks first.
            "where" => Ok(serde_json::json!({
                "config": cian_lua::config_read_path("init.lua").map(|p| p.display().to_string()),
                "state": cian_lua::config_read_path("state.toml").map(|p| p.display().to_string()),
                "shortcuts": cian_lua::config_read_path("shortcuts.lua").map(|p| p.display().to_string()),
                "macros": cian_lua::config_read_path("macro.lua").map(|p| p.display().to_string()),
                "writes": cian_lua::config_write_path("init.lua").map(|p| p.display().to_string()),
            })),
            // Mark by pattern. `*.rs` is what a person types; it becomes a
            // glob rather than a regex, because that is what the asterisk
            // means to everyone who is not writing one.
            "markglob" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pattern = req.params["glob"].as_str().unwrap_or("*").to_string();
                let on = req.params["on"].as_bool().unwrap_or(true);
                let re = glob_to_regex(&pattern)?;
                let pane = self.pane_mut(&which)?;
                let hits: Vec<usize> = pane
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| !e.is_parent && re.is_match(&e.name.to_lowercase()))
                    .map(|(i, _)| i)
                    .collect();
                for i in &hits {
                    if on {
                        pane.set_mark_at(*i);
                    } else if pane.is_marked(*i) {
                        pane.toggle_mark_at(*i);
                    }
                }
                let n = hits.len();
                let mut reply = self.view(&which)?;
                reply["matched"] = serde_json::json!(n);
                Ok(reply)
            }
            // Copy or move to somewhere that is not the other pane.
            "copyto" | "moveto" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let dest = req.params["dest"].as_str().unwrap_or("").trim().to_string();
                if dest.is_empty() {
                    anyhow::bail!("行き先がありません");
                }
                let dest = std::path::PathBuf::from(shellexpand(&dest));
                if !dest.is_dir() {
                    anyhow::bail!("{} はディレクトリではありません", dest.display());
                }
                let paths = self.targets(&which)?;
                if paths.is_empty() {
                    anyhow::bail!("対象がありません");
                }
                let kind = if req.method == "copyto" { Kind::Copy } else { Kind::Move };
                let count = paths.len();
                let op = self.jobs.start(
                    kind, paths, Some(dest.clone()), self.out.clone(), self.undo.clone());
                Ok(serde_json::json!({
                    "op": op, "count": count,
                    "kind": if matches!(kind, Kind::Move) { "move" } else { "copy" },
                    "dest": dest.display().to_string(),
                }))
            }
            // Hand the file to the editor named in init.lua, or the one the
            // environment names.
            "editexternal" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let (path, name, is_dir) = self.selected(&which)?;
                if is_dir {
                    anyhow::bail!("{name} はディレクトリです");
                }
                let editor = std::env::var("VISUAL")
                    .or_else(|_| std::env::var("EDITOR"))
                    .unwrap_or_else(|_| if cfg!(windows) { "notepad".into() } else { "vi".into() });
                cian_core::proc::quiet(&editor)
                    .arg(&path)
                    .spawn()
                    .map_err(|e| anyhow::anyhow!("{editor}: {e}"))?;
                Ok(serde_json::json!({ "editor": editor, "name": name }))
            }
            // ---- Office documents synced from a cloud drive ----
            //
            // `:office` opens the *cloud* copy in the web app; `:officelink`
            // makes a .url pointing at it. The distinction matters at work: a
            // local path in an email is a path only you can open.
            "office" | "officelink" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let (path, name, _) = self.selected(&which)?;
                let cfg = cian_lua::load();
                let Some(url) = cian_core::office::cloud_url(&path, &cian_core::office::SyncMap::from_pairs(&cfg.sharepoint)) else {
                    anyhow::bail!("{name} のクラウド側が分かりません（init.lua の cian.sync{{…}}）");
                };
                if req.method == "officelink" {
                    let at = path.with_extension("url");
                    std::fs::write(&at, cian_core::office::url_shortcut(&url))?;
                    self.undo.push(Undo::Created { path: at.clone() });
                    let pane = self.pane_mut(&which)?;
                    pane.reload()?;
                    let mut reply = self.view(&which)?;
                    reply["made"] = serde_json::json!(
                        at.file_name().map(|s| s.to_string_lossy().into_owned()));
                    return Ok(reply);
                }
                // The app's own URI where there is one — that opens Word
                // rather than a browser tab pretending to be Word.
                let target = cian_core::office::classify(&path)
                    .and_then(|doc| cian_core::office::app_uri(doc, &url))
                    .unwrap_or_else(|| url.clone());
                cian_core::proc::open_with_desktop(&target)?;
                Ok(serde_json::json!({ "opened": name, "url": url }))
            }
            // Re-read init.lua. Says what it could not change, rather than
            // pretending a restart is never needed.
            "reload" => {
                let cfg = cian_lua::load();
                Ok(serde_json::json!({
                    "ai": cfg.ai.is_some(),
                    "sync_maps": cfg.sharepoint.len(),
                    "ssh_hosts": cfg.ssh_hosts.len(),
                }))
            }
            // ---- Hex editing ----
            //
            // Overwrite only. Offsets never shift and the file cannot change
            // size, which is the difference between editing a binary and
            // corrupting one — an inserted byte moves every offset after it,
            // and in a binary those offsets are usually written down inside
            // the file itself.
            "hexset" => {
                let Some((_, view)) = self.hex.as_mut() else {
                    anyhow::bail!("16進で開いているファイルがありません");
                };
                let at = req.params["at"].as_u64().unwrap_or(0) as usize;
                let val = req.params["byte"].as_u64().unwrap_or(0) as u8;
                view.hex_set_byte(at, val);
                let line = at / 16;
                Ok(serde_json::json!({
                    "line": line,
                    "text": view.lines.get(line),
                }))
            }
            // Write the bytes back, keeping the original as `.bak`.
            //
            // The backup is not optional. A hex edit is the one change in here
            // nobody can read back to check, so the version that was working
            // has to survive it.
            "hexsave" => {
                let Some((path, view)) = self.hex.as_ref() else {
                    anyhow::bail!("16進で開いているファイルがありません");
                };
                let bak = path.with_extension(format!(
                    "{}.bak",
                    path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default()
                ));
                std::fs::copy(path, &bak)?;
                std::fs::write(path, view.raw_bytes())?;
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let bak_name = bak
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                for which in ["left", "right"] {
                    let _ = self.pane_mut(which).map(|p| p.reload());
                }
                Ok(serde_json::json!({ "saved": name, "backup": bak_name }))
            }
            // Who last changed each line of the open file.
            "blame" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let dir = self.pane_mut(&which)?.cwd.clone();
                let path = match self.open.as_ref() {
                    Some((p, _)) => p.clone(),
                    None => self.selected(&which)?.0,
                };
                let lines = if cian_core::git::status(&dir).is_some() {
                    cian_core::git::blame(&dir, &path)
                } else {
                    cian_core::svn::blame(&dir, &path)
                };
                let Some(lines) = lines else {
                    anyhow::bail!("blame を取れませんでした");
                };
                Ok(serde_json::json!({
                    "lines": lines.iter().map(|b| serde_json::json!({
                        "hash": b.hash, "author": b.author, "date": b.date,
                    })).collect::<Vec<_>>(),
                }))
            }
            // Read the open file again in another encoding.
            //
            // The bytes are already here, so this decodes rather than reads —
            // which matters because the file on disk may be a log something is
            // still writing to, and re-reading it would show a different file
            // than the one being looked at.
            "encoding" => {
                let Some((path, view)) = self.shown.as_mut() else {
                    anyhow::bail!("開いているファイルがありません");
                };
                let want = match req.params["as"].as_str() {
                    Some("utf8") => cian_core::viewer::TextEncoding::Utf8,
                    Some("sjis") => cian_core::viewer::TextEncoding::ShiftJis,
                    Some("utf16le") => cian_core::viewer::TextEncoding::Utf16Le,
                    Some("utf16be") => cian_core::viewer::TextEncoding::Utf16Be,
                    // No name means the next one round, which is how the
                    // terminal build's `:enc` is used: press it until the
                    // mojibake stops.
                    _ => view.encoding.next(),
                };
                view.redecode(want);
                let path = path.clone();
                let lines = view.lines.clone();
                let enc = format!("{:?}", view.encoding);
                let eol = format!("{:?}", view.eol);
                // The editable copy has to agree, or a save would write back
                // through the encoding the file *was* read with.
                if let Ok(mut f) = cian_core::grepedit::read_text(&path) {
                    f.lines = lines.clone();
                    f.encoding = match want {
                        cian_core::viewer::TextEncoding::Utf8 => cian_core::viewer::TextEncoding::Utf8,
                        other => other,
                    };
                    self.open = Some((path, f));
                }
                Ok(serde_json::json!({ "lines": lines, "encoding": enc, "eol": eol }))
            }
            // `:s/old/new/` inside the open file.
            //
            // The same substitution language as the grep-wide replace, because
            // it is the same question asked of one file instead of many —
            // learning two spellings of `s///` would be absurd.
            "substitute" => {
                let spec = req.params["spec"].as_str().unwrap_or("");
                let lines: Vec<String> = req.params["lines"]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_str().unwrap_or("").to_string()).collect())
                    .unwrap_or_default();
                let sub = cian_core::substitute::parse(spec).map_err(|e| anyhow::anyhow!(e))?;
                let hits = cian_core::substitute::find(&sub, &lines, None);
                if hits.is_empty() {
                    anyhow::bail!("見つかりません");
                }
                let out = cian_core::substitute::apply(&lines, &hits);
                Ok(serde_json::json!({ "lines": out, "changed": hits.len() }))
            }
            // Leave a flat listing and go back to the directory it came from.
            "leaveflat" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                if !pane.is_flat() {
                    anyhow::bail!("一覧はもともとのディレクトリです");
                }
                pane.leave_flat()?;
                self.view(&which)
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
                self.view(&which)
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
                self.view(&which)
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
                let made = if dir && req.params["deep"].as_bool().unwrap_or(false) {
                    // `:mkdir -p a/b/c`. The undo remembers the *outermost*
                    // one made, because removing that removes the chain — and
                    // remembering the innermost would leave the rest behind.
                    let full = at.join(&name);
                    std::fs::create_dir_all(&full)?;
                    let first = name.split(['/', '\\']).next().unwrap_or(&name);
                    at.join(first)
                } else if dir {
                    cian_core::ops::create_dir(&at, &name)?
                } else if req.params["touch"].as_bool().unwrap_or(false) && at.join(&name).exists() {
                    // `:touch` on something that is already there bumps its
                    // time rather than failing, which is what touch means.
                    let full = at.join(&name);
                    std::fs::OpenOptions::new().append(true).open(&full)?;
                    filetime_now(&full)?;
                    full
                } else {
                    cian_core::ops::create_file(&at, &name)?
                };
                self.undo.push(Undo::Created { path: made });
                let pane = self.pane_mut(&which)?;
                pane.reload()?;
                self.view(&which)
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
                self.left.now().reload()?;
                self.right.now().reload()?;
                Ok(serde_json::json!({
                    "said": said,
                    "left": PaneView::of(self.left.get()),
                    "right": PaneView::of(self.right.get()),
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
                self.view(&which)
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
                    "left" => self.left.get().cwd.clone(),
                    _ => self.right.get().cwd.clone(),
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
                self.view(&which)
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

/// A path, safe to hand to a shell.
///
/// Single quotes with the single quote itself escaped the only way `sh`
/// accepts — close, escape, reopen. A space in a path is the common case and
/// an apostrophe in a filename is not rare enough to leave broken.
fn quote(s: &str) -> String {
    if cfg!(windows) {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// What a browser will call this.
///
/// Only the ones a window can actually draw. Anything else is not offered as
/// an image at all, rather than handed over to be shown as a broken one.
fn mime_of(path: &std::path::Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        _ => return None,
    })
}

/// Base64, written out rather than pulled in.
///
/// One dependency avoided is one fewer crate in the offline bundle, and this
/// is twenty lines that have not changed since 1987.
fn b64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Remote entries as pane rows.
///
/// The row's `path` holds the *remote* absolute path, which is what every
/// remote operation needs and what nothing on this disk should ever be asked
/// to open. The `..` row is synthetic; navigation intercepts it.
fn remote_rows(dir: &str, entries: &[cian_scp::RemoteEntry]) -> Vec<cian_core::Entry> {
    let mut rows = vec![cian_core::Entry::remote("..", dir.to_string(), true, 0, true)];
    for e in entries {
        rows.push(cian_core::Entry::remote(
            e.name.clone(),
            cian_scp::remote_join(dir, &e.name),
            e.is_dir,
            e.size,
            false,
        ));
    }
    rows
}

/// The last `cap` bytes of a file, decoded loosely.
///
/// A log's meaning is at its end: the head of a hundred-megabyte log is the
/// day it was created, and the question is always about today.
fn read_tail(path: &std::path::Path, cap: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len > cap {
        let _ = f.seek(SeekFrom::Start(len - cap));
    }
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Set a file's modification time to now.
///
/// `:touch` on a file that already exists means "say this changed", which is
/// what half the uses of touch are for — a build that keys off mtime, a script
/// waiting on a marker.
fn filetime_now(path: &std::path::Path) -> std::io::Result<()> {
    let f = std::fs::OpenOptions::new().append(true).open(path)?;
    f.set_modified(std::time::SystemTime::now())
}

/// A shell-style glob as a regex, anchored.
///
/// `*.rs` is what a person types and `.*\.rs` is what they mean; treating the
/// input as a regex would make `.` match anything and quietly mark the wrong
/// files. Only `*` and `?` are special, which is the whole of what a filename
/// pattern has ever meant.
fn glob_to_regex(pattern: &str) -> anyhow::Result<regex::Regex> {
    let mut out = String::from("^");
    for c in pattern.to_lowercase().chars() {
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            c => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    out.push('$');
    Ok(regex::Regex::new(&out)?)
}

/// `~` at the start means home. Nothing else is expanded: a path typed into a
/// file manager is a path, not a shell line.
fn shellexpand(path: &str) -> String {
    let Some(rest) = path.strip_prefix('~') else { return path.to_string() };
    match std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        Some(home) => format!("{}{rest}", home.to_string_lossy()),
        None => path.to_string(),
    }
}
