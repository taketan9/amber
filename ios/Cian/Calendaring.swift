import SwiftUI

/// **カレンダー**（依頼 454・電話の側）。窓と同じ形。
///
/// 月の表に予定を並べ、日を押すとその日が下に開く。**予定を押したら
/// ノートへ** ── 予定は入口で、書くのはノート。**何もないところを押したら
/// 足せる**（足すのは新しいノートで、日付は前書きの `remind:` に書く ──
/// 依頼 73 の語彙のまま、カレンダーのためだけの置き場所を作らない）。
///
/// 数えるのは core（`month`）で、**窓と同じ一組** ── 同じフォルダを
/// 両方から見ても、同じ予定が同じ日に並ぶ。
struct Calendaring: View {
    @ObservedObject var store: NotesStore
    /// 押されたノートを開く先（一覧の側が引き受ける）。
    let open: (Note) -> Void
    @Environment(\.dismiss) private var dismiss

    struct Slot: Identifiable, Hashable {
        let day: String
        let at: String?
        let title: String
        /// よその予定表のものには、開く先が無い。
        let path: String
        let kind: String
        /// よその予定の場所と、どの予定表から来たか。
        var place: String = ""
        var from: String = ""
        var id: String { day + (at ?? "") + path + kind + title }
        var isPlan: Bool { kind != "note" }
        var isAway: Bool { kind == "away" }
        /// この iPhone の予定表のもの ── **こちらは直せる**。
        /// この端末の予定表のもの ── **こちらは直せる**。
        var isPhone: Bool { kind == "here" }
        /// ノートを開く先を持たないもの（よそ・この iPhone）。
        var noNote: Bool { kind == "away" || kind == "here" }
    }

    /// 何を出しているか（依頼 530）── `me` / `group` / `both`。
    @State private var side = CalPrefs.side
    /// グループカレンダーの名前（依頼 534）。**`@State` で持つ。**
    ///
    /// `CalPrefs`（`UserDefaults`）を画面から直に読んでいたせいで、作った
    /// 直後に「グループへ招待」が出てこなかった ── **SwiftUI に「変わった」
    /// が伝わらない**ので、別の画面へ行って戻るまで描き直されない。
    /// 憶える先は `CalPrefs` のままで、**画面が見るのはこちら**。
    @State private var groupName = CalPrefs.groupName
    /// グループを作る前の確認と、作っているあいだ（依頼 532）。
    @State private var making = false
    @State private var busy = false
    @State private var made = ""
    /// 招待の行き方を出しているところ（依頼 534）。
    @State private var inviting = false
    /// 消す前の確認（依頼 535）。
    @State private var dropping = false

    /// **開くたびに今月へ戻さない。** 先の予定を見にきた人を、
    /// 閉じて開くたびに今日へ連れ戻さない。
    @State private var year = Calendar.current.component(.year, from: Date())
    @State private var month = Calendar.current.component(.month, from: Date())
    @State private var picked = Calendaring.today
    @State private var slots: [Slot] = []
    @State private var trouble: String?
    @State private var adding = false
    @State private var newTitle = ""
    @State private var newAt = "09:00"
    @State private var newEnd = ""
    /// 直している、この iPhone の予定。
    @State private var editing: Slot?
    @State private var editTitle = ""
    /// 月／週／日（依頼 515・窓と同じ三つ）。
    @State private var mode = CalPrefs.view
    @State private var settings = false
    /// 表示設定を変えたら描き直すための数。
    @State private var prefsTick = 0

    static var today: String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: Date())
    }

    var body: some View {
        NavigationStack { inside }
            .task {
                // **押されて開いたときに訊く。** 起きた瞬間に訊くと、
                // 何のために訊かれたのか分からないまま断られる。
                if !Phone.asked { await Phone.ask() }
                count()
            }
    }

    /// **一つの `body` に積み上げない。** 積むと Swift が型を追いきれず、
    /// 「時間内に型検査できません」で組めなくなる（実際になった）。
    private var inside: some View {
        VStack(spacing: 0) {
            // 月／週／日の切り替え（依頼 515）── 窓の表の上の三つと同じ。
            // **何を出すかは、その右に一つ**（依頼 531・本人が絵を見て案ウ）──
            // 段をもう一つ足すと月の表が 85pt 下がる（画面の一割）。電話は
            // 縦が命なので、横に置いて段を増やさない。
            HStack(spacing: 8) {
                Picker("表示", selection: $mode) {
                    Text("月").tag("month")
                    Text("週").tag("week")
                    Text("日").tag("day")
                }
                .pickerStyle(.segmented)
                .onChange(of: mode) { _, now in CalPrefs.view = now; count() }
                if !groupName.isEmpty { sidePill }
            }
            .padding(.horizontal, 12).padding(.top, 6)
            if mode == "month" {
                grid
                Divider()
                day
            } else if mode == "week" {
                week
            } else {
                day
            }
        }
        // **`Text` に数をそのまま渡さない。** SwiftUI は土地の決まりで
        // 桁を区切るので、年が「2,026年」になる（実際になった）。
        .navigationTitle(Text(verbatim: heading))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { bar }
        .sheet(isPresented: $settings) {
            CalSettings(changed: { prefsTick += 1; count() })
        }
        .id(prefsTick)
        // 作る前の確認と、作ったあとの知らせ（依頼 532）。
        .alert("グループを作りますか", isPresented: $making) {
            Button("作る") { makeGroup() }
            Button("やめる", role: .cancel) { }
        } message: {
            Text("「ambər グループ」というカレンダーが一枚できます。"
                 + "そこに入れた予定だけが、招待した人に見えます。\n\n"
                 + "いまの予定は一つも動きません。")
        }
        .alert("グループカレンダーを消しますか", isPresented: $dropping) {
            Button("消す", role: .destructive) { dropGroup() }
            Button("やめる", role: .cancel) { }
        } message: {
            Text("「\(groupName)」を消します。\n\n"
                 + "グループの人の画面からも消えます。中の予定も一緒に消えて、戻せません。")
        }
        // **行き方を言う**（依頼 534）── iPhone では amber の中から招待できない。
        .alert("グループへ招待", isPresented: $inviting) {
            Button("Google カレンダーを開く") { openGoogleCalendar() }
            Button("閉じる", role: .cancel) { }
        } message: {
            Text("いまは Google カレンダーのアプリで招待します。\n\n"
                 + "1. 左上のメニュー → 設定\n"
                 + "2. 「\(groupName)」を選ぶ\n"
                 + "3. 「ユーザーまたはグループを追加」\n\n"
                 + "amber の中で招待できるようにするには Google の審査が要ります。"
                 + "一般公開のときに通します。")
        }
        .alert("できました", isPresented: Binding(get: { !made.isEmpty }, set: { if !$0 { made = "" } })) {
            Button("わかりました") { made = "" }
        } message: { Text(made) }
        .modifier(Asking(
            trouble: $trouble, adding: $adding, editing: $editing,
            newTitle: $newTitle, newAt: $newAt, newEnd: $newEnd, editTitle: $editTitle,
            day: spoken(picked), toPhone: Phone.allowed,
            add: add, rename: rename, drop: drop))
    }

    @ToolbarContentBuilder private var bar: some ToolbarContent {
        ToolbarItem(placement: .cancellationAction) {
            Button("閉じる") { dismiss() }
        }
        ToolbarItemGroup(placement: .topBarTrailing) {
            Button { settings = true } label: { Image(systemName: "gearshape") }
                .accessibilityLabel("カレンダー表示設定")
            Button { step(-1) } label: { Image(systemName: "chevron.left") }
            Button("今日") {
                year = Calendar.current.component(.year, from: Date())
                month = Calendar.current.component(.month, from: Date())
                picked = Self.today
                count()
            }
            Button { step(1) } label: { Image(systemName: "chevron.right") }
        }
    }

    // MARK: 月の表

    private var grid: some View {
        VStack(spacing: 3) {
            HStack(spacing: 3) {
                ForEach(Array(["月", "火", "水", "木", "金", "土", "日"].prefix(CalPrefs.weekend ? 7 : 5).enumerated()), id: \.offset) { i, w in
                    Text(w).font(.caption2)
                        .foregroundStyle(i == 5 ? Color.blue : (i == 6 ? Color.red : Color.secondary))
                        .frame(maxWidth: .infinity)
                }
            }
            ForEach(weeks, id: \.self) { week in
                HStack(spacing: 3) {
                    // 土日を出さないときは月〜金だけ（依頼 515）。
                    ForEach(Array(week.prefix(CalPrefs.weekend ? 7 : 5)), id: \.self) { d in cell(d) }
                }
            }
        }
        .padding(.horizontal, 8)
        .padding(.top, 6)
    }

    /// 何を出しているか（依頼 531）。**押すと回る。**
    ///
    /// 回る順は **両方 → 自分だけ → グループ → 両方**（本人）。
    /// 回る形の弱いところは「ほかに何が選べるか」が押すまで分からないこと
    /// なので、**長押しで三つ出す** ── 押し先が一つで済む軽さは残したまま、
    /// 全部を見る道も残す。
    private static let sides = ["both", "me", "group"]
    private func sideWord(_ k: String) -> String {
        ["both": "両方", "me": "自分だけ", "group": "グループ"][k] ?? "両方"
    }
    private func setSide(_ k: String) {
        side = k
        CalPrefs.side = k
        count()
    }
    private var sidePill: some View {
        Button {
            let i = Self.sides.firstIndex(of: side) ?? 0
            setSide(Self.sides[(i + 1) % Self.sides.count])
        } label: {
            Label(sideWord(side), systemImage: "person.2")
                .font(.footnote)
                .labelStyle(.titleAndIcon)
                .lineLimit(1)
                .padding(.horizontal, 10).padding(.vertical, 6)
                .background(Color(.secondarySystemBackground))
                .clipShape(Capsule())
        }
        .buttonStyle(.plain)
        .contextMenu {
            // **長押しで三つ出す。** 回る形だけだと、選べるものが分からない。
            ForEach(Self.sides, id: \.self) { k in
                Button { setSide(k) } label: {
                    if k == side { Label(sideWord(k), systemImage: "checkmark") }
                    else { Text(sideWord(k)) }
                }
            }
        }
    }

    /// グループカレンダーを一枚作る（依頼 532）。**押すのは一回。**
    ///
    /// サインインもカレンダーの許可も `Drive` が面倒を見る。作る前に
    /// 「いまの予定は一つも動きません」と言う ──「共有」を押した瞬間に
    /// 何が起きるか分からないのが、いちばん怖い。
    private func makeGroup() {
        busy = true
        Task {
            do {
                let got = try await Drive.shared.makeGroupCalendar(named: "ambər グループ")
                CalPrefs.groupName = got.name
                CalPrefs.groupId = got.id
                // **画面にも伝える。** ここを忘れると、作ったのに何も
                // 変わらないように見える（実機で出た・依頼 534）。
                groupName = got.name
                side = "group"
                CalPrefs.side = "group"
                // **二段あることを、その場で言う。** amber が作っただけでは
                // 誰にも届かない ── 招待は Google の画面で（一般公開のときに
                // amber の中へ入れる）。
                made = "「\(got.name)」を作りました。\n\nこのあと「グループへ招待」で、"
                    + "いっしょに使う人を呼べます。\n\n"
                    + "この iPhone のカレンダーに出てくるまで、少し時間がかかることがあります。"
                count()
            } catch {
                trouble = error.localizedDescription
            }
            busy = false
        }
    }

    /// グループへ招待する。**いまは Google カレンダーのアプリで**
    /// （依頼 534・実機で分かった）。
    ///
    /// はじめはパソコン版の設定ページの URL を開いていた。**URL は合っていた**
    /// （本人の本物のカレンダーで確かめた ── 組んだ字と Google が出す URL が
    /// 一字一句同じ）。**駄目だったのは、iPhone が `calendar.google.com` を
    /// Google カレンダーのアプリに渡してしまうこと** ── アプリは設定ページを
    /// 出せないので、「このカレンダーにアクセスするには…ログインしてください」
    /// とだけ出る。**URL の間違いではなく、行き先の取られ方の問題だった。**
    ///
    /// iPhone でも共有そのものはできる ── **アプリの設定から**。だから
    /// **URL を当てにいかず、行き方を言う**。amber の中で招待できるようにする
    /// には `calendar.acls`（審査の要る許可）が要るので、一般公開のときに通す。
    private func invite() { inviting = true }

    /// **グループカレンダーを消す**（依頼 535）。
    ///
    /// **本当に消える。** 持ち主が消すと、グループの人の画面からも消える ──
    /// 「amber から外す」ではない。戻す道がどこにも無いので、押す前にそう言う。
    private func dropGroup() {
        busy = true
        Task {
            do {
                try await Drive.shared.dropGroupCalendar(CalPrefs.groupId)
                let was = groupName
                CalPrefs.groupName = ""
                CalPrefs.groupId = ""
                groupName = ""
                side = "both"
                CalPrefs.side = "both"
                made = "「\(was)」を消しました。"
                count()
            } catch {
                trouble = error.localizedDescription
            }
            busy = false
        }
    }

    private func openGoogleCalendar() {
        if let u = URL(string: "https://calendar.google.com/") { UIApplication.shared.open(u) }
    }

    @ViewBuilder private func cell(_ d: String) -> some View {
        if d.isEmpty {
            Color.clear.frame(maxWidth: .infinity, minHeight: 46)
        } else {
            let mine = shown(on: d)
            let on = d == picked
            VStack(alignment: .leading, spacing: 1) {
                Text(String(Int(d.suffix(2)) ?? 0))
                    .font(.caption2).monospacedDigit()
                    .foregroundStyle(d == Self.today ? Color.accentColor : .secondary)
                    .fontWeight(d == Self.today ? .bold : .regular)
                ForEach(mine.prefix(2)) { s in
                    // **塗ってあるものが、グループに見えている予定**（依頼 530）。
                    // 窓とまったく同じ決まり ── 自分だけの予定は塗らない。
                    Text(s.title).font(.system(size: 8)).lineLimit(1)
                        .foregroundStyle(s.isAway ? Color.blue
                            : (s.isPhone ? CalPrefs.hereTint
                               : (s.isPlan ? Color.accentColor : Color.secondary)))
                        .padding(.horizontal, CalPrefs.inGroup(s) ? 3 : 0)
                        .padding(.vertical, CalPrefs.inGroup(s) ? 1 : 0)
                        .background(CalPrefs.inGroup(s)
                            ? CalPrefs.hereTint.opacity(0.22) : Color.clear)
                        .clipShape(RoundedRectangle(cornerRadius: 3))
                }
                if mine.count > 2 {
                    Text("ほか \(mine.count - 2)").font(.system(size: 8)).foregroundStyle(.tertiary)
                }
                Spacer(minLength: 0)
            }
            .padding(2)
            .frame(maxWidth: .infinity, minHeight: 46, alignment: .topLeading)
            .background(on ? Color.accentColor.opacity(0.12) : Color(.secondarySystemBackground))
            .overlay(RoundedRectangle(cornerRadius: 6)
                .stroke(d == Self.today ? Color.accentColor : .clear, lineWidth: 1))
            .clipShape(RoundedRectangle(cornerRadius: 6))
            .contentShape(Rectangle())
            .onTapGesture { picked = d }
        }
    }

    // MARK: その日

    private var day: some View {
        List {
            Section(spoken(picked)) {
                let plans = slots.filter { $0.day == picked && $0.isPlan && CalPrefs.visible($0) }
                if plans.isEmpty {
                    Text("予定はありません").foregroundStyle(.secondary).font(.footnote)
                }
                ForEach(plans) { s in row(s, time: true) }
            }
            // **予定に出ているノートを、下でもう一度出さない** ── 同じ一本が
            // 二度並ぶと、二つあるように見える。
            let said = Set(slots.filter { $0.day == picked && $0.isPlan && CalPrefs.visible($0) }.map(\.path))
            let notes = slots.filter { $0.day == picked && !$0.isPlan && !said.contains($0.path) && CalPrefs.visible($0) }
            if !notes.isEmpty {
                Section("この日に書いたノート") {
                    ForEach(notes) { s in row(s, time: false) }
                }
            }
            Section {
                Button {
                    newTitle = ""
                    newAt = "09:00"
                    newEnd = ""
                    adding = true
                } label: {
                    Label("予定を登録する", systemImage: "plus")
                }
                // **グループを始める道は、ここに置く**（依頼 532）── 設定の
                // 中ではない。狙いは IT に明るくない人で、**PC を持っていない
                // 可能性が大いにある**（本人）ので、電話で始められないのは
                // 致命的。作ったら消え、そのあとは「グループへ招待」になる。
                if groupName.isEmpty {
                    Button { making = true } label: {
                        Label("グループを作る", systemImage: "person.2.badge.plus")
                    }
                    .disabled(busy)
                } else if side == "group" {
                    Button { invite() } label: {
                        Label("グループへ招待", systemImage: "person.2")
                    }
                    // **作る道があるなら、やめる道もある**（依頼 535）。
                    // 一生に一度で戻せないので、赤くして、押す前に二度言う。
                    Button(role: .destructive) { dropping = true } label: {
                        Label("グループカレンダーを消す", systemImage: "trash")
                    }
                    .disabled(busy)
                }
            }
        }
        .listStyle(.insetGrouped)
    }

    @ViewBuilder private func row(_ s: Slot, time: Bool) -> some View {
        Button {
            // **よその予定にはノートが無い。** 押しても何も起きないより、
            // なぜ開かないかを言う。
            if s.isPhone {
                editing = s
                editTitle = s.title
                return
            }
            if s.isAway {
                trouble = "よその予定表のものなので、ここでは直せません。"
                return
            }
            guard let n = store.notes.first(where: { $0.path == s.path }) else { return }
            dismiss()
            open(n)
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                if time {
                    Text(s.at ?? "終日").font(.caption).monospacedDigit()
                        .foregroundStyle(.secondary).frame(width: 44, alignment: .leading)
                }
                VStack(alignment: .leading, spacing: 1) {
                    // 塗ってあるものが、グループに見えている予定（依頼 530）。
                    Text(s.title)
                        .foregroundStyle(s.isAway ? Color.blue
                            : (s.isPhone ? CalPrefs.hereTint : Color.primary))
                        .padding(.horizontal, CalPrefs.inGroup(s) ? 5 : 0)
                        .padding(.vertical, CalPrefs.inGroup(s) ? 2 : 0)
                        .background(CalPrefs.inGroup(s)
                            ? CalPrefs.hereTint.opacity(0.18) : Color.clear)
                        .clipShape(RoundedRectangle(cornerRadius: 5))
                    // **道は出さない** ── 読めない長さになるうえ、知りたいのは
                    // 中身のほう。自分のノートは一行目、よその予定は場所と出どころ。
                    if s.noNote {
                        let under = [s.place, s.from].filter { !$0.isEmpty }.joined(separator: "・")
                        if !under.isEmpty {
                            Text(under).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                        }
                    } else if let n = store.notes.first(where: { $0.path == s.path }),
                              !n.excerpt.isEmpty {
                        Text(n.excerpt).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
                    }
                }
            }
        }
        .buttonStyle(.plain)
    }

    // MARK: 数える・足す

    private func shown(on d: String) -> [Slot] {
        let mine = slots.filter { $0.day == d && CalPrefs.visible($0) }
        let plans = mine.filter(\.isPlan)
        let said = Set(plans.map(\.path))
        return plans + mine.filter { !$0.isPlan && !said.contains($0.path) }
    }

    private var weeks: [[String]] {
        var out: [[String]] = []
        var week: [String] = []
        var cal = Calendar(identifier: .gregorian)
        cal.firstWeekday = 2
        guard let first = cal.date(from: DateComponents(year: year, month: month, day: 1)),
              let span = cal.range(of: .day, in: .month, for: first)
        else { return [] }
        // 月曜はじまり ── 一覧の並びも週も、ここでは月曜から。
        let lead = (cal.component(.weekday, from: first) + 5) % 7
        week.append(contentsOf: Array(repeating: "", count: lead))
        for d in span {
            week.append(String(format: "%04d-%02d-%02d", year, month, d))
            if week.count == 7 { out.append(week); week = [] }
        }
        if !week.isEmpty {
            week.append(contentsOf: Array(repeating: "", count: 7 - week.count))
            out.append(week)
        }
        return out
    }

    private func spoken(_ d: String) -> String {
        guard d.count == 10 else { return d }
        let m = Int(d.dropFirst(5).prefix(2)) ?? 0
        let day = Int(d.suffix(2)) ?? 0
        return "\(m)月\(day)日"
    }

    private func rename() {
        guard let e = editing else { return }
        let title = editTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty else { return }
        do {
            try Phone.rename(e.path, to: title)
            count()
        } catch {
            trouble = error.localizedDescription
        }
    }

    private func drop() {
        guard let e = editing else { return }
        do {
            try Phone.drop(e.path)
            count()
        } catch {
            trouble = error.localizedDescription
        }
    }

    private func step(_ n: Int) {
        if mode == "week" || mode == "day" {
            picked = Self.shift(picked, n * (mode == "week" ? 7 : 1))
            year = Int(picked.prefix(4)) ?? year
            month = Int(picked.dropFirst(5).prefix(2)) ?? month
            count()
            return
        }
        var m = month + n
        var y = year
        if m < 1 { m = 12; y -= 1 }
        if m > 12 { m = 1; y += 1 }
        year = y
        month = m
        count()
    }

    // MARK: 週・日

    /// 選んだ日の週（月曜から）。土日を出さないなら月〜金。
    private var weekDays: [String] {
        guard let d = Self.date(picked) else { return [picked] }
        var cal = Calendar(identifier: .gregorian)
        cal.firstWeekday = 2
        let lead = (cal.component(.weekday, from: d) + 5) % 7
        let monday = Self.shift(picked, -lead)
        let n = CalPrefs.weekend ? 7 : 5
        return (0..<n).map { Self.shift(monday, $0) }
    }

    /// 上の題（月／週／日で違う）。
    private var heading: String {
        if mode == "week", let a = weekDays.first, let b = weekDays.last { return spoken(a) + " 〜 " + spoken(b) }
        if mode == "day" { return spoken(picked) + "（" + Self.weekName(picked) + "）" }
        return "\(year)年 \(month)月"
    }

    /// 週の表 ── 七日ぶんを段にして並べる（日ごとの予定と、登録する道）。
    private var week: some View {
        List {
            ForEach(weekDays, id: \.self) { d in
                Section {
                    let mine = shown(on: d)
                    if mine.isEmpty {
                        Text("予定はありません").foregroundStyle(.secondary).font(.footnote)
                    }
                    ForEach(mine) { s in row(s, time: true) }
                    Button {
                        picked = d
                        newTitle = ""
                        newAt = "09:00"
                        newEnd = ""
                        adding = true
                    } label: {
                        Label("予定を登録する", systemImage: "plus").font(.footnote)
                    }
                } header: {
                    Text(spoken(d) + "（" + Self.weekName(d) + "）" + (d == Self.today ? "　今日" : ""))
                        .foregroundStyle(d == Self.today ? Color.accentColor : Color.secondary)
                }
            }
        }
        .listStyle(.insetGrouped)
    }

    static func date(_ d: String) -> Date? {
        let f = DateFormatter()
        f.calendar = Calendar(identifier: .gregorian)
        f.dateFormat = "yyyy-MM-dd"
        return f.date(from: d)
    }

    static func shift(_ d: String, _ n: Int) -> String {
        guard let at = date(d), let to = Calendar(identifier: .gregorian).date(byAdding: .day, value: n, to: at) else { return d }
        let f = DateFormatter()
        f.calendar = Calendar(identifier: .gregorian)
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: to)
    }

    static func weekName(_ d: String) -> String {
        guard let at = date(d) else { return "" }
        let i = (Calendar(identifier: .gregorian).component(.weekday, from: at) + 5) % 7
        return ["月", "火", "水", "木", "金", "土", "日"][i]
    }

    /// 表に要る月（月なら一つ・週は跨ぐ二つまで・日は一つ）。
    private var months: [(Int, Int)] {
        let days = mode == "week" ? weekDays : (mode == "day" ? [picked] : [])
        guard !days.isEmpty else { return [(year, month)] }
        var out: [(Int, Int)] = []
        for d in [days.first!, days.last!] {
            let y = Int(d.prefix(4)) ?? year
            let m = Int(d.dropFirst(5).prefix(2)) ?? month
            if !out.contains(where: { $0 == (y, m) }) { out.append((y, m)) }
        }
        return out
    }

    private func count() {
        guard !store.rootPath.isEmpty else { return }
        let want = months
        var all: [Slot] = []
        do {
            // 保存ディレクトリごとに訊いて足す（依頼 511）── どの日のノートも、
            // どこに置いてあっても同じ表に出る。
            for (y, m) in want {
                for p in store.places {
                    guard let url = store.url(of: p) else { continue }
                    let got = try Cian.call("month", [
                        "path": url.path, "year": y, "month": m,
                    ])
                    all += (got["days"] as? [[String: Any]] ?? []).map {
                        Slot(day: $0["day"] as? String ?? "",
                             at: $0["at"] as? String,
                             title: $0["title"] as? String ?? "",
                             path: $0["path"] as? String ?? "",
                             kind: $0["kind"] as? String ?? "note")
                    }
                }
                // この iPhone の予定表（許可されているときだけ）。
                all += Phone.month(y, m)
            }
            slots = all
        } catch {
            trouble = error.localizedDescription
        }
        // よその予定表は、あとから足す ── 一つも読めなくても、自分のぶんは出る。
        Task { @MainActor in
            var more: [Slot] = []
            for (y, m) in want { more += await Away.month(y, m) }
            guard want.map({ "\($0.0)-\($0.1)" }) == months.map({ "\($0.0)-\($0.1)" }) else { return }   // 月を替えたあとの答えは捨てる（週・日の表でも同じ）
            slots = (slots.filter { !$0.isAway } + more).sorted {
                ($0.day, $0.at == nil ? 1 : 0, $0.at ?? "", $0.title)
                    < ($1.day, $1.at == nil ? 1 : 0, $1.at ?? "", $1.title)
            }
        }
    }

    /// 足す。**この iPhone の予定表が使えるならそちらへ**（依頼 460）──
    /// 普通のカレンダーとして期待されるのはこちら。使えないときだけ、
    /// これまでどおりノートを作る（そう書いてある）。
    private func add() {
        let title = newTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty else { return }
        let at = newAt.trimmingCharacters(in: .whitespacesAndNewlines)
        let end = newEnd.trimmingCharacters(in: .whitespacesAndNewlines)
        if Phone.allowed {
            do {
                try Phone.add(title: title, day: picked, at: at.isEmpty ? nil : at, end: end.isEmpty ? nil : end)
                count()
            } catch {
                trouble = error.localizedDescription
            }
            return
        }
        let when = at.isEmpty ? picked : picked + " " + at
        do {
            guard let made = try store.make(titled: title) else {
                trouble = "作れません"
                return
            }
            let (text, stamp) = try store.open(made)
            let out = try Cian.call("setfield", [
                "text": text, "key": "remind", "value": when,
            ])
            _ = try store.save(made, text: out["text"] as? String ?? text, stamp: stamp)
            store.reload()
            count()
        } catch {
            trouble = error.localizedDescription
        }
    }
}

/// カレンダーが訊く三つ（読めません・足す・直す）。
///
/// 画面から出したのは、**一つの `body` に積むと型検査が終わらない**から
/// ── 見た目の都合ではなく、組めるかどうかの都合。
private struct Asking: ViewModifier {
    @Binding var trouble: String?
    @Binding var adding: Bool
    @Binding var editing: Calendaring.Slot?
    @Binding var newTitle: String
    @Binding var newAt: String
    @Binding var newEnd: String
    @Binding var editTitle: String
    let day: String
    let toPhone: Bool
    let add: () -> Void
    let rename: () -> Void
    let drop: () -> Void

    func body(content: Content) -> some View {
        content
            .alert("読めません", isPresented: Binding(
                get: { trouble != nil }, set: { if !$0 { trouble = nil } })
            ) { Button("閉じる") {} } message: { Text(trouble ?? "") }
            // **時刻は打たせない**（依頼 493 の電話の側・本人「手打ちすると絶対ミスする」）
            // ── 窓と同じ小窓一枚: タイトル・終日・開始・終了（十五分刻み）。
            .sheet(isPresented: $adding) {
                EventForm(day: day, toPhone: toPhone, title: $newTitle, at: $newAt, end: $newEnd, add: add)
                    .presentationDetents([.medium])
            }
            // この iPhone の予定は、**押したら直せる**（よその予定表と
            // 違って、書き戻す口がある）。
            .alert("予定を修正する", isPresented: Binding(
                get: { editing != nil }, set: { if !$0 { editing = nil } })
            ) {
                TextField("タイトル", text: $editTitle)
                Button("修正する") { rename() }
                Button("削除する", role: .destructive) { drop() }
                Button("やめる", role: .cancel) {}
            } message: {
                Text("この iPhone の予定表のものです。")
            }
    }
}

/// **予定を登録する小窓**（窓の `askEvent` と同じ四つ）。開始を選ぶと終了は一時間後、
/// 終日に印を入れたら時刻は選べない、空のままなら登録できない。
struct EventForm: View {
    let day: String
    let toPhone: Bool
    @Binding var title: String
    @Binding var at: String
    @Binding var end: String
    let add: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var allDay = false
    @State private var start = "09:00"
    @State private var till = "10:00"
    @State private var said = ""

    static let times: [String] = (0..<24).flatMap { h in ["00", "15", "30", "45"].map { String(format: "%02d:", h) + $0 } }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("タイトル", text: $title)
                } footer: {
                    Text(toPhone ? "\(day) の予定表に登録します" : "\(day) に、ノートが一件できます")
                }
                Section {
                    Toggle("終日", isOn: $allDay)
                    Picker("開始", selection: $start) { ForEach(Self.times, id: \.self) { Text($0).tag($0) } }
                        .disabled(allDay)
                        .onChange(of: start) { _, now in till = Self.plus(now, 60) }
                    Picker("終了", selection: $till) { ForEach(Self.times, id: \.self) { Text($0).tag($0) } }
                        .disabled(allDay)
                } footer: {
                    if !said.isEmpty { Text(said).foregroundStyle(.red) }
                }
            }
            .navigationTitle("予定を登録する")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("やめる") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) { Button("登録する") { go() }.bold() }
            }
            .onAppear {
                allDay = at.isEmpty
                if !at.isEmpty { start = at }
                till = end.isEmpty ? Self.plus(start, 60) : end
            }
        }
    }

    private func go() {
        let t = title.trimmingCharacters(in: .whitespacesAndNewlines)
        if t.isEmpty { said = "タイトルを入れてください"; return }
        if !allDay && till <= start { said = "終了は開始より後にしてください"; return }
        at = allDay ? "" : start
        end = allDay ? "" : till
        add()
        dismiss()
    }

    static func plus(_ t: String, _ min: Int) -> String {
        let p = t.split(separator: ":").compactMap { Int($0) }
        guard p.count == 2 else { return t }
        let n = Swift.min(p[0] * 60 + p[1] + min, 23 * 60 + 45)
        return String(format: "%02d:%02d", n / 60, n % 60)
    }
}
