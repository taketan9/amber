#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""onenote2md.py の走査 ── COM を偽物に差し替えて、mac の上で全部通す。

本物は Windows のデスクトップ版 OneNote が要る。**だから mac では一行も
動かないまま置かれていた** ── そして定時で回すものは、誰も見ていないところで
壊れる。COM が返すのは XML の文字列だけなので、そこだけ偽れば、名前の付け方も
階層の作り方も差分の判断も `--prune` も、こちらで全部確かめられる。

偽れないのは二つ ── OneNote が本当にその XML を返すか、`SyncHierarchy` が
本当に同期するか。そこは会社の Windows で見るしかない。

    python3 scripts/onenote-test.py
"""

from __future__ import annotations

import base64
import importlib.util
import os
import shutil
import sys
import tempfile
from pathlib import Path

# **原本の `.pyc` を作らない。** 変異テストは同じ道のファイルを壊しては戻すので、
# `cp` が付ける秒とファイルの大きさがたまたま前回と揃うと、Python は古い `.pyc`
# をそのまま使う ── 戻したはずの原本ではなく、**直前に壊した版**が走る。
# 最初にこれを踏んだとき、生きている検査が二つ「黙った」ように見えた。
sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("onenote2md", ROOT / "onenote2md.py")
o2m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(o2m)


def _parse(argv):
    """**本体の引数定義をそのまま使う。**写して持つと、本体に旗を足した日に
    走査だけが古い定義で走り、`AttributeError` で落ちる ── 画面には
    「何が足りないのか」が出ない。"""
    return o2m.build_parser().parse_args(argv)


FAILED = []


def check(name, cond, detail=""):
    if cond:
        print(f"  ok   {name}")
    else:
        print(f"  NG   {name}  {detail}")
        FAILED.append(name)


# ---------------------------------------------------------------------------


def t_lock(tmp):
    print("一度に一本だけ ──")
    out = tmp / "lock"
    out.mkdir(parents=True, exist_ok=True)
    with o2m.only_one(out):
        try:
            with o2m.only_one(out):
                check("二本目は断られる", False, "通ってしまった")
        except SystemExit as e:
            check("二本目は断られる", "前の回がまだ走っています" in str(e), str(e))
    with o2m.only_one(out):
        check("一本目が終われば、また取れる", True)
    other = tmp / "lock2"
    other.mkdir(parents=True, exist_ok=True)
    with o2m.only_one(out):
        with o2m.only_one(other):
            check("出力先が違えば同時に走れる", True)


def t_names(tmp):
    print("名前 ──")
    check("Windows で使えない字", o2m.sanitize('a/b:c*d?e"f<g>h|i') == "a_b_c_d_e_f_g_h_i")
    check("SharePoint が断る字", o2m.sanitize("a#b%c&d~e{f}g") == "a_b_c_d_e_f_g")
    check("_vti_ で始まらない", not o2m.sanitize("_vti_x").lower().startswith("_vti_"))
    check("予約名 CON", o2m.sanitize("CON") == "CON_")
    check("末尾の点と空白", o2m.sanitize("あ. ") == "あ")
    check("空なら fallback", o2m.sanitize("   ") == "untitled")
    check("日本語は削られない", o2m.sanitize("九月の定例") == "九月の定例")
    # **ここも 60 字。** ambər の `file_stem` が 60 で切るので、120 のままだと
    # **長い題のページだけ画像の幹がずれ**、改名・移動した日に画像が付いてこない。
    check("ページの名前も 60 字で切る", len(o2m.sanitize("あ" * 100)) == 60,
          len(o2m.sanitize("あ" * 100)))

    # **ambər の決まりに合わせた幹**（依頼 593）── ハイフンで繋ぎ、60 字で切る。
    # `note::file_stem` が正本で、ここはその写し。**写しである以上ずれる。**
    check("幹は 60 字で切る", len(o2m.amber_stem("あ" * 120)) == 60,
          len(o2m.amber_stem("あ" * 120)))
    check("使えない字はハイフンに", o2m.amber_stem("斜/線") == "斜-線", o2m.amber_stem("斜/線"))
    # **先頭にハイフンを置かない** ── `-001.png` のような名前になる。
    check("先頭にハイフンを置かない", not o2m.amber_stem("??  notes").startswith("-"),
          o2m.amber_stem("??  notes"))
    check("予約名は避ける", o2m.amber_stem("CON") != "CON", o2m.amber_stem("CON"))


def t_select(tmp):
    print("写すものを選ぶ ──")

    class A:
        only = None
        skip = None

    a = A()
    a.only = ["議事録"]
    check("--only に当たるものだけ", o2m.chosen("仕事/議事録", a) is True)
    check("--only に外れたら写さない", o2m.chosen("仕事/買い物", a) is False)
    # **大文字小文字は問わない。** 打った人は覚えていない。
    a.only = ["ONENOTE"]
    check("大文字小文字は問わない", o2m.chosen("仕事/onenote メモ", a) is True)
    # **`--skip` は `--only` より強い。** 「これだけ、ただしこれは除く」と
    # 言えないと、一つ外すために一覧を全部書き出すことになる。
    a.only = ["仕事"]
    a.skip = ["秘密"]
    check("--skip は外す", o2m.chosen("仕事/秘密", a) is False)
    check("--skip は --only より強い", o2m.chosen("仕事/議事録", a) is True)
    a.only = None
    a.skip = None
    check("どちらも無ければ、ぜんぶ写す", o2m.chosen("なんでも/よい", a) is True)


def t_cp932(tmp):
    print("日本語 Windows の画面 ──")
    import subprocess
    # **日本語 Windows の既定は cp932。** そのまま出すと、セクションの名前が
    # 化けたまま画面に出る（本人の端末で出た）。出しどころを UTF-8 に向ける。
    r = subprocess.run(
        [sys.executable, "-c",
         "import io, sys;"
         "sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='cp932');"
         "sys.stderr = io.TextIOWrapper(sys.stderr.buffer, encoding='cp932');"
         "sys.path.insert(0, %r);" % str(ROOT) +
         "import importlib.util as iu;"
         "sp = iu.spec_from_file_location('o', %r);" % str(ROOT / "onenote2md.py") +
         "m = iu.module_from_spec(sp); sp.loader.exec_module(m);"
         "m.use_utf8(); print('セクション：９月の定例 ── ＡＢＣ')"],
        capture_output=True, text=True, errors="replace")
    check("cp932 に向けても、最後まで出る", "９月の定例" in r.stdout, r.stdout + r.stderr)


def t_offline(tmp):
    print("網に出ない ──")
    # **会社の端末は網に出られない**（本人・依頼 582）。取り込みの道に
    # 網を見に行く口が一つでもあると、そこで止まる。
    for name in ("onenote2md.py", "onestore.py", "onenote_ui.py"):
        src = (ROOT / name).read_text(encoding="utf-8")
        bad = [w for w in ("urllib.request", "requests.get", "http://", "https://",
                           "socket.create_connection", "pip install")
               if w in src and "example.com" not in src.split(w)[0][-40:]]
        check(f"網を見に行かせない（{name}）", not bad, bad)


# ---------------------------------------------------------------------------
# connect_onenote ── win32com ごと偽って、束ね方の梯子を確かめる
# ---------------------------------------------------------------------------
import types  # noqa: E402


def t_log(tmp):
    print("落ちたわけを、記録に残す ──")
    import subprocess
    at = tmp / "log"
    at.mkdir(exist_ok=True)
    logfile = at / "t.log"
    r = subprocess.run([sys.executable, str(ROOT / "onenote2md.py"),
                        "--out", str(at / "o"), "--log", str(logfile),
                        str(at / "そんなフォルダは無い")],
                       capture_output=True, text=True)
    body = logfile.read_text(encoding="utf-8") if logfile.exists() else ""
    # **画面には誰もいない。** `pythonw.exe` に stderr は無いので、
    # `sys.exit("わけ")` の字はどこにも出ないまま終わる。
    check("落ちた回は 0 を返さない", r.returncode != 0, r.returncode)
    check("落ちたわけが、記録に残る", "そんなフォルダは無い" in body, body)
    check("記録に ERROR として残る", "ERROR" in body, body)


def _one(at, fmt_guid):
    """作り物の `.one`。**見るのは先頭 64 バイトだけ**なので、そこだけ本物にする。"""
    import uuid
    head = bytearray(64)
    head[0:16] = uuid.UUID("7B5C52E4-D88C-4DA7-AEB1-5378D02996D3").bytes_le
    head[48:64] = uuid.UUID(fmt_guid).bytes_le
    at.write_bytes(bytes(head) + b"\x00" * 64)


def t_peek(tmp):
    print(".one の形式を数える（--peek）──")
    import io, contextlib
    d = tmp / "peek"
    (d / "奥").mkdir(parents=True, exist_ok=True)
    _one(d / "古い.one", "109ADD3F-911B-49F5-A5D0-1791EDC8AED8")
    _one(d / "奥" / "365-1.one", "638DE92F-A6D4-4BC1-9A36-B3FC2511A5B7")
    _one(d / "奥" / "365-2.one", "638DE92F-A6D4-4BC1-9A36-B3FC2511A5B7")
    (d / "目次.onetoc2").write_bytes(b"\x00" * 64)
    (d / "ただの.md").write_text("x", encoding="utf-8")

    def run(where):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            code = o2m.peek(str(where))
        return code, buf.getvalue()

    code, out = run(d)
    check("下まで歩いて数える", "3 本" in out and "2 本" in out and "1 本" in out, out)
    check("形式ごとに分ける", "109add3f" in out and "638de92f" in out, out)
    check("目次も数える", ".onetoc2" in out, out)
    check("関係ないファイルは数えない", "ただの" not in out, out)
    # **形式を数えるのは `.one` だけ。** 目次（`.onetoc2`）を混ぜると、
    # 「何本が読めるか」の分母が狂う ── 判断に使う数字なので、狂うと痛い。
    check("形式を数えるのは .one だけ", "1/3 本" in out and "目次" not in out, out)
    # **数えたら、意味を言う。** 数字だけ見せて人に判じさせない。
    check("混ざっていればそう言う", "混ざっています" in out, out)
    check("探す道の例を出す", "365-1.one" in out or "365-2.one" in out, out)
    check("見つかった回は 0 を返す", code == 0, code)

    only = tmp / "peek-ok"
    only.mkdir(exist_ok=True)
    _one(only / "a.one", "109ADD3F-911B-49F5-A5D0-1791EDC8AED8")
    _, out = run(only)
    check("ぜんぶ公開仕様なら、通ると言う", "ぜんぶ公開仕様" in out, out)

    bad = tmp / "peek-ng"
    bad.mkdir(exist_ok=True)
    _one(bad / "a.one", "638DE92F-A6D4-4BC1-9A36-B3FC2511A5B7")
    _, out = run(bad)
    check("一本も無ければ、重いと言う", "一本も公開仕様ではありません" in out, out)

    empty = tmp / "peek-empty"
    empty.mkdir(exist_ok=True)
    code, out = run(empty)
    # **無いときこそ、意味がある。** SharePoint にしか無い形かもしれない。
    check("一つも無ければ、SharePoint の線を言う", "SharePoint" in out, out)
    check("無い回は 0 を返さない", code != 0, code)
    code, out = run(tmp / "そんな道は無い")
    check("道が無ければ、そう言う", "ありません" in out and code != 0, out)
    # ── `.onepkg` は入れ物 ──
    #
    # **中を見ないと形式は分からない。** ノートブックをまとめて出すと
    # この形にしかならない（OneNote が「.pdf .xps .onepkg だけ」と言う）。
    pkg = tmp / "pkg"
    pkg.mkdir(exist_ok=True)
    (pkg / "まとめ.onepkg").write_bytes(b"MSCF" + b"\x00" * 60)
    keep_un = o2m.unpack_onepkg
    try:
        inside = tmp / "inside"
        inside.mkdir(exist_ok=True)
        _one(inside / "中身.one", "109ADD3F-911B-49F5-A5D0-1791EDC8AED8")
        o2m.unpack_onepkg = lambda at, into: (str(inside), None)
        _, out = run(pkg)
        check("入れ物の中まで数える", "中に 1 本" in out and "109add3f" in out, out)
        # 開けなかったら、そう言う ── 黙って 0 本と数えない。
        o2m.unpack_onepkg = lambda at, into: (None, "expand が無い")
        _, out = run(pkg)
        check("開けなければ、わけを言う", "開けない" in out and "expand が無い" in out, out)
        check("開けなくても、数だけは出す", ".onepkg" in out and "1 本" in out, out)
    finally:
        o2m.unpack_onepkg = keep_un

    # CAB でなければ、開かずに断る。
    notcab = tmp / "notcab"
    notcab.mkdir(exist_ok=True)
    (notcab / "ちがう.onepkg").write_bytes(b"PK\x03\x04" + b"\x00" * 60)
    _, out = run(notcab)
    check("CAB でなければ、そう言う", "CAB ではない" in out, out)

    # 一本を名指ししてもよい。
    _, out = run(d / "古い.one")
    check("ファイル一本でも数える", "1 本" in out, out)


def t_from_files(tmp):
    print("書き出したファイルから写す ──")
    # **走らせる場所を変えても読めるか。** `import onestore` に頼ると、
    # `sys.path` に `scripts/` が入らない場所で落ちる。
    import subprocess
    here = tmp / "よそ"
    here.mkdir(exist_ok=True)
    r = subprocess.run([sys.executable, str(ROOT / "onenote2md.py"),
                        "--out", str(tmp / "どこか"), str(here)],
                       capture_output=True, text=True, cwd=str(here), errors="replace")
    # **読み込みを抜けたことを見る。** 「落ちなかった」ではなく「先へ進んだ」 ──
    # 空のフォルダなので、抜けていれば「.one がありません」まで行く。
    # 字の無いことだけ見ていた版は、別の落ち方（道が違う・ファイルが無い）を
    # 素通りさせた。
    check("よその場所から走らせても、隣の一枚を読める",
          "の下に .one がありません" in (r.stdout + r.stderr), (r.stdout + r.stderr)[-200:])
    # **隣に居ないときは、そう言う。** 取り込みが半端なまま走らせた人に
    # `FileNotFoundError` の追跡を見せても、何をすればいいか分からない。
    alone = tmp / "ひとりぼっち"
    alone.mkdir(exist_ok=True)
    shutil.copy(ROOT / "onenote2md.py", alone / "onenote2md.py")
    r = subprocess.run([sys.executable, str(alone / "onenote2md.py"),
                        "--out", str(tmp / "どこか2"), str(here)],
                       capture_output=True, text=True, errors="replace")
    both = r.stdout + r.stderr
    check("隣に居なければ、そう言う（追跡ではなく）",
          "onestore.py がありません" in both and "Traceback" not in both, both[-200:])
    import io, contextlib
    # **本物の読み込みを通す。** `sys.modules` に偽物を差し込んでいた版は、
    # 本体が隣のファイルをどう読むかを一度も試していなかった ── 現場で
    # `ModuleNotFoundError` になって初めて分かった（偽物が本物より甘い、六度目）。
    ost = o2m.load_onestore()

    src = tmp / "書き出し"
    src.mkdir(exist_ok=True)
    (src / "議事録.one").write_bytes(b"\x00" * 8)     # 中身は偽物に読ませる
    out = tmp / "fromfiles"
    keep = ost.pages
    try:
        # **同じ題が二枚。** 上書きすると字が消えるので、ずらす。
        ost.pages = lambda d: [
            {"title": "9月の定例", "level": 1,
             "lines": [{"text": "決めたこと", "indent": 0},
                       {"text": "宿題", "indent": 1}]},
            {"title": "9月の定例", "level": 1, "lines": [{"text": "別の一枚", "indent": 0}]},
            {"title": "補足", "level": 2, "lines": [{"text": "サブページ", "indent": 0}]},
        ]
        args = _parse(["--out", str(out), str(src)])
        code = o2m.run(args, out)
        check("エラー無しなら 0 を返す", code == 0, code)
        got = sorted(str(q.relative_to(out)) for q in out.rglob("*.md"))
        check("セクション＝フォルダ、ページ＝.md",
              "書き出し/議事録/9月の定例.md" in got, got)
        check("同じ題は上書きせず、ずらす",
              "書き出し/議事録/9月の定例 (2).md" in got, got)
        # **サブページは親ページ名のフォルダ**（COM の道と同じ形）。
        check("サブページは親の下へ",
              "書き出し/議事録/9月の定例/補足.md" in got
              or "書き出し/議事録/9月の定例 (2)/補足.md" in got, got)
        body = (out / "書き出し" / "議事録" / "9月の定例.md").read_bytes()
        check("改行は LF", b"\r\n" not in body)
        text = body.decode("utf-8")
        check("前書きに題", 'title: "9月の定例"' in text, text[:120])
        check("本文が入る", "決めたこと" in text and "宿題" in text, text)
        # 絞りも効く（COM の道と同じ `--only`）。
        out2 = tmp / "fromfiles2"
        code = o2m.run(_parse(["--out", str(out2), str(src), "--only", "そんな名前は無い"]), out2)
        check("--only で外れたら書かない", not list(out2.rglob("*.md")) if out2.exists() else True)
        # 読めないファイルは、落ちずに数える。
        ost.pages = lambda d: (_ for _ in ()).throw(ValueError("壊れている"))
        out3 = tmp / "fromfiles3"
        # **落ちるのも「黙る」の一種**（依頼 569）── 受け止めて NG にする。
        try:
            code = o2m.run(_parse(["--out", str(out3), str(src)]), out3)
        except Exception as e:  # noqa
            code = f"落ちた: {type(e).__name__}"
        check("読めないファイルは、落ちずにエラーと数える", code == 1, code)

        # **0 ページを黙って通さない。** たいていは読めない形式のほう
        # （`638DE92F…`）── わけを言わないと、人は何をすればいいか分からない。
        ost.pages = lambda d: []
        import uuid as _uu
        src4 = tmp / "読めない形式"
        src4.mkdir(exist_ok=True)
        (src4 / "議事録.one").write_bytes(
            b"\x00" * 48 + _uu.UUID("638de92f-a6d4-4bc1-9a36-b3fc2511a5b7").bytes_le)
        out4 = tmp / "fromfiles4"
        import logging
        buf = io.StringIO()
        h = logging.StreamHandler(buf)
        o2m.log.addHandler(h)
        was = o2m.log.level
        o2m.log.setLevel(logging.INFO)
        try:
            o2m.run(_parse(["--out", str(out4), str(src4)]), out4)
        finally:
            o2m.log.removeHandler(h)
            o2m.log.setLevel(was)
        said = buf.getvalue()
        check("1 ページも取れなかったら、形式のわけを言う",
              "1 ページも取れなかった" in said and "形式" in said, said[-300:])

        # **いまの版を選ぶ規則**は、偽物を挟まずに直に試す ── `pages` を丸ごと
        # 差し替えていると、そこが壊れても走査は気づかない。
        check("いまの版は、最後の改訂",
              ost.current({1: ["古い"], 2: ["途中"], 3: ["いま"]}) == ["いま"],
              ost.current({1: ["古い"], 3: ["いま"]}))
        # 落ちるのも「黙る」の一種 ── 受け止めて NG にする（依頼 569）。
        try:
            empty = ost.current({})
        except Exception as e:  # noqa
            empty = f"落ちた: {type(e).__name__}"
        check("改訂が無ければ、空", empty == [], empty)
    finally:
        ost.pages = keep


def t_onestore_shape(tmp):
    print("`.one` の中身を Markdown に ──")
    import importlib.util as iu
    spec = iu.spec_from_file_location("onestore", ROOT / "onestore.py")
    ost = iu.module_from_spec(spec)
    spec.loader.exec_module(ost)

    def md(**kw):
        got = {"text": kw.pop("text", "字"), "indent": kw.pop("indent", 0),
               "style": kw.pop("style", ""), "bold": False, "italic": False,
               "strike": False, "list": None, "todo": False, "y": 0, "x": 0}
        got.update(kw)
        return ost.as_markdown(got)

    check("見出しは #", md(text="決めたこと", style="h1") == "# 決めたこと", md(style="h1"))
    check("深い見出しも段に合わせる", md(style="h3") == "### 字", md(style="h3"))
    check("見出しは 6 段まで", md(style="h9") == "###### 字", md(style="h9"))
    check("太字は **", md(bold=True) == "**字**", md(bold=True))
    check("斜体は *", md(italic=True) == "*字*", md(italic=True))
    check("取り消し線は ~~", md(strike=True) == "~~字~~", md(strike=True))
    check("箇条書きは -", md(list="bullet") == "- 字", md(list="bullet"))
    check("番号は 1.", md(list="number") == "1. 字", md(list="number"))
    check("チェックは升", md(todo=True) == "- [ ] 字", md(todo=True))
    check("引用は >", md(style="cite") == "> 字", md(style="cite"))
    check("コードは枠", md(style="code") == "```\n字\n```", md(style="code"))
    check("深さは字下げ", md(indent=2, list="bullet") == "    - 字", md(indent=2, list="bullet"))
    # **印は外側から。** 中に入れると `**- 字**` になって、箇条書きが消える。
    check("箇条書きの印は、太字の外",
          md(list="bullet", bold=True) == "- **字**", md(list="bullet", bold=True))

    # **ページ頭の日付と時刻は、本文ではない。**
    for pid in (ost.P_IS_DATE, ost.P_IS_TIME, ost.P_IS_BOILER, ost.P_IS_TITLE):
        got = ost.line_of({ost.P_ASCII: b"Friday, November 22, 2019", pid: 1})
        check(f"0x{pid:04X} の行は本文に混ぜない", got is None, got)
    check("ふつうの行は残る",
          ost.line_of({ost.P_ASCII: b"a"}) is not None)
    check("空の行は落とす", ost.line_of({ost.P_ASCII: b"   "}) is None)

    # **上から下、同じ高さなら左から右**（COM の道と同じ潰し方）。
    import struct as st

    # **本物の `pages` の並べ方を見る。** ここで自分で `sorted` を書いて
    # 確かめても、本体がそうしているかは何も言っていない。
    keep_sp, keep_rp = ost.spaces, ost.read_props
    try:
        def four(v):
            return st.pack("<I", v)
        made = [{"oid": i, "jcid": ost.JC_TEXT, "stp": 0, "cb": 0} for i in range(3)]
        made.append({"oid": 3, "jcid": ost.JC_PAGE, "stp": 0, "cb": 0})
        props = {0: {ost.P_ASCII: b"shita", ost.P_Y: four(200), ost.P_X: four(0)},
                 1: {ost.P_ASCII: b"migi", ost.P_Y: four(100), ost.P_X: four(90)},
                 2: {ost.P_ASCII: b"hidari", ost.P_Y: four(100), ost.P_X: four(10)},
                 3: {}}
        ost.spaces = lambda d: [("os", {1: made})]
        ost.read_props = lambda d, o: props[o["oid"]]
        got = [l["text"] for l in ost.pages(b"")[0]["lines"]]
        check("並びは上から下・左から右", got == ["hidari", "migi", "shita"], got)
    finally:
        ost.spaces, ost.read_props = keep_sp, keep_rp


def t_style(tmp):
    print("装飾は、書式のほうに載っている ──")
    import importlib.util as iu
    spec = iu.spec_from_file_location("onestore", ROOT / "onestore.py")
    ost = iu.module_from_spec(spec)
    spec.loader.exec_module(ost)

    # **色の決まり**（MS-ONE の COLORREF）── 最後が 0x00 のときだけ色。
    check("自動（最後が 0xFF）は色を付けない", ost._color(b"\x00\x00\x00\xff") is None)
    check("最後が 0x00 なら、前の三つが色", ost._color(b"\x76\x76\x76\x00") == "#767676")
    check("赤・緑・青の順", ost._color(b"\x12\x34\x56\x00") == "#123456")
    check("短すぎる・欄が無いときは色なし",
          ost._color(b"\x01\x02") is None and ost._color(None) is None)

    # **本物の `read_props` に、本物のバイト列を渡す。**
    #
    # 下の段は `read_props` を偽物に差し替えるので、**参照をどう読むかは
    # 一度も試していない** ── 偽物が本物より甘い、七度目（依頼 595 と同じ形）。
    # property set を手で組んで、指し先が本当に配られるかを見る。
    import struct as st2
    oid = 0x0000ABCD
    props = st2.pack("<H", 2)                          # プロパティ二つ
    props += st2.pack("<I", (0x8 << 26) | ost.P_STYLE)  # 一つ参照する
    props += st2.pack("<I", (0x3 << 26) | ost.P_BOLD)   # 1 バイトの値
    props += b"\x01"                                    # ↑ の中身
    head = st2.pack("<I", 1 | (1 << 31))                # 指し先 1 個・OSID 無し
    head += st2.pack("<I", oid)                        # ← 読み飛ばしていた並び
    raw = head + props
    got = ost.read_props(raw, {"oid": 0, "jcid": ost.JC_TEXT, "stp": 0, "cb": len(raw)})
    check("参照は、頭の並びから指し先を受け取る",
          got.get(ost.P_STYLE) == ("ref", oid), got)
    check("参照のあとの値も、ずれずに読める", got.get(ost.P_BOLD) == b"\x01", got)

    # **旗は本文ではなく書式が持つ。** ここを本文から読んでいたので、
    # 太字も斜体も取り消し線も**一度も落ちていなかった**（依頼 608）。
    keep = ost.read_props
    try:
        text = {"oid": 1, "jcid": ost.JC_TEXT, "stp": 0, "cb": 0}
        style = {"oid": 7, "jcid": 0x004D, "stp": 0, "cb": 0}
        props = {
            1: {ost.P_ASCII: b"important", ost.P_STYLE: ("ref", 7)},
            7: {ost.P_BOLD: 1, ost.P_COLOR: b"\x12\x34\x56\x00"},
        }
        ost.read_props = lambda d, o: props[o["oid"]]
        look = ost.by_oid([text, style])
        st = ost.style_of(b"", look, props[1])
        check("本文から書式をたどれる", st.get(ost.P_BOLD) == 1, st)
        got = ost.line_of(props[1], st)
        check("太字の旗が立つ", got["bold"] is True, got)
        check("色が取れる", got["color"] == "#123456", got)
        md = ost.as_markdown(got)
        check("色は印の外、字は印の中",
              md == '<span style="color:#123456">**important**</span>', md)

        # 指し先が無い・壊れているときは、黙って素の字に戻る。
        check("たどれなければ書式なし", ost.style_of(b"", look, {}) == {})
        check("指し先が居なければ書式なし",
              ost.style_of(b"", look, {ost.P_STYLE: ("ref", 999)}) == {})

        # **リンクはいちばん内側。** 外に出すと印がリンクの中に入る。
        props[7] = {ost.P_BOLD: 1, ost.P_LINK_URL: "https://x/a".encode("utf-16-le")}
        got = ost.line_of(props[1], ost.style_of(b"", look, props[1]))
        check("リンクの行き先が取れる", got["link"] == "https://x/a", got)
        check("リンクは印の中", ost.as_markdown(got) == "**[important](https://x/a)**",
              ost.as_markdown(got))

        # 見出しは印を重ねないが、色とリンクは残す。
        props[7] = {ost.P_COLOR: b"\x12\x34\x56\x00"}
        h = ost.line_of(props[1], ost.style_of(b"", look, props[1]))
        h["style"] = "h2"
        check("見出しにも色は付く",
              ost.as_markdown(h) == '## <span style="color:#123456">important</span>',
              ost.as_markdown(h))
    finally:
        ost.read_props = keep


def t_tables(tmp):
    print("表 ──")
    import importlib.util as iu
    spec = iu.spec_from_file_location("onestore", ROOT / "onestore.py")
    ost = iu.module_from_spec(spec)
    spec.loader.exec_module(ost)
    keep = ost.read_props
    try:
        objs, props = [], {}

        def add(jc, text=None):
            o = {"oid": len(objs), "jcid": jc, "stp": 0, "cb": 0}
            objs.append(o)
            if text is None:
                props[o["oid"]] = {}
            elif text.isascii():
                props[o["oid"]] = {ost.P_ASCII: text.encode("latin-1")}
            else:
                props[o["oid"]] = {ost.P_UNICODE: text.encode("utf-16-le")}

        add(ost.JC_TABLE)
        add(ost.JC_ROW); add(ost.JC_CELL); add(ost.JC_TEXT, "name")
        add(ost.JC_CELL); add(ost.JC_TEXT, "value")
        add(ost.JC_ROW); add(ost.JC_CELL); add(ost.JC_TEXT, "apple")
        add(ost.JC_CELL); add(ost.JC_TEXT, "120")
        ost.read_props = lambda d, o: props[o["oid"]]
        got = ost.tables(b"", objs)
        check("表・行・升をたどる", got and got[0]["rows"] == [["name", "value"], ["apple", "120"]],
              got)
        md = ost.table_markdown(got[0])
        check("Markdown の表になる", md[1] == "| --- | --- |" and "| apple | 120 |" in md, md)
        # **1 行目を見出しにすると、そのデータが一行消える**（依頼 578 と同じ形）。
        # 「1 行目が残っている」だけでは足りない ── 見出しに使っても残って
        # 見えるので、**見出しの行が空であること**と**行の数**の両方を見る。
        check("見出しの行は空で置く（データを一行も減らさない）",
              md[0].replace("|", "").strip() == ""
              and len(md) == len(got[0]["rows"]) + 2
              and "| name | value |" in md[2:], md)

        # **升の数が行ごとに違う表は、揃えないと画面の上で崩れる。**
        # OneNote では升を結合できるので、揃っていない表は普通に出てくる。
        objs.clear(); props.clear()
        add(ost.JC_TABLE)
        add(ost.JC_ROW); add(ost.JC_CELL); add(ost.JC_TEXT, "a")
        add(ost.JC_CELL); add(ost.JC_TEXT, "b")
        add(ost.JC_ROW); add(ost.JC_CELL); add(ost.JC_TEXT, "c")
        md = ost.table_markdown(ost.tables(b"", objs)[0])
        check("升の数を、行ごとに揃える",
              len({r.count("|") for r in md}) == 1 and "| c |  |" in md, md)

        # 升の中が複数行なら、空白で繋ぐ（Markdown の表に改行は入らない）。
        objs.clear(); props.clear()
        add(ost.JC_TABLE); add(ost.JC_ROW); add(ost.JC_CELL)
        add(ost.JC_TEXT, "one"); add(ost.JC_TEXT, "two")
        got = ost.tables(b"", objs)
        check("升の中の改行は、空白で繋ぐ", got[0]["rows"] == [["one two"]], got)

        # **表の中の字は、本文に二度出さない。**
        objs.clear(); props.clear()
        add(ost.JC_PAGE)
        add(ost.JC_TEXT, "そとの字")
        add(ost.JC_TABLE); add(ost.JC_ROW); add(ost.JC_CELL); add(ost.JC_TEXT, "なかの字")
        keep_sp = ost.spaces
        try:
            ost.spaces = lambda d: [("os", {1: list(objs)})]
            pg = ost.pages(b"")[0]
            body = [l["text"] for l in pg["lines"]]
            check("表の中の字は、本文に二度出さない",
                  body == ["そとの字"] and pg["tables"][0]["rows"] == [["なかの字"]],
                  (body, pg["tables"]))
        finally:
            ost.spaces = keep_sp
    finally:
        ost.read_props = keep


def t_empty_space(tmp):
    print("中身のない空間 ──")
    import importlib.util as iu
    spec = iu.spec_from_file_location("onestore", ROOT / "onestore.py")
    ost = iu.module_from_spec(spec)
    spec.loader.exec_module(ost)
    keep_sp, keep_rp = ost.spaces, ost.read_props
    try:
        # **セクションそのものの空間**（題は持つが `Page` が無い）と、
        # **本物のページ**（`Page` が居る）を並べる。
        sec = [{"oid": 0, "jcid": ost.JC_PAGEMETA, "stp": 0, "cb": 0}]
        page = [{"oid": 1, "jcid": ost.JC_PAGE, "stp": 0, "cb": 0},
                {"oid": 2, "jcid": ost.JC_PAGEMETA, "stp": 0, "cb": 0}]
        titles = {0: "セクションの名前", 2: "ほんとうのページ"}
        ost.spaces = lambda d: [("a", {1: sec}), ("b", {1: page})]
        ost.read_props = lambda d, o: ({ost.P_TITLE: titles[o["oid"]].encode("utf-16-le")}
                                       if o["oid"] in titles else {})
        got = ost.pages(b"")
        check("Page の無い空間は、ページにしない（空のノートを作らない）",
              [g["title"] for g in got] == ["ほんとうのページ"], got)
    finally:
        ost.spaces, ost.read_props = keep_sp, keep_rp


def t_cab(tmp):
    print("CAB の目録（日本語の名前）──")
    import struct as st

    def cab(entries, utf8=False):
        files = b""
        for name, size in entries:
            raw = name.replace("/", chr(92)).encode("utf-8" if utf8 else "cp932")
            files += st.pack("<IIHHHH", size, 0, 0, 0, 0, 0x80 if utf8 else 0) + raw + b"\0"
        head = bytearray(36)
        head[0:4] = b"MSCF"
        st.pack_into("<I", head, 16, 36)
        st.pack_into("<H", head, 26, len(entries))
        return bytes(head) + files

    at = tmp / "t.cab"
    want = [("400_打合せ/月_定例.one", 111), ("400_打合せ/金_定例.one", 222), ("表紙.one", 333)]
    # **`expand` は日本語の名前を壊す。** 中身は開かせて、名前はこちらで読む。
    for label, utf8 in (("cp932", False), ("UTF-8 の旗つき", True)):
        at.write_bytes(cab(want, utf8))
        got = o2m.cab_names(at)
        check(f"目録を読む（{label}）", got == want, got)
    # **フォルダはそのまま。** ここを潰すと多層が一段になり、取りこぼしに見える。
    check("名前の中の \\ は、フォルダの区切り",
          all("/" in n for n, _ in o2m.cab_names(at) if "打合せ" in n),
          o2m.cab_names(at))
    # **CAB の形をしていても、頭が MSCF でなければ読まない。** 中身が全部ゼロの
    # 偽物では足りない ── 読みにいっても空が返るので、検査が黙る。
    fake = bytearray(cab(want, utf8))
    fake[:4] = b"PK\x03\x04"
    at.write_bytes(bytes(fake))
    check("CAB でなければ、空を返す（決めつけない）", o2m.cab_names(at) == [],
          o2m.cab_names(at))


def t_ui(tmp):
    print("押して選ぶ小さい窓 ──")
    import importlib.util as iu, subprocess
    spec = iu.spec_from_file_location("onenote_ui", ROOT / "onenote_ui.py")
    ui = iu.module_from_spec(spec)
    spec.loader.exec_module(ui)

    # **走らせる一行は、Tk の外に出してある。** 中に置くと、Tk の要る機械で
    # しか確かめられない ── 押したときに何が走るのかを誰も試せなくなる。
    cmd = ui.command("C:\\out\\mesoso.onepkg", "C:\\Users\\t\\Documents\\OneNote")
    check("走らせるのは、いま動いている Python", cmd[0] == sys.executable, cmd)
    check("呼ぶのは隣の onenote2md.py", cmd[1].endswith("onenote2md.py"), cmd)
    check("出力先と取り込むものを渡す",
          cmd[2:] == ["--out", "C:\\Users\\t\\Documents\\OneNote", "C:\\out\\mesoso.onepkg"], cmd)

    # **ambər の既定の保存ディレクトリの下には掘らない**（本人が決めた）。
    out = ui._guess_out()
    check("出力先の下見は、ドキュメントの隣の OneNote", out.name == "OneNote", str(out))
    check("amber の下には掘らない", "amber" not in str(out).lower(), str(out))

    # **Tk が無い機械でも、黙って何もしないのではなく代わりの打ち方を出す。**
    src = (ROOT / "onenote_ui.py").read_text(encoding="utf-8")
    hidden = tmp / "tk無し"
    hidden.mkdir(exist_ok=True)
    (hidden / "onenote_ui.py").write_text(src, encoding="utf-8")
    (hidden / "tkinter.py").write_text("raise ImportError('tk なし')\n", encoding="utf-8")
    r = subprocess.run(
        [sys.executable, "-c",
         "import sys; sys.path.insert(0, '.'); import onenote_ui;"
         "sys.exit(onenote_ui.ask_and_run(type('A', (), {'files': '', 'out': ''})()))"],
        cwd=str(hidden), capture_output=True, text=True, errors="replace")
    both = r.stdout + r.stderr
    check("Tk が無ければ、代わりの打ち方を出して 1 を返す",
          r.returncode == 1 and "onenote2md.py --out" in both, both[-200:])

    # **バッチは、押しただけでも放り込まれても動く。**
    bat = (ROOT / "onenote2md.bat").read_text(encoding="utf-8", errors="replace")
    check("Python が無ければ、入れ方を言って止まる",
          "Python が見つかりません" in bat and "exit /b 1" in bat, "")
    check("押しただけなら窓を出す", 'if "%~1"=="" (' in bat, "")
    check("放り込まれたら、そのまま取り込む", "--out" in bat and "shift" in bat, "")
    # **日本語 Windows の既定は cp932。** ここを立てないと画面が化ける。
    check("先に chcp 65001 を打つ", "chcp 65001" in bat, "")
    check("既定の出力先は、ambər の下ではない",
          "Documents\\OneNote" in bat and "Documents\\amber" not in bat, "")


def main():
    tmp = Path(tempfile.mkdtemp(prefix="onenote-test-"))
    try:
        for fn in (t_names, t_select, t_cp932, t_offline, t_log, t_lock, t_cab, t_peek, t_from_files, t_ui,
                   t_onestore_shape, t_style, t_tables, t_empty_space):
            fn(tmp)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    print()
    if FAILED:
        print(f"NG {len(FAILED)} 件: " + " / ".join(FAILED))
        return 1
    print("すべて通りました。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
