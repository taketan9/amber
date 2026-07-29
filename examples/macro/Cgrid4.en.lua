-- A 2×2 grid of four servers, each logged into over SSH with a password.
--
--   ┌──────────────┬──────────────┐
--   │ A@ABCserver  │ B@DEFserver  │   zoom = maximize the shell panel first (F12)
--   ├──────────────┼──────────────┤   sync = OFF — each pane is driven on its own
--   │ C@GHIserver  │ D@JKLserver  │   from = which pane to split off (1-based),
--   └──────────────┴──────────────┘          the trick that makes a real grid
--
-- Each pane runs `ssh <user>@<host>`, waits for the password prompt, and sends
-- the password — logging in all the way. `expect` waits for the prompt text
-- before typing, so a slow connection does not race ahead; `send` types the line
-- and presses Enter.
--
-- ⚠ The passwords are written here in PLAIN TEXT. Same trade-off as an SSH
-- password in init.lua: opt-in, convenient, and a secret sitting in a file —
-- anyone who can read this file can read them. Use it only where that is
-- acceptable (prefer a per-host secret in cian.ssh{} / key auth when you can).
-- The users / hosts / passwords below are placeholders — replace with real ones.

local function login(from, dir, bg, user, host, password)
  return {
    from = from,                          -- nil for pane 1 (the shell you're on)
    dir  = dir,                           -- "right" | "down"
    cmd  = "ssh " .. user .. "@" .. host, -- ssh user@host
    bg   = bg,
    log  = "~/cian-logs",
    steps = {
      { expect = "assword", timeout = 30 },  -- matches "Password:" / "password:"
      { send   = password },                 -- the password, then Enter
    },
  }
end

return {
  name = "4 servers (2×2, SSH login)",
  zoom = true,
  sync = false,
  panes = {
    login(nil, nil,     "navy",    "A", "ABCserver", "ABCABC"), -- pane 1: top-left
    login(1,   "right", "teal",    "B", "DEFserver", "ABCABC"), -- pane 2: top-right
    login(1,   "down",  "olive",   "C", "GHIserver", "ABCABC"), -- pane 3: bottom-left
    login(2,   "down",  "crmaine", "D", "JKLserver", "ABCABC"), -- pane 4: bottom-right
  },
}
