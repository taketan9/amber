import SwiftUI

/// The tags on one note.
///
/// Offers the tags already in the folder rather than making somebody type
/// 「仕事」 again and get 「 仕事」 — a tag with a stray space is a second tag
/// that looks like the first one, and nothing on screen would say so.
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
                Section("足す") {
                    HStack {
                        TextField("新しいタグ", text: $typed)
                            .autocorrectionDisabled()
                            .onSubmit { add(typed) }
                        Button("足す") { add(typed) }
                            .disabled(clean(typed).isEmpty)
                    }
                    // The ones already in the folder, minus the ones already
                    // on this note.
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

    /// A tag is one word without its hash: people type the `#` because that is
    /// how they see it written, and a tag stored as `#仕事` would be `##仕事`
    /// everywhere it is shown.
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
