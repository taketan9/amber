//! **よその予定表を読む**（依頼 456）。
//!
//! Google カレンダーの「非公開 URL（iCal 形式）」も、iCloud の公開リンクも、
//! 書き出した `.ics` ファイルも、中身は同じ一つの形（RFC 5545）── だから
//! **口を一つ持てば、どこの予定表でも読める**。会社ごとの繋ぎこみも、
//! アカウントも、鍵も要らない。
//!
//! # 読むだけ
//!
//! この形は**読むためのもの**で、書き戻す口が無い。向こうが正で、こちらは
//! 写し ── 写しを書き換えても向こうは変わらないので、書けるふりをしない。
//!
//! # 返す形は `month` と同じ
//!
//! 画面は「この月の、どの日に、何があるか」しか要らない。同じ形で返すので、
//! 画面は amber 自身の予定と混ぜて並べるだけでよい。
//!
//! # 分からない時間帯は、その機械の時間として読む
//!
//! `TZID=Asia/Tokyo` のような名札は付いているが、**時間帯の表を抱えない**
//! ── 抱えると、その表が古くなった日に静かに一時間ずれる。
//! `Z`（世界標準時）だけはその機械の時間へ直し、それ以外は書いてある
//! とおりの時刻として読む。**自分の予定表を自分の国で見るぶんには合う。**

use chrono::{Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

/// よその予定、一つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub day: NaiveDate,
    /// 終日なら `None`。
    pub at: Option<NaiveTime>,
    pub title: String,
    /// 場所（`LOCATION`）。無ければ空。
    pub place: String,
}

/// 折り返された行を継ぎ、`名前;引数:中身` に分ける。
///
/// **継ぐのが先。** 長い題は途中で折り返して次の行の頭に空白を置く決まりで、
/// 継がずに読むと題が切れる（そして切れた続きが別の項目に見える）。
fn lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if (line.starts_with(' ') || line.starts_with('\t')) && !out.is_empty() {
            let last = out.len() - 1;
            out[last].push_str(&line[1..]);
        } else {
            out.push(line.to_string());
        }
    }
    out
}

/// `\,` `\;` `\n` `\\` を字に戻す。
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') | Some('N') => out.push(' '),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// `20260909T140000Z` / `20260909T140000` / `20260909`。
fn stamp(value: &str, utc: bool) -> Option<(NaiveDate, Option<NaiveTime>)> {
    let v = value.trim();
    let day = NaiveDate::from_ymd_opt(
        v.get(0..4)?.parse().ok()?,
        v.get(4..6)?.parse().ok()?,
        v.get(6..8)?.parse().ok()?,
    )?;
    if v.len() < 15 || !v.as_bytes().get(8).is_some_and(|c| *c == b'T') {
        return Some((day, None));
    }
    let time = NaiveTime::from_hms_opt(
        v.get(9..11)?.parse().ok()?,
        v.get(11..13)?.parse().ok()?,
        v.get(13..15)?.parse().ok()?,
    )?;
    if !utc {
        return Some((day, Some(time)));
    }
    // 世界標準時は、その機械の時間へ。
    let here = chrono::Utc
        .from_utc_datetime(&NaiveDateTime::new(day, time))
        .with_timezone(&chrono::Local);
    Some((here.date_naive(), Some(here.time())))
}

/// 繰り返しの決まり（要るところだけ）。
#[derive(Debug, Clone, Default)]
struct Rule {
    freq: String,
    every: u32,
    /// 週ごとのとき、どの曜日か（月曜=0）。空なら開始の曜日。
    days: Vec<u32>,
    until: Option<NaiveDate>,
    count: Option<u32>,
}

fn rule(value: &str) -> Rule {
    let mut r = Rule { every: 1, ..Default::default() };
    for part in value.split(';') {
        let Some((k, v)) = part.split_once('=') else { continue };
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => r.freq = v.to_ascii_uppercase(),
            "INTERVAL" => r.every = v.parse().unwrap_or(1).max(1),
            "COUNT" => r.count = v.parse().ok(),
            "UNTIL" => r.until = stamp(v, v.ends_with('Z')).map(|(d, _)| d),
            "BYDAY" => {
                for d in v.split(',') {
                    // `2MO` のような形もあるので、最後の二文字だけ見る。
                    let tail: String = d.chars().rev().take(2).collect::<Vec<_>>()
                        .into_iter().rev().collect();
                    let n = match tail.to_ascii_uppercase().as_str() {
                        "MO" => 0, "TU" => 1, "WE" => 2, "TH" => 3,
                        "FR" => 4, "SA" => 5, "SU" => 6,
                        _ => continue,
                    };
                    r.days.push(n);
                }
            }
            _ => {}
        }
    }
    r
}

/// 繰り返しが、この幅のどの日に落ちるか。
///
/// **数え上げに上限を置く。** 壊れた `RRULE`（`UNTIL` も `COUNT` も無い
/// 毎日、のような）を渡されても、ここで止まる。
fn repeats(start: NaiveDate, r: &Rule, from: NaiveDate, to: NaiveDate) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let mut seen = 0u32;
    let mut d = start;
    let mut steps = 0;
    while d <= to && steps < 4000 {
        steps += 1;
        if let Some(n) = r.count {
            if seen >= n {
                break;
            }
        }
        if let Some(u) = r.until {
            if d > u {
                break;
            }
        }
        let hit = match r.freq.as_str() {
            "DAILY" => (d - start).num_days() % (r.every as i64) == 0,
            "WEEKLY" => {
                let want = if r.days.is_empty() {
                    vec![start.weekday().num_days_from_monday()]
                } else {
                    r.days.clone()
                };
                // **週の頭からの差で数える。** 年をまたぐと週番号は
                // 巻き戻るので、番号で引き算すると隔週がずれる。
                let head = |x: NaiveDate| {
                    x - chrono::Duration::days(x.weekday().num_days_from_monday() as i64)
                };
                let weeks = (head(d) - head(start)).num_days() / 7;
                weeks.rem_euclid(r.every as i64) == 0
                    && want.contains(&d.weekday().num_days_from_monday())
            }
            "MONTHLY" => {
                let months = (d.year() - start.year()) * 12 + d.month() as i32 - start.month() as i32;
                d.day() == start.day() && months % (r.every as i32) == 0
            }
            "YEARLY" => {
                d.day() == start.day() && d.month() == start.month()
                    && (d.year() - start.year()) % (r.every as i32) == 0
            }
            _ => false,
        };
        if hit {
            seen += 1;
            if d >= from {
                out.push(d);
            }
        }
        let Some(next) = d.succ_opt() else { break };
        d = next;
    }
    out
}

/// ひと月ぶん、日の早い順に。
pub fn month(text: &str, year: i32, want: u32) -> Vec<Event> {
    let (Some(from), Some(to)) = (
        NaiveDate::from_ymd_opt(year, want, 1),
        NaiveDate::from_ymd_opt(
            if want == 12 { year + 1 } else { year },
            if want == 12 { 1 } else { want + 1 },
            1,
        )
        .and_then(|d| d.pred_opt()),
    ) else {
        return Vec::new();
    };

    let mut out: Vec<Event> = Vec::new();
    let mut inside = false;
    let mut title = String::new();
    let mut place = String::new();
    let mut start: Option<(NaiveDate, Option<NaiveTime>)> = None;
    let mut end: Option<NaiveDate> = None;
    let mut rrule: Option<Rule> = None;
    let mut skip: Vec<NaiveDate> = Vec::new();

    for line in lines(text) {
        let upper = line.to_ascii_uppercase();
        if upper.starts_with("BEGIN:VEVENT") {
            inside = true;
            title.clear();
            place.clear();
            start = None;
            end = None;
            rrule = None;
            skip.clear();
            continue;
        }
        if !inside {
            continue;
        }
        if upper.starts_with("END:VEVENT") {
            inside = false;
            let Some((day, at)) = start else { continue };
            let name = if title.is_empty() { "（題なし）".to_string() } else { title.clone() };
            let mut days: Vec<NaiveDate> = Vec::new();
            if let Some(r) = &rrule {
                days = repeats(day, r, from, to);
            } else if day >= from && day <= to {
                days.push(day);
            }
            // **終日で何日かにまたがるものは、その日ぶんぜんぶに置く** ──
            // 出張や休みは、始まった日にだけ出ても役に立たない。
            if at.is_none() && rrule.is_none() {
                if let Some(last) = end {
                    let mut d = day;
                    days.clear();
                    // `DTEND` は「その日を含まない」決まり。
                    while d < last {
                        if d >= from && d <= to {
                            days.push(d);
                        }
                        let Some(next) = d.succ_opt() else { break };
                        d = next;
                    }
                }
            }
            for d in days {
                if skip.contains(&d) {
                    continue;
                }
                out.push(Event { day: d, at, title: name.clone(), place: place.clone() });
            }
            continue;
        }
        let Some((head, value)) = line.split_once(':') else { continue };
        let mut bits = head.split(';');
        let name = bits.next().unwrap_or("").to_ascii_uppercase();
        let args: Vec<&str> = bits.collect();
        match name.as_str() {
            "SUMMARY" => title = unescape(value),
            "LOCATION" => place = unescape(value),
            "DTSTART" => start = stamp(value, value.ends_with('Z')),
            "DTEND" => {
                let date = args.iter().any(|a| a.eq_ignore_ascii_case("VALUE=DATE"));
                if date {
                    end = stamp(value, false).map(|(d, _)| d);
                }
            }
            "RRULE" => rrule = Some(rule(value)),
            "EXDATE" => {
                for one in value.split(',') {
                    if let Some((d, _)) = stamp(one, one.ends_with('Z')) {
                        skip.push(d);
                    }
                }
            }
            _ => {}
        }
    }

    out.sort_by(|a, b| {
        a.day.cmp(&b.day)
            .then(a.at.is_none().cmp(&b.at.is_none()))
            .then(a.at.cmp(&b.at))
            .then(a.title.cmp(&b.title))
    });
    out
}

/// 予定表そのものの名前（`X-WR-CALNAME`）。無ければ空。
pub fn name(text: &str) -> String {
    for line in lines(text) {
        if line.to_ascii_uppercase().starts_with("X-WR-CALNAME") {
            if let Some((_, v)) = line.split_once(':') {
                return unescape(v).trim().to_string();
            }
        }
    }
    String::new()
}

/// 見た目だけでも、予定表かどうか。
pub fn looks_like(text: &str) -> bool {
    text.to_ascii_uppercase().contains("BEGIN:VCALENDAR")
}

#[cfg(test)]
mod tests {
    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nX-WR-CALNAME:家の予定\r\n\
BEGIN:VEVENT\r\nDTSTART;TZID=Asia/Tokyo:20260909T140000\r\nSUMMARY:面談\r\n\
LOCATION:会議室 A\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20260921\r\nDTEND;VALUE=DATE:20260924\r\n\
SUMMARY:出張\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nDTSTART;TZID=Asia/Tokyo:20260902T090000\r\nRRULE:FREQ=WEEKLY;BYDAY=WE\r\n\
SUMMARY:朝会\r\nEXDATE;TZID=Asia/Tokyo:20260916T090000\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nDTSTART:20260910T000000Z\r\nSUMMARY:長い題は折り返され\r\n て届く\r\n\
END:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn a_calendar_becomes_days() {
        let got = super::month(SAMPLE, 2026, 9);
        assert_eq!(super::name(SAMPLE), "家の予定");
        assert!(super::looks_like(SAMPLE));

        let one = got.iter().find(|e| e.title == "面談").expect("面談");
        assert_eq!(one.day.to_string(), "2026-09-09");
        assert_eq!(one.at.unwrap().to_string(), "14:00:00");
        assert_eq!(one.place, "会議室 A");

        // 終日でまたがるものは、その日ぶんぜんぶに（`DTEND` は含まない）。
        let trip: Vec<&super::Event> = got.iter().filter(|e| e.title == "出張").collect();
        assert_eq!(trip.len(), 3, "21・22・23 の三日");
        assert!(trip.iter().all(|e| e.at.is_none()));
        assert_eq!(trip[0].day.to_string(), "2026-09-21");
        assert_eq!(trip[2].day.to_string(), "2026-09-23");

        // 毎週水曜は 2・9・16・23・30 だが、16 は EXDATE で抜く。
        let am: Vec<String> = got.iter().filter(|e| e.title == "朝会")
            .map(|e| e.day.to_string()).collect();
        assert_eq!(am, vec!["2026-09-02", "2026-09-09", "2026-09-23", "2026-09-30"]);

        // 折り返された題が繋がっている。
        assert!(got.iter().any(|e| e.title == "長い題は折り返されて届く"),
            "{:?}", got.iter().map(|e| &e.title).collect::<Vec<_>>());
    }

    /// 隔週は、年をまたいでもずれない ── 週番号で引き算していたときは
    /// 一月にずれた。
    #[test]
    fn every_other_week_survives_the_new_year() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20251217T090000Z\n\
RRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=WE\nSUMMARY:隔週\nEND:VEVENT\nEND:VCALENDAR\n";
        let got: Vec<String> = super::month(text, 2026, 1).iter()
            .map(|e| e.day.to_string()).collect();
        // 2025-12-17 から二週ごと: 12-31・01-14・01-28
        assert_eq!(got, vec!["2026-01-14", "2026-01-28"], "{got:?}");
    }

    /// **壊れた繰り返しで止まらない。** `UNTIL` も `COUNT` も無い毎日を
    /// 渡されても、数え上げは月の終わりで終わる。
    #[test]
    fn a_runaway_rule_still_ends() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nDTSTART:20200101T090000Z\n\
RRULE:FREQ=DAILY\nSUMMARY:毎日\nEND:VEVENT\nEND:VCALENDAR\n";
        let got = super::month(text, 2026, 9);
        assert_eq!(got.len(), 30, "九月ぶんだけ");
    }
}
