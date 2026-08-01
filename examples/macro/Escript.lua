-- ============================================================================
--  cian — スクリプトマクロ集（ファイル操作を自動化する AFXW 流マクロ）
-- ============================================================================
--
--  レイアウトマクロ（`panes = {...}`）が「画面を組む」のに対し、スクリプト
--  マクロは `run = function(cx) ... end` で「ファイル操作を自動化」します。
--  `@` / `:macros` / 右クリックのランチャーに、レイアウトマクロと並んで出ます。
--
--  ★ 同期実行：上から順に、その場で実行されます（結果で分岐できます）。
--  ★ 対象：起動時の「マーク（無ければカーソル）」「アクティブ/反対ペインのcwd」を
--    スナップショットとして受け取ります。
--
--  cx の API（すべて cx.xxx(...) 形式で呼ぶ）:
--    -- 取得
--    cx.dir()            アクティブペインのディレクトリ（作業ディレクトリ）
--    cx.other()          反対ペインのディレクトリ
--    cx.marked()         マーク（無ければカーソル）のパス配列
--    cx.cursor()         カーソル位置のパス（無ければ nil）
--    cx.list(dir?)       ディレクトリ内のパス配列（省略時は作業ディレクトリ）
--    cx.glob("*.log")    作業ディレクトリ内で `*`/`?` に一致する名前のパス配列
--    -- 操作（実行するとパネルは自動リロード）
--    cx.copy(paths, dest)   コピー（destフォルダは自動作成、上書き）
--    cx.move(paths, dest)   移動（同上）
--    cx.delete(paths)       削除（ゴミ箱へ）
--    cx.rename(path, name)  同じフォルダ内でリネーム
--    cx.mkdir(name)         フォルダ作成（親ごと）
--    cx.zip(paths, out)     zip にまとめる
--    cx.read(path) / cx.write(path, text)   テキスト読み書き
--    -- サブプロセス（作業ディレクトリで実行）
--    cx.sh("cmd")        戻り値 { code=, out=, err= }。★実際にコマンドが走る
--    -- パス補助（純粋関数）
--    cx.basename/stem/ext/join/exists/isdir/size
--    -- 表示
--    cx.message("...")   完了後にまとめて表示
--
--  注意：cx.delete はゴミ箱行き（安全）。cx.sh は本物のコマンドを実行します。
--  マクロは自分で書いた設定なので信頼前提です（init.lua と同じ扱い）。
-- ============================================================================

return {

  -- 1) 拡張子ごとにサブフォルダへ仕分け（txt/, png/, ...）
  {
    name = "拡張子ごとに仕分け",
    run = function(cx)
      local moved = 0
      for _, p in ipairs(cx.glob("*")) do
        if not cx.isdir(p) then
          local e = cx.ext(p)
          if e ~= "" then
            cx.mkdir(e)
            cx.move({ p }, e)
            moved = moved + 1
          end
        end
      end
      cx.message(moved .. " 件を拡張子フォルダへ仕分けしました")
    end,
  },

  -- 2) *.log を zip にまとめてゴミ箱へ
  {
    name = "ログをzip化して掃除",
    run = function(cx)
      local logs = cx.glob("*.log")
      if #logs == 0 then cx.message("*.log はありません") return end
      cx.zip(logs, "logs.zip")
      cx.delete(logs)
      cx.message(#logs .. " 件のログを logs.zip にまとめて削除しました")
    end,
  },

  -- 3) マーク（無ければカーソル）を反対ペインへコピー（簡易ミラー）
  {
    name = "選択を反対ペインへコピー",
    run = function(cx)
      local files = cx.marked()
      if #files == 0 then cx.message("対象がありません") return end
      local n = cx.copy(files, cx.other())
      cx.message(n .. " 件を " .. cx.other() .. " へコピーしました")
    end,
  },

  -- 4) マークを「YYYYMMDD_バックアップ」フォルダに退避
  {
    name = "日付フォルダにバックアップ",
    run = function(cx)
      local files = cx.marked()
      if #files == 0 then cx.message("対象がありません") return end
      local folder = os.date("%Y%m%d") .. "_backup"
      cx.mkdir(folder)
      local n = cx.copy(files, folder)
      cx.message(n .. " 件を " .. folder .. "/ に退避しました")
    end,
  },

  -- 5) マークしたファイル名に接頭辞を付ける
  {
    name = "選択に接頭辞 draft_ を付与",
    run = function(cx)
      local n = 0
      for _, p in ipairs(cx.marked()) do
        if not cx.isdir(p) then
          cx.rename(p, "draft_" .. cx.basename(p))
          n = n + 1
        end
      end
      cx.message(n .. " 件をリネームしました")
    end,
  },

  -- 6) *.txt の改行を CRLF → LF に統一
  {
    name = "改行を LF に統一 (*.txt)",
    run = function(cx)
      local n = 0
      for _, p in ipairs(cx.glob("*.txt")) do
        local body = cx.read(p)
        local fixed = body:gsub("\r\n", "\n")
        if fixed ~= body then cx.write(p, fixed); n = n + 1 end
      end
      cx.message(n .. " 件の改行を LF にしました")
    end,
  },

  -- 7) 0バイトの空ファイルをゴミ箱へ
  {
    name = "空ファイル(0byte)を掃除",
    run = function(cx)
      local empties = {}
      for _, p in ipairs(cx.glob("*")) do
        if not cx.isdir(p) and cx.size(p) == 0 then
          empties[#empties + 1] = p
        end
      end
      if #empties == 0 then cx.message("空ファイルはありません") return end
      cx.delete(empties)
      cx.message(#empties .. " 件の空ファイルを削除しました")
    end,
  },

  -- 8) マークした各ファイルのSHA-256を取って一覧表示（sh の使用例）
  --    ※ shasum / certutil は環境依存。無ければ err が返ります。
  {
    name = "選択のSHA-256を一覧",
    run = function(cx)
      for _, p in ipairs(cx.marked()) do
        if not cx.isdir(p) then
          -- Windows は certutil、その他は shasum を想定（適宜書き換えて）
          local r = cx.sh('shasum -a 256 "' .. p .. '"')
          local line = (r.code == 0) and r.out or ("error: " .. r.err)
          cx.message(cx.basename(p) .. "  " .. (line:gsub("%s+$", "")))
        end
      end
    end,
  },

  -- 9) このフォルダの目次 index.md を生成
  {
    name = "目次 index.md を生成",
    run = function(cx)
      local lines = { "# " .. cx.basename(cx.dir()), "" }
      for _, p in ipairs(cx.list()) do
        local mark = cx.isdir(p) and "📁 " or "📄 "
        lines[#lines + 1] = "- " .. mark .. cx.basename(p)
      end
      cx.write("index.md", table.concat(lines, "\n") .. "\n")
      cx.message("index.md を書き出しました（" .. (#lines - 2) .. " 項目）")
    end,
  },

  -- 10) 直下のサブフォルダのファイルを、この階層に平坦化（1段だけ）
  {
    name = "1段フラット化（サブfolderの中身を上へ）",
    run = function(cx)
      local moved = 0
      for _, d in ipairs(cx.list()) do
        if cx.isdir(d) then
          for _, f in ipairs(cx.list(d)) do
            if not cx.isdir(f) then cx.move({ f }, cx.dir()); moved = moved + 1 end
          end
        end
      end
      cx.message(moved .. " 件を1階層上へ移動しました")
    end,
  },

}
