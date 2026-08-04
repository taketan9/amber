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
        // With nothing configured, go straight to typing a server by hand (#2)
        // rather than a dead-end notice.
        if self.config.ssh_hosts.is_empty() {
            self.start_manual_ssh();
            return;
        }
        self.popup = Popup::SshHosts { cursor: 0, filter: String::new() };
    }

    /// Begin an SFTP transfer: capture the local side, then reuse the SSH
    /// host/user picker to choose the server. `ssh_pick` routes back here once
    /// a user is chosen because [`App::scp_dir`] is set.
    pub(crate) fn start_scp(&mut self, dir: ScpDir) {
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
            ScpDir::Download | ScpDir::BrowsePane => (Vec::new(), pane.cwd.clone()),
        };
        self.scp_dir = Some((dir, locals, local_dir));
        // Nothing configured: type the server by hand (#2).
        if self.config.ssh_hosts.is_empty() {
            self.start_manual_ssh();
            return;
        }
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
        self.scp_dispatch(target, label);
    }

    /// Kick off the transfer for a resolved `target`, whether it came from a
    /// configured host or was typed in manually. Consumes `scp_dir` (the pending
    /// local side + direction) set up in [`App::start_scp`].
    pub(crate) fn scp_dispatch(&mut self, target: cian_scp::Target, label: String) {
        let Some((dir, locals, _local_dir)) = self.scp_dir.take() else { return };
        match dir {
            ScpDir::Upload => {
                // Upload browses the server (WinSCP-style) to pick the
                // destination folder; the pending holds the local files to send.
                self.scp_pending = Some(ScpPending { target: target.clone(), label: label.clone(), locals });
                self.scp_target = Some((target, label.clone()));
                self.open_remote_browser(label, ".", BrowsePurpose::Upload);
            }
            ScpDir::Download => {
                // Download opens a remote browser: navigate, mark files, then
                // pick where they land locally.
                self.scp_target = Some((target, label.clone()));
                self.open_remote_browser(label, ".", BrowsePurpose::Download);
            }
            ScpDir::BrowsePane => {
                self.open_remote_pane(target, label);
            }
        }
    }

    /// Start typing a connection by hand from the host picker (#2): server, user,
    /// then password. `for_scp` remembers whether a transfer is being set up so
    /// the final step either kicks off the transfer or logs a shell in.
    pub(crate) fn start_manual_ssh(&mut self) {
        let for_scp = self.scp_dir.is_some();
        self.popup = text_input(
            "manual connection — server",
            "user@host  (e.g. root@10.0.1.5, or deploy@web1:2222):",
            String::new(),
            InputKind::ManualSshTarget { for_scp },
        );
    }

    /// Second manual step: ask for the password for `user@host:port`.
    pub(crate) fn manual_ssh_password(&mut self, user: String, host: String, port: u16, for_scp: bool) {
        self.popup = text_input(
            "manual connection — password",
            format!("password for {user}@{host} (blank = none):"),
            String::new(),
            InputKind::ManualSshPass { user, host, port, for_scp },
        );
    }

    /// Final manual step: build the connection and either run the transfer or log
    /// the shell in (typing `ssh …` and feeding the password on the prompt).
    pub(crate) fn manual_ssh_finish(&mut self, user: String, host: String, port: u16, password: String, for_scp: bool) {
        let label = format!("{user}@{host}");
        if for_scp {
            if password.is_empty() {
                self.message = Some("a transfer needs a password".into());
                self.scp_dir = None;
                return;
            }
            let target = cian_scp::Target { host, port, user, password };
            self.scp_dispatch(target, label);
            return;
        }
        // Plain shell login: type the command, then feed the password (if any) on
        // the prompt via the existing pending-auth watcher.
        let mut cmd = format!("ssh {user}@{host}");
        if port != 22 {
            cmd.push_str(&format!(" -p {port}"));
        }
        self.popup = Popup::None;
        self.run_in_shell(cmd);
        if password.is_empty() {
            self.message = Some(format!("→ {label}"));
        } else {
            self.pending_auth = Some(PendingAuth { secret: password, deadline: Instant::now() + AUTH_WINDOW });
            self.message = Some(format!("→ {label} (sending password on prompt)"));
        }
    }

    /// Open the remote file browser at `cwd` and kick off its listing.
    pub(crate) fn open_remote_browser(&mut self, label: String, cwd: &str, purpose: BrowsePurpose) {
        self.popup = Popup::RemoteBrowser {
            label,
            cwd: cwd.to_string(),
            entries: Vec::new(),
            cursor: 0,
            scroll: 0,
            marked: std::collections::BTreeSet::new(),
            loading: true,
            purpose,
        };
        self.remote_ls_spawn(cwd.to_string());
    }

    /// List remote directory `path` on a worker thread; the result lands in
    /// [`App::poll_remote_ls`].
    fn remote_ls_spawn(&mut self, path: String) {
        let Some((target, _)) = self.scp_target.clone() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // list_dir returns the canonical absolute path of `path`, which
            // becomes the browser's cwd so parent navigation can climb to "/".
            let res = cian_scp::list_dir(&target, &path).map_err(|e| e.to_string());
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
                    Ok((cwd_new, mut entries)) => {
                        // A ".." row to step up one level, like the file panes —
                        // except at the filesystem root, where there is no up.
                        if cwd_new != "/" {
                            entries.insert(0, cian_scp::RemoteEntry { name: "..".into(), is_dir: true, size: 0 });
                        }
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
            let Popup::RemoteBrowser { cwd, entries, cursor, purpose, .. } = &self.popup else { return };
            let Some(e) = entries.get(*cursor) else { return };
            if e.is_dir && e.name == ".." {
                // The synthetic up-row: climb to the parent (cwd is absolute).
                (Some(parent_remote(cwd)), None)
            } else if e.is_dir {
                (Some(join_remote(cwd, &e.name)), None)
            } else if *purpose == BrowsePurpose::Upload {
                // Uploading picks a *folder*; a file under the cursor is a no-op.
                (None, None)
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

    // ── remote pane (a persistent SFTP-backed file pane) ──────────────────────

    /// The file-pane side to open a remote pane on (the focused one, or the last
    /// file pane when the shell is focused).
    fn remote_side(&self) -> FocusedPane {
        match self.focused {
            FocusedPane::Left | FocusedPane::Right => self.focused,
            _ => self.last_file_pane,
        }
    }

    fn side_idx(side: FocusedPane) -> usize {
        usize::from(matches!(side, FocusedPane::Right))
    }

    fn side_tabs_mut(&mut self, side: FocusedPane) -> &mut PaneTabs {
        if matches!(side, FocusedPane::Right) { &mut self.right } else { &mut self.left }
    }

    /// Open `target` as a **remote pane** on the active file side: browse the
    /// server like a local pane, starting at the login directory.
    pub(crate) fn open_remote_pane(&mut self, target: cian_scp::Target, label: String) {
        let side = self.remote_side();
        self.remote_targets[Self::side_idx(side)] = Some((target, label.clone()));
        self.focus(side);
        self.message = Some(format!("⇅ connecting to {label} …"));
        self.remote_pane_ls_spawn(side, ".".to_string());
    }

    /// List remote directory `path` for the remote pane on `side`, on a worker
    /// thread; the result lands in [`App::poll_remote_pane_ls`].
    fn remote_pane_ls_spawn(&mut self, side: FocusedPane, path: String) {
        let Some((target, _)) = self.remote_targets[Self::side_idx(side)].clone() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(cian_scp::list_dir(&target, &path).map_err(|e| e.to_string()));
        });
        self.remote_pane_ls = Some((side, rx));
    }

    /// Install a finished remote-pane listing into its pane. Returns true to
    /// repaint.
    pub(crate) fn poll_remote_pane_ls(&mut self) -> bool {
        let Some((side, rx)) = &self.remote_pane_ls else { return false };
        let side = *side;
        match rx.try_recv() {
            Ok(result) => {
                self.remote_pane_ls = None;
                match result {
                    Ok((cwd, remotes)) => {
                        let label = self.remote_targets[Self::side_idx(side)]
                            .as_ref()
                            .map(|(_, l)| l.clone())
                            .unwrap_or_default();
                        // A ".." up-row (except at the filesystem root), then the
                        // entries — each carrying its remote absolute path.
                        let mut entries = Vec::with_capacity(remotes.len() + 1);
                        if cwd != "/" {
                            entries.push(cian_core::Entry::remote("..", parent_remote(&cwd), true, 0, true));
                        }
                        for e in remotes {
                            let full = join_remote(&cwd, &e.name);
                            entries.push(cian_core::Entry::remote(e.name, full, e.is_dir, e.size, false));
                        }
                        self.side_tabs_mut(side).active_mut().enter_remote(label, cwd, entries);
                        self.message = None;
                    }
                    Err(e) => {
                        self.message = Some(format!("remote listing failed: {e}"));
                        self.remote_targets[Self::side_idx(side)] = None;
                    }
                }
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.remote_pane_ls = None;
                true
            }
        }
    }

    /// Enter the highlighted remote entry: descend a directory, climb via `..`.
    /// (A file does nothing yet — remote open/transfer comes next.)
    pub(crate) fn remote_pane_enter(&mut self) {
        let side = self.remote_side();
        let path = {
            let Some(pane) = self.active_pane() else { return };
            let Some((_, cwd)) = pane.remote_view() else { return };
            let cwd = cwd.to_string();
            let Some(e) = pane.selected() else { return };
            if e.is_parent {
                parent_remote(&cwd)
            } else if e.is_dir {
                // `path` holds the remote absolute path built at listing time.
                e.path.to_string_lossy().into_owned()
            } else {
                return;
            }
        };
        self.message = Some(format!("⇅ {path} …"));
        self.remote_pane_ls_spawn(side, path);
    }

    /// Go to the parent of the remote pane's current directory.
    pub(crate) fn remote_pane_parent(&mut self) {
        let side = self.remote_side();
        let Some(cwd) = self.active_pane().and_then(|p| p.remote_view()).map(|(_, c)| c.to_string())
        else {
            return;
        };
        self.remote_pane_ls_spawn(side, parent_remote(&cwd));
    }

    /// The active pane on a given side (read-only).
    fn side_pane(&self, side: FocusedPane) -> &Pane {
        if matches!(side, FocusedPane::Right) { self.right.active_ref() } else { self.left.active_ref() }
    }

    /// If a copy (`c`) crosses the local/remote boundary, run it as an SFTP
    /// transfer and return true. Local→remote uploads the marked files to the
    /// remote pane's directory; remote→local downloads them. `move` and
    /// remote↔remote are declined with a message (still "handled").
    pub(crate) fn try_remote_pane_transfer(&mut self, is_move: bool) -> bool {
        let active = self.remote_side();
        let opp = if matches!(active, FocusedPane::Right) { FocusedPane::Left } else { FocusedPane::Right };
        let a_remote = self.side_pane(active).is_remote();
        let o_remote = self.side_pane(opp).is_remote();
        if !a_remote && !o_remote {
            return false; // purely local — let the normal copy handle it
        }
        if is_move {
            self.message = Some(tr(self.lang,
                "move across hosts isn't supported — use c to copy",
                "ホスト間の移動は未対応 — コピーは c",
            ).into());
            return true;
        }
        if a_remote && !o_remote {
            // Download: the remote pane's marked entries → the local pane's dir.
            let files: Vec<String> =
                self.side_pane(active).target_paths().iter().map(|p| p.to_string_lossy().into_owned()).collect();
            if files.is_empty() {
                self.message = Some(tr(self.lang, "nothing to copy", "コピー対象なし").into());
                return true;
            }
            let local_dir = self.side_pane(opp).cwd.clone();
            if let Some((target, label)) = self.remote_targets[Self::side_idx(active)].clone() {
                self.scp_target = Some((target, label));
                self.start_remote_download(files, local_dir, None);
            }
            return true;
        }
        if !a_remote && o_remote {
            // Upload: the local pane's marked files → the remote pane's dir.
            let locals: Vec<PathBuf> =
                self.side_pane(active).target_paths().into_iter().filter(|p| p.is_file()).collect();
            if locals.is_empty() {
                self.message = Some(tr(self.lang, "select a file to upload", "アップロードするファイルを選択").into());
                return true;
            }
            let rcwd = self.side_pane(opp).remote_view().map(|(_, p)| p.to_string());
            if let (Some(rcwd), Some((target, label))) =
                (rcwd, self.remote_targets[Self::side_idx(opp)].clone())
            {
                self.scp_pending = Some(ScpPending { target, label, locals });
                self.run_scp_upload(rcwd);
            }
            return true;
        }
        // Both remote.
        self.message = Some(tr(self.lang,
            "remote → remote copy isn't supported yet",
            "リモート→リモートのコピーは未対応",
        ).into());
        true
    }

    /// F3 on a remote pane: fetch the file under the cursor to a temp path on a
    /// worker thread, then open it in the viewer when it lands. Returns true if
    /// it started a fetch (so the caller skips the local viewer).
    pub(crate) fn remote_pane_view(&mut self) -> bool {
        let side = self.remote_side();
        let (remote_path, name) = {
            let Some(e) = self.side_pane(side).selected() else { return false };
            if e.is_dir || e.is_parent {
                return false;
            }
            (e.path.to_string_lossy().into_owned(), e.name.clone())
        };
        let Some((target, _)) = self.remote_targets[Self::side_idx(side)].clone() else { return false };
        // A stable temp name per remote file (its basename), overwritten each view.
        let base = std::path::Path::new(&name).file_name().map(|s| s.to_os_string()).unwrap_or_default();
        let temp = std::env::temp_dir().join("cian-remote").join(base);
        if let Some(dir) = temp.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let temp_worker = temp.clone();
        std::thread::spawn(move || {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let mut prog = |_: u64, _: u64| {};
            let mut ctl = cian_scp::Ctl { cancel: &cancel, on_progress: &mut prog };
            let r = cian_scp::download(&target, &remote_path, &temp_worker, &mut ctl)
                .map(|_| ())
                .map_err(|e| e.to_string());
            let _ = tx.send(r);
        });
        self.remote_view = Some(RemoteView { rx, temp, name: name.clone() });
        self.message = Some(format!("⇅ fetching {name} …"));
        true
    }

    /// Install a finished remote-file fetch: open it in the viewer. Returns true
    /// to repaint.
    pub(crate) fn poll_remote_view(&mut self) -> bool {
        let Some(rv) = &self.remote_view else { return false };
        match rv.rx.try_recv() {
            Ok(result) => {
                let RemoteView { temp, name, .. } = self.remote_view.take().unwrap();
                match result {
                    Ok(()) => {
                        self.message = None;
                        self.open_viewer_at(&temp, &name, 0);
                    }
                    Err(e) => self.message = Some(format!("remote view failed: {e}")),
                }
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.remote_view = None;
                true
            }
        }
    }

    /// Leave the remote pane, returning it to its local directory.
    pub(crate) fn leave_remote_pane(&mut self) {
        let side = self.remote_side();
        if let Some(p) = self.active_pane_mut() {
            let _ = p.leave_flat();
        }
        self.remote_targets[Self::side_idx(side)] = None;
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

    /// Confirm the current remote directory as the upload destination and move on
    /// to the chmod prompts. The pending upload (target + local files) is already
    /// captured; we only needed the folder. Each file is asked for its own mode.
    pub(crate) fn remote_browser_upload_here(&mut self) {
        let cwd = if let Popup::RemoteBrowser { cwd, purpose: BrowsePurpose::Upload, .. } = &self.popup {
            cwd.clone()
        } else {
            return;
        };
        self.scp_target = None; // done browsing; the upload runs off scp_pending
        self.scp_upload_modes.clear();
        self.prompt_upload_chmod(cwd, 0);
    }

    /// Ask for the `idx`-th pending file's upload mode (one prompt per file, so
    /// each can differ). Once every file has a mode, kick off the upload. `Enter`
    /// on the seeded value reuses the previous file's mode, so accepting the same
    /// mode for all is just repeated Enters.
    pub(crate) fn prompt_upload_chmod(&mut self, remote: String, idx: usize) {
        let Some(p) = self.scp_pending.as_ref() else { return };
        let n = p.locals.len();
        if idx >= n {
            self.run_scp_upload(remote);
            return;
        }
        let fname = p.locals[idx]
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Seed with the previous file's mode (or 777) so repeating is one keypress.
        let seed = self
            .scp_upload_modes
            .last()
            .and_then(|m| *m)
            .map(|m| format!("{m:o}"))
            .unwrap_or_else(|| "777".to_string());
        self.popup = text_input(
            format!("upload chmod — {}/{}", idx + 1, n),
            format!("mode for {fname} (octal e.g. 777; blank = keep server default):"),
            seed,
            InputKind::UploadChmod { remote, idx },
        );
    }

    /// Upload the pending files, each with its collected mode, on a worker thread.
    pub(crate) fn run_scp_upload(&mut self, remote: String) {
        let Some(p) = self.scp_pending.take() else { return };
        let remote = remote.trim().to_string();
        if remote.is_empty() {
            self.message = Some("cancelled (no remote path)".into());
            return;
        }
        let ScpPending { target, label, locals, .. } = p;
        let modes = std::mem::take(&mut self.scp_upload_modes);
        let verify = self.config.options.verify_transfers.unwrap_or(false);
        self.popup = Popup::None;
        self.message = Some(format!("uploading {} …", label));
        self.start_op("uploading", move |ctl| {
            let mut report = OpReport::default();
            let cancel = ctl.cancel;
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
                let mode = modes.get(i).copied().flatten();
                let mut sctl = cian_scp::Ctl { cancel, on_progress: &mut fwd };
                match cian_scp::upload(&target, local, &dest, mode, &mut sctl) {
                    Ok(via) => {
                        report.ok += 1;
                        report.note = Some(format!("via {}", via.label()));
                        // Verify only when SFTP carried it: the SCP fallback
                        // cannot be re-read for a second checksum.
                        if verify && via == cian_scp::Transport::Sftp {
                            if let Err(e) = verify_transfer(&target, &dest, local, cancel) {
                                report.note_error(format!("{}: {}", fname, e));
                            } else {
                                report.note = Some(format!("via {} ✓ verified", via.label()));
                            }
                        }
                    }
                    Err(e) => report.note_error(format!("{}: {}", fname, e)),
                }
            }
            report
        });
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

    /// Ask for the mode to apply to downloaded files. Skipped on Windows: NTFS
    /// has no Unix permission bits, so a chmod on the local file can never take
    /// effect — asking for one there is only misleading (a downloaded file shows
    /// up as 644 via a Samba/NFS view no matter what was typed). The upload chmod
    /// still works because it is applied server-side over SFTP.
    pub(crate) fn prompt_download_chmod(&mut self, files: Vec<String>, dir: PathBuf) {
        if cfg!(windows) {
            self.start_remote_download(files, dir, None);
            return;
        }
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
        let verify = self.config.options.verify_transfers.unwrap_or(false);
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
                        // The file is down; a chmod failure is secondary, so still
                        // count it as a success but surface why the mode did not
                        // stick rather than silently dropping it.
                        report.ok += 1;
                        if let Err(e) = chmod_local(&dest, mode) {
                            report.note_error(format!("{}: downloaded, but chmod failed: {}", fname, e));
                        }
                        report.note = Some(format!("via {}", via.label()));
                        // Confirm the local copy matches the file still on the
                        // server (SFTP only — SCP cannot be re-read).
                        if verify && via == cian_scp::Transport::Sftp {
                            if let Err(e) = verify_transfer(&target, remote, &dest, cancel) {
                                report.note_error(format!("{}: {}", fname, e));
                            } else {
                                report.note = Some(format!("via {} ✓ verified", via.label()));
                            }
                        }
                    }
                    Err(e) => report.note_error(format!("{}: {}", fname, e)),
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
            // Submit with a carriage return — a getpass/readpassphrase prompt
            // reads the line ended by Enter (CR), which a bare `\n` may not be.
            let mut bytes = auth.secret.into_bytes();
            bytes.push(b'\r');
            s.write_input(&bytes);
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

/// Re-read a just-transferred file from the server over SFTP and compare its
/// checksum with the local copy's. `Ok(())` when they match; `Err(reason)` on a
/// mismatch (the transfer corrupted or truncated the file) or when the check
/// could not be run. Runs on the transfer worker thread, so it honours the same
/// cancel flag.
fn verify_transfer(
    target: &cian_scp::Target,
    remote_path: &str,
    local_path: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
) -> std::result::Result<(), String> {
    use cian_core::attrs::{hash_file, HashKind, Hasher};
    let kind = HashKind::Sha256;
    let local = match hash_file(local_path, kind, cancel) {
        Ok(Some(h)) => h,
        Ok(None) => return Err("verify cancelled".into()),
        Err(e) => return Err(format!("verify: reading the local file failed: {}", e)),
    };
    let mut hasher = Hasher::new(kind);
    if let Err(e) = cian_scp::remote_read(target, remote_path, cancel, &mut |b| hasher.update(b)) {
        return Err(format!("verify unavailable: {}", e));
    }
    let remote = hasher.finish();
    if remote == local {
        Ok(())
    } else {
        let short = |s: &str| s.chars().take(12).collect::<String>();
        Err(format!("CHECKSUM MISMATCH — local {}… ≠ remote {}…", short(&local), short(&remote)))
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
fn chmod_local(path: &std::path::Path, mode: Option<u32>) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(m) = mode {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(m))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn chmod_local(_path: &std::path::Path, _mode: Option<u32>) -> std::io::Result<()> {
    Ok(())
}

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

        // The reported case: connected as userA (home /home/userA), climbing up
        // must reach /home and then / rather than stopping at home.
        assert_eq!(parent_remote("/home/userA"), "/home");
        assert_eq!(parent_remote("/home"), "/");
        assert_eq!(parent_remote("/home/userA/"), "/home"); // trailing slash tolerated
    }
}
