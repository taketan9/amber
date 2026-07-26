//! `count.lua` — configuration for the file/step counter (cian's kazoechao).
//!
//! Read-only, loaded like `init.lua`. It returns a table tuning what counts:
//!
//! ```lua
//! return {
//!   extensions      = { "rs", "lua", "py" },   -- omit for every text file
//!   count_blank     = false,                    -- blank lines as steps?
//!   count_comments  = false,                    -- comment lines as steps?
//!   comment_prefixes = { "//", "#", "--" },     -- what starts a comment line
//! }
//! ```
//!
//! Any field may be omitted; the built-in defaults ([`cian_core::count::Options`])
//! fill the rest.

use std::path::Path;

use cian_core::count::Options;
use mlua::{Lua, Value};

/// Read and evaluate a `count.lua` file into counter options.
pub fn load(path: &Path) -> Result<Options, String> {
    let src = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse(&src)
}

/// Evaluate `src` (which must `return` a table, or nothing) into options,
/// starting from the defaults and overriding whatever the table sets.
pub fn parse(src: &str) -> Result<Options, String> {
    let lua = Lua::new();
    let val: Value = lua.load(src).set_name("count.lua").eval().map_err(|e| e.to_string())?;
    let mut o = Options::default();
    let t = match val {
        Value::Table(t) => t,
        Value::Nil => return Ok(o),
        other => return Err(format!("count.lua must return a table, got {}", other.type_name())),
    };
    if let Ok(Value::Table(exts)) = t.get::<Value>("extensions") {
        let mut list = Vec::new();
        for e in exts.sequence_values::<String>() {
            let e = e.map_err(|e| e.to_string())?;
            let e = e.trim().trim_start_matches('.').to_lowercase();
            if !e.is_empty() {
                list.push(e);
            }
        }
        o.extensions = list;
    }
    if let Some(b) = t.get::<Option<bool>>("count_blank").map_err(|e| e.to_string())? {
        o.count_blank = b;
    }
    if let Some(b) = t.get::<Option<bool>>("count_comments").map_err(|e| e.to_string())? {
        o.count_comments = b;
    }
    if let Ok(Value::Table(pfx)) = t.get::<Value>("comment_prefixes") {
        let mut list = Vec::new();
        for p in pfx.sequence_values::<String>() {
            list.push(p.map_err(|e| e.to_string())?);
        }
        o.comment_prefixes = list;
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_apply_on_top_of_defaults() {
        let o = parse(
            r#"return {
                extensions = { ".RS", "lua" },
                count_blank = true,
                comment_prefixes = { "//", "--" },
            }"#,
        )
        .unwrap();
        assert_eq!(o.extensions, vec!["rs", "lua"]); // dot stripped, lower-cased
        assert!(o.count_blank);
        assert!(!o.count_comments); // untouched → default
        assert_eq!(o.comment_prefixes, vec!["//", "--"]);
    }

    #[test]
    fn nil_or_empty_is_the_default() {
        assert!(parse("return nil").unwrap().extensions.is_empty());
        assert!(!parse("return {}").unwrap().count_blank);
    }

    #[test]
    fn a_non_table_is_an_error() {
        assert!(parse("return 5").is_err());
    }
}
