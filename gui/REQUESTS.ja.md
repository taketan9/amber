# 依頼の台帳

Taketan が名指しで頼んだこと。**これは装飾ではなく契約です。**

## なぜこれがあるか

2026-08-30、「きちんとアクティブパネルのみ枠線を太くしてほしい」と言われ、
`0eb71b5` で直しました。翌 8/31、見た目を作り直す `951bcd1` で**その行を自分で
削除しました** ── 「枠はカーソルが既に答えている問いへの一番うるさい答え方だ」
というコメントまで添えて。8/31 の夕方、同じ指摘をもう一度受けました。

見落としではありません。**上書きです。**設計パスに入った瞬間、その CSS が
「自分で決め直してよい既存コード」に見えた。そう見えたのは、コードのどこにも
**その行がどこから来たのか書いていなかった**からです。私が自分の判断で書いた行と
区別が付きませんでした。

同じ commit は枠を4つ消していて、うち3つ（シェルにキーがある・同期中・記録中）は
別の signal に載せ替えてありました。**置き換えずに消したのは、頼まれた1つだけ**です。

だからこの表があります。行ごとに「これが今も真であること」を機械で確かめます。
消すなら検査が落ちる。落ちたら、直す前に**訊く**。

## 使い方

```bash
python3 scripts/requests.py          # 全部通るか
python3 scripts/requests.py --list   # 何を見ているか
```

`検査` 欄は `ファイル ~ 正規表現`（マッチすること）か `ファイル !~ 正規表現`
（どこにも出てこないこと）です。

否定は専用の書き方にしてあります。最初は否定先読み `^(?!.*やめる).*$` で書いて
いて、これは「その語を含まない行が1行でもあれば成功」なので**常に通ります**。
3行が何も検査せずに「守られています」と言っていました。足したら必ず壊して、
鳴るのを見てから戻すこと。

## 表

| # | 日付 | 依頼（原文） | 満たすもの | 検査 |
|---|---|---|---|---|
| 1 | 2026-08-30 | きちんとアクティブパネルのみ枠線を太くしてほしい | キーのあるペインに枠。`data-focus` で条件付け。**枠は中身より上に描く**（inset box-shadow だと Windows の実スクロールバーが右辺を覆って消える）。色は `--mode-accent` ＝ 既定はアクセント、`/` `//` は緑・`:` は紫・`v` は橙・シェルは金（端末版 `focus_badge_color`。2026-09-01「対象のファイラパネルを緑っぽい枠にしてほしい」） | `gui/index.html ~ (?s)\A(?=.*#work \{ --mode-accent: var\(--accent\); \})(?=.*data-focus="files"\]\s*\.pane\.active::after\s*\{[^}]*border: 2px solid var\(--mode-accent\))` |
| 2 | 2026-08-31 | フォーカス以外はちょっと控えめの配色に | 現在でないペインの一覧・パンくず・見出しを減光 | `gui/index.html ~ \.pane:not\(\.active\)[^{]*\{[^}]*opacity` |
| 3 | 2026-08-30 | 「やめる（Esc）」は日本語が変。キャンセル（Esc）がよい | 確認シートのボタンは「キャンセル」。足のヒントは端末版と同じ「取消」 | `gui/index.html ~ キャンセル` |
| 4 | 2026-08-30 | （同上）足のヒントに「やめる」を残さない | 端末版は取消を43回使い、やめるは1回 | `gui/renderer.js !~ Esc\s*やめる` |
| 5 | 2026-08-30 | 「/」「//」「:」で全部下部にコマンド入力できるように | 3つとも下端の1行に入力する。全画面のモーダルにしない | `gui/renderer.js ~ const PROMPT_SIGN = \{` |
| 6 | 2026-08-30 | 検索系だと緑枠、コマンド系だと紫っぽい枠に | 端末版 `focus_badge_color` の値をそのまま | `gui/index.html ~ --m-command:\s*rgb\(200,\s*100,\s*200\)` |
| 7 | 2026-08-30 | リモートペインの赤枠 | 端末版と同じ carmine | `gui/index.html ~ --m-remote:\s*rgb\(214,\s*45,\s*70\)` |
| 8 | 2026-08-30 | フォントは同梱する | HackGen Console NF を `vendor/fonts/cian.ttf` から窓が自分で読む | `gui/index.html ~ @font-face` |
| 9 | 2026-08-30 | 一覧は端末版の列構成に揃える | 4桁年の日時列 | `gui/renderer.js ~ \$\{d\.getFullYear\(\)\}-\$\{md\}` |
| 10 | 2026-08-30 | shell1 を名称変更できるようにしたい | `:shellname` / `:tabname`。端末版にも入れた | `gui/renderer.js ~ async function cmdShellName` |
| 11 | 2026-08-30 | シェルの初期表示を25%に | 起動時の分割比 | `gui/renderer.js ~ const layout = \{ main: 75` |
| 12 | 2026-08-30 | ゴーストの入力案内は不要なので消して欲しい | プロンプト行に placeholder を置かない | `gui/index.html !~ id="f-input"[^>]*placeholder` |
| 13 | 2026-08-31 | .md でプレビューと普通のテキストを行き来するショートカット | `Ctrl+E`。Monaco が同じ鍵を持つので捕捉フェーズで先に取る | `gui/renderer.js ~ async function togglePreview2` |
| 14 | 2026-08-31 | （vim 流で）Ctrl+C/V/X が効かない | 端末版 `viewer_vim_key` は Ctrl/Alt 付きを文法に渡さない。同じにする | `gui/renderer.js ~ cxvCXV` |
| 15 | 2026-08-31 | ブックマークのフォルダが最初から開いて見える | 端末版 `sc_level` と同じく1階層ずつ | `gui/renderer.js ~ let scPath` |
| 16 | 2026-08-31 | F11 でウィンドウが最大化しない | F11 で全画面。他のキーと同じ場所で読む | `gui/renderer.js ~ async function cmdFullscreen` |
| 17 | 2026-08-31 | :version でビルドの時間は見れないよ | ビルド日時と commit と書体を出す | `crates/cian-server/build.rs ~ CIAN_BUILT_AT` |
| 18 | 2026-08-30 | 会社ではGitコマンドは使えない前提で | Electron の場所は `electron.txt` に1行。zip に入れない（次の版で消えるので） | `gui/run.bat ~ for /f "usebackq delims=" %%L in \("%HERE%\\electron\.txt"\)` |
| 19 | 2026-08-31 | run.bat の文字化けエラー | `chcp` を実行しない。純 ASCII | `gui/run.bat !~ ^\s*chcp` |
| 20 | 2026-08-31 | cian-tui / cian-gui で表記の揺れ・動きの揺れがあれば揃えたい | 端末版の語を窓版のメニュー・スイッチ・並び替えに写し、**機械で確かめる**（`scripts/parity.py`）。CI にも入れた | `.github/workflows/ci.yml ~ python3 scripts/parity\.py` |
| 21 | 2026-08-31 | （同上）機能・挙動の違いをなくしていきたい | ビューアの文法の既定を端末版と同じ vim に | `gui/renderer.js ~ ^let style = 1;$` |
| 22 | 2026-09-02 | コピー・リネーム・移動のあとの Meta+z も戻したい | `Ctrl+z` / `⌘Z` を一覧の取り消しに（`Ctrl+Shift+z` でやり直し）。**コピーも取り消せるようにした** ── そのコピーが新しく作ったものだけをゴミ箱へ | `gui/renderer.js ~ if \(e\.shiftKey\) redo\(\); else undo\(\);` |
| 23 | 2026-09-02 | コピーの取り消しは、そのコピーが作ったものだけ | 先に存在した名前は上書きでも飛ばしでも対象外。判断は1か所（`copy_creates`）で、両前端が同じものを使う | `crates/cian-core/src/ops.rs ~ pub fn copy_creates` |
| 24 | 2026-09-02 | jj 系は端末版にも入れて欲しい | `jj` / `ｊｊ` / `っｊ` で挿入モードを抜ける。`ZZ` / `ZQ` も | `crates/cian-tui/src/viewer.rs ~ pub\(crate\) const JJ_ESCAPES` |
| 25 | 2026-09-02 | アイコンを exe に焼き込むのをやってほしい | `winresource` で `cian-tui.exe` / `cian.exe` / `cian-server.exe` に `cian.ico` を。**無ければビルドを止める**（黙って軽い zip を出した font の再発を防ぐ） | `crates/cian-bin/build.rs ~ res\.set_icon\(icon\)` |
| 26 | 2026-09-02 | AIの会話枠はいらない。コマンド提案・ディレクトリ検索・ゴミ検索の品質と精度を1段2段あげたい | モデルに渡す事実を増やした ── **種類・サイズ（ディレクトリは配下の合計）・最終更新からの日数・パス**の4列。ゴミ検索は木を4階層下まで見る（前は1階層） | `crates/cian-core/src/survey.rs ~ pub fn survey` |
| 27 | 2026-09-02 | （同上）コマンド提案の精度 | **OS ではなくシェル名**を渡す（Windows は PowerShell）。いま開いている一覧とマークも。1行で書けないときは `# ` で断らせる | `crates/cian-server/src/main.rs ~ Shell: \{shell\}\\nPlatform` |
| 28 | 2026-09-02 | （同上）見た範囲を偽らない | 打ち切ったら**モデルにも人にも**言う。「N件入らなかった」ではなく「何階層目までは全部見た」 | `crates/cian-core/src/survey.rs ~ pub fn whole_to` |

## 増やすとき

依頼を受けて何かを入れたら、**その場でここに1行足す**。あとでまとめてやると、
まとめる前に自分で消します（それが起きたのがこの表の1行目です）。

検査は「その依頼が満たされていること」を見るもので、実装の形を凍結するもの
ではありません。**作り直して構いません。ただし検査を通したまま**、あるいは
検査ごと変えるなら**先に訊いてから**。
