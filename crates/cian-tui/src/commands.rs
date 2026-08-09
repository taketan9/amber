//! The `:` command line: dispatch (`run_command`) and every builtin — cd, mark,
//! mkdir, touch, transfer, ls, file, wc, head/tail, df, zip, and `!`-to-shell.
//! Split out of lib.rs as an `impl App` block.
use super::*;

impl App {
    /// Enter the `:` command line. From the shell this is the only keyboard way
    /// in (typing `:` there goes to the terminal), reached by Ctrl+Enter or the
    /// "Command…" menu item.
    pub(crate) fn enter_command_mode(&mut self) {
        self.command_buffer.clear();
        self.mode = Mode::Command;
    }

    /// Enter command mode with `prefix` already typed (e.g. `"rag "`), so a menu
    /// item that needs an argument drops the user straight onto the `:` line.
    pub(crate) fn prefill_command(&mut self, prefix: &str) {
        self.command_buffer = prefix.to_string();
        self.mode = Mode::Command;
    }

    pub(crate) fn run_command(&mut self) {
        let raw = self.command_buffer.trim().to_string();
        self.command_buffer.clear();
        self.mode = Mode::Normal;
        if raw.is_empty() {
            return;
        }
        // `!cmd` is a shell escape: everything after the bang is the command,
        // so it is split off before tokenising (the command has its own
        // quoting, and `%`-substitution happens inside).
        if let Some(rest) = raw.strip_prefix('!') {
            self.run_bang(rest);
            return;
        }

        // Split into a verb and its arguments. Whitespace-separated is enough:
        // the commands that take a path accept it as the whole remainder, and
        // the ones that take flags take single tokens.
        let mut parts = raw.split_whitespace();
        let raw_verb = parts.next().unwrap_or("");
        let args: Vec<&str> = parts.collect();
        let rest = raw[raw_verb.len()..].trim(); // the arguments as one string
        // A verb typed with the IME on comes through in full-width — `ｑ` for
        // `q`. Verbs are ASCII by definition, so folding one costs nothing;
        // the arguments are left exactly as typed, since a path may genuinely
        // hold full-width characters.
        let folded_verb;
        let verb = if raw_verb.is_ascii() {
            raw_verb
        } else {
            folded_verb = crate::util::fold_ime_word(raw_verb);
            folded_verb.as_str()
        };

        match verb {
            "q" | "quit" => self.should_quit = true,
            "shell" => self.focus(FocusedPane::Shell),
            "man" | "help" | "h" => self.open_manual(),
            "paste" => { let _ = self.paste_clip(); }
            "hidden" => self.toggle_hidden(),
            "view" | "look" => self.look_inside(),
            "diff" | "compare" => self.open_diff(),
            "copyto" => self.start_dest_picker(PendingOp::Copy),
            "moveto" => self.start_dest_picker(PendingOp::Move),
            "grep" => self.start_grep_prompt(),
            "markall" | "selectall" => self.mark_all(),
            "office" => self.open_in_office(),
            "officelink" => self.write_office_link(),
            // Repaint from nothing. A stray control character — one the
            // terminal acted on rather than passing along — can leave the
            // screen holding text cian never drew, and there is otherwise no
            // way back short of restarting.
            "redraw" | "refresh!" => {
                self.full_clear = true;
                self.message = Some(tr(self.lang, "redrawn", "画面を描き直しました").into());
            }
            // Which key did the terminal actually send? The answer to every
            // "that shortcut does nothing on my machine".
            "keys" => {
                self.key_probe = !self.key_probe;
                let mode = if self.kbd_enhanced { "enhanced" } else { "legacy" };
                self.message = Some(if self.key_probe {
                    format!(
                        "{} [keyboard: {mode}]",
                        tr(self.lang, "showing every key as cian receives it — :keys again to stop",
                                      "受け取ったキーをそのまま表示します — 止めるには もう一度 :keys"),
                    )
                } else {
                    tr(self.lang, "key report off", "キー表示をやめました").to_string()
                });
            }
            "find" => self.start_find_prompt(),
            "menu" => self.open_menu_at_cursor(),
            "sync" | "broadcast" => {
                self.focus(FocusedPane::Shell);
                let on = self.shell.toggle_broadcast();
                self.message = Some(if on {
                    "⇄ synchronize ON — input goes to all panes in this tab".into()
                } else {
                    "synchronize off".into()
                });
            }
            "count" | "steps" => self.start_count(),
            "du" | "diskusage" | "usage" => self.start_du_here(),
            "palette" | "commands" | "p" => self.start_command_palette(),
            "jump" | "j" => self.start_fuzzy_jump(),
            "toggles" | "ui" | "toggle" => self.start_toggles(),
            "files" | "ff" | "findfile" => self.start_file_finder(),
            "recent" | "oldfiles" | "fo" => self.start_recent_files(),
            "each" | "foreach" => self.run_each(rest),
            "undo" => self.undo_last(),
            "edit" | "e" => self.edit_selected_file(),
            // Rename by editing a list of names (vidir's interface).
            "bulkrename" | "brn" | "vidir" => self.start_editor_rename(),
            // Cursor-follow preview in the shell panel's area.
            "preview" | "pv" => self.toggle_preview(),
            // The operation queue: running + waiting file operations.
            "queue" | "ops" | "jobs" => self.start_op_queue(),
            // The pane's directory history. It lost its `h` key to pane
            // movement, so it needs a name you can reach.
            "back" | "dirhistory" | "visited" => self.start_history(),
            // Strip UTF-8 BOMs from the selection (UTF-16 left alone).
            "nobom" | "stripbom" => self.start_nobom(),
            // Open the file in a specific vi-family editor in a new shell tab.
            "vi" | "vim" | "nvim" => self.edit_in_new_tab(Some(verb)),
            "macro" | "macros" => self.start_macros(),
            "ssh" => self.start_ssh(),
            "sftp" | "remote" | "scp" | "browse" | "remotepane" => {
                self.start_scp(ScpDir::BrowsePane)
            }
            "ai" | "chat" | "crmaine" => self.open_ai_chat(),
            "aicmd" | "crmainecmd" => {
                if rest.is_empty() {
                    self.start_ai_shell_prompt();
                } else {
                    self.start_ai_shell_cmd(rest);
                }
            }
            // These dispatch to git or svn based on the pane's VCS.
            "stage" | "add" | "svnadd" => self.git_stage(),
            "unstage" | "reset" => self.git_unstage(),
            "discard" | "revert" | "checkout" | "svnrevert" => self.git_discard_prompt(),
            "gitlog" | "log" | "history" | "svnlog" => self.start_git_log(),
            "gitdiff" | "gdiff" | "svndiff" => self.git_diff_file(),
            // SVN-only working-copy operations.
            "svnupdate" | "update" | "up" => self.svn_update(),
            "svncommit" | "commit" | "ci" => self.svn_commit_prompt(),
            "svnresolve" | "resolve" => self.svn_resolve(),
            "snip" | "snippet" | "snippets" => self.start_snippets(),
            "aicommit" | "commitmsg" | "crmainecommit" => self.start_ai_commit_message(),
            "aijunk" | "junk" | "crmainejunk" => self.start_ai_junk(),
            "aiorganize" | "aistructure" | "organize" | "crmaineorganize" => self.start_ai_structure(),
            "airename" | "rename" | "crmainerename" => {
                if rest.is_empty() {
                    self.start_ai_rename_prompt();
                } else {
                    self.start_ai_rename(rest);
                }
            }
            // Pattern-based (non-AI) bulk rename. With no argument it prompts;
            // with one it applies the pattern straight to the review checklist.
            "brename" | "bren" | "renumber" | "renum" => {
                if rest.is_empty() {
                    self.start_bulk_rename();
                } else {
                    let targets = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
                    if targets.is_empty() {
                        self.message = Some(tr(self.lang, "nothing selected to rename", "リネーム対象がありません").into());
                    } else {
                        self.build_bulk_rename(&targets, rest);
                    }
                }
            }
            "aisearch" | "semsearch" | "ask" | "crmainesearch" => {
                if rest.is_empty() {
                    self.start_ai_search_prompt();
                } else {
                    self.start_ai_search(rest);
                }
            }
            "aierror" | "explain" | "crmaineerror" => self.explain_shell_error(),
            // With a question, fire it; bare, just open the window to type in.
            "rag" | "crmainerag" | "ask_rag" => {
                if rest.is_empty() {
                    self.open_crmaine_chat(ChatMode::Rag);
                } else {
                    self.start_rag(rest);
                }
            }
            "agent" | "ajent" | "crmaineagent" => {
                if rest.is_empty() {
                    self.open_crmaine_chat(ChatMode::Agent);
                } else {
                    self.start_agent(rest);
                }
            }
            "raginfo" | "crmaineinfo" | "crmainedoctor" | "ragstatus" => self.crmaine_doctor(),
            "index" | "ragindex" | "reindex" => self.start_index(rest),
            "ragshared" | "ragdefault" | "unindex" => self.crmaine_use_shared_index(),
            "coding" | "code" | "crmainecoding" => self.start_coding(rest),
            "impact" => self.start_impact(rest),
            "contradiction" | "contra" => self.start_contradiction(rest),
            "glossary" | "glossagen" => self.start_glossary(),
            "searchfiles" | "sf" | "corpussearch" => self.start_searchfiles(rest),
            "ragdebug" | "debugsearch" | "ragwhy" | "ragtrace" => self.start_debug_search(rest),
            "aidiff" | "explaindiff" | "crmainediff" => self.explain_diff(),
            "ailog" | "logtriage" | "triage" | "crmainelog" => self.triage_log(),
            "dupes" | "dup" | "duplicates" => self.start_dupes(),
            "theme" | "colorscheme" | "colourscheme" => {
                if rest.is_empty() {
                    self.start_theme_picker();
                } else {
                    self.set_theme_by_name(rest);
                }
            }
            "reload" | "source" => self.reload_config(),
            "where" | "config" | "paths" => self.show_config_paths(),
            // Mark / unmark entries whose name matches a glob (`:mark *.rs`).
            "mark" | "select" => self.cmd_mark(rest, true),
            "unmark" | "deselect" => self.cmd_mark(rest, false),

            // Navigation.
            "cd" | "goto" => {
                if rest.is_empty() {
                    self.start_jump_path();
                } else {
                    let _ = self.cmd_cd(rest);
                }
            }
            "pwd" => self.cmd_pwd(),

            // Creation.
            "mkdir" | "md" => self.cmd_mkdir(&args),
            "touch" => self.cmd_touch(&args),

            // Transfers: no argument means "to the other pane", matching the
            // y/m keys; an argument is an explicit destination.
            "cp" | "copy" => self.cmd_transfer(PendingOp::Copy, rest),
            "mv" | "move" => self.cmd_transfer(PendingOp::Move, rest),
            "rm" | "del" | "delete" => self.start_delete(),

            // Inspection.
            "ls" | "dir" => self.cmd_ls(&args),
            "stat" | "attr" | "attrs" => self.show_attributes(),
            "file" => self.cmd_file(),
            "wc" => self.cmd_wc(),
            "head" => self.cmd_peek(cian_core::inspect::End::Head, &args),
            "tail" => self.cmd_peek(cian_core::inspect::End::Tail, &args),
            "df" => self.cmd_df(&args),

            // Attributes and integrity.
            "chmod" => self.set_attr_command(rest),
            "readonly" => match rest {
                "on" | "true" | "1" => self.set_readonly_command(true),
                "off" | "false" | "0" => self.set_readonly_command(false),
                _ => self.message = Some("usage: :readonly on|off".into()),
            },
            "hash" | "sha256" | "md5" => {
                // `:hash md5` or `:md5` both work.
                let spec = if verb == "hash" { rest } else { verb };
                match cian_core::attrs::HashKind::parse(spec) {
                    Some(k) => self.start_hash(k),
                    None => self.message = Some(format!("unknown hash: {} (md5 or sha256)", spec)),
                }
            }

            // Archiving.
            "zip" => self.cmd_zip(&args),
            "tar" => self.cmd_tar(&args, false),
            "targz" | "tgz" | "tarball" | "tar.gz" => self.cmd_tar(&args, true),
            // Extraction auto-detects the format (zip / tar / tar.gz), so all of
            // these aliases do the same thing on the archive under the cursor.
            "unzip" | "untar" | "untargz" | "untar.gz" | "untgz" | "extract" | "unar" => {
                self.extract_selected()
            }

            other => self.message = Some(format!("unknown command: :{}", other)),
        }
    }

    /// `pwd`: show the focused pane's directory and put it on the clipboard,
    /// since the usual reason to ask is to paste it somewhere.
    pub(crate) fn cmd_pwd(&mut self) {
        let Some(p) = self.active_pane() else {
            self.message = Some("no active pane".into());
            return;
        };
        let path = p.cwd.display().to_string();
        if let Some(cb) = self.clipboard.as_mut() {
            let _ = cb.set_text(path.clone());
        }
        self.message = Some(format!("{}  (copied)", path));
    }

    /// `cd <path>`: enter a directory directly, without the prompt.
    /// `:mark <glob>` / `:unmark <glob>` — (un)mark every entry whose name
    /// matches the wildcard pattern (`*`, `?`; case-insensitive). No pattern
    /// acts on all entries.
    pub(crate) fn cmd_mark(&mut self, pattern: &str, mark: bool) {
        let pat = pattern.trim();
        let Some(p) = self.active_pane_mut() else { return };
        let mut n = 0usize;
        for i in 0..p.entries.len() {
            if pat.is_empty() || glob_match(pat, &p.entries[i].name) {
                let was = p.is_marked(i);
                if mark && !was {
                    p.set_mark_at(i);
                    n += 1;
                } else if !mark && was {
                    p.toggle_mark_at(i);
                    n += 1;
                }
            }
        }
        self.message = Some(format!(
            "{} {} entr{}",
            if mark { "marked" } else { "unmarked" },
            n,
            if n == 1 { "y" } else { "ies" }
        ));
    }

    pub(crate) fn cmd_cd(&mut self, arg: &str) -> Result<()> {
        // `cd -` / `cd ..` / `cd ~` are worth honouring since the muscle memory
        // is universal; everything else is a path.
        let target = match arg {
            "-" => self.active_pane().and_then(|p| p.history.get(1).cloned()),
            _ => Some(expand_path(arg)),
        };
        let Some(target) = target else {
            self.message = Some("no previous directory".into());
            return Ok(());
        };
        if !target.is_dir() {
            self.message = Some(format!("not a directory: {}", target.display()));
            return Ok(());
        }
        if let Some(p) = self.active_pane_mut() {
            p.jump_to(target.clone())?;
        }
        self.message = Some(format!("→ {}", target.display()));
        Ok(())
    }

    /// `mkdir <name>` / `mkdir -p a/b/c`, created in the focused directory.
    pub(crate) fn cmd_mkdir(&mut self, args: &[&str]) {
        let parents = args.contains(&"-p");
        let names: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with('-')).collect();
        if names.is_empty() {
            self.message = Some("usage: :mkdir [-p] <name>".into());
            return;
        }
        let Some(cwd) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        let mut made = 0;
        for name in &names {
            match cian_core::ops::make_dir(&cwd, name, parents) {
                Ok(_) => made += 1,
                Err(e) => {
                    self.message = Some(format!("mkdir: {}", e));
                    break;
                }
            }
        }
        if made > 0 {
            self.reload_active();
            if self.message.is_none() {
                self.message = Some(format!("mkdir: created {}", made));
            }
        }
    }

    /// `touch <name>...`: create empty files, or bump the mtime of existing ones.
    pub(crate) fn cmd_touch(&mut self, args: &[&str]) {
        let names: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with('-')).collect();
        if names.is_empty() {
            self.message = Some("usage: :touch <name>".into());
            return;
        }
        let Some(cwd) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        let mut n = 0;
        for name in &names {
            match cian_core::ops::touch(&cwd, name) {
                Ok(_) => n += 1,
                Err(e) => {
                    self.message = Some(format!("touch: {}", e));
                    break;
                }
            }
        }
        if n > 0 {
            self.reload_active();
            if self.message.is_none() {
                self.message = Some(format!("touch: {}", n));
            }
        }
    }

    /// `cp`/`mv`: no argument moves the selection to the other pane; an
    /// argument is an explicit destination directory (or, for a single item, a
    /// new path).
    pub(crate) fn cmd_transfer(&mut self, op: PendingOp, arg: &str) {
        if arg.is_empty() {
            self.start_transfer(op);
            return;
        }
        let targets = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if targets.is_empty() {
            self.message = Some("nothing to operate on".into());
            return;
        }
        let dest = expand_path(arg);
        if dest.is_dir() {
            self.popup = Popup::ConfirmTransfer { op, targets, dest };
            return;
        }
        // Not an existing directory: only meaningful as a rename/copy of a
        // single item to that exact path, and only if its parent exists.
        if targets.len() != 1 {
            self.message = Some(format!("not a directory: {}", dest.display()));
            return;
        }
        let parent_ok = dest.parent().map(|p| p.as_os_str().is_empty() || p.is_dir()).unwrap_or(false);
        if !parent_ok {
            self.message = Some(format!("no such directory: {}", dest.display()));
            return;
        }
        let src = &targets[0];
        let res = match op {
            PendingOp::Move => std::fs::rename(src, &dest).map_err(anyhow::Error::from),
            PendingOp::Copy => cian_core::ops::copy_one(src, dest.parent().unwrap_or(&dest), Conflict::Overwrite)
                .and_then(|_| {
                    // copy_one lands it under the parent with the source name;
                    // if a different name was asked for, put it right.
                    let landed = dest.parent().unwrap_or(&dest).join(src.file_name().unwrap_or_default());
                    if landed != dest { std::fs::rename(&landed, &dest)?; }
                    Ok(())
                }),
        };
        match res {
            Ok(_) => {
                self.reload_both();
                self.message = Some(format!("{} → {}", if op == PendingOp::Move { "mv" } else { "cp" }, dest.display()));
            }
            Err(e) => self.message = Some(format!("{}: {}", if op == PendingOp::Move { "mv" } else { "cp" }, e)),
        }
    }

    /// `ls`: refresh the listing. `ls -a` toggles hidden files, which is the
    /// one flag that makes sense when the pane already *is* the listing.
    pub(crate) fn cmd_ls(&mut self, args: &[&str]) {
        // `:ls -a` still toggles dotfiles (the long-standing behaviour). A plain
        // `:ls` now shows the same Attributes window as the menu, but for every
        // entry in the listing — a detailed `ls -l`-style view.
        if args.iter().any(|a| a.starts_with('-') && a.contains('a')) {
            self.toggle_hidden();
            return;
        }
        let paths: Vec<PathBuf> = match self.active_pane() {
            Some(p) => p.entries.iter().filter(|e| !e.is_parent).map(|e| e.path.clone()).collect(),
            None => Vec::new(),
        };
        if paths.is_empty() {
            self.message = Some("empty directory".into());
            return;
        }
        // Same cap as the Attributes window — the popup is not scrollable, and a
        // longer list would clip; the trailing "… and N more" says so.
        self.popup = Popup::Notice { lines: self.attributes_lines(&paths, 40) };
    }

    /// `file`: name what the selection is, by magic number and content.
    pub(crate) fn cmd_file(&mut self) {
        let paths = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        let mut lines = Vec::new();
        for path in paths.iter().take(30) {
            let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let desc = cian_core::inspect::classify(path).unwrap_or_else(|e| e.to_string());
            lines.push(format!("{} {}", fit(&name, 28), desc));
        }
        if paths.len() > 30 {
            lines.push(format!("... and {} more", paths.len() - 30));
        }
        self.popup = Popup::Notice { lines };
    }

    /// `wc`: line, word and byte counts for the selection, with a total.
    pub(crate) fn cmd_wc(&mut self) {
        let paths = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if paths.is_empty() {
            self.message = Some("nothing selected".into());
            return;
        }
        let mut lines = vec![format!("{:>9} {:>9} {:>11}  name", "lines", "words", "bytes"), String::new()];
        let mut tot = cian_core::inspect::Counts::default();
        let mut shown = 0;
        for path in paths.iter().take(30) {
            let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match cian_core::inspect::count(path) {
                Ok(c) => {
                    tot.lines += c.lines;
                    tot.words += c.words;
                    tot.bytes += c.bytes;
                    lines.push(format!("{:>9} {:>9} {:>11}  {}", c.lines, c.words, c.bytes, truncate(&name, 30)));
                    shown += 1;
                }
                Err(e) => lines.push(format!("{:>31}  {}: {}", "", truncate(&name, 20), e)),
            }
        }
        if shown > 1 {
            lines.push(String::new());
            lines.push(format!("{:>9} {:>9} {:>11}  total", tot.lines, tot.words, tot.bytes));
        }
        self.popup = Popup::Notice { lines };
    }

    /// `head`/`tail [-n N]`: the first or last N lines of the selected file.
    pub(crate) fn cmd_peek(&mut self, end: cian_core::inspect::End, args: &[&str]) {
        let n = parse_dash_n(args).unwrap_or(10);
        let Some(path) = self.active_pane().and_then(|p| p.selected().map(|e| e.path.clone())) else {
            self.message = Some("nothing selected".into());
            return;
        };
        match cian_core::inspect::peek(&path, end, n) {
            Ok(rows) => {
                let which = if end == cian_core::inspect::End::Head { "head" } else { "tail" };
                let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let mut lines = vec![format!("{} -n {}  {}", which, n, name), String::new()];
                lines.extend(rows.into_iter().map(|l| truncate(&l, 200)));
                self.popup = Popup::Notice { lines };
            }
            Err(e) => self.message = Some(format!("{}", e)),
        }
    }

    /// `df [-h|-k|-m|-g]`: free space on the focused pane's filesystem.
    pub(crate) fn cmd_df(&mut self, args: &[&str]) {
        let unit = match args.iter().find(|a| a.starts_with('-')) {
            Some(flag) => match cian_core::inspect::Unit::parse(flag) {
                Some(u) => u,
                None => {
                    self.message = Some(format!("df: unknown flag {} (try -h -k -m -g)", flag));
                    return;
                }
            },
            None => cian_core::inspect::Unit::Human,
        };
        let Some(cwd) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        match cian_core::inspect::disk_space(&cwd) {
            Ok(s) => {
                let lines = vec![
                    format!("filesystem holding  {}", cwd.display()),
                    String::new(),
                    format!("total      {}", unit.format(s.total)),
                    format!("used       {}   ({}%)", unit.format(s.used()), s.percent_used()),
                    format!("available  {}", unit.format(s.available)),
                ];
                self.popup = Popup::Notice { lines };
            }
            Err(e) => self.message = Some(format!("df: {}", e)),
        }
    }

    /// `zip [-e] <name>`: bundle the selection. `-e` asks for a password and
    /// AES-encrypts the result.
    pub(crate) fn cmd_zip(&mut self, args: &[&str]) {
        let encrypt = args.contains(&"-e") || args.contains(&"-p");
        let name = args.iter().copied().find(|a| !a.starts_with('-'));
        let Some(name) = name else {
            self.message = Some("usage: :zip [-e] <name.zip>".into());
            return;
        };
        let sources = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if sources.is_empty() {
            self.message = Some("nothing selected to zip".into());
            return;
        }
        let Some(cwd) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        let mut fname = name.to_string();
        if !fname.to_lowercase().ends_with(".zip") {
            fname.push_str(".zip");
        }
        let dest = cwd.join(&fname);
        if dest.exists() {
            self.message = Some(format!("already exists: {}", fname));
            return;
        }
        if encrypt {
            // Collect the password on a masked prompt, then build the zip when
            // it is submitted.
            self.popup = text_input(
                "zip password",
                "password (AES-256; Explorer cannot open — use 7-Zip):",
                String::new(),
                InputKind::ZipPassword { dest, sources },
            );
        } else {
            self.start_zip(dest, sources, None);
        }
    }

    /// Kick off zip creation on a worker, with progress and cancel like the
    /// other bulk operations.
    pub(crate) fn start_zip(&mut self, dest: PathBuf, sources: Vec<PathBuf>, password: Option<String>) {
        self.start_op("zipping", move |ctl| {
            cian_core::archive::create_zip(&sources, &dest, password.as_deref(), ctl)
        });
    }

    /// `:tar <name>` / `:targz <name>` — tar up the marked files (or the cursor's)
    /// into the active pane's directory.
    pub(crate) fn cmd_tar(&mut self, args: &[&str], gz: bool) {
        let Some(name) = args.iter().copied().find(|a| !a.starts_with('-')) else {
            self.message = Some(if gz { "usage: :targz <name>" } else { "usage: :tar <name>" }.into());
            return;
        };
        let sources = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if sources.is_empty() {
            self.message = Some("nothing selected to archive".into());
            return;
        }
        let Some(cwd) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        let mut fname = name.to_string();
        let low = fname.to_lowercase();
        if gz {
            if !(low.ends_with(".tar.gz") || low.ends_with(".tgz")) {
                fname.push_str(".tar.gz");
            }
        } else if !low.ends_with(".tar") {
            fname.push_str(".tar");
        }
        let dest = cwd.join(&fname);
        if dest.exists() {
            self.message = Some(format!("already exists: {}", fname));
            return;
        }
        self.start_tar(dest, sources, gz);
    }

    /// Kick off tar creation on a worker (progress + cancel like zip).
    pub(crate) fn start_tar(&mut self, dest: PathBuf, sources: Vec<PathBuf>, gz: bool) {
        let label = if gz { "tarring gz" } else { "tarring" };
        self.start_op(label, move |ctl| cian_core::archive::create_tar(&sources, &dest, gz, ctl));
    }

    /// From the right-click Compress submenu: gather the selection and ask for
    /// the archive name; [`Self::finish_text_input`] builds it on submit.
    pub(crate) fn prompt_compress(&mut self, kind: CompressKind) {
        let sources = self.active_pane().map(|p| p.target_paths()).unwrap_or_default();
        if sources.is_empty() {
            self.message = Some(tr(self.lang, "nothing selected to compress", "圧縮対象がありません").into());
            return;
        }
        // A sensible default name: the single selection's stem, else the folder's.
        let default = sources
            .first()
            .filter(|_| sources.len() == 1)
            .and_then(|p| p.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()))
            .or_else(|| self.active_pane().and_then(|p| p.cwd.file_name().and_then(|n| n.to_str()).map(|s| s.to_string())))
            .unwrap_or_else(|| "archive".to_string());
        let ext = match kind {
            CompressKind::Zip | CompressKind::ZipEnc => ".zip",
            CompressKind::TarGz => ".tar.gz",
        };
        self.popup = text_input(
            "compress",
            format!("archive name (adds {}):", ext),
            default,
            InputKind::CompressName { kind, sources },
        );
    }

    /// `!cmd`: run a shell command in the shell panel, with `%` substituted by
    /// the selected paths, `%f` by the current file, `%d` by the directory.
    pub(crate) fn run_bang(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            self.message = Some("usage: :!<command>   (% = selection, %f = file, %d = dir)".into());
            return;
        }
        let pane = self.active_pane();
        let cwd = pane.map(|p| p.cwd.display().to_string()).unwrap_or_default();
        let file = pane
            .and_then(|p| p.selected().map(|e| e.path.display().to_string()))
            .unwrap_or_default();
        let sel: Vec<String> = pane
            .map(|p| p.target_paths())
            .unwrap_or_default()
            .iter()
            .map(|p| shell_quote(&p.display().to_string()))
            .collect();
        let sel = sel.join(" ");

        // Longer tokens first so `%f`/`%d` are not eaten by `%`.
        let expanded = cmd
            .replace("%f", &shell_quote(&file))
            .replace("%d", &shell_quote(&cwd))
            .replace('%', &sel);
        self.run_in_shell(expanded);
    }
}
