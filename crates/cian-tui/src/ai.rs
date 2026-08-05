//! AI-backed actions on the App: chat, NL→command, junk detection, structure
//! and rename suggestions, semantic search, commit-message drafting, error
//! explanation, file summary — plus the duplicate-file scan that shares the
//! same review-and-approve shape. Split out of lib.rs as an `impl App` block;
//! it reaches the rest of App through `use super::*`.

use super::*;

/// A stored chat conversation for `ai_history.json` — a transcript and the
/// backend it spoke to (so a reopened conversation still routes follow-ups).
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct StoredChat {
    mode: ChatMode,
    log: Vec<ChatMsg>,
}

/// Load the saved chat history (portable-aware), newest first. Empty if there
/// is none or it is unreadable.
pub(crate) fn restore_ai_history() -> Vec<(ChatMode, Vec<ChatMsg>)> {
    let Some(path) = cian_lua::config_read_path("ai_history.json").filter(|p| p.exists()) else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    match serde_json::from_str::<Vec<StoredChat>>(&text) {
        Ok(v) => v.into_iter().map(|c| (c.mode, c.log)).collect(),
        Err(_) => Vec::new(),
    }
}

/// Who the assistant is, prepended to every conversational (chat) system prompt.
/// The product is crmaine, read「カーマイン」in Japanese; that is the name it
/// answers to, and it refers to itself in the first person as「私」.
/// Read up to the last `max_bytes` of a file as text. Logs grow at the end, so
/// the tail is the part worth sending; a partial first line (from cutting mid
/// file) is dropped so the model does not read a fragment as a whole entry.
fn read_tail(path: &std::path::Path, max_bytes: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    let _ = f.seek(SeekFrom::Start(start));
    let mut buf = Vec::new();
    let _ = f.read_to_end(&mut buf);
    let s = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        if let Some(nl) = s.find('\n') {
            return s[nl + 1..].to_string();
        }
    }
    s
}

/// Who the assistant is, prepended to every conversational (chat) system prompt.
/// The product is crmaine, read「カーマイン」in Japanese; that is the name it
/// answers to, and it refers to itself in the first person as「私」.
fn persona() -> &'static str {
    "あなたはこの二画面ファイラ／ターミナル「cian」に組み込まれた AI アシスタントです。\
     あなたの名前は「カーマイン」。自分を指すときは常に一人称「私」を使い、\
     名前を尋ねられたら「私はカーマインです」と名乗ってください。\
     (Your name is Carmine / カーマイン; always refer to yourself as「私」.)"
}

impl App {
    /// Is the AI helper configured and working? Returns the cached result of the
    /// background probe (see [`Self::spawn_ai_probe`]); `false` until the probe
    /// lands, so this NEVER blocks — the python `--check` can take seconds and
    /// must not freeze the UI (e.g. when building the right-click menu).
    pub(crate) fn ai_ready(&mut self) -> bool {
        if self.ai.is_none() {
            return false;
        }
        self.ai_ready.unwrap_or(false)
    }

    /// Kick off the AI availability check on a worker thread. Called at startup
    /// and after `:reload`; the result is installed by [`Self::poll_ai_probe`].
    pub(crate) fn spawn_ai_probe(&mut self) {
        self.ai_ready = None;
        self.ai_probe = None;
        let Some(cfg) = self.ai.clone() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(cian_ai::available(&cfg));
        });
        self.ai_probe = Some(rx);
    }

    /// Install the AI probe's result once it lands. Returns true if it changed.
    pub(crate) fn poll_ai_probe(&mut self) -> bool {
        let Some(rx) = &self.ai_probe else { return false };
        match rx.try_recv() {
            Ok(ready) => {
                self.ai_ready = Some(ready);
                self.ai_probe = None;
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.ai_ready = Some(false);
                self.ai_probe = None;
                true
            }
        }
    }

    /// True during the brief startup window — while the AI probe is still
    /// running, or for a short minimum — so a "starting up" splash can show.
    /// Capped so it can never linger.
    pub(crate) fn is_starting_up(&self) -> bool {
        let e = self.startup_at.elapsed();
        e < std::time::Duration::from_secs(6)
            && (self.ai_probe.is_some() || e < std::time::Duration::from_millis(1200))
    }

    pub(crate) fn open_ai_chat(&mut self) {
        if self.ai.is_none() {
            self.message = Some("AI not configured — add cian.ai{...} to init.lua".into());
            return;
        }
        if !self.ai_ready() {
            self.message =
                Some("AI unavailable (python, packages, or sign-in) — feature hidden".into());
            return;
        }
        self.start_ai_chat(ChatMode::Ai, Vec::new(), false);
    }

    /// Summarise the file open in the F3 viewer. Unlike the metadata-only
    /// features, this sends the file's TEXT to the model (a content-egress
    /// action), so it is gated behind an explicit key in the viewer. The reply
    /// opens in the AI chat, where it can be read, selected and copied.
    pub(crate) fn summarize_viewer(&mut self) {
        if self.ai.is_none() {
            self.message = Some("AI not configured — add cian.ai{...} to init.lua".into());
            return;
        }
        // Pull the decoded text and a name out of the viewer.
        let (name, content) = if let Popup::Viewer { title, view, .. } = &self.popup {
            (title.clone(), view.lines.join("\n"))
        } else {
            return;
        };
        if content.trim().is_empty() {
            self.message = Some("nothing to summarise".into());
            return;
        }
        if !self.ai_ready() {
            self.message = Some("AI unavailable (python, packages, or sign-in)".into());
            return;
        }
        // Bound the payload: a summary rarely needs the whole of a large file,
        // and an unbounded body would blow the token budget.
        let body = truncate_text_for_ai(&content, 24_000);
        let system = "You summarise a file's contents for a developer. Give a \
             short, plain-text summary: what it is, its purpose, and the key \
             points or structure. Be concise; no preamble, no markdown headings."
            .to_string();
        // Open the chat with the request shown, so the reply lands in a place
        // that can be scrolled, selected and copied — and followed up in.
        self.start_ai_chat(ChatMode::Ai, vec![ChatMsg { user: true, text: format!("Summarise {}", name) }], true);
        self.ai_request(AiPurpose::Chat, system, body);
    }

    /// Explain the error visible in the active shell pane. Sends the visible
    /// terminal text (a content-egress action, hence an explicit command/menu
    /// item), and opens the reply in the AI chat.
    pub(crate) fn explain_shell_error(&mut self) {
        if self.ai.is_none() {
            self.message = Some("AI not configured — add cian.ai{...} to init.lua".into());
            return;
        }
        // Grab the visible screen of the active shell pane.
        let screen = self.shell.active_session().and_then(|s| {
            s.parser().lock().ok().map(|p| p.screen().contents())
        });
        let Some(screen) = screen else {
            self.message = Some("no shell here".into());
            return;
        };
        // Collapse the trailing blank rows a terminal screen is padded with.
        let text = screen.trim_end().to_string();
        if text.is_empty() {
            self.message = Some("nothing on the shell to explain".into());
            return;
        }
        if !self.ai_ready() {
            self.message = Some("AI unavailable (python, packages, or sign-in)".into());
            return;
        }
        let body = truncate_text_for_ai(&text, 8_000);
        let os = if cfg!(windows) { "Windows" } else if cfg!(target_os = "macos") { "macOS" } else { "Linux" };
        let system = format!(
            "You explain shell/terminal errors for a developer on {os}. Given the \
             recent terminal output, say plainly what went wrong and the most \
             likely fix (a command or a change). If there is no error, say the \
             output looks fine. Be concise; plain text, no markdown headings.",
        );
        self.start_ai_chat(ChatMode::Ai, vec![ChatMsg { user: true, text: "Explain the last error".into() }], true);
        self.ai_request(AiPurpose::Chat, system, body);
    }

    /// Explain the diff currently on screen (a two-file diff or a folder
    /// compare): what changed and the likely intent, grouped rather than line by
    /// line. Reuses the same text the copy/save actions produce.
    pub(crate) fn explain_diff(&mut self) {
        if self.ai.is_none() {
            self.message = Some("AI not configured — add cian.ai{...} to init.lua".into());
            return;
        }
        let Some(text) = self.diff_as_text() else {
            self.message = Some("no diff to explain".into());
            return;
        };
        if !self.ai_ready() {
            self.message = Some("AI unavailable (python, packages, or sign-in)".into());
            return;
        }
        let body = truncate_diff_for_ai(&text, 8_000);
        let system = "You explain a diff between two files (or two folders) for a \
             developer. Summarize WHAT changed and, where you can tell, the \
             likely intent — grouped by theme, not line by line. Call out \
             anything risky: a removed check, a changed default, a probable \
             typo. Be concise; plain text, no markdown headings."
            .to_string();
        self.start_ai_chat(ChatMode::Ai, vec![ChatMsg { user: true, text: "Explain this diff".into() }], true);
        self.ai_request(AiPurpose::Chat, system, body);
    }

    /// Triage the selected file as a log: from its tail, surface the errors that
    /// matter, a rough timeline, and the most likely cause / next check. Aimed
    /// at the RHEL/AIX/Oracle logs this is built for.
    pub(crate) fn triage_log(&mut self) {
        if self.ai.is_none() {
            self.message = Some("AI not configured — add cian.ai{...} to init.lua".into());
            return;
        }
        let picked = self
            .active_pane()
            .and_then(|p| p.selected())
            .filter(|e| !e.is_dir && !e.is_parent)
            .map(|e| (e.path.clone(), e.name.clone()));
        let Some((path, name)) = picked else {
            self.message = Some("select a log file to triage".into());
            return;
        };
        if !self.ai_ready() {
            self.message = Some("AI unavailable (python, packages, or sign-in)".into());
            return;
        }
        // A log's meaning is at its end — read the tail, not the head.
        let tail = read_tail(&path, 16_000);
        if tail.trim().is_empty() {
            self.message = Some("that file is empty".into());
            return;
        }
        let system = "You triage a log file for an operator (often RHEL/AIX or \
             Oracle). From the tail below: list the errors and warnings that \
             matter, each with its key line; note a rough timeline if the \
             timestamps show one; then give the single most likely cause and the \
             next thing to check. Ignore routine INFO noise. Be concise; plain \
             text, no markdown headings."
            .to_string();
        self.start_ai_chat(ChatMode::Ai, vec![ChatMsg { user: true, text: format!("Triage the log: {}", name) }], true);
        self.ai_request(AiPurpose::Chat, system, tail);
    }

    /// Does the shell's visible output look like it just ended in an error?
    ///
    /// A heuristic — cian has no shell-integration marks, so it cannot read an
    /// exit code — used only to *offer* an explanation (a hint chip), never to
    /// act. Kept to strong signatures on the last few non-empty lines so routine
    /// output does not keep the nudge lit. Off entirely when AI is unconfigured.
    pub(crate) fn shell_error_detected(&self) -> bool {
        if self.ai.is_none() {
            return false;
        }
        let Some(screen) = self
            .shell
            .active_session()
            .and_then(|s| s.parser().lock().ok().map(|p| p.screen().contents()))
        else {
            return false;
        };
        let tail: String = screen
            .lines()
            .rev()
            .filter(|l| !l.trim().is_empty())
            .take(6)
            .collect::<Vec<_>>()
            .join("\n")
            .to_lowercase();
        const SIGNS: [&str; 12] = [
            "command not found",
            "no such file or directory",
            "permission denied",
            "traceback (most recent call last)",
            "segmentation fault",
            "fatal:",
            "ora-0",
            "ora-1",
            "is not recognized as",
            "syntax error",
            "connection refused",
            "no such host",
        ];
        SIGNS.iter().any(|s| tail.contains(s))
    }

    /// Precondition facts to feed the model: the `cian.ai_context{...}` facts
    /// from init.lua, plus the connected server's `notes` when the active shell
    /// is on a known SSH host. Empty when nothing is configured.
    pub(crate) fn ai_context_block(&self) -> String {
        let mut facts: Vec<String> = self.config.ai_context.clone();
        // The server the active shell is logged into, matched to a configured
        // host so its recorded OS / middleware / versions can be handed over.
        if let Some(host) = self.shell.active_title().and_then(|t| host_from_title(&t)) {
            for h in &self.config.ssh_hosts {
                if h.host == host || h.name == host {
                    if let Some(notes) = &h.notes {
                        facts.push(format!("The server '{}' ({}): {}", h.name, h.host, notes));
                    }
                }
            }
        }
        if facts.is_empty() {
            return String::new();
        }
        let mut s = String::from("Context about the user's environment you can rely on:\n");
        for f in &facts {
            s.push_str("- ");
            s.push_str(f);
            s.push('\n');
        }
        s
    }

    /// Fire an AI request on a worker thread, tagged with what to do with the
    /// reply. Only one runs at a time.
    pub(crate) fn ai_request(&mut self, purpose: AiPurpose, system: String, user: String) {
        let Some(cfg) = self.ai.clone() else { return };
        // Prepend the user's environment facts so every purpose benefits.
        let context = self.ai_context_block();
        let system = if context.is_empty() {
            system
        } else {
            format!("{}\n{}", context, system)
        };
        // Give the conversational replies a consistent identity. Only for chat:
        // the structured purposes (rename / search / organize / commit) must
        // return parseable output, and a persona instruction would loosen that.
        let system = if matches!(purpose, AiPurpose::Chat) {
            format!("{}\n{}", persona(), system)
        } else {
            system
        };
        if self.ai_job.is_some() {
            self.message = Some("AI is busy".into());
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let r = cian_ai::chat(&cfg, &system, &user).map_err(|e| e.to_string());
            let _ = tx.send(r);
        });
        self.ai_job = Some(AiJob { rx, purpose });
    }

    /// Open a chat popup, first tucking the current conversation (if it has an
    /// answer in it) into the history so switching or restarting never loses it.
    pub(crate) fn start_ai_chat(&mut self, mode: ChatMode, log: Vec<ChatMsg>, pending: bool) {
        self.archive_current_ai_chat();
        self.popup =
            Popup::AiChat { input: String::new(), log, scroll: usize::MAX, pending, sel: None, mode };
    }

    /// Snapshot the open chat into `ai_history` (newest first, deduped) if it
    /// holds at least one answer. A no-op otherwise.
    pub(crate) fn archive_current_ai_chat(&mut self) {
        if let Popup::AiChat { log, mode, .. } = &self.popup {
            if log.iter().any(|m| !m.user) {
                let snap = (*mode, log.clone());
                if self.ai_history.first() != Some(&snap) {
                    self.ai_history.insert(0, snap);
                    self.ai_history.truncate(30);
                    self.save_ai_history();
                }
            }
        }
    }

    /// Persist the chat history so it survives a restart. Portable-aware (beside
    /// `init.lua`, or next to the executable). NOTE: this writes the full
    /// conversation text — including RAG answers — to `ai_history.json` in
    /// plaintext; failures are silent.
    pub(crate) fn save_ai_history(&self) {
        let Some(path) = cian_lua::config_write_path("ai_history.json") else { return };
        let stored: Vec<StoredChat> = self
            .ai_history
            .iter()
            .map(|(mode, log)| StoredChat { mode: *mode, log: log.clone() })
            .collect();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&stored) {
            let _ = std::fs::write(path, json);
        }
    }

    /// A one-line title for a stored conversation — its first question.
    pub(crate) fn ai_history_title(log: &[ChatMsg]) -> String {
        log.iter()
            .find(|m| m.user)
            .map(|m| m.text.replace('\n', " "))
            .map(|t| if t.chars().count() > 60 { format!("{}…", t.chars().take(60).collect::<String>()) } else { t })
            .unwrap_or_else(|| "(empty)".to_string())
    }

    /// `Ctrl+R` in the chat: archive the current conversation, then show the
    /// history picker. With nothing to show, say so instead of an empty box.
    pub(crate) fn open_ai_history(&mut self) {
        self.archive_current_ai_chat();
        if self.ai_history.is_empty() {
            self.message =
                Some(tr(self.lang, "no past conversations yet", "過去の会話はまだありません").into());
            return;
        }
        self.popup = Popup::AiHistory { cursor: 0 };
    }

    /// Reopen the conversation at `i` as the live chat.
    pub(crate) fn load_ai_conversation(&mut self, i: usize) {
        if let Some((mode, log)) = self.ai_history.get(i).cloned() {
            self.popup = Popup::AiChat {
                input: String::new(),
                log,
                scroll: usize::MAX,
                pending: false,
                sel: None,
                mode,
            };
        }
    }

    /// Forget the stored conversation at `i`.
    pub(crate) fn delete_ai_conversation(&mut self, i: usize) {
        if i < self.ai_history.len() {
            self.ai_history.remove(i);
            self.save_ai_history();
        }
        if self.ai_history.is_empty() {
            self.popup = Popup::None;
        }
    }

    /// Send the typed chat line, routing a follow-up to the same backend the
    /// conversation started on — the local `:ai` model, or crmaine `/query` /
    /// `/agent` — so a RAG/agent thread stays a RAG/agent thread.
    pub(crate) fn send_ai_message(&mut self) {
        let (question, mode) =
            if let Popup::AiChat { input, log, pending, scroll, mode, .. } = &mut self.popup {
                let q = input.trim().to_string();
                if q.is_empty() || *pending {
                    return;
                }
                input.clear();
                log.push(ChatMsg { user: true, text: q.clone() });
                *pending = true;
                *scroll = usize::MAX;
                (q, *mode)
            } else {
                return;
            };
        match mode {
            ChatMode::Ai => {
                let system = "You are a concise assistant embedded in a terminal file \
                              manager. Answer briefly in plain text."
                    .to_string();
                self.ai_request(AiPurpose::Chat, system, question);
            }
            ChatMode::Rag => self.fire_crmaine("/query", &question, serde_json::Value::Null),
            // Coding follow-ups are just agent follow-ups (the code went in the
            // first turn; the conversation history carries it from there).
            ChatMode::Agent | ChatMode::Coding => {
                let history = self.crmaine_history_json();
                self.fire_crmaine("/agent", &question, history);
            }
        }
    }

    /// Open the "describe a command" prompt (if AI is available).
    pub(crate) fn start_ai_shell_prompt(&mut self) {
        if self.ai.is_none() {
            self.message = Some("AI not configured — add cian.ai{...} to init.lua".into());
            return;
        }
        if !self.ai_ready() {
            self.message = Some("AI unavailable (python, packages, or sign-in)".into());
            return;
        }
        self.popup = text_input(
            "AI shell command",
            "describe what you want to do:",
            String::new(),
            InputKind::AiShellCmd,
        );
    }

    /// Ask the model for a shell command that does what `description` says, then
    /// show it for review before it touches the prompt.
    pub(crate) fn start_ai_shell_cmd(&mut self, description: &str) {
        let description = description.trim().to_string();
        if description.is_empty() {
            return;
        }
        // Where will this command actually run? The active shell may be local, or
        // already logged into a server over SSH — the command must suit THAT
        // system (AIX `ls` vs Windows `dir`, and never an ssh-wrapped command).
        // A `user@host` title means a Unix-style shell either way; a matched SSH
        // host also hands over its recorded OS/middleware via `notes`.
        let host = self.shell.active_title().and_then(|t| host_from_title(&t));
        let target = match &host {
            Some(h) => {
                let known = self
                    .config
                    .ssh_hosts
                    .iter()
                    .find(|x| x.host == *h || x.name == *h)
                    .and_then(|x| x.notes.as_deref());
                match known {
                    Some(notes) => format!(
                        "a shell already logged in over SSH to the server '{h}'. \
                         That system: {notes}. Use that system's own commands and flags \
                         (AIX / Solaris / HP-UX differ from GNU/Linux)."
                    ),
                    None => format!(
                        "a Unix-like shell on '{h}' (it may be a remote server reached over \
                         SSH). Use POSIX / Unix commands, not Windows ones."
                    ),
                }
            }
            None => {
                let os = if cfg!(windows) {
                    "Windows"
                } else if cfg!(target_os = "macos") {
                    "macOS"
                } else {
                    "Linux"
                };
                format!("your local {os} {} shell", self.shell_cmd_name())
            }
        };
        let ctx = self.ai_context_block();
        let system = format!(
            "Translate the user's request into ONE shell command to run in {target}.\n\
             The command is pasted into that shell exactly as written and run there, so:\n\
             - Do NOT wrap it in `ssh` and do NOT add a hostname or any login/connection \
               step — the shell is already at the right place.\n\
             - Use the command style and flags native to that system.\n\
             - Output ONLY the command — no explanation, no markdown, no code fences.\n\
             {ctx}"
        );
        self.message = Some("asking AI for a command…".into());
        self.ai_request(AiPurpose::ShellCommand, system, description);
    }

    /// Draft a commit message from the staged diff of the active pane's repo,
    /// then show it editable before committing. Silent-ish when AI is off, and
    /// helpful when the stage is empty (the common "forgot to `git add`" case).
    pub(crate) fn start_ai_commit_message(&mut self) {
        if self.ai.is_none() {
            self.message = Some("AI not configured — add cian.ai{...} to init.lua".into());
            return;
        }
        let Some(dir) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        // Not in a repo at all?
        let Some(diff) = cian_core::git::staged_diff(&dir) else {
            self.message = Some("not a git repository".into());
            return;
        };
        if diff.trim().is_empty() {
            self.message = Some("nothing staged — `git add` first (or stage from the pane)".into());
            return;
        }
        if !self.ai_ready() {
            self.message = Some("AI unavailable (python, packages, or sign-in)".into());
            return;
        }
        let stat = cian_core::git::staged_stat(&dir).unwrap_or_default();
        // Keep the payload bounded: a huge diff would blow the token budget and
        // rarely improves the message. The stat line still names every file.
        let diff = truncate_diff_for_ai(&diff, 12_000);
        let system = "You write a git commit message for the given staged diff. \
             Use the Conventional Commits style: a concise subject line under ~70 \
             characters (an optional type prefix like feat:/fix:/refactor: is fine), \
             then a blank line and a short body of bullet points explaining WHY, \
             only if it adds something. Output ONLY the commit message — no code \
             fences, no preamble."
            .to_string();
        self.message = Some("asking AI to draft a commit message…".into());
        self.ai_request(AiPurpose::CommitMessage { dir, stat }, system, diff);
    }

    /// Ask the AI which entries in the active pane look like junk (build output,
    /// caches, temp/backup files, OS cruft), then show them for review. Only
    /// metadata (names, sizes, dir flags) leaves the machine — never contents.
    pub(crate) fn start_ai_junk(&mut self) {
        if self.ai.is_none() {
            self.message = Some("AI not configured — add cian.ai{...} to init.lua".into());
            return;
        }
        // Snapshot the listing up front so the immutable pane borrow is dropped
        // before `ai_ready()` (which needs &mut self).
        let Some(pane) = self.active_pane() else { return };
        let dir = pane.cwd.clone();
        // Skip the ".." entry; everything else is fair game.
        let rows: Vec<(String, PathBuf, bool, u64)> = pane
            .entries
            .iter()
            .filter(|e| !e.is_parent)
            .map(|e| (e.name.clone(), e.path.clone(), e.is_dir, e.len))
            .collect();
        if rows.is_empty() {
            self.message = Some("nothing here to scan".into());
            return;
        }
        if !self.ai_ready() {
            self.message = Some("AI unavailable (python, packages, or sign-in)".into());
            return;
        }
        // The name→path map used both to build the prompt and to validate the
        // reply back to real paths.
        let names: Vec<(String, PathBuf)> =
            rows.iter().map(|(n, p, _, _)| (n.clone(), p.clone())).collect();
        // A compact one-line-per-entry listing (name, kind, size).
        let mut listing = String::new();
        for (name, _, is_dir, len) in rows.iter().take(400) {
            let kind = if *is_dir { "dir " } else { "file" };
            let size = if *is_dir { String::new() } else { cian_core::human_size(*len) };
            listing.push_str(&format!("{}\t{}\t{}\n", kind, size, name));
        }
        let system = "You spot disposable JUNK in a directory listing: build output \
             (target, build, dist, node_modules, __pycache__, .gradle), caches, \
             logs, temp and editor-backup files (*.tmp, *.bak, *~, *.swp), and OS \
             cruft (.DS_Store, Thumbs.db, desktop.ini). Be CONSERVATIVE — never \
             flag source code, documents, configs, or anything whose loss would \
             hurt. Reply with ONLY a JSON array of objects {\"name\": string, \
             \"reason\": short string}, using names exactly as given. Empty array \
             if nothing is clearly junk. No prose, no code fences."
            .to_string();
        let user = format!("Directory: {}\n\nEntries (kind, size, name):\n{}", dir.display(), listing);
        self.message = Some("asking AI to find junk…".into());
        self.ai_request(AiPurpose::Junk { names }, system, user);
    }

    /// Ask the AI to propose an organised folder layout for the active pane,
    /// then show the moves for review. Metadata only (names, sizes, dir flags).
    pub(crate) fn start_ai_structure(&mut self) {
        if self.ai.is_none() {
            self.message = Some("AI not configured — add cian.ai{...} to init.lua".into());
            return;
        }
        let Some(pane) = self.active_pane() else { return };
        let dir = pane.cwd.clone();
        let rows: Vec<(String, PathBuf, bool, u64)> = pane
            .entries
            .iter()
            .filter(|e| !e.is_parent)
            .map(|e| (e.name.clone(), e.path.clone(), e.is_dir, e.len))
            .collect();
        if rows.is_empty() {
            self.message = Some("nothing here to organise".into());
            return;
        }
        if !self.ai_ready() {
            self.message = Some("AI unavailable (python, packages, or sign-in)".into());
            return;
        }
        let names: Vec<(String, PathBuf)> =
            rows.iter().map(|(n, p, _, _)| (n.clone(), p.clone())).collect();
        let mut listing = String::new();
        for (name, _, is_dir, len) in rows.iter().take(400) {
            let kind = if *is_dir { "dir " } else { "file" };
            let size = if *is_dir { String::new() } else { cian_core::human_size(*len) };
            listing.push_str(&format!("{}\t{}\t{}\n", kind, size, name));
        }
        let system = "You propose a tidy folder structure for a directory by \
             grouping loose files into sub-folders (e.g. images/, docs/, src/, \
             archive/2023/). Only MOVE existing entries into sub-folders — never \
             rename, never delete, never move a file out of this directory. Group \
             by obvious type or theme; leave a file where it is if no grouping is \
             clearly better (omit it). Prefer a few meaningful folders over many \
             tiny ones. Reply with ONLY a JSON array of objects {\"name\": string \
             (exactly as given), \"folder\": string (a NEW or existing sub-folder, \
             a simple relative path, no ..), \"reason\": short string}. Empty array \
             if the directory is already well organised. No prose, no code fences."
            .to_string();
        let user = format!("Directory: {}\n\nEntries (kind, size, name):\n{}", dir.display(), listing);
        self.message = Some("asking AI to suggest a structure…".into());
        self.ai_request(AiPurpose::Structure { names, dir }, system, user);
    }

    /// Run the checked moves from a structure suggestion on a worker: create
    /// each destination sub-folder (under the pane's directory) and move the
    /// file in. Skips on name conflict rather than overwriting.
    pub(crate) fn apply_structure_plan(&mut self) {
        let (dir, moves) = if let Popup::StructureReview { items, dir, .. } = &self.popup {
            let picked: Vec<(PathBuf, String)> = items
                .iter()
                .filter(|it| it.selected)
                .map(|it| (it.path.clone(), it.dest.clone()))
                .collect();
            (dir.clone(), picked)
        } else {
            return;
        };
        if moves.is_empty() {
            self.message = Some("nothing checked".into());
            return;
        }
        self.popup = Popup::None;
        self.start_op("organising", move |ctl| {
            let mut report = OpReport::default();
            let total = moves.len();
            for (i, (src, folder)) in moves.iter().enumerate() {
                if ctl.cancel.load(Ordering::Relaxed) {
                    break;
                }
                (ctl.on_progress)(&cian_core::progress::Progress {
                    files_done: i,
                    files_total: total,
                    current: src.display().to_string(),
                    ..Default::default()
                });
                let dest_dir = dir.join(folder);
                if let Err(e) = cian_core::ops::make_dir(&dir, folder, true) {
                    report.note_error(format!("{}: {}", folder, e));
                    continue;
                }
                match cian_core::ops::move_one(src, &dest_dir, Conflict::Skip) {
                    Ok(true) => report.ok += 1,
                    Ok(false) => report.skipped += 1,
                    Err(e) => report.note_error(format!("{}: {}", src.display(), e)),
                }
            }
            report
        });
    }

    /// Ask how to rename, then propose new names for the chosen files. The files
    /// are the marked ones, or the whole listing when nothing is marked.
    pub(crate) fn start_ai_rename_prompt(&mut self) {
        if self.ai.is_none() {
            self.message = Some("AI not configured — add cian.ai{...} to init.lua".into());
            return;
        }
        if !self.ai_ready() {
            self.message = Some("AI unavailable (python, packages, or sign-in)".into());
            return;
        }
        // Which files: marks if any, else every real entry in the listing.
        let any = self.active_pane().map(|p| {
            p.mark_count() > 0 || p.entries.iter().any(|e| !e.is_parent)
        }).unwrap_or(false);
        if !any {
            self.message = Some("nothing here to rename".into());
            return;
        }
        self.popup = text_input(
            "AI bulk rename",
            "how should these be renamed? (e.g. snake_case, add a date prefix):",
            String::new(),
            InputKind::AiRename,
        );
    }

    /// Send the chosen files' names plus the instruction to the model and show
    /// its proposed renames for review.
    pub(crate) fn start_ai_rename(&mut self, instruction: &str) {
        let instruction = instruction.trim().to_string();
        if instruction.is_empty() {
            self.message = Some("cancelled (no instruction)".into());
            return;
        }
        let Some(pane) = self.active_pane() else { return };
        // Marked files, or the whole listing (never the `..` row).
        let chosen: Vec<(String, PathBuf)> = if pane.mark_count() > 0 {
            pane.entries.iter()
                .filter(|e| !e.is_parent && pane.marks.contains(&e.path))
                .map(|e| (e.name.clone(), e.path.clone()))
                .collect()
        } else {
            pane.entries.iter()
                .filter(|e| !e.is_parent)
                .map(|e| (e.name.clone(), e.path.clone()))
                .collect()
        };
        if chosen.is_empty() {
            self.message = Some("nothing to rename".into());
            return;
        }
        let listing: String = chosen.iter().take(400).map(|(n, _)| format!("{}\n", n)).collect();
        let system = "You propose new file names following the user's instruction. \
             Keep it a RENAME only: never change the folder, never add a path. \
             Preserve the extension unless the instruction says otherwise. Reply \
             with ONLY a JSON array of objects {\"name\": string (exactly as \
             given), \"new_name\": string (a bare filename, no path)}. Include \
             only files that should change; omit the rest. No prose, no fences."
            .to_string();
        let user = format!("Instruction: {}\n\nFiles:\n{}", instruction, listing);
        let names = chosen;
        self.message = Some("asking AI for new names…".into());
        self.ai_request(AiPurpose::Rename { names }, system, user);
    }

    /// Prompt for a natural-language query, then semantic-search the tree.
    pub(crate) fn start_ai_search_prompt(&mut self) {
        if self.ai.is_none() {
            self.message = Some("AI not configured — add cian.ai{...} to init.lua".into());
            return;
        }
        if !self.ai_ready() {
            self.message = Some("AI unavailable (python, packages, or sign-in)".into());
            return;
        }
        self.popup = text_input(
            "AI semantic search",
            "describe what you're looking for:",
            String::new(),
            InputKind::AiSearch,
        );
    }

    /// Build a catalog of file paths under the active pane and ask the model
    /// which are most relevant to `query`. Metadata only — paths, not contents.
    pub(crate) fn start_ai_search(&mut self, query: &str) {
        let query = query.trim().to_string();
        if query.is_empty() {
            self.message = Some("cancelled (no query)".into());
            return;
        }
        let Some(root) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        // Collect up to a bounded number of file paths, breadth-first, stopping
        // early so a huge tree cannot stall the UI. Files only — the results
        // preview in F3, and a directory has nothing to preview.
        const CATALOG_CAP: usize = 600;
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let mut catalog: Vec<cian_core::search::Hit> = Vec::new();
        let q = cian_core::search::Query { needle: String::new(), include_hidden: false, mode: cian_core::search::Mode::Name };
        {
            let cancel = &cancel;
            let catalog = &mut catalog;
            cian_core::search::search(&root, &q, cancel, &mut |h| {
                if !h.is_dir {
                    catalog.push(h);
                    if catalog.len() >= CATALOG_CAP {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
            });
        }
        if catalog.is_empty() {
            self.message = Some("no files here to search".into());
            return;
        }
        let listing: String = catalog.iter()
            .map(|h| format!("{}\n", h.rel.display().to_string().replace('\\', "/")))
            .collect();
        let system = "You do semantic file search. Given a list of file paths and \
             a natural-language query, return the paths whose names/locations are \
             most relevant to the query, most relevant first. Reply with ONLY a \
             JSON array of objects {\"path\": string (exactly as given), \
             \"reason\": short string}. Use only paths from the list. Empty array \
             if none are a good match. No prose, no code fences."
            .to_string();
        let user = format!("Query: {}\n\nPaths:\n{}", query, listing);
        self.message = Some("asking AI to find relevant files…".into());
        self.ai_request(AiPurpose::SemSearch { hits: catalog }, system, user);
    }

    /// Run the checked renames in place, then reload and report.
    pub(crate) fn apply_rename_plan(&mut self) {
        let renames: Vec<(PathBuf, String)> = if let Popup::RenameReview { items, .. } = &self.popup {
            items.iter().filter(|it| it.selected).map(|it| (it.path.clone(), it.new.clone())).collect()
        } else {
            return;
        };
        if renames.is_empty() {
            self.message = Some("nothing checked".into());
            return;
        }
        self.popup = Popup::None;
        let mut report = OpReport::default();
        for (src, new) in &renames {
            // Skip if the target already exists, rather than clobbering it.
            if src.parent().map(|p| p.join(new).exists()).unwrap_or(false) {
                report.skipped += 1;
                continue;
            }
            match cian_core::ops::rename_in_place(src, new) {
                Ok(_) => report.ok += 1,
                Err(e) => report.note_error(format!("{}: {}", src.display(), e)),
            }
        }
        self.reload_active();
        self.flash(self.focused);
        self.show_op_report(&report);
    }

    /// Scan the active pane's tree for byte-identical files on a worker thread.
    pub(crate) fn start_dupes(&mut self) {
        if self.dupes_job.is_some() {
            self.message = Some("a duplicate scan is already running".into());
            return;
        }
        let Some(root) = self.active_pane().map(|p| p.cwd.clone()) else { return };
        // Collect files recursively, bounded so a giant tree cannot run away.
        const CAP: usize = 20_000;
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let mut files: Vec<PathBuf> = Vec::new();
        let q = cian_core::search::Query { needle: String::new(), include_hidden: false, mode: cian_core::search::Mode::Name };
        {
            let cancel = &cancel;
            let files = &mut files;
            cian_core::search::search(&root, &q, cancel, &mut |h| {
                if !h.is_dir {
                    files.push(h.path);
                    if files.len() >= CAP {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
            });
        }
        if files.len() < 2 {
            self.message = Some("nothing to compare".into());
            return;
        }
        self.message = Some(format!("scanning {} files for duplicates…", files.len()));
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let cancel = AtomicBool::new(false);
            let groups = cian_core::dedup::find_duplicates(&files, &cancel);
            let _ = tx.send(groups);
        });
        self.dupes_job = Some(rx);
    }

    /// Drain the duplicate scan; when it finishes, open the review popup.
    pub(crate) fn poll_dupes_job(&mut self) -> bool {
        let Some(rx) = &self.dupes_job else { return false };
        let groups = match rx.try_recv() {
            Ok(g) => g,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.dupes_job = None;
                return false;
            }
        };
        self.dupes_job = None;
        if groups.is_empty() {
            self.message = Some("no duplicate files found".into());
            return true;
        }
        // Flatten into rows: the first file of each group is the keeper (left
        // unchecked); the rest are pre-checked for deletion.
        let mut items = Vec::new();
        for (g, group) in groups.iter().enumerate() {
            for (i, path) in group.iter().enumerate() {
                let keeper = i == 0;
                items.push(DupeItem { path: path.clone(), group: g, keeper, selected: !keeper });
            }
        }
        let dupes = groups.len();
        self.message = Some(format!("{} duplicate group(s) — review and delete", dupes));
        self.popup = Popup::DupeReview { items, cursor: 0, scroll: 0 };
        true
    }

    /// Hand the checked duplicate copies to the normal delete confirmation.
    pub(crate) fn confirm_dupe_deletion(&mut self) {
        let targets: Vec<PathBuf> = if let Popup::DupeReview { items, .. } = &self.popup {
            items.iter().filter(|it| it.selected).map(|it| it.path.clone()).collect()
        } else {
            return;
        };
        if targets.is_empty() {
            self.message = Some("nothing checked".into());
            return;
        }
        self.popup = Popup::ConfirmDelete { targets };
    }

    /// Hand the checked junk candidates to the normal delete confirmation, so
    /// removal goes through the same trash/permanent path (and its own y/Enter
    /// approval) as any other delete — never straight to disk from here.
    pub(crate) fn confirm_junk_deletion(&mut self) {
        let targets: Vec<PathBuf> = if let Popup::JunkReview { items, .. } = &self.popup {
            items.iter().filter(|it| it.selected).map(|it| it.path.clone()).collect()
        } else {
            return;
        };
        if targets.is_empty() {
            self.message = Some("nothing checked".into());
            return;
        }
        self.popup = Popup::ConfirmDelete { targets };
    }

    /// Commit the staged changes with the (possibly edited) drafted message.
    pub(crate) fn commit_with_drafted_message(&mut self) {
        let (dir, message) = if let Popup::CommitMessage { dir, buffer, .. } = &self.popup {
            (dir.clone(), buffer.trim().to_string())
        } else {
            return;
        };
        if message.is_empty() {
            self.message = Some("empty message — nothing committed".into());
            return;
        }
        self.popup = Popup::None;
        match cian_core::git::commit(&dir, &message) {
            Ok(()) => {
                let subject = message.lines().next().unwrap_or("").to_string();
                self.message = Some(format!("✔ committed: {}", truncate(&subject, 60)));
                // The stage is now clean; refresh the markers.
                self.invalidate_git();
            }
            Err(e) => {
                self.popup = Popup::Notice {
                    lines: vec!["commit failed:".into(), String::new(), e.to_string()],
                };
            }
        }
    }

    /// The shell program's base name, for the command-generation prompt.
    pub(crate) fn shell_cmd_name(&self) -> String {
        std::path::Path::new(self.shell.command())
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "sh".into())
    }

    /// Copy the chat: the selected transcript lines if a range is selected,
    /// otherwise the whole of the last assistant reply.
    pub(crate) fn copy_ai_text(&mut self) {
        let text = if let Popup::AiChat { log, sel, .. } = &self.popup {
            match sel {
                Some((a, b)) => {
                    let lo = (*a).min(*b);
                    let hi = (*a).max(*b).min(self.ai_lines.len().saturating_sub(1));
                    if self.ai_lines.is_empty() {
                        String::new()
                    } else {
                        self.ai_lines[lo..=hi].join("\n")
                    }
                }
                None => log.iter().rev().find(|m| !m.user).map(|m| m.text.clone()).unwrap_or_default(),
            }
        } else {
            return;
        };
        if text.trim().is_empty() {
            self.message = Some("nothing to copy".into());
            return;
        }
        if let Some(cb) = self.clipboard.as_mut() {
            let _ = cb.set_text(text);
        }
        self.message = Some("copied".into());
        if let Popup::AiChat { sel, .. } = &mut self.popup {
            *sel = None;
        }
    }

    /// Drain the AI worker and route the reply by its purpose.
    pub(crate) fn poll_ai_job(&mut self) -> bool {
        let Some(job) = &self.ai_job else { return false };
        let result = match job.rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("AI worker died".to_string()),
        };
        let purpose = self.ai_job.take().map(|j| j.purpose).unwrap_or(AiPurpose::Chat);
        match purpose {
            AiPurpose::Chat => {
                if let Popup::AiChat { log, pending, scroll, .. } = &mut self.popup {
                    *pending = false;
                    *scroll = usize::MAX;
                    match result {
                        Ok(text) => log.push(ChatMsg { user: false, text }),
                        Err(e) => log.push(ChatMsg { user: false, text: format!("[error] {}", e) }),
                    }
                }
            }
            AiPurpose::ShellCommand => match result {
                Ok(text) => {
                    let command = clean_ai_command(&text);
                    if command.is_empty() {
                        self.message = Some("AI returned no command".into());
                    } else {
                        self.popup = Popup::AiShellConfirm { command };
                    }
                }
                Err(e) => self.message = Some(format!("AI: {}", e)),
            },
            AiPurpose::CommitMessage { dir, stat } => match result {
                Ok(text) => {
                    let msg = clean_ai_commit_message(&text);
                    if msg.is_empty() {
                        self.message = Some("AI returned no message".into());
                    } else {
                        self.popup = Popup::CommitMessage { buffer: msg, stat, dir, editing: false };
                    }
                }
                Err(e) => self.message = Some(format!("AI: {}", e)),
            },
            AiPurpose::Junk { names, .. } => match result {
                Ok(text) => {
                    let items = parse_junk_reply(&text, &names);
                    if items.is_empty() {
                        self.message = Some("AI found no obvious junk".into());
                    } else {
                        self.popup = Popup::JunkReview { items, cursor: 0, scroll: 0 };
                    }
                }
                Err(e) => self.message = Some(format!("AI: {}", e)),
            },
            AiPurpose::Structure { names, dir } => match result {
                Ok(text) => {
                    let items = parse_structure_reply(&text, &names);
                    if items.is_empty() {
                        self.message = Some("AI had no structure changes to suggest".into());
                    } else {
                        self.popup = Popup::StructureReview { items, cursor: 0, scroll: 0, dir };
                    }
                }
                Err(e) => self.message = Some(format!("AI: {}", e)),
            },
            AiPurpose::Rename { names } => match result {
                Ok(text) => {
                    let items = parse_rename_reply(&text, &names);
                    if items.is_empty() {
                        self.message = Some("AI proposed no renames".into());
                    } else {
                        self.popup = Popup::RenameReview { items, cursor: 0, scroll: 0 };
                    }
                }
                Err(e) => self.message = Some(format!("AI: {}", e)),
            },
            AiPurpose::SemSearch { hits } => match result {
                Ok(text) => {
                    let matched = parse_sem_search_reply(&text, &hits);
                    if matched.is_empty() {
                        self.message = Some("AI found no relevant files".into());
                    } else {
                        // Reuse the find-results list: F3 preview, Ctrl+n/N, Esc.
                        self.find_return = None;
                        self.message = Some(format!("{} relevant file(s) — Enter previews", matched.len()));
                        self.popup = Popup::FindResults { hits: matched, cursor: 0, scroll: 0 };
                    }
                }
                Err(e) => self.message = Some(format!("AI: {}", e)),
            },
        }
        true
    }
}

#[cfg(test)]
mod ai_history_tests {
    use super::*;

    #[test]
    fn stored_chats_round_trip_mode_and_log() {
        let history: Vec<(ChatMode, Vec<ChatMsg>)> = vec![
            (
                ChatMode::Rag,
                vec![
                    ChatMsg { user: true, text: "q1".into() },
                    ChatMsg { user: false, text: "a1\nline".into() },
                ],
            ),
            (ChatMode::Agent, vec![ChatMsg { user: true, text: "q2".into() }]),
        ];
        // Serialize exactly as save_ai_history does, then read back as restore does.
        let stored: Vec<StoredChat> =
            history.iter().map(|(m, l)| StoredChat { mode: *m, log: l.clone() }).collect();
        let json = serde_json::to_string(&stored).unwrap();
        let back: Vec<(ChatMode, Vec<ChatMsg>)> = serde_json::from_str::<Vec<StoredChat>>(&json)
            .unwrap()
            .into_iter()
            .map(|c| (c.mode, c.log))
            .collect();
        assert_eq!(back, history, "mode and transcript survive a round trip");
    }
}
