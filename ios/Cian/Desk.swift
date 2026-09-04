import SwiftUI

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

    var body: some View {
        VStack(spacing: 0) {
            if desk.tabs.count > 1 { strip }
            // Swipe between the open notes. `.never` for the dots: the strip
            // above already says how many there are and which one this is,
            // and two answers to one question is one too many.
            TabView(selection: $desk.showing) {
                ForEach(desk.tabs) { tab in
                    if let bound = desk.binding(tab.id) {
                        NoteView(tab: bound, store: store, current: tab.id == desk.showing)
                            .tag(tab.id)
                    }
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .never))
        }
        .navigationTitle(desk.current?.note.title ?? "")
        .navigationBarTitleDisplayMode(.inline)
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
