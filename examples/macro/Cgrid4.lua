-- 4台のサーバを 2×2 グリッドに並べ、各ペインをパスワードで SSH ログイン。
--
--   ┌──────────────┬──────────────┐
--   │ A@ABCserver  │ B@DEFserver  │   zoom = まずシェルパネルを最大化（F12）
--   ├──────────────┼──────────────┤   sync = オフ — 各ペインは個別に操作
--   │ C@GHIserver  │ D@JKLserver  │   from = どのペインから分割するか（1始まり）、
--   └──────────────┴──────────────┘          本物のグリッドを作る鍵
--
-- 各ペインは `ssh <user>@<host>` を実行し、パスワードプロンプトを待って
-- パスワードを送り、最後までログインします。`expect` は打つ前にプロンプトの
-- テキストを待つので、遅い接続でも先走りません。`send` は行を打って Enter を
-- 押します。
--
-- ⚠ パスワードはここに「平文」で書かれています。init.lua の SSH パスワードと
-- 同じトレードオフです: オプトインで、便利で、ファイルに置かれた秘密 —
-- このファイルを読める人は誰でも読めます。許容できる場面でだけ使ってください
-- （可能なら cian.ssh{} のホスト別秘密 / 鍵認証を優先）。以下のユーザー / ホスト /
-- パスワードはプレースホルダです — 実際のものに置き換えてください。

local function login(from, dir, bg, user, host, password)
  return {
    from = from,                          -- ペイン1は nil（今いるシェル）
    dir  = dir,                           -- "right" | "down"
    cmd  = "ssh " .. user .. "@" .. host, -- ssh user@host
    bg   = bg,
    log  = "~/cian-logs",
    steps = {
      { expect = "assword", timeout = 30 },  -- "Password:" / "password:" に一致
      { send   = password },                 -- パスワード、そして Enter
    },
  }
end

return {
  name = "4 servers (2×2, SSH login)",
  zoom = true,
  sync = false,
  panes = {
    login(nil, nil,     "navy",    "A", "ABCserver", "ABCABC"), -- ペイン1: 左上
    login(1,   "right", "teal",    "B", "DEFserver", "ABCABC"), -- ペイン2: 右上
    login(1,   "down",  "olive",   "C", "GHIserver", "ABCABC"), -- ペイン3: 左下
    login(2,   "down",  "crmaine", "D", "JKLserver", "ABCABC"), -- ペイン4: 右下
  },
}
