#!/usr/bin/env python3
"""総ざらいの試し場に、**よそから来た形のノート**を置く。

core は読んだときの文字コード・BOM・改行のまま書き戻すが、**窓と電話を
通したときもそうか**は誰も見ていなかった（依頼 429）。Windows で作られた
ノート（CRLF）、BOM 付き、古い日本語（Shift_JIS）を置いて、開いて打って
保存したあとも同じ形で残るかを見る。
"""
import pathlib
import sys

at = pathlib.Path(sys.argv[1])
body = (
    "---\ntitle: {t}\ncreated: 2026-09-03\n---\n\n"
    "# {t}\n\n本文です。\n\n- 一つめ\n- 二つめ\n"
)

# Windows で作られたもの（CRLF）
(at / "改行CRLF.md").write_bytes(
    body.format(t="改行CRLF").replace("\n", "\r\n").encode("utf-8"))

# BOM 付き（古いメモ帳や Excel が置いていく）
(at / "BOM付き.md").write_bytes(
    b"\xef\xbb\xbf" + body.format(t="BOM付き").encode("utf-8"))

# 古い日本語（Shift_JIS・CRLF）
(at / "日本語SJIS.md").write_bytes(
    body.format(t="日本語SJIS").replace("\n", "\r\n").encode("shift_jis"))

print("よそから来た形のノートを三つ置きました")

# **大きいノート。** 一万行 ── 面の組み直しと保存が保つか（依頼 431）。
# 「速いこと」は一度測ったきりで、遅くなったことに気づく仕掛けが無かった。
rows = []
for i in range(1, 2001):
    rows.append("## 見出し %d" % i)
    rows.append("")
    rows.append("段落 %d です。**太字**と[リンク](https://example.com/%d)。" % (i, i))
    rows.append("")
    rows.append("- 項目 %d" % i)
    rows.append("")
(at / "大きいノート.md").write_text(
    "---\ntitle: 大きいノート\ncreated: 2026-09-04\n---\n\n# 大きいノート\n\n"
    + "\n".join(rows) + "\n", encoding="utf-8")
print("一万行のノートを置きました")

# **混ぜるための一本。** 同じノートを二か所から書き換えたとき、
# どちらも消えないか（依頼 433）。走査が横から書き換える先。
(at / "混ぜる.md").write_text(
    "---\ntitle: 混ぜる\ncreated: 2026-09-05\n---\n\n# 混ぜる\n\n"
    "はじめの行。\n\nおわりの行。\n", encoding="utf-8")
print("混ぜるための一本を置きました")
