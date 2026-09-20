import SwiftUI

/// ノートの構成要素 1 つを、描画できる形にしたもの。
///
/// 何が見出し*である*かは `amber_core::note` が決め、どう見えるかはここで決める。
/// 逆の分け方をすると、テストの届かない iPhone 側に Markdown パーサーを
/// 置くことになる。
struct Block: Identifiable {
    let id = UUID()
    let kind: String
    let text: String
    let level: Int
    let n: Int
    let lang: String
    let alt: String
    let link: String
    /// チェックボックスと、それが書かれているノートの行番号。
    let done: Bool
    let line: Int
    /// 行を色付きと色無しの部分に切ったもの ── 計算するのは
    /// `amber_core::note::spans` なので、デスクトップ版も同じ切り方で描く。
    let runs: [(String, String?)]
    /// 表 ── ヘッダー、列ごとの寄せ方、そして本体の行。
    let head: [String]
    let align: [String]
    let rows: [[String]]
    /// `> [!NOTE]` ── GitHub の 5 種類のどれかと、その下の段落。
    let alert: String
    let body: [String]

    init(_ o: [String: Any]) {
        kind = o["kind"] as? String ?? "paragraph"
        text = o["text"] as? String ?? ""
        level = o["level"] as? Int ?? 0
        n = o["n"] as? Int ?? 0
        lang = o["lang"] as? String ?? ""
        alt = o["alt"] as? String ?? ""
        link = o["link"] as? String ?? ""
        done = o["done"] as? Bool ?? false
        line = o["line"] as? Int ?? -1
        runs = (o["runs"] as? [[String: Any]] ?? []).map {
            ($0["text"] as? String ?? "", $0["color"] as? String)
        }
        // セルの中の書式は `text` のまま持ってきて、描くときに読む ──
        // ここで剥がすと、Markdown の読み手がiPhone にもう一つ生える。
        let cell = { (c: [String: Any]) in c["text"] as? String ?? "" }
        head = (o["head"] as? [[String: Any]] ?? []).map(cell)
        align = o["align"] as? [String] ?? []
        rows = (o["rows"] as? [[[String: Any]]] ?? []).map { $0.map(cell) }
        alert = o["alert"] as? String ?? ""
        body = (o["body"] as? [[String: Any]] ?? []).map(cell)
    }
}

// **`Reading` はここに在った**（2026-09-16 に外した・本人「いらない」）。
//
// 「表示」画面は `Paper.swift` に一本化してある ── `core` の `to_html` を
// WKWebView で出し、書き戻しはデスクトップ版と共有した一組（`paper.js`）を通る。
// `Reading` は SwiftUI で組み直した二つ目の読み手で、**どこからも呼ばれて
// いなかった。** 残しておくと「Markdown の読み手がiPhone にもう一つある」
// ように見えるうえ、こちらは `AttributedString(markdown:)` を使うので
// **逃がしの読み方がデスクトップ版と違った** ── 使われていない食い違いは、いつか
// 誰かが繋いだ日に本物の食い違いになる。
//
// この1 つに残したのは `Block` だけ ── `NotesStore.blocks(of:)` が
// core の答えを組み立てる型で、デスクトップ版の画面にもiPhone の画面にも要る。
