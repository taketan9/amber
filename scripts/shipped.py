#!/usr/bin/env python3
"""窓に配られていないものが、溜まっていないか。

**電話はケーブル、窓はリリース。** 道が違うので、片方だけが進む。実際に
そうなった ── iPhone には毎回入れていたのに、窓は `v2.5.0` のまま 23 コミット
置いていかれ、本人が「ゴミ箱に入れるに入力欄が出る、なんか古い気がする」と
気づくまで誰も知らなかった。直したはずのものが、直っていないように見える。

**憶えておく約束にしない。** 「電話に入れるときは窓も上げる」を決まりとして
書いても、忘れたときに何も鳴らない ── 忘れるから決まりを作るのに、その
決まりを憶えていないと効かない。機械が数える。

版の札を押すのは **Taketan**（自分からは上げない）ので、ここは上げない。
**溜まっていることを言う**だけ。

    python3 scripts/shipped.py
"""
import subprocess
import sys

# ここを超えたら止める。**日数ではなく件数** ── 一日に二十入る日もあれば、
# 一週間触らない週もある。「窓が動くものが何件溜まったか」のほうが、配る
# べきかどうかに近い。
LIMIT = 10

# 窓に配られるもの。README や PLANS を直しただけの日に鳴らせても、
# 鳴らした意味が無い（そして意味の無い警告は、そのうち誰も読まない）。
WATCH = ("gui/", "crates/", "packaging/")


def run(*args):
    return subprocess.run(args, capture_output=True, text=True).stdout.strip()


def main():
    tag = run("git", "describe", "--tags", "--abbrev=0", "--match", "v*")
    if not tag:
        print("版の札がまだありません（`git tag v0.1.0` から）")
        return 0

    commits = run("git", "rev-list", f"{tag}..HEAD", "--", *WATCH).splitlines()
    n = len(commits)
    when = run("git", "log", "-1", "--format=%ad", "--date=format:%m/%d %H:%M", tag)

    if n == 0:
        print(f"窓に配ってあります（{tag} ・ {when}）")
        return 0

    print(f"窓に配られていないものが {n} 件あります（いまの札は {tag} ・ {when}）")
    for c in commits[:5]:
        print("  " + run("git", "log", "-1", "--format=%h %s", c))
    if n > 5:
        print(f"  ほか {n - 5} 件")

    if n <= LIMIT:
        return 0

    print()
    print(f"**{LIMIT} 件を超えました。** 窓は札を押した版しか配られないので、")
    print("直したはずのものが、使う人には直っていないように見えます。")
    print("版を上げるかどうかは Taketan が決めます ── 訊いてください。")
    print("  git tag v?.?.? && git push origin v?.?.?")
    return 1


if __name__ == "__main__":
    sys.exit(main())
