//! The words cian says to a model, in one place.
//!
//! **They were written twice.** Every AI feature exists in both front ends and
//! each carried its own copy of the prompt — four of them, a paragraph each,
//! diverging quietly the way two copies of anything do. `scripts/parity.py`
//! exists because the two builds drifted on the *labels* people read; the
//! prompts are the labels the model reads, and nothing was checking those at
//! all. Sharing them removes the question rather than answering it.
//!
//! They live in `cian-core` because they are text and a rule, and depend on
//! nothing. The listing they are paired with is built by
//! [`crate::survey`], which gathers the facts; the division is that this file
//! holds the judgement and that one holds none.
//!
//! Written as raw strings with real line breaks. A prompt is read far more
//! often by a person deciding whether it is right than by the program, and a
//! wall of `\` continuations is not readable.

use std::time::SystemTime;

use crate::survey::{self, Survey};

/// Spot disposable build output, caches and cruft.
///
/// The old version of this saw one level of one directory and was handed
/// names with a blank where a folder's size should be — so it was asked to
/// judge `node_modules` by the sound of it. Everything specific here exists
/// because the columns now carry something worth reasoning over.
pub const JUNK: &str = r#"You spot disposable JUNK in a directory tree. Build output (target, build, dist, node_modules, __pycache__, .gradle, vendor caches), caches, logs, temp and editor-backup files (*.tmp, *.bak, *~, *.swp), and OS cruft (.DS_Store, Thumbs.db, desktop.ini).

You are given four columns: kind, size, age in days, path. **Use them.** A directory's size is the whole subtree below it, so it is what deleting the row would actually reclaim, and age is how long since anything under it changed. A size written `>2G` means the count stopped there and the real figure is larger — treat it as at least that big, which for ranking is usually enough to put it first. Rank what you return by how much space it frees, biggest first; a 4G build directory nobody has touched in a year is the answer, a 2K .DS_Store is a footnote.

Name the OUTERMOST row of any nest: if you list `target`, do not also list `target/debug`, because deleting the first takes the second with it and the second then reads as an error.

Be CONSERVATIVE. Never flag source code, documents, configuration, lockfiles, or anything whose loss would cost work — when a name is ambiguous, leave it out. A short list that is entirely right is worth more than a long one that has to be checked.

Reply with ONLY a JSON array of objects {"name": string (a path exactly as given), "reason": short string saying what it is and what regenerates it}. Empty array if nothing is clearly junk. No prose, no code fences."#;

/// Group the loose entries of one directory into sub-folders.
///
/// Depth one on purpose: this only ever moves what is directly here, so
/// showing it the tree would show it files it is not allowed to touch.
pub const STRUCTURE: &str = r#"You propose a tidy folder structure for a directory by grouping its loose entries into sub-folders (e.g. images/, docs/, src/, 2023/). Only MOVE existing entries into sub-folders — never rename, never delete, never move anything out of this directory.

You are given four columns: kind, size, age in days, path. Type is the first thing to group by, but **age is the second**: when a lot of files cluster into distinct periods, a year or a release is a better folder than a category nobody would look under. Sizes tell you which groups are worth making.

Leave a file where it is if no grouping is clearly better — omit it. Prefer a few meaningful folders over many tiny ones, and never propose a folder holding a single file.

Reply with ONLY a JSON array of objects {"name": string (exactly as given), "folder": string (a NEW or existing sub-folder, a simple relative path, no ..), "reason": short string}. Empty array if the directory is already well organised. No prose, no code fences."#;

/// Find the file somebody means, from a question rather than a pattern.
pub const SEARCH: &str = r#"You do semantic file search over a directory tree. You are given four columns — kind, size, age in days, path — and a question in natural language. Return the rows that answer it, best first.

**The question is not always about the name.** "the big ones", "what did I change this week", "the old backups" are answered from the size and age columns; a directory's size is its whole subtree, and `>2G` means at least that much. When the question names a thing, weigh the path as well as the filename — `src/auth/token.rs` answers "where is login handled" and `token.rs` alone answers it less well.

Prefer a short, ordered answer: ten rows that are right beat fifty that include them. Say in each reason what made it a match — the name, where it sits, its size, or when it changed.

Reply with ONLY a JSON array of objects {"path": string (exactly as given), "reason": short string}. Use only paths from the list. Empty array if none are a good match. No prose, no code fences."#;

/// Rename a set of files to an instruction.
pub const RENAME: &str = r#"You propose new file names following the user's instruction. Keep it a RENAME only: never change the folder, never add a path. Preserve the extension unless the instruction says otherwise.

Two names in one directory cannot be the same. If the instruction would collide — numbering that repeats, a pattern that ignores what makes two files different — number or disambiguate rather than proposing the collision, and say so in the reason.

Reply with ONLY a JSON array of objects {"name": string (exactly as given), "new_name": string (a bare filename, no path)}. Include only files that should change; omit the rest. No prose, no fences."#;

/// Write one command line.
///
/// The refusal clause is the part that earns its place. Asked for something
/// that is not one command, a model that must answer with a command answers
/// with a command — and a plausible half-right shell line is worse than
/// nothing, because it looks like the thing you asked for.
pub const CMD: &str = r#"You write ONE command line for the named shell, to be run in the named directory.

Answer with the command only — no explanation, no code fence, no leading prompt character, no `cd` to somewhere else (it already runs there).

**Write it for the shell you are told, not for the operating system.** powershell.exe takes PowerShell, not cmd.exe batch; a POSIX shell takes POSIX. Quote paths that contain spaces the way that shell quotes them.

The directory listing is there so you can use the real names. Prefer naming files over a wildcard when the listing shows you exactly which ones are meant.

**Two refusals, and they matter more than being helpful.** If the task cannot be done as one command line, answer with a single line beginning `# ` that says so in one sentence — do not invent a command that half does it. And never fold a destructive step (delete, overwrite, force-push, reset --hard) into a command that was asked to do something else; if deleting is what was asked for, write it plainly and alone so it can be read before it is run."#;

/// Draft a commit message from the staged diff.
pub const COMMIT: &str = r#"You write a git commit message for the given staged diff. Use the Conventional Commits style: a concise subject line under ~70 characters (an optional type prefix like feat:/fix:/refactor: is fine), then a blank line and a short body of bullet points explaining WHY, only if it adds something. Output ONLY the commit message — no code fences, no preamble."#;

/// Explain what went wrong in the shell panel.
///
/// Takes the platform because the fix depends on it — "command not found" is
/// a PATH question on Linux and quite often an execution-policy one on
/// Windows. Use [`os_name`] so both builds name the same three.
pub fn shell_error(os: &str) -> String {
    format!(
        "You explain shell/terminal errors for a developer on {os}. Given the recent terminal output, say plainly what went wrong and the most likely fix (a command or a change). If there is no error, say the output looks fine. Be concise; plain text, no markdown headings."
    )
}

/// What to call this platform when telling a model about it. Three names, and
/// both builds say the same one — they had a copy of this each.
pub fn os_name() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    }
}

/// Triage a log file from its tail.
pub const LOG: &str = r#"You triage a log file for an operator (often RHEL/AIX or Oracle). From the tail below: list the errors and warnings that matter, each with its key line; note a rough timeline if the timestamps show one; then give the single most likely cause and the next thing to check. Ignore routine INFO noise. Be concise; plain text, no markdown headings."#;

/// The survey rendered for a prompt: one row per entry, four columns, tab
/// separated.
///
/// Tabs rather than a table because the model is not reading it for looks and
/// every wasted character is a row that did not fit. The columns are the four
/// things the filesystem knows and a name does not: what it is, how much space
/// it stands for, how long since anyone touched it, and where it sits.
pub fn listing(rows: &[survey::Row], now: SystemTime) -> String {
    let mut out = String::from("kind\tsize\tage\tpath\n");
    for r in rows {
        let kind = if r.is_dir { "dir" } else { "file" };
        let age = match survey::age_days(r.modified, now) {
            Some(d) => format!("{d}d"),
            None => "?".to_string(),
        };
        // `>` where the sum stopped counting. The model is told what the
        // symbol means in the prompt; without it a floor reads as a total and
        // a four-gigabyte folder can rank below a two-gigabyte one.
        let size = if r.size_capped {
            format!(">{}", survey::brief_size(r.size))
        } else {
            survey::brief_size(r.size)
        };
        out.push_str(&format!("{kind}\t{size}\t{age}\t{}\n", r.rel));
    }
    out
}

/// What the model is told when the walk did not reach everything, in English
/// because it is going to a model. `None` when the survey is complete.
///
/// **A cap that is not said out loud is a lie in both directions.** Told
/// nothing, the model reasons from absence — "there is no build output here"
/// is a claim only a whole listing supports — and the person reads that back
/// as a fact about the directory rather than about the part that was scanned.
pub fn limit_note(s: &Survey) -> Option<String> {
    if !s.partial() {
        return None;
    }
    let mut parts = Vec::new();
    if s.stopped_at.is_some() {
        parts.push(match s.whole_to() {
            Some(d) => format!(
                "it lists everything down to {d} level(s) below the directory and then stops"
            ),
            None => "it stops partway through the directory's own entries".to_string(),
        });
    }
    if s.unopened > 0 {
        parts.push(format!("{} directories were too deep to open", s.unopened));
    }
    Some(parts.join("; "))
}

/// The whole user half of a survey-driven request: a heading, the caveat if
/// there is one, and the listing.
pub fn survey_user(head: &str, s: &Survey, now: SystemTime) -> String {
    match limit_note(s) {
        Some(n) => format!(
            "{head}\n\nNOTE: this listing is incomplete — {n}. Judge only what is shown, and do not conclude anything from what is absent.\n\n{}",
            listing(&s.rows, now)
        ),
        None => format!("{head}\n\n{}", listing(&s.rows, now)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    /// One row, aged against the *same* `now` the listing will be rendered
    /// with. Two calls to `now()` put a few microseconds between them, and 400
    /// days minus a few microseconds floors to 399.
    fn one_row(now: SystemTime, rel: &str, is_dir: bool, size: u64, age_days: u64) -> survey::Row {
        survey::Row {
            rel: rel.to_string(),
            path: rel.into(),
            is_dir,
            size,
            size_capped: false,
            modified: Some(now - Duration::from_secs(age_days * 86_400)),
            depth: 1,
        }
    }

    /// The four columns arrive, and a directory arrives carrying a size. That
    /// blank was the whole problem.
    #[test]
    fn the_listing_carries_what_a_name_cannot() {
        let now = SystemTime::now();
        let rows = vec![one_row(now, "node_modules", true, 4 << 30, 400), one_row(now, "a.rs", false, 300, 1)];
        let text = listing(&rows, now);
        assert!(text.starts_with("kind\tsize\tage\tpath\n"));
        assert!(text.contains("dir\t4G\t400d\tnode_modules\n"), "{text}");

        // A floor says so, in the listing and in the prompt that reads it.
        let mut capped = one_row(now, "target", true, 2 << 30, 1);
        capped.size_capped = true;
        assert!(listing(&[capped], now).contains("dir\t>2G\t1d\ttarget\n"));
        assert!(JUNK.contains("`>2G` means the count stopped there"));
        assert!(text.contains("file\t300B\t1d\ta.rs\n"), "{text}");
    }

    /// A complete survey says nothing extra; a truncated one says so in the
    /// prompt itself.
    #[test]
    fn an_incomplete_survey_tells_the_model_so() {
        let now = SystemTime::now();
        let whole = Survey { rows: vec![one_row(now, "a", false, 1, 0)], ..Default::default() };
        let text = survey_user("Directory: /x", &whole, now);
        assert!(!text.contains("NOTE"), "nothing to warn about");

        let part = Survey {
            rows: vec![one_row(now, "a", false, 1, 0)],
            stopped_at: Some(3),
            unopened: 3,
        };
        let text = survey_user("Directory: /x", &part, now);
        // The useful sentence: what *is* complete, not how much is missing.
        assert!(text.contains("down to 2 level(s)"), "{text}");
        assert!(text.contains("3 directories were too deep"), "{text}");
        assert!(text.contains("do not conclude anything from what is absent"));
    }

    /// The prompts are shared, so this is the one place a rule can be checked
    /// at all. These four are the ones a rewrite is most likely to drop.
    #[test]
    fn the_prompts_keep_the_rules_that_were_paid_for() {
        assert!(JUNK.contains("OUTERMOST"), "no nested-duplicate rule");
        assert!(JUNK.contains("CONSERVATIVE"));
        assert!(STRUCTURE.contains("never move anything out of this directory"));
        assert!(CMD.contains("beginning `# `"), "no refusal path");
        assert!(CMD.contains("not for the operating system"), "no shell rule");
        assert!(RENAME.contains("cannot be the same"), "no collision rule");
    }

    /// End to end over a real directory: survey, render, and the row a person
    /// would want flagged carries its subtree size into the prompt.
    #[test]
    fn a_real_tree_reaches_the_prompt_with_its_sizes() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("target/debug")).unwrap();
        std::fs::write(d.path().join("target/debug/big.o"), vec![b'x'; 5000]).unwrap();
        std::fs::write(d.path().join("main.rs"), b"fn main(){}").unwrap();
        let s = survey::survey(d.path(), survey::Limits::default(), &AtomicBool::new(false));
        let text = survey_user("Directory: x", &s, SystemTime::now());
        let line = text.lines().find(|l| l.ends_with("\ttarget")).expect("target listed");
        assert!(line.starts_with("dir\t4.9K"), "the subtree, not the folder entry: {line}");
    }
}
