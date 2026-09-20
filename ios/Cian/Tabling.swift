import SwiftUI

/// パイプを打たずに表を作る。
///
/// **2×2 の空の表を挿入するのは答えではなかった。** 打ち込んでいく表は、
/// `|` を数え続ける表になるし、寄せ方を決める行（`:---` `:---:` `---:`）は
/// 誰も形を覚えていない。だから形はここで選ばせる ── 列はいくつか、
/// 何という名前か、どちら寄せか ── そしてパイプは amber の仕事にする。
///
///
/// できるのは普通の Markdown。amber 独自の表ではなく、ほかのどのツールでも
/// 読める表を、数えずに作れるようにしただけ。
struct Tabling: View {
    /// 出来上がった Markdown を、挿入用に返す。
    let put: (String) -> Void
    @Environment(\.dismiss) private var dismiss

    @State private var heads: [String] = ["", ""]
    @State private var ways: [Way] = [.left, .left]
    @State private var rows = 2

    enum Way: String, CaseIterable, Identifiable {
        case left, center, right
        var id: String { rawValue }
        var mark: String {
            switch self {
            case .left: return "---"
            case .center: return ":---:"
            case .right: return "---:"
            }
        }
        var icon: String {
            switch self {
            case .left: return "text.alignleft"
            case .center: return "text.aligncenter"
            case .right: return "text.alignright"
            }
        }
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    ForEach(heads.indices, id: \.self) { i in
                        HStack(spacing: 10) {
                            TextField("列 \(i + 1)", text: binding(i))
                                .textInputAutocapitalization(.never)
                            Picker("", selection: $ways[i]) {
                                ForEach(Way.allCases) { w in
                                    Image(systemName: w.icon).tag(w)
                                }
                            }
                            .pickerStyle(.segmented)
                            .frame(width: 132)
                            .labelsHidden()
                        }
                    }
                    // 下の行と同じ言葉づかい・同じ形にする ── 2 つの数に
                    // 同じことをする 2 つのコントロールが、別々のものに
                    // 見えてはいけない。
                    // ideas.
                    more(add: { heads.append(""); ways.append(.left) },
                         drop: { heads.removeLast(); ways.removeLast() },
                         canAdd: heads.count < 6, canDrop: heads.count > 1)
                } header: {
                    Text("列")
                } footer: {
                    Text("見出しは空のままでも構いません。あとから書けます。")
                }

                Section("行") {
                    LabeledContent("空の行", value: "\(rows)")
                    more(add: { rows += 1 }, drop: { rows -= 1 },
                         canAdd: rows < 20, canDrop: rows > 0)
                }

                Section("できるもの") {
                    // 描画した表ではなく、実際に入る文字列として見せる ──
                    // ノートとは文字列であり、入るものは同意したものと
                    // 同じでなければならない。
                    ScrollView(.horizontal, showsIndicators: false) {
                        Text(markdown)
                            .font(.caption.monospaced())
                            .textSelection(.enabled)
                    }
                }
            }
            .navigationTitle("表を作る")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("やめる") { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("入れる") {
                        put(markdown)
                        dismiss()
                    }
                    .bold()
                }
            }
        }
    }

    /// 増やす / 減らす — one shape, used for both.
    private func more(add: @escaping () -> Void, drop: @escaping () -> Void,
                      canAdd: Bool, canDrop: Bool) -> some View {
        HStack {
            Button { add() } label: { Label("増やす", systemImage: "plus") }
                .disabled(!canAdd)
            Spacer()
            Button { drop() } label: { Label("減らす", systemImage: "minus") }
                .disabled(!canDrop)
        }
        .buttonStyle(.borderless)
    }

    private func binding(_ i: Int) -> Binding<String> {
        Binding(get: { heads.indices.contains(i) ? heads[i] : "" },
                set: { if heads.indices.contains(i) { heads[i] = $0 } })
    }

    /// この表を、Markdown の文字にする。
    ///
    /// 空のセルには全角空白を入れる。何も入れないと `|  |  |` の行は
    /// レンダラが落としうる形に潰れ、消えた表は「作り損ねた表」に
    /// 見える。
    private var markdown: String {
        let cells = heads.map { $0.isEmpty ? "\u{3000}" : $0 }
        var out = "| " + cells.joined(separator: " | ") + " |\n"
        out += "| " + ways.map(\.mark).joined(separator: " | ") + " |\n"
        for _ in 0..<rows {
            out += "| " + Array(repeating: "\u{3000}", count: heads.count).joined(separator: " | ") + " |\n"
        }
        return out
    }
}
