#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""MS-ONESTORE の `.one` を読む ── 骨格とプロパティ集合。

**COM が駄目でも写せるように。** 会社の端末では OneNote の COM に繋がらず
（登録も実体も取り次ぎ側も白いのに、呼ぶと落ちる ── 依頼 589）、そこは
こちらでは直せなかった。**書き出したファイルを直に読む**のがこの一枚。

**読めるのは公開仕様のほうだけ**（`guidFileFormat` が
`{109ADD3F-911B-49F5-A5D0-1791EDC8AED8}`）。SharePoint に置かれている `.one` は
MS-FSSHTTPB でくるまれた別ものなので読めない ── **OneNote から書き出せば、
公開仕様で出てくる**（依頼 592 で確かめた）。

**意味の表は、記憶で当てない。** ノードの意味は Apache Tika の
`FndStructureConstants.java`、プロパティの意味は `OneNotePropertyEnum.java` に
ある。この二つを見ずに書いて、二度誤診した ── ノードの名前を取り違えて
「数が合わない」と言い、`TextExtendedAscii` を UTF-16 で読んで
「本文が取れない」と言った。

    ヘッダ(1024) → fcrFileNodeListRoot(172 バイト目)
      → FileNodeListFragment（頭の魔法の数）→ FileNode を辿る
        → ObjectDeclaration → プロパティ集合 → 題と本文
"""
import struct

FRAG  = 0xA4567AB1F5F7F4C4
DECLS = {0x0A4, 0x0A5, 0x0C4, 0x0C5}     # ObjectDeclaration2*RefCount / ReadOnly


def _ref(b, i, stpf, cbf):
    n = {0: 8, 1: 4, 2: 2, 3: 4}[stpf]
    stp = int.from_bytes(b[i:i + n], "little"); i += n
    if stpf in (2, 3): stp *= 8
    m = {0: 4, 1: 8, 2: 1, 3: 2}[cbf]
    cb = int.from_bytes(b[i:i + m], "little"); i += m
    if cbf in (2, 3): cb *= 8
    return stp, cb, i


def objects(d):
    """ファイル中のオブジェクト宣言を、**出てくる順に**返す。"""
    out, seen = [], set()

    def frag(stp, cb):
        if not stp or (stp, cb) in seen or stp + cb > len(d): return
        seen.add((stp, cb))
        f = d[stp:stp + cb]
        if struct.unpack("<Q", f[0:8])[0] != FRAG: return
        i, end = 16, cb - 20
        while i + 4 <= end:
            h = struct.unpack("<I", f[i:i + 4])[0]
            if h == 0: break
            nid, size = h & 0x3FF, (h >> 10) & 0x1FFF
            stpf, cbf, base = (h >> 23) & 3, (h >> 25) & 3, (h >> 27) & 0xF
            if size < 4 or i + size > end: break
            if base in (1, 2):
                cstp, ccb, after = _ref(f, i + 4, stpf, cbf)
                if base == 2:
                    frag(cstp, ccb)
                elif nid in DECLS and cstp and cstp + ccb <= len(d):
                    oid = struct.unpack("<I", f[after:after + 4])[0]
                    jcid = struct.unpack("<I", f[after + 4:after + 8])[0]
                    out.append({"oid": oid, "jcid": jcid, "stp": cstp, "cb": ccb})
            i += size
        nstp, ncb, _ = _ref(f, cb - 20, 0, 0)
        frag(nstp, ncb)

    stp, cb = struct.unpack("<QI", d[172:184])
    frag(stp, cb)
    return out


# rgData に置かれる大きさ（型ごと）。0 は「置かれない」。
FIXED = {0x1: 0, 0x2: 0, 0x3: 1, 0x4: 2, 0x5: 4, 0x6: 8}


def propset(b, i=0):
    """PropertySet を読む。返すのは `{id: 値}`。"""
    if i + 2 > len(b): return {}, i
    n = struct.unpack("<H", b[i:i + 2])[0]; i += 2
    prids = []
    for _ in range(n):
        if i + 4 > len(b): return {}, i
        v = struct.unpack("<I", b[i:i + 4])[0]; i += 4
        prids.append((v & 0x03FFFFFF, (v >> 26) & 0x1F, (v >> 31) & 1))
    out = {}
    for pid, typ, flag in prids:
        if typ in FIXED:
            m = FIXED[typ]
            out[pid] = flag if typ == 0x2 else b[i:i + m]
            i += m
        elif typ == 0x7:                       # 長さ + 中身
            if i + 4 > len(b): break
            ln = struct.unpack("<I", b[i:i + 4])[0]; i += 4
            out[pid] = b[i:i + ln]; i += ln
        elif typ in (0x8, 0xA, 0xC):           # 一つ参照する（rgData には置かれない）
            out[pid] = ("ref", 1)
        elif typ in (0x9, 0xB, 0xD):           # 列（個数だけ置かれる）
            if i + 4 > len(b): break
            cnt = struct.unpack("<I", b[i:i + 4])[0]; i += 4
            out[pid] = ("refs", cnt)
        elif typ == 0x10:                      # 値の列
            if i + 8 > len(b): break
            cnt = struct.unpack("<I", b[i:i + 4])[0]; i += 4
            i += 4                             # prid
            kids = []
            for _ in range(cnt):
                kid, i = propset(b, i)
                kids.append(kid)
            out[pid] = kids
        elif typ == 0x11:                      # 入れ子
            kid, i = propset(b, i)
            out[pid] = kid
        else:
            break
    return out, i


def read_props(d, o):
    """オブジェクトの実体 → プロパティ集合。**頭に参照の並びが入っている。**"""
    b = d[o["stp"]:o["stp"] + o["cb"]]
    if len(b) < 4: return {}
    h = struct.unpack("<I", b[0:4])[0]
    cnt = h & 0x00FFFFFF
    ext, no_osid = (h >> 30) & 1, (h >> 31) & 1
    i = 4 + cnt * 4
    if not no_osid:                            # OSID の並びが続く
        if i + 4 > len(b): return {}
        h2 = struct.unpack("<I", b[i:i + 4])[0]
        c2, ext2 = h2 & 0x00FFFFFF, (h2 >> 30) & 1
        i += 4 + c2 * 4
        if ext2:                               # 文脈の並びも続く
            if i + 4 > len(b): return {}
            c3 = struct.unpack("<I", b[i:i + 4])[0] & 0x00FFFFFF
            i += 4 + c3 * 4
    got, _ = propset(b, i)
    return got


# ── 木を組む ────────────────────────────────────────────────────────
#
# **`.one` は改訂の履歴を丸ごと持つ。** 同じページの古い版が何度も入っている
# ので、素直に歩くと題が何度も出る（見本では `This is impo` →（五回）→
# `Section3HeaderTitle` という書き換えの跡がそのまま並んだ）。
# **空間＝ページ、その最後の改訂が「いま」。**

JC_PAGE, JC_OUTLINE, JC_OE, JC_TEXT, JC_PAGEMETA = 0x000B, 0x000C, 0x000D, 0x000E, 0x0030
P_TITLE, P_LEVEL   = 0x1CF3, 0x1DFF      # CachedTitleString / PageLevel
P_ASCII, P_UNICODE = 0x3498, 0x1C22      # TextExtendedAscii / RichEditTextUnicode
P_INDENT           = 0x1C03              # OutlineElementChildLevel
P_LASTMOD          = 0x1D7A


def spaces(d):
    """**空間ごと・改訂ごと**にオブジェクトを集める。

    返すのは `[(空間, [改訂ごとのオブジェクトの並び])]`（出てきた順）。
    """
    out, seen = {}, set()
    order = []
    ctx = {"os": None, "rev": 0}

    def frag(stp, cb):
        if not stp or (stp, cb) in seen or stp + cb > len(d): return
        seen.add((stp, cb))
        f = d[stp:stp + cb]
        if struct.unpack("<Q", f[0:8])[0] != FRAG: return
        i, end = 16, cb - 20
        while i + 4 <= end:
            h = struct.unpack("<I", f[i:i + 4])[0]
            if h == 0: break
            nid, size = h & 0x3FF, (h >> 10) & 0x1FFF
            s, c, base = (h >> 23) & 3, (h >> 25) & 3, (h >> 27) & 0xF
            if size < 4 or i + size > end: break
            if nid == 0x00C:                        # ObjectSpaceManifestListStart
                ctx["os"] = f[i + 4:i + 24].hex()
                if ctx["os"] not in out:
                    out[ctx["os"]] = {}
                    order.append(ctx["os"])
            elif nid in (0x01B, 0x01E, 0x01F):      # RevisionManifestStart*
                ctx["rev"] += 1
                out.setdefault(ctx["os"], {}).setdefault(ctx["rev"], [])
            elif nid in DECLS and base == 1:
                cstp, ccb, after = _ref(f, i + 4, s, c)
                if cstp and cstp + ccb <= len(d):
                    out.setdefault(ctx["os"], {}).setdefault(ctx["rev"], []).append({
                        "oid": struct.unpack("<I", f[after:after + 4])[0],
                        "jcid": struct.unpack("<I", f[after + 4:after + 8])[0] & 0xFFFF,
                        "stp": cstp, "cb": ccb})
            if base == 2:
                cs, cc, _ = _ref(f, i + 4, s, c)
                frag(cs, cc)
            i += size
        n, nc, _ = _ref(f, cb - 20, 0, 0)
        frag(n, nc)

    stp, cb = struct.unpack("<QI", d[172:184])
    frag(stp, cb)
    return [(k, out[k]) for k in order if out.get(k)]


def text_of(p):
    """本文の字。**`TextExtendedAscii` は 1 バイト文字**（名前のとおり）。

    UTF-16 で読むと `桔獩椠` になる ── 一度それで「本文が取れない」と誤診した。
    """
    for pid, enc in ((P_UNICODE, "utf-16-le"), (P_ASCII, "latin-1")):
        v = p.get(pid)
        if isinstance(v, bytes) and v:
            try:
                return v.decode(enc).replace("\x00", "")
            except Exception:
                continue
    return None


def current(revs):
    """改訂の並びから、**いまの版**を選ぶ。

    `.one` は改訂の履歴を丸ごと持つので、素直に歩くと同じページが何度も出る
    （見本では題が `This is impo` →（五回）→ `Section3HeaderTitle` と
    書き換わった跡がそのまま並んだ）。**最後が「いま」。**
    """
    return revs[max(revs)] if revs else []


def pages(d):
    """`.one` の、**いまのページ**だけを返す。"""
    got = []
    for _os, revs in spaces(d):
        if not revs: continue
        last = current(revs)
        meta = next((o for o in last if o["jcid"] == JC_PAGEMETA), None)
        lines = []
        for o in last:
            if o["jcid"] != JC_TEXT: continue
            p = read_props(d, o)
            t = text_of(p)
            if t is None or not t.strip(): continue
            ind = p.get(P_INDENT)
            lines.append({"text": t, "indent": ind[0] if isinstance(ind, bytes) and ind else 0})
        title, level = None, 1
        if meta:
            mp = read_props(d, meta)
            v = mp.get(P_TITLE)
            if isinstance(v, bytes):
                try: title = v.decode("utf-16-le").replace("\x00", "")
                except Exception: title = None
            lv = mp.get(P_LEVEL)
            if isinstance(lv, bytes) and len(lv) == 4:
                level = struct.unpack("<I", lv)[0] or 1
        if title or lines:
            got.append({"title": title, "level": level, "lines": lines})
    return got
