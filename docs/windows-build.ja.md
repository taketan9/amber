# ネットに出られない Windows で使う

## まず、組まずに済ませる

**Release の zip が、展開するだけで動く一枚**になっている（依頼 553）。

1. [Releases](https://github.com/taketan9/amber/releases) から
   **`amber-win-x64-<版>.zip`**（120MB ほど）を落とす
2. zip を右クリック →「プロパティ」→「セキュリティ: 許可する」に印 → OK
   （**先に外しておく** ── あとからだと、展開したものに印が残る）
3. 右クリック →「すべて展開」
4. `amber.exe` をダブルクリック

git も gh も Node も要らない。ノートは「ドキュメント\amber」に置かれる。

### 二枚ある ── どちらを落とすか

| 資材 | 中身 |
|---|---|
| `amber-win-x64-<版>.zip` | ふつうの一枚 |
| **`amber-win-x64-office-<版>.zip`** | **会社向け** ── 外へ運ぶものが入っていない |

**会社向けの一枚には、同期が無い**（依頼 602）。Google Drive の同期、
iCal の購読（＝カレンダーの同期）、グループでの共有 ── 献立にも
⌘⇧P にも出ないし、**訊きにも行かない**（サインインの有無を訊くこと自体が、
外の鍵入れを開けにいくこと）。

**カレンダーの面と、チームの CSV は残っている** ── あれは会社の Outlook が
書き出した一枚を読むだけで、外へは何も出さない（[team-csv.ja.md](team-csv.ja.md)）。

中身の違いは `gui\edition.json` 一枚だけ。手元で見え方を比べるなら、
環境変数のほうが早い:

```bash
AMBER_EDITION=office ./gui/run.sh
```

**この一枚は GitHub の上で組んでいる** ── `gui/vendor/` が git に入って
いないのと、exe の絵が Windows でしか焼けないため。手元で組むのと同じ
`scripts/pack.js` を通っている。

下は、**自分で組みたいとき**の話。

---

## いちばん軽い道 ── `amber-src.zip` を持ち込む（依頼 639）

cian や crmaine と同じ形。**持ち込むのは一枚だけ、会社では組むだけ。**

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
| `amber-server-win-x64.exe.zip` | エンジン一枚 |
| `amber-win-x64-office-<版>.zip` | 会社向けの ambər 本体 |

- **要るのは Node と Electron の一式だけ。** Rust も npm も python も要らない
  （エンジンと `gui/vendor/` は、一式の中に入っている）
- **Electron は入っていない**（百メガあり、会社には cian のぶんが既にある）
- 名前と絵を exe に焼くなら `--rcedit C:\tools\rcedit-x64.exe` を足す
- ふつうの版（同期入り）も要るなら `--full` を足す
- **網には一度も出ない**

手元で `amber-src.zip` を組み直すなら:

```bash
cd gui && npm ci && node vendor.js && cd ..
gh release download --pattern amber-server-win-x64.exe --dir dist
node scripts/src-zip.js --engine dist/amber-server-win-x64.exe --out out/amber-src.zip
```

**CI は、この一式から会社と同じ一行で組んで、三つ出来たかを数えている**
（`release.yml` の「一式から、会社と同じ手順で組んでみる」）── 配ってから
「あちらで組んだら落ちた」がいちばん高くつく。

---

## Electron ごと持ち込む道（`offline-kit`）

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
| `rcedit-x64.exe` | exe の絵と名前を焼く道具（`--rcedit` で入れたときだけ） |
| `組み方.txt` | 下と同じことが、実際の道つきで書いてある |

**コミットしていない変更は入らない。** `git archive` が見るのは HEAD なので、
直したばかりのものを持っていきたいなら先にコミットする ── まとめるときに
「入るのは HEAD の中身です」と言うので、そこで気づける。

---

### 会社でやること（`offline-kit` を持ち込んだとき）

要るのは **Node.js だけ**（`node --version` が通ること）。網には一度も出ない。

```
cd C:\amber-kit\amber

node scripts\pack.js --out dist --platform win32 ^
  --electron ..\electron-v33.4.11-win32-x64 ^
  --engine ..\amber-server-win-x64.exe ^
  --rcedit ..\rcedit-x64.exe ^
  --zip
```

`dist\amber-win-x64\amber.exe` ができる。`--zip` を付けると配れる一枚も出る。

**会社向けの一枚を組むなら** `--edition office` を足す ── 行き先が
`dist\amber-win-x64-office\` に変わる（名前を分けないと、どちらを配って
いるのか手元で見分けられない）。

**cian と同じ綴りで打てる。** `--platform win32` は `--win` と、`--engine` は
`--server` と同じものを指す ── 手が憶えているほうが正しい。

---

## 踏みやすいところ

- **`gui/vendor/` がありません** ── 持ち込んだ一式の `amber\gui\vendor` が
  欠けている。あれは `npm install` が置いたものを写したもので、**git には
  入っていない**ので、網の外では作れない。持ち込み直す。
  写す前に言うようにしてある（Electron を二百メガ写したあとで落ちない）
- **exe の絵が Electron のまま** ── `--rcedit` を渡していないか、Windows
  以外で組んでいる。絵と名前は PE のリソースなので、**Windows の上でしか
  焼けない**
- **zip の日本語の名前が化ける** ── `Compress-Archive` で組み直していないか。
  `scripts/zip.js` は UTF-8 の印（汎用ビット 11）を必ず立てる。あちらは
  立てない
- **エンジンだけ古い** ── `amber-server-win-x64.exe` は Release の資材なので、
  リポジトリを新しくしても勝手には変わらない。版を上げたら落とし直す

---

## なぜ electron-builder を使わないか

あれは組むあいだに取りに行く。**網の外では走らないし、網の中でも途中で
落ちると半端な生成物が残り、それが正常に見える**（crmaine が whl と vsce で
二度やった）。`scripts/pack.js` は写すだけで作り、**出口で必ず数えて**、
欠けていれば失敗させる。
