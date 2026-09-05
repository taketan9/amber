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
    let restore: () -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var zip: URL?
    @State private var trouble: String?
    @AppStorage("cian.look") private var look = Look.auto
    @AppStorage("cian.autosave") private var autosave = true

    var body: some View {
        NavigationStack {
            List {
                Section("ノートの保存場所") {
                    // The path, as the trail of names it is. "amber" alone
                    // answers "what is it called" when the question was
                    // "where is it" — and on a phone, where a folder can be
                    // in three different clouds, that is the whole question.
                    VStack(alignment: .leading, spacing: 6) {
                        Text(store.trail.joined(separator: "  ›  "))
                            .font(.callout)
                            .fixedSize(horizontal: false, vertical: true)
                        Text(store.own
                             ? "この iPhone の中。「ファイル」→ この iPhone 内 → amber で開けます"
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
                } header: {
                    Text("場所")
                } footer: {
                    // **The thing that is actually hard.** 2026-09-05:
                    // 「どこのディレクトリなのかが単純にわからないんだ。
                    // 探せなくて困っている」. The providers are all in the
                    // picker and all several taps down inside it, and none of
                    // them is where a person would guess. So: where to tap,
                    // in order, and what to do when one is not listed. This
                    // is a thing cian cannot do for him — the sidebar is the
                    // Files app's own setting — so the least it can do is say
                    // exactly where it is.
                    Text("選ぶ画面が開いたら、左上の「ブラウズ」から辿ります。\n\n・iCloud Drive → そのまま一覧にあります\n・Google Drive / Dropbox → 「場所」の下に並びます\n\n出てこないときは、その並びの下の「…」→「サイドバーを編集」で、使いたいものをオンにしてください（「ファイル」アプリ側の設定なので、amber からは変えられません）。\n\n灰色で選べないものがあります。フォルダを丸ごと他のアプリに渡せるかどうかは、そのアプリ側の作りによるもので、amber からは変えられません（2026-09 現在、「ドライブ」は灰色、iCloud Drive と Dropbox は選べます）。\n\nMac の cian の amber モードに同じフォルダを指定すれば、両方から同じノートを触れます。")
                }

                // Once found, never looked for again.
                Section {
                    // **This app's own folder is one of the places.** It used
                    // to be a button saying 「この iPhone の中に戻す」, which
                    // says nothing about where that is or why you would.
                    // 2026-09-05: 「元々開いてきたディレクトリの履歴っていう
                    // 表現にしてほしい」 ── so: everywhere the notes have
                    // been, this one included, newest first.
                    if let own = store.ownPlace {
                        Button {
                            store.useOwn()
                        } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(own.name)
                                    Text(own.trail).font(.caption2).foregroundStyle(.secondary)
                                }
                                Spacer()
                                if store.own {
                                    Image(systemName: "checkmark").foregroundStyle(.tint)
                                }
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                    ForEach(store.places) { place in
                            Button {
                                store.revisit(place)
                            } label: {
                                HStack {
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text(place.name)
                                        Text(place.trail)
                                            .font(.caption2)
                                            .foregroundStyle(.secondary)
                                            .lineLimit(1)
                                    }
                                    Spacer()
                                    if place.name == store.rootName {
                                        Image(systemName: "checkmark").foregroundStyle(.tint)
                                    }
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .swipeActions {
                                Button("忘れる", role: .destructive) { store.forget(place) }
                            }
                    }
                } header: {
                    Text("開いてきた場所")
                } footer: {
                    Text("一度見つければ、次からはここから1回で戻れます。左スワイプで履歴から消せます。")
                }

                Section {
                    Button {
                        dismiss()
                        bringIn()
                    } label: {
                        Label("インポート…", systemImage: "square.and.arrow.down")
                    }
                    // The other half of 「バックアップ」. Without it a zip is
                    // a thing you can make and never use, which is not a
                    // backup — it is a file.
                    Button {
                        dismiss()
                        restore()
                    } label: {
                        Label("バックアップから戻す…", systemImage: "clock.arrow.circlepath")
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
