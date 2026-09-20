//! OneNote が書き出したものを、ambər の読める Markdown に（依頼 621）。
//!
//! 読み込みは外部クレート、出力は自前。
//!
//! 読み込みは [`onenote_parser`]（MPL-2.0）が担う。`.onepkg`（CAB）も、公開仕様の
//! `.one` も、SharePoint／OneDrive から落とした FSSHTTP 形式の `.one` も読める。
//! 自前のパーサー（`scripts/onestore.py`）は公開仕様しか読めず、実データでは
//! 0 ページだった。
//!
//! テキストランの区切り方とリンクの取り出し方は [one2html](https://github.com/msiemens/one2html)
//! （同じ作者の HTML 変換）に倣った。ランの境界は UTF-16 のオフセットで、リンクは
//! 「隠しランに URL、次の可視ランがその表示文字列」という形で入っている。
//!
//! ここでやるのは構造を Markdown に移すところまで。どこにどういう名前で書くか
//! （ファイル名の長さ、画像の名前、タイトル重複時のずらし方）は `amber-core` が
//! 決める。ルールが 2 か所にあると、デスクトップ版と iPhone で別の名前が付く。

use anyhow::{Context, Result};
use onenote_parser::contents::{Content, EmbeddedFile, Image, OutlineElement, OutlineItem, RichText, Table};
use onenote_parser::notebook::Notebook;
use onenote_parser::page::{Page, PageContent};
use onenote_parser::section::{Section, SectionEntry};
use onenote_parser::Parser;
use std::path::Path;
use typed_path::{PathType, TypedPath};

/// 画像の位置を示すマーカー。本文の中に `PIC_OPEN` 番号 `PIC_CLOSE` と書いておき、
/// 名前が決まってから [`fill`] で差し替える（名前は `amber-core` が決める）。
///
/// 私用領域の文字を使う。OneNote の本文に `{1}` のような文字は普通に出てくるが、
/// U+E000 は出てこない。
pub const PIC_OPEN: char = '\u{E000}';
pub const PIC_CLOSE: char = '\u{E001}';
/// 添付ファイルの位置を示すマーカー（画像と同じ仕組みで、番号の系列だけ別）。
pub const FILE_OPEN: char = '\u{E002}';
pub const FILE_CLOSE: char = '\u{E003}';

/// 読み込んだノートブック。中身はすべて自前の型（外部クレートの型を公開しない）。
#[derive(Debug, Clone)]
pub struct Opened {
    /// ノートブックの名前（`.onepkg` ならファイル名、`.one` 一本なら空）。
    pub book: String,
    pub units: Vec<Unit>,
}

/// セクション 1 つ。`groups` はセクショングループの階層（外側から順に）。
#[derive(Debug, Clone)]
pub struct Unit {
    pub groups: Vec<String>,
    pub name: String,
    pub pages: Vec<PageOut>,
}

/// 1 ページ分の Markdown。
#[derive(Debug, Clone)]
pub struct PageOut {
    pub title: String,
    /// OneNote のページ階層（1 が親、2 以上がサブページ）。
    pub level: i32,
    /// 作った日（`YYYY-MM-DD`）。
    pub created: String,
    /// 本文。画像の場所には `PIC_OPEN` 番号 `PIC_CLOSE` が入っている。
    pub body: String,
    pub pictures: Vec<Picture>,
    /// 添付ファイル（名前と中身）。中身も保持する。名前だけ残すと、取り込んだ
    /// あとで元の OneNote が無くなったときに失われる。
    pub files: Vec<Attached>,
}

#[derive(Debug, Clone)]
pub struct Picture {
    /// 拡張子（`png` `jpg` …、点なし・小文字）。
    pub ext: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Attached {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// 読み込む。パスの形から読み方を選ぶ（`.onepkg` / `.one` / `.onetoc2` /
/// それらが入ったフォルダ）。
pub fn open(path: &Path) -> Result<Opened> {
    let p = Parser::new_with_fs(StdFs);
    let s = path.to_string_lossy().to_string();
    let typed = native(&s);
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    match ext.as_str() {
        "onepkg" => {
            let nb = p
                .parse_package(typed)
                .with_context(|| format!("{} を開けません", path.display()))?;
            Ok(Opened { book: stem, units: from_notebook(&nb) })
        }
        "onetoc2" => {
            let nb = p
                .parse_notebook(typed)
                .with_context(|| format!("{} を開けません", path.display()))?;
            let book = path
                .parent()
                .and_then(|d| d.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            Ok(Opened { book, units: from_notebook(&nb) })
        }
        "one" => {
            let sec = p
                .parse_section(typed)
                .with_context(|| format!("{} を開けません", path.display()))?;
            Ok(Opened { book: String::new(), units: vec![unit(&sec, Vec::new())] })
        }
        _ if path.is_dir() => open_dir(path),
        _ => anyhow::bail!("OneNote のファイルではありません: {}", path.display()),
    }
}

/// パスを、実行中の OS の形式としてパーサーに渡す（`derive` は `C:\\…` を Unix の
/// パスと見なしてしまう）。
fn native(s: &str) -> TypedPath<'_> {
    TypedPath::new(s, if cfg!(windows) { PathType::Windows } else { PathType::Unix })
}

/// パーサーにファイルを読ませるための実装。パーサー付属の `NativeFs` は使わない。
///
/// `NativeFs` はパスを 1 要素ずつ `push_checked` で組み直し、そこでドライブ名
/// （`C:`）を不正な前置きとして拒否する。そのため Windows ではどの `.onepkg` も
/// 開けなかった（3.1.3 のリリーステストで発覚。macOS では起きない）。ここでは
/// 標準の `std::fs` にそのまま渡す。
///
/// 目次（`.onetoc2`）に書かれた名前はパーサー側が先にサニタイズする（`..` や
/// 絶対パスを拒否する）ので、ここに来るのはノートブックのフォルダ内のパスだけ。
/// `COM1` のようなデバイス名は、Windows では `\\?\` を付けて通常のファイルとして
/// 開く。
#[derive(Clone, Copy)]
struct StdFs;

#[cfg(windows)]
fn host(p: TypedPath) -> std::path::PathBuf {
    let s = p.to_string_lossy().into_owned();
    let abs = std::path::absolute(&s).map(|a| a.to_string_lossy().replace('/', "\\")).unwrap_or(s);
    if abs.starts_with(r"\\?\") {
        abs.into()
    } else if let Some(unc) = abs.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{unc}").into()
    } else {
        format!(r"\\?\{abs}").into()
    }
}

#[cfg(not(windows))]
fn host(p: TypedPath) -> std::path::PathBuf {
    p.to_string_lossy().into_owned().into()
}

fn typed(p: &Path) -> typed_path::TypedPathBuf {
    native(&p.to_string_lossy()).to_path_buf()
}

impl onenote_parser::FileSystem for StdFs {
    fn is_directory(&self, path: TypedPath) -> std::io::Result<bool> {
        Ok(host(path).is_dir())
    }
    fn read_dir(&self, path: TypedPath) -> std::io::Result<Vec<typed_path::TypedPathBuf>> {
        std::fs::read_dir(host(path))?.map(|e| e.map(|e| typed(&e.path()))).collect()
    }
    fn read_file(&self, path: TypedPath) -> std::io::Result<Vec<u8>> {
        std::fs::read(host(path))
    }
    // 書き込みはしない。読み込み専用なので、取り込みには不要。
    fn write_file(&self, _: TypedPath, _: &[u8]) -> std::io::Result<()> {
        Err(std::io::Error::other("書き込みはしません"))
    }
    fn stream_to_file(&self, _: TypedPath, _: &mut dyn std::io::Read) -> std::io::Result<()> {
        Err(std::io::Error::other("書き込みはしません"))
    }
    fn make_dir(&self, _: TypedPath) -> std::io::Result<()> {
        Err(std::io::Error::other("書き込みはしません"))
    }
    fn canonicalize(&self, path: TypedPath) -> std::io::Result<typed_path::TypedPathBuf> {
        Ok(typed(&std::fs::canonicalize(host(path))?))
    }
    fn exists(&self, path: TypedPath) -> std::io::Result<bool> {
        std::fs::exists(host(path))
    }
}

/// フォルダを読む。目次（`.onetoc2`）があればそれを使う。セクショングループの
/// 並び順と名前は目次が持っているため。なければ `.one` を 1 つずつ読む。
fn open_dir(dir: &Path) -> Result<Opened> {
    let book = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    entries.sort();
    if let Some(toc) = entries.iter().find(|p| {
        p.extension().map(|e| e.eq_ignore_ascii_case("onetoc2")).unwrap_or(false)
    }) {
        let mut got = open(toc)?;
        got.book = book;
        return Ok(got);
    }
    let mut units = Vec::new();
    for p in entries {
        if p.extension().map(|e| e.eq_ignore_ascii_case("one")).unwrap_or(false) {
            units.extend(open(&p)?.units);
        }
    }
    Ok(Opened { book, units })
}

fn from_notebook(nb: &Notebook) -> Vec<Unit> {
    let mut out = Vec::new();
    walk(nb.entries(), &mut Vec::new(), &mut out);
    out
}

fn walk(entries: &[SectionEntry], groups: &mut Vec<String>, out: &mut Vec<Unit>) {
    for e in entries {
        match e {
            SectionEntry::Section(s) => out.push(unit(s, groups.clone())),
            SectionEntry::SectionGroup(g) => {
                // ゴミ箱は変換しない。OneNote が自動で作る隠しグループで、
                // 削除済みのページが入っている。
                if g.display_name() == "OneNote_RecycleBin" {
                    continue;
                }
                groups.push(g.display_name().to_string());
                walk(g.entries(), groups, out);
                groups.pop();
            }
        }
    }
}

fn unit(s: &Section, groups: Vec<String>) -> Unit {
    let name = s.display_name().trim_end_matches(".one").to_string();
    let pages = s
        .page_series()
        .iter()
        .flat_map(|ps| ps.pages().iter())
        .map(page)
        .collect();
    Unit { groups, name, pages }
}

fn page(p: &Page) -> PageOut {
    let mut w = Writer::default();
    for c in p.contents() {
        match c {
            PageContent::Outline(o) => {
                for item in o.items() {
                    w.item(item, "");
                }
                w.leave_list();
            }
            PageContent::Image(img) => {
                w.leave_list();
                w.picture(img);
                w.out.push_str("\n\n");
            }
            PageContent::EmbeddedFile(f) => {
                w.leave_list();
                w.attached(f);
            }
            _ => {}
        }
    }
    let t = p.created_time();
    PageOut {
        title: p.title_text().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
            .unwrap_or_else(|| "無題".to_string()),
        level: p.level(),
        created: format!("{:04}-{:02}-{:02}", t.year(), u8::from(t.month()), t.day()),
        body: tidy(&w.out),
        pictures: w.pictures,
        files: w.files,
    }
}

/// 名前の決まった画像と添付を、本文のマーカーに差し込む。`pics[i]` が `i+1` 番目の
/// 画像、`files[i]` が `i+1` 番目の添付。
pub fn fill(body: &str, pics: &[String], files: &[String]) -> String {
    let body = fill_one(body, PIC_OPEN, PIC_CLOSE, pics);
    fill_one(&body, FILE_OPEN, FILE_CLOSE, files)
}

fn fill_one(body: &str, open: char, close: char, names: &[String]) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(at) = rest.find(open) {
        out.push_str(&rest[..at]);
        let after = &rest[at + open.len_utf8()..];
        let Some(end) = after.find(close) else {
            out.push_str(&rest[at..]);
            return out;
        };
        let n: usize = after[..end].parse().unwrap_or(0);
        if let Some(name) = n.checked_sub(1).and_then(|i| names.get(i)) {
            out.push_str(name);
        }
        rest = &after[end + close.len_utf8()..];
    }
    out.push_str(rest);
    out
}

/// 空行を 2 行までに詰め、先頭と末尾の空白を落とす。
fn tidy(s: &str) -> String {
    let mut out = String::new();
    let mut blanks = 0;
    for line in s.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
        } else {
            blanks = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_matches('\n').to_string() + "\n"
}

#[derive(Default)]
struct Writer {
    out: String,
    pictures: Vec<Picture>,
    files: Vec<Attached>,
    /// いま箇条書きの中か。
    listing: bool,
}

impl Writer {
    /// `pad` はリストの中にいるときだけ伸びる。インデントした段落をそのまま
    /// 空白で出力すると、4 つ目の空白から Markdown はコードブロックと解釈する
    /// （実データで踏んだ。OneNote のインデントは見た目だけで入れ子ではない）。
    fn item(&mut self, item: &OutlineItem, pad: &str) {
        match item {
            OutlineItem::Group(g) => {
                for it in g.outlines() {
                    self.item(it, pad);
                }
            }
            OutlineItem::Element(e) => self.element(e, pad),
        }
    }

    fn element(&mut self, e: &OutlineElement, pad: &str) {
        // 番号付きか箇条書きか。番号付きの目印は U+FFFD（one2html と同じ判定）。
        let mark = e.list_contents().first().map(|l| {
            if l.list_format().first() == Some(&'\u{fffd}') { "1. " } else { "- " }
        });
        for c in e.contents() {
            match c {
                Content::RichText(t) => {
                    let line = runs(t);
                    if line.trim().is_empty() {
                        continue;
                    }
                    // チェックボックスは Markdown のチェックボックスに変換する
                    // （依頼 635・本人「OneNote でチェックボックスにしていたものが、
                    // ただの文字列になっていた」）。OneNote ではチェックボックスも
                    // 「ノートタグ」の一種。
                    if let Some(done) = ticked(t) {
                        self.out.push_str(&format!(
                            "{pad}- [{}] {}\n", if done { "x" } else { " " }, line.trim()));
                        self.listing = true;
                        continue;
                    }
                    if let Some(m) = mark {
                        self.out.push_str(&format!("{pad}{m}{}\n", line.trim()));
                        self.listing = true;
                        continue;
                    }
                    self.leave_list();
                    if let Some(h) = heading(t) {
                        self.out.push_str(&format!("{} {}\n\n", "#".repeat(h), line.trim()));
                    } else {
                        self.out.push_str(&format!("{}\n\n", line.trim()));
                    }
                }
                Content::Table(t) => {
                    self.leave_list();
                    self.table(t);
                }
                Content::Image(img) => {
                    self.leave_list();
                    self.picture(img);
                    self.out.push_str("\n\n");
                }
                Content::EmbeddedFile(f) => {
                    self.leave_list();
                    self.attached(f);
                }
                _ => {}
            }
        }
        // 子のインデント幅は、親のマーカーの幅ぶん（`- ` なら 2、`1. ` なら 3）。
        let kid_pad = match mark {
            Some(m) => format!("{pad}{}", " ".repeat(m.len())),
            None => pad.to_string(),
        };
        for kid in e.children() {
            self.item(kid, &kid_pad);
        }
    }

    /// リストを抜けるときは 1 行あける。詰めたままだと、次の段落が最後の項目の
    /// 続きとして扱われる。
    fn leave_list(&mut self) {
        if self.listing {
            self.out.push('\n');
            self.listing = false;
        }
    }

    fn picture(&mut self, img: &Image) {
        let Some(mut r) = img.read() else { return };
        let mut bytes = Vec::new();
        if std::io::Read::read_to_end(&mut r, &mut bytes).is_err() || bytes.is_empty() {
            return;
        }
        let ext = img
            .extension()
            .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .or_else(|| kind_of(&bytes).map(str::to_string))
            .unwrap_or_else(|| "png".to_string());
        self.pictures.push(Picture { ext, bytes });
        // 代替テキストは空にする（ambər 自身が貼り付けるときと同じ形・依頼 593）。
        self.out.push_str(&format!("![]({PIC_OPEN}{}{PIC_CLOSE})", self.pictures.len()));
    }

    fn attached(&mut self, f: &EmbeddedFile) {
        let mut bytes = Vec::new();
        let _ = std::io::Read::read_to_end(&mut f.read(), &mut bytes);
        let name = f.filename().to_string();
        let shown = name.replace(['[', ']'], "");
        self.files.push(Attached { name, bytes });
        self.out.push_str(&format!(
            "添付ファイル: [{shown}]({FILE_OPEN}{}{FILE_CLOSE})\n\n",
            self.files.len()
        ));
    }

    fn table(&mut self, t: &Table) {
        let mut rows: Vec<Vec<String>> = Vec::new();
        for r in t.contents() {
            let mut cells = Vec::new();
            for cell in r.contents() {
                let mut inner = Writer::default();
                for e in cell.contents() {
                    inner.element(e, "");
                }
                // セルの中の画像は、表の外側の通し番号に振り直す。
                let base = self.pictures.len();
                let mut text = inner.out;
                for (i, p) in inner.pictures.into_iter().enumerate() {
                    text = text.replace(
                        &format!("{PIC_OPEN}{}{PIC_CLOSE}", i + 1),
                        &format!("{PIC_OPEN}{}{PIC_CLOSE}", base + i + 1),
                    );
                    self.pictures.push(p);
                }
                let fbase = self.files.len();
                for (i, f) in inner.files.into_iter().enumerate() {
                    text = text.replace(
                        &format!("{FILE_OPEN}{}{FILE_CLOSE}", i + 1),
                        &format!("{FILE_OPEN}{}{FILE_CLOSE}", fbase + i + 1),
                    );
                    self.files.push(f);
                }
                // セル内の改行は空白でつなぐ（`<br>` は ambər の画面に文字として出る）。
                let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
                cells.push(joined.replace('|', "\\|"));
            }
            rows.push(cells);
        }
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        if width == 0 {
            return;
        }
        // ヘッダー行は空にする（依頼 578・600）。1 行目をヘッダーにすると、それが
        // ヘッダーかどうか分からないままデータが 1 行消える。
        self.out.push_str(&format!("|{}\n", "  |".repeat(width)));
        self.out.push_str(&format!("|{}\n", " --- |".repeat(width)));
        for mut r in rows {
            r.resize(width, String::new());
            self.out.push_str(&format!("| {} |\n", r.join(" | ")));
        }
        self.out.push('\n');
    }
}

/// この段落がチェックボックスかどうか。チェック済みかどうかも返す。
///
/// OneNote のチェックボックスは「ノートタグ」の一種で、種別名に `CheckBox` が
/// 入っている（`GreenCheckBox` / `YellowStarCheckBox` など、80 種ほどある）。
/// 種別名で判定する。80 個の分岐を書き写すと、パーサーが種別を 1 つ追加した
/// ときに、そこだけチェックボックスにならない。
fn ticked(t: &RichText) -> Option<bool> {
    t.note_tags().iter().find_map(|tag| {
        let shape = tag.definition()?.shape();
        if !format!("{shape:?}").contains("CheckBox") {
            return None;
        }
        Some(tag.item_status().completed())
    })
}

/// 段落のスタイルから見出しレベル（`h1`〜`h6`）を求める。
fn heading(t: &RichText) -> Option<usize> {
    let id = t.paragraph_style().style_id()?;
    let n = id.strip_prefix('h')?.parse::<usize>().ok()?;
    (1..=6).contains(&n).then_some(n)
}

/// 画像の先頭数バイトから形式を判定する。
fn kind_of(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if b.starts_with(b"\xff\xd8\xff") {
        Some("jpg")
    } else if b.starts_with(b"GIF8") {
        Some("gif")
    } else if b.starts_with(b"BM") {
        Some("bmp")
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// 1 段落を、テキストランごとの書式つきで出力する。
///
/// ランの境界は UTF-16 のオフセット（MS-ONE 2.3.76）。`char` で数えると、絵文字や
/// 一部の漢字（サロゲートペア）の後ろで書式が 1 文字ぶんずれる。
///
/// リンクの解釈はパーサーに任せる（`RichText::hyperlinks`）。OneNote は隠しランに
/// `\u{FDDF}HYPERLINK "URL"` を置き、次の可視ランをその表示文字列にする。
/// one2html と同じ解釈が、パーサーの 2.0 に入った。
fn runs(t: &RichText) -> String {
    let text = t.text().replace('\r', "");
    let styles = t.text_run_formatting();
    let units: Vec<u16> = text.encode_utf16().collect();
    let total = units.len();
    if styles.is_empty() {
        return dress(&text, t.paragraph_style());
    }
    // ランの [開始, 終了)。境界の数は書式の数より 1 つ少ない（最後は末尾まで）。
    let mut spans = Vec::new();
    let mut from = 0usize;
    for (i, style) in styles.iter().enumerate() {
        let to = t.text_run_indices().get(i).map(|&x| (x as usize).min(total)).unwrap_or(total);
        let to = to.max(from);
        spans.push((from, to, style));
        from = to;
    }
    let links = t.hyperlinks();
    let mut out = String::new();
    let mut i = 0;
    while i < spans.len() {
        let (s, e, style) = spans[i];
        if style.hidden() {
            i += 1;
            continue;
        }
        if let Some(l) = links.iter().find(|l| l.start() as usize <= s && e <= l.end() as usize && s < e) {
            // リンクの表示文字列は、ランをまたいで 1 つにまとめる。
            let mut inner = String::new();
            while i < spans.len() && spans[i].1 <= l.end() as usize {
                let (s2, e2, st2) = spans[i];
                if !st2.hidden() {
                    inner.push_str(&dress(&String::from_utf16_lossy(&units[s2..e2]), st2));
                }
                i += 1;
            }
            let label = inner.trim();
            out.push_str(&format!("[{}]({})", if label.is_empty() { l.target() } else { label }, l.target()));
            continue;
        }
        out.push_str(&dress(&String::from_utf16_lossy(&units[s..e]), style));
        i += 1;
    }
    // パーサーが解釈できなかった制御文字（壊れたデータ）は、テキストとして残さない。
    match out.find('\u{FDDF}') {
        Some(_) => out.split('\u{FDDF}').next().unwrap_or("").to_string(),
        None => out,
    }
}

/// 1 つのランに書式を適用する。内側から順に、取り消し線 → 太字・斜体 → 色
/// （依頼 608 と同じ順。逆にすると Markdown の記号とタグが噛み合わない）。
fn dress(text: &str, s: &onenote_parser::contents::ParagraphStyling) -> String {
    // 前後の空白は記号の外に出す。`** 文字**` は太字にならない。
    let lead = text.len() - text.trim_start().len();
    let tail = text.len() - text.trim_end().len();
    let core = text.trim();
    if core.is_empty() {
        return text.to_string();
    }
    let body = wrap(core, s.bold(), s.italic(), s.strikethrough(), color(s.font_color()).as_deref());
    format!("{}{}{}", &text[..lead], body, &text[text.len() - tail..])
}

/// 書式を適用する順序（依頼 633）。色がいちばん外側で、Markdown の記号はその内側。
/// 本人が実データで踏んだ。表のヘッダー行（濃い背景に白い太字）が `**hoge**` と
/// アスタリスクごと表示され、しかも白くて読めなかった。ambər は色の内側にある
/// 記号を解釈する（`Inline::Colored`）ので、この順なら太字も色も効く。白に近い色は
/// そもそも引き継がない（[`pale`]）。
fn wrap(core: &str, bold: bool, italic: bool, strike: bool, hex: Option<&str>) -> String {
    let mut body = core.to_string();
    if strike {
        body = format!("~~{body}~~");
    }
    body = match (bold, italic) {
        (true, true) => format!("***{body}***"),
        (true, false) => format!("**{body}**"),
        (false, true) => format!("*{body}*"),
        _ => body,
    };
    // 色がいちばん外側。内側の `**` は ambər が解釈する（`markdown.rs` の
    // `Inline::Colored`）。逆にすると `<span>` が太字の内側に入り、生の HTML は
    // 記号として解釈されないので `<span …>` がそのまま表示される。
    match hex {
        Some(hex) => format!("<span style=\"color:{hex}\">{body}</span>"),
        None => body,
    }
}

/// 文字色。自動と黒は色として出力しない。出すとノート全体が span で埋まる
/// （貼り付け時と同じルール・依頼 616）。
///
/// 白に近い色も出力しない（依頼 633・本人「文字色が白色なのでめっちゃ読みにく
/// かった」）。OneNote の表のヘッダー行は「濃い背景に白い文字」だが、背景色は
/// こちらへ引き継げない（Markdown の表にセルの背景色がない）。白だけ引き継ぐと、
/// ambər の明るい背景の上で見えない文字になる。色を落とせば太字は残る。
fn color(c: Option<onenote_parser::property::common::ColorRef>) -> Option<String> {
    use onenote_parser::property::common::ColorRef;
    match c? {
        ColorRef::Manual { r, g, b } if (r, g, b) != (0, 0, 0) && !pale(r, g, b) => {
            Some(format!("#{r:02x}{g:02x}{b:02x}"))
        }
        _ => None,
    }
}

/// 明るすぎて白い背景の上で読めない色か（明るさは人間の視感度で重み付けして測る）。
fn pale(r: u8, g: u8, b: u8) -> bool {
    let bright = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    bright >= 236.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/samples").join(rel)
    }

    #[test]
    fn マーカーに名前を差し込む() {
        let body = format!("a ![]({PIC_OPEN}1{PIC_CLOSE}) b ![]({PIC_OPEN}2{PIC_CLOSE})");
        let body = format!("{body} [f]({FILE_OPEN}1{FILE_CLOSE})");
        let got = fill(&body, &["attachments/x-001.png".into(), "attachments/x-002.jpg".into()], &["attachments/f.docx".into()]);
        assert_eq!(got, "a ![](attachments/x-001.png) b ![](attachments/x-002.jpg) [f](attachments/f.docx)");
    }

    #[test]
    fn 名前の無いマーカーは空にする() {
        let body = format!("![]({PIC_OPEN}9{PIC_CLOSE})");
        assert_eq!(fill(&body, &[], &[]), "![]()");
    }

    /// 依頼 633。実データ（OneNote の表のヘッダー行）で踏んだ 2 件。
    #[test]
    fn 色は太字の内側に置き_白は落とす() {
        // 色が外側、記号が内側。`<span>` を記号の内側に入れると文字のまま表示される。
        assert_eq!(wrap("hoge", true, false, false, Some("#c00000")),
                   "<span style=\"color:#c00000\">**hoge**</span>");
        assert_eq!(wrap("hoge", true, false, false, None), "**hoge**");
        assert_eq!(wrap("x", true, true, true, None), "***~~x~~***");
        assert!(pale(255, 255, 255));
        assert!(pale(240, 240, 240));
        assert!(!pale(127, 127, 127));
        assert!(!pale(255, 255, 0), "黄色は明るいが、白ではない");
    }

    #[test]
    fn 絵の種類を頭から() {
        assert_eq!(kind_of(b"\x89PNG\r\n\x1a\nxx"), Some("png"));
        assert_eq!(kind_of(b"\xff\xd8\xff\xe0"), Some("jpg"));
        assert_eq!(kind_of(b"<?xml"), None);
    }

    #[test]
    fn 空行は二つまで() {
        assert_eq!(tidy("a\n\n\n\nb\n"), "a\n\nb\n");
    }

    /// 実データ（FSSHTTP 形式）。自前のパーサーでは 0 ページだった。
    #[test]
    fn fsshttp_のサンプルから_題_見出し_表_画像_数式が出る() {
        let got = open(&sample("fsshttp/New Section 1.one")).expect("開けない");
        assert_eq!(got.units.len(), 1);
        let pages = &got.units[0].pages;
        assert!(!pages.is_empty(), "0 ページ");
        let p = &pages[0];
        assert_eq!(p.title, "Test Page");
        assert!(p.body.contains("ABCDEF"), "{}", p.body);
        // 表はセルの中身つきで確認する。
        assert!(p.body.contains("| A | B | C |"), "表が出ない: {}", p.body);
        assert!(p.body.contains("| 1 | 2 | 3 |"), "{}", p.body);
        // ヘッダー行は空。
        assert!(p.body.contains("|  |  |  |\n| --- | --- | --- |"), "{}", p.body);
        // 画像は実際の JPEG で、本文にはマーカーが入る。
        assert_eq!(p.pictures.len(), 1, "絵が出ない");
        assert_eq!(p.pictures[0].ext, "jpg");
        assert!(p.pictures[0].bytes.starts_with(b"\xff\xd8\xff"), "JPEG の頭ではない");
        assert!(p.body.contains(&format!("![]({PIC_OPEN}1{PIC_CLOSE})")), "{}", p.body);
        // 入れ子のリスト。
        assert!(p.body.contains("\n  - "), "入れ子の点が出ない: {}", p.body);
        assert!(p.body.contains("\n   1. "), "番号の入れ子はマーカーの幅（3）で: {}", p.body);
        // リストでない行はインデントしない。空白 4 つでコードブロックになる。
        for l in p.body.lines() {
            let t = l.trim_start();
            if l.len() != t.len() {
                assert!(t.starts_with("- ") || t.starts_with("1. "), "インデントした段落: {l:?}");
            }
        }
        // リストの後の段落は 1 行あける。詰めると最後の項目の続きとして扱われる。
        let at = p.body.rfind("\n1. ").expect("番号が無い");
        let after = &p.body[at + 1..];
        let end = after.find('\n').unwrap();
        assert!(after[end..].starts_with("\n\n"), "箇条書きのすぐ後に段落: {after:.200}");
        // リンクの制御文字をテキストとして出さない（試作では `﷟HYPERLINK "…"` と出た）。
        assert!(!p.body.contains('\u{FDDF}'), "{}", p.body);
        assert!(!p.body.contains("HYPERLINK"), "{}", p.body);
        assert!(p.body.contains("](https://example.com)"), "リンクにならない: {}", p.body);
        // 作成日。
        assert_eq!(p.created.len(), 10, "{}", p.created);
    }

    /// チェックボックス（依頼 635）。チェック済み・未チェックのどちらも。
    /// OneNote ではチェックボックスも「ノートタグ」の一種で、種別名で判定する。
    #[test]
    fn チェックボックスはチェックボックスのまま変換する() {
        let got = open(&sample("checks/handwriting_recognition.one")).expect("開けない");
        let body: String = got.units[0].pages.iter().map(|p| p.body.clone()).collect();
        assert!(body.contains("- [ ] "), "未チェックのチェックボックスが出ない: {body:.400}");
        assert!(body.contains("- [x] "), "チェック済みのチェックボックスが出ない: {body:.400}");
    }

    /// 会社の環境からエクスポートしたものと同じ形式（公開仕様・MS-ONESTORE 2.3）。
    #[test]
    fn 公開仕様のサンプルも読める() {
        let got = open(&sample("desktop/OneWithFileData.one")).expect("開けない");
        assert_eq!(got.units.len(), 1);
        let p = &got.units[0].pages[0];
        // 添付は名前だけでなく中身も保持する（元の OneNote が無くなっても失われない）。
        assert_eq!(p.files.len(), 1);
        assert_eq!(p.files[0].name, "testing.docx");
        assert!(p.files[0].bytes.starts_with(b"PK"), "docx の中身ではない");
        assert!(p.body.contains(&format!("[testing.docx]({FILE_OPEN}1{FILE_CLOSE})")), "{}", p.body);
    }

    /// 目次つきのノートブックを CAB にまとめる。OneNote の「エクスポート」と同じ形式。
    /// 実物の `.onepkg` はリポジトリに置けない（会社のノート）ので、中身は実データの
    /// ノートブックを使い、CAB 化だけこちらで行う。
    fn pack(dir: &Path, to: &Path) {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".one") || n.ends_with(".onetoc2"))
            .collect();
        names.sort();
        let mut b = cab::CabinetBuilder::new();
        {
            let folder = b.add_folder(cab::CompressionType::MsZip);
            for n in &names {
                folder.add_file(n.clone());
            }
        }
        let mut w = b.build(std::fs::File::create(to).unwrap()).unwrap();
        while let Some(mut f) = w.next_file().unwrap() {
            let bytes = std::fs::read(dir.join(f.file_name())).unwrap();
            std::io::Write::write_all(&mut f, &bytes).unwrap();
        }
        w.finish().unwrap();
    }

    fn names(o: &Opened) -> Vec<String> {
        o.units.iter().map(|u| u.name.clone()).collect()
    }

    #[test]
    fn onepkg_を開くと_目次の順にセクションが並ぶ() {
        let tmp = std::env::temp_dir().join(format!("amber-onenote-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let pkg = tmp.join("仕事のノート.onepkg");
        pack(&sample("notebook"), &pkg);
        let got = open(&pkg);
        let _ = std::fs::remove_dir_all(&tmp);
        let got = got.expect("開けない");
        // ノートブック名はファイル名から取る。
        assert_eq!(got.book, "仕事のノート");
        assert_eq!(names(&got), ["New Section 1", "New Section 2"]);
        let pages: usize = got.units.iter().map(|u| u.pages.len()).sum();
        assert!(pages >= 2, "ページが足りない: {pages}");
        assert!(got.units.iter().all(|u| u.groups.is_empty()));
    }

    #[test]
    fn フォルダと目次からも同じものが出る() {
        let by_dir = open(&sample("notebook")).expect("フォルダ");
        let by_toc = open(&sample("notebook/Open Notebook.onetoc2")).expect("目次");
        assert_eq!(by_dir.book, "notebook");
        assert_eq!(by_toc.book, "notebook");
        assert_eq!(names(&by_dir), names(&by_toc));
        assert_eq!(names(&by_dir), ["New Section 1", "New Section 2"]);
    }

    #[test]
    fn onenote_でないものは断る() {
        assert!(open(&sample("README.md")).is_err());
    }
}
