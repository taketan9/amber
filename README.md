# cian

**English** · [日本語](README.ja.md)

**C**omfortable **I**nterface for **A**gile File e**X**plorer **N**avigation —
a modern two-pane terminal file manager inspired by [AFXW (あふｗ)](https://akt.d.dooo.jp/akt_afxw.htm).

Runs in any terminal (designed to be used as WezTerm's `default_prog`).
Cross-platform: macOS / Windows / Linux.

## Status

Early development, but broadly usable. Working: two-pane navigation,
marks/visual selection, file operations (copy/move/delete/rename/create) with a
progress bar, incremental filtering, history, shortcuts, in-listing and
recursive search/grep, sorting, file and directory diff, a text/hex/zip viewer,
checksums and attributes, clipboard integration, an embedded PTY shell panel
with tabs and splits, built-in SFTP/SCP transfer, Lua configuration, a fully
remappable keymap, and a mouse-operable UI (including clickable dialogs).

## Help

Press **`?`** (or `Ctrl+.`, `:man`, or **right-click → Key manual**) inside
cian for the full key manual — it is generated from the live keymap, so it also
lists any keys you bound in `init.lua`. From a shell, `cian -man` prints the
same thing and `cian -h` prints the command-line usage.

The interface is **English by default**; switch it to Japanese with
`cian.set_option("lang", "ja")`, or toggle live from the right-click menu.

## Mouse

Almost everything is reachable with the mouse.

**Left** and **Right** move focus between the two file panes — the thing a
two-pane layout makes you reach for. `l` / `Enter` enter a directory and `-` /
`Backspace` go up, as before. Each pane also lists a **`..`** row at the top:
`Enter` it, or single-click it, to step up a level — handy for mouse-first
navigation. A **double-click** activates any other entry: a directory is
entered, a file is opened with its OS default program (or an `init.lua`
`on_open` handler).

A single left-click just moves the cursor to a row — it never marks. To mark a
range with the mouse, **drag** across the rows (a rubber-band selection);
marking individual files is `Space`. The `..` row is navigation only: it can be
neither marked nor used as the target of an operation.

Drag any border to re-proportion the split it divides — the two file panes, the
file/shell divider, and every split inside the shell panel. Neither side can be
dragged below 15% of its parent.

Drag an entry onto the other pane to copy it there, or Shift-drag to move —
the usual confirmation follows, so a slip of the mouse is not destructive.
Dropping onto the shell panel types the paths at the prompt instead, which is
as close as a console application gets to dragging a file into a terminal.

cian cannot take part in the *system's* drag and drop: a console application
has no window to be a drag source or target, so dragging to or from Explorer
is not possible and will not be. Dropping a file onto the terminal window makes
the terminal paste its path, which is the terminal's doing, not cian's.

**Dialogs and pickers are clickable too.** A confirmation dialog shows real
`[ Yes ]` / `[ No ]` (and `[ Overwrite ]` / `[ Rename ]`) buttons; every list —
the sort and encoding pickers, SSH host/user, find results, copy-to, history,
shortcuts, the directory comparison, an archive's members — selects a row on
click, and the mouse wheel scrolls whatever popup is open. The keyboard
shortcuts still work; the clicks just stand in for them.

`:copyto` and `:moveto` (and **Copy to…** in the menu) send the selection
somewhere other than the opposite pane, offering the directories used recently
— the ones that are not the other pane tend to be the same few, and retyping
them is the tedious part. `n` in that picker types a fresh path.

Right-click a pane for a context menu: copy, cut, paste, copy/move to the other
pane, rename, delete, a per-pane background color, and the key manual (the
last two are offered in the shell's menu too).

Background colors apply to whichever pane you right-clicked — including a
single split inside the shell, not the whole panel. In the shell the tint only
fills cells the shell left uncolored, so `ls` colors and editor themes come
through untouched. Copy and cut fill a file clipboard that persists while you
navigate, so you can copy here, move somewhere else, and paste there.

**Paste** takes cian's own clipboard when it holds something, and otherwise
falls back to the *system* clipboard — so a file copied in Explorer or Finder
pastes into the focused pane. The status line says which of the two it used.
`:paste` does the same from the command line. Clipboard entries that are not
real paths are ignored, since the platform queries return plain clipboard text
coerced into one.

`p` and `Shift+P` go the other way, putting paths or file references onto the
system clipboard. Background colors are session-only.

## Animation

Splitting, maximizing (`F12`) and closing a pane animate over 150ms. PTYs are
resized once, when the transition lands, so the shell never reflows mid-flight.
Any keypress lands the transition immediately — input is never held up by it.
Tune or disable it:

```lua
cian.set_option("animation_ms", 250)   -- slower
cian.set_option("animation_ms", 0)     -- off
```

## Keeping up with the filesystem

A file created by something else — a build, a download, a sync — used to never
appear: cian only reloaded after its own actions. Each pane's directory is now
checked about once a second and re-read when it has changed, so entries
appearing and disappearing show up on their own. Measured: a file created
externally shows within a second.

The check is one `stat` of the directory, not a re-read of it, so it costs
nothing on a large listing. That does mean a change to a file's *contents*
without any entry being added or removed is not noticed; `Ctrl+R` (or `F5`)
forces a full refresh.

## File operations

Copies, moves and deletes run on a worker thread with a progress bar: how far
along, how many files, how much data, and how long it has been going. **Esc**
stops it. Previously these ran inline — copying a 700 MB file locked the whole
UI for fourteen seconds with nothing on screen to explain why.

Files are copied in chunks, so the bar advances *within* a large file and a
cancel is acted on in a fraction of a second rather than at the next file
boundary. A cancelled copy removes its half-written destination instead of
leaving something that looks complete. Moves try a rename first, which is
instant within a volume, and only fall back to copy-then-delete across one.

### When the destination needs administrator rights (Windows)

Writing into a protected directory (`C:\Program Files`, `C:\Windows`) fails for
an ordinary process with "Access is denied", and nothing cian does gets past the
ACL. When a copy or move hits that specific error, cian says so plainly and, on
Windows, offers to **retry as administrator**: a UAC prompt appears, then the
transfer is redone elevated (robocopy for trees, Copy-Item for single files).
That elevated copy runs in its own process, so it has no in-app progress bar —
cian waits for it and reports the outcome ("as administrator"). The simpler
alternative is to launch cian itself elevated, after which every destination is
writable. On other platforms the notice just names the cause and suggests a
writable folder.

## Deleting

`d` moves items to the OS trash (Finder's Trash / the Windows Recycle Bin), so
a mistake is recoverable. The confirmation popup offers `a` to delete
permanently instead.

## Comparing files and directories

`=` (or `:diff`) compares the left pane's file against the right pane's, side by
side, with differing lines highlighted; `n` / `N` jump between changes and `f`
folds the identical runs away.

Point the two panes at two **directories** and `=` compares them recursively
instead, listing every path that differs — added on one side, missing on the
other, or present in both but not identical. Files are compared byte-for-byte
(not just size and timestamp), on a worker thread with a progress bar and Esc,
so a "same" verdict really means the same. Enter on a result moves both panes to
that path.

`:dupes` (or right-click **Find duplicate files**) finds byte-identical files
anywhere under the current pane, on a worker thread. It groups by size first and
only hashes the size-collisions, so most files are never read. The results are a
grouped checklist — one file per group is left as the keeper, the rest
pre-checked — and approving hands the checked copies to the ordinary delete
confirmation, so nothing is removed without the usual trash/permanent choice.

## File transfer (SFTP / SCP)

With SSH hosts configured (see below), the right-click menu gains **Upload →
server** and **Download ← server**. Pick a host and user, type the remote
directory (upload) or file (download), and the transfer runs on the usual worker
thread with the progress bar and Esc.

It is pure-Rust — no external `scp` binary — and picks the wire protocol
automatically over one authenticated connection: **SFTP** first (what modern
`scp` uses), falling back to the classic **SCP** protocol when the server has no
SFTP subsystem, which is the case on some appliances and locked-down sshd
configs. The status line reports which one carried it ("via SFTP" / "via SCP").
Single files and whole directories are supported; the host key is currently
accepted without a known-hosts check (a documented gap).

## SSH

`Shift+S`, `:ssh`, or **right-click → SSH connect…** opens a two-stage picker:
choose a host, then a user on it.

Right-click is the one that works while the shell pane has focus: keys go
straight to the shell there, so `Shift+S` would just type an `S`. SSH leads the
shell pane's context menu for that reason. Typing in the host stage filters.
Hosts with a single user connect straight away. The command is typed into the
shell panel, so your own shell config and agent apply, and the tab drops back to
a local prompt when the session ends.

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
remember — the picker does the remembering. The same host list feeds the SFTP/SCP
transfer flow above.

### Passwords

A login can carry a password, which cian types when ssh asks for one (and which
SFTP/SCP uses to authenticate):

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

## Looking inside things

**`F3`** answers "what is in here" without leaving cian. On a text file it
opens a scrollable viewer with line numbers; on a binary one, a hex dump,
since showing a compiled file as text is a screenful of mojibake that answers
nothing. Only the first 4 MB is read, so opening a huge log is instant.

A **Markdown** file (`.md`) opens straight into a rendered preview — headings,
emphasis, lists, blockquotes, rules, links and code blocks styled for the
terminal, with `mermaid` blocks shown as a clearly-labelled source box (a
terminal cannot draw the diagram itself). Press **`p`** to toggle between the
preview and the raw source.

**Office and PDF documents** preview as text, with nothing else installed.
`.docx`, `.xlsx` and `.pptx` are ZIP-of-XML and are read directly; a PDF's text
is pulled from its content streams; the legacy binary `.doc`/`.xls`/`.ppt` fall
back to a best-effort readable-text scan (clearly labelled as approximate — a
scanned-image PDF or a document with non-embedded font encodings may have no
text to extract). It reproduces no layout — it answers "what does this say" —
but because it lands in the same viewer, search, selection and copy all work
over it. This keeps cian's offline, single-binary promise: no converter, no
network, just the one executable.

The viewer is vim-flavoured: a cursor moves with `h`/`j`/`k`/`l`, `w`/`b`,
`0`/`$`, `gg`/`G` and `Ctrl-d`/`Ctrl-u`; `/` searches (all matches highlighted,
`n`/`N` step through them), `42G` jumps to a line, `%` to the matching bracket,
`{`/`}` between paragraphs. `v` / `V` / `Ctrl-v` start character-, line- and
block-wise visual selection; **Shift+arrow** selects character-wise and
**Alt+arrow** block-wise like an editor; a left-click **drag** selects (hold
**Alt** for a rectangle). `y` (or `c`), or right-click, copies it. `e` switches the text
encoding (UTF-8 / Shift_JIS / UTF-16) when a file was decoded wrong, and
`Shift+Enter` reveals the file in the pane (jump to its folder, cursor on it).

When the viewer was opened from a grep hit (below), `Ctrl+n` / `Ctrl+N` step to
the next / previous hit's preview without going back to the list.

On an archive — **zip** (also `.jar`, `.whl`, `.epub`) or a **tarball**
(`.tar`, `.tar.gz`, `.tgz`) — it lists the members instead, with their unpacked
sizes. `Enter` extracts the highlighted one into the opposite pane, `a` extracts
all — on the worker thread, with the usual progress bar and Esc.

Member paths are checked before anything is written: an archive can name
`../../etc/passwd` or an absolute path, and a naive extractor obliges. Those
are refused individually and the rest of the archive still comes out.

Reading covers zip and gzipped/plain tar; `:zip` still writes zip only (it is
what Windows reads and writes without extra software).

## Searching

`f` jumps between matches in the current listing. **`Shift+F`** searches the
whole tree below the pane's directory, on a worker thread — results appear as
they are found rather than after the walk finishes, `Esc` stops it, and Enter
on a result moves the pane into that directory with the cursor on the entry.

**`Ctrl+F`** greps *inside* the files instead, listing each matching line with
its number. `Enter` on a grep hit opens the F3 viewer right on that line — the
whole point of grepping. Binary files are skipped (a match inside a compiled
object is unreadable and answers nothing) and so are files over 8 MB, so a
stray database dump cannot stall the search.

The walk is breadth-first, so shallow matches — usually the wanted ones —
arrive first and a search abandoned early has still produced something useful.
Hidden directories are skipped, symlinked directories are not followed (a link
back up the tree would loop), and the search stops at 5000 hits.

## Attributes, checksums, dotfiles

`:attr` shows permissions and owner for the selection; `:chmod 644` and
`:readonly on|off` change them. `:hash` checksums the selected files —
`:hash md5` or `:hash sha256` — on a worker thread with the same progress bar
and Esc as any other long operation, since the files worth checksumming are the
big ones.

`:hidden` shows or hides dotfiles for the focused pane. All three are also in
the right-click menu. Dotfiles are shown by default, which is what cian has
always done; `cian.set_option("show_hidden", false)` changes that.

Note that `:chmod` is octal only. Symbolic forms like `u+x` are a small
language of their own, and half-implementing them would be worse than saying
no. On Windows there are no mode bits at all, so `:chmod` refuses and points at
`:readonly`.

## Git

When a pane sits inside a git repository, each entry carries a status badge:
`●` staged, `✚` modified, `?` untracked, `‼` conflict, and `~` on a folder that
contains changes below it. The status line shows a **branch bar** — the current
branch, ahead/behind counts, and how many files have changed (green when clean,
amber when not). Open a tracked file with F3 and a **change gutter** marks each
line against HEAD: green for added, amber for modified, a red underline where
lines were deleted.

You can act on the selection without leaving cian:

- `:stage` (`:add`) — `git add` the marked files, or the one under the cursor.
- `:unstage` (`:reset`) — `git reset HEAD`, keeping the worktree changes.
- `:discard` (`:revert`) — `git checkout --` to throw away worktree changes to
  tracked files; it confirms first, since that cannot be undone.

The same three are under right-click **Git ▸** (shown only in a repo), and the
AI can draft a commit message from the staged diff (see below). cian shells out
to the `git` on your PATH — there is no library dependency to keep the binary
self-contained.

## Going to a path

`z` (or `:cd`) prompts for a path, seeded with the current directory. A
directory is entered; a file is opened with whatever `Enter` would use,
including any `on_open` handler from `init.lua`. `~`, `$VAR`, `${VAR}` and
`%VAR%` are expanded, and a surrounding pair of quotes is stripped — so a path
copied out of a shell or an Explorer address bar can be pasted in as-is.

## Context menu from the keyboard

`Shift+Enter` opens the same menu the right mouse button does, next to the
highlighted entry. It needs a terminal that tells Shift+Enter apart from plain
Enter: the Windows console does, and on Unix it wants the kitty keyboard
protocol (WezTerm, kitty, foot). `:menu` works everywhere.

## Selecting

`v` starts a visual selection; `Enter` marks it. Inside visual mode `a` selects
the whole listing, and `gg` / `G` extend to the top or bottom — so both `v a`
and `gg v G` select everything.

Text fields (rename, new file, shortcut name and target) take **Ctrl+V** to
paste and **Ctrl+U** to clear. A new shortcut's target starts filled in with
the entry under the cursor, since that is usually what is being bookmarked.

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

## Remapping keys

Every file-pane action has a name and can be bound to a key in `init.lua`.
Because a user binding is consulted before the built-in keys, binding a key
**replaces** its default rather than only adding an alias, and `"none"` turns a
key off entirely:

```lua
cian.set_keymap("x", "delete")   -- add: x now deletes too
cian.set_keymap("d", "rename")   -- change: d renames instead of deleting
cian.set_keymap("d", "none")     -- disable: d does nothing
```

[`examples/init.lua`](examples/init.lua) lists every default binding as the
`set_keymap` line that would recreate it, so you can uncomment-and-edit to move
or disable any of them, along with the full list of action names. Structural
keys (arrows, Enter, Backspace, Tab, the F-keys, and Ctrl-/Shift- combinations)
are built in and not remapped here.

## AI (optional)

With `cian.ai{...}` configured, `:ai` opens a chat backed by Azure OpenAI. cian
reaches it through a small bundled Python helper that uses the same Windows
broker (WAM) sign-in as the crmaine extension — there is nothing to install
beyond Python and a couple of `azure`/`openai` packages, and the helper is
embedded in the binary and written out on first use. If Python, the packages, or
sign-in are unavailable the whole feature stays silent and cian runs exactly as
before; `auth_mode = "mock"` gives an offline echo for wiring it up, and an
`api_base_url` points it at a local OpenAI-compatible server (Ollama, LM Studio).

Beyond chat, the AI can act on what you have open, always with a human in the
loop:

- **Command from a description** (shell pane): `:aicmd <what you want>`, or the
  right-click **AI ▸** menu. The generated command is shown for review and only
  inserted at the prompt — never run for you.
- **Commit message** (file pane): `:aicommit`, or right-click **AI ▸ → Draft
  commit message**. cian reads the staged diff (`git diff --cached`) and drafts
  a Conventional-Commits-style message; you get an editable preview (`e` to
  edit, `Enter`/`c` to commit, `Esc` to cancel) before anything is committed.
  With nothing staged it says so rather than committing an empty change.
- **Detect junk** (file pane): `:aijunk`, or right-click **AI ▸ → Detect junk
  files**. cian sends only the listing's metadata (names, sizes, dir flags —
  never contents) and the model flags likely-disposable entries (build output,
  caches, temp and editor-backup files, OS cruft). You get a checklist —
  Space/click toggles, `a` toggles all — and approving hands the checked paths
  to the *normal* delete confirmation, so nothing is removed without the usual
  trash/permanent choice. A name the model invents matches nothing, so it can
  only ever target files that were actually shown.
- **Suggest structure** (file pane): `:aiorganize`, or right-click **AI ▸ →
  Suggest folder structure**. From the same metadata the model proposes a set
  of moves that group loose files into sub-folders (`images/`, `docs/`, …). You
  review the plan as `name → folder/` rows — Space/click toggles, `a` toggles
  all — and approving (Enter/`m`) creates the folders and moves the checked
  files. It can only ever move files *into* new sub-folders of the current
  directory: destinations are validated to reject `..`, absolute paths and
  drives, and a name the model invents matches nothing.
- **Explain an error** (shell pane): `:aierror`, or right-click **AI ▸ →
  Explain the last error**. cian sends the shell pane's visible text and the
  model explains what went wrong and the likely fix, in the AI chat. Like the
  summary below, this sends terminal *text*, so it is an explicit action.
- **Summarise a file** (F3 viewer): press **`S`** while viewing a file. Unlike
  the metadata-only features above, this sends the file's **text** to the model
  (bounded to keep the request small), so it is a deliberate keystroke rather
  than automatic. The summary opens in the AI chat, where it can be scrolled,
  selected and copied.
- **Semantic search** (file pane): `:aisearch <what you're looking for>`, or
  right-click **AI ▸ → Semantic search**. cian walks the tree, collects up to a
  few hundred file **paths** (names only — no contents), and asks the model
  which are most relevant to your description. The matches open in the same
  results list as `find`/`grep`: Enter previews the file in F3, `Ctrl+n`/`Ctrl+N`
  step between them, Esc returns to the list. A path the model invents matches
  nothing.
- **Bulk rename** (file pane): `:airename`, or right-click **AI ▸ → Bulk
  rename**. It asks how to rename ("snake_case", "add a date prefix", …), then
  proposes new names for the marked files — or the whole listing when nothing
  is marked. You review the plan as `old → new` rows (Space/click toggles, `a`
  toggles all) and approving (Enter/`r`) renames the checked files in place. A
  proposed name is validated to a bare filename (no path, no `..`), a target
  that already exists is skipped, and a name the model invents matches nothing.

**Give the AI your context.** Generic answers assume a generic machine.
`cian.ai_context("…")` (a string or a list) records facts about *your*
environment — the OS the panes browse, the deployment target, house
conventions — and cian prepends them to every AI prompt above. Per-server
facts belong on the SSH host instead: a host's `notes = "RHEL 8; Oracle 19c; …"`
is handed to the model automatically whenever the active shell is logged into
that host, so "explain the last error" already knows what it is looking at.

The AI part is the one place cian is not a single self-contained binary — it
opts into an external interpreter and network — which is why it is strictly
optional and off unless configured. See
[`examples/init.lua`](examples/init.lua) for the settings.

## Architecture

Cargo workspace, split into seven crates:

| Crate | Role |
|---|---|
| `cian-core` | Pure domain logic: file ops, marks, history, sorting, filtering, search, diff, dedup, elevation, git |
| `cian-tui`  | Rendering & input (ratatui + crossterm), layout, popups, mouse |
| `cian-pty`  | Embedded shell pane (portable-pty + vt100 + tui-term) |
| `cian-scp`  | Built-in SFTP/SCP file transfer (pure-Rust russh, no C deps) |
| `cian-ai`   | Optional AI helper (Azure OpenAI via a bundled Python broker-auth script) |
| `cian-lua`  | Lua configuration host (mlua): keymaps, themes, ext-open DSL |
| `cian-bin`  | Entry point — produces the `cian` binary |

## Configuration

cian reads `~/.config/cian/init.lua` (override the directory with
`$CIAN_CONFIG_DIR`). Configuration is written in Lua via a small WezTerm-style
API on the global `cian` table:

```lua
cian.set_theme({ accent = "#00d7d7", mark_fg = "yellow" })
cian.set_option("clipboard_on_copy", false)
cian.set_keymap("x", "delete")           -- binding a key replaces its default; "none" disables
cian.on_open("md", function(path)        -- extension-dispatch execution
  cian.spawn({ "open", "-a", "Typora", path })
end)
```

The file is optional — cian runs with defaults if it is absent. Any syntax or
runtime error is shown in a startup notice and cian falls back to defaults for
whatever could not be applied, so a broken config never blocks startup.

**Portable mode.** If `init.lua`, `shortcuts.lua` or `macro.lua` sits in the
same directory as the cian executable, that directory wins over
`~/.config/cian` — for reading *and* for the files cian writes back
(bookmarks, macros). Drop the binary and its `*.lua` on a USB stick and the
whole setup travels together, leaving no trace on the host. With nothing beside
the executable, cian behaves exactly as before, from `~/.config/cian`.

`:reload` re-reads `init.lua` without restarting — keymaps, options, SSH hosts
and open handlers apply immediately. The color theme and border style are
installed once at startup, so a change to those still needs a restart; `:reload`
says so when it sees one.

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

**Selecting text:** because cian owns the mouse, the terminal's own selection
does not reach the shell. Instead, plain-drag inside a shell pane to select, and
the selection is copied to the clipboard on release — no modifier needed.

**Right-click** in a shell pane for its menu: SSH connect, paste, start/stop a
**session log** (a scrubbed transcript written to a file), SFTP/SCP
upload/download, and a **text-encoding** picker (UTF-8 / Shift_JIS / UTF-16LE /
UTF-16BE) for shells that speak a non-UTF-8 codepage.

Shell tabs are driven by function keys (Ctrl-based shortcuts are unreliable
because some setups swallow the Ctrl modifier before it reaches the app):

| Key | Action |
|---|---|
| `F1`–`F8` | switch to shell tab 1–8 |
| `F9` | new shell tab |
| `F10` | close shell tab |
| `Shift+F1` / `Shift+F2` | focus next / previous split pane |
| `Shift+F8` | **v-split** — divide the active pane into two side by side |
| `Shift+F9` | **h-split** — divide the active pane into two stacked |
| `Shift+F10` | close the active split pane (asks first) |
| `F12` | zoom the focused surface to fill the window (toggle) |
| `Shift+F12` | zoom just the active split pane (toggle) |

Splits nest: splitting always divides the active pane, so you can build
arbitrary layouts (e.g. one pane on the left, two stacked on the right). These
keys are only active at a normal prompt; full-screen apps (vim, htop, …)
receive the function keys unchanged.

The file panes use the parallel controls: `Shift+F1` / `Shift+F2` switch to the
next / previous tab, and `Shift+F10` closes the active tab (asking first).

## Macros

A **layout macro** builds a whole shell working-set in one keystroke: split the
panel, connect each pane somewhere, tint them apart, start logging — done. Press
**`@`** (vim's play-a-macro key) to pick one, or run `:macros`.

Macros live in `macro.lua` (portable-aware, like the rest of the config). Each
returns a name and a list of panes; the first pane is the shell you are on, and
each later pane is split off the previous one:

```lua
return {
  { name = "Prod: db + app + logs", panes = {
    { cmd = "ssh admin@db",  bg = "40,24,24", log = "~/cian-logs" },
    { dir = "right", cmd = "ssh admin@app", bg = "24,40,24" },
    { dir = "down",  cmd = "ssh admin@app", steps = { "tail -f /var/log/app.log" } },
  }},
}
```

Per pane: `dir` (`"right"` | `"down"`), `cmd` (a line to run), `steps` (more
lines sent after it — e.g. `sqlplus /nolog` then `connect …`), `bg` (a colour so
panes are easy to tell apart), and `log` (a directory to record the session to).
Because each split spawns asynchronously, cian builds the layout pane-by-pane as
the shells come up. See [`examples/macro.lua`](examples/macro.lua).

**One macro per file.** As an alternative to the single list, put a `macro/`
directory next to `init.lua` with one file each — `macro/Adeploy.lua`,
`macro/Bdbcheck.lua` — where each returns a single `{ name =, panes = }` table.
They load in filename order alongside `macro.lua`. See
[`examples/macro/Adeploy.lua`](examples/macro/Adeploy.lua).

**Run one at startup** (TeraTerm-`.ttl` style). `cian --macro path/to/thing.lua`
builds that macro's layout the moment cian comes up; a bare `cian thing.lua`
does the same, so associating `.lua` with `cian.exe` makes a macro file run on
double-click. `cian --macro-name "Two local shells"` runs a named macro from
your normal config instead. Either way cian stays open afterwards — the macro
just seeds the session.

## Build

```sh
cargo build --release
./target/release/cian
```

## Which build am I running?

```sh
cian --version     # cian 0.1.0 (7f92bae)
```

The commit is baked in at build time. Worth checking first when a feature
seems missing: an older `cian.exe` left on PATH looks exactly like a bug.

## Border corners

Rounded corners (`╭╮╯╰`, U+256D–U+2570) are missing from several stock console
fonts — Consolas and Lucida Console among them — while the straight `─│` are in
almost all of them. Windows then font-links only the corners to another face,
whose metrics differ, so the frame looks a few pixels out at each corner while
its sides stay put.

cian therefore uses square corners in the legacy Windows console and rounded
ones elsewhere. Force it either way:

```lua
cian.set_option("borders", "rounded")   -- or "plain"
```

## Running it standalone

cian cannot restyle the console it is launched into; the font and colors belong
to the host terminal. Double-clicking `cian.exe`, or running it from `cmd`,
lands in the legacy Windows console, where the Nerd Font icons come out as
boxes. It says so once at startup when it detects that.

For the intended look, launch it from Windows Terminal or WezTerm:

```powershell
wt cian
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
