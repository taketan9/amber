import SwiftUI

/// **ぶつかったところを選ぶ**（依頼 501・窓の選び口と同じ三択）。
///
/// 同じ行を両方で直したところと、前書きの鍵（タグなど）のぶつかりを、一つずつ
/// 「こちらを残す／〇〇を残す／両方」で決める。消した側があっても同じ形
/// （本人が決めた・2026-09-11）。上に「ぜんぶこちら／ぜんぶ〇〇」。
struct Choosing: View {
    @ObservedObject var desk: Desk
    let store: NotesStore
    let id: String
    @Environment(\.dismiss) private var dismiss

    private var tab: Desk.Tab? { desk.tabs.first { $0.id == id } }
    private var who: String { (tab?.who.isEmpty == false ? tab?.who : nil) ?? "向こう" }

    var body: some View {
        NavigationStack {
            List {
                if let tab, tab.clashing {
                    Section {
                        Button("すべてこちらの記載を反映する") { desk.chooseAll(id, "ours", store) }
                        Button("すべて\(who)の記載を反映する") { desk.chooseAll(id, "theirs", store) }
                    } footer: {
                        Text("一つずつ選ぶなら下から。どちらを残しても、混ぜる前のこちらの姿は履歴に残っています。")
                    }
                    ForEach(Array(tab.spots.enumerated()), id: \.offset) { n, spot in
                        Section {
                            side("こちら", spot.ours)
                            side(who, spot.theirs)
                            HStack {
                                Button("こちらの記載を反映する") { desk.chooseSpot(id, n, "ours", store) }
                                    .buttonStyle(.borderedProminent)
                                Button("\(who)の記載を反映する") { desk.chooseSpot(id, n, "theirs", store) }
                                    .buttonStyle(.bordered)
                                Button("両方を反映する") { desk.chooseSpot(id, n, "both", store) }
                                    .buttonStyle(.bordered)
                            }
                            .font(.footnote.weight(.semibold))
                        } header: {
                            Text(spot.ours.isEmpty ? "こちらは消し、\(who) は直していました"
                                 : spot.theirs.isEmpty ? "こちらは直し、\(who) は消していました"
                                 : "同じ行を両方で直していました")
                        }
                    }
                    ForEach(Array(tab.fields.enumerated()), id: \.offset) { n, f in
                        Section {
                            LabeledContent("こちら", value: f.ours.isEmpty ? "（なし）" : f.ours)
                            LabeledContent(who, value: f.theirs.isEmpty ? "（なし）" : f.theirs)
                            HStack {
                                Button("こちらの記載を反映する") { desk.chooseField(id, n, "ours", store) }
                                    .buttonStyle(.borderedProminent)
                                Button("\(who)の記載を反映する") { desk.chooseField(id, n, "theirs", store) }
                                    .buttonStyle(.bordered)
                                Button("両方を反映する") { desk.chooseField(id, n, "both", store) }
                                    .buttonStyle(.bordered)
                            }
                            .font(.footnote.weight(.semibold))
                        } header: {
                            Text(f.key == "tags" ? "タグ" : f.key == "title" ? "タイトル" : f.key)
                        }
                    }
                } else {
                    Section {
                        Text("すべて決まりました").foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle("ぶつかったところ")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) { Button("閉じる") { dismiss() } }
            }
        }
    }

    private func side(_ name: String, _ rows: [String]) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(name).font(.caption).foregroundStyle(.secondary)
            if rows.isEmpty {
                Text("（消えている）").font(.footnote).foregroundStyle(.tertiary)
            } else {
                Text(rows.joined().trimmingCharacters(in: .newlines)).font(.body)
            }
        }
    }
}
