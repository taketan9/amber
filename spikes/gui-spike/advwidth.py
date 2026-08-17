import struct, sys

def read_font(path):
    d = open(path, 'rb').read()
    tag = d[:4]
    off = 0
    if tag == b'ttcf':
        off = struct.unpack('>I', d[12:16])[0]
    num = struct.unpack('>H', d[off+4:off+6])[0]
    tables = {}
    for i in range(num):
        p = off + 12 + 16*i
        t = d[p:p+4].decode('latin1')
        o, l = struct.unpack('>II', d[p+8:p+16])
        tables[t] = (o, l)
    return d, tables

def upem(d, t):
    o = t['head'][0]
    return struct.unpack('>H', d[o+18:o+20])[0]

def cmap_lookup(d, t, cps):
    o = t['cmap'][0]
    n = struct.unpack('>H', d[o+2:o+4])[0]
    best = None
    for i in range(n):
        pid, eid, off = struct.unpack('>HHI', d[o+4+8*i:o+4+8*i+8])
        fmt = struct.unpack('>H', d[o+off:o+off+2])[0]
        if fmt in (4, 12):
            best = (fmt, o+off)
    fmt, so = best
    out = {}
    if fmt == 12:
        ngroups = struct.unpack('>I', d[so+12:so+16])[0]
        groups = [struct.unpack('>III', d[so+16+12*g:so+16+12*g+12]) for g in range(ngroups)]
        for cp in cps:
            out[cp] = 0
            for s, e, gi in groups:
                if s <= cp <= e:
                    out[cp] = gi + (cp - s); break
    else:
        segx2 = struct.unpack('>H', d[so+6:so+8])[0]
        seg = segx2 // 2
        ends = struct.unpack('>%dH' % seg, d[so+14:so+14+segx2])
        sp = so+16+segx2
        starts = struct.unpack('>%dH' % seg, d[sp:sp+segx2])
        dp = sp+segx2
        deltas = struct.unpack('>%dh' % seg, d[dp:dp+segx2])
        rp = dp+segx2
        ranges = struct.unpack('>%dH' % seg, d[rp:rp+segx2])
        for cp in cps:
            out[cp] = 0
            if cp > 0xFFFF: continue
            for i in range(seg):
                if starts[i] <= cp <= ends[i]:
                    if ranges[i] == 0:
                        out[cp] = (cp + deltas[i]) & 0xFFFF
                    else:
                        gp = rp + 2*i + ranges[i] + 2*(cp - starts[i])
                        g = struct.unpack('>H', d[gp:gp+2])[0]
                        out[cp] = (g + deltas[i]) & 0xFFFF if g else 0
                    break
    return out

def advances(path, cps):
    d, t = read_font(path)
    em = upem(d, t)
    ho = t['hhea'][0]
    nhm = struct.unpack('>H', d[ho+34:ho+36])[0]
    mo = t['hmtx'][0]
    gmap = cmap_lookup(d, t, cps)
    res = {}
    for cp, gi in gmap.items():
        if gi == 0:
            res[cp] = None; continue
        i = min(gi, nhm-1)
        aw = struct.unpack('>H', d[mo+4*i:mo+4*i+2])[0]
        res[cp] = aw / em
    return res

CPS = {0x6d: "m (ASCII)", 0x3042: "あ (全角)", 0xf07b: " folder", 0xe702: " git",
       0xf408: " github", 0xe7a8: " rust", 0xf15b: " file"}
for path in sys.argv[1:]:
    print(f"\n{path.split('/')[-1]}")
    try:
        r = advances(path, list(CPS))
    except Exception as e:
        print("   read failed:", e); continue
    for cp, name in CPS.items():
        v = r.get(cp)
        print(f"   {name:<14} {'(no glyph)' if v is None else f'{v:.3f} em'}")

def bboxes(path, cps):
    d, t = read_font(path)
    em = upem(d, t)
    if 'glyf' not in t or 'loca' not in t:
        return None
    head = t['head'][0]
    long_loca = struct.unpack('>h', d[head+50:head+52])[0] == 1
    lo, ll = t['loca']
    go = t['glyf'][0]
    gmap = cmap_lookup(d, t, cps)
    out = {}
    for cp, gi in gmap.items():
        if gi == 0:
            out[cp] = None; continue
        if long_loca:
            s, e = struct.unpack('>II', d[lo+4*gi:lo+4*gi+8])
        else:
            a, b = struct.unpack('>HH', d[lo+2*gi:lo+2*gi+4])
            s, e = a*2, b*2
        if e <= s:
            out[cp] = (0.0, 0.0); continue
        p = go + s
        xmin, xmax = struct.unpack('>hh', d[p+2:p+4] + d[p+6:p+8])
        out[cp] = (xmin/em, xmax/em)
    return out

if len(sys.argv) > 1:
    print("\n=== ink bounding box (xMin..xMax, em) vs advance ===")
    for path in sys.argv[1:]:
        b = bboxes(path, list(CPS))
        a = advances(path, list(CPS))
        print(f"\n{path.split('/')[-1]}")
        if b is None:
            print("   no glyf table (CFF/OTF)"); continue
        for cp, name in CPS.items():
            if b.get(cp) is None: continue
            xmin, xmax = b[cp]
            adv = a[cp]
            over = "  ← 食み出し" if xmax > adv + 0.001 else ""
            print(f"   {name:<14} ink {xmin:+.3f}..{xmax:.3f}  advance {adv:.3f}{over}")
