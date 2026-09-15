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

# **原本を取っておく。** `run()` は `o2m.connect_onenote` を偽物に差し替えるので、
# あとで繋ぎ方そのものを試すときには、差し替えられた側を呼んでしまう
# （そして「繋がらない」ではなく「前のテストの偽物が返る」という、
# いちばん読みにくい落ち方をする）。
ORIG_CONNECT = o2m.connect_onenote

PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
)
PNG_B64 = base64.b64encode(PNG).decode()

NBS = 'xmlns:one="http://schemas.microsoft.com/office/onenote/2013/onenote"'


def hierarchy(mod_p1="2026-09-10T02:00:00.000Z") -> str:
    return f"""<?xml version="1.0"?>
<one:Notebooks {NBS}>
  <one:Notebook name="仕事" ID="{{NB1}}">
    <one:Section name="議事録" ID="{{S1}}">
      <one:Page ID="{{P1}}" name="9月の定例" pageLevel="1"
                dateTime="2026-09-01T01:00:00.000Z" lastModifiedTime="{mod_p1}"/>
      <one:Page ID="{{P2}}" name="補足" pageLevel="2"
                dateTime="2026-09-01T01:30:00.000Z" lastModifiedTime="2026-09-02T02:00:00.000Z"/>
      <one:Page ID="{{P9}}" name="ゴミ" pageLevel="1" isInRecycleBin="true"
                dateTime="2026-09-01T01:00:00.000Z" lastModifiedTime="2026-09-01T01:00:00.000Z"/>
    </one:Section>
    <one:SectionGroup name="案件" ID="{{G1}}">
      <one:Section name="A社" ID="{{S2}}">
        <one:Page ID="{{P3}}" name="見積" pageLevel="1"
                  dateTime="2026-09-03T01:00:00.000Z" lastModifiedTime="2026-09-03T02:00:00.000Z"/>
      </one:Section>
    </one:SectionGroup>
    <one:Section name="秘密" ID="{{S3}}" locked="true"/>
    <one:SectionGroup name="OneNote_RecycleBin" ID="{{G2}}" isRecycleBin="true">
      <one:Section name="消したもの" ID="{{S5}}">
        <one:Page ID="{{P8}}" name="消えた" pageLevel="1"
                  dateTime="2026-09-01T01:00:00.000Z" lastModifiedTime="2026-09-01T01:00:00.000Z"/>
      </one:Section>
    </one:SectionGroup>
  </one:Notebook>
  <one:Notebook name="私用" ID="{{NB2}}">
    <one:Section name="買い物" ID="{{S4}}">
      <one:Page ID="{{P4}}" name="週末" pageLevel="1"
                dateTime="2026-09-05T01:00:00.000Z" lastModifiedTime="2026-09-05T02:00:00.000Z"/>
    </one:Section>
  </one:Notebook>
</one:Notebooks>"""


def page_xml(pid, title, mod, images=0) -> str:
    imgs = "".join(
        f'<one:OE quickStyleIndex="1"><one:Image format="png"><one:Data>{PNG_B64}</one:Data></one:Image></one:OE>'
        for _ in range(images)
    )
    return f"""<?xml version="1.0"?>
<one:Page {NBS} ID="{pid}" dateTime="2026-09-01T01:00:00.000Z" lastModifiedTime="{mod}">
  <one:QuickStyleDef index="0" name="h1"/>
  <one:QuickStyleDef index="1" name="p"/>
  <one:QuickStyleDef index="2" name="cite"/>
  <one:TagDef index="0" name="To Do"/>
  <one:Title><one:OE><one:T><![CDATA[{title}]]></one:T></one:OE></one:Title>
  <one:Outline>
    <one:Position x="36" y="86"/>
    <one:OEChildren>
      <one:OE quickStyleIndex="0"><one:T><![CDATA[見出し]]></one:T></one:OE>
      <one:OE quickStyleIndex="1"><one:T><![CDATA[ふつうの<span style='font-weight:bold'>太い</span>字]]></one:T></one:OE>
      <one:OE quickStyleIndex="2"><one:T><![CDATA[引用]]></one:T></one:OE>
      <one:OE quickStyleIndex="1"><one:List><one:Bullet bullet="2"/></one:List><one:T><![CDATA[箇条]]></one:T></one:OE>
      <one:OE quickStyleIndex="1"><one:Tag index="0" completed="false"/><one:T><![CDATA[やること]]></one:T></one:OE>
      <one:OE quickStyleIndex="1"><one:T><![CDATA[<a href="https://example.com">外</a>]]></one:T></one:OE>
      <one:OE quickStyleIndex="1">
        <one:Table hasHeaderRow="true">
          <one:Row>
            <one:Cell><one:OEChildren><one:OE><one:T><![CDATA[名]]></one:T></one:OE></one:OEChildren></one:Cell>
            <one:Cell><one:OEChildren><one:OE><one:T><![CDATA[値]]></one:T></one:OE></one:OEChildren></one:Cell>
          </one:Row>
          <one:Row>
            <one:Cell><one:OEChildren><one:OE><one:T><![CDATA[あ]]></one:T></one:OE></one:OEChildren></one:Cell>
            <one:Cell><one:OEChildren><one:OE><one:T><![CDATA[い]]></one:T></one:OE></one:OEChildren></one:Cell>
          </one:Row>
        </one:Table>
      </one:OE>
      {imgs}
    </one:OEChildren>
  </one:Outline>
</one:Page>"""


class FakeOneNote:
    """COM の口だけ真似る。返すのは本物と同じ形の XML の文字列。"""

    def __init__(self, hier, pages, fail=()):
        self.hier = hier
        self.pages = pages
        self.fail = set(fail)
        self.synced = []
        self.fetched = []
        self.hier_calls = 0

    def GetHierarchy(self, start, scope, out=""):
        self.hier_calls += 1
        return self.hier

    def GetPageContent(self, pid, out, info):
        if pid in self.fail:
            raise RuntimeError("COM がしゃっくりした")
        self.fetched.append(pid)
        return self.pages[pid]

    def SyncHierarchy(self, hid):
        self.synced.append(hid)


def pages_for(mod_p1="2026-09-10T02:00:00.000Z", p1_images=0):
    return {
        "{P1}": page_xml("{P1}", "9月の定例", mod_p1, images=p1_images),
        "{P2}": page_xml("{P2}", "補足", "2026-09-02T02:00:00.000Z"),
        "{P3}": page_xml("{P3}", "見積", "2026-09-03T02:00:00.000Z"),
        "{P4}": page_xml("{P4}", "週末", "2026-09-05T02:00:00.000Z"),
        # ゴミ箱のページも OneNote からは取れる。取れない偽物にしておくと、
        # 「ゴミ箱も出す」ように壊したとき**取得が落ちるだけ**で、
        # 「ゴミ箱のページは出ない」が通ってしまう（検査が黙る）。
        "{P9}": page_xml("{P9}", "ゴミ", "2026-09-01T01:00:00.000Z"),
    }


def run(app, out, *argv):
    """本体の `run()` を呼ぶ（`main()` の鎖と logging は通さない）。

    `connect_onenote()` は **(相手, 最初の階層 XML)** を返す ── 繋ぐときに
    一度は訊いているので、その答えを捨てずに使うため。偽物も同じ形で返す。
    """
    o2m.connect_onenote = lambda: (app, o2m.get_hierarchy(app))
    ap_argv = ["--out", str(out)] + list(argv)
    parser_args = _parse(ap_argv)
    stats_before = {}
    code = o2m.run(parser_args, Path(out))
    return code, stats_before


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


def t_structure(tmp):
    print("階層とファイル ──")
    out = tmp / "structure"
    app = FakeOneNote(hierarchy(), pages_for(p1_images=1))
    run(app, out)

    check("ノートブックがフォルダになる", (out / "仕事").is_dir())
    check("セクションがフォルダになる", (out / "仕事" / "議事録").is_dir())
    check("セクショングループも一段になる", (out / "仕事" / "案件" / "A社").is_dir())
    check("ページが .md になる", (out / "仕事" / "議事録" / "9月の定例.md").is_file())
    check("サブページは親ページ名のフォルダへ",
          (out / "仕事" / "議事録" / "9月の定例" / "補足.md").is_file())
    check("別のノートブックも出る", (out / "私用" / "買い物" / "週末.md").is_file())
    check("鍵のかかったセクションは出ない", not (out / "仕事" / "秘密").exists())
    check("ゴミ箱のセクショングループは出ない", not (out / "仕事" / "OneNote_RecycleBin").exists())
    check("ゴミ箱のページは出ない", not (out / "仕事" / "議事録" / "ゴミ.md").exists())

    raw = (out / "仕事" / "議事録" / "9月の定例.md").read_bytes()
    text = raw.decode("utf-8")
    check("改行は LF", b"\r\n" not in raw)
    check("前書きに title", 'title: "9月の定例"' in text)
    check("前書きに created（日付だけ）", "created: 2026-09-01\n" in text)
    check("前書きに onenote_id", 'onenote_id: "{P1}"' in text)
    check("前書きに onenote_path", 'onenote_path: "仕事 / 議事録"' in text)
    check("見出しが # になる", "\n# 見出し" in text)
    check("太字が ** になる", "ふつうの**太い**字" in text)
    check("引用が > になる", "\n> 引用" in text)
    check("箇条書きが - になる", "\n- 箇条" in text)
    check("To Do がチェックボックスになる", "- [ ] やること" in text)
    check("リンクが [..](..) になる", "[外](https://example.com)" in text)
    check("表が組まれる（見出しの行あり）",
          "| 名 | 値 |\n| --- | --- |\n| あ | い |" in text)
    check("画像が attachments/ に落ちる",
          (out / "仕事" / "議事録" / "attachments" / "9月の定例-001.png").is_file())
    check("画像へのリンクが相対", "](attachments/9月の定例-001.png)" in text)


def t_incremental(tmp):
    print("変わっていないページは書かない ──")
    out = tmp / "incr"
    app = FakeOneNote(hierarchy(), pages_for())
    run(app, out)
    first = len(app.fetched)
    check("一回目は全ページ取りに行く", first == 4, f"{first}")

    app2 = FakeOneNote(hierarchy(), pages_for())
    run(app2, out)
    check("二回目は一枚も取りに行かない", app2.fetched == [], f"{app2.fetched}")

    md = out / "仕事" / "議事録" / "9月の定例.md"
    before = md.stat().st_mtime_ns
    app3 = FakeOneNote(hierarchy(), pages_for())
    run(app3, out)
    check("触っていないファイルの更新時刻が動かない", md.stat().st_mtime_ns == before)

    app4 = FakeOneNote(hierarchy(mod_p1="2026-09-14T09:00:00.000Z"),
                       pages_for(mod_p1="2026-09-14T09:00:00.000Z"))
    run(app4, out)
    check("変わったページだけ取りに行く", app4.fetched == ["{P1}"], f"{app4.fetched}")

    app5 = FakeOneNote(hierarchy(mod_p1="2026-09-14T09:00:00.000Z"),
                       pages_for(mod_p1="2026-09-14T09:00:00.000Z"))
    run(app5, out, "--force")
    check("--force なら全部書き直す", len(app5.fetched) == 4, f"{app5.fetched}")


def t_same_file(tmp):
    print("同じページは同じファイル ──")
    out = tmp / "same"
    for i in range(3):
        app = FakeOneNote(hierarchy(mod_p1=f"2026-09-1{i}T09:00:00.000Z"),
                          pages_for(mod_p1=f"2026-09-1{i}T09:00:00.000Z"))
        run(app, out)
    got = sorted(p.name for p in (out / "仕事" / "議事録").glob("*.md"))
    check("三回走らせても増えない", got == ["9月の定例.md"], f"{got}")


def t_prune_scope(tmp):
    print("--prune は歩いた場所だけ ──")
    out = tmp / "prune"
    app = FakeOneNote(hierarchy(), pages_for())
    run(app, out)

    # OneNote 側で消えたページ（前回の写しだけが残っている）
    gone = out / "仕事" / "議事録" / "むかしのページ.md"
    gone.write_text('---\ntitle: "むかし"\nonenote_id: "{P7}"\n---\n\n本文\n',
                    encoding="utf-8")
    # ambər で自分が書いたノート（onenote_id を持たない）
    mine = out / "仕事" / "議事録" / "自分のメモ.md"
    mine.write_text('---\ntitle: "自分の"\n---\n\n本文\n', encoding="utf-8")
    # 鍵のかかったセクションの、前回までの写し
    locked = out / "仕事" / "秘密"
    locked.mkdir(parents=True, exist_ok=True)
    (locked / "鍵の中.md").write_text('---\ntitle: "鍵"\nonenote_id: "{P6}"\n---\n\n本文\n',
                                      encoding="utf-8")

    app2 = FakeOneNote(hierarchy(), pages_for())
    run(app2, out, "--notebook", "仕事", "--prune")

    check("消えたページの写しは消える", not gone.exists())
    check("自分で書いたノートは残る", mine.exists())
    check("鍵のかかったセクションの下は残る", (locked / "鍵の中.md").exists())
    check("絞らなかったノートブックは残る（前は全滅した）",
          (out / "私用" / "買い物" / "週末.md").is_file())


def t_prune_error(tmp):
    print("取れなかったページを、消えたページと呼ばない ──")
    out = tmp / "err"
    app = FakeOneNote(hierarchy(), pages_for())
    run(app, out)
    victim = out / "仕事" / "案件" / "A社" / "見積.md"
    check("下ごしらえ: 一回目で書けている", victim.is_file())

    # 中身が変わったことにして、しかも取得が落ちる
    hier = hierarchy().replace(
        'ID="{P3}" name="見積" pageLevel="1"\n                  dateTime="2026-09-03T01:00:00.000Z" lastModifiedTime="2026-09-03T02:00:00.000Z"',
        'ID="{P3}" name="見積" pageLevel="1"\n                  dateTime="2026-09-03T01:00:00.000Z" lastModifiedTime="2026-09-14T09:00:00.000Z"')
    app2 = FakeOneNote(hier, pages_for(), fail=["{P3}"])
    o2m.connect_onenote = lambda: (app2, o2m.get_hierarchy(app2))
    code = o2m.run(_parse(["--out", str(out), "--prune"]), out)

    check("落ちた回は 0 を返さない", code == 1, f"{code}")
    check("前の版が残る", victim.is_file())


def t_stale_images(tmp):
    print("書き直したページの、古い画像 ──")
    out = tmp / "img"
    app = FakeOneNote(hierarchy(), pages_for(p1_images=2))
    run(app, out)
    att = out / "仕事" / "議事録" / "attachments"
    check("下ごしらえ: 画像が二枚", (att / "9月の定例-001.png").is_file()
          and (att / "9月の定例-002.png").is_file())

    # 隣のページの画像（巻き込まれてはいけない）。**ハイフンは名前の中にも
    # 出る**ので、`9月の定例-補足-001.png` が `9月の定例-*` に当たってしまう ──
    # アンダースコアのときより広く当たる。番号の形まで見て外す。
    (att / "9月の定例録-001.png").write_bytes(PNG)
    (att / "9月の定例-補足-001.png").write_bytes(PNG)

    app2 = FakeOneNote(hierarchy(mod_p1="2026-09-14T09:00:00.000Z"),
                       pages_for(mod_p1="2026-09-14T09:00:00.000Z", p1_images=1))
    run(app2, out)
    check("いま使っている画像は残る", (att / "9月の定例-001.png").is_file())
    check("使わなくなった画像は消える", not (att / "9月の定例-002.png").exists())
    check("名前が似ているだけの画像は巻き込まない",
          (att / "9月の定例録-001.png").is_file()
          and (att / "9月の定例-補足-001.png").is_file(),
          sorted(q.name for q in att.iterdir()))

    # --no-images のときに全部消したりしない
    app3 = FakeOneNote(hierarchy(mod_p1="2026-09-14T10:00:00.000Z"),
                       pages_for(mod_p1="2026-09-14T10:00:00.000Z", p1_images=1))
    run(app3, out, "--no-images")
    check("--no-images でも既にある画像は消さない", (att / "9月の定例-001.png").is_file())


def t_sync(tmp):
    print("読む前に同期を頼む ──")
    out = tmp / "sync"
    app = FakeOneNote(hierarchy(), pages_for())
    run(app, out, "--sync", "--sync-wait", "0")
    check("開いているノートブックぜんぶに頼む", app.synced == ["{NB1}", "{NB2}"], f"{app.synced}")

    app2 = FakeOneNote(hierarchy(), pages_for())
    run(app2, out, "--sync", "--sync-wait", "0", "--notebook", "私用")
    check("絞ったら、そのノートブックだけに頼む", app2.synced == ["{NB2}"], f"{app2.synced}")

    app3 = FakeOneNote(hierarchy(), pages_for())
    run(app3, out, "--sync", "--sync-wait", "0", "--dry-run")
    check("--dry-run では頼まない", app3.synced == [], f"{app3.synced}")


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


def _mds(out):
    return sorted(str(q.relative_to(out)) for q in out.rglob("*.md"))


def t_select(tmp):
    print("写すものを選ぶ ──")
    out = tmp / "sel"
    app = FakeOneNote(hierarchy(), pages_for())
    run(app, out, "--only", "議事録")
    check("--only はそのセクションだけ",
          _mds(out) == ["仕事/議事録/9月の定例.md", "仕事/議事録/9月の定例/補足.md"], f"{_mds(out)}")

    out = tmp / "sel2"
    run(FakeOneNote(hierarchy(), pages_for()), out, "--only", "仕事")
    check("--only にノートブック名を書けば、その下ぜんぶ",
          _mds(out) == ["仕事/案件/A社/見積.md", "仕事/議事録/9月の定例.md",
                        "仕事/議事録/9月の定例/補足.md"], f"{_mds(out)}")

    out = tmp / "sel3"
    run(FakeOneNote(hierarchy(), pages_for()), out, "--only", "案件/A社")
    check("--only は道で当たる（グループ/セクション）",
          _mds(out) == ["仕事/案件/A社/見積.md"], f"{_mds(out)}")

    out = tmp / "sel4"
    run(FakeOneNote(hierarchy(), pages_for()), out, "--skip", "私用")
    check("--skip は外す",
          _mds(out) == ["仕事/案件/A社/見積.md", "仕事/議事録/9月の定例.md",
                        "仕事/議事録/9月の定例/補足.md"], f"{_mds(out)}")

    out = tmp / "sel5"
    run(FakeOneNote(hierarchy(), pages_for()), out, "--only", "仕事", "--skip", "案件")
    check("--skip は --only より強い",
          _mds(out) == ["仕事/議事録/9月の定例.md", "仕事/議事録/9月の定例/補足.md"], f"{_mds(out)}")

    out = tmp / "sel6"
    run(FakeOneNote(hierarchy(), pages_for()), out, "--only", "ABC")
    check("当たらなければ一枚も出ない", _mds(out) == [], f"{_mds(out)}")

    out = tmp / "sel7"
    run(FakeOneNote(hierarchy(), pages_for()), out, "--only", "a社")
    check("大文字小文字は問わない", _mds(out) == ["仕事/案件/A社/見積.md"], f"{_mds(out)}")

    out = tmp / "sel8"
    run(FakeOneNote(hierarchy(), pages_for()), out, "--only", "議事録", "--only", "買い物")
    check("--only は重ねられる",
          _mds(out) == ["仕事/議事録/9月の定例.md", "仕事/議事録/9月の定例/補足.md",
                        "私用/買い物/週末.md"], f"{_mds(out)}")


def t_select_flatten(tmp):
    print("絞りは OneNote の道に当たる（畳んだ名前ではなく）──")
    out = tmp / "flat"
    run(FakeOneNote(hierarchy(), pages_for()), out, "--flatten-groups", "--only", "案件/A社")
    check("畳んでも OneNote の道で拾える",
          _mds(out) == ["仕事/案件 › A社/見積.md"], f"{_mds(out)}")

    out = tmp / "flat2"
    run(FakeOneNote(hierarchy(), pages_for()), out, "--flatten-groups", "--only", "案件 › A社")
    check("畳んだ名前では拾わない（同じ --only が旗で違うものを拾わない）",
          _mds(out) == [], f"{_mds(out)}")


def t_select_prune(tmp):
    print("絞りと --prune ──")
    out = tmp / "selprune"
    run(FakeOneNote(hierarchy(), pages_for()), out)
    check("下ごしらえ: 全部出ている", len(_mds(out)) == 4, f"{_mds(out)}")

    # OneNote 側で消えたページ（絞って写す側にある）
    gone = out / "仕事" / "議事録" / "むかしのページ.md"
    gone.write_text('---\ntitle: "むかし"\nonenote_id: "{P7}"\n---\n\n本文\n', encoding="utf-8")

    run(FakeOneNote(hierarchy(), pages_for()), out, "--only", "議事録", "--prune")

    check("絞って外したセクションの写しは残る（前は全滅した）",
          (out / "仕事" / "案件" / "A社" / "見積.md").is_file()
          and (out / "私用" / "買い物" / "週末.md").is_file())
    check("絞って写した側では、消えたページの写しは消える", not gone.exists())

    out2 = tmp / "selprune2"
    run(FakeOneNote(hierarchy(), pages_for()), out2)
    run(FakeOneNote(hierarchy(), pages_for()), out2, "--flatten-groups", "--only", "議事録", "--prune")
    check("畳んだときも、外したセクションの写しは残る",
          (out2 / "仕事" / "案件" / "A社" / "見積.md").is_file())


def t_list(tmp):
    print("--list ──")
    import io
    import contextlib as _c

    buf = io.StringIO()
    with _c.redirect_stdout(buf):
        run(FakeOneNote(hierarchy(), pages_for()), tmp / "list", "--list")
    got = buf.getvalue()
    check("セクションを道で並べる", "仕事/議事録" in got and "仕事/案件/A社" in got
          and "私用/買い物" in got, got)
    check("ページ数が出る", "2 ページ" in got, got)
    check("鍵のかかったセクションは印が付く", "（鍵）" in got, got)
    check("ゴミ箱は並べない", "OneNote_RecycleBin" not in got, got)
    check("--list は書かない", not (tmp / "list").exists())

    buf = io.StringIO()
    with _c.redirect_stdout(buf):
        run(FakeOneNote(hierarchy(), pages_for()), tmp / "list2", "--list", "--only", "議事録")
    got = buf.getvalue()
    check("外れるものに × が付く", "× 仕事/案件/A社" in got and "× 私用/買い物" in got, got)
    check("拾うものには × が付かない",
          any(l.startswith("  仕事/議事録") for l in got.splitlines()), got)


class EarlyBound(FakeOneNote):
    """`EnsureModule` / `EnsureDispatch` で束ねたときの形 ── `[out]` は戻り値。

    **わざと厳しくしてある。** 余った引数を黙って受ける偽物にすると、
    「早い形を試さない」ように壊しても遅い形が通ってしまい、検査が黙る。
    """

    def GetHierarchy(self, start, scope):
        return self.hier

    def GetPageContent(self, pid, info):
        if pid in self.fail:
            raise RuntimeError("COM がしゃっくりした")
        self.fetched.append(pid)
        return self.pages[pid]


class LateBound(FakeOneNote):
    """素の `Dispatch` の形 ── `[out]` の置き場所を渡す。**位置はメソッドで違う。**"""

    def GetHierarchy(self, start, scope, out):        # [out] は 3 番目
        if out != "":
            raise TypeError("[out] の位置が違う")
        return self.hier

    def GetPageContent(self, pid, out, info):        # [out] は **2 番目**
        if out != "":
            raise TypeError("[out] の位置が違う")
        if pid in self.fail:
            raise RuntimeError("COM がしゃっくりした")
        self.fetched.append(pid)
        return self.pages[pid]


class NeedsSchema(FakeOneNote):
    """**schema を省くと断る相手。**

    Microsoft の資料は「版を明示せよ、空で渡すな」と書いている ── 空で渡すと
    OneNote が「いまの版」を探しにいく。その版が登録されていなければ
    `ライブラリは登録されていません`。会社の Windows で出た形の候補。
    """

    # **`int なら何でも` では甘い。** `[out]` の置き場所に info（整数）が
    # 入った形まで通ってしまい、schema を落としても走査が黙る。
    def GetHierarchy(self, start, scope, xsSchema=None):
        if xsSchema != o2m.XS_2013:
            raise RuntimeError("(-2147319779, 'ライブラリは登録されていません。')")
        self.hier_calls += 1
        return self.hier

    def GetPageContent(self, pid, info=None, xsSchema=None):
        if xsSchema != o2m.XS_2013:
            raise RuntimeError("(-2147319779, 'ライブラリは登録されていません。')")
        self.fetched.append(pid)
        return self.pages[pid]


class Sneaky(FakeOneNote):
    """間違った渡し方でも**例外にならず、XML でない何か**を返す性悪。

    これが居るから、例外の有無だけで見分けてはいけない。
    """

    def GetHierarchy(self, start, scope, out=None):
        return self.hier if out is not None else ""

    def GetPageContent(self, pid, out=None, info=None):
        if out is None or out != "":
            return 0          # 早い形では数を返してくる
        self.fetched.append(pid)
        return self.pages[pid]


def t_binding(tmp):
    print("束ね方（早い／遅い）──")
    want = ["仕事/案件/A社/見積.md", "仕事/議事録/9月の定例.md",
            "仕事/議事録/9月の定例/補足.md", "私用/買い物/週末.md"]
    for name, cls in (("早い束ね", EarlyBound), ("遅い束ね", LateBound), ("性悪", Sneaky),
                      ("schema を要る相手", NeedsSchema)):
        out = tmp / f"bind-{cls.__name__}"
        # **落ちたら NG。** 例外のまま抜けると走査ごと止まり、
        # 「鳴らなかった」と「検査が無い」が同じ顔になる。
        try:
            run(cls(hierarchy(), pages_for()), out)
            got = _mds(out)
        except Exception as e:  # noqa
            got = f"落ちた: {type(e).__name__}: {e}"
        check(f"{name}でも同じものが出る", got == want, f"{got}")

    class Broken(FakeOneNote):
        def GetHierarchy(self, *a):
            raise RuntimeError("どの形でも駄目")

    try:
        o2m.get_hierarchy(Broken(hierarchy(), pages_for()))
        check("どの形でも駄目なら黙らない", False, "通ってしまった")
    except Exception as e:  # noqa
        check("どの形でも駄目なら黙らない", "どの形でも駄目" in str(e), str(e))


# ---------------------------------------------------------------------------
# connect_onenote ── win32com ごと偽って、束ね方の梯子を確かめる
# ---------------------------------------------------------------------------
import types  # noqa: E402


class NoTypeInfo:
    """繋がってはいるが、メソッドの名前が引けない ── 会社の Windows で出た姿。"""

    def __getattr__(self, name):
        raise AttributeError(f"OneNote.Application.{name}")


def _fake_win32com(ensure_module=None, ensure_dispatch=None, dispatch=None, gen_path=None,
                   load_typelib=None, generate=None):
    """`import win32com.client` と `from win32com.client import gencache` を通す。"""
    pkg = types.ModuleType("win32com")
    pkg.__gen_path__ = gen_path or ""
    # **本物と同じ形にする。** pywin32 は `import win32com` の時点で
    # `gen_py` を作り、`__path__` をそのときの `__gen_path__` で焼き付ける。
    # ここを省くと「書く先だけ動かした」誤りが走査を素通りする。
    gen_py = types.ModuleType("win32com.gen_py")
    gen_py.__path__ = [gen_path or ""]
    pkg.gen_py = gen_py
    client = types.ModuleType("win32com.client")
    gencache = types.ModuleType("win32com.client.gencache")

    def boom(name):
        def f(*a, **k):
            raise RuntimeError(f"{name} は使えない")
        return f

    gencache.EnsureModule = ensure_module or boom("EnsureModule")
    gencache.EnsureDispatch = ensure_dispatch or boom("EnsureDispatch")
    client.Dispatch = dispatch or boom("Dispatch")
    # **ファイルから型ライブラリを読む段**（依頼 587）。本物と同じ形で
    # 置かないと、その段は `ImportError` で素通りして、何を試したのか
    # 分からないまま次へ落ちる ── 偽物が甘いと検査は嘘をつく。
    makepy = types.ModuleType("win32com.client.makepy")
    makepy.GenerateFromTypeLibSpec = generate or (lambda *a, **k: None)
    pythoncom = types.ModuleType("pythoncom")
    pythoncom.LoadTypeLib = load_typelib or boom("LoadTypeLib")
    client.makepy = makepy
    client.gencache = gencache
    pkg.client = client
    return {"win32com": pkg, "win32com.gen_py": gen_py,
            "win32com.client": client, "win32com.client.gencache": gencache,
            "win32com.client.makepy": makepy, "pythoncom": pythoncom}


def _connect_with(**mods):
    saved = {k: sys.modules.get(k) for k in
             ("win32com", "win32com.gen_py", "win32com.client",
              "win32com.client.gencache", "win32com.client.makepy", "pythoncom")}
    sys.modules.update(_fake_win32com(**mods))
    try:
        return ORIG_CONNECT()
    finally:
        for k, v in saved.items():
            if v is None:
                sys.modules.pop(k, None)
            else:
                sys.modules[k] = v


def t_gen_py(tmp):
    print("makepy の作り置き先 ──")
    ok_dir = tmp / "gen-ok"
    ok_dir.mkdir(parents=True, exist_ok=True)
    no_dir = tmp / "gen-no"
    no_dir.mkdir(parents=True, exist_ok=True)
    no_dir.chmod(0o500)          # 読めるが書けない

    check("書ける場所は書けると言う", o2m._can_write(str(ok_dir)))
    check("書けない場所は書けないと言う", not o2m._can_write(str(no_dir)))
    check("試し書きの跡を残さない", list(ok_dir.iterdir()) == [], f"{list(ok_dir.iterdir())}")

    good = FakeOneNote(hierarchy(), pages_for())

    # 既定が書けるなら、触らない
    seen = []
    _connect_with(gen_path=str(ok_dir),
                  ensure_module=lambda *a: seen.append(sys.modules["win32com"].__gen_path__),
                  dispatch=lambda *a: good)
    check("書けるならそのまま使う", seen == [str(ok_dir)], f"{seen}")

    # 既定が書けないなら、書ける場所へ逃がす
    seen = []
    _connect_with(gen_path=str(no_dir),
                  ensure_module=lambda *a: seen.append(sys.modules["win32com"].__gen_path__),
                  dispatch=lambda *a: good)
    check("書けないなら逃がす", seen and seen[0] != str(no_dir), f"{seen}")
    check("逃がし先は書ける場所", seen and o2m._can_write(seen[0]), f"{seen}")
    # **順番が肝。** 本物の gencache は `win32com.client` を読んだ時点で道を
    # 見るので、そのあとで逃がしても遅い。**それは mac では起こせない**
    # （偽の client は読んでも何もしないから、実行の上では順番が見えない）ので、
    # **並び順そのものを見る。** 粗いが、取り違えは実際にこの形で起きる。
    body = (ROOT / "onenote2md.py").read_text(encoding="utf-8")
    body = body[body.index("def connect_onenote("):body.index("def _xml_call(")]
    check("逃がしてから win32com.client を読む（並び順）",
          body.index("_gen_py_somewhere_writable()") < body.index("import win32com.client"),
          "client を読んだあとで逃がしている")

    # **書く先と読む先が揃っていること。** ここが揃わないと、makepy は書けた
    # のに `No module named 'win32com.gen_py.…'` になる ── 会社の Windows で
    # 実際に出た姿。偽の EnsureModule は、本物と同じくそこで転ぶ。
    done = []

    def strict_ensure(*a):
        w32 = sys.modules["win32com"]
        gen = sys.modules["win32com.gen_py"]
        if gen.__path__[0] != w32.__gen_path__:
            raise ModuleNotFoundError(
                "No module named 'win32com.gen_py.0EA692EE-BB50-4E3C-AEF0-356D91732725x0x1x1'")
        done.append(1)

    got, _ = _connect_with(gen_path=str(no_dir), ensure_module=strict_ensure,
                           dispatch=lambda *a: good)
    # **`got is good` だけでは足りない。** 一段目が転んでも三段目が同じものを
    # 返すので、梯子を落ちたことが見えない。makepy が通ったことまで見る。
    check("書く先と読む先を揃えて逃がす", got is good and done == [1], f"{done}")

    # 書いた直後のものが見えないことがある ── 一度だけやり直す
    tries = []

    def flaky(guid, lcid, major, minor):
        tries.append((major, minor))
        # 1.1 の**二度目**だけ通す。やり直さない版では、一度目で諦めて
        # 次の段（1.0）へ落ちる ── 同じ `good` が返るので、
        # **何が返ったかだけ見ていると見分けられない。**
        if (major, minor) != (1, 1) or len(tries) == 1:
            raise ImportError("まだ見えない")

    got, _ = _connect_with(gen_path=str(ok_dir), ensure_module=flaky,
                           dispatch=lambda *a: good)
    check("見つからなければ一度やり直す",
          got is good and tries == [(1, 1), (1, 1)], f"{tries}")

    no_dir.chmod(0o700)          # 片付けられるように戻す


def _gen_module(with_decoy=True):
    """makepy が作る形の偽モジュール ── `GetHierarchy` を持つ型が一つ入っている。"""
    mod = types.ModuleType("win32com.gen_py.fake")

    class CApplication2:                       # 本命（名前はあてにならない）
        def __init__(self, ole):
            self.ole = ole

        def GetHierarchy(self, *a):
            return "<xml/>"

    mod.CApplication2 = CApplication2

    class AFussy:
        """型の上には `GetHierarchy` が居るのに、実物には居ない皮。

        `dir()` は名前順なので、**本命より先に当たる。** かぶせたあとに
        実物を見ないと、こちらを掴んで「かぶせた」と言ってしまう。
        """

        def __init__(self, ole):
            self.ole = ole

        @property
        def GetHierarchy(self):
            raise AttributeError("この皮は合わない")

    mod.AFussy = AFussy
    if with_decoy:
        class IApplication:                    # それらしい名前だが、何も持たない
            def __init__(self, ole):
                self.ole = ole

        mod.IApplication = IApplication
        mod.CLSIDToClassMap = {}               # 型ではないものも混ざっている
    return mod


def t_wrap(tmp):
    print("makepy の皮をかぶせる ──")
    blind = NoTypeInfo()
    mod = _gen_module()

    w = o2m._wrap_with_generated(mod, blind)
    check("遅い束ねに皮をかぶせられる", w is not None and hasattr(w, "GetHierarchy"),
          f"{type(w).__name__ if w else None}")
    # **名前で選ばない。** `IApplication` という名前は、それらしいだけで中身が無い。
    check("選ぶのは名前ではなく「GetHierarchy を持つこと」",
          w is not None and type(w).__name__ == "CApplication2", f"{type(w).__name__ if w else None}")

    # **かぶせた皮が実際に使えることまで見る。** 型の上に名前があるだけでは足りない。
    check("かぶせた皮が実物として使えるところまで見る",
          w is not None and type(w).__name__ != "AFussy", f"{type(w).__name__ if w else None}")

    check("かぶせる型が無ければ、素直に諦める",
          o2m._wrap_with_generated(_gen_module_empty(), blind) is None)
    check("モジュールが無ければ諦める", o2m._wrap_with_generated(None, blind) is None)

    # 梯子の一段目が、遅い束ねを返されても最後まで面倒を見ること
    # **落ちたら NG。** 包めないと梯子を落ちきって SystemExit になる ──
    # そのまま抜けると走査ごと止まり、「検査が無い」のと同じ顔になる。
    try:
        got, _ = _connect_with(gen_path=str(tmp / "wrap-gen"),
                               ensure_module=lambda *a: mod,
                               dispatch=lambda *a: blind)
        ok = hasattr(got, "GetHierarchy") and got is not blind
        why = type(got).__name__
    except BaseException as e:  # noqa
        ok, why = False, f"落ちた: {type(e).__name__}"
    check("一段目が遅い束ねを掴んでも、包んで返す", ok, why)


def _gen_module_empty():
    mod = types.ModuleType("win32com.gen_py.empty")

    class Nothing:
        pass

    mod.Nothing = Nothing
    return mod


def t_probe(tmp):
    print("--probe ──")
    import io
    import contextlib as _c

    out = tmp / "probe-out"
    buf = io.StringIO()
    code = None
    try:
        with _c.redirect_stdout(buf):
            code = o2m.run(_parse(["--out", str(out), "--probe"]), out)
    except BaseException as e:  # noqa
        # **壊れた端末で使う道具が落ちては意味がない。** 落ちたことを
        # 検査の答えにする ── 例外のまま抜けると走査ごと止まる。
        buf.write(f"\n（落ちた: {type(e).__name__}: {e}）")
    got = buf.getvalue()

    check("0 を返す", code == 0, f"{code}")
    check("何も書かない", not out.exists())
    for head in ("== python ==", "== pywin32 ==", "== 型ライブラリの登録 ==", "== OneNote =="):
        check(f"{head} が出る", head in got)
    # **壊れた端末で使う道具なので、途中が転んでも最後まで出ること。**
    # mac には win32com が無いので、ここは全行が転ぶ ── それでも最後まで並ぶ。
    check("一つ転んでも最後まで出る", "1.0 で包んで、呼んでみる" in got, got[-120:])
    check("転んだ行は印が付く", "✗" in got, got[:200])


def t_verify(tmp):
    print("呼べるまで信じない ──")
    good = FakeOneNote(hierarchy(), pages_for())

    class Registered:
        """`GetHierarchy` は見えるが、呼ぶと COM が断る。

        会社の Windows で出たのがこれ ── 皮はかぶさっているのに
        `ライブラリは登録されていません` になる。**見えることは、使えることでは
        ない。**
        """

        def GetHierarchy(self, *a):
            raise RuntimeError("(-2147319779, 'ライブラリは登録されていません。')")

    broken = Registered()
    seen = []

    def ensure(guid, lcid, major, minor):
        seen.append((major, minor))

    def dispatch(*a):
        return broken if seen and seen[-1] == (1, 1) else good

    try:
        got, xml = _connect_with(ensure_module=ensure, dispatch=dispatch)
    except BaseException as e:  # noqa
        got, xml = None, f"落ちた: {type(e).__name__}"
    check("呼んで落ちる相手は採らない", got is good, f"{type(got).__name__}")
    check("版が二つあるなら、もう一方を試す", seen == [(1, 1), (1, 0)], f"{seen}")
    check("繋ぐときの答えをそのまま返す",
          isinstance(xml, str) and xml.lstrip().startswith("<"), f"{str(xml)[:40]}")

    # 全部が「見えるのに呼べない」なら、黙って進まず止まる
    try:
        _connect_with(ensure_module=lambda *a: None, dispatch=lambda *a: Registered(),
                      ensure_dispatch=lambda *a: Registered())
        check("全部呼べないなら止まる", False, "通ってしまった")
    except SystemExit as e:
        check("全部呼べないなら止まる", "呼ぶと落ちる" in str(e), str(e)[:80])

    # **「見えない」と「呼べない」は別の話。** 両方を見分けて言う ── この二つを
    # 混ぜた報せでは、現場で何が起きているか読めない（実際、この session では
    # その区別だけを頼りに三度進んだ）。
    try:
        _connect_with(ensure_module=lambda *a: None, dispatch=lambda *a: NoTypeInfo(),
                      ensure_dispatch=lambda *a: NoTypeInfo())
        check("「見えない」と「呼べない」を言い分ける", False, "通ってしまった")
    except SystemExit as e:
        check("「見えない」と「呼べない」を言い分ける",
              "GetHierarchy が見えない" in str(e) and "呼ぶと落ちる" not in str(e), str(e)[:90])

    # 繋ぐときに訊いた答えを捨てない（ページ数の多い棚で二度歩かない）
    counted = FakeOneNote(hierarchy(), pages_for())
    run(counted, tmp / "twice")
    check("階層を二度は取りに行かない", counted.hier_calls == 1, f"{counted.hier_calls}")


def t_arch(tmp):
    print("bit の食い違いを、こちらから言う ──")
    v = o2m._arch_verdict({"Win32"}, bits=64)
    check("64 bit なのに win32 しか無ければ言う",
          v and "win64 の登録が無い" in v[1], f"{v}")
    check("何が原因かまで言う", v and "ライブラリは登録されていません" in v[2], f"{v}")
    check("噛み合っていれば黙る", o2m._arch_verdict({"Win64"}, bits=64) is None)
    check("32 bit 側でも見る", o2m._arch_verdict({"win64"}, bits=32) is not None)
    check("32 bit で win32 があれば黙る", o2m._arch_verdict({"win32"}, bits=32) is None)
    check("大文字小文字は問わない", o2m._arch_verdict({"WIN64"}, bits=64) is None)
    check("何も無ければ決めつけない", o2m._arch_verdict(set(), bits=64) is None)


def t_resource_index(tmp):
    print("型ライブラリの道 ──")
    f = o2m._strip_resource_index
    check("exe の中の番号を落とす",
          f(r"C:\x\ONENOTE.EXE\3") == r"C:\x\ONENOTE.EXE", f(r"C:\x\ONENOTE.EXE\3"))
    check("番号が無ければそのまま",
          f(r"C:\x\stdole2.tlb") == r"C:\x\stdole2.tlb")
    check("斜めの区切りでも落とす", f("/usr/x/lib.tlb/2") == "/usr/x/lib.tlb")
    check("途中の数字は落とさない",
          f(r"C:\Office16\ONENOTE.EXE") == r"C:\Office16\ONENOTE.EXE")
    check("空でも落ちない", f("") == "" and f(None) == "")


def t_arch_hint(tmp):
    print("繋げないとき、答えのほうを言う ──")
    check("Windows でなければ黙る（分からないことは言わない）",
          o2m._registered_arches() == set())

    # 繋げない回の報せに、bit の見立てが載ること
    saved = o2m._registered_arches
    o2m._registered_arches = lambda: {"win32"}
    try:
        _connect_with()
        check("繋げないとき bit の見立ても出す", False, "通ってしまった")
    except SystemExit as e:
        check("繋げないとき bit の見立ても出す", "win64 の登録が無い" in str(e), str(e)[-120:])

    o2m._registered_arches = lambda: {"win64"}
    try:
        _connect_with()
        check("噛み合っているときは、余計なことを言わない", False, "通ってしまった")
    except SystemExit as e:
        check("噛み合っているときは、余計なことを言わない",
              "登録が無い" not in str(e), str(e)[-120:])
    finally:
        o2m._registered_arches = saved


def t_connect(tmp):
    print("OneNote への繋ぎ方 ──")
    good = FakeOneNote(hierarchy(), pages_for())

    named = []
    got, _ = _connect_with(ensure_module=lambda *a: named.append(a), dispatch=lambda *a: good)
    check("版を名指しできれば、それで繋ぐ", got is good)
    # **ここが今回の肝。** OneNote の型ライブラリの登録には中身のない `1.0` の
    # 枝が混ざっていて、任せると pywin32 がそれを掴んで名前を引けなくなる。
    # 版（1.1）を名指しすれば跨げる。
    check("型ライブラリを GUID と版で名指しする",
          named and named[0] == (o2m.ONENOTE_TYPELIB, 0, 1, 1), f"{named}")

    got, _ = _connect_with(ensure_dispatch=lambda *a: good)
    check("駄目なら gencache に任せる", got is good)

    got, _ = _connect_with(dispatch=lambda *a: good)
    check("それも駄目なら素の Dispatch", got is good)

    # **繋がったことと、話が通じることは別。** 一度目は名前の引けない相手を
    # 返し、三度目に本物を返す ── 会社の Windows で出たのがこの姿だった。
    blind = NoTypeInfo()
    seen = []

    def dispatch(*a):
        seen.append(1)
        return blind if len(seen) == 1 else good

    got, _ = _connect_with(ensure_module=lambda *a: None, dispatch=dispatch)
    check("GetHierarchy が見えない相手は採らない", got is good, f"{type(got).__name__}")

    # ── ファイルから型ライブラリを読む（依頼 587）──
    #
    # 登録は白く、指す先のファイルも在るのに読めない端末がある。
    # `LoadRegTypeLib` が転ぶのと、**その資源に型ライブラリが入っていない**
    # のは別のことなので、道を名指しして読んでみる。
    keep_vals, keep_exists = o2m._typelib_values, o2m.os.path.exists
    A = "C:" + chr(92) + "在る.exe" + chr(92) + "3"
    B = "C:" + chr(92) + "無い.exe" + chr(92) + "3"
    try:
        o2m._typelib_values = lambda: [("1.0", "win32", B, ""), ("1.1", "win32", A, "")]
        o2m.os.path.exists = lambda p: "無い" not in p
        read = []
        got, _ = _connect_with(dispatch=lambda *a: good,
                               load_typelib=lambda path: read.append(path))
        check("ファイルから読めれば、それで繋ぐ", got is good, f"{type(got).__name__}")
        check("在る道だけ読みにいく", read == [A], read)

        # 読めなければ、**道ごとの言い分**を持ち帰る ── そこで初めて
        # 「資源に型ライブラリが入っていない」と分かる。
        def cant(path):
            raise RuntimeError("資源が無い")

        try:
            _connect_with(load_typelib=cant)
            check("全部駄目なら止まる（ファイルの段）", False, "止まらなかった")
        except SystemExit as e:
            check("ファイルの段も、梯子に並ぶ", "ファイルから型ライブラリを読む" in str(e.code), e.code)
            check("道ごとの言い分を持ち帰る", "在る.exe" in str(e.code) and "資源が無い" in str(e.code),
                  e.code)
    finally:
        o2m._typelib_values, o2m.os.path.exists = keep_vals, keep_exists

    try:
        _connect_with()
        check("全部駄目なら、わけを並べて止まる", False, "通ってしまった")
    except SystemExit as e:
        msg = str(e)
        check("全部駄目なら、わけを並べて止まる",
              "EnsureModule は使えない" in msg and "Dispatch は使えない" in msg, msg[:80])
        check("止まるとき、心当たりを出す",
              "管理者" in msg and "gen_py" in msg and "ストア版" in msg, msg[-120:])



# ---------------------------------------------------------------------------
# 中身の変換 ── **ここが道具の目的そのもの。**
#
# 階層・差分・`--prune`・COM の繋ぎ方には検査が並んでいたのに、出てくる
# Markdown を見る検査は素直な一枚に七つだけだった。写したものが読めるかを
# 見ないのでは、何を確かめているのか分からない。
# ---------------------------------------------------------------------------
def _convert(body, styles=None, page_attr="", with_images=False):
    """`<one:Outline>` の中身だけ渡して、出てくる Markdown を返す。"""
    styles = styles if styles is not None else (
        '<one:QuickStyleDef index="0" name="h1"/>'
        '<one:QuickStyleDef index="1" name="p"/>'
        '<one:QuickStyleDef index="2" name="code"/>'
    )
    import xml.etree.ElementTree as ET
    xml = (f'<one:Page {NBS} {page_attr}>{styles}'
           f'<one:Outline><one:Position x="0" y="0"/><one:OEChildren>{body}'
           f'</one:OEChildren></one:Outline></one:Page>')
    import tempfile as _tf
    at = Path(_tf.mkdtemp()) if with_images else Path("/tmp/なし")
    conv = o2m.PageConverter(ET.fromstring(xml), at, "p", "attachments", with_images)
    return conv.convert()


def _cell(t):
    return (f'<one:Cell><one:OEChildren><one:OE><one:T><![CDATA[{t}]]></one:T>'
            f'</one:OE></one:OEChildren></one:Cell>')


def t_amber_shape(tmp):
    print("ambər が読める形 ──")
    # **正本は Rust のほう**（`crates/amber-core/src/note.rs` の `file_stem`）。
    # ここは写しなので、**同じ答えになることを確かめる** ── ずれると、ambər が
    # ノートを改名・移動した日に画像が付いてこない。
    rust = (ROOT.parent / "crates" / "amber-core" / "src" / "note.rs").read_text(encoding="utf-8")
    check("正本の規則がまだそこにある（写しの拠りどころ）",
          "pub fn file_stem" in rust and "out.chars().count() >= 60" in rust)
    for title, want in [
        ("ふつう", "ふつう"),
        ("斜/線", "斜-線"),
        ("?? notes", "notes"),            # 先頭には `-` を置かない
        ("a//b", "a-b"),                  # 続いた一続きは `-` 一つ
        ("  前後  ", "前後"),
        ("末尾.", "末尾"),
        ("CON", "_CON"),                  # 予約名
        ("con.md", "_con.md"),
        ("あ" * 80, "あ" * 60),           # 60 字まで
    ]:
        got = o2m.amber_stem(title)
        check(f"幹 {title[:12]} を {want[:12]} にする", got == want, got[:20])

    # **画像の名前は `<幹>-NNN.<ext>`。** ambər は「幹 + ハイフン」で見分ける
    # （`note::attach` が `format!("{}-{stamp}.{ext}")` で書いている）。
    check("正本が幹とハイフンで名づけている",
          '-{stamp}.{ext}' in rust and 'attachments/{name}' in rust)
    got = _convert('<one:OE quickStyleIndex="1"><one:Image format="png">'
                   f'<one:Data>{PNG_B64}</one:Data></one:Image></one:OE>', with_images=True)
    check("画像は <幹>-NNN.<ext>", "](attachments/p-001.png)" in got, got)
    # ページの幹も 60 字 ── ここが 120 のままだと、長い題のページだけ幹がずれる。
    long = o2m.sanitize("ぺ" * 80)
    check("ページの幹も 60 字", len(long) == 60 and o2m.amber_stem(long) == long, len(long))


def t_table(tmp):
    print("表 ──")
    rows = f'<one:Row>{_cell("りんご")}{_cell("120円")}</one:Row><one:Row>{_cell("みかん")}{_cell("80円")}</one:Row>'

    got = _convert(f'<one:OE><one:Table hasHeaderRow="true">{rows}</one:Table></one:OE>')
    check("見出しの行があれば、1行目が見出し", "| りんご | 120円 |\n| --- | --- |\n| みかん | 80円 |" in got, got)

    # **無いのに 1 行目を見出しにすると、そのデータが一行、表から消える。**
    got = _convert(f'<one:OE><one:Table hasHeaderRow="false">{rows}</one:Table></one:OE>')
    check("見出しの行が無ければ、データは一行も減らない",
          "| りんご | 120円 |" in got and "| みかん | 80円 |" in got, got)
    check("見出しの行が無ければ、空の見出しを置く", "|  |  |\n| --- | --- |" in got, got)
    # 旗が無いのは「無い」（OneNote の既定）。
    got = _convert(f'<one:OE><one:Table>{rows}</one:Table></one:OE>')
    check("旗が無いときも、データは一行も減らない", "| りんご | 120円 |" in got, got)

    two = ('<one:Cell><one:OEChildren>'
           '<one:OE><one:T><![CDATA[一行目]]></one:T></one:OE>'
           '<one:OE><one:T><![CDATA[二行目]]></one:T></one:OE>'
           '</one:OEChildren></one:Cell>')
    got = _convert(f'<one:OE><one:Table hasHeaderRow="true"><one:Row>{_cell("見出")}{two}</one:Row>'
                   f'</one:Table></one:OE>')
    # ambər は本文の札をぜんぶ字にするので、`<br>` は `<br>` という字で出る。
    check("升の中の改行に、札を残さない", "<br>" not in got, got)
    check("升の中の改行は、繋いで残す", "一行目 二行目" in got, got)


def t_inline(tmp):
    print("字の中の印 ──")
    b = "font-weight:bold"
    got = _convert(f"<one:OE quickStyleIndex=\"2\"><one:T><![CDATA[if <span style='{b}'>x</span> > 0:]]>"
                   f"</one:T></one:OE>")
    # **枠の中で `**` は印として読まれない。** 写したコードに無い字が混ざる。
    check("コードの枠に、印を生やさない", "if x > 0:" in got and "**" not in got, got)

    got = _convert('<one:OE quickStyleIndex="1"><one:T><![CDATA['
                   '<a href="https://ex.com/a b.docx">資料</a>]]></one:T></one:OE>')
    check("リンクの URL が空白で切れない", "[資料](https://ex.com/a b.docx)" in got, got)

    got = _convert('<one:OE quickStyleIndex="1"><one:T><![CDATA['
                   '<a href=https://ex.com/x>括り無し</a>]]></one:T></one:OE>')
    check("括りの無い href も読む", "[括り無し](https://ex.com/x)" in got, got)

    got = _convert('<one:OE quickStyleIndex="1"><one:T><![CDATA['
                   '<a href="https://ex.com/?a=1&amp;b=2">と</a>]]></one:T></one:OE>')
    check("URL の実体参照を戻す", "(https://ex.com/?a=1&b=2)" in got, got)

    # **インクの言い方は一つ。** 二つあると、探す人は片方しか見つけられない
    # （docs が知っているのも片方だけだった）。
    ink = _convert('<one:OE quickStyleIndex="1"><one:InkParagraph/></one:OE>')
    import xml.etree.ElementTree as ET
    top = o2m.PageConverter(
        ET.fromstring(f'<one:Page {NBS}><one:InkDrawing><one:Position x="0" y="0"/>'
                      f'</one:InkDrawing></one:Page>'),
        Path("/tmp/なし"), "p", "attachments", False).convert()
    check("インクの言い方は、どこでも同じ",
          "[インク: 変換対象外]" in ink and "[インク: 変換対象外]" in top, ink + " / " + top)


def t_created(tmp):
    print("前書きの日付 ──")
    import os
    import time
    keep = os.environ.get("TZ")
    try:
        os.environ["TZ"] = "Asia/Tokyo"
        time.tzset()
        # UTC で 2026-09-01T23:00Z ＝ 東京では 9月2日の朝 8 時。
        got = o2m.local_date("2026-09-01T23:00:00.000Z")
        check("UTC を、この機械の日付に直す（東京）", got == "2026-09-02", got)
        os.environ["TZ"] = "UTC"
        time.tzset()
        check("UTC の機械では、そのまま", o2m.local_date("2026-09-01T23:00:00.000Z") == "2026-09-01")
        # 読めない形でも黙って落ちない ── 前の版と同じ「頭の 10 字」に戻る。
        # **落ちるのも「黙る」の一種。** 走査ごと止まると、画面には
        # 「NG が無い」としか出ない（依頼 569 で踏んだ形）。受け止めて NG にする。
        try:
            got = o2m.local_date("2026-09-01 01:00")
        except Exception as e:  # noqa
            got = f"落ちた: {type(e).__name__}"
        check("読めない形は、頭の 10 字", got == "2026-09-01", got)
        # **前書きを通しても効くか。** `local_date` だけ見ていると、
        # `frontmatter` がそれを呼ぶのをやめても黙る。
        os.environ["TZ"] = "Asia/Tokyo"
        time.tzset()
        fm = o2m.frontmatter("題", ["仕事"], {"dateTime": "2026-09-01T23:00:00.000Z",
                                             "lastModifiedTime": "", "ID": "{P}"})
        check("前書きの created も、この機械の日付", "created: 2026-09-02" in fm, fm)
        check("空なら空", o2m.local_date("") == "" and o2m.local_date(None) == "")
    finally:
        if keep is None:
            os.environ.pop("TZ", None)
        else:
            os.environ["TZ"] = keep
        time.tzset()


def t_log(tmp):
    print("落ちたわけを、記録に残す ──")
    import subprocess
    at = tmp / "log"
    at.mkdir(exist_ok=True)
    logfile = at / "t.log"
    r = subprocess.run([sys.executable, str(ROOT / "onenote2md.py"),
                        "--out", str(at / "o"), "--log", str(logfile)],
                       capture_output=True, text=True)
    body = logfile.read_text(encoding="utf-8") if logfile.exists() else ""
    # **画面には誰もいない。** `pythonw.exe` に stderr は無いので、
    # `sys.exit("わけ")` の字はどこにも出ないまま終わる。
    check("落ちた回は 0 を返さない", r.returncode != 0, r.returncode)
    check("落ちたわけが、記録に残る", "pywin32" in body, body)
    check("記録に ERROR として残る", "ERROR" in body, body)


def t_cp932(tmp):
    print("画面の字を、ファイルに落とせる ──")
    import subprocess
    env = dict(os.environ, PYTHONIOENCODING="cp932")
    r = subprocess.run([sys.executable, str(ROOT / "onenote2md.py"), "--probe"],
                       capture_output=True, text=True, env=env, errors="replace")
    # `✗` も `ambər` の `ə` も cp932 に無い。失敗した行を書こうとして止まる ──
    # `--probe` が要るのは、まさに失敗する端末の上。
    check("cp932 に向けても、最後まで出る", "包んで、呼んでみる" in r.stdout, r.stdout[-200:])
    check("cp932 に向けても、落ちない", r.returncode == 0, r.returncode)
    r = subprocess.run([sys.executable, str(ROOT / "onenote2md.py"), "--probe"],
                       capture_output=True, text=True, env=env, errors="replace")
    check("--probe に --out は要らない", "== python ==" in r.stdout, r.stdout[:120])


def t_check(tmp):
    print("一画面で終わる診断（--check）──")
    import io, contextlib
    EXE = "C:" + chr(92) + "Office16" + chr(92) + "ONENOTE.EXE" + chr(92) + "3"
    keep = (o2m._office_platform, o2m._typelib_tree, o2m._local_server,
            o2m._store_onenote, o2m._typelib_path, o2m.connect_onenote,
            o2m._typelib_values, o2m._onenote_exe, o2m.os.path.exists)

    def blew(msg="駄目", troubles=()):
        def f():
            raise o2m.CannotConnect(msg, troubles)
        return f

    def say(**kw):
        o2m._office_platform = kw.get("office", lambda: "x64")
        o2m._typelib_tree = kw.get("tree", lambda: {"1.1": {"win32"}})
        o2m._local_server = kw.get("server", lambda: (EXE, True))
        o2m._store_onenote = kw.get("store", lambda: False)
        o2m._typelib_path = kw.get("tl", lambda want: ("1.1", "0", EXE))
        o2m._typelib_values = kw.get("vals", lambda: [
            ("1.0", "win32", EXE, ""), ("1.0", "win64", EXE, ""),
            ("1.1", "win32", EXE, ""), ("1.1", "win64", EXE, "")])
        o2m._onenote_exe = kw.get("exe", lambda: None)
        o2m.os.path.exists = kw.get("exists", lambda p: True)
        o2m.connect_onenote = kw.get("connect", blew())
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            code = o2m.check()
        return code, buf.getvalue()

    try:
        code, out = say()
        # **手で打ち直して渡せる長さ。** `--probe` は 29 行出る ── 現場に
        # 立っている人に、それを写させるのは注文としておかしい。
        check("繋がらないときも、十数行で収まる", len(out.strip().splitlines()) <= 16,
              len(out.strip().splitlines()))
        check("落ちた回は 0 を返さない", code != 0, code)
        check("噛み合っていない枝を名指しする", "win64 の枝が無い" in out, out)
        # **道は自分で調べさせない。** 繋がらない端末の前に立っている人に
        # 「docs を見て、probe の値をそこから写して」は通らない。
        check("足す一行を、道ごと出す", 'reg add "HKCU' in out and EXE in out, out)
        check("戻す一行も出す", "reg delete" in out, out)

        # **束ねて見ない。** 1.0 に win64、1.1 に win32 ── 束ねると
        # 「両方ある」に見えるが、こちらが読むのは 1.1 なので落ちる。
        _, out = say(tree=lambda: {"1.0": {"win64"}, "1.1": {"win32"}})
        check("枝は版ごとに出す", "1.0=win64" in out and "1.1=win32" in out, out)
        check("読みにいく版で判じる", "版 1.1 に win64 の枝が無い" in out, out)
        check("ほかの版に有っても、助けにならないと言う",
              "助けにならない" in out and "1.0" in out, out)
        # 読む版に枝が有れば、枝の話はしない。
        _, out = say(tree=lambda: {"1.0": {"win32"}, "1.1": {"win32", "win64"}})
        check("読む版に枝が有れば、枝の話はしない", "枝が無い" not in out, out)

        # **呼んだときの答えを捨てない。** 登録が白なら、残る手がかりはそれだけ。
        _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                     connect=blew(troubles=["  型ライブラリ 1.1 を名指し: 呼ぶと落ちる ──"
                                            " (-2146959355, 'サーバーの実行に失敗しました。')"]))
        check("呼んだときの答えを、そのまま出す", "呼んだときの答え" in out
              and "-2146959355" in out, out)
        check("答えの番号から、原因を名指しする", "権限のずれ" in out, out)
        _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                     connect=blew(troubles=["  素の Dispatch: (-2147312566, '読み込みエラー')"]))
        check("読めない実体は、32 bit の Python へ導く", "32 bit の Python" in out, out)
        # **読めないと言われた道を、その場で出す。** ここで `--probe` へ送ると、
        # 29 行を手で打ち直させることになる。
        check("読めない道を、その場で出す", "1.1 / win32 = " in out and "1.1 / win64 = " in out, out)
        # **版はぜんぶ出す。** 一つだけ見せると、落ちた版と違うものを見せうる。
        check("版をぜんぶ出す", "1.0 / win32 = " in out and "1.1 / win32 = " in out, out)
        # 同じ道を二つの枝が指していたら、片方は写し ── 足しても直らない形。
        check("同じ道を指していたら、写しだと言う",
              "同じ道" in out and "足しても直らない" in out, out)
        _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                     vals=lambda: [("1.1", "win32", EXE, ""), ("1.1", "win64", EXE + "64", "")],
                     connect=blew(troubles=["  素の Dispatch: (-2147312566, '読み込みエラー')"]))
        check("違う道を指していれば、写しだとは言わない", "同じ道" not in out, out)

        # **一つも無いなら、話はそこで変わる。** bit でも枝でもなく、
        # 登録が居ないものを指している。
        # **枝は版ごとに二本ずつ。** 一本ずつだと「写しの話」が元から出ず、
        # 守りを外しても何も変わらない ── 作り物が薄いと、検査は嘘をつく。
        nowhere = "C:" + chr(92) + "無い.exe"
        gone = [("1.0", "win32", nowhere, ""), ("1.0", "win64", nowhere, ""),
                ("1.1", "win32", nowhere, ""), ("1.1", "win64", nowhere, "")]
        _, out = say(tree=lambda: {"1.1": {"win32"}}, vals=lambda: gone,
                     exists=lambda p: False, exe=lambda: None,
                     connect=blew(troubles=["  素の Dispatch: (-2147312566, '読み込みエラー')"]))
        check("一つも無ければ、bit の話ではないと言う",
              "ファイルが一つも無い" in out and "bit の話でも枝の話でもない" in out, out)
        # **在るか無いかが先。** bit の話を被せると、要らない道へ人を送る。
        check("一つも無ければ、32 bit の話はしない", "32 bit の Python を使う" not in out, out)
        check("一つも無ければ、写しの話もしない", "同じ道" not in out, out)
        check("どこにも無ければ、入っていないと言う",
              "入っていない" in out, out)
        _, out = say(tree=lambda: {"1.1": {"win32"}}, vals=lambda: gone,
                     exists=lambda p: False, exe=lambda: "D:" + chr(92) + "本物.exe",
                     connect=blew(troubles=["  素の Dispatch: (-2147312566, '読み込みエラー')"]))
        check("よそに実体が在れば、そこを教えて修復へ導く",
              "本物.exe" in out and "修復" in out, out)
        # 一つでも在るなら、その話はしない。
        _, out = say(tree=lambda: {"1.1": {"win32"}},
                     vals=lambda: [("1.1", "win32", EXE, "")],
                     connect=blew(troubles=["  素の Dispatch: (-2147312566, '読み込みエラー')"]))
        check("一つでも在れば、入っていない話はしない", "入っていない" not in out, out)

        # **32 bit と 64 bit で見え方が違う。** 片方しか読まないと、
        # 在るファイルを「無い」と言う ── 実際にそれで一度、遠回りさせた。
        _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                     vals=lambda: [("1.1", "win64", EXE, "64bit の見え方"),
                                   ("1.1", "win64", "C:" + chr(92) + "x86" + chr(92) + "無い.exe",
                                    "32bit の見え方")],
                     exists=lambda p: "無い" not in p,
                     connect=blew(troubles=["  素の Dispatch: (-2147312566, '読み込みエラー')"]))
        check("見え方が違えば、どちらの話か言う",
              "[64bit の見え方]" in out and "[32bit の見え方]" in out, out)
        check("片方で在るなら、無い話はしない", "ファイルが一つも無い" not in out, out)
        # 同じなら二度見せない ── 人が読む行が倍になる。
        _, out = say(tree=lambda: {"1.1": {"win32"}},
                     vals=lambda: [("1.1", "win32", EXE, "")],
                     connect=blew(troubles=["  素の Dispatch: (-2147312566, '読み込みエラー')"]))
        check("同じなら、見え方は言わない", "見え方" not in out, out)

        # **入れたあとの一手を、こちらで言う。** 32 bit を入れた人は 64 bit の
        # 癖でもう一度同じことを叩く ── そこで同じ答えが返るのでは、入れた
        # 意味が伝わらない。
        keep_py = o2m._other_pythons
        try:
            o2m._other_pythons = lambda: [("3.9", "C:x"), ("3.9-32", "C:y")]
            _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                         connect=blew(troubles=["  素の Dispatch: (-2147312566, '読み込みエラー')"]))
            check("32 bit が居れば、そちらで叩き直す一行を出す",
                  "py -3.9-32" in out and "--check" in out, out)
            o2m._other_pythons = lambda: [("3.9", "C:x")]
            _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                         connect=blew(troubles=["  素の Dispatch: (-2147312566, '読み込みエラー')"]))
            check("居なければ、その話はしない", "叩き直す" not in out, out)
        finally:
            o2m._other_pythons = keep_py

        # ほかの番号のときは、道の話はしない（要らないことを言わない）。
        _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                     connect=blew(troubles=["  素の Dispatch: (-2146959355, '権限')"]))
        check("ほかの番号では、道を出さない", "1.1 / win32 = " not in out, out)
        # **数えてから決める。** 先に並べた順で拾うと、四つのうち三つが同じことを
        # 言っているのに、一つだけ違う答えを採る（現場で実際にそうなった）。
        _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                     connect=blew(troubles=[
                         "  1.1 を名指し: (-2146959355, 'サーバーの実行に失敗しました')",
                         "  1.0 を名指し: (-2147319779, 'ライブラリは登録されていません')",
                         "  gencache: (-2146959355, 'サーバーの実行に失敗しました')",
                         "  素の Dispatch: (-2146959355, 'サーバーの実行に失敗しました')"]))
        check("多いほうの答えを採る", "権限のずれ" in out, out)
        check("少ないほうに引きずられない", "TYPE_E_LIBNOTREGISTERED" not in out, out)

        # **見られなかったのは、無かったのとは違う。**
        _, out = say(server=lambda: (None, None), tree=lambda: {"1.1": {"win32", "win64"}},
                     connect=blew(troubles=["  素の Dispatch: 知らない何か"]))
        check("読めなければ「読めない」と言う", "（読めない）" in out, out)
        check("読めないのに「無い」と結論しない", "入れ直すか、修復する" not in out, out)
        _, out = say(server=lambda: ("C:" + chr(92) + "無い.exe", False),
                     tree=lambda: {"1.1": {"win32", "win64"}},
                     connect=blew(troubles=["  素の Dispatch: 知らない何か"]))
        check("見て無ければ、そう言う", "入れ直すか、修復する" in out, out)

        # **走っている側で、言うことが変わる。** 32 bit で叩いて同じ答えなら、
        # そこで「32 bit を使え」と言い続けるのは、一度通った道へまた送ること。
        n64 = " ".join(o2m._cant_load_next(64))
        n32 = " ".join(o2m._cant_load_next(32))
        check("64 bit なら、32 bit を試させる",
              "32 bit" in n64 and "bit の話ではない" not in n64, n64)
        check("32 bit なら、bit の話ではないと言う",
              "bit の話ではない" in n32 and "32 bit の Python を使う" not in n32, n32)
        check("32 bit なら、ファイルから読む段へ導く", "ファイルから型ライブラリを読む" in n32, n32)

        # **「繋がらない」と「繋がるのに呼べない」は、別の話。**
        # 後者なら、取り次ぎ側（`HKCR\Interface\{IID}\TypeLib`）を見る ──
        # そこが壊れた版を指していれば、束ね方を変えても同じところで落ちる。
        IID = "{9E0F}"
        keep_if = o2m._interface_registration
        try:
            o2m._interface_registration = lambda: ("IApplication", IID, "{0EA}", "1.0")
            LIB = "(-2147312566, '読み込みエラー')"
            _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                         connect=blew(troubles=[
                             "  型ライブラリ 1.1 を名指し: 呼ぶと落ちる ── " + LIB,
                             "  型ライブラリ 1.0 を名指し: " + LIB]))
            check("呼ぶ瞬間なら、取り次ぎ側を出す", "取り次ぎ側が見ている登録" in out, out)
            check("壊れた版を指していれば、そう言う", "名指しでも読めなかったほう" in out, out)
            check("取り次ぎ側を直す一行を、道ごと出す",
                  'reg add "HKCU' in out and IID in out and "/v Version /d 1.1" in out, out)
            check("戻す一行も出す", "reg delete" in out, out)
            # **確かなことが分かったら、当て推量は並べない。**
            check("当て推量を並べない",
                  "資源に型ライブラリが入っていない" not in out and "片方は写し" not in out, out)

            # 繋がってすらいないなら、取り次ぎ側の話はしない。
            _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                         connect=blew(troubles=["  型ライブラリ 1.1 を名指し: " + LIB]))
            check("呼ぶ前に落ちているなら、取り次ぎ側は見ない",
                  "取り次ぎ側" not in out, out)
            # 指している版が生きているなら、直せとは言わない。
            o2m._interface_registration = lambda: ("IApplication", IID, "{0EA}", "1.1")
            _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                         connect=blew(troubles=[
                             "  型ライブラリ 1.1 を名指し: 呼ぶと落ちる ── " + LIB,
                             "  型ライブラリ 1.0 を名指し: " + LIB]))
            check("生きている版を指しているなら、直せとは言わない", "reg add" not in out, out)
            # **そこまで白いなら、こちらの手は尽きている。** そう言う。
            check("生きている版なら、落ちているのは向こう側だと言う",
                  "落ちているのは OneNote の側" in out, out)
            # **残っている手を、安いほうから。** 作り置きを捨てるのはただ。
            def at(word):
                return out.index(word) if word in out else -1
            check("残っている手を、安いほうから並べる",
                  0 <= at("--forget") < at("クイック修復") < at("--probe"), out)
            # **網の要らないほうを先に言う。** あの端末は網に出られない（依頼 582）
            # ので、「オンライン修復」を先に勧めるのは、できないことを勧めること。
            check("網の要らない修復を先に言う",
                  out.index("クイック修復") < out.index("オンライン修復"), out)
            check("クイック修復は網が要らないと言う",
                  "クイック修復は網が要らない" in out, out)
            # 引けなければ黙る。
            o2m._interface_registration = lambda: None
            _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                         connect=blew(troubles=["  1.1 を名指し: 呼ぶと落ちる ── " + LIB]))
            check("引けなければ、取り次ぎ側の話はしない", "取り次ぎ側" not in out, out)
        finally:
            o2m._interface_registration = keep_if

        # 知らない答えなら、決めつけない。
        _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                     connect=blew(troubles=["  素の Dispatch: 知らない何か"]))
        check("知らない答えには、決めつけない", "登録は白" in out, out)

        _, out = say(tree=lambda: {"1.1": {"win32", "win64"}},
                     connect=lambda: (FakeOneNote(hierarchy(), pages_for()), hierarchy()))
        check("繋がったら、開いているノートブックを数える", "2 冊" in out, out)
        check("繋がったら、次の一手は言わない", "次の一手" not in out, out)

        empty = '<?xml version="1.0"?><one:Notebooks ' + NBS + '></one:Notebooks>'
        _, out = say(tree=lambda: {"1.1": {"win64"}}, store=lambda: "Microsoft.Office.OneNote_x",
                     connect=lambda: (FakeOneNote(empty, {}), empty))
        # **ここがストア版の落とし穴。** 繋がっても、写すものが一つも見えない。
        check("一冊も無いとき、ストア版の落とし穴を言う",
              "デスクトップ版" in out and "一冊も無い" in out, out)
        _, out = say(tree=lambda: {"1.1": {"win64"}}, store=lambda: False,
                     connect=lambda: (FakeOneNote(empty, {}), empty))
        check("ストア版が無ければ、その話はしない", "ストア版" not in out, out)

        _, out = say(store=lambda: "Microsoft.Office.OneNote_x")
        check("ストア版が入っていれば、そう言う", "ストア版も入っている" in out, out)
        _, out = say(store=lambda: None)
        check("読めないときは、黙る", "ストア版" not in out, out)
    finally:
        (o2m._office_platform, o2m._typelib_tree, o2m._local_server,
         o2m._store_onenote, o2m._typelib_path, o2m.connect_onenote,
         o2m._typelib_values, o2m._onenote_exe, o2m.os.path.exists) = keep


def t_offline(tmp):
    print("網に出られない端末で（オフライン）──")
    got = o2m.wheel_hint()
    body = "\n".join(got)
    v = sys.version_info
    # **持ち込むファイルの名前を言う。** 「pip install pywin32」は、網の無い
    # 端末では何も起きない ── 要るのはどのファイルを運ぶかで、それは
    # この Python の版と bit で決まる。
    check("この Python の版に合う wheel を名指しする", f"cp{v[0]}{v[1]}" in body, body)
    # **取り違えやすいのは bit。** `win32` は 32 bit で、`win_amd64` が 64 bit。
    name = next(l for l in got if l.startswith("この Python に合う wheel"))
    check("bit も名指しする", name.endswith("win_amd64.whl") or name.endswith("win32.whl"), name)
    check("取り違えないよう、bit の読み方を書く", "32 bit" in body and "64 bit" in body, body)
    check("網を見に行かせない", "--no-index" in body, body)
    check("連れも探しに行かせない", "--no-deps" in body, body)

    # **32 bit のほうが本題。** この機械では走らせられないので、bit を渡して見る
    # （`_arch_verdict` と同じ手）。
    n32 = next(l for l in o2m.wheel_hint(32) if l.startswith("この Python に合う wheel"))
    n64 = next(l for l in o2m.wheel_hint(64) if l.startswith("この Python に合う wheel"))
    check("32 bit の wheel は win32", n32.endswith("win32.whl"), n32)
    check("64 bit の wheel は win_amd64", n64.endswith("win_amd64.whl"), n64)
    run32 = " ".join(o2m.wheel_hint(32))
    run64 = " ".join(o2m.wheel_hint(64))
    check("32 bit なら -32 の呼び方で言う", "-32 -m pip" in run32, run32)
    check("64 bit では -32 と言わない", "-32" not in run64, run64)

    # pywin32 の無い端末で繋ごうとすると、そのまま出る。
    import subprocess
    r = subprocess.run([sys.executable, str(ROOT / "onenote2md.py"), "--check"],
                       capture_output=True, text=True, errors="replace")
    check("pywin32 が無いとき、持ち込むものを言う",
          "wheel" in (r.stdout + r.stderr) or "pywin32" in (r.stdout + r.stderr),
          (r.stdout + r.stderr)[-200:])


def t_two_views(tmp):
    print("レジストリの、二つの見え方 ──")
    keep = o2m._read_typelib
    A = "C:" + chr(92) + "本物.exe" + chr(92) + "3"
    B = "C:" + chr(92) + "x86" + chr(92) + "無い.exe" + chr(92) + "3"
    try:
        import sys as _sys
        flag64, flag32 = 256, 512     # winreg の KEY_WOW64_* に当たるもの

        class FakeWinreg:
            KEY_WOW64_64KEY = flag64
            KEY_WOW64_32KEY = flag32

        _sys.modules["winreg"] = FakeWinreg
        seen = {flag64: {("1.1", "win64"): A}, flag32: {("1.1", "win64"): B}}
        o2m._read_typelib = lambda f: seen.get(f, {})
        got = o2m._typelib_values()
        check("両方の見え方を読む", len(got) == 2, got)
        check("違うときは、どちらの話か添える",
              {g[3] for g in got} == {"64bit の見え方", "32bit の見え方"}, got)
        # **同じなら二度見せない。** 人が読む行が倍になる。
        seen = {flag64: {("1.1", "win64"): A}, flag32: {("1.1", "win64"): A}}
        got = o2m._typelib_values()
        check("同じなら一つにまとめる", got == [("1.1", "win64", A, "")], got)
        # 片方にしか無い枝も落とさない。
        seen = {flag64: {("1.1", "win64"): A}, flag32: {}}
        got = o2m._typelib_values()
        check("片方にしか無い枝も出す", len(got) == 1 and got[0][3] == "64bit の見え方", got)
    finally:
        o2m._read_typelib = keep
        import sys as _sys
        _sys.modules.pop("winreg", None)


def t_forget(tmp):
    print("作り置きを捨てる（--forget）──")
    import io, contextlib, tempfile as tf
    keep = o2m.tempfile
    fake = tmp / "tmphome"
    fake.mkdir(exist_ok=True)
    at = fake / f"amber-gen_py-{sys.version_info[0]}.{sys.version_info[1]}"
    at.mkdir(exist_ok=True)
    (at / "なにか.py").write_text("x", encoding="utf-8")
    try:
        o2m.tempfile = type("t", (), {"gettempdir": staticmethod(lambda: str(fake))})()
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            code = o2m.forget()
        out = buf.getvalue()
        # **人に二つのフォルダを行き来させない。** docs の言う場所と、
        # こちらが逃がした先は別（依頼 567）── 機械が両方消す。
        check("逃がした先の作り置きを捨てる", not at.exists(), list(fake.iterdir()))
        check("捨てたものを言う", str(at) in out, out)
        check("捨てた回は 0 を返す", code == 0, code)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            o2m.forget()
        out = buf.getvalue()
        check("無ければ、探した先を言う", "探した先" in out and str(fake) in out, out)
        check("捨てても困らないと言う", "作り直されます" in out, out)
    finally:
        o2m.tempfile = keep


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
    import io, contextlib, importlib.util as iu
    spec = iu.spec_from_file_location("onestore", ROOT / "onestore.py")
    ost = iu.module_from_spec(spec)
    spec.loader.exec_module(ost)
    sys.modules["onestore"] = ost

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


def t_launcher(tmp):
    print("ランチャーに訊く（py -0p）──")
    # **本物で叩く。** この機械に `py` は無い ── 診断の道具は壊れた機械の
    # 上で使うものなので、**訊けないことで落ちてはいけない。**
    try:
        real = o2m._other_pythons()
        ok = isinstance(real, list)
        why = ""
    except Exception as e:  # noqa
        ok, why = False, f"落ちた: {type(e).__name__}"
    check("ランチャーが居なくても、落ちない", ok, why)
    lines = [" -V:3.12 *        C:" + chr(92) + "P312" + chr(92) + "python.exe",
             " -V:3.9-32        C:" + chr(92) + "P39-32" + chr(92) + "python.exe",
             " -3.9-32          C:" + chr(92) + "old" + chr(92) + "python.exe",
             "これは行ではない"]

    class Got:
        stdout = "\n".join(lines)

    import subprocess
    keep = subprocess.run
    try:
        subprocess.run = lambda *a, **k: Got()
        got = o2m._other_pythons()
        check("並びを読める（新しい形も古い形も）", len(got) == 3, got)
        check("道も拾う", all(g[1].endswith("python.exe") for g in got), got)
        check("名札から 32 bit を見つける", o2m._thirty_two_bit_here() == "3.9-32",
              o2m._thirty_two_bit_here())
        subprocess.run = lambda *a, **k: (_ for _ in ()).throw(OSError("py が無い"))
        # **ランチャーの無い機械もある。** 分からないときは黙る ── 落ちるのも
        # 「黙る」の一種なので、受け止めて NG にする（依頼 569 で踏んだ形）。
        try:
            quiet = o2m._other_pythons() == [] and o2m._thirty_two_bit_here() is None
        except Exception as e:  # noqa
            quiet, e = False, f"落ちた: {type(e).__name__}"
        check("訊けなければ黙る", quiet, e if quiet is False else "")
    finally:
        subprocess.run = keep


def t_two_onenotes(tmp):
    print("365 とストア版、両方を使う ──")
    import io, contextlib
    empty = '<?xml version="1.0"?><one:Notebooks ' + NBS + '></one:Notebooks>'
    keep = (o2m._office_platform, o2m._typelib_tree, o2m._local_server,
            o2m._store_onenote, o2m.connect_onenote)
    try:
        o2m._office_platform = lambda: "x64"
        o2m._typelib_tree = lambda: {"1.1": {"win64"}}
        o2m._local_server = lambda: ("C:" + chr(92) + "ONENOTE.EXE", True)
        o2m._store_onenote = lambda: "Microsoft.Office.OneNote_x"
        o2m.connect_onenote = lambda: (FakeOneNote(empty, {}), empty)
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            o2m.check()
        out = buf.getvalue()
        # **両方入っていること自体は困らない。** 困るのはどちらで開いているか。
        check("両方あることを、責めずに言う", "365 側に開いたものだけが写せる" in out, out)
        check("一冊も無いとき、どちらで開くかを言う",
              "365 側でも開いておく" in out, out)
        check("「使うな」とは言わない", "使わない" not in out and "やめ" not in out, out)
    finally:
        (o2m._office_platform, o2m._typelib_tree, o2m._local_server,
         o2m._store_onenote, o2m.connect_onenote) = keep


def t_readonly_no_lock(tmp):
    print("読むだけの回は、鎖を取らない ──")
    import subprocess
    at = tmp / "nolock"
    at.mkdir(exist_ok=True)
    # 定時の回が走っている最中を真似る。そこを調べるための道具が、その回の
    # せいで断られるのでは道具にならない。
    with o2m.only_one(at):
        r = subprocess.run([sys.executable, str(ROOT / "onenote2md.py"),
                            "--out", str(at), "--probe"],
                           capture_output=True, text=True, errors="replace")
        check("鎖の中でも --probe は走る", "== python ==" in r.stdout,
              (r.stdout + r.stderr)[:160])
        r = subprocess.run([sys.executable, str(ROOT / "onenote2md.py"),
                            "--out", str(at), "--list"],
                           capture_output=True, text=True, errors="replace")
        r = subprocess.run([sys.executable, str(ROOT / "onenote2md.py"),
                            "--out", str(at), "--check"],
                           capture_output=True, text=True, errors="replace")
        check("鎖の中でも --check は走る", "Python " in r.stdout, (r.stdout + r.stderr)[:160])
        r = subprocess.run([sys.executable, str(ROOT / "onenote2md.py"),
                            "--out", str(at), "--forget"],
                           capture_output=True, text=True, errors="replace")
        check("鎖の中でも --forget は走る", "作り直されます" in r.stdout,
              (r.stdout + r.stderr)[:160])
        check("鎖の中でも --list は断られない", "前の回がまだ走っています" not in r.stderr,
              r.stderr[:160])
        # 書く回は、これまでどおり断る。
        r = subprocess.run([sys.executable, str(ROOT / "onenote2md.py"),
                            "--out", str(at), "--log", str(at / "l.log")],
                           capture_output=True, text=True, errors="replace")
        body = (at / "l.log").read_text(encoding="utf-8") if (at / "l.log").exists() else ""
        check("書く回は、二本目を断る", "前の回がまだ走っています" in body, body[-160:])


def main():
    tmp = Path(tempfile.mkdtemp(prefix="onenote-test-"))
    try:
        for fn in (t_names, t_amber_shape, t_structure, t_table, t_inline, t_created,
                   t_log, t_cp932, t_two_views, t_from_files, t_peek, t_forget, t_launcher, t_check, t_offline, t_two_onenotes, t_readonly_no_lock, t_incremental, t_same_file,
                   t_prune_scope, t_prune_error, t_stale_images, t_sync, t_lock,
                   t_select, t_select_flatten, t_select_prune, t_list, t_binding, t_gen_py, t_wrap, t_probe, t_verify, t_arch, t_arch_hint, t_resource_index, t_connect):
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
