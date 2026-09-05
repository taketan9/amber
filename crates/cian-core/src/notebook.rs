//! What a folder of notes knows about itself.
//!
//! Two things do not fit inside a note: what colour a **folder** is (there is
//! no note to write it on), and the favourite folders that are **empty** (a
//! folder that exists only in the notes that name it disappears the moment
//! the last one leaves). Both go in one small JSON file beside the notes, at
//! `.cian/settings.json`.
//!
//! **Everything else stays in the notes.** Whether a note is a favourite is
//! written on the note, so losing this file loses only the colours and the
//! empty folders — never a note's own place. That is the test for whether
//! something belongs here: if losing it would lose something you wrote, it
//! does not.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Book {
    /// Folder path (relative to the root) → the colour it was given.
    ///
    /// Free-form, because it is his to choose: the app suggests a palette and
    /// does not enforce one.
    pub colors: BTreeMap<String, String>,
    /// Favourite folders, including the ones nothing is in yet.
    pub stars: Vec<String>,
}

pub fn file(root: &Path) -> PathBuf {
    root.join(".cian").join("settings.json")
}

/// What the folder says about itself, or the defaults.
///
/// **Never an error.** A missing file is a folder that has not been given a
/// colour yet, and a corrupt one is not a reason to refuse to show the notes
/// — the notes are the thing, and this is decoration and bookkeeping.
pub fn read(root: &Path) -> Book {
    let Ok(text) = std::fs::read_to_string(file(root)) else { return Book::default() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Book::default() };
    let mut b = Book::default();
    if let Some(m) = v.get("colors").and_then(|c| c.as_object()) {
        for (k, val) in m {
            if let Some(s) = val.as_str() {
                b.colors.insert(k.clone(), s.to_string());
            }
        }
    }
    if let Some(a) = v.get("stars").and_then(|s| s.as_array()) {
        b.stars = a.iter().filter_map(|s| s.as_str()).map(str::to_string).collect();
    }
    b
}

/// Write it back, making `.cian` if it is not there.
pub fn write(root: &Path, b: &Book) -> anyhow::Result<()> {
    let at = file(root);
    if let Some(dir) = at.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let v = serde_json::json!({ "colors": b.colors, "stars": b.stars });
    std::fs::write(at, serde_json::to_string_pretty(&v)?)?;
    Ok(())
}

/// Give a folder a colour, or take it away.
pub fn set_color(root: &Path, folder: &str, color: Option<&str>) -> anyhow::Result<()> {
    let mut b = read(root);
    match color {
        Some(c) => b.colors.insert(folder.to_string(), c.to_string()),
        None => b.colors.remove(folder),
    };
    write(root, &b)
}

/// Remember a favourite folder, so that an empty one still exists tomorrow.
pub fn add_star(root: &Path, folder: &str) -> anyhow::Result<()> {
    let mut b = read(root);
    if folder.is_empty() || b.stars.iter().any(|s| s == folder) {
        return Ok(());
    }
    b.stars.push(folder.to_string());
    b.stars.sort();
    write(root, &b)
}

/// Forget one, and everything under it.
pub fn drop_star(root: &Path, folder: &str) -> anyhow::Result<()> {
    let mut b = read(root);
    let under = format!("{folder}/");
    b.stars.retain(|s| s != folder && !s.starts_with(&under));
    write(root, &b)
}

/// Everything under `from`, moved to `to`.
///
/// **Copy, check, then remove — in that order.** Between two cloud providers
/// this is not a rename: the bytes have to be written on the far side before
/// anything is taken off this one, and if any of it fails the originals are
/// still there. A note lost in the middle of moving is the worst thing a
/// notes app can do.
///
/// Nothing is overwritten. A name already taken on the far side stops the
/// whole move — merging two folders of notes is a decision, and this is not
/// the moment to make it for somebody.
///
/// Everything, not only the notes: the pictures live in `attachments/` beside
/// them and `.cian` holds the colours and the empty shelves. A move that took
/// the notes and left the pictures would break every note with a picture.
pub fn migrate(from: &Path, to: &Path) -> anyhow::Result<usize> {
    let from = from.canonicalize()?;
    let to = to.canonicalize()?;
    if from == to {
        return Ok(0);
    }
    if to.starts_with(&from) {
        anyhow::bail!("移す先が、いまの場所の中にあります");
    }

    let mut jobs: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut walk = vec![from.clone()];
    while let Some(dir) = walk.pop() {
        for e in std::fs::read_dir(&dir)? {
            let at = e?.path();
            let Ok(rel) = at.strip_prefix(&from) else { continue };
            let dest = to.join(rel);
            if at.is_dir() {
                walk.push(at);
                std::fs::create_dir_all(&dest)?;
            } else {
                if dest.exists() {
                    anyhow::bail!("移す先に同じ名前があります: {}", rel.display());
                }
                jobs.push((at, dest));
            }
        }
    }
    for (src, dest) in &jobs {
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::copy(src, dest)?;
    }
    // Only now. Every byte is on the far side.
    for (src, _) in &jobs {
        let _ = std::fs::remove_file(src);
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut walk = vec![from.clone()];
    while let Some(dir) = walk.pop() {
        for e in std::fs::read_dir(&dir)? {
            let at = e?.path();
            if at.is_dir() {
                walk.push(at.clone());
                dirs.push(at);
            }
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        let _ = std::fs::remove_dir(d);
    }
    Ok(jobs.len())
}

/// A backup, put back. Returns (put in, left alone).
///
/// **Into the notes folder, never over a note.** A restore that overwrote
/// would be a restore that could lose today's work to last week's copy.
pub fn restore(zip: &Path, to: &Path) -> anyhow::Result<(usize, usize)> {
    std::fs::create_dir_all(to)?;
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut nothing = |_: &crate::progress::Progress| {};
    let mut ctl = crate::progress::Ctl { cancel: &cancel, on_progress: &mut nothing };

    // Into a room of its own first, so a half-unpacked archive never stands
    // among the notes.
    let hold = std::env::temp_dir().join(format!("cian-restore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&hold);
    std::fs::create_dir_all(&hold)?;

    let members: Vec<String> = crate::archive::list(zip)?.into_iter().map(|m| m.name).collect();
    // **The folder's own name comes off.** A backup of the notes folder is a
    // zip *of that folder*, so every member is `ノート/…` — put back as-is it
    // makes a `ノート` inside the notes, and every note in it looks new.
    // Stripped only when the whole archive is under one name.
    let top = members
        .iter()
        .filter_map(|m| m.split('/').next())
        .collect::<std::collections::BTreeSet<_>>();
    let strip = if top.len() == 1 && members.iter().any(|m| m.contains('/')) {
        top.into_iter().next().unwrap_or("").to_string()
    } else {
        String::new()
    };
    let report = crate::archive::extract(zip, &members, &hold, None, &strip, &mut ctl);
    if !report.errors.is_empty() {
        let _ = std::fs::remove_dir_all(&hold);
        anyhow::bail!("{}", report.errors.join(" / "));
    }

    let mut put = 0usize;
    let mut kept = 0usize;
    let mut walk = vec![hold.clone()];
    while let Some(dir) = walk.pop() {
        for e in std::fs::read_dir(&dir)? {
            let at = e?.path();
            let Ok(rel) = at.strip_prefix(&hold) else { continue };
            let dest = to.join(rel);
            if at.is_dir() {
                walk.push(at);
                std::fs::create_dir_all(&dest)?;
            } else if dest.exists() {
                kept += 1;
            } else {
                if let Some(d) = dest.parent() {
                    std::fs::create_dir_all(d)?;
                }
                std::fs::copy(&at, &dest)?;
                put += 1;
            }
        }
    }
    let _ = std::fs::remove_dir_all(&hold);
    Ok((put, kept))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_remembers_its_colour_and_its_empty_favourites() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(read(root), Book::default());

        set_color(root, "仕事", Some("#D9822B")).unwrap();
        add_star(root, "買い物").unwrap();
        add_star(root, "買い物/週次").unwrap();
        add_star(root, "買い物").unwrap(); // 二度足しても増えない

        let b = read(root);
        assert_eq!(b.colors.get("仕事").map(String::as_str), Some("#D9822B"));
        assert_eq!(b.stars, vec!["買い物".to_string(), "買い物/週次".to_string()]);

        // 親を消すと下も消える ── 残った子は、辿り着けない場所になる。
        drop_star(root, "買い物").unwrap();
        assert!(read(root).stars.is_empty());

        set_color(root, "仕事", None).unwrap();
        assert!(read(root).colors.is_empty());
    }

    #[test]
    fn a_broken_settings_file_is_not_a_reason_to_lose_the_notes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".cian")).unwrap();
        std::fs::write(file(dir.path()), "{ これは JSON ではない").unwrap();
        assert_eq!(read(dir.path()), Book::default());
    }
}
