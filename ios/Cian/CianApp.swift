import SwiftUI

@main
struct CianApp: App {
    // Before any view exists: a notification pressed from the lock screen
    // arrives while the app is still starting, and a delegate set later
    // never hears about it.
    init() { Ring.listen() }

    /// Light, dark, or whatever the phone is doing. **Three, not two** — a
    /// switch with only light and dark is a switch you can never put back.
    @AppStorage("cian.look") private var look = Look.auto

    var body: some Scene {
        WindowGroup {
            ContentView()
                // Cyan, because that is what the app is called and what its
                // icon is. One accent through the whole app rather than a
                // colour per screen: the tint is how you tell what can be
                // touched, and a different answer on every screen is no
                // answer.
                .tint(Color("AccentColor"))
                .preferredColorScheme(look.scheme)
        }
    }
}

/// What the app looks like, and where that is remembered.
enum Look: String, CaseIterable, Identifiable {
    case auto, light, dark
    var id: String { rawValue }
    var label: String {
        switch self {
        case .auto: return "iPhone に合わせる"
        case .light: return "ライト"
        case .dark: return "ダーク"
        }
    }
    var scheme: ColorScheme? {
        switch self {
        case .auto: return nil
        case .light: return .light
        case .dark: return .dark
        }
    }
}
