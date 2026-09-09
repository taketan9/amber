import EventKit
import Foundation

/// **この iPhone の予定表**（依頼 460）── 読み書きの両方。
///
/// iOS の設定に足してあるカレンダー（Google でも iCloud でも会社のでも）を、
/// そのまま読んで、そのまま書ける。**amber は鍵を一つも預からない** ──
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
            title: e.title ?? "（題なし）",
            // **ここでは道の代わりに、iOS の言う名札を持つ** ── あとで
            // その予定そのものを開く／直すときに要る。
            path: e.eventIdentifier ?? "",
            kind: "here",
            place: e.location ?? "",
            from: e.calendar?.title ?? "")
    }

    /// 予定を足す。**書ける先が無ければ、そう言う。**
    static func add(title: String, day: String, at: String?) throws {
        guard let cal = store.defaultCalendarForNewEvents else {
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
        } else {
            e.endDate = start.addingTimeInterval(60 * 60)
        }
        try store.save(e, span: .thisEvent)
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
        var errorDescription: String? {
            switch self {
            case .noCalendar: return "書ける予定表がこの iPhone にありません。"
            case .badDay: return "日付を読めませんでした。"
            case .gone: return "その予定は、もうありません。"
            }
        }
    }
}
