import SwiftUI

/// Naming a new folder.
///
/// A sheet and not an alert-with-a-field. Two reasons, and the second is the
/// honest one: a sheet has room to say *where* the folder is going, which an
/// alert's one line does not; and an alert containing a text field could not
/// be driven by the checks that watch this app work, so shipping one would
/// mean shipping a screen nobody had seen operate.
struct Booking: View {
    /// Where it will go, for the line that says so.
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
