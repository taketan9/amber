//! Running a layout macro: build a set of shell panes over several ticks.
//!
//! Splits spawn PTYs on a background thread (see [`crate::panes`]), so a macro
//! cannot build its layout in one shot — it would race the spawns. Instead it
//! runs as a tiny state machine advanced from the main loop once per tick:
//! wait for the shell to be idle, apply the current pane (colour, log, the
//! command to run), then split off the next one and wait again. The result is
//! that "split, ssh here, split, ssh there, start logging" collapses to one key.

use std::collections::VecDeque;

use cian_lua::macros::{Macro, PaneStep, Split};

use crate::{theme, tr, App, FocusedPane, Popup, SplitDir};

/// A layout macro in the middle of building itself.
pub(crate) struct MacroRun {
    /// For the "macro done" message.
    name: String,
    /// Panes not yet started. The very first applies to the current pane; each
    /// later one is split off the previous.
    queue: VecDeque<PaneStep>,
    /// A pane that now exists (its spawn has landed) and still needs its colour,
    /// log and command applied.
    apply: Option<PaneStep>,
    /// True until the first pane has been consumed (it is the shell pane you are
    /// on, so it is applied without a split).
    first: bool,
}

/// Load macros (portable-aware): first `macro.lua` (a list of macros), then
/// each `macro/*.lua` file (one macro — or a list — per file), sorted by
/// filename. Returns the macros and, separately, any parse errors so the
/// launcher can explain a short or empty list.
pub(crate) fn load_macros() -> (Vec<Macro>, Option<String>) {
    let mut macros = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    if let Some(path) = cian_lua::config_read_path("macro.lua").filter(|p| p.exists()) {
        match cian_lua::macros::load(&path) {
            Ok(mut m) => macros.append(&mut m),
            Err(e) => errors.push(format!("macro.lua: {}", e)),
        }
    }

    // One file per macro, e.g. macro/Adeploy.lua, macro/Bdbcheck.lua.
    if let Some(dir) = cian_lua::config_read_path("macro").filter(|p| p.is_dir()) {
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("lua"))
            .collect();
        files.sort();
        for f in files {
            let label = f.file_name().and_then(|n| n.to_str()).unwrap_or("macro").to_string();
            match cian_lua::macros::load(&f) {
                Ok(mut m) => macros.append(&mut m),
                Err(e) => errors.push(format!("{}: {}", label, e)),
            }
        }
    }

    let error = if errors.is_empty() { None } else { Some(errors.join("; ")) };
    (macros, error)
}

impl App {
    /// Open the macro launcher, or explain why there is nothing to launch.
    pub(crate) fn start_macros(&mut self) {
        if self.macros.is_empty() {
            self.message = Some(match &self.macro_error {
                Some(e) => e.clone(),
                None => tr(
                    self.lang,
                    "no macros — define them in macro.lua",
                    "マクロがありません — macro.lua で定義してください",
                )
                .to_string(),
            });
            return;
        }
        self.popup = Popup::Macros { cursor: 0, names: self.macro_names() };
    }

    /// Begin running macro `idx` from the loaded set. The build proceeds in
    /// [`App::tick_macro`].
    pub(crate) fn run_macro(&mut self, idx: usize) {
        let Some(m) = self.macros.get(idx).cloned() else { return };
        self.popup = Popup::None;
        self.begin_macro(&m);
    }

    /// Start running macro named `name` (from the loaded set). Returns false if
    /// there is no such macro — used by the `--macro-name` startup option.
    pub(crate) fn start_macro_by_name(&mut self, name: &str) -> bool {
        let Some(m) = self.macros.iter().find(|m| m.name == name).cloned() else {
            return false;
        };
        self.begin_macro(&m);
        true
    }

    /// Kick off building `m`'s layout, focusing the shell it builds into.
    pub(crate) fn begin_macro(&mut self, m: &Macro) {
        self.macro_run = Some(MacroRun {
            name: m.name.clone(),
            queue: m.panes.iter().cloned().collect(),
            apply: None,
            first: true,
        });
        self.focus(FocusedPane::Shell);
        self.message = Some(tr(self.lang, "running macro: ", "マクロ実行中: ").to_string() + &m.name);
    }

    /// Advance a running macro by one step. Returns true if anything changed
    /// (so the caller repaints). Called each tick from the main loop.
    pub(crate) fn tick_macro(&mut self) -> bool {
        if self.macro_run.is_none() {
            return false;
        }
        // Wait for calm: a spawn in flight means the previous split has not
        // landed yet, so its pane is not there to apply to or split from.
        if self.shell.busy() {
            return false;
        }
        let cwd = self.shell_cwd();
        self.shell.ensure(&cwd);
        if self.shell.active_session().is_none() {
            return true; // the first shell is still starting; try again next tick
        }

        // Apply a pane whose spawn has landed.
        if let Some(step) = self.macro_run.as_mut().and_then(|r| r.apply.take()) {
            self.apply_macro_step(&step);
        }

        // Start the next pane, or finish.
        let next = self.macro_run.as_mut().and_then(|r| r.queue.pop_front());
        match next {
            Some(step) => {
                let first = self.macro_run.as_ref().map(|r| r.first).unwrap_or(false);
                if let Some(r) = self.macro_run.as_mut() {
                    r.first = false;
                }
                if !first {
                    // Split off the previous pane; the new pane becomes active
                    // when the spawn lands, and we apply to it then.
                    let dir = match step.dir {
                        Split::Right => SplitDir::LeftRight,
                        Split::Down => SplitDir::TopBottom,
                    };
                    self.shell.split_active(&cwd, dir);
                }
                if let Some(r) = self.macro_run.as_mut() {
                    r.apply = Some(step);
                }
            }
            None => {
                // Nothing left to start; once the last apply is done we are through.
                if self.macro_run.as_ref().map(|r| r.apply.is_none()).unwrap_or(true) {
                    if let Some(r) = self.macro_run.take() {
                        self.message =
                            Some(tr(self.lang, "macro done: ", "マクロ完了: ").to_string() + &r.name);
                    }
                }
            }
        }
        true
    }

    /// Colour, log and run commands for the pane that is now active.
    fn apply_macro_step(&mut self, step: &PaneStep) {
        if let Some(spec) = &step.bg {
            if let Some(c) = theme::parse_color(spec) {
                self.shell.set_active_pane_bg(Some(c));
            }
        }
        if let Some(dir) = &step.log {
            self.start_session_log(dir);
        }
        if let Some(cmd) = &step.cmd {
            self.type_line_in_active(cmd);
        }
        for line in &step.steps {
            self.type_line_in_active(line);
        }
    }

    /// Type one line into the active shell pane and press Enter.
    fn type_line_in_active(&mut self, line: &str) {
        if let Some(s) = self.shell.active_session_mut() {
            let mut bytes = line.as_bytes().to_vec();
            bytes.push(b'\n');
            s.write_input(&bytes);
        }
    }

    /// The macros' names, for the launcher list.
    pub(crate) fn macro_names(&self) -> Vec<String> {
        self.macros.iter().map(|m| m.name.clone()).collect()
    }
}
