#!/usr/bin/env bash
# Build amber-ffi for iPhone and for the simulator.
#
# Does as much as the machine allows and says exactly what it could not do.
# The three static libraries need only rustup; turning them into an
# XCFramework needs Xcode, which is a separate install — so a machine with
# rustup and no Xcode still gets the libraries and a clear note about the one
# remaining step, rather than an error at the top and nothing to show.
set -euo pipefail
cd "$(dirname "$0")/.."

# Homebrew's rustup is keg-only precisely so it does not fight the `rust`
# formula, so it is usually not on PATH — and the toolchain's own cargo is
# what has to run, because Homebrew's cargo would find Homebrew's rustc and
# that one has no iOS standard library. Looked for rather than demanded: the
# error otherwise is "can't find crate for `core`", which reads like a missing
# target and is really a mixed toolchain.
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

# `--profile ios`, not release: release strips the two symbols the app links
# for, and its thin LTO leaves bitcode Xcode's older LLVM cannot read.
for t in $TARGETS; do
    echo "== $t"
    "$TC/cargo" build -p amber-ffi --profile ios --target "$t"
done

# The simulator needs both architectures in one library: his Mac is Intel and
# the CI's is not, and a library with only one of them fails to link on the
# other with a message about architectures rather than about the missing half.
lipo -create \
  target/aarch64-apple-ios-sim/ios/libamber_ffi.a \
  target/x86_64-apple-ios/ios/libamber_ffi.a \
  -output "$OUT/libamber_ffi_sim.a"

# `|| true` because `nm` exits non-zero on an archive that has any member
# with no symbols in it — which every Rust archive has — and `pipefail` then
# fails the check however well `grep` did.
#
# `-arch all`: the archive is arm64 and the Mac running this may not be, and
# `nm` quietly looks only at the host architecture — so the check found
# nothing and blamed the library, on a library that was fine.
#
# The check that matters: the two symbols Swift links against are really in
# there and really readable by Apple's tools. Both have been false — stripped
# by the release profile, and unreadable bitcode — and both failures show up
# as a link error in Xcode with nothing pointing back here.
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
