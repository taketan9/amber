#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
onenote2md.py ── デスクトップ版 OneNote を Markdown に（ambər の保存ディレクトリ向け）

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

前提
  - Windows + デスクトップ版 OneNote（Microsoft 365 / 2016 以降）
    ※ Store 版（OneNote for Windows 10）は COM 非対応
  - 変換対象のノートブックがすべて OneNote 上で「開いている」こと
  - pywin32（**オフラインの端末なら** wheel を持ち込んで
    `py -3-32 -m pip install --no-index --no-deps "<その .whl>"`。
    どの wheel が要るかは `--check` が名前で言う）

使い方
  python onenote2md.py --out D:\\notes                      # 全ノートブック
  python onenote2md.py --out D:\\notes --list              # 写せるセクションを道で並べる
  python onenote2md.py --out D:\\notes --notebook "案件A"    # ノートブックで絞る（部分一致）
  python onenote2md.py --out D:\\notes --only "仕事/議事録"  # セクションで絞る（道に部分一致）
  python onenote2md.py --out D:\\notes --skip "仕事/私的"    # これだけ外す（--only より強い）
  python onenote2md.py --out D:\\notes --dry-run            # 階層だけ表示、書き込みなし
  python onenote2md.py --out D:\\notes --no-images          # 画像を出力しない
  python onenote2md.py --out "\\\\テナント@SSL\\DavWWWRoot\\sites\\…"   # WebDAV へ直に（遅い。下の註）

繰り返し走らせるための決まり（2026-09-12・ambər 側の見立てから）
  1. **同じページは同じファイルに書く。** ファイル名の脇に OneNote のページ ID を
     持たない代わりに、**同じ親フォルダの同じ名前 → 同じファイル**とみなし、
     前書きの `onenote_id` が違えば `名前 (2).md` にずらす。前の版は
     `名前 (2)` を無限に増やしていた（毎回すべてのページが増殖する）。
  2. **変わっていないページは書かない。** 前書きの `modified` と OneNote の
     `lastModifiedTime` が同じなら飛ばす（ambər や Drive の同期に、触っていない
     ファイルの更新だけが流れない）。`--force` で全部書き直す。
  3. **OneNote 側で消えたページは消さない。** 消したいときは `--prune`（このスクリプトが
     書いた `.md` のうち、今回出てこなかったものをゴミ箱ではなく**削除**する。
     ambər で直したページも消える ── 使うときは承知の上で）。
  4. **改行は LF。** Windows の `write_text` は CRLF にするので、`newline="\\n"` で書く。
  5. **名前は SharePoint / WebDAV でも通るものに。** `\\ / : * ? " < > |` に加えて
     `# % & ~ { }` と先頭の `_vti_`、末尾の `.` と空白を避ける。一段 120 字まで、
     道ぜんたいで 200 字を超えそうなら詰める（WebDAV の 256 字の壁）。
  6. **画像は `attachments/`**（ambər の決まり）。ノートの隣のフォルダで、名前は
     `<ページ名>_001.png`。ambər の「使われていない画像」もここを数える。
  7. **深さ。** ambər が一覧に出すのはフォルダ 8 段まで。セクショングループが
     深いノートブックは `--flatten-groups` で「グループ名 › セクション名」を一つの
     フォルダ名に畳める。

定時で回すための決まり（2026-09-14・「1時間ごとに、更新があったノートだけ」から）
  8. **一度に一本だけ**（OS の鎖）。最初の一回は全ページ書くので一時間で終わらない
     ことがあり、終わる前に次の回が始まると二本が同じファイルを奪い合う。
  9. **読む前に `--sync`。** 開いているだけのノートブックは、電話や Web で直しても
     OneNote が同期するまでこの機械の上では古いまま ── こちらは「変わっていない」と
     見て飛ばす。頼んでおけば次の回には届いている。
 10. **`--prune` は歩いた場所だけ。** `--notebook` で絞った回や、ノートブックを
     閉じていた回に、出力先ぜんたいを消させない。鍵のかかったセクションの下と、
     取得に失敗したページも残す ── **見えなかった場所は、消えた場所ではない。**
 11. **`--log`。** 定時で回すと画面には誰もいない。落ちたことが残らなければ、
     落ちていないのと見分けがつかない。

  設定の手順とハマりどころは docs/onenote.ja.md。走査は scripts/onenote-test.py
  （COM を偽物に差し替えるので mac でも通る）、その検査を壊して鳴らすのが
  scripts/onenote-mutate.sh。

WebDAV（`\\\\テナント@SSL\\DavWWWRoot\\…`）へ直に書くとき
  - Windows の WebClient サービスが動いていること。
  - 一ファイルごとに PUT と PROPFIND が走るので、ページ数が多いと遅い。
    先にローカルへ出して `robocopy /MIR` で送るほうが速くて確実。
  - 上の 5 の名前の決まりは、直に書くときに効く。
"""

from __future__ import annotations

import argparse
import base64
import contextlib
import hashlib
import html
import importlib
import logging
import os
import re
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path
import xml.etree.ElementTree as ET

ONE_NS = "http://schemas.microsoft.com/office/onenote/2013/onenote"
NS = {"one": ONE_NS}
HS_PAGES = 4          # HierarchyScope.hsPages
XS_2013 = 2           # XMLSchema.xs2013
PI_BINARY_DATA = 1    # PageInfo.piBinaryData（画像を Base64 で同梱）
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
ONENOTE_TYPELIB = "{0EA692EE-BB50-4E3C-AEF0-356D91732725}"


def _can_write(d) -> bool:
    """そこに本当に書けるか。**`os.access` は Windows では当てにならない** ──
    ディレクトリについては読み取り専用属性しか見ず、ACL で拒まれる場所でも
    True を返す。だから実際に一つ置いて、消す。

    **この性質は mac の走査では証明できない** ── POSIX の `os.access` は正しく
    答えるので、`os.access` に戻しても走査は通ってしまう。ここは Windows の
    上でしか壊れない。"""
    try:
        os.makedirs(d, exist_ok=True)
        probe = os.path.join(d, f".amber-probe-{os.getpid()}")
        with open(probe, "w"):
            pass
        os.unlink(probe)
        return True
    except OSError:
        return False


def _gen_py_somewhere_writable():
    """makepy の作り置き先。**書けない場所なら、書ける場所へ逃がす。**

    既定は `site-packages\\win32com\\gen_py` で、Python が
    `C:\\Program Files\\` に入っていると管理者でないと書けない
    （`PermissionError: ...gen_py\\....py.100000.temp`）。会社の端末で管理者に
    なれるとは限らないので、こちらで逃がす ── 管理者の窓で一度作る手もあるが、
    **そこは OneNote に繋げない窓**（権限がずれる）なので、話が噛み合わない。

    **`win32com.client` を読む前に決めること。** gencache は読み込みの時点で
    この道を見るので、あとから変えても遅い。
    """
    import win32com

    default = getattr(win32com, "__gen_path__", "")
    if default and _can_write(default):
        return default, False
    at = os.path.join(tempfile.gettempdir(),
                      f"amber-gen_py-{sys.version_info[0]}.{sys.version_info[1]}")
    if not _can_write(at):
        return default, False        # そこも駄目なら、諦めて既定のまま進む

    # **書く先と読む先は別々にある。** pywin32 は `import win32com` した時点で
    # `win32com.gen_py` モジュールを作り、`__path__` をそのときの `__gen_path__`
    # で**焼き付ける**。書く先（`__gen_path__`）だけ動かすと、makepy は新しい
    # 場所へ書き、`__import__("win32com.gen_py.<名前>")` は古い場所を探す ──
    # `No module named 'win32com.gen_py.0EA692EE-...x0x1x1'`。両方動かす。
    win32com.__gen_path__ = at
    gen_py = sys.modules.get("win32com.gen_py") or getattr(win32com, "gen_py", None)
    if gen_py is not None:
        gen_py.__path__ = [at]
    # 作ったばかりのフォルダは、Python の作り置きの中では「空」のまま。
    # pywin32 は `invalidate_caches()` を呼ばないので、こちらで呼ぶ。
    importlib.invalidate_caches()
    return at, True


def _strip_resource_index(path: str) -> str:
    """型ライブラリの道から、末尾の**資源の番号**を落とす。

    型ライブラリが exe の中に埋まっていると、登録される道は
    `…\\ONENOTE.EXE\\3` のようになる。番号ごと `os.path.exists` に渡すと、
    **在るものまで「無い」と言う** ── そして「実体が無い」は、こちらの
    見立てをまるごと変えてしまう嘘になる。
    """
    return re.sub(r"[\\/]\d+$", "", path or "")


def _reg_subkeys(key):
    out, i = [], 0
    while True:
        try:
            import winreg
            out.append(winreg.EnumKey(key, i))
        except OSError:
            return out
        i += 1


def _registered_arches():
    """OneNote の型ライブラリが、**どの bit で登録されているか。**

    読めなければ空を返す（Windows でない・枝が無い）── 空のときは
    何も言わない。**分からないことを分かったように言わない。**
    """
    try:
        import winreg
    except ImportError:
        return set()
    out = set()
    try:
        key = winreg.OpenKey(winreg.HKEY_CLASSES_ROOT, "TypeLib" + chr(92) + ONENOTE_TYPELIB)
        for ver in _reg_subkeys(key):
            vkey = winreg.OpenKey(key, ver)
            for lcid in _reg_subkeys(vkey):
                lkey = winreg.OpenKey(vkey, lcid)
                out.update(a.lower() for a in _reg_subkeys(lkey))
    except OSError:
        return out
    return out


def _typelib_tree():
    """型ライブラリの登録を、**版ごとに**（`{"1.1": {"win32", "win64"}}`）。

    **束ねて見てはいけない。** 版は一つとは限らず、この端末には `1.0` と
    `1.1` の両方が登録されていた（依頼 570）。束ねると「1.0 に win64、
    1.1 に win32」でも**「両方ある」に見える** ── 64 bit の処理は 1.1 を
    読みにいって落ちるのに、登録は白に見える。いちばん読みにくい形を、
    こちらで作ってしまう。
    """
    try:
        import winreg
    except ImportError:
        return {}
    out = {}
    try:
        key = winreg.OpenKey(winreg.HKEY_CLASSES_ROOT, "TypeLib" + chr(92) + ONENOTE_TYPELIB)
        for ver in _reg_subkeys(key):
            vkey = winreg.OpenKey(key, ver)
            got = set()
            for lcid in _reg_subkeys(vkey):
                lkey = winreg.OpenKey(vkey, lcid)
                got.update(a.lower() for a in _reg_subkeys(lkey))
            out[ver] = got
    except OSError:
        return out
    return out


def _arch_verdict(arches, bits=None):
    """登録されている bit と、いま走っている bit が噛み合っているか。

    噛み合っていないと、**皮は正しくかぶさるのに呼んだ瞬間に落ちる** ──
    いちばん読みにくい形になる（`ライブラリは登録されていません`）。
    見たことから結論まで言う。
    """
    me = bits or (64 if sys.maxsize > 2 ** 32 else 32)
    want = "win64" if me == 64 else "win32"
    if not arches or want in {a.lower() for a in arches}:
        return None
    return [
        "",
        f"→ **この Python は {me} bit なのに、{want} の登録が無い"
        f"（あるのは {sorted(a.lower() for a in arches)}）。**",
        "   これが『ライブラリは登録されていません』の正体です。",
        "   直し方は docs/onenote.ja.md の「ハマりどころ」に。",
    ]


def wheel_hint(bits=None):
    """**この Python に合う wheel の名前**と、オフラインでの入れ方。

    繋ぐ先はインターネットの無い端末で、持ち込めるのはファイルだけ。
    `pip install pywin32` と言うだけでは、そこでは何も起きない ── 要るのは
    **どのファイルを持ち込むか**で、それはこの Python の版と bit で決まる。
    間違えやすいのは bit のほうで、`win32` が 32 bit、`win_amd64` が 64 bit。
    """
    v = sys.version_info
    tag = f"cp{v[0]}{v[1]}"
    me = bits or (64 if sys.maxsize > 2 ** 32 else 32)
    arch = "win32" if me == 32 else "win_amd64"
    launcher = f"py -{v[0]}.{v[1]}" + ("-32" if arch == "win32" else "")
    return [f"この Python に合う wheel: pywin32-*-{tag}-{tag}-{arch}.whl",
            "（`win32` が 32 bit・`win_amd64` が 64 bit。取り違えると入らない）",
            "持ち込んだら、網に出ずに入れる:",
            f'  {launcher} -m pip install --no-index --no-deps "<その .whl>"']


def _other_pythons():
    """この機械に入っている Python を、ランチャーに訊く（`py -0p`）。

    返すのは `[(名札, 道)]`。**32 bit を入れたあとの一手を、こちらで言う
    ため。** 入れた人は 64 bit の癖でもう一度同じことを叩く ── そこで
    同じ答えが返るのでは、入れた意味が伝わらない。
    """
    try:
        import subprocess
    except ImportError:
        return []
    try:
        got = subprocess.run(["py", "-0p"], capture_output=True, text=True,
                             timeout=10, errors="replace")
    except Exception:  # noqa
        return []
    out = []
    for line in (got.stdout or "").splitlines():
        m = re.match(r"\s*-(?:V:)?(\S+)\s+\*?\s*(\S.*\.exe)\s*$", line)
        if m:
            out.append((m.group(1), m.group(2).strip()))
    return out


def _thirty_two_bit_here():
    """32 bit の Python が、この機械に居るか。居れば名札を返す。"""
    for tag, _path in _other_pythons():
        if tag.endswith("-32"):
            return tag
    return None


def forget():
    """makepy の作り置きを捨てる。

    **「`%LOCALAPPDATA%\\Temp\\gen_py` を消す」と書いてあっても、そこに無い。**
    こちらは書ける場所へ逃がしている（`%TEMP%\\amber-gen_py-3.x`・依頼 567）ので、
    人に探させると二つのフォルダを行き来させることになる ── 機械が両方消す。

    捨てても困らない。**次に繋いだとき、作り直される。**
    """
    import shutil
    gone = []
    where = [os.path.join(tempfile.gettempdir(),
                          f"amber-gen_py-{sys.version_info[0]}.{sys.version_info[1]}")]
    try:
        import win32com
        got = getattr(win32com, "__gen_path__", "")
        if got:
            where.append(got)
    except ImportError:
        pass
    for at in where:
        if not os.path.isdir(at):
            continue
        try:
            shutil.rmtree(at)
            gone.append(at)
        except OSError as e:
            print(f"消せない {at}: {e}")
    for at in gone:
        print(f"捨てた: {at}")
    if not gone:
        print("作り置きはありませんでした（探した先: " + " / ".join(where) + "）")
    print("次に繋いだときに作り直されます。")
    return 0


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


def _office_platform():
    """Office がどちらの bit で入っているか（`x64` / `x86`）。読めなければ None。"""
    try:
        import winreg
        # **64bit の見え方で読む。** ここは 64bit 側にしか無いので、
        # 32 bit の処理が素直に開くと「読めない」になる（WOW64）。
        key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,
                             r"SOFTWARE\Microsoft\Office\ClickToRun\Configuration", 0,
                             winreg.KEY_READ | getattr(winreg, "KEY_WOW64_64KEY", 0))
        return winreg.QueryValueEx(key, "Platform")[0]
    except (ImportError, OSError):
        return None


def _local_server():
    """`OneNote.Application` の COM サーバーの実体 ── `(道, 在るか)`。

    在るかは **三通り**: `True` / `False` / `None`（**見られなかった**）。
    前は pywintypes で ProgID を引いていたので、**pywin32 の無い Python では
    引けず、それを「無い」と言っていた** ── そして「入れ直すか修復する」と、
    見てもいないことから結論を出していた。**分からないことを分かったように
    言わない。** ProgID → CLSID はレジストリだけで引ける。
    """
    try:
        import winreg
    except ImportError:
        return None, None
    try:
        key = winreg.OpenKey(winreg.HKEY_CLASSES_ROOT,
                             "OneNote.Application" + chr(92) + "CLSID")
        clsid = (winreg.QueryValue(key, None) or "").strip()
        if not clsid:
            return None, None
        key = winreg.OpenKey(winreg.HKEY_CLASSES_ROOT,
                             "CLSID" + chr(92) + clsid + chr(92) + "LocalServer32")
        path = (winreg.QueryValue(key, None) or "").strip().strip('"')
    except OSError:
        return None, None
    real = _strip_resource_index(path)
    return path, bool(real and os.path.exists(real))


def _read_typelib(flag):
    """型ライブラリの登録を、**その見え方で**読む。`{(版, bit): 道}`"""
    out = {}
    try:
        import winreg
    except ImportError:
        return out
    try:
        key = winreg.OpenKey(winreg.HKEY_CLASSES_ROOT,
                             "TypeLib" + chr(92) + ONENOTE_TYPELIB, 0,
                             winreg.KEY_READ | flag)
    except OSError:
        return out
    try:
        for ver in _reg_subkeys(key):
            vkey = winreg.OpenKey(key, ver, 0, winreg.KEY_READ | flag)
            for lcid in _reg_subkeys(vkey):
                lkey = winreg.OpenKey(vkey, lcid, 0, winreg.KEY_READ | flag)
                for arch in _reg_subkeys(lkey):
                    try:
                        out[(ver, arch.lower())] = winreg.QueryValue(lkey, arch)
                    except OSError:
                        continue
    except OSError:
        return out
    return out


def _typelib_values():
    """**版 × bit ごとの道。32 bit と 64 bit、両方の見え方で読む。**

    レジストリは走っている処理の bit で見え方が変わる（WOW64）。片方しか
    読まないと、**もう片方の Python で走らせたときに別のものが見える** ──
    そして「32 bit で叩いたら道が無いと言われた」が、どちらの話なのか
    分からなくなる。実際にそれで一度、在るファイルを「無い」と言った。

    返すのは `(版, bit, 道, 見え方)`。両方の見え方で同じなら、見え方は空 ──
    **違うときだけ言う。** 同じものを二度見せると、人が読む行が倍になる。
    """
    try:
        import winreg
    except ImportError:
        return []
    v64 = _read_typelib(getattr(winreg, "KEY_WOW64_64KEY", 0))
    v32 = _read_typelib(getattr(winreg, "KEY_WOW64_32KEY", 0))
    out = []
    for ver, arch in sorted(set(v64) | set(v32)):
        a, b = v64.get((ver, arch)), v32.get((ver, arch))
        if a and b and a == b:
            out.append((ver, arch, a, ""))
            continue
        if a:
            out.append((ver, arch, a, "64bit の見え方"))
        if b:
            out.append((ver, arch, b, "32bit の見え方"))
    return out


def _typelib_path(want_arch):
    """型ライブラリの、その bit の枝が指している道。無ければ None。

    **足りない枝を足すとき、道は自分で調べさせない。** 手で打ち直す人に
    「docs を見て、`--probe` の出した値をそこから写して」と言うのは、
    繋がらない端末の前に立っている人に出す注文ではない。
    """
    for ver, arch, path, _view in _typelib_values():
        if arch == want_arch.lower():
            return ver, "0", path
    return None


def _onenote_exe():
    """**デスクトップ版 OneNote の実体が、どこに在るか。**

    登録が指す先に無いなら、次に知りたいのは「入っていないのか、
    別の場所から起きているのか」。Click-to-Run の入れ場所を訊いて、
    ありそうなところを順に見る ── 見つからなければ何も言わない。
    """
    roots = []
    try:
        import winreg
        key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,
                             r"SOFTWARE\Microsoft\Office\ClickToRun\Configuration")
        got = winreg.QueryValueEx(key, "InstallationPath")[0]
        if got:
            roots.append(got)
    except Exception:  # noqa
        pass
    roots += [r"C:\Program Files\Microsoft Office", r"C:\Program Files (x86)\Microsoft Office"]
    for root in roots:
        for mid in ("root" + chr(92) + "Office16", "Office16", "root" + chr(92) + "Office15"):
            at = os.path.join(root, mid, "ONENOTE.EXE")
            if os.path.exists(at):
                return at
    return None


def _store_onenote():
    """**ストア版（OneNote for Windows 10）も入っているか。**

    入っていること自体は害ではないが、**COM を持つのはデスクトップ版だけ**
    なので、ふだんストア版を使っていると「繋がったのに、開いているノート
    ブックが一冊も無い」になる ── 写すノートブックは**デスクトップ版で**
    開いていないと、こちらからは見えない。
    """
    try:
        import winreg
        key = winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                             r"Software\Classes\ActivatableClasses\Package")
    except (ImportError, OSError):
        return None                      # Windows でない・読めない ── 何も言わない
    for name in _reg_subkeys(key):
        if name.lower().startswith("microsoft.office.onenote"):
            return name
    return False


def probe():
    """この端末で何が起きているかを、そのまま並べる。

    **当てにいって二度外した。** mac には COM が無いので、こちらでは
    再現できない ── ならば推し量るのをやめて、事実を見てから直す。
    一行ずつ守ってあるので、途中が転んでも残りは出る。
    """
    def say(label, fn):
        try:
            print(f"  {label}: {fn()}")
        except Exception as e:  # noqa
            print(f"  {label}: ✗ {type(e).__name__}: {e}")

    print("== python ==")
    say("版", lambda: sys.version.replace("\n", " "))
    say("bit", lambda: 64 if sys.maxsize > 2 ** 32 else 32)
    say("実行ファイル", lambda: sys.executable)

    print("== pywin32 ==")

    def w32():
        import win32com
        return win32com

    def gc():
        from win32com.client import gencache
        return gencache

    def cli():
        import win32com.client
        return win32com.client

    def pyc():
        import pythoncom
        return pythoncom

    say("pywin32", lambda: __import__("importlib.metadata", fromlist=["x"]).version("pywin32"))
    say("win32com", lambda: w32().__file__)
    say("作り置き先（書く）", lambda: w32().__gen_path__)
    say("そこに書けるか", lambda: _can_write(w32().__gen_path__))
    say("作り置き先（読む）",
        lambda: getattr(sys.modules.get("win32com.gen_py"), "__path__", "（無い）"))
    # **これは「逃がす前」の姿。** 実際に走るときはここから動く ── 動いた先も出す。
    say("逃がすとどこへ", lambda: _gen_py_somewhere_writable())

    print("== Office ==")

    say("Office の bit（x64 / x86）", lambda: _office_platform() or "（読めない）")
    say("ストア版も入っているか",
        lambda: {None: "（読めない）", False: "入っていない"}.get(_store_onenote(),
                                                                _store_onenote()))

    print("== 型ライブラリの登録 ==")

    def _subkeys(key):
        out, i = [], 0
        while True:
            try:
                import winreg
                out.append(winreg.EnumKey(key, i))
            except OSError:
                return out
            i += 1

    def versions():
        """**版ごとに、実体の道とその有無まで見る。**

        会社の端末には `1.0` と `1.1` の両方が登録されていた。版が並んで
        いるだけでは、どちらが使えるか分からない ── 片方は実体を指して
        いないことがある（それが `ライブラリは登録されていません` の正体）。
        だから道を引いて、**ファイルがあるかまで見る。**
        """
        import winreg
        root = "TypeLib" + chr(92) + ONENOTE_TYPELIB
        key = winreg.OpenKey(winreg.HKEY_CLASSES_ROOT, root)
        lines = []
        arches = set()
        for ver in _subkeys(key) or ["（版が一つも無い）"]:
            vkey = winreg.OpenKey(key, ver)
            found = False
            for lcid in _subkeys(vkey):
                lkey = winreg.OpenKey(vkey, lcid)
                for arch in _subkeys(lkey):
                    try:
                        path = winreg.QueryValue(lkey, arch)
                    except OSError:
                        path = "（値が無い）"
                    # 道の末尾に `\3` のような**資源の番号**が付くことがある
                    # （型ライブラリが exe の中に埋まっている場合）。番号ごと
                    # `os.path.exists` に渡すと、在るものまで「無い」と言う。
                    real = _strip_resource_index(path)
                    here = "ある" if real and os.path.exists(real) else "**無い**"
                    lines.append(f"{ver} / lcid {lcid} / {arch} = {path}  → {here}")
                    arches.add(arch.lower())
                    found = True
            if not found:
                lines.append(f"{ver}: 中に lcid が無い ── **空の枝**")
        # **見たことから結論まで言う。** ここが噛み合っていないと、
        # 皮は正しくかぶさるのに呼んだ瞬間に落ちる ── いちばん読みにくい形。
        lines.extend(_arch_verdict(arches) or [])
        return "\n      " + "\n      ".join(lines)

    say("TypeLib" + chr(92) + ONENOTE_TYPELIB, versions)

    print("== OneNote ==")
    def clsid():
        import pywintypes
        return str(pywintypes.IID("OneNote.Application"))   # ProgID も引ける

    say("ProgID → CLSID", clsid)

    def known():
        import pywintypes
        got = gc().GetClassForCLSID(pywintypes.IID("OneNote.Application"))
        return got or "（生成された型を知らない ── だから遅い束ねで返る）"

    say("gencache が知っている型", known)

    def local_server():
        """**COM サーバーの実体が、そこに在るか。**

        `サーバーの実行に失敗しました`（0x80080005）も
        `ライブラリは登録されていません` も、**登録が指す道にファイルが無い**
        だけで両方出る。動いている OneNote が別の場所から起きていると、
        「繋がるのに呼べない」というちぐはぐな形になる。
        """
        path, here = _local_server()
        return f"{path}  → {'ある' if here else '**無い**'}"

    say("COM サーバーの実体", local_server)

    def interface_typelib():
        """**取り次ぎ側が見ている登録。**

        別プロセスの COM を呼ぶと、呼び出しは取り次がれる（marshaling）。
        取り次ぐ側は `HKCR\\Interface\\{IID}\\TypeLib` を見て「どの型ライブラリの
        どの版か」を引く ── **ここが空の枝を指していれば、型ライブラリ本体が
        読めていても `ライブラリは登録されていません` になる。**
        繋がるのに呼べない、のいちばん奥の理由がここに出る。
        """
        import winreg
        mod = gc().EnsureModule(ONENOTE_TYPELIB, 0, 1, 1)
        names = [n for n in dir(mod)
                 if isinstance(getattr(mod, n, None), type)
                 and hasattr(getattr(mod, n), "GetHierarchy")]
        if not names:
            return "（GetHierarchy を持つ型が無い）"
        iid = str(getattr(mod, names[0]).CLSID)
        key = winreg.OpenKey(winreg.HKEY_CLASSES_ROOT,
                             "Interface" + chr(92) + iid + chr(92) + "TypeLib")
        lib = winreg.QueryValue(key, None)
        try:
            ver = winreg.QueryValueEx(key, "Version")[0]
        except OSError:
            ver = "（Version が無い）"
        return f"{names[0]} {iid} → 型ライブラリ {lib} の版 {ver}"

    say("取り次ぎ側が見ている登録", interface_typelib)

    def object_typelib():
        """**生きているオブジェクト自身が、どの型ライブラリを名乗るか。**

        ここまで見たのは全部「登録の上ではどうなっているか」だった。
        だが効くのは**オブジェクトが自分をどう言うか**で、そこが空の枝
        （版 1.0）を指していれば、登録がいくら正しくても引けない。
        `GetTypeInfo` そのものが落ちるなら、それもまた答えになる。
        """
        raw = cli().Dispatch("OneNote.Application")
        ole = getattr(raw, "_oleobj_", raw)
        n = ole.GetTypeInfoCount()
        if not n:
            return "型情報を 1 つも持っていない（GetTypeInfoCount = 0）"
        ti = ole.GetTypeInfo()
        lib, index = ti.GetContainingTypeLib()
        a = lib.GetLibAttr()
        return (f"名乗っている型ライブラリ {a[0]} / lcid {a[1]} / syskind {a[2]} / "
                f"版 {a[3]}.{a[4]}  （この型は {index} 番目）")

    say("オブジェクト自身の型情報", object_typelib)

    print("== 呼び方を試す ==")

    def shapes():
        """**当てにいかず、全部試して、どれが通ったかを言う。**

        登録まわりが全部白なのに呼べないなら、残るのは呼び方。
        引数の並び・`[out]` の位置・schema の有無で組み合わせがあるので、
        ひとつずつ当たって**結果を並べる**。
        """
        mod = gc().EnsureModule(ONENOTE_TYPELIB, 0, 1, 1)
        raw = cli().Dispatch("OneNote.Application")
        app = _wrap_with_generated(mod, raw) or raw
        names = [n for n in dir(mod)
                 if isinstance(getattr(mod, n, None), type)
                 and hasattr(getattr(mod, n), "GetHierarchy")]
        dispid = None
        if names:
            try:
                dispid = getattr(mod, names[0]).GetHierarchy.__defaults__
            except Exception:  # noqa
                dispid = None
        out = []
        tries = [
            ("包んで (起点, 深さ, schema)", lambda: app.GetHierarchy("", HS_PAGES, XS_2013)),
            ("包んで (起点, 深さ)", lambda: app.GetHierarchy("", HS_PAGES)),
            ("素で (起点, 深さ, [out], schema)",
             lambda: raw.GetHierarchy("", HS_PAGES, "", XS_2013)),
            ("素で (起点, 深さ, [out])", lambda: raw.GetHierarchy("", HS_PAGES, "")),
        ]
        for label, fn in tries:
            try:
                got = fn()
                ok = isinstance(got, str) and got.lstrip().startswith("<")
                out.append(f"{label}: {'**通った**（' + str(len(got)) + ' 字）' if ok else repr(got)[:60]}")
            except Exception as e:  # noqa
                out.append(f"{label}: ✗ {e}")
        return "\n      " + "\n      ".join(out)

    say("GetHierarchy の呼び方", shapes)

    def generated(major, minor):
        def f():
            mod = gc().EnsureModule(ONENOTE_TYPELIB, 0, major, minor)
            names = [n for n in dir(mod)
                     if isinstance(getattr(mod, n, None), type)
                     and hasattr(getattr(mod, n), "GetHierarchy")]
            return f"{getattr(mod, '__name__', '?')} / GetHierarchy を持つ型: {names or '（無い）'}"
        return f

    say("EnsureModule(1.1)", generated(1, 1))
    say("EnsureModule(1.0)", generated(1, 0))

    def dispatched():
        raw = cli().Dispatch("OneNote.Application")
        return f"{type(raw).__name__} / GetHierarchy: {hasattr(raw, 'GetHierarchy')}"

    say("Dispatch が返すもの", dispatched)

    def wrapped(major, minor):
        def f():
            mod = gc().EnsureModule(ONENOTE_TYPELIB, 0, major, minor)
            raw = cli().Dispatch("OneNote.Application")
            w = _wrap_with_generated(mod, raw) or raw
            if not hasattr(w, "GetHierarchy"):
                return f"{type(w).__name__} / GetHierarchy が見えない"
            # **見えるだけでは足りない ── 呼ぶ。**
            xml = get_hierarchy(w)
            return f"{type(w).__name__} / 呼べた（{len(xml)} 字）"
        return f

    say("1.1 で包んで、呼んでみる", wrapped(1, 1))
    say("1.0 で包んで、呼んでみる", wrapped(1, 0))
    return 0


def check():
    """**一画面で終わる診断。**

    `--probe` は 29 行出す。繋がらない端末の姿を残らず並べる道具で、それは
    それで要るのだが、**現場から手で打ち直して渡す人には長すぎる。**
    読むのは人なので、要るのは「どこが噛み合っていないか」と「次に何を
    するか」だけ ── それを数行にする。

    繋がったなら一行。繋がらないなら、決め手と、**呼んだときの答え**と、
    次の一手。答えを捨てないのは、**登録が白のときに残る手がかりが
    そこしかない**から。
    """
    me = 64 if sys.maxsize > 2 ** 32 else 32
    office = _office_platform()
    tree = _typelib_tree()
    server, server_here = _local_server()
    store = _store_onenote()

    print(f"Python {sys.version_info[0]}.{sys.version_info[1]} / {me} bit")
    print(f"Office {office or '（読めない）'}")
    # **版ごとに出す。** 束ねると「1.0 に win64、1.1 に win32」が
    # 「両方ある」に見え、こちらが白だと読み違える。
    print("型ライブラリの枝: "
          + ("  ".join(f"{v}={' '.join(sorted(a)) or '（空）'}" for v, a in sorted(tree.items()))
             or "（読めない）"))
    print("COM サーバー: "
          + {True: "ある", False: "**無い**", None: "（読めない）"}[server_here]
          + (f"  {server}" if server else ""))
    if store:
        # **入っていること自体は困らない。** 見えるのは 365 側に開いている
        # ぶんだけ、というだけ ── 責める話ではないので、そう書く。
        print(f"ストア版も入っている（365 側に開いたものだけが写せる）: {store}")

    try:
        app, first = connect_onenote()
    except SystemExit as e:
        troubles = getattr(e, "troubles", [])
        if troubles:
            print()
            print("呼んだときの答え:")
            for t in troubles:
                print(" " + t.rstrip()[:120])
        elif isinstance(e.code, str):
            # **答えが無いのに止まったなら、止めた言い分がある。** pywin32 が
            # 入っていない、鎖に断られた ── 飲み込むと、いちばん短い道で
            # 分かることを黙ることになる。
            print()
            for line in e.code.splitlines():
                print(line)
        print()
        print("次の一手:")
        for line in _next_move(me, office, tree, server_here, troubles):
            print(f"  {line}")
        return 1

    root = ET.fromstring(first)
    books = [nb.get("name", "") for nb in root.findall("one:Notebook", NS)]
    print()
    print(f"繋がった。開いているノートブック {len(books)} 冊: "
          + ("、".join(books) if books else "（一冊も無い）"))
    if not books:
        print("  → " + _no_books_hint(store))
    return 0


def _next_move(me, office, tree, server_here, troubles=()):
    """**繋がらないときに、次にやることを一つだけ言う。**

    並べると人は選べない。当てはまるものを、効きそうな順に一つ。
    """
    # **在るか無いかが先。** ファイルが一つも無いなら、番号が何であれ
    # 答えはそれ ── そこへ bit の話を被せると、要らない道へ人を送る。
    if _lib_files_gone():
        return _lib_paths_note() + _gone_note()
    code, said = _from_answer(troubles)
    if said:
        if code == "-2147312566":
            # **読めないと言われた道を、その場で出す。** ここで `--probe` へ
            # 送ると、29 行を手で打ち直させることになる。
            iface = _interface_note(troubles)
            if iface:
                # **呼ぶ瞬間まで行っているなら、読めないという話ではない。**
                # そこへ当て推量を足すと、確かなほうが埋もれる。
                return said + _lib_paths_note(speculate=False) + iface
            said = said + _lib_paths_note() + _cant_load_next(me)
        return said
    want = "win64" if me == 64 else "win32"
    # **見るのは、こちらが読みにいく版。** 1.1 → 1.0 の順に試すので、
    # 1.1 に枝が無ければそこで転ぶ ── ほかの版に有っても助けにならない。
    for ver in ("1.1", "1.0"):
        arches = tree.get(ver)
        if arches is None:
            continue
        if want in arches:
            break                      # この版は読める ── 枝の話ではない
        elsewhere = [v for v, a in tree.items() if want in a]
        out = [f"型ライブラリの版 {ver} に {want} の枝が無い"
               f"（あるのは {sorted(arches) or '何も'}）。"]
        if elsewhere:
            out.append(f"{want} が有るのは版 {elsewhere} のほう ── "
                       f"こちらは {ver} から読むので、そちらは助けにならない。")
        have = _typelib_path("win32" if want == "win64" else "win64")
        if have and office and office.lower() in ("x64", "x86"):
            office_bits = 64 if office.lower() == "x64" else 32
            if office_bits == me:
                hver, lcid, path = have
                add, undo = _reg_lines(ver, lcid, want, path)
                out += [f"Office も Python も {me} bit なので、枝を足せば読める。",
                        "この一行（HKCU なので管理者は要らない）:", add,
                        "戻すとき:", undo,
                        "レジストリを触るので、会社の決まりだけ先に確かめて。"]
                return out
            out.append(f"Office は {office} ── Python を {office_bits} bit に"
                       "合わせるのがいちばん確か。")
        return out
    if server_here is False:
        # **見て「無い」と分かったときだけ言う。** 見られなかったのは、
        # 無かったのとは違う。
        return ["COM サーバーの実体が、登録の指す場所に無い。",
                "デスクトップ版 OneNote を入れ直すか、修復する。"]
    return ["登録は白。残るのは権限か、OneNote 自身か、呼び方。",
            "管理者の窓で走らせていないか（OneNote と権限を揃える・普通の窓で）。",
            "デスクトップ版 OneNote を先に手で開き、サインインまで済ませる。",
            "%LOCALAPPDATA%" + chr(92) + "Temp" + chr(92) + "gen_py を消してからもう一度。",
            "それでも駄目なら --probe を（長いが、全部出る）。"]


# **答えの中の数字は、そのまま原因を名指しする。** COM は落ちた理由を
# 番号で言う ── 読み方を知っていれば、推し量らずに済む。
_ANSWERS = [
    ("-2147319779", ["型ライブラリが読めない（TYPE_E_LIBNOTREGISTERED）。",
                     "枝は有っても、指す先が読めていない ── 版の取り違えか、",
                     "実体が 32 bit のものしか無い。--probe の「版ごとの実体」を見る。"]),
    ("-2147312566", ["型ライブラリ／DLL の読み込みエラー（TYPE_E_CANTLOADLIBRARY）。",
                     "登録は白いのに、指す先が読めていない。"]),
    ("-2146959355", ["権限のずれ（0x80080005・サーバーの実行に失敗しました）。",
                     "管理者の窓からは、昇格していない OneNote に繋げない。普通の窓で叩く。",
                     "タスク スケジューラなら「最上位の特権で実行する」も外す。"]),
    ("-2147221164", ["その CLSID が登録されていない（REGDB_E_CLASSNOTREG）。",
                     "デスクトップ版 OneNote を修復する（ストア版は COM を持たない）。"]),
    ("-2147221005", ["ProgID が引けない（Invalid class string）。",
                     "デスクトップ版 OneNote が入っていない ── ストア版だけでは繋がらない。"]),
    ("-2147024891", ["拒まれた（アクセスが拒否されました）。権限のずれ ── 普通の窓で叩く。"]),
]


def _from_answer(troubles):
    """呼んだときの答えから、分かることがあれば言う。無ければ黙る。

    返すのは `(番号, 言うこと)`。番号を返すのは、**その先に出す事実が
    番号ごとに違う**から ── 読めないと言われたなら、読めないその道を出す。
    """
    joined = " ".join(troubles)
    # **数えてから決める。** 先に並べた順で拾うと、**四つのうち三つが
    # 同じことを言っているのに、一つだけ違う答えを採る**（現場で実際に
    # そうなった ── 三つが「サーバーの実行に失敗」なのに、一つだけの
    # 「登録されていません」を返した）。同じ数なら、表の順。
    best, n_best = None, 0
    for needle, _said in _ANSWERS:
        n = joined.count(needle)
        if n > n_best:
            best, n_best = needle, n
    if best is None:
        return None, []
    return best, list(dict(_ANSWERS)[best])


def _lib_paths_note(speculate=True):
    """**枝が何を指していて、それが在るか。** 版はぜんぶ出す。

    一つだけ見せると、落ちた版と違うものを見せうる（依頼 580 で束ねて
    見て踏んだのと、同じ形）。
    """
    out = []
    seen = {}
    for ver, arch, path, view in _typelib_values():
        real = _strip_resource_index(path)
        ok = bool(real and os.path.exists(real))
        seen.setdefault(ver, {})[arch] = path
        where = f" [{view}]" if view else ""
        out.append(f"{ver} / {arch}{where} = {path}  → {'ある' if ok else '**無い**'}")
    if _lib_files_gone() or not speculate:
        # 在処が無いとき、そして**もっと確かなことが分かっているとき**は、
        # 写しの当て推量は要らない ── 並べると、確かなほうが埋もれる。
        return out
    for ver, by in seen.items():
        if len(by) == 2 and len(set(by.values())) == 1:
            out.append(f"{ver} は win32 と win64 が**同じ道**を指している ── 片方は写し。")
            out.append("その実体が 32 bit のものなら、64 bit からは読めない（足しても直らない）。")
            break
    return out


def _lib_files_gone():
    """**登録が指す先に、ファイルが一つも無いか。**

    そうなら、話は bit でも枝でもない ── 登録が**居ないものを指している**。
    枝が何本あっても、読める道は一本も無い。
    """
    got = _typelib_values()
    if not got:
        return False
    for _ver, _arch, path, _view in got:
        real = _strip_resource_index(path)
        if real and os.path.exists(real):
            return False
    return True


def _gone_note():
    """ファイルが一つも無いときに言うこと。入っていないのか、よそに在るのか。"""
    out = ["**登録が指す先に、ファイルが一つも無い。** bit の話でも枝の話でもない。"]
    found = _onenote_exe()
    if found:
        out.append(f"デスクトップ版の実体は、ここに在る: {found}")
        out.append("登録のほうが古い ── Office の修復で焼き直す:")
        out.extend("  " + l for l in _repair_lines())
    else:
        out.append("探した場所のどこにも ONENOTE.EXE が無い ──")
        out.append("**デスクトップ版 OneNote が入っていない**（ストア版は COM を持たない）。")
        out.append("Microsoft 365 から OneNote を入れる。")
    return out


def _interface_registration():
    """**取り次ぎ側が見ている登録**（依頼 573 の `--probe` の節を、`--check` へ）。

    別プロセスの COM を呼ぶと、呼び出しは取り次がれる（marshaling）。
    取り次ぐ側は ``HKCR\\Interface\\{IID}\\TypeLib`` を見て「どの型ライブラリの
    どの版か」を引く ── **ここが壊れた版を指していれば、型ライブラリ本体が
    読めていても、呼んだ瞬間に落ちる。**

    「繋がったのに、呼ぶと落ちる」のいちばん奥の理由がここに出る。
    返すのは `(型の名前, IID, 型ライブラリ, 版)`。引けなければ None。
    """
    try:
        import winreg
        from win32com.client import gencache
    except ImportError:
        return None
    try:
        mod = gencache.EnsureModule(ONENOTE_TYPELIB, 0, 1, 1)
        names = [n for n in dir(mod)
                 if isinstance(getattr(mod, n, None), type)
                 and hasattr(getattr(mod, n), "GetHierarchy")]
        if not names:
            return None
        iid = str(getattr(mod, names[0]).CLSID)
        key = winreg.OpenKey(winreg.HKEY_CLASSES_ROOT,
                             "Interface" + chr(92) + iid + chr(92) + "TypeLib")
        lib = winreg.QueryValue(key, None)
        try:
            ver = winreg.QueryValueEx(key, "Version")[0]
        except OSError:
            ver = None
        return names[0], iid, lib, ver
    except Exception:  # noqa
        return None


def _repair_lines():
    """Office の登録を焼き直す道 ── **網の要らないほうを先に言う。**

    ずっと「オンライン修復」と言っていたが、**あの端末は網に出られない**
    （依頼 582）。勧めていたのは、そこではできないことだった。
    **クイック修復は網が要らない**（手元のファイルから直す）ので、そちらが先。
    """
    return ["設定 →「アプリ」→ Microsoft 365 →「変更」→ **クイック修復**。",
            "**クイック修復は網が要らない**（手元のファイルから登録を焼き直す）。",
            "それでも駄目なら「オンライン修復」だが、そちらは網が要る。"]


def _interface_note(troubles):
    """呼んだ瞬間に落ちているなら、取り次ぎ側の登録を出す。

    **「繋がらない」と「繋がるのに呼べない」は、別の話。** 後者のときだけ
    ここを見る ── 束ね方をいくら変えても、取り次ぐ側が壊れた版を指していれば
    同じところで落ちる。
    """
    if not any("呼ぶと落ちる" in t for t in troubles):
        return []
    got = _interface_registration()
    if not got:
        return []
    name, iid, lib, ver = got
    out = ["", "**繋がってはいる ── 落ちているのは呼んだ瞬間。**",
           f"取り次ぎ側が見ている登録: {name} → 型ライブラリ {lib} の版 {ver or '（無い）'}"]
    # 呼び方をいくら変えても、取り次ぐ側が壊れた版を指していれば同じ。
    broken = [v for v in ("1.0", "1.1") if f"型ライブラリ {v} を名指し" in " ".join(troubles)
              and f"型ライブラリ {v} を名指し: (-2147312566" in " ".join(troubles)]
    if ver and ver in broken:
        good = "1.1" if ver == "1.0" else "1.0"
        b = chr(92)
        root = "HKCU" + b + "Software" + b + "Classes" + b + "Interface" + b + iid + b + "TypeLib"
        out += [f"**その版（{ver}）は、名指しでも読めなかったほう。**",
                f"取り次ぎ側に {good} を見させる（HKCU なので管理者は要らない）:",
                f'  reg add "{root}" /v Version /d {good} /f',
                "戻すとき:",
                f'  reg delete "HKCU{b}Software{b}Classes{b}Interface{b}{iid}" /f',
                "レジストリを触るので、会社の決まりだけ先に確かめて。"]
    else:
        # **取り次ぎ側は生きている版を指している。それでも呼ぶと落ちる。**
        # こちら側でできることは、もう無い ── 名前を引くのも、呼びを受けるのも
        # OneNote がやる。そこが自分の型ライブラリを読めていない。
        me = os.path.basename(__file__)
        out += [f"**その版（{ver}）は生きている ── 取り次ぎ側は壊れていない。**",
                "こちらの束ね方で直せるところは、もう無い。",
                "名前を引くのも呼びを受けるのも OneNote 自身なので、",
                "**落ちているのは OneNote の側** ── 自分の型ライブラリを読めていない。",
                "残っている手を、安いほうから:",
                "  1. 作り置きを捨ててもう一度（捨てても困らない・作り直される）:",
                "     scripts" + chr(92) + me + " --forget",
                "  2. Office の登録を焼き直す:"]
        out.extend("     " + l for l in _repair_lines())
        out += ["  3. それでも同じなら `--probe`（29 行・写真で構わない）── ",
                "     そこにだけ出るものが三つある: 版ごとの実体、**生きている相手が",
                "     名乗る型ライブラリ**、そして呼び方を四通り試した結果。"]
    return out


def _cant_load_next(me):
    """`TYPE_E_CANTLOADLIBRARY` のとき、**走っている側で言うことが変わる。**

    64 bit なら、まず 32 bit で試す価値がある。**だが 32 bit でも同じ答えが
    返ったなら、bit の話ではない** ── そこで「32 bit を使え」と言い続けるのは、
    一度通った道へまた送ること。会社の端末で実際にそうなった。
    """
    if me == 64:
        got = _thirty_two_note()
        if got:
            return got
        return ["32 bit の Python でも試す（out-of-process なので OneNote は 64 bit のままでよい）。",
                "オフラインなら持ち込むのは二つ ── installer と pywin32 の wheel",
                "（docs の「32 bit の Python を、この道具のためだけに」）。"]
    return (["**32 bit でも同じ答えなら、bit の話ではない。**",
             "ファイルは在るのに、その資源に型ライブラリが入っていないか、読めない。",
             "`ファイルから型ライブラリを読む` の行に、道ごとの言い分が出ている ──",
             "それも駄目なら Office の登録を焼き直す:"]
            + ["  " + l for l in _repair_lines()])


def _thirty_two_note():
    """32 bit が居るなら、そちらで叩き直す一行。"""
    tag = _thirty_two_bit_here()
    if not (tag and sys.maxsize > 2 ** 32):
        return []
    me = os.path.basename(__file__)
    return [f"**32 bit の Python がこの機械に居る（{tag}）。そちらで叩き直す:**",
            "  py -" + tag + " scripts" + chr(92) + me + " --check"]


def _reg_lines(ver, lcid, want, path):
    """足す一行と、戻す一行。**道はこちらで埋める。**

    繋がらない端末の前に立っている人に「docs を見て `--probe` の値を
    そこから写して」と言うのは、注文としておかしい。
    """
    b = chr(92)
    root = "HKCU" + b + "Software" + b + "Classes" + b + "TypeLib" + b + ONENOTE_TYPELIB
    add = (f'  reg add "{root}{b}{ver}{b}{lcid}{b}{want.capitalize()}"'
           f' /ve /d "{path}" /f')
    undo = f"  reg delete " + chr(34) + root + chr(34) + " /f"
    return add, undo


def _no_books_hint(store):
    """繋がったのに一冊も見えないとき。**ここがストア版との境目。**

    二つ入っていること自体は困らない。困るのは**どちらで開いているか**で、
    こちらから見えるのはデスクトップ版に開いているものだけ。
    """
    if store:
        return ("こちらから見えるのは**デスクトップ版（365）に開いているノートブック"
                "だけ**。ストア版（OneNote for Windows 10）は COM を持たないので、"
                "そちらで開いていても写せない ── 写したいものは 365 側でも開いておく。")
    return "OneNote 上で、写したいノートブックを開く（閉じているものは見えない）。"


def _wrap_with_generated(mod, raw):
    """makepy が作った**皮をかぶせる**。

    `EnsureModule` が通っても、`Dispatch` が早い束ねを返すとは限らない ──
    ProgID → CLSID → 生成された型、の対応が引けないと、遅い束ねのまま返る
    （**繋がったが `GetHierarchy` が見えない**）。それなら、生成された型の
    ほうから包みにいく。

    型の名前は決め打たない（`IApplication` とは限らないし、版で変わる）──
    **`GetHierarchy` を持っている型を探す。** 探しているものの名前で探すのが、
    いちばん壊れにくい。
    """
    if mod is None:
        return None
    ole = getattr(raw, "_oleobj_", raw)
    for name in dir(mod):
        cls = getattr(mod, name, None)
        if not isinstance(cls, type) or not hasattr(cls, "GetHierarchy"):
            continue
        for candidate in (ole, raw):
            try:
                wrapped = cls(candidate)
            except Exception:  # noqa
                continue
            if hasattr(wrapped, "GetHierarchy"):
                log.debug("makepy の皮をかぶせた: %s", name)
                return wrapped
    return None


class CannotConnect(SystemExit):
    """繋げなかった ── **四つの呼び方が、それぞれ何と言ったか**を持つ。

    `SystemExit` のままなので、上の `main` の扱いは変わらない。持たせたのは
    `--check` のため: **登録が白なら、残る手がかりはこの答えしかない。**
    捨てると、いちばん知りたいところで何も言えなくなる。
    """

    def __init__(self, message, troubles):
        super().__init__(message)
        self.troubles = list(troubles)


def connect_onenote():
    """OneNote に繋ぐ。**束ね方を三通り試して、話が通じたものを採る。**

    素の `Dispatch`（遅い束ね）だけだった版は、会社の Windows で
    `AttributeError: OneNote.Application.GetHierarchy` になった ── **繋がって
    いるのに、メソッドの名前が引けない。** OneNote の型ライブラリの登録には
    中身のない `1.0` の枝が混ざっていて、pywin32 がそれを掴むと名前を引けなく
    なる（pywin32 の issue 1488）。レジストリを消せば直るが、**会社の端末は
    グループポリシーが書き戻す**ので、こちら側で避ける ── 版を明示して
    `EnsureModule` すれば、偽の枝を跨いで本物（1.1）を読む。

    どれで繋がったかは `-v` で出す。**繋がったことと、話が通じることは別**
    なので、`GetHierarchy` が見えるところまで確かめてから返す。
    """
    try:
        import win32com  # noqa
    except ImportError:
        sys.exit("pywin32 が要ります。\n" + "\n".join("  " + l for l in wheel_hint()))

    # **`win32com.client` を読む前に。** 逃がすならここでしか逃がせない。
    gen_path, moved = _gen_py_somewhere_writable()
    if moved:
        log.debug("makepy の作り置き先を移した: %s", gen_path)

    import win32com.client  # noqa
    from win32com.client import gencache

    # gencache は読み込みのときに「書けるか」を見て `is_readonly` を決める。
    # こちらで書ける場所へ移したのに読めないと言われたら、言い直させる。
    try:
        if getattr(gencache, "is_readonly", False) and _can_write(gen_path):
            gencache.is_readonly = False
            gencache.Rebuild()
    except Exception as e:  # noqa
        log.debug("gencache の言い直しに失敗（続ける）: %s", e)

    def by_typelib(major, minor):
        def make():
            try:
                mod = gencache.EnsureModule(ONENOTE_TYPELIB, 0, major, minor)
            except ImportError:
                # 書いた直後のものが見つからないことがある（作り置きが古い）。
                importlib.invalidate_caches()
                mod = gencache.EnsureModule(ONENOTE_TYPELIB, 0, major, minor)
            raw = win32com.client.Dispatch("OneNote.Application")
            if hasattr(raw, "GetHierarchy"):
                return raw
            return _wrap_with_generated(mod, raw) or raw
        return make

    def by_file():
        """**登録を通さず、ファイルから型ライブラリを読む。**

        登録は白く、指す先のファイルも在るのに `TYPE_E_CANTLOADLIBRARY` が
        返る端末がある（会社の端末・2026-09-15。32 bit でも 64 bit でも同じ
        答えだったので、bit の話ではない）。`LoadRegTypeLib` が転ぶのと、
        **その資源に型ライブラリが入っていない**のは別のことなので、
        道を名指しして読んでみる ── `gencache` が言う「makepy を手で
        走らせろ」を、こちらで走らせるのがこれ。

        読めたら、その型から皮を作って包む。読めなければ、どの道で
        どう転んだかを言う ── **そこで初めて「資源が無い」と分かる。**
        """
        import pythoncom
        from win32com.client import makepy
        trouble = []
        for ver, _arch, path, _view in _typelib_values():
            real = _strip_resource_index(path)
            if not (real and os.path.exists(real)):
                continue
            try:
                pythoncom.LoadTypeLib(path)
            except Exception as e:  # noqa
                trouble.append(f"{ver} {path}: {e}")
                continue
            makepy.GenerateFromTypeLibSpec(path)
            raw = win32com.client.Dispatch("OneNote.Application")
            if hasattr(raw, "GetHierarchy"):
                return raw
            mod = gencache.EnsureModule(ONENOTE_TYPELIB, 0, 1, 1)
            return _wrap_with_generated(mod, raw) or raw
        raise RuntimeError("ファイルから読めない ── " + " / ".join(trouble or ["道が無い"]))

    def by_gencache():
        return gencache.EnsureDispatch("OneNote.Application")

    def by_dispatch():
        return win32com.client.Dispatch("OneNote.Application")

    def arch_hint():
        """**分かるなら、答えのほうを言う。**

        レジストリを見れば「この bit では無理」と分かることがある ──
        そのときに「繋がりません」で終わるのは、知っていることを黙っている
        のと同じ。
        """
        v = _arch_verdict(_registered_arches())
        return ("\n" + "\n".join(v)) if v else ""

    troubles = []
    # **版は一つとは限らない。** 会社の端末には `1.0` と `1.1` の両方が登録されて
    # いた。どちらが本物かはレジストリの見た目では決まらない（片方は実体を
    # 指していない）ので、**両方試して、実際に答えが返ったほうを採る。**
    for how, make in (("型ライブラリ 1.1 を名指し", by_typelib(1, 1)),
                      ("型ライブラリ 1.0 を名指し", by_typelib(1, 0)),
                      ("ファイルから型ライブラリを読む", by_file),
                      ("gencache に任せる", by_gencache),
                      ("素の Dispatch（遅い束ね）", by_dispatch)):
        try:
            app = make()
        except Exception as e:  # noqa
            troubles.append(f"  {how}: {e}")
            continue
        if not hasattr(app, "GetHierarchy"):
            troubles.append(f"  {how}: 繋がったが GetHierarchy が見えない")
            continue
        # **見えるだけでは足りない ── 実際に訊いてみる。**
        # ここを `hasattr` で済ませていた版は、皮はかぶさっているのに呼ぶと
        # `ライブラリは登録されていません` になる組み合わせを掴んで、
        # そのまま先へ進んでいた。**合格の合図を、答えそのものにする。**
        try:
            first = get_hierarchy(app)
        except Exception as e:  # noqa
            troubles.append(f"  {how}: 呼ぶと落ちる ── {e}")
            continue
        log.debug("OneNote に繋がった（%s）", how)
        return app, first

    raise CannotConnect("OneNote (デスクトップ版) に接続できません:\n" + "\n".join(troubles) + """

よくある順に:
  1. 管理者の窓で走らせている ── OneNote と権限を揃える（普通の窓で叩く）
  2. OneNote を先に起動していない ── 手で開き、写すノートブックを開いておく
  3. gen_py の作り置きが壊れている ── %LOCALAPPDATA%\\Temp\\gen_py を消す
  4. ストア版にしか開いていない ── 見えるのはデスクトップ版（365）に開いたぶんだけ

まず `--check` を。数行で言います（`--probe` は全部出しますが、長い）。

`--probe` を付けると、この端末で何が起きているかを並べます。
そのまま貼ってもらえれば、推し量らずに直せます。""" + arch_hint(), troubles)


def _xml_call(func, *variants):
    """OneNote の `[out]` 引数は、束ね方で渡し方が変わる。

    **早い束ね**（`EnsureModule` / `EnsureDispatch`）では `[out]` が戻り値に
    なるので渡さない。**遅い束ね**では置き場所を渡す ── しかもその位置は
    メソッドごとに違う（`GetHierarchy` は 3 番目、`GetPageContent` は 2 番目）。

    片方に決め打つと、片方の端末でだけ動くものになる。**両方試して、XML が
    返ったほうを採る。** 例外だけで見分けないのは、間違った位置に渡しても
    例外にならず「XML でない何か」が返ることがあるから。
    """
    trouble = None
    for args in variants:
        try:
            out = func(*args)
        except Exception as e:  # noqa
            trouble = e
            continue
        if isinstance(out, tuple):   # (戻り値, out) で返る束ね方もある
            out = next((v for v in out
                        if isinstance(v, str) and v.lstrip().startswith("<")), None)
        if isinstance(out, str) and out.lstrip().startswith("<"):
            return out
        trouble = ValueError(f"XML ではないものが返った: {str(out)[:60]!r}")
    raise trouble


# **schema は明示する。** Microsoft の資料がそう言っている ── 空で渡すと
# OneNote 側が「いまの版」を探しにいき、その版が登録されていないと
# `ライブラリは登録されていません` になりうる。明示したほうを先に試す。
def get_hierarchy(app):
    # 早い束ね: (起点, 深さ, schema) / 遅い束ね: [out] は 3 番目
    return _xml_call(app.GetHierarchy,
                     ("", HS_PAGES, XS_2013),
                     ("", HS_PAGES, "", XS_2013),
                     ("", HS_PAGES),
                     ("", HS_PAGES, ""))


def _get_page(app, page_id, info):
    # 早い束ね: (ページ, 何を含めるか, schema) / 遅い束ね: [out] は **2 番目**
    return _xml_call(app.GetPageContent,
                     (page_id, info, XS_2013),
                     (page_id, "", info, XS_2013),
                     (page_id, info),
                     (page_id, "", info))


def sync_notebooks(app, root, filters, wait):
    """OneNote に「いま同期しろ」と頼む（`SyncHierarchy`）。

    一時間ごとに回すなら、これが要る。**この機械で開いているだけのノートブックは、
    誰かが電話や Web で直しても、OneNote が同期するまで古いまま**で、こちらは
    「変わっていない」と見て飛ばしてしまう。頼んでおけば次の回には届いている。

    `SyncHierarchy` は頼むだけで、終わるのを待たない。`--sync-wait` の秒だけ
    待ってから読み直すが、**間に合わなければ次の回で拾う**（一時間ごとなので、
    遅れても一時間。待ちを長くして毎回粘るより、そのほうが安い）。
    """
    ids = []
    for nb in root.findall("one:Notebook", NS):
        name = nb.get("name", "")
        if filters and not any(f.lower() in name.lower() for f in filters):
            continue
        ids.append((name, nb.get("ID")))
    for name, nb_id in ids:
        try:
            app.SyncHierarchy(nb_id)
            log.info("同期を頼んだ: %s", name)
        except Exception as e:  # noqa
            log.warning("同期を頼めない %s: %s", name, e)
    if ids and wait > 0:
        time.sleep(wait)
    return bool(ids)


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
    return name[:120] or fallback


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


def strip_ns(tag):
    return tag.split("}", 1)[-1]


# ---------------------------------------------------------------------------
# インライン変換（<one:T> の中身は HTML 風テキスト）
# ---------------------------------------------------------------------------
_SPAN = re.compile(r"<span([^>]*)>((?:(?!<span)(?!</span>).)*)</span>", re.S | re.I)
# `href` は括ってあれば**空白を含んでよい** ── 括りを見ずに空白で切ると、
# `file:///C:/My Documents/…` のようなリンクが途中で切れて、別の場所を指す。
_A = re.compile(r"""<a\s+[^>]*href=("[^"]*"|'[^']*'|[^\s>]+)[^>]*>(.*?)</a>""", re.S | re.I)
_TAG = re.compile(r"<[^>]+>")


def _wrap(inner, mark):
    # 前後の空白はマークの外に出す（**  text** は壊れる）
    lead = inner[: len(inner) - len(inner.lstrip())]
    trail = inner[len(inner.rstrip()):]
    core = inner.strip()
    if not core:
        return inner
    return f"{lead}{mark}{core}{mark}{trail}"


def _span_repl(m):
    attrs, inner = m.group(1).lower(), m.group(2)
    if "font-weight:bold" in attrs or "font-weight: bold" in attrs:
        inner = _wrap(inner, "**")
    if "font-style:italic" in attrs or "font-style: italic" in attrs:
        inner = _wrap(inner, "*")
    if "line-through" in attrs:
        inner = _wrap(inner, "~~")
    return inner


def plain_md(s: str) -> str:
    """札だけ落として、字はそのまま。

    **コードの枠に入れる字は、これで取る。** `inline_md` を通すと太字の札が
    `**` になり、元のコードに無い字が混ざる（`if x > 0:` が `if **x** > 0:`）。
    枠の中で印は印として読まれないので、写したコードがそのままでは動かない。
    """
    if not s:
        return ""
    s = re.sub(r"<br\s*/?>", "\n", s, flags=re.I)
    s = _TAG.sub("", s)
    return html.unescape(s).replace("\xa0", " ")


def inline_md(s: str) -> str:
    if not s:
        return ""
    s = re.sub(r"<br\s*/?>", "  \n", s, flags=re.I)
    while True:
        new = _SPAN.sub(_span_repl, s)
        if new == s:
            break
        s = new
    s = _A.sub(lambda m: f"[{_TAG.sub('', m.group(2)).strip()}]"
                         f"({html.unescape(m.group(1).strip(chr(34) + chr(39)))})", s)
    s = _TAG.sub("", s)
    s = html.unescape(s).replace("\xa0", " ")
    return s


# ---------------------------------------------------------------------------
# ページ XML → Markdown
# ---------------------------------------------------------------------------
class PageConverter:
    def __init__(self, page_el, img_dir: Path, img_prefix: str, rel_img: str, with_images: bool):
        self.root = page_el
        self.img_dir = img_dir
        self.img_prefix = img_prefix
        self.rel_img = rel_img           # md から見た画像フォルダの相対の道
        self.with_images = with_images
        self.img_count = 0
        self.images: list[str] = []      # 書いた画像の名前（--prune のため）
        self.styles = {}                 # quickStyleIndex -> style name
        self.tags = {}                   # tagDef index -> name
        for qs in page_el.findall("one:QuickStyleDef", NS):
            self.styles[qs.get("index")] = qs.get("name", "")
        for td in page_el.findall("one:TagDef", NS):
            self.tags[td.get("index")] = td.get("name", "")

    # --- 本体 -----------------------------------------------------------
    def convert(self):
        out = []
        # ページ上の要素を Y 座標順に並べる（OneNote は絶対配置）
        blocks = []
        for child in self.root:
            tag = strip_ns(child.tag)
            if tag in ("Outline", "Image", "InkDrawing", "InsertedFile"):
                pos = child.find("one:Position", NS)
                y = float(pos.get("y", "0")) if pos is not None else 0.0
                x = float(pos.get("x", "0")) if pos is not None else 0.0
                blocks.append((y, x, child))
        blocks.sort(key=lambda t: (t[0], t[1]))

        for _, _, el in blocks:
            tag = strip_ns(el.tag)
            if tag == "Outline":
                ch = el.find("one:OEChildren", NS)
                if ch is not None:
                    out.extend(self.oechildren(ch, depth=0))
                out.append("")
            elif tag == "Image":
                out.append(self.image(el))
                out.append("")
            elif tag == "InkDrawing":
                out.append("> [インク: 変換対象外]")
                out.append("")
            elif tag == "InsertedFile":
                out.append(self.inserted_file(el))
                out.append("")

        text = "\n".join(out)
        text = re.sub(r"```\n```\n", "", text)         # 連続コード行を1ブロックに結合
        text = re.sub(r"\n{3,}", "\n\n", text).strip() + "\n"
        return text

    # --- OE（段落） -----------------------------------------------------
    def oechildren(self, oechildren_el, depth, in_list=False):
        lines = []
        for oe in oechildren_el.findall("one:OE", NS):
            lines.extend(self.oe(oe, depth, in_list))
        return lines

    def oe(self, oe, depth, in_list):
        lines = []
        style = self.styles.get(oe.get("quickStyleIndex"), "")
        indent = "  " * depth

        # リスト記号
        bullet = ""
        lst = oe.find("one:List", NS)
        if lst is not None:
            if lst.find("one:Number", NS) is not None:
                bullet = "1. "
            else:
                bullet = "- "

        # タグ（To Do はチェックボックスに、それ以外は先頭ラベル）
        tag_prefix = ""
        for tg in oe.findall("one:Tag", NS):
            name = self.tags.get(tg.get("index"), "")
            if "To Do" in name or "タスク" in name:
                done = tg.get("completed", "false") == "true"
                tag_prefix += "[x] " if done else "[ ] "
                if not bullet:
                    bullet = "- "
            elif name:
                tag_prefix += f"`#{name}` "

        # テキスト（複数の one:T は連結）
        raw = [t.text or "" for t in oe.findall("one:T", NS)]
        text = "".join(inline_md(x) for x in raw)

        if text.strip() or tag_prefix:
            if style.startswith("h") and style[1:].isdigit() and not bullet:
                level = min(int(style[1:]), 6)
                lines.append(f"{'#' * level} {text.strip()}")
            elif style == "code" and not bullet:
                lines.append("```\n" + "".join(plain_md(x) for x in raw) + "\n```")
            elif style == "cite" and not bullet:
                lines.append(f"> {text.strip()}")
            elif bullet:
                lines.append(f"{indent}{bullet}{tag_prefix}{text}")
            else:
                lines.append(f"{indent}{tag_prefix}{text}" if depth else f"{tag_prefix}{text}")
        elif not any(strip_ns(c.tag) in ("Table", "Image", "OEChildren", "InkDrawing", "InsertedFile") for c in oe):
            lines.append("")  # 空行

        # 表
        for tbl in oe.findall("one:Table", NS):
            lines.append("")
            lines.extend(self.table(tbl))
            lines.append("")

        # 画像
        for img in oe.findall("one:Image", NS):
            lines.append(f"{indent}{self.image(img)}")

        for _ink in oe.findall("one:InkDrawing", NS) + oe.findall("one:InkParagraph", NS):
            lines.append(f"{indent}> [インク: 変換対象外]")

        for f in oe.findall("one:InsertedFile", NS):
            lines.append(f"{indent}{self.inserted_file(f)}")

        # 子段落
        children = oe.find("one:OEChildren", NS)
        if children is not None:
            lines.extend(self.oechildren(children, depth + 1, in_list=bool(bullet) or in_list))

        return lines

    # --- 表 -------------------------------------------------------------
    def table(self, tbl):
        rows = []
        for row in tbl.findall("one:Row", NS):
            cells = []
            for cell in row.findall("one:Cell", NS):
                ch = cell.find("one:OEChildren", NS)
                cell_lines = self.oechildren(ch, depth=0) if ch is not None else []
                # 升の中の改行は空白にする。前は `<br>` で繋いでいたが、
                # ambər は本文の札をぜんぶ字にして出すので、画面に
                # `<br>` という字がそのまま出ていた。Markdown の表に
                # 改行を入れる書き方は無いので、繋ぐしかない。
                cell_text = " ".join(l.strip() for l in cell_lines if l.strip())
                cells.append(cell_text.replace("|", "\\|"))
            rows.append(cells)
        if not rows:
            return []
        width = max(len(r) for r in rows)
        rows = [r + [""] * (width - len(r)) for r in rows]
        # **見出しの行があるかは、OneNote が知っている。** 無いのに 1 行目を
        # 見出しにすると、そのデータが一行、表から消える ── 画面の上では
        # 「一行目が濃いだけ」に見えるので、気づけない。Markdown の表は
        # 見出しの行を省けないので、無いときは空で置く。
        if tbl.get("hasHeaderRow") == "true":
            head, body = rows[0], rows[1:]
        else:
            head, body = [""] * width, rows
        out = ["| " + " | ".join(head) + " |", "|" + " --- |" * width]
        for r in body:
            out.append("| " + " | ".join(r) + " |")
        return out

    # --- 画像 -----------------------------------------------------------
    def image(self, img):
        alt = inline_md(img.get("alt", "") or "").strip() or "image"
        if not self.with_images:
            return f"![{alt}](<画像: 出力スキップ>)"
        data_el = img.find("one:Data", NS)
        if data_el is None or not (data_el.text or "").strip():
            return f"![{alt}](<画像: データなし>)"
        fmt = (img.get("format") or "png").lower()
        ext = {"jpg": "jpg", "jpeg": "jpg", "png": "png", "gif": "gif", "bmp": "bmp", "emf": "emf", "wmf": "wmf"}.get(fmt, fmt)
        self.img_count += 1
        fname = f"{self.img_prefix}_{self.img_count:03d}.{ext}"
        self.img_dir.mkdir(parents=True, exist_ok=True)
        try:
            raw = base64.b64decode(data_el.text)
            at = self.img_dir / fname
            # 同じ中身なら書かない（同期に、触っていない画像の更新だけが流れないように）。
            if not (at.exists() and at.stat().st_size == len(raw) and at.read_bytes() == raw):
                at.write_bytes(raw)
            self.images.append(fname)
        except Exception as e:  # noqa
            log.warning("画像デコード失敗 %s: %s", fname, e)
            return f"![{alt}](<画像: デコード失敗>)"
        return f"![{alt}]({self.rel_img}/{fname})"

    def inserted_file(self, f):
        name = f.get("preferredName") or os.path.basename(f.get("pathSource", "") or "") or "file"
        return f"添付ファイル: {name}"


# ---------------------------------------------------------------------------
# 前書き ── ambər が読む `title:` と `created:` を持つ
# ---------------------------------------------------------------------------
_FM_RE = re.compile(r"\A---\n(.*?)\n---\n", re.S)


def local_date(stamp):
    """OneNote の時刻を、**この機械の日付**に。

    返ってくるのは UTC（`2026-09-01T23:00:00.000Z`）なので、頭から 10 字を
    切ると東京では朝 9 時前に書いたものが前の日に並ぶ。日付は人が読むもの
    なので、人の居る時刻で。
    """
    if not stamp:
        return ""
    try:
        t = datetime.strptime(stamp[:19], "%Y-%m-%dT%H:%M:%S")
    except ValueError:
        return stamp[:10]            # 読めない形は、そのまま頭を取る
    return t.replace(tzinfo=timezone.utc).astimezone().strftime("%Y-%m-%d")


def page_title(page_el, hierarchy_name):
    t = page_el.find("one:Title/one:OE/one:T", NS)
    if t is not None and (t.text or "").strip():
        return inline_md(t.text).strip()
    return hierarchy_name or "Untitled"


def frontmatter(title, path_parts, page_attr):
    # ambər の `created:` は `YYYY-MM-DD`（時刻つきの ISO も読めるが、揃えておく）。
    created = local_date(page_attr.get("dateTime"))
    lines = ["---",
             f'title: "{title.replace(chr(34), chr(39))}"',
             f"created: {created}",
             f"modified: {page_attr.get('lastModifiedTime', '')}",
             f"onenote_id: \"{page_attr.get('ID', '')}\"",
             f"onenote_path: \"{' / '.join(path_parts)}\"",
             "---", ""]
    return "\n".join(lines)


def read_head(path: Path) -> dict:
    """既にあるファイルの前書きから、`onenote_id` と `modified` だけ拾う。"""
    try:
        head = path.read_text(encoding="utf-8", errors="replace")[:2000]
    except OSError:
        return {}
    m = _FM_RE.match(head)
    if not m:
        return {}
    out = {}
    for ln in m.group(1).splitlines():
        if ":" not in ln:
            continue
        k, v = ln.split(":", 1)
        out[k.strip()] = v.strip().strip('"')
    return out


def place_for(parent_dir: Path, h_name: str, page_id: str) -> Path:
    """このページを書くファイル。**同じ親・同じ名前なら同じファイル**。

    その名前が別のページ（`onenote_id` が違う）に使われていたら `名前 (2).md`。
    前の版はファイルがあれば必ずずらしていて、走らせるたびに増えていた。
    """
    n = 1
    while True:
        cand = parent_dir / (f"{h_name}.md" if n == 1 else f"{h_name} ({n}).md")
        cand = shorten(cand)
        if not cand.exists():
            return cand
        head = read_head(cand)
        if not head.get("onenote_id") or head.get("onenote_id") == page_id:
            return cand
        n += 1


# ---------------------------------------------------------------------------
# 階層走査
# ---------------------------------------------------------------------------
def walk_section(app, section_el, out_dir: Path, path_parts, args, stats, written: set, keep: Keep):
    sec_name = sanitize(section_el.get("name"))
    if section_el.get("locked") == "true":
        log.warning("パスワード保護のためスキップ: %s / %s", " / ".join(path_parts), sec_name)
        stats["skipped_sections"] += 1
        # **見えなかった場所は、消えた場所ではない。** 鍵のかかったセクションの
        # 下は今回一枚も出てこないので、`--prune` に任せると前回の写しを
        # まるごと消してしまう（そして次の回も鍵は開かないので、二度と戻らない）。
        keep.add_dir(out_dir / sec_name)
        keep.add_pages_of(section_el)   # 鍵が開いていれば ID も拾える
        return
    sec_dir = out_dir / sec_name
    log.info("セクション: %s / %s", " / ".join(path_parts), sec_name)

    # サブページはページレベルに応じて親ページ名のフォルダに入れる
    level_dirs = {1: sec_dir}
    for page in section_el.findall("one:Page", NS):
        if page.get("isInRecycleBin") == "true":
            continue
        level = int(page.get("pageLevel", "1") or 1)
        parent_dir = level_dirs.get(level - 1, sec_dir) if level > 1 else sec_dir
        h_name = sanitize(page.get("name"))
        md_path = place_for(parent_dir, h_name, page.get("ID") or "")
        level_dirs[level] = parent_dir / md_path.stem
        for k in [k for k in level_dirs if k > level]:
            del level_dirs[k]

        stats["pages"] += 1
        if args.dry_run:
            print("  " * (level + len(path_parts)) + f"- {h_name}")
            continue

        # **変わっていないページは書かない。**
        if not args.force and md_path.exists():
            head = read_head(md_path)
            if head.get("modified") and head.get("modified") == (page.get("lastModifiedTime") or ""):
                stats["skipped_pages"] += 1
                written.add(md_path.resolve())
                continue

        try:
            xml = _get_page(app, page.get("ID"), PI_BINARY_DATA if not args.no_images else 0)
        except Exception as e:  # noqa
            log.error("ページ取得失敗 %s: %s", h_name, e)
            stats["errors"] += 1
            # **取れなかったページを、消えたページと呼ばない。** 書けなかった
            # ファイルをそのまま `--prune` に渡すと、COM が一度しゃっくりした
            # だけで前回の写しが消える。前の版を残す。
            written.add(md_path.resolve())
            continue
        try:
            page_el = ET.fromstring(xml)
        except ET.ParseError as e:
            log.error("XML パース失敗 %s: %s", h_name, e)
            stats["errors"] += 1
            written.add(md_path.resolve())
            continue

        md_path.parent.mkdir(parents=True, exist_ok=True)
        # 画像は**そのページの隣**の attachments/（サブページのフォルダなら、そこの隣）。
        page_img_dir = md_path.parent / ATTACH
        rel_img = ATTACH
        conv = PageConverter(page_el, page_img_dir, md_path.stem, rel_img, not args.no_images)
        body = conv.convert()
        title = page_title(page_el, page.get("name"))
        content = frontmatter(title, path_parts + [sec_name], page.attrib) + f"# {title}\n\n" + body
        # LF で書く（Windows の既定は CRLF）。
        with open(md_path, "w", encoding="utf-8", newline="\n") as f:
            f.write(content)
        written.add(md_path.resolve())
        for name in conv.images:
            written.add((page_img_dir / name).resolve())
        # このページの古い画像を片付ける。**いま全部書き直したページの分だけ**なので、
        # 触っていないページの画像は数に入らない。名前が `<ページ名>_NNN.ext` なので
        # 隣のページを巻き込まない（`会議_001.png` は `会議録_*` に当たらない）。
        # `--no-images` のときはやらない ── 出さないだけのつもりが全部消える。
        if not args.no_images and page_img_dir.is_dir():
            for old in page_img_dir.glob(f"{md_path.stem}_*"):
                if old.name not in conv.images:
                    try:
                        old.unlink()
                        stats["pruned_images"] += 1
                    except OSError as e:
                        log.warning("古い画像を消せない %s: %s", old, e)
        stats["images"] += conv.img_count
        stats["written"] += 1
        if args.save_xml:
            md_path.with_suffix(".xml").write_text(xml, encoding="utf-8")


def walk_container(app, el, out_dir: Path, path_parts, args, stats, written: set, keep: Keep, group_prefix=""):
    """Notebook / SectionGroup の下を再帰的に処理"""
    for child in el:
        tag = strip_ns(child.tag)
        if tag == "Section":
            raw = sanitize(child.get("name"))
            # --flatten-groups: グループ名を畳んで一段にする
            out_name = (group_prefix + " › " + raw) if group_prefix else raw
            if not chosen("/".join(path_parts + [raw]), args):
                # **選ばなかった場所は、消えた場所ではない。** `--prune` は
                # 歩いたノートブックの下を見るので、絞って外したセクションを
                # 教えておかないと、前回の写しをまるごと消してしまう
                # （そして次の回も絞りは同じなので、二度と戻らない）。
                keep.add_dir(out_dir / out_name)
                keep.add_pages_of(child)
                stats["filtered_sections"] += 1
                log.debug("絞りで外した: %s", "/".join(path_parts + [raw]))
                continue
            if group_prefix:
                child.set("name", out_name)
            walk_section(app, child, out_dir, path_parts, args, stats, written, keep)
        elif tag == "SectionGroup":
            if child.get("isRecycleBin") == "true":
                continue
            name = sanitize(child.get("name"))
            if args.dry_run:
                print("  " * len(path_parts) + f"[{name}]")
            if args.flatten_groups:
                walk_container(app, child, out_dir, path_parts + [name], args, stats, written, keep,
                               group_prefix=(group_prefix + " › " if group_prefix else "") + name)
            else:
                walk_container(app, child, out_dir / name, path_parts + [name], args, stats, written, keep)


class Keep:
    """`--prune` が触ってはいけないもの ── **見なかった場所と、選ばなかったもの。**

    二本立てなのは、片方だけでは足りないから。

    **選ばなかったセクションは ID で守る。** 階層 XML にページ ID が並んで
    いるので、その写しが**どこに置かれていても**守れる ── あとから
    `--flatten-groups` を付けて出力の形が変わっても、セクションの名前が
    変わっても効く。道だけで守っていた版は、旗を足した次の回に、絞りで
    外したセクションの写しをまるごと消していた（走査が見つけた）。

    **鍵のかかったセクションは道でしか守れない。** 中が見えないので、
    ページ ID が一つも出てこない。
    """

    def __init__(self):
        self.dirs = set()
        self.ids = set()

    def add_dir(self, d):
        self.dirs.add(Path(d).resolve())

    def add_pages_of(self, section_el):
        for pg in section_el.findall("one:Page", NS):
            if pg.get("ID"):
                self.ids.add(pg.get("ID"))

    def covers(self, path: Path, onenote_id: str) -> bool:
        if onenote_id and onenote_id in self.ids:
            return True
        return any(_under(path, d) for d in self.dirs)


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


def _under(path: Path, base: Path) -> bool:
    return path == base or base in path.parents


def prune(scope: list, keep: Keep, written: set, stats):
    """このスクリプトが書いた形（前書きに `onenote_id`）の `.md` のうち、今回出てこなかったものを消す。

    **消してよいのは、今回ちゃんと歩いた場所だけ。** 前の版は出力先ぜんたいを
    歩いていて、`--notebook` で一つに絞って走らせると、**絞られたほうの
    ノートブックが一枚残らず消えた**（今回出てこないので、全部「消えたページ」に
    見える）。同じことが、OneNote 側でノートブックを閉じていた回にも起きる。

    だから歩いた範囲（`scope`）の中だけを見て、その中でも見えなかった場所
    （`keep` ── 鍵のかかったセクション）は避ける。
    """
    for nb_dir in scope:
        if not nb_dir.is_dir():
            continue
        for at in nb_dir.rglob("*.md"):
            r = at.resolve()
            if r in written:
                continue
            oid = read_head(at).get("onenote_id")
            if not oid:
                continue  # ambər で作ったノートは触らない
            if keep.covers(r, oid):
                continue
            try:
                at.unlink()
                stats["pruned"] += 1
                log.info("消した: %s", at)
            except OSError as e:
                log.warning("消せない %s: %s", at, e)


def build_parser():
    """引数の定義は**ここ一つ**。走査（onenote-test.py）も同じものを使う ──
    写すと、片方にだけ足した旗で走査が落ちる（しかも落ち方が `AttributeError`
    なので、何が足りないのか画面から分からない）。"""
    ap = argparse.ArgumentParser(description="OneNote → Markdown（階層保持・ambər の保存ディレクトリ向け）")
    ap.add_argument("--out", help="出力先フォルダ（ローカルでも \\\\…@SSL\\DavWWWRoot\\… でも）"
                                 "。--probe だけは無くてよい")
    ap.add_argument("--notebook", action="append", help="対象ノートブック名（部分一致、複数指定可）")
    ap.add_argument("--only", action="append", metavar="道",
                    help="このセクションだけ写す。`ノートブック/グループ/セクション` の道に部分一致（複数指定可）")
    ap.add_argument("--skip", action="append", metavar="道",
                    help="このセクションは写さない。--only より強い（複数指定可）")
    ap.add_argument("--peek", metavar="道",
                    help=".one を探して、どちらの形式かだけ数える（中身は読まない）")
    ap.add_argument("--forget", action="store_true",
                    help="makepy の作り置きを捨てる（次に繋いだときに作り直す）")
    ap.add_argument("--check", action="store_true",
                    help="繋がるかを数行で言う（繋がらないなら、次の一手も）。手で打ち直して渡せる長さ")
    ap.add_argument("--probe", action="store_true",
                    help="この端末で何が起きているかを残らず並べる（29 行。--check で足りないとき）")
    ap.add_argument("--list", action="store_true",
                    help="写せるセクションを道で並べるだけ（--only/--skip を付けると、外れるものに × が付く）")
    ap.add_argument("--dry-run", action="store_true", help="階層表示のみ、書き込みなし")
    ap.add_argument("--no-images", action="store_true", help="画像を出力しない")
    ap.add_argument("--force", action="store_true", help="変わっていないページも書き直す")
    ap.add_argument("--prune", action="store_true", help="OneNote 側で消えたページの .md を削除する（ambər で直したものも消える）")
    ap.add_argument("--flatten-groups", action="store_true", help="セクショングループを「グループ › セクション」の一段に畳む（深いノートブック向け）")
    ap.add_argument("--save-xml", action="store_true", help="元の XML も .md と同じ場所に保存（デバッグ用）")
    ap.add_argument("--sync", action="store_true", help="読む前に OneNote へ同期を頼む（1時間ごとに回すときはこれ）")
    ap.add_argument("--sync-wait", type=float, default=15.0, metavar="秒", help="--sync のあと読み直すまで待つ秒数（既定 15）")
    ap.add_argument("--log", metavar="ファイル", help="経過をこのファイルにも書き足す（タスク スケジューラ向け）")
    ap.add_argument("-v", "--verbose", action="store_true")
    return ap


def list_sections(root, args):
    """写せるセクションを道で並べる。`--only` / `--skip` を付けて走らせれば、
    **書き出す前に、絞りが狙ったものを拾っているか**が見える。"""
    def walk(el, parts):
        for child in el:
            tag = strip_ns(child.tag)
            if tag == "Section":
                raw = sanitize(child.get("name"))
                path = "/".join(parts + [raw])
                n = sum(1 for pg in child.findall("one:Page", NS)
                        if pg.get("isInRecycleBin") != "true")
                mark = "  " if chosen(path, args) else "× "
                lock = "  （鍵）" if child.get("locked") == "true" else ""
                print(f"{mark}{path}    {n} ページ{lock}")
            elif tag == "SectionGroup" and child.get("isRecycleBin") != "true":
                walk(child, parts + [sanitize(child.get("name"))])

    for nb in root.findall("one:Notebook", NS):
        name = nb.get("name", "")
        if args.notebook and not any(f.lower() in name.lower() for f in args.notebook):
            continue
        walk(nb, [sanitize(name)])


def main():
    args = build_parser().parse_args()
    use_utf8()

    logging.basicConfig(level=logging.DEBUG if args.verbose else logging.INFO,
                        format="%(levelname)s %(message)s")
    if args.log:
        # 定時で回すと、画面には誰もいない。**落ちたことが残らなければ、落ちていない
        # のと見分けがつかない。**
        fh = logging.FileHandler(args.log, encoding="utf-8")
        fh.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(message)s"))
        logging.getLogger().addHandler(fh)
    if not (args.check or args.probe or args.forget or args.peek):
        # **読むだけの回に、見出しは要らない。** 画面の字をそのまま人が
        # 打ち直して渡すので、一行でも短いほうがいい。
        log.info("=== onenote2md 開始 %s", datetime.now().isoformat(timespec="seconds"))

    out_root = Path(args.out) if args.out else Path(".")
    try:
        # **読むだけの回は、鎖を取らない。** 固まっている回を調べるための
        # `--probe` が、その固まっている回のせいで断られるのでは道具にならない。
        if args.probe or args.check or args.forget or args.peek or args.list or args.dry_run:
            return run(args, out_root)
        with only_one(out_root):
            return run(args, out_root)
    except SystemExit as e:
        # **`sys.exit("わけ")` は stderr にしか出ない。** 定時で回すときの
        # `pythonw.exe` に stderr は無いので、繋がらない理由も bit の見立ても
        # どこにも残らず、記録には「開始」の一行だけが残る。
        if isinstance(e.code, str):
            log.error("%s", e.code)
            return 1
        return e.code or 0
    except Exception:
        # 思っていなかった落ち方も同じ ── 追跡は stderr へ消える。
        log.exception("落ちました")
        return 1


def run(args, out_root: Path):
    if args.probe:
        return probe()
    if args.peek:
        return peek(args.peek)
    if args.forget:
        return forget()
    if args.check:
        return check()
    if not args.out:
        sys.exit("--out が要ります（出力先フォルダ）。")
    # 繋ぐときに一度は訊いている（そうでないと「繋がった」と言えない）ので、
    # その答えをそのまま使う ── ページ数の多いノートブックで二度歩かない。
    app, first = connect_onenote()
    root = ET.fromstring(first)
    if args.sync and not args.dry_run and not args.list:
        if sync_notebooks(app, root, args.notebook, args.sync_wait):
            root = ET.fromstring(get_hierarchy(app))   # 同期後の姿で読み直す
    notebooks = root.findall("one:Notebook", NS)
    if not notebooks:
        sys.exit("開いているノートブックがありません。" + _no_books_hint(_store_onenote()))

    if args.list:
        list_sections(root, args)
        return 0

    stats = {"pages": 0, "written": 0, "skipped_pages": 0, "images": 0, "errors": 0,
             "skipped_sections": 0, "filtered_sections": 0, "pruned": 0, "pruned_images": 0}
    written: set = set()
    keep = Keep()
    scope: list = []
    for nb in notebooks:
        name = nb.get("name", "")
        if args.notebook and not any(f.lower() in name.lower() for f in args.notebook):
            continue
        nb_name = sanitize(name)
        log.info("=== ノートブック: %s", nb_name)
        if args.dry_run:
            print(f"# {nb_name}")
        nb_dir = out_root / nb_name
        scope.append(nb_dir)
        walk_container(app, nb, nb_dir, [nb_name], args, stats, written, keep)

    if args.prune and not args.dry_run:
        prune(scope, keep, written, stats)

    log.info("完了: ページ %d（書いた %d・変わらず %d）/ 画像 %d / エラー %d / "
             "セクション: 鍵 %d・絞りで外した %d / 消した %d / 古い画像 %d",
             stats["pages"], stats["written"], stats["skipped_pages"], stats["images"],
             stats["errors"], stats["skipped_sections"], stats["filtered_sections"],
             stats["pruned"], stats["pruned_images"])
    return 1 if stats["errors"] else 0


if __name__ == "__main__":
    sys.exit(main() or 0)
