//! **ひと月ぶんの予定**（依頼 453）。
//!
//! カレンダーの画面が要るのは「この月の、どの日に、何があるか」だけ。
//! それを一度の呼び出しで返す。
//!
//! # いま乗るのは、amber が既に知っているものだけ
//!
//! 前書きの `remind:`（一度きり）と `repeat:`（繰り返し）だけ。
//! **語彙を増やさない** ──「ノートに日付を書くと予定になる」が既にあるので、
//! カレンダーはそれを見せる場所になる。よそのカレンダー（Outlook など）は
//! ここに**足す**形で入る予定で、この関数の返す形は変えない。
//!
//! # ノートも一緒に返す
//!
//! 予定表とノートが別のアプリでは作れないものを作るのが、この画面の
//! 値打ち ── その日に書いたノートが、予定の隣に並ぶ。

use chrono::Datelike;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// 一度読んだノートの、憶えておく形。
///
/// **月を替えるたびに全部読み直さない**（依頼 470）── 二万本で測ると
/// 一回 415 ミリ秒かかっていて、押すたびにそれが乗る。ファイルの日付と
/// 大きさが変わっていなければ、前に読んだものをそのまま使う。
///
/// **ディスクには書かない。** `.amber/` はフォルダと一緒に旅をするので、
/// そこに索引を置くと、別の端末で古い索引を読むことになる。エンジンが
/// 生きているあいだだけ、頭の中に持つ。
#[derive(Clone)]
struct Known {
    when: u64,
    size: u64,
    title: String,
    created: Option<u64>,
    plan: crate::note::Remind,
}

fn seen() -> &'static Mutex<HashMap<std::path::PathBuf, Known>> {
    static SEEN: OnceLock<Mutex<HashMap<std::path::PathBuf, Known>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(HashMap::new()))
}

/// ファイルの「変わったかどうか」を見るしるし。
///
/// **秒ではなく、もっと細かく見る。** 秒までしか見ないと、同じ一秒の
/// あいだに書き替えて長さも変わらなかったノートを、変わっていないものと
/// 思い込む ── 一字だけ直した題が、月の表に古いまま出ることになる。
fn mark(at: &std::path::Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(at).ok()?;
    let when = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    Some((when, m.len()))
}

/// **書かれたばかりのものは、信用しない**（依頼 552）。
///
/// しるしは「更新時刻と長さ」だが、**更新時刻はどこでも細かいわけではない**:
///
/// - NTFS の時刻はシステム時計の刻み（約 15ms）でしか進まない
/// - HFS+ は一秒、FAT は二秒まで
///
/// なので、同じ刻みのあいだに**同じ長さで**書き替えると、しるしが一字も
/// 変わらず、直した題が古いまま出続ける（CI の Windows で実際に出た）。
///
/// 見分ける方法が無いので、**新しすぎるタイムスタンプは初めから信じない** ── git が
/// 「racily clean」と呼んで同じことをしている。読み直すのは「たったいま
/// 書かれた数本」だけなので、憶えておく甲斐（二万本で 415ms → 58ms）は残る。
const GRACE: u64 = 2_000_000_000;

fn now_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// その日にあるもの、一つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub day: chrono::NaiveDate,
    /// 時刻。終日のものと、日付だけのノートは `None`。
    pub at: Option<chrono::NaiveTime>,
    pub title: String,
    /// もとのノート（押したら開く先）。
    pub path: String,
    pub kind: Kind,
}

/// 何としてそこに居るか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `remind: 2026-09-10 09:00` ── 一度きり。
    Once,
    /// `repeat: weekly wed 09:00` ── 繰り返しの、その月にあたるぶん。
    Repeat,
    /// その日に書いた（`created`）ノート。
    Note,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Once => "once",
            Kind::Repeat => "repeat",
            Kind::Note => "note",
        }
    }
}

fn first_of(year: i32, month: u32) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::from_ymd_opt(year, month, 1)
}

fn last_of(year: i32, month: u32) -> Option<chrono::NaiveDate> {
    let (y, m) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    chrono::NaiveDate::from_ymd_opt(y, m, 1)?.pred_opt()
}

/// 秒を、端末のローカル日付に。
fn local_day(secs: u64) -> Option<chrono::NaiveDate> {
    use chrono::TimeZone;
    chrono::Local.timestamp_opt(secs as i64, 0).single().map(|t| t.date_naive())
}

/// 繰り返しが、この月のどの日に落ちるか。
///
/// **`due_since` は借りない。** あちらは「留守の間に過ぎたぶん」を数える
/// もので、上限も `last:` も要る ── こちらが訊きたいのは「未来も含めて、
/// この月のどこか」なので、別の問い。
fn lands(every: crate::note::Every, from: chrono::NaiveDate, to: chrono::NaiveDate)
    -> Vec<chrono::NaiveDate>
{
    let mut out = Vec::new();
    let mut d = from;
    while d <= to {
        let hit = match every {
            crate::note::Every::Daily => true,
            crate::note::Every::Weekly(w) => d.weekday().num_days_from_monday() == w,
            // 三十一日は、短い月では最後の日へ寄せる（`due_on` と同じ答え）。
            crate::note::Every::Monthly(day) => {
                d.day() == day.min(last_of(d.year(), d.month()).map(|l| l.day()).unwrap_or(28))
            }
        };
        if hit {
            out.push(d);
        }
        let Some(next) = d.succ_opt() else { break };
        d = next;
    }
    out
}

/// ひと月ぶん、日の早い順に。
pub fn of(rows: &[crate::survey::Row], year: i32, month: u32) -> Vec<Slot> {
    let (Some(from), Some(to)) = (first_of(year, month), last_of(year, month)) else {
        return Vec::new();
    };
    let mut out: Vec<Slot> = Vec::new();
    // **一度だけ時計を見る**（依頼 552）── 一本ごとに見ると、数えている
    // あいだに時が進んで、同じ一回の中で信じる／信じないが分かれる。
    let here = now_nanos();

    for r in rows {
        if r.is_dir {
            continue;
        }
        let name = r.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let low = name.to_lowercase();
        if !(low.ends_with(".md") || low.ends_with(".markdown")) {
            continue;
        }
        // **変わっていなければ、読み直さない**（依頼 470）。
        let now = mark(&r.path);
        let mut store = seen().lock().ok();
        let cached = store.as_ref().and_then(|m| m.get(&r.path)).cloned().filter(|k| {
            // **新しすぎるしるしは信じない**（依頼 552）── 粗い時計の上では、
            // 同じ刻みのあいだの書き替えを見分けられない。
            now.is_some_and(|(w, z)| k.when == w && k.size == z && here.saturating_sub(w) >= GRACE)
        });
        let known = match cached {
            Some(k) => k,
            None => {
                // **一度だけ読む。** 題も予定も、同じ頭の行から読む ── 前は
                // `read` で一度、`read_to_string` でもう一度、まるごと読んで
                // いた。
                let Some(lines) = crate::note::head(&r.path, 60) else { continue };
                let Some(note) = crate::note::from_head(&r.path, &lines) else { continue };
                let k = Known {
                    when: now.map(|(w, _)| w).unwrap_or(0),
                    size: now.map(|(_, z)| z).unwrap_or(0),
                    title: if note.title.is_empty() {
                        name.trim_end_matches(".md").to_string()
                    } else {
                        note.title.clone()
                    },
                    created: note.created,
                    plan: crate::note::remind_lines(&lines),
                };
                if let Some(m) = store.as_mut() {
                    // 際限なく溜めない ── 途方もない数のノートを歩いた
                    // ときは、いったん捨てて憶え直す。
                    if m.len() > 50_000 {
                        m.clear();
                    }
                    m.insert(r.path.clone(), k.clone());
                }
                k
            }
        };
        drop(store);
        let path = r.path.to_string_lossy().into_owned();
        let title = known.title.clone();

        // 書いた日 ── 予定ではないが、その日に何をしていたかが分かる。
        // **端末のローカル日付で**（UTC ではなく）── 日本の朝に作った
        // ノートが前の日に並ぶのは、見た人には理由が分からない。
        if let Some(d) = known.created.and_then(local_day) {
            if d >= from && d <= to {
                out.push(Slot { day: d, at: None, title: title.clone(),
                                path: path.clone(), kind: Kind::Note });
            }
        }

        let plan = &known.plan;
        if let Some(once) = plan.once {
            let d = once.date();
            if d >= from && d <= to {
                out.push(Slot { day: d, at: Some(once.time()), title: title.clone(),
                                path: path.clone(), kind: Kind::Once });
            }
        }
        if let Some((every, h, m)) = plan.every {
            let at = chrono::NaiveTime::from_hms_opt(h, m, 0);
            for d in lands(every, from, to) {
                out.push(Slot { day: d, at, title: title.clone(),
                                path: path.clone(), kind: Kind::Repeat });
            }
        }
    }

    // 日の順、同じ日なら時刻の早い順、時刻の無いものは後ろ。
    out.sort_by(|a, b| {
        a.day.cmp(&b.day)
            .then(a.at.is_none().cmp(&b.at.is_none()))
            .then(a.at.cmp(&b.at))
            .then(a.title.cmp(&b.title))
    });
    out
}

#[cfg(test)]
mod tests {
    fn walk(root: &std::path::Path) -> Vec<crate::survey::Row> {
        let stop = std::sync::atomic::AtomicBool::new(false);
        let limits = crate::survey::Limits { depth: 4, rows: 200, hidden: false, ..Default::default() };
        crate::survey::survey(root, limits, &stop).rows
    }

    #[test]
    fn a_month_holds_the_once_the_repeats_and_the_notes() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let put = |name: &str, body: &str| std::fs::write(root.join(name), body).unwrap();

        put("面談.md", "---\ntitle: 面談\ncreated: 2026-09-09\nremind: 2026-09-09 14:00\n---\n\n# 面談\n");
        put("週報.md", "---\ntitle: 週報\ncreated: 2026-08-01\nrepeat: weekly wed 09:00\n---\n\n# 週報\n");
        // 来月のものは出ない。
        put("来月.md", "---\ntitle: 来月\ncreated: 2026-10-02\nremind: 2026-10-02 10:00\n---\n\n# 来月\n");

        let got = super::of(&walk(root), 2026, 9);

        let once: Vec<&super::Slot> = got.iter().filter(|s| s.kind == super::Kind::Once).collect();
        assert_eq!(once.len(), 1, "一度きりは一つだけ");
        assert_eq!(once[0].title, "面談");
        assert_eq!(once[0].day.to_string(), "2026-09-09");
        assert_eq!(once[0].at.unwrap().to_string(), "14:00:00");

        // 2026 年 9 月の水曜は 2・9・16・23・30 の五つ。
        let rep: Vec<&super::Slot> = got.iter().filter(|s| s.kind == super::Kind::Repeat).collect();
        assert_eq!(rep.len(), 5, "毎週水曜は五回: {:?}", rep.iter().map(|s| s.day).collect::<Vec<_>>());
        assert!(rep.iter().all(|s| s.title == "週報"));

        // 書いた日 ── 九月のものだけ。
        let notes: Vec<&super::Slot> = got.iter().filter(|s| s.kind == super::Kind::Note).collect();
        assert_eq!(notes.len(), 1, "九月に作られたノートは一本");
        assert_eq!(notes[0].title, "面談");

        assert!(!got.iter().any(|s| s.title == "来月"), "よその月は出ない");
        // 日の早い順に並んでいる。
        assert!(got.windows(2).all(|w| w[0].day <= w[1].day));
    }

    /// 三十一日の繰り返しは、短い月では**最後の日**に落ちる ──
    /// 「毎月31日」が七か月だけになるのは、静かに間違っているほうが悪い。
    #[test]
    fn the_thirty_first_lands_on_the_last_day_of_a_short_month() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("棚卸し.md"),
            "---\ntitle: 棚卸し\ncreated: 2026-01-01\nrepeat: monthly 31 18:00\n---\n\n# 棚卸し\n",
        ).unwrap();
        let got = super::of(&walk(d.path()), 2026, 9);
        let rep: Vec<&super::Slot> = got.iter().filter(|s| s.kind == super::Kind::Repeat).collect();
        assert_eq!(rep.len(), 1);
        assert_eq!(rep[0].day.to_string(), "2026-09-30", "九月は三十日まで");
    }

    /// 憶えたものが、**書き替えたら古いまま出てこない**（依頼 470）。
    ///
    /// 二万本で測ると、月を替えるたびに 415 ミリ秒かかっていた ── 憶えて
    /// おくと 58 ミリ秒になる。ただし「憶える」を入れた日にいちばん怖いのは
    /// 速さではなく、**直した予定が古い姿で出続けること**なので、そこを見張る。
    /// とりわけ、同じ一秒のあいだに長さも変えずに書き替えた場合。
    #[test]
    fn a_rewritten_note_is_read_again() {
        let d = tempfile::tempdir().unwrap();
        let at = d.path().join("面談.md");
        std::fs::write(
            &at,
            "---\ntitle: 面談あ\ncreated: 2026-09-01\nremind: 2026-09-10 10:00\n---\n\n# 面談あ\n",
        ).unwrap();
        let first = super::of(&walk(d.path()), 2026, 9);
        assert!(first.iter().any(|s| s.title == "面談あ"));

        // 長さも変えず、間も置かずに書き替える。
        std::fs::write(
            &at,
            "---\ntitle: 面談い\ncreated: 2026-09-01\nremind: 2026-09-11 10:00\n---\n\n# 面談い\n",
        ).unwrap();
        let then = super::of(&walk(d.path()), 2026, 9);
        assert!(then.iter().any(|s| s.title == "面談い"), "直した題で出る");
        assert!(!then.iter().any(|s| s.title == "面談あ"), "古い題は残らない");
        let moved = then
            .iter()
            .find(|s| s.title == "面談い" && s.kind == super::Kind::Once)
            .unwrap();
        assert_eq!(moved.day.to_string(), "2026-09-11", "動かした日で出る");
    }

    /// **粗い時計の上でも、直したものが出る**（依頼 552）。
    ///
    /// NTFS の時刻はシステム時計の刻み（約 15ms）でしか進まず、HFS+ は一秒、
    /// FAT は二秒まで ── 同じ刻みのあいだに**同じ長さで**書き替えると、
    /// 憶えたしるしが一字も変わらない。上の試験は mac の細かい時計に
    /// 寄りかかっていて、CI の Windows でだけ落ちた。
    ///
    /// ここでは**時刻を手で戻して**、その状況をどの環境でも再現する。
    #[test]
    fn a_note_rewritten_under_a_coarse_clock_is_read_again() {
        let d = tempfile::tempdir().unwrap();
        let at = d.path().join("面談.md");
        std::fs::write(
            &at,
            "---\ntitle: 面談あ\ncreated: 2026-09-01\nremind: 2026-09-10 10:00\n---\n\n# 面談あ\n",
        ).unwrap();
        let stamp = std::fs::metadata(&at).unwrap().modified().unwrap();
        let first = super::of(&walk(d.path()), 2026, 9);
        assert!(first.iter().any(|s| s.title == "面談あ"));

        // 長さも変えずに書き替えて、**時刻を書き替える前に戻す**。
        std::fs::write(
            &at,
            "---\ntitle: 面談い\ncreated: 2026-09-01\nremind: 2026-09-10 10:00\n---\n\n# 面談い\n",
        ).unwrap();
        let f = std::fs::File::options().write(true).open(&at).unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(stamp)).unwrap();
        drop(f);
        assert_eq!(
            std::fs::metadata(&at).unwrap().modified().unwrap(),
            stamp,
            "しるしは書き替える前と同じ（粗い時計と同じ状況）"
        );

        let then = super::of(&walk(d.path()), 2026, 9);
        assert!(then.iter().any(|s| s.title == "面談い"), "直した題で出る");
        assert!(!then.iter().any(|s| s.title == "面談あ"), "古い題は残らない");
    }
}
