#!/bin/zsh
# **mac の予定表に話しかける道具**をビルドする（依頼 462）。
#
#     scripts/mac-build.sh
#
# 出来上がりは `target/mac/amber-cal`。ウィンドウ（`gui/main.js`）がこれを呼ぶ。
# Rust ではなく Swift なのは、**EventKit が Apple のものだから** ── iPhone と
# 同じ枠組みを使えば、デスクトップ版と iPhone で同じ答えになる。
set -e
here="${0:A:h}"
root="${here:h}"
mkdir -p "$root/target/mac"
# **名札を実行ファイルの中に埋める。** macOS は、なぜ予定表が要るのかを
# 持っていない素の実行ファイルを**小デスクトップ版も出さずに断る** ── 埋めていなかった
# ときは「許可しますか」が一度も出なかった（依頼 466）。
xcrun swiftc -O -swift-version 5 \
  -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __info_plist \
  -Xlinker "$root/mac/amber-cal.plist" \
  -o "$root/target/mac/amber-cal" "$root/mac/amber-cal.swift"

# **署名する。** 署名の無いものへの許可は、macOS が憶えていられない
# （毎回訊かれるか、そもそも通らない）。
codesign --force --sign - "$root/target/mac/amber-cal"
echo "できました: target/mac/amber-cal"
