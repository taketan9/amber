#!/usr/bin/env node
/* 「表示」の面が、打っても字を失わないか。
 *
 * **この面はいま二つの amber で動く。** 窓は `<div contenteditable>`、
 * iPhone は `WKWebView` の中の同じ `<div contenteditable>` ── 組み方も
 * 書き戻し方も同じ一組（`richBlock` … `inlineToMd`）を使う。書き戻しを
 * もう一組 Swift で書けば、**同じノートが端末によって別の字に保存される**。
 * 失うのはたいてい表と升と図で、気づくのは何回か保存したあと。
 *
 * ここは HTML → Markdown の一方向を見る。組む側（Markdown → HTML）は
 * core の `markdown::to_html` で、`cargo test` が見ている。
 *
 *     node scripts/paper-test.js
 */
'use strict';
const fs = require('fs');
const path = require('path');

// 画面が要るので、軽い DOM を借りる。**無ければ飛ばす** ── 図の試験も
// 台帳もこれに依らないので、ここだけのために依存を増やさない。
let JSDOM;
try {
    ({ JSDOM } = require(path.join(__dirname, '..', 'gui', 'node_modules', 'jsdom')));
} catch {
    console.log('jsdom がありません（gui で npm install すると走ります）── 飛ばします');
    process.exit(0);
}

const src = fs.readFileSync(path.join(__dirname, '..', 'gui', 'renderer.js'), 'utf8');
const from = src.indexOf('function richBlock(');
const to = src.indexOf('/// この窓の「表示」の面を、上の切り出しに繋ぐ薄い包み。');
if (from < 0 || to < 0 || to < from) {
    console.error('gui/renderer.js から「表示」の面を切り出せません'
        + '（`richBlock` から `inlineToMd` までの並びが変わりました）');
    process.exit(2);
}

const dom = new JSDOM('<!doctype html><body><div id="paper"></div></body>');
global.window = dom.window;
global.document = dom.window.document;
global.Node = dom.window.Node;
// caret の居場所を見る道具も渡す ── 升の行の改行はここを見て決める。
global.getSelection = () => dom.window.getSelection();
// eslint-disable-next-line no-eval
(0, eval)(src.slice(from, to));

let bad = 0;
const ok = (yes, what, got) => {
    console.log((yes ? '  ✓ ' : '  ✗ ') + what);
    if (!yes) { bad++; if (got !== undefined) console.log('      ' + JSON.stringify(got)); }
};

const box = document.getElementById('paper');
const round = (html, head = '') => {
    box.innerHTML = html;
    return paperToMd(box, head);
};

console.log('打った字が、そのまま戻るか');
ok(round('<p>ふつうの一行</p>') === 'ふつうの一行\n', '段落');
ok(round('<h2>見出し</h2>') === '## 見出し\n', '見出し');
ok(round('<p><strong>太字</strong>と<em>斜め</em></p>') === '**太字**と*斜め*\n', '飾り');
ok(round('<ul><li>ひとつ</li><li>ふたつ</li></ul>') === '- ひとつ\n- ふたつ\n', '箇条書き');
ok(round('<blockquote><p>引いた言葉</p></blockquote>') === '> 引いた言葉\n', '引用');

console.log('升は、押せる形のまま戻るか');
{
    // **押した升が消える**のは一度やった（`blockToMd` が画面を壊していた）。
    const html = '<ul><li><button type="button" class="box" data-line="3"'
        + ' aria-pressed="true"></button>すんだこと</li>'
        + '<li><button type="button" class="box" data-line="4"'
        + ' aria-pressed="false"></button>まだのこと</li></ul>';
    const md = round(html);
    ok(md === '- [x] すんだこと\n- [ ] まだのこと\n', 'チェックリスト', md);
    // 読んだあとも、升は画面に残っている。
    ok(box.querySelectorAll('.box').length === 2, '読んでも升を壊さない');
}

console.log('戻せないものは、元の字をそのまま返すか');
{
    const md = round('<div class="mermaid" data-md="```mermaid\nflowchart LR\n  A --> B\n```">'
        + '<svg></svg></div>');
    ok(md === '```mermaid\nflowchart LR\n  A --> B\n```\n', '図は元の字', md);
    // **持っていないときは書き戻さない。** 空を返すと、そのかたまりが
    // 黙って消える。
    ok(round('<div class="mermaid"><svg></svg></div>') === null, '元の字が無ければ諦める');
}

console.log('注記と表');
{
    const md = round('<div class="alert warning"><p class="alert-h">注意</p>'
        + '<p>気をつけて</p></div>');
    ok(md === '> [!WARNING]\n> 気をつけて\n', '注記', md);
    const t = round('<table><thead><tr><th>面</th><th>いつ</th></tr></thead>'
        + '<tbody><tr><td>表示</td><td>ふだん</td></tr></tbody></table>');
    ok(t.includes('| 面 | いつ |') && t.includes('| 表示 | ふだん |'), '表', t);
}

console.log('前書きのあるノート');
{
    // 前書きの後ろに一行空ける ── 詰めると、触っただけのノートが
    // 同期先で差分になる。
    ok(round('<p>本文</p>', 'title: あ\n') === '\n本文\n', '一行空ける');
}

console.log('升の行で改行すると');
{
    // **点・番号と同じ押し心地**（次も同じ、空ならそこで降りる）。
    // 既定に任せると升の付かない `<li>` が出て、押した人は升を足した
    // つもりで点が出る。
    const task = (t, on) => '<li class="task"><button type="button" class="box"'
        + ' aria-pressed="' + (on ? 'true' : 'false') + '"></button>' + t + '</li>';
    const caret = (node, at) => {
        const r = document.createRange();
        r.setStart(node, at);
        r.collapse(true);
        const sel = window.getSelection();
        sel.removeAllRanges();
        sel.addRange(r);
    };

    box.innerHTML = '<ul>' + task('ひとつめ') + '</ul>';
    let li = box.querySelector('li');
    caret(li.lastChild, li.lastChild.length);
    ok(checkEnter(li) === true, '字のある升で押すと、受ける');
    ok(box.querySelectorAll('li > .box').length === 2, '次の行にも升が付く');
    ok(paperToMd(box, '') === '- [ ] ひとつめ\n- [ ] \n', '字にすると升が二つ',
       paperToMd(box, ''));

    // 何も書かずにもう一度 ── 一覧から降りる。
    li = box.querySelectorAll('li')[1];
    caret(li, li.childNodes.length);
    ok(checkEnter(li) === true, '空の升で押すと、受ける');
    ok(box.querySelectorAll('li').length === 1, '空の行は消える');
    ok(box.lastElementChild.tagName === 'P', '素の行に降りる',
       box.lastElementChild.tagName);

    // 真ん中で押したら、後ろの字は次の升へ ── 下の行は残る。
    box.innerHTML = '<ul>' + task('あいうえお') + task('のこり') + '</ul>';
    li = box.querySelector('li');
    caret(li.lastChild, 2);
    checkEnter(li);
    ok(paperToMd(box, '') === '- [ ] あい\n- [ ] うえお\n- [ ] のこり\n',
       '後ろの字は次の升へ', paperToMd(box, ''));

    // 真ん中の空の升で降りても、下の行は失わない。
    box.innerHTML = '<ul>' + task('あたま') + task('') + task('おしり') + '</ul>';
    li = box.querySelectorAll('li')[1];
    caret(li, li.childNodes.length);
    checkEnter(li);
    ok(paperToMd(box, '') === '- [ ] あたま\n\n- [ ] おしり\n',
       '真ん中で降りても下は残る', paperToMd(box, ''));

    // 点と番号は既定のまま ── 受けない。
    box.innerHTML = '<ul><li>ただの点</li></ul>';
    li = box.querySelector('li');
    caret(li.firstChild, 3);
    ok(checkEnter(li) === false, '升の無い行は、既定に任せる');
}

console.log('引用と注記から、空の行で降りる');
{
    const caret = (node, at) => {
        const r = document.createRange();
        r.setStart(node, at);
        r.collapse(true);
        const sel = window.getSelection();
        sel.removeAllRanges();
        sel.addRange(r);
    };

    box.innerHTML = '<blockquote><p>引いてきた字</p><p><br></p></blockquote>';
    let line = box.querySelectorAll('blockquote > p')[1];
    caret(line, 0);
    ok(quitEnter(line) === true, '空の行で押すと、受ける');
    ok(box.querySelector('blockquote > p').textContent === '引いてきた字', '引用は残る');
    ok(box.lastElementChild.tagName === 'P' && !box.lastElementChild.closest('blockquote'),
       '引用の外に降りる', box.lastElementChild.outerHTML);

    // 字のある行では、既定のまま（引用が続く）。
    box.innerHTML = '<blockquote><p>引いてきた字</p></blockquote>';
    line = box.querySelector('blockquote > p');
    caret(line.firstChild, 3);
    ok(quitEnter(line) === false, '字のある行は、既定に任せる');

    // 真ん中で降りても、後ろの行は失わない。
    box.innerHTML = '<blockquote><p>あたま</p><p><br></p><p>おしり</p></blockquote>';
    line = box.querySelectorAll('blockquote > p')[1];
    caret(line, 0);
    quitEnter(line);
    ok(box.querySelectorAll('blockquote').length === 2, '引用が二つに割れる');
    ok(paperToMd(box, '') === '> あたま\n\n> おしり\n', '後ろの行は残る', paperToMd(box, ''));

    // 注記も同じ ── 種類の札は割った先にも付く。
    box.innerHTML = '<div class="alert warning"><p class="alert-h">注意</p>'
        + '<p>気をつけて</p><p><br></p><p>あとの行</p></div>';
    line = box.querySelectorAll('.alert > p')[2];
    caret(line, 0);
    quitEnter(line);
    ok(paperToMd(box, '') === '> [!WARNING]\n> 気をつけて\n\n> [!WARNING]\n> あとの行\n',
       '注記も割れて、札が付き直す', paperToMd(box, ''));

    // 種類の札そのものでは受けない。
    box.innerHTML = '<div class="alert note"><p class="alert-h">ノート</p><p>中身</p></div>';
    const label = box.querySelector('.alert-h');
    caret(label, 0);
    ok(quitEnter(label) === false, '種類の札では受けない');
}

console.log('飾りの終わりから、外へ出る');
{
    const caretIn = (node, at) => {
        const r = document.createRange();
        r.setStart(node, at);
        r.collapse(true);
        const sel = window.getSelection();
        sel.removeAllRanges();
        sel.addRange(r);
    };
    // **飾ったのは選んだ字で、これから打つ字ではない。**
    box.innerHTML = '<p><b>ふとい</b>ふつう</p>';
    let b = box.querySelector('b');
    caretIn(b.firstChild, 3);
    ok(outOfDress() === true, '飾りの終わりなら、出す');
    ok(window.getSelection().anchorNode === box.querySelector('p'), '出た先は段落の中',
       window.getSelection().anchorNode.nodeName);

    // 途中なら出さない ── そこは中の字。
    caretIn(b.firstChild, 1);
    ok(outOfDress() === false, '飾りの途中では、出さない');

    // 飾りの外なら、そもそも関わらない。
    caretIn(box.querySelector('p').lastChild, 2);
    ok(outOfDress() === false, '飾りの外では、何もしない');

    // 二重の飾りは、いちばん外まで出る。
    box.innerHTML = '<p><b><i>ふとくて斜め</i></b>あと</p>';
    caretIn(box.querySelector('i').firstChild, 6);
    outOfDress();
    ok(window.getSelection().anchorNode === box.querySelector('p'),
       '二重でも、いちばん外まで出る', window.getSelection().anchorNode.nodeName);
}

console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
process.exit(bad ? 1 : 0);
