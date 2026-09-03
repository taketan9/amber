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
        guard let out = method.withCString({ m in body.withCString { p in cian_call(m, p) } }) else {
            throw Failure.engine("cian が答えませんでした")
        }
        defer { cian_free(out) }
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
    let title: String
    let excerpt: String
    let tags: [String]
    let updated: UInt64
    /// The directory it sits in, relative to the chosen root — a notebook, in
    /// the sense Inkdrop means. Empty for a note at the top.
    ///
    /// Shown because the list reaches six levels down: without it two notes
    /// called 「打合せ」 in two different months are the same row twice.
    let book: String
    /// Title, `#tags` and the start of the body, lowercased — **cian's own
    /// answer to "what does this note match"**, so a search here finds the
    /// same notes it finds in the window.
    let search: String

    var id: String { path }

    init?(_ o: [String: Any]) {
        guard let path = o["path"] as? String else { return nil }
        self.path = path
        title = o["title"] as? String ?? path
        excerpt = o["excerpt"] as? String ?? ""
        tags = o["tags"] as? [String] ?? []
        updated = o["updated"] as? UInt64 ?? 0
        book = o["book"] as? String ?? ""
        search = o["search"] as? String ?? ""
    }
}
