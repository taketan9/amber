import Foundation

/// amber への唯一の入口。
///
/// アプリがノートについて知っていることは全部 ── タイトル、その下の行、
/// タグ ── ここを通って Rust から返ってくる。**ノートに関することは Swift
/// では何も決めない。** Mac のデスクトップ版も同じ `amber_core::note` を読む。
/// 「何がタイトルか」の実装が 2 つあれば、どちらかに手を入れた瞬間から
/// 答えは 2 つに分かれていく。
enum Cian {
    enum Failure: LocalizedError {
        case engine(String)
        var errorDescription: String? {
            switch self { case .engine(let why): return why }
        }
    }

    /// amber に問い合わせる。`method` と `params` を JSON で渡し、答えも
    /// JSON で返る。
    ///
    /// 答えは C 側で確保されていて、**必ず**返さなければならない。`defer` が
    /// 抜けるすべての経路で返す（throw する経路も含めて）── 末尾に書いて
    /// いないのはそのため。
    static func call(_ method: String, _ params: [String: Any] = [:]) throws -> [String: Any] {
        // **クラウドと同じフォルダを触るときは、一言通す**（PLANS 一）。
        guard let at = coordinated(method, params) else { return try raw(method, params) }
        var answer: Result<[String: Any], Error>!
        var trouble: NSError?
        let hand = NSFileCoordinator()
        let take = { (_: URL) in answer = Result { try raw(method, params) } }
        if writers.contains(method) {
            hand.coordinate(writingItemAt: at, options: .forMerging,
                            error: &trouble, byAccessor: take)
        } else {
            hand.coordinate(readingItemAt: at, options: [],
                            error: &trouble, byAccessor: take)
        }
        // **通せなかったことを、読み書きの失敗にしない。** 作法は同期の
        // ためのもので、手元のフォルダなら通らなくても読める。
        if let answer { return try answer.get() }
        _ = trouble
        return try raw(method, params)
    }

    /// 作法を通す操作と、その相手のファイル。
    ///
    /// **全部に通さない。** 一覧（`notes`）はフォルダを歩くだけで、
    /// あちらを止める意味が無いうえ、毎回まるごと待たされる。通すのは
    /// **一本のノートを読むところと書くところ**だけ。
    ///
    /// 読むほうを通すと、iOS は**まだ降りてきていないファイルを降ろして
    /// から**渡してくれる ── 札（`.名前.md.icloud`）しか無いノートを
    /// 開いたときに、「無い」ではなく中身が返る。
    /// 書くほうを通すと、書いている間だけあちらが止まる。
    private static let writers: Set<String> = ["write", "keep", "syncdown"]
    private static let readers: Set<String> = ["read", "blocks", "html", "oldtext", "syncprint"]

    private static func coordinated(_ method: String, _ params: [String: Any]) -> URL? {
        guard writers.contains(method) || readers.contains(method),
              let path = params["path"] as? String, !path.isEmpty
        else { return nil }
        return URL(fileURLWithPath: path)
    }

    private static func raw(_ method: String, _ params: [String: Any]) throws -> [String: Any] {
        let body = String(
            data: try JSONSerialization.data(withJSONObject: params),
            encoding: .utf8
        ) ?? "{}"
        guard let out = method.withCString({ m in body.withCString { p in amber_call(m, p) } }) else {
            throw Failure.engine("ambər が答えませんでした")
        }
        defer { amber_free(out) }
        let text = String(cString: out)
        guard let obj = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else {
            throw Failure.engine("答えを読めません: \(text)")
        }
        // エラーもほかと同じ 1 つの答え ── 呼び出しが失敗する経路はほかに
        // 無いので、確かめる場所もここだけでよい。
        if let why = obj["error"] as? String { throw Failure.engine(why) }
        return obj
    }
}

/// amber が返すノート 1 件の形。
///
/// 通信形式に対する `Codable` ではなく、JSON から組み立てる素の struct。
/// amber が送るフィールドは増えていくので、知らないキーで失敗する decoder は、
/// Mac 側でフィールドが 1 つ増えた日に iPhone を壊す。
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
    /// いつ始めたか。最後にいつ触ったか、とは別の問いで、人はその両方で
    /// ノートを探す。
    let created: UInt64
    /// 選んだルートから見た、そのノートのディレクトリ ── Inkdrop の言う
    /// ノートブックのこと。ルート直下のノートでは空。
    ///
    /// 一覧が 6 階層まで届くので表示する。これが無いと、名前の同じ 2 件の
    /// called 「打合せ」 in two different months are the same row twice.
    let book: String
    /// どの保存ディレクトリのものか（そのフォルダの道）と、その呼び名（依頼 511）。
    /// `book` は保存ディレクトリからの相対のまま ── 二つの保存ディレクトリの同じ
    /// 「仕事」を分けるのはこちら。
    let root: String
    let place: String
    /// お気に入りかどうかと、どのお気に入りフォルダに入っているか ── `""` は
    /// お気に入りの最上位。**移動ではなく 2 つ目の居場所** ── ノートは書かれた
    /// フォルダに残り、これはそれがほかにどこへ出るかを言う。
    let star: String?
    /// タイトル・`#tags`・本文の冒頭を小文字にしたもの ── **「このノートは何に
    /// 一致するか」に対する amber 自身の答え**なので、ここでの検索は
    /// デスクトップ版と同じノートを見つける。
    let search: String
    /// クラウドが同時に書いたときに作った控えなら、もとのノートの名前と
    /// 誰のものか。**一覧からは消さない** ── 消すと、中身を助け出すパスが
    /// どこにも無くなる。並べたうえでラベルを貼る。
    let clash: Clash?
    /// グループと分けてあるフォルダの中か。**判断は core に一つ** ── デスクトップ版と iPhone で
    /// 二度書くと、片方だけ「共有」のマークが出るノートができる。
    let shared: Bool

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
        root = o["root"] as? String ?? ""
        place = o["place"] as? String ?? ""
        star = o["star"] as? String
        search = o["search"] as? String ?? ""
        shared = o["shared"] as? Bool ?? false
        if let c = o["clash"] as? [String: Any] {
            clash = Clash(of: c["of"] as? String ?? "", by: c["by"] as? String ?? "")
        } else {
            clash = nil
        }
    }
}
