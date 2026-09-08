import SwiftUI

/// Making a note: its title, then its tags, then it exists.
///
/// Two steps and not one. Typing a title used to make the note the moment the
/// keyboard's return was pressed, which put the note into the world before
/// anybody had said what it was about — and tagging it afterwards meant going
/// back into a note you had only just left.
///
/// A sheet rather than an alert, because an alert with a text field in it can
/// hold one question and this is two.
struct Making: View {
    /// Called with the title and the tags when 作成 is pressed.
    let make: (String, [String]) -> Void
    let known: [String]
    /// いま置いてある型（依頼 417）。**一つも無ければ、その段は出ない** ──
    /// 使っていない人の画面に、空の入れ物を見せない。
    let stencils: [Note]
    /// 型が選ばれたとき。ここで作って、この小窓は閉じる。
    let fromStencil: (Note) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var title = ""
    @State private var tags: [String] = []
    @State private var typed = ""
    @FocusState private var naming: Bool

    var body: some View {
        NavigationStack {
            Form {
                Section("タイトル") {
                    TextField("新しいノート", text: $title)
                        .focused($naming)
                        // Return moves on to the tags rather than making the
                        // note: the note is made by the button that says so.
                        .submitLabel(.next)
                        .onSubmit { naming = false }
                    Text("空のままでかまいません。本文の1行目が、そのまま題になります")
                        .font(.caption).foregroundStyle(.secondary)
                }
                // **型は、作るところに置く。** 「新しいノート」を押した人が
                // 探しているのがまさにこれで、献立の奥に置くと見つからない。
                // 選んだ瞬間に作る ── 型を選んでから題とタグを訊くと、
                // 型の中に書いてあるものをもう一度訊くことになる。
                if !stencils.isEmpty {
                    Section("型から") {
                        ForEach(stencils, id: \.path) { t in
                            Button {
                                fromStencil(t)
                                dismiss()
                            } label: {
                                Label(t.title.isEmpty ? "（タイトルなし）" : t.title,
                                      systemImage: "doc.on.doc")
                            }
                        }
                    }
                }
                Section("タグ") {
                    if !tags.isEmpty {
                        HStack {
                            ForEach(tags, id: \.self) { t in
                                Button {
                                    tags.removeAll { $0 == t }
                                } label: {
                                    Label("#\(t)", systemImage: "xmark.circle.fill")
                                        .labelStyle(.titleAndIcon)
                                        .font(.caption)
                                }
                                .buttonStyle(.bordered)
                            }
                        }
                    }
                    HStack {
                        TextField("新しいタグ", text: $typed)
                            .autocorrectionDisabled()
                            .onSubmit { add(typed) }
                        Button("作成") { add(typed) }.disabled(clean(typed).isEmpty)
                    }
                    ForEach(known.filter { !tags.contains($0) }, id: \.self) { t in
                        Button("#\(t)") { add(t) }
                    }
                }
            }
            .navigationTitle("新しいノート")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("やめる") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("作成") { make(title, tags); dismiss() }.bold()
                }
            }
            .task { naming = true }
        }
    }

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
