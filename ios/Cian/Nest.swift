import SwiftUI

/// あるものすべてを、そのままの構造で ── フォルダを開くと中身が出て、
/// ノートもそこに並ぶ。
///
/// 2026-09-05: 「フォルダの構成という表示がすごくわかりやすい。デフォルトを
/// この見え方にして、＋ノートもその中にあるものを表示させる」. The list that
/// 1 階層ずつ見せる形はフォルダを*使う*のに向いている。こちらはフォルダを
/// *把握する*のに向いていて、ノートアプリの用途はたいていそちらだった。
///
/// **お気に入りを先頭に。フォルダとしては出さない。** あれはノートの 2 つ目の
/// 居場所なので、ディレクトリに混ざってフォルダのふりをするのではなく、
/// 星印を付けていちばん上に開く。
struct Nest: View {
    @ObservedObject var store: NotesStore
    let open: (Note) -> Void
    /// 描くのは呼び出し側。ここの行と向こうの行が同じ見た目になるように。
    let row: (Note) -> AnyView

    var body: some View {
        Group {
            if !store.stars.isEmpty || store.notes.contains(where: { $0.star != nil }) {
                Section {
                    branch(star: "", depth: 0)
                } header: {
                    // `textCase(nil)` を付けないと、リストの見出しは名前を
                    // 大文字で返してくる ──「cian」が「CIAN」になり、
                    // 人が付けたフォルダ名が別のものになる。
                    Label("ブックマーク", systemImage: "star.fill")
                        .foregroundStyle(.orange)
                        .textCase(nil)
                }
            }
            Section {
                branch(book: "", depth: 0)
            } header: {
                Label(store.rootName, systemImage: "tray.full.fill")
                    .foregroundStyle(.tint)
                    .textCase(nil)
            }
        }
    }

    // MARK: folders

    /// フォルダ 1 つの中身 ── 配下のフォルダ、そのあとに自分のノート。
    ///
    /// インデント付きの平らな一覧ではなく再帰にしてある ── 開閉の三角は
    /// 自分の配下を隠さなければならず、平らな一覧ではどの行がそれに当たるかを
    /// 自力で計算することになる。
    ///
    /// **`AnyView` はここだけで使う。** 自分自身を含む View は自分自身を含む型に
    /// なり、Swift はそれを推論できない ── エラーは "defines the opaque type in
    /// terms of itself" と言う。再帰が起きるこの 1 か所だけで型を消すのが、
    /// ツリー表示の代償。
    private func branch(book: String, depth: Int) -> AnyView {
        AnyView(branchBody(book: book, depth: depth))
    }

    @ViewBuilder
    private func branchBody(book: String, depth: Int) -> some View {
        ForEach(store.shelfless(in: book), id: \.path) { b in
            DisclosureGroup(isExpanded: store.opened(b.path)) {
                branch(book: b.path, depth: depth + 1)
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: "folder.fill")
                        .foregroundStyle(store.colors[b.path].flatMap { Color(hex: $0) }
                            .map { AnyShapeStyle($0) } ?? AnyShapeStyle(.tint))
                    Text(b.name).lineLimit(1)
                    Spacer(minLength: 6)
                    Text("\(b.count)").foregroundStyle(.secondary).monospacedDigit().font(.caption)
                }
            }
            .padding(.leading, CGFloat(depth) * 10)
        }
        ForEach(store.notes.filter { store.here($0) && $0.book == book }.sorted { $0.title < $1.title }) { note in
            row(note).padding(.leading, CGFloat(depth) * 10 + 8)
        }
    }

    // MARK: お気に入りのフォルダ

    private func branch(star: String, depth: Int) -> AnyView {
        AnyView(branchBody(star: star, depth: depth))
    }

    @ViewBuilder
    private func branchBody(star: String, depth: Int) -> some View {
        ForEach(store.shelves(in: star), id: \.path) { s in
            DisclosureGroup(isExpanded: store.opened("★" + s.path)) {
                branch(star: s.path, depth: depth + 1)
            } label: {
                HStack(spacing: 8) {
                    // フォルダではなく星印にする。お気に入りはディレクトリでは
                    // なく、フォルダとして描くと「動かせばノートも動く」と
                    // files.
                    Image(systemName: "star.square.fill").foregroundStyle(.orange)
                    Text(s.name).lineLimit(1)
                    Spacer(minLength: 6)
                    Text("\(s.count)").foregroundStyle(.secondary).monospacedDigit().font(.caption)
                }
            }
            .padding(.leading, CGFloat(depth) * 10)
        }
        ForEach(store.starred(on: star)) { note in
            row(note).padding(.leading, CGFloat(depth) * 10 + 8)
        }
    }
}
