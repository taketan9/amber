#!/bin/zsh
# **初めて開いた人が見るもの**を、実際に確かめる（依頼 432）。
#
#     scripts/first-run.sh
#
# 総ざらい（`walk.sh`）はいつも**ノートのある場所**から始まるので、
# 「一本も無いところから始まる」パスを誰も通っていなかった ── そこは
# 置き場所を決め、サンプルを入れ、最初の一本を開くまでが一続きになっている。
#
# 見るのは五つ:
#   一。置き場所が決まること（`~/Documents/amber` が作られる）
#   二。サンプルのノートが入ること（空のデスクトップ版を見せない）
#   三。そのサンプルが**読める形**で出ること（組めずに白いまま、を許さない）
#   四。落ちも `console.error` も出ないこと
#   五。**二度目に開いても増えないこと**（同じ名前は飛ばす）
#
# 本人のノートと設定には触らない ── 試験用の家（`AMBER_TEST_HOME`）を渡す。
#
# **前は本物の設定を写して消し、終わったら書き戻していた**（初めて開いた人に
# するため）。途中で落ちると、本人の設定が**消えたまま**残る作りだった。
# いまは試験用の家がまっさらなので、何も消さなくても初めての人になる。
set -e
here="${0:A:h}"
root="${here:h}"
work="${TMPDIR:-/tmp}"
work="${work%/}/amber-first"
# 本物の設定。**読むだけ** ── 変わっていないかを見比べるために、指紋を取る。
mine="$HOME/Library/Application Support/amber/amber.json"
port="${PORT:-9334}"
home="$work/home"

print_mine() {
  if [ -f "$mine" ]; then
    echo "$(stat -f '%m %z' "$mine") $(shasum -a 256 "$mine" | cut -d' ' -f1)"
  else
    echo "なし"
  fi
}

quit() {
  pkill -f 'Electron.*remote-debugging-port='"$port" 2>/dev/null || true
  sleep 1
  # **本物の設定が変わっていないかを、何があっても見る**（`walk.sh` と同じ）。
  if [ -n "${before:-}" ]; then
    if [ "$(print_mine)" = "$before" ]; then
      echo "本物の設定は変わっていません"
    else
      echo "✗ 本物の設定（$mine）が変わりました"
      echo "  試験用の家（AMBER_TEST_HOME）が効いていません。gui/main.js を見てください。"
      exit 3
    fi
  fi
}
trap quit EXIT INT TERM

rm -rf "$work"
mkdir -p "$home"
before="$(print_mine)"

(cd "$root" && cargo build -q -p amber-server) || { echo "エンジンが作れません"; exit 2 }

# **`--use-mock-keychain` が要る**（`walk.sh` と同じ）。試験用の家の引き出しは
# まっさらなので、渡さないと Chromium が鍵束に問い合わせ、本人の画面に
# 「キーチェーンが見つかりません」を出して止まる。
(cd "$root/gui" && HOME="$home" AMBER_TEST_HOME="$home" npx electron --remote-debugging-port="$port" --use-mock-keychain . \
  >"$work/win.log" 2>&1 &)
for i in $(seq 1 40); do
  curl -s -m 1 "http://127.0.0.1:$port/json" >/dev/null 2>&1 && break
  sleep 0.5
done

PORT="$port" HOME_DIR="$home" node "$here/first-run.mjs"
