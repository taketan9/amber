//! 訊かれたことに答える一枚。**扉が二つあっても、答えは一つ。**
//!
//! iPhone は C ABI（`amber-ffi`）から、窓は標準入出力（`amber-server`）から
//! ここへ来る。前は電話しか居なかったので dispatcher は ffi の中にあったが、
//! **窓が増えるときに写すと、そこから二つの答えが育つ** ── 「同じ操作なのに
//! Mac と iPhone で結果が違う」は、一度の編集で作れてしまう。
//!
//! 約束: `call` は JSON を受けて JSON を返す。失敗は `Err` で、扉の側が
//! `{"error": "…"}` に包む。**扉は包むだけで、判断しない。**



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
            "amber": env!("CARGO_PKG_VERSION"),
            // Whether this build has a desktop under it. The phone's does
            // not, and `delete` to the trash refuses there rather than
            // deleting outright.
            "desktop": crate::DESKTOP,
        })),

        // Every note under a directory. The same walk the window asks for.
        "notes" => {
            let dir = std::path::PathBuf::from(arg(p, "path"));
            if !dir.is_dir() {
                anyhow::bail!("{} を開けません", dir.display());
            }
            let limits = crate::survey::Limits {
                depth: p["depth"].as_u64().unwrap_or(6) as usize,
                rows: 4000,
                hidden: false,
                ..Default::default()
            };
            let stop = std::sync::atomic::AtomicBool::new(false);
            let (found, walk) = crate::note::list(&dir, limits, &stop);
            let book = crate::notebook::read(&dir);
            // **共有かどうかは、フォルダ自身が言う。** 設定に書いていた頃は
            // 相手の amber に何も伝わらず、受け取った人が自分で教え直す手が
            // 要った（`notebook::SHARE_MARK` の註）。
            let shares = crate::notebook::shares(&dir, &walk.rows);
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
                        "search": crate::note::haystack(n),
                        // **家族と分けてあるか。** 判断は core に一つ ──
                        // 窓と電話で二度書くと、片方だけ「共有」の印が
                        // 出るノートができる。
                        "shared": crate::notebook::shared(
                            &shares,
                            f.rel.rsplit_once('/').map(|(d, _)| d).unwrap_or(""),
                        ),
                        // **クラウドが作った控えなら、そう言う。**
                        // 一覧から消さない ── 消すと、中身を助け出す道が
                        // どこにも無くなる。並べたうえで札を貼る。
                        "clash": n.path.file_name()
                            .and_then(|f| f.to_str())
                            .and_then(crate::cloud::shape)
                            .filter(|s| s.kind == crate::cloud::Kind::Clash)
                            .map(|s| serde_json::json!({ "of": s.of, "by": s.by })),
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
            // **まだ落ちてきていないノート。** iCloud は中身を消して
            // `.買い物リスト.md.icloud` という札を置くので、名前が違って
            // 一覧に出ない ── 黙っていると「ノートが消えた」にしか
            // 見えないが、待てば戻ってくるだけ。
            let waiting: Vec<serde_json::Value> = crate::cloud::waiting(&dir, &walk.rows)
                .into_iter()
                .map(|at| {
                    serde_json::json!({
                        "of": at.file_name().and_then(|f| f.to_str()).unwrap_or(""),
                        // 本来の道 ── 電話はこれを iOS に渡して「落として
                        // きて」と頼む。
                        "at": at.to_string_lossy(),
                    })
                })
                .collect();
            Ok(serde_json::json!({
                "root": dir.display().to_string(),
                // **一つとは限らない。** 家族用と仕事用の棚が両方あって
                // いい ── 印はフォルダごとに置くので、数を決める理由が無い。
                "shares": shares.iter().map(|s| serde_json::json!({
                    "at": s,
                    "by": crate::notebook::share_mark(&dir.join(s))
                        .map(|m| m.by).unwrap_or_default(),
                })).collect::<Vec<_>>(),
                "waiting": waiting,
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
            let f = crate::text::read(&path)?;
            let stamp = crate::stamp::of(&path);
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
                    if crate::stamp::changed(&path, &expect) {
                        return Ok(serde_json::json!({
                            "conflict": true,
                            "why": crate::stamp::describe(&path, &expect),
                        }));
                    }
                }
            }
            // A note that is not there yet is a new note, not a failure:
            // the phone writes one it has only just made.
            let mut f = crate::text::read(&path).unwrap_or_default();
            f.lines = text.split('\n').map(|l| l.to_string()).collect();
            crate::text::write(&path, &f)?;
            Ok(serde_json::json!({
                "ok": true,
                "stamp": crate::stamp::of(&path).as_ref().map(stamp_json),
            }))
        }

        // A new note, named and shaped by the same rules the window uses.
        "new" => {
            let dir = std::path::PathBuf::from(arg(p, "dir"));
            let at = crate::note::create(
                &dir,
                &arg(p, "title"),
                &crate::note::today(),
                &crate::note::now_stamp(),
            )?;
            Ok(serde_json::json!({
                "path": at.display().to_string(),
                "name": at.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            }))
        }

        // A note, as things to draw. The reading half of the phone: what a
        // heading *is* is decided in `crate::note`, and what a heading
        // *looks like* is decided on the phone. Splitting it the other way
        // would put a Markdown parser somewhere no test can reach.
        "blocks" => {
            let text = if p["text"].is_string() {
                arg(p, "text")
            } else {
                let path = std::path::PathBuf::from(arg(p, "path"));
                crate::text::read(&path)?.lines.join("\n")
            };
            use crate::note::Block;
            // The coloured pieces of a line, worked out here so the window
            // and the phone cannot disagree about what a note says.
            fn runs(text: &str) -> serde_json::Value {
                serde_json::Value::Array(
                    crate::note::spans(text)
                        .into_iter()
                        .map(|s| serde_json::json!({ "text": s.text, "color": s.color }))
                        .collect(),
                )
            }
            let out: Vec<serde_json::Value> = crate::note::blocks(&text)
                .into_iter()
                .map(|b| match b {
                    Block::Heading { level, text, line } => serde_json::json!({
                        "kind": "heading", "level": level, "runs": runs(&text),
                        "text": text, "line": line,
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
                    // **升は `runs` を持って渡す。** 表の中にも太字は書かれる
                    // し、引く側が自分で `**` を剥がしはじめると、二つ目の
                    // Markdown の読み手がそこに生える。
                    Block::Table { head, align, rows } => serde_json::json!({
                        "kind": "table",
                        "align": align.iter().map(|a| match a {
                            crate::markdown::Align::Center => "center",
                            crate::markdown::Align::Right => "right",
                            crate::markdown::Align::Left => "left",
                        }).collect::<Vec<_>>(),
                        "head": head.iter().map(|c| serde_json::json!({
                            "runs": runs(c), "text": c,
                        })).collect::<Vec<_>>(),
                        "rows": rows.iter().map(|r| r.iter().map(|c| serde_json::json!({
                            "runs": runs(c), "text": c,
                        })).collect::<Vec<_>>()).collect::<Vec<_>>(),
                    }),
                    Block::Alert { kind, body } => serde_json::json!({
                        "kind": "alert", "alert": kind,
                        "body": body.iter().map(|t| serde_json::json!({
                            "runs": runs(t), "text": t,
                        })).collect::<Vec<_>>(),
                    }),
                    Block::Rule => serde_json::json!({ "kind": "rule" }),
                })
                .collect();
            Ok(serde_json::json!({ "blocks": out }))
        }

        // 書く道具の一押し。**押したときに何が起きるかは、ここが決める。**
        //
        // 渡すのは**選んだ字だけ**で、位置は渡さない ── JS は UTF-16 の桁で
        // 数え、Rust は文字で数えるので、絵文字が一つ混ざれば境目がずれる。
        // 返ってきた字を、選んだところに置き換えてもらう。
        "mark" => {
            let text = arg(p, "text");
            let with = arg(p, "with");
            let out = match arg(p, "kind").as_str() {
                "wrap" => crate::markdown::marks::wrap(&text, &with),
                "line" => crate::markdown::marks::prefix(&text, &with),
                "heading" => crate::markdown::marks::deepen(&text),
                // `with` は深さ（`"2"` で `##`、`"0"` で見出しをやめる）。
                "head" => crate::markdown::marks::level(
                    &text,
                    with.parse::<usize>().unwrap_or(1),
                ),
                other => anyhow::bail!("知らない印です: {other}"),
            };
            Ok(serde_json::json!({ "text": out }))
        }

        // 一本だけ、ノートとして読む。
        //
        // `notes` はフォルダを歩くもので、**amber の外にある一本**には
        // 使えない。かといって前端が題を自分で決めはじめると、そこだけ
        // 別の題になる ── 題は `title:` → 最初の見出し → 書き出しの一行 →
        // ファイル名、という一つの答えが `note::read` にある。
        "note" => {
            let path = std::path::PathBuf::from(arg(p, "path"));
            let Some(n) = crate::note::read(&path, 40) else {
                anyhow::bail!("{} を読めません", path.display());
            };
            Ok(serde_json::json!({
                "path": n.path.display().to_string(),
                "rel": n.path.file_name().map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                "book": "",
                "title": n.title,
                "excerpt": n.excerpt,
                "search": crate::note::haystack(&n),
                "star": n.star,
                "tags": n.tags,
                "updated": n.updated,
                "created": n.created,
                "bytes": n.bytes,
            }))
        }

        // 読める形。**組み方は core、見た目は前端。**
        //
        // `blocks` の隣にもう一つ扉を開けるのは、窓と iPhone で描き方が違う
        // から ── iPhone は SwiftUI の View を積むので `blocks` が要り、窓は
        // HTML を流し込むほうが速い。**解釈は一つ**（どちらも
        // `note::blocks` と同じ行単位の読み方を通る）で、分かれるのは
        // 最後の組み立てだけ。
        //
        // 逃がし（`javascript:` を落とす、`onclick` も `class` も字にする、
        // 色は検査済みの6桁だけ通す）は `markdown::to_html` の中にある。
        // **ここで足すと二か所になる。**
        "html" => {
            let text = if p["text"].is_string() {
                arg(p, "text")
            } else {
                let path = std::path::PathBuf::from(arg(p, "path"));
                crate::text::read(&path)?.lines.join("\n")
            };
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            Ok(serde_json::json!({ "html": crate::markdown::to_html(&lines) }))
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
                Some("") => crate::note::set_field(&text, "star", Some("true")),
                Some(sh) => crate::note::set_field(&text, "star", Some(sh)),
                None => crate::note::set_field(&text, "star", None),
            };
            // The one written before favourites had a name. Left behind, it
            // would keep the note a favourite after it was taken off one.
            let out = crate::note::set_field(&out, "pinned", None);
            Ok(serde_json::json!({ "text": out }))
        }

        // Make or forget a favourite shelf. Only the empty ones need saying
        // out loud — the rest are named by the notes standing on them.
        "shelf" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let name = arg(p, "name");
            if p["drop"].as_bool().unwrap_or(false) {
                crate::notebook::drop_star(&root, &name)?;
            } else {
                crate::notebook::add_star(&root, &name)?;
            }
            Ok(serde_json::json!({ "stars": crate::notebook::read(&root).stars }))
        }

        // 合わせ方を決める。**繋がない。**
        //
        // 向こうにあるものの一覧は、呼ぶ側が持ってくる（通信は I/O なので
        // core の外）── ここがするのは、こちらを歩いて指紋を取り、前に
        // 合わせたときの憶えと突き合わせて、**運ぶ向きを決める**こと。
        "syncplan" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let who = p["who"].as_str().unwrap_or("drive");
            let stop = std::sync::atomic::AtomicBool::new(false);
            let (found, _) = crate::note::list(
                &root,
                crate::survey::Limits { depth: 6, rows: 4000, hidden: false, ..Default::default() },
                &stop,
            );
            let here: Vec<crate::sync::Here> = found
                .iter()
                .map(|f| crate::sync::Here {
                    rel: f.rel.clone(),
                    hash: crate::sync::fingerprint(
                        &std::fs::read(&f.note.path).unwrap_or_default(),
                    ),
                })
                .collect();
            let there: Vec<crate::sync::There> = p["remote"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| {
                            Some(crate::sync::There {
                                rel: v.get("rel")?.as_str()?.to_string(),
                                id: v.get("id")?.as_str()?.to_string(),
                                tag: v.get("tag")?.as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let was = crate::sync::recall(&root, who);
            let steps: Vec<serde_json::Value> = crate::sync::plan(&here, &there, &was)
                .into_iter()
                .map(|s| {
                    let (id, rel) = match &s {
                        crate::sync::Step::Up { id, rel } => (id.clone(), rel.clone()),
                        crate::sync::Step::Down { id, rel }
                        | crate::sync::Step::DropThere { id, rel }
                        | crate::sync::Step::Clash { id, rel } => (Some(id.clone()), rel.clone()),
                        crate::sync::Step::DropHere { rel } => (None, rel.clone()),
                    };
                    serde_json::json!({ "do": s.word(), "rel": rel, "id": id })
                })
                .collect();
            Ok(serde_json::json!({ "steps": steps }))
        }

        // 運び終わったぶんを憶える。
        //
        // **全部やるか何もしないか、にしない** ── 電波の悪いところで一本も
        // 進まなくなる。運べたぶんだけ憶えて、残りは次に。
        "synced" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let who = p["who"].as_str().unwrap_or("drive");
            let done: Vec<crate::sync::Was> = p["done"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| {
                            let rel = v.get("rel")?.as_str()?.to_string();
                            // 指紋は、いまディスクにあるものから取り直す ──
                            // 呼ぶ側に計算させると、二つの土台で違う答えが出る。
                            let hash = crate::sync::fingerprint(
                                &std::fs::read(root.join(&rel)).unwrap_or_default(),
                            );
                            Some(crate::sync::Was {
                                rel,
                                hash,
                                id: v.get("id")?.as_str()?.to_string(),
                                tag: v.get("tag")?.as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let gone: Vec<String> = p["gone"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            crate::sync::remember(&root, who, &done, &gone)?;
            Ok(serde_json::json!({ "kept": done.len(), "dropped": gone.len() }))
        }

        // 共有の棚にする（`off` で、やめる）。
        //
        // **分けるのはクラウドの仕事。** amber がするのは、そのフォルダに
        // 印を一枚置くことだけ ── それだけで「このノートを共有する」が
        // そのフォルダへ移すことになり、いまあるフォルダの仕組みがそのまま
        // 効く。そして印はフォルダと一緒に旅をするので、**受け取った人は
        // 何も教えなくていい**。
        //
        // フォルダが無ければ作る ── 「共有する」を押した人に、その前に
        // 「フォルダを作る」を押させない。
        "share" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let folder = p["folder"].as_str().unwrap_or("").trim_matches('/');
            let dir = if folder.is_empty() { root.clone() } else { root.join(folder) };
            if p["off"].as_bool().unwrap_or(false) {
                crate::notebook::unmark_share(&dir)?;
            } else {
                let by = p["by"].as_str().unwrap_or("");
                let today = p["today"].as_str().unwrap_or("");
                crate::notebook::mark_share(&dir, by, today)?;
            }
            let stop = std::sync::atomic::AtomicBool::new(false);
            let walk = crate::survey::survey(&root, crate::survey::Limits {
                depth: 6, rows: 4000, hidden: false, ..Default::default()
            }, &stop);
            Ok(serde_json::json!({ "shares": crate::notebook::shares(&root, &walk.rows) }))
        }

        // What colour a folder is. His to choose — cian offers a palette and
        // does not insist on it.
        "color" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let folder = arg(p, "folder");
            crate::notebook::set_color(&root, &folder, p["color"].as_str())?;
            Ok(serde_json::json!({ "colors": crate::notebook::read(&root).colors }))
        }

        // Wrap a piece of text in a colour, the way cian writes it. Here
        // and not in the front ends: the notation is one decision, and two
        // places that write it are two notations one edit apart.
        "paint" => {
            Ok(serde_json::json!({
                "text": crate::note::paint(&arg(p, "text"), &arg(p, "color")),
            }))
        }
        // Everything, moved to a new home — `notebook::migrate`, the
        // same one the window calls. Copy, check, then remove; and
        // nothing is overwritten.
        "migrate" => {
            let from = std::path::PathBuf::from(arg(p, "from"));
            let to = std::path::PathBuf::from(arg(p, "to"));
            Ok(serde_json::json!({ "moved": crate::notebook::migrate(&from, &to)? }))
        }

        // A backup, put back — `notebook::restore`, the same one the
        // window calls.
        // ── 前の姿 ──────────────────────────────────────────────
        //
        // **判断はここ。** いつ一世代にするか、何世代残すか、いつ落とすかは
        // 窓と電話で同じでなければならない ── 同じフォルダを二つの端末で
        // 触るので、片方の決まりで消したものを、もう片方が残っていると思う。

        // いまの姿を一つ残す。`gap` 秒たっていなければ何もしない。
        "keep" => {
            let root = std::path::PathBuf::from(arg(p, "root"));
            let note = std::path::PathBuf::from(arg(p, "path"));
            let gap = p["gap"].as_u64().unwrap_or(300);
            let force = p["force"].as_bool().unwrap_or(false);
            let kept = p["kept"].as_bool().unwrap_or(false);
            let text = match p["text"].as_str() {
                Some(t) => t.to_string(),
                None => std::fs::read_to_string(&note)?,
            };
            let stamp = crate::history::keep(&root, &note, &text, gap, force, kept)?;
            Ok(serde_json::json!({ "stamp": stamp }))
        }

        // 前の姿を、新しい順に。`path` はノートでもフォルダでもよい。
        "history" => {
            let root = std::path::PathBuf::from(arg(p, "root"));
            let at = std::path::PathBuf::from(arg(p, "path"));
            let rows: Vec<serde_json::Value> = crate::history::list(&root, &at)?
                .into_iter()
                .map(|v| {
                    serde_json::json!({
                        "stamp": v.stamp,
                        "when": crate::history::spoken(&v.stamp),
                        "note": v.note,
                        "kept": v.kept,
                        "bytes": v.bytes,
                    })
                })
                .collect();
            Ok(serde_json::json!({
                "versions": rows,
                "gens": crate::history::KEEP_GENS,
                "days": crate::history::KEEP_DAYS,
            }))
        }

        // 一つの姿の中身。
        "oldtext" => {
            let root = std::path::PathBuf::from(arg(p, "root"));
            let note = std::path::PathBuf::from(arg(p, "path"));
            let text = crate::history::read(&root, &note, arg(p, "stamp").as_str())?;
            Ok(serde_json::json!({ "text": text }))
        }

        // 「残す」の印を付ける／外す。
        "keepmark" => {
            let root = std::path::PathBuf::from(arg(p, "root"));
            let note = std::path::PathBuf::from(arg(p, "path"));
            crate::history::mark(&root, &note, arg(p, "stamp").as_str(),
                                 p["kept"].as_bool().unwrap_or(true))?;
            Ok(serde_json::json!({ "ok": true }))
        }

        // フォルダに付けられる色。**並びは core が持つ** ── 窓と電話に
        // 同じ表を書いていた頃、十一色のうち六色がずれていた。
        "palette" => Ok(serde_json::json!({
            "colors": crate::notebook::PALETTE
                .iter()
                .map(|(hex, name)| serde_json::json!({ "hex": hex, "name": name }))
                .collect::<Vec<_>>(),
        })),

        "restore" => {
            let zip = std::path::PathBuf::from(arg(p, "zip"));
            let to = std::path::PathBuf::from(arg(p, "to"));
            let (put, kept) = crate::notebook::restore(&zip, &to)?;
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
                let n = crate::note::list(
                    &from,
                    crate::survey::Limits { depth: 9, rows: 9999, hidden: false, ..Default::default() },
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
            // 前端は **見出しごとに探し分ける**（題だけ・タグだけ・
            // フォルダだけ）。字だけを渡していた頃は、`tag:定型` と打っても
            // 「tag:定型」という字を本文から探していた。
            let groups: Vec<serde_json::Value> = crate::note::terms(&arg(p, "q"))
                .into_iter()
                .map(|g| {
                    serde_json::Value::Array(
                        g.into_iter()
                            .map(|t| serde_json::json!({
                                "field": t.field, "word": t.word, "not": t.not,
                            }))
                            .collect(),
                    )
                })
                .collect();
            Ok(serde_json::json!({ "groups": groups }))
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
            let n = crate::note::front(&lines).lines;
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
            Ok(serde_json::json!({ "text": crate::note::set_check(&text, line, done) }))
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
            let cap = p["limit"].as_u64().unwrap_or(200) as usize;
            let cancel = std::sync::atomic::AtomicBool::new(false);
            // 判断は core に。**借りていた cian の grep をやめた** ── あちらは
            // 打った文字列をそのまま含むかを見るだけで、窓の `/` 絞り込みの
            // AND / OR が効かなかった。同じ言葉で探して同じものが出る、が
            // 二つの前端の間で成り立つようになる。一本につき一行なのは前と同じ。
            let hits: Vec<serde_json::Value> = crate::note::find(
                &root,
                &arg(p, "needle"),
                cap,
                crate::survey::Limits::default(),
                &cancel,
            )
            .into_iter()
            .map(|h| {
                serde_json::json!({
                    "path": h.path.display().to_string(),
                    "line": h.line,
                    "text": h.text,
                })
            })
            .collect();
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
                "text": crate::note::set_tags(&arg(p, "text"), &tags),
            }))
        }

        // One plain field on or off — `pinned` today, whatever tomorrow.
        // Text in, text out, so it saves like any other edit.
        "setfield" => {
            let value = p["value"].as_str();
            Ok(serde_json::json!({
                "text": crate::note::set_field(&arg(p, "text"), &arg(p, "key"), value),
            }))
        }

        // Move a note into another notebook, pictures and all.
        "move" => {
            let note = std::path::PathBuf::from(arg(p, "path"));
            let dir = std::path::PathBuf::from(arg(p, "dir"));
            let at = crate::note::move_to(&note, &dir)?;
            Ok(serde_json::json!({ "path": at.display().to_string() }))
        }

        // Make a notebook. A folder, because that is what a notebook is
        // here — somebody looking at the same place from a Mac sees folders.
        // クラウドの中に、amber の置き場所を用意する。
        //
        // **`mkbook` と分けてある。** あちらは「新しいフォルダを作る」で、
        // 既にあれば止まるのが正しい（同じ名前の二つ目を黙って作らない）。
        // こちらは「ここに置きたい」で、**二度目に同じところを選んだ人を
        // 止める理由が無い** ── 一度目に作ったフォルダが、二度目には
        // エラーになるのはおかしい。
        "place" => {
            let dir = std::path::PathBuf::from(arg(p, "dir"));
            if dir.as_os_str().is_empty() {
                anyhow::bail!("場所がありません");
            }
            std::fs::create_dir_all(&dir)?;
            Ok(serde_json::json!({ "dir": dir.to_string_lossy() }))
        }

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
            // 名前は「何が入っているか」＋日付。既定は `amber` ──
            // ここが `cian` のまま残っていて、amber の全体バックアップが
            // `cian-2026-09-06.zip` として出ていた。
            let mut name = String::from("amber");
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
                    let limits = crate::survey::Limits {
                        depth: 6, rows: 4000, hidden: false, ..Default::default()
                    };
                    let stop = std::sync::atomic::AtomicBool::new(false);
                    let (found, _) = crate::note::list(&root, limits, &stop);
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
            let at = dir.join(format!("{name}-{}.zip", crate::note::today()));
            let _ = std::fs::remove_file(&at);
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let mut nothing = |_: usize, _: usize| {};
            let mut ctl = crate::Ctl { cancel: &cancel, on_progress: &mut nothing };
            // 何の zip かを中に書いておく ── 戻すときに、頭を外すかどうかが
            // これで決まる（`zipbox::LABEL` を見よ）。
            let label = serde_json::json!({
                "scope": scope, "what": what, "made": crate::note::today(),
            })
            .to_string();
            let files = crate::zipbox::create_labelled(&sources, &at, Some(&label), &mut ctl)?;
            Ok(serde_json::json!({
                "path": at.display().to_string(),
                "files": files,
            }))
        }

        // What a note wants to be reminded about, and what its routine owes.
        "remind" => {
            let text = if p["text"].is_string() {
                arg(p, "text")
            } else {
                crate::text::read(&std::path::PathBuf::from(arg(p, "path")))?
                    .lines
                    .join("\n")
            };
            let r = crate::note::remind(&text);
            let today = chrono_today();
            let due: Vec<String> = match r.every {
                Some((every, _, _)) => crate::note::due_since(every, r.last, today)
                    .iter()
                    .map(|d| d.to_string())
                    .collect(),
                None => Vec::new(),
            };
            Ok(serde_json::json!({
                "once": r.once.map(|t| t.format("%Y-%m-%d %H:%M").to_string()),
                "every": r.every.map(|(e, h, m)| serde_json::json!({
                    "kind": match e {
                        crate::note::Every::Daily => "daily",
                        crate::note::Every::Weekly(_) => "weekly",
                        crate::note::Every::Monthly(_) => "monthly",
                    },
                    "n": match e {
                        crate::note::Every::Daily => 0,
                        crate::note::Every::Weekly(w) => w,
                        crate::note::Every::Monthly(d) => d,
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
            let made = crate::note::carry_out(&path, day)?;
            let text = std::fs::read_to_string(&path)?;
            std::fs::write(&path, crate::note::set_field(&text, "last", Some(&on)))?;
            Ok(serde_json::json!({ "path": made.display().to_string() }))
        }

        // A photo, put beside the note. The phone sends it base64 because
        // that is what fits down a C string; everything about *where it goes*
        // is `crate::note::attach`, the same call the window makes when a
        // screenshot is pasted into the editor.
        "image" => {
            let note = std::path::PathBuf::from(arg(p, "note"));
            let bytes = b64(&arg(p, "b64")).ok_or_else(|| anyhow::anyhow!("画像を読めません"))?;
            let link = crate::note::attach(&note, &bytes, &arg(p, "ext"))?;
            Ok(serde_json::json!({ "link": link, "bytes": bytes.len() }))
        }

        // Remove a note.
        //
        // Outright, because there is no trash on a phone to move it to —
        // `crate::DESKTOP` is false here and `DeleteMode::Trash` refuses
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
/// below caught it. `crate::stamp::token` keeps it exact, and the phone
/// never has to know what is inside.
fn stamp_json(s: &crate::stamp::Stamp) -> serde_json::Value {
    serde_json::Value::String(crate::stamp::token(s))
}

fn json_stamp(v: &serde_json::Value) -> Option<crate::stamp::Stamp> {
    crate::stamp::from_token(v.as_str()?)
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
    fn 書く道具は_押し直すと外れる() {
        let m = |kind: &str, with: &str, text: &str| -> String {
            call("mark", &serde_json::json!({ "kind": kind, "with": with, "text": text }))
                .unwrap()["text"].as_str().unwrap().to_string()
        };
        // 挟む・外す
        assert_eq!(m("wrap", "**", "太字"), "**太字**");
        assert_eq!(m("wrap", "**", "**太字**"), "太字");
        // 選んでいないときは印だけ ── 中に入って打てるように。
        assert_eq!(m("wrap", "`", ""), "``");

        // 行頭。**すべてに付いていれば外れる。**
        assert_eq!(m("line", "- ", "あ\nい"), "- あ\n- い");
        assert_eq!(m("line", "- ", "- あ\n- い"), "あ\nい");
        // 一つでも無ければ、揃える。
        assert_eq!(m("line", "- ", "- あ\nい"), "- あ\n- い");
        // 別の印は付け替える ── `> - もの` はたいてい望んだことではない。
        assert_eq!(m("line", "> ", "- あ"), "> あ");
        // チェックは `[x]` でも「付いている」。
        assert_eq!(m("line", "- [ ] ", "- [x] 済み"), "済み");
        // 番号は振り直す。
        assert_eq!(m("line", "1. ", "あ\nい\nう"), "1. あ\n2. い\n3. う");
        // 空行は数に入れない。
        assert_eq!(m("line", "- ", "- あ\n\n- い"), "あ\n\nい");

        // 見出しは押すたびに深くなり、四度目で戻る。
        assert_eq!(m("heading", "", "題"), "# 題");
        assert_eq!(m("heading", "", "# 題"), "## 題");
        assert_eq!(m("heading", "", "## 題"), "### 題");
        assert_eq!(m("heading", "", "### 題"), "題");

        assert!(call("mark", &serde_json::json!({ "kind": "なにか", "text": "あ" })).is_err());
    }

    #[test]
    fn 窓に渡す読める形は_前書きを出さず_危ないものを字にする() {
        let r = call("html", &serde_json::json!({
            "text": "---\ntitle: x\ntags: [仕事]\n---\n# 題\n- [ ] やること\n",
        })).unwrap();
        let h = r["html"].as_str().unwrap();
        assert!(h.contains("<h1"), "見出しが組まれていない: {h}");
        assert!(!h.contains("title: x"), "前書きが漏れている: {h}");
        // チェックは行番号を積んで渡る ── 何番目の升かを数えると、
        // 前書きのあるノートでずれる。
        assert!(h.contains("data-line="), "チェックに行番号が無い: {h}");

        // 逃がしは `markdown::to_html` の中にある。**ここでも効くこと**を
        // 見ておかないと、扉を増やしたときに素通りする道ができる。
        let bad = call("html", &serde_json::json!({
            "text": "<script>alert(1)</script>\n\n[押す](javascript:alert(1))\n",
        })).unwrap();
        let h = bad["html"].as_str().unwrap();
        assert!(!h.contains("<script"), "script が生きている: {h}");
        assert!(!h.contains("javascript:"), "javascript: が生きている: {h}");
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
        crate::note::attach(&note, &[1], "png").unwrap();
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
            assert!(at.to_string_lossy().contains(&crate::note::today()), "{at:?}");
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

    /// 四つの範囲が、**それぞれ違うものを包む**か。
    ///
    /// 窓は長いあいだ `scope: "all"` を決め打ちで渡していて、四つあることは
    /// エンジンしか知らなかった ── 電話にできて窓にできない、が起きていた。
    #[test]
    fn 範囲の四つは_それぞれ違うものを包む() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("ノート");
        std::fs::create_dir_all(root.join("仕事")).unwrap();
        std::fs::create_dir_all(root.join("日記")).unwrap();
        std::fs::write(root.join("上.md"), "---\ntags: [雑]\n---\n根っこ\n").unwrap();
        std::fs::write(root.join("仕事/週報.md"), "---\ntags: [仕事]\n---\n今週\n").unwrap();
        std::fs::write(root.join("仕事/議事.md"), "---\ntags: [仕事, 雑]\n---\n打合せ\n").unwrap();
        std::fs::write(root.join("日記/9月.md"), "---\ntags: [日記]\n---\n晴れ\n").unwrap();

        let take = |scope: &str, what: &str| -> (String, u64) {
            let out = call("backup", &serde_json::json!({
                "path": root.display().to_string(), "scope": scope, "what": what,
                "into": d.path().display().to_string(),
            })).unwrap();
            (out["path"].as_str().unwrap().to_string(), out["files"].as_u64().unwrap())
        };

        let (all, n) = take("all", "");
        assert_eq!(n, 4, "すべて: {all}");
        // 名前は何が入っているかを言う ── `backup.zip` の並んだフォルダは
        // 「どれがどれか」という一つの問いになる。
        assert!(all.contains("amber-"), "全体の zip の名前: {all}");

        let (book, n) = take("book", "仕事");
        assert_eq!(n, 2, "フォルダ: {book}");
        assert!(book.contains("仕事-"), "{book}");

        // タグは**フォルダをまたぐ** ── 「雑」は根っこと仕事の両方にある。
        let (tag, n) = take("tag", "雑");
        assert_eq!(n, 2, "タグ: {tag}");
        assert!(tag.contains("tag-雑"), "{tag}");

        let (note, n) = take("note", &root.join("日記/9月.md").display().to_string());
        assert_eq!(n, 1, "ノート一つ: {note}");
        assert!(note.contains("9月-"), "{note}");

        // 空振りは、空の zip ではなく断り ── 中身の無い zip は
        // 「取れた」に見えて、戻したときに何も戻らない。
        assert!(call("backup", &serde_json::json!({
            "path": root.display().to_string(), "scope": "tag", "what": "存在しない",
            "into": d.path().display().to_string(),
        })).is_err());
        assert!(call("backup", &serde_json::json!({
            "path": root.display().to_string(), "scope": "book", "what": "無いフォルダ",
            "into": d.path().display().to_string(),
        })).is_err());
    }

    /// **別のノートが入っている場所へ戻したら、どうなるか。**
    ///
    /// 戻すのは「消えたものを取り返す」ためで、いま書いているものを消して
    /// いいという意味ではない。三つとも確かめる ──
    /// 知らないノートは残る／同じ名前は今のが勝つ／構造が違っても入る。
    #[test]
    fn 戻すとき_いまあるものを消さない() {
        let d = tempfile::tempdir().unwrap();
        let from = d.path().join("むかし");
        std::fs::create_dir_all(from.join("仕事")).unwrap();
        std::fs::write(from.join("a.md"), "むかしの a\n").unwrap();
        std::fs::write(from.join("仕事/b.md"), "むかしの b\n").unwrap();
        let zip = call("backup", &serde_json::json!({
            "path": from.display().to_string(), "scope": "all", "what": "",
            "into": d.path().display().to_string(),
        })).unwrap()["path"].as_str().unwrap().to_string();

        // 戻す先は**別のノート帳**。中身も構造も違う。
        let to = d.path().join("いま");
        std::fs::create_dir_all(to.join("日記")).unwrap();
        std::fs::write(to.join("a.md"), "いまの a\n").unwrap();          // 同じ名前
        std::fs::write(to.join("知らない.md"), "触るな\n").unwrap();     // 向こうに無い
        std::fs::create_dir_all(to.join("仕事")).unwrap();
        std::fs::write(to.join("仕事/c.md"), "いまの c\n").unwrap();     // 同じ棚の別の紙
        std::fs::write(to.join("日記/9月.md"), "晴れ\n").unwrap();       // 向こうに無い棚

        let back = call("restore", &serde_json::json!({
            "zip": zip, "to": to.display().to_string(),
        })).unwrap();
        assert_eq!(back["put"].as_u64().unwrap(), 1, "入ったのは b だけ: {back}");
        assert_eq!(back["kept"].as_u64().unwrap(), 1, "避けたのは a だけ: {back}");

        // 同じ名前は**いまのが勝つ**。
        assert_eq!(std::fs::read_to_string(to.join("a.md")).unwrap(), "いまの a\n");
        // 向こうに無かったものは、そのまま。
        assert_eq!(std::fs::read_to_string(to.join("知らない.md")).unwrap(), "触るな\n");
        assert_eq!(std::fs::read_to_string(to.join("日記/9月.md")).unwrap(), "晴れ\n");
        // 同じ棚の別の紙も、そのまま。
        assert_eq!(std::fs::read_to_string(to.join("仕事/c.md")).unwrap(), "いまの c\n");
        // 向こうにしか無かったものは、構造ごと入る。
        assert_eq!(std::fs::read_to_string(to.join("仕事/b.md")).unwrap(), "むかしの b\n");
    }

    /// 一枚だけの zip も、フォルダだけの zip も、同じ場所へ戻せるか。
    ///
    /// 全体の zip は `ノート/…` という一つの山の下にあるので頭を外すが、
    /// **一枚だけの zip は山になっていない** ── そこで同じ外し方をすると
    /// ファイル名そのものが外れて、何も戻らない。
    #[test]
    fn 一枚だけの_zip_も戻せる() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("ノート");
        std::fs::create_dir_all(root.join("仕事")).unwrap();
        std::fs::write(root.join("仕事/週報.md"), "今週の\n").unwrap();

        let one = call("backup", &serde_json::json!({
            "path": root.display().to_string(), "scope": "note",
            "what": root.join("仕事/週報.md").display().to_string(),
            "into": d.path().display().to_string(),
        })).unwrap()["path"].as_str().unwrap().to_string();

        let to = d.path().join("さら");
        let back = call("restore", &serde_json::json!({
            "zip": one, "to": to.display().to_string(),
        })).unwrap();
        assert_eq!(back["put"].as_u64().unwrap(), 1, "{back}");
        assert_eq!(std::fs::read_to_string(to.join("週報.md")).unwrap(), "今週の\n");

        // **フォルダの zip は、そのフォルダごと戻る。** 中身だけ根に散ると、
        // 戻したのに元の場所に戻っていない ── そうなっていた。
        let book = call("backup", &serde_json::json!({
            "path": root.display().to_string(), "scope": "book", "what": "仕事",
            "into": d.path().display().to_string(),
        })).unwrap()["path"].as_str().unwrap().to_string();
        let to2 = d.path().join("さら2");
        call("restore", &serde_json::json!({
            "zip": book, "to": to2.display().to_string(),
        })).unwrap();
        assert_eq!(std::fs::read_to_string(to2.join("仕事/週報.md")).unwrap(), "今週の\n");

        // 札そのものは、ノート帳に置いていかない。
        assert!(!to2.join(crate::zipbox::LABEL).exists());
    }

    /// 一世代は「書いていた一区切り」か。
    ///
    /// **保存のたびに残すと、十分書けば数十世代になる** ── 五十世代が一回の
    /// 執筆で埋まり、「昨日の姿」を訊いたときにはもう無い。
    #[test]
    fn 続けて打っているあいだは_一世代のまま() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().to_path_buf();
        let note = root.join("a.md");
        std::fs::write(&note, "一\n").unwrap();

        let keep = |text: &str, gap: u64| {
            call("keep", &serde_json::json!({
                "root": root.display().to_string(), "path": note.display().to_string(),
                "text": text, "gap": gap,
            })).unwrap()["stamp"].as_str().map(str::to_string)
        };
        let count = || {
            call("history", &serde_json::json!({
                "root": root.display().to_string(), "path": note.display().to_string(),
            })).unwrap()["versions"].as_array().unwrap().len()
        };

        assert!(keep("一\n", 300).is_some(), "はじめの一つは残る");
        assert_eq!(count(), 1);
        // 打鍵のたびに呼ばれても、間が空いていなければ増えない。
        for n in 2..30 {
            keep(&format!("{n}\n"), 300);
        }
        assert_eq!(count(), 1, "続けて打っているあいだは増えない");
        // 間が空いたことにすれば、一段増える。
        assert!(keep("あとで\n", 0).is_some());
        assert_eq!(count(), 2);
        // **同じ中身は二度置かない** ── 開いて閉じただけで増えると、履歴が
        // 「触った回数」の記録になる。
        assert!(keep("あとで\n", 0).is_none());
        assert_eq!(count(), 2);
    }

    /// 古いものは落ちるか。**印を付けたものは残るか。**
    #[test]
    fn 五十を超えたら落ちる_ただし印のあるものは残る() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().to_path_buf();
        let note = root.join("a.md");
        std::fs::write(&note, "").unwrap();
        let shelf = crate::history::shelf(&root, &note).unwrap();
        std::fs::create_dir_all(&shelf).unwrap();

        // 六十世代を、古い日付で置く（三十日より前）。
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 86_400);
        for n in 0..60 {
            let at = shelf.join(format!("2026-01-{:02}T00-00-{:02}.md", n / 24 + 1, n % 60));
            std::fs::write(&at, format!("{n}")).unwrap();
            let f = std::fs::File::options().write(true).open(&at).unwrap();
            f.set_modified(old).unwrap();
        }
        // 一つだけ「残す」の印を付ける（いちばん古いもの）。
        let oldest = "2026-01-01T00-00-00";
        std::fs::rename(shelf.join(format!("{oldest}.md")),
                        shelf.join(format!("{oldest}.keep.md"))).unwrap();

        // ここで一つ足すと、掃除が走る。
        call("keep", &serde_json::json!({
            "root": root.display().to_string(), "path": note.display().to_string(),
            "text": "あたらしい", "gap": 0,
        })).unwrap();

        let rows = call("history", &serde_json::json!({
            "root": root.display().to_string(), "path": note.display().to_string(),
        })).unwrap();
        let v = rows["versions"].as_array().unwrap();
        // 印の無いものは新しい五十まで（＋いま足した一つ）。
        assert!(v.len() <= crate::history::KEEP_GENS + 2, "{} 件", v.len());
        // **印のあるものは、どれだけ古くても残る。**
        assert!(v.iter().any(|x| x["stamp"] == oldest && x["kept"] == true),
                "印のいちばん古い姿が消えました");
    }

    /// フォルダを訊くと、その中のノートの姿が**まとめて**時系列で出るか。
    #[test]
    fn フォルダに訊くと_中のノートの姿がまとめて出る() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().to_path_buf();
        std::fs::create_dir_all(root.join("仕事")).unwrap();
        for (at, text) in [("仕事/a.md", "あ"), ("仕事/b.md", "い"), ("外.md", "そと")] {
            let note = root.join(at);
            std::fs::write(&note, text).unwrap();
            call("keep", &serde_json::json!({
                "root": root.display().to_string(), "path": note.display().to_string(),
                "text": text, "gap": 0,
            })).unwrap();
        }
        let rows = call("history", &serde_json::json!({
            "root": root.display().to_string(),
            "path": root.join("仕事").display().to_string(),
        })).unwrap();
        let v = rows["versions"].as_array().unwrap();
        assert_eq!(v.len(), 2, "{v:?}");
        // どのノートのものかを言う ── 言わないと、まとめた一覧が読めない。
        let notes: Vec<&str> = v.iter().map(|x| x["note"].as_str().unwrap()).collect();
        assert!(notes.contains(&"仕事/a.md"), "{notes:?}");
        assert!(notes.contains(&"仕事/b.md"), "{notes:?}");
    }

    /// 前の姿を読み出せるか。**ノートの置き場所の外は触れないか。**
    #[test]
    fn 前の姿を読み出せる_外は触れない() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().to_path_buf();
        let note = root.join("a.md");
        std::fs::write(&note, "むかし\n").unwrap();
        let stamp = call("keep", &serde_json::json!({
            "root": root.display().to_string(), "path": note.display().to_string(),
            "text": "むかし\n", "gap": 0,
        })).unwrap()["stamp"].as_str().unwrap().to_string();

        let got = call("oldtext", &serde_json::json!({
            "root": root.display().to_string(), "path": note.display().to_string(),
            "stamp": stamp,
        })).unwrap();
        assert_eq!(got["text"].as_str().unwrap(), "むかし\n");

        // 置き場所の外のノートには、履歴を作らない。
        let outside = d.path().parent().unwrap().join("よそ.md");
        assert!(call("keep", &serde_json::json!({
            "root": root.display().to_string(), "path": outside.display().to_string(),
            "text": "よそ", "gap": 0,
        })).is_err());
    }

    #[test]
    fn 合わせ方を決めて_運び終わったぶんを憶える() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("こちらだけ.md"), "本文\n").unwrap();
        std::fs::write(d.path().join("両方.md"), "本文\n").unwrap();
        let root = d.path().to_string_lossy().to_string();

        // 一度目 ── 憶えが無いので、こちらのは上へ、向こうのは下へ。
        let out = call("syncplan", &serde_json::json!({
            "path": root, "who": "drive",
            "remote": [
                { "rel": "両方.md", "id": "id2", "tag": "v1" },
                { "rel": "あちらだけ.md", "id": "id3", "tag": "v1" },
            ],
        }))
        .unwrap();
        let steps = out["steps"].as_array().unwrap();
        let got: Vec<(&str, &str)> = steps
            .iter()
            .map(|s| (s["rel"].as_str().unwrap(), s["do"].as_str().unwrap()))
            .collect();
        assert_eq!(got, vec![
            ("あちらだけ.md", "down"),
            ("こちらだけ.md", "up"),
            // 前に合わせたことがなく、同じ名前で両方にある ── 混ぜる。
            ("両方.md", "clash"),
        ], "{got:?}");

        // 運び終わったことにする。
        call("synced", &serde_json::json!({
            "path": root, "who": "drive",
            "done": [{ "rel": "こちらだけ.md", "id": "id1", "tag": "v1" }],
        }))
        .unwrap();

        // 二度目 ── **憶えたぶんは、もう運ばない。**
        let remote = serde_json::json!([
            { "rel": "こちらだけ.md", "id": "id1", "tag": "v1" },
            { "rel": "両方.md", "id": "id2", "tag": "v1" },
        ]);
        let out = call("syncplan", &serde_json::json!({
            "path": root, "who": "drive", "remote": remote,
        }))
        .unwrap();
        let got: Vec<(&str, &str)> = out["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["rel"].as_str().unwrap(), s["do"].as_str().unwrap()))
            .collect();
        // 憶えた一本は消え、まだ混ぜていない一本だけが残る。
        assert_eq!(got, vec![("両方.md", "clash")], "{got:?}");

        // 中身を直せば、また上へ。**時刻ではなく指紋で見ている。**
        std::fs::write(d.path().join("こちらだけ.md"), "直した\n").unwrap();
        let out = call("syncplan", &serde_json::json!({
            "path": root, "who": "drive", "remote": remote,
        }))
        .unwrap();
        let up = out["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["rel"] == "こちらだけ.md")
            .unwrap()
            .clone();
        assert_eq!(up["do"], "up");
        assert_eq!(up["id"], "id1", "行き先を憶えているので、作り直さない");
    }

    #[test]
    fn 家族と分けるフォルダを_一つ憶える() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("共有").join("買い物")).unwrap();
        std::fs::create_dir_all(d.path().join("仕事")).unwrap();
        std::fs::write(d.path().join("共有").join("リスト.md"), "- [ ] 牛乳\n").unwrap();
        std::fs::write(d.path().join("共有").join("買い物").join("週末.md"), "本文\n").unwrap();
        std::fs::write(d.path().join("仕事").join("週報.md"), "本文\n").unwrap();
        std::fs::write(d.path().join("ひとりごと.md"), "本文\n").unwrap();
        let root = serde_json::json!({ "path": d.path().to_string_lossy() });

        // 決めるまでは、何も共有していない ── **空を「全部」と読まない。**
        let out = call("notes", &root).unwrap();
        assert!(out["shares"].as_array().unwrap().is_empty());
        assert!(out["notes"].as_array().unwrap().iter().all(|n| n["shared"] == false));

        call("share", &serde_json::json!({
            "path": d.path().to_string_lossy(), "folder": "共有",
            "by": "Taketan", "today": "2026-09-07",
        }))
        .unwrap();

        // **印はフォルダの中に置く** ── 設定ではなく、フォルダ自身が言う。
        assert!(d.path().join("共有").join(crate::notebook::SHARE_MARK).is_file());
        let mark = crate::notebook::share_mark(&d.path().join("共有")).unwrap();
        assert_eq!(mark.by, "Taketan");

        let out = call("notes", &root).unwrap();
        assert_eq!(out["shares"][0]["at"], "共有");
        assert_eq!(out["shares"][0]["by"], "Taketan");
        // **題ではなく道で確かめる** ── 題は一行目から決まるので、
        // `- [ ] 牛乳` で始まるノートの題は「牛乳」になる（そこはこの
        // 試験の話ではない）。
        let shared: Vec<&str> = out["notes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["shared"] == true)
            .map(|n| n["rel"].as_str().unwrap_or(""))
            .collect();
        // フォルダそのものと、その下ぜんぶ。
        assert_eq!(shared.len(), 2, "共有/ の下は全部: {shared:?}");
        assert!(shared.contains(&"共有/リスト.md"));
        assert!(shared.contains(&"共有/買い物/週末.md"));

        // **フォルダが無ければ作る** ── 「共有する」を押した人に、その前に
        // 「フォルダを作る」を押させない。
        call("share", &serde_json::json!({
            "path": d.path().to_string_lossy(), "folder": "あたらしい棚",
            "by": "Taketan", "today": "2026-09-07",
        }))
        .unwrap();
        assert!(d.path().join("あたらしい棚").is_dir());

        // やめれば、誰も共有していない ── ノートは一本も書き換えない。
        for f in ["共有", "あたらしい棚"] {
            call("share", &serde_json::json!({
                "path": d.path().to_string_lossy(), "folder": f, "off": true,
            }))
            .unwrap();
        }
        let out = call("notes", &root).unwrap();
        assert!(out["shares"].as_array().unwrap().is_empty());
        assert!(out["notes"].as_array().unwrap().iter().all(|n| n["shared"] == false));
        assert!(d.path().join("共有").join("リスト.md").is_file(), "ノートは残る");
    }

    #[test]
    fn クラウドの置き土産を_一覧が言う() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("買い物リスト.md"), "- [ ] 牛乳\n").unwrap();
        // 落ちてきていない札（隠しファイル）。
        std::fs::write(d.path().join(".週報.md.icloud"), "").unwrap();
        // クラウドが作った控え ── **ノートとしては並ぶ**。
        std::fs::write(
            d.path().join("買い物リスト (Taketan の競合コピー 2026-09-06).md"),
            "- [ ] 牛乳\n- [ ] パン\n",
        )
        .unwrap();

        let out = call("notes", &serde_json::json!({ "path": d.path().to_string_lossy() })).unwrap();

        let waiting = out["waiting"].as_array().unwrap();
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0]["of"], "週報.md");

        let notes = out["notes"].as_array().unwrap();
        assert_eq!(notes.len(), 2, "控えも一覧に出る ── 消すと中身を助け出せない");
        let clash: Vec<&serde_json::Value> =
            notes.iter().filter(|n| !n["clash"].is_null()).collect();
        assert_eq!(clash.len(), 1);
        assert_eq!(clash[0]["clash"]["of"], "買い物リスト.md");
        assert_eq!(clash[0]["clash"]["by"], "Taketan");

        // ふつうのノートに札は付かない。
        let plain: Vec<&serde_json::Value> =
            notes.iter().filter(|n| n["clash"].is_null()).collect();
        assert_eq!(plain.len(), 1);
    }

    #[test]
    fn 置き場所は_二度目でも作れる() {
        // **`mkbook` と分けてある理由そのもの。** 一度目に作ったフォルダが
        // 二度目にはエラーになるのはおかしい ── クラウドを選び直した人は、
        // たいてい同じところをもう一度選ぶ。
        let d = tempfile::tempdir().unwrap();
        let ask = |dir: &std::path::Path| {
            call("place", &serde_json::json!({ "dir": dir.to_string_lossy() }))
        };
        let dir = d.path().join("amber");
        assert!(ask(&dir).is_ok());
        assert!(dir.is_dir());
        assert!(ask(&dir).is_ok(), "二度目で止まってはいけない");

        // 深いところも、途中を作りながら。
        let deep = d.path().join("Library").join("CloudStorage").join("Dropbox").join("amber");
        assert!(ask(&deep).is_ok());
        assert!(deep.is_dir());

        // 場所が空なら、作らずに言う。
        assert!(call("place", &serde_json::json!({ "dir": "" })).is_err());
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
}
