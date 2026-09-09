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
/// **釦の位置が変わるたびに検査が壊れる**から ── 壊れた検査は直されず、
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
        step("升を押す") {
            let out = try store.checked("- [ ] 牛乳\n", line: 0, done: true)
            return out.contains("- [x]") ? nil : "升が入りません: \(out)"
        }
        step("絵の大きさを変える") {
            let out = try store.sized("![絵](a.png)", width: "200")
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
