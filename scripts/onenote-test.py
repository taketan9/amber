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

# **原本の `.pyc` を作らない。** 変異テストは同じパスのファイルを壊しては戻すので、
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
    走査だけが古い定義でラン、`AttributeError` で落ちる ── 画面には
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
    check("Windows で使えない文字", o2m.sanitize('a/b:c*d?e"f<g>h|i') == "a_b_c_d_e_f_g_h_i")
    check("SharePoint が断る文字", o2m.sanitize("a#b%c&d~e{f}g") == "a_b_c_d_e_f_g")
    check("_vti_ で始まらない", not o2m.sanitize("_vti_x").lower().startswith("_vti_"))
    check("予約名 CON", o2m.sanitize("CON") == "CON_")
    check("末尾の点と空白", o2m.sanitize("あ. ") == "あ")
    check("空なら fallback", o2m.sanitize("   ") == "untitled")
    check("日本語は削られない", o2m.sanitize("九月の定例") == "九月の定例")
    # **ここも 60 文字。** ambər の `file_stem` が 60 で切るので、120 のままだと
    # **長い題のページだけ画像の幹がずれ**、改名・移動した日に画像が付いてこない。
    check("ページの名前も 60 文字で切る", len(o2m.sanitize("あ" * 100)) == 60,
          len(o2m.sanitize("あ" * 100)))

    # **ambər の決まりに合わせた幹**（依頼 593）── ハイフンで繋ぎ、60 文字で切る。
    # `note::file_stem` が正本で、ここはその写し。**写しである以上ずれる。**
    check("幹は 60 文字で切る", len(o2m.amber_stem("あ" * 120)) == 60,
          len(o2m.amber_stem("あ" * 120)))
    check("使えない文字はハイフンに", o2m.amber_stem("斜/線") == "斜-線", o2m.amber_stem("斜/線"))
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
    print("ネットワークに出ない ──")
    # **会社の端末はネットワークに出られない**（本人・依頼 582）。取り込みのパスに
    # ネットワークを見に行く口が一つでもあると、そこで止まる。
    for name in ("onenote2md.py", "onestore.py", "onenote_ui.py"):
        src = (ROOT / name).read_text(encoding="utf-8")
        bad = [w for w in ("urllib.request", "requests.get", "http://", "https://",
                           "socket.create_connection", "pip install")
               if w in src and "example.com" not in src.split(w)[0][-40:]]
        check(f"ネットワークを見に行かせない（{name}）", not bad, bad)


# ---------------------------------------------------------------------------
# connect_onenote ── win32com ごと偽って、まとめ方の梯子を確かめる
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
    # `sys.exit("わけ")` の文字はどこにも出ないまま終わる。
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
    check("探すパスの例を出す", "365-1.one" in out or "365-2.one" in out, out)
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
    code, out = run(tmp / "そんなパスは無い")
    check("パスが無ければ、そう言う", "ありません" in out and code != 0, out)
    # ── `.onepkg` は入れ物 ──
    #
    # **中を見ないと形式は分からない。** ノートブックをまとめて出すと
    # この形にしかならない（OneNote が「.pdf .xps .onepkg だけ」と言う）。
    # **偽物では試さない** ── 本物の CAB を組んで、本当に開けるか見る。
    pkg = tmp / "pkg"
    pkg.mkdir(exist_ok=True)
    body = (b"\x00" * 48
            + __import__("uuid").UUID("109ADD3F-911B-49F5-A5D0-1791EDC8AED8").bytes_le
            + b"\x00" * 16)
    (pkg / "まとめ.onepkg").write_bytes(
        _cab([("400_打合せ/水曜日打合せ.one", body)], compress=True))
    _, out = run(pkg)
    check("入れ物を開いて、中の形式まで数える", "109add3f" in out, out)
    check("中のパスをそのまま出す",
          "  400_打合せ/水曜日打合せ.one" in out.splitlines(), out)
    check("圧縮の種類を言う", "MSZIP" in out, out)

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
    # 文字の無いことだけ見ていた版は、別の落ち方（パスが違う・ファイルが無い）を
    # 素通りさせた。
    check("よその場所から走らせても、隣の1 つを読める",
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
        # **同じ題が二枚。** 上書きすると文字が消えるので、ずらす。
        ost.pages = lambda d: [
            {"title": "9月の定例", "level": 1,
             "lines": [{"text": "決めたこと", "indent": 0},
                       {"text": "宿題", "indent": 1}]},
            {"title": "9月の定例", "level": 1, "lines": [{"text": "別の1 つ", "indent": 0}]},
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
        # **サブページは親ページ名のフォルダ**（COM のパスと同じ形）。
        check("サブページは親の下へ",
              "書き出し/議事録/9月の定例/補足.md" in got
              or "書き出し/議事録/9月の定例 (2)/補足.md" in got, got)
        body = (out / "書き出し" / "議事録" / "9月の定例.md").read_bytes()
        check("改行は LF", b"\r\n" not in body)
        text = body.decode("utf-8")
        check("前書きに題", 'title: "9月の定例"' in text, text[:120])
        check("本文が入る", "決めたこと" in text and "宿題" in text, text)
        # **何本目かを数で言う**（依頼 617）。デスクトップ版はこの行を読んで上に出す ──
        # 回っているだけの棒は、何も測っていなかった。
        import logging as _lg, io as _io
        buf = _io.StringIO()
        h = _lg.StreamHandler(buf)
        o2m.log.addHandler(h)
        was = o2m.log.level
        o2m.log.setLevel(_lg.INFO)
        try:
            o2m.run(_parse(["--out", str(tmp / "fromfiles-progress"), str(src)]),
                    tmp / "fromfiles-progress")
        finally:
            o2m.log.removeHandler(h)
            o2m.log.setLevel(was)
        check("いま何本目かを数で言う", "[1/1]" in buf.getvalue(), buf.getvalue()[:200])
        # 絞りも効く（COM のパスと同じ `--only`）。
        out2 = tmp / "fromfiles2"
        code = o2m.run(_parse(["--out", str(out2), str(src), "--only", "そんな名前は無い"]), out2)
        check("--only で外れたら書かない", not list(out2.rglob("*.md")) if out2.exists() else True)
        # 読めないファイルは、落ちずに数える。
        ost.pages = lambda d: (_ for _ in ()).throw(ValueError("壊れている"))
        out3 = tmp / "fromfiles3"
        # **落ちるのも「報告しない」の一種**（依頼 569）── 受け止めて NG にする。
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
        # 落ちるのも「報告しない」の一種 ── 受け止めて NG にする（依頼 569）。
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
        got = {"text": kw.pop("text", "文字"), "indent": kw.pop("indent", 0),
               "style": kw.pop("style", ""), "bold": False, "italic": False,
               "strike": False, "list": None, "todo": False, "y": 0, "x": 0}
        got.update(kw)
        return ost.as_markdown(got)

    check("見出しは #", md(text="決めたこと", style="h1") == "# 決めたこと", md(style="h1"))
    check("深い見出しも段に合わせる", md(style="h3") == "### 文字", md(style="h3"))
    check("見出しは 6 段まで", md(style="h9") == "###### 文字", md(style="h9"))
    check("太字は **", md(bold=True) == "**文字**", md(bold=True))
    check("斜体は *", md(italic=True) == "*文字*", md(italic=True))
    check("取り消し線は ~~", md(strike=True) == "~~文字~~", md(strike=True))
    check("箇条書きは -", md(list="bullet") == "- 文字", md(list="bullet"))
    check("番号は 1.", md(list="number") == "1. 文字", md(list="number"))
    check("チェックはセル", md(todo=True) == "- [ ] 文字", md(todo=True))
    check("引用は >", md(style="cite") == "> 文字", md(style="cite"))
    check("コードは枠", md(style="code") == "```\n文字\n```", md(style="code"))
    check("深さはインデント", md(indent=2, list="bullet") == "    - 文字", md(indent=2, list="bullet"))
    # **マークは外側から。** 中に入れると `**- 文字**` になって、箇条書きが消える。
    check("箇条書きのマークは、太字の外",
          md(list="bullet", bold=True) == "- **文字**", md(list="bullet", bold=True))

    # **ページ頭の日付と時刻は、本文ではない。**
    for pid in (ost.P_IS_DATE, ost.P_IS_TIME, ost.P_IS_BOILER, ost.P_IS_TITLE):
        got = ost.line_of({ost.P_ASCII: b"Friday, November 22, 2019", pid: 1})
        check(f"0x{pid:04X} の行は本文に混ぜない", got is None, got)
    check("ふつうの行は残る",
          ost.line_of({ost.P_ASCII: b"a"}) is not None)
    check("空の行は落とす", ost.line_of({ost.P_ASCII: b"   "}) is None)

    # **上から下、同じ高さなら左から右**（COM のパスと同じ潰し方）。
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
        check("色はマークの外、文字はマークの中",
              md == '<span style="color:#123456">**important**</span>', md)

        # 指し先が無い・壊れているときは、黙って素のテキストに戻る。
        check("たどれなければ書式なし", ost.style_of(b"", look, {}) == {})
        check("指し先が居なければ書式なし",
              ost.style_of(b"", look, {ost.P_STYLE: ("ref", 999)}) == {})

        # **リンクはいちばん内側。** 外に出すとマークがリンクの中に入る。
        props[7] = {ost.P_BOLD: 1, ost.P_LINK_URL: "https://x/a".encode("utf-16-le")}
        got = ost.line_of(props[1], ost.style_of(b"", look, props[1]))
        check("リンクの行き先が取れる", got["link"] == "https://x/a", got)
        check("リンクはマークの中", ost.as_markdown(got) == "**[important](https://x/a)**",
              ost.as_markdown(got))

        # 見出しはマークを重ねないが、色とリンクは残す。
        props[7] = {ost.P_COLOR: b"\x12\x34\x56\x00"}
        h = ost.line_of(props[1], ost.style_of(b"", look, props[1]))
        h["style"] = "h2"
        check("見出しにも色は付く",
              ost.as_markdown(h) == '## <span style="color:#123456">important</span>',
              ost.as_markdown(h))
    finally:
        ost.read_props = keep


def t_pictures(tmp):
    print("絵は、指し先から取る ──")
    import importlib.util as iu, struct as st
    spec = iu.spec_from_file_location("onestore", ROOT / "onestore.py")
    ost = iu.module_from_spec(spec)
    spec.loader.exec_module(ost)

    PNG = b"\x89PNG\r\n\x1a\n" + "ちいさな絵".encode() + b"\x00" * 8
    JPG = b"\xff\xd8\xff\xe0" + b"jfif" + b"\x00" * 12

    check("頭から種類が分かる（png）", ost.kind_of(PNG) == "png")
    check("頭から種類が分かる（jpg）", ost.kind_of(JPG) == "jpg")
    check("絵でなければ None", ost.kind_of(b"<?xml version=") is None)
    check("空でも落ちない", ost.kind_of(b"") is None and ost.kind_of(None) is None)
    # RIFF は 8 バイト目まで見ないと WebP と言えない。
    check("RIFF だけでは webp と言わない", ost.kind_of(b"RIFF" + b"\x00" * 8) is None)
    check("WEBP まで見て webp", ost.kind_of(b"RIFF" + b"\x00" * 4 + b"WEBP") == "webp")

    # **指し先から取る。** 前は画像オブジェクト自身のバイト列を出していて、
    # 出てくる .png は property set の生バイトだった（1 つも開けない）。
    keep = ost.read_props
    try:
        img = {"oid": 1, "jcid": ost.JC_IMAGE, "stp": 0, "cb": 0}
        blob = {"oid": 9, "jcid": 0x0000, "stp": 0, "cb": len(PNG)}
        props = {1: {ost.P_PICTURE: ("ref", 9),
                     ost.P_IMG_NAME: "猫.png".encode("utf-16-le")},
                 9: {}}
        ost.read_props = lambda d, o: props[o["oid"]]
        got = ost.pictures(PNG, [img, blob])
        check("指し先の中身を取る", len(got) == 1 and got[0]["bytes"] == PNG, got)
        check("種類も持って帰る", got and got[0]["kind"] == "png", got)
        check("名前も取る", got and got[0]["name"] == "猫.png", got)

        # **指し先が居なければ、出さない。**
        props[1] = {ost.P_PICTURE: ("ref", 999)}
        check("指し先が居なければ出さない", ost.pictures(PNG, [img, blob]) == [])
        # **参照を持たないものは、絵ではない。**
        props[1] = {ost.P_IMG_NAME: "猫.png".encode("utf-16-le")}
        check("参照が無ければ出さない", ost.pictures(PNG, [img, blob]) == [])

        # **`FileDataStoreObject` の頭（36 バイト）を外す。**
        head = b"\x01" * 16 + st.pack("<Q", len(PNG)) + b"\x00" * 4 + b"\x00" * 8
        wrapped = head + PNG + b"\x00" * 4
        blob2 = {"oid": 9, "jcid": 0x0000, "stp": 0, "cb": len(wrapped)}
        props[1] = {ost.P_PICTURE: ("ref", 9)}
        got = ost.pictures(wrapped, [img, blob2])
        check("頭を被せてあっても、絵だけ取り出す",
              len(got) == 1 and got[0]["bytes"] == PNG, got and got[0]["bytes"][:12])

        # **外側が既に絵なら、頭を外しにいかない。**
        #
        # 中の 16〜24 バイト目がたまたま長さらしい数になっていると、
        # 「頭が被さっている」と読み違えて**絵の途中から切り出す** ──
        # 名乗っているほうを信じる。
        inner = b"\x89PNG\r\n\x1a\n" + "なかみ".encode()
        tricky = (b"\x89PNG\r\n\x1a\n" + b"\x00" * 8
                  + st.pack("<Q", len(inner)) + b"\x00" * 12 + inner)
        blob3 = {"oid": 9, "jcid": 0x0000, "stp": 0, "cb": len(tricky)}
        props[1] = {ost.P_PICTURE: ("ref", 9)}
        got = ost.pictures(tricky, [img, blob3])
        check("外側が絵なら、そのまま出す",
              len(got) == 1 and got[0]["bytes"] == tricky, got and got[0]["bytes"][:12])

        # 埋め込みファイルも同じ道（`EmbeddedFileContainer`）。
        props[1] = {ost.P_FILE_BLOB: ("ref", 9), ost.P_FILE_NAME: "図.png".encode("utf-16-le")}
        got = ost.pictures(wrapped, [img, blob2])
        check("埋め込みファイルも同じ道", len(got) == 1 and got[0]["name"] == "図.png", got)
    finally:
        ost.read_props = keep


def t_table_refs(tmp):
    print("表は、指し先でたどる ──")
    import importlib.util as iu
    spec = iu.spec_from_file_location("onestore", ROOT / "onestore.py")
    ost = iu.module_from_spec(spec)
    spec.loader.exec_module(ost)
    keep = ost.read_props
    try:
        objs, props = [], {}

        def add(jc, *, kids=None, text=None):
            o = {"oid": len(objs) + 1, "jcid": jc, "stp": 0, "cb": 0}
            objs.append(o)
            pr = {}
            if kids is not None:
                pr[ost.P_KIDS] = ("refs", list(kids))
            if text is not None:
                if text.isascii():
                    pr[ost.P_ASCII] = text.encode("latin-1")
                else:
                    pr[ost.P_UNICODE] = text.encode("utf-16-le")
            props[o["oid"]] = pr
            return o["oid"]

        # **セルの中は、セル → アウトライン要素 → 本文 と下がる。**
        t11 = add(ost.JC_TEXT, text="name")
        t12 = add(ost.JC_TEXT, text="value")
        t21 = add(ost.JC_TEXT, text="apple")
        t22 = add(ost.JC_TEXT, text="120")
        o11 = add(ost.JC_OE, kids=[t11]); o12 = add(ost.JC_OE, kids=[t12])
        o21 = add(ost.JC_OE, kids=[t21]); o22 = add(ost.JC_OE, kids=[t22])
        c11 = add(ost.JC_CELL, kids=[o11]); c12 = add(ost.JC_CELL, kids=[o12])
        c21 = add(ost.JC_CELL, kids=[o21]); c22 = add(ost.JC_CELL, kids=[o22])
        r1 = add(ost.JC_ROW, kids=[c11, c12])
        r2 = add(ost.JC_ROW, kids=[c21, c22])
        add(ost.JC_TABLE, kids=[r1, r2])
        そと = add(ost.JC_TEXT, text="そとの文字")

        ost.read_props = lambda d, o: props[o["oid"]]
        got = ost.tables(b"", objs)
        check("指し先で表を組む", got and got[0]["rows"] == [["name", "value"], ["apple", "120"]],
              got)

        # **並び順を入れ替えても、同じ表になる。** ここが本題 ── 本物は
        # 改訂をまたぐと順が入れ替わる（現場で「表もぐちゃぐちゃ」と出た）。
        shuffled = list(reversed(objs))
        check("並びが入れ替わっても、同じ表",
              ost.tables(b"", shuffled) and
              ost.tables(b"", shuffled)[0]["rows"] == [["name", "value"], ["apple", "120"]],
              ost.tables(b"", shuffled))

        # 表の中の文字は、本文に二度出さない（指し先でたどって数える）。
        inside = ost.table_texts(b"", objs)
        check("表の中の文字を数える", len(inside) >= 4, len(inside))
        check("表の外の文字は数えない",
              id(objs[[o["oid"] for o in objs].index(そと)]) not in inside)

        # **本文に二度出さない** ── `_content` を通して、外の文字だけが残ること。
        keep_sp = ost.spaces
        try:
            ost.spaces = lambda d: [("os", {1: list(objs) + [
                {"oid": 999, "jcid": ost.JC_PAGE, "stp": 0, "cb": 0}]})]
            props[999] = {}
            pg = ost.pages(b"")[0]
            body = [l["text"] for l in pg["lines"]]
            check("表の中の文字は、本文に二度出さない", body == ["そとの文字"], body)
            check("表はちゃんと組める", pg["tables"] and
                  pg["tables"][0]["rows"] == [["name", "value"], ["apple", "120"]], pg["tables"])
        finally:
            ost.spaces = keep_sp

        # **行でないものを行として数えない。** 表の下にアウトラインが
        # ぶら下がっていることがあり、それを行に混ぜるとセルがずれる。
        # **セルを抱えた別物**を混ぜる ── 中身が空だと、行として数えても
        # 結果が変わらず、検査が報告しない（一度そうなった）。
        tx = add(ost.JC_TEXT, text="まぎれ")
        oe = add(ost.JC_OE, kids=[tx])
        cx = add(ost.JC_CELL, kids=[oe])
        まぎれ = add(ost.JC_OE, kids=[cx])
        tbl = next(o for o in objs if o["jcid"] == ost.JC_TABLE)
        props[tbl["oid"]] = {ost.P_KIDS: ("refs", [r1, まぎれ, r2])}
        got2 = ost.tables(b"", objs)
        check("行でないものは行にしない",
              got2 and got2[0]["rows"] == [["name", "value"], ["apple", "120"]], got2)
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
        check("表・行・セルをたどる", got and got[0]["rows"] == [["name", "value"], ["apple", "120"]],
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

        # **セルの数が行ごとに違う表は、揃えないと画面の上で崩れる。**
        # OneNote ではセルを結合できるので、揃っていない表は普通に出てくる。
        objs.clear(); props.clear()
        add(ost.JC_TABLE)
        add(ost.JC_ROW); add(ost.JC_CELL); add(ost.JC_TEXT, "a")
        add(ost.JC_CELL); add(ost.JC_TEXT, "b")
        add(ost.JC_ROW); add(ost.JC_CELL); add(ost.JC_TEXT, "c")
        md = ost.table_markdown(ost.tables(b"", objs)[0])
        check("セルの数を、行ごとに揃える",
              len({r.count("|") for r in md}) == 1 and "| c |  |" in md, md)

        # セルの中が複数行なら、空白で繋ぐ（Markdown の表に改行は入らない）。
        objs.clear(); props.clear()
        add(ost.JC_TABLE); add(ost.JC_ROW); add(ost.JC_CELL)
        add(ost.JC_TEXT, "one"); add(ost.JC_TEXT, "two")
        got = ost.tables(b"", objs)
        check("セルの中の改行は、空白で繋ぐ", got[0]["rows"] == [["one two"]], got)

        # **表の中の文字は、本文に二度出さない。**
        objs.clear(); props.clear()
        add(ost.JC_PAGE)
        add(ost.JC_TEXT, "そとの文字")
        add(ost.JC_TABLE); add(ost.JC_ROW); add(ost.JC_CELL); add(ost.JC_TEXT, "なかの文字")
        keep_sp = ost.spaces
        try:
            ost.spaces = lambda d: [("os", {1: list(objs)})]
            pg = ost.pages(b"")[0]
            body = [l["text"] for l in pg["lines"]]
            check("表の中の文字は、本文に二度出さない",
                  body == ["そとの文字"] and pg["tables"][0]["rows"] == [["なかの文字"]],
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

def t_revisions(tmp):
    print("改訂を重ねる ──")
    import importlib.util as iu
    spec = iu.spec_from_file_location("onestore", ROOT / "onestore.py")
    ost = iu.module_from_spec(spec)
    spec.loader.exec_module(ost)

    def obj(oid, jc):
        return {"oid": oid, "jcid": jc, "stp": 0, "cb": 0}

    # **OID で重ねる。** 同じものの新しい版が、古い版の居た場所に座る ──
    # 順は動かない（文書の上の並びが崩れると、本文が入れ替わって出る）。
    old_t = obj(7, ost.JC_TEXT)
    new_t = obj(7, ost.JC_TEXT)
    got = ost.merged({1: [obj(1, ost.JC_PAGE), old_t], 2: [new_t]})
    check("同じ OID は新しいほうを採る", got[1] is new_t, got)
    check("古い版の居た場所のまま", [o["oid"] for o in got] == [1, 7], got)
    check("改訂が無ければ、空", ost.merged({}) == [], ost.merged({}))

    keep_sp, keep_rp = ost.spaces, ost.read_props
    try:
        # **最後の改訂は「変えたところ」しか持っていないことがある。**
        # `Page` のラベルも本文も前の改訂に置きっぱなしで、最後だけを見ると
        # **ページまるごと取りこぼす**（現場で「中身がほぼ入っていない」）。
        first = [obj(1, ost.JC_PAGE), obj(2, ost.JC_PAGEMETA), obj(3, ost.JC_TEXT)]
        later = [obj(4, ost.JC_PAGEMETA)]
        body = {3: "本文だ"}
        titles = {2: "むかしの題", 4: "いまの題"}

        def props(d, o):
            if o["oid"] in titles:
                return {ost.P_TITLE: titles[o["oid"]].encode("utf-16-le")}
            if o["oid"] in body:
                return {ost.P_UNICODE: body[o["oid"]].encode("utf-16-le")}
            return {}

        ost.spaces = lambda d: [("a", {1: first, 2: later})]
        ost.read_props = props
        got = ost.pages(b"")
        check("最後の改訂に Page が無くても、取りこぼさない", len(got) == 1, got)
        if got:
            check("前の改訂の本文を拾う",
                  [l["text"] for l in got[0]["lines"]] == ["本文だ"], got[0]["lines"])
            check("題はいちばん新しいものを採る", got[0]["title"] == "いまの題", got[0])

        # **同じ場所の同じ文字は一つ。** 重ねて拾うと同じ行が二つ並ぶことがある。
        ost.spaces = lambda d: [("a", {1: [obj(1, ost.JC_PAGE), obj(3, ost.JC_TEXT),
                                           obj(5, ost.JC_TEXT)]})]
        ost.read_props = lambda d, o: ({ost.P_UNICODE: "おなじ".encode("utf-16-le")}
                                       if o["jcid"] == ost.JC_TEXT else {})
        got = ost.pages(b"")
        check("同じ場所の同じ文字は一つにまとめる",
              got and [l["text"] for l in got[0]["lines"]] == ["おなじ"], got)

        # **本文も題も無い空間は、ノートにしない。**
        ost.spaces = lambda d: [("a", {1: [obj(1, ost.JC_PAGE)]})]
        ost.read_props = lambda d, o: {}
        check("題も本文も無ければ、ノートを作らない", ost.pages(b"") == [], ost.pages(b""))
    finally:
        ost.spaces, ost.read_props = keep_sp, keep_rp


def _cab(entries, *, utf8_flag=False, encoding="utf-8", compress=False, block=32768):
    """**本物の CAB を組む。**（無圧縮 / MSZIP）

    `entries` は `[(入れ物の中の道, 中身)]`。前の走査は偽物を組んでいて、
    **ファイルの数を 26 バイト目に書いていた** ── 本体の読み方と同じずれ。
    偽物が本物より甘いと、検査は間違いを一緒に抱いて黙る（七度目・依頼 617）。
    ここは実際の CAB と同じ並びで組むので、頭のずれはそのまま NG になる。
    """
    import struct as st
    import zlib

    blob = b"".join(body for _, body in entries)
    blocks, hist = [], b""
    for k in range(0, max(len(blob), 1), block):
        chunk = blob[k:k + block]
        if compress:
            co = (zlib.compressobj(9, zlib.DEFLATED, -15, zdict=hist) if hist
                  else zlib.compressobj(9, zlib.DEFLATED, -15))
            data = b"CK" + co.compress(chunk) + co.flush()
        else:
            data = chunk
        blocks.append((data, len(chunk)))
        hist = (hist + chunk)[-32768:]

    files, at = b"", 0
    for name, body in entries:
        attribs = 0x20 | (0x80 if utf8_flag else 0)
        raw = name.replace("/", chr(92)).encode(encoding)
        files += st.pack("<IIHHHH", len(body), at, 0, 0, 0, attribs) + raw + b"\0"
        at += len(body)

    coff_files = 36 + 8
    data_off = coff_files + len(files)
    folder = st.pack("<IHH", data_off, len(blocks), 1 if compress else 0)
    data = b"".join(st.pack("<IHH", 0, len(d), cbu) + d for d, cbu in blocks)
    total = data_off + len(data)
    head = st.pack("<4sIIIIIBBHHHHH", b"MSCF", 0, total, 0, coff_files, 0, 3, 1,
                   1, len(entries), 0, 0, 0)
    assert len(head) == 36, len(head)
    return head + folder + files + data


def t_cab(tmp):
    print("CAB を自分でほどく ──")
    want = [("400_打合せ/水曜日打合せ.one", b"A" * 111),
            ("400_打合せ/金曜日打合せ.one", b"B" * 222),
            ("表紙.onetoc2", b"C" * 333)]
    names = [n for n, _ in want]

    at = tmp / "t.onepkg"
    # **名前の符号は三通り試す。** 現場で化けたのは「UTF-8 なのに旗が立って
    # いない」形 ── 数字はそのまま出て、漢字だけが読めなくなる。
    for label, kw in (("UTF-8 の旗つき", dict(utf8_flag=True, encoding="utf-8")),
                      ("旗なしの UTF-8（OneNote はこれ）", dict(encoding="utf-8")),
                      ("旗なしの cp932", dict(encoding="cp932"))):
        at.write_bytes(_cab(want, **kw))
        got = o2m.cab_names(at)
        check(f"目録の名前を読む（{label}）", [n for n, _ in got] == names, got)
        check(f"大きさも読む（{label}）",
              [cb for _, cb in got] == [len(b) for _, b in want], got)

    # **ファイルの数は 28 バイト目。** 26（フォルダの数）を読むと、
    # 目録が 1 本で切れる ── 現場で「セクションが 3 つしかできない」と出た顔。
    at.write_bytes(_cab(want))
    check("フォルダの数ではなくファイルの数を読む", len(o2m.cab_names(at)) == 3,
          o2m.cab_names(at))

    # **ほどいて、中身も名前も階層も合っているか。**
    for label, comp in (("無圧縮", False), ("MSZIP", True)):
        at.write_bytes(_cab(want, compress=comp))
        into = tmp / f"opened-{label}"
        entries, why = o2m.unpack_onepkg(at, into)
        check(f"開ける（{label}）", entries is not None, why)
        if entries is None:
            continue
        check(f"目録のパスのまま出す（{label}）", [n for n, _ in entries] == names, entries)
        for (name, q), (_n, body) in zip(entries, want):
            check(f"中身が合う（{label}・{name}）", q.read_bytes() == body,
                  f"{len(q.read_bytes())} ≠ {len(body)}")
        # **セクショングループはフォルダのまま。** ここが潰れると多層が
        # 一段になり、取りこぼしに見える（依頼 597）。
        check(f"セクショングループはフォルダのまま（{label}）",
              (into / "400_打合せ" / "水曜日打合せ.one").is_file(),
              sorted(str(q.relative_to(into)) for q in into.rglob("*")))

    # **MSZIP は塊をまたぐ。** 一塊に収まるサンプルでは、前の塊を辞書に使うパスが
    # 一度も通らない ── 32KB を超える中身で確かめる。
    big = [("長い/中身.one", bytes(range(256)) * 400)]     # 102,400 バイト
    at.write_bytes(_cab(big, compress=True, block=32768))
    entries, why = o2m.unpack_onepkg(at, tmp / "opened-big")
    check("塊をまたいでも中身が合う（MSZIP）",
          entries is not None and entries[0][1].read_bytes() == big[0][1], why)

    # **入れ物の言うパスを、そのまま信じない。** `..` を入れた CAB を渡されたら、
    # 出力先の外に書ける。
    evil = [("../../逃げる.one", b"X" * 9)]
    at.write_bytes(_cab(evil))
    into = tmp / "opened-evil"
    entries, _why = o2m.unpack_onepkg(at, into)
    check("上の階へ出さない（.. は落とす）",
          entries and entries[0][1] == into / "逃げる.one", entries)

    # **セクションのパスは、目録からビルドする。**
    at.write_bytes(_cab(want))
    entries, _ = o2m.unpack_onepkg(at, tmp / "opened-sec")
    secs = o2m.opened_sections(tmp / "みそそ.onepkg", entries)
    check(".one だけを拾う（目次は写さない）", len(secs) == 2, secs)
    check("ノートブック・グループ・セクションに分かれる",
          [(b, g, n) for b, g, _q, n in secs]
          == [("みそそ", ["400_打合せ"], "水曜日打合せ"),
              ("みそそ", ["400_打合せ"], "金曜日打合せ")], secs)

    # **CAB でなければ、決めつけない。**
    at.write_bytes(b"PK\x03\x04" + b"\x00" * 60)
    check("CAB でなければ、空を返す", o2m.cab_names(at) == [], o2m.cab_names(at))
    got, why = o2m.unpack_onepkg(at, tmp / "opened-not")
    # **わけはそのまま言う。** 「目録が空」と言い換えると、頭を見ずに
    # 中身を読みにいってたまたま空だった回と見分けがつかない。
    check("CAB でなければ、わけを言う", got is None and why == "CAB ではない", why)


def t_ui(tmp):
    print("押して選ぶ小さいウィンドウ ──")
    import importlib.util as iu, subprocess
    spec = iu.spec_from_file_location("onenote_ui", ROOT / "onenote_ui.py")
    ui = iu.module_from_spec(spec)
    spec.loader.exec_module(ui)

    # **走らせる一行は、Tk の外に出してある。** 中に置くと、Tk の要る環境で
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

    # **Tk が無い環境でも、黙って何もしないのではなく代わりの打ち方を出す。**
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

    # **回っているだけの棒は置かない**（依頼 617）。本人の端末で一度も動かず、
    # 「止まっている」ようにしか見えなかった ── 測っていないものを、
    # 測っている顔で見せない。
    check("回っているだけの棒を置かない", "Progressbar" not in src, "")
    check("本体の [n/m] を読んで、上の一行に出す",
          "(\\d+)/(\\d+)" in src and "写しています" in src, "")

    # **バッチは、押しただけでも放り込まれても動く。**
    bat = (ROOT / "onenote2md.bat").read_text(encoding="utf-8", errors="replace")
    check("Python が無ければ、入れ方を言って止まる",
          "Python が見つかりません" in bat and "exit /b 1" in bat, "")
    check("押しただけならデスクトップ版を出す", 'if "%~1"=="" (' in bat, "")
    check("放り込まれたら、そのまま取り込む", "--out" in bat and "shift" in bat, "")
    # **日本語 Windows の既定は cp932。** ここを立てないと画面が化ける。
    check("先に chcp 65001 を打つ", "chcp 65001" in bat, "")
    check("既定の出力先は、ambər の下ではない",
          "Documents\\OneNote" in bat and "Documents\\amber" not in bat, "")


def main():
    tmp = Path(tempfile.mkdtemp(prefix="onenote-test-"))
    try:
        for fn in (t_names, t_select, t_cp932, t_offline, t_log, t_lock, t_cab, t_peek, t_from_files, t_ui,
                   t_onestore_shape, t_style, t_pictures, t_table_refs, t_tables, t_empty_space, t_revisions):
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
