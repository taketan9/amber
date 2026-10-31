#!/usr/bin/env bash
# **無料署名の七日を、切れる前に延ばす**（依頼 709）。
#
#     ./scripts/resign.sh          要るときだけ入れ直す
#     ./scripts/resign.sh --now    残り日数を見ずに、いま入れ直す
#     ./scripts/resign.sh --check  何もせず、残り日数だけ言う
#
# `launchd` から一日一度呼ばれる（`packaging/resign.plist`）。
# **呼ばれるたびに入れ直すのではありません** ── 残りが 4 日を切ったときだけ
# 動きます。毎日ビルドしても証書は延びず、Mac が重くなるだけです。
#
# # なぜ消してから入れ直すのか
#
# 無料の個人チームの証書は **7 日で切れます**。入れ直しただけでは延びません ──
# Xcode は手元にある証書をそのまま使い回すので、期限も元のままです。
# **消すと新しいものが作られ、その日から 7 日**になります
# （2026-10-31 に確かめました。11-04 までだったものが 11-07 までになりました）。
#
# 消すのは**この Mac の中の控え**だけです。iPhone に入っているアプリは自分の
# ぶんを持っているので、入れ直しが失敗しても、**元の期限まではいままでどおり
# 動きます**。消したことで動かなくなることはありません。
#
# # 黙って失敗させない
#
# 無料アカウントは、ときどき Xcode の画面で Apple ID に入り直すよう求めます。
# 人が居ないところで求められた回は失敗します。だから
#
# - 記録を必ず書く（`~/Library/Logs/amber-resign.log`）
# - **期限が延びていなければ、延びていないと言う。** 入れ直せたことと、
#   期限が延びたことは別のことです
# - 失敗したら画面に知らせる（残り日数も言う ── 何日のうちに手を打てばよいか）
set -euo pipefail
cd "$(dirname "$0")/.."

# `launchd` から呼ばれると PATH がほとんど空なので、ここで決めます。
export PATH="/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin"

log="$HOME/Library/Logs/amber-resign.log"
profiles="$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles"
lock="${TMPDIR:-/tmp}/amber-resign.lock"

mode="${1:-}"

say() {
    printf '%s  %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$1" >>"$log"
}

# 画面に知らせる。失敗したときだけ出します ── うまくいった回に毎度出すと、
# 出ていることに慣れて、本当に困った回を読み飛ばします。
# `AMBER_RESIGN_QUIET` を立てると黙ります（この枝を試すときに使います）。
tell() {
    [ -n "${AMBER_RESIGN_QUIET:-}" ] && return 0
    osascript -e "display notification \"$1\" with title \"amber の署名\"" >/dev/null 2>&1 || true
}

# いま iPhone に入っているアプリの期限（残り日数）。読めなければ空を返す。
#
# **読むのは組み上がったアプリに埋まっている証書**で、この Mac の控えでは
# ありません ── 控えを新しくしても、入れ直すまで iPhone の中身は古いままです。
days_left() {
    local prof
    prof=$(find "$HOME/Library/Developer/Xcode/DerivedData"/Cian-*/Build/Products/Debug-iphoneos/Cian.app \
           -name 'embedded.mobileprovision' 2>/dev/null | head -1)
    [ -z "$prof" ] && return 0
    security cms -D -i "$prof" 2>/dev/null | python3 -c "
import sys, plistlib, datetime
try:
    d = plistlib.loads(sys.stdin.buffer.read())
except Exception:
    raise SystemExit(0)
end = d.get('ExpirationDate')
if end:
    now = datetime.datetime.now(datetime.timezone.utc).replace(tzinfo=None)
    print((end - now).days)
" 2>/dev/null || true
}

expires_on() {
    local prof
    prof=$(find "$HOME/Library/Developer/Xcode/DerivedData"/Cian-*/Build/Products/Debug-iphoneos/Cian.app \
           -name 'embedded.mobileprovision' 2>/dev/null | head -1)
    [ -z "$prof" ] && return 0
    security cms -D -i "$prof" 2>/dev/null | python3 -c "
import sys, plistlib
try:
    d = plistlib.loads(sys.stdin.buffer.read())
except Exception:
    raise SystemExit(0)
end = d.get('ExpirationDate')
if end:
    print(f'{end:%Y-%m-%d}')
" 2>/dev/null || true
}

left=$(days_left)

if [ "$mode" = "--check" ]; then
    if [ -z "$left" ]; then
        echo "まだ一度も組んでいないので、期限が読めません。"
    else
        echo "$(expires_on) まで動きます（あと ${left} 日）。"
    fi
    exit 0
fi

# **要るときだけ動く。** 残りが 4 日以上あるなら何もしません ── 一日一度
# 呼ばれるので、4 日を切った最初の朝に動けば、そこから 7 日に戻ります。
if [ "$mode" != "--now" ] && [ -n "$left" ] && [ "$left" -ge 4 ]; then
    say "まだ ${left} 日あるので、何もしません（$(expires_on) まで）"
    exit 0
fi

# **二つ同時に走らせない。** 総ざらい（`walk-phone.sh`）や、手で叩いた
# `phone-install.sh` と重なると、どちらも xcodebuild を掴んで落ちます。
# この Mac は swap が 8GB しかないので、重なった回は落ちる段がばらけます。
if ! mkdir "$lock" 2>/dev/null; then
    say "ほかの入れ直しが走っているので、今日は見送ります"
    exit 0
fi
trap 'rmdir "$lock" 2>/dev/null || true' EXIT

for busy in xcodebuild walk-phone.sh phone-install.sh; do
    if pgrep -f "$busy" >/dev/null 2>&1; then
        say "$busy が走っているので、今日は見送ります"
        exit 0
    fi
done

say "入れ直します（残り ${left:-不明} 日）"

# **証書の控えを消す。** これをしないと期限が延びません。
# 消えても、iPhone に入っているアプリは元の期限まで動きます。
rm -f "$profiles"/*.mobileprovision 2>/dev/null || true

out="${TMPDIR:-/tmp}/amber-resign-out.$$"
if ./scripts/phone-install.sh >"$out" 2>&1; then
    sed 's/^/    /' "$out" >>"$log"
else
    sed 's/^/    /' "$out" >>"$log"
    say "入れ直しに失敗しました"
    tell "入れ直せませんでした。残り ${left:-不明} 日。記録: ~/Library/Logs/amber-resign.log"
    rm -f "$out"
    exit 1
fi
rm -f "$out"

# **入れ直せたことと、期限が延びたことは別のこと。**
# ここを見ないと、同じ証書で入れ直しただけの回を「成功」と記録します。
#
# 見るのは**残り日数が 6 日以上になったか**です。新しい証書は作った日から
# 7 日なので、切り捨てで 6 日と出ます。「前より増えたか」で見ると、
# `--now` を証書を作った当日に走らせた回が嘘の失敗になります
# （その日は既に新しいので、増えないのが正しい）。
now=$(days_left)
if [ -z "$now" ] || [ "$now" -lt 6 ]; then
    say "入れ直しましたが、期限が延びていません（${left:-不明} → ${now:-不明} 日）"
    tell "入れ直しましたが、期限が延びていません。Xcode で Apple ID に入り直す必要があるかもしれません。"
    exit 1
fi

say "延びました（${left:-不明} → ${now} 日・$(expires_on) まで）"
