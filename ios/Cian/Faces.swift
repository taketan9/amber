import SwiftUI

/// 絵文字の板（依頼 418）。**押して入れるだけ。**
///
/// `:tada:` のような書き方は入れない（本人：「僕でも :tada とか打たない」・
/// 2026-09-09）── 覚える記法が増えるだけで、ノートに残るのは同じ一文字。
///
/// 表は core が持っている ── 窓と電話で並びも名前も違う、を作らない。
/// 外の何かを取りに行かないので、電波の無いところでも同じものが出る。
struct Faces: View {
    /// 選ばれた絵文字。ここで閉じるかどうかは呼んだ側が決める。
    let put: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    /// 最近つかったもの。**憶えるのは字だけ** ── 名前は表から引ける。
    @AppStorage("amber.faces") private var usedRaw = ""
    @State private var table: FaceTable?
    @State private var trouble: String?
    @State private var find = ""
    /// いま見ている束（`0` は「最近つかったもの」）。
    @State private var tab = 0

    private var used: [String] {
        usedRaw.isEmpty ? (table?.first ?? []) : usedRaw.map(String.init)
    }

    /// 一行に八つ ── 窓の板と同じ数（同じノートを二つの端末で見る人が、
    /// 同じ場所に同じ絵を見つけられる）。
    private let cols = Array(repeating: GridItem(.flexible(), spacing: 2), count: 8)

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                if let table {
                    tabs(table)
                    Divider()
                    ScrollView {
                        let rows = shown(table)
                        if rows.isEmpty {
                            Text("見つかりません")
                                .font(.footnote).foregroundStyle(.secondary)
                                .padding(.top, 40)
                        } else {
                            LazyVGrid(columns: cols, spacing: 2) {
                                ForEach(rows, id: \.ch) { x in
                                    Button {
                                        tapped(x.ch)
                                    } label: {
                                        Text(x.ch).font(.system(size: 27))
                                            .frame(maxWidth: .infinity, minHeight: 42)
                                    }
                                    .buttonStyle(.plain)
                                    .accessibilityLabel(x.words.split(separator: " ").first
                                        .map(String.init) ?? x.ch)
                                }
                            }
                            .padding(.horizontal, 10)
                            .padding(.top, 6)
                        }
                    }
                } else if let trouble {
                    Text(trouble).font(.footnote).foregroundStyle(.secondary).padding()
                } else {
                    ProgressView().padding(.top, 40)
                    Spacer()
                }
            }
            .searchable(text: $find, placement: .navigationBarDrawer(displayMode: .always),
                        prompt: "絵文字を探す（おめでとう、など）")
            .navigationTitle("絵文字")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("閉じる") { dismiss() }
                }
            }
        }
        .task {
            guard table == nil else { return }
            do { table = try FaceTable.load() }
            catch { trouble = "絵文字が出せません: " + error.localizedDescription }
        }
    }

    /// 束の帯。先頭は「最近つかったもの」── 二度目からはここだけで済む。
    private func tabs(_ t: FaceTable) -> some View {
        HStack(spacing: 2) {
            ForEach(Array((["🕘"] + t.groups.map(\.icon)).enumerated()), id: \.offset) { n, icon in
                Button {
                    tab = n
                    find = ""
                } label: {
                    Text(icon).font(.system(size: 19))
                        .frame(maxWidth: .infinity, minHeight: 32)
                        .background(n == tab ? Color.accentColor.opacity(0.22) : .clear,
                                    in: RoundedRectangle(cornerRadius: 7))
                        .opacity(n == tab ? 1 : 0.55)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 5)
    }

    /// いま出す顔ぶれ。**探しているときは束を無視する** ── 探している人が
    /// 知りたいのは字であって、どの束に居るかではない。
    private func shown(_ t: FaceTable) -> [Face] {
        let all = t.groups.flatMap(\.faces)
        let q = find.trimmingCharacters(in: .whitespaces).lowercased()
        if !q.isEmpty {
            return all.filter { $0.ch == q || $0.words.lowercased().contains(q) }
        }
        if tab == 0 {
            return used.map { ch in
                all.first { $0.ch == ch } ?? Face(ch: ch, words: "")
            }
        }
        return t.groups[tab - 1].faces
    }

    private func tapped(_ ch: String) {
        put(ch)
        // 同じものは前へ出す（二つに増やさない）。24 まで。
        var now = used.filter { $0 != ch }
        now.insert(ch, at: 0)
        usedRaw = now.prefix(24).joined()
    }
}

/// core から受け取った表。
struct Face: Hashable {
    let ch: String
    let words: String
}

struct FaceGroup: Hashable {
    let name: String
    let icon: String
    let faces: [Face]
}

struct FaceTable {
    let first: [String]
    let groups: [FaceGroup]

    /// **一度だけ取りに行く。** 表は変わらないので、開くたびに訊かない。
    static var kept: FaceTable?

    static func load() throws -> FaceTable {
        if let kept { return kept }
        let got = try Cian.call("emoji", [:])
        let first = (got["first"] as? [String]) ?? []
        let groups = ((got["groups"] as? [[String: Any]]) ?? []).map { g in
            FaceGroup(
                name: g["name"] as? String ?? "",
                icon: g["icon"] as? String ?? "",
                faces: ((g["faces"] as? [[String: Any]]) ?? []).map {
                    Face(ch: $0["ch"] as? String ?? "", words: $0["words"] as? String ?? "")
                })
        }
        let made = FaceTable(first: first, groups: groups)
        kept = made
        return made
    }
}
