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
    /// 直している、この iPhone の予定。
    @State private var editing: Slot?
    @State private var editTitle = ""

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
            grid
            Divider()
            day
        }
        // **`Text` に数をそのまま渡さない。** SwiftUI は土地の決まりで
        // 桁を区切るので、年が「2,026年」になる（実際になった）。
        .navigationTitle(Text(verbatim: "\(year)年 \(month)月"))
        .navigationBarTitleDisplayMode(.inline)
        .toolbar { bar }
        .modifier(Asking(
            trouble: $trouble, adding: $adding, editing: $editing,
            newTitle: $newTitle, newAt: $newAt, editTitle: $editTitle,
            day: spoken(picked), toPhone: Phone.allowed,
            add: add, rename: rename, drop: drop))
    }

    @ToolbarContentBuilder private var bar: some ToolbarContent {
        ToolbarItem(placement: .cancellationAction) {
            Button("閉じる") { dismiss() }
        }
        ToolbarItemGroup(placement: .topBarTrailing) {
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
                ForEach(Array(["月", "火", "水", "木", "金", "土", "日"].enumerated()), id: \.offset) { i, w in
                    Text(w).font(.caption2)
                        .foregroundStyle(i == 5 ? Color.blue : (i == 6 ? Color.red : Color.secondary))
                        .frame(maxWidth: .infinity)
                }
            }
            ForEach(weeks, id: \.self) { week in
                HStack(spacing: 3) {
                    ForEach(week, id: \.self) { d in cell(d) }
                }
            }
        }
        .padding(.horizontal, 8)
        .padding(.top, 6)
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
                    Text(s.title).font(.system(size: 8)).lineLimit(1)
                        .foregroundStyle(s.isAway ? Color.blue
                            : (s.isPhone ? Color.green
                               : (s.isPlan ? Color.accentColor : Color.secondary)))
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
                let plans = slots.filter { $0.day == picked && $0.isPlan }
                if plans.isEmpty {
                    Text("予定はありません").foregroundStyle(.secondary).font(.footnote)
                }
                ForEach(plans) { s in row(s, time: true) }
            }
            // **予定に出ているノートを、下でもう一度出さない** ── 同じ一本が
            // 二度並ぶと、二つあるように見える。
            let said = Set(slots.filter { $0.day == picked && $0.isPlan }.map(\.path))
            let notes = slots.filter { $0.day == picked && !$0.isPlan && !said.contains($0.path) }
            if !notes.isEmpty {
                Section("この日に書いたノート") {
                    ForEach(notes) { s in row(s, time: false) }
                }
            }
            Section {
                Button {
                    newTitle = ""
                    newAt = "09:00"
                    adding = true
                } label: {
                    Label("この日に予定を足す", systemImage: "plus")
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
                    Text(s.title)
                        .foregroundStyle(s.isAway ? Color.blue
                            : (s.isPhone ? Color.green : Color.primary))
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
        let mine = slots.filter { $0.day == d }
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
        var m = month + n
        var y = year
        if m < 1 { m = 12; y -= 1 }
        if m > 12 { m = 1; y += 1 }
        year = y
        month = m
        count()
    }

    private func count() {
        guard !store.rootPath.isEmpty else { return }
        do {
            let got = try Cian.call("month", [
                "path": store.rootPath, "year": year, "month": month,
            ])
            slots = (got["days"] as? [[String: Any]] ?? []).map {
                Slot(day: $0["day"] as? String ?? "",
                     at: $0["at"] as? String,
                     title: $0["title"] as? String ?? "",
                     path: $0["path"] as? String ?? "",
                     kind: $0["kind"] as? String ?? "note")
            }
        } catch {
            trouble = error.localizedDescription
        }
        // よその予定表は、あとから足す ── 一つも読めなくても、自分のぶんは出る。
        // この iPhone の予定表（許可されているときだけ）。
        slots += Phone.month(year, month)
        let (y, m) = (year, month)
        Task { @MainActor in
            let more = await Away.month(y, m)
            guard y == year, m == month else { return }   // 月を替えたあとの答えは捨てる
            slots = (slots + more).sorted {
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
        if Phone.allowed {
            do {
                try Phone.add(title: title, day: picked, at: at.isEmpty ? nil : at)
                count()
            } catch {
                trouble = error.localizedDescription
            }
            return
        }
        let when = at.isEmpty ? picked : picked + " " + at
        do {
            guard let made = try store.make(titled: title) else {
                trouble = "作れませんでした"
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
            .alert("予定を足す", isPresented: $adding) {
                TextField("何をする", text: $newTitle)
                TextField("何時から（空なら終日）", text: $newAt)
                    .keyboardType(.numbersAndPunctuation)
                Button("足す") { add() }
                Button("やめる", role: .cancel) {}
            } message: {
                Text(toPhone
                     ? "\(day) の予定表に足します。"
                     : "\(day) に、ノートが一本できます。")
            }
            // この iPhone の予定は、**押したら直せる**（よその予定表と
            // 違って、書き戻す口がある）。
            .alert("予定を直す", isPresented: Binding(
                get: { editing != nil }, set: { if !$0 { editing = nil } })
            ) {
                TextField("題", text: $editTitle)
                Button("直す") { rename() }
                Button("消す", role: .destructive) { drop() }
                Button("やめる", role: .cancel) {}
            } message: {
                Text("この iPhone の予定表のものです。")
            }
    }
}
