-- ============================================================================
--  cian — file / step counter  (the `:count` command)
-- ============================================================================
--
--  `:count` tallies files, lines and "steps" (source lines) under the target:
--  the marked entries if any, otherwise the active pane's whole directory tree.
--  This file tunes what gets counted. It is optional — without it, cian counts
--  every text file and reports steps as non-blank, non-comment lines.
--
--  Where it goes (next to init.lua):
--    Linux / macOS : ~/.config/cian/count.lua
--    Windows       : %USERPROFILE%\.config\cian\count.lua
--  Portable: a count.lua next to the cian executable wins over this.
-- ============================================================================

return {
  -- Only count these extensions. Omit (or leave empty) to count every text
  -- file. Case- and dot-insensitive: "rs", ".RS" and "Rs" are the same.
  extensions = { "rs", "lua", "py", "js", "ts", "go", "c", "h", "cpp" },

  -- Should blank lines count as steps? Off = kazoechao-style SLOC.
  count_blank = false,

  -- Should comment lines count as steps?
  count_comments = false,

  -- What starts a line comment. A line whose first non-space text begins with
  -- one of these is a comment. (Block comments are not tracked.)
  comment_prefixes = { "//", "#", "--", ";", "*" },
}
