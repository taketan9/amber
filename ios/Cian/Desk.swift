import SwiftUI
import PhotosUI

/// The notes that are open at once.
///
/// **The text lives here, not in the view.** A `TabView` builds and throws
/// away its pages as you swipe; anything a page held would go with it, and
/// what a page holds is what you have typed and not saved. So a tab is a
/// piece of state on the desk, and the editor is a window onto it.
@MainActor
final class Desk: ObservableObject {
    /// ぶつかった場所 ── こちらの行と向こうの行（どちらかが空のこともある）。
    struct Spot: Equatable {
        var ours: [String]
        var theirs: [String]
    }
    /// 前書きの鍵のぶつかり。
    struct Field: Equatable {
        let key: String
        let ours: String
        let theirs: String
    }

    struct Tab: Identifiable, Equatable {
        let note: Note
        /// How the note describes itself: the `---` block at the top.
        ///
        /// **Kept apart from the text so the writing half never shows it.**
        /// The title, the date and the tags are cian's bookkeeping — a person
        /// who did not type them should not have to scroll past them to reach
        /// their own first line. Typing cannot change it; the sheets can, and
        /// they go through `whole`.
        var head = ""
        /// 仮のタブか（依頼 522・VS Code のプレビュータブ）。一覧から押しただけなら仮で、
        /// 次を開くと入れ替わる。書くか「このタブを残す」で本のタブに。
        var preview = false
        /// What the note says. This is what the editor holds.
        var text = ""
        /// What was on disk when it was opened or last saved.
        var stamp = ""
        /// The text as saved, to tell "changed" from "opened".
        var saved = ""
        var reading = true
        var blocks: [Block] = []
        var loaded = false
        /// 一つ戻す道と、やり直す道。**窓と同じ持ち方**（`gui/renderer.js`
        /// の `backs` / `forwards` / `lastSaved`）。
        ///
        /// UIKit の取り消しでは足りない ── あれは「打った字」の取り消しで、
        /// 見出しや升のように**面を組み直したところで積み木ごと消える**。
        /// しかも「表示」の面（`WKWebView`）にはそもそも届かない。窓が
        /// 自前に一本化したのと同じ理由で、ここもノートの姿を積む。
        var backs: [String] = []
        var forwards: [String] = []
        /// 最後に積んだ姿。空は「まだ何も積んでいない」。
        var lastSaved = ""
        /// Where the cursor is, in UTF-16 units. On the desk with the text
        /// because it belongs to the note, not to the moment on screen: swipe
        /// away and back and you are where you left off.
        var pick = NSRange(location: 0, length: 0)
        /// **混ぜるときの土台**（分かれる前）── 開いた時点、または前に
        /// 保存できた時点の中身。動くのは**ファイルと確かに一致した瞬間**
        /// だけ。履歴用の控えを土台に使うと、自動保存が一度でも通ったあと
        /// 「こちらは何も更新していない」ことになり、向こうで丸ごと上書き
        /// される（窓で実際にそうなった ── 依頼 355）。
        var base = ""
        /// 向こうから来た行と、両方残した行。**押すまで残る。**
        var came: [Int] = []
        var both: [Int] = []
        var eyes = false
        /// 同じ行を両方で直したところ（依頼 501・窓の `incoming.spots` と同じ・
        /// 行の中身で憶える）と、前書きの鍵のぶつかり、相手の名前。
        var spots: [Spot] = []
        var fields: [Field] = []
        var who = ""
        var clashing: Bool { !spots.isEmpty || !fields.isEmpty }

        /// 「表示」の面で、いま見ている（打っている）**ファイルの行**。
        /// **面を替えても同じ場所に居る**ために要る ── 替えたあとでは、
        /// 前の面の caret も巻き位置も残っていない。まだ分からなければ -1。
        var at = -1

        var id: String { note.path }
        /// The file, as it would be written.
        var whole: String { head + text }
        var dirty: Bool { loaded && text != saved }

        /// 書く面の caret が、**ファイルの何行目**にあるか（前書きを含む）。
        /// core の行番号はファイルの行、この面が持っているのは本文だけ。
        var lineOfCaret: Int {
            let head2 = head.isEmpty ? 0 : head.components(separatedBy: "\n").count - 1
            let upto = (text as NSString).substring(to: min(pick.location, (text as NSString).length))
            return head2 + upto.components(separatedBy: "\n").count - 1
        }

        static func == (a: Tab, b: Tab) -> Bool { a.id == b.id && a.text == b.text && a.reading == b.reading }
    }

    @Published var tabs: [Tab] = []
    /// 目次から選ばれた行 ── 面がそこへ滑ったら `nil` に戻す。
    ///
    /// **面は二つある**（`WKWebView` の「表示」と `UITextView` の「コード」）
    /// ので、飛ぶ先を持つのは desk、飛ぶのはそれぞれの面。
    @Published var jumping: Int?
    /// Which tab is showing, by path — **not by index**. Closing a tab shifts
    /// every index after it, and a selection that is an index quietly starts
    /// pointing at the note next door.
    @Published var showing: String = ""

    var current: Tab? { tabs.first { $0.id == showing } }

    /// Open a note, or come back to it if it is already open.
    /// ノートを机に出す。
    ///
    /// **ふつうに押したら、いまのタブを差し替える。** 前は押すたびに
    /// `append` していて、入れ替える道がそもそも無かった ── 一覧を上から
    /// 順に見ただけでタブが溜まる（窓に同じものを持ってきたら、1002 本の
    /// 棚で 1002 タブになる）。増やしたいときだけ `fresh` で増やす
    /// （窓の `⌥` 押しと、長押しの「新しいタブで開く」）。
    ///
    /// **もう机の上にあるなら、そのタブへ。** 同じノートが二枚並ぶと、
    /// 片方に打った字がもう片方から見えない。
    /// `store` は差し替えるときだけ要る（**置いていく書きかけを、先に
    /// 書くため**）。渡さなければ書かない ── 呼ぶ側が既に書いている場合。
    func open(_ note: Note, _ store: NotesStore? = nil,
              writing: Bool = false, fresh: Bool = false, pin: Bool = false) {
        if let at = tabs.firstIndex(where: { $0.id == note.path }) {
            if writing { tabs[at].reading = false }
            if writing || pin { tabs[at].preview = false }
        } else if fresh || pin || writing {
            // 本のタブとして、いまのすぐ右へ（窓と同じ）。
            let t = Tab(note: note, reading: !writing)
            if let now = tabs.firstIndex(where: { $0.id == showing }) { tabs.insert(t, at: now + 1) } else { tabs.append(t) }
        } else {
            // **仮のタブ**（依頼 522）── あれば差し替え、無ければいまの右に一枚。
            // 差し替えるぶんの書きかけは置いていかない（`Tab` ごと捨てるので、
            // ここで書かないと消える）── 書いていれば仮ではなくなっているはずだが、念のため。
            var t = Tab(note: note, reading: !writing)
            t.preview = true
            if let pre = tabs.firstIndex(where: { $0.preview }) {
                if let store, tabs[pre].dirty { _ = try? save(tabs[pre].id, store) }
                if let store { settle(tabs[pre].id, store) }
                if let again = tabs.firstIndex(where: { $0.preview }) { tabs[again] = t } else { tabs.append(t) }
            } else if let now = tabs.firstIndex(where: { $0.id == showing }) {
                tabs.insert(t, at: now + 1)
            } else {
                tabs.append(t)
            }
        }
        showing = note.path
    }

    /// 仮のタブを本のタブに。
    func pin(_ id: String) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        tabs[at].preview = false
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

    /// Close one tab, and choose what to show next.
    ///
    /// The neighbour on the left, because that is where you came from — a
    /// close that jumps to the far end of the row loses your place.
    /// 棚（閉じるときに名前を揃えるのに要る・依頼 502）。`ContentView` が渡す。
    weak var store: NotesStore?

    func close(_ id: String) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        // 離れるノートの名前を、題に揃えてから（依頼 502）。
        if let store { settle(id, store) }
        // 改名で札が差し替わっているので、取り直す。
        guard let at = tabs.firstIndex(where: { $0.id == id }) ?? tabs.firstIndex(where: { $0.id == showing && showing != id }) else { return }
        tabs.remove(at: at)
        if showing == id {
            let next = min(max(0, at - 1), tabs.count - 1)
            showing = tabs.indices.contains(next) ? tabs[next].id : ""
        }
    }

    /// Read a note in, once. Coming back to a tab must not throw away what
    /// is in it — that is the whole reason the text lives here.
    func load(_ id: String, _ store: NotesStore) throws {
        guard let at = tabs.firstIndex(where: { $0.id == id }), !tabs[at].loaded else { return }
        let (text, stamp) = try store.open(tabs[at].note)
        let (head, body) = (try? store.split(text)) ?? ("", text)
        tabs[at].head = head
        tabs[at].text = body
        tabs[at].saved = body
        // **開いた姿を、戻る先の一段目にしておく。** 空のままだと最初の
        // 保存が「積むのではなく憶えるだけ」で終わり、開いてから最初の
        // 一手だけ戻せない（窓の `openNote` も同じ場所で同じことをする）。
        tabs[at].lastSaved = body
        tabs[at].backs = []
        tabs[at].forwards = []
        tabs[at].stamp = stamp
        tabs[at].loaded = true
        tabs[at].blocks = (try? store.blocks(of: text)) ?? []
        // 混ぜるときの土台 ── いまファイルと一致している。
        tabs[at].base = head + body
        // 前に来ていて、まだ確認していないものを思い出す（押すまで残る）。
        recallIncoming(tabs[at].id)
    }

    /// Take a whole note back apart — after a sheet has changed a field.
    func adopt(_ id: String, _ whole: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        let (head, body) = (try? store.split(whole)) ?? ("", whole)
        tabs[at].head = head
        tabs[at].text = body
    }

    /// The blocks are drawn from the **whole** note, because a task's line
    /// number is a line number in the file — that is what `set_check` takes.
    func redraw(_ id: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.id == id }) else { return }
        tabs[at].blocks = (try? store.blocks(of: tabs[at].whole)) ?? []
    }

    /// Save, and say what happened. `nil` is "nothing to do".
    ///
    /// Returns the conflict's words rather than throwing them: a clash is not
    /// a failure, it is the other device having got there first, and the
    /// caller has to ask a question rather than report an error.
    @discardableResult
    func save(_ id: String, _ store: NotesStore, force: Bool = false) throws -> String? {
        guard let at = tabs.firstIndex(where: { $0.id == id }), tabs[at].loaded else { return nil }
        guard force || tabs[at].dirty else { return nil }
        // 書き込む直前の姿を積む ── 書いたあとだと、戻る先が「いまの姿」に
        // なる（窓の `save()` と同じ場所で同じことをしている）。
        keepStep(at)
        switch try store.save(tabs[at].note, text: tabs[at].whole, stamp: tabs[at].stamp, force: force) {
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
            // 押しても片方の更新が消える。グループで同じ棚を触るのが前提の
            // アプリで、それは強すぎる（窓と同じ直し・依頼 354）。
            let got = try store.merge(tabs[at].note, was: tabs[at].base, ours: tabs[at].whole)
            guard case .ok(let fresh) = try store.save(
                tabs[at].note, text: got.text, stamp: tabs[at].stamp, force: true
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

    /// 同期が混ぜた印を、開いている札に付ける（依頼 500・501）。
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

    // ── ぶつかった場所を選ぶ（依頼 501・窓の chooseSpot / chooseField の写し）──

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
    /// ぶつかった場所を、いまの字の中で探す（末尾の改行は見ない）。見つからなければ -1。
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

    /// 字を丸ごと差し替えて保存する（前書きも含めて）。
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

    /// 前書きの鍵を選ぶ。タグの「両方」は和集合。
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

    // ── ファイル名は題に合わせる（依頼 502・窓の settleName の写し）──

    /// 離れるときに、そのノートの名前を題に揃える（打ちかけなら触らない）。
    func settle(_ id: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.id == id }), tabs[at].loaded, !tabs[at].dirty else { return }
        guard let to = store.settle(tabs[at].note.path) else { return }
        moved(from: tabs[at].note.path, to: to, store)
        store.reload()
    }

    /// 入ってきたものの控え。**この機械の引き出しに置く** ── 「自分が
    /// 確認したか」は人ごと・機械ごとのことで、フォルダに置くとグループの
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

    /// 「確認した」を押された ── 印を消す。
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

    /// 積める数。窓と同じ（`BACKS`）。
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
            // 新しく打ったら、先の道は消える ── 分かれた先を持っておくと
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
                var next = new
                // 書いたら本のタブに（依頼 522）。
                if next.preview && (next.text != self.tabs[now].text || next.head != self.tabs[now].head) { next.preview = false }
                self.tabs[now] = next
            }
        )
    }
}

/// **電話に置かないと決めたもの**（2026-09-06、本人と確認）。
///
/// 窓を正として揃えるにあたって、揃えないほうがよいものを先に決めた ──
/// 「まだ作っていない」と「作らないことにした」は画面の上では同じ顔を
/// するので、どちらなのかをここに書いておく。
///
/// * **並べて表示**（窓の ⌘P）── 393pt で「表示」と「コード」を左右に
///   並べても、どちらも読めない。電話は切り替えのまま。**iPad を本気で
///   やるときは欲しい**（本人・2026-09-12）。
/// * **ノートだけを大きく**（F12 と、上の帯の ⤢）── 電話はもともと全画面。
/// * **前に見たノート／次に見たノート** ── 2026-09-12 に**窓からも外した**
///   （本人「要らない」）。行き来は上の札（タブ）と「‹ 一覧」で足りる。
/// * **字数・行数** ── 窓は帯の右端に出ているが、電話の帯には四つで
///   すでに一杯（六つ並べたら iOS が黙って二つ落とした）。
/// * **ショートカット一覧・vim・行番号・コマンド一覧** ── 鍵盤が無い。
///   命令は ⋯ と設定に名前で並んでいる（本人・2026-09-12「置かないでよい」）。
/// * **チームの予定表（CSV）** ── 合言葉つきの隠しもので、パソコン版だけ
///   （本人・2026-09-12）。CSV は SharePoint の保存ディレクトリに置く予定。
/// * **全部まとめて見る** ── 一覧の頭の「すべてのノート」と同じものだった
///   ので、並び順の献立からは消した（本人・2026-09-12「同一機能は不要」）。
///   「フォルダごと（ツリー）」は窓の左の列にあたるもので、電話だけに残す。
///
/// The open notes, with a strip of tabs above them.
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
    /// このノート一枚の zip（共有シートに渡す）。
    @State private var zipping: URL?
    @State private var pasting: String?
    @State private var dropping: Note?
    @State private var touring = false
    @State private var kept: String?
    /// On by default. Off is for people who want to decide when a note is
    /// written — and then the button has to be there, because nothing else
    /// will write it.
    @AppStorage("cian.autosave") private var autosave = true

    private var here: Desk.Tab? { desk.current }

    private var pages: some View {
        VStack(spacing: 0) {
            if desk.tabs.count > 1 { strip }
            // Swipe between the open notes. `.never` for the dots: the strip
            // above already says how many there are and which one this is,
            // and two answers to one question is one too many.
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
                    // The sheet reads and writes the *whole* note — a
                    // reminder lives in the front matter, which the editor
                    // does not hold.
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

    /// The chrome, and the things that keep the note written down.
    ///
    /// Split off from `body` only because one long chain of modifiers is
    /// more than the Swift type-checker will sit through.
    private var wired: some View {
        pages
            .navigationTitle(here?.note.shown ?? "")
            .navigationBarTitleDisplayMode(.inline)
            // **The chrome belongs to the desk, not to the page.** A
            // `TabView` keeps the neighbouring page alive and rebuilds pages
            // as things change, and a toolbar built by a page is rebuilt
            // with it — which SwiftUI shows as a 「⋯」 appearing for an
            // instant and going away. Up here there is one toolbar, and
            // nothing the page does rebuilds it.
            .toolbar { chrome }
            .photosPicker(isPresented: $picking, selection: $picked, matching: .images)
            .onChange(of: picked) { _, item in if let item { take(item) } }
            .onChange(of: desk.showing) { _, _ in load() }
            .task { load() }
            // Written down as you write, so there is nothing to remember to
            // do. Debounced: a save per keystroke is a file rewritten forty
            // times a sentence, and on a synced folder that is forty things
            // for the other device to notice.
            .onChange(of: here?.text ?? "") { _, _ in if autosave { later() } }
            // Leaving is the other moment worth saving at — the phone can
            // stop the app without asking, so a save that only happened on a
            // timer would lose the last thing typed.
            // Even with automatic saving off, leaving is not the moment to
            // lose what was typed — so this is not a save, it is the last
            // chance to *offer* one. With it on, it is the save.
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
            // Not a button any more: it saves itself. This says which of the
            // two states it is in, because a note that says nothing about
            // whether it is written down is a note you cannot walk away from.
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
        // 窓は歯車の左に置いた（依頼 265）。電話でも同じ場所に置いてみたら、
        // iOS が**黙って二つ落とした** ── 題の隣に六つは入らず、消えたのは
        // 「表示／コード」とベルだった。落ちたことはどこにも出ないので、
        // 「無くなった」としか見えない。
        //
        // 下の帯にしたのは幅のためだけではない。**電話の親指は下に居る** ──
        // 打ちながら押すものは、打っている手の側にあるほうがいい。帯は
        // 「表示」にも「コード」にも出ているので、保存場所は一つで済む。
        ToolbarItem(id: "read", placement: .topBarTrailing) {
            Button {
                guard let id = here?.id else { return }
                if here?.reading == false { desk.redraw(id, store) }
                guard let n = desk.tabs.firstIndex(where: { $0.id == id }) else { return }
                // **替える前に、どこに居たかを控える。** 替えたあとでは、
                // 前の面の caret も巻き位置も残っていない ── 替えるたびに
                // 頭へ飛ばされると、そのつど探し直すことになる（窓と同じ
                // 直し・依頼 346）。飛ぶ先を持つのは desk、飛ぶのは面。
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
                // **字にしない。** いつか端末の言葉に合わせて配るときに、
                // 訳の要らないところを増やしておく ── `</>` は世界のどこでも
                // コードで、`doc.richtext` は組んだ文書。押すと何になるかを
                // 出す（いま「表示」なら `</>`）。
                // 「表示」側は `doc.richtext` を試したが、実機では**「あ」の
                // 入った札**に見えて、字の入った絵になってしまった（絵で
                // 案内する意味が薄れる）── 行の並んだ `text.alignleft` に
                // する。`</>` と並べたときに「組んだ文書 / 記号そのもの」に
                // 読める。
                Image(systemName: here?.reading == true
                      ? "chevron.left.forwardslash.chevron.right"
                      : "text.alignleft")
            }
            .accessibilityLabel(here?.reading == true ? "コード" : "表示")
        }
        // The bell on the bar and not in the menu: whether a note is going
        // to ring is something you want to *see* without opening anything —
        // it is a state, and a state hidden behind a menu is a state nobody
        // knows about.
        ToolbarItem(id: "bell", placement: .topBarTrailing) {
            Button { ringing = true } label: {
                Image(systemName: reminded ? "bell.fill" : "bell")
                    .foregroundStyle(reminded ? AnyShapeStyle(.orange) : AnyShapeStyle(.tint))
            }
            .accessibilityLabel(reminded ? "通知あり" : "通知")
        }
        // **⋯ の顔ぶれは、窓の「ノート ▾」と同じ。**
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
                    // 窓と同じ三つ（依頼 516）── Markdown はそのまま、HTML は一枚で完結、PDF は刷ったもの。
                    Menu {
                        ShareLink(item: URL(fileURLWithPath: note.path)) {
                            Label("Markdown", systemImage: "doc.plaintext")
                        }
                        Button { export("html") } label: { Label("HTML", systemImage: "doc.richtext") }
                        Button { export("pdf") } label: { Label("PDF", systemImage: "doc") }
                    } label: {
                        Label("エクスポート", systemImage: "square.and.arrow.up")
                    }
                    // 窓のバックアップの「このノート一枚」と同じもの（絵も一緒に zip に）。
                    Button {
                        do { zipping = try store.backup(scope: "note", what: note.path) }
                        catch { trouble = error.localizedDescription }
                    } label: {
                        Label("このノート一枚をバックアップ", systemImage: "archivebox")
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
                // 打鍵の切れ目で決まる ── 「ここは残しておきたい」を人が
                // 言える道が要る（窓の ⌘S と同じもの）。
                Button { keepNow() } label: {
                    Label("いまのバージョンを保護", systemImage: "square.and.arrow.down")
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

    /// **いまの姿を、一世代として残す。** 自動保存だと世代が打鍵の切れ目で
    /// 決まる ── 「ここは残しておきたい」を人が言える道が要る（窓の ⌘S）。
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

    /// Save in a moment, unless more typing arrives first.
    private func later() {
        saving?.cancel()
        let id = desk.showing
        saving = Task {
            try? await Task.sleep(for: .milliseconds(900))
            if Task.isCancelled { return }
            write(id)
        }
    }

    /// Save right now.
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

    /// The picture goes to disk first, and only then into the text: the other
    /// order writes a link to a file that may never arrive.
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
                // Where you were, not at the end: a picture belongs in the
                // paragraph you were writing when you reached for it.
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

    /// What the first bytes say the picture is — a screenshot is a PNG and a
    /// photo is usually a HEIC, and calling either one the other leaves a
    /// file nothing will open.
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
            VStack(spacing: 0) {
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
            // 仮のタブを出しているときだけ、一言（人の言葉で・依頼 522）。
            if let t = desk.current, t.preview {
                Text("まだ読んでいるだけのタブです。ほかのノートを開くと入れ替わります。書き始めるか、タブを長押しして「このタブを残す」で残ります")
                    .font(.caption2).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 12).padding(.bottom, 5)
            }
            }
            .background(.bar)
            // Swiping to a tab that is off the end of the strip should bring
            // the strip with it, or the two disagree about where you are.
            .onChange(of: desk.showing) { _, now in
                withAnimation { to.scrollTo(now, anchor: .center) }
            }
        }
    }

    private func chip(_ tab: Desk.Tab) -> some View {
        let on = tab.id == desk.showing
        return HStack(spacing: 4) {
            if tab.dirty {
                // Unsaved, said in the one place you are looking when you
                // decide to close something.
                Circle().frame(width: 6, height: 6).foregroundStyle(.orange)
            }
            Text(tab.note.shown).lineLimit(1).font(.subheadline)
                .italic(tab.preview)
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
        // 仮のタブは点線（依頼 522）── 斜体だけでは気づきにくい。
        .overlay(Capsule().strokeBorder(style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
            .foregroundStyle(tab.preview ? Color.secondary.opacity(0.6) : Color.clear))
        .foregroundStyle(on ? Color.accentColor : (tab.preview ? Color.secondary : Color.primary))
        .onTapGesture { desk.showing = tab.id }
        .contextMenu {
            // 言い方は本人が決めた（2026-09-12）。
            if tab.preview {
                Button { desk.pin(tab.id) } label: { Label("このタブを残す", systemImage: "pin") }
            }
            Button { desk.close(tab.id) } label: { Label("このノートを閉じる", systemImage: "xmark") }
            Button { desk.closeRight(of: tab.id) } label: { Label("このノートより右のものを閉じる", systemImage: "arrow.right.to.line") }
            Button { desk.closeOthers(tab.id) } label: { Label("このノート以外をすべて閉じる", systemImage: "rectangle.on.rectangle.slash") }
        }
    }
}
