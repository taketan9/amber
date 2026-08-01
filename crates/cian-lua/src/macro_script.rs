//! Script macros — the AFXW-style half of the macro feature.
//!
//! A layout macro (see [`crate::macros`]) builds shell panes. A **script macro**
//! instead automates *file operations*: it is a macro whose `run` is a Lua
//! function that receives a `cx` handle and drives copies, moves, renames,
//! zipping, shelling out, and so on — with Lua's own `for` / `if` for control.
//!
//! ```lua
//! return {
//!   name = "Archive *.log then bin them",
//!   run = function(cx)
//!     local logs = cx.glob("*.log")
//!     if #logs == 0 then cx.message("no logs here") return end
//!     cx.zip(logs, "logs.zip")
//!     cx.delete(logs)
//!     cx.message("archived " .. #logs .. " logs")
//!   end,
//! }
//! ```
//!
//! Everything runs **synchronously** on the calling thread, so the operations
//! happen in the order written and the macro can branch on their results. It is
//! deliberately self-contained (filesystem + subprocess via `cian_core` and
//! `std`), so it never needs to reach back into the TUI while running; the caller
//! just applies the [`Outcome`] (reload the panes, show the messages) afterward.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use mlua::{Lua, Table, Value};

/// A snapshot of the file panes, handed to a script macro when it runs.
pub struct Ctx {
    /// The active pane's directory — the macro's working directory.
    pub dir: PathBuf,
    /// The opposite pane's directory (a natural copy/move destination).
    pub other: PathBuf,
    /// The marked entries, or the entry under the cursor when nothing is marked.
    pub marked: Vec<PathBuf>,
    /// The entry under the cursor, if any.
    pub cursor: Option<PathBuf>,
}

/// What running a script macro produced, for the caller to apply.
#[derive(Default)]
pub struct Outcome {
    /// `cx.message(...)` lines, shown after the macro finishes.
    pub messages: Vec<String>,
    /// A Lua error (syntax or runtime), if the macro blew up. What it managed to
    /// do before the error still stands — file operations are not transactional.
    pub error: Option<String>,
    /// True if any operation changed the filesystem, so the panes want a reload.
    pub touched: bool,
}

struct Shared {
    ctx: Ctx,
    messages: Vec<String>,
    touched: bool,
}

/// Run script macro `macro_name`, defined in `source` (the whole macro file), on
/// the panes described by `ctx`.
pub fn run(source: &str, macro_name: &str, ctx: Ctx) -> Outcome {
    let lua = Lua::new();
    let shared = Rc::new(RefCell::new(Shared { ctx, messages: Vec::new(), touched: false }));

    let run_fn = match find_run(&lua, source, macro_name) {
        Ok(f) => f,
        Err(e) => return Outcome { error: Some(e), ..Default::default() },
    };
    let cx = match build_cx(&lua, &shared) {
        Ok(t) => t,
        Err(e) => return Outcome { error: Some(e.to_string()), ..Default::default() },
    };

    let call: mlua::Result<()> = run_fn.call(cx);
    let mut out = {
        let s = shared.borrow();
        Outcome { messages: s.messages.clone(), touched: s.touched, error: None }
    };
    if let Err(e) = call {
        out.error = Some(e.to_string());
    }
    out
}

/// Evaluate the macro file and pull out the `run` function of the named macro.
fn find_run(lua: &Lua, source: &str, name: &str) -> Result<mlua::Function, String> {
    let val: Value = lua.load(source).set_name("macro").eval().map_err(|e| e.to_string())?;
    let Value::Table(t) = val else {
        return Err("macro file did not return a table".into());
    };
    // A file may hold one macro ({name=,run=}) or a list of them.
    let singles: Vec<Table> = if t.contains_key("name").unwrap_or(false) {
        vec![t]
    } else {
        t.sequence_values::<Table>().filter_map(|m| m.ok()).collect()
    };
    for m in singles {
        if m.get::<Option<String>>("name").ok().flatten().as_deref() == Some(name) {
            return m
                .get::<mlua::Function>("run")
                .map_err(|_| format!("macro {:?} has no `run` function", name));
        }
    }
    Err(format!("macro {:?} not found in the file", name))
}

// ── the `cx` API ─────────────────────────────────────────────────────────────

fn to_lua_err(e: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::runtime(e.to_string())
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Resolve a user path: `~` → home, relative → under `base`, absolute as-is.
fn resolve(base: &Path, raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| (raw == "~").then_some("")) {
        if let Some(home) = home_dir() {
            return if rest.is_empty() { home } else { home.join(rest) };
        }
    }
    let p = Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Read a path argument as one or many paths (a string, or an array of strings).
fn paths_arg(base: &Path, v: &Value) -> mlua::Result<Vec<PathBuf>> {
    match v {
        Value::String(str_) => Ok(vec![resolve(base, &str_.to_string_lossy())]),
        Value::Table(t) => {
            let mut out = Vec::new();
            for item in t.clone().sequence_values::<String>() {
                out.push(resolve(base, &item?));
            }
            Ok(out)
        }
        Value::Nil => Ok(Vec::new()),
        other => Err(mlua::Error::runtime(format!(
            "expected a path or a list of paths, got {}",
            other.type_name()
        ))),
    }
}

/// Case-insensitive `*`/`?` glob of a single filename component.
fn glob_match(pat: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) =
        (pat.to_lowercase().chars().collect(), name.to_lowercase().chars().collect());
    // Classic iterative wildcard match with backtracking on `*`.
    let (mut pi, mut ni, mut star, mut mark) = (0usize, 0usize, None, 0usize);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn build_cx(lua: &Lua, shared: &Rc<RefCell<Shared>>) -> mlua::Result<Table> {
    let cx = lua.create_table()?;
    let dir = shared.borrow().ctx.dir.clone();

    // ── query ──
    {
        let sh = Rc::clone(shared);
        cx.set("dir", lua.create_function(move |_, ()| Ok(s(&sh.borrow().ctx.dir)))?)?;
    }
    {
        let sh = Rc::clone(shared);
        cx.set("other", lua.create_function(move |_, ()| Ok(s(&sh.borrow().ctx.other)))?)?;
    }
    {
        let sh = Rc::clone(shared);
        cx.set(
            "marked",
            lua.create_function(move |_, ()| {
                Ok(sh.borrow().ctx.marked.iter().map(|p| s(p)).collect::<Vec<_>>())
            })?,
        )?;
    }
    {
        let sh = Rc::clone(shared);
        cx.set(
            "cursor",
            lua.create_function(move |_, ()| Ok(sh.borrow().ctx.cursor.as_deref().map(s)))?,
        )?;
    }
    // list(dir?) — entries in a directory (default: the working dir), as paths.
    {
        let base = dir.clone();
        cx.set(
            "list",
            lua.create_function(move |_, arg: Option<String>| {
                let d = arg.map(|a| resolve(&base, &a)).unwrap_or_else(|| base.clone());
                let mut out = Vec::new();
                if let Ok(rd) = std::fs::read_dir(&d) {
                    for e in rd.flatten() {
                        out.push(s(&e.path()));
                    }
                }
                out.sort();
                Ok(out)
            })?,
        )?;
    }
    // glob(pattern) — names in the working dir matching a `*`/`?` pattern.
    {
        let base = dir.clone();
        cx.set(
            "glob",
            lua.create_function(move |_, pat: String| {
                let mut out = Vec::new();
                if let Ok(rd) = std::fs::read_dir(&base) {
                    for e in rd.flatten() {
                        let name = e.file_name().to_string_lossy().into_owned();
                        if glob_match(&pat, &name) {
                            out.push(s(&e.path()));
                        }
                    }
                }
                out.sort();
                Ok(out)
            })?,
        )?;
    }

    // ── operations (each marks the panes dirty) ──
    // copy(paths, dest_dir)
    {
        let base = dir.clone();
        let sh = Rc::clone(shared);
        cx.set(
            "copy",
            lua.create_function(move |_, (paths, dest): (Value, String)| {
                let srcs = paths_arg(&base, &paths)?;
                let dest = resolve(&base, &dest);
                std::fs::create_dir_all(&dest).map_err(to_lua_err)?;
                let mut n = 0;
                for src in &srcs {
                    if cian_core::ops::copy_one(src, &dest, cian_core::ops::Conflict::Overwrite)
                        .map_err(to_lua_err)?
                    {
                        n += 1;
                    }
                }
                sh.borrow_mut().touched = true;
                Ok(n)
            })?,
        )?;
    }
    // move(paths, dest_dir)
    {
        let base = dir.clone();
        let sh = Rc::clone(shared);
        cx.set(
            "move",
            lua.create_function(move |_, (paths, dest): (Value, String)| {
                let srcs = paths_arg(&base, &paths)?;
                let dest = resolve(&base, &dest);
                std::fs::create_dir_all(&dest).map_err(to_lua_err)?;
                let mut n = 0;
                for src in &srcs {
                    if cian_core::ops::move_one(src, &dest, cian_core::ops::Conflict::Overwrite)
                        .map_err(to_lua_err)?
                    {
                        n += 1;
                    }
                }
                sh.borrow_mut().touched = true;
                Ok(n)
            })?,
        )?;
    }
    // delete(paths) — to the trash, like `d`.
    {
        let base = dir.clone();
        let sh = Rc::clone(shared);
        cx.set(
            "delete",
            lua.create_function(move |_, paths: Value| {
                let srcs = paths_arg(&base, &paths)?;
                let report = cian_core::ops::delete_many(&srcs, cian_core::ops::DeleteMode::Trash);
                sh.borrow_mut().touched = true;
                Ok(report.ok)
            })?,
        )?;
    }
    // rename(path, new_name) — within the same directory.
    {
        let base = dir.clone();
        let sh = Rc::clone(shared);
        cx.set(
            "rename",
            lua.create_function(move |_, (path, new): (String, String)| {
                let src = resolve(&base, &path);
                let parent = src.parent().unwrap_or(&base);
                let dst = parent.join(&new);
                std::fs::rename(&src, &dst).map_err(to_lua_err)?;
                sh.borrow_mut().touched = true;
                Ok(s(&dst))
            })?,
        )?;
    }
    // mkdir(name) — create a directory (and parents) under the working dir.
    {
        let base = dir.clone();
        let sh = Rc::clone(shared);
        cx.set(
            "mkdir",
            lua.create_function(move |_, name: String| {
                let p = resolve(&base, &name);
                std::fs::create_dir_all(&p).map_err(to_lua_err)?;
                sh.borrow_mut().touched = true;
                Ok(s(&p))
            })?,
        )?;
    }
    // zip(paths, out) — bundle paths into a .zip.
    {
        let base = dir.clone();
        let sh = Rc::clone(shared);
        cx.set(
            "zip",
            lua.create_function(move |_, (paths, out): (Value, String)| {
                let srcs = paths_arg(&base, &paths)?;
                let dest = resolve(&base, &out);
                let cancel = std::sync::atomic::AtomicBool::new(false);
                let mut prog = |_: &cian_core::progress::Progress| {};
                let mut ctl = cian_core::progress::Ctl { cancel: &cancel, on_progress: &mut prog };
                let report = cian_core::archive::create_zip(&srcs, &dest, None, &mut ctl);
                sh.borrow_mut().touched = true;
                if let Some(err) = report.errors.first() {
                    return Err(mlua::Error::runtime(err.clone()));
                }
                Ok(s(&dest))
            })?,
        )?;
    }
    // read(path) / write(path, text)
    {
        let base = dir.clone();
        cx.set(
            "read",
            lua.create_function(move |_, path: String| {
                std::fs::read_to_string(resolve(&base, &path)).map_err(to_lua_err)
            })?,
        )?;
    }
    {
        let base = dir.clone();
        let sh = Rc::clone(shared);
        cx.set(
            "write",
            lua.create_function(move |_, (path, text): (String, String)| {
                let p = resolve(&base, &path);
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent).map_err(to_lua_err)?;
                }
                std::fs::write(&p, text).map_err(to_lua_err)?;
                sh.borrow_mut().touched = true;
                Ok(s(&p))
            })?,
        )?;
    }

    // ── subprocess ──
    // sh(cmd) — run a shell command in the working dir; returns {code,out,err}.
    {
        let base = dir.clone();
        let sh = Rc::clone(shared);
        cx.set(
            "sh",
            lua.create_function(move |lua, cmd: String| {
                let out = shell_command(&cmd)
                    .current_dir(&base)
                    .output()
                    .map_err(to_lua_err)?;
                let t = lua.create_table()?;
                t.set("code", out.status.code().unwrap_or(-1))?;
                t.set("out", String::from_utf8_lossy(&out.stdout).into_owned())?;
                t.set("err", String::from_utf8_lossy(&out.stderr).into_owned())?;
                // A shelled command may well have changed files.
                sh.borrow_mut().touched = true;
                Ok(t)
            })?,
        )?;
    }

    // ── feedback ──
    {
        let sh = Rc::clone(shared);
        cx.set(
            "message",
            lua.create_function(move |_, text: String| {
                sh.borrow_mut().messages.push(text);
                Ok(())
            })?,
        )?;
    }

    // ── path helpers (pure) ──
    cx.set(
        "basename",
        lua.create_function(|_, p: String| {
            Ok(Path::new(&p).file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or(p))
        })?,
    )?;
    cx.set(
        "stem",
        lua.create_function(|_, p: String| {
            Ok(Path::new(&p).file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or(p))
        })?,
    )?;
    cx.set(
        "ext",
        lua.create_function(|_, p: String| {
            Ok(Path::new(&p)
                .extension()
                .map(|x| x.to_string_lossy().into_owned())
                .unwrap_or_default())
        })?,
    )?;
    {
        let base = dir.clone();
        cx.set(
            "join",
            lua.create_function(move |_, (a, b): (String, String)| {
                let a = resolve(&base, &a);
                Ok(s(&a.join(b)))
            })?,
        )?;
    }
    {
        let base = dir.clone();
        cx.set(
            "exists",
            lua.create_function(move |_, p: String| Ok(resolve(&base, &p).exists()))?,
        )?;
    }
    {
        let base = dir.clone();
        cx.set(
            "isdir",
            lua.create_function(move |_, p: String| Ok(resolve(&base, &p).is_dir()))?,
        )?;
    }
    // size(path) — file size in bytes (0 if it cannot be read / is a directory).
    {
        let base = dir.clone();
        cx.set(
            "size",
            lua.create_function(move |_, p: String| {
                Ok(std::fs::metadata(resolve(&base, &p)).map(|m| m.len()).unwrap_or(0))
            })?,
        )?;
    }

    Ok(cx)
}

#[cfg(windows)]
fn shell_command(cmd: &str) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/C").arg(cmd);
    c
}

#[cfg(not(windows))]
fn shell_command(cmd: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(dir: &Path) -> Ctx {
        Ctx { dir: dir.to_path_buf(), other: dir.to_path_buf(), marked: Vec::new(), cursor: None }
    }

    #[test]
    fn glob_matches_wildcards_case_insensitively() {
        assert!(glob_match("*.log", "app.LOG"));
        assert!(glob_match("app.?", "app.c"));
        assert!(glob_match("*", "anything"));
        assert!(!glob_match("*.log", "app.txt"));
        assert!(glob_match("a*z", "abcz"));
        assert!(!glob_match("a*z", "abc"));
    }

    #[test]
    fn a_macro_can_glob_zip_and_delete() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.log"), b"one").unwrap();
        std::fs::write(d.path().join("b.log"), b"two").unwrap();
        std::fs::write(d.path().join("keep.txt"), b"stay").unwrap();

        let src = r#"return {
          name = "archive",
          run = function(cx)
            local logs = cx.glob("*.log")
            cx.message("found " .. #logs)
            cx.zip(logs, "logs.zip")
            cx.delete(logs)
          end,
        }"#;
        let out = run(src, "archive", ctx(d.path()));
        assert!(out.error.is_none(), "no error: {:?}", out.error);
        assert!(out.touched, "the filesystem changed");
        assert_eq!(out.messages, vec!["found 2".to_string()]);
        assert!(d.path().join("logs.zip").is_file(), "zip was created");
        assert!(!d.path().join("a.log").exists(), "logs were removed");
        assert!(d.path().join("keep.txt").exists(), "the non-log stayed");
    }

    #[test]
    fn a_macro_can_sort_files_into_folders_by_extension() {
        let d = tempfile::tempdir().unwrap();
        for n in ["one.txt", "two.txt", "pic.png"] {
            std::fs::write(d.path().join(n), b"x").unwrap();
        }
        let src = r#"return {
          name = "sort",
          run = function(cx)
            for _, p in ipairs(cx.glob("*")) do
              local e = cx.ext(p)
              if e ~= "" then
                cx.mkdir(e)
                cx.move({ p }, e)
              end
            end
          end,
        }"#;
        let out = run(src, "sort", ctx(d.path()));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert!(d.path().join("txt/one.txt").is_file());
        assert!(d.path().join("txt/two.txt").is_file());
        assert!(d.path().join("png/pic.png").is_file());
    }

    #[test]
    fn a_missing_macro_or_run_is_reported_not_panicked() {
        let d = tempfile::tempdir().unwrap();
        let out = run(r#"return { name = "x", run = function(cx) end }"#, "nope", ctx(d.path()));
        assert!(out.error.as_deref().unwrap().contains("not found"));

        let out2 = run(r#"return { name = "x", panes = {} }"#, "x", ctx(d.path()));
        assert!(out2.error.as_deref().unwrap().contains("no `run`"));
    }

    #[test]
    fn shipped_sample_macros_load_as_scripts_and_run() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/macro/Escript.lua");
        if !path.exists() {
            eprintln!("sample macros not found; skipping");
            return;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        // They must all parse as script macros (a `run` function, no panes).
        let macros = crate::macros::parse(&src).expect("Escript.lua parses");
        assert!(macros.len() >= 8, "a good handful of samples");
        assert!(macros.iter().all(|m| m.is_script()), "every sample is a script macro");

        // And the "sort by extension" one actually sorts a temp dir.
        let d = tempfile::tempdir().unwrap();
        for n in ["one.txt", "pic.png"] {
            std::fs::write(d.path().join(n), b"x").unwrap();
        }
        let out = run(&src, "拡張子ごとに仕分け", ctx(d.path()));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert!(d.path().join("txt/one.txt").is_file());
        assert!(d.path().join("png/pic.png").is_file());
    }

    #[test]
    fn a_runtime_error_in_the_macro_is_captured() {
        let d = tempfile::tempdir().unwrap();
        let out = run(
            r#"return { name = "boom", run = function(cx) error("kaboom") end }"#,
            "boom",
            ctx(d.path()),
        );
        assert!(out.error.as_deref().unwrap().contains("kaboom"));
    }
}
