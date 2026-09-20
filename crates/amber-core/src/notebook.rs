//! ノートのフォルダが、自分自身について持っている情報。
//!
//! ノートの中に書けないものが 2 つある。**フォルダ**の色（書く先のノートが無い）と、
//! **空の**お気に入りフォルダ（そのフォルダを名前に持つノートだけで存在している
//! フォルダは、最後の 1 つが出て行った瞬間に消える）。この 2 つだけを、ノートの
//! 隣に置いた小さな JSON（`.cian/settings.json`）に入れる。
//!
//! **それ以外はすべてノートの中に残す。** ノートがお気に入りかどうかはノート自身に
//! 書いてあるので、このファイルを失っても失うのは色と空のフォルダだけで、ノート
//! 自身の情報は失われない。ここに置いてよいかどうかの判定はそれ ── 失ったときに
//! 「書いたもの」が失われるなら、ここには置かない。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Book {
    /// フォルダのパス（ルートからの相対）→ 付けられた色。
    ///
    /// 値は自由形式。選ぶのは使う人なので、アプリはパレットを提案するだけで
    /// 強制しない。
    pub colors: BTreeMap<String, String>,
    /// お気に入りのフォルダ。まだ何も入っていないものも含む。
    pub stars: Vec<String>,
    /// 共有フォルダへ移したノートの、**移動前のフォルダ**。ルートからの相対パス →
    /// フォルダ（ルート直下だったなら空）。
    ///
    /// **ノートには書かない。** ノートはただの Markdown で、amber の都合を中に
    /// 書き足す理由が無い ── 家族に渡ったノートに「元は くらし にあった」と
    /// 書いてあっても、相手には何の意味も無い。この設定ファイルに 1 行持つ。
    ///
    /// 記録するのは共有へ入れるときだけ。通常の「フォルダへ移動」では記録しない ──
    /// 人が自分で選んで動かしたものに、戻し先は要らない。
    pub came: BTreeMap<String, String>,
}

/// 共有フォルダであることを示すファイル。**設定ではなく、フォルダ自身が持つ。**
///
/// 最初は `.amber/settings.json` に「どれが共有か」を書いていた。動きはしたが、
/// **相手の amber には何も伝わらない** ── 受け取った人が自分で「これが共有です」と
/// 設定し直す必要があり、端末を替えるたびにもう一度必要だった。
///
/// フォルダの中にファイルを 1 つ置けば、**読むだけで分かる**。設定する手間が、
/// どちらの側からも消える。しかも**フォルダと一緒に移動する**ので、置き場所を
/// 変えても端末を替えてもずれない ── amber がもともと持っていた考え方
/// （「隠しデータベースを持たない。全部フォルダの中のファイル」）そのもの。
pub const SHARE_MARK: &str = ".amber-share.json";

/// そのファイルに書いてある内容。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shelf {
    /// 誰が共有を始めたか。空のこともある（名前を入れなかった場合）。
    pub by: String,
    /// いつから。`YYYY-MM-DD`。
    pub since: String,
}

/// このフォルダは共有フォルダか。そのファイルを読む。
pub fn share_mark(dir: &Path) -> Option<Shelf> {
    let text = std::fs::read_to_string(dir.join(SHARE_MARK)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(Shelf {
        by: v.get("by").and_then(|s| s.as_str()).unwrap_or("").to_string(),
        since: v.get("since").and_then(|s| s.as_str()).unwrap_or("").to_string(),
    })
}

/// そのファイルを置く。**既にあれば触らない** ── 相手が置いたファイルの
/// 「誰が」を、こちらの名前で上書きしない（共有を始めたのは相手なので）。
pub fn mark_share(dir: &Path, by: &str, today: &str) -> anyhow::Result<()> {
    if share_mark(dir).is_some() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    let v = serde_json::json!({ "by": by, "since": today });
    std::fs::write(dir.join(SHARE_MARK), serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

/// そのファイルを外す。**中のノートには触らない。**
pub fn unmark_share(dir: &Path) -> anyhow::Result<()> {
    let at = dir.join(SHARE_MARK);
    if at.exists() {
        std::fs::remove_file(at)?;
    }
    Ok(())
}

/// フォルダに付けられる十一色。
///
/// **定義はここ 1 か所だけ。** 以前はデスクトップ版の `PALETTE` と iPhone の
/// `Colouring.palette` に同じものを書いていて、「同じ並び」と両方のコメントに
/// 書いてあった ── それでも**11 色のうち 6 色がずれていた**。iPhone で付けた青が、
/// Mac では少し違う青で表示されていた。コピーを持てば、いつかずれる。
///
/// 色でしか区別できないノートは grep に映らず、読み上げにも伝わらないので、色は
/// 増やさない。名前はカタカナで揃える ── 「みどり青」と「青むらさき」は、2 つ
/// 並べたときにどちらがどちらか言えない。
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
    // 以前は `#5E8C42`（オリーブ寄り）で、緑というよりくすんだ黄土色に見えた。
    ("#3FA05C", "グリーン"),
    ("#7A7A7A", "グレー"),
];

/// 色とブックマークの置き場所（`<root>/.amber/settings.json`）。
///
/// **隠しフォルダは 1 つだけにする。** 履歴は `.amber/history/` に置いていたのに、
/// 色とお気に入りは `.cian/settings.json`、同期の記録は `.cian/sync.json` に
/// 置いていた ── 名前を amber に替えたときに、片方だけ替えそこねていた。
/// 人のノートのフォルダに amber のものが**2 か所**あって、片方が古い名前で
/// 残っているのは、いつか片方だけ消される形。
pub fn file(root: &Path) -> PathBuf {
    root.join(".amber").join("settings.json")
}

/// 以前の置き場所。**まだ移行していないフォルダのために、読むときだけ見る。**
fn old_file(root: &Path) -> PathBuf {
    root.join(".cian").join("settings.json")
}

/// 前の隠しフォルダ（`.cian`）に残っているものを、`.amber` へ移す。
///
/// **隠しフォルダは 1 つだけにする。** 履歴は `.amber/history/` に置いていたのに、
/// 色とブックマークは `.cian/settings.json`、同期の記録は `.cian/sync.json` に
/// 置いていた ── 名前を amber に替えたときに、片方だけ替えそこねていた。
/// 人のノートのフォルダに amber のものが 2 か所あって、片方が古い名前で残って
/// いるのは、**いつか片方だけ消される**形。
///
/// **コピーしてから消す。** `rename` は別のディスクをまたぐと失敗するし、先に
/// 消すと途中で失敗したときに色も記録も無くなる。**移行先に同じ名前があるなら
/// 何もしない** ── そちらが新しい。
///
/// 同期の記録（`sync.json`）もここで一緒に移す。移す処理を 2 か所に書くと、
/// 片方だけ直したときに `.cian` が半分だけ残る。
pub fn tidy(root: &Path) {
    let old = root.join(".cian");
    if !old.is_dir() {
        return;
    }
    let new = root.join(".amber");
    for name in ["settings.json", "sync.json"] {
        let (from, to) = (old.join(name), new.join(name));
        if !from.is_file() || to.exists() {
            continue;
        }
        if std::fs::create_dir_all(&new).is_err() {
            return;
        }
        if std::fs::copy(&from, &to).is_ok() {
            let _ = std::fs::remove_file(&from);
        }
    }
    // 空になったら削除する。**空でなければ何もしない** ── `remove_dir` は中身の
    // あるフォルダを消さないので、こちらが知らないものを置いた人のぶんは残る。
    let _ = std::fs::remove_dir(&old);
}

/// そのフォルダが自身について持っている情報、または既定値。
///
/// **エラーを返さない。** ファイルが無いのは「まだ色を付けていないフォルダ」で
/// あり、壊れていることはノートの表示を拒否する理由にならない ── 大事なのは
/// ノートのほうで、ここにあるのは見た目と付随情報だけ。
pub fn read(root: &Path) -> Book {
    // **`.cian` にあるなら、そちらが正。** 今日まで書き込んでいたのはそこで、
    // `.amber/settings.json` があるとすれば、改名前のもっと古い版が置いていった
    // もの ── 新しいほうを先に読むと、色が何世代か巻き戻る。次に何か書き込んだ
    // 時点で `.amber` へ移り、`.cian` は消える。
    let from = if old_file(root).exists() { old_file(root) } else { file(root) };
    let Ok(text) = std::fs::read_to_string(from) else { return Book::default() };
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
    if let Some(m) = v.get("came").and_then(|c| c.as_object()) {
        for (k, val) in m {
            if let Some(s) = val.as_str() {
                b.came.insert(k.clone(), s.to_string());
            }
        }
    }
    b
}

/// 書き戻す。`.amber` が無ければ作る。以前の隠しフォルダに残っているものは、
/// 書く前に引き取る（[`tidy`]）。
pub fn write(root: &Path, b: &Book) -> anyhow::Result<()> {
    tidy(root);
    let at = file(root);
    if let Some(dir) = at.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let v = serde_json::json!({ "colors": b.colors, "stars": b.stars, "came": b.came });
    std::fs::write(at, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

/// 走査結果から、共有フォルダを示すファイルがあるものを拾う（ルートからの相対パスで）。
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

/// このパスは、共有フォルダの中か。
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

/// フォルダに色を付ける、または外す。
pub fn set_color(root: &Path, folder: &str, color: Option<&str>) -> anyhow::Result<()> {
    let mut b = read(root);
    match color {
        Some(c) => b.colors.insert(folder.to_string(), c.to_string()),
        None => b.colors.remove(folder),
    };
    write(root, &b)
}

/// お気に入りのフォルダを記録する。空のフォルダが明日も残っているようにするため。
pub fn add_star(root: &Path, folder: &str) -> anyhow::Result<()> {
    let mut b = read(root);
    if folder.is_empty() || b.stars.iter().any(|s| s == folder) {
        return Ok(());
    }
    b.stars.push(folder.to_string());
    b.stars.sort();
    write(root, &b)
}

/// 共有フォルダへ入れたノートの、移動前のフォルダを記録する。
///
/// `rel` は移動後のルートからの相対パス（`家族/買い物.md`）、`from` は移動前の
/// いたフォルダ（ルート直下だったなら空）。
pub fn came_from(root: &Path, rel: &str, from: &str) -> anyhow::Result<()> {
    let mut b = read(root);
    b.came.insert(rel.to_string(), from.to_string());
    write(root, &b)
}

/// 憶えを忘れる（憶えていた場所を返す）。**戻すのは一度きり** ── 戻した
/// あとも憶えていると、別のフォルダへ移してからもう一度共有して外した人が、
/// **二回前の場所**へ連れて行かれる。
///
/// 憶えが無ければ `None`（いちばん上へ戻す、といういままでの形）。
pub fn came_back(root: &Path, rel: &str) -> Option<String> {
    let mut b = read(root);
    let from = b.came.remove(rel)?;
    let _ = write(root, &b);
    Some(from)
}

/// ノートが改名された ── 記録のキーを新しいパスへ（依頼 492）。記録が無ければ何もしない。
pub fn came_moved(root: &Path, from: &str, to: &str) {
    let mut b = read(root);
    if let Some(v) = b.came.remove(from) {
        b.came.insert(to.to_string(), v);
        let _ = write(root, &b);
    }
}

/// 1 件と、その配下すべてを記録から消す。
pub fn drop_star(root: &Path, folder: &str) -> anyhow::Result<()> {
    let mut b = read(root);
    let under = format!("{folder}/");
    b.stars.retain(|s| s != folder && !s.starts_with(&under));
    write(root, &b)
}

/// `from` の配下すべてを `to` へ移す。
///
/// **コピー → 確認 → 削除の順。** クラウドをまたぐ場合これは rename ではない。
/// こちら側から何かを取り除く前に、向こう側へ中身が書き終わっている必要があり、
/// どこかで失敗しても元のファイルは残っている。移動の途中でノートを失うのは、
/// ノートアプリがやりうる最悪のことだから。
///
/// 上書きは一切しない。移動先に同じ名前が既にあれば、移動全体を中止する ──
/// 2 つのノートフォルダを統合するかどうかは人が決めることで、ここで勝手に
/// 決める場面ではない。
///
/// 移すのはノートだけではない。画像はノートの隣の `attachments/` にあり、
/// `.cian` には色と空のフォルダが入っている。ノートだけ移して画像を置いて
/// いったら、画像のあるノートが全部壊れる。
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
    // ここで初めて削除する。すべて移動先に書き終わっている。
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
/// その形式で、`cloud::shape` は当てずっぽうで印を付けないためにその名前を
/// 拾わないと決めてある ── 持ってきたノートを、貼られない控えと同じ顔に
/// しない（依頼 316）。
///
/// **元のファイルは動かさない。** 写すだけ ── 人が選んだのは自分の
/// フォルダにあるもので、amber がそれを引き取っていい理由は無い。
///
/// 判断（名前の付け直し）がここにあるのは、デスクトップ版と iPhone に 2 つ書くと必ず
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

/// バックアップを書き戻す。返すのは (書き戻した数, 手を付けなかった数)。
///
/// **ノートフォルダの中へ入れるだけで、ノートを上書きすることはない。** 上書き
/// する復元は、先週のコピーで今日の作業を失わせうる復元になる。
/// zip の中のラベルから、何の範囲のバックアップかを読む。無ければ `None`。
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

    // まず専用の一時ディレクトリへ展開する。展開途中のアーカイブがノートの
    // 中に混ざることが決して無いように。
    //
    // **一時ディレクトリは呼び出しごとに作る。プロセスごとではない。** pid だけで
    // 名前を付けると、同時に 2 つ復元したときに同じディレクトリへ展開され、互いの
    // ファイルを数えてしまう ── さらに 2 つ目の `remove_dir_all` が、1 つ目の展開
    // 途中のアーカイブを足元から消しうる。1 プロセスで 2 回復元するテストが、
    // 1 件しか入っていないのに 2 件と報告されて見つけた。異常終了した実行が残した
    // ディレクトリが他人の問題になることも、これで無くなる。
    static ROOM: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nth = ROOM.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let hold = std::env::temp_dir().join(format!(
        "amber-restore-{}-{nth}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&hold);
    std::fs::create_dir_all(&hold)?;

    let members = crate::zipbox::list(zip)?;
    // **フォルダ自身の名前を取り除く ── ただしノートフォルダ全体のときだけ。**
    //
    // ノートフォルダのバックアップは*そのフォルダの* zip なので、すべてのエントリ
    // is `ノート/…`; put back as-is it makes a `ノート` inside the notes and
    // 中のノートがすべて新規に見える。**1 つのフォルダ**のバックアップは、まったく
    // same shape (`仕事/…`) and must keep its head — strip it and 週報 comes
    // back to the root instead of to 仕事, which is not "back".
    //
    // 構造からは区別できないので、zip 自身がどちらかを持つ（`zipbox::LABEL`・
    // 作成時に書き込む）。そのラベルが無かった頃のアーカイブは、従来の推測に
    // フォールバックする ── ノートフォルダ全体の場合には正しく、そちらのほうが
    // よく取られるバックアップだから。
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
                // ラベルはノートではないので、ノートフォルダには展開しない。
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
    fn 名前が埋まっていたら_取り込みは両方を残す() {
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
    fn フォルダは色と_空のお気に入りを憶えている() {
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
    fn 壊れた設定ファイルは_ノートを失う理由にならない() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".amber")).unwrap();
        std::fs::write(file(dir.path()), "{ これは JSON ではない").unwrap();
        assert_eq!(read(dir.path()), Book::default());
    }

    #[test]
    fn 前の隠しフォルダに置いた色は_引き継がれて片付く() {
        // `.cian` に色を置いていたフォルダを開いても、色は消えない ── そして
        // 何か書いた時点で `.amber` へ移り、古いほうは残らない。
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".cian")).unwrap();
        std::fs::write(
            dir.path().join(".cian").join("settings.json"),
            r##"{"colors":{"仕事":"#D07A2E"},"stars":["買い物"]}"##,
        )
        .unwrap();
        let b = read(dir.path());
        assert_eq!(b.colors.get("仕事").map(String::as_str), Some("#D07A2E"));
        assert_eq!(b.stars, vec!["買い物".to_string()]);

        write(dir.path(), &b).unwrap();
        assert!(file(dir.path()).exists(), ".amber に移っていること");
        assert!(!dir.path().join(".cian").exists(), "空になった .cian は残さない");
        // 移したあとも、同じことを言う。
        assert_eq!(read(dir.path()), b);
    }

    #[test]
    fn 共有をやめたら_もといたフォルダへ戻る() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // 「くらし」に居た買い物リストを、「家族」へ入れた。
        came_from(root, "家族/買い物.md", "くらし").unwrap();
        assert_eq!(read(root).came.get("家族/買い物.md").map(String::as_str), Some("くらし"));
        // やめたら、そこへ戻る ── そして**忘れる**。
        assert_eq!(came_back(root, "家族/買い物.md"), Some("くらし".into()));
        assert_eq!(came_back(root, "家族/買い物.md"), None, "戻すのは一度きり");
        assert!(read(root).came.is_empty());
    }

    #[test]
    fn ルート直下から入れたノートは_ルート直下へ戻る() {
        // 空は「憶えが無い」ではなく「いちばん上に居た」── 取り違えると、
        // 上に居たノートが戻ってこない。
        let dir = tempfile::tempdir().unwrap();
        came_from(dir.path(), "家族/めも.md", "").unwrap();
        assert_eq!(came_back(dir.path(), "家族/めも.md"), Some(String::new()));
    }

    #[test]
    fn 色とブックマークと憶えは_同じ帳面に並んで残る() {
        // どれか一つを書いたときに、ほかが消えない。
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        add_star(root, "買い物").unwrap();
        let mut b = read(root);
        b.colors.insert("仕事".into(), "#D07A2E".into());
        write(root, &b).unwrap();
        came_from(root, "家族/め.md", "仕事").unwrap();
        let got = read(root);
        assert_eq!(got.stars, vec!["買い物".to_string()]);
        assert_eq!(got.colors.get("仕事").map(String::as_str), Some("#D07A2E"));
        assert_eq!(got.came.get("家族/め.md").map(String::as_str), Some("仕事"));
    }

    #[test]
    fn 古い方が居るあいだは_古い方を読む() {
        // `.amber/settings.json` は名前を替える前のもっと古い版が置いた
        // もの ── 新しい名前のほうを先に読むと、色が何代か巻き戻る。
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".cian")).unwrap();
        std::fs::create_dir_all(dir.path().join(".amber")).unwrap();
        std::fs::write(dir.path().join(".amber").join("settings.json"), r#"{"stars":["ふるい"]}"#)
            .unwrap();
        std::fs::write(dir.path().join(".cian").join("settings.json"), r#"{"stars":["いま"]}"#)
            .unwrap();
        assert_eq!(read(dir.path()).stars, vec!["いま".to_string()]);
    }
}
