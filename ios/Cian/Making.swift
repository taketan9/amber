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
                    Text("空のままなら今日の日付がタイトルになります")
                        .font(.caption).foregroundStyle(.secondary)
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
                        Button("足す") { add(typed) }.disabled(clean(typed).isEmpty)
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
