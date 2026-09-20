#!/usr/bin/env bash
# 一か所だけ壊して、台帳が気づくのを見て、戻す。
#
# **台帳（`REQUESTS.ja.md`）に足した検査は、壊した版で落ちるのを見るまで
# 信じない。** これまでに五回、検査が「通った」のではなく**黙った**ことが
# ある ── 黙る検査は、無い検査より悪い。画面の上では同じ顔をしていて、
# こちらは守られているつもりでいるから。
#
# **一つ目だけでなく、ぜんぶ置き換える。** その場で打ち直した
# `perl -0pi -e s///` に `/g` が無く、4 つある `data-line=` のうち 1 つしか
# 変わらなかったことがある。検査は当たったままで、**効かない検査と
# 見分けがつかなかった** ── だからこれは、毎回打つものではなく台本にした。
#
#   scripts/mutate.sh crates/cian-core/src/note.rs 'pub fn spans(' 'pub fn colours('
set -euo pipefail
cd "$(dirname "$0")/.."

file="${1:?どのファイル}"
from="${2:?何を}"
to="${3:?何に}"

[ -f "$file" ] || { echo "ありません: $file"; exit 1; }

before=$(python3 scripts/requests.py 2>&1 | grep -c "✗" || true)
if [ "$before" != "0" ]; then
    echo "先に台帳が $before 件落ちています。壊す前に直してください。"
    exit 1
fi

keep=$(mktemp)
cp "$file" "$keep"
trap 'cp "$keep" "$file"; rm -f "$keep"' EXIT

FROM="$from" TO="$to" perl -0pi -e 's/\Q$ENV{FROM}\E/$ENV{TO}/g' "$file"
if ! grep -qF -- "$to" "$file"; then
    echo "置き換わりませんでした ── その文字列はこのファイルに無い: $from"
    exit 1
fi

after=$(python3 scripts/requests.py 2>&1 | grep -c "✗" || true)
if [ "$after" = "0" ]; then
    echo "✗ 壊しても気づきません: $file の「$from」"
    echo "  検査が別の場所に当たっているか、書き方が緩すぎます。"
    exit 1
fi
echo "ok 壊すと ✗ $after 件 : $file の「$from」"
