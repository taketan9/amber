// **この Mac の予定表**（依頼 462）。窓（Electron）から呼ばれる小さな道具。
//
//     amber-cal ask                     許可を訊く
//     amber-cal month 2026 9            ひと月ぶん（JSON）
//     amber-cal add 題 2026-09-11 11:00 足す（時刻は省ける＝終日）
//     amber-cal rename <id> 新しい題    直す
//     amber-cal drop <id>               消す
//     amber-cal notes <id> <メモ>       メモ欄を書き換える（タグの置き場所）
//
// **なぜ別の実行ファイルなのか。** Electron から EventKit は呼べない。
// AppleScript で「カレンダー」アプリに話しかける道もあるが、あちらは
// アプリを起こす必要があり、月ぶんを読むのに何秒もかかる。
// **電話と同じ EventKit** を使えば、窓と電話で同じ答えになる ── 二つの
// amber から見て、違う予定が出るのがいちばん困る。
//
// 答えは JSON を一行。**判断はここに置かない** ── 何を並べるか・何を
// 混ぜるかは窓の側（と core）の仕事で、ここは OS への口だけ。

import EventKit
import Foundation

let store = EKEventStore()

func out(_ any: Any) -> Never {
    let data = (try? JSONSerialization.data(withJSONObject: any)) ?? Data("{}".utf8)
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
    exit(0)
}

func no(_ why: String) -> Never {
    out(["error": why])
}

/// 許可を訊く。**待つ** ── 呼んだ側は一行返ってくることだけを知っている。
func ask() -> Bool {
    if EKEventStore.authorizationStatus(for: .event) == .fullAccess { return true }
    var ok = false
    let wait = DispatchSemaphore(value: 0)
    store.requestFullAccessToEvents { got, _ in
        ok = got
        wait.signal()
    }
    _ = wait.wait(timeout: .now() + 60)
    return ok
}

func need() {
    if EKEventStore.authorizationStatus(for: .event) != .fullAccess {
        no("この Mac の予定表を読む許可がありません")
    }
}

let args = Array(CommandLine.arguments.dropFirst())
guard let what = args.first else { no("何をするか言われていません") }

switch what {
case "ask":
    out(["ok": ask()])

// この Mac の予定表の名前（表示の設定で、出す・出さないを選ぶのに使う）。
case "calendars":
    need()
    let names = store.calendars(for: .event).map { $0.title }.sorted()
    out(["ok": true, "calendars": names])

case "month":
    need()
    guard args.count >= 3, let year = Int(args[1]), let month = Int(args[2]) else {
        no("年と月が要ります")
    }
    var cal = Calendar(identifier: .gregorian)
    cal.timeZone = .current
    guard let from = cal.date(from: DateComponents(year: year, month: month, day: 1)),
          let to = cal.date(byAdding: DateComponents(month: 1), to: from) else {
        no("その月がありません")
    }
    let day = DateFormatter()
    day.dateFormat = "yyyy-MM-dd"
    let clock = DateFormatter()
    clock.dateFormat = "HH:mm"

    var rows: [[String: Any]] = []
    for e in store.events(matching: store.predicateForEvents(withStart: from, end: to, calendars: nil)) {
        guard let start = e.startDate else { continue }
        // **終日で何日かにまたがるものは、その日ぶんぜんぶに置く** ──
        // 出張や休みは、始まった日にだけ出ても役に立たない（電話と同じ）。
        if e.isAllDay {
            var d = cal.startOfDay(for: start)
            let last = e.endDate ?? start
            while d <= last && d < to {
                if d >= from {
                    rows.append(one(e, day.string(from: d), nil, nil))
                }
                guard let next = cal.date(byAdding: .day, value: 1, to: d) else { break }
                d = next
            }
        } else {
            // **終わりの時刻も渡す** ── 週と日の表では、それが高さになる。
            let till = e.endDate.map { clock.string(from: $0) }
            rows.append(one(e, day.string(from: start), clock.string(from: start), till))
        }
    }
    out(["days": rows])

case "add":
    need()
    guard args.count >= 3 else { no("題と日が要ります") }
    guard let into = store.defaultCalendarForNewEvents else {
        no("書ける予定表がこの Mac にありません")
    }
    let at: String? = args.count >= 4 && !args[3].isEmpty ? args[3] : nil
    let f = DateFormatter()
    f.dateFormat = at == nil ? "yyyy-MM-dd" : "yyyy-MM-dd HH:mm"
    guard let start = f.date(from: at == nil ? args[2] : args[2] + " " + (at ?? "")) else {
        no("日付を読めませんでした")
    }
    let e = EKEvent(eventStore: store)
    e.calendar = into
    e.title = args[1]
    e.startDate = start
    if at == nil {
        e.isAllDay = true
        e.endDate = start
    } else {
        // 終わりの時刻（依頼 493）── 渡されなければ一時間後。
        let until: String? = args.count >= 5 && !args[4].isEmpty ? args[4] : nil
        if let u = until, let endAt = f.date(from: args[2] + " " + u), endAt > start {
            e.endDate = endAt
        } else {
            e.endDate = start.addingTimeInterval(3600)
        }
    }
    // メモ（依頼 523）── 足すときからタグを持てる。いちばん後ろに置くのは、
    // いままでの呼び方（題・日・開始・終了）を一つも変えないため。
    if args.count >= 6 && !args[5].isEmpty { e.notes = args[5] }
    do { try store.save(e, span: .thisEvent) } catch { no(error.localizedDescription) }
    out(["ok": true, "id": e.eventIdentifier ?? ""])

case "rename":
    need()
    guard args.count >= 3, let e = store.event(withIdentifier: args[1]) else {
        no("その予定は、もうありません")
    }
    e.title = args[2]
    do { try store.save(e, span: .thisEvent) } catch { no(error.localizedDescription) }
    out(["ok": true])

// **メモ欄を丸ごと入れ替える。** タグの行だけを差し替えた字は、呼ぶ側が
// core（`caltag::set`）に作らせて持ってくる ── ここは OS への口だけで、
// 「どこを書き換えるか」の判断は持たない。
case "notes":
    need()
    guard args.count >= 3, let e = store.event(withIdentifier: args[1]) else {
        no("その予定は、もうありません")
    }
    // **空にもできる。** タグを全部外すと、メモが空文字になることがある。
    e.notes = args[2].isEmpty ? nil : args[2]
    do { try store.save(e, span: .thisEvent) } catch { no(error.localizedDescription) }
    out(["ok": true])

case "drop":
    need()
    guard args.count >= 2, let e = store.event(withIdentifier: args[1]) else {
        no("その予定は、もうありません")
    }
    do { try store.remove(e, span: .thisEvent) } catch { no(error.localizedDescription) }
    out(["ok": true])

default:
    no("知らない操作: \(what)")
}

func one(_ e: EKEvent, _ day: String, _ at: String?, _ to: String?) -> [String: Any] {
    [
        "day": day,
        "at": at as Any,
        "to": to as Any,
        "title": e.title ?? "（題なし）",
        "id": e.eventIdentifier ?? "",
        "place": e.location ?? "",
        "from": e.calendar?.title ?? "",
        // **メモ欄をそのまま渡す。** 誰の用事かのタグはこの中の最後の行に
        // 置いてあるが、**それを読む判断は core（`caltag`）の仕事** ──
        // ここで切り出すと、窓と電話で二つの読み方ができる。
        "notes": e.notes ?? "",
    ]
}
