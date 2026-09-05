#!/usr/bin/env python3
"""amber のアイコン（案 S4「生成りの葉・尻尾つき」）を焼く。

    python3 packaging/amber.py          # amber.ico / amber.png / iOS の 1024

cian の `icon.py` と同じ理由で外部ライブラリを使わない ── **この機械に無いものを
要求しない**。rsvg も ImageMagick も PIL も入っていない。

ただし cian のアイコンと違い、こちらは角丸長方形だけでは描けない（葉は
3次ベジェ4本、尻尾と字は丸端の線）。そこで**走査線で塗る**:

  * 葉は多角形に折って、走査線と交わる x を偶奇で対にする
  * 尻尾と二行は**円を並べたもの**として扱う ── 丸端の線の正体は
    「経路に沿って置いた円の和」なので、走査線と円の交わりは閉じた式で出る
  * 横方向は区間のまま画素に配るので**解析的に階調が出る**。縦だけ 4 倍に
    刻めばよく、16 倍の超標本より速くて綺麗

**iOS の 1024 はアルファ無し・角丸無しで書く。** iOS は自分で角丸を被せるので
角を丸めた絵を渡すと二重になり、App Store はアルファのあるアイコンを弾く。
`icon.py` の PNG は RGBA 固定なので、ここでは色型 2（RGB）で書く。
そのぶん `ios-icon.sh` の「1.18 倍して中央を切る」小細工は要らない。

数字は `packaging/amber.svg` と同じ。**片方だけ直すと必ずずれる。**
"""

import math
import re
import struct
import zlib
from pathlib import Path


OUT = Path(__file__).resolve().parent
ROOT = OUT.parent
SIZES = [16, 24, 32, 48, 64, 128, 256]

# --- 案 S4。すべて 100x100 の座標系（SVG の数字そのまま） ---
TILE_A = (0xFF, 0xD9, 0x7F)      # 左上
TILE_B = (0xF2, 0xA6, 0x2C)      # 右下
LEAF_C = (0xFF, 0xF4, 0xDE)      # 生成り。純白は琥珀の上で青く見える
RX = 26.0

LEAF = [
    ((10, 62), (6, 38), (26, 18), (50, 15)),
    ((50, 15), (74, 12), (90, 24), (97, 35)),
    ((97, 35), (88, 50), (66, 68), (44, 77)),
    ((44, 77), (26, 84), (12, 78), (10, 62)),
]
TAIL = ((12, 66), (6, 74), (0, 84), (-4, 96))
TAIL_W = 7.0
CUTS = [
    ((24, 46), (42, 36), (60, 30), (78, 27)),
    ((24, 66), (40, 57), (54, 51), (66, 47)),
]
CUT_W = 8.0



def ico(pngs):
    """PNG を並べた .ico（全部 PNG、32bpp）。

    cian の `packaging/icon.py` から写した ── 分かれたので、向こうを消しても
    こちらは焼ける。8行の仕様（`.ico` のヘッダ）で、判断は入っていない。
    """
    n = len(pngs)
    head = struct.pack('<HHH', 0, 1, n)
    off = 6 + 16 * n
    dirs, body = b'', b''
    for size, data in pngs:
        dirs += struct.pack('<BBBBHHII',
                            0 if size == 256 else size,
                            0 if size == 256 else size,
                            0, 0, 1, 32, len(data), off)
        off += len(data)
        body += data
    return head + dirs + body


def flatten(curve, n):
    p0, p1, p2, p3 = curve
    out = []
    for i in range(n + 1):
        t = i / n
        u = 1 - t
        a, b, c, d = u * u * u, 3 * u * u * t, 3 * u * t * t, t * t * t
        out.append((a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
                    a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1]))
    return out


def leaf_poly():
    poly = []
    for curve in LEAF:
        pts = flatten(curve, 64)
        poly += pts[:-1]          # 端点は次の曲線の始点と同じ
    return poly


def disks(curve, r):
    """丸端の線＝経路に沿って置いた円の和。128 点なら中心の間隔は半径よりずっと狭い。"""
    return [(x, y, r) for x, y in flatten(curve, 128)]


def poly_spans(poly, y):
    xs = []
    n = len(poly)
    for i in range(n):
        x0, y0 = poly[i]
        x1, y1 = poly[(i + 1) % n]
        if (y0 <= y < y1) or (y1 <= y < y0):
            xs.append(x0 + (y - y0) * (x1 - x0) / (y1 - y0))
    xs.sort()
    return [(xs[i], xs[i + 1]) for i in range(0, len(xs) - 1, 2)]


def disk_spans(ds, y):
    out = []
    for cx, cy, r in ds:
        dy = y - cy
        if -r < dy < r:
            d = math.sqrt(r * r - dy * dy)
            out.append((cx - d, cx + d))
    return merge(out)


def merge(iv):
    if not iv:
        return []
    iv = sorted(iv)
    out = [list(iv[0])]
    for a, b in iv[1:]:
        if a <= out[-1][1]:
            out[-1][1] = max(out[-1][1], b)
        else:
            out.append([a, b])
    return [(a, b) for a, b in out]


def subtract(a, b):
    if not b:
        return a
    out = []
    for s, e in a:
        cur = [(s, e)]
        for bs, be in b:
            nxt = []
            for cs, ce in cur:
                if be <= cs or bs >= ce:
                    nxt.append((cs, ce))
                    continue
                if bs > cs:
                    nxt.append((cs, bs))
                if be < ce:
                    nxt.append((be, ce))
            cur = nxt
        out += cur
    return out


def intersect(a, b):
    out = []
    for s, e in a:
        for bs, be in b:
            lo, hi = max(s, bs), min(e, be)
            if hi > lo:
                out.append((lo, hi))
    return merge(out)


def tile_spans(y, r):
    """角丸長方形が走査線 y で占める区間。r=0 なら真四角。"""
    if y < 0 or y > 100:
        return []
    if r <= 0:
        return [(0.0, 100.0)]
    dy = 0.0
    if y < r:
        dy = r - y
    elif y > 100 - r:
        dy = y - (100 - r)
    if dy <= 0:
        return [(0.0, 100.0)]
    d = math.sqrt(max(r * r - dy * dy, 0.0))
    return [(r - d, 100 - r + d)]


def add_span(row, x0, x1, w):
    n = len(row)
    x0 = max(x0, 0.0)
    x1 = min(x1, float(n))
    if x1 <= x0:
        return
    i0 = int(x0)
    i1 = int(math.ceil(x1)) - 1
    if i0 == i1:
        row[i0] += (x1 - x0) * w
        return
    row[i0] += (i0 + 1 - x0) * w
    for i in range(i0 + 1, i1):
        row[i] += w
    row[i1] += (x1 - i1) * w


def render(size, square=False):
    """(rgba, rgb) を返す。square のときは角を丸めず、アルファは全面 255。"""
    n = size
    ss = 4
    sc = n / 100.0
    poly = leaf_poly()
    tail = disks(TAIL, TAIL_W / 2)
    cut = disks(CUTS[0], CUT_W / 2) + disks(CUTS[1], CUT_W / 2)
    r = 0.0 if square else RX
    # 小さいところは尻尾も字も潰れる。線を細らせず、絵ごと少しだけ太らせる
    rgba = bytearray(n * n * 4)
    rgb = bytearray(n * n * 3)
    for py in range(n):
        row_t = [0.0] * n
        row_l = [0.0] * n
        for s in range(ss):
            yy = (py + (s + 0.5) / ss) * 100.0 / n
            tv = tile_spans(yy, r)
            if not tv:
                continue
            shape = merge(poly_spans(poly, yy) + disk_spans(tail, yy))
            shape = subtract(shape, disk_spans(cut, yy))
            shape = intersect(shape, tv)
            w = 1.0 / ss
            for a, b in tv:
                add_span(row_t, a * sc, b * sc, w)
            for a, b in shape:
                add_span(row_l, a * sc, b * sc, w)
        gy = (py + 0.5) / n
        for px in range(n):
            a = row_t[px]
            a = 1.0 if a > 1.0 else (0.0 if a < 0.0 else a)
            lf = row_l[px]
            if lf > a:
                lf = a
            gx = (px + 0.5) / n
            t = ((gx - 0.15) * 0.7 + gy) / (0.7 * 0.7 + 1.0)
            t = 0.0 if t < 0 else (1.0 if t > 1 else t)
            o = px * 4
            p = px * 3
            if a <= 0.0:
                continue
            for k in range(3):
                g = TILE_A[k] + (TILE_B[k] - TILE_A[k]) * t
                v = (g * (a - lf) + LEAF_C[k] * lf) / a
                v = int(v + 0.5)
                rgba[py * n * 4 + o + k] = 255 if v > 255 else (0 if v < 0 else v)
                rgb[py * n * 3 + p + k] = rgba[py * n * 4 + o + k]
            rgba[py * n * 4 + o + 3] = int(a * 255 + 0.5)
    return bytes(rgba), bytes(rgb)


def png(size, data, chan):
    """chan=4 なら RGBA（色型 6）、3 なら RGB（色型 2、アルファ無し）。"""
    raw = bytearray()
    for y in range(size):
        raw.append(0)
        raw += data[y * size * chan:(y + 1) * size * chan]

    def chunk(tag, body):
        c = struct.pack('>I', len(body)) + tag + body
        return c + struct.pack('>I', zlib.crc32(tag + body) & 0xFFFFFFFF)

    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8,
                                         6 if chan == 4 else 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(bytes(raw), 9))
            + chunk(b'IEND', b''))



def fmt(v):
    return '%g' % v


def leaf_d():
    """LEAF を SVG の d 属性の文字列に組み直す。"""
    d = 'M{} {}'.format(*map(fmt, LEAF[0][0]))
    for curve in LEAF:
        d += ' C' + ' '.join('{} {}'.format(*map(fmt, pt)) for pt in curve[1:])
    return d + ' Z'


def agree():
    """葉の形が三か所で同じか。**ここが黙ると、窓とアプリで別の葉になる。**

    数字は `packaging/amber.svg`（原本）、この file（焼く）、
    `ios/Cian/Writing.swift` の `Mark()`、`gui/renderer.js` の `mark()` の四か所にある。増やしたくは
    なかったが、SVG を読む道具がこの機械に無い以上どこかで写すしかない。
    ならば**写しがずれたときに焼けなくする**のが筋。
    """
    d = leaf_d()
    for path in ('packaging/amber.svg', 'gui/renderer.js'):
        if d not in (ROOT / path).read_text(encoding='utf-8'):
            raise SystemExit(
                f'葉の形が {path} と食い違っています。\n'
                f'  ここ  : {d}\n'
                '四か所すべて同じにしてください。')

    # iPhone は SwiftUI なので `d` 属性を持たない。`addCurve` は**終点が先**で
    # 制御点が後ろに来るので、同じ点を SwiftUI の順に並べ直して突き合わせる。
    want = [LEAF[0][0]]
    for c in LEAF:
        want += [c[3], c[1], c[2]]
    want += [TAIL[0], TAIL[3], TAIL[1], TAIL[2]]
    for c in CUTS:
        want += [c[0], c[3], c[1], c[2]]
    swift = ROOT / 'ios/Cian/Writing.swift'
    got = [(float(x), float(y)) for x, y in
           re.findall(r'at\((-?[\d.]+), (-?[\d.]+),', swift.read_text(encoding='utf-8'))]
    if got != [(float(a), float(b)) for a, b in want]:
        raise SystemExit(
            '葉の形が ios/Cian/Writing.swift と食い違っています。\n'
            f'  ここ  : {want}\n'
            f'  むこう: {got}\n'
            '四か所すべて同じにしてください。')
    return d


if __name__ == '__main__':
    print('葉の形は四か所で一致:', agree())
    made = []
    for s in SIZES:
        rgba, _ = render(s)
        data = png(s, rgba, 4)
        made.append((s, data))
        print(f'  {s:>3}x{s:<3} {len(data):>7} B')
    blob = ico(made)
    (OUT / 'amber.ico').write_bytes(blob)
    print(f'\namber.ico  {len(blob)} B  ({len(made)} 枚)')
    (OUT / 'amber.png').write_bytes(dict(made)[256])
    print(f'amber.png  {len(dict(made)[256])} B  (256px、Dock 用)')

    # iOS。角丸もアルファも付けない ── 付けると角が二重に丸まり、審査で弾かれる
    _, rgb = render(1024, square=True)
    big = png(1024, rgb, 3)
    dest = ROOT / 'ios/Cian/Assets.xcassets/AppIcon.appiconset/AppIcon.png'
    dest.write_bytes(big)
    print(f'{dest.relative_to(ROOT)}  {len(big)} B  (1024px、角丸なし・アルファなし)')
