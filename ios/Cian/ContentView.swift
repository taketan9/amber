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
            }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result { store.choose(url) }
        }
        .task { store.restore() }
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

/// One note, read.
///
/// Reading only, for now. Writing is the half that needs the conflict check
/// on the way back — the stamp the engine hands over with the text — and
/// putting it in before it can be tried on a device is how the other person's
/// writing gets quietly overwritten.
struct NoteView: View {
    let note: Note
    let store: NotesStore
    @State private var text = ""
    @State private var trouble: String?

    var body: some View {
        ScrollView {
            Text(trouble ?? text)
                .font(.body.monospaced())
                .frame(maxWidth: .infinity, alignment: .leading)
                .textSelection(.enabled)
                .padding()
        }
        .navigationTitle(note.title)
        .navigationBarTitleDisplayMode(.inline)
        .task {
            do { text = try store.text(of: note) } catch { trouble = error.localizedDescription }
        }
    }
}
