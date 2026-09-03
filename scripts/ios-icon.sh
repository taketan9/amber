#!/usr/bin/env bash
# cian.ico から iOS のアプリアイコンを作る。
#
#   ./scripts/ios-icon.sh
#
# **iOS はアイコンに自分で角丸を被せる。** 角が透明な絵をそのまま渡すと角が
# 二重に丸まり、透明だったところが黒くなる。背景を単色で塗る手もあるが、cian の
# アイコンは斜めの階調なので角だけ色が浮く。**1.18 倍にしてから中央を 1024 で
# 切り出す** ── 丸い角は画布の外に出て、階調は端まで続く。
#
# アルファは JPEG を経由して落とす。App Store はアルファのあるアイコンを
# 受け付けず、`sips` にアルファだけ外す指定が無いため。品質は 100 で通す。
#
# AppKit で書く手は捨てた: `swift` のスクリプトから `NSImage.draw(in:)` は
# 何も描かず、真っ黒な 1024×1024 が二度できた。
set -euo pipefail
cd "$(dirname "$0")/.."

SRC=cian.ico
OUT=ios/Cian/Assets.xcassets/AppIcon.appiconset/AppIcon.png
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

sips -s format png "$SRC" --out "$TMP/a.png" >/dev/null
sips -Z 1208 "$TMP/a.png" >/dev/null            # 1024 * 1.18
sips -c 1024 1024 "$TMP/a.png" >/dev/null       # 中央を正方形に
sips -s format jpeg -s formatOptions 100 "$TMP/a.png" --out "$TMP/b.jpg" >/dev/null
sips -s format png "$TMP/b.jpg" --out "$OUT" >/dev/null

# 出来たものが本当に正方形でアルファ無しか。どちらも外すとアイコンが黒くなる。
read -r W H A < <(sips -g pixelWidth -g pixelHeight -g hasAlpha "$OUT" \
  | awk '/pixelWidth/{w=$2} /pixelHeight/{h=$2} /hasAlpha/{a=$2} END{print w, h, a}')
[ "$W" = 1024 ] && [ "$H" = 1024 ] || { echo "大きさが違います: ${W}x${H}"; exit 1; }
[ "$A" = "no" ] || { echo "アルファが残っています"; exit 1; }
echo "できました: $OUT (${W}x${H}, アルファなし)"
