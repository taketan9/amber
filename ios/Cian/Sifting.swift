import SwiftUI

/// 絞り込みの帯（電話）── **窓と同じ形**（`gui/renderer.js` の引き出し）。
///
/// タグ・フォルダ・期間の三つが一覧の頭に常に並び、いくつでも重なる。
/// **絞れることが、絞る前から見えている** ── 「フィルタ」という言葉を
/// 覚えなくてよい。押すと、そのすぐ下に選ぶ面が開く。
///
/// 重なり方は種類で違う: **タグは全部・フォルダはどれか**。ノートは一つの
/// フォルダにしか居ないので、フォルダを「全部」にすると二つ選んだ瞬間に
/// 必ず 0 件になる。どちらなのかは開いた面に書く。
struct Sifting: View {
    @ObservedObject var store: NotesStore
    /// いま開いている引き出し。
    @Binding var open: Which?

    enum Which: String, Identifiable { case tag, book, span; var id: String { rawValue } }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    pill(.tag, name("タグ", store.only.map { $0 }))
                    pill(.book, name("フォルダ", store.onlyBooks.map { $0.split(separator: "/").last.map(String.init) ?? $0 }))
                    pill(.span, spanName)
                    if store.narrowing {
                        Button("ぜんぶ外す") {
                            store.only = []
                            store.onlyBooks = []
                            store.span = nil
                            open = nil
                        }
                        .font(.caption)
                        .padding(.horizontal, 9).padding(.vertical, 3)
                        .overlay(Capsule().strokeBorder(.secondary.opacity(0.5),
                                                        style: StrokeStyle(lineWidth: 1, dash: [3, 2])))
                        .foregroundStyle(.secondary)
                    }
                }
            }
        }
    }

    /// 帯の一つに出す字。**選んだものを、開かずに読ませる** ──「タグ 1」では
    /// 何で絞っているか分からない。一つなら名前、二つ以上なら数。
    private func name(_ head: String, _ on: [String]) -> String {
        if on.isEmpty { return head }
        if on.count == 1 { return head + " " + on[0] }
        return head + " \(on.count)"
    }

    private var spanName: String {
        guard let s = store.span else { return "期間" }
        let head = s.created ? "作った日 " : ""
        let from = Calendar.short(s.from), to = Calendar.short(s.to)
        if s.from != nil && s.to != nil { return head + from + "〜" + to }
        return head + (s.from != nil ? from + " から" : to + " まで")
    }

    private func pill(_ which: Which, _ label: String) -> some View {
        let lit = which == .tag ? !store.only.isEmpty
            : which == .book ? !store.onlyBooks.isEmpty : store.span != nil
        return Button {
            open = open == which ? nil : which
        } label: {
            HStack(spacing: 3) {
                Text(label).font(.caption)
                Image(systemName: open == which ? "chevron.up" : "chevron.down")
                    .font(.system(size: 8, weight: .semibold))
            }
            .padding(.horizontal, 9).padding(.vertical, 3)
            .background(
                Capsule().fill(lit ? AnyShapeStyle(.tint)
                    : (open == which ? AnyShapeStyle(.tint.opacity(0.16)) : AnyShapeStyle(.clear)))
            )
            .overlay(Capsule().strokeBorder(.secondary.opacity(lit ? 0 : 0.4)))
            .foregroundStyle(lit ? AnyShapeStyle(.white) : AnyShapeStyle(.primary))
        }
        .buttonStyle(.plain)
    }
}

/// 開いた引き出しの中身。
struct Sifted: View {
    @ObservedObject var store: NotesStore
    let which: Sifting.Which

    var body: some View {
        switch which {
        case .tag:
            picks(head: "押して付け外し（全部付いたものだけ）",
                  rows: store.tagsHere.map { t in
                      (t, t, store.notes.filter { $0.tags.contains(t) }.count)
                  },
                  none: "タグがまだありません（ノートに付けると出ます）",
                  on: { store.only.contains($0) },
                  tap: { if store.only.contains($0) { store.only.remove($0) } else { store.only.insert($0) } })
        case .book:
            picks(head: "押して付け外し（どれかに入っているもの）",
                  rows: store.allBooks.map { b in
                      (b, b, store.notes.filter { $0.book == b || $0.book.hasPrefix(b + "/") }.count)
                  },
                  none: "フォルダがまだありません（上のフォルダの印から作れます）",
                  on: { store.onlyBooks.contains($0) },
                  tap: { if store.onlyBooks.contains($0) { store.onlyBooks.remove($0) }
                         else { store.onlyBooks.insert($0) } })
        case .span:
            Calendaring(store: store)
        }
    }

    @ViewBuilder
    private func picks(head: String, rows: [(String, String, Int)], none: String,
                       on: @escaping (String) -> Bool,
                       tap: @escaping (String) -> Void) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            if rows.isEmpty {
                Text(none).font(.footnote).foregroundStyle(.secondary)
                    .padding(.vertical, 8)
            } else {
                Text(head).font(.caption2).foregroundStyle(.secondary)
                    .padding(.bottom, 4)
                ForEach(rows, id: \.0) { value, label, count in
                    Button { tap(value) } label: {
                        HStack(spacing: 9) {
                            Image(systemName: on(value) ? "checkmark.square.fill" : "square")
                                .foregroundStyle(on(value) ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                            Text(label).font(.subheadline)
                            Spacer()
                            Text("\(count) 件").font(.caption2).foregroundStyle(.secondary)
                                .monospacedDigit()
                        }
                        .contentShape(Rectangle())
                        .padding(.vertical, 5)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }
}

/// こよみ ── **いつからいつまでを、押して決める**（窓と同じ）。
///
/// 「7日以内」のような決め打ちは**去年の秋**を探せない。押した日が範囲の端に
/// なり、片方だけでもよい（「この日から先ぜんぶ」が言えないと範囲は使いもの
/// にならない）。次に押した日がどちらへ入るかは、先に見せる。
struct Calendaring: View {
    @ObservedObject var store: NotesStore
    /// いま見ている月。**開くたびに今月へ戻さない** ── 去年の秋を探して
    /// いる人は、閉じて開くたびに今月へ連れ戻されると探せない。
    @State private var month = Calendar.current.dateInterval(of: .month, for: Date())!.start
    /// 次に押した日を、どちらに入れるか。
    @State private var edge: Edge = .from
    enum Edge { case from, to }

    private let cal = Calendar.current

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 6) {
                ForEach([false, true], id: \.self) { made in
                    let on = (store.span?.created ?? false) == made
                    Button(made ? "作った日" : "直した日") {
                        var s = store.span ?? NotesStore.Span()
                        s.created = made
                        if s.from != nil || s.to != nil { store.span = s } else { store.span = nil }
                        held = made
                    }
                    .font(.caption)
                    .padding(.horizontal, 9).padding(.vertical, 3)
                    .background(Capsule().fill(on ? AnyShapeStyle(.tint) : AnyShapeStyle(.clear)))
                    .overlay(Capsule().strokeBorder(.secondary.opacity(on ? 0 : 0.4)))
                    .foregroundStyle(on ? AnyShapeStyle(.white) : AnyShapeStyle(.primary))
                    .buttonStyle(.plain)
                }
            }
            HStack(spacing: 6) {
                end("いつから", .from, store.span?.from)
                Text("〜").font(.caption).foregroundStyle(.secondary)
                end("いつまで", .to, store.span?.to)
            }
            HStack {
                Button { month = cal.date(byAdding: .month, value: -1, to: month)! } label: {
                    Image(systemName: "chevron.left")
                }
                .accessibilityLabel("前の月")
                Text(title).font(.subheadline.weight(.semibold)).monospacedDigit()
                Button { month = cal.date(byAdding: .month, value: 1, to: month)! } label: {
                    Image(systemName: "chevron.right")
                }
                .accessibilityLabel("次の月")
                Spacer()
                Button("今月") { month = cal.dateInterval(of: .month, for: Date())!.start }
                    .font(.caption).foregroundStyle(.secondary)
            }
            .buttonStyle(.borderless)
            grid
        }
        .padding(.vertical, 4)
    }

    /// 日付を一つも選んでいない間の「どちらの日付で」。
    @State private var held = false

    private var title: String {
        let f = DateFormatter()
        f.calendar = cal
        f.locale = Locale(identifier: "ja_JP")
        f.dateFormat = "yyyy年 M月"
        return f.string(from: month)
    }

    private func end(_ name: String, _ side: Edge, _ day: String?) -> some View {
        HStack(spacing: 2) {
            Button {
                edge = side
            } label: {
                Text(day.map(Calendar.short) ?? name)
                    .font(.caption).monospacedDigit()
                    .padding(.horizontal, 9).padding(.vertical, 4)
                    .overlay(RoundedRectangle(cornerRadius: 7)
                        .strokeBorder(edge == side ? AnyShapeStyle(.tint)
                                      : AnyShapeStyle(.secondary.opacity(0.4))))
            }
            .buttonStyle(.plain)
            if day != nil {
                Button { put(side, nil) } label: { Image(systemName: "xmark") }
                    .font(.caption2).foregroundStyle(.secondary)
                    .buttonStyle(.borderless)
                    .accessibilityLabel(name + "を外す")
            }
        }
    }

    private var grid: some View {
        let first = cal.dateInterval(of: .month, for: month)!.start
        let lead = cal.component(.weekday, from: first) - 1
        let days = cal.range(of: .day, in: .month, for: month)!.count
        // 前の月と次の月のはみ出しも押せる ── 月末をまたぐ範囲はよくある。
        let before = cal.date(byAdding: .month, value: -1, to: first)!
        let beforeDays = cal.range(of: .day, in: .month, for: before)!.count
        let after = cal.date(byAdding: .month, value: 1, to: first)!
        var cells: [(Date, Int, Bool)] = []
        for i in stride(from: lead, to: 0, by: -1) {
            cells.append((before, beforeDays - i + 1, true))
        }
        for d in 1...days { cells.append((first, d, false)) }
        var n = 1
        while cells.count % 7 != 0 { cells.append((after, n, true)); n += 1 }

        return VStack(spacing: 2) {
            HStack(spacing: 2) {
                ForEach(["日", "月", "火", "水", "木", "金", "土"], id: \.self) { w in
                    Text(w).font(.caption2).foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity)
                }
            }
            ForEach(0..<(cells.count / 7), id: \.self) { row in
                HStack(spacing: 2) {
                    ForEach(0..<7, id: \.self) { col in
                        let (base, d, out) = cells[row * 7 + col]
                        day(base, d, out)
                    }
                }
            }
        }
    }

    private func day(_ base: Date, _ d: Int, _ out: Bool) -> some View {
        let key = Calendar.stamp(cal.date(bySetting: .day, value: d, of: base) ?? base, d: d, cal: cal)
        let from = store.span?.from, to = store.span?.to
        let end = key == from || key == to
        let inside = from != nil && to != nil && key > from! && key < to!
        return Button { put(edge, key) } label: {
            Text("\(d)")
                .font(.subheadline).monospacedDigit()
                .frame(maxWidth: .infinity, minHeight: 30)
                .background(end ? AnyShapeStyle(.tint)
                            : (inside ? AnyShapeStyle(.tint.opacity(0.18)) : AnyShapeStyle(.clear)))
                .clipShape(RoundedRectangle(cornerRadius: inside ? 0 : 7))
                .foregroundStyle(end ? AnyShapeStyle(.white)
                                 : (out ? AnyShapeStyle(.secondary) : AnyShapeStyle(.primary)))
        }
        .buttonStyle(.plain)
    }

    /// 範囲の端を決める。**前後が入れ替わったら、黙って入れ替える** ──
    /// たいていは始まりを言い直しているので、0 件を返して考えさせる場面
    /// ではない。
    private func put(_ side: Edge, _ day: String?) {
        var s = store.span ?? NotesStore.Span(created: held, from: nil, to: nil)
        if side == .from { s.from = day } else { s.to = day }
        if let f = s.from, let t = s.to, f > t { s.from = t; s.to = f }
        store.span = (s.from == nil && s.to == nil) ? nil : s
        // 次はもう片方 ── 二度押しで範囲が決まる。
        if day != nil { edge = side == .from ? .to : .from }
    }
}

extension Calendar {
    /// `YYYY-MM-DD` を作る（その月の `d` 日）。
    @MainActor
    static func stamp(_ base: Date, d: Int, cal: Calendar) -> String {
        var parts = cal.dateComponents([.year, .month], from: base)
        parts.day = d
        guard let at = cal.date(from: parts) else { return "" }
        return NotesStore.day(UInt64(max(0, at.timeIntervalSince1970)))
    }

    /// 帯に出す短い呼び名。**今年なら年を言わない** ── 帯は狭いし、たいてい
    /// は今年。年が違うときだけ年を出す（`12/31` が去年か今年かは見て分から
    /// ない）。
    static func short(_ day: String?) -> String {
        guard let day, day.count == 10 else { return "" }
        let year = String(day.prefix(4))
        let md = day.dropFirst(5).replacingOccurrences(of: "-", with: "/")
        let now = Calendar.current.component(.year, from: Date())
        return year == String(now) ? md : year + "/" + md
    }
}
