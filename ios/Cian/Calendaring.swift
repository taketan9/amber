import SwiftUI

/// **カレンダー**（依頼 454・iPhone の側）。デスクトップ版と同じ形。
///
/// 月の表に予定を並べ、日を押すとその日が下に開く。**予定を押したら
/// ノートへ** ── 予定は入口で、書くのはノート。**何もないところを押したら
/// 足せる**（足すのは新しいノートで、日付は前書きの `remind:` に書く ──
/// 依頼 73 の語彙のまま、カレンダーのためだけの置き場所を作らない）。
///
/// 数えるのは core（`month`）で、**デスクトップ版と同じ一組** ── 同じフォルダを
/// 両方から見ても、同じ予定が同じ日に並ぶ。
struct Calendaring: View {
    @ObservedObject var store: NotesStore
    /// 押されたノートを開く先（一覧の側が引き受ける）。
    let open: (Note) -> Void
    @Environment(\.dismiss) private var dismiss

    struct Slot: Identifiable, Hashable {
        let day: String
        let at: String?
        let title: String
        /// よその予定表のものには、開く先が無い。
        let path: String
        let kind: String
        /// よその予定の場所と、どの予定表から来たか。
        var place: String = ""
        var from: String = ""
        /// 予定のメモ欄（依頼 547）。**だれの用事かは、その最後の行にある。**
        var notes: String = ""
        /// メモ欄から読んだ、だれの用事か。切り出すのは core（`caltag`）。
        var tags: [String] = []
        /// その予定の色の名前（依頼 702・前書きの `color:`）。空なら出どころの色。
        var color: String = ""
        /// 何で塗るか（依頼 711）── core（`caltint`）が決めた言葉と、
        /// 決まっているなら `#rrggbb`。
        ///
        /// **描くときではなく、読み込むときに決める。** 月の表は描き直しが
        /// 多く、一つ描くごとに core を叩くと、予定の数だけ入口を往復する
        /// ことになる（デスクトップ版はそれが別のプロセスへの往復になる）。
        var paint: String = ""
        var paintHex: String = ""
        var id: String { day + (at ?? "") + path + kind + title }
        var isPlan: Bool { kind != "note" }
        var isAway: Bool { kind == "away" }
        /// この iPhone の予定表のもの ── **こちらは直せる**。
        /// この端末の予定表のもの ── **こちらは直せる**。
        var isPhone: Bool { kind == "here" }
        /// ノートを開く先を持たないもの（よそ・この iPhone）。
        var noNote: Bool { kind == "away" || kind == "here" }
    }

    /// 何を出しているか（依頼 530）── `me` / `group` / `both`。
    @State private var side = CalPrefs.side
    /// グループカレンダーの名前（依頼 534）。**`@State` で持つ。**
    ///
    /// `CalPrefs`（`UserDefaults`）を画面から直に読んでいたせいで、作った
    /// 直後に「グループへ招待」が出てこなかった ── **SwiftUI に「変わった」
    /// が伝わらない**ので、別の画面へ行って戻るまで描き直されない。
    /// 憶える先は `CalPrefs` のままで、**画面が見るのはこちら**。
    @State private var groupName = CalPrefs.groupName
    /// グループを作る前の確認と、作っているあいだ（依頼 532）。
    @State private var making = false
    @State private var busy = false
    @State private var made = ""
    /// 招待の行き方を出しているところ（依頼 534）。
    @State private var inviting = false
    /// 消す前の確認（依頼 535）。
    @State private var dropping = false
    /// 見つけたグループカレンダーを使うか訊いているところ（依頼 538）。
    @State private var found = false

    /// **開くたびに今月へ戻さない。** 先の予定を見にきた人を、
    /// 閉じて開くたびに今日へ連れ戻さない。
    @State private var year = Calendar.current.component(.year, from: Date())
    @State private var month = Calendar.current.component(.month, from: Date())
    @State private var picked = Calendaring.today
    @State private var slots: [Slot] = []
    @State private var trouble: String?
    @State private var adding = false
    @State private var newTitle = ""
    @State private var newAt = "09:00"
    @State private var newEnd = ""
    /// 予定をどこへ入れるか（依頼 545・デスクトップ版の `toGroup` と同じ）。**選ばれていない
    /// 状態で開く** ── 既定を持たせると、その既定のまま押した予定が出る。
    @State private var newToGroup: Bool?
    /// だれの用事か（グループに出すときだけ）。
    @State private var newTags: [String] = []
    /// いつまで・アラーム（依頼 700・701）。
    @State private var newUntil = ""
    @State private var newAlarm = ""
    @State private var newAlarmFor = ""
    /// 作る予定だけの色の名前（依頼 713）。空なら「自分の色」。
    @State private var newColor = ""
    /// 直している、この iPhone の予定。
    @State private var editing: Slot?
    /// 色を選ばせているところ（依頼 712）。
    @State private var coloring: Slot?
    @State private var editTitle = ""
    /// 月／週／日（依頼 515・デスクトップ版と同じ3 つ）。
    ///
    /// **開いたときは、いつも月**（依頼 697・本人）── 前に日を見ていても、
    /// 次に開くのは月から。カレンダーを開く理由はたいてい「今月どうなって
    /// いるか」で、前回の続きではない。`CalPrefs.view` は憶え続けるので、
    /// 開いているあいだの行き来はそのまま効く。
    @State private var mode = "month"
    @State private var settings = false
    /// 表示設定を変えたら描き直すための数。
    @State private var prefsTick = 0

    static var today: String {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: Date())
    }

    var body: some View {
        inside
            .task {
                // **押されて開いたときに訊く。** 起きた瞬間に訊くと、
                // 何のために訊かれたのか分からないまま断られる。
                if !Phone.asked { await Phone.ask() }
                count()
                // 招待された側のために、一度だけ探す（依頼 538）。
                if CalPrefs.foundGroup() { found = true }
            }
    }

    /// **一つの `body` に積み上げない。** 積むと Swift が型を追いきれず、
    /// 「時間内に型検査できません」で組めなくなる（実際になった）。
    private var inside: some View {
        VStack(spacing: 0) {
            stepper
            // 月／週／日の切り替え（依頼 515）── デスクトップ版の表の上の3 つと同じ。
            // **何を出すかは、その右に一つ**（依頼 531・本人が絵を見て案ウ）──
            // 段をもう一つ足すと月の表が 85pt 下がる（画面の一割）。iPhone は
            // 縦が命なので、横に置いて段を増やさない。
            HStack(spacing: 8) {
                Picker("表示", selection: $mode) {
                    Text("月").tag("month")
                    Text("週").tag("week")
                    Text("日").tag("day")
                }
                .pickerStyle(.segmented)
                .onChange(of: mode) { _, now in CalPrefs.view = now; count() }
                if !groupName.isEmpty { sidePill }
            }
            .padding(.horizontal, 12).padding(.top, 6)
            if mode == "month" {
                grid
            } else if mode == "week" {
                week
            } else {
                day
            }
        }
        .overlay(alignment: .bottomTrailing) { plusButton }
        .navigationTitle("カレンダー")
        .navigationBarTitleDisplayMode(.inline)
        .navigationBarBackButtonHidden(true)
        .toolbar { bar }
        .sheet(isPresented: $settings) {
            // **設定で変わったものを、画面にも伝える**（依頼 707・708）。
            // `side` と `groupName` はこの画面の `@State` なので、`CalPrefs` を
            // 書き換えただけでは描き直されない ── 依頼 534 で「作ったのに何も
            // 変わらないように見える」を踏んだのと同じ形。
            //
            // **`groupName` を忘れると、グループが丸ごと無いことになる**
            // （依頼 708）── 設定でグループの予定表を選んでも、見出しの
            // 「自分だけ／グループ／両方」が出ず、予定を足す画面に
            // 「グループと共有」も出ず、設定には「グループを作る」が残る。
            // 別の画面へ行って戻るまでそのままなので、選んだ人には
            // 「選んだのに何も起きない」としか見えない。
            CalSettings(changed: {
                side = CalPrefs.side
                groupName = CalPrefs.groupName
                prefsTick += 1
                count()
            })
        }
        .id(prefsTick)
        // 作る前の確認と、作ったあとの知らせ（依頼 532）。
        .alert("グループを作りますか", isPresented: $making) {
            Button("作る") { makeGroup() }
            Button("やめる", role: .cancel) { }
        } message: {
            Text("「ambər グループ」という新しいカレンダーを作ります。\n\n"
                 + "このカレンダーに入れた予定だけが、招待した人に見えます。"
                 + "今ある予定はそのままで、共有されません。")
        }
        // 招待された側に、一度だけ訊く（依頼 538）。
        .alert("グループカレンダーが見つかりました", isPresented: $found) {
            Button("使う") {
                CalPrefs.groupAsked = true
                CalPrefs.groupName = CalPrefs.groupWord
                // **id は持たない。** 招待された側は持ち主ではないので、
                // Google 側の id を知らない（招待も削除もできない）。
                CalPrefs.groupId = ""
                groupName = CalPrefs.groupWord
                side = "group"
                CalPrefs.side = "group"
                count()
            }
            Button("使わない", role: .cancel) { CalPrefs.groupAsked = true }
        } message: {
            Text("「\(CalPrefs.groupWord)」というカレンダーがこの iPhone にあります。\n\n"
                 + "グループカレンダーとして使いますか。"
                 + "このカレンダーの予定が、グループの予定として色分けされます。")
        }
        .alert("グループカレンダーを削除しますか", isPresented: $dropping) {
            Button("削除", role: .destructive) { dropGroup() }
            Button("キャンセル", role: .cancel) { }
        } message: {
            Text("「\(groupName)」を削除します。\n\n"
                 + "招待した人のカレンダーからも消えます。"
                 + "中に入っている予定もすべて削除され、元に戻すことはできません。")
        }
        // **行き方を言う**（依頼 534）── iPhone では amber の中から招待できない。
        .alert("グループへ招待", isPresented: $inviting) {
            Button("Google カレンダーを開く") { openGoogleCalendar() }
            Button("閉じる", role: .cancel) { }
        } message: {
            Text("招待は Google カレンダーのアプリで行います。\n\n"
                 + "1. 左上のメニューから「設定」\n"
                 + "2. 「\(groupName)」を選ぶ\n"
                 + "3. 「ユーザーまたはグループを追加」\n\n"
                 + "相手のメールアドレスを入力すると、招待が届きます。")
        }
        .alert("完了しました", isPresented: Binding(get: { !made.isEmpty }, set: { if !$0 { made = "" } })) {
            Button("OK") { made = "" }
        } message: { Text(made) }
        .modifier(Asking(
            trouble: $trouble, adding: $adding, editing: $editing, coloring: $coloring,
            newTitle: $newTitle, newAt: $newAt, newEnd: $newEnd, editTitle: $editTitle,
            newToGroup: $newToGroup, newTags: $newTags,
            newUntil: $newUntil, newAlarm: $newAlarm, newAlarmFor: $newAlarmFor,
            newColor: $newColor,
            day: spoken(picked), toPhone: Phone.allowed, groupName: groupName,
            add: add, rename: rename, drop: drop, recolor: recolor))
    }

    /// 題と、前後へ動く矢印（依頼 670）。**右上の角に固めない。**
    ///
    /// いままでは歯車・矢印・今日・矢印が全部バーの右端にあった ── いちばん
    /// よく押すものが、親指のいちばん届かないところに四つ並んでいたことになる。
    /// 題の両脇に置くと、押すところが 56×44 取れる。
    ///
    /// **`Text` に数をそのまま渡さない。** SwiftUI は土地の決まりで桁を
    /// 区切るので、年が「2,026年」になる（実際になった）。
    private var stepper: some View {
        HStack(spacing: 0) {
            Button { step(-1) } label: {
                Image(systemName: "chevron.left")
                    .font(.title3.weight(.semibold))
                    .frame(width: 56, height: 44)
            }
            .accessibilityLabel("前へ")
            Spacer(minLength: 0)
            Text(verbatim: heading)
                .font(.title2.weight(.semibold))
                .monospacedDigit()
                .lineLimit(1)
                .minimumScaleFactor(0.5)
            Spacer(minLength: 0)
            Button { step(1) } label: {
                Image(systemName: "chevron.right")
                    .font(.title3.weight(.semibold))
                    .frame(width: 56, height: 44)
            }
            .accessibilityLabel("次へ")
        }
        .padding(.horizontal, 4)
    }

    /// **予定を足す丸いボタン**（依頼 672・本人「予定を追加する プラスボタンが
    /// ほしいなぁ」）。
    ///
    /// 月の表に画面を全部渡したとき、下の日別リストと一緒に「予定を登録する」も
    /// 消えてしまい、**月から予定を足す道が無くなっていた。** 三つの画面
    /// どれからでも押せるように、ここに一つ置く。
    ///
    /// 置くのは**右下** ── いちばんよく押すものを、親指の届くところに。
    /// 形は一覧の「新しいノート」と同じ琥珀の丸で揃える。
    /// 足す先は、いま選んである日（登録の画面がその日付を出す）。
    private var plusButton: some View {
        Button {
            newTitle = ""
            newAt = "09:00"
            newEnd = ""
            adding = true
        } label: {
            Image(systemName: "plus")
                .font(.system(size: 24, weight: .semibold))
                .foregroundStyle(.white)
                .frame(width: 56, height: 56)
                .background(Circle().fill(Color.accentColor))
                .shadow(color: Color.accentColor.opacity(0.45), radius: 6, y: 2)
        }
        .accessibilityLabel("予定を登録する")
        .padding(.trailing, 18)
        .padding(.bottom, 18)
    }

    @ToolbarContentBuilder private var bar: some ToolbarContent {
        // **「＜」ではなく「ノートに戻る」**（依頼 697・本人）── 矢印だけだと
        // どこへ戻るのか分からない。押すまではカレンダーが既定の画面なので、
        // **この札が「出口はここ」と言う役**も持つ。
        ToolbarItem(placement: .topBarLeading) {
            Button {
                CalPrefs.stay = false
                dismiss()
            } label: {
                // **`Label` は使わない** ── バーの `Label` は字を落として
                // 絵だけになることがあり、実際にそうなった（シミュレータ）。
                // 並べて置けば、字は必ず出る。
                HStack(spacing: 3) {
                    Image(systemName: "chevron.backward")
                    Text("ノートに戻る")
                }
            }
        }
        ToolbarItemGroup(placement: .topBarTrailing) {
            Button { settings = true } label: { Image(systemName: "gearshape") }
                .accessibilityLabel("カレンダー表示設定")
            Button("今日") {
                year = Calendar.current.component(.year, from: Date())
                month = Calendar.current.component(.month, from: Date())
                picked = Self.today
                count()
            }
        }
    }

    // MARK: 月の表

    /// セルの高さと、一日に出す件数（依頼 670）。
    ///
    /// **月の表に画面を全部渡した**ので、46pt だった段を 118pt にできた。
    /// 日付 24pt ＋ 札 4 枚 ＋「ほか N」で 118pt に収まる。
    /// 一画面に五段は入り、六段の月だけ少し巻く（Urecy も同じ）。
    private static let cellHigh: CGFloat = 118
    private static let chips = 4
    /// セルとセルのすき間。**端から端まで使う**（依頼 670・本人「Urecy だと
    /// 一つのセルで 5 文字は表示できている」）── 外に 8pt の余白を置き、
    /// すき間を 3pt 取っていたので、横が 34pt 減ってセルが 51pt しか無かった。
    /// すき間を 1pt にして余白を落とすと 55pt になり、**一文字ぶん増える**。
    private static let gap: CGFloat = 1

    private var grid: some View {
        VStack(spacing: 3) {
            // 曜日の段は巻かない ── 巻くと、何曜日の列を見ているか分からなくなる。
            HStack(spacing: Self.gap) {
                ForEach(Array(["月", "火", "水", "木", "金", "土", "日"].prefix(CalPrefs.weekend ? 7 : 5).enumerated()), id: \.offset) { i, w in
                    Text(w).font(.footnote.weight(.semibold))
                        .foregroundStyle(i == 5 ? Color.blue : (i == 6 ? Color.red : Color.secondary))
                        .frame(maxWidth: .infinity)
                }
            }
            ScrollView {
                VStack(spacing: Self.gap) {
                    ForEach(weeks, id: \.self) { week in
                        HStack(spacing: Self.gap) {
                            // 土日を出さないときは月〜金だけ（依頼 515）。
                            ForEach(Array(week.prefix(CalPrefs.weekend ? 7 : 5)), id: \.self) { d in cell(d) }
                        }
                    }
                }
                .padding(.bottom, 86)
            }
            // **横に払うと、前の月・次の月**（依頼 697・本人）。
            //
            // **縦の巻きを邪魔しない** ── 横に 40pt 以上動き、かつ縦より
            // 横のほうが大きいときだけ月を替える。`simultaneousGesture` に
            // しないと、巻くための指が払いとして取られて表が動かなくなる。
            .simultaneousGesture(
                DragGesture(minimumDistance: 40)
                    .onEnded { g in
                        let x = g.translation.width
                        let y = g.translation.height
                        guard abs(x) > abs(y) * 1.5 else { return }
                        step(x < 0 ? 1 : -1)
                    }
            )
        }
        .padding(.top, 6)
    }

    /// 何を出しているか（依頼 531）。**押すと回る。**
    ///
    /// 回る順は **両方 → 自分だけ → グループ → 両方**（本人）。
    /// 回る形の弱いところは「ほかに何が選べるか」が押すまで分からないこと
    /// なので、**長押しで3 つ出す** ── 押し先が一つで済む軽さは残したまま、
    /// 全部を見るパスも残す。
    /// **一人なら、その色。二人以上なら、帯を分ける**（依頼 547・デスクトップ版の
    /// `tagPaint` と同じ決まり）── 二人の用事は二人のものなので、どちらか
    /// 片方の色にしてしまうと嘘になる。タグが無ければ何も差さず、
    /// いままでの出どころ別の色のままにする。
    private func tagTints(_ s: Slot) -> [Color] {
        s.tags.compactMap { Color(hex: CalPrefs.laneColor($0, among: CalPrefs.tagsKnown)) }
    }

    /// タグの色の帯（予定の左に立てる）。色が無ければ何も出さない。
    @ViewBuilder private func tagBand(_ s: Slot, width: CGFloat, height: CGFloat) -> some View {
        let tints = tagTints(s)
        if !tints.isEmpty {
            VStack(spacing: 0) {
                ForEach(Array(tints.enumerated()), id: \.offset) { _, c in c }
            }
            .frame(width: width, height: height)
            .clipShape(RoundedRectangle(cornerRadius: width / 2))
        }
    }

    private static let sides = ["both", "me", "group"]
    private func sideWord(_ k: String) -> String {
        ["both": "両方", "me": "自分だけ", "group": "グループ"][k] ?? "両方"
    }
    private func setSide(_ k: String) {
        side = k
        CalPrefs.side = k
        count()
    }
    private var sidePill: some View {
        Button {
            let i = Self.sides.firstIndex(of: side) ?? 0
            setSide(Self.sides[(i + 1) % Self.sides.count])
        } label: {
            Label(sideWord(side), systemImage: "person.2")
                .font(.footnote)
                .labelStyle(.titleAndIcon)
                .lineLimit(1)
                .padding(.horizontal, 10).padding(.vertical, 6)
                .background(Color(.secondarySystemBackground))
                .clipShape(Capsule())
        }
        .buttonStyle(.plain)
        .contextMenu {
            // **長押しで3 つ出す。** 回る形だけだと、選べるものが分からない。
            ForEach(Self.sides, id: \.self) { k in
                Button { setSide(k) } label: {
                    if k == side { Label(sideWord(k), systemImage: "checkmark") }
                    else { Text(sideWord(k)) }
                }
            }
        }
    }

    /// グループカレンダーを1 つ作る（依頼 532）。**押すのは一回。**
    ///
    /// サインインもカレンダーの許可も `Drive` が面倒を見る。作る前に
    /// 「いまの予定は一つも動きません」と言う ──「共有」を押した瞬間に
    /// 何が起きるか分からないのが、いちばん怖い。
    private func makeGroup() {
        busy = true
        Task {
            do {
                let got = try await Drive.shared.makeGroupCalendar(named: "ambər グループ")
                CalPrefs.groupName = got.name
                CalPrefs.groupId = got.id
                // **画面にも伝える。** ここを忘れると、作ったのに何も
                // 変わらないように見える（実機で出た・依頼 534）。
                groupName = got.name
                side = "group"
                CalPrefs.side = "group"
                // **二段あることを、その場で言う。** amber が作っただけでは
                // 誰にも届かない ── 招待は Google の画面で（一般公開のときに
                // amber の中へ入れる）。
                made = "「\(got.name)」を作成しました。\n\n"
                    + "「グループへ招待」から、一緒に使う人を招待できます。"
                count()
            } catch {
                trouble = error.localizedDescription
            }
            busy = false
        }
    }

    /// グループへ招待する。**いまは Google カレンダーのアプリで**
    /// （依頼 534・実機で分かった）。
    ///
    /// はじめはパソコン版の設定ページの URL を開いていた。**URL は合っていた**
    /// （本人の本物のカレンダーで確かめた ── 組んだ文字と Google が出す URL が
    /// 一字一句同じ）。**駄目だったのは、iPhone が `calendar.google.com` を
    /// Google カレンダーのアプリに渡してしまうこと** ── アプリは設定ページを
    /// 出せないので、「このカレンダーにアクセスするには…ログインしてください」
    /// とだけ出る。**URL の間違いではなく、行き先の取られ方の問題だった。**
    ///
    /// iPhone でも共有そのものはできる ── **アプリの設定から**。だから
    /// **URL を当てにいかず、行き方を言う**。amber の中で招待できるようにする
    /// には `calendar.acls`（審査の要る許可）が要るので、一般公開のときに通す。
    private func invite() { inviting = true }

    /// **グループカレンダーを消す**（依頼 535）。
    ///
    /// **本当に消える。** 持ち主が消すと、グループの人の画面からも消える ──
    /// 「amber から外す」ではない。戻すパスがどこにも無いので、押す前にそう言う。
    private func dropGroup() {
        busy = true
        Task {
            do {
                let killed = try await Drive.shared.dropGroupCalendar(CalPrefs.groupId)
                let was = groupName
                CalPrefs.groupName = ""
                CalPrefs.groupId = ""
                groupName = ""
                side = "both"
                CalPrefs.side = "both"
                // **もう無かったときも、そう言う**（依頼 536）── 黙って
                // 「消しました」と言うと、まだあるのに消したように読める。
                made = killed
                    ? "「\(was)」を削除しました。"
                    : "「\(was)」は Google カレンダー側ですでに削除されていました。\n\n"
                      + "amber の設定からも解除しました。"
                count()
            } catch {
                trouble = error.localizedDescription
            }
            busy = false
        }
    }

    private func openGoogleCalendar() {
        if let u = URL(string: "https://calendar.google.com/") { UIApplication.shared.open(u) }
    }

    @ViewBuilder private func cell(_ d: String) -> some View {
        if d.isEmpty {
            Color.clear.frame(maxWidth: .infinity, minHeight: Self.cellHigh)
        } else {
            let mine = shown(on: d)
            let on = d == picked
            VStack(alignment: .leading, spacing: 2) {
                Text(String(Int(d.suffix(2)) ?? 0))
                    .font(.system(size: 20, weight: d == Self.today ? .bold : .medium))
                    .monospacedDigit()
                    .foregroundStyle(d == Self.today ? Color.accentColor : .primary)
                ForEach(mine.prefix(Self.chips)) { s in dayChip(s) }
                if mine.count > Self.chips {
                    Text("ほか \(mine.count - Self.chips)")
                        .font(.system(size: 11)).foregroundStyle(.secondary)
                        .padding(.leading, 3)
                }
                Spacer(minLength: 0)
            }
            .padding(2)
            .frame(maxWidth: .infinity, minHeight: Self.cellHigh, alignment: .topLeading)
            .background(on ? Color.accentColor.opacity(0.12) : Color(.secondarySystemBackground))
            .overlay(RoundedRectangle(cornerRadius: 6)
                .stroke(d == Self.today ? Color.accentColor : .clear, lineWidth: 1))
            .clipShape(RoundedRectangle(cornerRadius: 6))
            .contentShape(Rectangle())
            // **押したら「日」の画面へ**（依頼 670）── 月の下に日別リストを
            // 置くのをやめたので、読む場所は日の画面になった。
            .onTapGesture { picked = d; mode = "day" }
        }
    }

    /// 月の表の、一件ぶんの札（依頼 670）。
    ///
    /// **8pt の素の文字をやめて、色の付いた札にした**（本人「月の表示が小さい」）。
    /// 字は 11pt で三つしか増えていないが、色の面に乗ると読める ── Urecy の
    /// 月表示も同じ作りで、字は 10pt ほどしかない。効いているのは字の大きさ
    /// ではなく、**色と、月の表に画面を全部渡したこと**。
    ///
    /// **しっかり塗ってあるものが、グループに見えている予定**（依頼 530）──
    /// 自分だけの予定は薄い塗りにして、この決まりは残した。
    /// だれの用事かは、左の色の帯で（依頼 547）。
    /// その予定の色（依頼 702）。
    ///
    /// **決める順は core**（`caltint`・依頼 711）── 読み込むときに決めて
    /// `paint` に入れてある。ここがするのは、その言葉を色に直すだけ。
    ///
    /// **青と琥珀は core から来ない。** 明るい紙と暗い紙で濃さを変えたい
    /// もので、`#rrggbb` に潰すと暗いところで読めなくなる。
    ///
    /// `paint` が空なのは、core に訊けなかったとき（まず起きない）。
    /// そのときはいままでの決まりをここで通す ── 色が無いより、前の絵のほうがよい。
    private func tint(of s: Slot) -> Color {
        switch s.paint {
        case "event", "shelf":
            if let got = Color(hex: s.paintHex) { return got }
        case "away": return .blue
        case "here": return CalPrefs.hereTint
        case "plan": return .accentColor
        case "other": return .secondary
        default: break
        }
        if s.isAway { return .blue }
        if s.isPhone { return CalPrefs.hereTint }
        return s.isPlan ? .accentColor : .secondary
    }

    @ViewBuilder private func dayChip(_ s: Slot) -> some View {
        let tint = tint(of: s)
        let full = CalPrefs.inGroup(s)
        let tints = tagTints(s)
        // **文字を、セルの幅に口出しさせない。**
        //
        // `fixedSize()` を素のまま使うと、長い題の入ったセルだけ横に広がり、
        // **隣の列を押して土曜日が画面の外へ出た**（シミュレータで出た）。
        // 幅を決めるのは `Color.clear` のほうで、文字はその上に重ねて切る
        // ── 重ねたものは、下の大きさに口出ししない。
        Color.clear
            .frame(height: 15)
            .overlay(alignment: .leading) {
                // **だれの用事かの帯が無いときは、すき間も取らない** ──
                // 空の帯でも `HStack` の間隔は残り、文字の幅が 2pt 減る。
                HStack(spacing: tints.isEmpty ? 0 : 2) {
                    if !tints.isEmpty { tagBand(s, width: 2, height: 12) }
                    // **「…」を付けずに切る**（Urecy と同じ）── セルは横 55pt
                    // しか無く、「…」だけで一文字ぶん食う。「ドライブスル」
                    // のように途中で切れていても、**五文字目まで読めるほうが
                    // 分かる**（「ドラ…」では、ドライブスルーかドラッグストアか
                    // 決まらない）。
                    Text(s.title)
                        .font(.system(size: 10.5))
                        .lineLimit(1)
                        .fixedSize()
                        .foregroundStyle(full ? Color.white : tint)
                }
                .padding(.leading, 1)
            }
            .clipped()
            .background(full ? CalPrefs.hereTint : tint.opacity(0.16))
            .clipShape(RoundedRectangle(cornerRadius: 3))
    }

    // MARK: その日

    private var day: some View {
        List {
            // **段に見出しを付けない**（依頼 670）── すぐ上の題が
            // 「10月7日（水）」と言っているので、同じ日付が二度出る。
            Section {
                let plans = slots.filter { $0.day == picked && $0.isPlan && CalPrefs.visible($0) }
                if plans.isEmpty {
                    Text("予定はありません").foregroundStyle(.secondary).font(.body)
                }
                ForEach(plans) { s in row(s, time: true) }
            }
            // **予定に出ているノートを、下でもう一度出さない** ── 同じ一本が
            // 二度並ぶと、二つあるように見える。
            let said = Set(slots.filter { $0.day == picked && $0.isPlan && CalPrefs.visible($0) }.map(\.path))
            let notes = slots.filter { $0.day == picked && !$0.isPlan && !said.contains($0.path) && CalPrefs.visible($0) }
            if !notes.isEmpty {
                Section("この日に書いたノート") {
                    ForEach(notes) { s in row(s, time: false) }
                }
            }
            Section {
                Button {
                    newTitle = ""
                    newAt = "09:00"
                    newEnd = ""
                    adding = true
                } label: {
                    Label("予定を登録する", systemImage: "plus")
                }
                // **グループを始めるパスは、ここに置く**（依頼 532）── 設定の
                // 中ではない。狙いは IT に明るくない人で、**PC を持っていない
                // 可能性が大いにある**（本人）ので、iPhone で始められないのは
                // 致命的。作ったら消え、そのあとは「グループへ招待」になる。
                if groupName.isEmpty {
                    Button { making = true } label: {
                        Label("グループを作る", systemImage: "person.2.badge.plus")
                    }
                    .disabled(busy)
                } else if side == "group" && !CalPrefs.groupId.isEmpty {
                    // **招待と削除は、作った人だけ。** 招待された側は Google 側の
                    // id を持たないので、共有設定にも行けないし消せもしない
                    // （どちらも作った人の端末からやってもらう）。
                    Button { invite() } label: {
                        Label("グループへ招待", systemImage: "person.2")
                    }
                    // **作るパスがあるなら、やめるパスもある**（依頼 535）。
                    // 一生に一度で戻せないので、赤くして、押す前に二度言う。
                    Button(role: .destructive) { dropping = true } label: {
                        Label("グループカレンダーを削除", systemImage: "trash")
                    }
                    .disabled(busy)
                }
            }
        }
        .listStyle(.insetGrouped)
        .contentMargins(.bottom, 86, for: .scrollContent)
    }

    /// 週・日の一行。
    ///
    /// **押せるのは行ぜんぶ**（依頼 697・本人「文字以外の箇所をタップしても
    /// その予定を編集できるようにしてほしい」）── 前は字の形のところしか
    /// 当たらず、右の余白を押しても何も起きなかった。横いっぱいに広げて、
    /// 透明なところにも当たり判定を置く。
    @ViewBuilder private func row(_ s: Slot, time: Bool) -> some View {
        Button {
            // **よその予定にはノートが無い。** 押しても何も起きないより、
            // なぜ開かないかを言う。
            if s.isPhone {
                editing = s
                editTitle = s.title
                return
            }
            if s.isAway {
                trouble = "よその予定表のものなので、ここでは直せません。"
                return
            }
            guard let n = store.notes.first(where: { $0.path == s.path }) else { return }
            // **自分では閉じない**（依頼 670）── 一覧へ戻ってからノートを
            // 開くのは一覧の仕事で、こちらは「これが押された」とだけ言う。
            open(n)
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                if time {
                    // 時刻は本文の大きさ（17pt）── 予定名より一段落として、
                    // 「何時」と「何を」を目で分けられるようにする。
                    Text(s.at ?? "終日").font(.body).monospacedDigit()
                        .foregroundStyle(.secondary).frame(width: 62, alignment: .leading)
                }
                VStack(alignment: .leading, spacing: 2) {
                    // 塗ってあるものが、グループに見えている予定（依頼 530）。
                    // だれの用事かは、左の帯の色で（依頼 547）。
                    HStack(spacing: 6) {
                        tagBand(s, width: 4, height: 24)
                        // **予定名は見出しの大きさ**（依頼 670・本人「2 倍から
                        // 3 倍くらい文字を大きく」）── 22pt。`title2` にして
                        // あるので、アプリの「文字の大きさ」でさらに伸びる。
                        //
                        // 色は `tint(of:)` が決める（依頼 702）── 前書きに
                        // `color:` があればそれ、無ければ出どころの色。
                        // **自分のノートの予定で色を書いていなければ、黒のまま**
                        // ── 一覧の文字と同じ読みやすさを崩さない。
                        Text(s.title)
                            .font(.title2)
                            .foregroundStyle(s.color.isEmpty && !s.isAway && !s.isPhone
                                             ? Color.primary : tint(of: s))
                    }
                    .padding(.horizontal, CalPrefs.inGroup(s) ? 5 : 0)
                    .padding(.vertical, CalPrefs.inGroup(s) ? 2 : 0)
                    .background(CalPrefs.inGroup(s)
                        ? CalPrefs.hereTint.opacity(0.18) : Color.clear)
                    .clipShape(RoundedRectangle(cornerRadius: 5))
                    // **パスは出さない** ── 読めない長さになるうえ、知りたいのは
                    // 中身のほう。自分のノートは一行目、よその予定は場所と出どころ。
                    if s.noNote {
                        let under = [s.place, s.from].filter { !$0.isEmpty }.joined(separator: "・")
                        if !under.isEmpty {
                            Text(under).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                        }
                    } else if let n = store.notes.first(where: { $0.path == s.path }),
                              !n.excerpt.isEmpty {
                        Text(n.excerpt).font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
                    }
                }
                Spacer(minLength: 0)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    // MARK: 数える・足す

    private func shown(on d: String) -> [Slot] {
        let mine = slots.filter { $0.day == d && CalPrefs.visible($0) }
        let plans = mine.filter(\.isPlan)
        let said = Set(plans.map(\.path))
        return plans + mine.filter { !$0.isPlan && !said.contains($0.path) }
    }

    private var weeks: [[String]] {
        var out: [[String]] = []
        var week: [String] = []
        var cal = Calendar(identifier: .gregorian)
        cal.firstWeekday = 2
        guard let first = cal.date(from: DateComponents(year: year, month: month, day: 1)),
              let span = cal.range(of: .day, in: .month, for: first)
        else { return [] }
        // 月曜はじまり ── 一覧の並びも週も、ここでは月曜から。
        let lead = (cal.component(.weekday, from: first) + 5) % 7
        week.append(contentsOf: Array(repeating: "", count: lead))
        for d in span {
            week.append(String(format: "%04d-%02d-%02d", year, month, d))
            if week.count == 7 { out.append(week); week = [] }
        }
        if !week.isEmpty {
            week.append(contentsOf: Array(repeating: "", count: 7 - week.count))
            out.append(week)
        }
        return out
    }

    private func spoken(_ d: String) -> String {
        guard d.count == 10 else { return d }
        let m = Int(d.dropFirst(5).prefix(2)) ?? 0
        let day = Int(d.suffix(2)) ?? 0
        return "\(m)月\(day)日"
    }

    private func rename() {
        guard let e = editing else { return }
        let title = editTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty else { return }
        do {
            try Phone.rename(e.path, to: title)
            count()
        } catch {
            trouble = error.localizedDescription
        }
    }

    private func drop() {
        guard let e = editing else { return }
        do {
            try Phone.drop(e.path)
            count()
        } catch {
            trouble = error.localizedDescription
        }
    }

    /// **この予定だけの色を書き換える**（依頼 712）。`name` が空ならやめる。
    ///
    /// メモ欄のいちばん最後の行（タグと同じ一行）に `#色:カーマイン` を置く。
    /// **どこを書き換えるかを決めるのは core**（`caltagset`）── ここで
    /// 切り出すと、同じ予定がデスクトップ版と iPhone で違う顔をする。
    ///
    /// **人が書いた文章には触らない。** 書き換わるのはその一行だけ。
    ///
    /// 文字を組むのは `CalPrefs.recolored` ── 総ざらいから呼べるように
    /// 画面の外に置いてある。
    private func recolor(_ name: String) {
        guard let e = coloring else { return }
        guard let notes = CalPrefs.recolored(notes: e.notes, tags: e.tags, color: name) else {
            trouble = "色を書けませんでした。"
            return
        }
        do {
            try Phone.setNotes(e.path, notes)
            count()
        } catch {
            trouble = error.localizedDescription
        }
    }

    private func step(_ n: Int) {
        if mode == "week" || mode == "day" {
            picked = Self.shift(picked, n * (mode == "week" ? 7 : 1))
            year = Int(picked.prefix(4)) ?? year
            month = Int(picked.dropFirst(5).prefix(2)) ?? month
            count()
            return
        }
        var m = month + n
        var y = year
        if m < 1 { m = 12; y -= 1 }
        if m > 12 { m = 1; y += 1 }
        year = y
        month = m
        count()
    }

    // MARK: 週・日

    /// 選んだ日の週（月曜から）。土日を出さないなら月〜金。
    private var weekDays: [String] {
        guard let d = Self.date(picked) else { return [picked] }
        var cal = Calendar(identifier: .gregorian)
        cal.firstWeekday = 2
        let lead = (cal.component(.weekday, from: d) + 5) % 7
        let monday = Self.shift(picked, -lead)
        let n = CalPrefs.weekend ? 7 : 5
        return (0..<n).map { Self.shift(monday, $0) }
    }

    /// 上の題（月／週／日で違う）。
    private var heading: String {
        if mode == "week", let a = weekDays.first, let b = weekDays.last { return spoken(a) + " 〜 " + spoken(b) }
        if mode == "day" { return spoken(picked) + "（" + Self.weekName(picked) + "）" }
        return "\(year)年 \(month)月"
    }

    /// 週の表 ── 七日ぶんを段にして並べる（日ごとの予定と、登録する道）。
    private var week: some View {
        List {
            ForEach(weekDays, id: \.self) { d in
                Section {
                    let mine = shown(on: d)
                    if mine.isEmpty {
                        Text("予定はありません").foregroundStyle(.secondary).font(.body)
                    }
                    ForEach(mine) { s in row(s, time: true) }
                    Button {
                        picked = d
                        newTitle = ""
                        newAt = "09:00"
                        newEnd = ""
                        adding = true
                    } label: {
                        Label("予定を登録する", systemImage: "plus").font(.body)
                    }
                } header: {
                    Text(spoken(d) + "（" + Self.weekName(d) + "）" + (d == Self.today ? "　今日" : ""))
                        .font(.headline)
                        .foregroundStyle(d == Self.today ? Color.accentColor : Color.secondary)
                }
            }
        }
        .listStyle(.insetGrouped)
        .contentMargins(.bottom, 86, for: .scrollContent)
    }

    static func date(_ d: String) -> Date? {
        let f = DateFormatter()
        f.calendar = Calendar(identifier: .gregorian)
        f.dateFormat = "yyyy-MM-dd"
        return f.date(from: d)
    }

    static func shift(_ d: String, _ n: Int) -> String {
        guard let at = date(d), let to = Calendar(identifier: .gregorian).date(byAdding: .day, value: n, to: at) else { return d }
        let f = DateFormatter()
        f.calendar = Calendar(identifier: .gregorian)
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: to)
    }

    static func weekName(_ d: String) -> String {
        guard let at = date(d) else { return "" }
        let i = (Calendar(identifier: .gregorian).component(.weekday, from: at) + 5) % 7
        return ["月", "火", "水", "木", "金", "土", "日"][i]
    }

    /// 表に要る月（月なら一つ・週は跨ぐ二つまで・日は一つ）。
    private var months: [(Int, Int)] {
        let days = mode == "week" ? weekDays : (mode == "day" ? [picked] : [])
        guard !days.isEmpty else { return [(year, month)] }
        var out: [(Int, Int)] = []
        for d in [days.first!, days.last!] {
            let y = Int(d.prefix(4)) ?? year
            let m = Int(d.dropFirst(5).prefix(2)) ?? month
            if !out.contains(where: { $0 == (y, m) }) { out.append((y, m)) }
        }
        return out
    }

    private func count() {
        guard !store.rootPath.isEmpty else { return }
        let want = months
        var all: [Slot] = []
        do {
            // 保存ディレクトリごとに訊いて足す（依頼 511）── どの日のノートも、
            // どこに置いてあっても同じ表に出る。
            for (y, m) in want {
                for p in store.places {
                    guard let url = store.url(of: p) else { continue }
                    let got = try Cian.call("month", [
                        "path": url.path, "year": y, "month": m,
                    ])
                    all += (got["days"] as? [[String: Any]] ?? []).map {
                        Slot(day: $0["day"] as? String ?? "",
                             at: $0["at"] as? String,
                             title: $0["title"] as? String ?? "",
                             path: $0["path"] as? String ?? "",
                             kind: $0["kind"] as? String ?? "note",
                             color: $0["color"] as? String ?? "")
                    }
                }
                // この iPhone の予定表（許可されているときだけ）。
                all += Phone.month(y, m)
            }
            slots = readTags(all)
        } catch {
            trouble = error.localizedDescription
        }
        // よその予定表は、あとから足す ── 一つも読めなくても、自分のぶんは出る。
        Task { @MainActor in
            var more: [Slot] = []
            for (y, m) in want { more += await Away.month(y, m) }
            guard want.map({ "\($0.0)-\($0.1)" }) == months.map({ "\($0.0)-\($0.1)" }) else { return }   // 月を替えたあとの答えは捨てる（週・日の表でも同じ）
            slots = (slots.filter { !$0.isAway } + more).sorted {
                ($0.day, $0.at == nil ? 1 : 0, $0.at ?? "", $0.title)
                    < ($1.day, $1.at == nil ? 1 : 0, $1.at ?? "", $1.title)
            }
        }
    }

    /// **だれの用事かを、メモ欄から読む**（依頼 547・デスクトップ版の `readCalTags` と同じ）。
    ///
    /// 訊くのはグループカレンダーの予定だけ ── 自分だけの予定に人の名前は
    /// 入らないし、よその予定表のメモまで覗く筋合いが無い。**一度にまとめて
    /// 訊く**（`caltag` は文字の一覧を受ける）── 予定の数だけ入口を叩かない。
    private func readTags(_ all: [Slot]) -> [Slot] {
        // **色は、この端末の予定表のものぜんぶから読む**（依頼 702・甲）──
        // だれの用事かはグループの予定にしか入らないが、色は自分だけの
        // 予定にも入る。**よその予定表のメモは覗かない**（`isAway` を外す）。
        let want = all.indices.filter {
            !all[$0].notes.isEmpty && (CalPrefs.inGroup(all[$0]) || all[$0].isPhone)
        }
        guard !want.isEmpty else { return all }
        guard let got = try? Cian.call("caltag", ["notes": want.map { all[$0].notes }]),
              let each = got["each"] as? [[String: Any]] else { return all }
        var out = all
        for (i, k) in want.enumerated() where i < each.count {
            // タグはグループの予定だけ ── 自分だけの予定に人の名前は入らない。
            if CalPrefs.inGroup(out[k]) {
                out[k].tags = each[i]["tags"] as? [String] ?? []
            }
            out[k].color = each[i]["color"] as? String ?? ""
        }
        // 見た名前は憶える ── 次に予定を登録するとき、打たずに選べる。
        for s in out { for t in s.tags { CalPrefs.rememberTag(t) } }
        return paintAll(out)
    }

    /// **何で塗るかを、読み込むときに決めておく**（依頼 711）。
    ///
    /// 決めるのは core（`caltint`）。ここがするのは、**同じ答えになるものを
    /// 二度訊かない**ことだけ ── ひと月ぶんの予定の組み合わせは、たいてい
    /// 数通りしかない（予定表の色と、予定に書いてある色と、種類）。
    private func paintAll(_ all: [Slot]) -> [Slot] {
        CalPrefs.refreshShelfColors()
        var out = all
        var seen: [String: (String, String)] = [:]
        for i in out.indices {
            let shelf = out[i].isPhone ? CalPrefs.shelfHex(out[i].from) : ""
            let key = [out[i].color, shelf, out[i].kind].joined(separator: "\u{1}")
            if let got = seen[key] {
                out[i].paint = got.0
                out[i].paintHex = got.1
                continue
            }
            guard let got = try? Cian.call("caltint", [
                "color": out[i].color, "shelf": shelf,
                "away": out[i].isAway, "here": out[i].isPhone, "plan": out[i].isPlan,
            ]) else { continue }
            let word = got["from"] as? String ?? ""
            let hex = got["hex"] as? String ?? ""
            seen[key] = (word, hex)
            out[i].paint = word
            out[i].paintHex = hex
        }
        return out
    }

    /// **自分の色の名前**（依頼 702）── ノートの前書きにも、予定表の
    /// メモ欄にも、同じ名前を書く。
    ///
    /// **決めていなければ、何も書かない。** 既定の色を書き込むと、
    /// 選んでいない人の予定にまで色の名前が入る（グループの予定なら、
    /// その字は相手の Google カレンダーにも出る）。それに、どちらの端末も
    /// 既定のままなら**二人が同じ色になる**ので、書いても見分けは付かない。
    /// 色で見分けたい人は設定で選ぶ ── 本人が「変更できるようにしたい」と
    /// 言ったのは、そこのこと。
    private func myColorName() -> String {
        CalPrefs.hereColor.isEmpty ? "" : CalPrefs.colorName(CalPrefs.hereColor)
    }

    /// この予定に書き込む色の名前（依頼 713）。決めるのは
    /// `CalPrefs.colorToWrite` ── 総ざらいから呼べるように画面の外にある。
    private func chosenColor() -> String {
        CalPrefs.colorToWrite(chosen: newColor)
    }

    /// 足す。**この iPhone の予定表が使えるならそちらへ**（依頼 460）──
    /// 普通のカレンダーとして期待されるのはこちら。使えないときだけ、
    /// これまでどおりノートを作る（そう書いてある）。
    private func add() {
        let title = newTitle.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty else { return }
        let at = newAt.trimmingCharacters(in: .whitespacesAndNewlines)
        let end = newEnd.trimmingCharacters(in: .whitespacesAndNewlines)
        if Phone.allowed {
            // **選ばれた行き先に入れる**（依頼 545）。グループならそのカレンダーへ
            // 書き、だれの用事かをメモ欄の最後の行に置く（文字を作るのは core の
            // `caltagset` ── デスクトップ版と同じ1 つに訊く）。
            let toGroup = newToGroup == true && !groupName.isEmpty
            // **色と「全員に通知する」も、メモ欄のいちばん最後の行に置く**
            // （依頼 701・702・本人が甲を選んだ・2026-10-31）。
            //
            // 端末の予定表に入る予定は**ノートを持たない**ので、前書きに
            // 書く先が無い。ここに置くと予定そのものに付いて旅をするので、
            // 同期した相手の amber でも同じ色・同じ通知になる。
            // 文字を作るのは core の `caltagset` ── デスクトップ版と同じ
            // 一か所に訊く。
            //
            // **色はいつも書く。** 書いておけば、あとで設定の色を変えても
            // 古い予定の色は動かない（ノートの道と同じ約束）。
            var notes = ""
            var ask: [String: Any] = [
                "notes": "", "tags": toGroup ? newTags : [], "color": chosenColor(),
            ]
            if newAlarmFor == "全員" && !newAlarm.isEmpty { ask["alarm"] = newAlarm }
            // タグが書けなくても、予定そのものは登録する。
            if let out = try? Cian.call("caltagset", ask) {
                notes = out["notes"] as? String ?? ""
            }
            do {
                try Phone.add(title: title, day: picked, at: at.isEmpty ? nil : at,
                              end: end.isEmpty ? nil : end,
                              until: newUntil.isEmpty ? nil : newUntil,
                              alarm: newAlarm.isEmpty ? nil : newAlarm,
                              notes: notes, into: toGroup ? groupName : "")
                count()
            } catch {
                trouble = error.localizedDescription
            }
            return
        }
        let when = at.isEmpty ? picked : picked + " " + at
        do {
            guard let made = try store.make(titled: title) else {
                trouble = "作れません"
                return
            }
            let (text, stamp) = try store.open(made)
            var body = text
            // **前書きに置くのは、入れたものだけ**（依頼 700・701）── 空の鍵を
            // 残すと、読む側が毎回「終わりがある」「アラームがある」と数える。
            // **自分の色を書き込む**（依頼 702）── これがあるので、
            // 同期した先でも同じ色で出る。設定を変えても古い予定の色は
            // 変わらない（書いた時点の色が、そのノートの色）。
            let mine = chosenColor()
            for (key, value) in [("remind", when), ("until", newUntil),
                                 ("alarm", newAlarm), ("alarm-who", newAlarmFor),
                                 ("color", mine)]
            where !value.isEmpty {
                let out = try Cian.call("setfield", ["text": body, "key": key, "value": value])
                body = out["text"] as? String ?? body
            }
            _ = try store.save(made, text: body, stamp: stamp)
            store.reload()
            count()
        } catch {
            trouble = error.localizedDescription
        }
    }
}

/// カレンダーが訊く3 つ（読めません・足す・直す）。
///
/// 画面から出したのは、**一つの `body` に積むと型検査が終わらない**から
/// ── 見た目の都合ではなく、組めるかどうかの都合。
private struct Asking: ViewModifier {
    @Binding var trouble: String?
    @Binding var adding: Bool
    @Binding var editing: Calendaring.Slot?
    /// 色を選ばせているところ（依頼 712）。
    @Binding var coloring: Calendaring.Slot?
    @Binding var newTitle: String
    @Binding var newAt: String
    @Binding var newEnd: String
    @Binding var editTitle: String
    @Binding var newToGroup: Bool?
    @Binding var newTags: [String]
    @Binding var newUntil: String
    @Binding var newAlarm: String
    @Binding var newAlarmFor: String
    /// 作る予定だけの色の名前（依頼 713）。
    @Binding var newColor: String
    let day: String
    let toPhone: Bool
    let groupName: String
    let add: () -> Void
    let rename: () -> Void
    let drop: () -> Void
    /// その予定だけの色を書き換える（依頼 712）。空ならやめる。
    let recolor: (String) -> Void

    func body(content: Content) -> some View {
        content
            .alert("読めません", isPresented: Binding(
                get: { trouble != nil }, set: { if !$0 { trouble = nil } })
            ) { Button("閉じる") {} } message: { Text(trouble ?? "") }
            // **時刻は打たせない**（依頼 493 のiPhone の側・本人「手打ちすると絶対ミスする」）
            // ── デスクトップ版と同じダイアログ1 つ: タイトル・終日・開始・終了（十五分刻み）。
            .sheet(isPresented: $adding) {
                EventForm(day: day, toPhone: toPhone, groupName: groupName,
                          title: $newTitle, at: $newAt, end: $newEnd,
                          toGroup: $newToGroup, tags: $newTags,
                          untilDay: $newUntil, alarmAt: $newAlarm, alarmFor: $newAlarmFor,
                          color: $newColor,
                          add: add)
                    .presentationDetents([.large])
            }
            // この iPhone の予定は、**押したら直せる**（よその予定表と
            // 違って、書き戻す口がある）。
            .alert("予定を修正する", isPresented: Binding(
                get: { editing != nil }, set: { if !$0 { editing = nil } })
            ) {
                TextField("タイトル", text: $editTitle)
                Button("修正する") { rename() }
                // **この予定だけ、別の色にする**（依頼 712・本人「タスクや
                // 予定に対して、デフォルトと違う色を指定することができる
                // ようにしてほしい」）。色を読む仕掛けは依頼 702 で入って
                // いたが、**人が選ぶ口が無かった** ── amber が書くのは
                // いつも「自分の色」だけで、押せるボタンが無かった。
                //
                // 色を並べるのは別の紙にする ── この紙は題を打つところで、
                // 八つのボタンを足すと打つ欄が押し出される。
                Button("色を変える") { coloring = editing }
                Button("削除する", role: .destructive) { drop() }
                Button("やめる", role: .cancel) {}
            } message: {
                Text("この iPhone の予定表のものです。")
            }
            // **色を選ぶ**（依頼 712）。閉じるときに `coloring` を空にする。
            .confirmationDialog("この予定の色", isPresented: Binding(
                get: { coloring != nil }, set: { if !$0 { coloring = nil } }),
                titleVisibility: .visible
            ) {
                ForEach(CalPrefs.colors, id: \.hex) { c in
                    Button(c.name) { recolor(c.name) }
                }
                // **元に戻す道を残す。** 選んだあとに「もとはどうだったか」
                // へ帰れないのは事故。空を書くと、予定表の色に落ちる。
                Button("この予定だけの色をやめる") { recolor("") }
                Button("やめる", role: .cancel) {}
            } message: {
                Text("選んだ色は予定そのものに付くので、同期している相手の amber でも同じ色で出ます")
            }
    }
}

/// **予定を登録するダイアログ**（デスクトップ版の `askEvent` と同じ四つ）。開始を選ぶと終了は一時間後、
/// 終日にマークを入れたら時刻は選べない、空のままなら登録できない。
struct EventForm: View {
    let day: String
    let toPhone: Bool
    /// グループカレンダーの名前（空なら、まだ作っていない）。
    let groupName: String
    @Binding var title: String
    @Binding var at: String
    @Binding var end: String
    @Binding var toGroup: Bool?
    @Binding var tags: [String]
    /// いつまで（依頼 700）。`yyyy-MM-dd`。日をまたがないなら空。
    @Binding var untilDay: String
    /// アラームと、鳴らす人（依頼 701）。付けないなら空。
    @Binding var alarmAt: String
    @Binding var alarmFor: String
    /// この予定だけの色の名前（依頼 713）。空なら「自分の色」。
    @Binding var color: String
    let add: () -> Void
    /// **選びは、このダイアログが持つ**（依頼 547）── `@Binding` を直に触ると、
    /// 一つ選ぶたびに外の画面まで描き直されて、選んだマークが消える（画面で出た）。
    /// 外に渡すのは「登録する」を押したときだけ。
    @State private var toG: Bool?
    @State private var who: [String] = []
    @Environment(\.dismiss) private var dismiss
    @State private var allDay = false
    @State private var start = "09:00"
    @State private var till = "10:00"
    @State private var said = ""
    @State private var known: [String] = []
    /// グループカレンダーが、この端末に降りてきているか（依頼 545）。
    /// **名前を憶えているだけでは足りない** ── 作った直後や、Google の
    /// アカウントをまだ iPhone に足していないうちは、予定表そのものが無い。
    /// 出してしまうと、選んで「登録する」を押した先で「予定表がありません」
    /// になる ── 選ばせておいて断るのがいちばん悪い。
    @State private var canGroup = false
    @State private var naming = false
    @State private var typed = ""
    /// 終わりの日とアラーム（依頼 699・まだ絵を見てもらうところ）。
    @State private var manyDays = false
    @State private var until = Date()
    @State private var alarm = "なし"
    @State private var alarmWho = "自分だけ"
    static let alarms = ["なし", "5分前", "10分前", "30分前", "1時間前", "前日"]

    static let times: [String] = (0..<24).flatMap { h in ["00", "15", "30", "45"].map { String(format: "%02d:", h) + $0 } }

    var body: some View {
        NavigationStack {
            Form {
                // **どこに入れるかを、いちばん先に選ばせる**（依頼 545・デスクトップ版と同じ丙）。
                // グループカレンダーが無ければ、選ぶものが無いので出さない。
                if canGroup {
                    Section("どこに入れる") {
                        Picker("どこに入れる", selection: $toG) {
                            Text("自分だけ").tag(Bool?.some(false))
                            Text("グループと共有").tag(Bool?.some(true))
                        }
                        .pickerStyle(.segmented)
                        .labelsHidden()
                    }
                    // **だれの用事かは、グループに出すときだけ訊く** ── 誰にも
                    // 見えない予定に、誰の用事かを書く意味がない。
                    if toG == true {
                        Section("だれの") {
                            ForEach(known, id: \.self) { t in
                                Button {
                                    if let i = who.firstIndex(of: t) { who.remove(at: i) } else { who.append(t) }
                                } label: {
                                    HStack {
                                        Circle().fill(Color(hex: CalPrefs.laneColor(t, among: known)) ?? .gray)
                                            .frame(width: 10, height: 10)
                                        Text(t).foregroundStyle(.primary)
                                        Spacer()
                                        if who.contains(t) { Image(systemName: "checkmark").foregroundStyle(.tint) }
                                    }
                                    // **行のどこを押しても効く。** `.plain` に
                                    // すると押せるのは文字のところだけになり、
                                    // 名前の右の空きを押しても何も起きない
                                    // （画面で出た ── 押したのに付かなかった）。
                                    .contentShape(Rectangle())
                                }
                                // **選んでいない名前を、選んだ色で出さない** ──
                                // `Button` の文字は既定で琥珀になるので、全部が
                                // 選ばれているように見える（実機の前に画面で出た）。
                                .buttonStyle(.plain)
                            }
                            Button("＋ 名前を追加") { typed = ""; naming = true }
                        }
                    }
                }
                Section {
                    TextField("タイトル", text: $title)
                } footer: {
                    Text(toPhone ? "\(day) の予定表に登録します" : "\(day) に、ノートが一件できます")
                }
                Section {
                    Toggle("終日", isOn: $allDay)
                    Picker("開始", selection: $start) { ForEach(Self.times, id: \.self) { Text($0).tag($0) } }
                        .disabled(allDay)
                        .onChange(of: start) { _, now in till = Self.plus(now, 60) }
                    Picker("終了", selection: $till) { ForEach(Self.times, id: \.self) { Text($0).tag($0) } }
                        .disabled(allDay)
                    // **「日をまたぐ」と「いつまで」**（依頼 699・本人が字を決めた）。
                    //
                    // はじめ「何日もつづく」「終わりの日」と書いたが、本人に
                    // 止められた ── **「終わりの日」は世の終わりに読める**し、
                    // 「何日もつづく」はうんざりしている響きがある。
                    // 「日をまたぐ」なら、終日の何日間も、22:00〜翌 02:00 のような
                    // 夜をまたぐ予定も、同じ一言で読める。
                    //
                    // **畳まない**（本人が絵を見て甲を選んだ）── アラームは
                    // ついでに付けるもので、一回押さないと見えない場所にあると
                    // 使われない。全部ひらいても一画面に収まることを画面で確かめた。
                    Toggle("日をまたぐ", isOn: $manyDays)
                    if manyDays {
                        DatePicker("いつまで", selection: $until, displayedComponents: .date)
                    }
                } footer: {
                    if !said.isEmpty { Text(said).foregroundStyle(.red) }
                }
                Section {
                    Picker("アラーム", selection: $alarm) {
                        ForEach(Self.alarms, id: \.self) { Text($0).tag($0) }
                    }
                    if alarm != "なし" {
                        // **「鳴らす」とは言わない**（依頼 701・本人「鳴らすは変だ」）
                        // ── 鳴るのは端末で、人ではない。切っておけば自分の端末
                        // だけ、入れれば同期している全員の端末に届く。
                        // 前書きの `alarm-who` に残るので、変えれば全員に伝わる。
                        Toggle("全員に通知する", isOn: Binding(
                            get: { alarmWho == "全員" },
                            set: { alarmWho = $0 ? "全員" : "自分だけ" }))
                    }
                } footer: {
                    // **誰に届くかだけ言う**（依頼 701・本人が字を決めた）。
                    // 「次の同期で届く」という時間差もあるが、本人が自明と
                    // 判断した ── 実際に困ったら、そのとき足す。
                    if alarmWho == "全員" { Text("同期相手に通知されます") }
                }
                // **作るときに色を選べる**（依頼 713・本人が決めた）。
                //
                // はじめは足さなかった ── 既定（自分の色）で十分に動くし、
                // 作る画面に一行増えると毎日押すものが一行ぶん下がる。
                // **本人が「足す」と決めた**ので足した。
                //
                // 既定を先頭に置く ── ほとんどの予定はそれで、選ばなければ
                // いままでと同じ色で入る。
                Section {
                    Picker("色", selection: $color) {
                        Text("自分の色").tag("")
                        ForEach(CalPrefs.colors, id: \.hex) { c in
                            Text(c.name).tag(c.name)
                        }
                    }
                } footer: {
                    Text(color.isEmpty
                         ? "設定の「自分の色」で塗ります"
                         : "この予定だけ「\(color)」で塗ります。あとから変えられます")
                }
            }
            .navigationTitle("予定を登録する")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("やめる") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    // 選ぶまで登録できない（依頼 545）。**押せないボタンは
                    // 押せないように見せる** ── 見た目が同じだと「押しても
                    // 何も起きない」になる。
                    Button("登録する") { go() }
                        .bold()
                        .disabled(canGroup && toG == nil)
                }
            }
            .alert("だれの用事ですか", isPresented: $naming) {
                TextField("名前を入力してください（例: 太郎）", text: $typed)
                Button("やめる", role: .cancel) {}
                Button("追加") {
                    let name = typed.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !name.isEmpty, !name.contains(" "), !name.contains("　") else { return }
                    CalPrefs.rememberTag(name)
                    known = CalPrefs.tagsKnown
                    if !who.contains(name) { who.append(name) }
                }
            }
            .onAppear {
                allDay = at.isEmpty
                if !at.isEmpty { start = at }
                till = end.isEmpty ? Self.plus(start, 60) : end
                known = CalPrefs.tagsKnown
                canGroup = toPhone && !groupName.isEmpty && Phone.calendars.contains(groupName)
                toG = nil
                who = []
                // **開くたびに「自分の色」へ戻す**（依頼 713）── 前に一度
                // 選んだ色が黙って次の予定にも付くと、気づかないまま
                // 色の意味が崩れる。
                color = ""
            }
        }
    }

    private func go() {
        let t = title.trimmingCharacters(in: .whitespacesAndNewlines)
        if t.isEmpty { said = "タイトルを入れてください"; return }
        if !allDay && till <= start { said = "終了は開始より後にしてください"; return }
        at = allDay ? "" : start
        end = allDay ? "" : till
        toGroup = toG
        tags = who
        // **日をまたがないなら、鍵そのものを書かない**（依頼 700）── 空の
        // `until:` が前書きに残ると、読む側は「終わりがある」と思って毎回数える。
        untilDay = manyDays ? Self.ymd(until) : ""
        alarmAt = alarm == "なし" ? "" : alarm
        // **自分だけなら、鍵を書かない**（依頼 701）── 既定を前書きに
        // 並べても、読む側の仕事が増えるだけ。
        alarmFor = (alarm == "なし" || alarmWho != "全員") ? "" : "全員"
        add()
        dismiss()
    }

    /// `Date` を `yyyy-MM-dd` に（前書きに書く形）。
    static func ymd(_ d: Date) -> String {
        let f = DateFormatter()
        f.calendar = Calendar(identifier: .gregorian)
        f.dateFormat = "yyyy-MM-dd"
        return f.string(from: d)
    }

    static func plus(_ t: String, _ min: Int) -> String {
        let p = t.split(separator: ":").compactMap { Int($0) }
        guard p.count == 2 else { return t }
        let n = Swift.min(p[0] * 60 + p[1] + min, 23 * 60 + 45)
        return String(format: "%02d:%02d", n / 60, n % 60)
    }
}
