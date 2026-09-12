//! **予定のメモ欄に置くタグ**（誰の用事か）。
//!
//! グループカレンダーの予定に「太郎の用事」「次郎の用事」と印を付けたい。
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
//! `set` は**いつも整えた字を返す** ── 末尾の空行が落ちるので、タグが
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

/// この予定に付いているタグ。**`#` は外して返す。**
///
/// 同じタグが二度書いてあれば、最初の一つだけ。順は書いてある順 ──
/// 並べ替えない（人が並べた順に意味があることがある）。
pub fn tags(notes: &str) -> Vec<String> {
    let (_, line) = split(notes);
    let mut out: Vec<String> = Vec::new();
    if let Some(line) = line {
        for word in line.split_whitespace() {
            let name = word.trim_start_matches('#').to_string();
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
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
    let nl = newline(notes);
    let body = body(notes);
    let mut seen: Vec<&str> = Vec::new();
    for t in tags {
        let name = t.trim().trim_start_matches('#').trim();
        if !name.is_empty() && !name.contains(char::is_whitespace) && !seen.contains(&name) {
            seen.push(name);
        }
    }
    if seen.is_empty() {
        return body.to_string();
    }
    let line = seen
        .iter()
        .map(|n| format!("#{n}"))
        .collect::<Vec<_>>()
        .join(" ");
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
    fn the_last_line_of_only_tags_is_the_tag_line() {
        let memo = "買い物のあと、駅前で待ち合わせ。\n保険証を忘れずに。\n\n#太郎 #次郎";
        assert_eq!(tags(memo), v(&["太郎", "次郎"]));
        assert_eq!(body(memo), "買い物のあと、駅前で待ち合わせ。\n保険証を忘れずに。");
    }

    #[test]
    fn a_sentence_that_merely_contains_a_hash_is_not_a_tag_line() {
        // **ここを取り違えると、人の文章がタグになる。**
        assert!(tags("#1 の件、よろしく").is_empty());
        assert!(tags("明日は #太郎 の参観日です").is_empty());
        assert!(tags("# 見出し").is_empty());
        assert!(tags("#").is_empty());
        assert_eq!(body("#1 の件、よろしく"), "#1 の件、よろしく");
    }

    #[test]
    fn a_bare_hash_is_a_persons_text_and_stays() {
        // **`tags()` だけでは足りない。** `#` を「タグ行」と読んでも、
        // 名前が空なので `tags()` は空を返す ── 同じ顔をする。
        // 違いが出るのは `body()` で、ここが人の一文字を落とすかどうか。
        assert_eq!(body("#"), "#");
        assert_eq!(body("本文\n\n#"), "本文\n\n#");
        assert_eq!(set("#", &v(&["太郎"])), "#\n\n#太郎");
    }

    #[test]
    fn a_blank_line_is_not_a_tag_line() {
        // いまは空行を先に弾いてからしか呼ばないが、**呼ぶ側の都合に
        // 寄りかからない** ── 弾く場所が動いた日に、空行がタグ行になる。
        assert!(!only_tags(""));
        assert!(!only_tags("   "));
        assert!(only_tags("#太郎"));
    }

    #[test]
    fn no_notes_at_all_means_no_tags() {
        assert!(tags("").is_empty());
        assert!(tags("   \n\n  ").is_empty());
        assert_eq!(body(""), "");
    }

    #[test]
    fn the_whole_memo_can_be_one_tag_line() {
        assert_eq!(tags("#太郎"), v(&["太郎"]));
        assert_eq!(body("#太郎"), "");
    }

    #[test]
    fn writing_tags_does_not_touch_what_a_person_wrote() {
        let memo = "買い物のあと、駅前で待ち合わせ。\n保険証を忘れずに。";
        let out = set(memo, &v(&["太郎"]));
        assert_eq!(out, "買い物のあと、駅前で待ち合わせ。\n保険証を忘れずに。\n\n#太郎");
        assert_eq!(body(&out), memo);
    }

    #[test]
    fn writing_again_replaces_the_tag_line_and_nothing_else() {
        let memo = "保険証を忘れずに。\n\n#太郎 #次郎";
        let out = set(memo, &v(&["花子"]));
        assert_eq!(out, "保険証を忘れずに。\n\n#花子");
        // 二度書いても増えない。
        assert_eq!(set(&out, &v(&["花子"])), out);
    }

    #[test]
    fn clearing_the_tags_removes_the_line_and_the_blank_line_with_it() {
        let memo = "保険証を忘れずに。\n\n#太郎";
        assert_eq!(set(memo, &[]), "保険証を忘れずに。");
        // もともと無ければ、何も起きない。
        assert_eq!(set("保険証を忘れずに。", &[]), "保険証を忘れずに。");
        assert_eq!(set("", &[]), "");
    }

    #[test]
    fn tags_on_an_empty_memo_stand_alone() {
        assert_eq!(set("", &v(&["太郎"])), "#太郎");
        assert_eq!(set("   ", &v(&["太郎"])), "#太郎");
    }

    #[test]
    fn the_shape_of_the_line_survives_a_round_trip() {
        // 字 → タグ → 字 → タグ で、同じものに戻る。
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
    fn the_newline_of_the_memo_is_kept() {
        // **Exchange や Windows から来たメモを LF で書き戻さない** ──
        // 触っていない行まで差分になる。
        let memo = "保険証を忘れずに。\r\n二行目。";
        assert_eq!(set(memo, &v(&["太郎"])), "保険証を忘れずに。\r\n二行目。\r\n\r\n#太郎");
        assert_eq!(tags("本文\r\n\r\n#太郎 #次郎"), v(&["太郎", "次郎"]));
        assert_eq!(body("本文\r\n\r\n#太郎"), "本文");
    }

    #[test]
    fn the_same_name_twice_is_one_tag() {
        assert_eq!(tags("#太郎 #太郎 #次郎"), v(&["太郎", "次郎"]));
        assert_eq!(set("", &v(&["太郎", "太郎"])), "#太郎");
    }

    #[test]
    fn a_hash_in_front_is_optional_when_writing() {
        // 呼ぶ側が `#太郎` と渡しても `太郎` と渡しても同じ。
        assert_eq!(set("", &v(&["#太郎"])), "#太郎");
        assert_eq!(set("", &v(&["  太郎  "])), "#太郎");
    }

    #[test]
    fn a_name_with_a_space_in_it_cannot_be_a_tag() {
        // 空白で区切って読むので、空白を含む名前は**書けない**。
        // 黙って壊れた行を書くより、落とす。
        assert_eq!(set("", &v(&["山田 太郎"])), "");
        assert_eq!(set("", &v(&["太郎", "山田 太郎"])), "#太郎");
    }

    #[test]
    fn trailing_blank_lines_go_away_but_nothing_else_does() {
        // タグ行を置ける場所がそこしか無いので、末尾の空行だけは落とす。
        assert_eq!(body("本文\n\n\n"), "本文");
        assert_eq!(set("本文\n\n\n", &v(&["太郎"])), "本文\n\n#太郎");
        // 途中の空行は、人の段落なので残す。
        assert_eq!(body("一段落\n\n二段落"), "一段落\n\n二段落");
    }
}
