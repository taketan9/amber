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
    /// `created` from the front matter if it has one, else the file's own
    /// birth time. **A different question from `updated`** — "when did I
    /// start this" and "when did I last touch it" put a note in two
    /// different places in a list, and both are things people look for.
    pub created: Option<u64>,
    pub bytes: u64,
    /// A favourite, and **which favourite folder it is in** — `Some("")` is
    /// the top of the favourites, `Some("買い物/週次")` is a shelf inside it.
    ///
    /// A favourite is a *second* place a note is, not a move: it stays in the
    /// folder it was written in, and `star` says where it also appears. That
    /// is the whole difference between this and filing, and it is why it
    /// lives on the note rather than in a list somewhere — a note that is
    /// moved, renamed or synced takes its favourite place with it.
    ///
    /// Written as `star: true` or `star: 買い物`. `pinned: true` is still
    /// read, because notes written before this existed say that.
    pub star: Option<String>,
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
    // `created` on a filesystem that does not keep one falls back to the
    // mtime rather than to nothing: a note with no date at all drops out of
    // a list grouped by date, which looks like a lost note.
    let created = f.get("created").and_then(date_secs).or_else(|| {
        meta.as_ref()
            .and_then(|m| m.created().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .or(updated)
    });
    Some(Note {
        path: path.to_path_buf(),
        title,
        excerpt: excerpt(body),
        star: star(&f),
        tags: f.tags,
        updated,
        created,
        bytes: meta.map(|m| m.len()).unwrap_or(0),
    })
}

/// Where a note sits in the favourites, if it is one.
///
/// `true`/`yes`/`1` are the three ways people write yes, because nobody
/// remembers which one a given app wanted; anything else is the name of a
/// shelf. `false` is a note that says, in writing, that it is not one.
fn star(f: &Front) -> Option<String> {
    let raw = f
        .fields
        .get("star")
        .or_else(|| f.fields.get("favorite"))
        .or_else(|| f.fields.get("pinned"))?;
    let v = raw.trim();
    match v.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" => Some(String::new()),
        "false" | "no" | "0" | "" => None,
        _ => Some(v.trim_matches(['"', '\'']).to_string()),
    }
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
        // A table is not a sentence. `| 名前 | 状態 |` in the one line meant
        // to remind you what the note is about tells you the note has a
        // table, which you can see, and nothing about what is in it.
        if t.starts_with('|') {
            continue;
        }
        // A picture is not a sentence. `![](attachments/note-1788450324680.jpg)`
        // is forty characters of filename in a line meant to remind you what
        // the note is about, and a note that opens with a screenshot showed
        // nothing else at all.
        let t = strip_images(t);
        // A colour is not a sentence either. `<span style="color:#D9822B">`
        // is thirty characters of notation in the one line meant to remind
        // you what the note is about — and it is the line the search runs
        // against, so a coloured word would stop being findable.
        let t = plain(&t);
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
pub fn create(
    dir: &std::path::Path,
    title: &str,
    today: &str,
    now: &str,
) -> anyhow::Result<std::path::PathBuf> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let (name, body) = new_note(title, today, now);
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
        anyhow::bail!("そのノートの保存場所が分かりません")
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
    /// `- [ ] milk`. Carries the line it came from, because the only useful
    /// thing to do with a checkbox is press it — and pressing it has to say
    /// *which* one without the caller re-deriving an index that the next
    /// edit will move.
    Check { done: bool, text: String, line: usize },
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
            // A task before a bullet: `- [ ] x` is both, and the one that
            // can be pressed is the more useful answer.
            if let Some(done) = ticked(rest) {
                out.push(Block::Check {
                    done,
                    text: rest[3..].trim().to_string(),
                    line: i,
                });
            } else {
                out.push(Block::Bullet(rest.trim().to_string()));
            }
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

/// A piece of a line, and the colour it was written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    /// `#rrggbb`, lowercased, or `None` for the colour the reader is using.
    pub color: Option<String>,
}

/// Split a line into coloured and uncoloured pieces.
///
/// **Markdown has no colour**, so this reads the one notation the most tools
/// already understand: `<span style="color:#rrggbb">…</span>`. VS Code's
/// preview, Obsidian, Typora and pandoc all render it; GitHub strips the
/// `style` attribute and shows the words plainly. That last part is the
/// reason for choosing it over the `$\color{red}{...}$` trick, which renders
/// on GitHub and leaves an unreadable pile of symbols everywhere else — a
/// note has to degrade into *the sentence*, not into notation.
///
/// **Here rather than in either front end.** Two parsers for one notation is
/// two answers, and the window and the phone would start disagreeing about
/// what a note says the first time either was touched.
///
/// Anything that is not a well-formed colour span is left exactly as typed,
/// including a `<span>` with some other style on it: cian is not an HTML
/// renderer and should not pretend to be one.
pub fn spans(line: &str) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut rest = line;

    while let Some(at) = rest.find("<span") {
        let (before, from) = rest.split_at(at);
        let Some(gt) = from.find('>') else { break };
        let open = &from[..=gt];
        let Some(color) = color_of(open) else {
            // Not ours. Keep the tag as text and carry on past it, or a
            // `<span class=…>` would swallow the rest of the line.
            push(&mut out, &rest[..at + gt + 1], None);
            rest = &from[gt + 1..];
            continue;
        };
        let after = &from[gt + 1..];
        let Some(end) = after.find("</span>") else { break };
        push(&mut out, before, None);
        push(&mut out, &after[..end], Some(color));
        rest = &after[end + "</span>".len()..];
    }
    push(&mut out, rest, None);
    out
}

/// A colour span at the very start of `s`: its text, its colour, and how
/// many **characters** it took.
///
/// For a scanner that is walking a line character by character and needs to
/// know whether *this* is the start of one. The recognising is the same as
/// [`spans`] — one notation, one place that knows it.
///
/// **Characters and not bytes.** `find` counts bytes; the caller counts
/// characters. With Japanese inside the span the two are three times apart,
/// so the scanner jumped past the span *and* the text after it — which
/// showed up as a second coloured word on a line coming out as
/// `e="color:#0E93A8">シアン</span>` in the middle of a sentence.
pub fn first_color(s: &str) -> Option<(String, String, usize)> {
    if !s.starts_with("<span") {
        return None;
    }
    let gt = s.find('>')?;
    let color = color_of(&s[..=gt])?;
    let after = &s[gt + 1..];
    let end = after.find("</span>")?;
    let bytes = gt + 1 + end + "</span>".len();
    Some((after[..end].to_string(), color, s[..bytes].chars().count()))
}

fn push(out: &mut Vec<Span>, text: &str, color: Option<String>) {
    if text.is_empty() {
        return;
    }
    // Two runs of the same colour side by side are one run — the renderer
    // should not have to care how the line was cut up.
    if let Some(last) = out.last_mut() {
        if last.color == color {
            last.text.push_str(text);
            return;
        }
    }
    out.push(Span { text: text.to_string(), color });
}

/// `#rrggbb` out of `<span style="color:#0e93a8">`, if that is what this is.
///
/// Only hex, and only six digits: a name like `red` means a different colour
/// in every renderer, and cian writing one would be writing something it
/// cannot promise the Mac will draw the same.
fn color_of(open: &str) -> Option<String> {
    let lower = open.to_ascii_lowercase();
    let at = lower.find("color:")?;
    let rest = lower[at + "color:".len()..].trim_start();
    let hex = rest.strip_prefix('#')?;
    let digits: String = hex.chars().take(6).collect();
    if digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(format!("#{digits}"))
    } else {
        None
    }
}

/// What a search box means: an OR of ANDs.
///
/// `仕事 週報` finds notes with both. `仕事 OR 家` finds either. Written
/// together — `仕事 週報 OR 家` — the OR is the weaker join, so that reads as
/// (仕事 AND 週報) OR (家), which is how everybody writes it and nobody
/// explains it.
///
/// **Here rather than in the front ends.** What a query means is a decision;
/// three front ends deciding it separately is three search boxes that agree
/// until somebody types two words. The matching itself stays where the list
/// is — this says what to match.
///
/// `OR`, `or`, `|` and `｜` all mean the same thing: nobody remembers which
/// one an app wanted, and the full-width bar is what a Japanese keyboard
/// gives you without switching.
pub fn terms(query: &str) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut group: Vec<String> = Vec::new();
    for word in query.split_whitespace() {
        if matches!(word, "OR" | "or" | "|" | "｜") {
            // A bare `OR` at the start, or two in a row, is somebody still
            // typing — not an empty group that matches everything.
            if !group.is_empty() {
                out.push(std::mem::take(&mut group));
            }
            continue;
        }
        group.push(word.to_lowercase());
    }
    if !group.is_empty() {
        out.push(group);
    }
    out
}

/// Whether `hay` answers the query. Empty query matches everything.
pub fn hits(hay: &str, query: &str) -> bool {
    let groups = terms(query);
    if groups.is_empty() {
        return true;
    }
    let hay = hay.to_lowercase();
    groups.iter().any(|g| g.iter().all(|w| hay.contains(w)))
}

/// The words, with the colour notation taken off.
///
/// For the places that want a sentence rather than a drawing: the second line
/// of a row, and what a search matches against.
pub fn plain(line: &str) -> String {
    spans(line).into_iter().map(|s| s.text).collect()
}

/// Wrap a piece of text in a colour, the way cian writes it.
pub fn paint(text: &str, color: &str) -> String {
    format!("<span style=\"color:{color}\">{text}</span>")
}

/// Whether `[ ] x` / `[x] x` starts this bullet's text, and which.
///
/// `[X]` counts too: a note typed on somebody else's machine is still a note.
fn ticked(rest: &str) -> Option<bool> {
    let b = rest.as_bytes();
    if b.len() < 3 || b[0] != b'[' || b[2] != b']' {
        return None;
    }
    match b[1] {
        b' ' => Some(false),
        b'x' | b'X' => Some(true),
        _ => None,
    }
}

/// Tick or untick the checkbox on one line, and hand the whole note back.
///
/// **By line number, not by which checkbox it is.** The list on screen was
/// drawn from a `blocks()` that may be a moment old; counting boxes would
/// tick the wrong one the first time a note gains a task above the one you
/// pressed. A line that is not a checkbox is left exactly as it was — the
/// screen and the file can disagree, and when they do nothing should happen.
pub fn set_check(text: &str, line: usize, done: bool) -> String {
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let Some(row) = lines.get_mut(line) else { return text.to_string() };
    let indent: String = row.chars().take_while(|c| c.is_whitespace()).collect();
    let t = row.trim_start();
    let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) else {
        return text.to_string();
    };
    if ticked(rest).is_none() {
        return text.to_string();
    }
    let lead = &t[..t.len() - rest.len()];
    *row = format!("{indent}{lead}[{}]{}", if done { "x" } else { " " }, &rest[3..]);
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
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

/// Set or remove one plain field in a note's front matter, and hand back the
/// whole note.
///
/// Text in, text out, for the same reason as [`set_tags`]: the caller saves
/// it the ordinary way, so pinning a note is checked against the file on disk
/// exactly as typing in it is.
///
/// `None` takes the field off rather than writing an empty one — a note that
/// says `pinned:` with nothing after it is a note that will be read as pinned
/// by the next thing that looks.
pub fn set_field(text: &str, key: &str, value: Option<&str>) -> String {
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let end = text.ends_with('\n');
    let f = front(&lines);
    let key_l = key.to_ascii_lowercase();
    let line = value.map(|v| format!("{key}: {v}"));

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 4);
    if f.lines == 0 {
        let Some(line) = line else { return text.to_string() };
        out.push("---".into());
        out.push(line);
        out.push("---".into());
        out.extend(lines);
    } else {
        let mut wrote = false;
        out.push(lines[0].clone());
        for raw in &lines[1..f.lines - 1] {
            let t = raw.trim_start();
            let k = t.split_once(':').map(|(k, _)| k.trim().to_ascii_lowercase());
            if k.as_deref() == Some(key_l.as_str()) {
                if let Some(line) = &line {
                    if !wrote {
                        out.push(line.clone());
                        wrote = true;
                    }
                }
                continue;
            }
            out.push(raw.clone());
        }
        if let Some(line) = line {
            if !wrote {
                out.push(line);
            }
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

/// Move a note into another folder, taking its pictures with it.
///
/// **The pictures have to come.** A note's links are relative — `![](
/// attachments/note-1788450324680.jpg)` — so a note that moves on its own
/// arrives with every picture broken, and the breakage shows up later, when
/// somebody opens the note and cannot tell whether the image was deleted or
/// never arrived.
///
/// Which pictures are "its" is answerable because of how they were named:
/// [`attach`] calls them `<the note's stem>-<clock>.<ext>`. A picture some
/// other note also points at would be moved out from under it — but that can
/// only happen if somebody wrote the link by hand, and the alternative
/// (leaving every picture behind) breaks the note that is actually moving.
///
/// Nothing is overwritten: a name already taken at the destination stops the
/// move with the note still where it was.
pub fn move_to(note: &std::path::Path, dir: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
    let Some(name) = note.file_name() else {
        anyhow::bail!("移せません: {}", note.display())
    };
    let Some(from) = note.parent() else {
        anyhow::bail!("移せません: {}", note.display())
    };
    if from == dir {
        return Ok(note.to_path_buf());
    }
    std::fs::create_dir_all(dir)?;
    let to = dir.join(name);
    if to.exists() {
        anyhow::bail!("{} には同じ名前があります", dir.display());
    }

    let stem = note
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut pictures: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(from.join("attachments")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with(&format!("{stem}-")) {
                pictures.push(e.path());
            }
        }
    }

    // The note last: if a picture cannot be moved, the note is still where it
    // was and still points at pictures that are still there.
    if !pictures.is_empty() {
        let at = dir.join("attachments");
        std::fs::create_dir_all(&at)?;
        for p in &pictures {
            if let Some(n) = p.file_name() {
                std::fs::rename(p, at.join(n))?;
            }
        }
    }
    std::fs::rename(note, &to)?;
    Ok(to)
}

/// Today, where the person is sitting.
///
/// Local and not UTC: a note written at ten at night in Tokyo is dated that
/// day, not the next one. Separate from [`new_note`] so that one can be told
/// what day it is and tested.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// Now, as the clock on this machine reads it.
///
/// Here so that a front end which has to keep its own timers does not have to
/// depend on a date library to ask what time it is — the two things it needs
/// (this and [`next_ring`]) then come from the same place, and cannot end up
/// disagreeing about which day it is.
pub fn now_local() -> chrono::NaiveDateTime {
    chrono::Local::now().naive_local()
}

/// The moment, to the second — what an untitled note is called.
///
/// The day alone is not enough: three notes made in one afternoon were
/// 「2026-09-05」, 「2026-09-05-2」 and 「2026-09-05-3」, and the number says
/// nothing about which is which. The time does.
pub fn now_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
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
pub fn new_note(title: &str, today: &str, now: &str) -> (String, String) {
    let title = title.trim();
    // An untitled note is named for the *moment* it was made — the day alone
    // gives three notes one afternoon the same name and a number after it,
    // and the number says nothing about which is which. `created` stays a
    // date: it is a field things parse, and a title is a thing people read.
    let shown = if title.is_empty() { now } else { title };
    let stem = match file_stem(shown) {
        s if s.is_empty() => today.to_string(),
        s => s,
    };
    let body = format!("---\ntitle: {shown}\ncreated: {today}\ntags: []\n---\n\n");
    (format!("{stem}.md"), body)
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_search_box_is_an_or_of_ands() {
        assert_eq!(terms("仕事 週報"), vec![vec!["仕事".to_string(), "週報".to_string()]]);
        assert_eq!(
            terms("仕事 OR 家"),
            vec![vec!["仕事".to_string()], vec!["家".to_string()]]
        );
        // OR のほうが弱い綴じ ── (仕事 AND 週報) OR (家)。
        assert_eq!(
            terms("仕事 週報 or 家"),
            vec![vec!["仕事".to_string(), "週報".to_string()], vec!["家".to_string()]]
        );
        // 打ちかけの OR は、何にでも当たる空の組にしない。
        assert_eq!(terms("OR"), Vec::<Vec<String>>::new());
        assert_eq!(terms("仕事 OR OR 家").len(), 2);
        // 全角の縦棒も同じ ── 日本語キーボードで切り替えずに出るのはこちら。
        assert_eq!(terms("あ ｜ い").len(), 2);

        assert!(hits("週報 仕事 定型", "仕事 週報"));
        assert!(!hits("週報 仕事", "仕事 買い物"));
        assert!(hits("買うもの 家", "仕事 OR 家"));
        assert!(hits("なんでも", ""), "空の問いは全部に当たる");
        // 大文字小文字は問わない。
        assert!(hits("Weekly Report", "weekly"));
    }

    #[test]
    fn catching_up_writes_down_that_it_caught_up() {
        use chrono::NaiveDate;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("週報.md");
        std::fs::write(
            &p,
            "---\ntitle: 週報\nrepeat: weekly wed 09:00\nlast: 2026-08-30\n---\n本文。\n",
        )
        .unwrap();

        let made = catch_up(&p, NaiveDate::from_ymd_opt(2026, 9, 5).unwrap()).unwrap();
        assert_eq!(made.len(), 1, "2026-09-02 は水曜: {made:?}");
        assert!(std::fs::read_to_string(&p).unwrap().contains("last: 2026-09-02"));

        // **二度目は何も作らない。** これを忘れると、フォルダを開くたびに
        // 「作りました」と言い続ける。
        assert!(catch_up(&p, NaiveDate::from_ymd_opt(2026, 9, 5).unwrap()).unwrap().is_empty());

        // 定型でないノートは、何も作らないしエラーでもない。
        let plain = d.path().join("ただのノート.md");
        std::fs::write(&plain, "---\ntitle: x\n---\n本文。\n").unwrap();
        assert!(catch_up(&plain, NaiveDate::from_ymd_opt(2026, 9, 5).unwrap()).unwrap().is_empty());
    }

    #[test]
    fn the_next_time_a_routine_comes_round() {
        use chrono::NaiveDate;
        let at = |y, m, d, h, mi| NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, mi, 0).unwrap();

        // 今日の時刻がまだなら今日、過ぎていれば明日。
        assert_eq!(next_ring(Every::Daily, 9, 0, at(2026, 9, 5, 8, 0)), Some(at(2026, 9, 5, 9, 0)));
        assert_eq!(next_ring(Every::Daily, 9, 0, at(2026, 9, 5, 9, 0)), Some(at(2026, 9, 6, 9, 0)));

        // 2026-09-05 は土曜。水曜は 09-09。
        assert_eq!(
            next_ring(Every::Weekly(2), 9, 0, at(2026, 9, 5, 12, 0)),
            Some(at(2026, 9, 9, 9, 0))
        );
        // 当日の朝、時刻の前なら今日。
        assert_eq!(
            next_ring(Every::Weekly(2), 9, 0, at(2026, 9, 9, 8, 0)),
            Some(at(2026, 9, 9, 9, 0))
        );

        // 31日は、30日しかない月では末日へ寄る ── そうしないと
        // 「毎月31日」が「7か月だけ」になる。
        assert_eq!(
            next_ring(Every::Monthly(31), 9, 0, at(2026, 11, 1, 0, 0)),
            Some(at(2026, 11, 30, 9, 0))
        );
        // 鳴った直後に訊いても、同じものを返さない。
        let fired = at(2026, 9, 5, 9, 0);
        assert!(next_ring(Every::Daily, 9, 0, fired).unwrap() > fired);
    }

    #[test]
    fn a_colour_is_a_span_and_everything_else_is_left_as_typed() {
        let one = spans("ふつうの<span style=\"color:#0E93A8\">シアン</span>あと");
        assert_eq!(one.len(), 3);
        assert_eq!(one[0], Span { text: "ふつうの".into(), color: None });
        assert_eq!(one[1], Span { text: "シアン".into(), color: Some("#0e93a8".into()) });
        assert_eq!(one[2], Span { text: "あと".into(), color: None });

        // 色の無い span は、cian のものではない。字として残す。
        let asis = spans("<span class=\"x\">そのまま</span>");
        assert_eq!(asis.iter().filter(|s| s.color.is_some()).count(), 0);
        assert_eq!(asis.iter().map(|s| s.text.as_str()).collect::<String>(),
                   "<span class=\"x\">そのまま</span>");

        // 名前の色は読まない ── 描く側ごとに違う色になるので。
        assert!(spans("<span style=\"color:red\">あか</span>")
            .iter()
            .all(|s| s.color.is_none()));

        // 閉じていないものは、書いた通りに。
        assert_eq!(spans("<span style=\"color:#123456\">とじ忘れ").len(), 1);

        // 色の無い行は1つの塊。
        assert_eq!(spans("ただの行"), vec![Span { text: "ただの行".into(), color: None }]);
        assert!(spans("").is_empty());

        // 表の行も二行目には出さない ── 「表がある」は見れば分かる。
        {
            let d = tempfile::tempdir().unwrap();
            let p = d.path().join("t.md");
            std::fs::write(&p, "---\ntitle: x\n---\n\n本文です。\n\n| 名前 | 状態 |\n| --- | --- |\n| 通知 | 済 |\n").unwrap();
            assert_eq!(read(&p, 40).unwrap().excerpt, "本文です。");
        }

        // 一覧の二行目と検索は、字だけを見る ── 色を付けた語が
        // 探せなくなるのが一番困る。
        assert_eq!(plain("あ<span style=\"color:#0e93a8\">い</span>う"), "あいう");

        // 書いたものが読める。
        let painted = paint("ここ", "#d9822b");
        let back = spans(&painted);
        assert_eq!(back, vec![Span { text: "ここ".into(), color: Some("#d9822b".into()) }]);
    }

    #[test]
    fn a_task_is_a_block_you_can_press_and_a_bullet_is_not() {
        let text = "---\ntitle: x\n---\n\n- [ ] 牛乳\n- [x] 珈琲\n- ふつうの箇条書き\n";
        let bs = blocks(text);
        let mut checks = Vec::new();
        for b in &bs {
            if let Block::Check { done, text, line } = b {
                checks.push((*done, text.clone(), *line));
            }
        }
        assert_eq!(checks.len(), 2, "{bs:?}");
        assert_eq!(checks[0], (false, "牛乳".to_string(), 4));
        assert_eq!(checks[1], (true, "珈琲".to_string(), 5));
        assert!(matches!(&bs[2], Block::Bullet(t) if t == "ふつうの箇条書き"));
    }

    #[test]
    fn pressing_a_task_changes_that_line_and_nothing_else() {
        let text = "- [ ] 牛乳\n- [ ] 珈琲\n";
        let on = set_check(text, 1, true);
        assert_eq!(on, "- [ ] 牛乳\n- [x] 珈琲\n");
        assert_eq!(set_check(&on, 1, false), text);
        // 行がずれていた・そこは箇条書きだった、のときは何もしない ──
        // 画面とファイルが食い違っているのに書き込むのが一番悪い。
        assert_eq!(set_check(text, 9, true), text);
        assert_eq!(set_check("- ふつう\n", 0, true), "- ふつう\n");
        // 字下げは字下げのまま
        assert_eq!(set_check("  - [ ] 中\n", 0, true), "  - [x] 中\n");
    }
    use super::*;

    #[test]
    fn a_note_is_searchable_by_its_title_and_its_tags_not_its_filename() {
        let n = Note {
            path: "/n/page-0012.md".into(),
            title: "段取り".into(),
            excerpt: "本文です。".into(),
            tags: vec!["仕事".into(), "OneNote".into()],
            updated: Some(0),
            created: Some(0),
            bytes: 0,
            star: None,
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
    fn a_note_that_moves_takes_its_pictures_with_it() {
        let d = tempfile::tempdir().unwrap();
        let note = d.path().join("段取り.md");
        std::fs::write(&note, "# 段取り\n").unwrap();
        let link = attach(&note, &[1, 2, 3], "png").unwrap();
        // Another note's picture, which must stay where it is.
        let other = d.path().join("他.md");
        std::fs::write(&other, "x").unwrap();
        let others = attach(&other, &[9], "png").unwrap();

        let book = d.path().join("仕事");
        let moved = move_to(&note, &book).unwrap();
        assert_eq!(moved, book.join("段取り.md"));
        // The link inside the note is relative, so it has to still find the
        // picture from where the note now is.
        assert_eq!(std::fs::read(book.join(&link)).unwrap(), vec![1, 2, 3]);
        assert!(!d.path().join(&link).exists(), "and not left behind as well");
        assert!(std::fs::read(d.path().join(&others)).is_ok(), "the other note's picture stays");

        // A name already taken stops the move rather than overwriting.
        let clash = d.path().join("段取り.md");
        std::fs::write(&clash, "別物").unwrap();
        assert!(move_to(&clash, &book).is_err());
        assert_eq!(std::fs::read_to_string(&clash).unwrap(), "別物", "still where it was");

        // Moving into the folder it is already in is not a failure.
        assert_eq!(move_to(&moved, &book).unwrap(), moved);
    }

    #[test]
    fn a_routine_makes_a_task_and_not_another_template() {
        use chrono::NaiveDate;
        let d = tempfile::tempdir().unwrap();
        let t = d.path().join("ごみ出し.md");
        std::fs::write(
            &t,
            "---\ntitle: ごみ出し\nrepeat: weekly wed 09:00\nlast: 2026-08-30\ntags: [家]\n---\n- [ ] 燃えるゴミ\n",
        )
        .unwrap();

        let made = carry_out(&t, NaiveDate::from_ymd_opt(2026, 9, 2).unwrap()).unwrap();
        assert_eq!(made.file_name().unwrap(), "ごみ出し 2026-09-02.md");
        let copy = std::fs::read_to_string(&made).unwrap();
        // A task, not another template: it must not spawn copies of its own.
        assert!(!copy.contains("repeat"), "{copy}");
        assert!(!copy.contains("last"), "{copy}");
        assert!(copy.contains("title: ごみ出し 2026-09-02"), "{copy}");
        assert!(copy.contains("created: 2026-09-02"), "{copy}");
        // What the task is stays: the tags and the checklist.
        assert!(copy.contains("tags: [家]"), "{copy}");
        assert!(copy.contains("- [ ] 燃えるゴミ"), "{copy}");
        // And the template is untouched.
        assert!(std::fs::read_to_string(&t).unwrap().contains("repeat: weekly wed 09:00"));

        // Twice for the same day is once — otherwise two devices catching up
        // put two Wednesdays in the list.
        let again = carry_out(&t, NaiveDate::from_ymd_opt(2026, 9, 2).unwrap()).unwrap();
        assert_eq!(again, made);
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 2);
    }

    #[test]
    fn the_three_shapes_a_reminder_is_written_in() {
        use chrono::NaiveDate;
        let r = remind("---\nremind: 2026-09-10 09:00\n---\n本文\n");
        assert_eq!(r.once, NaiveDate::from_ymd_opt(2026, 9, 10).unwrap().and_hms_opt(9, 0, 0));

        assert_eq!(remind("---\nrepeat: daily 07:30\n---\n").every, Some((Every::Daily, 7, 30)));
        // Monday is 0, as chrono counts from Monday — wed is 2.
        assert_eq!(remind("---\nrepeat: weekly wed 09:00\n---\n").every, Some((Every::Weekly(2), 9, 0)));
        // The character people actually type in Japanese.
        assert_eq!(remind("---\nrepeat: weekly 水 09:00\n---\n").every, Some((Every::Weekly(2), 9, 0)));
        assert_eq!(remind("---\nrepeat: monthly 1 09:00\n---\n").every, Some((Every::Monthly(1), 9, 0)));

        // Nonsense is nothing, not a guess: a reminder invented from a typo
        // goes off at a time nobody chose.
        assert_eq!(remind("---\nrepeat: weekly ときどき 09:00\n---\n").every, None);
        assert_eq!(remind("---\nremind: あした\n---\n").once, None);
        assert_eq!(remind("---\nrepeat: daily 25:00\n---\n").every, None);
        assert_eq!(remind("本文だけ\n"), Remind::default());
    }

    #[test]
    fn a_routine_says_what_it_missed_while_the_phone_was_off() {
        use chrono::NaiveDate;
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();

        // 2026-09-02 is a Wednesday. Away for two weeks, two Wednesdays owed.
        let due = due_since(Every::Weekly(2), Some(d(2026, 8, 30)), d(2026, 9, 10));
        assert_eq!(due, vec![d(2026, 9, 2), d(2026, 9, 9)]);

        // Nothing owed between one Wednesday and the next day.
        assert!(due_since(Every::Weekly(2), Some(d(2026, 9, 2)), d(2026, 9, 3)).is_empty());

        // Never carried out: today counts, if today is the day.
        assert_eq!(due_since(Every::Weekly(2), None, d(2026, 9, 2)), vec![d(2026, 9, 2)]);
        assert!(due_since(Every::Weekly(2), None, d(2026, 9, 3)).is_empty());

        // A 31st in a short month lands on the last day rather than being
        // skipped — a monthly routine that misses February is worse than one
        // that is a day early.
        assert_eq!(
            due_since(Every::Monthly(31), Some(d(2026, 1, 31)), d(2026, 2, 28)),
            vec![d(2026, 2, 28)]
        );

        // A `last` from long ago produces a handful, not seven hundred.
        assert!(due_since(Every::Daily, Some(d(2024, 1, 1)), d(2026, 9, 10)).len() <= 32);
    }

    #[test]
    fn a_note_can_be_pinned_and_unpinned_without_losing_anything() {
        let src = "---\ntitle: 段取り\ntags: [仕事]\n---\n本文。\n";
        let on = set_field(src, "pinned", Some("true"));
        assert_eq!(on, "---\ntitle: 段取り\ntags: [仕事]\npinned: true\n---\n本文。\n");

        // Off takes the line away rather than writing `pinned: false` — and
        // certainly rather than `pinned:` with nothing after it, which the
        // next thing to read the note would take for pinned.
        let off = set_field(&on, "pinned", None);
        assert_eq!(off, src);

        // Setting it twice does not write it twice.
        let twice = set_field(&set_field(src, "pinned", Some("true")), "pinned", Some("true"));
        assert_eq!(twice.matches("pinned").count(), 1);

        // Removing a field a note does not have leaves the note alone.
        assert_eq!(set_field(src, "pinned", None), src);
        // A note with no front matter and nothing to write stays as it is.
        assert_eq!(set_field("本文。\n", "pinned", None), "本文。\n");
    }

    #[test]
    fn the_three_ways_people_write_yes() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("n.md");
        // `pinned` is what notes written before favourites existed say, and
        // they must not quietly stop being favourites.
        for key in ["star", "favorite", "pinned"] {
            for (v, want) in [
                ("true", Some("")),
                ("yes", Some("")),
                ("1", Some("")),
                ("false", None),
                ("", None),
                ("買い物/週次", Some("買い物/週次")),
            ] {
                std::fs::write(&p, format!("---\ntitle: x\n{key}: {v}\n---\n本文。\n")).unwrap();
                assert_eq!(
                    read(&p, 20).unwrap().star.as_deref(),
                    want,
                    "{key}: {v:?}"
                );
            }
        }
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
        let (name, body) = new_note("段取り", "2026-09-02", "2026-09-02 14:03:09");
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
    fn an_untitled_note_is_named_for_the_moment_it_was_made() {
        let (name, body) = new_note("   ", "2026-09-02", "2026-09-02 14:03:09");
        // 題の無いノートは、その瞬間の名前。日付だけだと、午後に3本作ると
        // 「-2」「-3」が付くだけで、どれがどれか分からない。
        assert!(body.contains("title: 2026-09-02 14:03:09"), "{body}");
        assert!(body.contains("created: 2026-09-02\n"), "{body}");
        // ファイル名からは `:` が落ちる（Windows で作れない）。
        assert_eq!(name, "2026-09-02 14-03-09.md");
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

/// Carry out a routine: make today's copy of a template note.
///
/// The copy is the note without its `repeat` and `last` — it is a task, not
/// another template — with `created` set to the day it stands for and the day
/// appended to its title, so a month of them reads as a list of days rather
/// than the same word twelve times.
///
/// **Not scheduled by cian.** A phone does not let an app wake at nine on a
/// Wednesday to write a file. The notification arrives on time; the copy is
/// made the next time the app is opened, from `last`, which is why `last`
/// exists. Say so plainly rather than implying a clock nobody has.
/// Make every copy a routine owes, and write down that it was made.
///
/// **The writing down is the whole point.** The copy is easy; remembering
/// that it happened is what stops tomorrow making it again — and the first
/// version of this in the window did the first half only, so opening the
/// folder said 「1 件作りました」 every single time. One function, called by
/// both front ends, so there is one answer to "has this been done".
///
/// Returns what it made. A note with no routine makes nothing and is not an
/// error — most notes have no routine.
pub fn catch_up(path: &Path, today: chrono::NaiveDate) -> anyhow::Result<Vec<PathBuf>> {
    let text = std::fs::read_to_string(path)?;
    let r = remind(&text);
    let Some((every, _, _)) = r.every else { return Ok(Vec::new()) };
    let owed = due_since(every, r.last, today);
    let mut made = Vec::new();
    let mut last = None;
    for day in owed {
        made.push(carry_out(path, day)?);
        last = Some(day);
    }
    if let Some(day) = last {
        // Read again: `carry_out` did not touch this file, but somebody else
        // may have, and the field goes onto what is there now.
        let now = std::fs::read_to_string(path)?;
        std::fs::write(path, set_field(&now, "last", Some(&day.to_string())))?;
    }
    Ok(made)
}

/// When a routine next comes round, from a given moment.
///
/// **For a front end that has to keep its own clock.** The phone hands the
/// times to iOS and forgets them; a window has no such thing, so it sets a
/// timer — and the moment to set it for is a judgement about what "every
/// Wednesday at nine" means, which belongs here rather than in JavaScript.
///
/// `from` is exclusive: called again the instant after one fires, it gives
/// the one after, not the same one for ever.
pub fn next_ring(
    every: Every,
    hour: u32,
    minute: u32,
    from: chrono::NaiveDateTime,
) -> Option<chrono::NaiveDateTime> {
    let day = from.date();
    match every {
        Every::Daily => {
            let today = day.and_hms_opt(hour, minute, 0)?;
            if today > from {
                Some(today)
            } else {
                day.succ_opt()?.and_hms_opt(hour, minute, 0)
            }
        }
        Every::Weekly(w) => {
            // The note counts Monday as 0; chrono counts it as 0 too
            // (`num_days_from_monday`), which is the one place these two
            // agree and worth saying out loud.
            use chrono::Datelike;
            let mut d = day;
            for _ in 0..8 {
                if d.weekday().num_days_from_monday() == w {
                    if let Some(at) = d.and_hms_opt(hour, minute, 0) {
                        if at > from {
                            return Some(at);
                        }
                    }
                }
                d = d.succ_opt()?;
            }
            None
        }
        Every::Monthly(want) => {
            let mut d = day;
            // Two months of days is enough to find the next one, and it
            // handles a 31st in a month that has thirty — `last_day` is what
            // decides where that lands, so this asks the same question the
            // catching-up does rather than a second opinion.
            for _ in 0..64 {
                if due_on(want, d) {
                    if let Some(at) = d.and_hms_opt(hour, minute, 0) {
                        if at > from {
                            return Some(at);
                        }
                    }
                }
                d = d.succ_opt()?;
            }
            None
        }
    }
}

pub fn carry_out(
    template: &std::path::Path,
    on: chrono::NaiveDate,
) -> anyhow::Result<std::path::PathBuf> {
    let text = std::fs::read_to_string(template)?;
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let f = front(&lines);
    let title = f
        .fields
        .get("title")
        .cloned()
        .or_else(|| heading(&lines[f.lines..]))
        .unwrap_or_else(|| {
            template.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        });
    let day = on.format("%Y-%m-%d").to_string();

    let mut copy = set_field(&text, "repeat", None);
    copy = set_field(&copy, "last", None);
    copy = set_field(&copy, "remind", None);
    copy = set_field(&copy, "created", Some(&day));
    copy = set_field(&copy, "title", Some(&format!("{title} {day}")));

    let Some(dir) = template.parent() else {
        anyhow::bail!("保存場所が分かりません: {}", template.display())
    };
    let name = format!("{}.md", file_stem(&format!("{title} {day}")));
    let at = dir.join(&name);
    // Already carried out — by another device, or by this one before `last`
    // was written. Doing it again would put two of the same day in the list.
    if at.exists() {
        return Ok(at);
    }
    std::fs::write(&at, copy)?;
    Ok(at)
}

// ---- Reminders and routines ----------------------------------------------

/// How often a routine comes round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Every {
    Daily,
    /// 0 = Monday, as `chrono::Weekday::num_days_from_monday` counts.
    Weekly(u32),
    /// Day of the month. A 31 in a short month lands on the last day rather
    /// than being skipped — a monthly routine that silently misses February
    /// is worse than one that is a day early.
    Monthly(u32),
}

/// When a note wants to be brought up.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Remind {
    /// `remind: 2026-09-10 09:00` — once.
    pub once: Option<chrono::NaiveDateTime>,
    /// `repeat: weekly wed 09:00` — again and again.
    pub every: Option<(Every, u32, u32)>,
    /// `last: 2026-09-03` — the last day this routine was carried out. Kept
    /// in the note because that is where everything else about the note is,
    /// and because a phone that is off for a week must be able to work out
    /// what it missed.
    pub last: Option<chrono::NaiveDate>,
}

/// Read the reminder out of a note's front matter.
///
/// Written by hand rather than by a date library's parser: the three shapes
/// below are what a person types, and a parser that also accepts eleven other
/// shapes accepts eleven ways to be surprised.
///
/// ```text
/// remind: 2026-09-10 09:00
/// repeat: daily 07:30
/// repeat: weekly wed 09:00
/// repeat: monthly 1 09:00
/// ```
pub fn remind(text: &str) -> Remind {
    let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let f = front(&lines);
    let mut out = Remind::default();

    if let Some(v) = f.fields.get("remind") {
        out.once = when(v);
    }
    if let Some(v) = f.fields.get("last") {
        out.last = day(v.trim());
    }
    if let Some(v) = f.fields.get("repeat") {
        let mut it = v.split_whitespace();
        let kind = it.next().unwrap_or("").to_ascii_lowercase();
        match kind.as_str() {
            "daily" => {
                if let Some((h, m)) = clock(it.next().unwrap_or("")) {
                    out.every = Some((Every::Daily, h, m));
                }
            }
            "weekly" => {
                let d = weekday(it.next().unwrap_or(""));
                if let (Some(d), Some((h, m))) = (d, clock(it.next().unwrap_or(""))) {
                    out.every = Some((Every::Weekly(d), h, m));
                }
            }
            "monthly" => {
                let d: Option<u32> = it.next().and_then(|s| s.parse().ok());
                if let (Some(d), Some((h, m))) = (d, clock(it.next().unwrap_or(""))) {
                    if (1..=31).contains(&d) {
                        out.every = Some((Every::Monthly(d), h, m));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// `2026-09-10 09:00` or `2026-09-10T09:00`.
fn when(s: &str) -> Option<chrono::NaiveDateTime> {
    let s = s.trim();
    let (d, t) = s.split_once(['T', ' '])?;
    let d = day(d)?;
    let (h, m) = clock(t)?;
    d.and_hms_opt(h, m, 0)
}

fn day(s: &str) -> Option<chrono::NaiveDate> {
    let mut it = s.trim().split('-');
    let y: i32 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    chrono::NaiveDate::from_ymd_opt(y, m, d)
}

fn clock(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    (h < 24 && m < 60).then_some((h, m))
}

/// `mon`…`sun`, and the Japanese single characters people actually type.
fn weekday(s: &str) -> Option<u32> {
    let s = s.trim().to_ascii_lowercase();
    let names = [
        ("mon", "月"), ("tue", "火"), ("wed", "水"), ("thu", "木"),
        ("fri", "金"), ("sat", "土"), ("sun", "日"),
    ];
    names.iter().position(|(en, ja)| s.starts_with(en) || s == *ja).map(|i| i as u32)
}

/// The days a routine came due between `last` and `today`, inclusive of
/// today.
///
/// **Catching up matters.** A phone that was off for a week, or an app that
/// was not opened, must be able to say what it missed — otherwise a weekly
/// routine quietly becomes "whenever you happen to open cian".
///
/// Capped, because a note whose `last` is two years old should produce a
/// handful of copies and not seven hundred.
pub fn due_since(
    every: Every,
    last: Option<chrono::NaiveDate>,
    today: chrono::NaiveDate,
) -> Vec<chrono::NaiveDate> {
    use chrono::Datelike;
    let from = match last {
        // Never carried out: today counts if today is a day it falls on.
        None => today,
        Some(l) => l.succ_opt().unwrap_or(today),
    };
    let mut out = Vec::new();
    let mut d = from;
    while d <= today && out.len() < 32 {
        let hit = match every {
            Every::Daily => true,
            Every::Weekly(w) => d.weekday().num_days_from_monday() == w,
            Every::Monthly(day) => due_on(day, d),
        };
        if hit {
            out.push(d);
        }
        let Some(next) = d.succ_opt() else { break };
        d = next;
    }
    out
}

/// Whether a monthly routine falls on this day.
///
/// The 31st in a month of thirty lands on the last day of it — otherwise
/// 「毎月31日」 quietly means 「7か月だけ」. One answer, asked by both the
/// catching-up and the next-time-round.
fn due_on(want: u32, d: chrono::NaiveDate) -> bool {
    use chrono::Datelike;
    d.day() == want.min(last_day(d.year(), d.month()))
}

fn last_day(year: i32, month: u32) -> u32 {
    let (y, m) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    chrono::NaiveDate::from_ymd_opt(y, m, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| chrono::Datelike::day(&d))
        .unwrap_or(28)
}

/// A note the search matched, and the first line worth showing for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: std::path::PathBuf,
    /// 1-based, as it is shown. 0 when nothing on one line matched.
    pub line: usize,
    pub text: String,
}

/// ノートを本文ごと探す。**一本につき一行**。
///
/// 前は cian の `search`（総当たりの grep）を借りていた。借りるのをやめた
/// 理由は二つ。**ひとつ、あちらは `cloud` を引きずる** ── ノートのアプリが
/// OneDrive のプレースホルダ判定を積む理由が無い。**ふたつ、探し方が違った** ──
/// あちらは打った文字列をそのまま含むかを見る。こちらは [`hits`] を通すので、
/// 窓の `/` 絞り込みと同じ **AND と OR**（`OR` / `or` / `|` / `｜`）が効く。
/// 同じ言葉で探して同じものが出る、が二つの前端の間で成り立つ。
///
/// 探す先は題・タグ・本文。一行に全部の語が揃っている必要は無いので、
/// 見せる行は「どれか一語を含む最初の行」── 揃っていないから出せない、より
/// 一行でも見せたほうが「なぜ当たったか」に近い。
pub fn find(
    dir: &std::path::Path,
    query: &str,
    limit: usize,
    limits: crate::survey::Limits,
    stop: &std::sync::atomic::AtomicBool,
) -> Vec<Hit> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let words: Vec<String> = terms(query).into_iter().flatten().collect();
    let (found, _) = list(dir, limits, stop);
    let mut out = Vec::new();
    for f in found {
        if out.len() >= limit || stop.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        let Ok(file) = crate::text::read(&f.note.path) else { continue };
        let body = file.lines.join("\n");
        let hay = format!("{}\n{}\n{}", f.note.title, f.note.tags.join(" "), body);
        if !hits(&hay, query) {
            continue;
        }
        let (line, text) = file
            .lines
            .iter()
            .enumerate()
            .find(|(_, l)| {
                let low = l.to_lowercase();
                words.iter().any(|w| low.contains(&w.to_lowercase()))
            })
            .map(|(i, l)| (i + 1, l.trim().to_string()))
            .unwrap_or((0, String::new()));
        out.push(Hit { path: f.note.path.clone(), line, text });
    }
    out
}

#[cfg(test)]
mod find_tests {
    use super::*;

    fn write(dir: &std::path::Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn 本文とタグと題から探す() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.md", "---\ntitle: 段取り\ntags: [仕事]\n---\n明日は会議。\n");
        write(d.path(), "b.md", "# 買い物\n牛乳とパン\n");
        let stop = std::sync::atomic::AtomicBool::new(false);
        let lim = crate::survey::Limits::default();

        let r = find(d.path(), "会議", 20, lim, &stop);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].text, "明日は会議。");
        assert_eq!(r[0].line, 5, "front matter を数えた行番号であること");

        // 題からも、タグからも当たる。
        assert_eq!(find(d.path(), "段取り", 20, lim, &stop).len(), 1);
        assert_eq!(find(d.path(), "仕事", 20, lim, &stop).len(), 1);
    }

    #[test]
    fn and_と_or_が効く() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.md", "# 一\n牛乳とパン\n");
        write(d.path(), "b.md", "# 二\n牛乳だけ\n");
        let stop = std::sync::atomic::AtomicBool::new(false);
        let lim = crate::survey::Limits::default();
        // AND ── 両方ある一本だけ。**借りていた grep はこれができなかった。**
        assert_eq!(find(d.path(), "牛乳 パン", 20, lim, &stop).len(), 1);
        // OR ── どちらかで二本とも。
        assert_eq!(find(d.path(), "パン OR だけ", 20, lim, &stop).len(), 2);
    }

    #[test]
    fn 空の問いでは何も返さない() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.md", "# 一\n本文\n");
        let stop = std::sync::atomic::AtomicBool::new(false);
        assert!(find(d.path(), "   ", 20, crate::survey::Limits::default(), &stop).is_empty());
    }
}
