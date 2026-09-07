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
    /// 目次からの飛び先を受け取るため ── **面は二つある**ので、飛ぶ先は
    /// desk が持ち、飛ぶのはこちら。
    @ObservedObject var desk: Desk
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
    /// 長押しされた図の、元の字（枠ごと）。
    @State private var fixingText: Fixing?
    /// 「表示」の面へ合図を渡す糸。
    @StateObject private var hand = PaperHand()
    /// 表示の面で鍵盤が出ているか ── 帯を出すかどうかの目安。
    @State private var reading = true

    /// 表示の面の道具。**書く面より少ない** ── `execCommand` で確かに
    /// できるものだけを出す。できないものを並べると、押しても何も起きない
    /// 釦ができ、それはあることより悪い。
    private var readMarks: some View {
        VStack(spacing: 0) {
            Divider()
            // **戻す・やり直すは流れない。** 記号は横に流れる帯だが、この
            // 二つは押し続けるものなので、端に固定して指の下から逃げない
            // ようにする（窓の「ほかの記号」の釦と同じ考え）。
            HStack(spacing: 0) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    mark("見出し", "number") { hand.mark("head") }
                    mark("箇条書き", "list.bullet") { hand.mark("ul") }
                    mark("チェック", "checklist") { hand.mark("check") }
                    mark("番号リスト", "list.number") { hand.mark("ol") }
                    mark("太字", "bold") { hand.mark("bold") }
                    mark("斜体", "italic") { hand.mark("italic") }
                    mark("取り消し線", "strikethrough") { hand.mark("strike") }
                    mark("引用", "text.quote") { hand.mark("quote") }
                    Divider().frame(height: 20)
                    mark("画像", "photo", act: photo)
                    mark("表", "tablecells", act: table)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
            }
            steps
            }
        }
        .background(.bar)
    }

    /// 一つ戻す・やり直す。**「表示」でも「コード」でも同じ一本**（desk が
    /// ノートの姿を積んでいる ── `Desk.stepBack`）。
    ///
    /// UIKit の取り消しではない ── あれは「打った字」の取り消しで、見出しや
    /// 升のように面を組み直したところで積み木ごと消えるし、「表示」の面には
    /// そもそも届かない。同じ名前の道具が二つあって片方だけ効かないのは、
    /// 見分けの付かない差になる。
    private var steps: some View {
        HStack(spacing: 6) {
            Divider().frame(height: 20)
            mark("一つ戻す", "arrow.uturn.backward") { desk.stepBack(forward: false, store) }
                .disabled(!desk.canStepBack)
            mark("やり直す", "arrow.uturn.forward") { desk.stepBack(forward: true, store) }
                .disabled(!desk.canStepForward)
        }
        .padding(.trailing, 10)
        .padding(.vertical, 7)
    }
    @Environment(\.colorScheme) private var scheme
    @AppStorage("cian.look") private var look = Look.auto

    struct Fixing: Identifiable {
        let md: String
        var id: String { md }
    }

    /// ` ```mermaid ` の中身。
    private func fence(_ md: String) -> String {
        var lines = md.components(separatedBy: "\n")
        if lines.first?.hasPrefix("```") == true { lines.removeFirst() }
        if lines.last?.trimmingCharacters(in: .whitespaces).hasPrefix("```") == true {
            lines.removeLast()
        }
        return lines.joined(separator: "\n")
    }

    /// 目次で選ばれた見出しへ。**書く面ならその行へ、表示ならその見出しへ**
    /// （窓の `gotoHead` と同じ）。
    ///
    /// core の行番号は前書きを含むファイルの行。書く面が持っているのは
    /// 本文だけなので、前書きのぶんを引く ── 引き忘れると、前書きのある
    /// ノートでだけ数行ずれる（升の行番号で一度やった）。
    private func jump(_ line: Int) {
        guard line >= 0 else { return }
        if tab.reading { hand.go(line: line); return }
        let head = tab.head.isEmpty ? 0 : tab.head.components(separatedBy: "\n").count - 1
        let want = max(0, line - head)
        let rows = tab.text.components(separatedBy: "\n")
        guard want < rows.count else { return }
        var at = 0
        for i in 0..<want { at += rows[i].utf16.count + 1 }
        tab.pick = NSRange(location: at, length: 0)
    }

    /// 升を押されたとき ── 行番号で裏返す（何番目の升かではない）。
    private func tickLine(_ line: Int, _ done: Bool) {
        guard line >= 0 else { return }
        do {
            let whole = try store.checked(tab.whole, line: line, done: done)
            let (head, body) = try store.split(whole)
            tab.head = head
            tab.text = body
            tab.blocks = try store.blocks(of: whole)
        } catch { trouble = error.localizedDescription }
    }

    /// 直した図を、本文の中の元の場所へ返す。
    ///
    /// **枠ごと入れ替える。** 行番号で切ると、前書きのあるノートでずれる
    /// ── 元の字そのものを探して置き換えるほうが、数え方を一つ減らせる。
    /// 同じ図が二つあるノートでは前のほうが変わるが、そこで人が見ている
    /// のはたいてい前のほう。
    private func swapFence(_ whole: String, was: String, now: String) -> String {
        let from = "```mermaid\n" + was + "\n```"
        let to = "```mermaid\n" + now.trimmingCharacters(in: .whitespacesAndNewlines) + "\n```"
        if whole.contains(from) { return whole.replacingOccurrences(of: from, with: to) }
        // 枠の言葉が `mermaid` 以外（大文字など）で書かれていることがある。
        if whole.contains(was) { return whole.replacingOccurrences(of: was, with: now) }
        return whole
    }

    var body: some View {
        Group {
            if tab.reading {
                // **窓と同じ面。** 組む側は core の `to_html`、書き戻す側は
                // `gui/renderer.js` から切り出した一組（`paper.js`）── 電話
                // だけ読むだけだと、同じ名前の面が二つの amber で別のものに
                // なる。SwiftUI で書き直すと書き戻しがもう一組でき、同じ
                // ノートが端末によって別の字に保存される。
                VStack(spacing: 0) {
                    Paper(text: $tab.text, folder: folder,
                          dark: look == .dark || (look == .auto && scheme == .dark),
                          onCheck: tickLine, onFix: { fixingText = Fixing(md: $0) },
                          hand: hand)
                    // **道具の帯は、表示の面にも要る。** 打てる面なのに
                    // 記号の入れ方が無いと、`#` や `- [ ]` を覚えている人に
                    // しか使えない ── 電話の鍵盤にその記号は出ていない。
                    if reading { readMarks }
                }
                // **工房はここで開く。** 図は表示の面の中にあり、直した字を
                // 戻す先はこのノートの本文なので、間に人を挟まない。
                .sheet(item: $fixingText) { f in
                    Studio(source: f.md) { now in
                        tab.text = swapFence(tab.text, was: fence(f.md), now: now)
                    }
                }
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
        // 目次で選ばれた見出しへ。**受け取ったら空に戻す** ── 残しておくと、
        // 面を入れ替えたときにもう一度飛ぶ。
        .onChange(of: desk.jumping) { _, line in
            guard let line else { return }
            jump(line)
            desk.jumping = nil
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
                // **ここは UIKit の取り消しだった。** あれは「打った字」の
                // 取り消しで、見出しや升のように面を組み直したところで積み木
                // ごと消える ── しかも「表示」の面には届かない。desk が
                // ノートの姿を積む一本に替えた（窓と同じ理由・同じ持ち方）。
                mark("一つ戻す", "arrow.uturn.backward") { desk.stepBack(forward: false, store) }
                    .disabled(!desk.canStepBack)
                mark("やり直す", "arrow.uturn.forward") { desk.stepBack(forward: true, store) }
                    .disabled(!desk.canStepForward)
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
                Text("ambƏr")
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
    /// ここから新しいフォルダを作る。**選ぶのと作るのは、同じ用事の裏表**
    /// ── 「フォルダへ行きたい」で開いて、無ければその場で作る。
    var make: ((String) -> Void)?
    @Environment(\.dismiss) private var dismiss
    @State private var making: String?

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
            .navigationTitle("フォルダ")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("閉じる") { dismiss() } }
                if make != nil {
                    ToolbarItem(placement: .topBarTrailing) {
                        Button { making = store.at } label: {
                            Image(systemName: "folder.badge.plus")
                        }
                        .accessibilityLabel("新しいフォルダ")
                    }
                }
            }
            .sheet(item: Binding(
                get: { making.map { Where.Named(name: $0) } },
                set: { if $0 == nil { making = nil } }
            )) { at in
                Booking(inside: at.name.isEmpty ? store.rootName : at.name) { name in
                    make?(name)
                    making = nil
                    dismiss()
                }
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
