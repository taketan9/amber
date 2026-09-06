import SwiftUI

/// ノートの、前の姿（電話）。
///
/// **決まりは core が持っている** ── いつ一世代にするか、何世代残すか、
/// いつ落とすかは窓と同じでなければならない。同じフォルダを二つの端末で
/// 触るので、片方の決まりで消したものをもう片方が残っていると思う、が
/// 起きてはいけない。ここは並べて、見せて、戻すだけ。
struct Past: View {
    let store: NotesStore
    /// ノートの道か、フォルダの道。
    let at: String
    /// フォルダを訊いたときは、どのノートのものかを行に出す。
    let isBook: Bool
    @Environment(\.dismiss) private var dismiss

    @State private var rows: [[String: Any]] = []
    @State private var gens = 50
    @State private var days = 30
    @State private var peek: Peek?
    @State private var trouble: String?

    /// 誰の履歴か。`sheet(item:)` に渡すので `Identifiable`。
    struct Which: Identifiable {
        let at: String
        let book: Bool
        var id: String { (book ? "b:" : "n:") + at }
    }

    struct Peek: Identifiable {
        let id = UUID()
        let when: String
        let note: String
        let stamp: String
        let text: String
    }

    var body: some View {
        NavigationStack {
            Group {
                if rows.isEmpty {
                    ContentUnavailableView(
                        "まだありません", systemImage: "clock.arrow.circlepath",
                        description: Text("書いて手を止めるたびに、一つずつ残ります（\(gens) 世代・\(days) 日ぶん）"))
                } else {
                    List(rows.indices, id: \.self) { n in
                        let v = rows[n]
                        Button { open(v) } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(v["when"] as? String ?? "")
                                    if isBook, let note = v["note"] as? String {
                                        Text(note).font(.caption2).foregroundStyle(.secondary)
                                    }
                                }
                                Spacer()
                                // 印の付いた姿は、古くなっても消えない。
                                if v["kept"] as? Bool == true {
                                    Image(systemName: "star.fill").foregroundStyle(.orange)
                                }
                                Text(size(v)).font(.caption2).foregroundStyle(.tertiary)
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            .navigationTitle("過去バージョン")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { Button("閉じる") { dismiss() } }
            }
        }
        .onAppear(perform: load)
        // **見てから決める。** 名前と日付だけで戻すかを決めさせない。
        .sheet(item: $peek) { p in
            NavigationStack {
                ScrollView {
                    Text(p.text).font(.callout.monospaced())
                        .frame(maxWidth: .infinity, alignment: .leading).padding()
                }
                .navigationTitle(p.when)
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .topBarLeading) { Button("閉じる") { peek = nil } }
                    ToolbarItem(placement: .topBarTrailing) {
                        Button("この姿に戻す") { revert(p) }.bold()
                    }
                }
            }
        }
        .alert("できません", isPresented: Binding(
            get: { trouble != nil }, set: { if !$0 { trouble = nil } }
        )) { Button("閉じる") {} } message: { Text(trouble ?? "") }
    }

    private func size(_ v: [String: Any]) -> String {
        let b = (v["bytes"] as? NSNumber)?.doubleValue ?? 0
        return String(format: "%.1f KB", b / 1024)
    }

    private func load() {
        do {
            let out = try Cian.call("history", ["root": store.rootPath, "path": at])
            rows = out["versions"] as? [[String: Any]] ?? []
            gens = (out["gens"] as? NSNumber)?.intValue ?? 50
            days = (out["days"] as? NSNumber)?.intValue ?? 30
        } catch {
            trouble = error.localizedDescription
        }
    }

    private func noteOf(_ v: [String: Any]) -> String {
        isBook ? store.rootPath + "/" + (v["note"] as? String ?? "") : at
    }

    private func open(_ v: [String: Any]) {
        let note = noteOf(v)
        let stamp = v["stamp"] as? String ?? ""
        do {
            let out = try Cian.call("oldtext", ["root": store.rootPath, "path": note, "stamp": stamp])
            peek = Peek(when: v["when"] as? String ?? "", note: note, stamp: stamp,
                        text: out["text"] as? String ?? "")
        } catch {
            trouble = error.localizedDescription
        }
    }

    /// **戻す前に、いまの姿を一世代残す。** これが無いと「戻す」は
    /// 取り返しのつかない操作になり、押すのが怖くなる。
    private func revert(_ p: Peek) {
        do {
            _ = try Cian.call("keep", ["root": store.rootPath, "path": p.note,
                                       "gap": 0, "force": true])
            _ = try Cian.call("write", ["path": p.note, "text": p.text, "force": true])
            store.reload()
            peek = nil
            dismiss()
        } catch {
            trouble = error.localizedDescription
        }
    }
}
