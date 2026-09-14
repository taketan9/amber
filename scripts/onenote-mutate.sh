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
mutate "--no-images の守りを外す" "--no-images でも既にある絵は消さない" \
    'if not args.no_images and page_img_dir.is_dir():' 'if page_img_dir.is_dir():'
mutate "いま使っている絵まで消す" "いま使っている絵は残る" \
    'if old.name not in conv.images:' 'if True:'
mutate "隣のページの絵まで巻き込む" "名前が似ているだけの絵は巻き込まない" \
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
