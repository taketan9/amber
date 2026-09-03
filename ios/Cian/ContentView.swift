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
    @State private var title = ""
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
            .navigationTitle(store.rootName.isEmpty ? "cian" : store.rootName)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button { picking = true } label: { Image(systemName: "folder") }
                        .accessibilityLabel("ノートの置き場所")
                }
                if !store.rootName.isEmpty {
                    ToolbarItem(placement: .topBarTrailing) {
                        Button { naming = true } label: { Image(systemName: "square.and.pencil") }
                            .accessibilityLabel("新しいノート")
                    }
                }
            }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result { store.choose(url) }
        }
        .task { store.restore() }
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
        // Blank is allowed: cian names an untitled note for the day, which is
        // what you want when you are writing before you know what it is about.
        .alert("新しいノート", isPresented: $naming) {
            TextField("題", text: $title)
            Button("作る") { make() }
            Button("やめる", role: .cancel) { title = "" }
        }
    }

    /// `try?` here would be the whole bug: a delete that fails silently looks
    /// exactly like a delete that was never asked for, and the row stays.
    /// `try?` here would be the whole bug: a delete that fails silently looks
    /// exactly like a delete that was never asked for, and the row stays.
    private func remove(_ note: Note) {
        do { try store.remove(note) } catch { store.trouble = error.localizedDescription }
    }

    private func make() {
        do { made = try store.make(titled: title) } catch { store.trouble = error.localizedDescription }
        title = ""
    }

    private var empty: some View {
        ContentUnavailableView {
            Label("ノートの置き場所", systemImage: "folder.badge.questionmark")
        } description: {
            // Named rather than "choose a folder": the point is that it can be
            // the folder the Mac already has, wherever it is kept.
            Text("マークダウンのノートがあるフォルダを選びます。iCloud Drive・Google Drive・Dropbox のどれでも構いません。")
        } actions: {
            Button("選ぶ") { picking = true }.buttonStyle(.borderedProminent)
        }
    }

    private var list: some View {
        List(store.matching(needle)) { note in
            NavigationLink(value: note) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(note.title).font(.body.weight(.semibold)).lineLimit(1)
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
        }
        .searchable(text: $needle, prompt: "題・タグ・本文")
        .onChange(of: needle) { _, now in store.find(now) }
        .refreshable { store.reload() }
        .navigationDestination(for: Note.self) { NoteView(note: $0, store: store) }
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
    @FocusState private var writing: Bool

    private var dirty: Bool { text != saved }

    var body: some View {
        Group {
            if reading {
                ScrollView { Reading(blocks: blocks, base: folder) }
            } else {
                TextEditor(text: $text)
                    .font(.body.monospaced())
                    .focused($writing)
                    .padding(.horizontal, 8)
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
                        Image(systemName: reading ? "square.and.pencil" : "eye")
                    }
                    .accessibilityLabel(reading ? "編集" : "表示")
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
                } catch { trouble = error.localizedDescription }
            }
            .onChange(of: picked) { _, item in if let item { take(item) } }
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

    /// The folder the note sits in — a picture's link is relative to it.
    private var folder: URL {
        URL(fileURLWithPath: note.path).deletingLastPathComponent()
    }

    private func redraw() {
        do { blocks = try store.blocks(of: text) } catch { trouble = error.localizedDescription }
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
