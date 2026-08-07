//! Browsing an archive as a folder (`Enter` on a zip/tar): the pane switches
//! to a synthetic listing of the members under a directory inside the
//! archive. Navigation mirrors a real directory — `Enter`/`l` descends, the
//! `..` row (or `h`) climbs, and climbing past the root lands back on the
//! archive file itself. `F3` extracts the member to a temp file and opens the
//! normal viewer on it; copying to the other pane extracts.
//!
//! Member rows are [`cian_core::Entry`]s with a *synthetic* path — the
//! archive's path joined with the member path, which does not exist on disk.
//! That is the same trick the SFTP pane uses, and like there, operations that
//! would touch the path are either mapped to archive equivalents here or
//! refused with a message. This phase is read-only; zip writing is next.

use std::path::{Path, PathBuf};

use cian_core::archive::Member;
use cian_core::Entry;

use crate::{tr, App};

/// Members are listed once per archive and kept while browsing inside it —
/// tar.gz listing decompresses the whole stream, which must not happen per
/// keystroke. Invalidated by mtime, so an archive rebuilt underneath is
/// noticed on the next navigation.
pub(crate) struct ArchiveCache {
    pub path: PathBuf,
    mtime: Option<std::time::SystemTime>,
    pub members: Vec<Member>,
}

/// The rows for directory `sub` (`""` = root) of `members`: the `..` row,
/// then direct child directories (explicit or implied by deeper members),
/// then direct child files. Pure, so the tests can pin the shape down.
pub(crate) fn archive_rows(archive: &Path, members: &[Member], sub: &str) -> Vec<Entry> {
    let mut dirs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut files: Vec<(String, u64)> = Vec::new();
    for m in members {
        let name = m.name.trim_end_matches('/');
        let Some(rest) = name.strip_prefix(sub) else { continue };
        if rest.is_empty() {
            continue; // the directory entry for `sub` itself
        }
        match rest.split_once('/') {
            // Deeper than one level: only its first segment shows, as a dir.
            Some((head, _)) => {
                dirs.insert(head.to_string());
            }
            None => {
                if m.is_dir {
                    dirs.insert(rest.to_string());
                } else {
                    files.push((rest.to_string(), m.size));
                }
            }
        }
    }
    files.sort();
    // The `..` row: its synthetic path is the archive-or-parent target, but
    // navigation intercepts it before anything touches the path.
    let up = Entry::remote("..", archive.display().to_string(), true, 0, true);
    let mut out = vec![up];
    for d in dirs {
        let p = format!("{}/{}{}", archive.display(), sub, d);
        out.push(Entry::remote(d, p, true, 0, false));
    }
    for (f, size) in files {
        let p = format!("{}/{}{}", archive.display(), sub, f);
        out.push(Entry::remote(f, p, false, size, false));
    }
    out
}

/// The member names under `prefix` (the entries a dir row stands for),
/// including the file `prefix` itself when it names a file directly.
pub(crate) fn members_under<'a>(members: &'a [Member], prefix: &str) -> Vec<&'a Member> {
    members
        .iter()
        .filter(|m| {
            let name = m.name.trim_end_matches('/');
            name == prefix.trim_end_matches('/') || name.starts_with(&format!("{}/", prefix.trim_end_matches('/')))
        })
        .collect()
}

impl App {
    /// The cached member list for `archive`, (re)read when missing or stale.
    fn archive_members(&mut self, archive: &Path) -> Result<Vec<Member>, String> {
        let mtime = std::fs::metadata(archive).ok().and_then(|m| m.modified().ok());
        if let Some(c) = &self.archive_cache {
            if c.path == archive && c.mtime == mtime {
                return Ok(c.members.clone());
            }
        }
        let members = cian_core::archive::list(archive).map_err(|e| e.to_string())?;
        self.archive_cache =
            Some(ArchiveCache { path: archive.to_path_buf(), mtime, members: members.clone() });
        Ok(members)
    }

    /// `Enter` on an archive file: browse into it (at `sub`, `""` = root).
    pub(crate) fn enter_archive(&mut self, archive: PathBuf, sub: String) {
        let members = match self.archive_members(&archive) {
            Ok(m) => m,
            Err(e) => {
                self.message = Some(format!("not a readable archive: {e}"));
                return;
            }
        };
        let rows = archive_rows(&archive, &members, &sub);
        if let Some(p) = self.active_pane_mut() {
            p.enter_archive(archive, sub, rows);
        }
    }

    /// Climb one level inside the archive; past the root, leave it and put
    /// the cursor back on the archive file.
    pub(crate) fn archive_go_up(&mut self) {
        let Some((archive, sub)) =
            self.active_pane().and_then(|p| p.archive_view()).map(|(a, s)| (a.to_path_buf(), s.to_string()))
        else {
            return;
        };
        if sub.is_empty() {
            // Leave: back to the real directory, cursor on the archive.
            let dir = archive.parent().map(|p| p.to_path_buf());
            let name = archive.file_name().map(|s| s.to_string_lossy().into_owned());
            if let (Some(dir), Some(p)) = (dir, self.active_pane_mut()) {
                let _ = p.jump_to(dir);
                if let Some(name) = name {
                    if let Some(i) = p.entries.iter().position(|e| e.name == name) {
                        p.cursor = i;
                    }
                }
            }
            return;
        }
        // "a/b/" → "a/"; "a/" → "".
        let parent = sub
            .trim_end_matches('/')
            .rsplit_once('/')
            .map(|(head, _)| format!("{head}/"))
            .unwrap_or_default();
        let child = sub.trim_end_matches('/').rsplit('/').next().map(|s| s.to_string());
        self.enter_archive(archive, parent);
        // Land on the directory we just left, as real navigation does.
        if let (Some(child), Some(p)) = (child, self.active_pane_mut()) {
            if let Some(i) = p.entries.iter().position(|e| e.name == child) {
                p.cursor = i;
            }
        }
    }

    /// `Enter` on a row inside the archive: descend into a directory, or view
    /// a file member (same as F3 — there is no OS association for something
    /// that exists only inside an archive).
    pub(crate) fn archive_activate(&mut self) {
        let Some((archive, sub)) =
            self.active_pane().and_then(|p| p.archive_view()).map(|(a, s)| (a.to_path_buf(), s.to_string()))
        else {
            return;
        };
        let Some(e) = self.active_pane().and_then(|p| p.selected()).cloned() else { return };
        if e.is_parent {
            self.archive_go_up();
            return;
        }
        if e.is_dir {
            self.enter_archive(archive, format!("{}{}/", sub, e.name));
        } else {
            self.archive_view_member(&archive, &format!("{}{}", sub, e.name));
        }
    }

    /// F3 on a member: extract it to a temp file and open the normal viewer —
    /// text, image, office, whatever it turns out to be.
    pub(crate) fn archive_view_member(&mut self, archive: &Path, member: &str) {
        const MAX_VIEW: u64 = 64 * 1024 * 1024;
        let size = self
            .archive_cache
            .as_ref()
            .and_then(|c| c.members.iter().find(|m| m.name.trim_end_matches('/') == member))
            .map(|m| m.size)
            .unwrap_or(0);
        if size > MAX_VIEW {
            self.message = Some(tr(
                self.lang,
                "member too large to view — copy it out instead",
                "大きすぎて閲覧できません — 展開してから開いてください",
            ).into());
            return;
        }
        if cian_core::archive::zip_needs_password(archive) {
            self.message = Some(tr(
                self.lang,
                "encrypted zip — use :unzip to extract with the password",
                "暗号化zip — :unzip でパスワード指定で展開してください",
            ).into());
            return;
        }
        // A per-process temp dir; one subdir per view keeps names collision-free.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dest =
            std::env::temp_dir().join(format!("cian-arcview-{}-{}", std::process::id(), seq));
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut sink = |_: &cian_core::progress::Progress| {};
        let mut ctl = cian_core::progress::Ctl { cancel: &cancel, on_progress: &mut sink };
        // Strip the member's own directory so the temp file is dest/<basename>.
        let strip = member.rsplit_once('/').map(|(h, _)| format!("{h}/")).unwrap_or_default();
        let report = cian_core::archive::extract(
            archive,
            &[member.to_string()],
            &dest,
            None,
            &strip,
            &mut ctl,
        );
        if let Some(err) = report.errors.first() {
            self.message = Some(format!("view member: {err}"));
            return;
        }
        let base = member.rsplit('/').next().unwrap_or(member);
        let path = dest.join(base);
        let title = format!(
            "{}:{}",
            archive.file_name().map(|s| s.to_string_lossy()).unwrap_or_default(),
            member
        );
        self.open_viewer_at(&path, &title, 0);
    }

    /// Copy the marked members (or the cursor's) out to `dest`, extracting
    /// relative to the directory being browsed. Runs on the op worker with
    /// progress, like any transfer.
    pub(crate) fn archive_copy_out(&mut self, dest: PathBuf) {
        let Some((archive, sub)) =
            self.active_pane().and_then(|p| p.archive_view()).map(|(a, s)| (a.to_path_buf(), s.to_string()))
        else {
            return;
        };
        let Some(pane) = self.active_pane() else { return };
        // Selected rows → member prefixes (dirs expand to everything under).
        let arc_prefix = format!("{}/", archive.display());
        let picked: Vec<(String, bool)> = if pane.mark_count() > 0 {
            pane.entries
                .iter()
                .filter(|e| !e.is_parent && pane.marks.contains(&e.path))
                .map(|e| (format!("{}{}", sub, e.name), e.is_dir))
                .collect()
        } else {
            match pane.selected().filter(|e| !e.is_parent) {
                Some(e) => vec![(format!("{}{}", sub, e.name), e.is_dir)],
                None => Vec::new(),
            }
        };
        let _ = arc_prefix;
        if picked.is_empty() {
            self.message = Some(tr(self.lang, "nothing selected", "選択がありません").into());
            return;
        }
        let members: Vec<String> = match self.archive_members(&archive) {
            Ok(all) => picked
                .iter()
                .flat_map(|(prefix, is_dir)| {
                    if *is_dir {
                        members_under(&all, prefix)
                            .into_iter()
                            .map(|m| m.name.clone())
                            .collect::<Vec<_>>()
                    } else {
                        vec![prefix.clone()]
                    }
                })
                .collect(),
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        // Extract relative to the browsed directory, so copying `c/` from
        // inside `a/b/` produces dest/c/…, not dest/a/b/c/….
        self.start_extract_stripped(archive, members, dest, sub);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str, is_dir: bool, size: u64) -> Member {
        Member { name: name.to_string(), is_dir, size, compressed: 0 }
    }

    /// The listing contract: direct children only, implied directories shown,
    /// dirs before files, `..` first.
    #[test]
    fn rows_show_direct_children_with_implied_dirs() {
        let members = vec![
            m("top.txt", false, 3),
            m("a/", true, 0),
            m("a/b.txt", false, 1),
            m("a/c/d.txt", false, 2), // `a/c/` never appears explicitly
            m("z.txt", false, 9),
        ];
        let arc = Path::new("/tmp/x.zip");
        let names: Vec<String> =
            archive_rows(arc, &members, "").iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["..", "a", "top.txt", "z.txt"]);

        let inside: Vec<(String, bool)> = archive_rows(arc, &members, "a/")
            .iter()
            .map(|e| (e.name.clone(), e.is_dir))
            .collect();
        assert_eq!(
            inside,
            vec![
                ("..".to_string(), true),
                ("c".to_string(), true), // implied by a/c/d.txt
                ("b.txt".to_string(), false),
            ]
        );
    }

    #[test]
    fn members_under_expands_a_directory_prefix() {
        let members =
            vec![m("a/", true, 0), m("a/b.txt", false, 1), m("a/c/d.txt", false, 2), m("ab.txt", false, 1)];
        let names: Vec<&str> = members_under(&members, "a").iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["a/", "a/b.txt", "a/c/d.txt"], "ab.txt is not under a/");
    }
}
