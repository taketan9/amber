#!/usr/bin/env bash
# Break one thing, watch the ledger notice, put it back.
#
# Every check added to `gui/REQUESTS.ja.md` has to be shown to *fail* on a
# broken version before it is trusted — five times now a check has gone quiet
# rather than gone green, and a quiet check is worse than no check.
#
# **Every occurrence, not the first.** A retyped `perl -0pi -e s///` without
# the `/g` changed one of four `data-line=` and the check still matched, which
# looked exactly like a check that does not work. That is the reason this is a
# script and not something typed out each time.
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
    echo "✗ 壊しても鳴りません: $file の「$from」"
    echo "  検査が別の場所に当たっているか、書き方が緩すぎます。"
    exit 1
fi
echo "ok 壊すと ✗ $after 件 : $file の「$from」"
