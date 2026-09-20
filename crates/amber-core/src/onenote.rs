//! OneNote を取り込む（依頼 621）── ⚙「OneNote を取り込む」の中身。
//!
//! **読むのは `amber-onenote`、どこに・どういう名前で置くかはここ。**
//! 名前の決まり（本体は 60 文字・使えない文字は `-`・同じ名前は `-2`）は
//! `note::file_stem` と `note::create` のものをそのまま使う ── 取り込んだ
//! ノートと手で作ったノートが、同じフォルダで別の決まりの名前を持たない。
//!
//! 置き方（本人に見取り図を見せて通ったもの・2026-09-16）:
//!
//! ```text
//! <出力先>/<ノートブック>/<セクショングループ>/…/<セクション>/<ページ>.md
//!                                                        /attachments/<ページ>-001.png
//! ```
//!
//! **一度に全部書かない。** エンジンは一本の糸で順に答えるので、大きい
//! `.onepkg` を一回の呼び出しで書くと、その間は画面が何を訊いても返らない。
//! 開く（`open`）で読み終えて手元に持ち、書くのは**セクション一つずつ**
//! （`write`）── デスクトップ版はその合間に「3/12」と表示できる。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use amber_onenote::{Opened, PageOut};

/// 開いたものの保持場所。**キーは数値**（パスをキーにすると、同じ `.onepkg` を
/// 二度開いたときに片方の `close` がもう片方を消す）。
static OPENED: Mutex<Option<(u64, HashMap<u64, Opened>)>> = Mutex::new(None);

/// 下見の一件。
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    pub name: String,
    pub bytes: u64,
    /// UNIX 秒。新しい順に並べるのに使う。
    pub modified: u64,
}

/// `.onepkg` を探す。**渡されたフォルダの直下だけ** ── ドキュメントの下を
/// 掘り進むと、会社の端末では共有ドライブの写しまで歩いて帰ってこない。
/// 新しい順（いま書き出したものが一番上）。
pub fn find(dirs: &[PathBuf]) -> Vec<Found> {
    let mut out = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let is_pkg = p
                .extension()
                .map(|x| x.eq_ignore_ascii_case("onepkg"))
                .unwrap_or(false);
            if !is_pkg || out.iter().any(|f: &Found| f.path == p) {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.push(Found {
                name: p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                path: p,
                bytes: meta.len(),
                modified,
            });
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.name.cmp(&b.name)));
    out
}

/// 開いて読み終え、預ける。返すのは鍵と、中身の一覧（書く前に見せる用）。
///
/// **読み手の panic をここで止める。** 壊れた `.onepkg` は珍しくなく、
/// 外部クレートのパーサーが panic すると、エンジンごと落ちてアプリ全体が止まる。
pub fn open(path: &Path) -> anyhow::Result<(u64, serde_json::Value)> {
    let got = std::panic::catch_unwind(|| amber_onenote::open(path))
        .map_err(|_| anyhow::anyhow!("読めない形でした（壊れているかもしれません）"))??;
    let summary = summary(&got);
    let mut slot = OPENED.lock().unwrap_or_else(|e| e.into_inner());
    let (next, map) = slot.get_or_insert_with(|| (1, HashMap::new()));
    let key = *next;
    *next += 1;
    map.insert(key, got);
    Ok((key, summary))
}

fn summary(o: &Opened) -> serde_json::Value {
    let units: Vec<_> = o
        .units
        .iter()
        .map(|u| {
            serde_json::json!({
                "groups": u.groups,
                "name": u.name,
                "pages": u.pages.len(),
                "pictures": u.pages.iter().map(|p| p.pictures.len()).sum::<usize>(),
            })
        })
        .collect();
    serde_json::json!({
        "book": o.book,
        "units": units,
        "pages": o.units.iter().map(|u| u.pages.len()).sum::<usize>(),
    })
}

/// 手放す。**書き終えたら必ず** ── 絵の中身まで持っているので、放って
/// おくと取り込むたびにエンジンが太る。
pub fn close(key: u64) {
    let mut slot = OPENED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, map)) = slot.as_mut() {
        map.remove(&key);
    }
}

/// 書いた数。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Wrote {
    pub dir: PathBuf,
    pub pages: usize,
    pub pictures: usize,
    pub files: usize,
}

/// セクション 1 つを書く。`to` は出力先（その下にノートブック名のフォルダを作る）。
pub fn write(key: u64, i: usize, to: &Path) -> anyhow::Result<Wrote> {
    let slot = OPENED.lock().unwrap_or_else(|e| e.into_inner());
    let Some(o) = slot.as_ref().and_then(|(_, m)| m.get(&key)) else {
        anyhow::bail!("開いていません（もう一度選んでください）");
    };
    let Some(u) = o.units.get(i) else {
        anyhow::bail!("{} 番目のセクションはありません", i + 1);
    };
    let mut dir = to.to_path_buf();
    // `.one` 単体（ノートブック名が無い）ときは、セクションを出力先の直下に。
    for seg in std::iter::once(&o.book).chain(u.groups.iter()).chain(std::iter::once(&u.name)) {
        let s = crate::note::file_stem(seg);
        if !s.is_empty() {
            dir.push(s);
        }
    }
    std::fs::create_dir_all(&dir)?;
    let mut wrote = Wrote { dir: dir.clone(), ..Wrote::default() };
    for p in &u.pages {
        let (pics, files) = page(&dir, p)?;
        wrote.pages += 1;
        wrote.pictures += pics;
        wrote.files += files;
    }
    Ok(wrote)
}

/// ページ 1 つ。**上書きしない** ── 同じタイトルは `-2`（手で作るノートと同じ）。
/// 2 度取り込んだら 2 つになるが、1 つ目を黙って潰すよりいい。
fn page(dir: &Path, p: &PageOut) -> anyhow::Result<(usize, usize)> {
    let stem = match crate::note::file_stem(&p.title) {
        s if s.is_empty() => "無題".to_string(),
        s => s,
    };
    let at = fresh(dir, &stem, "md")?;
    let stem = at
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or(stem);

    let mut pics = Vec::new();
    if !p.pictures.is_empty() || !p.files.is_empty() {
        std::fs::create_dir_all(dir.join("attachments"))?;
    }
    for (n, pic) in p.pictures.iter().enumerate() {
        // 名前はページの名前と番号（`段取り-001.png`）── **絵がどのページの
        // ものか、フォルダを覗いた人に分かる。**
        let got = fresh(&dir.join("attachments"), &format!("{}-{:03}", plain(&stem), n + 1), &pic.ext)?;
        std::fs::write(&got, &pic.bytes)?;
        pics.push(link(&got));
    }
    let mut files = Vec::new();
    for f in &p.files {
        let name = Path::new(&f.name);
        let base = crate::note::file_stem(
            &name.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        );
        let base = match plain(&base) {
            b if b.is_empty() => format!("{}-添付", plain(&stem)),
            b => b,
        };
        let ext = name
            .extension()
            .map(|e| crate::note::file_stem(&e.to_string_lossy()))
            .unwrap_or_default();
        let got = fresh(&dir.join("attachments"), &base, &ext)?;
        std::fs::write(&got, &f.bytes)?;
        files.push(link(&got));
    }

    let body = amber_onenote::fill(&p.body, &pics, &files);
    // 前書きは手で作るノートと同じ形（`note::new_note`）── 題と作った日。
    let text = format!("---\ntitle: {}\ncreated: {}\n---\n\n{}", one_line(&p.title), p.created, body);
    std::fs::write(&at, text.as_bytes())?;
    Ok((pics.len(), files.len()))
}

/// 前書きの一行に収める。題に改行が入っていると、前書きが壊れる。
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Markdown に書くパス（`attachments/名前`）。**名前はそのまま** ── デスクトップ版はパスを
/// 自分で URL に直すので、ここで `%20` にすると二重になって絵が出ない
/// （デスクトップ版で踏んだ）。代わりに、置く名前から空白と括弧を抜いておく（[`plain`]）。
fn link(file: &Path) -> String {
    let name = file.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    format!("attachments/{name}")
}

/// 添付の名前に使う形。**空白と括弧は `-`** ── `![](a b.png)` も
/// `![](a(1).png)` も、パーサーによってはそこでパスが切れる。
fn plain(stem: &str) -> String {
    let mut out = String::new();
    for c in stem.chars() {
        let c = if c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '<' | '>' | '#' | '%') { '-' } else { c };
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    out.trim_matches('-').to_string()
}

/// まだ無い名前（`名前.ext`、`名前-2.ext`、…）を取る。
///
/// `note::fresh_file` と同じ取り方（`create_new`）だが、**上限を置かない** ──
/// 手で作るノートが 99 本同じ名前になることは無いが、OneNote のセクションに
/// 「無題」のページが 100 枚あるのは、ありうる。
fn fresh(dir: &Path, stem: &str, ext: &str) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let name = |n: usize| {
        let s = if n == 1 { stem.to_string() } else { format!("{stem}-{n}") };
        if ext.is_empty() { s } else { format!("{s}.{ext}") }
    };
    for n in 1.. {
        let at = dir.join(name(n));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&at) {
            Ok(_) => return Ok(at),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 100_000 => continue,
            Err(e) => return Err(e.into()),
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../amber-onenote/tests/samples")
    }

    #[test]
    fn 探すのは直下の_onepkg_だけで_新しい順() {
        let t = tempfile::tempdir().unwrap();
        let docs = t.path().join("Documents");
        std::fs::create_dir_all(docs.join("奥")).unwrap();
        std::fs::write(docs.join("古い.onepkg"), b"x").unwrap();
        std::fs::write(docs.join("奥/深い.onepkg"), b"x").unwrap();
        std::fs::write(docs.join("ほか.one"), b"x").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options().write(true).open(docs.join("古い.onepkg")).unwrap().set_modified(old).unwrap();
        std::fs::write(docs.join("新しい.ONEPKG"), b"xy").unwrap();
        // 同じフォルダを二度渡しても二度数えない。
        let got = find(&[docs.clone(), docs.clone(), t.path().join("無い")]);
        let names: Vec<_> = got.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["新しい", "古い"]);
        assert_eq!(got[0].bytes, 2);
    }

    #[test]
    fn セクションごとに_一冊_セクションの下へ書く() {
        let t = tempfile::tempdir().unwrap();
        let (key, sum) = open(&samples().join("fsshttp/New Section 1.one")).unwrap();
        assert_eq!(sum["units"][0]["name"], "New Section 1");
        assert_eq!(sum["units"][0]["pictures"], 1);
        let w = write(key, 0, t.path()).unwrap();
        // `.one` 単体はノートブック名が無い ── 出力先の直下にセクション。
        assert_eq!(w.dir, t.path().join("New Section 1"));
        assert_eq!((w.pages, w.pictures), (1, 1));
        let md = std::fs::read_to_string(w.dir.join("Test Page.md")).unwrap();
        assert!(md.starts_with("---\ntitle: Test Page\ncreated: 2020-10-27\n---\n\n"), "{md}");
        assert!(md.contains("![](attachments/Test-Page-001.jpg)"), "{md}");
        assert!(!md.contains(amber_onenote::PIC_OPEN), "印が残った");
        let jpg = std::fs::read(w.dir.join("attachments/Test-Page-001.jpg")).unwrap();
        assert!(jpg.starts_with(b"\xff\xd8\xff"));

        // **二度書いても上書きしない。**
        let w2 = write(key, 0, t.path()).unwrap();
        assert_eq!(w2.pages, 1);
        let md2 = std::fs::read_to_string(w.dir.join("Test Page-2.md")).unwrap();
        assert!(md2.contains("![](attachments/Test-Page-2-001.jpg)"), "{md2}");
        assert_eq!(std::fs::read_to_string(w.dir.join("Test Page.md")).unwrap(), md);

        close(key);
        assert!(write(key, 0, t.path()).is_err(), "手放したのに書けた");
    }

    #[test]
    fn 一冊の名前とセクションの名前がフォルダになる() {
        let t = tempfile::tempdir().unwrap();
        let (key, sum) = open(&samples().join("notebook")).unwrap();
        assert_eq!(sum["units"].as_array().unwrap().len(), 2);
        let w = write(key, 1, t.path()).unwrap();
        close(key);
        assert_eq!(w.dir, t.path().join("notebook").join("New Section 2"));
        assert_eq!(w.files, 1);
        let mp3 = w.dir.join("attachments/ff-16b-2c-44100hz.mp3");
        assert!(mp3.is_file(), "添付が置かれていない");
        let any = std::fs::read_dir(&w.dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
            .map(|e| std::fs::read_to_string(e.path()).unwrap())
            .collect::<String>();
        assert!(any.contains("[ff-16b-2c-44100hz.mp3](attachments/ff-16b-2c-44100hz.mp3)"), "{any}");
    }

    #[test]
    fn 名前の決まりはノートと同じ() {
        let t = tempfile::tempdir().unwrap();
        let a = fresh(t.path(), "無題", "md").unwrap();
        let b = fresh(t.path(), "無題", "md").unwrap();
        assert_eq!(a.file_name().unwrap(), "無題.md");
        assert_eq!(b.file_name().unwrap(), "無題-2.md");
        assert_eq!(link(Path::new("x/段取り-001.png")), "attachments/段取り-001.png");
        assert_eq!(plain("a b(1) [x]"), "a-b-1-x");
        assert_eq!(one_line("上\n下"), "上 下");
    }

    #[test]
    fn 読めないものは断る_落ちない() {
        let t = tempfile::tempdir().unwrap();
        let bad = t.path().join("壊れた.onepkg");
        std::fs::write(&bad, b"MSCF not really").unwrap();
        assert!(open(&bad).is_err());
    }
}
