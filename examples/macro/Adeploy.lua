-- 1ファイル1マクロ: これらを ~/.config/cian/macro/（ポータブルモードなら実行
-- ファイルの隣）に置きます。ファイル名順に読み込まれ — 先頭の文字は並び順を
-- 決める手軽な手段です — macro.lua の内容と一緒に扱われます。
--
-- ファイル別マクロは、単一の { name =, panes = } テーブルを返します（リストでは
-- ありません）。

return {
  name = "Deploy check",
  panes = {
    { cmd = "ssh deploy@app.example.com", bg = "24,36,28", log = "~/cian-logs" },
    { dir = "down", cmd = "ssh deploy@app.example.com",
      steps = { "cd /srv/app", "git log --oneline -5", "systemctl status app" } },
  },
}
