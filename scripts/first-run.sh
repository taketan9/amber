#!/bin/zsh
# **初めて開いた人が見るもの**を、実際に確かめる（依頼 432）。
#
#     scripts/first-run.sh
#
# 総ざらい（`walk.sh`）はいつも**ノートのある場所**から始まるので、
# 「一本も無いところから始まる」道を誰も通っていなかった ── そこは
# 置き場所を決め、見本を入れ、最初の一本を開くまでが一続きになっている。
#
# 見るのは五つ:
#   一。置き場所が決まること（`~/Documents/amber` が作られる）
#   二。見本のノートが入ること（空の窓を見せない）
#   三。その見本が**読める形**で出ること（組めずに白いまま、を許さない）
#   四。落ちも `console.error` も出ないこと
#   五。**二度目に開いても増えないこと**（同じ名前は飛ばす）
#
# 本人のノートと設定には触らない ── `walk.sh` と同じ守り方。
set -e
here="${0:A:h}"
root="${here:h}"
work="${TMPDIR:-/tmp}/amber-first"
mine="$HOME/Library/Application Support/amber/amber.json"
port="${PORT:-9334}"

quit() {
  pkill -f 'Electron.*remote-debugging-port='"$port" 2>/dev/null || true
  sleep 1
  # **設定を戻すのは、何があっても**（`walk.sh` と同じ理由 ── macOS の
  # Electron は `appData` に $HOME を見ない）。
  if [ -f "$work/amber.json.mine" ]; then
    mkdir -p "$(dirname "$mine")"
    cp "$work/amber.json.mine" "$mine"
    echo "設定を戻しました"
  elif [ -f "$work/なかった" ]; then
    rm -f "$mine"
    echo "設定を消しました（もともと無かったので）"
  fi
}
trap quit EXIT INT TERM

rm -rf "$work"
mkdir -p "$work/home"
if [ -f "$mine" ]; then cp "$mine" "$work/amber.json.mine"; else touch "$work/なかった"; fi
# **設定も消しておく。** 残っていると「前に決めた置き場所」を思い出して
# しまい、初めての人の道を通らない。
rm -f "$mine"

(cd "$root" && cargo build -q -p amber-server) || { echo "エンジンが作れません"; exit 2 }

(cd "$root/gui" && HOME="$work/home" npx electron --remote-debugging-port="$port" . \
  >"$work/win.log" 2>&1 &)
for i in $(seq 1 40); do
  curl -s -m 1 "http://127.0.0.1:$port/json" >/dev/null 2>&1 && break
  sleep 0.5
done

PORT="$port" HOME_DIR="$work/home" node "$here/first-run.mjs"
