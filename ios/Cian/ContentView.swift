import SwiftUI
import UniformTypeIdentifiers
import PhotosUI

/// The list of notes, and one note open.
///
/// A row is a title and the line under it — the same two lines the cian view
/// draws in the window, for the same reason: the eye runs down the titles and
/// only drops into the second line when it has stopped somewhere.
struct ContentView: View {
    @StateObject private var store = NotesStore()
    @StateObject private var desk = Desk()
    @ObservedObject private var ring = Ring.shared
    @State private var picking = false
    @State private var naming = false
    @State private var booking = false
    /// What the one file picker is being asked for this time.
    @State private var fetching: Fetching?
    enum Fetching { case folder, notes }
    @State private var showing = false
    /// Which folder row the finger is over, and whether it is over `..`.
    @State private var into: String?
    @State private var outside = false
    @State private var needle = ""
    @State private var shelving: Note?
    @State private var colouring: String?
    @State private var renaming: String?
    @State private var dropping: String?
    @State private var fresh = ""
    @State private var treeing = false
    /// The folder the list is drawn for, and which way it last moved.
    @State private var walked = ""
    @State private var deeper = true
    @State private var wide: CGFloat = 393

    var body: some View {
        NavigationStack {
            Group {
                if store.rootName.isEmpty {
                    empty
                } else {
                    list
                }
            }
            // The notebook, when one is chosen: the title bar is where you
            // look to know what you are looking at, and a filtered list that
            // still says the folder's name reads as a list that lost notes.
            // At the top the name is drawn in the list, so the bar stays
            // out of the way; inside a folder the bar says where you are.
            // The trail is drawn in the list, so the bar stays out of the
            // way — a large title saying the folder's name *and* a
            // breadcrumb saying the same name is the name twice.
            .navigationTitle("")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if let up = store.up {
                    ToolbarItem(placement: .topBarLeading) {
                        Button { go { store.leave(for: up) } } label: {
                            Label("上へ", systemImage: "chevron.backward")
                        }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { picking = true } label: { Image(systemName: "gearshape") }
                        .accessibilityLabel("設定")
                }
                if !store.rootName.isEmpty {
                    // Making a folder is a top-level thing to do and was
                    // buried in the sort menu, where nobody found it.
                    ToolbarItem(placement: .topBarTrailing) {
                        Button { booking = true } label: {
                            Image(systemName: "folder.badge.plus")
                        }
                        .accessibilityLabel("新しいフォルダ")
                    }
                    ToolbarItem(placement: .topBarTrailing) {
                        // The same `.badge.plus` as the folder beside it:
                        // two buttons that both mean "a new one of these"
                        // should be built the same way, or the pair reads as
                        // two unrelated things that happen to sit together.
                        Button { naming = true } label: {
                            Image(systemName: "note.text.badge.plus")
                        }
                        .accessibilityLabel("新しいノート")
                    }
                    // Both narrowings in one menu: which notebook, and in what
                    // order. They are the two questions asked of a list that
                    // has grown, and a toolbar with a button each would leave
                    // no room for the ones that make a note.
                    ToolbarItem(placement: .topBarTrailing) {
                        Menu {
                            Picker("並び", selection: $store.order) {
                                ForEach(NotesStore.Order.allCases) { Text($0.label).tag($0) }
                            }
                            Divider()
                            Toggle(isOn: $store.tree) {
                                Label("フォルダごと（ツリー）", systemImage: "list.bullet.indent")
                            }
                            Toggle(isOn: $store.flat) {
                                Label("全部まとめて見る", systemImage: "list.bullet")
                            }
                            Divider()
                            Button { treeing = true } label: {
                                Label("フォルダの構成…", systemImage: "list.bullet.indent")
                            }

                        } label: {
                            Image(systemName: store.flat
                                ? "line.3.horizontal.decrease.circle.fill"
                                : "line.3.horizontal.decrease.circle")
                        }
                        .accessibilityLabel("並びとフォルダ")
                    }
                }
            }
        }
        .sheet(isPresented: $picking) {
            // The sheet closes itself first; these open a beat later, from
            // here, where there is no presentation in the way.
            Where(store: store,
                  choose: { DispatchQueue.main.async { fetching = .folder } },
                  bringIn: { DispatchQueue.main.async { fetching = .notes } })
        }
        .sheet(item: $shelving) { note in Shelving(store: store, note: note) }
        .sheet(item: $colouring) { f in Colouring(store: store, folder: f) }
        .sheet(isPresented: $treeing) {
            Tree(store: store) { to in go { store.into(to) } }
        }
        .alert("名前を変える", isPresented: Binding(
            get: { renaming != nil }, set: { if !$0 { renaming = nil } }
        )) {
            TextField("名前", text: $fresh)
            Button("やめる", role: .cancel) {}
            Button("変える") {
                guard let b = renaming else { return }
                do { try store.rename(b, to: fresh) }
                catch { store.trouble = error.localizedDescription }
            }
        } message: {
            Text("中のノートはそのままです。")
        }
        // **Said before it is done, and said in numbers.** There is no
        // wastepaper basket on a phone: this is the real thing, and 「中の
        // ノートごと」 is not a figure of speech.
        .alert("このフォルダを削除しますか", isPresented: Binding(
            get: { dropping != nil }, set: { if !$0 { dropping = nil } }
        )) {
            Button("やめる", role: .cancel) {}
            Button("中のノートごと削除", role: .destructive) {
                guard let b = dropping else { return }
                do { try store.drop(b) }
                catch { store.trouble = error.localizedDescription }
            }
        } message: {
            if let b = dropping {
                let n = store.under(b)
                Text(n == 0
                     ? "「\(b.split(separator: "/").last.map(String.init) ?? b)」は空です。元には戻せません。"
                     : "「\(b.split(separator: "/").last.map(String.init) ?? b)」の中のノート \(n) 本も一緒に消えます。元には戻せません。")
            }
        }
        .sheet(isPresented: $booking) {
            Booking(inside: store.here) { name in
                do { try store.makeBook(name) }
                catch { store.trouble = error.localizedDescription }
            }
        }
        // A notification pressed opens the note it was about. It can arrive
        // before the notes are loaded (pressed from the lock screen, cold
        // start), so this watches the store as well as the ring: whichever
        // is second does the opening.
        .onChange(of: ring.wanted) { _, _ in answer() }
        .onChange(of: store.notes) { _, _ in answer() }
        .task {
            store.restore()
            // What the routines owed while the phone was doing something
            // else. Asked for once, on the way in — see `Bell` for why this
            // is the moment and not nine on a Wednesday.
            _ = await Bell.ask()
            store.catchUp()
        }
        // **One `.fileImporter`, and one only.** Two on the same view is one
        // importer — SwiftUI keeps the last and the other button does
        // nothing at all. That is written two lines above where it happened
        // for the second time: 「保存場所を選ぶ」 lost to 「インポート」 and
        // pressing it looked like a button that was never wired up. So there
        // is one, and what it is picking decides what it accepts.
        .fileImporter(
            isPresented: Binding(get: { fetching != nil },
                                 set: { if !$0 { fetching = nil } }),
            allowedContentTypes: fetching == .folder
                ? [.folder]
                : [UTType(filenameExtension: "md") ?? .plainText, .plainText],
            allowsMultipleSelection: fetching == .notes
        ) { r in
            switch (fetching, r) {
            case (.folder, .success(let urls)):
                if let url = urls.first { store.choose(url) }
            case (.notes, .success(let urls)):
                store.bring(urls)
            case (_, .failure(let why)):
                store.trouble = why.localizedDescription
            case (nil, _):
                break
            }
            fetching = nil
        }
        // **Two `.alert` on one view is one alert.** SwiftUI keeps the last
        // and the other one shows without its buttons doing anything — which
        // is exactly what the delete confirmation did: it appeared, both
        // answers were inert, and nothing said why. This one lives a level up,
        // on the stack rather than on the list.
        .alert(
            "できません",
            isPresented: Binding(get: { store.trouble != nil }, set: { if !$0 { store.trouble = nil } })
        ) {
            Button("閉じる") {}
        } message: {
            Text(store.trouble ?? "")
        }
        .sheet(isPresented: $naming) {
            Making(make: { title, tags in
                if let note = make(title, tags) {
                    // Straight into it, in the writing half — you asked for
                    // it in order to write in it.
                    desk.open(note, writing: true)
                    showing = true
                }
            }, known: store.allTags)
        }

    }

    /// `try?` here would be the whole bug: a delete that fails silently looks
    /// exactly like a delete that was never asked for, and the row stays.
    /// `try?` here would be the whole bug: a delete that fails silently looks
    /// exactly like a delete that was never asked for, and the row stays.
    private func remove(_ note: Note) {
        do { try store.remove(note) } catch { store.trouble = error.localizedDescription }
    }

    @ViewBuilder
    private func row(_ note: Note) -> some View {
                Button {
                desk.open(note)
                showing = true
            } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 4) {
                            if note.star != nil {
                                Image(systemName: "star.fill")
                                    .font(.caption2).foregroundStyle(.orange)
                            }
                            Text(note.title).font(.body.weight(.semibold)).lineLimit(1)
                        }
                        // The line the word was actually on, when there is one:
                        // showing the note's opening instead would be answering a
                        // question nobody asked.
                        if let hit = store.hits[note.path], !needle.isEmpty {
                            Text(hit).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        } else if !note.excerpt.isEmpty {
                            Text(note.excerpt).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                        if !note.tags.isEmpty || !note.book.isEmpty {
                            HStack(spacing: 6) {
                                if !note.book.isEmpty {
                                    // The notebook first: it says *where*, and
                                    // where is what tells two same-named notes
                                    // apart. Quieter than the tags, which are a
                                    // thing you chose rather than a place.
                                    Label(note.book, systemImage: "folder")
                                        .font(.caption2).foregroundStyle(.tint.opacity(0.8))
                                }
                                if !note.tags.isEmpty {
                                    Text(note.tags.map { "#\($0)" }.joined(separator: " "))
                                        .font(.caption2).foregroundStyle(.tint)
                                }
                            }
                        }
                    }
                    // The whole row, not the words in it. A label is as wide
                    // as its longest line, so a short title left most of the
                    // row dead to the finger — and a row that answers in one
                    // place and not the one beside it reads as a bug.
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                // The row keeps the colours its own text asked for — a title
                // in link-blue says "this is a link" about every note in the
                // list, which is the one thing they all are.
                .buttonStyle(.plain)
                // Pick a note up by its row. The path is what travels: it is
                // what every one of these actions takes, and a note is not a
                // thing that can be halfway between two folders.
                .draggable(note.path)
                // Swipe, then tap — the two steps *are* the confirmation, which
                // is how Apple's own Notes does it. `allowsFullSwipe: false` so a
                // long swipe cannot delete on its own: there is no trash on a
                // phone, and this is the one action here that cannot be undone.
                //
                // An alert asking again was written and taken out: its destructive
                // button did not fire under the automated taps that drive these
                // checks, while its cancel did, and I could not explain the
                // difference. Shipping a confirmation I have not seen work would
                // be worse than the gesture that I have.
                .swipeActions(allowsFullSwipe: false) {
                    Button("削除", role: .destructive) { remove(note) }
                }
                // Pinning on the other side, where a swipe that starts left is
                // for keeping and one that starts right is for losing.
                .swipeActions(edge: .leading) {
                    Button {
                        do { try store.star(note, on: note.star == nil ? "" : nil) }
                        catch { store.trouble = error.localizedDescription }
                    } label: {
                        Label(note.star == nil ? "お気に入り" : "外す",
                              systemImage: note.star == nil ? "star" : "star.slash")
                    }
                    .tint(.orange)
                }
                .contextMenu {
                    // The order they are reached for. Favouriting is the one
                    // done in passing; moving is filing; exporting is the one
                    // that leaves cian, and leaving is always last.
                    Button { shelving = note } label: {
                        Label(note.star == nil ? "お気に入りに登録する" : "棚を変える", systemImage: "star")
                    }
                    // Every notebook, not just the ones beside this note —
                    // filing is often filing *away*.
                    Menu("フォルダへ移す") {
                        Button("デフォルト") { moveTo(note, nil) }
                        ForEach(store.allBooks, id: \.self) { b in
                            Button(b) { moveTo(note, b) }
                        }
                    }
                    // One note, out to wherever — Files, Drive, Dropbox,
                    // mail. The system sheet does all of those, so cian does
                    // not have to know any of them by name.
                    ShareLink(item: URL(fileURLWithPath: note.path)) {
                        Label("書き出す…", systemImage: "square.and.arrow.up")
                    }
                }
    }

    /// Notes dropped somewhere. `nil` is the top of the folder.
    ///
    /// Returns whether anything moved, which is what tells the phone to keep
    /// the drop animation rather than snapping the row back.
    private func drop(_ paths: [String], into book: String?) -> Bool {
        var moved = false
        for path in paths {
            guard let note = store.notes.first(where: { $0.path == path }) else { continue }
            do { try store.move(note, to: book); moved = true }
            catch { store.trouble = error.localizedDescription }
        }
        into = nil
        outside = false
        return moved
    }

    /// Open the note a notification was about, once it is known.
    private func answer() {
        guard let want = ring.wanted else { return }
        guard let note = store.notes.first(where: { $0.path == want }) else { return }
        ring.wanted = nil
        desk.open(note)
        showing = true
    }

    private func moveTo(_ note: Note, _ book: String?) {
        do { try store.move(note, to: book) } catch { store.trouble = error.localizedDescription }
    }

    private func make(_ title: String, _ tags: [String]) -> Note? {
        do { return try store.make(titled: title, tags: tags) }
        catch { store.trouble = error.localizedDescription; return nil }
    }

    private var empty: some View {
        ContentUnavailableView {
            Label("ノートの保存場所", systemImage: "folder.badge.questionmark")
        } description: {
            // Named rather than "choose a folder": the point is that it can be
            // the folder the Mac already has, wherever it is kept.
            Text("マークダウンのノートがあるフォルダを選びます。iCloud Drive・Google Drive・Dropbox のどれでも構いません。")
        } actions: {
            Button("保存場所を見る") { picking = true }.buttonStyle(.borderedProminent)
        }
    }

    /// Move, and let it be seen moving.
    ///
    /// **A list that changes instantly reads as a list that did not change.**
    /// The finger lands, the contents are already different, and the eye has
    /// nothing to follow — so you press again to check whether it worked.
    /// Which way it slides says which way you went; sliding the same way in
    /// both directions would be worse than not sliding at all.
    private func go(_ act: () -> Void) {
        let was = store.at
        act()
        guard store.at != was else { return }
        deeper = store.at.count > was.count
        withAnimation(.easeOut(duration: 0.24)) { walked = store.at }
    }

    /// Whether the tree is what is being drawn right now.
    ///
    /// One answer, asked in four places — the folder rows, the favourites,
    /// the tree itself and the bands all have to agree, and four copies of
    /// the same condition is how three of them end up agreeing.
    private var treeing2: Bool {
        store.tree && needle.isEmpty && store.only.isEmpty && !store.flat
    }

    private var list: some View {
        List {
            if needle.isEmpty, store.only.isEmpty {
                Section {
                    if store.at.isEmpty {
                        Wordmark(notes: store.notes.count, books: store.allBooks.count)
                            .listRowInsets(EdgeInsets(top: 4, leading: 16, bottom: 14, trailing: 16))
                    } else {
                        // A folder's own name does not say where it is, and
                        // two folders called 「2026」 look identical at the
                        // top of a list.
                        Crumbs(at: store.at, root: store.rootName) { to in
                            go { store.leave(for: to) }
                        }
                        .listRowInsets(EdgeInsets(top: 2, leading: 16, bottom: 8, trailing: 16))
                    }
                }
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
            }
            // The notebooks first, then the notes in this one. Folders above
            // files is what every file manager since the first one has done,
            // and this is the same gesture: go in, come back.
            // The one-level-at-a-time listing. **Not while the tree is on**,
            // or the folders and the favourites are each drawn twice — which
            // is exactly what happened the first time, and reads as the list
            // having lost its mind rather than as two views agreeing.
            if !store.flat && needle.isEmpty && !treeing2 {
                // The way out, and a place to drop things through it. cian's
                // own panes have had a `..` row since the beginning, and it
                // has always meant both: go up, and put this up there.
                if let up = store.up {
                    Button { go { store.leave(for: up) } } label: {
                        Label("..", systemImage: "arrow.up.left")
                            .foregroundStyle(.tint)
                    }
                    .buttonStyle(.plain)
                    .dropDestination(for: String.self) { paths, _ in
                        drop(paths, into: up.isEmpty ? nil : up)
                    } isTargeted: { over in outside = over }
                    .listRowBackground(outside ? Color.accentColor.opacity(0.15) : nil)
                }
                ForEach(store.books, id: \.path) { b in
                    Button {
                        go { store.into(b.path) }
                    } label: {
                        HStack {
                            Label {
                                Text(b.name)
                            } icon: {
                                Image(systemName: "folder.fill")
                                    .foregroundStyle(store.colors[b.path].flatMap { Color(hex: $0) }
                                        .map { AnyShapeStyle($0) } ?? AnyShapeStyle(.tint))
                            }
                            Spacer()
                            Text("\(b.count)").foregroundStyle(.secondary).monospacedDigit()
                            Image(systemName: "chevron.right")
                                .font(.caption).foregroundStyle(.tertiary)
                        }
                    }
                    .buttonStyle(.plain)
                    // A note dropped on a folder goes into it — pictures and
                    // all, which is `note::move_to`'s business and not this
                    // view's.
                    .dropDestination(for: String.self) { paths, _ in
                        drop(paths, into: b.path)
                    } isTargeted: { over in into = over ? b.path : nil }
                    .listRowBackground(into == b.path ? Color.accentColor.opacity(0.15) : nil)
                    .contextMenu {
                        Button { colouring = b.path } label: {
                            Label("色をつける", systemImage: "paintpalette")
                        }
                        Button {
                            renaming = b.path
                            fresh = b.name
                        } label: {
                            Label("名前を変える", systemImage: "pencil")
                        }
                        Button(role: .destructive) { dropping = b.path } label: {
                            Label("削除する", systemImage: "trash")
                        }
                    }
                }
            }
            // Pinned notes under a heading that says what pinning did.
            // A note that silently jumps to the top is a note that moved for
            // no reason you can see.
            let stuck = treeing2 ? [] : store.pinnedHere(needle)
            if !stuck.isEmpty {
                Section {
                    ForEach(stuck) { row($0) }
                } header: {
                    HStack {
                        Label {
                            Text("お気に入り")
                        } icon: {
                            Image(systemName: "star.fill").foregroundStyle(.orange)
                        }
                        Spacer()
                        NavigationLink("ぜんぶ見る") {
                            Stars(store: store) { note in
                                desk.open(note)
                                showing = true
                            }
                        }
                        .font(.caption)
                        .textCase(nil)
                    }
                }
            }
            // **The tree, unless you are looking for something.** A tree of
            // search results is not a shape anybody reads — when you are
            // narrowing, what you want is the shortest list of answers, and
            // when you are not, what you want is to see where things are.
            if treeing2 {
                Nest(store: store, open: { note in
                    desk.open(note)
                    showing = true
                }, row: { note in AnyView(row(note)) })
            } else {
                // Under headings that follow the ordering — see `bands`.
                ForEach(store.bands(store.matching(needle))) { band in
                    Section(band.name) {
                        ForEach(band.notes) { row($0) }
                    }
                }
            }
        }
        // The whole list is replaced when the folder changes, so it can
        // slide in from the side it came from.
        .id(walked)
        .transition(.asymmetric(
            insertion: .move(edge: deeper ? .trailing : .leading).combined(with: .opacity),
            removal: .move(edge: deeper ? .leading : .trailing).combined(with: .opacity)
        ))
        // **Only from the edges.** A whole-screen horizontal swipe is the
        // rows' own gesture — that is how a note is starred or deleted — so
        // the way out lives where the phone already puts it. Simultaneous
        // rather than exclusive: scrolling must still win, and this only
        // decides anything once the finger is up.
        .simultaneousGesture(
            DragGesture(minimumDistance: 12).onEnded { g in
                // A flick counts for more than it travelled: the finger that
                // is still moving when it leaves the glass meant to go
                // further. Without this the gesture wanted a deliberate drag
                // across a third of the screen, which is not what a swipe is.
                let went = max(abs(g.translation.width), abs(g.predictedEndTranslation.width) * 0.6)
                guard went > abs(g.translation.height) * 1.4, went > 22 else { return }
                if g.startLocation.x < 36, g.translation.width > 0, let up = store.up {
                    go { store.leave(for: up) }
                } else if g.startLocation.x > wide - 36, g.translation.width < 0 {
                    go { _ = store.back() }
                }
            }
        )
        .background {
            GeometryReader { geo in
                Color.clear.onChange(of: geo.size.width, initial: true) { _, w in wide = w }
            }
        }
        // Somebody else may move us — restoring a folder, or coming back
        // from a note. The slide is for moves you made; this keeps the two
        // in step when it was not one.
        .onChange(of: store.at, initial: true) { _, now in if walked != now { walked = now } }
        // Always shown, not hidden until you pull down: with the bar set
        // to `.inline` so the wordmark can have the top of the list, the
        // search field would otherwise be somewhere you have to know about.
        .searchable(text: $needle, placement: .navigationBarDrawer(displayMode: .always),
                    prompt: "タイトル・タグ・本文　（空白で AND、OR で OR）")
        // **The tags, offered where you are already typing.** They used to be
        // a bar of chips above the list, which cost a row of the screen for
        // ever so that they could be pressed occasionally. Suggestions appear
        // when the field is, and are gone the rest of the time.
        .searchSuggestions {
            if needle.trimmingCharacters(in: .whitespaces).isEmpty {
                ForEach(store.tagsHere.prefix(20), id: \.self) { t in
                    Label("#\(t)", systemImage: "tag")
                        .searchCompletion("#\(t)")
                }
            }
        }
        .onChange(of: needle) { _, now in
            store.read(now)
            store.find(now)
        }
        .refreshable { store.reload() }
        // One screen for every open note, with the tabs above them.
        .navigationDestination(isPresented: $showing) { DeskView(desk: desk, store: store) }
        // Straight into the note that was just made, and **in the writing
        // half** — you asked for it in order to write in it.
        //
        // **Inside the stack, not on it.** Attached to the `NavigationStack`
        // itself this does nothing at all: the note was made, the sheet
        // closed, and the list just sat there.

    }
}

/// A folder path, so a sheet can be presented for one.
///
/// `String` is not `Identifiable` and should not be made so app-wide — this
/// is the one place that needs it, and only for the sheet.
extension String: @retroactive Identifiable {
    public var id: String { self }
}
