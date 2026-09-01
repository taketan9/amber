#!/usr/bin/env python3
"""案B「カーソル行」を .ico の7サイズに焼く。

外部ライブラリを使わずに描いている。理由は cian のビルドと同じで、
**この機械に無いものを要求しない**こと ── rsvg も ImageMagick も PIL も
入っていないし、アイコン1枚のために入れるものでもない。図形は角丸長方形
だけなので、4x4 のスーパーサンプリングで十分に滑らかになる。

サイズごとに描き分ける。16px で「一行が光っている」だけが残ればよく、
256px で同じ太さだと子供っぽい。アイコンは一枚の絵を縮めたものではなく、
同じ考えを各サイズで描き直したものの束。
"""

import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent
SIZES = [16, 24, 32, 48, 64, 128, 256]
SS = 4  # スーパーサンプル倍率

# いまのアイコンから読んだ二色。彼が「色味は好き」と言った当のもの。
G0 = (0x1E, 0x8F, 0x9B)
G1 = (0x16, 0xCB, 0xE1)
INK = (0xEA, 0xFC, 0xFF)      # 行の白
BAR = (0x17, 0x83, 0x8F)      # 光った行の中の「ファイル名」


def rounded(x, y, w, h, r, px, py):
    """(px, py) が角丸長方形の中か。すべて 128 単位系。"""
    if px < x or py < y or px > x + w or py > y + h:
        return False
    r = min(r, w / 2, h / 2)
    cx = min(max(px, x + r), x + w - r)
    cy = min(max(py, y + r), y + h - r)
    dx, dy = px - cx, py - cy
    return dx * dx + dy * dy <= r * r


def shapes_for(size):
    """このサイズで描くもの。前から順に重ねる。

    三段に分ける。**16 と 24 は行を四本から三本に減らす** ── 最初の版は
    四本のまま細くしただけで、16px では上の一本が消えて残りが滲み、
    「何かの塊」にしかならなかった。小さいアイコンは同じ絵の縮小ではなく、
    同じ考えを少ない部品で描き直したもの。
    """
    big = size >= 64
    small = size <= 24

    if small:
        # 三本。上下は控えめな行、真ん中が光っている行。
        # 高さ 18 / 32 / 18、間隔 14、上下の余白 12 で縦に中央揃え。
        return [
            (26, 16, 54, 18, 9, INK, 0.50),
            (12, 48, 104, 32, 11, INK, 1.00),
            (26, 94, 46, 18, 9, INK, 0.50),
        ]

    h = 8 if big else 9
    out = [(24, 28, 52, h, h / 2, INK, 0.42),
           (24, 50, 66, h, h / 2, INK, 0.42),
           (24, 92, 58, h, h / 2, INK, 0.42)]
    out.append((18, 64, 92, 22, 7, INK, 1.0))
    if big:
        # 大きいときだけ、光った行の中にファイル名が入る。小さいときは
        # 帯の白さそのものが情報なので、中を抜くと弱くなる。
        out.append((27, 70, 46, 10, 5, BAR, 1.0))
    return out


def render(size):
    """RGBA の bytes を返す。"""
    n = size * SS
    step = 128.0 / n
    layers = shapes_for(size)
    px = bytearray(size * size * 4)

    for iy in range(size):
        for ix in range(size):
            r = g = b = a = 0.0
            for sy in range(SS):
                for sx in range(SS):
                    # サブピクセルの中心を 128 単位系へ
                    u = (ix * SS + sx + 0.5) * step
                    v = (iy * SS + sy + 0.5) * step
                    if not rounded(4, 4, 120, 120, 26, u, v):
                        continue
                    # 地の斜めグラデーション
                    t = max(0.0, min(1.0, (u + v) / 256.0))
                    cr = G0[0] + (G1[0] - G0[0]) * t
                    cg = G0[1] + (G1[1] - G0[1]) * t
                    cb = G0[2] + (G1[2] - G0[2]) * t
                    ca = 1.0
                    for (lx, ly, lw, lh, lr, col, op) in layers:
                        if rounded(lx, ly, lw, lh, lr, u, v):
                            cr = cr + (col[0] - cr) * op
                            cg = cg + (col[1] - cg) * op
                            cb = cb + (col[2] - cb) * op
                    r += cr * ca
                    g += cg * ca
                    b += cb * ca
                    a += ca
            k = SS * SS
            o = (iy * size + ix) * 4
            if a > 0:
                # 非プリマルチプライド: 色はカバーした分だけの平均、alpha は被覆率
                px[o + 0] = int(round(r / a))
                px[o + 1] = int(round(g / a))
                px[o + 2] = int(round(b / a))
                px[o + 3] = int(round(255 * a / k))
            else:
                px[o + 3] = 0
    return bytes(px)


def png(size, rgba):
    raw = bytearray()
    for y in range(size):
        raw.append(0)  # フィルタ None。図形が単純で、圧縮より読みやすさを取る
        raw += rgba[y * size * 4:(y + 1) * size * 4]

    def chunk(tag, data):
        c = struct.pack('>I', len(data)) + tag + data
        return c + struct.pack('>I', zlib.crc32(tag + data) & 0xFFFFFFFF)

    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(bytes(raw), 9))
            + chunk(b'IEND', b''))


def ico(pngs):
    """PNG を並べた .ico。いまの cian.ico と同じ作り（7枚とも PNG、32bpp）。"""
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


if __name__ == '__main__':
    made = []
    for s in SIZES:
        data = png(s, render(s))
        (OUT / f'b-{s}.png').write_bytes(data)
        made.append((s, data))
        print(f'  {s:>3}x{s:<3} {len(data):>7} B')
    blob = ico(made)
    (OUT / 'cian.ico').write_bytes(blob)
    print(f'\ncian.ico  {len(blob)} B  ({len(made)} 枚)')
