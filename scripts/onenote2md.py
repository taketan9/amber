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
            attachments/        … ambər の絵の置き場所（ノートの隣・同じ名前）
            <ページ>.md
            <ページ>/           … サブページはページ名のフォルダに入る
              <サブページ>.md

前提
  - Windows + デスクトップ版 OneNote（Microsoft 365 / 2016 以降）
    ※ Store 版（OneNote for Windows 10）は COM 非対応
  - 変換対象のノートブックがすべて OneNote 上で「開いている」こと
  - pip install pywin32

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
  6. **絵は `attachments/`**（ambər の決まり）。ノートの隣のフォルダで、名前は
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
from datetime import datetime
from pathlib import Path
import xml.etree.ElementTree as ET

ONE_NS = "http://schemas.microsoft.com/office/onenote/2013/onenote"
NS = {"one": ONE_NS}
HS_PAGES = 4          # HierarchyScope.hsPages
PI_BINARY_DATA = 1    # PageInfo.piBinaryData（画像を Base64 で同梱）
ATTACH = "attachments"   # ambər の絵の置き場所（ノートの隣・この名前）
PATH_LIMIT = 200         # WebDAV の道の長さの壁（256）に余裕を見た数

log = logging.getLogger("onenote2md")


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

    def office_bits():
        import winreg
        key = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,
                             r"SOFTWARE\Microsoft\Office\ClickToRun\Configuration")
        return winreg.QueryValueEx(key, "Platform")[0]

    say("Office の bit（x64 / x86）", office_bits)

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
        import winreg
        import pywintypes
        clsid = str(pywintypes.IID("OneNote.Application"))
        key = winreg.OpenKey(winreg.HKEY_CLASSES_ROOT, "CLSID" + chr(92) + clsid
                             + chr(92) + "LocalServer32")
        path = (winreg.QueryValue(key, None) or "").strip().strip('"')
        real = _strip_resource_index(path)
        return f"{path}  → {'ある' if real and os.path.exists(real) else '**無い**'}"

    say("COM サーバーの実体", local_server)

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
        sys.exit("pywin32 が必要です:  pip install pywin32")

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

    def by_gencache():
        return gencache.EnsureDispatch("OneNote.Application")

    def by_dispatch():
        return win32com.client.Dispatch("OneNote.Application")

    troubles = []
    # **版は一つとは限らない。** 会社の端末には `1.0` と `1.1` の両方が登録されて
    # いた。どちらが本物かはレジストリの見た目では決まらない（片方は実体を
    # 指していない）ので、**両方試して、実際に答えが返ったほうを採る。**
    for how, make in (("型ライブラリ 1.1 を名指し", by_typelib(1, 1)),
                      ("型ライブラリ 1.0 を名指し", by_typelib(1, 0)),
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

    sys.exit("OneNote (デスクトップ版) に接続できません:\n" + "\n".join(troubles) + """

よくある順に:
  1. 管理者の窓で走らせている ── OneNote と権限を揃える（普通の窓で叩く）
  2. OneNote を先に起動していない ── 手で開き、写すノートブックを開いておく
  3. gen_py の作り置きが壊れている ── %LOCALAPPDATA%\\Temp\\gen_py を消す
  4. ストア版の OneNote ── COM を持たないので、こちらでは手が出ない

`--probe` を付けると、この端末で何が起きているかを並べます。
そのまま貼ってもらえれば、推し量らずに直せます。""")


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


def get_hierarchy(app):
    # 早い束ね: (起点, 深さ) で XML が返る / 遅い束ね: [out] は 3 番目
    return _xml_call(app.GetHierarchy, ("", HS_PAGES), ("", HS_PAGES, ""))


def _get_page(app, page_id, info):
    # 早い束ね: (ページ, 何を含めるか) / 遅い束ね: [out] は **2 番目**
    return _xml_call(app.GetPageContent, (page_id, info), (page_id, "", info))


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
_A = re.compile(r"<a\s+[^>]*href=[\"']?([^\"'>\s]+)[\"']?[^>]*>(.*?)</a>", re.S | re.I)
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


def inline_md(s: str) -> str:
    if not s:
        return ""
    s = re.sub(r"<br\s*/?>", "  \n", s, flags=re.I)
    while True:
        new = _SPAN.sub(_span_repl, s)
        if new == s:
            break
        s = new
    s = _A.sub(lambda m: f"[{_TAG.sub('', m.group(2)).strip()}]({m.group(1)})", s)
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
        self.images: list[str] = []      # 書いた絵の名前（--prune のため）
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
                out.append("> [インク描画: 変換対象外]")
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
        texts = [inline_md(t.text or "") for t in oe.findall("one:T", NS)]
        text = "".join(texts)

        if text.strip() or tag_prefix:
            if style.startswith("h") and style[1:].isdigit() and not bullet:
                level = min(int(style[1:]), 6)
                lines.append(f"{'#' * level} {text.strip()}")
            elif style == "code" and not bullet:
                lines.append(f"```\n{text}\n```")
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
                cell_text = "<br>".join(l.strip() for l in cell_lines if l.strip())
                cells.append(cell_text.replace("|", "\\|"))
            rows.append(cells)
        if not rows:
            return []
        width = max(len(r) for r in rows)
        rows = [r + [""] * (width - len(r)) for r in rows]
        out = ["| " + " | ".join(rows[0]) + " |", "|" + " --- |" * width]
        for r in rows[1:]:
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
            # 同じ中身なら書かない（同期に、触っていない絵の更新だけが流れないように）。
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


def page_title(page_el, hierarchy_name):
    t = page_el.find("one:Title/one:OE/one:T", NS)
    if t is not None and (t.text or "").strip():
        return inline_md(t.text).strip()
    return hierarchy_name or "Untitled"


def frontmatter(title, path_parts, page_attr):
    # ambər の `created:` は `YYYY-MM-DD`（時刻つきの ISO も読めるが、揃えておく）。
    created = (page_attr.get("dateTime") or "")[:10]
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
    img_dir = sec_dir / ATTACH
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
        # 絵は**そのページの隣**の attachments/（サブページのフォルダなら、そこの隣）。
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
        # このページの古い絵を片付ける。**いま全部書き直したページの分だけ**なので、
        # 触っていないページの絵は数に入らない。名前が `<ページ名>_NNN.ext` なので
        # 隣のページを巻き込まない（`会議_001.png` は `会議録_*` に当たらない）。
        # `--no-images` のときはやらない ── 出さないだけのつもりが全部消える。
        if not args.no_images and page_img_dir.is_dir():
            for old in page_img_dir.glob(f"{md_path.stem}_*"):
                if old.name not in conv.images:
                    try:
                        old.unlink()
                        stats["pruned_images"] += 1
                    except OSError as e:
                        log.warning("古い絵を消せない %s: %s", old, e)
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
    ap.add_argument("--out", required=True, help="出力先フォルダ（ローカルでも \\\\…@SSL\\DavWWWRoot\\… でも）")
    ap.add_argument("--notebook", action="append", help="対象ノートブック名（部分一致、複数指定可）")
    ap.add_argument("--only", action="append", metavar="道",
                    help="このセクションだけ写す。`ノートブック/グループ/セクション` の道に部分一致（複数指定可）")
    ap.add_argument("--skip", action="append", metavar="道",
                    help="このセクションは写さない。--only より強い（複数指定可）")
    ap.add_argument("--probe", action="store_true",
                    help="この端末で何が起きているかを並べる（繋がらないときに、そのまま貼ってほしい）")
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

    logging.basicConfig(level=logging.DEBUG if args.verbose else logging.INFO,
                        format="%(levelname)s %(message)s")
    if args.log:
        # 定時で回すと、画面には誰もいない。**落ちたことが残らなければ、落ちていない
        # のと見分けがつかない。**
        fh = logging.FileHandler(args.log, encoding="utf-8")
        fh.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(message)s"))
        logging.getLogger().addHandler(fh)
    log.info("=== onenote2md 開始 %s", datetime.now().isoformat(timespec="seconds"))

    out_root = Path(args.out)
    with only_one(out_root):
        code = run(args, out_root)
    return code


def run(args, out_root: Path):
    if args.probe:
        return probe()
    # 繋ぐときに一度は訊いている（そうでないと「繋がった」と言えない）ので、
    # その答えをそのまま使う ── ページ数の多いノートブックで二度歩かない。
    app, first = connect_onenote()
    root = ET.fromstring(first)
    if args.sync and not args.dry_run and not args.list:
        if sync_notebooks(app, root, args.notebook, args.sync_wait):
            root = ET.fromstring(get_hierarchy(app))   # 同期後の姿で読み直す
    notebooks = root.findall("one:Notebook", NS)
    if not notebooks:
        sys.exit("開いているノートブックがありません。OneNote 上で対象ノートブックを開いてください。")

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
             "セクション: 鍵 %d・絞りで外した %d / 消した %d（絵 %d）",
             stats["pages"], stats["written"], stats["skipped_pages"], stats["images"],
             stats["errors"], stats["skipped_sections"], stats["filtered_sections"],
             stats["pruned"], stats["pruned_images"])
    return 1 if stats["errors"] else 0


if __name__ == "__main__":
    sys.exit(main() or 0)
