import SwiftUI
import UIKit

/// The place you type, and **where the cursor is**.
///
/// `TextEditor` does not say where the cursor is, so every writing tool had
/// to work on the last line of the note — you pressed 見出し and the heading
/// appeared at the bottom. This is the same box with the one thing it was
/// missing reported back.
///
/// **The composing text is not the text.** While an IME is putting a word
/// together (`markedTextRange`), the box holds characters that have not been
/// committed. Writing into it then throws the composition away and the
/// half-typed word jumps or vanishes — so nothing is pushed in either
/// direction until the composition ends.
struct Editor: UIViewRepresentable {
    @Binding var text: String
    /// The selection, in UTF-16 units — the units `NSString` counts in, and
    /// therefore the ones every edit below is written in.
    @Binding var pick: NSRange
    /// Whether it has the keyboard. Reported rather than commanded: the
    /// keyboard also goes away for reasons that have nothing to do with us.
    @Binding var editing: Bool

    func makeUIView(context: Context) -> UITextView {
        let v = UITextView()
        v.delegate = context.coordinator
        v.font = .monospacedSystemFont(ofSize: UIFont.preferredFont(forTextStyle: .body).pointSize,
                                       weight: .regular)
        v.adjustsFontForContentSizeCategory = true
        v.backgroundColor = .clear
        v.textContainerInset = UIEdgeInsets(top: 8, left: 4, bottom: 8, right: 4)
        v.alwaysBounceVertical = true
        // Nothing helpful: a notes app is where you type `- [ ]` and
        // `](https://`, and a capitaliser that decides otherwise is fighting
        // you at the one moment you know exactly what you meant.
        v.autocorrectionType = .no
        v.autocapitalizationType = .none
        v.smartQuotesType = .no
        v.smartDashesType = .no
        v.text = text
        return v
    }

    func updateUIView(_ v: UITextView, context: Context) {
        // Mid-composition: leave it alone entirely.
        if v.markedTextRange != nil { return }
        if v.text != text {
            v.text = text
            v.selectedRange = clamp(pick, in: v.text)
        } else if v.selectedRange != pick {
            v.selectedRange = clamp(pick, in: v.text)
        }
        if editing, !v.isFirstResponder {
            v.becomeFirstResponder()
        } else if !editing, v.isFirstResponder {
            v.resignFirstResponder()
        }
    }

    private func clamp(_ r: NSRange, in s: String) -> NSRange {
        let n = (s as NSString).length
        let at = min(max(0, r.location), n)
        return NSRange(location: at, length: min(r.length, n - at))
    }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    final class Coordinator: NSObject, UITextViewDelegate {
        private let owner: Editor
        init(_ owner: Editor) { self.owner = owner }

        func textViewDidChange(_ v: UITextView) {
            guard v.markedTextRange == nil else { return }
            owner.text = v.text
            owner.pick = v.selectedRange
        }

        func textViewDidChangeSelection(_ v: UITextView) {
            guard v.markedTextRange == nil else { return }
            if owner.pick != v.selectedRange { owner.pick = v.selectedRange }
        }

        func textViewDidBeginEditing(_ v: UITextView) { owner.editing = true }
        func textViewDidEndEditing(_ v: UITextView) { owner.editing = false }
    }
}

/// The writing tools, as edits on text and a selection.
///
/// Kept apart from the view so each one is a plain function of (text,
/// selection) → (text, selection): that is the whole of what a Markdown
/// button does, and it is the shape a test can hold.
enum Marks {
    /// The line the cursor is on, as a range over `text`.
    static func lineRange(_ text: String, _ pick: NSRange) -> NSRange {
        let s = text as NSString
        let at = min(max(0, pick.location), s.length)
        return s.lineRange(for: NSRange(location: at, length: 0))
    }

    /// Put `prefix` on the cursor's line, or take it off if it is already
    /// there. Toggling matters: the moment you press it by mistake, pressing
    /// it again is what you reach for.
    static func line(_ text: String, _ pick: NSRange, _ prefix: String) -> (String, NSRange) {
        let s = text as NSString
        let r = lineRange(text, pick)
        var row = s.substring(with: r)
        let end = row.hasSuffix("\n") ? "\n" : ""
        if !end.isEmpty { row.removeLast() }
        let out: String
        let shift: Int
        if row.hasPrefix(prefix) {
            out = String(row.dropFirst(prefix.count))
            shift = -(prefix as NSString).length
        } else {
            out = prefix + row
            shift = (prefix as NSString).length
        }
        let whole = s.replacingCharacters(in: r, with: out + end)
        return (whole, NSRange(location: max(r.location, pick.location + shift), length: 0))
    }

    /// One `#` deeper on the cursor's line, and back to none after three.
    static func deepen(_ text: String, _ pick: NSRange) -> (String, NSRange) {
        let s = text as NSString
        let r = lineRange(text, pick)
        var row = s.substring(with: r)
        let end = row.hasSuffix("\n") ? "\n" : ""
        if !end.isEmpty { row.removeLast() }
        let had = row.prefix(while: { $0 == "#" }).count
        var body = String(row.dropFirst(had))
        if body.hasPrefix(" ") { body.removeFirst() }
        let next = (had + 1) % 4
        let out = next == 0 ? body : String(repeating: "#", count: next) + " " + body
        let whole = s.replacingCharacters(in: r, with: out + end)
        let before = (row as NSString).length
        let after = (out as NSString).length
        return (whole, NSRange(location: max(r.location, pick.location + after - before), length: 0))
    }

    /// Wrap the selection, or open an empty pair with the cursor inside it.
    static func wrap(_ text: String, _ pick: NSRange, _ mark: String) -> (String, NSRange) {
        let s = text as NSString
        let n = (mark as NSString).length
        if pick.length > 0 {
            let inner = s.substring(with: pick)
            let whole = s.replacingCharacters(in: pick, with: mark + inner + mark)
            return (whole, NSRange(location: pick.location + n, length: pick.length))
        }
        let whole = s.replacingCharacters(in: pick, with: mark + mark)
        return (whole, NSRange(location: pick.location + n, length: 0))
    }

    /// Drop a block in below the cursor's line.
    ///
    /// `caret` is how far into what was inserted the cursor should land —
    /// inside the fence rather than after it, which is where you were going
    /// to type anyway.
    static func block(_ text: String, _ pick: NSRange, _ body: String, caret: Int? = nil) -> (String, NSRange) {
        let s = text as NSString
        let r = lineRange(text, pick)
        var at = r.location + r.length
        var insert = body
        // A note whose last line has no newline would otherwise get the
        // block welded onto the end of that line.
        if at > 0, s.substring(with: NSRange(location: at - 1, length: 1)) != "\n" {
            insert = "\n" + insert
        }
        if at > s.length { at = s.length }
        let whole = s.replacingCharacters(in: NSRange(location: at, length: 0), with: insert)
        let landing = caret ?? (insert as NSString).length
        return (whole, NSRange(location: at + landing, length: 0))
    }

    /// Put text in at the cursor, replacing whatever is selected.
    static func insert(_ text: String, _ pick: NSRange, _ body: String) -> (String, NSRange) {
        let s = text as NSString
        let whole = s.replacingCharacters(in: pick, with: body)
        return (whole, NSRange(location: pick.location + (body as NSString).length, length: 0))
    }
}
