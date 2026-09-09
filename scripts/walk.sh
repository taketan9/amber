#!/bin/zsh
# 窓の総ざらいを、**安全な場所で**一回やる。
#
#     scripts/walk.sh
#
# やること: 試す場所を作る → 設定を退避 → 窓を出す → `walk.mjs` を走らせる
#          → 窓を閉じる → 設定を戻す → 試す場所を消す
#
# **本人のノートには触らない。** 触らないための仕掛けが二つ要る:
#
#   一。ノートの置き場所 ── `$HOME` を作り替えて渡す。窓は `documents` を
#       そこから引くので、ノートは試す場所の中にできる。
#
#   二。設定ファイル ── **macOS の Electron は `appData` に `$HOME` を
#       見ない**。隔離した `$HOME` を渡しても、設定だけは本物のほうに
#       落ちる（2026-09-09 に実際に書き込んでしまった ── `open` と `tabs`
#       が消えるフォルダを指し、次に開いたとき「開けません」になるところ
#       だった）。だから**始める前に写しを取り、終わったら必ず戻す**。
set -e
here="${0:A:h}"
root="${here:h}"
work="${TMPDIR:-/tmp}/amber-walk"
mine="$HOME/Library/Application Support/amber/amber.json"
port="${PORT:-9333}"

quit() {
  pkill -f 'Electron.*remote-debugging-port='"$port" 2>/dev/null || true
  pkill -f "http.server $siteport" 2>/dev/null || true
  sleep 1
  # **設定を戻すのは、何があっても。** ここを飛ばすと本人の窓が変わる。
  if [ -f "$work/amber.json.mine" ]; then
    mkdir -p "$(dirname "$mine")"
    cp "$work/amber.json.mine" "$mine"
    echo "設定を戻しました"
  elif [ -f "$work/なかった" ]; then
    rm -f "$mine"
    echo "設定を消しました（もともと無かったので）"
  fi
}
trap quit EXIT INT TERM

rm -rf "$work"
mkdir -p "$work/home/Documents/amber" "$work/site"
notes="$work/home/Documents/amber"
mkdir -p "$notes/attachments" "$notes/仕事" "$notes/テンプレート" "$notes/家族"

# 設定の写し（無ければ「無かった」と憶えておく）
if [ -f "$mine" ]; then cp "$mine" "$work/amber.json.mine"; else touch "$work/なかった"; fi

# ── 試すノート。**書けるものを一通り**入れておく ──
cp "$root/packaging/welcome/attachments/amber.png" "$notes/attachments/" 2>/dev/null || true
cat > "$notes/よくばり.md" <<'MD'
---
title: よくばり
created: 2026-09-01
tags: [仕事, 急ぎ]
---

# よくばり

段落です。**太字**と*斜体*と~~取り消し~~と`コード`と[リンク](https://example.com/a)。

## 一覧

- ひとつ
- ふたつ
  - 入れ子

1. 番号
2. 番号

- [ ] やること
- [x] やった

## 表

| 朝 | 夕 |
| :--- | ---: |
| 掃除 | 片づけ |

> 引用です。

> [!NOTE]
> 注記です。

```rust
fn main() { println!("{}", 1); }
```

![amber の印](attachments/amber.png)

```mermaid
graph TD
  A --> B
```

---

　全角の字下げ。
MD
printf -- '---\ntitle: 買い物\ncreated: 2026-09-08\ntags: [暮らし]\n---\n\n# 買い物\n\n- 牛乳\n- パン\n' > "$notes/買い物.md"
printf -- '---\ntitle: からっぽ\ncreated: 2026-09-06\n---\n\n' > "$notes/からっぽ.md"
printf -- '---\ntitle: 段取り\ncreated: 2026-09-07\ntags: [仕事]\n---\n\n# 段取り\n\n## 朝\n\n本文。\n' > "$notes/仕事/段取り.md"
printf -- '---\ntitle: 週報のひな型\ncreated: 2020-01-01\n---\n\n# 週報\n\n## やったこと\n' > "$notes/テンプレート/週報のひな型.md"
printf -- '---\ntitle: 買い物リスト\ncreated: 2026-09-05\n---\n\n# 買い物リスト\n\n- 家族のぶん\n' > "$notes/家族/買い物リスト.md"
# **競合の控え。** クラウドが同時更新を見つけたときに置いていく形 ──
# これがある一覧を一度も開いていなかった（走査で気づいた）。
printf -- '---\ntitle: 買い物\ncreated: 2026-09-08\n---\n\n# 買い物\n\n- 牛乳を二本\n' \
  > "$notes/買い物 (Taketan の競合コピー 2026-09-08).md"

# **往復だけに使うノート。ほかの試しは触らない。**
# 前は「よくばり」で往復を見ていたが、そこは記号の帯を片端から押す先でも
# あって、往復にたどり着くころには壊れやすい行が残っていなかった ──
# 前後の空白を落とす壊し方を入れても鳴らなかった（2026-09-09）。
cat > "$notes/往復.md" <<'MD'
---
title: 往復
created: 2026-09-02
---

# 往復

　全角の字下げから始まる段落。
行末で折り返した  
つづき。

* 点は星のまま
* 二つめ

1. 番号は
1. 書いた通り
1. 三つめ

> 引用。
> 二行目。

| 朝 | 夕 |
| :--- | ---: |
| 掃除 | 片づけ |

```rust
fn main() {}
```

![amber の印](attachments/amber.png)
MD

# **よそから来た形のノート。** Windows で作られたもの・古い日本語のもの。
# core は読んだときの文字コード・BOM・改行のまま書き戻すが、**窓を通した
# ときもそうか**は誰も見ていなかった。
python3 "$here/fixtures.py" "$notes"

# ── 取り込む先のページ（外へは出ない） ──
siteport="${SITEPORT:-8731}"
cat > "$work/site/index.html" <<'HTML'
<!doctype html><html><head><meta charset="utf-8">
<title>段取りの決め方 | 試しのページ</title></head><body>
<nav>案内</nav><article><h1>段取りの決め方</h1>
<p>まず<b>朝</b>に<a href="/次">次</a>を決めます。</p>
<ul><li>一つめ</li></ul><pre><code class="language-js">const a = 1;</code></pre>
</article><footer>足</footer></body></html>
HTML
# よその予定表（依頼 456）── 一度きり・終日でまたぐもの・毎週の三つ。
printf 'BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:%s\r\nBEGIN:VEVENT\r\nDTSTART;TZID=Asia/Tokyo:20260904T183000\r\nSUMMARY:%s\r\nLOCATION:%s\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20260921\r\nDTEND;VALUE=DATE:20260924\r\nSUMMARY:%s\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;TZID=Asia/Tokyo:20260907T200000\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\nSUMMARY:%s\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n' \
  '家の予定' '歯医者' '駅前' '旅行' 'ごみ出し' > "$work/site/away.ics"
(cd "$work/site" && python3 -m http.server "$siteport" >/dev/null 2>&1 &)

# **エンジンを作り直してから出す。** 窓は起動時の実行ファイルを掴んだまま
# なので、直したはずの判断が効かないまま「通りました」になる（実際になった）。
(cd "$root" && cargo build -q -p amber-server) || {
  echo "エンジンが作れません"; exit 2
}

# ── 窓を出す ──
(cd "$root/gui" && HOME="$work/home" npx electron --remote-debugging-port="$port" . \
  >"$work/win.log" 2>&1 &)
for i in $(seq 1 40); do
  curl -s -m 1 "http://127.0.0.1:$port/json" >/dev/null 2>&1 && break
  sleep 0.5
done

SITE="http://127.0.0.1:$siteport/" PORT="$port" NOTES="$notes" node "$here/walk.mjs"
