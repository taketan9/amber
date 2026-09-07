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

# **版の数字も見る。** 長いあいだ `*.rs` だけを見ていて、`Cargo.toml` の
# 版を上げただけの回はここが黙って通った ── 「ambƏr について」の画面が
# 「画面 2.7.0 / エンジン 2.6.0」と出る。ずれているのが原因の不具合を
# 追うためにその画面を作ったのに、その画面自身が古い数字を言っていた。
#
# **`find` を一度で済ませる。** 二度に分けて `grep -v '^$'` で繋いだ回は、
# 何も新しくないときに grep が 1 を返し、`set -e` がそこで script を
# 黙って殺した ── 検査は「落ちた」のではなく「何も言わずに失敗した」
# ので、画面には何も出ないまま電話のビルドだけが止まる。
NEWER=$(find crates/amber-core/src crates/amber-ffi/src -name '*.rs' -newer "$FW")
STAMPS=$(find Cargo.toml crates/amber-core/Cargo.toml crates/amber-ffi/Cargo.toml \
              -newer "$FW")
NEWER=$(printf '%s\n%s' "$NEWER" "$STAMPS" | sed '/^$/d' | head -5)
if [ -n "$NEWER" ]; then
    echo "error: エンジンがソースより古いです。scripts/ios-build.sh を実行してください。"
    echo "$NEWER" | while read -r f; do echo "error:   $f"; done
    exit 1
fi
echo "エンジンは最新です"
