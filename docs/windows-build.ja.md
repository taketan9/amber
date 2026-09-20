# ネットに出られない Windows で使う

## Release に置いてあるもの（依頼 641）

**その環境で作れないものだけ**置いてある。4 つ:

| 資材 | | |
|---|---|---|
| `amber-src.zip` | 10MB | **会社へ持ち込む一式。** ビルドに必要なものが全部入っている |
| `amber-dev.zip` | 40MB | 判断の側（Rust）まで直したいとき |
| `amber-gui.zip` | 4.5MB | 同梱する側（crmaine）が `vendor-amber\` に置く画面一式 |
| `amber-server-win-x64.exe.zip` | 1.7MB | 同上。エンジンの実行ファイル |

**出来合いの `amber-win-x64-<版>.zip` は、もう置いていない**（2026-09-20 に
本人が決めた）。会社の端末には Node と Electron があり、下の一行で**同じもの
が作れる**ので、120MB を版ごとに置く意味が無くなった。

そのとき一緒に決まったこと ── **Windows の ambər に、同期と Google
カレンダーは要らない。** 下のコマンドは既定で**会社向け（office）だけ**をビルドするので、
そのまま叩けばそうなる（同期・iCal 購読・グループ共有は、メニューにも ⌘⇧P にも
出ないし、訊きにも行かない）。

**カレンダー画面と、チームの CSV は残っている** ── あれは会社の Outlook が
書き出したファイルを読むだけで、外へは何も出さない（[team-csv.ja.md](team-csv.ja.md)）。

中身の違いは `gui\edition.json` が 1 つあるかどうかだけ。手元で見え方を比べるなら、
環境変数のほうが早い:

```bash
AMBER_EDITION=office ./gui/run.sh
```

mac と Linux のエンジン・`amber-cal-mac` も置いていない ── 手元の Mac に
Rust と Xcode が揃っているので、そこで組めば出る。**CI は今までどおり全部
ビルドしている**ので、必要なときは `gh run download` で取れる。

---

下は、**自分で組みたいとき**の話。

---

## いちばん軽い方法 ── `amber-src.zip` を持ち込む（依頼 639）

cian や crmaine と同じ形。**持ち込むのは 1 ファイルだけ、会社ではビルドするだけ。**

1. Release から **`amber-src.zip`**（10MB ほど）を落として、USB で運ぶ
2. 会社の端末で展開する（`amber-src\` が一つできる）
3. その中でコマンドプロンプトを開いて、一行:

```
node scripts\build-win.js --electron C:\electron-v33.4.11-win32-x64
```

`dist\` に**三つ**できる:

| | |
|---|---|
| `amber-gui.zip` | 同梱する側（crmaine）へ渡す画面一式 |
| `amber-server-win-x64.exe.zip` | エンジンの実行ファイル |
| `amber-win-x64-office-<版>.zip` | 会社向けの ambər 本体 |

- **要るのは Node と Electron の一式だけ。** Rust も npm も python も要らない
  （エンジンと `gui/vendor/` は、一式の中に入っている）
- **Electron は入っていない**（百メガあり、会社には cian のぶんが既にある）
- 名前とアイコンを exe に埋め込むなら `--rcedit C:\tools\rcedit-x64.exe` を足す
- ふつうの版（同期入り）も要るなら `--full` を足す
- **ネットワークには一度も出ない**

手元で `amber-src.zip` を組み直すなら:

```bash
cd gui && npm ci && node vendor.js && cd ..
gh release download --pattern amber-server-win-x64.exe --dir dist
node scripts/src-zip.js --engine dist/amber-server-win-x64.exe --out out/amber-src.zip
```

**CI は、この一式から会社と同じコマンドでビルドして、3 つできたかを数えている**
（`release.yml` の「一式から、会社と同じ手順でビルドしてみる」）── 配ってから
「あちらでビルドしたら落ちた」がいちばん高くつく。

---

## 修正までやる方法 ── `amber-dev.zip`（依頼 640）

**判断の側（Rust）まで会社で直したいとき。** Release の `amber-dev.zip`
（**40MB**）と、**Rust 本体**を持ち込む。

```
amber-dev\
  はじめに読んでください.txt                  ← Rust の落とし先が書いてある
  amber\                                     ← ソース一式（試験も台帳も）
    .cargo\config.toml                       ← 依存は隣の vendor から
    vendor\                                  ← 依存の実体（cargo vendor）
    amber-server-win-x64.exe                  ← 出来合いのエンジン
```

**Rust 本体（375MB）は、この一式に入っていない**（依頼 641）。amber の版が
変わっても 1 バイトも変わらないので、バージョンごとに置き直すと同じ 375MB がタグの数だけ
積み上がる（v3.1.2〜v3.1.7 で実際に 1.66GB 積んで、消した）。ネットに出られないのは
**会社の端末**で、落とす人の手元は外に出られる ── **一度だけ**落として、
`amber-dev.zip` と一緒に USB へ入れる。次からは同じものを使い回せる。

```
https://static.rust-lang.org/dist/rust-<版>-x86_64-pc-windows-gnu.msi
```

版は `はじめに読んでください.txt` に焼き込んである ── **CI がそれで「組めた」
を見た版**なので、会社で入る Rust と、ここで確かめた Rust が同じものになる。

1. 落とした `rust-<版>-x86_64-pc-windows-gnu.msi` をダブルクリックして入れる
   （**Visual Studio は要らない** ── このインストーラに、ビルドに必要なものが全部入っている）
2. `cd amber`
3. ビルド: `cargo build --release -p amber-server --target x86_64-pc-windows-gnu`
4. 試す: `cargo test --workspace` ／ `python scripts\requests.py`

**回らないもの**:

- jsdom を使う試験（`paper-test` ほか）── npm が要る
- iPhone 側 ── Mac と Xcode が要る

**なぜ MSVC 版ではないか。** あちらは Visual Studio の Build Tools が別に
要る（数 GB）。自己完結版（gnu）ならインストーラ 1 つで足りる ── **CI が毎回その方法で
エンジンを組み、動かしてから包んでいる**（`release.yml` の「自己完結版（gnu）で、
エンジンをビルドしてみる」）。

**端末のディスク**は、Rust 本体で 1.2GB ほど、`target\` が 2〜3GB 育つ。

---

## Electron ごと持ち込む方法（`offline-kit`）

会社に Electron が無いときは、こちら（300MB ほど）。

### 家でやること（一度）

```bash
cd gui && npm install && node vendor.js   # エディタと図の実体（16MB・git には入らない）
cd .. && gh release download --pattern amber-server-win-x64.exe --dir dist
node scripts/offline-kit.js --out dist --zip
```

Electron の一式（`electron-v33.4.11-win32-x64`）は、リポジトリの隣にあれば
勝手に拾う。無ければ
[electron/releases](https://github.com/electron/electron/releases) から
`electron-v33.4.11-win32-x64.zip` を落として展開し、`--electron` で指す。

`dist/amber-kit-<版>.zip`（300MB ほど）ができる。**これを USB で運ぶ。**

中身:

| | |
|---|---|
| `amber/` | いまの **HEAD** の中身（`git archive`）＋ `gui/vendor/` |
| `electron-v33.4.11-win32-x64/` | Electron の一式 |
| `amber-server-win-x64.exe` | エンジン。**CRT ごと静的**なので、置くだけで動く（Visual C++ の再頒布可能パッケージが要らない） |
| `rcedit-x64.exe` | exe のアイコンと名前を埋め込むツール（`--rcedit` で入れたときだけ） |
| `組み方.txt` | 下と同じことが、実際のパスつきで書いてある |

**コミットしていない変更は入らない。** `git archive` が見るのは HEAD なので、
直したばかりのものを持っていきたいなら先にコミットする ── まとめるときに
「入るのは HEAD の中身です」と言うので、そこで気づける。

---

### 会社でやること（`offline-kit` を持ち込んだとき）

要るのは **Node.js だけ**（`node --version` が通ること）。ネットワークには一度も出ない。

```
cd C:\amber-kit\amber

node scripts\pack.js --out dist --platform win32 ^
  --electron ..\electron-v33.4.11-win32-x64 ^
  --engine ..\amber-server-win-x64.exe ^
  --rcedit ..\rcedit-x64.exe ^
  --zip
```

`dist\amber-win-x64\amber.exe` ができる。`--zip` を付けると配布用の zip も出る。

**会社向けをビルドするなら** `--edition office` を足す ── 出力先が
`dist\amber-win-x64-office\` に変わる（名前を分けないと、どちらを配って
いるのか手元で見分けられない）。

**cian と同じ綴りで打てる。** `--platform win32` は `--win` と、`--engine` は
`--server` と同じものを指す ── 手が憶えているほうが正しい。

---

## 踏みやすいところ

- **`gui/vendor/` がありません** ── 持ち込んだ一式の `amber\gui\vendor` が
  欠けている。あれは `npm install` が置いたものを写したもので、**git には
  入っていない**ので、ネットに出られない環境では作れない。持ち込み直す。
  写す前に言うようにしてある（Electron を二百メガ写したあとで落ちない）
- **exe の絵が Electron のまま** ── `--rcedit` を渡していないか、Windows
  以外でビルドしている。アイコンと名前は PE のリソースなので、**Windows の上でしか
  埋め込めない**
- **zip の日本語の名前が化ける** ── `Compress-Archive` で組み直していないか。
  `scripts/zip.js` は UTF-8 フラグ（汎用ビット 11）を必ず立てる。あちらは
  立てない
- **エンジンだけ古い** ── `amber-server-win-x64.exe` は Release の資材なので、
  リポジトリを新しくしても勝手には変わらない。版を上げたら落とし直す

---

## なぜ electron-builder を使わないか

あれはビルド中に取りに行く。**ネットに出られない環境では動かないし、出られても途中で
落ちると半端な生成物が残り、それが正常に見える**（crmaine が whl と vsce で
二度やった）。`scripts/pack.js` は写すだけで作り、**出口で必ず数えて**、
欠けていれば失敗させる。
