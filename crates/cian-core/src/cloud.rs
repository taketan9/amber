//! Cloud placeholder files — the ones a sync client shows but has not
//! downloaded (OneDrive's "Files On-Demand", iCloud Drive, Google Drive).
//!
//! They look like ordinary files: they have a name, a size, a date. Opening
//! one is what costs — the sync client hydrates it on the spot, over the
//! network. That is fine for a deliberate act (F3, a copy) and ruinous for a
//! sweep: a `Ctrl+F` across a synced team library would pull the whole thing
//! down, slowly, with no way to tell it was going to happen.
//!
//! So cian learns to see them. [`is_placeholder`] is the one primitive; the
//! bulk readers ask [`skip_read`] before opening anything, and the file panes
//! badge them.
//!
//! Detection is per-platform but costs nothing extra either way — both read a
//! field of the `stat` the listing already performed:
//!
//! * **Windows** — the file attributes carry `RECALL_ON_DATA_ACCESS` (the
//!   Files On-Demand marker) or `RECALL_ON_OPEN` / `OFFLINE` (older HSM).
//! * **macOS** — the File Provider API marks dataless files with `SF_DATALESS`
//!   in `st_flags`, and because that API is shared, this catches iCloud and
//!   Google Drive as well as OneDrive.
//! * **Linux** — no equivalent convention; everything reads as local, which
//!   leaves behaviour exactly as it was.

use std::fs::Metadata;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// `FILE_ATTRIBUTE_OFFLINE`
#[cfg(windows)]
const OFFLINE: u32 = 0x0000_1000;
/// `FILE_ATTRIBUTE_RECALL_ON_OPEN`
#[cfg(windows)]
const RECALL_ON_OPEN: u32 = 0x0004_0000;
/// `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`
#[cfg(windows)]
const RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

/// `SF_DATALESS` — "file is a dataless placeholder" (macOS File Provider).
#[cfg(target_os = "macos")]
const SF_DATALESS: u32 = 0x4000_0000;

/// Would reading this file pull it down from a sync service?
#[cfg(windows)]
pub fn is_placeholder(meta: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    let a = meta.file_attributes();
    a & (RECALL_ON_DATA_ACCESS | RECALL_ON_OPEN | OFFLINE) != 0
}

#[cfg(target_os = "macos")]
pub fn is_placeholder(meta: &Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    meta.st_flags() & SF_DATALESS != 0
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn is_placeholder(_meta: &Metadata) -> bool {
    false
}

/// [`is_placeholder`] for a path, when no `Metadata` is already at hand.
/// Uses `symlink_metadata`, which reads the placeholder's own attributes
/// rather than following it into a hydration.
pub fn placeholder_at(path: &Path) -> bool {
    std::fs::symlink_metadata(path).map(|m| is_placeholder(&m)).unwrap_or(false)
}

/// Whether sweeps are allowed to hydrate placeholders. Off by default.
///
/// A process-wide switch rather than a parameter on every reader: this is a
/// property of the machine and the user's intent for the session, not of any
/// one call, and threading it through grep, count, hash and dedup — each with
/// its own options type — would put the same bool in four places and every
/// test that builds them.
static INCLUDE: AtomicBool = AtomicBool::new(false);

/// Let sweeps read placeholders (and so download them). The toggle menu and
/// `cian.set_option("read_cloud_files", true)` drive this.
pub fn set_include(on: bool) {
    INCLUDE.store(on, Ordering::Relaxed);
}

pub fn include() -> bool {
    INCLUDE.load(Ordering::Relaxed)
}

/// The question every bulk reader asks before opening a file: skip it?
///
/// True only for a placeholder while sweeps are set to leave them alone.
/// Deliberate single-file actions (F3, a copy, a hash of one file the user
/// pointed at) do not call this — hydrating because you asked for that file
/// is the sync client working as intended.
pub fn skip_read(path: &Path) -> bool {
    !include() && placeholder_at(path)
}

/// [`skip_read`] for a reader that already holds the `Metadata`, so the
/// decision costs no second `stat`.
pub fn skip_meta(meta: &Metadata) -> bool {
    !include() && is_placeholder(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ordinary file is never a placeholder, on any platform. (The positive
    /// case needs a real sync client, so it is verified by hand — on macOS,
    /// against iCloud and Google Drive folders, where `SF_DATALESS` shows up
    /// as `flags=0x40000060`.)
    #[test]
    fn a_local_file_is_not_a_placeholder() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("real.txt");
        std::fs::write(&p, b"here").unwrap();
        assert!(!placeholder_at(&p));
        assert!(!skip_read(&p), "a local file is never skipped");
    }

    /// The switch gates the skip, and a missing file is not a placeholder
    /// (it is simply gone — the reader's own error path handles that).
    #[test]
    fn include_switch_gates_skipping() {
        let d = tempfile::tempdir().unwrap();
        let missing = d.path().join("nope.txt");
        assert!(!placeholder_at(&missing));

        assert!(!include(), "sweeps leave placeholders alone by default");
        set_include(true);
        assert!(include());
        assert!(!skip_read(&missing), "nothing is skipped once sweeps may read");
        set_include(false);
    }
}
