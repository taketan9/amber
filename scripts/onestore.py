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


def propset(b, i=0, oids=None):
    """PropertySet を読む。返すのは `{id: 値}`。

    **参照は、頭の並びから順に配られる**（依頼 608）。`rgData` には
    「何個ぶん」しか置かれず、指し先（OID）は property set の頭の並びに
    まとまって入っている ── 前の版はそれを読み飛ばしていたので、
    参照を持つプロパティは全部 `("ref", 1)` という**中身の無い札**だった。
    `oids` を渡すと、出てくる順に取り出して指し先そのものを返す。
    """
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
            out[pid] = ("ref", oids.pop(0) if oids else None)
        elif typ in (0x9, 0xB, 0xD):           # 列（個数だけ置かれる）
            if i + 4 > len(b): break
            cnt = struct.unpack("<I", b[i:i + 4])[0]; i += 4
            take = [oids.pop(0) for _ in range(min(cnt, len(oids)))] if oids else []
            out[pid] = ("refs", take if take else cnt)
        elif typ == 0x10:                      # 値の列
            if i + 8 > len(b): break
            cnt = struct.unpack("<I", b[i:i + 4])[0]; i += 4
            i += 4                             # prid
            kids = []
            for _ in range(cnt):
                kid, i = propset(b, i, oids)
                kids.append(kid)
            out[pid] = kids
        elif typ == 0x11:                      # 入れ子
            kid, i = propset(b, i, oids)
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
    # **指し先の並びは、読み飛ばさずに持っておく**（依頼 608）。
    oids = [struct.unpack("<I", b[4 + k * 4:8 + k * 4])[0]
            for k in range(cnt) if 8 + k * 4 <= len(b)]
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
    got, _ = propset(b, i, oids)
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
# **表は参照でたどる**（依頼 620）。表 → 行 → 升 → 中身は、どれも
# `ElementChildNodes`（参照の列）で繋がっている ── 正本は Tika の
# `OneNotePropertyEnum`（`ElementChildNodesOfTable(0x24001C20)`・型 9）。
P_KIDS       = 0x1C20                    # ElementChildNodes（参照の列）
P_ROW_COUNT  = 0x1D57                    # RowCount
P_COL_COUNT  = 0x1D58                    # ColumnCount
# **色とリンク**（依頼 608）。表は Apache Tika の `OneNotePropertyEnum.java` から。
P_COLOR      = 0x1C0C                    # FontColor
P_HIGHLIGHT  = 0x1C0D                    # Highlight（蛍光ペン）
P_LINK       = 0x1E14                    # Hyperlink（旗）
P_LINK_URL   = 0x1E20                    # WzHyperlinkUrl
P_ROWS, P_COLS = 0x1D57, 0x1D58
P_PICTURE    = 0x1C3F                    # PictureContainer（参照）
P_FILE_BLOB  = 0x1D9B                    # EmbeddedFileContainer（参照）
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


def merged(revs):
    """改訂を**古い順に重ね**、OID ごとに新しいほうを採る。

    **最後の改訂だけでは足りないことがある**（依頼 617）。改訂は「変えた
    ところ」しか持たないことがあり、直していない本文も、ページそのものの
    札（`Page`）も、前の改訂に置きっぱなしになる ── そうなると
    `current` は**題も本文も無いページ**を返し、写しても中身が入らない。

    かといって素直に全部並べると、同じ題が何度も出る（前にそれで転んだ）。
    だから **OID で重ねる** ── 同じものの新しい版が、古い版の居た場所に
    座る。Python の dict は入れた順を覚えていて、入れ直しても順は動かない
    ので、**文書の上の順はいちばん古い版のまま**になる。
    """
    got = {}
    for n in sorted(revs):
        for o in revs[n]:
            got[o["oid"]] = o
    return list(got.values())


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


def _color(v):
    """`COLORREF` を `#rrggbb` に。**自動なら色を付けない。**

    MS-ONE の決まり ── 4 バイトのうち**最後が `0xFF` なら「自動」**（前の
    三つは `0x00`）。「自動」は「黒を指定した」ではなく「指定していない」
    なので、そこに色を書くと**ノートの全部の行が span に包まれる。**
    最後が `0x00` のときだけ、前の三つが赤・緑・青。
    """
    if not isinstance(v, (bytes, bytearray)) or len(v) < 4:
        return None
    if v[3] != 0x00:
        return None
    return "#%02x%02x%02x" % (v[0], v[1], v[2])


def style_of(d, by_oid, p):
    """本文の段落 → その書式（`ParagraphStyle`）。**旗はこちらに載っている。**

    太字も斜体も取り消し線も色も、本文のオブジェクトではなく**書式のほう**が
    持っている（依頼 608 で分かった ── それまで旗を本文から読んでいたので、
    **装飾は一度も落ちていなかった**）。
    """
    ref = p.get(P_STYLE)
    if not (isinstance(ref, tuple) and ref[0] == "ref" and ref[1]):
        return {}
    o = by_oid.get(ref[1])
    return read_props(d, o) if o else {}


def line_of(p, style=None):
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
    # **`style` は引数の名前**（書式のプロパティ集合）── ここで上書きしない。
    # 一度やって、旗が一つも立たないのに検査は通る形を作った。
    sid = _str(p, P_STYLE_ID, "latin-1") or ""
    got["style"] = sid.strip()
    # **旗は書式のほうが持っている**（依頼 608）。本文側も見るのは、
    # 走査が本文だけを渡してくる形を残すため。
    f = {**p, **(style or {})}
    got["bold"] = bool(f.get(P_BOLD))
    got["italic"] = bool(f.get(P_ITALIC))
    got["strike"] = bool(f.get(P_STRIKE))
    got["color"] = _color(f.get(P_COLOR))
    got["link"] = _str(f, P_LINK_URL)
    got["list"] = "number" if p.get(P_NUM_FORMAT) is not None else (
        "bullet" if p.get(P_LIST_NODES) is not None else None)
    tag = p.get(P_TAG_SHAPE)
    got["todo"] = isinstance(tag, bytes) and len(tag) == 2
    got["y"] = _num(p, P_Y) or 0
    got["x"] = _num(p, P_X) or 0
    return got


def linked(body, line):
    """リンクを巻く。**いちばん内側**（依頼 608）── `**[字](url)**` の順。

    外に出すと `[**字**](url)` になり、太字の印がリンクの中に入る。
    """
    url = line.get("link")
    return f"[{body}]({url})" if url else body


def colored(body, line):
    """色を巻く。**いちばん外側**（ambər の書き方 ── `note::first_color`）。

    印の中に入れると `**<span…>字</span>**` になり、色の札が印に挟まれる。
    """
    c = line.get("color")
    return f'<span style="color:{c}">{body}</span>' if c else body


def as_markdown(line):
    """一行を Markdown に。**印は外側から。**"""
    body = linked(line["text"].strip(), line)
    if line.get("bold"):
        body = f"**{body}**"
    if line.get("italic"):
        body = f"*{body}*"
    if line.get("strike"):
        body = f"~~{body}~~"
    body = colored(body, line)
    style = line.get("style") or ""
    pad = "  " * min(line.get("indent", 0), 6)
    if line.get("todo"):
        return f"{pad}- [ ] {body}"
    if line.get("list") == "number":
        return f"{pad}1. {body}"
    if line.get("list") == "bullet":
        return f"{pad}- {body}"
    if style.startswith("h") and style[1:].isdigit():
        # 見出しは印を重ねない（`# **字**` は二重）が、色とリンクは残す。
        head = colored(linked(line["text"].strip(), line), line)
        return "#" * min(int(style[1:]), 6) + f" {head}"
    if style == "cite":
        return f"> {body}"
    if style == "code":
        return "```\n" + line["text"] + "\n```"
    return f"{pad}{body}" if pad else body


# 画像の頭の数バイト（**拡張子は、名前ではなく中身で決める**）。
MAGIC = (
    (b"\x89PNG\r\n\x1a\n", "png"),
    (b"\xff\xd8\xff", "jpg"),
    (b"GIF87a", "gif"), (b"GIF89a", "gif"),
    (b"BM", "bmp"),
    (b"II*\x00", "tif"), (b"MM\x00*", "tif"),
    (b"RIFF", "webp"),                       # 実際は 8 バイト目から WEBP
)


def kind_of(raw):
    """バイト列の頭から、絵の種類。分からなければ `None`。"""
    if not raw:
        return None
    for head, ext in MAGIC:
        if raw.startswith(head):
            if ext == "webp" and raw[8:12] != b"WEBP":
                continue
            return ext
    return None


def _blob(d, o):
    """`FileDataStoreObject` の中身を取り出す（依頼 620）。

    **実体には頭が付いていることがある** ── MS-ONESTORE の
    `FileDataStoreObject` は `GUID(16) + 長さ(8) + 未使用(4) + 予備(8)` の
    36 バイトを被せてから中身を置く（Apache Tika の
    `deserializeFileDataStoreObject` と同じ形）。被せたまま書き出すと、
    **PNG のつもりのファイルが 36 バイトずれて開けない。**

    どちらの形でも通るように、**中身が絵として名乗るほうを採る。**
    """
    raw = d[o["stp"]:o["stp"] + o["cb"]]
    if kind_of(raw):
        return raw
    if len(raw) > 36:
        try:
            ln = struct.unpack("<Q", raw[16:24])[0]
        except struct.error:
            ln = 0
        if 0 < ln <= len(raw) - 36:
            inner = raw[36:36 + ln]
            if kind_of(inner):
                return inner
    return raw


def pictures(d, objs):
    """画像の実体（`[{名前, バイト列}]`）。

    **中身は、指し先のオブジェクトにある**（依頼 620）。`PictureContainer`
    も `EmbeddedFileContainer` も**参照**（型 8）── 正本は Tika の
    `OneNotePropertyEnum`（`PictureContainer(0x20001C3F)`）。
    前はここで**画像オブジェクト自身のバイト列**を書き出していたので、
    出てくる `.png` はプロパティ集合の生バイトで、**一枚も開けなかった**
    （現場で「絵や図が出力されていない」と出た顔）。
    """
    look = by_oid(objs)
    out = []
    for o in objs:
        p = read_props(d, o)
        ref = p.get(P_PICTURE) or p.get(P_FILE_BLOB)
        if not (isinstance(ref, tuple) and ref[0] == "ref" and ref[1]):
            continue
        blob = look.get(ref[1])
        if blob is None:
            continue
        raw = _blob(d, blob)
        if not raw:
            continue
        out.append({"name": _str(p, P_IMG_NAME) or _str(p, P_FILE_NAME),
                    "alt": _str(p, P_IMG_ALT), "bytes": raw,
                    "kind": kind_of(raw), "oid": o["oid"]})
    return out


def by_oid(objs):
    """OID から実体を引く一枚（参照をたどるのに要る・依頼 608）。"""
    return {o["oid"]: o for o in objs}


def kids_of(p):
    """`ElementChildNodes` の指し先（無ければ空）。"""
    v = p.get(P_KIDS)
    if isinstance(v, tuple) and v[0] == "refs" and isinstance(v[1], list):
        return [x for x in v[1] if x]
    return []


def _under(d, look, oid, seen):
    """そのオブジェクトの下にある**本文の字**を、順に集める。

    升の中は `升 → アウトライン要素 → 本文` と下がるので、**字に当たるまで
    降りる。** 輪になっている指し先で回らないように、通った先は憶える。
    """
    o = look.get(oid)
    if o is None or id(o) in seen:
        return []
    seen.add(id(o))
    p = read_props(d, o)
    if o["jcid"] == JC_TEXT:
        one = line_of(p, style_of(d, look, p))
        return [one["text"].strip()] if one and one["text"].strip() else []
    got = []
    for k in kids_of(p):
        got += _under(d, look, k, seen)
    return got


def table_texts(d, objs):
    """**表の中に居る本文**の集まり（`id()` で持つ）。

    本文に二度出さないための一枚 ── 升をたどるときに拾うので、ここで
    拾うと同じ字が表の外にも並ぶ。
    """
    look = by_oid(objs)
    inside = set()

    def walk(oid, seen):
        o = look.get(oid)
        if o is None or id(o) in seen:
            return
        seen.add(id(o))
        inside.add(id(o))
        for k in kids_of(read_props(d, o)):
            walk(k, seen)

    for o in objs:
        if o["jcid"] != JC_TABLE:
            continue
        for k in kids_of(read_props(d, o)):
            walk(k, set())
    return inside


def tables(d, objs):
    """表を組む ── `[{"rows": [[升の字, …], …], "y": 上からの位置}]`。

    **指し先でたどる**（依頼 620）。表 → 行 → 升 → 中身は
    `ElementChildNodes`（参照の列）で繋がっている ── 正本は Tika の
    `OneNotePropertyEnum`（`ElementChildNodesOfTable(0x24001C20)`）。

    前は**並んでいる順**で「表が始まった／行が始まった」と数えていた。
    作り物の見本では通るが、**本物は改訂をまたぐと順が入れ替わる** ──
    現場で「表もぐちゃぐちゃ」と出た顔がこれ。

    Markdown の表に改行は入らないので、**升の中の複数行は空白で繋ぐ**
    （`<br>` は ambər の画面に字として出る）。

    **指し先の無い表は、並び順で拾い直す** ── 古い OneNote が書いた
    `.one` でそうなることがある。出ないよりは、順で組んだほうがまし。
    """
    look = by_oid(objs)
    out = []
    for o in objs:
        if o["jcid"] != JC_TABLE:
            continue
        p = read_props(d, o)
        rows = []
        for r_oid in kids_of(p):
            r = look.get(r_oid)
            if r is None or r["jcid"] != JC_ROW:
                continue
            cells = []
            for c_oid in kids_of(read_props(d, r)):
                c = look.get(c_oid)
                if c is None or c["jcid"] != JC_CELL:
                    continue
                got = []
                for k in kids_of(read_props(d, c)):
                    got += _under(d, look, k, set())
                cells.append(" ".join(got))
            if cells:
                rows.append(cells)
        out.append({"rows": rows, "y": _num(p, P_Y) or 0, "x": _num(p, P_X) or 0,
                    "cols": _num(p, P_COL_COUNT) or _num(p, P_COLS) or 0})
    if any(t["rows"] for t in out):
        return [t for t in out if t["rows"]]
    return _tables_in_order(d, objs) or [t for t in out if t["rows"]]


def _tables_in_order(d, objs):
    """指し先の無い表を、**並んでいる順**で組む（古い形の逃げ道）。"""
    look = by_oid(objs)
    out, table, row = [], None, None
    for o in objs:
        jc = o["jcid"]
        if jc == JC_TABLE:
            p = read_props(d, o)
            table = {"rows": [], "y": _num(p, P_Y) or 0, "x": _num(p, P_X) or 0,
                     "cols": _num(p, P_COL_COUNT) or _num(p, P_COLS) or 0}
            out.append(table)
            row = None
        elif jc == JC_ROW and table is not None:
            row = []
            table["rows"].append(row)
        elif jc == JC_CELL and row is not None:
            row.append([])
        elif jc == JC_TEXT and row and row[-1] is not None:
            pr = read_props(d, o)
            got = line_of(pr, style_of(d, look, pr))
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


def _content(d, objs):
    """オブジェクトの並びから、**一枚ぶんの中身**を組む。"""
    # **表の中の字は、本文に二度出さない。** 升をたどるときに拾うので、
    # ここで拾うと同じ字が表の外にも並ぶ。
    #
    # **どれが表の中かは、指し先でたどる**（依頼 620）── 前は「表が出たら
    # 次のアウトラインまで」と並び順で数えていたので、順が入れ替わると
    # 表の外の字まで消したり、表の字が二度出たりした。
    look = by_oid(objs)
    skip = table_texts(d, objs)
    if not skip:
        # 指し先の無い表（古い形）── そのときだけ、並び順で数える。
        in_table = False
        for o in objs:
            if o["jcid"] == JC_TABLE:
                in_table = True
            elif o["jcid"] in (JC_OUTLINE, JC_PAGE) and in_table:
                in_table = False
            elif in_table and o["jcid"] == JC_TEXT:
                skip.add(id(o))
    lines = []
    for o in objs:
        if o["jcid"] != JC_TEXT or id(o) in skip:
            continue
        pr = read_props(d, o)
        one = line_of(pr, style_of(d, look, pr))
        if one:
            lines.append(one)
    # **ページの上にある順に並べる。** OneNote は箱をどこにでも置けるので、
    # 出てきた順は書いた順ではない ── 上から下、同じ高さなら左から右
    # （COM の道と同じ潰し方）。
    lines.sort(key=lambda l: (l["y"], l["x"]))
    # **同じ場所の同じ字は一つ。** 改訂を重ねて拾ったときに、同じ行が
    # 二つ並ぶことがある（依頼 617）。位置まで同じなら、それは同じ行。
    seen, uniq = set(), []
    for l in lines:
        key = (l["text"], l["y"], l["x"])
        if key in seen:
            continue
        seen.add(key)
        uniq.append(l)
    # **前書きは、いちばん新しいものを採る。** 題は書き換わるので、
    # 古い改訂のほうを拾うと前の題で書き出される。
    meta = None
    for o in objs:
        if o["jcid"] == JC_PAGEMETA:
            meta = o
    title, level, created = None, 1, None
    if meta:
        mp = read_props(d, meta)
        title = _str(mp, P_TITLE)
        level = _num(mp, P_LEVEL) or 1
        created = _num(mp, P_CREATED)
    return {"title": title, "level": level, "created": created, "lines": uniq,
            "images": pictures(d, objs), "tables": tables(d, objs)}


def _has_body(pg):
    """**中身があるか。** 題は数えない ── 前書きだけのノートは、空と同じ。"""
    return bool(pg["lines"] or pg["tables"] or pg["images"])


def _empty(pg):
    """**空っぽの一枚か。** 題も本文も表も絵も無ければ、写しても何も残らない。"""
    return not (_has_body(pg) or (pg["title"] or "").strip())


def pages(d):
    """`.one` の、**いまのページ**だけを返す。"""
    got = []
    for _os, revs in spaces(d):
        if not revs:
            continue
        # **空間がぜんぶページとは限らない。** セクションそのものの空間が
        # 頭に一つある（題は持つが本文が無い）── そのまま出すと、
        # **中身のないノートがセクションごとに一本できる。**
        # ページの空間には `Page`（0x0B）が入っている。
        #
        # **その札が最後の改訂にあるとは限らない**（依頼 617）── 直していない
        # ものは前の改訂に置きっぱなしになるので、最後だけを見ていると
        # **ページまるごと取りこぼす。** 見分けは改訂ぜんぶを重ねたほうで。
        whole = merged(revs)
        if not any(o["jcid"] == JC_PAGE for o in whole):
            continue
        pg = _content(d, current(revs))
        if not _has_body(pg):
            # **最後の改訂は「変えたところ」しか持っていなかった。** 本文を
            # 直していなければ、字は前の改訂に置きっぱなしになる ── 題だけが
            # 新しい改訂にあると、**題しか入っていないノート**が出来上がる
            # （現場で「中身もほぼほぼ入っていない」と出た顔）。重ねて拾い直す。
            deep = _content(d, whole)
            if _has_body(deep):
                pg = deep
        if not _empty(pg):
            got.append(pg)
    return got
