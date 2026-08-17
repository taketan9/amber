# cian GUI spike — 窓を持つ cian は日本語で成立するか

2026-08-16。`cian-gui`（窓を持つ cian）に踏み出す前の、一点突破の検証。
cian 本体には触っていない。**自前ワークスペース**なので cian の `Cargo.lock`
にも影響しない。

構成は winit 0.30 + ratatui-wgpu 0.5 + ratatui 0.30。ratatui-wgpu が
`ratatui-core ^0.1` に依存していて、cian の ratatui 0.30 が使っているのが
`ratatui-core 0.1.2` なので、**バージョンの壁は無い**。

## 動かす

```
cargo run --release                                  # 既定のフォント（下記）
cargo run --release -- /path/to/Font.ttf [別のフォント...]
SPIKE_SIZE=40 SPIKE_PAGE=panes cargo run --release   # 初期サイズとページ
SPIKE_PIN=1 cargo run --release                      # 窓を左上に固定して最前面へ
```

`+`/`-` フォント拡縮 · `t` ページ切替 · `Esc` 終了。押したキーは修飾キー込みで
画面に出る（`t` で「受け取ったキー」ページへ）。

引数を省いたときは `~/Downloads/HackGenConsoleNF-Regular.ttf` があればそれ1本、
無ければ `~/Library/Fonts/HackNerdFontMono-Regular.ttf` と
`/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc` の2本を使う。1本と2本鎖の
違いがそのまま「なぜ同梱1本なのか」の答えになっている。

フォントは HackGen v2.10.0 の `HackGen_NF_v2.10.0.zip`
（<https://github.com/yuru7/HackGen/releases>）から取った Console NF の Regular。

## 分かったこと

### 通った

* **全角の幅が正しい。** `wgpu_backend.rs:586` が `cell.symbol().width() *
  min_width_px` で advance を出しているとおり、全角は2セル進む。`shot-panes.png`
  の右揃えサイズ列が `日本語ファイル名.rs` でも `報告書_2026年度.pdf` でも
  ASCII 名と揃うのが証拠 — 測った幅と描いた幅が一致している。
* **罫線は繋がる。** `shot-ruler-hackgen.png` の細線・太線の格子を見ること。
  （最初 `─│┌┐└┘├┤┬┴┼` を一列に並べて「崩れている」と読んだが、これは繋がらない
  部品を並べただけで、フォントもレンダラも悪くなかった。）
* **Nerd Font アイコンは1セルに収まる。** `[ ]` で挟んで確認済み、隣を食わない。
* 太字・斜体・下線・反転・truecolor すべて可。反転（カーソル行）の幅が文字幅と一致。
* **実行時のフォント拡縮**が `WgpuBackend::update_fonts` で効く。cian が今
  init.lua に端末ごとのコマンドを書かせている `Ctrl +/-` が、自前で成立する。
* **`.ttc` が読める**（ttf-parser が index 0 を取る）。ヒラギノがそのまま使える。
* 性能：84×30 セルを**毎フレーム全消し＋全描画**して 60fps（vsync 上限）、
  最悪フレーム 17.8ms。cian は変化時しか描かないので余裕がある。
* **IME が通る。** `window.set_ime_allowed(true)` を呼ぶこと — 既定は無効で、
  無効な窓は「IME に対応していないプラットフォーム」と見分けがつかない。
  呼べば `Ime::Preedit` が流れ、中身は「にほん」と**かな**で来る。ローマ字かな
  変換は macOS がやってくれるので、cian は未確定文字を下線で描くだけでよい。
  窓化で唯一「端末より難しくなる」と踏んでいた箇所が、いちばん楽だった。
* **Ctrl と Command を区別して受け取れる。** 端末には Cmd が届かないので、
  これは窓でしかできない。下の「おまけ」の入れ替え環境では、Control と刻印
  されたキーが Super として届く — 窓なら「Super を Ctrl として扱う」設定を
  cian 側で用意できる。端末では不可能。

### 引っかかった

* **プロポーショナルフォントは使えない。** ヒラギノ単体で走らせると全グリフが
  重なって壊滅する。セル幅は `'m'` の advance ひとつから決まるので、
  **等幅・ASCII 1 : 全角 2** のフォントが要る。
* **メトリクスの違う2本を混ぜると崩れる。** Hack Nerd Font Mono + ヒラギノでは
  ✔ ✖ が豆腐になった（cian は `✖` をエラーメッセージで実際に使っている）。
  HackGen Console NF 1本にしたら消えた。
* `▶` `◀` `✔` `✖` `⚠` が細い輪郭で描かれて薄い。`●` は濃く出る。フォント側の
  字形の問題で、崩れではない。cian は ▶ を18箇所、⚠ を14箇所使っているので、
  実装時に見え方を確認すること。

## Nerd Font のアイコンが右側で切れる理由（2026-08-16）

`advwidth.py` 相当でメトリクスを測った結果:

| グリフ | インク幅 | advance |
| --- | --- | --- |
| `m` | 0.487 em | 0.527 em |
| `あ` | 0.930 em | 1.055 em |
|  folder | **0.812 em** | 0.527 em |
|  github | **0.879 em** | 0.527 em |

HackGen Console NF の Nerd Font アイコンは、**インクが advance を最大 67% 食み
出している**（Nerd Fonts の非 Mono 版の性質）。`ratatui-wgpu` は
`wgpu_backend.rs:620` でグリフを `unicode幅 × セル幅` のタイルにラスタライズ
するので、食み出した分は垂直に切り落とされる。`.github` の猫が `(` に見えるのは
これ。

**Symbols Nerd Font Mono との2本鎖は効かない。** 単体で描くとアイコンは完璧
（インク 0.000..1.000 / advance 1.000）だが、セル幅は全フォントの `'m'` advance
の**最小値**で決まるので、HackGen の 0.527 em が採用され、Symbols の 1.0 em の
アイコンはそこに押し込められて逆に半分近く切れる。

残る手は2つ。**日本語 NF フォントに `font-patcher --mono` を当てて同梱する**か、
**ピクセル層でアイコンを画像として描く**か。日本語 Nerd Font に Mono 版
（`NFM`）を出しているものは、HackGen・UDEV Gothic・PlemolJP・Moralerspace・Cica
のいずれにも無い（2026-08-16 時点で全リリースを確認）。

## おまけ：長年の「Ctrl が届かない」の正体

このスパイクで判明した。**この Mac はシステム設定で Control と Command を
入れ替えている**（内蔵 1452-636-0 と外付け 1278-33-0 の両方。`defaults
-currentHost read -g` の `com.apple.keyboard.modifiermapping.*` に出ている）。
つまり物理 Ctrl キーは OS が Command として配送する。

実測（2026-08-16、このスパイクで）:

| 押したキー | 報告 |
| --- | --- |
| 「Control」と刻印されたキー | `Super` `[物理 SuperLeft]` |
| 右の「⌘」 | `Control` `[物理 ControlRight]` |
| 左の「⌘」 | 何も出ない（未解明） |

つまり `Ctrl+M` が `Super+"m"` になり、`Ctrl+H` が macOS の Cmd+H（アプリを
隠す）に食われて修飾キーしか残らなかった。iTerm2 で「Ctrl+F が find バーを
開く」のも Cmd+F だったということ。**本物の Ctrl は右の ⌘ キーにある。**

入れ替えは HID 層で起きるのでプログラムからは見えない。winit の
`physical_key` すら入れ替え後の `SuperLeft` を返す（＝「物理キー」は
キーボードの刻印ではなく winit の言葉づかい）。Karabiner-Elements も
入っているが無関係 — ルールは Finder 限定の2つと Option+L だけ。

窓を持ってもこの入れ替えは無くならない（OS の設定なので）。ただし窓なら
Super と Control を区別して受け取れるので、端末では不可能な「⌘ に割り当てる」
という逃げ道が新たに使える。

## 結論

**日本語 Nerd Font を1本同梱する。** フォールバック鎖という問題そのものが消え、
システムフォントに依存しなくなる（＝「exe を叩くだけ」が成立する）。

代償はサイズ。HackGen Console NF Regular は 12.3MB あり、素で埋めると
`cian-gui` は 30MB 前後になる。使う字だけにサブセットすれば数MBまで落とせる。
