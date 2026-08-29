#!/bin/sh
# cian (Electron) を起動する。run.bat の Mac / Linux 版。
#
# 探す順番は Windows 版と同じ: 環境変数、リポジトリの隣の配布版、npm で
# 入れたもの。見つからなければ、どこを探したかを言って終わる。
set -eu

here=$(cd "$(dirname "$0")" && pwd)
found=""

if [ -n "${CIAN_ELECTRON:-}" ] && [ -x "$CIAN_ELECTRON" ]; then
    found="$CIAN_ELECTRON"
fi

if [ -z "$found" ]; then
    for d in "$here"/../../electron-v*; do
        # macOS は .app の中、Linux は直下。
        for candidate in \
            "$d/Electron.app/Contents/MacOS/Electron" \
            "$d/electron"
        do
            [ -x "$candidate" ] && found="$candidate" && break 2
        done
    done
fi

if [ -z "$found" ]; then
    for candidate in \
        "$here/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron" \
        "$here/node_modules/electron/dist/electron"
    do
        [ -x "$candidate" ] && found="$candidate" && break
    done
fi

if [ -z "$found" ]; then
    echo "Electron が見つかりません。探した場所:" >&2
    echo "  1. \$CIAN_ELECTRON  (いまの値: '${CIAN_ELECTRON:-}')" >&2
    echo "  2. $here/../../electron-v*/" >&2
    echo "  3. $here/node_modules/electron/dist/" >&2
    exit 1
fi

# エンジンが無ければ、空の窓を見せるより先に言う。
if [ ! -x "$here/cian-server" ] \
   && [ ! -x "$here/../target/release/cian-server" ] \
   && [ ! -x "$here/../target/debug/cian-server" ]; then
    echo "cian-server がありません: cargo build --release -p cian-server" >&2
    exit 1
fi

# エディタの資材が無ければ、開いた瞬間に困る前に言う。
if [ ! -f "$here/vendor/monaco/vs/loader.js" ]; then
    echo "gui/vendor がありません（エディタは開けません）: node gui/vendor.js" >&2
fi

echo "Electron: $found"
exec "$found" "$here" "$@"
