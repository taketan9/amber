//! Switching the system's input method with cian's mode.
//!
//! A Japanese IME sits between the keyboard and the terminal: while it is
//! composing, a letter never reaches cian at all, so `j`, `d`, `y` — every
//! single-key command — are dead until it is switched off by hand. Nothing
//! cian does to the keys it receives can fix that, because it does not receive
//! them.
//!
//! What cian can do is ask for the input method to be off while it is being
//! *driven*, and on again the moment it is being *typed into*: the `:` line, a
//! search, a rename, the chat, the shell. That is the same trick vim users run
//! on macOS, and it needs the same kind of helper — a small program that can
//! switch the input source. cian runs whatever command `cian.ime{}` names:
//!
//! ```lua
//! cian.ime{                                    -- macOS, with `brew install macism`
//!   off = "macism com.apple.keylayout.ABC",
//!   on  = "macism com.apple.inputmethod.Kotoeri.RomajiTyping.Japanese",
//! }
//! ```
//!
//! Commands run on one worker thread, in the order they were asked for — the
//! switch is not worth a frame of lag, and a helper that is missing or slow
//! must never wedge the keyboard.

use super::*;

impl App {
    /// Is cian taking text right now, rather than being driven by commands?
    ///
    /// This is the whole rule. Everything that reads a typed string says yes;
    /// the file panes in normal mode and the viewer being read say no.
    pub(crate) fn wants_text_input(&self) -> bool {
        match &self.popup {
            Popup::None => {
                matches!(self.mode, Mode::Command | Mode::Filter | Mode::Search)
                    || self.focused == FocusedPane::Shell
            }
            // The viewer is vim: reading is commands, and only the prompts and
            // the editor take text.
            Popup::Viewer { find_input, sub_input, block_input, editing, .. } => {
                find_input.is_some()
                    || sub_input.is_some()
                    || block_input.is_some()
                    || *editing
            }
            Popup::TextInput { .. }
            | Popup::Search { .. }
            | Popup::Palette { .. }
            | Popup::AiChat { .. }
            | Popup::CommitMessage { .. }
            | Popup::Snippets { .. }
            | Popup::GrepReplace(_) => true,
            // Everything else is a list to steer or a question to answer with
            // one key.
            _ => false,
        }
    }

    /// Put the input method where this moment wants it. Cheap to call — it
    /// compares one bool and does nothing until the answer changes — so it
    /// runs once per turn round the event loop rather than being remembered
    /// at every place that opens a prompt.
    pub(crate) fn sync_ime(&mut self) {
        let Some(cfg) = self.config.ime.clone() else { return };
        // Say so when a switch failed. It happens on a worker thread, so this
        // is where the news is collected — once per failure.
        if let Ok(mut slot) = last_switch().lock() {
            if let Some(log) = slot.as_mut() {
                if !log.reported {
                    log.reported = true;
                    if let Some(e) = &log.error {
                        self.message = Some(format!("ime: {} — {e}", log.cmd));
                    }
                }
            }
        }
        let want = self.wants_text_input();
        if self.ime_on == Some(want) {
            return;
        }
        self.ime_on = Some(want);
        let cmd = if want { cfg.on } else { cfg.off };
        if let Some(c) = cmd {
            run_switch(&c, None);
        }
    }

    /// Hand the keyboard back on the way out: someone who set this up types
    /// Japanese, and leaving their input method off because cian turned it off
    /// is cian's mess to clean up.
    pub(crate) fn release_ime(&self) {
        let Some(cfg) = &self.config.ime else { return };
        if !cfg.restore || self.ime_on != Some(false) {
            return;
        }
        if let Some(c) = &cfg.on {
            // Through the same queue, so anything still in flight lands first
            // — then waited for, briefly: the process is about to end, and a
            // switch that never ran is worse than a few milliseconds at exit.
            let (tx, rx) = std::sync::mpsc::channel();
            run_switch(c, Some(tx));
            let _ = rx.recv_timeout(std::time::Duration::from_secs(3));
        }
    }

    /// `:ime` — what is configured, and which way the switch is currently
    /// thrown. The first thing to look at when the keyboard misbehaves.
    pub(crate) fn ime_report(&mut self, arg: &str) {
        let Some(cfg) = self.config.ime.clone() else {
            self.popup = Popup::Notice {
                lines: vec![
                    tr(self.lang, "input method — not configured", "日本語入力の切替 — 未設定").to_string(),
                    String::new(),
                    tr(
                        self.lang,
                        "Add cian.ime{ off = \"…\", on = \"…\" } to init.lua. The commands are",
                        "init.lua に cian.ime{ off = \"…\", on = \"…\" } を書いてください。コマンドは",
                    )
                    .to_string(),
                    tr(
                        self.lang,
                        "whatever switches the input source on this machine — macism or",
                        "この環境で入力ソースを切り替えるものなら何でも構いません — macOS なら",
                    )
                    .to_string(),
                    tr(
                        self.lang,
                        "im-select on macOS, zenhan or im-select on Windows.",
                        "macism / im-select、Windows なら zenhan / im-select。",
                    )
                    .to_string(),
                ],
            };
            return;
        };
        // `:ime on` / `:ime off` run the command now, so a helper can be tested
        // without hunting for the mode that would trigger it.
        match arg.trim() {
            "on" | "off" => {
                let on = arg.trim() == "on";
                let cmd = if on { cfg.on.clone() } else { cfg.off.clone() };
                match cmd {
                    Some(c) => {
                        self.ime_on = Some(on);
                        let out = shell_command(&c).output();
                        self.message = Some(match out {
                            Ok(o) if o.status.success() => format!("ime: {c}"),
                            Ok(o) => format!(
                                "ime: {c} — {}",
                                String::from_utf8_lossy(&o.stderr).trim().chars().take(80).collect::<String>()
                            ),
                            Err(e) => format!("ime: {c} — {e}"),
                        });
                    }
                    None => self.message = Some(format!("ime: no {arg} command configured")),
                }
                return;
            }
            _ => {}
        }
        // What the last switch actually did — the answer to "it is configured
        // and nothing happened".
        let last = match last_switch().lock().ok().and_then(|s| {
            s.as_ref().map(|l| (l.cmd.clone(), l.error.clone()))
        }) {
            Some((cmd, None)) => format!("{cmd}  ✔"),
            Some((cmd, Some(e))) => format!("{cmd}  ⚠ {e}"),
            None => tr(self.lang, "(none run yet)", "（まだ実行されていません）").to_string(),
        };
        let state = match self.ime_on {
            Some(true) => tr(self.lang, "on (cian is taking text)", "オン（入力中）"),
            Some(false) => tr(self.lang, "off (cian is being driven)", "オフ（操作中）"),
            None => tr(self.lang, "not set yet", "未適用"),
        };
        let lines = vec![
            tr(self.lang, "input method switching", "日本語入力の自動切替").to_string(),
            String::new(),
            format!("{:<8}: {}", "state", state),
            format!("{:<8}: {}", "off", cfg.off.clone().unwrap_or_else(|| "—".into())),
            format!("{:<8}: {}", "on", cfg.on.clone().unwrap_or_else(|| "—".into())),
            format!("{:<8}: {}", "restore", cfg.restore),
            format!("{:<8}: {}", "last", last),
            String::new(),
            tr(
                self.lang,
                ":ime on / :ime off runs the command now, to check the helper works.",
                ":ime on / :ime off でコマンドを即実行して動作確認できます。",
            )
            .to_string(),
            tr(
                self.lang,
                "Letters typed while the IME composes never reach cian, so this is",
                "IME が変換中の英字は cian に届きません。単キーのコマンドを効かせる",
            )
            .to_string(),
            tr(
                self.lang,
                "the only way single-key commands can work with Japanese input on.",
                "方法はこの切替だけです。",
            )
            .to_string(),
        ];
        self.popup = Popup::Notice { lines };
    }
}

/// A command line as the platform's shell would run it, so the config can hold
/// exactly what the user would type.
fn shell_command(cmd: &str) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    }
}

/// What the last switch cian ran did — the command, whether it worked, and
/// what it said if it did not.
///
/// A switch runs on a worker thread, so its failure has nowhere to be seen at
/// the moment it happens. Without this, a helper that is missing or renamed is
/// perfectly silent: cian goes on believing the input method is where it asked
/// for it to be, and the only symptom is that Japanese input is still on. Kept
/// here rather than on `App` because the thread that learns it has no `App`.
struct SwitchLog {
    cmd: String,
    error: Option<String>,
    /// Cleared once the failure has been put on screen, so it is said once.
    reported: bool,
}

fn last_switch() -> &'static std::sync::Mutex<Option<SwitchLog>> {
    static L: std::sync::OnceLock<std::sync::Mutex<Option<SwitchLog>>> = std::sync::OnceLock::new();
    L.get_or_init(|| std::sync::Mutex::new(None))
}

/// Switches are queued to one worker thread rather than each getting its own.
///
/// Order is the reason. Two switches thrown quickly — off, then on — must land
/// in that order or the keyboard ends up in the state the *older* command
/// asked for, and the last thing cian does on the way out is exactly such a
/// pair. One consumer, one queue, submission order preserved.
struct Switch {
    cmd: String,
    /// Dropped once the command has finished, so a caller can wait for it.
    done: Option<std::sync::mpsc::Sender<()>>,
}

fn switch_queue() -> &'static std::sync::mpsc::Sender<Switch> {
    static Q: std::sync::OnceLock<std::sync::mpsc::Sender<Switch>> = std::sync::OnceLock::new();
    Q.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Switch>();
        std::thread::spawn(move || {
            for s in rx {
                let error = match shell_command(&s.cmd).output() {
                    Ok(o) if o.status.success() => None,
                    Ok(o) => {
                        let said = String::from_utf8_lossy(&o.stderr);
                        let said = said.trim();
                        Some(if said.is_empty() {
                            format!("exit {}", o.status.code().unwrap_or(-1))
                        } else {
                            said.chars().take(120).collect()
                        })
                    }
                    Err(e) => Some(e.to_string()),
                };
                if let Ok(mut slot) = last_switch().lock() {
                    *slot = Some(SwitchLog { cmd: s.cmd.clone(), error, reported: false });
                }
                drop(s.done);
            }
        });
        tx
    })
}

/// Queue a switch, off the UI thread. `ack` (when given) is signalled — by
/// being dropped — once this command, and everything queued before it, is
/// done.
fn run_switch(cmd: &str, ack: Option<std::sync::mpsc::Sender<()>>) {
    let _ = switch_queue().send(Switch { cmd: cmd.to_string(), done: ack });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The queue and its last-result slot are process-wide — one keyboard, one
    /// switch — so the tests that watch them take turns.
    static SWITCH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The rule, stated once: a keystroke is either text or a command, and the
    /// input method follows that and nothing else.
    #[test]
    fn text_and_command_states_are_told_apart() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        assert!(!app.wants_text_input(), "the panes in normal mode are commands");
        app.mode = Mode::Command;
        assert!(app.wants_text_input(), "the `:` line is text");
        app.mode = Mode::Filter;
        assert!(app.wants_text_input(), "so is a filter");
        app.mode = Mode::Normal;
        app.focused = FocusedPane::Shell;
        assert!(app.wants_text_input(), "and so is the shell");
        app.focused = FocusedPane::Left;

        app.popup = Popup::Notice { lines: Vec::new() };
        assert!(!app.wants_text_input(), "a notice is answered with one key");
        app.start_rename();
        assert!(app.wants_text_input(), "a rename is text");
    }

    /// A helper that is missing, renamed or broken must say so. Before this,
    /// the only symptom of a bad `cian.ime{}` was that nothing happened — the
    /// switch failed on a worker thread with no one listening.
    #[cfg(unix)]
    #[test]
    fn a_failed_switch_is_reported_once() {
        let _g = SWITCH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.ime = Some(cian_lua::ImeOptions {
            off: Some("exit 3".into()),
            on: None,
            restore: false,
        });
        let mut app = App::new(p.clone(), p, config).unwrap();
        app.sync_ime(); // queues the failing "off"
        // The failure lands on the worker thread; the next sync collects it.
        let mut said = None;
        for _ in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            app.sync_ime();
            if let Some(m) = app.message.clone() {
                said = Some(m);
                break;
            }
        }
        let said = said.expect("a failed switch is reported");
        assert!(said.contains("ime:") && said.contains("exit 3"), "{said}");
        // …and only once: the next sync does not repeat it.
        app.message = None;
        app.sync_ime();
        assert!(app.message.is_none(), "a failure is said once, not every frame");
    }

    /// The switch is thrown on the way into text and on the way out, and not
    /// otherwise — a helper run on every keystroke would be a subprocess per
    /// keystroke.
    #[test]
    fn the_switch_moves_only_when_the_answer_changes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        // No commands: this is about the decision, not about running anything.
        config.ime = Some(cian_lua::ImeOptions { off: None, on: None, restore: true });
        let mut app = App::new(p.clone(), p, config).unwrap();

        assert_eq!(app.ime_on, None, "nothing decided before the first sync");
        app.sync_ime();
        assert_eq!(app.ime_on, Some(false), "driving cian — input method off");
        app.sync_ime();
        assert_eq!(app.ime_on, Some(false), "still off, and nothing to re-run");

        app.mode = Mode::Command;
        app.sync_ime();
        assert_eq!(app.ime_on, Some(true), "the `:` line takes text");
        app.mode = Mode::Normal;
        app.sync_ime();
        assert_eq!(app.ime_on, Some(false), "and back off on the way out");
    }

    /// The configured command really is run. Nothing here needs `macism` — the
    /// point is that whatever is configured gets executed, so the command is a
    /// shell line that leaves a file behind.
    #[cfg(unix)]
    #[test]
    fn the_configured_command_actually_runs() {
        let _g = SWITCH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("switched");
        let p = dir.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.ime = Some(cian_lua::ImeOptions {
            off: Some(format!("echo off >> {}", marker.display())),
            on: Some(format!("echo on >> {}", marker.display())),
            restore: true,
        });
        let mut app = App::new(p.clone(), p, config).unwrap();

        app.sync_ime(); // off
        app.mode = Mode::Command;
        app.sync_ime(); // on
        // The commands run on their own threads; wait for them, briefly.
        let read = || std::fs::read_to_string(&marker).unwrap_or_default();
        for _ in 0..100 {
            if read().lines().count() >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let got: Vec<String> = read().lines().map(str::to_string).collect();
        assert_eq!(got, ["off", "on"], "both switches ran, in order");

        // Exiting hands the keyboard back.
        app.mode = Mode::Normal;
        app.sync_ime(); // off again
        app.release_ime();
        assert_eq!(read().lines().last(), Some("on"), "the way out turns it back on");
    }
}
