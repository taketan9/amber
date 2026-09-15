#if DEBUG
import Foundation

/// **電話の総ざらい**（依頼 448）── 窓の `scripts/walk.sh` にあたるもの。
///
///     scripts/walk-phone.sh
///
/// 窓の総ざらいは、画素を押していない ── **窓の関数**を押している
/// （`openNote`・`setView`・`toggleToc`…）。電話も同じにする: 画面の下に
/// ある関数（`NotesStore` と `Cian` と `Clipping`）を片端から呼び、
/// **落ちないこと・答えが筋の通った形であること**を見る。
///
/// 画素を押す道（XCUITest）を採らなかったのは、
/// **ボタンの位置が変わるたびに検査が壊れる**から ── 壊れた検査は直されず、
/// そのうち誰も走らせなくなる。ここで見たいのは「押せるか」ではなく
/// 「**押した先が壊れていないか**」で、それは関数の側にある。
///
/// **見えないところ**（画素でしか触れないもの）は、これでは見られない ──
/// 吹き出し・記号の帯・写真選び・共有シート。そこは手で押す（技能書の
/// 「三。電話（手で押す）」）。
///
/// **走査は、アプリの後片づけを肩代わりしない。** 一覧を読み直すのは
/// `NotesStore` の仕事で、ここで気を利かせて `reload()` を呼ぶと、
/// **読み直しを忘れた不具合をこちらが隠す** ── 実際に `remove` から
/// 読み直しを取り除いても、この走査は「落ちたものはありません」と
/// 言った（2026-09-09）。呼ぶのは、アプリの誰も読み直さないところだけ
/// （保存のあと）。
///
/// `--walk` を渡して起こすと、画面の代わりにこれが走り、終わったら
/// 落ちた数を持って自分で終わる。**`#if DEBUG` の中** ── 配るものには
/// 入らない。
@MainActor
enum Walk {
    static var asked: Bool { ProcessInfo.processInfo.arguments.contains("--walk") }

    private static var bad: [(String, String)] = []
    private static var ran = 0

    /// 一つ動かして、落ちなかったか・答えが筋の通った形かを見る。
    ///
    /// 返すのは「よければ nil、悪ければその理由」── 真偽値だと、落第の
    /// ときに何が違ったのかが残らない（窓の総ざらいで学んだ）。
    private static func step(_ name: String, _ body: () throws -> String?) {
        ran += 1
        do {
            if let why = try body() { bad.append((name, why)) }
        } catch {
            bad.append((name, error.localizedDescription))
        }
    }

    private static func step(_ name: String, _ body: () async throws -> String?) async {
        ran += 1
        do {
            if let why = try await body() { bad.append((name, why)) }
        } catch {
            bad.append((name, error.localizedDescription))
        }
    }

    static func run() async {
        let store = NotesStore()
        // **自分のフォルダで走る。** 選ぶ小窓は出せないし、出せたとしても
        // 本人のノートを触ることになる。
        store.useOwn()
        store.reload()

        // ── 一。始まり ──────────────────────────────────
        step("見本が入る") {
            let put = store.addWelcome()
            return store.notes.isEmpty ? "一本も入りませんでした（置いたのは \(put) 本）" : nil
        }
        step("二度目は増えない") {
            let before = store.notes.count
            _ = store.addWelcome()
            return store.notes.count == before ? nil
                : "\(before) 本が \(store.notes.count) 本になりました"
        }

        // ── 二。一本のノート ────────────────────────────
        var made: Note?
        step("ノートを作る") {
            made = try store.make(titled: "総ざらい")
            return made == nil ? "作れませんでした" : nil
        }
        guard let note = made else { return finish() }

        step("開いて、読める") {
            let (text, stamp) = try store.open(note)
            if text.isEmpty { return "中身が空です" }
            if stamp.isEmpty { return "しるしがありません" }
            return nil
        }
        step("打って、保存されて、同じ字が返る") {
            let (text, stamp) = try store.open(note)
            let want = text + "\n打った行。\n"
            guard case .ok = try store.save(note, text: want, stamp: stamp) else {
                return "保存できませんでした"
            }
            let (again, _) = try store.open(note)
            return again == want ? nil : "書いた字と読んだ字が違います"
        }
        step("しるしが古いと、黙って上書きしない") {
            let (text, stamp) = try store.open(note)
            // よそが先に書いた ── これでしるしが変わる。**作り物の
            // しるしを渡さない**（形が違うと、核が黙って見送る）。
            _ = try store.save(note, text: text + "よそから。\n", stamp: "", force: true)
            // こちらは、古いしるしのまま書こうとする。
            if case .ok = try store.save(note, text: text + "こちらから。\n", stamp: stamp) {
                return "古いしるしで書けてしまいました"
            }
            return nil
        }
        step("前書きと本文に分かれる") {
            let (text, _) = try store.open(note)
            let (head, body) = try store.split(text)
            if !head.hasPrefix("---") { return "前書きが前書きに見えません: \(head.prefix(20))" }
            if body.contains("title:") { return "前書きが本文に漏れています" }
            return nil
        }
        step("かたまりに分かれる") {
            let (text, _) = try store.open(note)
            let (_, body) = try store.split(text)
            return try store.blocks(of: body).isEmpty ? "一つも組めません" : nil
        }

        // ── 三。ノートにすることぜんぶ ──────────────────
        step("タグを付ける") {
            let (text, stamp) = try store.open(note)
            let out = try store.tagged(text, ["仕事", "段取り"])
            _ = try store.save(note, text: out, stamp: stamp)
            store.reload()   // 保存は一覧を読み直さない ── ここは走査が読む
            let now = store.notes.first { $0.path == note.path }
            return now?.tags.contains("仕事") == true ? nil : "タグが読み返せません"
        }
        step("チェックを押す") {
            let out = try store.checked("- [ ] 牛乳\n", line: 0, done: true)
            return out.contains("- [x]") ? nil : "チェックが入りません: \(out)"
        }
        step("画像の大きさを変える") {
            let out = try store.sized("![画像](a.png)", width: "200")
            if !out.contains("w:200") { return "大きさが入りません: \(out)" }
            let off = try store.sized(out, width: nil)
            return off.contains("w:") ? "大きさが外れません: \(off)" : nil
        }
        step("絵文字の表がある") {
            let table = try Cian.call("emoji")
            let groups = table["groups"] as? [[String: Any]] ?? []
            let first = table["first"] as? [String] ?? []
            if groups.isEmpty { return "組がありません" }
            if !first.contains("👍") || !first.contains("👌") { return "よく使う二十四に 👍👌 がありません" }
            return nil
        }
        step("★ を付けて、外す") {
            let on = store.notes.first { $0.path == note.path }!
            try store.star(on, on: "あとで")
            let mid = store.notes.first { $0.path == note.path }
            if mid?.star != "あとで" { return "付きません（いまは \(mid?.star ?? "なし")）" }
            try store.star(mid!, on: nil)
            return store.notes.first { $0.path == note.path }?.star == nil
                ? nil : "外れません"
        }
        step("フォルダを作って、移して、戻す") {
            try store.makeBook("段取り")
            let n = store.notes.first { $0.path == note.path }!
            try store.move(n, to: "段取り")
            guard let moved = store.notes.first(where: { $0.title == n.title }) else {
                return "移した先で見つかりません"
            }
            if !moved.path.contains("段取り") { return "移っていません: \(moved.path)" }
            try store.move(moved, to: nil)
            return nil
        }
        step("複製すると、二本になる") {
            let before = store.notes.count
            let n = store.notes.first { $0.title == "総ざらい" }!
            _ = try store.duplicate(n)
            return store.notes.count == before + 1 ? nil
                : "\(before) 本が \(store.notes.count) 本になりました"
        }
        step("探すと、見つかる") {
            let hit = store.matching("総ざらい")
            return hit.isEmpty ? "一本も見つかりません" : nil
        }
        step("いまの姿を残して、読み返せる") {
            let n = store.notes.first { $0.title == "総ざらい" }!
            let (text, _) = try store.open(n)
            _ = try store.keepNow(path: n.path, text: text)
            let past = try Cian.call("history", ["root": store.rootPath, "path": n.path])
            let rows = past["versions"] as? [[String: Any]] ?? []
            return rows.isEmpty ? "一世代も残っていません" : nil
        }
        step("同時に書いても、どちらも消えない") {
            let n = store.notes.first { $0.title == "総ざらい" }!
            let (was, stamp) = try store.open(n)
            // 向こうが先に書いた。
            _ = try store.save(n, text: was + "向こうの行。\n", stamp: stamp, force: true)
            // こちらは古い姿から書こうとする。
            let out = try store.merge(n, was: was, ours: was + "こちらの行。\n")
            if !out.text.contains("向こうの行。") { return "向こうの行が消えました" }
            if !out.text.contains("こちらの行。") { return "こちらの行が消えました" }
            return nil
        }
        step("ゴミ箱へ入れると、一覧から消える") {
            let n = store.notes.first { $0.title == "総ざらい" }!
            try store.remove(n)
            return store.notes.contains { $0.path == n.path }
                ? "まだ一覧にいます" : nil
        }

        // ── 三の二。使われていない画像（依頼 449）────────
        step("使われていない画像だけを数える") {
            guard let root = store.rootURL else { return "保存場所がありません" }
            let pics = root.appendingPathComponent("attachments")
            try FileManager.default.createDirectory(at: pics, withIntermediateDirectories: true)
            let used = pics.appendingPathComponent("走査-1.png")
            let idle = pics.appendingPathComponent("走査-2.png")
            try Data([0x89, 0x50, 0x4E, 0x47]).write(to: used)
            try Data([0x89, 0x50, 0x4E, 0x47, 0x00]).write(to: idle)
            guard let n = try store.make(titled: "画像のノート") else { return "作れませんでした" }
            let (text, stamp) = try store.open(n)
            _ = try store.save(n, text: text + "\n![](attachments/走査-1.png)\n", stamp: stamp)

            let got = try Cian.call("spare", ["path": root.path])
            let rows = (got["pictures"] as? [[String: Any]] ?? [])
                .compactMap { $0["rel"] as? String }
            if rows.contains(where: { $0.contains("走査-1") }) {
                return "使っている画像が出ています"
            }
            if !rows.contains(where: { $0.contains("走査-2") }) {
                return "使っていない画像が出ていません"
            }
            if let unsure = got["unsure"] as? [String], !unsure.isEmpty {
                return "読めないノートがあります: \(unsure.joined(separator: "・"))"
            }
            // 消せることも見る ── 数えるだけでは、片づけにならない。
            let at = (got["pictures"] as? [[String: Any]] ?? [])
                .first { ($0["rel"] as? String ?? "").contains("走査-2") }?["path"] as? String
            _ = try Cian.call("delete", ["path": at ?? ""])
            return FileManager.default.fileExists(atPath: idle.path)
                ? "消えていません" : nil
        }

        // ── 三の三。カレンダー（依頼 454）────────────────
        step("カレンダーに、一度きりと繰り返しが並ぶ") {
            guard let n = try store.make(titled: "面談") else { return "作れませんでした" }
            let (text, stamp) = try store.open(n)
            let one = try Cian.call("setfield", [
                "text": text, "key": "remind", "value": "2026-09-09 14:00",
            ])
            _ = try store.save(n, text: one["text"] as? String ?? text, stamp: stamp)

            guard let w = try store.make(titled: "週報") else { return "作れませんでした" }
            let (wt, ws) = try store.open(w)
            let many = try Cian.call("setfield", [
                "text": wt, "key": "repeat", "value": "weekly wed 09:00",
            ])
            _ = try store.save(w, text: many["text"] as? String ?? wt, stamp: ws)

            let got = try Cian.call("month", [
                "path": store.rootPath, "year": 2026, "month": 9,
            ])
            let rows = got["days"] as? [[String: Any]] ?? []
            let once = rows.filter { ($0["kind"] as? String) == "once" && ($0["title"] as? String) == "面談" }
            let rep = rows.filter { ($0["kind"] as? String) == "repeat" && ($0["title"] as? String) == "週報" }
            if once.count != 1 { return "一度きりが \(once.count) 件です" }
            if rep.count != 5 { return "毎週水曜が \(rep.count) 回です" }
            if (once[0]["at"] as? String) != "14:00" { return "時刻が違います" }
            return nil
        }

        // ── 三の四。よその予定表（依頼 456）──────────────
        if let site = ProcessInfo.processInfo.environment["SITE"],
           let ics = Clipping.reach(site + "away.ics") {
            await step("よその予定表を読む") {
                let text = try await Away.fetch(ics)
                let name = try Away.check(text)
                if name != "家の予定" { return "名前が \(name) です" }
                Away.feeds = [Away.Feed(url: ics.absoluteString, name: name)]

                let got = await Away.month(2026, 9)
                let trip = got.filter { $0.title == "旅行" }.map(\.day)
                let bin = got.filter { $0.title == "ごみ出し" }
                if trip.count != 3 { return "終日でまたぐものが \(trip.count) 日です" }
                if trip.first != "2026-09-21" { return "またぐ初日が \(trip.first ?? "") です" }
                if bin.count != 4 { return "毎週月曜が \(bin.count) 回です" }
                guard let one = got.first(where: { $0.title == "歯医者" }) else {
                    return "時刻つきのものが出ていません"
                }
                if one.at != "18:30" { return "時刻が \(one.at ?? "なし") です" }
                if one.place != "駅前" { return "場所が読めていません" }
                if !one.isAway { return "よそのものだと分かりません" }
                Away.feeds = []
                return nil
            }
        }

        // ── 三の五。この iPhone の予定表（依頼 460）──────
        //
        // **読むだけでなく、書けること。** よその予定表（iCal）との違いは
        // そこ一つなので、足す・直す・消すを一周してみる。
        if Phone.allowed {
            step("iPhone の予定表に、足して・直して・消せる") {
                let day = "2026-09-11"
                let name = "走査の予定 \(Int(Date().timeIntervalSince1970))"
                try Phone.add(title: name, day: day, at: "11:00")

                var got = Phone.month(2026, 9)
                guard let made = got.first(where: { $0.title == name }) else {
                    return "足したものが出てきません"
                }
                if made.day != day { return "日が \(made.day) です" }
                if made.at != "11:00" { return "時刻が \(made.at ?? "なし") です" }
                if !made.isPhone { return "この iPhone のものだと分かりません" }

                try Phone.rename(made.path, to: name + "・直した")
                got = Phone.month(2026, 9)
                if !got.contains(where: { $0.title == name + "・直した" }) {
                    return "直したのに、タイトルが変わっていません"
                }

                try Phone.drop(made.path)
                got = Phone.month(2026, 9)
                if got.contains(where: { $0.title.hasPrefix(name) }) {
                    return "消したのに、まだあります"
                }
                return nil
            }
        } else {
            step("iPhone の予定表") { "許可されていません（走査の前に grant していますか）" }
        }

        // ── 四。よそから来るもの ────────────────────────
        if let site = ProcessInfo.processInfo.environment["SITE"],
           let url = Clipping.reach(site) {
            await step("Web からインポート") {
                let hand = Clipping()
                await hand.warm()
                let got = try await hand.clip(url)
                if got.title.isEmpty { return "タイトルが取れません" }
                if !got.body.contains("出典:") { return "出どころの行がありません" }
                return nil
            }
        }

        // ── 四の二。同期（偽の Drive を相手に・依頼 500） ────
        if let drive = ProcessInfo.processInfo.environment["AMBER_DRIVE_URL"] {
            await syncWalk(store, drive)
        }

        // ── 四の三。保存ディレクトリを二つ（依頼 511） ────
        await placeWalk(store, ProcessInfo.processInfo.environment["AMBER_DRIVE_URL"])

        // ── 四の四。カレンダー表示設定と、読める形の書き出し（依頼 515・516） ────
        step("カレンダー表示設定：隠した予定表は見えなくなり、戻せば見える") {
            let was = CalPrefs.hide
            defer { CalPrefs.hide = was }
            let me = Calendaring.Slot(day: "2026-09-01", at: nil, title: "t", path: "/x.md", kind: "note")
            let away = Calendaring.Slot(day: "2026-09-01", at: nil, title: "t", path: "", kind: "away", place: "", from: "会社")
            CalPrefs.hide = []
            if !CalPrefs.visible(me) || !CalPrefs.visible(away) { return "隠していないのに見えません" }
            CalPrefs.hide = ["away:会社"]
            if CalPrefs.visible(away) { return "隠したのに見えます" }
            if !CalPrefs.visible(me) { return "別のものまで隠れました" }
            if !CalPrefs.sources().contains(where: { $0.key == "away:会社" }) { return "隠したものが一覧に無く、戻せません" }
            return nil
        }
        step("カレンダー表示設定：土日と個人カレンダーの色を憶える") {
            let (w, c) = (CalPrefs.weekend, CalPrefs.hereColor)
            defer { CalPrefs.weekend = w; CalPrefs.hereColor = c }
            CalPrefs.weekend = false
            CalPrefs.hereColor = "#c0392b"
            if CalPrefs.weekend { return "土日が戻っています" }
            if CalPrefs.colorName(CalPrefs.hereColor) != "カーマイン" { return "色の名前が \(CalPrefs.colorName(CalPrefs.hereColor))" }
            return nil
        }
        step("カレンダー：週は月曜から、日は一日ずつ") {
            if Calendaring.shift("2026-09-12", 7) != "2026-09-19" { return "七日先が違います" }
            if Calendaring.weekName("2026-09-12") != "土" { return "曜日が \(Calendaring.weekName("2026-09-12"))" }
            if Calendaring.shift("2026-09-30", 1) != "2026-10-01" { return "月を跨げません" }
            return nil
        }
        step("ほかの場所のノートを開く：一覧に入れずに読める") {
            let at = FileManager.default.temporaryDirectory.appendingPathComponent("よそ-\(Int(Date().timeIntervalSince1970)).md")
            try "# よその一枚\n\n一覧には入らない。\n".write(to: at, atomically: true, encoding: .utf8)
            defer { try? FileManager.default.removeItem(at: at) }
            guard let n = store.openOutside(at) else { return "開けません: \(store.trouble ?? "")" }
            if n.title != "よその一枚" { return "タイトルが \(n.title)" }
            if !store.isOutside(n.path) { return "一時的に開いている印が付いていません" }
            if store.notes.contains(where: { $0.path == at.path }) { return "一覧に入っています" }
            return nil
        }
        step("エクスポート：HTML は一枚で完結し、PDF は頁になる") {
            guard let note = store.notes.first(where: { $0.title == "ストラテジーパターン" }) ?? store.notes.first else { return "ノートがありません" }
            let (text, _) = try store.open(note)
            let html = try Exporting.html(note, text: text)
            let page = try String(contentsOf: html, encoding: .utf8)
            if !page.contains("<!doctype html") || !page.contains(note.shown) { return "HTML の形になっていません" }
            let pdf = try Exporting.pdf(note, text: text)
            let bytes = try Data(contentsOf: pdf)
            if bytes.count < 1000 || String(decoding: bytes.prefix(5), as: UTF8.self) != "%PDF-" { return "PDF になっていません（\(bytes.count) bytes）" }
            return nil
        }

        // ── 四の五。タブ（依頼 555・本人が依頼 522 の仮のタブを撤回） ────
        do {
            let desk = Desk()
            desk.store = store
            step("タブ：押したぶんだけ増える（入れ替わらない）") {
                guard store.notes.count >= 3 else { return "ノートが足りません" }
                let (a, b, c) = (store.notes[0], store.notes[1], store.notes[2])
                desk.open(a, store)
                desk.open(b, store)
                if desk.tabs.count != 2 { return "二本目で \(desk.tabs.count) 枚です" }
                desk.open(c, store)
                if desk.tabs.count != 3 { return "三本目で \(desk.tabs.count) 枚です（入れ替わっています）" }
                if desk.showing != c.path { return "三本目が出ていません" }
                return nil
            }
            step("タブ：同じノートをもう一度押しても増えない") {
                guard store.notes.count >= 3 else { return "ノートが足りません" }
                desk.open(store.notes[1], store)
                if desk.tabs.count != 3 { return "同じ一本で増えました（\(desk.tabs.count) 枚）" }
                if desk.showing != store.notes[1].path { return "そのタブに移っていません" }
                guard let id = desk.current?.id else { return "タブが無い" }
                desk.closeOthers(id)
                return desk.tabs.count == 1 ? nil : "ほかを閉じられません（\(desk.tabs.count)）"
            }
        }

        // ── 五。「表示」の面の網（位置 × 操作・`Mesh`） ────
        let grid = await Mesh.run()
        ran += grid.ran
        bad.append(contentsOf: grid.bad)

        finish()
    }

    /// 偽の Drive（`scripts/fake-drive.js`）に、上げて・下ろして・改名を写して・消す。
    /// 向こうの端末は `/_put` `/_move` `/_trash` で演じる（窓の `walk-sync.mjs` と同じ）。
    private static func syncWalk(_ store: NotesStore, _ drive: String) async {
        func talk(_ path: String, _ body: [String: Any]? = nil) async -> [String: Any] {
            var req = URLRequest(url: URL(string: drive + path)!)
            if let body {
                req.httpMethod = "POST"
                req.httpBody = try? JSONSerialization.data(withJSONObject: body)
            }
            guard let (data, _) = try? await URLSession.shared.data(for: req) else { return [:] }
            if let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] { return obj }
            if let arr = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] { return ["list": arr] }
            return [:]
        }
        func rels() async -> [String] {
            let got = await talk("/_list")
            return (got["list"] as? [[String: Any]] ?? []).compactMap { ($0["appProperties"] as? [String: Any])?["rel"] as? String }
        }
        _ = await talk("/_reset", [:])
        let sync = Syncing.shared
        sync.store = store
        sync.auto = false
        sync.load()
        await step("同期：サインイン済みに見える") { sync.signedIn ? nil : "サインインしていないことになっています" }
        await step("同期：一度目でこちらのノートがぜんぶ上がる") {
            guard let r = await sync.now("手") else { return "運びませんでした" }
            if let t = r.trouble.first { return "困りごと: " + t }
            return r.up >= store.notes.count ? nil : "\(r.up) 本しか上がりません（\(store.notes.count) 本のはず）"
        }
        await step("同期：向こうに同じ道で並ぶ") {
            let there = await rels()
            return there.contains("ambər へようこそ.md") ? nil : "向こうの一覧: " + there.prefix(6).joined(separator: " / ")
        }
        await step("同期：二度目は何も運ばない") {
            guard let r = await sync.now("手") else { return "運びませんでした" }
            return r.up == 0 && r.down == 0 && r.clash == 0 ? nil : "\(r.up)/\(r.down)/\(r.clash)"
        }
        _ = await talk("/_put", ["rel": "太郎から.md", "text": "---\ncreated: 2026-09-12\n---\n\n# 太郎から\n\nMac で書いた。\n", "by": "太郎の Mac"])
        await step("同期：向こうが置いたノートが、こちらに来る") {
            guard let r = await sync.now("手") else { return "運びませんでした" }
            if r.down != 1 { return "下りたのが \(r.down) 本" }
            return store.notes.contains { $0.title == "太郎から" } ? nil : "一覧に出ません"
        }
        await step("同期：こちらで直すと、向こうに上がる") {
            guard let note = store.notes.first(where: { $0.title == "太郎から" }) else { return "太郎から がありません" }
            let (text, stamp) = try store.open(note)
            _ = try store.save(note, text: text + "\niPhone で足した。\n", stamp: stamp)
            guard let r = await sync.now("手") else { return "運びませんでした" }
            if r.up != 1 { return "上がったのが \(r.up) 本" }
            let got = await talk("/_get?rel=" + ("太郎から.md".addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? ""))
            return (got["text"] as? String ?? "").contains("iPhone で足した。") ? nil : "向こうの字が古いままです"
        }
        _ = await talk("/_move", ["rel": "太郎から.md", "to": "太郎のメモ.md"])
        await step("同期：向こうで名前が変わると、こちらのファイルも変わる") {
            guard let r = await sync.now("手") else { return "運びませんでした" }
            if r.moved != 1 { return "改名が \(r.moved) 本（\(r.trouble.joined(separator: " / ")))" }
            let here = store.notes.map { $0.path.split(separator: "/").last.map(String.init) ?? "" }
            if here.contains("太郎から.md") { return "古い名前が残っています" }
            return here.contains("太郎のメモ.md") ? nil : "新しい名前がありません: " + here.prefix(6).joined(separator: " / ")
        }
        // 同じ行を両方で直した → 両方残って、選び口（依頼 501）。
        let desk = Desk()
        desk.store = store
        Syncing.shared.desk = desk
        await step("同期：同じ行を両方で直すと、両方残って選び口が出る") {
            guard let note = store.notes.first(where: { $0.path.hasSuffix("/太郎のメモ.md") }) else { return "太郎のメモ.md がありません" }
            desk.open(note, store, writing: true)
            try desk.load(note.path, store)
            guard let at = desk.tabs.firstIndex(where: { $0.id == note.path }) else { return "札がありません" }
            desk.tabs[at].text = desk.tabs[at].text.replacingOccurrences(of: "iPhone で足した。", with: "iPhone で足した（こちらは十個）。")
            _ = try desk.save(note.path, store)
            _ = await sync.now("手")
            let there = await talk("/_get?rel=" + ("太郎のメモ.md".addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? ""))
            let far = (there["text"] as? String ?? "").replacingOccurrences(of: "こちらは十個", with: "向こうは六個")
            _ = await talk("/_put", ["rel": "太郎のメモ.md", "text": far, "by": "太郎の Mac"])
            desk.tabs[at].text += "\nこちらでもう一行。\n"
            _ = try desk.save(note.path, store)
            guard let r = await sync.now("手") else { return "運びませんでした" }
            if r.clash != 1 { return "ぶつかりが \(r.clash)（\(r.trouble.joined(separator: " / ")))" }
            guard let now = desk.tabs.firstIndex(where: { $0.id == note.path }) else { return "札が消えました" }
            let tab = desk.tabs[now]
            if !tab.whole.contains("十個") || !tab.whole.contains("六個") { return "両方残っていません" }
            if tab.spots.count != 1 { return "ぶつかった場所が \(tab.spots.count) です" }
            if tab.who != "太郎の Mac" { return "相手の名前が \(tab.who) です" }
            return nil
        }
        await step("同期：「こちらの記載を反映する」を選ぶと、向こうの行が消えて向こうにも上がる") {
            guard let note = store.notes.first(where: { $0.path.hasSuffix("/太郎のメモ.md") }) else { return "太郎のメモ.md がありません" }
            desk.chooseSpot(note.path, 0, "ours", store)
            guard let now = desk.tabs.firstIndex(where: { $0.id == note.path }) else { return "札が消えました" }
            let tab = desk.tabs[now]
            if tab.whole.contains("六個") || !tab.whole.contains("十個") { return "選んだあとの字が違います" }
            if tab.clashing { return "選び口が残っています" }
            guard let r = await sync.now("手") else { return "運びませんでした" }
            if r.up != 1 { return "上がったのが \(r.up) 本" }
            let there = await talk("/_get?rel=" + ("太郎のメモ.md".addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? ""))
            let t = there["text"] as? String ?? ""
            desk.close(note.path)
            return t.contains("十個") && !t.contains("六個") ? nil : "向こうの字が違います"
        }
        // ファイル名は題に合わせる（依頼 502）── 離れたときと、時刻名の揃え直し。
        await step("名前：一行目でタイトルが決まるノートは、離れたときに名前が揃う") {
            let made = try Cian.call("new", ["dir": store.rootPath, "title": ""])
            guard let path = made["path"] as? String else { return "作れません" }
            _ = try Cian.call("write", ["path": path, "text": "名前は一行目から\n\n本文。\n", "force": true])
            store.reload()
            guard let note = store.notes.first(where: { $0.path == path }) else { return "一覧にありません" }
            desk.open(note, store)
            try desk.load(path, store)
            desk.close(path)
            let names = store.notes.map { $0.path.split(separator: "/").last.map(String.init) ?? "" }
            if names.contains(path.split(separator: "/").last.map(String.init) ?? "?") { return "時刻の名前のままです" }
            return names.contains("名前は一行目から.md") ? nil : "揃っていません: " + names.prefix(8).joined(separator: " / ")
        }
        await step("名前：時刻の名前のノートは、一度にタイトルの名前に揃う") {
            let made = try Cian.call("new", ["dir": store.rootPath, "title": ""])
            guard let path = made["path"] as? String else { return "作れません" }
            _ = try Cian.call("write", ["path": path, "text": "めそぽたみあ\n", "force": true])
            let n = store.tidyNames()
            let names = store.notes.map { $0.path.split(separator: "/").last.map(String.init) ?? "" }
            return n >= 1 && names.contains("めそぽたみあ.md") ? nil : "揃ったのが \(n) 本: " + names.prefix(8).joined(separator: " / ")
        }
        await step("同期：改名したぶんは、向こうも同じ ID のまま道が変わる") {
            guard let r = await sync.now("手") else { return "運びませんでした" }
            if r.trouble.first != nil { return "困りごと: " + r.trouble[0] }
            let there = await rels()
            return there.contains("名前は一行目から.md") && there.contains("めそぽたみあ.md") ? nil : "向こう: " + there.suffix(6).joined(separator: " / ")
        }
        Syncing.shared.desk = nil
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
        _ = await talk("/_put", ["rel": "attachments/太郎の絵.png", "b64": png, "by": "太郎の Mac"])
        await step("同期：向こうが置いた画像が、bytes のまま下りてくる") {
            guard let r = await sync.now("手") else { return "運びませんでした" }
            if r.down != 1 { return "下りたのが \(r.down) 本（\(r.trouble.joined(separator: " / ")))" }
            let at = store.rootPath + "/attachments/太郎の絵.png"
            guard let data = FileManager.default.contents(atPath: at) else { return "画像がありません" }
            return data.base64EncodedString() == png ? nil : "bytes が違います"
        }
        // 離れたときに題（# 太郎から）に揃えて 太郎から.md になり、向こうも同じ名前に
        // なっている ── いまの名前で消す。
        let taro = store.notes.first { $0.title == "太郎から" }.map { $0.path.split(separator: "/").last.map(String.init) ?? "" } ?? "太郎から.md"
        _ = await talk("/_trash", ["rel": taro])
        await step("同期：向こうで消したノートは、こちらからも消える") {
            guard let r = await sync.now("手") else { return "運びませんでした" }
            if r.gone != 1 { return "消えたのが \(r.gone) 本（\(taro)・\(r.trouble.joined(separator: " / ")))" }
            return store.notes.contains { $0.title == "太郎から" } ? "一覧に残っています" : nil
        }
        await step("同期：こちらで消したノートは、向こうでもゴミ箱へ") {
            guard let note = store.notes.first(where: { $0.title == "ストラテジーパターン" }) else { return "ノートがありません" }
            try store.remove(note)
            guard let r = await sync.now("手") else { return "運びませんでした" }
            if r.gone != 1 { return "消えたのが \(r.gone) 本" }
            return (await rels()).contains("ストラテジーパターン.md") ? "向こうに残っています" : nil
        }
        sync.auto = true
    }

    /// **保存ディレクトリを二つ**（依頼 511・窓の「二十の四」と同じ）── 足す・切り替える・
    /// 作る・移す・運ぶ・外す。二つ目はアプリの一時フォルダに置く（憶えは書かない）。
    private static func placeWalk(_ store: NotesStore, _ drive: String?) async {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("二つ目-\(Int(Date().timeIntervalSince1970))", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir.appendingPathComponent("持ち帰り"), withIntermediateDirectories: true)
        try? "---\ntitle: 別口\n---\n# 別口\n\n二つ目の保存ディレクトリのノート。\n"
            .write(to: dir.appendingPathComponent("別口.md"), atomically: true, encoding: .utf8)
        try? "# 宿題\n\n持ち帰りの中。\n"
            .write(to: dir.appendingPathComponent("持ち帰り/宿題.md"), atomically: true, encoding: .utf8)
        let first = store.placeId
        let before = store.notes.count
        step("保存ディレクトリ：一つのときは切り替えが無い") { store.many ? "二つ以上あります" : nil }
        step("保存ディレクトリ：二つ目を足すと、一覧が二つぶんになる") {
            store.add(dir, named: "二つ目")
            if store.places.count != 2 { return "場所が \(store.places.count) つ（\(store.trouble ?? "")）" }
            if store.notes.count != before + 2 { return "\(before) 本が \(store.notes.count) 本" }
            if store.rootName != "二つ目" { return "開いているのが \(store.rootName)" }
            if !store.allBooks.contains("持ち帰り") { return "二つ目のフォルダが無い: \(store.allBooks)" }
            if store.notes.filter(store.here).count != 2 { return "二つ目の中が \(store.notes.filter(store.here).count) 本" }
            if store.places[1].sync != "none" { return "足した直後に同期する設定になっています" }
            return nil
        }
        step("保存ディレクトリ：二つ目で作ると、二つ目に出来る") {
            guard let n = try store.make(titled: "二つ目の新しいノート") else { return "作れません" }
            return n.path.hasPrefix(dir.path) ? nil : "出来た道: \(n.path)"
        }
        step("保存ディレクトリ：一つ目に戻ると、一つ目のフォルダだけ") {
            store.enter(first)
            if store.rootName == "二つ目" { return "戻れていません" }
            if store.allBooks.contains("持ち帰り") { return "二つ目のフォルダが混ざっています" }
            return nil
        }
        step("保存ディレクトリ：二つ目のノートを一つ目へ移せる") {
            guard let n = store.notes.first(where: { $0.title == "二つ目の新しいノート" }) else { return "ノートが無い" }
            try store.move(n, to: nil)
            guard let now = store.notes.first(where: { $0.title == "二つ目の新しいノート" }) else { return "移したら消えた" }
            return now.root == store.rootPath ? nil : "居場所: \(now.root)"
        }
        step("保存ディレクトリ：よその保存ディレクトリのノートは、名前を頭に付けて言う") {
            guard let n = store.notes.first(where: { $0.title == "別口" }) else { return "別口が無い" }
            return store.bookLabel(n) == "二つ目" ? nil : store.bookLabel(n)
        }
        if let drive {
            func rels() async -> [String] {
                guard let url = URL(string: drive + "/_list"),
                      let (data, _) = try? await URLSession.shared.data(from: url),
                      let arr = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]] else { return [] }
                return arr.compactMap { ($0["appProperties"] as? [String: Any])?["rel"] as? String }
            }
            let sync = Syncing.shared
            sync.store = store
            sync.auto = false
            await step("同期：二つ目を Drive にすると、向こうでは ambər/二つ目/ の下に上がる") {
                guard let p = store.places.first(where: { $0.name == "二つ目" }) else { return "二つ目が無い" }
                store.setSync(p.id, "drive")
                guard let r = await sync.now("手") else { return "運びませんでした" }
                if let t = r.trouble.first { return "困りごと: " + t }
                guard let two = r.places["二つ目"], two.up >= 2 else { return "二つ目のぶんが上がっていません: \(r.places.keys.sorted())" }
                let there = await rels()
                if !there.contains("二つ目/別口.md") || !there.contains("二つ目/持ち帰り/宿題.md") {
                    return "向こうの一覧: " + there.filter { $0.contains("二つ目") || $0.contains("別口") }.joined(separator: " / ")
                }
                if !sync.freshWords(r).contains("二つ目:") { return "列に名前が付いていません: " + sync.freshWords(r) }
                return nil
            }
            await step("同期：二つ目を「同期しない」に戻すと、一つ目だけを運ぶ") {
                guard let p = store.places.first(where: { $0.name == "二つ目" }) else { return "二つ目が無い" }
                store.setSync(p.id, "none")
                guard let r = await sync.now("手") else { return "運びませんでした" }
                return r.places.keys.contains("二つ目") ? "二つ目も運んでいます" : nil
            }
            sync.auto = true
        }
        step("保存ディレクトリ：外すと一覧から消える（ファイルは残る）") {
            guard let p = store.places.first(where: { $0.name == "二つ目" }) else { return "二つ目が無い" }
            if !store.remove(place: p.id) { return "外せません" }
            if store.places.count != 1 { return "場所が \(store.places.count) つ" }
            if store.notes.contains(where: { $0.title == "別口" }) { return "別口が残っています" }
            return FileManager.default.fileExists(atPath: dir.appendingPathComponent("別口.md").path) ? nil : "ファイルが消えています"
        }
        try? FileManager.default.removeItem(at: dir)
    }

    private static func finish() {
        print("")
        for (name, why) in bad {
            print("✗ \(name)")
            print("   \(why)")
        }
        print("\(ran) とおり動かして、\(bad.isEmpty ? "落ちたものはありません" : "\(bad.count) 件おかしいです")")
        // 出力が届いてから終わる。
        fflush(stdout)
        exit(bad.isEmpty ? 0 : 1)
    }
}
#endif
