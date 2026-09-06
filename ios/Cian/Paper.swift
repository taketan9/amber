import SwiftUI
import WebKit

/// 「表示」の面（電話）── **窓と同じものを動かす。**
///
/// 窓の「表示」は `<div contenteditable>` で、そこに直接打てる。電話だけ
/// 読むだけだと、**同じ名前の面が二つの amber で別のもの**になる ── 打てる
/// と思って叩いた人は、何も起きない画面を前に「壊れている」と思う。
///
/// SwiftUI で書き直さない。組む側は core の `to_html`、書き戻す側は
/// `gui/renderer.js` から切り出した一組（`paper.js`）── **書き戻しをもう一組
/// Swift で書けば、同じノートが端末によって別の字に保存される**。失うのは
/// たいてい表と升と図で、気づくのは何回か保存したあと。
///
/// 絵と図は `amber://` で配る。`WKWebView` は文字列から作った頁に隣の
/// ファイルを読ませないので、**Swift が給仕する**（束ねの中の道具と、
/// ノートの隣の絵）。ついでに、ノートの外は配らないことがここで保証できる。
/// 道具の帯から「表示」の面へ、合図を渡す口。
///
/// **帯は SwiftUI、面は WebView。** 間に糸を一本通しておかないと、帯は
/// 書く面だけの道具のままになる ── 電話で「表示」に打てるようにした意味が
/// 半分になる。
final class PaperHand: ObservableObject {
    var send: ((String) -> Void)?
    func mark(_ what: String) { send?(what) }
}

struct Paper: UIViewRepresentable {
    /// いまのノートの Markdown（前書きを除いた本文）。
    @Binding var text: String
    /// このノートのあるフォルダ ── 絵の道はここから測る。
    let folder: URL
    let dark: Bool
    /// 升を押したときなど、core を通したいことがある。
    var onCheck: ((Int, Bool) -> Void)?
    /// 図を長押しされた（工房を開く）。
    var onFix: ((String) -> Void)?
    /// 道具の帯からの合図を受け取る糸。
    var hand: PaperHand?

    func makeCoordinator() -> Hand { Hand(self) }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.userContentController.add(context.coordinator, name: "wrote")
        config.userContentController.add(context.coordinator, name: "tick")
        config.userContentController.add(context.coordinator, name: "fix")
        config.userContentController.add(context.coordinator, name: "trouble")
        config.setURLSchemeHandler(context.coordinator, forURLScheme: Waiter.scheme)
        let web = WKWebView(frame: .zero, configuration: config)
        web.navigationDelegate = context.coordinator
        web.isOpaque = false
        web.backgroundColor = .clear
        web.scrollView.backgroundColor = .clear
        web.scrollView.keyboardDismissMode = .interactive
        context.coordinator.web = web
        context.coordinator.folder = folder
        hand?.send = { [weak web] what in
            web?.evaluateJavaScript("window.mark(\"\(what)\"); true")
        }
        web.loadHTMLString(Self.page, baseURL: URL(string: Waiter.scheme + "://app/")!)
        return web
    }

    func updateUIView(_ web: WKWebView, context: Context) {
        context.coordinator.parent = self
        context.coordinator.folder = folder
        context.coordinator.show(text, dark: dark)
    }

    /// 面そのもの。**窓の見た目に寄せる** ── 同じノートが二つの amber で
    /// 同じ形に見えないと、「同じ面」と言えない。
    static let page = """
    <!doctype html><html><head><meta charset="utf-8">
    <meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1">
    <style>
      :root{--paper:#fffdf8;--ink:#2a2011;--ink-2:#6b5a41;--ink-3:#9a8a6f;
        --line:#e4d9c4;--line-2:#efe6d4;--amber:#f0a52b;--amber-deep:#b5760f;
        --rail:#f3ecdf;--sel:#f7e2b6;}
      html[data-dark]{--paper:#14110c;--ink:#f0e7d6;--ink-2:#bcac91;--ink-3:#8a7d66;
        --line:#302a20;--line-2:#262017;--amber-deep:#e0a94e;--rail:#1d1913;--sel:#3b2f16;}
      html,body{margin:0;padding:0;background:transparent;color:var(--ink);
        font:17px/1.85 -apple-system,"Hiragino Sans",sans-serif;
        -webkit-text-size-adjust:100%;}
      #paper{outline:none;padding:14px 16px 45vh;caret-color:var(--amber-deep);
        min-height:60vh;}
      #paper>*{margin:.85em 0}
      #paper h1{font-size:1.5em;font-weight:700;margin:.6em 0 .3em}
      #paper h2{font-size:1.25em;font-weight:700;margin:1.2em 0 .3em;
        border-bottom:1px solid var(--line-2);padding-bottom:.2em}
      #paper h3{font-size:1.08em;font-weight:700;margin:1.1em 0 .2em}
      #paper ul,#paper ol{padding-left:1.4em}
      #paper li{margin:.25em 0}
      #paper blockquote{margin:.9em 0;padding-left:.9em;
        border-left:3px solid var(--line);color:var(--ink-2)}
      #paper hr{border:0;border-top:1px solid var(--line);margin:1.6em 0}
      #paper code{font:.88em ui-monospace,Menlo,monospace;background:var(--rail);
        padding:.1em .35em;border-radius:5px}
      #paper pre{background:var(--rail);padding:11px 13px;border-radius:9px;
        overflow-x:auto}
      #paper pre code{background:none;padding:0}
      #paper table{border-collapse:collapse;display:block;overflow-x:auto;
        max-width:100%}
      #paper th,#paper td{border:1px solid var(--line);padding:6px 10px;
        text-align:left}
      #paper th{background:var(--rail);font-weight:700}
      #paper img{max-width:100%;height:auto;border-radius:9px;
        border:1px solid var(--line)}
      #paper .box{appearance:none;width:19px;height:19px;margin:0 7px 0 -1.5em;
        border:1.6px solid var(--ink-3);border-radius:5px;background:none;
        vertical-align:-4px}
      #paper .box[aria-pressed=true]{background:var(--amber);border-color:var(--amber);}
      #paper li:has(>.box){list-style:none}
      #paper .alert{margin:.9em 0;padding:.1em .9em .1em .9em;border-radius:9px;
        border-left:3px solid var(--amber);background:var(--sel)}
      #paper .alert-h{font-weight:700;color:var(--amber-deep);margin:.7em 0 .2em}
      #paper .mermaid{margin:1.2em 0;text-align:center;overflow-x:auto}
      #paper .mermaid svg{max-width:100%;height:auto}
    </style></head><body><div id="paper"></div>
    <script src="paper.js"></script>
    <script>
    const box = document.getElementById('paper');
    let head = '';
    let quiet = false;
    let hold = null;

    window.onerror = (m, s, l) =>
      window.webkit.messageHandlers.trouble.postMessage(m + ' @' + l);

    /// 組み上がった姿を置いて、打てるようにする。
    window.show = (html, text, dark) => {
      document.documentElement.toggleAttribute('data-dark', !!dark);
      quiet = true;
      box.innerHTML = html;
      // 絵はノートの隣にある ── `amber://note/` で取りに行く。
      for (const img of box.querySelectorAll('img')) {
        const src = img.getAttribute('src') || '';
        if (!/^[a-z]+:/i.test(src)) img.src = 'amber://note/' + src;
      }
      armPaper(box, text, true);
      draw();
      quiet = false;
    };

    /// 打ったら、落ち着いてから字に戻して渡す。
    box.addEventListener('input', () => {
      if (quiet) return;
      clearTimeout(hold);
      hold = setTimeout(() => {
        const md = paperToMd(box, head);
        // **戻せないときは渡さない。** 空を渡すと、そのかたまりが黙って
        // 消える ── 気づくのは何回か保存したあと。
        if (md === null) return;
        window.webkit.messageHandlers.wrote.postMessage(md);
      }, 500);
    });

    /// 升は打つものではなく押すもの ── 行番号で裏返す（何番目かではない）。
    box.addEventListener('click', (e) => {
      const mark = e.target.closest('.box');
      if (mark) {
        e.preventDefault();
        window.webkit.messageHandlers.tick.postMessage({
          line: Number(mark.dataset.line), done: mark.getAttribute('aria-pressed') !== 'true',
        });
        return;
      }
    });

    /// 図は長押しで工房へ（窓は右押し）。
    let pressed = null;
    box.addEventListener('touchstart', (e) => {
      const art = e.target.closest('.mermaid, pre');
      if (!art || !art.dataset.md) return;
      pressed = setTimeout(() => {
        window.webkit.messageHandlers.fix.postMessage(art.dataset.md);
        pressed = null;
      }, 450);
    }, { passive: true });
    for (const ev of ['touchend', 'touchmove', 'touchcancel']) {
      box.addEventListener(ev, () => { clearTimeout(pressed); }, { passive: true });
    }

    /// caret のいるかたまり。
    function here() {
      const sel = getSelection();
      if (!sel || !sel.rangeCount) return null;
      let n = sel.getRangeAt(0).startContainer;
      while (n && n.parentElement !== box) n = n.parentElement || n.parentNode;
      return n && n.parentElement === box ? n : null;
    }

    /// 道具の帯から。窓と同じ `execCommand`。
    window.mark = (what) => {
      box.focus();
      if (what === 'bold') document.execCommand('bold');
      else if (what === 'italic') document.execCommand('italic');
      else if (what === 'strike') document.execCommand('strikeThrough');
      else if (what === 'ul') document.execCommand('insertUnorderedList');
      else if (what === 'ol') document.execCommand('insertOrderedList');
      else if (what === 'quote') document.execCommand('formatBlock', false, 'blockquote');
      else if (what === 'check') check();
      else if (what === 'head') {
        // 押すたびに深くなる ── 窓と同じ（`#` → `##` → `###` → 無し）。
        const n = here();
        const now = n && /^H[1-6]$/.test(n.tagName) ? Number(n.tagName[1]) : 0;
        document.execCommand('formatBlock', false, now >= 3 ? 'p' : 'h' + (now + 1));
      } else if (what.startsWith('h')) document.execCommand('formatBlock', false, what);
      box.dispatchEvent(new Event('input'));
    };

    /// いまの行を、押せる升の付いた一行にする。
    ///
    /// **`execCommand` に升は作れない。** 箇条書きにしてから、升を自分で
    /// 前に置く ── 升は `<button class="box">` で、`paperToMd` はそれを
    /// 見て `- [ ]` に戻す。
    function check() {
      const n = here();
      if (!n || n.closest('li')?.querySelector(':scope > .box')) return;
      if (n.tagName !== 'LI' && !n.closest('li')) {
        document.execCommand('insertUnorderedList');
      }
      const li = here()?.closest?.('li') || box.querySelector('li:focus-within');
      const at = li || here();
      if (!at || at.querySelector(':scope > .box')) return;
      const mark = document.createElement('button');
      mark.type = 'button';
      mark.className = 'box';
      mark.setAttribute('aria-pressed', 'false');
      mark.contentEditable = 'false';
      at.prepend(mark);
    }

    /// 升の行の Enter は、窓と同じ関数（`checkEnter`）に渡す。
    ///
    /// **押し心地を端末で分けない。** ここで別の答えを書けば、同じノートを
    /// 窓で足すと升、電話で足すと点、になる ── 気づくのは何日か経ってから。
    box.addEventListener('keydown', (e) => {
      if (e.key !== 'Enter' || e.isComposing || e.keyCode === 229) return;
      if (e.shiftKey || e.metaKey || e.ctrlKey) return;
      // `here()` は箱の直下まで登る（ここでは `<ul>`）ので、caret の
      // 節から直に数える。
      let n = getSelection()?.anchorNode;
      if (n && n.nodeType === 3) n = n.parentElement;
      if (!n || !box.contains(n)) return;
      const li = n.closest('li');
      if (!(li ? checkEnter(li) : false) && !quitEnter(n)) return;
      e.preventDefault();
      box.dispatchEvent(new Event('input'));
    });

    /// 打った字を、飾りの外へ（窓と同じ `outOfDress`）。
    ///
    /// **飾ったのは選んだ字で、これから打つ字ではない。** かな漢字は
    /// 組み始めに出す ── 組んでいる最中に選び目を動かすと変換が壊れる。
    box.addEventListener('beforeinput', (e) => {
      if (e.isComposing || e.inputType !== 'insertText' || e.data == null) return;
      if (!outOfDress()) return;
      e.preventDefault();
      document.execCommand('insertText', false, e.data);
      box.dispatchEvent(new Event('input'));
    });
    box.addEventListener('compositionstart', () => outOfDress());

    /// 図を描く。**要るときだけ読む** ── 3.4MB を、図の無いノートで払わない。
    let lib = null;
    async function draw() {
      const blocks = [...box.querySelectorAll('pre > code.language-mermaid')];
      if (!blocks.length) return;
      if (!lib) {
        await new Promise((go, no) => {
          const tag = document.createElement('script');
          tag.src = 'mermaid.min.js';
          tag.onload = go;
          tag.onerror = no;
          document.head.append(tag);
        }).catch(() => {});
        lib = globalThis.mermaid || null;
        if (!lib) return;
        // 図の色も設定も、窓と同じもの（`Drawing` が組み立てて渡す）。
        lib.initialize(window.__mmd || { startOnLoad: false, securityLevel: 'strict' });
      }
      for (const code of blocks) {
        try {
          const { svg } = await lib.render('m' + Math.random().toString(36).slice(2),
                                           code.textContent);
          const div = document.createElement('div');
          div.className = 'mermaid';
          div.innerHTML = svg;
          // **元の字と行番号を引き継ぐ。** 引き継がないと、字に戻すとき
          // この図の中身がどこにも無く、保存のたびに図が消える。
          for (const k of ['line', 'span', 'md']) {
            if (code.parentElement.dataset[k] !== undefined) {
              div.dataset[k] = code.parentElement.dataset[k];
            }
          }
          div.contentEditable = 'false';
          code.parentElement.replaceWith(div);
        } catch { /* 描けない図は、書いた字のまま残す */ }
      }
    }
    </script></body></html>
    """

    final class Hand: NSObject, WKNavigationDelegate, WKScriptMessageHandler, WKURLSchemeHandler {
        var parent: Paper
        weak var web: WKWebView?
        var folder: URL?
        private var ready = false
        private var shown = ""
        private var darkShown: Bool?

        init(_ parent: Paper) { self.parent = parent }

        func webView(_ web: WKWebView, didFinish: WKNavigation!) {
            ready = true
            shown = ""
            show(parent.text, dark: parent.dark)
        }

        /// 組み上がった姿を渡す。**組むのは core** ── 見出しが何かを電話が
        /// 決めはじめると、`#仕事` というタグの行が見出しになる。
        func show(_ text: String, dark: Bool) {
            guard ready, let web, text != shown || dark != darkShown else { return }
            shown = text
            darkShown = dark
            guard let out = try? Cian.call("html", ["text": text]),
                  let html = out["html"] as? String
            else { return }
            web.evaluateJavaScript(
                "window.__mmd = \(Paper.mmdOptions(dark: dark));"
                + "window.show(\(json(html)), \(json(text)), \(dark)); true")
        }

        func userContentController(
            _ c: WKUserContentController, didReceive m: WKScriptMessage
        ) {
            switch m.name {
            case "wrote":
                guard let md = m.body as? String else { return }
                // 自分が渡した姿が返ってきただけなら、書き直さない。
                guard md != shown else { return }
                shown = md
                parent.text = md
            case "tick":
                guard let d = m.body as? [String: Any],
                      let line = (d["line"] as? NSNumber)?.intValue,
                      let done = d["done"] as? Bool else { return }
                parent.onCheck?(line, done)
            case "fix":
                if let md = m.body as? String { parent.onFix?(md) }
            default:
                print("[表示] \(m.body)")
            }
        }

        private func json(_ v: Any) -> String {
            guard let d = try? JSONSerialization.data(withJSONObject: [v]),
                  let s = String(data: d, encoding: .utf8) else { return "\"\"" }
            return String(s.dropFirst().dropLast())
        }

        // ── 給仕 ────────────────────────────────────────────
        //
        // **ノートの外は配らない。** 絵の道はノートの隣から測り、`..` で
        // 外へ出ようとするものは返さない ── 人が書いた字を頁に載せている
        // ので、そこが外を指していないことは、こちらで確かめる。

        func webView(_ web: WKWebView, start task: WKURLSchemeTask) {
            guard let url = task.request.url else { return task.didFailWithError(Waiter.gone) }
            let name = url.lastPathComponent
            var at: URL?
            if url.host == "app" {
                at = Bundle.main.url(forResource: (name as NSString).deletingPathExtension,
                                     withExtension: (name as NSString).pathExtension)
            } else if url.host == "note", let folder {
                let rest = url.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
                let want = folder.appendingPathComponent(rest).standardizedFileURL
                if want.path.hasPrefix(folder.standardizedFileURL.path) { at = want }
            }
            guard let at, let body = try? Data(contentsOf: at) else {
                return task.didFailWithError(Waiter.gone)
            }
            let kind = Waiter.kinds[at.pathExtension.lowercased()] ?? "application/octet-stream"
            task.didReceive(URLResponse(url: url, mimeType: kind,
                                        expectedContentLength: body.count,
                                        textEncodingName: "utf-8"))
            task.didReceive(body)
            task.didFinish()
        }

        func webView(_ web: WKWebView, stop task: WKURLSchemeTask) {}
    }

    /// 図の設定 ── **`Drawing` と同じもの**。同じノートの同じ図が、
    /// 一枚の面と工房の中で違う色に出る理由が無い。
    static func mmdOptions(dark: Bool) -> String {
        Canvas.Hand.options(dark: dark)
    }

    enum Waiter {
        static let scheme = "amber"
        static let gone = NSError(domain: "amber", code: 404)
        static let kinds = [
            "js": "text/javascript", "css": "text/css", "png": "image/png",
            "jpg": "image/jpeg", "jpeg": "image/jpeg", "gif": "image/gif",
            "webp": "image/webp", "heic": "image/heic", "svg": "image/svg+xml",
        ]
    }
}
