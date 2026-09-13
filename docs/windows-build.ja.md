# ネットに出られない Windows で組む

会社の端末は網の外にある。**そこで `npm install` も `cargo build` もしない** ──
必要なものは家でまとめて、持ち込んで、写すだけで組む。

---

## 家でやること（一度）

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

## 会社でやること

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
