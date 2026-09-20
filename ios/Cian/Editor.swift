import SwiftUI
import UIKit

/// 入力する場所と、**カーソルの位置**。
///
/// `TextEditor` はカーソルの位置を教えてくれないので、編集の道具はどれも
/// to work on the last line of the note — you pressed 見出し and the heading
/// 末尾に挿入されていた。ここは同じ入力欄に、足りなかった 1 つを
/// 返させるようにしたもの。
///
/// **変換中の文字は、本文ではない。** IME が語を組み立てているあいだ
/// （`markedTextRange`）、入力欄には確定していない文字が入っている。
/// そこへ書き込むと変換が破棄され、打ちかけの語が飛んだり消えたり
/// する ── だから変換が終わるまで、どちら向きにも何も渡さない。
///
struct Editor: UIViewRepresentable {
    /// 入力欄そのもの。UIKit にしかできないこと ── 取り消しと、
    /// カーソルを 1 行ずつ動かすこと ── のため。
    let pen: Pen
    @Binding var text: String
    /// 選択範囲。単位は UTF-16 ── `NSString` が数える単位であり、
    /// 下のすべての編集もその単位で書いてある。
    @Binding var pick: NSRange
    /// キーボードを持っているか。命令ではなく報告 ── キーボードは
    /// こちらと関係のない理由でも引っ込む。
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
        // おせっかいは全部切る ── ノートアプリは `- [ ]` や
        // `](https://` を打つ場所で、勝手に大文字にする機能は、
        // いちばん意図がはっきりしている瞬間に邪魔をする。
        v.autocorrectionType = .no
        v.autocapitalizationType = .none
        v.smartQuotesType = .no
        v.smartDashesType = .no
        v.text = text
        pen.view = v
        return v
    }

    func updateUIView(_ v: UITextView, context: Context) {
        // 変換中は、一切触らない。
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

/// 置換 1 回ぶん ── 何を取り除き、何を入れ、カーソルをどこに
/// 置くか。
///
/// **本文を丸ごと差し替えるのではなく、編集として渡す。** ノート全体を
/// 返す道具は「何が変わったか」を捨てていて、UIKit の取り消しは
/// changes — so 見出し could not be undone, only the typing around it.
struct Edit {
    let at: NSRange
    let with: String
    let then: NSRange
}

/// 編集の道具を、(本文, 選択範囲) → 編集 という形で置く。
///
/// View から切り離してあるので、どれも (本文, 選択範囲) → 編集 の素の
/// 関数になる。Markdown のボタンがやることはそれで全部だし、
/// テストが掴める形でもある。
enum Marks {
    /// カーソルのある行を、`text` 上の範囲として返す。
    static func lineRange(_ text: String, _ pick: NSRange) -> NSRange {
        let s = text as NSString
        let at = min(max(0, pick.location), s.length)
        return s.lineRange(for: NSRange(location: at, length: 0))
    }

    /// カーソルのある行に `prefix` を付ける。既に付いていれば外す。
    /// 切り替えにしてあるのが大事 ── 押し間違えた瞬間に手が伸びるのは、
    /// もう一度押すことだから。
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

    /// カーソルのある行の `#` を 1 つ深くする。3 つの次は無しに戻る。
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

    /// 選択範囲を囲む。選択が無ければ空の対を置き、カーソルをその中へ。
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

    /// カーソルのある行の下にブロックを挿入する。
    ///
    /// `caret` は、挿入したものの中のどこにカーソルを置くか ── フェンスの
    /// 後ろではなく中。どのみちそこから打ち始めるのだから。
    ///
    static func block(_ text: String, _ pick: NSRange, _ body: String, caret: Int? = nil) -> Edit {
        let s = text as NSString
        let r = lineRange(text, pick)
        var at = min(r.location + r.length, s.length)
        var insert = body
        // 最終行が改行で終わっていないノートでは、そうしないと
        // ブロックがその行の末尾にくっついてしまう。
        if at > 0, s.substring(with: NSRange(location: at - 1, length: 1)) != "\n" {
            insert = "\n" + insert
        }
        if at > s.length { at = s.length }
        let landing = caret ?? (insert as NSString).length
        return Edit(at: NSRange(location: at, length: 0), with: insert,
                    then: NSRange(location: at + landing, length: 0))
    }

    /// カーソル位置に文字列を入れる。選択があれば置き換える。
    static func insert(_ text: String, _ pick: NSRange, _ body: String) -> Edit {
        Edit(at: pick, with: body,
             then: NSRange(location: pick.location + (body as NSString).length, length: 0))
    }
}

/// 入力欄を、外から保持する。
///
/// 取り消しは UIKit のもの ── 三本指スワイプやシェイクが使うのと同じ
/// 取り消しで、Swift で作り直せば 2 つ目の、しかも端末の挙動と食い違う
/// 劣ったものになる。だから道具は本文へ、文字列を丸ごと差し替える
/// のではなく View 経由で届く（`replace(_:withText:)` が取り消しの段を
/// 登録する）。その届け方がここ。
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

    /// 編集を 1 回、端末の取り消しが理解できる形で行う。
    ///
    /// **`replace(_:withText:)` ではなく text storage 経由で。** あちらは
    /// 入力と同じ入口を通り、その入口はスマート置換を掛ける ── 表の
    /// `| --- |` が `| — |`（emダッシュ）になり、表が表でなくなった。
    /// storage へ書けばそこを通らない。取り消しの段は UIKit ではなく
    /// ここで登録する。
    ///
    /// 生きた View が無いときは素の文字列差し替えに落とす ── 表示側には
    /// View が無く、そこで黙って何も起きない編集は、取り消せない編集より
    /// 悪い。
    func apply(_ e: Edit, to text: inout String, pick: inout NSRange) {
        guard let v = view else {
            let s = text as NSString
            text = s.replacingCharacters(in: e.at, with: e.with)
            pick = clamp(e.then, in: text)
            return
        }
        let was = v.text ?? ""
        let wasPick = v.selectedRange
        v.textStorage.replaceCharacters(in: e.at, with: e.with)
        v.selectedRange = clamp(e.then, in: v.text)
        remember(v, was, wasPick)
        text = v.text
        pick = v.selectedRange
        refresh()
    }

    /// 取り消し 1 段ぶん ── ノート全体を元に戻す。
    ///
    /// 粗いのは意図的 ── 道具を 1 回押したことは 1 つの出来事であって、
    /// 表の半分だけ取り消したい人はいない。取り消しの中から逆向きを
    /// 登録しておくと、やり直しができるようになる。
    private func remember(_ v: UITextView, _ was: String, _ pick: NSRange) {
        v.undoManager?.registerUndo(withTarget: v) { [weak self] tv in
            let now = tv.text ?? ""
            let nowPick = tv.selectedRange
            tv.text = was
            tv.selectedRange = self?.clamp(pick, in: was) ?? NSRange(location: 0, length: 0)
            self?.remember(tv, now, nowPick)
            tv.delegate?.textViewDidChange?(tv)
        }
    }

    /// カーソルを 1 つ動かす。**iPhone のキーボードに無い矢印キー** ──
    /// そして iPhone で 1 行より長いものを書くのが苦痛な理由。上下は
    /// `.layout` の移動なので、折り返しも含めて*描かれたとおりの*行に
    /// 従う。
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
