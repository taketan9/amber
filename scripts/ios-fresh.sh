#!/usr/bin/env bash
# Is the engine in the app older than the engine in the sources?
#
# Xcode links a prebuilt `AmberFFI.xcframework`; nothing in an Xcode build
# rebuilds the Rust. So a build after a change to `amber-core` or `amber-ffi`
# succeeds, installs, launches, and then answers 「知らない操作: remind」 the
# first time you press the new button — at which point the evidence points at
# the Swift, which is fine.
#
# That happened. This is the check that would have said so at the moment the
# app was built, in the one place that is looking.
#
# **名前は `AmberFFI`。** cian から分かれた日に `ios-build.sh` と pbxproj は
# 新しい名前に変わったが、ここだけ `CianFFI` のまま残った ── この検査は
# 必ず「エンジンがまだありません」で落ち、iOS のビルドが一度も通らなく
# なっていた。名前の取り残しは、**動かないのではなく、常に同じ嘘をつく。**
set -euo pipefail
cd "$(dirname "$0")/.."

FW=target/ios/AmberFFI.xcframework/Info.plist
if [ ! -f "$FW" ]; then
    echo "error: エンジンがまだありません。scripts/ios-build.sh を実行してください。"
    exit 1
fi

NEWER=$(find crates/amber-core/src crates/amber-ffi/src -name '*.rs' -newer "$FW" | head -5)
if [ -n "$NEWER" ]; then
    echo "error: エンジンがソースより古いです。scripts/ios-build.sh を実行してください。"
    echo "$NEWER" | while read -r f; do echo "error:   $f"; done
    exit 1
fi
echo "エンジンは最新です"
