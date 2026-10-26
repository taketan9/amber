#!/usr/bin/env python3
"""総ざらいを、初めての人の画面に邪魔されずに、**試し場の中だけで**始める
（依頼 654・656・667）。

ようこそ画面（`#hello`）と はじめの案内（`#tour`）は、どちらも画面ぜんぶを
覆う。案内は「次へ」を押すまで進めない作りなので、憶えさせずに始めると
**282 段が一段目から通らない**。

**保存ディレクトリも、ここで試し場に向ける**（依頼 667・2026-10-17）。

`walk.sh` は `$HOME` を作り替えるが、**保存ディレクトリは絶対パスで設定に
入っている**ので、`$HOME` を替えても効かない。本人が amber を実際に使い
はじめてからは、設定の `root` と `places` が本物のノート
（`~/Documents/cian`・`~/batchPro`）を指すようになっていて、**総ざらいが
本人のノートの上で走る状態になっていた。** 2026-10-17 に実際にそうなった
── 試験の画面が本人の `batchPro` のノートを開いていた。

**危ないのは同期そのものではなく、本物のノートを偽の同期先に向けること。**
本人の `Documents/cian` は同期先が drive になっていて、`walk.sh` が立てるのは
**空の偽 Drive**。そのまま同期が走れば「向こうでぜんぶ消えた」と読めてしまう。
保存ディレクトリを試し場に向け替えれば、そこは断たれる ── 同期先は `drive`
のまま残す。**切ると同期の段が十三も落ちる**（2026-10-17 に実際に落とした）。

開いていたノートとタブも落とす ── 本物のノートの道が残っていると、
最初に開くノートがそれになる。

`walk.sh` が本物の設定を写したあとに呼ぶ。終わりの戻しが元へ返す。
"""
import io
import json
import os

at = os.environ['AMBER_STATE']
# **道は正してから渡す。** `//` が混ざると、アプリは道を文字として見比べる
# ので、見張りの跳ね返りを自分のものだと見分けられなくなる。
notes = os.path.normpath(os.environ['AMBER_NOTES'])

try:
    now = json.load(io.open(at, encoding='utf-8'))
except Exception:
    now = {}

now['greeted'] = True
now['toured'] = True

# **試し場の一つだけにする。** 名前は画面に出るので、見てそれと分かるものに。
now['root'] = notes
now['places'] = [{'name': '試し場', 'dir': notes, 'sync': 'drive', 'at': ''}]

# 本物のノートの道を持ち越さない。
for k in ('open', 'tabs', 'incomings', 'noBins'):
    now.pop(k, None)

io.open(at, 'w', encoding='utf-8').write(json.dumps(now, ensure_ascii=False, indent=2))
