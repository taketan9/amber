-- One macro per file: drop these in ~/.config/cian/macro/ (or next to the
-- executable in portable mode). They load in filename order — the leading
-- letter is a handy way to order them — alongside anything in macro.lua.
--
-- A per-file macro returns a single { name =, panes = } table (not a list).

return {
  name = "Deploy check",
  panes = {
    { cmd = "ssh deploy@app.example.com", bg = "24,36,28", log = "~/cian-logs" },
    { dir = "down", cmd = "ssh deploy@app.example.com",
      steps = { "cd /srv/app", "git log --oneline -5", "systemctl status app" } },
  },
}
