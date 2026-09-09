#!/bin/zsh
# **mac の予定表に話しかける道具**を組む（依頼 462）。
#
#     scripts/mac-build.sh
#
# 出来上がりは `target/mac/amber-cal`。窓（`gui/main.js`）がこれを呼ぶ。
# Rust ではなく Swift なのは、**EventKit が Apple のものだから** ── 電話と
# 同じ枠組みを使えば、窓と電話で同じ答えになる。
set -e
here="${0:A:h}"
root="${here:h}"
mkdir -p "$root/target/mac"
# **名札を実行ファイルの中に埋める。** macOS は、なぜ予定表が要るのかを
# 持っていない素の実行ファイルを**小窓も出さずに断る** ── 埋めていなかった
# ときは「許可しますか」が一度も出なかった（依頼 466）。
xcrun swiftc -O -swift-version 5 \
  -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __info_plist \
  -Xlinker "$root/mac/amber-cal.plist" \
  -o "$root/target/mac/amber-cal" "$root/mac/amber-cal.swift"

# **署名する。** 署名の無いものへの許可は、macOS が憶えていられない
# （毎回訊かれるか、そもそも通らない）。
codesign --force --sign - "$root/target/mac/amber-cal"
echo "できました: target/mac/amber-cal"
