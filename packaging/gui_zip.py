#!/usr/bin/env python3
"""同梱する側へ渡す zip（`amber-gui.zip`）を作る。

    python3 packaging/gui_zip.py out/amber-gui.zip

入れるのは3 つ。

  * `gui/**` ── 画面そのもの（`node_modules` は入れない。あそこには
    Electron 本体が居て、同梱する側は自分の Electron の中で動かす）
  * `packaging/amber-mark.png` ── 画面はアイコンを `../packaging/amber-mark.png`
    と指している。`gui/` だけではアイコンの出ない画面になる
  * `packaging/welcome/**` ── 「サンプルノートを入れる」のコピー元。
    入れておかないと、同梱した側でそれを実行した人に「サンプルが入っていません」が出る

**`zip` コマンドではなく Python で作る。** サンプルノートの名前は日本語で、
1 つは `ambər へようこそ.md`（シュワー入り）── Info-ZIP の `zip` は既定で
**UTF-8 フラグ（EFS ビット）を立てない**ので、日本語 Windows で展開すると
名前が化ける。`zip -UN=UTF8` で立つが、**Apple が同梱している zip はその
オプションを知らない**（`short option 'N' not supported`）ので、手元で試せない
ものを CI にだけ書くことになる。Python の `zipfile` は非 ASCII の名前に
必ずフラグを立てるし、手元でも CI でも同じものが出る。

作ったあと、**自分で開いて数える** ── 入れたつもりで空、を配らないため。
"""

from __future__ import annotations

import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def rows() -> list[tuple[Path, str]]:
    """(実ファイル, zip の中でのパス)。"""
    out: list[tuple[Path, str]] = []
    for at in sorted((ROOT / "gui").rglob("*")):
        if not at.is_file():
            continue
        rel = at.relative_to(ROOT)
        if "node_modules" in rel.parts:
            continue
        out.append((at, rel.as_posix()))
    out.append((ROOT / "packaging" / "amber-mark.png", "packaging/amber-mark.png"))
    for at in sorted((ROOT / "packaging" / "welcome").rglob("*")):
        if at.is_file():
            out.append((at, at.relative_to(ROOT).as_posix()))
    lic = ROOT / "LICENSE"
    if lic.exists():
        out.append((lic, "LICENSE"))
    return out


# **フォルダの項目には、フォルダだと書く。**
#
# 素の `ZipInfo` は権限を持たない ── そのまま書くと `0o600`（`S_IFDIR` も
# 実行ビットも無し）になり、Unix で展開したフォルダに**入れなくなる**。
# 中のファイルは正しく入っているのに `gui/index.html` が「無い」ように
# 見えるので、配ってから同梱する側の CI が止まって初めて分かった。
# Windows は Unix の権限を見ないので、いちばん配りたい相手では起きない。
#
#   `0o40755 << 16` … S_IFDIR と rwxr-xr-x（入るには実行ビットが要る）
#   `| 0x10`         … MS-DOS のフォルダ属性（Windows の道具が見る）
DIR_MODE = (0o40755 << 16) | 0x10


def folder(name: str) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(name)
    info.external_attr = DIR_MODE
    return info


def check(at: Path) -> None:
    """作った zip を開いて、必要なものが本当に入っているか。"""
    with zipfile.ZipFile(at) as z:
        names = z.namelist()
        info = z.infolist()

    def want(pred, what: str) -> int:
        n = sum(1 for x in names if pred(x))
        if n == 0:
            sys.exit(f"NG: {what} が入っていません")
        return n

    want(lambda x: x == "gui/index.html", "画面")
    want(lambda x: x == "gui/vendor/monaco/vs/loader.js", "エディタ")
    want(lambda x: x == "gui/vendor/mermaid/mermaid.min.js", "図")
    want(lambda x: x == "packaging/amber-mark.png", "アイコン")
    seen = want(lambda x: x.startswith("packaging/welcome/") and x.endswith(".md"), "サンプルノート")

    if any(x.startswith("gui/node_modules/") for x in names):
        sys.exit("NG: node_modules が混ざっています（200MB 超）")

    # **名前が化けないフラグが立っているか。** ここが立っていないと、
    # 日本語 Windows で展開したときにサンプルの名前だけが読めなくなる。
    flat = [i.filename for i in info
            if not i.filename.isascii() and not (i.flag_bits & 0x800)]
    if flat:
        sys.exit("NG: UTF-8 フラグが立っていない名前があります: " + ", ".join(flat[:3]))

    # **フォルダの項目に、フォルダだと書いてあるか。** 一度これを落とし、
    # Unix で「中身の無いフォルダ」に見える zip を 3 バージョンぶん配った。
    for i in info:
        if not i.filename.endswith("/"):
            continue
        mode = i.external_attr >> 16
        if not mode & 0o040000:
            sys.exit(f"NG: {i.filename} が「フォルダ」になっていません（S_IFDIR 無し）")
        if not mode & 0o111:
            sys.exit(f"NG: {i.filename} に実行ビットがありません（Unix で中に入れません）")

    kb = at.stat().st_size // 1024
    print(f"できました: {at} ({kb} KB ・ {len(names)} 件 ・ サンプル {seen} 枚)")


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit("使い方: python3 packaging/gui_zip.py <出す先>.zip")
    at = Path(sys.argv[1])
    at.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(at, "w", zipfile.ZIP_DEFLATED) as z:
        # **フォルダの項目も書く。** `zip -r` は書くので、書かないと
        # 同じ中身でも**構造の違う zip** になる ── 同梱する側は配られた
        # この zip をそのまま組み込むと言ってきていて、実機で展開まで
        # 確かめてある。ほどく側はたいてい親を勝手に作るが、
        # 「たいてい」で配るものではない。
        seen: set[str] = set()
        for _, inside in rows():
            parts = inside.split("/")[:-1]
            for i in range(len(parts)):
                d = "/".join(parts[: i + 1]) + "/"
                if d not in seen:
                    seen.add(d)
                    z.writestr(folder(d), b"")
        for real, inside in rows():
            z.write(real, inside)
    check(at)


if __name__ == "__main__":
    main()
