import SwiftUI

/// **外から渡された一本を、単発で開く**（依頼 659）。
///
/// デスクトップ版の `openGuest` と同じ考え ── 索引にも机にも載せない、amber の
/// フォルダの外にある一本。帯に「元のファイルを直しています」と出して、
/// 何もしなければ元がそのまま更新される（本人「圧倒的に　もとのファイルを
/// 更新する　が優勢になる UI」）。
///
/// **許可は開いて、閉じるまで持ち続ける。**
/// 「ファイル」から渡された場所は許可の要る場所で、
/// `startAccessingSecurityScopedResource` を通さないと**画面には出るのに保存
/// だけ黙って失敗する** ── 以前踏んだ「画面に出ているのに、保存されない」と
/// 同じ形。
@MainActor
final class Guest: ObservableObject {
    static let shared = Guest()

    @Published private(set) var note: Note?
    @Published var trouble: String?
    /// 取り込んだ先（画面が開き直すために見る）。
    @Published var adopted: String?

    /// 許可を開いた道。**閉じるまで手放さない。**
    private var scoped: URL?

    var on: Bool { note != nil }

    /// 開けるもの。**デスクトップ版と同じ三つ。**
    static let kinds = ["md", "markdown", "txt"]

    /// 素のまま（コード）で開くか ── `.txt` はそう（本人が決めた）。
    ///
    /// 表示で開くと `# ` が見出しに、`* ` が箇条に組み直され、保存で amber の
    /// 書き方に寄る。`.md` の往復は `round-test.js` が総当たりで守っているが、
    /// `.txt` を丸める理由は無い。
    var bare: Bool { (note?.path ?? "").lowercased().hasSuffix(".txt") }

    func take(_ url: URL) {
        shut()
        guard Self.kinds.contains(url.pathExtension.lowercased()) else {
            trouble = "開けるのは .md / .markdown / .txt です"
            return
        }
        // 許可が開けなくても、読めるなら使う（アプリ自身のフォルダ）。
        let ok = url.startAccessingSecurityScopedResource()
        if ok { scoped = url }
        guard ok || FileManager.default.isReadableFile(atPath: url.path) else {
            trouble = "そのファイルを開く許可がありません"
            shut()
            return
        }
        guard let got = try? Cian.call("note", ["path": url.path]), let one = Note(got) else {
            trouble = "開けません: " + url.lastPathComponent
            shut()
            return
        }
        note = one
        // **案内の途中に来たら、案内はやめる**（依頼 659）── 開きたいファイルが
        // あって来た人に、幕をかぶせたままにしない。
        Tour.shared.shut()
    }

    func shut() {
        scoped?.stopAccessingSecurityScopedResource()
        scoped = nil
        note = nil
    }

    /// **ambər に取り込む（移す）。** 判断は core（`adopt`）── 写す → 画像も
    /// 運ぶ → 中身が同じか確かめる → **そこで初めて元を消す**。
    ///
    /// `.txt` は `.md` に改名される（一覧に載るのはそれだけ）。
    func adopt(into dir: String) {
        guard let note else { return }
        do {
            let got = try Cian.call("adopt", ["path": note.path, "to": dir])
            let at = got["path"] as? String ?? ""
            shut()
            adopted = at
        } catch {
            trouble = "取り込めませんでした: " + error.localizedDescription
        }
    }
}

/// 外から来た一本と、一覧の画面をつなぐ見張り。
///
/// **一つにまとめてある** ── `ContentView` の本体に `onChange` を足すたび、
/// Swift の型検査が音を上げる（依頼 656 で一度そうなった）。
struct GuestHooks: ViewModifier {
    @ObservedObject var guest = Guest.shared
    /// 外から来た一本を、机に載せる。
    let open: (Note) -> Void
    /// 取り込んだ先を開き直す。
    let opened: (String) -> Void

    func body(content: Content) -> some View {
        content
            .onChange(of: guest.note) { _, note in
                if let note { open(note) }
            }
            .onChange(of: guest.adopted) { _, at in
                guard let at else { return }
                guest.adopted = nil
                opened(at)
            }
            .alert("開けません", isPresented: Binding(
                get: { guest.trouble != nil }, set: { if !$0 { guest.trouble = nil } }
            )) {
                Button("閉じる", role: .cancel) {}
            } message: {
                Text(guest.trouble ?? "")
            }
    }
}
