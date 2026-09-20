import Foundation
import SwiftUI

/// **運ぶ**（iPhone・依頼 500）── デスクトップ版の `syncNow`（`gui/renderer.js`）の写し。
///
/// **判断は core（`syncplan`）、運ぶのはここ。** 向こうの一覧を持ってきて、
/// 手順書をもらい、一つずつやって、運べたぶんだけ憶えてもらう（`synced`）。
/// 途中で切れても、運べたぶんは憶えに残る ── 次に続きから。
///
/// いつ運ぶか: 保存して三秒後・三十秒ごと・前に出たとき・サインインしたとき。
/// **打っている最中には触らない** ── 下ろしたものは、打ちかけでない札だけを
/// 読み直す。打ちかけなら、保存のときの混ぜ（`Desk.save`）に任せる。
@MainActor
final class Syncing: ObservableObject {
    static let shared = Syncing()

    @Published var signedIn = false
    @Published var who: Drive.Who?
    @Published var last: Date?
    @Published var busy = false
    @Published var trouble = ""
    @Published var troubleSince: Date?
    /// 「あとで」を押したか（この起動のあいだだけ・デスクトップ版の `syncLater` と同じ）。
    @Published var later = false
    /// 運んだ直後の数（数秒だけ出す）。
    @Published var fresh: Report?

    /// 勝手に運ぶか。**総ざらいは切ってから順に押す**。手で呼ぶ `now("手")` は切っても通る。
    var auto = true
    weak var store: NotesStore?
    weak var desk: Desk?

    private var soonTask: Task<Void, Never>?
    private var clockTask: Task<Void, Never>?
    private var freshTask: Task<Void, Never>?

    struct Report {
        var reason: String
        var up = 0, down = 0, gone = 0, clash = 0, moved = 0, eyes = 0
        var trouble: [String] = []
        var touched: Set<String> = []
        /// 保存ディレクトリごとの数（名前 → そのぶん・依頼 511）。
        var places: [String: Report] = [:]
    }

    /// 運ぶ相手 ── Drive にしてある保存ディレクトリ（開けているものだけ・依頼 511）。
    private func targets(_ store: NotesStore) -> [NotesStore.Place] {
        store.places.filter { $0.sync == "drive" && store.url(of: $0) != nil }
    }

    /// 向こうの一覧のうち、この保存ディレクトリのぶん（パスは保存ディレクトリからの相対に）。
    /// **いちばん目は `ambər` の直下、二つ目からは `ambər/<at>/`**（デスクトップ版の `remoteOf` と同じ）。
    private static func remoteOf(_ all: [Drive.Remote], _ place: NotesStore.Place, _ places: [NotesStore.Place]) -> [Drive.Remote] {
        let others = places.map(\.at).filter { !$0.isEmpty && $0 != place.at }
        let pre = place.at.isEmpty ? "" : place.at + "/"
        var out: [Drive.Remote] = []
        for x in all {
            if !pre.isEmpty {
                guard x.rel.hasPrefix(pre) else { continue }
                out.append(Drive.Remote(rel: String(x.rel.dropFirst(pre.count)), id: x.id, tag: x.tag, by: x.by))
            } else {
                if others.contains(where: { x.rel.hasPrefix($0 + "/") }) { continue }
                out.append(x)
            }
        }
        return out
    }

    /// 開いた直後に一度 ── サインインしているなら時計を回して、六秒後に一度合わせる。
    func load() {
        signedIn = Drive.shared.signedIn
        who = Drive.shared.who
        if signedIn { clock(); soon(6) } else { clockTask?.cancel(); clockTask = nil }
    }

    func soon(_ seconds: Double = 3) {
        guard signedIn else { return }
        soonTask?.cancel()
        soonTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
            guard !Task.isCancelled else { return }
            _ = await self?.now("保存")
        }
    }

    func clock() {
        clockTask?.cancel()
        clockTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 30_000_000_000)
                guard !Task.isCancelled else { return }
                _ = await self?.now("時計")
            }
        }
    }

    /// 一度、合わせる。返すのは何を運んだかの数（走査が見る）。
    /// 保存ディレクトリごとに順に運び、数は足す（`places` に一つずつも残す）。
    @discardableResult
    func now(_ reason: String) async -> Report? {
        guard signedIn, !busy, let store else { return nil }
        let targets = targets(store)
        guard !targets.isEmpty else { return nil }
        if !auto && reason != "手" { return nil }
        busy = true
        defer { busy = false }
        var report = Report(reason: reason)
        do {
            let all = try await Drive.shared.list()
            for place in targets {
                let one = await carry(place, Self.remoteOf(all, place, store.places), store)
                report.up += one.up; report.down += one.down; report.gone += one.gone
                report.clash += one.clash; report.moved += one.moved; report.eyes += one.eyes
                report.trouble += one.trouble.map { (store.many ? place.name + " › " : "") + $0 }
                report.touched.formUnion(one.touched)
                report.places[place.name] = one
            }
            if !report.touched.isEmpty || report.gone > 0 {
                store.reload()
                desk?.pull(report.touched, store)
            }
            last = Date()
            trouble = report.trouble.first ?? ""
            if report.up + report.down + report.gone + report.clash + report.moved > 0 {
                fresh = report
                freshTask?.cancel()
                freshTask = Task { [weak self] in
                    try? await Task.sleep(nanoseconds: 6_000_000_000)
                    if !Task.isCancelled { self?.fresh = nil }
                }
            }
        } catch {
            trouble = error.localizedDescription
            report.trouble.append(trouble)
        }
        troubleSince = trouble.isEmpty ? nil : (troubleSince ?? Date())
        return report
    }

    /// 一つの保存ディレクトリを運ぶ。`remote` はそのぶんの一覧（相対）。
    ///
    /// core（`syncplan`・`synced`）は保存ディレクトリ一つしか知らない ── 帳画面
    /// （`.amber/sync.json`）もそこにある。Drive の上の道だけ `pre` を頭に付ける。
    private func carry(_ place: NotesStore.Place, _ remote: [Drive.Remote], _ store: NotesStore) async -> Report {
        var report = Report(reason: place.name)
        guard let root = store.url(of: place)?.path else { return report }
        let pre = place.at.isEmpty ? "" : place.at + "/"
        do {
            let plan = try Cian.call("syncplan", [
                "path": root, "who": "drive",
                "remote": remote.map { ["rel": $0.rel, "id": $0.id, "tag": $0.tag, "by": $0.by] },
            ])
            var done: [[String: String]] = []
            var gone: [String] = []
            var moved: [String] = []
            for s in plan["steps"] as? [[String: Any]] ?? [] {
                guard let what = s["do"] as? String, let rel = s["rel"] as? String else { continue }
                let id = s["id"] as? String ?? ""
                let bin = s["bin"] as? Bool ?? false
                let at = root + "/" + rel
                let there = remote.first { $0.id == id }
                do {
                    switch what {
                    case "up":
                        let print = try Cian.call("syncprint", ["path": at])["print"] as? String ?? ""
                        let newId: String
                        if bin {
                            newId = try await Drive.shared.upload(rel: pre + rel, bytes: try Data(contentsOf: URL(fileURLWithPath: at)),
                                                                  print: print, id: id.isEmpty ? nil : id)
                        } else {
                            let text = try Cian.call("read", ["path": at])["text"] as? String ?? ""
                            newId = try await Drive.shared.upload(rel: pre + rel, text: text, print: print, id: id.isEmpty ? nil : id)
                        }
                        done.append(["rel": rel, "id": newId, "tag": print])
                        report.up += 1
                    case "down":
                        if bin {
                            try Self.put(try await Drive.shared.downloadBytes(id), at: at)
                        } else {
                            let text = try await Drive.shared.download(id)
                            _ = try Cian.call("syncdown", ["path": at, "text": text])
                        }
                        done.append(["rel": rel, "id": id, "tag": there?.tag ?? ""])
                        report.down += 1
                        report.touched.insert(at)
                    case "drophere":
                        // 向こうで消え、こちらは触っていない ── ゴミ箱へ（消さない）。
                        let url = URL(fileURLWithPath: at)
                        do { try FileManager.default.trashItem(at: url, resultingItemURL: nil) }
                        catch { try FileManager.default.removeItem(at: url) }
                        gone.append(rel)
                        report.gone += 1
                        report.touched.insert(at)
                    case "dropthere":
                        try await Drive.shared.trash(id)
                        gone.append(rel)
                        report.gone += 1
                    case "movethere":
                        try await Drive.shared.rename(id, rel: pre + rel)
                        done.append(["rel": rel, "id": id, "tag": there?.tag ?? ""])
                        moved.append(rel)
                        report.moved += 1
                    case "movehere":
                        let from = s["from"] as? String ?? ""
                        let r = try Cian.call("syncmove", ["path": root, "from": from, "to": rel])
                        let to = r["path"] as? String ?? at
                        done.append(["rel": r["rel"] as? String ?? rel, "id": id, "tag": there?.tag ?? ""])
                        desk?.moved(from: root + "/" + from, to: to, store)
                        report.moved += 1
                        report.touched.insert(to)
                    case "clash" where bin:
                        // 画像は混ぜられない ── こちらを残し、向こうのものは隣に `名前.2.png`。
                        var beside = Self.numbered(at, 2)
                        var n = 3
                        while FileManager.default.fileExists(atPath: beside) && n < 100 { beside = Self.numbered(at, n); n += 1 }
                        try Self.put(try await Drive.shared.downloadBytes(id), at: beside)
                        let print = try Cian.call("syncprint", ["path": at])["print"] as? String ?? ""
                        let newId = try await Drive.shared.upload(rel: pre + rel, bytes: try Data(contentsOf: URL(fileURLWithPath: at)), print: print, id: id)
                        done.append(["rel": rel, "id": newId, "tag": print])
                        report.up += 1
                        report.down += 1
                        report.touched.insert(at)
                    case "clash":
                        // 両方が変わった ── 混ぜる。分かれる前の姿は synced が取っておいたもの。
                        let theirs = try await Drive.shared.download(id)
                        let ours = try Cian.call("read", ["path": at])["text"] as? String ?? ""
                        var base = ""
                        if let hash = s["base"] as? String, !hash.isEmpty {
                            base = try Cian.call("baseread", ["path": root, "hash": hash])["text"] as? String ?? ""
                        }
                        let got = try Cian.call("merge", ["was": base, "ours": ours, "theirs": theirs])
                        let text = got["text"] as? String ?? ours
                        _ = try? Cian.call("keep", ["root": root, "path": at, "text": ours, "gap": 0, "force": true])
                        _ = try Cian.call("syncdown", ["path": at, "text": text])
                        let print = try Cian.call("syncprint", ["path": at])["print"] as? String ?? ""
                        let newId = try await Drive.shared.upload(rel: pre + rel, text: text, print: print, id: id)
                        done.append(["rel": rel, "id": newId, "tag": print])
                        let merged = NotesStore.Merged.from(got, text: text)
                        desk?.incoming(at, merged, who: (there?.by.isEmpty == false ? there!.by : "向こう"), store)
                        report.clash += 1
                        if merged.eyes { report.eyes += 1 }
                        report.touched.insert(at)
                    default:
                        break
                    }
                } catch {
                    report.trouble.append(rel + ": " + error.localizedDescription)
                }
            }
            if !done.isEmpty || !gone.isEmpty {
                _ = try Cian.call("synced", ["path": root, "who": "drive", "done": done, "gone": gone, "moved": moved])
            }
        } catch {
            report.trouble.append(error.localizedDescription)
        }
        return report
    }

    /// 運んだ直後の列の文字。二つ以上あるときは保存ディレクトリの名前を頭に（依頼 511）。
    func freshWords(_ r: Report) -> String {
        func parts(_ r: Report) -> [String] {
            var out: [String] = []
            if r.up > 0 { out.append("アップロード\(r.up)件") }
            if r.down > 0 { out.append("ダウンロード\(r.down)件") }
            if r.gone > 0 { out.append("ゴミ箱へ\(r.gone)件") }
            if r.clash > 0 { out.append("同じ行を両方で直したノート\(r.clash)件") }
            if r.moved > 0 { out.append("名前の変更\(r.moved)件") }
            return out
        }
        let each = r.places.filter { !parts($0.value).isEmpty }.sorted { $0.key < $1.key }
        if store?.many == true, !each.isEmpty {
            return each.map { $0.key + ": " + parts($0.value).joined(separator: "・") }.joined(separator: "　")
        }
        return parts(r).joined(separator: "・")
    }

    /// サインインして、時計を回す。
    func signIn() async throws -> Drive.Who {
        let who = try await Drive.shared.signIn()
        load()
        return who
    }

    func signOut() async {
        await Drive.shared.signOut()
        load()
        last = nil
        trouble = ""
    }

    /// 困りごとを人の言葉に（デスクトップ版の `syncTroubleFace` と同じ）。
    var troubleFace: (text: String, button: String, again: Bool) {
        let e = trouble.lowercased()
        if e.contains("offline") || e.contains("network") || e.contains("インターネット") || e.contains("-1009") || e.contains("-1004") || e.contains("could not connect") {
            return ("インターネットに繋がっていないようです。次回接続時に同期します。", "接続確認する", true)
        }
        if e.contains("サインインが切れ") || e.contains("invalid_grant") || e.contains("401") {
            return ("Google のサインインが切れています。もう一度サインインしてください。", "Google でサインイン", false)
        }
        if e.contains("insufficient") || e.contains("storage") || e.contains("quota") || e.contains("507") {
            return ("Google Drive の空きが足りないようです。空けてから、もう一度試してください。", "もう一度試す", true)
        }
        return (trouble, "もう一度試す", true)
    }

    /// 一行の様子（一覧の頭に出す）。
    var line: String {
        if let store, !store.places.contains(where: { $0.sync == "drive" }) { return "同期していません ・ どの保存ディレクトリも「同期しない」" }
        if !signedIn { return "同期していません" }
        if busy { return "同期しています…" }
        var out = "同期しています"
        if let last { out += " ・ 最終 " + Self.hhmm(last) }
        if let who, !who.email.isEmpty { out += " ・ " + who.email }
        return out
    }

    static func hhmm(_ d: Date) -> String {
        let f = DateFormatter()
        f.dateFormat = "HH:mm"
        return f.string(from: d)
    }

    // ── 画像の置き方 ──

    /// 仮の名で書いてから改名 ── 途中で切れても半端な画像を残さない。
    private static func put(_ bytes: Data, at path: String) throws {
        let url = URL(fileURLWithPath: path)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        let tmp = URL(fileURLWithPath: path + ".amber-part")
        try bytes.write(to: tmp)
        if FileManager.default.fileExists(atPath: path) { try FileManager.default.removeItem(at: url) }
        try FileManager.default.moveItem(at: tmp, to: url)
    }

    private static func numbered(_ path: String, _ n: Int) -> String {
        guard let dot = path.lastIndex(of: "."), path[dot...].firstIndex(of: "/") == nil else { return path + ".\(n)" }
        return String(path[..<dot]) + ".\(n)" + String(path[dot...])
    }
}

extension Desk {
    /// 下りてきたノートを、**打ちかけでない札だけ**読み直す（依頼 500）。
    func pull(_ paths: Set<String>, _ store: NotesStore) {
        for at in tabs.indices where paths.contains(tabs[at].note.path) && !tabs[at].dirty {
            let id = tabs[at].id
            tabs[at].loaded = false
            try? load(id, store)
        }
    }

    /// 向こうで改名された ── 開いているラベルがあれば、新しいパスのラベルに差し替える。
    func moved(from: String, to: String, _ store: NotesStore) {
        guard let at = tabs.firstIndex(where: { $0.note.path == from }) else { return }
        guard let got = try? Cian.call("note", ["path": to]), let note = Note(got) else { return }
        let was = tabs[at]
        var fresh = Tab(note: note)
        fresh.reading = was.reading
        tabs[at] = fresh
        if showing == from { showing = to }
        try? load(to, store)
    }
}
