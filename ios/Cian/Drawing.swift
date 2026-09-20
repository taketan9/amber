import SwiftUI
import WebKit

/// mermaid の図を描く。
///
/// **以前 iPhone はソースをそのまま出していた。** Mac が絵を出しているところで、
/// `flowchart LR` とインデントされた 4 行が灰色の箱に入って出ていた ── 同じ
/// ノートが、どちらの amber で開いたかで別のものとして読めていた。それは
/// このアプリがやってはいけない唯一のこと。
///
/// 描くのは `WKWebView` の中の mermaid 自身で、iPhone 用に書いた Swift の
/// レンダラではない ── 8 種類の図の 2 つ目の実装は 2 つ目の答えであり、
/// 面白いところ（マインドマップの配置、ガントの目盛り）こそ、2 つの実装が
/// 食い違う場所そのもの。
///
/// **ライブラリは、図のあるノートを開くまで読み込まない。** mermaid は 3.4MB
/// あり、ほとんどのノートに図は無い。デスクトップ版が遅れて読み込むのと
/// 同じ理由。
struct Drawing: View {
    let source: String
    @Environment(\.colorScheme) private var scheme
    @State private var tall: CGFloat = 120
    @State private var missing = false

    var body: some View {
        Group {
            if missing || Diagrams.tool == nil {
                // **何も出さずに、ソースを出す。** 描けなかった図も
                // 誰かが書いた図であり、隠せばこの端末でだけ
                // ノートの中身が失われる。
                ScrollView(.horizontal, showsIndicators: false) {
                    Text(source).font(.callout.monospaced())
                }
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.accentColor.opacity(0.10), in: RoundedRectangle(cornerRadius: 8))
            } else {
                Canvas(source: source, dark: scheme == .dark, tall: $tall)
                    .frame(height: tall)
                    .frame(maxWidth: .infinity)
            }
        }
        .onAppear { missing = Diagrams.tool == nil }
    }
}

/// 描画に使うファイルを展開する場所。
///
/// ライブラリとそれを使うページは、バンドルから Caches へ一度だけコピーする ──
/// `WKWebView` は `file:` のページを読み込み、その**隣**にあるスクリプトを
/// 取りに行かせてくれるが、文字列から組み立てたページのためにアプリの
/// バンドルを読むことはしない。画面に出る図ごとに 3.4MB を HTML へ
/// 埋め込むより、コピーのほうが安い。
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
            // アプリが展開済みのものより新しければコピーし直す ──
            // そうしないと、更新で入った新しい mermaid が一度も使われない。
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

    /// 11 色。**デスクトップ版が使うのと同じ 11 色** ── `gui/renderer.js` の
    /// `FAMILY` を見よ。表が 2 つあると、同じ円グラフが iPhone では違う色で
    /// 出ることになる ── 誰も報告しないが、誰もが気づく類の違い。
    ///
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
      // 2 フレーム待つ ── 1 つ目で SVG が入り、2 つ目で
      // ブラウザが配置を終える。早すぎると高さが 0 と返り、
      // 図が細い帯になって開く。
      requestAnimationFrame(() => requestAnimationFrame(() => {
        const h = document.getElementById('box').getBoundingClientRect().height;
        window.webkit.messageHandlers.tall.postMessage(Math.ceil(h) + 2);
      }));
    }
    // mermaid が測るために建てた仮の箱を片付ける。**`document.body` の
    // 直下だけ** ── 返ってくる SVG にも同じ id が付くので、id だけで
    // 消すと、いま画面に挿した図そのものが消える（デスクトップ版でそうなった）。
    window.sweep = (id) => {
      for (const at of [id, 'd' + id]) {
        const n = document.getElementById(at);
        if (n && n.parentElement === document.body) n.remove();
      }
      for (const x of document.body.querySelectorAll(':scope > [id^="dm"]')) x.remove();
    };
    window.draw = async (src, opts, bad) => {
      const box = document.getElementById('box');
      const id = 'm' + Math.random().toString(36).slice(2);
      try {
        mermaid.initialize(opts);
        const { svg } = await mermaid.render(id, src);
        box.innerHTML = svg;
      } catch (e) {
        // **書かれていたものと、描けなかった理由を出す。** 空白は
        // 「ノートが何かを失った」と言っている。
        const p = document.createElement('pre');
        p.id = 'bad';
        p.style.background = bad.bg;
        p.style.color = bad.fg;
        p.textContent = '図にできません: ' + String((e && e.message) || e) + '\\n\\n' + src;
        box.innerHTML = '';
        box.append(p);
      } finally {
        window.sweep(id);
      }
      tell();
    };
    </script></body></html>
    """
}

/// WebView そのもの。
struct Canvas: UIViewRepresentable {
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
        // ノートの背景を透かす ── 図の後ろに白いカードを置くと、
        // 暗いノートの中でそこだけが明るい四角になる。
        web.isOpaque = false
        web.backgroundColor = .clear
        web.scrollView.backgroundColor = .clear
        // ページが自分の高さを返し、スクロールは一覧側がする ──
        // スクロールの中のスクロールは、指の動きを飲み込む。
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
            print("[図] ページを開けません: \(e)")
        }

        func webView(_ web: WKWebView, didFinish: WKNavigation!) {
            ready = true
            self.web = web
            draw(web)
        }

        func draw(_ web: WKWebView) {
            self.web = web
            guard ready, let (src, dark) = want else { return }
            // 配置のたびに同じ図を描き直すと、読んでいる人が
            // スクロールしている最中に mermaid の配置エンジンが読み直される。
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

        /// Swift の値を JavaScript のリテラルにする。
        private func json(_ v: Any) -> String {
            guard let d = try? JSONSerialization.data(withJSONObject: [v], options: []),
                  let s = String(data: d, encoding: .utf8)
            else { return "\"\"" }
            return String(s.dropFirst().dropLast())
        }

        /// 描けなかった図に使う色。
        ///
        /// **raw 文字列のシャープは 2 つ。** 1 つだと `"#3b2f16"` の中の `"#` が
        /// リテラルを終わらせ、行の残りがコードとして読まれる。
        static func bad(_ dark: Bool) -> String {
            dark ? ##"{"bg":"#3b2f16","fg":"#bcac91"}"## : ##"{"bg":"#f7e2b6","fg":"#6b5a41"}"##
        }

        /// **デスクトップ版と同じ設定**（`gui/renderer.js` の `mermaidOpts`）に、
        /// CSS 変数の代わりに iPhone の色を入れたもの。デスクトップ版はテーマを
        /// ページから読むが、ここでは 2 つのテーマを書き出してある ── iPhone に
        /// あるテーマはちょうど 2 つだから。
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
                // 11 色はどれも淡いので、その上の文字は常に濃い色になる ──
                // デスクトップ版は色の明るさから計算していて、同じ答えに
                // 行き着く。
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
                // **書き損じの絵を、mermaid に描かせない。**
                //
                // 既定では、文字が通らないと mermaid は自分で赤い絵を描いて
                // `document.body` に置いていく ── こちらの `catch` は届かない。
                // 打つたびに描き直すので一文字ごとに1 つ積み上がり、積まれた
                // 絵が幅を持つので画面が潰れる。デスクトップ版で実際にそうなった（依頼 347）。
                "suppressErrorRendering": true,
                "theme": "base",
                "themeVariables": vars.merging(["darkMode": dark ? "true" : "false"]) { a, _ in a },
                "themeCSS": css,
                // **iPhone の幅は 402 ポイント。** デスクトップ版は図を 52em まで
                // 伸ばしてスクロールさせるが、ここでは全部収める必要があるので、
                // 間隔を詰め、箱を狭くしてある。
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
                // ノートは人が書いたもの。図のラベルの中の HTML を
                // 実行させはしない。
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
