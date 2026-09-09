---
name: check-all
description: amber の全部の動作チェック。いつもの検査（Rust・js・台帳・電話の組み立て）を回し、そのうえで窓と電話を実際に押して回って、壊れているものと想定外の挙動を探す。「動作チェック」「全部チェックして」「一通り確かめて」と言われたら、これ。
---

# 全部の動作チェック

**単体の検査が通っていても、実物では落ちる。** それが何度もあったので、
ここは三段になっている ── 検査を回す・押して回る・**まだ誰も見ていない
ところを名指しで言う**。

三つ目がいちばん大事。**「ぜんぶ通りました」は「ぜんぶ見ました」ではない。**
最後に必ず「見ていないところ」を出す（下の一覧をそのまま使う）。

順にやる。前が落ちたら、そこで止めて報せる。

---

## 一。いつもの検査

```bash
cd ~/workspace/amber
cargo test --workspace
cargo clippy --workspace --all-targets
for t in diagram-test paper-test win-test switch-test contract round-test key-test web-test; do
  printf '%-14s ' "$t"; node scripts/$t.js 2>&1 | tail -1
done
python3 scripts/requests.py     # 台帳（依頼が守られているか）
python3 scripts/shipped.py      # 未リリースの数
./scripts/ios-build.sh && xcodebuild -project ios/Cian.xcodeproj -scheme Cian \
  -sdk iphonesimulator -destination 'platform=iOS Simulator,name=iPhone 16 Pro' build \
  2>&1 | grep -E 'error:|\*\* BUILD'
```

**どれが何を守っているか**（重なりと隙間を見るために）:

| 検査 | 見ているもの |
| --- | --- |
| `cargo test` | core の判断（題・前書き・混ぜる・履歴・絵の大きさ・絵文字の表） |
| `paper-test` | 面 → 字（手で書いた HTML から） |
| `round-test` | **字 → 面 → 字の往復**（core と切り出しをまたぐ・81 件） |
| `key-test` | 鍵を押してからの往復（Backspace・Tab・Enter・矢印） |
| `switch-test` | ノートを替えたとき、本文が混ざらないこと |
| `diagram-test` | 図の組み立て |
| `win-test` | Windows でだけ出るもの（`file://`・鍵の名前） |
| `web-test` | よそから来た HTML → ノートの字（15 件） |
| `contract` | crmaine との約束（名前を黙って変えていないか） |
| `requests.py` | 台帳 ── **人が頼んだことが、まだ守られているか** |

台帳が落ちたら、**直す前にまず訊く** ── そこに書いてあるのは人が頼んだ
ことで、勝手に消してよいものではない。

---

## 二。窓の総ざらい（自動）

```bash
./scripts/walk.sh
```

百とおりほどを実際に押して、三つを見張る:

1. 例外が飛ばないこと
2. `console.error` が出ないこと
3. **触ったあと、そのノートがまだ字に戻せること**（`paperToMd`）──
   ここが `null` になったノートは、見た目は何ともないのに保存が黙って止まる

回っているのは: 面の入れ替え・目次・字の大きさ・列の開け閉め・行き先と
絞り込み・並び順・言葉で探す・タブ・まとめて選ぶ・**記号の帯ぜんぶ（両方の
面で）**・升・絵文字・絵の大きさ・貼り付け・ノートを作る／複製／型から／
題を直す・★・フォルダと色・履歴・パレット・献立・**右押し八か所**・
**うまくいかないとき**（無い道・外へ出られない・壊れた HTML・競合の控え）。

### 触ってはいけないもの

`walk.sh` は自分で守っているが、**手で窓を出すときは自分で守る**：

- **本人のノート（`~/Documents/cian`）では絶対に試さない。**
  隔離した `$HOME` を渡す（`HOME=... npx electron ...`）と、ノートは
  そちらの `Documents/` にできる。
- **設定ファイルは、始める前に写しを取り、終わったら戻す。**
  macOS の Electron は `appData` に `$HOME` を見ない ── 隔離した `$HOME`
  を渡しても、`~/Library/Application Support/amber/amber.json` は
  **本物のほうに書かれる**。2026-09-09 に実際に書き込んだ（`open` と
  `tabs` が消えるフォルダを指し、次に開いたとき「開けません」になる
  ところだった）。

窓は `gui/` から出す（`cd gui && npx electron .`）── repo の根には
`package.json` が無い。エンジンを直したら **`cargo build` してから窓を
出し直す**（窓は起動時の実行ファイルを掴んだままなので、`ipcMain` の
新しい口も新しい op も、出し直すまで無い）。

---

## 三。電話（手で押す）

自動では回せない。iOS シミュレータの MCP で押して回る。

```bash
./scripts/ios-build.sh
xcodebuild -project ios/Cian.xcodeproj -scheme Cian -sdk iphonesimulator \
  -destination 'platform=iOS Simulator,name=iPhone 16 Pro' build
xcrun simctl install BOOTED ~/Library/Developer/Xcode/DerivedData/Cian-*/Build/Products/Debug-iphonesimulator/Cian.app
xcrun simctl launch BOOTED com.taketan.cian
# 落ちたことは、ログに出る（画面には出ない）
xcrun simctl spawn BOOTED log stream --style compact \
  --predicate 'processImagePath CONTAINS "Cian" AND messageType == error'
```

**画面ごとに一つずつ**（`ios/Cian/*.swift` と同じ並び）:

- `ContentView` 一覧 ── 開く／横に払って ★／長押しの献立（新しいタブ・
  複製・移す・エクスポート・共有・過去バージョン・ゴミ箱）／並べ替え／絞り込み
- `Desk` 帯 ── 表示 ⇄ コード・ベル・⋯ の献立・タブの行き来
- `Writing` / `Paper` ── 矢印で caret・升を押す・記号の帯ぜんぶ・
  絵文字の板・図や枠や絵を叩いて吹き出し（大きさ・消す・コードで直す）
- `Making` ＋ 新しいノート ── 題・タグ・**型から**
- `Nest` フォルダ、`Tagging` タグ、`Stars` ブックマークの棚
- `Sifting` 絞り込み、`Where` 置き場所、`Naming` 名前
- `Past` 過去バージョン、`Studio`/`Drawing` 図の工房、`Tabling` 表、`Syntax` 書き方
- `Bell`/`Booking`/`Ringing` 通知、`Touring` 案内
- 歯車の中ぜんぶ

**押した結果は、必ずファイルで確かめる。** 画面が変わっただけでは
保存されたことにならない（何度もそれで騙された）:

```bash
D="$(xcrun simctl get_app_container BOOTED com.taketan.cian data)/Documents"
cat "$D/amber へようこそ.md"
```

### 座標のこと

スクリーンショットの絵と、`tap` に渡す点は**尺が違う**。
**絵の座標を 2.286 で割る**と点になる（iPhone 16 Pro）。ここを間違えて
一つ下の釦を押し、「SwiftUI が壊れている」と結論しかけた（2026-09-09）──
**押す前にスクリーンショットを撮り、押したあとも撮って、狙ったものが
動いたか見る。**

### 試したら、戻す

見本ノートを書き換えたら戻す。作ったノートは消す。

---

## 四。**まだ誰も見ていないところ**（毎回、名指しで報せる）

ここは「通った」に混ぜてはいけない。**触れていないと言う。**

### OS の小窓が出るので、自動では押せない

`バックアップ` / `バックアップから戻す` / `ノートを取り込む` /
`ambər フォルダ以外を開く` / `エクスポート` / `PDF で保存` /
`保存ディレクトリ変更` / `ゴミ箱へ入れる`（OS のゴミ箱へ出る） /
絵の貼り付け（ファイル選び） / 通知の許可

→ 要るときは**手で一度押す**。押していないなら、そう言う。

### 仕組みとして、まだ検査が無い

- **窓と電話で、同じノートが同じ字になるか** ── 切り出しを共有している
  意味はそこなのに、二つを突き合わせる検査が無い（電話側の糊
  `Paper.swift` の中の JS は、窓と別に書いてある）
- **同期と競合** ── 競合の控えを開くところまでは回るが、**混ぜる**
  （`merge`）を実物で通していない
- **速さの回帰** ── 一度測った（開く 236 倍）が、遅くなったことに気づく
  仕掛けが無い
- **大きいノート** ── 一万行のノートで、面の組み直しと保存が保つか
- **文字コードと改行** ── CRLF・BOM・Shift_JIS のノートを実物で開く
  （core には試験がある。窓と電話では通していない）
- **鍵だけで全部できるか** ── 命令に鍵はあるが、押さずに一周する道を
  確かめていない
- **Windows の実物** ── CI に一往復はあるが、手で触っていない。
  「mac では一生出ない」類を**四度**踏んでいる（`file://`・鍵の名前・
  絵の道・改行）
- **初回の起動** ── ノートが一本も無いところから、見本が入って置き場所が
  決まるまで

---

## 五。報せ方

- 落ちたものは、**何をしたら何が起きたか**を書く（「壊れています」では直せない）
- 通ったものは数で言う（一つずつ並べない）
- **四の一覧を必ず添える** ── 見ていないところが見えていないのが、いちばん危ない
- 走査そのものが間違えることがある（名前を推測した・入切を絶対値で見た）。
  **落第が出たら、まず「これは本物か、走査の側か」を切り分けてから報せる**
