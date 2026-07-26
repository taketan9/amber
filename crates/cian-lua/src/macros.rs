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

/// One macro: a name and the panes it builds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Macro {
    pub name: String,
    pub panes: Vec<PaneStep>,
    /// Turn on input broadcast (synchronize) across the built panes once the
    /// layout is up — for "run the same command on every server" setups.
    pub sync: bool,
}

/// One pane in a layout macro.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaneStep {
    /// How this pane splits off the previous one. Ignored for the first pane.
    pub dir: Split,
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
    match val {
        // Either a single macro ({ name =, panes = }) — the natural shape for a
        // per-macro file in macro/ — or a list of them (macro.lua).
        Value::Table(t) => {
            if t.contains_key("name").map_err(|e| e.to_string())? {
                Ok(vec![macro_from(&t)?])
            } else {
                let mut out = Vec::new();
                for m in t.sequence_values::<Table>() {
                    out.push(macro_from(&m.map_err(|e| e.to_string())?)?);
                }
                Ok(out)
            }
        }
        Value::Nil => Ok(Vec::new()),
        other => Err(format!("a macro file must return a table, got {}", other.type_name())),
    }
}

fn macro_from(t: &Table) -> Result<Macro, String> {
    let name: String = t.get("name").map_err(|_| "a macro is missing its name".to_string())?;
    let mut panes = Vec::new();
    if let Ok(Value::Table(pt)) = t.get::<Value>("panes") {
        for p in pt.clone().sequence_values::<Table>() {
            panes.push(pane_from(&p.map_err(|e| e.to_string())?)?);
        }
    }
    if panes.is_empty() {
        return Err(format!("macro {:?} has no panes", name));
    }
    let sync = t.get::<Option<bool>>("sync").unwrap_or(None).unwrap_or(false);
    Ok(Macro { name, panes, sync })
}

fn pane_from(t: &Table) -> Result<PaneStep, String> {
    let dir = t
        .get::<Option<String>>("dir")
        .unwrap_or(None)
        .map(|s| Split::parse(&s))
        .unwrap_or_default();
    let cmd = t.get::<Option<String>>("cmd").unwrap_or(None).filter(|s| !s.is_empty());
    let bg = t.get::<Option<String>>("bg").unwrap_or(None).filter(|s| !s.is_empty());
    let log = t.get::<Option<String>>("log").unwrap_or(None).filter(|s| !s.is_empty());
    let mut steps = Vec::new();
    if let Ok(Value::Table(st)) = t.get::<Value>("steps") {
        for s in st.sequence_values::<Value>() {
            if let Some(step) = step_from(s.map_err(|e| e.to_string())?)? {
                steps.push(step);
            }
        }
    }
    Ok(PaneStep { dir, cmd, steps, bg, log })
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
