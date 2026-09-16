#!/usr/bin/env bash
# onenote-test.py の検査を、一つずつ壊して確かめる。
#
# `scripts/mutate.sh` と同じ理由でこれは script になっている ── 一度、手で
# 打ち直した版が黙って通り、「検査が効かない」のと見分けがつかなかった。
# 壊し方を書き留めておけば、次に検査を足した人も同じ手で確かめられる。
#
#   scripts/onenote-mutate.sh
set -uo pipefail
cd "$(dirname "$0")/.."

# **壊す先は二枚ある。** 本体と、`.one` を読む一枚（依頼 594）。
# どちらに当たるかは `mutate` が自分で探す ── 呼ぶ側に書かせると、
# 足した日にどちらかだけ壊し忘れる。
TARGETS=(scripts/onenote2md.py scripts/onestore.py scripts/onenote_ui.py)
keep=$(mktemp -d)
for t in "${TARGETS[@]}"; do cp "$t" "$keep/$(basename "$t")"; done
restore() { for t in "${TARGETS[@]}"; do cp "$keep/$(basename "$t")" "$t"; done; }
trap 'restore; rm -rf "$keep"' EXIT

restore
if ! python3 scripts/onenote-test.py >/dev/null 2>&1; then
    echo "先に走査が落ちています。壊す前に直してください。"
    exit 1
fi

silent=0

mutate() {  # 名前 / 気づいてほしい検査 / 何を / 何に
    local name="$1" want="$2" from="$3" to="$4"
    restore
    # **本当に変わったかは、中身の指紋で見る。** はじめ `grep -F "$to"` で
    # 見ていたが、`to` が複数行だと grep はそれを**行の並び**と読み、
    # どれか一行が元からあれば通る ── 一文字も置換されていないのに
    # 「壊した」と言い、続く「気づかなかった」が検査のせいに見えた。
    local was
    was=$(shasum "${TARGETS[@]}" | cut -d" " -f1 | tr -d "\n")
    for t in "${TARGETS[@]}"; do
        FROM="$from" TO="$to" perl -0pi -e 's/\Q$ENV{FROM}\E/$ENV{TO}/g' "$t"
    done
    if [ "$was" = "$(shasum "${TARGETS[@]}" | cut -d" " -f1 | tr -d "\n")" ]; then
        echo "★置換できず $name ── その文字列がそのままでは無い"
        silent=$((silent + 1))
        return
    fi
    local out
    out=$(python3 scripts/onenote-test.py 2>&1)
    if printf '%s\n' "$out" | grep -qF "NG   $want"; then
        echo "気づいた   $name"
    else
        echo "★見逃した $name  →  $want"
        printf '%s\n' "$out" | grep -F "NG " | head -5
        silent=$((silent + 1))
    fi
}

mutate "落ちても 0 を返す" "読めないファイルは、落ちずにエラーと数える" \
    'return 1 if stats["errors"] else 0' 'return 0'
# **`glob` の形を壊す手は置いていない** ── 守っているのは番号の形を見る
# 正規表現のほうで（すぐ下の「古い画像で番号の形を見ない」で気づく）、`glob` は
# 速さのためだけになった。振る舞いの変わらない壊し方を並べると、
# 「気づかないのが普通」になってしまう。
mutate "鎖を掛けない" "二本目は断られる" \
    'fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)' 'pass'
mutate "--skip を見ない" "--skip は外す" \
    'if args.skip and any(pat.lower() in low for pat in args.skip):
        return False' 'if False:
        return False'
mutate "--skip を見ない" "--skip は外す" \
    'if args.skip and any(pat.lower() in low for pat in args.skip):
        return False
    return True' 'return True'
mutate "大文字小文字を区別する" "大文字小文字は問わない" \
    'if args.only and not any(pat.lower() in low for pat in args.only):' \
    'if args.only and not any(pat in sec_path for pat in args.only):'
# `os.access` に戻す壊し方は**わざと置いていない** ── POSIX の `os.access` は
# 正しく答えるので mac では気づけない。Windows の上でしか壊れない性質で、
# 気づけない壊し方を並べると「気づかないのが普通」になってしまう。
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
# 構文を壊す壊し方は書かない ── 走査ごと止まると「NG が無い」と同じ顔になる（依頼 569）。
mutate "網を見に行く口を足す" "網を見に行かせない（onenote2md.py）" \
    'import struct' 'import struct, urllib.request'
# **ここは壊し方を置いていない**（`os.access` と同じ理由）── どちらも
# `winreg` の上でしか壊れず、mac には `winreg` が無いので気づけない。
#   * COM サーバーを pywintypes 経由で引く（pywin32 の無い Python で見失う）
#   * Office の bit を 32bit の見え方で読む（32 bit の処理から見えない枝）
# 気づけない壊し方を並べると「気づかないのが普通」になってしまう。
mutate "下まで歩かない" "下まで歩いて数える" \
    'found = [q for q in root.rglob("*")' 'found = [q for q in root.glob("*")'
mutate "形式を見ずに数える" "形式ごとに分ける" \
    'got = str(uuid.UUID(bytes_le=head[48:64])).lower()' 'got = "00000000-0000-0000-0000-000000000000"'
mutate "目次まで形式に数える" "形式を数えるのは .one だけ" \
    'for q in kinds[".one"]:' 'for q in found:'
mutate "数だけ見せて、意味を言わない" "混ざっていればそう言う" \
    'print(f"→ 公開仕様は {ok}/{total} 本。混ざっています。")' 'pass'
mutate "ぜんぶ公開仕様でも言わない" "ぜんぶ公開仕様なら、通ると言う" \
    'if total and ok == total:' 'if False:'
mutate "一本も無いのに通ると言う" "一本も無ければ、重いと言う" \
    'if total and ok == total:' 'if total:'
mutate "道の例を出さない" "探す道の例を出す" \
    'print(f"          例: {sample[key]}")' 'pass'
mutate "無いときに SharePoint の線を黙る" "一つも無ければ、SharePoint の線を言う" \
    'print("ノートブックが SharePoint にしか無いのかもしれません（手元は別の形）。")' 'pass'
mutate "無いのに 0 を返す" "無い回は 0 を返さない" \
    'print("ノートブックが SharePoint にしか無いのかもしれません（手元は別の形）。")
        return 1' 'return 0'
mutate "入れ物の中を見ない" "入れ物を開いて、中の形式まで数える" \
    'found += opened' 'found += []'
mutate "中の道を出さない" "中の道をそのまま出す" \
    'print(f"  {name}")' 'pass'
mutate "圧縮の種類を黙る" "圧縮の種類を言う" \
    'COMP_NAMES = {0: "無圧縮", 1: "MSZIP", 2: "Quantum", 3: "LZX"}' 'COMP_NAMES = {}'
mutate "幹を 60 字で切らない" "幹は 60 字で切る" \
    'if len(out) >= 60:' 'if len(out) >= 120:'
mutate "ページの幹を 120 字のままにする" "ページの名前も 60 字で切る" \
    'return name[:60] or fallback' 'return name[:120] or fallback'
mutate "使えない字を落として詰める" "使えない字はハイフンに" \
    'if gap and out:
            out.append("-")' 'if False:
            out.append("-")'
mutate "先頭にも - を置く" "先頭にハイフンを置かない" \
    'if gap and out:' 'if gap:'
mutate "予約名を見ない" "予約名は避ける" \
    'return f"_{got}" if head in _RESERVED else got' 'return got'
mutate "同じ題を上書きする" "同じ題は上書きせず、ずらす" \
    'if md_path.resolve() not in written:
                    break' 'break'
mutate "サブページを親の下に置かない" "サブページは親の下へ" \
    'parent = levels.get(level - 1, sec_dir) if level > 1 else sec_dir' 'parent = sec_dir'
mutate "ファイルの道でも絞りを見ない" "--only で外れたら書かない" \
    'if not chosen("/".join([nb] + gs + [sec]), args):' 'if False:'
mutate "読めないファイルで落ちる" "読めないファイルは、落ちずにエラーと数える" \
    'except Exception as e:  # noqa
            log.error("読めない %s: %s", at.name, e)' 'except ZeroDivisionError as e:
            log.error("読めない %s: %s", at.name, e)'
mutate "ファイルの道で CRLF にする" "改行は LF" \
    'with open(md_path, "w", encoding="utf-8", newline="\n") as f:
                f.write("\n".join(head)' \
    'with open(md_path, "w", encoding="utf-8", newline="\r\n") as f:
                f.write("\n".join(head)'
mutate "最後の改訂でなく最初を採る" "いまの版は、最後の改訂" \
    'return revs[max(revs)] if revs else []' 'return revs[min(revs)] if revs else []'
mutate "改訂が無いときに落ちる" "改訂が無ければ、空" \
    'return revs[max(revs)] if revs else []' 'return revs[max(revs)]'
mutate "隣の一枚を import で頼る" "よその場所から走らせても、隣の一枚を読める" \
    'at = Path(__file__).resolve().parent / f"{name}.py"' 'at = Path(f"{name}.py")'
mutate "無いときに黙って進む" "隣に居なければ、そう言う（追跡ではなく）" \
    'if not at.is_file():
        sys.exit(f"{at} がありません（`git pull` は済んでいますか）。")' 'pass'
mutate "見出しを段に合わせない" "深い見出しも段に合わせる" \
    'return "#" * min(int(style[1:]), 6)' 'return "#" * min(1, 6)'
mutate "見出しの段を止めない" "見出しは 6 段まで" \
    'min(int(style[1:]), 6)' 'int(style[1:])'
mutate "箇条書きの印を太字の中に入れる" "箇条書きの印は、太字の外" \
    'body = linked(line["text"].strip(), line)
    if line.get("bold"):
        body = f"**{body}**"' 'body = linked(line["text"].strip(), line)
    if line.get("bold"):
        body = body'
mutate "日付の行も本文に混ぜる" "0x1CB5 の行は本文に混ぜない" \
    'if p.get(P_IS_DATE) or p.get(P_IS_TIME) or p.get(P_IS_BOILER):
        return None' 'if False:
        return None'
mutate "題の行も本文に混ぜる" "0x1CB4 の行は本文に混ぜない" \
    'if p.get(P_IS_TITLE):
        return None' 'if False:
        return None'
mutate "空の行も残す" "空の行は落とす" \
    'if not text.strip():
        return None' 'if False:
        return None'
mutate "ページの上の順に並べない" "並びは上から下・左から右" \
    'lines.sort(key=lambda l: (l["y"], l["x"]))' 'pass'
# ── CAB を自分でほどく（依頼 617）──
mutate "CAB でなくても読みにいく" "CAB でなければ、わけを言う" \
    'if len(d) < 36 or d[:4] != b"MSCF":
        return None, "CAB ではない"' 'if False:
        return None, "CAB ではない"'
mutate "ファイルの数をフォルダの数と取り違える" "フォルダの数ではなくファイルの数を読む" \
    'n_folders, n_files, flags = struct.unpack("<HHH", d[26:32])' \
    'n_files, n_folders, flags = struct.unpack("<HHH", d[26:32])'
mutate "名前を cp932 から読む（現場で化けた顔）" "目録の名前を読む（旗なしの UTF-8（OneNote はこれ））" \
    'for enc in (("utf-8",) if attribs & _A_NAME_IS_UTF else ("utf-8", "cp932", "cp1252")):' \
    'for enc in (("utf-8",) if attribs & _A_NAME_IS_UTF else ("cp932", "cp1252", "utf-8")):'
mutate "入れ物の中のフォルダを捨てる" "セクショングループはフォルダのまま（無圧縮）" \
    'files.append({"name": cab_name(raw, attribs).replace(chr(92), "/"),' \
    'files.append({"name": cab_name(raw, attribs).split(chr(92))[-1],'
mutate "MSZIP で前の塊を辞書に使わない" "塊をまたいでも中身が合う（MSZIP）" \
    'z = zlib.decompressobj(-15, zdict=history)' 'z = zlib.decompressobj(-15)'
mutate "MSZIP の塊を繋がない" "塊をまたいでも中身が合う（MSZIP）" \
    'history = (history + out)[-32768:]' 'history = b""'
mutate "入れ物の言う道をそのまま信じる" "上の階へ出さない（.. は落とす）" \
    'if not x or x in (".", ".."):' 'if not x:'
mutate "セクショングループを畳む" "ノートブック・グループ・セクションに分かれる" \
    'out.append((pkg.stem, parts[:-1], Path(at), Path(parts[-1]).stem))' \
    'out.append((pkg.stem, [], Path(at), Path(parts[-1]).stem))'
mutate "目次まで写しにいく" ".one だけを拾う（目次は写さない）" \
    'if not name.lower().endswith(".one"):
            continue' 'if False:
            continue'
mutate "升の中の改行を捨てる（最後だけ残す）" "升の中の改行は、空白で繋ぐ" \
    't["rows"] = [[" ".join(c) for c in r] for r in t["rows"] if r]' \
    't["rows"] = [[(c[-1] if c else "") for c in r] for r in t["rows"] if r]'
mutate "1 行目を見出しに使う（データが一行消える）" "見出しの行は空で置く" \
    'out = ["|" + "  |" * width, "|" + " --- |" * width]' \
    'out = ["| " + " | ".join(rows.pop(0)) + " |", "|" + " --- |" * width]'
mutate "升の数を行ごとに揃えない" "升の数を、行ごとに揃える" \
    'for c in r] + [""] * (width - len(r)) for r in rows]' \
    'for c in r] for r in rows]'
mutate "表の中の字を本文にも出す" "表の中の字は、本文に二度出さない" \
    'skip.add(id(o))' \
    'pass'
mutate "Page の無い空間もページにする" "Page の無い空間は、ページにしない（空のノートを作らない）" \
    'if not any(o["jcid"] == JC_PAGE for o in whole):
            continue' 'if False:
            continue'
# ── 改訂を重ねる（依頼 617）──
mutate "最後の改訂だけで Page を探す" "最後の改訂に Page が無くても、取りこぼさない" \
    'if not any(o["jcid"] == JC_PAGE for o in whole):' \
    'if not any(o["jcid"] == JC_PAGE for o in current(revs)):'
mutate "古い版を新しい版より優先する" "同じ OID は新しいほうを採る" \
    'got[o["oid"]] = o' 'got.setdefault(o["oid"], o)'
mutate "改訂を新しい順に重ねる" "同じ OID は新しいほうを採る" \
    'for n in sorted(revs):' 'for n in sorted(revs, reverse=True):'
mutate "本文が空でも拾い直さない" "前の改訂の本文を拾う" \
    'if not _has_body(pg):' 'if False:'
mutate "題を本文の代わりに数える" "前の改訂の本文を拾う" \
    'return bool(pg["lines"] or pg["tables"] or pg["images"])' \
    'return bool(pg["lines"] or pg["tables"] or pg["images"] or pg["title"])'
mutate "前書きを古いほうから採る" "題はいちばん新しいものを採る" \
    'meta = None
    for o in objs:
        if o["jcid"] == JC_PAGEMETA:
            meta = o' \
    'meta = next((o for o in objs if o["jcid"] == JC_PAGEMETA), None)
    if False:
        pass'
mutate "同じ行を二度並べる" "同じ場所の同じ字は一つにまとめる" \
    'if key in seen:
            continue' 'if False:
            continue'
# ── 進み具合（依頼 617）──
mutate "何本目かを言わない" "いま何本目かを数で言う" \
    'log.info("[%d/%d] %s ── %d ページ", n, len(sections),' \
    'log.info("%d/%d %s ── %d ページ", n, len(sections),'
mutate "0 ページを黙って通す" "1 ページも取れなかったら、形式のわけを言う" \
    'log.warning("%s は 1 ページも取れなかった ── %s", sec, what)' 'pass'
mutate "指し先の並びを読み飛ばす" "参照は、頭の並びから指し先を受け取る" \
    'oids = [struct.unpack("<I", b[4 + k * 4:8 + k * 4])[0]
            for k in range(cnt) if 8 + k * 4 <= len(b)]' 'oids = []'
mutate "参照に指し先を配らない" "参照は、頭の並びから指し先を受け取る" \
    'out[pid] = ("ref", oids.pop(0) if oids else None)' 'out[pid] = ("ref", None)'
mutate "旗を本文からだけ読む" "太字の旗が立つ" \
    'f = {**p, **(style or {})}' 'f = p'
mutate "自動の色も色として出す" "自動（最後が 0xFF）は色を付けない" \
    'if v[3] != 0x00:
        return None' 'if False:
        return None'
mutate "色を青・緑・赤の順に読む" "赤・緑・青の順" \
    'return "#%02x%02x%02x" % (v[0], v[1], v[2])' 'return "#%02x%02x%02x" % (v[2], v[1], v[0])'
mutate "リンクを巻かない" "リンクは印の中" \
    'return f"[{body}]({url})" if url else body' 'return body'
mutate "色を巻かない" "色は印の外、字は印の中" \
    "return f'<span style=\"color:{c}\">{body}</span>' if c else body" 'return body'
mutate "見出しから色とリンクを落とす" "見出しにも色は付く" \
    'head = colored(linked(line["text"].strip(), line), line)' \
    'head = line["text"].strip()'
mutate "窓が出せないのに 0 を返す" "Tk が無ければ、代わりの打ち方を出して 1 を返す" \
    '        return 1' '        return 0'
mutate "窓と別の Python で走らせる" "走らせるのは、いま動いている Python" \
    'return [sys.executable, str(HERE / "onenote2md.py"), "--out", out, where]' \
    'return ["python", str(HERE / "onenote2md.py"), "--out", out, where]'
mutate "出力先を ambər の下に掘る" "amber の下には掘らない" \
    'return _docs() / "OneNote"' 'return _docs() / "amber" / "OneNote"'
mutate "CRLF で書く" "改行は LF" \
    'with open(md_path, "w", encoding="utf-8", newline="\n") as f:' \
    'with open(md_path, "w", encoding="utf-8", newline="\r\n") as f:'

restore
echo
if [ "$silent" != "0" ]; then
    echo "★ $silent 件、壊しても気づきません ── その検査は何も守っていません。"
    exit 1
fi
echo "壊すと、ぜんぶ気づきました。"
