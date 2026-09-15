#!/usr/bin/env bash
# onenote-test.py の検査を、一つずつ壊して鳴らせる。
#
# `scripts/mutate.sh` と同じ理由でこれは script になっている ── 一度、手で
# 打ち直した版が黙って通り、「検査が効かない」のと見分けがつかなかった。
# 壊し方を書き留めておけば、次に検査を足した人も同じ手で確かめられる。
#
#   scripts/onenote-mutate.sh
set -uo pipefail
cd "$(dirname "$0")/.."

TARGET=scripts/onenote2md.py
keep=$(mktemp)
cp "$TARGET" "$keep"
restore() { cp "$keep" "$TARGET"; }
trap 'restore; rm -f "$keep"' EXIT

restore
if ! python3 scripts/onenote-test.py >/dev/null 2>&1; then
    echo "先に走査が落ちています。壊す前に直してください。"
    exit 1
fi

silent=0

mutate() {  # 名前 / 鳴ってほしい検査 / 何を / 何に
    local name="$1" want="$2" from="$3" to="$4"
    restore
    # **本当に変わったかは、中身の指紋で見る。** はじめ `grep -F "$to"` で
    # 見ていたが、`to` が複数行だと grep はそれを**行の並び**と読み、
    # どれか一行が元からあれば通る ── 一文字も置換されていないのに
    # 「壊した」と言い、続く「鳴らなかった」が検査のせいに見えた。
    local was
    was=$(shasum "$TARGET" | cut -d" " -f1)
    FROM="$from" TO="$to" perl -0pi -e 's/\Q$ENV{FROM}\E/$ENV{TO}/g' "$TARGET"
    if [ "$was" = "$(shasum "$TARGET" | cut -d" " -f1)" ]; then
        echo "★置換できず $name ── その文字列がそのままでは無い"
        silent=$((silent + 1))
        return
    fi
    local out
    out=$(python3 scripts/onenote-test.py 2>&1)
    if printf '%s\n' "$out" | grep -qF "NG   $want"; then
        echo "鳴った   $name"
    else
        echo "★黙った $name  →  $want"
        printf '%s\n' "$out" | grep -F "NG " | head -5
        silent=$((silent + 1))
    fi
}

mutate "--prune の範囲を出力先ぜんたいに戻す" "絞らなかったノートブックは残る（前は全滅した）" \
    'for nb_dir in scope:' 'for nb_dir in [scope[0].parent] if scope else []:'
mutate "鍵のかかったセクションを覚えない" "鍵のかかったセクションの下は残る" \
    'keep.add_dir(out_dir / sec_name)' 'pass'
mutate "自分で書いたノートも消す" "自分で書いたノートは残る" \
    'if not oid:
                continue  # ambər で作ったノートは触らない' 'if False:
                continue'
mutate "取れなかったページを守らない" "前の版が残る" \
    '            written.add(md_path.resolve())
            continue
        try:' '            continue
        try:'
mutate "落ちても 0 を返す" "落ちた回は 0 を返さない" \
    'return 1 if stats["errors"] else 0' 'return 0'
mutate "--no-images の守りを外す" "--no-images でも既にある画像は消さない" \
    'if not args.no_images and page_img_dir.is_dir():' 'if page_img_dir.is_dir():'
mutate "いま使っている画像まで消す" "いま使っている画像は残る" \
    'if old.name not in conv.images:' 'if True:'
mutate "隣のページの画像まで巻き込む" "名前が似ているだけの画像は巻き込まない" \
    'page_img_dir.glob(f"{md_path.stem}_*")' 'page_img_dir.glob("*")'
mutate "鎖を掛けない" "二本目は断られる" \
    'fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)' 'pass'
mutate "--dry-run でも同期を頼む" "--dry-run では頼まない" \
    'if args.sync and not args.dry_run and not args.list:' 'if args.sync and not args.list:'
mutate "同期のとき絞りを見ない" "絞ったら、そのノートブックだけに頼む" \
    'if filters and not any(f.lower() in name.lower() for f in filters):
            continue
        ids.append' 'ids.append'
mutate "変わっていないページも取りに行く" "二回目は一枚も取りに行かない" \
    'if not args.force and md_path.exists():' 'if False:'
mutate "同じページを毎回ずらす" "三回走らせても増えない" \
    'if not head.get("onenote_id") or head.get("onenote_id") == page_id:' 'if False:'
mutate "ゴミ箱のページも出す" "ゴミ箱のページは出ない" \
    'if page.get("isInRecycleBin") == "true":' 'if False:'
mutate "--only を見ない" "--only はそのセクションだけ" \
    'if not chosen("/".join(path_parts + [raw]), args):' 'if False:'
mutate "--skip を見ない" "--skip は外す" \
    'if args.skip and any(pat.lower() in low for pat in args.skip):
        return False' 'if False:
        return False'
mutate "--only が --skip に負けない" "--skip は --only より強い" \
    'if args.skip and any(pat.lower() in low for pat in args.skip):
        return False
    return True' 'return True'
mutate "大文字小文字を区別する" "大文字小文字は問わない" \
    'if args.only and not any(pat.lower() in low for pat in args.only):' \
    'if args.only and not any(pat in sec_path for pat in args.only):'
mutate "絞りを畳んだ名前に当てる" "畳んだ名前では拾わない（同じ --only が旗で違うものを拾わない）" \
    'if not chosen("/".join(path_parts + [raw]), args):' \
    'if not chosen("/".join(path_parts + [out_name]), args):'
mutate "外したセクションを守らない" "絞って外したセクションの写しは残る（前は全滅した）" \
    'keep.add_dir(out_dir / out_name)
                keep.add_pages_of(child)' 'pass'
mutate "外したセクションを道でしか守らない" "畳んだときも、外したセクションの写しは残る" \
    'keep.add_pages_of(child)' 'pass'
mutate "--list が書き出しに進む" "--list は書かない" \
    'if args.list:
        list_sections(root, args)
        return 0' 'if False:
        return 0'
mutate "--list が絞りを映さない" "外れるものに × が付く" \
    'mark = "  " if chosen(path, args) else "× "' 'mark = "  "'
mutate "早い束ねの形を試さない" "早い束ねでも同じものが出る" \
    '("", HS_PAGES),
                     ("", HS_PAGES, ""))' '("", HS_PAGES, ""))'
mutate "GetPageContent の [out] を末尾だと思う" "遅い束ねでも同じものが出る" \
    '(page_id, info),
                     (page_id, "", info))' '(page_id, info),
                     (page_id, info, ""))'
mutate "例外の有無だけで見分ける" "性悪でも同じものが出る" \
    'if isinstance(out, str) and out.lstrip().startswith("<"):
            return out' 'if True:
            return out'
mutate "どの形でも駄目なときに黙る" "どの形でも駄目なら黙らない" \
    'raise trouble' 'return ""'
mutate "話が通じるかを確かめない" "「見えない」と「呼べない」を言い分ける" \
    'if not hasattr(app, "GetHierarchy"):' 'if False:'
mutate "版を名指しせず gencache だけに頼る" "型ライブラリを GUID と版で名指しする" \
    'mod = gencache.EnsureModule(ONENOTE_TYPELIB, 0, major, minor)' 'mod = None'
mutate "どちらの段も同じ版を掴む" "版が二つあるなら、もう一方を試す" \
    '("型ライブラリ 1.0 を名指し", by_typelib(1, 0)),' '("型ライブラリ 1.0 を名指し", by_typelib(1, 1)),'
mutate "繋げないとき黙って返る" "全部駄目なら、わけを並べて止まる" \
    'raise CannotConnect("OneNote (デスクトップ版) に接続できません:' 'return ("OneNote (デスクトップ版) に接続できません:'
mutate "作り置き先を逃がさない" "書けないなら逃がす" \
    'if default and _can_write(default):
        return default, False' 'if True:
        return default, False'
# `os.access` に戻す壊し方は**わざと置いていない** ── POSIX の `os.access` は
# 正しく答えるので mac では鳴らない。Windows の上でしか壊れない性質で、
# 黙る壊し方を並べると「鳴らないのが普通」になる。
mutate "試し書きを片付けない" "試し書きの跡を残さない" \
    'os.unlink(probe)
        return True' 'return True'
mutate "逃がす前に client を読む" "逃がしてから win32com.client を読む（並び順）" \
    'gen_path, moved = _gen_py_somewhere_writable()
    if moved:
        log.debug("makepy の作り置き先を移した: %s", gen_path)

    import win32com.client  # noqa' \
    'import win32com.client  # noqa
    gen_path, moved = _gen_py_somewhere_writable()'
mutate "書く先だけ動かす（読む先は古いまま）" "書く先と読む先を揃えて逃がす" \
    'gen_py = sys.modules.get("win32com.gen_py") or getattr(win32com, "gen_py", None)
    if gen_py is not None:
        gen_py.__path__ = [at]' 'pass'
mutate "見つからなくてもやり直さない" "見つからなければ一度やり直す" \
    'importlib.invalidate_caches()
                mod = gencache.EnsureModule(ONENOTE_TYPELIB, 0, major, minor)' 'raise'
mutate "皮をかぶせない" "一段目が遅い束ねを掴んでも、包んで返す" \
    'return _wrap_with_generated(mod, raw) or raw' 'return raw'
mutate "名前で型を選ぶ" "選ぶのは名前ではなく「GetHierarchy を持つこと」" \
    'if not isinstance(cls, type) or not hasattr(cls, "GetHierarchy"):' \
    'if not isinstance(cls, type) or not name.startswith("I"):'
mutate "かぶせられないのに、かぶせたと言う" "かぶせた皮が実物として使えるところまで見る" \
    'if hasattr(wrapped, "GetHierarchy"):
                log.debug("makepy の皮をかぶせた: %s", name)
                return wrapped' 'return wrapped'
mutate "probe が途中で止まる" "一つ転んでも最後まで出る" \
    'try:
            print(f"  {label}: {fn()}")
        except Exception as e:  # noqa
            print(f"  {label}: ✗ {type(e).__name__}: {e}")' 'print(f"  {label}: {fn()}")'
mutate "probe が書き出しに進む" "何も書かない" \
    'if args.probe:
        return probe()' 'if False:
        return 0'
mutate "見えたら合格にする（呼ばない）" "呼んで落ちる相手は採らない" \
    'try:
            first = get_hierarchy(app)
        except Exception as e:  # noqa
            troubles.append(f"  {how}: 呼ぶと落ちる ── {e}")
            continue' 'first = None'
mutate "版は 1.1 しか試さない" "版が二つあるなら、もう一方を試す" \
    '("型ライブラリ 1.0 を名指し", by_typelib(1, 0)),' ''
mutate "繋ぐときの答えを捨てて、二度歩く" "階層を二度は取りに行かない" \
    'app, first = connect_onenote()
    root = ET.fromstring(first)' 'app, first = connect_onenote()
    root = ET.fromstring(get_hierarchy(app))'
mutate "bit の食い違いに黙る" "64 bit なのに win32 しか無ければ言う" \
    'if not arches or want in {a.lower() for a in arches}:
        return None' 'if True:
        return None'
mutate "大文字小文字で取り違える" "大文字小文字は問わない" \
    'want in {a.lower() for a in arches}' 'want in arches'
mutate "登録が空でも決めつける" "何も無ければ決めつけない" \
    'if not arches or want in' 'if want in'
mutate "資源の番号を落とさない" "exe の中の番号を落とす" \
    'return re.sub(r"[\\/]\d+$", "", path or "")' 'return path or ""'
mutate "途中の数字まで落とす" "途中の数字は落とさない" \
    'return re.sub(r"[\\/]\d+$", "", path or "")' 'return re.sub(r"\d+", "", path or "")'
mutate "繋げないとき bit の見立てを黙る" "繋げないとき bit の見立ても出す" \
    'そのまま貼ってもらえれば、推し量らずに直せます。""" + arch_hint(), troubles)' \
    'そのまま貼ってもらえれば、推し量らずに直せます。""", troubles)'
mutate "噛み合っていても言い立てる" "噛み合っているときは、余計なことを言わない" \
    'return ("\n" + "\n".join(v)) if v else ""' \
    'return "\n" + "\n".join(v or ["→ **win64 の登録が無い**"])'
mutate "schema を明示しない（GetHierarchy）" "schema を要る相手でも同じものが出る" \
    '("", HS_PAGES, XS_2013),
                     ("", HS_PAGES, "", XS_2013),' ''
mutate "schema を明示しない（GetPageContent）" "schema を要る相手でも同じものが出る" \
    '(page_id, info, XS_2013),
                     (page_id, "", info, XS_2013),' ''
mutate "見出しの行の旗を見ない" "見出しの行が無ければ、空の見出しを置く" \
    'if tbl.get("hasHeaderRow") == "true":' 'if True:'
mutate "見出しの行があっても空で置く" "見出しの行があれば、1行目が見出し" \
    'if tbl.get("hasHeaderRow") == "true":' 'if False:'
mutate "升の中の改行を <br> に戻す" "升の中の改行に、札を残さない" \
    'cell_text = " ".join(l.strip() for l in cell_lines if l.strip())' \
    'cell_text = "<br>".join(l.strip() for l in cell_lines if l.strip())'
mutate "升の中の改行を捨てる" "升の中の改行は、繋いで残す" \
    'cell_text = " ".join(l.strip() for l in cell_lines if l.strip())' \
    'cell_text = (cell_lines or [""])[0].strip()'
mutate "コードにも印を通す" "コードの枠に、印を生やさない" \
    'lines.append("```\n" + "".join(plain_md(x) for x in raw) + "\n```")' \
    'lines.append(f"```\n{text}\n```")'
mutate "plain_md が札を落とさない" "コードの枠に、印を生やさない" \
    's = _TAG.sub("", s)
    return html.unescape(s).replace("\xa0", " ")' \
    'return html.unescape(s).replace("\xa0", " ")'
mutate "href を空白で切る" "リンクの URL が空白で切れない" \
    'href=("[^"]*"|' 'href=("[^" ]*"|'
mutate "URL の括りを外さない" "リンクの URL が空白で切れない" \
    'f"({html.unescape(m.group(1).strip(chr(34) + chr(39)))})", s)' \
    'f"({m.group(1)})", s)'
mutate "インクの言い方を二つに戻す" "インクの言い方は、どこでも同じ" \
    'out.append("> [インク: 変換対象外]")' 'out.append("> [インク描画: 変換対象外]")'
mutate "UTC のまま頭を切る" "前書きの created も、この機械の日付" \
    'created = local_date(page_attr.get("dateTime"))' \
    'created = (page_attr.get("dateTime") or "")[:10]'
mutate "時差を足さない" "UTC を、この機械の日付に直す（東京）" \
    'return t.replace(tzinfo=timezone.utc).astimezone().strftime("%Y-%m-%d")' \
    'return t.strftime("%Y-%m-%d")'
mutate "読めない形で落ちる" "読めない形は、頭の 10 字" \
    'except ValueError:
        return stamp[:10]            # 読めない形は、そのまま頭を取る' \
    'except ValueError:
        raise'
mutate "落ちたわけを記録に落とす" "落ちたわけが、記録に残る" \
    'if isinstance(e.code, str):
            log.error("%s", e.code)
            return 1' \
    'if isinstance(e.code, str):
            return 1'
mutate "思わぬ落ち方を黙って捨てる" "落ちたわけが、記録に残る" \
    'except SystemExit as e:' 'except ZeroDivisionError as e:'
mutate "画面の字を cp932 のままにする" "cp932 に向けても、最後まで出る" \
    'stream.reconfigure(encoding="utf-8", errors="replace")' 'pass' 
mutate "読むだけの回も鎖を取る" "鎖の中でも --probe は走る" \
    'if args.probe or args.check or args.list or args.dry_run:
            return run(args, out_root)' \
    'if False:
            return run(args, out_root)'
mutate "--check が probe と同じだけ出す" "繋がらないときも、十数行で収まる" \
    '    try:
        app, first = connect_onenote()
    except SystemExit as e:' '    probe()
    try:
        app, first = connect_onenote()
    except SystemExit as e:'
mutate "--check が落ちても 0 を返す" "落ちた回は 0 を返さない" \
    '        for line in _next_move(me, office, tree, server_here, troubles):
            print(f"  {line}")
        return 1' '        for line in _next_move(me, office, tree, server_here, troubles):
            print(f"  {line}")
        return 0'
mutate "足す一行を出さず docs に送る" "足す一行を、道ごと出す" \
    'have = _typelib_path("win32" if want == "win64" else "win64")' 'have = None'
mutate "戻す一行を出さない" "戻す一行も出す" \
    '"戻すとき:", undo,' '' 
mutate "戻す一行に、消す命令が入らない" "戻す一行も出す" \
    'undo = f"  reg delete " + chr(34) + root + chr(34) + " /f"' \
    'undo = ""'
mutate "繋がっても次の一手を言う" "繋がったら、次の一手は言わない" \
    '    root = ET.fromstring(first)
    books =' \
    '    print("繋がらない。次の一手:")
    root = ET.fromstring(first)
    books ='
mutate "一冊も無いときに黙る" "一冊も無いとき、ストア版の落とし穴を言う" \
    'if not books:
        print("  → " + _no_books_hint(store))' 'if False:
        print("")'
mutate "ストア版が無くてもその話をする" "ストア版が無ければ、その話はしない" \
    'if store:
        return ("こちらから見えるのは' 'if True:
        return ("こちらから見えるのは'
mutate "読めなくてもストア版の話をする" "読めないときは、黙る" \
    'if store:
        # **入っていること自体は困らない。**' 'if store is not False:
        # **入っていること自体は困らない。**'
mutate "--check も鎖を取る" "鎖の中でも --check は走る" \
    'if args.probe or args.check or args.list or args.dry_run:' \
    'if args.probe or args.list or args.dry_run:'
mutate "枝を版ごとに見ず、束ねる" "読みにいく版で判じる" \
    'arches = tree.get(ver)
        if arches is None:
            continue' 'arches = set().union(*tree.values()) if tree else None
        if arches is None:
            continue'
# 構文を壊す壊し方は書かない ── 走査ごと止まると「NG が無い」と同じ顔になる（依頼 569）。
mutate "枝を版ごとに出さない（版の名前を落とす）" "枝は版ごとに出す" \
    '{v}=' ''
mutate "ほかの版に有れば安心する" "ほかの版に有っても、助けにならないと言う" \
    'elsewhere = [v for v, a in tree.items() if want in a]' 'elsewhere = []'
mutate "呼んだときの答えを捨てる" "呼んだときの答えを、そのまま出す" \
    'troubles = getattr(e, "troubles", [])' 'troubles = []'
mutate "答えを持ち帰らない" "呼んだときの答えを、そのまま出す" \
    'self.troubles = list(troubles)' 'self.troubles = []'
mutate "答えの番号を読まない" "答えの番号から、原因を名指しする" \
    'code, said = _from_answer(troubles)' 'code, said = None, []'
mutate "知らない答えでも決めつける" "知らない答えには、決めつけない" \
    'return None, []' 'return _ANSWERS[0][0], list(_ANSWERS[0][1])'
mutate "読み込みエラーを権限の話にする" "読めない実体は、32 bit の Python へ導く" \
    '("-2147312566", ["型ライブラリ／DLL の読み込みエラー（TYPE_E_CANTLOADLIBRARY）。",' \
    '("-2147312566x", ["型ライブラリ／DLL の読み込みエラー（TYPE_E_CANTLOADLIBRARY）。",'
mutate "読む版に枝が有っても言い立てる" "読む版に枝が有れば、枝の話はしない" \
    'if want in arches:
            break' 'if False:
            break'
mutate "読めない道を出さず probe へ送る" "読めない道を、その場で出す" \
    'if code == "-2147312566":' 'if False:'
mutate "同じ道でも黙る" "同じ道を指していたら、写しだと言う" \
    'if len(got) == 2 and got["win32"] == got["win64"]:' 'if False:'
mutate "違う道でも「同じ」と言う" "違う道を指していれば、写しだとは言わない" \
    'if len(got) == 2 and got["win32"] == got["win64"]:' 'if len(got) == 2:'
mutate "どの番号でも道を出す" "ほかの番号では、道を出さない" \
    'if code == "-2147312566":' 'if code:'
mutate "番号を持ち帰らない" "ほかの番号では、道を出さない" \
    'return needle, list(said)' 'return "-2147312566", list(said)'
mutate "持ち込む wheel の版を言わない" "この Python の版に合う wheel を名指しする" \
    'tag = f"cp{v[0]}{v[1]}"' 'tag = "cpXY"'
mutate "bit を取り違える" "32 bit の wheel は win32" \
    'arch = "win32" if me == 32 else "win_amd64"' 'arch = "win_amd64"'
mutate "網を見に行かせる" "網を見に行かせない" \
    '-m pip install --no-index --no-deps' '-m pip install'
mutate "32 bit でも -32 を言わない" "32 bit なら -32 の呼び方で言う" \
    'launcher = f"py -{v[0]}.{v[1]}" + ("-32" if arch == "win32" else "")' \
    'launcher = f"py -{v[0]}.{v[1]}"'
mutate "止めた言い分を飲み込む" "pywin32 が無いとき、持ち込むものを言う" \
    'elif isinstance(e.code, str):' 'elif False:'
mutate "両方あることを責める" "両方あることを、責めずに言う" \
    'print(f"ストア版も入っている（365 側に開いたものだけが写せる）: {store}")' \
    'print(f"ストア版も入っている: {store}")'
mutate "どちらで開くかを言わない" "一冊も無いとき、どちらで開くかを言う" \
    '"そちらで開いていても写せない ── 写したいものは 365 側でも開いておく。")' \
    '"そちらで開いていても写せない。")'
mutate "CRLF で書く" "改行は LF" \
    'with open(md_path, "w", encoding="utf-8", newline="\n") as f:' \
    'with open(md_path, "w", encoding="utf-8", newline="\r\n") as f:'

restore
echo
if [ "$silent" != "0" ]; then
    echo "★ $silent 件が黙っています ── その検査は何も守っていません。"
    exit 1
fi
echo "すべて鳴りました。"
