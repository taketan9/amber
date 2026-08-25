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
//! flight. Nothing is pushed unasked yet; when the watchers arrive they will
//! come as replies with no `id`.

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};

use cian_core::Pane;

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

/// The two panes, which is the whole of cian's state so far.
struct Session {
    left: Pane,
    right: Pane,
}

impl Session {
    fn new(dir: std::path::PathBuf) -> anyhow::Result<Self> {
        Ok(Session { left: Pane::new(dir.clone())?, right: Pane::new(dir)? })
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
                pane.enter_selected()?;
                Ok(serde_json::to_value(PaneView::of(pane))?)
            }
            "parent" => {
                let which = req.params["pane"].as_str().unwrap_or("left").to_string();
                let pane = self.pane_mut(&which)?;
                pane.go_parent()?;
                Ok(serde_json::to_value(PaneView::of(pane))?)
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
    let mut session = Session::new(start)?;

    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        // A line that is not a request at all still gets an answer, because a
        // front end waiting on an id it will never be sent is the worst way
        // for this to go wrong.
        let reply = match serde_json::from_str::<Request>(&line) {
            Ok(req) => match session.handle(&req) {
                Ok(ok) => serde_json::json!({ "id": req.id, "ok": ok }),
                Err(e) => serde_json::json!({ "id": req.id, "error": e.to_string() }),
            },
            Err(e) => serde_json::json!({ "id": serde_json::Value::Null, "error": format!("bad request: {e}") }),
        };
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    Ok(())
}
