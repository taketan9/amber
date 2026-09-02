//! "Has this file changed since I read it?"
//!
//! **cian's save wrote unconditionally.** It kept the encoding, the BOM and
//! the line endings the file arrived with — everything about *how* to write —
//! and never asked whether the thing it was about to write over was still the
//! thing it had read. Two people editing one note on a shared drive both
//! saved, and the second one silently erased the first. Nothing on screen
//! said so, because nothing had looked.
//!
//! That is a hazard on any shared folder — a synced OneDrive library, a
//! SharePoint mount over WebDAV, an NFS home — and it costs one `metadata`
//! call to notice.
//!
//! **What this cannot catch**: a change made within the same second that
//! leaves the file exactly as long. Filesystems keep mtime to a second on
//! some volumes, so that pair really can repeat. Catching it would mean
//! hashing the contents on every read, which is a file-sized cost paid on
//! every open to close a hole this small. The trade is written down rather
//! than papered over: if the length and the timestamp both match, this says
//! unchanged, and it can be wrong.

use std::path::Path;
use std::time::SystemTime;

/// What a file looked like when it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    pub len: u64,
    /// `None` where the filesystem would not say — a stamp with no time
    /// compares on length alone, which is weaker but still catches the
    /// common case.
    pub modified: Option<SystemTime>,
}

/// Take a file's stamp. `None` when it is not there — which is itself an
/// answer: a file that has since been created where none was is a change.
pub fn of(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    Some(Stamp { len: m.len(), modified: m.modified().ok() })
}

/// Did the file move under us?
///
/// A file that has *gone* counts as changed: writing would put it back
/// without anyone asking, and somebody deleted it on purpose.
pub fn changed(path: &Path, since: &Stamp) -> bool {
    match of(path) {
        Some(now) => now != *since,
        None => true,
    }
}

/// How to say it to a person: what is different, not that something is.
pub fn describe(path: &Path, since: &Stamp) -> String {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    match of(path) {
        None => format!("{name} は消えています"),
        Some(now) if now.len != since.len => {
            let (a, b) = (since.len, now.len);
            format!("{name} は開いたあとで変わっています（{a} → {b} バイト）")
        }
        Some(_) => format!("{name} は開いたあとで書き換えられています"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(p: &Path, s: &str) {
        let mut f = std::fs::File::create(p).unwrap();
        f.write_all(s.as_bytes()).unwrap();
    }

    #[test]
    fn an_untouched_file_is_unchanged() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("note.md");
        write(&p, "one\n");
        let s = of(&p).unwrap();
        assert!(!changed(&p, &s));
    }

    /// The case this exists for: somebody else wrote to it while it was open.
    #[test]
    fn a_different_length_is_a_change() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("note.md");
        write(&p, "one\n");
        let s = of(&p).unwrap();
        write(&p, "one\ntwo\n");
        assert!(changed(&p, &s));
        assert!(describe(&p, &s).contains("4 → 8"), "{}", describe(&p, &s));
    }

    /// **A file that has gone is a change.** Saving would put it back, and
    /// somebody removed it deliberately.
    #[test]
    fn a_missing_file_is_a_change() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("note.md");
        write(&p, "one\n");
        let s = of(&p).unwrap();
        std::fs::remove_file(&p).unwrap();
        assert!(changed(&p, &s));
        assert!(describe(&p, &s).contains("消えています"));
    }

    /// A rewrite of the same length, far enough apart in time to be seen.
    /// (Within one second it cannot be — that hole is in the module's doc.)
    #[test]
    fn the_same_length_at_a_different_time_is_a_change() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("note.md");
        write(&p, "one\n");
        let s = of(&p).unwrap();
        // Set the time back rather than sleeping: a test that waits a second
        // is a test people start skipping.
        let old = std::fs::metadata(&p).unwrap().modified().unwrap()
            - std::time::Duration::from_secs(120);
        std::fs::OpenOptions::new().write(true).open(&p).unwrap().set_modified(old).unwrap();
        assert!(changed(&p, &s));
    }
}
