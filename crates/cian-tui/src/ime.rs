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
                let _ = shell_command(&s.cmd).output();
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
