//! クラウドが、ノートの隣に置いていくもの。
//!
//! amber は同期の仕組みを一つも知らない ── ノートはただのフォルダに置いた
//! ただの Markdown で、運ぶのは iCloud なり Dropbox なりの仕事。それでいい。
//! **ただし、あちらが置いていくものが二種類ある。**
//!
//! 一つは**まだ落ちてきていないファイル**。iCloud も OneDrive も「使うときに
//! 落とす」が既定で、iCloud はそのとき `買い物リスト.md` を消して
//! `.買い物リスト.md.icloud` という**札**を置く。名前が違うので amber の
//! 一覧には出ない ── つまり**ノートが消えたようにしか見えない**。落ちてくる
//! まで待てばいいだけなのに、それを言う口がどこにも無かった。
//!
//! もう一つは**衝突の控え**。二台で同時に書くと、amber 自身の衝突検査より
//! 先にクラウドが `買い物リスト (Taketan の競合コピー 2026-09-06).md` を作る。
//! これは amber からは**ただの新しいノート**に見え、そのうち二本のリストが
//! 並んでどちらが本物か分からなくなる。
//!
//! # 当てずっぽうで札を貼らない
//!
//! **`買い物リスト 2.md` は拾わない。** iCloud も Finder もその名前で控えを
//! 作るが、「週報 2」という**本物のノート**と字の上で見分けが付かない。
//! 本物のノートに「これは衝突です」と貼るほうが、控えを見逃すより悪い ──
//! 見逃したものはただのノートとして読めるが、貼られたほうは消していいものに
//! 見える。**確かに見分けられる形だけ**を拾う。

/// クラウドが置いていったものの、種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// まだ落ちてきていない ── 置いてあるのは中身ではなく札。
    Waiting,
    /// 同時に書いたので、クラウドが作った控え。
    Clash,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Waiting => "waiting",
            Kind::Clash => "clash",
        }
    }
}

/// 見分けた一つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spot {
    pub kind: Kind,
    /// もとのノートのファイル名（`買い物リスト.md`）。
    pub of: String,
    /// 誰の控えか。分からなければ空。
    pub by: String,
}

/// Markdown のノートの名前か。
fn is_note(name: &str) -> bool {
    let low = name.to_lowercase();
    low.ends_with(".md") || low.ends_with(".markdown")
}

/// この名前は、クラウドが置いていったものか。
///
/// 渡すのはファイル名だけ（道ではなく）。判断に道は要らないし、道を見ると
/// フォルダの名前に釣られる。
pub fn shape(name: &str) -> Option<Spot> {
    // ── まだ落ちてきていない ──────────────────────────────
    //
    // `.買い物リスト.md.icloud`。頭の `.` は「隠す」ためのもので、名前の
    // 一部ではない。
    if let Some(rest) = name.strip_prefix('.') {
        if let Some(of) = rest.strip_suffix(".icloud") {
            if is_note(of) {
                return Some(Spot { kind: Kind::Waiting, of: of.to_string(), by: String::new() });
            }
        }
    }
    if !is_note(name) {
        return None;
    }
    // ── 衝突の控え ────────────────────────────────────────
    //
    // Syncthing: `買い物リスト.sync-conflict-20260906-210400-ABCDEFG.md`
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s, e),
        None => return None,
    };
    if let Some((of, _tail)) = stem.split_once(".sync-conflict-") {
        return Some(Spot {
            kind: Kind::Clash,
            of: format!("{of}.{ext}"),
            by: String::new(),
        });
    }
    // Dropbox: `買い物リスト (Taketan's conflicted copy 2026-09-06).md`
    //          `買い物リスト (Taketan の競合コピー 2026-09-06).md`
    //          `買い物リスト (conflicted copy).md`
    //
    // **括弧の中に合言葉があるときだけ。** 「(下書き)」で終わるノートを
    // 控えにしてしまわないように、中の言葉を確かめる。
    if stem.ends_with(')') {
        if let Some(at) = stem.rfind(" (") {
            let inside = &stem[at + 2..stem.len() - 1];
            let low = inside.to_lowercase();
            let mark = if low.contains("conflicted copy") {
                Some("'s conflicted copy")
            } else if inside.contains("競合コピー") {
                Some(" の競合コピー")
            } else {
                None
            };
            if let Some(mark) = mark {
                let of = stem[..at].to_string();
                let by = inside.split(mark).next().unwrap_or("").trim().to_string();
                // `(conflicted copy)` だけのときは、誰のものか書いていない。
                let by = if by.eq_ignore_ascii_case("conflicted copy") { String::new() } else { by };
                return Some(Spot { kind: Kind::Clash, of: format!("{of}.{ext}"), by });
            }
        }
    }
    None
}

/// まだ落ちてきていないノートを拾う。
///
/// **歩き直さない。** ノートを数える歩き（`note::list`）は隠しファイルを
/// 見ないので札が一つも出てこないが、**そのときに見つけたフォルダは分かって
/// いる** ── その一段ずつを覗くだけで足りる。木をもう一度下りる必要は無い。
///
/// 衝突の控えはこちらでは拾わない ── あちらは隠れておらず、ノートとして
/// 一覧に出ている。**一覧から消さずに札を貼る**のが正しい（消すと、中身を
/// 助け出す道がどこにも無くなる）。
/// 返すのは**本来のノートの道**（札の道ではなく）── 電話はこれを iOS に
/// 渡して「落としてきて」と頼む（`startDownloadingUbiquitousItem`）。札の道を
/// 渡しても、あちらは何のことか分からない。
pub fn waiting(root: &std::path::Path, rows: &[crate::survey::Row]) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut look = |dir: &std::path::Path| {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(s) = shape(&name) {
                if s.kind == Kind::Waiting {
                    out.push(dir.join(&s.of));
                }
            }
        }
    };
    look(root);
    for r in rows {
        // 自分の持ちもの（`.amber` の中の履歴）は覗かない ── あそこに
        // クラウドの札が出ることはあるが、それはノートの話ではない。
        if r.is_dir && !r.rel.split('/').any(|p| p.starts_with('.')) {
            look(&r.path);
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn まだ落ちてきていない札を見分ける() {
        let s = shape(".買い物リスト.md.icloud").unwrap();
        assert_eq!(s.kind, Kind::Waiting);
        // **もとの名前を返す。** 「.買い物リスト.md.icloud が無い」と言われても
        // 誰も分からない ── 人が探しているのは「買い物リスト」。
        assert_eq!(s.of, "買い物リスト.md");

        assert_eq!(shape(".週報.markdown.icloud").unwrap().of, "週報.markdown");
        // ノートでないものの札は、ノートの話ではない。
        assert!(shape(".写真.png.icloud").is_none());
        // 頭に `.` の付かない `.icloud` は、そういう名前のファイル。
        assert!(shape("買い物リスト.md.icloud").is_none());
    }

    #[test]
    fn 衝突の控えを見分ける() {
        let s = shape("買い物リスト (Taketan's conflicted copy 2026-09-06).md").unwrap();
        assert_eq!(s.kind, Kind::Clash);
        assert_eq!(s.of, "買い物リスト.md");
        assert_eq!(s.by, "Taketan");

        // Dropbox は土地の言葉で書く。
        let j = shape("買い物リスト (Taketan の競合コピー 2026-09-06).md").unwrap();
        assert_eq!(j.of, "買い物リスト.md");
        assert_eq!(j.by, "Taketan");

        // 誰のものか書いていないこともある。
        let n = shape("買い物リスト (conflicted copy).md").unwrap();
        assert_eq!(n.of, "買い物リスト.md");
        assert_eq!(n.by, "");

        // Syncthing。
        let y = shape("買い物リスト.sync-conflict-20260906-210400-ABCDEFG.md").unwrap();
        assert_eq!(y.kind, Kind::Clash);
        assert_eq!(y.of, "買い物リスト.md");
    }

    #[test]
    fn 落ちてきていない札を_歩き直さずに拾う() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("仕事");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(d.path().join(".amber").join("history")).unwrap();
        std::fs::write(d.path().join(".買い物リスト.md.icloud"), "").unwrap();
        std::fs::write(sub.join(".週報.md.icloud"), "").unwrap();
        std::fs::write(d.path().join("ふつう.md"), "本文\n").unwrap();
        // `.amber` の中の札は、ノートの話ではない。
        std::fs::write(d.path().join(".amber").join(".なにか.md.icloud"), "").unwrap();

        let stop = std::sync::atomic::AtomicBool::new(false);
        let walk = crate::survey::survey(d.path(), crate::survey::Limits::default(), &stop);
        let out = waiting(d.path(), &walk.rows);
        let mut names: Vec<&str> = out
            .iter()
            .map(|p| p.file_name().and_then(|f| f.to_str()).unwrap_or(""))
            .collect();
        // **並びは確かめない。** 見せる順は画面が決めるし、道の順で並べて
        // いるので木の形で変わる ── ここで欲しいのは、二つとも見つかること、
        // 二度言わないこと、`.amber` の中を覗かないこと。
        names.sort();
        assert_eq!(names, vec!["買い物リスト.md", "週報.md"]);

        // 返すのは**本来の道**（札の道ではない）── 電話はこれを渡して
        // 「落としてきて」と頼む。
        assert!(out.iter().all(|p| !p.exists() && !p.to_string_lossy().contains(".icloud")));
        assert!(out.iter().any(|p| p.ends_with("仕事/週報.md")));
    }

    #[test]
    fn 当てずっぽうで本物のノートに札を貼らない() {
        // **これがこの module でいちばん大事な試験。**
        // 見逃した控えはただのノートとして読めるが、貼られた本物は
        // 「消していいもの」に見える。
        assert!(shape("週報 2.md").is_none(), "iCloud の控えと同じ形だが、本物かもしれない");
        assert!(shape("買い物リスト (下書き).md").is_none(), "括弧で終わるだけのノート");
        assert!(shape("会議メモ (2026-09-06).md").is_none(), "日付を括弧に入れる人はいる");
        assert!(shape("copy of 週報.md").is_none());
        assert!(shape("買い物リスト.md").is_none());
        assert!(shape("読書メモ.txt").is_none(), "そもそもノートではない");
    }
}
