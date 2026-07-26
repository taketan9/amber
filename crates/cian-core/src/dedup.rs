//! Find duplicate files by content.
//!
//! Two files are duplicates when their bytes are identical. Comparing every
//! pair would be quadratic and read far too much, so this groups by size first
//! (a cheap `stat`) and only hashes the files that collide on size — most files
//! are a unique length and never get read. Hashing is cancellable, since the
//! files worth de-duplicating are the big ones.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::attrs::{hash_file, HashKind};

/// Group `paths` by identical content, returning only the groups of two or
/// more (the actual duplicates), each sorted, with the largest groups first.
///
/// Zero-length files are skipped: every empty file hashes the same, and calling
/// a directory full of empty markers "duplicates" is noise, not a finding.
/// Cancellation returns whatever groups were completed so far.
pub fn find_duplicates(paths: &[PathBuf], cancel: &AtomicBool) -> Vec<Vec<PathBuf>> {
    // Bucket by size; only sizes shared by 2+ files can hold duplicates.
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for p in paths {
        if cancel.load(Ordering::Relaxed) {
            return Vec::new();
        }
        if let Ok(m) = std::fs::metadata(p) {
            if m.is_file() && m.len() > 0 {
                by_size.entry(m.len()).or_default().push(p.clone());
            }
        }
    }

    let mut groups: Vec<Vec<PathBuf>> = Vec::new();
    for (_size, same_size) in by_size {
        if same_size.len() < 2 {
            continue;
        }
        // Hash the size-collisions and regroup by digest.
        let mut by_hash: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for p in same_size {
            if cancel.load(Ordering::Relaxed) {
                return finalize(groups);
            }
            if let Ok(Some(h)) = hash_file(&p, HashKind::Sha256, cancel) {
                by_hash.entry(h).or_default().push(p);
            }
        }
        for (_hash, dupes) in by_hash {
            if dupes.len() >= 2 {
                let mut g = dupes;
                g.sort();
                groups.push(g);
            }
        }
    }
    finalize(groups)
}

/// Largest groups first, so the most wasteful duplicates lead the list.
fn finalize(mut groups: Vec<Vec<PathBuf>>) -> Vec<Vec<PathBuf>> {
    groups.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn groups_identical_files_and_ignores_uniques_and_empties() {
        let d = tempfile::tempdir().unwrap();
        // Two identical, one different, two empty (must be ignored).
        fs::write(d.path().join("a.txt"), b"hello world").unwrap();
        fs::write(d.path().join("b.txt"), b"hello world").unwrap();
        fs::write(d.path().join("c.txt"), b"different").unwrap();
        fs::write(d.path().join("e1"), b"").unwrap();
        fs::write(d.path().join("e2"), b"").unwrap();

        let paths: Vec<PathBuf> = ["a.txt", "b.txt", "c.txt", "e1", "e2"]
            .iter()
            .map(|n| d.path().join(n))
            .collect();
        let cancel = AtomicBool::new(false);
        let groups = find_duplicates(&paths, &cancel);
        assert_eq!(groups.len(), 1, "one duplicate group");
        assert_eq!(groups[0].len(), 2);
        let names: Vec<String> = groups[0]
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.txt", "b.txt"], "sorted within the group");
    }

    #[test]
    fn same_size_but_different_content_is_not_a_duplicate() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("x"), b"abcd").unwrap();
        fs::write(d.path().join("y"), b"abce").unwrap(); // same length, differs
        let paths = vec![d.path().join("x"), d.path().join("y")];
        let cancel = AtomicBool::new(false);
        assert!(find_duplicates(&paths, &cancel).is_empty());
    }
}
