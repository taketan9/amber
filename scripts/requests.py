#!/usr/bin/env python3
"""gui/REQUESTS.ja.md の約束は、いまも守られているか。

**これが在る理由になった失敗** ── 本人が頼んで入ったものを、あとの設計の
見直しで私が消し、しかも「こうすべきでない」という註まで添えた。
コードのどこにも「その行がどこから来たか」が書いていなかったので、
二度目に見たときには**自分で選んだ行と見分けがつかなかった。**

ここはコードの良し悪しを見ない。**本人が頼んだことが、いまも満たされて
いるか**だけを見て、満たされていなければその依頼の名前を言う。ここが
落ちたときにやることは「正規表現を直す」ではない ──
**「頼まれたものを消した。消す前に訊きに行く」である。**

    python3 scripts/requests.py          # 通ったか落ちたか
    python3 scripts/requests.py --list   # 各行が何を見ているか

各行の検査の欄は `path ~ 正規表現`（合うこと）か `path !~ 正規表現`
（ファイルのどこにも出ないこと）。

**否定を別の演算子にしてあるのは、わざと。** はじめは否定の先読み
（`^(?!.*やめる).*$`）で書いていたが、あれは**その語を含まない行が一行でも
あれば通る** ── つまり、いつでも通る。三行が何も確かめないまま「守られて
います」と言っていた。**このファイルが在るのは、まさにそれを起こさない
ため。** 検査を足したら必ず変異テストする ── 壊して、鳴るのを見て、戻す。
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TABLE = ROOT / "REQUESTS.ja.md"


def rows() -> list[dict]:
    """Every data row of the one table in the register."""
    out: list[dict] = []
    for line in TABLE.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line.startswith("|"):
            continue
        cells = [c.strip() for c in line.strip("|").split("|")]
        # 一つ目のセルが数字の行だけが中身の行。それ以外は見出しか区切りなので、
        # 素通りする。
        if not cells or not cells[0].isdigit():
            continue
        # **セルの数が合わない行は、「壊れている」のであって「無い」のではない。**
        # 22 行目の検査に `\|\|` と書いたことがある ── 正規表現としては
        # まったく正しく、markdown の表に入る `|` の数より 2 つ多い。7 セルに
        # 割れて下の `continue` をすり抜け、**25 行あるのに台帳は「24 件、
        # すべて守られています」と言った。** 読んでいない問いに答える検査は、
        # 読めないと断る検査より悪い。
        if len(cells) != 5:
            out.append({
                "n": cells[0],
                "asked": cells[2] if len(cells) > 2 else "",
                "error": f"5列のはずが {len(cells)} 列です — 検査に `|` は書けません（表が割れます）",
            })
            continue
        num, date, asked, what, check = cells
        # **パスに空白を許す。** ノートの名前に空白は当たり前で、
        # `packaging/welcome/amber へようこそ.md` が書けなかった。
        # 切れ目は「空白に挟まれた `~` / `!~`」の最初の一つ。
        m = re.match(r"`(.+?)\s+(!?~)\s+(.+)`$", check)
        if not m:
            out.append({"n": num, "asked": asked, "error": f"検査の書き方が読めません: {check}"})
            continue
        out.append({
            "n": num, "date": date, "asked": asked, "what": what,
            "path": m.group(1), "negate": m.group(2) == "!~", "pattern": m.group(3),
        })
    return out


def main() -> int:
    listing = "--list" in sys.argv
    entries = rows()
    if not entries:
        print(f"！ {TABLE} に読める行がありません")
        return 1

    bad: list[tuple[dict, str]] = []
    for e in entries:
        if "error" in e:
            bad.append((e, e["error"]))
            continue
        f = ROOT / e["path"]
        if not f.exists():
            bad.append((e, f"{e['path']} がありません"))
            continue
        try:
            rx = re.compile(e["pattern"], re.MULTILINE)
        except re.error as err:
            bad.append((e, f"正規表現が壊れています: {err}"))
            continue
        hit = rx.search(f.read_text(encoding="utf-8"))
        if e["negate"] and hit:
            line = f.read_text(encoding="utf-8")[: hit.start()].count("\n") + 1
            bad.append((e, f"{e['path']}:{line} に、あってはならないものがあります"))
        elif not e["negate"] and not hit:
            bad.append((e, f"{e['path']} が条件を満たしていません"))
        elif listing:
            op = "!~" if e["negate"] else "~"
            print(f"  ok  {e['n']:>2}. {e['asked'][:44]}")
            print(f"          {e['path']}  {op}  {e['pattern'][:60]}")

    print("=" * 72)
    if bad:
        print(f"守られていない依頼が {len(bad)} 件あります。\n")
        for e, why in bad:
            print(f"  ✗ {e['n']}. {e.get('date', '')}  {e['asked']}")
            print(f"      満たすもの: {e.get('what', '(未記入)')}")
            print(f"      {why}\n")
        print("直す前に、まず訊いてください。これは頼まれて入ったものです。")
        print("=" * 72)
        return 1
    print(f"依頼 {len(entries)} 件、すべて守られています")
    print("=" * 72)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
