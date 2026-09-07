#!/usr/bin/env node
/* ノートを替えても、本文が混ざらないか。
 *
 * **二本のノートが、前書きだけ自分のまま本文が別のノートになった**
 * （2026-09-07）。「表示」の面の書き戻し（`syncRead`）は、面に出ている
 * 字を「いま開いているノートの字」だと思い込んでいた ── 「コード」の
 * 面ではノートを替えても組み直さないので、面は前のノートの字のまま。
 * 次に別のノートへ替えた瞬間、その字が今のノートへ書き込まれた。同じ道で、
 * 「コード」で打った行がノートを替えた瞬間に**組んだ時の字へ戻される**。
 *
 * ここは `gui/renderer.js` から `syncRead` と `drawRead`、面の札
 * （`readDrawn` / `readStale` / `readCurrent`）を切り出し、周りを偽物で
 * 囲って踏み直す。**組み直さずにノートを替える**（「コード」の面の形）のが
 * 肝で、それでも前の字が書き戻されなければよい。
 *
 *     node scripts/switch-test.js
 */
'use strict';
const fs = require('fs');
const path = require('path');

// 画面が要るので、軽い DOM を借りる。**無ければ飛ばす**（paper-test と同じ）。
let JSDOM;
try {
    ({ JSDOM } = require(path.join(__dirname, '..', 'gui', 'node_modules', 'jsdom')));
} catch {
    console.log('jsdom がありません（gui で npm install すると走ります）── 飛ばします');
    process.exit(0);
}

const src = fs.readFileSync(path.join(__dirname, '..', 'gui', 'renderer.js'), 'utf8');

/// 上の階の関数を一つ切り出す（`function 名(` から、行頭の `}` まで）。
function cut(sig) {
    const from = src.indexOf(sig);
    const to = from < 0 ? -1 : src.indexOf('\n}\n', from);
    if (from < 0 || to < 0) {
        console.error('gui/renderer.js から切り出せません: ' + sig);
        process.exit(2);
    }
    return src.slice(from, to + 3);
}
/// 一行で書いてある関数。
function line(sig) {
    const from = src.indexOf(sig);
    const to = from < 0 ? -1 : src.indexOf('\n', from);
    if (from < 0 || to < 0) {
        console.error('gui/renderer.js から切り出せません: ' + sig);
        process.exit(2);
    }
    return src.slice(from, to + 1);
}

const dom = new JSDOM('<!doctype html><body><div id="read"></div></body>');
global.window = dom.window;
global.document = dom.window.document;

/* ── 周りの偽物 ── */
global.el = (id) => document.getElementById(id);
global.state = { open: null, head: '', dirty: false };
global.editor = {
    text: '',
    getValue() { return this.text; },
    setValue(v) { this.text = v; },
};
global.view = 'read';
global.loading = false;
global.syncing = false;
global.tocOn = false;
global.readSeq = 0;
global.writes = [];
global.told = [];
global.say = (s) => told.push(s);
global.why = (e) => String(e && e.message || e);
global.whole = () => state.head + editor.getValue();
// 面の字 → Markdown。本物（`paperToMd`）は paper-test が見ている。
global.readToMd = () => el('read').textContent;
global.save = async () => { writes.push({ path: state.open.path, text: whole() }); };
global.armRead = () => {};
global.drawCount = () => {};
global.drawToc = () => {};
global.headLines = () => 0;
global.tailStop = () => {};
global.findPictures = () => {};
global.paintCode = () => Promise.resolve();
global.drawDiagrams = () => {};
// 組む口。**一拍遅れて返す** ── 本物もエンジンとの往復で、その間に
// ノートが替わりうる。
global.htmlDelay = 0;
global.ask = async (method, params) => {
    if (method !== 'html') throw new Error('知らない口: ' + method);
    if (htmlDelay) await new Promise((r) => setTimeout(r, htmlDelay));
    const body = params.text.startsWith(state.head) ? params.text.slice(state.head.length) : params.text;
    return { html: '<p>' + body + '</p>' };
};

// eslint-disable-next-line no-eval
(0, eval)(line('function readDrawn(') + line('function readStale(') + line('function readCurrent(')
    + cut('function sameNote(')
    + cut('async function syncRead(') + cut('async function drawRead('));

let bad = 0;
const ok = (yes, what, got) => {
    console.log((yes ? '  ✓ ' : '  ✗ ') + what);
    if (!yes) { bad++; if (got !== undefined) console.log('      ' + JSON.stringify(got)); }
};

const A = { path: '/notes/障害/A.md', head: '---\ntitle: A\n---\n', body: '\n# A\n\nこれは A の本文です。\n' };
const B = { path: '/notes/設計/B.md', head: '---\ntitle: B\n---\n', body: '\n# B\n\nこれは B の本文です。\n' };
const C = { path: '/notes/カーマイン/C.md', head: '---\ntitle: C\n---\n', body: '\n# C\n\nこれは C の本文です。\n' };

/// `openNote` の、面に関わるところだけ。**面は触らない** ── 「コード」の
/// 面では組み直さないので、前のノートの字が残ったまま次が開く。
/// （本物の `openNote` は替えるとき面を空にもするが、それに頼らない。）
function open(n) {
    state.open = { path: n.path };
    state.head = n.head;
    loading = true;
    editor.setValue(n.body);
    loading = false;
    state.dirty = false;
}
function fresh() {
    el('read').replaceChildren();
    delete el('read').dataset.of;
    writes = [];
    told = [];
    view = 'read';
    htmlDelay = 0;
}

(async () => {
    console.log('ノートを替えたあと、前のノートの面を書き戻さないか');
    {
        fresh();
        open(A);
        await drawRead();
        ok(el('read').textContent.includes('A の本文'), '面には A が組んである');
        // 「コード」へ替えて、B・C と開く（組み直しは走らない）。
        view = 'write';
        open(B);
        await syncRead();
        open(C);
        await syncRead();
        ok(writes.length === 0, '何も書かない', writes);
        ok(editor.getValue() === C.body, 'エディタは C のまま', editor.getValue());
    }

    console.log('「コード」で打ったあと、組んだ時の字へ戻さないか');
    {
        fresh();
        open(A);
        await drawRead();
        view = 'write';
        // 打った ── Monaco の変更で `readStale()` が呼ばれる（renderer.js の
        // `onDidChangeModelContent`。台帳 360 がそこを見ている）。
        editor.setValue(A.body + '足した一行\n');
        readStale();
        await syncRead();
        ok(writes.length === 0, '古い面を書き戻さない', writes);
        ok(editor.getValue().includes('足した一行'), '足した行が残る', editor.getValue());
    }

    console.log('面で打ったものは、いつも通り書き戻るか（守りが強すぎないか）');
    {
        fresh();
        open(B);
        await drawRead();
        el('read').querySelector('p').textContent = 'B を面で直した';
        await syncRead();
        ok(writes.length === 1 && writes[0].path === B.path, 'B に一度書く', writes);
        ok(writes.length === 1 && writes[0].text === B.head + 'B を面で直した', '書くのは直した字', writes[0]);
        ok(editor.getValue() === 'B を面で直した', 'エディタも直した字', editor.getValue());
    }

    console.log('遅れて着いた前のノートの字に、今のノートの札を付けないか');
    {
        fresh();
        open(A);
        htmlDelay = 20;
        const late = drawRead();   // A を組みはじめる
        view = 'write';
        open(B);                   // 帰ってくる前に B へ
        await late;
        ok(!readCurrent(), '面は B のものではない', el('read').dataset.of);
        ok(!el('read').textContent.includes('A の本文'), '遅れた A の字を面に出さない', el('read').textContent);
        await syncRead();
        ok(writes.length === 0, '何も書かない', writes);
    }

    console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
    process.exit(bad ? 1 : 0);
})();
