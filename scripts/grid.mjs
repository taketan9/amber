#!/usr/bin/env node
/* 網 ── **位置×操作の総当たり**。人が押しうるものを、押しうる場所ぜんぶで押す。
 *
 *     scripts/walk.sh grid         # 場所を作り、デスクトップ版を出し、これを走らせ、片づける
 *     node scripts/grid.mjs        # 既に 9333 で出ているデスクトップ版に対して走らせる
 *
 *     QUICK=1 …                    # ふつうのノートの主な行だけ（十分の一）
 *     ONLY='Backspace|絵文字' …     # 名前で絞る（正規表現）
 *     GRID_OUT=…                   # 報せを書く場所（既定 $TMPDIR/amber-grid-out）
 *
 * **なぜ要るのか。** 「端に入るなら途中にも入るだろう」は成り立たなかった
 * （依頼 461・絵文字がノートの頭に入った）。`walk.sh` は一つの操作を一つの
 * 場所で押す ── ここは**同じ操作を、行頭・行中・行末 × 文書の頭・途中・
 * 末尾 × かたまりの種類ぜんぶ**で押す。
 *
 * 見ているのは、決めごとが要らないもの（誰が見ても落第）と、決めごとの
 * あるもの（`PAPER.ja.md` 六章）の二段:
 *
 *   一。例外が飛ばない・`console.error` が出ない
 *   二。触ったあと、まだテキストに戻せる（`paperToMd` が null でない）
 *   三。**二度目の書き戻しで文字が変わらない**（一度保存した形は安定している）
 *   四。**焦点が画面から飛ばない**（芯の 1 ──「焦点が飛ぶ」は事故）
 *   五。**入れたものは caret のところに入る**（文字・絵文字・貼った文字）
 *   六。**行のどこで押しても同じ結果**（行ごとの記号・Tab・セル）
 *   七。決めごとのある鍵（Tab・Backspace・Enter・矢印…）は、決めごとの通り
 *
 * 決めごとの**無い**ところは落第にしない ── 「見たまま」として報せに集め、
 * 本人に見せて訊く（**正しい挙動が分からなければ、勝手に決めない**）。
 *
 * **ウィンドウへ送る文字の中に、逆引用符と円記号を書かないこと**（`walk.mjs` の頭と
 * 同じ理由）。アプリの中で動く側は `grid-page.js` に分けてあり、あちらは素の
 * JS なので、その約束は当たらない。こちらから渡す文字は `JSON.stringify` で
 * 包んで `${}` に差す ── 差した文字は素通しなので、逃がした改行も無事。
 */
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const PORT = process.env.PORT || 9333;
const OUT = process.env.GRID_OUT || join(process.env.TMPDIR || '/tmp', 'amber-grid-out');
const ONLY = process.env.ONLY ? new RegExp(process.env.ONLY) : null;
const QUICK = !!process.env.QUICK;
/// 打ったあと、書き戻しが落ち着くまで（`readChanged` の 700ms + 保存）。
const WAIT = Number(process.env.WAIT || 950);
const q = JSON.stringify;

/* ── デスクトップ版と話す ── */

const tabs = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
const page = tabs.find((x) => x.type === 'page');
if (!page) {
    console.error(`デスクトップ版が見つかりません（${PORT} で出ていますか）`);
    process.exit(2);
}
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((go, no) => { ws.onopen = go; ws.onerror = no; });

let id = 0;
const waits = new Map();
let noise = [];
ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && waits.has(m.id)) { waits.get(m.id)(m); waits.delete(m.id); return; }
    if (m.method === 'Runtime.consoleAPICalled' && ['error', 'assert'].includes(m.params.type)) {
        noise.push('console: ' + m.params.args
            .map((a) => a.value ?? a.description ?? '?').join(' ').slice(0, 200));
    }
    if (m.method === 'Runtime.exceptionThrown') {
        const d = m.params.exceptionDetails;
        noise.push('例外: ' + String(d.exception?.description || d.text).split('\n')[0].slice(0, 200));
    }
};
/// デスクトップ版が消えたら、待っている問いはぜんぶ「デスクトップ版が消えました」で返す ──
/// 返さないと node は待ちぼうけのまま黙って終わり（exit 13）、報せが
/// 書かれない（実際に一時間ぶん消えた・2026-09-11）。
let gone = false;
ws.onclose = async (ev) => {
    gone = true;
    // **どちらが切ったかを残す** ── ウィンドウ（Electron）が死んだのか、繋ぎだけが
    // 切れたのか。生きていれば `/json` が答える。
    let alive = 'デスクトップ版は死んでいます（/json が答えません）';
    try {
        const list = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
        alive = 'デスクトップ版は生きています: ' + list.map((t) => t.type + ':' + (t.url || '').slice(0, 60)).join(' / ');
    } catch { /* 死んでいる */ }
    console.log(`  ！ ${new Date().toISOString()} 繋ぎが切れました（code ${ev && ev.code}）── ${alive}`);
    for (const [, go] of waits) go({ error: { message: 'デスクトップ版が消えました（Electron が落ちたか、閉じられた）' } });
    waits.clear();
};
const send = (method, params) => new Promise((go) => {
    if (gone) { go({ error: { message: 'デスクトップ版が消えました' } }); return; }
    const n = ++id;
    waits.set(n, go);
    try { ws.send(JSON.stringify({ id: n, method, params })); } catch (e) { waits.delete(n); go({ error: { message: e.message } }); }
});
await send('Runtime.enable');
const sleep = (ms) => new Promise((go) => setTimeout(go, ms));

const QUIET = ['Autofill.enable', 'Request Autofill.setAddresses', 'net::ERR_FILE_NOT_FOUND'];

/// デスクトップ版の中で一つ動かす（`walk.mjs` と同じ形）。
async function run(src) {
    const r = await Promise.race([
        send('Runtime.evaluate', {
            expression: `(async () => { ${src} })()`,
            awaitPromise: true, returnByValue: true, userGesture: true,
        }),
        sleep(Number(process.env.PATIENCE || 8000)).then(() => 'まった'),
    ]);
    if (r === 'まった') return { bad: '返ってきません（ダイアログが開いたまま待っている？）' };
    if (r.error) return { bad: 'CDP: ' + r.error.message };
    const bad = r.result?.exceptionDetails;
    if (bad) return { bad: String(bad.exception?.description || bad.text).split('\n')[0].slice(0, 300) };
    return { value: r.result?.result?.value };
}
/// `return` 付きで動かして、値だけ取る。落ちたら `{ bad }`。
async function call(src) {
    const r = await run('return ' + src);
    if (r.bad) return { bad: r.bad };
    return r.value === undefined ? {} : r.value;
}

// アプリの中で動く側を流し込む。
{
    const src = readFileSync(join(here, 'grid-page.js'), 'utf8');
    const r = await send('Runtime.evaluate', { expression: src, returnByValue: true });
    if (r.result?.exceptionDetails) {
        console.error('grid-page.js が流し込めません: ' + r.result.exceptionDetails.text);
        process.exit(2);
    }
}

/* ── キーボード（本物のキーとして送る） ──
 *
 * `KeyboardEvent` を作って投げるのでは、**既定の振る舞い**（段落の Enter・
 * 文字を消す Backspace・矢印）が起きない ── デスクトップ版の受け口が受けなかったパスは
 * 何も起きず、「受けなかった」と「既定が壊した」の区別がつかない。CDP の
 * `Input` から送れば、Chromium が本物のキーとして扱う。
 */
const KEYS = {
    Enter: { key: 'Enter', code: 'Enter', vk: 13, text: '\r' },
    Backspace: { key: 'Backspace', code: 'Backspace', vk: 8 },
    Delete: { key: 'Delete', code: 'Delete', vk: 46 },
    Tab: { key: 'Tab', code: 'Tab', vk: 9, text: '\t' },
    ArrowUp: { key: 'ArrowUp', code: 'ArrowUp', vk: 38 },
    ArrowDown: { key: 'ArrowDown', code: 'ArrowDown', vk: 40 },
    ArrowLeft: { key: 'ArrowLeft', code: 'ArrowLeft', vk: 37 },
    ArrowRight: { key: 'ArrowRight', code: 'ArrowRight', vk: 39 },
    KeyZ: { key: 'z', code: 'KeyZ', vk: 90 },
    KeyA: { key: 'a', code: 'KeyA', vk: 65 },
};
async function press(name, mods = {}) {
    const k = KEYS[name];
    const modifiers = (mods.alt ? 1 : 0) | (mods.ctrl ? 2 : 0) | (mods.meta ? 4 : 0) | (mods.shift ? 8 : 0);
    const base = { key: k.key, code: k.code, windowsVirtualKeyCode: k.vk, nativeVirtualKeyCode: k.vk, modifiers };
    const down = { ...base, type: 'rawKeyDown' };
    if (k.text && !mods.meta) { down.type = 'keyDown'; down.text = k.text; down.unmodifiedText = k.text; }
    let r = await send('Input.dispatchKeyEvent', down);
    if (r.error) throw new Error('キーが送れません: ' + r.error.message);
    r = await send('Input.dispatchKeyEvent', { ...base, type: 'keyUp' });
    if (r.error) throw new Error('キーが送れません: ' + r.error.message);
}
/// 文字を打つ（IME で確定したときと同じ道）。
async function type(text) {
    const r = await send('Input.insertText', { text });
    if (r.error) throw new Error('文字が打てません: ' + r.error.message);
}

/* ── 試すノート ──
 *
 * かたまりの種類ぜんぶを、一本に。**狙う文字は一つの節に一度だけ**出る
 * （`seek` は最初の一つを取る）。`kind` は判断の側が決めごとを引く鍵。
 */
const NORMAL = [
    '# 見出し',
    '',
    '書式のない長い段落です。まん中に置けるくらいには長い。',
    '',
    '- ひとつ',
    '- ふたつ',
    '  - 入れ子',
    '- みっつ',
    '',
    '1. 一番',
    '2. 二番',
    '',
    '- [ ] やること',
    '- [x] やった',
    '',
    '> 引用の一行目',
    '> 引用の二行目',
    '',
    '> [!NOTE]',
    '> 注記の本文',
    '',
    '| 朝 | 夕 |',
    '| --- | --- |',
    '| 掃除 | 片づけ |',
    '| 洗濯 | 買い出し |',
    '',
    '上の段落。',
    '',
    '```rust',
    'fn main() {}',
    '```',
    '',
    '下の段落。',
    '',
    '　インデントた段落。',
    '',
    '最後の段落。',
].join('\n');

/// [名前, 狙う文字, 種類]。
const NORMAL_TARGETS = [
    ['見出し（頭）', '見出し', 'h'],
    ['段落', '書式のない長い段落です。まん中に置けるくらいには長い。', 'p'],
    ['一覧の一つめ', 'ひとつ', 'li1'],
    ['一覧の途中', 'ふたつ', 'li'],
    ['入れ子', '入れ子', 'nest'],
    ['一覧の最後', 'みっつ', 'liN'],
    ['番号', '二番', 'ol'],
    ['セル', 'やること', 'task'],
    ['済んだセル', 'やった', 'done'],
    ['引用の一行目', '引用の一行目', 'q1'],
    ['引用の二行目', '引用の二行目', 'q2'],
    ['注記', '注記の本文', 'alert'],
    ['表のセル', '掃除', 'cell'],
    ['表の最後のセル', '買い出し', 'cellN'],
    ['枠の上の段落', '上の段落。', 'before'],
    ['枠の下の段落', '下の段落。', 'after'],
    ['インデント', '　インデントた段落。', 'pad'],
    ['末尾の段落', '最後の段落。', 'last'],
];
const QUICK_TARGETS = ['段落', '一覧の途中', 'セル', '表のセル', '枠の下の段落', '末尾の段落'];

const FIXTURES = [
    { name: 'ふつう', note: '網.md', md: NORMAL,
      targets: QUICK ? NORMAL_TARGETS.filter((t) => QUICK_TARGETS.includes(t[0])) : NORMAL_TARGETS },
    { name: '一行だけ', note: '網.md', md: 'たった一行のノート。',
      targets: [['その一行', 'たった一行のノート。', 'one']] },
    { name: '見出しだけ', note: '網.md', md: '# 題だけ',
      targets: [['その見出し', '題だけ', 'h1only']] },
    { name: '枠にはさまれた', note: '網.md',
      md: '```\nコード\n```\n\n真ん中の段落。\n\n```\nコード\n```',
      targets: [['真ん中の段落', '真ん中の段落。', 'between']] },
    { name: '空', note: '網.md', md: '',
      targets: [['空の画面', '', 'empty']] },
    { name: '前書きなし', note: '網なし.md', md: '一行目の段落。\n\n二行目の段落。',
      targets: [['頭の段落', '一行目の段落。', 'p'], ['末尾の段落', '二行目の段落。', 'last']] },
];
if (QUICK) FIXTURES.splice(1);

const WHERES = ['head', 'mid', 'tail'];
const WHERE_JA = { head: '行頭', mid: '行中', tail: '行末' };

/* ── 操作 ── */

const mark = (name) => () => run('const r = __g.mark(' + q(name) + '); if (r && r.then) await r; return true;');
/// `focus: false` は、焦点が画面に残らなくても落第にしないもの。
const OPS = [
    ['Enter', () => press('Enter')],
    ['⇧Enter', () => press('Enter', { shift: true })],
    ['Backspace', () => press('Backspace')],
    ['Delete', () => press('Delete')],
    ['Tab', () => press('Tab')],
    ['⇧Tab', () => press('Tab', { shift: true })],
    ['↑', () => press('ArrowUp')],
    ['↓', () => press('ArrowDown')],
    ['←', () => press('ArrowLeft')],
    ['→', () => press('ArrowRight')],
    ['あ', () => type('あ')],
    ['あ→⌘Z', async () => { await type('あ'); await sleep(WAIT); await press('KeyZ', { meta: true }); }, { focus: false }],
    ['太字→あ', async () => { await mark('太字')(); await sleep(80); await type('あ'); }],
    ['斜体→あ', async () => { await mark('斜体')(); await sleep(80); await type('あ'); }],
    ['見出し', mark('見出し')],
    ['箇条書き', mark('箇条書き')],
    ['チェックリスト', mark('チェックリスト')],
    ['番号リスト', mark('番号リスト')],
    ['太字', mark('太字')],
    ['斜体', mark('斜体')],
    ['取り消し線', mark('取り消し線')],
    ['リンク', mark('リンク')],
    ['表', mark('表')],
    ['水平線', mark('水平線')],
    ['引用', mark('引用')],
    ['注記', () => run('await __g.markAnswering(' + q('注記') + '); return true;')],
    ['絵文字', () => run('await __g.face(' + q('😀') + '); return true;')],
    ['文字を貼る', () => run('__g.paste(' + q('貼った文字') + '); return true;')],
    ['HTMLを貼る', () => run('__g.paste(' + q('貼った見出し') + ', '
        + q('<h3>貼った見出し</h3><ul><li>貼った項目</li></ul>') + '); return true;')],
    ['セルを押す', () => run('const r = __g.tick(); if (r && r.bad) throw new Error(r.bad); return true;'),
        { kinds: ['task', 'done'], focus: false }],
];
/// **行のどこで押しても同じ結果**であるべきもの。
const SAME_ANYWHERE = new Set(['見出し', '箇条書き', 'チェックリスト', '番号リスト', '引用',
    'リンク', '表', '水平線', '注記', 'Tab', '⇧Tab', 'セルを押す', '太字', '斜体', '取り消し線']);

/* ── 選んでから ── */

const SELS = [
    ['段落の一部', '長い段落', null],
    ['行をまたぐ', 'ひとつ', 'ふたつ'],
    ['かたまりをまたぐ', '書式のない', 'ひとつ'],
    ['見出しを含む', '見出し', '書式のない'],
    ['セルをまたぐ', '掃除', '買い出し'],
    ['表の外から中へ', '注記の本文', '掃除'],
    ['枠を含む', '上の段落', '下の段落'],
    ['ぜんぶ', '*', null],
];
const SEL_OPS = ['太字', '斜体', '取り消し線', '見出し', '箇条書き', 'チェックリスト', '引用',
    'Backspace', 'Delete', 'あ', 'Enter', '文字を貼る', '絵文字'];

/* ── コード画面 ── */

const CODE_OPS = ['見出し', '箇条書き', 'チェックリスト', '番号リスト', '太字', '斜体', '取り消し線',
    'リンク', '表', '水平線', '引用'];
const CODE_WRAP = new Set(['太字', '斜体', '取り消し線']);
const CODE_PUT = { 'リンク': '[](https://)', '表': '| 見出し | 見出し |', '水平線': '---' };

/* ── 判断 ── */

const same = (a, b) => String(a).replace(/\n+$/, '') === String(b).replace(/\n+$/, '');
const show = (s) => (s === null || s === undefined ? String(s) : JSON.stringify(String(s)).replace(/\\n/g, '⏎'));
const mdLine = (md, text) => String(md).split('\n').find((l) => l.includes(text)) ?? null;
const indentOf = (l) => (l === null ? -1 : l.length - l.trimStart().length);
const tableShape = (md) => String(md).split('\n').filter((l) => l.trim().startsWith('|'))
    .map((l) => l.split('|').length - 2).join('/');

/// 二つの文字の、変わったところだけ（前後で同じ行を落とす）。
function diff(a, b) {
    // 末尾の改行の数は見ない（`sameNote` と同じ考え）── 見ると、差分が
    // 末尾までぜんぶ膨らむ。
    const x = String(a).replace(/\n+$/, '').split('\n');
    const y = String(b).replace(/\n+$/, '').split('\n');
    let i = 0;
    while (i < x.length && i < y.length && x[i] === y[i]) i += 1;
    let j = 0;
    while (j < x.length - i && j < y.length - i && x[x.length - 1 - j] === y[y.length - 1 - j]) j += 1;
    const from = x.slice(i, x.length - j);
    const to = y.slice(i, y.length - j);
    return { at: i, from, to, text: `${i + 1} 行目: ${show(from.join('\n'))} → ${show(to.join('\n'))}` };
}

/// 決めごとの通りか。`true` は通った、文字は落第の理由、`undefined` は決めごとが無い（見たまま）。
function expect(c) {
    const { op, kind, where, before, st } = c;
    const md = st.md;
    const unchanged = same(md, before.md);
    const bt = before.lineText ?? '';
    const lt = st.lineText;
    const pos = before.pos;
    const text = c.text;
    const PARA = ['p', 'before', 'after', 'pad', 'last', 'one', 'between', 'q1', 'q2', 'alert'];
    const LIST = ['li1', 'li', 'nest', 'liN', 'ol', 'task', 'done'];
    const CELL = ['cell', 'cellN'];
    const ins = (s) => {
        if (lt === null) return `入れたあと、狙った行が画面から消えました（${show(bt)}）`;
        const want = bt.slice(0, pos) + s + bt.slice(pos);
        if (lt === want) return true;
        const at = lt.indexOf(s);
        if (at < 0) return `「${s}」が入っていません: ${show(bt)} → ${show(lt)}`;
        return `「${s}」が caret のところに入っていません（${pos} 文字目のはずが ${at} 文字目）: ${show(lt)}`;
    };
    const stay = (why) => (unchanged ? true : `${why}のに文字が変わりました: ${diff(before.md, md).text}`);
    const line = (want) => {
        // インデントの行は、`　` を外した文字で探す（項目になるとインデントは消える）。
        const got = mdLine(md, text.replace(/^\u3000+/, ''));
        return got === want ? true : `その行が ${show(want)} になるはずが ${show(got)} です`;
    };
    /// 一覧の行を触ったとき、**ほかの項目の文字が一つも消えていない**こと。
    const keepList = (r) => {
        if (r !== true) return r;
        for (const t of ['ひとつ', 'ふたつ', '入れ子', 'みっつ', '一番', '二番', 'やること', 'やった']) {
            if (before.md.includes(t) && !md.includes(t)) return `「${t}」が消えました: ${diff(before.md, md).text}`;
        }
        return true;
    };
    const dent = (delta) => {
        const was = indentOf(mdLine(before.md, text));
        const now = indentOf(mdLine(md, text));
        if (now === -1) return 'その行が文字から消えました';
        return now === was + delta ? true : `インデントが ${was} → ${now}（${was + delta} のはず）`;
    };

    switch (op) {
        case 'あ': return ins('あ');
        case '絵文字': return ins('😀');
        case '文字を貼る': return ins('貼った文字');
        case '太字→あ': case '斜体→あ': {
            const r = ins('あ');
            if (r !== true) return r;
            if (kind === 'h' || kind === 'h1only') return true;   // 見出しは飾らない（丙）
            const m = op.startsWith('太') ? '**あ**' : '*あ*';
            return md.includes(m) ? true : `文字は入りましたが書式が付いていません（${m} が無い）`;
        }
        case 'あ→⌘Z': return unchanged ? true : '一つ戻しても元の文字に戻りません: ' + diff(before.md, md).text;
        case '↑': case '↓': case '←': case '→': {
            if (!unchanged) return '矢印で文字が変わりました: ' + diff(before.md, md).text;
            const back = op === '↑' || op === '←';
            if (kind === 'after' && back && where === 'head') {
                return st.block < before.block ? true : '枠の下の行頭で押しても、枠を跨ぎません';
            }
            if (kind === 'before' && !back && where === 'tail') {
                return st.block > before.block ? true : '枠の上の行末で押しても、枠を跨ぎません';
            }
            if (kind === 'between' && back && where === 'head') {
                return st.block < before.block ? true : '枠で始まるノートで、上に降りられません';
            }
            if (kind === 'between' && !back && where === 'tail') {
                return st.block > before.block ? true : '枠で終わるノートで、下に降りられません';
            }
            return true;
        }
        case 'Tab': {
            if (kind === 'h' || kind === 'h1only') return stay('見出しで Tab を押した');
            if (kind === 'cell') return stay('セルで Tab を押した（次のセルへ）');
            if (kind === 'cellN') return md.includes('| 　 | 　 |') ? true : '最後のセルで Tab を押しても、行が増えません';
            if (kind === 'li1' || kind === 'nest' || kind === 'task') return stay('上に項目が無い');
            if (LIST.includes(kind)) return dent(2);
            if (kind === 'empty') return lt === '　' ? true : `空の画面で Tab を押したら ${show(lt)}`;
            if (lt === null) return '狙った行が画面から消えました';
            return lt === '　' + bt ? true : `インデントが付いていません: ${show(bt)} → ${show(lt)}`;
        }
        case '⇧Tab': {
            if (kind === 'nest') return dent(-2);
            if (kind === 'pad') return lt === bt.slice(1) ? true : `インデントが外れていません: ${show(lt)}`;
            return stay('外すインデントが無い');
        }
        case 'Backspace': {
            if (where !== 'head') {
                if (pos === 0) return stay('消す文字が無い');
                const want = bt.slice(0, pos - 1) + bt.slice(pos);
                return lt === want ? true : `一文字消えるはずが: ${show(bt)} → ${show(lt)}`;
            }
            switch (kind) {
                case 'h': case 'h1only': return line(text);
                case 'nest': return dent(-2);
                case 'li1': case 'li': case 'liN': case 'ol': case 'task': case 'done': {
                    const r = line(text);
                    if (r !== true) return r;
                    for (const t of ['ひとつ', 'ふたつ', '入れ子', 'みっつ', '一番', '二番', 'やること', 'やった']) {
                        if (before.md.includes(t) && !md.includes(t)) return `記号を外したら「${t}」が消えました`;
                    }
                    return true;
                }
                case 'q1': return md.includes('引用の一行目\n\n> 引用の二行目') ? true
                    : `一行目だけ引用から出るはずが: ${show(mdLine(md, '引用') )}`;
                case 'q2': return md.includes('引用の一行目引用の二行目') ? true
                    : `途中の行は前と繋がるはずが: ${diff(before.md, md).text}`;
                case 'alert': return line(text) === true && !md.includes('[!NOTE]') ? true
                    : `注記から出るはずが: ${diff(before.md, md).text}`;
                case 'after': return stay('枠のすぐ下の行頭で Backspace を押した');
                case 'one': case 'empty': case 'h': return stay('消すものが無い');
                case 'cell': case 'cellN':
                    return tableShape(md) === tableShape(before.md) ? undefined
                        : `表の形が変わりました（${tableShape(before.md)} → ${tableShape(md)}）`;
                case 'p': case 'last': case 'pad': {
                    // 前のかたまりの末尾の行と繋がる（既定）── 前が無ければ何も起きない。
                    const prev = before.blocks.slice(0, before.block).filter((b) => b).pop();
                    if (prev === undefined) return stay('前の行が無い');
                    const tail = String(prev).split('\n').pop();
                    return md.includes(tail + bt) ? true : `前の行と繋がるはずが: ${diff(before.md, md).text}`;
                }
                default: return undefined;
            }
        }
        case 'Delete': {
            if (where === 'tail') {
                // 次が記号付きの行なら、何も起きない（本人が決めた・2026-09-11）。
                if (['before', 'last', 'one', 'empty', 'between', 'h1only'].includes(kind)) return stay('行末で Delete を押した');
                if (LIST.includes(kind) || CELL.includes(kind)) return stay('項目・セルの終わりで Delete を押した');
                if (['p', 'q2', 'alert', 'after', 'pad'].includes(kind)) {
                    // 次のかたまりが素の段落なら繋がる。そうでなければ何も起きない。
                    const nextBlock = before.blocks.slice(before.block + 1).find((b) => b !== null && b !== '');
                    const plain = nextBlock !== undefined && !/^(#|-|\d+\.|>|\||`|!\[|\s{2,})/.test(nextBlock);
                    if (!plain) return stay('次が記号付きの行なので、何も起きない');
                    return md.includes(bt + nextBlock.split('\n')[0]) ? true : `次の段落と繋がるはずが: ${diff(before.md, md).text}`;
                }
                if (kind === 'h') return md.includes('# 見出し書式のない') ? true : `次の段落と繋がるはずが: ${diff(before.md, md).text}`;
                if (kind === 'q1') return md.includes('引用の一行目引用の二行目') ? true : `同じ箱の次の行と繋がるはずが: ${diff(before.md, md).text}`;
                return undefined;
            }
            const want = bt.slice(0, pos) + bt.slice(pos + 1);
            return lt === want ? true : `一文字消えるはずが: ${show(bt)} → ${show(lt)}`;
        }
        case 'Enter': {
            if (kind === 'h' || kind === 'h1only') {
                if (where !== 'mid') return stay('見出しの端で Enter を押した');
                const a = bt.slice(0, pos);
                const b = bt.slice(pos);
                return md.includes('# ' + a + '\n\n' + b) ? true
                    : `前は見出し・後ろは段落になるはずが: ${diff(before.md, md).text}`;
            }
            if (kind === 'cell') return stay('セルで Enter を押した（下のセルへ）');
            if (kind === 'cellN') return md.includes('| 　 | 　 |') ? true : '最後の行のセルで Enter を押しても、行が増えません';
            if (['p', 'last', 'one', 'between', 'before', 'after'].includes(kind)) {
                if (where !== 'mid') return stay('段落の端で Enter を押した');
                return md.includes(bt.slice(0, pos) + '\n\n' + bt.slice(pos)) ? true
                    : `段落が二つに割れるはずが: ${diff(before.md, md).text}`;
            }
            if (LIST.includes(kind)) {
                // 端では空の項目が画面に出るだけで、文字は変わらない（本人が決めた・2026-09-11）。
                // 入れ子を持つ項目（ふたつ）の行末は、既定が入れ子を新しい項目へ移す ── 見たまま。
                if (kind === 'li' && where === 'tail') return undefined;
                if (where !== 'mid') return stay('項目の端で Enter を押した');
                return undefined;
            }
            if (kind === 'q1' || kind === 'q2' || kind === 'alert') {
                // 引用・注記の途中は改行（本人が決めた・2026-09-11）。狙う文字の中で割る。
                const k = pos - bt.indexOf(text);
                if (where === 'mid') {
                    return md.includes('> ' + text.slice(0, k) + '\n> ' + text.slice(k)) ? true
                        : `改行になるはずが: ${diff(before.md, md).text}`;
                }
                // 行の端 ── 行と行のあいだなら空の `>` の行が一つ入る。箱の端なら変わらない。
                if ((kind === 'q1' && where === 'tail') || (kind === 'q2' && where === 'head')) {
                    return md.includes('> 引用の一行目\n>\n> 引用の二行目') ? true
                        : `行のあいだに空の行が入るはずが: ${diff(before.md, md).text}`;
                }
                return stay('箱の端で Enter を押した');
            }
            if (kind === 'empty') return stay('空の画面で Enter を押した');
            return undefined;
        }
        case '⇧Enter': {
            if (CELL.includes(kind)) return stay('セルで ⇧Enter を押した');
            if (kind === 'h' || kind === 'h1only') {
                if (where !== 'mid') return stay('見出しの端で ⇧Enter を押した');
                return md.includes('# ' + bt.slice(0, pos) + '\n\n' + bt.slice(pos)) ? true
                    : `見出しでは Enter と同じはずが: ${diff(before.md, md).text}`;
            }
            if (['p', 'last', 'one', 'between', 'before', 'after'].includes(kind)) {
                if (where !== 'mid') return stay('段落の端で ⇧Enter を押した');
                return md.includes(bt.slice(0, pos) + '\n' + bt.slice(pos)) ? true
                    : `段落の中の改行になるはずが: ${diff(before.md, md).text}`;
            }
            return undefined;
        }
        case '見出し': {
            if (kind === 'empty') return stay('空の画面では、文字を打つまで書かない');
            if (kind === 'pad') return line('# ' + bt.slice(1));
            if (['p', 'before', 'after', 'last', 'one', 'between'].includes(kind)) return line('# ' + bt);
            if (kind === 'h' || kind === 'h1only') return line('## ' + text);
            return undefined;
        }
        case '箇条書き': {
            if (kind === 'empty') return stay('空の画面では、文字を打つまで書かない');
            if (kind === 'pad') return line('- ' + bt.slice(1));
            if (['p', 'before', 'after', 'last', 'one', 'between'].includes(kind)) return line('- ' + bt);
            if (kind === 'h') return line('- ' + text);
            if (['li1', 'li', 'liN', 'task', 'done'].includes(kind)) return keepList(line(text));
            if (kind === 'nest') return keepList(line(text));
            if (kind === 'ol') return keepList(line('- ' + text));       // 種類を替える（本人が決めた）
            if (CELL.includes(kind)) return stay('表のセルでは何もしない');
            return undefined;
        }
        case '番号リスト': {
            if (kind === 'empty') return stay('空の画面では、文字を打つまで書かない');
            if (kind === 'pad') return line('1. ' + bt.slice(1));
            if (['p', 'before', 'after', 'last', 'one', 'between'].includes(kind)) return line('1. ' + bt);
            if (kind === 'h') return line('1. ' + text);
            if (kind === 'ol') return keepList(line(text));
            if (['li1', 'li', 'liN'].includes(kind)) return keepList(line('1. ' + text));   // 種類を替える
            if (kind === 'nest') return keepList(line('  1. ' + text));
            if (kind === 'task') return keepList(line('1. [ ] ' + text));
            if (kind === 'done') return keepList(line('1. [x] ' + text));
            if (CELL.includes(kind)) return stay('表のセルでは何もしない');
            return undefined;
        }
        case 'チェックリスト': {
            // **caret の一行だけ**（本人が決めた・2026-09-10）。
            if (kind === 'empty') return stay('空の画面では、文字を打つまで書かない');
            if (['p', 'before', 'after', 'last', 'one', 'between'].includes(kind)) return line('- [ ] ' + bt);
            if (kind === 'pad') return line('- [ ] ' + bt.slice(1));
            if (kind === 'h' || kind === 'h1only') return line('- [ ] ' + text);
            if (kind === 'li1' || kind === 'li' || kind === 'liN') return line('- [ ] ' + text);
            if (kind === 'nest') return line('  - [ ] ' + text);
            if (kind === 'ol') return line('2. [ ] ' + text);
            if (kind === 'task' || kind === 'done') return line('- ' + text);
            if (kind === 'q1') return md.includes('- [ ] 引用の一行目') ? true : `引用から出てセルになるはずが: ${diff(before.md, md).text}`;
            if (kind === 'alert') return line('- [ ] ' + text) === true && !md.includes('[!NOTE]') ? true : `注記から出てセルになるはずが: ${diff(before.md, md).text}`;
            if (CELL.includes(kind)) return stay('表のセルでは何もしない');
            return undefined;
        }
        case '引用': {
            if (['p', 'before', 'after', 'last', 'one', 'between', 'pad'].includes(kind)) return line('> ' + bt);
            if (LIST.includes(kind)) return keepList(line('> ' + text));   // 段落にしてから引用（本人が決めた）
            if (kind === 'q1' || kind === 'q2') {
                return mdLine(md, '引用の一行目') === '引用の一行目' ? true
                    : `引用の中で押したら外れるはずが: ${diff(before.md, md).text}`;
            }
            return undefined;
        }
        case '太字': case '斜体': case '取り消し線': return stay('選ばずに書式を押した');
        case 'リンク': case '表': case '水平線': case '注記': {
            const put = { 'リンク': '[リンクの文字](https://)', '表': '| 見出し | 見出し |', '水平線': '---', '注記': '> [!NOTE]' }[op];
            if (kind === 'empty') return md.includes(put) ? true : `${op}が入っていません`;
            const block = before.blocks[before.block];
            if (block === undefined || block === null) return undefined;
            return md.includes(block + '\n\n' + put) ? true
                : `そのかたまりの次に ${show(put)} が入るはずが: ${diff(before.md, md).text}`;
        }
        case 'HTMLを貼る': {
            // セルには文字だけ・項目には文字だけ（改行ごとに項目）── 本人が決めた・2026-09-10。
            if (CELL.includes(kind)) {
                if (!md.includes('貼った見出し') || md.includes('### ') || !md.includes('貼った見出し')) return `セルには文字だけのはずが: ${diff(before.md, md).text}`;
                return tableShape(md) === tableShape(before.md) ? true : `表の形が変わりました（${tableShape(before.md)} → ${tableShape(md)}）`;
            }
            if (LIST.includes(kind)) {
                return md.includes('貼った見出し') && !md.includes('### ') && !md.includes('貼った項目') ? true
                    : `項目には文字だけのはずが: ${diff(before.md, md).text}`;
            }
            return md.includes('貼った見出し') && md.includes('貼った項目') ? undefined
                : `貼ったものが入っていません: ${diff(before.md, md).text}`;
        }
        case 'セルを押す': {
            if (kind === 'task') return line('- [x] やること');
            if (kind === 'done') return line('- [ ] やった');
            return undefined;
        }
        default: return undefined;
    }
}

/* ── 走らせる ── */

const cases = [];
let ran = 0;
const note = (name) => `state.root + '/${name}'`;
let canon = {};      // ノートごとの、正規化した文字
let canonBlocks = {};

async function setBody(fx) {
    // ノートを置くのは待ってよい（打ったあとの見張りとは別）── 一度は
    // 三十秒待ち、駄目ならもう一度だけ試す。
    const was = process.env.PATIENCE;
    process.env.PATIENCE = '30000';
    let r = await call('await __g.body(' + q(fx.md) + ', ' + note(fx.note) + ')');
    if (r && r.bad) r = await call('await __g.body(' + q(fx.md) + ', ' + note(fx.note) + ')');
    if (was === undefined) delete process.env.PATIENCE; else process.env.PATIENCE = was;
    if (r && r.bad) {
        // **返ってこないなら、同期で覗く** ── 何が開いたままなのかは、
        // 待たない問いなら答えられる。
        const peek = await send('Runtime.evaluate', { returnByValue: true, expression:
            '({ veil: !el("veil").hidden, more: !el("more").hidden, studio: !el("studio").hidden,'
            + ' emoji: !el("emoji").hidden, syncing, loading, composing, view,'
            + ' open: state.open && state.open.path, active: document.activeElement && document.activeElement.id,'
            + ' say: el("say").textContent, state: el("state").textContent })' });
        throw new Error('ノートが置けません: ' + r.bad + ' ' + JSON.stringify(peek.result?.result?.value || peek));
    }
    return r;
}

/// 一つのノートを置いて、一度書き戻して正規化し、その文字を憶える。
async function prime(fx) {
    await setBody(fx);
    const first = await call('__g.state()');
    const settled = await call('await __g.settle()');
    const st = await call('__g.state()');
    canon[fx.name] = st.md;
    canonBlocks[fx.name] = await call('__g.blocks()');
    // **置いただけで文字が変わるなら、それ自体が落第**（往復の検査と同じ）。
    if (!same(first.md, settled)) {
        record({ fx: fx.name, target: '（置いただけ）', where: '-', op: '往復', why:
            ['一度書き戻しただけで文字が変わります: ' + diff(first.md, settled).text] });
    }
}

function record(c) {
    ran += 1;
    cases.push(c);
}

async function oneCase(fx, target, where, op, opt) {
    const [label, text, kind] = target;
    const name = `[${fx.name}] ${label} / ${WHERE_JA[where]} / ${op}`;
    if (ONLY && !ONLY.test(name)) return;
    await setBody({ ...fx, md: canon[fx.name].slice(bodyStart(canon[fx.name])) });
    const spot = await call('__g.spot(' + q(text) + ', ' + q(where) + ')');
    if (spot.bad) { record({ fx: fx.name, target: label, where, op, why: [spot.bad] }); return; }
    const before = { md: canon[fx.name], blocks: canonBlocks[fx.name], lineText: spot.text, pos: spot.pos, block: spot.block };
    noise = [];
    let broke = null;
    try {
        const r = await opt.do();
        if (r && r.bad) broke = r.bad;
    } catch (e) {
        broke = e.message;
    }
    await sleep(WAIT);
    const st = await call('__g.state()');
    await call('__g.tidy()');
    const said = noise.filter((s) => !QUIET.some((x) => s.includes(x)));
    const why = [];
    if (broke) why.push(broke);
    if (said.length) why.push(...said);
    // **姿が取れなければ、その一件を落第にして先へ進む** ── ここで止まると
    // 一時間ぶんの結果が全部消える（実際に消えた・2026-09-10）。
    if (st.bad || typeof st.md !== 'string') {
        why.push('デスクトップ版の姿が取れません: ' + (st.bad || JSON.stringify(st).slice(0, 200)));
        record({ fx: fx.name, target: label, where, op, why, name });
        console.log('  ✗ ' + name + ' ── ' + why[0]);
        return;
    }
    if (st.again === null) why.push('もう文字に戻せません（paperToMd が null）');
    else if (typeof st.again === 'string' && st.again.startsWith('throw:')) why.push('文字に戻すとき落ちます: ' + st.again);
    else if (!same(st.md, st.again)) why.push('二度目の書き戻しで文字が変わります: ' + diff(st.md, st.again).text);
    if (opt.focus !== false && !st.caretRead) {
        why.push('焦点が画面から外れました' + (st.veil ? '（ダイアログが開いたまま）' : st.emoji ? '（板が開いたまま）' : ''));
    }
    const c = { fx: fx.name, target: label, text, kind, where, op, before, st, why, name };
    const want = expect(c);
    if (typeof want === 'string') why.push(want);
    c.want = want;
    c.change = same(before.md, st.md) ? null : diff(before.md, st.md);
    record(c);
    if (why.length) console.log('  ✗ ' + name + ' ── ' + why[0]);
}

const bodyStart = (md) => {
    // 前書きの後ろから（`__g.body` は前書きを残して本文だけ差し替える）。
    if (!md.startsWith('---\n')) return 0;
    const end = md.indexOf('\n---\n', 4);
    if (end < 0) return 0;
    // 前書きの後ろの一行空きは `body` が足すので、ここでは食べる。
    let at = end + 5;
    if (md[at] === '\n') at += 1;
    return at;
};

const t0 = Date.now();

// **エンジンが立ち上がるのを待つ**（`walk.mjs` と同じ ── 一回目は子が
// まだ起きていないことがある）。ノートを一本開いて、エディタを起こす。
{
    const r = await call(`(async () => {
        for (let i = 0; i < 40; i += 1) {
            await reload({ quiet: true });
            if (state.notes.length > 0) break;
            await new Promise((g) => setTimeout(g, 250));
        }
        if (!state.notes.length) return { bad: 'ノートが一本も読めません' };
        // ネットワークは固定の名前で開き直すので、題に合わせた改名は切る（依頼 492）。
        nameAuto = false;
        await openNote(${note('網.md')});
        for (let i = 0; i < 40 && !editor; i += 1) await new Promise((g) => setTimeout(g, 100));
        return editor ? { ok: true } : { bad: 'エディタが起きません' };
    })()`);
    if (r.bad) { console.error(r.bad); process.exit(2); }
}

// ── 一。表示画面で、位置 × 操作 ──
console.log('一。表示画面 ── 位置 × 操作');
for (const fx of FIXTURES) {
    if (ONLY && !fx.targets.some((t) => WHERES.some((w) => OPS.some(([op]) =>
        ONLY.test(`[${fx.name}] ${t[0]} / ${WHERE_JA[w]} / ${op}`))))) continue;
    try {
        await prime(fx);
    } catch (e) {
        record({ fx: fx.name, target: '（置く）', where: '-', op: '置く', why: ['ノートが置けません: ' + e.message], name: `[${fx.name}] 置く` });
        console.log(`  ✗ [${fx.name}] 置く ── ${e.message}`);
        continue;
    }
    let stuck = 0;
    walkFixture:
    for (const target of fx.targets) {
        for (const [op, doIt, opt = {}] of OPS) {
            if (opt.kinds && !opt.kinds.includes(target[2])) continue;
            for (const where of WHERES) {
                // 空の画面に「行中・行末」は無い。
                if (target[2] === 'empty' && where !== 'head') continue;
                try {
                    await oneCase(fx, target, where, op, { ...opt, do: doIt });
                    stuck = 0;
                } catch (e) {
                    const name = `[${fx.name}] ${target[0]} / ${WHERE_JA[where]} / ${op}`;
                    record({ fx: fx.name, target: target[0], where, op, why: ['台本が落ちました: ' + e.message], name });
                    console.log('  ✗ ' + name + ' ── 台本が落ちました: ' + e.message);
                    // **デスクトップ版が返ってこなくなったら、このノートの残りは飛ばす**
                    // ── 同じ落第を何百件も積んでも、何も分からない。
                    if (gone) { console.log('  ！ デスクトップ版が消えたので、ここで打ち切ります'); break walkFixture; }
                    if (/返ってきません/.test(e.message) && ++stuck >= 2) {
                        console.log(`  ！ [${fx.name}] デスクトップ版が返ってこないので、残りを飛ばします`);
                        break walkFixture;
                    }
                }
            }
        }
    }
}

// **行のどこで押しても同じ結果か**（六）── 3 つの位置の結果を突き合わせる。
{
    const groups = new Map();
    for (const c of cases) {
        if (!c.st || !SAME_ANYWHERE.has(c.op)) continue;
        const k = `[${c.fx}] ${c.target} / ${c.op}`;
        if (!groups.has(k)) groups.set(k, {});
        groups.get(k)[c.where] = c.st.md;
    }
    for (const [k, g] of groups) {
        const seen = Object.entries(g);
        if (seen.length < 2) continue;
        const [w0, m0] = seen[0];
        for (const [w, m] of seen.slice(1)) {
            if (same(m0, m)) continue;
            const why = `行のどこで押したかで結果が違います（${WHERE_JA[w0]} と ${WHERE_JA[w]}）: ${diff(m0, m).text}`;
            record({ fx: k.slice(1, k.indexOf(']')), target: k.slice(k.indexOf(']') + 2), where: '-', op: '位置ちがい', why: [why], name: k });
            console.log('  ✗ ' + k + ' ── ' + why);
            break;
        }
    }
}

async function rest() {
if (gone) return;
// ── 二。選んでから ──
if (!QUICK) {
    console.log('二。表示画面 ── 選んでから');
    const fx = FIXTURES[0];
    const ALL = ['見出し', '書式のない長い段落', 'ひとつ', 'ふたつ', '入れ子', 'みっつ', '一番', '二番',
        'やること', 'やった', '引用の一行目', '引用の二行目', '注記の本文', '掃除', '片づけ', '洗濯', '買い出し',
        '上の段落', 'fn main', '下の段落', 'インデントた段落', '最後の段落'];
    for (const [label, a, b] of SELS) {
        for (const op of SEL_OPS) {
            const name = `[選ぶ] ${label} / ${op}`;
            if (ONLY && !ONLY.test(name)) continue;
            if (!canon[fx.name]) await prime(fx);
            await setBody({ ...fx, md: canon[fx.name].slice(bodyStart(canon[fx.name])) });
            const sel = a === '*' ? await call('__g.selectAll()')
                : await call('__g.select(' + q(a) + ', ' + q(b) + ')');
            if (sel.bad) { record({ fx: '選ぶ', target: label, where: '-', op, why: [sel.bad], name }); continue; }
            const picked = a === '*' ? ALL : ALL.filter((t) => String(sel.picked || '').includes(t));
            const before = { md: canon[fx.name], blocks: canonBlocks[fx.name] };
            noise = [];
            let broke = null;
            try {
                const doIt = OPS.find(([o]) => o === op)[1];
                const r = await doIt();
                if (r && r.bad) broke = r.bad;
            } catch (e) { broke = e.message; }
            await sleep(WAIT);
            const st = await call('__g.state()');
            await call('__g.tidy()');
            const said = noise.filter((s) => !QUIET.some((x) => s.includes(x)));
            const why = [];
            if (broke) why.push(broke);
            if (said.length) why.push(...said);
            if (st.bad || typeof st.md !== 'string') {
                why.push('デスクトップ版の姿が取れません: ' + (st.bad || JSON.stringify(st).slice(0, 200)));
                record({ fx: '選ぶ', target: label, where: '-', op, why, name });
                console.log('  ✗ ' + name + ' ── ' + why[0]);
                continue;
            }
            if (st.again === null) why.push('もう文字に戻せません（paperToMd が null）');
            else if (!same(st.md, st.again)) why.push('二度目の書き戻しで文字が変わります: ' + diff(st.md, st.again).text);
            if (!st.caretRead) why.push('焦点が画面から外れました');
            // **選んでいない文字は、一文字も消えない。**（ぜんぶ選んだときを除く）
            if (a !== '*') {
                for (const t of ALL) {
                    // 選んだ文字と重なるもの（選んだ文字を含む行）は、形が変わって当然。
                    if (t.includes(a) || (b && t.includes(b))) continue;
                    // 枠を含む選びでは、枠は選んだうち（芯の 2）── 別の見張りが見る。
                    if (label === '枠を含む' && t === 'fn main') continue;
                    if (!picked.includes(t) && !st.md.includes(t)) why.push(`選んでいない「${t}」が消えました`);
                }
            }
            // 触れないかたまりは、選んで消すとき以外は変わらない（丙）。
            const fence = '```rust\nfn main() {}\n```';
            const cut = ['Backspace', 'Delete', 'あ', 'Enter', '文字を貼る', '絵文字'].includes(op);
            if (label === '枠を含む' || label === 'ぜんぶ') {
                if (cut && st.md.includes('fn main')) why.push('選んで消したのに、枠が残っています');
                if (!cut && !st.md.includes(fence)) why.push('選んで飾ったら、枠の中身が変わりました');
            }
            // 表の形（丙 ── セルの数は変えない）。
            if (['セルをまたぐ', '表の外から中へ'].includes(label) && ['Backspace', 'Delete'].includes(op)) {
                if (tableShape(st.md) !== tableShape(before.md)) why.push(`表の形が変わりました（${tableShape(before.md)} → ${tableShape(st.md)}）`);
                if (label === '表の外から中へ' && !same(st.md, before.md)) why.push('表の外から中へ跨ぐ選びを消したら、文字が変わりました: ' + diff(before.md, st.md).text);
                if (label === 'セルをまたぐ' && !st.md.includes('| 　 | 　 |\n| 　 | 　 |')) why.push('セルの中身が空になっていません: ' + show(mdLine(st.md, '|')));
            }
            if (label === '見出しを含む' && op === '太字' && mdLine(st.md, '見出し') !== '# 見出し') {
                why.push('見出しまで太字になりました: ' + show(mdLine(st.md, '見出し')));
            }
            if (label === 'セルをまたぐ' && ['太字', '斜体', '取り消し線'].includes(op)) {
                const m = { '太字': '**', '斜体': '*', '取り消し線': '~~' }[op];
                if (!st.md.includes(`| ${m}掃除${m} | ${m}片づけ${m} |`) || !st.md.includes(`| ${m}洗濯${m} | ${m}買い出し${m} |`)) why.push(`セルごとに閉じるはずが: ${show(mdLine(st.md, '掃除'))} / ${show(mdLine(st.md, '洗濯'))}`);
            }
            if (label === '段落の一部' && ['太字', '斜体', '取り消し線'].includes(op)) {
                const m = { '太字': '**', '斜体': '*', '取り消し線': '~~' }[op];
                if (!st.md.includes(`${m}長い段落${m}`)) why.push(`選んだ文字だけ飾るはずが: ${show(mdLine(st.md, '長い段落'))}`);
            }
            if (label === '行をまたぐ' && op === '太字' && !st.md.includes('- **ひとつ**\n- **ふたつ**')) {
                why.push(`項目ごとに閉じるはずが: ${diff(before.md, st.md).text}`);
            }
            if (label === 'ぜんぶ' && ['Backspace', 'Delete'].includes(op) && !same(st.md, st.md.slice(0, bodyStart(st.md)))) {
                why.push('ぜんぶ選んで消しても、文字が残っています: ' + show(st.md.slice(bodyStart(st.md), bodyStart(st.md) + 80)));
            }
            const c = { fx: '選ぶ', target: label, where: '-', op, before, st, why, name,
                change: same(before.md, st.md) ? null : diff(before.md, st.md) };
            record(c);
            if (why.length) console.log('  ✗ ' + name + ' ── ' + why[0]);
        }
    }
}

// ── 三。並べて表示で、読む側に打つ ──
if (!QUICK) {
    console.log('三。並べて表示 ── 読む側');
    const fx = FIXTURES[0];
    const SPLIT_OPS = ['見出し', '太字→あ', '絵文字', 'Enter', 'Tab', 'Backspace', 'チェックリスト'];
    for (const op of SPLIT_OPS) {
        for (const where of WHERES) {
            const target = NORMAL_TARGETS.find((t) => t[0] === '段落');
            const name = `[並べて] ${target[0]} / ${WHERE_JA[where]} / ${op}`;
            if (ONLY && !ONLY.test(name)) continue;
            if (!canon[fx.name]) await prime(fx);
            await setBody({ ...fx, md: canon[fx.name].slice(bodyStart(canon[fx.name])) });
            await call('await setView(' + q('split') + ')');
            await sleep(300);
            const spot = await call('__g.spot(' + q(target[1]) + ', ' + q(where) + ')');
            if (spot.bad) { record({ fx: '並べて', target: target[0], where, op, why: [spot.bad], name }); continue; }
            const before = { md: canon[fx.name], blocks: canonBlocks[fx.name], lineText: spot.text, pos: spot.pos, block: spot.block };
            if (process.env.PROBE) {
                await sleep(300);
                const pr = await call('({ at: caretIn(__g.line), active: document.activeElement && (document.activeElement.id || document.activeElement.tagName), '
                    + 'anchorInRead: !!getSelection().anchorNode && el("read").contains(getSelection().anchorNode), sel: getSelection().toString().length, '
                    + 'lineConnected: __g.line.isConnected, anchorConnected: getSelection().anchorNode.isConnected, anchorOffset: getSelection().anchorOffset, '
                    + 'anchorLen: getSelection().anchorNode.data && getSelection().anchorNode.data.length, kids: __g.line.childNodes.length, '
                    + 'html: __g.line.innerHTML.slice(0, 200) })');
                console.log('  probe ' + name + ' ' + JSON.stringify(pr) + ' spot=' + JSON.stringify(spot));
            }
            noise = [];
            let broke = null;
            try {
                const [, doIt] = OPS.find(([o]) => o === op);
                const r = await doIt();
                if (r && r.bad) broke = r.bad;
            } catch (e) { broke = e.message; }
            await sleep(WAIT);
            const st = await call('__g.state()');
            await call('__g.tidy()');
            await call('await setView(' + q('read') + ')');
            const said = noise.filter((s) => !QUIET.some((x) => s.includes(x)));
            const why = [];
            if (broke) why.push(broke);
            if (said.length) why.push(...said);
            if (st.bad || typeof st.md !== 'string') {
                why.push('デスクトップ版の姿が取れません: ' + (st.bad || JSON.stringify(st).slice(0, 200)));
                record({ fx: '並べて', target: target[0], where, op, why, name });
                console.log('  ✗ ' + name + ' ── ' + why[0]);
                continue;
            }
            if (st.view !== 'split') why.push('並べて表示のはずが、画面が ' + st.view + ' に変わりました');
            if (st.again === null) why.push('もう文字に戻せません（paperToMd が null）');
            else if (!same(st.md, st.again)) why.push('二度目の書き戻しで文字が変わります: ' + diff(st.md, st.again).text);
            if (!st.caretRead) why.push('焦点が画面から外れました');
            const c = { fx: '並べて', target: target[0], text: target[1], kind: 'p', where, op, before, st, why, name };
            const want = expect(c);
            if (typeof want === 'string') why.push(want);
            c.want = want;
            c.change = same(before.md, st.md) ? null : diff(before.md, st.md);
            record(c);
            if (why.length) console.log('  ✗ ' + name + ' ── ' + why[0]);
        }
    }
}

// ── 四。コード画面で、位置 × 記号 ──
if (!QUICK) {
    console.log('四。コード画面 ── 位置 × 記号');
    const fx = FIXTURES[0];
    if (!canon[fx.name]) await prime(fx);
    const body = canon[fx.name].slice(bodyStart(canon[fx.name]));
    await setBody({ ...fx, md: body });
    await call('await setView(' + q('write') + ')');
    await sleep(300);
    const base = await call('__g.codeText()');
    const groups = new Map();
    for (const target of NORMAL_TARGETS) {
        for (const op of CODE_OPS) {
            for (const where of WHERES) {
                const name = `[コード] ${target[0]} / ${WHERE_JA[where]} / ${op}`;
                if (ONLY && !ONLY.test(name)) continue;
                // 戻す（保存は要らない ── 見るのはエディタの文字だけ）。
                await call('(loading = true, editor.setValue(' + q(base) + '), loading = false, true)');
                const spot = await call('__g.codeSpot(' + q(target[1]) + ', ' + q(where) + ')');
                if (spot.bad) { record({ fx: 'コード', target: target[0], where, op, why: [spot.bad], name }); continue; }
                noise = [];
                const r = await mark(op)();
                await sleep(120);
                const now = await call('__g.codeText()');
                const said = noise.filter((s) => !QUIET.some((x) => s.includes(x)));
                const why = [];
                if (r && r.bad) why.push(r.bad);
                if (said.length) why.push(...said);
                const d = diff(base, now);
                if (same(base, now)) why.push('押しても何も変わりません');
                else if (CODE_WRAP.has(op)) {
                    // 選ばずに押した ── 印だけが caret のところに入る。
                    const m = { '太字': '****', '斜体': '**', '取り消し線': '~~~~' }[op];
                    const line = now.split('\n')[spot.line - 1] || '';
                    const want = spot.text.slice(0, spot.col - 1) + m + spot.text.slice(spot.col - 1);
                    if (line !== want) why.push(`マークが caret のところに入っていません: ${show(spot.text)} → ${show(line)}`);
                } else if (CODE_PUT[op]) {
                    const rows = now.split('\n');
                    const line = rows[spot.line - 1] || '';
                    if (op === '水平線') {
                        // 水平線は行を割って、**次の行**に一本で入る（`put('\n---\n\n')`）。
                        const head = spot.text.slice(0, spot.col - 1);
                        if (line !== head || rows[spot.line] !== '---') {
                            why.push(`水平線は caret で行を割って次の行に入るはずが: ${show(rows.slice(spot.line - 1, spot.line + 2).join('\n'))}`);
                        }
                    } else if (!line.startsWith(spot.text.slice(0, spot.col - 1) + CODE_PUT[op].slice(0, 3))) {
                        why.push(`${op}が caret のところに入っていません: ${show(spot.text)} → ${show(line)}`);
                    }
                } else if (d.from.length !== 1 || d.to.length !== 1) {
                    why.push(`その一行だけが変わるはずが、${d.from.length} 行 → ${d.to.length} 行: ${d.text}`);
                }
                const c = { fx: 'コード', target: target[0], where, op, why, name, change: d, st: { md: now } };
                record(c);
                if (why.length) console.log('  ✗ ' + name + ' ── ' + why[0]);
                if (!CODE_WRAP.has(op) && !CODE_PUT[op]) {
                    const k = `[コード] ${target[0]} / ${op}`;
                    if (!groups.has(k)) groups.set(k, {});
                    groups.get(k)[where] = now;
                }
            }
        }
    }
    for (const [k, g] of groups) {
        const seen = Object.entries(g);
        for (const [w, m] of seen.slice(1)) {
            if (same(seen[0][1], m)) continue;
            const why = `行のどこで押したかで結果が違います（${WHERE_JA[seen[0][0]]} と ${WHERE_JA[w]}）: ${diff(seen[0][1], m).text}`;
            record({ fx: 'コード', target: k, where: '-', op: '位置ちがい', why: [why], name: k });
            console.log('  ✗ ' + k + ' ── ' + why);
            break;
        }
    }
    await call('await setView(' + q('read') + ')');
}

}
try {
    await rest();
} catch (e) {
    record({ fx: '台本', target: '-', where: '-', op: '-', why: ['台本が途中で落ちました: ' + e.message], name: '[台本] 途中で落ちた' });
    console.log('  ✗ 台本が途中で落ちました: ' + e.message);
}
// 後始末 ── ネットワークのノートを元の文字に（デスクトップ版が返ってこないなら諦める）。
try { await setBody({ ...FIXTURES[0], md: 'ここはネットワークが使うノートです。' }); } catch { /* 報せは書く */ }

/* ── 報せ ── */

mkdirSync(OUT, { recursive: true });
const bad = cases.filter((c) => c.why && c.why.length);
const seen = cases.filter((c) => !(c.why && c.why.length) && c.want === undefined && c.st);
const lines = [];
lines.push(`# ネットワークの報せ（${new Date().toISOString().slice(0, 16).replace('T', ' ')}）`, '');
lines.push(`${ran} とおり動かして、落第 ${bad.length} 件・見たまま ${seen.length} 件・${Math.round((Date.now() - t0) / 1000)} 秒`, '');
lines.push('## 落第', '');
for (const c of bad) {
    lines.push(`- **${c.name || `[${c.fx}] ${c.target} / ${c.where} / ${c.op}`}**`);
    for (const w of c.why) lines.push(`  - ${w}`);
    if (c.change) lines.push(`  - 変わったところ: ${c.change.text}`);
}
lines.push('', '## 見たまま（決めごとが無いところ ── 本人に訊く）', '');
// 操作 → かたまりの種類 → 位置ごとの結果、で並べる（同じ結果はまとめる）。
const byOp = new Map();
for (const c of seen) {
    if (!byOp.has(c.op)) byOp.set(c.op, new Map());
    const m = byOp.get(c.op);
    const k = `[${c.fx}] ${c.target}`;
    if (!m.has(k)) m.set(k, []);
    m.get(k).push(`${WHERE_JA[c.where] || c.where}: ${c.change ? c.change.text : '変わらず'}`);
}
for (const [op, m] of byOp) {
    lines.push(`### ${op}`, '');
    for (const [k, rows] of m) {
        const uniq = [...new Set(rows.map((r) => r.replace(/^[^:]+: /, '')))];
        if (uniq.length === 1) lines.push(`- ${k} ── ${uniq[0]}`);
        else { lines.push(`- ${k}`); for (const r of rows) lines.push(`  - ${r}`); }
    }
    lines.push('');
}
writeFileSync(join(OUT, 'report.md'), lines.join('\n'));
writeFileSync(join(OUT, 'cases.jsonl'), cases.map((c) => JSON.stringify({
    name: c.name, fx: c.fx, target: c.target, where: c.where, op: c.op, why: c.why,
    want: c.want === undefined ? null : c.want, change: c.change ? c.change.text : null,
    after: c.st ? c.st.md : null,
    html: c.st && c.why && c.why.length ? c.st.html : undefined,
})).join('\n'));

console.log('');
console.log(`${ran} とおり動かして、落第 ${bad.length} 件・見たまま ${seen.length} 件（${Math.round((Date.now() - t0) / 1000)} 秒）`);
console.log(`報せ: ${join(OUT, 'report.md')}`);
ws.close();
process.exit(bad.length ? 1 : 0);
