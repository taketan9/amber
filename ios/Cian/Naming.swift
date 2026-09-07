import SwiftUI

/// 名乗り（電話）。
///
/// **設定画面に置かない。** 一度しか使わないものを、毎日見る画面に置く
/// 値打ちは無い ── 要る瞬間（初めて共有する瞬間）に一度だけ訊いて憶える。
///
/// 名前は**履歴に付いて回る**（ノートには書かない）ので、相手の amber が
/// 「Taketan が足しました」と言える。**書かなくてもいい** ── そのときは
/// 「だれか」になるだけで、共有そのものは動く。
struct Naming: View {
    let folder: String
    @Binding var me: String
    let done: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @FocusState private var typing: Bool

    struct Which: Identifiable {
        let at: String
        var id: String { at }
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("名前", text: $name)
                        .focused($typing)
                        .autocorrectionDisabled()
                        .submitLabel(.done)
                        .onSubmit { go() }
                } header: {
                    Text("あなたの名前")
                } footer: {
                    Text("共有したノートに「誰が直したか」を出すために使います。ノートには書きません（.md はただのマークダウンのままです）。空のままでも共有はできます。")
                }
                Section {
                    Text("「\(folder)」を、家族と分ける棚にします。")
                        .font(.footnote).foregroundStyle(.secondary)
                    Text("このあと、フォルダそのものをクラウド側で家族に分けてください ── ambƏr が印を置いただけでは、まだ誰にも届きません。")
                        .font(.footnote).foregroundStyle(.secondary)
                }
            }
            .navigationTitle("家族と共有する")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("やめる") { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) { Button("共有する") { go() }.bold() }
            }
            .task {
                // **この機械が既に知っていることを、もう一度打たせない。**
                if name.isEmpty { name = me.isEmpty ? UIDevice.current.name : me }
                typing = true
            }
        }
    }

    private func go() {
        let n = name.trimmingCharacters(in: .whitespacesAndNewlines)
        me = n
        done(n)
        dismiss()
    }
}
