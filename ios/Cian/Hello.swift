import SwiftUI

/// **ようこそ画面**（依頼 654）── 初めて開いた人に、まず三つだけ見せる。
///
/// デスクトップ版の `#hello` と同じ三行・同じ押しかた。**文は本人が書いた**
/// （2026-09-27）。Markdown は三行目に一度だけ出す ── 前の案は二行目で
/// 「Markdown で書きます」と言い、三行目で「Markdown を知らなくても」と
/// 打ち消していて、身構えさせてから慰める形だった。
///
/// **押すものは二つだけ。** 大きな「Google で始める」と、小さな文字リンク。
/// 三つ目を足すと「どれでもよい」に見えて、同期まで進めさせる狙いが消える。
///
/// **逃げ道は必ず置く。** Apple の 5.1.1(v) は「主な機能に要らないサインインを
/// 強いてはいけない」と書いている ── ここを塞ぐと審査で止まる。
struct Hello: View {
    /// 憶える鍵。**新しい鍵は `amber.` で始める**（`cian.` は消えると困るので
    /// 残しているだけ）。
    static let greetedKey = "amber.greeted"

    /// 見たことがあるか。
    static var seen: Bool { UserDefaults.standard.bool(forKey: greetedKey) }

    @ObservedObject var sync: Syncing
    /// 片したあとに呼ぶ。サインインまで行けたかどうかを渡す。
    var done: (Bool) -> Void

    @State private var busy = false
    @State private var said: String?

    /// デスクトップ版の `HELLO_SELL` と同じ三行。**二つの amber で同じ字**。
    private static let sell: [(String, String)] = [
        ("家族やグループと、同じノートを。", "予定表も一緒に使えます"),
        ("読みやすく、きれいに。", "見出しも表も、そのまま整います"),
        ("記法を知らなくても、ボタンで書けます", "（中身は Markdown です）"),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 22) {
            Spacer(minLength: 0)
            (Text("amb")
                + Text("ə").foregroundColor(Color("BrandSchwa"))
                + Text("r")
                + Text(" へようこそ"))
                .font(.largeTitle.weight(.bold))
                .accessibilityLabel("ambər へようこそ")
            VStack(alignment: .leading, spacing: 15) {
                ForEach(Self.sell, id: \.0) { head, rest in
                    (Text(head).font(.callout.weight(.semibold))
                        + Text(" " + rest).font(.callout).foregroundColor(.secondary))
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            if let said {
                Text(said).font(.footnote).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
            VStack(spacing: 14) {
                Button {
                    start()
                } label: {
                    Text(busy ? "ブラウザで許可してください…" : "Google で始める")
                        .font(.headline)
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                }
                .buttonStyle(.borderedProminent)
                .disabled(busy)
                // **逃げ道は文字リンク**（本人が決めた）── ボタンにすると
                // 二択に見えて、どちらでもよいことになる。
                Button("いまはしない") { shut(false) }
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .disabled(busy)
            }
        }
        .padding(.horizontal, 26)
        .padding(.bottom, 24)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        .background(Color(.systemBackground))
        .interactiveDismissDisabled()
    }

    private func start() {
        busy = true
        Task {
            do {
                _ = try await sync.signIn()
                shut(true)
            } catch {
                // **消えない形で言う**（デスクトップ版と同じ）── 帯の一言は
                // 数秒で消え、見逃すと「押したのに何も起きない」にしか見えない。
                // **もう一度は勧めない**（本人が決めた）── 二度目の誘いは嫌われる。
                said = "サインインできませんでした: " + error.localizedDescription
                busy = false
            }
        }
    }

    private func shut(_ signedIn: Bool) {
        // **憶えるのは片したとき。** 開いた時点で憶えると、途中で落ちた回に
        // 二度と出ない（初めての人が、初めての画面を一度も見られない）。
        UserDefaults.standard.set(true, forKey: Self.greetedKey)
        done(signedIn)
    }
}
