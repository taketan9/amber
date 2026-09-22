//! ノートとは、自分自身について少しだけ書いてある Markdown ファイルのこと。
//!
//! ノート機能の全体が 1 つの決定の上に乗っている ── **ノートはただのファイルで、
//! データベースは無い。** ほかのことはすべてそこから導かれる:
//!
//!   * OneNote からの移行は、ファイルを書くスクリプトであって、取り込み形式ではない
//!   * SharePoint のライブラリも、同期した OneDrive のフォルダも、Dropbox の
//!     フォルダも、何も足さずにそのままノートのフォルダになる
//!   * crmaine が索引を作れる。ディスク上のテキストだから
//!   * 囲い込みが無い。出口は `ls`
//!
//! ノートが自分自身について持っている情報は YAML の front matter に入れる。
//! 静的サイトジェネレータもノートアプリも、既に読める書き方:
//!
//! ```text
//! ---
//! title: 移行の段取り
//! tags: [onenote, 2026]
//! created: 2026-09-02
//! ---
//! # 移行の段取り
//! ```
//!
//! **このモジュールを core に置いているのは意図的。** Electron は iOS で動かないので、
//! iPhone 版は 3 つ目のフロントエンドになり、共有できるのはここにあるものだけ ──
//! 純粋な Rust で、ファイルを読む以外の I/O も UI も持たない。ノートの判断を
//! ペインやレンダラに置くのは今日は安上がりで、ちょうど一度だけ高くつく。
//!
//!
//! **ここで読む YAML は意図的に部分集合。** `key: value` と、`[a, b]` または
//! `- a` の行で書かれたリストだけ。アンカー、入れ子のマップ、複数行のスカラー、
//! フローマップは解釈しない ── それらを使ったノートは front matter をテキストの
//! まま残し、タグ無しと答えるだけにする。YAML の 5 分の 1 のために書いた
//! パーサーが、残りの 5 分の 4 を推測するよりよい。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// ノートの front matter と、本文の開始位置。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Front {
    /// `key: value` の組。順序は安定させる（ソートしてあるので、同じファイルを
    /// 2 回読んで結果が変わることはない）。
    pub fields: BTreeMap<String, String>,
    /// `tags:` をリストとして読んだもの。2 通りの書き方のどちらでも。
    pub tags: Vec<String>,
    /// そのブロックが占めた行数。区切り行を含む。無ければ `0` ── そちらが
    /// 普通で、エラーにしてはいけない。
    pub lines: usize,
}

impl Front {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }
}

/// ファイルの先頭から front matter を読み取る。
///
/// ブロックとして数えるのは、いちばん先頭にあり、かつ閉じている場合だけ。文書の
/// 途中にある `---` は水平線であり、先頭にあって閉じていないものは、たまたま
/// 水平線で始まる文書。どちらかを front matter として扱えば、誰かのノートの
/// 冒頭を黙って飲み込むことになる。
pub fn front(lines: &[String]) -> Front {
    let mut out = Front::default();
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return out;
    }
    let Some(end) = lines.iter().skip(1).position(|l| {
        let t = l.trim_end();
        t == "---" || t == "..."
    }) else {
        return out;
    };
    let end = end + 1;
    out.lines = end + 1;
    let mut list_key: Option<String> = None;
    for raw in &lines[1..end] {
        let line = raw.trim_end();
        // `  - value` は、そのリストを開始したキーの続き。
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            if let Some(k) = &list_key {
                if k == "tags" {
                    out.tags.push(unquote(item.trim()).to_string());
                }
            }
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let key = k.trim().to_ascii_lowercase();
        let val = v.trim();
        if val.is_empty() {
            list_key = Some(key);
            continue;
        }
        list_key = None;
        if key == "tags" {
            out.tags = split_list(val);
            continue;
        }
        out.fields.insert(key, unquote(val).to_string());
    }
    out
}

/// `[a, b]` または `a, b` → その要素。引用符は外し、空は落とす。
fn split_list(v: &str) -> Vec<String> {
    let inner = v.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|x| unquote(x.trim()).to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// 一覧に表示するのに必要な、ノート 1 件ぶんの情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub path: PathBuf,
    /// front matter の `title`、無ければ最初の見出し、それも無ければ拡張子を
    /// 除いたファイル名。空にはしない ── 空白の並びは一覧ではない。
    pub title: String,
    /// 本文の先頭数行を 1 行に均したもの。一覧の 2 行目に使う。見出し・コード
    /// ブロック・front matter は除く。それらはノートが*何でできているか*を
    /// 言うもので、何についてのノートかを言うものではない。
    pub excerpt: String,
    /// **検索用のテキストだけ** ── 見出しと、表のセルの中身（依頼 606・本人が決めた）。
    ///
    /// `excerpt` には入れない。あれは一覧の二行目に出す**一行の要約**で、
    /// 見出しも表も「何でできているか」であって「何について書いたか」では
    /// ないから外してある ── その判断は変えない。
    ///
    /// **けれど、探せないのは別の話だった。** `## 来期の見通し` と書いた
    /// 小見出しや、`| ＣＰＵ | ８コア |` のセルの中身は、書いた本人にとっては
    /// まぎれもなく「そのノートに書いたこと」で、探して出てこないほうが驚く。
    /// 表示するテキストと検索対象のテキストを分ければ、一覧の行は短いまま検索できる。
    pub findable: String,
    pub tags: Vec<String>,
    /// front matter に `updated` があればそれ、無ければファイルの mtime を
    /// epoch からの秒数で。書式化は描く側の仕事。
    pub updated: Option<u64>,
    /// front matter に `created` があればそれ、無ければファイル自身の作成時刻。
    /// **`updated` とは別の問い** ──「いつ始めたか」と「最後にいつ触ったか」は、
    /// 一覧の中でノートを別の場所に置くし、どちらも人が探す手がかりになる。
    ///
    pub created: Option<u64>,
    pub bytes: u64,
    /// お気に入りかどうかと、**どのお気に入りフォルダに入っているか** ── `Some("")` は
    /// the top of the favourites, `Some("買い物/週次")` is a shelf inside it.
    ///
    /// お気に入りはノートの*2 つ目*の居場所であって、移動ではない。ノートは書かれた
    /// フォルダに残り、`star` はそれがほかにどこへ出るかを言う。ここが整理との
    /// 違いのすべてで、この情報がどこかの一覧ではなくノート自身に乗っている
    /// 理由でもある ── 移動・改名・同期されたノートは、お気に入りの居場所を
    /// 一緒に持っていく。
    ///
    /// Written as `star: true` or `star: 買い物`. `pinned: true` is still
    /// として読む。これができる前に書かれたノートがそう書いているため。
    pub star: Option<String>,
}

/// ノートを 1 件読む。見るのはファイルの先頭だけ ── 200 件の一覧を描くのに
/// 200 個のファイルを丸ごと読んではいけない。
/// ノートの**頭だけ**読む。
///
/// **最後まで読まない。** 一覧も月の表も見ているのは前書きと数行で、
/// 一万二千行のノートを丸ごと読んでから頭を切り出すのは、ノートが増える
/// ほど効いてくる（依頼 470 ── 二万本で測った）。
///
/// UTF-8 でないノートだけ、文字コードを見る側（`text::read`）で読み直す
/// ── ほとんどのノートは UTF-8 なので、速い経路はそのまま（依頼 429）。
pub fn head(path: &Path, head_lines: usize) -> Option<Vec<String>> {
    use std::io::BufRead;
    let want = head_lines.max(8);
    let lines: Vec<String> = match std::fs::File::open(path) {
        Ok(f) => {
            let mut out = Vec::with_capacity(want.min(64));
            let mut bad = false;
            for line in std::io::BufReader::new(f).lines().take(want) {
                match line {
                    Ok(l) => out.push(l),
                    Err(_) => { bad = true; break; }
                }
            }
            if bad {
                crate::text::read(path).ok()?.lines.into_iter().take(want).collect()
            } else {
                out
            }
        }
        Err(_) => crate::text::read(path).ok()?.lines.into_iter().take(want).collect(),
    };
    // **BOM は本文の文字ではない。** 残すと 1 行目が `\u{feff}---` になり、front matter が
    // 前書きに見えない ── 題も `tags:` も読まれず、前書きぜんぶが本文の
    // 書き出しとして一覧に出る（実際に出た）。
    let mut lines = lines;
    if let Some(first) = lines.first_mut() {
        if let Some(cut) = first.strip_prefix('\u{feff}') {
            *first = cut.to_string();
        }
    }
    Some(lines)
}

pub fn read(path: &Path, head_lines: usize) -> Option<Note> {
    // **UTF-8 でないノートも、一覧に出す**（依頼 429）。
    //
    // ここは先頭だけ読む速い経路で、`read_to_string` は UTF-8 でなければ
    // 何も返さない ── そのまま素通りさせていたので、**Shift_JIS で
    // 書かれたノートが amber から丸ごと消えていた**。開けるのに一覧に
    // 無い、という形（`read` op は通る）で、どこから探せばいいのかが
    // 画面のどこにも出ない。
    //
    let lines = head(path, head_lines)?;
    from_head(path, &lines)
}

/// 読み取った先頭の行から、一覧に出す形を作る。
///
/// `read` と月の表が同じ行を使い回すために分けてある ── 分けていなかった
/// 頃は、同じファイルを二度読んでいた（依頼 470）。
pub fn from_head(path: &Path, lines: &[String]) -> Option<Note> {
    let lines = lines.to_vec();
    let meta = std::fs::metadata(path).ok();
    let f = front(&lines);
    let body = &lines[f.lines.min(lines.len())..];
    // 題は **`title:` → 最初の見出し → 書き出しの一行 → ファイル名**。
    //
    // 書き出しの一行を挟むのは、雑なメモのため ── 「牛乳」とだけ書いた
    // ノートに題を付けさせるのは、書くことより多い。ファイル名まで落ちる
    // のは**中身がまだ何も無いとき**だけで、そのときは他に呼びようがない。
    let from_line = f
        .get("title")
        .map(str::to_string)
        .filter(|t| !t.trim().is_empty())
        .is_none()
        && heading(body).is_none();
    let title = f
        .get("title")
        .map(str::to_string)
        .filter(|t| !t.trim().is_empty())
        .or_else(|| heading(body))
        .or_else(|| first_line(body))
        .or_else(|| {
            // **amber が付けた符号は、題ではない。** 名前を思いつけなかった
            // ときの `2026-09-06 13-07-22` を題として出すと、書いた人が
            // 一度も言っていない名前が画面に出る ── 依頼 174 で前書きから
            // 追い出したものが、ファイル名から戻ってきていた。人が名づけた
            // ファイル名（`買うもの.md`）は、中身が空でも題でいい。
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !made_up_name(s))
        })
        .unwrap_or_default();
    let updated = f
        .get("updated")
        .and_then(date_secs)
        .or_else(|| {
            meta.as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
        });
    // 作成時刻を持たないファイルシステムでは、何も無しではなく mtime に
    // フォールバックする。日付がまったく無いノートは、日付でまとめた一覧から
    // 落ちてしまい、消えたノートのように見える。
    let created = f.get("created").and_then(date_secs).or_else(|| {
        meta.as_ref()
            .and_then(|m| m.created().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .or(updated)
    });
    Some(Note {
        path: path.to_path_buf(),
        title,
        // **題が書き出しの一行から来たなら、その行は二行目に出さない。**
        // 同じ文字列が 2 段に並ぶと、1 行のノートが 2 行に見える。
        excerpt: if from_line { excerpt(&body[first_at(body)..]) } else { excerpt(body) },
        findable: findable(body),
        star: star(&f),
        tags: f.tags,
        updated,
        created,
        bytes: meta.map(|m| m.len()).unwrap_or(0),
    })
}

/// お気に入りなら、お気に入りの中のどこに置かれているか。
///
/// `true`/`yes`/`1` は「はい」の 3 通りの書き方。どのアプリがどれを求めるか
/// 誰も覚えていないため。それ以外はフォルダの名前。`false` は「お気に入りでは
/// ない」と明示的に書いてあるノート。
fn star(f: &Front) -> Option<String> {
    let raw = f
        .fields
        .get("star")
        .or_else(|| f.fields.get("favorite"))
        .or_else(|| f.fields.get("pinned"))?;
    let v = raw.trim();
    match v.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Some(String::new()),
        "false" | "no" | "0" | "" => None,
        _ => Some(v.trim_matches(['"', '\'']).to_string()),
    }
}

/// ノートが見出しで始まっているなら、その最初の `# 見出し`。
fn heading(body: &[String]) -> Option<String> {
    body.iter()
        .map(|l| l.trim())
        .find(|l| l.starts_with('#'))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .filter(|t| !t.is_empty())
}

/// 前書きも見出しも無いノートの題は、**書き出しの一行**。
///
/// 「牛乳」とだけ書きたいときに題を付けさせるのは、書くことより多い。
/// そして題を書かなかったノートが**日付で並ぶ**のは、書いた人が一度も
/// 言っていない名前が一覧を埋めるということ。
///
/// 行頭の記号は外す ── `- 牛乳` のタイトルは「牛乳」。強調やコードの記号も外す
/// （題は読むもので、記法ではない）。長い一行は切る ── 段落をそのまま
/// 題にすると、一覧の一行をそれだけで食い尽くす。
fn first_line(body: &[String]) -> Option<String> {
    let mut fenced = false;
    for line in body {
        let t = line.trim();
        if t.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        // コードブロックの中も人が書いた文字ではあるが、タイトルにはならない（`fn main() {`）。
        if fenced || t.is_empty() || crate::markdown::is_rule(t) {
            continue;
        }
        let bare = match crate::markdown::list_item(t) {
            Some((_, rest, _)) => match crate::markdown::task_item(&rest) {
                Some((_, r)) => r,
                None => rest,
            },
            None => t.strip_prefix('>').map(|r| r.trim_start().to_string())
                .unwrap_or_else(|| t.to_string()),
        };
        let flat: String = crate::markdown::inline(&bare).iter().map(|i| i.text()).collect();
        let flat = flat.trim();
        if flat.is_empty() {
            continue;
        }
        return Some(clip(flat, 60));
    }
    None
}

/// `first_line` が採った行の、次の行。
fn first_at(body: &[String]) -> usize {
    let mut fenced = false;
    for (n, line) in body.iter().enumerate() {
        let t = line.trim();
        if t.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || t.is_empty() || crate::markdown::is_rule(t) {
            continue;
        }
        return (n + 1).min(body.len());
    }
    body.len()
}

/// 長い文字列を切る。**文字数で数える** ── バイト数で切るとマルチバイト文字が割れる。
fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n).collect::<String>() + "…"
}

/// 本文 1 行を、1 行に均したもの。
fn excerpt(body: &[String]) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for line in body {
        let t = line.trim();
        if t.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || t.is_empty() || t.starts_with('#') || t.starts_with("---") {
            continue;
        }
        // A table is not a sentence. `| 名前 | 状態 |` in the one line meant
        // 思い出させるための行に出しても、「表がある」ことしか伝わらない ── それは
        // 見れば分かるし、中身については何も言っていない。
        if t.starts_with('|') {
            continue;
        }
        // **折りたたみのタグも、文ではない**（依頼 619）。`<details>` と
        // `</details>` は「ここから畳んである」という形の話で、一覧の
        // 二行目に山括弧が並ぶ（実機で出た）。**見出し（`<summary>`）は
        // 残す** ── あれは畳んだ中身に人が付けた名前で、文として読める。
        if crate::markdown::is_details_open(t) || t == "</details>" {
            continue;
        }
        let t = match t.strip_prefix("<summary>").and_then(|r| r.strip_suffix("</summary>")) {
            Some(inner) => inner,
            None => t,
        };
        // 画像は文ではない。`![](attachments/note-1788450324680.jpg)` は 40 文字の
        // ファイル名で、何についてのノートかを思い出させるための行がそれで埋まる。
        // スクリーンショットで始まるノートは、それ以外に何も表示されていなかった。
        //
        let t = strip_images(t);
        // 色も文ではない。`<span style="color:#D9822B">` は 30 文字の記法で、
        // 何についてのノートかを思い出させるための唯一の行がそれで埋まる ── しかも
        // 検索が走るのはその行なので、色を付けた語が検索に引っかからなくなる。
        //
        let t = plain(&t);
        // **Nor is the notation.** `**ここにあるのは、ただの Markdown ファイル
        // です。**` puts four asterisks in the one line meant to remind you
        // what the note is about, and `**大事**` stops being findable by
        // searching for 大事. The title already goes through `inline` for
        // exactly this reason (依頼 174); the second line was left behind.
        // `>` と箇条書きの記号も外す ── 引用もリスト項目も文であることに変わりは
        // なく、その記号は表示のためのもので、言葉ではない。
        let t = match crate::markdown::list_item(t.trim()) {
            Some((_, rest, _)) => match crate::markdown::task_item(&rest) {
                Some((_, r)) => r,
                None => rest,
            },
            None => t
                .trim()
                .strip_prefix('>')
                .map(|r| r.trim_start().to_string())
                .unwrap_or_else(|| t.trim().to_string()),
        };
        let t: String = crate::markdown::inline(&t).iter().map(|i| i.text()).collect();
        let t = t.trim();
        if t.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(t);
        if out.chars().count() >= 120 {
            break;
        }
    }
    out.chars().take(120).collect()
}

/// 見出しと、表のセルの中身を集める ── **検索のためだけに**（依頼 606）。
///
/// `excerpt` が落としているもののうち、**人が書いた言葉**はこの二つ。
/// 画像のパスとコードブロックは拾わない ── 前者はファイル名、後者は書いた言葉では
/// あるが、探し先に入れると `fn` や `const` がどのノートにも当たる。
///
/// **記号は落とす。** `## **来期**の見通し` を探す人は `来期` と打つので、
/// `inline` を通して書式を外す（`excerpt` と同じ理由・依頼 174）。
/// 表の区切り行（`| --- |`）は本文ではないので落とす。
///
/// 読むのは `read` が既に取っている頭の数十行だけ ── ファイルを二度読まない。
fn findable(body: &[String]) -> String {
    let mut out = String::new();
    let mut fenced = false;
    let mut put = |t: &str| {
        let t: String = crate::markdown::inline(t).iter().map(|i| i.text()).collect();
        let t = t.trim();
        if t.is_empty() {
            return;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(t);
    };
    for line in body {
        let t = line.trim();
        if t.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        // **畳みの見出しも探せる**（依頼 619）── 畳んだ中身に人が付けた
        // 名前なので、見出しと同じ扱い。
        if let Some(inner) = t.strip_prefix("<summary>").and_then(|r| r.strip_suffix("</summary>")) {
            put(inner.trim());
        } else if let Some(rest) = t.strip_prefix('#') {
            put(rest.trim_start_matches('#').trim());
        } else if t.starts_with('|') {
            // 区切り行（`| --- | :--: |`）は書式であって本文ではない。
            let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).collect();
            if cells.iter().all(|c| {
                !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
            }) {
                continue;
            }
            for c in cells {
                put(c);
            }
        }
    }
    out
}

/// 行から `![alt](link)` を取り除く。代替テキストがあれば残す。
///
/// 対象は画像だけ ── ただの `[テキスト](link)` は人が書いた言葉で、抜粋の
/// 中でもそのまま読める。
fn strip_images(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("![") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        // `![alt](link)` ── 両方揃っている必要がある。そうでなければ、たまたま
        // 感嘆符で始まるテキストにすぎない。
        let Some(close) = after.find(']') else { break };
        let tail = &after[close + 1..];
        if !tail.starts_with('(') {
            out.push_str(&rest[at..at + 2]);
            rest = after;
            continue;
        }
        let Some(end) = tail.find(')') else { break };
        // 代替テキストは書いた人が画像に付けた呼び名なので抜粋に入れる。
        // ファイル名は入れない。
        out.push_str(&after[..close]);
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// `2026-09-02` または `2026-09-02T10:00:00` → epoch からの秒数。
///
/// タイムゾーンを使わず、暦の日数から計算する。ノートの `updated` は人が
/// 打った日付であり、その何時のことか、誰のタイムゾーンかを知っているふりを
/// するのは、ありもしない精度をでっち上げることになる。
fn date_secs(s: &str) -> Option<u64> {
    let d = s.trim();
    let d = d.split(['T', ' ']).next()?;
    let mut it = d.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let day: i64 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // Howard Hinnant の days_from_civil。
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days.checked_mul(86_400)?).ok()
}

/// Windows はデバイス名を 11 個予約していて、拡張子が何であれファイルには
/// その名前を付けられない。「CON」というタイトルのノートは架空の例ではなく、
/// 人が実際に書く略語。
pub const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// タイトルを、動作対象のどのファイルシステムでも受け付けるファイル名に変える。
///
/// slug ではない。ここのタイトルは日本語であることが多く、ASCII に落とすと
/// ほとんどのノートが `.md` という名前になる。取り除くのはファイルシステムが
/// 受け付けないものだけ ── Windows が予約している 9 文字、制御文字、そして
/// エクスプローラーが黙って削る末尾のドットと空白。
///
/// 上限は**文字数で、文字境界で切る**が、値はバイト数から決めた ── 日本語
/// 60 文字は 180 バイトで、ext4・APFS・NTFS がいずれも止まる 255 の内側に
/// 十分収まる。
pub fn file_stem(title: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in title.trim().chars() {
        let bad = matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            || c.is_control();
        if bad {
            gap = true;
            continue;
        }
        // 受け付けない文字が続いたら `-` 1 つにまとめる。先頭には置かない ──
        // `?? notes` は `- notes` ではなく `notes` になるべき。
        if gap && !out.is_empty() {
            out.push('-');
        }
        gap = false;
        if out.chars().count() >= 60 {
            break;
        }
        out.push(c);
    }
    let out = out.trim_matches([' ', '.', '\u{3000}']).to_string();
    if out.is_empty() {
        return String::new();
    }
    // `CON.md` も Windows からは CON。`con` も同じ。
    let head = out.split('.').next().unwrap_or(&out).to_ascii_uppercase();
    if RESERVED.contains(&head.as_str()) {
        return format!("_{out}");
    }
    out
}

/// ノートを検索するための 1 行 ── タイトル、タグ、本文の冒頭。
///
/// 一覧の絞り込みはファイル名に対して効くので、中身に合わせた名前を付けた
/// ノートしか見つからない。それが成り立つのは amber が作ったノートだけで、
/// 取り込んだページは中にタイトルを持つ `page-0012.md` だし、タグはそもそも
/// ファイル名に入らない。
///
/// Tags keep their `#`, so `#仕事` narrows to the tag and `仕事` also finds
/// 単に言及しているだけのものより上に出す。小文字化は絞り込みのキー入力ごと
/// ではなく、ここで一度だけ行う。
pub fn haystack(n: &Note) -> String {
    let mut s = String::with_capacity(n.title.len() + n.excerpt.len() + 16);
    s.push_str(&n.title);
    for t in &n.tags {
        s.push_str(" #");
        s.push_str(t);
    }
    s.push(' ');
    s.push_str(&n.excerpt);
    // 見出しと表のセルは**検索できるだけ**（一覧の 2 行目には出さない・依頼 606）。
    if !n.findable.is_empty() {
        s.push(' ');
        s.push_str(&n.findable);
    }
    s.to_lowercase()
}

/// ノートと、走査したフォルダの中でのその位置。
pub struct Found {
    /// 走査したフォルダからの相対パス。区切りは `/`。
    pub rel: String,
    pub note: Note,
}

/// `dir` 配下のすべての Markdown ノートと、走査自体の結果。
///
/// エンジン側ではなくここに置いているのは、呼び出し側が 2 つあるから ──
/// デスクトップ版はパイプ越しに、iPhone は C ABI 越しに問い合わせる ── そして
/// 「何をノートと見なすか」を 2 回書けば、答えは 2 つに分かれていく。規則が
/// 中身のすべて ── ディレクトリはノートではない、`.md`/`.markdown` はノート、
/// タイトルと抜粋を知るには先頭 60 行で足りる。
pub fn list(
    dir: &std::path::Path,
    limits: crate::survey::Limits,
    stop: &std::sync::atomic::AtomicBool,
) -> (Vec<Found>, crate::survey::Survey) {
    let found = crate::survey::survey(dir, limits, stop);
    let mut out = Vec::new();
    for r in &found.rows {
        if r.is_dir {
            continue;
        }
        let md = r
            .path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
            .unwrap_or(false);
        if !md {
            continue;
        }
        if scratch(&r.path) {
            continue;
        }
        let Some(note) = read(&r.path, 60) else { continue };
        out.push(Found { rel: r.rel.clone(), note });
    }
    (out, found)
}

/// **書きかけの置き土産か。** 人の書いたノートではないので、一覧に出さない。
///
/// 隠しファイル（`.` で始まるもの）は歩く側が既に落としている ── iCloud の
/// まだダウンロードされていないプレースホルダ（`.名前.md.icloud`）も、macOS が作る
/// LibreOffice の錠（`.~lock.名前.md#`）もそこで落ちる。**落ちないのは
/// Windows 側の作法**で、Office と同じ `~$` で始まる置き土産は隠しに
/// ならない ── 会社の端末で同じフォルダを開いた人の一覧に、`x` という
/// 題のノートが一本増えていた。
///
/// **同期がぶつかった控えは落とさない**（`段取り (競合コピー…).md`）──
/// あれは人が書いた内容で、消えていいものではない。
fn scratch(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("~$"))
}

/// `dir` の中にノートを作り、どこに置いたかを返す。
///
/// 「いま空いている名前」であって、「少し前に空いていた名前」ではない ──
/// `create_new` は確認と書き込みのあいだにファイルができていれば失敗する。
/// 共有フォルダを 2 人で使う状況こそ、この仕組み全体が存在する理由。
/// エンジンと共有しているのは [`list`] と同じ理由。
pub fn create(
    dir: &std::path::Path,
    title: &str,
    today: &str,
    now: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let (name, body) = new_note(title, today, now);
    let at = fresh_file(dir, name.trim_end_matches(".md"))?;
    std::fs::write(&at, body.as_bytes())?;
    Ok(at)
}

/// **同じ中身のノートをもう一つ。**
///
/// 下書きの型を持っている人が、毎回それを開いて全部写しているのを見た
/// ── 写すのは道具の仕事で、人の仕事ではない。
///
/// 中身はそのまま写すが、`created` は**今日**にする（`updated` は落とす）
/// ── 写しは今日できたもので、元の日付を名乗ると並べ替えが嘘をつく。
/// 題は写したまま: Markdown の中の題は書いた人の言葉で、amber が
/// 「（コピー）」を書き足す筋合いは無い（ノートはただの Markdown）。
///
/// 画像は写さない ── `attachments/` は同じフォルダの中で、二つのノートが
/// 同じファイルを指すだけ。ノートを消しても画像は残る（`delete` は `.md` しか
/// 消さない）ので、片方を消してもう片方の画像が欠ける、は起きない。
///
/// `into` を渡せば、そこへ写す（テンプレートから作るとき・依頼 417）──
/// **コピーの仕組みは 1 つ**にする。「テンプレートから作る」を別の経路にすると、`created`
/// を今日にするのを片方だけ直した日に、二つの作り方が食い違う。
pub fn duplicate(
    at: &std::path::Path,
    into: Option<&std::path::Path>,
    today: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let dir = into.unwrap_or_else(|| at.parent().unwrap_or(std::path::Path::new(".")));
    let file = crate::text::read(at)?;
    let text = file.lines.join("\n");
    let text = set_field(&text, "created", Some(today));
    let text = set_field(&text, "updated", None);
    let stem = at
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| today.to_string());
    let made = fresh_file(dir, &stem)?;
    std::fs::write(&made, text.as_bytes())?;
    Ok(made)
}

/// まだ無い名前を一つ ── `名前.md`、埋まっていれば `名前-2.md`、…。
///
/// **上書きしない。** 同じ名前で作りにいく経路が 2 つある（新規とコピー）ので、
/// 空いているかを見てから開くのではなく、`create_new` で取りにいく。
fn fresh_file(dir: &std::path::Path, stem: &str) -> anyhow::Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut at = dir.join(format!("{stem}.md"));
    let mut n = 2;
    loop {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&at) {
            Ok(_) => return Ok(at),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n <= 99 => {
                at = dir.join(format!("{stem}-{n}.md"));
                n += 1;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// ノートの隣に画像を置き、本文に何と書けばよいかを返す。
///
/// 置き場所はノートの隣の `attachments/`。ノートごとのフォルダでも
/// データベースでもない ── ノートのフォルダがただのフォルダであることが
/// この仕組みの要点で、Mac から見ても、エクスプローラーから見ても、
/// iPhone の「ファイル」から見ても、期待どおりに見えるべきだから。
///
/// 名前にはノート名と時刻を入れる。**連番にはしない** ── そのフォルダは
/// そこに添付されたほかのものとも共有されるので、連番はいつか既にある
/// 名前を選んでしまう。
///
/// エンジンと共有しているのは、iPhone が写真を添付し、デスクトップ版が
/// スクリーンショットを貼り付けるから。両者は同じ場所へ同じ形の名前で
/// 着地しなければならない ── そうでないと、両方から書かれたノートフォルダが
/// たまたま重なった 2 つのフォルダのように見える。
pub fn attach(note: &std::path::Path, bytes: &[u8], ext: &str) -> anyhow::Result<String> {
    if bytes.is_empty() {
        anyhow::bail!("画像が空です");
    }
    let Some(dir) = note.parent() else {
        anyhow::bail!("そのノートの保存場所が分かりません")
    };
    let ext = match ext.trim().trim_start_matches('.') {
        "" => "png".to_string(),
        e => e.to_ascii_lowercase(),
    };
    let at = dir.join("attachments");
    std::fs::create_dir_all(&at)?;
    let stem = note
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "note".into());
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let name = format!("{}-{stamp}.{ext}", file_stem(&stem));
    std::fs::write(at.join(&name), bytes)?;
    // 相対パスで、区切りはスラッシュ。これは Markdown のリンクに入るもので、
    // URL であって Windows のパスではない。
    Ok(format!("attachments/{name}"))
}

/// ノートの構成要素 1 つを、描画できる形で表したもの。
///
/// 木ではなくブロックの列。ノートは上から下へ読まれるし、これを受け取る
/// 側 ── iPhone の View の並び、デスクトップ版、タブレットで何になるにせよ ──
/// はどれも列としてレイアウトする。入れ子が要るもの（引用の中のリストなど）は、
/// ノートの中では十分に稀なので、捨てる価値がある。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// `line` は**ファイルの行番号**（前書きを含めて数えた 0 起点）。
    /// 目次から飛ぶのに要る ── チェックボックスと同じ理由で、何番目の見出しかを
    /// 数えると、前書きのあるノートでずれる。
    Heading { level: u8, text: String, line: usize },
    Paragraph(String),
    /// 項目 1 つ。`text` はインラインの記法を保持したまま渡し、描く側が処理する。
    Bullet(String),
    /// `- [ ] 牛乳`。元の行番号を持つ。チェックボックスに対してできる唯一
    /// 有用なことは押すことで、押すときには*どれ*かを言う必要がある ── 呼び出し
    /// 側が、次の編集でずれる添え字を計算し直さずに済むように。
    ///
    Check { done: bool, text: String, line: usize },
    Numbered { n: u32, text: String },
    Quote(String),
    /// 中の空行も含めてそのまま。`lang` は空のことがある。
    Code { lang: String, text: String },
    /// 単独の行に置かれた `![alt](link)` ── ブロックになる画像はこれだけ。
    /// 文の中にあるものは、書かれた場所のまま文の中に残る。
    Image { alt: String, link: String },
    /// 表。`align` はヘッダーの列ごとに 1 つ持つ。
    ///
    /// **iPhone 側ではなくここで処理する。** これが無かった頃、iPhone は行を
    /// 1 つの段落として扱っていた。
    /// 1 つの文として扱っていた ── 誰も解析しなかったときの表の見た目そのもの。
    /// 行はヘッダーより短いことも長いこともある。それをどう扱うかは描く側が
    /// 決めるが、渡されるのは事実そのもの。
    Table {
        head: Vec<String>,
        align: Vec<crate::markdown::Align>,
        rows: Vec<Vec<String>>,
    },
    /// `> [!NOTE]` ── GitHub と同じ 5 種類だけ。
    ///
    /// 中身は**段落**であってブロックではない。ノートの注記に入るのは実際には文
    /// だけだし、入れ子のブロック木にすると、存在するすべての描画側がそれを
    /// 理解しなければならない ── デスクトップ版は HTML から自前で組み立てる。
    Alert { kind: String, body: Vec<String> },
    Rule,
}

/// ノートを描画単位に分割する。front matter は飛ばす。
///
/// **Swift 側ではなくここに置く。** iPhone 側に書いたレンダラはテストが
/// 届かないレンダラになるし、「何が見出しか」はまさに解釈がずれていく類の
/// 問い。`Heading` をどう描くかは iPhone の裁量だが、その行が見出し*である*
/// かどうかは違う。
///
/// front matter を除くのは、それがノートの自己説明であって本文として述べて
/// いることではないから ── タイトルとタグは既に画面に出ている。
pub fn blocks(text: &str) -> Vec<Block> {
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let start = front(&lines).lines;
    let mut out = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let mut i = start;

    fn flush(para: &mut Vec<String>, out: &mut Vec<Block>) {
        if !para.is_empty() {
            out.push(Block::Paragraph(para.join(" ")));
            para.clear();
        }
    }

    while i < lines.len() {
        let raw = &lines[i];
        let t = raw.trim();

        // フェンスは閉じるフェンスまで、無ければノートの終わりまで続く ── 閉じ忘れは
        // 誰かのミスであり、残りを丸ごと取り込むほうが、1 行ずつ段落のふりをするより
        // 親切。
        if let Some(lang) = t.strip_prefix("```") {
            flush(&mut para, &mut out);
            let lang = lang.trim().to_string();
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                body.push(lines[i].clone());
                i += 1;
            }
            i += 1; // the closing fence, or past the end
            out.push(Block::Code { lang, text: body.join("\n") });
            continue;
        }

        if t.is_empty() {
            flush(&mut para, &mut out);
            i += 1;
            continue;
        }

        // ここでの `---` は水平線であって front matter ではない。front matter は
        // このループが始まる前に先頭から取り除いてある。
        if t == "---" || t == "***" || t == "___" {
            flush(&mut para, &mut out);
            out.push(Block::Rule);
            i += 1;
            continue;
        }

        if let Some(rest) = t.strip_prefix('#') {
            let level = 1 + rest.chars().take_while(|c| *c == '#').count();
            let text = rest.trim_start_matches('#').trim();
            // 行頭の `#tag` はタグであって見出しではない ── 見出しにするのは空白で、
            // それが Markdown の規定であり、ノートアプリが取り違えてはいけないところ。
            //
            if level <= 6 && (rest.starts_with(' ') || rest.trim_start_matches('#').starts_with(' ')) {
                flush(&mut para, &mut out);
                out.push(Block::Heading { level: level as u8, text: text.to_string(), line: i });
                i += 1;
                continue;
            }
        }

        if let Some(img) = lone_image(t) {
            flush(&mut para, &mut out);
            out.push(img);
            i += 1;
            continue;
        }

        // 表 ── ヘッダー行、その下の区切り行、そして行が続くかぎりの本体。
        // 表にしているのは区切り行で、パイプが入っているだけの単独の行は
        // 誰かが書いた文にすぎない。
        if t.starts_with('|')
            && i + 1 < lines.len()
            && crate::markdown::is_table_separator(&lines[i + 1])
        {
            flush(&mut para, &mut out);
            let head = crate::markdown::split_cells(t);
            let align = crate::markdown::split_cells(&lines[i + 1])
                .iter()
                .map(|c| crate::markdown::cell_align(c))
                .collect();
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() && lines[i].trim_start().starts_with('|') {
                rows.push(crate::markdown::split_cells(lines[i].trim()));
                i += 1;
            }
            out.push(Block::Table { head, align, rows });
            continue;
        }

        // `> [!NOTE]` と、その下の引用行。
        if let Some(kind) = crate::markdown::alert_kind(t) {
            flush(&mut para, &mut out);
            i += 1;
            let mut body = Vec::new();
            let mut piece: Vec<String> = Vec::new();
            while i < lines.len() {
                let q = lines[i].trim();
                let Some(rest) = q.strip_prefix('>') else { break };
                let rest = rest.trim();
                if rest.is_empty() {
                    if !piece.is_empty() {
                        body.push(piece.join(" "));
                        piece.clear();
                    }
                } else {
                    piece.push(rest.to_string());
                }
                i += 1;
            }
            if !piece.is_empty() {
                body.push(piece.join(" "));
            }
            out.push(Block::Alert { kind, body });
            continue;
        }

        if let Some(rest) = t.strip_prefix("> ").or_else(|| t.strip_prefix('>')) {
            flush(&mut para, &mut out);
            out.push(Block::Quote(rest.trim().to_string()));
            i += 1;
            continue;
        }

        if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            flush(&mut para, &mut out);
            // チェックボックスを箇条書きより先に判定する。`- [ ] x` は両方に当てはまり、
            // 押せるほうが有用な答えだから。
            if let Some(done) = ticked(rest) {
                out.push(Block::Check {
                    done,
                    text: rest[3..].trim().to_string(),
                    line: i,
                });
            } else {
                out.push(Block::Bullet(rest.trim().to_string()));
            }
            i += 1;
            continue;
        }

        if let Some((num, rest)) = t.split_once(". ") {
            if let Ok(n) = num.parse::<u32>() {
                flush(&mut para, &mut out);
                out.push(Block::Numbered { n, text: rest.trim().to_string() });
                i += 1;
                continue;
            }
        }

        para.push(t.to_string());
        i += 1;
    }
    flush(&mut para, &mut out);
    out
}

/// 行の一部分と、それが書かれた色。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    /// 小文字の `#rrggbb`、または読み手の既定色を意味する `None`。
    pub color: Option<String>,
}

/// 行を、色付きの部分と色無しの部分に分ける。
///
/// **Markdown に色の記法は無い**ので、最も多くのツールが解釈できる書き方を
/// 1 つだけ読む ── `<span style="color:#rrggbb">…</span>`。VS Code の
/// プレビューも Obsidian も Typora も pandoc も描画するし、GitHub は `style`
/// 属性を落として言葉をそのまま表示する。最後の点が、`$\color{red}{...}$` の
/// 手を採らなかった理由 ── あれは GitHub では描画されるが、ほかのすべての場所
/// では読めない記号の山を残す。ノートは*文*に劣化すべきであって、記法に
/// 劣化すべきではない。
///
/// **どちらのフロントエンドでもなくここに置く。** 1 つの記法に 2 つのパーサーは
/// 2 つの答えであり、どちらかに手を入れた瞬間から、デスクトップ版と iPhone は
/// ノートの解釈で食い違いはじめる。
///
/// 正しい形の色 span でないものは、打たれたとおりに残す。ほかの style が
/// 付いた `<span>` も同じ ── ここは HTML レンダラではないし、そのふりを
/// すべきでもない。
pub fn spans(line: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut rest = line;

    while let Some(at) = rest.find("<span") {
        let (before, from) = rest.split_at(at);
        let Some(gt) = from.find('>') else { break };
        let open = &from[..=gt];
        let Some(color) = color_of(open) else {
            // こちらが書いたものではない。タグをテキストとして残して先へ進む。
            // そうしないと `<span class=…>` が行の残りを飲み込む。
            push(&mut out, &rest[..at + gt + 1], None);
            rest = &from[gt + 1..];
            continue;
        };
        let after = &from[gt + 1..];
        let Some(end) = after.find("</span>") else { break };
        push(&mut out, before, None);
        push(&mut out, &after[..end], Some(color));
        rest = &after[end + "</span>".len()..];
    }
    push(&mut out, rest, None);
    out
}

/// `s` の先頭にある色 span ── そのテキスト、色、そして消費した**文字数**。
///
///
/// 行を 1 文字ずつ走査していて、*ここ*が色 span の開始かどうかを知りたい
/// 側のためのもの。判定は [`spans`] と同じ ── 記法は 1 つ、それを知る場所も
/// 1 つ。
///
/// **バイトではなく文字で数える。** `find` はバイトを数え、呼び出し側は文字を
/// 数える。span の中が日本語だと 3 倍ずれるので、スキャナが span *とその後ろの
/// テキスト*をまとめて飛ばしていた ── 1 行に色付きの語が 2 つあるときに、
/// 2 つ目の色付きの語が
/// `e="color:#0E93A8">シアン</span>` in the middle of a sentence.
pub fn first_color(s: &str) -> Option<(String, String, usize)> {
    if !s.starts_with("<span") {
        return None;
    }
    let gt = s.find('>')?;
    let color = color_of(&s[..=gt])?;
    let after = &s[gt + 1..];
    let end = after.find("</span>")?;
    let bytes = gt + 1 + end + "</span>".len();
    Some((after[..end].to_string(), color, s[..bytes].chars().count()))
}

fn push(out: &mut Vec<Span>, text: &str, color: Option<String>) {
    if text.is_empty() {
        return;
    }
    // 同じ色が隣り合っていれば 1 つにまとめる ── 行がどう切られたかを、
    // 描く側が気にする必要は無い。
    if let Some(last) = out.last_mut() {
        if last.color == color {
            last.text.push_str(text);
            return;
        }
    }
    out.push(Span { text: text.to_string(), color });
}

/// `<span style="color:#0e93a8">` から `#rrggbb` を取り出す。そうでなければ `None`。
///
/// 16 進の 6 桁だけを受ける。`red` のような名前はレンダラごとに違う色になり、
/// それを書くのは「Mac でも同じに描かれる」と約束できないものを書くことに
/// なる。
fn color_of(open: &str) -> Option<String> {
    let lower = open.to_ascii_lowercase();
    let at = lower.find("color:")?;
    let rest = lower[at + "color:".len()..].trim_start();
    let hex = rest.strip_prefix('#')?;
    let digits: String = hex.chars().take(6).collect();
    if digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("#{digits}"))
    } else {
        None
    }
}

/// 検索ボックスの意味 ── AND のまとまりを OR でつないだもの。
///
/// `仕事 週報` finds notes with both. `仕事 OR 家` finds either. Written
/// together — `仕事 週報 OR 家` — the OR is the weaker join, so that reads as
/// (仕事 AND 週報) OR (家), which is how everybody writes it and nobody
/// が説明する。
///
/// **フロントエンドではなくここに置く。** クエリの意味は決めごとであり、
/// 3 つのフロントエンドが別々に決めれば、2 語打たれるまでは一致している
/// 3 つの検索ボックスになる。照合そのものは一覧のある場所に残す ── ここは
/// 何を照合するかを言う。
///
/// `OR` `or` `|` `｜` はすべて同じ意味。どのアプリがどれを求めるか誰も
/// 覚えていないし、全角の縦棒は日本語キーボードで切り替えずに打てるもの
/// だから。
/// 絞り込みの一語。
///
/// `field` は `any`（どこでも）/ `title` / `tag` / `book`。**`body` は無い** ──
/// 一覧が持っているのは本文の先頭 100 文字だけなので、`body:` を受けると
/// 「本文を探したのに見つからない」を作る。奥の一文は `find` の仕事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub field: String,
    pub word: String,
    /// `-` を頭に付けたもの。**当たったら落とす。**
    pub not: bool,
}

/// 二重引用符を尊重して切る。`title:"週次 報告"` が一語で通るように。
fn shatter(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in query.chars() {
        match c {
            '"' | '”' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn terms(query: &str) -> Vec<Vec<Term>> {
    let mut out: Vec<Vec<Term>> = Vec::new();
    let mut group: Vec<Term> = Vec::new();
    for word in shatter(query) {
        if matches!(word.as_str(), "OR" | "or" | "|" | "｜") {
            // 先頭の裸の `OR` や、2 つ続いた `OR` は、まだ打っている途中 ──
            // すべてに一致する空のグループではない。
            if !group.is_empty() {
                out.push(std::mem::take(&mut group));
            }
            continue;
        }
        let (not, rest) = match word.strip_prefix('-') {
            // `-` だけ、あるいは `-` のあとが空なら、それはまだ打ちかけ。
            Some(r) if !r.is_empty() => (true, r.to_string()),
            _ => (false, word),
        };
        // `tag:定型` のような絞り込み。**知らない接頭辞は文字列として扱う** ──
        // `http://…` や「10:30」を書いただけで消える語ができると、
        // 探せなくなったことに気づけない。
        let (field, w) = match rest.split_once(':') {
            Some((f, v))
                if !v.is_empty()
                    && matches!(
                        f,
                        "title" | "tag" | "book" | "タイトル" | "題" | "タグ" | "フォルダ"
                    ) =>
            {
                let f = match f {
                    // **画面は「タイトル」と言うので、そう打てる。**
                    // 「題」も受け続ける ── 前から打てたものを取り上げない。
                    "タイトル" | "題" => "title",
                    "タグ" => "tag",
                    "フォルダ" => "book",
                    other => other,
                };
                (f.to_string(), v.to_string())
            }
            _ => ("any".to_string(), rest),
        };
        if w.is_empty() {
            continue;
        }
        group.push(Term { field, word: w.to_lowercase(), not });
    }
    if !group.is_empty() {
        out.push(group);
    }
    out
}

/// `hay` がクエリに一致するか。空のクエリはすべてに一致する。
pub fn hits(hay: &str, query: &str) -> bool {
    let groups = terms(query);
    if groups.is_empty() {
        return true;
    }
    let hay = hay.to_lowercase();
    // ここには一本の藁束しか無いので、見出しの区別は付けられない ──
    // どの見出しの語も、この束の中を探す。見出しごとに探し分けるのは、
    // 題もタグも別々に持っている一覧の側（`Term::field` を見る）。
    groups
        .iter()
        .any(|g| g.iter().all(|t| hay.contains(&t.word) != t.not))
}

/// 色の記法を取り除いた言葉。
///
/// 描画ではなく文が欲しい場所のため ── 一覧の 2 行目と、検索の照合対象。
///
pub fn plain(line: &str) -> String {
    spans(line)
        .into_iter()
        .map(|s| {
            // 色の内側に記号があるときは、そこも剥がす（依頼 633）── 抜粋や
            // 検索対象に `**` が出ると、本文ではない記号で引っかかる。
            if s.text.contains('*') || s.text.contains('~') || s.text.contains('`') {
                let inner: String = spans(&s.text).into_iter().map(|x| x.text).collect();
                if inner != s.text {
                    return inner;
                }
            }
            s.text
        })
        .collect()
}

/// テキストを色で包む。amber が書くのと同じ形式で。
pub fn paint(text: &str, color: &str) -> String {
    format!("<span style=\"color:{color}\">{text}</span>")
}

/// この箇条書きの本文が `[ ] x` / `[x] x` で始まるか、そしてどちらか。
///
/// `[X]` も受ける ── 別の環境で打たれたノートもノートには変わりない。
fn ticked(rest: &str) -> Option<bool> {
    let b = rest.as_bytes();
    if b.len() < 3 || b[0] != b'[' || b[2] != b']' {
        return None;
    }
    match b[1] {
        b' ' => Some(false),
        b'x' | b'X' => Some(true),
        _ => None,
    }
}

/// 指定した行のチェックボックスを切り替え、ノート全体を返す。
///
/// **何番目のチェックボックスかではなく、行番号で指定する。** 画面の一覧は
/// 少し前の `blocks()` から描かれているかもしれない。個数で数えると、押した
/// ものより上にチェックボックスが 1 つ増えた時点で、別のものを切り替えて
/// しまう。チェックボックスでない行はそのまま残す ── 画面とファイルは
/// 食い違いうるし、食い違ったときは何も起きないのが正しい。
pub fn set_check(text: &str, line: usize, done: bool) -> String {
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let Some(row) = lines.get_mut(line) else { return text.to_string() };
    let indent: String = row.chars().take_while(|c| c.is_whitespace()).collect();
    let t = row.trim_start();
    let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) else {
        return text.to_string();
    };
    if ticked(rest).is_none() {
        return text.to_string();
    }
    let lead = &t[..t.len() - rest.len()];
    *row = format!("{indent}{lead}[{}]{}", if done { "x" } else { " " }, &rest[3..]);
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// 行に `![alt](link)` だけがある状態。
///
/// `markdown::to_html` もここを呼ぶ。**どこからが画像かの判断は 1 か所** ── デスクトップ版が
/// 自分で `![` を探しはじめると、iPhone が画像として表示する行をデスクトップ版がテキストで出す、
/// という食い違いが静かに育つ。
pub(crate) fn lone_image(t: &str) -> Option<Block> {
    let rest = t.strip_prefix("![")?;
    let close = rest.find(']')?;
    let tail = &rest[close + 1..];
    let inner = tail.strip_prefix('(')?;
    let end = inner.find(')')?;
    if !inner[end + 1..].trim().is_empty() {
        return None;
    }
    Some(Block::Image {
        alt: rest[..close].to_string(),
        link: inner[..end].trim().to_string(),
    })
}

/// ノートの front matter にある単純なフィールドを 1 つ設定または削除し、
/// ノート全体を返す。
///
/// テキストを受けてテキストを返す。理由は [`set_tags`] と同じ ── 呼び出し側が
/// 通常の手順で保存するので、ピン留めもディスク上のファイルに対して、入力と
/// まったく同じように競合検査される。
///
/// `None` は空の値を書くのではなくフィールドごと削除する ── `pinned:` と
/// 書いて後ろが空のノートは、次に読んだものからピン留め済みと解釈される
/// から。
pub fn set_field(text: &str, key: &str, value: Option<&str>) -> String {
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let end = text.ends_with('\n');
    let f = front(&lines);
    let key_l = key.to_ascii_lowercase();
    let line = value.map(|v| format!("{key}: {v}"));

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 4);
    if f.lines == 0 {
        let Some(line) = line else { return text.to_string() };
        out.push("---".into());
        out.push(line);
        out.push("---".into());
        out.extend(lines);
    } else {
        let mut wrote = false;
        out.push(lines[0].clone());
        for raw in &lines[1..f.lines - 1] {
            let t = raw.trim_start();
            let k = t.split_once(':').map(|(k, _)| k.trim().to_ascii_lowercase());
            if k.as_deref() == Some(key_l.as_str()) {
                if let Some(line) = &line {
                    if !wrote {
                        out.push(line.clone());
                        wrote = true;
                    }
                }
                continue;
            }
            out.push(raw.clone());
        }
        if let Some(line) = line {
            if !wrote {
                out.push(line);
            }
        }
        out.push(lines[f.lines - 1].clone());
        out.extend(lines[f.lines..].iter().cloned());
    }
    let mut s = out.join("\n");
    if end {
        s.push('\n');
    }
    s
}

/// ノートのタグを入れ替え、ノート全体を返す。
///
/// テキストを受けてテキストを返す。呼び出し側はほかの編集と同じ手順で保存する
/// ので、タグ付けも入力と同じ競合検査を通る。自分でファイルを書くタグ付けは
/// ノートを書く 2 つ目の経路になり、2 つ目の経路こそが他人の段落を
/// 消すもの。
///
/// front matter の無いノートには作る。front matter はあるが `tags:` の無い
/// ノートには、その末尾に行を足す ── **先頭ではない**。自分のフィールドを
/// どの順に並べたかは、その人のもの。
pub fn set_tags(text: &str, tags: &[String]) -> String {
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let end = text.ends_with('\n');
    let f = front(&lines);
    let line = format!(
        "tags: [{}]",
        tags.iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 4);
    if f.lines == 0 {
        // front matter がまったく無い。先頭に、それだけを置く。
        out.push("---".into());
        out.push(line);
        out.push("---".into());
        out.extend(lines);
    } else {
        // `f.lines` は区切り行も数えるので、中身は 1..f.lines-1。
        let mut wrote = false;
        out.push(lines[0].clone());
        let mut list = false;
        for raw in &lines[1..f.lines - 1] {
            let t = raw.trim_start();
            // リスト形式で書かれた `tags:` は、その `- item` の行も一緒に持っていく。
            if list && t.starts_with("- ") {
                continue;
            }
            list = false;
            let key = t.split_once(':').map(|(k, _)| k.trim().to_ascii_lowercase());
            if key.as_deref() == Some("tags") {
                if !wrote {
                    out.push(line.clone());
                    wrote = true;
                }
                list = t.split_once(':').map(|(_, v)| v.trim().is_empty()).unwrap_or(false);
                continue;
            }
            out.push(raw.clone());
        }
        if !wrote {
            out.push(line);
        }
        out.push(lines[f.lines - 1].clone());
        out.extend(lines[f.lines..].iter().cloned());
    }
    let mut s = out.join("\n");
    if end {
        s.push('\n');
    }
    s
}

/// ノートを別のフォルダへ移す。画像も一緒に連れていく。
///
/// **画像は必ず一緒に動かす。** ノートのリンクは相対パス ── `![](
/// attachments/note-1788450324680.jpg)` ── なので、ノートだけが移動すると
/// 画像が全部壊れた状態で着く。しかもそれが表に出るのは後になってからで、
/// ノートを開いた人は、画像が消されたのか最初から来なかったのか判断
/// できない。
///
/// どの画像が「そのノートのもの」かは、**ノートを読んで**判断する。ファイル名
/// からの推測ではない ── [`crate::naming::bring_pictures`] がそれをやる。
/// 同じフォルダの別のノートも参照している画像は、移動ではなく**コピー**する。
/// 移動すると、誰も触っていないそのノートが壊れるから。
///
/// 上書きはしない。移動先に同じ名前があれば、ノートを元の場所に残したまま
/// 移動を中止する。
pub fn move_to(note: &std::path::Path, dir: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let Some(name) = note.file_name() else {
        anyhow::bail!("移せません: {}", note.display())
    };
    let Some(from) = note.parent() else {
        anyhow::bail!("移せません: {}", note.display())
    };
    if from == dir {
        return Ok(note.to_path_buf());
    }
    std::fs::create_dir_all(dir)?;
    let to = dir.join(name);
    if to.exists() {
        anyhow::bail!("{} には同じ名前があります", dir.display());
    }

    // 画像が先、ノートは最後。**画像が置けなければ、ノートも動かない** ──
    // 元の場所のノートは、元の場所にある画像を指したままになる。
    let fresh = crate::naming::bring_pictures(note, &to)?;
    std::fs::rename(note, &to)?;
    if let Some(t) = fresh {
        std::fs::write(&to, t)?;
    }
    Ok(to)
}

/// 今日の日付。使っている人のいる場所での。
///
/// UTC ではなくローカル時刻。東京で夜 10 時に書いたノートは翌日ではなく
/// その日の日付になる。[`new_note`] と分けてあるのは、あちらに日付を渡して
/// テストできるようにするため。
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 現在時刻。この端末の時計が示すもの。
///
/// ここに置いてあるのは、自前のタイマーを持つフロントエンドが、現在時刻を
/// 知るためだけに日付ライブラリに依存せずに済むようにするため ── 必要な
/// 2 つ（これと [`next_ring`]）が同じ場所から来るので、どちらの日付かで
/// 食い違うことがない。
pub fn now_local() -> chrono::NaiveDateTime {
    chrono::Local::now().naive_local()
}

/// 秒までの時刻 ── 無題のノートの名前になるもの。
///
/// 日付だけでは足りない。1 つの午後に作った 3 つのノートが「2026-09-05」
/// 「2026-09-05-2」「2026-09-05-3」になり、番号はどれがどれかを何も
/// 言わない。時刻なら言える。
pub fn now_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 新しいノートのファイル名と、最初の中身。
///
/// 純粋関数で、時計を読まずに日付を受け取る。新しいノートの形をテストで
/// 記述できるようにするため。書き込みと空き名前の選択はエンジンがやる。
/// *ノートとは何か*に関わることはすべてここにある。ここが iPhone まで
/// 持っていけるモジュールだから。
///
/// 中身は front matter だけ。`title:` フィールドの下に `# タイトル` の見出しを
/// 置くと同じことを 2 回言うことになり、2 つ目のほうが、ノートを改名したときに
/// 古くなって残る。
/// その名前は、amber が付けた「作った時刻」か。
///
/// **人が付けた名前と区別が要る。** `買うもの.md` の中身が空でも、題は
/// 「買うもの」でいい ── 人がそう名づけたのだから。`2026-09-06 13-07-22.md`
/// は amber が名前を思いつけなかったときの符号なので、**題として出すと
/// 「書いた人が一度も言っていない名前」になる**（依頼 174 で前書きから
/// 追い出したものが、ファイル名から戻ってきていた）。
pub fn made_up_name(stem: &str) -> bool {
    let b = stem.as_bytes();
    b.len() == 19
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b' '
        && b[13] == b'-'
        && b[16] == b'-'
        && b.iter().enumerate().all(|(i, c)| {
            matches!(i, 4 | 7 | 10 | 13 | 16) || c.is_ascii_digit()
        })
}

pub fn new_note(title: &str, today: &str, now: &str) -> (String, String) {
    let title = title.trim();
    // 無題のノートは作られた*時刻*で名前を付ける ── 日付だけだと、1 つの午後に
    // 作った 3 つのノートが同じ名前＋番号になり、番号はどれがどれかを何も
    // 言わない。`created` は日付のまま ── あれはプログラムが解析するフィールドで、
    // タイトルは人が読むもの。
    let shown = if title.is_empty() { now } else { title };
    let stem = match file_stem(shown) {
        s if s.is_empty() => today.to_string(),
        s => s,
    };
    // **題を書かなかったなら、`title:` を書かない。**
    //
    // 前は作った時刻を題として書き込んでいた。書いた人が一度も言って
    // いない名前が入り、あとから「牛乳」と書いても題は時刻のままになる
    // ── 直すには前書きを開いて消すしかなく、その前書きは書く画面に
    // 出していない。題が無ければ `read` が書き出しの一行を採る。
    //
    // `tags: []` も書かない ── タグの無いノートが空の配列を持ち歩く理由は
    // 無く、`settags` が要るときに足す。`created` だけは残す: これは
    // 人が読む名前ではなく、並べ替えが読む欄。
    let body = if title.is_empty() {
        format!("---\ncreated: {today}\n---\n\n")
    } else {
        format!("---\ntitle: {shown}\ncreated: {today}\n---\n\n")
    };
    (format!("{stem}.md"), body)
}

#[cfg(test)]
mod tests {
    #[test]
    fn utf8_でないノートも一覧に出る() {
        let dir = tempfile::tempdir().unwrap();
        // 古い日本語（Shift_JIS・CRLF）── Windows のメモ帳が置いていく形。
        let body = "---\r\ntitle: 日本語\r\ntags: [仕事]\r\n---\r\n\r\n# 日本語\r\n\r\n本文です。\r\n";
        let (bytes, _, bad) = encoding_rs::SHIFT_JIS.encode(body);
        assert!(!bad, "試しの文字が Shift_JIS で書けません");
        std::fs::write(dir.path().join("日本語.md"), &bytes[..]).unwrap();

        // BOM 付き（古いメモ帳・Excel が置いていく形）。
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice("---\ntitle: 前書き\n---\n\n本文。\n".as_bytes());
        std::fs::write(dir.path().join("BOM.md"), bom).unwrap();

        let one = |name: &str| super::read(&dir.path().join(name), 40).expect(name);

        // **一覧から消えない。** 前は `read_to_string` が断って `None` を
        // 返し、Shift_JIS のノートが丸ごと出てこなかった。
        let sjis = one("日本語.md");
        assert_eq!(sjis.title, "日本語");
        assert_eq!(sjis.tags, vec!["仕事"]);

        // **BOM は前書きを隠さない。** 前は一行目が `\u{feff}---` になり、
        // 前書きが前書きに見えず、題がファイル名から採られていた。
        let bom = one("BOM.md");
        assert_eq!(bom.title, "前書き");
        assert!(!bom.excerpt.contains('\u{feff}'), "BOM が本文に残っています: {:?}", bom.excerpt);
        assert!(!bom.excerpt.contains("---"), "前書きが本文に漏れています: {:?}", bom.excerpt);
    }

    /// **同期が置いていくものと、人が書いたものを分ける。**
    ///
    /// 同じフォルダを Windows からも開くので、Office 系の置き土産
    /// （`~$…`）が一覧に紛れ込んでいた。逆に、**同期がぶつかった控えは
    /// 人が打った文字**なので、落としてはいけない。
    #[test]
    fn 一覧は一時ファイルを落とし_競合コピーは残す() {
        let dir = tempfile::tempdir().unwrap();
        let put = |name: &str, body: &str| {
            std::fs::write(dir.path().join(name), body).unwrap();
        };
        put("段取り.md", "---\ntitle: 段取り\n---\n\n# 段取り\n");
        put("~$段取り.md", "x");
        put("段取り (競合コピー 2026-09-09).md", "---\ntitle: 競合\n---\n\n# 競合\n");
        // 隠しのものは歩く側が落とす ── ここでも消えていることだけ見る。
        put(".段取り.md.icloud", "x");
        put("._段取り.md", "x");
        std::fs::create_dir(dir.path().join("フォルダ.md")).unwrap();

        let limits = crate::survey::Limits { depth: 4, rows: 100, hidden: false, ..Default::default() };
        let stop = std::sync::atomic::AtomicBool::new(false);
        let (found, _) = super::list(dir.path(), limits, &stop);
        let mut names: Vec<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        names.sort_unstable();

        assert_eq!(
            names,
            vec!["段取り (競合コピー 2026-09-09).md", "段取り.md"],
            "一覧に出るのは人の書いたものだけ",
        );
    }

    #[test]
    fn 複製は本文を保ち_日付は今日になる() {
        let dir = tempfile::tempdir().unwrap();
        let at = dir.path().join("段取り.md");
        std::fs::write(
            &at,
            "---\ntitle: 段取り\ncreated: 2020-01-01\nupdated: 2020-02-02\ntags: [仕事]\n---\n\n# 段取り\n\n本文。\n",
        )
        .unwrap();
        let made = super::duplicate(&at, None, "2026-09-08").unwrap();
        // 元は触らない。
        assert!(at.is_file());
        assert_eq!(made.file_name().unwrap(), "段取り-2.md");
        let got = std::fs::read_to_string(&made).unwrap();
        // 書いた人の言葉は、そのまま。
        assert!(got.contains("title: 段取り"), "{got}");
        assert!(got.contains("tags: [仕事]"), "{got}");
        assert!(got.contains("# 段取り"), "{got}");
        assert!(got.contains("本文。"), "{got}");
        // できたのは今日で、直した日はまだ無い。
        assert!(got.contains("created: 2026-09-08"), "{got}");
        assert!(!got.contains("2020-01-01"), "{got}");
        assert!(!got.contains("updated:"), "{got}");
        // もう一度写しても、上書きしない。
        let again = super::duplicate(&at, None, "2026-09-08").unwrap();
        assert_eq!(again.file_name().unwrap(), "段取り-3.md");

        // 行き先を渡せば、そこへ。**同じ名前で置ける**（別のフォルダなので）。
        let other = dir.path().join("仕事");
        let there = super::duplicate(&at, Some(&other), "2026-09-08").unwrap();
        assert_eq!(there.parent().unwrap(), other);
        assert_eq!(there.file_name().unwrap(), "段取り.md");
    }


    /// 読みやすさのため、`Term` を `接頭辞:語` の 1 つの文字列に畳む。
    fn flat(q: &str) -> Vec<Vec<String>> {
        terms(q)
            .into_iter()
            .map(|g| {
                g.into_iter()
                    .map(|t| format!("{}{}:{}", if t.not { "-" } else { "" }, t.field, t.word))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn 検索ボックスは_and_のまとまりの_or() {
        assert_eq!(flat("仕事 週報"), vec![vec!["any:仕事", "any:週報"]]);
        assert_eq!(flat("仕事 OR 家"), vec![vec!["any:仕事"], vec!["any:家"]]);
        // OR のほうが弱い綴じ ── (仕事 AND 週報) OR (家)。
        assert_eq!(flat("仕事 週報 or 家"), vec![vec!["any:仕事", "any:週報"], vec!["any:家"]]);
        // 打ちかけの OR は、何にでも当たる空の組にしない。
        assert!(flat("OR").is_empty());
        assert_eq!(flat("仕事 OR OR 家").len(), 2);
        // 全角の縦棒も同じ ── 日本語キーボードで切り替えずに出るのはこちら。
        assert_eq!(flat("あ ｜ い").len(), 2);

        assert!(hits("週報 仕事 定型", "仕事 週報"));
        assert!(!hits("週報 仕事", "仕事 買い物"));
        assert!(hits("買うもの 家", "仕事 OR 家"));
        assert!(hits("なんでも", ""), "空の問いは全部に当たる");
        // 大文字小文字は問わない。
        assert!(hits("Weekly Report", "weekly"));
    }

    #[test]
    fn 探す先を名指しできる() {
        // Inkdrop と同じ綴じ。日本語の見出しも通す ── `tag:` を打つのに
        // 英字へ切り替えるのは、日本語で書いている最中には高い。
        assert_eq!(flat("tag:定型"), vec![vec!["tag:定型"]]);
        assert_eq!(flat("タグ:定型"), vec![vec!["tag:定型"]]);
        assert_eq!(flat("book:仕事 題:週報"), vec![vec!["book:仕事", "title:週報"]]);
        // 画面が「タイトル」と言うようになったので、そう打てる（「題」も残す）。
        assert_eq!(flat("タイトル:週報"), vec![vec!["title:週報"]]);
        // 二重引用符で、空白を含む一語。
        assert_eq!(flat("title:\"週次 報告\""), vec![vec!["title:週次 報告"]]);
        assert_eq!(flat("\"AND を含む句\"").len(), 1);

        // `-` は落とす。
        assert_eq!(flat("-tag:雑"), vec![vec!["-tag:雑"]]);
        assert!(hits("週報 仕事", "仕事 -買い物"));
        assert!(!hits("週報 仕事 買い物", "仕事 -買い物"));

        // **知らない接頭辞は文字列のまま。** `http://…` や「10:30」を打っただけで
        // 消える語ができると、探せなくなったことに気づけない。
        assert_eq!(flat("http://example.com"), vec![vec!["any:http://example.com"]]);
        assert_eq!(flat("10:30"), vec![vec!["any:10:30"]]);
        // `-` だけ、`tag:` だけは、まだ打ちかけ。
        assert!(flat("-").is_empty() || flat("-") == vec![vec!["any:-"]]);
        assert_eq!(flat("tag:"), vec![vec!["any:tag:"]]);
    }

    #[test]
    fn 溜まったぶんの実行は_実行したことを記録する() {
        use chrono::NaiveDate;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("週報.md");
        std::fs::write(
            &p,
            "---\ntitle: 週報\nrepeat: weekly wed 09:00\nlast: 2026-08-30\n---\n本文。\n",
        )
        .unwrap();

        let made = catch_up(&p, NaiveDate::from_ymd_opt(2026, 9, 5).unwrap()).unwrap();
        assert_eq!(made.len(), 1, "2026-09-02 は水曜: {made:?}");
        assert!(std::fs::read_to_string(&p).unwrap().contains("last: 2026-09-02"));

        // **二度目は何も作らない。** これを忘れると、フォルダを開くたびに
        // 「作りました」と言い続ける。
        assert!(catch_up(&p, NaiveDate::from_ymd_opt(2026, 9, 5).unwrap()).unwrap().is_empty());

        // 定型でないノートは、何も作らないしエラーでもない。
        let plain = d.path().join("ただのノート.md");
        std::fs::write(&plain, "---\ntitle: x\n---\n本文。\n").unwrap();
        assert!(catch_up(&plain, NaiveDate::from_ymd_opt(2026, 9, 5).unwrap()).unwrap().is_empty());
    }

    #[test]
    fn 繰り返しが次に来る時刻() {
        use chrono::NaiveDate;
        let at = |y, m, d, h, mi| NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, mi, 0).unwrap();

        // 今日の時刻がまだなら今日、過ぎていれば明日。
        assert_eq!(next_ring(Every::Daily, 9, 0, at(2026, 9, 5, 8, 0)), Some(at(2026, 9, 5, 9, 0)));
        assert_eq!(next_ring(Every::Daily, 9, 0, at(2026, 9, 5, 9, 0)), Some(at(2026, 9, 6, 9, 0)));

        // 2026-09-05 は土曜。水曜は 09-09。
        assert_eq!(
            next_ring(Every::Weekly(2), 9, 0, at(2026, 9, 5, 12, 0)),
            Some(at(2026, 9, 9, 9, 0))
        );
        // 当日の朝、時刻の前なら今日。
        assert_eq!(
            next_ring(Every::Weekly(2), 9, 0, at(2026, 9, 9, 8, 0)),
            Some(at(2026, 9, 9, 9, 0))
        );

        // 31日は、30日しかない月では末日へ寄る ── そうしないと
        // 「毎月31日」が「7か月だけ」になる。
        assert_eq!(
            next_ring(Every::Monthly(31), 9, 0, at(2026, 11, 1, 0, 0)),
            Some(at(2026, 11, 30, 9, 0))
        );
        // 鳴った直後に訊いても、同じものを返さない。
        let fired = at(2026, 9, 5, 9, 0);
        assert!(next_ring(Every::Daily, 9, 0, fired).unwrap() > fired);
    }

    #[test]
    fn 色は_span_として読み_ほかは打たれたまま残す() {
        let one = spans("ふつうの<span style=\"color:#0E93A8\">シアン</span>あと");
        assert_eq!(one.len(), 3);
        assert_eq!(one[0], Span { text: "ふつうの".into(), color: None });
        assert_eq!(one[1], Span { text: "シアン".into(), color: Some("#0e93a8".into()) });
        assert_eq!(one[2], Span { text: "あと".into(), color: None });

        // 色の無い span は、こちらが書いたものではない。文字として残す。
        let asis = spans("<span class=\"x\">そのまま</span>");
        assert_eq!(asis.iter().filter(|s| s.color.is_some()).count(), 0);
        assert_eq!(asis.iter().map(|s| s.text.as_str()).collect::<String>(),
                   "<span class=\"x\">そのまま</span>");

        // 名前の色は読まない ── 描く側ごとに違う色になるので。
        assert!(spans("<span style=\"color:red\">あか</span>")
            .iter()
            .all(|s| s.color.is_none()));

        // 閉じていないものは、書いた通りに。
        assert_eq!(spans("<span style=\"color:#123456\">とじ忘れ").len(), 1);

        // 色の無い行は 1 つのまとまり。
        assert_eq!(spans("ただの行"), vec![Span { text: "ただの行".into(), color: None }]);
        assert!(spans("").is_empty());

        // 表の行も二行目には出さない ── 「表がある」は見れば分かる。
        {
            let d = tempfile::tempdir().unwrap();
            let p = d.path().join("t.md");
            std::fs::write(&p, "---\ntitle: x\n---\n\n本文です。\n\n| 名前 | 状態 |\n| --- | --- |\n| 通知 | 済 |\n").unwrap();
            assert_eq!(read(&p, 40).unwrap().excerpt, "本文です。");
        }

        // 一覧の 2 行目と検索は、テキストだけを見る ── 色を付けた語が
        // 探せなくなるのが一番困る。
        assert_eq!(plain("あ<span style=\"color:#0e93a8\">い</span>う"), "あいう");

        // 書いたものが読める。
        let painted = paint("ここ", "#d9822b");
        let back = spans(&painted);
        assert_eq!(back, vec![Span { text: "ここ".into(), color: Some("#d9822b".into()) }]);
    }

    #[test]
    fn チェックボックスは押せるブロックで_箇条書きは違う() {
        let text = "---\ntitle: x\n---\n\n- [ ] 牛乳\n- [x] 珈琲\n- ふつうの箇条書き\n";
        let bs = blocks(text);
        let mut checks = Vec::new();
        for b in &bs {
            if let Block::Check { done, text, line } = b {
                checks.push((*done, text.clone(), *line));
            }
        }
        assert_eq!(checks.len(), 2, "{bs:?}");
        assert_eq!(checks[0], (false, "牛乳".to_string(), 4));
        assert_eq!(checks[1], (true, "珈琲".to_string(), 5));
        assert!(matches!(&bs[2], Block::Bullet(t) if t == "ふつうの箇条書き"));
    }

    #[test]
    fn チェックを押すとその行だけが変わる() {
        let text = "- [ ] 牛乳\n- [ ] 珈琲\n";
        let on = set_check(text, 1, true);
        assert_eq!(on, "- [ ] 牛乳\n- [x] 珈琲\n");
        assert_eq!(set_check(&on, 1, false), text);
        // 行がずれていた・そこは箇条書きだった、のときは何もしない ──
        // 画面とファイルが食い違っているのに書き込むのが一番悪い。
        assert_eq!(set_check(text, 9, true), text);
        assert_eq!(set_check("- ふつう\n", 0, true), "- ふつう\n");
        // インデントはそのまま
        assert_eq!(set_check("  - [ ] 中\n", 0, true), "  - [x] 中\n");
    }
    use super::*;

    #[test]
    fn ノートはタイトルとタグで探せる_ファイル名ではなく() {
        let n = Note {
            path: "/n/page-0012.md".into(),
            title: "段取り".into(),
            excerpt: "本文です。".into(),
            findable: "来期の見通し ＣＰＵ ８コア".into(),
            tags: vec!["仕事".into(), "OneNote".into()],
            updated: Some(0),
            created: Some(0),
            bytes: 0,
            star: None,
        };
        let h = haystack(&n);
        assert!(h.contains("段取り"), "the title: {h}");
        assert!(h.contains("#仕事"), "the tag, with its hash: {h}");
        assert!(h.contains("#onenote"), "lowercased, so the filter need not be: {h}");
        assert!(h.contains("本文です"), "and the start of it: {h}");
        // 見出しと表のセルも検索できる（依頼 606・本人が決めた）。
        assert!(h.contains("来期の見通し"), "見出しも探し先に入る: {h}");
        // `haystack` は小文字に揃える（探す側も揃えるので、これで当たる）。
        assert!(h.contains("ｃｐｕ"), "表のセルも検索対象に入る: {h}");
    }

    /// **表示するテキストと、検索対象のテキストを分ける**（依頼 606）。
    ///
    /// 一覧の二行目（`excerpt`）は一行の要約なので、見出しも表も入れない
    /// ── そこは変えない。けれど「書いたのに探せない」は別の話で、
    /// 小見出しやセルの中身は、書いた本人にとっては書いたことそのもの。
    #[test]
    fn 見出しと表のセルは探せるが_一覧の二行目には出ない() {
        let dir = tempfile::tempdir().unwrap();
        let at = dir.path().join("a.md");
        std::fs::write(
            &at,
            "---\ntitle: 週報\n---\n\n書き出しの一行。\n\n## **来期**の見通し\n\n             | 名前 | 値 |\n| --- | --- |\n| ＣＰＵ | ８コア |\n\n             ```md\n# 枠の中の見出し\n| 枠の中の升 |\n```\n",
        )
        .unwrap();
        let n = read(&at, 60).unwrap();
        // 一覧の二行目は、いままでどおり地の文だけ。
        assert_eq!(n.excerpt, "書き出しの一行。", "{:?}", n.excerpt);
        // 検索対象には見出しとセルが入る。**書式は落ちている**（`**来期**` ではない）。
        let h = haystack(&n);
        assert!(h.contains("来期の見通し"), "見出し: {h}");
        assert!(h.contains("ｃｐｕ") && h.contains("８コア"), "セルの中身: {h}");
        assert!(h.contains("名前"), "ヘッダー行のセルも: {h}");
        // 区切り行は本文ではない。コードブロックの中は拾わない。
        assert!(!h.contains("---"), "区切りの行は入れない: {h}");
        // **枠の中の見出しは、見出しではない。** 「Markdown の書き方」を
        // 書いたノートが、中の例文ぜんぶで当たるようになる。
        assert!(!h.contains("枠の中の見出し"), "コード枠の中の見出しは入れない: {h}");
        assert!(!h.contains("枠の中のセル"), "コード枠の中の表も入れない: {h}");
        assert!(!h.contains("**"), "書式は落とす: {h}");
    }

    /// **折りたたみのタグは、一覧の 2 行目に出さない**（依頼 619）。
    ///
    /// `<details>` と `</details>` は「ここから畳んである」という形の話で、
    /// 一覧に山括弧が並ぶ（実機で出た）。**見出しは残す** ── あれは
    /// 畳んだ中身に人が付けた名前で、文として読める。
    #[test]
    fn 折りたたみのラベルは抜粋に出さず_見出しは残す() {
        let dir = tempfile::tempdir().unwrap();
        let at = dir.path().join("a.md");
        std::fs::write(
            &at,
            "---\ntitle: 週報\n---\n\n<details>\n<summary>ながいコード</summary>\n\n             書き出しの一行。\n\n</details>\n",
        )
        .unwrap();
        let n = read(&at, 60).unwrap();
        assert!(!n.excerpt.contains('<'), "山括弧が並んでいる: {:?}", n.excerpt);
        assert!(n.excerpt.contains("書き出しの一行"), "{:?}", n.excerpt);
        // 見出しは文として読めるので、抜粋にも探し先にも残す。
        assert!(n.excerpt.contains("ながいコード"), "見出しが落ちた: {:?}", n.excerpt);
        assert!(haystack(&n).contains("ながいコード"), "探せない: {}", haystack(&n));
    }

    /// **表は、文ではない。**
    ///
    /// セルに分けずに渡していた頃、iPhone は行を空白でつないで
    /// `| 画面 | 何が見えるか | …` を 1 つの段落として描いていた ── 表を誰も
    /// 解釈しなかったときの見た目そのもの。デスクトップ版は `to_html` で解釈できていたので、
    /// **同じノートが二つの amber で別のものに見えていた。**
    #[test]
    fn 表はセルに切って渡す() {
        let b = blocks("| 面 | いつ |\n|---|:---:|\n| **表示** | ふだん |\n| コード | 直すとき |\n");
        let Block::Table { head, align, rows } = &b[0] else {
            panic!("表になっていません: {:?}", b)
        };
        assert_eq!(head, &["面".to_string(), "いつ".to_string()]);
        assert_eq!(align[0], crate::markdown::Align::Left);
        assert_eq!(align[1], crate::markdown::Align::Center);
        assert_eq!(rows.len(), 2);
        // セルの中の書式は剥がさない ── 剥がす処理を 2 か所に持たない。
        assert_eq!(rows[0][0], "**表示**");
        assert_eq!(rows[1][1], "直すとき");

        // 区切りの無い縦棒は、人が書いた文。
        let b = blocks("| これは | 表ではない\n");
        assert!(matches!(b[0], Block::Paragraph(_)), "{:?}", b);
    }

    /// 注記は引用とは別物 ── `[!TIP]` を文字として出さない。
    #[test]
    fn 注記は種類と中身に分かれる() {
        let b = blocks("> [!TIP]\n> ここで打てます。\n> 手順はありません。\n>\n> 二つめの段。\n");
        let Block::Alert { kind, body } = &b[0] else { panic!("注記になっていません: {:?}", b) };
        assert_eq!(kind, "tip");
        // 続く行は一つの段にまとまり、空の `>` で段が変わる。
        assert_eq!(body, &["ここで打てます。 手順はありません。".to_string(),
                           "二つめの段。".to_string()]);

        // GitHub が知らない種類は、ただの引用のまま ── ここでだけ見える
        // 記法を増やすと、同じノートが GitHub で壊れる。
        let b = blocks("> [!HINT]\n> ふつうの引用です。\n");
        assert!(matches!(b[0], Block::Quote(_)), "{:?}", b);
    }

    #[test]
    fn 画像が_ノートの内容を示す行を埋めてしまわない() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.md");
        std::fs::write(
            &p,
            "# 題\n![](attachments/n-1788450324680.jpg)\n本文はこちら。\n",
        )
        .unwrap();
        let n = read(&p, 60).unwrap();
        assert_eq!(n.excerpt, "本文はこちら。", "got {:?}", n.excerpt);

        // 代替テキストは人が選んだ言葉なので残す。
        std::fs::write(&p, "# 題\n![現場の写真](a.jpg) のとおり。\n").unwrap();
        assert_eq!(read(&p, 60).unwrap().excerpt, "現場の写真 のとおり。");

        // 通常のリンクは言葉を残して記法を落とす ── その大きさでは誰も読まない
        // same as the title does. `[手順](x.md)` in a one-line reminder is
        // 数十文字の URL は落とし、
        // title next to it already says just 手順.
        std::fs::write(&p, "# 題\n[手順](x.md) を見て。\n").unwrap();
        assert_eq!(read(&p, 60).unwrap().excerpt, "手順 を見て。");

        // Emphasis is drawing, not words. Left in, `**大事**` puts four
        // asterisks in the line and stops being findable by 大事.
        std::fs::write(&p, "# 題\n> **大事**なのは `ここ`。\n").unwrap();
        assert_eq!(read(&p, 60).unwrap().excerpt, "大事なのは ここ。");

        // 箇条書きも文であることに変わりはない。その記号は文の一部ではない。
        std::fs::write(&p, "# 題\n- [ ] 牛乳を買う\n").unwrap();
        assert_eq!(read(&p, 60).unwrap().excerpt, "牛乳を買う");

        // 画像だけのノートは抜粋を持たない ── ファイル名でできた抜粋を
        // 持つよりよい。
        std::fs::write(&p, "# 題\n![](a.jpg)\n").unwrap();
        assert_eq!(read(&p, 60).unwrap().excerpt, "");
    }

    #[test]
    fn ノートは描画できる単位に分かれる() {
        let md = "---\ntitle: 段取り\n---\n# 見出し\n本文の一行目\nと二行目。\n\n- ひとつ\n2. ふたつ\n> 引用\n![現場](a.jpg)\n\n```rust\nfn main() {}\n\nlet x = 1;\n```\n---\nおわり\n";
        let b = blocks(md);
        // front matter はノートの自己説明であって、本文として述べていることでは
        // ない ── タイトルはこの上に既に表示されている。
        assert_eq!(b[0], Block::Heading { level: 1, text: "見出し".into(), line: 3 });
        // あいだに空行の無い 2 行は 1 つの段落。Markdown の規定どおりで、
        // iPhone で打っている人が期待するとおり。
        assert_eq!(b[1], Block::Paragraph("本文の一行目 と二行目。".into()));
        assert_eq!(b[2], Block::Bullet("ひとつ".into()));
        assert_eq!(b[3], Block::Numbered { n: 2, text: "ふたつ".into() });
        assert_eq!(b[4], Block::Quote("引用".into()));
        assert_eq!(b[5], Block::Image { alt: "現場".into(), link: "a.jpg".into() });
        // コードブロックは空行を保つ。落とすとコードが変わってしまう。
        assert_eq!(b[6], Block::Code { lang: "rust".into(), text: "fn main() {}\n\nlet x = 1;".into() });
        assert_eq!(b[7], Block::Rule, "`---` below the front matter is a rule");
        assert_eq!(b[8], Block::Paragraph("おわり".into()));
        assert_eq!(b.len(), 9);
    }

    #[test]
    fn 空白の無いハッシュはタグであって見出しではない() {
        // `#仕事` on its own line is how people write a tag. Reading it as a
        // 見出しにすると、タグの付いたノートがすべて 32pt のタグで始まることになる。
        assert_eq!(blocks("#仕事\n"), vec![Block::Paragraph("#仕事".into())]);
        assert_eq!(blocks("# 仕事\n"), vec![Block::Heading { level: 1, text: "仕事".into(), line: 0 }]);
        // 後ろに言葉が続く画像は文であって、単独の画像ではない。
        assert_eq!(
            blocks("![a](b.jpg) のとおり\n"),
            vec![Block::Paragraph("![a](b.jpg) のとおり".into())]
        );
        // 閉じられなかったコードブロックは、1 行ずつ段落のふりをせず、残りを
        // 丸ごと取り込む。
        assert_eq!(
            blocks("```\nx\ny\n"),
            vec![Block::Code { lang: String::new(), text: "x\ny".into() }]
        );
    }

    #[test]
    fn 移動したノートは画像を連れていく() {
        let d = tempfile::tempdir().unwrap();
        let note = d.path().join("段取り.md");
        std::fs::write(&note, "# 段取り\n").unwrap();
        let link = attach(&note, &[1, 2, 3], "png").unwrap();
        // **リンクがノートの中にあることが条件。** どの画像が一緒に動くかは本文を
        // 読んで決める。ファイル名からの推測ではない ── どこからも参照されていない
        // 画像は「そのノートのもの」ではなく余りで、`spare::find` がそれを扱う。
        std::fs::write(&note, format!("# 段取り\n![]({link})\n")).unwrap();
        // 別のノートの画像なので、その場に残さなければならない。
        let other = d.path().join("他.md");
        let others = attach(&other, &[9], "png").unwrap();
        std::fs::write(&other, format!("x ![]({others})\n")).unwrap();

        let book = d.path().join("仕事");
        let moved = move_to(&note, &book).unwrap();
        assert_eq!(moved, book.join("段取り.md"));
        // ノートの中のリンクは相対パスなので、移動先でも同じものを指せなければ
        // ならない。
        assert_eq!(std::fs::read(book.join(&link)).unwrap(), vec![1, 2, 3]);
        assert!(!d.path().join(&link).exists(), "and not left behind as well");
        assert!(std::fs::read(d.path().join(&others)).is_ok(), "the other note's picture stays");

        // 同じ名前が既にあれば、上書きせずに移動を中止する。
        let clash = d.path().join("段取り.md");
        std::fs::write(&clash, "別物").unwrap();
        assert!(move_to(&clash, &book).is_err());
        assert_eq!(std::fs::read_to_string(&clash).unwrap(), "別物", "still where it was");

        // すでにいるフォルダへの移動は失敗ではない。
        assert_eq!(move_to(&moved, &book).unwrap(), moved);
    }

    #[test]
    fn 繰り返しが作るのはタスクであってテンプレートではない() {
        use chrono::NaiveDate;
        let d = tempfile::tempdir().unwrap();
        let t = d.path().join("ごみ出し.md");
        std::fs::write(
            &t,
            "---\ntitle: ごみ出し\nrepeat: weekly wed 09:00\nlast: 2026-08-30\ntags: [家]\n---\n- [ ] 燃えるゴミ\n",
        )
        .unwrap();

        let made = carry_out(&t, NaiveDate::from_ymd_opt(2026, 9, 2).unwrap()).unwrap();
        assert_eq!(made.file_name().unwrap(), "ごみ出し 2026-09-02.md");
        let copy = std::fs::read_to_string(&made).unwrap();
        // できるのはタスクであってテンプレートではない。自分自身の複製を生んではいけない。
        assert!(!copy.contains("repeat"), "{copy}");
        assert!(!copy.contains("last"), "{copy}");
        assert!(copy.contains("title: ごみ出し 2026-09-02"), "{copy}");
        assert!(copy.contains("created: 2026-09-02"), "{copy}");
        // そのタスクの中身 ── タグとチェックリスト ── は残る。
        assert!(copy.contains("tags: [家]"), "{copy}");
        assert!(copy.contains("- [ ] 燃えるゴミ"), "{copy}");
        // そしてテンプレート自体は変わらない。
        assert!(std::fs::read_to_string(&t).unwrap().contains("repeat: weekly wed 09:00"));

        // 同じ日に 2 回は 1 回として扱う ── そうしないと、2 台が追いつく処理を
        // したときに水曜日が 2 つ並ぶ。
        let again = carry_out(&t, NaiveDate::from_ymd_opt(2026, 9, 2).unwrap()).unwrap();
        assert_eq!(again, made);
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 2);
    }

    #[test]
    fn 通知の書き方は三つの形() {
        use chrono::NaiveDate;
        let r = remind("---\nremind: 2026-09-10 09:00\n---\n本文\n");
        assert_eq!(r.once, NaiveDate::from_ymd_opt(2026, 9, 10).unwrap().and_hms_opt(9, 0, 0));

        assert_eq!(remind("---\nrepeat: daily 07:30\n---\n").every, Some((Every::Daily, 7, 30)));
        // 月曜が 0（chrono と同じ数え方）なので、水曜は 2。
        assert_eq!(remind("---\nrepeat: weekly wed 09:00\n---\n").every, Some((Every::Weekly(2), 9, 0)));
        // 日本語で実際に打たれる文字。
        assert_eq!(remind("---\nrepeat: weekly 水 09:00\n---\n").every, Some((Every::Weekly(2), 9, 0)));
        assert_eq!(remind("---\nrepeat: monthly 1 09:00\n---\n").every, Some((Every::Monthly(1), 9, 0)));

        // 解釈できないものは「無し」にする。推測はしない ── 打ち間違いから作られた
        // 通知は、誰も選んでいない時刻に鳴る。
        assert_eq!(remind("---\nrepeat: weekly ときどき 09:00\n---\n").every, None);
        assert_eq!(remind("---\nremind: あした\n---\n").once, None);
        assert_eq!(remind("---\nremind: あした\n---\n").day, None);
        assert_eq!(remind("---\nrepeat: daily 25:00\n---\n").every, None);
        assert_eq!(remind("本文だけ\n"), Remind::default());
    }

    #[test]
    fn 時刻の無い通知は_終日として読む() {
        use chrono::NaiveDate;
        // **書く側が実際に書く二つの形**（デスクトップ版の `calAdd` と iPhone の
        // `Calendaring` は、どちらも `day` か `day + ' ' + 時刻` を書く）。
        // 読めない形を書いていたので、登録した日に出なかった（2026-09-22）。
        let d = NaiveDate::from_ymd_opt(2026, 9, 23).unwrap();
        let all = remind("---\nremind: 2026-09-23\n---\n");
        assert_eq!(all.day, Some(d), "日付だけは終日");
        assert_eq!(all.once, None, "鳴らす時刻は勝手に決めない");

        let timed = remind("---\nremind: 2026-09-23 14:30\n---\n");
        assert_eq!(timed.once, d.and_hms_opt(14, 30, 0));
        assert_eq!(timed.day, None, "時刻があれば終日ではない");
    }

    #[test]
    fn 電源が切れていたあいだに逃したぶんを_繰り返しが言える() {
        use chrono::NaiveDate;
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();

        // 2026-09-02 は水曜日。2 週間空ければ、水曜が 2 回ぶん溜まる。
        let due = due_since(Every::Weekly(2), Some(d(2026, 8, 30)), d(2026, 9, 10));
        assert_eq!(due, vec![d(2026, 9, 2), d(2026, 9, 9)]);

        // ある水曜日とその翌日のあいだには、何も溜まらない。
        assert!(due_since(Every::Weekly(2), Some(d(2026, 9, 2)), d(2026, 9, 3)).is_empty());

        // 一度も実行していない場合、今日がその曜日なら今日も数える。
        assert_eq!(due_since(Every::Weekly(2), None, d(2026, 9, 2)), vec![d(2026, 9, 2)]);
        assert!(due_since(Every::Weekly(2), None, d(2026, 9, 3)).is_empty());

        // 31 日は、その月に 31 日が無ければ末日に落とす。飛ばさない ── 2 月を
        // 飛ばす毎月の繰り返しは、1 日早く来るものより悪い。
        //
        assert_eq!(
            due_since(Every::Monthly(31), Some(d(2026, 1, 31)), d(2026, 2, 28)),
            vec![d(2026, 2, 28)]
        );

        // ずっと前の `last` からでも、出るのは数件であって 700 件ではない。
        assert!(due_since(Every::Daily, Some(d(2024, 1, 1)), d(2026, 9, 10)).len() <= 32);
    }

    #[test]
    fn ピン留めと解除で_何も失わない() {
        let src = "---\ntitle: 段取り\ntags: [仕事]\n---\n本文。\n";
        let on = set_field(src, "pinned", Some("true"));
        assert_eq!(on, "---\ntitle: 段取り\ntags: [仕事]\npinned: true\n---\n本文。\n");

        // 解除は `pinned: false` を書くのではなく行ごと削除する ── ましてや
        // 後ろが空の `pinned:` にはしない。次にノートを読んだものが、それを
        // ピン留め済みと解釈するから。
        let off = set_field(&on, "pinned", None);
        assert_eq!(off, src);

        // 2 回設定しても、2 行にはならない。
        let twice = set_field(&set_field(src, "pinned", Some("true")), "pinned", Some("true"));
        assert_eq!(twice.matches("pinned").count(), 1);

        // 持っていないフィールドを削除しても、ノートは変わらない。
        assert_eq!(set_field(src, "pinned", None), src);
        // front matter が無く、書くものも無いノートは、そのまま。
        assert_eq!(set_field("本文。\n", "pinned", None), "本文。\n");
    }

    #[test]
    fn 人が_はい_と書く三通り() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.md");
        // `pinned` は、お気に入りができる前に書かれたノートが使っている書き方。
        // それらが黙ってお気に入りでなくなってはいけない。
        for key in ["star", "favorite", "pinned"] {
            for (v, want) in [
                ("true", Some("")),
                ("yes", Some("")),
                ("1", Some("")),
                ("false", None),
                ("", None),
                ("買い物/週次", Some("買い物/週次")),
            ] {
                std::fs::write(&p, format!("---\ntitle: x\n{key}: {v}\n---\n本文。\n")).unwrap();
                assert_eq!(
                    read(&p, 20).unwrap().star.as_deref(),
                    want,
                    "{key}: {v:?}"
                );
            }
        }
    }

    #[test]
    fn タグを付けても_ノートのほかの部分は乱れない() {
        // ほかのフィールドと、その並び順は書いた人のもの ── 置き換えるのは
        // タグの行だけ。
        let src = "---\ntitle: 段取り\ncreated: 2026-09-04\ntags: [古い]\n---\n本文。\n";
        let out = set_tags(src, &["仕事".into(), "cian".into()]);
        assert_eq!(
            out,
            "---\ntitle: 段取り\ncreated: 2026-09-04\ntags: [仕事, cian]\n---\n本文。\n"
        );
        // そして読み戻すとそのタグになる。実際に意味があるのはそこだけ。
        //
        let lines: Vec<String> = out.lines().map(String::from).collect();
        assert_eq!(front(&lines).tags, vec!["仕事".to_string(), "cian".into()]);

        // front matter が無い場合は作り、ノート本体はその下にそのまま残す。
        let out = set_tags("# 題\n本文。\n", &["あ".into()]);
        assert_eq!(out, "---\ntags: [あ]\n---\n# 題\n本文。\n");

        // front matter はあるがタグが無い場合、行はその*末尾*に足す。
        let out = set_tags("---\ntitle: x\n---\n本文。\n", &["あ".into()]);
        assert_eq!(out, "---\ntitle: x\ntags: [あ]\n---\n本文。\n");

        // リスト形式で書かれたタグは、その項目も一緒に置き換える ── 新しい行の
        // 後ろに `- ` の行が取り残されないように。
        let out = set_tags("---\ntags:\n  - 古い\n  - もっと古い\ntitle: x\n---\n本文。\n", &["新しい".into()]);
        assert_eq!(out, "---\ntags: [新しい]\ntitle: x\n---\n本文。\n");

        // 全部外したら空のリストになる。壊れた行にはならない。
        let out = set_tags("---\ntags: [あ]\n---\n本文。\n", &[]);
        assert_eq!(out, "---\ntags: []\n---\n本文。\n");
    }

    #[test]
    fn 画像はノートの隣に置かれ_リンクがそれを指す() {
        let d = tempfile::tempdir().unwrap();
        let note = d.path().join("段取り.md");
        std::fs::write(&note, "# 段取り\n").unwrap();

        let link = attach(&note, &[1, 2, 3], "PNG").unwrap();
        assert!(link.starts_with("attachments/段取り-"), "{link}");
        assert!(link.ends_with(".png"), "the extension is lowercased: {link}");
        // リンクはノートからの相対パスなので、フォルダから辿ればファイルに
        // 行き着く ── Mac でも、エクスプローラーでも、iPhone でも。
        assert_eq!(std::fs::read(d.path().join(&link)).unwrap(), vec![1, 2, 3]);

        // 衝突しうるのは同じミリ秒に 2 回のときだけで、時刻を入れているのはその
        // 場合だけに限るため。一般に 2 回作って衝突してはいけない。
        let again = attach(&note, &[4], "png").unwrap();
        assert!(std::fs::read(d.path().join(&link)).is_ok(), "the first is still there");
        assert!(std::fs::read(d.path().join(&again)).is_ok());

        // ファイルシステムが受け付けない名前が、ノートのタイトル経由で戻って
        // こないように ── ここでも `file_stem` を通す。
        let odd = d.path().join("a-b.md");
        std::fs::write(&odd, "x").unwrap();
        assert!(attach(&odd, &[1], "").unwrap().ends_with(".png"), "no extension means png");

        // 添付する中身が無ければ、リンク先になる空ファイルを書かずに拒否する。
        //
        assert!(attach(&note, &[], "png").is_err());
    }

    #[test]
    fn 新しいノートは_自分が名乗るとおりに読み戻せる() {
        let (name, body) = new_note("段取り", "2026-09-02", "2026-09-02 14:03:09");
        assert_eq!(name, "段取り.md");
        // この形の要点 ── `new_note` が書いたものを `front` が解釈できる。
        // この 2 つを別々に持つノートアプリは、どれもいつか食い違っている。
        let lines: Vec<String> = body.lines().map(|l| l.to_string()).collect();
        let f = front(&lines);
        assert_eq!(f.fields.get("title").map(String::as_str), Some("段取り"));
        assert_eq!(f.fields.get("created").map(String::as_str), Some("2026-09-02"));
        assert!(f.tags.is_empty(), "an empty list is empty, not one empty tag: {:?}", f.tags);
    }

    /// **amber が付けた符号は、題として出さない。**
    ///
    /// 依頼 174 で前書きから追い出した「書いた人が一度も言っていない名前」
    /// が、ファイル名から戻ってきていた ── 題を空のまま作ると、上に
    /// `2026-09-06 13-07-22` と出る。人が名づけたファイル名は、中身が
    /// 空でも題でいい（そこは人が言った名前だから）。
    #[test]
    fn 作った時刻のファイル名は_題にならない() {
        let d = tempfile::tempdir().unwrap();

        // amber が付けた符号 ── 中身が空なら、題は無い。
        let made = d.path().join("2026-09-06 13-07-22.md");
        std::fs::write(&made, "---\ncreated: 2026-09-06\n---\n\n").unwrap();
        assert_eq!(read(&made, 60).unwrap().title, "");

        // 一行書けば、それが題。
        std::fs::write(&made, "---\ncreated: 2026-09-06\n---\n\n牛乳\n").unwrap();
        assert_eq!(read(&made, 60).unwrap().title, "牛乳");

        // 人が名づけたファイル名は、中身が空でも題。
        let mine = d.path().join("買うもの.md");
        std::fs::write(&mine, "").unwrap();
        assert_eq!(read(&mine, 60).unwrap().title, "買うもの");

        // 似ているが人が付けた名前（秒が無い・文字が混じる）は、タイトルのまま。
        for name in ["2026-09-06.md", "2026-09-06 13-07.md", "2026-09-06 会議.md"] {
            let at = d.path().join(name);
            std::fs::write(&at, "").unwrap();
            assert_eq!(read(&at, 60).unwrap().title, name.trim_end_matches(".md"),
                       "{name} は人が名づけた名前です");
        }
    }

    #[test]
    fn 題を書かなかったノートには_題を書かない() {
        let (name, body) = new_note("   ", "2026-09-02", "2026-09-02 14:03:09");
        // **書いた人が一度も言っていない名前を、前書きに入れない。**
        // 前は作った時刻を `title:` に書き込んでいて、あとから「牛乳」と
        // 書いても題は時刻のまま残った ── 直すには前書きを開いて消すしか
        // なく、その前書きは書く画面に出していない。
        assert!(!body.contains("title:"), "題が書き込まれている: {body}");
        // タグの無いノートが、空の配列を持ち歩く理由も無い。
        assert!(!body.contains("tags:"), "空のタグが書き込まれている: {body}");
        // `created` は残す ── 人が読む名前ではなく、並べ替えが読む欄。
        assert!(body.contains("created: 2026-09-02\n"), "{body}");
        // **ファイル名はその瞬間のまま。** 日付だけだと、午後に3本作ると
        // 「-2」「-3」が付くだけで、どれがどれか分からない。`:` は落とす
        // （Windows で作れない）。
        assert_eq!(name, "2026-09-02 14-03-09.md");

        // 題を書いたなら、書いたとおりに入る。
        let (_, body) = new_note("段取り", "2026-09-02", "2026-09-02 14:03:09");
        assert!(body.contains("title: 段取り"), "{body}");
    }

    #[test]
    fn ファイルシステムが拒む題は_受け付ける名前に直される() {
        // スラッシュとコロンは、人がタイトルに何気なく打つ文字 ──
        // 日付・パス・比。
        assert_eq!(file_stem("2026/09/02 の予定"), "2026-09-02 の予定");
        assert_eq!(file_stem("a:b*c?d"), "a-b-c-d");
        // 連続したものはダッシュ 1 つにまとめ、名前の先頭には置かない。
        assert_eq!(file_stem("??  なぞ"), "なぞ");
        // エクスプローラーは末尾のドットと空白を削るので、表示される名前が
        // ディスク上の名前と食い違う。
        assert_eq!(file_stem("あとで書く. "), "あとで書く");
        // Windows のデバイス名は、拡張子を付けてもデバイス名のまま。
        assert_eq!(file_stem("CON"), "_CON");
        assert_eq!(file_stem("con.old"), "_con.old");
        assert_eq!(file_stem("console"), "console", "only the exact name is reserved");
        // 十分な長さで、文字の途中ではなく文字境界で切る ── 日本語 60 文字は
        // 180 バイトで、どの上限にも収まる。
        let long = "あ".repeat(200);
        assert_eq!(file_stem(&long).chars().count(), 60);
        // 使える文字が残らなかった場合、呼び出し側は日付にフォールバックする。
        assert_eq!(file_stem("///"), "");
    }

    fn ls(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn front_matter_は書いても読んでも往復する() {
        let f = front(&ls("---\ntitle: 段取り\ntags: [onenote, 2026]\n---\n# 段取り\n"));
        assert_eq!(f.get("title"), Some("段取り"));
        assert_eq!(f.tags, ["onenote", "2026"]);
        assert_eq!(f.lines, 4, "the block, fences included");

        let f = front(&ls("---\ntags:\n  - a\n  - b\nstatus: done\n---\nbody\n"));
        assert_eq!(f.tags, ["a", "b"]);
        assert_eq!(f.get("status"), Some("done"));
    }

    /// **front matter でない `---` を front matter として扱ってはいけない。**
    /// 途中にある水平線も、先頭にあって閉じられない文書も、どちらも普通の
    /// Markdown ── どちらかを飲み込めば、誰かのノートの冒頭を食べることに
    /// なる。
    #[test]
    fn 水平線は_front_matter_ではない() {
        assert_eq!(front(&ls("# title\n\n---\n\nbody\n")).lines, 0);
        assert_eq!(front(&ls("---\nnot closed\nbody\n")).lines, 0);
        assert_eq!(front(&ls("")).lines, 0);
    }

    #[test]
    fn タイトルは見つかるまで順に候補を下る() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("kickoff.md");

        std::fs::write(&p, "---\ntitle: 名前\n---\n# 見出し\n").unwrap();
        assert_eq!(read(&p, 40).unwrap().title, "名前");

        std::fs::write(&p, "# 見出し\n本文\n").unwrap();
        assert_eq!(read(&p, 40).unwrap().title, "見出し", "then the heading");

        // **そして書き出しの一行。** 「牛乳」とだけ書きたいときに題を
        // 付けさせるのは、書くことより多い。
        std::fs::write(&p, "本文だけ\n").unwrap();
        assert_eq!(read(&p, 40).unwrap().title, "本文だけ", "then the first line");

        // 行頭の記号は外す ── `- 牛乳` のタイトルは「牛乳」。
        std::fs::write(&p, "- 牛乳\n- パン\n").unwrap();
        let n = read(&p, 40).unwrap();
        assert_eq!(n.title, "牛乳");
        // **題になった行は、二行目に出さない。** 一行のノートが二行に見える。
        assert!(!n.excerpt.starts_with("牛乳"), "二度言っている: {}", n.excerpt);

        // チェックボックスの記号も、強調の記号も外す。
        std::fs::write(&p, "- [ ] **急ぎ**の用\n").unwrap();
        assert_eq!(read(&p, 40).unwrap().title, "急ぎの用");

        // 枠の中は題にしない ── `fn main() {` はノートの名前ではない。
        std::fs::write(&p, "```rust\nfn main() {}\n```\nあとがき\n").unwrap();
        assert_eq!(read(&p, 40).unwrap().title, "あとがき");

        // 長い一行は切る ── 段落をそのまま題にすると一覧の一行を食い尽くす。
        std::fs::write(&p, "あ".repeat(200)).unwrap();
        let t = read(&p, 40).unwrap().title;
        assert_eq!(t.chars().count(), 61, "60 文字＋…: {t}");

        // **ファイル名まで落ちるのは、中身がまだ何も無いときだけ。**
        // そのときは他に呼びようがない。
        std::fs::write(&p, "---\ncreated: 2026-09-06\n---\n\n").unwrap();
        assert_eq!(read(&p, 40).unwrap().title, "kickoff", "then the file name");
    }

    /// 一覧の 2 行目は、そのノートが*何についてのものか*を言う。だから何で
    /// できているか ── 見出し、コードブロック、front matter ── は除く。
    #[test]
    fn 抜粋は骨組みの部分を飛ばす() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.md");
        std::fs::write(&p, "---\ntitle: t\n---\n# 見出し\n\n本文の一行目。\n```\ncode\n```\n二行目。\n").unwrap();
        let n = read(&p, 40).unwrap();
        assert_eq!(n.excerpt, "本文の一行目。 二行目。");
    }

    #[test]
    fn 打たれた日付が_mtime_より優先される() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.md");
        std::fs::write(&p, "---\nupdated: 2020-01-02\n---\nx\n").unwrap();
        // 2020-01-02T00:00:00Z
        assert_eq!(read(&p, 40).unwrap().updated, Some(1_577_923_200));
        std::fs::write(&p, "x\n").unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let got = read(&p, 40).unwrap().updated.unwrap();
        assert!(got.abs_diff(now) < 60, "otherwise the file's own time");
    }

    #[test]
    fn epoch_の計算が正しい() {
        assert_eq!(date_secs("1970-01-01"), Some(0));
        assert_eq!(date_secs("2026-09-02"), Some(1_788_307_200));
        assert_eq!(date_secs("2026-09-02T10:11:12"), Some(1_788_307_200), "the day only");
        assert_eq!(date_secs("nonsense"), None);
        assert_eq!(date_secs("2026-13-01"), None, "no thirteenth month");
    }
}

/// 繰り返しを実行する ── テンプレートのノートから今日のぶんを作る。
///
/// 作られるのは `repeat` と `last` を除いたノート ── テンプレートではなく
/// タスク ── で、`created` はそれが表す日、タイトルの末尾にもその日を
/// 付ける。1 か月ぶん並べたときに、同じ言葉が 12 回ではなく日付の一覧として
/// 読めるように。
///
/// **amber がスケジュール実行するのではない。** iPhone はアプリを水曜の 9 時に
/// 起こしてファイルを書かせてくれない。通知は時間どおりに届き、コピーが
/// 作られるのは次にアプリを開いたとき ── `last` を見て。`last` があるのは
/// そのため。誰も持っていない時計をほのめかさず、はっきりそう書く。
/// 繰り返しが溜めているぶんをすべて作り、作ったことを記録する。
///
/// **記録することが要点。** コピーを作るのは簡単で、作ったことを憶えておくのが
/// 明日もう一度作るのを止める ── デスクトップ版の最初の版は前半しかやって
/// いなかったので、開くたびに増えていった。
/// folder said 「1 件作りました」 every single time. One function, called by
/// 両方のフロントエンドから使うので、「これは実行済みか」の答えが 1 つになる。
///
/// 作ったものを返す。繰り返しの無いノートは何も作らないが、それはエラーでは
/// ない ── ほとんどのノートは繰り返しを持たない。
pub fn catch_up(path: &Path, today: chrono::NaiveDate) -> anyhow::Result<Vec<PathBuf>> {
    let text = std::fs::read_to_string(path)?;
    let r = remind(&text);
    let Some((every, _, _)) = r.every else { return Ok(Vec::new()) };
    let owed = due_since(every, r.last, today);
    let mut made = Vec::new();
    let mut last = None;
    for day in owed {
        made.push(carry_out(path, day)?);
        last = Some(day);
    }
    if let Some(day) = last {
        // 読み直す ── `carry_out` はこのファイルに触っていないが、ほかの誰かが
        // 触ったかもしれない。フィールドはいまそこにある内容に対して書く。
        let now = std::fs::read_to_string(path)?;
        std::fs::write(path, set_field(&now, "last", Some(&day.to_string())))?;
    }
    Ok(made)
}

/// 与えた時点から見て、繰り返しが次に来るのはいつか。
///
/// **自前の時計を持たなければならないフロントエンドのため。** iPhone はこれを
/// iOS に渡して忘れるが、デスクトップ版にはそういう仕組みが無いので自分で
/// タイマーを張る ── そのタイマーを何時に張るかは「毎週水曜の 9 時」が
/// 何を意味するかの判断で、JavaScript ではなくここに属する。
///
/// `from` は含まない。1 回鳴った直後にもう一度呼べば、同じものではなく
/// 次のものを返す。
pub fn next_ring(
    every: Every,
    hour: u32,
    minute: u32,
    from: chrono::NaiveDateTime,
) -> Option<chrono::NaiveDateTime> {
    let day = from.date();
    match every {
        Every::Daily => {
            let today = day.and_hms_opt(hour, minute, 0)?;
            if today > from {
                Some(today)
            } else {
                day.succ_opt()?.and_hms_opt(hour, minute, 0)
            }
        }
        Every::Weekly(w) => {
            // ノート側は月曜を 0 とし、chrono も 0 とする
            // （`num_days_from_monday`）。この 2 つが一致している唯一の場所なので、
            // はっきり書いておく価値がある。
            use chrono::Datelike;
            let mut d = day;
            for _ in 0..8 {
                if d.weekday().num_days_from_monday() == w {
                    if let Some(at) = d.and_hms_opt(hour, minute, 0) {
                        if at > from {
                            return Some(at);
                        }
                    }
                }
                d = d.succ_opt()?;
            }
            None
        }
        Every::Monthly(want) => {
            let mut d = day;
            // 2 か月ぶん見れば次の 1 件は必ず見つかるし、30 日しかない月の 31 日も
            // 扱える ── どこに落ちるかを決めるのは `last_day` なので、ここは
            // 溜まったぶんを数える側と同じ問いを投げていて、2 つ目の判断を
            // 持たない。
            for _ in 0..64 {
                if due_on(want, d) {
                    if let Some(at) = d.and_hms_opt(hour, minute, 0) {
                        if at > from {
                            return Some(at);
                        }
                    }
                }
                d = d.succ_opt()?;
            }
            None
        }
    }
}

pub fn carry_out(
    template: &std::path::Path,
    on: chrono::NaiveDate,
) -> anyhow::Result<std::path::PathBuf> {
    let text = std::fs::read_to_string(template)?;
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let f = front(&lines);
    let title = f
        .fields
        .get("title")
        .cloned()
        .or_else(|| heading(&lines[f.lines..]))
        .unwrap_or_else(|| {
            template.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        });
    let day = on.format("%Y-%m-%d").to_string();

    let mut copy = set_field(&text, "repeat", None);
    copy = set_field(&copy, "last", None);
    copy = set_field(&copy, "remind", None);
    copy = set_field(&copy, "created", Some(&day));
    copy = set_field(&copy, "title", Some(&format!("{title} {day}")));

    let Some(dir) = template.parent() else {
        anyhow::bail!("保存場所が分かりません: {}", template.display())
    };
    let name = format!("{}.md", file_stem(&format!("{title} {day}")));
    let at = dir.join(&name);
    // 実行済み ── 別の端末か、`last` が書かれる前のこの端末によって。
    // もう一度やると、同じ日のものが 2 つ並ぶ。
    if at.exists() {
        return Ok(at);
    }
    std::fs::write(&at, copy)?;
    Ok(at)
}

// ---- 通知と繰り返し ------------------------------------------------------

/// 繰り返しの周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Every {
    Daily,
    /// 0 が月曜（`chrono::Weekday::num_days_from_monday` の数え方に合わせる）。
    Weekly(u32),
    /// 月の何日か。31 日が無い月では末日に落とす。飛ばさない ── 2 月を黙って
    /// 飛ばす毎月の繰り返しは、1 日早く来るものより悪い。
    ///
    Monthly(u32),
}

/// そのノートがいつ呼び出されたいか。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Remind {
    /// `remind: 2026-09-10 09:00` — once.
    pub once: Option<chrono::NaiveDateTime>,
    /// `remind: 2026-09-10` ── 時刻の無い一度きり（終日）。
    ///
    /// **書く側がこの形で書いている**（デスクトップ版の `calAdd`・iPhone の
    /// `Calendaring` ── 予定表が使えない端末で「終日」を選ぶと、ノートに
    /// 日付だけの `remind:` を置く）。読む側が時刻を必須にしていたので、
    /// **登録したはずの日に出ず**、作った日にノートとして出るだけだった
    /// （crmaine の紹介動画を撮っていて見つかった・2026-09-22）。
    ///
    /// `once` とは別に持つ ── **鳴らす時刻を勝手に決めない**。終日の予定を
    /// 朝の何時に知らせるかは本人が決めること（まだ決めていない）。
    pub day: Option<chrono::NaiveDate>,
    /// `repeat: weekly wed 09:00` ── 繰り返し。
    pub every: Option<(Every, u32, u32)>,
    /// `last: 2026-09-03` ── この繰り返しを最後に実行した日。ノートの中に持つのは、
    /// そのノートに関するほかのものが全部そこにあるから。そして 1 週間電源を
    /// 切っていた iPhone が、何を逃したかを自分で計算できる必要があるから。
    ///
    pub last: Option<chrono::NaiveDate>,
}

/// ノートの front matter から通知の設定を読む。
///
/// 日付ライブラリのパーサーではなく手書きにしてある ── 下の 3 つの形が人の
/// 打つもので、ほかに 11 通りも受け付けるパーサーは、驚き方を 11 通り
/// 受け入れることになる。
///
/// ```text
/// remind: 2026-09-10 09:00
/// repeat: daily 07:30
/// repeat: weekly wed 09:00
/// repeat: monthly 1 09:00
/// ```
pub fn remind(text: &str) -> Remind {
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    remind_lines(&lines)
}

/// 前書きから予定を読む。**行を渡す形** ── 頭だけ読んだものを、そのまま
/// 使い回せる（同じファイルを二度読まないため・依頼 470）。
pub fn remind_lines(lines: &[String]) -> Remind {
    let f = front(lines);
    let mut out = Remind::default();

    if let Some(v) = f.fields.get("remind") {
        out.once = when(v);
        // 時刻が無ければ、日付だけの終日として読む（`day` の註）。
        if out.once.is_none() {
            out.day = day(v.trim());
        }
    }
    if let Some(v) = f.fields.get("last") {
        out.last = day(v.trim());
    }
    if let Some(v) = f.fields.get("repeat") {
        let mut it = v.split_whitespace();
        let kind = it.next().unwrap_or("").to_ascii_lowercase();
        match kind.as_str() {
            "daily" => {
                if let Some((h, m)) = clock(it.next().unwrap_or("")) {
                    out.every = Some((Every::Daily, h, m));
                }
            }
            "weekly" => {
                let d = weekday(it.next().unwrap_or(""));
                if let (Some(d), Some((h, m))) = (d, clock(it.next().unwrap_or(""))) {
                    out.every = Some((Every::Weekly(d), h, m));
                }
            }
            "monthly" => {
                let d: Option<u32> = it.next().and_then(|s| s.parse().ok());
                if let (Some(d), Some((h, m))) = (d, clock(it.next().unwrap_or(""))) {
                    if (1..=31).contains(&d) {
                        out.every = Some((Every::Monthly(d), h, m));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// `2026-09-10 09:00` or `2026-09-10T09:00`.
fn when(s: &str) -> Option<chrono::NaiveDateTime> {
    let s = s.trim();
    let (d, t) = s.split_once(['T', ' '])?;
    let d = day(d)?;
    let (h, m) = clock(t)?;
    d.and_hms_opt(h, m, 0)
}

fn day(s: &str) -> Option<chrono::NaiveDate> {
    let mut it = s.trim().split('-');
    let y: i32 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    chrono::NaiveDate::from_ymd_opt(y, m, d)
}

fn clock(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    (h < 24 && m < 60).then_some((h, m))
}

/// `mon`…`sun` と、日本語で実際に打たれる 1 文字の曜日。
fn weekday(s: &str) -> Option<u32> {
    let s = s.trim().to_ascii_lowercase();
    let names = [
        ("mon", "月"), ("tue", "火"), ("wed", "水"), ("thu", "木"),
        ("fri", "金"), ("sat", "土"), ("sun", "日"),
    ];
    names.iter().position(|(en, ja)| s.starts_with(en) || s == *ja).map(|i| i as u32)
}

/// `last` から `today` までに繰り返しが来た日。`today` も含む。
/// today.
///
/// **溜まったぶんを数えられることが重要。** 1 週間電源を切っていた iPhone も、
/// 開かれなかったアプリも、何を逃したか言えなければならない ── そうでないと
/// 毎週の繰り返しが、黙って「たまたまアプリを開いたとき」になる。
///
/// 上限を設けている。`last` が 2 年前のノートが作るべきなのは数件であって、
/// 700 件ではない。
pub fn due_since(
    every: Every,
    last: Option<chrono::NaiveDate>,
    today: chrono::NaiveDate,
) -> Vec<chrono::NaiveDate> {
    use chrono::Datelike;
    let from = match last {
        // 一度も実行していない場合、今日がその日に当たるなら今日も数える。
        None => today,
        Some(l) => l.succ_opt().unwrap_or(today),
    };
    let mut out = Vec::new();
    let mut d = from;
    while d <= today && out.len() < 32 {
        let hit = match every {
            Every::Daily => true,
            Every::Weekly(w) => d.weekday().num_days_from_monday() == w,
            Every::Monthly(day) => due_on(day, d),
        };
        if hit {
            out.push(d);
        }
        let Some(next) = d.succ_opt() else { break };
        d = next;
    }
    out
}

/// 毎月の繰り返しが、その日に当たるか。
///
/// 30 日しかない月の 31 日は、その月の末日に落とす ── そうしないと
/// 「毎月31日」が黙って「7 か月だけ」になる。**答えは一つ** ── 取りこぼしを
/// 拾うときも、次はいつかを訊くときも、ここに訊く。
fn due_on(want: u32, d: chrono::NaiveDate) -> bool {
    use chrono::Datelike;
    d.day() == want.min(last_day(d.year(), d.month()))
}

fn last_day(year: i32, month: u32) -> u32 {
    let (y, m) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    chrono::NaiveDate::from_ymd_opt(y, m, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| chrono::Datelike::day(&d))
        .unwrap_or(28)
}

/// 検索に一致したノートと、そのノートについて最初に見せるべき行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: std::path::PathBuf,
    /// 表示と同じく 1 始まり。どの行にも一致しなかったときは 0。
    pub line: usize,
    pub text: String,
}

/// ノートを本文ごと探す。**一本につき一行**。
///
/// 前は cian の `search`（総当たりの grep）を借りていた。借りるのをやめた
/// 理由は二つ。**ひとつ、あちらは `cloud` を引きずる** ── ノートのアプリが
/// OneDrive のプレースホルダ判定を積む理由が無い。**ふたつ、探し方が違った** ──
/// あちらは打った文字列をそのまま含むかを見る。こちらは [`hits`] を通すので、
/// デスクトップ版の `/` 絞り込みと同じ **AND と OR**（`OR` / `or` / `|` / `｜`）が効く。
/// 同じ言葉で探して同じものが出る、が二つの前端の間で成り立つ。
///
/// 探す先は題・タグ・本文。一行に全部の語が揃っている必要は無いので、
/// 見せる行は「どれか一語を含む最初の行」── 揃っていないから出せない、より
/// 一行でも見せたほうが「なぜ当たったか」に近い。
pub fn find(
    dir: &std::path::Path,
    query: &str,
    limit: usize,
    limits: crate::survey::Limits,
    stop: &std::sync::atomic::AtomicBool,
) -> Vec<Hit> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    // 抜粋を切るのに使う語 ── **落とす語（`-`）は要らない**。
    // 「無い語」の周りは切り出せない。
    let words: Vec<String> = terms(query)
        .into_iter()
        .flatten()
        .filter(|t| !t.not)
        .map(|t| t.word)
        .collect();
    let (found, _) = list(dir, limits, stop);
    let mut out = Vec::new();
    for f in found {
        if out.len() >= limit || stop.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        let Ok(file) = crate::text::read(&f.note.path) else { continue };
        let body = file.lines.join("\n");
        let hay = format!("{}\n{}\n{}", f.note.title, f.note.tags.join(" "), body);
        if !hits(&hay, query) {
            continue;
        }
        let (line, text) = file
            .lines
            .iter()
            .enumerate()
            .find(|(_, l)| {
                let low = l.to_lowercase();
                words.iter().any(|w| low.contains(&w.to_lowercase()))
            })
            .map(|(i, l)| (i + 1, l.trim().to_string()))
            .unwrap_or((0, String::new()));
        out.push(Hit { path: f.note.path.clone(), line, text });
    }
    out
}

#[cfg(test)]
mod find_tests {
    use super::*;

    fn write(dir: &std::path::Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn 本文とタグと題から探す() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.md", "---\ntitle: 段取り\ntags: [仕事]\n---\n明日は会議。\n");
        write(d.path(), "b.md", "# 買い物\n牛乳とパン\n");
        let stop = std::sync::atomic::AtomicBool::new(false);
        let lim = crate::survey::Limits::default();

        let r = find(d.path(), "会議", 20, lim, &stop);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].text, "明日は会議。");
        assert_eq!(r[0].line, 5, "front matter を数えた行番号であること");

        // 題からも、タグからも当たる。
        assert_eq!(find(d.path(), "段取り", 20, lim, &stop).len(), 1);
        assert_eq!(find(d.path(), "仕事", 20, lim, &stop).len(), 1);
    }

    #[test]
    fn and_と_or_が効く() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.md", "# 一\n牛乳とパン\n");
        write(d.path(), "b.md", "# 二\n牛乳だけ\n");
        let stop = std::sync::atomic::AtomicBool::new(false);
        let lim = crate::survey::Limits::default();
        // AND ── 両方ある一本だけ。**借りていた grep はこれができなかった。**
        assert_eq!(find(d.path(), "牛乳 パン", 20, lim, &stop).len(), 1);
        // OR ── どちらかで二本とも。
        assert_eq!(find(d.path(), "パン OR だけ", 20, lim, &stop).len(), 2);
    }

    #[test]
    fn 空の問いでは何も返さない() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.md", "# 一\n本文\n");
        let stop = std::sync::atomic::AtomicBool::new(false);
        assert!(find(d.path(), "   ", 20, crate::survey::Limits::default(), &stop).is_empty());
    }
}
