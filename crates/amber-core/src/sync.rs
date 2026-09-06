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
}

impl Step {
    pub fn rel(&self) -> &str {
        match self {
            Step::Up { rel, .. }
            | Step::Down { rel, .. }
            | Step::DropHere { rel }
            | Step::DropThere { rel, .. }
            | Step::Clash { rel, .. } => rel,
        }
    }

    pub fn word(&self) -> &'static str {
        match self {
            Step::Up { .. } => "up",
            Step::Down { .. } => "down",
            Step::DropHere { .. } => "drophere",
            Step::DropThere { .. } => "dropthere",
            Step::Clash { .. } => "clash",
        }
    }
}

/// 手順を組む。
///
/// 見るのは三つ ── いまこちらにあるもの、いま向こうにあるもの、前に合わせた
/// ときの姿。**時刻はどこにも出てこない**（時刻で比べると、時計のずれた
/// 端末が毎回勝つか毎回負ける）。
pub fn plan(here: &[Here], there: &[There], was: &[Was]) -> Vec<Step> {
    use std::collections::BTreeMap;
    let h: BTreeMap<&str, &Here> = here.iter().map(|x| (x.rel.as_str(), x)).collect();
    let t: BTreeMap<&str, &There> = there.iter().map(|x| (x.rel.as_str(), x)).collect();
    let w: BTreeMap<&str, &Was> = was.iter().map(|x| (x.rel.as_str(), x)).collect();

    let mut names: Vec<&str> = h.keys().chain(t.keys()).chain(w.keys()).copied().collect();
    names.sort();
    names.dedup();

    let mut out = Vec::new();
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

/* ── 前に合わせたときの姿を、憶えておく ── */

/// 憶えの置き場所。**ノートの隣ではなく `.amber` の中** ── これは amber の
/// 都合であって、ノートの中身ではない。
pub fn ledger(root: &std::path::Path) -> std::path::PathBuf {
    root.join(".cian").join("sync.json")
}

/// 相手ごとの憶え。`who` は `drive` など ── **一つに決め打たない**。
/// いつか二つ目の相手が来たときに、片方の憶えがもう片方を上書きしない。
pub fn recall(root: &std::path::Path, who: &str) -> Vec<Was> {
    let Ok(text) = std::fs::read_to_string(ledger(root)) else { return Vec::new() };
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
    let at = ledger(root);
    let mut v: serde_json::Value = std::fs::read_to_string(&at)
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
        std::fs::create_dir_all(d.path().join(".cian")).unwrap();
        std::fs::write(ledger(d.path()), "{ こわれている").unwrap();
        assert!(recall(d.path(), "drive").is_empty());
        // 書き直せる（壊れた字を持ち越さない）。
        let one = vec![Was { rel: "a.md".into(), hash: "1".into(), id: "i".into(), tag: "x".into() }];
        remember(d.path(), "drive", &one, &[]).unwrap();
        assert_eq!(recall(d.path(), "drive"), one);
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
