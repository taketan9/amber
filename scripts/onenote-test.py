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
        <one:Table>
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
    check("表が組まれる", "| 名 | 値 |" in text and "| --- | --- |" in text)
    check("絵が attachments/ に落ちる",
          (out / "仕事" / "議事録" / "attachments" / "9月の定例_001.png").is_file())
    check("絵へのリンクが相対", "](attachments/9月の定例_001.png)" in text)


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
    print("書き直したページの、古い絵 ──")
    out = tmp / "img"
    app = FakeOneNote(hierarchy(), pages_for(p1_images=2))
    run(app, out)
    att = out / "仕事" / "議事録" / "attachments"
    check("下ごしらえ: 絵が二枚", (att / "9月の定例_001.png").is_file()
          and (att / "9月の定例_002.png").is_file())

    # 隣のページの絵（巻き込まれてはいけない）
    (att / "9月の定例録_001.png").write_bytes(PNG)

    app2 = FakeOneNote(hierarchy(mod_p1="2026-09-14T09:00:00.000Z"),
                       pages_for(mod_p1="2026-09-14T09:00:00.000Z", p1_images=1))
    run(app2, out)
    check("いま使っている絵は残る", (att / "9月の定例_001.png").is_file())
    check("使わなくなった絵は消える", not (att / "9月の定例_002.png").exists())
    check("名前が似ているだけの絵は巻き込まない", (att / "9月の定例録_001.png").is_file())

    # --no-images のときに全部消したりしない
    app3 = FakeOneNote(hierarchy(mod_p1="2026-09-14T10:00:00.000Z"),
                       pages_for(mod_p1="2026-09-14T10:00:00.000Z", p1_images=1))
    run(app3, out, "--no-images")
    check("--no-images でも既にある絵は消さない", (att / "9月の定例_001.png").is_file())


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
    for name, cls in (("早い束ね", EarlyBound), ("遅い束ね", LateBound), ("性悪", Sneaky)):
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


def _fake_win32com(ensure_module=None, ensure_dispatch=None, dispatch=None, gen_path=None):
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
    client.gencache = gencache
    pkg.client = client
    return {"win32com": pkg, "win32com.gen_py": gen_py,
            "win32com.client": client, "win32com.client.gencache": gencache}


def _connect_with(**mods):
    saved = {k: sys.modules.get(k) for k in
             ("win32com", "win32com.gen_py", "win32com.client",
              "win32com.client.gencache")}
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

    try:
        _connect_with()
        check("全部駄目なら、わけを並べて止まる", False, "通ってしまった")
    except SystemExit as e:
        msg = str(e)
        check("全部駄目なら、わけを並べて止まる",
              "EnsureModule は使えない" in msg and "Dispatch は使えない" in msg, msg[:80])
        check("止まるとき、心当たりを出す",
              "管理者" in msg and "gen_py" in msg and "ストア版" in msg, msg[-120:])


def main():
    tmp = Path(tempfile.mkdtemp(prefix="onenote-test-"))
    try:
        for fn in (t_names, t_structure, t_incremental, t_same_file,
                   t_prune_scope, t_prune_error, t_stale_images, t_sync, t_lock,
                   t_select, t_select_flatten, t_select_prune, t_list, t_binding, t_gen_py, t_wrap, t_probe, t_verify, t_arch, t_resource_index, t_connect):
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
