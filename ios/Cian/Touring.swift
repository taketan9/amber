import SwiftUI

/// ノートの目次（電話）。
///
/// **何が見出しかは core が決める**（`note::blocks`）── 電話が `#` を
/// 数えはじめると、`#仕事` というタグの行が目次に出る（空白の有無で決まる）
/// し、窓と別のものが並ぶ。ここは並べて、飛ぶだけ。
///
/// 窓は右に居座る列だが、**電話は畳む**。393pt の幅で常に一列取ると、
/// ノートそのものが半分になる ── 目次は「いま全体のどこか」を確かめる
/// ために一瞬開くもので、書いている間ずっと見るものではない。
struct Touring: View {
    /// 見出しだけ（`kind == "heading"`）。
    let heads: [Block]
    /// 飛び先の行 ── ファイルの行番号（前書きを含む）。
    let go: (Int) -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Group {
                if heads.isEmpty {
                    ContentUnavailableView {
                        Label("見出しがありません", systemImage: "list.bullet.indent")
                    } description: {
                        Text("行の頭に `#` と空白を置くと見出しになります（下の帯の「見出し」でも入ります）。")
                    }
                } else {
                    List(heads) { h in
                        Button {
                            go(h.line)
                            dismiss()
                        } label: {
                            HStack(spacing: 0) {
                                // 深さは字下げそのもので見せる ── 「2」と
                                // 書くより、目が形で拾う。
                                Spacer().frame(width: CGFloat(max(0, h.level - 1)) * 14)
                                Text(h.text)
                                    .font(h.level <= 1 ? .body.weight(.semibold) : .body)
                                    .foregroundStyle(h.level <= 2
                                        ? AnyShapeStyle(.primary) : AnyShapeStyle(.secondary))
                                    .lineLimit(1)
                                Spacer(minLength: 0)
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            .navigationTitle("目次")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { Button("閉じる") { dismiss() } }
            }
        }
        .presentationDetents([.medium, .large])
    }
}
