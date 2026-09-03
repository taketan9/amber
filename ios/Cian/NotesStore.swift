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

    func text(of note: Note) throws -> String {
        let answer = try Cian.call("read", ["path": note.path])
        return answer["text"] as? String ?? ""
    }
}
