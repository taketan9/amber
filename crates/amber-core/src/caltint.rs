//! **予定を何色で塗るか**（依頼 711・712）。
//!
//! 本人が奥さんの iPhone と予定表を同期できた日に言った ──
//! 「ただ、全部同じ色に見えるな・・・。各カレンダーの色をセットできる
//! ようにすると、僕と妻のカレンダーの色を分けれるよね？」
//!
//! それまでの決まりは、端末にある予定表が何本あっても**ぜんぶ一色**だった
//! （「自分の色」）。奥さんの予定も、自分の予定も、祝日も同じ色。
//!
//! # 決める順
//!
//! 1. **その予定に書いてある色** ── メモ欄の最後の行の `#色:カーマイン`、
//!    またはノートの前書きの `color:`。人が一つの予定に指したものなので、
//!    いちばん強い
//! 2. **その予定表の色**（依頼 711）── iPhone が持っている色をそのまま使う。
//!    カレンダーアプリで見えている色と揃うので、設定を一つも触らずに
//!    奥さんの予定と分かれる。amber の設定で上書きもできる
//! 3. よそから足したカレンダー ── 青
//! 4. この端末の予定表で、色が読めなかったもの ── 「自分の色」
//! 5. 自分のノートに書いた予定 ── 琥珀
//! 6. それ以外 ── 画面にまかせる
//!
//! **3 以降はいままでと同じ。** 2 を足しただけで、予定表の色が読めない
//! ときは前と同じ絵になる。
//!
//! # 色そのものは返さない（3 以降）
//!
//! 3・4・5 でどの色を使うかは**画面が決める**。青も琥珀も、明るい紙と
//! 暗い紙で違う濃さにしたいもので、ここで `#rrggbb` に潰すと暗いところで
//! 読めなくなる。ここが返すのは「どれで塗るか」だけ。
//!
//! # 八色の名前
//!
//! `#色:カーマイン` の「カーマイン」を `#c0392b` に直すのは、ここの仕事。
//! **同じ表が iPhone とデスクトップ版にもある**（選ぶ丸を並べるため）。
//! 三つが揃っているかは `scripts/colors-test.js` が見ている。

/// 選べる色。**名前で呼べる八色**（依頼 540・本人が名前を決めた）。
///
/// **十色から二色やめた**（本人）── 青は水色と、オレンジは赤と似ていて、
/// 小さな一行になると見分けが付かない。
///
/// **青をやめたので、青はよその予定表だけの色になった** ── 人のタグに青が
/// 出てこないぶん、青い予定は「自分では直せないもの」だと読める。
pub const COLORS: [(&str, &str); 8] = [
    ("#e0669c", "ローズ"),
    ("#d9a400", "アンバー"),
    ("#2f8a52", "リーフ"),
    ("#8e5cb3", "バイオレット"),
    ("#1fa3a3", "シアン"),
    ("#c0392b", "カーマイン"),
    ("#7a5c3a", "セピア"),
    ("#5a6b7f", "スレート"),
];

/// 何で塗るか。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// その予定に書いてあった色。
    Event(String),
    /// その予定表の色。
    Shelf(String),
    /// よその予定表（青）。
    Away,
    /// この端末の予定表（自分の色）。
    Here,
    /// 自分のノートに書いた予定（琥珀）。
    Plan,
    /// 画面の既定にまかせる。
    Other,
}

impl Pick {
    /// 画面に渡す言葉。
    pub fn word(&self) -> &'static str {
        match self {
            Pick::Event(_) => "event",
            Pick::Shelf(_) => "shelf",
            Pick::Away => "away",
            Pick::Here => "here",
            Pick::Plan => "plan",
            Pick::Other => "other",
        }
    }

    /// 色が決まっているなら、その `#rrggbb`。決まっていなければ空。
    pub fn hex(&self) -> &str {
        match self {
            Pick::Event(h) | Pick::Shelf(h) => h,
            _ => "",
        }
    }
}

/// 色の名前を `#rrggbb` に直す。知らない名前は空を返す。
///
/// **知らない名前を捨てる**のは、変な色で塗るより、いつもの色のほうが
/// よいから。人がメモ欄に `#色:すごい色` と書いていても、そこは人の字。
pub fn hex_of(name: &str) -> &'static str {
    COLORS
        .iter()
        .find(|(_, n)| *n == name)
        .map(|(h, _)| *h)
        .unwrap_or("")
}

/// `#rrggbb` を色の名前に直す。知らない色は、その `#rrggbb` のまま返す
/// （画面はそれで塗れる）。
pub fn name_of(hex: &str) -> String {
    COLORS
        .iter()
        .find(|(h, _)| *h == hex)
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| hex.to_string())
}

/// その予定を何で塗るか。
///
/// - `color` ── その予定に書いてあった色の名前（無ければ空）
/// - `shelf` ── その予定が入っている予定表の色（`#rrggbb`・読めなければ空）
/// - `away` ── よそから足したカレンダーの予定か
/// - `here` ── この端末の予定表の予定か
/// - `plan` ── 自分のノートに書いた予定か
pub fn pick(color: &str, shelf: &str, away: bool, here: bool, plan: bool) -> Pick {
    if !color.is_empty() {
        let hex = hex_of(color);
        if !hex.is_empty() {
            return Pick::Event(hex.to_string());
        }
    }
    // **よそが先。** よその予定表には色を上書きする道が無く（予定表として
    // 持っていない）、青は「自分では直せないもの」の印にしてある。
    if away {
        return Pick::Away;
    }
    if !shelf.is_empty() {
        return Pick::Shelf(shelf.to_string());
    }
    if here {
        return Pick::Here;
    }
    if plan {
        return Pick::Plan;
    }
    Pick::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 予定に書いてある色がいちばん強い() {
        let got = pick("カーマイン", "#1fa3a3", false, true, false);
        assert_eq!(got, Pick::Event("#c0392b".into()));
        assert_eq!(got.word(), "event");
        assert_eq!(got.hex(), "#c0392b");
    }

    #[test]
    fn 知らない色の名前は捨てて_予定表の色に落ちる() {
        // 変な色で塗るより、いつもの色のほうがよい。
        let got = pick("すごい色", "#1fa3a3", false, true, false);
        assert_eq!(got, Pick::Shelf("#1fa3a3".into()));
    }

    #[test]
    fn 予定表の色があれば_それで塗る() {
        let got = pick("", "#e0669c", false, true, false);
        assert_eq!(got, Pick::Shelf("#e0669c".into()));
        assert_eq!(got.word(), "shelf");
    }

    #[test]
    fn 予定表の色が読めなければ_いままでの自分の色() {
        // **足しただけで、前の絵を変えない。** 色が読めない端末では、
        // 依頼 711 の前とまったく同じ絵になる。
        let got = pick("", "", false, true, false);
        assert_eq!(got, Pick::Here);
        assert_eq!(got.hex(), "");
    }

    #[test]
    fn よその予定表は_予定表の色より青が強い() {
        // 青は「自分では直せないもの」の印。
        assert_eq!(pick("", "#e0669c", true, false, false), Pick::Away);
    }

    #[test]
    fn よその予定表でも_予定に書いてある色には負ける() {
        assert_eq!(pick("シアン", "", true, false, false), Pick::Event("#1fa3a3".into()));
    }

    #[test]
    fn 自分のノートに書いた予定は琥珀() {
        assert_eq!(pick("", "", false, false, true), Pick::Plan);
    }

    #[test]
    fn どれでもなければ_画面にまかせる() {
        let got = pick("", "", false, false, false);
        assert_eq!(got, Pick::Other);
        assert_eq!(got.word(), "other");
        assert_eq!(got.hex(), "");
    }

    #[test]
    fn 色の名前と_rrggbb_を行き来できる() {
        assert_eq!(hex_of("カーマイン"), "#c0392b");
        assert_eq!(hex_of("すごい色"), "");
        assert_eq!(name_of("#c0392b"), "カーマイン");
    }

    #[test]
    fn 知らない_rrggbb_は_そのまま返す() {
        // 予定表の色は iPhone が持っているもので、八色の中には無い。
        assert_eq!(name_of("#123456"), "#123456");
    }

    #[test]
    fn 八色に同じものが二つ無い() {
        for (i, (h, n)) in COLORS.iter().enumerate() {
            for (h2, n2) in COLORS.iter().skip(i + 1) {
                assert_ne!(h, h2, "同じ色が二つあります");
                assert_ne!(n, n2, "同じ名前が二つあります");
            }
        }
    }
}
