//! Running a layout macro: build a set of shell panes over several ticks.
//!
//! Splits spawn PTYs on a background thread (see [`crate::panes`]), so a macro
//! cannot build its layout in one shot — it would race the spawns. Instead it
//! runs as a tiny state machine advanced from the main loop once per tick:
//! wait for the shell to be idle, apply the current pane (colour, log, the
//! command to run), then split off the next one and wait again. The result is
//! that "split, ssh here, split, ssh there, start logging" collapses to one key.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use cian_lua::macros::{Macro, PaneStep, Split, Step};

use crate::{tr, App, FocusedPane, Popup, SplitDir};

/// Nobody wants a mistyped `wait = 100000` to wedge cian; cap any single pause
/// or prompt-wait here.
const MAX_WAIT_SECS: f64 = 600.0;

/// A macro in the middle of building itself.
pub(crate) struct MacroRun {
    /// For the "macro done" message.
    name: String,
    /// Panes not yet started. The very first applies to the current pane; each
    /// later one is split off the previous.
    queue: VecDeque<PaneStep>,
    /// A pane that now exists (its spawn has landed) and still needs its colour,
    /// log and scripted steps applied.
    apply: Option<PaneStep>,
    /// True until the first pane has been consumed (it is the shell pane you are
    /// on, so it is applied without a split).
    first: bool,
    /// The current pane's remaining scripted steps (send / wait / expect).
    steps: VecDeque<Step>,
    /// When a timed `wait` ends, if one is in progress.
    wait_until: Option<Instant>,
    /// An `expect` in progress: the (lower-cased) text to watch for and the
    /// deadline after which we give up and move on.
    expect: Option<(String, Instant)>,
    /// Turn on input broadcast once the whole layout is built.
    sync: bool,
    /// The shell-leaf id of each pane created so far, in `panes` order, so a
    /// later pane's `from = N` can split off pane N rather than the previous.
    leaf_ids: Vec<usize>,
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
            steps: VecDeque::new(),
            wait_until: None,
            expect: None,
            sync: m.sync,
            leaf_ids: Vec::new(),
        });
        self.focus(FocusedPane::Shell);
        // Maximize the shell panel so a grid has the whole window.
        if m.zoom && !self.zoomed {
            self.toggle_zoom();
        }
        self.message = Some(tr(self.lang, "running macro: ", "マクロ実行中: ").to_string() + &m.name);
    }

    /// Advance a running macro. Returns true if anything changed (so the caller
    /// repaints). Called each tick from the main loop. It processes one thing
    /// per tick — apply a landed pane, run/observe one scripted step, or start
    /// the next pane — so waits and prompt-waits are honoured without blocking.
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

        // Own the runner for the tick so we can freely call other `self` methods.
        let mut run = self.macro_run.take().unwrap();

        // 1. A pane whose spawn just landed: colour/log it and load its steps.
        if let Some(step) = run.apply.take() {
            self.apply_pane(&step, &mut run);
        }

        // 2. A timed pause still in progress.
        if let Some(t) = run.wait_until {
            if Instant::now() < t {
                self.macro_run = Some(run);
                return true;
            }
            run.wait_until = None;
        }

        // 3. Waiting for a prompt/text to appear.
        if let Some((needle, deadline)) = run.expect.clone() {
            if self.shell_screen_has(&needle) {
                run.expect = None;
            } else if Instant::now() >= deadline {
                run.expect = None;
                self.message = Some(format!("macro: gave up waiting for {:?}", needle));
            } else {
                self.macro_run = Some(run);
                return true;
            }
        }

        // 4. Run the current pane's next scripted step.
        if let Some(step) = run.steps.pop_front() {
            match step {
                Step::Send(line) => self.type_line_in_active(&line),
                Step::Wait(secs) => {
                    run.wait_until = Some(Instant::now() + secs_to_dur(secs));
                }
                Step::Expect { text, timeout } => {
                    run.expect = Some((text.to_lowercase(), Instant::now() + secs_to_dur(timeout)));
                }
            }
            self.macro_run = Some(run);
            return true;
        }

        // 5. Current pane finished; start the next one, or we are done.
        match run.queue.pop_front() {
            Some(next) => {
                let first = run.first;
                run.first = false;
                if !first {
                    // `from = N` (1-based) splits pane N rather than the previous
                    // one — the key to a real grid. Otherwise split off the pane
                    // built just before. Target the leaf explicitly so the async
                    // split lands on it even though `active` may move meanwhile.
                    let target = next
                        .from
                        .and_then(|n| run.leaf_ids.get(n.saturating_sub(1)))
                        .or_else(|| run.leaf_ids.last())
                        .copied();
                    let dir = match next.dir {
                        Split::Right => SplitDir::LeftRight,
                        Split::Down => SplitDir::TopBottom,
                    };
                    match target {
                        Some(leaf) => self.shell.split_leaf(&cwd, leaf, dir),
                        None => self.shell.split_active(&cwd, dir),
                    }
                }
                run.apply = Some(next);
                self.macro_run = Some(run);
            }
            None => {
                // The layout is built. Turn on input broadcast if asked, so the
                // same keystrokes now reach every pane the macro made.
                if run.sync {
                    self.shell.set_broadcast(true);
                }
                self.message =
                    Some(tr(self.lang, "macro done: ", "マクロ完了: ").to_string() + &run.name);
                // Drop `run`: the macro is finished.
            }
        }
        true
    }

    /// Colour and log the now-active pane, remember its leaf id (so a later
    /// `from` can target it), and load its command + scripted steps.
    fn apply_pane(&mut self, step: &PaneStep, run: &mut MacroRun) {
        // Record this pane's leaf id in creation order.
        if let Some(id) = self.shell.active_leaf_id() {
            run.leaf_ids.push(id);
        }
        if let Some(spec) = &step.bg {
            if let Some(c) = crate::resolve_bg(spec) {
                self.shell.set_active_pane_bg(Some(c));
            }
        }
        if let Some(dir) = &step.log {
            self.start_session_log(dir);
        }
        run.steps.clear();
        if let Some(cmd) = &step.cmd {
            run.steps.push_back(Step::Send(cmd.clone()));
        }
        run.steps.extend(step.steps.iter().cloned());
    }

    /// Does the active shell pane's visible screen contain `needle` (already
    /// lower-cased)? Used by `expect` to wait for a prompt.
    fn shell_screen_has(&self, needle: &str) -> bool {
        self.shell
            .active_session()
            .and_then(|s| s.parser().lock().ok().map(|p| p.screen().contents()))
            .map(|c| c.to_lowercase().contains(needle))
            .unwrap_or(false)
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

/// A non-negative, capped duration from a seconds value.
fn secs_to_dur(secs: f64) -> Duration {
    Duration::from_secs_f64(secs.clamp(0.0, MAX_WAIT_SECS))
}
