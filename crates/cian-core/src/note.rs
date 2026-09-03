//! A note: a Markdown file that says a little about itself.
//!
//! The whole of cian mode rests on one decision — **the notes are plain files
//! and there is no database.** Everything else follows from it:
//!
//!   * a OneNote migration is a script that writes files, not an import format
//!   * a SharePoint library, a synced OneDrive folder or a Dropbox folder is a
//!     notes folder with nothing added
//!   * crmaine can index them, because they are text on a disk
//!   * and nothing here is a lock-in: the exit is `ls`
//!
//! What a note knows about itself lives in YAML front matter, the convention
//! every static-site generator and every notes app already reads:
//!
//! ```text
//! ---
//! title: 移行の段取り
//! tags: [onenote, 2026]
//! created: 2026-09-02
//! ---
//! # 移行の段取り
//! ```
//!
//! **This module is in `cian-core` on purpose.** Electron does not run on iOS,
//! so an iPhone build would be a third front end and the only thing that could
//! cross is what lives here — pure Rust, no I/O beyond reading a file, no UI.
//! Putting note logic in a pane or a renderer would be cheap today and
//! expensive exactly once.
//!
//! **The YAML read here is a subset, deliberately.** `key: value`, and lists
//! written either `[a, b]` or as `- a` lines. Anchors, nested maps, multi-line
//! scalars and flow maps are not understood — a note that uses them keeps its
//! front matter as text and simply reports no tags, rather than a parser
//! written for a fifth of YAML guessing at the other four.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A note's front matter, and where the body starts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Front {
    /// `key: value` pairs, in the order YAML happens to give them (sorted, so
    /// two reads of one file cannot differ).
    pub fields: BTreeMap<String, String>,
    /// `tags:` read as a list, whichever of the two spellings was used.
    pub tags: Vec<String>,
    /// How many lines the block occupied, fences included. `0` when there was
    /// none — which is the common case and must not be an error.
    pub lines: usize,
}

impl Front {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }
}

/// Read the front matter off the top of a file's lines.
///
/// A block only counts at the very top, and only when it closes. A stray `---`
/// in the middle of a document is a horizontal rule, and an unclosed one at the
/// top is a document that happens to start with a rule — treating either as
/// front matter would silently swallow the beginning of somebody's note.
pub fn front(lines: &[String]) -> Front {
    let mut out = Front::default();
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return out;
    }
    let Some(end) = lines.iter().skip(1).position(|l| {
        let t = l.trim_end();
        t == "---" || t == "..."
    }) else {
        return out;
    };
    let end = end + 1;
    out.lines = end + 1;
    let mut list_key: Option<String> = None;
    for raw in &lines[1..end] {
        let line = raw.trim_end();
        // `  - value` continues whichever key opened the list.
        if let Some(item) = line.trim_start().strip_prefix("- ") {
            if let Some(k) = &list_key {
                if k == "tags" {
                    out.tags.push(unquote(item.trim()).to_string());
                }
            }
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let key = k.trim().to_ascii_lowercase();
        let val = v.trim();
        if val.is_empty() {
            list_key = Some(key);
            continue;
        }
        list_key = None;
        if key == "tags" {
            out.tags = split_list(val);
            continue;
        }
        out.fields.insert(key, unquote(val).to_string());
    }
    out
}

/// `[a, b]` or `a, b` → the items. Quotes come off; empties are dropped.
fn split_list(v: &str) -> Vec<String> {
    let inner = v.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|x| unquote(x.trim()).to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// One note, as a list needs to show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub path: PathBuf,
    /// The front matter's `title`, else the first heading, else the file name
    /// without its extension. Never empty — a list of blanks is not a list.
    pub title: String,
    /// The first few lines of body text, flattened, for the second line of a
    /// row. Headings, fences and front matter are left out: they say what the
    /// note is *made of* rather than what it is about.
    pub excerpt: String,
    pub tags: Vec<String>,
    /// `updated` from the front matter if it has one, else the file's mtime as
    /// seconds since the epoch. Formatting belongs to whoever is drawing.
    pub updated: Option<u64>,
    pub bytes: u64,
}

/// Read one note. Only the head of the file is looked at — a list of two
/// hundred notes must not read two hundred whole files to draw itself.
pub fn read(path: &Path, head_lines: usize) -> Option<Note> {
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<String> = text.lines().take(head_lines.max(8)).map(str::to_string).collect();
    let meta = std::fs::metadata(path).ok();
    let f = front(&lines);
    let body = &lines[f.lines.min(lines.len())..];
    let title = f
        .get("title")
        .map(str::to_string)
        .filter(|t| !t.trim().is_empty())
        .or_else(|| heading(body))
        .or_else(|| {
            path.file_stem().map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "(no name)".to_string());
    let updated = f
        .get("updated")
        .and_then(date_secs)
        .or_else(|| {
            meta.as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
        });
    Some(Note {
        path: path.to_path_buf(),
        title,
        excerpt: excerpt(body),
        tags: f.tags,
        updated,
        bytes: meta.map(|m| m.len()).unwrap_or(0),
    })
}

/// The first `# heading`, if the note leads with one.
fn heading(body: &[String]) -> Option<String> {
    body.iter()
        .map(|l| l.trim())
        .find(|l| l.starts_with('#'))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .filter(|t| !t.is_empty())
}

/// A line of body text, flattened.
fn excerpt(body: &[String]) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for line in body {
        let t = line.trim();
        if t.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || t.is_empty() || t.starts_with('#') || t.starts_with("---") {
            continue;
        }
        // A picture is not a sentence. `![](attachments/note-1788450324680.jpg)`
        // is forty characters of filename in a line meant to remind you what
        // the note is about, and a note that opens with a screenshot showed
        // nothing else at all.
        let t = strip_images(t);
        let t = t.trim();
        if t.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(t);
        if out.chars().count() >= 120 {
            break;
        }
    }
    out.chars().take(120).collect()
}

/// Take `![alt](link)` out of a line, keeping the alt text if there is any.
///
/// Only images — a plain `[text](link)` is words somebody wrote and reads
/// perfectly well in an excerpt.
fn strip_images(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("![") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        // `![alt](link)` — both halves have to be there, or it is just text
        // that happens to start with an exclamation mark.
        let Some(close) = after.find(']') else { break };
        let tail = &after[close + 1..];
        if !tail.starts_with('(') {
            out.push_str(&rest[at..at + 2]);
            rest = after;
            continue;
        }
        let Some(end) = tail.find(')') else { break };
        // The alt text is what the writer chose to call the picture, so it
        // belongs in an excerpt; the filename does not.
        out.push_str(&after[..close]);
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// `2026-09-02` or `2026-09-02T10:00:00` → seconds since the epoch.
///
/// Days from the civil calendar, no time zone. A note's `updated` is a date
/// somebody typed; pretending to know which hour of it they meant, or in whose
/// zone, would be inventing precision.
fn date_secs(s: &str) -> Option<u64> {
    let d = s.trim();
    let d = d.split(['T', ' ']).next()?;
    let mut it = d.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let day: i64 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days.checked_mul(86_400)?).ok()
}

/// Windows keeps eleven names for devices, and a file cannot have one of them
/// whatever the extension. A note titled "CON" is not a silly case: it is an
/// abbreviation people write.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A title, turned into a filename that every filesystem cian runs on accepts.
///
/// Not a slug: the titles here are Japanese as often as not, and stripping a
/// title to ASCII would leave most notes named `.md`. What it removes is only
/// what a filesystem refuses — the nine characters Windows reserves, control
/// characters, and the trailing dot or space Explorer silently eats.
///
/// The cap is in **characters and on a char boundary**, but chosen for bytes:
/// 60 Japanese characters is 180 bytes, comfortably inside the 255 that ext4,
/// APFS and NTFS all stop at.
pub fn file_stem(title: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in title.trim().chars() {
        let bad = matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            || c.is_control();
        if bad {
            gap = true;
            continue;
        }
        // A run of refused characters collapses to one `-`, and never opens
        // the name: `?? notes` should be `notes`, not `- notes`.
        if gap && !out.is_empty() {
            out.push('-');
        }
        gap = false;
        if out.chars().count() >= 60 {
            break;
        }
        out.push(c);
    }
    let out = out.trim_matches([' ', '.', '\u{3000}']).to_string();
    if out.is_empty() {
        return String::new();
    }
    // `CON.md` is still CON to Windows. So is `con`.
    let head = out.split('.').next().unwrap_or(&out).to_ascii_uppercase();
    if RESERVED.contains(&head.as_str()) {
        return format!("_{out}");
    }
    out
}

/// One line to search a note by: its title, its tags, and the start of it.
///
/// A listing filters on the filename, which finds a note only if you named
/// the file what the note is about. That holds for notes cian made and for
/// nothing else — an imported page is `page-0012.md` with a title inside it,
/// and a tag is never in a filename at all.
///
/// Tags keep their `#`, so `#仕事` narrows to the tag and `仕事` also finds
/// the ones that merely say it. Lowercased once here rather than at every
/// keystroke of the filter.
pub fn haystack(n: &Note) -> String {
    let mut s = String::with_capacity(n.title.len() + n.excerpt.len() + 16);
    s.push_str(&n.title);
    for t in &n.tags {
        s.push_str(" #");
        s.push_str(t);
    }
    s.push(' ');
    s.push_str(&n.excerpt);
    s.to_lowercase()
}

/// A note, and where it sits under the folder that was walked.
pub struct Found {
    /// Path relative to the walked folder, with `/` separators.
    pub rel: String,
    pub note: Note,
}

/// Every Markdown note under `dir`, with the walk's own account of itself.
///
/// Here rather than in the engine because there are two callers now — the
/// window asks over a pipe, and a phone will ask over a C ABI — and "what
/// counts as a note" written twice is two answers that drift. The rules are
/// the whole content: directories are not notes, a `.md`/`.markdown` suffix
/// is, and the first sixty lines are enough to know a title from an excerpt.
pub fn list(
    dir: &std::path::Path,
    limits: crate::survey::Limits,
    stop: &std::sync::atomic::AtomicBool,
) -> (Vec<Found>, crate::survey::Survey) {
    let found = crate::survey::survey(dir, limits, stop);
    let mut out = Vec::new();
    for r in &found.rows {
        if r.is_dir {
            continue;
        }
        let md = r
            .path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
            .unwrap_or(false);
        if !md {
            continue;
        }
        let Some(note) = read(&r.path, 60) else { continue };
        out.push(Found { rel: r.rel.clone(), note });
    }
    (out, found)
}

/// Make a note in `dir` and say where it went.
///
/// The name that is free, not the name that was free a moment ago:
/// `create_new` fails if the file appeared between the check and the write,
/// and two people on one shared folder is the case this whole mode exists
/// for. Shared with the engine for the same reason as [`list`].
pub fn create(dir: &std::path::Path, title: &str, today: &str) -> anyhow::Result<std::path::PathBuf> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let (name, body) = new_note(title, today);
    let stem = name.trim_end_matches(".md").to_string();
    let mut at = dir.join(&name);
    let mut n = 2;
    let mut file = loop {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&at) {
            Ok(f) => break f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n <= 99 => {
                at = dir.join(format!("{stem}-{n}.md"));
                n += 1;
            }
            Err(e) => return Err(e.into()),
        }
    };
    file.write_all(body.as_bytes())?;
    Ok(at)
}

/// Put a picture beside a note, and say what to write in the text.
///
/// `attachments/` next to the note, not a folder of its own per note and not
/// a database: the whole point of cian mode is that a notes folder is a
/// folder, and somebody looking at it from the Mac — or from Explorer, or
/// from a phone's Files — should see what they expect.
///
/// The name carries the note's own and the clock. **Not a counter**: the
/// folder is shared with everything else attached there, and a counter
/// eventually picks a name that already exists.
///
/// Shared with the engine because the phone attaches photos and the window
/// pastes screenshots, and the two must land in the same place with the same
/// kind of name — otherwise a notes folder written from both looks like two
/// folders that happen to overlap.
pub fn attach(note: &std::path::Path, bytes: &[u8], ext: &str) -> anyhow::Result<String> {
    if bytes.is_empty() {
        anyhow::bail!("画像が空です");
    }
    let Some(dir) = note.parent() else {
        anyhow::bail!("そのノートの置き場所が分かりません")
    };
    let ext = match ext.trim().trim_start_matches('.') {
        "" => "png".to_string(),
        e => e.to_ascii_lowercase(),
    };
    let at = dir.join("attachments");
    std::fs::create_dir_all(&at)?;
    let stem = note
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "note".into());
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let name = format!("{}-{stamp}.{ext}", file_stem(&stem));
    std::fs::write(at.join(&name), bytes)?;
    // Relative, with forward slashes: this goes into a Markdown link, which is
    // a URL and not a Windows path.
    Ok(format!("attachments/{name}"))
}

/// One piece of a note, as something that can be drawn.
///
/// Blocks, not a tree: a note is read top to bottom, and every renderer this
/// has to feed — a phone's list of views, the window, whatever a tablet turns
/// out to be — lays out a sequence. Anything that needs nesting (a list inside
/// a quote) is rare enough in a notebook to be worth losing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading { level: u8, text: String },
    Paragraph(String),
    /// One item. `text` keeps its inline markup for the renderer to handle.
    Bullet(String),
    Numbered { n: u32, text: String },
    Quote(String),
    /// Verbatim, including the blank lines inside it. `lang` may be empty.
    Code { lang: String, text: String },
    /// `![alt](link)` on a line of its own — the only image that gets a block.
    /// One inside a sentence stays in the sentence, where it was written.
    Image { alt: String, link: String },
    Rule,
}

/// Split a note into things to draw, skipping its front matter.
///
/// **Here rather than in Swift.** A renderer written on the phone is a
/// renderer no test can reach, and "what is a heading" is exactly the kind of
/// question that drifts. What the phone does with a `Heading` is the phone's
/// business; whether a line *is* one is not.
///
/// The front matter goes because it is how the note describes itself, not
/// something it says — the title and the tags are already on screen.
pub fn blocks(text: &str) -> Vec<Block> {
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let start = front(&lines).lines;
    let mut out = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let mut i = start;

    fn flush(para: &mut Vec<String>, out: &mut Vec<Block>) {
        if !para.is_empty() {
            out.push(Block::Paragraph(para.join(" ")));
            para.clear();
        }
    }

    while i < lines.len() {
        let raw = &lines[i];
        let t = raw.trim();

        // A fence runs to its closing fence, or to the end of the note — an
        // unclosed one is a mistake somebody made, and swallowing the rest of
        // the file is friendlier than pretending each line is a paragraph.
        if let Some(lang) = t.strip_prefix("```") {
            flush(&mut para, &mut out);
            let lang = lang.trim().to_string();
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                body.push(lines[i].clone());
                i += 1;
            }
            i += 1; // the closing fence, or past the end
            out.push(Block::Code { lang, text: body.join("\n") });
            continue;
        }

        if t.is_empty() {
            flush(&mut para, &mut out);
            i += 1;
            continue;
        }

        // `---` is a rule here and not front matter: the front matter was
        // taken off the top before this loop began.
        if t == "---" || t == "***" || t == "___" {
            flush(&mut para, &mut out);
            out.push(Block::Rule);
            i += 1;
            continue;
        }

        if let Some(rest) = t.strip_prefix('#') {
            let level = 1 + rest.chars().take_while(|c| *c == '#').count();
            let text = rest.trim_start_matches('#').trim();
            // `#tag` at the start of a line is a tag, not a heading — the
            // space is what makes it one, which is what Markdown says and
            // what a notes app has to get right.
            if level <= 6 && (rest.starts_with(' ') || rest.trim_start_matches('#').starts_with(' ')) {
                flush(&mut para, &mut out);
                out.push(Block::Heading { level: level as u8, text: text.to_string() });
                i += 1;
                continue;
            }
        }

        if let Some(img) = lone_image(t) {
            flush(&mut para, &mut out);
            out.push(img);
            i += 1;
            continue;
        }

        if let Some(rest) = t.strip_prefix("> ").or_else(|| t.strip_prefix('>')) {
            flush(&mut para, &mut out);
            out.push(Block::Quote(rest.trim().to_string()));
            i += 1;
            continue;
        }

        if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            flush(&mut para, &mut out);
            out.push(Block::Bullet(rest.trim().to_string()));
            i += 1;
            continue;
        }

        if let Some((num, rest)) = t.split_once(". ") {
            if let Ok(n) = num.parse::<u32>() {
                flush(&mut para, &mut out);
                out.push(Block::Numbered { n, text: rest.trim().to_string() });
                i += 1;
                continue;
            }
        }

        para.push(t.to_string());
        i += 1;
    }
    flush(&mut para, &mut out);
    out
}

/// `![alt](link)` and nothing else on the line.
fn lone_image(t: &str) -> Option<Block> {
    let rest = t.strip_prefix("![")?;
    let close = rest.find(']')?;
    let tail = &rest[close + 1..];
    let inner = tail.strip_prefix('(')?;
    let end = inner.find(')')?;
    if !inner[end + 1..].trim().is_empty() {
        return None;
    }
    Some(Block::Image {
        alt: rest[..close].to_string(),
        link: inner[..end].trim().to_string(),
    })
}

/// Put a new set of tags on a note, and hand back the whole note.
///
/// Text in, text out: the caller saves it the way it saves any other edit, so
/// tagging goes through the same conflict check as typing does. A tagger that
/// wrote the file itself would be a second way to write a note, and the
/// second way is the one that loses somebody else's paragraph.
///
/// A note with no front matter gets one. A note whose front matter has no
/// `tags:` gets the line added at the end of it — **not the start**: the
/// order somebody put their own fields in is theirs.
pub fn set_tags(text: &str, tags: &[String]) -> String {
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let end = text.ends_with('\n');
    let f = front(&lines);
    let line = format!(
        "tags: [{}]",
        tags.iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 4);
    if f.lines == 0 {
        // No front matter at all. It goes on the top, with nothing else in it.
        out.push("---".into());
        out.push(line);
        out.push("---".into());
        out.extend(lines);
    } else {
        // `f.lines` counts the fences too, so the body of it is 1..f.lines-1.
        let mut wrote = false;
        out.push(lines[0].clone());
        let mut list = false;
        for raw in &lines[1..f.lines - 1] {
            let t = raw.trim_start();
            // A `tags:` written as a list takes its `- item` lines with it.
            if list && t.starts_with("- ") {
                continue;
            }
            list = false;
            let key = t.split_once(':').map(|(k, _)| k.trim().to_ascii_lowercase());
            if key.as_deref() == Some("tags") {
                if !wrote {
                    out.push(line.clone());
                    wrote = true;
                }
                list = t.split_once(':').map(|(_, v)| v.trim().is_empty()).unwrap_or(false);
                continue;
            }
            out.push(raw.clone());
        }
        if !wrote {
            out.push(line);
        }
        out.push(lines[f.lines - 1].clone());
        out.extend(lines[f.lines..].iter().cloned());
    }
    let mut s = out.join("\n");
    if end {
        s.push('\n');
    }
    s
}

/// Today, where the person is sitting.
///
/// Local and not UTC: a note written at ten at night in Tokyo is dated that
/// day, not the next one. Separate from [`new_note`] so that one can be told
/// what day it is and tested.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// The filename and the first bytes of a new note.
///
/// Pure, and given the date rather than reading a clock, so the shape of a new
/// note is a thing tests can state. The engine does the writing and picks a
/// name that is free; everything about *what a note is* is here, because this
/// is the module that can travel to a phone.
///
/// The body is front matter and nothing else. A `# title` heading under a
/// `title:` field says the same thing twice, and the second copy is the one
/// that goes stale when the note is renamed.
pub fn new_note(title: &str, today: &str) -> (String, String) {
    let title = title.trim();
    // An untitled note is named for the day it was made — which is what you
    // reach for when you are writing before you know what it is about.
    let shown = if title.is_empty() { today } else { title };
    let stem = match file_stem(shown) {
        s if s.is_empty() => today.to_string(),
        s => s,
    };
    let body = format!("---\ntitle: {shown}\ncreated: {today}\ntags: []\n---\n\n");
    (format!("{stem}.md"), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_searchable_by_its_title_and_its_tags_not_its_filename() {
        let n = Note {
            path: "/n/page-0012.md".into(),
            title: "段取り".into(),
            excerpt: "本文です。".into(),
            tags: vec!["仕事".into(), "OneNote".into()],
            updated: Some(0),
            bytes: 0,
        };
        let h = haystack(&n);
        assert!(h.contains("段取り"), "the title: {h}");
        assert!(h.contains("#仕事"), "the tag, with its hash: {h}");
        assert!(h.contains("#onenote"), "lowercased, so the filter need not be: {h}");
        assert!(h.contains("本文です"), "and the start of it: {h}");
    }

    #[test]
    fn a_picture_does_not_fill_the_line_that_says_what_the_note_is_about() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.md");
        std::fs::write(
            &p,
            "# 題\n![](attachments/n-1788450324680.jpg)\n本文はこちら。\n",
        )
        .unwrap();
        let n = read(&p, 60).unwrap();
        assert_eq!(n.excerpt, "本文はこちら。", "got {:?}", n.excerpt);

        // The alt text is words somebody chose, so it stays.
        std::fs::write(&p, "# 題\n![現場の写真](a.jpg) のとおり。\n").unwrap();
        assert_eq!(read(&p, 60).unwrap().excerpt, "現場の写真 のとおり。");

        // An ordinary link is words too, and is left alone.
        std::fs::write(&p, "# 題\n[手順](x.md) を見て。\n").unwrap();
        assert_eq!(read(&p, 60).unwrap().excerpt, "[手順](x.md) を見て。");

        // A note that is only a picture has no excerpt, rather than an
        // excerpt made of a filename.
        std::fs::write(&p, "# 題\n![](a.jpg)\n").unwrap();
        assert_eq!(read(&p, 60).unwrap().excerpt, "");
    }

    #[test]
    fn a_note_comes_apart_into_things_that_can_be_drawn() {
        let md = "---\ntitle: 段取り\n---\n# 見出し\n本文の一行目\nと二行目。\n\n- ひとつ\n2. ふたつ\n> 引用\n![現場](a.jpg)\n\n```rust\nfn main() {}\n\nlet x = 1;\n```\n---\nおわり\n";
        let b = blocks(md);
        // The front matter is how the note describes itself, not something it
        // says — and the title is already on screen above this.
        assert_eq!(b[0], Block::Heading { level: 1, text: "見出し".into() });
        // Two lines with no blank between them are one paragraph, as Markdown
        // says and as anybody typing on a phone expects.
        assert_eq!(b[1], Block::Paragraph("本文の一行目 と二行目。".into()));
        assert_eq!(b[2], Block::Bullet("ひとつ".into()));
        assert_eq!(b[3], Block::Numbered { n: 2, text: "ふたつ".into() });
        assert_eq!(b[4], Block::Quote("引用".into()));
        assert_eq!(b[5], Block::Image { alt: "現場".into(), link: "a.jpg".into() });
        // A fence keeps its blank line: losing it would change the code.
        assert_eq!(b[6], Block::Code { lang: "rust".into(), text: "fn main() {}\n\nlet x = 1;".into() });
        assert_eq!(b[7], Block::Rule, "`---` below the front matter is a rule");
        assert_eq!(b[8], Block::Paragraph("おわり".into()));
        assert_eq!(b.len(), 9);
    }

    #[test]
    fn a_hash_without_a_space_is_a_tag_and_not_a_heading() {
        // `#仕事` on its own line is how people write a tag. Reading it as a
        // heading would make every tagged note open with its tag in 32pt.
        assert_eq!(blocks("#仕事\n"), vec![Block::Paragraph("#仕事".into())]);
        assert_eq!(blocks("# 仕事\n"), vec![Block::Heading { level: 1, text: "仕事".into() }]);
        // An image with words after it is a sentence, not a picture on its own.
        assert_eq!(
            blocks("![a](b.jpg) のとおり\n"),
            vec![Block::Paragraph("![a](b.jpg) のとおり".into())]
        );
        // A fence nobody closed swallows the rest rather than pretending each
        // line is a paragraph.
        assert_eq!(
            blocks("```\nx\ny\n"),
            vec![Block::Code { lang: String::new(), text: "x\ny".into() }]
        );
    }

    #[test]
    fn tags_go_on_without_disturbing_the_rest_of_the_note() {
        // The other fields, and the order they were written in, are the
        // writer's — only the tags line is replaced.
        let src = "---\ntitle: 段取り\ncreated: 2026-09-04\ntags: [古い]\n---\n本文。\n";
        let out = set_tags(src, &["仕事".into(), "cian".into()]);
        assert_eq!(
            out,
            "---\ntitle: 段取り\ncreated: 2026-09-04\ntags: [仕事, cian]\n---\n本文。\n"
        );
        // And it reads back as those tags, which is the only thing that
        // actually matters.
        let lines: Vec<String> = out.lines().map(String::from).collect();
        assert_eq!(front(&lines).tags, vec!["仕事".to_string(), "cian".into()]);

        // No front matter: one is made, and the note is left below it.
        let out = set_tags("# 題\n本文。\n", &["あ".into()]);
        assert_eq!(out, "---\ntags: [あ]\n---\n# 題\n本文。\n");

        // Front matter with no tags: the line is added at the *end* of it.
        let out = set_tags("---\ntitle: x\n---\n本文。\n", &["あ".into()]);
        assert_eq!(out, "---\ntitle: x\ntags: [あ]\n---\n本文。\n");

        // Tags written as a list take their items with them, rather than
        // leaving orphaned `- ` lines behind the new line.
        let out = set_tags("---\ntags:\n  - 古い\n  - もっと古い\ntitle: x\n---\n本文。\n", &["新しい".into()]);
        assert_eq!(out, "---\ntags: [新しい]\ntitle: x\n---\n本文。\n");

        // Taking them all off leaves an empty list, not a broken line.
        let out = set_tags("---\ntags: [あ]\n---\n本文。\n", &[]);
        assert_eq!(out, "---\ntags: []\n---\n本文。\n");
    }

    #[test]
    fn a_picture_lands_beside_the_note_and_the_link_points_at_it() {
        let d = tempfile::tempdir().unwrap();
        let note = d.path().join("段取り.md");
        std::fs::write(&note, "# 段取り\n").unwrap();

        let link = attach(&note, &[1, 2, 3], "PNG").unwrap();
        assert!(link.starts_with("attachments/段取り-"), "{link}");
        assert!(link.ends_with(".png"), "the extension is lowercased: {link}");
        // The link is relative to the note, so following it from the folder
        // finds the file — on a Mac, in Explorer, and on a phone.
        assert_eq!(std::fs::read(d.path().join(&link)).unwrap(), vec![1, 2, 3]);

        // Twice in the same millisecond is the only way to collide, and the
        // stamp makes that the only case; twice in general must not.
        let again = attach(&note, &[4], "png").unwrap();
        assert!(std::fs::read(d.path().join(&link)).is_ok(), "the first is still there");
        assert!(std::fs::read(d.path().join(&again)).is_ok());

        // A name a filesystem would refuse cannot come back through a note's
        // own title — `file_stem` is applied to it here too.
        let odd = d.path().join("a-b.md");
        std::fs::write(&odd, "x").unwrap();
        assert!(attach(&odd, &[1], "").unwrap().ends_with(".png"), "no extension means png");

        // Nothing to attach is refused rather than written as an empty file
        // the link would then point at.
        assert!(attach(&note, &[], "png").is_err());
    }

    #[test]
    fn a_new_note_reads_back_as_the_note_it_says_it_is() {
        let (name, body) = new_note("段取り", "2026-09-02");
        assert_eq!(name, "段取り.md");
        // The point of the shape: what `new_note` writes, `front` understands.
        // These two have drifted apart in every notes app that has two.
        let lines: Vec<String> = body.lines().map(|l| l.to_string()).collect();
        let f = front(&lines);
        assert_eq!(f.fields.get("title").map(String::as_str), Some("段取り"));
        assert_eq!(f.fields.get("created").map(String::as_str), Some("2026-09-02"));
        assert!(f.tags.is_empty(), "an empty list is empty, not one empty tag: {:?}", f.tags);
    }

    #[test]
    fn an_untitled_note_is_named_for_the_day() {
        let (name, body) = new_note("   ", "2026-09-02");
        assert_eq!(name, "2026-09-02.md");
        assert!(body.contains("title: 2026-09-02"));
    }

    #[test]
    fn a_title_a_filesystem_would_refuse_is_made_into_one_it_takes() {
        // Slashes and colons are what people type in a title without thinking
        // — a date, a path, a ratio.
        assert_eq!(file_stem("2026/09/02 の予定"), "2026-09-02 の予定");
        assert_eq!(file_stem("a:b*c?d"), "a-b-c-d");
        // A run collapses to one dash, and never opens the name.
        assert_eq!(file_stem("??  なぞ"), "なぞ");
        // Explorer eats a trailing dot or space, so the name it shows would
        // not be the name on disk.
        assert_eq!(file_stem("あとで書く. "), "あとで書く");
        // Windows device names are still device names with an extension.
        assert_eq!(file_stem("CON"), "_CON");
        assert_eq!(file_stem("con.old"), "_con.old");
        assert_eq!(file_stem("console"), "console", "only the exact name is reserved");
        // Long enough to matter, cut on a character and not in the middle of
        // one — 60 characters of Japanese is 180 bytes, inside every limit.
        let long = "あ".repeat(200);
        assert_eq!(file_stem(&long).chars().count(), 60);
        // Nothing usable left: the caller falls back to the date.
        assert_eq!(file_stem("///"), "");
    }

    fn ls(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn front_matter_is_read_both_ways_round() {
        let f = front(&ls("---\ntitle: 段取り\ntags: [onenote, 2026]\n---\n# 段取り\n"));
        assert_eq!(f.get("title"), Some("段取り"));
        assert_eq!(f.tags, ["onenote", "2026"]);
        assert_eq!(f.lines, 4, "the block, fences included");

        let f = front(&ls("---\ntags:\n  - a\n  - b\nstatus: done\n---\nbody\n"));
        assert_eq!(f.tags, ["a", "b"]);
        assert_eq!(f.get("status"), Some("done"));
    }

    /// **A `---` that is not front matter must not be treated as some.** A
    /// horizontal rule partway down, and a document that opens with one and
    /// never closes it, are both ordinary Markdown — swallowing either would
    /// eat the start of somebody's note.
    #[test]
    fn a_rule_is_not_front_matter() {
        assert_eq!(front(&ls("# title\n\n---\n\nbody\n")).lines, 0);
        assert_eq!(front(&ls("---\nnot closed\nbody\n")).lines, 0);
        assert_eq!(front(&ls("")).lines, 0);
    }

    #[test]
    fn a_title_falls_back_until_it_finds_one() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("kickoff.md");

        std::fs::write(&p, "---\ntitle: 名前\n---\n# 見出し\n").unwrap();
        assert_eq!(read(&p, 40).unwrap().title, "名前");

        std::fs::write(&p, "# 見出し\n本文\n").unwrap();
        assert_eq!(read(&p, 40).unwrap().title, "見出し", "then the heading");

        std::fs::write(&p, "本文だけ\n").unwrap();
        assert_eq!(read(&p, 40).unwrap().title, "kickoff", "then the file name");
    }

    /// The second line of a row says what the note is *about*, so what it is
    /// made of — headings, fences, front matter — is left out.
    #[test]
    fn the_excerpt_skips_the_scaffolding() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.md");
        std::fs::write(&p, "---\ntitle: t\n---\n# 見出し\n\n本文の一行目。\n```\ncode\n```\n二行目。\n").unwrap();
        let n = read(&p, 40).unwrap();
        assert_eq!(n.excerpt, "本文の一行目。 二行目。");
    }

    #[test]
    fn a_typed_date_beats_the_mtime() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.md");
        std::fs::write(&p, "---\nupdated: 2020-01-02\n---\nx\n").unwrap();
        // 2020-01-02T00:00:00Z
        assert_eq!(read(&p, 40).unwrap().updated, Some(1_577_923_200));
        std::fs::write(&p, "x\n").unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let got = read(&p, 40).unwrap().updated.unwrap();
        assert!(got.abs_diff(now) < 60, "otherwise the file's own time");
    }

    #[test]
    fn the_epoch_arithmetic_is_right() {
        assert_eq!(date_secs("1970-01-01"), Some(0));
        assert_eq!(date_secs("2026-09-02"), Some(1_788_307_200));
        assert_eq!(date_secs("2026-09-02T10:11:12"), Some(1_788_307_200), "the day only");
        assert_eq!(date_secs("nonsense"), None);
        assert_eq!(date_secs("2026-13-01"), None, "no thirteenth month");
    }
}
