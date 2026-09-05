#!/usr/bin/env bash
# amber の窓を、ソースから走らせる。
#
#     ./gui/run.sh
#
# エンジンを先に建てるのは、`cargo test` が bin を更新しないから ── cian で
# 何度も踏んだ。「直したのに効かない」の半分はこれ。
#
# **`vendor/` も見る。** Monaco も vim も図も `gui/vendor/` に置いてあり、
# そこは git に入れていない（数メガの、決して変わらないものを複製のたびに
# 永久に配る理由が無い）。無いまま起動すると窓は開くが中身が真っ白で、
# 原因が「落としていない」だと画面のどこにも書いていない。
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -p amber-server
cd gui
[ -d node_modules ] || npm install
[ -d vendor/monaco ] || node vendor.js
npm start
