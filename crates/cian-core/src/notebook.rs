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
