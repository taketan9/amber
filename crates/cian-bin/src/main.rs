use std::env;
use std::path::PathBuf;

use anyhow::Result;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| matches!(a.as_str(), "-v" | "-V" | "--version")) {
        println!("{}", cian_tui::version_text());
        return Ok(());
    }
    if args.iter().any(|a| matches!(a.as_str(), "-h" | "--help")) {
        println!("{}", cian_tui::usage_text());
        return Ok(());
    }
    if args.iter().any(|a| matches!(a.as_str(), "-man" | "--man")) {
        println!("{}", cian_tui::manual_text());
        return Ok(());
    }
    // Pull out the startup-macro options, and treat a bare `*.lua` argument as
    // `--macro <that file>` so a macro file can be file-associated with cian and
    // launched by double-click (cian's TeraTerm-`.ttl`-style entry point).
    let mut macro_file: Option<PathBuf> = None;
    let mut macro_name: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--macro" | "-m" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    macro_file = Some(PathBuf::from(v));
                }
            }
            "--macro-name" => {
                i += 1;
                if let Some(v) = args.get(i) {
                    macro_name = Some(v.clone());
                }
            }
            a if a.ends_with(".lua") && macro_file.is_none() => macro_file = Some(PathBuf::from(a)),
            a => positional.push(a.to_string()),
        }
        i += 1;
    }

    // Paths default to the configured home (see cian_tui::run) when omitted,
    // rather than to the process's working directory — which, launched from a
    // shortcut, is wherever the exe sits.
    let left = positional.first().map(PathBuf::from);
    let right = positional.get(1).map(PathBuf::from);
    let startup = match (macro_file, macro_name) {
        (Some(f), _) => cian_tui::StartupMacro::File(f),
        (None, Some(n)) => cian_tui::StartupMacro::Named(n),
        (None, None) => cian_tui::StartupMacro::None,
    };
    cian_tui::run(left, right, startup)
}
