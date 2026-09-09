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
            guard let root = store.rootURL else { return "置き場所がありません" }
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

        // ── 四。よそから来るもの ────────────────────────
        if let site = ProcessInfo.processInfo.environment["SITE"],
           let url = Clipping.reach(site) {
            await step("Web から取り込む") {
                let hand = Clipping()
                await hand.warm()
                let got = try await hand.clip(url)
                if got.title.isEmpty { return "題が取れません" }
                if !got.body.contains("出典:") { return "出どころの行がありません" }
                return nil
            }
        }

        finish()
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
