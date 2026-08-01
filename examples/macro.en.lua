-- ============================================================================
--  cian — macros (the `@` menu)
-- ============================================================================
--
--  Press `@` (vim's play-a-macro key) to open the launcher and pick one, or run
--  `:macros`. A macro builds a whole shell layout in one keystroke.
--
--  Note: there are also "script macros" (run = function(cx) …) that automate
--  file operations; they appear in the launcher tagged ⚙. See
--  examples/macro/Escript.en.lua.
--
--  Where it goes (next to init.lua):
--    Linux / macOS : ~/.config/cian/macro.lua
--    Windows       : %USERPROFILE%\.config\cian\macro.lua
--  Portable: a macro.lua sitting next to the cian executable wins over this.
--
--  One macro per file: instead of (or as well as) this list, put a `macro/`
--  directory next to init.lua and give each macro its own file, e.g.
--  macro/Adeploy.lua, macro/Bdbcheck.lua. They load in filename order and each
--  returns a single { name =, panes = } table. See examples/macro/Adeploy.lua.
--
--  Run one at startup, TeraTerm-.ttl style:  cian --macro path/to/thing.lua
--  (a bare `cian thing.lua` works too, so a .lua file associated with cian.exe
--  runs on double-click), or  cian --macro-name "Two local shells".
--
--  A macro returns { name = ..., panes = { ... }, sync = false }. Set
--  `sync = true` to turn on input broadcast (synchronize) once the layout is
--  built, so the same keystrokes then reach every pane at once. The FIRST pane
--  is the shell
--  pane you are on; each later pane is split off the previous one:
--    dir   = "right" (side by side) or "down" (stacked). Default "right".
--    from  = which earlier pane to split off (1-based; default = the previous
--            one). This is how you build a real grid — see macro/Cgrid4.lua.
--    cmd   = a command line to run (typed, then Enter) — e.g. an ssh line.
--    steps = a scripted sequence run after cmd, played out over time so it can
--            wait for a prompt instead of racing ahead. Each step is:
--              "text"                       -- send a line (Enter)
--              { send = "text" }            -- send a line
--              { wait = 2 }                 -- pause 2 seconds
--              { expect = "SQL>" }          -- wait until text appears
--              { expect = "pw:", timeout=20 } -- ...or give up after 20s
--            See examples/macro/Bmacro.lua for a scripted DB login.
--    bg    = pane background colour ("#rrggbb", a name, or "r,g,b"), so each
--            pane is easy to tell apart.
--    log   = a directory to start a session log in for that pane.
-- ============================================================================

return {
  -- A three-pane working set: two servers plus a live log tail.
  { name = "Prod: db + app + logs", panes = {
    { cmd = "ssh admin@db.example.com",  bg = "40,24,24", log = "~/cian-logs" },
    { dir = "right", cmd = "ssh admin@app.example.com", bg = "24,40,24" },
    { dir = "down",  cmd = "ssh admin@app.example.com", steps = { "tail -f /var/log/app/app.log" } },
  }},

  -- Drop straight into a database session (bash → sqlplus → connect).
  { name = "Oracle sqlplus login", panes = {
    { cmd = "bash", steps = {
        "sqlplus /nolog",
        "connect scott/tiger@orclpdb",
        "select sysdate from dual;",
      } },
  }},

  -- A quick side-by-side pair of local shells.
  { name = "Two local shells", panes = {
    { bg = "20,28,40" },
    { dir = "right", bg = "40,28,20" },
  }},
}
