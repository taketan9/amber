-- A 2×2 grid of servers, each logged into over SSH with a password.
--
--   ┌──────────┬──────────┐
--   │ pane 1 A │ pane 2 B │   zoom = maximize the shell panel first (F12)
--   ├──────────┼──────────┤   sync = OFF here — each pane is driven on its own
--   │ pane 3 C │ pane 4 D │   from = which pane to split off (1-based), the
--   └──────────┴──────────┘          trick that makes a real grid
--
-- Each pane runs `ssh A@ABCserver`, waits for the password prompt, and sends the
-- password — logging in all the way. `expect` waits for the prompt text before
-- typing, so a slow connection does not race ahead; `send` types the line and
-- presses Enter.
--
-- ⚠ The password is written here in PLAIN TEXT. Same trade-off as an SSH
-- password in init.lua: it is opt-in, convenient, and a secret sitting in a
-- file — anyone who can read this file can read the password. Use it only where
-- that is acceptable (and prefer a per-host secret in cian.ssh{} / key auth
-- when you can). "A" / "ABCserver" / "ABCABC" below are placeholders — replace
-- them with your real user / host / password.

local function login(from, dir, bg)
  return {
    from = from,               -- nil for pane 1 (the shell you're on)
    dir  = dir,                -- "right" | "down"
    cmd  = "ssh A@ABCserver",  -- user A @ host ABCserver
    bg   = bg,
    log  = "~/cian-logs",
    steps = {
      { expect = "assword", timeout = 30 },  -- matches "Password:" / "password:"
      { send   = "ABCABC" },                 -- the password, then Enter
    },
  }
end

return {
  name = "4 servers (2×2, SSH login)",
  zoom = true,
  sync = false,
  panes = {
    login(nil, nil,     "navy"),    -- pane 1: top-left (the shell you're on)
    login(1,   "right", "teal"),    -- pane 2: split pane 1 → top-right
    login(1,   "down",  "olive"),   -- pane 3: split pane 1 → bottom-left
    login(2,   "down",  "crmaine"), -- pane 4: split pane 2 → bottom-right
  },
}
