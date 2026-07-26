//! The bookmark shortcuts (`s` menu) as a Lua data file.
//!
//! Unlike `init.lua`, this file is *round-tripped*: the app rewrites it whenever
//! the user adds, renames or deletes a bookmark from the menu. So it lives here,
//! in the crate that owns the Lua runtime — read by executing it (it must
//! evaluate to a list of entries) and written back as pretty-printed Lua source.
//!
//! ```lua
//! return {
//!   { name = "home", target = "~" },
//!   { name = "Projects", children = {
//!     { name = "cian", target = "~/workspace/cian" },
//!   } },
//! }
//! ```
//!
//! An entry is a *leaf* (has `target`) or a *folder* (has `children`, a nested
//! list of the same shape).

use std::path::Path;

use mlua::{Lua, Table, Value};

/// One bookmark: a leaf (`target` set) or a folder (`children` set). This mirrors
/// the UI's own `Shortcut`, kept UI-agnostic here so this crate need not know
/// about the TUI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub target: Option<String>,
    pub children: Option<Vec<Node>>,
}

impl Node {
    pub fn leaf(name: impl Into<String>, target: impl Into<String>) -> Self {
        Self { name: name.into(), target: Some(target.into()), children: None }
    }
}

/// Read and evaluate a `shortcuts.lua` file into a list of entries.
pub fn load(path: &Path) -> Result<Vec<Node>, String> {
    let src = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse(&src)
}

/// Evaluate `src` (which must `return` a list of entries) into nodes.
pub fn parse(src: &str) -> Result<Vec<Node>, String> {
    let lua = Lua::new();
    let val: Value = lua
        .load(src)
        .set_name("shortcuts.lua")
        .eval()
        .map_err(|e| e.to_string())?;
    match val {
        Value::Table(t) => table_to_nodes(&t),
        Value::Nil => Ok(Vec::new()),
        other => Err(format!("shortcuts.lua must return a table, got {}", other.type_name())),
    }
}

fn table_to_nodes(t: &Table) -> Result<Vec<Node>, String> {
    let mut out = Vec::new();
    for entry in t.clone().sequence_values::<Table>() {
        let entry = entry.map_err(|e| e.to_string())?;
        out.push(table_to_node(&entry)?);
    }
    Ok(out)
}

fn table_to_node(t: &Table) -> Result<Node, String> {
    let name: String = t.get("name").map_err(|_| "shortcut entry is missing a name".to_string())?;
    let target: Option<String> = t.get::<Option<String>>("target").unwrap_or(None);
    let children = match t.get::<Value>("children") {
        Ok(Value::Table(ct)) => Some(table_to_nodes(&ct)?),
        _ => None,
    };
    Ok(Node { name, target, children })
}

const HEADER: &str = "\
-- cian shortcuts — the `s` menu.
--
-- Managed from inside the app: press `s`, then `a` add, `A` add folder,
-- `r` rename, `d` delete. It is rewritten on every change, so hand-editing is
-- optional — but perfectly fine. Each entry is a leaf (has `target`) or a
-- folder (has `children`). Targets may be a path, a URL, or an app/command.

";

/// Serialise `nodes` to a `shortcuts.lua` source string that [`parse`] reads back
/// identically.
pub fn to_lua(nodes: &[Node]) -> String {
    let mut s = String::from(HEADER);
    s.push_str("return ");
    write_list(&mut s, nodes, 0);
    s.push('\n');
    s
}

fn write_list(s: &mut String, nodes: &[Node], depth: usize) {
    if nodes.is_empty() {
        s.push_str("{}");
        return;
    }
    s.push_str("{\n");
    let pad = "  ".repeat(depth + 1);
    for n in nodes {
        s.push_str(&pad);
        s.push_str("{ name = ");
        s.push_str(&lua_str(&n.name));
        if let Some(t) = &n.target {
            s.push_str(", target = ");
            s.push_str(&lua_str(t));
        }
        if let Some(ch) = &n.children {
            s.push_str(", children = ");
            write_list(s, ch, depth + 1);
        }
        s.push_str(" },\n");
    }
    s.push_str(&"  ".repeat(depth));
    s.push('}');
}

/// Quote and escape a string as a Lua double-quoted literal.
fn lua_str(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    out.push('"');
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Node> {
        vec![
            Node::leaf("home", "~"),
            Node {
                name: "Projects".into(),
                target: None,
                children: Some(vec![
                    Node::leaf("cian", "~/workspace/cian"),
                    Node {
                        name: "Web".into(),
                        target: None,
                        children: Some(vec![Node::leaf("GitHub", "https://github.com")]),
                    },
                ]),
            },
        ]
    }

    #[test]
    fn round_trips_through_lua() {
        let nodes = sample();
        let src = to_lua(&nodes);
        assert!(src.contains("return {"), "emits a return table:\n{src}");
        assert!(src.contains("name = \"home\""));
        let back = parse(&src).unwrap();
        assert_eq!(back, nodes, "parse(to_lua(x)) == x");
    }

    #[test]
    fn escapes_quotes_and_backslashes() {
        let nodes = vec![Node::leaf("a \"quote\"", "C:\\Users\\me")];
        let back = parse(&to_lua(&nodes)).unwrap();
        assert_eq!(back, nodes);
    }

    #[test]
    fn empty_and_nil_are_ok() {
        assert!(to_lua(&[]).contains("return {}"));
        assert_eq!(parse("return {}").unwrap(), Vec::new());
        assert_eq!(parse("return nil").unwrap(), Vec::new());
    }

    #[test]
    fn a_hand_written_file_parses() {
        let src = r#"
            return {
              { name = "home", target = "~" },
              { name = "docs", children = {
                { name = "rust", target = "https://doc.rust-lang.org" },
              }},
            }
        "#;
        let nodes = parse(src).unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].target.as_deref(), Some("~"));
        assert_eq!(nodes[1].children.as_ref().unwrap()[0].name, "rust");
    }

    #[test]
    fn a_non_table_return_is_an_error() {
        assert!(parse("return 42").is_err());
    }
}
