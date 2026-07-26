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
        self.popup = Popup::SshHosts { cursor: 0, filter: String::new() };
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
        let (title, prompt, seed) = match dir {
            ScpDir::Upload => (
                "SFTP upload — remote folder",
                "remote directory to upload into:",
                String::new(),
            ),
            ScpDir::Download => (
                "SFTP download — remote file",
                "remote file path to download:",
                String::new(),
            ),
        };
        self.scp_pending = Some(ScpPending { target, label, dir, locals, local_dir });
        self.popup = text_input(title, prompt, seed, InputKind::ScpRemote);
    }

    /// Run the pending transfer against `remote` (a directory for upload, a file
    /// for download), on a worker thread with the shared progress popup.
    pub(crate) fn start_scp_transfer(&mut self, remote: &str) {
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
                        match cian_scp::upload(&target, local, &dest, &mut sctl) {
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
