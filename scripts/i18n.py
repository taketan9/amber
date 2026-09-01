#!/usr/bin/env python3
"""窓版のうち、まだ日本語しか話せない部分がどれだけ残っているか。

端末版は `tr(en, ja)` で書かれていて、窓版は日本語直書きから始まりました。
`言語` を入れるというのは、**画面に出る文字列を1つずつ両方の言葉にする**
作業で、途中で止まると「切り替えたのに半分そのまま」になります。だから
残りを数えます。

数え方: `gui/renderer.js` の文字列リテラルのうち日本語を含むものを取り、
`tr(` の引数になっているものを引きます。**コメントは数えません**（経緯は
日本語で書いてあり、それは画面に出ません）。

    python3 scripts/i18n.py          # 残りの件数
    python3 scripts/i18n.py --list   # 残っている文字列
    python3 scripts/i18n.py --list 40

端末版も完全ではありません（`tr` 755 箇所に対し、日本語だけの文字列が
500 以上ある）。ここでの目標は「端末版と同じところまで」ではなく、
**毎日目に入る面から順に減らす**ことです。
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
JA = re.compile(r"[぀-ヿ一-鿿]")
# `'…'` `"…"` `` `…` ``、ただし改行をまたぐ単引用符は文字列ではない。
LIT = re.compile(r"'((?:[^'\\\n]|\\.)*)'|\"((?:[^\"\\\n]|\\.)*)\"|`((?:[^`\\]|\\.)*)`")


def source_without_comments(text):
    """行コメントを落とす。文字列の中の `//` を消さないよう、素朴に
    「行頭が // の行だけ」にする ── audit.py が一度これで `'http://'` を
    壊しているので、賢くしない。"""
    out = []
    for line in text.splitlines():
        t = line.lstrip()
        out.append("" if t.startswith("//") else line)
    return "\n".join(out)


def spans_inside_tr(text):
    """`tr(` の丸括弧の中。入れ子とテンプレート文字列があるので、括弧を数える。"""
    spans = []
    for m in re.finditer(r"\btr\(", text):
        depth = 0
        i = m.end() - 1
        while i < len(text):
            c = text[i]
            if c == "(":
                depth += 1
            elif c == ")":
                depth -= 1
                if depth == 0:
                    spans.append((m.end(), i))
                    break
            i += 1
    return spans


def untranslated(path):
    text = source_without_comments((ROOT / path).read_text(encoding="utf-8"))
    inside = spans_inside_tr(text)

    def covered(at):
        return any(a <= at < b for a, b in inside)

    out = []
    for m in LIT.finditer(text):
        v = m.group(1) or m.group(2) or m.group(3) or ""
        if JA.search(v) and not covered(m.start()):
            out.append((text[: m.start()].count("\n") + 1, v))
    return out


def nested(path):
    """`tr(en, tr(en, ja))` ── 一語を一括で包むと、既に包んであるものの中の
    日本語まで包みます。二度やりました。英語側が二重になるので画面には
    出ませんが、次に誰かが英語を直すとき片方しか直りません。"""
    text = source_without_comments((ROOT / path).read_text(encoding="utf-8"))
    return [
        (text[: m.start()].count("\n") + 1, m.group(0)[:80])
        for m in re.finditer(r"\btr\([^,()]*,\s*tr\(", text)
    ]


def main():
    left = untranslated("gui/renderer.js")
    bad = nested("gui/renderer.js")
    done = len(spans_inside_tr(source_without_comments(
        (ROOT / "gui/renderer.js").read_text(encoding="utf-8"))))
    total = done + len(left)
    if "--list" in sys.argv:
        after = sys.argv[sys.argv.index("--list") + 1:]
        cap = int(after[0]) if after and after[0].isdigit() else 60
        for n, v in left[:cap]:
            print(f"  renderer.js:{n}  {v[:90]}")
        if len(left) > cap:
            print(f"  …ほか {len(left) - cap} 件")
        print()
    pct = 100 * done // total if total else 100
    print("=" * 72)
    for n, t in bad:
        print(f"  ■ tr が入れ子になっています  renderer.js:{n}  {t}")
    if bad:
        print()
    print(f"  両方の言葉で言えるもの {done} / {total}（{pct}%）"
          f" ── まだ日本語だけ {len(left)} 件")
    print("=" * 72)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
