import SwiftUI
import UniformTypeIdentifiers
import PhotosUI

/// ノートの一覧と、開いている 1 件。
///
/// 1 行はタイトルと、その下の 1 行 ── デスクトップ版が描くのと同じ 2 行で、
/// 理由も同じ。目はタイトルを縦に追い、どこかで止まったときにだけ
/// 2 行目に降りる。
struct ContentView: View {
    @StateObject private var store = NotesStore()
    @StateObject private var desk = Desk()
    @ObservedObject private var ring = Ring.shared
    @State private var picking = false
    @State private var naming = false
    @State private var booking = false
    /// ただ 1 つのファイル選択が、今回は何を訊かれているか。
    @State private var fetching: Fetching?
    /// **同じものを、答えを受け取るために保持しておく。**
    ///
    /// 選択画面を閉じると `isPresented` が false になり、その setter が
    /// `fetching` を消す ── そして*そのあとで*完了処理が走ってそれを読むので、
    /// そのときには `nil` になっている。つまり画面が開き、フォルダが選ばれ、
    /// 何も起きない ── 答えが `nil` の枝に落ちる。SwiftUI の表示が、自分の
    /// コールバックが必要としているものを消したのはこれで 3 度目で、
    /// 開く側だけを見て「確かめた」と言ったのは 2 度目。
    ///
    @State private var asked: Fetching?
    enum Fetching { case folder, addFolder, notes, zip, outside }
    /// 場所を変えようとしている保存ディレクトリ（`nil` は足す）。
    @State private var relocating: String?
    /// いま離れたフォルダ。中のノートを持っていくか訊いているあいだだけ保持する。
    @State private var moving: URL?
    @State private var moved: String?
    @State private var showing = false
    /// カレンダーが出ているか（依頼 454）。
    @State private var showCal = false
    /// 指がどのフォルダの行の上にあるか、そしてそれが `..` かどうか。
    @State private var into: String?
    @State private var outside = false
    @State private var needle = ""
    /// 探す欄が出ているか。**ふだんは畳んでおく**（デスクトップ版と同じ）。
    @State private var seeking = false
    /// いま開いている絞り込みの引き出し。
    @State private var sifting: Sifting.Which?
    /// これから共有のフォルダにするフォルダ（名乗りを訊いている間）。
    @State private var sharing: String?
    /// あなたの名乗り。**設定画面に置かない** ── 一度しか使わないものを、
    /// 毎日見る画面に置く値打ちは無い。要る瞬間に一度だけ訊いて憶える。
    @AppStorage("cian.me") private var me = ""
    @State private var shelving: Note?
    @State private var colouring: String?
    @State private var renaming: String?
    @State private var dropping: String?
    @State private var fresh = ""
    @State private var treeing = false
    /// この中にフォルダを作る（親の道）。
    @State private var making: String?
    /// 一覧がどのフォルダのものか、そして最後にどちら向きに動いたか。
    @State private var walked = ""
    /// 履歴を見せている相手（ノートかフォルダ）。
    @State private var past: Past.Which?
    /// 消してよいか訊いている相手。**取り返しがつかないので、必ず訊く。**
    @State private var dropping2: Note?
    @State private var deeper = true
    @State private var wide: CGFloat = 393

    var body: some View {
        NavigationStack {
            Group {
                if store.rootName.isEmpty {
                    empty
                } else {
                    list
                }
            }
            // ノートブックを選んでいればその名前 ── タイトルバーは
            // 「いま何を見ているか」を知るために見る場所で、絞り込んだ
            // 一覧がフォルダ名のままだと「ノートが消えた一覧」に見える。
            // 最上位では名前を一覧の中に描くので、バーは引っ込む。
            // フォルダの中では、バーが現在地を言う。
            // 経路は一覧の中に描くので、バーは引っ込む ── 大きな見出しが
            // フォルダ名を言い、*さらに*パンくずが同じ名前を言えば、
            // 名前が 2 回出ることになる。
            .navigationTitle("")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                if let up = store.up {
                    ToolbarItem(placement: .topBarLeading) {
                        Button { go { store.leave(for: up) } } label: {
                            Label("上へ", systemImage: "chevron.backward")
                        }
                    }
                }
                // **最上段は歯車だけ。** 前は新しいフォルダ・新しいノート・
                // 並びの3 つが並んでいて、デスクトップ版の左の列とは別のものになって
                // いた。作るのは一覧の頭のボタン、並べ替えとフィルタはその下 ──
                // デスクトップ版がそうしているので、二つの amber で同じ場所を探せる。
                if store.up == nil {
                    ToolbarItem(placement: .topBarLeading) {
                        // **マークではなく、名前。**
                        //
                        // 前はここに `Mark()` を 26pt で置いていた ── iPhone は
                        // Dock も ⌘Tab も無く、外に「いま何のアプリか」を
                        // 言ってくれるものが無いから。名前に姿ができたので、
                        // 綴りそのものを出す方に替えた（デスクトップ版の掴む帯と同じ）。
                        //
                        // **絵と文字を並べない**（本人の言葉で「くどい」）──
                        // 同じことを二つの形で言うことになる。
                        //
                        // 大きな題（34pt）にはしなかった: あれは巻けば縮む
                        // かわりに、開くたび一覧を 52pt 押し下げる。帯なら
                        // 高さは変わらない ── ここは一覧を見に来る画面。
                        (Text("amb")
                            + Text("ə").foregroundColor(Color("BrandSchwa"))
                            + Text("r"))
                            .font(.headline)
                            .accessibilityLabel("ambər")
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button { picking = true } label: { Image(systemName: "gearshape") }
                        .accessibilityLabel("設定")
                }
            }
        }
        .alert("ゴミ箱へ入れますか", isPresented: Binding(
            get: { dropping2 != nil }, set: { if !$0 { dropping2 = nil } }
        )) {
            Button("やめる", role: .cancel) {}
            Button("入れる", role: .destructive) {
                if let n = dropping2 { remove(n) }
            }
        } message: {
            Text(dropping2.map { "「\($0.shown)」" } ?? "")
        }
        .sheet(item: Binding(get: { sharing.map { Naming.Which(at: $0) } },
                             set: { if $0 == nil { sharing = nil } })) { w in
            Naming(folder: w.at, me: $me) { by in
                do { try store.setShare(w.at, by: by) }
                catch { store.trouble = error.localizedDescription }
            }
        }
        .sheet(item: $past) { w in
            Past(store: store, at: w.at, isBook: w.book)
        }
        .sheet(isPresented: $showCal) {
            Calendaring(store: store, open: { note in
                desk.open(note, store)
                showing = true
            })
        }
        .sheet(isPresented: $picking) {
            // シートが先に自分で閉じ、これらはひと呼吸あとに、
            // 表示が重なっていないここから開く。
            Where(store: store,
                  choose: { id in DispatchQueue.main.async {
                      relocating = id
                      asked = id == nil ? .addFolder : .folder
                      fetching = asked
                  } },
                  bringIn: { DispatchQueue.main.async { asked = .notes; fetching = .notes } },
                  openOutside: { DispatchQueue.main.async { asked = .outside; fetching = .outside } },
                  restore: { DispatchQueue.main.async { asked = .zip; fetching = .zip } })
        }
        .alert("いままでのノートを持っていきますか", isPresented: Binding(
            get: { moving != nil }, set: { if !$0 { moving = nil } }
        )) {
            Button("そのままにする", role: .cancel) {}
            Button("丸ごと移す") {
                guard let old = moving,
                      let fresh = relocating.flatMap({ id in store.places.first { $0.id == id } }).flatMap(store.url(of:))
                else { return }
                do { moved = "\(try store.migrate(from: old, to: fresh)) 件を移しました。" }
                catch { store.trouble = error.localizedDescription }
            }
        } message: {
            if let old = moving {
                Text("フォルダの構成ごと、新しい保存場所へ移します（\(store.notesAt(old)) 件）。元の場所には残りません。同じ名前があるときは、何も動かしません。")
            }
        }
        .alert("できました", isPresented: Binding(
            get: { moved != nil }, set: { if !$0 { moved = nil } }
        )) { Button("閉じる") {} } message: { Text(moved ?? "") }
        .sheet(item: $shelving) { note in Shelving(store: store, note: note) }
        .sheet(item: $colouring) { f in Colouring(store: store, folder: f) }
        .sheet(item: Binding(
            get: { making.map { Where.Named(name: $0) } },
            set: { if $0 == nil { making = nil } }
        )) { at in
            Booking(inside: at.name.split(separator: "/").last.map(String.init) ?? at.name) { name in
                do { try store.makeBook(name, under: at.name) }
                catch { store.trouble = error.localizedDescription }
                making = nil
            }
        }
        .sheet(isPresented: $treeing) {
            Tree(store: store, go: { to in go { store.into(to) } },
                 make: { name in
                     do { try store.makeBook(name) }
                     catch { store.trouble = error.localizedDescription }
                 })
        }
        .alert("名前を変える", isPresented: Binding(
            get: { renaming != nil }, set: { if !$0 { renaming = nil } }
        )) {
            TextField("名前", text: $fresh)
            Button("やめる", role: .cancel) {}
            Button("変える") {
                guard let b = renaming else { return }
                do { try store.rename(b, to: fresh) }
                catch { store.trouble = error.localizedDescription }
            }
        } message: {
            Text("中のノートはそのままです。")
        }
        // **やる前に、数で言う。** 取り消す手段は
        // wastepaper basket on a phone: this is the real thing, and 「中の
        // ノートごと」 is not a figure of speech.
        // 確認文はデスクトップ版と同じ形（本人・2026-09-12）── iPhone にゴミ箱は無いので「削除」。
        .alert("「\(dropping.map { $0.split(separator: "/").last.map(String.init) ?? $0 } ?? "")」を、中の \(dropping.map(store.under) ?? 0) 件ごと削除しますか", isPresented: Binding(
            get: { dropping != nil }, set: { if !$0 { dropping = nil } }
        )) {
            Button("やめる", role: .cancel) {}
            Button("削除", role: .destructive) {
                guard let b = dropping else { return }
                do { try store.drop(b) }
                catch { store.trouble = error.localizedDescription }
            }
        } message: {
            if let b = dropping {
                let n = store.under(b)
                Text(n == 0
                     ? "「\(b.split(separator: "/").last.map(String.init) ?? b)」は空です。元には戻せません。"
                     : "「\(b.split(separator: "/").last.map(String.init) ?? b)」の中のノート \(n) 件も一緒に消えます。元には戻せません。")
            }
        }
        .sheet(isPresented: $booking) {
            Booking(inside: store.here) { name in
                do { try store.makeBook(name) }
                catch { store.trouble = error.localizedDescription }
            }
        }
        // 押された通知は、それが指していたノートを開く。通知はノートが
        // 読み込まれる前に届きうる（ロック画面から押した、冷えた状態での
        // 起動）ので、通知だけでなくストアも見張る ── 後に来たほうが
        // 開く。
        .onChange(of: ring.wanted) { _, _ in answer() }
        .onChange(of: store.notes) { _, _ in answer() }
        .task {
            store.restore()
            // 同期（依頼 500）── フォルダと机を渡して、サインインしていれば時計を回す。
            Syncing.shared.store = store
            Syncing.shared.desk = desk
            Syncing.shared.load()
            desk.store = store
            // 端末が別のことをしているあいだに繰り返しが溜めたぶん。
            // 入ってきたときに一度だけ訊く ── なぜ水曜の 9 時ではなく
            // この瞬間なのかは `Bell` を見よ。
            _ = await Bell.ask()
            store.catchUp()
        }
        // **`.fileImporter` は 1 つだけ。** 同じ View に 2 つ置くと 1 つに
        // なる ── SwiftUI は最後のものを残し、もう一方のボタンは
        // 何もしない。それが起きた場所の 2 行上にそう書いてあるのに、
        // for the second time: 「保存場所を選ぶ」 lost to 「インポート」 and
        // 押しても配線されていないボタンにしか見えなかった。だから
        // 1 つにして、何を選んでいるかで受け付けるものを決める。
        .fileImporter(
            isPresented: Binding(get: { fetching != nil },
                                 set: { if !$0 { fetching = nil } }),
            allowedContentTypes: {
                switch asked {
                case .folder, .addFolder: return [.folder]
                case .zip: return [.zip]
                default: return [UTType(filenameExtension: "md") ?? .plainText, .plainText]
                }
            }(),
            allowsMultipleSelection: asked == .notes
        ) { r in
            switch (asked, r) {
            case (.folder, .success(let urls)):
                // **移す前に訊き、切り替える前に訊く。** いまここにある
                // ノートは勝手にはついてこない ── 新しいフォルダは空の
                // フォルダで、それを予想していなかった人は、書いたもの全部を
                // 見失ったことになる。
                if let url = urls.first, let id = relocating,
                   let p = store.places.first(where: { $0.id == id }) {
                    let old = store.url(of: p)
                    store.relocate(id, to: url)
                    if let old, store.trouble == nil, store.notesAt(old) > 0 { moving = old }
                }
            case (.addFolder, .success(let urls)):
                // **足す**（依頼 511）── 入った直後は同期しない（`NotesStore.add`）。
                if let url = urls.first { store.add(url) }
            case (.notes, .success(let urls)):
                store.bring(urls)
            case (.outside, .success(let urls)):
                // ほかの場所の .md を一時的に開く（一覧には入れない・依頼 517）。
                if let u = urls.first, let n = store.openOutside(u) {
                    desk.open(n, store)
                    showing = true
                }
            case (.zip, .success(let urls)):
                guard let zip = urls.first else { break }
                do {
                    let (put, kept) = try store.restore(zip)
                    moved = kept > 0
                        ? "\(put) 件を戻しました。同じ名前の \(kept) 件は、いまのノートを残しました。"
                        : "\(put) 件を戻しました。"
                } catch { store.trouble = error.localizedDescription }
            case (_, .failure(let why)):
                store.trouble = why.localizedDescription
            case (nil, _):
                break
            }
            asked = nil
        }
        // **1 つの View に `.alert` を 2 つ置くと 1 つになる。** SwiftUI は
        // 最後のものを残し、もう一方はボタンが何もしないまま出る ──
        // 削除の確認がまさにそれだった。出るのに、どちらの答えも
        // 効かず、理由はどこにも出ない。こちらは 1 つ上、一覧ではなく
        // スタックに置いてある。
        .alert(
            "できません",
            isPresented: Binding(get: { store.trouble != nil }, set: { if !$0 { store.trouble = nil } })
        ) {
            Button("閉じる") {}
        } message: {
            Text(store.trouble ?? "")
        }
        .sheet(isPresented: $naming) {
            Making(make: { title, tags in
                if let note = make(title, tags) {
                    // そのまま編集側で開く ── 書くために作ったのだから。
                    //
                    // **作ったノートも「表示」で開く。** デスクトップ版がそうなので
                    // iPhone も同じに ── 打ちたくなったら、画面のどこを叩いても
                    // その場で編集画面に入る（`NoteView`）。
                    desk.open(note, store)
                    showing = true
                }
            }, known: store.allTags, stencils: store.stencils, fromStencil: { t in
                do {
                    guard let made = try store.fromStencil(t),
                          let note = store.notes.first(where: { $0.path == made }) else { return }
                    desk.open(note, store)
                    showing = true
                } catch { store.trouble = error.localizedDescription }
            }, seedStencils: {
                let n = store.addStencils()
                if n == 0, store.trouble == nil { store.trouble = "もう入っています" }
            })
        }

    }

    /// ここで `try?` を使うとそれが不具合そのものになる ── 黙って失敗した削除は、
    /// そもそも頼まれなかった削除と見分けがつかず、行は残ったままになる。
    /// ここで `try?` を使うとそれが不具合そのものになる ── 黙って失敗した削除は、
    /// そもそも頼まれなかった削除と見分けがつかず、行は残ったままになる。
    private func remove(_ note: Note) {
        do { try store.remove(note) } catch { store.trouble = error.localizedDescription }
    }

    @ViewBuilder
    private func row(_ note: Note) -> some View {
                Button {
                desk.open(note, store)
                showing = true
            } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 4) {
                            if note.star != nil {
                                Image(systemName: "star.fill")
                                    .font(.caption2).foregroundStyle(.orange)
                            }
                            Text(note.shown).font(.body.weight(.semibold)).lineLimit(1)
                            if note.shared {
                                Text("共有").font(.caption2.weight(.bold))
                                    .padding(.horizontal, 5).padding(.vertical, 1)
                                    .background(Capsule().fill(.green.opacity(0.2)))
                                    .foregroundStyle(.green)
                            }
                            if note.clash != nil {
                                Text("競合").font(.caption2.weight(.bold))
                                    .padding(.horizontal, 5).padding(.vertical, 1)
                                    .background(Capsule().fill(.orange.opacity(0.22)))
                                    .foregroundStyle(.orange)
                            }
                        }
                        // その語が実際にあった行を出す ── ノートの冒頭を
                        // 代わりに出すのは、誰も訊いていない問いに
                        // 答えることになる。
                        if let hit = store.hits[note.path], !needle.isEmpty {
                            Text(hit).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        } else if !note.excerpt.isEmpty {
                            Text(note.excerpt).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        }
                        // 二つ以上の保存ディレクトリがあるときは、よその保存ディレクトリの
                        // ノートにその名前を（依頼 511）── 探した結果に混ざる。
                        let away = store.many && !store.here(note)
                        if !note.tags.isEmpty || !note.book.isEmpty || away {
                            HStack(spacing: 6) {
                                if !note.book.isEmpty || away {
                                    // ノートブックを先に ── それは*どこか*を言い、
                                    // 同じ名前の 2 件を見分けるのはそこだから。
                                    // タグより控えめにする ── あちらは場所ではなく
                                    // 自分で選んだもの。
                                    Label(store.bookLabel(note), systemImage: "folder")
                                        .font(.caption2).foregroundStyle(.tint.opacity(0.8))
                                }
                                if !note.tags.isEmpty {
                                    Text(note.tags.map { "#\($0)" }.joined(separator: " "))
                                        .font(.caption2).foregroundStyle(.tint)
                                }
                            }
                        }
                    }
                    // 行の全体を当たり判定にする。ラベルの幅はいちばん長い行に
                    // 合うので、短いタイトルでは行の大半が指に反応しなかった ──
                    // ある場所では反応し、その隣では反応しない行は、
                    // 不具合に見える。
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                // 行は本文が求めた色をそのまま保つ ── リンクの青で描いた
                // タイトルは、一覧のすべてのノートについて「これはリンクだ」と
                // 言うことになる。それは全部に当てはまる唯一のこと。
                .buttonStyle(.plain)
                // 行を掴んでノートを持ち上げる。運ばれるのはパス ── ここの
                // どの操作も受け取るのがそれで、ノートは 2 つのフォルダの
                // 中間に存在できるものではない。
                .draggable(note.path)
                // スワイプしてからタップ ── その 2 段が確認そのもので、
                // Apple の「メモ」もそうしている。`allowsFullSwipe: false` に
                // してあるので長いスワイプだけでは消えない ── iPhone には
                // ゴミ箱が無く、ここで唯一取り消せない操作だから。
                //
                // もう一度訊くアラートは書いて、外した ── これらの検査を
                // 動かす自動のタップでは破壊的なボタンが反応せず、取り消しは
                // 反応した。その違いを説明できなかった。動くところを見て
                // いない確認を出すのは、見たことのあるこのジェスチャーより
                // 悪い。
                .swipeActions(allowsFullSwipe: false) {
                    Button("削除", role: .destructive) { remove(note) }
                }
                // ピン留めは反対側に置く ── 左から始まるスワイプは残すため、
                // 右から始まるスワイプは失うため。
                .swipeActions(edge: .leading) {
                    Button {
                        do { try store.star(note, on: note.star == nil ? "" : nil) }
                        catch { store.trouble = error.localizedDescription }
                    } label: {
                        Label(note.star == nil ? "ブックマークに登録する" : "外す",
                              systemImage: note.star == nil ? "star" : "star.slash")
                    }
                    .tint(.orange)
                }
                .contextMenu {
                    // **新しいタブで開く。** 押しただけならいまのタブを
                    // 差し替えるので（デスクトップ版と同じ）、増やすパスをここに置く ──
                    // iPhone に右押しは無いので、長押しがその手ぶり。デスクトップ版の
                    // 「⌥ 押し」と同じことをする。
                    Button {
                        desk.open(note, store)
                        showing = true
                    } label: {
                        Label("新しいタブで開く", systemImage: "rectangle.stack.badge.plus")
                    }
                    // **同じ中身のノートをもう一つ**（依頼 412）。下書きの型を
                    // 持っている人が、毎回それを開いて全部写していた。
                    //
                    // 置くのは**この段**（仕切りの上）── 下の3 つは
                    // 「ブックマーク → フォルダへ移す → エクスポート」という
                    // 手の伸びる順で並んでいて（依頼 90）、間に割り込ませると
                    // その順が崩れる。複製は「もう一つ増やす」ほうの仲間で、
                    // 「新しいタブで開く」と同じ段にあるのが素直。
                    Button {
                        do { _ = try store.duplicate(note) }
                        catch { store.trouble = error.localizedDescription }
                    } label: {
                        Label("複製", systemImage: "plus.square.on.square")
                    }
                    // **このノートをテンプレートにする**（依頼 506・デスクトップ版と同じ言葉）。
                    Button {
                        do { _ = try store.toStencil(note) }
                        catch { store.trouble = error.localizedDescription }
                    } label: {
                        Label("このノートをテンプレートにする", systemImage: "doc.on.doc")
                    }
                    Divider()
                    // 手が伸びる順に並べる。お気に入りはついでにやること、
                    // 移動は整理、書き出しは amber の外へ出ること ── 出ていくものは
                    // 常に最後。
                    Button { shelving = note } label: {
                        Label(note.star == nil ? "ブックマークに登録する" : "ブックマークグループを変える", systemImage: "star")
                    }
                    // このノートの隣だけでなく、すべてのノートブックを出す ──
                    // 整理とはたいてい*よそへ*しまうこと。
                    Menu("フォルダへ移動") {
                        Button("（トップページ）") { moveTo(note, nil) }
                        ForEach(store.allBooks, id: \.self) { b in
                            Button(b) { moveTo(note, b) }
                        }
                    }
                    // ノート 1 件を、どこへでも ── ファイル、Drive、Dropbox、
                    // メール。OS のシートが全部やってくれるので、amber がそれらを
                    // 名前で知っている必要は無い。
                    ShareLink(item: URL(fileURLWithPath: note.path)) {
                        Label("エクスポート", systemImage: "square.and.arrow.up")
                    }
                    // 共有のフォルダへ出し入れする。**移すこと以上のことは
                    // しない** ── 前書きに書くと、共有をやめた日に全部の
                    // ノートを書き換えることになる（同期先で全部が差分）。
                    Button {
                        // やめるときは、**もといたフォルダへ戻す**（デスクトップ版と
                        // 同じ）── いちばん上へ返していたので、フォルダに
                        // 分けている人ほど「どこへ行った」になっていた。
                        if note.shared { unshare(note) }
                        else if let sh = store.shares.first { share(note, sh.at) }
                        else { sharing = "グループ" }   // フォルダが無ければ、作るところから
                    } label: {
                        Label(note.shared ? unshareWords(note) : "グループと共有する",
                              systemImage: "person.2")
                    }
                    // **長押しから履歴へ。** デスクトップ版は右押しで開く ── iPhone に
                    // 右押しは無いので、同じ意味の手ぶりに割り当てる。
                    Button { past = .init(at: note.path, book: false) } label: {
                        Label("過去バージョン", systemImage: "clock.arrow.circlepath")
                    }
                    // **長押しからも消せる。** 消し方は横払いしか無く、
                    // 「長押しのメニューに無い＝消せない」と読める（実際に
                    // そう読まれた）。デスクトップ版のメニューにも入っているもの。
                    Divider()
                    Button(role: .destructive) { dropping2 = note } label: {
                        Label("ゴミ箱へ入れる", systemImage: "trash")
                    }
                }
    }

    /// どこかにドロップされたノート。`nil` は最上位。
    ///
    /// 何か動いたかを返す。端末はそれを見て、ドロップのアニメーションを
    /// 続けるか、行を元に戻すかを決める。
    private func drop(_ paths: [String], into book: String?) -> Bool {
        var moved = false
        for path in paths {
            guard let note = store.notes.first(where: { $0.path == path }) else { continue }
            do { try store.move(note, to: book); moved = true }
            catch { store.trouble = error.localizedDescription }
        }
        into = nil
        outside = false
        return moved
    }

    /// 通知が指していたノートを、分かった時点で開く。
    private func answer() {
        guard let want = ring.wanted else { return }
        guard let note = store.notes.first(where: { $0.path == want }) else { return }
        ring.wanted = nil
        desk.open(note, store)
        showing = true
    }

    private func moveTo(_ note: Note, _ book: String?) {
        do { try store.move(note, to: book) } catch { store.trouble = error.localizedDescription }
    }

    private func share(_ note: Note, _ book: String) {
        do { try store.share(note, to: book) } catch { store.trouble = error.localizedDescription }
    }

    /// フォルダの錠（依頼 629）。**目印を置いたフォルダごと**かける／やめる。
    private func setBookLock(_ book: String, on: Bool) {
        let at = URL(fileURLWithPath: store.rootPath).appendingPathComponent(book).path
        do {
            try store.setLock(path: at, on: on)
            store.reload()
        } catch {
            store.trouble = error.localizedDescription
        }
    }

    /// 共有のフォルダを「ファイル」で開く ── クラウド側でグループに分けるのは、人がやる。
    private func invite(_ book: String) {
        let at = URL(fileURLWithPath: store.rootPath).appendingPathComponent(book).path
        guard let enc = at.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed),
              let url = URL(string: "shareddocuments://" + enc) else { return }
        UIApplication.shared.open(url)
    }

    private func unshare(_ note: Note) {
        do { try store.unshare(note) } catch { store.trouble = error.localizedDescription }
    }

    /// **押す前に、どこへ戻るかを言う。** 「やめる」とだけ出しておいて別の
    /// フォルダへ入るのは、黙って動かすのと同じ。
    private func unshareWords(_ note: Note) -> String {
        guard let home = store.home(of: note) else { return "グループとの共有をやめる" }
        return "共有をやめて「\(home.split(separator: "/").last.map(String.init) ?? home)」へ戻す"
    }

    private func make(_ title: String, _ tags: [String]) -> Note? {
        do { return try store.make(titled: title, tags: tags) }
        catch { store.trouble = error.localizedDescription; return nil }
    }

    private var empty: some View {
        ContentUnavailableView {
            Label("ノートの保存場所", systemImage: "folder.badge.questionmark")
        } description: {
            // 「フォルダを選ぶ」ではなく、そう名乗らせる ── 要点は、
            // Mac が既に持っているフォルダをそのまま指せること。
            Text("マークダウンのノートがあるフォルダを選びます。iCloud Drive・Google Drive・Dropbox のどれでも構いません。")
        } actions: {
            Button("保存場所を見る") { picking = true }.buttonStyle(.borderedProminent)
        }
    }

    /// クラウドの置き土産を言う一段。
    @ViewBuilder
    private func band(_ tint: Color, _ head: String, _ body: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(head).font(.footnote.weight(.semibold)).foregroundStyle(tint)
            Text(body).font(.caption2).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 12).padding(.vertical, 8)
        .background(RoundedRectangle(cornerRadius: 10).fill(tint.opacity(0.12)))
        .listRowInsets(EdgeInsets(top: 2, leading: 16, bottom: 2, trailing: 16))
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    /// 移動し、それが動いて見えるようにする。
    ///
    /// **瞬時に変わる一覧は、変わらなかった一覧に見える。** 指が触れた
    /// ときには中身が既に別物で、目が追うものが何も無い ── だから
    /// 効いたかどうか確かめるために、もう一度押すことになる。
    /// どちらへ滑るかが、どちらへ進んだかを言う。両方向で同じ向きに
    /// 滑らせるなら、滑らせないほうがまし。
    private func go(_ act: () -> Void) {
        let was = store.at
        act()
        guard store.at != was else { return }
        deeper = store.at.count > was.count
        withAnimation(.easeOut(duration: 0.24)) { walked = store.at }
    }

    /// 共有のフォルダを除いた、自分だけのフォルダ。
    ///
    /// **二つの場所に同じものを出さない** ── 共有のぶんは上の「共有」の段に
    /// 並ぶ（ブックマークを別枠にしたのと同じ理由）。一か所で数えないと、
    /// 段が空なのに見出しだけ残る（実際に残った）。
    private var ownBooks: [(name: String, path: String, count: Int)] {
        store.books.filter { b in
            !store.shares.contains { !$0.at.isEmpty
                && (b.path == $0.at || b.path.hasPrefix($0.at + "/")) }
        }
    }

    /// いまツリー表示を描いているかどうか。
    ///
    /// 答えは 1 つで、訊く場所は 4 つ ── フォルダの行、お気に入り、
    /// ツリー本体、見出しの帯。すべてが一致していなければならず、同じ条件を
    /// 4 か所に書けば、そのうち 3 か所だけが一致する。
    private var treeing2: Bool {
        store.tree && needle.isEmpty && !store.narrowing && !store.flat
    }

    private var list: some View {
        List {
            if needle.isEmpty {
                // **並びは本人が決めた**（2026-09-12・依頼 505）: 探す（題の下の帯）→
                // タグ／フォルダ／期間 → 同期の様子 → カレンダー → 新しいノート →
                // すべてのノート → ブックマーク → フォルダ。**デスクトップ版の左の列と同じ順**
                // （依頼 247）── デスクトップ版もカレンダーをいちばん上にした。
                Section {
                    if store.at.isEmpty {
                        // 絞り込みの帯（デスクトップ版と同じ3 つの引き出し）と、並び順。
                        HStack(alignment: .center, spacing: 10) {
                            Sifting(store: store, open: $sifting)
                            Spacer(minLength: 0)
                        Menu {
                            Picker("並び", selection: $store.order) {
                                ForEach(NotesStore.Order.allCases) { Text($0.label).tag($0) }
                            }
                            Divider()
                            Toggle(isOn: $store.tree) {
                                Label("フォルダごと（ツリー）", systemImage: "list.bullet.indent")
                            }
                        } label: {
                            Text("並び順").font(.subheadline)
                        }
                        }
                        // **右の余白は、何でもない場所**（依頼 270）── 段のどこを触っても
                        // 最初のボタンが鳴らないように、段そのものに当たり判定を敷く。
                        .contentShape(Rectangle())
                        .onTapGesture {}
                        .listRowInsets(EdgeInsets(top: 4, leading: 16, bottom: 4, trailing: 16))
                        .listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                        if let which = sifting {
                            Sifted(store: store, which: which)
                                .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 8, trailing: 16))
                                .listRowSeparator(.hidden)
                        }
                        // **同期の様子は一覧の頭に**（依頼 500・デスクトップ版と同じ場所）。
                        SyncLine()
                            .listRowBackground(Color.clear)
                            .listRowSeparator(.hidden)
                            .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 4, trailing: 16))
                        // **クラウドの置き土産。** 黙って足りない一覧を見せない
                        // ── 落ちてきていないノートも、同時に書いた控えも、
                        // amber の側では直せないが、言わないと「ノートが消えた」
                        // にしか見えない（デスクトップ版と同じ言い方）。
                    if !store.waiting.isEmpty {
                        band(.blue, "\(store.waiting.count) 件、まだ落ちてきていません",
                             store.waiting.prefix(3).joined(separator: "・")
                             + (store.waiting.count > 3 ? " ほか" : "")
                             + " ── クラウドが中身をまだ持ってきていないだけで、消えてはいません")
                    }
                    if !store.clashes.isEmpty {
                        band(.orange, "\(store.clashes.count) 件、同時に更新されたコピーがあります",
                             store.clashes.prefix(3).map {
                                 ($0.clash?.of ?? "") + (($0.clash?.by.isEmpty == false)
                                     ? "（\($0.clash!.by)）" : "")
                             }.joined(separator: "・")
                             + (store.clashes.count > 3 ? " ほか" : "")
                             + " ── クラウドが作ったもの。中身を見比べて、どちらにするか決めてください")
                    }
                        // **カレンダー**（依頼 454）── デスクトップ版の左の列でもいちばん上。
                        // **三つの段は同じ大きさ・同じ色**（本人・2026-09-12）── マークは 26pt の
                        // 枠にアクセント色、文字は 16pt の太め。「新しいノート」と揃える。
                        Button { showCal = true } label: {
                            HStack(spacing: 10) {
                                Image(systemName: "calendar")
                                    .font(.system(size: 18, weight: .semibold))
                                    .foregroundStyle(Color.accentColor)
                                    .frame(width: 26, height: 26)
                                Text("カレンダー").font(.system(size: 16, weight: .semibold))
                                Spacer(minLength: 0)
                            }
                        }
                        .buttonStyle(.plain)
                        .listRowInsets(EdgeInsets(top: 2, leading: 16, bottom: 0, trailing: 16))
                        // **一つだけの、押させたいボタン。** デスクトップ版と同じ形 ── 塊に
                        // せず、琥珀は丸だけに残す。
                        Button { naming = true } label: {
                            HStack(spacing: 10) {
                                ZStack {
                                    Circle().fill(Color.accentColor)
                                    Image(systemName: "plus")
                                        .font(.system(size: 12, weight: .bold))
                                        .foregroundStyle(.white)
                                }
                                .frame(width: 26, height: 26)
                                .shadow(color: Color.accentColor.opacity(0.45), radius: 3, y: 1)
                                Text("新しいノート").font(.system(size: 16, weight: .semibold))
                                Spacer(minLength: 0)
                            }
                        }
                        .buttonStyle(.plain)
                        .listRowInsets(EdgeInsets(top: 2, leading: 16, bottom: 0, trailing: 16))
                        Button { store.flat.toggle() } label: {
                            HStack(spacing: 10) {
                                Image(systemName: store.flat ? "tray.full.fill" : "tray.full")
                                    .font(.system(size: 18, weight: .semibold))
                                    .foregroundStyle(Color.accentColor)
                                    .frame(width: 26, height: 26)
                                Text("すべてのノート").font(.system(size: 16, weight: .semibold))
                                Spacer(minLength: 0)
                                Text("\(store.notes.count)")
                                    .foregroundStyle(.secondary).monospacedDigit()
                            }
                        }
                        .buttonStyle(.plain)
                        .foregroundStyle(store.flat ? AnyShapeStyle(.tint) : AnyShapeStyle(.primary))
                        .listRowInsets(EdgeInsets(top: 2, leading: 16, bottom: 0, trailing: 16))
                    } else {
                        // フォルダの名前だけでは、それがどこにあるかを言えない。
                        // 「2026」という 2 つのフォルダは
                        // top of a list.
                        Crumbs(at: store.at, root: store.rootName) { to in
                            go { store.leave(for: to) }
                        }
                        .listRowInsets(EdgeInsets(top: 2, leading: 16, bottom: 8, trailing: 16))
                    }
                }
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
            }
            // ブックマーク。**デスクトップ版と同じ名前**（依頼 212 で「お気に入り」から
            // 改名した ── 「棚」も含めて、何のことか画面が説明して
            // いなかった）。ここだけ古い名前のままだと、同じものが
            // 二つの amber で違う名前で呼ばれる。
            // ピン留めしたノートを、それが何をした結果かを言う見出しの下に出す。
            // 黙って先頭へ飛ぶノートは、理由の見えない移動を
            // したノートになる。
            let stuck = treeing2 ? [] : store.pinnedHere(needle)
            // **一つも無くても段は出す**（デスクトップ版と同じ ── 依頼で「一つも無い
            // ときに段ごと消えると、最初の一つを作るパスがどこにも無くなる」）。
            // 「実装されていないのか、まだ無いだけなのか」は使う人には
            // 見分けられない。
            if !stuck.isEmpty || (needle.isEmpty && store.at.isEmpty && !store.flat) {
                Section {
                    if stuck.isEmpty {
                        // **どうすれば登録できるかを言う。** 「まだありません」だけでは、
                        // 登録するパスが画面のどこにも書いていない（本人の指摘・
                        // 2026-09-08）。横払いは長押しより手数が少ないので、
                        // そちらを先に言う。
                        Text("ブックマークには何も登録されていません。ノートを右へ払って ★ を押すと登録できます。")
                            .font(.footnote).foregroundStyle(.secondary)
                    }
                    ForEach(stuck) { row($0) }
                } header: {
                    HStack {
                        Label {
                            Text("ブックマーク")
                        } icon: {
                            Image(systemName: "star.fill").foregroundStyle(.orange)
                        }
                        Spacer()
                        NavigationLink("すべて見る") {
                            Stars(store: store) { note in
                                desk.open(note, store)
                                showing = true
                            }
                        }
                        .font(.caption)
                        .textCase(nil)
                    }
                }
            }
            // **共有は、行き先の一つ。** 分けるのはクラウドの仕事で、amber が
            // 持つのは「どれが分けてあるか」だけ ── 新しい仕組みではなく、
            // フォルダの一つを別の名前で呼んでいるだけ。
            //
            // **決めていないうちは出さない** ── 空の「共有」が並ぶと、共有が
            // 壊れているのか、まだ何も分けていないのかが見分けられない。
            if needle.isEmpty, store.at.isEmpty, !store.flat, !store.shares.isEmpty {
                Section("共有") {
                    ForEach(store.shares) { sh in
                        Button { go { store.into(sh.at) } } label: {
                            HStack {
                                Label(sh.name, systemImage: "person.2")
                                Spacer()
                                Text("\(store.notes.filter { store.inShare(sh.at, $0) }.count)")
                                    .foregroundStyle(.secondary).monospacedDigit()
                                Image(systemName: "chevron.right")
                                    .font(.caption).foregroundStyle(.tertiary)
                            }
                        }
                        .buttonStyle(.plain)
                    }
                }
            }

            // 先にノートブック、次にこのフォルダのノート。フォルダが
            // ファイルより上なのは、最初のファイラ以来どれもそうしてきた
            // からで、ここも同じ操作 ── 入って、戻る。
            // 1 階層ずつの一覧。**ツリー表示のあいだは出さない** ──
            // 出すとフォルダもお気に入りも 2 回ずつ描かれる。最初に
            // まさにそうなって、2 つの View が一致しているのではなく
            // 一覧が壊れたように見えた。
            if !store.flat && needle.isEmpty && !treeing2 {
                // 見出しを付ける ── デスクトップ版の左の列がそう呼んでいる。
                // 出口であり、そこへ物を落とす場所でもある。amber の
                // ペインには最初から `..` の行があり、常にその両方を
                // 意味してきた ── 上へ行く、そして上へ置く。
                if let up = store.up {
                    Button { go { store.leave(for: up) } } label: {
                        Label("..", systemImage: "arrow.up.left")
                            .foregroundStyle(.tint)
                    }
                    .buttonStyle(.plain)
                    .dropDestination(for: String.self) { paths, _ in
                        drop(paths, into: up.isEmpty ? nil : up)
                    } isTargeted: { over in outside = over }
                    .listRowBackground(outside ? Color.accentColor.opacity(0.15) : nil)
                }
                Section {
                  // **保存ディレクトリが二つ以上なら、切り替えの一行**（依頼 511）── デスクトップ版は
                  // 左の列に親として並べるが、iPhone は一段ずつなので、フォルダの頭に一つ。
                  // 押すと並びが出て、選んだ保存ディレクトリのフォルダに替わる。
                  if store.many, store.at.isEmpty {
                      Menu {
                          ForEach(store.places) { p in
                              Button { go { store.enter(p.id) } } label: {
                                  if p.id == store.placeId {
                                      Label(p.name, systemImage: "checkmark")
                                  } else {
                                      Text(p.name + (store.placeTrouble[p.id] != nil ? "（見つかりません）" : ""))
                                  }
                              }
                          }
                      } label: {
                          HStack {
                              Label(store.rootName, systemImage: "tray.full.fill")
                              Spacer()
                              if store.place?.sync == "drive" {
                                  Text("Drive").font(.caption2.weight(.semibold)).foregroundStyle(.secondary)
                                      .padding(.horizontal, 5).padding(.vertical, 1)
                                      .overlay(RoundedRectangle(cornerRadius: 4).stroke(Color.secondary.opacity(0.4)))
                              }
                              Text("\(store.notes.filter(store.here).count)").foregroundStyle(.secondary).monospacedDigit()
                              Image(systemName: "chevron.up.chevron.down").font(.caption).foregroundStyle(.tertiary)
                          }
                          .contentShape(Rectangle())
                      }
                      .buttonStyle(.plain)
                  }
                  ForEach(ownBooks, id: \.path) { b in
                    Button {
                        go { store.into(b.path) }
                    } label: {
                        HStack {
                            Label {
                                HStack(spacing: 5) {
                                    Text(b.name)
                                    // 錠のフォルダは、一覧でそう見える（依頼 629）──
                                    // 長押しするまで分からないのでは、かけたことを忘れる。
                                    if store.isLocked(book: b.path) {
                                        Image(systemName: "lock.fill")
                                            .font(.caption2).foregroundStyle(.secondary)
                                    }
                                }
                            } icon: {
                                Image(systemName: "folder.fill")
                                    .foregroundStyle(store.colors[b.path].flatMap { Color(hex: $0) }
                                        .map { AnyShapeStyle($0) } ?? AnyShapeStyle(.tint))
                            }
                            Spacer()
                            Text("\(b.count)").foregroundStyle(.secondary).monospacedDigit()
                            Image(systemName: "chevron.right")
                                .font(.caption).foregroundStyle(.tertiary)
                        }
                    }
                    .buttonStyle(.plain)
                    // フォルダに落としたノートはその中に入る ── 画像もろとも。
                    // それは `note::move_to` の仕事で、ここの
                    // view's.
                    .dropDestination(for: String.self) { paths, _ in
                        drop(paths, into: b.path)
                    } isTargeted: { over in into = over ? b.path : nil }
                    .listRowBackground(into == b.path ? Color.accentColor.opacity(0.15) : nil)
                    .contextMenu {
                        // **下の階層は、ここから作る**（デスクトップ版と同じ）── 名前に「/」を
                        // 打たせるのは、書き方を知っている人にしか通じない。
                        Button { making = b.path } label: {
                            Label("この中にフォルダを作る", systemImage: "folder.badge.plus")
                        }
                        Button { colouring = b.path } label: {
                            Label("フォルダに色をつける", systemImage: "paintpalette")
                        }
                        // 錠（依頼 629・本人「スマホ版でもロックできるように
                        // して欲しいぞ」）── **中のノートとサブフォルダぜんぶ。**
                        if let at = store.lockRoot(of: b.path) {
                            Button { setBookLock(at, on: false) } label: {
                                Label(at == b.path ? "このフォルダのロックをやめる"
                                      : "「\((at as NSString).lastPathComponent)」のロックをやめる",
                                      systemImage: "lock.open")
                            }
                        } else {
                            Button { setBookLock(b.path, on: true) } label: {
                                Label("このフォルダをロックする", systemImage: "lock")
                            }
                        }
                        if store.shares.contains(where: { $0.at == b.path }) {
                            // 分けるのはクラウドの仕事 ── 「ファイル」でそのフォルダを開く。
                            Button { invite(b.path) } label: {
                                Label("グループへ招待", systemImage: "person.badge.plus")
                            }
                        }
                        // **分けるのはクラウドの仕事。** amber が憶えるのは
                        // 「どれが分けてあるか」の一言だけ ── そのうえで
                        // このフォルダを、クラウド側でグループの人に共有してもらう。
                        Button {
                            sharing = b.path
                        } label: {
                            Label("グループと共有するフォルダにする", systemImage: "person.2")
                        }
                        // フォルダの履歴は、**中のノートの姿をまとめて** ──
                        // 「あのあたりで壊した」は、どのノートかを覚えて
                        // いないほうが多い。
                        Button {
                            past = .init(at: store.rootPath + "/" + b.path, book: true)
                        } label: {
                            Label("過去バージョン", systemImage: "clock.arrow.circlepath")
                        }
                        Button {
                            renaming = b.path
                            fresh = b.name
                        } label: {
                            Label("名前を変える", systemImage: "pencil")
                        }
                        Button(role: .destructive) { dropping = b.path } label: {
                            Label("このフォルダを削除", systemImage: "trash")
                        }
                    }
                  }
                } header: {
                    // 中に入っているときは、上の帯が既にどこかを言っている。
                    // **フォルダのことは、この印一つに**（依頼 269）── 押すとフォルダの
                    // 構成が出て、そこで選ぶ／作る（名前をいきなり打たせない）。
                    // 置き場所は「フォルダ」の見出しの右（本人・2026-09-12・依頼 504
                    // 「上の帯ではパッと探せなかった」）。
                    if store.at.isEmpty {
                        HStack {
                            Text("フォルダ")
                            Spacer()
                            Button { treeing = true } label: {
                                Image(systemName: "folder.badge.plus").font(.body)
                            }
                            .buttonStyle(.borderless)
                            .accessibilityLabel("フォルダを作る・選ぶ")
                        }
                    }
                }
                if store.at.isEmpty, ownBooks.isEmpty {
                    Text("まだありません（上の「フォルダ」から作れます）")
                        .font(.footnote).foregroundStyle(.secondary)
                }
            }

            // **タグの段は、絞り込みの帯へ移した。**
            //
            // 一覧の中にタグの段があり、その上に「タグ ▾」の引き出しもある
            // ── 同じことを頼むパスが二つあると、片方を直した日にもう片方が
            // 古いまま残る。段のほうを畳んだのは、**押すと一覧じゅうが
            // 動く**からでもある（絞った瞬間に段が消え、次に押した指が
            // 別の行に当たった）。
            // **探していないときはツリー。** 検索結果のツリーは誰も
            // 読める形ではない ── 絞り込んでいるときに欲しいのは
            // いちばん短い答えの並びで、そうでないときに欲しいのは
            // 何がどこにあるかを見ることだから。
            if treeing2 {
                Nest(store: store, open: { note in
                    desk.open(note, store)
                    showing = true
                }, row: { note in AnyView(row(note)) })
            } else {
                // 並び順に沿った見出しの下に置く ── `bands` を見よ。
                ForEach(store.bands(store.matching(needle))) { band in
                    Section(band.name) {
                        ForEach(band.notes) { row($0) }
                    }
                }
            }
        }
        // 段のあいだを詰める。既定のままだと、ボタンと絞りと見出しだけで
        // 画面の三分の一が空き、ノートが下に押し出される。
        .listSectionSpacing(.compact)
        // 探す欄のすぐ下から始める ── 一覧が自分で取る上の余白は、
        // 大きな題があった頃のためのもので、いまは何も置いていない。
        .contentMargins(.top, 2, for: .scrollContent)
        // フォルダが変わると一覧ごと差し替える。来た側から
        // 滑り込ませるため。
        .id(walked)
        .transition(.asymmetric(
            insertion: .move(edge: deeper ? .trailing : .leading).combined(with: .opacity),
            removal: .move(edge: deeper ? .leading : .trailing).combined(with: .opacity)
        ))
        // **端からだけ。** 画面全体の横スワイプは行自身の操作 ──
        // ノートに星を付けたり消したりするのがそれ ── なので
        // 出口は端末が既に置いている場所に置く。排他ではなく同時に
        // してあるのは、スクロールが勝たなければならないから。ここが
        // 何かを決めるのは、指が離れたあとだけ。
        .simultaneousGesture(
            DragGesture(minimumDistance: 12).onEnded { g in
                // 素早い動きは、実際の距離より多く数える ── 画面から離れる
                // 瞬間もまだ動いていた指は、もっと先へ行くつもりだった。
                // これが無いと、画面の 3 分の 1 をゆっくり引きずる操作に
                // なり、それはスワイプではない。
                let went = max(abs(g.translation.width), abs(g.predictedEndTranslation.width) * 0.6)
                guard went > abs(g.translation.height) * 1.4, went > 22 else { return }
                if g.startLocation.x < 36, g.translation.width > 0, let up = store.up {
                    go { store.leave(for: up) }
                } else if g.startLocation.x > wide - 36, g.translation.width < 0 {
                    // **右から左は「開いていたノートへ戻る」。**
                    //
                    // 一覧から左へ払うのは、ノートを開くときと同じ向きの
                    // 手ぶり ── 開いていた一本があるなら、そこへ戻るのが
                    // 素直。フォルダを一つ潜っただけの「進む」より、
                    // 待っているノートのほうを先に見る。
                    if !desk.showing.isEmpty && desk.current != nil {
                        showing = true
                    } else {
                        go { _ = store.back() }
                    }
                }
            }
        )
        .background {
            GeometryReader { geo in
                Color.clear.onChange(of: geo.size.width, initial: true) { _, w in wide = w }
            }
        }
        // こちら以外の理由で移動することもある ── フォルダの復元や、
        // ノートからの戻り。滑らせるのは自分でした移動のためで、
        // そうでないときに両者の辻褄を合わせる。
        .onChange(of: store.at, initial: true) { _, now in if walked != now { walked = now } }
        // **畳んでおく。** 絞り込みの帯と並べて置きっぱなしにすると、一覧の
        // 頭が毎回二段ぶん要る ── 言葉で探すのは、絞るより回数が少ない
        // （デスクトップ版も同じ形にした）。「ノートを探す」のボタンから開く。
        .modifier(Seeking(needle: $needle, on: $seeking, tags: store.tagsHere))
        .onChange(of: needle) { _, now in
            store.read(now)
            store.find(now)
        }
        .refreshable { store.reload() }
        .modifier(Waking(store: store, desk: desk))
        // 開いているノートごとに 1 画面、その上にタブ。
        .navigationDestination(isPresented: $showing) { DeskView(desk: desk, store: store) }
        // 作ったばかりのノートをそのまま開く。しかも**編集側で** ──
        // 書くために作ったのだから。
        //
        // **スタックの中に置く。スタック自体ではない。** `NavigationStack`
        // に付けると何も起きない ── ノートはでき、シートは閉じ、
        // 一覧はそのまま座っていた。

    }
}

/// フォルダのパス。それに対してシートを出せるようにするため。
///
/// `String` は `Identifiable` ではないし、アプリ全体でそうすべきでもない ──
/// 必要なのはここだけで、しかもシートのためだけ。
extension String: @retroactive Identifiable {
    public var id: String { self }
}

/// 探す欄を、**押したときだけ**出す。
///
/// `isPresented:` だけでは畳めない ── あの糸が決めるのは「いま打っている
/// か」であって、欄が場所を取るかどうかではない。`.navigationBarDrawer` は
/// 一覧の頭に居座り、`displayMode: .automatic` にしても**上まで戻れば必ず
/// 出てくる**。そこに自前の「ノートを探す」ボタンも並ぶと、同じことを頼む入口が
/// 二つになる（このデスクトップ版がいちばん嫌う形）。
///
/// なので `searchable` そのものを付け外しする。閉じるとき（取り消しを押した
/// とき）は `on` が false になり、この修飾ごと消える。
/// **戻ってきたら、読み直す**（依頼 479）。
///
/// 同じフォルダをデスクトップ版と iPhone が触るので、iPhone を置いているあいだに向こうで
/// 書かれる ── 読み直さないと、開いた瞬間の一覧が古いまま出る。
/// **「引き下げれば来る」では気づけない** ── そこに新しいものがあると
/// 知らない人は、引き下げようと思わない。
///
/// 直しかけを抱えているときは触らない ── 打っている文字を下から
/// 書き換えない。
///
/// **一覧の本体には足さない。** あの `body` はもう型を追いきれる限界に
/// 居て、一行足すだけで組めなくなる（`Seeking` と同じ理由でここに出す）。
struct Waking: ViewModifier {
    let store: NotesStore
    @ObservedObject var desk: Desk

    /// **`scenePhase` ではなく、報せを受ける。** `onChange(of:)` は
    /// 新旧どちらの形に解けるかが呼ぶ場所で変わり、引数の数で組めたり
    /// 組めなかったりする ── 前に出るという一点だけが要るので、
    /// それだけを言ってくる報せを直に受ける。
    func body(content: Content) -> some View {
        content.onReceive(
            NotificationCenter.default
                .publisher(for: UIApplication.willEnterForegroundNotification)
        ) { _ in
            // **打ちかけを抱えているときは触らない。** 一覧を読み直すと
            // 開いている札まで組み直されて、打った文字が消えることがある。
            if desk.tabs.contains(where: { $0.dirty }) { return }
            store.reload()
            Task { await Syncing.shared.now("戻った") }
        }
    }
}

struct Seeking: ViewModifier {
    @Binding var needle: String
    @Binding var on: Bool
    let tags: [String]

    func body(content: Content) -> some View {
        // **探す欄はいつも出す**（本人・2026-09-12）── 前は押してから出していた。
        content
                .searchable(text: $needle, isPresented: $on,
                            placement: .navigationBarDrawer(displayMode: .always),
                            prompt: "ノートを探す")
                // **タグは、打っているところで差し出す。** 帯にすると一段を
                // 永久に取るのに、押されるのは時々だけ。
                .searchSuggestions {
                    if needle.trimmingCharacters(in: .whitespaces).isEmpty {
                        ForEach(tags.prefix(20), id: \.self) { t in
                            Label("#\(t)", systemImage: "tag").searchCompletion("#\(t)")
                        }
                    }
                }
    }
}

/// 一覧の頭の一行 ── 「● 同期しています ・ 最終 hh:mm ・ メールアドレス」。
/// 困っているときは赤く、その言い分と「接続確認する」。始める前は灰色で
/// 「同期していません」（始めるのは設定から）。
struct SyncLine: View {
    @ObservedObject private var sync = Syncing.shared

    var body: some View {
        if sync.store?.places.contains(where: { $0.sync == "drive" }) == false {
            // **どの保存ディレクトリも「同期しない」なら、灰色の一行**（依頼 511・デスクトップ版と同じ）。
            HStack(spacing: 6) {
                Circle().fill(Color.secondary).frame(width: 7, height: 7)
                Text("同期していません ・ どの保存ディレクトリも「同期しない」").font(.footnote).foregroundStyle(.secondary)
            }
        } else if !sync.signedIn, !sync.later {
            // **始める前は色つきの列**（デスクトップ版の `before` と同じ・本人が決めた案甲）。
            VStack(alignment: .leading, spacing: 4) {
                Text("まだ同期していません").font(.footnote.weight(.semibold)).foregroundStyle(Color("AccentColor"))
                Text("ノートはこの iPhone だけにあります。ほかの端末やグループの人と同じノートを使うには、Google でサインインします。")
                    .font(.footnote).foregroundStyle(.secondary)
                HStack(spacing: 8) {
                    Button("同期をはじめる") { Task { _ = try? await sync.signIn() } }
                        .buttonStyle(.borderedProminent).controlSize(.small)
                    Button("あとで") { sync.later = true }.controlSize(.small)
                }
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color("AccentColor").opacity(0.12), in: RoundedRectangle(cornerRadius: 10))
        } else if !sync.signedIn {
            HStack(spacing: 6) {
                Circle().fill(Color.secondary).frame(width: 7, height: 7)
                Text("同期していません ・ ").font(.footnote).foregroundStyle(.secondary)
                // デスクトップ版と同じく、押せるボタンを（本人・2026-09-12）。
                Button("同期をはじめる") { Task { _ = try? await sync.signIn() } }.font(.footnote.weight(.semibold))
            }
        } else if !sync.trouble.isEmpty {
            VStack(alignment: .leading, spacing: 3) {
                Text("同期できません" + (sync.troubleSince.map { " ── " + Syncing.hhmm($0) + " から" } ?? ""))
                    .font(.footnote.weight(.semibold)).foregroundStyle(Color(red: 0.72, green: 0.26, blue: 0.23))
                Text(sync.troubleFace.text).font(.footnote).foregroundStyle(.secondary)
                if sync.troubleFace.again {
                    Button(sync.troubleFace.button) { Task { await sync.now("手") } }
                        .font(.footnote.weight(.semibold))
                }
            }
        } else if let fresh = sync.fresh {
            VStack(alignment: .leading, spacing: 2) {
                Text("同期しました" + (sync.last.map { " ── " + Syncing.hhmm($0) } ?? ""))
                    .font(.footnote.weight(.semibold)).foregroundStyle(Color(red: 0.25, green: 0.49, blue: 0.32))
                Text(sync.freshWords(fresh)).font(.footnote).foregroundStyle(.secondary)
            }
        } else {
            HStack(spacing: 6) {
                Circle().fill(sync.busy ? Color("AccentColor") : Color(red: 0.25, green: 0.49, blue: 0.32)).frame(width: 7, height: 7)
                Text(sync.line).font(.footnote).foregroundStyle(.secondary).lineLimit(1)
            }
        }
    }
}
