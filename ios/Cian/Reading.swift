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

    init(_ o: [String: Any]) {
        kind = o["kind"] as? String ?? "paragraph"
        text = o["text"] as? String ?? ""
        level = o["level"] as? Int ?? 0
        n = o["n"] as? Int ?? 0
        lang = o["lang"] as? String ?? ""
        alt = o["alt"] as? String ?? ""
        link = o["link"] as? String ?? ""
    }
}

/// A note, read rather than edited.
struct Reading: View {
    let blocks: [Block]
    /// The folder the note is in — a picture's link is relative to it.
    let base: URL

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
            inline(b.text)
                .font(heading(b.level))
                .padding(.top, b.level <= 2 ? 8 : 2)
        case "bullet":
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("•").foregroundStyle(.secondary)
                inline(b.text)
            }
        case "numbered":
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text("\(b.n).").foregroundStyle(.secondary).monospacedDigit()
                inline(b.text)
            }
        case "quote":
            HStack(spacing: 10) {
                // A bar in the margin rather than a wash behind the words:
                // the same thing the editor's gutter does on the Mac, and for
                // the same reason — quoted text is not more important, only
                // somebody else's.
                Rectangle().frame(width: 3).foregroundStyle(.tint.opacity(0.5))
                inline(b.text).foregroundStyle(.secondary)
            }
        case "code":
            ScrollView(.horizontal, showsIndicators: false) {
                Text(b.text).font(.callout.monospaced())
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.secondary.opacity(0.12), in: RoundedRectangle(cornerRadius: 8))
        case "image":
            picture(b)
        case "rule":
            Divider()
        default:
            inline(b.text)
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
    private func inline(_ s: String) -> Text {
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
