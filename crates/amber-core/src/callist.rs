//! **予定表の一覧に出す名前**（依頼 705）。
//!
//! カレンダーの表示設定に並ぶのは「ファミリー（iCloud）」のような一行。
//! いままで括弧の中は決め打ちで、デスクトップ版は何であろうと
//! 「（このパソコン）」、iPhone は「（この iPhone）」と書いていた ──
//! iCloud の予定表も Gmail の予定表も端末の中だけの予定表も同じ字になり、
//! 本人に「何がなんのカレンダーか分からない」と言われた（2026-10-31）。
//!
//! # 出どころの言葉は、扉が渡してくるものを使う
//!
//! EventKit は予定表ごとに出どころを持っている（`EKSource.title`）。
//! Google のアカウントは「Gmail」、祝日のような購読は「その他」になる ──
//! **こちらで言い換えない。** 利用者が設定アプリで見ている字と、amber が
//! 出す字を揃えるため。
//!
//! **ただし端末の中だけの予定表は、API が「Default」と返す**
//! （2026-10-31 に実機で出た ── 依頼 710）。設定アプリは同じものを
//! 「このiPhone」と書くので、API の字は人に見せられない。
//! **そこだけは扉（`Phone.swift` / `amber-cal.swift`）が言い換えて渡す** ──
//! 呼び名が端末ごとに違う一つなので、こちらでは決められない。
//! ここが受け取るのは、もう人に見せられる字になっている。
//!
//! # 同じ名前の予定表は、一行にまとめる
//!
//! 「誕生日」は iCloud にも Gmail にもある。amber は予定表を**名前で
//! 指している**（`here:誕生日`）ので、一行に一つの名前しか置けない ──
//! だから出どころを二つ並べて「誕生日（iCloud・Gmail）」と出す。
//! 出し入れの切り替えは、その名前の予定表ぜんぶに効く。
//!
//! **名前で指すのをやめない。** 鍵の形を変えると、本人が引っ込めてある
//! 予定表の憶え（`hide`）が当たらなくなり、**黙って出てくる**。

/// 画面から渡される、予定表 1 つぶん。
///
/// **`serde` の derive は使わない** ── amber-core は `serde_json` だけに
/// 繋いでいる。読み書きは `api` が手で組む。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shelf {
    /// 予定表の名前。
    pub title: String,
    /// 出どころ（OS の言う名前）。読めなければ空。
    pub source: String,
    /// 予定を足せるか。読むだけのものをグループに選ばせないため。
    pub writable: bool,
}

/// 一覧の一行。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    /// 出し入れの憶えに使う鍵。**いままでと同じ `here:` ＋ 名前。**
    pub key: String,
    pub title: String,
    /// 画面に出す一行。
    pub name: String,
    /// この名前の予定表の出どころ（同じ名前が二つあれば二つ）。
    pub sources: Vec<String>,
    /// その名前の予定表のどれか一つでも書けるか。
    pub writable: bool,
    /// グループの予定表に選ばれているもの。
    pub group: bool,
}

/// 読むだけの予定表に添える言葉。
const READONLY: &str = "読むだけ";
/// グループの予定表に添える言葉。
const GROUPED: &str = "グループの予定表";

/// 渡された予定表を、一覧の行に直す。
///
/// 並びは**渡された順のまま**（OS が返した順）。こちらで並べ替えると、
/// 設定アプリで見える順と変わってしまう。
pub fn rows(shelves: &[Shelf], group: &str) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    for s in shelves {
        if s.title.is_empty() {
            continue;
        }
        // 同じ名前が来たら、出どころを足すだけ。
        if let Some(seen) = out.iter_mut().find(|r| r.title == s.title) {
            // **一つでも書ければ、書ける。** 読むだけの「誕生日」と
            // 書ける「誕生日」があるとき、足せないと言うのは嘘になる。
            seen.writable = seen.writable || s.writable;
            if !s.source.is_empty() && !seen.sources.iter().any(|x| x == &s.source) {
                seen.sources.push(s.source.clone());
            }
            continue;
        }
        let on = !group.is_empty() && s.title == group;
        out.push(Row {
            key: "here:".to_string() + &s.title,
            title: s.title.clone(),
            name: String::new(),
            sources: if s.source.is_empty() {
                Vec::new()
            } else {
                vec![s.source.clone()]
            },
            writable: s.writable,
            group: on,
        });
    }
    // **名前は最後に組む。** 組んだ名前を読み返して足していくと、
    // 予定表の名前そのものに括弧が入っているときに崩れる
    // （「予定（旧）」の括弧を出どころと読んでしまう）。
    for r in out.iter_mut() {
        r.name = name(&r.title, &r.sources, r.writable, r.group);
    }
    out
}

/// 一行の名前を組む。
///
/// 出どころが読めなければ括弧を出さない ── 空の括弧は、何も言って
/// いないのに何か言っているように見える。
pub fn name(title: &str, from: &[String], writable: bool, group: bool) -> String {
    let mut marks: Vec<String> = from.to_vec();
    if !writable {
        marks.push(READONLY.to_string());
    }
    if group {
        marks.push(GROUPED.to_string());
    }
    if marks.is_empty() {
        return title.to_string();
    }
    format!("{}（{}）", title, marks.join("・"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shelf(title: &str, source: &str, writable: bool) -> Shelf {
        Shelf {
            title: title.to_string(),
            source: source.to_string(),
            writable,
        }
    }

    #[test]
    fn 出どころを括弧に入れて出す() {
        let got = rows(&[shelf("ファミリー", "iCloud", true)], "");
        assert_eq!(got[0].name, "ファミリー（iCloud）");
        assert_eq!(got[0].key, "here:ファミリー");
    }

    #[test]
    fn 出どころが読めなければ括弧を出さない() {
        let got = rows(&[shelf("自宅", "", true)], "");
        assert_eq!(got[0].name, "自宅");
    }

    #[test]
    fn 読むだけの予定表はそう言う() {
        let got = rows(&[shelf("日本の祝日", "その他", false)], "");
        assert_eq!(got[0].name, "日本の祝日（その他・読むだけ）");
        assert!(!got[0].writable);
    }

    #[test]
    fn グループに選ばれているものはそう言う() {
        let got = rows(&[shelf("ファミリー", "iCloud", true)], "ファミリー");
        assert_eq!(got[0].name, "ファミリー（iCloud・グループの予定表）");
        assert!(got[0].group);
    }

    #[test]
    fn 同じ名前の予定表は一行にまとめ_出どころを並べる() {
        let got = rows(
            &[
                shelf("誕生日", "iCloud", false),
                shelf("誕生日", "Gmail", false),
            ],
            "",
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "誕生日（iCloud・Gmail・読むだけ）");
    }

    #[test]
    fn まとめたとき_一つでも書ければ書けると言う() {
        let got = rows(
            &[
                shelf("予定", "その他", false),
                shelf("予定", "iCloud", true),
            ],
            "",
        );
        assert_eq!(got.len(), 1);
        assert!(got[0].writable);
        // 「読むだけ」は消える ── 書ける予定表があるので。
        assert_eq!(got[0].name, "予定（その他・iCloud）");
    }

    #[test]
    fn まとめても_グループの言葉は一度だけ() {
        let got = rows(
            &[
                shelf("ファミリー", "iCloud", true),
                shelf("ファミリー", "Gmail", true),
            ],
            "ファミリー",
        );
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].name,
            "ファミリー（iCloud・Gmail・グループの予定表）"
        );
    }

    #[test]
    fn 同じ出どころが二度来ても_一度しか出さない() {
        let got = rows(
            &[
                shelf("仕事", "iCloud", true),
                shelf("仕事", "iCloud", true),
            ],
            "",
        );
        assert_eq!(got[0].name, "仕事（iCloud）");
    }

    #[test]
    fn 名前の無い予定表は落とす() {
        let got = rows(&[shelf("", "iCloud", true)], "");
        assert!(got.is_empty());
    }

    #[test]
    fn 予定表の名前に括弧が入っていても_出どころと混ざらない() {
        let got = rows(
            &[
                shelf("予定（旧）", "iCloud", true),
                shelf("予定（旧）", "Gmail", true),
            ],
            "",
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "予定（旧）（iCloud・Gmail）");
    }

    #[test]
    fn 並びは渡された順のまま() {
        let got = rows(
            &[
                shelf("あ", "iCloud", true),
                shelf("い", "Gmail", true),
                shelf("う", "", true),
            ],
            "",
        );
        let names: Vec<&str> = got.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(names, ["あ", "い", "う"]);
    }
}
