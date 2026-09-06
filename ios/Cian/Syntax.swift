import SwiftUI

/// マークダウンの書き方（電話）。
///
/// **顔ぶれは窓と同じ**（`gui/renderer.js` の `cmdSyntax`）── 記号の説明が
/// 端末によって違うと、片方で覚えたことがもう片方で通じない。
///
/// 電話では**入れる**のではなく**見せる**だけにした。窓は「選ぶと、いま
/// 書いているところに入る」が、設定の奥から辿ってくる電話では、そこへ
/// 戻ってくる頃には打っていた場所を離れている ── 記号そのものは下の帯の
/// 釦から入る（そちらが電話の入口）。
struct Syntax: View {
    private static let rows: [(String, String, String)] = [
        ("見出し", "# 大きい見出し", "# から始める。## で一段小さく"),
        ("箇条書き", "- もの", "行の頭に - と空白"),
        ("番号つき", "1. ひとつめ", "1. 2. 3. と書く"),
        ("チェック", "- [ ] やること", "押すと入り切りできる升になる"),
        ("太字", "**ここが太字**", "前後を ** で挟む"),
        ("斜体", "*ここが斜体*", "前後を * で挟む"),
        ("取り消し線", "~~消した字~~", "前後を ~~ で挟む"),
        ("コード", "`コード`", "前後を ` で挟む"),
        ("コードの枠", "```\nここに何行でも\n```", "``` の行で挟む"),
        ("リンク", "[見せる字](https://)", "角括弧が字、丸括弧が行き先"),
        ("画像", "![説明](絵の場所)", "頭に ! を付けるとリンクではなく絵"),
        ("引用", "> 引いてきた字", "行の頭に > と空白"),
        ("注記", "> [!NOTE]\n> 覚えておくこと", "NOTE / TIP / IMPORTANT / WARNING / CAUTION"),
        ("区切り線", "---", "ハイフン三つだけの行"),
        ("表", "| a | b |\n|---|---|\n| 1 | 2 |", "縦棒で区切る"),
        ("図", "```mermaid\nflowchart LR\n  A --> B\n```", "mermaid の書き方で図になる"),
    ]

    var body: some View {
        List(Self.rows, id: \.0) { name, example, how in
            VStack(alignment: .leading, spacing: 4) {
                Text(name).font(.body.weight(.semibold))
                Text(example)
                    .font(.system(.footnote, design: .monospaced))
                    .foregroundStyle(.tint)
                    .textSelection(.enabled)
                Text(how).font(.caption).foregroundStyle(.secondary)
            }
            .padding(.vertical, 2)
        }
        .navigationTitle("マークダウンの書き方")
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// amber について（電話）。
///
/// **不具合を伝えるときの三つ**（窓の `cmdAbout` と同じ顔ぶれ）── 画面の
/// 版、エンジンの版、ノートの置き場所。これが無いと、どちらの amber の
/// どの版の話なのかが分からないまま話が始まる。
struct About: View {
    let store: NotesStore?

    init(store: NotesStore? = nil) { self.store = store }

    private var app: String {
        let v = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "?"
        let b = Bundle.main.infoDictionary?["CFBundleVersion"] as? String ?? "?"
        return "amber \(v) (\(b))"
    }

    private var engine: String {
        guard let out = try? Cian.call("version", [:]) else { return "（答えません）" }
        return "amber-server " + (out["amber"] as? String ?? "?")
    }

    var body: some View {
        List {
            Section {
                LabeledContent("画面", value: app)
                LabeledContent("エンジン", value: engine)
                LabeledContent("ノートの置き場所", value: store?.rootName ?? "（この画面からは見えません）")
            } footer: {
                Text("不具合を伝えるときは、この三つを添えてください。")
            }
        }
        .navigationTitle("amber について")
        .navigationBarTitleDisplayMode(.inline)
    }
}
