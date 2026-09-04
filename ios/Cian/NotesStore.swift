import Foundation

/// Where the notes are, and what is in there.
///
/// The folder is chosen once with the system picker and remembered as a
/// **security-scoped bookmark**. That is not a detail: it is the whole reason
/// this app needs no Google Drive, Dropbox or iCloud code at all. On iOS all
/// three are Files providers, so one picked folder reaches any of them — and
/// it is the *same folder the Mac has open*, which is the linkage this was
/// built for.
@MainActor
final class NotesStore: ObservableObject {
    @Published var notes: [Note] = []
    @Published var rootName: String = ""
    @Published var trouble: String?

    private var root: URL?
    private static let bookmarkKey = "cian.notes.root"

    /// The app's own folder, which is where notes go when nothing else is
    /// chosen.
    ///
    /// It shows in Files as **cian**, because the app declares
    /// `UIFileSharingEnabled` — so it is a real place you can put a file into
    /// from anywhere else, not a hidden container. Starting here rather than
    /// with a picker matters: the picker offers cloud providers, and a
    /// provider whose app is not installed is *listed but greyed out*, which
    /// reads as "cian cannot see my Drive" rather than as "Drive is not on
    /// this phone".
    private var ownFolder: URL? {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
    }

    /// Whether the notes are in the app's own folder rather than one picked.
    @Published var own = true
    /// The chosen folder's path, for the one screen that should say it.
    var rootPath: String { root?.path ?? "" }
    var rootURL: URL? { root }

    /// Go back to the app's own folder.
    func useOwn() {
        UserDefaults.standard.removeObject(forKey: Self.bookmarkKey)
        if let own = ownFolder { adopt(own, remember: false, scoped: false, named: "cian") }
    }

    /// Copy Markdown files in from somewhere else.
    ///
    /// **Copied, not moved.** Whatever exported them still has them, which is
    /// the answer somebody wants the first time they try this and are not yet
    /// sure cian is where the notes are going to live.
    func bring(_ urls: [URL]) {
        guard let root else { return }
        var brought = 0
        for url in urls {
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            var to = root.appendingPathComponent(url.lastPathComponent)
            // A name already here is not a reason to overwrite somebody's
            // note; it is a reason to keep both.
            var n = 2
            let stem = to.deletingPathExtension().lastPathComponent
            let ext = to.pathExtension
            while FileManager.default.fileExists(atPath: to.path), n <= 99 {
                to = root.appendingPathComponent("\(stem)-\(n).\(ext)")
                n += 1
            }
            do { try FileManager.default.copyItem(at: url, to: to); brought += 1 }
            catch { trouble = error.localizedDescription }
        }
        if brought > 0 { reload() }
    }

    /// The folder from last time, or this app's own.
    func restore() {
        guard let data = UserDefaults.standard.data(forKey: Self.bookmarkKey) else {
            if let ownFolder { adopt(ownFolder, remember: false, scoped: false, named: "cian") }
            return
        }
        var stale = false
        guard let url = try? URL(
            resolvingBookmarkData: data,
            options: [],
            relativeTo: nil,
            bookmarkDataIsStale: &stale
        ) else { return }
        // A folder in a cloud provider can move, be signed out of, or be
        // handed back stale after an update. Saying so beats an empty list
        // that looks like "you have no notes".
        adopt(url, remember: stale, scoped: true)
    }

    func choose(_ url: URL) { adopt(url, remember: true, scoped: true) }

    private func adopt(_ url: URL, remember: Bool, scoped: Bool, named: String? = nil) {
        // **The permission has to be opened and closed** — for a folder
        // somebody picked. Not for the app's own: asking to open a scope it
        // was never given fails, and the failure reads as "cian cannot see my
        // notes" when the truth is that no permission was needed.
        if scoped, !url.startAccessingSecurityScopedResource() {
            trouble = "そのフォルダを開く許可がありません"
            return
        }
        if scoped { root?.stopAccessingSecurityScopedResource() }
        root = url
        // The app's own folder is literally named `Documents`, which is what
        // the filesystem calls it and not what anybody calls it — Files shows
        // it as **cian**, and so should the title above it.
        rootName = named ?? url.lastPathComponent
        own = named != nil
        if remember, let data = try? url.bookmarkData() {
            UserDefaults.standard.set(data, forKey: Self.bookmarkKey)
        }
        reload()
    }

    func reload() {
        guard let root else { return }
        do {
            let answer = try Cian.call("notes", ["path": root.path])
            let rows = answer["notes"] as? [[String: Any]] ?? []
            notes = rows.compactMap(Note.init)
            trouble = nil
        } catch {
            trouble = error.localizedDescription
        }
    }

    /// Where a word was found inside the notes: path → the line it was on.
    ///
    /// Kept apart from `notes` because it answers a different question. The
    /// listing knows a note's title, tags and first hundred characters and
    /// narrows against them the instant you type; this walks the files, which
    /// is slower and finds the sentence you actually remember.
    @Published var hits: [String: String] = [:]

    private var finding: Task<Void, Never>?

    /// Look inside the notes for `needle`, a moment after you stop typing.
    ///
    /// Debounced and cancellable: a search per keystroke would walk the folder
    /// five times for a five-letter word, and the four thrown away would be
    /// the four the phone spent its battery on.
    func find(_ needle: String) {
        finding?.cancel()
        let n = needle.trimmingCharacters(in: .whitespaces)
        guard let root, n.count >= 2 else {
            hits = [:]
            return
        }
        finding = Task {
            try? await Task.sleep(for: .milliseconds(250))
            if Task.isCancelled { return }
            guard let answer = try? Cian.call("find", ["path": root.path, "needle": n]) else { return }
            if Task.isCancelled { return }
            var found: [String: String] = [:]
            for h in answer["hits"] as? [[String: Any]] ?? [] {
                if let p = h["path"] as? String { found[p] = h["text"] as? String ?? "" }
            }
            hits = found
        }
    }

    /// How the list is ordered.
    ///
    /// Newest first by default: a notes list is read from the top, and what
    /// belongs there is what you were last writing. By title is for when you
    /// know the name and not the day — the same two the window offers.
    enum Order: String, CaseIterable, Identifiable {
        case updated, title
        var id: String { rawValue }
        var label: String { self == .updated ? "更新順" : "題順" }
    }

    @Published var order: Order = .updated
    /// Only this notebook, or all of them when nil.
    @Published var book: String?

    /// Every notebook in the folder, with how many notes are in each.
    var books: [(name: String, count: Int)] {
        var n: [String: Int] = [:]
        for note in notes where !note.book.isEmpty { n[note.book, default: 0] += 1 }
        return n.sorted { $0.key < $1.key }.map { ($0.key, $0.value) }
    }

    /// Narrow by what a note is *about*, not by what its file is called.
    func matching(_ needle: String) -> [Note] {
        let n = needle.trimmingCharacters(in: .whitespaces).lowercased()
        var out = notes
        if let book { out = out.filter { $0.book == book } }
        if !n.isEmpty {
            // Either half: what the listing knows, or what was found inside.
            out = out.filter { $0.search.contains(n) || hits[$0.path] != nil }
        }
        switch order {
        case .updated: out.sort { $0.updated > $1.updated }
        // `localizedStandardCompare` and not `<`: 「あ」 before 「い」, and
        // note-2 before note-10, which plain string order gets wrong both ways.
        case .title: out.sort { $0.title.localizedStandardCompare($1.title) == .orderedAscending }
        }
        // Pinned first, and *stably*: within the pinned ones the chosen order
        // still holds. Sorting by `pinned` alone would shuffle them.
        return out.filter(\.pinned) + out.filter { !$0.pinned }
    }

    /// The text of a note, and the stamp that says which version it was.
    ///
    /// The two travel together on purpose. Saving has to hand the stamp back,
    /// and a caller that has to remember to ask for it separately is a caller
    /// that will one day forget — and the forgetting is silent until the day
    /// two devices are open on the same note.
    func open(_ note: Note) throws -> (String, String) {
        let answer = try Cian.call("read", ["path": note.path])
        return (answer["text"] as? String ?? "", answer["stamp"] as? String ?? "")
    }

    /// The note with a different set of tags on it.
    ///
    /// Comes back as text for the caller to save, so this goes through the
    /// same conflict check as any other edit.
    func tagged(_ text: String, _ tags: [String]) throws -> String {
        let answer = try Cian.call("settags", ["text": text, "tags": tags])
        return answer["text"] as? String ?? text
    }

    /// Every tag in the folder, commonest first — what to offer rather than
    /// make somebody type again.
    var allTags: [String] {
        var n: [String: Int] = [:]
        for note in notes { for t in note.tags { n[t, default: 0] += 1 } }
        return n.sorted { $0.value > $1.value || ($0.value == $1.value && $0.key < $1.key) }
            .map(\.key)
    }

    /// The note, split into things to draw.
    ///
    /// Given the text rather than the path, so what is on screen is what the
    /// preview shows — including edits not saved yet. A preview of the file on
    /// disk would show yesterday's note while you are looking at today's.
    func blocks(of text: String) throws -> [Block] {
        let answer = try Cian.call("blocks", ["text": text])
        return (answer["blocks"] as? [[String: Any]] ?? []).map(Block.init)
    }

    enum Saved {
        case ok(stamp: String)
        /// Somebody else wrote it first. `why` is cian's own account of the
        /// difference; nothing has been written.
        case conflict(why: String)
    }

    func save(_ note: Note, text: String, stamp: String, force: Bool = false) throws -> Saved {
        var params: [String: Any] = ["path": note.path, "text": text, "force": force]
        if !stamp.isEmpty { params["stamp"] = stamp }
        let answer = try Cian.call("write", params)
        if answer["conflict"] as? Bool == true {
            return .conflict(why: answer["why"] as? String ?? "開いたあとで書き換えられています")
        }
        return .ok(stamp: answer["stamp"] as? String ?? "")
    }

    /// Put a picture beside a note and hand back the Markdown link for it.
    ///
    /// Base64 because that is what fits down a C string. The bytes go over
    /// once; nothing about where the file lands is decided here.
    func attach(_ data: Data, ext: String, to note: Note) throws -> String {
        let answer = try Cian.call("image", [
            "note": note.path,
            "b64": data.base64EncodedString(),
            "ext": ext,
        ])
        guard let link = answer["link"] as? String else {
            throw Cian.Failure.engine("画像を置けませんでした")
        }
        return link
    }

    /// Pin or unpin, by rewriting the note and saving it the ordinary way.
    ///
    /// Reads the file rather than taking text from a caller: this is done from
    /// the list, where nothing has the note open, and inventing a stamp for a
    /// file nobody is looking at would be pretending to a check that has not
    /// happened.
    func pin(_ note: Note, _ on: Bool) throws {
        let read = try Cian.call("read", ["path": note.path])
        let text = read["text"] as? String ?? ""
        let out = try Cian.call("setfield", [
            "text": text, "key": "pinned", "value": on ? "true" : NSNull(),
        ])
        _ = try Cian.call("write", [
            "path": note.path,
            "text": out["text"] as? String ?? text,
            "stamp": read["stamp"] as? String ?? "",
        ])
        reload()
    }

    /// Move a note into another notebook — `nil` means the top of the folder.
    func move(_ note: Note, to book: String?) throws {
        guard let root else { return }
        let dir = book.map { root.appendingPathComponent($0) } ?? root
        _ = try Cian.call("move", ["path": note.path, "dir": dir.path])
        reload()
    }

    /// Remove a note. There is no trash on a phone, so this cannot be undone
    /// — the caller asks first.
    func remove(_ note: Note) throws {
        _ = try Cian.call("delete", ["path": note.path])
        reload()
    }

    /// A new note in the chosen folder, named and shaped by cian.
    func make(titled title: String, tags: [String] = []) throws -> Note? {
        guard let root else { return nil }
        let made = try Cian.call("new", ["dir": root.path, "title": title])
        guard let path = made["path"] as? String else { return nil }
        // The tags go on by rewriting the note that was just written, rather
        // than by teaching `new` about tags: one place decides what a note's
        // front matter looks like, and it is already `note::set_tags`.
        if !tags.isEmpty {
            let read = try Cian.call("read", ["path": path])
            let text = read["text"] as? String ?? ""
            let out = try Cian.call("settags", ["text": text, "tags": tags])
            _ = try Cian.call("write", [
                "path": path,
                "text": out["text"] as? String ?? text,
                "stamp": read["stamp"] as? String ?? "",
            ])
        }
        reload()
        return notes.first { $0.path == path }
    }
}
