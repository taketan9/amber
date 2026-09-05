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
    /// Asks the desk for the table sheet. **Presented up there, not here** —
    /// a sheet put on a `TabView` page is a sheet on a view the TabView is
    /// free to rebuild, and it does not open.
    let table: () -> Void
    /// Likewise the photo picker.
    let photo: () -> Void
    /// Whether the seldom-used half of the writing bar is unfolded.
    @State private var more = false
    @State private var trouble: String?

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
                        mark("表", "tablecells", act: table)
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
            // What a note is made of. Pressing 見出し again goes deeper:
            // # → ## → ### → none. Three buttons would be three names for
            // one idea.
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    mark("見出し", "number", on: heads > 0) { put(Marks.deepen(tab.text, tab.pick)) }
                    mark("箇条書き", "list.bullet") { line("- ") }
                    mark("チェック", "checklist") { line("- [ ] ") }
                    mark("太字", "bold") { wrap("**") }
                    mark("画像", "photo", act: photo)
                    Divider().frame(height: 20)
                    mark(more ? "たたむ" : "ほかの記号", "ellipsis", on: more) {
                        withAnimation(.easeOut(duration: 0.15)) { more.toggle() }
                    }
                }
                .padding(.horizontal, 10)
                .padding(.top, 6)
            }
            // Moving about, on its own row and pushed to the right — the
            // side the thumb is on, and away from the marks so a press meant
            // for one is never a press on the other.
            HStack(spacing: 6) {
                Button("閉じる") { writing = false }.font(.callout)
                Spacer(minLength: 0)
                // The phone's own undo, not a second one written here.
                mark("元に戻す", "arrow.uturn.backward") { pen.undo() }
                    .disabled(!pen.canUndo)
                mark("やり直す", "arrow.uturn.forward") { pen.redo() }
                    .disabled(!pen.canRedo)
                Spacer().frame(width: 18)
                // **The arrows a phone keyboard does not have.** In vim's
                // order, because that is the order his hands know.
                mark("左", "arrow.left") { pen.step(.left) }
                mark("下", "arrow.down") { pen.step(.down) }
                mark("上", "arrow.up") { pen.step(.up) }
                mark("右", "arrow.right") { pen.step(.right) }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
        }
        .background(.bar)
    }

    private func mark(_ name: String, _ icon: String, on: Bool = false,
                      act: @escaping () -> Void) -> some View {
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
            // **The whole note, because a task's line number is a line
            // number in the file.** The editor holds only the body; the
            // front matter is still up there taking lines.
            let whole = try store.checked(tab.whole, line: b.line, done: !b.done)
            let (head, body) = try store.split(whole)
            tab.head = head
            tab.text = body
            tab.blocks = try store.blocks(of: whole)
        } catch { trouble = error.localizedDescription }
    }

    private var folder: URL {
        URL(fileURLWithPath: tab.note.path).deletingLastPathComponent()
    }
}

/// The name of the thing, at the top of its own list.
///
/// **A navigation bar's large title is somebody else's typography.** cian
/// gets one screen where its name is the first thing you see, so it is set
/// the way the icon is set — the mark, the word, and one quiet line of what
/// is inside. Below the top folder the bar takes over again: there the
/// question is *where am I*, and a wordmark does not answer it.
struct Wordmark: View {
    let notes: Int
    let books: Int

    var body: some View {
        HStack(spacing: 14) {
            Mark().frame(width: 38, height: 38)
            VStack(alignment: .leading, spacing: 1) {
                Text("amber")
                    .font(.system(size: 34, weight: .heavy, design: .rounded))
                    .kerning(-0.5)
                Text("\(notes) のノート ・ \(books) のフォルダ")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
            Spacer(minLength: 0)
        }
    }
}

/// The icon, loaded rather than drawn.
///
/// Literally the app icon, so the thing on the home screen and the thing at
/// the top of the list cannot drift apart.
struct Mark: View {
    // **アプリのアイコンそのもの**を小さくして出す。案2「琥珀の中の
    // Markdown」で、`packaging/amber_icon.py` が焼いた 128px の一枚。
    //
    // 前はここに葉（案 S4）を `Path` で描いていた。同じ形が
    // `packaging/amber.svg`・`packaging/amber.py`・`gui/renderer.js` にもあり、
    // 四か所が揃っているかを `agree()` が見張っていた ── それでも**アイコンを
    // 替えた日に、中の印だけが前の絵のまま残った**。見張れていたのは「四つの
    // 写しが揃っているか」であって、「アイコンと同じか」ではなかった。
    // 同じ一枚を渡せば、ずれようがない。
    //
    // 色替えには追従させない（前の印は `Color.accentColor` を拾っていた）。
    // **ロゴが端末の気分で色を変えるのは、ロゴではない。**
    var body: some View {
        Image("Mark")
            .resizable()
            .interpolation(.high)
            .aspectRatio(contentMode: .fit)
            .accessibilityHidden(true)
    }
}

/// Where you are, as the trail of names it is.
///
/// **A folder's own name is not an answer to "where am I".** Two folders
/// called 「2026」 in two different places look identical at the top of a
/// list, and the one thing the title bar had room to say was the half that
/// does not tell them apart. Each name is a step back to that level.
struct Crumbs: View {
    /// The path from the root, `""` for the root itself.
    let at: String
    let root: String
    /// Called with the path to walk back to.
    let go: (String) -> Void

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 4) {
                step(root, "")
                ForEach(Array(parts.enumerated()), id: \.offset) { i, name in
                    Image(systemName: "chevron.right")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                    step(String(name), parts[...i].joined(separator: "/"))
                }
            }
            .padding(.vertical, 2)
        }
    }

    private var parts: [String] {
        at.split(separator: "/").map(String.init)
    }

    /// The one you are in is in the text colour; the way back is the accent.
    /// Colouring them the same would make the last name look like something
    /// to press, and pressing it does nothing.
    private func step(_ name: String, _ path: String) -> some View {
        let here = path == at
        return Button {
            if !here { go(path) }
        } label: {
            Text(name)
                .font(.subheadline.weight(here ? .semibold : .regular))
                .foregroundStyle(here ? AnyShapeStyle(.primary) : AnyShapeStyle(.tint))
                .lineLimit(1)
        }
        .buttonStyle(.plain)
        .disabled(here)
    }
}

/// Every folder there is, laid out as the shape it is.
///
/// A list shows one level at a time, which is the right way to *use* a
/// folder and the wrong way to *understand* one. This is the other question:
/// what is in here, and how deep does it go.
struct Tree: View {
    @ObservedObject var store: NotesStore
    let go: (String) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section {
                    row(store.rootName, "", 0, store.notes.count)
                    ForEach(store.allBooks, id: \.self) { b in
                        row(b.split(separator: "/").last.map(String.init) ?? b,
                            b,
                            b.split(separator: "/").count,
                            store.under(b))
                    }
                } footer: {
                    Text("数字は、そのフォルダの中にあるノートの本数です（下の階層も数えます）。")
                }
            }
            .navigationTitle("フォルダの構成")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("閉じる") { dismiss() } }
            }
        }
    }

    private func row(_ name: String, _ path: String, _ depth: Int, _ count: Int) -> some View {
        Button {
            go(path)
            dismiss()
        } label: {
            HStack(spacing: 8) {
                // The indent *is* the structure — `allBooks` is already every
                // folder in order, so the depth of the path is the depth of
                // the row and nothing has to be assembled.
                if depth > 0 {
                    Spacer().frame(width: CGFloat(depth - 1) * 18)
                    Image(systemName: "arrow.turn.down.right")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
                Image(systemName: depth == 0 ? "tray.full.fill" : "folder.fill")
                    .foregroundStyle(store.colors[path].flatMap { Color(hex: $0) }
                        .map { AnyShapeStyle($0) } ?? AnyShapeStyle(.tint))
                Text(name).lineLimit(1)
                Spacer(minLength: 6)
                Text("\(count)").foregroundStyle(.secondary).monospacedDigit()
                if path == store.at {
                    Image(systemName: "location.fill").font(.caption2).foregroundStyle(.tint)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}
