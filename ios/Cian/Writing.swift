import SwiftUI

/// One note on screen, reading or writing.
///
/// **Its text belongs to the desk, not to this view.** A `TabView` throws its
/// pages away as you swipe, and a page that owned the text would take your
/// unsaved paragraph with it. Everything that survives a swipe is in the
/// binding; everything in `@State` here is about this moment on screen.
///
/// The toolbar, the sheets and the saving all live a level up, on the desk —
/// see `DeskView`. A page that builds its own toolbar has it rebuilt every
/// time the page changes, and SwiftUI shows that as a 「⋯」 flickering in and
/// out.
struct NoteView: View {
    @Binding var tab: Desk.Tab
    let store: NotesStore
    @ObservedObject var pen: Pen
    @Binding var writing: Bool
    /// Whether the seldom-used half of the writing bar is unfolded.
    @State private var more = false
    @State private var trouble: String?
    @State private var making = false

    var body: some View {
        Group {
            if tab.reading {
                ScrollView { Reading(blocks: tab.blocks, base: folder, tick: tick) }
            } else {
                VStack(spacing: 0) {
                    Editor(pen: pen, text: $tab.text, pick: $tab.pick, editing: $writing)
                    // Only while the keyboard is up, which is the only time
                    // it is *above the keyboard* rather than sitting at the
                    // bottom of a page nobody is typing into.
                    if writing { marks }
                }
            }
        }
        .sheet(isPresented: $making) {
            Tabling { body in put(Marks.block(tab.text, tab.pick, body, caret: 2)) }
        }
        .alert(
            "できません",
            isPresented: Binding(get: { trouble != nil }, set: { if !$0 { trouble = nil } })
        ) {
            Button("閉じる") {}
        } message: {
            Text(trouble ?? "")
        }
    }

    // MARK: the writing bar

    /// The Markdown a phone keyboard makes you hunt for, and the four keys it
    /// does not have at all.
    ///
    /// Two rows, the second folded away. The first row is what a note is
    /// actually made of; the rest are real Markdown and really occasional,
    /// and a bar of fourteen icons costs you the five you use every time.
    private var marks: some View {
        VStack(spacing: 0) {
            if more {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 6) {
                        mark("斜体", "italic") { wrap("*") }
                        mark("取り消し線", "strikethrough") { wrap("~~") }
                        mark("コード", "chevron.left.forwardslash.chevron.right") { wrap("`") }
                        Divider().frame(height: 20)
                        mark("リンク", "link") { block("[](https://)\n", caret: 1) }
                        mark("表", "tablecells") { making = true }
                        mark("コード枠", "curlybraces") { block("```\n\n```\n", caret: 4) }
                        mark("水平線", "minus") { block("---\n") }
                        Divider().frame(height: 20)
                        mark("引用", "text.quote") { line("> ") }
                        mark("番号つき", "list.number") { line("1. ") }
                        Divider().frame(height: 20)
                        Menu {
                            ForEach(Colouring.palette, id: \.0) { hex, name in
                                Button {
                                    paint(hex)
                                } label: {
                                    Label {
                                        Text(name)
                                    } icon: {
                                        Image(uiImage: Colouring.dot(hex))
                                    }
                                }
                            }
                        } label: {
                            Image(systemName: "paintpalette")
                        }
                        .buttonStyle(.bordered)
                        .accessibilityLabel("文字色")
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                }
                Divider()
            }
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    // **The arrows a phone keyboard does not have.** Moving
                    // the cursor by dragging a magnifying glass around is the
                    // single most miserable part of writing anything longer
                    // than a line on a phone. In vim's order, because that is
                    // the order his hands know.
                    mark("左", "arrow.left") { pen.step(.left) }
                    mark("下", "arrow.down") { pen.step(.down) }
                    mark("上", "arrow.up") { pen.step(.up) }
                    mark("右", "arrow.right") { pen.step(.right) }
                    Divider().frame(height: 20)
                    // The phone's own undo, not a second one written here —
                    // this is the same stack the shake gesture and the
                    // three-finger swipe use.
                    mark("元に戻す", "arrow.uturn.backward") { pen.undo() }
                        .disabled(!pen.canUndo)
                    mark("やり直す", "arrow.uturn.forward") { pen.redo() }
                        .disabled(!pen.canRedo)
                    Divider().frame(height: 20)
                    // Pressing it again goes deeper: # → ## → ### → none.
                    // Three buttons would be three names for one idea.
                    mark("見出し", "number", on: heads > 0) { put(Marks.deepen(tab.text, tab.pick)) }
                    mark("箇条書き", "list.bullet") { line("- ") }
                    mark("チェック", "checklist") { line("- [ ] ") }
                    mark("太字", "bold") { wrap("**") }
                    Divider().frame(height: 20)
                    mark(more ? "たたむ" : "ほかの記号", "ellipsis", on: more) {
                        withAnimation(.easeOut(duration: 0.15)) { more.toggle() }
                    }
                    Button("閉じる") { writing = false }.font(.callout)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
            }
        }
        .background(.bar)
    }

    private func mark(_ name: String, _ icon: String, on: Bool = false,
                      _ act: @escaping () -> Void) -> some View {
        Button(action: act) { Image(systemName: icon) }
            .buttonStyle(.bordered)
            .tint(on ? Color.accentColor : nil)
            .accessibilityLabel(name)
    }

    // MARK: the edits

    /// How many `#` the cursor's line already carries.
    private var heads: Int {
        let r = Marks.lineRange(tab.text, tab.pick)
        let row = (tab.text as NSString).substring(with: r)
        return row.prefix(while: { $0 == "#" }).count
    }

    private func line(_ prefix: String) { put(Marks.line(tab.text, tab.pick, prefix)) }
    private func wrap(_ mark: String) { put(Marks.wrap(tab.text, tab.pick, mark)) }
    private func block(_ body: String, caret: Int? = nil) {
        put(Marks.block(tab.text, tab.pick, body, caret: caret))
    }

    /// Wrap what is selected in a colour.
    ///
    /// With nothing selected there is nothing to paint, so this opens an
    /// empty pair and leaves the cursor inside it — the same thing 太字 does,
    /// for the same reason.
    private func paint(_ hex: String) {
        let s = tab.text as NSString
        let inner = tab.pick.length > 0 ? s.substring(with: tab.pick) : ""
        guard let out = try? store.painted(inner, hex) else { return }
        let inside = (out as NSString).range(of: inner.isEmpty ? ">" : inner)
        let at = tab.pick.location + inside.location + (inner.isEmpty ? 1 : 0)
        put(Edit(at: tab.pick, with: out,
                 then: NSRange(location: at, length: (inner as NSString).length)))
    }

    /// Make one edit through the text view, so the phone's undo knows it
    /// happened.
    private func put(_ e: Edit) {
        var text = tab.text
        var pick = tab.pick
        pen.apply(e, to: &text, pick: &pick)
        tab.text = text
        tab.pick = pick
    }

    /// A task pressed in the reading view.
    ///
    /// Written straight to the text; the desk saves it a moment later, like
    /// anything else typed.
    private func tick(_ b: Block) {
        guard b.line >= 0 else { return }
        do {
            tab.text = try store.checked(tab.text, line: b.line, done: !b.done)
            tab.blocks = try store.blocks(of: tab.text)
        } catch { trouble = error.localizedDescription }
    }

    private var folder: URL {
        URL(fileURLWithPath: tab.note.path).deletingLastPathComponent()
    }
}
