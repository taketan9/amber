---
name: check-all
description: amber の全部の動作チェック。いつもの検査（Rust・js・台帳・電話の組み立て）を回し、そのうえで窓と電話を実際に動かして、壊れているものと想定外の挙動を探す。「動作チェック」「全部チェックして」「一通り確かめて」と言われたら、これ。
---

# 全部の動作チェック

**単体の検査が通っていても、実物では落ちる。** それが何度もあったので、
ここは二段構えになっている ── 検査を回すのと、**実際に押して回る**の。

順にやる。前が落ちたら、そこで止めて報せる（落ちた土台の上で動かしても、
出てくるのは二次被害だけ）。

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

台帳が落ちたら、**直す前にまず訊く** ── そこに書いてあるのは人が頼んだ
ことで、勝手に消してよいものではない。

## 二。窓の総ざらい（自動）

```bash
./scripts/walk.sh
```

六十とおりほどを実際に動かして、三つを見張る:

1. 例外が飛ばないこと
2. `console.error` が出ないこと
3. **触ったあと、そのノートがまだ字に戻せること**（`paperToMd`）──
   ここが `null` になったノートは、見た目は何ともないのに保存が黙って止まる

落ちたものだけ出る。ぜんぶ通れば一行。

### 触ってはいけないもの

`walk.sh` はこの二つを自分で守っているが、**手で窓を出すときは自分で守る**：

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

## 三。電話（手で）

自動では回せないので、iOS シミュレータの MCP で押して回る。

```bash
./scripts/ios-build.sh
xcodebuild -project ios/Cian.xcodeproj -scheme Cian -sdk iphonesimulator \
  -destination 'platform=iOS Simulator,name=iPhone 16 Pro' build
xcrun simctl install BOOTED ~/Library/Developer/Xcode/DerivedData/Cian-*/Build/Products/Debug-iphonesimulator/Cian.app
xcrun simctl launch BOOTED com.taketan.cian
```

見るところ（**これで一通り**）:

- 一覧 ── 開く／横に払って ★／長押しの献立（新しいタブ・複製・移す・
  エクスポート・共有・過去バージョン・ゴミ箱）
- ノート ── 表示とコードの入れ替え、矢印での caret 移動、升を押す、
  記号の帯ぜんぶ、絵文字の板、図や枠や絵を叩いて吹き出し（大きさ・消す）
- 作る ── ＋ 新しいノート（題・タグ・**型から**）
- 設定 ── 歯車の中

**押した結果は、必ずファイルで確かめる。** 画面が変わっただけでは
保存されたことにならない（何度もそれで騙された）:

```bash
D="$(xcrun simctl get_app_container BOOTED com.taketan.cian data)/Documents"
cat "$D/amber へようこそ.md"
```

### 座標のこと

スクリーンショットの絵の大きさと、`tap` に渡す点は**尺が違う**。
絵の座標を **2.286 で割る**と点になる（iPhone 16 Pro）。ここを間違えて、
一つ下の釦を押していたのに「SwiftUI が壊れている」と結論しかけた
（2026-09-09）── **押す前にスクリーンショットを撮り、押したあとにも
撮って、狙ったものが動いたか見る。**

### 試したら、戻す

見本ノートを書き換えたら元に戻す。作ったノートは消す。
次に走らせる人が、同じところから始められるように。

## 四。報せ方

- 落ちたものは、**何をしたら何が起きたか**を書く（「壊れています」だけでは直せない）
- 落ちなかったものは、数で言えばよい（一つずつ並べない）
- 触れなかったもの（ファイル選び・印刷・ゴミ箱など、OS の小窓が出るもの）は
  **触れなかったと言う** ── 通ったことにしない
