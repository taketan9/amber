#!/usr/bin/env node
/* よそから来た HTML が、ノートの字になるか（依頼 421）。
 *
 *     ブラウザの clipboard の text/html ──▶ webToMd（js）──▶ .md の字
 *
 * **二本目の変換器を書かない**のがこの機能の芯なので、ここで見張るのも
 * そこ ── `webToMd` は「均す」だけで、字にするのは面の書き戻しと同じ
 * `blockToMd`。均しが足りなければ、見出しも一覧も一行の字になって出る。
 *
 * 見るのは七つ:
 *   一。要らない札（script、nav、隠してあるもの）が落ちること
 *   二。入れ物（div、section）がほどけて、中の形が残ること
 *   三。相対の行き先が、絶対の道になること
 *   四。絵が `![](…)` として残り、数取りの 1px は落ちること
 *   五。枠（`<pre>`）が ``` で囲まれること
 *   六。ページの題から、サイト名の尻尾が落ちること（`clipTitle`）
 *   七。本文らしいところだけ採れること（`bestPart`）
 *
 * **六と七も、切り出しの中で見る。** ここは切り出し（`richBlock` から
 * 「薄い包み」まで）だけを読んで動かしているので、`clipTitle` と
 * `bestPart` が切り出しの外へ出た日にはこの検査が落ちる ── 電話は
 * この二つを窓と同じ一組から呼んでいて、外へ出た瞬間に電話だけ
 * 取り込めなくなる（実際にそうなった）。
 *
 *     node scripts/web-test.js
 */
'use strict';
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');

let JSDOM;
try {
    ({ JSDOM } = require(path.join(root, 'gui', 'node_modules', 'jsdom')));
} catch {
    console.log('jsdom がありません（gui で npm install すると走ります）── 飛ばします');
    process.exit(0);
}

const src = fs.readFileSync(path.join(root, 'gui', 'renderer.js'), 'utf8');
const from = src.indexOf('function richBlock(');
const to = src.indexOf('/// この窓の「表示」の面を、上の切り出しに繋ぐ薄い包み。');
if (from < 0 || to < 0 || to < from) {
    console.error('gui/renderer.js から「表示」の面を切り出せません');
    process.exit(2);
}
const dom = new JSDOM('<!doctype html><body></body>');
global.window = dom.window;
global.document = dom.window.document;
global.Node = dom.window.Node;
global.DOMParser = dom.window.DOMParser;
global.URL = dom.window.URL;
// eslint-disable-next-line no-eval
(0, eval)(src.slice(from, to));

const BASE = 'https://example.com/記事/段取り';

/// [名前, 入れる HTML, 出てほしい .md]
const CASES = [
    ['見出しと段落',
        '<h2>段取り</h2><p>本文です。</p>',
        '## 段取り\n\n本文です。'],

    ['太字と斜体とリンク',
        '<p><b>朝</b>に<i>必ず</i><a href="/次">次</a>を見る</p>',
        '**朝**に*必ず*[次](https://example.com/次)を見る'],

    ['入れ物をほどく ── div の中の形が残る',
        '<div><div><h3>朝</h3><ul><li>一つめ</li><li>二つめ</li></ul></div></div>',
        '### 朝\n\n- 一つめ\n- 二つめ'],

    ['要らない札は落ちる',
        '<nav>案内</nav><p>本文</p><script>alert(1)</script><style>p{}</style><footer>ここは footer</footer>',
        '本文'],

    ['隠してあるものは、読む人に見えていない',
        '<p>見える</p><p hidden>隠し</p><p aria-hidden="true">読み上げ用</p>',
        '見える'],

    ['相対の行き先が、絶対の道になる',
        '<p><a href="../ほか">ほか</a></p>',
        '[ほか](https://example.com/ほか)'],

    ['行き先の無いリンクは、字だけ残る',
        '<p><a>ただの字</a>と<a href="#中">中へ</a></p>',
        'ただの字と中へ'],

    ['絵はリンクのまま残る',
        '<p><img src="/絵/a.png" alt="図"></p>',
        '![図](https://example.com/絵/a.png)'],

    ['数取りの 1px と data: は落ちる',
        '<p>本文<img src="/t.gif" width="1" height="1"><img src="data:image/gif;base64,R0lGOD"></p>',
        '本文'],

    ['枠は囲まれる',
        '<pre><code class="language-rust">fn main() {}\n</code></pre>',
        '```rust\nfn main() {}\n```'],

    ['枠の中に ``` があっても、途中で閉じない',
        '<pre><code>これは ``` です</code></pre>',
        '````\nこれは ``` です\n````'],

    ['引用と表',
        '<blockquote><p>言われたこと</p></blockquote>'
        + '<table><thead><tr><th>朝</th><th>夕</th></tr></thead>'
        + '<tbody><tr><td>掃除</td><td>片づけ</td></tr></tbody></table>',
        '> 言われたこと\n\n| 朝 | 夕 |\n| --- | --- |\n| 掃除 | 片づけ |'],

    ['番号つきの一覧',
        '<ol><li>まず</li><li>つぎ</li></ol>',
        '1. まず\n2. つぎ'],

    ['空の入れ物だらけでも、隙間だらけにならない',
        '<div></div><p>本文</p><div><span></span></div><p>つづき</p><div></div>',
        '本文\n\nつづき'],

    ['何も無ければ、何も返さない',
        '<div><script>x</script></div>',
        ''],
];

/// [名前, 入れる HTML まるごと, 出てほしい題]
const TITLES = [
    ['サイト名の尻尾を落とす',
        '<title>段取りの決め方 | example</title>', '段取りの決め方'],
    ['全角の縦棒でも落とす',
        '<title>段取りの決め方 ｜ example</title>', '段取りの決め方'],
    ['ダッシュでも落とす',
        '<title>段取りの決め方 — example</title>', '段取りの決め方'],
    ['尻尾が無ければ、そのまま',
        '<title>段取りの決め方</title>', '段取りの決め方'],
    ['実体参照はほどく',
        '<title>朝 &amp; 夜</title>', '朝 & 夜'],
    ['題が無ければ、何も返さない', '<p>本文</p>', ''],
];

/// [名前, 入れる HTML まるごと, 本文として出てほしい .md]
const PARTS = [
    ['article が名乗っていれば、それを信じる',
        '<nav>案内</nav><header><h1>ここは飾り</h1></header>'
        + '<article><h1>本題</h1><p>' + 'あ'.repeat(300) + '</p></article>'
        + '<footer>足</footer>',
        '# 本題\n\n' + 'あ'.repeat(300)],
    ['名乗りが無ければ、いちばん字の多いかたまり',
        '<div><p>案内</p></div><div><h2>本題</h2><p>' + 'い'.repeat(200)
        + '</p><p>' + 'ろ'.repeat(200) + '</p></div>',
        '## 本題\n\n' + 'い'.repeat(200) + '\n\n' + 'ろ'.repeat(200)],
    ['長い台本は、字の量で本文に勝てない',
        '<script type="application/ld+json">' + 'x'.repeat(2000)
        + '</script><div><h2>本題</h2><p>' + 'は'.repeat(200)
        + '</p><p>' + 'に'.repeat(200) + '</p></div>',
        '## 本題\n\n' + 'は'.repeat(200) + '\n\n' + 'に'.repeat(200)],
];

// **切り出しが caret の憶えを持っていること**（依頼 461）。
//
// 電話の「表示」の面は、絵文字を入れる前にここへ caret を戻す ── 窓の
// 走査は実際に押して見張っているが、電話は押せないので、**一組が
// 切り出しの中に居ること**だけでも見ておく（`clipTitle` が外へ出て
// 電話だけ取り込めなくなった、と同じ形を防ぐ）。
let bad = 0;
for (const name of ['markCaret', 'caretBack']) {
    if (typeof globalThis[name] !== 'function') {
        bad += 1;
        console.log('✗ 切り出しに ' + name + ' がありません（電話が caret を戻せません）');
    }
}
for (const [name, html, want] of CASES) {
    const got = webToMd(html, BASE);
    if (got === want) continue;
    bad += 1;
    console.log('✗ ' + name);
    console.log('  ほしい: ' + JSON.stringify(want));
    console.log('  出た　: ' + JSON.stringify(got));
}
for (const [name, html, want] of TITLES) {
    const got = clipTitle(html);
    if (got === want) continue;
    bad += 1;
    console.log('✗ 題：' + name);
    console.log('  ほしい: ' + JSON.stringify(want));
    console.log('  出た　: ' + JSON.stringify(got));
}
for (const [name, html, want] of PARTS) {
    const got = webToMd(bestPart(html), BASE);
    if (got === want) continue;
    bad += 1;
    console.log('✗ 本文：' + name);
    console.log('  ほしい: ' + JSON.stringify(want));
    console.log('  出た　: ' + JSON.stringify(got));
}

console.log('');
if (bad) {
    console.log((CASES.length + TITLES.length + PARTS.length)
        + ' 件中 ' + bad + ' 件おかしいです');
    process.exit(1);
}
console.log('ぜんぶ通りました');
