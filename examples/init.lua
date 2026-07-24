-- ============================================================================
--  cian — configuration
-- ============================================================================
--
--  Everything below is COMMENTED OUT. Delete the leading "-- " on any line to
--  turn that setting on. With nothing uncommented, cian runs on its defaults.
--
--  Where this file goes:
--    Linux / macOS : ~/.config/cian/init.lua
--    Windows       : %USERPROFILE%\.config\cian\init.lua
--                    (e.g. C:\Users\you\.config\cian\init.lua)
--    Or set $CIAN_CONFIG_DIR to point somewhere else; cian reads
--    $CIAN_CONFIG_DIR/init.lua then.
--
--  It is plain Lua 5.4, so you can use variables, functions and `if` around
--  any of this (e.g. a different shell per machine). Mistakes are never fatal:
--  cian collects them and shows a notice on startup instead of refusing to run.
--
--  See the full key list any time with `?` in the app, `:man`, or `cian -man`.
-- ============================================================================


-- ----------------------------------------------------------------------------
--  THEME
-- ----------------------------------------------------------------------------
-- Start from a named preset. Built in: "solarized-light" (aka "solarized"),
-- and "default" / "dark" (the built-in dark theme, which is the default).
-- cian.set_theme "solarized-light"

-- …or tune individual colors. Values are "#rrggbb" or a color name
-- ("yellow", "cyan", …). Any key you leave out keeps its current value, and a
-- preset can be combined with overrides:
-- cian.set_theme {
--   preset      = "solarized-light",  -- optional base to start from
--   accent      = "#268bd2",  -- focused borders, highlights, the bar
--   status_bg   = "#eee8d5",  -- background of the bottom status line
--   selected_bg = "#dcd5be",  -- highlight behind the selected row
--   visual_bg   = "#f7e4b0",  -- highlight while marking in visual mode
--   mark_fg     = "#cb4b16",  -- color of the ● on marked files
-- }


-- ----------------------------------------------------------------------------
--  OPTIONS  —  cian.set_option(name, value)
-- ----------------------------------------------------------------------------

-- The directory both panes open in when cian is started with no path argument.
-- Defaults to the Desktop, then your home folder. Handy when launched from a
-- shortcut, where the working directory would otherwise be wherever the exe is.
-- cian.set_option("home", "C:\\Users\\you\\Desktop")   -- Windows
-- cian.set_option("home", "~/Desktop")                 -- Linux / macOS (~ and $VARS expand)

-- Which shell runs in the embedded shell panel. Defaults to $SHELL / %COMSPEC%.
-- cian.set_option("shell", "powershell.exe")
-- cian.set_option("shell", "pwsh.exe")     -- PowerShell 7
-- cian.set_option("shell", "/bin/zsh")

-- Also put files on the SYSTEM clipboard when you copy with `y` (so they paste
-- in Explorer / Finder too). Default: true.
-- cian.set_option("clipboard_on_copy", true)

-- Show dotfiles (names starting with ".") on startup. Toggle live with the
-- right-click menu. Default: true.
-- cian.set_option("show_hidden", true)

-- Border corners: "rounded" (╭╮╯╰) or "plain" (square). Unset auto-picks —
-- rounded where the terminal/font can render them, square in the legacy
-- Windows console. Force "plain" if the corners look misaligned.
-- cian.set_option("borders", "rounded")

-- The contextual key-hint bar above the status line. Default: true. Set false
-- for a cleaner screen once the keys are muscle memory.
-- cian.set_option("key_hints", true)

-- Split / zoom / close animation length, in milliseconds. 0 disables all
-- animation. Default is a short, snappy transition.
-- cian.set_option("animation_ms", 120)
-- cian.set_option("animation_ms", 0)      -- off


-- ----------------------------------------------------------------------------
--  EXTRA KEY BINDINGS  —  cian.set_keymap("key", "action")
-- ----------------------------------------------------------------------------
-- Binds an ADDITIONAL single key to a built-in action; the default keys keep
-- working too. `key` is one character. Example: also delete with "x".
-- cian.set_keymap("x", "delete")
-- cian.set_keymap("e", "open_external")   -- open with the OS default app
--
-- Every action name you can bind:
--   Movement : cursor_down  cursor_up  cursor_bottom  page_down  page_up
--              parent  enter
--   Marking  : mark_down  mark_up  invert_marks  visual
--   Files    : copy  move  delete  rename  new_file  new_dir
--   Panes    : open_other  open_other_tab
--   Open     : open_external           (hand the file to the OS opener)
--   Clipboard: copy_path  copy_file_ref
--   Find     : search  search_next  search_prev
--   Misc     : history  shortcuts  command  quit


-- ----------------------------------------------------------------------------
--  SSH HOSTS  —  cian.ssh { ... }
-- ----------------------------------------------------------------------------
-- Populates the SSH picker (right-click → SSH connect, or in the shell) and the
-- file upload/download flow. Transfers use SFTP, falling back to the classic
-- SCP protocol automatically when the server has no SFTP subsystem; the status
-- line shows which one carried it. `users` at the top level is the fleet-wide default;
-- a host can override it. A user is either a bare name, or a table carrying its
-- password so cian can log in for you.
--
-- SECURITY NOTE: a plain `password` here is stored in clear text in this file.
-- Prefer `password_cmd`, which runs a command and uses its stdout — so the
-- secret lives in your OS credential store, not on disk.
--
-- cian.ssh {
--   -- Applied to every host that doesn't list its own users:
--   users = { "root", "deploy" },
--
--   hosts = {
--     -- Simplest: a name for the picker and an address. Uses the default users.
--     { name = "web1", host = "10.0.1.11" },
--
--     -- A non-standard port:
--     { name = "db1",  host = "10.0.2.31", port = 2222 },
--
--     -- Per-host users, one with a password typed in for auto-login:
--     { name = "stage", host = "stage.example.com",
--       users = {
--         "readonly",                                    -- prompts, no auto-login
--         { name = "admin", password = "hunter2" },      -- clear text (avoid)
--         { name = "ci",    password_cmd = "pass show ci/stage" },  -- from a store
--       },
--     },
--   },
-- }


-- ----------------------------------------------------------------------------
--  OPEN HANDLERS & HELPERS
-- ----------------------------------------------------------------------------
-- What Enter (or `open_external`) does for a given extension. The handler gets
-- the full path. Use cian.spawn to launch a program detached, or cian.open to
-- hand the path to the OS default opener.
--
-- cian.on_open("md", function(path)
--   cian.spawn { "code", path }          -- open Markdown in VS Code
-- end)
--
-- cian.on_open("png", function(path)
--   cian.open(path)                      -- let the OS pick the image viewer
-- end)
--
-- cian.on_open("csv", function(path)
--   cian.spawn { "nvim", path }
-- end)


-- ----------------------------------------------------------------------------
--  A note on shortcuts
-- ----------------------------------------------------------------------------
-- Bookmarks (the `s` menu) are managed inside the app — press `a` there, or `a`
-- on a path in the history list — and saved to shortcuts.toml next to this file.
-- There is nothing to configure here for them.
