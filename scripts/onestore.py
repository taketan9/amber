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

# jcid（下位 16bit）── 表は Apache Tika の `JCIDPropertySetTypeEnum.java` から。
# **記憶で当てない**（この一日で二度それで転んだ）。
JC_PAGE, JC_OUTLINE, JC_OE, JC_TEXT, JC_PAGEMETA = 0x000B, 0x000C, 0x000D, 0x000E, 0x0030
JC_IMAGE, JC_NUMLIST, JC_TITLE, JC_FILE = 0x0011, 0x0012, 0x002C, 0x0035
JC_TABLE, JC_ROW, JC_CELL = 0x0022, 0x0023, 0x0024
P_TITLE, P_LEVEL   = 0x1CF3, 0x1DFF      # CachedTitleString / PageLevel
P_ASCII, P_UNICODE = 0x3498, 0x1C22      # TextExtendedAscii / RichEditTextUnicode
P_INDENT           = 0x1C03              # OutlineElementChildLevel
P_LASTMOD          = 0x1D7A

# **装飾**（依頼 597）。表は Apache Tika の `OneNotePropertyEnum.java` から。
P_STYLE_ID   = 0x345A                    # ParagraphStyleId ── "h1".."h6" "p" "cite" "code"
P_STYLE      = 0x342C                    # ParagraphStyle（参照）
P_BOLD, P_ITALIC     = 0x1C04, 0x1C05
P_UNDER, P_STRIKE    = 0x1C06, 0x1C07
P_LIST_NODES = 0x1C26                    # ListNodes（参照の列 ── 在れば箇条書き）
P_NUM_FORMAT = 0x1C1A                    # NumberListFormat（在れば番号）
P_COL_WIDTHS = 0x1D66                    # TableColumnWidths
P_ROWS, P_COLS = 0x1D57, 0x1D58
P_PICTURE    = 0x1C3F                    # PictureContainer（参照）
P_IMG_NAME   = 0x1DD7                    # ImageFilename
P_IMG_ALT    = 0x1E58                    # ImageAltText
P_FILE_NAME  = 0x1D9C                    # EmbeddedFileName
P_TAG_SHAPE  = 0x3464                    # NoteTagShape（チェックの升）
P_IS_TITLE   = 0x1CB4                    # IsTitleText
P_IS_DATE    = 0x1CB5                    # IsTitleDate
P_IS_TIME    = 0x1C87                    # IsTitleTime
P_IS_BOILER  = 0x1C88                    # IsBoilerText
P_X, P_Y     = 0x1C14, 0x1C15            # OffsetFromParentHoriz / Vert
P_CREATED    = 0x1D09                    # CreationTimeStamp


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


def _str(p, pid, enc="utf-16-le"):
    v = p.get(pid)
    if not isinstance(v, bytes) or not v:
        return None
    try:
        return v.decode(enc).replace("\x00", "")
    except Exception:  # noqa
        return None


def _num(p, pid):
    v = p.get(pid)
    if isinstance(v, bytes) and len(v) == 4:
        return struct.unpack("<I", v)[0]
    return None


def line_of(p):
    """一つの本文を、**Markdown の一行**に。

    かたまりの意味（見出し・引用・コード）は `ParagraphStyleId` が持っていて、
    **名前は OneNote の XML と同じ**（`h1`〜`h6` `p` `cite` `code`）── COM の
    道で書いた変換と、同じ言葉で話せる。
    """
    text = text_of(p)
    if text is None:
        return None
    text = text.rstrip("\r\n")
    if not text.strip():
        return None
    # **ページ頭の日付と時刻は、本文ではない。** OneNote が自分で置くもので、
    # 写すと毎ページに `Friday, November 22, 2019` と `6:39 AM` が混ざる。
    if p.get(P_IS_DATE) or p.get(P_IS_TIME) or p.get(P_IS_BOILER):
        return None
    if p.get(P_IS_TITLE):
        return None                       # 題は前書きが持つ
    got = {"text": text, "indent": (p.get(P_INDENT) or b"\x00")[0]
           if isinstance(p.get(P_INDENT), bytes) and p.get(P_INDENT) else 0}
    style = _str(p, P_STYLE_ID, "latin-1") or ""
    got["style"] = style.strip()
    got["bold"] = bool(p.get(P_BOLD))
    got["italic"] = bool(p.get(P_ITALIC))
    got["strike"] = bool(p.get(P_STRIKE))
    got["list"] = "number" if p.get(P_NUM_FORMAT) is not None else (
        "bullet" if p.get(P_LIST_NODES) is not None else None)
    tag = p.get(P_TAG_SHAPE)
    got["todo"] = isinstance(tag, bytes) and len(tag) == 2
    got["y"] = _num(p, P_Y) or 0
    got["x"] = _num(p, P_X) or 0
    return got


def as_markdown(line):
    """一行を Markdown に。**印は外側から。**"""
    body = line["text"].strip()
    if line.get("bold"):
        body = f"**{body}**"
    if line.get("italic"):
        body = f"*{body}*"
    if line.get("strike"):
        body = f"~~{body}~~"
    style = line.get("style") or ""
    pad = "  " * min(line.get("indent", 0), 6)
    if line.get("todo"):
        return f"{pad}- [ ] {body}"
    if line.get("list") == "number":
        return f"{pad}1. {body}"
    if line.get("list") == "bullet":
        return f"{pad}- {body}"
    if style.startswith("h") and style[1:].isdigit():
        return "#" * min(int(style[1:]), 6) + f" {line['text'].strip()}"
    if style == "cite":
        return f"> {body}"
    if style == "code":
        return "```\n" + line["text"] + "\n```"
    return f"{pad}{body}" if pad else body


def pictures(d, objs):
    """画像の実体（`[(名前, バイト列)]`）。**中身はオブジェクトの blob。**"""
    out = []
    for o in objs:
        p = read_props(d, o)
        if P_PICTURE not in p and P_IMG_NAME not in p and P_FILE_NAME not in p:
            continue
        name = _str(p, P_IMG_NAME) or _str(p, P_FILE_NAME)
        raw = d[o["stp"]:o["stp"] + o["cb"]]
        out.append({"name": name, "alt": _str(p, P_IMG_ALT), "bytes": raw, "oid": o["oid"]})
    return out


def tables(d, objs):
    """表を組む ── `[{"rows": [[升の字, …], …], "y": 上からの位置}]`。

    **升の中身は、升の下にぶら下がる本文。** 表・行・升は入れ子で並ぶので、
    出てきた順に「表が始まった／行が始まった」と数えていけば組める ──
    参照をたどらずに済む（`ObjectGroup` は木の順に並ぶ）。

    Markdown の表に改行は入らないので、**升の中の複数行は空白で繋ぐ**
    （COM の道と同じ潰し方 ── `<br>` は ambər の画面に字として出る）。
    """
    out, table, row = [], None, None
    for o in objs:
        jc = o["jcid"]
        if jc == JC_TABLE:
            p = read_props(d, o)
            table = {"rows": [], "y": _num(p, P_Y) or 0, "x": _num(p, P_X) or 0,
                     "cols": _num(p, P_COLS) or 0}
            out.append(table)
            row = None
        elif jc == JC_ROW and table is not None:
            row = []
            table["rows"].append(row)
        elif jc == JC_CELL and row is not None:
            row.append([])
        elif jc == JC_TEXT and row and row[-1] is not None:
            got = line_of(read_props(d, o))
            if got:
                row[-1].append(got["text"].strip())
    for t in out:
        t["rows"] = [[" ".join(c) for c in r] for r in t["rows"] if r]
    return [t for t in out if t["rows"]]


def table_markdown(t):
    """表を Markdown に。**升の数は行ごとに揃える** ── 揃っていない表は崩れる。"""
    rows = t["rows"]
    if not rows:
        return []
    width = max(len(r) for r in rows)
    rows = [[c.replace("|", "\\|") for c in r] + [""] * (width - len(r)) for r in rows]
    # **見出しの行があるとは限らない。** OneNote は旗で持つが、`.one` からは
    # 素直に引けない ── 1 行目を見出しにすると**そのデータが一行消える**ので、
    # 空の見出しを置く（COM の道で同じ形に決めた・依頼 578）。
    out = ["|" + "  |" * width, "|" + " --- |" * width]
    for r in rows:
        out.append("| " + " | ".join(r) + " |")
    return out


def pages(d):
    """`.one` の、**いまのページ**だけを返す。"""
    got = []
    for _os, revs in spaces(d):
        if not revs:
            continue
        last = current(revs)
        # **空間がぜんぶページとは限らない。** セクションそのものの空間が
        # 頭に一つある（題は持つが本文が無い）── そのまま出すと、
        # **中身のないノートがセクションごとに一本できる。**
        # ページの空間には `Page`（0x0B）が入っている。
        if not any(o["jcid"] == JC_PAGE for o in last):
            continue
        meta = next((o for o in last if o["jcid"] == JC_PAGEMETA), None)
        # **表の中の字は、本文に二度出さない。** 升をたどるときに拾うので、
        # ここで拾うと同じ字が表の外にも並ぶ。
        in_table, skip = False, set()
        for o in last:
            if o["jcid"] == JC_TABLE:
                in_table = True
            elif o["jcid"] in (JC_OUTLINE, JC_PAGE) and in_table:
                in_table = False
            elif in_table and o["jcid"] == JC_TEXT:
                skip.add(id(o))
        lines = []
        for o in last:
            if o["jcid"] != JC_TEXT or id(o) in skip:
                continue
            one = line_of(read_props(d, o))
            if one:
                lines.append(one)
        # **ページの上にある順に並べる。** OneNote は箱をどこにでも置けるので、
        # 出てきた順は書いた順ではない ── 上から下、同じ高さなら左から右
        # （COM の道と同じ潰し方）。
        lines.sort(key=lambda l: (l["y"], l["x"]))
        title, level, created = None, 1, None
        if meta:
            mp = read_props(d, meta)
            title = _str(mp, P_TITLE)
            level = _num(mp, P_LEVEL) or 1
            created = _num(mp, P_CREATED)
        if title or lines:
            got.append({"title": title, "level": level, "created": created,
                        "lines": lines, "images": pictures(d, last),
                        "tables": tables(d, last)})
    return got
