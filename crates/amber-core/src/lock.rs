//! うっかり書き換えないための錠（依頼 629）。
//!
//! **止めるのはここ。** 窓も電話も `api::call` に訊くので、錠をかけた判断を
//! 前端に置くと、片方の画面からは書けてしまう ── 錠は「見た目が触れない」
//! ことではなく、「書こうとしても書けない」ことで守る。
//!
//! 憶え方は本人が決めた（2026-09-18）:
//!
//! * **ノートは前書きに `locked: true`** ── ファイルと一緒に動くので、同期
//!   しても iPhone でも会社の PC でも同じように錠がかかる。メモ帳で開いた
//!   人にも見える（隠しDBを持たない、という amber の決まりのまま）
//! * **フォルダは目印のファイル**（`.amberlock`）── そのフォルダの下は
//!   ぜんぶ錠。あとから増えたノートにも効く
//!
//! 外し方も本人が決めた ── **二つある**:
//!
//! 1. **今だけ編集する** ── 前端が `unlock: true` を添えて書く。ノートの
//!    中身は変わらないので、閉じればまた錠がかかる
//! 2. **錠をやめる** ── 前書きの `locked` を外す（フォルダなら目印を消す）

use std::path::{Path, PathBuf};

/// フォルダの目印。**点で始める** ── 一覧にもノートの数にも出さない
/// （`.` で始まるものは、amber がどこでも数えない）。
pub const MARK: &str = ".amberlock";

/// なぜ錠がかかっているか。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// そのノート自身の前書き（`locked: true`）。
    Note,
    /// このフォルダに目印がある（そのフォルダの道）。
    Folder(PathBuf),
}

/// 錠がかかっているか。**ノート自身と、その上のフォルダぜんぶを見る。**
///
/// 上へは根まで歩く ── 目印は置いた所にしか無いので、深いノートでも
/// 見るのは数段。まだ無いファイル（これから作るノート）も、置く先の
/// フォルダが錠なら錠。
pub fn of(path: &Path) -> Option<Why> {
    if note_locked(path) {
        return Some(Why::Note);
    }
    let mut at = path.parent();
    while let Some(dir) = at {
        if dir.join(MARK).exists() {
            return Some(Why::Folder(dir.to_path_buf()));
        }
        at = dir.parent();
    }
    None
}

/// 錠なら断る。**`unlock` が真なら通す** ──「今だけ編集する」を押した人。
pub fn keep_out(path: &Path, unlock: bool) -> anyhow::Result<()> {
    if unlock {
        return Ok(());
    }
    match of(path) {
        None => Ok(()),
        Some(Why::Note) => anyhow::bail!("このノートはロックされています"),
        Some(Why::Folder(dir)) => {
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            anyhow::bail!("「{name}」はロックされているフォルダです")
        }
    }
}

/// ノート自身の前書き。**`true` と書いてあるときだけ** ── `locked: false`
/// と書いた人は、外したつもりの人。
pub fn note_locked(path: &Path) -> bool {
    // 前書きは頭にしか無いので、頭だけ読む（何万行のノートを開かない）。
    let Some(lines) = crate::note::head(path, 32) else { return false };
    yes(crate::note::front(&lines).get("locked"))
}

fn yes(v: Option<&str>) -> bool {
    matches!(
        v.map(|s| s.trim().trim_matches(['"', '\'']).to_ascii_lowercase()).as_deref(),
        Some("true" | "yes" | "on" | "1" | "はい")
    )
}

/// ノートに錠をかける／やめる（前書きの `locked`）。
pub fn set_note(path: &Path, on: bool) -> anyhow::Result<()> {
    let f = crate::text::read(path)?;
    let text = f.lines.join("\n");
    let text = crate::note::set_field(&text, "locked", on.then_some("true"));
    let mut out = f;
    out.lines = text.split('\n').map(|l| l.to_string()).collect();
    crate::text::write(path, &out)?;
    Ok(())
}

/// フォルダに錠をかける／やめる（目印のファイル）。
pub fn set_dir(dir: &Path, on: bool) -> anyhow::Result<()> {
    if !dir.is_dir() {
        anyhow::bail!("{} がありません", dir.display());
    }
    let at = dir.join(MARK);
    if on {
        // 中身は人が読むための一行。**中身は見ない**（あることが錠）。
        std::fs::write(&at, "このフォルダは ambər でロックされています。\n")?;
    } else if at.exists() {
        std::fs::remove_file(&at)?;
    }
    Ok(())
}

/// 画面に見せる形。`locked` と、なぜか。
pub fn tell(path: &Path) -> serde_json::Value {
    match of(path) {
        None => serde_json::json!({ "locked": false }),
        Some(Why::Note) => serde_json::json!({ "locked": true, "why": "note" }),
        Some(Why::Folder(dir)) => serde_json::json!({
            "locked": true, "why": "folder", "dir": dir.to_string_lossy(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(at: &Path, text: &str) {
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }

    #[test]
    fn 前書きに書いてあれば錠() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("守る.md");
        put(&a, "---\ntitle: 守る\nlocked: true\n---\n\n本文\n");
        let b = t.path().join("ふつう.md");
        put(&b, "---\ntitle: ふつう\n---\n\n本文\n");
        assert_eq!(of(&a), Some(Why::Note));
        assert_eq!(of(&b), None);
        assert!(keep_out(&a, false).is_err());
        // 「今だけ編集する」を押した人は通る。
        assert!(keep_out(&a, true).is_ok());
        assert!(keep_out(&b, false).is_ok());
    }

    #[test]
    fn 外したつもりの人を錠にしない() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("a.md");
        put(&a, "---\nlocked: false\n---\n\n本文\n");
        assert_eq!(of(&a), None);
        // 書き方の揺れは飲む（`\"true\"` も `はい` も）。
        put(&a, "---\nlocked: \"true\"\n---\n\n本文\n");
        assert_eq!(of(&a), Some(Why::Note));
    }

    #[test]
    fn フォルダの目印は下ぜんぶに効く() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("仕事");
        let deep = dir.join("2026").join("議事録.md");
        put(&deep, "---\ntitle: 議事録\n---\n\n本文\n");
        let outside = t.path().join("よそ.md");
        put(&outside, "x\n");
        assert_eq!(of(&deep), None);
        set_dir(&dir, true).unwrap();
        // **サブフォルダの中まで**。あとから増えるノートにも効く。
        assert_eq!(of(&deep), Some(Why::Folder(dir.clone())));
        let made = dir.join("2026").join("これから.md");
        assert_eq!(of(&made), Some(Why::Folder(dir.clone())), "まだ無いノートも錠");
        assert_eq!(of(&outside), None, "外のノートまで錠にしない");
        set_dir(&dir, false).unwrap();
        assert_eq!(of(&deep), None);
    }

    #[test]
    fn 錠をかける_やめる() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("a.md");
        put(&a, "---\ntitle: a\ncreated: 2026-09-18\n---\n\n本文\n");
        set_note(&a, true).unwrap();
        let text = std::fs::read_to_string(&a).unwrap();
        assert!(text.contains("locked: true"), "{text}");
        // **本文は動かさない。**
        assert!(text.ends_with("本文\n"), "{text}");
        assert!(text.contains("title: a"), "題を落とした: {text}");
        assert_eq!(of(&a), Some(Why::Note));
        set_note(&a, false).unwrap();
        let text = std::fs::read_to_string(&a).unwrap();
        assert!(!text.contains("locked"), "{text}");
        assert_eq!(of(&a), None);
    }

    #[test]
    fn 画面に見せる形() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("仕事");
        let a = dir.join("a.md");
        put(&a, "x\n");
        assert_eq!(tell(&a)["locked"], false);
        set_dir(&dir, true).unwrap();
        assert_eq!(tell(&a)["why"], "folder");
        assert_eq!(tell(&a)["dir"], dir.to_string_lossy().to_string());
    }
}
