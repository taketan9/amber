#!/usr/bin/env python3
"""押して選ぶだけの小さいウィンドウ（依頼 610）。

    py -3 scripts\\onenote2md.py            ← 何も渡さなければ、これが出る
    onenote2md.bat をダブルクリック          ← 同じ

**バッチをダブルクリックした人は、引数を渡せない。** そこで使い方を出して
終わるのは道具ではない ── 押す場所を出す。

**`tkinter` だけで組む。** Python に最初から入っていて、ネットワークに出られない
会社の端末でも何も足さずに動く（本人の端末はネットワークの外・依頼 586）。
見た目より、**入っていることのほうが値打ちがある。**

**変換は別の糸で走らせる。** 同じ糸でやると、デスクトップ版が固まって「落ちた」ように
見える ── `.onepkg` 一本で数分かかることがある（本人の端末で実測）。
ウィンドウへ書き戻すのは主の糸だけ（Tk の決まり）なので、経過は箱に入れて渡す。
"""

from __future__ import annotations

import queue
import re
import subprocess
import sys
import threading
from pathlib import Path

HERE = Path(__file__).resolve().parent


def _docs() -> Path:
    """この人の「ドキュメント」。**無ければ home。**"""
    for d in (Path.home() / "Documents", Path.home() / "ドキュメント"):
        if d.is_dir():
            return d
    return Path.home()


def _guess_out() -> Path:
    """出力先の下見。

    **ambər の既定の保存ディレクトリの中には掘らない**（本人・依頼 596 の前)。
    隣に `OneNote` を作って、ambər 側で保存ディレクトリを一つ足してもらう。
    """
    return _docs() / "OneNote"


def _guess_in() -> str:
    """取り込むものの下見。**書き出し先に置いてあることが多い。**"""
    for d in (_docs(), Path.home() / "Downloads", Path.home() / "デスクトップ",
              Path.home() / "Desktop"):
        if not d.is_dir():
            continue
        got = sorted(d.glob("*.onepkg"))
        if got:
            return str(got[0])
    return ""


def command(where: str, out: str) -> list:
    """走らせる一行。**デスクトップ版の外に出しておく** ── Tk の要る環境でしか
    確かめられない形にすると、押したときに何が走るのかを誰も試せない。

    `sys.executable` を使う ── `py` や `python` を探し直すと、デスクトップ版を出した
    Python と**別の Python** で走りかねない（会社の端末には何本か入っている）。
    """
    return [sys.executable, str(HERE / "onenote2md.py"), "--out", out, where]


def ask_and_run(args) -> int:
    """デスクトップ版を出して、押されたら走らせる。**閉じられたら 0**（やめただけ）。"""
    try:
        import tkinter as tk
        from tkinter import filedialog, ttk
    except Exception as e:                                   # noqa: BLE001
        # tkinter の入っていない Python はある（Linux の一部）。
        # **黙って何もしないのではなく、代わりの打ち方を出す。**
        print("デスクトップ版を出せません:", e)
        print("コマンドで:  py -3 scripts\\onenote2md.py --out <出力先> <.onepkg>")
        return 1

    win = tk.Tk()
    win.title("OneNote を ambər に取り込む")
    win.minsize(620, 380)
    pad = {"padx": 12, "pady": 6}

    src = tk.StringVar(value=(args.files or _guess_in()))
    dst = tk.StringVar(value=(args.out or str(_guess_out())))
    say = tk.StringVar(value="取り込むものと出力先を決めて、「取り込む」を押してください。")

    frm = ttk.Frame(win)
    frm.pack(fill="both", expand=True)
    frm.columnconfigure(1, weight=1)

    ttk.Label(frm, text="取り込むもの").grid(row=0, column=0, sticky="w", **pad)
    ttk.Entry(frm, textvariable=src).grid(row=0, column=1, sticky="ew", **pad)

    def pick_file():
        got = filedialog.askopenfilename(
            title="OneNote が書き出したもの",
            filetypes=[("OneNote が書き出したもの", "*.onepkg *.one"), ("ぜんぶ", "*.*")])
        if got:
            src.set(got)

    def pick_src_dir():
        got = filedialog.askdirectory(title=".one が入ったフォルダ")
        if got:
            src.set(got)

    btns = ttk.Frame(frm)
    btns.grid(row=0, column=2, sticky="e", **pad)
    ttk.Button(btns, text="ファイル…", command=pick_file).pack(side="left")
    ttk.Button(btns, text="フォルダ…", command=pick_src_dir).pack(side="left", padx=(6, 0))

    ttk.Label(frm, text="出力先").grid(row=1, column=0, sticky="w", **pad)
    ttk.Entry(frm, textvariable=dst).grid(row=1, column=1, sticky="ew", **pad)

    def pick_out():
        got = filedialog.askdirectory(title="どこに書き出すか")
        if got:
            dst.set(got)

    ttk.Button(frm, text="フォルダ…", command=pick_out).grid(row=1, column=2, sticky="e", **pad)

    ttk.Label(frm, textvariable=say, wraplength=560, foreground="#6b5a41") \
        .grid(row=2, column=0, columnspan=3, sticky="w", **pad)

    seen = tk.Text(frm, height=10, wrap="word")
    seen.grid(row=3, column=0, columnspan=3, sticky="nsew", padx=12, pady=(0, 6))
    frm.rowconfigure(3, weight=1)
    seen.configure(state="disabled")

    # **回っているだけの棒は置かない**（依頼 617）。本人の端末では一度も
    # 動かず、「止まっている」ようにしか見えなかった ── 測っていないものを
    # 測っている顔で見せるくらいなら、**何本目を写しているかを文字で出す。**
    foot = ttk.Frame(frm)
    foot.grid(row=4, column=0, columnspan=3, sticky="e", **pad)
    go = ttk.Button(foot, text="取り込む")
    go.pack(side="left")
    opener = ttk.Button(foot, text="出力先を開く", state="disabled")
    opener.pack(side="left", padx=(6, 0))
    ttk.Button(foot, text="閉じる", command=win.destroy).pack(side="left", padx=(6, 0))

    box: "queue.Queue[tuple[str, str]]" = queue.Queue()

    def put(line: str) -> None:
        seen.configure(state="normal")
        seen.insert("end", line + "\n")
        seen.see("end")
        seen.configure(state="disabled")

    def work(where: str, out: str) -> None:
        """別の糸で走らせる ── **子process で。**

        同じ process の中で呼ぶと、`logging` の出しどころを横取りすることに
        なるうえ、途中で落ちたときにウィンドウごと道連れになる。
        """
        cmd = command(where, out)
        try:
            pr = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                  text=True, encoding="utf-8", errors="replace",
                                  creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            for line in pr.stdout:                            # type: ignore[union-attr]
                box.put(("行", line.rstrip()))
            box.put(("終", str(pr.wait())))
        except Exception as e:                                # noqa: BLE001
            box.put(("行", "落ちました: " + str(e)))
            box.put(("終", "1"))

    def drain() -> None:
        try:
            while True:
                kind, what = box.get_nowait()
                if kind == "行":
                    put(what)
                    # **いま何本目かは、本体が `[3/12]` と言う。** そこだけ
                    # 取り出して上の一行に出す ── 箱の文字は流れて消える。
                    got = re.search(r"\[(\d+)/(\d+)\]\s*(.*)", what)
                    if got:
                        say.set(f"写しています（{got.group(1)}/{got.group(2)}）… "
                                f"{got.group(3)[:60]}")
                else:
                    go.configure(state="normal")
                    ok = what == "0"
                    say.set("取り込みました。ambər の ⚙ →「保存ディレクトリの追加・変更・削除」で"
                            "出力先を足すと読めます。" if ok else
                            "うまくいきませんでした。上の行に理由が出ています。")
                    if ok:
                        opener.configure(state="normal")
                    return
        except queue.Empty:
            pass
        win.after(120, drain)

    def start() -> None:
        where, out = src.get().strip(), dst.get().strip()
        if not where:
            say.set("取り込むものを選んでください（.onepkg / .one / それが入ったフォルダ）。")
            return
        if not Path(where).exists():
            say.set("ありません: " + where)
            return
        if not out:
            say.set("出力先を選んでください。")
            return
        go.configure(state="disabled")
        opener.configure(state="disabled")
        say.set("取り込んでいます… 本数が多いと数分かかります。")
        threading.Thread(target=work, args=(where, out), daemon=True).start()
        win.after(120, drain)

    def reveal() -> None:
        at = dst.get().strip()
        if sys.platform == "win32":
            subprocess.Popen(["explorer", at])
        elif sys.platform == "darwin":
            subprocess.Popen(["open", at])
        else:
            subprocess.Popen(["xdg-open", at])

    go.configure(command=start)
    opener.configure(command=reveal)
    win.mainloop()
    return 0
