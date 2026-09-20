import SwiftUI
import UserNotifications

/// そのノートが通知してほしいと言っている内容。
struct Reminder: Equatable {
    var once: String = ""
    /// "", "daily", "weekly", "monthly"
    var kind: String = ""
    /// 曜日（0 = 月曜）または月の何日か。
    var n: Int = 0
    var hour: Int = 9
    var minute: Int = 0
    var last: String = ""
    /// 繰り返しの期日が来て、まだ実行していない日。
    var due: [String] = []

    var repeats: Bool { !kind.isEmpty }

    init() {}

    init(_ o: [String: Any]) {
        once = o["once"] as? String ?? ""
        last = o["last"] as? String ?? ""
        due = o["due"] as? [String] ?? []
        if let e = o["every"] as? [String: Any] {
            kind = e["kind"] as? String ?? ""
            n = e["n"] as? Int ?? 0
            hour = e["hour"] as? Int ?? 9
            minute = e["minute"] as? Int ?? 0
        }
    }

    /// ノートへの書き方。
    var repeatLine: String? {
        switch kind {
        case "daily": return String(format: "daily %02d:%02d", hour, minute)
        case "weekly": return String(format: "weekly %@ %02d:%02d", Bell.dayNames[n], hour, minute)
        case "monthly": return String(format: "monthly %d %02d:%02d", n, hour, minute)
        default: return nil
        }
    }
}

/// iPhone 自身の目覚まし。
///
/// **amber は時計を持っていない。** iOS はサンドボックスの中のアプリを水曜の
/// 9 時に起こしてファイルを書かせたりしない。持っているふりをすると、
/// 繰り返しは黙って「アプリを開いたときだけ」起きることになる。だから ──
/// *通知*は OS に登録して時間どおりに届き、それが表すノートは次に amber を
/// 開いたときに書く。ノートの中の `last` はそのためにある。
///
@MainActor
enum Bell {
    static let dayNames = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
    static let dayLabels = ["月", "火", "水", "木", "金", "土", "日"]

    /// 許可を求めるのは一度だけ。断られたのも答えであって、起動のたびに
    static func ask() async -> Bool {
        let c = UNUserNotificationCenter.current()
        if let granted = try? await c.requestAuthorization(options: [.alert, .sound, .badge]) {
            return granted
        }
        return false
    }

    /// このノートの通知を OS に登録する。そのノートについて登録済みのものは
    /// 置き換える。
    ///
    /// キーはノートのパス。登録し直しても積み上がらず置き換わる ── 5 回編集
    /// したノートが 5 回鳴ってはいけない。
    static func set(_ note: Note, _ r: Reminder) {
        let c = UNUserNotificationCenter.current()
        let ids = ["\(note.path)#once", "\(note.path)#every"]
        c.removePendingNotificationRequests(withIdentifiers: ids)

        let body = UNMutableNotificationContent()
        body.title = note.title
        body.body = note.excerpt.isEmpty ? "ambər" : note.excerpt
        body.sound = .default
        // どのノートについての通知か。これが無いと通知は「amber を開いて」と
        // しか言えず、たったいま邪魔してきた当のノートを自分で探すことに
        // なる。
        body.userInfo = ["path": note.path]

        if !r.once.isEmpty, let at = parts(r.once) {
            let t = UNCalendarNotificationTrigger(dateMatching: at, repeats: false)
            c.add(UNNotificationRequest(identifier: ids[0], content: body, trigger: t))
        }
        if r.repeats {
            var when = DateComponents()
            when.hour = r.hour
            when.minute = r.minute
            switch r.kind {
            case "weekly":
                // `DateComponents.weekday` は日曜を 1 と数え、ノート側は月曜を 0 と
                // 数える。ここを間違えると繰り返しが全部 1 日ずれるが、画面には
                // 何も出ない。
                when.weekday = (r.n + 1) % 7 + 1
            case "monthly":
                when.day = r.n
            default: break
            }
            let t = UNCalendarNotificationTrigger(dateMatching: when, repeats: true)
            c.add(UNNotificationRequest(identifier: ids[1], content: body, trigger: t))
        }
    }

    static func clear(_ note: Note) {
        UNUserNotificationCenter.current().removePendingNotificationRequests(
            withIdentifiers: ["\(note.path)#once", "\(note.path)#every"]
        )
    }

    private static func parts(_ s: String) -> DateComponents? {
        let bits = s.split(whereSeparator: { $0 == " " || $0 == "-" || $0 == ":" }).map(String.init)
        guard bits.count >= 5,
              let y = Int(bits[0]), let mo = Int(bits[1]), let d = Int(bits[2]),
              let h = Int(bits[3]), let mi = Int(bits[4]) else { return nil }
        var c = DateComponents()
        c.year = y; c.month = mo; c.day = d; c.hour = h; c.minute = mi
        return c
    }
}

/// 通知が指していたノートを、押された瞬間から一覧が開くまで
/// 預かっておく。
///
/// コールバックではなく小さなオブジェクトにしてある ── 通知はアプリが
/// 動いていないときにも押され、そのとき答えは渡す先の View ができる*前*に
/// 届く。誰かが訊きに来るまで置いておく場所を作るのが、ここの仕事の
/// すべて。
@MainActor
final class Ring: NSObject, ObservableObject, UNUserNotificationCenterDelegate {
    static let shared = Ring()
    /// 押されたノートのパス。処理した側が消す。
    @Published var wanted: String?

    /// 受け取りを始める。アプリの起動が終わる前に設定しなければならない ──
    /// ロック画面から押された通知が、誰にも届かなくなる。
    static func listen() {
        UNUserNotificationCenter.current().delegate = shared
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse
    ) async {
        let path = response.notification.request.content.userInfo["path"] as? String
        await MainActor.run { Ring.shared.wanted = path }
    }

    /// amber を開いているあいだも通知を出す。そうしないと、期日が来ても
    /// どこにも鳴らず、壊れているように見える。
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .sound]
    }
}
