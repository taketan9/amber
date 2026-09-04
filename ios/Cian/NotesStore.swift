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
    /// The path as a trail of names — 「この iPhone › cian › 仕事」.
    ///
    /// A phone hides paths, which is usually kind and here is not: the same
    /// folder name can exist in three different clouds, and "cian" on its own
    /// answers *what is it called* when the question is *where is it*.
    var trail: [String] {
        guard let root else { return [] }
        if own { return ["この iPhone", "cian"] }
        // The tail of the path, which is the part that means anything: the
        // front of it is the provider's own bookkeeping.
        let parts = root.pathComponents.filter { $0 != "/" }
        let keep = parts.suffix(4)
        return (parts.count > keep.count ? ["…"] : []) + keep
    }

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
            allBooks = answer["books"] as? [String] ?? []
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

    /// Which notebook is open, as a path relative to the root. `""` is the
    /// top. This is *where you are*, not a filter — the two look the same on
    /// screen for one level and stop looking the same the moment there is a
    /// notebook inside a notebook.
    @Published var at = ""

    /// Show everything at once, folders ignored.
    ///
    /// Both ways of keeping notes are real. Some people put four hundred in
    /// one pile and find them by searching; some want them filed. Neither is
    /// a mistake to be corrected by an app, so this is a switch.
    @Published var flat = false

    /// Every notebook there is, as paths relative to the root — including the
    /// empty ones, which is why this comes from the engine's walk of the
    /// directories rather than from the notes.
    @Published var allBooks: [String] = []

    /// The notebooks directly inside the one that is open, with how many
    /// notes are anywhere underneath each.
    var books: [(name: String, path: String, count: Int)] {
        let prefix = at.isEmpty ? "" : at + "/"
        var seen: [String] = []
        for b in allBooks where b.hasPrefix(prefix) {
            let rest = String(b.dropFirst(prefix.count))
            guard !rest.isEmpty else { continue }
            let head = rest.split(separator: "/").first.map(String.init) ?? rest
            let full = prefix + head
            if !seen.contains(full) { seen.append(full) }
        }
        return seen.map { full in
            let n = notes.filter { $0.book == full || $0.book.hasPrefix(full + "/") }.count
            return (String(full.dropFirst(prefix.count)), full, n)
        }
    }

    /// The name to put above the list.
    var here: String {
        at.isEmpty ? rootName : (at.split(separator: "/").last.map(String.init) ?? at)
    }

    /// One level up, or nil at the top.
    var up: String? {
        guard !at.isEmpty else { return nil }
        let parts = at.split(separator: "/").dropLast()
        return parts.joined(separator: "/")
    }

    /// Narrow by what a note is *about*, not by what its file is called.
    func matching(_ needle: String) -> [Note] {
        let n = needle.trimmingCharacters(in: .whitespaces).lowercased()
        var out = notes
        // Searching looks everywhere, whatever notebook is open: the note you
        // are looking for is the one you have forgotten where you put.
        if !flat && n.isEmpty { out = out.filter { $0.book == at } }
        if !n.isEmpty {
            // Either half: what the listing knows, or what was found inside.
            out = out.filter { $0.search.contains(n) || hits[$0.path] != nil }
        }
        // The pinned ones are drawn in their own section above this, so they
        // come out here rather than being sorted to the front: a note in two
        // places at once is a note somebody deletes twice.
        let stuck = Set(pinnedHere(needle).map(\.path))
        return sorted(out.filter { !stuck.contains($0.path) })
    }

    /// The pinned notes to show above the list.
    ///
    /// At the top of the folder, **every** pinned note wherever it lives —
    /// that is what pinning is for: the note you keep coming back to, within
    /// reach without going to find it. Inside a notebook, only that
    /// notebook's, because there you are looking at one place on purpose.
    func pinnedHere(_ needle: String) -> [Note] {
        guard needle.trimmingCharacters(in: .whitespaces).isEmpty, !flat else { return [] }
        let all = notes.filter(\.pinned)
        return sorted(at.isEmpty ? all : all.filter { $0.book == at })
    }

    private func sorted(_ list: [Note]) -> [Note] {
        var out = list
        switch order {
        case .updated: out.sort { $0.updated > $1.updated }
        // `localizedStandardCompare` and not `<`: 「あ」 before 「い」, and
        // note-2 before note-10, which plain string order gets wrong both ways.
        case .title: out.sort { $0.title.localizedStandardCompare($1.title) == .orderedAscending }
        }
        return out
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

    /// What a note asked to be reminded about.
    func reminder(of text: String) throws -> Reminder {
        Reminder(try Cian.call("remind", ["text": text]))
    }

    /// Set or clear one front-matter field, coming back as text to save.
    func field(_ text: String, _ key: String, _ value: String?) throws -> String {
        let out = try Cian.call("setfield", [
            "text": text, "key": key, "value": value ?? NSNull(),
        ])
        return out["text"] as? String ?? text
    }

    /// Make the copies every routine owes, and write down that they were made.
    ///
    /// Done on opening the app, because that is the only moment a phone gives
    /// an app to do it — see `Bell`. Quiet when there is nothing owed, which
    /// is almost always.
    func catchUp() {
        guard root != nil else { return }
        var made = 0
        for note in notes {
            guard let r = try? Cian.call("remind", ["path": note.path]),
                  let due = r["due"] as? [String], !due.isEmpty else { continue }
            for day in due {
                if (try? Cian.call("carryout", ["path": note.path, "on": day])) != nil { made += 1 }
            }
        }
        if made > 0 { reload() }
    }

    /// A zip of some or all of the notes, for handing to something else.
    func backup(scope: String, what: String) throws -> URL {
        guard let root else { throw Cian.Failure.engine("置き場所がありません") }
        let r = try Cian.call("backup", [
            "path": root.path, "scope": scope, "what": what,
        ])
        guard let at = r["path"] as? String else { throw Cian.Failure.engine("作れませんでした") }
        return URL(fileURLWithPath: at)
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

    /// Make a notebook inside the one that is open.
    func makeBook(_ name: String) throws {
        guard let root else { return }
        let clean = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty else { return }
        let dir = root.appendingPathComponent(at).appendingPathComponent(clean)
        _ = try Cian.call("mkbook", ["dir": dir.path])
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
        // In the notebook that is open, not always at the top — otherwise
        // filing is something you do afterwards, every time.
        let dir = root.appendingPathComponent(at)
        let made = try Cian.call("new", ["dir": dir.path, "title": title])
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
