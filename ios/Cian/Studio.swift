import SwiftUI
import JavaScriptCore

/// 図を、見ながら直す。
///
/// **窓と同じ解析器を使う。** 八種類の形をもう一組 Swift で書けば、答えが
/// もう一組できて、食い違うのはマインドマップの深さや予定表の日付という、
/// いちばん確かめにくいところになる。`gui/renderer.js` の読み書きを切り出して
/// `JavaScriptCore` で走らせる ── `scripts/diagram-test.js` が同じ切り出し方で
/// 往復を見ているので、電話が使うのは**試験の通ったものそのもの**。
///
/// WebView ではなく `JSContext` なのは、これがただの関数だから ── 画面も
/// DOM も要らない。図を描くほうは、いままでどおり `Drawing`。
enum Mmd {
    /// 図の字 → 表。読めなければ `nil`（そのときは字で直す面になる）。
    static func parse(_ src: String) -> [String: Any]? {
        guard let ctx = shared else { return nil }
        let out = ctx.objectForKeyedSubscript("mmdParse")?.call(withArguments: [src])
        guard let v = out, !v.isNull, !v.isUndefined else { return nil }
        return v.toDictionary() as? [String: Any]
    }

    /// 表 → 図の字。
    static func build(_ model: [String: Any]) -> String? {
        guard let ctx = shared else { return nil }
        let out = ctx.objectForKeyedSubscript("mmdBuild")?.call(withArguments: [model])
        return out?.toString()
    }

    /// 種類ごとの、表の形（`DIAGRAM_FORM`）。窓と同じ欄・同じ名前。
    static func shape(_ kind: String) -> [String: Any]? {
        guard let ctx = shared else { return nil }
        return ctx.objectForKeyedSubscript("amberForm")?
            .objectForKeyedSubscript(kind)?.toDictionary() as? [String: Any]
    }

    private static let shared: JSContext? = {
        guard let at = Bundle.main.url(forResource: "diagram", withExtension: "js"),
              let src = try? String(contentsOf: at, encoding: .utf8),
              let ctx = JSContext()
        else { return nil }
        // 切り出したところは `say()` を呼ばない（呼ぶのは工房の側）が、
        // 万一足したときに黙って落ちないよう受け皿だけ置いておく。
        ctx.exceptionHandler = { _, e in print("[図] \(e?.toString() ?? "?")") }
        ctx.evaluateScript(src)
        // **`const` はグローバルの持ちものにならない。** `function` の宣言は
        // グローバルに載るので `mmdParse` は引けるが、`const DIAGRAM_FORM` は
        // 別の棚（グローバル語彙環境）に居て `objectForKeyedSubscript` から
        // 見えない ── 表の形が空になり、「形は選べるのに名前を書く欄が無い」
        // 画面ができた。あとから走らせる一行なら同じ棚を見られるので、
        // そこで載せ替える。
        ctx.evaluateScript("globalThis.amberForm = DIAGRAM_FORM;")
        return ctx
    }()
}

/// 図の工房（電話）。
///
/// 窓は左に表・右に図の二段組みだが、402pt にそれは入らない ── **上に図、
/// 下に表**。直すと上が描き直るので、「見ながら直す」は変わらない。
struct Studio: View {
    let source: String
    let onDone: (String) -> Void
    @Environment(\.dismiss) private var dismiss

    @State private var model: [String: Any]?
    @State private var rows: [[String: Any]] = []
    @State private var edges: [[String: Any]] = []
    @State private var title = ""
    @State private var dir = "LR"
    @State private var raw = ""
    @State private var byText = false
    @State private var live = ""

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // **図が先。** 直した結果が目に入らないところに置くと、
                // 「見ながら直す」でなくなる。
                Drawing(source: live.isEmpty ? source : live)
                    .padding(.horizontal, 12)
                    .padding(.top, 6)
                Divider().padding(.top, 8)
                form
            }
            .navigationTitle(byText || model == nil ? "図を直す（字）" : "図を直す")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("やめる") { dismiss() }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button("この形にする") { onDone(text()); dismiss() }.bold()
                }
                if model != nil {
                    ToolbarItem(placement: .bottomBar) {
                        Button(byText ? "表で直す" : "字で直す") { swap() }
                    }
                }
            }
        }
        .onAppear(perform: load)
    }

    @ViewBuilder
    private var form: some View {
        if byText || model == nil {
            VStack(alignment: .leading, spacing: 8) {
                if model == nil {
                    Text("この図は表にできない形（手で書いたか、ambƏr の知らない書き方）です。上の絵を見ながら、ここで直してください。")
                        .font(.footnote).foregroundStyle(.secondary)
                        .padding(.horizontal, 16).padding(.top, 10)
                }
                TextEditor(text: $raw)
                    .font(.callout.monospaced())
                    .onChange(of: raw) { _, _ in live = raw }
            }
        } else {
            List {
                if let t = shape()?["title"] as? String {
                    Section(t) {
                        TextField(t, text: $title).onChange(of: title) { _, _ in redraw() }
                    }
                }
                if kind == "flow" {
                    Section("向き") {
                        Picker("向き", selection: $dir) {
                            Text("左から右").tag("LR")
                            Text("上から下").tag("TD")
                            Text("右から左").tag("RL")
                            Text("下から上").tag("BT")
                        }
                        .pickerStyle(.segmented)
                        .onChange(of: dir) { _, _ in redraw() }
                    }
                }
                Section(kind == "flow" ? "箱" : "中身") {
                    ForEach(rows.indices, id: \.self) { n in row(n) }
                        .onDelete { at in rows.remove(atOffsets: at); settle(); redraw() }
                        .onMove { from, to in rows.move(fromOffsets: from, toOffset: to); settle(); redraw() }
                    Button {
                        rows.append(blank())
                        redraw()
                    } label: {
                        Label(shape()?["add"] as? String ?? "足す", systemImage: "plus.circle")
                    }
                }
                if kind == "flow" {
                    Section("線") {
                        ForEach(edges.indices, id: \.self) { n in edge(n) }
                            .onDelete { at in edges.remove(atOffsets: at); redraw() }
                        Button {
                            guard rows.count >= 2 else { return }
                            edges.append(["from": rows[0]["id"] ?? "A",
                                          "to": rows[1]["id"] ?? "B", "b": ""])
                            redraw()
                        } label: { Label("線を足す", systemImage: "plus.circle") }
                    }
                }
                if deep {
                    Section {
                        Text("「→」で一段深く、「←」で一段浅く。枝の下に枝を、そのまた下にも書けます。")
                            .font(.footnote).foregroundStyle(.secondary)
                    }
                }
            }
            .listStyle(.insetGrouped)
            .environment(\.editMode, .constant(.active))
        }
    }

    // ── 一行 ──────────────────────────────────────────────

    @ViewBuilder
    private func row(_ n: Int) -> some View {
        let cols = (shape()?["cols"] as? [[String: Any]]) ?? []
        HStack(spacing: 8) {
            if deep {
                // 深さは字下げそのもので見せる ── 「2」と書くより、
                // ずれている形のほうが枝に見える。
                let at = (rows[n]["at"] as? Int) ?? 0
                if at > 0 {
                    Text(String(repeating: "  ", count: at) + "└")
                        .font(.caption2).foregroundStyle(.tertiary)
                }
            }
            ForEach(cols.indices, id: \.self) { c in cell(n, cols[c]) }
            if kind == "flow" {
                Picker("", selection: shapeOf(n)) {
                    Text("四角").tag("box"); Text("丸み").tag("round"); Text("ひし形").tag("diamond")
                }
                .labelsHidden().pickerStyle(.menu)
                // **箱の色も、電話から。** 窓は右押しと工房の両方から
                // 変えられる ── 色を付けられるのが片方だけだと、電話で
                // 直したノートから色が消えたように見える（消えはしないが、
                // 「電話では変えられない」に気づけない）。
                Menu {
                    Button("色なし") { paint(n, nil) }
                    ForEach(Colouring.palette, id: \.0) { hex, name in
                        Button { paint(n, hex) } label: {
                            // **`systemImage:` では十一個とも同じ色になる** ──
                            // 献立は記号を accent で塗る（`Colouring.dot` の註）。
                            Label { Text(name) } icon: { Image(uiImage: Colouring.dot(hex)) }
                        }
                    }
                } label: {
                    Image(systemName: (rows[n]["color"] as? String) == nil
                          ? "paintpalette" : "paintpalette.fill")
                        .foregroundStyle(colorOf(n).map { AnyShapeStyle($0) } ?? AnyShapeStyle(.tint))
                }
                .accessibilityLabel("箱の色")
            }
            if deep {
                Button { shift(n, -1) } label: { Image(systemName: "arrow.left") }
                    .buttonStyle(.borderless).disabled(((rows[n]["at"] as? Int) ?? 0) == 0)
                Button { shift(n, 1) } label: { Image(systemName: "arrow.right") }
                    .buttonStyle(.borderless).disabled(((rows[n]["at"] as? Int) ?? 0) >= roof(n))
            }
        }
    }

    @ViewBuilder
    private func cell(_ n: Int, _ col: [String: Any]) -> some View {
        let k = col["k"] as? String ?? "a"
        if col["check"] as? Bool == true {
            Toggle("点線", isOn: Binding(
                get: { (rows[n][k] as? Bool) ?? false },
                set: { rows[n][k] = $0; redraw() }))
                .labelsHidden()
        } else if col["slide"] as? Bool == true {
            let v = Binding<Double>(
                get: { Double(String(describing: rows[n][k] ?? "0.5")) ?? 0.5 },
                set: { rows[n][k] = String(format: "%.2f", $0); redraw() })
            VStack(spacing: 1) {
                Text(col["label"] as? String ?? "").font(.caption2).foregroundStyle(.secondary)
                Slider(value: v, in: 0...1, step: 0.05)
            }
        } else {
            TextField(col["ph"] as? String ?? (col["label"] as? String ?? ""), text: Binding(
                get: { String(describing: rows[n][k] ?? "") },
                set: { rows[n][k] = $0; redraw() }))
                .textInputAutocapitalization(.never)
        }
    }

    /// 線は**二段**にする。402pt に「ここから・言葉・ここへ」を横に三つ
    /// 並べると、箱の名前が二文字ずつに折れて読めない（そうなった）。
    @ViewBuilder
    private func edge(_ n: Int) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                Picker("", selection: pick(n, "from")) { nodes }
                    .labelsHidden().pickerStyle(.menu)
                Image(systemName: "arrow.right").font(.caption).foregroundStyle(.secondary)
                Picker("", selection: pick(n, "to")) { nodes }
                    .labelsHidden().pickerStyle(.menu)
                Spacer(minLength: 0)
            }
            TextField("線の言葉（なくてもいい）", text: Binding(
                get: { String(describing: edges[n]["b"] ?? "") },
                set: { edges[n]["b"] = $0; redraw() }))
                .font(.callout).textInputAutocapitalization(.never)
        }
    }

    /// 線の行き先は、箱の名前から選ぶ ── `A`、`B` のような合言葉を人に
    /// 打たせない（打たせると、消した箱を指したままの線が残る）。
    private var nodes: some View {
        ForEach(rows.indices, id: \.self) { i in
            let id = String(describing: rows[i]["id"] ?? "")
            Text(String(describing: rows[i]["a"] ?? "").isEmpty
                 ? id : String(describing: rows[i]["a"] ?? "")).tag(id)
        }
    }

    // ── 中身 ──────────────────────────────────────────────

    private var kind: String { (model?["kind"] as? String) ?? "" }
    private var deep: Bool { shape()?["deep"] as? Bool == true }
    private func shape() -> [String: Any]? { Mmd.shape(kind) }

    private func pick(_ n: Int, _ k: String) -> Binding<String> {
        Binding(get: { String(describing: edges[n][k] ?? "") },
                set: { edges[n][k] = $0; redraw() })
    }
    /// 箱の色を決める。`nil` で外す ── **キーごと外す**（空の字を入れると、
    /// 書き戻しで `style` の行が色なしで出る）。
    private func paint(_ n: Int, _ hex: String?) {
        if let hex { rows[n]["color"] = hex } else { rows[n].removeValue(forKey: "color") }
        redraw()
    }

    private func colorOf(_ n: Int) -> Color? {
        (rows[n]["color"] as? String).flatMap { Color(hex: $0) }
    }

    private func shapeOf(_ n: Int) -> Binding<String> {
        Binding(get: { String(describing: rows[n]["shape"] ?? "box") },
                set: { rows[n]["shape"] = $0; redraw() })
    }

    private func blank() -> [String: Any] {
        var out: [String: Any] = [:]
        for c in (shape()?["cols"] as? [[String: Any]]) ?? [] {
            out[c["k"] as? String ?? "a"] = (c["check"] as? Bool == true) ? false : ""
        }
        if kind == "flow" {
            out["id"] = freshId()
            out["shape"] = "box"
            if let prev = rows.last?["id"] {
                edges.append(["from": prev, "to": out["id"] ?? "", "b": ""])
            }
        }
        // 足した枝は、直前の枝と同じ深さに ── 一段目に戻すと、枝の下に
        // 続きを書いている途中で毎回まん中まで戻される。
        if deep { out["at"] = (rows.last?["at"] as? Int) ?? 0 }
        return out
    }

    private func freshId() -> String {
        let used = Set(rows.compactMap { $0["id"] as? String })
        for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ" where !used.contains(String(c)) {
            return String(c)
        }
        var n = 1
        while used.contains("N\(n)") { n += 1 }
        return "N\(n)"
    }

    /// **親のいない孫を作らせない。** 一段目の次にいきなり三段目を置くと
    /// mermaid はその枝を捨て、「足したのに図に出ない」という分かりにくい
    /// 壊れ方になる。
    private func roof(_ n: Int) -> Int { n == 0 ? 0 : ((rows[n - 1]["at"] as? Int) ?? 0) + 1 }

    private func shift(_ n: Int, _ by: Int) {
        let now = (rows[n]["at"] as? Int) ?? 0
        rows[n]["at"] = min(max(now + by, 0), roof(n))
        settle()
        redraw()
    }

    private func settle() {
        var top = 0
        for i in rows.indices {
            rows[i]["at"] = min(max((rows[i]["at"] as? Int) ?? 0, 0), top)
            top = ((rows[i]["at"] as? Int) ?? 0) + 1
        }
    }

    private func load() {
        let body = fence(source)
        raw = body
        live = body
        guard let m = Mmd.parse(body) else { model = nil; byText = true; return }
        model = m
        rows = (m["rows"] as? [[String: Any]]) ?? []
        edges = (m["edges"] as? [[String: Any]]) ?? []
        title = (m["title"] as? String) ?? ""
        dir = (m["dir"] as? String) ?? "LR"
    }

    private func swap() {
        if byText {
            guard let m = Mmd.parse(raw) else { return }
            model = m
            rows = (m["rows"] as? [[String: Any]]) ?? []
            edges = (m["edges"] as? [[String: Any]]) ?? []
            title = (m["title"] as? String) ?? ""
            dir = (m["dir"] as? String) ?? "LR"
            byText = false
        } else {
            raw = text()
            byText = true
        }
        live = byText ? raw : text()
    }

    private func redraw() { live = text() }

    /// いまの表（か字）から、図の字を。
    private func text() -> String {
        if byText || model == nil { return raw }
        var m = model ?? [:]
        m["rows"] = rows
        m["edges"] = edges
        m["title"] = title
        m["dir"] = dir
        return Mmd.build(m) ?? raw
    }

    /// ` ```mermaid ` の中身を取り出す。
    private func fence(_ md: String) -> String {
        let t = md.trimmingCharacters(in: .whitespacesAndNewlines)
        guard t.hasPrefix("```") else { return t }
        var lines = t.components(separatedBy: "\n")
        lines.removeFirst()
        if lines.last?.trimmingCharacters(in: .whitespaces).hasPrefix("```") == true {
            lines.removeLast()
        }
        return lines.joined(separator: "\n")
    }
}
