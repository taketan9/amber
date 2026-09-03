#!/usr/bin/env bash
# Build cian-ffi for iPhone and for the simulator, as one XCFramework.
#
# **This script has never been run.** The Mac it was written on has no Xcode
# and no rustup, so neither half of it could be executed; it is the recipe,
# checked by reading and not by running. Treat the first run as a test of the
# script as much as of the code.
#
# Needs, and neither can be installed without the user's password:
#   * Xcode (not just the Command Line Tools), and
#     sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
#   * rustup, then:
#     rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
set -euo pipefail
cd "$(dirname "$0")/.."

command -v rustup >/dev/null || { echo "rustup がありません（iOS のターゲットを入れられません）"; exit 1; }
xcodebuild -version >/dev/null 2>&1 || { echo "Xcode がありません（Command Line Tools だけでは足りません）"; exit 1; }

OUT=target/ios
rm -rf "$OUT"
mkdir -p "$OUT"

# The phone itself.
cargo build -p cian-ffi --release --target aarch64-apple-ios

# The simulator. Two architectures because his Mac is Intel and the CI's is
# not — a simulator library with only one of them fails to link on the other,
# with a message about the architecture and not about the missing half.
cargo build -p cian-ffi --release --target aarch64-apple-ios-sim
cargo build -p cian-ffi --release --target x86_64-apple-ios
lipo -create \
  target/aarch64-apple-ios-sim/release/libcian_ffi.a \
  target/x86_64-apple-ios/release/libcian_ffi.a \
  -output "$OUT/libcian_ffi_sim.a"

xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libcian_ffi.a -headers crates/cian-ffi/include \
  -library "$OUT/libcian_ffi_sim.a" -headers crates/cian-ffi/include \
  -output "$OUT/CianFFI.xcframework"

echo "できました: $OUT/CianFFI.xcframework"
