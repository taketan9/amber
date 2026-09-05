import SwiftUI
import UniformTypeIdentifiers

/// Where the notes are kept, and how they get in and out.
///
/// The folder button used to open the system file picker straight away, which
/// answers a question nobody asked: it looks like "find a file" when what it
/// is for is "this is where my notes live, and I might move them". This says
/// where they are first, and offers the picker as one of the things you can
/// do about it.
struct Where: View {
    @ObservedObject var store: NotesStore
    /// Asked for after this sheet has closed.
    ///
    /// **A file picker cannot be opened from inside a sheet** — it is a
    /// presentation over a presentation, and it silently does nothing, which
    /// is exactly how 「保存場所を選ぶ」 behaved: pressed, and no answer at
    /// all. So the sheet closes first and the screen underneath opens it.
    let choose: () -> Void
    let bringIn: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var zip: URL?
    @State private var trouble: String?
    @AppStorage("cian.look") private var look = Look.auto
    @AppStorage("cian.autosave") private var autosave = true

    var body: some View {
        NavigationStack {
            List {
                Section("ノートの置き場所") {
                    // The path, as the trail of names it is. "cian" alone
                    // answers "what is it called" when the question was
                    // "where is it" — and on a phone, where a folder can be
                    // in three different clouds, that is the whole question.
                    VStack(alignment: .leading, spacing: 6) {
                        Text(store.trail.joined(separator: "  ›  "))
                            .font(.callout)
                            .fixedSize(horizontal: false, vertical: true)
                        Text(store.own
                             ? "この iPhone の中。「ファイル」→ この iPhone 内 → cian で開けます"
                             : store.rootPath)
                            .font(.caption2).foregroundStyle(.secondary)
                            .lineLimit(4)
                    }
                    LabeledContent("ノート", value: "\(store.notes.count) 本")
                }

                Section {
                    Button {
                        dismiss()
                        choose()
                    } label: {
                        Label("保存場所を選ぶ…", systemImage: "folder")
                    }
                    if !store.own {
                        Button {
                            store.useOwn()
                        } label: {
                            Label("この iPhone の中に戻す", systemImage: "iphone")
                        }
                    }
                } header: {
                    Text("場所")
                } footer: {
                    // The one sentence that makes the whole thing make sense,
                    // and the reason there is no Google Drive code in here.
                    Text("iCloud Drive・Google Drive・Dropbox のフォルダを選べます。iPhone に そのアプリが入っていないものは灰色で選べません。Mac の cian に同じフォルダを指定すれば、両方から同じノートを触れます。")
                }

                Section {
                    Button {
                        dismiss()
                        bringIn()
                    } label: {
                        Label("インポート…", systemImage: "square.and.arrow.down")
                    }
                    // The scope is a choice because backing up is
                    // something people do *before* something — before a
                    // reinstall, before handing a folder to somebody, before
                    // tidying. Each of those wants a different amount.
                    Menu {
                        Button("すべて") { make("all", "") }
                        if !store.allBooks.isEmpty {
                            Menu("フォルダ") {
                                ForEach(store.allBooks, id: \.self) { b in
                                    Button(b) { make("book", b) }
                                }
                            }
                        }
                        if !store.allTags.isEmpty {
                            Menu("タグ") {
                                ForEach(store.allTags, id: \.self) { t in
                                    Button("#\(t)") { make("tag", t) }
                                }
                            }
                        }
                    } label: {
                        Label("バックアップ…", systemImage: "square.and.arrow.up")
                    }
                } header: {
                    Text("バックアップとインポート")
                } footer: {
                    Text("インポートした .md はこのフォルダにコピーされます。元のファイルはそのまま。同じ名前があるときは番号を付けて、いまあるノートは上書きしません。")
                }

                Section {
                    Toggle("自動保存", isOn: $autosave)
                } footer: {
                    Text("切ると、書く画面に「保存」が出ます。切っていても、画面を離れるときに一度だけ訊きます。")
                }

                Section {
                    Picker("見た目", selection: $look) {
                        ForEach(Look.allCases) { Text($0.label).tag($0) }
                    }
                    .pickerStyle(.inline)
                    .labelsHidden()
                } header: {
                    Text("見た目")
                } footer: {
                    // Three and not two: a phone that goes dark at sunset is
                    // the common case, and a switch with no way back to it
                    // is a switch that gets set once and regretted.
                    Text("「iPhone に合わせる」は、夜になると暗くなる設定にしているときに一緒に暗くなります。")
                }
            }
            // The zip exists before the share sheet opens, so what is being
            // handed over is a file that is already there — not a promise.
            .sheet(item: $zip) { at in
                ActivityView(item: at)
            }
            .alert(
                "作れませんでした",
                isPresented: Binding(get: { trouble != nil }, set: { if !$0 { trouble = nil } })
            ) {
                Button("閉じる") {}
            } message: {
                Text(trouble ?? "")
            }
            .navigationTitle("設定")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { Button("閉じる") { dismiss() } }
            }
        }
    }

    private func make(_ scope: String, _ what: String) {
        do { zip = try store.backup(scope: scope, what: what) }
        catch { trouble = error.localizedDescription }
    }
}

extension URL: @retroactive Identifiable {
    public var id: String { absoluteString }
}

/// The system's own share sheet, for a file that already exists.
///
/// `ShareLink` wants its item when the view is built; a backup is made when
/// the button is pressed, which is a different moment.
struct ActivityView: UIViewControllerRepresentable {
    let item: URL
    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: [item], applicationActivities: nil)
    }
    func updateUIViewController(_ vc: UIActivityViewController, context: Context) {}
}
