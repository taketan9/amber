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
//! 3. 使えない文字は**全角に置き換える**。先頭の `.` は外す。長いタイトルは 80 文字で切る。
//! 4. 同じ題は `買い物.2.md` `買い物.3.md`。番号は**加算**（空き番を埋めない）。
//! 5. 改名に付いてくるもの: 画像（`attachments/<名前>-…`）と本文のリンク・履歴のディレクトリ・
//!    共有から戻る場所の憶え・同期の憶え。
//! 6. 同期では「削除＋新規」ではなく**名前が変わった**として運ぶ（`sync::moved`）。
//! 7. すでにある時刻名のノートは、一度だけ題の名前に揃える（`tidy_names`）。
//!
//! ここは**いつ**改名するかを決めない ── それはデスクトップ版と iPhone の仕事（入力欄から出た・
//! ノートから離れた）。ここが決めるのは**何という名前にするか**と、改名に
//! 付いてくるものを一つ残らず連れて行くこと。

use std::path::{Path, PathBuf};

/// タイトルからファイル名の本体（拡張子なし）。**使えない文字は全角に。**
///
/// `file_stem`（画像の名前に使う）は使えない文字を `-` に潰すが、ここは読める
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
    // 連続する空白は 1 つに（改行やタブは空白に）。全角の空白は文字として残す。
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

/// **タイトルに合わせて改名する。** 改名したら新しいパス、しなくてよければ `None`。
///
/// 改名しないもの: 題が空・もう合っている（`買い物.2` のように番号付きで
/// 合っているものも）・クラウドが置いていった控え（名前に目印がある）。
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
    let Some(dir) = note.parent() else { anyhow::bail!("パスがありません") };
    let to = dir.join(free_name(dir, &wanted, note));
    let rewrote = relocate(root, note, &to, true)?;
    Ok(Some((to, rewrote)))
}

/// 一本を `to` へ。**付いてくるものを一つ残らず連れて行く。**
///
/// 画像・本文のリンク・履歴のディレクトリ・共有から戻る場所・同期の記録。`record_move`
/// なら同期の憶えに「まだ向こうに伝えていない改名」を残す（向こうの改名を
/// こちらに写すときは残さない ── 向こうはもう知っている）。
pub fn relocate(root: &Path, from: &Path, to: &Path, record_move: bool) -> anyhow::Result<bool> {
    if to.exists() {
        anyhow::bail!("{} はもうあります", to.display());
    }
    if from.parent().is_none() {
        anyhow::bail!("パスがありません")
    }
    let Some(to_dir) = to.parent() else { anyhow::bail!("パスがありません") };
    std::fs::create_dir_all(to_dir)?;

    // ── 画像と、本文のリンク ──
    let fresh = bring_pictures(from, to)?;

    // ── ノートそのもの ──
    std::fs::rename(from, to)?;
    // **書き戻すのは動いたあと、新しい場所へ。** 先に書くと、移動が転んだ
    // ときに「元の場所にあるノートが、向こうの画像を指している」状態が残る。
    let rewrote = match fresh {
        Some(t) => {
            std::fs::write(to, t)?;
            true
        }
        None => false,
    };
    carry(root, from, to, record_move);
    Ok(rewrote)
}

/// ノートが連れて行く画像を、引っ越し先の隣へ。返すのは書き直した本文
/// （直すところが無ければ `None`）。**ノートそのものは動かさない** ──
/// 呼ぶ側が最後に動かす（画像が置けないなら、ノートも動かさないため）。
///
/// **名前では見分けない ── 本文が指しているものを運ぶ。**（依頼 576）
/// 前は `attachments/<幹>-<時計>` の付け方で見分けていたので、改名で幹が
/// 変われば見失い（だから改名のたびに画像まで改名していた）、その付け方でない
/// 画像 ── OneNote から写した `<ページ名>_001.png` など ── は拾えなかった。
///
/// ほかのノートも指している画像は、動かさずに写す。動かすと、残ったほうは
/// 一文字も触られていないのに画像を失う。写せば両方が自分の隣の
/// `attachments/` を見たままで、`..` の付いたパスが 1 つも生まれない ──
/// ノートはいつでも自分と隣の `attachments/` だけで持ち運べる。
///
/// 探すのは移動元のフォルダの中だけでよい。`..` が無い以上、その画像を
/// 指しうるノートは同じフォルダにしか居ない。
pub(crate) fn bring_pictures(from: &Path, to: &Path) -> anyhow::Result<Option<String>> {
    let (Some(from_dir), Some(to_dir)) = (from.parent(), to.parent()) else {
        return Ok(None);
    };
    // **同じフォルダでの改名では、画像に触らない。** 画像は動かず、本文の
    // `attachments/…` も変わらないので、ノートからは今までどおり見える。
    if from_dir == to_dir {
        return Ok(None);
    }
    let Some((text, utf8)) = note_text(from) else { return Ok(None) };
    let att_from = from_dir.join("attachments");
    let mut mine: Vec<PathBuf> = crate::spare::points_at(&text, from_dir)
        .into_iter()
        .filter(|p| p.parent() == Some(att_from.as_path()) && p.is_file())
        .collect();
    mine.sort();
    mine.dedup();
    if mine.is_empty() {
        return Ok(None);
    }

    let shared = pointed_at_by_others(from_dir, from);
    let att_to = to_dir.join("attachments");
    let mut plan: Vec<(PathBuf, PathBuf, bool)> = Vec::new();
    let mut renamed: Vec<(String, String)> = Vec::new();
    let mut taken: Vec<String> = Vec::new();
    for p in &mine {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let at = att_to.join(&name);
        // 先客が**同じ中身**なら、それを使う（写しも改名も要らない）。
        if !taken.contains(&name) && same_file(p, &at) {
            continue;
        }
        let free = free_picture_name(&att_to, &name, &taken);
        taken.push(free.clone());
        if free != name {
            // 符号が読めないノートは書き戻せない ── リンクを直せないまま
            // 画像の名前だけ変えると、黙って見失う。
            if !utf8 {
                anyhow::bail!("{} に同じ名前の画像があります: {}", att_to.display(), name);
            }
            renamed.push((name.clone(), free.clone()));
        }
        plan.push((p.clone(), att_to.join(&free), shared.iter().any(|s| s == p)));
    }
    if plan.is_empty() && renamed.is_empty() {
        return Ok(None);
    }
    std::fs::create_dir_all(&att_to)?;
    for (a, b, copy) in &plan {
        if *copy {
            std::fs::copy(a, b)?;
        } else {
            std::fs::rename(a, b)?;
        }
    }
    if renamed.is_empty() {
        return Ok(None);
    }
    // `%E6%AC%A1` と書かれたリンクは直せない（ここは文字列のまま探す）。名前が
    // ぶつかるのは同じ名前で中身の違う画像が先に居たときだけなので、追って
    // いない ── 直せなかったぶんは、前の版と同じ姿で残る。
    let mut out = text.clone();
    for (a, b) in &renamed {
        out = out.replace(&format!("attachments/{a}"), &format!("attachments/{b}"));
    }
    Ok(if out != text { Some(out) } else { None })
}

/// 同じフォルダの**ほかの**ノートが指している行き先。
fn pointed_at_by_others(dir: &Path, me: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let at = e.path();
        if at == me || !is_note(&at) {
            continue;
        }
        // Shift_JIS のノートも読む。飛ばすと、その一本が指していた画像を
        // 「誰も指していない」と見て運び去る。
        if let Some((text, _)) = note_text(&at) {
            out.extend(crate::spare::points_at(&text, dir));
        }
    }
    out
}

fn is_note(at: &Path) -> bool {
    at.is_file()
        && at
            .extension()
            .map(|e| {
                let e = e.to_string_lossy().to_lowercase();
                e == "md" || e == "markdown"
            })
            .unwrap_or(false)
}

/// ノートのテキストと、**それが UTF-8 だったか**。偽なら書き戻さない ──
/// 書き戻すと符号が変わり、頼まれてもいないのにファイルが作り替わる。
fn note_text(at: &Path) -> Option<(String, bool)> {
    match std::fs::read_to_string(at) {
        Ok(t) => Some((t, true)),
        Err(_) => crate::text::read(at).ok().map(|f| (f.lines.join("\n"), false)),
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// `名前.png` が取られていたら `名前-2.png`。
fn free_picture_name(dir: &Path, name: &str, taken: &[String]) -> String {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    let mut n = 1;
    let mut try_name = name.to_string();
    while (dir.join(&try_name).exists() || taken.contains(&try_name)) && n < 1000 {
        n += 1;
        try_name = format!("{stem}-{n}{ext}");
    }
    try_name
}

/// パスで記録しているものを、新しいパスへ移す ── 履歴のディレクトリ・共有から戻る場所・
/// 同期の憶え。**改名でもフォルダ移動でも同じ一本**（`move` op もここを通る・
/// 依頼 496）。ノートそのものはもう動いたあとに呼ぶ。
pub fn carry(root: &Path, from: &Path, to: &Path, record_move: bool) {
    // ── 履歴のディレクトリ（`.amber/history/<パス>/`）──
    if let (Some(a), Some(b)) = (crate::history::shelf(root, from), crate::history::shelf(root, to)) {
        if a.is_dir() && !b.exists() {
            if let Some(p) = b.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let _ = std::fs::rename(&a, &b);
        }
    }
    // ── パスで記録しているもの ──
    if let (Some(fr), Some(tr)) = (rel_of(root, from), rel_of(root, to)) {
        crate::notebook::came_moved(root, &fr, &tr);
        crate::sync::moved(root, &fr, &tr, record_move);
    }
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
/// 揃う。返すのは改名した (前, 後) のパス。
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
    fn 使えない文字は_全角に置き換わる() {
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

    /// ノート二本が同じ画像を指しているところから、片方を動かす。
    ///
    /// **前の版は、ここで残ったほうを壊していた。** 画像を「幹が一致するもの」
    /// で見分けて連れ去るので、`会議.md` は一文字も触られていないのに
    /// 画像を失う ── 気づくのは次に開いた日で、原因からは遠い。
    #[test]
    fn 二本が指している画像は_動かさずに写す() {
        let d = root();
        let r = d.path();
        std::fs::create_dir_all(r.join("attachments")).unwrap();
        std::fs::write(r.join("attachments/段取り-1.png"), [7u8]).unwrap();
        let mine = r.join("段取り.md");
        let other = r.join("会議.md");
        std::fs::write(&mine, "---\ntitle: 段取り\n---\n\n![](attachments/段取り-1.png)\n").unwrap();
        std::fs::write(&other, "---\ntitle: 会議\n---\n\n![](attachments/段取り-1.png)\n").unwrap();

        let to = crate::note::move_to(&mine, &r.join("仕事")).unwrap();

        assert_eq!(std::fs::read(r.join("仕事/attachments/段取り-1.png")).unwrap(), vec![7u8]);
        assert_eq!(
            std::fs::read(r.join("attachments/段取り-1.png")).unwrap(),
            vec![7u8],
            "残ったノートの画像まで連れ去っている",
        );
        // どちらのノートも、**自分の隣**を見たまま。`..` は一本も生えない。
        for at in [&to, &other] {
            let text = std::fs::read_to_string(at).unwrap();
            assert!(!text.contains(".."), "`..` の道が生えた: {text}");
            let here = at.parent().unwrap();
            let seen = crate::spare::points_at(&text, here);
            assert!(seen.iter().any(|p| p.is_file()), "{at:?} から画像が見えない: {seen:?}");
        }
    }

    /// 誰も指していない画像は、連れて行かない ── それは「使われていない画像」。
    #[test]
    fn 一本しか指していない画像は_連れて行く() {
        let d = root();
        let r = d.path();
        std::fs::create_dir_all(r.join("attachments")).unwrap();
        std::fs::write(r.join("attachments/段取り-1.png"), [7u8]).unwrap();
        std::fs::write(r.join("attachments/誰も指さない.png"), [8u8]).unwrap();
        let mine = r.join("段取り.md");
        std::fs::write(&mine, "# 段取り\n![](attachments/段取り-1.png)\n").unwrap();

        crate::note::move_to(&mine, &r.join("仕事")).unwrap();

        assert!(r.join("仕事/attachments/段取り-1.png").is_file(), "画像が付いてこない");
        assert!(!r.join("attachments/段取り-1.png").exists(), "元の場所にも残っている");
        assert!(r.join("attachments/誰も指さない.png").is_file(), "指されていない画像まで連れ去った");
    }

    /// 名前の付け方では見分けない ── OneNote から写した `<ページ名>_001.png`
    /// のような画像も、本文が指していれば付いてくる。
    #[test]
    fn 名前の形が違う画像でも_本文が指していれば付いてくる() {
        let d = root();
        let r = d.path();
        std::fs::create_dir_all(r.join("attachments")).unwrap();
        std::fs::write(r.join("attachments/9月の定例_001.png"), [3u8]).unwrap();
        // **空白入りの名前も。** amber 自身の既定の名前がこの形。
        std::fs::write(r.join("attachments/2026-09-06 19-18-30-1.png"), [4u8]).unwrap();
        let mine = r.join("9月の定例.md");
        std::fs::write(
            &mine,
            "# 9月の定例\n![](attachments/9月の定例_001.png)\n![](attachments/2026-09-06 19-18-30-1.png)\n",
        ).unwrap();

        crate::note::move_to(&mine, &r.join("仕事")).unwrap();

        assert!(r.join("仕事/attachments/9月の定例_001.png").is_file(), "幹の形が違う画像が付いてこない");
        assert!(r.join("仕事/attachments/2026-09-06 19-18-30-1.png").is_file(), "空白入りの画像が付いてこない");
    }

    #[test]
    fn 画像とリンクと履歴と憶えが_付いてくる() {
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
        // **改名では、画像に触らない**（依頼 576）。画像の名前がノートの名前を
        // 追いかける必要は無い ── 同じフォルダに居るのだから、本文の
        // `attachments/…` はそのままで、ノートからは今までどおり見える。
        // 追いかけさせていた版は、**同じ画像を指しているもう一本のノートを
        // 黙って壊していた**（そのノートは一文字も触られていないのに）。
        assert!(!rewrote, "改名で本文を書き直している");
        assert!(!r.join("attachments/旅-1.png").exists(), "画像まで改名している");
        assert!(r.join("attachments/2026-09-06 19-18-30-1.png").is_file(), "画像が消えた");
        assert_eq!(
            std::fs::read_to_string(&to).unwrap(),
            "# 旅\n![](attachments/2026-09-06 19-18-30-1.png)\n",
        );
        // 依頼 492 の「画像も付いてくる」── **ノートから画像が見え続ける**こと。
        let link = crate::spare::points_at(&std::fs::read_to_string(&to).unwrap(), r);
        assert!(link.iter().any(|p| p.is_file()), "ノートから画像が見えない: {link:?}");
        assert!(crate::history::shelf(r, &to).unwrap().is_dir(), "履歴のフォルダが付いてこない");
        assert_eq!(crate::notebook::read(r).came.get("旅.md").map(String::as_str), Some("くらし"));
        let was = crate::sync::recall(r, "drive");
        assert_eq!(was.len(), 1);
        assert_eq!(was[0].rel, "旅.md");
        assert_eq!(was[0].id, "i");
        assert_eq!(crate::sync::moves(r, "drive"), vec![("旅.md".to_string(), "2026-09-06 19-18-30.md".to_string())]);
    }

    #[test]
    fn フォルダへ移しても_同期の憶えと履歴が付いてくる() {
        let d = root();
        let r = d.path();
        let at = r.join("旅.md");
        std::fs::write(&at, "# 旅\n").unwrap();
        crate::history::keep(r, &at, "前の姿", 0, true, false).unwrap();
        crate::sync::remember(r, "drive", &[crate::sync::Was {
            rel: "旅.md".into(), hash: "h".into(), id: "i".into(), tag: "t".into(),
        }], &[]).unwrap();
        let to = crate::note::move_to(&at, &r.join("仕事")).unwrap();
        carry(r, &at, &to, true);
        assert_eq!(to, r.join("仕事/旅.md"));
        assert!(crate::history::shelf(r, &to).unwrap().is_dir(), "履歴のフォルダが付いてこない");
        let was = crate::sync::recall(r, "drive");
        assert_eq!(was[0].rel, "仕事/旅.md");
        assert_eq!(crate::sync::moves(r, "drive"), vec![("仕事/旅.md".to_string(), "旅.md".to_string())]);
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
