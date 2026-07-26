-- A 2×2 grid of servers, built and then driven together.
--
--   ┌──────────┬──────────┐
--   │ web1 A   │ web2 B   │   zoom  = maximize the shell panel first (F12)
--   ├──────────┼──────────┤   sync  = after building, type into all 4 at once
--   │ web3 C   │ db1  D   │   from  = which pane to split off (1-based),
--   └──────────┴──────────┘           the trick that makes a real grid
--
-- The `cmd` lines are short because they lean on your ~/.ssh/config Host
-- aliases (or cian.ssh{} hosts) — put the full user@host:port there once and
-- write just the alias here. Each pane is tinted, logs to ~/cian-logs, and
-- drops into bash. Because `sync = true`, once it's up whatever you type goes
-- to all four (the panes wear the amber ⇄ SYNC border) — run one command on
-- every server at once.

return {
  name = "4 servers (2×2)",
  zoom = true,
  sync = true,
  panes = {
    -- pane 1: top-left (the shell you're on)
    { cmd = "ssh web1", bg = "navy",    log = "~/cian-logs", steps = { "bash" } },
    -- pane 2: split pane 1 to the right → top-right
    { from = 1, dir = "right", cmd = "ssh web2", bg = "teal",  log = "~/cian-logs", steps = { "bash" } },
    -- pane 3: split pane 1 downward → bottom-left
    { from = 1, dir = "down",  cmd = "ssh web3", bg = "olive", log = "~/cian-logs", steps = { "bash" } },
    -- pane 4: split pane 2 downward → bottom-right
    { from = 2, dir = "down",  cmd = "ssh db1",  bg = "crmaine", log = "~/cian-logs", steps = { "bash" } },
  },
}
