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
        let path: String
        let kind: String
        var id: String { day + (at ?? "") + path + kind }
        var isPlan: Bool { kind != "note" }
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

    static var today: String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: Date())
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                grid
                Divider()
                day
            }
            // **`Text` に数をそのまま渡さない。** SwiftUI は土地の決まりで
            // 桁を区切るので、年が「2,026年」になる（実際になった）。
            .navigationTitle(Text(verbatim: "\(year)年 \(month)月"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
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
                Text("\(spoken(picked)) に、ノートが一本できます。")
            }
        }
        .task { count() }
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
                        .foregroundStyle(s.isPlan ? Color.accentColor : .secondary)
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
                    // **道は出さない** ── 読めない長さになるうえ、知りたいのは
                    // 中身のほう。一行目を添える。
                    if let n = store.notes.first(where: { $0.path == s.path }), !n.excerpt.isEmpty {
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
    }

    private func add() {
        let title = newTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty else { return }
        let at = newAt.trimmingCharacters(in: .whitespacesAndNewlines)
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
