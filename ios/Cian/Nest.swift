import SwiftUI

/// Everything there is, in the shape it is in — folders opening to show what
/// is inside them, notes included.
///
/// 2026-09-05: 「フォルダの構成という表示がすごくわかりやすい。デフォルトを
/// この見え方にして、＋ノートもその中にあるものを表示させる」. The list that
/// shows one level at a time is right for *using* a folder; this is right for
/// *knowing* one, and knowing turns out to be what a notes app is mostly for.
///
/// **Favourites first, and not as a folder.** They are a second place a note
/// is, so they open at the top with a star rather than sitting among the
/// directories pretending to be one.
struct Nest: View {
    @ObservedObject var store: NotesStore
    let open: (Note) -> Void
    /// Drawn by the caller, so a row here looks like a row there.
    let row: (Note) -> AnyView

    var body: some View {
        Group {
            if !store.stars.isEmpty || store.notes.contains(where: { $0.star != nil }) {
                Section {
                    branch(star: "", depth: 0)
                } header: {
                    // `textCase(nil)` or a list header shouts the name back
                    // in capitals — which turns 「cian」 into 「CIAN」 and a
                    // folder somebody named into something they did not.
                    Label("お気に入り", systemImage: "star.fill")
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

    /// One folder's contents: the folders under it, then its own notes.
    ///
    /// Recursion rather than a flattened list with indents: the disclosure
    /// arrow has to hide what is under it, and a flat list would have to work
    /// out for itself which rows those are.
    ///
    /// **`AnyView`, and only here.** A view that contains itself has a type
    /// that contains itself, and Swift will not infer one — the error says
    /// "defines the opaque type in terms of itself". Erasing at the one place
    /// the recursion happens is the price of a tree.
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
        ForEach(store.notes.filter { $0.book == book }.sorted { $0.title < $1.title }) { note in
            row(note).padding(.leading, CGFloat(depth) * 10 + 8)
        }
    }

    // MARK: favourite shelves

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
                    // A star and not a folder: a shelf is not a directory,
                    // and drawing it as one promises that moving it moves
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
