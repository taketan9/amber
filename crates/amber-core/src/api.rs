//! リクエストに答える 1 か所。**入口が 2 つあっても、答えは 1 つ。**
//!
//! iPhone は C ABI（`amber-ffi`）から、デスクトップ版は標準入出力（`amber-server`）
//! から、ここへ来る。以前は iPhone しか無かったので dispatcher は ffi の中にあったが、
//! **デスクトップ版を足すときにコピーすると、そこから 2 つの答えが育つ** ── 「同じ操作なのに
//! Mac と iPhone で結果が違う」は、一度の編集で作れてしまう。
//!
//! 取り決め: `call` は JSON を受けて JSON を返す。失敗は `Err` で返し、入口側が
//! `{"error": "…"}` に包む。**入口は包むだけで、判断しない。**



fn arg(p: &serde_json::Value, key: &str) -> String {
    p[key].as_str().unwrap_or("").to_string()
}

/// 錠がかかっていないか（依頼 629）。**書くところだけで訊く。**
///
/// 書式を付ける・タグを付ける・分割する、といった操作は文字列を返すだけで、ファイルに
/// 落ちるのは必ず `write` ── **門は一つでいい**。二つ目の門を作ると、
/// 片方だけ直した日にそこから書けてしまう。
///
/// `unlock: true` は「今だけ編集する」を押した人。押していないのに前端が
/// 勝手に添えることはない（デスクトップ版も iPhone も、操作されたときだけ添える）。
fn keep_out(p: &serde_json::Value, path: &std::path::Path) -> anyhow::Result<()> {
    crate::lock::keep_out(path, p["unlock"].as_bool().unwrap_or(false))
}

/// 操作の一覧を、Rust の側から見たもの。
///
/// `extern "C"` の殻から分けてあるので、下のテストは本物を走らせる ──
/// ポインタ操作ではなくこちらを呼ぶので、テストが述べる規則はすべて
/// iPhone にもそのまま効く規則になる。
pub fn call(method: &str, p: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
    match method {
        "version" => Ok(serde_json::json!({
            "amber": env!("CARGO_PKG_VERSION"),
            // このビルドがデスクトップ環境の上で動いているか。iPhone 版は違うので、
            // ゴミ箱への `delete` はそこでは拒否する ── いきなり消すのではなく。
            //
            "desktop": crate::DESKTOP,
        })),

        // ディレクトリ配下のすべてのノート。デスクトップ版が呼ぶのと同じ走査。
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
            // **開いたときに、前の隠しフォルダを引き取る。** `.cian` と
            // `.amber` の二つが人のノートのフォルダに並んでいるのを、放って
            // おかない ── いつか片方だけ消される。移すものが無ければ何もしない。
            crate::notebook::tidy(&dir);
            let stop = std::sync::atomic::AtomicBool::new(false);
            let (found, walk) = crate::note::list(&dir, limits, &stop);
            let book = crate::notebook::read(&dir);
            // **共有かどうかは、フォルダ自身が言う。** 設定に書いていた頃は
            // 相手の amber に何も伝わらず、受け取った人が自分で教え直す手が
            // 要った（`notebook::SHARE_MARK` の註）。
            let shares = crate::notebook::shares(&dir, &walk.rows);
            // お気に入りのフォルダ ── ノートが実際に入っているものに加えて、作った
            // だけでまだ空のものも。後半が無いと、最後のノートが出ていった瞬間に
            // フォルダが消え、amber がフォルダを失くしたように見える。
            //
            let mut shelves: Vec<String> = book.stars.clone();
            for f in &found {
                if let Some(sh) = f.note.star.clone() {
                    // Every level of it: a note on 買い物/週次 means 買い物
                    // 中に何かあるかどうかに関わらず、存在するものとして扱う。
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
                        // そのノートが入っているディレクトリ（ルートからの相対）── ここでも
                        // デスクトップ版と同じく、ノートブックはディレクトリのこと。
                        "book": f.rel.rsplit_once('/').map(|(d, _)| d).unwrap_or(""),
                        "title": n.title,
                        "excerpt": n.excerpt,
                        "tags": n.tags,
                        "updated": n.updated,
                        "created": n.created,
                        "bytes": n.bytes,
                        "star": n.star,
                        // iPhone が一覧を絞り込むときの照合対象。デスクトップ版と同じものに
                        // that `#仕事` finds the same notes it finds in the
                        // なるように、向こうで導出させず、こちらから送る ── 向こうで導出
                        // させるのが、答えが 2 つに分かれていく道。
                        "search": crate::note::haystack(n),
                        // **家族と分けてあるか。** 判断は core に一つ ──
                        // デスクトップ版と iPhone に 2 つ書くと、片方だけ「共有」の表示が
                        // 出るノートができる。
                        "shared": crate::notebook::shared(
                            &shares,
                            f.rel.rsplit_once('/').map(|(d, _)| d).unwrap_or(""),
                        ),
                        // **クラウドが作った控えなら、そう言う。**
                        // 一覧から消さない ── 消すと、中身を取り戻す手段が
                        // どこにも無くなる。一覧に出したうえで目印を付ける。
                        "clash": n.path.file_name()
                            .and_then(|f| f.to_str())
                            .and_then(crate::cloud::shape)
                            .filter(|s| s.kind == crate::cloud::Kind::Clash)
                            .map(|s| serde_json::json!({ "of": s.of, "by": s.by })),
                    })
                })
                .collect();
            // フォルダの一覧も同じ走査から。**ノートからではなくディレクトリから
            // 導く** ── 作ったばかりのノートブックは空なので、中のノートから作った
            // 一覧には出てこない。それは「フォルダが作られなかった」と見分けが
            // つかない。
            //
            let mut books: Vec<String> = walk
                .rows
                .iter()
                .filter(|r| r.is_dir && r.rel != "attachments")
                .filter(|r| !r.rel.split('/').any(|p| p == "attachments"))
                .map(|r| r.rel.clone())
                .collect();
            books.sort();
            // ロックされたフォルダ（依頼 629）。**同じ走査から拾う** ── デスクトップ版が
            // フォルダごとに問い合わせ直すと、一覧を作るたびに何十回も往復する。
            // ルート自身も見る（保存ディレクトリまるごとの錠）。
            let locks: Vec<String> = std::iter::once(String::new())
                .chain(books.iter().cloned())
                .filter(|rel| dir.join(rel).join(crate::lock::MARK).exists())
                .collect();
            // **まだ落ちてきていないノート。** iCloud は中身を消して
            // `.買い物リスト.md.icloud` というプレースホルダを置くので、名前が違って
            // 一覧に出ない ── 黙っていると「ノートが消えた」にしか
            // 見えないが、待てば戻ってくるだけ。
            let waiting: Vec<serde_json::Value> = crate::cloud::waiting(&dir, &walk.rows)
                .into_iter()
                .map(|at| {
                    serde_json::json!({
                        "of": at.file_name().and_then(|f| f.to_str()).unwrap_or(""),
                        // 本来のパス ── iPhone はこれを iOS に渡して「ダウンロードして
                        // きて」と頼む。
                        "at": at.to_string_lossy(),
                    })
                })
                .collect();
            Ok(serde_json::json!({
                "root": dir.display().to_string(),
                // **1 つとは限らない。** 家族用と仕事用のフォルダが両方あって
                // いい ── 共有の目印はフォルダごとに置くので、数を制限する理由が無い。
                "shares": shares.iter().map(|s| serde_json::json!({
                    "at": s,
                    "by": crate::notebook::share_mark(&dir.join(s))
                        .map(|m| m.by).unwrap_or_default(),
                })).collect::<Vec<_>>(),
                "waiting": waiting,
                "books": books,
                // ロックされたフォルダ（ルートからの相対パス。空はルート自身）。
                "locks": locks,
                "stars": shelves,
                "colors": book.colors,
                // 共有へ入れたノートが、もといたフォルダ。**一覧と一緒に
                // 渡す** ── メニューに「どこへ戻すか」を出すのに要る（問い合わせに
                // 行くと、押す前に消費してしまう）。
                "came": book.came,
                "notes": notes,
                "partial": walk.partial().then(|| serde_json::json!({
                    "whole_to": walk.whole_to(),
                    "stopped": walk.stopped_at.is_some(),
                    "unopened": walk.unopened,
                })),
            }))
        }

        // ノート 1 件の本文と、読んだ時点でのファイルの状態。
        //
        // スタンプも一緒に返す。iPhone は保存時にそれを渡し直す必要があるから ──
        // 同期されたディレクトリを 2 台で使う状況こそ、この仕組み全体が存在する
        // 理由であり、「これはまだ自分が開いたファイルか」は、呼び出し側が
        // 訊き方を知っているべき問いではない。
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

        // ノートを書き戻す。ただし先に誰かが書いていたら書かない。
        //
        // 文字コード・改行コード・末尾の改行はファイル自身のものを、ここで
        // もう一度読む ── Shift_JIS + CRLF で来たノートはその形で戻す。
        // iPhone が UTF-8 + LF で保存すると、編集していないファイルの全行に
        // 差分が出たように Mac からは見える。
        "write" => {
            let path = std::path::PathBuf::from(arg(p, "path"));
            let text = arg(p, "text");
            keep_out(p, &path)?;
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
            // まだ存在しないノートは新規ノートであって、失敗ではない ──
            // iPhone は作ったばかりのものを書く。
            let mut f = crate::text::read(&path).unwrap_or_default();
            // **「今だけ編集する」で錠が落ちない**（依頼 629）── 保存は前書きごと
            // 書き直すので、`locked: true` の行が消えた内容を書くとロックまで外れる。
            // 外すのは「ロックをやめる」だけ、と本人が決めた（2026-09-18）。
            let text = if crate::lock::note_locked(&path) {
                crate::note::set_field(&text, "locked", Some("true"))
            } else {
                text
            };
            f.lines = text.split('\n').map(|l| l.to_string()).collect();
            crate::text::write(&path, &f)?;
            Ok(serde_json::json!({
                "ok": true,
                "stamp": crate::stamp::of(&path).as_ref().map(stamp_json),
            }))
        }

        // 新しいノート。名前も形も、デスクトップ版と同じ規則で決める。
        "new" => {
            let dir = std::path::PathBuf::from(arg(p, "dir"));
            keep_out(p, &dir.join("新しいノート.md"))?;
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

        // ノートを描画単位にしたもの。iPhone の表示側が使う ── 何が見出し
        // *である*かは `crate::note` が決め、見出しが*どう見えるか*は iPhone が
        // 決める。逆の分け方をすると、テストの届かない場所に Markdown パーサーを
        // 置くことになる。
        "blocks" => {
            let text = if p["text"].is_string() {
                arg(p, "text")
            } else {
                let path = std::path::PathBuf::from(arg(p, "path"));
                crate::text::read(&path)?.lines.join("\n")
            };
            use crate::note::Block;
            // 行の色付き部分。ここで求めることで、デスクトップ版と iPhone が
            // ノートの解釈で食い違わない。
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
                    // **セルは `runs` を持って渡す。** 表の中にも太字は書かれる
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
        // 渡すのは**選択された文字列だけ**で、位置は渡さない ── JS は UTF-16 の桁で
        // 数え、Rust は文字で数えるので、絵文字が一つ混ざれば境目がずれる。
        // 返した文字列で、選択範囲を置き換えてもらう。
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
                other => anyhow::bail!("知らないマークです: {other}"),
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
        // `blocks` の隣にもう 1 つ口を開けているのは、デスクトップ版と iPhone で
        // 描き方が違うから ── iPhone は SwiftUI の View を構築するので `blocks` が要り、
        // デスクトップ版は
        // HTML を流し込むほうが速い。**解釈は一つ**（どちらも
        // `note::blocks` と同じ行単位の読み方を通る）で、分かれるのは
        // 最後の組み立てだけ。
        //
        // エスケープ（`javascript:` を落とす、`onclick` も `class` も文字として出す、
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

        // ノートをお気に入りフォルダに入れる、または外す。
        //
        // ほかの編集と同じく、テキストを受けてテキストを返す。お気に入りは
        // ノート自身の front matter の 1 行なので、ノートと一緒に移動し、
        // Mac も同じ語を読む。
        "star" => {
            let text = arg(p, "text");
            let shelf = p["shelf"].as_str();
            let out = match shelf {
                // `star:` ではなく `star: true` と書く ── 値の無いフィールドは、
                // 次に読んだものから「はい」と解釈される。
                Some("") => crate::note::set_field(&text, "star", Some("true")),
                Some(sh) => crate::note::set_field(&text, "star", Some(sh)),
                None => crate::note::set_field(&text, "star", None),
            };
            // お気に入りに名前が付く前の書き方。残しておくと、お気に入りから外した
            // あともお気に入りのままになる。
            let out = crate::note::set_field(&out, "pinned", None);
            Ok(serde_json::json!({ "text": out }))
        }

        // お気に入りフォルダを作る／忘れる。明示的に記録が要るのは空のものだけ ──
        // ほかは中のノートが名前を持っている。
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
            let mut here: Vec<crate::sync::Here> = found
                .iter()
                .map(|f| crate::sync::Here {
                    rel: f.rel.clone(),
                    hash: crate::sync::fingerprint(
                        &std::fs::read(&f.note.path).unwrap_or_default(),
                    ),
                })
                .collect();
            // 画像も一緒に（依頼 497）。
            here.extend(crate::sync::assets(&root));
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
            // こちらで改名して、まだ向こうに伝えていないもの（依頼 492）。
            let moves = crate::sync::moves(&root, who);
            let steps: Vec<serde_json::Value> = crate::sync::plan_with_moves(&here, &there, &was, &moves)
                .into_iter()
                .map(|s| {
                    let from = match &s {
                        crate::sync::Step::MoveThere { from, .. }
                        | crate::sync::Step::MoveHere { from, .. } => Some(from.clone()),
                        _ => None,
                    };
                    let (id, rel) = match &s {
                        crate::sync::Step::Up { id, rel } => (id.clone(), rel.clone()),
                        crate::sync::Step::Down { id, rel }
                        | crate::sync::Step::DropThere { id, rel }
                        | crate::sync::Step::Clash { id, rel } => (Some(id.clone()), rel.clone()),
                        crate::sync::Step::DropHere { rel } => (None, rel.clone()),
                        crate::sync::Step::MoveThere { id, to, .. }
                        | crate::sync::Step::MoveHere { id, to, .. } => (Some(id.clone()), to.clone()),
                    };
                    // **ぶつかったら、分かれる前の姿の指紋を添える** ── 呼ぶ側は
                    // `baseread` でその中身を取り、三方向で混ぜる。
                    let base = match &s {
                        crate::sync::Step::Clash { .. } => was
                            .iter()
                            .find(|w| w.rel == rel)
                            .map(|w| serde_json::Value::String(w.hash.clone()))
                            .unwrap_or(serde_json::Value::Null),
                        _ => serde_json::Value::Null,
                    };
                    serde_json::json!({ "do": s.word(), "rel": rel, "id": id, "base": base, "from": from,
                                        "bin": crate::sync::is_asset(&rel) })
                })
                .collect();
            Ok(serde_json::json!({ "steps": steps }))
        }

        // **タイトルに合わせて改名する**（依頼 492）。改名したら新しいパスを返す。いつ
        // 呼ぶかは呼ぶ側（題の欄から出た・ノートから離れた）。
        "settle" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let note = std::path::PathBuf::from(arg(p, "note"));
            match crate::naming::settle(&root, &note)? {
                Some((to, rewrote)) => Ok(serde_json::json!({
                    "path": to.display().to_string(), "renamed": true, "rewrote": rewrote,
                })),
                None => Ok(serde_json::json!({ "path": note.display().to_string(), "renamed": false, "rewrote": false })),
            }
        }

        // 向こうで改名されたものを、こちらでも改名する（`movehere`）。その名前が
        // 取られていれば番号を付け、**その改名は向こうへ伝えるものとして残す**。
        "syncmove" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let from = root.join(arg(p, "from"));
            let want = arg(p, "to");
            let to = root.join(&want);
            if !from.is_file() {
                anyhow::bail!("{} がありません", from.display());
            }
            let (to, deviated) = if to.exists() {
                let dir = to.parent().map(std::path::Path::to_path_buf).unwrap_or_else(|| root.clone());
                let stem = to.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let mut n = 2;
                let mut at = dir.join(format!("{stem}.{n}.md"));
                while at.exists() && n < 1000 {
                    n += 1;
                    at = dir.join(format!("{stem}.{n}.md"));
                }
                (at, true)
            } else {
                (to, false)
            };
            let rewrote = crate::naming::relocate(&root, &from, &to, deviated)?;
            let rel = to
                .strip_prefix(&root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or(want);
            Ok(serde_json::json!({ "path": to.display().to_string(), "rel": rel, "deviated": deviated, "rewrote": rewrote }))
        }

        // 時刻の名前のまま残っているノートを、一度だけ題の名前に揃える（決めごと 7）。
        "tidynames" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let done: Vec<serde_json::Value> = crate::naming::tidy_names(&root)
                .into_iter()
                .map(|(a, b)| serde_json::json!({ "from": a.display().to_string(), "to": b.display().to_string() }))
                .collect();
            Ok(serde_json::json!({ "renamed": done }))
        }

        // 1 件の指紋（`sync::fingerprint`）── アップロード時に向こうへメタデータとして付ける。
        "syncprint" => {
            let bytes = std::fs::read(arg(p, "path")).unwrap_or_default();
            Ok(serde_json::json!({ "print": crate::sync::fingerprint(&bytes) }))
        }

        // 向こうから下ろしたものを置く。**親のフォルダが無ければ作り、仮の名で
        // 書いてから改名する**（Git の object の書き方の写し）── 途中で切れても
        // 半端なノートを残さない。
        "syncdown" => {
            let path = std::path::PathBuf::from(arg(p, "path"));
            let text = arg(p, "text");
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("md.amber-part");
            std::fs::write(&tmp, text.as_bytes())?;
            std::fs::rename(&tmp, &path)?;
            Ok(serde_json::json!({
                "ok": true,
                "stamp": crate::stamp::of(&path).as_ref().map(stamp_json),
            }))
        }

        // 分かれる前の姿の中身（`synced` が取っておいたもの）。無ければ null。
        "baseread" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let hash = arg(p, "hash");
            let at = root.join(".amber").join("base").join(&hash);
            Ok(serde_json::json!({ "text": std::fs::read_to_string(at).ok() }))
        }

        // 運び終わったぶんを憶える。
        //
        // **全部やるか何もしないか、にしない** ── 電波の悪いところで一本も
        // 進まなくなる。運べたぶんだけ憶えて、残りは次に。
        "synced" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let who = p["who"].as_str().unwrap_or("drive");
            // **合わせた姿の中身を取っておく**（Git の merge base の写し）──
            // 指紋だけでは三方向に混ぜられない。`.amber/base/<指紋>` に置く。
            let keep_base = p["base"].as_bool().unwrap_or(true);
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
            if keep_base {
                let dir = root.join(".amber").join("base");
                std::fs::create_dir_all(&dir)?;
                for d in &done {
                    // 画像の分かれる前の姿は取っておかない（混ぜないので要らない）。
                    if crate::sync::is_asset(&d.rel) {
                        continue;
                    }
                    let at = dir.join(&d.hash);
                    if !at.exists() {
                        if let Ok(bytes) = std::fs::read(root.join(&d.rel)) {
                            let _ = std::fs::write(&at, bytes);
                        }
                    }
                }
            }
            crate::sync::remember(&root, who, &done, &gone)?;
            // 伝え終わった改名（`movethere` を実行したパス）は記録から消す。
            let moved: Vec<String> = p["moved"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            crate::sync::forget_moves(&root, who, &moved);
            if keep_base {
                // もう指されていない姿は捨てる ── 増えるだけの引き出しにしない。
                let live: std::collections::HashSet<String> =
                    crate::sync::recall(&root, who).into_iter().map(|w| w.hash).collect();
                if let Ok(rd) = std::fs::read_dir(root.join(".amber").join("base")) {
                    for e in rd.flatten() {
                        if let Some(name) = e.file_name().to_str() {
                            if !live.contains(name) {
                                let _ = std::fs::remove_file(e.path());
                            }
                        }
                    }
                }
            }
            Ok(serde_json::json!({ "kept": done.len(), "dropped": gone.len() }))
        }

        // 共有フォルダにする（`off` で解除）。
        //
        // **分けるのはクラウドの仕事。** amber がするのは、そのフォルダに
        // ファイルを 1 つ置くことだけ ── それだけで「このノートを共有する」が
        // そのフォルダへ移すことになり、いまあるフォルダの仕組みがそのまま
        // 効く。そしてそのファイルはフォルダと一緒に移動するので、**受け取った人は
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

        // フォルダの色。選ぶのは使う人 ── amber はパレットを提案するだけで、
        // 押し付けない。
        "color" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let folder = arg(p, "folder");
            crate::notebook::set_color(&root, &folder, p["color"].as_str())?;
            Ok(serde_json::json!({ "colors": crate::notebook::read(&root).colors }))
        }

        // テキストを色で包む。amber が書くのと同じ形式で。フロントエンドでは
        // なくここに置く ── 記法は 1 つの決めごとで、それを書く場所が 2 つ
        // あれば、1 回の編集で記法が 2 つになる。
        "paint" => {
            Ok(serde_json::json!({
                "text": crate::note::paint(&arg(p, "text"), &arg(p, "color")),
            }))
        }
        // 全部を新しい場所へ移す ── `notebook::migrate`、デスクトップ版が
        // 呼ぶのと同じもの。コピー → 確認 → 削除の順で、上書きは
        // 一切しない。
        "migrate" => {
            let from = std::path::PathBuf::from(arg(p, "from"));
            let to = std::path::PathBuf::from(arg(p, "to"));
            Ok(serde_json::json!({ "moved": crate::notebook::migrate(&from, &to)? }))
        }

        // バックアップの書き戻し ── `notebook::restore`、デスクトップ版が
        // 呼ぶのと同じもの。
        // ── 前の姿 ──────────────────────────────────────────────
        //
        // **判断はここ。** いつ一世代にするか、何世代残すか、いつ落とすかは
        // デスクトップ版と iPhone で同じでなければならない ── 同じフォルダを 2 つの端末で
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

        // 「残す」の指定を付ける／外す。
        "keepmark" => {
            let root = std::path::PathBuf::from(arg(p, "root"));
            let note = std::path::PathBuf::from(arg(p, "path"));
            crate::history::mark(&root, &note, arg(p, "stamp").as_str(),
                                 p["kept"].as_bool().unwrap_or(true))?;
            Ok(serde_json::json!({ "ok": true }))
        }

        // フォルダに付けられる色。**定義は core が持つ** ── デスクトップ版と iPhone に
        // 同じ表を書いていた頃、十一色のうち六色がずれていた。
        // **予定のメモ欄のタグ**（依頼 523）── 誰の用事かは、メモの
        // いちばん最後のタグだけの行に置く。**読むのも書くのもここ 1 か所**なので、
        // デスクトップ版（`amber-cal` 経由）と iPhone（EventKit）が同じ答えになる。
        "caltag" => {
            // **まとめて訊けるようにする**（依頼 539）── 月の表には予定が
            // 何十本も並ぶ。一本ずつ訊くと、その数だけ行き来することになる。
            if let Some(many) = p["notes"].as_array() {
                let out: Vec<_> = many
                    .iter()
                    .map(|n| {
                        let notes = n.as_str().unwrap_or_default();
                        serde_json::json!({
                            "tags": crate::caltag::tags(notes),
                            "body": crate::caltag::body(notes),
                        })
                    })
                    .collect();
                return Ok(serde_json::json!({ "each": out }));
            }
            let notes = p["notes"].as_str().unwrap_or_default();
            Ok(serde_json::json!({
                "tags": crate::caltag::tags(notes),
                "body": crate::caltag::body(notes),
            }))
        }

        // タグを書き換えた**メモ欄ぜんぶ**を返す。人の文章には触らない。
        // 書き戻すのは呼び出し側（`amber-cal notes <id> <テキスト>` / EventKit）。
        "caltagset" => {
            let notes = p["notes"].as_str().unwrap_or_default();
            let tags: Vec<String> = p["tags"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|t| t.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            Ok(serde_json::json!({ "notes": crate::caltag::set(notes, &tags) }))
        }

        "palette" => Ok(serde_json::json!({
            "colors": crate::notebook::PALETTE
                .iter()
                .map(|(hex, name)| serde_json::json!({ "hex": hex, "name": name }))
                .collect::<Vec<_>>(),
        })),

        // 同じノートを二人が書いたときに、混ぜる。**繋がない** ── 3 つの
        // 内容を渡されて、マージ結果と「どの行が向こうから来たか」を返すだけ。
        // ファイルに書き戻すのは呼ぶ側（通信も保存も I/O）。
        "merge" => {
            let m = crate::merge::merge(&arg(p, "was"), &arg(p, "ours"), &arg(p, "theirs"));
            Ok(serde_json::json!({
                "text": m.text,
                "came": m.came,
                "both": m.both,
                "eyes": m.needs_eyes(),
                // ぶつかった場所（人が三択で選ぶ）── こちらの行の範囲と、その直後の向こうの行の範囲。
                "spots": m.spots.iter().map(|s| serde_json::json!({
                    "ours": [s.ours.0, s.ours.1], "theirs": [s.theirs.0, s.theirs.1],
                })).collect::<Vec<_>>(),
                // 前書きでぶつかった鍵（本文にはこちらの値が置いてある）。
                "fields": m.fields.iter().map(|f| serde_json::json!({
                    "key": f.key, "ours": f.ours, "theirs": f.theirs,
                })).collect::<Vec<_>>(),
            }))
        }

        // よそから .md を持ってくる。**上書きしない**（`notebook::bring`）。
        "bring" => {
            let to = std::path::PathBuf::from(arg(p, "to"));
            keep_out(p, &to.join("入れるもの.md"))?;
            let files: Vec<std::path::PathBuf> = p["files"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).map(std::path::PathBuf::from).collect())
                .unwrap_or_default();
            let r = crate::notebook::bring(&files, &to)?;
            Ok(serde_json::json!({ "put": r.put, "renamed": r.renamed, "failed": r.failed }))
        }

        "restore" => {
            let zip = std::path::PathBuf::from(arg(p, "zip"));
            let to = std::path::PathBuf::from(arg(p, "to"));
            keep_out(p, &to.join("戻すもの.md"))?;
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

        // 検索ボックスの意味を、語のグループとして表したもの。
        //
        // ノートごとではなくクエリごとに 1 回問い合わせる ── クエリの*意味*が
        // 決めごとでここに属する。iPhone のメモリ上にある一覧に対して実行
        // する部分は、そうではない。
        "terms" => {
            // 前端は **見出しごとに探し分ける**（題だけ・タグだけ・
            // フォルダだけ）。文字列だけを渡していた頃は、`tag:定型` と打っても
            // 「tag:定型」という文字列を本文から探していた。
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

        // ノートを「自己説明の部分」と「本文」に分ける。
        //
        // **編集側が本文だけを表示できるように。** front matter は amber の
        // 管理情報 ── 導出したタイトル、打った日付、シートから設定したタグ ──
        // であり、それを打っていない人が、自分の 1 行目に辿り着くために
        // そこをスクロールする必要は無い。
        // どこで終わるかは `note::front` の答えで、表示側もデスクトップ版も
        // 同じものを使う。
        "split" => {
            let text = arg(p, "text");
            let lines: Vec<String> = text.lines().map(str::to_string).collect();
            let n = crate::note::front(&lines).lines;
            // バイト位置で切らず、行から組み直す ── ノートは改行で終わらないことが
            // あり、先頭部分は自分の改行を保たなければならない。
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

        // チェックボックスを 1 つ切り替える。指定は行番号で。
        //
        // ここのほかの編集と同じく、テキストを受けてテキストを返す。呼び出し側が
        // 通常の手順で保存するので、チェックボックスを押す操作も、入力と同じ
        // 競合検査をディスク上のファイルに対して通る。ここではそれがどこよりも
        // 重要 ── チェックボックスは、人が
        // ノートを開かずに行う唯一の編集だから。
        "check" => {
            let text = arg(p, "text");
            let line = p["line"].as_u64().unwrap_or(0) as usize;
            let done = p["done"].as_bool().unwrap_or(false);
            Ok(serde_json::json!({ "text": crate::note::set_check(&text, line, done) }))
        }

        // 一覧が既に持っている情報だけでなく、ノートの中身まで見る。
        //
        // 一覧はノートごとに 1 行持っている ── タイトル、タグ、先頭 100 文字 ──
        // そして検索欄はそれに対して即座に絞り込む。だがそれでは足りない ──
        // 憶えている文はたいていもっと下にある。ここはファイルを走査していて、
        // デスクトップ版の `:grep` が使うのと同じ `search` を呼ぶ。
        //
        "find" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let cap = p["limit"].as_u64().unwrap_or(200) as usize;
            let cancel = std::sync::atomic::AtomicBool::new(false);
            // 判断は core に。**借りていた cian の grep をやめた** ── あちらは
            // 入力した文字列をそのまま含むかを見るだけで、デスクトップ版の `/` 絞り込みの
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

        // タグの付け外し。テキストを受けてテキストを返す ── 呼び出し側は
        // ほかの編集と同じ手順で保存するので、タグ付けも入力と同じ競合検査を
        // 通る。自分でファイルを書くタグ付けはノートを書く 2 つ目の経路になり、
        // 2 つ目の経路こそが他人の段落を消すもの。
        //
        "settags" => {
            let tags: Vec<String> = p["tags"]
                .as_array()
                .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
                .unwrap_or_default();
            Ok(serde_json::json!({
                "text": crate::note::set_tags(&arg(p, "text"), &tags),
            }))
        }

        // 単純なフィールドを 1 つ設定／削除する ── 今は `pinned`、明日は何であれ。
        // テキストを受けてテキストを返すので、ほかの編集と同じように保存される。
        "setfield" => {
            let value = p["value"].as_str();
            Ok(serde_json::json!({
                "text": crate::note::set_field(&arg(p, "text"), &arg(p, "key"), value),
            }))
        }

        // 画像の大きさを、押して選べるように書き換える（依頼 420）。
        // **決めるのはここ 1 か所** ── デスクトップ版と iPhone が別々に文字列をいじると、
        // 片方で付けた大きさをもう片方が読めない形になる。
        "imgsize" => {
            let width = p["width"].as_str();
            Ok(serde_json::json!({
                "line": crate::markdown::set_picture_size(&arg(p, "line"), width),
            }))
        }

        // 絵文字の表（依頼 418）。**一度受け取れば、あとは前端の仕事。**
        // 外部から取りに行かない ── 会社の閉じた端末でも動く。
        // **ノートから使われていない画像。**（依頼 449）
        //
        // 数えるだけで、消さない ── 消すのは呼び出し側（デスクトップ版はゴミ箱へ入れる）。
        // 読めなかったノートがあれば `unsure` で言う: 「使われていない」は
        // ぜんぶ読み切って初めて言えることで、黙って少なく数えるのが
        // いちばん危ない。
        "spare" => {
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
            let walk = crate::survey::survey(&dir, limits, &stop);
            let got = crate::spare::find(&dir, &walk.rows);
            Ok(serde_json::json!({
                "pictures": got.spare.iter().map(|s| serde_json::json!({
                    "path": s.path.to_string_lossy(),
                    "rel": s.rel,
                    "bytes": s.bytes,
                    "when": s.when,
                    "note": s.note,
                })).collect::<Vec<_>>(),
                "bytes": got.spare.iter().map(|s| s.bytes).sum::<u64>(),
                "unsure": got.unsure,
            }))
        }
        // **ひと月ぶんの予定**（依頼 453）。カレンダーの画面が要るのは
        // 「この月の、どの日に、何があるか」だけなので、一度で返す。
        //
        // いま乗るのは amber が既に知っているものだけ（前書きの `remind:`
        // と `repeat:`、それとノートを書いた日）── **語彙を増やさない**。
        // よそのカレンダーは、ここに足す形で入る。
        "month" => {
            let dir = std::path::PathBuf::from(arg(p, "path"));
            if !dir.is_dir() {
                anyhow::bail!("{} を開けません", dir.display());
            }
            let year = p["year"].as_i64().unwrap_or(0) as i32;
            let month = p["month"].as_u64().unwrap_or(0) as u32;
            if !(1..=12).contains(&month) {
                anyhow::bail!("月が 1〜12 ではありません: {month}");
            }
            let limits = crate::survey::Limits {
                depth: p["depth"].as_u64().unwrap_or(6) as usize,
                rows: 4000,
                hidden: false,
                ..Default::default()
            };
            let stop = std::sync::atomic::AtomicBool::new(false);
            let walk = crate::survey::survey(&dir, limits, &stop);
            let slots = crate::month::of(&walk.rows, year, month);
            Ok(serde_json::json!({
                "days": slots.iter().map(|s| serde_json::json!({
                    "day": s.day.to_string(),
                    "at": s.at.map(|t| t.format("%H:%M").to_string()),
                    "title": s.title,
                    "path": s.path,
                    "kind": s.kind.word(),
                })).collect::<Vec<_>>(),
            }))
        }
        // **よその予定表を読む**（依頼 456）。取りに行くのは呼んだ側
        // （デスクトップ版は `fetchPage`、iPhone は `URLSession`）── core はネットワークに触らない。
        // 返す形は `month` と同じなので、画面は混ぜて並べるだけでよい。
        "ics" => {
            let text = arg(p, "text");
            if !crate::ics::looks_like(&text) {
                anyhow::bail!("予定表の形をしていません");
            }
            let year = p["year"].as_i64().unwrap_or(0) as i32;
            let month = p["month"].as_u64().unwrap_or(0) as u32;
            if !(1..=12).contains(&month) {
                anyhow::bail!("月が 1〜12 ではありません: {month}");
            }
            let got = crate::ics::month(&text, year, month);
            Ok(serde_json::json!({
                "name": crate::ics::name(&text),
                "days": got.iter().map(|e| serde_json::json!({
                    "day": e.day.to_string(),
                    "at": e.at.map(|t| t.format("%H:%M").to_string()),
                    "to": e.to.map(|t| t.format("%H:%M").to_string()),
                    "title": e.title,
                    "place": e.place,
                    "kind": "away",
                })).collect::<Vec<_>>(),
            }))
        }
        // **人ごとに並べる予定表を、CSV から読む**（依頼 471）。
        //
        // 取りに行かない ── 別のツールが置いたファイルを読むだけ。取り決めは
        // `docs/team-csv.ja.md`。**読めなくても、理由を返して終わる** ──
        // 会社のネットワークの中にしか無いファイルなので、自宅では必ず読めない。
        "team" => {
            let at = std::path::PathBuf::from(arg(p, "path"));
            // **月を指定しなければ、ファイル全体**（依頼 476）。
            //
            // このファイルは「今日から何日ぶん」のもので、月ごとに分かれて
            // いない ── 月を替えるたびに読み直すと、同じ紙を何度も
            // 開くことになるうえ、**途中で置き換わると月によって時点の
            // 違うものが並ぶ**。呼ぶ側が一度読んで、持っておく。
            let year = p["year"].as_i64().unwrap_or(0) as i32;
            let month = p["month"].as_u64().unwrap_or(0) as u32;
            if month != 0 && !(1..=12).contains(&month) {
                anyhow::bail!("月が 1〜12 ではありません: {month}");
            }
            let file = crate::text::read(&at)
                .map_err(|e| anyhow::anyhow!("{} を読めません（{e}）", at.display()))?;
            let text = file.lines.join("\n");
            let got = if month == 0 {
                crate::team::all(&text)
            } else {
                crate::team::of(&text, year, month)
            };
            Ok(serde_json::json!({
                "fetched": got.fetched,
                "people": got.people.iter().map(|w| serde_json::json!({
                    "name": w.name,
                    "mail": w.mail,
                })).collect::<Vec<_>>(),
                "days": got.plans.iter().map(|p| serde_json::json!({
                    "day": p.day,
                    "at": p.at,
                    "to": p.to,
                    "title": p.title,
                    "place": p.place,
                    "who": p.who,
                    "mail": p.mail,
                    "show": p.show,
                    "shut": p.shut,
                    "kind": "team",
                })).collect::<Vec<_>>(),
            }))
        }

        "emoji" => Ok(crate::emoji::table()),

        // 同じ中身のノートをもう一つ（依頼 412）。
        "copy" => {
            let at = std::path::PathBuf::from(arg(p, "path"));
            // `dir` を渡されたら、そこへコピーする（テンプレートから作る経路）。
            let into = p["dir"].as_str().map(std::path::PathBuf::from);
            keep_out(p, into.as_deref().map(|d| d.join("写し.md")).as_deref().unwrap_or(&at))?;
            let made = crate::note::duplicate(&at, into.as_deref(), &crate::note::today())?;
            Ok(serde_json::json!({ "path": made.display().to_string() }))
        }

        // ノートを別のノートブックへ移す。画像もまとめて。
        //
        // `root` を渡されたら、履歴のディレクトリ・共有から戻る場所・同期の記録も一緒に移す
        // （依頼 496）── 同期は「消して新しく上げる」ではなく「名前が変わった」として運ぶ。
        "move" => {
            let note = std::path::PathBuf::from(arg(p, "path"));
            let dir = std::path::PathBuf::from(arg(p, "dir"));
            keep_out(p, &note)?;
            keep_out(p, &dir.join(note.file_name().map(std::ffi::OsString::from).unwrap_or_default()))?;
            let at = crate::note::move_to(&note, &dir)?;
            if let Some(root) = p["root"].as_str().filter(|r| !r.is_empty()) {
                let root = std::path::PathBuf::from(root);
                if at != note {
                    crate::naming::carry(&root, &note, &at, true);
                }
            }
            Ok(serde_json::json!({ "path": at.display().to_string() }))
        }

        // 共有フォルダへ入れたノートの、**移動前のフォルダ**を記録する／読み出す。
        //
        // 「共有をやめる」を押した人が探しているのは、そのノートが前に居た
        // ところ ── いままではいちばん上へ戻していて、フォルダに分けている
        // 人ほど「どこへ行った」になった。
        //
        // **ノートには書かない。** ノートはただの Markdown で、家族に渡った
        // ノートに「元は くらし に居た」と書いてあっても相手には意味が無い。
        // 設定ファイル（`.amber/settings.json`）に 1 行持つ。
        //
        // 憶えは `notes` が一覧と一緒に渡すので、ここは**書くだけ** ──
        // `from` があれば憶える、`forget` なら忘れる。戻したあとも憶えて
        // いると、別のフォルダへ移してからもう一度共有して外した人が
        // **二回前の場所**へ連れて行かれる。
        "came" => {
            let root = std::path::PathBuf::from(arg(p, "path"));
            let rel = arg(p, "rel");
            if p["forget"].as_bool().unwrap_or(false) {
                crate::notebook::came_back(&root, &rel);
            } else {
                crate::notebook::came_from(&root, &rel, &arg(p, "from"))?;
            }
            Ok(serde_json::json!({ "came": crate::notebook::read(&root).came }))
        }

        // ノートブックを作る。実体はフォルダ ── ここではノートブックとは
        // そういうもので、同じ場所を Mac から見た人にはフォルダが見える。
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

        // バックアップ。どこにでも置ける zip として。
        //
        // 範囲は呼び出し側が選び、答えはファイル ── amber はクラウドを知らないが、
        // iPhone の共有シートは全部知っている。フォルダのコピーではなく `zip` に
        // しているのは、フォルダはメールアプリに渡せるものではないから。
        //
        //
        // `all` ── ノートのルート配下すべて。画像も含む。
        // `book` ── ノートブック 1 つとその配下。
        // `tag`  ── そのタグを持つノートすべて。どこにあっても。
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
            // アプリ自身の一時ファイルの隣に置く。名前には中身と日付を入れる ──
            // `backup.zip` ばかりのフォルダは、「どれがどれか」という問いだけが
            // 並んだフォルダになる。
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

        // そのノートの通知設定と、繰り返しが溜めているぶん。
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

        // 期日が来た日のぶんの繰り返しを実行し、実行したことを記録する。
        // 2 つの手順を 1 回の呼び出しで ── 記録せずに作ったコピーは、明日も
        // もう一度作られる。
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

        // 写真をノートの隣に置く。iPhone が base64 で送るのは、それが C の文字列に
        // 載るから。*どこに置くか*に関わることはすべて `crate::note::attach` にあり、
        // デスクトップ版がエディタにスクリーンショットを貼り付けたときに呼ぶのと
        // 同じ関数。
        "image" => {
            let note = std::path::PathBuf::from(arg(p, "note"));
            keep_out(p, &note)?;
            let bytes = b64(&arg(p, "b64")).ok_or_else(|| anyhow::anyhow!("画像を読めません"))?;
            let link = crate::note::attach(&note, &bytes, &arg(p, "ext"))?;
            Ok(serde_json::json!({ "link": link, "bytes": bytes.len() }))
        }

        // ノートを削除する。
        //
        // 完全に削除する。iPhone には移動先のゴミ箱が無いため ── ここでは
        // `crate::DESKTOP` が false で、`DeleteMode::Trash` はふりをせずに
        // 拒否する。呼び出し側が先に確認している前提で、ここは取り消せない
        // 部分。
        "delete" => {
            let path = std::path::PathBuf::from(arg(p, "path"));
            if !path.is_file() {
                anyhow::bail!("{} がありません", path.display());
            }
            keep_out(p, &path)?;
            std::fs::remove_file(&path)?;
            Ok(serde_json::json!({ "ok": true }))
        }

        // OneNote を取り込む（依頼 621）。**デスクトップ版だけ** ── iPhone のバンドルには
        // 読み手が入っていないので、訊かれたら「ここには無い」と答える。
        // デスクトップ版はこれを**別のエンジン**に投げる（大きいノートブックを読むあいだ、ノートの
        // 一覧や保存を待たせない）。
        #[cfg(feature = "desktop")]
        "onenote_find" => {
            let dirs: Vec<std::path::PathBuf> = p["dirs"]
                .as_array()
                .map(|a| a.iter().filter_map(|d| d.as_str()).map(Into::into).collect())
                .unwrap_or_default();
            let got: Vec<_> = crate::onenote::find(&dirs)
                .into_iter()
                .map(|f| serde_json::json!({
                    "path": f.path.to_string_lossy(),
                    "name": f.name,
                    "bytes": f.bytes,
                    "modified": f.modified,
                }))
                .collect();
            Ok(serde_json::json!({ "found": got }))
        }
        #[cfg(feature = "desktop")]
        "onenote_open" => {
            let (key, mut sum) = crate::onenote::open(std::path::Path::new(&arg(p, "path")))?;
            sum["key"] = key.into();
            Ok(sum)
        }
        #[cfg(feature = "desktop")]
        "onenote_write" => {
            let key = p["key"].as_u64().ok_or_else(|| anyhow::anyhow!("key がありません"))?;
            let i = p["i"].as_u64().ok_or_else(|| anyhow::anyhow!("i がありません"))? as usize;
            let to = arg(p, "to");
            if to.is_empty() {
                anyhow::bail!("出力先がありません");
            }
            let w = crate::onenote::write(key, i, std::path::Path::new(&to))?;
            Ok(serde_json::json!({
                "dir": w.dir.to_string_lossy(),
                "pages": w.pages,
                "pictures": w.pictures,
                "files": w.files,
            }))
        }
        #[cfg(feature = "desktop")]
        "onenote_close" => {
            if let Some(key) = p["key"].as_u64() {
                crate::onenote::close(key);
            }
            Ok(serde_json::json!({ "ok": true }))
        }
        #[cfg(not(feature = "desktop"))]
        "onenote_find" | "onenote_open" | "onenote_write" | "onenote_close" => {
            anyhow::bail!("OneNote の取り込みは、パソコンの ambər だけにあります")
        }

        // 錠（依頼 629）。**かける／やめる**はここ、**今だけ外す**は
        // 書くときに `unlock: true` を添える（錠はそのまま残る）。
        "lock" => {
            let path = std::path::PathBuf::from(arg(p, "path"));
            let on = p["on"].as_bool().unwrap_or(true);
            if path.is_dir() {
                crate::lock::set_dir(&path, on)?;
            } else {
                crate::lock::set_note(&path, on)?;
            }
            Ok(crate::lock::tell(&path))
        }
        // いま錠か。**そのノート自身と、上のフォルダぜんぶ**を見た答え。
        "locked" => Ok(crate::lock::tell(std::path::Path::new(&arg(p, "path")))),

        other => anyhow::bail!("知らない操作: {other}"),
    }
}

fn chrono_today() -> chrono::NaiveDate {
    chrono::Local::now().date_naive()
}

/// base64 のデコード。外部クレートを入れずにここに書いてある ── 20 行で済むし、
/// フィールドを 1 つデコードするためだけの依存は、iOS 版が iPhone まで
/// 運ばなければならない依存になる。
fn b64(text: &str) -> Option<Vec<u8>> {
    // ブラウザが渡してくるのは data: URL なので、iPhone にもそれを送らせて
    // かまわない。
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

/// スタンプを、呼び出し側が保存して返すための 1 つの文字列にしたもの。
///
/// 以前は `{len, modified: <秒>}` で出していた。読みやすかったが誤りで、
/// 秒に丸めた時刻は元のファイルと等しくなくなるため、**すべての**保存が
/// 相手のいない競合として返ってきた。下のテストがそれを捕まえた。
/// `crate::stamp::token` は正確なまま保ち、iPhone は中身を知らなくて
/// よい。
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

    /// ロック（依頼 629）。**ゲートは `write` 側に 1 つ** ── 書式を付ける・タグを付けるは
    /// 文字列を返すだけなので、そこを通してもロックは破れない。
    #[test]
    fn 錠のかかったノートは書けない() {
        let d = note_dir();
        let a = d.path().join("a.md");
        let path = a.to_string_lossy().to_string();
        // かける（前書きに `locked: true`）。
        let r = call("lock", &serde_json::json!({ "path": path })).unwrap();
        assert_eq!(r["locked"], true);
        assert_eq!(call("locked", &serde_json::json!({ "path": path })).unwrap()["why"], "note");

        let write = |extra: serde_json::Value| {
            let mut p = serde_json::json!({ "path": path, "text": "書き換えた\n", "force": true });
            for (k, v) in extra.as_object().unwrap() { p[k] = v.clone(); }
            call("write", &p)
        };
        assert!(write(serde_json::json!({})).is_err(), "錠なのに書けた");
        assert!(call("delete", &serde_json::json!({ "path": path })).is_err(), "錠なのに消せた");
        assert!(std::fs::read_to_string(&a).unwrap().contains("本文です。"), "中身が変わった");

        // **今だけ編集する**（`unlock`）── 錠は外れたままにならない。
        assert!(write(serde_json::json!({ "unlock": true })).is_ok());
        assert_eq!(call("locked", &serde_json::json!({ "path": path })).unwrap()["locked"], true,
            "一度書いたら錠が外れてしまった");

        // **錠をやめる。**
        call("lock", &serde_json::json!({ "path": path, "on": false })).unwrap();
        assert_eq!(call("locked", &serde_json::json!({ "path": path })).unwrap()["locked"], false);
        assert!(write(serde_json::json!({})).is_ok());
    }

    #[test]
    fn 錠のかかったフォルダには入れられない() {
        let d = note_dir();
        let dir = d.path().to_string_lossy().to_string();
        call("lock", &serde_json::json!({ "path": dir })).unwrap();
        assert!(call("new", &serde_json::json!({ "dir": dir, "title": "新しい" })).is_err());
        assert!(call("copy", &serde_json::json!({
            "path": d.path().join("a.md").to_string_lossy(), "dir": dir,
        })).is_err());
        // 中のノートも錠（上のフォルダを見る）。
        let a = d.path().join("a.md").to_string_lossy().to_string();
        assert_eq!(call("locked", &serde_json::json!({ "path": a })).unwrap()["why"], "folder");
        assert!(call("write", &serde_json::json!({ "path": a, "text": "x", "force": true })).is_err());
        // やめれば、また作れる。
        call("lock", &serde_json::json!({ "path": dir, "on": false })).unwrap();
        assert!(call("new", &serde_json::json!({ "dir": dir, "title": "新しい" })).is_ok());
    }

    #[test]
    fn ノートは描画単位になって渡る() {
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
        // 選択が無いときは記号だけ ── 中にカーソルを置いて打てるように。
        assert_eq!(m("wrap", "`", ""), "``");

        // 行頭。**すべてに付いていれば外れる。**
        assert_eq!(m("line", "- ", "あ\nい"), "- あ\n- い");
        assert_eq!(m("line", "- ", "- あ\n- い"), "あ\nい");
        // 一つでも無ければ、揃える。
        assert_eq!(m("line", "- ", "- あ\nい"), "- あ\n- い");
        // 別の記号は付け替える ── `> - もの` はたいてい意図した結果ではない。
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
    fn デスクトップ版に渡す読める形は_前書きを出さず_危ないものを文字にする() {
        let r = call("html", &serde_json::json!({
            "text": "---\ntitle: x\ntags: [仕事]\n---\n# 題\n- [ ] やること\n",
        })).unwrap();
        let h = r["html"].as_str().unwrap();
        assert!(h.contains("<h1"), "見出しが組まれていない: {h}");
        assert!(!h.contains("title: x"), "前書きが漏れている: {h}");
        // チェックは行番号を持って渡る ── 何番目のチェックボックスかを数えると、
        // 前書きのあるノートでずれる。
        assert!(h.contains("data-line="), "チェックに行番号が無い: {h}");

        // 逃がしは `markdown::to_html` の中にある。**ここでも効くこと**を
        // 見ておかないと、口を増やしたときに素通りする経路ができる。
        let bad = call("html", &serde_json::json!({
            "text": "<script>alert(1)</script>\n\n[押す](javascript:alert(1))\n",
        })).unwrap();
        let h = bad["html"].as_str().unwrap();
        assert!(!h.contains("<script"), "script が生きている: {h}");
        assert!(!h.contains("javascript:"), "javascript: が生きている: {h}");
    }

    #[test]
    fn 憶えている文は_たいてい抜粋より下にある() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("long.md"),
            // その語は抜粋が切れる位置より後ろにある。この操作が存在する理由が
            // まさにそれ。
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

        // 検索語が無いことは「すべてのノート」を意味しない。
        let r = call("find", &serde_json::json!({
            "path": d.path().to_str().unwrap(), "needle": "  ",
        })).unwrap();
        assert!(r["hits"].as_array().unwrap().is_empty());
    }

    #[test]
    fn 空のノートブックもノートブック() {
        let d = note_dir();
        let made = call("mkbook", &serde_json::json!({
            "dir": d.path().join("仕事").to_str().unwrap(),
        })).unwrap();
        assert!(made["path"].as_str().unwrap().ends_with("仕事"));

        let r = call("notes", &serde_json::json!({ "path": d.path().to_str().unwrap() })).unwrap();
        let books: Vec<String> = r["books"].as_array().unwrap()
            .iter().map(|b| b.as_str().unwrap().to_string()).collect();
        // まだ中身が無くても表示されなければならない ── そうでないと、フォルダを
        // 作ったことと作られなかったことの見分けがつかない。
        assert!(books.contains(&"仕事".to_string()), "{books:?}");
        // `attachments` は画像の置き場所であって、ノートブックではない。
        let note = d.path().join("a.md");
        crate::note::attach(&note, &[1], "png").unwrap();
        let r = call("notes", &serde_json::json!({ "path": d.path().to_str().unwrap() })).unwrap();
        let books: Vec<String> = r["books"].as_array().unwrap()
            .iter().map(|b| b.as_str().unwrap().to_string()).collect();
        assert!(!books.iter().any(|b| b.contains("attachments")), "{books:?}");

        // 既にあるものを作ろうとしたら、黙って何もしない（それは成功に見える）
        // のではなく、拒否する。
        assert!(call("mkbook", &serde_json::json!({
            "dir": d.path().join("仕事").to_str().unwrap(),
        })).is_err());
    }

    #[test]
    fn バックアップは_ほかのアプリに渡せるファイルになる() {
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
            // 名前に中身と日付を入れる ── `backup.zip` ばかりのフォルダは、
            // 「どれがどれか」という問いだけが並んだフォルダになる。
            assert!(at.to_string_lossy().contains(&crate::note::today()), "{at:?}");
            assert!(std::fs::metadata(&at).unwrap().len() > 0, "{scope} は空でした");
        }

        // 誰も使っていないタグはエラーにする。バックアップに見えるが必要な日に
        // 空だと分かる zip にはしない。
        assert!(call("backup", &serde_json::json!({
            "path": root, "scope": "tag", "what": "ない",
            "into": out.path().to_str().unwrap(),
        })).is_err());
    }

    #[test]
    fn 繰り返しは一度だけ実行され_記録される() {
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
        // 記録する。しないと明日もう一度起きる。
        let after = call("remind", &serde_json::json!({ "path": path })).unwrap();
        assert_eq!(after["last"], "2026-09-02");
    }

    #[test]
    fn 写真はノートの隣に書かれ_リンクがそれを指す() {
        let d = note_dir();
        let note = d.path().join("a.md").to_str().unwrap().to_string();
        // "aGk=" は "hi"。パディングも data: の前置きも、往復で壊れてはいけない ──
        // 2 つの呼び出し側が実際に送ってくるのがそれだから。
        let r = call("image", &serde_json::json!({
            "note": note, "b64": "data:image/png;base64,aGk=", "ext": "png",
        })).unwrap();
        let link = r["link"].as_str().unwrap();
        assert_eq!(r["bytes"], 2);
        assert_eq!(std::fs::read(d.path().join(link)).unwrap(), b"hi");
        // 添付する中身が無いときに、ノートのリンクが指すことになる空ファイルを
        // 黙って作ったりはしない。
        assert!(call("image", &serde_json::json!({ "note": note, "b64": "" })).is_err());
    }

    #[test]
    fn 削除はノートを消し_無ければ無いと言う() {
        let d = note_dir();
        let note = d.path().join("a.md").to_str().unwrap().to_string();
        assert_eq!(call("delete", &serde_json::json!({ "path": note })).unwrap()["ok"], true);
        assert!(!d.path().join("a.md").exists());
        // 2 回目はエラーにする。黙って成功を返さない ── 呼び出し側は存在しない
        // ものの削除を求めている。
        assert!(call("delete", &serde_json::json!({ "path": note })).is_err());
    }

    #[test]
    fn 照合対象は_iphone_に渡す_向こうで導出させない() {
        let d = note_dir();
        let r = call("notes", &serde_json::json!({ "path": d.path().to_str().unwrap() })).unwrap();
        let notes = r["notes"].as_array().unwrap();
        assert_eq!(notes.len(), 1, "only the Markdown one is a note");
        assert_eq!(notes[0]["title"], "段取り");
        // The same line the window filters on, so `#仕事` narrows to the same
        // 両方のノート。向こう側で導出させると、答えがずれていく。
        let search = notes[0]["search"].as_str().unwrap();
        assert!(search.contains("#仕事"), "{search}");
        assert!(search.contains("段取り"), "{search}");
    }

    #[test]
    fn 読んで書き戻したノートは同じファイル() {
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
    fn 他人の書き込みへの上書きは_実行せず拒否する() {
        let d = note_dir();
        let path = d.path().join("a.md").to_str().unwrap().to_string();
        let r = call("read", &serde_json::json!({ "path": path })).unwrap();
        let stamp = r["stamp"].clone();

        // もう一方の端末。長くしてあるので、ファイルシステムのタイムスタンプが
        // 粗い環境でもスタンプが変わる。
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

        // `force` は「見たうえで、それでもそうする」と人が言うための手段。
        let w = call(
            "write",
            &serde_json::json!({ "path": path, "text": "こちらの版", "stamp": stamp, "force": true }),
        )
        .unwrap();
        assert_eq!(w["ok"], true, "{w}");
    }

    #[test]
    fn 新しいノートは指定された場所にできる() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("まだ無い");
        let r = call(
            "new",
            &serde_json::json!({ "dir": sub.to_str().unwrap(), "title": "段取り" }),
        )
        .unwrap();
        assert_eq!(r["name"], "段取り.md");
        assert!(sub.join("段取り.md").is_file(), "including the directory");
        // 同じ日に 2 回作っても、1 つ目を上書きしない。
        let r2 = call(
            "new",
            &serde_json::json!({ "dir": sub.to_str().unwrap(), "title": "段取り" }),
        )
        .unwrap();
        assert_eq!(r2["name"], "段取り-2.md");
    }

    #[test]
    fn 色付きの語は_描く側が使える形に分かれて返る() {
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
    fn 予定のタグ行は_書いて読み戻せる() {
        // **口が繋がっていることを見る。** 判断（`caltag`）の試験は
        // あちらにある ── ここで見たいのは、デスクトップ版と iPhone が呼ぶ名前で
        // 同じ答えが返ってくることだけ。
        let memo = "保険証を忘れずに。\n\n#太郎 #次郎";
        let got = call("caltag", &serde_json::json!({ "notes": memo })).unwrap();
        assert_eq!(got["tags"][0], "太郎");
        assert_eq!(got["tags"][1], "次郎");
        assert_eq!(got["body"], "保険証を忘れずに。");

        let out = call(
            "caltagset",
            &serde_json::json!({ "notes": memo, "tags": ["花子"] }),
        )
        .unwrap();
        assert_eq!(out["notes"], "保険証を忘れずに。\n\n#花子");

        // 空で呼べば、タグ行ごと消える（人の文章はそのまま）。
        let none = call(
            "caltagset",
            &serde_json::json!({ "notes": memo, "tags": [] }),
        )
        .unwrap();
        assert_eq!(none["notes"], "保険証を忘れずに。");

        // メモが無い予定でも落ちない。
        let empty = call("caltag", &serde_json::json!({})).unwrap();
        assert!(empty["tags"].as_array().unwrap().is_empty());

        // **まとめて訊ける**（依頼 539）── 月の表の予定ぜんぶを一度で。
        let many = call(
            "caltag",
            &serde_json::json!({ "notes": [memo, "本文だけ", "", "#花子"] }),
        )
        .unwrap();
        let each = many["each"].as_array().unwrap();
        assert_eq!(each.len(), 4);
        assert_eq!(each[0]["tags"][0], "太郎");
        assert!(each[1]["tags"].as_array().unwrap().is_empty());
        assert_eq!(each[1]["body"], "本文だけ");
        assert!(each[2]["tags"].as_array().unwrap().is_empty());
        assert_eq!(each[3]["tags"][0], "花子");
    }

    #[test]
    fn お気に入りは二つ目の居場所であって_移動ではない() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("仕事")).unwrap();
        let note = root.join("仕事").join("週報.md");
        std::fs::write(&note, "---\ntitle: 週報\n---\n本文。\n").unwrap();

        // お気に入りに入れても、ノートは 仕事 フォルダから動かない。
        let text = std::fs::read_to_string(&note).unwrap();
        let out = call("star", &serde_json::json!({ "text": text, "shelf": "買い物/週次" })).unwrap();
        std::fs::write(&note, out["text"].as_str().unwrap()).unwrap();

        let all = call("notes", &serde_json::json!({ "path": root.display().to_string() })).unwrap();
        let n = &all["notes"].as_array().unwrap()[0];
        assert_eq!(n["book"], "仕事", "実体のフォルダは変わらない");
        assert_eq!(n["star"], "買い物/週次");
        // 途中のフォルダも存在する。
        let stars: Vec<String> = all["stars"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(stars, vec!["買い物".to_string(), "買い物/週次".to_string()]);

        // 空のフォルダは、設定ファイルが記録している。
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
    fn 全体の移動はコピーが先で_上書きはしない() {
        let d = tempfile::tempdir().unwrap();
        let from = d.path().join("いま");
        let to = d.path().join("あたらしい");
        std::fs::create_dir_all(from.join("仕事")).unwrap();
        std::fs::create_dir_all(from.join("attachments")).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join("a.md"), "---\ntitle: a\n---\n本文\n").unwrap();
        std::fs::write(from.join("仕事").join("b.md"), "---\ntitle: b\n---\n本文\n").unwrap();
        // 画像もついていく ── 置いていったら、画像のあるノートが全部壊れる。
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
    fn バックアップの書き戻しは_そこにあるものを踏まない() {
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
    /// デスクトップ版は長いあいだ `scope: "all"` を決め打ちで渡していて、4 つあることは
    /// エンジンしか知らなかった ── iPhone にできてデスクトップ版にできない、が起きていた。
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
    /// いいという意味ではない。3 つとも確かめる ──
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
        std::fs::write(to.join("仕事/c.md"), "いまの c\n").unwrap();     // 同じフォルダの別のファイル
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
        // 同じフォルダの別のファイルも、そのまま。
        assert_eq!(std::fs::read_to_string(to.join("仕事/c.md")).unwrap(), "いまの c\n");
        // 向こうにしか無かったものは、構造ごと入る。
        assert_eq!(std::fs::read_to_string(to.join("仕事/b.md")).unwrap(), "むかしの b\n");
    }

    /// ファイル 1 つだけの zip も、フォルダだけの zip も、同じ場所へ戻せるか。
    ///
    /// 全体の zip は `ノート/…` という一つの山の下にあるので頭を外すが、
    /// **ファイル 1 つだけの zip には共通の親フォルダが無い** ── そこで同じ剥がし方をすると
    /// ファイル名そのものが外れて、何も戻らない。
    #[test]
    fn ファイル一つだけの_zip_も戻せる() {
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

        // ラベル自体は、ノートフォルダに展開しない。
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

    /// 古いものは削除されるか。**「残す」を付けたものは残るか。**
    #[test]
    fn 五十を超えたら落ちる_ただしマークのあるものは残る() {
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
        // 1 つだけ「残す」を指定する（いちばん古いもの）。
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
        // 指定の無いものは新しい 50 件まで（＋いま追加した 1 件）。
        assert!(v.len() <= crate::history::KEEP_GENS + 2, "{} 件", v.len());
        // **「残す」が付いたものは、どれだけ古くても残る。**
        assert!(v.iter().any(|x| x["stamp"] == oldest && x["kept"] == true),
                "「残す」を付けたいちばん古いものが消えました");
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

        // **共有の目印はフォルダの中に置く** ── 設定ではなく、フォルダ自身が持つ。
        assert!(d.path().join("共有").join(crate::notebook::SHARE_MARK).is_file());
        let mark = crate::notebook::share_mark(&d.path().join("共有")).unwrap();
        assert_eq!(mark.by, "Taketan");

        let out = call("notes", &root).unwrap();
        assert_eq!(out["shares"][0]["at"], "共有");
        assert_eq!(out["shares"][0]["by"], "Taketan");
        // **タイトルではなくパスで確かめる** ── タイトルは 1 行目から決まるので、
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
        // ダウンロードされていないプレースホルダ（隠しファイル）。
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

        // 通常のノートにプレースホルダは付かない。
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
    fn フォルダは改名も削除もできるが_ノートの中に限る() {
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
    fn ノートは管理情報と本文に分かれる() {
        let text = "---\ntitle: x\ntags: [a]\n---\n\n本文。\n";
        let out = call("split", &serde_json::json!({ "text": text })).unwrap();
        assert_eq!(out["head"], "---\ntitle: x\ntags: [a]\n---\n");
        assert_eq!(out["body"], "\n本文。\n");
        // 連結すると元に戻る ── ここがずれると、書いた内容が消える。
        let back = format!("{}{}", out["head"].as_str().unwrap(), out["body"].as_str().unwrap());
        assert_eq!(back, text);

        // 前書きの無いノートは、まるごと本文。
        let plain = call("split", &serde_json::json!({ "text": "ただの本文\n" })).unwrap();
        assert_eq!(plain["head"], "");
        assert_eq!(plain["body"], "ただの本文\n");
    }

    #[test]
    fn チェックボックスは押せる形で返り_押すと行が書き換わる() {
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
