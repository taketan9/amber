# ambƏr

**Advanced Markdown Browser & Editor for Readability**
── 読みやすさのためのマークダウン閲覧・編集。

`Ə` はシュワー（曖昧母音）。大きく見えるが、ただの e の音。

ノートはただの Markdown ファイル。フォルダがそのまま索引で、front matter が
題とタグ。**独自の形式も、隠しデータベースも無い** ── 別のアプリで開いても、
十年後に開いても、それはただのテキストファイル。

## 走らせる

```
./gui/run.sh        # Mac / Linux
gui\run.bat         # Windows
```

どちらも同じことをする ── エンジン（`amber-server`）を建て、初回だけ
`npm install` と `node vendor.js` を通し、窓を開く。要るのは **rustup と
Node.js** の二つだけ。

**`gui/vendor/` は git に入れていない。** Monaco も vim も図もそこに置く
ものだが、数メガの決して変わらないものを複製のたびに永久に配る理由は無い
── `run.sh` / `run.bat` が無いときだけ `node vendor.js` を通す。

## いまあるもの

- **窓版**（Electron）── 三列（行き先・ノート・中身）、読む面で直に書ける、
  Markdown の道具、vim、目次、図（mermaid）、テーマ、書き出し
- **iPhone アプリ**（SwiftUI）── 一覧・編集・タグ・お気に入り・フォルダの色・
  画像の貼り付け・通知つきの繰り返し・バックアップと復元
- **`amber-core`**（Rust）── 判断ぜんぶ。**I/O と UI に依存しない。**
  題をどう決めるか、チェックをどう切り替えるか、AND/OR をどう解くか
- **`amber-ffi`** ── 扉は一つ。JSON を渡して JSON が返る（`amber_call` /
  `amber_free`）

## まだ無いもの

同期、家族での共有、カレンダーの読み書き。この三つは大玉なので、決めごとを
[PLANS.ja.md](PLANS.ja.md) にまとめてある ── **決まったこと・まだ決まって
いないこと・やらないと決めたこと**を分けて置く（「まだ作っていない」と
「作らないことにした」は、画面の上でも同じ顔をするので）。

## 建て方

```
cargo test --workspace          # 判断のテスト
node scripts/diagram-test.js    # 図が、直しても失われないか
node scripts/paper-test.js      # 「表示」の面が、打っても字を失わないか
node scripts/contract.js        # 同梱する側（crmaine）との約束が生きているか
node gui/vendor.js              # Monaco・vim・mermaid を落とす（git に入れていない）
./scripts/ios-build.sh          # iPhone 向け（3ターゲット、記号まで見る）
python3 packaging/amber_icon.py # アイコンを焼く
python3 scripts/requests.py     # 頼まれたことが守られているか
python3 scripts/shipped.py      # 窓に配られていないものが溜まっていないか
```

**iPhone も `gui/vendor/` を見る。** 図は電話でも mermaid が描くので、
`node gui/vendor.js` を通していない木から建てると**図の出ないアプリ**が
できる（落ちるのではなく、ノートは字のまま出る）。Xcode の「図の道具」
フェーズがそこを警告で言う。

**rustup は keg-only で PATH に居ない**（`/usr/local/opt/rustup/bin`）。
ツールチェーン側の cargo を使わないと Homebrew の rustc を拾い、
「can't find crate for `core`」で止まる ── `ios-build.sh` はそこを見ている。

## cian から分かれた

2026-09-05 まで、これは 2画面ファイラ [cian](https://github.com/taketan9/cian)
の中の一つのモードだった。分けた理由は数えて出た ── **iPhone は 18,128 行の
cian-core を丸ごと積んでいた**。git も svn も SharePoint も差分もアーカイバも、
ノートを書くのに要らないものが全部。

履歴はそのまま持ってきてある。`git log --follow crates/amber-core/src/note.rs`
は、これがファイラの一部だった頃まで遡る。
