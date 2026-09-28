import SwiftUI

/// **はじめの案内**（依頼 656）── 初めての人に、一覧を上から四つと、書く道具の
/// 帯を一つ。デスクトップ版の `TOUR` と**同じ五つ・同じ文**（本人が書いた）。
///
/// 指す先は、一覧の並びがデスクトップ版の左の列と同じ順に揃えてある（依頼
/// 505・247）ので、そのまま当て直せる。
///
/// **次へを押すまで進めない**（本人が決めた）ので、後ろは押せない。
///
/// **指す先の位置は、それぞれの画面が自分で書き込む**（`tourAnchor`）──
/// SwiftUI の preference は、押し出した先（`navigationDestination`）から
/// 根まで流れてこない。最後の段だけノートの画面に居るので、そこだけ
/// 取りこぼす形になる。
@MainActor
final class Tour: ObservableObject {
    static let shared = Tour()

    /// 憶える鍵。**新しい鍵は `amber.` で始める**（`cian.` は消えると困るので
    /// 残しているだけ）。
    static let touredKey = "amber.toured"
    static var seen: Bool { UserDefaults.standard.bool(forKey: touredKey) }

    struct Step {
        /// 指す先の名前。`nil` なら真ん中に置く。
        let at: String?
        let say: String
        /// 会社向けのビルドでも出すか（依頼 602 で共有は閉じている）。
        let office: Bool
        /// 先にノートを開く段か。
        let opens: Bool

        init(at: String?, say: String, office: Bool = true, opens: Bool = false) {
            self.at = at
            self.say = say
            self.office = office
            self.opens = opens
        }
    }

    /// **デスクトップ版の `TOUR` と一字一句そろえる。** 家族で両方を使うとき、
    /// 説明が食い違わないように。
    ///
    /// **三つ目に指す先が無いのは、わざと。** 共有の段は持っている人の一覧に
    /// しか出ないので、初めての人の画面には無い ── 無いものを指すより、
    /// 真ん中に置いて読ませる。文も場所ではなく「何に使えるか」を言っている。
    static let steps: [Step] = [
        Step(at: "cal", say: "予定表はここから。"),
        Step(at: "new", say: "ノートはここから。"),
        Step(at: nil,
             say: "家族との買い物リストや、メモの共有に便利です。\nMarkdownの記法を知らなくても、ボタンでかんたんに書けます。",
             office: false),
        Step(at: "book", say: "フォルダとタグで整理できます。"),
        Step(at: "marks", say: "見出し、表、チェックなど、ここから押すだけ。", opens: true),
    ]

    @Published private(set) var on = false
    @Published private(set) var n = 0
    /// 最後の段のために、ノートを開いてほしい ── 一覧の画面が見て開く。
    @Published private(set) var wantsNote = false

    /// 開いたノートが描かれるのを待っているあいだ、幕を出さない。
    ///
    /// **待たないと、真っ白なノートの上に輪が出る**（2026-09-28・シミュレータで
    /// 見た）── 「表示」画面は `WKWebView` で、押し出してから最初の絵が出るまで
    /// 一拍ある。そこへ幕をかぶせると、何も無いところを指しているように見える。
    @Published private(set) var waiting = false

    /// ノートを開いた ── 少し待ってから幕を出す。
    func opened() {
        guard on, waiting else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.45) { [weak self] in
            self?.waiting = false
        }
    }

    private(set) var plan: [Step] = []

    /// 指す先の位置（画面ぜんたいの座標）。**`@Published` にしない** ──
    /// 組み立ての最中に書き込むので、published だと「表示中に状態を変えた」と
    /// 叱られる。代わりに、いま指している先が動いたときだけ [`moved`] を叩く。
    private(set) var frames: [String: CGRect] = [:]

    /// 指している先が動いた回数。**これが無いと、古い場所に輪が残る。**
    ///
    /// 実際に残った（2026-09-28・シミュレータ）── 一覧の上に「まだ同期して
    /// いません」の帯が後から挟まって行が下がったのに、輪は帯の上に掛かった
    /// ままだった。組み立ての途中では場所が決まっていないので、**一度でも
    /// 読んだら終わり、にはできない。**
    @Published private(set) var moved = 0

    /// 指す先の場所を憶える。動いていたら、次の回で描き直してもらう。
    func note(_ name: String, _ rect: CGRect?) {
        if let rect {
            if frames[name] == rect { return }
            frames[name] = rect
        } else {
            if frames[name] == nil { return }
            frames[name] = nil
        }
        guard on, step?.at == name else { return }
        // **組み立ての外で叩く** ── その場で `@Published` を触ると
        // 「表示中に状態を変えた」と叱られる。
        DispatchQueue.main.async { [weak self] in self?.moved += 1 }
    }

    var step: Step? { plan.indices.contains(n) ? plan[n] : nil }
    var count: Int { plan.count }

    /// いまの段が指している枠。無ければ `nil`（真ん中に置く）。
    func frame() -> CGRect? {
        guard let at = step?.at else { return nil }
        return frames[at]
    }

    /// 案内を出す。`forced` なら、見たことがあっても出す（設定から呼ぶとき）。
    ///
    /// **指す先が画面に無い段は、黙って飛ばす。** 止まると、次へを押すまで
    /// 進めない案内が行き止まりになる。**一段も残らなければ、何も出さない。**
    func start(office: Bool, forced: Bool) {
        guard !on else { return }
        plan = Self.steps.filter { $0.office || !office }
        guard !plan.isEmpty else {
            UserDefaults.standard.set(true, forKey: Self.touredKey)
            return
        }
        n = 0
        on = true
        waiting = plan[0].opens
        wantsNote = plan[0].opens
    }

    /// 次へ。最後なら片づく。
    func next() {
        guard on else { return }
        var i = n + 1
        // 指す先の無くなった段は飛ばす（枠を書き込んでいない画面がある）。
        while i < plan.count, let at = plan[i].at, frames[at] == nil, !plan[i].opens {  // 指す先が無い段は飛ばす
            i += 1
        }
        if i >= plan.count { shut(); return }
        n = i
        waiting = plan[i].opens
        wantsNote = plan[i].opens
    }

    /// やめる／終わり。**憶えるのは片したとき。**
    func shut() {
        guard on else { return }
        on = false
        wantsNote = false
        waiting = false
        UserDefaults.standard.set(true, forKey: Self.touredKey)
    }
}

/// 指す先に名札を付ける。位置は画面ぜんたいの座標で憶える。
struct TourAnchor: ViewModifier {
    let name: String

    func body(content: Content) -> some View {
        content.background(
            GeometryReader { g in
                Color.clear
                    .onAppear { Tour.shared.note(name, g.frame(in: .global)) }
                    .onChange(of: g.frame(in: .global)) { _, now in
                        Tour.shared.note(name, now)
                    }
                    .onDisappear { Tour.shared.note(name, nil) }
            }
        )
    }
}

extension View {
    /// はじめの案内が指す先（依頼 656）。
    func tourAnchor(_ name: String) -> some View { modifier(TourAnchor(name: name)) }
}

/// 案内と一覧の画面をつなぐ二つの見張り。
///
/// **一つにまとめてある。** `ContentView` の本体に `onChange` を二つ足したら、
/// Swift の型検査が「時間内に解けない」と音を上げた（2026-09-28）── あの本体は
/// 既に十分大きい。
struct TourHooks: ViewModifier {
    @ObservedObject var tour = Tour.shared
    /// 最後の段のために、サンプルを一本開く。
    let openSample: () -> Void
    /// 案内が終わったあとにすること（通知を訊くのはここ ── 依頼 654）。
    let afterTour: () -> Void

    func body(content: Content) -> some View {
        content
            .onChange(of: tour.on) { was, now in
                if was, !now { afterTour() }
            }
            .onChange(of: tour.wantsNote) { _, want in
                if want { openSample() }
            }
    }
}

/// 案内の幕。**アプリのいちばん外側に一枚**だけ掛ける ── 押し出した先の
/// 画面にも同じ幕が要るので、根に置いて全部を覆う。
struct TourVeil: View {
    @ObservedObject var tour = Tour.shared

    var body: some View {
        // **測る枠と、描く枠を同じにする。**
        //
        // `ignoresSafeArea` を中の重なりだけに付けていたら、**輪が 60pt ほど
        // 上にずれた**（2026-09-28・シミュレータで見た）── 測るほうは安全域の
        // 下から始まっていて、描くほうは画面のてっぺんから始まっていたので、
        // その差がそのまま輪のずれになった。外側に付けて、どちらも画面ぜんたいに。
        GeometryReader { screen in
            if tour.on, !tour.waiting, let step = tour.step {
                // 指す先は画面ぜんたいの座標で憶えてあるので、この枠の
                // 座標へ直してから使う。
                let mine = screen.frame(in: .global)
                let hole = tour.frame().map {
                    $0.offsetBy(dx: -mine.minX, dy: -mine.minY).insetBy(dx: -4, dy: -4)
                }
                ZStack(alignment: .topLeading) {
                    // **幕は四枚の板で作る。** 型で抜く書き方（`blendMode`）は
                    // 端末と装いで出方が変わるうえ、ここは一度も目で見られない
                    // （シミュレータでしか出せない）── 予想どおりに描けるほうを取る。
                    shade(hole, screen.size)
                    if let hole {
                        RoundedRectangle(cornerRadius: 10)
                            .strokeBorder(Color.accentColor, lineWidth: 2)
                            .frame(width: hole.width, height: hole.height)
                            .position(x: hole.midX, y: hole.midY)
                            .allowsHitTesting(false)
                    }
                    bubble(step)
                        .frame(width: min(300, screen.size.width - 40))
                        .position(spot(hole, screen.size))
                }
                // **後ろは押せない**（次へを押すまで進めない）。
                .contentShape(Rectangle())
                .onTapGesture {}
            }
        }
        .ignoresSafeArea()
    }

    /// 幕 ── 指す先のまわりを四枚で囲う。指す先が無ければ一枚で覆う。
    @ViewBuilder
    private func shade(_ hole: CGRect?, _ size: CGSize) -> some View {
        let ink = Color.black.opacity(0.5)
        if let h = hole {
            ZStack(alignment: .topLeading) {
                ink.frame(width: size.width, height: max(0, h.minY)).position(
                    x: size.width / 2, y: max(0, h.minY) / 2)
                ink.frame(width: size.width, height: max(0, size.height - h.maxY)).position(
                    x: size.width / 2, y: (h.maxY + size.height) / 2)
                ink.frame(width: max(0, h.minX), height: h.height).position(
                    x: max(0, h.minX) / 2, y: h.midY)
                ink.frame(width: max(0, size.width - h.maxX), height: h.height).position(
                    x: (h.maxX + size.width) / 2, y: h.midY)
            }
        } else {
            ink
        }
    }

    private func bubble(_ step: Tour.Step) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(step.say).font(.callout).fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 10) {
                Text("\(tour.n + 1) / \(tour.count)").font(.caption2).foregroundStyle(.secondary)
                Spacer(minLength: 0)
                // **やめるは常にある。** 塞ぐと、指す先を見失った人にできることが
                // 無くなる。
                Button("やめる") { tour.shut() }
                    .font(.caption).foregroundStyle(.secondary)
                Button(tour.n == tour.count - 1 ? "はじめる" : "次へ") { tour.next() }
                    .font(.footnote.weight(.semibold))
                    .buttonStyle(.borderedProminent).controlSize(.small)
            }
        }
        .padding(13)
        .background(Color(.systemBackground), in: RoundedRectangle(cornerRadius: 13))
        .shadow(color: .black.opacity(0.28), radius: 14, y: 6)
    }

    /// 吹き出しの場所 ── 指す先の下、入らなければ上。**画面の中へ押し戻す。**
    private func spot(_ hole: CGRect?, _ size: CGSize) -> CGPoint {
        let w = min(300, size.width - 40)
        guard let hole else { return CGPoint(x: size.width / 2, y: size.height / 2) }
        let h: CGFloat = 130
        var y = hole.maxY + 14 + h / 2
        if y + h / 2 > size.height - 20 { y = hole.minY - 14 - h / 2 }
        y = min(max(y, h / 2 + 20), size.height - h / 2 - 20)
        let x = min(max(hole.midX, w / 2 + 16), size.width - w / 2 - 16)
        return CGPoint(x: x, y: y)
    }
}
