//! **人ごとに並べる予定表を、CSV から読む**（依頼 471）。
//!
//! 会社の Outlook は、外から読める形を一つも出してくれない ── ICS も
//! 出せず、Graph も閉じている。そこで、**別の道具が三十分おきに書き出す
//! 1 つの CSV** を読む。amber は取りに行かない。置いてあるものを読むだけ。
//!
//! 列の取り決めは `docs/team-csv.ja.md`（書き出す側と交わしたもの）。
//! ただし**その形だけを読むようには作らない** ── 見出しの名前も日時の
//! 書き方も道具によって揺れる。読めるものは読み、読めない行は黙って
//! 飛ばす。一行のせいで、その日の全員が消えるほうがずっと困る。

use chrono::{NaiveDate, NaiveTime, TimeZone};

/// CSV の一行から起こした、誰かの予定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// 縦に並べるときに出す名前。
    pub who: String,
    /// **突き合わせの鍵。** 表示名は同姓・改姓・全角半角で揺れるので、
    /// 段をまとめるのはこちら（無ければ表示名で代用する）。
    pub mail: String,
    pub day: String,
    /// `None` は終日。
    pub at: Option<String>,
    pub to: Option<String>,
    pub title: String,
    pub place: String,
    /// `busy` / `tentative` / `oof` / `workingElsewhere` など。色に使う。
    pub show: String,
    /// **中身が見えていない。** 件名が伏せられているという意味で、
    /// 「取れなかった」とは違う ── 出し方を変える。
    pub shut: bool,
}

/// 段に並べる一人。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who {
    pub name: String,
    pub mail: String,
}

/// 1 件分。
#[derive(Debug, Clone, Default)]
pub struct Team {
    /// **いつ時点のものか。** 三十分おきに置き換わる紙なので、これを
    /// 出さないと、古い紙を今の予定だと思って読むことになる。
    pub fetched: String,
    pub people: Vec<Who>,
    pub plans: Vec<Plan>,
}

/// その月ぶんを読む。
///
/// **段に出す人は、その月に予定があるかどうかで決めない。** 予定の無い
/// 週の人が段ごと消えると、その人だけ書き出せていないのか、本当に空
/// なのかが読めない ── 紙に出てくる人は、空でも段を持つ。
pub fn of(text: &str, year: i32, month: u32) -> Team {
    let mut got = all(text);
    let want = format!("{year:04}-{month:02}");
    got.plans.retain(|p| p.day.starts_with(&want));
    got
}

/// ぜんぶ。
pub fn all(text: &str) -> Team {
    let rows = rows(text);
    let Some(head) = rows.first() else {
        return Team::default();
    };
    let cols = Columns::find(head);
    // **人の列が無ければ、人ごとに並べようがない。** ほかの列だけ当たって
    // いても出さない ── 全員が「（名前なし）」の一段に潰れるほうが困る。
    if cols.who.is_none() && cols.mail.is_none() {
        return Team::default();
    }
    let mut out = Team::default();
    // **同じ予定を二度出さない。** 置き換えの途中を読んだり、書き出す側が
    // 二回まわしたりすると、同じ行が並ぶ。繰り返しの予定は展開されて
    // くるので、`uid` だけで重ねると消える ── 開始と組にして見る。
    let mut said: std::collections::HashSet<(String, String, String)> =
        std::collections::HashSet::new();
    for row in rows.iter().skip(1) {
        if out.fetched.is_empty() {
            out.fetched = cols.get(row, cols.fetched).to_string();
        }
        // **段は、予定より先に立てる。** その人の行が「空き時間」と
        // 取り消しだけだった週に段ごと消えると、書き出せていないのか
        // 本当に空なのかが画面から区別できない ── いちばん知りたいのが
        // 「空いているかどうか」なのに。
        if let Some(w) = cols.person(row) {
            if !out.people.iter().any(|x| x.mail == w.mail) {
                out.people.push(w);
            }
        }
        let Some(made) = cols.plans(row) else { continue };
        for p in made {
            let uid = cols.get(row, cols.uid).to_string();
            let key = (
                p.mail.clone(),
                if uid.is_empty() { p.title.clone() } else { uid },
                format!("{} {}", p.day, p.at.clone().unwrap_or_default()),
            );
            if !said.insert(key) {
                continue;
            }
            out.plans.push(p);
        }
    }
    out.people.sort_by(|a, b| a.name.cmp(&b.name));
    out.plans.sort_by(|a, b| {
        a.mail
            .cmp(&b.mail)
            .then(a.day.cmp(&b.day))
            .then(a.at.cmp(&b.at))
            .then(a.title.cmp(&b.title))
    });
    out
}

/// 見出しの行から、どの列が何かを当てる。
struct Columns {
    who: Option<usize>,
    mail: Option<usize>,
    fetched: Option<usize>,
    uid: Option<usize>,
    gone: Option<usize>,
    shut: Option<usize>,
    title: Option<usize>,
    from: Option<usize>,
    from_day: Option<usize>,
    from_at: Option<usize>,
    till: Option<usize>,
    till_day: Option<usize>,
    till_at: Option<usize>,
    place: Option<usize>,
    whole: Option<usize>,
    show: Option<usize>,
}

/// 見出しを見比べる形に均す。**大文字小文字・空白・記号を落とす** ──
/// 「開始日時」「開始 日時」「Start Time」「start_time」は同じ列。
fn plain(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-' && *c != '　' && *c != '\u{feff}')
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn hit(head: &[String], names: &[&str]) -> Option<usize> {
    let flat: Vec<String> = head.iter().map(|h| plain(h)).collect();
    // **ぴったり合うものを先に。** 「開始」と「開始日」が両方あるとき、
    // 前から順に見ていくと「開始日」が「開始」で当たってしまう。
    for n in names {
        if let Some(i) = flat.iter().position(|h| h == n) {
            return Some(i);
        }
    }
    None
}

/// 日と時刻が**別の列に分かれている**形をほどく。
///
/// 「開始時刻」は道具によって意味が違う ── 一つの列に日時をまとめて書く
/// ものもあれば、日本語版 Outlook のように「開始日」と対で時刻だけを書く
/// ものもある。**「開始日」が別にあるなら、時刻の列は時刻**と読む。
fn pair(
    head: &[String],
    one: &[&str],
    day: &[&str],
    at: &[&str],
) -> (Option<usize>, Option<usize>, Option<usize>) {
    let day = hit(head, day);
    let mut one = hit(head, one);
    let mut clock = hit(head, at);
    if day.is_some() {
        // 「開始日」があるなら、「開始時刻」は時刻の列。
        if clock.is_none() {
            clock = one;
        }
        if clock == one {
            one = None;
        }
    } else {
        // 日の列が無いなら、時刻だけの列は使いようがない。
        clock = None;
    }
    (one, day, clock)
}

impl Columns {
    fn find(head: &[String]) -> Self {
        let (from, from_day, from_at) = pair(
            head,
            &[
                "start", "開始", "開始日時", "開始時刻", "starttime", "startdatetime",
                "startsat", "from", "begin",
            ],
            &["開始日", "startdate", "startday"],
            &["開始時刻", "開始時間", "starttime", "starttimeofday"],
        );
        let (till, till_day, till_at) = pair(
            head,
            &[
                "end", "終了", "終了日時", "終了時刻", "endtime", "enddatetime",
                "endsat", "to", "finish",
            ],
            &["終了日", "enddate", "endday"],
            &["終了時刻", "終了時間", "endtime", "endtimeofday"],
        );
        Self {
            from,
            from_day,
            from_at,
            till,
            till_day,
            till_at,
            who: hit(
                head,
                &[
                    "owner", "名前", "氏名", "表示名", "担当", "担当者", "社員", "メンバー",
                    "人", "ユーザー", "ユーザー名", "name", "displayname", "username",
                    "user", "member", "person", "calendar", "ownername",
                ],
            ),
            // **突き合わせの鍵。** 表示名は揺れるので、段をまとめるのは
            // こちら（取り決めの `owner_mail`）。
            mail: hit(
                head,
                &[
                    "ownermail", "owner_mail", "メール", "メールアドレス", "mail", "email",
                    "emailaddress", "upn", "userprincipalname", "address",
                ],
            ),
            fetched: hit(head, &["fetchedat", "取得時刻", "取得日時", "asof", "updatedat"]),
            uid: hit(head, &["uid", "icaluid", "id", "eventid", "識別子"]),
            // 取り消された予定は、予定ではない。
            gone: hit(head, &["cancelled", "canceled", "iscancelled", "取消", "取り消し"]),
            shut: hit(head, &["sensitivity", "公開範囲", "秘密度"]),
            title: hit(
                head,
                &["件名", "予定", "予定名", "タイトル", "内容", "subject", "title", "summary"],
            ),
            place: hit(head, &["場所", "会議室", "location", "place", "room", "where"]),
            whole: hit(
                head,
                &["終日", "終日イベント", "allday", "isallday", "alldayevent", "wholeday"],
            ),
            show: hit(
                head,
                &["公開方法", "状態", "予定の状態", "showas", "status", "freebusy", "busystatus"],
            ),
        }
    }

    fn get<'a>(&self, row: &'a [String], at: Option<usize>) -> &'a str {
        at.and_then(|i| row.get(i)).map(|s| s.trim()).unwrap_or("")
    }

    /// その行が誰のものか。**予定として出すかどうかとは別に見る。**
    fn person(&self, row: &[String]) -> Option<Who> {
        let who = self.get(row, self.who);
        let mail = self.get(row, self.mail);
        if who.is_empty() && mail.is_empty() {
            return None;
        }
        Some(Who {
            name: if who.is_empty() { mail.to_string() } else { who.to_string() },
            mail: if mail.is_empty() { who.to_string() } else { mail.to_string() },
        })
    }

    /// 1 行から、**日ごとに 1 件ずつ**起こす。
    ///
    /// 終日の予定は何日にもまたがることがあり、頭の日にだけ出すと、
    /// 三日の出張が初日しか出ない ── 表の上では「二日目から居る」と
    /// 読めてしまう。
    fn plans(&self, row: &[String]) -> Option<Vec<Plan>> {
        let who = self.get(row, self.who);
        let mail = self.get(row, self.mail);
        if who.is_empty() && mail.is_empty() {
            return None;
        }
        // **取り消された予定は、予定ではない。** 残すと、消えたはずの
        // 会議のせいで一日が埋まって見える。
        if yes(self.get(row, self.gone)) {
            return None;
        }
        // **「空き時間」は出さない。** 本人が「空いています」と言っている
        // ものを予定として並べると、誰が空いているかを見るための表が、
        // まさにそこで読めなくなる。
        let show = plain(self.get(row, self.show));
        if show == "free" || show == "空き時間" || show == "空き" {
            return None;
        }

        // 終日かどうかを**先に**決める。終日の日付は、どこで読んでも
        // その日 ── 時差で直すと、日をまたいで前日にずれる。
        let said = plain(self.get(row, self.whole));
        let flagged = match said.as_str() {
            "true" | "1" | "yes" | "はい" | "○" | "真" => Some(true),
            "false" | "0" | "no" | "いいえ" => Some(false),
            _ => None,
        };
        let float = flagged == Some(true);
        let start = self.moment(row, self.from, self.from_day, self.from_at, float)?;
        let end = self.moment(row, self.till, self.till_day, self.till_at, float);
        let whole = flagged.unwrap_or(start.1.is_none());

        let title = self.get(row, self.title);
        let shut = matches!(
            plain(self.get(row, self.shut)).as_str(),
            "private" | "confidential" | "personal"
        );
        // **名前のない予定にも、何かを出す。** 件名を伏せて書き出す会社が
        // ある ── 空欄の帯は、押せない模様にしか見えない。
        //
        // 「見せてもらえていない」と「読み取れなかった」は違うので、文言も
        // 分ける。**どちらも「空」とは書かない** ── 直前の行で「空き時間」
        // を落としているので、同じ文言を使うと「空いている」と読める。
        // ここに帯が出ているということは、**予定はある**。
        let title = if !title.is_empty() {
            title.to_string()
        } else if shut {
            "非公開".to_string()
        } else {
            "件名なし".to_string()
        };

        let one = |day: chrono::NaiveDate, at: Option<String>, to: Option<String>| Plan {
            who: if who.is_empty() { mail.to_string() } else { who.to_string() },
            mail: if mail.is_empty() { who.to_string() } else { mail.to_string() },
            day: day.format("%Y-%m-%d").to_string(),
            at,
            to,
            title: title.clone(),
            place: self.get(row, self.place).to_string(),
            show: show.clone(),
            shut,
        };

        if whole {
            // **終わりの日は「入らない」。** 一日の終日予定は、翌日の
            // 零時が終わりとして書き出される（Graph も ICS もこの数え方）。
            let mut out = Vec::new();
            let last = end.map(|e| e.0).unwrap_or(start.0);
            let mut d = start.0;
            while d < last && out.len() < 400 {
                out.push(one(d, None, None));
                d = d.succ_opt()?;
            }
            if out.is_empty() {
                out.push(one(start.0, None, None));
            }
            return Some(out);
        }

        let to = end.and_then(|e| {
            // **日をまたいだら、その日の終わりまで。** 翌朝の時刻をそのまま
            // 高さにすると、帯が上に向かって伸びる。
            if e.0 == start.0 {
                e.1.map(clock)
            } else if e.0 > start.0 {
                Some("24:00".to_string())
            } else {
                None
            }
        });
        Some(vec![one(start.0, start.1.map(clock), to)])
    }

    /// 日時を一つ取る。**一つの列にまとまっている形と、日と時刻が別の
    /// 列に分かれている形**（日本語版 Outlook の書き出しはこちら）の両方。
    fn moment(
        &self,
        row: &[String],
        one: Option<usize>,
        day: Option<usize>,
        at: Option<usize>,
        float: bool,
    ) -> Option<(NaiveDate, Option<NaiveTime>)> {
        let both = self.get(row, one);
        if !both.is_empty() {
            return read(both, float);
        }
        let d = self.get(row, day);
        if d.is_empty() {
            return None;
        }
        let t = self.get(row, at);
        if t.is_empty() {
            return read(d, float);
        }
        read(&format!("{d} {t}"), float)
    }
}

/// 「はい」と書いてあるか。書き方は道具によって違う。
fn yes(s: &str) -> bool {
    matches!(plain(s).as_str(), "true" | "1" | "yes" | "はい" | "○" | "真")
}

fn clock(t: NaiveTime) -> String {
    t.format("%H:%M").to_string()
}

/// 日時の文字列を読む。
///
/// **タイムゾーン付きのものは、端末のローカル時刻に直す。** Graph が返すのは既定で
/// 世界時なので、直さないと十時の会議が朝一時に並ぶ ── 一目で分かる
/// 壊れ方ではなく、「なんだか一日ずれている」という形で出る。
pub fn moment(text: &str) -> Option<(NaiveDate, Option<NaiveTime>)> {
    read(text, false)
}

/// `float` が真なら、**時差を無視してそのままの日付を取る** ── 終日の
/// 予定はどこで読んでもその日で、直すと前日にずれる。
fn read(text: &str, float: bool) -> Option<(NaiveDate, Option<NaiveTime>)> {
    let s = text.trim().trim_matches('"').trim();
    if s.is_empty() {
        return None;
    }
    // タイムゾーン部分を切り離す。
    let (body, shift) = split_shift(s);
    let body = body.trim();
    let (dpart, tpart) = match body.find(['T', 't', ' ']) {
        Some(i) if body.len() > i + 1 => (&body[..i], body[i + 1..].trim()),
        _ => (body, ""),
    };
    let date = date(dpart)?;
    let time = if tpart.is_empty() { None } else { time(tpart) };

    match (if float { None } else { shift }, time) {
        (Some(off), Some(t)) => {
            let naive = date.and_time(t);
            let there = chrono::FixedOffset::east_opt(off)?.from_local_datetime(&naive).single()?;
            let here = there.with_timezone(&chrono::Local).naive_local();
            Some((here.date(), Some(here.time())))
        }
        _ => Some((date, time)),
    }
}

/// 末尾の `Z` や `+09:00` を切り離して、秒に直す。
fn split_shift(s: &str) -> (&str, Option<i32>) {
    if let Some(rest) = s.strip_suffix(['Z', 'z']) {
        return (rest, Some(0));
    }
    let b = s.as_bytes();
    // `+09:00` / `-0500` / `+09`
    for cut in [6usize, 5, 3] {
        if b.len() > cut {
            let i = b.len() - cut;
            let sign = b[i] as char;
            if sign != '+' && sign != '-' {
                continue;
            }
            // 日付の `-` と間違えない ── 前に時刻が要る。
            if !s[..i].contains(':') {
                continue;
            }
            let tail = &s[i + 1..];
            let digits: String = tail.chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() != 4 && digits.len() != 2 {
                continue;
            }
            let h: i32 = digits[..2].parse().ok().unwrap_or(0);
            let m: i32 = if digits.len() == 4 { digits[2..].parse().ok().unwrap_or(0) } else { 0 };
            let secs = (h * 60 + m) * 60;
            return (&s[..i], Some(if sign == '-' { -secs } else { secs }));
        }
    }
    (s, None)
}

fn date(s: &str) -> Option<NaiveDate> {
    let parts: Vec<&str> = s.split(['-', '/', '.', '年', '月']).filter(|p| !p.is_empty()).collect();
    let parts: Vec<&str> = parts.iter().map(|p| p.trim_end_matches('日')).collect();
    if parts.len() < 3 {
        return None;
    }
    let a: i32 = parts[0].trim().parse().ok()?;
    let b: u32 = parts[1].trim().parse().ok()?;
    let c: u32 = parts[2].trim().parse().ok()?;
    // **四桁のほうが年。** アメリカ式の `9/10/2026` と日本式の `2026/9/10`
    // は、どちらも三つ組で来る ── 年がどちらにあるかで見分ける。
    if parts[0].trim().len() == 4 || a > 31 {
        NaiveDate::from_ymd_opt(a, b, c)
    } else {
        NaiveDate::from_ymd_opt(c as i32, a as u32, b)
    }
}

fn time(s: &str) -> Option<NaiveTime> {
    let low = s.to_ascii_lowercase();
    let pm = low.contains("pm") || s.contains("午後");
    let am = low.contains("am") || s.contains("午前");
    let body: String = s
        .chars()
        .map(|c| if c == '時' || c == '分' { ':' } else { c })
        .filter(|c| c.is_ascii_digit() || *c == ':')
        .collect();
    let parts: Vec<&str> = body.split(':').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    let mut h: u32 = parts[0].parse().ok()?;
    let m: u32 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
    let sec: u32 = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0);
    if pm && h < 12 {
        h += 12;
    }
    if am && h == 12 {
        h = 0;
    }
    NaiveTime::from_hms_opt(h, m, sec)
}

/// CSV を表形式に。
///
/// **括りの中の区切りと改行を、区切りとして読まない。** 件名に読点が入る
/// のはふつうのことで（「定例、および…」）、そこで列がずれると、その一行
/// から先の全部が一つずつ横にずれる ── 気づきにくい壊れ方の代表格。
fn rows(text: &str) -> Vec<Vec<String>> {
    let sep = guess(text);
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut inside = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if inside {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    inside = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' => inside = true,
            _ if c == sep => row.push(std::mem::take(&mut cell)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut cell));
                if row.iter().any(|f| !f.trim().is_empty()) {
                    out.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            _ => cell.push(c),
        }
    }
    row.push(cell);
    if row.iter().any(|f| !f.trim().is_empty()) {
        out.push(row);
    }
    // 頭の BOM を落とす。
    if let Some(first) = out.first_mut() {
        if let Some(cell) = first.first_mut() {
            *cell = cell.trim_start_matches('\u{feff}').to_string();
        }
    }
    out
}

/// 区切りを当てる。**タブ区切りで来ることがある** ── Excel で開いて
/// 保存し直すと、そうなる環境がある。
fn guess(text: &str) -> char {
    let head = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let commas = head.matches(',').count();
    let tabs = head.matches('\t').count();
    let semis = head.matches(';').count();
    if tabs > commas && tabs >= semis {
        '\t'
    } else if semis > commas {
        ';'
    } else {
        ','
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "名前,件名,開始,終了,場所\n";

    /// 取り決め（`docs/team-csv.ja.md`）のサンプルが、そのまま読めること。
    /// **ここが通らなくなったら、書き出す側と話が合っていない。**
    const DEAL: &str = "\u{feff}fetched_at,owner,owner_mail,start,end,all_day,subject,location,show_as,sensitivity,cancelled,organizer,uid\r\n2026-09-10T08:15:00+09:00,山田 武,yamada.takeshi@example.co.jp,2026-09-10T10:00:00+09:00,2026-09-10T11:00:00+09:00,false,週次定例,会議室A,busy,normal,false,鈴木 一郎,040000008200E00074C5B7101A82E008\r\n2026-09-10T08:15:00+09:00,山田 武,yamada.takeshi@example.co.jp,2026-09-11T00:00:00+09:00,2026-09-12T00:00:00+09:00,true,終日出張,,oof,normal,false,,040000008200E00074C5B7101A82E009\r\n2026-09-10T08:15:00+09:00,鈴木 一郎,suzuki.ichiro@example.co.jp,2026-09-10T14:00:00+09:00,2026-09-10T15:00:00+09:00,false,,,busy,private,false,,040000008200E00074C5B7101A82E00A\r\n";

    /// `+09:00` の時刻を、**端末のローカル時刻で**言い直す（依頼 551）。
    ///
    /// 「10:00」と書いてしまうと、**東京の机でしか通らない試験**になる ──
    /// 時刻つきの予定は現地時刻に直す決まり（依頼 471）なので、UTC の環境が
    /// 読めば 01:00 が正しい。CI は Windows でしか試験を回さず、あそこは
    /// UTC なので、`team.rs` が入った日から落ちていた（v3.0.0 のタグを打つ前の
    /// 試し組みで、初めて鳴った）。
    fn 東京の(h: u32, m: u32) -> chrono::DateTime<chrono::Local> {
        use chrono::TimeZone;
        chrono::FixedOffset::east_opt(9 * 3600)
            .unwrap()
            .with_ymd_and_hms(2026, 9, 10, h, m, 0)
            .unwrap()
            .with_timezone(&chrono::Local)
    }

    /// **v0.3 のサンプルが、そのまま読めること**（2026-09-14）。
    ///
    /// `docs/team-csv.ja.md` のサンプルそのもの。v0.1 から 3 つ変わった:
    /// **`cancelled` の列が消え**、`organizer` は表示名になり、開始と終了は
    /// 最初からローカル時刻になった。列を落とされても落ちないことを、
    /// 上の v0.1 の試験とは**別に**見る ── 片方だけ通る形があるので。
    const DEAL3: &str = "\u{feff}fetched_at,owner,owner_mail,start,end,all_day,subject,location,show_as,sensitivity,organizer,uid\r\n2026-09-10T08:15:00+09:00,山田 武,yamada.takeshi@example.co.jp,2026-09-10T10:00:00+09:00,2026-09-10T11:00:00+09:00,false,週次定例,会議室A,busy,normal,鈴木 一郎,040000008200E00074C5B7101A82E008\r\n2026-09-10T08:15:00+09:00,山田 武,yamada.takeshi@example.co.jp,2026-09-11T00:00:00+09:00,2026-09-12T00:00:00+09:00,true,終日出張,,oof,normal,,040000008200E00074C5B7101A82E009\r\n2026-09-10T08:15:00+09:00,鈴木 一郎,suzuki.ichiro@example.co.jp,2026-09-10T14:00:00+09:00,2026-09-10T15:00:00+09:00,false,,,busy,private,,040000008200E00074C5B7101A82E00A\r\n";

    #[test]
    fn v03_の形が取り決めどおり読める() {
        let got = of(DEAL3, 2026, 9);
        assert_eq!(got.fetched, "2026-09-10T08:15:00+09:00", "いつ時点かを出す");
        assert_eq!(got.people.len(), 2, "二人");
        assert_eq!(got.plans.len(), 3, "取消の列が無くても、三件そのまま");
        let 始 = 東京の(10, 0);
        let one = got.plans.iter().find(|p| p.title == "週次定例").unwrap();
        assert_eq!(one.at.as_deref(), Some(始.format("%H:%M").to_string().as_str()));
        assert_eq!(one.mail, "yamada.takeshi@example.co.jp", "鍵はメール");
        let hidden = got.plans.iter().find(|p| p.shut).unwrap();
        assert_eq!(hidden.title, "非公開", "件名が見えなくても、予定はある");
    }

    /// **同じ会議は、出席者の人数ぶん残ること**（v0.3 の鍵・2026-09-14）。
    ///
    /// チームの会議は出席者全員の予定表に**同じ識別子・同じ開始**で入る
    /// （実機で 2,218 件のうち 1,009 件がこれ）── `uid` と開始だけで重ねると、
    /// **その会議が一人ぶんしか残らない**。人ごとの段に並べる紙なので、
    /// 鍵には人が要る。
    #[test]
    fn 同じ会議は出席者全員に残る() {
        let csv = "owner,owner_mail,start,end,subject,uid\n                   山田 武,a@x.jp,2026-09-10T10:00:00+09:00,2026-09-10T11:00:00+09:00,週次定例,U1\n                   鈴木 一郎,b@x.jp,2026-09-10T10:00:00+09:00,2026-09-10T11:00:00+09:00,週次定例,U1\n                   佐藤 花子,c@x.jp,2026-09-10T10:00:00+09:00,2026-09-10T11:00:00+09:00,週次定例,U1\n                   山田 武,a@x.jp,2026-09-10T10:00:00+09:00,2026-09-10T11:00:00+09:00,週次定例,U1\n";
        let got = of(csv, 2026, 9);
        assert_eq!(got.people.len(), 3, "三人");
        assert_eq!(got.plans.len(), 3, "同じ人の同じ行だけを重ねる（三人ぶんは残す）");
        let mut who: Vec<&str> = got.plans.iter().map(|p| p.mail.as_str()).collect();
        who.sort();
        assert_eq!(who, vec!["a@x.jp", "b@x.jp", "c@x.jp"]);
    }

    #[test]
    fn 取り決めた形が取り決めどおり読める() {
        let got = of(DEAL, 2026, 9);
        assert_eq!(got.fetched, "2026-09-10T08:15:00+09:00", "いつ時点かを出す");
        assert_eq!(got.people.len(), 2, "二人");
        assert_eq!(got.plans.len(), 3);

        let 始 = 東京の(10, 0);
        let 終 = 東京の(11, 0);
        let teiれい = got.plans.iter().find(|p| p.title == "週次定例").unwrap();
        assert_eq!(teiれい.day, 始.format("%Y-%m-%d").to_string(), "時刻つきは現地時刻の日");
        assert_eq!(teiれい.at.as_deref(), Some(始.format("%H:%M").to_string().as_str()));
        assert_eq!(teiれい.to.as_deref(), Some(終.format("%H:%M").to_string().as_str()));
        assert_eq!(teiれい.place, "会議室A");
        assert_eq!(teiれい.mail, "yamada.takeshi@example.co.jp", "鍵はメール");
        assert_eq!(teiれい.show, "busy");

        let tabi = got.plans.iter().find(|p| p.title == "終日出張").unwrap();
        assert_eq!(tabi.day, "2026-09-11");
        assert_eq!(tabi.at, None, "終日は時刻を持たない");
        assert_eq!(tabi.show, "oof");

        // 件名の見えない予定。
        let hidden = got.plans.iter().find(|p| p.shut).unwrap();
        assert_eq!(hidden.title, "非公開");
        assert_eq!(hidden.who, "鈴木 一郎");
    }

    /// **段に出す人は、表示名ではなくメールでまとめる。** 同じ人が
    /// 「山田 武」「山田武」で書き出されても、段は一つ。
    #[test]
    fn 同じ人を二通りに書いても一つの列になる() {
        let csv = "owner,owner_mail,subject,start,end\n                   山田 武,a@example.jp,朝会,2026-09-10T09:00:00+09:00,2026-09-10T09:30:00+09:00\n                   山田武,a@example.jp,夕会,2026-09-10T17:00:00+09:00,2026-09-10T17:30:00+09:00\n";
        let got = of(csv, 2026, 9);
        assert_eq!(got.people.len(), 1);
        assert_eq!(got.plans.len(), 2);
    }

    /// **同じ行が二度あっても、二度は出さない。** 置き換えの途中を読むと
    /// そうなる。ただし**繰り返しの予定は `uid` が同じで開始が違う**ので、
    /// `uid` だけで重ねると回が消える。
    #[test]
    fn 重複行は落とすが_繰り返しの予定は落とさない() {
        let csv = "owner_mail,subject,start,end,uid\n                   a@x.jp,定例,2026-09-10T10:00:00+09:00,2026-09-10T11:00:00+09:00,U1\n                   a@x.jp,定例,2026-09-10T10:00:00+09:00,2026-09-10T11:00:00+09:00,U1\n                   a@x.jp,定例,2026-09-17T10:00:00+09:00,2026-09-17T11:00:00+09:00,U1\n";
        let got = of(csv, 2026, 9);
        assert_eq!(got.plans.len(), 2, "同じ回は一つ、次の回は残る");
    }

    /// 取り消された予定は、予定ではない。
    #[test]
    fn 取り消された予定は_その日を埋めない() {
        let csv = "owner,subject,start,end,cancelled\n                   山田,消えた会議,2026-09-10T10:00:00+09:00,2026-09-10T11:00:00+09:00,true\n                   山田,ある会議,2026-09-10T13:00:00+09:00,2026-09-10T14:00:00+09:00,false\n";
        let got = of(csv, 2026, 9);
        assert_eq!(got.plans.len(), 1);
        assert_eq!(got.plans[0].title, "ある会議");
    }

    /// **何日もある終日の予定は、日ごとに出す。** 頭の日にだけ出すと、
    /// 三日の出張が初日しか出ず、二日目から居るように読める。
    #[test]
    fn 複数日にまたがる終日予定は_どの日にも出る() {
        let csv = "owner,subject,start,end,all_day\n                   山田,出張,2026-09-10T00:00:00+09:00,2026-09-13T00:00:00+09:00,true\n";
        let got = of(csv, 2026, 9);
        let days: Vec<&str> = got.plans.iter().map(|p| p.day.as_str()).collect();
        assert_eq!(days, vec!["2026-09-10", "2026-09-11", "2026-09-12"],
                   "終わりの日は入らない（Graph も ICS もこの数え方）");
    }

    /// **終日の日付は、時差で直さない。** 直すと、どこで読むかで前日に
    /// ずれる ── 一日の出張が前日に出る。
    #[test]
    fn 終日予定は日付を保つ() {
        let csv = "owner,subject,start,end,all_day\n                   山田,休み,2026-09-10T00:00:00+09:00,2026-09-11T00:00:00+09:00,true\n";
        assert_eq!(of(csv, 2026, 9).plans[0].day, "2026-09-10");
    }

    #[test]
    fn 素の形式を読める() {
        let csv = format!(
            "{HEAD}山田 太郎,定例,2026-09-10 10:00,2026-09-10 11:00,会議室A\n\
             鈴木 花子,面談,2026-09-10 13:30,2026-09-10 14:00,\n"
        );
        let got = of(&csv, 2026, 9);
        assert_eq!(got.plans.len(), 2);
        assert_eq!(got.people.len(), 2);
        let one = got.plans.iter().find(|p| p.who == "山田 太郎").unwrap();
        assert_eq!(one.at.as_deref(), Some("10:00"));
        assert_eq!(one.to.as_deref(), Some("11:00"));
        assert_eq!(one.place, "会議室A");
    }

    /// **件名に読点が入っても、列がずれない。** ここがずれると、その行から
    /// 先の全部が一つずつ横にずれる。
    #[test]
    fn 件名の中のカンマで列がずれない() {
        let csv = format!("{HEAD}山田,\"定例、および報告\",2026-09-10 10:00,2026-09-10 11:00,A\n");
        let got = of(&csv, 2026, 9);
        assert_eq!(got.plans.len(), 1);
        assert_eq!(got.plans[0].title, "定例、および報告");
        assert_eq!(got.plans[0].place, "A");
    }

    /// 括りの中の改行と、括りの中の `""`。
    #[test]
    fn 引用符の中の改行は_新しい行ではない() {
        let csv = format!("{HEAD}山田,\"一行目\n二行目\",2026-09-10 10:00,,\n山田,\"\"\"引用\"\"\",2026-09-11 09:00,,\n");
        let got = of(&csv, 2026, 9);
        assert_eq!(got.plans.len(), 2);
        assert!(got.plans.iter().any(|p| p.title == "一行目\n二行目"));
        assert!(got.plans.iter().any(|p| p.title == "\"引用\""));
    }

    /// **UTC は、端末のローカル時刻に直す**（Graph の既定がこれ）。
    #[test]
    fn utc_の時刻はローカル時刻になる() {
        let got = of(&format!("{HEAD}山田,朝会,2026-09-10T01:00:00Z,2026-09-10T02:00:00Z,\n"), 2026, 9);
        let same = of(&format!("{HEAD}山田,朝会,2026-09-10T10:00:00+09:00,2026-09-10T11:00:00+09:00,\n"), 2026, 9);
        assert_eq!(got.plans[0].at, same.plans[0].at);
        assert_eq!(got.plans[0].day, same.plans[0].day);
    }

    /// 時差の無いものは、そのまま読む（直すと二重にずれる）。
    #[test]
    fn タイムゾーンの無い時刻はそのまま() {
        let csv = format!("{HEAD}山田,朝会,2026-09-10T10:00:00,2026-09-10T11:00:00,\n");
        assert_eq!(of(&csv, 2026, 9).plans[0].at.as_deref(), Some("10:00"));
    }

    /// 日と時刻が**別の列**に分かれている形（日本語版 Outlook の書き出し）。
    #[test]
    fn 日付と時刻が別の列でも読める() {
        let csv = "表示名,件名,開始日,開始時刻,終了日,終了時刻,終日イベント\n\
                   山田,定例,2026/09/10,10:00:00,2026/09/10,11:30:00,FALSE\n";
        let got = of(csv, 2026, 9);
        assert_eq!(got.plans[0].at.as_deref(), Some("10:00"));
        assert_eq!(got.plans[0].to.as_deref(), Some("11:30"));
    }

    /// アメリカ式の `9/10/2026` と、午前午後。
    #[test]
    fn 米国式の形式も読める() {
        let csv = "Name,Subject,Start Time,End Time,Location,All day event\n\
                   Yamada,Sync,9/10/2026 1:00:00 PM,9/10/2026 2:00:00 PM,Room,FALSE\n";
        let got = of(csv, 2026, 9);
        assert_eq!(got.plans[0].day, "2026-09-10");
        assert_eq!(got.plans[0].at.as_deref(), Some("13:00"));
        assert_eq!(got.plans[0].to.as_deref(), Some("14:00"));
    }

    /// 件名を伏せて書き出す会社がある ── 空欄の帯は出さない。
    /// **「空」とは書かない**（「空き時間」と読み違える）。
    #[test]
    fn 件名の無い予定でも何か言う() {
        let csv = format!("{HEAD}山田,,2026-09-10 10:00,2026-09-10 11:00,\n");
        assert_eq!(of(&csv, 2026, 9).plans[0].title, "件名なし");
    }

    /// 「空き時間」は、空いているという意味なので出さない。
    #[test]
    fn 空き時間は予定ではない() {
        let csv = "名前,件名,開始,終了,公開方法\n\
                   山田,あき,2026-09-10 10:00,2026-09-10 11:00,Free\n\
                   山田,会議,2026-09-10 13:00,2026-09-10 14:00,Busy\n";
        let got = of(csv, 2026, 9);
        assert_eq!(got.plans.len(), 1);
        assert_eq!(got.plans[0].title, "会議");
    }

    /// **一日じゅう空いている人も、段を持つ。** 段ごと消えると、書き出せて
    /// いないのか本当に空なのかが、画面からは区別できない。
    #[test]
    fn 空き時間しか無い人にも列ができる() {
        let csv = "名前,件名,開始,終了,公開方法,cancelled\n\
                   山田,会議,2026-09-10 13:00,2026-09-10 14:00,Busy,false\n\
                   佐藤,あき,2026-09-10 10:00,2026-09-10 11:00,Free,false\n\
                   鈴木,消えた,2026-09-10 10:00,2026-09-10 11:00,Busy,true\n";
        let got = of(csv, 2026, 9);
        assert_eq!(got.plans.len(), 1);
        let names: Vec<&str> = got.people.iter().map(|w| w.name.as_str()).collect();
        assert!(names.contains(&"佐藤"), "空いている人の段が消えた");
        assert!(names.contains(&"鈴木"), "取り消しだけの人の段が消えた");
    }

    /// 日をまたぐ予定は、**その日の終わりで切る** ── 翌朝の時刻をそのまま
    /// 高さにすると、帯が上に向かって伸びる。
    #[test]
    fn 日付をまたぐ予定は_その日の終わりで止まる() {
        let csv = format!("{HEAD}山田,夜勤,2026-09-10 22:00,2026-09-11 06:00,\n");
        assert_eq!(of(&csv, 2026, 9).plans[0].to.as_deref(), Some("24:00"));
    }

    /// **人の列が無ければ、何も出さない。** 全員が一段に潰れるほうが困る。
    #[test]
    fn 人の列が無ければ何も出ない() {
        let csv = "件名,開始,終了\n定例,2026-09-10 10:00,2026-09-10 11:00\n";
        assert!(of(csv, 2026, 9).plans.is_empty());
    }

    /// 読めない行は飛ばして、読める行は出す ── 一行のせいで全部消えない。
    #[test]
    fn 壊れた行が_ほかの行を巻き添えにしない() {
        let csv = format!("{HEAD}山田,こわれ,いつか,,\n山田,定例,2026-09-10 10:00,,\n");
        let got = of(&csv, 2026, 9);
        assert_eq!(got.plans.len(), 1);
        assert_eq!(got.plans[0].title, "定例");
    }

    /// タブ区切り（Excel で開いて保存し直すと、そうなる環境がある）。
    #[test]
    fn タブ区切りも読める() {
        let csv = "名前\t件名\t開始\t終了\n山田\t定例\t2026-09-10 10:00\t2026-09-10 11:00\n";
        assert_eq!(of(csv, 2026, 9).plans.len(), 1);
    }

    /// 頭の BOM で、一つ目の見出しが読めなくならない（取り決めは BOM あり）。
    #[test]
    fn bom_があっても最初の列は隠れない() {
        let csv = format!("\u{feff}{HEAD}山田,定例,2026-09-10 10:00,,\n");
        assert_eq!(of(&csv, 2026, 9).plans.len(), 1);
    }

    /// よその月は出さない。
    #[test]
    fn ほかの月は入らない() {
        let csv = format!("{HEAD}山田,先月,2026-08-31 10:00,,\n山田,今月,2026-09-01 10:00,,\n");
        let got = of(&csv, 2026, 9);
        assert_eq!(got.plans.len(), 1);
        assert_eq!(got.plans[0].title, "今月");
    }

    /// 空っぽの紙でも落ちない（置き換えの途中を読むと、そうなることがある）。
    #[test]
    fn 空のシートでも落ちない() {
        assert!(of("", 2026, 9).plans.is_empty());
        assert!(of("fetched_at,owner,start\n", 2026, 9).plans.is_empty());
    }
}
