# チーム予定 CSV の取り決め（叩き台 v0.1）

**書く側**: Power Automate（クラウド）が Outlook を読んで、定期的に書き出す
**読む側**: ambər のカレンダー

**この文書が契約です。** 片方が勝手に列を変えると、もう片方は黙って落ちます。
変えるときは両方のセッションで合わせてください。

---

## ファイル

| | |
| --- | --- |
| 名前 | `team-calendar.csv`（固定。日付を付けません ―― **常に最新の1つ**） |
| 文字コード | **UTF-8（BOM あり）** |
| 改行 | CRLF |
| 引用 | RFC 4180（`"` で囲み、中の `"` は `""`） |

**BOM を付ける理由**: 付けないと Excel でそのまま開いたとき日本語が化けます。
中身を目で確かめられることに意味があるので付けます。
**読む側は BOM を落としてから解釈してください。**

---

## 列（この順・この名前）

見出しは ASCII に固定します。日本語の見出しは、書く側（Power Automate）と
読む側（Rust）で符号化の事故が起きやすいためです。

| # | 列 | 例 | 説明 |
| --- | --- | --- | --- |
| 1 | `fetched_at` | `2026-09-10T08:15:00+09:00` | **この行を取り出した時刻**。全行同じ |
| 2 | `owner` | `山田 武` | 予定表の表示名（人の名前） |
| 3 | `owner_mail` | `yamada.takeshi@example.co.jp` | **突き合わせの鍵はこちら**。名前は揺れます |
| 4 | `start` | `2026-09-10T10:00:00+09:00` | 開始（ISO 8601・オフセット付き） |
| 5 | `end` | `2026-09-10T11:00:00+09:00` | 終了 |
| 6 | `all_day` | `true` / `false` | 終日か |
| 7 | `subject` | `週次定例` | 件名。**取れないときは空**（下記） |
| 8 | `location` | `会議室A` | 場所。無ければ空 |
| 9 | `show_as` | `busy` | `free` / `tentative` / `busy` / `oof` / `workingElsewhere` / `unknown` |
| 10 | `sensitivity` | `normal` | `normal` / `personal` / `private` / `confidential` |
| 11 | `cancelled` | `false` | 取り消された予定か |
| 12 | `organizer` | `鈴木 一郎` | 主催者。無ければ空 |
| 13 | `uid` | `040000008200E00074C5B7…` | 予定の識別子（Graph の `iCalUId`） |

### 重複を見分ける鍵は `uid` + `start`

繰り返しの予定は**期間内に展開して1行ずつ**出します。展開された回は
**同じ `uid` で `start` が違う**ので、`uid` だけで重ねると消えます。

---

## 意味の取り決め

**① これはスナップショットです。**
書き出すたびに**ファイルごと置き換わります**。`fetched_at` より後の変更は
入っていません。**読む側は追記ではなく「入れ替え」として扱ってください** ――
前回あって今回無い予定は、**消された**か**期間から外れた**かのどちらかです。

**② 期間は「今日から N 日」。** 既定は 14 日。
`start` がその範囲に入る予定だけが出ます（範囲の外は行ごとありません）。

**③ 件名が空のことがあります。**
非公開の予定や、空き時間しか見えない相手では件名が取れません。
そのときは `subject` が空で、`show_as` は入っています ――
**「予定あり」として描けます**。`sensitivity` が `private` なら、
中身は見えていないという意味です。

**④ 人を突き合わせる鍵は `owner_mail`。**
`owner` は表示名なので、同姓・改姓・全角半角で揺れます。

**⑤ 空の値は空文字。** `null` も `-` も書きません。

---

## 見本

```csv
fetched_at,owner,owner_mail,start,end,all_day,subject,location,show_as,sensitivity,cancelled,organizer,uid
2026-09-10T08:15:00+09:00,山田 武,yamada.takeshi@example.co.jp,2026-09-10T10:00:00+09:00,2026-09-10T11:00:00+09:00,false,週次定例,会議室A,busy,normal,false,鈴木 一郎,040000008200E00074C5B7101A82E008
2026-09-10T08:15:00+09:00,山田 武,yamada.takeshi@example.co.jp,2026-09-11T00:00:00+09:00,2026-09-12T00:00:00+09:00,true,終日出張,,oof,normal,false,,040000008200E00074C5B7101A82E009
2026-09-10T08:15:00+09:00,鈴木 一郎,suzuki.ichiro@example.co.jp,2026-09-10T14:00:00+09:00,2026-09-10T15:00:00+09:00,false,,,busy,private,false,,040000008200E00074C5B7101A82E00A
```

3行目が**件名の見えない予定**です（`subject` が空・`sensitivity` が `private`）。

---

## 出どころ（Power Automate 側の対応）

読むのは **「カレンダー ビューの取得 (V3)」**（`Get calendar view of events`）です。
「イベントの取得」は**繰り返しの予定を1回しか返さない**ので使いません。

| CSV の列 | Graph の項目 |
| --- | --- |
| `start` / `end` | `start.dateTime` + `start.timeZone` |
| `all_day` | `isAllDay` |
| `subject` | `subject` |
| `location` | `location.displayName` |
| `show_as` | `showAs` |
| `sensitivity` | `sensitivity` |
| `cancelled` | `isCancelled` |
| `organizer` | `organizer.emailAddress.name` |
| `uid` | `iCalUId` |

---

## まだ決まっていないこと

- **置き場所**（SharePoint のどこか／同期フォルダ／共有フォルダ）
- **何分おきに書き出すか**（既定 15 分のつもり）
- **対象の15人**をどう指定するか（Power Automate 側に一覧を持ちます）
- 期間を 14 日から変えるか
