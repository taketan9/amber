import SwiftUI

/// **カレンダー表示設定**（依頼 515・窓の `cmdCalSettings` と同じ三つ）──
/// どの予定表を出すか・土日を出すか・個人カレンダーの色。
///
/// 窓と同じ鍵（`me` / `here:<予定表>` / `away:<名前>`）で持つが、**この端末の中だけ**
/// ── 見え方の好みは端末ごとで、ノートと一緒に旅をさせない（窓も `remember`）。
@MainActor
enum CalPrefs {
    private static let hideKey = "amber.calHide"
    private static let weekendKey = "amber.calWeekend"
    private static let colorKey = "amber.calHereColor"
    private static let viewKey = "amber.calView"

    static var hide: [String] {
        get { UserDefaults.standard.stringArray(forKey: hideKey) ?? [] }
        set { UserDefaults.standard.set(newValue, forKey: hideKey) }
    }
    static var weekend: Bool {
        get { UserDefaults.standard.object(forKey: weekendKey) as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: weekendKey) }
    }
    /// 個人カレンダー（この端末の予定表）の色。空なら緑（既定・窓と同じ）。
    static var hereColor: String {
        get { UserDefaults.standard.string(forKey: colorKey) ?? "" }
        set { UserDefaults.standard.set(newValue, forKey: colorKey) }
    }
    /// 月／週／日（窓の `calView` と同じ三つ）。
    static var view: String {
        get { UserDefaults.standard.string(forKey: viewKey) ?? "month" }
        set { UserDefaults.standard.set(newValue, forKey: viewKey) }
    }

    /// 窓の `CAL_COLORS` と同じ十色・同じ順。
    static let colors: [(hex: String, name: String)] = [
        ("#e8702a", "オレンジ"), ("#3b78c9", "青"), ("#e0669c", "ピンク"), ("#d9a400", "黄"),
        ("#2f8a52", "緑"), ("#8e5cb3", "紫"), ("#1fa3a3", "水色"), ("#c0392b", "赤"),
        ("#7a5c3a", "茶"), ("#5a6b7f", "灰"),
    ]
    static func colorName(_ hex: String) -> String { colors.first { $0.hex == hex }?.name ?? hex }
    static var hereTint: Color { Color(hex: hereColor.isEmpty ? "#2f8a52" : hereColor) ?? .green }

    /// その予定はどの予定表のものか（窓の `whoOf` と同じ鍵）。
    static func key(of s: Calendaring.Slot) -> String {
        if s.kind == "away" { return "away:" + s.from }
        if s.kind == "here" { return "here:" + s.from }
        return "me"
    }
    static func visible(_ s: Calendaring.Slot) -> Bool { !hide.contains(key(of: s)) }

    /// 出し入れできる予定表の一覧（窓の `calSources` と同じ並び）。
    static func sources() -> [(key: String, name: String)] {
        var out: [(key: String, name: String)] = [("me", "自分のノート（日付を書いたノート）")]
        for c in Phone.calendars { out.append(("here:" + c, c + "（この iPhone）")) }
        for f in Away.feeds { out.append(("away:" + f.name, f.name + "（カレンダー設定追加で足したもの）")) }
        // 隠しているのに一覧に無いもの（もう無い予定表）も出す ── 戻せないと困る。
        for k in hide where !out.contains(where: { $0.key == k }) {
            out.append((k, String(k.drop(while: { $0 != ":" }).dropFirst())))
        }
        return out
    }
}

/// カレンダー表示設定の紙。
struct CalSettings: View {
    /// 変えたら呼ぶ（表を描き直す）。
    let changed: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var hide = CalPrefs.hide
    @State private var weekend = CalPrefs.weekend
    @State private var color = CalPrefs.hereColor

    var body: some View {
        NavigationStack {
            List {
                Section {
                    ForEach(CalPrefs.sources(), id: \.key) { s in
                        Toggle(s.name, isOn: Binding(
                            get: { !hide.contains(s.key) },
                            set: { on in
                                if on { hide.removeAll { $0 == s.key } } else if !hide.contains(s.key) { hide.append(s.key) }
                                CalPrefs.hide = hide
                                changed()
                            }))
                    }
                } header: {
                    Text("出す予定表")
                } footer: {
                    Text("切ったものは表から消えるだけで、予定そのものは消えません")
                }
                Section {
                    Toggle("土日表示", isOn: Binding(get: { weekend }, set: { weekend = $0; CalPrefs.weekend = $0; changed() }))
                } footer: {
                    Text(weekend ? "" : "月〜金だけ出しています")
                }
                Section("カラー設定（個人カレンダーの色）") {
                    ForEach(CalPrefs.colors, id: \.hex) { c in
                        Button {
                            color = c.hex
                            CalPrefs.hereColor = c.hex
                            changed()
                        } label: {
                            HStack {
                                Circle().fill(Color(hex: c.hex) ?? .gray).frame(width: 14, height: 14)
                                Text(c.name).foregroundStyle(.primary)
                                Spacer()
                                if c.hex == (color.isEmpty ? "#2f8a52" : color) { Image(systemName: "checkmark").foregroundStyle(.tint) }
                            }
                        }
                    }
                }
            }
            .navigationTitle("カレンダー表示設定")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("閉じる") { dismiss() } } }
        }
    }
}
