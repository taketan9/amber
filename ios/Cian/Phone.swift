import EventKit
import Foundation

/// **この iPhone の予定表**（依頼 460）── 読み書きの両方。
///
/// iOS の設定に足してあるカレンダー（Google でも iCloud でも会社のでも）を、
/// そのまま読んで、そのまま書ける。**amber はキーを一つも預からない** ──
/// アカウントを持っているのは iPhone のほうで、amber はそれを借りるだけ。
///
/// よその予定表（`Away`・iCal）との違いは一つ、**こちらは書ける**こと。
/// 向こうは読むための形で、書き戻す口が無い。
@MainActor
enum Phone {
    private static let store = EKEventStore()

    /// 許可を訊いたことがあるか。**訊く前に断られた形にしない** ──
    /// 「まだ訊いていない」と「断られた」は別のことで、後者なら設定へ
    /// 案内するしかない。
    static var asked: Bool {
        EKEventStore.authorizationStatus(for: .event) != .notDetermined
    }

    static var allowed: Bool {
        EKEventStore.authorizationStatus(for: .event) == .fullAccess
    }

    /// 許可を訊く。**押されたときだけ訊く** ── 起きた瞬間に訊くと、
    /// 何のために訊かれたのか分からないまま断られる。
    @discardableResult
    static func ask() async -> Bool {
        if allowed { return true }
        return (try? await store.requestFullAccessToEvents()) ?? false
    }

    /// この端末にある予定表の名前（表示設定で出し入れするため）。
    /// 書ける予定表そのもの（名前で指すため・依頼 545）。
    static var calendarObjects: [EKCalendar] { store.calendars(for: .event) }

    static var calendars: [String] {
        guard allowed else { return [] }
        var seen: [String] = []
        for c in store.calendars(for: .event) where !seen.contains(c.title) { seen.append(c.title) }
        return seen
    }

    /// ひと月ぶん。
    static func month(_ year: Int, _ month: Int) -> [Calendaring.Slot] {
        guard allowed else { return [] }
        var cal = Calendar(identifier: .gregorian)
        cal.firstWeekday = 2
        guard let from = cal.date(from: DateComponents(year: year, month: month, day: 1)),
              let to = cal.date(byAdding: DateComponents(month: 1), to: from)
        else { return [] }
        let found = store.events(matching:
            store.predicateForEvents(withStart: from, end: to, calendars: nil))
        let day = DateFormatter()
        day.dateFormat = "yyyy-MM-dd"
        let clock = DateFormatter()
        clock.dateFormat = "HH:mm"

        var out: [Calendaring.Slot] = []
        for e in found {
            guard let start = e.startDate else { continue }
            // **終日で何日かにまたがるものは、その日ぶんぜんぶに置く**
            // ── 出張や休みは、始まった日にだけ出ても役に立たない。
            if e.isAllDay {
                var d = cal.startOfDay(for: start)
                let last = e.endDate ?? start
                while d <= last && d < to {
                    if d >= from {
                        out.append(slot(e, day.string(from: d), nil))
                    }
                    guard let next = cal.date(byAdding: .day, value: 1, to: d) else { break }
                    d = next
                }
            } else {
                out.append(slot(e, day.string(from: start), clock.string(from: start)))
            }
        }
        return out
    }

    private static func slot(_ e: EKEvent, _ day: String, _ at: String?) -> Calendaring.Slot {
        Calendaring.Slot(
            day: day, at: at,
            title: e.title ?? "（タイトルなし）",
            // **ここではパスの代わりに、iOS の言う名札を持つ** ── あとで
            // その予定そのものを開く／直すときに要る。
            path: e.eventIdentifier ?? "",
            kind: "here",
            place: e.location ?? "",
            from: e.calendar?.title ?? "",
            // **メモ欄をそのまま持ってくる**（依頼 547）── どこがタグ行かの
            // 判断はここでしない。切り出しは core に1 つだけ置く（写すと、
            // デスクトップ版と iPhone で二つの読み方ができる）。
            notes: e.notes ?? "")
    }

    /// 予定を登録する。**書ける先が無ければ、そう言う。**
    /// `into` に予定表の名前を渡すと、そこへ書く（依頼 545）──
    /// グループカレンダーは端末に降りてきた1 つなので、**名前でしか指せない**。
    /// `notes` はメモ欄（誰の用事かのタグが、その最後の行に入る）。
    /// 予定を一つ足す。
    ///
    /// `until` は**日をまたぐ予定の最後の日**（依頼 700・`yyyy-MM-dd`）。
    /// `alarm` は「10分前」のような言葉（依頼 701）── 端末の予定表に
    /// 置くので、**鳴らすのは OS** で、amber が起きている必要はない。
    static func add(title: String, day: String, at: String?, end: String? = nil,
                    until: String? = nil, alarm: String? = nil,
                    notes: String = "", into: String = "") throws {
        var target = store.defaultCalendarForNewEvents
        if !into.isEmpty {
            target = calendarObjects.first { $0.title == into && $0.allowsContentModifications }
            guard target != nil else { throw Trouble.noNamed(into) }
        }
        guard let cal = target else {
            throw Trouble.noCalendar
        }
        guard let start = when(day, at) else { throw Trouble.badDay }
        let e = EKEvent(eventStore: store)
        e.calendar = cal
        e.title = title
        e.startDate = start
        if at == nil {
            e.isAllDay = true
            e.endDate = start
        } else if let end, let till = when(day, end), till > start {
            // 終わりの時刻（デスクトップ版と同じ・依頼 493）── 開始より前なら一時間後に。
            e.endDate = till
        } else {
            e.endDate = start.addingTimeInterval(60 * 60)
        }
        // **日をまたぐなら、終わりの日まで伸ばす**（依頼 700）。終日なら
        // その日いっぱい、時刻つきなら終わりの時刻まで。
        if let until, let last = when(until, at == nil ? nil : end), last > e.endDate ?? start {
            e.endDate = last
        }
        // **アラームは端末の予定表に預ける**（依頼 701）── OS が鳴らすので、
        // amber が起きていなくても鳴る。
        if let alarm, let before = Self.before(alarm) {
            e.addAlarm(EKAlarm(relativeOffset: before))
        }
        if !notes.isEmpty { e.notes = notes }
        try store.save(e, span: .thisEvent)
    }

    /// 「10分前」のような言葉を、秒（負の数）に（依頼 701）。
    /// 知らない言葉なら `nil` ── 鳴らさないほうが、変な時刻に鳴るよりよい。
    static func before(_ word: String) -> TimeInterval? {
        switch word {
        case "5分前": return -5 * 60
        case "10分前": return -10 * 60
        case "30分前": return -30 * 60
        case "1時間前": return -60 * 60
        case "前日": return -24 * 60 * 60
        default: return nil
        }
    }

    /// **全員に通知する予定を、この端末の予定表から拾う**（依頼 701・甲）。
    ///
    /// 予定表のアラーム（`EKAlarm`）は**その端末にしか入らない**ので、
    /// 同期した相手には届かない。メモ欄のいちばん最後の行に
    /// `#全員に通知:10分前` と書いてある予定は、**受け取った端末が自分で
    /// 仕掛ける。** 仕掛けるのは `NotesStore` の仕事で、ここは材料を返す。
    ///
    /// **何分前かを言葉から直すのは core**（`caltag` が `alarmMin` を返す）
    /// ── ここで直すと、ノートの道と読み方が二つになる。
    ///
    /// 過ぎた時刻は返さない ── 仕掛けると、開いた瞬間に昔の予定が鳴る。
    static func sharedAlarms(days: Int = 60) -> [(id: String, title: String, at: Date)] {
        guard allowed else { return [] }
        let now = Date()
        guard let to = Calendar.current.date(byAdding: .day, value: days, to: now) else {
            return []
        }
        let found = store.events(matching:
            store.predicateForEvents(withStart: now, end: to, calendars: nil))
        let memos = found.map { $0.notes ?? "" }
        guard !memos.isEmpty,
              let got = try? Cian.call("caltag", ["notes": memos]),
              let each = got["each"] as? [[String: Any]] else { return [] }
        var out: [(id: String, title: String, at: Date)] = []
        for (i, e) in found.enumerated() where i < each.count {
            guard let mins = each[i]["alarmMin"] as? Int,
                  let start = e.startDate,
                  let id = e.eventIdentifier else { continue }
            let at = start.addingTimeInterval(-Double(mins) * 60)
            if at > now { out.append((id: id, title: e.title ?? "", at: at)) }
        }
        return out
    }

    /// 予定を消す。
    static func drop(_ id: String) throws {
        guard let e = store.event(withIdentifier: id) else { throw Trouble.gone }
        try store.remove(e, span: .thisEvent)
    }

    /// 題を直す。
    static func rename(_ id: String, to title: String) throws {
        guard let e = store.event(withIdentifier: id) else { throw Trouble.gone }
        e.title = title
        try store.save(e, span: .thisEvent)
    }

    private static func when(_ day: String, _ at: String?) -> Date? {
        let f = DateFormatter()
        f.dateFormat = at == nil ? "yyyy-MM-dd" : "yyyy-MM-dd HH:mm"
        return f.date(from: at == nil ? day : day + " " + (at ?? ""))
    }

    enum Trouble: LocalizedError {
        case noCalendar, badDay, gone
        /// 名前で指した予定表が、この端末に見つからない（依頼 545）。
        case noNamed(String)
        var errorDescription: String? {
            switch self {
            case .noCalendar: return "書ける予定表がこの iPhone にありません。"
            case .badDay: return "日付を読めませんでした。"
            case .gone: return "その予定は、もうありません。"
            // **黙って自分の予定表に入れない。** グループと共有するつもりで
            // 登録した予定が、誰にも見えないところに入るほうがずっと悪い。
            case .noNamed(let name):
                return "「\(name)」がこの iPhone のカレンダーにまだありません。"
                    + "少し待ってから、もう一度お試しください。"
            }
        }
    }
}
