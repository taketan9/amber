import SwiftUI

/// The favourites, with shelves of their own.
///
/// **A favourite is a second place a note is, not a move.** The note stays in
/// the folder it was written in; this is a different way in to the same file.
/// So the shelves here are not directories, and a note can only stand on one
/// of them — two answers to "where is this" is the thing this exists to
/// avoid.
struct Stars: View {
    @ObservedObject var store: NotesStore
    let open: (Note) -> Void
    /// Which shelf is open, `""` for the top.
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
                            // A star and not a folder: these are not
                            // directories, and drawing them as directories
                            // would promise that moving one moves files.
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
                    Button("棚を消す", role: .destructive) {
                        // The notes are not touched — only the shelf they
                        // were standing on. They are still in their folders,
                        // which is where they always were.
                        do { try store.shelf(s.path, drop: true) }
                        catch { store.trouble = error.localizedDescription }
                    }
                }
            }
            ForEach(store.starred(on: at)) { note in
                Button { open(note) } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(note.shown).font(.body.weight(.semibold)).lineLimit(1)
                        // Where it actually lives, which is the question a
                        // favourite makes you ask.
                        Label(note.book.isEmpty ? store.rootName : note.book, systemImage: "folder")
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
                    Button { moving = note } label: { Label("棚を変える", systemImage: "star") }
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
                .accessibilityLabel("新しい棚")
            }
        }
        .alert("新しい棚", isPresented: $making) {
            TextField("名前", text: $name)
            Button("やめる", role: .cancel) {}
            Button("作る") {
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

/// Choosing which shelf a favourite stands on.
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
                    row("デフォルト", "")
                    ForEach(store.stars, id: \.self) { s in row(s, s) }
                }
                // **Making a shelf from where you need one.** It was only
                // possible from inside the favourites screen, which is only
                // reachable once something is already a favourite — so the
                // first shelf could not be made at the moment anybody wanted
                // one, and the list looked like it had no shelves at all.
                Section {
                    Button {
                        name = ""
                        making = true
                    } label: {
                        Label("新しい棚…", systemImage: "plus.rectangle.on.folder")
                    }
                }
            }
            .navigationTitle("棚を選ぶ")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("やめる") { dismiss() } }
            }
            .alert("新しい棚", isPresented: $making) {
                TextField("名前", text: $name)
                Button("やめる", role: .cancel) {}
                Button("作る") {
                    let n = name.trimmingCharacters(in: .whitespaces)
                    guard !n.isEmpty else { return }
                    do {
                        try store.shelf(n)
                        try store.star(note, on: n)
                        dismiss()
                    } catch { store.trouble = error.localizedDescription }
                }
            } message: {
                Text("作ってから、このノートをそこに入れます。「棚/中の棚」と書けば階層になります。")
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

/// The colour a folder is.
///
/// **Free choice, not a cyan-only palette.** The point of a colour on a
/// folder is telling folders apart at a glance, and a palette that keeps them
/// all in one hue defeats that.
struct Colouring: View {
    @ObservedObject var store: NotesStore
    let folder: String
    @Environment(\.dismiss) private var dismiss

    static let palette: [(String, String)] = [
        ("#0E93A8", "シアン"), ("#2AA79B", "みどり青"), ("#3D7FA8", "青"),
        ("#5E6FA8", "青むらさき"), ("#8A6BA8", "むらさき"), ("#B85C8A", "もも"),
        ("#C4544C", "あか"), ("#D9822B", "だいだい"), ("#B39429", "からし"),
        ("#4E8C43", "みどり"), ("#7A7F86", "はいいろ"),
    ]

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
    /// **`.alwaysOriginal` or it comes out cyan.** A menu tints its images
    /// with the accent like any other symbol, so a palette drawn with
    /// `systemImage:` is eleven identical cyan dots — which is exactly what
    /// it was, until somebody looked.
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
    /// `#RRGGBB` as written in the settings file. `nil` for anything else —
    /// a colour somebody typed by hand into the file should not take the
    /// list down with it.
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
