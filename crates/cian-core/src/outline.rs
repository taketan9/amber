//! The shape of a file: its headings, functions and sections, found with
//! regular expressions rather than a parser.
//!
//! Sakura Editor's outline analysis is what this is for, and its lesson is
//! that the cheap version is worth most of the expensive one. A language
//! server would know more, but it would also mean shipping one per language,
//! a background process, and a project that has to build before anything
//! shows up. A dozen anchored patterns get you a jump list for a 4000-line
//! shell script or a stored procedure on a machine that has no toolchain at
//! all, which is exactly where the need is felt.
//!
//! Being approximate is fine here in a way it would not be for a compiler: a
//! missed function costs one scroll, and a false positive costs one glance.
//! The rules are therefore anchored and conservative — a definition at the
//! start of a line — and deliberately blind to strings, comments and macros.
//!
//! Nesting comes from the pattern for languages that mark it (`##` in
//! Markdown) and from leading whitespace for those that do not, which is what
//! makes the same list usable for folding: a section runs until the next entry
//! at its level or shallower.

use std::path::Path;

use regex::Regex;

/// What an entry is, so the list can be coloured and read at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A document heading, or anything else that names a region of prose.
    Heading,
    /// A type, class, module or namespace — a container for other entries.
    Type,
    /// A function, method, procedure or target.
    Function,
    /// A config section, a label, an SQL statement — a flat marker.
    Section,
}

/// One entry in the outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// 0-based line the entry starts on.
    pub line: usize,
    /// Depth, 0 being outermost. Drives the indent in the list and the extent
    /// of a fold.
    pub level: usize,
    /// What to show — the line, trimmed, with the syntax that identified it
    /// left in place. `## Notes` reads better than `Notes` in a list that also
    /// holds `### Details`.
    pub text: String,
    pub kind: Kind,
}

/// One rule: a pattern that identifies a line, and what to call it.
struct Rule {
    re: Regex,
    kind: Kind,
    /// Depth comes from the length of capture group 1 rather than from
    /// indentation — `#` in Markdown, where a heading is never indented.
    level_from_capture: bool,
}

fn rule(pat: &str, kind: Kind) -> Rule {
    Rule { re: Regex::new(pat).expect("built-in outline pattern"), kind, level_from_capture: false }
}

/// The rules for `path`, chosen by extension.
///
/// Compiled per call. Building a dozen small regexes costs far less than
/// reading the file that is about to be scanned, and the alternative — a
/// process-wide cache — buys nothing measurable and has to be kept correct.
fn rules(path: &Path) -> Vec<Rule> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_lowercase();

    match ext.as_str() {
        "md" | "markdown" | "mdown" => vec![Rule {
            re: Regex::new(r"^(#{1,6})\s+\S").unwrap(),
            kind: Kind::Heading,
            level_from_capture: true,
        }],
        "rs" => vec![
            rule(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:unsafe\s+)?(?:impl|trait|struct|enum|union|mod)\b", Kind::Type),
            rule(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+\S+\s+)?fn\s+\w", Kind::Function),
            rule(r"^\s*macro_rules!\s+\w", Kind::Function),
        ],
        "py" | "pyw" => vec![
            rule(r"^\s*class\s+\w", Kind::Type),
            rule(r"^\s*(?:async\s+)?def\s+\w", Kind::Function),
        ],
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" => vec![
            rule(r"^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?(?:class|interface|enum|type)\s+\w", Kind::Type),
            rule(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s*\w", Kind::Function),
            // `const handler = (…) => {` and the method shorthand inside a
            // class body — between them these cover most modern code.
            rule(r"^\s*(?:export\s+)?(?:const|let|var)\s+\w+\s*(?::[^=]+)?=\s*(?:async\s+)?(?:function\b|\(|\w+\s*=>)", Kind::Function),
            rule(r"^\s{2,}(?:public|private|protected|static|async|get|set)?\s*\w+\s*\([^;]*\)\s*\{\s*$", Kind::Function),
        ],
        "java" | "cs" | "kt" | "scala" | "groovy" => vec![
            rule(r"^\s*(?:public|private|protected|internal)?\s*(?:static\s+)?(?:final\s+|abstract\s+|sealed\s+|data\s+)*(?:class|interface|enum|record|object|trait)\s+\w", Kind::Type),
            rule(r"^\s+(?:public|private|protected|internal|fun|def)[^;=]*\([^;]*\)\s*(?:\{|throws|:)", Kind::Function),
        ],
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" => vec![
            rule(r"^\s*(?:typedef\s+)?(?:struct|union|enum|class|namespace)\s+\w", Kind::Type),
            // A definition, not a declaration: the brace has to be there.
            rule(r"^[A-Za-z_][\w \t\*&:<>,]*\**\s*\w+\s*\([^;]*\)\s*(?:const\s*)?\{?\s*$", Kind::Function),
            rule(r"^\s*#\s*(?:define|pragma\s+mark)\s+\w", Kind::Section),
        ],
        "sh" | "bash" | "ksh" | "zsh" => vec![
            rule(r"^\s*(?:function\s+)?\w[\w\-]*\s*\(\s*\)\s*\{?", Kind::Function),
            rule(r"^\s*function\s+\w[\w\-]*\s*\{?", Kind::Function),
            // A banner comment is how long shell scripts are actually divided.
            rule(r"^#{2,}\s*\S|^#\s*-{3,}", Kind::Section),
        ],
        "sql" | "pks" | "pkb" | "prc" | "trg" => vec![
            rule(r"(?i)^\s*create\s+(?:or\s+replace\s+)?(?:package|procedure|function|trigger|view|table|index|type|materialized)\b", Kind::Type),
            rule(r"(?i)^\s*(?:alter|drop|truncate)\s+\w", Kind::Section),
            rule(r"(?i)^\s*(?:procedure|function)\s+\w", Kind::Function),
        ],
        "lua" => vec![
            rule(r"^\s*(?:local\s+)?function\s+[\w.:]+", Kind::Function),
            rule(r"^\s*(?:local\s+)?[\w.]+\s*=\s*function\b", Kind::Function),
        ],
        "yaml" | "yml" => vec![rule(r"^\s*[\w.\-]+:\s*$", Kind::Section)],
        "toml" | "ini" | "cfg" | "conf" | "properties" | "service" => {
            vec![rule(r"^\s*\[[^\]]+\]\s*$", Kind::Section)]
        }
        "html" | "htm" | "xhtml" | "jsp" | "vue" | "svg" | "xml" => {
            vec![rule(r"(?i)^\s*<(?:h[1-6]|section|article|head|body|template|script|style)\b", Kind::Section)]
        }
        "css" | "scss" | "less" => vec![rule(r"^\s*[.#@\w\[][^{;]*\{\s*$", Kind::Section)],
        "go" => vec![
            rule(r"^\s*type\s+\w", Kind::Type),
            rule(r"^\s*func\s+", Kind::Function),
        ],
        "rb" => vec![
            rule(r"^\s*(?:class|module)\s+\w", Kind::Type),
            rule(r"^\s*def\s+\w", Kind::Function),
        ],
        "ps1" | "psm1" => vec![rule(r"(?i)^\s*function\s+[\w\-]+", Kind::Function)],
        "bat" | "cmd" => vec![rule(r"^\s*:\w", Kind::Section)],
        "mk" | "mak" => vec![rule(r"^[\w./$(){}%\-]+\s*:(?:[^=]|$)", Kind::Section)],
        _ => {
            // Extensionless files that are still worth an outline.
            if name == "makefile" || name == "gnumakefile" {
                vec![rule(r"^[\w./$(){}%\-]+\s*:(?:[^=]|$)", Kind::Section)]
            } else {
                Vec::new()
            }
        }
    }
}

/// Read the shape of `lines`, using the rules for `path`'s type.
///
/// An empty result means "no rules for this kind of file", which the caller
/// should say out loud — an outline panel that opens blank is indistinguishable
/// from one that thinks the file has no structure.
pub fn outline(path: &Path, lines: &[String]) -> Vec<Item> {
    let rules = rules(path);
    if rules.is_empty() {
        return Vec::new();
    }
    let mut raw: Vec<(usize, usize, String, Kind)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        // Only the first rule that matches: `pub fn` is a function, and the
        // later, looser patterns must not claim it a second time.
        for r in &rules {
            let Some(caps) = r.re.captures(line) else { continue };
            let depth = if r.level_from_capture {
                caps.get(1).map(|m| m.len().saturating_sub(1)).unwrap_or(0)
            } else {
                indent_of(line)
            };
            raw.push((i, depth, line.trim().to_string(), r.kind));
            break;
        }
    }
    // Indentation is a column count, not a level: turn the distinct depths that
    // actually occur into 0, 1, 2… so a file indented with eight spaces does
    // not produce an outline indented off the side of the panel.
    let mut depths: Vec<usize> = raw.iter().map(|(_, d, _, _)| *d).collect();
    depths.sort_unstable();
    depths.dedup();
    raw.into_iter()
        .map(|(line, depth, text, kind)| Item {
            line,
            level: depths.iter().position(|d| *d == depth).unwrap_or(0),
            text,
            kind,
        })
        .collect()
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// The lines an entry covers: from its own line to just before the next entry
/// at the same level or shallower, or the end of the file.
///
/// Returned end-inclusive, and `None` when the entry is a single line with
/// nothing under it — there is nothing to fold, and offering to would be a
/// key that appears to do nothing.
pub fn extent(items: &[Item], idx: usize, total_lines: usize) -> Option<(usize, usize)> {
    let item = items.get(idx)?;
    let end = items[idx + 1..]
        .iter()
        .find(|n| n.level <= item.level)
        .map(|n| n.line.saturating_sub(1))
        .unwrap_or(total_lines.saturating_sub(1));
    (end > item.line).then_some((item.line, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn on(name: &str, src: &str) -> Vec<Item> {
        let lines: Vec<String> = src.lines().map(str::to_string).collect();
        outline(&PathBuf::from(name), &lines)
    }
    fn texts(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.text.as_str()).collect()
    }

    #[test]
    fn markdown_nests_by_hashes() {
        let items = on("doc.md", "# Top\nprose\n## Second\n### Third\n#not a heading\n## Back\n");
        assert_eq!(texts(&items), ["# Top", "## Second", "### Third", "## Back"]);
        assert_eq!(items.iter().map(|i| i.level).collect::<Vec<_>>(), [0, 1, 2, 1]);
    }

    #[test]
    fn rust_separates_types_from_functions() {
        let items = on(
            "a.rs",
            "use std::io;\nstruct Config {\n    field: u8,\n}\npub async fn run() {}\n    fn helper() {}\n// fn commented() {}\n",
        );
        assert_eq!(texts(&items), ["struct Config {", "pub async fn run() {}", "fn helper() {}"]);
        assert_eq!(items[0].kind, Kind::Type);
        assert_eq!(items[1].kind, Kind::Function);
        // The indented one is a level deeper, from its indentation alone.
        assert!(items[2].level > items[1].level);
    }

    /// The two kinds of shell function declaration, and the banner comments
    /// long scripts are really divided by.
    #[test]
    fn shell_finds_both_function_forms_and_banners() {
        let items = on(
            "run.sh",
            "#!/bin/sh\n### setup\nmain() {\n  echo hi\n}\nfunction cleanup {\n  :\n}\nVAR=1\n",
        );
        assert_eq!(texts(&items), ["### setup", "main() {", "function cleanup {"]);
    }

    #[test]
    fn sql_finds_the_statements_that_name_things() {
        let items = on(
            "p.sql",
            "-- header\nCREATE OR REPLACE PACKAGE BODY pkg AS\n  PROCEDURE do_it IS\n  BEGIN\n    NULL;\n  END;\nEND;\nalter table t add x;\n",
        );
        assert_eq!(
            texts(&items),
            ["CREATE OR REPLACE PACKAGE BODY pkg AS", "PROCEDURE do_it IS", "alter table t add x;"],
        );
    }

    #[test]
    fn a_file_type_with_no_rules_says_so_by_being_empty() {
        assert!(on("notes.txt", "anything\nat all\n").is_empty());
        assert!(on("no-extension", "x\n").is_empty());
        // …but a Makefile is worth one even without an extension.
        assert_eq!(texts(&on("Makefile", "all: build\n\tgo build\nCFLAGS = -g\n")), ["all: build"]);
    }

    /// A fold runs to the next entry at the same level or shallower — which is
    /// what makes "collapse this section" mean the section and not the file.
    #[test]
    fn an_entry_extends_to_the_next_one_at_its_level() {
        let items = on("d.md", "# One\na\n## Under\nb\nc\n# Two\nd\n");
        let total = 7;
        assert_eq!(extent(&items, 0, total), Some((0, 4)), "# One holds ## Under");
        assert_eq!(extent(&items, 1, total), Some((2, 4)), "## Under stops at # Two");
        assert_eq!(extent(&items, 2, total), Some((5, 6)), "the last runs to the end");

        // Nothing beneath it is nothing to fold.
        let flat = on("e.md", "# One\n# Two\n");
        assert_eq!(extent(&flat, 0, 2), None);
    }
}
