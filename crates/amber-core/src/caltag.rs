//! **予定のメモ欄に置くタグ**（誰の用事か）。
//!
//! グループカレンダーの予定に「太郎の用事」「次郎の用事」と目印を付けたい。
//! 置き場所は**メモ欄のいちばん最後の、タグだけの行**にした。
//!
//! # なぜ題ではなくメモ欄なのか（本人・2026-09-13）
//!
//! 題に `#太郎` と書くと、Google カレンダーを横から見られたときに**氏名が
//! 画面に出る**。メモ欄なら月の表には出ず、予定を開いた人にだけ見える。
//!
//! # なぜ amber の中に対応表を持たないのか
//!
//! 予定を指す ID が当てにならない。EventKit の `eventIdentifier` は端末ごとに
//! 違う値になりうるし、端末をまたいで揃う `calendarItemExternalIdentifier` は
//! **繰り返し予定の全インスタンスで同じ**になる（「毎週水曜の当番の、今週だけ
//! 次郎」が書けない）。作り直された予定では対応表が孤児になり、**壊れても
//! 画面には何も出ない** ── タグが黙って消えるだけになる。
//!
//! メモ欄に書けば、タグは**予定そのものに付いて旅をする**。端末を替えても、
//! アプリを替えても付いてくる。ID の問題がそもそも存在しない。
//!
//! # 人が書いた文章には触らない
//!
//! メモ欄は人が文章を書く場所でもある。だから**読むのも書き換えるのも、
//! いちばん最後の一行だけ**。そこが「タグだけの行」でなければ、この予定に
//! タグは無い ── 本文の途中に出てくる `#1 の件` を拾わないための決まりでも
//! ある。行の途中を見はじめた瞬間、人の文章がタグとして読まれる。
//!
//! 唯一こちらが落とすのは**末尾の空行**。タグ行を置ける場所がそこしか無く、
//! 空行を残したままだと「最後の行」が二通りに読めてしまう。
//!
//! # 書き戻す前に、元と見比べること（呼ぶ側へ）
//!
//! `set` は**常に整形した文字列を返す** ── 末尾の空行が落ちるので、タグが
//! 何も変わっていなくても元と一字違うことがある。それをそのまま保存すると
//! **触っていない予定の更新時刻が動く**。
//!
//! amber はこの形のバグを一度踏んでいる（開いただけで保存され、同期先には
//! 「向こうが編集した」と見える）。だから呼ぶ側は、**`set` の返りが元と
//! 同じなら書かない**。ここでは判断できない ── 何が「元」かを知っているのは
//! 予定を持っている側だから。

/// 改行の形。**元のメモに合わせて返す** ── Windows や Exchange から来た
/// メモを LF で書き戻すと、触っていない行まで差分になる。
fn newline(notes: &str) -> &'static str {
    if notes.contains("\r\n") { "\r\n" } else { "\n" }
}

/// その行は「タグだけの行」か。
///
/// 空白で区切ったどの語も `#` で始まり、`#` のあとに一文字以上ある。
/// 語が一つも無い行（空行）は**タグ行ではない**。
fn only_tags(line: &str) -> bool {
    let mut any = false;
    for word in line.split_whitespace() {
        any = true;
        let rest = match word.strip_prefix('#') {
            Some(r) => r,
            None => return false,
        };
        if rest.is_empty() {
            return false;
        }
    }
    any
}

/// メモを「人の文章」と「タグ行」に割る。
///
/// 返すのは (本文の終わりの位置, タグ行) ── 本文はメモの先頭からその位置まで。
fn split(notes: &str) -> (usize, Option<&str>) {
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut at = 0usize;
    for line in notes.split('\n') {
        let clean = line.strip_suffix('\r').unwrap_or(line);
        lines.push((at, clean));
        at += line.len() + 1;
    }
    // 末尾の空行を飛ばして、最後の中身のある行を探す。
    let last = lines.iter().rposition(|(_, l)| !l.trim().is_empty());
    match last {
        Some(i) if only_tags(lines[i].1) => (lines[i].0, Some(lines[i].1)),
        Some(i) => (lines[i].0 + lines[i].1.len(), None),
        None => (0, None),
    }
}

/// メモ欄の最後の行が持てるもの ── **だれの用事か**のほかに、**色**と
/// **全員に通知するか**（依頼 701・702・本人が 2026-10-31 に甲を選んだ）。
///
/// # なぜ同じ一行に置くのか
///
/// 行を増やすと「どちらが新しいか」を決めなければならなくなる。一行なら、
/// 読む・書く・消すがこれまでと同じ一か所で済む。
///
/// # 人の名前と、amber が置いたものを見分ける
///
/// amber が置くものは `#色:カーマイン` のように `:` の前に鍵を書く。**知っている
/// 鍵だけを取り、知らないものは人の名前として残す** ── 人が `#A:B` と
/// 書いていたら、それは人が書いた字である。逆にしたら、人のタグが黙って
/// 消える。
///
/// # 見えることを承知の上で置いている
///
/// この行は Google カレンダーの画面にも出る。それでもここに置くと本人が
/// 決めた（2026-10-31）── **色と通知が予定そのものに付いて旅をする**ので、
/// 同期した相手の amber でも同じ色・同じ通知になる。amber の中に対応表を
/// 持つ案は採らなかった（この書き出しの、ID の話と同じ理由）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    /// だれの用事か。
    pub tags: Vec<String>,
    /// 色の名前（`カーマイン` など）。
    pub color: Option<String>,
    /// 全員に通知するなら、何分前かの言葉（`10分前` など）。
    /// 分に直すのは `crate::note::alarm_minutes`。
    pub alarm_all: Option<String>,
}

/// 色の鍵。
const COLOR_KEY: &str = "色:";
/// 全員に通知するかの鍵。
const ALARM_KEY: &str = "全員に通知:";

/// メモ欄の最後の行を読む。
pub fn marks(notes: &str) -> Marks {
    let (_, line) = split(notes);
    let mut out = Marks::default();
    let Some(line) = line else { return out };
    for word in line.split_whitespace() {
        let rest = word.trim_start_matches('#');
        if let Some(v) = rest.strip_prefix(COLOR_KEY) {
            if out.color.is_none() && !v.is_empty() {
                out.color = Some(v.to_string());
            }
            continue;
        }
        if let Some(v) = rest.strip_prefix(ALARM_KEY) {
            if out.alarm_all.is_none() && !v.is_empty() {
                out.alarm_all = Some(v.to_string());
            }
            continue;
        }
        if !rest.is_empty() && !out.tags.iter().any(|t| t == rest) {
            out.tags.push(rest.to_string());
        }
    }
    out
}

/// この予定に付いているタグ。**`#` は外して返す。**
///
/// 同じタグが二度書いてあれば、最初の一つだけ。順は書いてある順 ──
/// 並べ替えない（人が並べた順に意味があることがある）。
/// **amber が置いたもの（`#色:…`）は含まない。**
pub fn tags(notes: &str) -> Vec<String> {
    marks(notes).tags
}

/// タグ行を除いた、**人が書いた文章**。
pub fn body(notes: &str) -> &str {
    let (end, _) = split(notes);
    notes[..end].trim_end_matches(['\r', '\n'])
}

/// タグを書き換える。**触るのはタグ行だけ。**
///
/// - タグが空なら、タグ行を消す（本文だけが残る）。
/// - タグ行が既にあれば、差し替える。
/// - 無ければ、本文のうしろに**空行を一つ置いて**足す。
/// - 本文が空なら、タグ行だけを返す（頭に空行を置かない）。
pub fn set(notes: &str, tags: &[String]) -> String {
    // **色と通知は、そのまま残す。** タグだけを渡す呼び出しは前からあり
    // （デスクトップ版の `caltagset`）、それで色が消えると
    // **人を一人足しただけで、同期した全員の色が落ちる。**
    let mut m = marks(notes);
    m.tags = tags.to_vec();
    set_marks(notes, &m)
}

/// タグ・色・全員に通知するかを、まとめて書き換える。
///
/// 並べる順は**人の名前が先、amber が置くものがあと** ── 読むのは人なので、
/// 知りたいものが頭に来るほうがよい。何も無ければ、その行ごと消える。
pub fn set_marks(notes: &str, m: &Marks) -> String {
    let nl = newline(notes);
    let body = body(notes);
    let mut words: Vec<String> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for t in &m.tags {
        let name = t.trim().trim_start_matches('#').trim();
        if name.is_empty() || name.contains(char::is_whitespace) || seen.contains(&name) {
            continue;
        }
        // **amber の鍵と同じ形の名前は書かない** ── 書いたら、読み直したとき
        // 人の名前が色になる。
        if name.starts_with(COLOR_KEY) || name.starts_with(ALARM_KEY) {
            continue;
        }
        seen.push(name);
        words.push(format!("#{name}"));
    }
    for (key, value) in [(COLOR_KEY, &m.color), (ALARM_KEY, &m.alarm_all)] {
        let Some(v) = value.as_deref() else { continue };
        let v = v.trim();
        // **空白を含む値は書かない** ── 空白で区切って読むので、書いたら
        // 読み直せない。黙って壊れた行を書くより、落とす。
        if v.is_empty() || v.contains(char::is_whitespace) {
            continue;
        }
        words.push(format!("#{key}{v}"));
    }
    if words.is_empty() {
        return body.to_string();
    }
    let line = words.join(" ");
    if body.is_empty() {
        line
    } else {
        format!("{body}{nl}{nl}{line}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn タグだけの最終行がタグ行になる() {
        let memo = "買い物のあと、駅前で待ち合わせ。\n保険証を忘れずに。\n\n#太郎 #次郎";
        assert_eq!(tags(memo), v(&["太郎", "次郎"]));
        assert_eq!(body(memo), "買い物のあと、駅前で待ち合わせ。\n保険証を忘れずに。");
    }

    #[test]
    fn ハッシュを含むだけの文はタグ行ではない() {
        // **ここを取り違えると、人の文章がタグになる。**
        assert!(tags("#1 の件、よろしく").is_empty());
        assert!(tags("明日は #太郎 の参観日です").is_empty());
        assert!(tags("# 見出し").is_empty());
        assert!(tags("#").is_empty());
        assert_eq!(body("#1 の件、よろしく"), "#1 の件、よろしく");
    }

    #[test]
    fn 裸のハッシュは人の書いた文字なので残す() {
        // **`tags()` だけでは足りない。** `#` を「タグ行」と読んでも、
        // 名前が空なので `tags()` は空を返す ── 同じ顔をする。
        // 違いが出るのは `body()` で、ここが人の一文字を落とすかどうか。
        assert_eq!(body("#"), "#");
        assert_eq!(body("本文\n\n#"), "本文\n\n#");
        assert_eq!(set("#", &v(&["太郎"])), "#\n\n#太郎");
    }

    #[test]
    fn 空行はタグ行ではない() {
        // いまは空行を先に弾いてからしか呼ばないが、**呼ぶ側の都合に
        // 寄りかからない** ── 弾く場所が動いた日に、空行がタグ行になる。
        assert!(!only_tags(""));
        assert!(!only_tags("   "));
        assert!(only_tags("#太郎"));
    }

    #[test]
    fn メモが無ければタグも無い() {
        assert!(tags("").is_empty());
        assert!(tags("   \n\n  ").is_empty());
        assert_eq!(body(""), "");
    }

    #[test]
    fn メモ全体がタグ行一行のこともある() {
        assert_eq!(tags("#太郎"), v(&["太郎"]));
        assert_eq!(body("#太郎"), "");
    }

    #[test]
    fn タグを書いても_人が書いた内容には触らない() {
        let memo = "買い物のあと、駅前で待ち合わせ。\n保険証を忘れずに。";
        let out = set(memo, &v(&["太郎"]));
        assert_eq!(out, "買い物のあと、駅前で待ち合わせ。\n保険証を忘れずに。\n\n#太郎");
        assert_eq!(body(&out), memo);
    }

    #[test]
    fn 書き直すとタグ行だけが置き換わる() {
        let memo = "保険証を忘れずに。\n\n#太郎 #次郎";
        let out = set(memo, &v(&["花子"]));
        assert_eq!(out, "保険証を忘れずに。\n\n#花子");
        // 二度書いても増えない。
        assert_eq!(set(&out, &v(&["花子"])), out);
    }

    #[test]
    fn タグを空にすると_その行と直前の空行も消える() {
        let memo = "保険証を忘れずに。\n\n#太郎";
        assert_eq!(set(memo, &[]), "保険証を忘れずに。");
        // もともと無ければ、何も起きない。
        assert_eq!(set("保険証を忘れずに。", &[]), "保険証を忘れずに。");
        assert_eq!(set("", &[]), "");
    }

    #[test]
    fn 空のメモに付けたタグは_それだけで成り立つ() {
        assert_eq!(set("", &v(&["太郎"])), "#太郎");
        assert_eq!(set("   ", &v(&["太郎"])), "#太郎");
    }

    #[test]
    fn タグ行の形は往復しても変わらない() {
        // テキスト → タグ → テキスト → タグ で、同じものに戻る。
        for memo in [
            "",
            "#太郎",
            "本文だけ",
            "本文\n\n#太郎 #次郎",
            "本文\n二行目\n\n#家族",
        ] {
            let got = tags(memo);
            assert_eq!(set(memo, &got), memo.trim_end().to_string(), "{memo:?}");
        }
    }

    #[test]
    fn メモの末尾の改行は保たれる() {
        // **Exchange や Windows から来たメモを LF で書き戻さない** ──
        // 触っていない行まで差分になる。
        let memo = "保険証を忘れずに。\r\n二行目。";
        assert_eq!(set(memo, &v(&["太郎"])), "保険証を忘れずに。\r\n二行目。\r\n\r\n#太郎");
        assert_eq!(tags("本文\r\n\r\n#太郎 #次郎"), v(&["太郎", "次郎"]));
        assert_eq!(body("本文\r\n\r\n#太郎"), "本文");
    }

    #[test]
    fn 同じ名前を二度書いてもタグは一つ() {
        assert_eq!(tags("#太郎 #太郎 #次郎"), v(&["太郎", "次郎"]));
        assert_eq!(set("", &v(&["太郎", "太郎"])), "#太郎");
    }

    #[test]
    fn 書くときの先頭のハッシュは省ける() {
        // 呼ぶ側が `#太郎` と渡しても `太郎` と渡しても同じ。
        assert_eq!(set("", &v(&["#太郎"])), "#太郎");
        assert_eq!(set("", &v(&["  太郎  "])), "#太郎");
    }

    #[test]
    fn 空白を含む名前はタグにならない() {
        // 空白で区切って読むので、空白を含む名前は**書けない**。
        // 黙って壊れた行を書くより、落とす。
        assert_eq!(set("", &v(&["山田 太郎"])), "");
        assert_eq!(set("", &v(&["太郎", "山田 太郎"])), "#太郎");
    }

    #[test]
    fn 色と全員通知は_人の名前と同じ行に並ぶ() {
        // 依頼 701・702（甲）。人の名前が先、amber が置くものがあと。
        let m = Marks {
            tags: v(&["太郎"]),
            color: Some("カーマイン".into()),
            alarm_all: Some("10分前".into()),
        };
        let out = set_marks("駅前で待ち合わせ。", &m);
        assert_eq!(out, "駅前で待ち合わせ。\n\n#太郎 #色:カーマイン #全員に通知:10分前");
        // 読み直すと同じものに戻り、**人の名前に amber のものが混ざらない**。
        let got = marks(&out);
        assert_eq!(got, m);
        assert_eq!(tags(&out), v(&["太郎"]), "色は人の名前ではない");
        assert_eq!(body(&out), "駅前で待ち合わせ。");
    }

    #[test]
    fn 知らない鍵は_人の名前として残す() {
        // **人が `#A:B` と書いていたら、それは人が書いた字である。**
        // 逆にしたら、人のタグが黙って消える。
        let got = marks("#太郎 #担当:次郎 #色:シアン");
        assert_eq!(got.tags, v(&["太郎", "担当:次郎"]));
        assert_eq!(got.color.as_deref(), Some("シアン"));
        assert_eq!(got.alarm_all, None);
    }

    #[test]
    fn タグだけを渡す呼び出しで_色が落ちない() {
        // **これが落ちると、人を一人足しただけで同期した全員の色が消える。**
        // デスクトップ版の `caltagset` はタグだけを渡してくる（前からある）。
        let memo = "本文\n\n#太郎 #色:カーマイン #全員に通知:30分前";
        let out = set(memo, &v(&["太郎", "花子"]));
        assert_eq!(out, "本文\n\n#太郎 #花子 #色:カーマイン #全員に通知:30分前");
        // タグを空にしても、色と通知は残る（消すのは `set_marks`）。
        assert_eq!(set(memo, &[]), "本文\n\n#色:カーマイン #全員に通知:30分前");
    }

    #[test]
    fn 色をやめると_その言葉だけが消える() {
        let memo = "本文\n\n#太郎 #色:カーマイン";
        let out = set_marks(memo, &Marks { tags: v(&["太郎"]), ..Default::default() });
        assert_eq!(out, "本文\n\n#太郎");
        // 何も残らなければ、行ごと消える。
        assert_eq!(set_marks(memo, &Marks::default()), "本文");
    }

    #[test]
    fn 空白を含む色や言葉は書かない() {
        // 空白で区切って読むので、書いたら読み直せない。
        let m = Marks {
            tags: Vec::new(),
            color: Some("カー マイン".into()),
            alarm_all: Some("".into()),
        };
        assert_eq!(set_marks("", &m), "");
        // 鍵と同じ形の名前も書かない ── 書くと、読み直したとき色になる。
        let bad = Marks { tags: v(&["色:カーマイン"]), ..Default::default() };
        assert_eq!(set_marks("", &bad), "");
    }

    #[test]
    fn 色と通知の形は往復しても変わらない() {
        for memo in [
            "#色:アンバー",
            "#全員に通知:前日",
            "本文\n\n#太郎 #色:リーフ",
            "本文\n二行目\n\n#家族 #色:シアン #全員に通知:1時間前",
        ] {
            let got = marks(memo);
            assert_eq!(set_marks(memo, &got), memo.trim_end().to_string(), "{memo:?}");
        }
    }

    #[test]
    fn 末尾の空行は消えるが_ほかは消えない() {
        // タグ行を置ける場所がそこしか無いので、末尾の空行だけは落とす。
        assert_eq!(body("本文\n\n\n"), "本文");
        assert_eq!(set("本文\n\n\n", &v(&["太郎"])), "本文\n\n#太郎");
        // 途中の空行は、人の段落なので残す。
        assert_eq!(body("一段落\n\n二段落"), "一段落\n\n二段落");
    }
}
