import Foundation

/// The one door to cian.
///
/// Everything the app knows about a note — its title, the line under it, its
/// tags — comes back through here from Rust. **Nothing about notes is decided
/// in Swift.** The window on the Mac reads the same `cian_core::note`, and two
/// implementations of "what is a title" would be two answers that drift apart
/// the first time either is touched.
enum Cian {
    enum Failure: LocalizedError {
        case engine(String)
        var errorDescription: String? {
            switch self { case .engine(let why): return why }
        }
    }

    /// Ask cian something. `method` and `params` go over as JSON and the
    /// answer comes back as JSON.
    ///
    /// The answer is C-allocated and **must** be handed back: `defer` does it
    /// on every path out, including the throwing ones, which is the whole
    /// reason the free is not written at the end.
    static func call(_ method: String, _ params: [String: Any] = [:]) throws -> [String: Any] {
        let body = String(
            data: try JSONSerialization.data(withJSONObject: params),
            encoding: .utf8
        ) ?? "{}"
        guard let out = method.withCString({ m in body.withCString { p in amber_call(m, p) } }) else {
            throw Failure.engine("amber が答えませんでした")
        }
        defer { amber_free(out) }
        let text = String(cString: out)
        guard let obj = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else {
            throw Failure.engine("答えを読めません: \(text)")
        }
        // An error is an answer like any other — there is no second way for a
        // call to fail, so this is the only place that has to be checked.
        if let why = obj["error"] as? String { throw Failure.engine(why) }
        return obj
    }
}

/// One note, as cian describes it.
///
/// A plain struct built from the JSON rather than `Codable` against a wire
/// format: the fields cian sends will grow, and a decoder that fails on an
/// unknown key would turn a new field on the Mac into a broken phone.
struct Note: Identifiable, Hashable {
    let path: String
    /// 題。**空のことがある** ── amber が付けた `2026-09-06 13-07-22` は
    /// 書いた人が一度も言っていない名前なので、core は題として返さない。
    /// 画面に出すときは `shown`（「（タイトルなし）」）を使う。
    let title: String
    /// 画面に出す題。一行書けば、それが題になる。
    var shown: String { title.isEmpty ? "（タイトルなし）" : title }
    let excerpt: String
    let tags: [String]
    let updated: UInt64
    /// When it was started, as against when it was last touched. Two
    /// different questions, and people look for notes by both.
    let created: UInt64
    /// The directory it sits in, relative to the chosen root — a notebook, in
    /// the sense Inkdrop means. Empty for a note at the top.
    ///
    /// Shown because the list reaches six levels down: without it two notes
    /// called 「打合せ」 in two different months are the same row twice.
    let book: String
    /// A favourite, and which favourite shelf it stands on — `""` is the top
    /// of the favourites. **A second place, not a move**: the note stays in
    /// the folder it was written in, and this says where it also shows up.
    let star: String?
    /// Title, `#tags` and the start of the body, lowercased — **cian's own
    /// answer to "what does this note match"**, so a search here finds the
    /// same notes it finds in the window.
    let search: String
    /// クラウドが同時に書いたときに作った控えなら、もとのノートの名前と
    /// 誰のものか。**一覧からは消さない** ── 消すと、中身を助け出す道が
    /// どこにも無くなる。並べたうえで札を貼る。
    let clash: Clash?

    struct Clash: Equatable, Hashable {
        let of: String
        let by: String
    }

    var id: String { path }

    init?(_ o: [String: Any]) {
        guard let path = o["path"] as? String else { return nil }
        self.path = path
        title = o["title"] as? String ?? path
        excerpt = o["excerpt"] as? String ?? ""
        tags = o["tags"] as? [String] ?? []
        updated = o["updated"] as? UInt64 ?? 0
        created = o["created"] as? UInt64 ?? o["updated"] as? UInt64 ?? 0
        book = o["book"] as? String ?? ""
        star = o["star"] as? String
        search = o["search"] as? String ?? ""
        if let c = o["clash"] as? [String: Any] {
            clash = Clash(of: c["of"] as? String ?? "", by: c["by"] as? String ?? "")
        } else {
            clash = nil
        }
    }
}
