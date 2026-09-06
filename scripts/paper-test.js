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
const to = src.indexOf('/// 打ったら、落ち着いてから書き戻す。');
if (from < 0 || to < 0 || to < from) {
    console.error('gui/renderer.js から「表示」の面を切り出せません'
        + '（`richBlock` から `inlineToMd` までの並びが変わりました）');
    process.exit(2);
}

const dom = new JSDOM('<!doctype html><body><div id="paper"></div></body>');
global.window = dom.window;
global.document = dom.window.document;
global.Node = dom.window.Node;
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

console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
process.exit(bad ? 1 : 0);
