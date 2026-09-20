"""試しの「チームの予定表」を 1 つ書く（依頼 471）。

取り決めは `docs/team-csv.ja.md`。**そこに書いてある嫌らしいところを
全部入れる** ── BOM・CRLF・件名の中の読点・件名の見えない予定・
取り消された予定・何日もある終日・「空き時間」。
"""
import sys

ROWS = [
    "fetched_at,owner,owner_mail,start,end,all_day,subject,location,"
    "show_as,sensitivity,cancelled,organizer,uid",
    "2026-09-09T08:15:00+09:00,山田 武,yamada@example.co.jp,"
    "2026-09-09T10:00:00+09:00,2026-09-09T11:00:00+09:00,false,週次定例,"
    "会議室A,busy,normal,false,鈴木 一郎,U1",
    "2026-09-09T08:15:00+09:00,山田 武,yamada@example.co.jp,"
    "2026-09-09T14:00:00+09:00,2026-09-09T15:30:00+09:00,false,"
    '"設計、および見積",,busy,normal,false,,U2',
    "2026-09-09T08:15:00+09:00,山田 武,yamada@example.co.jp,"
    "2026-09-10T00:00:00+09:00,2026-09-12T00:00:00+09:00,true,出張,,"
    "oof,normal,false,,U3",
    "2026-09-09T08:15:00+09:00,鈴木 一郎,suzuki@example.co.jp,"
    "2026-09-09T13:00:00+09:00,2026-09-09T14:00:00+09:00,false,,,"
    "busy,private,false,,U4",
    "2026-09-09T08:15:00+09:00,鈴木 一郎,suzuki@example.co.jp,"
    "2026-09-09T16:00:00+09:00,2026-09-09T17:00:00+09:00,false,消えた会議,,"
    "busy,normal,true,,U5",
    "2026-09-09T08:15:00+09:00,佐藤 花,sato@example.co.jp,"
    "2026-09-09T09:00:00+09:00,2026-09-09T09:30:00+09:00,false,あき,,"
    "free,normal,false,,U6",
]

with open(sys.argv[1], "wb") as f:
    f.write(("﻿" + "\r\n".join(ROWS) + "\r\n").encode("utf-8"))
