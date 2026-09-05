//! cian's notes, for a machine that cannot run the engine.
//!
//! The window talks to `cian-server` over a pipe: a method name, a JSON
//! object, a JSON answer. **A phone gets the same conversation through a C
//! ABI** — [`cian_call`] takes those two strings and returns that answer.
//!
//! One symbol rather than one per operation, deliberately. Every function a
//! C ABI exports has to be declared again in a bridging header, matched by
//! hand, and kept in step; a second method would otherwise be a change in
//! three places, and the third is in Xcode where nothing here can check it.
//! With one door, adding an operation is a match arm and no header edit.
//!
//! **The judgement is not here.** What a title is, what an excerpt leaves
//! out, what a note is called when it is made — all of that is
//! `cian_core::note`, which the window uses too. This crate is the doorway:
//! strings in, strings out, and nothing decided on the way past. That is the
//! whole reason the notes half of cian was written in the core rather than in
//! the renderer.

use std::ffi::{c_char, CStr, CString};

/// Answer a request. Both arguments are UTF-8 C strings; the answer is a
/// JSON object the caller must hand back to [`cian_free`].
///
/// # Safety
///
/// `method` and `params` must be valid NUL-terminated strings, or null.
/// The returned pointer is owned by the caller and is freed only by
/// [`cian_free`]; it is never null.
#[no_mangle]
pub unsafe extern "C" fn cian_call(method: *const c_char, params: *const c_char) -> *mut c_char {
    // A panic that unwinds across a C ABI is undefined behaviour, and the
    // caller here is an app that must not simply vanish. Anything that goes
    // wrong comes back as an error the phone can show.
    let answer = std::panic::catch_unwind(|| {
        let method = unsafe { cstr(method) };
        let params = unsafe { cstr(params) };
        let params: serde_json::Value = if params.trim().is_empty() {
            serde_json::json!({})
        } else {
            match serde_json::from_str(&params) {
                Ok(v) => v,
                Err(e) => return err(format!("params は JSON ではありません: {e}")),
            }
        };
        match call(&method, &params) {
            Ok(v) => v,
            Err(e) => err(format!("{e:#}")),
        }
    })
    .unwrap_or_else(|_| err("cian が内部で落ちました".into()));
    into_c(answer)
}

/// Give back a string [`cian_call`] returned.
///
/// # Safety
///
/// `p` must be a pointer this library returned and has not already been
/// given back. Null is accepted and does nothing.
#[no_mangle]
pub unsafe extern "C" fn cian_free(p: *mut c_char) {
    if !p.is_null() {
        drop(unsafe { CString::from_raw(p) });
    }
}

unsafe fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

fn into_c(v: serde_json::Value) -> *mut c_char {
    let text = v.to_string();
    // A NUL inside would truncate the answer at the C boundary. It cannot
    // happen — `serde_json` escapes it — but the fallback says so rather than
    // handing back a silently shortened object.
    CString::new(text)
        .unwrap_or_else(|_| CString::new(r#"{"error":"答えに NUL が入りました"}"#).unwrap())
        .into_raw()
}

fn err(why: String) -> serde_json::Value {
    serde_json::json!({ "error": why })
}

fn arg(p: &serde_json::Value, key: &str) -> String {
    p[key].as_str().unwrap_or("").to_string()
}

/// The methods, in Rust terms.
///
/// Separated from the `extern "C"` shell so the tests below run the real
/// thing: they call this, not a pointer dance, and every rule they state is
/// a rule the phone gets.
pub fn call(method: &str, p: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    match method {
        "version" => Ok(serde_json::json!({
            "cian": env!("CARGO_PKG_VERSION"),
            // Whether this build has a desktop under it. The phone's does
            // not, and `delete` to the trash refuses there rather than
            // deleting outright.
            "desktop": cian_core::DESKTOP,
        })),

        // Every note under a directory. The same walk the window asks for.
        "notes" => {
            let dir = std::path::PathBuf::from(arg(p, "path"));
            if !dir.is_dir() {
                anyhow::bail!("{} を開けません", dir.display());
            }
            let limits = cian_core::survey::Limits {
                depth: p["depth"].as_u64().unwrap_or(6) as usize,
                rows: 4000,
                hidden: false,
                ..Default::default()
            };
            let stop = std::sync::atomic::AtomicBool::new(false);
            let (found, walk) = cian_core::note::list(&dir, limits, &stop);
            let book = cian_core::notebook::read(&dir);
            // The favourite shelves: the ones notes are standing on, plus the
            // ones that were made and are still empty. Without the second
            // half a shelf vanishes the moment its last note leaves it, which
            // reads as cian losing the folder.
            let mut shelves: Vec<String> = book.stars.clone();
            for f in &found {
                if let Some(sh) = f.note.star.clone() {
                    // Every level of it: a note on 買い物/週次 means 買い物
                    // exists too, whether or not anything stands on it.
                    let parts: Vec<&str> = sh.split('/').filter(|p| !p.is_empty()).collect();
                    for n in 1..=parts.len() {
                        shelves.push(parts[..n].join("/"));
                    }
                }
            }
            shelves.sort();
            shelves.dedup();
            let notes: Vec<serde_json::Value> = found
                .iter()
                .map(|f| {
                    let n = &f.note;
                    serde_json::json!({
                        "path": n.path.display().to_string(),
                        "rel": f.rel,
                        // The directory it sits in, relative to the root: a
                        // notebook is a directory here, as it is in the window.
                        "book": f.rel.rsplit_once('/').map(|(d, _)| d).unwrap_or(""),
                        "title": n.title,
                        "excerpt": n.excerpt,
                        "tags": n.tags,
                        "updated": n.updated,
                        "created": n.created,
                        "bytes": n.bytes,
                        "star": n.star,
                        // What to match when the phone narrows the list, so
                        // that `#仕事` finds the same notes it finds in the
                        // window. Sent rather than derived on the far side:
                        // deriving it there is how the two answers drift.
                        "search": cian_core::note::haystack(n),
                    })
                })
                .collect();
            // The folders, from the same walk. **Derived from the
            // directories and not from the notes**: a notebook somebody just
            // made is empty, and a list built out of the notes inside would
            // not show it — which looks exactly like the folder not having
            // been made.
            let mut books: Vec<String> = walk
                .rows
                .iter()
                .filter(|r| r.is_dir && r.rel != "attachments")
                .filter(|r| !r.rel.split('/').any(|p| p == "attachments"))
                .map(|r| r.rel.clone())
                .collect();
            books.sort();
            Ok(serde_json::json!({
                "root": dir.display().to_string(),
                "books": books,
                "stars": shelves,
                "colors": book.colors,
                "notes": notes,
                "partial": walk.partial().then(|| serde_json::json!({
                    "whole_to": walk.whole_to(),
                    "stopped": walk.stopped_at.is_some(),
                    "unopened": walk.unopened,
                })),
            }))
        }

        // The text of one note, and what it was when it was read.
        //
        // The stamp comes back with it because the phone has to hand it in
        // again to save: two devices on one synced directory is the case this
        // whole mode exists for, and "was this still the file I opened?" is
        // not a question the caller should have to know how to ask.
        "read" => {
            let path = std::path::PathBuf::from(arg(p, "path"));
            let f = cian_core::grepedit::read_text(&path)?;
            let stamp = cian_core::stamp::of(&path);
            Ok(serde_json::json!({
                "text": f.lines.join("\n"),
                "encoding": format!("{:?}", f.encoding),
                "eol": format!("{:?}", f.eol),
                "bom": f.bom,
                "trailing_eol": f.trailing_eol,
                "stamp": stamp.as_ref().map(stamp_json),
            }))
        }

        // Write a note back, unless somebody else wrote it first.
        //
        // The encoding, the line ending and the trailing newline are the
        // file's own, read again here: a note that arrived as Shift_JIS with
        // CRLF goes back that way, and a phone that saved it as UTF-8 with LF
        // would show the Mac a diff on every line of a file it had not edited.
        "write" => {
            let path = std::path::PathBuf::from(arg(p, "path"));
            let text = arg(p, "text");
            let force = p["force"].as_bool().unwrap_or(false);
            if !force {
                if let Some(expect) = p.get("stamp").and_then(json_stamp) {
                    if cian_core::stamp::changed(&path, &expect) {
                        return Ok(serde_json::json!({
                            "conflict": true,
                            "why": cian_core::stamp::describe(&path, &expect),
                        }));
                    }
                }
            }
            let mut f = match cian_core::grepedit::read_text(&path) {
                Ok(f) => f,
                // A note that is not there yet is a new note, not a failure:
                // the phone writes one it has only just made.
                Err(_) => cian_core::grepedit::TextFile {
                    lines: Vec::new(),
                    encoding: cian_core::viewer::TextEncoding::Utf8,
                    bom: false,
                    eol: cian_core::viewer::Eol::Lf,
                    trailing_eol: true,
                },
            };
            f.lines = text.split('\n').map(|l| l.to_string()).collect();
            cian_core::grepedit::write_text(&path, &f)?;
            Ok(serde_json::json!({
                "ok": true,
                "stamp": cian_core::stamp::of(&path).as_ref().map(stamp_json),
            }))
        }

        // A new note, named and shaped by the same rules the window uses.
        "new" => {
            let dir = std::path::PathBuf::from(arg(p, "dir"));
            let at = cian_core::note::create(
                &dir,
                &arg(p, "title"),
                &cian_core::note::today(),
                &cian_core::note::now_stamp(),
            )?;
            Ok(serde_json::json!({
                "path": at.display().to_string(),
                "name": at.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            }))
        }

        // A note, as things to draw. The reading half of the phone: what a
        // heading *is* is decided in `cian_core::note`, and what a heading
        // *looks like* is decided on the phone. Splitting it the other way
        // would put a Markdown parser somewhere no test can reach.
        "blocks" => {
            let text = if p["text"].is_string() {
                arg(p, "text")
            } else {
                let path = std::path::PathBuf::from(arg(p, "path"));
                cian_core::grepedit::read_text(&path)?.lines.join("\n")
            };
            use cian_core::note::Block;
            // The coloured pieces of a line, worked out here so the window
            // and the phone cannot disagree about what a note says.
            fn runs(text: &str) -> serde_json::Value {
                serde_json::Value::Array(
                    cian_core::note::spans(text)
                        .into_iter()
                        .map(|s| serde_json::json!({ "text": s.text, "color": s.color }))
                        .collect(),
                )
            }
            let out: Vec<serde_json::Value> = cian_core::note::blocks(&text)
                .into_iter()
                .map(|b| match b {
                    Block::Heading { level, text } => serde_json::json!({
                        "kind": "heading", "level": level, "runs": runs(&text), "text": text,
                    }),
                    Block::Paragraph(text) => serde_json::json!({
                        "kind": "paragraph", "runs": runs(&text), "text": text,
                    }),
                    Block::Bullet(text) => serde_json::json!({
                        "kind": "bullet", "runs": runs(&text), "text": text,
                    }),
                    Block::Check { done, text, line } => serde_json::json!({
                        "kind": "check", "done": done, "runs": runs(&text), "text": text, "line": line,
                    }),
                    Block::Numbered { n, text } => serde_json::json!({
                        "kind": "numbered", "n": n, "runs": runs(&text), "text": text,
                    }),
                    Block::Quote(text) => serde_json::json!({
                        "kind": "quote", "runs": runs(&text), "text": text,
                    }),
                    Block::Code { lang, text } => {
                        serde_json::json!({ "kind": "code", "lang": lang, "text": text })
                    }
                    Block::Image { alt, link } => {
                        serde_json::json!({ "kind": "image", "alt": alt, "link": link })
                    }
                    Block::Rule => serde_json::json!({ "kind": "rule" }),
                })
                .collect();
            Ok(serde_json::json!({ "blocks": out }))
        }

        // Put a note on a favourite shelf, or take it off.
        //
        // Text in, text out, like every other edit: a favourite is a line in
        // the note's own front matter, so it travels with the note and the
        // Mac reads the same word.
        "star" => {
            let text = arg(p, "text");
            let shelf = p["shelf"].as_str();
            let out = match shelf {
                // `star: true` rather than `star:` — a field with nothing
                // after it reads as yes to the next thing that looks.
                Some("") => cian_core::note::set_field(&text, "star", Some("true")),
                Some(sh) => cian_core::note::set_field(&text, "star", Some(sh)),
                None => cian_core::note::set_field(&text, "star", None),
            };
            // The one written before favourites had a name. Left behind, it
            // would keep the note a favourite after it was taken off one.
            let out = cian_core::note::set_field(&out, "pinned", None);
            Ok(serde_json::json!({ "text": out }))
        }

        // Make or forget a favourite shelf. Only the empty ones need saying
        // out loud — the rest are named by the notes standing on them.
        "shelf" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let name = arg(p, "name");
            if p["drop"].as_bool().unwrap_or(false) {
                cian_core::notebook::drop_star(&root, &name)?;
            } else {
                cian_core::notebook::add_star(&root, &name)?;
            }
            Ok(serde_json::json!({ "stars": cian_core::notebook::read(&root).stars }))
        }

        // What colour a folder is. His to choose — cian offers a palette and
        // does not insist on it.
        "color" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let folder = arg(p, "folder");
            cian_core::notebook::set_color(&root, &folder, p["color"].as_str())?;
            Ok(serde_json::json!({ "colors": cian_core::notebook::read(&root).colors }))
        }

        // Wrap a piece of text in a colour, the way cian writes it. Here
        // and not in the front ends: the notation is one decision, and two
        // places that write it are two notations one edit apart.
        "paint" => {
            Ok(serde_json::json!({
                "text": cian_core::note::paint(&arg(p, "text"), &arg(p, "color")),
            }))
        }
        // Everything, moved to a new home — `notebook::migrate`, the
        // same one the window calls. Copy, check, then remove; and
        // nothing is overwritten.
        "migrate" => {
            let from = std::path::PathBuf::from(arg(p, "from"));
            let to = std::path::PathBuf::from(arg(p, "to"));
            Ok(serde_json::json!({ "moved": cian_core::notebook::migrate(&from, &to)? }))
        }

        // A backup, put back — `notebook::restore`, the same one the
        // window calls.
        "restore" => {
            let zip = std::path::PathBuf::from(arg(p, "zip"));
            let to = std::path::PathBuf::from(arg(p, "to"));
            let (put, kept) = cian_core::notebook::restore(&zip, &to)?;
            Ok(serde_json::json!({ "put": put, "kept": kept }))
        }

        "book" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let root = root.canonicalize()?;
            let from = root.join(arg(p, "book"));
            let inside = |at: &std::path::Path| -> anyhow::Result<std::path::PathBuf> {
                let full = at.canonicalize()?;
                if !full.starts_with(&root) {
                    anyhow::bail!("ノートの外は触れません");
                }
                Ok(full)
            };
            let from = inside(&from)?;
            if p["drop"].as_bool().unwrap_or(false) {
                let n = cian_core::note::list(
                    &from,
                    cian_core::survey::Limits { depth: 9, rows: 9999, hidden: false, ..Default::default() },
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .0
                .len();
                std::fs::remove_dir_all(&from)?;
                return Ok(serde_json::json!({ "gone": n }));
            }
            let name = arg(p, "name");
            let name = name.trim();
            if name.is_empty() || name.contains('/') || name.contains('\\') {
                anyhow::bail!("フォルダの名前に使えません");
            }
            let to = from
                .parent()
                .map(|d| d.join(name))
                .ok_or_else(|| anyhow::anyhow!("いちばん外側は変えられません"))?;
            if to.exists() {
                anyhow::bail!("{name} はもうあります");
            }
            std::fs::rename(&from, &to)?;
            Ok(serde_json::json!({ "path": to.display().to_string() }))
        }

        // What a search box means, as groups of words.
        //
        // Asked once per query rather than once per note: the *meaning* of
        // the query is the decision, and it belongs here; running it over a
        // list that is already in the phone's memory does not.
        "terms" => {
            Ok(serde_json::json!({ "groups": cian_core::note::terms(&arg(p, "q")) }))
        }

        // Split a note into how it describes itself and what it says.
        //
        // **So the writing half can show only the second part.** The front
        // matter is cian's bookkeeping — the title it derived, the date it
        // stamped, the tags set from a sheet — and a person who did not type
        // it should not have to scroll past it to reach their own first line.
        // Where it ends is `note::front`'s answer, the same one the reading
        // half and the window use.
        "split" => {
            let text = arg(p, "text");
            let lines: Vec<String> = text.lines().map(str::to_string).collect();
            let n = cian_core::note::front(&lines).lines;
            // Rebuilt from the lines rather than sliced by bytes: the note
            // may end without a newline, and the head must keep its own.
            let head = if n == 0 {
                String::new()
            } else {
                let mut h = lines[..n].join("\n");
                h.push('\n');
                h
            };
            let body = text.get(head.len()..).unwrap_or("").to_string();
            Ok(serde_json::json!({ "head": head, "body": body }))
        }

        // Tick or untick one task, by the line it is on.
        //
        // Text in, text out, like every other edit here: the caller saves it
        // the ordinary way, so pressing a checkbox goes through the same
        // check against the file on disk as typing does. That matters more
        // here than anywhere — a checkbox is the one edit somebody makes
        // without looking at the note.
        "check" => {
            let text = arg(p, "text");
            let line = p["line"].as_u64().unwrap_or(0) as usize;
            let done = p["done"].as_bool().unwrap_or(false);
            Ok(serde_json::json!({ "text": cian_core::note::set_check(&text, line, done) }))
        }

        // Look inside the notes, not only at what the listing already knows.
        //
        // The listing carries a line per note — title, tags, the first
        // hundred characters — and that is what the search field narrows
        // against instantly. It is not enough: the sentence you remember is
        // usually further down. This walks the files, and it is
        // `cian_core::search`, the same one `:grep` uses in the window.
        "find" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let needle = arg(p, "needle");
            if needle.trim().is_empty() {
                return Ok(serde_json::json!({ "hits": [] }));
            }
            let q = cian_core::search::Query::content(needle);
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let cap = p["limit"].as_u64().unwrap_or(200) as usize;
            let mut hits: Vec<serde_json::Value> = Vec::new();
            // One line per note, the first that matched: a phone shows a row
            // per note, and twenty rows of the same note is a worse answer
            // than one.
            let mut seen: std::collections::HashSet<std::path::PathBuf> =
                std::collections::HashSet::new();
            cian_core::search::search(&root, &q, &cancel, &mut |h| {
                if hits.len() >= cap || h.is_dir || !seen.insert(h.path.clone()) {
                    return;
                }
                let md = h
                    .path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
                    .unwrap_or(false);
                if !md {
                    return;
                }
                let (line, text) = h.line.unwrap_or((0, String::new()));
                hits.push(serde_json::json!({
                    "path": h.path.display().to_string(),
                    "line": line,
                    "text": text.trim(),
                }));
            });
            Ok(serde_json::json!({ "hits": hits }))
        }

        // Tags on, tags off. Text in and text out: the caller saves the
        // result the way it saves any other edit, so tagging goes through the
        // same conflict check as typing. A tagger that wrote the file itself
        // would be a second way to write a note, and the second way is the one
        // that loses somebody else's paragraph.
        "settags" => {
            let tags: Vec<String> = p["tags"]
                .as_array()
                .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
                .unwrap_or_default();
            Ok(serde_json::json!({
                "text": cian_core::note::set_tags(&arg(p, "text"), &tags),
            }))
        }

        // One plain field on or off — `pinned` today, whatever tomorrow.
        // Text in, text out, so it saves like any other edit.
        "setfield" => {
            let value = p["value"].as_str();
            Ok(serde_json::json!({
                "text": cian_core::note::set_field(&arg(p, "text"), &arg(p, "key"), value),
            }))
        }

        // Move a note into another notebook, pictures and all.
        "move" => {
            let note = std::path::PathBuf::from(arg(p, "path"));
            let dir = std::path::PathBuf::from(arg(p, "dir"));
            let at = cian_core::note::move_to(&note, &dir)?;
            Ok(serde_json::json!({ "path": at.display().to_string() }))
        }

        // Make a notebook. A folder, because that is what a notebook is
        // here — somebody looking at the same place from a Mac sees folders.
        "mkbook" => {
            let dir = std::path::PathBuf::from(arg(p, "dir"));
            if dir.as_os_str().is_empty() {
                anyhow::bail!("名前がありません");
            }
            if dir.exists() {
                anyhow::bail!("{} はもうあります", dir.display());
            }
            std::fs::create_dir_all(&dir)?;
            Ok(serde_json::json!({ "path": dir.display().to_string() }))
        }

        // A backup, as a zip somebody can put anywhere.
        //
        // The scope is chosen by the caller and the answer is a file: cian
        // does not know about clouds, and the phone's own share sheet knows
        // about all of them. `zip` and not a folder copy, because a folder is
        // not a thing you can hand to a mail app.
        //
        // `all` — everything under the notes root, pictures included.
        // `book` — one notebook and what is under it.
        // `tag`  — every note carrying a tag, wherever it lives.
        // `note` — one file.
        "backup" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let scope = arg(p, "scope");
            let what = arg(p, "what");
            let mut sources: Vec<std::path::PathBuf> = Vec::new();
            let mut name = String::from("cian");
            match scope.as_str() {
                "all" => sources.push(root.clone()),
                "book" => {
                    sources.push(root.join(&what));
                    name = what.replace('/', "-");
                }
                "note" => {
                    sources.push(std::path::PathBuf::from(&what));
                    name = std::path::Path::new(&what)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "note".into());
                }
                "tag" => {
                    let limits = cian_core::survey::Limits {
                        depth: 6, rows: 4000, hidden: false, ..Default::default()
                    };
                    let stop = std::sync::atomic::AtomicBool::new(false);
                    let (found, _) = cian_core::note::list(&root, limits, &stop);
                    for f in &found {
                        if f.note.tags.iter().any(|t| t == &what) {
                            sources.push(f.note.path.clone());
                        }
                    }
                    if sources.is_empty() {
                        anyhow::bail!("#{what} のノートがありません");
                    }
                    name = format!("tag-{what}");
                }
                other => anyhow::bail!("知らない範囲: {other}"),
            }
            for s in &sources {
                if !s.exists() {
                    anyhow::bail!("{} がありません", s.display());
                }
            }
            // Beside the app's own temporary files, named for what is in it
            // and the day — a folder of `backup.zip` is a folder of one
            // question: which one is which.
            let dir = std::path::PathBuf::from(arg(p, "into"));
            let dir = if dir.as_os_str().is_empty() { std::env::temp_dir() } else { dir };
            std::fs::create_dir_all(&dir)?;
            let at = dir.join(format!("{name}-{}.zip", cian_core::note::today()));
            let _ = std::fs::remove_file(&at);
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let mut nothing = |_: &cian_core::progress::Progress| {};
            let mut ctl = cian_core::progress::Ctl { cancel: &cancel, on_progress: &mut nothing };
            let report = cian_core::archive::create_zip(&sources, &at, None, &mut ctl);
            if !report.errors.is_empty() {
                anyhow::bail!("{}", report.errors.join(" / "));
            }
            Ok(serde_json::json!({
                "path": at.display().to_string(),
                "files": report.ok,
            }))
        }

        // What a note wants to be reminded about, and what its routine owes.
        "remind" => {
            let text = if p["text"].is_string() {
                arg(p, "text")
            } else {
                cian_core::grepedit::read_text(&std::path::PathBuf::from(arg(p, "path")))?
                    .lines
                    .join("\n")
            };
            let r = cian_core::note::remind(&text);
            let today = chrono_today();
            let due: Vec<String> = match r.every {
                Some((every, _, _)) => cian_core::note::due_since(every, r.last, today)
                    .iter()
                    .map(|d| d.to_string())
                    .collect(),
                None => Vec::new(),
            };
            Ok(serde_json::json!({
                "once": r.once.map(|t| t.format("%Y-%m-%d %H:%M").to_string()),
                "every": r.every.map(|(e, h, m)| serde_json::json!({
                    "kind": match e {
                        cian_core::note::Every::Daily => "daily",
                        cian_core::note::Every::Weekly(_) => "weekly",
                        cian_core::note::Every::Monthly(_) => "monthly",
                    },
                    "n": match e {
                        cian_core::note::Every::Daily => 0,
                        cian_core::note::Every::Weekly(w) => w,
                        cian_core::note::Every::Monthly(d) => d,
                    },
                    "hour": h,
                    "minute": m,
                })),
                "last": r.last.map(|d| d.to_string()),
                "due": due,
            }))
        }

        // Carry out a routine for a day it came due, and write down that it
        // was done. Two steps in one call: a copy made without the note of it
        // is a copy that gets made again tomorrow.
        "carryout" => {
            let path = std::path::PathBuf::from(arg(p, "path"));
            let on = arg(p, "on");
            let Some(day) = chrono::NaiveDate::parse_from_str(&on, "%Y-%m-%d").ok() else {
                anyhow::bail!("日付が読めません: {on}")
            };
            let made = cian_core::note::carry_out(&path, day)?;
            let text = std::fs::read_to_string(&path)?;
            std::fs::write(&path, cian_core::note::set_field(&text, "last", Some(&on)))?;
            Ok(serde_json::json!({ "path": made.display().to_string() }))
        }

        // A photo, put beside the note. The phone sends it base64 because
        // that is what fits down a C string; everything about *where it goes*
        // is `cian_core::note::attach`, the same call the window makes when a
        // screenshot is pasted into the editor.
        "image" => {
            let note = std::path::PathBuf::from(arg(p, "note"));
            let bytes = b64(&arg(p, "b64")).ok_or_else(|| anyhow::anyhow!("画像を読めません"))?;
            let link = cian_core::note::attach(&note, &bytes, &arg(p, "ext"))?;
            Ok(serde_json::json!({ "link": link, "bytes": bytes.len() }))
        }

        // Remove a note.
        //
        // Outright, because there is no trash on a phone to move it to —
        // `cian_core::DESKTOP` is false here and `DeleteMode::Trash` refuses
        // rather than pretending. The caller is expected to have asked first;
        // this is the part that cannot be taken back.
        "delete" => {
            let path = std::path::PathBuf::from(arg(p, "path"));
            if !path.is_file() {
                anyhow::bail!("{} がありません", path.display());
            }
            std::fs::remove_file(&path)?;
            Ok(serde_json::json!({ "ok": true }))
        }

        other => anyhow::bail!("知らない操作: {other}"),
    }
}

fn chrono_today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// Base64, in. Written here rather than pulled in: it is twenty lines, and a
/// dependency that exists to decode one field is a dependency the iOS build
/// has to carry to a phone.
fn b64(text: &str) -> Option<Vec<u8>> {
    // A data: URL is what a browser hands over, and the phone may as well be
    // allowed to send one.
    let text = match text.find(',') {
        Some(at) if text.starts_with("data:") => &text[at + 1..],
        _ => text,
    };
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in text.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        let v = TABLE.iter().position(|&t| t == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// The stamp as the one string the caller stores and hands back.
///
/// It used to go out as `{len, modified: <seconds>}`, which read well and was
/// wrong: a time rounded to the second no longer equals the file it came
/// from, so **every** save came back as a conflict with nobody. The test
/// below caught it. `cian_core::stamp::token` keeps it exact, and the phone
/// never has to know what is inside.
fn stamp_json(s: &cian_core::stamp::Stamp) -> serde_json::Value {
    serde_json::Value::String(cian_core::stamp::token(s))
}

fn json_stamp(v: &serde_json::Value) -> Option<cian_core::stamp::Stamp> {
    cian_core::stamp::from_token(v.as_str()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_dir() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("a.md"),
            "---\ntitle: 段取り\ntags: [仕事]\n---\n本文です。\n",
        )
        .unwrap();
        std::fs::write(d.path().join("b.txt"), "not a note\n").unwrap();
        d
    }

    #[test]
    fn a_note_crosses_as_things_to_draw() {
        let r = call("blocks", &serde_json::json!({
            "text": "---\ntitle: x\n---\n# 題\n本文。\n![](a.jpg)\n",
        })).unwrap();
        let b = r["blocks"].as_array().unwrap();
        assert_eq!(b[0]["kind"], "heading");
        assert_eq!(b[0]["level"], 1);
        assert_eq!(b[1]["text"], "本文。");
        assert_eq!(b[2]["kind"], "image");
        assert_eq!(b[2]["link"], "a.jpg");
        assert_eq!(b.len(), 3, "the front matter does not cross");
    }

    #[test]
    fn the_sentence_you_remember_is_usually_further_down() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("long.md"),
            // The word is past where an excerpt would stop, which is the whole
            // reason this method exists.
            format!("---\ntitle: 長いノート\n---\n{}\n合言葉はここ。\n", "埋草。".repeat(80)),
        )
        .unwrap();
        std::fs::write(d.path().join("other.txt"), "合言葉はここ。\n").unwrap();

        let r = call("find", &serde_json::json!({
            "path": d.path().to_str().unwrap(), "needle": "合言葉",
        })).unwrap();
        let hits = r["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1, "the .txt is not a note: {hits:?}");
        assert!(hits[0]["path"].as_str().unwrap().ends_with("long.md"));
        assert_eq!(hits[0]["text"], "合言葉はここ。");

        // Nothing to look for is not "every note".
        let r = call("find", &serde_json::json!({
            "path": d.path().to_str().unwrap(), "needle": "  ",
        })).unwrap();
        assert!(r["hits"].as_array().unwrap().is_empty());
    }

    #[test]
    fn an_empty_notebook_is_still_a_notebook() {
        let d = note_dir();
        let made = call("mkbook", &serde_json::json!({
            "dir": d.path().join("仕事").to_str().unwrap(),
        })).unwrap();
        assert!(made["path"].as_str().unwrap().ends_with("仕事"));

        let r = call("notes", &serde_json::json!({ "path": d.path().to_str().unwrap() })).unwrap();
        let books: Vec<String> = r["books"].as_array().unwrap()
            .iter().map(|b| b.as_str().unwrap().to_string()).collect();
        // Nothing is in it yet, and it still has to show — otherwise making a
        // folder looks exactly like the folder not having been made.
        assert!(books.contains(&"仕事".to_string()), "{books:?}");
        // `attachments` is where the pictures live, not a notebook.
        let note = d.path().join("a.md");
        cian_core::note::attach(&note, &[1], "png").unwrap();
        let r = call("notes", &serde_json::json!({ "path": d.path().to_str().unwrap() })).unwrap();
        let books: Vec<String> = r["books"].as_array().unwrap()
            .iter().map(|b| b.as_str().unwrap().to_string()).collect();
        assert!(!books.iter().any(|b| b.contains("attachments")), "{books:?}");

        // Making one that is already there is refused rather than silently
        // doing nothing, which would read as success.
        assert!(call("mkbook", &serde_json::json!({
            "dir": d.path().join("仕事").to_str().unwrap(),
        })).is_err());
    }

    #[test]
    fn a_backup_is_a_file_somebody_can_hand_to_something_else() {
        let d = note_dir();
        std::fs::create_dir_all(d.path().join("仕事")).unwrap();
        std::fs::write(
            d.path().join("仕事/x.md"),
            "---\ntitle: x\ntags: [家]\n---\n本文\n",
        )
        .unwrap();
        let out = tempfile::tempdir().unwrap();
        let root = d.path().to_str().unwrap();

        for (scope, what) in [("all", ""), ("book", "仕事"), ("tag", "家")] {
            let r = call("backup", &serde_json::json!({
                "path": root, "scope": scope, "what": what,
                "into": out.path().to_str().unwrap(),
            })).unwrap();
            let at = std::path::PathBuf::from(r["path"].as_str().unwrap());
            assert!(at.is_file(), "{scope}: {at:?}");
            // Named for what is in it and the day: a folder of `backup.zip`
            // is a folder of one question.
            assert!(at.to_string_lossy().contains(&cian_core::note::today()), "{at:?}");
            assert!(std::fs::metadata(&at).unwrap().len() > 0, "{scope} は空でした");
        }

        // A tag nobody used is an error, not an empty zip that looks like a
        // backup until the day somebody needs it.
        assert!(call("backup", &serde_json::json!({
            "path": root, "scope": "tag", "what": "ない",
            "into": out.path().to_str().unwrap(),
        })).is_err());
    }

    #[test]
    fn a_routine_is_carried_out_once_and_written_down() {
        let d = tempfile::tempdir().unwrap();
        let t = d.path().join("ごみ.md");
        std::fs::write(&t, "---\ntitle: ごみ\nrepeat: weekly wed 09:00\nlast: 2026-08-30\n---\n本文\n").unwrap();
        let path = t.to_str().unwrap();

        let r = call("remind", &serde_json::json!({ "path": path })).unwrap();
        assert_eq!(r["every"]["kind"], "weekly");
        assert_eq!(r["every"]["hour"], 9);
        assert_eq!(r["last"], "2026-08-30");

        let made = call("carryout", &serde_json::json!({ "path": path, "on": "2026-09-02" })).unwrap();
        assert!(made["path"].as_str().unwrap().ends_with("ごみ 2026-09-02.md"));
        // Written down, or it happens again tomorrow.
        let after = call("remind", &serde_json::json!({ "path": path })).unwrap();
        assert_eq!(after["last"], "2026-09-02");
    }

    #[test]
    fn a_photo_is_written_beside_the_note_and_the_link_finds_it() {
        let d = note_dir();
        let note = d.path().join("a.md").to_str().unwrap().to_string();
        // "aGk=" is "hi". Padding and a data: prefix both have to survive the
        // trip, because that is what the two callers actually send.
        let r = call("image", &serde_json::json!({
            "note": note, "b64": "data:image/png;base64,aGk=", "ext": "png",
        })).unwrap();
        let link = r["link"].as_str().unwrap();
        assert_eq!(r["bytes"], 2);
        assert_eq!(std::fs::read(d.path().join(link)).unwrap(), b"hi");
        // Nothing to attach does not silently make an empty file that the
        // link in the note would then point at.
        assert!(call("image", &serde_json::json!({ "note": note, "b64": "" })).is_err());
    }

    #[test]
    fn deleting_removes_the_note_and_says_so_when_there_is_none() {
        let d = note_dir();
        let note = d.path().join("a.md").to_str().unwrap().to_string();
        assert_eq!(call("delete", &serde_json::json!({ "path": note })).unwrap()["ok"], true);
        assert!(!d.path().join("a.md").exists());
        // Twice is an error, not a second silent success — the caller asked
        // to remove something that is not there.
        assert!(call("delete", &serde_json::json!({ "path": note })).is_err());
    }

    #[test]
    fn the_phone_is_told_what_to_match_not_left_to_work_it_out() {
        let d = note_dir();
        let r = call("notes", &serde_json::json!({ "path": d.path().to_str().unwrap() })).unwrap();
        let notes = r["notes"].as_array().unwrap();
        assert_eq!(notes.len(), 1, "only the Markdown one is a note");
        assert_eq!(notes[0]["title"], "段取り");
        // The same line the window filters on, so `#仕事` narrows to the same
        // notes on both. Derived on the far side, these would drift.
        let search = notes[0]["search"].as_str().unwrap();
        assert!(search.contains("#仕事"), "{search}");
        assert!(search.contains("段取り"), "{search}");
    }

    #[test]
    fn a_note_read_and_written_back_is_the_same_file() {
        let d = note_dir();
        let path = d.path().join("a.md").to_str().unwrap().to_string();
        let before = std::fs::read(d.path().join("a.md")).unwrap();

        let r = call("read", &serde_json::json!({ "path": path })).unwrap();
        let text = r["text"].as_str().unwrap().to_string();
        let stamp = r["stamp"].clone();

        let w = call(
            "write",
            &serde_json::json!({ "path": path, "text": text, "stamp": stamp }),
        )
        .unwrap();
        assert_eq!(w["ok"], true, "{w}");
        assert_eq!(
            std::fs::read(d.path().join("a.md")).unwrap(),
            before,
            "a round trip that changes the bytes would show the Mac a diff on \
             every line of a file the phone did not edit"
        );
    }

    #[test]
    fn saving_over_someone_elses_writing_is_refused_not_done() {
        let d = note_dir();
        let path = d.path().join("a.md").to_str().unwrap().to_string();
        let r = call("read", &serde_json::json!({ "path": path })).unwrap();
        let stamp = r["stamp"].clone();

        // The other device. Longer, so the stamp differs even where the
        // filesystem's timestamps are coarse.
        std::fs::write(d.path().join("a.md"), "むこうで書き足した行\nもう一行\nさらに\n").unwrap();

        let w = call(
            "write",
            &serde_json::json!({ "path": path, "text": "こちらの版", "stamp": stamp }),
        )
        .unwrap();
        assert_eq!(w["conflict"], true, "{w}");
        assert!(
            std::fs::read_to_string(d.path().join("a.md")).unwrap().contains("むこうで"),
            "and the other person's writing is still there"
        );

        // `force` is how the person says they looked and meant it anyway.
        let w = call(
            "write",
            &serde_json::json!({ "path": path, "text": "こちらの版", "stamp": stamp, "force": true }),
        )
        .unwrap();
        assert_eq!(w["ok"], true, "{w}");
    }

    #[test]
    fn a_new_note_is_made_where_it_was_asked_for() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("まだ無い");
        let r = call(
            "new",
            &serde_json::json!({ "dir": sub.to_str().unwrap(), "title": "段取り" }),
        )
        .unwrap();
        assert_eq!(r["name"], "段取り.md");
        assert!(sub.join("段取り.md").is_file(), "including the directory");
        // Twice on the same day does not overwrite the first one.
        let r2 = call(
            "new",
            &serde_json::json!({ "dir": sub.to_str().unwrap(), "title": "段取り" }),
        )
        .unwrap();
        assert_eq!(r2["name"], "段取り-2.md");
    }

    #[test]
    fn a_coloured_word_comes_back_as_pieces_the_drawer_can_use() {
        let text = "ふつうと<span style=\"color:#0E93A8\">シアン</span>。\n";
        let bs = call("blocks", &serde_json::json!({ "text": text })).unwrap();
        let runs = bs["blocks"][0]["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0]["color"], serde_json::Value::Null);
        assert_eq!(runs[1]["text"], "シアン");
        assert_eq!(runs[1]["color"], "#0e93a8");

        // 書く側も同じ1か所から。
        let out = call("paint", &serde_json::json!({ "text": "ここ", "color": "#d9822b" })).unwrap();
        assert_eq!(out["text"], "<span style=\"color:#d9822b\">ここ</span>");
    }

    #[test]
    fn a_favourite_is_a_second_place_and_not_a_move() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("仕事")).unwrap();
        let note = root.join("仕事").join("週報.md");
        std::fs::write(&note, "---\ntitle: 週報\n---\n本文。\n").unwrap();

        // 棚に載せても、ノートは 仕事 フォルダから動かない。
        let text = std::fs::read_to_string(&note).unwrap();
        let out = call("star", &serde_json::json!({ "text": text, "shelf": "買い物/週次" })).unwrap();
        std::fs::write(&note, out["text"].as_str().unwrap()).unwrap();

        let all = call("notes", &serde_json::json!({ "path": root.display().to_string() })).unwrap();
        let n = &all["notes"].as_array().unwrap()[0];
        assert_eq!(n["book"], "仕事", "実体のフォルダは変わらない");
        assert_eq!(n["star"], "買い物/週次");
        // 途中の棚も存在する。
        let stars: Vec<String> = all["stars"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(stars, vec!["買い物".to_string(), "買い物/週次".to_string()]);

        // 空の棚は、設定ファイルが憶えている。
        call("shelf", &serde_json::json!({ "path": root.display().to_string(), "name": "あとで" })).unwrap();
        let all = call("notes", &serde_json::json!({ "path": root.display().to_string() })).unwrap();
        assert!(all["stars"].as_array().unwrap().iter().any(|v| v == "あとで"));

        // 外すと、古い `pinned` も一緒に消える。
        let text = std::fs::read_to_string(&note).unwrap();
        let text = text.replace("---\ntitle:", "---\npinned: true\ntitle:");
        let out = call("star", &serde_json::json!({ "text": text })).unwrap();
        let text = out["text"].as_str().unwrap();
        assert!(!text.contains("pinned"), "{text}");
        assert!(!text.contains("star"), "{text}");
    }

    #[test]
    fn moving_everywhere_copies_first_and_never_overwrites() {
        let d = tempfile::tempdir().unwrap();
        let from = d.path().join("いま");
        let to = d.path().join("あたらしい");
        std::fs::create_dir_all(from.join("仕事")).unwrap();
        std::fs::create_dir_all(from.join("attachments")).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join("a.md"), "---\ntitle: a\n---\n本文\n").unwrap();
        std::fs::write(from.join("仕事").join("b.md"), "---\ntitle: b\n---\n本文\n").unwrap();
        // 画像もついていく ── 置いていったら、絵のあるノートが全部壊れる。
        std::fs::write(from.join("attachments").join("p.png"), [0u8; 4]).unwrap();

        let out = call("migrate", &serde_json::json!({
            "from": from.display().to_string(), "to": to.display().to_string(),
        })).unwrap();
        assert_eq!(out["moved"], 3);
        assert!(to.join("仕事").join("b.md").is_file());
        assert!(to.join("attachments").join("p.png").is_file());
        assert!(!from.join("a.md").exists());

        // 同じ名前が向こうにあったら、**何も動かさない**。
        let again = d.path().join("もどす");
        std::fs::create_dir_all(&again).unwrap();
        std::fs::write(again.join("a.md"), "べつの中身\n").unwrap();
        std::fs::write(to.join("a.md"), "こちら\n").unwrap();
        assert!(call("migrate", &serde_json::json!({
            "from": to.display().to_string(), "to": again.display().to_string(),
        })).is_err());
        assert_eq!(std::fs::read_to_string(again.join("a.md")).unwrap(), "べつの中身\n");
    }

    #[test]
    fn a_backup_goes_back_without_treading_on_what_is_there() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("ノート");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.md"), "---\ntitle: a\n---\nむかしの\n").unwrap();
        let out = call("backup", &serde_json::json!({
            "path": root.display().to_string(), "scope": "all", "what": "",
            "into": d.path().display().to_string(),
        })).unwrap();
        let zip = out["path"].as_str().unwrap().to_string();

        // いまのノートを書き換えてから戻す ── 上書きしないこと。
        std::fs::write(root.join("a.md"), "きょうの\n").unwrap();
        let back = call("restore", &serde_json::json!({
            "zip": zip, "to": root.display().to_string(),
        })).unwrap();
        assert_eq!(back["kept"].as_u64().unwrap_or(0), 1, "{back}");
        assert_eq!(std::fs::read_to_string(root.join("a.md")).unwrap(), "きょうの\n");
    }

    #[test]
    fn a_folder_can_be_renamed_or_thrown_away_but_only_inside_the_notes() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("仕事")).unwrap();
        std::fs::write(root.join("仕事").join("a.md"), "---\ntitle: a\n---\n本文\n").unwrap();
        let at = root.display().to_string();

        // 名前を変える。
        call("book", &serde_json::json!({ "path": at, "book": "仕事", "name": "しごと" })).unwrap();
        assert!(root.join("しごと").is_dir());
        assert!(!root.join("仕事").exists());

        // 外へは出られない ── `..` を渡しても。
        std::fs::create_dir_all(d.path().join("よそ")).unwrap();
        assert!(call("book", &serde_json::json!({ "path": at, "book": "../よそ", "drop": true })).is_err()
            || d.path().join("よそ").is_dir());

        // 使えない名前。
        assert!(call("book", &serde_json::json!({ "path": at, "book": "しごと", "name": "a/b" })).is_err());
        assert!(call("book", &serde_json::json!({ "path": at, "book": "しごと", "name": "  " })).is_err());

        // 捨てる ── 何本が道連れになるかを先に言う。
        let out = call("book", &serde_json::json!({ "path": at, "book": "しごと", "drop": true })).unwrap();
        assert_eq!(out["gone"], 1);
        assert!(!root.join("しごと").exists());
    }

    #[test]
    fn a_note_splits_into_its_bookkeeping_and_its_words() {
        let text = "---\ntitle: x\ntags: [a]\n---\n\n本文。\n";
        let out = call("split", &serde_json::json!({ "text": text })).unwrap();
        assert_eq!(out["head"], "---\ntitle: x\ntags: [a]\n---\n");
        assert_eq!(out["body"], "\n本文。\n");
        // くっつけると元に戻る ── ここがずれると、書いた字が消える。
        let back = format!("{}{}", out["head"].as_str().unwrap(), out["body"].as_str().unwrap());
        assert_eq!(back, text);

        // 前書きの無いノートは、まるごと本文。
        let plain = call("split", &serde_json::json!({ "text": "ただの本文\n" })).unwrap();
        assert_eq!(plain["head"], "");
        assert_eq!(plain["body"], "ただの本文\n");
    }

    #[test]
    fn a_task_comes_back_pressable_and_pressing_it_writes_the_line() {
        let text = "- [ ] 牛乳\n- [x] 珈琲\n";
        let bs = call("blocks", &serde_json::json!({ "text": text })).unwrap();
        let bs = bs["blocks"].as_array().unwrap();
        assert_eq!(bs[0]["kind"], "check");
        assert_eq!(bs[0]["done"], false);
        assert_eq!(bs[0]["line"], 0);
        assert_eq!(bs[1]["done"], true);

        let out = call(
            "check",
            &serde_json::json!({ "text": text, "line": 0, "done": true }),
        )
        .unwrap();
        assert_eq!(out["text"], "- [x] 牛乳\n- [x] 珈琲\n");
    }

    #[test]
    fn a_bad_request_is_an_answer_and_not_a_crash() {
        assert!(call("いない", &serde_json::json!({})).is_err());
        // Through the real door, an error is JSON like anything else — an app
        // that got a null here would have no way to say what went wrong.
        let m = CString::new("いない").unwrap();
        let p = CString::new("{}").unwrap();
        let out = unsafe { cian_call(m.as_ptr(), p.as_ptr()) };
        assert!(!out.is_null());
        let text = unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned();
        unsafe { cian_free(out) };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(v["error"].as_str().unwrap().contains("知らない操作"), "{text}");
    }

    #[test]
    fn null_and_nonsense_do_not_take_the_app_down_with_them() {
        // Swift can hand over a null pointer, and it must not be the last
        // thing the app ever does.
        let out = unsafe { cian_call(std::ptr::null(), std::ptr::null()) };
        assert!(!out.is_null());
        unsafe { cian_free(out) };

        let m = CString::new("notes").unwrap();
        let p = CString::new("{ これは JSON ではない").unwrap();
        let out = unsafe { cian_call(m.as_ptr(), p.as_ptr()) };
        let text = unsafe { CStr::from_ptr(out) }.to_string_lossy().into_owned();
        unsafe { cian_free(out) };
        assert!(text.contains("JSON ではありません"), "{text}");

        // Freeing null is allowed, because the caller's error path will.
        unsafe { cian_free(std::ptr::null_mut()) };
    }
}
