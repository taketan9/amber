import SwiftUI

/// One piece of a note, ready to draw.
///
/// What a heading *is* was decided in `cian_core::note`; what it looks like is
/// decided here. Splitting it the other way would put a Markdown parser on
/// the phone, where no test can reach it.
struct Block: Identifiable {
    let id = UUID()
    let kind: String
    let text: String
    let level: Int
    let n: Int
    let lang: String
    let alt: String
    let link: String
    /// A task, and the line of the note it is written on.
    let done: Bool
    let line: Int
    /// The line cut into coloured and uncoloured pieces — worked out by
    /// `cian_core::note::spans`, so the window draws the same pieces.
    let runs: [(String, String?)]
    /// A table: the header, how each column lines up, and the rows.
    let head: [String]
    let align: [String]
    let rows: [[String]]
    /// `> [!NOTE]` — which of GitHub's five, and the paragraphs under it.
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
        // 升の中の飾りは `text` のまま持ってきて、描くときに読む ──
        // ここで剥がすと、Markdown の読み手が電話にもう一つ生える。
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
// 「表示」の面は `Paper.swift` に一本化してある ── `core` の `to_html` を
// WKWebView で出し、書き戻しは窓と共有した一組（`paper.js`）を通る。
// `Reading` は SwiftUI で組み直した二つ目の読み手で、**どこからも呼ばれて
// いなかった。** 残しておくと「Markdown の読み手が電話にもう一つある」
// ように見えるうえ、こちらは `AttributedString(markdown:)` を使うので
// **逃がしの読み方が窓と違った** ── 使われていない食い違いは、いつか
// 誰かが繋いだ日に本物の食い違いになる。
//
// この一枚に残したのは `Block` だけ ── `NotesStore.blocks(of:)` が
// core の答えを組み立てる型で、窓の面にも電話の面にも要る。
