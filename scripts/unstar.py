#!/usr/bin/env python3
"""**`_` が `*` に化けた跡を、元に戻す**（依頼 666・2026-10-17）。

    scripts/unstar.py <フォルダかファイル> ...          見るだけ（既定）
    scripts/unstar.py --write <フォルダかファイル> ...   書き換える

依頼 666 で直すまで、amber は `READ_COUNT と LAST_UPDATED` のように
**`_` が二つ以上ある行**を強調として読み、保存のたびに `READ*COUNT と
LAST*UPDATED` と書き戻していた。この道具は、その跡を探して `_` に戻す。

**本物のアスタリスクを巻き込まない。** そのための決めごとが三つある。

  一。**コード枠の中は見ない。** 枠の中では強調が効かないので、そこの `*`
      は化けた跡ではない（本人の文書では 9 箇所あり、どれも本物だった）。

  二。**行内コードの中も見ない。** 同じ理由。

  三。**知っている製品名は、そのまま。** `SQL*Net`・`SQL*Plus`・
      `SQL*Loader` は Oracle の正しい綴りで、本人の文書に 46 箇所ある。
      ここを `_` にすると、直すつもりで壊すことになる。

**既定は見るだけ。** `--write` を付けるまで一文字も書かない ── 戻す道具が
戻しすぎるのがいちばん悪い。
"""
import io
import os
import re
import sys

# 化けた跡の形。**前が字か数か `$`、後ろが字か数か `_`。**
#
# 強調は語の中でしか起きなかったので、両隣が語の文字であるものだけを見る。
# `A * B` のように空白で囲まれたものは、最初から強調ではない。
STAR = re.compile(r'(?<=[0-9A-Za-z$])\*(?=[0-9A-Za-z_])')

# **本物のアスタリスクを持つ名前。** 名前そのもので見分ける。
#
# 前の語だけで見分けると取りこぼす ── `DBMS*SQL*MONITOR` の二つめを
# 「`SQL` の直後だから製品名」と誤り、`DBMS_SQL*MONITOR` で止まった。
KEEP_WORDS = frozenset((
    'SQL*PLUS', 'SQL*NET', 'SQL*LOADER',
    'SQL*FORMS', 'SQL*MENU', 'SQL*REPORT', 'SQL*DBA',
))

# 行内コード。中は見ない。
CODE = re.compile(r'`[^`]*`')


def fix_line(line):
    """その行を直したものと、直した数を返す。コードの中は触らない。"""
    # 行内コードの場所を控えて、あとで戻す。
    holes = []

    def stash(m):
        holes.append(m.group(0))
        return '\x00%d\x00' % (len(holes) - 1)

    body = CODE.sub(stash, line)

    n = 0

    def swap(m):
        nonlocal n
        at = m.start()
        # **知っている製品名は、そのまま。** 前後の語を繋いで名前で見る。
        head = re.search(r'[0-9A-Za-z$]+$', body[:at])
        tail = re.match(r'[0-9A-Za-z]+', body[at + 1:])
        if head and tail and (head.group(0) + '*' + tail.group(0)).upper() in KEEP_WORDS:
            return '*'
        n += 1
        return '_'

    body = STAR.sub(swap, body)
    body = re.sub('\x00(\\d+)\x00', lambda m: holes[int(m.group(1))], body)
    return body, n


# **直したあとに残った `*`。** 字の隣にいる `*` は、化けた跡かもしれないし
# 本物かもしれない ── 見分けられないので、推測で詰めずに人に見せる。
# 実例: `gc_contact_*.log`（`*` はワイルドカード）が `gc*contact**.log` に化け、
# 前半は戻せるが `**` の片方が本物なのでどちらとも決められない。
LEFT = re.compile(r'[0-9A-Za-z$_]\*|\*[0-9A-Za-z_]')


def fix_text(text):
    """一本ぶん。**コード枠の中は飛ばす。**"""
    out = []
    hits = []
    left = []
    fence = None
    for no, line in enumerate(text.split('\n'), 1):
        bare = line.lstrip()
        if fence is None and (bare.startswith('```') or bare.startswith('~~~')):
            fence = bare[:3]
            out.append(line)
            continue
        if fence is not None:
            if bare.startswith(fence):
                fence = None
            out.append(line)
            continue
        fixed, n = fix_line(line)
        if n:
            hits.append((no, line, fixed))
        # 直したあとに字の隣の `*` が残っていたら、見てもらう。
        # **分かっているものは先に除く** ── 行内コード・知っている製品名・
        # 本物の強調。除かないと `SQL*Plus` の行で一覧が埋まって読めない。
        bare = CODE.sub('', fixed)
        for w in KEEP_WORDS:
            bare = re.sub(re.escape(w).replace(r'\*', r'\*'), ' ', bare, flags=re.I)
        bare = re.sub(r'\*\*[^*]+\*\*', ' ', bare)
        bare = re.sub(r'\*[^*\s][^*]*\*', ' ', bare)
        if LEFT.search(bare):
            left.append((no, fixed))
        out.append(fixed)
    return '\n'.join(out), hits, left


def walk(targets):
    for t in targets:
        if os.path.isfile(t):
            yield t
        else:
            for here, dirs, files in os.walk(t):
                # amber 自身の控え（`.amber/history`）は直さない ── あれは
                # 「そのときこうだった」という記録なので、書き換えると
                # 戻す先が消える。
                dirs[:] = [d for d in dirs if d != '.amber']
                for f in sorted(files):
                    if f.endswith('.md'):
                        yield os.path.join(here, f)


def main(argv):
    write = '--write' in argv
    targets = [a for a in argv if not a.startswith('--')]
    if not targets:
        print(__doc__.split('\n\n')[1].strip())
        return 2

    total = 0
    books = 0
    unsure = []
    for at in walk(targets):
        text = io.open(at, encoding='utf-8').read()
        fixed, hits, left = fix_text(text)
        if left:
            unsure.append((at, left))
        if not hits:
            continue
        books += 1
        total += sum(1 for _ in hits)
        print('── %s（%d 行）' % (at, len(hits)))
        for no, before, after in hits:
            print('  %5d 前: %s' % (no, before.strip()[:150]))
            print('        後: %s' % after.strip()[:150])
        if write:
            io.open(at, 'w', encoding='utf-8').write(fixed)

    if unsure:
        print('')
        print('── 直しきれません。目で見てください ──')
        for at, rows in unsure:
            print('  %s' % at)
            for no, line in rows:
                print('    %5d %s' % (no, line.strip()[:150]))
    print('')
    print('=' * 60)
    if not total:
        print('直すところはありません')
    elif write:
        print('%d 本・%d 行を直しました' % (books, total))
    else:
        print('%d 本・%d 行に直すところがあります（--write で書き換えます）' % (books, total))
    print('=' * 60)
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
