//! SSH / SFTP-SCP actions on the App: matching configured hosts, the host and
//! user pickers, kicking off a transfer, connecting a shell, and watching for
//! the password prompt. Split out of lib.rs as an `impl App` block.
use super::*;

impl App {
    /// Hosts matching the picker's current filter, as `(index, host)`.
    pub(crate) fn ssh_matches(&self, filter: &str) -> Vec<(usize, &cian_lua::SshHost)> {
        let needle = filter.to_lowercase();
        self.config
            .ssh_hosts
            .iter()
            .enumerate()
            .filter(|(_, h)| {
                needle.is_empty()
                    || h.name.to_lowercase().contains(&needle)
                    || h.host.to_lowercase().contains(&needle)
            })
            .collect()
    }

    pub(crate) fn start_ssh(&mut self) {
        if self.config.ssh_hosts.is_empty() {
            self.popup = Popup::Notice {
                lines: vec![
                    "No SSH hosts configured.".to_string(),
                    String::new(),
                    "Declare them in init.lua:".to_string(),
                    String::new(),
                    "  cian.ssh({".to_string(),
                    "    users = { \"root\", \"deploy\" },".to_string(),
                    "    hosts = {".to_string(),
                    "      { name = \"web1\", host = \"10.0.1.11\" },".to_string(),
                    "    },".to_string(),
                    "  })".to_string(),
                ],
            };
            return;
        }
        self.popup = Popup::SshHosts { cursor: 0, filter: String::new() };
    }

    /// Begin an SFTP transfer: capture the local side, then reuse the SSH
    /// host/user picker to choose the server. `ssh_pick` routes back here once
    /// a user is chosen because [`App::scp_dir`] is set.
    pub(crate) fn start_scp(&mut self, dir: ScpDir) {
        if self.config.ssh_hosts.is_empty() {
            self.start_ssh(); // shows the "configure a host" notice
            return;
        }
        // Works from the shell too, acting on the last-focused file pane.
        let pane = self.effective_file_pane();
        let (locals, local_dir) = match dir {
            ScpDir::Upload => {
                let files: Vec<PathBuf> =
                    pane.target_paths().into_iter().filter(|p| p.is_file()).collect();
                if files.is_empty() {
                    self.message = Some("select a file to upload".into());
                    return;
                }
                (files, PathBuf::new())
            }
            ScpDir::Download => (Vec::new(), pane.cwd.clone()),
        };
        self.scp_dir = Some((dir, locals, local_dir));
        // From the shell, if it is logged into a configured host we can
        // authenticate, go straight to that server; otherwise show the picker.
        if self.focused == FocusedPane::Shell {
            if let Some((idx, user)) = self.connected_shell_host() {
                self.scp_after_pick(idx, &user);
                return;
            }
        }
        self.popup = Popup::SshHosts { cursor: 0, filter: String::new() };
    }

    /// The configured host+user the active shell is logged into, if its title is
    /// `user@host` for a host we have a usable (password-bearing) login for.
    fn connected_shell_host(&self) -> Option<(usize, String)> {
        let title = self.shell.active_title()?;
        let user = title.split('@').next()?.trim();
        if user.is_empty() {
            return None;
        }
        let host = host_from_title(&title)?;
        let idx = self.config.ssh_hosts.iter().position(|h| {
            (h.host == host || h.name == host)
                && h.users.iter().any(|u| u.name == user && u.has_secret())
        })?;
        Some((idx, user.to_string()))
    }

    /// After a host+user is picked for a transfer, resolve the connection and
    /// ask for the remote path.
    pub(crate) fn scp_after_pick(&mut self, host_idx: usize, user: &str) {
        let Some((dir, locals, local_dir)) = self.scp_dir.take() else { return };
        let Some(h) = self.config.ssh_hosts.get(host_idx) else { return };
        let Some(u) = h.users.iter().find(|u| u.name == user) else { return };
        let Some(password) = u.secret() else {
            self.message = Some(format!(
                "no password set for {}@{} — a transfer needs one in init.lua",
                u.name, h.name
            ));
            return;
        };
        let target = cian_scp::Target {
            host: h.host.clone(),
            port: h.port.unwrap_or(22),
            user: u.name.clone(),
            password,
        };
        let label = format!("{}@{}", u.name, h.name);
        match dir {
            ScpDir::Upload => {
                // Upload still asks for the destination folder as text.
                self.scp_pending = Some(ScpPending { target, label, dir, locals, local_dir });
                self.popup = text_input(
                    "SFTP upload — remote folder",
                    "remote directory to upload into:",
                    String::new(),
                    InputKind::ScpRemote,
                );
            }
            ScpDir::Download => {
                // Download opens a remote browser: navigate, mark files, then
                // pick where they land locally.
                self.scp_target = Some((target, label.clone()));
                self.open_remote_browser(label, ".");
            }
        }
    }

    /// Open the remote file browser at `cwd` and kick off its listing.
    pub(crate) fn open_remote_browser(&mut self, label: String, cwd: &str) {
        self.popup = Popup::RemoteBrowser {
            label,
            cwd: cwd.to_string(),
            entries: Vec::new(),
            cursor: 0,
            scroll: 0,
            marked: std::collections::BTreeSet::new(),
            loading: true,
        };
        self.remote_ls_spawn(cwd.to_string());
    }

    /// List remote directory `path` on a worker thread; the result lands in
    /// [`App::poll_remote_ls`].
    fn remote_ls_spawn(&mut self, path: String) {
        let Some((target, _)) = self.scp_target.clone() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = cian_scp::list_dir(&target, &path).map(|e| (path, e)).map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
        self.remote_ls = Some(rx);
    }

    /// Install a finished remote listing into the open browser. Returns true if
    /// anything changed (so the caller repaints).
    pub(crate) fn poll_remote_ls(&mut self) -> bool {
        let Some(rx) = &self.remote_ls else { return false };
        match rx.try_recv() {
            Ok(result) => {
                self.remote_ls = None;
                match result {
                    Ok((cwd_new, entries)) => {
                        if let Popup::RemoteBrowser { cwd, entries: es, cursor, scroll, loading, marked, .. } =
                            &mut self.popup
                        {
                            *cwd = cwd_new;
                            *es = entries;
                            *cursor = 0;
                            *scroll = 0;
                            *loading = false;
                            marked.clear();
                        }
                    }
                    Err(e) => {
                        self.popup = Popup::None;
                        self.scp_target = None;
                        self.message = Some(format!("remote listing failed: {}", e));
                    }
                }
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.remote_ls = None;
                true
            }
        }
    }

    /// Enter the highlighted remote entry: descend into a directory, or mark a
    /// file and move on (Enter on a file selects it for download).
    pub(crate) fn remote_browser_enter(&mut self) {
        let (dir_to, is_dir_name) = {
            let Popup::RemoteBrowser { cwd, entries, cursor, .. } = &self.popup else { return };
            let Some(e) = entries.get(*cursor) else { return };
            if e.is_dir {
                (Some(join_remote(cwd, &e.name)), None)
            } else {
                (None, Some(e.name.clone()))
            }
        };
        if let Some(path) = dir_to {
            if let Popup::RemoteBrowser { loading, .. } = &mut self.popup {
                *loading = true;
            }
            self.remote_ls_spawn(path);
        } else if let Some(name) = is_dir_name {
            if let Popup::RemoteBrowser { marked, cursor, entries, .. } = &mut self.popup {
                if !marked.insert(name.clone()) {
                    marked.remove(&name);
                }
                *cursor = (*cursor + 1).min(entries.len().saturating_sub(1));
            }
        }
    }

    /// Go to the parent of the current remote directory.
    pub(crate) fn remote_browser_parent(&mut self) {
        let parent = if let Popup::RemoteBrowser { cwd, .. } = &self.popup {
            parent_remote(cwd)
        } else {
            return;
        };
        if let Popup::RemoteBrowser { loading, .. } = &mut self.popup {
            *loading = true;
        }
        self.remote_ls_spawn(parent);
    }

    /// Toggle the mark on the highlighted file (directories can't be marked).
    pub(crate) fn remote_browser_mark(&mut self) {
        if let Popup::RemoteBrowser { entries, cursor, marked, .. } = &mut self.popup {
            if let Some(e) = entries.get(*cursor) {
                if !e.is_dir && !marked.insert(e.name.clone()) {
                    marked.remove(&e.name);
                }
            }
            *cursor = (*cursor + 1).min(entries.len().saturating_sub(1));
        }
    }

    /// Confirm the remote selection (marked files, else the file under the
    /// cursor) and move on to choose the local destination.
    pub(crate) fn remote_browser_download(&mut self) {
        let files: Vec<String> = if let Popup::RemoteBrowser { cwd, entries, cursor, marked, .. } = &self.popup {
            if !marked.is_empty() {
                marked.iter().map(|n| join_remote(cwd, n)).collect()
            } else {
                match entries.get(*cursor).filter(|e| !e.is_dir) {
                    Some(e) => vec![join_remote(cwd, &e.name)],
                    None => Vec::new(),
                }
            }
        } else {
            return;
        };
        if files.is_empty() {
            self.message = Some("mark a file (Space) or put the cursor on one".into());
            return;
        }
        self.popup = Popup::LocalDest { files, cursor: 0 };
    }

    /// The four local-destination choices, in order, as (label, resolved dir).
    /// The last (`None` dir) means "type a path".
    pub(crate) fn local_dest_options(&self) -> Vec<(String, Option<PathBuf>)> {
        let desktop = dirs_desktop();
        vec![
            ("Left pane".into(), Some(self.left.active_ref().cwd.clone())),
            ("Right pane".into(), Some(self.right.active_ref().cwd.clone())),
            ("Desktop".into(), desktop),
            ("Type a path…".into(), None),
        ]
    }

    /// Act on the chosen local destination: download into a resolved dir, or
    /// prompt for a typed path.
    pub(crate) fn local_dest_pick(&mut self, cursor: usize) {
        let files = if let Popup::LocalDest { files, .. } = &self.popup { files.clone() } else { return };
        let opts = self.local_dest_options();
        let Some((_, dir)) = opts.get(cursor) else { return };
        match dir {
            Some(dir) => {
                // L / R / Desktop: on to the chmod step (local, Unix only).
                let dir = dir.clone();
                self.prompt_download_chmod(files, dir);
            }
            None => {
                self.popup = text_input(
                    "download to",
                    "local directory:",
                    self.active_pane().map(|p| p.cwd.display().to_string()).unwrap_or_default(),
                    InputKind::LocalDestPath { files },
                );
            }
        }
    }

    /// Ask for the mode to apply to downloaded files (blank keeps them as-is;
    /// on Windows the mode is a no-op).
    pub(crate) fn prompt_download_chmod(&mut self, files: Vec<String>, dir: PathBuf) {
        self.popup = text_input(
            "download — chmod",
            "mode for downloaded files (octal, e.g. 644; blank = keep):",
            String::new(),
            InputKind::DownloadChmod { files, dir },
        );
    }

    /// Download `files` (remote paths) into `local_dir` on a worker thread, then
    /// apply `mode` to each (Unix; a no-op elsewhere).
    pub(crate) fn start_remote_download(&mut self, files: Vec<String>, local_dir: PathBuf, mode: Option<u32>) {
        let Some((target, label)) = self.scp_target.take() else { return };
        self.popup = Popup::None;
        if let Err(e) = std::fs::create_dir_all(&local_dir) {
            self.message = Some(format!("cannot create {}: {}", local_dir.display(), e));
            return;
        }
        self.message = Some(format!("downloading {} file(s) from {} …", files.len(), label));
        self.start_op("downloading", move |ctl| {
            let mut report = OpReport::default();
            let cancel = ctl.cancel;
            let total = files.len();
            for (i, remote) in files.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let fname = remote.rsplit('/').next().unwrap_or("download").to_string();
                let dest = local_dir.join(&fname);
                let cur = fname.clone();
                let mut fwd = |done: u64, tot: u64| {
                    (ctl.on_progress)(&cian_core::progress::Progress {
                        bytes_done: done,
                        bytes_total: tot,
                        files_done: i,
                        files_total: total,
                        current: cur.clone(),
                    });
                };
                let mut sctl = cian_scp::Ctl { cancel, on_progress: &mut fwd };
                match cian_scp::download(&target, remote, &dest, &mut sctl) {
                    Ok(via) => {
                        chmod_local(&dest, mode);
                        report.ok += 1;
                        report.note = Some(format!("via {}", via.label()));
                    }
                    Err(e) => report.note_error(format!("{}: {}", fname, e)),
                }
            }
            report
        });
    }

    /// Run the pending transfer against `remote` (a directory for upload, a file
    /// for download), on a worker thread with the shared progress popup.
    pub(crate) fn start_scp_transfer(&mut self, remote: &str, mode: Option<u32>) {
        let Some(p) = self.scp_pending.take() else { return };
        let remote = remote.trim().to_string();
        if remote.is_empty() {
            self.message = Some("cancelled (no remote path)".into());
            return;
        }
        let ScpPending { target, label, dir, locals, local_dir } = p;
        let verb = if dir == ScpDir::Upload { "uploading" } else { "downloading" };
        self.message = Some(format!("{} {} …", verb, label));
        self.start_op(if dir == ScpDir::Upload { "uploading" } else { "downloading" }, move |ctl| {
            let mut report = OpReport::default();
            // Bridge cian-scp's byte progress into the shared op progress.
            let cancel = ctl.cancel;
            match dir {
                ScpDir::Upload => {
                    let total = locals.len();
                    for (i, local) in locals.iter().enumerate() {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let fname = local.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                        let dest = format!("{}/{}", remote.trim_end_matches('/'), fname);
                        let cur = fname.clone();
                        let mut fwd = |done: u64, tot: u64| {
                            (ctl.on_progress)(&cian_core::progress::Progress {
                                bytes_done: done,
                                bytes_total: tot,
                                files_done: i,
                                files_total: total,
                                current: cur.clone(),
                            });
                        };
                        let mut sctl = cian_scp::Ctl { cancel, on_progress: &mut fwd };
                        match cian_scp::upload(&target, local, &dest, mode, &mut sctl) {
                            Ok(via) => {
                                report.ok += 1;
                                report.note = Some(format!("via {}", via.label()));
                            }
                            Err(e) => report.note_error(format!("{}: {}", fname, e)),
                        }
                    }
                }
                ScpDir::Download => {
                    let fname = remote.rsplit('/').next().unwrap_or("download").to_string();
                    let dest = local_dir.join(&fname);
                    let cur = fname.clone();
                    let mut fwd = |done: u64, tot: u64| {
                        (ctl.on_progress)(&cian_core::progress::Progress {
                            bytes_done: done,
                            bytes_total: tot,
                            files_done: 0,
                            files_total: 1,
                            current: cur.clone(),
                        });
                    };
                    let mut sctl = cian_scp::Ctl { cancel, on_progress: &mut fwd };
                    match cian_scp::download(&target, &remote, &dest, &mut sctl) {
                        Ok(via) => {
                            report.ok += 1;
                            report.note = Some(format!("via {}", via.label()));
                        }
                        Err(e) => report.note_error(format!("{}: {}", fname, e)),
                    }
                }
            }
            report
        });
    }

    /// Connect as `user` to host index `idx`, by typing the command into the
    /// shell panel.
    ///
    /// Typing it into a shell rather than spawning `ssh` directly means the
    /// user's own shell config and agent apply, and when the session ends the
    /// tab drops back to a local prompt instead of closing.
    pub(crate) fn ssh_connect(&mut self, idx: usize, user: &str) {
        let Some(h) = self.config.ssh_hosts.get(idx) else { return };
        let Some(u) = h.users.iter().find(|u| u.name == user) else { return };
        let mut cmd = format!("ssh {}@{}", u.name, h.host);
        if let Some(p) = h.port {
            cmd.push_str(&format!(" -p {}", p));
        }
        let label = format!("{}@{}", u.name, h.name);
        // Resolved before the command is sent so a slow `password_cmd` cannot
        // make us miss the prompt.
        let secret = u.secret();
        self.run_in_shell(cmd);
        match secret {
            Some(s) => {
                self.pending_auth =
                    Some(PendingAuth { secret: s, deadline: Instant::now() + AUTH_WINDOW });
                self.message = Some(format!("→ {} (sending password on prompt)", label));
            }
            None => self.message = Some(format!("→ {}", label)),
        }
    }

    /// Send the held password if ssh is now asking for one.
    ///
    /// Returns true if the UI should repaint. The secret is written straight to
    /// the PTY and never logged, echoed, or put in `message`.
    pub(crate) fn poll_pending_auth(&mut self) -> bool {
        let Some(auth) = &self.pending_auth else { return false };
        if Instant::now() > auth.deadline {
            // Expired: keyed host, refused login, or a prompt we do not answer.
            self.pending_auth = None;
            return false;
        }
        // Nothing to look at until the command has actually been delivered.
        if self.pending_shell_input.is_some() {
            return false;
        }
        let asking = match self.shell.active_session() {
            Some(s) => match s.parser().lock() {
                Ok(p) => looks_like_password_prompt(&p.screen().contents()),
                Err(_) => false,
            },
            None => false,
        };
        if !asking {
            return false;
        }
        let Some(auth) = self.pending_auth.take() else { return false };
        if let Some(s) = self.shell.active_session_mut() {
            let mut line = auth.secret;
            line.push('\n');
            s.write_input(line.as_bytes());
        }
        true
    }
}

/// Join a remote directory and a child name into a remote path (POSIX `/`).
fn join_remote(cwd: &str, name: &str) -> String {
    match cwd {
        "." | "" => name.to_string(),
        "/" => format!("/{}", name),
        _ => format!("{}/{}", cwd.trim_end_matches('/'), name),
    }
}

/// The parent of a remote path. Home-relative "." stays "."; an absolute path
/// climbs toward "/".
fn parent_remote(cwd: &str) -> String {
    match cwd {
        "." | "" => ".".to_string(),
        "/" => "/".to_string(),
        _ => {
            let trimmed = cwd.trim_end_matches('/');
            match trimmed.rsplit_once('/') {
                Some(("", _)) => "/".to_string(),      // "/foo" -> "/"
                Some((parent, _)) => parent.to_string(),
                None => ".".to_string(),                 // "foo" (relative) -> "."
            }
        }
    }
}

/// Apply Unix permission bits to a just-downloaded local file. A no-op on
/// Windows (NTFS has no Unix mode) and when `mode` is `None`.
#[cfg(unix)]
fn chmod_local(path: &std::path::Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt;
    if let Some(m) = mode {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(m));
    }
}

#[cfg(not(unix))]
fn chmod_local(_path: &std::path::Path, _mode: Option<u32>) {}

/// The user's Desktop, if it exists.
fn dirs_desktop() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let d = PathBuf::from(home).join("Desktop");
    d.is_dir().then_some(d)
}

#[cfg(test)]
mod tests {
    use super::{join_remote, parent_remote};

    #[test]
    fn remote_path_join_and_parent() {
        assert_eq!(join_remote(".", "docs"), "docs");
        assert_eq!(join_remote("/var", "log"), "/var/log");
        assert_eq!(join_remote("/", "etc"), "/etc");
        assert_eq!(join_remote("a/b", "c"), "a/b/c");

        assert_eq!(parent_remote("."), ".");
        assert_eq!(parent_remote("/"), "/");
        assert_eq!(parent_remote("/var/log"), "/var");
        assert_eq!(parent_remote("/var"), "/");     // climbs to root
        assert_eq!(parent_remote("a/b"), "a");
        assert_eq!(parent_remote("docs"), ".");     // relative single -> home
    }
}
