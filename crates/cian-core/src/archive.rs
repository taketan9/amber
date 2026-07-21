//! Looking inside a zip, and taking things out of it.
//!
//! Browsing an archive as though it were a directory is the feature people
//! come to a two-pane file manager for, and the thing they miss first when it
//! is absent. Only zip is handled: it is what Windows produces and consumes
//! without extra software, and claiming to open archives while silently
//! failing on half of them would be worse than being clear about the one.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use anyhow::{Context, Result};

use crate::ops::OpReport;
use crate::progress::{Ctl, Progress};

/// One member of an archive.
#[derive(Debug, Clone)]
pub struct Member {
    /// Path within the archive.
    pub name: String,
    pub is_dir: bool,
    /// Size once extracted.
    pub size: u64,
    pub compressed: u64,
}

/// Whether cian can look inside this file, judged by extension.
///
/// Extension rather than content sniffing: the answer decides what a keypress
/// does, so it has to be known before the file is opened, and being wrong
/// merely means falling back to the viewer.
pub fn is_archive(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(),
        Some("zip") | Some("jar") | Some("xpi") | Some("whl") | Some("epub")
    )
}

/// List an archive's contents.
pub fn list(path: &Path) -> Result<Vec<Member>> {
    let f = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut zip = zip::ZipArchive::new(f)
        .with_context(|| format!("not a readable zip: {}", path.display()))?;
    let mut out = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let e = zip.by_index(i).with_context(|| format!("entry {} of {}", i, path.display()))?;
        out.push(Member {
            name: e.name().to_string(),
            is_dir: e.is_dir(),
            size: e.size(),
            compressed: e.compressed_size(),
        });
    }
    // Archives are stored in whatever order they were written; a listing wants
    // to be predictable.
    out.sort_by_key(|m| m.name.to_lowercase());
    Ok(out)
}

/// Reject member paths that would escape the destination directory.
///
/// A zip can name `../../etc/passwd`, or an absolute path, and a naive
/// extractor will happily write there. Everything is resolved to a plain
/// relative path underneath `dest` or refused.
fn safe_join(dest: &Path, name: &str) -> Option<PathBuf> {
    let mut out = dest.to_path_buf();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => return None,
            p => {
                // A Windows drive letter or a leading slash would otherwise
                // make the join absolute and throw away `dest` entirely.
                if p.contains(':') {
                    return None;
                }
                out.push(p);
            }
        }
    }
    if out.starts_with(dest) {
        Some(out)
    } else {
        None
    }
}

/// Extract `members` — or all of them, if empty — into `dest`.
pub fn extract(
    archive: &Path,
    members: &[String],
    dest: &Path,
    ctl: &mut Ctl,
) -> OpReport {
    let mut report = OpReport::default();
    let f = match fs::File::open(archive) {
        Ok(f) => f,
        Err(e) => {
            report.note_error(format!("{}: {}", archive.display(), e));
            return report;
        }
    };
    let mut zip = match zip::ZipArchive::new(f) {
        Ok(z) => z,
        Err(e) => {
            report.note_error(format!("{}: {}", archive.display(), e));
            return report;
        }
    };

    let wanted: Vec<usize> = (0..zip.len())
        .filter(|i| {
            members.is_empty()
                || zip.by_index(*i).map(|e| members.iter().any(|m| m == e.name())).unwrap_or(false)
        })
        .collect();
    let mut p = Progress {
        files_total: wanted.len(),
        bytes_total: wanted
            .iter()
            .filter_map(|i| zip.by_index(*i).ok().map(|e| e.size()))
            .sum(),
        ..Default::default()
    };

    for i in wanted {
        if ctl.cancel.load(Ordering::Relaxed) {
            break;
        }
        let mut e = match zip.by_index(i) {
            Ok(e) => e,
            Err(err) => {
                report.note_error(format!("entry {}: {}", i, err));
                continue;
            }
        };
        let name = e.name().to_string();
        let Some(target) = safe_join(dest, &name) else {
            report.note_error(format!("{}: refused, escapes the destination", name));
            continue;
        };
        p.current = name.clone();
        (ctl.on_progress)(&p);

        if e.is_dir() {
            if let Err(err) = fs::create_dir_all(&target) {
                report.note_error(format!("{}: {}", name, err));
            }
            continue;
        }
        if let Some(parent) = target.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                report.note_error(format!("{}: {}", name, err));
                continue;
            }
        }
        let mut out = match fs::File::create(&target) {
            Ok(f) => f,
            Err(err) => {
                report.note_error(format!("{}: {}", name, err));
                continue;
            }
        };
        // Copied in chunks so progress moves inside a large member and a
        // cancel is noticed without waiting for it to finish.
        let mut buf = vec![0u8; 256 * 1024];
        let mut failed = false;
        loop {
            if ctl.cancel.load(Ordering::Relaxed) {
                drop(out);
                let _ = fs::remove_file(&target);
                return report;
            }
            match e.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    use std::io::Write as _;
                    if let Err(err) = out.write_all(&buf[..n]) {
                        report.note_error(format!("{}: {}", name, err));
                        failed = true;
                        break;
                    }
                    p.bytes_done += n as u64;
                    (ctl.on_progress)(&p);
                }
                Err(err) => {
                    report.note_error(format!("{}: {}", name, err));
                    failed = true;
                    break;
                }
            }
        }
        if !failed {
            report.ok += 1;
        }
        p.files_done += 1;
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::AtomicBool;

    fn make_zip(dir: &Path) -> PathBuf {
        let path = dir.join("a.zip");
        let f = fs::File::create(&path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
        w.add_directory("sub/", opts).unwrap();
        w.start_file("readme.txt", opts).unwrap();
        w.write_all(b"hello from the archive").unwrap();
        w.start_file("sub/inner.txt", opts).unwrap();
        w.write_all(b"nested").unwrap();
        w.finish().unwrap();
        path
    }

    fn ctl<'a>(c: &'a AtomicBool, f: &'a mut dyn FnMut(&Progress)) -> Ctl<'a> {
        Ctl { cancel: c, on_progress: f }
    }

    #[test]
    fn recognises_archives_by_extension() {
        assert!(is_archive(Path::new("x.zip")));
        assert!(is_archive(Path::new("X.ZIP")));
        assert!(is_archive(Path::new("lib.jar")));
        assert!(!is_archive(Path::new("notes.txt")));
        assert!(!is_archive(Path::new("noext")));
    }

    #[test]
    fn lists_members_sorted_with_sizes() {
        let d = tempfile::tempdir().unwrap();
        let z = make_zip(d.path());
        let members = list(&z).unwrap();
        let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["readme.txt", "sub/", "sub/inner.txt"]);
        let readme = &members[0];
        assert_eq!(readme.size, "hello from the archive".len() as u64);
        assert!(members[1].is_dir);
    }

    #[test]
    fn extracts_everything_when_no_member_is_named() {
        let d = tempfile::tempdir().unwrap();
        let z = make_zip(d.path());
        let out = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut n = |_: &Progress| {};
        let report = extract(&z, &[], out.path(), &mut ctl(&cancel, &mut n));

        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(fs::read_to_string(out.path().join("readme.txt")).unwrap(), "hello from the archive");
        assert_eq!(fs::read_to_string(out.path().join("sub/inner.txt")).unwrap(), "nested");
    }

    #[test]
    fn extracts_only_the_named_member() {
        let d = tempfile::tempdir().unwrap();
        let z = make_zip(d.path());
        let out = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut n = |_: &Progress| {};
        extract(&z, &["readme.txt".to_string()], out.path(), &mut ctl(&cancel, &mut n));

        assert!(out.path().join("readme.txt").exists());
        assert!(!out.path().join("sub/inner.txt").exists(), "only what was asked for");
    }

    /// A zip can name `../../etc/passwd`; a naive extractor writes there.
    #[test]
    fn member_paths_cannot_escape_the_destination() {
        let dest = Path::new("/tmp/dest");
        assert_eq!(safe_join(dest, "a/b.txt"), Some(PathBuf::from("/tmp/dest/a/b.txt")));
        assert_eq!(safe_join(dest, "./a.txt"), Some(PathBuf::from("/tmp/dest/a.txt")));
        // Traversal, absolute paths and drive letters are all refused.
        assert_eq!(safe_join(dest, "../evil"), None);
        assert_eq!(safe_join(dest, "a/../../evil"), None);
        assert_eq!(safe_join(dest, "C:/Windows/system32/evil"), None);
        // A leading slash is stripped rather than making the join absolute.
        assert_eq!(safe_join(dest, "/etc/passwd"), Some(PathBuf::from("/tmp/dest/etc/passwd")));
    }

    #[test]
    fn a_traversing_member_is_reported_and_the_rest_still_extract() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("evil.zip");
        {
            let f = fs::File::create(&path).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            w.start_file("../escaped.txt", opts).unwrap();
            w.write_all(b"nope").unwrap();
            w.start_file("fine.txt", opts).unwrap();
            w.write_all(b"ok").unwrap();
            w.finish().unwrap();
        }
        let out = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut n = |_: &Progress| {};
        let report = extract(&path, &[], out.path(), &mut ctl(&cancel, &mut n));

        assert_eq!(report.ok, 1, "the safe member still came out");
        assert!(report.errors.iter().any(|e| e.contains("escapes")), "{:?}", report.errors);
        assert!(out.path().join("fine.txt").exists());
        assert!(!out.path().parent().unwrap().join("escaped.txt").exists());
    }

    #[test]
    fn a_file_that_is_not_a_zip_reports_rather_than_panicking() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("notazip.zip");
        fs::write(&f, b"just text").unwrap();
        assert!(list(&f).is_err());
    }
}
