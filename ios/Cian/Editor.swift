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
    /// The box itself, for the things only UIKit can do: undo, and moving
    /// the cursor a line at a time.
    let pen: Pen
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
        pen.view = v
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
            owner.pen.refresh()
        }

        func textViewDidChangeSelection(_ v: UITextView) {
            guard v.markedTextRange == nil else { return }
            if owner.pick != v.selectedRange { owner.pick = v.selectedRange }
        }

        func textViewDidBeginEditing(_ v: UITextView) { owner.editing = true }
        func textViewDidEndEditing(_ v: UITextView) { owner.editing = false }
    }
}

/// One replacement: what to take out, what to put in, and where the cursor
/// lands afterwards.
///
/// **An edit rather than a whole new text.** A tool that hands back the
/// entire note has thrown away what changed, and UIKit's undo works on
/// changes — so 見出し could not be undone, only the typing around it.
struct Edit {
    let at: NSRange
    let with: String
    let then: NSRange
}

/// The writing tools, as edits on text and a selection.
///
/// Kept apart from the view so each one is a plain function of (text,
/// selection) → edit: that is the whole of what a Markdown button does, and
/// it is the shape a test can hold.
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
    static func line(_ text: String, _ pick: NSRange, _ prefix: String) -> Edit {
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
        return Edit(at: r, with: out + end,
                    then: NSRange(location: max(r.location, pick.location + shift), length: 0))
    }

    /// One `#` deeper on the cursor's line, and back to none after three.
    static func deepen(_ text: String, _ pick: NSRange) -> Edit {
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
        let before = (row as NSString).length
        let after = (out as NSString).length
        return Edit(at: r, with: out + end,
                    then: NSRange(location: max(r.location, pick.location + after - before), length: 0))
    }

    /// Wrap the selection, or open an empty pair with the cursor inside it.
    static func wrap(_ text: String, _ pick: NSRange, _ mark: String) -> Edit {
        let s = text as NSString
        let n = (mark as NSString).length
        if pick.length > 0 {
            let inner = s.substring(with: pick)
            return Edit(at: pick, with: mark + inner + mark,
                        then: NSRange(location: pick.location + n, length: pick.length))
        }
        return Edit(at: pick, with: mark + mark,
                    then: NSRange(location: pick.location + n, length: 0))
    }

    /// Drop a block in below the cursor's line.
    ///
    /// `caret` is how far into what was inserted the cursor should land —
    /// inside the fence rather than after it, which is where you were going
    /// to type anyway.
    static func block(_ text: String, _ pick: NSRange, _ body: String, caret: Int? = nil) -> Edit {
        let s = text as NSString
        let r = lineRange(text, pick)
        var at = min(r.location + r.length, s.length)
        var insert = body
        // A note whose last line has no newline would otherwise get the
        // block welded onto the end of that line.
        if at > 0, s.substring(with: NSRange(location: at - 1, length: 1)) != "\n" {
            insert = "\n" + insert
        }
        if at > s.length { at = s.length }
        let landing = caret ?? (insert as NSString).length
        return Edit(at: NSRange(location: at, length: 0), with: insert,
                    then: NSRange(location: at + landing, length: 0))
    }

    /// Put text in at the cursor, replacing whatever is selected.
    static func insert(_ text: String, _ pick: NSRange, _ body: String) -> Edit {
        Edit(at: pick, with: body,
             then: NSRange(location: pick.location + (body as NSString).length, length: 0))
    }
}

/// The box, held from outside it.
///
/// Undo belongs to UIKit — it is the same undo three-finger-swipe and the
/// shake gesture use, and reimplementing it in Swift would be a second,
/// worse one that disagrees with the phone. So the tools reach the text
/// through the view (`replace(_:withText:)` registers an undo step) rather
/// than by swapping the whole string, and this is how they reach it.
@MainActor
final class Pen: ObservableObject {
    weak var view: UITextView?
    @Published var canUndo = false
    @Published var canRedo = false

    func refresh() {
        canUndo = view?.undoManager?.canUndo ?? false
        canRedo = view?.undoManager?.canRedo ?? false
    }

    func undo() { view?.undoManager?.undo(); refresh() }
    func redo() { view?.undoManager?.redo(); refresh() }

    /// Make one edit, in a way the phone's own undo understands.
    ///
    /// Falls back to a plain string swap when there is no live view — the
    /// reading half has none, and an edit that silently did nothing there
    /// would be worse than one that cannot be undone.
    func apply(_ e: Edit, to text: inout String, pick: inout NSRange) {
        if let v = view, let r = range(v, e.at) {
            v.replace(r, withText: e.with)
            v.selectedRange = clamp(e.then, in: v.text)
            text = v.text
            pick = v.selectedRange
            refresh()
            return
        }
        let s = text as NSString
        text = s.replacingCharacters(in: e.at, with: e.with)
        pick = clamp(e.then, in: text)
    }

    /// Move the cursor one step. **The arrow keys a phone keyboard does not
    /// have** — and the reason writing anything longer than a line on a
    /// phone is miserable. Up and down are `.layout` moves, so they follow
    /// the line as it is *drawn*, wrapping included.
    func step(_ way: UITextLayoutDirection) {
        guard let v = view, let from = v.selectedTextRange?.start else { return }
        guard let to = v.position(from: from, in: way, offset: 1) else { return }
        v.selectedTextRange = v.textRange(from: to, to: to)
    }

    private func range(_ v: UITextView, _ r: NSRange) -> UITextRange? {
        guard let a = v.position(from: v.beginningOfDocument, offset: r.location),
              let b = v.position(from: a, offset: r.length) else { return nil }
        return v.textRange(from: a, to: b)
    }

    private func clamp(_ r: NSRange, in s: String) -> NSRange {
        let n = (s as NSString).length
        let at = min(max(0, r.location), n)
        return NSRange(location: at, length: min(r.length, n - at))
    }
}
