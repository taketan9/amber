import SwiftUI

/// Setting a note's reminder, and its routine.
struct Ringing: View {
    let note: Note
    @Binding var text: String
    let store: NotesStore
    @Environment(\.dismiss) private var dismiss

    @State private var r = Reminder()
    @State private var onceOn = false
    @State private var when = Date()
    @State private var everyOn = false
    @State private var kind = "weekly"
    @State private var day = 2
    @State private var monthDay = 1
    @State private var clock = Date()

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Toggle("一度だけ知らせる", isOn: $onceOn)
                    if onceOn {
                        DatePicker("いつ", selection: $when)
                    }
                }

                Section {
                    Toggle("繰り返す", isOn: $everyOn)
                    if everyOn {
                        Picker("間隔", selection: $kind) {
                            Text("毎日").tag("daily")
                            Text("毎週").tag("weekly")
                            Text("毎月").tag("monthly")
                        }
                        .pickerStyle(.segmented)
                        if kind == "weekly" {
                            Picker("曜日", selection: $day) {
                                ForEach(0..<7, id: \.self) { Text(Bell.dayLabels[$0]).tag($0) }
                            }
                            .pickerStyle(.segmented)
                        }
                        if kind == "monthly" {
                            Picker("日", selection: $monthDay) {
                                ForEach(1...31, id: \.self) { Text("\($0) 日").tag($0) }
                            }
                        }
                        DatePicker("時刻", selection: $clock, displayedComponents: .hourAndMinute)
                    }
                } footer: {
                    // The one sentence that stops this being a lie. iOS will
                    // not wake an app to write a file, and a routine that
                    // claimed to would quietly only happen when you looked.
                    Text("繰り返しは、時刻に通知が届きます。その日のノートは、次に cian を開いたときに作られます（iPhone はアプリを勝手に動かさないため）。")
                }
            }
            .navigationTitle("通知")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("やめる") { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) { Button("決定") { apply() }.bold() }
            }
            .task { load() }
        }
    }

    private func load() {
        r = (try? store.reminder(of: text)) ?? Reminder()
        onceOn = !r.once.isEmpty
        if let at = date(r.once) { when = at }
        everyOn = r.repeats
        if r.repeats {
            kind = r.kind
            if r.kind == "weekly" { day = r.n }
            if r.kind == "monthly" { monthDay = r.n }
            var c = DateComponents()
            c.hour = r.hour; c.minute = r.minute
            clock = Calendar.current.date(from: c) ?? clock
        }
    }

    private func apply() {
        var out = text
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm"
        out = (try? store.field(out, "remind", onceOn ? f.string(from: when) : nil)) ?? out

        var next = Reminder()
        next.once = onceOn ? f.string(from: when) : ""
        if everyOn {
            let c = Calendar.current.dateComponents([.hour, .minute], from: clock)
            next.kind = kind
            next.n = kind == "weekly" ? day : (kind == "monthly" ? monthDay : 0)
            next.hour = c.hour ?? 9
            next.minute = c.minute ?? 0
        }
        out = (try? store.field(out, "repeat", next.repeatLine)) ?? out
        text = out
        Bell.set(note, next)
        dismiss()
    }

    private func date(_ s: String) -> Date? {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd HH:mm"
        return f.date(from: s)
    }
}
