import SwiftUI
import UserNotifications

/// What a note asked to be reminded about.
struct Reminder: Equatable {
    var once: String = ""
    /// "", "daily", "weekly", "monthly"
    var kind: String = ""
    /// Weekday (0 = Monday) or day of month.
    var n: Int = 0
    var hour: Int = 9
    var minute: Int = 0
    var last: String = ""
    /// Days the routine came due and has not been carried out.
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

    /// How the note writes it down.
    var repeatLine: String? {
        switch kind {
        case "daily": return String(format: "daily %02d:%02d", hour, minute)
        case "weekly": return String(format: "weekly %@ %02d:%02d", Bell.dayNames[n], hour, minute)
        case "monthly": return String(format: "monthly %d %02d:%02d", n, hour, minute)
        default: return nil
        }
    }
}

/// The phone's own alarm clock.
///
/// **cian does not have a clock.** iOS will not wake a sandboxed app at nine
/// on a Wednesday to write a file, and pretending otherwise would mean a
/// routine that silently only happens when you open the app. So: the
/// *notification* is scheduled with the system and arrives on time, and the
/// note it stands for is written the next time cian is opened — which is what
/// `last` in the note is for.
@MainActor
enum Bell {
    static let dayNames = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
    static let dayLabels = ["月", "火", "水", "木", "金", "土", "日"]

    /// Ask once. Refused is an answer, not a failure to retry at every launch.
    static func ask() async -> Bool {
        let c = UNUserNotificationCenter.current()
        if let granted = try? await c.requestAuthorization(options: [.alert, .sound, .badge]) {
            return granted
        }
        return false
    }

    /// Put this note's reminders on the system's clock, replacing whatever
    /// was there for it.
    ///
    /// Keyed by the note's path so re-scheduling replaces rather than stacks:
    /// a note edited five times should not ring five times.
    static func set(_ note: Note, _ r: Reminder) {
        let c = UNUserNotificationCenter.current()
        let ids = ["\(note.path)#once", "\(note.path)#every"]
        c.removePendingNotificationRequests(withIdentifiers: ids)

        let body = UNMutableNotificationContent()
        body.title = note.title
        body.body = note.excerpt.isEmpty ? "amber" : note.excerpt
        body.sound = .default
        // Which note this was about. Without it a notification can only say
        // "open amber", and the person then has to find the note the phone
        // just interrupted them about.
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
                // `DateComponents.weekday` counts Sunday as 1; the note counts
                // Monday as 0. Getting this wrong moves every routine by a day
                // and nothing on screen says so.
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

/// The note a notification was about, from the moment it is pressed until
/// the list has opened it.
///
/// A tiny object rather than a callback: the notification can be pressed
/// while the app is not running, in which case the answer arrives *before*
/// there is a view to hand it to. Somewhere to put it until somebody asks is
/// the whole job.
@MainActor
final class Ring: NSObject, ObservableObject, UNUserNotificationCenterDelegate {
    static let shared = Ring()
    /// The path pressed, cleared by whoever acts on it.
    @Published var wanted: String?

    /// Start listening. Must happen before the app finishes launching, or a
    /// notification pressed from the lock screen is delivered to nobody.
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

    /// Show it even while cian is open. The alternative is a routine that
    /// comes due, rings nowhere, and looks broken.
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .sound]
    }
}
