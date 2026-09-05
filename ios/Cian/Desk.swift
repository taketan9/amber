import SwiftUI
import PhotosUI

/// The notes that are open at once.
///
/// **The text lives here, not in the view.** A `TabView` builds and throws
/// away its pages as you swipe; anything a page held would go with it, and
/// what a page holds is what you have typed and not saved. So a tab is a
/// piece of state on the desk, and the editor is a window onto it.
@MainActor
final class Desk: ObservableObject {
    struct Tab: Identifiable, Equatable {
        let note: Note
        var text = ""
        /// What was on disk when it was opened or last saved.
        var stamp = ""
        /// The text as saved, to tell "changed" from "opened".
        var saved = ""
        var reading = true
        var blocks: [Block] = []
        var loaded = false
        /// Where the cursor is, in UTF-16 units. On the desk with the text
        /// because it belongs to the note, not to the moment on screen: swipe
        /// away and back and you are where you left off.
        var pick = NSRange(location: 0, length: 0)

        var id: String { note.path }
        var dirty: Bool { loaded && text != saved }

        static func == (a: Tab, b: Tab) -> Bool { a.id == b.id && a.text == b.text && a.reading == b.reading }
    }

    @Published var tabs: [Tab] = []
    /// Which tab is showing, by path — **not by index**. Closing a tab shifts
    /// every index after it, and a selection that is an index quietly starts
    /// pointing at the note next door.
    @Published var showing: String = ""

    var current: Tab? { tabs.first { $0.id == showing } }

    /// Open a note, or come back to it if it is already open.
    func open(_ note: Note, writing: Bool = false) {
        if let at = tabs.firstIndex(where: { $0.id == note.path }) {
            if writing { tabs[at].reading = false }
        } else {
            tabs.append(Tab(note: note, reading: !writing))
        }
        showing = note.path
    }

    /// Close one tab, and choose what to show next.
    ///
    /// The neighbour on the left, because that is where you came from — a
    /// close that jumps to the far end of the row loses your place.
    func close(_ id: String) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        tabs.remove(at: at)
        if showing == id {
            let next = min(max(0, at - 1), tabs.count - 1)
            showing = tabs.indices.contains(next) ? tabs[next].id : ""
        }
    }

    /// Read a note in, once. Coming back to a tab must not throw away what
    /// is in it — that is the whole reason the text lives here.
    func load(_ id: String, _ store: NotesStore) throws {
        guard let at = tabs.firstIndex(where: { $0.id == id }), !tabs[at].loaded else { return }
        let (text, stamp) = try store.open(tabs[at].note)
        tabs[at].text = text
        tabs[at].saved = text
        tabs[at].stamp = stamp
        tabs[at].loaded = true
        tabs[at].blocks = (try? store.blocks(of: text)) ?? []
    }

    func redraw(_ id: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        tabs[at].blocks = (try? store.blocks(of: tabs[at].text)) ?? []
    }

    /// Save, and say what happened. `nil` is "nothing to do".
    ///
    /// Returns the conflict's words rather than throwing them: a clash is not
    /// a failure, it is the other device having got there first, and the
    /// caller has to ask a question rather than report an error.
    @discardableResult
    func save(_ id: String, _ store: NotesStore, force: Bool = false) throws -> String? {
        guard let at = tabs.firstIndex(where: { $0.id == id }), tabs[at].loaded else { return nil }
        guard force || tabs[at].dirty else { return nil }
        switch try store.save(tabs[at].note, text: tabs[at].text, stamp: tabs[at].stamp, force: force) {
        case .ok(let fresh):
            guard let now = tabs.firstIndex(where: { $0.id == id }) else { return nil }
            tabs[now].stamp = fresh
            tabs[now].saved = tabs[now].text
            redraw(id, store)
            store.reload()
            return nil
        case .conflict(let why):
            return why
        }
    }

    func binding(_ id: String) -> Binding<Tab>? {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return nil }
        return Binding(
            get: { [weak self] in self?.tabs.indices.contains(at) == true ? self!.tabs[at] : Tab(note: Note(["path": id])!) },
            set: { [weak self] new in
                guard let self, let now = self.tabs.firstIndex(where: { $0.id == id }) else { return }
                self.tabs[now] = new
            }
        )
    }
}

/// The open notes, with a strip of tabs above them.
struct DeskView: View {
    @ObservedObject var desk: Desk
    let store: NotesStore
    @StateObject private var pen = Pen()
    @Environment(\.scenePhase) private var phase
    @State private var trouble: String?
    @State private var clash: String?
    @State private var tagging = false
    @State private var ringing = false
    @State private var picking = false
    @State private var picked: PhotosPickerItem?
    @State private var busy = false
    @State private var tags: [String] = []
    @State private var writing = false
    @State private var saving: Task<Void, Never>?
    @State private var tabling = false

    private var here: Desk.Tab? { desk.current }

    private var pages: some View {
        VStack(spacing: 0) {
            if desk.tabs.count > 1 { strip }
            // Swipe between the open notes. `.never` for the dots: the strip
            // above already says how many there are and which one this is,
            // and two answers to one question is one too many.
            TabView(selection: $desk.showing) {
                ForEach(desk.tabs) { tab in
                    if let bound = desk.binding(tab.id) {
                        NoteView(tab: bound, store: store, pen: pen, writing: $writing,
                                 table: { tabling = true })
                            .tag(tab.id)
                    }
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .never))
        }
    }

    var body: some View {
        wired
            .sheet(isPresented: $tabling) {
                Tabling { body in
                    guard let id = here?.id,
                          let at = desk.tabs.firstIndex(where: { $0.id == id }) else { return }
                    var text = desk.tabs[at].text
                    var pick = desk.tabs[at].pick
                    pen.apply(Marks.block(text, pick, body, caret: 2), to: &text, pick: &pick)
                    desk.tabs[at].text = text
                    desk.tabs[at].pick = pick
                }
            }
            .sheet(isPresented: $tagging, onDismiss: applyTags) {
                Tagging(tags: $tags, known: store.allTags)
            }
            .sheet(isPresented: $ringing) {
                if let bound = desk.binding(desk.showing), let note = here?.note {
                    Ringing(note: note, text: bound.text, store: store)
                }
            }
            .alert(
                "あちらでも書き換えられています",
                isPresented: Binding(get: { clash != nil }, set: { if !$0 { clash = nil } })
            ) {
                Button("やめる", role: .cancel) {}
                Button("それでも上書き", role: .destructive) { now(force: true) }
            } message: { Text(clash ?? "") }
            .alert(
                "できません",
                isPresented: Binding(get: { trouble != nil }, set: { if !$0 { trouble = nil } })
            ) { Button("閉じる") {} } message: { Text(trouble ?? "") }
    }

    /// The chrome, and the things that keep the note written down.
    ///
    /// Split off from `body` only because one long chain of modifiers is
    /// more than the Swift type-checker will sit through.
    private var wired: some View {
        pages
            .navigationTitle(here?.note.title ?? "")
            .navigationBarTitleDisplayMode(.inline)
            // **The chrome belongs to the desk, not to the page.** A
            // `TabView` keeps the neighbouring page alive and rebuilds pages
            // as things change, and a toolbar built by a page is rebuilt
            // with it — which SwiftUI shows as a 「⋯」 appearing for an
            // instant and going away. Up here there is one toolbar, and
            // nothing the page does rebuilds it.
            .toolbar { chrome }
            .photosPicker(isPresented: $picking, selection: $picked, matching: .images)
            .onChange(of: picked) { _, item in if let item { take(item) } }
            .onChange(of: desk.showing) { _, _ in load() }
            .task { load() }
            // Written down as you write, so there is nothing to remember to
            // do. Debounced: a save per keystroke is a file rewritten forty
            // times a sentence, and on a synced folder that is forty things
            // for the other device to notice.
            .onChange(of: here?.text ?? "") { _, _ in later() }
            // Leaving is the other moment worth saving at — the phone can
            // stop the app without asking, so a save that only happened on a
            // timer would lose the last thing typed.
            .onChange(of: phase) { _, going in if going != .active { now() } }
            .onDisappear { saving?.cancel(); now() }
    }

    @ToolbarContentBuilder
    private var chrome: some ToolbarContent {
        ToolbarItem(id: "state", placement: .topBarTrailing) {
            // Not a button any more: it saves itself. This says which of the
            // two states it is in, because a note that says nothing about
            // whether it is written down is a note you cannot walk away from.
            Group {
                if here?.dirty == true {
                    Label("保存中", systemImage: "circle.fill")
                        .font(.caption2)
                        .foregroundStyle(.orange)
                } else {
                    Label("保存済み", systemImage: "checkmark")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            .labelStyle(.iconOnly)
            .accessibilityLabel(here?.dirty == true ? "保存中" : "保存済み")
        }
        ToolbarItem(id: "read", placement: .topBarTrailing) {
            Button {
                guard let id = here?.id else { return }
                if here?.reading == false { desk.redraw(id, store) }
                if let at = desk.tabs.firstIndex(where: { $0.id == id }) {
                    desk.tabs[at].reading.toggle()
                }
            } label: {
                Image(systemName: here?.reading == true ? "eye.slash" : "eye")
            }
            .accessibilityLabel(here?.reading == true ? "編集" : "表示")
        }
        ToolbarItem(id: "more", placement: .topBarTrailing) {
            Menu {
                Button { tags = here?.note.tags ?? []; tagging = true } label: {
                    Label("タグ", systemImage: "tag")
                }
                Button { ringing = true } label: {
                    Label("通知", systemImage: reminded ? "bell.fill" : "bell")
                }
                Button { picking = true } label: { Label("写真", systemImage: "photo") }
                    .disabled(busy)
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .accessibilityLabel("その他")
        }
    }

    private var reminded: Bool {
        guard let text = here?.text else { return false }
        return (try? store.reminder(of: text)).map { !$0.once.isEmpty || $0.repeats } ?? false
    }

    private func load() {
        guard !desk.showing.isEmpty else { return }
        do { try desk.load(desk.showing, store) } catch { trouble = error.localizedDescription }
    }

    /// Save in a moment, unless more typing arrives first.
    private func later() {
        saving?.cancel()
        let id = desk.showing
        saving = Task {
            try? await Task.sleep(for: .milliseconds(900))
            if Task.isCancelled { return }
            write(id)
        }
    }

    /// Save right now.
    private func now(force: Bool = false) {
        saving?.cancel()
        write(desk.showing, force: force)
    }

    private func write(_ id: String, force: Bool = false) {
        guard !id.isEmpty else { return }
        do {
            if let why = try desk.save(id, store, force: force) { clash = why }
        } catch { trouble = error.localizedDescription }
    }

    private func applyTags() {
        guard let id = here?.id, let note = here?.note, tags != note.tags,
              let at = desk.tabs.firstIndex(where: { $0.id == id }) else { return }
        do {
            desk.tabs[at].text = try store.tagged(desk.tabs[at].text, tags)
            desk.redraw(id, store)
        } catch { trouble = error.localizedDescription }
    }

    /// The picture goes to disk first, and only then into the text: the other
    /// order writes a link to a file that may never arrive.
    private func take(_ item: PhotosPickerItem) {
        guard let id = here?.id, let note = here?.note else { return }
        busy = true
        Task {
            defer { busy = false; picked = nil }
            do {
                guard let data = try await item.loadTransferable(type: Data.self) else {
                    trouble = "その写真を読めませんでした"
                    return
                }
                let link = try store.attach(data, ext: Self.kind(of: data), to: note)
                guard let at = desk.tabs.firstIndex(where: { $0.id == id }) else { return }
                // Where you were, not at the end: a picture belongs in the
                // paragraph you were writing when you reached for it.
                var text = desk.tabs[at].text
                var pick = desk.tabs[at].pick
                pen.apply(Marks.block(text, pick, "![](\(link))\n"), to: &text, pick: &pick)
                desk.tabs[at].text = text
                desk.tabs[at].pick = pick
            } catch {
                trouble = error.localizedDescription
            }
        }
    }

    /// What the first bytes say the picture is — a screenshot is a PNG and a
    /// photo is usually a HEIC, and calling either one the other leaves a
    /// file nothing will open.
    private static func kind(of data: Data) -> String {
        let b = [UInt8](data.prefix(12))
        if b.count >= 8, b[0] == 0x89, b[1] == 0x50 { return "png" }
        if b.count >= 3, b[0] == 0xFF, b[1] == 0xD8 { return "jpg" }
        if b.count >= 12, b[4] == 0x66, b[5] == 0x74, b[6] == 0x79, b[7] == 0x70 { return "heic" }
        if b.count >= 4, b[0] == 0x47, b[1] == 0x49, b[2] == 0x46 { return "gif" }
        return "png"
    }

    private var strip: some View {
        ScrollViewReader { to in
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(desk.tabs) { tab in
                        chip(tab)
                            .id(tab.id)
                    }
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
            }
            .background(.bar)
            // Swiping to a tab that is off the end of the strip should bring
            // the strip with it, or the two disagree about where you are.
            .onChange(of: desk.showing) { _, now in
                withAnimation { to.scrollTo(now, anchor: .center) }
            }
        }
    }

    private func chip(_ tab: Desk.Tab) -> some View {
        let on = tab.id == desk.showing
        return HStack(spacing: 4) {
            if tab.dirty {
                // Unsaved, said in the one place you are looking when you
                // decide to close something.
                Circle().frame(width: 6, height: 6).foregroundStyle(.orange)
            }
            Text(tab.note.title).lineLimit(1).font(.subheadline)
            Button {
                desk.close(tab.id)
            } label: {
                Image(systemName: "xmark").font(.caption2)
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(on ? Color.accentColor.opacity(0.18) : Color.secondary.opacity(0.12),
                    in: Capsule())
        .foregroundStyle(on ? Color.accentColor : Color.primary)
        .onTapGesture { desk.showing = tab.id }
    }
}
