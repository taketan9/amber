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
                        "bytes": n.bytes,
                        "pinned": n.pinned,
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
            let at = cian_core::note::create(&dir, &arg(p, "title"), &cian_core::note::today())?;
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
            let out: Vec<serde_json::Value> = cian_core::note::blocks(&text)
                .into_iter()
                .map(|b| match b {
                    Block::Heading { level, text } => {
                        serde_json::json!({ "kind": "heading", "level": level, "text": text })
                    }
                    Block::Paragraph(text) => serde_json::json!({ "kind": "paragraph", "text": text }),
                    Block::Bullet(text) => serde_json::json!({ "kind": "bullet", "text": text }),
                    Block::Numbered { n, text } => {
                        serde_json::json!({ "kind": "numbered", "n": n, "text": text })
                    }
                    Block::Quote(text) => serde_json::json!({ "kind": "quote", "text": text }),
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
