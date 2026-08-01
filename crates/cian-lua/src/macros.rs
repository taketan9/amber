//! User-defined macros (`macro.lua`) — read-only, loaded like `init.lua`.
//!
//! The first kind of macro is a **layout**: build a set of shell panes in one
//! go, each optionally connecting somewhere, tinted a colour so the panes are
//! told apart, and logging to a file. It folds a repetitive "split, ssh here,
//! split, ssh there, start logging" ritual into one keystroke.
//!
//! ```lua
//! return {
//!   { name = "Prod session", panes = {
//!     { cmd = "ssh admin@db",  bg = "#402018", log = "~/logs" },
//!     { dir = "right", cmd = "ssh admin@app" },
//!     { dir = "down",  steps = { "sqlplus /nolog", "connect user/pw@db" } },
//!   }},
//! }
//! ```
//!
//! The first pane is the shell pane you are on; each later pane is split off the
//! previous one in its `dir` ("right" = side by side, "down" = stacked).

use std::path::Path;

use mlua::{Lua, Table, Value};

/// One macro. Usually a **layout** — a name and the shell panes it builds — but
/// a macro whose table carries a `run` function is a **script macro** instead
/// (see [`crate::macro_script`]): `panes` is then empty and `script` holds the
/// macro file's source, to be re-evaluated and run on demand.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Macro {
    pub name: String,
    pub panes: Vec<PaneStep>,
    /// Turn on input broadcast (synchronize) across the built panes once the
    /// layout is up — for "run the same command on every server" setups.
    pub sync: bool,
    /// Maximize the shell panel (as F12 does) before building the layout, so a
    /// multi-pane grid has the whole window.
    pub zoom: bool,
    /// Set for a **script macro**: the source of the file it was defined in, so
    /// the caller can re-evaluate it and invoke this macro's `run` function.
    pub script: Option<String>,
}

impl Macro {
    /// True when this is a file-operation script macro rather than a layout.
    pub fn is_script(&self) -> bool {
        self.script.is_some()
    }
}

/// One pane in a layout macro.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneStep {
    /// How this pane splits off its source. Ignored for the first pane.
    pub dir: Split,
    /// Which earlier pane to split off (1-based, in `panes` order). `None` =
    /// the pane created just before this one. Lets a macro build a real grid:
    /// split pane 1 right for pane 2, split pane 1 down for pane 3, etc.
    pub from: Option<usize>,
    /// A command line to run in the pane (typed, then Enter). Runs before `steps`.
    pub cmd: Option<String>,
    /// A scripted sequence run in the pane after `cmd`: type lines, pause, or
    /// wait for a prompt to appear — an in-tool login (`sqlplus /nolog` →
    /// `connect …`), a paced command run, etc.
    pub steps: Vec<Step>,
    /// Background colour spec for the pane (a `"#rrggbb"` / named / `"r,g,b"`
    /// string, parsed by the UI).
    pub bg: Option<String>,
    /// A directory to start a session log in for this pane.
    pub log: Option<String>,
    /// Percentage of the split the *source* pane keeps (5–95). `None` = 50/50.
    /// Lets a grid make even thirds (33 then 50) instead of 1/2, 1/4, 1/4.
    pub ratio: Option<u16>,
}

/// One scripted action inside a pane. In Lua a bare string is a `Send`; a table
/// is `{ send = }`, `{ wait = seconds }`, or `{ expect = "text", timeout = s }`.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Type a line and press Enter.
    Send(String),
    /// Pause for this many seconds before the next step.
    Wait(f64),
    /// Wait until `text` appears in the pane (case-insensitive), or `timeout`
    /// seconds pass — for logins/tools that are ready only when they say so.
    Expect { text: String, timeout: f64 },
}

/// Default seconds to wait on an `expect` before giving up and moving on.
const DEFAULT_EXPECT_TIMEOUT: f64 = 30.0;

/// The split direction for a pane relative to the previous one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Split {
    /// Side by side (a vertical divider).
    #[default]
    Right,
    /// Stacked (a horizontal divider).
    Down,
}

impl Split {
    fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "down" | "bottom" | "below" | "vertical" | "v" | "d" => Split::Down,
            _ => Split::Right,
        }
    }
}

/// Read and evaluate a `macro.lua` file into a list of macros.
pub fn load(path: &Path) -> Result<Vec<Macro>, String> {
    let src = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse(&src)
}

/// Evaluate `src` (which must `return` a list of macros) into macros.
pub fn parse(src: &str) -> Result<Vec<Macro>, String> {
    let lua = Lua::new();
    let val: Value = lua
        .load(src)
        .set_name("macro.lua")
        .eval()
        .map_err(|e| e.to_string())?;
    let mut out = match val {
        // Either a single macro ({ name =, panes = }) — the natural shape for a
        // per-macro file in macro/ — or a list of them (macro.lua).
        Value::Table(t) => {
            if t.contains_key("name").map_err(|e| e.to_string())? {
                vec![macro_from(&t)?]
            } else {
                let mut v = Vec::new();
                for m in t.sequence_values::<Table>() {
                    v.push(macro_from(&m.map_err(|e| e.to_string())?)?);
                }
                v
            }
        }
        Value::Nil => Vec::new(),
        other => return Err(format!("a macro file must return a table, got {}", other.type_name())),
    };
    // A script macro is re-run by re-evaluating its file, so each one carries the
    // source it came from (the `run` closure itself cannot outlive this `Lua`).
    for m in &mut out {
        if m.is_script() {
            m.script = Some(src.to_string());
        }
    }
    Ok(out)
}

fn macro_from(t: &Table) -> Result<Macro, String> {
    let name: String = t.get("name").map_err(|_| "a macro is missing its name".to_string())?;
    // A `run` function makes this a script macro — it automates file operations
    // instead of building panes, so `panes` is not required (or read) for it.
    // `parse` fills in `script` with the file source afterward.
    let is_script = matches!(t.get::<Value>("run"), Ok(Value::Function(_)));
    let sync = t.get::<Option<bool>>("sync").unwrap_or(None).unwrap_or(false);
    let zoom = t.get::<Option<bool>>("zoom").unwrap_or(None).unwrap_or(false);
    if is_script {
        return Ok(Macro { name, panes: Vec::new(), sync, zoom, script: Some(String::new()) });
    }
    let mut panes = Vec::new();
    if let Ok(Value::Table(pt)) = t.get::<Value>("panes") {
        for p in pt.clone().sequence_values::<Table>() {
            panes.push(pane_from(&p.map_err(|e| e.to_string())?)?);
        }
    }
    if panes.is_empty() {
        return Err(format!("macro {:?} has no panes (and no `run` function)", name));
    }
    Ok(Macro { name, panes, sync, zoom, script: None })
}

fn pane_from(t: &Table) -> Result<PaneStep, String> {
    let dir = t
        .get::<Option<String>>("dir")
        .unwrap_or(None)
        .map(|s| Split::parse(&s))
        .unwrap_or_default();
    let from = t.get::<Option<usize>>("from").unwrap_or(None).filter(|n| *n > 0);
    let cmd = t.get::<Option<String>>("cmd").unwrap_or(None).filter(|s| !s.is_empty());
    let bg = t.get::<Option<String>>("bg").unwrap_or(None).filter(|s| !s.is_empty());
    let log = t.get::<Option<String>>("log").unwrap_or(None).filter(|s| !s.is_empty());
    let ratio = t.get::<Option<u16>>("ratio").unwrap_or(None).map(|r| r.clamp(5, 95));
    let mut steps = Vec::new();
    if let Ok(Value::Table(st)) = t.get::<Value>("steps") {
        for s in st.sequence_values::<Value>() {
            if let Some(step) = step_from(s.map_err(|e| e.to_string())?)? {
                steps.push(step);
            }
        }
    }
    Ok(PaneStep { dir, from, cmd, steps, bg, log, ratio })
}

/// Parse one entry of a `steps` list: a bare string (`Send`), or a table
/// carrying `send` / `wait` / `expect`.
fn step_from(v: Value) -> Result<Option<Step>, String> {
    match v {
        Value::String(s) => {
            let line = s.to_str().map_err(|e| e.to_string())?.to_string();
            Ok(Some(Step::Send(line)))
        }
        Value::Table(t) => {
            if let Some(send) = t.get::<Option<String>>("send").map_err(|e| e.to_string())? {
                Ok(Some(Step::Send(send)))
            } else if let Some(secs) = t.get::<Option<f64>>("wait").map_err(|e| e.to_string())? {
                Ok(Some(Step::Wait(secs.max(0.0))))
            } else if let Some(text) = t.get::<Option<String>>("expect").map_err(|e| e.to_string())? {
                let timeout = t
                    .get::<Option<f64>>("timeout")
                    .map_err(|e| e.to_string())?
                    .filter(|s| *s > 0.0)
                    .unwrap_or(DEFAULT_EXPECT_TIMEOUT);
                Ok(Some(Step::Expect { text, timeout }))
            } else {
                Err("a step table needs one of `send`, `wait`, or `expect`".into())
            }
        }
        Value::Nil => Ok(None),
        other => Err(format!("a step must be a string or a table, got {}", other.type_name())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_layout_macro() {
        let src = r##"
            return {
              { name = "Prod", panes = {
                { cmd = "ssh admin@db", bg = "#402018", log = "~/logs" },
                { dir = "right", cmd = "ssh admin@app" },
                { dir = "down", steps = { "sqlplus /nolog", "connect u/p@db" } },
              }},
            }
        "##;
        let macros = parse(src).unwrap();
        assert_eq!(macros.len(), 1);
        let m = &macros[0];
        assert_eq!(m.name, "Prod");
        assert_eq!(m.panes.len(), 3);
        assert_eq!(m.panes[0].cmd.as_deref(), Some("ssh admin@db"));
        assert_eq!(m.panes[0].bg.as_deref(), Some("#402018"));
        assert_eq!(m.panes[0].log.as_deref(), Some("~/logs"));
        assert_eq!(m.panes[0].dir, Split::Right); // default for the first pane
        assert_eq!(m.panes[1].dir, Split::Right);
        assert_eq!(m.panes[2].dir, Split::Down);
        assert_eq!(
            m.panes[2].steps,
            vec![Step::Send("sqlplus /nolog".into()), Step::Send("connect u/p@db".into())]
        );
    }

    #[test]
    fn ratio_parses_and_clamps() {
        let src = r#"return { { name = "g", panes = {
            { cmd = "a" },
            { from = 1, dir = "right", ratio = 33, cmd = "b" },
            { from = 1, dir = "down",  ratio = 200, cmd = "c" },
        } } }"#;
        let m = &parse(src).unwrap()[0];
        assert_eq!(m.panes[0].ratio, None);
        assert_eq!(m.panes[1].ratio, Some(33));
        assert_eq!(m.panes[2].ratio, Some(95), "clamped to 95");
    }

    #[test]
    fn shipped_grid6_macro_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/macro/Cgrid6.lua");
        if !path.exists() {
            eprintln!("example macro not found; skipping");
            return;
        }
        let ms = parse(&std::fs::read_to_string(&path).unwrap()).expect("Cgrid6.lua parses");
        let m = &ms[0];
        assert_eq!(m.panes.len(), 6, "six panes");
        assert!(!m.sync && m.zoom);
        assert_eq!(m.panes[0].from, None);
        // Even-thirds ratios: the two column tops keep 33%.
        assert_eq!(m.panes[2].ratio, Some(33));
        assert_eq!(m.panes[3].ratio, Some(33));
        assert_eq!(m.panes[5].from, Some(4), "pane 6 splits off pane 4");
    }

    #[test]
    fn shipped_grid_macro_parses() {
        // The 2×2 SSH-login example must always load: 4 panes, sync off, each
        // logging in with an expect/send pair.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/macro/Cgrid4.lua");
        if !path.exists() {
            eprintln!("example macro not found; skipping");
            return;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        let ms = parse(&src).expect("Cgrid4.lua parses");
        let m = &ms[0];
        assert_eq!(m.panes.len(), 4, "four panes");
        assert!(!m.sync, "sync is off");
        assert!(m.zoom, "zoom is on");
        assert_eq!(m.panes[0].from, None, "pane 1 is the current shell");
        assert_eq!(m.panes[3].from, Some(2), "pane 4 splits off pane 2");
        let expected = ["ssh A@ABCserver", "ssh B@DEFserver", "ssh C@GHIserver", "ssh D@JKLserver"];
        for (p, want) in m.panes.iter().zip(expected) {
            assert_eq!(p.cmd.as_deref(), Some(want), "each pane sshes to its own host");
            assert!(matches!(p.steps.first(), Some(Step::Expect { .. })), "waits for the prompt");
            assert!(matches!(p.steps.last(), Some(Step::Send(_))), "sends the password");
        }
    }

    #[test]
    fn steps_can_wait_and_expect() {
        let src = r#"return { { name = "login", panes = { {
            cmd = "ssh admin@db",
            steps = {
              { expect = "password:", timeout = 15 },
              { send = "hunter2" },
              { wait = 2 },
              "sqlplus /nolog",
              { expect = "SQL>" },
            },
        } } } }"#;
        let m = &parse(src).unwrap()[0];
        assert_eq!(
            m.panes[0].steps,
            vec![
                Step::Expect { text: "password:".into(), timeout: 15.0 },
                Step::Send("hunter2".into()),
                Step::Wait(2.0),
                Step::Send("sqlplus /nolog".into()),
                Step::Expect { text: "SQL>".into(), timeout: 30.0 }, // default timeout
            ]
        );
    }

    #[test]
    fn a_step_table_without_a_verb_is_an_error() {
        assert!(parse(r#"return { { name = "x", panes = { { steps = { { foo = 1 } } } } } }"#).is_err());
    }

    #[test]
    fn direction_synonyms_and_defaults() {
        assert_eq!(Split::parse("down"), Split::Down);
        assert_eq!(Split::parse("BOTTOM"), Split::Down);
        assert_eq!(Split::parse("right"), Split::Right);
        assert_eq!(Split::parse("anything else"), Split::Right);
    }

    #[test]
    fn empty_and_nil_are_ok() {
        assert_eq!(parse("return {}").unwrap(), Vec::new());
        assert_eq!(parse("return nil").unwrap(), Vec::new());
    }

    #[test]
    fn a_macro_without_panes_is_an_error() {
        assert!(parse(r#"return { { name = "empty" } }"#).is_err());
    }

    #[test]
    fn a_non_table_return_is_an_error() {
        assert!(parse("return 7").is_err());
    }

    #[test]
    fn from_and_zoom_parse() {
        let m = &parse(
            r#"return { { name = "grid", zoom = true, panes = {
                { cmd = "a" },
                { from = 1, dir = "right", cmd = "b" },
                { from = 1, dir = "down", cmd = "c" },
            } } }"#,
        )
        .unwrap()[0];
        assert!(m.zoom);
        assert_eq!(m.panes[0].from, None, "first pane has no source");
        assert_eq!(m.panes[1].from, Some(1));
        assert_eq!(m.panes[2].from, Some(1));
    }

    #[test]
    fn sync_flag_parses() {
        let on = &parse(r#"return { { name = "x", sync = true, panes = { { cmd = "a" } } } }"#).unwrap()[0];
        assert!(on.sync);
        let off = &parse(r#"return { { name = "x", panes = { { cmd = "a" } } } }"#).unwrap()[0];
        assert!(!off.sync, "sync defaults to false");
    }

    #[test]
    fn a_single_macro_file_parses() {
        // The shape a per-macro file in macro/ uses: one macro, not a list.
        let macros = parse(r#"return { name = "Solo", panes = { { cmd = "echo hi" } } }"#).unwrap();
        assert_eq!(macros.len(), 1);
        assert_eq!(macros[0].name, "Solo");
        assert_eq!(macros[0].panes.len(), 1);
    }
}
