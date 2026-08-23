"""いつものチェック — 冗長・死にロジック・名前の散らかりを機械で洗う。

    python3 scripts/audit.py            # 全部
    python3 scripts/audit.py dead       # 死にコードだけ（dup / naming も同様）

**目視でやらない。** 3万行を人が読み返すのは無理で、読み返したつもりに
なるのが一番危ない。毎回同じ物差しで測れるように道具にしてある。

**コンパイラが見ているものは見ない。** Rust の `dead_code` は、そのクレートの
中で誰にも呼ばれない非公開の項目を自分で見つけるし、cian は
`cargo clippy -D warnings` が緑の状態で保たれている。ここが見るのは
**コンパイラの目が届かないところ** ―― 誰も使っていない `pub`、握り潰した
`#[allow(dead_code)]`、そして「関数としては呼ばれているが、人間が辿り着けない」
メニュー項目やコマンド。実際に死んでいたのは毎回そこだった。

出るのは「候補」であって「誤り」ではない。判断は人（と AI）がする。
"""
from __future__ import annotations

import collections
import difflib
import glob
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

CRATES = ['cian-core', 'cian-tui', 'cian-lua', 'cian-pty', 'cian-scp', 'cian-ai',
          'cian-bin', 'cian-gui']
#: 本体のソース。tests.rs は読み手が違う（テストが唯一の利用者でも死んではいない）
SRC = sorted(p for c in CRATES
             for p in glob.glob(os.path.join(ROOT, 'crates', c, 'src', '**', '*.rs'),
                                recursive=True)
             if os.path.basename(p) != 'tests.rs')
TESTS = sorted(glob.glob(os.path.join(ROOT, 'crates', '*', 'src', 'tests.rs'))
               + glob.glob(os.path.join(ROOT, 'crates', '*', 'tests', '*.rs')))


def _read(p: str) -> str:
    with open(p, encoding='utf-8') as f:
        return f.read()


def _rel(p: str) -> str:
    return os.path.relpath(p, ROOT)


def _strip(src: str) -> str:
    """コメントと文字列を落とす。**名前を数えるのに散文を混ぜない。**

    文字リテラルを先に落とすこと。`'"'` の中の引用符が次の引用符と対になり、
    **その間のコードごと消える** ―― cian は `'"'` を実際に使っているので、
    これを忘れた版は「使われている」を大量に取りこぼした。
    """
    src = re.sub(r"'(?:[^'\\]|\\.)'", "'.'", src)
    src = re.sub(r'//[^\n]*', '', src)
    src = re.sub(r'/\*[\s\S]*?\*/', '', src)
    return re.sub(r'"(?:[^"\\\n]|\\.)*"', '""', src)


def _body(src: str, at: int) -> str:
    """`at` から始まる項目の本体を、波括弧の対応で取り出す。"""
    i = src.find('{', at)
    if i < 0:
        return ''
    depth, j = 0, i
    while j < len(src):
        if src[j] == '{':
            depth += 1
        elif src[j] == '}':
            depth -= 1
            if depth == 0:
                return src[i:j + 1]
        j += 1
    return src[i:]


FN = re.compile(r'^[ \t]*(?:pub(?:\([^)]*\))? )?(?:const |async |unsafe )*fn '
                r'([a-z_][a-z0-9_]*)', re.M)


def _functions(src: str):
    """(名前, 行, 本体) を返す。"""
    for m in FN.finditer(src):
        yield m.group(1), src[:m.start()].count('\n') + 1, _body(src, m.end())


# ── ① 死にコード ────────────────────────────────
def dead() -> int:
    print('=' * 72)
    print('① 死にコード（コンパイラに見えないもの）')
    print('=' * 72)
    found = 0
    all_src = {p: _read(p) for p in SRC}
    all_test = ''.join(_read(p) for p in TESTS)
    stripped = {p: _strip(s) for p, s in all_src.items()}
    everything = '\n'.join(stripped.values()) + '\n' + _strip(all_test)

    # (a) 誰も使っていない `pub`。dead_code は pub には黙っているので、
    #     ワークスペースの誰も呼んでいない公開 API はここでしか出てこない。
    print('  ■ ワークスペースの誰も使っていない pub')
    pub = re.compile(r'^[ \t]*pub (?:const |async |unsafe )*'
                     r'(fn|struct|enum|trait|const|type) ([A-Za-z_][A-Za-z0-9_]*)', re.M)
    for p, src in all_src.items():
        # バイナリクレートの pub は外から呼ばれない前提なので数えない
        if '/cian-bin/' in p or '/cian-gui/' in p:
            continue
        for m in pub.finditer(src):
            kind, name = m.group(1), m.group(2)
            if name.startswith('_') or name == 'main':
                continue
            # **ドットを除かない。** Rust の呼び出しは `p.target_paths()` で、
            # 除いた版は「使われている」を全部取りこぼした
            uses = len(re.findall(r'(?<![\w])' + re.escape(name) + r'(?![\w])',
                                  everything))
            if uses <= 1:
                line = src[:m.start()].count('\n') + 1
                print(f'    ★ {_rel(p)}:{line} pub {kind} {name}')
                found += 1

    # (b) 握り潰した dead_code。**1つ1つが「なぜ残すのか」の借金。**
    print('  ■ #[allow(dead_code)]（残す理由が要る）')
    for p, src in all_src.items():
        for m in re.finditer(r'#\[allow\([^)]*dead_code[^)]*\)\]', src):
            line = src[:m.start()].count('\n') + 1
            nxt = src[m.end():m.end() + 120].strip().splitlines()
            what = nxt[0][:60] if nxt else ''
            if any(k in what for k in DEAD_OK):
                continue
            print(f'    ★ {_rel(p)}:{line} → {what}')
            found += 1

    # (c) 人間が辿り着けないもの。**関数としては生きていても、押す道がなければ
    #     死んでいる。** リリース直前に、パレットが存在しないコマンドを5つ
    #     出していたのが実際にこれ。
    print('  ■ 辿り着けないメニュー項目・コマンド')
    lib = stripped.get(os.path.join(ROOT, 'crates', 'cian-tui', 'src', 'lib.rs'), '')
    menu = _body(lib, lib.find('enum MenuItem'))
    reachable = '\n'.join(v for k, v in stripped.items() if not k.endswith('lib.rs'))
    for m in re.finditer(r'^\s*([A-Z][A-Za-z0-9]*)[,({]', menu, re.M):
        name = m.group(1)
        if not re.search(r'MenuItem::' + re.escape(name) + r'\b', reachable):
            print(f'    ★ MenuItem::{name} — どのメニューにも入っていない')
            found += 1

    # **腕ごとに見る。別名は文書に載らなくてよい。** `:cp | :copy` の
    # `:copy` を「文書に無い」と言い出すと、指摘が全部ノイズになる ―― 実際
    # 名前単位で見た最初の版は67件を出し、そのほとんどが別名だった。
    # 一つも載っていない腕だけが、押す道の無いコマンド。
    pal = set(re.findall(r'^\s*\("([a-z][a-z0-9]*)",',
                         _read(os.path.join(ROOT, 'crates', 'cian-tui', 'src',
                                            'palette.rs')), re.M))
    manual = _read(os.path.join(ROOT, 'crates', 'cian-tui', 'src', 'lib.rs'))
    readme = _read(os.path.join(ROOT, 'README.md')) + _read(
        os.path.join(ROOT, 'README.ja.md'))
    for arm in _arms():
        if any(v in pal or f':{v}' in readme
               or re.search(r'[:`]' + re.escape(v) + r'\b', manual) for v in arm):
            continue
        print(f'    ★ :{" / :".join(sorted(arm))} — どの名前も文書に無い')
        found += 1

    print(f'  → {found} 件' if found else '  なし')
    return found


def _arms() -> list[set[str]]:
    """`:` コマンドを match の腕ごとに。同じ腕の名前は互いの別名。

    **一番外側の腕だけ。** `:editstyle vim|notepad` の引数も同じ形をしていて、
    数えると「`:notepad` が文書に無い」を二重に言い出す。深さは字下げで分かり、
    最小の字下げがコマンド本体の段。
    """
    out: list[set[str]] = []
    arm = re.compile(r'^([ \t]*)((?:"[a-z][a-z0-9.]*"(?: \| )?)+) =>', re.M)
    for f in ('commands.rs', 'viewer.rs'):
        src = _read(os.path.join(ROOT, 'crates', 'cian-tui', 'src', f))
        hits = [(len(m.group(1)), m.group(2)) for m in arm.finditer(src)]
        if not hits:
            continue
        top = min(i for i, _ in hits)
        out += [set(re.findall(r'"([a-z][a-z0-9.]*)"', names))
                for i, names in hits if i == top]
    return out


def _verbs() -> set[str]:
    """`:` コマンドの名前、全部。"""
    return {v for arm in _arms() for v in arm}


#: 似ているが**分けたままでよい**関数（理由つき）。
#
# ここに書くのは「重複を見逃す」ためではなく、**なぜ括らないか**を残すため。
# 括った方が高くつく組がある。
DUP_OK = {
    # 前後の鏡像。1本にすると「どちらのメソッドを呼ぶか」「矢印」「メッセージ」
    # で `if back` が3つ入る。10行の読める関数2本が、13行の分岐だらけ1本に
    # 化けるだけで、読む側は毎回どちらの方向の話かを追うことになる
    ('pane_go_back', 'pane_go_forward'),
    # 共有していた「返答から JSON 配列を切り出す」7行は `json_array` に出した。
    # 残る類似は、別々の構造体を別々の項目に組み立てている部分そのもので、
    # 括るには型を1つにするしかない ―― 片方は移動先を、もう片方は理由だけを
    # 持つので、1つにした型は常にどちらかの欄が空になる
    ('parse_junk_reply', 'parse_structure_reply'),
}


#: 握り潰したままでよい dead_code（理由つき）
DEAD_OK = {
    # **死んでいない。** Lua ランタイムを app の生存期間ぶん生かしておくためだけ
    # の保持で、読まないことが役目。落とすと ext_open のハンドルが道連れになる
    '_lua: Option<Lua>',
}


# ── ② 重複・冗長 ────────────────────────────────
def dup() -> int:
    print()
    print('=' * 72)
    print('② 重複（中身が似ている関数 / 同じ行の繰り返し）')
    print('=' * 72)
    found = 0
    for p in SRC:
        src = _read(p)
        # テストは互いに似ていて当然（同じ段取りを条件だけ変えて並べる）ので
        # 落とす。**列0のモジュールだけを切ること。** どこにでもある
        # `#[cfg(test)]` を探した版は、テスト専用ヘルパに付いた字下げ済みの
        # 1つを拾って soft.rs の3分の2を監査から消していた ―― 指摘が減るので
        # 「きれいになった」に見える
        m = re.search(r'^#\[cfg\(test\)\]', src, re.M)
        if m:
            src = src[:m.start()]
        funcs = {}
        for name, line, body in _functions(src):
            # テスト専用のヘルパは、本体の関数を真似て作るので似ていて当然
            head = src.splitlines()[max(0, line - 3):line - 1]
            if any('#[cfg(test)]' in h for h in head):
                continue
            norm = re.sub(r'//[^\n]*', '', body)
            norm = re.sub(r'\s+', ' ', norm).strip()
            if len(norm) > 300:
                funcs[name] = (norm, line, body.count('\n') + 1)
        names = sorted(funcs)
        for i, a in enumerate(names):
            for b in names[i + 1:]:
                # 入れ子の関数は本体を含むので似ていて当然
                if (funcs[a][1] <= funcs[b][1] <= funcs[a][1] + funcs[a][2]
                        or funcs[b][1] <= funcs[a][1] <= funcs[b][1] + funcs[b][2]):
                    continue
                s = difflib.SequenceMatcher(None, funcs[a][0], funcs[b][0])
                if s.quick_ratio() < 0.80:
                    continue
                r = s.ratio()
                if r >= 0.80 and (a, b) not in DUP_OK and (b, a) not in DUP_OK:
                    print(f'  ★ {r:.2f} {_rel(p)}: {a}:{funcs[a][1]} ↔ {b}:{funcs[b][1]}')
                    found += 1

    # 同じ行が何度も出てくる（コピペの跡）
    c = collections.Counter(
        l.strip() for p in SRC for l in _read(p).splitlines()
        if len(l.strip()) > 70 and not l.strip().startswith(('//', '///', '*')))
    for line, n in c.most_common(8):
        if n >= 4:
            print(f'  ★ 同じ行が {n} 回  {line[:78]}')
            found += 1
    print(f'  → {found} 件' if found else '  なし')
    return found


# ── ③ 名前の散らかり ──────────────────────────────
#
# **同じものを別の名前で呼んでいないか。** 呼び分けが意図的でないなら、
# 読む側は毎回「どちらだったか」を考えることになる。
TERM_FAMILIES = {
    'ペイン':     ['ペイン', '枠'],
    'フォルダ':   ['フォルダ', 'ディレクトリ'],
    'マーク':     ['マーク', '選択'],
    'ビューア':   ['ビューア', 'エディタ', 'パネル'],
    'シェル':     ['シェル', '端末', 'ターミナル'],
    '書庫':       ['書庫', 'アーカイブ'],
    '取り消し':   ['取り消し', '元に戻す', 'アンドゥ'],
    '一覧':       ['一覧', 'リスト'],
    '設定':       ['設定', 'コンフィグ'],
}

#: 使い分けているもの（意図があるので指摘しない）
TERM_OK = {
    # **粒度が違う。** 「枠」は画面上の四角（ビューアの枠、ポップアップの枠）で、
    # 「ペイン」はファイル一覧を持つ左右の単位。枠はペインの一部でもある
    'ペイン': {'ペイン', '枠'},
    # 「マーク」は Space で付ける印（複数・操作の対象）、「選択」はビューアの
    # 範囲選択とメニューの行選び。**揃えると対象が何か分からなくなる**
    'マーク': {'マーク', '選択'},
    # 3つとも別物。ビューア＝読む、エディタ＝書く（同じ窓の2つの状態）、
    # パネル＝ペインに嵌まっている状態そのもの
    'ビューア': {'ビューア', 'エディタ', 'パネル'},
    # 「シェル」は内蔵のシェル枠、「端末」は cian が乗っている外側の端末。
    # **混ぜると「どちらの話か」が消える** ―― IME やキーの説明で致命的になる
    'シェル': {'シェル', '端末', 'ターミナル'},
    # 画面に出る「リスト」は :renamelist の名前一覧。一覧表示のことではない
    '一覧': {'一覧', 'リスト'},
}

#: 単数で通す（2026-08 に決めた）。複数形が残っていないか
PLURAL_OK = {
    # init.lua が `view = "details"` と書き、エクスプローラもそう呼ぶ。
    # **ここだけは複数形が正名**
    'details', 'icons',
    # 本家 vim の実名
    'oldfiles', 'files',
    # 名詞そのもの
    'toggles', 'keys', 'vimkeys', 'colors', 'always',
    # 複数形ではない。unix のコマンド名と "save as"
    'ls', 'saveas', 'less', 'ps', 'gitstatus', 'status',
}


def _ui_text() -> str:
    """画面に出る文言だけを取り出す。

    **コメントと変数名は読み手が違う。** 混ぜて数えると「散らかっている」
    ように見えてしまい、指摘が信用されなくなる。
    """
    out = []
    for p in SRC:
        src = _read(p)
        src = re.sub(r'^\s*//[^\n]*', '', src, flags=re.M)
        out += [m.group(1) for m in re.finditer(r'"((?:[^"\\\n]|\\.)*)"', src)]
    return '\n'.join(x for x in out if re.search(r'[ぁ-んァ-ヶ一-龯]', x))


def naming() -> int:
    print()
    print('=' * 72)
    print('③ 名前・用語の散らかり')
    print('=' * 72)
    found = 0
    ui = _ui_text()

    print('  ■ 画面に出る用語（同じものを別の言葉で呼んでいないか）')
    for label, words in TERM_FAMILIES.items():
        hit = {w: len(re.findall(re.escape(w), ui)) for w in words}
        hit = {w: n for w, n in hit.items() if n}
        ok = TERM_OK.get(label, set())
        if len(hit) > 1 and set(hit) - ok:
            print(f'    ★ {label}: ' + ' / '.join(f'{w} {n}回' for w, n in hit.items()))
            found += 1

    print('  ■ コマンド名（基本単数。複数形は理由が要る）')
    for v in sorted(_verbs()):
        if v in PLURAL_OK or not v.endswith('s') or v.endswith('ss'):
            continue
        if v[:-1] in _verbs():
            print(f'    ★ :{v} と :{v[:-1]} が両方ある')
        else:
            print(f'    ★ :{v} — 複数形')
        found += 1

    # 「メニューにはあるがコマンドが無い」は測ろうとして**やめた**。
    # cian のメニュー項目と関数名に命名の対応が無いため（`MenuItem::Copy` は
    # `clip_targets()` を呼ぶ）、名前照合では作れない。ゆるくすると66件が
    # 全部素通りし、厳しくすると55件が誤検出になった。**素通りする検査は
    # 「所見なし」に見えるぶん、無い検査より悪い。** 非対称は人が見る。

    print(f'  → {found} 件' if found else '  揃っています')
    return found


def main() -> int:
    which = sys.argv[1] if len(sys.argv) > 1 else 'all'
    n = 0
    if which in ('all', 'dead'):
        n += dead()
    if which in ('all', 'dup'):
        n += dup()
    if which in ('all', 'naming'):
        n += naming()
    print()
    print('=' * 72)
    print(f'合計 {n} 件の候補' if n else 'きれいです')
    print('候補であって誤りではない。意図があって分けているものは')
    print('scripts/audit.py の TERM_OK / PLURAL_OK に理由つきで書く。')
    print('=' * 72)
    return 0


if __name__ == '__main__':
    sys.exit(main())
