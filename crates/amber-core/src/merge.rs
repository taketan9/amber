//! 同じノートを、二人が同時に書いたとき。**黙って混ぜる。**
//!
//! 使う場所は二つ ── クラウド越しに合わせたときの `sync::Step::Clash` と、
//! 家族と分けている棚で両方が同じノートを書いたとき。どちらも「どちらかを
//! 捨てる」という問いにしない。
//!
//! # 決めてあること
//!
//! **迷ったら残す。** 同じ場所を二人が別々に書き換えたら、**両方置く** ──
//! 消えたものは取り返せないが、余分に残ったものは読んで消せる。
//!
//! **ノートには何も書き足さない。** `<<<<<<<` のような印を本文に入れない
//! ── ノートはただの Markdown で、GitHub でも VS Code でも開く。印を入れる
//! と、混ぜた日から**そのノートは amber でしか読めない形**になる。
//! 代わりに「どの行が向こうから来たか」を**返り値で**言う（`Merged::came`）
//! ── 画面はそれを見て色を差せるし、何も見なければただの Markdown のまま。
//!
//! # 時刻を見ない
//!
//! どちらが新しいかは決めない。**三つの姿を比べるだけ** ── 分かれる前
//! （`was`）・こちら（`ours`）・向こう（`theirs`）。時計のずれた端末が
//! 毎回勝つ、という負け方をしない。

/// 混ぜた結果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Merged {
    /// 混ざった本文。
    pub text: String,
    /// **向こうから来た行**（`text` の中の行番号・0 起点）。
    /// 画面はここに印を付けられる ── 本文には何も書いていない。
    pub came: Vec<usize>,
    /// 同じ場所を二人が別々に書き換えて、**両方残した**行
    /// （`came` にも入る）。人が読んで、要らないほうを消す。
    pub both: Vec<usize>,
}

impl Merged {
    /// 人の目が要るか ── 両方残した場所があるかどうか。
    pub fn needs_eyes(&self) -> bool {
        !self.both.is_empty()
    }
}

/// 分かれる前・こちら・向こう を混ぜる。
///
/// 行で見る。**字の中までは踏み込まない** ── 一行の中で二人が別の語を
/// 直したとき、字で混ぜると「どちらの文でもない一文」ができる。行ごと
/// 両方残せば、読んだ人が選べる。
pub fn merge(was: &str, ours: &str, theirs: &str) -> Merged {
    let a: Vec<&str> = was.lines().collect();
    let b: Vec<&str> = ours.lines().collect();
    let c: Vec<&str> = theirs.lines().collect();

    // 片方が手つかずなら、混ぜる話にならない。
    //
    // **こちらが手つかずでも、下の道を通す。** 丸ごと向こうを採ると
    // 楽だが、そのとき `came` が**全行**になる ── 一行足されただけの
    // 百行のノートが、百行ぶん「向こうから来た」ことになり、印が
    // 何も言わなくなる。下を通せば、来た行だけに付く。
    if a == c {
        return Merged { text: ours.to_string(), ..Default::default() };
    }
    if b == c {
        // 二人が**同じ直し**をした。片方だけ採ればいい。
        return Merged { text: ours.to_string(), ..Default::default() };
    }

    let mut out: Vec<String> = Vec::new();
    let mut came: Vec<usize> = Vec::new();
    let mut both: Vec<usize> = Vec::new();

    // 分かれる前の行のうち、**両側で残っているもの**が繋ぎ目になる。
    let ab = pairs(&a, &b);
    let ac = pairs(&a, &c);
    let anchors = shared(&ab, &ac);

    let (mut ia, mut ib, mut ic) = (0usize, 0usize, 0usize);
    for &(pa, pb, pc) in &anchors {
        weave(
            &a[ia..pa], &b[ib..pb], &c[ic..pc],
            &mut out, &mut came, &mut both,
        );
        out.push(a[pa].to_string());
        ia = pa + 1;
        ib = pb + 1;
        ic = pc + 1;
    }
    weave(&a[ia..], &b[ib..], &c[ic..], &mut out, &mut came, &mut both);

    Merged { text: join(&out, ours, theirs), came, both }
}

/// 繋ぎ目のあいだの、三つの切れはし。
fn weave(
    a: &[&str], b: &[&str], c: &[&str],
    out: &mut Vec<String>, came: &mut Vec<usize>, both: &mut Vec<usize>,
) {
    if b == c {
        // 二人が同じことをした（何もしなかった、も含む）。
        push(out, b, None);
    } else if a == b {
        // こちらは触っていない ── 向こうのを入れる。
        push(out, c, Some(came));
    } else if a == c {
        // 向こうが触っていない ── こちらのまま。
        push(out, b, None);
    } else {
        // **両方が触った。両方残す。**
        //
        // こちらを先に置く ── 自分の書いたものが上にあるほうが、
        // 何が起きたのか読み取りやすい。
        push(out, b, None);
        let from = out.len();
        push(out, c, Some(came));
        for n in from..out.len() {
            both.push(n);
        }
    }
}

fn push(out: &mut Vec<String>, rows: &[&str], mut mark: Option<&mut Vec<usize>>) {
    for r in rows {
        if let Some(m) = mark.as_deref_mut() {
            m.push(out.len());
        }
        out.push((*r).to_string());
    }
}

/// 行を繋ぎ直す。**終わりの改行は、元にあったなら残す。**
///
/// `lines()` は最後の改行を落とすので、そのまま繋ぐと保存のたびに
/// ファイルの尻から改行が一つ消える ── 何度か混ぜたノートだけ、
/// ほかと形が違うことになる。
fn join(rows: &[String], ours: &str, theirs: &str) -> String {
    let mut s = rows.join("\n");
    if ours.ends_with('\n') || theirs.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// `a` の行が `b` のどこに残っているか（いちばん長い共通の並び）。
///
/// **前と後ろの揃っているところを先に削る。** ノートの直しはたいてい
/// 真ん中の数行で、そこを削れば表は一気に小さくなる。削らずに全部を
/// 表にすると、千行のノートで百万個の升を数えることになる。
fn pairs(a: &[&str], b: &[&str]) -> Vec<(usize, usize)> {
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let rest = a.len().min(b.len()) - head;
    let tail = a[head..].iter().rev().zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y).count().min(rest);

    let mut out: Vec<(usize, usize)> = (0..head).map(|i| (i, i)).collect();
    let (x, y) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);

    // **大きすぎたら、真ん中は諦める。** 諦めた先は「両方が触った」に
    // なり、両方残る ── 遅くなるより、余分に残るほうがまし。
    if x.len().saturating_mul(y.len()) <= 4_000_000 {
        let mut grid = vec![0u32; (x.len() + 1) * (y.len() + 1)];
        let w = y.len() + 1;
        for i in (0..x.len()).rev() {
            for j in (0..y.len()).rev() {
                grid[i * w + j] = if x[i] == y[j] {
                    grid[(i + 1) * w + j + 1] + 1
                } else {
                    grid[(i + 1) * w + j].max(grid[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0usize, 0usize);
        while i < x.len() && j < y.len() {
            if x[i] == y[j] {
                out.push((head + i, head + j));
                i += 1;
                j += 1;
            } else if grid[(i + 1) * w + j] >= grid[i * w + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }

    for k in 0..tail {
        out.push((a.len() - tail + k, b.len() - tail + k));
    }
    out
}

/// 両側で残っている行だけを、繋ぎ目にする。
fn shared(ab: &[(usize, usize)], ac: &[(usize, usize)]) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < ab.len() && j < ac.len() {
        match ab[i].0.cmp(&ac[j].0) {
            std::cmp::Ordering::Equal => {
                out.push((ab[i].0, ab[i].1, ac[j].1));
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &[&str]) -> String {
        s.join("\n") + "\n"
    }

    #[test]
    fn different_places_both_land() {
        // 二人が別の場所を書いた ── どちらも残る。これが**ふだんの姿**で、
        // 家族と買い物リストを分けていれば、たいていこうなる。
        let was = t(&["# 買い物", "", "- 牛乳", "- パン"]);
        let ours = t(&["# 買い物", "", "- 牛乳", "- パン", "- 卵"]);
        let theirs = t(&["# 買い物", "", "- 牛乳 2本", "- パン"]);

        let m = merge(&was, &ours, &theirs);
        assert_eq!(m.text, t(&["# 買い物", "", "- 牛乳 2本", "- パン", "- 卵"]));
        // 「牛乳 2本」は向こうから来た。
        assert_eq!(m.came, vec![2]);
        assert!(!m.needs_eyes());
    }

    #[test]
    fn the_same_line_from_both_keeps_both() {
        // **同じ行を、二人が別々に書き換えた。** どちらかを捨てない ──
        // 捨てられたほうは、書いた人には二度と見えない。
        let was = t(&["集合は 10 時"]);
        let ours = t(&["集合は 11 時"]);
        let theirs = t(&["集合は 12 時"]);

        let m = merge(&was, &ours, &theirs);
        assert_eq!(m.text, t(&["集合は 11 時", "集合は 12 時"]));
        assert_eq!(m.came, vec![1]);
        assert_eq!(m.both, vec![1]);
        assert!(m.needs_eyes(), "人が読んで、要らないほうを消す");
    }

    #[test]
    fn a_line_deleted_here_and_written_there_is_kept() {
        // **迷ったら残す。** こちらで消し、向こうで書き足した行は残る。
        let was = t(&["- 牛乳", "- パン"]);
        let ours = t(&["- パン"]);
        let theirs = t(&["- 牛乳 2本", "- パン"]);

        let m = merge(&was, &ours, &theirs);
        assert!(m.text.contains("牛乳 2本"), "書いたほうが残る: {:?}", m.text);
    }

    #[test]
    fn one_side_untouched_takes_the_other() {
        let was = t(&["ひとつ", "ふたつ"]);
        let ours = was.clone();
        let theirs = t(&["ひとつ", "ふたつ", "みっつ"]);

        let m = merge(&was, &ours, &theirs);
        assert_eq!(m.text, theirs);
        // **来た行にだけ印が付く。** 触っていない二行には付かない ──
        // 全行に付けると、印は何も言わなくなる。
        assert_eq!(m.came, vec![2]);
        assert_eq!(m.text.lines().nth(2), Some("みっつ"));

        // 逆も。**向こうが触っていないなら、印は付かない。**
        let m = merge(&was, &theirs, &was);
        assert_eq!(m.text, theirs);
        assert!(m.came.is_empty());
    }

    #[test]
    fn the_same_edit_on_both_sides_is_not_a_clash() {
        // 二人が同じ直しをした（片方が先に直して、もう片方も同じ形に）。
        let was = t(&["集合は 10 時"]);
        let same = t(&["集合は 11 時"]);
        let m = merge(&was, &same, &same);
        assert_eq!(m.text, same);
        assert!(m.came.is_empty(), "二重に置かない");
        assert!(!m.needs_eyes());
    }

    #[test]
    fn the_last_newline_survives() {
        // `lines()` は終わりの改行を落とす ── 混ぜるたびに尻の改行が
        // 一つ消えると、何度か混ぜたノートだけ形が違うことになる。
        let was = "あ\nい\n";
        let m = merge(was, "あ\nい\nう\n", "あ\nい\n");
        assert!(m.text.ends_with('\n'));

        // 元から無いなら、足さない。
        let m = merge("あ", "あ\nい", "あ");
        assert!(!m.text.ends_with('\n'), "{:?}", m.text);
    }

    #[test]
    fn empty_sides_do_not_panic() {
        assert_eq!(merge("", "", "").text, "");
        assert_eq!(merge("", "あ\n", "").text, "あ\n");
        assert_eq!(merge("あ\n", "", "").text, "");
        let m = merge("あ\n", "", "い\n");
        assert!(m.text.contains('い'), "消しと書き足しなら、書いたほうを残す");
    }

    #[test]
    fn a_note_with_a_diagram_survives_a_merge() {
        // 図は行のかたまり ── 触っていない側の図が崩れないこと。
        let was = t(&["# 図", "", "```mermaid", "flowchart LR", "  A --> B", "```", "", "あと"]);
        let ours = t(&["# 図", "", "```mermaid", "flowchart LR", "  A --> B", "```", "", "あと", "追記"]);
        let theirs = t(&["# 図と字", "", "```mermaid", "flowchart LR", "  A --> B", "```", "", "あと"]);

        let m = merge(&was, &ours, &theirs);
        assert!(m.text.contains("```mermaid\nflowchart LR\n  A --> B\n```"), "{:?}", m.text);
        assert!(m.text.contains("# 図と字"));
        assert!(m.text.contains("追記"));
        assert!(!m.needs_eyes());
    }

    #[test]
    fn the_marked_rows_point_at_the_merged_text() {
        // 返した行番号が、返した本文の行を指していること ──
        // ずれていると、画面は**別の行**に印を差す。
        let was = t(&["a", "b", "c"]);
        let ours = t(&["a", "b", "c", "ours"]);
        let theirs = t(&["a", "THEIRS", "c"]);

        let m = merge(&was, &ours, &theirs);
        let rows: Vec<&str> = m.text.lines().collect();
        for &n in &m.came {
            assert!(n < rows.len(), "行番号が本文の外: {n}");
        }
        assert_eq!(m.came.iter().map(|&n| rows[n]).collect::<Vec<_>>(), vec!["THEIRS"]);
    }

    #[test]
    fn a_very_long_note_still_answers() {
        // 諦める側に落ちても、**落ちずに何か返す**こと（両方残る）。
        let was: String = (0..3000).map(|i| format!("行 {i}\n")).collect();
        let ours = was.replace("行 0\n", "こちら\n");
        let theirs = was.replace("行 2999\n", "向こう\n");
        let m = merge(&was, &ours, &theirs);
        assert!(m.text.contains("こちら"));
        assert!(m.text.contains("向こう"));
    }
}
