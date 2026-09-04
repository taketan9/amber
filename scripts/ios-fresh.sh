#!/usr/bin/env bash
# Is the engine in the app older than the engine in the sources?
#
# Xcode links a prebuilt `CianFFI.xcframework`; nothing in an Xcode build
# rebuilds the Rust. So a build after a change to `cian-core` or `cian-ffi`
# succeeds, installs, launches, and then answers 「知らない操作: remind」 the
# first time you press the new button — at which point the evidence points at
# the Swift, which is fine.
#
# That happened. This is the check that would have said so at the moment the
# app was built, in the one place that is looking.
set -euo pipefail
cd "$(dirname "$0")/.."

FW=target/ios/CianFFI.xcframework/Info.plist
if [ ! -f "$FW" ]; then
    echo "error: エンジンがまだありません。scripts/ios-build.sh を実行してください。"
    exit 1
fi

NEWER=$(find crates/cian-core/src crates/cian-ffi/src -name '*.rs' -newer "$FW" | head -5)
if [ -n "$NEWER" ]; then
    echo "error: エンジンがソースより古いです。scripts/ios-build.sh を実行してください。"
    echo "$NEWER" | while read -r f; do echo "error:   $f"; done
    exit 1
fi
echo "エンジンは最新です"
