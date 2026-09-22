#!/usr/bin/env bash
# **実機の iPhone に入れる**（依頼 533）。
#
#     ./scripts/phone-install.sh              つながっている実機ぜんぶに入れる
#     ./scripts/phone-install.sh --list       いま見えている実機を数えるだけ
#     ./scripts/phone-install.sh <名前の一部> その端末にだけ入れる
#
# **シミュレータには入れない。** あちらは `xcodebuild ... -sdk iphonesimulator`
# で別に建てる ── 実機に入れたつもりでシミュレータを見ていた、が起きないように
# パスを分ける（2026-09-13 に実際にそれをやった）。
#
# **識別子は二つある。** 入れるときのものと、ビルドするときのものは別物で、取り違える
# と「その端末は見つかりません」で止まる。一覧は `scripts/devices.py` が JSON で
# 訊いて、両方そのまま返す ── 名前で突き合わせると、名前に含まれる「の」が
# 文字化けして一致せず、**その端末が黙って飛ばされる**（実際に飛ばされた）。
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

rows=$(python3 scripts/devices.py || true)
echo "── つながっている実機 ──"
if [ -z "$rows" ]; then
    echo "  ありません。"
    echo
    echo "ケーブルでつないで、その端末で「信頼」を押してもらってください"
    echo "（持ち主自身に。暗証番号が要ります）。"
    exit 1
fi
while IFS=$'\t' read -r name ident udid model trouble; do
    printf '  %-24s %s\n' "$name" "$model"
    [ -n "$trouble" ] && printf '  %-24s → %s\n' '' "$trouble"
done <<< "$rows"
[ "$want" = "--list" ] && exit 0

hit=0
failed=0
# 組むはずの版。**入れたものがこれと違えば、入れたとは言わない。**
version=$(sed -n 's/.*MARKETING_VERSION = \([0-9.]*\);.*/\1/p' ios/Cian.xcodeproj/project.pbxproj | head -1)
while IFS=$'\t' read -r name ident udid model trouble; do
    [ -z "$ident" ] && continue
    if [ -n "$want" ] && [[ "$name" != *"$want"* ]]; then continue; fi
    hit=1
    echo
    echo "── $name に入れる ──"
    # **端末が言っている理由を、そのまま出す。** こちらで当て推量を並べない。
    if [ -n "$trouble" ]; then echo "  $trouble"; continue; fi
    # **端末を署名の書類に入れてもらう**（`-allowProvisioningUpdates`・依頼 543）。
    # 新しい端末は、はじめは書類に入っていないので「この端末は入っていません」で
    # 止まる。この旗があると Xcode が Apple に登録しにいく ── **本人の Apple ID に
    # 「この端末で開発する」と記録される**（Xcode からいつでも消せる）。
    out=$(xcodebuild -project ios/Cian.xcodeproj -scheme Cian \
        -destination "platform=iOS,id=$udid" -configuration Debug \
        -allowProvisioningUpdates build 2>&1 || true)
    printf '%s\n' "$out" | grep -E 'error:|\*\* BUILD' || true
    # **組めなかったら、入れない**（2026-09-22）。前は失敗を `|| true` で飲み込み、
    # そのあと DerivedData に**前回組めた古い `Cian.app`** を見つけて入れていた ──
    # 署名が切れて組めなかった日に「入れました」と言い、実際には一つ前の版が
    # 入り直しただけだった（3.2.4 のつもりで 3.2.3 が二台に入った）。
    if ! printf '%s\n' "$out" | grep -q '\*\* BUILD SUCCEEDED \*\*'; then
        echo "  組めませんでした ── 上の error を見てください。入れていません"
        failed=1
        continue
    fi
    app=$(find ~/Library/Developer/Xcode/DerivedData/Cian-*/Build/Products/Debug-iphoneos \
          -maxdepth 1 -name 'Cian.app' 2>/dev/null | head -1)
    [ -n "$app" ] || { echo "  組めていません"; failed=1; continue; }
    # **入れる前に、版を見る** ── 組めたと言っても、見つけた `Cian.app` が
    # 別の場所の古いものなら同じことが起きる。
    got=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Info.plist" 2>/dev/null || true)
    if [ "$got" != "$version" ]; then
        echo "  組んだはずの版（$version）ではなく $got が見つかりました。入れていません"
        failed=1
        continue
    fi
    if xcrun devicectl device install app --device "$ident" "$app" >/dev/null 2>&1; then
        echo "  入れました（$version）。"
    else
        echo "  入れられませんでした（端末のロックを解いて、つないだままにしてください）"
        failed=1
    fi
done <<< "$rows"

[ "$hit" = "1" ] || { echo; echo "「$want」に当たる端末がありません。"; exit 1; }

# **一台でも入らなければ、そう言って止まる。** 下の「いつまで動くか」は、
# 入らなかったときは**前に組んだ古い署名の日付**を読んでしまうので、出さない。
if [ "$failed" = "1" ]; then
    echo
    echo "入らなかった端末があります。上の理由を見てください。"
    exit 1
fi

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
