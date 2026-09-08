#!/usr/bin/env node
/* 「表示」の面で鍵を押したら、字がどうなるか。
 *
 * `round-test.js` は**触らずに**往復させる。こちらは**押してから**往復
 * させる ── 押した跡が字に落ちるか、押していない行が動いていないか。
 *
 *     もとの .md ──▶ 面 ──▶ 鍵を押す ──▶ paperToMd ──▶ 字
 *
 * 決めごとは `PAPER.ja.md` 六章の乙（鍵）。芯は三つ:
 *
 *   1. 面の上の一打は、字の上の一つの記号に対応する
 *      （行頭の Backspace は記号が一つ外れ、Tab は一段深くなる。
 *        「何も起きない・焦点が飛ぶ」は無い ── それは既定の事故）
 *   2. 選びは意思、一打は事故（選ばずに押した一打で、図や枠は消えない）
 *   3. 触れないものには caret を置かない・入れない
 *
 *     node scripts/key-test.js
 */
'use strict';
const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');

const root = path.join(__dirname, '..');

let JSDOM;
try {
    ({ JSDOM } = require(path.join(root, 'gui', 'node_modules', 'jsdom')));
} catch {
    console.log('jsdom がありません（gui で npm install すると走ります）── 飛ばします');
    process.exit(0);
}
const engine = [
    path.join(root, 'target', 'release', 'amber-server'),
    path.join(root, 'gui', 'amber-server'),
].find((at) => fs.existsSync(at));
if (!engine) {
    console.log('エンジンがありません（cargo build --release -p amber-server）── 飛ばします');
    process.exit(0);
}

const src = fs.readFileSync(path.join(root, 'gui', 'renderer.js'), 'utf8');
const from = src.indexOf('function richBlock(');
const to = src.indexOf('/// この窓の「表示」の面を、上の切り出しに繋ぐ薄い包み。');
if (from < 0 || to < 0 || to < from) {
    console.error('gui/renderer.js から「表示」の面を切り出せません');
    process.exit(2);
}
const dom = new JSDOM('<!doctype html><body><div id="paper"></div></body>');
global.window = dom.window;
global.document = dom.window.document;
global.Node = dom.window.Node;
global.getSelection = () => dom.window.getSelection();
// eslint-disable-next-line no-eval
(0, eval)(src.slice(from, to));
const box = document.getElementById('paper');

const child = spawn(engine, [], { stdio: ['pipe', 'pipe', 'inherit'] });
let buf = '';
const waiting = [];
child.stdout.on('data', (b) => {
    buf += b.toString();
    let n;
    while ((n = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, n);
        buf = buf.slice(n + 1);
        const go = waiting.shift();
        if (go) go(JSON.parse(line));
    }
});
let id = 0;
const ask = (m, p2) => new Promise((go) => {
    waiting.push(go);
    child.stdin.write(JSON.stringify({ id: ++id, method: m, params: p2 }) + '\n');
});

/// 面を組む。
async function draw(md) {
    const r = await ask('html', { text: md });
    box.innerHTML = r.ok.html || '';
    armPaper(box, md, true);
}

/// `at` 番目の行（面に出ている順の、いちばん外のかたまり）の中の
/// `n` 文字目に caret を置く。
function caretAt(node, n) {
    const walk = document.createTreeWalker(node, 4);
    let seen = 0;
    let t;
    while ((t = walk.nextNode())) {
        if (seen + t.data.length >= n) {
            const r = document.createRange();
            r.setStart(t, n - seen);
            r.collapse(true);
            const sel = getSelection();
            sel.removeAllRanges();
            sel.addRange(r);
            return true;
        }
        seen += t.data.length;
    }
    // 字の無い節（空の段落）── 節そのものの中へ。
    const r = document.createRange();
    r.selectNodeContents(node);
    r.collapse(true);
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
    return true;
}

/// 中の字で節を探す。**いちばん内側を掴む** ── 入れ子の一覧では、外側の
/// 項目も中の字を持っている（`textContent` は子まで拾う）。
const find = (text) => [...box.querySelectorAll('li, p, h1, h2, h3, td, th, blockquote > p, .alert > p')]
    .filter((n) => n.textContent.includes(text))
    .pop();

let bad = 0;
const ok = (yes, what, got) => {
    console.log((yes ? '  ✓ ' : '  ✗ ') + what);
    if (!yes) { bad++; if (got !== undefined) console.log('      ' + JSON.stringify(got)); }
};
const say = (s) => console.log(s);

/// 押して、字に戻す。
///
/// caret は **`where` の直前から数えて `at` 文字目**に置く ── 段落の中の
/// 改行は `<br>` なので、二行の引用は一つの節になっている（節の先頭から
/// 数えると、二行目に置けない）。
async function press(md, where, at, hit) {
    await draw(md);
    const node = find(where);
    if (!node) return { なし: where };
    caretAt(node, Math.max(0, node.textContent.indexOf(where)) + at);
    const took = hit();
    return { took, md: paperToMd(box, '') };
}

(async () => {
    say('Tab ── 一覧の中は段、外は字下げ');
    {
        let r = await press('- あ\n- い', 'い', 0, () => checkTab(box, false));
        ok(r.md === '- あ\n  - い\n', '二つ目を Tab で一段深く', r.md);

        r = await press('- あ\n- い', 'あ', 0, () => checkTab(box, false));
        ok(r.md === '- あ\n- い\n', '最初の項目は深くしない（親が無い）', r.md);

        r = await press('- あ\n  - こ', 'こ', 0, () => checkTab(box, true));
        ok(r.md === '- あ\n- こ\n', 'Shift+Tab で一段浅く', r.md);

        r = await press('- あ', 'あ', 0, () => checkTab(box, true));
        ok(r.md === '- あ\n', 'いちばん浅い段では何もしない', r.md);

        r = await press('- [ ] やること\n- [ ] もう一つ', 'もう一つ', 0, () => checkTab(box, false));
        ok(r.md === '- [ ] やること\n  - [ ] もう一つ\n', '升は升のまま深くなる', r.md);
    }

    say('Tab ── 段落は字下げ（全角空白）');
    {
        let r = await press('ふつうの段落。', 'ふつう', 0, () => checkTab(box, false));
        ok(r.md === '　ふつうの段落。\n', '段落の頭に全角空白が一つ', r.md);

        r = await press('　字下げた段落。', '字下げた', 0, () => checkTab(box, true));
        ok(r.md === '字下げた段落。\n', 'Shift+Tab で外れる', r.md);

        r = await press('ふつうの段落。', 'ふつう', 3, () => checkTab(box, false));
        ok(r.md === '　ふつうの段落。\n', '行のどこで押しても、頭に付く', r.md);

        r = await press('# 見出し', '見出し', 0, () => checkTab(box, false));
        ok(r.md === '# 見出し\n', '見出しでは何も起きない', r.md);
        ok(r.took === true, '見出しでも受ける（焦点を飛ばさない）', r.took);
    }

    say('行頭の Backspace ── 記号を一つ外す');
    {
        let r = await press('- あ\n- い\n- う', 'い', 0, () => checkBack(box));
        ok(r.md === '- あ\n\nい\n\n- う\n', '途中の項目は、記号が外れて段落になる（下は残る）', r.md);

        r = await press('- [x] やった', 'やった', 0, () => checkBack(box));
        ok(r.md === 'やった\n', '升も一緒に外れる', r.md);

        r = await press('- あ\n  - こ', 'こ', 0, () => checkBack(box));
        ok(r.md === '- あ\n- こ\n', '入れ子の項目は、一段浅くなる', r.md);

        r = await press('## 見出し', '見出し', 0, () => checkBack(box));
        ok(r.md === '見出し\n', '見出しは段落になる', r.md);

        // 字下げの `　` は**ただの字**なので、Backspace は既定のまま一つ
        // 消せばよい（`PAPER.ja.md` 六章 ──「芯の 1 がそのまま効く」）。
        r = await press('　字下げた段落。', '字下げた', 0, () => checkBack(box));
        ok(r.took === false, '字下げは、ただの字として一打で消える（既定）', r.took);

        r = await press('> 一行目\n> 二行目', '一行目', 0, () => checkBack(box));
        ok(r.md === '一行目\n\n> 二行目\n', '引用は、最初の行の行頭だけ出る', r.md);

        r = await press('> 一行目\n> 二行目', '二行目', 0, () => checkBack(box));
        ok(r.took === false, '引用の途中の行は、前の行と繋がる（既定に任せる）', r.took);

        r = await press('ふつうの段落。', 'ふつう', 3, () => checkBack(box));
        ok(r.took === false, '行頭でなければ、字を消す鍵に戻る', r.took);

        r = await press('ふつうの段落。', 'ふつう', 0, () => checkBack(box));
        ok(r.took === false, '外すものが無ければ、既定に任せる', r.took);
    }

    say('Enter ── 見出しの次は段落、表は下のセルへ');
    {
        let r = await press('## 見出し', '見出し', 3, () => checkReturn(box));
        ok(r.md === '## 見出し\n', '末尾で押したら、見出しはそのまま', r.md);

        r = await press('## 見出しの途中', '見出し', 3, () => checkReturn(box));
        ok(r.md === '## 見出し\n\nの途中\n', '途中で押したら、後ろは段落', r.md);

        r = await press('## 見出し', '見出し', 0, () => checkReturn(box));
        ok(r.took === false, '先頭では既定に任せる（上に空の段落）', r.took);

        r = await press('ふつうの段落。', 'ふつう', 3, () => checkReturn(box));
        ok(r.took === false, '段落では既定に任せる', r.took);

        const two = '| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |';
        r = await press(two, '1', 0, () => checkReturn(box));
        ok(r.took === true, '表のセルでは受ける（下のセルへ）', r.took);
        ok(r.md === two + '\n', '下に行があるなら、行は増えない', r.md);

        const one = '| a | b |\n| --- | --- |\n| 1 | 2 |';
        r = await press(one, '1', 0, () => checkReturn(box));
        ok(r.md === one + '\n| 　 | 　 |\n', '最後の行なら、行を一つ足す', r.md);
    }

    say('Shift+Enter ── 段落の中の改行');
    {
        let r = await press('一行目。', '一行目。', 4, () => checkSoftReturn(box));
        ok(r.md === '一行目。\n', '段落の中に改行が入る（字は変わらない）', r.md);

        r = await press('一行目。つづき', '一行目。', 4, () => checkSoftReturn(box));
        ok(r.md === '一行目。\nつづき\n', '途中で押したら、後ろが次の行へ', r.md);

        r = await press('# 見出し', '見出し', 2, () => checkSoftReturn(box));
        ok(r.took === false, '見出しでは、ふつうの Enter に任せる', r.took);

        r = await press('- あ', 'あ', 1, () => checkSoftReturn(box));
        ok(r.took === false, '項目でも、ふつうの Enter に任せる', r.took);

        const t = '| a | b |\n| --- | --- |\n| 1 | 2 |';
        r = await press(t, '1', 1, () => checkSoftReturn(box));
        ok(r.took === true, '表のセルでは、受けて止める（改行は書けない）', r.took);
        ok(r.md === t + '\n', '表は一文字も変わらない', r.md);
    }

    say('触れないかたまりは、一打では消えない');
    {
        const md = '```\nコード\n```\n\nそのあと。';
        const r = await press(md, 'そのあと', 0, () => checkBack(box));
        ok(r.took === true, '枠のすぐ下の行頭では、何も起きない（受けて止める）', r.took);
        ok(r.md === md + '\n', '字は一文字も動いていない', r.md);
    }

    say('選んで飾る ── 一行は見出しか項目か、どちらか一つ');
    {
        // 見出しの行を箇条書きにすると、見出しは落ちる（押した瞬間に見える）。
        await draw('## 見出し\n\n段落。');
        const h = box.querySelector('h2');
        caretAt(h, 0);
        flattenHeads(box);
        ok(!box.querySelector('h2'), '見出しが段落に落ちる');
        ok(paperToMd(box, '') === '見出し\n\n段落。\n',
           '字はそのまま残る（`#` だけが落ちる）', paperToMd(box, ''));

        // 触れる行が無ければ、何もしない。
        await draw('段落だけ。');
        caretAt(box.firstElementChild, 0);
        flattenHeads(box);
        ok(paperToMd(box, '') === '段落だけ。\n', '見出しでなければ、触らない', paperToMd(box, ''));
    }

    say('選んで飾る ── 中で押したら外れる');
    {
        await draw('> 一行目\n> 二行目');
        const q = box.querySelector('blockquote p');
        caretAt(q, 0);
        ok(inside(box, 'blockquote') === true, '引用の中に居ることが分かる');
        unwrapBlock(box, 'blockquote');
        ok(paperToMd(box, '') === '一行目\n二行目\n', '引用がまとめて外れる', paperToMd(box, ''));

        await draw('- あ\n- い');
        const li = box.querySelector('li');
        caretAt(li, 0);
        ok(inside(box, 'UL') === true, '一覧の中に居ることが分かる');
        unwrapList(box);
        ok(paperToMd(box, '') === 'あ\n\n- い\n', '押した項目だけ外れる', paperToMd(box, ''));

        await draw('段落。');
        caretAt(box.firstElementChild, 0);
        ok(inside(box, 'blockquote') === false, '外に居るなら、付けるほうへ');
    }

    say('選んで消す ── 表のセルの数を変えない');
    {
        const t = '| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |';
        // セルをまたいで選ぶ（1 → 4）。
        await draw(t);
        const cells = [...box.querySelectorAll('td')];
        let r = document.createRange();
        r.setStart(cells[0].firstChild || cells[0], 0);
        r.setEnd(cells[3].firstChild || cells[3], (cells[3].textContent || '').length);
        let sel = getSelection(); sel.removeAllRanges(); sel.addRange(r);
        ok(checkCut(box) === true, 'セルをまたいだ選びは、受けて止める');
        const md = paperToMd(box, '');
        ok(md === '| a | b |\n| --- | --- |\n| 　 | 　 |\n| 　 | 　 |\n',
           '中身は空になるが、セルの数は変わらない', md);

        // 一つのセルの中だけの選びは、既定に任せる。
        await draw(t);
        const one = box.querySelector('td');
        r = document.createRange();
        r.selectNodeContents(one);
        sel = getSelection(); sel.removeAllRanges(); sel.addRange(r);
        ok(checkCut(box) === false, '一つのセルの中は、ふつうに消える');

        // 表の外から中へ跨ぐ ── 何も起きない。
        const mix = '段落。\n\n| a | b |\n| --- | --- |\n| 1 | 2 |';
        await draw(mix);
        const para = [...box.children].find((n) => n.tagName === 'P');
        const cell = box.querySelector('td');
        r = document.createRange();
        r.setStart(para.firstChild, 0);
        r.setEnd(cell.firstChild || cell, 1);
        sel = getSelection(); sel.removeAllRanges(); sel.addRange(r);
        ok(checkCut(box) === true, '表の外から中へ跨ぐ選びは、受けて止める');
        ok(paperToMd(box, '') === mix + '\n', '字は一文字も動かない', paperToMd(box, ''));
    }

    say('矢印 ── 触れないかたまりを跨ぐ');
    {
        const md = '上の段落。\n\n```\nコード\n```\n\n下の段落。';
        let r = await press(md, '上の段落', 5, () => checkArrow(box, 'down'));
        ok(r.took === true, '枠の上の行の末尾で ↓ を押すと、跨ぐ', r.took);
        ok(r.md === md + '\n', '字は一文字も動かない', r.md);

        r = await press(md, '下の段落', 0, () => checkArrow(box, 'up'));
        ok(r.took === true, '枠の下の行の頭で ↑ を押すと、跨ぐ', r.took);

        r = await press(md, '上の段落', 2, () => checkArrow(box, 'down'));
        ok(r.took === false, '行の途中では、ふつうに動く（既定に任せる）', r.took);

        // 図で始まるノート ── 上に一行足す道が要る。
        const top = '```\nコード\n```\n\n下の段落。';
        await draw(top);
        headStop(box);
        const first = box.firstElementChild;
        ok(first && first.tagName === 'P' && !first.textContent.trim(),
           '枠で始まるノートの上に、降りられる一行が置かれる', first && first.tagName);
        ok(paperToMd(box, '') === top + '\n',
           'その一行は、字に戻すとき落ちる（ファイルは増えない）', paperToMd(box, ''));

        await draw('ふつうの段落。');
        headStop(box);
        ok(box.firstElementChild.textContent.includes('ふつう'),
           '先頭が触れるものなら、何も置かない', box.firstElementChild.textContent);
    }

    say('注記の札は、行として数えない');
    {
        const r = await press('> [!NOTE]\n> 覚えておくこと。', '覚えて', 0, () => checkBack(box));
        ok(r.md === '覚えておくこと。\n', '注記の最初の行も出られる（札ごと消える）', r.md);
    }

    child.stdin.end();
    console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
    process.exit(bad ? 1 : 0);
})();
