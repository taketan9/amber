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


def cab_names(at):
    """CAB の**目録**を読む ── `[(名前, 大きさ)]`（入っている順）。

    **`expand` は日本語の名前を壊す。** 本文は無事なのに、セクションの名前
    （＝ファイル名）だけが化けるのはこれ（現場で出た・依頼 597）。
    中身は `expand` に開かせて、**名前はこちらで読む。**

    名前の符号は旗で決まる ── `_A_NAME_IS_UTF`（0x80）が立っていれば UTF-8、
    立っていなければ機械の符号（日本語 Windows なら cp932）。
    **名前に `\\` が入っていれば、それはフォルダ** ── セクショングループが
    そこに入っている。

    読めなければ空を返す。**分からないことを分かったように言わない。**
    """
    try:
        d = at.read_bytes() if hasattr(at, "read_bytes") else open(at, "rb").read()
    except OSError:
        return []
    if len(d) < 36 or d[:4] != b"MSCF":
        return []
    coff_files, n_files, flags = (struct.unpack("<I", d[16:20])[0],
                                  struct.unpack("<H", d[26:28])[0],
                                  struct.unpack("<H", d[30:32])[0])
    i = coff_files
    out = []
    for _ in range(n_files):
        if i + 16 > len(d):
            break
        cb = struct.unpack("<I", d[i:i + 4])[0]
        attribs = struct.unpack("<H", d[i + 14:i + 16])[0]
        j = d.find(b"\0", i + 16)
        if j < 0:
            break
        raw = d[i + 16:j]
        for enc in (("utf-8",) if attribs & 0x80 else ("cp932", "cp1252", "utf-8")):
            try:
                name = raw.decode(enc)
                break
            except UnicodeDecodeError:
                continue
        else:
            name = raw.decode("latin-1")
        out.append((name.replace(chr(92), "/"), cb))
        i = j + 1
    return out


def unpack_onepkg(at, into):
    """`.onepkg` を開く。**中身は CAB**（Windows 標準の `expand` で開ける）。

    OneNote が「ノートブック全体は `.pdf` `.xps` `.onepkg` だけ」と言うので、
    まとめて出すとこの形になる。中に入っているのは `.one` なので、
    **開けば形式が分かる** ── 書き出した `.one` が公開仕様なら、話が変わる。
    """
    import subprocess
    os.makedirs(into, exist_ok=True)
    with open(at, "rb") as f:
        sig = f.read(4)
    if sig != b"MSCF":
        return None, f"CAB ではない（先頭は {sig.hex()}）"
    try:
        got = subprocess.run(["expand", "-F:*", str(at), str(into)],
                             capture_output=True, timeout=600)
    except FileNotFoundError:
        return None, "expand が無い（Windows の外では開けない）"
    except subprocess.TimeoutExpired:
        return None, "expand が返ってこない"
    if got.returncode != 0:
        return None, f"expand が転んだ（{got.returncode}）"
    return into, None


def load_onestore():
    """隣の `onestore.py` を読む。**道を名指しする。**

    `import onestore` に頼ると、**走らせる場所によって通らない**（`sys.path` に
    `scripts/` が入るとは限らない）── 現場で `ModuleNotFoundError` になった。
    走査が偽物を `sys.modules` に差し込んでいたので、**本物の読み込みを一度も
    試していなかった**のが見逃した理由。偽物が本物より甘いと検査は嘘をつく。
    """
    import importlib.util
    got = sys.modules.get("onestore")
    if got is not None:
        return got                       # 一度読んだものを使い回す
    at = Path(__file__).resolve().parent / "onestore.py"
    if not at.is_file():
        sys.exit(f"{at} がありません（`git pull` は済んでいますか）。")
    spec = importlib.util.spec_from_file_location("onestore", at)
    mod = importlib.util.module_from_spec(spec)
    sys.modules["onestore"] = mod
    spec.loader.exec_module(mod)
    return mod


def opened_sections(pkg, at):
    """開いた `.onepkg` の中身を、**入れ物の中の道ごと**返す。

    `[(ノートブック, 道の並び, ファイル)]` ── 道の並びはセクショングループ。
    前の版は `rglob` で `.one` を集めるだけで**どのフォルダに居たかを捨てて
    いた**ので、`400_打合せ/月_定例` のような多層が一段に潰れ、**取りこぼして
    いるように見えた**（現場で気づかれた・依頼 597）。

    名前は CAB の目録から取る（`expand` は日本語を壊す）。目録と実物は
    **大きさで突き合わせる** ── 名前が化けている以上、名前では繋げない。
    """
    listed = [(n, cb) for n, cb in cab_names(pkg) if n.lower().endswith(".one")]
    on_disk = sorted(Path(at).rglob("*.one"), key=lambda q: q.stat().st_size)
    left = list(on_disk)
    out = []
    for name, cb in listed:
        got = next((q for q in left if q.stat().st_size == cb), None)
        if got is None:
            continue
        left.remove(got)
        parts = [x for x in name.split("/") if x and x not in (".", "..")]
        out.append((pkg.stem, parts[:-1], got, Path(parts[-1]).stem))
    if not out:
        # 目録が読めなかった（CAB でない・壊れている）── 名前は化けたままだが、
        # **黙って何も出さないよりはよい。**
        out = [(pkg.stem, list(q.relative_to(at).parts[:-1]), q, q.stem)
               for q in sorted(on_disk)]
    return out


def from_files(where, args, out_root):
    """**書き出したファイルから写す。** COM を通らない道（依頼 594）。

    受け取るのは `.onepkg`（入れ物）か `.one`（セクション一本）か、その両方が
    入ったフォルダ。`.onepkg` は `expand` で開いてから中の `.one` を読む。

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
            at, why = unpack_onepkg(q, into)
            if at is None:
                log.error("開けない %s: %s", q.name, why)
                continue
            sections += opened_sections(q, Path(at))
        else:
            sections.append((q.parent.name, [], q, q.stem))
    if not sections:
        sys.exit(f"{root} の下に .one がありません。")

    stats = {"pages": 0, "written": 0, "skipped_pages": 0, "images": 0, "errors": 0,
             "skipped_sections": 0, "filtered_sections": 0, "pruned": 0, "pruned_images": 0}
    written: set = set()
    for book, groups, at, name in sections:
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
        log.info("セクション: %s（%d ページ）", "/".join([nb] + gs + [sec]), len(got))
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
                    ext = (Path(im.get("name") or "").suffix or ".png").lstrip(".").lower()
                    kind = ext if ext in ("png", "jpg", "jpeg", "gif", "bmp", "webp") else "png"
                    fname = f"{stem}-{i:03d}.{kind}"
                    img_dir.mkdir(parents=True, exist_ok=True)
                    at_img = img_dir / fname
                    raw = im.get("bytes") or b""
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
    opened = []
    for q in [q for q in found if q.suffix.lower() == ".onepkg"]:
        into = Path(tempfile.gettempdir()) / f"amber-onepkg-{os.getpid()}-{q.stem[:20]}"
        at, why = unpack_onepkg(q, into)
        if at is None:
            print(f"開けない {q.name}: {why}")
            continue
        inner = [r for r in Path(at).rglob("*")
                 if r.is_file() and r.suffix.lower() in kinds]
        print(f"開いた {q.name} → 中に {len(inner)} 本")
        opened += inner
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
        from onenote_ui import ask_and_run
        return ask_and_run(args)

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
