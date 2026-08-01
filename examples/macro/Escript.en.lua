-- ============================================================================
--  cian — script macros (AFXW-style file-operation automation)
-- ============================================================================
--
--  Where a layout macro (`panes = {...}`) builds the screen, a **script macro**
--  automates *file operations* with `run = function(cx) ... end`. It appears in
--  the `@` / `:macros` / right-click launcher next to the layout macros.
--
--  ★ Synchronous: statements run in order, on the spot, so you can branch on
--    their results.
--  ★ Target: at launch it snapshots the marked entries (or the cursor entry if
--    none) and the active/opposite pane directories.
--
--  The `cx` API (all called as cx.xxx(...)):
--    -- query
--    cx.dir()            the active pane's directory (the working directory)
--    cx.other()          the opposite pane's directory
--    cx.marked()         array of marked paths (or the cursor path)
--    cx.cursor()         the path under the cursor (or nil)
--    cx.list(dir?)       array of paths in a directory (default: the working dir)
--    cx.glob("*.log")    paths in the working dir whose name matches `*`/`?`
--    -- operations (each reloads the panes afterward)
--    cx.copy(paths, dest)   copy (dest folder auto-created; overwrite)
--    cx.move(paths, dest)   move (same)
--    cx.delete(paths)       delete (to the trash)
--    cx.rename(path, name)  rename within the same folder
--    cx.mkdir(name)         make a folder (and parents)
--    cx.zip(paths, out)     bundle into a .zip
--    cx.read(path) / cx.write(path, text)   read/write text
--    -- subprocess (runs in the working directory)
--    cx.sh("cmd")        returns { code=, out=, err= }. ★ runs a real command
--    -- path helpers (pure)
--    cx.basename/stem/ext/join/exists/isdir/size
--    -- feedback
--    cx.message("...")   shown together when the macro finishes
--
--  Note: cx.delete goes to the trash (safe). cx.sh runs real commands. A macro
--  is config you wrote, so it is trusted (same as init.lua).
-- ============================================================================

return {

  -- 1) Sort files into subfolders by extension (txt/, png/, ...)
  {
    name = "Sort by extension",
    run = function(cx)
      local moved = 0
      for _, p in ipairs(cx.glob("*")) do
        if not cx.isdir(p) then
          local e = cx.ext(p)
          if e ~= "" then
            cx.mkdir(e)
            cx.move({ p }, e)
            moved = moved + 1
          end
        end
      end
      cx.message(moved .. " files sorted into extension folders")
    end,
  },

  -- 2) Bundle *.log into a zip and bin them
  {
    name = "Archive logs, then bin them",
    run = function(cx)
      local logs = cx.glob("*.log")
      if #logs == 0 then cx.message("no *.log here") return end
      cx.zip(logs, "logs.zip")
      cx.delete(logs)
      cx.message(#logs .. " logs bundled into logs.zip and removed")
    end,
  },

  -- 3) Copy the marked (or cursor) files to the opposite pane
  {
    name = "Copy selection to other pane",
    run = function(cx)
      local files = cx.marked()
      if #files == 0 then cx.message("nothing selected") return end
      local n = cx.copy(files, cx.other())
      cx.message(n .. " copied to " .. cx.other())
    end,
  },

  -- 4) Stash the marked files in a YYYYMMDD_backup folder
  {
    name = "Backup selection to a dated folder",
    run = function(cx)
      local files = cx.marked()
      if #files == 0 then cx.message("nothing selected") return end
      local folder = os.date("%Y%m%d") .. "_backup"
      cx.mkdir(folder)
      local n = cx.copy(files, folder)
      cx.message(n .. " stashed in " .. folder .. "/")
    end,
  },

  -- 5) Prefix the marked files' names
  {
    name = "Prefix selection with draft_",
    run = function(cx)
      local n = 0
      for _, p in ipairs(cx.marked()) do
        if not cx.isdir(p) then
          cx.rename(p, "draft_" .. cx.basename(p))
          n = n + 1
        end
      end
      cx.message(n .. " renamed")
    end,
  },

  -- 6) Normalise *.txt line endings CRLF -> LF
  {
    name = "Normalise line endings to LF (*.txt)",
    run = function(cx)
      local n = 0
      for _, p in ipairs(cx.glob("*.txt")) do
        local body = cx.read(p)
        local fixed = body:gsub("\r\n", "\n")
        if fixed ~= body then cx.write(p, fixed); n = n + 1 end
      end
      cx.message(n .. " files converted to LF")
    end,
  },

  -- 7) Bin zero-byte empty files
  {
    name = "Clean up empty (0-byte) files",
    run = function(cx)
      local empties = {}
      for _, p in ipairs(cx.glob("*")) do
        if not cx.isdir(p) and cx.size(p) == 0 then
          empties[#empties + 1] = p
        end
      end
      if #empties == 0 then cx.message("no empty files") return end
      cx.delete(empties)
      cx.message(#empties .. " empty files removed")
    end,
  },

  -- 8) List the SHA-256 of each marked file (a cx.sh example)
  --    (shasum / certutil are platform-dependent; adapt as needed.)
  {
    name = "SHA-256 of selection",
    run = function(cx)
      for _, p in ipairs(cx.marked()) do
        if not cx.isdir(p) then
          local r = cx.sh('shasum -a 256 "' .. p .. '"')
          local line = (r.code == 0) and r.out or ("error: " .. r.err)
          cx.message(cx.basename(p) .. "  " .. (line:gsub("%s+$", "")))
        end
      end
    end,
  },

  -- 9) Generate an index.md for this folder
  {
    name = "Generate index.md",
    run = function(cx)
      local lines = { "# " .. cx.basename(cx.dir()), "" }
      for _, p in ipairs(cx.list()) do
        local mark = cx.isdir(p) and "📁 " or "📄 "
        lines[#lines + 1] = "- " .. mark .. cx.basename(p)
      end
      cx.write("index.md", table.concat(lines, "\n") .. "\n")
      cx.message("wrote index.md (" .. (#lines - 2) .. " entries)")
    end,
  },

  -- 10) Flatten one level: move files out of the immediate subfolders
  {
    name = "Flatten one level (subfolder files up)",
    run = function(cx)
      local moved = 0
      for _, d in ipairs(cx.list()) do
        if cx.isdir(d) then
          for _, f in ipairs(cx.list(d)) do
            if not cx.isdir(f) then cx.move({ f }, cx.dir()); moved = moved + 1 end
          end
        end
      end
      cx.message(moved .. " files moved up a level")
    end,
  },

}
