import Foundation
import SwiftUI

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
    private static let seededKey = "amber.notes.seeded"

    /// The app's own folder, which is where notes go when nothing else is
    /// chosen.
    ///
    /// It shows in Files as **cian**, because the app declares
    /// `UIFileSharingEnabled` — so it is a real place you can put a file into
    /// from anywhere else, not a hidden container. Starting here rather than
    /// with a picker matters: the picker offers cloud providers, and a
    /// provider whose app is not installed is *listed but greyed out*, which
    /// reads as "amber cannot see my Drive" rather than as "Drive is not on
    /// this phone".
    private var ownFolder: URL? {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
    }

    /// Whether the notes are in the app's own folder rather than one picked.
    @Published var own = true
    /// The path as a trail of names — 「この iPhone › amber › 仕事」.
    ///
    /// A phone hides paths, which is usually kind and here is not: the same
    /// folder name can exist in three different clouds, and "amber" on its own
    /// answers *what is it called* when the question is *where is it*.
    var trail: [String] {
        guard let root else { return [] }
        if own { return ["この iPhone", "amber"] }
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
        if let own = ownFolder { adopt(own, remember: false, scoped: false, named: "amber") }
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
        seedWelcome()
        guard let data = UserDefaults.standard.data(forKey: Self.bookmarkKey) else {
            if let ownFolder { adopt(ownFolder, remember: false, scoped: false, named: "amber") }
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

    /// **Don't show a first-time reader an empty list.**
    ///
    /// With nothing in it there is nowhere to learn what the app can do —
    /// Markdown, folders, tags, the diagrams. Three notes that can be read,
    /// pressed and rewritten teach it faster than a screen of instructions,
    /// and they are the same three the Mac window puts down
    /// (`packaging/welcome`), so both ambers open on the same page.
    ///
    /// Once only. **Deleted means deleted** — nothing that somebody threw
    /// away should grow back on the next launch, so the flag records *that we
    /// put them down*, not whether the folder is empty now.
    private func seedWelcome() {
        let defaults = UserDefaults.standard
        guard !defaults.bool(forKey: Self.seededKey) else { return }
        // **Somebody who already picked a folder is not a first-time reader.**
        // Their notes live in that folder; putting samples in the app's own
        // one would drop three files into a place they are not looking at,
        // and they would find them weeks later without knowing where from.
        guard defaults.data(forKey: Self.bookmarkKey) == nil else {
            defaults.set(true, forKey: Self.seededKey)
            return
        }
        guard let from = Bundle.main.resourceURL?.appendingPathComponent("welcome"),
              FileManager.default.fileExists(atPath: from.path),
              let to = ownFolder else { return }
        // Notes already here: somebody's own folder, or a restored backup.
        // Say we seeded anyway, so an empty day later stays empty.
        if !hasNotes(to) {
            copyTree(from: from, to: to)
        }
        defaults.set(true, forKey: Self.seededKey)
    }

    /// 見本のノートを、いま見ているフォルダへ置く。
    ///
    /// **初回に置けなかった人のための道。** 自動で置くのはアプリ自身の
    /// フォルダを使っている初回だけで、既に自分のフォルダを選んでいる人
    /// （同期先を向けている人）には置かない ── 見ていないところに三枚
    /// 落ちて、何週間かあとにどこから来たか分からないものとして見つかる。
    /// それでも「入れてくれ」と言える場所が要る。
    ///
    /// 返すのは置いた数。同じ名前があるものは飛ばすので、二度押しても
    /// 増えない。
    @discardableResult
    func addWelcome() -> Int {
        guard let from = Bundle.main.resourceURL?.appendingPathComponent("welcome"),
              FileManager.default.fileExists(atPath: from.path),
              let to = root
        else {
            trouble = "見本が入っていません"
            return 0
        }
        let before = countNotes(to)
        copyTree(from: from, to: to)
        reload()
        return countNotes(to) - before
    }

    private func countNotes(_ dir: URL) -> Int {
        var n = 0
        let walker = FileManager.default.enumerator(
            at: dir, includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles, .skipsPackageDescendants])
        while let next = walker?.nextObject() as? URL {
            if next.pathExtension.lowercased() == "md" { n += 1 }
        }
        return n
    }

    private func hasNotes(_ dir: URL) -> Bool {
        let walker = FileManager.default.enumerator(
            at: dir, includingPropertiesForKeys: nil,
            options: [.skipsHiddenFiles, .skipsPackageDescendants])
        while let next = walker?.nextObject() as? URL {
            if next.pathExtension.lowercased() == "md" { return true }
        }
        return false
    }

    /// Copy a folder in, keeping its shape. **Never over a file that is
    /// already there** — a sample must not be able to eat somebody's note.
    private func copyTree(from: URL, to: URL) {
        let fm = FileManager.default
        guard let walker = fm.enumerator(
            at: from, includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles]) else { return }
        for case let at as URL in walker {
            let rest = at.path.replacingOccurrences(of: from.path + "/", with: "")
            let landing = to.appendingPathComponent(rest)
            let isDir = (try? at.resourceValues(forKeys: [.isDirectoryKey]))?.isDirectory ?? false
            if isDir {
                try? fm.createDirectory(at: landing, withIntermediateDirectories: true)
            } else if !fm.fileExists(atPath: landing.path) {
                try? fm.createDirectory(
                    at: landing.deletingLastPathComponent(), withIntermediateDirectories: true)
                try? fm.copyItem(at: at, to: landing)
            }
        }
    }

    func choose(_ url: URL) {
        adopt(url, remember: true, scoped: true)
    }

    /// Everything that is here now, moved to a folder that was just chosen.
    ///
    /// **Copy, check, then remove** — see the engine's `migrate`. Between two
    /// providers this is not a rename, and a note lost halfway is the worst
    /// thing this app could do.
    func migrate(from old: URL, to fresh: URL) throws -> Int {
        let scoped = old.startAccessingSecurityScopedResource()
        defer { if scoped { old.stopAccessingSecurityScopedResource() } }
        let out = try Cian.call("migrate", ["from": old.path, "to": fresh.path])
        reload()
        return out["moved"] as? Int ?? 0
    }

    /// A backup, put back into the notes folder. Nothing already there is
    /// overwritten — the count of what was left alone comes back too.
    func restore(_ zip: URL) throws -> (Int, Int) {
        guard let root else { throw Cian.Failure.engine("保存場所がありません") }
        let scoped = zip.startAccessingSecurityScopedResource()
        defer { if scoped { zip.stopAccessingSecurityScopedResource() } }
        let out = try Cian.call("restore", ["zip": zip.path, "to": root.path])
        reload()
        return (out["put"] as? Int ?? 0, out["kept"] as? Int ?? 0)
    }

    /// How many notes are in a folder that is not the current one — asked
    /// before offering to move them, because 「N 件」 is the difference
    /// between a question and a shrug.
    func notesAt(_ url: URL) -> Int {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        guard let out = try? Cian.call("notes", ["path": url.path]) else { return 0 }
        return (out["notes"] as? [[String: Any]])?.count ?? 0
    }

    /// アプリ自身のフォルダ。**「この iPhone の中に戻す」ための一つ。**
    ///
    /// 前はここに「開いてきた場所」の履歴が並んでいた。窓は置き場所を一つ
    /// しか持たないので、電話も一つにした ── 二つの amber で「いまどこに
    /// 書いているか」の答えが違う形をしているのが、いちばん分かりにくい。
    /// 探すのが大変なのは変わらないが、それは選ぶ画面の話で、選んだあとに
    /// 八つ並べておく話ではない。
    var ownName: String { "この iPhone の中（amber）" }

    /// The path as a trail of names, for a URL that is not the current one.
    ///
    /// The tail, because the front of a provider's path is its own
    /// bookkeeping — 「Google Drive › 仕事 › ノート」 is the answer;
    /// `/private/var/mobile/Library/CloudStorage/…` is not.
    private func trail(of url: URL) -> String {
        let parts = url.pathComponents.filter { $0 != "/" }
        let keep = parts.suffix(3)
        return (parts.count > keep.count ? "… › " : "") + keep.joined(separator: " › ")
    }

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
            stars = answer["stars"] as? [String] ?? []
            colors = answer["colors"] as? [String: String] ?? [:]
            shares = (answer["shares"] as? [[String: Any]] ?? []).compactMap {
                guard let at = $0["at"] as? String else { return nil }
                return Shelf(at: at, by: $0["by"] as? String ?? "")
            }
            waiting = (answer["waiting"] as? [[String: Any]] ?? [])
                .compactMap { $0["of"] as? String }
            fetch(answer["waiting"] as? [[String: Any]] ?? [])
            clashes = notes.filter { $0.clash != nil }
            trouble = nil
        } catch {
            trouble = error.localizedDescription
        }
    }

    /// 家族と分けてある棚。**一つとは限らない** ── 印はフォルダごとに置くので、
    /// 家族用と仕事用が両方あっていい。
    ///
    /// **教えてもらわなくても分かる。** 印は共有フォルダの中の一枚
    /// （`notebook::SHARE_MARK`）で、フォルダと一緒に旅をする ── 受け取った
    /// 人が自分の amber に「これが共有です」と教え直す手が要らない。
    @Published var shares: [Shelf] = []

    struct Shelf: Identifiable, Equatable {
        let at: String
        let by: String
        var id: String { at }
        var name: String { at.split(separator: "/").last.map(String.init) ?? "ぜんぶ" }
    }

    /// このノートは、その棚の中か。
    func inShare(_ at: String, _ note: Note) -> Bool {
        at.isEmpty || note.book == at || note.book.hasPrefix(at + "/")
    }

    /// 共有の棚にする（`off` で、やめる）。**フォルダが無ければ作る。**
    func setShare(_ folder: String, off: Bool = false, by: String = "") throws {
        guard let root else { return }
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd"
        _ = try Cian.call("share", [
            "path": root.path, "folder": folder, "off": off,
            "by": by, "today": f.string(from: Date()),
        ])
        reload()
    }

    /// まだ落ちてきていないノートの名前。**黙って足りない一覧を見せない。**
    ///
    /// iCloud は中身を消して `.買い物リスト.md.icloud` という札を置くので、
    /// 名前が違って一覧に出ない ── 言わないと「ノートが消えた」にしか
    /// 見えないが、待てば戻ってくるだけ。
    @Published var waiting: [String] = []

    /// クラウドが作った控え。ノートとしては一覧に出したまま、札を貼る。
    @Published var clashes: [Note] = []

    /// 落ちてきていないものを、**落としてきてもらう。**
    ///
    /// 待てば来るとはいえ、待つきっかけは要る ── iCloud は「誰かが要ると
    /// 言った」ときに取りに行く。頼むだけで、返事は待たない（来たら
    /// ファイルの見張りが一覧を描き直す）。
    ///
    /// 頼めなくても何も言わない ── iCloud に置いていないフォルダなら
    /// そもそも札が出ないし、出たのに頼めないのは向こうの都合で、
    /// 人にできることが何も無い。
    private func fetch(_ rows: [[String: Any]]) {
        for r in rows {
            guard let at = r["at"] as? String else { continue }
            try? FileManager.default
                .startDownloadingUbiquitousItem(at: URL(fileURLWithPath: at))
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
        case updated, created, title
        var id: String { rawValue }
        var label: String {
            switch self {
            case .updated: return "更新順"
            case .created: return "作成順"
            case .title: return "タイトル順"
            }
        }
    }

    @Published var order: Order = .updated

    /// Tags being narrowed by, pressed rather than typed.
    ///
    /// Several at once means **all of them** — the note that is both 仕事 and
    /// 定型. Any-of would grow the list as you press, which is the opposite of
    /// what pressing a filter is for.
    @Published var only: Set<String> = []

    /// 絞り込んでいるフォルダ。**タグとは重なり方が違う** ── ノートは一つの
    /// フォルダにしか居ないので、フォルダを「全部」にすると二つ選んだ瞬間に
    /// 必ず 0 件になる。どれかに入っていれば通す（窓と同じ）。
    @Published var onlyBooks: Set<String> = []

    /// 期間の絞り込み。`from` / `to` は `YYYY-MM-DD`（片方だけでもよい）。
    ///
    /// **日で比べる。** 秒で比べると「9月6日まで」が 9月6日 0時 までになり、
    /// その日に書いたものが軒並み落ちる ── 人の言う「まで」はその日を含む。
    struct Span: Equatable {
        var created = false
        var from: String?
        var to: String?
    }
    @Published var span: Span?

    /// 何か絞っているか（言葉で探しているぶんは含めない）。
    var narrowing: Bool { !only.isEmpty || !onlyBooks.isEmpty || span != nil }

    /// その日付が、選んだ範囲の中にあるか。
    func inSpan(_ note: Note) -> Bool {
        guard let s = span else { return true }
        let at = s.created ? note.created : note.updated
        let day = Self.day(at)
        if let from = s.from, day < from { return false }
        if let to = s.to, day > to { return false }
        return true
    }

    /// 秒を `YYYY-MM-DD` に（その土地の日付で）。
    static func day(_ secs: UInt64) -> String {
        stamper.string(from: Date(timeIntervalSince1970: TimeInterval(secs)))
    }

    /// 作り直さない ── 一覧を絞るたびに数十回呼ばれる。
    private static let stamper: DateFormatter = {
        let f = DateFormatter()
        f.calendar = Calendar.current
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd"
        return f
    }()

    /// The tags on the notes you can see from here, most used first.
    ///
    /// From what is in front of you and not from the whole folder: a bar of
    /// forty tags is a bar nobody reads, and the ones worth pressing are the
    /// ones this pile actually has.
    var tagsHere: [String] {
        var count: [String: Int] = [:]
        for n in notes where flat || n.book == at || at.isEmpty {
            for t in n.tags { count[t, default: 0] += 1 }
        }
        return count.keys.sorted {
            count[$0] == count[$1] ? $0 < $1 : count[$0]! > count[$1]!
        }
    }

    /// Which notebook is open, as a path relative to the root. `""` is the
    /// top. This is *where you are*, not a filter — the two look the same on
    /// screen for one level and stop looking the same the moment there is a
    /// notebook inside a notebook.
    @Published var at = ""

    /// The folders stepped out of, newest last.
    ///
    /// **So that going back can be undone.** Swiping one way is "up", and a
    /// gesture that cannot be taken back is a gesture people stop trusting —
    /// so swiping the other way walks back down the way you came.
    @Published private(set) var forward: [String] = []

    /// Up one, remembering where we were.
    func leave(for to: String) {
        forward.append(at)
        at = to
    }

    /// Into a folder chosen by hand. That is a new direction, so whatever
    /// was ahead is no longer ahead of anything.
    func into(_ book: String) {
        forward.removeAll()
        at = book
    }

    /// Back down the way we came, if there is a way.
    func back() -> Bool {
        guard let last = forward.popLast() else { return false }
        at = last
        return true
    }

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

    /// Every favourite shelf, including the empty ones.
    @Published var stars: [String] = []
    /// Folder path → the colour it was given.
    @Published var colors: [String: String] = [:]

    /// The shelves directly inside `shelf`, with how many notes are under each.
    func shelves(in shelf: String) -> [(name: String, path: String, count: Int)] {
        let prefix = shelf.isEmpty ? "" : shelf + "/"
        var seen: [String] = []
        for s in stars where s.hasPrefix(prefix) {
            let rest = String(s.dropFirst(prefix.count))
            guard !rest.isEmpty else { continue }
            let head = rest.split(separator: "/").first.map(String.init) ?? rest
            let full = prefix + head
            if !seen.contains(full) { seen.append(full) }
        }
        return seen.map { full in
            let n = notes.filter { $0.star == full || ($0.star?.hasPrefix(full + "/") ?? false) }.count
            return (String(full.dropFirst(prefix.count)), full, n)
        }
    }

    /// Which folders and shelves are open in the tree.
    ///
    /// On the store rather than in the view: the list is rebuilt on every
    /// keystroke and every reload, and a disclosure state that lived in the
    /// view would close every folder each time a note was saved.
    @Published var unfolded: Set<String> = []

    func opened(_ key: String) -> Binding<Bool> {
        Binding(
            get: { self.unfolded.contains(key) },
            set: { if $0 { self.unfolded.insert(key) } else { self.unfolded.remove(key) } }
        )
    }

    /// Show the tree rather than one folder at a time.
    ///
    /// **既定は切り。** 窓の左の列は「すべてのノート・ブックマーク・
    /// フォルダ・タグ」を並べて、その右に日付ごとのノートを出す ── 電話も
    /// 同じ形にした。木は同じものを二度描く（フォルダの段とノートの段）
    /// ので、出すときは上のフォルダの段のほうを引っこめる。
    /// 木が要る人は「フィルタ」から入れる。
    @Published var tree = false

    /// The folders directly inside one, wherever the list is standing.
    ///
    /// Like `books` but from a given path rather than from `at` — the tree
    /// asks about every level, not only the one that is open.
    func shelfless(in book: String) -> [(name: String, path: String, count: Int)] {
        let prefix = book.isEmpty ? "" : book + "/"
        var seen: [String] = []
        for b in allBooks where b.hasPrefix(prefix) {
            let rest = String(b.dropFirst(prefix.count))
            guard !rest.isEmpty else { continue }
            let head = rest.split(separator: "/").first.map(String.init) ?? rest
            let full = prefix + head
            if !seen.contains(full) { seen.append(full) }
        }
        return seen.map { full in
            (String(full.dropFirst(prefix.count)), full, under(full))
        }
    }

    /// The favourites standing directly on one shelf.
    func starred(on shelf: String) -> [Note] {
        sorted(notes.filter { $0.star == shelf })
    }

    /// Put a note on a shelf, or take it off the favourites entirely.
    ///
    /// Reads the file rather than taking text from a caller: this is done
    /// from the list, where nothing has the note open, and the stamp that
    /// comes back with the text is the one the save is checked against — so
    /// starring a note goes through the same check as typing in it.
    func star(_ note: Note, on shelf: String?) throws {
        let (text, stamp) = try open(note)
        var p: [String: Any] = ["text": text]
        if let shelf { p["shelf"] = shelf }
        let out = try Cian.call("star", p)
        _ = try save(note, text: out["text"] as? String ?? text, stamp: stamp)
        reload()
    }

    /// Make a favourite shelf, or forget one and everything under it.
    func shelf(_ name: String, drop: Bool = false) throws {
        guard let root else { return }
        _ = try Cian.call("shelf", ["path": root.path, "name": name, "drop": drop])
        reload()
    }

    /// How many notes are anywhere under one folder — what a delete would
    /// take with it, said before it is done.
    func under(_ book: String) -> Int {
        notes.filter { $0.book == book || $0.book.hasPrefix(book + "/") }.count
    }

    /// Rename a folder. The notes inside keep their names and their words.
    func rename(_ book: String, to name: String) throws {
        guard let root else { return }
        _ = try Cian.call("book", ["path": root.path, "book": book, "name": name])
        // Standing in the folder that was renamed, the old path is a place
        // that no longer exists — so step up rather than show nothing.
        if at == book || at.hasPrefix(book + "/") {
            at = book.split(separator: "/").dropLast().joined(separator: "/")
        }
        reload()
    }

    /// Throw a folder away with everything in it. **There is no wastepaper
    /// basket on a phone** — the asking happens before this is called.
    func drop(_ book: String) throws {
        guard let root else { return }
        _ = try Cian.call("book", ["path": root.path, "book": book, "drop": true])
        if at == book || at.hasPrefix(book + "/") {
            at = book.split(separator: "/").dropLast().joined(separator: "/")
        }
        reload()
    }

    /// Give a folder a colour, or take it away.
    func color(_ folder: String, _ hex: String?) throws {
        guard let root else { return }
        _ = try Cian.call("color", ["path": root.path, "folder": folder, "color": hex ?? NSNull()])
        reload()
    }

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

    /// The query, as groups of words: an OR of ANDs.
    ///
    /// **What the query means is `cian-core`'s answer** (`note::terms`) —
    /// asked once when the text changes, not once per note. Three front ends
    /// each deciding what two words mean is three search boxes that agree
    /// until somebody types two words.
    private var groups: [[Term]] = []

    /// 絞り込みの一語。**どれが見出しでどれが字かは `note::terms` が決める。**
    ///
    /// `tag:定型` `book:仕事` `title:週報`（`タグ:` `フォルダ:` `題:` も同じ）と
    /// `-` の打ち消し。電話が自分で `:` を数えはじめると、窓と別のものが
    /// 見つかる検索窓が二つできる。
    private struct Term {
        let field: String
        let word: String
        let not: Bool
    }

    func read(_ needle: String) {
        let q = needle.trimmingCharacters(in: .whitespaces)
        let raw = (try? Cian.call("terms", ["q": q])["groups"] as? [[[String: Any]]]) ?? []
        groups = raw.map { g in
            g.compactMap { d in
                guard let w = d["word"] as? String else { return nil }
                return Term(field: d["field"] as? String ?? "any",
                            word: w,
                            not: d["not"] as? Bool ?? false)
            }
        }
    }

    /// 一語が当たるか。**見出しごとに探し先が違う。**
    ///
    /// `search` は `note::haystack`（題＋`#タグ`＋本文の頭）で、既に小文字。
    private func hit(_ note: Note, _ t: Term) -> Bool {
        let hay: String
        switch t.field {
        case "title": hay = note.title.lowercased()
        case "tag": hay = note.tags.joined(separator: " ").lowercased()
        case "book": hay = note.book.lowercased()
        default: hay = note.search
        }
        return hay.contains(t.word) != t.not
    }

    /// Narrow by what a note is *about*, not by what its file is called.
    func matching(_ needle: String) -> [Note] {
        let n = needle.trimmingCharacters(in: .whitespaces).lowercased()
        var out = notes
        // Searching looks everywhere, whatever notebook is open: the note you
        // are looking for is the one you have forgotten where you put.
        // A tag narrows like a search does — everywhere, not just here.
        // Pressing 「#仕事」 while standing in one folder and being shown only
        // that folder's 仕事 notes is the answer to a question nobody asked.
        if !flat && n.isEmpty && !narrowing { out = out.filter { $0.book == at } }
        if !n.isEmpty {
            // Either half: what the listing knows, or what was found inside.
            // Every word of one group has to be there; any group will do.
            out = out.filter { note in
                if hits[note.path] != nil { return true }
                guard !groups.isEmpty else { return note.search.contains(n) }
                return groups.contains { g in g.allSatisfy { hit(note, $0) } }
            }
        }
        if !only.isEmpty { out = out.filter { only.isSubset(of: Set($0.tags)) } }
        // フォルダは「どれか」、タグは「全部」── 重なり方が違うことは
        // 引き出しの中に書いてある（窓と同じ）。
        if !onlyBooks.isEmpty {
            out = out.filter { note in
                onlyBooks.contains { note.book == $0 || note.book.hasPrefix($0 + "/") }
            }
        }
        if span != nil { out = out.filter(inSpan) }
        // The favourites are drawn in their own section above this, so they
        // come out here rather than being sorted to the front: a note in two
        // places at once is a note somebody deletes twice.
        let stuck = Set(pinnedHere(needle).map(\.path))
        return sorted(out.filter { !stuck.contains($0.path) })
    }

    /// The favourites to show above the list.
    ///
    /// At the top of the folder, **every** favourite wherever it lives — that
    /// is what a favourite is for: the note you keep coming back to, within
    /// reach without going to find it. Inside a folder, only that folder's,
    /// because there you are looking at one place on purpose.
    func pinnedHere(_ needle: String) -> [Note] {
        guard needle.trimmingCharacters(in: .whitespaces).isEmpty, !flat, !narrowing else { return [] }
        let all = notes.filter { $0.star != nil }
        return sorted(at.isEmpty ? all : all.filter { $0.book == at })
    }

    /// A run of notes under one heading.
    struct Band: Identifiable {
        let name: String
        let notes: [Note]
        var id: String { name }
    }

    /// The list, cut into the runs a person actually reads it in.
    ///
    /// **The headings follow the ordering.** Grouped by date while sorted by
    /// title would put 「今日」 above a note from March, which is worse than
    /// no headings at all — so by title the headings are first letters, and
    /// by date they are days.
    func bands(_ list: [Note]) -> [Band] {
        var names: [String] = []
        var rows: [String: [Note]] = [:]
        for n in list {
            let name = order == .title ? initial(n.title) : when(order == .created ? n.created : n.updated)
            if rows[name] == nil { names.append(name) }
            rows[name, default: []].append(n)
        }
        return names.map { Band(name: $0, notes: rows[$0] ?? []) }
    }

    /// 今日 / 昨日 / 今週 / 今月 / それ以前.
    private func when(_ secs: UInt64) -> String {
        guard secs > 0 else { return "日付なし" }
        let cal = Calendar.current
        let day = Date(timeIntervalSince1970: TimeInterval(secs))
        if cal.isDateInToday(day) { return "今日" }
        if cal.isDateInYesterday(day) { return "昨日" }
        if cal.isDate(day, equalTo: Date(), toGranularity: .weekOfYear) { return "今週" }
        if cal.isDate(day, equalTo: Date(), toGranularity: .month) { return "今月" }
        if cal.isDate(day, equalTo: Date(), toGranularity: .year) {
            return "\(cal.component(.month, from: day)) 月"
        }
        return "\(cal.component(.year, from: day)) 年"
    }

    private func initial(_ title: String) -> String {
        guard let c = title.first else { return "—" }
        return String(c).uppercased()
    }

    private func sorted(_ list: [Note]) -> [Note] {
        var out = list
        switch order {
        case .updated: out.sort { $0.updated > $1.updated }
        case .created: out.sort { $0.created > $1.created }
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

    /// A note split into its bookkeeping and its words.
    ///
    /// The writing half shows only the second part — see the engine's
    /// `split`. Where the front matter ends is decided there, so the phone,
    /// the window and the terminal all agree where a note starts.
    func split(_ text: String) throws -> (String, String) {
        let out = try Cian.call("split", ["text": text])
        return (out["head"] as? String ?? "", out["body"] as? String ?? text)
    }

    /// The note with one task ticked or unticked, as text to save.
    ///
    /// By the line it is on, not by which box it is — see `note::set_check`.
    func checked(_ text: String, line: Int, done: Bool) throws -> String {
        let out = try Cian.call("check", ["text": text, "line": line, "done": done])
        return out["text"] as? String ?? text
    }

    /// A piece of text wrapped in a colour, written the way cian writes it.
    ///
    /// Through the engine rather than a format string here: the notation is
    /// one decision, and two places that write it are two notations one edit
    /// apart.
    func painted(_ text: String, _ color: String) throws -> String {
        let out = try Cian.call("paint", ["text": text, "color": color])
        return out["text"] as? String ?? text
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
        guard let root else { throw Cian.Failure.engine("保存場所がありません") }
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
        // **書き込む直前の姿を、履歴に渡す。** 一世代にするかどうかを決める
        // のは core（最後の一区切りから間が空いたときだけ）── 電話と窓で
        // 決まりが違うと、片方で消えたものをもう片方が残っていると思う。
        // 履歴が置けないことで、保存が止まる理由はない。
        if let root = root?.path {
            _ = try? Cian.call("keep", [
                "root": root, "path": note.path, "gap": 300,
            ])
        }
        var params: [String: Any] = ["path": note.path, "text": text, "force": force]
        if !stamp.isEmpty { params["stamp"] = stamp }
        let answer = try Cian.call("write", params)
        if answer["conflict"] as? Bool == true {
            return .conflict(why: answer["why"] as? String ?? "開いたあとで書き換えられています")
        }
        return .ok(stamp: answer["stamp"] as? String ?? "")
    }

    /// **いまの姿を、一世代として残す。**
    ///
    /// 自動保存だと世代が打鍵の切れ目で決まる ── 「ここは残しておきたい」
    /// を人が言える道が要る（窓の ⌘S と同じ `keep`：間を置かず・印を付けて）。
    /// 印の付いた世代は数の勘定から外れるので、あとから流れて消えない。
    func keepNow(path: String, text: String) throws -> String {
        guard let root = root?.path else { return "保存場所がありません" }
        let out = try Cian.call("keep", [
            "root": root, "path": path, "text": text,
            "gap": 0, "force": true, "kept": true,
        ])
        let stamp = out["stamp"] as? String ?? ""
        return stamp.isEmpty
            ? "このバージョンはもう残してあります。"
            : "いまのバージョンを残しました（これは消えません）。"
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
