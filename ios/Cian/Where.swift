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
    @Environment(\.dismiss) private var dismiss
    @State private var picking = false
    @State private var importing = false

    var body: some View {
        NavigationStack {
            List {
                Section("いま") {
                    LabeledContent("場所", value: store.rootName)
                    LabeledContent("ノート", value: "\(store.notes.count) 本")
                    if store.own {
                        Text("この iPhone の中（「ファイル」からは **cian** として見えます）")
                            .font(.caption).foregroundStyle(.secondary)
                    } else {
                        Text(store.rootPath).font(.caption).foregroundStyle(.secondary)
                            .lineLimit(3)
                    }
                }

                Section {
                    Button {
                        picking = true
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
                        importing = true
                    } label: {
                        Label("マークダウンを取り込む…", systemImage: "square.and.arrow.down")
                    }
                    ShareLink(item: store.rootURL ?? URL(fileURLWithPath: "/")) {
                        Label("このフォルダを書き出す…", systemImage: "square.and.arrow.up")
                    }
                    .disabled(store.rootURL == nil)
                } header: {
                    Text("出し入れ")
                } footer: {
                    Text("取り込んだ .md はこのフォルダにコピーされます。元のファイルはそのままです。")
                }
            }
            .navigationTitle("ノートの置き場所")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { Button("閉じる") { dismiss() } }
            }
            .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { r in
                if case .success(let url) = r { store.choose(url) }
            }
            // `allowsMultipleSelection`: bringing notes in is nearly always
            // more than one — a folder somebody exported from somewhere else.
            .fileImporter(
                isPresented: $importing,
                allowedContentTypes: [UTType(filenameExtension: "md") ?? .plainText, .plainText],
                allowsMultipleSelection: true
            ) { r in
                if case .success(let urls) = r { store.bring(urls) }
            }
        }
    }
}
