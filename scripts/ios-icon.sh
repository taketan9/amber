#!/usr/bin/env bash
# iOS のアプリアイコンを焼く。
#
#   ./scripts/ios-icon.sh
#
# **中身は `packaging/amber.py` に移した。** ここは扉を1つに保つためだけに残る
# ── 以前は `cian.ico`（二画面ファイラの印）を iOS のアイコンにしていたので、
# このまま走らせると amber のアイコンを cian のもので上書きしてしまう。
#
# 移す前にここに書いてあったことのうち、まだ効いているもの:
#
#   * **iOS はアイコンに自分で角丸を被せる。** 角が透明な絵を渡すと角が二重に
#     丸まり、透明だったところが黒くなる。だから 1024 は**角丸なしの真四角**で
#     書く（amber.py の `render(..., square=True)`）。以前ここでやっていた
#     「1.18 倍して中央を切る」小細工は、そのぶん要らなくなった。
#   * **App Store はアルファのあるアイコンを受け付けない。** 以前は sips で
#     JPEG を経由してアルファを落としていたが、いまは PNG を色型 2（RGB）で
#     書くので経由が要らない。劣化もしない。
#   * AppKit で書く手は捨ててある: `swift` のスクリプトから `NSImage.draw(in:)`
#     は何も描かず、真っ黒な 1024×1024 が二度できた。
set -euo pipefail
cd "$(dirname "$0")/.."

python3 packaging/amber.py

OUT=ios/Cian/Assets.xcassets/AppIcon.appiconset/AppIcon.png
# 出来たものが本当に正方形でアルファ無しか。どちらも外すとアイコンが黒くなる。
read -r W H A < <(sips -g pixelWidth -g pixelHeight -g hasAlpha "$OUT" \
  | awk '/pixelWidth/{w=$2} /pixelHeight/{h=$2} /hasAlpha/{a=$2} END{print w, h, a}')
[ "$W" = 1024 ] && [ "$H" = 1024 ] || { echo "大きさが違います: ${W}x${H}"; exit 1; }
[ "$A" = "no" ] || { echo "アルファが残っています"; exit 1; }
echo "確かめました: $OUT (${W}x${H}, アルファなし)"
