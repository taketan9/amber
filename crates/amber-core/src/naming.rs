//! **ファイル名は題に合わせる**（依頼 492・本人が決めた・2026-09-11）。
//!
//! ファイル名は、amber の外から見える唯一の名札 ── Finder、iPhone の
//! 「ファイル」、Drive の画面、家族が開いたとき、将来の雲。amber の中だけで
//! 題を持っていると、外から見るたびに `2026-09-06 19-18-30.md` に戻る。
//!
//! 決めごと（本人の答え）:
//!
//! 1. 改名は**題の欄から出たとき**（一字ごとにはしない）。
//! 2. 名前の元は**一覧に出ている題**（`title:` → 見出し → 一行目）。見出しや
//!    一行目で決まるノートは、打つたびには改名せず、**そのノートから離れた
//!    とき**に。題が空なら作った時刻の名前のまま。
//! 3. 使えない字は**全角に置き換える**。先頭の `.` は外す。長い題は 80 字で切る。
//! 4. 同じ題は `買い物.2.md` `買い物.3.md`。番号は**加算**（空き番を埋めない）。
//! 5. 改名に付いてくるもの: 絵（`attachments/<名前>-…`）と本文のリンク・履歴の棚・
//!    共有から戻る場所の憶え・同期の憶え。
//! 6. 同期では「削除＋新規」ではなく**名前が変わった**として運ぶ（`sync::moved`）。
//! 7. すでにある時刻名のノートは、一度だけ題の名前に揃える（`tidy_names`）。
//!
//! ここは**いつ**改名するかを決めない ── それは窓と電話の仕事（欄から出た・
//! ノートから離れた）。ここが決めるのは**何という名前にするか**と、改名に
//! 付いてくるものを一つ残らず連れて行くこと。

use std::path::{Path, PathBuf};

/// 題からファイル名の幹（拡張子なし）。**使えない字は全角に。**
///
/// `file_stem`（絵の名前に使う）は使えない字を `-` に潰すが、ここは読める
/// ように全角へ置き換える ── 「A/B」の題のノートは `A／B.md`。
pub fn title_name(title: &str) -> Option<String> {
    let mut out = String::new();
    for c in title.chars() {
        let c = match c {
            '/' => '／',
            '\\' => '＼',
            ':' => '：',
            '*' => '＊',
            '?' => '？',
            '"' => '＂',
            '<' => '＜',
            '>' => '＞',
            '|' => '｜',
            c if c.is_control() => ' ',
            c => c,
        };
        out.push(c);
    }
    // 空白の連なりは一つに（改行やタブは空白に）。全角の空白は字として残す。
    let out = out
        .split([' ', '\t', '\n', '\r'])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    // 長い題は切る ── 段落をそのまま題にしたノートで、名前が路を埋め尽くさない。
    let out: String = out.chars().take(80).collect();
    // 先頭の `.` は隠しファイルになる。末尾の `.` と空白は Windows が落とす。
    let out = out.trim_start_matches('.').trim_matches([' ', '.', '\u{3000}']).to_string();
    if out.is_empty() {
        return None;
    }
    // `CON.md` は Windows では CON のまま。
    let head = out.split('.').next().unwrap_or(&out).to_ascii_uppercase();
    if crate::note::RESERVED.contains(&head.as_str()) {
        return Some(format!("_{out}"));
    }
    Some(out)
}

/// このノートの、題から見たあるべき幹。題が空なら `None`（時刻の名前のまま）。
pub fn wanted_stem(note: &Path) -> Option<String> {
    let n = crate::note::read(note, 60)?;
    title_name(&n.title)
}

/// 名前が同じか（大文字小文字は見ない ── Mac と Windows は区別しない）。
fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// `買い物.2` の形なら、その幹と番号。
fn numbered(stem: &str) -> Option<(&str, u32)> {
    let (head, n) = stem.rsplit_once('.')?;
    let n: u32 = n.parse().ok()?;
    (n >= 2 && !head.is_empty()).then_some((head, n))
}

/// このフォルダで `wanted` に付ける名前。空いていれば `wanted.md`、
/// 取られていれば**いちばん大きい番号＋1**（空き番は埋めない ── 本人）。
fn free_name(dir: &Path, wanted: &str, me: &Path) -> String {
    let mut taken = false;
    let mut top = 1u32;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.path() == me {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(".md") else { continue };
            if same(stem, wanted) {
                taken = true;
            } else if let Some((head, n)) = numbered(stem) {
                if same(head, wanted) {
                    taken = true;
                    top = top.max(n);
                }
            }
        }
    }
    if !taken {
        format!("{wanted}.md")
    } else {
        format!("{wanted}.{}.md", top.max(1) + 1)
    }
}

/// **題に合わせて改名する。** 改名したら新しい道、しなくてよければ `None`。
///
/// 改名しないもの: 題が空・もう合っている（`買い物.2` のように番号付きで
/// 合っているものも）・クラウドが置いていった控え（名前に印がある）。
pub fn settle(root: &Path, note: &Path) -> anyhow::Result<Option<(PathBuf, bool)>> {
    let name = note.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if crate::cloud::shape(&name).is_some() {
        return Ok(None);
    }
    let Some(wanted) = wanted_stem(note) else { return Ok(None) };
    let stem = note.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if same(&stem, &wanted) {
        return Ok(None);
    }
    if let Some((head, _)) = numbered(&stem) {
        if same(head, &wanted) {
            return Ok(None);
        }
    }
    let Some(dir) = note.parent() else { anyhow::bail!("道がありません") };
    let to = dir.join(free_name(dir, &wanted, note));
    let rewrote = relocate(root, note, &to, true)?;
    Ok(Some((to, rewrote)))
}

/// 一本を `to` へ。**付いてくるものを一つ残らず連れて行く。**
///
/// 絵・本文のリンク・履歴の棚・共有から戻る場所・同期の憶え。`record_move`
/// なら同期の憶えに「まだ向こうに伝えていない改名」を残す（向こうの改名を
/// こちらに写すときは残さない ── 向こうはもう知っている）。
pub fn relocate(root: &Path, from: &Path, to: &Path, record_move: bool) -> anyhow::Result<bool> {
    if to.exists() {
        anyhow::bail!("{} はもうあります", to.display());
    }
    let Some(from_dir) = from.parent() else { anyhow::bail!("道がありません") };
    let Some(to_dir) = to.parent() else { anyhow::bail!("道がありません") };
    std::fs::create_dir_all(to_dir)?;

    // ── 絵と、本文のリンク ──
    //
    // 絵は `attachments/<幹>-<時計>.png` という名前で、幹でノートに結び付く
    // （`note::move_to` はそれで自分の絵を見分ける）。幹が変わるなら絵も
    // 改名し、本文のリンクも書き直す ── 絵だけ古い名前で残すと、次にフォルダを
    // 移した日に置き去りになる。
    let old_prefix = crate::note::file_stem(&stem_of(from));
    let new_prefix = crate::note::file_stem(&stem_of(to));
    let mut renamed_pictures: Vec<(String, String)> = Vec::new();
    if old_prefix != new_prefix && !old_prefix.is_empty() {
        let att = from_dir.join("attachments");
        if let Ok(rd) = std::fs::read_dir(&att) {
            let mut names: Vec<String> =
                rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            for n in names {
                let Some(rest) = n.strip_prefix(&format!("{old_prefix}-")) else { continue };
                let new_name = format!("{new_prefix}-{rest}");
                if att.join(&new_name).exists() {
                    continue; // 取られている ── その絵は古い名前のまま（リンクも触らない）
                }
                if std::fs::rename(att.join(&n), att.join(&new_name)).is_ok() {
                    renamed_pictures.push((n, new_name));
                }
            }
        }
    }
    let mut rewrote = false;
    if !renamed_pictures.is_empty() {
        // UTF-8 で読めるノートだけ書き直す（Shift_JIS のノートは、絵の名前が
        // ASCII でないかぎりリンクが古いまま残る ── 壊すよりよい）。
        if let Ok(text) = std::fs::read_to_string(from) {
            let mut out = text.clone();
            for (a, b) in &renamed_pictures {
                out = out.replace(&format!("attachments/{a}"), &format!("attachments/{b}"));
            }
            if out != text {
                std::fs::write(from, out)?;
                rewrote = true;
            }
        }
    }

    // ── ノートそのもの ──
    std::fs::rename(from, to)?;

    // ── 履歴の棚（`.amber/history/<道>/`）──
    if let (Some(a), Some(b)) = (crate::history::shelf(root, from), crate::history::shelf(root, to)) {
        if a.is_dir() && !b.exists() {
            if let Some(p) = b.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::rename(&a, &b);
        }
    }

    // ── 道で憶えているもの ──
    if let (Some(fr), Some(tr)) = (rel_of(root, from), rel_of(root, to)) {
        crate::notebook::came_moved(root, &fr, &tr);
        crate::sync::moved(root, &fr, &tr, record_move);
    }
    Ok(rewrote)
}

fn stem_of(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn rel_of(root: &Path, p: &Path) -> Option<String> {
    let r = p.strip_prefix(root).ok()?;
    Some(r.to_string_lossy().replace('\\', "/"))
}

/// 一度きり: **時刻の名前のまま残っているノートを、題の名前に揃える**（決めごと 7）。
///
/// 触るのは amber が付けた時刻の名前（`2026-09-06 19-18-30.md`）だけ。人が
/// 名づけたファイルは、題と違っていてもここでは触らない ── 開いて離れたときに
/// 揃う。返すのは改名した (前, 後) の道。
pub fn tidy_names(root: &Path) -> Vec<(PathBuf, PathBuf)> {
    let stop = std::sync::atomic::AtomicBool::new(false);
    let (found, _) = crate::note::list(
        root,
        crate::survey::Limits { depth: 6, rows: 4000, hidden: false, ..Default::default() },
        &stop,
    );
    let mut out = Vec::new();
    for f in found {
        let stem = stem_of(&f.note.path);
        if !crate::note::made_up_name(&stem) {
            continue;
        }
        if let Ok(Some((to, _))) = settle(root, &f.note.path) {
            out.push((f.note.path.clone(), to));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 題が_そのまま名前になる() {
        assert_eq!(title_name("買い物"), Some("買い物".into()));
        assert_eq!(title_name("  牛乳 と  パン \n"), Some("牛乳 と パン".into()));
        assert_eq!(title_name(""), None);
        assert_eq!(title_name("   "), None);
    }

    #[test]
    fn 使えない字は_全角に置き換わる() {
        // 本人が決めた（決めごと 3）── 潰すのではなく、読めるように置き換える。
        assert_eq!(title_name("A/B:C*D?E\"F<G>H|I\\J"), Some("A／B：C＊D？E＂F＜G＞H｜I＼J".into()));
        assert_eq!(title_name(".隠れる"), Some("隠れる".into()));
        assert_eq!(title_name("末尾の点."), Some("末尾の点".into()));
        assert_eq!(title_name("con"), Some("_con".into()));
        let long: String = "あ".repeat(100);
        assert_eq!(title_name(&long).unwrap().chars().count(), 80);
    }

    fn root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn 題を付けたノートは_その名前になる() {
        let d = root();
        let at = d.path().join("2026-09-06 19-18-30.md");
        std::fs::write(&at, "---\ntitle: 買い物\n---\n\n- 牛乳\n").unwrap();
        let (to, _) = settle(d.path(), &at).unwrap().unwrap();
        assert_eq!(to, d.path().join("買い物.md"));
        assert!(!at.exists() && to.exists());
        // もう合っているなら、何もしない。
        assert_eq!(settle(d.path(), &to).unwrap(), None);
    }

    #[test]
    fn 見出しや一行目からも_名前になる() {
        let d = root();
        let at = d.path().join("2026-09-06 19-18-30.md");
        std::fs::write(&at, "---\ncreated: 2026-09-06\n---\n\nめそぽたみあ\n").unwrap();
        let (to, _) = settle(d.path(), &at).unwrap().unwrap();
        assert_eq!(to, d.path().join("めそぽたみあ.md"));
        // 題が空なら、時刻の名前のまま。
        let empty = d.path().join("2026-09-10 14-27-58.md");
        std::fs::write(&empty, "---\ncreated: 2026-09-10\n---\n\n").unwrap();
        assert_eq!(settle(d.path(), &empty).unwrap(), None);
    }

    #[test]
    fn 同じ題は_番号を加算する() {
        let d = root();
        std::fs::write(d.path().join("買い物.md"), "# 買い物\n").unwrap();
        let two = d.path().join("a.md");
        std::fs::write(&two, "# 買い物\n").unwrap();
        assert_eq!(settle(d.path(), &two).unwrap().unwrap().0, d.path().join("買い物.2.md"));
        let three = d.path().join("b.md");
        std::fs::write(&three, "# 買い物\n").unwrap();
        assert_eq!(settle(d.path(), &three).unwrap().unwrap().0, d.path().join("買い物.3.md"));
        // **空き番は埋めない**（本人）── 2 を消しても、次は 4。
        std::fs::remove_file(d.path().join("買い物.2.md")).unwrap();
        let four = d.path().join("c.md");
        std::fs::write(&four, "# 買い物\n").unwrap();
        assert_eq!(settle(d.path(), &four).unwrap().unwrap().0, d.path().join("買い物.4.md"));
        // 番号付きで合っているものは、番号を付け直さない。
        assert_eq!(settle(d.path(), &d.path().join("買い物.3.md")).unwrap(), None);
        // 大文字小文字だけ違うものも、同じ名前（Mac と Windows は区別しない）。
        std::fs::write(d.path().join("Memo.md"), "# memo\n").unwrap();
        assert_eq!(settle(d.path(), &d.path().join("Memo.md")).unwrap(), None);
        let m = d.path().join("x.md");
        std::fs::write(&m, "# MEMO\n").unwrap();
        assert_eq!(settle(d.path(), &m).unwrap().unwrap().0, d.path().join("MEMO.2.md"));
    }

    #[test]
    fn 競合の控えは_改名しない() {
        let d = root();
        let at = d.path().join("買い物 (Taketan の競合コピー 2026-09-08).md");
        std::fs::write(&at, "# 買い物\n").unwrap();
        assert_eq!(settle(d.path(), &at).unwrap(), None);
    }

    #[test]
    fn 絵とリンクと履歴と憶えが_付いてくる() {
        let d = root();
        let r = d.path();
        let at = r.join("2026-09-06 19-18-30.md");
        std::fs::create_dir_all(r.join("attachments")).unwrap();
        std::fs::write(r.join("attachments/2026-09-06 19-18-30-1.png"), [1u8]).unwrap();
        std::fs::write(&at, "# 旅\n![](attachments/2026-09-06 19-18-30-1.png)\n").unwrap();
        crate::history::keep(r, &at, "前の姿", 0, true, false).unwrap();
        crate::notebook::came_from(r, "2026-09-06 19-18-30.md", "くらし").unwrap();
        crate::sync::remember(r, "drive", &[crate::sync::Was {
            rel: "2026-09-06 19-18-30.md".into(), hash: "h".into(), id: "i".into(), tag: "t".into(),
        }], &[]).unwrap();

        let (to, rewrote) = settle(r, &at).unwrap().unwrap();
        assert_eq!(to, r.join("旅.md"));
        assert!(rewrote, "リンクを書き直したと言わない");
        assert!(r.join("attachments/旅-1.png").exists(), "絵が付いてこない");
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "# 旅\n![](attachments/旅-1.png)\n");
        assert!(crate::history::shelf(r, &to).unwrap().is_dir(), "履歴の棚が付いてこない");
        assert_eq!(crate::notebook::read(r).came.get("旅.md").map(String::as_str), Some("くらし"));
        let was = crate::sync::recall(r, "drive");
        assert_eq!(was.len(), 1);
        assert_eq!(was[0].rel, "旅.md");
        assert_eq!(was[0].id, "i");
        assert_eq!(crate::sync::moves(r, "drive"), vec![("旅.md".to_string(), "2026-09-06 19-18-30.md".to_string())]);
    }

    #[test]
    fn 時刻の名前だけを_一度に揃える() {
        let d = root();
        let r = d.path();
        std::fs::write(r.join("2026-09-06 19-18-30.md"), "めそぽたみあ\n").unwrap();
        std::fs::write(r.join("2026-09-10 14-27-58.md"), "---\ncreated: 2026-09-10\n---\n\n").unwrap();
        // 人が名づけたものは、題と違っていても触らない。
        std::fs::write(r.join("memo.md"), "# 別の題\n").unwrap();
        let done = tidy_names(r);
        assert_eq!(done.len(), 1);
        assert!(r.join("めそぽたみあ.md").exists());
        assert!(r.join("2026-09-10 14-27-58.md").exists(), "空のノートは時刻のまま");
        assert!(r.join("memo.md").exists());
    }
}
