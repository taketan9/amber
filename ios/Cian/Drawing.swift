import SwiftUI
import WebKit

/// A mermaid diagram, drawn.
///
/// **The phone used to show the source.** `flowchart LR` and four indented
/// lines, in a grey box, where the Mac showed a picture — the same note read
/// as two different things depending on which amber you opened it in. That is
/// the one thing this app is not allowed to do.
///
/// Drawn by mermaid itself in a `WKWebView`, not by a Swift renderer written
/// for the phone: a second implementation of eight diagram kinds is a second
/// set of answers, and the interesting ones (mindmap layout, gantt scales) are
/// exactly where two implementations would disagree.
///
/// **The library is not in the app's memory until a note has a diagram in it.**
/// mermaid is 3.4MB and most notes have no diagram, which is the same reason
/// the window loads it late.
struct Drawing: View {
    let source: String
    @Environment(\.colorScheme) private var scheme
    @State private var tall: CGFloat = 120
    @State private var missing = false

    var body: some View {
        Group {
            if missing || Diagrams.tool == nil {
                // **Say the source, rather than nothing.** A diagram the app
                // cannot draw is still a diagram somebody wrote; hiding it
                // would lose the note's content on this device only.
                ScrollView(.horizontal, showsIndicators: false) {
                    Text(source).font(.callout.monospaced())
                }
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.accentColor.opacity(0.10), in: RoundedRectangle(cornerRadius: 8))
            } else {
                Paper(source: source, dark: scheme == .dark, tall: $tall)
                    .frame(height: tall)
                    .frame(maxWidth: .infinity)
            }
        }
        .onAppear { missing = Diagrams.tool == nil }
    }
}

/// Where the drawing tools are unpacked.
///
/// The library and the page that uses it are copied out of the bundle once,
/// into Caches: a `WKWebView` will load a `file:` page and let it fetch a
/// script **beside** it, but it will not read out of the app bundle for a page
/// built from a string. Copying is cheaper than inlining 3.4MB into the HTML
/// of every diagram on screen.
enum Diagrams {
    static let tool: URL? = unpack()

    private static func unpack() -> URL? {
        guard let lib = Bundle.main.url(forResource: "mermaid.min", withExtension: "js") else {
            return nil        // built without `node gui/vendor.js`
        }
        let fm = FileManager.default
        guard let caches = fm.urls(for: .cachesDirectory, in: .userDomainMask).first else { return nil }
        let dir = caches.appendingPathComponent("draw", isDirectory: true)
        let js = dir.appendingPathComponent("mermaid.min.js")
        let page = dir.appendingPathComponent("draw.html")
        do {
            try fm.createDirectory(at: dir, withIntermediateDirectories: true)
            // Copy again when the app is newer than what is unpacked —
            // otherwise an update ships a new mermaid that never gets used.
            if !fm.fileExists(atPath: js.path) || newer(lib, than: js) {
                try? fm.removeItem(at: js)
                try fm.copyItem(at: lib, to: js)
            }
            try Self.html.write(to: page, atomically: true, encoding: .utf8)
        } catch {
            return nil
        }
        return page
    }

    private static func newer(_ a: URL, than b: URL) -> Bool {
        let at = (try? a.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
        let bt = (try? b.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
        guard let at, let bt else { return true }
        return at > bt
    }

    /// The eleven colours. **The same eleven the window uses** — see `FAMILY`
    /// in `gui/renderer.js`. Two lists would mean the same pie chart came out
    /// in different colours on the phone, which is the kind of difference
    /// nobody reports and everybody notices.
    static let family = [
        "#F7BD5C", "#8FC8E8", "#A8D9A8", "#C9AEE0", "#F7A99C", "#8ED9CE",
        "#EFDA8A", "#AEBBEE", "#F4B4CE", "#C6DE8E", "#D3D3D9",
    ]
    static let ink = "#3a2408"

    private static let html = """
    <!doctype html><html><head><meta charset="utf-8">
    <meta name="viewport" content="width=device-width,initial-scale=1">
    <style>
      html,body{margin:0;padding:0;background:transparent;
        -webkit-text-size-adjust:100%;overflow-x:auto;overflow-y:hidden}
      #box{display:flex;justify-content:center;padding:1px}
      #box svg{max-width:100%;height:auto}
      #bad{margin:0;padding:9px 11px;border-radius:8px;white-space:pre-wrap;
        font:13px/1.6 -apple-system,sans-serif}
    </style></head><body><div id="box"></div>
    <script src="mermaid.min.js"></script>
    <script>
    window.onerror = (m, src, line) => {
      window.webkit.messageHandlers.trouble.postMessage(m + ' @' + src + ':' + line);
    };
    function tell() {
      // Two frames: the first lets the SVG land, the second lets the
      // browser finish laying it out. Measuring once too early reports a
      // height of nothing and the diagram opens as a sliver.
      requestAnimationFrame(() => requestAnimationFrame(() => {
        const h = document.getElementById('box').getBoundingClientRect().height;
        window.webkit.messageHandlers.tall.postMessage(Math.ceil(h) + 2);
      }));
    }
    window.draw = async (src, opts, bad) => {
      const box = document.getElementById('box');
      try {
        mermaid.initialize(opts);
        const { svg } = await mermaid.render('m' + Math.random().toString(36).slice(2), src);
        box.innerHTML = svg;
      } catch (e) {
        // **Show what was written, and why it did not draw.** An empty
        // space says the note lost something.
        const p = document.createElement('pre');
        p.id = 'bad';
        p.style.background = bad.bg;
        p.style.color = bad.fg;
        p.textContent = '図にできません: ' + String((e && e.message) || e) + '\\n\\n' + src;
        box.innerHTML = '';
        box.append(p);
      }
      tell();
    };
    </script></body></html>
    """
}

/// The web view itself.
private struct Paper: UIViewRepresentable {
    let source: String
    let dark: Bool
    @Binding var tall: CGFloat

    func makeCoordinator() -> Hand { Hand(tall: $tall) }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.userContentController.add(context.coordinator, name: "tall")
        config.userContentController.add(context.coordinator, name: "trouble")
        let web = WKWebView(frame: .zero, configuration: config)
        web.navigationDelegate = context.coordinator
        // The note's background shows through — a white card behind every
        // diagram would be the one bright rectangle in a dark note.
        web.isOpaque = false
        web.backgroundColor = .clear
        web.scrollView.backgroundColor = .clear
        // The page reports its own height and the list scrolls; a scroll
        // view inside a scroll view swallows the flick.
        web.scrollView.isScrollEnabled = false
        web.scrollView.bounces = false
        // **触りは通す。** 図そのものは押しても何もしないので、web view が
        // 受け取る意味が無い ── 受け取ってしまうと、長押しで工房を開く手が
        // 図の上でだけ効かなくなる（いちばん押したいところで）。
        web.isUserInteractionEnabled = false
        if let page = Diagrams.tool {
            web.loadFileURL(page, allowingReadAccessTo: page.deletingLastPathComponent())
        }
        return web
    }

    func updateUIView(_ web: WKWebView, context: Context) {
        context.coordinator.want = (source, dark)
        context.coordinator.draw(web)
    }

    final class Hand: NSObject, WKNavigationDelegate, WKScriptMessageHandler {
        @Binding var tall: CGFloat
        var want: (String, Bool)?
        private var drawn: String?
        private weak var web: WKWebView?
        private var ready = false

        init(tall: Binding<CGFloat>) { _tall = tall }

        func webView(_ web: WKWebView, didFail: WKNavigation!, withError e: Error) {
            print("[図] 頁を開けません: \(e)")
        }

        func webView(_ web: WKWebView, didFinish: WKNavigation!) {
            ready = true
            self.web = web
            draw(web)
        }

        func draw(_ web: WKWebView) {
            self.web = web
            guard ready, let (src, dark) = want else { return }
            // Redrawing the same diagram on every layout pass would reload
            // mermaid's layout engine while the reader is scrolling past.
            let key = src + (dark ? "#dark" : "#light")
            guard key != drawn else { return }
            drawn = key
            let opts = Self.options(dark: dark)
            // **返り値を残さない。** `draw` は async なので、最後の式の値は
            // Promise になり、WKWebView は「対応していないタイプ」を返す ──
            // 図はちゃんと描けているのに、毎回エラーが一つ記録される。
            let call = "window.draw(\(json(src)), \(opts), \(Self.bad(dark))); true"
            web.evaluateJavaScript(call) { _, err in
                if let err { print("[図] 呼べません: \(err)") }
            }
        }

        func userContentController(
            _ c: WKUserContentController, didReceive message: WKScriptMessage
        ) {
            if message.name == "trouble" {
                print("[図] \(message.body)")
                return
            }
            guard let h = message.body as? NSNumber else { return }
            let want = max(CGFloat(truncating: h), 40)
            if abs(want - tall) > 1 { tall = want }
        }

        /// A Swift value as a JavaScript literal.
        private func json(_ v: Any) -> String {
            guard let d = try? JSONSerialization.data(withJSONObject: [v], options: []),
                  let s = String(data: d, encoding: .utf8)
            else { return "\"\"" }
            return String(s.dropFirst().dropLast())
        }

        /// The colours for a diagram that would not draw.
        ///
        /// **Two hashes on the raw string.** With one, the `"#` in `"#3b2f16"`
        /// ends the literal and the rest of the line is read as code.
        static func bad(_ dark: Bool) -> String {
            dark ? ##"{"bg":"#3b2f16","fg":"#bcac91"}"## : ##"{"bg":"#f7e2b6","fg":"#6b5a41"}"##
        }

        /// **The same settings the window uses** (`mermaidOpts` in
        /// `gui/renderer.js`), with the phone's colours in place of the CSS
        /// variables. The window reads its theme from the page; here the two
        /// themes are written out, because a phone has exactly two.
        static func options(dark: Bool) -> String {
            let paper = dark ? "#14110c" : "#fffdf8"
            let rail = dark ? "#1d1913" : "#f3ecdf"
            let list = dark ? "#1a1611" : "#f8f3e8"
            let line = dark ? "#302a20" : "#e4d9c4"
            let ink = dark ? "#f0e7d6" : "#2a2011"
            let ink2 = dark ? "#bcac91" : "#6b5a41"
            let ink3 = dark ? "#8a7d66" : "#9a8a6f"
            let amber = "#f0a52b"
            var vars: [String: String] = [
                "background": paper, "primaryColor": rail, "primaryTextColor": ink,
                "primaryBorderColor": amber, "secondaryColor": list, "tertiaryColor": paper,
                "lineColor": ink3, "textColor": ink, "mainBkg": rail, "nodeBorder": amber,
                "clusterBkg": list, "clusterBorder": line, "edgeLabelBackground": paper,
                "pieStrokeColor": paper, "pieOuterStrokeColor": line, "pieTitleTextColor": ink,
                "pieSectionTextColor": Diagrams.ink, "pieLegendTextColor": ink2,
                "pieOpacity": "1", "fontSize": "14px", "nodeTextColor": ink,
            ]
            for (n, c) in Diagrams.family.enumerated() {
                vars["pie\(n + 1)"] = c
                vars["cScale\(n)"] = c
                vars["cScaleInv\(n)"] = c
                // Every one of the eleven is a pale wash, so the label on it
                // is always the dark ink — the window works this out from the
                // colour's lightness and lands in the same place.
                vars["cScaleLabel\(n)"] = Diagrams.ink
            }
            let mind = Diagrams.family.enumerated().map { n, c in
                ".mindmap-node.section-\(n) .node-bkg{fill:color-mix(in srgb,\(c) 46%,\(paper));"
                + "stroke:color-mix(in srgb,\(c) 78%,#6b5a41)}"
                + ".mindmap-node.section-\(n) .nodeLabel{color:\(ink);font-weight:600}"
                + ".mindmap-node.section-\(n) line{stroke:color-mix(in srgb,\(c) 78%,#6b5a41);stroke-width:2px}"
                + ".edge.section-edge-\(n){stroke:color-mix(in srgb,\(c) 72%,#6b5a41);stroke-width:2.5px}"
            }.joined()
            let css = ".pieTitleText{font-size:15px;font-weight:700}"
                + ".slice{font-size:13px;font-weight:600}"
                + ".pieCircle{stroke:\(paper);stroke-width:2px}"
                + ".pieOuterCircle{stroke:\(line)}"
                + ".legend text{font-size:13px}"
                + ".timeline text,.timeline tspan{font-size:15px}"
                + ".mindmap-node.section--1 circle.basic{fill:#C97F16;stroke:\(amber);"
                + "stroke-width:3px;filter:drop-shadow(0 2px 5px rgba(0,0,0,.28))}"
                + ".mindmap-node.section--1 .nodeLabel{color:#fff;font-weight:700;font-size:15px}"
                + mind
            let opts: [String: Any] = [
                "startOnLoad": false,
                "theme": "base",
                "themeVariables": vars.merging(["darkMode": dark ? "true" : "false"]) { a, _ in a },
                "themeCSS": css,
                // **A phone is 402 points wide.** The window lets a diagram
                // run to 52em and scroll; here everything has to fit, so the
                // spacings are tighter and the boxes narrower.
                "flowchart": ["curve": "basis", "padding": 10, "nodeSpacing": 28,
                              "rankSpacing": 32, "htmlLabels": true, "useMaxWidth": true],
                "pie": ["textPosition": 0.62, "useMaxWidth": true],
                "sequence": ["actorMargin": 28, "mirrorActors": false, "useMaxWidth": true],
                "timeline": ["useMaxWidth": true, "width": 140, "height": 60, "padding": 8,
                             "boxMargin": 9, "boxTextMargin": 6, "diagramMarginX": 14,
                             "diagramMarginY": 14, "leftMargin": 46],
                "gantt": ["useWidth": 720, "useMaxWidth": true, "barHeight": 20, "barGap": 6,
                          "topPadding": 42, "leftPadding": 72, "gridLineStartPadding": 26,
                          "fontSize": 11, "sectionFontSize": 11, "numberSectionStyles": 4],
                // A note is something a person wrote. HTML in a diagram's
                // label does not get to run.
                "securityLevel": "strict",
                "fontFamily": "-apple-system, \"Hiragino Sans\", sans-serif",
            ]
            guard let d = try? JSONSerialization.data(withJSONObject: opts),
                  let s = String(data: d, encoding: .utf8)
            else { return "{}" }
            return s
        }
    }
}
