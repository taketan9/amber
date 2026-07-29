-- ============================================================================
--  cian — ショートカット（`s` メニュー）
-- ============================================================================
--
--  このファイルはアプリ内から管理します。`s` でメニューを開き、`a` で
--  ショートカット追加、`A` でフォルダ追加、`r` でリネーム、`d` で削除。自動で
--  書き戻されるので手編集は任意です — この例は入れ子を含めた「形」を示すだけの
--  ものです。
--
--  置き場所（init.lua の隣）:
--    Linux / macOS : ~/.config/cian/shortcuts.lua
--    Windows       : %USERPROFILE%\.config\cian\shortcuts.lua
--
--  このファイルはリストを返します。各エントリは次のいずれか:
--    * ショートカット — `target`（パス・URL・アプリ/コマンド）を持つ
--    * フォルダ       — `children`（同じ形の入れ子リスト）を持つ
--  フォルダは好きなだけ入れ子にできます。メニューでは Enter / → でフォルダに
--  入り、Esc / ← で戻り、`A` で今いる階層に新しいフォルダを作ります。
-- ============================================================================

return {
  -- --- 素の、最上位ショートカット -------------------------------------------
  { name = "home", target = "~" },
  { name = "Downloads", target = "~/Downloads" },

  -- --- 関連する行き先をまとめるフォルダ -------------------------------------
  { name = "Projects", children = {
    { name = "cian", target = "~/workspace/cian" },
    { name = "crmaine", target = "~/workspace/crmaine" },
    { name = "scratch", target = "~/workspace/scratch" },
  } },

  -- --- フォルダは入れ子にでき、URL やアプリも入れられる ---------------------
  { name = "Web", children = {
    { name = "GitHub", target = "https://github.com" },
    { name = "Docs", children = {
      { name = "Rust std", target = "https://doc.rust-lang.org/std/" },
      { name = "ratatui", target = "https://docs.rs/ratatui/" },
    } },
  } },
}
