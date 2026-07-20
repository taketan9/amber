-- cian configuration — example init.lua
--
-- Place this file at ~/.config/cian/init.lua (or set $CIAN_CONFIG_DIR to a
-- directory containing init.lua). Everything here is optional; cian runs with
-- sensible defaults if the file is missing. Any error in this file is shown in
-- a startup notice and cian continues with defaults for whatever failed.
--
-- The API is intentionally WezTerm-flavoured: you call functions on the global
-- `cian` table.

----------------------------------------------------------------------
-- Theme — colours accept "#rrggbb", "r,g,b", or a named colour
-- (cyan, red, yellow, blue, green, magenta, white, black, gray, darkgray,
--  lightblue, ...). Omit any field to keep its default.
----------------------------------------------------------------------
cian.set_theme({
  accent      = "#00d7d7", -- focused-pane border, icons, active tab
  status_bg   = "#28283c", -- status bar background
  selected_bg = "#3c3c5a", -- cursor row highlight
  visual_bg   = "#503c1e", -- visual-mode selection
  mark_fg     = "yellow",  -- mark indicator
})

----------------------------------------------------------------------
-- Options
----------------------------------------------------------------------
cian.set_option("clipboard_on_copy", true) -- also push paths to the OS clipboard on copy/move
cian.set_option("mask", "*.*")              -- default file mask shown in the status bar
----------------------------------------------------------------------
-- Shell panel program
--
-- Defaults to $SHELL on Unix and %COMSPEC% (cmd.exe) on Windows.
--
-- IMPORTANT, Windows users: a backslash starts an escape sequence in Lua,
-- so a path pasted into "..." is a *syntax error* that takes this whole
-- file down with it — leaving you on the default shell (cmd.exe) wondering
-- why nothing applied. Write full paths in [[...]], where backslashes are
-- literal:
--
--     BAD   "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
--     GOOD  [[C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe]]
--
-- The bare exe name also works and is looked up on PATH: "powershell.exe".
----------------------------------------------------------------------
local is_windows = package.config:sub(1, 1) == "\\"
if is_windows then
  -- Windows PowerShell 5.1, which ships with every Windows install.
  cian.set_option("shell", [[C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe]])

  -- PowerShell 7 instead, if you have it (the path varies by install):
  -- cian.set_option("shell", [[C:\Program Files\PowerShell\7\pwsh.exe]])
end

-- Length of the split / maximize / close transitions, in milliseconds.
-- Set to 0 to turn animation off entirely.
cian.set_option("animation_ms", 150)

----------------------------------------------------------------------
-- Keymaps — bind a single key to an action name. These are *additive
-- overrides*: a key you bind here takes priority, every other key keeps its
-- default. Useful for adding aliases without losing the defaults.
--
-- Available actions:
--   cursor_down, cursor_up, cursor_bottom, page_up, page_down,
--   parent, enter, quit, search, search_next, search_prev,
--   history, shortcuts, copy, move, delete, rename, new_file, new_dir,
--   open_other, open_other_tab, open_external, copy_path, copy_file_ref,
--   mark_down, mark_up, invert_marks, visual, command
----------------------------------------------------------------------
-- cian.set_keymap("x", "delete")   -- add `x` as an alias for delete
-- cian.set_keymap("e", "rename")   -- add `e` as an alias for rename

-- 日本語入力(IME)のまま使う:
--   ・全角英数モード(ｑ ｄ ｊ ｋ …)は設定不要で全コマンドがそのまま動きます。
--   ・ひらがなを直接コマンドに割り当てたい場合は、かなを1文字キーとして指定:
-- cian.set_keymap("あ", "new_file")   -- 「あ」で新規ファイル
-- cian.set_keymap("ふ", "search")     -- 「ふ」で検索 など、覚えやすい字で自由に

----------------------------------------------------------------------
-- Extension-dispatch execution — decide how files open by extension.
-- The handler receives the absolute path as a string. Use cian.spawn{...}
-- to launch a process, or cian.open(path) for the OS default opener.
----------------------------------------------------------------------
-- cian.on_open("md", function(path)
--   cian.spawn({ "open", "-a", "Typora", path })   -- macOS example
-- end)
--
-- cian.on_open("png", function(path)
--   cian.open(path)   -- explicitly hand to the OS default app
-- end)
