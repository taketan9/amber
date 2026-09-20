import SwiftUI

/// お気に入りと、その中のフォルダ。
///
/// **お気に入りはノートの 2 つ目の居場所であって、移動ではない。** ノートは
/// 書かれたフォルダに残り、ここは同じファイルへの別の入口。
/// だからここのフォルダはディレクトリではないし、ノートが入れるのは 1 つだけ ──
/// 「これはどこにあるか」の答えが 2 つになるのは、この仕組みが避けるべき
/// avoid.
struct Stars: View {
    @ObservedObject var store: NotesStore
    let open: (Note) -> Void
    /// どのフォルダを開いているか。`""` は最上位。
    @State private var at = ""
    @State private var making = false
    @State private var name = ""
    @State private var moving: Note?

    var body: some View {
        List {
            if !at.isEmpty {
                Button {
                    at = up
                } label: {
                    Label("..", systemImage: "arrow.up.left").foregroundStyle(.tint)
                }
                .buttonStyle(.plain)
            }
            ForEach(store.shelves(in: at), id: \.path) { s in
                Button { at = s.path } label: {
                    HStack {
                        Label {
                            Text(s.name)
                        } icon: {
                            // フォルダではなく星印にする。これらはディレクトリでは
                            // なく、ディレクトリとして描くと「動かせばファイルも
                            // 動く」と約束することになる。
                            Image(systemName: "star.square.fill").foregroundStyle(.orange)
                        }
                        Spacer()
                        Text("\(s.count)").foregroundStyle(.secondary).monospacedDigit()
                        Image(systemName: "chevron.right").font(.caption).foregroundStyle(.tertiary)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .swipeActions {
                    Button("ブックマークグループを消す", role: .destructive) {
                        // ノートには触っていない ── 変わるのは入っていた
                        // お気に入りのフォルダだけ。ノートは元のフォルダに
                        // あり、それはずっと変わっていない。
                        do { try store.shelf(s.path, drop: true) }
                        catch { store.trouble = error.localizedDescription }
                    }
                }
            }
            ForEach(store.starred(on: at)) { note in
                Button { open(note) } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(note.shown).font(.body.weight(.semibold)).lineLimit(1)
                        // 実際にどこにあるか ── お気に入りがあると
                        // 訊きたくなる問い。
                        Label(store.bookLabel(note), systemImage: "folder")
                            .font(.caption2).foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .swipeActions {
                    Button("外す") {
                        do { try store.star(note, on: nil) }
                        catch { store.trouble = error.localizedDescription }
                    }
                    .tint(.orange)
                }
                .contextMenu {
                    Button { moving = note } label: { Label("ブックマークグループを変える", systemImage: "star") }
                }
            }
            if store.shelves(in: at).isEmpty && store.starred(on: at).isEmpty {
                Text("ここには何もありません。ノートを左にスワイプして ⭐️ を押すと、ここに並びます。")
                    .font(.callout).foregroundStyle(.secondary)
            }
        }
        .navigationTitle(at.isEmpty ? "ブックマーク" : (at.split(separator: "/").last.map(String.init) ?? at))
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button { name = ""; making = true } label: {
                    Image(systemName: "plus.rectangle.on.folder")
                }
                .accessibilityLabel("新しいブックマークグループ")
            }
        }
        .alert("新しいブックマークグループ", isPresented: $making) {
            TextField("名前", text: $name)
            Button("やめる", role: .cancel) {}
            Button("作成") {
                let n = name.trimmingCharacters(in: .whitespaces)
                guard !n.isEmpty else { return }
                do { try store.shelf(at.isEmpty ? n : "\(at)/\(n)") }
                catch { store.trouble = error.localizedDescription }
            }
        } message: {
            Text(at.isEmpty ? "ブックマークの中に作ります" : "「\(at)」の中に作ります")
        }
        .sheet(item: $moving) { note in
            Shelving(store: store, note: note)
        }
    }

    private var up: String {
        at.split(separator: "/").dropLast().joined(separator: "/")
    }
}

/// お気に入りをどのフォルダに入れるか選ぶ。
struct Shelving: View {
    @ObservedObject var store: NotesStore
    let note: Note
    @Environment(\.dismiss) private var dismiss
    @State private var making = false
    @State private var name = ""

    var body: some View {
        NavigationStack {
            List {
                Section {
                    row("（トップページ）", "")
                    ForEach(store.stars, id: \.self) { s in row(s, s) }
                }
                // **必要になった場所でフォルダを作れるようにした。** 以前は
                // お気に入りの画面からしか作れず、そこへは何かが既に
                // お気に入りになっていないと辿り着けなかった ── つまり
                // 最初の 1 つは欲しい瞬間に作れず、一覧はフォルダが
                // 1 つも無いように見えていた。
                Section {
                    Button {
                        name = ""
                        making = true
                    } label: {
                        Label("新しいブックマークグループ…", systemImage: "plus.rectangle.on.folder")
                    }
                }
            }
            .navigationTitle("ブックマークグループを選ぶ")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("やめる") { dismiss() } }
            }
            .alert("新しいブックマークグループ", isPresented: $making) {
                TextField("名前", text: $name)
                Button("やめる", role: .cancel) {}
                Button("作成") {
                    let n = name.trimmingCharacters(in: .whitespaces)
                    guard !n.isEmpty else { return }
                    do {
                        try store.shelf(n)
                        try store.star(note, on: n)
                        dismiss()
                    } catch { store.trouble = error.localizedDescription }
                }
            } message: {
                Text("作ってから、このノートをそこに入れます。「グループ/中のグループ」と書けば階層になります")
            }
        }
    }

    private func row(_ label: String, _ shelf: String) -> some View {
        Button {
            do { try store.star(note, on: shelf) } catch { store.trouble = error.localizedDescription }
            dismiss()
        } label: {
            HStack {
                Text(label)
                Spacer()
                if note.star == shelf {
                    Image(systemName: "checkmark").foregroundStyle(.tint)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// フォルダの色。
///
/// **シアン系だけのパレットではなく、自由に選べるようにする。** フォルダに
/// 色を付けるのは一目で見分けるためで、全部同じ色相のパレットでは
/// その目的を潰す。
struct Colouring: View {
    @ObservedObject var store: NotesStore
    let folder: String
    @Environment(\.dismiss) private var dismiss

    /// フォルダに付けられる色。**core に訊く。**
    ///
    /// 前はこことデスクトップ版の `PALETTE` に同じ表を書いていて、両方のコメントに
    /// 「同じ並び」と書いてあった ── それでも**十一色のうち六色がずれて
    /// いた**。iPhone で付けた青が、Mac では少し違う青で出ていた。
    static let palette: [(String, String)] = {
        guard let out = try? Cian.call("palette", [:]),
              let rows = out["colors"] as? [[String: Any]]
        else { return [] }
        return rows.compactMap { r in
            guard let hex = r["hex"] as? String, let name = r["name"] as? String
            else { return nil }
            return (hex, name)
        }
    }()

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(Self.palette, id: \.0) { hex, name in
                        Button {
                            pick(hex)
                        } label: {
                            HStack(spacing: 12) {
                                RoundedRectangle(cornerRadius: 5)
                                    .fill(Color(hex: hex) ?? .gray)
                                    .frame(width: 22, height: 22)
                                Text(name)
                                Spacer()
                                if store.colors[folder] == hex {
                                    Image(systemName: "checkmark").foregroundStyle(.tint)
                                }
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                Section {
                    Button("色をなくす", role: .destructive) { pick(nil) }
                }
            }
            .navigationTitle(folder.split(separator: "/").last.map(String.init) ?? folder)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("やめる") { dismiss() } }
            }
        }
    }

    private func pick(_ hex: String?) {
        do { try store.color(folder, hex) } catch { store.trouble = error.localizedDescription }
        dismiss()
    }
}

extension Colouring {
    /// A dot of one colour, for a menu.
    ///
    /// **`.alwaysOriginal` を付けないとシアンになる。** メニューはほかの記号と
    /// 同じようにアクセント色で画像を染めるので、`systemImage:` で描いた
    /// パレットは同じシアンの丸が 11 個並ぶ ── 誰かが見るまで、実際に
    /// そうなっていた。
    static func dot(_ hex: String) -> UIImage {
        let side = 16.0
        let r = UIGraphicsImageRenderer(size: CGSize(width: side, height: side))
        let img = r.image { ctx in
            (UIColor(Color(hex: hex) ?? .gray)).setFill()
            ctx.cgContext.fillEllipse(in: CGRect(x: 0, y: 0, width: side, height: side))
        }
        return img.withRenderingMode(.alwaysOriginal)
    }
}

extension Color {
    /// 設定ファイルに書かれたままの `#RRGGBB`。それ以外は `nil` ── 人が手で
    /// ファイルに書いた色が、一覧ごと落とすことがないように。
    ///
    init?(hex: String) {
        var s = hex.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("#") { s.removeFirst() }
        guard s.count == 6, let n = UInt32(s, radix: 16) else { return nil }
        self.init(
            red: Double((n >> 16) & 0xFF) / 255,
            green: Double((n >> 8) & 0xFF) / 255,
            blue: Double(n & 0xFF) / 255
        )
    }
}
