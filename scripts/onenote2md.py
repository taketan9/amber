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
  python onenote2md.py --out D:\\notes --notebook "案件A"    # 名前で絞り込み（部分一致）
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
def connect_onenote():
    try:
        import win32com.client  # noqa
    except ImportError:
        sys.exit("pywin32 が必要です:  pip install pywin32")
    try:
        app = win32com.client.Dispatch("OneNote.Application")
    except Exception as e:  # noqa
        sys.exit(f"OneNote (デスクトップ版) に接続できません: {e}")
    return app


def call_with_out(func, *args):
    """pywin32 は遅延バインディングの [out] 引数の扱いが版で違うため両方試す。"""
    try:
        return func(*args, "")
    except TypeError:
        return func(*args)


def get_hierarchy(app):
    return call_with_out(app.GetHierarchy, "", HS_PAGES)


def _get_page(app, page_id, info):
    try:
        return app.GetPageContent(page_id, "", info)
    except TypeError:
        return app.GetPageContent(page_id, info)


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
def walk_section(app, section_el, out_dir: Path, path_parts, args, stats, written: set, keep: set):
    sec_name = sanitize(section_el.get("name"))
    if section_el.get("locked") == "true":
        log.warning("パスワード保護のためスキップ: %s / %s", " / ".join(path_parts), sec_name)
        stats["skipped_sections"] += 1
        # **見えなかった場所は、消えた場所ではない。** 鍵のかかったセクションの
        # 下は今回一枚も出てこないので、`--prune` に任せると前回の写しを
        # まるごと消してしまう（そして次の回も鍵は開かないので、二度と戻らない）。
        keep.add((out_dir / sec_name).resolve())
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


def walk_container(app, el, out_dir: Path, path_parts, args, stats, written: set, keep: set, group_prefix=""):
    """Notebook / SectionGroup の下を再帰的に処理"""
    for child in el:
        tag = strip_ns(child.tag)
        if tag == "Section":
            if group_prefix:
                # --flatten-groups: グループ名を畳んで一段にする
                child_name = group_prefix + " › " + sanitize(child.get("name"))
                child.set("name", child_name)
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


def _under(path: Path, base: Path) -> bool:
    return path == base or base in path.parents


def prune(scope: list, keep: set, written: set, stats):
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
            if any(_under(r, k) for k in keep):
                continue
            if not read_head(at).get("onenote_id"):
                continue  # ambər で作ったノートは触らない
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
    app = connect_onenote()
    root = ET.fromstring(get_hierarchy(app))
    if args.sync and not args.dry_run:
        if sync_notebooks(app, root, args.notebook, args.sync_wait):
            root = ET.fromstring(get_hierarchy(app))   # 同期後の姿で読み直す
    notebooks = root.findall("one:Notebook", NS)
    if not notebooks:
        sys.exit("開いているノートブックがありません。OneNote 上で対象ノートブックを開いてください。")

    stats = {"pages": 0, "written": 0, "skipped_pages": 0, "images": 0, "errors": 0,
             "skipped_sections": 0, "pruned": 0, "pruned_images": 0}
    written: set = set()
    keep: set = set()
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

    log.info("完了: ページ %d（書いた %d・変わらず %d）/ 画像 %d / エラー %d / スキップしたセクション %d / 消した %d（絵 %d）",
             stats["pages"], stats["written"], stats["skipped_pages"], stats["images"],
             stats["errors"], stats["skipped_sections"], stats["pruned"], stats["pruned_images"])
    return 1 if stats["errors"] else 0


if __name__ == "__main__":
    sys.exit(main() or 0)
