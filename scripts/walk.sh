#!/bin/zsh
# デスクトップ版の総ざらいを、**安全な場所で**一回やる。
#
#     scripts/walk.sh            # 総ざらい（walk.mjs）
#     scripts/walk.sh grid       # 網 ── 位置×操作の総当たり（grid.mjs）
#
# やること: 試す場所を作る → デスクトップ版を出す → `walk.mjs` を走らせる
#          → デスクトップ版を閉じる → 本物の設定が変わっていないかを見る
#
# **本人のノートと設定には触らない。** デスクトップ版に試験用の家
# （`AMBER_TEST_HOME`）を渡すと、設定（`amber.json`）・ドキュメント・
# 初めて開いたときの置き場所・クラウドのフォルダが、ぜんぶその下に向く
# （`gui/main.js`・依頼 717）。
#
# **前は本物の設定を写して、終わったら書き戻していた。** macOS の Electron は
# `$HOME` を作り替えても `appData` を移さないので、設定だけは本物のほうに
# 落ちる（2026-09-09 に実際に書き込んでしまった）。写しと戻しには弱点が
# 三つあった ── 途中で落ちると戻らない、走っているあいだは本物そのものが
# 書き換わる、本人の amber が開いていると同じファイルを取り合う。
#
# 終わりに**本物の設定の更新日時と中身を見比べ**、変わっていたら失敗で終える。
set -e
here="${0:A:h}"
root="${here:h}"
# **末尾の `/` を落としてから繋ぐ。** macOS の `TMPDIR` は `/` で終わるので、
# そのまま繋ぐと `…/T//amber-walk` と重なる。アプリはパスを**文字として**
# 見比べるので、`//` が保存ディレクトリに入ると**見張りの跳ね返りが
# 自分のものだと分からなくなる** ── `lastWrote` が消えず、「二台目：
# こちらで保存する」が落ちた（2026-10-17・二度続けて落として気づいた）。
tmp="${TMPDIR:-/tmp}"
work="${tmp%/}/amber-walk"
# 本物の設定。**読むだけ** ── 変わっていないかを見比べるために、指紋を取る。
mine="$HOME/Library/Application Support/amber/amber.json"
port="${PORT:-9333}"
# 試験用の家（`AMBER_TEST_HOME`）と、その中の設定。並びは `gui/main.js` の `testDirs`。
home="$work/home"
state="$home/appData/amber/amber.json"

# 本物の設定の指紋（更新日時・大きさ・中身）。無ければ「なし」。
print_mine() {
  if [ -f "$mine" ]; then
    echo "$(stat -f '%m %z' "$mine") $(shasum -a 256 "$mine" | cut -d' ' -f1)"
  else
    echo "なし"
  fi
}

quit() {
  pkill -f 'Electron.*remote-debugging-port='"$port" 2>/dev/null || true
  pkill -f "http.server $siteport" 2>/dev/null || true
  pkill -f "fake-drive.js $driveport" 2>/dev/null || true
  sleep 1
  # **本物の設定が変わっていないかを、何があっても見る。** 戻すのではなく
  # 見るだけ ── 試験用の家が効いていれば、そもそも触っていない。
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
mkdir -p "$work/home/Documents/amber" "$work/site"
notes="$work/home/Documents/amber"
mkdir -p "$notes/attachments" "$notes/仕事" "$notes/テンプレート" "$notes/家族"
# **二つ目の保存ディレクトリ**（依頼 511）── 走査の中で足して、外す。
notes2="$work/home/Documents/二つ目"
mkdir -p "$notes2/持ち帰り"
cat > "$notes2/別口.md" <<'MD'
---
title: 別口
---
# 別口

二つ目の保存ディレクトリのノート。#別口
MD
cat > "$notes2/持ち帰り/宿題.md" <<'MD'
# 宿題

持ち帰りのフォルダの中。
MD

# **本人の amber が開いていたら、始めない**（依頼 667・2026-10-17）。
#
# **この見張りを置いた理由は、依頼 717 で無くなった。** 前は引き出し
# （`userData`）を隔離できず、二つのアプリが同じ `amber.json` を取り合って、
# こちらが試し場に向けた設定を向こうがすぐ本物のノートで上書きし、
# **総ざらいが本人のノートの上で走った。** いまは試験用の家
# （`AMBER_TEST_HOME`）で引き出しが分かれているので、取り合わない。
# **外すかどうかは本人が決める**ので、それまでは残す（見張りは外すほうが
# 戻しにくい）。
#
# 2026-10-17 に実際に起きた ── 試験の画面が本人の `batchPro` のノートを開き、
# 同期先が drive の `~/Documents/cian` を、**空の偽 Drive** に向けていた。
# 止めずに進めば、ノートがゴミ箱へ入り得た。
# **配られているアプリの名前は `cian.app`**（2026-10-31 に気づいた）。
# 表示名は amber だが、アプリの名前は cian のまま ── バンドル ID と同じ理由で
# 変えられない。それを `amb` で始まると思い込んで書いたので、**この見張りは
# 依頼 667 で書かれてから一度も鳴っていなかった。** 気づいたのは、本人の
# amber が開いたまま総ざらいが走り出したのを見たとき。両方を見る。
if pgrep -f 'Applications/(amb|cian).*\.app/Contents/MacOS/' >/dev/null 2>&1; then
  echo "amber が開いています。終了してから、もう一度走らせてください。"
  exit 1
fi

# **よその試験が走っていたら、空くまで待つ**（依頼 683・本人が決めた）。
#
# この Mac では Electron の試験を二つ同時に回せない ── swap が 6GB しかなく、
# 描く側がページングで止まる。止まると「デスクトップ版が固まっている
# （1+1 も返らない）」で落ち、**落ちる段が毎回ばらける**ので、コードの不具合と
# 見分けがつかない。2026-10-30 だけで三度、無関係な段を追いかけた。
#
# 見るのは**作業場から起こした Electron の本体**（`gui/node_modules/.../MacOS/Electron`）。
# 下働き（Helper）は道に `Frameworks` が入るので当たらない。この見張りは自分を
# 起こす前に走るので、自分自身は数えない。
busy() { pgrep -f 'gui/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron' 2>/dev/null | wc -l | tr -d ' '; }

# **一度空いただけでは始めない**（2026-10-31）── よその試験は Electron を
# **起こして落とし、また起こす**ので、そのあいだに必ず空く瞬間がある。
# そこで始めると向こうの次の一つとぶつかり、**関係のない段で落ちる** ──
# 209 段目で実際に落ちた（この見張りが「空きました」と言った直後）。
# 続けて空いていることを見る。10 秒ごとに見て、空いていれば数を増やし、
# 走っていれば 0 に戻す。$1 回続けて空いたら 0、$2 回見てもそうならなければ 1。
#
# **走っているときも寝る。** 最初にこれを「空いていなければすぐ戻る」形で
# 書いたら、外側の待つ繰り返しが**一瞬で 15 回まわり切って**「まだ走って
# います」と言った ── 30 分待つはずのものが、1 秒で諦めた。
quiet() {
  run=0; seen=0
  while [ "$seen" -lt "$2" ]; do
    if [ "$(busy)" = "0" ]; then
      run=$((run + 1))
      [ "$run" -ge "$1" ] && return 0
    else
      run=0
    fi
    seen=$((seen + 1))
    sleep 10
  done
  return 1
}
# はじめから空いていても 30 秒は見る ── 段のあいだの隙間は数秒なので、
# これで見分けられる。走っていたら、続けて 120 秒空くまで最大 30 分待つ。
if [ "$(busy)" != "0" ] || ! quiet 3 3; then
  echo "よその試験が走っています。空くまで待ちます（最大 30 分）。"
  if ! quiet 12 180; then
    echo "まだ走っています。向こうが終わってから、もう一度走らせてください。"
    echo
    echo "  二つ同時に回すと描く側がページングで止まり、"
    echo "  関係のない段で「固まっている」と落ちます。"
    exit 1
  fi
  echo "空きました。始めます。"
fi

# 本物の設定の指紋。終わりの片付けが見比べる。
before="$(print_mine)"

# **ようこそ画面と案内は、憶えた状態で始める**（依頼 654・656）── どちらも
# 画面ぜんぶを覆い、案内は次へを押すまで進めない。憶えさせずに始めると
# **282 段が一段目から通らない。**
#
# **書くのは試験用の家の設定**（`$state`）。デスクトップ版に `AMBER_TEST_HOME` を
# 渡すので、Electron はこちらを読む。本物の設定には触らない。
#
# 台本（`AMBER_DO`）で `showHello()` と `startTour()` を直に呼ぶ段があるので、
# 出なくても確かめられる。
#
# **保存ディレクトリも、ここで試し場に向ける**（依頼 667）── `$HOME` を
# 作り替えても、保存ディレクトリは**絶対パスで設定に入っている**ので効かない。
# 本人が amber を実際に使いはじめてから、設定が本物のノートを指すようになり、
# **総ざらいが本人のノートの上で走る状態になっていた**（2026-10-17 に実際に
# そうなった）。同期先も切る ── こちらが立てるのは空の偽 Drive なので、
# 本物のノートに向けたまま同期が走ると「向こうでぜんぶ消えた」と読める。
mkdir -p "$(dirname "$state")"
AMBER_STATE="$state" AMBER_NOTES="$notes" python3 "$root/scripts/walk-first.py"

# ── 試すノート。**書けるものを一通り**入れておく ──
cp "$root/packaging/welcome/attachments/amber.png" "$notes/attachments/" 2>/dev/null || true
# **途中に書くための一本**（依頼 461）。書式の無い長い段落が一つ ──
# よくばり.md は書式で文字が細切れになるので、caret を「途中」に置けない。
cat > "$notes/途中.md" <<'MD'
---
title: 途中
created: 2026-09-01
---

# 途中

これは書式のない十分に長い段落なので、まん中あたりにカーソルを置けます。
MD

cat > "$notes/よくばり.md" <<'MD'
---
title: よくばり
created: 2026-09-01
tags: [仕事, 急ぎ]
---

# よくばり

段落です。**太字**と*斜体*と~~取り消し~~と`コード`と[リンク](https://example.com/a)。

## 一覧

- ひとつ
- ふたつ
  - 入れ子

1. 番号
2. 番号

- [ ] やること
- [x] やった

## 表

| 朝 | 夕 |
| :--- | ---: |
| 掃除 | 片づけ |

> 引用です。

> [!NOTE]
> 注記です。

```rust
fn main() { println!("{}", 1); }
```

![amber の印](attachments/amber.png)

```mermaid
graph TD
  A --> B
```

---

　全角のインデント。
MD
printf -- '---\ntitle: 買い物\ncreated: 2026-09-08\ntags: [暮らし]\n---\n\n# 買い物\n\n- 牛乳\n- パン\n' > "$notes/買い物.md"
printf -- '---\ntitle: からっぽ\ncreated: 2026-09-06\n---\n\n' > "$notes/からっぽ.md"
printf -- '---\ntitle: 段取り\ncreated: 2026-09-07\ntags: [仕事]\n---\n\n# 段取り\n\n## 朝\n\n本文。\n' > "$notes/仕事/段取り.md"
printf -- '---\ntitle: 週報のひな型\ncreated: 2020-01-01\n---\n\n# 週報\n\n## やったこと\n' > "$notes/テンプレート/週報のひな型.md"
printf -- '---\ntitle: 買い物リスト\ncreated: 2026-09-05\n---\n\n# 買い物リスト\n\n- 家族のぶん\n' > "$notes/家族/買い物リスト.md"
# **競合の控え。** クラウドが同時更新を見つけたときに置いていく形 ──
# これがある一覧を一度も開いていなかった（走査で気づいた）。
printf -- '---\ntitle: 買い物\ncreated: 2026-09-08\n---\n\n# 買い物\n\n- 牛乳を二本\n' \
  > "$notes/買い物 (Taketan の競合コピー 2026-09-08).md"

# **往復だけに使うノート。ほかの試しは触らない。**
# 前は「よくばり」で往復を見ていたが、そこは記号の帯を片端から押す先でも
# あって、往復にたどり着くころには壊れやすい行が残っていなかった ──
# 前後の空白を落とす壊し方を入れても鳴らなかった（2026-09-09）。
cat > "$notes/往復.md" <<'MD'
---
title: 往復
created: 2026-09-02
---

# 往復

　全角のインデントから始まる段落。
行末で折り返した  
つづき。

* 点は星のまま
* 二つめ

1. 番号は
1. 書いた通り
1. 3 つめ

> 引用。
> 二行目。

| 朝 | 夕 |
| :--- | ---: |
| 掃除 | 片づけ |

```rust
fn main() {}
```

![amber の印](attachments/amber.png)
MD

# **コードブロックの段が使うノート**（依頼 644）。色を付ける枠（言語つき）と
# 段落を混ぜておく ── 表示画面で選んで囲む・枠の中に打つ、をここで試す。
cat > "$notes/枠.md" <<'MD'
---
title: 枠
created: 2026-09-03
---

前の段落。

```js
const a = 1;
```

後ろの段落。

もう一つの段落。

```rust
fn main() {
    let x = 1;
}
```
MD

# **改名の段が使うノート**（依頼 492）。一行目で題が決まるもの・題が同じ二本目。
printf -- '名前は一行目から\n\n本文。\n' > "$notes/改名.md"
printf -- '# 買い物\n\n二本目。\n' > "$notes/二本目.md"

# **総当たりが使うノート**（`grid.mjs`）。中身は総当たりが毎回置き換える ── ここに
# あるのは「一本ある」ことと前書きだけ。前書きの無いほうも一本。
printf -- '---\ntitle: 総当たり\ncreated: 2026-09-01\n---\n\nここは総当たりのテストが使うノートです。\n' > "$notes/総当たり.md"
printf -- '一行目の段落。\n\n二行目の段落。\n' > "$notes/網なし.md"

# **よそから来た形のノート。** Windows で作られたもの・古い日本語のもの。
# core は読んだときの文字コード・BOM・改行のまま書き戻すが、**デスクトップ版を通した
# ときもそうか**は誰も見ていなかった。
python3 "$here/fixtures.py" "$notes"

# ── 取り込む先のページ（外へは出ない） ──
siteport="${SITEPORT:-8731}"
cat > "$work/site/index.html" <<'HTML'
<!doctype html><html><head><meta charset="utf-8">
<title>段取りの決め方 | 試しのページ</title></head><body>
<nav>案内</nav><article><h1>段取りの決め方</h1>
<p>まず<b>朝</b>に<a href="/次">次</a>を決めます。</p>
<ul><li>一つめ</li></ul><pre><code class="language-js">const a = 1;</code></pre>
</article><footer>足</footer></body></html>
HTML
# よその予定表（依頼 456）── 一度きり・終日でまたぐもの・毎週の3 つ。
printf 'BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:%s\r\nBEGIN:VEVENT\r\nDTSTART;TZID=Asia/Tokyo:20260904T183000\r\nSUMMARY:%s\r\nLOCATION:%s\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20260921\r\nDTEND;VALUE=DATE:20260924\r\nSUMMARY:%s\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;TZID=Asia/Tokyo:20260907T200000\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\nSUMMARY:%s\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n' \
  '家の予定' '歯医者' '駅前' '旅行' 'ごみ出し' > "$work/site/away.ics"
# チームの予定表（依頼 471）── 取り決め（`docs/team-csv.ja.md`）の形。
# BOM あり・CRLF・件名の中の読点・非公開・取り消し・終日のまたぎを含める。
python3 "$here/team-fixture.py" "$work/team.csv"

(cd "$work/site" && python3 -m http.server "$siteport" >/dev/null 2>&1 &)

# **偽の Google Drive**（`scripts/fake-drive.js`）── 同期の上げ下ろしを、Google
# 無しで回す。デスクトップ版には `AMBER_DRIVE_URL` と `AMBER_DRIVE_TOKEN` で「そこを
# Google だと思え・サインインは済んでいる」と教える。人が使うパスには出ない。
driveport="${DRIVEPORT:-8732}"
(node "$here/fake-drive.js" "$driveport" >/dev/null 2>&1 &)
sleep 0.5

# **エンジンを作り直してから出す。** デスクトップ版は起動時の実行ファイルを掴んだまま
# なので、直したはずの判断が効かないまま「通りました」になる（実際になった）。
(cd "$root" && cargo build -q -p amber-server) || {
  echo "エンジンが作れません"; exit 2
}

# ── デスクトップ版を出す ──
# **タイマーの絞りを切る。** デスクトップ版が隠れる（画面が消える・ほかのデスクトップ版の下に
# 入る）と、Chromium は数分でタイマーを一分に一度まで絞る ── 夜通し回した
# 総当たりが七時間かかり、「返ってきません」が混ざった（2026-09-10）。
# **引き出し（`userData`）は試験用の家に置く**（`AMBER_TEST_HOME`・依頼 717）。
# 2026-09-11 に一度これを試して、まっさらな引き出しで開いたデスクトップ版が数分で
# 固まり、やめていた（Chromium が鍵束に訊きに行って、裏の側が止まる・二度）。
# **固まったのは鍵束のせいで、その対策（下の `--use-mock-keychain`）が入ったのは
# 9 日後の 2026-09-20** ── 対策の入ったいまは、まっさらな引き出しでも
# 総ざらいが最後まで通ることを確かめてある（2026-11-20）。
# `AMBER_AWAKE` ── 後ろに隠れたテスト用のアプリを App Nap に止めさせない（`main.js`）。
# **`--use-mock-keychain` が要る。** `$HOME` を作り替えているので、Chromium が
# 鍵束を `$HOME/Library/Keychains` に探しに行って見つからず、**本人の画面に
# 「amber を保持するキーチェーンが見つかりません」を出して、答えるまで止まる**
# （2026-09-20 に実際に出した ── 走らせている本人が強制終了することになった）。
# この旗を渡すと、鍵束の代わりに其の場限りの入れ物を使うので、何も訊かない。
(cd "$root/gui" && env HOME="$home" AMBER_TEST_HOME="$home" AMBER_AWAKE=1 AMBER_DRIVE_URL="http://127.0.0.1:$driveport" AMBER_DRIVE_TOKEN=fake \
  npx electron --remote-debugging-port="$port" --use-mock-keychain \
  --disable-background-timer-throttling --disable-renderer-backgrounding \
  --disable-features=IntensiveWakeUpThrottling . \
  >"$work/win.log" 2>&1 &)
for i in $(seq 1 40); do
  curl -s -m 1 "http://127.0.0.1:$port/json" >/dev/null 2>&1 && break
  sleep 0.5
done

# どの台本を走らせるか（既定は総ざらい）。
script="${1:-walk}"
SITE="http://127.0.0.1:$siteport/" PORT="$port" NOTES="$notes" NOTES2="$notes2" TEAMCSV="$work/team.csv" TEST_HOME="$home" \
  DRIVE="http://127.0.0.1:$driveport" node "$here/$script.mjs"
