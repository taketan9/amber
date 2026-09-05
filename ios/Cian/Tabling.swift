import SwiftUI

/// Making a table without typing pipes.
///
/// **A 2×2 skeleton was the wrong answer.** A table you have to type into is
/// a table you have to keep counting `|` for, and the row that decides the
/// alignment (`:---`, `:---:`, `---:`) is the one nobody remembers the shape
/// of. So the shape is chosen here — how many columns, what they are called,
/// which way each one lines up — and the pipes are cian's problem.
///
/// What it makes is ordinary Markdown. Nothing here is a cian table; it is a
/// table any other tool reads, made without the counting.
struct Tabling: View {
    /// Hands back the finished Markdown to drop in.
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
                    HStack {
                        Button {
                            heads.append("")
                            ways.append(.left)
                        } label: {
                            Label("列を足す", systemImage: "plus")
                        }
                        .disabled(heads.count >= 6)
                        Spacer()
                        Button(role: .destructive) {
                            heads.removeLast()
                            ways.removeLast()
                        } label: {
                            Label("減らす", systemImage: "minus")
                        }
                        .disabled(heads.count <= 1)
                    }
                    .buttonStyle(.borderless)
                } header: {
                    Text("列")
                } footer: {
                    Text("見出しは空のままでも構いません。あとから書けます。")
                }

                Section("行") {
                    Stepper("空の行 \(rows)", value: $rows, in: 0...20)
                }

                Section("できるもの") {
                    // Shown as the text it will be, not as a drawn table: the
                    // note is the text, and what goes in has to be what you
                    // are agreeing to.
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

    private func binding(_ i: Int) -> Binding<String> {
        Binding(get: { heads.indices.contains(i) ? heads[i] : "" },
                set: { if heads.indices.contains(i) { heads[i] = $0 } })
    }

    /// The table, as Markdown.
    ///
    /// An ideographic space in the empty cells rather than nothing: a row of
    /// `|  |  |` collapses to something a renderer may drop, and an empty
    /// table that vanishes looks like a table that failed to be made.
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
