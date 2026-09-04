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
            ScrollView(.horizontal, showsIndicators: false) {
                Text(b.text).font(.callout.monospaced())
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.accentColor.opacity(0.10), in: RoundedRectangle(cornerRadius: 8))
        case "image":
            picture(b)
        case "rule":
            Divider()
        default:
            inline(b)
        }
    }

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
