-- The smallest possible 2×2 grid — layout only, no SSH — for checking that a
-- macro builds a real grid rather than four side-by-side columns.
--
--   ┌──────────────┬──────────────┐
--   │   pane 1     │   pane 2     │
--   ├──────────────┼──────────────┤
--   │   pane 3     │   pane 4     │
--   └──────────────┴──────────────┘
--
-- The trick that makes a grid (instead of a 4-column row) is two things on
-- panes 3 and 4:
--   * dir = "down"  — split downward, not to the right
--   * from = N      — split off an *earlier* pane (1-based), not the previous one
--
-- pane 2 splits pane 1 to the right; pane 3 splits pane 1 downward; pane 4
-- splits pane 2 downward. If every pane came out in one row, check that panes 3
-- and 4 really say dir = "down" (and have their `from`).

return {
  name = "2×2 grid (layout test)",
  zoom = true,   -- maximize the shell panel first, so the grid has the whole window
  sync = false,  -- each pane is independent
  panes = {
    { },                          -- pane 1: the shell you are on
    { from = 1, dir = "right" },  -- pane 2: right of pane 1
    { from = 1, dir = "down"  },  -- pane 3: below pane 1
    { from = 2, dir = "down"  },  -- pane 4: below pane 2
  },
}
