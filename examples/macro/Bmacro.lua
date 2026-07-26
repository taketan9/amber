-- A scripted-login macro — the "wait for the prompt, then type" kind.
--
-- `steps` are played out over time, one per tick, so a step can pause or wait
-- for a prompt instead of dumping every line at once:
--   "text"                          -- send a line (shorthand for { send = })
--   { send = "text" }               -- send a line
--   { wait = 2 }                    -- pause 2 seconds
--   { expect = "text" }             -- wait until "text" appears (case-insensitive)
--   { expect = "text", timeout = 20 } -- ...but give up after 20s and move on
--
-- This one logs into a DB box and drops straight into a SQL session, waiting
-- for each prompt so it works over a slow link instead of racing ahead.

return {
  name = "DB login (scripted)",
  panes = {
    {
      cmd = "ssh admin@db.example.com",
      bg = "36,24,28",
      log = "~/cian-logs",
      steps = {
        { expect = "password:", timeout = 20 },  -- wait for the ssh prompt
        { send = "hunter2" },                     -- (better: use key auth!)
        { expect = "$", timeout = 15 },           -- wait for the shell prompt
        { send = "sqlplus /nolog" },
        { expect = "SQL>" },
        { send = "connect app/secret@orclpdb" },
        { expect = "Connected" },
        { wait = 1 },
        { send = "select sysdate from dual;" },
      },
    },
  },
}
