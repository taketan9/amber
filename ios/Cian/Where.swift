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

    var body: some View {
        NavigationStack {
            List {
                Section("いま") {
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
                    ShareLink(item: store.rootURL ?? URL(fileURLWithPath: "/")) {
                        Label("すべてバックアップ…", systemImage: "square.and.arrow.up")
                    }
                    .disabled(store.rootURL == nil)
                } header: {
                    Text("バックアップとインポート")
                } footer: {
                    Text("インポートした .md はこのフォルダにコピーされます。元のファイルはそのまま。同じ名前があるときは番号を付けて、いまあるノートは上書きしません。")
                }
            }
            .navigationTitle("ノートの置き場所")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { Button("閉じる") { dismiss() } }
            }
        }
    }
}
