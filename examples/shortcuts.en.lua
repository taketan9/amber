-- ============================================================================
--  cian — shortcuts (the `s` menu)
-- ============================================================================
--
--  This file is managed from inside the app: press `s` to open the menu, then
--  `a` to add a shortcut, `A` to add a folder, `r` to rename, `d` to delete.
--  It is written back automatically, so hand-editing is optional — this example
--  just shows the shape, including nesting.
--
--  Where it goes (next to init.lua):
--    Linux / macOS : ~/.config/cian/shortcuts.lua
--    Windows       : %USERPROFILE%\.config\cian\shortcuts.lua
--
--  The file returns a list. Each entry is either:
--    * a shortcut — has a `target` (a path, a URL, or an app/command), or
--    * a folder   — has `children` (a nested list of the same shape).
--  Folders can nest as deep as you like. In the menu, Enter / → steps into a
--  folder, Esc / ← steps back out, and `A` makes a new folder at the level
--  you are currently in.
-- ============================================================================

return {
  -- --- plain, top-level shortcuts -------------------------------------------
  { name = "home", target = "~" },
  { name = "Downloads", target = "~/Downloads" },

  -- --- a folder, grouping related destinations ------------------------------
  { name = "Projects", children = {
    { name = "cian", target = "~/workspace/cian" },
    { name = "crmaine", target = "~/workspace/crmaine" },
    { name = "scratch", target = "~/workspace/scratch" },
  } },

  -- --- folders can nest, and hold URLs and apps too -------------------------
  { name = "Web", children = {
    { name = "GitHub", target = "https://github.com" },
    { name = "Docs", children = {
      { name = "Rust std", target = "https://doc.rust-lang.org/std/" },
      { name = "ratatui", target = "https://docs.rs/ratatui/" },
    } },
  } },
}
