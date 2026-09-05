#!/usr/bin/env bash
# amber の窓を、ソースから走らせる。
#
#     ./gui/run.sh
#
# エンジンを先に建てるのは、`cargo test` が bin を更新しないから ── cian で
# 何度も踏んだ。「直したのに効かない」の半分はこれ。
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -p amber-server
cd gui
[ -d node_modules ] || npm install
npm start
