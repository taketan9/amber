import SwiftUI

/// **ノートから使われていない画像を削除する**（依頼 449）。
///
/// `attachments/` に置かれた画像は、貼ったノートが消えても・その行だけ
/// 消しても、そこに残る。一枚ずつは小さいが、**消す道がどこにも無い**ので、
/// 使っているうちにフォルダだけが重くなる。
///
/// **小さく見せて、選んで消す**（本人が甲を選んだ）── 名前だけの一覧では、
/// 消していいかどうかを誰も判断できない。
///
/// 数えるのは core（`spare`）。**窓と同じ一組**なので、同じフォルダを
/// 窓と電話の両方から見ても、同じ枚数が出る。
///
/// **電話にはゴミ箱が無い。** 窓は OS のゴミ箱へ入れるので戻せるが、
/// こちらは消したら終わり ── 訊くときに、そう言う。
struct Sparing: View {
    @ObservedObject var store: NotesStore
    @Environment(\.dismiss) private var dismiss

    struct Picture: Identifiable, Hashable {
        let path: String
        let bytes: Int
        let when: Double
        /// 名前から見て、もとはどのノートのものらしいか。
        let note: String
        var id: String { path }
    }

    @State private var pictures: [Picture] = []
    @State private var unsure: [String] = []
    @State private var picked: Set<String> = []
    @State private var looking = true
    @State private var asking = false
    @State private var trouble: String?

    private let cells = [GridItem(.adaptive(minimum: 104), spacing: 10)]

    var body: some View {
        NavigationStack {
            Group {
                if looking {
                    ProgressView("数えています…")
                } else if pictures.isEmpty {
                    ContentUnavailableView(
                        "使われていない画像はありません",
                        systemImage: "checkmark.circle",
                        description: Text("どの画像も、どこかのノートから使われています。"))
                } else {
                    ScrollView {
                        if !unsure.isEmpty {
                            Text("読めなかったノートが \(unsure.count) 本あります。"
                                 + "そのノートが使っている画像も、ここに出ているかもしれません。")
                                .font(.footnote)
                                .padding(12)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .background(Color.accentColor.opacity(0.15))
                        }
                        LazyVGrid(columns: cells, spacing: 10) {
                            ForEach(pictures) { p in cell(p) }
                        }
                        .padding(12)
                    }
                }
            }
            .navigationTitle("使われていない画像")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("閉じる") { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("削除") { asking = true }
                        .disabled(picked.isEmpty)
                }
                ToolbarItem(placement: .bottomBar) {
                    if !pictures.isEmpty {
                        Button(picked.count == pictures.count ? "ぜんぶやめる" : "ぜんぶ選ぶ") {
                            picked = picked.count == pictures.count
                                ? [] : Set(pictures.map(\.path))
                        }
                    }
                }
                ToolbarItem(placement: .status) {
                    if !pictures.isEmpty {
                        Text(sum).font(.footnote).foregroundStyle(.secondary)
                    }
                }
            }
            // **戻せないことを、訊くときに言う。** 窓は OS のゴミ箱へ
            // 入れるので戻せるが、iPhone にゴミ箱は無い。
            .confirmationDialog("\(picked.count) 枚を削除しますか",
                                isPresented: $asking, titleVisibility: .visible) {
                Button("削除する", role: .destructive) { drop() }
                Button("やめる", role: .cancel) {}
            } message: {
                Text("この iPhone にゴミ箱はありません。消したものは戻せません。")
            }
            .alert("できませんでした", isPresented: Binding(
                get: { trouble != nil }, set: { if !$0 { trouble = nil } })
            ) {
                Button("閉じる") {}
            } message: {
                Text(trouble ?? "")
            }
        }
        .task { count() }
    }

    private var sum: String {
        let bytes = pictures.reduce(0) { $0 + $1.bytes }
        let head = "\(pictures.count) 枚・\(Self.size(bytes))"
        return picked.isEmpty ? head : head + "（\(picked.count) 枚を選んでいます）"
    }

    @ViewBuilder private func cell(_ p: Picture) -> some View {
        let on = picked.contains(p.path)
        VStack(spacing: 0) {
            ZStack {
                Color(.secondarySystemBackground)
                if let img = UIImage(contentsOfFile: p.path) {
                    Image(uiImage: img).resizable().scaledToFit()
                } else {
                    Image(systemName: "photo").foregroundStyle(.secondary)
                }
            }
            .frame(height: 92)
            .clipped()
            VStack(alignment: .leading, spacing: 2) {
                Text(on ? "選んでいます" : (p.note.isEmpty ? "出どころは分かりません"
                                            : "\(p.note) のもの"))
                    .font(.caption2).bold()
                    .lineLimit(1)
                Text("\(Self.size(p.bytes))・\(Self.day(p.when))")
                    .font(.caption2).foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(6)
        }
        .background(Color(.systemBackground))
        .overlay(RoundedRectangle(cornerRadius: 9)
            .stroke(on ? Color.accentColor : Color(.separator), lineWidth: on ? 2 : 1))
        .clipShape(RoundedRectangle(cornerRadius: 9))
        .onTapGesture {
            if on { picked.remove(p.path) } else { picked.insert(p.path) }
        }
    }

    private func count() {
        looking = true
        defer { looking = false }
        guard !store.rootPath.isEmpty else { pictures = []; return }
        do {
            let got = try Cian.call("spare", ["path": store.rootPath])
            pictures = (got["pictures"] as? [[String: Any]] ?? []).map {
                Picture(path: $0["path"] as? String ?? "",
                        bytes: $0["bytes"] as? Int ?? 0,
                        when: $0["when"] as? Double ?? 0,
                        note: $0["note"] as? String ?? "")
            }
            unsure = got["unsure"] as? [String] ?? []
            picked = []
        } catch {
            trouble = error.localizedDescription
        }
    }

    private func drop() {
        var gone = 0
        for at in picked {
            do {
                _ = try Cian.call("delete", ["path": at])
                gone += 1
            } catch {
                trouble = error.localizedDescription
            }
        }
        if gone > 0 { count() }
        if picked.isEmpty || gone == picked.count { dismiss() }
    }

    /// バイトを、人の読む字に。
    static func size(_ n: Int) -> String {
        if n < 1024 { return "\(n) B" }
        if n < 1024 * 1024 { return "\(Int((Double(n) / 1024).rounded())) KB" }
        return String(format: "%.1f MB", Double(n) / 1024 / 1024)
    }

    /// 秒を、その機械の日付に。
    static func day(_ sec: Double) -> String {
        if sec <= 0 { return "日付なし" }
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: Date(timeIntervalSince1970: sec))
    }
}
