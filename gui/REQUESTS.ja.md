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

**識別子には終端を書くこと**（`\(` や `\b`）。`~ pub fn foo` は `pub fn foo2` に
そのまま当たるので、名前を変える変異をすり抜けます。2026-09-02 に3回やりました
── 検査を足したら壊して鳴らす、をやっていなければ3回とも気づいていません。

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
| 27 | 2026-09-02 | （同上）コマンド提案の精度 | **OS ではなく「そのシェルが今どこにいるか」**を渡す ── タイトルの `user@host`／画面に残る `ssh` 行／プロンプトの形、そして `init.lua` の `notes` から**接続先のOS**まで。端末版が持っていたものを窓版にも。一覧とマークも渡し、1行で書けないときは `# ` で断らせる | `crates/cian-server/src/main.rs ~ cian_core::shellwhere::describe\(` |
| 28 | 2026-09-02 | （同上）見た範囲を偽らない | 打ち切ったら**モデルにも人にも**言う。「N件入らなかった」ではなく「何階層目までは全部見た」 | `crates/cian-core/src/survey.rs ~ pub fn whole_to` |
| 29 | 2026-09-02 | 端末版にあって窓版に無いポップアップを埋めたい | 開いている zip へのコピー（端末版 `ConfirmZipAdd`）。**それまでは zip の隣に落として「コピーしました」と言っていた** ── アーカイブ表示は作り物で、ペインは入る前の `cwd` を覚えたままだった | `crates/cian-server/src/main.rs ~ "zipadd" =>` |
| 30 | 2026-09-02 | リモートの深さを実装してほしい | ローカル↔サーバの `c`/`m`（**それまでは黙って手元にコピーしていた**）・サーバ内の移動・chmod・サーバ間の中継。同一サーバの移動は rename で済ませる | `crates/cian-server/src/jobs.rs ~ pub fn start_remote\(` |
| 31 | 2026-09-02 | （同上）実際に通したい | **127.0.0.1 に自分の sshd を立てて通す**（`scripts/remote.py`）。管理者権限は要らない。これが無かったのでリモートは一度も検証されていなかった | `scripts/remote.py ~ def free_port\(` |
| 32 | 2026-09-02 | （同上）鍵で入れるようにしたい | `cian.ssh{ users = { { key = "~/.ssh/id_ed25519" } } }`。**設定例は前から鍵認証を勧めていたのに、機能が無かった** | `crates/cian-scp/src/lib.rs ~ \.authenticate_publickey\(` |
| 33 | 2026-09-02 | （同上）ディレクトリごと送れるようにしたい | `cian_scp::plan_upload` / `plan_download` が木を歩き、ジョブが親から順に mkdir する。**端末版にも無かった**（両方ファイルだけ送っていた）。リンクは辿らない | `crates/cian-scp/src/lib.rs ~ pub fn plan_upload\(` |
| 34 | 2026-09-02 | Shift+f / Ctrl+f のポップアップの表題が `:find` `:grep` で何をするか分かりづらい | 表題を**何をするか**の文（`about`）に。欄の中に何を打つか（`arg`）、コマンド名は下に小さく残す ── 名前は覚える価値があるので消しはしない | `gui/renderer.js ~ a = await askFor\(cmd\.about,` |
| 35 | 2026-09-02 | 「z」の宛先入力窓の横幅が小さく、入力した文字列が見切れる | パスを訊く窓は広く（`min(60rem, 84vw)`）。380px は**ファイル名の幅**であってパスの幅ではなかった | `gui/index.html ~ #ask \.sheet\.wide \{ min-width` |
| 36 | 2026-09-02 | ビューアに対ディスク差分ガター | 編集中の行のうち**ディスクと違う行**を行番号の脇に出す。判定はエンジンの `cian_core::diff`（窓で二つ目の差分器を書かない） | `crates/cian-server/src/main.rs ~ "diskdiff" =>` |
| 37 | 2026-09-02 | AVD の Program Files に置いて共通デスクトップの run.bat で全員に使わせたい。初回の人には僕の init.lua や ssh 設定を配りたい | `run.bat` が `default-config\` から各自の `~/.config/cian` へ配る。**初回だけ配る**（以後その人のもの。ssh.lua も含めて上書きしない ── 使っている最中に設定を書き換えられる道具は信用できない）。**exe の隣に置く配り方は罠** ── 書き込み先まで Program Files になり、全員のしおりが保存できなくなる | `gui/run.bat ~ default-config` |
| 38 | 2026-09-02 | tar への書き戻し | `tar_modify` ── zip と同じ4つの操作（削除・改名・追加）。tar は差分編集できないので**丸ごと書き直す**が、**新しいものが完成するまで元を消さない**（誰かの唯一のコピーであることがある） | `crates/cian-core/src/archive.rs ~ pub fn tar_modify\(` |
| 39 | 2026-09-02 | ディレクトリ転送に確認シート | サーバへフォルダを送るとき、**中に何ファイル・何バイトあるか**を確認前に出す。数は転送が使うのと**同じ計画器**（`plan_upload`）から取る ── シートと実行が別々に数えると、約束と結果がずれる | `crates/cian-server/src/main.rs ~ "transferplan" =>` |
| 40 | 2026-09-02 | スクリプトマクロを窓版でも | `macro_script::run` をエンジンから呼ぶ。**それまでは「まだ動かせません」と断っていた** ── `macro.lua` は両方の種類を1ファイルに持つので、半分だけ動く状態だった | `crates/cian-server/src/main.rs ~ macro_script::run\(` |
| 41 | 2026-09-02 | （同上）`macro/` ディレクトリも読む | 探索の規則（`macro.lua` と `macro/*.lua`、`.en.lua` は飛ばす）を `cian_lua::macros::load_all` に。**窓版は前者しか読まず、同梱の例のように1ファイル1マクロにしている人にはランチャーが空だった** | `crates/cian-lua/src/macros.rs ~ pub fn load_all\(` |
| 42 | 2026-09-02 | ダークテーマのときもタイトルバーが明るいまま | タイトルバーは OS が描くので CSS では届かない。`nativeTheme.themeSource` を**実際に描かれている `--bg` の輝度**から決めて渡す（配色の名前ではなく）。起動時の地の色もエンジンに訊く ── `main.js` は18ある配色のうち3つしか表を持っていなかった | `gui/renderer.js ~ function tellFrame\(` |

## 増やすとき

依頼を受けて何かを入れたら、**その場でここに1行足す**。あとでまとめてやると、
まとめる前に自分で消します（それが起きたのがこの表の1行目です）。

検査は「その依頼が満たされていること」を見るもので、実装の形を凍結するもの
ではありません。**作り直して構いません。ただし検査を通したまま**、あるいは
検査ごと変えるなら**先に訊いてから**。
