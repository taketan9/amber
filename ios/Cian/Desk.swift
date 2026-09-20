import SwiftUI
import PhotosUI

/// 同時に開いているノート。
///
/// **本文はここにあり、View には無い。** `TabView` はスワイプのたびにページを
/// 作っては捨てるので、ページが持っていたものは一緒に消える。そしてページが
/// 持つのは、打ったまま保存していない文字。だからタブはこの画面が持つ状態で、
/// エディタはそこを覗く窓口にしてある。
@MainActor
final class Desk: ObservableObject {
    /// ぶつかった場所 ── こちらの行と向こうの行（どちらかが空のこともある）。
    struct Spot: Equatable {
        var ours: [String]
        var theirs: [String]
    }
    /// 前書きのキーのぶつかり。
    struct Field: Equatable {
        let key: String
        let ours: String
        let theirs: String
    }

    struct Tab: Identifiable, Equatable {
        let note: Note
        /// ノートの自己説明 ── 先頭の `---` のブロック。
        ///
        /// **本文と分けてあるので、編集側には一度も出ない。**
        /// タイトル・日付・タグは amber の管理情報 ── 打っていない人が、自分の
        /// 1 行目に辿り着くためにそこをスクロールする必要は無い。入力では
        /// 変えられず、変えられるのはシートだけで、そちらは `whole` を通る。
        ///
        var head = ""
        /// ノートの本文。エディタが持っているのはこれ。
        var text = ""
        /// 開いたとき、または最後に保存したときのディスク上の状態。
        var stamp = ""
        /// 保存した時点の本文。「変わった」と「開いただけ」を見分けるため。
        var saved = ""
        var reading = true
        /// 錠（依頼 629）。**core が答えたそのまま** ── 前書きの `locked: true` か、
        /// 上のフォルダの目印。デスクトップ版と同じ判断を二度書かない。
        var locked = false
        /// 錠のわけ（`"note"` / `"folder"`）と、そのフォルダの名前。
        var lockWhy = ""
        var lockDir = ""
        /// **今だけ編集する**を押したか。タブを閉じれば消えるので、錠へ戻る。
        var freed = false
        var blocks: [Block] = []
        var loaded = false
        /// 一つ戻すパスと、やり直す道。**デスクトップ版と同じ持ち方**（`gui/renderer.js`
        /// の `backs` / `forwards` / `lastSaved`）。
        ///
        /// UIKit の取り消しでは足りない ── あれは「打った文字」の取り消しで、
        /// 見出しやセルのように**画面を組み直したところで積み木ごと消える**。
        /// しかも「表示」画面（`WKWebView`）にはそもそも届かない。デスクトップ版が
        /// 自前に一本化したのと同じ理由で、ここもノートの姿を積む。
        var backs: [String] = []
        var forwards: [String] = []
        /// 最後に積んだ姿。空は「まだ何も積んでいない」。
        var lastSaved = ""
        /// カーソルの位置（UTF-16 単位）。本文と一緒にこの画面が持つ ── それは
        /// 画面に出ている瞬間ではなくノートに属するものだから。スワイプで離れて
        /// 戻っても、離れたところに居る。
        var pick = NSRange(location: 0, length: 0)
        /// **混ぜるときの土台**（分かれる前）── 開いた時点、または前に
        /// 保存できた時点の中身。動くのは**ファイルと確かに一致した瞬間**
        /// だけ。履歴用の控えを土台に使うと、自動保存が一度でも通ったあと
        /// 「こちらは何も更新していない」ことになり、向こうで丸ごと上書き
        /// される（デスクトップ版で実際にそうなった ── 依頼 355）。
        var base = ""
        /// 向こうから来た行と、両方残した行。**押すまで残る。**
        var came: [Int] = []
        var both: [Int] = []
        var eyes = false
        /// 同じ行を両方で直したところ（依頼 501・デスクトップ版の `incoming.spots` と同じ・
        /// 行の中身で憶える）と、前書きのキーのぶつかり、相手の名前。
        var spots: [Spot] = []
        var fields: [Field] = []
        var who = ""
        var clashing: Bool { !spots.isEmpty || !fields.isEmpty }

        /// 「表示」画面で、いま見ている（打っている）**ファイルの行**。
        /// **画面を替えても同じ場所に居る**ために要る ── 替えたあとでは、
        /// 前の画面の caret も巻き位置も残っていない。まだ分からなければ -1。
        var at = -1

        var id: String { note.path }
        /// 書き出したときのファイルの中身。
        var whole: String { head + text }
        var dirty: Bool { loaded && text != saved }

        /// 編集画面の caret が、**ファイルの何行目**にあるか（前書きを含む）。
        /// core の行番号はファイルの行、この画面が持っているのは本文だけ。
        var lineOfCaret: Int {
            let head2 = head.isEmpty ? 0 : head.components(separatedBy: "\n").count - 1
            let upto = (text as NSString).substring(to: min(pick.location, (text as NSString).length))
            return head2 + upto.components(separatedBy: "\n").count - 1
        }

        static func == (a: Tab, b: Tab) -> Bool { a.id == b.id && a.text == b.text && a.reading == b.reading }
    }

    @Published var tabs: [Tab] = []
    /// 目次から選ばれた行 ── 画面がそこへ滑ったら `nil` に戻す。
    ///
    /// **画面は二つある**（`WKWebView` の「表示」と `UITextView` の「コード」）
    /// ので、飛ぶ先を持つのは desk、飛ぶのはそれぞれの画面。
    @Published var jumping: Int?
    /// どのタブを表示しているか。パスで持つ ── **添え字ではない**。タブを閉じると
    /// それ以降の添え字が全部ずれ、添え字で持った選択は黙って隣のノートを
    /// 指しはじめる。
    @Published var showing: String = ""

    var current: Tab? { tabs.first { $0.id == showing } }

    /// ノートを開く。既に開いていればそこへ戻る。
    /// ノートを机に出す。**押したぶんだけ、タブが増える**（依頼 555）。
    ///
    /// 前は「仮のタブ」があって、一覧から押しただけのものは次を開くと
    /// 入れ替わっていた（依頼 522・VS Code の決まり）── **本人が実際に
    /// 動かして分かりにくいと言ったので撤回した。** 見たものは残る。
    ///
    /// **もう机の上にあるなら、そのタブへ。** 同じノートが二枚並ぶと、
    /// 片方に打った文字がもう片方から見えない。
    /// `store` はもう要らない（差し替えが無くなったので、置いていく
    /// 書きかけも無い）が、呼ぶ側の形は変えない。
    func open(_ note: Note, _ store: NotesStore? = nil, writing: Bool = false) {
        if let at = tabs.firstIndex(where: { $0.id == note.path }) {
            if writing { tabs[at].reading = false }
        } else {
            // **押したぶんだけ、タブが増える**（依頼 555・本人が依頼 522 を撤回）。
            // 前は「仮のタブ」があって、一覧から押しただけのものは次を開くと
            // 入れ替わっていた ── 実際に動かすと分かりにくい（本人）。
            // 置く先はいまのすぐ右（デスクトップ版と同じ）── たどった順と並びが合う。
            let t = Tab(note: note, reading: !writing)
            if let now = tabs.firstIndex(where: { $0.id == showing }) { tabs.insert(t, at: now + 1) } else { tabs.append(t) }
        }
        showing = note.path
    }

    /// このノートより右のものを閉じる。
    func closeRight(of id: String) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        for t in tabs[(at + 1)...].reversed() { close(t.id) }
    }

    /// このノート以外をすべて閉じる。
    func closeOthers(_ id: String) {
        for t in tabs.reversed() where t.id != id { close(t.id) }
    }

    /// タブを 1 つ閉じ、次に何を表示するか決める。
    ///
    /// 左隣にする。そこから来たから ── 端へ飛ぶ閉じ方は、
    /// 居場所を見失わせる。
    /// 棚（閉じるときに名前を揃えるのに要る・依頼 502）。`ContentView` が渡す。
    weak var store: NotesStore?

    func close(_ id: String) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        // 離れるノートの名前を、題に揃えてから（依頼 502）。
        if let store { settle(id, store) }
        // 改名でラベルが差し替わっているので、取り直す。
        guard let at = tabs.firstIndex(where: { $0.id == id }) ?? tabs.firstIndex(where: { $0.id == showing && showing != id }) else { return }
        tabs.remove(at: at)
        if showing == id {
            let next = min(max(0, at - 1), tabs.count - 1)
            showing = tabs.indices.contains(next) ? tabs[next].id : ""
        }
    }

    /// ノートを 1 回だけ読み込む。タブに戻ったときに中身を捨ててはいけない ──
    /// 本文をここに置いている理由がまさにそれ。
    func load(_ id: String, _ store: NotesStore) throws {
        guard let at = tabs.firstIndex(where: { $0.id == id }), !tabs[at].loaded else { return }
        let (text, stamp) = try store.open(tabs[at].note)
        let (head, body) = (try? store.split(text)) ?? ("", text)
        tabs[at].head = head
        tabs[at].text = body
        tabs[at].saved = body
        // **開いた姿を、戻る先の一段目にしておく。** 空のままだと最初の
        // 保存が「積むのではなく憶えるだけ」で終わり、開いてから最初の
        // 一手だけ戻せない（デスクトップ版の `openNote` も同じ場所で同じことをする）。
        tabs[at].lastSaved = body
        tabs[at].backs = []
        tabs[at].forwards = []
        tabs[at].stamp = stamp
        tabs[at].loaded = true
        tabs[at].blocks = (try? store.blocks(of: text)) ?? []
        // 錠（依頼 629）。**開いた時に一度訊く** ── 打つたびに訊くと、
        // 一文字ごとにエンジンを呼ぶことになる。
        let lock = store.lock(of: tabs[at].note.path)
        tabs[at].locked = lock.locked
        tabs[at].lockWhy = lock.why
        tabs[at].lockDir = lock.dir
        // 混ぜるときの土台 ── いまファイルと一致している。
        tabs[at].base = head + body
        // 前に来ていて、まだ確認していないものを思い出す（押すまで残る）。
        recallIncoming(tabs[at].id)
    }

    /// 錠をかける／やめる（依頼 629）。やめたときは、今だけの許しも捨てる。
    func lock(_ id: String, on: Bool, _ store: NotesStore) throws {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        let got = try store.setLock(path: tabs[at].note.path, on: on)
        guard let now = tabs.firstIndex(where: { $0.id == id }) else { return }
        tabs[now].locked = got.locked
        tabs[now].lockWhy = got.why
        tabs[now].lockDir = got.dir
        tabs[now].freed = false
        // 前書きが変わったので、読み直す（`locked: true` の行が増える／減る）。
        if let (text, stamp) = try? store.open(tabs[now].note) {
            let (head, body) = (try? store.split(text)) ?? ("", text)
            tabs[now].head = head
            tabs[now].text = body
            tabs[now].saved = body
            tabs[now].stamp = stamp
            tabs[now].base = head + body
        }
        redraw(id, store)
    }

    /// 錠を訊き直すだけ（**ノートには触らない**）── フォルダの錠を外した
    /// あとに呼ぶ。ここでノートを書き直すと、前書きに何も無いノートの
    /// 更新時刻が動く（同期には「向こうが編集した」に見える）。
    func relock(_ id: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        let got = store.lock(of: tabs[at].note.path)
        tabs[at].locked = got.locked
        tabs[at].lockWhy = got.why
        tabs[at].lockDir = got.dir
        tabs[at].freed = false
    }

    /// **今だけ編集する。** 錠はそのまま ── タブを閉じれば、また錠。
    func freeNow(_ id: String) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        tabs[at].freed = true
    }

    /// ノート全体を分解し直す ── シートがフィールドを変えたあとに。
    func adopt(_ id: String, _ whole: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        let (head, body) = (try? store.split(whole)) ?? ("", whole)
        tabs[at].head = head
        tabs[at].text = body
    }

    /// ブロックは**ノート全体**から作る。チェックボックスの行番号はファイルの
    /// 中の行番号で、`set_check` が受け取るのもそれだから。
    func redraw(_ id: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        tabs[at].blocks = (try? store.blocks(of: tabs[at].whole)) ?? []
    }

    /// 保存して、何が起きたかを返す。`nil` は「することが無い」。
    ///
    /// 競合は throw せず、その文言を返す ── ぶつかったことは失敗ではなく、
    /// 別の端末が先に着いていたということ。呼び出し側はエラーを報告するのでは
    /// なく、人に問わなければならない。
    @discardableResult
    func save(_ id: String, _ store: NotesStore, force: Bool = false) throws -> String? {
        guard let at = tabs.firstIndex(where: { $0.id == id }), tabs[at].loaded else { return nil }
        guard force || tabs[at].dirty else { return nil }
        // **錠のノートは書かない**（依頼 629）── 「今だけ編集する」を押した
        // ぶんだけ、`unlock` を添えて通す。押していなければ core が断る。
        if tabs[at].locked && !tabs[at].freed { return nil }
        // 書き込む直前の姿を積む ── 書いたあとだと、戻る先が「いまの姿」に
        // なる（デスクトップ版の `save()` と同じ場所で同じことをしている）。
        keepStep(at)
        switch try store.save(tabs[at].note, text: tabs[at].whole, stamp: tabs[at].stamp,
                              force: force, unlock: tabs[at].freed) {
        case .ok(let fresh):
            guard let now = tabs.firstIndex(where: { $0.id == id }) else { return nil }
            tabs[now].stamp = fresh
            tabs[now].saved = tabs[now].text
            // 書けた ── ここでファイルと一致したので、土台を進める。
            tabs[now].base = tabs[now].whole
            redraw(id, store)
            store.freshen(tabs[now].note.path)
            return nil
        case .conflict:
            // **どちらかを捨てない。混ぜる。**
            //
            // 前はここで「やめる／それでも上書き」と訊いていた ── どちらを
            // 押しても片方の更新が消える。グループで同じフォルダを触るのが前提の
            // アプリで、それは強すぎる（デスクトップ版と同じ直し・依頼 354）。
            let got = try store.merge(tabs[at].note, was: tabs[at].base, ours: tabs[at].whole)
            guard case .ok(let fresh) = try store.save(
                tabs[at].note, text: got.text, stamp: tabs[at].stamp, force: true,
                unlock: tabs[at].freed
            ) else { return nil }
            guard let now = tabs.firstIndex(where: { $0.id == id }) else { return nil }
            let (head, body) = (try? store.split(got.text)) ?? (tabs[now].head, got.text)
            tabs[now].stamp = fresh
            tabs[now].head = head
            tabs[now].text = body
            tabs[now].saved = body
            tabs[now].base = got.text
            tabs[now].came = got.came
            tabs[now].both = got.both
            tabs[now].eyes = got.eyes
            tabs[now].spots = got.spots
            tabs[now].fields = got.fields
            tabs[now].who = "向こう"
            keepIncoming(tabs[now])
            redraw(id, store)
            store.freshen(tabs[now].note.path)
            return nil
        }
    }

    /// 同期が混ぜたマークを、開いているラベルに付ける（依頼 500・501）。
    func incoming(_ path: String, _ got: NotesStore.Merged, who: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.note.path == path }) else { return }
        tabs[at].loaded = false
        try? load(path, store)
        guard let now = tabs.firstIndex(where: { $0.note.path == path }) else { return }
        tabs[now].came = got.came
        tabs[now].both = got.both
        tabs[now].eyes = got.eyes
        tabs[now].spots = got.spots
        tabs[now].fields = got.fields
        tabs[now].who = who
        keepIncoming(tabs[now])
    }

    // ── ぶつかった場所を選ぶ（依頼 501・デスクトップ版の chooseSpot / chooseField の写し）──

    /// 改行を行に含めたまま、行に割る（core の `records` と同じ割り方）。
    nonisolated static func rowsOf(_ text: String) -> [String] {
        var out: [String] = []
        var cur = ""
        for ch in text {
            cur.append(ch)
            if ch == "\n" { out.append(cur); cur = "" }
        }
        if !cur.isEmpty { out.append(cur) }
        return out
    }
    /// ぶつかった場所を、いまの文字の中で探す（末尾の改行は見ない）。見つからなければ -1。
    nonisolated static func spotAt(_ rows: [String], _ spot: Spot) -> Int {
        let want = spot.ours + spot.theirs
        if want.isEmpty { return -1 }
        let trim = { (s: String) -> String in s.hasSuffix("\n") ? String(s.dropLast()) : s }
        if rows.count < want.count { return -1 }
        for i in 0...(rows.count - want.count) {
            var ok = true
            for k in 0..<want.count where trim(rows[i + k]) != trim(want[k]) { ok = false; break }
            if ok { return i }
        }
        return -1
    }

    /// 文字を丸ごと差し替えて保存する（前書きも含めて）。
    private func putWhole(_ id: String, _ text: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        let (head, body) = (try? store.split(text)) ?? ("", text)
        tabs[at].head = head
        tabs[at].text = body
        _ = try? save(id, store, force: true)
    }

    /// 一つ選ぶ。`which` は ours / theirs / both。
    func chooseSpot(_ id: String, _ n: Int, _ which: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.id == id }), tabs[at].spots.indices.contains(n) else { return }
        let spot = tabs[at].spots[n]
        let rows = Self.rowsOf(tabs[at].whole)
        let found = Self.spotAt(rows, spot)
        if found >= 0, which != "both" {
            let drop: Set<Int> = which == "ours"
                ? Set((0..<spot.theirs.count).map { found + spot.ours.count + $0 })
                : Set((0..<spot.ours.count).map { found + $0 })
            if !drop.isEmpty {
                let kept = rows.enumerated().filter { !drop.contains($0.offset) }.map { $0.element }
                let shift = { (k: Int) -> Int in drop.contains(k) ? -1 : k - drop.filter { $0 < k }.count }
                tabs[at].came = tabs[at].came.map(shift).filter { $0 >= 0 }
                tabs[at].both = tabs[at].both.map(shift).filter { $0 >= 0 }
                tabs[at].spots.remove(at: n)
                putWhole(id, kept.joined(), store)
            }
        } else {
            if found >= 0 {
                let from = found + spot.ours.count
                tabs[at].both = tabs[at].both.filter { $0 < from || $0 >= from + spot.theirs.count }
            }
            tabs[at].spots.remove(at: n)
        }
        settleIncoming(id)
    }

    /// 前書きのキーを選ぶ。タグの「両方」は和集合。
    func chooseField(_ id: String, _ n: Int, _ which: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.id == id }), tabs[at].fields.indices.contains(n) else { return }
        let f = tabs[at].fields[n]
        var value: String? = which == "theirs" ? f.theirs : f.ours
        if which == "both", f.key == "tags" {
            let list = { (v: String) -> [String] in
                v.replacingOccurrences(of: "[", with: "").replacingOccurrences(of: "]", with: "")
                    .split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
            }
            var seen: [String] = []
            for t in list(f.ours) + list(f.theirs) where !seen.contains(t) { seen.append(t) }
            value = "[" + seen.joined(separator: ", ") + "]"
        }
        if value?.isEmpty == true { value = nil }
        if let text = try? store.field(tabs[at].whole, f.key, value) { putWhole(id, text, store) }
        guard let now = tabs.firstIndex(where: { $0.id == id }) else { return }
        tabs[now].fields.remove(at: n)
        settleIncoming(id)
    }

    /// ぜんぶ、こちら（か向こう）で。
    func chooseAll(_ id: String, _ which: String, _ store: NotesStore) {
        while let at = tabs.firstIndex(where: { $0.id == id }), !tabs[at].spots.isEmpty { chooseSpot(id, 0, which, store) }
        while let at = tabs.firstIndex(where: { $0.id == id }), !tabs[at].fields.isEmpty { chooseField(id, 0, which, store) }
    }

    private func settleIncoming(_ id: String) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        if tabs[at].spots.isEmpty && tabs[at].fields.isEmpty {
            tabs[at].eyes = false
            if tabs[at].came.isEmpty { tabs[at].who = "" }
        }
        keepIncoming(tabs[at])
    }

    // ── ファイル名は題に合わせる（依頼 502・デスクトップ版の settleName の写し）──

    /// 離れるときに、そのノートの名前を題に揃える（打ちかけなら触らない）。
    func settle(_ id: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.id == id }), tabs[at].loaded, !tabs[at].dirty else { return }
        guard let to = store.settle(tabs[at].note.path) else { return }
        moved(from: tabs[at].note.path, to: to, store)
        store.reload()
    }

    /// 入ってきたものの控え。**この環境の引き出しに置く** ── 「自分が
    /// 確認したか」は人ごと・環境ごとのことで、フォルダに置くとグループの
    /// 誰かが読んだ時点で全員のぶんが消える。ノートにも書かない。
    private static let seenKey = "amber.incoming"

    private func keepIncoming(_ tab: Tab) {
        var all = UserDefaults.standard.dictionary(forKey: Self.seenKey) ?? [:]
        if tab.came.isEmpty && !tab.clashing {
            all.removeValue(forKey: tab.id)
        } else {
            all[tab.id] = ["came": tab.came, "both": tab.both, "eyes": tab.eyes, "who": tab.who,
                           "spots": tab.spots.map { ["ours": $0.ours, "theirs": $0.theirs] },
                           "fields": tab.fields.map { ["key": $0.key, "ours": $0.ours, "theirs": $0.theirs] }]
        }
        UserDefaults.standard.set(all, forKey: Self.seenKey)
    }

    /// 開いたときに、前に来ていたものを思い出す（押すまで残る）。
    func recallIncoming(_ id: String) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        let all = UserDefaults.standard.dictionary(forKey: Self.seenKey) ?? [:]
        guard let one = all[id] as? [String: Any] else { return }
        tabs[at].came = (one["came"] as? [Int]) ?? []
        tabs[at].both = (one["both"] as? [Int]) ?? []
        tabs[at].eyes = (one["eyes"] as? Bool) ?? false
        tabs[at].who = (one["who"] as? String) ?? ""
        tabs[at].spots = ((one["spots"] as? [[String: Any]]) ?? []).map {
            Spot(ours: $0["ours"] as? [String] ?? [], theirs: $0["theirs"] as? [String] ?? [])
        }
        tabs[at].fields = ((one["fields"] as? [[String: Any]]) ?? []).map {
            Field(key: $0["key"] as? String ?? "", ours: $0["ours"] as? String ?? "", theirs: $0["theirs"] as? String ?? "")
        }
    }

    /// 「確認した」を押された ── マークを消す。
    func seenIncoming(_ id: String) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        tabs[at].came = []
        tabs[at].both = []
        tabs[at].eyes = false
        tabs[at].spots = []
        tabs[at].fields = []
        tabs[at].who = ""
        keepIncoming(tabs[at])
    }

    /// 積める数。デスクトップ版と同じ（`BACKS`）。
    private static let backs = 120

    /// いまの姿を積む。**戻している最中は積まない** ── 積むと、戻った先が
    /// また戻る先になって前へ進めなくなる。
    private var stepping = false

    private func keepStep(_ at: Int) {
        guard !stepping else { return }
        let now = tabs[at].text
        guard now != tabs[at].lastSaved else { return }
        if !tabs[at].lastSaved.isEmpty {
            tabs[at].backs.append(tabs[at].lastSaved)
            if tabs[at].backs.count > Self.backs { tabs[at].backs.removeFirst() }
            // 新しく打ったら、先のパスは消える ── 分かれた先を持っておくと
            // 「やり直し」が何を指すのか誰にも言えなくなる。
            tabs[at].forwards = []
        }
        tabs[at].lastSaved = now
    }

    var canStepBack: Bool { current.map { !$0.backs.isEmpty } ?? false }
    var canStepForward: Bool { current.map { !$0.forwards.isEmpty } ?? false }

    /// 一段もどす／すすめる。**「表示」でも「コード」でも同じ一本。**
    func stepBack(forward: Bool, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.id == showing }) else { return }
        let has = forward ? !tabs[at].forwards.isEmpty : !tabs[at].backs.isEmpty
        guard has else { return }
        if forward {
            tabs[at].backs.append(tabs[at].lastSaved)
            tabs[at].text = tabs[at].forwards.removeLast()
        } else {
            tabs[at].forwards.append(tabs[at].lastSaved)
            tabs[at].text = tabs[at].backs.removeLast()
        }
        tabs[at].lastSaved = tabs[at].text
        stepping = true
        try? save(showing, store, force: true)
        stepping = false
        redraw(showing, store)
    }

    func binding(_ id: String) -> Binding<Tab>? {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return nil }
        return Binding(
            get: { [weak self] in self?.tabs.indices.contains(at) == true ? self!.tabs[at] : Tab(note: Note(["path": id])!) },
            set: { [weak self] new in
                guard let self, let now = self.tabs.firstIndex(where: { $0.id == id }) else { return }
                self.tabs[now] = new
            }
        )
    }
}

/// **iPhone に置かないと決めたもの**（2026-09-06、本人と確認）。
///
/// デスクトップ版を正として揃えるにあたって、揃えないほうがよいものを先に決めた ──
/// 「まだ作っていない」と「作らないことにした」は画面の上では同じ顔を
/// するので、どちらなのかをここに書いておく。
///
/// * **並べて表示**（デスクトップ版の ⌘P）── 393pt で「表示」と「コード」を左右に
///   並べても、どちらも読めない。iPhone は切り替えのまま。**iPad を本気で
///   やるときは欲しい**（本人・2026-09-12）。
/// * **ノートだけを大きく**（F12 と、上の帯の ⤢）── iPhone はもともと全画面。
/// * **前に見たノート／次に見たノート** ── 2026-09-12 に**ウィンドウからも外した**
///   （本人「要らない」）。行き来は上の札（タブ）と「‹ 一覧」で足りる。
/// * **文字数・行数** ── デスクトップ版は帯の右端に出ているが、iPhone の帯には四つで
///   すでに一杯（六つ並べたら iOS が黙って二つ落とした）。
/// * **ショートカット一覧・vim・行番号・コマンド一覧** ── キーボードが無い。
///   命令は ⋯ と設定に名前で並んでいる（本人・2026-09-12「置かないでよい」）。
/// * **チームの予定表（CSV）** ── 合言葉つきの隠しもので、パソコン版だけ
///   （本人・2026-09-12）。CSV は SharePoint の保存ディレクトリに置く予定。
/// * **全部まとめて見る** ── 一覧の頭の「すべてのノート」と同じものだった
///   ので、並び順のメニューからは消した（本人・2026-09-12「同一機能は不要」）。
///   「フォルダごと（ツリー）」はデスクトップ版の左の列にあたるもので、iPhone だけに残す。
///
/// 開いているノートと、その上のタブの帯。
struct DeskView: View {
    @ObservedObject var desk: Desk
    let store: NotesStore
    @StateObject private var pen = Pen()
    @Environment(\.scenePhase) private var phase
    @State private var trouble: String?
    @State private var tagging = false
    @State private var ringing = false
    @State private var picking = false
    @State private var picked: PhotosPickerItem?
    @State private var busy = false
    @State private var tags: [String] = []
    @State private var writing = false
    @State private var saving: Task<Void, Never>?
    @State private var leaving = false
    @State private var tabling = false
    /// ⋯ から開くもの。**一覧まで戻らずに、開いているノートへ。**
    @State private var shelving: Note?
    /// このノート1 つの zip（共有シートに渡す）。
    @State private var zipping: URL?
    @State private var pasting: String?
    @State private var dropping: Note?
    @State private var touring = false
    @State private var kept: String?
    /// 既定で on。off は「いつ書き込むかを自分で決めたい」人のためで、
    /// そのときはボタンが要る ── ほかに書き込むものが無いから。
    ///
    @AppStorage("cian.autosave") private var autosave = true

    private var here: Desk.Tab? { desk.current }

    private var pages: some View {
        VStack(spacing: 0) {
            if desk.tabs.count > 1 { strip }
            // 開いているノートをスワイプで行き来する。ドットは `.never` ──
            // 上の帯が「いくつあって、いまどれか」を既に言っていて、
            // 1 つの問いに答えが 2 つあるのは 1 つ多い。
            TabView(selection: $desk.showing) {
                ForEach(desk.tabs) { tab in
                    if let bound = desk.binding(tab.id) {
                        NoteView(tab: bound, desk: desk, store: store, pen: pen, writing: $writing,
                                 table: { tabling = true }, photo: { picking = true })
                            .tag(tab.id)
                    }
                }
            }
            .tabViewStyle(.page(indexDisplayMode: .never))
        }
    }

    var body: some View {
        wired
            .sheet(isPresented: $tabling) {
                Tabling { body in
                    guard let id = here?.id,
                          let at = desk.tabs.firstIndex(where: { $0.id == id }) else { return }
                    var text = desk.tabs[at].text
                    var pick = desk.tabs[at].pick
                    pen.apply(Marks.block(text, pick, body, caret: 2), to: &text, pick: &pick)
                    desk.tabs[at].text = text
                    desk.tabs[at].pick = pick
                }
            }
            .sheet(item: $shelving) { note in Shelving(store: store, note: note) }
            .sheet(item: $zipping) { at in ActivityView(item: at) }
            .sheet(item: Binding(get: { pasting.map { Past.Which(at: $0, book: false) } },
                                 set: { if $0 == nil { pasting = nil } })) { w in
                Past(store: store, at: w.at, isBook: w.book)
            }
            .sheet(isPresented: $touring) {
                Touring(heads: (here?.blocks ?? []).filter { $0.kind == "heading" }) { line in
                    desk.jumping = line
                }
            }
            .alert("ゴミ箱へ入れますか", isPresented: Binding(
                get: { dropping != nil }, set: { if !$0 { dropping = nil } }
            )) {
                Button("やめる", role: .cancel) {}
                Button("入れる", role: .destructive) { if let n = dropping { remove(n) } }
            } message: {
                Text(dropping.map { "「\($0.shown)」" } ?? "")
            }
            .alert("残しました", isPresented: Binding(
                get: { kept != nil }, set: { if !$0 { kept = nil } }
            )) { Button("閉じる") {} } message: { Text(kept ?? "") }
            .sheet(isPresented: $tagging, onDismiss: applyTags) {
                Tagging(tags: $tags, known: store.allTags)
            }
            .sheet(isPresented: $ringing) {
                if let note = here?.note, let whole = here?.whole {
                    // このシートはノート*全体*を読み書きする ── 通知の設定は
                    // front matter にあり、エディタはそこを持っていない。
                    //
                    Ringing(
                        note: note,
                        text: Binding(
                            get: { whole },
                            set: { desk.adopt(note.path, $0, store) }
                        ),
                        store: store
                    )
                }
            }
            .alert(
                "できません",
                isPresented: Binding(get: { trouble != nil }, set: { if !$0 { trouble = nil } })
            ) { Button("閉じる") {} } message: { Text(trouble ?? "") }
            .alert("保存していません", isPresented: $leaving) {
                Button("保存する") { now() }
                Button("そのままにする", role: .cancel) {}
            } message: {
                Text("自動保存を切っているので、書いたものはまだファイルになっていません。")
            }
    }

    /// 画面の枠まわりと、ノートを書き込み続けるための仕掛け。
    ///
    /// `body` から切り出してあるのは、modifier を長く繋ぐと Swift の型検査が
    /// 音を上げるから、というだけの理由。
    private var wired: some View {
        pages
            .navigationTitle(here?.note.shown ?? "")
            .navigationBarTitleDisplayMode(.inline)
            // **枠まわりはこの画面のもので、ページのものではない。**
            // `TabView` は隣のページを生かしたまま、変化に応じてページを
            // 組み直す。ページが組み立てたツールバーも一緒に組み直され、
            // SwiftUI ではそれが「⋯」が一瞬出て消える形で見える。
            // ここに置けばツールバーは 1 つで、ページが何をしても
            // 組み直されない。
            .toolbar { chrome }
            .photosPicker(isPresented: $picking, selection: $picked, matching: .images)
            .onChange(of: picked) { _, item in if let item { take(item) } }
            .onChange(of: desk.showing) { _, _ in load() }
            .task { load() }
            // 書いているそばから書き込むので、覚えておくことは何も無い。
            // 間引いてある ── 打鍵ごとの保存は、一文で 40 回ファイルを
            // 書き直すことになり、同期フォルダではそれが向こうの端末に
            // 40 回気づかせることになる。
            .onChange(of: here?.text ?? "") { _, _ in if autosave { later() } }
            // 離れる瞬間も保存に値する ── 端末は断りなくアプリを止められる
            // ので、タイマーだけの保存では最後に打ったものが失われる。
            //
            // 自動保存が off でも、離れる瞬間は打ったものを失ってよい
            // 瞬間ではない ── これは保存ではなく、保存を*提案する*最後の
            // 機会。on のときは、これが保存そのものになる。
            .onChange(of: phase) { _, going in if going != .active, autosave { now() } }
            .onDisappear {
                saving?.cancel()
                if autosave { now() } else if here?.dirty == true { leaving = true }
            }
    }

    @ToolbarContentBuilder
    private var chrome: some ToolbarContent {
        if !autosave {
            ToolbarItem(id: "save", placement: .topBarTrailing) {
                Button("保存") { now() }.disabled(here?.dirty != true)
            }
        }
        ToolbarItem(id: "state", placement: .topBarTrailing) {
            // もうボタンではない ── 自分で保存する。ここが言うのは
            // 2 つの状態のどちらかで、書き込まれたかどうかを何も言わない
            // ノートは、安心して離れられないノートだから。
            Group {
                if here?.dirty == true {
                    Label(autosave ? "保存中" : "未保存", systemImage: "circle.fill")
                        .font(.caption2)
                        .foregroundStyle(.orange)
                } else {
                    Label("保存済み", systemImage: "checkmark")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            }
            .labelStyle(.iconOnly)
            .accessibilityLabel(here?.dirty == true ? "保存中" : "保存済み")
        }
        // **一つ戻す／やり直すは、上の帯ではなく下の帯に。**
        //
        // デスクトップ版は歯車の左に置いた（依頼 265）。iPhone でも同じ場所に置いてみたら、
        // iOS が**黙って二つ落とした** ── 題の隣に六つは入らず、消えたのは
        // 「表示／コード」とベルだった。落ちたことはどこにも出ないので、
        // 「無くなった」としか見えない。
        //
        // 下の帯にしたのは幅のためだけではない。**iPhone の親指は下に居る** ──
        // 打ちながら押すものは、打っている手の側にあるほうがいい。帯は
        // 「表示」にも「コード」にも出ているので、保存場所は一つで済む。
        ToolbarItem(id: "read", placement: .topBarTrailing) {
            Button {
                guard let id = here?.id else { return }
                if here?.reading == false { desk.redraw(id, store) }
                guard let n = desk.tabs.firstIndex(where: { $0.id == id }) else { return }
                // **替える前に、どこに居たかを控える。** 替えたあとでは、
                // 前の画面の caret も巻き位置も残っていない ── 替えるたびに
                // 頭へ飛ばされると、そのつど探し直すことになる（デスクトップ版と同じ
                // 直し・依頼 346）。飛ぶ先を持つのは desk、飛ぶのは画面。
                let line = desk.tabs[n].reading
                    ? desk.tabs[n].at
                    : desk.tabs[n].lineOfCaret
                desk.tabs[n].reading.toggle()
                if line >= 0 { desk.jumping = line }
            } label: {
                // **絵で、いま押すと何になるかを言う。**
                //
                // 前は `eye` / `eye.slash` だった ── あれは「隠す／見せる」に
                // 読める。ここで替わるのは**組んだ姿と記号そのもの**で、
                // 隠す話ではない（本人：「めっちゃわかりにくいね」・2026-09-08）。
                //
                // **文字にしない。** いつか端末の言葉に合わせて配るときに、
                // 訳の要らないところを増やしておく ── `</>` は世界のどこでも
                // コードで、`doc.richtext` は組んだ文書。押すと何になるかを
                // 出す（いま「表示」なら `</>`）。
                // 「表示」側は `doc.richtext` を試したが、実機では**「あ」の
                // 入った札**に見えて、文字の入った絵になってしまった（絵で
                // 案内する意味が薄れる）── 行の並んだ `text.alignleft` に
                // する。`</>` と並べたときに「組んだ文書 / 記号そのもの」に
                // 読める。
                Image(systemName: here?.reading == true
                      ? "chevron.left.forwardslash.chevron.right"
                      : "text.alignleft")
            }
            .accessibilityLabel(here?.reading == true ? "コード" : "表示")
        }
        // ベルはメニューではなくバーに置く ── 鳴るかどうかは
        // 何も開かずに*見えて*ほしいもの。あれは状態であり、
        // メニューの奥に隠れた状態は、誰も知らない状態になる。
        //
        ToolbarItem(id: "bell", placement: .topBarTrailing) {
            Button { ringing = true } label: {
                Image(systemName: reminded ? "bell.fill" : "bell")
                    .foregroundStyle(reminded ? AnyShapeStyle(.orange) : AnyShapeStyle(.tint))
            }
            .accessibilityLabel(reminded ? "通知あり" : "通知")
        }
        // **⋯ の顔ぶれは、デスクトップ版の「ノート ▾」と同じ。**
        //
        // ここには「タグ」しか無く、ブックマークもフォルダ移動も履歴も
        // 削除も**一覧まで戻って長押し**するしかなかった ── 開いている
        // ノートに対してすることなのに、開いている画面からは頼めない。
        // 二つの amber で同じ順に並べる。
        ToolbarItem(id: "more", placement: .topBarTrailing) {
            Menu {
                Button { shelving = here?.note } label: {
                    Label(here?.note.star == nil ? "ブックマークに登録する" : "ブックマークグループを変える",
                          systemImage: "star")
                }
                Button { tags = here?.note.tags ?? []; tagging = true } label: {
                    Label("タグ設定", systemImage: "tag")
                }
                Menu {
                    Button("（トップページ）") { moveHere(nil) }
                    ForEach(store.allBooks, id: \.self) { b in
                        Button(b) { moveHere(b) }
                    }
                } label: {
                    Label("フォルダへ移動", systemImage: "folder")
                }
                if let note = here?.note {
                    // デスクトップ版と同じ3 つ（依頼 516）── Markdown はそのまま、HTML は1 つで完結、PDF は刷ったもの。
                    Menu {
                        ShareLink(item: URL(fileURLWithPath: note.path)) {
                            Label("Markdown", systemImage: "doc.plaintext")
                        }
                        Button { export("html") } label: { Label("HTML", systemImage: "doc.richtext") }
                        Button { export("pdf") } label: { Label("PDF", systemImage: "doc") }
                    } label: {
                        Label("エクスポート", systemImage: "square.and.arrow.up")
                    }
                    // デスクトップ版のバックアップの「このノート1 つ」と同じもの（画像も一緒に zip に）。
                    Button {
                        do { zipping = try store.backup(scope: "note", what: note.path) }
                        catch { trouble = error.localizedDescription }
                    } label: {
                        Label("このノートをバックアップ", systemImage: "archivebox")
                    }
                }
                Divider()
                Button { touring = true } label: {
                    Label("目次", systemImage: "list.bullet.indent")
                }
                Button { pasting = here?.note.path } label: {
                    Label("過去バージョン", systemImage: "clock.arrow.circlepath")
                }
                // **いまの姿を、一世代として残す。** 自動保存だと世代が
                // 打キーの切れ目で決まる ── 「ここは残しておきたい」を人が
                // 言えるパスが要る（デスクトップ版の ⌘S と同じもの）。
                Button { keepNow() } label: {
                    Label("いまのバージョンを保護", systemImage: "square.and.arrow.down")
                }
                Divider()
                // 錠（依頼 629）。**開いているノートにすること**なので、
                // 設定ではなくこのメニューに置く（デスクトップ版の「ノート ▾」と同じ場所）。
                if let tab = here {
                    if tab.locked {
                        Button { setLock(false) } label: {
                            Label(tab.lockWhy == "folder" ? "フォルダのロックをやめる" : "ロックをやめる",
                                  systemImage: "lock.open")
                        }
                    } else {
                        Button { setLock(true) } label: {
                            Label("このノートをロックする", systemImage: "lock")
                        }
                    }
                }
                Divider()
                Button(role: .destructive) { dropping = here?.note } label: {
                    Label("ゴミ箱へ入れる", systemImage: "trash")
                }
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .accessibilityLabel("その他")
        }
    }

    private var reminded: Bool {
        guard let text = here?.whole else { return false }
        return (try? store.reminder(of: text)).map { !$0.once.isEmpty || $0.repeats } ?? false
    }

    /// 読める形で書き出す（HTML／PDF）── 出来たら共有シートへ。
    private func export(_ how: String) {
        guard let note = here?.note, let whole = here?.whole else { return }
        do {
            zipping = how == "pdf" ? try Exporting.pdf(note, text: whole) : try Exporting.html(note, text: whole)
        } catch { trouble = error.localizedDescription }
    }

    /// 開いているノートを、別のフォルダへ。
    private func moveHere(_ book: String?) {
        guard let note = here?.note else { return }
        do { try store.move(note, to: book) } catch { trouble = error.localizedDescription }
    }

    /// **いまの姿を、一世代として残す。** 自動保存だと世代が打キーの切れ目で
    /// 決まる ── 「ここは残しておきたい」を人が言えるパスが要る（デスクトップ版の ⌘S）。

    /// 錠をかける／やめる（依頼 629）。**フォルダの錠は、そのフォルダごと。**
    private func setLock(_ on: Bool) {
        guard let tab = here else { return }
        do {
            // フォルダの錠は、目印のあるフォルダを外す ── ノートの前書きを
            // 触っても、上のフォルダの錠は外れない。
            let what = (!on && tab.lockWhy == "folder" && !tab.lockDir.isEmpty)
                ? tab.lockDir : tab.note.path
            if what == tab.note.path {
                try desk.lock(tab.id, on: on, store)
            } else {
                try store.setLock(path: what, on: on)
                desk.relock(tab.id, store)
            }
            store.reload()
        } catch {
            trouble = error.localizedDescription
        }
    }

    private func keepNow() {
        guard let id = here?.id, let whole = here?.whole else { return }
        do {
            try desk.save(id, store)
            kept = try store.keepNow(path: id, text: whole)
        } catch { trouble = error.localizedDescription }
    }

    private func remove(_ note: Note) {
        do {
            try store.remove(note)
            desk.close(note.path)
        } catch { trouble = error.localizedDescription }
    }

    private func load() {
        guard !desk.showing.isEmpty else { return }
        do { try desk.load(desk.showing, store) } catch { trouble = error.localizedDescription }
    }

    /// 少しあとで保存する。その前に入力が来たら、待ち直す。
    private func later() {
        saving?.cancel()
        let id = desk.showing
        saving = Task {
            try? await Task.sleep(for: .milliseconds(900))
            if Task.isCancelled { return }
            write(id)
        }
    }

    /// いますぐ保存する。
    private func now(force: Bool = false) {
        saving?.cancel()
        write(desk.showing, force: force)
    }

    private func write(_ id: String, force: Bool = false) {
        guard !id.isEmpty else { return }
        do {
            try desk.save(id, store, force: force)
        } catch { trouble = error.localizedDescription }
    }

    private func applyTags() {
        guard let id = here?.id, let note = here?.note, tags != note.tags,
              let whole = here?.whole else { return }
        do {
            desk.adopt(id, try store.tagged(whole, tags), store)
            desk.redraw(id, store)
        } catch { trouble = error.localizedDescription }
    }

    /// 画像を先にディスクへ書き、そのあとで本文に入れる ── 逆の順だと、
    /// 来ないかもしれないファイルへのリンクを書くことになる。
    private func take(_ item: PhotosPickerItem) {
        guard let id = here?.id, let note = here?.note else { return }
        busy = true
        Task {
            defer { busy = false; picked = nil }
            do {
                guard let data = try await item.loadTransferable(type: Data.self) else {
                    trouble = "その写真を読めませんでした"
                    return
                }
                let link = try store.attach(data, ext: Self.kind(of: data), to: note)
                guard let at = desk.tabs.firstIndex(where: { $0.id == id }) else { return }
                // 末尾ではなく、居た場所に入れる ── 画像は、手を伸ばした
                // ときに書いていた段落に属する。
                var text = desk.tabs[at].text
                var pick = desk.tabs[at].pick
                pen.apply(Marks.block(text, pick, "![](\(link))\n"), to: &text, pick: &pick)
                desk.tabs[at].text = text
                desk.tabs[at].pick = pick
            } catch {
                trouble = error.localizedDescription
            }
        }
    }

    /// 先頭のバイトが言う画像の形式 ── スクリーンショットは PNG、写真は
    /// たいてい HEIC で、取り違えると何でも開けないファイルが
    /// 残る。
    private static func kind(of data: Data) -> String {
        let b = [UInt8](data.prefix(12))
        if b.count >= 8, b[0] == 0x89, b[1] == 0x50 { return "png" }
        if b.count >= 3, b[0] == 0xFF, b[1] == 0xD8 { return "jpg" }
        if b.count >= 12, b[4] == 0x66, b[5] == 0x74, b[6] == 0x79, b[7] == 0x70 { return "heic" }
        if b.count >= 4, b[0] == 0x47, b[1] == 0x49, b[2] == 0x46 { return "gif" }
        return "png"
    }

    private var strip: some View {
        ScrollViewReader { to in
            HStack(spacing: 0) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    ForEach(desk.tabs) { tab in
                        chip(tab)
                            .id(tab.id)
                    }
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
            }
            tabList
            }
            .background(.bar)
            // 帯の外にあるタブへスワイプしたら、帯も一緒に動かす ──
            // でないと帯と中身が「いまどこか」で食い違う。
            .onChange(of: desk.showing) { _, now in
                withAnimation { to.scrollTo(now, anchor: .center) }
            }
            // **一覧から開いて戻ってきたときも、いま出しているタブを見える所へ**
            // （依頼 625・本人「タブが溢れかえっている時、見切れているタブにアクセスできず
            // 不便だ」）。帯は横にはじけば動くが、開き直すたびに作り直され、そのとき
            // `showing` は変わっていないので上の `onChange` が鳴らない ── 開いたばかりの
            // ノートのタブが、右端の外に居た（シミュレータで見た）。
            .onAppear { to.scrollTo(desk.showing, anchor: .center) }
            .onChange(of: desk.tabs.count) { _, _ in
                withAnimation { to.scrollTo(desk.showing, anchor: .center) }
            }
        }
    }

    /// 開いているノートの一覧（依頼 627・本人「実装してほしいぞ」）。
    ///
    /// 帯ははじけば動くが、何十枚も開くと探すのが遠い ── **帯の右端に、名前で
    /// 選べる一覧を。** デスクトップ版の「N 件 ▾」と同じ役目。いま出しているものに印。
    private var tabList: some View {
        Menu {
            Section("開いているノート（\(desk.tabs.count) 件）") {
                ForEach(desk.tabs) { tab in
                    Button {
                        desk.showing = tab.id
                    } label: {
                        if tab.id == desk.showing {
                            Label(tab.note.shown, systemImage: "checkmark")
                        } else {
                            Text(tab.note.shown)
                        }
                    }
                }
            }
        } label: {
            HStack(spacing: 2) {
                Text("\(desk.tabs.count)").font(.subheadline.monospacedDigit())
                Image(systemName: "chevron.down").font(.caption)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .contentShape(Rectangle())
        }
        .foregroundStyle(Color.accentColor)
        .accessibilityLabel("開いているノートの一覧")
        // **高さは帯に合わせる** ── 縦に伸ばす指定（`maxHeight: .infinity`）を置いたら、
        // 帯が画面の半分まで膨らんで本文が消えた（シミュレータで踏んだ）。
        .overlay(alignment: .leading) { Divider().frame(height: 20) }
    }

    private func chip(_ tab: Desk.Tab) -> some View {
        let on = tab.id == desk.showing
        return HStack(spacing: 4) {
            if tab.dirty {
                // 未保存であることを、閉じようと決めた瞬間に目が行く
                // ただ 1 か所で言う。
                Circle().frame(width: 6, height: 6).foregroundStyle(.orange)
            }
            Text(tab.note.shown).lineLimit(1).font(.subheadline)
            Button {
                desk.close(tab.id)
            } label: {
                Image(systemName: "xmark").font(.caption)
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
            .accessibilityLabel("このノートを閉じる")
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(on ? Color.accentColor.opacity(0.18) : Color.secondary.opacity(0.12),
                    in: Capsule())
        .foregroundStyle(on ? Color.accentColor : Color.primary)
        .onTapGesture { desk.showing = tab.id }
        .contextMenu {
            // 言い方は本人が決めた（2026-09-12）。
            Button { desk.close(tab.id) } label: { Label("このノートを閉じる", systemImage: "xmark") }
            Button { desk.closeRight(of: tab.id) } label: { Label("このノートより右のものを閉じる", systemImage: "arrow.right.to.line") }
            Button { desk.closeOthers(tab.id) } label: { Label("このノート以外をすべて閉じる", systemImage: "rectangle.on.rectangle.slash") }
        }
    }
}
