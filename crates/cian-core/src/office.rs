//! Fully self-contained text extraction for common document formats, so F3 can
//! preview them without any external converter — the point of cian's offline,
//! single-binary Windows story is that "double-click, F3, read it" works on a
//! machine with nothing else installed.
//!
//! Modern Office files (`.docx` / `.xlsx` / `.pptx`) are ZIP containers of XML,
//! parsed here directly. PDFs have their text pulled from the (usually
//! Flate-compressed) content streams. The legacy binary formats (`.doc` /
//! `.xls` / `.ppt`) fall back to a best-effort readable-text scan — honest
//! about being approximate rather than pretending to render them.
//!
//! None of this reproduces layout. It answers "what does this say", which is
//! what a file-manager preview is for; the viewer it feeds gives search,
//! selection and copy on top for free.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// A document format cian can pull text out of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Doc {
    Docx,
    Xlsx,
    Pptx,
    Pdf,
    /// Pre-2007 Word (`.doc`): OLE binary, best-effort text scan.
    LegacyWord,
    /// Pre-2007 Excel (`.xls`): OLE binary, best-effort text scan.
    LegacyExcel,
    /// Pre-2007 PowerPoint (`.ppt`): OLE binary, best-effort text scan.
    LegacyPpt,
}

impl Doc {
    /// True when extraction is a best-effort scan of a legacy binary format,
    /// so the caller can say so rather than imply a faithful render.
    pub fn is_best_effort(self) -> bool {
        matches!(self, Doc::LegacyWord | Doc::LegacyExcel | Doc::LegacyPpt)
    }

    /// A short human label for the preview header.
    pub fn label(self) -> &'static str {
        match self {
            Doc::Docx => "Word",
            Doc::Xlsx => "Excel",
            Doc::Pptx => "PowerPoint",
            Doc::Pdf => "PDF",
            Doc::LegacyWord => "Word (legacy .doc)",
            Doc::LegacyExcel => "Excel (legacy .xls)",
            Doc::LegacyPpt => "PowerPoint (legacy .ppt)",
        }
    }
}

/// Classify `path` by extension, or `None` if it is not a previewable document.
pub fn classify(path: &Path) -> Option<Doc> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    Some(match ext.as_str() {
        "docx" | "docm" => Doc::Docx,
        "xlsx" | "xlsm" => Doc::Xlsx,
        "pptx" | "pptm" => Doc::Pptx,
        "pdf" => Doc::Pdf,
        "doc" => Doc::LegacyWord,
        // `.xlm` is a typo people make for `.xls`; treat it the same.
        "xls" | "xlm" => Doc::LegacyExcel,
        "ppt" => Doc::LegacyPpt,
        _ => return None,
    })
}

/// One synced folder, and the address the same files have in the cloud.
///
/// The mapping cannot be discovered reliably — a synced library looks like an
/// ordinary directory, and the registry key that records it exists only on
/// Windows and only sometimes — so it is stated in `init.lua` instead of
/// guessed at. Being told is better than being nearly right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncMap {
    /// The local folder the library syncs into.
    pub local: PathBuf,
    /// Its address in SharePoint / OneDrive, without a trailing slash.
    pub url: String,
}

impl SyncMap {
    /// The maps a config declared, with `~` expanded.
    ///
    /// Here rather than in a front end because both of them read the same
    /// `cian.sharepoint{…}` and would otherwise each decide separately what a
    /// leading tilde means.
    pub fn from_pairs(pairs: &[(String, String)]) -> Vec<SyncMap> {
        pairs
            .iter()
            .map(|(local, url)| SyncMap {
                local: expand_home(local),
                url: url.trim_end_matches('/').to_string(),
            })
            .collect()
    }
}

fn expand_home(path: &str) -> PathBuf {
    let Some(rest) = path.strip_prefix('~') else { return PathBuf::from(path) };
    match std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        Some(home) => PathBuf::from(format!("{}{rest}", home.to_string_lossy())),
        None => PathBuf::from(path),
    }
}

/// The cloud address of `path`, if it lives under one of the mapped folders.
///
/// The longest match wins, so a library synced inside another library resolves
/// to the inner one — which is the one the file actually belongs to.
pub fn cloud_url(path: &Path, maps: &[SyncMap]) -> Option<String> {
    let best = maps
        .iter()
        .filter(|m| path.starts_with(&m.local))
        .max_by_key(|m| m.local.as_os_str().len())?;
    let rest = path.strip_prefix(&best.local).ok()?;
    let mut url = best.url.trim_end_matches('/').to_string();
    for part in rest.components() {
        url.push('/');
        url.push_str(&percent_encode(&part.as_os_str().to_string_lossy()));
    }
    Some(url)
}

/// Percent-encode one path segment. Deliberately small: the characters that
/// actually turn up in a document name and would break a URL, and no attempt
/// at a general encoder.
fn percent_encode(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for b in seg.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The URI that opens `url` in the desktop application for `doc`.
///
/// `ofe|u|` is Office's "open for edit" verb: it hands the *cloud* copy to the
/// installed application, which is what makes co-authoring and check-out work.
/// Opening the synced local file instead gets a copy that has to be
/// reconciled later.
pub fn app_uri(doc: Doc, url: &str) -> Option<String> {
    let scheme = match doc {
        Doc::Docx | Doc::LegacyWord => "ms-word",
        Doc::Xlsx | Doc::LegacyExcel => "ms-excel",
        Doc::Pptx | Doc::LegacyPpt => "ms-powerpoint",
        Doc::Pdf => return None,
    };
    Some(format!("{scheme}:ofe|u|{url}"))
}

/// The contents of a Windows `.url` shortcut pointing at `url`.
///
/// Plain text by design: a `.url` file is an INI, it works on every Windows
/// without anything installed, and it survives being mailed to someone.
pub fn url_shortcut(url: &str) -> String {
    format!("[InternetShortcut]\r\nURL={url}\r\n")
}

/// Cap on how much of a PDF we read (they can be enormous, and the preview only
/// needs enough to be useful).
const PDF_READ_LIMIT: u64 = 48 * 1024 * 1024;
/// Cap on extracted lines, so a giant spreadsheet cannot blow up the viewer.
const MAX_LINES: usize = 50_000;

/// Extract `path`'s text as display lines. The `Doc` is returned too so the
/// caller can label the preview and flag best-effort results.
pub fn extract(path: &Path) -> Result<(Doc, Vec<String>)> {
    let doc = classify(path).context("not a previewable document")?;
    let mut lines = match doc {
        Doc::Docx => docx(path)?,
        Doc::Xlsx => xlsx(path)?,
        Doc::Pptx => pptx(path)?,
        Doc::Pdf => pdf(path)?,
        Doc::LegacyWord | Doc::LegacyExcel | Doc::LegacyPpt => legacy(path)?,
    };
    if lines.len() > MAX_LINES {
        lines.truncate(MAX_LINES);
        lines.push(String::new());
        lines.push(format!("… (truncated at {} lines)", MAX_LINES));
    }
    if lines.iter().all(|l| l.trim().is_empty()) {
        lines = vec![
            "(no extractable text)".to_string(),
            String::new(),
            "This document has no text cian can read out — it may be scanned".to_string(),
            "images, or use fonts whose character mapping isn't embedded.".to_string(),
        ];
    }
    Ok((doc, lines))
}

// ─────────────────────────── OOXML (docx/xlsx/pptx) ───────────────────────────

type Zip = zip::ZipArchive<std::fs::File>;

fn open_zip(path: &Path) -> Result<Zip> {
    let f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    zip::ZipArchive::new(f).with_context(|| format!("{} is not a readable zip", path.display()))
}

/// Read a named entry as UTF-8 (OOXML parts are UTF-8), or `None` if absent.
fn entry(zip: &mut Zip, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn docx(path: &Path) -> Result<Vec<String>> {
    let mut zip = open_zip(path)?;
    let xml = entry(&mut zip, "word/document.xml").context("no word/document.xml (not a .docx?)")?;
    let text = ooxml_text(&xml, "w:t", &["w:p"], &["w:br", "w:cr"], &["w:tab"]);
    Ok(tidy_lines(&text))
}

fn pptx(path: &Path) -> Result<Vec<String>> {
    let mut zip = open_zip(path)?;
    let mut names: Vec<String> = zip
        .file_names()
        .filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))
        .map(|s| s.to_string())
        .collect();
    names.sort_by_key(|n| number_in(n));
    let mut out = Vec::new();
    for (idx, name) in names.iter().enumerate() {
        let Some(xml) = entry(&mut zip, name) else { continue };
        let text = ooxml_text(&xml, "a:t", &["a:p"], &["a:br"], &[]);
        out.push(format!("── Slide {} ──", idx + 1));
        out.extend(tidy_lines(&text));
        out.push(String::new());
    }
    Ok(out)
}

fn xlsx(path: &Path) -> Result<Vec<String>> {
    let mut zip = open_zip(path)?;
    let shared = entry(&mut zip, "xl/sharedStrings.xml")
        .map(|x| shared_strings(&x))
        .unwrap_or_default();
    let titles = entry(&mut zip, "xl/workbook.xml")
        .map(|x| workbook_sheet_names(&x))
        .unwrap_or_default();
    let mut names: Vec<String> = zip
        .file_names()
        .filter(|n| n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml"))
        .map(|s| s.to_string())
        .collect();
    names.sort_by_key(|n| number_in(n));
    let mut out = Vec::new();
    for (idx, name) in names.iter().enumerate() {
        let Some(xml) = entry(&mut zip, name) else { continue };
        let title = titles.get(idx).cloned().unwrap_or_else(|| format!("Sheet{}", idx + 1));
        out.push(format!("── {} ──", title));
        out.extend(sheet_lines(&xml, &shared));
        out.push(String::new());
    }
    Ok(out)
}

/// A tiny tag-walking extractor shared by Word and PowerPoint: collect the text
/// inside `text_el`, insert a newline at each `para_end` close tag and each
/// `br` tag, and a tab at each `tab` tag. Not a real XML parser — but OOXML text
/// runs are regular enough that this reads them faithfully.
fn ooxml_text(xml: &str, text_el: &str, para_ends: &[&str], breaks: &[&str], tabs: &[&str]) -> String {
    let mut out = String::new();
    let mut depth = 0u32; // nesting depth inside text_el
    let b = xml.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'<' {
            let Some(rel) = xml[i..].find('>') else { break };
            let close = i + rel;
            let inner = &xml[i + 1..close]; // e.g. "w:t xml:space=\"preserve\"", "/w:p", "w:br/"
            let is_close = inner.starts_with('/');
            let self_closing = inner.ends_with('/');
            let name = tag_name(inner);
            if name == text_el {
                if is_close {
                    depth = depth.saturating_sub(1);
                } else if !self_closing {
                    depth += 1;
                }
            } else if (is_close && para_ends.contains(&name)) || breaks.contains(&name) {
                out.push('\n');
            } else if tabs.contains(&name) {
                out.push('\t');
            }
            i = close + 1;
        } else {
            let next = xml[i..].find('<').map(|r| i + r).unwrap_or(b.len());
            if depth > 0 {
                out.push_str(&decode_entities(&xml[i..next]));
            }
            i = next;
        }
    }
    out
}

/// The element name from a tag's interior, stripped of a leading `/` and any
/// attributes: `"w:t xml:space=…"` → `"w:t"`, `"/w:p"` → `"w:p"`.
fn tag_name(inner: &str) -> &str {
    let s = inner.strip_prefix('/').unwrap_or(inner);
    let end = s
        .find([' ', '/', '\t', '\n', '\r', '>'])
        .unwrap_or(s.len());
    &s[..end]
}

/// The trailing integer in a path like `slide12.xml` / `sheet3.xml`, for order.
fn number_in(name: &str) -> u32 {
    let digits: String = name
        .rsplit(|c: char| !c.is_ascii_digit())
        .find(|s| !s.is_empty())
        .unwrap_or("0")
        .to_string();
    digits.parse().unwrap_or(0)
}

/// Shared-string table: one string per `<si>`, concatenating its `<t>` runs.
fn shared_strings(xml: &str) -> Vec<String> {
    between_all(xml, "<si>", "</si>")
        .iter()
        .map(|si| tag_contents(si, "t").join(""))
        .collect()
}

/// Sheet display names in workbook order, from `xl/workbook.xml`'s `<sheet>`s.
fn workbook_sheet_names(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(rel) = xml[i..].find("<sheet ") {
        let start = i + rel;
        let Some(gt) = xml[start..].find('>') else { break };
        let tag = &xml[start..start + gt];
        if let Some(name) = attr(tag, "name") {
            out.push(decode_entities(&name));
        }
        i = start + gt + 1;
    }
    out
}

/// A worksheet's rows as `│`-separated cell lines.
fn sheet_lines(xml: &str, shared: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    for row in between_all(xml, "<row", "</row>") {
        let mut cells = row_cells(&row, shared);
        cells.sort_by_key(|(c, _)| *c);
        // Drop trailing empties so a mostly-blank row isn't a wall of separators.
        while cells.last().map(|(_, v)| v.trim().is_empty()).unwrap_or(false) {
            cells.pop();
        }
        lines.push(cells.iter().map(|(_, v)| v.replace('\n', " ")).collect::<Vec<_>>().join(" │ "));
    }
    lines
}

/// Parse a `<row>`'s cells into `(column-index, value)` pairs.
fn row_cells(row: &str, shared: &[String]) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(rel) = row[i..].find("<c") {
        let cstart = i + rel;
        let after = row[cstart + 2..].chars().next();
        if !matches!(after, Some(' ') | Some('>') | Some('/')) {
            i = cstart + 2;
            continue;
        }
        let Some(gtrel) = row[cstart..].find('>') else { break };
        let gt = cstart + gtrel;
        let opentag = &row[cstart..=gt];
        let col = attr(opentag, "r").map(|r| col_index(&r)).unwrap_or(out.len() as u32);
        let typ = attr(opentag, "t").unwrap_or_default();
        if opentag.ends_with("/>") {
            i = gt + 1;
            continue; // empty self-closed cell
        }
        let cend = row[gt..].find("</c>").map(|r| gt + r).unwrap_or(row.len());
        let body = &row[gt + 1..cend];
        let val = match typ.as_str() {
            "s" => tag_contents(body, "v")
                .first()
                .and_then(|s| s.trim().parse::<usize>().ok())
                .and_then(|idx| shared.get(idx).cloned())
                .unwrap_or_default(),
            "inlineStr" | "str" => tag_contents(body, "t").join(""),
            _ => tag_contents(body, "v").first().cloned().unwrap_or_default(),
        };
        out.push((col, val));
        i = cend + 4;
    }
    out
}

/// Column index from a cell ref: `"A1"` → 0, `"B7"` → 1, `"AA3"` → 26.
fn col_index(cell_ref: &str) -> u32 {
    let mut n: u32 = 0;
    for c in cell_ref.chars() {
        if c.is_ascii_alphabetic() {
            n = n * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
        } else {
            break;
        }
    }
    n.saturating_sub(1)
}

/// The value of attribute `key` in an opening tag, e.g. `attr("<c r=\"A1\">", "r")`.
fn attr(tag: &str, key: &str) -> Option<String> {
    let pat = format!("{}=\"", key);
    let start = tag.find(&pat)? + pat.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

/// The text inside every `<name …>…</name>` element (self-closing tags yield
/// nothing). Entity-decoded. Used for `<t>` and `<v>` in spreadsheet XML.
fn tag_contents(xml: &str, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let open = format!("<{}", name);
    let close = format!("</{}>", name);
    let mut i = 0;
    while let Some(rel) = xml[i..].find(&open) {
        let start = i + rel + open.len();
        let after = xml[start..].chars().next();
        if !matches!(after, Some('>') | Some(' ') | Some('/') | Some('\t') | Some('\n') | Some('\r')) {
            i = start; // e.g. matched "<title" while looking for "<t"
            continue;
        }
        let Some(gt) = xml[start..].find('>') else { break };
        let content_start = start + gt + 1;
        if xml[start..content_start].ends_with("/>") {
            i = content_start;
            continue;
        }
        let Some(cl) = xml[content_start..].find(&close) else { break };
        out.push(decode_entities(&xml[content_start..content_start + cl]));
        i = content_start + cl + close.len();
    }
    out
}

/// Every substring bracketed by `open` … `close`, in order (non-nesting).
fn between_all(xml: &str, open: &str, close: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(rel) = xml[i..].find(open) {
        let start = i + rel + open.len();
        let Some(crel) = xml[start..].find(close) else { break };
        out.push(xml[start..start + crel].to_string());
        i = start + crel + close.len();
    }
    out
}

/// Decode the XML entities that appear in OOXML text.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        if let Some(semi) = tail.find(';') {
            let ent = &tail[1..semi];
            match ent {
                "amp" => out.push('&'),
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                    if let Some(c) = u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32) {
                        out.push(c);
                    }
                }
                _ if ent.starts_with('#') => {
                    if let Some(c) = ent[1..].parse::<u32>().ok().and_then(char::from_u32) {
                        out.push(c);
                    }
                }
                _ => {
                    out.push('&');
                    out.push_str(ent);
                    out.push(';');
                }
            }
            rest = &tail[semi + 1..];
        } else {
            out.push('&');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// Collapse a raw extracted block into display lines: trim trailing spaces,
/// expand nothing (the viewer expands tabs), and squeeze runs of blank lines.
fn tidy_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end().to_string();
        if line.is_empty() && out.last().map(|l| l.is_empty()).unwrap_or(true) {
            continue; // no leading blank, no doubled blanks
        }
        out.push(line);
    }
    while out.last().map(|l| l.is_empty()).unwrap_or(false) {
        out.pop();
    }
    out
}

// ─────────────────────────────────── PDF ───────────────────────────────────

fn pdf(path: &Path) -> Result<Vec<String>> {
    let mut f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut data = Vec::new();
    f.by_ref().take(PDF_READ_LIMIT).read_to_end(&mut data)?;

    let mut text = String::new();
    let mut i = 0;
    while let Some(rel) = find_sub(&data[i..], b"stream") {
        let kw = i + rel;
        // The stream's dictionary is just before it; a Flate filter shows there.
        let dict = &data[kw.saturating_sub(500)..kw];
        let flate = window_contains(dict, b"FlateDecode");
        // Data starts after "stream" and its EOL (CR?, LF).
        let mut ds = kw + 6;
        if data.get(ds) == Some(&b'\r') {
            ds += 1;
        }
        if data.get(ds) == Some(&b'\n') {
            ds += 1;
        }
        let Some(erel) = find_sub(&data[ds..], b"endstream") else { break };
        let raw = &data[ds..ds + erel];
        let decoded = if flate { inflate(raw).unwrap_or_default() } else { raw.to_vec() };
        // Only content streams carry showable text; skip fonts, images, etc.
        if window_contains(&decoded, b"BT") || window_contains(&decoded, b"Tj") || window_contains(&decoded, b"TJ") {
            text.push_str(&pdf_content_text(&decoded));
            text.push('\n');
        }
        i = ds + erel + 9;
    }
    Ok(tidy_lines(&text))
}

/// Text from one decoded PDF content stream: the `(…)` string literals, with a
/// newline at the line-positioning operators (`Td`, `TD`, `T*`, `'`, `"`).
/// CID/Type0 text (hex strings under custom CMaps) is out of scope — this is the
/// pragmatic path that reads the many PDFs which use simple text showing.
fn pdf_content_text(b: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'(' => {
                let (s, ni) = read_pdf_string(b, i + 1);
                out.push_str(&s);
                i = ni;
            }
            b'\'' | b'"' => {
                out.push('\n');
                i += 1;
            }
            b'T' if i + 1 < b.len() => {
                match b[i + 1] {
                    b'*' | b'd' | b'D' => out.push('\n'),
                    _ => {}
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    out
}

/// Read a PDF literal string starting just after `(`, honouring `\` escapes and
/// balanced parentheses. Bytes are taken as Latin-1 (a good default for the
/// WinAnsi/PdfDoc text most simple PDFs use).
fn read_pdf_string(b: &[u8], mut i: usize) -> (String, usize) {
    let mut bytes = Vec::new();
    let mut depth = 1;
    while i < b.len() {
        match b[i] {
            b'\\' => {
                i += 1;
                if i >= b.len() {
                    break;
                }
                match b[i] {
                    b'n' => bytes.push(b'\n'),
                    b'r' => bytes.push(b'\r'),
                    b't' => bytes.push(b'\t'),
                    b'b' => bytes.push(8),
                    b'f' => bytes.push(12),
                    b'(' => bytes.push(b'('),
                    b')' => bytes.push(b')'),
                    b'\\' => bytes.push(b'\\'),
                    b'\n' => {} // line continuation
                    b'\r' => {
                        if b.get(i + 1) == Some(&b'\n') {
                            i += 1;
                        }
                    }
                    d @ b'0'..=b'7' => {
                        let mut val = (d - b'0') as u32;
                        let mut k = 1;
                        while k < 3 && i + 1 < b.len() && (b'0'..=b'7').contains(&b[i + 1]) {
                            i += 1;
                            val = val * 8 + (b[i] - b'0') as u32;
                            k += 1;
                        }
                        bytes.push(val as u8);
                    }
                    other => bytes.push(other),
                }
                i += 1;
            }
            b'(' => {
                depth += 1;
                bytes.push(b'(');
                i += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    i += 1;
                    break;
                }
                bytes.push(b')');
                i += 1;
            }
            c => {
                bytes.push(c);
                i += 1;
            }
        }
    }
    (bytes.iter().map(|&c| c as char).collect(), i)
}

/// zlib-inflate a Flate-compressed PDF stream (rust_backend miniz — no C).
fn inflate(raw: &[u8]) -> Option<Vec<u8>> {
    let mut d = flate2::read::ZlibDecoder::new(raw);
    let mut out = Vec::new();
    d.read_to_end(&mut out).ok()?;
    Some(out)
}

// ────────────────────────────── legacy binary ──────────────────────────────

/// Best-effort text from a pre-2007 OLE binary (`.doc` / `.xls` / `.ppt`): the
/// printable runs, tried both as UTF-16LE (Word stores text that way) and as
/// Latin-1, keeping whichever yields more readable text.
fn legacy(path: &Path) -> Result<Vec<String>> {
    let data = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let utf16 = printable_runs_utf16le(&data);
    let latin = printable_runs_latin1(&data);
    let pick = if utf16.chars().filter(|c| !c.is_whitespace()).count()
        >= latin.chars().filter(|c| !c.is_whitespace()).count()
    {
        utf16
    } else {
        latin
    };
    Ok(tidy_lines(&pick))
}

/// Runs of ≥4 printable UTF-16LE characters, each run on its own line.
fn printable_runs_utf16le(data: &[u8]) -> String {
    let mut out = String::new();
    let mut run = String::new();
    let mut i = 0;
    while i + 1 < data.len() {
        let u = u16::from_le_bytes([data[i], data[i + 1]]);
        i += 2;
        match char::from_u32(u as u32) {
            Some(c) if is_readable(c) => run.push(c),
            _ => flush_run(&mut out, &mut run),
        }
    }
    flush_run(&mut out, &mut run);
    out
}

/// Runs of ≥4 printable Latin-1 bytes, each run on its own line.
fn printable_runs_latin1(data: &[u8]) -> String {
    let mut out = String::new();
    let mut run = String::new();
    for &b in data {
        let c = b as char;
        if is_readable(c) {
            run.push(c);
        } else {
            flush_run(&mut out, &mut run);
        }
    }
    flush_run(&mut out, &mut run);
    out
}

fn is_readable(c: char) -> bool {
    c == ' ' || c == '\t' || (!c.is_control() && !c.is_whitespace() && c as u32 >= 0x20)
}

fn flush_run(out: &mut String, run: &mut String) {
    let trimmed = run.trim();
    if trimmed.chars().count() >= 4 {
        out.push_str(trimmed);
        out.push('\n');
    }
    run.clear();
}

// ─────────────────────────────── byte helpers ───────────────────────────────

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn window_contains(hay: &[u8], needle: &[u8]) -> bool {
    find_sub(hay, needle).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(local: &str, url: &str) -> SyncMap {
        SyncMap { local: PathBuf::from(local), url: url.to_string() }
    }

    #[test]
    fn a_synced_file_resolves_to_its_address_in_the_library() {
        let maps = vec![
            map("/Users/t/OneDrive - Corp", "https://corp.sharepoint.com/Shared%20Documents"),
            // A library synced inside another one: the inner mapping is the
            // one the file belongs to, so the longest match has to win.
            map("/Users/t/OneDrive - Corp/Team", "https://corp.sharepoint.com/sites/Team/Docs"),
        ];
        assert_eq!(
            cloud_url(&PathBuf::from("/Users/t/OneDrive - Corp/plan.docx"), &maps).as_deref(),
            Some("https://corp.sharepoint.com/Shared%20Documents/plan.docx"),
        );
        assert_eq!(
            cloud_url(&PathBuf::from("/Users/t/OneDrive - Corp/Team/q3 report.xlsx"), &maps).as_deref(),
            Some("https://corp.sharepoint.com/sites/Team/Docs/q3%20report.xlsx"),
            "the inner library, and the space encoded",
        );
        // Somewhere else entirely: no address to give.
        assert_eq!(cloud_url(&PathBuf::from("/tmp/plan.docx"), &maps), None);
    }

    #[test]
    fn the_uri_names_the_application_and_asks_to_edit() {
        let u = "https://corp.sharepoint.com/Shared%20Documents/plan.docx";
        assert_eq!(app_uri(Doc::Docx, u).as_deref(), Some("ms-word:ofe|u|https://corp.sharepoint.com/Shared%20Documents/plan.docx"));
        assert!(app_uri(Doc::Xlsx, u).unwrap().starts_with("ms-excel:ofe|u|"));
        assert!(app_uri(Doc::LegacyPpt, u).unwrap().starts_with("ms-powerpoint:ofe|u|"));
        // A PDF has no Office application to hand it to.
        assert_eq!(app_uri(Doc::Pdf, u), None);
    }

    #[test]
    fn a_url_shortcut_is_an_ini_file() {
        let s = url_shortcut("https://example.invalid/x.docx");
        assert!(s.starts_with("[InternetShortcut]"));
        assert!(s.contains("URL=https://example.invalid/x.docx"));
        assert!(s.ends_with("\r\n"), "CRLF, as Windows writes them");
    }

        use std::io::Write;

    /// Write a minimal zip with the given (name, contents) members.
    fn make_zip(path: &Path, members: &[(&str, &str)]) {
        let f = std::fs::File::create(path).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in members {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(body.as_bytes()).unwrap();
        }
        zw.finish().unwrap();
    }

    #[test]
    fn docx_paragraphs_and_entities() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.docx");
        let doc = r#"<w:document><w:body>
            <w:p><w:r><w:t xml:space="preserve">Hello </w:t></w:r><w:r><w:t>world &amp; more</w:t></w:r></w:p>
            <w:p><w:r><w:t>Second line</w:t></w:r></w:p>
        </w:body></w:document>"#;
        make_zip(&p, &[("word/document.xml", doc)]);
        let (kind, lines) = extract(&p).unwrap();
        assert_eq!(kind, Doc::Docx);
        assert_eq!(lines[0], "Hello world & more");
        assert_eq!(lines[1], "Second line");
    }

    #[test]
    fn xlsx_shared_strings_and_numbers() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.xlsx");
        let shared = r#"<sst><si><t>Name</t></si><si><t>Age</t></si><si><t>Ada</t></si></sst>"#;
        let workbook = r#"<workbook><sheets><sheet name="People" sheetId="1"/></sheets></workbook>"#;
        let sheet = r#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
            <row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><v>36</v></c></row>
        </sheetData></worksheet>"#;
        make_zip(
            &p,
            &[
                ("xl/sharedStrings.xml", shared),
                ("xl/workbook.xml", workbook),
                ("xl/worksheets/sheet1.xml", sheet),
            ],
        );
        let (kind, lines) = extract(&p).unwrap();
        assert_eq!(kind, Doc::Xlsx);
        assert!(lines.iter().any(|l| l == "── People ──"), "sheet title shown");
        assert!(lines.iter().any(|l| l == "Name │ Age"), "header row: {:?}", lines);
        assert!(lines.iter().any(|l| l == "Ada │ 36"), "data row: {:?}", lines);
    }

    #[test]
    fn pptx_slides_in_order() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.pptx");
        let s1 = r#"<p:sld><p:txBody><a:p><a:r><a:t>First</a:t></a:r></a:p></p:txBody></p:sld>"#;
        let s2 = r#"<p:sld><p:txBody><a:p><a:r><a:t>Second</a:t></a:r></a:p></p:txBody></p:sld>"#;
        // Deliberately add out of filename order to prove numeric sort.
        make_zip(&p, &[("ppt/slides/slide2.xml", s2), ("ppt/slides/slide1.xml", s1)]);
        let (_, lines) = extract(&p).unwrap();
        let joined = lines.join("\n");
        let first = joined.find("First").unwrap();
        let second = joined.find("Second").unwrap();
        assert!(first < second, "slide 1 before slide 2: {:?}", lines);
        assert!(lines.iter().any(|l| l == "── Slide 1 ──"));
    }

    #[test]
    fn pdf_uncompressed_text() {
        // A hand-built PDF with one uncompressed content stream showing text.
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.pdf");
        let body = b"%PDF-1.4\n4 0 obj\n<< /Length 44 >>\nstream\nBT /F1 12 Tf (Hello PDF) Tj T* (line two) Tj ET\nendstream\nendobj\n";
        std::fs::write(&p, body).unwrap();
        let (kind, lines) = extract(&p).unwrap();
        assert_eq!(kind, Doc::Pdf);
        let joined = lines.join("\n");
        assert!(joined.contains("Hello PDF"), "got: {:?}", lines);
        assert!(joined.contains("line two"), "got: {:?}", lines);
    }

    #[test]
    fn classify_by_extension() {
        assert_eq!(classify(Path::new("a.docx")), Some(Doc::Docx));
        assert_eq!(classify(Path::new("a.XLSX")), Some(Doc::Xlsx));
        assert_eq!(classify(Path::new("a.doc")), Some(Doc::LegacyWord));
        assert!(Doc::LegacyWord.is_best_effort());
        assert!(!Doc::Docx.is_best_effort());
        assert_eq!(classify(Path::new("a.txt")), None);
    }

    #[test]
    fn column_indices() {
        assert_eq!(col_index("A1"), 0);
        assert_eq!(col_index("B10"), 1);
        assert_eq!(col_index("Z1"), 25);
        assert_eq!(col_index("AA1"), 26);
    }
}
