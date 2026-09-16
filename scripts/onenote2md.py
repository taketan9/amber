#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
onenote2md.py ── OneNote が書き出したものを Markdown に（ambər の保存ディレクトリ向け）

**押すのは一回。** `onenote2md.bat` をダブルクリックすると小さい窓が出て、
取り込むものと出力先を選んで押すだけ（`.onepkg` をバッチに放り込めば窓も出ない）。

  py -3 scripts\\onenote2md.py                         窓を出す
  py -3 scripts\\onenote2md.py --out <出力先> <.onepkg>   コマンドで
  py -3 scripts\\onenote2md.py --peek <フォルダ>          形式を数えるだけ

ノートブック / セクショングループ / セクション / ページ の階層を、そのまま
フォルダ構成にして書き出す。**ambər の保存ディレクトリとして、そのまま読める形**:

    <out>/
      <ノートブック>/
        <セクショングループ>/ … 任意の深さ
          <セクション>/
            attachments/        … ambər の画像の置き場所（ノートの隣・同じ名前）
            <ページ>.md
            <ページ>/           … サブページはページ名のフォルダに入る
              <サブページ>.md

要るもの ── **Python だけ。**
  Office も pywin32 も 32 bit の Python も要らない。網にも出ない。
  mac でも Linux でも動く（`.one` はただのバイナリで、読むのはこちら側）。

  **前は OneNote に直接繋いでいた**（COM）。本人の会社の端末ではどうやっても
  繋がらず、2026-09-16 にその道ごと外した ── 経緯と外した 23 件の依頼は
  `REQUESTS.withdrawn.ja.md` に、拾い直すための場所は `git log` にある。

取り込むもの
  OneNote の ファイル → エクスポート → **ノートブック** → `.onepkg`。
  （まとめて出せるのは `.pdf` `.xps` `.onepkg` だけ。押すのは一回。）
  `.one` を直に指してもよいし、それが入ったフォルダでもよい。
  **`.onepkg` の名前が、そのままノートブックのフォルダ名になる。**

  読めるのは**公開仕様の `.one`**（`109ADD3F…`）。SharePoint に置かれている
  ものは `638DE92F…` という別ものだが、**書き出せば公開仕様で出てくる。**
  どちらかは `--peek` が数えて言う。

繰り返し走らせるための決まり
  1. **同じページは同じファイルに書く。** 同じ親フォルダの同じ名前 → 同じ
     ファイル。題がぶつかったら `名前 (2).md` にずらす（上書きすると字が消える）。
  2. **改行は LF。** Windows の `write_text` は CRLF にするので `newline="\\n"`。
  3. **名前は SharePoint / WebDAV でも通るものに。** `\\ / : * ? " < > |` に加えて
     `# % & ~ { }` と先頭の `_vti_`、末尾の `.` と空白を避ける。一段 120 字まで、
     道ぜんたいで 200 字を超えそうなら詰める（WebDAV の 256 字の壁）。
  4. **画像は `attachments/`**（ambər の決まり）。ノートの隣のフォルダで、名前は
     `<ページ名>-001.png`（**ハイフン** ── `note::attach` と同じ形で、ambər は
     「幹 + ハイフン」で自分の画像を見分ける）。幹は 60 字（`note::file_stem`）。
  5. **一度に一本だけ**（OS の鎖）。本数が多いと数分かかるので、終わる前に
     次の回が始まると二本が同じファイルを奪い合う。
  6. **`--log`。** 窓を出さずに回すと画面には誰もいない。落ちたことが残らなければ、
     落ちていないのと見分けがつかない。
  7. **片道。** ambər 側で直したページは、次に取り込んだとき上書きされる。

  何が Markdown に落ちるかは docs/onenote.ja.md。走査は scripts/onenote-test.py、
  その検査を壊して確かめるのが scripts/onenote-mutate.sh。
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import importlib
import logging
import os
import re
import struct
import sys
import tempfile
import zlib
from datetime import datetime, timedelta
from pathlib import Path

XS_2013 = 2           # XMLSchema.xs2013
ATTACH = "attachments"   # ambər の画像の置き場所（ノートの隣・この名前）
PATH_LIMIT = 200         # WebDAV の道の長さの壁（256）に余裕を見た数

log = logging.getLogger("onenote2md")


def use_utf8():
    """画面に出す字を UTF-8 で。

    日本語 Windows の既定は cp932 で、**画面に直に出すぶんには平気だが、
    ファイルへ向けた瞬間に cp932 になる。** `--probe` が失敗した行に付ける
    `✗` も、`--help` の `ambər` も cp932 に無いので、そこで
    `UnicodeEncodeError` が出て途中で止まる ── **いちばん知りたい行で。**
    `--probe` は繋がらない端末の姿を貼ってもらう道具なので、ファイルに
    落とせないと使えない。`pythonw.exe` には stdout が無いので、あるときだけ。"""
    for stream in (sys.stdout, sys.stderr):
        if not hasattr(stream, "reconfigure"):
            continue
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (ValueError, OSError):
            try:
                stream.reconfigure(errors="replace")   # せめて落ちないように
            except (ValueError, OSError):
                pass


# ---------------------------------------------------------------------------
# COM
# ---------------------------------------------------------------------------
























# `.one` の形式。**二つある。**
ONE_FORMATS = {
    "109add3f-911b-49f5-a5d0-1791edc8aed8": ("公開仕様（MS-ONESTORE）", True),
    "638de92f-a6d4-4bc1-9a36-b3fc2511a5b7": ("未公開の別形式（Office 365 由来）", False),
}


def one_format(path):
    """`.one` の形式 GUID を読む ── **先頭 64 バイトだけ。**

    中身は見ない。見るのは「どちらの形式か」だけで、それは 48 バイト目からの
    16 バイトに書いてある。**会社のノートを外へ出さずに決められる。**
    """
    try:
        with open(path, "rb") as f:
            head = f.read(64)
    except OSError as e:
        return None, f"読めない（{e.strerror or e}）"
    if len(head) < 64:
        return None, "短すぎる"
    import uuid
    got = str(uuid.UUID(bytes_le=head[48:64])).lower()
    name, _ok = ONE_FORMATS.get(got, ("知らない形式", False))
    return got, name


# ---------------------------------------------------------------------------
# CAB ── `.onepkg` の中身。**`expand` に渡さず、自分でほどく。**
# ---------------------------------------------------------------------------
#
# 前の版は Windows の `expand` に開かせて、名前だけ CAB の目録から読んでいた。
# 二つ壊れていた ──
#
#   * **ファイルの数を読む場所が一つずれていた。** CAB の頭は 26 バイト目が
#     `cFolders`、28 バイト目が `cFiles`。26 を数えていたので、目録は
#     **フォルダの数だけ**しか読まれず、セクションが数本しか出てこない。
#     走査は同じずれで偽物の CAB を組んでいたので、**検査も一緒に間違えていた**
#     （偽物が本物より甘い、七度目 ── 依頼 617）。
#   * **`expand` は日本語の名前を壊す。** 出したファイルを大きさで目録と
#     突き合わせていたが、同じ大きさが二つあれば取り違える。
#
# いまは頭から目録から中身まで全部こちらで読む。**名前は目録のまま**、
# フォルダの区切り（`\`）もそのまま活かすので、セクショングループが潰れない。
# mac でも Linux でも開ける ── だから走査で本物の CAB を組んで確かめられる。

_A_NAME_IS_UTF = 0x80            # CFFILE.attribs ── 名前が UTF-8
_HDR_PREV, _HDR_NEXT, _HDR_RESERVE = 0x0001, 0x0002, 0x0004
COMP_NAMES = {0: "無圧縮", 1: "MSZIP", 2: "Quantum", 3: "LZX"}


class CabUnsupported(Exception):
    """ほどき方を知らない圧縮（Quantum / LZX）。**`expand` に回す合図。**"""


def _cstr(d, i):
    """NUL で終わる並びを一つ取る ── `(中身, 次の位置)`。"""
    j = d.find(b"\0", i)
    if j < 0:
        return b"", len(d)
    return d[i:j], j + 1


def cab_name(raw, attribs):
    """目録の名前を字にする。**UTF-8 を先に試す。**

    仕様は「`_A_NAME_IS_UTF`（0x80）が立っていれば UTF-8、でなければ機械の
    符号」だが、**OneNote は旗を立てずに UTF-8 で書く**（本人の端末で出た ──
    セクションの名前だけが読めない漢字に化け、数字はそのまま出ていた。これは
    UTF-8 の並びを cp932 で読んだときの顔）。

    だから旗に頼らず、**UTF-8 で読めるなら UTF-8** とする。UTF-8 は自分で
    自分の正しさを言える符号で、cp932 の日本語（`打` = 91 C5 のように、
    継続バイトから始まる並び）はまず通らない ── 取り違えない。
    """
    for enc in (("utf-8",) if attribs & _A_NAME_IS_UTF else ("utf-8", "cp932", "cp1252")):
        try:
            return raw.decode(enc)
        except UnicodeDecodeError:
            continue
    return raw.decode("latin-1")


def cab_read(d):
    """CAB の頭・フォルダ・目録を読む ── `(中身, わけ)`。

    返す `files` の `name` は**入れ物の中の道**（`400_打合せ/水曜日打合せ.one`）。
    """
    if len(d) < 36 or d[:4] != b"MSCF":
        return None, "CAB ではない"
    coff_files = struct.unpack("<I", d[16:20])[0]
    # **26 は `cFolders`、28 が `cFiles`。** ここを取り違えると、
    # 目録がフォルダの数だけで切れる（依頼 617 で踏んだ）。
    n_folders, n_files, flags = struct.unpack("<HHH", d[26:32])
    i, cb_folder, cb_data = 36, 0, 0
    if flags & _HDR_RESERVE:
        if len(d) < 40:
            return None, "頭が短い"
        cb_header, cb_folder, cb_data = struct.unpack("<HBB", d[36:40])
        i = 40 + cb_header
    for flag in (_HDR_PREV, _HDR_NEXT):
        if flags & flag:
            _, i = _cstr(d, i)
            _, i = _cstr(d, i)
    folders = []
    for _ in range(n_folders):
        if i + 8 > len(d):
            break
        off, blocks, comp = struct.unpack("<IHH", d[i:i + 8])
        i += 8 + cb_folder
        folders.append({"off": off, "blocks": blocks,
                        "comp": comp & 0x000F, "window": (comp >> 8) & 0x1F})
    files, j = [], coff_files
    for _ in range(n_files):
        if j + 16 > len(d):
            break
        size, off, folder, _da, _ti, attribs = struct.unpack("<IIHHHH", d[j:j + 16])
        raw, j = _cstr(d, j + 16)
        if not raw:
            break
        files.append({"name": cab_name(raw, attribs).replace(chr(92), "/"),
                      "size": size, "off": off, "folder": folder})
    return {"folders": folders, "files": files, "cb_data": cb_data}, None


def cab_names(at):
    """CAB の**目録**を読む ── `[(道, 大きさ)]`（入っている順）。

    中身は開かない。**名前と大きさだけ**（`--peek` と突き合わせに使う）。
    読めなければ空を返す ── **分からないことを分かったように言わない。**
    """
    try:
        d = at.read_bytes() if hasattr(at, "read_bytes") else open(at, "rb").read()
    except OSError:
        return []
    got, _why = cab_read(d)
    if got is None:
        return []
    return [(f["name"], f["size"]) for f in got["files"]]


def _folder_stream(d, folder, cb_data, sink):
    """フォルダ一つぶんの中身を、ほどいて `sink` に流す。

    **MSZIP は塊ごとの deflate**（頭に `CK`）で、**前の塊の末尾 32KB を辞書に
    使う** ── 塊ごとに独立に開くと、二つ目から化ける。
    """
    comp = folder["comp"]
    if comp not in (0, 1):
        raise CabUnsupported(COMP_NAMES.get(comp, str(comp)))
    i, history = folder["off"], b""
    for _ in range(folder["blocks"]):
        if i + 8 > len(d):
            raise ValueError("塊が途中で切れている")
        cb, cbu = struct.unpack("<HH", d[i + 4:i + 8])
        i += 8 + cb_data
        blob = d[i:i + cb]
        i += cb
        if comp == 0:
            out = blob
        else:
            if blob[:2] != b"CK":
                raise ValueError("MSZIP の印（CK）が無い")
            z = zlib.decompressobj(-15, zdict=history)
            out = z.decompress(blob[2:]) + z.flush()
        if cbu and len(out) != cbu:
            raise ValueError(f"ほどいた大きさが合わない（{len(out)} ≠ {cbu}）")
        sink(out)
        history = (history + out)[-32768:]


def _safe_parts(name):
    """入れ物の中の道を、**外へ出られない形**に。

    `..` と絶対の道は落とす ── 入れ物が作った道をそのまま信じて書くと、
    出力先の外に書ける（`..\\..\\` を入れた CAB を渡されたとき）。
    """
    parts = []
    for x in name.replace(chr(92), "/").split("/"):
        x = x.strip()
        if not x or x in (".", ".."):
            continue
        if len(x) > 1 and x[1] == ":":        # C:\... は道ではなく名前として扱う
            x = x.replace(":", "_")
        parts.append(x)
    return parts


def _expand_fallback(at, into, listed):
    """ほどき方を知らない圧縮は、**Windows の `expand` に回す。**

    `expand` は日本語の名前を壊すので、出てきたものは**大きさで目録と
    突き合わせて**、正しい名前に置き直す（同じ大きさが複数あれば、出てきた
    順に配る ── それでも名前が化けたままよりはよい）。
    """
    import subprocess
    raw = Path(into) / "_expand"
    raw.mkdir(parents=True, exist_ok=True)
    try:
        got = subprocess.run(["expand", "-F:*", str(at), str(raw)],
                             capture_output=True, timeout=1800)
    except FileNotFoundError:
        return None, "この圧縮はこちらでほどけず、expand も居ません（Windows で実行してください）"
    except subprocess.TimeoutExpired:
        return None, "expand が返ってこない"
    if got.returncode != 0:
        return None, f"expand が転んだ（{got.returncode}）"
    on_disk = sorted(raw.rglob("*"), key=lambda q: (q.stat().st_size if q.is_file() else 0))
    left = [q for q in on_disk if q.is_file()]
    out = []
    for f in listed:
        hit = next((q for q in left if q.stat().st_size == f["size"]), None)
        if hit is None:
            continue
        left.remove(hit)
        parts = _safe_parts(f["name"])
        if not parts:
            continue
        dest = Path(into).joinpath(*parts)
        dest.parent.mkdir(parents=True, exist_ok=True)
        hit.replace(dest)
        out.append((f["name"], dest))
    return out, None


def unpack_onepkg(at, into):
    """`.onepkg` を開く ── `([(入れ物の中の道, 出したファイル), …], わけ)`。

    OneNote が「ノートブック全体は `.pdf` `.xps` `.onepkg` だけ」と言うので、
    まとめて出すとこの形になる。中身は CAB。
    """
    at = Path(at)
    into = Path(into)
    into.mkdir(parents=True, exist_ok=True)
    try:
        d = at.read_bytes()
    except OSError as e:
        return None, f"読めない（{e.strerror or e}）"
    got, why = cab_read(d)
    if got is None:
        return None, why
    if not got["files"]:
        return None, "目録が空（壊れているか、CAB ではない）"
    comps = sorted({f["comp"] for f in got["folders"]})
    log.debug("CAB: フォルダ %d / ファイル %d / 圧縮 %s", len(got["folders"]),
              len(got["files"]), "・".join(COMP_NAMES.get(c, str(c)) for c in comps))
    # **フォルダごとにほどいて、ほどいた端から切り出す。**
    #
    # ファイルは「フォルダの中の位置」で置かれているので、まず一続きに戻す。
    # ノートブック一冊で数百 MB になるから**丸ごと持たない** ── 一時ファイルへ
    # 流して、その中を seek で切り出し、**そのフォルダを配り終えたら捨てる。**
    # 全部ほどいてから配ると、置き場所が一時と出力先で二重に要る。
    out = []
    by_folder = {}
    for k, f in enumerate(got["files"]):
        by_folder.setdefault(f["folder"], []).append((k, f))
    tmp = into / "_stream.bin"
    try:
        for n, folder in enumerate(got["folders"]):
            mine = by_folder.get(n)
            if not mine:
                continue
            with open(tmp, "wb") as w:
                _folder_stream(d, folder, got["cb_data"], w.write)
            with open(tmp, "rb") as src:
                for k, f in mine:
                    parts = _safe_parts(f["name"])
                    if not parts:
                        continue
                    dest = into.joinpath(*parts)
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    src.seek(f["off"])
                    left = f["size"]
                    with open(dest, "wb") as w:
                        while left > 0:
                            chunk = src.read(min(left, 1 << 20))
                            if not chunk:
                                break
                            w.write(chunk)
                            left -= len(chunk)
                    out.append((k, f["name"], dest))
            tmp.unlink(missing_ok=True)
    except CabUnsupported as e:
        log.info("%s は %s で圧縮されています ── expand に回します", at.name, e)
        return _expand_fallback(at, into, got["files"])
    except (ValueError, zlib.error, OSError) as e:
        return None, f"ほどけない（{e}）"
    finally:
        tmp.unlink(missing_ok=True)
    # **目録の順のまま返す。** フォルダごとに配ったので、番号で並べ直す。
    out.sort(key=lambda kv: kv[0])
    return [(name, dest) for _k, name, dest in out], None


def load_beside(name):
    """隣の `<name>.py` を読む。**道を名指しする。**

    `import` に頼ると、**走らせる場所によって通らない**（`sys.path` に
    `scripts/` が入るとは限らない）── 現場で `ModuleNotFoundError` になった。
    走査が偽物を `sys.modules` に差し込んでいたので、**本物の読み込みを一度も
    試していなかった**のが見逃した理由。偽物が本物より甘いと検査は嘘をつく。
    """
    import importlib.util
    got = sys.modules.get(name)
    if got is not None:
        return got                       # 一度読んだものを使い回す
    at = Path(__file__).resolve().parent / f"{name}.py"
    if not at.is_file():
        sys.exit(f"{at} がありません（`git pull` は済んでいますか）。")
    spec = importlib.util.spec_from_file_location(name, at)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


def load_onestore():
    """`.one` を読む隣の一枚。**道を名指しで読む**（`load_beside`）。"""
    return load_beside("onestore")


def opened_sections(pkg, entries):
    """開いた `.onepkg` の中身を、**入れ物の中の道ごと**返す。

    `[(ノートブック, 道の並び, ファイル, セクション名)]` ── 道の並びが
    セクショングループ。前の版は `rglob` で `.one` を集めるだけで**どの
    フォルダに居たかを捨てていた**ので、`400_打合せ/水曜日打合せ` のような
    多層が一段に潰れ、取りこぼしに見えた（現場で気づかれた・依頼 597）。

    いまは CAB の目録に書いてある道をそのまま使う ── 名前も階層も、
    `expand` の手に渡さないので壊れない（依頼 617）。
    """
    out = []
    for name, at in entries:
        if not name.lower().endswith(".one"):
            continue
        parts = _safe_parts(name)
        if not parts:
            continue
        out.append((pkg.stem, parts[:-1], Path(at), Path(parts[-1]).stem))
    return out


def from_files(where, args, out_root):
    """**書き出したファイルから写す。** COM を通らない道（依頼 594）。

    受け取るのは `.onepkg`（入れ物）か `.one`（セクション一本）か、その両方が
    入ったフォルダ。`.onepkg` は**こちらでほどいて**から中の `.one` を読む。

    フォルダの形は COM の道と同じ ── **セクション＝フォルダ、ページ＝`.md`、
    画像はノートの隣の `attachments/`**。下流（前書き・差分・`--prune`）も同じ。
    """
    onestore = load_onestore()

    root = Path(where)
    if not root.exists():
        sys.exit(f"ありません: {root}")
    kinds = (".one", ".onepkg")
    if root.is_file():
        found = [root]
    elif root.is_dir():
        found = sorted(q for q in root.rglob("*")
                       if q.is_file() and q.suffix.lower() in kinds)
    else:
        found = []
    sections = []
    for q in found:
        if q.suffix.lower() == ".onepkg":
            into = Path(tempfile.gettempdir()) / f"amber-onepkg-{os.getpid()}-{q.stem[:20]}"
            entries, why = unpack_onepkg(q, into)
            if entries is None:
                log.error("開けない %s: %s", q.name, why)
                continue
            sections += opened_sections(q, entries)
        else:
            sections.append((q.parent.name, [], q, q.stem))
    if not sections:
        sys.exit(f"{root} の下に .one がありません。")

    stats = {"pages": 0, "written": 0, "skipped_pages": 0, "images": 0, "errors": 0,
             "skipped_sections": 0, "filtered_sections": 0, "pruned": 0, "pruned_images": 0}
    written: set = set()
    # **どこまで進んだかを数で言う。** 窓には回っているだけの棒が出ていたが、
    # あれは何も測っていなかった（本人「ローディングバーは全く動かなかった」）。
    # 動かない棒より、`[3/12]` のほうが正直で、役に立つ（依頼 617）。
    for n, (book, groups, at, name) in enumerate(sections, 1):
        nb = sanitize(book)
        # **セクショングループは、フォルダのまま。** ここを潰すと
        # `400_打合せ/月_定例` が一段になり、取りこぼしに見える。
        gs = [sanitize(g) for g in groups]
        sec = sanitize(name)
        if not chosen("/".join([nb] + gs + [sec]), args):
            stats["filtered_sections"] += 1
            log.debug("絞りで外した: %s/%s", nb, sec)
            continue
        try:
            got = onestore.pages(at.read_bytes())
        except Exception as e:  # noqa
            log.error("読めない %s: %s", at.name, e)
            stats["errors"] += 1
            continue
        log.info("[%d/%d] %s ── %d ページ", n, len(sections),
                 "/".join([nb] + gs + [sec]), len(got))
        if not got:
            # **0 ページを黙って通さない。** たいていは形式のほう
            # （`638DE92F…` は読めない ── 依頼 591）。わけを言う。
            _, what = one_format(at)
            log.warning("%s は 1 ページも取れなかった ── %s", sec, what)
            stats["skipped_sections"] += 1
        sec_dir = out_root / nb
        for g in gs:
            sec_dir = sec_dir / g
        sec_dir = sec_dir / sec
        levels = {1: sec_dir}
        for pg in got:
            stats["pages"] += 1
            title = (pg["title"] or "").strip() or "Untitled"
            level = max(1, pg["level"])
            parent = levels.get(level - 1, sec_dir) if level > 1 else sec_dir
            # **同じ題のページが二枚あると、上書きで字が消える。**
            # COM の道の `place_for` と同じ考えで、`名前 (2).md` にずらす。
            base = sanitize(title)
            n = 1
            while True:
                md_path = shorten(parent / (f"{base}.md" if n == 1 else f"{base} ({n}).md"))
                if md_path.resolve() not in written:
                    break
                n += 1
            levels[level] = parent / md_path.stem
            for k in [k for k in levels if k > level]:
                del levels[k]
            if args.dry_run:
                print("  " * level + f"- {title}")
                continue
            # **画像は、ノートの隣の `attachments/` へ**（ambər の決まり・依頼 593）。
            img_dir = md_path.parent / ATTACH
            stem = amber_stem(md_path.stem)
            shots = []
            if not args.no_images:
                for i, im in enumerate(pg.get("images") or [], 1):
                    raw = im.get("bytes") or b""
                    # **拡張子は、中身から決める**（依頼 620）。OneNote の
                    # 画像はたいてい名前を持たない ── 名前だけを見ていると
                    # 何でも `.png` になり、**JPEG を .png として置く**ことに
                    # なる（ambər は開けるが、外へ出したときに困る）。
                    kind = im.get("kind")
                    if not kind:
                        ext = (Path(im.get("name") or "").suffix or "").lstrip(".").lower()
                        kind = ext if ext in ("png", "jpg", "jpeg", "gif", "bmp",
                                              "webp", "tif") else None
                    if not kind:
                        # **絵として名乗らないものは置かない。** 前はここで
                        # プロパティ集合の生バイトを `.png` として書いていて、
                        # 一枚も開けない画像がノートごとに並んだ。
                        log.debug("絵として読めない中身を飛ばした（%d バイト）", len(raw))
                        stats["skipped_images"] = stats.get("skipped_images", 0) + 1
                        continue
                    fname = f"{stem}-{i:03d}.{kind}"
                    img_dir.mkdir(parents=True, exist_ok=True)
                    at_img = img_dir / fname
                    if not (at_img.exists() and at_img.stat().st_size == len(raw)
                            and at_img.read_bytes() == raw):
                        at_img.write_bytes(raw)
                    shots.append(f"![]({ATTACH}/{fname})")
                    stats["images"] += 1
            lines = [onestore.as_markdown(ln) for ln in pg["lines"]]
            lines = [x for x in lines if x and x.strip()]
            # **表は、ページの上での位置に置く。** 末尾にまとめると、
            # 前後の文と離れて何の表か分からなくなる。
            for t in pg.get("tables") or []:
                lines.append("\n".join(onestore.table_markdown(t)))
            lines += shots
            md_path.parent.mkdir(parents=True, exist_ok=True)
            created = pg.get("created")
            day = (datetime(1980, 1, 1) + timedelta(seconds=created)).strftime("%Y-%m-%d") \
                if created else datetime.now().strftime("%Y-%m-%d")
            head = ["---", f'title: "{title.replace(chr(34), chr(39))}"',
                    f"created: {day}",
                    f'onenote_path: "{" / ".join([nb] + gs + [sec])}"', "---", ""]
            with open(md_path, "w", encoding="utf-8", newline="\n") as f:
                f.write("\n".join(head) + f"# {title}\n\n" + "\n\n".join(lines) + "\n")
            written.add(md_path.resolve())
            stats["written"] += 1
    log.info("完了: ページ %d（書いた %d）/ セクション %d / 絞りで外した %d / エラー %d",
             stats["pages"], stats["written"], len(sections),
             stats["filtered_sections"], stats["errors"])
    return 1 if stats["errors"] else 0


def peek(where):
    """`.one` を探して、**どちらの形式かだけ**数える。

    散らばっているものを一つずつ開いて道を打ち直すのは、人にやらせる仕事では
    ない ── フォルダを一つ指せば、機械が歩いて数える。
    """
    root = Path(where)
    if not root.exists():
        print(f"ありません: {root}")
        return 1
    kinds = {".one": [], ".onetoc2": [], ".onepkg": []}
    if root.is_file():
        found = [root]
    else:
        found = [q for q in root.rglob("*")
                 if q.is_file() and q.suffix.lower() in kinds]
    # **`.onepkg` は入れ物。** 中を見ないと、形式は分からない。
    # **開いたら、中の道をそのまま出す。** 階層が合っているかは、人が見れば
    # 一目で分かる ── 数だけ出しても、どこが潰れたのかは分からない。
    opened = []
    for q in [q for q in found if q.suffix.lower() == ".onepkg"]:
        raw = q.read_bytes()
        head, why = cab_read(raw)
        if head is None:
            print(f"読めない {q.name}: {why}")
            continue
        comps = "・".join(sorted({COMP_NAMES.get(f["comp"], str(f["comp"]))
                                  for f in head["folders"]})) or "？"
        print(f"{q.name}: フォルダ {len(head['folders'])} / "
              f"ファイル {len(head['files'])} / 圧縮 {comps}")
        into = Path(tempfile.gettempdir()) / f"amber-onepkg-{os.getpid()}-{q.stem[:20]}"
        entries, why = unpack_onepkg(q, into)
        if entries is None:
            print(f"  開けない: {why}")
            continue
        for name, at in entries:
            if Path(name).suffix.lower() in kinds:
                print(f"  {name}")
        opened += [Path(at) for name, at in entries
                   if Path(name).suffix.lower() in kinds]
    found += opened
    for q in found:
        kinds[q.suffix.lower()].append(q)
    if not any(kinds.values()):
        print(f"{root} の下に .one はありませんでした。")
        print("ノートブックが SharePoint にしか無いのかもしれません（手元は別の形）。")
        return 1

    counts, sample = {}, {}
    for q in kinds[".one"]:
        got, name = one_format(q)
        key = f"{(got or '?')[:8]}…（{name}）"
        counts[key] = counts.get(key, 0) + 1
        sample.setdefault(key, q)
    print(f"歩いた: {root}")
    for ext in (".one", ".onetoc2", ".onepkg"):
        if kinds[ext]:
            print(f"  {ext:10} {len(kinds[ext])} 本")
    print()
    for key, n in sorted(counts.items(), key=lambda kv: -kv[1]):
        print(f"  {n:4} 本  {key}")
        print(f"          例: {sample[key]}")
    # **数えたら、意味を言う。** 数字だけ見せて人に判じさせない。
    ok = sum(n for k, n in counts.items()
             if any(k.startswith(g[:8]) for g, (_n, good) in ONE_FORMATS.items() if good))
    total = sum(counts.values())
    print()
    if total and ok == total:
        print("→ **ぜんぶ公開仕様。** ファイルから直に読む道（案C）が通ります。")
    elif ok:
        print(f"→ 公開仕様は {ok}/{total} 本。混ざっています。")
    else:
        print("→ **一本も公開仕様ではありません。** ファイルから直に読む道は重くなります。")
    return 0






















# **答えの中の数字は、そのまま原因を名指しする。** COM は落ちた理由を
# 番号で言う ── 読み方を知っていれば、推し量らずに済む。







































@contextlib.contextmanager
def only_one(out_root: Path):
    """一度に一本だけ走らせる。**一時間ごとに回すなら、これが要る。**

    最初の一回は全ページを書くので、一時間で終わらないことがある。終わる前に
    次の回が始まると、二本が同じファイルへ同時に書き、しかも `--prune` は
    相手がまだ書いていないページを「OneNote 側で消えたページ」と見て消す。

    鎖は OS のもの（PID を書いたファイルではなく）── 落ちても電源が切れても、
    **誰も外せない鎖が残らない**。置き場所は出力先ではなく一時フォルダ:
    出力先は ambər の保存ディレクトリで、そこに置いたものは一覧に並ぶ。
    """
    key = hashlib.sha1(str(out_root.resolve()).encode("utf-8")).hexdigest()[:16]
    lock_path = Path(tempfile.gettempdir()) / f"onenote2md-{key}.lock"
    fd = os.open(str(lock_path), os.O_RDWR | os.O_CREAT, 0o600)
    try:
        try:
            if os.name == "nt":
                import msvcrt
                msvcrt.locking(fd, msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            sys.exit(f"前の回がまだ走っています（鎖: {lock_path}）。今回は何もしません。")
        yield
    finally:
        os.close(fd)


# ---------------------------------------------------------------------------
# 名前 ── SharePoint / WebDAV / Windows のどこでも通るもの
# ---------------------------------------------------------------------------
# Windows で使えない字 + SharePoint が断る字（# % & ~ { }）+ 制御文字
_INVALID = re.compile(r'[\\/:*?"<>|#%&~{}\x00-\x1f]+')


# Windows の予約名（`CON.md` も Windows には CON）。ambər の `note::file_stem`
# と同じ顔ぶれ。
_RESERVED = {"CON", "PRN", "AUX", "NUL",
             *(f"COM{i}" for i in range(1, 10)), *(f"LPT{i}" for i in range(1, 10))}


def amber_stem(title):
    """**ambər が画像の名前に使う幹**（`note::file_stem` と同じ規則）。

    正本は `crates/amber-core/src/note.rs` の `pub fn file_stem`。ここは写しで、
    **写しである以上ずれうる** ── ずれると、ambər がノートを改名・移動した日に
    画像が付いてこない（あちらは「幹 + ハイフン」で自分の画像を見分ける）。
    だから走査で、同じ答えになることを確かめている。

      * 使えない字（`/ \\ : * ? " < > |` と制御文字）は落とし、続いた一続きは
        `-` 一つに潰す。**先頭には置かない**（`?? notes` は `notes`）
      * 60 字まで
      * 前後の空白・`.`・全角空白は落とす
      * 予約名なら頭に `_`
    """
    out, gap = [], False
    for c in (title or "").strip():
        if c in '/\\:*?"<>|' or ord(c) < 0x20:
            gap = True
            continue
        if gap and out:
            out.append("-")
        gap = False
        if len(out) >= 60:
            break
        out.append(c)
    got = "".join(out).strip(" ." + chr(0x3000))
    if not got:
        return ""
    head = got.split(".")[0].upper()
    return f"_{got}" if head in _RESERVED else got


def sanitize(name, fallback="untitled"):
    name = (name or "").strip()
    name = _INVALID.sub("_", name)
    name = re.sub(r"\s+", " ", name).strip(" .")
    # SharePoint は `_vti_` で始まる名前を断る。
    if name.lower().startswith("_vti_"):
        name = "x" + name
    # Windows の予約名（CON・PRN・AUX・NUL・COM1〜・LPT1〜）。
    if re.fullmatch(r"(?i)(con|prn|aux|nul|com[1-9]|lpt[1-9])", name):
        name = name + "_"
    # **60 字で切る。** ambər の `file_stem` が 60 で切るので、ここを 120 の
    # ままにすると**長い題のページだけ、画像の幹がずれる** ── そのノートは
    # 改名・移動したときに画像が付いてこない。
    return name[:60] or fallback


def shorten(path: Path, room: int = PATH_LIMIT) -> Path:
    """道ぜんたいが長すぎるとき、ファイル名の茎を詰める（拡張子は残す）。"""
    s = str(path)
    if len(s) <= room:
        return path
    over = len(s) - room
    stem = path.stem
    if len(stem) - over < 8:
        return path  # 詰めきれない ── そのまま書いて、落ちたら落ちたと言う
    return path.with_name(stem[: len(stem) - over].rstrip(" .") + path.suffix)




# ---------------------------------------------------------------------------
# インライン変換（<one:T> の中身は HTML 風テキスト）
# ---------------------------------------------------------------------------















# ---------------------------------------------------------------------------
# 前書き ── ambər が読む `title:` と `created:` を持つ
# ---------------------------------------------------------------------------



















def chosen(sec_path: str, args) -> bool:
    """このセクションを写すか。`sec_path` は **OneNote 側の道**
    （`ノートブック/グループ/セクション`）── 出力先の名前ではない。

    `--flatten-groups` は出力の名前を「グループ › セクション」に畳むが、
    **絞りはいつも OneNote で見えている道に当たる** ── 畳んだ名前に当てると、
    同じ `--only` が旗の有無で違うものを拾う。

    `--only` は「どれか一つにでも当たれば写す」、`--skip` は「どれか一つにでも
    当たれば写さない」。両方あれば `--skip` が勝つ（外すほうが、足すより強い ──
    逆にすると「外したはずのものが混ざる」ほうの事故になる）。
    """
    low = sec_path.lower()
    if args.only and not any(pat.lower() in low for pat in args.only):
        return False
    if args.skip and any(pat.lower() in low for pat in args.skip):
        return False
    return True






def build_parser():
    """引数の定義は**ここ一つ**。走査（onenote-test.py）も同じものを使う ──
    写すと、片方にだけ足した旗で走査が落ちる（しかも落ち方が `AttributeError`
    なので、何が足りないのか画面から分からない）。"""
    ap = argparse.ArgumentParser(description="OneNote → Markdown（階層保持・ambər の保存ディレクトリ向け）")
    ap.add_argument("--out", help="出力先フォルダ（ambər の保存ディレクトリの下）")
    ap.add_argument("files", nargs="?", metavar="ファイル",
                    help=".onepkg / .one / それが入ったフォルダ")
    ap.add_argument("--only", action="append", metavar="道",
                    help="このセクションだけ写す。`ノートブック/グループ/セクション` の道に部分一致（複数指定可）")
    ap.add_argument("--skip", action="append", metavar="道",
                    help="このセクションは写さない。--only より強い（複数指定可）")
    ap.add_argument("--peek", metavar="道",
                    help=".one を探して、どちらの形式かだけ数える（中身は読まない）")
    ap.add_argument("--ui", action="store_true",
                    help="小さい窓を出して、押して選ぶ（何も渡さずに走らせたときは、これが既定）")
    ap.add_argument("--dry-run", action="store_true", help="どこに何を書くか並べるだけ。書き込みなし")
    ap.add_argument("--no-images", action="store_true", help="画像を出力しない")
    ap.add_argument("--log", metavar="ファイル", help="経過をこのファイルにも書き足す")
    ap.add_argument("-v", "--verbose", action="store_true")
    return ap




def main():
    args = build_parser().parse_args()
    use_utf8()

    logging.basicConfig(level=logging.DEBUG if args.verbose else logging.INFO,
                        format="%(levelname)s %(message)s")
    if args.log:
        # 誰も見ていない回の記録。**落ちたことが残らなければ、落ちていない
        # のと見分けがつかない。**
        fh = logging.FileHandler(args.log, encoding="utf-8")
        fh.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(message)s"))
        logging.getLogger().addHandler(fh)

    # **何も渡されなければ、窓を出す。** バッチをダブルクリックした人は
    # 引数を渡せない ── そこで使い方を出して終わるのは、道具ではない。
    if args.ui or not (args.files or args.peek):
        # **隣の一枚は、道を名指しして読む**（`load_onestore` と同じ理由）──
        # `import` に頼ると、走らせる場所によって `sys.path` に `scripts/` が
        # 入らず `ModuleNotFoundError` になる。窓が出ないのが「落ちた」と
        # 見分けられない形で出る。
        return load_beside("onenote_ui").ask_and_run(args)

    if not args.peek:
        log.info("=== onenote2md 開始 %s", datetime.now().isoformat(timespec="seconds"))
    out_root = Path(args.out) if args.out else Path(".")
    try:
        if args.peek or args.dry_run:
            return run(args, out_root)
        with only_one(out_root):
            return run(args, out_root)
    except SystemExit as e:
        # **`sys.exit("わけ")` は stderr にしか出ない。** 定時で回すときの
        # `pythonw.exe` に stderr は無いので、わけがどこにも残らない。
        if isinstance(e.code, str):
            log.error("%s", e.code)
            return 1
        return e.code or 0
    except Exception:
        log.exception("落ちました")
        return 1


def run(args, out_root: Path):
    if args.peek:
        return peek(args.peek)
    if not args.out:
        sys.exit("--out が要ります（出力先フォルダ）。")
    return from_files(args.files, args, out_root)


if __name__ == "__main__":
    sys.exit(main() or 0)
