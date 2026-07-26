//! A small, dependency-free syntax highlighter for the F3 viewer.
//!
//! Not a parser — a per-line lexer that colours strings, comments, numbers and
//! a per-language keyword/type set (plus tags/attributes for markup). It carries
//! only "am I inside a block comment" across lines, which is enough to read
//! code at a glance without pulling in a heavyweight grammar engine. The lexer
//! emits a [`Category`] per character; mapping those to colours is the UI's job.

use std::path::Path;

/// What a run of characters is, semantically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Plain,
    Keyword,
    Type,
    Str,
    Comment,
    Number,
    /// A markup tag (`<div`, `>`).
    Tag,
    /// A markup attribute name.
    Attr,
}

/// A language the highlighter knows. Markdown is excluded — it has its own
/// rendered preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Java,
    Html,
    Css,
    Sql,
    Shell,
    Lua,
    Yaml,
    Json,
}

/// Pick a language from the file extension, or `None` to leave it unhighlighted.
pub fn detect(path: &Path) -> Option<Lang> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    Some(match ext.as_str() {
        "rs" => Lang::Rust,
        "py" | "pyw" => Lang::Python,
        "js" | "mjs" | "cjs" | "jsx" => Lang::JavaScript,
        "ts" | "tsx" => Lang::TypeScript,
        "java" => Lang::Java,
        // JSP is markup with embedded tags; the HTML lexer reads it well enough.
        "html" | "htm" | "xml" | "xhtml" | "jsp" | "vue" | "svg" => Lang::Html,
        "css" | "scss" | "less" => Lang::Css,
        "sql" => Lang::Sql,
        "sh" | "bash" | "ksh" | "zsh" => Lang::Shell,
        "lua" => Lang::Lua,
        "yaml" | "yml" => Lang::Yaml,
        "json" => Lang::Json,
        _ => return None,
    })
}

/// Highlight `lines`, returning one [`Category`] per character (same shape as the
/// input, so `out[line][col]` is the category of `lines[line]`'s `col`-th char).
pub fn highlight(lines: &[String], lang: Lang) -> Vec<Vec<Category>> {
    let spec = spec(lang);
    let mut out = Vec::with_capacity(lines.len());
    let mut in_block = false; // inside a block comment, carried across lines
    for line in lines {
        let chars: Vec<char> = line.chars().collect();
        let mut cats = vec![Category::Plain; chars.len()];
        if spec.markup {
            highlight_markup_line(&chars, &mut cats, &mut in_block);
        } else {
            highlight_code_line(&chars, &mut cats, &mut in_block, &spec);
        }
        out.push(cats);
    }
    out
}

/// Per-language lexing rules.
struct Spec {
    line_comments: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    strings: &'static [char],
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    markup: bool,
}

fn highlight_code_line(chars: &[char], cats: &mut [Category], in_block: &mut bool, spec: &Spec) {
    let n = chars.len();
    let mut i = 0;
    // Finish a block comment carried in from a previous line.
    if *in_block {
        if let Some((_, end)) = spec.block {
            match find_str(chars, 0, end) {
                Some(e) => {
                    let stop = e + end.chars().count();
                    fill(cats, 0, stop, Category::Comment);
                    *in_block = false;
                    i = stop;
                }
                None => {
                    fill(cats, 0, n, Category::Comment);
                    return;
                }
            }
        }
    }
    while i < n {
        // Line comment → rest of the line.
        if let Some(lc) = spec.line_comments.iter().find(|lc| starts_with(chars, i, lc)) {
            let _ = lc;
            fill(cats, i, n, Category::Comment);
            return;
        }
        // Block comment start.
        if let Some((start, end)) = spec.block {
            if starts_with(chars, i, start) {
                match find_str(chars, i + start.chars().count(), end) {
                    Some(e) => {
                        let stop = e + end.chars().count();
                        fill(cats, i, stop, Category::Comment);
                        i = stop;
                        continue;
                    }
                    None => {
                        fill(cats, i, n, Category::Comment);
                        *in_block = true;
                        return;
                    }
                }
            }
        }
        let c = chars[i];
        // Strings.
        if spec.strings.contains(&c) {
            let end = scan_string(chars, i, c);
            fill(cats, i, end, Category::Str);
            i = end;
            continue;
        }
        // Numbers (a digit, optionally with a leading sign handled as punctuation).
        if c.is_ascii_digit() {
            let end = scan_number(chars, i);
            fill(cats, i, end, Category::Number);
            i = end;
            continue;
        }
        // Identifiers → keyword / type / plain.
        if is_ident_start(c) {
            let mut j = i + 1;
            while j < n && is_ident_part(chars[j]) {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let cat = if spec.keywords.contains(&word.as_str()) {
                Category::Keyword
            } else if spec.types.contains(&word.as_str()) {
                Category::Type
            } else {
                Category::Plain
            };
            fill(cats, i, j, cat);
            i = j;
            continue;
        }
        i += 1;
    }
}

/// A pragmatic markup lexer for HTML/XML/JSP: tags, attribute names, quoted
/// values, and `<!-- -->` comments (carried across lines).
fn highlight_markup_line(chars: &[char], cats: &mut [Category], in_block: &mut bool) {
    let n = chars.len();
    let mut i = 0;
    if *in_block {
        match find_str(chars, 0, "-->") {
            Some(e) => {
                let stop = e + 3;
                fill(cats, 0, stop, Category::Comment);
                *in_block = false;
                i = stop;
            }
            None => {
                fill(cats, 0, n, Category::Comment);
                return;
            }
        }
    }
    while i < n {
        if starts_with(chars, i, "<!--") {
            match find_str(chars, i + 4, "-->") {
                Some(e) => {
                    let stop = e + 3;
                    fill(cats, i, stop, Category::Comment);
                    i = stop;
                    continue;
                }
                None => {
                    fill(cats, i, n, Category::Comment);
                    *in_block = true;
                    return;
                }
            }
        }
        if chars[i] == '<' {
            // A tag: `<`, name and `/`, attributes, quoted values, up to `>`.
            let mut j = i + 1;
            // `<`, optional `/`, and the tag name are the tag colour.
            while j < n && (chars[j] == '/' || chars[j] == '!' || is_ident_part(chars[j])) {
                j += 1;
            }
            fill(cats, i, j, Category::Tag);
            while j < n && chars[j] != '>' {
                let c = chars[j];
                if c == '"' || c == '\'' {
                    let end = scan_string(chars, j, c);
                    fill(cats, j, end, Category::Str);
                    j = end;
                } else if is_ident_start(c) {
                    let s = j;
                    while j < n && (is_ident_part(chars[j]) || chars[j] == '-' || chars[j] == ':') {
                        j += 1;
                    }
                    fill(cats, s, j, Category::Attr);
                } else {
                    j += 1;
                }
            }
            if j < n {
                cats[j] = Category::Tag; // the closing '>'
                j += 1;
            }
            i = j;
            continue;
        }
        i += 1;
    }
}

// ─────────────────────────── small scanners ───────────────────────────

fn scan_string(chars: &[char], start: usize, quote: char) -> usize {
    let n = chars.len();
    let mut i = start + 1;
    while i < n {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i] == quote {
            return i + 1;
        }
        i += 1;
    }
    n // unterminated — colour to end of line
}

fn scan_number(chars: &[char], start: usize) -> usize {
    let n = chars.len();
    let mut i = start;
    while i < n {
        let c = chars[i];
        if c.is_ascii_hexdigit() || c == '.' || c == '_' || c == 'x' || c == 'X' || c == 'b' || c == 'o' {
            i += 1;
        } else {
            break;
        }
    }
    i.max(start + 1)
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$' || c == '@' || c == '#'
}

fn is_ident_part(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Do `chars[at..]` begin with `pat`?
fn starts_with(chars: &[char], at: usize, pat: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    at + p.len() <= chars.len() && chars[at..at + p.len()] == p[..]
}

/// Index in `chars` at or after `from` where `pat` begins.
fn find_str(chars: &[char], from: usize, pat: &str) -> Option<usize> {
    let p: Vec<char> = pat.chars().collect();
    if p.is_empty() || chars.len() < p.len() {
        return None;
    }
    (from..=chars.len() - p.len()).find(|&i| chars[i..i + p.len()] == p[..])
}

fn fill(cats: &mut [Category], from: usize, to: usize, cat: Category) {
    let to = to.min(cats.len());
    if from < to {
        for c in &mut cats[from..to] {
            *c = cat;
        }
    }
}

fn spec(lang: Lang) -> Spec {
    // Shared token classes; keyword/type sets are curated per language.
    match lang {
        Lang::Rust => Spec {
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            strings: &['"'],
            keywords: &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
                "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
                "super", "trait", "true", "type", "unsafe", "use", "where", "while",
            ],
            types: &[
                "bool", "char", "str", "String", "u8", "u16", "u32", "u64", "u128", "usize", "i8",
                "i16", "i32", "i64", "i128", "isize", "f32", "f64", "Vec", "Option", "Result", "Box",
            ],
            markup: false,
        },
        Lang::Python => Spec {
            line_comments: &["#"],
            block: None,
            strings: &['"', '\''],
            keywords: &[
                "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
                "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in",
                "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
                "with", "yield", "None", "True", "False", "self",
            ],
            types: &["int", "str", "float", "bool", "list", "dict", "set", "tuple", "bytes"],
            markup: false,
        },
        Lang::JavaScript | Lang::TypeScript => Spec {
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            strings: &['"', '\'', '`'],
            keywords: &[
                "async", "await", "break", "case", "catch", "class", "const", "continue", "debugger",
                "default", "delete", "do", "else", "export", "extends", "false", "finally", "for",
                "function", "if", "import", "in", "instanceof", "let", "new", "null", "of", "return",
                "super", "switch", "this", "throw", "true", "try", "typeof", "undefined", "var",
                "void", "while", "yield", "interface", "type", "enum", "implements", "readonly",
                "public", "private", "protected",
            ],
            types: &["number", "string", "boolean", "any", "unknown", "never", "object", "Array"],
            markup: false,
        },
        Lang::Java => Spec {
            line_comments: &["//"],
            block: Some(("/*", "*/")),
            strings: &['"'],
            keywords: &[
                "abstract", "assert", "break", "case", "catch", "class", "const", "continue",
                "default", "do", "else", "enum", "extends", "final", "finally", "for", "goto", "if",
                "implements", "import", "instanceof", "interface", "native", "new", "package",
                "private", "protected", "public", "return", "static", "super", "switch",
                "synchronized", "this", "throw", "throws", "transient", "try", "volatile", "while",
                "true", "false", "null",
            ],
            types: &[
                "boolean", "byte", "char", "double", "float", "int", "long", "short", "void",
                "String", "Integer", "Object", "List", "Map",
            ],
            markup: false,
        },
        Lang::Sql => Spec {
            line_comments: &["--"],
            block: Some(("/*", "*/")),
            strings: &['\'', '"'],
            keywords: &[
                "select", "from", "where", "insert", "into", "values", "update", "set", "delete",
                "create", "table", "drop", "alter", "add", "index", "view", "join", "inner", "left",
                "right", "outer", "on", "group", "by", "order", "having", "limit", "and", "or",
                "not", "null", "as", "distinct", "union", "all", "in", "like", "between", "is",
                "primary", "key", "foreign", "references", "default", "constraint",
            ],
            types: &["int", "integer", "varchar", "char", "text", "date", "timestamp", "number", "boolean"],
            markup: false,
        },
        Lang::Shell => Spec {
            line_comments: &["#"],
            block: None,
            strings: &['"', '\''],
            keywords: &[
                "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while", "until",
                "case", "esac", "function", "return", "break", "continue", "local", "export", "echo",
                "read", "exit", "cd", "source",
            ],
            types: &[],
            markup: false,
        },
        Lang::Lua => Spec {
            line_comments: &["--"],
            block: Some(("--[[", "]]")),
            strings: &['"', '\''],
            keywords: &[
                "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto",
                "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until",
                "while",
            ],
            types: &[],
            markup: false,
        },
        Lang::Yaml => Spec {
            line_comments: &["#"],
            block: None,
            strings: &['"', '\''],
            keywords: &["true", "false", "null", "yes", "no", "on", "off"],
            types: &[],
            markup: false,
        },
        Lang::Json => Spec {
            line_comments: &[],
            block: None,
            strings: &['"'],
            keywords: &["true", "false", "null"],
            types: &[],
            markup: false,
        },
        Lang::Css => Spec {
            line_comments: &[],
            block: Some(("/*", "*/")),
            strings: &['"', '\''],
            keywords: &[],
            types: &[],
            markup: false,
        },
        Lang::Html => Spec {
            line_comments: &[],
            block: None, // handled by the markup lexer (`<!-- -->`)
            strings: &['"', '\''],
            keywords: &[],
            types: &[],
            markup: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cats(src: &str, lang: Lang) -> Vec<Vec<Category>> {
        let lines: Vec<String> = src.lines().map(|s| s.to_string()).collect();
        highlight(&lines, lang)
    }

    #[test]
    fn detects_by_extension() {
        assert_eq!(detect(Path::new("a.rs")), Some(Lang::Rust));
        assert_eq!(detect(Path::new("a.TS")), Some(Lang::TypeScript));
        assert_eq!(detect(Path::new("a.jsp")), Some(Lang::Html));
        assert_eq!(detect(Path::new("a.md")), None); // markdown has its own preview
        assert_eq!(detect(Path::new("a")), None);
    }

    #[test]
    fn rust_keyword_string_comment_number() {
        // `let x = "hi"; // note`
        let c = &cats("let x = \"hi\"; // note", Lang::Rust)[0];
        assert_eq!(c[0], Category::Keyword, "`let` is a keyword");
        assert_eq!(c[8], Category::Str, "inside the string");
        // The `// note` tail is a comment.
        assert!(c.iter().rev().take(4).all(|&x| x == Category::Comment), "trailing comment");
    }

    #[test]
    fn block_comments_span_lines() {
        let c = cats("a /* start\nmiddle\nend */ b", Lang::Rust);
        assert_eq!(c[1][0], Category::Comment, "middle line is all comment");
        assert_eq!(c[2][0], Category::Comment, "up to the close is comment");
        // After `*/ ` the `b` is plain again.
        assert_eq!(*c[2].last().unwrap(), Category::Plain, "code resumes after close");
    }

    #[test]
    fn html_tags_and_attributes() {
        let c = &cats("<a href=\"x\">t</a>", Lang::Html)[0];
        assert_eq!(c[0], Category::Tag, "`<` starts a tag");
        assert!(c.contains(&Category::Attr), "href is an attribute");
        assert!(c.contains(&Category::Str), "the value is a string");
    }

    #[test]
    fn json_numbers_and_literals() {
        let c = &cats("{\"n\": 42, \"ok\": true}", Lang::Json)[0];
        assert!(c.contains(&Category::Number), "42 is a number");
        assert!(c.contains(&Category::Keyword), "true is a literal");
        assert!(c.contains(&Category::Str), "keys are strings");
    }
}
