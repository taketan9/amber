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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Macro {
    pub name: String,
    pub panes: Vec<PaneStep>,
}

/// One pane in a layout macro.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneStep {
    /// How this pane splits off the previous one. Ignored for the first pane.
    pub dir: Split,
    /// A command line to run in the pane (typed, then Enter).
    pub cmd: Option<String>,
    /// Further lines sent in order after `cmd` — e.g. an in-tool login sequence.
    pub steps: Vec<String>,
    /// Background colour spec for the pane (a `"#rrggbb"` / named / `"r,g,b"`
    /// string, parsed by the UI).
    pub bg: Option<String>,
    /// A directory to start a session log in for this pane.
    pub log: Option<String>,
}

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
        Value::Table(t) => {
            let mut out = Vec::new();
            for m in t.clone().sequence_values::<Table>() {
                out.push(macro_from(&m.map_err(|e| e.to_string())?)?);
            }
            Ok(out)
        }
        Value::Nil => Ok(Vec::new()),
        other => Err(format!("macro.lua must return a table, got {}", other.type_name())),
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
    Ok(Macro { name, panes })
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
        for s in st.sequence_values::<String>() {
            steps.push(s.map_err(|e| e.to_string())?);
        }
    }
    Ok(PaneStep { dir, cmd, steps, bg, log })
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
        assert_eq!(m.panes[2].steps, vec!["sqlplus /nolog", "connect u/p@db"]);
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
}
