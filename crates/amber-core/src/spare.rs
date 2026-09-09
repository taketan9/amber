//! **ノートから使われていない画像。**
//!
//! `attachments/` に置かれた画像は、貼ったノートが消えても・その行だけ
//! 消しても、そこに残る。一枚ずつは小さいが、**消す道がどこにも無い**
//! ので、使っているうちにフォルダだけが重くなる。
//!
//! # 数えるだけ。消さない。
//!
//! ここが返すのは「どのノートからも指されていない画像」の一覧で、
//! 消すのは呼んだ側（窓はゴミ箱へ入れる）。**戻せる形で消す**のは
//! ノートと同じ扱いにしたいから ── 見誤って消したときに、取り返しが
//! つかないのがいちばん悪い。
//!
//! # 読めないノートが一本でもあったら、言う
//!
//! 「使われていない」は**ぜんぶのノートを読み切って初めて言えること**。
//! 一本でも読めなければ、その一本が指していた画像を「使われていない」と
//! 呼んでしまう。読めなかったものは `unsure` で返し、呼んだ側はそれを
//! 人に見せる ── 黙って少なく数えるのがいちばん危ない。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 使われていない画像、一枚。
#[derive(Debug, Clone)]
pub struct Spare {
    pub path: PathBuf,
    /// 置き場所からの道（`attachments/段取り-123.png`）。
    pub rel: String,
    pub bytes: u64,
    /// 最後に触られた時刻（秒）。
    pub when: u64,
    /// 名前から見て、もとはどのノートのものらしいか（分からなければ空）。
    pub note: String,
}

/// 画像として置かれる形。**知らない形は数えない** ── `attachments/` に
/// 人が置いた別のものを、amber が勝手に「使われていない」と呼ばない。
const KINDS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "heic", "bmp", "svg"];

fn is_picture(name: &str) -> bool {
    name.rsplit_once('.')
        .map(|(_, e)| KINDS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// 一本のノートの字から、**そこが指している行き先**をぜんぶ拾う。
///
/// `](…)` と `src="…"` の両方 ── 読む面は `<img>` を書き戻すことがあり、
/// 片方しか見ないと、その画像を「使われていない」と数える。
///
/// **枠（コード）の中も拾う。** 出はしないが、人がそこに道を書いている
/// なら消していい理由にはならない ── 迷ったら残す。
fn targets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == ']' && i + 1 < b.len() && b[i + 1] == '(' {
            let mut j = i + 2;
            let mut deep = 0;
            let mut got = String::new();
            while j < b.len() {
                match b[j] {
                    '(' => { deep += 1; got.push('('); }
                    ')' if deep == 0 => break,
                    ')' => { deep -= 1; got.push(')'); }
                    '\n' => break,
                    c => got.push(c),
                }
                j += 1;
            }
            out.push(got);
            i = j;
        } else if b[i] == 's' && text[byte_at(&b, i)..].starts_with("src=") {
            let rest = &text[byte_at(&b, i) + 4..];
            let quote = rest.chars().next();
            if let Some(q) = quote.filter(|c| *c == '"' || *c == '\'') {
                if let Some(end) = rest[1..].find(q) {
                    out.push(rest[1..1 + end].to_string());
                }
            }
        }
        i += 1;
    }
    out
}

/// 文字の位置を、バイトの位置に。
fn byte_at(chars: &[char], upto: usize) -> usize {
    chars[..upto].iter().map(|c| c.len_utf8()).sum()
}

/// 行き先を、ファイルの道に直す。**よそ行きは数えない。**
fn resolve(from: &Path, target: &str) -> Option<PathBuf> {
    let t = target.trim();
    // `![](a.png "説明")` の説明を落とす。
    let t = t.split_whitespace().next().unwrap_or(t);
    if t.is_empty() { return None; }
    let low = t.to_lowercase();
    if low.starts_with("http://") || low.starts_with("https://")
        || low.starts_with("data:") || low.starts_with("mailto:")
        || t.starts_with('#')
    {
        return None;
    }
    let t = t.split('#').next().unwrap_or(t);
    let t = t.split('?').next().unwrap_or(t);
    let t = percent(t);
    if t.is_empty() { return None; }
    let at = if t.starts_with('/') { PathBuf::from(&t) } else { from.join(&t) };
    Some(tidy(&at))
}

/// `%E6%AC%A1` を字に戻す。**戻せなければ、そのまま** ── 半端に戻すより
/// 当たらないほうがよい（当たらなければ「使っている」側に倒れる）。
fn percent(s: &str) -> String {
    if !s.contains('%') { return s.to_string(); }
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(n) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(n);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// `a/./b` と `a/x/../b` を畳む。**ファイルを触らずに畳む** ──
/// `canonicalize` は無いファイルで落ちるし、同期の途中のものが無い。
fn tidy(at: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in at.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => { out.pop(); }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 使われていない画像と、読めなかったノート。
pub struct Found {
    pub spare: Vec<Spare>,
    /// 読めなかったノートの道（あるうちは、数が少なく出ている）。
    pub unsure: Vec<String>,
}

/// 置き場所の下ぜんぶを見て、どのノートからも指されていない画像を返す。
pub fn find(root: &Path, rows: &[crate::survey::Row]) -> Found {
    let mut used: HashSet<PathBuf> = HashSet::new();
    let mut unsure: Vec<String> = Vec::new();
    let mut dirs: HashSet<PathBuf> = HashSet::new();
    dirs.insert(root.to_path_buf());

    for r in rows {
        if r.is_dir {
            dirs.insert(r.path.clone());
            continue;
        }
        let name = r.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let md = name.to_lowercase();
        if !(md.ends_with(".md") || md.ends_with(".markdown")) {
            continue;
        }
        let Some(here) = r.path.parent() else { continue };
        let text = match std::fs::read_to_string(&r.path) {
            Ok(t) => t,
            Err(_) => match crate::text::read(&r.path) {
                Ok(f) => f.lines.join("\n"),
                Err(_) => {
                    unsure.push(r.rel.clone());
                    continue;
                }
            },
        };
        for t in targets(&text) {
            if let Some(at) = resolve(here, &t) {
                used.insert(at);
            }
        }
    }

    let mut spare = Vec::new();
    for d in &dirs {
        let at = d.join("attachments");
        let Ok(rd) = std::fs::read_dir(&at) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !is_picture(&name) {
                continue;
            }
            let path = tidy(&e.path());
            if used.contains(&path) {
                continue;
            }
            let meta = e.metadata().ok();
            let when = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let note = name
                .rsplit_once('-')
                .map(|(head, _)| head.to_string())
                .unwrap_or_default();
            let rel = path
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| name.clone());
            spare.push(Spare {
                path,
                rel,
                bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                when,
                note,
            });
        }
    }
    // 新しいものが上 ── 消していいか迷うのは、たいてい最近のもの。
    spare.sort_by(|a, b| b.when.cmp(&a.when).then(a.rel.cmp(&b.rel)));
    Found { spare, unsure }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    fn walk(root: &Path) -> Vec<crate::survey::Row> {
        let stop = std::sync::atomic::AtomicBool::new(false);
        let limits = crate::survey::Limits { depth: 6, rows: 500, hidden: false, ..Default::default() };
        crate::survey::survey(root, limits, &stop).rows
    }

    #[test]
    fn only_the_pictures_nobody_points_at() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("attachments")).unwrap();
        let put = |name: &str, body: &str| std::fs::write(root.join(name), body).unwrap();
        let pic = |name: &str| std::fs::write(root.join("attachments").join(name), [0u8; 8]).unwrap();

        pic("段取り-1.png");
        pic("段取り-2.png");
        pic("空 白-3.png");
        pic("よそ-4.png");
        pic("メモ.txt");           // 画像ではない ── 数えない
        pic("段取り-5.png");

        put("段取り.md", "# 段取り\n![](attachments/段取り-1.png)\n");
        // 読む面が書き戻した形も拾う。
        put("写し.md", "<img src=\"attachments/段取り-2.png\">\n");
        // 逃がしてある名前も拾う。
        put("空.md", "![](attachments/%E7%A9%BA%20%E7%99%BD-3.png)\n");
        // よそ行きは数えない ── これで `よそ-4.png` が守られたりしない。
        put("外.md", "![](https://example.com/よそ-4.png)\n");

        let got = super::find(root, &walk(root));
        let mut names: Vec<&str> = got.spare.iter().map(|s| s.rel.as_str()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["attachments/よそ-4.png", "attachments/段取り-5.png"],
            "指されている三枚は残り、指されていない二枚だけが出る",
        );
        assert!(got.unsure.is_empty());
        let one = got.spare.iter().find(|s| s.rel.ends_with("段取り-5.png")).unwrap();
        assert_eq!(one.note, "段取り", "もとのノートの名前が読める");
        assert_eq!(one.bytes, 8);
    }

    /// **枠の中に書いてあっても、消さない。** 出はしないが、人がそこに
    /// 道を書いているなら、消していい理由にはならない。
    #[test]
    fn a_path_written_inside_a_fence_still_counts_as_used() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("attachments")).unwrap();
        std::fs::write(root.join("attachments").join("控え-1.png"), [0u8; 4]).unwrap();
        std::fs::write(
            root.join("書き方.md"),
            "# 書き方\n\n```\n![](attachments/控え-1.png)\n```\n",
        )
        .unwrap();

        let got = super::find(root, &walk(root));
        assert!(got.spare.is_empty(), "{:?}", got.spare.iter().map(|s| &s.rel).collect::<Vec<_>>());
    }
}
