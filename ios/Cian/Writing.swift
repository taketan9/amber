import SwiftUI
import UIKit

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
    /// 絵文字の板を出しているか（依頼 418）。
    @State private var facing = false
    /// 絵の大きさを訊いているか（依頼 420）。
    @State private var sizing = false
    /// 表示の面で叩かれた、触れないかたまり・リンク（依頼 403）。
    /// **どの小窓を出すか**を決めるのはこちら（閉じると空になる）。
    @State private var tapped: Tapped?
    /// **何を叩いたか**はこちらに残す ── 小窓は「閉じてから」釦の用事を
    /// 走らせるので、`tapped` を読みに行くともう空になっている。
    @State private var held: Tapped?
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
            // **caret を動かす矢印。** 「コード」の面は `UITextView` なので
            // iOS が鍵盤の上に純正の帯（∧ ∨）を出すが、「表示」の面は
            // `WKWebView` なので出ない ── iPhone で caret を一文字動かすのは
            // 指では難しい（本人の指摘・2026-09-08）。
            //
            // **流れない一列にする。** 打っている間ずっと使うものなので、
            // 記号の帯に混ぜると流れた先へ行ってしまう（本人が「下の帯が
            // 二列になってもよい」と言ったのはここ）。
            HStack(spacing: 2) {
                arrow("chevron.left", "mv:left")
                arrow("chevron.up", "mv:up")
                arrow("chevron.down", "mv:down")
                arrow("chevron.right", "mv:right")
                Spacer()
            }
            .padding(.horizontal, 10)
            .padding(.top, 5)
            Divider()
            HStack(spacing: 0) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 6) {
                    mark("見出し", "number") { hand.mark("head") }
                    mark("箇条書き", "list.bullet") { hand.mark("ul") }
                    mark("チェック", "checklist") { hand.mark("check") }
                    mark("番号リスト", "list.number") { hand.mark("ol") }
                    // **一覧の釦の隣に置く。** 電話に Tab は無いので、段を
                    // 深く・浅くするのはこの二つが手になる（窓の
                    // Tab / Shift+Tab と同じ。段落では字下げ／字下げ外し）。
                    // 帯は横に流れるので、**使うものの隣に置かないと
                    // 流れた先に埋もれる** ── 一度に見えるのは六つだけ。
                    mark("一段深く", "increase.indent") { hand.mark("in") }
                    mark("一段浅く", "decrease.indent") { hand.mark("out") }
                    Divider().frame(height: 20)
                    mark("太字", "bold") { hand.mark("bold") }
                    mark("斜体", "italic") { hand.mark("italic") }
                    mark("取り消し線", "strikethrough") { hand.mark("strike") }
                    mark("引用", "text.quote") { hand.mark("quote") }
                    // **新しい段落は、ここから。** 電話の Return は改行に
                    // した（本人が決めた・2026-09-08「改行二回で段落」は
                    // 取らない）ので、段落を分ける手をどこかに置く必要がある。
                    mark("新しい段落", "text.insert") { hand.mark("para") }
                    Divider().frame(height: 20)
                    mark("画像", "photo", act: photo)
                    mark("表", "tablecells", act: table)
                    // **絵文字は一列目。** 毎日の返事に使うもので、畳んだ
                    // ほうに入れるとあることに気づかれない（依頼 418）。
                    //
                    // 絵は SF Symbol の顔にする ── 帯はぜんぶ一色の記号で
                    // 揃っていて、ここだけ色の付いた 😀 を置くと、記号の帯に
                    // 絵が一つ落ちているように見える（窓の帯は字なので、
                    // あちらは 😀 そのものを置いた）。
                    mark("絵文字", "face.smiling") { facing = true }
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
    /// caret を動かす一つ。**絵だけ** ── 四つ並ぶので、名前を付けると
    /// 一列に収まらない（意味は形で分かる）。
    private func arrow(_ icon: String, _ what: String) -> some View {
        Button { hand.mark(what) } label: {
            Image(systemName: icon)
                .font(.system(size: 15, weight: .semibold))
                .frame(width: 46, height: 30)
        }
        .buttonStyle(.plain)
        .foregroundStyle(.tint)
        .accessibilityLabel(["mv:left": "左へ", "mv:up": "上へ",
                             "mv:down": "下へ", "mv:right": "右へ"][what] ?? "")
    }

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
    @AppStorage("amber.font") private var font = Size.system

    struct Fixing: Identifiable {
        let md: String
        var id: String { md }
    }

    /// **押されたもの。** 窓は面の中に吹き出しを描くが、電話は iOS の小窓で
    /// 訊く ── 面の中に自前で描くと、鍵盤や選び目の丸とぶつかる。
    struct Tapped: Identifiable {
        /// `fig`（図）・`pre`（枠）・`img`（絵）・`link`。
        let kind: String
        /// 図と枠は元の字、リンクは行き先。
        let at: String
        /// ファイルの行（前書きを含む）。リンクは -1。
        let line: Int
        var id: String { kind + "\u{1}" + at + "\u{1}" + String(line) }
    }

    /// その種類の小窓が出ているか ── 閉じたら憶えも空にする。
    private func showing(_ kind: String) -> Binding<Bool> {
        Binding(get: { tapped?.kind == kind }, set: { if !$0 { tapped = nil } })
    }

    /// 選ばれた大きさを、**ノートの字に書く**（依頼 420）。
    ///
    /// 押して選んだ結果が `![猫 w:200px](…)` という**打てる字**として残る ──
    /// あとから記法で直せるし、amber の外でも読める（芯の 1）。
    ///
    /// 書き換えるのは**その一行だけ** ── 同じ絵を二度貼っている人の、
    /// もう一方まで変えない（`held.at` は押された絵の元の字）。
    private func size(_ width: String?) {
        guard let was = held?.at, !was.isEmpty else { return }
        do {
            let now = try store.sized(was, width: width)
            guard now != was else { return }
            tab.text = tab.text.replacingOccurrences(of: was, with: now)
        } catch { trouble = error.localizedDescription }
    }

    /// 絵文字を、いま打っているところへ（依頼 418）。
    ///
    /// **面ごとに入れ方が違う。** 表示の面は `WKWebView` の中の caret、
    /// コードの面は `UITextView` の選び ── 同じ字を、それぞれの面が
    /// 憶えている場所へ置く。
    private func putFace(_ ch: String) {
        // **段ではなく、字として入れる。** `Marks.block` は新しい行に置く
        // ので、文の途中に絵文字を入れたい人には使えない。
        if tab.reading { hand.put(ch) } else { put(Marks.insert(tab.text, tab.pick, ch)) }
    }

    /// 枠を「コード」の面のその行へ。**表示のまま直せないものは、記号を出す。**
    private func toCode(_ line: Int) {
        // **小窓が閉じ切ってから替える。** 同じ拍で面を入れ替えると、
        // SwiftUI は小窓を畳む処理の途中で下の view を作り直すことになり、
        // 替えたはずの面が表示のまま残った（押しても何も起きないように
        // 見える）。次の拍に回すと、畳んでから替わる。
        DispatchQueue.main.async {
            tab.reading = false
            // **面が入れ替わってから飛ぶ。** 同じ拍で行を渡すと、受け取る
            // コードの面がまだ建っていない ── 頭のまま止まる。
            if line >= 0 { DispatchQueue.main.async { desk.jumping = line } }
        }
    }

    /// 行き先を外で開く ── **amber の中では開かない**。題字を面の中に
    /// 描いている以上、持っていかれると戻る道が無い（窓と同じ）。
    private func open(_ href: String) {
        guard let u = URL(string: href), u.scheme != nil,
              UIApplication.shared.canOpenURL(u) else {
            trouble = "この行き先は開けません: " + href
            return
        }
        UIApplication.shared.open(u)
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

    /// 入ってきたものの報せ。**面の上に置く** ── 「表示」でも「コード」でも
    /// 同じことが起きているので、片方の面の中に入れると面を替えたときに
    /// 消えたように見える（窓と同じ形・依頼 354）。
    ///
    /// 言葉は「更新」に揃えてある ── 帯と釦で言葉が割れると、同じことに
    /// 名前が二つ付く。
    @ViewBuilder
    private var band: some View {
        if !tab.came.isEmpty {
            HStack(spacing: 9) {
                Circle()
                    .fill(tab.eyes ? Color(red: 0.77, green: 0.34, blue: 0.31) : Color("BrandSchwa"))
                    .frame(width: 8, height: 8)
                Text(tab.eyes
                     ? "同じところを二人が更新しました。どちらにするか決めてください"
                     : "ほかの人が \(tab.came.count) 行更新しました")
                    .font(.footnote)
                Spacer(minLength: 8)
                Button("確認した") { desk.seenIncoming(tab.id) }
                    .font(.footnote.weight(.semibold))
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 9)
            .background(Color("BrandSchwa").opacity(0.12))
            Divider()
        }
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
                    band
                    Paper(text: $tab.text, folder: folder,
                          dark: look == .dark || (look == .auto && scheme == .dark),
                          size: font.px,
                          onCheck: tickLine, onAt: { tab.at = $0 },
                          came: tab.came, both: tab.both,
                          onFix: { fixingText = Fixing(md: $0) },
                          onMenu: {
                              let t = Tapped(kind: $0, at: $1, line: $2)
                              held = t
                              tapped = t
                          },
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
                // **触れないものとリンクは、叩くと訊く**（窓の吹き出しと同じ
                // 顔ぶれ・依頼 403）。前は一叩きで何も起きず、450 ミリ秒の
                // 長押しだけが工房へ行っていた ── 押せるものを押して何も
                // 起きないのは、壊れているのと見分けがつかない。
                //
                // **押されたものごとに、別の小窓を書き下す。** 一つの小窓の
                // 中で釦を組み替えるより、どれを押すと何が並ぶかがその場で
                // 読める ── 顔ぶれは四つしかない。
                .confirmationDialog("枠", isPresented: showing("pre"),
                                    titleVisibility: .visible) {
                    Button("コードで直す") { toCode(held?.line ?? -1) }
                    Button("消す", role: .destructive) { hand.did("drop") }
                    Button("やめる", role: .cancel) {}
                }
                .confirmationDialog("図", isPresented: showing("fig"),
                                    titleVisibility: .visible) {
                    Button("図を直す") { if let md = held?.at { fixingText = Fixing(md: md) } }
                    Button("消す", role: .destructive) { hand.did("drop") }
                    Button("やめる", role: .cancel) {}
                }
                .confirmationDialog("画像", isPresented: showing("img"),
                                    titleVisibility: .visible) {
                    // **絵の大きさは、押して選べる**（依頼 420）── 記法を
                    // 覚えていない人が、いちばん変えたがるのがこれ。
                    Button("大きさ…") { sizing = true }
                    Button("消す", role: .destructive) { hand.did("drop") }
                    Button("やめる", role: .cancel) {}
                }
                .confirmationDialog("画像の大きさ", isPresented: $sizing,
                                    titleVisibility: .visible) {
                    // 数は訊かない ── 打てる人は記法で書く（`![w:200px]`）。
                    // ここに来るのは打てない人なので、言葉で選ばせる。
                    Button("小さめ（横 200px）") { size("200px") }
                    Button("中くらい（横 400px）") { size("400px") }
                    Button("大きめ（横 640px）") { size("640px") }
                    Button("はばいっぱい") { size(nil) }
                    Button("やめる", role: .cancel) {}
                }
                .confirmationDialog(held?.at ?? "リンク", isPresented: showing("link"),
                                    titleVisibility: .visible) {
                    Button("開く") { open(held?.at ?? "") }
                    Button("字を直す") { hand.did("edit") }
                    Button("リンク先を写す") { UIPasteboard.general.string = held?.at }
                    Button("やめる", role: .cancel) {}
                }
            } else {
                VStack(spacing: 0) {
                    band
                    Editor(pen: pen, text: $tab.text, pick: $tab.pick, editing: $writing)
                    // Only while the keyboard is up, which is the only time
                    // it is *above the keyboard* rather than sitting at the
                    // bottom of a page nobody is typing into.
                    if writing { marks }
                }
            }
        }
        // **絵文字の板は、どちらの面からも**（依頼 418）。表示でもコードでも
        // 同じ板から同じ字が入る ── 面によって道具が違うと、面を替えた人が
        // 「さっきのはどこへ行った」になる。
        //
        // 板は選んでも閉じない ── 顔文字は続けて置くもので（「👍✨」）、
        // 一つ入れるたびに開き直させない。
        .sheet(isPresented: $facing) {
            Faces(put: putFace)
                .presentationDetents([.medium, .large])
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
                        mark("絵文字", "face.smiling") { facing = true }
                        mark("斜体", "italic") { wrap("*") }
                        mark("取り消し線", "strikethrough") { wrap("~~") }
                        // **`</>` は面の切り替えが持っている**（上の帯）ので、
                        // ここは波括弧にする ── 同じ絵が二つの意味を持つと、
                        // 押すまでどちらか分からない。
                        mark("コード", "curlybraces") { wrap("`") }
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

/// The icon, loaded rather than drawn.
///
/// Literally the app icon, so the thing on the home screen and the thing at
/// the top of the list cannot drift apart.
///
/// **いまは電話のどこからも呼んでいない**（2026-09-07、帯の印を名前に
/// 替えたので ── 絵と字を並べると「くどい」）。残してあるのは、絵そのもの
/// （`Mark.imageset`）はまだ束ねに入っていて、窓は空の面で同じ一枚を出して
/// いるから。下の注釈に、描き直すと必ずずれるという教訓が残っている。
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
