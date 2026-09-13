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
    /// グループカレンダーの名前（依頼 530）。**窓が作り、電話は見るだけ。**
    private static let groupKey = "amber.calGroupName"
    /// 何を出しているか ── `me` / `group` / `both`。
    private static let sideKey = "amber.calSide"
    private static let groupIdKey = "amber.calGroupId"
    private static let groupAskedKey = "amber.calGroupAsked"
    private static let tagsKey = "amber.calTagsKnown"

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

    /// 窓の `CAL_COLORS` と同じ八色・同じ順（依頼 540）。
    static let colors: [(hex: String, name: String)] = [
        ("#e0669c", "ローズ"), ("#d9a400", "アンバー"), ("#2f8a52", "リーフ"),
        ("#8e5cb3", "バイオレット"), ("#1fa3a3", "シアン"), ("#c0392b", "カーマイン"),
        ("#7a5c3a", "セピア"), ("#5a6b7f", "スレート"),
    ]
    static func colorName(_ hex: String) -> String { colors.first { $0.hex == hex }?.name ?? hex }
    /// グループカレンダーの名前（無ければ空）。**窓が作ったものを、電話は
    /// 端末のカレンダー越しに見る** ── Google のアカウントが iPhone に足して
    /// あれば、作った翌日には降りてきている。
    static var groupName: String {
        get { UserDefaults.standard.string(forKey: groupKey) ?? "" }
        set { UserDefaults.standard.set(newValue, forKey: groupKey) }
    }

    /// グループカレンダーの id（招待の画面を開くのに使う）。
    static var groupId: String {
        get { UserDefaults.standard.string(forKey: groupIdKey) ?? "" }
        set { UserDefaults.standard.set(newValue, forKey: groupIdKey) }
    }

    /// 見つけたグループカレンダーについて、一度訊いたか（依頼 538）。
    /// **断った人に毎回訊かない。**
    static var groupAsked: Bool {
        get { UserDefaults.standard.bool(forKey: groupAskedKey) }
        set { UserDefaults.standard.set(newValue, forKey: groupAskedKey) }
    }

    /// **招待された側が、グループカレンダーを見つける**（依頼 538）。
    ///
    /// 作った端末は名前を憶えているが、**招待された人の amber は何も知らない**
    /// ── カレンダーは端末に降りてくるのに、amber から見ると「よその予定表」と
    /// 見分けが付かず、絞り込みも色分けも出ない。名前が決まっているので、
    /// 端末の予定表にその名前があれば見つけられる。
    static let groupWord = "ambər グループ"
    static func foundGroup() -> Bool {
        groupName.isEmpty && !groupAsked && Phone.calendars.contains(groupWord)
    }

    /// この端末が見たことのあるタグ（依頼 545・窓の `tagsKnown` と同じ）。
    /// **選ばせるために憶えておく** ── 毎回名前を打たせない。
    static var tagsKnown: [String] {
        get { UserDefaults.standard.stringArray(forKey: tagsKey) ?? [] }
        set { UserDefaults.standard.set(newValue, forKey: tagsKey) }
    }
    /// そのタグの色（依頼 545・窓の `tagColor` と同じ五色・同じ配り方）。
    /// **決めていなければ、順に配る** ── 設定を開かなくても色分けされた表が
    /// 見られるほうがよい。`among` は憶えている順で、窓の `tagOrder` にあたる。
    static let laneColors = ["#e0669c", "#d9a400", "#2f8a52", "#8e5cb3", "#1fa3a3"]
    static func laneColor(_ t: String, among all: [String]) -> String {
        let i = all.firstIndex(of: t) ?? 0
        return laneColors[i % laneColors.count]
    }

    static func rememberTag(_ t: String) {
        var all = tagsKnown
        guard !t.isEmpty, !all.contains(t) else { return }
        all.append(t)
        tagsKnown = all
    }

    /// 何を出しているか（`me` / `group` / `both`）。**既定は両方**。
    static var side: String {
        get { UserDefaults.standard.string(forKey: sideKey) ?? "both" }
        set { UserDefaults.standard.set(newValue, forKey: sideKey) }
    }

    /// その予定は、グループカレンダーのものか（依頼 530）。
    ///
    /// **予定表の名前で当てる** ── 窓の `inGroup` とまったく同じ決まり。
    /// ここが窓とずれると、同じ予定が端末によって違う顔をする。
    static func inGroup(_ s: Calendaring.Slot) -> Bool {
        !groupName.isEmpty && s.isPhone && s.from == groupName
    }

    static var hereTint: Color { Color(hex: hereColor.isEmpty ? "#2f8a52" : hereColor) ?? .green }

    /// その予定はどの予定表のものか（窓の `whoOf` と同じ鍵）。
    static func key(of s: Calendaring.Slot) -> String {
        if s.kind == "away" { return "away:" + s.from }
        if s.kind == "here" { return "here:" + s.from }
        return "me"
    }
    /// 出すか。引っ込めた予定表を落とし、**自分だけ／グループの絞り込み**も効かせる。
    static func visible(_ s: Calendaring.Slot) -> Bool {
        if hide.contains(key(of: s)) { return false }
        if groupName.isEmpty || side == "both" { return true }
        return side == "group" ? inGroup(s) : !inGroup(s)
    }

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
