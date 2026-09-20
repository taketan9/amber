import SwiftUI
import UniformTypeIdentifiers

/// Where the notes are kept, and how they get in and out.
///
/// The folder button used to open the system file picker straight away, which
/// answers a question nobody asked: it looks like "find a file" when what it
/// is for is "this is where my notes live, and I might move them". This says
/// where they are first, and offers the picker as one of the things you can
/// do about it.
struct Where: View {
    @ObservedObject var store: NotesStore
    /// Asked for after this sheet has closed.
    ///
    /// **A file picker cannot be opened from inside a sheet** — it is a
    /// presentation over a presentation, and it silently does nothing, which
    /// is exactly how 「保存場所を選ぶ」 behaved: pressed, and no answer at
    /// all. So the sheet closes first and the screen underneath opens it.
    /// 保存ディレクトリの場所を選ぶ（`nil` は「足す」、それ以外はその id の場所を変える）。
    let choose: (String?) -> Void
    let bringIn: () -> Void
    /// ほかの場所の .md を一時的に開く（依頼 517・パソコン版の ⌘O）。
    let openOutside: () -> Void
    let restore: () -> Void
    /// 取ってきて、一本のノートにする（依頼 421 の乙）。
    ///
    /// **題はページのもの、中身は本文だけ、出どころは本文の最後に文字で。**
    /// 判断はぜんぶ `Clipping` と core にあり、ここは繋ぐだけ。
    private func clip() {
        guard let url = Clipping.reach(clipUrl) else {
            trouble = "URL の形になっていません"
            return
        }
        clipBusy = true
        Task { @MainActor in
            defer { clipBusy = false }
            let hand = Clipping()
            await hand.warm()
            do {
                let got = try await hand.clip(url)
                let name = got.title.isEmpty ? url.host ?? "インポート" : got.title
                guard let made = try store.make(titled: name) else {
                    trouble = "ノートを作れません"
                    return
                }
                let read = try Cian.call("read", ["path": made.path])
                let head = try store.split(read["text"] as? String ?? "").0
                _ = try store.save(made, text: head + "\n" + got.body,
                                   stamp: read["stamp"] as? String ?? "")
                store.reload()
                clipDone = name
            } catch {
                trouble = error.localizedDescription
            }
        }
    }

    /// **URL を訊く紙**（依頼 442 の丙）。
    ///
    /// ダイアログ（`alert`）ではなく紙にしてあるのは、**貼り付けのボタンを置けるのが
    /// 紙だけ**だから。クリップボードを amber から覗くと iOS が毎回
    /// 「ペーストしてよいか」と訊いてくるが、**人が貼り付けのボタンを押した
    /// こと自体がその答えになる**ので、訊かれない（iOS がそのために
    /// 用意したボタン）。
    ///
    /// ボタンに出る文字（「ペースト」）は iOS が決めるもので、amber からは
    /// 変えられない ── なので、何をするボタンかは下の一行で言う。
    @ViewBuilder private var clipSheet: some View {
        NavigationStack {
            Form {
                Section("URL を入力する") {
                    TextField("https://…", text: $clipUrl)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                }
                Section {
                    PasteButton(payloadType: String.self) { got in
                        guard let text = got.first,
                              let url = Clipping.reach(text) else { return }
                        clipUrl = url.absoluteString
                    }
                    .labelStyle(.titleAndIcon)
                    .frame(maxWidth: .infinity)
                } footer: {
                    Text("クリップボードの URL を貼り付ける")
                }
                Section {
                    Text("ページの本文だけを、一本のノートにします。")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Web からインポート")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("やめる") { clipping = false }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("インポート") { clipping = false; clip() }
                        // **形になっていない間は押させない。** 押してから
                        // 「URL の形になっていません」と言うより早い。
                        .disabled(Clipping.reach(clipUrl) == nil)
                }
            }
        }
        .presentationDetents([.medium])
    }

    /// 名前を `sheet(item:)` に渡すための包み。
    struct Named: Identifiable {
        let name: String
        var id: String { name }
    }

    /// サンプルを何枚置いたか。**言わないと、押しても何も起きなかったように
    /// 見える**（既にあるものは飛ばすので、本当に何も起きない回がある）。
    @State private var added: Int?
    @Environment(\.dismiss) private var dismiss
    @State private var zip: URL?
    @State private var trouble: String?
    /// Web から取り込むときの、URL と最中かどうか（依頼 421 の乙）。
    @State private var clipping = false
    @State private var clipUrl = ""
    @State private var clipBusy = false
    @State private var clipDone: String?
    /// 使われていない画像の片づけ（依頼 449）。
    @State private var sparing = false
    /// よその予定表の出し入れ（依頼 456）。
    @State private var feeding = false
    @AppStorage("cian.look") private var look = Look.auto
    @AppStorage("amber.palette") private var palette = ""
    @ObservedObject private var sync = Syncing.shared
    @State private var signingIn = false
    @State private var signInSaid: String?

    /// デスクトップ版の `THEMES` と同じ鍵 ── 空は琥珀（iPhone に合わせる）、`amber-light`／`amber-dark`
    /// は琥珀の明暗、それ以外は配色の名前。
    private var themeKey: Binding<String> {
        Binding(
            get: { palette.isEmpty ? (look == .light ? "amber-light" : look == .dark ? "amber-dark" : "") : palette },
            set: { key in
                switch key {
                case "": palette = ""; look = .auto
                case "amber-light": palette = ""; look = .light
                case "amber-dark": palette = ""; look = .dark
                default: palette = key
                }
            }
        )
    }
    @AppStorage("amber.font") private var font = Size.system
    @AppStorage("cian.autosave") private var autosave = true

    var body: some View {
        NavigationStack {
            List {
                // **保存ディレクトリ**（依頼 511・デスクトップ版の ⚙「保存ディレクトリの追加・変更・削除」と
                // 同じ三段）── 一覧 → 一つ → 同期先／名前／場所／外す。同期の入れる切るもここ。
                Section {
                    ForEach(store.places) { p in
                        NavigationLink {
                            PlaceSheet(store: store, id: p.id, choose: { dismiss(); choose(p.id) })
                        } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(p.name)
                                    Text(store.trail(of: p).joined(separator: " › ") + " ・ "
                                         + (p.sync == "drive" ? "Google Drive" : "同期しない")
                                         + (store.placeTrouble[p.id] != nil ? " ・ 見つかりません" : ""))
                                        .font(.caption2).foregroundStyle(.secondary).lineLimit(2)
                                }
                                Spacer()
                                Text("\(store.notes.filter { $0.root == (store.url(of: p)?.path ?? "\u{0}") }.count) 件")
                                    .font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                    Button {
                        dismiss()
                        choose(nil)
                    } label: {
                        Label("保存ディレクトリを追加", systemImage: "folder.badge.plus")
                    }
                } header: {
                    Text("保存ディレクトリの追加・変更・削除")
                } footer: {
                    // **The thing that is actually hard.** 2026-09-05:
                    // 「どこのディレクトリなのかが単純にわからないんだ。探せなくて困っている」。
                    // 提供者はみな選ぶ画面の何段か下に居て、どれも人が当たりを付ける場所に無い。
                    Text("ノートを置くフォルダ。いくつでも。同期先はフォルダごとに選べます（iCloud と OneDrive は、これから）。\n\n選ぶ画面が開いたら、左上の「ブラウズ」から辿ります。iCloud Drive はそのまま一覧に、Google Drive / Dropbox は「場所」の下。出てこないときは「…」→「サイドバーを編集」でオンに（「ファイル」アプリ側の設定）。パソコン版の ambər に同じフォルダを指定すれば、両方から同じノートを触れます。")
                }

                Section {
                    if sync.signedIn {
                        if let who = sync.who, !who.email.isEmpty {
                            LabeledContent("Google", value: who.email)
                        }
                        LabeledContent("最終", value: sync.last.map(Syncing.hhmm) ?? "まだ")
                        Button {
                            Task { await sync.now("手") }
                        } label: {
                            Label(sync.busy ? "同期しています…" : "いま同期する", systemImage: "arrow.triangle.2.circlepath")
                        }
                        .disabled(sync.busy)
                        Button(role: .destructive) {
                            Task { await sync.signOut() }
                        } label: {
                            Label("同期をやめる", systemImage: "xmark.circle")
                        }
                    } else {
                        Button {
                            signingIn = true
                            Task {
                                defer { signingIn = false }
                                do {
                                    let who = try await sync.signIn()
                                    signInSaid = "Google にサインインしました" + (who.email.isEmpty ? "" : "（" + who.email + "）")
                                } catch {
                                    signInSaid = "サインインできませんでした: " + error.localizedDescription
                                }
                            }
                        } label: {
                            Label(signingIn ? "ブラウザで「許可」を押してください…" : "Google でサインイン", systemImage: "person.crop.circle.badge.checkmark")
                        }
                        .disabled(signingIn)
                    }
                    if let said = signInSaid {
                        Text(said).font(.footnote).foregroundStyle(.secondary)
                    }
                } header: {
                    Text("同期")
                } footer: {
                    Text(sync.signedIn
                         ? "Mac と同じノートを Google Drive の「ambər」フォルダで使っています。保存の三秒後・三十秒ごと・この画面に戻ったときに同期します。"
                         : "Mac と iPhone で同じノートを使えるようにします。ambər が触れるのは、ambər が作ったファイルだけです。")
                }

                Section {
                    // **Web から取り込む**（依頼 421 の乙・デスクトップ版と同じ）。
                    //
                    // **押しただけでクリップボードを覗かない**（依頼 442）。
                    // 覗くと iOS が毎回「ペーストしてよいか」と訊いてきて、
                    // amber の言葉ではないダイアログが先に1 つ出る ── 貼るかどうかは
                    // 人が中で決める。
                    Button {
                        clipUrl = ""
                        clipping = true
                    } label: {
                        Label("Web からインポート", systemImage: "safari")
                    }
                    Button {
                        dismiss()
                        bringIn()
                    } label: {
                        Label("インポート", systemImage: "square.and.arrow.down")
                    }
                    Button {
                        dismiss()
                        openOutside()
                    } label: {
                        Label("ほかの場所のノートを開く", systemImage: "doc.badge.ellipsis")
                    }
                    // The other half of 「バックアップ」. Without it a zip is
                    // a thing you can make and never use, which is not a
                    // backup — it is a file.
                    Button {
                        dismiss()
                        restore()
                    } label: {
                        Label("バックアップから戻す", systemImage: "clock.arrow.circlepath")
                    }
                    // **片づけ**（依頼 449）── 貼ったノートを消しても、
                    // 画像は `attachments/` に残る。消すパスがどこにも
                    // 無かったので、フォルダだけが重くなっていた。
                    Button {
                        feeding = true
                    } label: {
                        Label("カレンダー設定追加", systemImage: "calendar.badge.plus")
                    }
                    Button {
                        sparing = true
                    } label: {
                        Label("使われていない画像", systemImage: "photo.badge.checkmark")
                    }
                    // The scope is a choice because backing up is
                    // something people do *before* something — before a
                    // reinstall, before handing a folder to somebody, before
                    // tidying. Each of those wants a different amount.
                    Menu {
                        Button("すべて") { make("all", "") }
                        if !store.allBooks.isEmpty {
                            Menu("フォルダ") {
                                ForEach(store.allBooks, id: \.self) { b in
                                    Button(b) { make("book", b) }
                                }
                            }
                        }
                        if !store.allTags.isEmpty {
                            Menu("タグ") {
                                ForEach(store.allTags, id: \.self) { t in
                                    Button("#\(t)") { make("tag", t) }
                                }
                            }
                        }
                    } label: {
                        Label("バックアップ", systemImage: "square.and.arrow.up")
                    }
                } header: {
                    Text("バックアップとインポート")
                } footer: {
                    Text("インポートした .md はこのフォルダにコピーされます。元のファイルはそのまま。同じ名前があるときは番号を付けて、いまあるノートは上書きしません。「不要添付削除」は、ノートから使われていない画像を小さく見て、選んでゴミ箱へ。")
                }

                Section {
                    Button {
                        let n = store.addWelcome()
                        added = n
                    } label: {
                        Label("サンプルのノートを入れる", systemImage: "sparkles")
                    }
                } footer: {
                    Text("Markdown の書き方・『覚悟の磨き方』・ストラテジーパターンの三枚を、いま見ているフォルダの直下に置きます（フォルダもタグも作りません）。同じ名前があるものは飛ばすので、二度押しても増えません。")
                }

                Section {
                    Toggle("自動保存", isOn: $autosave)
                } footer: {
                    Text("切ると、書く画面に「保存」が出ます。切っていても、画面を離れるときに一度だけ確認します。")
                }

                // **デスクトップ版の歯車にあって、iPhone に無かった二つ。**
                // 記号の書き方を知らない人がいちばん先に困るのがここで、
                // iPhone には帯のボタンしかパスが無かった。
                Section {
                    NavigationLink {
                        Syntax()
                    } label: {
                        Label("マークダウンの書き方", systemImage: "text.book.closed")
                    }
                    NavigationLink {
                        About(store: store)
                    } label: {
                        Label("ambər について", systemImage: "info.circle")
                    }
                } header: {
                    Text("困ったとき")
                }

                Section {
                    // **デスクトップ版の「テーマ」と同じ一つの表**（本人「文言をウィンドウ版に合わせて」・2026-09-12）
                    // ── 琥珀の3 つ（iPhone に合わせる・明るい・暗い）と、cian と同じ二十一。
                    // 琥珀を選ぶと `look`、配色を選ぶと `palette`（明暗はその配色に従う）。
                    Picker("テーマ", selection: themeKey) {
                        Text("琥珀 ── OS に合わせる").tag("")
                        Text("琥珀 ── 明るい").tag("amber-light")
                        Text("琥珀 ── 暗い").tag("amber-dark")
                        ForEach(Palettes.all, id: \.name) { Text($0.label).tag($0.name) }
                    }
                    // **文字の大きさは、デスクトップ版にもある**（⌘+ / ⌘−）── iPhone にだけ
                    // 無いと、同じノートが端末によって読みやすさで分かれる。
                    Picker("文字の大きさ", selection: $font) {
                        ForEach(Size.allCases) { Text($0.label).tag($0) }
                    }
                } header: {
                    Text("テーマ")
                } footer: {
                    // Three and not two: a phone that goes dark at sunset is
                    // the common case, and a switch with no way back to it
                    // is a switch that gets set once and regretted.
                    Text("「琥珀 ── OS に合わせる」は、OS がダークのとき一緒に暗くなります。")
                }
            }
            // The zip exists before the share sheet opens, so what is being
            // handed over is a file that is already there — not a promise.
            .sheet(item: $zip) { at in
                ActivityView(item: at)
            }
            .alert("サンプルのノート", isPresented: Binding(
                get: { added != nil }, set: { if !$0 { added = nil } }
            )) {
                Button("閉じる") {}
            } message: {
                Text(added == 0
                     ? "もう入っています（同じ名前のものは飛ばしました）。"
                     : "\(added ?? 0) 件置きました")
            }
            .alert(
                "できません",
                isPresented: Binding(get: { trouble != nil }, set: { if !$0 { trouble = nil } })
            ) {
                Button("閉じる") {}
            } message: {
                Text(trouble ?? "")
            }
            // **Web から取り込む**（依頼 421 の乙）。URL を一つ訊いて、
            // 取ってきて、一本のノートにする。
            .sheet(isPresented: $clipping) { clipSheet }
            .sheet(isPresented: $sparing) { Sparing(store: store) }
            .sheet(isPresented: $feeding) { Feeds() }
            .alert("インポートしました", isPresented: Binding(
                get: { clipDone != nil }, set: { if !$0 { clipDone = nil } }
            )) {
                Button("閉じる") {}
            } message: {
                Text("「\(clipDone ?? "")」を作りました。")
            }
            .overlay {
                if clipBusy {
                    // **待っていることを見せる。** 取りに行くのは何秒かかかる
                    // ので、何も出ないと押せていないように見える。
                    ZStack {
                        Color.black.opacity(0.25).ignoresSafeArea()
                        VStack(spacing: 10) {
                            ProgressView()
                            Text("取りに行っています…").font(.footnote)
                        }
                        .padding(22)
                        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 14))
                    }
                }
            }
            .navigationTitle("設定")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { Button("閉じる") { dismiss() } }
            }
        }
    }

    private func make(_ scope: String, _ what: String) {
        do { zip = try store.backup(scope: scope, what: what) }
        catch { trouble = error.localizedDescription }
    }
}

extension URL: @retroactive Identifiable {
    public var id: String { absoluteString }
}

/// The system's own share sheet, for a file that already exists.
///
/// `ShareLink` wants its item when the view is built; a backup is made when
/// the button is pressed, which is a different moment.
struct ActivityView: UIViewControllerRepresentable {
    let item: URL
    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: [item], applicationActivities: nil)
    }
    func updateUIViewController(_ vc: UIActivityViewController, context: Context) {}
}

/// **保存ディレクトリ一つの画面**（二段目・依頼 511）── 同期先／名前／場所／外す。
struct PlaceSheet: View {
    @ObservedObject var store: NotesStore
    let id: String
    /// 場所を選ぶ画面を開く（設定の紙を閉じてから ── 紙の上に紙は開かない）。
    let choose: () -> Void
    @ObservedObject private var sync = Syncing.shared
    @Environment(\.dismiss) private var dismiss
    @State private var name = ""
    @State private var dropping = false
    @State private var signingIn = false

    private var place: NotesStore.Place? { store.places.first { $0.id == id } }

    var body: some View {
        List {
            if let p = place {
                Section {
                    Picker("同期先", selection: Binding(get: { p.sync }, set: { pick($0) })) {
                        Text("同期しない").tag("none")
                        Text("Google Drive").tag("drive")
                    }
                    .pickerStyle(.inline)
                    .labelsHidden()
                    if p.sync == "drive", !sync.signedIn {
                        Button { signIn() } label: {
                            Label(signingIn ? "ブラウザで「許可」を押してください…" : "Google でサインイン",
                                  systemImage: "person.crop.circle.badge.checkmark")
                        }
                        .disabled(signingIn)
                    }
                } header: {
                    Text("同期先")
                } footer: {
                    Text(p.sync == "drive"
                         ? "Google Drive の「ambər」" + (p.at.isEmpty ? "" : " › " + p.at) + " に置きます。iCloud と OneDrive は、これから"
                         : "この iPhone だけに置きます。iCloud と OneDrive は、これから")
                }
                Section {
                    TextField("呼び名", text: $name)
                        .onSubmit { store.rename(place: id, to: name) }
                } header: {
                    Text("名前")
                } footer: {
                    Text("一覧での呼び名だけ。フォルダの名前は変わりません")
                }
                Section {
                    LabeledContent("いま", value: store.trail(of: p).joined(separator: " › "))
                    Button { choose() } label: { Label("場所を変える…", systemImage: "folder") }
                    if !p.own, !store.places.contains(where: { $0.own }) {
                        Button { store.relocateToOwn(id) } label: { Label("この iPhone の中にする", systemImage: "iphone") }
                    }
                } header: {
                    Text("場所")
                } footer: {
                    Text("場所を変えると、いままでのノートを一緒に移すか確認します")
                }
                Section {
                    Button(role: .destructive) { dropping = true } label: { Label("外す", systemImage: "minus.circle") }
                        .disabled(store.places.count < 2)
                } footer: {
                    Text(store.places.count < 2
                         ? "最後の一つは外せません（動かすなら「場所を変える…」）"
                         : "ambər の一覧から外します。フォルダとノートはそのまま残ります")
                }
            }
        }
        .navigationTitle(place?.name ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { name = place?.name ?? "" }
        .onDisappear { if let p = place, name != p.name { store.rename(place: id, to: name) } }
        .alert("「\(place?.name ?? "")」を ambər から外しますか", isPresented: $dropping) {
            Button("やめる", role: .cancel) {}
            Button("外す", role: .destructive) { if store.remove(place: id) { dismiss() } }
        } message: {
            Text("フォルダと中のノートはそのまま残ります")
        }
    }

    private func pick(_ to: String) {
        store.setSync(id, to)
        guard to == "drive" else { return }
        if sync.signedIn { sync.soon(1) } else { signIn() }
    }

    private func signIn() {
        signingIn = true
        Task {
            defer { signingIn = false }
            do { _ = try await sync.signIn(); sync.soon(1) }
            catch { store.trouble = "サインインできませんでした: " + error.localizedDescription }
        }
    }
}
