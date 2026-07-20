# cian

**C**omfortable **I**nterface for **A**gile File e**X**plorer **N**avigation —
a modern two-pane terminal file manager inspired by [AFXW (あふｗ)](https://akt.d.dooo.jp/akt_afxw.htm).

Runs in any terminal (designed to be used as WezTerm's `default_prog`).
Cross-platform: macOS / Windows / Linux.

## Status

Early development. Working: two-pane navigation, marks/visual selection, file
operations (copy/move/delete/rename/create), incremental filtering, history,
shortcuts, search, clipboard integration, Lua configuration, and an embedded
PTY shell panel.

## Help

Press **`?`** (or `Ctrl+.`, `:man`, or **right-click → Key manual**) inside
cian for the full key manual —
it is generated from the live keymap, so it also lists any keys you bound in
`init.lua`. From a shell, `cian -man` prints the same thing and `cian -h`
prints the command-line usage.

## Mouse

Drag any border to re-proportion the split it divides — the two file panes, the
file/shell divider, and every split inside the shell panel. Neither side can be
dragged below 15% of its parent.

Right-click a pane for a context menu: copy, cut, paste, copy/move to the other
pane, rename, delete, a per-pane background colour, and the key manual (the
last two are offered in the shell's menu too).

Background colours apply to whichever pane you right-clicked, the shell panel
included. In the shell the tint only fills cells the shell left uncoloured, so
`ls` colours and editor themes come through untouched. Copy and cut fill a file
clipboard that persists while you navigate, so you can copy here, move
somewhere else, and paste there — the system clipboard is separate and is still
driven by `p` / `Shift+P`. Background colours are session-only.

## Animation

Splitting, maximizing (`F12`) and closing a pane animate over 150ms. PTYs are
resized once, when the transition lands, so the shell never reflows mid-flight.
Any keypress lands the transition immediately — input is never held up by it.
Tune or disable it:

```lua
cian.set_option("animation_ms", 250)   -- slower
cian.set_option("animation_ms", 0)     -- off
```

## Deleting

`d` moves items to the OS trash (Finder's Trash / the Windows Recycle Bin), so
a mistake is recoverable. The confirmation popup offers `a` to delete
permanently instead.

## SSH

`Shift+S` (or `:ssh`) opens a two-stage picker: choose a host, then a user on
it. Typing in the host stage filters. Hosts with a single user connect
straight away. The command is typed into the shell panel, so your own shell
config and agent apply, and the tab drops back to a local prompt when the
session ends.

```lua
cian.ssh({
  users = { "root", "deploy", "app", "taketan" },   -- offered for every host
  hosts = {
    { name = "web1", host = "10.0.1.11" },
    { name = "db1",  host = "10.0.2.31", users = { "postgres", "root" } },
    { name = "bast", host = "203.0.113.9", port = 2222 },
  },
})
```

Eight hosts times four users is a dozen lines here instead of 32 aliases to
remember — the picker does the remembering.

### Passwords

A login can carry a password, which cian types when ssh asks for one:

```lua
users = {
  { name = "postgres", password = "..." },        -- stored in this file
  { name = "deploy", password_cmd = "pass srv/deploy" },  -- from a credential store
  "root",                                          -- key auth; nothing stored
}
```

ssh reads the password from its controlling terminal rather than stdin, so it
cannot be piped in — but cian owns that terminal, so it writes to the PTY when
the prompt appears. This is what TeraTerm's `.ttl` macros do, and expect(1)
before them. cian waits for the prompt rather than sending blindly, so a host
on key auth simply never receives anything and the attempt expires after 20
seconds. A host-key confirmation is never answered automatically.

The password is never logged (including under `CIAN_LOG`), never shown in the
status bar, and redacted from debug output.

**Understand the trade.** `password` puts a plaintext secret in a file that
gets backed up, copied between machines, and shared more readily than its
contents deserve. On Unix, cian warns at startup if such a file is readable by
anyone else. `password_cmd` avoids storing anything by taking the value from a
credential manager. Key authentication avoids the question entirely and is
usually less work to set up than a credential list is to maintain.

## Sorting

`,` opens the sort picker: name, size, date or extension, with `n`/`s`/`d`/`e`
as direct shortcuts. Choosing the key that is already active reverses it, the
way a column header does. Directories always stay at the top regardless — a
size sort that scattered folders through the listing would make the pane much
harder to navigate. The order is per-pane and shown in the status bar
(`size ▼`).

## Key hints

A bar above the status line lists the keys that apply right now, and changes
with the mode. Turn it off with `cian.set_option("key_hints", false)`; it also
yields automatically on a short window.

## Filtering

`/` narrows the listing as you type (case-insensitive substring). **Enter**
keeps the filter applied so you can mark and operate on just the matches;
**Esc** clears it. The status bar shows the active filter and how many of the
directory's entries it matches, so a narrowed pane never looks like a full one.
Changing directory always clears the filter.

## Architecture

Cargo workspace, split into five crates:

| Crate | Role |
|---|---|
| `cian-core` | Pure domain logic: file ops, marks, history, sorting, filtering, search |
| `cian-tui`  | Rendering & input (ratatui + crossterm), layout, popups |
| `cian-pty`  | Embedded shell pane (portable-pty + alacritty_terminal) |
| `cian-lua`  | Lua configuration host (mlua): keymaps, themes, ext-open DSL |
| `cian-bin`  | Entry point — produces the `cian` binary |

## Configuration

cian reads `~/.config/cian/init.lua` (override the directory with
`$CIAN_CONFIG_DIR`). Configuration is written in Lua via a small WezTerm-style
API on the global `cian` table:

```lua
cian.set_theme({ accent = "#00d7d7", mark_fg = "yellow" })
cian.set_option("clipboard_on_copy", false)
cian.set_keymap("x", "delete")          -- additive override; defaults stay intact
cian.on_open("md", function(path)        -- extension-dispatch execution
  cian.spawn({ "open", "-a", "Typora", path })
end)
```

The file is optional — cian runs with defaults if it is absent. Any syntax or
runtime error is shown in a startup notice and cian falls back to defaults for
whatever could not be applied, so a broken config never blocks startup.

### Windows paths need `[[...]]`

A backslash starts an escape sequence in Lua, so pasting a path into `"..."` is
a syntax error — and it takes the *whole* config file down with it, leaving you
on the default shell wondering why none of your settings applied:

```lua
-- BAD: \W is not a valid escape, and this kills the entire file
cian.set_option("shell", "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")

-- GOOD: backslashes are literal inside long brackets
cian.set_option("shell", [[C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe]])

-- Also fine: a bare name is looked up on PATH
cian.set_option("shell", "powershell.exe")
```

cian adds this hint to the startup notice whenever it sees an invalid escape.

See [`examples/init.lua`](examples/init.lua) for a fully-commented template and
the complete list of bindable actions.

## Shell panel

The bottom panel is a real PTY running your `$SHELL`, started on first focus.
Focus it with `Shift+J` (from a file pane), a mouse click, or `:shell`. While
the shell is focused, keys go straight to it; press **Esc** to return to the
files. Esc is passed through to full-screen programs (vim, less, htop, …) so
they keep working — it only leaves the shell at a normal prompt.

Shell tabs are driven by function keys (Ctrl-based shortcuts are unreliable
because some setups swallow the Ctrl modifier before it reaches the app):

| Key | Action |
|---|---|
| `F1`–`F8` | switch to shell tab 1–8 |
| `F9` | new shell tab |
| `F10` | close shell tab |
| `Shift+F1` / `Shift+F2` | focus next / previous split pane |
| `Shift+F8` | split the active pane left/right |
| `Shift+F9` | split the active pane top/bottom |
| `Shift+F10` | close the active split pane (asks first) |
| `F12` | zoom the focused surface to fill the window (toggle) |
| `Shift+F12` | zoom just the active split pane (toggle) |

Splits nest: splitting always divides the active pane, so you can build
arbitrary layouts (e.g. one pane on the left, two stacked on the right). These
keys are only active at a normal prompt; full-screen apps (vim, htop, …)
receive the function keys unchanged.

The file panes use the parallel controls: `Shift+F1` / `Shift+F2` switch to the
next / previous tab, and `Shift+F10` closes the active tab (asking first).

## Build

```sh
cargo build --release
./target/release/cian
```

## Troubleshooting

If cian misbehaves, set `CIAN_LOG` to capture diagnostics — shell spawns,
panics, and PTY errors are appended there, and the variable being unset (the
default) makes logging a no-op:

```sh
CIAN_LOG=/tmp/cian.log cian
```

A panic restores the terminal before it unwinds, so you should never be left
in raw mode needing `reset`.

## Install on Windows (offline)

cian compiles to a single self-contained `cian.exe` — no runtime, no DLLs, no
network access needed at runtime. To get a Windows x64 build without a Windows
dev machine, use the bundled GitHub Actions workflow, which builds on a real
Windows runner and packages a ready-to-carry zip:

1. Trigger a build — either push a tag (`git tag v0.1.0 && git push --tags`) or
   open the repo's **Actions** tab → **release** → **Run workflow**.
2. Download `cian-windows-x64.zip` from that run's artifacts (tagged builds are
   also attached to a GitHub Release).
3. Carry the zip into the offline machine and unzip it. Then either just run
   `cian.exe`, or run `install.ps1` to put `cian` on your PATH:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\install.ps1
   ```

   The default installs for the current user (no admin) under
   `%LOCALAPPDATA%\Programs\cian`. To install into Program Files for all users,
   run an **elevated** PowerShell and pass a destination:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\install.ps1 -Dest "C:\Program Files\cian" -AllUsers
   ```

   The installer unblocks the exe (so a terminal launch isn't "Access denied")
   and adds the folder to PATH. Open a new terminal and type `cian`. Use a Nerd
   Font terminal (Windows Terminal / WezTerm) for the file-type icons.
