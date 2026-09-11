//! こちらと向こうを、どう合わせるか。**決めるだけで、繋がない。**
//!
//! `amber-core` の規則は「判断だけ。I/O と UI に依存しない」── 通信は I/O
//! なので、ここには入れない。ここが出すのは**手順書**で、それを実際に
//! やるのは窓（Node）と電話（URLSession）。
//!
//! そうしてあるのは、**判断を一組にするため**。同じ「どちらが新しいか」を
//! 二つの土台で書けば、いつか片方だけが違う答えを出す ── 失うのはノートで、
//! 気づくのは何日か経ってから。ここは繋がないので、通信なしで全部試験できる。
//!
//! # 迷ったら残す
//!
//! 消すのは取り返しがつかず、残すのはつかない。**片方で消して、もう片方で
//! 書き足したとき、amber は書き足したほうを残す** ── 消したかったものが
//! 一本残るのは、書いたものが黙って消えるより、ずっとましだから。

/// こちらにある一本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Here {
    /// ルートからの道（`家族/買い物リスト.md`）。
    pub rel: String,
    /// 中身の指紋。**時刻では比べない** ── クラウドから降りてきたファイルの
    /// 時刻は、書いた時刻とは限らない。
    pub hash: String,
}

/// 向こうにある一本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct There {
    pub rel: String,
    /// 向こうでの合言葉（Drive の file id など）。
    pub id: String,
    /// 向こうの版。変わったかどうかだけ見る（中身は問わない）。
    pub tag: String,
}

/// 前に合わせたときの姿。**これが「分かれる前」** ── 三方向マージの土台にも
/// なる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Was {
    pub rel: String,
    pub hash: String,
    pub id: String,
    pub tag: String,
}

/// やること一つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// こちらのを、向こうへ。`id` が無ければ新しく作る。
    Up { rel: String, id: Option<String> },
    /// 向こうのを、こちらへ。
    Down { rel: String, id: String },
    /// 向こうで消えたので、こちらからも消す。
    DropHere { rel: String },
    /// こちらで消したので、向こうからも消す。
    DropThere { rel: String, id: String },
    /// 両方が変わった。**混ぜる**（混ぜ方は `merge` の仕事）。
    Clash { rel: String, id: String },
    /// こちらで名前が変わった（依頼 492）── 向こうも改名する。**削除＋新規に
    /// しない**（Drive の ID は同じまま・履歴も向こうの版も繋がったまま）。
    MoveThere { from: String, to: String, id: String },
    /// 向こうで名前が変わった ── こちらも改名する。
    MoveHere { from: String, to: String, id: String },
}

impl Step {
    pub fn rel(&self) -> &str {
        match self {
            Step::Up { rel, .. }
            | Step::Down { rel, .. }
            | Step::DropHere { rel }
            | Step::DropThere { rel, .. }
            | Step::Clash { rel, .. } => rel,
            Step::MoveThere { to, .. } | Step::MoveHere { to, .. } => to,
        }
    }

    pub fn word(&self) -> &'static str {
        match self {
            Step::Up { .. } => "up",
            Step::Down { .. } => "down",
            Step::DropHere { .. } => "drophere",
            Step::DropThere { .. } => "dropthere",
            Step::Clash { .. } => "clash",
            Step::MoveThere { .. } => "movethere",
            Step::MoveHere { .. } => "movehere",
        }
    }
}

/// こちらで改名して、**まだ向こうに伝えていない**もの（いまの道, 向こうがまだ持つ道）。
pub type Move = (String, String);

/// 手順を組む。
///
/// 見るのは三つ ── いまこちらにあるもの、いま向こうにあるもの、前に合わせた
/// ときの姿。**時刻はどこにも出てこない**（時刻で比べると、時計のずれた
/// 端末が毎回勝つか毎回負ける）。
pub fn plan(here: &[Here], there: &[There], was: &[Was]) -> Vec<Step> {
    plan_with_moves(here, there, was, &[])
}

/// 手順を組む ── **改名も込みで**（依頼 492）。
///
/// 名前で突き合わせる前に、**ID で改名を見つける**。向こうの一本は ID で
/// 同じままなので、こちらで改名したもの（`moves`）は向こうを改名し、向こうで
/// 改名されたもの（憶えと違う道に同じ ID がある）はこちらを改名する。両方で
/// 別の名前に変えていたら**向こうの名前に従う** ── 題そのものは中身の混ぜで
/// 決まり、ファイル名は題に合わせて後から揃うので、ここで争わない。
pub fn plan_with_moves(here: &[Here], there: &[There], was: &[Was], moves: &[Move]) -> Vec<Step> {
    use std::collections::BTreeMap;
    let mut here: Vec<Here> = here.to_vec();
    let mut there: Vec<There> = there.to_vec();
    let mut was: Vec<Was> = was.to_vec();
    let mut out = Vec::new();

    // ── こちらの改名（憶えはもう新しい道・向こうはまだ古い道）──
    for (now, old) in moves {
        let Some(w) = was.iter().find(|w| &w.rel == now) else { continue };
        let id = w.id.clone();
        let Some(t) = there.iter_mut().find(|t| t.id == id) else { continue };
        if &t.rel == old {
            out.push(Step::MoveThere { from: old.clone(), to: now.clone(), id: id.clone() });
            t.rel = now.clone();
        } else if &t.rel != now {
            // 向こうも別の名前に変えていた ── 向こうに従う。
            let to = t.rel.clone();
            out.push(Step::MoveHere { from: now.clone(), to: to.clone(), id: id.clone() });
            rename_local(&mut here, &mut was, now, &to);
        }
    }

    // ── 向こうの改名（同じ ID が、憶えと違う道にある）──
    let mut skip: Vec<String> = Vec::new();
    let mut skip_rels: Vec<String> = Vec::new();
    for w in was.clone() {
        let Some(t) = there.iter().find(|t| t.id == w.id) else { continue };
        if t.rel == w.rel {
            continue;
        }
        let to = t.rel.clone();
        out.push(Step::MoveHere { from: w.rel.clone(), to: to.clone(), id: w.id.clone() });
        // こちらにその名前の別のノートがある ── 改名は番号付きになるので、
        // この回は名前を合わせるだけにして、中身はその次に運ぶ。
        let taken = here.iter().any(|h| h.rel == to) && !was.iter().any(|x| x.rel == to && x.id == w.id);
        if taken {
            skip.push(w.id.clone());
            skip_rels.push(w.rel.clone());
        } else {
            rename_local(&mut here, &mut was, &w.rel, &to);
        }
    }
    if !skip.is_empty() {
        there.retain(|t| !skip.contains(&t.id));
        was.retain(|w| !skip.contains(&w.id));
        here.retain(|h| !skip_rels.contains(&h.rel));
    }

    let h: BTreeMap<&str, &Here> = here.iter().map(|x| (x.rel.as_str(), x)).collect();
    let t: BTreeMap<&str, &There> = there.iter().map(|x| (x.rel.as_str(), x)).collect();
    let w: BTreeMap<&str, &Was> = was.iter().map(|x| (x.rel.as_str(), x)).collect();

    let mut names: Vec<&str> = h.keys().chain(t.keys()).chain(w.keys()).copied().collect();
    names.sort();
    names.dedup();

    for rel in names {
        let (a, b, c) = (h.get(rel), t.get(rel), w.get(rel));
        let step = match (a, b, c) {
            // 前に合わせたことがない。
            (Some(_), None, None) => Some(Step::Up { rel: rel.into(), id: None }),
            (None, Some(t), None) => Some(Step::Down { rel: rel.into(), id: t.id.clone() }),
            // 両方に新しく現れた ── 同じ名前で別々に作られた。**混ぜる**
            // （どちらかを捨てる理由が無い）。
            (Some(a), Some(t), None) => {
                if a.hash == t.tag {
                    None
                } else {
                    Some(Step::Clash { rel: rel.into(), id: t.id.clone() })
                }
            }
            // 前はあったが、いまはどちらにも無い ── 憶えを消すだけ。
            (None, None, Some(_)) => None,

            (Some(a), None, Some(w)) => {
                if a.hash == w.hash {
                    // こちらは触っていない。向こうで消された ── 従う。
                    Some(Step::DropHere { rel: rel.into() })
                } else {
                    // **こちらで書き足した。向こうで消された。** 書いたほうを
                    // 残す ── 消したかったものが一本残るのは、書いたものが
                    // 黙って消えるより、ずっとまし。
                    Some(Step::Up { rel: rel.into(), id: None })
                }
            }
            (None, Some(t), Some(w)) => {
                if t.tag == w.tag {
                    // 向こうは触っていない。こちらで消した ── 従う。
                    Some(Step::DropThere { rel: rel.into(), id: t.id.clone() })
                } else {
                    // 向こうで書き足された。こちらで消した ── 書いたほうを残す。
                    Some(Step::Down { rel: rel.into(), id: t.id.clone() })
                }
            }
            (Some(a), Some(t), Some(w)) => {
                match (a.hash != w.hash, t.tag != w.tag) {
                    (false, false) => None,
                    (true, false) => Some(Step::Up { rel: rel.into(), id: Some(t.id.clone()) }),
                    (false, true) => Some(Step::Down { rel: rel.into(), id: t.id.clone() }),
                    (true, true) => Some(Step::Clash { rel: rel.into(), id: t.id.clone() }),
                }
            }
            (None, None, None) => None,
        };
        if let Some(s) = step {
            out.push(s);
        }
    }
    out
}

/// 手順を組むあいだだけ、こちらの一本と憶えを新しい道で呼ぶ。
fn rename_local(here: &mut [Here], was: &mut [Was], from: &str, to: &str) {
    for h in here.iter_mut() {
        if h.rel == from {
            h.rel = to.to_string();
        }
    }
    for w in was.iter_mut() {
        if w.rel == from {
            w.rel = to.to_string();
        }
    }
}

/// **絵も運ぶ**（依頼 497）── `attachments/` の中の絵を、ノートと同じ手順書に乗せる。
///
/// 道は `仕事/attachments/段取り-123.png` のように、ノートと同じルートからの道。
/// 中身は字ではないので混ぜられない ── 両方が変わったら、こちらを残して向こうの
/// ものは `名前.2.png` として隣に置く（失うよりよい）。それは呼ぶ側の仕事。
pub fn assets(root: &std::path::Path) -> Vec<Here> {
    let mut out = Vec::new();
    let mut dirs = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = e.path();
            if !path.is_dir() {
                continue;
            }
            if name == "attachments" {
                let Ok(pics) = std::fs::read_dir(&path) else { continue };
                for pic in pics.flatten() {
                    let n = pic.file_name().to_string_lossy().into_owned();
                    if n.starts_with('.') || !crate::spare::is_picture(&n) || !pic.path().is_file() {
                        continue;
                    }
                    let at = pic.path();
                    let Ok(rel) = at.strip_prefix(root) else { continue };
                    let bytes = std::fs::read(&at).unwrap_or_default();
                    out.push(Here { rel: rel.to_string_lossy().replace('\\', "/"), hash: fingerprint(&bytes) });
                }
            } else if depth < 6 {
                dirs.push((path, depth + 1));
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// 絵の道か（同じ手順書の中で、字として読まないもの）。
pub fn is_asset(rel: &str) -> bool {
    rel.rsplit_once('/').map(|(d, _)| d.ends_with("attachments") || d == "attachments").unwrap_or(false)
        && crate::spare::is_picture(rel.rsplit('/').next().unwrap_or(rel))
}

/* ── 前に合わせたときの姿を、憶えておく ── */

/// 憶えの置き場所。**ノートの隣ではなく `.amber` の中** ── これは amber の
/// 都合であって、ノートの中身ではない。
pub fn ledger(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".amber").join("sync.json")
}

/// 前の置き場所。**読むときだけ見る**（`notebook::old_file` と同じ理由 ──
/// 隠しフォルダは一つにする）。
fn old_ledger(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".cian").join("sync.json")
}

/// いま読むべき憶え。**`.cian` に居るなら、そちらが本物** ── 今日まで
/// 書いていたのはそこ。次に憶え直した時点で `.amber` へ移る。
fn ledger_now(root: &std::path::Path) -> std::path::PathBuf {
    let old = old_ledger(root);
    if old.exists() { old } else { ledger(root) }
}

/// 相手ごとの憶え。`who` は `drive` など ── **一つに決め打たない**。
/// いつか二つ目の相手が来たときに、片方の憶えがもう片方を上書きしない。
pub fn recall(root: &std::path::Path, who: &str) -> Vec<Was> {
    let Ok(text) = std::fs::read_to_string(ledger_now(root)) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Vec::new() };
    let Some(files) = v.get(who).and_then(|w| w.get("files")).and_then(|f| f.as_object()) else {
        return Vec::new();
    };
    let mut out: Vec<Was> = files
        .iter()
        .filter_map(|(rel, f)| {
            Some(Was {
                rel: rel.clone(),
                hash: f.get("hash")?.as_str()?.to_string(),
                id: f.get("id")?.as_str()?.to_string(),
                tag: f.get("tag")?.as_str()?.to_string(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

/// 憶え直す。**運び終わったぶんだけ。**
///
/// 途中で切れたら、運べたぶんだけが憶えに残る ── 次に合わせたときに、
/// 残りをもう一度運ぶ。**全部やるか何もしないか、にしない**のは、電波の
/// 悪いところで一本も進まなくなるから。
pub fn remember(root: &std::path::Path, who: &str, done: &[Was], gone: &[String])
    -> anyhow::Result<()>
{
    // 前の隠しフォルダに憶えが残っているなら、書く前に引き取る ── 移す
    // 場所を二か所に書かないため、片付けは `notebook::tidy` に一つ。
    crate::notebook::tidy(root);
    let at = ledger(root);
    let mut v: serde_json::Value = std::fs::read_to_string(ledger_now(root))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !v.is_object() {
        v = serde_json::json!({});
    }
    let files = v
        .as_object_mut()
        .unwrap()
        .entry(who.to_string())
        .or_insert_with(|| serde_json::json!({ "files": {} }))
        .as_object_mut()
        .unwrap()
        .entry("files".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !files.is_object() {
        *files = serde_json::json!({});
    }
    let m = files.as_object_mut().unwrap();
    for d in done {
        m.insert(
            d.rel.clone(),
            serde_json::json!({ "hash": d.hash, "id": d.id, "tag": d.tag }),
        );
    }
    for g in gone {
        m.remove(g);
    }
    if let Some(dir) = at.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(at, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

/// こちらで改名した（依頼 492）── 憶えの鍵を新しい道へ。`record` なら
/// 「まだ向こうに伝えていない改名」として残す（向こうの改名を写すときは残さない）。
///
/// 相手ごとの憶えぜんぶに効く。憶えに無い一本（まだ一度も合わせていない）は
/// 何も書かない ── 向こうには無いので、伝える改名も無い。
pub fn moved(root: &std::path::Path, from: &str, to: &str, record: bool) {
    let at = ledger(root);
    let Some(mut v) = std::fs::read_to_string(ledger_now(root))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    else { return };
    let Some(whos) = v.as_object_mut() else { return };
    let mut touched = false;
    for (_, who) in whos.iter_mut() {
        let Some(who) = who.as_object_mut() else { continue };
        let had = who
            .get_mut("files")
            .and_then(|f| f.as_object_mut())
            .and_then(|f| f.remove(from));
        let Some(entry) = had else { continue };
        if let Some(f) = who.get_mut("files").and_then(|f| f.as_object_mut()) {
            f.insert(to.to_string(), entry);
        }
        touched = true;
        let m = who
            .entry("moves".to_string())
            .or_insert_with(|| serde_json::json!({}));
        if !m.is_object() {
            *m = serde_json::json!({});
        }
        let m = m.as_object_mut().unwrap();
        // 続けて改名したら、向こうがまだ持つ道は最初のもの。
        let origin = m
            .remove(from)
            .and_then(|o| o.as_str().map(str::to_string))
            .unwrap_or_else(|| from.to_string());
        if record && origin != to {
            m.insert(to.to_string(), serde_json::Value::String(origin));
        }
    }
    if touched {
        crate::notebook::tidy(root);
        if let Some(dir) = at.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&v) {
            let _ = std::fs::write(at, text);
        }
    }
}

/// まだ向こうに伝えていない改名（いまの道, 向こうがまだ持つ道）。
pub fn moves(root: &std::path::Path, who: &str) -> Vec<Move> {
    let Ok(text) = std::fs::read_to_string(ledger_now(root)) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Vec::new() };
    let Some(m) = v.get(who).and_then(|w| w.get("moves")).and_then(|m| m.as_object()) else {
        return Vec::new();
    };
    let mut out: Vec<Move> = m
        .iter()
        .filter_map(|(now, old)| Some((now.clone(), old.as_str()?.to_string())))
        .collect();
    out.sort();
    out
}

/// 伝え終わった改名を忘れる。
pub fn forget_moves(root: &std::path::Path, who: &str, done: &[String]) {
    if done.is_empty() {
        return;
    }
    let at = ledger(root);
    let Some(mut v) = std::fs::read_to_string(ledger_now(root))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    else { return };
    if let Some(m) = v.get_mut(who).and_then(|w| w.get_mut("moves")).and_then(|m| m.as_object_mut()) {
        for d in done {
            m.remove(d);
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(&v) {
        let _ = std::fs::write(at, text);
    }
}

/// 中身の指紋。**時刻では比べない。**
///
/// クラウドから降りてきたファイルの時刻は、書いた時刻とは限らない ── 時計の
/// ずれた端末が毎回勝つか毎回負ける。中身そのものを見れば、そこは揺れない。
///
/// 暗号の強さは要らない（守るのではなく、変わったかを見るだけ）ので、
/// **依存を増やさずに書ける FNV-1a** で足りる。同じ二本が違う指紋になること
/// は無く、違う二本が同じになるのは 1800京分の1。
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(rel: &str, hash: &str) -> Here { Here { rel: rel.into(), hash: hash.into() } }
    fn t(rel: &str, tag: &str) -> There {
        There { rel: rel.into(), id: format!("id:{rel}"), tag: tag.into() }
    }
    fn w(rel: &str, hash: &str, tag: &str) -> Was {
        Was { rel: rel.into(), hash: hash.into(), id: format!("id:{rel}"), tag: tag.into() }
    }

    #[test]
    fn 憶えは_相手ごとに分かれている() {
        let d = tempfile::tempdir().unwrap();
        let one = vec![Was { rel: "a.md".into(), hash: "1".into(), id: "i".into(), tag: "x".into() }];
        remember(d.path(), "drive", &one, &[]).unwrap();
        // **一つに決め打たない** ── 二つ目の相手が来ても、片方の憶えが
        // もう片方を上書きしない。
        let two = vec![Was { rel: "b.md".into(), hash: "2".into(), id: "j".into(), tag: "y".into() }];
        remember(d.path(), "webdav", &two, &[]).unwrap();

        assert_eq!(recall(d.path(), "drive"), one);
        assert_eq!(recall(d.path(), "webdav"), two);
        assert!(recall(d.path(), "だれか").is_empty());

        // 消したものは憶えから落ちる。
        remember(d.path(), "drive", &[], &["a.md".to_string()]).unwrap();
        assert!(recall(d.path(), "drive").is_empty());
        assert_eq!(recall(d.path(), "webdav"), two, "隣の憶えは触らない");
    }

    #[test]
    fn 壊れた憶えでも_落ちずに一から合わせる() {
        // **憶えが読めないのは、合わせ直せば済むこと。** ここで落ちると、
        // ノートが一本も見られなくなる。
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".amber")).unwrap();
        std::fs::write(ledger(d.path()), "{ こわれている").unwrap();
        assert!(recall(d.path(), "drive").is_empty());
        // 書き直せる（壊れた字を持ち越さない）。
        let one = vec![Was { rel: "a.md".into(), hash: "1".into(), id: "i".into(), tag: "x".into() }];
        remember(d.path(), "drive", &one, &[]).unwrap();
        assert_eq!(recall(d.path(), "drive"), one);
    }

    #[test]
    fn 前の隠しフォルダの憶えは_引き継がれて片付く() {
        // `.cian/sync.json` に憶えがある棚でも、次の同期が全部を運び直さない。
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".cian")).unwrap();
        let one = vec![Was { rel: "a.md".into(), hash: "1".into(), id: "i".into(), tag: "x".into() }];
        std::fs::write(
            d.path().join(".cian").join("sync.json"),
            r#"{"drive":{"files":{"a.md":{"hash":"1","id":"i","tag":"x"}}}}"#,
        )
        .unwrap();
        assert_eq!(recall(d.path(), "drive"), one);

        let two = vec![Was { rel: "b.md".into(), hash: "2".into(), id: "j".into(), tag: "y".into() }];
        remember(d.path(), "drive", &two, &[]).unwrap();
        assert!(ledger(d.path()).exists(), ".amber に移っていること");
        assert!(!d.path().join(".cian").exists(), "空になった .cian は残さない");
        // **前の憶えを持ち越す** ── 落とすと、次の同期が全部を運び直す。
        let mut both = recall(d.path(), "drive");
        both.sort_by(|a, b| a.rel.cmp(&b.rel));
        assert_eq!(both.len(), 2, "前の一本も残っていること");
    }

    #[test]
    fn 指紋は_中身だけを見る() {
        assert_eq!(fingerprint(b"abc"), fingerprint(b"abc"));
        assert_ne!(fingerprint(b"abc"), fingerprint(b"abd"));
        assert_ne!(fingerprint(b""), fingerprint(b" "));
        assert_eq!(fingerprint(b"").len(), 16);
    }

    #[test]
    fn 触っていないものは_何もしない() {
        let out = plan(&[h("a.md", "1")], &[t("a.md", "x")], &[w("a.md", "1", "x")]);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn 片方だけ変わったら_その向きへ運ぶ() {
        // こちらで書いた。
        let out = plan(&[h("a.md", "2")], &[t("a.md", "x")], &[w("a.md", "1", "x")]);
        assert_eq!(out, vec![Step::Up { rel: "a.md".into(), id: Some("id:a.md".into()) }]);

        // 向こうで書かれた。
        let out = plan(&[h("a.md", "1")], &[t("a.md", "y")], &[w("a.md", "1", "x")]);
        assert_eq!(out, vec![Step::Down { rel: "a.md".into(), id: "id:a.md".into() }]);
    }

    #[test]
    fn 初めての一本は_あるほうから運ぶ() {
        let out = plan(&[h("a.md", "1")], &[], &[]);
        assert_eq!(out, vec![Step::Up { rel: "a.md".into(), id: None }]);

        let out = plan(&[], &[t("b.md", "x")], &[]);
        assert_eq!(out, vec![Step::Down { rel: "b.md".into(), id: "id:b.md".into() }]);
    }

    #[test]
    fn 両方が変わったら_混ぜる() {
        let out = plan(&[h("a.md", "2")], &[t("a.md", "y")], &[w("a.md", "1", "x")]);
        assert_eq!(out, vec![Step::Clash { rel: "a.md".into(), id: "id:a.md".into() }]);

        // 前に合わせたことがなく、同じ名前で別々に作られた ── これも混ぜる
        // （どちらかを捨てる理由が無い）。
        let out = plan(&[h("a.md", "2")], &[t("a.md", "y")], &[]);
        assert_eq!(out, vec![Step::Clash { rel: "a.md".into(), id: "id:a.md".into() }]);
    }

    #[test]
    fn 消したものは_触っていなければ従う() {
        // 向こうで消された。こちらは触っていない。
        let out = plan(&[h("a.md", "1")], &[], &[w("a.md", "1", "x")]);
        assert_eq!(out, vec![Step::DropHere { rel: "a.md".into() }]);

        // こちらで消した。向こうは触っていない。
        let out = plan(&[], &[t("a.md", "x")], &[w("a.md", "1", "x")]);
        assert_eq!(out, vec![Step::DropThere { rel: "a.md".into(), id: "id:a.md".into() }]);
    }

    #[test]
    fn 消すのと書くのがぶつかったら_書いたほうを残す() {
        // **この試験がこの module でいちばん大事。**
        // 消すのは取り返しがつかず、残すのはつかない。

        // 向こうで消された。こちらでは書き足していた ── 残す。
        let out = plan(&[h("a.md", "2")], &[], &[w("a.md", "1", "x")]);
        assert_eq!(out, vec![Step::Up { rel: "a.md".into(), id: None }],
                   "書いたものを、向こうの削除で消してはいけない");

        // こちらで消した。向こうでは書き足されていた ── 残す。
        let out = plan(&[], &[t("a.md", "y")], &[w("a.md", "1", "x")]);
        assert_eq!(out, vec![Step::Down { rel: "a.md".into(), id: "id:a.md".into() }],
                   "あちらが書いたものを、こちらの削除で消してはいけない");
    }

    #[test]
    fn どちらにも無くなったものは_憶えを捨てるだけ() {
        let out = plan(&[], &[], &[w("a.md", "1", "x")]);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn こちらで改名したら_向こうも改名する() {
        // 憶えはもう新しい道、向こうはまだ古い道（`moved` がそうしておく）。
        let was = vec![Was { rel: "旅.md".into(), hash: "1".into(), id: "i".into(), tag: "x".into() }];
        let there = vec![There { rel: "old.md".into(), id: "i".into(), tag: "x".into() }];
        let mv = vec![("旅.md".to_string(), "old.md".to_string())];
        let out = plan_with_moves(&[h("旅.md", "1")], &there, &was, &mv);
        assert_eq!(out, vec![Step::MoveThere { from: "old.md".into(), to: "旅.md".into(), id: "i".into() }]);

        // 改名して、さらに書いた ── 改名のあとに上げる。
        let out = plan_with_moves(&[h("旅.md", "2")], &there, &was, &mv);
        assert_eq!(out, vec![
            Step::MoveThere { from: "old.md".into(), to: "旅.md".into(), id: "i".into() },
            Step::Up { rel: "旅.md".into(), id: Some("i".into()) },
        ]);

        // 改名して、消した ── 向こうも消す（改名は伝えない）。
        let out = plan_with_moves(&[], &there, &was, &mv);
        assert_eq!(out, vec![
            Step::MoveThere { from: "old.md".into(), to: "旅.md".into(), id: "i".into() },
            Step::DropThere { rel: "旅.md".into(), id: "i".into() },
        ]);
    }

    #[test]
    fn 向こうで改名されたら_こちらも改名する() {
        let was = vec![Was { rel: "old.md".into(), hash: "1".into(), id: "i".into(), tag: "x".into() }];
        let there = vec![There { rel: "旅.md".into(), id: "i".into(), tag: "x".into() }];
        let out = plan_with_moves(&[h("old.md", "1")], &there, &was, &[]);
        assert_eq!(out, vec![Step::MoveHere { from: "old.md".into(), to: "旅.md".into(), id: "i".into() }]);

        // 向こうで改名して、さらに書かれた ── 改名のあとに下ろす。
        let there2 = vec![There { rel: "旅.md".into(), id: "i".into(), tag: "y".into() }];
        let out = plan_with_moves(&[h("old.md", "1")], &there2, &was, &[]);
        assert_eq!(out, vec![
            Step::MoveHere { from: "old.md".into(), to: "旅.md".into(), id: "i".into() },
            Step::Down { rel: "旅.md".into(), id: "i".into() },
        ]);

        // **両方で別の名前に変えていたら、向こうに従う**（題は中身の混ぜで決まる）。
        let was2 = vec![Was { rel: "こっち.md".into(), hash: "1".into(), id: "i".into(), tag: "x".into() }];
        let mv = vec![("こっち.md".to_string(), "old.md".to_string())];
        let out = plan_with_moves(&[h("こっち.md", "1")], &there, &was2, &mv);
        assert_eq!(out, vec![Step::MoveHere { from: "こっち.md".into(), to: "旅.md".into(), id: "i".into() }]);

        // こちらにその名前の別のノートがある ── この回は名前だけ、中身は次に。
        let out = plan_with_moves(&[h("old.md", "2"), h("旅.md", "9")], &there2, &was, &[]);
        assert_eq!(out, vec![
            Step::MoveHere { from: "old.md".into(), to: "旅.md".into(), id: "i".into() },
            Step::Up { rel: "旅.md".into(), id: None },
        ]);
    }

    #[test]
    fn 改名の憶えは_続けて改名しても最初の道を持つ() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        remember(r, "drive", &[Was { rel: "a.md".into(), hash: "1".into(), id: "i".into(), tag: "x".into() }], &[]).unwrap();
        moved(r, "a.md", "b.md", true);
        moved(r, "b.md", "c.md", true);
        assert_eq!(moves(r, "drive"), vec![("c.md".to_string(), "a.md".to_string())]);
        assert_eq!(recall(r, "drive")[0].rel, "c.md");
        // 元の名前に戻したら、伝える改名は無い。
        moved(r, "c.md", "a.md", true);
        assert!(moves(r, "drive").is_empty());
        // 向こうの改名を写すときは、伝える改名として残さない。
        moved(r, "a.md", "d.md", false);
        assert!(moves(r, "drive").is_empty());
        assert_eq!(recall(r, "drive")[0].rel, "d.md");
        // 憶えに無い一本は、何も書かない。
        moved(r, "z.md", "y.md", true);
        assert!(moves(r, "drive").is_empty());
        // 伝え終わったら忘れる。
        moved(r, "d.md", "e.md", true);
        forget_moves(r, "drive", &["e.md".to_string()]);
        assert!(moves(r, "drive").is_empty());
    }

    #[test]
    fn 絵も_手順書に乗る() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        std::fs::create_dir_all(r.join("attachments")).unwrap();
        std::fs::create_dir_all(r.join("仕事/attachments")).unwrap();
        std::fs::create_dir_all(r.join(".amber/attachments")).unwrap();
        std::fs::write(r.join("attachments/a-1.png"), [1u8, 2]).unwrap();
        std::fs::write(r.join("attachments/note.txt"), "x").unwrap();
        std::fs::write(r.join("仕事/attachments/b-2.jpg"), [3u8]).unwrap();
        std::fs::write(r.join(".amber/attachments/hidden.png"), [4u8]).unwrap();
        let got = assets(r);
        let rels: Vec<&str> = got.iter().map(|h| h.rel.as_str()).collect();
        assert_eq!(rels, vec!["attachments/a-1.png", "仕事/attachments/b-2.jpg"]);
        assert_eq!(got[0].hash, fingerprint(&[1u8, 2]));
        assert!(is_asset("attachments/a-1.png") && is_asset("仕事/attachments/b.jpg"));
        assert!(!is_asset("attachments.md") && !is_asset("仕事/b.jpg") && !is_asset("attachments/x.txt"));
    }

    #[test]
    fn たくさんあっても_道の順に並ぶ() {
        let out = plan(
            &[h("z.md", "1"), h("a.md", "1")],
            &[t("m.md", "x")],
            &[],
        );
        let names: Vec<&str> = out.iter().map(|s| s.rel()).collect();
        assert_eq!(names, vec!["a.md", "m.md", "z.md"]);
    }
}
