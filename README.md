# cian

**English** · [日本語](README.ja.md)

**C**omfortable **I**nterface for **A**gile File e**X**plorer **N**avigation — a
two-pane terminal file manager, with a real shell built in. Inspired by
[AFXW (あふｗ)](https://akt.d.dooo.jp/akt_afxw.htm).

One binary. Runs in any terminal, on macOS, Windows and Linux. No runtime, no
DLLs, nothing to install alongside it.

---

## Try it

```sh
cargo build --release
./target/release/cian
```

On Windows, use it inside **Windows Terminal** or **WezTerm** with a Nerd Font —
that's where the icons and rounded corners look right. (Offline install is at the
bottom.)

Press **`?`** any time for the full key list. It's generated from your live
keymap, so keys you rebind show up too. `cian -man` prints it from a shell,
`cian -h` prints the command-line usage.

The UI is English by default. Want Japanese? `cian.set_option("lang", "ja")`, or
flip it from the right-click menu.

---

## The basics

Two panes side by side. You copy and move between them — that's the whole idea.

| Do this | And… |
|---|---|
| **← / →** | move focus between the two panes |
| **`l` / Enter** | enter a folder — or open a file with its default app |
| **`-` / Backspace** | go up a level (or click the `..` row at the top) |
| **`j` / `k`**, arrows | move the cursor |
| **`Space`** | mark the file under the cursor |
| **`c`** | copy the marked files to the other pane |
| **`m`** | move them to the other pane |
| **`d`** | delete (to the Recycle Bin / Trash) |
| **`r`** | rename |
| **`a` / `A`** | new file / new folder |
| **`u`** | undo the last rename / create / move |
| **`F3`** | look inside the file under the cursor |
| **`?`** | the full key list |

Copy, move and delete always **ask first**, and delete goes to the trash — so a
slip never costs you anything. `u` (or `:undo`) walks back the last few renames,
creates and moves too.

Big copies run in the background with a progress bar; **Esc** stops one.

**Mouse works everywhere.** Click to move the cursor, drag across rows to
select, double-click to open. Drag a file onto the other pane to copy it there
(Shift-drag to move). Every dialog has real clickable buttons, and the wheel
scrolls any popup. Drag a border to resize the split it divides.

---

## Look inside anything — `F3`

Press `F3` on a file and cian shows you what's in it, without leaving:

- **Text** — a scrollable viewer with line numbers and syntax highlighting
  (Rust, Python, JS/TS, Java, HTML, CSS, SQL, shell, Lua, YAML, JSON, …).
- **Markdown** — rendered right there. `p` toggles preview ↔ source.
- **Images** (`.png/.jpg/.gif/.bmp/.webp`) — drawn in the terminal. Coarse, but
  enough to see what it is.
- **Office & PDF** (`.docx/.xlsx/.pptx/.pdf`, plus legacy `.doc/.xls/.ppt`) —
  their text, no converter needed.
- **Archives** (`.zip/.jar/.tar/.tar.gz/…`) — the file list. `Enter` extracts
  the highlighted member to the other pane, `a` extracts all.

The viewer is vim-flavoured: `h j k l`, `w b`, `0 $`, `gg G`, `Ctrl-d/u` move;
`/` searches, `42G` jumps to a line, `%` to the matching bracket. `v` / `V` /
`Ctrl-v` select, `y` copies. `e` switches text encoding (UTF-8 / Shift_JIS /
UTF-16) if a file decoded wrong.

**Edit in place:** press `i` to edit the file right there (`Ctrl+S` saves in its
own encoding, `Esc` leaves). Prefer your own editor? **`E`** (or `:edit`) opens
it in `$VISUAL` / `$EDITOR` — or nvim → vim → vi — and reloads when you're back.

**Archives, more:** `:zip` / `:tar` / `:targz` bundle the marked files;
`:zip -e` makes an encrypted one. `:unzip` (or right-click **▸ Extract here**)
unpacks the file under the cursor into a fresh sub-folder. Locked zips still list
their members on F3, and extracting one asks for the password first.

---

## Find things

| Do this | And… |
|---|---|
| **`/`** | filter the listing as you type (Enter keeps it, Esc clears) |
| **`f`** | jump between matches in the current folder |
| **`Shift+F`** | find by name, anywhere below this folder |
| **`Ctrl+F`** | grep inside files — Enter opens the hit right on its line |
| **`b`** | branch view — flatten this whole subtree into one flat list |
| **`,`** | sort by name / size / date / extension (`n` `s` `d` `e`) |

Search runs in the background and streams results as it finds them — **Esc**
stops it, `Enter` jumps to a result. In the find/grep results, **`p`**
"panelizes" the matches into the pane, so you can mark and operate on them like
any other listing.

`:hidden` shows or hides dotfiles (shown by default).

---

## Compare & clean up

**Compare — `=`** (or `:diff`). Point the two panes at two files and `=` shows
them **side by side**, differences highlighted; `n`/`N` jump between changes.
Point them at two **folders** and `=` compares the trees byte-for-byte and lists
what differs. From either:

- **`>` / `<`** — copy the highlighted entry to the other side (a file or a whole
  subtree). WinMerge-style reconcile.
- **`w`** — save the comparison as a **side-by-side HTML or Markdown** report
  (the extension picks the format).
- **`x`** — ask the AI to explain what changed.

**Duplicates — `:dupes`** (or right-click **Find duplicate files**) finds
byte-identical files under the current pane and shows them as a checklist; one
per group is kept, the rest go through the normal delete confirmation.

**Bulk rename — `:brename`** renames the marked files by a pattern — no AI, no
network. Either a template (`report_{n3}.{ext}` → `report_001.log`, …) or a
substitution (`s/IMG/photo/i`). You review `old → new` and tick which to apply.

---

## Files, attributes, space

| Command | Does |
|---|---|
| `:attr` | permissions & owner of the selection |
| `:chmod 644` | change the mode (octal; Windows → use `:readonly`) |
| `:readonly on\|off` | toggle the read-only bit |
| `:hash md5` / `:hash sha256` | checksum the selected files |
| `:count` | count files, lines and source "steps" under the target |

The status line always shows **free space** on the active pane's drive
(`12.3G free / 100G`) — amber past 80% used, red past 95%.

**Version control just works.** In a **git** or **svn** working copy, each entry
gets a status badge (`●` staged, `✚` modified, `?` untracked, `‼` conflict), the
status line shows the branch (or `svn r123`), and F3 marks changed lines against
HEAD. Act on the selection with `:stage`, `:unstage`, `:discard`, `:gitlog`,
`:gitdiff`, and `B` in the viewer for a blame gutter — all under right-click
**Git ▸** / **SVN ▸**. cian shells out to your `git`/`svn`.

---

## Transfer files over SSH

Configure your hosts once (below), and the right-click **Transfer ▸** menu gives
you **Upload → server** and **Download ← server**, in a file pane or the shell.

- **Upload** — pick a host/user, type the remote folder, optionally set the mode
  (chmod), and the marked files go up.
- **Download** — browse the remote folder (Enter to open, `Space` to mark), then
  choose where they land: left pane, right pane, Desktop, or a typed path.

It's pure-Rust — no external `scp`. It uses **SFTP**, falling back to classic
**SCP** on servers without an SFTP subsystem, and the status line says which. Turn
on **verify** to re-read each transferred file and checksum both ends:

```lua
cian.set_option("verify_transfers", true)   -- off by default
```

**Connect — `Shift+S`** (or `:ssh`, or right-click) opens a two-stage picker:
host, then user. The command is typed into the shell, so your own shell config
and agent apply. Set your hosts in `init.lua`:

```lua
cian.ssh({
  users = { "root", "deploy", "app" },          -- offered for every host
  hosts = {
    { name = "web1", host = "10.0.1.11" },
    { name = "db1",  host = "10.0.2.31", users = { "postgres", "root" } },
    { name = "bast", host = "203.0.113.9", port = 2222 },
  },
})
```

**Passwords** are optional. cian types one when ssh asks for it (and uses it for
SFTP/SCP). Three ways:

```lua
users = {
  { name = "postgres", password = "..." },                -- in this file
  { name = "deploy",   password_cmd = "pass srv/deploy" }, -- from a credential store
  "root",                                                  -- key auth; nothing stored
}
```

A plaintext `password` is convenient but it's a secret in a file — cian warns on
Unix if that file is world-readable. `password_cmd` keeps it in your credential
manager; key auth avoids the question entirely. The password is never logged,
shown, or answered for a host-key prompt.

---

## The shell panel

The bottom panel is a real shell (your `$SHELL`). Focus it with **`Shift+J`**, a
click, or `:shell`; **Esc** returns to the files. Full-screen programs (vim,
less, htop) keep Esc and the function keys for themselves.

Drag inside a shell pane to select — it copies on release, no modifier needed.
**Right-click** for its menu: SSH connect, paste, session log, SFTP/SCP, and a
text-encoding picker.

**Tabs & splits** are on the function keys:

| Key | Action |
|---|---|
| `F1`–`F8` | switch to shell tab 1–8 |
| `F9` / `F10` | new tab / close tab |
| `Shift+F1` / `Shift+F2` | focus next / previous split pane |
| `Shift+F8` / `Shift+F9` | split the active pane — side by side / stacked |
| `Shift+F10` | close the active split (asks first) |
| `F12` / `Shift+F12` | zoom the whole surface / just the split (toggle) |

**Synchronize input** across a tab's panes with right-click **▸ Synchronize
input** (or `:sync`) — type once, it goes to every pane at once. The panes wear a
bright **⇄ SYNC** border while it's on, so you can't miss it.

**Snippets** — the lines you type over and over. Declare them once:

```lua
cian.snippets{
  { name = "sqlplus dev", cmd = "sqlplus user@DEVDB", enter = false },
  { name = "tail app log", cmd = "tail -f /var/log/app/app.log" },
  { name = "hulft send",  cmd = "utlsend -f SENDID -sync", confirm = true },
}
```

**Ctrl+Shift+Enter** (or `:snip`, or right-click) opens the picker; type to
filter, Enter sends the line to the shell. `enter = false` types it for you to
review, `confirm = true` asks first.

---

## Macros

A macro sets up your session in one keystroke. Press **`@`** (or `:macros`, or
right-click) to pick one. Two kinds:

**Layout macros** build the *screen*: split the panel, SSH each pane somewhere,
tint them apart, start logging.

```lua
return {
  { name = "Prod: db + app + logs", panes = {
    { cmd = "ssh admin@db",  bg = "40,24,24", log = "~/cian-logs" },
    { dir = "right", cmd = "ssh admin@app", bg = "24,40,24" },
    { dir = "down",  cmd = "ssh admin@app", steps = { "tail -f /var/log/app.log" } },
  }},
}
```

Per pane: `dir` (`right`/`down`), `cmd`, `steps` (a scripted login that can
`{ wait = 2 }` and `{ expect = "SQL>" }` for a prompt), `bg`, `log`. Add
`from = N` to build a grid, `zoom = true` to maximize first, `sync = true` to
synchronize input once it's up. Full examples in
[`examples/macro.en.lua`](examples/macro.en.lua) and
[`examples/macro/`](examples/macro/).

**Script macros** automate *file operations* — the AFXW side of the word. Give a
macro a `run` function and drive it with Lua's own `for` / `if`:

```lua
return {
  name = "Archive *.log, then bin them",
  run = function(cx)
    local logs = cx.glob("*.log")
    if #logs == 0 then cx.message("no logs here") return end
    cx.zip(logs, "logs.zip")
    cx.delete(logs)                     -- to the trash
    cx.message("archived " .. #logs .. " logs")
  end,
}
```

`cx` gives you: **query** (`dir`, `other`, `marked`, `cursor`, `list`, `glob`),
**operations** (`copy`, `move`, `delete`, `rename`, `mkdir`, `zip`, `read`,
`write`), **subprocess** (`sh("cmd")` → `{ code, out, err }`), **path helpers**
(`basename`, `stem`, `ext`, `join`, `exists`, `isdir`, `size`), and `message`.
A dozen ready samples — sort by extension, dated backup, normalise line endings,
clean empty files, checksum each file, generate an `index.md` — are in
[`examples/macro/Escript.en.lua`](examples/macro/Escript.en.lua).

**Snippet or macro?** One shell, a command or two → snippet. Several panes wired
up, or a file-op job → macro.

**At startup:** `cian --macro thing.lua` runs a macro as cian comes up (so a
`.lua` associated with `cian.exe` runs on double-click), or `--macro-name "..."`
runs one from your config.

---

## AI (optional)

With `cian.ai{...}` set, cian gets an assistant (it calls itself **Carmine /
カーマイン**). It's off unless configured, and always keeps you in the loop —
nothing runs or deletes without your say-so.

| Do this | You get |
|---|---|
| `:ai` | a chat, backed by Azure OpenAI |
| `:aicmd <what you want>` | a shell command drafted for you to review (never run for you) |
| `:aicommit` | a commit message drafted from the staged diff |
| `:aijunk` | a checklist of likely-disposable files → normal delete confirm |
| `:aiorganize` | a proposed folder layout → you approve the moves |
| `:airename` | AI-suggested new names → you review `old → new` |
| `:aisearch <…>` | files most relevant to a description, as a results list |
| `:aierror` | explain the last shell error |
| `:aidiff` | explain the diff on screen (also `x` in the diff view) |
| `:ailog` | triage the selected log — errors, timeline, likely cause |
| `S` in F3 | summarise the file you're viewing |

**Give it context.** `cian.ai_context("…")` records facts about *your* setup
(the OS, the deployment target, house rules) and cian prepends them to every
prompt. Per-server facts go on the host: a `notes = "RHEL 8; Oracle 19c; …"` is
handed over automatically when the shell is logged into that host.

cian reaches the model through a small bundled Python helper (Windows broker
sign-in, like the crmaine extension) — nothing to install beyond Python and a
couple of packages. `auth_mode = "mock"` gives an offline echo for wiring it up,
and `api_base_url` points it at a local server (Ollama, LM Studio). This is the
one place cian isn't fully self-contained, which is why it's opt-in. See
[`examples/init.en.lua`](examples/init.en.lua).

---

## Configuration

cian reads `~/.config/cian/init.lua` (override with `$CIAN_CONFIG_DIR`). It's
Lua, on a small `cian` table — no init.lua needed to start:

```lua
cian.set_theme({ accent = "#00d7d7", mark_fg = "yellow" })
cian.set_option("clipboard_on_copy", false)
cian.set_keymap("x", "delete")           -- binding a key replaces its default; "none" disables
cian.on_open("md", function(path)        -- open .md files your way
  cian.spawn({ "open", "-a", "Typora", path })
end)
```

A broken config never blocks startup — cian shows the error and falls back to
defaults for whatever didn't apply. `:reload` re-reads it live (keymaps,
options, SSH hosts, open handlers; theme and borders need a restart).

**Themes.** 13 presets, live-previewed: `:theme` opens a gallery, `:theme <name>`
sets one, and you can theme each pane separately. The choice sticks across
restarts.

**Portable.** Put `init.lua` (and `shortcuts.lua` / `macro.lua`) next to the
`cian` executable and that folder wins over `~/.config/cian`, for reading *and*
writing. Drop the binary and its `.lua` on a USB stick and the whole setup
travels with it, leaving nothing on the host.

**Session.** Launched with no path, cian reopens the two folders you had last
time. Pass a folder on the command line to override it.

**Remapping keys.** Every file-pane action has a name you can bind:

```lua
cian.set_keymap("x", "delete")   -- x now deletes too
cian.set_keymap("d", "rename")   -- d renames instead
cian.set_keymap("d", "none")     -- d does nothing
```

[`examples/init.en.lua`](examples/init.en.lua) is a fully-commented template with
every default binding and the complete action list. **Windows paths need long
brackets** — a backslash is an escape in Lua:

```lua
cian.set_option("shell", [[C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe]])
cian.set_option("shell", "powershell.exe")   -- or a bare name, looked up on PATH
```

---

## How it fits together

A cargo workspace, seven crates:

| Crate | Role |
|---|---|
| `cian-core` | Pure logic: file ops, marks, sorting, search, diff, dedup, git |
| `cian-tui`  | Rendering & input (ratatui + crossterm), layout, popups, mouse |
| `cian-pty`  | The embedded shell (portable-pty + vt100) |
| `cian-scp`  | Built-in SFTP/SCP transfer (pure-Rust russh) |
| `cian-ai`   | Optional AI helper (Azure OpenAI via a bundled Python script) |
| `cian-lua`  | Lua config host (mlua): keymaps, themes, macros |
| `cian-bin`  | The entry point — produces the `cian` binary |

One main loop owns all the UI and drawing. Anything that could block — search,
diff, transfer, AI — runs on a worker thread and its result is polled back each
frame, so the UI never freezes.

```mermaid
flowchart TD
    user([User])
    term([Terminal])

    user -- "keys / mouse<br/>(crossterm)" --> disp

    subgraph mainloop["cian-tui — main loop (single thread)"]
        direction TB
        disp["dispatch<br/>keys · mouse · commands"]
        state["App state<br/>2× Pane · popups · shell · focus"]
        draw["render → ratatui"]
        poll["poll worker channels"]
        disp --> state --> draw
        poll --> state
    end

    draw --> term --> user

    cfg["cian-lua<br/>init.lua · ssh.lua · keymap.lua → Config"]
    cfg -- "startup / :reload" --> state

    core["cian-core (pure domain)<br/>Pane · Entry · sort/filter/marks · file ops · git"]
    state <--> core

    disp -- "keystrokes" --> pty["cian-pty<br/>portable-pty child + vt100"]
    pty -- "screen" --> draw

    subgraph work["worker threads — mpsc channels, polled each frame"]
        direction TB
        heavy["search · diff · dir-compare · dedup"]
        scp["cian-scp<br/>russh SFTP / SCP"]
        ai["cian-ai<br/>Python broker → Azure OpenAI"]
    end

    disp -- "heavy / remote / AI" --> work
    work -- "results" --> poll
```

---

## Install on Windows (offline)

cian is a single self-contained `cian.exe` — no runtime, no DLLs, no network at
runtime. To get a Windows x64 build without a Windows dev machine, use the
bundled GitHub Actions workflow (it builds on a real Windows runner and packages
a ready-to-carry zip):

1. Push a tag (`git tag v0.1.0 && git push --tags`), or open **Actions →
   release → Run workflow**.
2. Download `cian-windows-x64.zip` from that run.
3. Carry it to the offline machine, unzip, and either run `cian.exe` or install
   it on your PATH:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\install.ps1
   ```

   That installs for the current user under `%LOCALAPPDATA%\Programs\cian` (no
   admin). For all users, run an elevated PowerShell:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\install.ps1 -Dest "C:\Program Files\cian" -AllUsers
   ```

Open a new terminal and type `cian`. Use a Nerd Font terminal (Windows Terminal
/ WezTerm) for the file-type icons.

---

## Good to know

- **Which build?** `cian --version` prints the commit baked in at build time.
  An old `cian.exe` left on PATH looks exactly like a missing feature.
- **Border corners** default to square in the legacy Windows console (rounded
  ones are missing from some console fonts) and rounded elsewhere. Force it:
  `cian.set_option("borders", "rounded")` (or `"plain"`).
- **Trouble?** Set `CIAN_LOG=/tmp/cian.log` to capture diagnostics. A panic
  restores the terminal on the way out, so you're never left needing `reset`.
