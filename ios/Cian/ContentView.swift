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
    @State private var picking = false
    @State private var naming = false
    @State private var booking = false
    @State private var choosing = false
    @State private var importing = false
    @State private var made: Note?
    @State private var needle = ""

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
            .navigationTitle(store.rootName.isEmpty ? "cian" : store.here)
            .toolbar {
                if let up = store.up {
                    ToolbarItem(placement: .topBarLeading) {
                        Button { store.at = up } label: {
                            Label("上へ", systemImage: "chevron.backward")
                        }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { picking = true } label: { Image(systemName: "externaldrive") }
                        .accessibilityLabel("ノートの置き場所")
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
                        Button { naming = true } label: { Image(systemName: "square.and.pencil") }
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
                            Toggle(isOn: $store.flat) {
                                Label("全部まとめて見る", systemImage: "list.bullet")
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
                  choose: { DispatchQueue.main.async { choosing = true } },
                  bringIn: { DispatchQueue.main.async { importing = true } })
        }
        .fileImporter(isPresented: $choosing, allowedContentTypes: [.folder]) { r in
            if case .success(let url) = r { store.choose(url) }
        }
        .sheet(isPresented: $booking) {
            Booking(inside: store.here) { name in
                do { try store.makeBook(name) }
                catch { store.trouble = error.localizedDescription }
            }
        }
        .task { store.restore() }
        // One `.fileImporter` per view: two on the same one is one importer,
        // and the loser's button does nothing at all.
        .fileImporter(
            isPresented: $importing,
            allowedContentTypes: [UTType(filenameExtension: "md") ?? .plainText, .plainText],
            allowsMultipleSelection: true
        ) { r in
            if case .success(let urls) = r { store.bring(urls) }
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
            Making(make: { title, tags in made = make(title, tags) }, known: store.allTags)
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
                NavigationLink(value: note) {
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 4) {
                            if note.pinned {
                                Image(systemName: "pin.fill")
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
                                        .font(.caption2).foregroundStyle(.secondary)
                                }
                                if !note.tags.isEmpty {
                                    Text(note.tags.map { "#\($0)" }.joined(separator: " "))
                                        .font(.caption2).foregroundStyle(.tint)
                                }
                            }
                        }
                    }
                }
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
                        do { try store.pin(note, !note.pinned) }
                        catch { store.trouble = error.localizedDescription }
                    } label: {
                        Label(note.pinned ? "外す" : "留める",
                              systemImage: note.pinned ? "pin.slash" : "pin")
                    }
                    .tint(.orange)
                }
                .contextMenu {
                    // Moving lives in the long press: it is the one action here
                    // that needs a second choice (which notebook), and a swipe
                    // cannot ask a question. Every notebook, not just the ones
                    // beside this note — filing is often filing *away*.
                    // One note, out to wherever — Files, Drive, Dropbox, mail.
                // The system sheet does all of those, so cian does not have
                // to know any of them by name.
                ShareLink(item: URL(fileURLWithPath: note.path)) {
                    Label("書き出す…", systemImage: "square.and.arrow.up")
                }
                Menu("ノートブックへ移す") {
                        Button("（いちばん上）") { moveTo(note, nil) }
                        ForEach(store.allBooks, id: \.self) { b in
                            Button(b) { moveTo(note, b) }
                        }
                    }
                }
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
            Label("ノートの置き場所", systemImage: "folder.badge.questionmark")
        } description: {
            // Named rather than "choose a folder": the point is that it can be
            // the folder the Mac already has, wherever it is kept.
            Text("マークダウンのノートがあるフォルダを選びます。iCloud Drive・Google Drive・Dropbox のどれでも構いません。")
        } actions: {
            Button("置き場所を見る") { picking = true }.buttonStyle(.borderedProminent)
        }
    }

    private var list: some View {
        List {
            // The notebooks first, then the notes in this one. Folders above
            // files is what every file manager since the first one has done,
            // and this is the same gesture: go in, come back.
            if !store.flat && needle.isEmpty {
                ForEach(store.books, id: \.path) { b in
                    Button {
                        store.at = b.path
                    } label: {
                        HStack {
                            Label(b.name, systemImage: "folder")
                            Spacer()
                            Text("\(b.count)").foregroundStyle(.secondary)
                            Image(systemName: "chevron.right")
                                .font(.caption).foregroundStyle(.tertiary)
                        }
                    }
                    .tint(.primary)
                }
            }
            // Pinned notes under a heading that says what pinning did.
            // A note that silently jumps to the top is a note that moved for
            // no reason you can see.
            let stuck = store.pinnedHere(needle)
            if !stuck.isEmpty {
                Section("上に固定") {
                    ForEach(stuck) { row($0) }
                }
            }
            ForEach(store.matching(needle)) { row($0) }
        }
        .searchable(text: $needle, prompt: "題・タグ・本文")
        .onChange(of: needle) { _, now in store.find(now) }
        .refreshable { store.reload() }
        .navigationDestination(for: Note.self) { NoteView(note: $0, store: store) }
        // Straight into the note that was just made, and **in the writing
        // half** — you asked for it in order to write in it.
        //
        // **Inside the stack, not on it.** Attached to the `NavigationStack`
        // itself this does nothing at all: the note was made, the sheet
        // closed, and the list just sat there.
        .navigationDestination(item: $made) { note in
            NoteView(note: note, store: store, writingFirst: true)
        }
    }
}

/// One note, open for writing.
///
/// The stamp read with the text is kept and handed back on save. **That is
/// the whole of the two-device story**: the same folder is open on a Mac, and
/// without this the later save wins silently and the earlier writing is gone
/// with nothing on screen to say it happened.
struct NoteView: View {
    let note: Note
    let store: NotesStore
    /// Which half it opens on. A note somebody just made opens ready to type
    /// in; one they tapped in the list opens ready to read.
    var writingFirst = false
    @State private var text = ""
    @State private var stamp = ""
    @State private var saved = ""
    @State private var trouble: String?
    @State private var clash: String?
    @State private var picked: PhotosPickerItem?
    @State private var busy = false
    /// Reading or writing. A notes app is read far more often than it is
    /// written, so this opens on the reading side.
    @State private var reading = true
    @State private var blocks: [Block] = []
    @State private var tagging = false
    @State private var tags: [String] = []
    @FocusState private var writing: Bool

    private var dirty: Bool { text != saved }

    var body: some View {
        Group {
            if reading {
                ScrollView { Reading(blocks: blocks, base: folder) }
            } else {
                VStack(spacing: 0) {
                    TextEditor(text: $text)
                        .font(.body.monospaced())
                        .focused($writing)
                        .padding(.horizontal, 8)
                    // The marks that are a nuisance to reach on a phone
                    // keyboard, on a rail above it. Not a formatting toolbar:
                    // the file stays Markdown, and what these do is type the
                    // characters you would have typed.
                    marks
                }
            }
        }
            .navigationTitle(note.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("保存") { save(force: false) }.disabled(!dirty)
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        if !reading { redraw() }
                        reading.toggle()
                    } label: {
                        // The same eye, open or shut. Two different pictures
                        // (an eye and a pencil) read as two different
                        // buttons; one that opens and closes reads as the
                        // one switch it is.
                        Image(systemName: reading ? "eye.slash" : "eye")
                    }
                    .accessibilityLabel(reading ? "編集" : "表示")
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { tags = note.tags; tagging = true } label: {
                        Image(systemName: "tag")
                    }
                    .accessibilityLabel("タグ")
                }
                ToolbarItem(placement: .topBarTrailing) {
                    // The phone's version of pasting a screenshot on the Mac.
                    // The camera is one more tap away in the same sheet, so
                    // there is one button and not two.
                    PhotosPicker(selection: $picked, matching: .images) {
                        Image(systemName: "photo")
                    }
                    .disabled(busy)
                }
                ToolbarItem(placement: .keyboard) {
                    Button("閉じる") { writing = false }
                }
            }
            .task {
                do {
                    (text, stamp) = try store.open(note)
                    saved = text
                    redraw()
                    reading = !writingFirst
                } catch { trouble = error.localizedDescription }
            }
            .onChange(of: picked) { _, item in if let item { take(item) } }
            // The tags go into the text, and the text is saved the ordinary
            // way — so tagging is checked against the file on disk like any
            // other edit rather than being a second door into the note.
            .sheet(isPresented: $tagging, onDismiss: applyTags) {
                Tagging(tags: $tags, known: store.allTags)
            }
            // Not a yes/no: overwriting is the thing you do having read what
            // the other person wrote, so the reason is on screen and the
            // destructive answer is marked as one.
            .alert(
                "あちらでも書き換えられています",
                isPresented: Binding(get: { clash != nil }, set: { if !$0 { clash = nil } })
            ) {
                Button("やめる", role: .cancel) {}
                Button("それでも上書き", role: .destructive) { save(force: true) }
            } message: {
                Text(clash ?? "")
            }
            .alert(
                "できません",
                isPresented: Binding(get: { trouble != nil }, set: { if !$0 { trouble = nil } })
            ) {
                Button("閉じる") {}
            } message: {
                Text(trouble ?? "")
            }
    }

    /// The picture goes to disk first, and only then into the text.
    ///
    /// The other order writes a link to a file that may never arrive, and a
    /// note whose picture is missing looks the same as a note whose picture
    /// was deleted — you cannot tell later which one happened.
    private func take(_ item: PhotosPickerItem) {
        busy = true
        Task {
            defer { busy = false; picked = nil }
            do {
                guard let data = try await item.loadTransferable(type: Data.self) else {
                    trouble = "その写真を読めませんでした"
                    return
                }
                // The bytes decide the extension, not the picker: a screenshot
                // is a PNG and a photo is usually a HEIC, and calling either
                // one the other leaves a file nothing will open.
                let link = try store.attach(data, ext: Self.kind(of: data), to: note)
                if !text.isEmpty && !text.hasSuffix("\n") { text += "\n" }
                text += "![](\(link))\n"
            } catch {
                trouble = error.localizedDescription
            }
        }
    }

    /// What the first bytes say the picture is.
    private static func kind(of data: Data) -> String {
        let b = [UInt8](data.prefix(12))
        if b.count >= 8, b[0] == 0x89, b[1] == 0x50 { return "png" }
        if b.count >= 3, b[0] == 0xFF, b[1] == 0xD8 { return "jpg" }
        if b.count >= 12, b[4] == 0x66, b[5] == 0x74, b[6] == 0x79, b[7] == 0x70 {
            // ...ftyp... — HEIC and its relatives.
            return "heic"
        }
        if b.count >= 4, b[0] == 0x47, b[1] == 0x49, b[2] == 0x46 { return "gif" }
        return "png"
    }

    /// The Markdown a phone keyboard makes you hunt for.
    ///
    /// Line marks (`#`, `- `, `- [ ] `, `> `) go on the **start of the line
    /// the cursor is in**, and toggle: pressing 見出し twice takes it off
    /// again, which is what you want the moment you press it by mistake.
    /// Wrapping marks (`**`, `` ` ``) go around the selection, or leave the
    /// cursor between them when there is none.
    private var marks: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                mark("見出し", "number") { line("# ") }
                mark("箇条書き", "list.bullet") { line("- ") }
                mark("チェック", "checklist") { line("- [ ] ") }
                mark("引用", "text.quote") { line("> ") }
                Divider().frame(height: 20)
                mark("太字", "bold") { wrap("**") }
                mark("斜体", "italic") { wrap("*") }
                mark("コード", "chevron.left.forwardslash.chevron.right") { wrap("`") }
                Divider().frame(height: 20)
                mark("区切り", "minus") { block("\n---\n") }
                mark("表", "tablecells") { block("\n| 　 | 　 |\n| --- | --- |\n| 　 | 　 |\n") }
                mark("コード枠", "curlybraces") { block("\n```\n\n```\n") }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
        }
        .background(.bar)
    }

    private func mark(_ name: String, _ icon: String, _ act: @escaping () -> Void) -> some View {
        Button(action: act) { Image(systemName: icon) }
            .buttonStyle(.bordered)
            .accessibilityLabel(name)
    }

    /// Put `prefix` on the line the cursor is in, or take it off again.
    ///
    /// Without a selection to work from — `TextEditor` does not hand one over
    /// — this works on the **last line**, which is where somebody typing is.
    /// It is the honest limit of a plain `TextEditor`: a proper cursor needs
    /// UIKit, and that is a bigger change than these buttons are worth today.
    private func line(_ prefix: String) {
        var lines = text.components(separatedBy: "\n")
        let at = max(0, lines.count - 1)
        if lines[at].hasPrefix(prefix) {
            lines[at].removeFirst(prefix.count)
        } else {
            lines[at] = prefix + lines[at]
        }
        text = lines.joined(separator: "\n")
    }

    private func wrap(_ mark: String) {
        text += "\(mark)\(mark)"
    }

    private func block(_ s: String) {
        if !text.isEmpty && !text.hasSuffix("\n") { text += "\n" }
        text += s
    }

    /// The folder the note sits in — a picture's link is relative to it.
    private var folder: URL {
        URL(fileURLWithPath: note.path).deletingLastPathComponent()
    }

    private func redraw() {
        do { blocks = try store.blocks(of: text) } catch { trouble = error.localizedDescription }
    }

    private func applyTags() {
        guard tags != note.tags else { return }
        do {
            text = try store.tagged(text, tags)
            redraw()
        } catch { trouble = error.localizedDescription }
    }

    private func save(force: Bool) {
        do {
            switch try store.save(note, text: text, stamp: stamp, force: force) {
            case .ok(let fresh):
                stamp = fresh
                saved = text
                redraw()
                store.reload()
            case .conflict(let why):
                clash = why
            }
        } catch { trouble = error.localizedDescription }
    }
}
