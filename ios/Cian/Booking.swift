import SwiftUI

/// 新しいフォルダに名前を付ける。
///
/// アラートに入力欄を付けるのではなくシートにした。理由は 2 つで、正直なのは
/// 2 つ目 ── シートにはフォルダが*どこに*できるかを書く余白があり、アラートの
/// 1 行には無い。そして入力欄の入ったアラートは、このアプリの動作を見張る
/// テストから操作できないので、それを出すのは「誰も動くところを見ていない
/// 画面」を出すことになる。
struct Booking: View {
    /// どこにできるか。それを書く行のため。
    let inside: String
    let make: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @FocusState private var typing: Bool

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("名前", text: $name)
                        .focused($typing)
                        .autocorrectionDisabled()
                        .submitLabel(.done)
                        .onSubmit { done() }
                } footer: {
                    Text("\(inside) の中に作ります")
                }
            }
            .navigationTitle("新しいフォルダ")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("やめる") { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("作成") { done() }
                        .bold()
                        .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                }
            }
            .task { typing = true }
        }
    }

    private func done() {
        let n = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !n.isEmpty else { return }
        make(n)
        dismiss()
    }
}
