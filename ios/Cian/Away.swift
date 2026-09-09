import SwiftUI

/// **よその予定表**（依頼 456・電話の側）。
///
/// Google カレンダーの非公開 URL も、iCloud の公開リンクも、中身は同じ形
/// （iCal）── 読むのは核（`ics`）で、**窓と同じ一組**。同じ URL を貼れば、
/// 窓と電話に同じ予定が同じ日に並ぶ。
///
/// **アドレスは、この iPhone の中だけに持つ。** それを知っている人が予定を
/// 全部読めるので、鍵と同じ扱い ── ノートにも `.amber/` にも書かない
/// （あそこはフォルダと一緒に旅をする）。
@MainActor
enum Away {
    struct Feed: Identifiable, Hashable {
        let url: String
        let name: String
        var id: String { url }
    }

    private static let key = "amber.away"

    static var feeds: [Feed] {
        get {
            let rows = UserDefaults.standard.array(forKey: key) as? [[String: String]] ?? []
            return rows.compactMap {
                guard let u = $0["url"] else { return nil }
                return Feed(url: u, name: $0["name"] ?? u)
            }
        }
        set {
            UserDefaults.standard.set(newValue.map { ["url": $0.url, "name": $0.name] }, forKey: key)
        }
    }

    enum Trouble: LocalizedError {
        case road, reply(String), notCalendar, tooBig
        var errorDescription: String? {
            switch self {
            case .road: return "URL の形になっていません"
            case .reply(let why): return why
            case .notCalendar: return "予定表の形をしていません"
            case .tooBig: return "大きすぎます（8MB まで）"
            }
        }
    }

    /// 取ってくる。**`http`/`https` だけ**（`Clipping` と同じ理由）。
    static func fetch(_ url: URL) async throws -> String {
        var ask = URLRequest(url: url, timeoutInterval: 15)
        ask.setValue("amber (+calendar)", forHTTPHeaderField: "User-Agent")
        ask.setValue("text/calendar,text/plain", forHTTPHeaderField: "Accept")
        do {
            let (data, answer) = try await URLSession.shared.data(for: ask)
            if let http = answer as? HTTPURLResponse, !(200...299).contains(http.statusCode) {
                throw Trouble.reply("\(http.statusCode) \(HTTPURLResponse.localizedString(forStatusCode: http.statusCode))")
            }
            if data.count > 8 * 1024 * 1024 { throw Trouble.tooBig }
            return String(decoding: data, as: UTF8.self)
        } catch let e as Trouble {
            throw e
        } catch {
            throw Trouble.reply(error.localizedDescription)
        }
    }

    /// 読めるかどうか見て、予定表の名前を返す。
    static func check(_ text: String) throws -> String {
        let got = try Cian.call("ics", ["text": text, "year": 2026, "month": 1])
        return got["name"] as? String ?? ""
    }

    /// ひと月ぶん、購読しているところぜんぶから。
    ///
    /// **一つ取れなくても、ほかは出す。** 網の向こうの都合で全部が出ない
    /// のは、カレンダーとして使いものにならない。
    static func month(_ year: Int, _ month: Int) async -> [Calendaring.Slot] {
        var out: [Calendaring.Slot] = []
        for f in feeds {
            guard let u = Clipping.reach(f.url) else { continue }
            guard let text = try? await fetch(u) else { continue }
            guard let got = try? Cian.call("ics", [
                "text": text, "year": year, "month": month,
            ]) else { continue }
            for r in got["days"] as? [[String: Any]] ?? [] {
                out.append(Calendaring.Slot(
                    day: r["day"] as? String ?? "",
                    at: r["at"] as? String,
                    title: r["title"] as? String ?? "",
                    path: "",
                    kind: "away",
                    place: r["place"] as? String ?? "",
                    from: f.name))
            }
        }
        return out
    }
}

/// 読んでいる予定表を、増やす・やめる。
struct Feeds: View {
    @Environment(\.dismiss) private var dismiss
    @State private var feeds = Away.feeds
    @State private var asking = false
    @State private var url = ""
    @State private var busy = false
    @State private var trouble: String?

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(feeds) { f in
                        VStack(alignment: .leading, spacing: 2) {
                            Text(f.name)
                            Text(f.url).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                        }
                    }
                    .onDelete { at in
                        feeds.remove(atOffsets: at)
                        Away.feeds = feeds
                    }
                    if feeds.isEmpty {
                        Text("まだ読んでいる予定表はありません。")
                            .foregroundStyle(.secondary).font(.footnote)
                    }
                } header: {
                    Text("読んでいる予定表")
                } footer: {
                    Text("読むだけです。向こうの予定表は何も変わりません。"
                         + "アドレスはこの iPhone の中だけに置きます。")
                }
                Section {
                    Button {
                        url = ""
                        asking = true
                    } label: {
                        Label("予定表を足す", systemImage: "plus")
                    }
                } footer: {
                    Text("Google カレンダーなら「設定 → カレンダーの統合 → "
                         + "非公開 URL（iCal 形式）」のアドレスです。")
                }
            }
            .navigationTitle("よその予定表")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("閉じる") { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) { EditButton() }
            }
            .overlay {
                if busy {
                    ZStack {
                        Color.black.opacity(0.25).ignoresSafeArea()
                        VStack(spacing: 10) {
                            ProgressView()
                            Text("取りに行っています…").font(.footnote)
                        }
                        .padding(22)
                        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 14))
                    }
                }
            }
            .alert("予定表を足す", isPresented: $asking) {
                TextField("https://…", text: $url)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.URL)
                Button("足す") { add() }
                Button("やめる", role: .cancel) {}
            } message: {
                Text("iCal（.ics）のアドレスを貼ってください。")
            }
            .alert("できませんでした", isPresented: Binding(
                get: { trouble != nil }, set: { if !$0 { trouble = nil } })
            ) { Button("閉じる") {} } message: { Text(trouble ?? "") }
        }
    }

    private func add() {
        guard let u = Clipping.reach(url) else {
            trouble = "URL の形になっていません"
            return
        }
        busy = true
        Task { @MainActor in
            defer { busy = false }
            do {
                let text = try await Away.fetch(u)
                let name = try Away.check(text)
                feeds.removeAll { $0.url == u.absoluteString }
                feeds.append(Away.Feed(url: u.absoluteString,
                                       name: name.isEmpty ? (u.host ?? "予定表") : name))
                Away.feeds = feeds
            } catch {
                trouble = error.localizedDescription
            }
        }
    }
}
