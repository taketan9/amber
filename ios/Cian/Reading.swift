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

/// A note, read rather than edited.
struct Reading: View {
    let blocks: [Block]
    /// The folder the note is in — a picture's link is relative to it.
    let base: URL
    /// What to do when a task is pressed. Reading is not only reading: a
    /// shopping list is read *while* it is being crossed off, and going back
    /// to the editor to type an `x` between two brackets is not that.
    var tick: ((Block) -> Void)?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(blocks) { b in draw(b) }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding()
    }

    @ViewBuilder
    private func draw(_ b: Block) -> some View {
        switch b.kind {
        case "heading":
            VStack(alignment: .leading, spacing: 4) {
                inline(b)
                    .font(heading(b.level))
                if b.level <= 2 {
                    Rectangle()
                        .frame(width: 44, height: 2)
                        .foregroundStyle(.tint)
                }
            }
            .padding(.top, b.level <= 2 ? 8 : 2)
        case "check":
            Button {
                tick?(b)
            } label: {
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Image(systemName: b.done ? "checkmark.square.fill" : "square")
                        .foregroundStyle(b.done ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))
                        .imageScale(.large)
                    inline(b)
                        .strikethrough(b.done)
                        .foregroundStyle(b.done ? .secondary : .primary)
                    Spacer(minLength: 0)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(tick == nil)
        case "bullet":
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("•").foregroundStyle(.secondary)
                inline(b)
            }
        case "numbered":
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("\(b.n).").foregroundStyle(.secondary).monospacedDigit()
                inline(b)
            }
        case "quote":
            HStack(spacing: 10) {
                // A bar in the margin rather than a wash behind the words:
                // the same thing the editor's gutter does on the Mac, and for
                // the same reason — quoted text is not more important, only
                // somebody else's.
                Rectangle().frame(width: 3).foregroundStyle(.tint.opacity(0.5))
                inline(b).foregroundStyle(.secondary)
            }
        case "code":
            // **`mermaid` の枠は、枠ではなく図。** 窓が絵を出しているところで
            // 電話が `flowchart LR` と出していたので、同じノートが開く端末に
            // よって別のものに読めていた。
            if b.lang == "mermaid" {
                Drawing(source: b.text)
            } else {
                ScrollView(.horizontal, showsIndicators: false) {
                    Text(b.text).font(.callout.monospaced())
                }
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.accentColor.opacity(0.10), in: RoundedRectangle(cornerRadius: 8))
            }
        case "image":
            picture(b)
        case "table":
            table(b)
        case "alert":
            alertBox(b)
        case "rule":
            Divider()
        default:
            inline(b)
        }
    }

    /// A table.
    ///
    /// **Scrolls sideways rather than squeezing.** A phone is 402 points
    /// wide and a four-column table is not; wrapping every cell to two
    /// characters turns a table into a column of syllables. Sideways is a
    /// gesture people already have.
    ///
    /// Drawn with `Grid` so the columns line up down the whole table —
    /// a `VStack` of `HStack`s lines up nothing, which is the one thing a
    /// table is for.
    @ViewBuilder
    private func table(_ b: Block) -> some View {
        let wide = b.rows.reduce(b.head.count) { max($0, $1.count) }
        ScrollView(.horizontal, showsIndicators: false) {
            Grid(alignment: .topLeading, horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    ForEach(0..<wide, id: \.self) { c in
                        cell(at(b.head, c), align: at(b.align, c), head: true)
                    }
                }
                Divider().gridCellUnsizedAxes(.horizontal)
                ForEach(Array(b.rows.enumerated()), id: \.offset) { _, row in
                    GridRow {
                        ForEach(0..<wide, id: \.self) { c in
                            cell(at(row, c), align: at(b.align, c), head: false)
                        }
                    }
                    Divider().gridCellUnsizedAxes(.horizontal)
                }
            }
            .padding(.vertical, 2)
        }
        .overlay(
            RoundedRectangle(cornerRadius: 8).strokeBorder(Color.secondary.opacity(0.25))
        )
    }

    /// The `n`th of something, or empty — **a short row is not a crash.**
    /// Somebody's table has a row with one cell missing; drawing it with a
    /// blank there is what they meant.
    private func at<T>(_ xs: [T], _ n: Int) -> T? { n < xs.count ? xs[n] : nil }

    private func cell(_ text: String?, align: String?, head: Bool) -> some View {
        let how: Alignment = align == "center" ? .center : (align == "right" ? .trailing : .leading)
        return markdown(text ?? "")
            .font(head ? .subheadline.bold() : .subheadline)
            .frame(minWidth: 74, maxWidth: 210, alignment: how)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 11)
            .padding(.vertical, 7)
    }

    /// `> [!NOTE]` and its four siblings.
    ///
    /// **The kind is not the text.** Left as a quote, the phone drew the
    /// literal `[!TIP]` on a line of its own — notation showing through, in
    /// the one place a reader is being told something. It gets the colour and
    /// the symbol GitHub gives it, so the same note reads the same in both.
    @ViewBuilder
    private func alertBox(_ b: Block) -> some View {
        let look = Self.alerts[b.alert] ?? ("いいたいこと", "info.circle.fill", Color.accentColor)
        HStack(alignment: .top, spacing: 10) {
            Rectangle().frame(width: 3).foregroundStyle(look.2)
            VStack(alignment: .leading, spacing: 7) {
                Label(look.0, systemImage: look.1)
                    .font(.footnote.bold())
                    .foregroundStyle(look.2)
                ForEach(Array(b.body.enumerated()), id: \.offset) { _, p in
                    markdown(p).font(.subheadline)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.vertical, 9)
            .padding(.trailing, 11)
        }
        .background(look.2.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
    }

    /// GitHub の五つ。**名前は日本語で出す** ── `NOTE` と `IMPORTANT` の
    /// 違いを英語で読ませるより、「おぼえておく」「大事」のほうが早い。
    private static let alerts: [String: (String, String, Color)] = [
        "note": ("おぼえておく", "info.circle.fill", .blue),
        "tip": ("こつ", "lightbulb.fill", .green),
        "important": ("大事", "exclamationmark.circle.fill", .purple),
        "warning": ("注意", "exclamationmark.triangle.fill", .orange),
        "caution": ("あぶない", "hand.raised.fill", .red),
    ]

    /// The picture beside the note.
    ///
    /// Loaded from the folder rather than by URL: these live next to the note,
    /// and `AsyncImage` over a `file:` URL would be a round trip to say what
    /// the filesystem already knows. A missing one says so — a note that
    /// silently drops a picture looks the same as a note that never had one.
    @ViewBuilder
    private func picture(_ b: Block) -> some View {
        let at = base.appendingPathComponent(b.link)
        if let data = try? Data(contentsOf: at), let img = UIImage(data: data) {
            VStack(alignment: .leading, spacing: 4) {
                Image(uiImage: img)
                    .resizable()
                    .scaledToFit()
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                if !b.alt.isEmpty {
                    Text(b.alt).font(.caption).foregroundStyle(.secondary)
                }
            }
        } else {
            Label(b.alt.isEmpty ? b.link : b.alt, systemImage: "photo.badge.exclamationmark")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    /// Bold, italic, code spans and links — Markdown's inline half, which
    /// `AttributedString` already knows. Anything it cannot parse is shown as
    /// what was typed, which is the right answer for a notes app.
    ///
    /// Colour is added on top, run by run: **which pieces are coloured was
    /// decided in `cian-core`**, and all this does is paint them.
    private func inline(_ b: Block) -> Text {
        guard !b.runs.isEmpty else { return markdown(b.text) }
        return b.runs.reduce(Text("")) { out, run in
            let piece = markdown(run.0)
            guard let hex = run.1, let c = Color(hex: hex) else { return out + piece }
            return out + piece.foregroundColor(c)
        }
    }

    private func markdown(_ s: String) -> Text {
        if let a = try? AttributedString(
            markdown: s,
            options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        ) {
            return Text(a)
        }
        return Text(s)
    }

    private func heading(_ level: Int) -> Font {
        switch level {
        case 1: return .title.bold()
        case 2: return .title2.bold()
        case 3: return .title3.bold()
        default: return .headline
        }
    }
}
