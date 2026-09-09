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
