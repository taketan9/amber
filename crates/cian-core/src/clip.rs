//! Files held for a later paste, Explorer-style: copy or cut here, walk
//! somewhere else, paste there.
//!
//! The rules live here rather than in either front end because there are two
//! of them now and the interesting parts are judgements, not plumbing: a cut
//! is spent by its paste while a copy is not, cian's own register outranks the
//! system clipboard, and pasting into the directory the files are already in
//! is refused. Written twice, those drift — and the last time a copy rule was
//! written twice, one of the copies emptied the file it was copying.

use std::path::{Path, PathBuf};

/// Whether the held files will be copied or moved when pasted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Copy,
    Cut,
}

/// What is held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clipboard {
    pub paths: Vec<PathBuf>,
    pub op: Op,
}

/// What a paste would do, decided before anything is touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Paste {
    /// Neither register holds anything.
    Empty,
    /// The files are already in the destination. Carrying on would be a no-op
    /// at best and, when the copy resolves onto the source, a truncation.
    AlreadyHere,
    Go {
        paths: Vec<PathBuf>,
        op: Op,
        /// True when these came from the system clipboard rather than from
        /// cian's own. The caller says so, because "paste" is otherwise
        /// ambiguous about which of the two clipboards it just used.
        from_os: bool,
    },
}

/// Work out what pasting into `dest` would do.
///
/// `os` is only consulted when cian's own register is empty — that one was
/// filled deliberately, from here, so it wins. The system clipboard is the
/// fallback, so a file copied in Explorer or Finder pastes as expected.
pub fn plan(own: Option<&Clipboard>, os: impl FnOnce() -> Vec<PathBuf>, dest: &Path) -> Paste {
    let (paths, op, from_os) = match own {
        Some(c) if !c.paths.is_empty() => (c.paths.clone(), c.op, false),
        _ => {
            let paths = os();
            if paths.is_empty() {
                return Paste::Empty;
            }
            (paths, Op::Copy, true)
        }
    };
    if paths.iter().any(|p| p.parent() == Some(dest)) {
        return Paste::AlreadyHere;
    }
    Paste::Go { paths, op, from_os }
}

/// Whether the register survives a paste that has just been carried out.
///
/// A cut is spent — the files are no longer where it pointed. A copy is not,
/// so the same set can be dropped in several places, which is the whole reason
/// to have a register rather than to just move things.
pub fn survives(op: Op) -> bool {
    op == Op::Copy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(paths: &[&str], op: Op) -> Clipboard {
        Clipboard { paths: paths.iter().map(PathBuf::from).collect(), op }
    }

    #[test]
    fn own_register_beats_the_system_one() {
        let own = c(&["/a/one.txt"], Op::Cut);
        let got = plan(Some(&own), || vec![PathBuf::from("/b/other.txt")], Path::new("/dest"));
        assert_eq!(got, Paste::Go {
            paths: vec![PathBuf::from("/a/one.txt")],
            op: Op::Cut,
            from_os: false,
        });
    }

    #[test]
    fn falls_back_to_the_system_clipboard_as_a_copy() {
        let got = plan(None, || vec![PathBuf::from("/b/other.txt")], Path::new("/dest"));
        assert_eq!(got, Paste::Go {
            paths: vec![PathBuf::from("/b/other.txt")],
            op: Op::Copy,
            from_os: true,
        });
    }

    #[test]
    fn an_empty_own_register_is_not_treated_as_held() {
        let own = c(&[], Op::Copy);
        assert_eq!(plan(Some(&own), Vec::new, Path::new("/dest")), Paste::Empty);
    }

    #[test]
    fn refuses_a_paste_into_the_directory_the_files_are_in() {
        let own = c(&["/dest/one.txt"], Op::Copy);
        assert_eq!(plan(Some(&own), Vec::new, Path::new("/dest")), Paste::AlreadyHere);
    }

    #[test]
    fn refuses_when_only_one_of_the_set_is_already_there() {
        // Half a paste is worse than none: the ones that could move would,
        // and the report would say it worked.
        let own = c(&["/elsewhere/a.txt", "/dest/b.txt"], Op::Copy);
        assert_eq!(plan(Some(&own), Vec::new, Path::new("/dest")), Paste::AlreadyHere);
    }

    #[test]
    fn a_cut_is_spent_and_a_copy_is_not() {
        assert!(!survives(Op::Cut));
        assert!(survives(Op::Copy));
    }
}
