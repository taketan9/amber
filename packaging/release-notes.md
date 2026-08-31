## どれを落とせばいい？

| 使いたいもの | 落とすもの |
|---|---|
| **Windows で窓版（Electron）** | **`cian-gui-win-x64.zip`** — Electron 本体だけ別途必要 |
| **Windows で端末版** | **`cian-windows-x64.zip`** — 展開して `cian.exe` か `cian-tui.exe` |
| **Mac** | **`cian-macos.zip`** — 展開して `cian.app` |
| **Linux** | **`cian-linux-x64.tar.gz`** |

迷ったら、Windows なら `cian-windows-x64.zip`、Mac なら `cian-macos.zip`。

<details>
<summary>残りの3つ（普段は要りません）</summary>

| | |
|---|---|
| `cian-server-win-x64.exe` | エンジンだけ。窓版のエンジンを差し替えるとき用（9MB） |
| `cian-source-offline.zip` | ネットに繋がらない機械でソースからビルドするとき用（182MB）。依存を全部同梱 |
| `SHA256SUMS` | 壊れずに届いたかの確認用。`sha256sum -c SHA256SUMS` / `Get-FileHash` |

</details>

### Windows で落としたら、まずブロックを外す

zip を右クリック →「プロパティ」→ 下に「セキュリティ: 他のコンピューターから
取得したものです」があれば **「許可する」にチェック** → OK。**展開する前に。**

外さないと展開後の全ファイルに印が残り、`.exe` が黙って起動しません
（窓は出るのに中身が空、という形で出ます）。

窓版の詳しい手順は zip の中の `GUI.txt` にあります。

---
