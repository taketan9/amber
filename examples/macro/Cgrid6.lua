-- 6ペインのグリッド（2列 × 3行）— 埋めて使う骨組み。
--
--   ┌──────────┬──────────┐
--   │ pane 1   │ pane 2   │   zoom  = まずシェルパネルを最大化（F12）
--   ├──────────┼──────────┤   sync  = オフ — 各ペインは個別に操作
--   │ pane 3   │ pane 4   │   from  = どのペインから分割するか（1始まり）
--   ├──────────┼──────────┤   ratio = 分割元ペインが「残す」割合（%）。行がきれいな
--   │ pane 5   │ pane 6   │           三等分（33 → 50）になるよう。1/2, 1/4, 1/4
--   └──────────┴──────────┘           にならないための指定。
--
-- 骨組みとして各ペインは `bash` に入るだけです。ペインをサーバへログインさせるには
-- examples/macro/Cgrid4.lua のように `cmd` + `steps` を与えます:
--   cmd = "ssh A@ABCserver",
--   steps = { { expect = "assword", timeout = 30 }, { send = "ABCABC" } },
-- （⚠ ここに書くパスワードは平文 — init.lua と同じトレードオフです。）

local function pane(from, dir, ratio, bg)
  return {
    from  = from,       -- ペイン1は nil（今いるシェル）
    dir   = dir,        -- "right" | "down"
    ratio = ratio,      -- nil = 50/50
    bg    = bg,
    log   = "~/cian-logs",
    steps = { "bash" }, -- ssh + ログインさせるなら cmd/steps に置き換え
  }
end

return {
  name = "6 panes (2×3 grid)",
  zoom = true,
  sync = false,
  panes = {
    pane(nil, nil,     nil, "navy"),    -- ペイン1: 左上（今いるシェル）
    pane(1,   "right", 50,  "teal"),    -- ペイン2: ペイン1を左右2等分に分割
    pane(1,   "down",  33,  "olive"),   -- ペイン3: 左列、ペイン1が上1/3を残す
    pane(2,   "down",  33,  "crimson"), -- ペイン4: 右列、同上
    pane(3,   "down",  50,  "crmaine"), -- ペイン5: 左の残りを均等に分割
    pane(4,   "down",  50,  "plum"),    -- ペイン6: 右の残りを均等に分割
  },
}
