//! ディレクトリの中に実際に何があるか。AI への問い合わせを組み立てるための事実。
//!
//! ディレクトリを読む 3 つの AI 機能 ── 不要なものを見つける、構成を提案する、
//! 目的のファイルを探す ── は、どれもモデルに**名前の一覧**だけを送っていた。
//! いちばん割を食っていたのは「不要なものを見つける」で、1 階層しか見ないので
//! 2 つ下の `node_modules` は見えず、ディレクトリのサイズは空のまま届いていた。
//! つまり、この機能が答えるべきただ 1 つの問い ──*何が容量を食っているか*── を、
//! モデルは考えようがなかった。語彙だけから推測させられていて、語彙だけから
//! 推測したとおりの答えを返していた。
//!
//! そこでここは事実を集めるだけで、何も判断しない。**不要なものの一覧は
//! プロンプト側にあって、ここには無い。** このモジュールが「`target/` は捨てて
//! よい」と決め始めた瞬間、プログラムの中に意見が 2 つできて、しかもモデル側の
//! 意見は誰にも読めない。ここが持ち寄るのは、ファイルシステムしか知らないこと ──
//! どれだけ大きいか、いつのものか、どれだけ深いか、いくつあるか。
//!
//! 探索は幅優先。上限に当たったときに、最初の大きいフォルダ以降が丸ごと落ちる
//! のではなく、いちばん深いものから落ちるようにするため。そして上限に当たったら
//! **そう報告する** ── 400 行目で黙って止まった調査結果は、受け取った側からは
//! 「400 個入っているディレクトリ」としか読めない。

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

/// 調査結果の 1 項目。
#[derive(Debug, Clone)]
pub struct Row {
    /// 調査対象のルートから見た位置。区切りは常に `/`。どの OS でも同じ形で
    /// 読めるようにするため ── モデルに 2 通りの書き方を扱わせず、1 通りだけ見せる。
    pub rel: String,
    pub path: PathBuf,
    pub is_dir: bool,
    /// 自身とその配下の合計バイト数。**ディレクトリでは再帰的に集計する。**
    /// 単なる `read_dir` で済ませていない理由がこれ。
    pub size: u64,
    /// サイズが合計ではなく下限であることを示す。この配下を集計している途中で
    /// [`SIZE_ENTRY_CAP`] に達して打ち切った。表示は `>` 付きになるが、それでも
    /// 答えとしては十分 ── 「2GB 超」でも正確な数値と同じだけ順位付けに使えて、
    /// しかも桁違いに速い。
    pub size_capped: bool,
    /// 最終更新時刻。ファイルシステムが返さないときは `None`。
    pub modified: Option<SystemTime>,
    /// ルートからの深さ。ルート直下の子が 1。
    pub depth: usize,
}

/// どこまで探索するか。どちらの上限も、キー 1 つでホームディレクトリ全体を
/// 歩き回らせないためにある。上限に当たったときは、どちらも報告される。
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// ルートからの階層数。1 なら単なる一覧。
    pub depth: usize,
    /// 返す項目数の上限。
    pub rows: usize,
    /// ドットで始まるディレクトリの中を見るか。整理系の機能では off
    /// （`.git` は不要物ではない）、名前でファイルを探すときは on。
    pub hidden: bool,
    /// 配下のサイズを集計するあいだに、*調査全体*で訪問してよいディレクトリ
    /// エントリの数。
    ///
    /// **予算は調査全体で 1 つ。ディレクトリごとではない。** ディレクトリ単位で
    /// 上限を設けた版は、Rust のチェックアウトに対して 4.5 秒かかった。800 項目が
    /// それぞれ自分の予算を使い、入れ子のものが同じファイルを何度も歩いたため。
    /// 幅優先の順に消費すれば、浅い項目 ── この機能が答える問いは、実際にはすべて
    /// そこで答えが出る ── に正確な数値が渡り、深い項目は 1 件ごとに 1 秒かける
    /// 代わりに「未集計」と言えるようになる。
    pub size_budget: usize,
}

impl Default for Limits {
    fn default() -> Self {
        // 3 階層あれば `project/src/module` や、ビルド成果物が置かれる典型的な
        // 場所には届く。キー 1 つでディスク全体を歩き回ることにもならない。
        Self { depth: 3, rows: 600, hidden: false, size_budget: 120_000 }
    }
}

/// 調査結果と、それが完全かどうか。
#[derive(Debug, Clone, Default)]
pub struct Survey {
    pub rows: Vec<Row>,
    /// この深さの途中で項目数の上限に達し、それより深いところは見ていない。
    ///
    /// **件数ではなく深さで言うのは意図的。** 最初の版は入りきらなかった件数を
    /// 数えていて、Rust のチェックアウトに対して「42160 件が入りませんでした」と
    /// 報告した ── 正しいが、役に立たず、無用に不安にさせる。探索は幅優先なので、
    /// 実際にやったのは「上位 2 階層を完全に列挙して止まった」であり、そちらの
    /// ほうがずっと有用で、読む人が知りたいのもそれ。
    pub stopped_at: Option<usize>,
    /// 深さの上限で打ち切られ、開かなかったディレクトリ。
    pub unopened: usize,
}

impl Survey {
    /// どちらかの理由で、何かが漏れているか。
    pub fn partial(&self) -> bool {
        self.stopped_at.is_some() || self.unopened > 0
    }

    /// 完全に列挙できたと分かっている、いちばん深い階層。最初の階層の途中で
    /// 止まった場合は `None`（何も言えないため）。
    pub fn whole_to(&self) -> Option<usize> {
        match self.stopped_at {
            None => self.rows.iter().map(|r| r.depth).max(),
            Some(1) => None,
            Some(d) => Some(d - 1),
        }
    }
}

/// `root` を `limits` の範囲で幅優先に探索する。
///
/// ディレクトリのサイズは*配下すべて*で集計する。深さの上限より下の部分も含む ──
/// 上限が制限するのは列挙する範囲であって、集計する範囲ではない。4GB あることだけが
/// 重要なフォルダは、中身が列挙されない場合でも、そう伝わる必要がある。
pub fn survey(root: &Path, limits: Limits, cancel: &AtomicBool) -> Survey {
    let mut out = Survey::default();
    let mut budget = limits.size_budget;
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    while let Some((dir, depth)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            return out;
        }
        // 読めないディレクトリは普通にある（権限）。調査ごと諦めるより飛ばす。
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            if cancel.load(Ordering::Relaxed) {
                return out;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            if !limits.hidden && name.starts_with('.') {
                continue;
            }
            let path = e.path();
            let ft = e.file_type();
            // シンボリックリンクはそれ自体として扱い、決して辿らない。`du` と
            // 同じ規則で、調査が無限ループしない理由でもある。
            let link = ft.as_ref().map(|t| t.is_symlink()).unwrap_or(false);
            let is_dir = !link && ft.as_ref().map(|t| t.is_dir()).unwrap_or(false);
            let meta = e.metadata().ok();
            let modified = meta.as_ref().and_then(|m| m.modified().ok());
            let (size, size_capped) = if is_dir {
                subtree_size(&path, cancel, &mut budget)
            } else {
                (meta.as_ref().map(|m| m.len()).unwrap_or(0), false)
            };
            // **上限に達したら、入りきらない件数を数えずに止める。** 探索は
            // 幅優先なので、集まっている項目はいちばん浅いものばかり ── この
            // 機能が答える問いは、すべてそこに答えがある。深さ 1 の `target` は
            // 重要だが、その中の 1 万個のオブジェクトファイルは重要ではない。
            // 飛ばした件数を言うためにそれを列挙すると、1 秒かけて誰の役にも
            // 立たない数字を出すことになる。
            if out.rows.len() >= limits.rows {
                out.stopped_at = Some(depth + 1);
                return out;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.rows.push(Row { rel, path: path.clone(), is_dir, size, size_capped, modified, depth: depth + 1 });
            if is_dir {
                if depth + 1 < limits.depth {
                    queue.push_back((path, depth + 1));
                } else {
                    out.unopened += 1;
                }
            }
        }
    }
    out
}

/// 1 つのディレクトリのサイズ集計に、何件まで費やしてよいか。これを超えると
/// 答えは「下限」になる。
///
/// **これは応答時間の上限で、性能劣化と引き換えに決めた値。** 最初の版は配下を
/// すべて正確に合計していて、Rust のチェックアウトに対しては 14GB のビルド成果物を
/// 歩くことになった ── リクエストハンドラの中で 6 秒、そのあいだエンジン全体が
/// 待たされる。そのディレクトリを見つけることだけが目的のキー操作で、画面は
/// 固まったように見えたはずだった。
///
/// 2 万件は、数値が判断を変える境目をとうに超えている。これだけのファイルを
/// 数えないと分からないものが、結果として小さかった、ということは起こらない。
pub const SIZE_ENTRY_CAP: usize = 20_000;

/// `dir` 以下の通常ファイルの合計バイト数と、それが下限かどうか。再帰ではなく
/// 反復で書いてあるので、深いディレクトリでもスタックを潰さない。シンボリック
/// リンクは辿らない。
///
/// 自分の [`SIZE_ENTRY_CAP`] に加えて、調査全体で共有する `budget` からも消費する。
/// 1 つのディレクトリが調査全体を食い潰してはいけないし、調査全体がキー操作の
/// 応答時間を食い潰してもいけない。
///
/// **打ち切った合計を、合計として見せることはない。** 黙って数えるのをやめた
/// サイズは、正しい数値の顔をした誤った数値であり、それで順位付けすると、いちばん
/// 大きいディレクトリが一覧の真ん中に来る。
fn subtree_size(dir: &Path, cancel: &AtomicBool, budget: &mut usize) -> (u64, bool) {
    if *budget == 0 {
        return (0, true);
    }
    let mut total = 0u64;
    let mut seen = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return (total, true);
        }
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            seen += 1;
            *budget = budget.saturating_sub(1);
            if seen > SIZE_ENTRY_CAP || *budget == 0 {
                return (total, true);
            }
            let ft = e.file_type();
            if ft.as_ref().map(|t| t.is_symlink()).unwrap_or(false) {
                continue;
            }
            if ft.as_ref().map(|t| t.is_dir()).unwrap_or(false) {
                stack.push(e.path());
            } else {
                total += e.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    (total, false)
}

/// `now` から見て何日前か（丸 1 日単位）。時刻が不明、または未来のときは `None`
/// （時計のずれ、展開したアーカイブなど）── 負の日数は、実際の状況ではなく
/// 一覧のバグとして読まれてしまう。
pub fn age_days(modified: Option<SystemTime>, now: SystemTime) -> Option<u64> {
    let m = modified?;
    now.duration_since(m).ok().map(|d| d.as_secs() / 86_400)
}

/// バイト数を人が読む形に。呼び出し側ではなくここに置いてあるのは、3 つの
/// プロンプトが同じ列を表示するからで、「1.5G」の意味が食い違ってはいけない。
pub fn brief_size(bytes: u64) -> String {
    const UNITS: [(u64, &str); 4] =
        [(1 << 30, "G"), (1 << 20, "M"), (1 << 10, "K"), (1, "B")];
    for (scale, tag) in UNITS {
        if bytes >= scale {
            // 10 未満は小数第 1 位まで、10 以上は整数。「9.4G」も「512M」も、
            // その数値が持てる精度としてはこれで上限。
            let v = bytes as f64 / scale as f64;
            if v >= 10.0 || scale == 1 {
                return format!("{}{tag}", v.round() as u64);
            }
            // ただし、何も言っていない小数点は付けない。「4.0G」は 2 文字使って
            // 「ちょうど 4 です」と言うが、実際はちょうど 4 ではない。
            let one = format!("{v:.1}");
            return match one.strip_suffix(".0") {
                Some(whole) => format!("{whole}{tag}"),
                None => format!("{one}{tag}"),
            };
        }
    }
    "0B".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sandbox() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        fs::create_dir_all(r.join("src/deep/deeper")).unwrap();
        fs::create_dir_all(r.join("node_modules/pkg")).unwrap();
        fs::create_dir(r.join(".git")).unwrap();
        fs::write(r.join("README.md"), vec![b'x'; 100]).unwrap();
        fs::write(r.join("src/main.rs"), vec![b'x'; 200]).unwrap();
        fs::write(r.join("src/deep/deeper/buried.rs"), vec![b'x'; 400]).unwrap();
        fs::write(r.join("node_modules/pkg/index.js"), vec![b'x'; 8000]).unwrap();
        fs::write(r.join(".git/HEAD"), vec![b'x'; 20]).unwrap();
        d
    }

    fn find<'a>(s: &'a Survey, rel: &str) -> &'a Row {
        s.rows.iter().find(|r| r.rel == rel).unwrap_or_else(|| panic!("no row {rel}: {:?}", s.rows.iter().map(|r| &r.rel).collect::<Vec<_>>()))
    }

    /// ディレクトリは*配下*の合計を伴って届く。この機能の成否を分ける数値が
    /// それ。以前は空のまま届いていた。
    #[test]
    fn ディレクトリは配下の合計を持って返る() {
        let d = sandbox();
        let s = survey(d.path(), Limits::default(), &AtomicBool::new(false));
        assert_eq!(find(&s, "node_modules").size, 8000, "the package, not the folder entry");
        assert_eq!(find(&s, "src").size, 600, "200 here and 400 buried");
    }

    /// …深さの上限より下の部分も含めて。上限が制限するのは列挙であって集計では
    /// ない。巨大であることだけが重要なフォルダは、中身が表示されない場合でも
    /// そう伝わる必要がある。
    #[test]
    fn 深さの上限はサイズを小さくしない() {
        let d = sandbox();
        let shallow = survey(
            d.path(),
            Limits { depth: 1, ..Limits::default() },
            &AtomicBool::new(false),
        );
        assert!(!shallow.rows.iter().any(|r| r.rel.contains('/')), "one level listed");
        assert_eq!(find(&shallow, "src").size, 600, "still counted to the bottom");
        assert!(shallow.unopened >= 2, "and it says which doors it did not open");
    }

    /// ドットで始まるディレクトリは不要物ではない。整理系の機能に `.git` を
    /// 候補として見せてはいけない。
    #[test]
    fn 隠しファイルを見るかどうかは選べる() {
        let d = sandbox();
        let without = survey(d.path(), Limits::default(), &AtomicBool::new(false));
        assert!(!without.rows.iter().any(|r| r.rel.starts_with(".git")));
        let with = survey(
            d.path(),
            Limits { hidden: true, ..Limits::default() },
            &AtomicBool::new(false),
        );
        assert!(with.rows.iter().any(|r| r.rel == ".git"));
    }

    /// **報告されない上限は嘘である。** 「見つからなかった」と「見た範囲では
    /// 見つからなかった」は、区別できなければならない。
    #[test]
    fn 上限に達した調査は_そう報告する() {
        let d = sandbox();
        let s = survey(
            d.path(),
            Limits { rows: 2, ..Limits::default() },
            &AtomicBool::new(false),
        );
        assert_eq!(s.rows.len(), 2);
        assert_eq!(s.stopped_at, Some(1), "it stopped inside the first level");
        assert_eq!(s.whole_to(), None, "so no level is known to be complete");
        assert!(s.partial());
        // テスト用ディレクトリのいちばん下まで届く深さ。既定の深さ 3 では、この
        // ディレクトリ自体に開かれないまま残る場所がある（`src/deep/deeper`）。
        // `partial()` はそれを報告する ── これは仕様どおりの動作で、テストデータの
        // 不備ではない。
        let whole = survey(
            d.path(),
            Limits { depth: 6, ..Limits::default() },
            &AtomicBool::new(false),
        );
        assert!(!whole.partial(), "reaching the bottom claims nothing extra");
        assert_eq!(whole.whole_to(), Some(4), "and it knows how deep it went");

        // ある階層の*後*で止まったなら、その階層は完全に列挙できている。深さで
        // 表現しているのは、この一文を言えるようにするため。
        let two = survey(
            d.path(),
            Limits { rows: 6, depth: 6, ..Limits::default() },
            &AtomicBool::new(false),
        );
        // 6 件は最上位の階層すべて（3 件）とその 1 つ下すべて（3 件）。7 件目は
        // 深さ 3 の最初の 1 件になるはずだった。
        assert_eq!(two.stopped_at, Some(3));
        assert_eq!(two.whole_to(), Some(2), "both levels above it are complete");
    }

    #[test]
    fn 経過日数が負にならない() {
        let now = SystemTime::now();
        assert_eq!(age_days(Some(now - Duration::from_secs(86_400 * 3)), now), Some(3));
        assert_eq!(age_days(Some(now + Duration::from_secs(86_400)), now), None, "clock skew");
        assert_eq!(age_days(None, now), None);
    }

    #[test]
    fn サイズが人の読める形で出る() {
        assert_eq!(brief_size(0), "0B");
        assert_eq!(brief_size(999), "999B");
        assert_eq!(brief_size(1536), "1.5K");
        assert_eq!(brief_size(4 << 30), "4G", "no decimal point that says nothing");
        assert_eq!(brief_size(20 * (1 << 20)), "20M");
        assert_eq!(brief_size(3 * (1 << 30) + (1 << 29)), "3.5G");
    }
}
