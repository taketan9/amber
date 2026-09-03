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
    private var own: URL? {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
    }

    /// The folder from last time, or this app's own.
    func restore() {
        guard let data = UserDefaults.standard.data(forKey: Self.bookmarkKey) else {
            if let own { adopt(own, remember: false, scoped: false, named: "cian") }
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
            notes = rows.compactMap(Note.init).sorted { $0.updated > $1.updated }
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

    /// Narrow by what a note is *about*, not by what its file is called.
    func matching(_ needle: String) -> [Note] {
        let n = needle.trimmingCharacters(in: .whitespaces).lowercased()
        guard !n.isEmpty else { return notes }
        // Either half: what the listing knows, or what was found inside.
        return notes.filter { $0.search.contains(n) || hits[$0.path] != nil }
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

    /// Remove a note. There is no trash on a phone, so this cannot be undone
    /// — the caller asks first.
    func remove(_ note: Note) throws {
        _ = try Cian.call("delete", ["path": note.path])
        reload()
    }

    /// A new note in the chosen folder, named and shaped by cian.
    func make(titled title: String) throws -> Note? {
        guard let root else { return nil }
        let made = try Cian.call("new", ["dir": root.path, "title": title])
        reload()
        guard let path = made["path"] as? String else { return nil }
        return notes.first { $0.path == path }
    }
}
