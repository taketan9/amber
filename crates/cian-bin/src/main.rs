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
    // Paths default to the configured home (see cian_tui::run) when omitted,
    // rather than to the process's working directory — which, launched from a
    // shortcut, is wherever the exe sits.
    let left = args.first().map(PathBuf::from);
    let right = args.get(1).map(PathBuf::from);
    cian_tui::run(left, right)
}
