import Foundation
import SwiftUI

/// ノートがどこにあり、中に何があるか。
///
/// フォルダは OS の選択画面で一度選び、**security-scoped bookmark** として
/// 記録する。これは細部の話ではなく、このアプリが Google Drive も Dropbox も
/// iCloud のコードも一切持たずに済む理由そのもの。iOS ではその 3 つとも
/// 「ファイル」のプロバイダなので、選んだフォルダ 1 つでどれにも届く ── しかも
/// それは*Mac が開いているのと同じフォルダ*で、この仕組みはそのために
/// 作ってある。
@MainActor
final class NotesStore: ObservableObject {
    @Published var notes: [Note] = []
    @Published var trouble: String?

    /// **保存ディレクトリ**（依頼 511・デスクトップ版の `state.places` と同じ形）。**いくつでも。**
    /// `sync` は `drive`／`none`、`at` は Drive の上での置き場所（'' はいちばん目）。
    /// 選んだフォルダは security-scoped bookmark で憶える ── それがこのアプリに
    /// Google Drive も Dropbox も iCloud のコードも要らない理由（どれも「ファイル」の
    /// 提供者で、選んだ一つのフォルダがそのまま Mac が開いているフォルダになる）。
    struct Place: Identifiable, Codable, Equatable {
        var id: String
        var name: String
        var sync: String
        var at: String
        /// アプリ自身のフォルダ（「ファイル」→ この iPhone 内 → ambər）。
        var own: Bool
        var bookmark: Data?
    }
    @Published var places: [Place] = []
    /// 読めなかった保存ディレクトリ（id → 理由）。無かったことにしない。
    @Published var placeTrouble: [String: String] = [:]
    /// いま開いている保存ディレクトリ（一覧のフォルダはこの中のもの）。
    @Published var placeId: String = ""
    /// 開けた保存ディレクトリ（id → URL・キーを開けたまま）。
    private var urls: [String: URL] = [:]
    private static let bookmarkKey = "cian.notes.root"
    private static let placesKey = "amber.places"
    private static let placeKey = "amber.place"
    private static let seededKey = "amber.notes.seeded"

    /// アプリ自身のフォルダ。ほかに何も指定されていないときのノートの置き場所。
    /// chosen.
    ///
    /// 「ファイル」には **cian** として出る。アプリが `UIFileSharingEnabled` を
    /// 宣言しているため ── つまり隠しコンテナではなく、どこからでもファイルを
    /// 置ける本物の場所。選択画面から始めずにここから始めるのが大事 ── 選択画面は
    /// クラウドのプロバイダを並べるが、アプリが入っていないプロバイダは
    /// *並ぶが灰色*になり、それは「Drive がこの端末に無い」ではなく
    /// 「amber が自分の Drive を見られない」と読める。
    ///
    private var ownFolder: URL? {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
    }

    /// いま開いている保存ディレクトリ（無ければいちばん目）。
    var place: Place? { places.first { $0.id == placeId } ?? places.first }
    /// その保存ディレクトリのフォルダ。
    private var root: URL? { place.flatMap { urls[$0.id] } }
    func url(of p: Place) -> URL? { urls[p.id] }
    /// 二つ以上あるか ── 名前を頭に付けるのはそのときだけ。
    var many: Bool { places.count > 1 }
    /// 一覧の上に出す名前（いま開いている保存ディレクトリのもの）。
    var rootName: String { place?.name ?? "" }
    /// ノートが、選んだフォルダではなくアプリ自身のフォルダにあるか。
    var own: Bool { place?.own ?? true }
    /// The path as a trail of names — 「この iPhone › ambər › 仕事」.
    ///
    /// 端末はパスを隠す。たいていは親切だがここでは違う ── 同じフォルダ名が
    /// 3 つのクラウドに存在しうるし、「ambər」だけでは*どこにあるか*を
    /// 訊いているのに*何という名前か*に答えていることになる。
    var trail: [String] { place.map(trail(of:)) ?? [] }
    func trail(of p: Place) -> [String] {
        if p.own { return ["この iPhone", "ambər"] }
        guard let url = urls[p.id] else { return [p.name, "（見つかりません）"] }
        // パスの末尾。意味があるのはそこだけで、前半は
        // プロバイダ自身の管理情報。
        let parts = url.pathComponents.filter { $0 != "/" }
        let keep = parts.suffix(4)
        return (parts.count > keep.count ? ["…"] : []) + keep
    }

    /// 選んだフォルダのパス。それを表示すべき唯一の画面のため。
    var rootPath: String { root?.path ?? "" }
    var rootURL: URL? { root }

    /// そのパスが入っている保存ディレクトリのフォルダ（長いほうが勝つ）。分からなければいま開いているもの。
    func rootOf(_ path: String) -> String {
        var hit = ""
        for (_, url) in urls where path == url.path || path.hasPrefix(url.path + "/") {
            if url.path.count > hit.count { hit = url.path }
        }
        return hit.isEmpty ? rootPath : hit
    }
    /// このノートは、いま開いている保存ディレクトリのものか。
    func here(_ n: Note) -> Bool { n.root == rootPath }
    /// ノートの居場所を言葉に（二つ以上なら保存ディレクトリの名前を頭に）。
    func bookLabel(_ n: Note) -> String {
        let name = places.first { urls[$0.id]?.path == n.root }?.name ?? ""
        if n.book.isEmpty { return many ? name : rootName }
        return (many && !name.isEmpty ? name + " › " : "") + n.book
    }

    /// Go back to the app's own folder ── **一つだけにする**（走査と「この iPhone の中に戻す」）。
    func useOwn() {
        UserDefaults.standard.removeObject(forKey: Self.bookmarkKey)
        for p in places where !p.own { urls[p.id]?.stopAccessingSecurityScopedResource() }
        let own = Place(id: places.first { $0.own }?.id ?? UUID().uuidString, name: "ambər", sync: "drive", at: "", own: true, bookmark: nil)
        urls = [:]
        placeTrouble = [:]
        places = [own]
        placeId = own.id
        open()
        persist()
        reload()
        tidyNames()
    }

    /// 保存ディレクトリを一つ足す（依頼 511）。**入った直後は同期しない** ── 会社の共有
    /// フォルダを足した人の一覧を、黙って Drive に上げない。
    func add(_ url: URL, named: String? = nil) {
        let path = url.standardizedFileURL.path
        if urls.values.contains(where: { $0.path == path }) { trouble = "そのフォルダはもう入っています"; return }
        if urls.values.contains(where: { path.hasPrefix($0.path + "/") || $0.path.hasPrefix(path + "/") }) {
            trouble = "そこは、ほかの保存ディレクトリと重なります（入れ子にはできません）"
            return
        }
        let leaf = named ?? url.lastPathComponent
        var name = leaf
        var n = 2
        while places.contains(where: { $0.name == name }) { name = leaf + " \(n)"; n += 1 }
        // Drive の上の置き場所 ── いちばん目のフォルダと名前がぶつからないように。
        var taken = Set(places.map(\.at))
        if let first = places.first, let f = urls[first.id] {
            for b in booksBy[f.path] ?? [] { taken.insert(String(b.split(separator: "/").first ?? "")) }
        }
        var at = name
        n = 2
        while taken.contains(at) { at = name + " \(n)"; n += 1 }
        let p = Place(id: UUID().uuidString, name: name, sync: "none", at: at, own: false, bookmark: try? url.bookmarkData())
        places.append(p)
        open()
        placeId = p.id
        self.at = ""
        persist()
        reload()
        tidyNames()
    }

    /// 場所を変える（前の「保存場所を変える」と同じ流れ ── 移すかどうかは画面が訊く）。
    func relocate(_ id: String, to url: URL) {
        guard let i = places.firstIndex(where: { $0.id == id }) else { return }
        let path = url.standardizedFileURL.path
        if urls.contains(where: { $0.key != id && ($0.value.path == path || path.hasPrefix($0.value.path + "/") || $0.value.path.hasPrefix(path + "/")) }) {
            trouble = "そこは、ほかの保存ディレクトリと重なります（入れ子にはできません）"
            return
        }
        if let was = urls[id], !places[i].own { was.stopAccessingSecurityScopedResource() }
        urls[id] = nil
        places[i].own = false
        places[i].bookmark = try? url.bookmarkData()
        open()
        persist()
        reload()
        tidyNames()
    }

    /// この iPhone の中（アプリ自身のフォルダ）にする。
    func relocateToOwn(_ id: String) {
        guard let i = places.firstIndex(where: { $0.id == id }) else { return }
        if places.contains(where: { $0.own && $0.id != id }) { trouble = "この iPhone の中は、もう別の保存ディレクトリになっています"; return }
        if let was = urls[id], !places[i].own { was.stopAccessingSecurityScopedResource() }
        urls[id] = nil
        places[i].own = true
        places[i].bookmark = nil
        open()
        persist()
        reload()
    }

    /// 一覧から外す。**ファイルは消さない。** 最後の一つは外せない。
    @discardableResult
    func remove(place id: String) -> Bool {
        guard places.count > 1, let i = places.firstIndex(where: { $0.id == id }) else { return false }
        if let was = urls[id], !places[i].own { was.stopAccessingSecurityScopedResource() }
        urls[id] = nil
        placeTrouble[id] = nil
        places.remove(at: i)
        if placeId == id { placeId = places[0].id; at = "" }
        persist()
        reload()
        return true
    }

    func rename(place id: String, to name: String) {
        let n = name.trimmingCharacters(in: .whitespaces)
        guard !n.isEmpty, let i = places.firstIndex(where: { $0.id == id }) else { return }
        if places.contains(where: { $0.id != id && $0.name == n }) { trouble = "「\(n)」はもうあります"; return }
        places[i].name = n
        persist()
        reload()
    }

    func setSync(_ id: String, _ sync: String) {
        guard let i = places.firstIndex(where: { $0.id == id }) else { return }
        places[i].sync = sync == "drive" ? "drive" : "none"
        persist()
    }

    /// 別の保存ディレクトリへ移る（一覧のフォルダの段の切り替え）。
    func enter(_ id: String) {
        guard places.contains(where: { $0.id == id }), id != placeId else { return }
        placeId = id
        at = ""
        forward.removeAll()
        UserDefaults.standard.set(id, forKey: Self.placeKey)
    }

    private func persist() {
        if let data = try? JSONEncoder().encode(places) { UserDefaults.standard.set(data, forKey: Self.placesKey) }
        UserDefaults.standard.set(placeId, forKey: Self.placeKey)
    }

    /// 憶えたぶんを開く（キーも開ける）。開けないものは `placeTrouble` に。
    private func open() {
        for i in places.indices {
            let p = places[i]
            if urls[p.id] != nil { continue }
            if p.own {
                if let own = ownFolder { urls[p.id] = own; placeTrouble[p.id] = nil }
                continue
            }
            guard let data = p.bookmark else { placeTrouble[p.id] = "そのフォルダの憶えがありません"; continue }
            var stale = false
            guard let url = try? URL(resolvingBookmarkData: data, options: [], relativeTo: nil, bookmarkDataIsStale: &stale) else {
                // クラウドのフォルダは移動するし、サインアウトされるし、
                // 更新後に古い状態で返ってくることもある。そう言うほうが、
                // 「ノートが 1 つもありません」に見える空の一覧よりよい。
                placeTrouble[p.id] = "そのフォルダが見つかりません"
                continue
            }
            // **権限は開いて閉じる必要がある** ── フォルダに対して
            // somebody picked. 開けなくても読めるなら使う（アプリの中のフォルダ）。
            if !url.startAccessingSecurityScopedResource(), !FileManager.default.isReadableFile(atPath: url.path) {
                placeTrouble[p.id] = "そのフォルダを開く許可がありません"
                continue
            }
            urls[p.id] = url
            placeTrouble[p.id] = nil
            if places[i].name.isEmpty { places[i].name = url.lastPathComponent }
            if stale, let fresh = try? url.bookmarkData() { places[i].bookmark = fresh }
        }
    }

    /// 開いている「ほかの場所のノート」の鍵（一つだけ・次を開いたら前のは閉じる）。
    private var outside: URL?

    /// ほかの場所の .md を、一覧に入れずに開く（パソコン版の ⌘O と同じ・依頼 517）。
    /// 返るノートは `root` が空 ── それが「一時的に開いている」マークになる。
    func openOutside(_ url: URL) -> Note? {
        outside?.stopAccessingSecurityScopedResource()
        _ = url.startAccessingSecurityScopedResource()
        outside = url
        guard let got = try? Cian.call("note", ["path": url.path]), let note = Note(got) else {
            trouble = "そのファイルを読めません"
            return nil
        }
        return note
    }

    /// いま一時的に開いている、ほかの場所のノートか。
    func isOutside(_ path: String) -> Bool { outside?.path == path }

    /// よそから Markdown のファイルをコピーして取り込む。
    ///
    /// **移動ではなくコピー。** 書き出した側にも残る ── 初めて試す人が、
    /// amber をノートの置き場所にすると決めきる前に欲しい答えが
    /// それだから。
    /// **名前の付け直しは core が決める。**
    ///
    /// ここで `名前-2.md` を組み立てていた ── デスクトップ版にも取り込みを付けたので、
    /// 同じ規則が二組になった。二組あるものは必ずずれ、ずれると**同じ
    /// ノートが端末によって別の名前で入る**（フォルダの色をデスクトップ版と iPhone に
    /// 二度書いて、十一色のうち六色がずれたのと同じ）。写す仕事だけ
    /// こちらに残る ── 選ばれた URL のキーを開けていられるのはここだけ。
    func bring(_ urls: [URL]) {
        guard let root else { return }
        var scoped: [URL] = []
        defer { for u in scoped { u.stopAccessingSecurityScopedResource() } }
        for url in urls where url.startAccessingSecurityScopedResource() { scoped.append(url) }
        do {
            let r = try Cian.call("bring", [
                "files": urls.map(\.path),
                "to": root.path,
            ])
            if (r["put"] as? Int ?? 0) > 0 { reload() }
            // 入らなかったぶんは、黙らない（デスクトップ版と同じ）。
            if let no = r["failed"] as? Int, no > 0 {
                trouble = "\(no) 件は入れられませんでした。"
            }
        } catch {
            trouble = error.localizedDescription
        }
    }

    /// 前回のフォルダ、無ければアプリ自身のフォルダ。
    ///
    /// **前の `cian.notes.root` 一つから引き継ぐ**（依頼 511）── 憶えが `places` に
    /// なっていないiPhone では、いままでの場所が一つ目になる（同期はいままで通り Drive）。
    func restore() {
        seedWelcome()
        let d = UserDefaults.standard
        if let data = d.data(forKey: Self.placesKey),
           let saved = try? JSONDecoder().decode([Place].self, from: data), !saved.isEmpty {
            places = saved
        } else if let data = d.data(forKey: Self.bookmarkKey) {
            places = [Place(id: UUID().uuidString, name: "", sync: "drive", at: "", own: false, bookmark: data)]
        } else {
            places = [Place(id: UUID().uuidString, name: "ambər", sync: "drive", at: "", own: true, bookmark: nil)]
        }
        open()
        let want = d.string(forKey: Self.placeKey) ?? ""
        placeId = places.contains { $0.id == want } ? want : places[0].id
        persist()
        reload()
        tidyNames()
    }

    /// **初めて開いた人に空の一覧を見せない。**
    ///
    /// 何も入っていなければ、アプリに何ができるかを学ぶ場所が無い ──
    /// Markdown、フォルダ、タグ、図。読めて、押せて、書き換えられる 3 件の
    /// ノートは、説明の画面より速くそれを教える。しかも Mac のデスクトップ版が
    /// 置くのと同じ 3 件（`packaging/welcome`）なので、どちらの amber も
    /// 同じ画面で始まる。
    ///
    /// 一度だけ。**消したものは消したまま** ── 捨てたものが次の起動で
    /// 生え直してはいけないので、記録するのは*置いたこと*であって、
    /// いまフォルダが空かどうかではない。
    private func seedWelcome() {
        let defaults = UserDefaults.standard
        guard !defaults.bool(forKey: Self.seededKey) else { return }
        // **既にフォルダを選んだ人は、初めて開いた人ではない。**
        // その人のノートはそのフォルダにあり、アプリ自身のフォルダに
        // サンプルを置けば、見ていない場所に 3 つのファイルが落ち、
        // 何週間も経ってから、どこから来たか分からないまま見つけることになる。
        guard defaults.data(forKey: Self.bookmarkKey) == nil, defaults.data(forKey: Self.placesKey) == nil else {
            defaults.set(true, forKey: Self.seededKey)
            return
        }
        guard let from = Bundle.main.resourceURL?.appendingPathComponent("welcome"),
              FileManager.default.fileExists(atPath: from.path),
              let to = ownFolder else { return }
        // 既にノートがある ── 自分のフォルダか、復元したバックアップ。
        // それでも「置いた」と記録する。あとで空になった日も空のままにするため。
        if !hasNotes(to) {
            copyTree(from: from, to: to)
        }
        defaults.set(true, forKey: Self.seededKey)
    }

    /// サンプルのノートを、いま見ているフォルダへ置く。
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
            trouble = "サンプルが入っていません"
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

    /// フォルダを構造ごとコピーして取り込む。**既にあるファイルを上書きしない** ──
    /// サンプルが人のノートを食べられてはいけない。
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

    /// いまここにあるものすべてを、選んだばかりのフォルダへ移す。
    ///
    /// **コピー → 確認 → 削除の順** ── エンジンの `migrate` を見よ。2 つの
    /// プロバイダをまたぐとこれは rename ではないし、途中でノートを失うのは
    /// このアプリがやりうる最悪のこと。
    func migrate(from old: URL, to fresh: URL) throws -> Int {
        let scoped = old.startAccessingSecurityScopedResource()
        defer { if scoped { old.stopAccessingSecurityScopedResource() } }
        let out = try Cian.call("migrate", ["from": old.path, "to": fresh.path])
        reload()
        return out["moved"] as? Int ?? 0
    }

    /// バックアップをノートのフォルダへ書き戻す。既にあるものは上書きせず、
    /// 手を付けなかった件数も返す。
    func restore(_ zip: URL) throws -> (Int, Int) {
        guard let root else { throw Cian.Failure.engine("保存場所がありません") }
        let scoped = zip.startAccessingSecurityScopedResource()
        defer { if scoped { zip.stopAccessingSecurityScopedResource() } }
        let out = try Cian.call("restore", ["zip": zip.path, "to": root.path])
        reload()
        return (out["put"] as? Int ?? 0, out["kept"] as? Int ?? 0)
    }

    /// いまのフォルダではない場所に、ノートがいくつあるか ── 問いと
    /// before offering to move them, because 「N 件」 is the difference
    /// 諦めのあいだで訊く。
    func notesAt(_ url: URL) -> Int {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        guard let out = try? Cian.call("notes", ["path": url.path]) else { return 0 }
        return (out["notes"] as? [[String: Any]])?.count ?? 0
    }

    /// アプリ自身のフォルダ。**「この iPhone の中に戻す」ための一つ。**
    ///
    /// 前はここに「開いてきた場所」の履歴が並んでいた。デスクトップ版は保存場所を一つ
    /// しか持たないので、iPhone も一つにした ── 二つの amber で「いまどこに
    /// 書いているか」の答えが違う形をしているのが、いちばん分かりにくい。
    /// 探すのが大変なのは変わらないが、それは選ぶ画面の話で、選んだあとに
    /// 八つ並べておく話ではない。
    var ownName: String { "この iPhone の中（ambər）" }

    /// パスを名前の連なりにする。いまのものではない URL のため。
    ///
    /// 末尾だけ。プロバイダのパスの前半は、あちらの管理情報だから。
    /// bookkeeping — 「Google Drive › 仕事 › ノート」 is the answer;
    /// `/private/var/mobile/Library/CloudStorage/…` is not.
    /// フォルダ・色・憶え・共有のマークは**保存ディレクトリごと**（core の帳画面が
    /// そこにある）。見せるのは、いま開いている保存ディレクトリのぶん。
    private var booksBy: [String: [String]] = [:]
    private var colorsBy: [String: [String: String]] = [:]
    /// 錠のかかったフォルダ（保存ディレクトリからの道・依頼 629）。
    private var locksBy: [String: [String]] = [:]
    private var cameBy: [String: [String: String]] = [:]
    private var sharesBy: [String: [Shelf]] = [:]

    /// **保存ディレクトリごとに数えて、一つに重ねる**（依頼 511・デスクトップ版の `reload` と同じ）。
    /// core は一つの保存ディレクトリしか知らない ── 二つを一つに見せるのは画面の都合。
    func reload() {
        var all: [Note] = []
        var stars = Set<String>()
        var waiting: [String] = []
        var fetchRows: [[String: Any]] = []
        var firstTrouble: String?
        var readAny = false
        for p in places {
            guard let url = urls[p.id] else { continue }
            do {
                let answer = try Cian.call("notes", ["path": url.path])
                var rows = answer["notes"] as? [[String: Any]] ?? []
                for i in rows.indices { rows[i]["root"] = url.path; rows[i]["place"] = p.name }
                all += rows.compactMap(Note.init)
                booksBy[url.path] = answer["books"] as? [String] ?? []
                for st in answer["stars"] as? [String] ?? [] { stars.insert(st) }
                colorsBy[url.path] = answer["colors"] as? [String: String] ?? [:]
                locksBy[url.path] = answer["locks"] as? [String] ?? []
                cameBy[url.path] = answer["came"] as? [String: String] ?? [:]
                sharesBy[url.path] = (answer["shares"] as? [[String: Any]] ?? []).compactMap {
                    guard let at = $0["at"] as? String else { return nil }
                    return Shelf(at: at, by: $0["by"] as? String ?? "")
                }
                waiting += (answer["waiting"] as? [[String: Any]] ?? []).compactMap { $0["of"] as? String }
                fetchRows += answer["waiting"] as? [[String: Any]] ?? []
                placeTrouble[p.id] = nil
                readAny = true
            } catch {
                placeTrouble[p.id] = error.localizedDescription
                firstTrouble = firstTrouble ?? error.localizedDescription
            }
        }
        notes = all
        self.stars = stars.sorted()
        self.waiting = waiting
        fetch(fetchRows)
        clashes = notes.filter { $0.clash != nil }
        // 一つも読めなかったときだけ言う ── 一つが読めないのは列の中で言う。
        trouble = readAny || places.isEmpty ? nil : firstTrouble
    }

    /// いま書いた一本の行だけ、新しくする。**書いたあとにフォルダを丸ごと数え
    /// 直さない**（デスクトップ版の `freshenRow` と同じ直し）。
    ///
    /// デスクトップ版で測ったら、1002 本のフォルダでは数え直しに 370ms かかっていた ── 一本
    /// ずつ読み直して、全部を JSON にして渡している。**自分が書いた一本の
    /// ことは自分が知っている**：文字を書いて変わるのは題・書き出し・タグ・
    /// 時刻だけで、どのフォルダに居るか（`book`・`shared`・`clash`）は動か
    /// ない。だから手元の行のものを残し、core が答えた分だけ上に重ねる。
    ///
    /// **題の決め方は core に一つ。** ここで「一行目が題」と決めると、
    /// core の決め方（`title:` → 見出し → 書き出し → ファイル名）とずれて、
    /// 一覧と帯で違う名前が出る。
    ///
    /// 訊けなかったら、前のように丸ごと数え直す ── 一覧が古いまま残るよりよい。
    func freshen(_ path: String) {
        guard let at = notes.firstIndex(where: { $0.path == path }),
              let one = try? Cian.call("note", ["path": path])
        else { return reload() }
        let now = notes[at]
        var o: [String: Any] = [
            "path": now.path, "book": now.book, "shared": now.shared, "root": now.root, "place": now.place,
            "title": now.title, "excerpt": now.excerpt, "tags": now.tags,
            "updated": now.updated, "created": now.created, "search": now.search,
        ]
        if let s = now.star { o["star"] = s }
        if let c = now.clash { o["clash"] = ["of": c.of, "by": c.by] }
        // **重ねる欄は名指しで。** `note` は「amber の外にある一本」も読める
        // 口なので、`book` は空で返る ── 丸ごと重ねると、保存するたびに
        // ノートがいちばん上のフォルダへ移ったように見える。
        for k in ["title", "excerpt", "tags", "updated", "created", "search"] {
            if let v = one[k] { o[k] = v }
        }
        guard let fresh = Note(o) else { return reload() }
        notes[at] = fresh
    }

    /// グループと分けてある棚。**一つとは限らない** ── マークはフォルダごとに置くので、
    /// グループ用と仕事用が両方あっていい。
    ///
    /// **教えてもらわなくても分かる。** マークは共有フォルダの中の1 つ
    /// （`notebook::SHARE_MARK`）で、フォルダと一緒に旅をする ── 受け取った
    /// 人が自分の amber に「これが共有です」と教え直す手が要らない。
    var shares: [Shelf] { sharesBy[rootPath] ?? [] }

    struct Shelf: Identifiable, Equatable {
        let at: String
        let by: String
        var id: String { at }
        var name: String { at.split(separator: "/").last.map(String.init) ?? "すべて" }
    }

    /// このノートは、そのフォルダの中か。
    func inShare(_ at: String, _ note: Note) -> Bool {
        at.isEmpty || note.book == at || note.book.hasPrefix(at + "/")
    }

    /// 共有のフォルダにする（`off` で、やめる）。**フォルダが無ければ作る。**
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
    /// iCloud は中身を消して `.買い物リスト.md.icloud` というラベルを置くので、
    /// 名前が違って一覧に出ない ── 言わないと「ノートが消えた」にしか
    /// 見えないが、待てば戻ってくるだけ。
    @Published var waiting: [String] = []

    /// クラウドが作った控え。ノートとしては一覧に出したまま、ラベルを貼る。
    @Published var clashes: [Note] = []

    /// 落ちてきていないものを、**落としてきてもらう。**
    ///
    /// 待てば来るとはいえ、待つきっかけは要る ── iCloud は「誰かが要ると
    /// 言った」ときに取りに行く。頼むだけで、返事は待たない（来たら
    /// ファイルの見張りが一覧を描き直す）。
    ///
    /// 頼めなくても何も言わない ── iCloud に置いていないフォルダなら
    /// そもそもラベルが出ないし、出たのに頼めないのは向こうの都合で、
    /// 人にできることが何も無い。
    private func fetch(_ rows: [[String: Any]]) {
        for r in rows {
            guard let at = r["at"] as? String else { continue }
            try? FileManager.default
                .startDownloadingUbiquitousItem(at: URL(fileURLWithPath: at))
        }
    }

    /// ノートの中で語が見つかった場所 ── パス → その行番号。
    ///
    /// `notes` と分けてあるのは、答える問いが違うから。一覧はノートの
    /// タイトル・タグ・先頭 100 文字を知っていて、打った瞬間にそれで絞り込む。
    /// こちらはファイルを走査するので遅いが、実際に憶えている文を
    /// 見つけられる。
    @Published var hits: [String: String] = [:]

    private var finding: Task<Void, Never>?

    /// 打つのをやめて少ししてから、ノートの中を `needle` で探す。
    ///
    /// 間引いてあり、途中で止められる ── 打鍵ごとの検索は、5 文字の語で
    /// フォルダを 5 回歩くことになり、捨てられる 4 回ぶんが端末の電池を
    /// 使っている。
    func find(_ needle: String) {
        finding?.cancel()
        let n = needle.trimmingCharacters(in: .whitespaces)
        let roots = places.compactMap { urls[$0.id]?.path }
        guard !roots.isEmpty, n.count >= 2 else {
            hits = [:]
            return
        }
        finding = Task {
            try? await Task.sleep(for: .milliseconds(250))
            if Task.isCancelled { return }
            var found: [String: String] = [:]
            for root in roots {
                guard let answer = try? Cian.call("find", ["path": root, "needle": n]) else { continue }
                if Task.isCancelled { return }
                for h in answer["hits"] as? [[String: Any]] ?? [] {
                    if let p = h["path"] as? String { found[p] = h["text"] as? String ?? "" }
                }
            }
            hits = found
        }
    }

    /// 一覧の並び順。
    ///
    /// 既定は新しい順 ── ノートの一覧は上から読まれ、そこにあるべきなのは
    /// 最後に書いていたもの。名前順は、日付ではなく名前を憶えているときの
    /// ためで、デスクトップ版が出すのと同じ 2 つ。
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

    /// 絞り込みに使っているタグ。打つのではなく押して選ぶ。
    ///
    /// Several at once means **all of them** — the note that is both 仕事 and
    /// 定型. Any-of would grow the list as you press, which is the opposite of
    /// 絞り込みを押すのはそのため。
    @Published var only: Set<String> = []

    /// 絞り込んでいるフォルダ。**タグとは重なり方が違う** ── ノートは一つの
    /// フォルダにしか居ないので、フォルダを「全部」にすると二つ選んだ瞬間に
    /// 必ず 0 件になる。どれかに入っていれば通す（デスクトップ版と同じ）。
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

    /// ここから見えているノートに付いているタグ。よく使う順。
    ///
    /// フォルダ全体ではなく、目の前にあるものから作る ── 40 個並んだ帯は
    /// 誰も読まないし、押す価値があるのはこの山が実際に持っている
    /// タグだから。
    var tagsHere: [String] {
        var count: [String: Int] = [:]
        for n in notes where flat || (here(n) && (n.book == at || at.isEmpty)) {
            for t in n.tags { count[t, default: 0] += 1 }
        }
        return count.keys.sorted {
            count[$0] == count[$1] ? $0 < $1 : count[$0]! > count[$1]!
        }
    }

    /// どのノートブックを開いているか。ルートからの相対パスで、`""` は最上位。
    /// これは*現在地*であって絞り込みではない ── 1 階層のあいだは画面上で
    /// 同じに見えるが、ノートブックの中にノートブックがある瞬間から
    /// 同じではなくなる。
    @Published var at = ""

    /// 出てきたフォルダ。新しいものが末尾。
    ///
    /// **戻る操作を取り消せるように。** 一方向のスワイプが「上へ」で、
    /// 取り消せない操作は、そのうち信用されなくなる ── だから逆向きの
    /// スワイプで、来た道を下りられるようにしてある。
    @Published private(set) var forward: [String] = []

    /// 1 つ上へ。いた場所は憶えておく。
    func leave(for to: String) {
        forward.append(at)
        at = to
    }

    /// 手で選んだフォルダへ入る。それは新しい方向なので、先にあったものは
    /// もう誰の先でもなくなる。
    func into(_ book: String) {
        forward.removeAll()
        at = book
    }

    /// 来た道を下りる。道があれば。
    func back() -> Bool {
        guard let last = forward.popLast() else { return false }
        at = last
        return true
    }

    /// フォルダを無視して、全部を一度に出す。
    ///
    /// ノートの持ち方はどちらも本物。400 件を 1 つの山に入れて検索で
    /// 見つける人もいれば、整理したい人もいる。どちらもアプリが正すべき
    /// 間違いではないので、ここはスイッチにしてある。
    @Published var flat = false

    /// あるノートブックすべてを、ルートからの相対パスで ── 空のものも含む。
    /// だからノートからではなく、エンジンによるディレクトリの走査から
    /// 作っている。
    var allBooks: [String] { booksBy[rootPath] ?? [] }

    /// お気に入りのフォルダすべて。空のものも含む。
    @Published var stars: [String] = []
    /// フォルダのパス → 付けられた色。
    var colors: [String: String] { colorsBy[rootPath] ?? [:] }

    /// 錠のかかったフォルダ（依頼 629）。**上のフォルダの錠も効く** ──
    /// 目印は上に置いてあっても、下のフォルダは錠。
    var locks: [String] { locksBy[rootPath] ?? [] }
    func isLocked(book: String) -> Bool {
        locks.contains { $0.isEmpty || book == $0 || book.hasPrefix($0 + "/") }
    }
    /// その錠の目印があるフォルダ（外すときはここを外す）。
    func lockRoot(of book: String) -> String? {
        locks.first { $0.isEmpty || book == $0 || book.hasPrefix($0 + "/") }
    }

    /// `shelf` の直下にあるフォルダと、それぞれの配下のノート数。
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

    /// ツリーの中で、どのフォルダとお気に入りを開いているか。
    ///
    /// View ではなくストアに置く ── 一覧は打鍵のたび、再読み込みのたびに
    /// 組み直されるので、開閉の状態を View に持たせると、ノートを保存する
    /// たびにフォルダが全部閉じる。
    @Published var unfolded: Set<String> = []

    func opened(_ key: String) -> Binding<Bool> {
        Binding(
            get: { self.unfolded.contains(key) },
            set: { if $0 { self.unfolded.insert(key) } else { self.unfolded.remove(key) } }
        )
    }

    /// 1 階層ずつではなく、ツリーで表示する。
    ///
    /// **既定は切り。** デスクトップ版の左の列は「すべてのノート・ブックマーク・
    /// フォルダ・タグ」を並べて、その右に日付ごとのノートを出す ── iPhone も
    /// 同じ形にした。木は同じものを二度描く（フォルダの段とノートの段）
    /// ので、出すときは上のフォルダの段のほうを引っこめる。
    /// 木が要る人は「フィルタ」から入れる。
    @Published var tree = false

    /// あるフォルダの直下のフォルダ。一覧がどこにいても。
    ///
    /// `books` と同じだが、`at` ではなく渡されたパスから見る ── ツリーは
    /// 開いている階層だけでなく、すべての階層について訊く。
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

    /// あるお気に入りフォルダの直下にあるお気に入り。
    func starred(on shelf: String) -> [Note] {
        sorted(notes.filter { $0.star == shelf })
    }

    /// ノートをお気に入りフォルダに入れる、またはお気に入りから完全に外す。
    ///
    /// 呼び出し側から本文を受け取らず、ファイルを読む ── これは一覧から
    /// 行う操作で、そこではノートを開いているものが無い。本文と一緒に返る
    /// スタンプが保存時の照合に使われるので、星を付ける操作も入力と
    /// 同じ検査を通る。
    func star(_ note: Note, on shelf: String?) throws {
        let (text, stamp) = try open(note)
        var p: [String: Any] = ["text": text]
        if let shelf { p["shelf"] = shelf }
        let out = try Cian.call("star", p)
        _ = try save(note, text: out["text"] as? String ?? text, stamp: stamp)
        reload()
    }

    /// お気に入りフォルダを作る、または配下ごと忘れる。
    func shelf(_ name: String, drop: Bool = false) throws {
        if drop {
            for p in places { if let u = urls[p.id] { _ = try? Cian.call("shelf", ["path": u.path, "name": name, "drop": true]) } }
        } else {
            guard let first = places.first, let u = urls[first.id] else { return }
            _ = try Cian.call("shelf", ["path": u.path, "name": name, "drop": false])
        }
        reload()
    }

    /// あるフォルダの配下すべてにノートがいくつあるか ── 削除が
    /// 何を巻き込むかを、実行前に言う。
    func under(_ book: String) -> Int {
        notes.filter { here($0) && ($0.book == book || $0.book.hasPrefix(book + "/")) }.count
    }

    /// フォルダを改名する。中のノートの名前も中身も変わらない。
    func rename(_ book: String, to name: String) throws {
        guard let root else { return }
        _ = try Cian.call("book", ["path": root.path, "book": book, "name": name])
        // 改名したフォルダの中にいる場合、古いパスはもう存在しない
        // 場所になる ── 何も出さずに、1 つ上へ移る。
        if at == book || at.hasPrefix(book + "/") {
            at = book.split(separator: "/").dropLast().joined(separator: "/")
        }
        reload()
    }

    /// フォルダを中身ごと削除する。**iPhone にはゴミ箱が無い** ── 確認は
    /// これを呼ぶ前に済ませてある。
    func drop(_ book: String) throws {
        guard let root else { return }
        _ = try Cian.call("book", ["path": root.path, "book": book, "drop": true])
        if at == book || at.hasPrefix(book + "/") {
            at = book.split(separator: "/").dropLast().joined(separator: "/")
        }
        reload()
    }

    /// フォルダに色を付ける、または外す。
    func color(_ folder: String, _ hex: String?) throws {
        guard let root else { return }
        _ = try Cian.call("color", ["path": root.path, "folder": folder, "color": hex ?? NSNull()])
        reload()
    }

    /// 開いているノートブックの直下のノートブックと、それぞれの配下に
    /// あるノートの数。
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
            let n = notes.filter { here($0) && ($0.book == full || $0.book.hasPrefix(full + "/")) }.count
            return (String(full.dropFirst(prefix.count)), full, n)
        }
    }

    /// 一覧の上に出す名前。
    var here: String {
        at.isEmpty ? rootName : (at.split(separator: "/").last.map(String.init) ?? at)
    }

    /// 1 つ上の階層。最上位では nil。
    var up: String? {
        guard !at.isEmpty else { return nil }
        let parts = at.split(separator: "/").dropLast()
        return parts.joined(separator: "/")
    }

    /// クエリを語のグループにしたもの ── AND のまとまりを OR でつないだ形。
    ///
    /// **クエリの意味を決めるのは core**（`note::terms`）── 文字列が変わった
    /// ときに一度だけ訊き、ノートごとには訊かない。3 つのフロントエンドが
    /// 「2 語」の意味を別々に決めれば、2 語打たれるまでは一致している
    /// 3 つの検索ボックスになる。
    private var groups: [[Term]] = []

    /// 絞り込みの一語。**どれが見出しでどれが文字かは `note::terms` が決める。**
    ///
    /// `tag:定型` `book:仕事` `title:週報`（`タグ:` `フォルダ:` `題:` も同じ）と
    /// `-` の打ち消し。iPhone が自分で `:` を数えはじめると、デスクトップ版と別のものが
    /// 見つかる検索デスクトップ版が二つできる。
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

    /// ファイル名ではなく、ノートが*何についてのものか*で絞り込む。
    func matching(_ needle: String) -> [Note] {
        let n = needle.trimmingCharacters(in: .whitespaces).lowercased()
        var out = notes
        // 検索はどのノートブックを開いていても全体を見る ── 探している
        // ノートとは、どこに置いたか忘れたノートだから。
        // タグも検索と同じように全体を絞り込む ── ここだけではない。
        // Pressing 「#仕事」 while standing in one folder and being shown only
        // that folder's 仕事 notes is the answer to a question nobody asked.
        if !flat && n.isEmpty && !narrowing { out = out.filter { here($0) && $0.book == at } }
        if !n.isEmpty {
            // どちらでもよい ── 一覧が知っていることか、中から見つかったものか。
            // 1 つのグループの語は全部必要。グループはどれか 1 つ当たればよい。
            out = out.filter { note in
                if hits[note.path] != nil { return true }
                guard !groups.isEmpty else { return note.search.contains(n) }
                return groups.contains { g in g.allSatisfy { hit(note, $0) } }
            }
        }
        if !only.isEmpty { out = out.filter { only.isSubset(of: Set($0.tags)) } }
        // フォルダは「どれか」、タグは「全部」── 重なり方が違うことは
        // 引き出しの中に書いてある（デスクトップ版と同じ）。
        if !onlyBooks.isEmpty {
            out = out.filter { note in
                here(note) && onlyBooks.contains { note.book == $0 || note.book.hasPrefix($0 + "/") }
            }
        }
        if span != nil { out = out.filter(inSpan) }
        // お気に入りはこの上の独立した節に描くので、先頭へ並べ替える
        // のではなくここから外す ── 一度に 2 か所に出るノートは、
        // 2 回消されるノートになる。
        let stuck = Set(pinnedHere(needle).map(\.path))
        return sorted(out.filter { !stuck.contains($0.path) })
    }

    /// 一覧の上に出すお気に入り。
    ///
    /// 最上位では、どこにあろうと**すべての**お気に入りを出す ── それが
    /// お気に入りの意味だから。何度も戻るノートを、探しに行かずに
    /// 手の届くところに置く。フォルダの中ではそのフォルダのものだけ ──
    /// そこでは意図して 1 か所を見ているから。
    func pinnedHere(_ needle: String) -> [Note] {
        guard needle.trimmingCharacters(in: .whitespaces).isEmpty, !flat, !narrowing else { return [] }
        let all = notes.filter { $0.star != nil }
        return sorted(at.isEmpty ? all : all.filter { here($0) && $0.book == at })
    }

    /// 1 つの見出しの下に並ぶノートのまとまり。
    struct Band: Identifiable {
        let name: String
        let notes: [Note]
        var id: String { name }
    }

    /// 一覧を、人が実際に読むまとまりに切ったもの。
    ///
    /// **見出しは並び順に従う。** 名前順に並べながら日付でまとめると
    /// title would put 「今日」 above a note from March, which is worse than
    /// 意味が無いので、名前順なら見出しは頭文字、日付順なら
    /// 日付になる。
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
        // note-2 を note-10 より前に。素の文字列順ではどちらも間違える。
        case .title: out.sort { $0.title.localizedStandardCompare($1.title) == .orderedAscending }
        }
        return out
    }

    /// ノートの本文と、どの時点のものかを言うスタンプ。
    ///
    /// 2 つを一緒に運ぶのは意図的。保存はスタンプを返す必要があり、別に
    /// 訊くことを覚えておかなければならない呼び出し側は、いつか忘れる ──
    /// そして忘れたことは、2 台が同じノートを開く日まで黙っている。
    ///
    func open(_ note: Note) throws -> (String, String) {
        let answer = try Cian.call("read", ["path": note.path])
        return (answer["text"] as? String ?? "", answer["stamp"] as? String ?? "")
    }

    /// ノートを、管理情報と本文に分けたもの。
    ///
    /// 編集側は後半だけを出す ── エンジンの `split` を見よ。front matter が
    /// どこで終わるかはそこで決まるので、iPhone もデスクトップ版も端末版も、
    /// ノートの開始位置について一致する。
    func split(_ text: String) throws -> (String, String) {
        let out = try Cian.call("split", ["text": text])
        return (out["head"] as? String ?? "", out["body"] as? String ?? text)
    }

    /// チェックボックスを 1 つ切り替えたノートを、保存用のテキストとして返す。
    ///
    /// 何番目のチェックボックスかではなく、行番号で指定する ── `note::set_check` を見よ。
    func checked(_ text: String, line: Int, done: Bool) throws -> String {
        let out = try Cian.call("check", ["text": text, "line": line, "done": done])
        return out["text"] as? String ?? text
    }

    /// テキストを色で包んだもの。amber が書くのと同じ形式で。
    ///
    /// ここで書式文字列を組まず、エンジンを通す ── 記法は 1 つの決めごとで、
    /// それを書く場所が 2 つあれば、1 回の編集で記法が 2 つになる。
    /// apart.
    func painted(_ text: String, _ color: String) throws -> String {
        let out = try Cian.call("paint", ["text": text, "color": color])
        return out["text"] as? String ?? text
    }

    /// タグを入れ替えたノート。
    ///
    /// 呼び出し側が保存するためのテキストとして返るので、ほかの編集と
    /// 同じ競合検査を通る。
    func tagged(_ text: String, _ tags: [String]) throws -> String {
        let answer = try Cian.call("settags", ["text": text, "tags": tags])
        return answer["text"] as? String ?? text
    }

    /// フォルダにあるタグすべて。よく使う順 ── 打ち直させるのではなく
    /// 候補として出すため。
    var allTags: [String] {
        var n: [String: Int] = [:]
        for note in notes { for t in note.tags { n[t, default: 0] += 1 } }
        return n.sorted { $0.value > $1.value || ($0.value == $1.value && $0.key < $1.key) }
            .map(\.key)
    }

    /// そのノートが通知してほしいと言っている内容。
    func reminder(of text: String) throws -> Reminder {
        Reminder(try Cian.call("remind", ["text": text]))
    }

    /// front matter のフィールドを 1 つ設定／削除し、保存用のテキストとして返す。
    func field(_ text: String, _ key: String, _ value: String?) throws -> String {
        let out = try Cian.call("setfield", [
            "text": text, "key": key, "value": value ?? NSNull(),
        ])
        return out["text"] as? String ?? text
    }

    /// 繰り返しが溜めているぶんのコピーを作り、作ったことを記録する。
    ///
    /// アプリを開いたときに行う。端末がそれをやらせてくれる唯一の瞬間だから ──
    /// `Bell` を見よ。溜まっていなければ何もしない。たいていはそちら。
    ///
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

    /// ノートの一部または全部の zip。ほかのアプリに渡すため。
    func backup(scope: String, what: String) throws -> URL {
        guard let root else { throw Cian.Failure.engine("保存場所がありません") }
        let r = try Cian.call("backup", [
            "path": root.path, "scope": scope, "what": what,
        ])
        guard let at = r["path"] as? String else { throw Cian.Failure.engine("作れません") }
        return URL(fileURLWithPath: at)
    }

    /// ノートを、描画単位に分けたもの。
    ///
    /// パスではなく本文を渡す。画面に出ているものがそのままプレビューに
    /// 出るように ── まだ保存していない編集も含めて。ディスク上のファイルの
    /// プレビューは、今日のノートを見ているときに昨日のノートを出す。
    func blocks(of text: String) throws -> [Block] {
        let answer = try Cian.call("blocks", ["text": text])
        return (answer["blocks"] as? [[String: Any]] ?? []).map(Block.init)
    }

    enum Saved {
        case ok(stamp: String)
        /// 先に誰かが書いていた。`why` は amber 自身による違いの説明で、
        /// 何も書き込まれていない。
        case conflict(why: String)
    }

    /// 混ざった結果 ── 文字と、どの行が向こうから来たか。
    struct Merged {
        let text: String
        /// 向こうから来た行（ファイルの行・0 起点）。
        let came: [Int]
        /// 同じところを二人が更新して、両方残した行。
        let both: [Int]
        /// 人の目が要るか。
        let eyes: Bool
        /// 同じ行を両方で直したところ（行の中身で）と、前書きのキーのぶつかり。
        let spots: [Desk.Spot]
        let fields: [Desk.Field]

        /// core の答え（`came`・`both`・`spots`・`fields`）を、行の中身で持つ形に。
        static func from(_ got: [String: Any], text: String) -> Merged {
            let rows = Desk.rowsOf(text)
            let slice = { (a: [Any]?) -> [String] in
                guard let a, a.count == 2, let s = (a[0] as? NSNumber)?.intValue, let n = (a[1] as? NSNumber)?.intValue,
                      s >= 0, s + n <= rows.count else { return [] }
                return Array(rows[s..<(s + n)])
            }
            return Merged(
                text: text,
                came: (got["came"] as? [Any] ?? []).compactMap { ($0 as? NSNumber)?.intValue },
                both: (got["both"] as? [Any] ?? []).compactMap { ($0 as? NSNumber)?.intValue },
                eyes: got["eyes"] as? Bool ?? false,
                spots: (got["spots"] as? [[String: Any]] ?? []).map {
                    Desk.Spot(ours: slice($0["ours"] as? [Any]), theirs: slice($0["theirs"] as? [Any]))
                },
                fields: (got["fields"] as? [[String: Any]] ?? []).map {
                    Desk.Field(key: $0["key"] as? String ?? "", ours: $0["ours"] as? String ?? "", theirs: $0["theirs"] as? String ?? "")
                }
            )
        }
    }

    /// 同じノートを二人が更新したとき、**どちらかを捨てずに混ぜる**。
    ///
    /// **判断は core**（`merge`）── デスクトップ版と同じ一組を呼ぶ。二組書けば、同じ
    /// ノートが端末によって別の形に混ざる（フォルダの色を二度書いて
    /// 十一色のうち六色がずれたのと同じ）。ここがするのは、向こうの
    /// いまの中身を読んで渡し、混ざったものを書き戻すことだけ。
    func merge(_ note: Note, was: String, ours: String) throws -> Merged {
        let answer = try Cian.call("read", ["path": note.path])
        // **空が返ってきたら混ぜない。** 読めなかったのか本当に空なのかを
        // 見分けられないまま混ぜると、混ざった結果も空になり、それを
        // そのまま書き戻す（デスクトップ版で一度それでノートを消した）。
        guard let theirs = answer["text"] as? String else {
            throw Cian.Failure.engine("向こうの中身を読めません")
        }
        let got = try Cian.call("merge", ["was": was, "ours": ours, "theirs": theirs])
        let text = got["text"] as? String ?? ""
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
           !(was + ours).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            throw Cian.Failure.engine("混ぜた結果が空になりました")
        }
        return Merged.from(got, text: text)
    }

    /// **題に合わせて改名する**（依頼 502・デスクトップ版と同じ core の `settle`）。改名したら新しい道。
    func settle(_ path: String) -> String? {
        let root = rootOf(path)
        guard !root.isEmpty else { return nil }
        guard let got = try? Cian.call("settle", ["path": root, "note": path]),
              got["renamed"] as? Bool == true, let to = got["path"] as? String else { return nil }
        return to
    }

    /// 時刻の名前のまま残っているノートを、一度だけ題の名前に揃える（決めごと 7）。
    @discardableResult
    func tidyNames() -> Int {
        var n = 0
        for p in places {
            guard let u = urls[p.id] else { continue }
            let got = try? Cian.call("tidynames", ["path": u.path])
            n += (got?["renamed"] as? [[String: Any]])?.count ?? 0
        }
        if n > 0 { reload() }
        return n
    }

    /// 錠（依頼 629）。**答えるのは core** ── 前書きの `locked: true` と、
    /// 上のフォルダの目印（`.amberlock`）の両方を見た答え。
    func lock(of path: String) -> (locked: Bool, why: String, dir: String) {
        guard let a = try? Cian.call("locked", ["path": path]) else { return (false, "", "") }
        return (a["locked"] as? Bool ?? false, a["why"] as? String ?? "", a["dir"] as? String ?? "")
    }

    @discardableResult
    func setLock(path: String, on: Bool) throws -> (locked: Bool, why: String, dir: String) {
        let a = try Cian.call("lock", ["path": path, "on": on])
        return (a["locked"] as? Bool ?? false, a["why"] as? String ?? "", a["dir"] as? String ?? "")
    }

    func save(_ note: Note, text: String, stamp: String, force: Bool = false,
              unlock: Bool = false) throws -> Saved {
        // **書き込む直前の姿を、履歴に渡す。** 一世代にするかどうかを決める
        // のは core（最後の一区切りから間が空いたときだけ）── iPhone とデスクトップ版で
        // 決まりが違うと、片方で消えたものをもう片方が残っていると思う。
        // 履歴が置けないことで、保存が止まる理由はない。
        _ = try? Cian.call("keep", [
            "root": rootOf(note.path), "path": note.path, "gap": 300,
        ])
        var params: [String: Any] = ["path": note.path, "text": text, "force": force]
        // 「今だけ編集する」を押した人のぶんだけ（依頼 629）。
        if unlock { params["unlock"] = true }
        if !stamp.isEmpty { params["stamp"] = stamp }
        let answer = try Cian.call("write", params)
        if answer["conflict"] as? Bool == true {
            return .conflict(why: answer["why"] as? String ?? "開いたあとで更新されています")
        }
        // 保存の三秒後に同期（依頼 500・デスクトップ版の `syncSoon` と同じ）。
        Syncing.shared.soon()
        return .ok(stamp: answer["stamp"] as? String ?? "")
    }

    /// **いまの姿を、一世代として残す。**
    ///
    /// 自動保存だと世代が打キーの切れ目で決まる ── 「ここは残しておきたい」
    /// を人が言えるパスが要る（デスクトップ版の ⌘S と同じ `keep`：間を置かず・マークを付けて）。
    /// マークの付いた世代は数の勘定から外れるので、あとから流れて消えない。
    func keepNow(path: String, text: String) throws -> String {
        let root = rootOf(path)
        guard !root.isEmpty else { return "保存場所がありません" }
        let out = try Cian.call("keep", [
            "root": root, "path": path, "text": text,
            "gap": 0, "force": true, "kept": true,
        ])
        let stamp = out["stamp"] as? String ?? ""
        return stamp.isEmpty
            ? "このバージョンは、もう保護してあります。"
            : "いまのバージョンを保護しました（古くなっても消えません）。"
    }

    /// 画像をノートの隣に置き、そのための Markdown のリンクを返す。
    ///
    /// base64 なのは、それが C の文字列に載るから。バイト列は一度だけ渡り、
    /// ファイルがどこに置かれるかはここでは決めない。
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

    /// 開いているノートブックの中に、ノートブックを作る。
    func makeBook(_ name: String, under: String? = nil) throws {
        guard let root else { return }
        let clean = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !clean.isEmpty else { return }
        let dir = root.appendingPathComponent(under ?? at).appendingPathComponent(clean)
        _ = try Cian.call("mkbook", ["dir": dir.path])
        reload()
    }

    /// ノートを別のノートブックへ移す ── `nil` は最上位。
    func move(_ note: Note, to book: String?) throws {
        guard let root else { return }
        let dir = book.map { root.appendingPathComponent($0) } ?? root
        // **同じ保存ディレクトリの中なら `root` を渡す**（core が画像を連れて行き、
        // 同期に「名前が変わった」と憶えさせる・依頼 496）。別の保存ディレクトリへ
        // 渡るときは渡さない ── 向こうの帳画面に、外のパスを書かせない。
        var p: [String: Any] = ["path": note.path, "dir": dir.path]
        if note.root == root.path { p["root"] = root.path }
        _ = try Cian.call("move", p)
        reload()
    }

    /// 画像の行に、大きさの指示を書く／外す（依頼 420）。**決めるのは core**
    /// ── デスクトップ版と iPhone が別々に文字列をいじると、片方で付けた大きさをもう片方が
    /// 読めない形になる。
    func sized(_ line: String, width: String?) throws -> String {
        var p: [String: Any] = ["line": line]
        if let width { p["width"] = width }
        let got = try Cian.call("imgsize", p)
        return got["line"] as? String ?? line
    }

    /// **型を置くフォルダの名前**（デスクトップ版と同じ一語・依頼 417）。
    ///
    /// 決め打ちにする ── 設定にすると「どこに置けば型になるか」が人に
    /// よって違い、サンプルノートにも書けない。ただのフォルダなので、中の
    /// ノートは一覧にも普通に出るし、開いて直せる。
    static let templates = "テンプレート"

    /// いま置いてある型。**無ければ空** ── フォルダが無いのは普通のこと。
    var stencils: [Note] {
        notes.filter { $0.book == Self.templates || $0.book.hasPrefix(Self.templates + "/") }
    }

    /// サンプルのテンプレート（週報・議事録・買い物リスト）を「テンプレート」フォルダへ
    /// （依頼 506・デスクトップ版と同じ一組 `packaging/templates`）。同じ名前は飛ばす。返すのは置いた数。
    @discardableResult
    func addStencils() -> Int {
        guard let from = Bundle.main.resourceURL?.appendingPathComponent("templates"),
              FileManager.default.fileExists(atPath: from.path), let root
        else { trouble = "サンプルのテンプレートが入っていません"; return 0 }
        let dir = root.appendingPathComponent(Self.templates)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        var put = 0
        for name in ((try? FileManager.default.contentsOfDirectory(atPath: from.path)) ?? []).sorted() where name.hasSuffix(".md") {
            let to = dir.appendingPathComponent(name)
            if FileManager.default.fileExists(atPath: to.path) { continue }
            if (try? FileManager.default.copyItem(at: from.appendingPathComponent(name), to: to)) != nil { put += 1 }
        }
        reload()
        return put
    }

    /// このノートをテンプレートにする ── 「テンプレート」フォルダへ写す（元はそのまま）。
    @discardableResult
    func toStencil(_ note: Note) throws -> String? {
        let root = rootOf(note.path)
        guard !root.isEmpty else { return nil }
        let got = try Cian.call("copy", ["path": note.path, "dir": root + "/" + Self.templates])
        reload()
        return got["path"] as? String
    }

    /// 型から新しいノートを作る。**写す仕組みは「複製」と同じ**（core の
    /// `duplicate`）で、行き先だけが違う ── いちばん上へ置く。
    func fromStencil(_ note: Note) throws -> String? {
        guard let root else { return nil }
        let got = try Cian.call("copy", ["path": note.path, "dir": root.path])
        reload()
        return got["path"] as? String
    }

    /// 同じ中身のノートをもう一つ（依頼 412）。できたほうを返す。
    ///
    /// 何を写して何を写さないかは core が決める（`created` は今日・題は
    /// そのまま）── デスクトップ版と iPhone で別の写しができると、同じ操作の名前で
    /// 別のものが二つの端末に増える。
    @discardableResult
    func duplicate(_ note: Note) throws -> String? {
        let got = try Cian.call("copy", ["path": note.path])
        reload()
        return got["path"] as? String
    }

    /// ルートからの道（`グループ/買い物.md`）。憶えの見出しに使う ── core が
    /// `rel` として返しているのと同じ形。
    private func rel(of note: Note) -> String {
        let name = URL(fileURLWithPath: note.path).lastPathComponent
        return note.book.isEmpty ? name : note.book + "/" + name
    }

    /// 共有をやめたら戻る先。**憶えていなければ nil**（いちばん上へ戻す、
    /// といういままでの形）。
    ///
    /// 憶えていたフォルダが、もう無いことはある（消した・名前を変えた）──
    /// **無いところへは戻さない**。移せずに止まるより、いちばん上へ。
    func home(of note: Note) -> String? {
        guard let was = cameBy[note.root]?[rel(of: note)],
              (booksBy[note.root] ?? []).contains(was) else { return nil }
        return was
    }

    /// 共有の棚へ入れる。**入れる前に居たフォルダを憶える** ── やめたときに
    /// そこへ戻せるように（デスクトップ版と同じ・`.amber/settings.json` の中）。
    func share(_ note: Note, to book: String) throws {
        guard let root else { return }
        let from = note.book
        try move(note, to: book)
        // **憶えるのは移せてから。** 移せなかった回の憶えが残ると、次に
        // やめた人が身に覚えのないフォルダへ連れて行かれる。
        let name = URL(fileURLWithPath: note.path).lastPathComponent
        let now = book.isEmpty ? name : book + "/" + name
        // 憶えられないことで、共有が止まる理由はない。
        if let got = try? Cian.call("came", ["path": root.path, "rel": now, "from": from]) {
            cameBy[root.path] = got["came"] as? [String: String] ?? cameBy[root.path]
        }
    }

    /// 共有をやめる。もといたフォルダへ戻し、**憶えは忘れる** ── 戻した
    /// あとも憶えていると、別のフォルダへ移してからもう一度共有してやめた
    /// 人が、二回前の場所へ連れて行かれる。
    func unshare(_ note: Note) throws {
        guard let root else { return }
        let was = rel(of: note)
        try move(note, to: home(of: note))
        if let got = try? Cian.call("came", ["path": root.path, "rel": was, "forget": true]) {
            cameBy[root.path] = got["came"] as? [String: String] ?? cameBy[root.path]
        }
    }

    /// ノートを削除する。iPhone にはゴミ箱が無いので取り消せない ──
    /// 呼び出し側が先に確認する。
    func remove(_ note: Note) throws {
        _ = try Cian.call("delete", ["path": note.path])
        reload()
    }

    /// 選んだフォルダに新しいノートを作る。名前も形も amber が決める。
    func make(titled title: String, tags: [String] = []) throws -> Note? {
        guard let root else { return nil }
        // 常に最上位ではなく、開いているノートブックの中に作る ──
        // でないと毎回、あとから整理することになる。
        let dir = root.appendingPathComponent(at)
        let made = try Cian.call("new", ["dir": dir.path, "title": title])
        guard let path = made["path"] as? String else { return nil }
        // タグは、書いたばかりのノートを書き直して付ける。`new` に
        // タグを教えるのではなく ── ノートの front matter の形を決めるのは
        // 1 か所で、それは既に `note::set_tags` だから。
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
