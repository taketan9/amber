import SwiftUI

/// ノート 1 件のタグ。
///
/// そのフォルダに既にあるタグを候補として出す。打ち直させない ── 打ち直すと、
/// 「仕事」 again and get 「 仕事」 — a tag with a stray space is a second tag
/// 最初のものとそっくりな別のタグができるが、画面には何も出ない。
struct Tagging: View {
    @Binding var tags: [String]
    let known: [String]
    @Environment(\.dismiss) private var dismiss
    @State private var typed = ""

    var body: some View {
        NavigationStack {
            List {
                Section("このノート") {
                    if tags.isEmpty {
                        Text("まだありません").foregroundStyle(.secondary)
                    }
                    ForEach(tags, id: \.self) { t in
                        HStack {
                            Text("#\(t)")
                            Spacer()
                            Button {
                                tags.removeAll { $0 == t }
                            } label: {
                                Image(systemName: "minus.circle.fill").foregroundStyle(.red)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
                Section("新しいタグを作る") {
                    HStack {
                        TextField("新しいタグ", text: $typed)
                            .autocorrectionDisabled()
                            .onSubmit { add(typed) }
                        Button("作る") { add(typed) }
                            .disabled(clean(typed).isEmpty)
                    }
                    // フォルダに既にあるもののうち、このノートに
                    // まだ付いていないもの。
                    ForEach(known.filter { !tags.contains($0) }, id: \.self) { t in
                        Button("#\(t)") { add(t) }
                    }
                }
            }
            .navigationTitle("タグ")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { Button("閉じる") { dismiss() } }
            }
        }
    }

    /// タグはハッシュを除いた 1 語。人が `#` を打つのは、表示される場所がどこも
    /// how they see it written, and a tag stored as `#仕事` would be `##仕事`
    /// そうなっているから。
    private func clean(_ s: String) -> String {
        s.trimmingCharacters(in: .whitespacesAndNewlines)
            .trimmingCharacters(in: CharacterSet(charactersIn: "#"))
            .trimmingCharacters(in: .whitespaces)
    }

    private func add(_ s: String) {
        let t = clean(s)
        guard !t.isEmpty, !tags.contains(t) else { typed = ""; return }
        tags.append(t)
        typed = ""
    }
}
