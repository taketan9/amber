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

# **使われていない画像**（依頼 449）。一枚は本文から指し、一枚は
# どこからも指さない ── 数える側が、指しているほうを巻き込まないか。
import zlib as _z, struct as _s
def _png(path, w, h, rgb):
    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))
    def chunk(t, d):
        c = t + d
        return _s.pack(">I", len(d)) + c + _s.pack(">I", _z.crc32(c) & 0xffffffff)
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", _s.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", _z.compress(raw))
        + chunk(b"IEND", b""))

pics = at / "attachments"
pics.mkdir(exist_ok=True)
_png(pics / "絵のノート-1788000001.png", 60, 40, (240, 165, 43))
_png(pics / "絵のノート-1788000002.png", 40, 40, (90, 140, 200))
(at / "絵のノート.md").write_text(
    "---\ntitle: 絵のノート\ncreated: 2026-09-09\n---\n\n# 絵のノート\n\n"
    "![](attachments/絵のノート-1788000001.png)\n", encoding="utf-8")
print("使われている画像と、使われていない画像を置きました")

# **カレンダーに乗るもの**（依頼 453）。一度きりと、繰り返しと、
# その日に書いたノート ── 三つとも月の表に出るか。
(at / "面談.md").write_text(
    "---\ntitle: 面談\ncreated: 2026-09-09\nremind: 2026-09-09 14:00\n---\n\n"
    "# 面談\n\n会議室 A。\n", encoding="utf-8")
(at / "週報.md").write_text(
    "---\ntitle: 週報\ncreated: 2026-08-01\nrepeat: weekly wed 09:00\n---\n\n"
    "# 週報\n", encoding="utf-8")
print("カレンダーに乗る二本を置きました")
