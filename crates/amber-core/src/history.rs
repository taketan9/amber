//! ノートの、前の姿。
//!
//! **自動保存のアプリに履歴が要る理由は、自動保存だから。** 打鍵の 0.9 秒後に
//! 書くので、消してしまったことに気づいたときには、もう消えたほうが保存されて
//! いる。取り消し（`stepBack`）は窓を閉じれば消えるので、日をまたぐ後悔には
//! 届かない。
//!
//! **一世代は「書いていた一区切り」。** 保存のたびに残すと、十分書けば数十世代
//! になって、五十世代が一回の執筆で埋まる。最後の打鍵から間が空いたときだけ
//! 残す ── 世代の単位が人の感覚（「昨日の夕方の姿」）と揃う。
//!
//! **置き場所は `.amber/history/`。** ノートの隣、ただのファイル ── 同期先に
//! そのまま運ばれる（Inkdrop は最新の版しか同期しないと言っている）。点で
//! 始まるので、ノートの一覧にも見張りにも出てこない。
//!
//! 消し方も数えて言い切る。**新しい五十、または三十日以内**（大きいほう）を
//! 残す。人が「残す」と印を付けたものは、数にも日数にも入れない。

use anyhow::Result;
use std::path::{Path, PathBuf};

/// 何世代残すか。**数えて言い切れるのが、ただのファイルである強み** ──
/// Inkdrop は「容量のために自動で消す」としか言えない（中身が CouchDB の
/// 版なので、消えるのは世代数ではなく圧縮の都合で決まる）。
pub const KEEP_GENS: usize = 50;
/// 何日ぶん残すか。世代数と**どちらか大きいほう**。
pub const KEEP_DAYS: u64 = 30;

/// 一つの、前の姿。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// `2026-09-06T12-30-05`。名前がそのまま順番になる。
    pub stamp: String,
    /// このノートの、置き場所から見た道。
    pub note: String,
    /// 人が「残す」と印を付けたか。
    pub kept: bool,
    pub bytes: u64,
}

/// 履歴の置き場所（`<root>/.amber/history`）。
pub fn home(root: &Path) -> PathBuf {
    root.join(".amber").join("history")
}

/// このノートの、前の姿を置く棚。
///
/// ノートの道をそのまま写した形にする ── `仕事/週報.md` なら
/// `.amber/history/仕事/週報.md/`。Finder で開いても、どのノートのものか
/// 名前で分かる（一つの平らなフォルダに符号で並べると、人には読めない）。
pub fn shelf(root: &Path, note: &Path) -> Option<PathBuf> {
    let rel = note.strip_prefix(root).ok()?;
    Some(home(root).join(rel))
}

/// いまの姿を一つ残す。**同じ中身なら残さない。**
///
/// `kept` は人が付ける「消すな」の印。`force` が偽のときは、前の世代から
/// `gap` 秒たっていなければ残さない ── 打つたびに残すと、一回の執筆で
/// 五十世代が埋まる。
pub fn keep(
    root: &Path,
    note: &Path,
    text: &str,
    gap: u64,
    force: bool,
    kept: bool,
) -> Result<Option<String>> {
    let Some(shelf) = shelf(root, note) else {
        anyhow::bail!("ノートの置き場所の外です");
    };
    let mine = list_shelf(&shelf);
    if let Some(last) = mine.last() {
        // 同じ中身を二度置かない ── 開いて閉じただけで一世代増えると、
        // 履歴が「触った回数」の記録になる。
        let at = shelf.join(last);
        if std::fs::read_to_string(&at).map(|s| s == text).unwrap_or(false) {
            return Ok(None);
        }
        if !force && !kept {
            let age = age_secs(&at).unwrap_or(u64::MAX);
            if age < gap {
                return Ok(None);
            }
        }
    }
    std::fs::create_dir_all(&shelf)?;
    // **同じ秒に二つ置かれることがある。** 自動の一世代の直後に人が
    // 「残す」を押すと、名前がぶつかって前のほうが黙って消える（テストで
    // そうなった）。ぶつかったら `.2` `.3` と付ける ── 並べ替えは名前の
    // ままで正しい（`…05` → `…05.2` → `…06`）。
    let base = now_stamp();
    let mut stamp = base.clone();
    for n in 2..100 {
        if !shelf.join(format!("{stamp}.md")).exists()
            && !shelf.join(format!("{stamp}.keep.md")).exists()
        {
            break;
        }
        stamp = format!("{base}.{n}");
    }
    let name = if kept {
        format!("{stamp}.keep.md")
    } else {
        format!("{stamp}.md")
    };
    std::fs::write(shelf.join(&name), text)?;
    sweep(&shelf);
    Ok(Some(stamp))
}

/// 前の姿を、新しい順に。`at` はノート一つでもフォルダでもよい。
pub fn list(root: &Path, at: &Path) -> Result<Vec<Version>> {
    let Some(shelf) = shelf(root, at) else {
        anyhow::bail!("ノートの置き場所の外です");
    };
    let mut out = Vec::new();
    // ノート一つの棚（中に `<stamp>.md` が並ぶ）か、フォルダ（中に棚が
    // 並ぶ）か ── どちらも同じ歩き方で集まる。
    if shelf.is_dir() {
        walk(&shelf, &home(root), &mut out, 0);
    }
    // 新しい順。名前がそのまま時刻なので、名前で並べれば時刻順。
    out.sort_by(|a, b| b.stamp.cmp(&a.stamp));
    Ok(out)
}

fn walk(dir: &Path, home: &Path, out: &mut Vec<Version>, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut here: Vec<PathBuf> = Vec::new();
    for e in rd.flatten() {
        let at = e.path();
        if at.is_dir() {
            walk(&at, home, out, depth + 1);
        } else {
            here.push(at);
        }
    }
    for at in here {
        let Some(name) = at.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(stamp) = stamp_of(name) else { continue };
        // 棚の名前が、そのノートの道。
        let note = dir
            .strip_prefix(home)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        out.push(Version {
            stamp,
            note,
            kept: name.ends_with(".keep.md"),
            bytes: std::fs::metadata(&at).map(|m| m.len()).unwrap_or(0),
        });
    }
}

/// 一つの姿の中身。
pub fn read(root: &Path, note: &Path, stamp: &str) -> Result<String> {
    let Some(shelf) = shelf(root, note) else {
        anyhow::bail!("ノートの置き場所の外です");
    };
    for name in [format!("{stamp}.md"), format!("{stamp}.keep.md")] {
        let at = shelf.join(&name);
        if at.exists() {
            return Ok(std::fs::read_to_string(at)?);
        }
    }
    anyhow::bail!("その姿はもうありません（{stamp}）")
}

/// 「残す」の印を付ける／外す。
pub fn mark(root: &Path, note: &Path, stamp: &str, kept: bool) -> Result<()> {
    let Some(shelf) = shelf(root, note) else {
        anyhow::bail!("ノートの置き場所の外です");
    };
    let plain = shelf.join(format!("{stamp}.md"));
    let held = shelf.join(format!("{stamp}.keep.md"));
    match (kept, plain.exists(), held.exists()) {
        (true, true, _) => std::fs::rename(&plain, &held)?,
        (false, _, true) => std::fs::rename(&held, &plain)?,
        _ => {}
    }
    if !kept {
        sweep(&shelf);
    }
    Ok(())
}

/// 古いものを落とす。**印の付いたものは、数にも日数にも入れない。**
fn sweep(shelf: &Path) {
    let all = list_shelf(shelf);
    let mut n = 0usize;
    for name in all.iter().rev() {
        if name.ends_with(".keep.md") {
            continue;
        }
        n += 1;
        let at = shelf.join(name);
        let young = age_secs(&at).map(|s| s < KEEP_DAYS * 86_400).unwrap_or(true);
        // 新しい五十、**または**三十日以内。どちらかを満たせば残る。
        if n <= KEEP_GENS || young {
            continue;
        }
        let _ = std::fs::remove_file(&at);
    }
}

/// 棚の中の姿を、古い順の名前で。
///
/// **並べるのは刻であって、ファイル名ではない。** 名前で並べると
/// `…01.2.md` が `…01.md` より前に来る（`.` の次が `2` と `m` の比較に
/// なるので）── いちばん新しい姿が真ん中に埋もれ、「同じ中身か」の判定が
/// 一つ前を見にいく。
fn list_shelf(shelf: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(shelf) else { return Vec::new() };
    let mut out: Vec<(String, String)> = rd
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| stamp_of(&n).map(|s| (s, n)))
        .collect();
    out.sort();
    out.into_iter().map(|(_, n)| n).collect()
}

/// `2026-09-06T12-30-05.md` / `….keep.md` / `…05.2.md` → 刻。
/// それ以外は `None`（人が置いた別のファイルを姿と数えない）。
fn stamp_of(name: &str) -> Option<String> {
    let base = name
        .strip_suffix(".keep.md")
        .or_else(|| name.strip_suffix(".md"))?;
    let b = base.as_bytes();
    // `YYYY-MM-DDTHH-MM-SS`、うしろに `.2` が付くことがある。
    let shape = b.len() >= 19 && b[4] == b'-' && b[7] == b'-' && b[10] == b'T';
    let tail = b.len() == 19 || (b.len() > 20 && b[19] == b'.' && b[20..].iter().all(u8::is_ascii_digit));
    (shape && tail).then(|| base.to_string())
}

fn age_secs(at: &Path) -> Option<u64> {
    let m = std::fs::metadata(at).ok()?;
    let t = m.modified().ok()?;
    std::time::SystemTime::now().duration_since(t).ok().map(|d| d.as_secs())
}

/// いまの刻を、名前にできる形で。
///
/// **その機械の時計で。** 履歴を読むのは書いた人なので、「昨日の夕方」が
/// 昨日の夕方に見えないと意味がない ── UTC で並べると、夜に書いたものが
/// 翌日として並ぶ。
fn now_stamp() -> String {
    crate::note::now_local().format("%Y-%m-%dT%H-%M-%S").to_string()
}

/// 刻を、人が読む形に（`2026-09-06 12:30`）。
pub fn spoken(stamp: &str) -> String {
    if stamp.len() < 19 {
        return stamp.to_string();
    }
    format!("{} {}:{}", &stamp[..10], &stamp[11..13], &stamp[14..16])
}
