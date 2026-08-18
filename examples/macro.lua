-- ============================================================================
--  cian — マクロ（`@` メニュー）
-- ============================================================================
--
--  `@`（vim のマクロ再生キー）でランチャーを開いて選ぶか、`:macros` を実行。
--  マクロは1キーでシェルのレイアウト一式を組み立てます。
--
--  ※ ファイル操作を自動化する「スクリプトマクロ」（run = function(cx) …）も
--     あります。ランチャーに § 印で並びます。例は examples/macro/Escript.lua。
--
--  置き場所（init.lua の隣）:
--    Linux / macOS : ~/.config/cian/macro.lua
--    Windows       : %USERPROFILE%\.config\cian\macro.lua
--  ポータブル: cian 実行ファイルの隣にある macro.lua が優先されます。
--
--  1ファイル1マクロ: このリストの代わりに（または併用して）、init.lua の隣に
--  `macro/` ディレクトリを置き、マクロごとに1ファイルにできます。例:
--  macro/Adeploy.lua、macro/Bdbcheck.lua。ファイル名順に読み込まれ、各ファイルは
--  単一の { name =, panes = } テーブルを返します。examples/macro/Adeploy.lua 参照。
--
--  起動時に1つ実行（TeraTerm の .ttl 風）:  cian --macro path/to/thing.lua
--  （素の `cian thing.lua` でも動くので、cian.exe に関連付けた .lua は
--  ダブルクリックで実行できます）。または  cian --macro-name "Two local shells"。
--
--  マクロは { name = ..., panes = { ... }, sync = false } を返します。レイアウトを
--  組んだ後に入力ブロードキャスト（同期）を有効化するには `sync = true`。以降は
--  同じキー入力が全ペインに同時に届きます。最初のペインは今いるシェルペインで、
--  以降の各ペインは前のペインから分割されます:
--    dir   = "right"（横並び）または "down"（縦積み）。既定は "right"。
--    from  = どの前ペインから分割するか（1始まり。既定は直前）。これが本物の
--            グリッドを作る鍵です — macro/Cgrid4.lua 参照。
--    cmd   = 実行するコマンド行（打ち込んで Enter）— 例えば ssh の行。
--    steps = cmd の後に走る台本。時間をかけて再生されるので、先走らずに
--            プロンプトを待てます。各ステップは:
--              "text"                       -- 1行送る（Enter）
--              { send = "text" }            -- 1行送る
--              { wait = 2 }                 -- 2秒待つ
--              { expect = "SQL>" }          -- テキストが現れるまで待つ
--              { expect = "pw:", timeout=20 } -- …または20秒で諦める
--            台本による DB ログインは examples/macro/Bmacro.lua 参照。
--    bg    = ペインの背景色（"#rrggbb"・色名・"r,g,b"）。各ペインを見分けやすく。
--    log   = そのペインでセッションログを開始するディレクトリ。
-- ============================================================================

return {
  -- 3ペインの作業セット: サーバ2台に加えてログのライブ追尾。
  { name = "Prod: db + app + logs", panes = {
    { cmd = "ssh admin@db.example.com",  bg = "40,24,24", log = "~/cian-logs" },
    { dir = "right", cmd = "ssh admin@app.example.com", bg = "24,40,24" },
    { dir = "down",  cmd = "ssh admin@app.example.com", steps = { "tail -f /var/log/app/app.log" } },
  }},

  -- そのままデータベースセッションへ（bash → sqlplus → connect）。
  { name = "Oracle sqlplus login", panes = {
    { cmd = "bash", steps = {
        "sqlplus /nolog",
        "connect scott/tiger@orclpdb",
        "select sysdate from dual;",
      } },
  }},

  -- ローカルシェルをさっと横並びで2つ。
  { name = "Two local shells", panes = {
    { bg = "20,28,40" },
    { dir = "right", bg = "40,28,20" },
  }},
}
