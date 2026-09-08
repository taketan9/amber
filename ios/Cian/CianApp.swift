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
    /// 字の大きさ。**窓と同じものを電話にも**（窓は ⌘+ / ⌘−）── 本人が
    /// 「文字が全般的に小さくない？」（2026-09-08）。
    @AppStorage("amber.font") private var font = Size.system

    var body: some Scene {
        WindowGroup {
            ContentView()
                // **選んでいないなら、iPhone の設定に従う。** 大きい字に
                // している人の設定を、こちらが上書きしない ── 上書きすると
                // 「iPhone を大きくしたのに amber だけ小さい」になる。
                .modifier(Sized(size: font))
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

/// 字の大きさ。**「iPhone に合わせる」を既定にする** ── 大きい字にして
/// いる人の設定を、こちらが黙って上書きしない。
enum Size: String, CaseIterable, Identifiable {
    case small, system, big, bigger, biggest
    var id: String { rawValue }
    var label: String {
        switch self {
        case .small: return "小さめ"
        case .system: return "iPhone に合わせる"
        case .big: return "大きめ"
        case .bigger: return "もっと大きく"
        case .biggest: return "いちばん大きく"
        }
    }
    /// SwiftUI の字の段。`nil` は「触らない」（iPhone の設定のまま）。
    var step: DynamicTypeSize? {
        switch self {
        case .small: return .small
        case .system: return nil
        case .big: return .xLarge
        case .bigger: return .xxLarge
        case .biggest: return .xxxLarge
        }
    }
    /// 「表示」の面（`WKWebView`）の字。**あちらは SwiftUI の段を知らない**
    /// ので、同じ増え方を px で渡す ── 一覧だけ大きくなって本文が小さい
    /// ままだと、「大きくした」が半分しか効いていない。
    var px: Int {
        switch self {
        case .small: return 15
        case .system: return 17
        case .big: return 19
        case .bigger: return 21
        case .biggest: return 24
        }
    }
}

/// 選んだ段があるときだけ、字の大きさを差し替える薄い包み。
struct Sized: ViewModifier {
    let size: Size
    func body(content: Content) -> some View {
        if let step = size.step { content.dynamicTypeSize(step) } else { content }
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
