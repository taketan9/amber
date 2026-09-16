//! OneNote が書き出したものを、ambər の読める Markdown に（依頼 621）。
//!
//! **読むのは借り物、書くのは自前。**
//!
//! 読み手は [`onenote_parser`]（MPL-2.0）── `.onepkg`（CAB）も、公開仕様の
//! `.one` も、SharePoint／OneDrive から落とした FSSHTTP 包みの `.one` も読む。
//! 自前の読み手（`scripts/onestore.py`）は公開仕様しか読めず、本物の見本で
//! **0 ページ**だった。
//!
//! 走りの切り方とリンクの取り出し方は [one2html](https://github.com/msiemens/one2html)
//! （同じ作者の HTML 書き出し）に倣った ── 走りの境目は **UTF-16 の位置**で、
//! リンクは「隠れた走りに URL の印、次の見える走りが字」という形で入っている。
//!
//! ここがやるのは**形を Markdown に移すところまで**。どこに・どういう名前で
//! 書くか（幹の長さ・画像の名前・同じ題のずらし方）は `amber-core` が決める
//! ── 決まりが二か所にあると、窓と電話で別の名前が付く。

use anyhow::{Context, Result};
use onenote_parser::contents::{Content, EmbeddedFile, Image, OutlineElement, OutlineItem, RichText, Table};
use onenote_parser::notebook::Notebook;
use onenote_parser::page::{Page, PageContent};
use onenote_parser::section::{Section, SectionEntry};
use onenote_parser::Parser;
use std::path::Path;
use typed_path::{PathType, TypedPath};

/// 画像の置き場所の印。本文の中で `PIC_OPEN` 番号 `PIC_CLOSE` と書いておき、
/// 名前が決まってから [`fill`] で差し替える（名前は `amber-core` が決める）。
///
/// **私用領域の字を使う** ── OneNote の本文に `{1}` のような字はふつうに
/// 出てくるが、U+E000 は出てこない。
pub const PIC_OPEN: char = '\u{E000}';
pub const PIC_CLOSE: char = '\u{E001}';
/// 添付ファイルの置き場所の印（絵と同じ仕組み、別の番号）。
pub const FILE_OPEN: char = '\u{E002}';
pub const FILE_CLOSE: char = '\u{E003}';

/// 開いた一冊。**中身はすべて持ち物**（借り物の型を外に出さない）。
#[derive(Debug, Clone)]
pub struct Opened {
    /// ノートブックの名前（`.onepkg` ならファイル名、`.one` 一本なら空）。
    pub book: String,
    pub units: Vec<Unit>,
}

/// セクション一つ。`groups` はセクショングループの道（外から順に）。
#[derive(Debug, Clone)]
pub struct Unit {
    pub groups: Vec<String>,
    pub name: String,
    pub pages: Vec<PageOut>,
}

/// ページ一枚ぶんの Markdown。
#[derive(Debug, Clone)]
pub struct PageOut {
    pub title: String,
    /// OneNote のページの段（1 が親、2 以上がサブページ）。
    pub level: i32,
    /// 作った日（`YYYY-MM-DD`）。
    pub created: String,
    /// 本文。画像の場所には `PIC_OPEN` 番号 `PIC_CLOSE` が入っている。
    pub body: String,
    pub pictures: Vec<Picture>,
    /// 添付ファイル（名前と中身）。**中身は捨てない** ── 名前だけ残すと、
    /// 取り込んだあとで元の OneNote が無くなった日に失われる。
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

/// 開く。**道の形で読み方を選ぶ** ── `.onepkg` / `.one` / `.onetoc2` /
/// それが入ったフォルダ。
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

/// 道を、**この機械の形として**部品に渡す（`derive` は `C:\\…` を Unix の道と見なす）。
fn native(s: &str) -> TypedPath<'_> {
    TypedPath::new(s, if cfg!(windows) { PathType::Windows } else { PathType::Unix })
}

/// 部品にファイルを読ませる口。**部品の `NativeFs` は使わない。**
///
/// `NativeFs` は道を一つずつ `push_checked` で組み直し、そこで**ドライブ名
/// （`C:`）を「思わぬ前置き」として断る** ── Windows では、どの `.onepkg` も
/// 開けなかった（3.1.3 のリリースの試験で踏んだ。Mac では決して起きない）。
/// こちらは標準の `std::fs` にそのまま渡す。
///
/// 目次（`.onetoc2`）に書かれた名前は部品が先に消毒する（`..` や絶対の道を断る）
/// ので、ここに来るのは一冊のフォルダの中の道だけ。**`COM1` のような機器の
/// 名前**は、Windows では `\\?\` を付けて字どおりのファイルとして開く。
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
    // **書かない。** 読み手が書く道は、取り込みには要らない。
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

/// フォルダ。**目次（`.onetoc2`）があればそれで読む** ── セクショングループの
/// 並びと名前は目次が持っている。無ければ `.one` を一本ずつ。
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
                // **ゴミ箱は写さない。** OneNote が自分で作る隠しグループで、
                // 消したページが入っている。
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

/// 名前が決まった画像と添付を、本文の印に差し込む。`pics[i]` が `i+1` 番目の絵、
/// `files[i]` が `i+1` 番目の添付。
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

/// 空行を二つまでに詰め、頭と尻の空白を落とす。
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
    /// `pad` は**箇条書きの中にいるときだけ**伸びる。字下げした段落を
    /// そのまま空白で写すと、4 つ目の空白で Markdown はコードの塊と読む
    /// （本物の見本で踏んだ ── OneNote の字下げは見た目だけの入れ子）。
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
        // 番号か点か ── 番号の印は U+FFFD（one2html と同じ見分け方）。
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
        // 子の字下げは、親の印の幅だけ（`- ` なら 2、`1. ` なら 3）。
        let kid_pad = match mark {
            Some(m) => format!("{pad}{}", " ".repeat(m.len())),
            None => pad.to_string(),
        };
        for kid in e.children() {
            self.item(kid, &kid_pad);
        }
    }

    /// 箇条書きを抜けるときは一行あける ── 詰めたままだと、次の段落が
    /// 最後の項目の続きとして呑まれる。
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
        // **説明は空**（ambər が自分で貼るのと同じ形・依頼 593）。
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
                // 升の中の絵は、外の番号に振り直して持っていく。
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
                // **升の中の改行は空白で繋ぐ**（`<br>` は ambər の画面に字として出る）。
                let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
                cells.push(joined.replace('|', "\\|"));
            }
            rows.push(cells);
        }
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        if width == 0 {
            return;
        }
        // **見出しの行は空で置く**（依頼 578・600）── 1 行目を見出しにすると、
        // それが見出しかどうか分からないままデータが一行消える。
        self.out.push_str(&format!("|{}\n", "  |".repeat(width)));
        self.out.push_str(&format!("|{}\n", " --- |".repeat(width)));
        for mut r in rows {
            r.resize(width, String::new());
            self.out.push_str(&format!("| {} |\n", r.join(" | ")));
        }
        self.out.push('\n');
    }
}

/// 段落の書式から見出しの段（`h1`〜`h6`）。
fn heading(t: &RichText) -> Option<usize> {
    let id = t.paragraph_style().style_id()?;
    let n = id.strip_prefix('h')?.parse::<usize>().ok()?;
    (1..=6).contains(&n).then_some(n)
}

/// 絵の頭の数バイトから、種類。
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

/// 一段落を、走りごとの飾りつきで。
///
/// **走りの境目は UTF-16 の位置**（MS-ONE 2.3.76）── `char` で数えると、
/// 絵文字や一部の漢字（サロゲート対）の後ろで飾りが一字ずれる。
///
/// **リンクは部品に読ませる**（`RichText::hyperlinks`）。OneNote は隠れた走りに
/// `\u{FDDF}HYPERLINK "URL"` を置き、次の見える走りを字にする ──
/// one2html で見た読み方と同じものが、部品の 2.0 に入った。
fn runs(t: &RichText) -> String {
    let text = t.text().replace('\r', "");
    let styles = t.text_run_formatting();
    let units: Vec<u16> = text.encode_utf16().collect();
    let total = units.len();
    if styles.is_empty() {
        return dress(&text, t.paragraph_style());
    }
    // 走りの [始め, 終わり)。印の数が飾りより一つ少ないのが決まり（最後は尻まで）。
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
            // リンクの字は、走りをまたいで一つに束ねる。
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
    // 部品が読めなかった印（壊れた形）は、字として残さない。
    match out.find('\u{FDDF}') {
        Some(_) => out.split('\u{FDDF}').next().unwrap_or("").to_string(),
        None => out,
    }
}

/// 一つの走りに飾りを巻く。**内から: 取り消し → 太字・斜体 → 色**
/// （依頼 608 と同じ順 ── 逆にすると印と札が噛み合わない）。
fn dress(text: &str, s: &onenote_parser::contents::ParagraphStyling) -> String {
    // 前後の空白は印の外に出す ── `** 字**` は太字にならない。
    let lead = text.len() - text.trim_start().len();
    let tail = text.len() - text.trim_end().len();
    let core = text.trim();
    if core.is_empty() {
        return text.to_string();
    }
    let mut body = core.to_string();
    if s.strikethrough() {
        body = format!("~~{body}~~");
    }
    body = match (s.bold(), s.italic()) {
        (true, true) => format!("***{body}***"),
        (true, false) => format!("**{body}**"),
        (false, true) => format!("*{body}*"),
        _ => body,
    };
    if let Some(hex) = color(s.font_color()) {
        body = format!("<span style=\"color:{hex}\">{body}</span>");
    }
    format!("{}{}{}", &text[..lead], body, &text[text.len() - tail..])
}

/// 字の色。**自動と黒は色として出さない** ── 出すとノートじゅうが span で
/// 埋まる（貼り付けと同じ決まり・依頼 616）。
fn color(c: Option<onenote_parser::property::common::ColorRef>) -> Option<String> {
    use onenote_parser::property::common::ColorRef;
    match c? {
        ColorRef::Manual { r, g, b } if (r, g, b) != (0, 0, 0) => {
            Some(format!("#{r:02x}{g:02x}{b:02x}"))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/samples").join(rel)
    }

    #[test]
    fn 印に名前を差し込む() {
        let body = format!("a ![]({PIC_OPEN}1{PIC_CLOSE}) b ![]({PIC_OPEN}2{PIC_CLOSE})");
        let body = format!("{body} [f]({FILE_OPEN}1{FILE_CLOSE})");
        let got = fill(&body, &["attachments/x-001.png".into(), "attachments/x-002.jpg".into()], &["attachments/f.docx".into()]);
        assert_eq!(got, "a ![](attachments/x-001.png) b ![](attachments/x-002.jpg) [f](attachments/f.docx)");
    }

    #[test]
    fn 名前の無い印は空にする() {
        let body = format!("![]({PIC_OPEN}9{PIC_CLOSE})");
        assert_eq!(fill(&body, &[], &[]), "![]()");
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

    /// **本物の見本**（FSSHTTP 包み）── 自前の読み手では 0 ページだった。
    #[test]
    fn fsshttp_の見本から_題_見出し_表_絵_数式が出る() {
        let got = open(&sample("fsshttp/New Section 1.one")).expect("開けない");
        assert_eq!(got.units.len(), 1);
        let pages = &got.units[0].pages;
        assert!(!pages.is_empty(), "0 ページ");
        let p = &pages[0];
        assert_eq!(p.title, "Test Page");
        assert!(p.body.contains("ABCDEF"), "{}", p.body);
        // 表は升の中身つきで。
        assert!(p.body.contains("| A | B | C |"), "表が出ない: {}", p.body);
        assert!(p.body.contains("| 1 | 2 | 3 |"), "{}", p.body);
        // 見出しの行は空。
        assert!(p.body.contains("|  |  |  |\n| --- | --- | --- |"), "{}", p.body);
        // 絵は本物の JPEG、本文には印。
        assert_eq!(p.pictures.len(), 1, "絵が出ない");
        assert_eq!(p.pictures[0].ext, "jpg");
        assert!(p.pictures[0].bytes.starts_with(b"\xff\xd8\xff"), "JPEG の頭ではない");
        assert!(p.body.contains(&format!("![]({PIC_OPEN}1{PIC_CLOSE})")), "{}", p.body);
        // 入れ子の箇条書き。
        assert!(p.body.contains("\n  - "), "入れ子の点が出ない: {}", p.body);
        assert!(p.body.contains("\n   1. "), "番号の入れ子は印の幅（3）で: {}", p.body);
        // **箇条書きでない行を字下げしない** ── 空白 4 つでコードの塊になる。
        for l in p.body.lines() {
            let t = l.trim_start();
            if l.len() != t.len() {
                assert!(t.starts_with("- ") || t.starts_with("1. "), "字下げした段落: {l:?}");
            }
        }
        // 箇条書きの後の段落は、一行あけて ── 詰めると最後の項目に呑まれる。
        let at = p.body.rfind("\n1. ").expect("番号が無い");
        let after = &p.body[at + 1..];
        let end = after.find('\n').unwrap();
        assert!(after[end..].starts_with("\n\n"), "箇条書きのすぐ後に段落: {after:.200}");
        // **リンクの印を字として出さない**（試作では `﷟HYPERLINK "…"` と出た）。
        assert!(!p.body.contains('\u{FDDF}'), "{}", p.body);
        assert!(!p.body.contains("HYPERLINK"), "{}", p.body);
        assert!(p.body.contains("](https://example.com)"), "リンクにならない: {}", p.body);
        // 作った日。
        assert_eq!(p.created.len(), 10, "{}", p.created);
    }

    /// **会社の書き出しと同じ形**（公開仕様・MS-ONESTORE 2.3）。
    #[test]
    fn 公開仕様の見本も読める() {
        let got = open(&sample("desktop/OneWithFileData.one")).expect("開けない");
        assert_eq!(got.units.len(), 1);
        let p = &got.units[0].pages[0];
        // 添付は名前だけでなく**中身ごと**（元の OneNote が無くなった日に失われない）。
        assert_eq!(p.files.len(), 1);
        assert_eq!(p.files[0].name, "testing.docx");
        assert!(p.files[0].bytes.starts_with(b"PK"), "docx の中身ではない");
        assert!(p.body.contains(&format!("[testing.docx]({FILE_OPEN}1{FILE_CLOSE})")), "{}", p.body);
    }

    /// 目次つきの一冊を CAB に包む ── **OneNote の「エクスポート」と同じ形。**
    /// 本物の `.onepkg` はよそに置けない（会社のノート）ので、中身は本物の
    /// 一冊、包みだけこちらで作る。
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
        // 一冊の名前は、ファイルの名前から。
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
