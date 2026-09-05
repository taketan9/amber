#!/usr/bin/env python3
"""amber のアイコン（案2「琥珀の中の Markdown」）を焼く。

    python3 packaging/amber_icon.py                 # 原本 → すべての大きさ
    python3 packaging/amber_icon.py --lift 見本.jpg  # 見本の紙から原本を起こす

**この絵は描けない。** 琥珀は透けていて、中に気泡があり、縁には光が回っている。
`amber.py` の走査線は角丸と丸端の線しか塗れないので、案 S4 の葉のようには
式で描き直せない ── ここでは**画素そのものを原本として持つ**。

原本は二枚:

  * `amber-master.png`        RGBA。角丸のまま、外は透明。
  * `amber-master-square.png` RGB。角まで埋めた真四角。iOS 用の下地。

見本の絵は **790x886 と、正方形ではなかった**（描き手が少し縦長に置いた）。
アプリのアイコンは正方形でなければ iOS と macOS の型に角を削られるので、
`--lift` で**縦を詰めて正方形にする**。伸ばさず詰めるのは、伸ばすと無い画素を
作ることになるから。角丸は 12% ぶんだけ縦につぶれるが、その差は 180 の半径に
対して見えない。

外部ライブラリは使わない ── `amber.py` と同じ理由で、この機械には PIL も
ImageMagick も rsvg も無い。使うのは `sips`（JPEG を PNG に直すときだけ）と
`iconutil`（icns を綴じるときだけ）で、どちらも macOS に元から居る。
"""

import math
import struct
import subprocess
import sys
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent
ROOT = OUT.parent

MASTER = OUT / 'amber-master.png'
MASTER_SQ = OUT / 'amber-master-square.png'

ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
ICNS_SIZES = [16, 32, 64, 128, 256, 512, 1024]

# macOS だけ、絵を canvas いっぱいに置かない。Apple の升目では 1024 のうち
# 角丸の四角は 824 で、まわりの 100 は余白（と影）のために空いている。
# **いっぱいに置くと Dock で隣より一回り大きく見える。**
MAC_BODY = 824
MAC_CANVAS = 1024


# ── PNG の読み書き（stdlib だけ） ──────────────────────────────────────

def png_read(path):
    """8bit・非インタレースの PNG を (w, h, RGBA) で返す。"""
    d = Path(path).read_bytes()
    if d[:8] != b'\x89PNG\r\n\x1a\n':
        raise SystemExit(f'PNG ではありません: {path}')
    i, idat, hdr = 8, b'', None
    while i < len(d):
        ln = struct.unpack('>I', d[i:i + 4])[0]
        tag, body = d[i + 4:i + 8], d[i + 8:i + 8 + ln]
        if tag == b'IHDR':
            hdr = struct.unpack('>IIBBBBB', body)
        elif tag == b'IDAT':
            idat += body
        i += 12 + ln
    w, h, depth, ctype, _, _, inter = hdr
    if depth != 8 or inter or ctype not in (2, 6):
        raise SystemExit(f'読めない PNG です（{depth}bit 色型{ctype}）: {path}')
    nch = 4 if ctype == 6 else 3
    raw = zlib.decompress(idat)
    stride = w * nch
    rows = []
    prev = bytearray(stride)
    p = 0
    for _ in range(h):
        f = raw[p]
        p += 1
        line = bytearray(raw[p:p + stride])
        p += stride
        if f == 1:
            for x in range(nch, stride):
                line[x] = (line[x] + line[x - nch]) & 255
        elif f == 2:
            for x in range(stride):
                line[x] = (line[x] + prev[x]) & 255
        elif f == 3:
            for x in range(stride):
                a = line[x - nch] if x >= nch else 0
                line[x] = (line[x] + ((a + prev[x]) >> 1)) & 255
        elif f == 4:
            for x in range(stride):
                a = line[x - nch] if x >= nch else 0
                b = prev[x]
                c = prev[x - nch] if x >= nch else 0
                q = a + b - c
                pa, pb, pc = abs(q - a), abs(q - b), abs(q - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 255
        rows.append(line)
        prev = line
    if nch == 4:
        return w, h, b''.join(bytes(r) for r in rows)
    out = bytearray(w * h * 4)
    for y, line in enumerate(rows):
        for x in range(w):
            o = (y * w + x) * 4
            out[o:o + 3] = line[x * 3:x * 3 + 3]
            out[o + 3] = 255
    return w, h, bytes(out)


def png_write(path, w, h, rgba, alpha=True):
    """alpha=False なら色型 2（RGB）で書く ── iOS はアルファのある絵を弾く。"""
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        if alpha:
            raw += rgba[y * w * 4:(y + 1) * w * 4]
        else:
            line = rgba[y * w * 4:(y + 1) * w * 4]
            for x in range(w):
                raw += line[x * 4:x * 4 + 3]

    def chunk(tag, body):
        c = struct.pack('>I', len(body)) + tag + body
        return c + struct.pack('>I', zlib.crc32(tag + body) & 0xFFFFFFFF)

    blob = (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8,
                                         6 if alpha else 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(bytes(raw), 9))
            + chunk(b'IEND', b''))
    Path(path).write_bytes(blob)
    return blob


# ── 大きさを変える ────────────────────────────────────────────────────
#
# **アルファを掛けた値で混ぜる。** 掛けずに混ぜると、透明なところに残っている
# 色が縁ににじむ ── 立方体の外は真っ白なので、白いふちになって出る。

def _mitchell(t):
    """縮めるとき。B=C=1/3。輪を作らないので小さい札で崩れない。"""
    t = abs(t)
    b = c = 1.0 / 3.0
    if t < 1:
        return ((12 - 9 * b - 6 * c) * t ** 3
                + (-18 + 12 * b + 6 * c) * t ** 2 + (6 - 2 * b)) / 6
    if t < 2:
        return ((-b - 6 * c) * t ** 3 + (6 * b + 30 * c) * t ** 2
                + (-12 * b - 48 * c) * t + (8 * b + 24 * c)) / 6
    return 0.0


def _catrom(t):
    """伸ばすとき。Mitchell より締まって見える。"""
    t = abs(t)
    if t < 1:
        return 1.5 * t ** 3 - 2.5 * t ** 2 + 1
    if t < 2:
        return -0.5 * t ** 3 + 2.5 * t ** 2 - 4 * t + 2
    return 0.0


def _weights(src, dst):
    """出力の一点ごとに (始点, [重み]) を返す。縦横で同じものを使い回す。"""
    scale = dst / src
    f = _catrom if scale >= 1 else _mitchell
    step = 1.0 if scale >= 1 else 1.0 / scale     # 縮めるぶん、傘を広げる
    support = 2.0 * step
    out = []
    for i in range(dst):
        center = (i + 0.5) / scale - 0.5
        lo = int(math.floor(center - support + 0.5))
        hi = int(math.ceil(center + support - 0.5))
        ws, total = [], 0.0
        for j in range(lo, hi + 1):
            v = f((j - center) / step)
            ws.append(v)
            total += v
        if total:
            ws = [v / total for v in ws]
        out.append((lo, ws))
    return out


def resize(w, h, rgba, nw, nh):
    """RGBA を (nw, nh) に。アルファを掛けて混ぜ、最後に戻す。"""
    pre = [0.0] * (w * h * 4)
    for i in range(w * h):
        a = rgba[i * 4 + 3] / 255.0
        pre[i * 4] = rgba[i * 4] * a
        pre[i * 4 + 1] = rgba[i * 4 + 1] * a
        pre[i * 4 + 2] = rgba[i * 4 + 2] * a
        pre[i * 4 + 3] = a * 255.0

    wx = _weights(w, nw)
    tmp = [0.0] * (nw * h * 4)                    # 横だけ先に
    for y in range(h):
        row = y * w * 4
        dst = y * nw * 4
        for i, (lo, ws) in enumerate(wx):
            acc = [0.0, 0.0, 0.0, 0.0]
            for k, wt in enumerate(ws):
                j = lo + k
                j = 0 if j < 0 else (w - 1 if j >= w else j)
                o = row + j * 4
                for c in range(4):
                    acc[c] += pre[o + c] * wt
            o = dst + i * 4
            for c in range(4):
                tmp[o + c] = acc[c]

    wy = _weights(h, nh)
    out = bytearray(nw * nh * 4)
    for y, (lo, ws) in enumerate(wy):
        for x in range(nw):
            acc = [0.0, 0.0, 0.0, 0.0]
            for k, wt in enumerate(ws):
                j = lo + k
                j = 0 if j < 0 else (h - 1 if j >= h else j)
                o = (j * nw + x) * 4
                for c in range(4):
                    acc[c] += tmp[o + c] * wt
            a = acc[3]
            a = 0.0 if a < 0 else (255.0 if a > 255 else a)
            o = (y * nw + x) * 4
            out[o + 3] = int(a + 0.5)
            if a <= 0.5:
                continue
            for c in range(3):
                v = acc[c] * 255.0 / a            # 掛けたぶんを戻す
                out[o + c] = 0 if v < 0 else (255 if v > 255 else int(v + 0.5))
    return bytes(out)


def paste(canvas, cw, art, aw, x0, y0):
    """canvas（RGBA）の (x0, y0) に art をそのまま置く。重ねない。"""
    for y in range(aw):
        d = ((y0 + y) * cw + x0) * 4
        s = y * aw * 4
        canvas[d:d + aw * 4] = art[s:s + aw * 4]


# ── .ico ──────────────────────────────────────────────────────────────

def ico(pngs):
    """PNG を並べた .ico（全部 PNG、32bpp）。`amber.py` から写した。"""
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


# ── 見本の紙から原本を起こす（--lift） ────────────────────────────────

def lift(sheet, pick=None):
    """見本の紙から琥珀の四角を切り出し、外を透かして原本にする。

    紙には案が二つ並んでいる。**濃い橙が続く列の帯**を数えて塊に分け、
    面積の一番大きいものを採る（`--pick` で左から数えて選び直せる）。

    縁は一画素で立ち上がる。横に走査すると左右はきれいに出るが、上下の縁は
    寝ているので出ない ── だから**縦にも走査して、二つの小さいほうを採る**。
    角では両方が効く。

    絵は **788x846 と、正方形ではなかった**。縦を詰めて正方形にする。
    """
    src = Path(sheet)
    tmp = None
    if src.suffix.lower() != '.png':
        tmp = OUT / '.lift.png'
        subprocess.run(['sips', '-s', 'format', 'png', str(src), '--out', str(tmp)],
                       check=True, stdout=subprocess.DEVNULL)
        src = tmp
    w, h, px = png_read(src)
    if tmp:
        tmp.unlink()          # 5MB の途中の絵を置いていかない
    print(f'見本: {w}x{h}')

    def amber(x, y):
        """彩度。**橙み（赤-青）では下の縁が切れない。**

        琥珀の下には光が回り込んでいて、そこは薄い橙 ── 赤-青で測ると
        本体と地続きに見え、35 画素かけてだらだら下がる。どこで切っても
        生成りの帯が縁に残った。彩度なら、光は白に近いぶん一気に落ちる。
        上下左右の四辺が**同じ一つの敷居で**切れるのはこちらだけ。
        """
        o = (y * w + x) * 4
        r, g, b = px[o], px[o + 1], px[o + 2]
        return max(r, g, b) - min(r, g, b)

    def rgb(x, y):
        o = (y * w + x) * 4
        return px[o], px[o + 1], px[o + 2]

    EDGE = 120         # ここを越えたら本体。回り込んだ光は 100 までしか来ない
    # ── 濃い橙の列を数え、途切れで塊に分ける
    cols = [0] * w
    for y in range(h):
        for x in range(w):
            if amber(x, y) > EDGE:
                cols[x] += 1
    runs, cur = [], None
    for x in range(w):
        if cols[x] > 30:
            cur = [x, x] if cur is None else [cur[0], x]
        elif cur is not None and x - cur[1] > 12:
            runs.append(cur)
            cur = None
    if cur is not None:
        runs.append(cur)
    if not runs:
        raise SystemExit('琥珀の塊が見つかりません。')

    boxes = []
    for x0, x1 in runs:
        rows = [y for y in range(h)
                if sum(1 for x in range(x0, x1 + 1) if amber(x, y) > EDGE) > 30]
        if rows:
            boxes.append((x0, rows[0], x1, rows[-1]))
    for i, (a, b, c, d) in enumerate(boxes):
        print(f'  塊{i}: ({a}, {b}) - ({c}, {d})  {c - a + 1}x{d - b + 1}')
    if pick is None:
        box = max(boxes, key=lambda b: (b[2] - b[0]) * (b[3] - b[1]))
    else:
        box = boxes[pick]
    x0, y0, x1, y1 = box
    bw, bh = x1 - x0 + 1, y1 - y0 + 1
    print(f'採る: ({x0}, {y0}) から {bw}x{bh}')


    # ── 形を測る。**縁を追いかけるのはやめる。**
    #
    # この絵の輪郭は、どの一つの物差しでも切れなかった:
    #   * 橙み（赤-青）── 下は影に溶けて 35 画素だらだら下がる。どこで切っても
    #     生成りの帯が残る。
    #   * 彩度 ── 下はきれいに切れるが、**上の縁は光って白に寄る**ので欠ける。
    #
    # 追う相手が悪い。これは角丸の四角で、形は式で書ける。**測って当てはめ、
    # それを型にする。** そうすれば四辺とも同じ理屈で、どの大きさでも
    # 縁は解析的に滑らかになる ── `amber.py` が葉でやっていたのと同じことを、
    # ここでは外枠だけについてやる。
    body = [None] * bh
    for y in range(bh):
        ins = [x for x in range(bw) if amber(x0 + x, y0 + y) > EDGE]
        body[y] = (ins[0], ins[-1]) if ins else None

    def radius():
        """四つの角から半径を測る。中央値を採るので、一つ崩れても効かない。

        中心 (R, R) の円なら (R-dx)^2 + (R-dy)^2 = R^2 なので、
        角からの食い込み (dx, dy) 一組ごとに R = (dx+dy) + sqrt(2 dx dy)。
        """
        got = []
        rows = [(y, body[y]) for y in range(bh) if body[y]]
        top, bot = rows[0][0], rows[-1][0]
        for y, (lo, hi) in rows:
            for dy, dx in ((y - top, lo), (y - top, bw - 1 - hi),
                           (bot - y, lo), (bot - y, bw - 1 - hi)):
                if 8 < dx < dy < bh // 3:
                    got.append(dx + dy + math.sqrt(2.0 * dx * dy))
        if not got:
            raise SystemExit('角が測れません。')
        got.sort()
        return got[len(got) // 2]

    R = radius()
    print(f'  角の半径: {R:.1f}（{bw} のうち {100 * R / bw:.1f}%）')

    def spans(yy):
        """角丸四角が走査線 yy で占める区間。`amber.py` の tile_spans と同じ理屈。"""
        if yy < 0 or yy > bh:
            return None
        d = 0.0
        if yy < R:
            d = R - yy
        elif yy > bh - R:
            d = yy - (bh - R)
        if d <= 0:
            return 0.0, float(bw)
        if d >= R:
            return None
        t = math.sqrt(R * R - d * d)
        return R - t, bw - R + t

    # ── 型を焼く。縦に 4 枚重ね、横は区間のまま画素に配る（階調が閉じた式で出る）
    SS = 4
    cover = [[0.0] * bw for _ in range(bh)]
    for py in range(bh):
        row = cover[py]
        for s in range(SS):
            sp = spans(py + (s + 0.5) / SS)
            if sp is None:
                continue
            a, b = sp
            i0, i1 = int(a), min(int(math.ceil(b)) - 1, bw - 1)
            if i0 == i1:
                row[i0] += (b - a) / SS
                continue
            row[i0] += (i0 + 1 - a) / SS
            for i in range(i0 + 1, i1):
                row[i] += 1.0 / SS
            row[i1] += (b - i1) / SS

    # ── 色。型の内側は元の画素をそのまま使う。**型が絵より外に出る角だけ**、
    #    一番近い内側の色で埋める ── そこは元の絵では回り込んだ光で、
    #    そのまま使うと角に生成りの粒が残る。
    rgba = bytearray(bw * bh * 4)
    for y in range(bh):
        lo, hi = body[y] if body[y] else (bw, -1)
        for x in range(bw):
            a = cover[y][x]
            if a <= 0.0:
                continue
            sx, sy = x, y
            if x < lo or x > hi:
                sy = y
                if body[y] is None:                # 上下の端、絵がまだ無い行
                    d = 1
                    while True:
                        for uy in (y - d, y + d):
                            if 0 <= uy < bh and body[uy]:
                                sy = uy
                                break
                        else:
                            d += 1
                            continue
                        break
                l2, h2 = body[sy]
                sx = min(max(x, l2), h2)
            o = (y * bw + x) * 4
            r, g, b = rgb(x0 + sx, y0 + sy)
            rgba[o], rgba[o + 1], rgba[o + 2] = r, g, b
            rgba[o + 3] = int(a * 255 + 0.5)

    # ── 正方形にする。**詰めるだけで、伸ばさない**
    n = min(bw, bh)
    if (bw, bh) != (n, n):
        print(f'  正方形に詰めます: {bw}x{bh} → {n}x{n}')
        rgba = resize(bw, bh, bytes(rgba), n, n)
    png_write(MASTER, n, n, bytes(rgba))
    print(f'{MASTER.relative_to(ROOT)}  {n}x{n} RGBA')

    # ── 真四角の下地（iOS）。角の外は**縁より 8px 内側**の色で伸ばす。
    #    縁そのものは光が回って明るいので、それを引き伸ばすと角に輪が出る。
    sq = bytearray(n * n * 4)
    w2, h2, m = n, n, rgba
    for y in range(h2):
        row = [x for x in range(w2) if m[(y * w2 + x) * 4 + 3] > 200]
        if not row:
            yy = y
            while True:
                yy += 1 if y < h2 // 2 else -1
                row = [x for x in range(w2) if m[(yy * w2 + x) * 4 + 3] > 200]
                if row:
                    break
        else:
            yy = y
        lo, hi = row[0], row[-1]
        for x in range(w2):
            sx = x
            if x < lo:
                sx = min(lo + 8, hi)
            elif x > hi:
                sx = max(hi - 8, lo)
            s = (yy * w2 + sx) * 4
            o = (y * w2 + x) * 4
            sq[o:o + 3] = m[s:s + 3]
            sq[o + 3] = 255
    png_write(MASTER_SQ, n, n, bytes(sq))
    print(f'{MASTER_SQ.relative_to(ROOT)}  {n}x{n} RGB（角まで埋めた）')


# ── 焼く ──────────────────────────────────────────────────────────────

def bake():
    if not MASTER.exists():
        raise SystemExit(f'原本がありません: {MASTER}\n'
                         '  python3 packaging/amber_icon.py --lift 見本.jpg')
    w, h, art = png_read(MASTER)
    print(f'原本 {w}x{h}')

    made = []
    for s in ICO_SIZES:
        data = png_write(OUT / f'.tmp{s}.png', s, s, resize(w, h, art, s, s))
        (OUT / f'.tmp{s}.png').unlink()
        made.append((s, data))
        print(f'  {s:>4}x{s:<4} {len(data):>7} B')

    blob = ico(made)
    (OUT / 'amber.ico').write_bytes(blob)
    print(f'\namber.ico  {len(blob)} B  （{len(made)} 枚）')

    png_write(OUT / 'amber.png', 256, 256, resize(w, h, art, 256, 256))
    print(f'amber.png  256px（窓と Linux 用）')

    # アプリの**中**の印。窓の左上（26）、空のときの札（54）、iPhone の
    # 見出し（38pt）で使う。
    #
    # **描き直さない。** 前は葉を三か所に写して描いていて、四か所が同じ形か
    # 見張る仕掛けまで要った ── それでもアイコンを替えた日に、中の印だけが
    # 前の絵のまま残った。同じ絵を渡せば、ずれようがない。
    #
    # 128 なのは、いちばん大きい使い方（54）の 2倍と iPhone の 38pt の 3倍
    # （114）を両方覆う一枚だから。倍寸を別々に焼くほどの数ではない。
    png_write(OUT / 'amber-mark.png', 128, 128, resize(w, h, art, 128, 128))
    print(f'amber-mark.png  128px（アプリの中の印）')

    # macOS。**余白を空ける** ── Apple の升目に合わせないと Dock で浮く。
    iconset = OUT / 'amber.iconset'
    iconset.mkdir(exist_ok=True)
    body = resize(w, h, art, MAC_BODY, MAC_BODY)
    canvas = bytearray(MAC_CANVAS * MAC_CANVAS * 4)
    off = (MAC_CANVAS - MAC_BODY) // 2
    paste(canvas, MAC_CANVAS, body, MAC_BODY, off, off)
    canvas = bytes(canvas)
    for s in ICNS_SIZES:
        img = canvas if s == MAC_CANVAS else resize(MAC_CANVAS, MAC_CANVAS, canvas, s, s)
        if s <= 512:
            png_write(iconset / f'icon_{s}x{s}.png', s, s, img)
        if s >= 32:
            png_write(iconset / f'icon_{s // 2}x{s // 2}@2x.png', s, s, img)
    subprocess.run(['iconutil', '-c', 'icns', str(iconset),
                    '-o', str(OUT / 'amber.icns')], check=True)
    # 同じ絵を素の PNG でも置く。**Electron は `.icns` を読めない** ──
    # `nativeImage.createFromPath` が黙って空を返し、`dock.setIcon` は
    # 何も言わずに何もしない。走らせて確かめている間の Dock はこれを使う。
    png_write(OUT / 'amber-dock.png', 512, 512,
              resize(MAC_CANVAS, MAC_CANVAS, canvas, 512, 512))
    print('amber-dock.png  512px（走らせている間の Dock 用、余白つき）')
    for f in iconset.iterdir():
        f.unlink()
    iconset.rmdir()
    print(f'amber.icns  {(OUT / "amber.icns").stat().st_size} B  （Apple の升目、本体 {MAC_BODY}/1024）')

    # iOS。角丸もアルファも付けない ── 付けると角が二重に丸まり、審査で弾かれる
    if not MASTER_SQ.exists():
        raise SystemExit(f'真四角の原本がありません: {MASTER_SQ}')
    sw, sh, sq = png_read(MASTER_SQ)
    dest = ROOT / 'ios/Cian/Assets.xcassets/AppIcon.appiconset/AppIcon.png'
    big = png_write(dest, 1024, 1024, resize(sw, sh, sq, 1024, 1024), alpha=False)
    print(f'{dest.relative_to(ROOT)}  {len(big)} B  （1024px、角丸なし・アルファなし）')


if __name__ == '__main__':
    args = sys.argv[1:]
    pick = None
    if '--pick' in args:
        i = args.index('--pick')
        pick = int(args[i + 1])
        del args[i:i + 2]
    only = '--lift-only' in args
    if only:
        args.remove('--lift-only')
    if args and args[0] == '--lift':
        if len(args) < 2:
            raise SystemExit('使い方: --lift 見本の画像 [--pick 番号] [--lift-only]')
        lift(args[1], pick)
    if not only:
        bake()
