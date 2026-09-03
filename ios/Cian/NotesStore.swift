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

    /// The folder from last time, if it is still reachable.
    func restore() {
        guard let data = UserDefaults.standard.data(forKey: Self.bookmarkKey) else { return }
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
        adopt(url, remember: stale)
    }

    func choose(_ url: URL) { adopt(url, remember: true) }

    private func adopt(_ url: URL, remember: Bool) {
        // **The permission has to be opened and closed.** Without this the
        // paths are readable in the picker and refused everywhere else, which
        // reads as "cian cannot see my notes" rather than as a permission.
        guard url.startAccessingSecurityScopedResource() else {
            trouble = "そのフォルダを開く許可がありません"
            return
        }
        root?.stopAccessingSecurityScopedResource()
        root = url
        rootName = url.lastPathComponent
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

    /// Narrow by what a note is *about*, not by what its file is called.
    func matching(_ needle: String) -> [Note] {
        let n = needle.trimmingCharacters(in: .whitespaces).lowercased()
        guard !n.isEmpty else { return notes }
        return notes.filter { $0.search.contains(n) }
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

    /// A new note in the chosen folder, named and shaped by cian.
    func make(titled title: String) throws -> Note? {
        guard let root else { return nil }
        let made = try Cian.call("new", ["dir": root.path, "title": title])
        reload()
        guard let path = made["path"] as? String else { return nil }
        return notes.first { $0.path == path }
    }
}
