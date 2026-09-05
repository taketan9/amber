## どれを落とせばいい？

| 使いたいもの | 落とすもの |
|---|---|
| **Windows で窓版（Electron）** | **`cian-gui-win-x64.zip`** — Electron 本体だけ別途必要 |
| **Mac で窓版（Electron）** | **`cian-gui-macos.zip`** — 同上。エンジンは Intel と Apple Silicon の両方入り |
| エンジンだけ差し替えたい | `cian-server-win-x64.exe`（9MB）／`cian-server-macos.bin` |
| 壊れずに届いたかの確認 | `SHA256SUMS` — `sha256sum -c SHA256SUMS` / `Get-FileHash` |

**いまは窓版（Windows と Mac）を出しています。** 端末版・Mac の `.app`・
Linux・オフラインビルド用のソース一式は、必要になったら Actions から
`release` を **everything = true** で手動実行すれば揃います。

### Windows で落としたら、まずブロックを外す

zip を右クリック →「プロパティ」→ 下に「セキュリティ: 他のコンピューターから
取得したものです」があれば **「許可する」にチェック** → OK。**展開する前に。**

外さないと展開後の全ファイルに印が残り、`.exe` が黙って起動しません
（窓は出るのに中身が空、という形で出ます）。

窓版の詳しい手順は zip の中の `GUI.txt` にあります。

### Mac で落としたら

macOS は落としたものに「隔離」の印を付けます。cian は公証していないので、
展開したフォルダで一度だけ:

```
xattr -dr com.apple.quarantine .
```

詳しい手順は zip の中の `GUI.ja.txt` にあります。

---
