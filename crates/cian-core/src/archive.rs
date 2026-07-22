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

/// Build a zip at `dest` from `sources`, optionally encrypted with `password`.
///
/// Directories are added recursively, with their tree preserved relative to
/// the source's own name, so zipping `proj/` yields members `proj/...`. When a
/// password is given the members are AES-256 encrypted — strong, but note that
/// Windows Explorer's built-in unzip cannot open AES zips (7-Zip and the like
/// can); the caller is expected to have warned about that.
pub fn create_zip(
    sources: &[PathBuf],
    dest: &Path,
    password: Option<&str>,
    ctl: &mut Ctl,
) -> OpReport {
    use std::io::Write as _;

    let mut report = OpReport::default();
    let f = match fs::File::create(dest) {
        Ok(f) => f,
        Err(e) => {
            report.note_error(format!("{}: {}", dest.display(), e));
            return report;
        }
    };
    let mut zip = zip::ZipWriter::new(f);

    // Gather the members first so progress has a total and so an empty
    // selection is caught before a file is created.
    let mut jobs: Vec<(PathBuf, String)> = Vec::new();
    for src in sources {
        let base = match src.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => {
                report.note_error(format!("{}: unusable name", src.display()));
                continue;
            }
        };
        if src.is_dir() {
            collect_tree(src, &base, &mut jobs, &mut report);
        } else {
            jobs.push((src.clone(), base));
        }
    }

    let mut p = Progress {
        files_total: jobs.len(),
        bytes_total: jobs.iter().filter_map(|(pth, _)| fs::metadata(pth).ok().map(|m| m.len())).sum(),
        ..Default::default()
    };

    let base_opts: zip::write::FileOptions<()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    for (path, name) in jobs {
        if ctl.cancel.load(Ordering::Relaxed) {
            drop(zip);
            let _ = fs::remove_file(dest);
            return report;
        }
        p.current = name.clone();
        (ctl.on_progress)(&p);

        // `FileOptions` is Copy, so `base_opts` is reused each iteration.
        let opts = match password {
            Some(pw) => base_opts.with_aes_encryption(zip::AesMode::Aes256, pw),
            None => base_opts,
        };
        if let Err(e) = zip.start_file(&name, opts) {
            report.note_error(format!("{}: {}", name, e));
            continue;
        }
        let mut src_f = match fs::File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                report.note_error(format!("{}: {}", name, e));
                continue;
            }
        };
        let mut buf = vec![0u8; 256 * 1024];
        let mut failed = false;
        loop {
            match src_f.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Err(e) = zip.write_all(&buf[..n]) {
                        report.note_error(format!("{}: {}", name, e));
                        failed = true;
                        break;
                    }
                    p.bytes_done += n as u64;
                    (ctl.on_progress)(&p);
                }
                Err(e) => {
                    report.note_error(format!("{}: {}", name, e));
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

    if let Err(e) = zip.finish() {
        report.note_error(format!("{}: {}", dest.display(), e));
    }
    report
}

/// Add every file under `dir` to `jobs`, naming each member relative to
/// `prefix` so the directory structure is kept inside the zip.
fn collect_tree(dir: &Path, prefix: &str, jobs: &mut Vec<(PathBuf, String)>, report: &mut OpReport) {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            report.note_error(format!("{}: {}", dir.display(), e));
            return;
        }
    };
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        // Forward slashes: the zip format mandates them, and a backslash from
        // Windows would otherwise be stored as part of the name.
        let member = format!("{}/{}", prefix, name);
        if path.is_dir() {
            collect_tree(&path, &member, jobs, report);
        } else {
            jobs.push((path, member));
        }
    }
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

    #[test]
    fn creates_a_zip_preserving_directory_structure() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("proj/sub")).unwrap();
        fs::write(d.path().join("proj/top.txt"), b"top").unwrap();
        fs::write(d.path().join("proj/sub/inner.txt"), b"inner").unwrap();
        fs::write(d.path().join("loose.txt"), b"loose").unwrap();

        let out = d.path().join("bundle.zip");
        let cancel = AtomicBool::new(false);
        let mut n = |_: &Progress| {};
        let report = create_zip(
            &[d.path().join("proj"), d.path().join("loose.txt")],
            &out,
            None,
            &mut ctl(&cancel, &mut n),
        );
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.ok, 3);

        let names: Vec<String> = list(&out).unwrap().into_iter().map(|m| m.name).collect();
        assert!(names.contains(&"proj/top.txt".to_string()), "{:?}", names);
        assert!(names.contains(&"proj/sub/inner.txt".to_string()), "{:?}", names);
        assert!(names.contains(&"loose.txt".to_string()), "{:?}", names);
    }

    /// A password-protected member must actually refuse the wrong password and
    /// yield its contents with the right one.
    #[test]
    fn a_password_zip_round_trips_only_with_the_password() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("secret.txt"), b"classified").unwrap();
        let out = d.path().join("locked.zip");
        let cancel = AtomicBool::new(false);
        let mut n = |_: &Progress| {};
        let report =
            create_zip(&[d.path().join("secret.txt")], &out, Some("hunter2"), &mut ctl(&cancel, &mut n));
        assert_eq!(report.ok, 1, "{:?}", report.errors);

        let f = fs::File::open(&out).unwrap();
        let mut zip = zip::ZipArchive::new(f).unwrap();
        // Wrong password is refused.
        assert!(zip.by_name_decrypt("secret.txt", b"wrong").is_err());
        // Right password yields the bytes.
        let mut e = zip.by_name_decrypt("secret.txt", b"hunter2").unwrap();
        let mut got = String::new();
        e.read_to_string(&mut got).unwrap();
        assert_eq!(got, "classified");
    }

    #[test]
    fn cancelling_a_zip_leaves_no_half_file_behind() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("a.txt"), b"data").unwrap();
        let out = d.path().join("x.zip");
        let cancel = AtomicBool::new(true);
        let mut n = |_: &Progress| {};
        create_zip(&[d.path().join("a.txt")], &out, None, &mut ctl(&cancel, &mut n));
        assert!(!out.exists(), "a cancelled zip is cleaned up");
    }
}
