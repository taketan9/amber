#!/usr/bin/env bash
# **実機の iPhone に入れる**（依頼 533）。
#
#     ./scripts/phone-install.sh              つながっている実機ぜんぶに入れる
#     ./scripts/phone-install.sh --list       いま見えている実機を数えるだけ
#     ./scripts/phone-install.sh <名前の一部> その端末にだけ入れる
#
# **シミュレータには入れない。** あちらは `xcodebuild ... -sdk iphonesimulator`
# で別に建てる ── 実機に入れたつもりでシミュレータを見ていた、が起きないように
# 道を分ける（2026-09-13 に実際にそれをやった）。
#
# # 新しい端末を足すには
#
# **一度だけ、ケーブルでこの Mac につないで「信頼」を押してもらう**（その端末の
# 暗証番号が要る）。持ち主自身にやってもらうこと ── 人の端末なので。
# つないだあと、この道具をもう一度走らせれば一覧に出てくる。
#
# # 七日で切れる
#
# いまの署名は**無料の個人チーム**なので、入れたアプリは **7 日で起動しなく
# なる**。切れたらこの道具をもう一度走らせる。七日ごとに端末を集めて回るのが
# 続かないなら、Apple Developer Program（年 99 ドル）の TestFlight にすると、
# ケーブルも Mac も要らず 90 日動く ── そこは本人が決めること。
set -euo pipefail
cd "$(dirname "$0")/.."

want="${1:-}"

echo "── つながっている実機 ──"
list=$(xcrun devicectl list devices 2>/dev/null | grep -E 'available|connected' || true)
if [ -z "$list" ]; then
    echo "ありません。"
    echo
    echo "ケーブルでつないで、その端末で「信頼」を押してもらってください"
    echo "（持ち主自身に。暗証番号が要ります）。"
    exit 1
fi
echo "$list" | sed 's/^/  /'
[ "$want" = "--list" ] && exit 0

# **識別子は二つある。** `devicectl` が返すもの（入れるときに使う）と、
# `xcodebuild` が受けるもの（組むときに使う）は**別物**で、取り違えると
# 「その端末は見つかりません」で止まる（実際に止まった・2026-09-13）。
# 名前で突き合わせて、両方持つ。
ids=$(echo "$list" | sed -E 's/ {2,}/\t/g' | cut -f1,3 | tr '\t' '|')
dest=$(xcodebuild -project ios/Cian.xcodeproj -scheme Cian -showdestinations 2>/dev/null \
       | grep -E 'platform:iOS, arch' || true)

hit=0
while IFS='|' read -r name id; do
    [ -z "$id" ] && continue
    if [ -n "$want" ] && [ "$want" != "--list" ] && [[ "$name" != *"$want"* ]]; then continue; fi
    build_id=$(echo "$dest" | grep -F "name:$name" | sed -E 's/.*id:([^,]+).*/\1/' | head -1)
    if [ -z "$build_id" ]; then
        echo "「$name」は Xcode からは見えていません（ケーブルでつなぎ直してください）"
        continue
    fi
    hit=1
    echo
    echo "── $name に入れる ──"
    xcodebuild -project ios/Cian.xcodeproj -scheme Cian \
        -destination "platform=iOS,id=$build_id" -configuration Debug build \
        2>&1 | grep -E 'error:|\*\* BUILD' || true
    app=$(find ~/Library/Developer/Xcode/DerivedData/Cian-*/Build/Products/Debug-iphoneos \
          -maxdepth 1 -name 'Cian.app' 2>/dev/null | head -1)
    [ -n "$app" ] || { echo "組めていません"; exit 1; }
    xcrun devicectl device install app --device "$id" "$app" >/dev/null
    echo "入れました。"
done <<< "$ids"

[ "$hit" = "1" ] || { echo; echo "「$want」に当たる端末がありません。"; exit 1; }

# **いつ切れるかを言う。** 言わないと、ある朝いきなり起動しなくなる。
prof=$(find ~/Library/Developer/Xcode/DerivedData/Cian-*/Build/Products/Debug-iphoneos/Cian.app \
       -name 'embedded.mobileprovision' 2>/dev/null | head -1)
if [ -n "$prof" ]; then
    echo
    security cms -D -i "$prof" 2>/dev/null | python3 -c "
import sys, plistlib, datetime
d = plistlib.loads(sys.stdin.buffer.read())
end = d.get('ExpirationDate')
if end:
    left = (end - datetime.datetime.now(datetime.timezone.utc).replace(tzinfo=None)).days
    print(f'この版は {end:%Y-%m-%d} まで動きます（あと {left} 日）。')
    print('切れたら、この道具をもう一度走らせてください。')
"
fi
