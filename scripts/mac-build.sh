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
xcrun swiftc -O -swift-version 5 \
  -o "$root/target/mac/amber-cal" "$root/mac/amber-cal.swift"
echo "できました: target/mac/amber-cal"
