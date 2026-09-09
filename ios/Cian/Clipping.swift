import Foundation
import UIKit
import WebKit

/// **Web のページを、一本のノートに**（依頼 421 の乙・電話の側）。
///
/// 窓と同じ手順を、同じ道具で: 取りに行く → 均す → 字にする → ノートにする。
/// **字にするのは窓と同じ一組**（`paper.js` の `webClean` / `webToMd`）──
/// 二本目の変換器を書くと、同じページが端末によって別の字で残る。
///
/// そのために見えない `WKWebView` を一つ持つ。重そうに見えるが、**開くのは
/// 取り込むときだけ**で、終われば捨てる ── 常に居るものではない。
@MainActor
final class Clipping: NSObject {
    /// 取りに行ける URL か。**`http`/`https` だけ** ── `file:` を許すと、
    /// 打った URL で機械の中のファイルを読み出せることになる（窓と同じ）。
    static func reach(_ text: String) -> URL? {
        var t = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if t.isEmpty { return nil }
        // 「example.com/…」と打つ人のために、頭が無ければ足す。
        if !t.contains("://") { t = "https://" + t }
        guard let u = URL(string: t), let s = u.scheme?.lowercased(),
              s == "http" || s == "https", u.host != nil else { return nil }
        return u
    }

    enum Trouble: LocalizedError {
        case road, reply(String), notPage(String), tooBig, empty, js(String)
        var errorDescription: String? {
            switch self {
            case .road: return "URL の形になっていません"
            case .reply(let why): return why
            case .notPage(let kind): return "ページではありません（\(kind)）"
            case .tooBig: return "大きすぎます（8MB まで）"
            case .empty: return "本文が見つかりませんでした"
            case .js(let why): return "本文を読み取れませんでした（\(why)）"
            }
        }
    }

    /// 取ってきて、ノートに入れる字にする。題も返す。
    func clip(_ url: URL) async throws -> (title: String, body: String) {
        let (data, answer) = try await fetch(url)
        let kind = (answer as? HTTPURLResponse)?.value(forHTTPHeaderField: "Content-Type") ?? ""
        if !kind.isEmpty, !(kind.contains("html") || kind.contains("xml")) {
            throw Trouble.notPage(kind)
        }
        if data.count > 8 * 1024 * 1024 { throw Trouble.tooBig }
        let here = answer.url ?? url
        let got = try await turn(Self.text(data, said: kind), at: here)
        if got.md.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            throw Trouble.empty
        }

        // **出どころは本文の最後に、字として**（依頼 423・窓と同じ）。
        // 前書きに `source:` は足さない ── amber の都合をノートに書かない。
        let day = DateFormatter()
        day.dateFormat = "yyyy-MM-dd"
        let from = Self.host(here) + here.path
        let body = got.md + "\n\n---\n\n出典: [\(from)](\(here.absoluteString))"
            + "（\(day.string(from: Date())) に取り込み）\n"
        return (got.title, body)
    }

    /// 出どころの行に出す**宿の名前**。
    ///
    /// **窓と同じ形にする。** JS の `new URL(…).host` は番号を付ける
    /// （既定の 80 と 443 のときだけ落とす）が、Swift の `URL.host` は
    /// いつも落とす ── 同じページを取り込んでも、電話の一行だけ番号が
    /// 消えていた。
    private static func host(_ url: URL) -> String {
        let name = url.host ?? ""
        guard let port = url.port else { return name }
        let plain = url.scheme?.lowercased() == "https" ? 443 : 80
        return port == plain ? name : "\(name):\(port)"
    }

    private func fetch(_ url: URL) async throws -> (Data, URLResponse) {
        var ask = URLRequest(url: url, timeoutInterval: 15)
        // **名乗る。** 名乗らないものを断る先がある。
        ask.setValue("amber (+markdown clipper)", forHTTPHeaderField: "User-Agent")
        ask.setValue("text/html,application/xhtml+xml", forHTTPHeaderField: "Accept")
        do {
            let (data, answer) = try await URLSession.shared.data(for: ask)
            if let http = answer as? HTTPURLResponse, !(200...299).contains(http.statusCode) {
                throw Trouble.reply("\(http.statusCode) \(HTTPURLResponse.localizedString(forStatusCode: http.statusCode))")
            }
            return (data, answer)
        } catch let e as Trouble {
            throw e
        } catch {
            throw Trouble.reply(error.localizedDescription)
        }
    }

    /// **ページが名乗る文字の種類を先に見る。** 日本語の古いページは
    /// Shift_JIS のことがあり、UTF-8 で読むと全部化ける（窓と同じ）。
    private static func text(_ data: Data, said kind: String) -> String {
        let names = [name(in: kind), name(in: String(decoding: data.prefix(4096), as: UTF8.self))]
        for n in names.compactMap({ $0 }) {
            let cf = CFStringConvertIANACharSetNameToEncoding(n as CFString)
            if cf != kCFStringEncodingInvalidId {
                let enc = String.Encoding(rawValue: CFStringConvertEncodingToNSStringEncoding(cf))
                if let s = String(data: data, encoding: enc) { return s }
            }
        }
        return String(decoding: data, as: UTF8.self)
    }

    private static func name(in text: String) -> String? {
        guard let r = text.range(of: "charset=", options: .caseInsensitive) else { return nil }
        let tail = text[r.upperBound...].prefix(40)
        let got = tail.drop { $0 == "\"" || $0 == "'" }
            .prefix { $0.isLetter || $0.isNumber || $0 == "-" || $0 == "_" }
        return got.isEmpty ? nil : String(got)
    }

    // MARK: 字にするところ

    private var web: WKWebView?

    /// HTML を **題と本文**に。**窓と同じ切り出しに通す。**
    ///
    /// 見えない面を一つ建てて `paper.js` を読ませ、その中の `clipTitle` と
    /// `bestPart` と `webToMd` を、**窓の `cmdClip` と一字一句同じ順で**呼ぶ
    /// ── Swift 側で書き直さないので、片方だけ直った日に窓と電話で別の字に
    /// ならない。
    ///
    /// **字は引数で渡す。** 前は本文を JS の中に差し込んでいたが、差し込む
    /// 目印（`HTML`）が `outerHTML` の中にもあって、置き換えが命令を壊した
    /// ── 実際に「JavaScript の例外処理」だけが出て、何も取り込めなかった。
    private func turn(_ html: String, at url: URL) async throws
        -> (title: String, md: String) {
        let web = try ready()
        let js = """
        return { title: clipTitle(html), md: webToMd(bestPart(html), base) };
        """
        do {
            let got = try await web.callAsyncJavaScript(
                js, arguments: ["html": html, "base": url.absoluteString],
                contentWorld: .page)
            guard let out = got as? [String: Any] else { throw Trouble.empty }
            return (out["title"] as? String ?? "", out["md"] as? String ?? "")
        } catch let e as Trouble {
            throw e
        } catch {
            // **中で何が落ちたかを言う。** `WKError` の言い分は「例外が
            // 起きました」だけで、直す手がかりが一つも残らない。
            let why = (error as NSError)
                .userInfo["WKJavaScriptExceptionMessage"] as? String
            NSLog("amber clip: %@", why ?? error.localizedDescription)
            throw Trouble.js(why ?? error.localizedDescription)
        }
    }

    /// 見えない面を建てて、切り出しを読ませる。
    ///
    /// 二つ、素直に書くと動かないところがある:
    ///
    /// 一。**切り出しは `WKUserScript` で入れる。** HTML の字の中に混ぜると、
    /// ページの側の都合（`</script` の並び）一つで途中で切れる。
    ///
    /// 二。**面を窓にぶら下げる。** どこにも乗っていない `WKWebView` は
    /// 中身の処理が止められることがあり、`evaluateJavaScript` が返って
    /// こない ── 実際に「取りに行っています…」から動かなくなった。
    /// 大きさ零で置いて、誰にも見えないまま生かしておく。
    private func ready() throws -> WKWebView {
        if let web { return web }
        // `paper.js` は窓から切り出したもの（組み立てのときに写している）。
        let slice = Bundle.main.url(forResource: "paper", withExtension: "js")
            .flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? ""
        let setup = WKWebViewConfiguration()
        setup.userContentController.addUserScript(
            WKUserScript(source: slice, injectionTime: .atDocumentEnd,
                         forMainFrameOnly: true))
        let made = WKWebView(frame: .zero, configuration: setup)
        made.isHidden = true
        if let on = UIApplication.shared.connectedScenes
            .compactMap({ ($0 as? UIWindowScene)?.keyWindow }).first {
            on.addSubview(made)
        }
        // **道は付けない。** `amber://` を渡すと、その道を配る係が
        // この面には居ないので、頁がそもそも建たない ── 建たなければ
        // 切り出しも入らず、`webClean` が居ないと言われる。
        made.loadHTMLString("<!doctype html><meta charset=\"utf-8\"><body>",
                            baseURL: nil)
        web = made
        return made
    }

    /// 読み終わるまで待つ。**待たずに呼ぶと `webToMd` がまだ居ない。**
    func warm() async {
        guard let web = try? ready() else { return }
        for _ in 0..<40 {
            if let ok = try? await web.evaluateJavaScript("typeof webToMd"),
               (ok as? String) == "function" { return }
            try? await Task.sleep(nanoseconds: 100_000_000)
        }
    }
}
