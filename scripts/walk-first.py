#!/usr/bin/env python3
"""総ざらいを、初めての人の画面に邪魔されずに始める（依頼 654・656）。

ようこそ画面（`#hello`）と はじめの案内（`#tour`）は、どちらも画面ぜんぶを
覆う。案内は「次へ」を押すまで進めない作りなので、憶えさせずに始めると
**282 段が一段目から通らない**。

`walk.sh` が本物の設定を写したあとに呼ぶ。終わりの戻しが元へ返す。
"""
import io
import json
import os

at = os.environ['AMBER_STATE']
try:
    now = json.load(io.open(at, encoding='utf-8'))
except Exception:
    now = {}
now['greeted'] = True
now['toured'] = True
io.open(at, 'w', encoding='utf-8').write(json.dumps(now, ensure_ascii=False, indent=2))
