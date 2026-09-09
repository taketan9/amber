#!/bin/zsh
# **電話の総ざらい**（依頼 448）── 窓の `walk.sh` にあたるもの。
#
#     scripts/walk-phone.sh
#
# 起こすときに `--walk` を渡すと、画面の代わりに `ios/Cian/Walk.swift` が
# 走り、落ちた数を持って自分で終わる（`#if DEBUG` の中なので、配るものには
# 入らない）。
#
# **まっさらから始める。** 先に消してから入れるので、置き場所を決める道も
# 見本を入れる道も、毎回通る ── 窓の `first-run.sh` と同じ考え。
# 本人のノートには触らない（電話のアプリは自分の入れ物の中で走る）。
set -e
here="${0:A:h}"
root="${here:h}"
dev="${DEVICE:-iPhone 16 Pro}"
app="com.taketan.cian"
port="${PORT:-8778}"
work="${TMPDIR:-/tmp}/amber-phone-walk"

quit() {
  [ -n "$site" ] && kill "$site" 2>/dev/null
  rm -rf "$work"
}
trap quit EXIT INT TERM

# 取り込みを見るための、手元の一枚。**よそのページで試さない** ── 相手が
# 変わった日に、何が原因か分からなくなる。
rm -rf "$work"; mkdir -p "$work"
cat > "$work/index.html" <<'PAGE'
<!doctype html><html lang="ja"><head><meta charset="utf-8">
<title>取り込みの試し | example</title></head><body>
<nav><a href="/">戻る</a></nav>
<article><h1>取り込みの試し</h1>
<p>本文の一段落目。<a href="/next">続き</a>があります。</p>
<p>本文らしいところだけ採るかを見たいので、案内や足より字を多くしておきます。
だからこの段落はわざと長く書いてあります。もっと長く。もっと長く。</p>
</article><footer>足の字。</footer></body></html>
PAGE
# **入れ子の中で起こさない** ── 括弧の中だと `$!` は括弧の番号で、
# 片づけのときに本体が生き残る。
python3 -m http.server "$port" --directory "$work" >/dev/null 2>&1 &
site=$!

boot=$(xcrun simctl list devices booted | grep -o '([0-9A-F-]\{36\})' | head -1 | tr -d '()')
if [ -z "$boot" ]; then
  xcrun simctl boot "$dev"
  sleep 8
  boot=$(xcrun simctl list devices booted | grep -o '([0-9A-F-]\{36\})' | head -1 | tr -d '()')
fi
[ -n "$boot" ] || { echo "電話が起きていません"; exit 2 }

(cd "$root" && ./scripts/ios-build.sh >/dev/null 2>&1) || { echo "エンジンが作れません"; exit 2 }
(cd "$root/ios" && xcodebuild -project Cian.xcodeproj -scheme Cian \
  -sdk iphonesimulator -destination "platform=iOS Simulator,name=$dev" \
  -quiet build 2>&1 | grep -E 'error:' ) && { echo "電話が組めません"; exit 2 }

built=$(ls -d ~/Library/Developer/Xcode/DerivedData/Cian-*/Build/Products/Debug-iphonesimulator/Cian.app 2>/dev/null | head -1)
[ -d "$built" ] || { echo "組んだものが見つかりません"; exit 2 }

# **まっさらから。** 前の回のノートが残っていると、「二度目は増えない」が
# 一度目から二度目になる。
xcrun simctl uninstall "$boot" "$app" >/dev/null 2>&1 || true
xcrun simctl install "$boot" "$built"

out="$work/out.txt"
SIMCTL_CHILD_SITE="http://127.0.0.1:$port/" \
  xcrun simctl launch --console-pty "$boot" "$app" --walk 2>&1 | tee "$out" || true
xcrun simctl uninstall "$boot" "$app" >/dev/null 2>&1 || true

# **`set -e` の下では `a && exit 1` を書かない** ── 当たらなかった grep
# 自体が落第になり、通ったのに落ちた顔で終わる（実際にそうなった）。
if grep -q 'おかしいです' "$out"; then
  exit 1
fi
if grep -q '落ちたものはありません' "$out"; then
  exit 0
fi
echo "答えが返ってきませんでした"
exit 2
