-- 台本ログインのマクロ — 「プロンプトを待ってから打つ」タイプ。
--
-- `steps` は時間をかけて1ティックに1つずつ再生されるので、全行を一度に吐き出さず
-- 一時停止したりプロンプトを待ったりできます:
--   "text"                          -- 1行送る（{ send = } の短縮形）
--   { send = "text" }               -- 1行送る
--   { wait = 2 }                    -- 2秒待つ
--   { expect = "text" }             -- "text" が現れるまで待つ（大文字小文字無視）
--   { expect = "text", timeout = 20 } -- …ただし20秒で諦めて次へ
--
-- これは DB サーバにログインしてそのまま SQL セッションに入ります。プロンプトを
-- 1つずつ待つので、先走らず遅い回線でも動きます。

return {
  name = "DB login (scripted)",
  panes = {
    {
      cmd = "ssh admin@db.example.com",
      bg = "36,24,28",
      log = "~/cian-logs",
      steps = {
        { expect = "password:", timeout = 20 },  -- ssh のプロンプトを待つ
        { send = "hunter2" },                     -- （鍵認証の方が良い！）
        { expect = "$", timeout = 15 },           -- シェルのプロンプトを待つ
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
