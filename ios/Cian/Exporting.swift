import UIKit

/// **エクスポート**（依頼 516・窓の `cmdExport` と同じ三つ）── Markdown はファイルそのもの、
/// HTML は一枚で完結する読める形、PDF はそれを刷ったもの。
///
/// 読める形にするのは core（`html`）で、**窓と同じ一組**。画像は一枚の中に入れる
/// （`data:`）── 人に送った先で画像が出ないのは、「一枚で完結」と言っていることに反する。
@MainActor
enum Exporting {
    /// 窓の `onePage` と同じ姿（同じ CSS）。
    private static func page(_ title: String, _ body: String) -> String {
        let t = title.replacingOccurrences(of: "&", with: "&amp;").replacingOccurrences(of: "<", with: "&lt;")
        return """
        <!doctype html><html lang="ja"><head><meta charset="utf-8"><title>\(t)</title><style>
        body{max-width:44rem;margin:3rem auto;padding:0 1.4rem;font:16px/1.9 -apple-system,BlinkMacSystemFont,"Hiragino Sans","Yu Gothic UI",sans-serif;color:#2a2011;background:#fffdf8}
        h1,h2,h3,h4{line-height:1.4;margin:1.6em 0 .5em}h2{padding-bottom:.2em;border-bottom:1px solid #efe6d4}
        pre>code.language-amber{color:#b5760f;display:block;line-height:1}
        code{font:.88em/1.6 ui-monospace,Menlo,monospace;background:#f3ecdf;border:1px solid #efe6d4;border-radius:5px;padding:.1em .35em}
        pre{padding:11px 14px;overflow-x:auto;background:#f3ecdf;border:1px solid #efe6d4;border-radius:9px}pre code{background:none;border:0;padding:0}
        blockquote{margin:.85em 0;padding:.1em 0 .1em 1em;border-left:3px solid #e4d9c4;color:#6b5a41}
        table{border-collapse:collapse}th,td{border:1px solid #e4d9c4;padding:5px 11px}th{background:#f3ecdf}
        img{max-width:100%;height:auto;border-radius:8px}hr{border:0;border-top:1px solid #e4d9c4;margin:1.6em 0}
        a{color:#b5760f}li.task{list-style:none;margin-left:-1.35em}
        .box{display:inline-block;width:1.35em;background:none;border:0;color:#b5760f;font-size:1.05em}
        </style></head><body>\(body)</body></html>
        """
    }

    /// 画像を一枚の中へ（`attachments/…` のような隣を指す道だけ。`http` などはそのまま）。
    private static func inlinePictures(_ html: String, near dir: URL) -> String {
        guard let re = try? NSRegularExpression(pattern: "src=\"([^\"]+)\"") else { return html }
        var out = html
        let hits = re.matches(in: html, range: NSRange(html.startIndex..., in: html)).reversed()
        for m in hits {
            guard let r = Range(m.range(at: 1), in: html) else { continue }
            let src = String(html[r])
            if src.contains(":") || src.hasPrefix("//") || src.hasPrefix("data:") { continue }
            let at = dir.appendingPathComponent(src.removingPercentEncoding ?? src)
            guard let data = try? Data(contentsOf: at) else { continue }
            let ext = at.pathExtension.lowercased()
            let mime = ["png": "image/png", "jpg": "image/jpeg", "jpeg": "image/jpeg", "gif": "image/gif",
                        "webp": "image/webp", "svg": "image/svg+xml", "heic": "image/heic"][ext] ?? "application/octet-stream"
            let uri = "data:" + mime + ";base64," + data.base64EncodedString()
            if let whole = Range(m.range, in: out) { out.replaceSubrange(whole, with: "src=\"" + uri + "\"") }
        }
        return out
    }

    private static func standalone(_ note: Note, text: String) throws -> String {
        let body = try Cian.call("html", ["text": text])["html"] as? String ?? ""
        let dir = URL(fileURLWithPath: note.path).deletingLastPathComponent()
        return page(note.shown, inlinePictures(body, near: dir))
    }

    private static func out(_ note: Note, _ ext: String) -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("amber-export", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let stem = URL(fileURLWithPath: note.path).deletingPathExtension().lastPathComponent
        return dir.appendingPathComponent(stem + "." + ext)
    }

    /// HTML 一枚。
    static func html(_ note: Note, text: String) throws -> URL {
        let at = out(note, "html")
        try standalone(note, text: text).write(to: at, atomically: true, encoding: .utf8)
        return at
    }

    /// PDF（A4・余白 40pt・頁に分ける）。刷るのは iOS の印刷の仕組み ── 窓が
    /// 見えない窓で `printToPDF` するのと同じで、読める形だけを刷る。
    static func pdf(_ note: Note, text: String) throws -> URL {
        let html = try standalone(note, text: text)
        let fmt = UIMarkupTextPrintFormatter(markupText: html)
        let renderer = UIPrintPageRenderer()
        renderer.addPrintFormatter(fmt, startingAtPageAt: 0)
        let paper = CGRect(x: 0, y: 0, width: 595.2, height: 841.8)
        renderer.setValue(paper, forKey: "paperRect")
        renderer.setValue(paper.insetBy(dx: 40, dy: 40), forKey: "printableRect")
        let data = NSMutableData()
        UIGraphicsBeginPDFContextToData(data, paper, nil)
        for i in 0..<max(renderer.numberOfPages, 1) {
            UIGraphicsBeginPDFPage()
            renderer.drawPage(at: i, in: UIGraphicsGetPDFContextBounds())
        }
        UIGraphicsEndPDFContext()
        let at = out(note, "pdf")
        try data.write(to: at, options: .atomic)
        return at
    }
}
