import SwiftUI
import UniformTypeIdentifiers

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
        // Blank is allowed: cian names an untitled note for the day, which is
        // what you want when you are writing before you know what it is about.
        .alert("新しいノート", isPresented: $naming) {
            TextField("題", text: $title)
            Button("作る") { make() }
            Button("やめる", role: .cancel) { title = "" }
        }
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
                    if !note.excerpt.isEmpty {
                        Text(note.excerpt).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    if !note.tags.isEmpty {
                        Text(note.tags.map { "#\($0)" }.joined(separator: " "))
                            .font(.caption2).foregroundStyle(.tint)
                    }
                }
            }
        }
        .searchable(text: $needle, prompt: "題・タグ・本文")
        .refreshable { store.reload() }
        .navigationDestination(for: Note.self) { NoteView(note: $0, store: store) }
        .overlay { if let why = store.trouble { Text(why).foregroundStyle(.red).padding() } }
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
    @FocusState private var writing: Bool

    private var dirty: Bool { text != saved }

    var body: some View {
        TextEditor(text: $text)
            .font(.body.monospaced())
            .focused($writing)
            .padding(.horizontal, 8)
            .navigationTitle(note.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("保存") { save(force: false) }.disabled(!dirty)
                }
                ToolbarItem(placement: .keyboard) {
                    Button("閉じる") { writing = false }
                }
            }
            .task {
                do {
                    (text, stamp) = try store.open(note)
                    saved = text
                } catch { trouble = error.localizedDescription }
            }
            // Not a yes/no: overwriting is the thing you do having read what
            // the other person wrote, so the reason is on screen and the
            // destructive answer is marked as one.
            .alert("あちらでも書き換えられています", isPresented: .constant(clash != nil)) {
                Button("やめる", role: .cancel) { clash = nil }
                Button("それでも上書き", role: .destructive) { clash = nil; save(force: true) }
            } message: {
                Text(clash ?? "")
            }
            .alert("保存できません", isPresented: .constant(trouble != nil)) {
                Button("閉じる") { trouble = nil }
            } message: {
                Text(trouble ?? "")
            }
    }

    private func save(force: Bool) {
        do {
            switch try store.save(note, text: text, stamp: stamp, force: force) {
            case .ok(let fresh):
                stamp = fresh
                saved = text
                store.reload()
            case .conflict(let why):
                clash = why
            }
        } catch { trouble = error.localizedDescription }
    }
}
