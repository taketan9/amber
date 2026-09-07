//! What a folder of notes knows about itself.
//!
//! Two things do not fit inside a note: what colour a **folder** is (there is
//! no note to write it on), and the favourite folders that are **empty** (a
//! folder that exists only in the notes that name it disappears the moment
//! the last one leaves). Both go in one small JSON file beside the notes, at
//! `.cian/settings.json`.
//!
//! **Everything else stays in the notes.** Whether a note is a favourite is
//! written on the note, so losing this file loses only the colours and the
//! empty folders — never a note's own place. That is the test for whether
//! something belongs here: if losing it would lose something you wrote, it
//! does not.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Book {
    /// Folder path (relative to the root) → the colour it was given.
    ///
    /// Free-form, because it is his to choose: the app suggests a palette and
    /// does not enforce one.
    pub colors: BTreeMap<String, String>,
    /// Favourite folders, including the ones nothing is in yet.
    pub stars: Vec<String>,
}

/// 共有の棚の印。**設定ではなく、フォルダ自身が持つ。**
///
/// 初めは `.amber/settings.json` に「どれが共有か」を書いていた。動きはした
/// が、**相手の amber には何も伝わらない** ── 受け取った人が自分で「これが
/// 共有です」と教え直す手が要り、機種を替えるたびにもう一度要った。
///
/// フォルダの中に一枚置けば、**読むだけで分かる**。教える手が、どちらの側
/// からも消える。しかも**フォルダと一緒に旅をする**ので、置き場所を変えても
/// 機種を替えてもずれない ── amber がもともと持っていた考え方
/// （「隠しデータベースを持たない。全部フォルダの中のファイル」）そのもの。
pub const SHARE_MARK: &str = ".amber-share.json";

/// 印に書いてあること。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shelf {
    /// 誰が分けはじめたか。空のこともある（名乗らなかった人）。
    pub by: String,
    /// いつから。`YYYY-MM-DD`。
    pub since: String,
}

/// このフォルダは共有の棚か。印を読む。
pub fn share_mark(dir: &Path) -> Option<Shelf> {
    let text = std::fs::read_to_string(dir.join(SHARE_MARK)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(Shelf {
        by: v.get("by").and_then(|s| s.as_str()).unwrap_or("").to_string(),
        since: v.get("since").and_then(|s| s.as_str()).unwrap_or("").to_string(),
    })
}

/// 印を置く。**もうあれば触らない** ── 相手が置いた印の「誰が」を、
/// こちらの名前で上書きしない（分けはじめたのはあちらなので）。
pub fn mark_share(dir: &Path, by: &str, today: &str) -> anyhow::Result<()> {
    if share_mark(dir).is_some() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let v = serde_json::json!({ "by": by, "since": today });
    std::fs::write(dir.join(SHARE_MARK), serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

/// 印を外す。**中のノートには触らない。**
pub fn unmark_share(dir: &Path) -> anyhow::Result<()> {
    let at = dir.join(SHARE_MARK);
    if at.exists() {
        std::fs::remove_file(at)?;
    }
    Ok(())
}

/// フォルダに付けられる十一色。
///
/// **ここが唯一の並び。** 窓の `PALETTE` と電話の `Colouring.palette` に
/// 同じものを書いていて、「同じ並び」と両方のコメントに書いてあった ──
/// それでも**十一色のうち六色がずれていた**。電話で付けた青が、Mac では
/// 少し違う青で出ていた。写しを持てば、いつかずれる。
///
/// 色でしか区別できないノートは grep に映らず、読み上げにも伝わらないので、
/// 増やさない。名前はカタカナで揃える ── 「みどり青」と「青むらさき」は、
/// 二つ並べたときにどちらがどちらか言えない。
pub const PALETTE: [(&str, &str); 11] = [
    ("#0E93A8", "シアン"),
    ("#2AA79B", "ターコイズ"),
    ("#3D7FA8", "ブルー"),
    ("#6E7BC4", "バイオレット"),
    ("#9A6FB5", "パープル"),
    ("#C2649A", "ベルガモット"),
    ("#C4564E", "カーマイン"),
    ("#D07A2E", "アンバー"),
    ("#B08A2E", "マスタード"),
    // 前は `#5E8C42`（オリーブ寄り）で、緑というよりくすんだ黄土に見えた。
    ("#3FA05C", "グリーン"),
    ("#7A7A7A", "グレー"),
];

pub fn file(root: &Path) -> PathBuf {
    root.join(".cian").join("settings.json")
}

/// What the folder says about itself, or the defaults.
///
/// **Never an error.** A missing file is a folder that has not been given a
/// colour yet, and a corrupt one is not a reason to refuse to show the notes
/// — the notes are the thing, and this is decoration and bookkeeping.
pub fn read(root: &Path) -> Book {
    let Ok(text) = std::fs::read_to_string(file(root)) else { return Book::default() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Book::default() };
    let mut b = Book::default();
    if let Some(m) = v.get("colors").and_then(|c| c.as_object()) {
        for (k, val) in m {
            if let Some(s) = val.as_str() {
                b.colors.insert(k.clone(), s.to_string());
            }
        }
    }
    if let Some(a) = v.get("stars").and_then(|s| s.as_array()) {
        b.stars = a.iter().filter_map(|s| s.as_str()).map(str::to_string).collect();
    }
    b
}

/// Write it back, making `.cian` if it is not there.
pub fn write(root: &Path, b: &Book) -> anyhow::Result<()> {
    let at = file(root);
    if let Some(dir) = at.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let v = serde_json::json!({ "colors": b.colors, "stars": b.stars });
    std::fs::write(at, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

/// 歩いた結果から、印のあるフォルダを拾う（ルートからの道で）。
///
/// **教えてもらわなくても分かる。** これが `settings.json` に書いていた頃と
/// のいちばんの違いで、受け取った人の手が一つ消える。
pub fn shares(root: &Path, rows: &[crate::survey::Row]) -> Vec<String> {
    let mut out = Vec::new();
    if share_mark(root).is_some() {
        // ルートそのものが共有、はありうる（フォルダを丸ごと分けた人）。
        out.push(String::new());
    }
    for r in rows {
        if !r.is_dir || r.rel.split('/').any(|p| p.starts_with('.')) {
            continue;
        }
        if share_mark(&r.path).is_some() {
            out.push(r.rel.clone());
        }
    }
    out.sort();
    out
}

/// この道は、分けてあるフォルダの中か。
///
/// フォルダそのものと、その下ぜんぶ。**一つも無ければ何も分けていない** ──
/// ここで空を「全部が共有」と読むと、決めていない人のノートが全部共有の顔を
/// する。
pub fn shared(shares: &[String], book: &str) -> bool {
    shares.iter().any(|s| {
        if s.is_empty() {
            return true;
        }
        book == s || book.starts_with(&format!("{s}/"))
    })
}

/// Give a folder a colour, or take it away.
pub fn set_color(root: &Path, folder: &str, color: Option<&str>) -> anyhow::Result<()> {
    let mut b = read(root);
    match color {
        Some(c) => b.colors.insert(folder.to_string(), c.to_string()),
        None => b.colors.remove(folder),
    };
    write(root, &b)
}

/// Remember a favourite folder, so that an empty one still exists tomorrow.
pub fn add_star(root: &Path, folder: &str) -> anyhow::Result<()> {
    let mut b = read(root);
    if folder.is_empty() || b.stars.iter().any(|s| s == folder) {
        return Ok(());
    }
    b.stars.push(folder.to_string());
    b.stars.sort();
    write(root, &b)
}

/// Forget one, and everything under it.
pub fn drop_star(root: &Path, folder: &str) -> anyhow::Result<()> {
    let mut b = read(root);
    let under = format!("{folder}/");
    b.stars.retain(|s| s != folder && !s.starts_with(&under));
    write(root, &b)
}

/// Everything under `from`, moved to `to`.
///
/// **Copy, check, then remove — in that order.** Between two cloud providers
/// this is not a rename: the bytes have to be written on the far side before
/// anything is taken off this one, and if any of it fails the originals are
/// still there. A note lost in the middle of moving is the worst thing a
/// notes app can do.
///
/// Nothing is overwritten. A name already taken on the far side stops the
/// whole move — merging two folders of notes is a decision, and this is not
/// the moment to make it for somebody.
///
/// Everything, not only the notes: the pictures live in `attachments/` beside
/// them and `.cian` holds the colours and the empty shelves. A move that took
/// the notes and left the pictures would break every note with a picture.
pub fn migrate(from: &Path, to: &Path) -> anyhow::Result<usize> {
    let from = from.canonicalize()?;
    let to = to.canonicalize()?;
    if from == to {
        return Ok(0);
    }
    if to.starts_with(&from) {
        anyhow::bail!("移す先が、いまの場所の中にあります");
    }

    let mut jobs: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut walk = vec![from.clone()];
    while let Some(dir) = walk.pop() {
        for e in std::fs::read_dir(&dir)? {
            let at = e?.path();
            let Ok(rel) = at.strip_prefix(&from) else { continue };
            let dest = to.join(rel);
            if at.is_dir() {
                walk.push(at);
                std::fs::create_dir_all(&dest)?;
            } else {
                if dest.exists() {
                    anyhow::bail!("移す先に同じ名前があります: {}", rel.display());
                }
                jobs.push((at, dest));
            }
        }
    }
    for (src, dest) in &jobs {
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::copy(src, dest)?;
    }
    // Only now. Every byte is on the far side.
    for (src, _) in &jobs {
        let _ = std::fs::remove_file(src);
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut walk = vec![from.clone()];
    while let Some(dir) = walk.pop() {
        for e in std::fs::read_dir(&dir)? {
            let at = e?.path();
            if at.is_dir() {
                walk.push(at.clone());
                dirs.push(at);
            }
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        let _ = std::fs::remove_dir(d);
    }
    Ok(jobs.len())
}

/// 取り込んだ結果 ── 入れた数・名前を変えた数・入らなかった数。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Brought {
    pub put: usize,
    pub renamed: usize,
    pub failed: usize,
}

/// よそから持ってきた .md を、ノート帳へ。
///
/// **同じ名前は、上書きではなく両方残す。** 戻す（`restore`）ほうは同じ
/// 名前を「いまのを残す」で飛ばすが、持ってくるのは逆 ── 戻すのは前に
/// あったものを取り返す行いで、持ってくるのは**知らないものを入れる**
/// 行いなので、飛ばすと「入れたはずのものが入っていない」になる。
/// `週報.md` が既にあれば `週報-2.md` にする。
///
/// **`週報 2.md`（間が空白）にはしない。** クラウドが作る衝突の控えが
/// その形で、`cloud::shape` は当てずっぽうで札を貼らないためにその名前を
/// 拾わないと決めてある ── 持ってきたノートを、貼られない控えと同じ顔に
/// しない（依頼 316）。
///
/// **元のファイルは動かさない。** 写すだけ ── 人が選んだのは自分の
/// フォルダにあるもので、amber がそれを引き取っていい理由は無い。
///
/// 判断（名前の付け直し）がここにあるのは、窓と電話で二組書くと必ず
/// ずれるから。写す仕事そのものは呼ぶ側にもできるが、**同じノートが
/// 端末によって別の名前で入る**のは直しようがない。
pub fn bring(files: &[std::path::PathBuf], to: &Path) -> anyhow::Result<Brought> {
    std::fs::create_dir_all(to)?;
    let mut put = 0usize;
    let mut renamed = 0usize;
    let mut failed = 0usize;
    for from in files {
        let Some(name) = from.file_name() else { continue };
        let mut dest = to.join(name);
        if dest.exists() {
            let stem = Path::new(name)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let ext = Path::new(name)
                .extension()
                .map(|s| format!(".{}", s.to_string_lossy()))
                .unwrap_or_default();
            // 99 で止める ── ここまで来たら名前を付け直すのは人の仕事で、
            // 数え続けても同じ名前のノートが百本並ぶだけ。
            let mut n = 2;
            while dest.exists() && n <= 99 {
                dest = to.join(format!("{stem}-{n}{ext}"));
                n += 1;
            }
            if dest.exists() {
                // ここまで来たら名前を付け直すのは人の仕事。飛ばして次へ。
                failed += 1;
                continue;
            }
            renamed += 1;
        }
        // **一本で転んでも、残りは運ぶ。** 十本選んだうちの三本目が読めない
        // ときに一本も入らないと、人にできることは「一本ずつ選び直す」しか
        // ない ── 入ったぶんは入ったと言い、入らなかった数を添える。
        match std::fs::copy(from, &dest) {
            Ok(_) => put += 1,
            Err(_) => failed += 1,
        }
    }
    Ok(Brought { put, renamed, failed })
}

/// A backup, put back. Returns (put in, left alone).
///
/// **Into the notes folder, never over a note.** A restore that overwrote
/// would be a restore that could lose today's work to last week's copy.
/// zip の中の札から、何の範囲のバックアップかを読む。無ければ `None`。
fn read_label(zip: &Path) -> Option<String> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(zip).ok()?).ok()?;
    let mut f = z.by_name(crate::zipbox::LABEL).ok()?;
    let mut body = String::new();
    use std::io::Read;
    f.read_to_string(&mut body).ok()?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    v["scope"].as_str().map(str::to_string)
}

pub fn restore(zip: &Path, to: &Path) -> anyhow::Result<(usize, usize)> {
    std::fs::create_dir_all(to)?;

    // Into a room of its own first, so a half-unpacked archive never stands
    // among the notes.
    //
    // **A room per call, not per process.** Named by the pid alone, two
    // restores at once unpack into the same room and each counts the other's
    // files — and the second one's `remove_dir_all` can take the first one's
    // half-unpacked archive out from under it. Found by a test that restored
    // twice in one process and was told two files went in when one had.
    // A leftover room from a run that died also stops being somebody else's
    // problem this way.
    static ROOM: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nth = ROOM.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let hold = std::env::temp_dir().join(format!(
        "amber-restore-{}-{nth}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&hold);
    std::fs::create_dir_all(&hold)?;

    let members = crate::zipbox::list(zip)?;
    // **The folder's own name comes off — but only for a whole notebook.**
    //
    // A backup of the notes folder is a zip *of that folder*, so every member
    // is `ノート/…`; put back as-is it makes a `ノート` inside the notes and
    // every note in it looks new. A backup of **one folder** has exactly the
    // same shape (`仕事/…`) and must keep its head — strip it and 週報 comes
    // back to the root instead of to 仕事, which is not "back".
    //
    // The shape cannot tell them apart, so the zip says which it is
    // (`zipbox::LABEL`, written when it was made). Archives from before that
    // label fall back to the old guess — it is right for the whole-notebook
    // case, which is the one people take most.
    let scope = read_label(zip);
    let top = members
        .iter()
        .filter(|m| m.as_str() != crate::zipbox::LABEL)
        .filter_map(|m| m.split('/').next())
        .collect::<std::collections::BTreeSet<_>>();
    let one_head = top.len() == 1 && members.iter().any(|m| m.contains('/'));
    let strip = match scope.as_deref() {
        Some("all") if one_head => top.into_iter().next().unwrap_or("").to_string(),
        Some(_) => String::new(),
        None if one_head => top.into_iter().next().unwrap_or("").to_string(),
        None => String::new(),
    };
    if let Err(e) = crate::zipbox::extract(zip, &hold, &strip) {
        let _ = std::fs::remove_dir_all(&hold);
        return Err(e);
    }

    let mut put = 0usize;
    let mut kept = 0usize;
    let mut walk = vec![hold.clone()];
    while let Some(dir) = walk.pop() {
        for e in std::fs::read_dir(&dir)? {
            let at = e?.path();
            let Ok(rel) = at.strip_prefix(&hold) else { continue };
            let dest = to.join(rel);
            if at.is_dir() {
                walk.push(at);
                std::fs::create_dir_all(&dest)?;
            } else if at.file_name().is_some_and(|n| n == crate::zipbox::LABEL) {
                // 札はノートではないので、ノート帳には置いていかない。
            } else if dest.exists() {
                kept += 1;
            } else {
                if let Some(d) = dest.parent() {
                    std::fs::create_dir_all(d)?;
                }
                std::fs::copy(&at, &dest)?;
                put += 1;
            }
        }
    }
    let _ = std::fs::remove_dir_all(&hold);
    Ok((put, kept))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bringing_notes_in_keeps_both_when_the_name_is_taken() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("ノート");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("週報.md"), "いま書いているほう").unwrap();

        let away = tempfile::tempdir().unwrap();
        let one = away.path().join("週報.md");
        let two = away.path().join("献立.md");
        std::fs::write(&one, "よそから来たほう").unwrap();
        std::fs::write(&two, "カレー").unwrap();

        let r = bring(&[one.clone(), two], &root).unwrap();
        assert_eq!(r, Brought { put: 2, renamed: 1, failed: 0 });

        // **いま書いているほうは動かさない。**
        assert_eq!(std::fs::read_to_string(root.join("週報.md")).unwrap(), "いま書いているほう");
        assert_eq!(std::fs::read_to_string(root.join("週報-2.md")).unwrap(), "よそから来たほう");
        assert!(root.join("献立.md").exists());

        // 元のファイルはそのまま ── 写すだけ。
        assert!(one.exists());

        // 二度持ってくれば、三本目になる（飛ばさない）。
        let r = bring(&[one], &root).unwrap();
        assert_eq!(r, Brought { put: 1, renamed: 1, failed: 0 });
        assert!(root.join("週報-3.md").exists());

        // 空白を挟む名前にはしない ── クラウドの控えと同じ顔にならないように。
        assert!(!root.join("週報 2.md").exists());

        // 一本読めなくても、残りは運ぶ。
        let gone = away.path().join("ない.md");
        let ok = away.path().join("味噌汁.md");
        std::fs::write(&ok, "だし").unwrap();
        let r = bring(&[gone, ok], &root).unwrap();
        assert_eq!(r, Brought { put: 1, renamed: 0, failed: 1 });
        assert!(root.join("味噌汁.md").exists());
    }

    #[test]
    fn a_folder_remembers_its_colour_and_its_empty_favourites() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(read(root), Book::default());

        set_color(root, "仕事", Some("#D9822B")).unwrap();
        add_star(root, "買い物").unwrap();
        add_star(root, "買い物/週次").unwrap();
        add_star(root, "買い物").unwrap(); // 二度足しても増えない

        let b = read(root);
        assert_eq!(b.colors.get("仕事").map(String::as_str), Some("#D9822B"));
        assert_eq!(b.stars, vec!["買い物".to_string(), "買い物/週次".to_string()]);

        // 親を消すと下も消える ── 残った子は、辿り着けない場所になる。
        drop_star(root, "買い物").unwrap();
        assert!(read(root).stars.is_empty());

        set_color(root, "仕事", None).unwrap();
        assert!(read(root).colors.is_empty());
    }

    #[test]
    fn a_broken_settings_file_is_not_a_reason_to_lose_the_notes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".cian")).unwrap();
        std::fs::write(file(dir.path()), "{ これは JSON ではない").unwrap();
        assert_eq!(read(dir.path()), Book::default());
    }
}
