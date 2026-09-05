#!/usr/bin/env bash
# Mac から Windows の `amber-server.exe` を組む。
#
#     ./scripts/win-build.sh                 # 組む
#     ./scripts/win-build.sh どこか/gui/      # 組んで、そこへ置く
#
# **本筋は CI**（`.github/workflows/release.yml`）。ここは近道で、
# **確かめられるのは形まで ── 走るところは見ていない**（mac の上で
# Windows の実行ファイルは動かない）。
#
# 出来上がりも CI のほうが良い:
#
#   * CI（MSVC ＋ `crt-static`）が見る DLL は `kernel32` / `ntdll` /
#     `bcryptprimitives` / `api-ms-win-core-synch` の四つだけ
#   * ここ（mingw）は加えて `api-ms-win-crt-*` の九つを見る。どれも
#     Windows 10 以降には元からあるが、**少ないほうが安心できる**
#
# 急がないなら `gh workflow run release.yml` で組んで、資材を落とすほうがよい。
#
# 要るもの:
#
#   rustup target add x86_64-pc-windows-gnu
#   brew install mingw-w64        # `chrono` の Windows 経路が dlltool を要る
#
# **`gnu` で組む理由。** mac から `msvc` を狙うには Microsoft の SDK を
# 落としてくることになる。`gnu` なら mingw だけで済み、mingw のランタイム
# （`libgcc` / `libwinpthread` / `libstdc++`）は静的に取り込まれるので、
# 出来上がりは Windows に元からある DLL しか見ない ── **落として置くだけ**
# という運びが保てる。
set -euo pipefail
cd "$(dirname "$0")/.."

# rustup は keg-only で PATH に居ないことがある（README 参照）。ツールチェーン
# 側の cargo を使わないと Homebrew の rustc を拾い、std が無いと言われる。
RUSTUP="$(command -v rustup || true)"
[ -n "$RUSTUP" ] || [ ! -x /usr/local/opt/rustup/bin/rustup ] || RUSTUP=/usr/local/opt/rustup/bin/rustup
[ -n "$RUSTUP" ] || [ ! -x /opt/homebrew/opt/rustup/bin/rustup ] || RUSTUP=/opt/homebrew/opt/rustup/bin/rustup
[ -n "$RUSTUP" ] || { echo "rustup がありません。brew install rustup"; exit 1; }
TC="$("$RUSTUP" show home)/toolchains/$("$RUSTUP" show active-toolchain | cut -d' ' -f1)/bin"
export PATH="$TC:$PATH"

command -v x86_64-w64-mingw32-dlltool >/dev/null || {
    echo "mingw-w64 がありません。brew install mingw-w64"
    exit 1
}
"$RUSTUP" target list --installed | grep -qx x86_64-pc-windows-gnu || {
    echo "target がありません。rustup target add x86_64-pc-windows-gnu"
    exit 1
}

cargo build --release --target x86_64-pc-windows-gnu -p amber-server
EXE=target/x86_64-pc-windows-gnu/release/amber-server.exe

# **連れていく DLL が無いか、ここで見る。** 配ったあとに「起動しない」で
# 出ると、原因が実行ファイルの中に書いていない。
if strings "$EXE" | grep -qiE 'libgcc|libwinpthread|libstdc\+\+'; then
    echo "error: mingw のランタイム DLL を連れています"
    exit 1
fi
echo "できました: $EXE"
ls -l "$EXE"
echo
echo "見ている DLL（どれも Windows 10 以降に元からあるもの）:"
strings "$EXE" | grep -iE '^[a-z0-9_.-]+\.dll$' | sort -u | sed 's/^/  /'

if [ $# -ge 1 ]; then
    mkdir -p "$1"
    cp "$EXE" "$1/amber-server.exe"
    echo
    echo "置きました: $1/amber-server.exe"
fi

echo
echo "Windows での確かめ方（一往復すれば十分）:"
echo '  echo {"id":1,"method":"version","params":{}} | amber-server.exe'
