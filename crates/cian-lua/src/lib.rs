//! cian-lua: mlua-based configuration host.
//!
//! Loads `~/.config/cian/init.lua` (overridable with `CIAN_CONFIG_DIR`) and
//! exposes a small WezTerm-flavoured `cian` API to user scripts:
//!
//! ```lua
//! cian.set_theme "solarized-light"   -- a named preset, or
//! cian.set_theme({ accent = "#00d7d7", mark_fg = "yellow" })
//! cian.set_keymap("x", "delete")          -- bind key `x` to the delete action
//! cian.set_option("clipboard_on_copy", false)
//! cian.on_open("md", function(path)        -- extension-dispatch execution
//!   cian.spawn({ "open", "-a", "Typora", path })
//! end)
//! ```
//!
//! Loading never fails the program: any syntax/runtime error is captured in
//! [`Config::errors`] and the UI falls back to defaults for whatever could not
//! be applied. This crate stays UI-agnostic — colors are passed through as raw
//! strings and parsed by the UI layer.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;

use mlua::{Function, FromLua, Lua, Table, Value};

pub mod count;
pub mod macros;
pub mod shortcuts;

/// A color spec exactly as the user wrote it: `"#rrggbb"`, a named color
/// (`"cyan"`, `"yellow"`), or `"r,g,b"`. Parsed by the UI layer.
pub type ColorSpec = String;

/// User-supplied color overrides. `None` means "keep the built-in default".
#[derive(Debug, Clone, Default)]
pub struct Theme {
    /// A named palette to start from — e.g. "solarized-light". Unset uses the
    /// built-in dark theme. Per-key overrides below still apply on top.
    pub preset: Option<String>,
    pub accent: Option<ColorSpec>,
    pub status_bg: Option<ColorSpec>,
    pub selected_bg: Option<ColorSpec>,
    pub visual_bg: Option<ColorSpec>,
    pub mark_fg: Option<ColorSpec>,
}

/// Behavioural switches. `None` means "keep the built-in default".
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub clipboard_on_copy: Option<bool>,
    /// Program to run in the embedded shell panel (e.g. "powershell.exe").
    pub shell: Option<String>,
    /// Duration of split/zoom/close transitions in milliseconds. `0` disables
    /// animation entirely.
    pub animation_ms: Option<u64>,
    /// Show the contextual key-hint bar above the status line.
    pub key_hints: Option<bool>,
    /// Border corners: "rounded", "plain", or unset for per-terminal defaults.
    pub borders: Option<String>,
    /// Start with dotfiles visible. Defaults to true.
    pub show_hidden: Option<bool>,
    /// Directory both panes open in when cian is started with no path
    /// argument. Unset falls back to the Desktop, then the working directory.
    pub home: Option<String>,
    /// Interface language for the key manual and help text: "ja" (default) or
    /// "en".
    pub lang: Option<String>,
    /// External editor for `E` in the viewer / `:edit`. A command line, e.g.
    /// "nvim" or "code -w". Unset falls back to $VISUAL/$EDITOR, then nvim →
    /// vim → vi on PATH.
    pub editor: Option<String>,
}

/// A login on a host.
///
/// `password` is stored exactly as written in `init.lua`. That is a plaintext
/// secret in a file, with everything that implies — it is opt-in, per user, and
/// `password_cmd` exists so the value can come from a credential store instead
/// without changing anything else. cian never logs or displays either.
#[derive(Clone)]
pub struct SshUser {
    pub name: String,
    pub password: Option<String>,
    /// Shell command whose stdout is the password (trailing newline trimmed).
    pub password_cmd: Option<String>,
}

impl SshUser {
    pub fn plain(name: impl Into<String>) -> Self {
        Self { name: name.into(), password: None, password_cmd: None }
    }

    pub fn has_secret(&self) -> bool {
        self.password.is_some() || self.password_cmd.is_some()
    }

    /// Resolve the secret to send, running `password_cmd` if that is the source.
    pub fn secret(&self) -> Option<String> {
        if let Some(p) = &self.password {
            return Some(p.clone());
        }
        let cmd = self.password_cmd.as_ref()?;
        let out = if cfg!(windows) {
            Command::new("cmd").args(["/C", cmd]).output().ok()?
        } else {
            Command::new("sh").arg("-c").arg(cmd).output().ok()?
        };
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).trim_end_matches(['\r', '\n']).to_string())
    }
}

/// Never let a secret reach a log line or a panic message.
impl std::fmt::Debug for SshUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshUser")
            .field("name", &self.name)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field("password_cmd", &self.password_cmd.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// One SSH target: a host plus the users worth offering for it.
#[derive(Debug, Clone)]
pub struct SshHost {
    /// Label shown in the picker.
    pub name: String,
    /// Hostname or address passed to `ssh`.
    pub host: String,
    pub users: Vec<SshUser>,
    pub port: Option<u16>,
    /// Free-text facts about this server (OS, installed middleware, versions),
    /// fed to the AI as context when a shell is connected here. `None` = none.
    pub notes: Option<String>,
}

/// AI settings from `cian.ai{...}`. Presence enables the (optional) AI
/// features; the TUI still verifies the helper actually works before showing
/// them. Fields mirror crmaine's backend so the same Azure endpoint is reached.
#[derive(Debug, Clone)]
pub struct AiOptions {
    pub python: String,
    pub endpoint: String,
    pub model: String,
    pub api_version: String,
    pub auth_mode: String,
    pub api_key: String,
    pub api_base_url: String,
}

impl Default for AiOptions {
    fn default() -> Self {
        Self {
            python: "python".into(),
            // Same Azure APIM endpoint / model family as crmaine, so `cian.ai{}`
            // with no fields works in that environment.
            endpoint: "https://apim-jri-dev-apim1.azure-api.net/llmoai".into(),
            model: "gpt-5.4".into(),
            api_version: "2025-04-01-preview".into(),
            auth_mode: "broker".into(),
            api_key: String::new(),
            api_base_url: String::new(),
        }
    }
}

/// Mutable accumulator shared with the Lua callbacks during script execution.
#[derive(Default)]
struct Builder {
    theme: Theme,
    options: Options,
    keymaps: Vec<(char, String)>,
    ext_open: HashMap<String, Function>,
    ssh_hosts: Vec<SshHost>,
    ai: Option<AiOptions>,
    /// Precondition facts about the environment, fed to every AI prompt.
    ai_context: Vec<String>,
    errors: Vec<String>,
}

/// Fully-parsed configuration.
///
/// Owns the Lua runtime so `ext_open` callbacks — and the helpers they call,
/// like `cian.spawn` — stay valid for the whole life of the app.
#[derive(Default)]
pub struct Config {
    pub theme: Theme,
    pub options: Options,
    /// `(key, action-name)` pairs the user explicitly bound. The UI validates
    /// the action names and reports any it does not recognise.
    pub keymaps: Vec<(char, String)>,
    /// SSH targets declared with `cian.ssh{...}`.
    pub ssh_hosts: Vec<SshHost>,
    /// AI settings declared with `cian.ai{...}`, if any.
    pub ai: Option<AiOptions>,
    /// Precondition facts declared with `cian.ai_context{...}`, prepended to
    /// every AI prompt so answers assume the user's actual environment.
    pub ai_context: Vec<String>,
    /// Non-fatal problems collected while loading (surfaced in a notice popup).
    pub errors: Vec<String>,
    ext_open: HashMap<String, Function>,
    /// Held purely to keep the Lua runtime (and thus every `ext_open` handle and
    /// helper) alive for the app's lifetime. Never read directly.
    #[allow(dead_code)]
    _lua: Option<Lua>,
}

impl Config {
    /// Does the user have a handler registered for this (lower-cased) extension?
    pub fn has_ext_open(&self, ext: &str) -> bool {
        self.ext_open.contains_key(&ext.to_lowercase())
    }

    /// Invoke the user's handler for `ext`, passing the file path as a string.
    /// Returns `None` if no handler is registered.
    pub fn run_ext_open(&self, ext: &str, path: &Path) -> Option<Result<(), String>> {
        let f = self.ext_open.get(&ext.to_lowercase())?;
        let arg = path.to_string_lossy().into_owned();
        Some(f.call::<()>(arg).map_err(|e| e.to_string()))
    }
}

/// The user's home directory: `$HOME`, or `$USERPROFILE` on Windows.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// The cian config files that mark a directory as a cian config directory. Used
/// for portable-mode detection: if any of these sits next to the executable,
/// that directory is treated as a portable config set.
const CONFIG_MARKERS: [&str; 3] = ["init.lua", "shortcuts.lua", "macro.lua"];

/// The directory the running executable lives in, resolved through symlinks.
fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(|p| p.to_path_buf())
}

/// The user config directory: `$CIAN_CONFIG_DIR` if set, else `~/.config/cian`.
pub fn user_config_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("CIAN_CONFIG_DIR") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    Some(home_dir()?.join(".config").join("cian"))
}

/// True when the executable's directory holds a cian config file — i.e. cian is
/// being run "portable", carried around with its config next to it.
pub fn is_portable() -> bool {
    exe_dir()
        .map(|d| CONFIG_MARKERS.iter().any(|m| d.join(m).exists()))
        .unwrap_or(false)
}

/// Where to **read** config file `name` from. A copy sitting next to the
/// executable wins (portable: carry cian + its `*.lua` on a stick and they take
/// precedence); otherwise the user config directory.
pub fn config_read_path(name: &str) -> Option<PathBuf> {
    read_path_in(exe_dir().as_deref(), user_config_dir().as_deref(), name)
}

/// Where to **write** config file `name`. Next to the executable when a copy is
/// already there, or when the executable directory is a portable config set
/// (so bookmarks/macros created in portable mode stay with the binary);
/// otherwise the user config directory.
pub fn config_write_path(name: &str) -> Option<PathBuf> {
    write_path_in(exe_dir().as_deref(), user_config_dir().as_deref(), name)
}

/// The read-resolution logic, split out from the executable lookup so it can be
/// tested against real directories: the portable copy next to the executable
/// wins only if it actually exists, else the user directory.
fn read_path_in(exe_dir: Option<&Path>, user_dir: Option<&Path>, name: &str) -> Option<PathBuf> {
    if let Some(dir) = exe_dir {
        let p = dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    user_dir.map(|d| d.join(name))
}

/// The write-resolution logic (see [`read_path_in`]): next to the executable if
/// that file is already there or the directory is a portable config set, else
/// the user directory.
fn write_path_in(exe_dir: Option<&Path>, user_dir: Option<&Path>, name: &str) -> Option<PathBuf> {
    if let Some(dir) = exe_dir {
        let here = dir.join(name);
        if here.exists() || CONFIG_MARKERS.iter().any(|m| dir.join(m).exists()) {
            return Some(here);
        }
    }
    user_dir.map(|d| d.join(name))
}

/// Resolve `init.lua`: the portable copy next to the executable if present,
/// otherwise `$CIAN_CONFIG_DIR`/`~/.config/cian`.
pub fn config_path() -> Option<PathBuf> {
    config_read_path("init.lua")
}

/// Load the configuration. Never panics and never returns an error: anything
/// that goes wrong is recorded in [`Config::errors`] and defaults are used.
/// Turn Lua's terse "invalid escape sequence" into advice.
///
/// Pasting a Windows path straight into a quoted string — the natural thing to
/// do for `shell` — makes `\W`, `\S`, `\v` … look like escape sequences, and
/// the resulting syntax error takes the *whole* config down with it. The raw
/// message gives no hint that backslashes are the problem, so spell it out.
fn escape_hint(err: &str) -> Vec<String> {
    if !err.contains("invalid escape sequence") {
        return Vec::new();
    }
    vec![
        "  hint: a backslash starts an escape sequence in Lua, so a Windows".into(),
        "  path cannot be pasted into \"...\" as-is. Use [[...]] instead:".into(),
        "    cian.set_option(\"shell\", [[C:\\path\\to\\shell.exe]])".into(),
    ]
}

pub fn load() -> Config {
    match config_path() {
        Some(p) if p.exists() => {
            let mut c = load_from(&p);
            // Only worth saying if the file actually holds a secret.
            if c.ssh_hosts.iter().any(|h| h.users.iter().any(|u| u.password.is_some())) {
                c.errors.extend(permission_warning(&p));
            }
            c
        }
        _ => Config::default(),
    }
}

/// Warn when a config holding plaintext passwords is readable by anyone else
/// on the machine. Storing them is the user's call; leaving them world-readable
/// is almost never intended.
#[cfg(unix)]
fn permission_warning(path: &Path) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    let Ok(meta) = std::fs::metadata(path) else { return Vec::new() };
    let mode = meta.permissions().mode() & 0o077;
    if mode == 0 {
        return Vec::new();
    }
    vec![
        format!("init.lua holds SSH passwords but is readable by others (mode {:o}).", meta.permissions().mode() & 0o777),
        format!("  fix: chmod 600 {}", path.display()),
    ]
}

#[cfg(not(unix))]
fn permission_warning(_path: &Path) -> Vec<String> {
    // Windows ACLs are not a mode bitmask; a meaningful check would need the
    // security API, and a wrong warning is worse than none.
    Vec::new()
}

fn load_from(path: &Path) -> Config {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return Config {
                errors: vec![format!("cannot read {}: {}", path.display(), e)],
                ..Config::default()
            };
        }
    };

    let lua = Lua::new();
    let builder = Rc::new(RefCell::new(Builder::default()));

    if let Err(e) = install_api(&lua, &builder) {
        return Config {
            errors: vec![format!("failed to set up Lua API: {}", e)],
            ..Config::default()
        };
    }

    let mut errors = Vec::new();
    if let Err(e) = lua.load(&src).set_name("init.lua").exec() {
        errors.push(format!("init.lua: {}", e));
        errors.extend(escape_hint(&e.to_string()));
    }

    // Pull the accumulated config out by cloning; the Lua handles stay valid
    // because we move `lua` into the returned Config below.
    let (theme, options, keymaps, ext_open, ssh_hosts, ai, ai_context, builder_errors) = {
        let b = builder.borrow();
        (
            b.theme.clone(),
            b.options.clone(),
            b.keymaps.clone(),
            b.ext_open.clone(),
            b.ssh_hosts.clone(),
            b.ai.clone(),
            b.ai_context.clone(),
            b.errors.clone(),
        )
    };
    errors.extend(builder_errors);

    Config {
        theme,
        options,
        keymaps,
        ext_open,
        ssh_hosts,
        ai,
        ai_context,
        errors,
        _lua: Some(lua),
    }
}

/// Parse a `users` list, where each entry is either a bare name or a table
/// carrying credentials:
///
/// ```lua
/// users = { "root", { name = "deploy", password = "..." } }
/// ```
///
/// Mixing the two matters: it lets a host move to key auth one login at a
/// time, dropping the stored secret as each one is migrated.
fn parse_users(t: Option<Table>) -> mlua::Result<Vec<SshUser>> {
    let Some(t) = t else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for v in t.sequence_values::<Value>() {
        match v? {
            Value::String(s) => out.push(SshUser::plain(s.to_str()?.to_owned())),
            Value::Table(u) => {
                let Some(name) = u.get::<Option<String>>("name")? else { continue };
                out.push(SshUser {
                    name,
                    password: u.get::<Option<String>>("password")?,
                    password_cmd: u.get::<Option<String>>("password_cmd")?,
                });
            }
            _ => {}
        }
    }
    Ok(out)
}

fn install_api(lua: &Lua, builder: &Rc<RefCell<Builder>>) -> mlua::Result<()> {
    let cian = lua.create_table()?;

    // cian.set_theme "solarized-light"   -- pick a preset, or
    // cian.set_theme { preset = "solarized-light", accent = "#...", ... }
    {
        let b = builder.clone();
        cian.set(
            "set_theme",
            lua.create_function(move |_, v: mlua::Value| {
                let mut bm = b.borrow_mut();
                // A bare string is shorthand for choosing a preset.
                let t = match v {
                    mlua::Value::String(s) => {
                        bm.theme.preset = Some(s.to_str()?.to_string());
                        return Ok(());
                    }
                    mlua::Value::Table(t) => t,
                    other => {
                        return Err(mlua::Error::runtime(format!(
                            "set_theme expects a table or a preset name, got {}",
                            other.type_name()
                        )))
                    }
                };
                if let Some(v) = t.get::<Option<String>>("preset")? {
                    bm.theme.preset = Some(v);
                }
                if let Some(v) = t.get::<Option<String>>("accent")? {
                    bm.theme.accent = Some(v);
                }
                if let Some(v) = t.get::<Option<String>>("status_bg")? {
                    bm.theme.status_bg = Some(v);
                }
                if let Some(v) = t.get::<Option<String>>("selected_bg")? {
                    bm.theme.selected_bg = Some(v);
                }
                if let Some(v) = t.get::<Option<String>>("visual_bg")? {
                    bm.theme.visual_bg = Some(v);
                }
                if let Some(v) = t.get::<Option<String>>("mark_fg")? {
                    bm.theme.mark_fg = Some(v);
                }
                Ok(())
            })?,
        )?;
    }

    // cian.set_keymap("x", "delete")
    {
        let b = builder.clone();
        cian.set(
            "set_keymap",
            lua.create_function(move |_, (key, action): (String, String)| {
                let mut bm = b.borrow_mut();
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => bm.keymaps.push((c, action)),
                    _ => bm.errors.push(format!(
                        "set_keymap: key must be a single character, got {:?}",
                        key
                    )),
                }
                Ok(())
            })?,
        )?;
    }

    // cian.set_option("clipboard_on_copy", false)
    {
        let b = builder.clone();
        cian.set(
            "set_option",
            lua.create_function(move |lua, (name, val): (String, Value)| {
                let mut bm = b.borrow_mut();
                match name.as_str() {
                    "clipboard_on_copy" => match bool::from_lua(val, lua) {
                        Ok(v) => bm.options.clipboard_on_copy = Some(v),
                        Err(_) => bm
                            .errors
                            .push("set_option: clipboard_on_copy expects a boolean".into()),
                    },
                    // Removed: `mask` was only ever displayed, never applied.
                    // Name it explicitly so an old config gets told what to do
                    // instead of a bare "unknown option".
                    "mask" => bm.errors.push(
                        "set_option: `mask` was removed — it never filtered anything. \
                         Press `/` in the app to narrow the listing instead."
                            .into(),
                    ),
                    "shell" => match String::from_lua(val, lua) {
                        Ok(v) => bm.options.shell = Some(v),
                        Err(_) => {
                            bm.errors.push("set_option: shell expects a string".into())
                        }
                    },
                    "borders" => match String::from_lua(val, lua) {
                        Ok(v) => bm.options.borders = Some(v),
                        Err(_) => bm
                            .errors
                            .push("set_option: borders expects \"rounded\" or \"plain\"".into()),
                    },
                    "show_hidden" => match bool::from_lua(val, lua) {
                        Ok(v) => bm.options.show_hidden = Some(v),
                        Err(_) => bm
                            .errors
                            .push("set_option: show_hidden expects a boolean".into()),
                    },
                    "home" => match String::from_lua(val, lua) {
                        Ok(v) => bm.options.home = Some(v),
                        Err(_) => {
                            bm.errors.push("set_option: home expects a directory path".into())
                        }
                    },
                    "editor" => match String::from_lua(val, lua) {
                        Ok(v) => bm.options.editor = Some(v),
                        Err(_) => {
                            bm.errors.push("set_option: editor expects a command string".into())
                        }
                    },
                    "key_hints" => match bool::from_lua(val, lua) {
                        Ok(v) => bm.options.key_hints = Some(v),
                        Err(_) => bm
                            .errors
                            .push("set_option: key_hints expects a boolean".into()),
                    },
                    "animation_ms" => match u64::from_lua(val, lua) {
                        Ok(v) => bm.options.animation_ms = Some(v),
                        Err(_) => bm
                            .errors
                            .push("set_option: animation_ms expects a number".into()),
                    },
                    "lang" => match String::from_lua(val, lua) {
                        Ok(v) if v == "ja" || v == "en" => bm.options.lang = Some(v),
                        Ok(_) => bm
                            .errors
                            .push("set_option: lang expects \"ja\" or \"en\"".into()),
                        Err(_) => {
                            bm.errors.push("set_option: lang expects \"ja\" or \"en\"".into())
                        }
                    },
                    other => bm
                        .errors
                        .push(format!("set_option: unknown option {:?}", other)),
                }
                Ok(())
            })?,
        )?;
    }

    // cian.ssh { users = {...}, hosts = { { name=, host=, users= }, ... } }
    {
        let b = builder.clone();
        cian.set(
            "ssh",
            lua.create_function(move |_, t: Table| {
                let mut bm = b.borrow_mut();
                // Fleet-wide default, overridable per host.
                let default_users = parse_users(t.get::<Option<Table>>("users")?)?;
                let hosts: Vec<Table> = match t.get::<Option<Vec<Table>>>("hosts")? {
                    Some(v) => v,
                    None => {
                        bm.errors.push("cian.ssh: expected a `hosts` list".into());
                        return Ok(());
                    }
                };
                for h in hosts {
                    let host: String = match h.get::<Option<String>>("host")? {
                        Some(v) => v,
                        None => {
                            bm.errors.push("cian.ssh: a host entry is missing `host`".into());
                            continue;
                        }
                    };
                    // `name` is what the picker shows; default to the address.
                    let name = h.get::<Option<String>>("name")?.unwrap_or_else(|| host.clone());
                    let users = match h.get::<Option<Table>>("users")? {
                        Some(t) => parse_users(Some(t))?,
                        None => default_users.clone(),
                    };
                    if users.is_empty() {
                        bm.errors.push(format!(
                            "cian.ssh: host {:?} has no users (set `users` here or at the top level)",
                            name
                        ));
                        continue;
                    }
                    let port = h.get::<Option<u16>>("port")?;
                    let notes = h.get::<Option<String>>("notes")?.filter(|s| !s.trim().is_empty());
                    bm.ssh_hosts.push(SshHost { name, host, users, port, notes });
                }
                Ok(())
            })?,
        )?;
    }

    // cian.ai { endpoint=, model=, auth_mode=, python=, api_version=, api_key=, api_base_url= }
    {
        let b = builder.clone();
        cian.set(
            "ai",
            lua.create_function(move |_, t: Table| {
                let mut ai = AiOptions::default();
                let get = |k: &str| -> Option<String> { t.get::<Option<String>>(k).ok().flatten() };
                if let Some(v) = get("python") { ai.python = v; }
                if let Some(v) = get("endpoint") { ai.endpoint = v; }
                if let Some(v) = get("model") { ai.model = v; }
                if let Some(v) = get("api_version") { ai.api_version = v; }
                if let Some(v) = get("auth_mode") { ai.auth_mode = v; }
                if let Some(v) = get("api_key") { ai.api_key = v; }
                if let Some(v) = get("api_base_url") { ai.api_base_url = v; }
                b.borrow_mut().ai = Some(ai);
                Ok(())
            })?,
        )?;
    }

    // cian.ai_context("fact")  or  cian.ai_context{ "fact one", "fact two" }
    //
    // Precondition facts the AI should assume — e.g. the OS the file panes
    // browse, the deployment target, house conventions. Prepended to every AI
    // prompt. Additive across calls.
    {
        let b = builder.clone();
        cian.set(
            "ai_context",
            lua.create_function(move |_, v: Value| {
                let mut bm = b.borrow_mut();
                match v {
                    Value::String(s) => {
                        let f = s.to_str()?.trim().to_string();
                        if !f.is_empty() {
                            bm.ai_context.push(f);
                        }
                    }
                    Value::Table(t) => {
                        for item in t.sequence_values::<String>() {
                            let f = item?.trim().to_string();
                            if !f.is_empty() {
                                bm.ai_context.push(f);
                            }
                        }
                    }
                    other => {
                        bm.errors.push(format!(
                            "cian.ai_context: expected a string or a list of strings, got {}",
                            other.type_name()
                        ));
                    }
                }
                Ok(())
            })?,
        )?;
    }

    // cian.on_open("md", function(path) ... end)
    {
        let b = builder.clone();
        cian.set(
            "on_open",
            lua.create_function(move |_, (ext, f): (String, Function)| {
                let key = ext.trim_start_matches('.').to_lowercase();
                b.borrow_mut().ext_open.insert(key, f);
                Ok(())
            })?,
        )?;
    }

    // cian.spawn({ "nvim", path }) — launch a detached process.
    cian.set(
        "spawn",
        lua.create_function(|_, args: Vec<String>| {
            if args.is_empty() {
                return Err(mlua::Error::RuntimeError("cian.spawn: empty command".into()));
            }
            Command::new(&args[0])
                .args(&args[1..])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| mlua::Error::RuntimeError(format!("cian.spawn: {}", e)))?;
            Ok(())
        })?,
    )?;

    // cian.open(path) — hand a path/URL to the OS default opener.
    cian.set(
        "open",
        lua.create_function(|_, target: String| {
            os_open(&target)
                .map_err(|e| mlua::Error::RuntimeError(format!("cian.open: {}", e)))?;
            Ok(())
        })?,
    )?;

    lua.globals().set("cian", cian)?;
    Ok(())
}

fn os_open(target: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(target_os = "linux")]
    let mut cmd = Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg("start").arg("");
        c
    };
    cmd.arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_context_and_host_notes_parse() {
        let dir = tempfile::tempdir().unwrap();
        let init = dir.path().join("init.lua");
        std::fs::write(
            &init,
            r#"
            cian.ai_context("The file panes browse a RHEL 8 NFS mount.")
            cian.ai_context{ "Prefer POSIX sh.", "Deploy target is Oracle 19c." }
            cian.ssh{
              users = { "root" },
              hosts = {
                { name = "db", host = "10.0.0.9", notes = "RHEL 8, Oracle 19c, nginx 1.24" },
                { name = "web", host = "10.0.0.10" },
              },
            }
            "#,
        )
        .unwrap();
        let cfg = load_from(&init);
        assert!(cfg.errors.is_empty(), "{:?}", cfg.errors);
        assert_eq!(
            cfg.ai_context,
            vec![
                "The file panes browse a RHEL 8 NFS mount.",
                "Prefer POSIX sh.",
                "Deploy target is Oracle 19c.",
            ]
        );
        assert_eq!(cfg.ssh_hosts[0].notes.as_deref(), Some("RHEL 8, Oracle 19c, nginx 1.24"));
        assert_eq!(cfg.ssh_hosts[1].notes, None);
    }

    #[test]
    fn shipped_example_init_parses_cleanly() {
        // The template users copy must always load without errors — the AI
        // context block and every other snippet in it stay valid Lua.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/init.lua");
        if !path.exists() {
            eprintln!("example init.lua not found at {}; skipping", path.display());
            return;
        }
        let cfg = load_from(&path);
        assert!(cfg.errors.is_empty(), "{:?}", cfg.errors);
    }

    #[test]
    fn ai_context_rejects_a_number() {
        let dir = tempfile::tempdir().unwrap();
        let init = dir.path().join("init.lua");
        std::fs::write(&init, "cian.ai_context(42)").unwrap();
        let cfg = load_from(&init);
        assert!(cfg.ai_context.is_empty());
        assert!(cfg.errors.iter().any(|e| e.contains("ai_context")), "{:?}", cfg.errors);
    }

    #[test]
    fn portable_copy_next_to_exe_wins_for_reading() {
        let exe = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        // No file next to the exe yet → read resolves to the user dir.
        assert_eq!(
            read_path_in(Some(exe.path()), Some(user.path()), "init.lua"),
            Some(user.path().join("init.lua"))
        );
        // Drop a copy next to the exe → it now wins.
        std::fs::write(exe.path().join("init.lua"), "-- portable").unwrap();
        assert_eq!(
            read_path_in(Some(exe.path()), Some(user.path()), "init.lua"),
            Some(exe.path().join("init.lua"))
        );
    }

    #[test]
    fn writes_go_beside_the_exe_only_in_portable_mode() {
        let exe = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        // Nothing portable next to the exe → a new file is written to the user dir.
        assert_eq!(
            write_path_in(Some(exe.path()), Some(user.path()), "shortcuts.lua"),
            Some(user.path().join("shortcuts.lua"))
        );
        // Once init.lua sits beside the exe, the directory is a portable set, so
        // a freshly-created shortcuts.lua stays with the binary.
        std::fs::write(exe.path().join("init.lua"), "-- portable").unwrap();
        assert_eq!(
            write_path_in(Some(exe.path()), Some(user.path()), "shortcuts.lua"),
            Some(exe.path().join("shortcuts.lua"))
        );
    }

    #[test]
    fn an_existing_file_beside_the_exe_is_written_in_place() {
        let exe = tempfile::tempdir().unwrap();
        let user = tempfile::tempdir().unwrap();
        std::fs::write(exe.path().join("shortcuts.lua"), "return {}").unwrap();
        // Even without any other marker, an existing copy keeps being written there.
        assert_eq!(
            write_path_in(Some(exe.path()), Some(user.path()), "shortcuts.lua"),
            Some(exe.path().join("shortcuts.lua"))
        );
    }

    /// A Windows path pasted into a quoted string is the likeliest way for a
    /// config to fail, and Lua's own message never mentions backslashes.
    #[test]
    fn an_invalid_escape_gets_a_windows_path_hint() {
        let msg = r#"syntax error: [string "init.lua"]:1: invalid escape sequence near '"C:\W'"#;
        let hint = escape_hint(msg);
        assert!(!hint.is_empty(), "should explain the backslash problem");
        let joined = hint.join("\n");
        assert!(joined.contains("[[...]]"), "should show the fix: {}", joined);
        assert!(joined.contains("escape sequence"), "should name the cause");
    }

    #[test]
    fn unrelated_errors_get_no_hint() {
        assert!(escape_hint("attempt to call a nil value").is_empty());
        assert!(escape_hint(r#"set_option: unknown option "nope""#).is_empty());
    }
}
