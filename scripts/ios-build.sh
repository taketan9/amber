#!/usr/bin/env bash
# amber-ffi を、iPhone の実機とシミュレータ向けに組む。
#
# **その端末でできるところまでやって、できなかったことを名指しで言う。**
# ライブラリ 3 つは rustup だけで作れるが、XCFramework にまとめるには
# Xcode が要る（Command Line Tools とは別物）── だから Xcode の無い端末でも
# ライブラリはできて、残り一手だけが分かる。頭でエラーにして何も渡さない
# のとは、直す人の手間がまるで違う。
set -euo pipefail
cd "$(dirname "$0")/.."

# **Homebrew の rustup は PATH に居ない。** `rust` の formula とぶつからない
# ように keg-only にしてあるので、そういうものとして探す。
#
# **ツールチェーン側の cargo を走らせること。** Homebrew の cargo を使うと
# Homebrew の rustc を拾い、あれには iOS の標準ライブラリが入っていない。
# そのときのエラーは "can't find crate for `core`" ── ターゲットを足し忘れた
# ように読めるが、**本当はツールチェーンが混ざっている**。
RUSTUP="$(command -v rustup || true)"
[ -n "$RUSTUP" ] || [ ! -x /usr/local/opt/rustup/bin/rustup ] || RUSTUP=/usr/local/opt/rustup/bin/rustup
[ -n "$RUSTUP" ] || [ ! -x /opt/homebrew/opt/rustup/bin/rustup ] || RUSTUP=/opt/homebrew/opt/rustup/bin/rustup
if [ -z "$RUSTUP" ]; then
    echo "rustup がありません。brew install rustup && rustup default stable"
    exit 1
fi
TC="$("$RUSTUP" show home)/toolchains/$("$RUSTUP" show active-toolchain | cut -d' ' -f1)/bin"
[ -x "$TC/cargo" ] || { echo "ツールチェーンが見つかりません: $TC"; exit 1; }
export PATH="$TC:$PATH" RUSTC="$TC/rustc"

TARGETS="aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios"
for t in $TARGETS; do
    "$RUSTUP" target list --installed | grep -qx "$t" || {
        echo "ターゲットがありません: $t"
        echo "  rustup target add $TARGETS"
        exit 1
    }
done

OUT=target/ios
rm -rf "$OUT"; mkdir -p "$OUT"

# **release ではなく `--profile ios`。** release は `strip` でアプリがリンク
# する 2 つの記号を消し、thin LTO が Xcode の LLVM より新しいビットコードを
# 残す（どちらも Xcode 側のリンクエラーになって、ここには何も指さない）。
for t in $TARGETS; do
    echo "== $t"
    "$TC/cargo" build -p amber-ffi --profile ios --target "$t"
done

# **シミュレータ向けは、2 つのアーキテクチャを 1 つのライブラリに入れる。**
# 本人の Mac は Intel で、CI の Mac はそうではない ── 片方しか入っていない
# ライブラリは、もう片方でリンクに失敗する。そのときの文面はアーキテクチャの
# 話で、「足りないほう」の話はしてくれない。
lipo -create \
  target/aarch64-apple-ios-sim/ios/libamber_ffi.a \
  target/x86_64-apple-ios/ios/libamber_ffi.a \
  -output "$OUT/libamber_ffi_sim.a"

# **ここが、この台本でいちばん大事な検査。** Swift がリンクする 2 つの記号が
# 本当に入っていて、しかも Apple の道具で読めること ── **どちらも偽だった
# ことがある**（release プロファイルに消された・ビットコードが読めなかった）。
# どちらも Xcode のリンクエラーとして出るだけで、ここを指してはくれない。
#
# `|| true` が要る ── `nm` は、記号を 1 つも持たないメンバーが混じった
# アーカイブで 0 以外を返す。Rust のアーカイブには必ず混じっているので、
# `grep` がどれだけうまくいっても `pipefail` が検査ごと落とす。
#
# `-arch all` が要る ── アーカイブは arm64 だが、これを走らせている Mac は
# そうとは限らない。`nm` は黙って**自分のアーキテクチャだけ**を見るので、
# 何も見つからず、何ともないライブラリのせいにされた。
for f in target/aarch64-apple-ios/ios/libamber_ffi.a "$OUT/libamber_ffi_sim.a"; do
    for sym in _amber_call _amber_free; do
        { nm -g -arch all "$f" 2>/dev/null || true; } | grep -q "T $sym" \
            || { echo "記号がありません: $sym in $f"; exit 1; }
    done
done
echo "記号 ok: _amber_call / _amber_free"

if xcodebuild -version >/dev/null 2>&1; then
    xcodebuild -create-xcframework \
      -library target/aarch64-apple-ios/ios/libamber_ffi.a -headers crates/amber-ffi/include \
      -library "$OUT/libamber_ffi_sim.a" -headers crates/amber-ffi/include \
      -output "$OUT/AmberFFI.xcframework"
    echo "できました: $OUT/AmberFFI.xcframework"
else
    echo
    echo "ライブラリはできました:"
    echo "  実機         target/aarch64-apple-ios/ios/libamber_ffi.a"
    echo "  シミュレータ $OUT/libamber_ffi_sim.a"
    echo "XCFramework にまとめるには Xcode が要ります（Command Line Tools だけでは足りません）。"
fi
