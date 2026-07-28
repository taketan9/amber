-- A 6-pane grid (2 columns × 3 rows) — a skeleton to fill in.
--
--   ┌──────────┬──────────┐
--   │ pane 1   │ pane 2   │   zoom  = maximize the shell panel first (F12)
--   ├──────────┼──────────┤   sync  = OFF — each pane is driven on its own
--   │ pane 3   │ pane 4   │   from  = which pane to split off (1-based)
--   ├──────────┼──────────┤   ratio = % the source pane KEEPS, so the rows come
--   │ pane 5   │ pane 6   │           out even thirds (33 then 50) instead of
--   └──────────┴──────────┘           1/2, 1/4, 1/4.
--
-- As a skeleton each pane just drops into `bash`. To make a pane log into a
-- server, give it `cmd` + `steps` like examples/macro/Cgrid4.lua:
--   cmd = "ssh A@ABCserver",
--   steps = { { expect = "assword", timeout = 30 }, { send = "ABCABC" } },
-- (⚠ a password written here is plain text — same trade-off as init.lua.)

local function pane(from, dir, ratio, bg)
  return {
    from  = from,       -- nil for pane 1 (the shell you're on)
    dir   = dir,        -- "right" | "down"
    ratio = ratio,      -- nil = 50/50
    bg    = bg,
    log   = "~/cian-logs",
    steps = { "bash" }, -- replace with cmd/steps to ssh + log in
  }
end

return {
  name = "6 panes (2×3 grid)",
  zoom = true,
  sync = false,
  panes = {
    pane(nil, nil,     nil, "navy"),    -- pane 1: top-left (the shell you're on)
    pane(1,   "right", 50,  "teal"),    -- pane 2: split pane 1 → two even columns
    pane(1,   "down",  33,  "olive"),   -- pane 3: left column, top third kept by 1
    pane(2,   "down",  33,  "crimson"), -- pane 4: right column, same
    pane(3,   "down",  50,  "crmaine"), -- pane 5: split the left remainder evenly
    pane(4,   "down",  50,  "plum"),    -- pane 6: split the right remainder evenly
  },
}
