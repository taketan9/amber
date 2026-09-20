#!/usr/bin/env node
/* 「表示」画面でキーを押したら、文字がどうなるか。
 *
 * `round-test.js` は**触らずに**往復させる。こちらは**押してから**往復
 * させる ── 押した跡が文字に落ちるか、押していない行が動いていないか。
 *
 *     もとの .md ──▶ 画面 ──▶ キーを押す ──▶ paperToMd ──▶ 文字
 *
 * 決めごとは `PAPER.ja.md` 六章の乙（鍵）。芯は3 つ:
 *
 *   1. 画面の上の一打は、文字の上の一つの記号に対応する
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
// **Windows では `.exe`。** ここを見落とすと、Windows の runner では
// 「エンジンがありません」と言って**黙って飛ばす** ── 通ったように見えて、
// 何も見ていない（依頼 434）。
const engine = [
    path.join(root, 'target', 'release', 'amber-server.exe'),
    path.join(root, 'target', 'release', 'amber-server'),
    path.join(root, 'gui', 'amber-server.exe'),
    path.join(root, 'gui', 'amber-server'),
].find((at) => fs.existsSync(at));
if (!engine) {
    console.log('エンジンがありません（cargo build --release -p amber-server）── 飛ばします');
    process.exit(0);
}

const src = fs.readFileSync(path.join(root, 'gui', 'renderer.js'), 'utf8');
const from = src.indexOf('function richBlock(');
const to = src.indexOf('/// このデスクトップ版の「表示」画面を、上の切り出しに繋ぐ薄い包み。');
if (from < 0 || to < 0 || to < from) {
    console.error('gui/renderer.js から「表示」画面を切り出せません');
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

/// 画面をビルドする。
async function draw(md) {
    const r = await ask('html', { text: md });
    box.innerHTML = r.ok.html || '';
    armPaper(box, md, true);
}

/// `at` 番目の行（画面に出ている順の、いちばん外のかたまり）の中の
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
    // 文字の無い節（空の段落）── 節そのものの中へ。
    const r = document.createRange();
    r.selectNodeContents(node);
    r.collapse(true);
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
    return true;
}

/// 中の文字で節を探す。**いちばん内側を掴む** ── 入れ子の一覧では、外側の
/// 項目も中の文字を持っている（`textContent` は子まで拾う）。
const find = (text) => [...box.querySelectorAll('li, p, h1, h2, h3, td, th, blockquote > p, .alert > p')]
    .filter((n) => n.textContent.includes(text))
    .pop();

let bad = 0;
const ok = (yes, what, got) => {
    console.log((yes ? '  ✓ ' : '  ✗ ') + what);
    if (!yes) { bad++; if (got !== undefined) console.log('      ' + JSON.stringify(got)); }
};
const say = (s) => console.log(s);

/// 押して、テキストに戻す。
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
    say('Tab ── 一覧の中は段、外はインデント');
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
        ok(r.md === '- [ ] やること\n  - [ ] もう一つ\n', 'セルはセルのまま深くなる', r.md);
    }

    say('Tab ── 段落はインデント（全角空白）');
    {
        let r = await press('ふつうの段落。', 'ふつう', 0, () => checkTab(box, false));
        ok(r.md === '　ふつうの段落。\n', '段落の頭に全角空白が一つ', r.md);

        r = await press('　インデントた段落。', 'インデントた', 0, () => checkTab(box, true));
        ok(r.md === 'インデントた段落。\n', 'Shift+Tab で外れる', r.md);

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
        ok(r.md === 'やった\n', 'セルも一緒に外れる', r.md);

        r = await press('- あ\n  - こ', 'こ', 0, () => checkBack(box));
        ok(r.md === '- あ\n- こ\n', '入れ子の項目は、一段浅くなる', r.md);

        r = await press('## 見出し', '見出し', 0, () => checkBack(box));
        ok(r.md === '見出し\n', '見出しは段落になる', r.md);

        // インデントの `　` は**ただの文字**なので、Backspace は既定のまま一つ
        // 消せばよい（`PAPER.ja.md` 六章 ──「芯の 1 がそのまま効く」）。
        r = await press('　インデントた段落。', 'インデントた', 0, () => checkBack(box));
        ok(r.took === false, 'インデントは、ただの文字として一打で消える（既定）', r.took);

        r = await press('> 一行目\n> 二行目', '一行目', 0, () => checkBack(box));
        ok(r.md === '一行目\n\n> 二行目\n', '引用は、最初の行の行頭だけ出る', r.md);

        r = await press('> 一行目\n> 二行目', '二行目', 0, () => checkBack(box));
        ok(r.took === false, '引用の途中の行は、前の行と繋がる（既定に任せる）', r.took);

        r = await press('ふつうの段落。', 'ふつう', 3, () => checkBack(box));
        ok(r.took === false, '行頭でなければ、文字を消すキーに戻る', r.took);

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
        // **既定に任せない** ── Chromium は空の見出しを上に作り、`# ` の一行が
        // ファイルに残る（総当たりが捕まえた・2026-09-10）。
        ok(r.took === true, '先頭では受ける（上に空の段落を置く）', r.took);
        ok(r.md === '## 見出し\n', '空の段落は文字に出ない（見出しは見出しのまま）', r.md);
        ok(box.firstElementChild.tagName === 'P' && box.children[1].tagName === 'H2',
           '上に置かれたのは段落で、見出しではない', box.innerHTML);

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
        ok(r.md === '一行目。\n', '段落の中に改行が入る（文字は変わらない）', r.md);

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
        // **材料は図。** コードブロックは 2026-09-20 に触れるようになった
        // （依頼 644）ので、守りが要るのは図と画像だけになった ── 材料を
        // 替えずに残すと、この守りは何も見ていないのに通り続ける。
        const md = '```mermaid\nflowchart LR\n  A --> B\n```\n\nそのあと。';
        let r = await press(md, 'そのあと', 0, () => checkBack(box));
        ok(r.took === true, '図のすぐ下の行頭では、何も起きない（受けて止める）', r.took);
        ok(r.md === md + '\n', '文字は一文字も動いていない', r.md);

        // Delete の裏（総当たりが捕まえた・2026-09-10 ── 既定は図を消して下と繋ぐ）。
        const up = 'そのまえ。\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\nそのあと。';
        r = await press(up, 'そのまえ。', 5, () => checkDel(box));
        ok(r.took === true, '図のすぐ上の行末の Delete は、何も起きない（受けて止める）', r.took);
        ok(r.md === up + '\n', '文字は一文字も動いていない', r.md);
        r = await press(up, 'そのまえ。', 2, () => checkDel(box));
        ok(r.took === false, '行の途中の Delete は、文字を消すキーのまま', r.took);
        r = await press(up, 'そのあと。', 5, () => checkDel(box));
        ok(r.took === false, '隣が図でなければ、既定のまま', r.took);
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
           '文字はそのまま残る（`#` だけが落ちる）', paperToMd(box, ''));

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
        ok(paperToMd(box, '') === mix + '\n', '文字は一文字も動かない', paperToMd(box, ''));
    }

    say('矢印 ── 触れないかたまりを跨ぐ');
    {
        const md = '上の段落。\n\n```mermaid\nflowchart LR\n  A --> B\n```\n\n下の段落。';
        let r = await press(md, '上の段落', 5, () => checkArrow(box, 'down'));
        ok(r.took === true, '図の上の行の末尾で ↓ を押すと、跨ぐ', r.took);
        ok(r.md === md + '\n', '文字は一文字も動かない', r.md);

        r = await press(md, '下の段落', 0, () => checkArrow(box, 'up'));
        ok(r.took === true, '図の下の行の頭で ↑ を押すと、跨ぐ', r.took);

        r = await press(md, '上の段落', 2, () => checkArrow(box, 'down'));
        ok(r.took === false, '行の途中では、ふつうに動く（既定に任せる）', r.took);

        // **コードブロックは跨がない**（依頼 644）── 触れるようになったので、
        // ↓ で入っていける。跨いでしまうと、中に caret を置く手が無くなる。
        const fence = '上の段落。\n\n```\nコード\n```\n\n下の段落。';
        r = await press(fence, '上の段落', 5, () => checkArrow(box, 'down'));
        ok(r.took === false, 'コードブロックは跨がない（中へ入れる）', r.took);

        // 図で始まるノート ── 上に一行足すパスが要る。
        const top = '```mermaid\nflowchart LR\n  A --> B\n```\n\n下の段落。';
        await draw(top);
        headStop(box);
        const first = box.firstElementChild;
        ok(first && first.tagName === 'P' && !first.textContent.trim(),
           '図で始まるノートの上に、降りられる一行が置かれる', first && first.tagName);
        ok(paperToMd(box, '') === top + '\n',
           'その一行は、文字に戻すとき落ちる（ファイルは増えない）', paperToMd(box, ''));

        // **コードブロックで始まるノートには、要らない**（依頼 644）──
        // 中に caret を置けるので、上に空の一行を足す理由が無い。
        await draw('```\nコード\n```\n\n下の段落。');
        headStop(box);
        ok(box.firstElementChild.tagName === 'PRE',
           'コードブロックで始まるノートには、一行を足さない', box.firstElementChild.tagName);

        await draw('ふつうの段落。');
        headStop(box);
        ok(box.firstElementChild.textContent.includes('ふつう'),
           '先頭が触れるものなら、何も置かない', box.firstElementChild.textContent);
    }

    say('記号を外しても、入れ子は失わない（総当たりが捕まえた・2026-09-10）');
    {
        const md = '- ひとつ\n- ふたつ\n  - 入れ子\n- みっつ';
        let r = await press(md, 'ふたつ', 0, () => checkBack(box));
        ok(r.md === '- ひとつ\n\nふたつ\n\n- 入れ子\n- みっつ\n',
           '行頭の Backspace で、入れ子は下の項目の頭に残る', r.md);

        await draw(md);
        caretAt(find('ふたつ'), 0);
        unwrapList(box);
        ok(paperToMd(box, '') === '- ひとつ\n\nふたつ\n\n- 入れ子\n- みっつ\n',
           '「箇条書き」で外しても同じ', paperToMd(box, ''));
    }

    say('段落の中に潜った一覧を、落とさない（Chromium の insertOrderedList の形）');
    {
        // `execCommand` は軽い DOM に無いので、Chromium が作る形を手で置く。
        // **`innerHTML` では作れない** ── 文字の上では `<p><ol>` は読めない形なので、
        // 読む側が `<p></p><ol>…</ol><p></p>` に直してしまう。DOM の手でビルドする。
        const nest = (outer, innerTag, text, lead) => {
            box.innerHTML = '';
            const wrap = document.createElement(outer);
            wrap.dataset.line = '0';
            wrap.dataset.span = '1';
            if (lead) wrap.append(document.createTextNode(lead));
            const list = document.createElement(innerTag);
            const li = document.createElement('li');
            li.textContent = text;
            list.append(li);
            wrap.append(list);
            box.append(wrap);
        };
        await draw('段落。');
        nest('p', 'ol', '最後の段落。');
        ok(paperToMd(box, '') === '1. 最後の段落。\n', '文字に戻すとき、一覧として書く', paperToMd(box, ''));
        tidyLists(box);
        ok(box.firstElementChild.tagName === 'OL', '皮を剥ぐと一覧がかたまりになる', box.innerHTML);
        ok(box.firstElementChild.dataset.line === '0', '元の行のラベルは一覧へ移る', box.firstElementChild.dataset.line);

        nest('p', 'ul', '項目', '文字。');
        ok(paperToMd(box, '') === '文字。\n\n- 項目\n', '文字と一覧が混ざっていても、両方残る', paperToMd(box, ''));
        tidyLists(box);
        ok([...box.children].map((n) => n.tagName).join(',') === 'P,UL', '文字は段落に残り、一覧は後ろへ', box.innerHTML);
    }

    say('項目を見出しにすると、点が外れる（丙）');
    {
        await draw('- ひとつ\n- ふたつ\n- みっつ');
        caretAt(find('ふたつ'), 0);
        // `formatBlock` は軽い DOM に無い ── 手前の「点を外す」だけを見る。
        blockAs(box, 'h2');
        ok(paperToMd(box, '') === '- ひとつ\n\nふたつ\n\n- みっつ\n',
           '見出しにする前に、点が外れて段落になる', paperToMd(box, ''));
        ok(!box.querySelector('li:empty'), '空の項目を残さない', box.innerHTML);
    }

    say('インデントた段落を見出しにすると、インデントは外れる（総当たりの決めごと 9 の筋）');
    {
        await draw('　インデントた段落。');
        caretAt(find('インデントた'), 0);
        blockAs(box, 'h2');
        ok(!box.textContent.startsWith('　'), '画面の上でインデントが消えている', box.textContent);
    }

    say('表のセルでは、一覧にしない');
    {
        const t = '| a | b |\n| --- | --- |\n| 1 | 2 |';
        await draw(t);
        caretAt(box.querySelector('td'), 0);
        ok(blockAs(box, 'ul') === false, '受けずに、何もしない');
        ok(paperToMd(box, '') === t + '\n', '表は一文字も変わらない', paperToMd(box, ''));
        ok(blockAs(box, 'h1') === false, '見出しも同じ');
    }

    say('セルは、caret の一行に（本人が決めた・2026-09-10）');
    {
        const md = '- ひとつ\n- ふたつ\n  - 入れ子\n- みっつ';
        await draw(md);
        caretAt(find('ふたつ'), 0);
        ok(checkLine(box) === true, '項目で受ける');
        ok(paperToMd(box, '') === '- ひとつ\n- [ ] ふたつ\n  - 入れ子\n- みっつ\n', 'その一行だけセルになる', paperToMd(box, ''));
        caretAt(find('ふたつ'), 0);
        checkLine(box);
        ok(paperToMd(box, '') === md + '\n', 'もう一度押すと外れる（点は残る）', paperToMd(box, ''));

        await draw('## 見出し');
        caretAt(find('見出し'), 0);
        checkLine(box);
        ok(paperToMd(box, '') === '- [ ] 見出し\n', '見出しは # が外れてセルに', paperToMd(box, ''));

        await draw('> 一行目\n\n> 二行目');
        caretAt(find('一行目'), 0);
        checkLine(box);
        ok(paperToMd(box, '') === '- [ ] 一行目\n\n> 二行目\n', '引用の行は、引用から出てセルに', paperToMd(box, ''));

        await draw('　インデントた段落。');
        caretAt(find('インデントた'), 0);
        checkLine(box);
        ok(paperToMd(box, '') === '- [ ] インデントた段落。\n', 'インデントは外れる', paperToMd(box, ''));
        // 文字の上では `trim()` が `　` を落とすので、**画面の側**でも外れていることを見る
        // （外さないと、次にビルドし直すまで画面に `　` が残る）。
        ok(!box.querySelector('li').textContent.startsWith('　'), '画面の上でもインデントが消えている', box.querySelector('li').textContent);

        const t = '| a | b |\n| --- | --- |\n| 1 | 2 |';
        await draw(t);
        caretAt(box.querySelector('td'), 0);
        ok(checkLine(box) === false, '表のセルでは何もしない');
        ok(paperToMd(box, '') === t + '\n', '表は変わらない', paperToMd(box, ''));
    }

    say('項目の種類を替える ── その一行だけ（本人が決めた・2026-09-10）');
    {
        await draw('- ひとつ\n- ふたつ\n  - 入れ子\n- みっつ');
        caretAt(find('ふたつ'), 0);
        blockAs(box, 'ol');
        ok(paperToMd(box, '') === '- ひとつ\n\n1. ふたつ\n  - 入れ子\n\n- みっつ\n', '点の項目が番号になり、一覧はそこで割れる', paperToMd(box, ''));

        await draw('- [ ] やること\n- [x] やった');
        caretAt(find('やること'), 0);
        blockAs(box, 'ol');
        ok(paperToMd(box, '') === '1. [ ] やること\n\n- [x] やった\n', 'セルは連れていく', paperToMd(box, ''));

        await draw('1. 一番\n2. 二番');
        caretAt(find('二番'), 0);
        blockAs(box, 'ul');
        ok(paperToMd(box, '') === '1. 一番\n\n- 二番\n', '番号の項目が点になる', paperToMd(box, ''));
    }

    say('項目で「引用」を押すと、段落にしてから引用に（本人が決めた・2026-09-10）');
    {
        await draw('- ひとつ\n- ふたつ\n  - 入れ子\n- みっつ');
        caretAt(find('入れ子'), 0);
        blockAs(box, 'blockquote');
        // `formatBlock` は軽い DOM に無い ── 段落まで出ることを見る。
        ok(paperToMd(box, '') === '- ひとつ\n- ふたつ\n\n入れ子\n\n- みっつ\n', '入れ子の項目でも文字は消えず、外へ出る', paperToMd(box, ''));
    }

    say('空の注記に、打てる一行（本人が決めた・2026-09-10）');
    {
        await draw('> [!NOTE]\n\n次。');
        ok(!box.querySelector('.alert > p:not(.alert-h)'), '描いた直後は中身の行が無い');
        fillAlerts(box);
        ok(!!box.querySelector('.alert > p:not(.alert-h)'), '打てる一行が置かれる');
        ok(paperToMd(box, '') === '> [!NOTE]\n\n次。\n', '空のままなら文字に出ない（札だけ）', paperToMd(box, ''));
    }

    say('項目に貼ると、改行ごとに項目が増える（本人が決めた・2026-09-10）');
    {
        await draw('- [ ] やること\n- [x] やった');
        caretAt(find('やること'), 4);
        pasteLines(box, ['一つめ', '二つめ', '3 つめ']);
        // 一行目の `insertText` は軽い DOM に無いので、増えた項目だけ見る。
        ok(paperToMd(box, '') === '- [ ] やること\n- [ ] 二つめ\n- [ ] 3 つめ\n- [x] やった\n', 'セルの項目にはセルつきで増える', paperToMd(box, ''));
    }

    say('行末の Delete ── 次が記号付きの行なら、何も起きない（本人が決めた・2026-09-11）');
    {
        let r = await press('- [ ] やること\n- [x] やった', 'やること', 4, () => checkDel(box));
        ok(r.took === true, '項目の終わりでは受けて止める', r.took);
        ok(r.md === '- [ ] やること\n- [x] やった\n', '次のセルは消えない', r.md);

        r = await press('段落。\n\n- ひとつ', '段落。', 3, () => checkDel(box));
        ok(r.took === true, '次が一覧なら、段落の終わりでも止める', r.took);

        r = await press('段落。\n\n## 見出し', '段落。', 3, () => checkDel(box));
        ok(r.took === true, '次が見出しでも止める', r.took);

        r = await press('一つめ。\n\n二つめ。', '一つめ。', 4, () => checkDel(box));
        ok(r.took === false, '段落と段落は、既定のまま繋がる', r.took);

        r = await press('一つめ。\n\n二つめ。', '一つめ。', 1, () => checkDel(box));
        ok(r.took === false, '行の途中は、文字を消すキーのまま', r.took);

        const t = '| a | b |\n| --- | --- |\n| 1 | 2 |\n\n下。';
        r = await press(t, '2', 1, () => checkDel(box));
        ok(r.took === true, 'セルの終わりでは、下の段落を吸い込まない', r.took);

        r = await press('> 一行目\n>\n> 二行目', '一行目', 3, () => checkDel(box));
        ok(r.took === false, '引用の中の段落同士は繋がる（既定）', r.took);
    }

    say('空の記号だけの行は、文字を打つまで書かない（本人が決めた・2026-09-11）');
    {
        box.innerHTML = '<h1><br></h1><p>文字。</p>';
        ok(paperToMd(box, '') === '文字。\n', '空の見出しは書かれない', paperToMd(box, ''));
        box.innerHTML = '<ul><li>ひとつ</li><li><br></li></ul>';
        ok(paperToMd(box, '') === '- ひとつ\n', '空の項目は書かれない', paperToMd(box, ''));
        box.innerHTML = '<ul><li><br></li></ul>';
        ok(paperToMd(box, '') === '\n', '項目が一つも無ければ一覧ごと消える', paperToMd(box, ''));
        box.innerHTML = '<ul><li><ul><li>入れ子</li></ul></li></ul>';
        ok(paperToMd(box, '') === '- \n  - 入れ子\n', '文字が無くても入れ子があれば残る（親の行は要る）', paperToMd(box, ''));
    }

    say('引用・注記の途中の Enter は、改行（本人が決めた・2026-09-11）');
    {
        let r = await press('> [!NOTE]\n> 注記の本文', '注記の本文', 2, () => quoteEnter(box));
        ok(r.took === true, '注記の途中で受ける', r.took);
        ok(r.md === '> [!NOTE]\n> 注記\n> の本文\n', '段落に割れず、改行になる', r.md);

        r = await press('> 引用の一行目', '引用の一行目', 3, () => quoteEnter(box));
        ok(r.md === '> 引用の\n> 一行目\n', '引用でも同じ', r.md);

        r = await press('ふつうの段落。', 'ふつう', 2, () => quoteEnter(box));
        ok(r.took === false, '箱の外では受けない', r.took);
    }

    say('入れ子を持つ項目の行末の Delete・済んだセルの行頭の Enter（総当たりが捕まえた・2026-09-11）');
    {
        let r = await press('- ふたつ\n  - 入れ子', 'ふたつ', 3, () => checkDel(box));
        ok(r.took === true, '入れ子を持つ項目の文字の終わりで、受けて止める', r.took);
        ok(r.md === '- ふたつ\n  - 入れ子\n', '入れ子は吸い込まれない', r.md);

        r = await press('- [x] やった', 'やった', 0, () => checkEnter(find('やった')));
        ok(r.took === true, '済んだセルの行頭で受ける', r.took);
        ok(r.md === '- [x] やった\n', 'チェックは文字と一緒に残る（空のセルは書かれない）', r.md);
        ok(box.querySelectorAll('li').length === 2 && box.querySelector('li .box').getAttribute('aria-pressed') === 'false',
           '上に空のセルが置かれる', box.innerHTML);
    }

    say('画面の道具（選び口）は、文字に戻さない');
    {
        await draw('ひとつ。\n\nふたつ。');
        const g = document.createElement('div');
        g.className = 'gadget';
        g.innerHTML = '<b>同じ行を両方で直していました</b><button>こちらを残す</button>';
        box.firstElementChild.before(g);
        ok(paperToMd(box, '') === 'ひとつ。\n\nふたつ。\n', '選び口の文字が本文に混ざらない', paperToMd(box, ''));
    }

    say('注記のラベルは、行として数えない');
    {
        const r = await press('> [!NOTE]\n> 覚えておくこと。', '覚えて', 0, () => checkBack(box));
        ok(r.md === '覚えておくこと。\n', '注記の最初の行も出られる（札ごと消える）', r.md);
    }

    child.stdin.end();
    console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
    process.exit(bad ? 1 : 0);
})();
