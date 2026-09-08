//! 絵文字の表。**amber が持つ**（依頼 418）。
//!
//! 外の何かを取りに行かない ── 会社の窓に閉じた機械では外へ出られないし、
//! 出られたとしても「今日は絵文字が出ない」がありうる形にはしない。
//!
//! **全部は載せない。** Unicode の絵文字は三千を超えるが、そこから探すのは
//! 「絵文字を探す」という仕事になる。ここに載せるのは**日本語で名前を付けた
//! ぶんだけ** ── 名前が付いていないものは探せないので、載っていても同じ。
//! 足りないものは端末の絵文字盤から打てる（`.md` に入るのは同じ字）。
//!
//! 一つの表を二つの amber が読む ── 窓と電話で並びも名前も違う、を作らない。

/// 絵文字ひとつ。`words` は**空白で区切った探し言葉**（日本語が先）。
pub struct Face {
    pub ch: &'static str,
    pub words: &'static str,
}

/// 束ひとつ。`icon` は帯に出す見出しの絵。
pub struct Group {
    pub name: &'static str,
    pub icon: &'static str,
    pub faces: &'static [Face],
}

const fn f(ch: &'static str, words: &'static str) -> Face {
    Face { ch, words }
}

/// **初めて開いた人が見る 24 個。**
///
/// 「最近つかったもの」の初期値でもある ── 空の棚を見せない。
/// 選び方は「毎日の返事に要るもの」: 祝う・褒める・頼む・印を付ける。
/// 👍 と 👌 は本人がいちばん使うと言ったもの（2026-09-09）。
pub const FIRST: [&str; 24] = [
    "🎉", "🎊", "🥳", "👏", "🙌", "✨", "👍", "👌",
    "😀", "😂", "🥲", "😍", "🤔", "😴", "🙏", "💪",
    "✅", "❌", "⚠️", "🔥", "⭐️", "📌", "📝", "💡",
];

const FACES: &[Face] = &[
    f("😀", "にっこり 笑顔 うれしい grin"),
    f("😄", "笑う たのしい smile"),
    f("😁", "にやり 笑顔 beam"),
    f("😂", "爆笑 泣き笑い うける joy"),
    f("🤣", "大爆笑 うける rofl"),
    f("🙂", "微笑み ふつう slight"),
    f("😉", "ウィンク wink"),
    f("😊", "照れ うれしい ほほえみ blush"),
    f("😍", "好き 大好き ハート目 heart eyes"),
    f("🥰", "愛しい 好き love"),
    f("😘", "投げキス kiss"),
    f("🤗", "ハグ 歓迎 hug"),
    f("🤔", "考える うーん 悩む thinking"),
    f("🤨", "疑い ほんとに eyebrow"),
    f("😐", "無表情 まじめ neutral"),
    f("🙄", "やれやれ 目をそらす roll eyes"),
    f("😏", "ドヤ にやり smirk"),
    f("😥", "困った すみません sad"),
    f("😭", "号泣 かなしい sob"),
    f("😱", "驚き 悲鳴 scream"),
    f("😳", "びっくり 照れ flushed"),
    f("🥺", "お願い うるうる pleading"),
    f("😤", "ふんぬ 気合い triumph"),
    f("😡", "怒り おこ angry"),
    f("🥲", "うれし涙 やれやれ tear"),
    f("😅", "汗 苦笑い sweat"),
    f("😇", "天使 いい子 innocent"),
    f("🤩", "きらきら すごい star struck"),
    f("😎", "かっこいい サングラス cool"),
    f("🥳", "お祝い パーティ party"),
    f("😴", "寝る おやすみ sleep"),
    f("🤒", "熱 具合が悪い sick"),
    f("🤯", "衝撃 頭が爆発 mind blown"),
    f("🫠", "とけた だめ melting"),
    f("😬", "気まずい grimace"),
    f("🤫", "しずかに ないしょ shush"),
];

const HANDS: &[Face] = &[
    f("👍", "いいね ぐっど 賛成 親指 good thumbs up"),
    f("👎", "だめ 反対 thumbs down"),
    f("👌", "オーケー おっけー 了解 ok"),
    f("🙏", "お願い ありがとう 拝む 感謝 pray thanks"),
    f("👏", "拍手 すごい clap"),
    f("🙌", "やった 万歳 raise hands"),
    f("🤝", "握手 よろしく handshake"),
    f("✌️", "ピース victory"),
    f("🤞", "祈る うまくいけ fingers crossed"),
    f("👋", "やあ ばいばい 手を振る wave"),
    f("💪", "がんばる 力こぶ muscle"),
    f("🫡", "了解 敬礼 salute"),
    f("👀", "見てる 注目 eyes"),
    f("🧠", "頭 考え brain"),
    f("❤️", "ハート 好き heart"),
    f("💔", "失恋 かなしい broken heart"),
    f("🎯", "的中 ねらい target"),
    f("🫶", "ハートの手 感謝 heart hands"),
];

const NATURE: &[Face] = &[
    f("🐶", "犬 いぬ dog"),
    f("🐱", "猫 ねこ cat"),
    f("🐻", "熊 くま bear"),
    f("🐰", "兎 うさぎ rabbit"),
    f("🐼", "パンダ panda"),
    f("🐧", "ペンギン penguin"),
    f("🐤", "ひよこ chick"),
    f("🐝", "蜂 はち bee"),
    f("🐢", "亀 かめ turtle"),
    f("🐳", "鯨 くじら whale"),
    f("🌸", "桜 さくら 花 cherry blossom"),
    f("🌱", "芽 はじまり 育つ seedling"),
    f("🌳", "木 き tree"),
    f("🍀", "四つ葉 幸運 clover"),
    f("🌼", "花 はな flower"),
    f("🌞", "太陽 晴れ sun"),
    f("🌙", "月 夜 moon"),
    f("⭐️", "星 ほし star"),
    f("☁️", "雲 くもり cloud"),
    f("🌧️", "雨 あめ rain"),
    f("❄️", "雪 ゆき 寒い snow"),
    f("🌈", "虹 にじ rainbow"),
    f("🔥", "炎 ほのお 熱い すごい fire"),
    f("💧", "水 しずく drop"),
    f("🌊", "波 海 wave"),
    f("🏔️", "山 やま mountain"),
];

const FOOD: &[Face] = &[
    f("🍎", "りんご 林檎 apple"),
    f("🍌", "バナナ banana"),
    f("🍇", "ぶどう 葡萄 grapes"),
    f("🍓", "いちご 苺 strawberry"),
    f("🍅", "トマト tomato"),
    f("🥕", "にんじん 人参 carrot"),
    f("🍞", "パン ぱん bread"),
    f("🍚", "ご飯 ごはん 米 rice"),
    f("🍙", "おにぎり onigiri"),
    f("🍜", "ラーメン 麺 ramen"),
    f("🍣", "寿司 すし sushi"),
    f("🍛", "カレー curry"),
    f("🍕", "ピザ pizza"),
    f("🍔", "ハンバーガー burger"),
    f("🍰", "ケーキ cake"),
    f("🍩", "ドーナツ doughnut"),
    f("🍫", "チョコ chocolate"),
    f("☕️", "コーヒー 珈琲 coffee"),
    f("🍵", "お茶 緑茶 tea"),
    f("🍺", "ビール beer"),
    f("🥛", "牛乳 ミルク milk"),
    f("🧃", "ジュース juice"),
];

const DOING: &[Face] = &[
    f("⚽️", "サッカー soccer"),
    f("⚾️", "野球 baseball"),
    f("🏃", "走る ランニング run"),
    f("🚶", "歩く walk"),
    f("🧘", "座禅 落ち着く meditate"),
    f("🎧", "音楽 ヘッドホン music"),
    f("🎸", "ギター guitar"),
    f("🎮", "ゲーム game"),
    f("📷", "写真 カメラ camera"),
    f("🎬", "映画 撮影 movie"),
    f("🎨", "絵 アート art"),
    f("✈️", "飛行機 旅 plane"),
    f("🚗", "車 くるま car"),
    f("🚃", "電車 でんしゃ train"),
    f("🏠", "家 いえ home"),
    f("🏢", "会社 ビル office"),
    f("🏥", "病院 hospital"),
    f("🛏️", "寝る ベッド bed"),
    f("🛒", "買い物 カート cart"),
    f("🎁", "贈り物 プレゼント gift"),
    f("🎂", "誕生日 ケーキ birthday"),
    f("🎊", "紙吹雪 お祝い confetti"),
    f("🎉", "クラッカー おめでとう 祝い パーティ tada"),
    f("🍾", "乾杯 シャンパン champagne"),
];

const THINGS: &[Face] = &[
    f("📝", "メモ 書く note memo"),
    f("📄", "書類 ファイル page"),
    f("📚", "本 ほん 資料 books"),
    f("📌", "ピン 留める 大事 pin"),
    f("📎", "クリップ 添付 clip"),
    f("📁", "フォルダ folder"),
    f("🗓️", "予定 カレンダー calendar"),
    f("⏰", "時間 目覚まし alarm"),
    f("⏳", "待ち 砂時計 hourglass"),
    f("💡", "ひらめき アイデア idea bulb"),
    f("🔑", "鍵 かぎ key"),
    f("🔒", "鍵をかける 非公開 lock"),
    f("💰", "お金 かね money"),
    f("💻", "パソコン ノートパソコン laptop"),
    f("📱", "スマホ 電話 phone"),
    f("🖥️", "画面 デスクトップ desktop"),
    f("⌨️", "キーボード keyboard"),
    f("🖨️", "印刷 プリンタ printer"),
    f("🔍", "探す 検索 虫眼鏡 search"),
    f("🔧", "直す 工具 wrench"),
    f("🧹", "掃除 片づけ broom"),
    f("🗑️", "ごみ箱 捨てる trash"),
    f("💊", "薬 くすり pill"),
    f("✉️", "手紙 メール mail"),
    f("📢", "お知らせ announce"),
    f("🔔", "通知 ベル bell"),
    f("🏷️", "タグ ラベル label"),
    f("🧪", "実験 試す test"),
];

const SIGNS: &[Face] = &[
    f("✅", "チェック 済み 完了 done check"),
    f("☑️", "チェック 済み checkbox"),
    f("❌", "だめ 中止 バツ cross"),
    f("⭕️", "まる 正解 circle"),
    f("⚠️", "注意 警告 warning"),
    f("❓", "疑問 質問 question"),
    f("❗️", "大事 注目 exclamation"),
    f("💯", "満点 完璧 hundred"),
    f("✨", "きらきら 新しい すごい sparkles"),
    f("🚀", "打ち上げ 速い 出発 rocket"),
    f("🐛", "不具合 バグ bug"),
    f("🔗", "リンク つなぐ link"),
    f("➡️", "右 次へ right"),
    f("⬅️", "左 戻る left"),
    f("⬆️", "上 up"),
    f("⬇️", "下 down"),
    f("🔁", "繰り返し ループ repeat"),
    f("➕", "足す 追加 plus"),
    f("➖", "引く 減らす minus"),
    f("🆕", "新しい new"),
    f("🆗", "オーケー ok"),
    f("🈵", "満 いっぱい full"),
    f("🅰️", "A えー a"),
    f("🔴", "赤 red"),
    f("🟠", "橙 orange"),
    f("🟡", "黄 yellow"),
    f("🟢", "緑 green"),
    f("🔵", "青 blue"),
    f("⚫️", "黒 black"),
    f("⚪️", "白 white"),
];

/// 束ぜんぶ。**並びがそのまま帯の並び。**
pub const GROUPS: &[Group] = &[
    Group { name: "顔", icon: "😀", faces: FACES },
    Group { name: "手・気持ち", icon: "👍", faces: HANDS },
    Group { name: "自然・動物", icon: "🌸", faces: NATURE },
    Group { name: "食べもの", icon: "🍎", faces: FOOD },
    Group { name: "することと場所", icon: "⚽️", faces: DOING },
    Group { name: "もの", icon: "📝", faces: THINGS },
    Group { name: "記号", icon: "✅", faces: SIGNS },
];

/// 送る形。窓も電話もこれを一度受け取って、あとは自分で並べる。
pub fn table() -> serde_json::Value {
    serde_json::json!({
        "first": FIRST,
        "groups": GROUPS.iter().map(|g| serde_json::json!({
            "name": g.name,
            "icon": g.icon,
            "faces": g.faces.iter().map(|x| serde_json::json!({
                "ch": x.ch, "words": x.words,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_face_has_japanese_words_and_appears_once() {
        let mut seen = std::collections::HashMap::new();
        for g in super::GROUPS {
            assert!(!g.faces.is_empty(), "{} が空です", g.name);
            for x in g.faces {
                // **名前の無い絵文字は載せない。** 探せないので、載っていても同じ。
                assert!(!x.words.trim().is_empty(), "{} に探し言葉がありません", x.ch);
                assert!(
                    x.words.chars().any(|c| c > '\u{7f}'),
                    "{} の探し言葉が日本語を含みません: {}",
                    x.ch,
                    x.words
                );
                // 同じ絵が二つの束に出ていると、探した人が二度見る。
                if let Some(was) = seen.insert(x.ch, g.name) {
                    panic!("{} が {was} と {} の両方にあります", x.ch, g.name);
                }
            }
        }
        // 初めの 24 は、どれも表のどこかに居る ── 居ないと、選んだあと
        // 「最近つかったもの」から名前が引けない。
        for ch in super::FIRST {
            assert!(seen.contains_key(ch), "{ch} が表にありません");
        }
        // 本人がいちばん使うと言ったもの（2026-09-09）。
        assert!(super::FIRST.contains(&"👍"), "👍 が初めの 24 にありません");
        assert!(super::FIRST.contains(&"👌"), "👌 が初めの 24 にありません");
    }
}
