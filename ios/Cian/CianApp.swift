import SwiftUI

@main
struct CianApp: App {
    // View ができる前に設定する ── ロック画面から押された通知は、アプリが
    // まだ起動している途中に届くので、あとから設定した delegate には
    // 一度も届かない。
    init() { Ring.listen() }

    /// ライト、ダーク、または端末の設定に従う。**2 つではなく 3 つ** ──
    /// ライトとダークだけのスイッチは、二度と元に戻せないスイッチになる。
    @AppStorage("cian.look") private var look = Look.auto
    /// 文字の大きさ。**デスクトップ版と同じものをiPhone にも**（デスクトップ版は ⌘+ / ⌘−）── 本人が
    /// 「文字が全般的に小さくない？」（2026-09-08）。
    @AppStorage("amber.font") private var font = Size.system
    /// 配色（依頼 499）── cian と同じ二十一。空なら琥珀（明暗は `look`）。
    @AppStorage("amber.palette") private var palette = ""
    private var chosen: Palette? { Palettes.named(palette) }

    var body: some Scene {
        WindowGroup {
            // **総ざらいのときは、画面を出さない**（依頼 448）── 出すと
            // 一覧が勝手に読み直して、走査の見ている姿が動く。
            content
                // **選んでいないなら、iPhone の設定に従う。** 大きい文字に
                // している人の設定を、こちらが上書きしない ── 上書きすると
                // 「iPhone を大きくしたのに amber だけ小さい」になる。
                .modifier(Sized(size: font))
                // シアン。アプリの名前もアイコンもそれだから。画面ごとに
                // 色を変えず、アプリ全体で 1 色にする ── 色は「触れるもの」を
                // 見分ける手がかりで、画面ごとに答えが違えば
                // touched, and a different answer on every screen is no
                // answer.
                // 配色を選んでも tint はシアンのまま（依頼 75・色は一つ、置き場所も一つ）。
                .tint(Color("AccentColor"))
                .preferredColorScheme(chosen.map { $0.light ? ColorScheme.light : .dark } ?? look.scheme)
        }
    }

    @ViewBuilder private var content: some View {
        #if DEBUG
        if Walk.asked {
            Color.clear.task { await Walk.run() }
        } else {
            ContentView()
        }
        #else
        ContentView()
        #endif
    }
}

/// 文字の大きさ。**「iPhone に合わせる」を既定にする** ── 大きい文字にして
/// いる人の設定を、こちらが黙って上書きしない。
enum Size: String, CaseIterable, Identifiable {
    case small, system, big, bigger, biggest
    var id: String { rawValue }
    var label: String {
        switch self {
        case .small: return "小さめ"
        case .system: return "OS に合わせる"
        case .big: return "大きめ"
        case .bigger: return "もっと大きく"
        case .biggest: return "いちばん大きく"
        }
    }
    /// SwiftUI の文字の段。`nil` は「触らない」（iPhone の設定のまま）。
    var step: DynamicTypeSize? {
        switch self {
        case .small: return .small
        case .system: return nil
        case .big: return .xLarge
        case .bigger: return .xxLarge
        case .biggest: return .xxxLarge
        }
    }
    /// 「表示」画面（`WKWebView`）の文字。**あちらは SwiftUI の段を知らない**
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

/// 選んだ段があるときだけ、文字の大きさを差し替える薄い包み。
struct Sized: ViewModifier {
    let size: Size
    func body(content: Content) -> some View {
        if let step = size.step { content.dynamicTypeSize(step) } else { content }
    }
}

/// アプリの見た目と、それをどこに記録しているか。
enum Look: String, CaseIterable, Identifiable {
    case auto, light, dark
    var id: String { rawValue }
    var label: String {
        switch self {
        case .auto: return "OS に合わせる"
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
