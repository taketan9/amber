use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::{Context, Result};

pub mod archive;
pub mod attrs;
pub mod count;
pub mod dedup;
pub mod diff;
pub mod dirdiff;
pub mod elevate;
pub mod git;
pub mod highlight;
pub mod image;
pub mod inspect;
pub mod log;
pub mod office;
pub mod ops;
pub mod progress;
pub mod search;
pub mod viewer;

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    /// Size in bytes. Meaningless for directories, which report `0`.
    pub len: u64,
    /// Last modification time, if the filesystem reports one.
    pub modified: Option<SystemTime>,
    /// True for the synthetic `..` row that steps up to the parent directory.
    /// It is navigable but never a target: it cannot be marked, copied, moved,
    /// renamed or deleted, and file operations skip it.
    pub is_parent: bool,
}

impl Entry {
    fn from_dir_entry(de: fs::DirEntry) -> Result<Self> {
        let path = de.path();
        let name = de
            .file_name()
            .into_string()
            .map_err(|raw| anyhow::anyhow!("non-utf8 filename: {:?}", raw))?;
        let is_dir = de.file_type()?.is_dir();
        // Metadata can fail on broken symlinks and races; the entry is still
        // worth listing, so fall back to unknown size/time rather than drop it.
        let meta = de.metadata().ok();
        let len = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified = meta.as_ref().and_then(|m| m.modified().ok());
        Ok(Self { name, path, is_dir, len, modified, is_parent: false })
    }

    /// The synthetic `..` entry pointing at `parent`.
    fn parent_row(parent: PathBuf) -> Self {
        Self {
            name: "..".to_string(),
            path: parent,
            is_dir: true,
            len: 0,
            modified: None,
            is_parent: true,
        }
    }
}

/// Format a byte count the way a file manager should: short, aligned, and
/// never more than one decimal place.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["B", "K", "M", "G", "T", "P", "E"];
    if bytes < 1024 {
        return format!("{}{}", bytes, UNITS[0]);
    }
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if v < 10.0 {
        format!("{:.1}{}", v, UNITS[unit])
    } else {
        format!("{:.0}{}", v, UNITS[unit])
    }
}

/// Format a timestamp as local `YYYY-MM-DD HH:MM`.
///
/// Uses chrono's `Local` rather than a hand-rolled offset: getting the zone
/// right means DST rules and per-platform system calls, and cian is built and
/// shipped for Windows from CI where that code could not be tested locally.
/// Current local time as `YYYYMMDD_HHMMSS`, for building log file names.
pub fn timestamp_compact() -> String {
    chrono::Local::now().format("%Y%m%d_%H%M%S").to_string()
}

pub fn format_time(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

/// What the listing is ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Modified,
    Extension,
}

impl SortKey {
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Size => "size",
            SortKey::Modified => "date",
            SortKey::Extension => "ext",
        }
    }

    /// The order the picker offers, so the UI and the core agree.
    pub const ALL: [SortKey; 4] =
        [SortKey::Name, SortKey::Size, SortKey::Modified, SortKey::Extension];
}

/// How a pane's listing is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    /// Largest / newest / last-alphabetically first.
    pub reverse: bool,
}

impl Default for Sort {
    fn default() -> Self {
        Self { key: SortKey::Name, reverse: false }
    }
}

#[derive(Debug, Clone)]
pub struct Pane {
    pub cwd: PathBuf,
    /// The visible list: [`Pane::all_entries`] narrowed by [`Pane::filter`].
    /// Everything else (cursor, marks, file operations, rendering) works off
    /// this, so filtering automatically scopes them all to what is on screen.
    pub entries: Vec<Entry>,
    /// Every entry in `cwd`, before filtering.
    pub all_entries: Vec<Entry>,
    /// Case-insensitive substring that narrows the listing. Empty shows all.
    pub filter: String,
    /// Show entries whose name starts with a dot. Defaults to true, which is
    /// what cian has always done; most file managers hide them, so it is a
    /// toggle rather than a fixed choice.
    pub show_hidden: bool,
    /// Ordering of the listing.
    pub sort: Sort,
    pub cursor: usize,
    /// Marked entries keyed by full path (survives reload).
    pub marks: HashSet<PathBuf>,
    /// Recently visited paths for this pane (most recent first, deduped, capped).
    pub history: Vec<PathBuf>,
    /// `cwd`'s modification time as of the last read, used to notice changes
    /// made by anything other than cian.
    stamp: Option<SystemTime>,
}

const HISTORY_CAP: usize = 30;

impl Pane {
    pub fn new(cwd: impl Into<PathBuf>) -> Result<Self> {
        // `dunce` rather than `Path::canonicalize`, which on Windows returns an
        // extended-length path (`\\?\C:\...`). That prefix is a filesystem
        // convention the Windows *Shell* does not accept, so it would show up
        // in pane titles and, worse, break trashing a file — every entry path
        // is built by joining onto this one.
        let cwd = dunce::canonicalize(cwd.into()).context("invalid initial path")?;
        let mut pane = Self {
            cwd,
            entries: Vec::new(),
            all_entries: Vec::new(),
            filter: String::new(),
            show_hidden: true,
            sort: Sort::default(),
            cursor: 0,
            marks: HashSet::new(),
            history: Vec::new(),
            stamp: None,
        };
        pane.reload()?;
        pane.cursor_to_first_real();
        Ok(pane)
    }

    fn push_history(&mut self, path: PathBuf) {
        self.history.retain(|p| p != &path);
        self.history.insert(0, path);
        if self.history.len() > HISTORY_CAP {
            self.history.truncate(HISTORY_CAP);
        }
    }

    /// Whether `cwd` has changed since it was last read.
    ///
    /// A directory's own mtime moves when an entry is added, removed or
    /// renamed, which covers "a file appeared while I was looking at this" —
    /// the case where a stale listing actively misleads. Checking one stat is
    /// cheap enough to do on a timer; re-reading the whole directory is not.
    pub fn is_stale(&self) -> bool {
        let now = fs::metadata(&self.cwd).ok().and_then(|m| m.modified().ok());
        match (now, self.stamp) {
            (Some(a), Some(b)) => a != b,
            // No timestamp either time: nothing to compare, assume unchanged
            // rather than reloading forever.
            (None, None) => false,
            _ => true,
        }
    }

    pub fn reload(&mut self) -> Result<()> {
        self.stamp = fs::metadata(&self.cwd).ok().and_then(|m| m.modified().ok());
        let entries: Vec<Entry> = fs::read_dir(&self.cwd)
            .with_context(|| format!("read_dir failed: {}", self.cwd.display()))?
            .filter_map(|res| res.ok())
            .filter_map(|de| Entry::from_dir_entry(de).ok())
            .collect();
        self.all_entries = entries;
        self.apply_sort();
        self.apply_filter();
        // Forget marks whose path no longer exists in this directory. This
        // checks the unfiltered list on purpose: narrowing the view must not
        // silently drop marks on entries the filter is hiding.
        let live: HashSet<PathBuf> = self.all_entries.iter().map(|e| e.path.clone()).collect();
        self.marks.retain(|p| live.contains(p));
        Ok(())
    }

    /// Order `all_entries` according to `sort`.
    ///
    /// Directories always come first regardless of key or direction — that is
    /// what navigation depends on, and burying folders among files to satisfy
    /// a size sort would make the pane much harder to move around in.
    fn apply_sort(&mut self) {
        let sort = self.sort;
        self.all_entries.sort_by(|a, b| {
            match (a.is_dir, b.is_dir) {
                (true, false) => return std::cmp::Ordering::Less,
                (false, true) => return std::cmp::Ordering::Greater,
                _ => {}
            }
            let by_name = |x: &Entry, y: &Entry| x.name.to_lowercase().cmp(&y.name.to_lowercase());
            let ord = match sort.key {
                SortKey::Name => by_name(a, b),
                // Ties fall back to name so the order is stable and predictable
                // rather than filesystem-dependent.
                SortKey::Size => a.len.cmp(&b.len).then_with(|| by_name(a, b)),
                SortKey::Modified => a.modified.cmp(&b.modified).then_with(|| by_name(a, b)),
                SortKey::Extension => {
                    let ext = |e: &Entry| {
                        std::path::Path::new(&e.name)
                            .extension()
                            .and_then(|x| x.to_str())
                            .unwrap_or("")
                            .to_lowercase()
                    };
                    ext(a).cmp(&ext(b)).then_with(|| by_name(a, b))
                }
            };
            if sort.reverse {
                ord.reverse()
            } else {
                ord
            }
        });
    }

    /// Change the ordering and re-apply it, keeping the filter intact.
    pub fn set_sort(&mut self, sort: Sort) {
        self.sort = sort;
        self.apply_sort();
        self.apply_filter();
    }

    /// Rebuild `entries` from `all_entries` according to `filter` and
    /// `show_hidden`.
    fn apply_filter(&mut self) {
        let needle = self.filter.to_lowercase();
        let show_hidden = self.show_hidden;
        let mut entries: Vec<Entry> = self
            .all_entries
            .iter()
            .filter(|e| show_hidden || !e.name.starts_with('.'))
            .filter(|e| needle.is_empty() || e.name.to_lowercase().contains(&needle))
            .cloned()
            .collect();
        // A `..` row at the very top, so stepping up a level is a visible,
        // clickable target (as in classic file managers). Not at the filesystem
        // root, which has no parent. It always shows, even under a filter —
        // hiding the way out would be surprising — but never when it would not
        // match: it is navigation, not a listed file.
        if let Some(parent) = self.cwd.parent().map(|p| p.to_path_buf()) {
            entries.insert(0, Entry::parent_row(parent));
        }
        self.entries = entries;
        if self.cursor >= self.entries.len() {
            self.cursor = self.entries.len().saturating_sub(1);
        }
    }

    /// Narrow the listing. Passing an empty string shows everything again.
    pub fn set_filter(&mut self, filter: impl Into<String>) {
        self.filter = filter.into();
        self.apply_filter();
    }

    /// Show or hide dotfiles. Kept across directory changes, unlike the
    /// filter: it is a preference about how you want to look at things, not a
    /// query about one folder.
    pub fn set_show_hidden(&mut self, show: bool) {
        if self.show_hidden != show {
            self.show_hidden = show;
            self.apply_filter();
        }
    }

    /// Drop the filter. Called whenever the pane changes directory, since a
    /// filter left over from the previous folder would hide files the user
    /// has no reason to expect are missing.
    pub fn clear_filter(&mut self) {
        if !self.filter.is_empty() {
            self.filter.clear();
            self.apply_filter();
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let len = self.entries.len() as isize;
        let next = (self.cursor as isize + delta).clamp(0, len - 1);
        self.cursor = next as usize;
    }

    pub fn enter_selected(&mut self) -> Result<()> {
        if let Some(e) = self.entries.get(self.cursor).cloned() {
            if e.is_dir {
                let prev = self.cwd.clone();
                self.push_history(prev);
                self.cwd = e.path;
                self.marks.clear();
                self.filter.clear();
                self.reload()?;
                self.cursor_to_first_real();
            }
        }
        Ok(())
    }

    pub fn go_parent(&mut self) -> Result<()> {
        let parent_owned = self.cwd.parent().map(|p| p.to_path_buf());
        if let Some(parent) = parent_owned {
            let prev = self.cwd.clone();
            self.push_history(prev);
            self.cwd = parent;
            self.marks.clear();
            self.filter.clear();
            self.reload()?;
            self.cursor_to_first_real();
        }
        Ok(())
    }

    pub fn jump_to(&mut self, path: PathBuf) -> Result<()> {
        let prev = self.cwd.clone();
        self.push_history(prev);
        self.cwd = path;
        self.marks.clear();
        self.filter.clear();
        self.reload()?;
        self.cursor_to_first_real();
        Ok(())
    }

    /// Park the cursor on the first real entry, skipping the `..` row, so a
    /// freshly opened directory does not start with the cursor on "up a level".
    fn cursor_to_first_real(&mut self) {
        self.cursor = self.entries.iter().position(|e| !e.is_parent).unwrap_or(0);
    }

    pub fn selected(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    pub fn toggle_mark_at(&mut self, idx: usize) {
        if let Some(e) = self.entries.get(idx) {
            if e.is_parent {
                return; // `..` is navigation, never a selection
            }
            let p = e.path.clone();
            if !self.marks.remove(&p) {
                self.marks.insert(p);
            }
        }
    }

    pub fn set_mark_at(&mut self, idx: usize) {
        if let Some(e) = self.entries.get(idx) {
            if e.is_parent {
                return;
            }
            self.marks.insert(e.path.clone());
        }
    }

    pub fn is_marked(&self, idx: usize) -> bool {
        self.entries
            .get(idx)
            .map(|e| self.marks.contains(&e.path))
            .unwrap_or(false)
    }

    pub fn clear_marks(&mut self) {
        self.marks.clear();
    }

    pub fn mark_count(&self) -> usize {
        self.marks.len()
    }

    /// Return marked paths, or if none marked, the cursor's path as a fallback.
    /// The synthetic `..` row is never a target — acting on the cursor while it
    /// sits on `..` (delete, copy, rename, …) yields nothing rather than
    /// operating on the parent directory.
    pub fn target_paths(&self) -> Vec<PathBuf> {
        if !self.marks.is_empty() {
            let mut v: Vec<PathBuf> = self.marks.iter().cloned().collect();
            v.sort();
            v
        } else if let Some(e) = self.selected().filter(|e| !e.is_parent) {
            vec![e.path.clone()]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pane over a temp dir containing `names` (all plain files).
    fn pane_with(names: &[&str]) -> (tempfile::TempDir, Pane) {
        let dir = tempfile::tempdir().unwrap();
        for n in names {
            fs::write(dir.path().join(n), b"").unwrap();
        }
        let pane = Pane::new(dir.path()).unwrap();
        (dir, pane)
    }

    /// Paths must stay in the form the Windows Shell understands.
    ///
    /// `Path::canonicalize` returns `\\?\C:\...` on Windows. That prefix is a
    /// filesystem convention the Shell rejects, so it would show up in pane
    /// titles and make trashing fail — and since every entry path is joined
    /// onto the pane's cwd, one bad root poisons all of them. This assertion
    /// only means anything on the Windows CI runner, which is the point.
    #[test]
    fn pane_paths_avoid_the_extended_length_prefix() {
        let (_d, pane) = pane_with(&["a.txt"]);
        assert!(pane.cwd.is_absolute(), "cwd should still be absolute");
        assert!(!pane.entries.is_empty());

        #[cfg(windows)]
        {
            assert!(
                !pane.cwd.to_string_lossy().starts_with(r"\\?\"),
                "extended-length prefix leaked into cwd: {:?}",
                pane.cwd
            );
            for e in &pane.entries {
                assert!(
                    !e.path.to_string_lossy().starts_with(r"\\?\"),
                    "extended-length prefix leaked into an entry: {:?}",
                    e.path
                );
            }
        }
    }

    #[test]
    fn sorting_by_size_orders_files_and_keeps_directories_first() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("zzz_folder")).unwrap();
        fs::write(dir.path().join("big.bin"), vec![0u8; 3000]).unwrap();
        fs::write(dir.path().join("mid.bin"), vec![0u8; 200]).unwrap();
        fs::write(dir.path().join("small.bin"), b"x").unwrap();
        let mut pane = Pane::new(dir.path()).unwrap();

        pane.set_sort(Sort { key: SortKey::Size, reverse: false });
        assert_eq!(
            names(&pane),
            vec!["zzz_folder", "small.bin", "mid.bin", "big.bin"],
            "directories stay on top even when sorting by size"
        );

        pane.set_sort(Sort { key: SortKey::Size, reverse: true });
        assert_eq!(names(&pane), vec!["zzz_folder", "big.bin", "mid.bin", "small.bin"]);
    }

    #[test]
    fn sorting_by_extension_then_name() {
        let (_d, mut pane) = pane_with(&["b.rs", "a.rs", "c.md"]);
        pane.set_sort(Sort { key: SortKey::Extension, reverse: false });
        // .md before .rs, and within .rs alphabetically.
        assert_eq!(names(&pane), vec!["c.md", "a.rs", "b.rs"]);
    }

    #[test]
    fn sorting_survives_reload_and_composes_with_the_filter() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("big.log"), vec![0u8; 3000]).unwrap();
        fs::write(dir.path().join("small.log"), b"x").unwrap();
        fs::write(dir.path().join("other.txt"), vec![0u8; 900]).unwrap();
        let mut pane = Pane::new(dir.path()).unwrap();

        pane.set_sort(Sort { key: SortKey::Size, reverse: true });
        pane.set_filter("log");
        assert_eq!(names(&pane), vec!["big.log", "small.log"], "sort applies within the filter");

        pane.reload().unwrap();
        assert_eq!(pane.sort.key, SortKey::Size, "reload must not reset the order");
        assert_eq!(names(&pane), vec!["big.log", "small.log"]);
    }

    #[test]
    fn default_order_is_name_ascending_with_directories_first() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("zdir")).unwrap();
        fs::write(dir.path().join("a.txt"), b"").unwrap();
        let pane = Pane::new(dir.path()).unwrap();
        assert_eq!(pane.sort, Sort::default());
        assert_eq!(names(&pane), vec!["zdir", "a.txt"]);
    }

    /// cian only ever reloaded after its own actions, so a file created by
    /// anything else never appeared.
    #[test]
    fn a_pane_notices_its_directory_changing_underneath_it() {
        let (dir, mut pane) = pane_with(&["a.txt"]);
        assert!(!pane.is_stale(), "nothing has happened yet");

        // Directory mtimes have coarse resolution on some filesystems; make
        // sure the change lands in a later tick.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(dir.path().join("appeared.txt"), b"x").unwrap();
        assert!(pane.is_stale(), "a new entry should show as stale");

        pane.reload().unwrap();
        assert!(!pane.is_stale(), "reloading clears it");
        assert!(names(&pane).contains(&"appeared.txt".to_string()));

        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::remove_file(dir.path().join("appeared.txt")).unwrap();
        assert!(pane.is_stale(), "a removed entry counts too");
        pane.reload().unwrap();
        assert!(!names(&pane).contains(&"appeared.txt".to_string()));
    }

    #[test]
    fn hidden_entries_can_be_toggled_and_the_choice_outlives_a_reload() {
        let (_d, mut pane) = pane_with(&["a.txt", ".config", ".env"]);
        assert!(pane.show_hidden, "cian has always shown them; that is the default");
        assert_eq!(names(&pane).len(), 3);

        pane.set_show_hidden(false);
        assert_eq!(names(&pane), vec!["a.txt"]);

        // A preference about how to look at things, not a query about one
        // folder, so it survives a reload — unlike the filter.
        pane.reload().unwrap();
        assert_eq!(names(&pane), vec!["a.txt"]);

        pane.set_show_hidden(true);
        assert_eq!(names(&pane).len(), 3);
    }

    #[test]
    fn hiding_composes_with_the_filter() {
        let (_d, mut pane) = pane_with(&["notes.txt", ".notes.swp", "other.md"]);
        pane.set_show_hidden(false);
        pane.set_filter("notes");
        assert_eq!(names(&pane), vec!["notes.txt"], "the dotfile stays hidden");
    }

    /// The listed names, excluding the synthetic `..` navigation row (a temp
    /// dir always has a parent, so it is always present).
    fn names(pane: &Pane) -> Vec<String> {
        pane.entries.iter().filter(|e| !e.is_parent).map(|e| e.name.clone()).collect()
    }

    /// Count of real entries (without the `..` row).
    fn real_len(pane: &Pane) -> usize {
        pane.entries.iter().filter(|e| !e.is_parent).count()
    }

    #[test]
    fn filter_narrows_and_is_case_insensitive() {
        let (_d, mut pane) = pane_with(&["Alpha.rs", "beta.rs", "gamma.txt"]);
        assert_eq!(real_len(&pane), 3);

        pane.set_filter("RS");
        assert_eq!(names(&pane), vec!["Alpha.rs", "beta.rs"]);

        pane.set_filter("alp");
        assert_eq!(names(&pane), vec!["Alpha.rs"]);
    }

    #[test]
    fn clearing_filter_restores_every_entry() {
        let (_d, mut pane) = pane_with(&["a.txt", "b.txt", "c.md"]);
        pane.set_filter("md");
        assert_eq!(real_len(&pane), 1);
        pane.clear_filter();
        assert_eq!(real_len(&pane), 3);
    }

    #[test]
    fn filter_clamps_cursor_into_range() {
        let (_d, mut pane) = pane_with(&["a.txt", "b.txt", "c.txt"]);
        pane.cursor = 3;
        pane.set_filter("a.txt");
        // Only `..` and the one match survive, so the cursor must not dangle
        // past the end of the list.
        assert_eq!(real_len(&pane), 1);
        assert_eq!(pane.entries.len(), 2, "`..` plus the match");
        assert!(pane.cursor < pane.entries.len());
    }

    #[test]
    fn no_match_yields_empty_list_and_zero_cursor() {
        let (_d, mut pane) = pane_with(&["a.txt"]);
        pane.set_filter("zzz");
        // No real matches, but `..` stays as the way out.
        assert_eq!(real_len(&pane), 0);
        assert_eq!(pane.cursor, 0);
        assert!(pane.selected().map(|e| e.is_parent).unwrap_or(false), "only `..` remains");
        // `..` is never a target, so acting on the cursor yields nothing.
        assert!(pane.target_paths().is_empty());
    }

    /// Regression guard: reload() prunes marks against the *unfiltered* list,
    /// so a mark on a hidden entry must survive a reload while filtered.
    #[test]
    fn reload_while_filtered_keeps_marks_on_hidden_entries() {
        let (_d, mut pane) = pane_with(&["keep.txt", "hidden.md"]);
        let hidden = pane
            .all_entries
            .iter()
            .find(|e| e.name == "hidden.md")
            .unwrap()
            .path
            .clone();
        pane.marks.insert(hidden.clone());

        pane.set_filter("keep");
        assert_eq!(names(&pane), vec!["keep.txt"]);

        pane.reload().unwrap();
        assert!(pane.marks.contains(&hidden), "mark on a filtered-out entry was dropped");
    }

    #[test]
    fn filter_survives_reload_but_not_directory_change() {
        let (dir, mut pane) = pane_with(&["a.txt", "b.md"]);
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub").join("inner.txt"), b"").unwrap();

        pane.set_filter("md");
        pane.reload().unwrap();
        assert_eq!(pane.filter, "md", "reload must not drop the filter");

        pane.jump_to(dir.path().join("sub")).unwrap();
        assert_eq!(pane.filter, "", "changing directory must clear the filter");
        assert_eq!(names(&pane), vec!["inner.txt"]);
    }

    #[test]
    fn target_paths_prefers_marks_over_cursor() {
        let (_d, mut pane) = pane_with(&["a.txt", "b.txt"]);
        // Move off the `..` row (index 0) onto a real entry first.
        pane.cursor = 1;
        assert_eq!(pane.target_paths().len(), 1, "falls back to the cursor");
        // set_mark_at skips `..`, so marking indices 1 and 2 marks both files.
        pane.set_mark_at(1);
        pane.set_mark_at(2);
        assert_eq!(pane.target_paths().len(), 2);
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn human_size_scales_and_stays_short() {
        assert_eq!(human_size(0), "0B");
        assert_eq!(human_size(512), "512B");
        assert_eq!(human_size(1024), "1.0K");
        assert_eq!(human_size(1536), "1.5K");
        assert_eq!(human_size(10 * 1024), "10K");
        assert_eq!(human_size(1024 * 1024), "1.0M");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0G");
        // Never wider than 5 columns, so the column can be fixed-width.
        for n in [0u64, 1, 999, 1023, 1024, u64::MAX] {
            assert!(human_size(n).len() <= 5, "{} -> {}", n, human_size(n));
        }
    }

    /// The bug this replaced: timestamps rendered in UTC instead of local
    /// time. 2021-01-01 00:00 UTC is 09:00 the same day in JST, so a UTC
    /// implementation shows the wrong hour (and, near midnight, wrong date).
    #[test]
    fn format_time_uses_the_local_zone_not_utc() {
        let t = UNIX_EPOCH + Duration::from_secs(1_609_459_200); // 2021-01-01 00:00 UTC
        let local = chrono::DateTime::<chrono::Local>::from(t);
        let offset_secs = local.offset().local_minus_utc() as i64;

        let s = format_time(t);
        // Derive what local time *should* be from the offset the OS reports,
        // so this passes in any zone the test happens to run in.
        let expect = chrono::DateTime::<chrono::Utc>::from(t) + chrono::Duration::seconds(offset_secs);
        assert_eq!(s, expect.format("%Y-%m-%d %H:%M").to_string());

        // And specifically: in a +09:00 zone this instant must read 09:00.
        if offset_secs == 9 * 3600 {
            assert_eq!(s, "2021-01-01 09:00", "JST should be UTC+9");
        }
    }

    #[test]
    fn format_time_renders_a_sortable_stamp() {
        // 2021-01-01 00:00:00 UTC
        let t = UNIX_EPOCH + Duration::from_secs(1_609_459_200);
        let s = format_time(t);
        assert_eq!(s.len(), 16, "fixed width for column alignment: {:?}", s);
        assert!(s.starts_with("202"), "{}", s);
        // Shape must be YYYY-MM-DD HH:MM regardless of the machine's zone.
        let bytes = s.as_bytes();
        assert_eq!(bytes[4], b'-');
        assert_eq!(bytes[7], b'-');
        assert_eq!(bytes[10], b' ');
        assert_eq!(bytes[13], b':');
    }
}
