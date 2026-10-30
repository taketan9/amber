# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this project is

amb**ə**r は **Advanced Markdown Browser & Editor for Readability**。
ノートはただの Markdown ファイルで、**独自形式も隠しDBも持たない** ──
front matter が題とタグ、フォルダがそのまま索引。狙いはライト層（iPhone を
好むような人）で、売りは**書きやすさ・使いやすさ・美しさ**。

- `crates/amber-core` ── 判断ぜんぶ（Rust）
- `crates/amber-ffi` ── iPhone の扉（`amber_call` / `amber_free`）
- `crates/amber-server` ── デスクトップ版の扉（標準入出力に JSON 一行ずつ）
- `gui/` ── Electron。三列（行き先・ノート・中身）
- `ios/` ── SwiftUI（`ios/Cian.xcodeproj`。名前は歴史的理由でそのまま）

**扉は二つ、答えは一つ。** iPhone もデスクトップ版も `amber_core::api::call` に
訊く。片方に判断を写した瞬間、「同じ操作なのに Mac と iPhone で結果が違う」が
一度の編集で作れる。

---

## ⛔ 開発のルール（**関門のあるものと、無いものを分けて書く**）

**ルールは書いただけでは守られません。** 関門のある行は機械が落とします。
`（手順）` の行は落ちません ── 守るのは人です。**関門を外すときは、理由を
コミットに書くこと。**

| ルール | 関門 |
| --- | --- |
| **頼まれたことは `REQUESTS.ja.md` に1行**。消したら落ちる | `python3 scripts/requests.py`（592 件） |
| **検査を足したら必ず変異テスト。** 壊した版で落ちるのを見てから戻す | `scripts/mutate.sh FILE FROM TO` |
| **註は普通の日本語で。** 比喩と英語の地の文を入れない | `node scripts/plain-test.js`（152 ファイル・15 語） |
| **判断は core に一度だけ書く。** 画面側に写さない | `node scripts/round-test.js` ほか往復の試験 |
| **表示画面の編集は、位置 × 操作の総当たりを通す** | `./scripts/walk.sh grid`（2755 件・約 50 分） |
| **画面の動きは本物のアプリで通す** | `./scripts/walk.sh`（300 件）・`./scripts/walk-phone.sh`（938 件） |
| **同梱する側（crmaine）との約束を壊さない** | `node scripts/contract.js` |
| **葉の形は二か所で一致させる** | `python3 packaging/amber.py`（**終了コード 1 が合格**） |
| **核のソースを触ったら iOS のエンジンを建て直す** | Xcode のビルド段（`エンジンがソースより古いです`） |
| **Electron の試験を二つ同時に回さない** | `scripts/walk.sh` の見張り（空くまで待つ） |
| **本人の amber が開いているときは試験を走らせない** | `scripts/walk.sh` の見張り（断る） |
| **Windows でも core の判断を通す** | CI（`cargo test --workspace` を Windows でも） |
| **識別子は英語**（変数・関数・型）。**註と画面の文言は日本語**。Rust の試験名だけ日本語 | （手順・**関門はまだ無い**） |
| **主張は自分で再現してから直す。** 人の報告も、自分の推測も | （手順） |
| **版番号とタグは本人が決める。** 自分から `Cargo.toml` の version に触らない | （手順） |
| **`git add -A` を使わない**（同じ作業場に二つのセッションが居る） | （手順） |

---

## 🔒 絶対に変えないもの

変えると**本人の端末からノートと設定が消えます。**

| | なぜ |
| --- | --- |
| バンドル ID `com.taketan.cian` | 変えると iOS は別アプリと見なし、iPhone のノートと設定が全部消える |
| Xcode のターゲット名と `ios/Cian` | 同じ理由。**表示名は既に `amber`** なので、人が見るところは変わらない |
| `cian.look` / `cian.autosave` / `cian.notes.root` などの UserDefaults キー | 変えると設定が消える。**新しい鍵は `amber.` で始める** |
| `~/Documents/cian` | いま本人のノートが入っている。デスクトップ版の既定はここを見に行く |

---

## 💀 踏んだバグの型（**同じ形で何度も出る**）

1. **画面に出ているのに、保存されない** ── いちばん怖い型。気づくのは何回か
   保存したあと。`blockToMd` が読まない場所に文字が入ると起きる
   （`</code>` の外・箱の直下の裸の文字節）
2. **検査が黙る** ── 「指摘が消えた」と「検査が何も見ていない」は画面の上で
   同じ顔をしている。だから変異テストが要る
3. **画面から拾った文字が、画面の都合を連れてくる** ── 表示画面のコード枠は
   Monaco が色を付けたあとの姿で、空白が U+00A0、タブが空白四つ。
   `textContent` で拾うと**写して貼ったら動かないコード**になる
4. **パスで持っているものを、片方だけ繋ぎ直す** ── ノートを移すとパスが変わる。
   タブの `t.path` は直っても `t.keep.open.path` が古いまま、など
5. **一語を二つの意味で使っていて、機械で置き換える** ── 置き換える前に、
   その語の全用例を目で見る。**ファイル名も見る**（`網.md` → `総当たり.md` の
   片直しで、総当たりが一か月走っていなかった）
6. **決めごとの無いところを、自分の美意識で決める** ── 本人の指定を後のパスで
   消さない。消す前に訊く

---

## 🧭 迷ったときの寄りどころ

- **「まだ作っていない」と「作らないことにした」は、画面の上でもコードの上でも
  同じ顔をしている。** どちらなのかを `PLANS.ja.md` に書く
- **押せるのに効かないボタンを作らない。** 選ばせておいて断るのがいちばん悪い
- **caret の無いものが一打で消えるのは事故。** 消すパスは、押したときの吹き出し
- **鳴らないアラームより、鳴らないと分かっているほうがよい。** 効かない場面が
  あるなら、画面がそう言う
- **観測したことと、推測しただけのことを分けて言う**

---

## 🔧 毎回通すもの

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/requests.py          # 台帳
node scripts/plain-test.js           # 普通の日本語
./scripts/walk.sh                    # 総ざらい（300 件・約 4 分）
```

**版を上げるときは、これも**（`*test*.js` の輪に入っていない）:

```bash
node scripts/contract.js             # 同梱する側との約束
python3 scripts/onenote-test.py
bash scripts/onenote-mutate.sh
python3 packaging/amber.py           # 葉の形（終了コード 1 が合格）
```

**版は五か所**（`Cargo.toml` / `gui/package.json` / `gui/package-lock.json` /
`MARKETING_VERSION` 二つ）。順番は
`検査 → 版を五か所 → cargo build → ios-build.sh → commit → tag → push`。
**`ios-build.sh` を先に回さないと `phone-install.sh` が止まります。**

---

## 📱 iPhone

```bash
./scripts/ios-build.sh               # エンジン（3 ターゲット）
./scripts/phone-install.sh           # 実機に入れる
./scripts/walk-phone.sh              # 総ざらい（938 件）
```

- **無料署名は 7 日で切れる。** 延ばすには証書を消してから入れ直す
  （`rm ~/Library/Developer/Xcode/UserData/Provisioning\ Profiles/*.mobileprovision`）
- **初めてその端末に入れたときは、本人の操作が一度だけ要る** ──
  設定 → 一般 → VPN とデバイス管理 → デベロッパ APP → 信頼
- **入れたあとは端末から版を読んで確かめる** ──
  `xcrun devicectl device info apps --device <UDID>`。道具の「入れました」だけを信じない
- **`navigationDestination(isPresented:)` を同じ画面に二つ置くと、あとの一つしか
  効かない。** 行き先は道（`NavigationStack(path:)`）で持つ

---

## 🖥 この Mac の癖

- **rustup は keg-only で PATH に居ない**（`/usr/local/opt/rustup/bin`）。
  ツールチェーン側の cargo を使わないと「can't find crate for `core`」
- **`[profile.ios]` が要る** ── release は `strip` で `_amber_call` を消す
- **swap が 6GB しかない。** Electron の試験を二つ同時に回すと描く側が
  ページングで止まり、**落ちる段が毎回ばらける** ── コードの不具合と
  見分けがつかない。`uptime` と `sysctl vm.swapusage` を先に見る
- **総ざらいが「固まった」ように見えたら、まず機械を疑う。** CPU 0.1% は
  固まっている証拠にならない（一件ごとに `await` で待つので）

---

## 📚 ほかの文書

| | |
| --- | --- |
| [REQUESTS.ja.md](REQUESTS.ja.md) | 果たしたことの台帳（機械が検査する） |
| [PLANS.ja.md](PLANS.ja.md) | これから果たすこと／やらないと決めたこと |
| [PAPER.ja.md](PAPER.ja.md) | 表示画面の設計図（いちばん壊れやすい場所） |
| [docs/on-device.ja.md](docs/on-device.ja.md) | 実機に入れる |
| [docs/open-with.ja.md](docs/open-with.ja.md) | ほかの形のファイルを開く |
