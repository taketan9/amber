# amber

**A Markdown Box for Everything we Remember**
── 憶えておきたいもの、ぜんぶ入るマークダウンの箱。

ノートはただの Markdown ファイル。フォルダがそのまま索引で、front matter が
題とタグ。**独自の形式も、隠しデータベースも無い** ── 別のアプリで開いても、
十年後に開いても、それはただのテキストファイル。

## いまあるもの

- **iPhone アプリ**（SwiftUI）── 一覧・編集・タグ・お気に入り・フォルダの色・
  画像の貼り付け・通知つきの繰り返し・バックアップと復元
- **`amber-core`**（Rust）── 判断ぜんぶ。**I/O と UI に依存しない。**
  題をどう決めるか、チェックをどう切り替えるか、AND/OR をどう解くか
- **`amber-ffi`** ── 扉は一つ。JSON を渡して JSON が返る（`amber_call` /
  `amber_free`）

## まだ無いもの

窓版（Mac / Windows）、同期、家族での共有、カレンダーの取り込み。

## 建て方

```
cargo test --workspace          # 判断のテスト
./scripts/ios-build.sh          # iPhone 向け（3ターゲット、記号まで見る）
python3 packaging/amber.py      # アイコンを焼く
python3 scripts/requests.py     # 頼まれたことが守られているか
```

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
