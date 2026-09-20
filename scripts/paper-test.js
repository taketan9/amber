#!/usr/bin/env node
/* 「表示」画面が、打っても文字を失わないか。
 *
 * **この画面はいま二つの amber で動く。** デスクトップ版は `<div contenteditable>`、
 * iPhone は `WKWebView` の中の同じ `<div contenteditable>` ── 組み方も
 * 書き戻し方も同じ一組（`richBlock` … `inlineToMd`）を使う。書き戻しを
 * もう一組 Swift で書けば、**同じノートが端末によって別の文字に保存される**。
 * 失うのはたいてい表とセルと図で、気づくのは何回か保存したあと。
 *
 * ここは HTML → Markdown の一方向を見る。ビルドする側（Markdown → HTML）は
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
// **`oneCell` から** ── 貼られたものを均すところ（`webClean`）まで、
// 一続きで切り出す。Excel のセルをどう扱うかも、この画面の判断のうち。
const from = src.indexOf('function oneCell(');
const to = src.indexOf('/// このデスクトップ版の「表示」画面を、上の切り出しに繋ぐ薄い包み。');
if (from < 0 || to < 0 || to < from) {
    console.error('gui/renderer.js から「表示」画面を切り出せません'
        + '（`richBlock` から `inlineToMd` までの並びが変わりました）');
    process.exit(2);
}

const dom = new JSDOM('<!doctype html><body><div id="paper"></div></body>');
global.window = dom.window;
global.document = dom.window.document;
global.Node = dom.window.Node;
// よそから来た HTML を掃除するところ（`webClean`）も見る。
global.DOMParser = dom.window.DOMParser;
// caret の居場所を見る道具も渡す ── セルの行の改行はここを見て決める。
global.getSelection = () => dom.window.getSelection();
// eslint-disable-next-line no-eval
(0, eval)(src.slice(from, to));
// **貼られたものが絵そのものか**を見る一本は、画面の外（受け口の隣）に居る。
// eslint-disable-next-line no-eval
(0, eval)(src.slice(src.indexOf('function justAPicture(data)'),
                    src.indexOf("document.addEventListener('paste'",
                                src.indexOf('function justAPicture(data)'))));

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

console.log('打った文字が、そのまま戻るか');
ok(round('<p>ふつうの一行</p>') === 'ふつうの一行\n', '段落');
ok(round('<h2>見出し</h2>') === '## 見出し\n', '見出し');
ok(round('<p><strong>太字</strong>と<em>斜め</em></p>') === '**太字**と*斜め*\n', '書式');
ok(round('<ul><li>ひとつ</li><li>ふたつ</li></ul>') === '- ひとつ\n- ふたつ\n', '箇条書き');
ok(round('<blockquote><p>引いた言葉</p></blockquote>') === '> 引いた言葉\n', '引用');

console.log('セルは、押せる形のまま戻るか');
{
    // **押したセルが消える**のは一度やった（`blockToMd` が画面を壊していた）。
    const html = '<ul><li><button type="button" class="box" data-line="3"'
        + ' aria-pressed="true"></button>すんだこと</li>'
        + '<li><button type="button" class="box" data-line="4"'
        + ' aria-pressed="false"></button>まだのこと</li></ul>';
    const md = round(html);
    ok(md === '- [x] すんだこと\n- [ ] まだのこと\n', 'チェックリスト', md);
    // 読んだあとも、セルは画面に残っている。
    ok(box.querySelectorAll('.box').length === 2, '読んでもセルを壊さない');
}

console.log('戻せないものは、元の文字をそのまま返すか');
{
    const md = round('<div class="mermaid" data-md="```mermaid\nflowchart LR\n  A --> B\n```">'
        + '<svg></svg></div>');
    ok(md === '```mermaid\nflowchart LR\n  A --> B\n```\n', '図は元の文字', md);
    // **持っていないときは書き戻さない。** 空を返すと、そのかたまりが
    // 黙って消える。
    ok(round('<div class="mermaid"><svg></svg></div>') === null, '元の文字が無ければ諦める');
}

console.log('注記と表');
{
    const md = round('<div class="alert warning"><p class="alert-h">注意</p>'
        + '<p>気をつけて</p></div>');
    ok(md === '> [!WARNING]\n> 気をつけて\n', '注記', md);
    const t = round('<table><thead><tr><th>画面</th><th>いつ</th></tr></thead>'
        + '<tbody><tr><td>表示</td><td>ふだん</td></tr></tbody></table>');
    ok(t.includes('| 画面 | いつ |') && t.includes('| 表示 | ふだん |'), '表', t);
}

console.log('前書きのあるノート');
{
    // 前書きの後ろに一行空ける ── 詰めると、触っただけのノートが
    // 同期先で差分になる。
    ok(round('<p>本文</p>', 'title: あ\n') === '\n本文\n', '一行空ける');
}

console.log('セルの行で改行すると');
{
    // **点・番号と同じ押し心地**（次も同じ、空ならそこで降りる）。
    // 既定に任せるとセルの付かない `<li>` が出て、押した人はセルを足した
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
    ok(checkEnter(li) === true, '文字のあるセルで押すと、受ける');
    ok(box.querySelectorAll('li > .box').length === 2, '次の行にもセルが付く');
    // **空のセルは、文字を打つまでファイルに出さない**（本人が決めた・2026-09-11・
    // ネットワークの決めごと 11）── 画面には二つ見えているが、文字は一つ。
    ok(paperToMd(box, '') === '- [ ] ひとつめ\n', '文字にすると、空のセルはまだ書かれない',
       paperToMd(box, ''));
    {
        const second = box.querySelectorAll('li')[1];
        const typed = document.createTextNode('ふたつめ');
        second.append(typed);
        ok(paperToMd(box, '') === '- [ ] ひとつめ\n- [ ] ふたつめ\n', '文字を打った瞬間に、二つめが書かれる',
           paperToMd(box, ''));
        typed.remove();     // 続きの試し（空のセルで降りる）は、空のまま
    }

    // 何も書かずにもう一度 ── 一覧から降りる。
    li = box.querySelectorAll('li')[1];
    caret(li, li.childNodes.length);
    ok(checkEnter(li) === true, '空のセルで押すと、受ける');
    ok(box.querySelectorAll('li').length === 1, '空の行は消える');
    ok(box.lastElementChild.tagName === 'P', '素の行に降りる',
       box.lastElementChild.tagName);

    // 真ん中で押したら、後ろの文字は次のセルへ ── 下の行は残る。
    box.innerHTML = '<ul>' + task('あいうえお') + task('のこり') + '</ul>';
    li = box.querySelector('li');
    caret(li.lastChild, 2);
    checkEnter(li);
    ok(paperToMd(box, '') === '- [ ] あい\n- [ ] うえお\n- [ ] のこり\n',
       '後ろの文字は次のセルへ', paperToMd(box, ''));

    // 真ん中の空のセルで降りても、下の行は失わない。
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
    ok(checkEnter(li) === false, 'セルの無い行は、既定に任せる');
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

    box.innerHTML = '<blockquote><p>引いてきた文字</p><p><br></p></blockquote>';
    let line = box.querySelectorAll('blockquote > p')[1];
    caret(line, 0);
    ok(quitEnter(line) === true, '空の行で押すと、受ける');
    ok(box.querySelector('blockquote > p').textContent === '引いてきた文字', '引用は残る');
    ok(box.lastElementChild.tagName === 'P' && !box.lastElementChild.closest('blockquote'),
       '引用の外に降りる', box.lastElementChild.outerHTML);

    // 文字のある行では、既定のまま（引用が続く）。
    box.innerHTML = '<blockquote><p>引いてきた文字</p></blockquote>';
    line = box.querySelector('blockquote > p');
    caret(line.firstChild, 3);
    ok(quitEnter(line) === false, '文字のある行は、既定に任せる');

    // 真ん中で降りても、後ろの行は失わない。
    box.innerHTML = '<blockquote><p>あたま</p><p><br></p><p>おしり</p></blockquote>';
    line = box.querySelectorAll('blockquote > p')[1];
    caret(line, 0);
    quitEnter(line);
    ok(box.querySelectorAll('blockquote').length === 2, '引用が二つに割れる');
    ok(paperToMd(box, '') === '> あたま\n\n> おしり\n', '後ろの行は残る', paperToMd(box, ''));

    // 注記も同じ ── 種類のラベルは割った先にも付く。
    box.innerHTML = '<div class="alert warning"><p class="alert-h">注意</p>'
        + '<p>気をつけて</p><p><br></p><p>あとの行</p></div>';
    line = box.querySelectorAll('.alert > p')[2];
    caret(line, 0);
    quitEnter(line);
    ok(paperToMd(box, '') === '> [!WARNING]\n> 気をつけて\n\n> [!WARNING]\n> あとの行\n',
       '注記も割れて、ラベルが付き直す', paperToMd(box, ''));

    // 種類の札そのものでは受けない。
    box.innerHTML = '<div class="alert note"><p class="alert-h">ノート</p><p>中身</p></div>';
    const label = box.querySelector('.alert-h');
    caret(label, 0);
    ok(quitEnter(label) === false, '種類のラベルでは受けない');
}

console.log('書式の終わりから、外へ出る');
{
    const caretIn = (node, at) => {
        const r = document.createRange();
        r.setStart(node, at);
        r.collapse(true);
        const sel = window.getSelection();
        sel.removeAllRanges();
        sel.addRange(r);
    };
    // **飾ったのは選んだ文字で、これから打つ文字ではない。**
    box.innerHTML = '<p><b>ふとい</b>ふつう</p>';
    let b = box.querySelector('b');
    caretIn(b.firstChild, 3);
    ok(outOfDress() === true, '書式の終わりなら、出す');
    ok(window.getSelection().anchorNode === box.querySelector('p'), '出た先は段落の中',
       window.getSelection().anchorNode.nodeName);

    // 途中なら出さない ── そこは中の文字。
    caretIn(b.firstChild, 1);
    ok(outOfDress() === false, '書式の途中では、出さない');

    // 書式の外なら、そもそも関わらない。
    caretIn(box.querySelector('p').lastChild, 2);
    ok(outOfDress() === false, '書式の外では、何もしない');

    // 二重の書式は、いちばん外まで出る。
    box.innerHTML = '<p><b><i>ふとくて斜め</i></b>あと</p>';
    caretIn(box.querySelector('i').firstChild, 6);
    outOfDress();
    ok(window.getSelection().anchorNode === box.querySelector('p'),
       '二重でも、いちばん外まで出る', window.getSelection().anchorNode.nodeName);
}

console.log('図の「元の文字」は、行番号がずれても作り直さない');
{
    // **これが図を丸ごと消した。**
    //
    // ビルドし直したときの行番号で本文を切って持たせていたが、`syncRead` は
    // caret を飛ばさないためにビルドし直さずに保存する ── 上に一行足した次の
    // 保存から番号がずれ、切り直した「元の文字」が別の場所の文字になり、その
    // 次の保存でそれが図の場所へ書き戻された。キーボードでは消していないのに
    // 図が消えるので、原因が画面のどこにも出ない。
    const was = '一行目。\n\n```mermaid\nflowchart LR\n  A --> B\n```\n';
    box.innerHTML = '<pre class="mermaid" data-line="2" data-span="4">A --&gt; B</pre>';
    armPaper(box, was, true);
    const first = box.firstElementChild.dataset.md;
    ok(first === '```mermaid\nflowchart LR\n  A --> B\n```',
       '初めは、行番号のところの文字を持つ', first);

    // 上に一行増えた。番号はビルドし直すまで古いまま。
    const now = '一行目。\n足した行。\n\n```mermaid\nflowchart LR\n  A --> B\n```\n';
    armPaper(box, now, true);
    ok(box.firstElementChild.dataset.md === first,
       'ずれても、持っている文字は変わらない', box.firstElementChild.dataset.md);
}

/* **よそから来た書式は、持ち込まない**（依頼 616・本人「やや白いハイライト
 * というかマーカーがついた文字で入力される」）。
 *
 * Excel はセルに `style="background:white;color:black"` を付けて寄こす ──
 * 琥珀の紙の上では、その白がマーカーを引いたように見える。
 */
{
    const one = '<html xmlns:x="urn:schemas-microsoft-com:office:excel"><body><table>'
        + '<tr><td style="background:white;color:black" x:str>売上</td></tr></table></body></html>';
    const many = '<html><body><table>'
        + '<tr><td style="background:white">名前</td><td style="background:white">値</td></tr>'
        + '<tr><td style="background:white">ＣＰＵ</td><td style="background:white">8</td></tr>'
        + '</table></body></html>';
    const cleanOne = webClean(one, '');
    const cleanMany = webClean(many, '');
    ok(!/style=/.test(cleanMany.innerHTML), 'Excel の書式を落とす', cleanMany.innerHTML.slice(0, 120));
    ok(!/background/i.test(cleanMany.innerHTML), '白いマーカーを持ち込まない');
    ok(/<td>名前<\/td>/.test(cleanMany.innerHTML), 'セルの文字は残る', cleanMany.innerHTML.slice(0, 120));
    // **色だけは残す** ── ambər の記法（`note::first_color` が読む形）。
    const colored = webClean('<p><span style="color:#D9822B;background:white">橙</span></p>', '');
    ok(/style="color:#D9822B"/i.test(colored.innerHTML), '色は残す', colored.innerHTML);
    ok(!/background/i.test(colored.innerHTML), '色以外は落とす', colored.innerHTML);
    // **黒は色ではない。** 既定の文字に付いてくるので、残すとノートじゅうが span になる。
    const black = webClean('<p><span style="color:black">ふつう</span></p>', '');
    ok(!/style=/.test(black.innerHTML), '黒は色として残さない', black.innerHTML);
    const black6 = webClean('<p><span style="color:#000000">ふつう</span></p>', '');
    ok(!/style=/.test(black6.innerHTML), '#000000 も残さない', black6.innerHTML);
    // **行き先と画像は残す** ── 落とすとリンクが文字になる。
    const link = webClean('<p><a href="https://x/a" target="_blank" class="u">文字</a></p>', '');
    ok(/href="https:\/\/x\/a"/.test(link.innerHTML), '行き先は残す', link.innerHTML);
    ok(!/target=|class=/.test(link.innerHTML), 'それ以外は落とす', link.innerHTML);
    // **枠の言語は書式ではない** ── 落とすと、貼った枠から言語が消える。
    const code = webClean('<pre><code class="hljs language-rust">fn main() {}</code></pre>', '');
    ok(/class="language-rust"/.test(code.innerHTML), '枠の言語は残す', code.innerHTML);
    ok(!/hljs/.test(code.innerHTML), 'ほかの class は落とす', code.innerHTML);

    // **セルひとつは、表ではない**（本人「不思議なところで改行する」）。
    ok(oneCell(cleanOne) === '売上', 'セルひとつは文字だけにする', oneCell(cleanOne));
    ok(oneCell(cleanMany) === null, 'セルが二つ以上なら、表のまま', oneCell(cleanMany));
    ok(oneCell(webClean('<p>ただの段</p>', '')) === null, '表でなければ、触らない');

    // **Excel は絵も一緒に載せてくる**（本人「絵の扱いになっている」）。
    // 見分けるのは文字の有無 ── 画面を撮った回だけが、文字を一つも載せない。
    const clip = (html, plain) => ({ getData: (t) => (t === 'text/html' ? html : (t === 'text/plain' ? plain : '')) });
    ok(justAPicture(clip('', '')) === true, '画面写真は絵のまま');
    ok(justAPicture(clip('<img src="https://x/a.png">', '')) === true,
       'ウェブの画像も絵のまま（文字が無い）');
    ok(justAPicture(clip(many, '名前\t値\nＣＰＵ\t8\n')) === false,
       'Excel の範囲は絵ではない（文字が載っている）');
    ok(justAPicture(clip(one, '売上\n')) === false, 'セルひとつも絵ではない');
    ok(justAPicture(clip('<p>文字のある HTML</p>', '')) === false,
       '文字のある HTML は絵ではない（文字だけ別の欄に載らない道具もある）');
    // **文字だけ載せてくる道具もある**（HTML を作らない表計算・端末から写した文字）。
    // そこを見ないと、絵と一緒に来た文字がぜんぶ画面写真になる。
    ok(justAPicture(clip('', 'ただの文字')) === false, '文字だけでも絵ではない');

    // **枠の中の文字は、元の文字から**（依頼 614）── 画面の枠は色が付いたあとの
    // 姿で、改行が `<br>`、空白が `&nbsp;` になっている。
    const pre = document.createElement('pre');
    pre.dataset.md = '```python\ndef 短い():\n    return 1\n```';
    pre.innerHTML = '<code>def&nbsp;短い():<br>&nbsp;&nbsp;&nbsp;&nbsp;return 1</code>';
    ok(codeOf(pre) === 'def 短い():\n    return 1', '囲みを外して、改行のまま返す', codeOf(pre));
    const pasted = document.createElement('pre');
    pasted.innerHTML = '<code>a\u00a0b\n</code>';
    ok(codeOf(pasted) === 'a b', '元の文字が無ければ、文字から拾って &nbsp; を戻す', codeOf(pasted));
}

console.log('表示画面で直したコードブロックが、ファイルの形で戻るか');
{
    // **色を付けたあとの枠は、改行が `<br>`、空白が `&nbsp;`**（`codeOf` と
    // 同じ話）。そのまま `textContent` で拾うと、直した枠が一行に潰れて、
    // 空白がぜんぶ U+00A0 で保存される ── 本物のアプリで実際に出た
    // （2026-09-20・依頼 644）。
    const pre = document.createElement('pre');
    pre.innerHTML = '<code class="language-rust">fn&nbsp;main()&nbsp;{<br>'
        + '&nbsp;&nbsp;&nbsp;&nbsp;let&nbsp;x&nbsp;=&nbsp;1;<br>}</code>';
    ok(fenceText(pre.querySelector('code')) === 'fn main() {\n    let x = 1;\n}',
       '改行は改行のまま、空白はふつうの空白で返る', fenceText(pre.querySelector('code')));

    // 直した枠（元の文字と食い違う）は、組み直して返す。
    pre.dataset.md = '```rust\nfn main() {\n    let x = 1;\n}\n```';
    pre.innerHTML = '<code class="language-rust">fn&nbsp;main()&nbsp;{<br>'
        + '&nbsp;&nbsp;&nbsp;&nbsp;let&nbsp;xy&nbsp;=&nbsp;1;<br>}</code>';
    ok(round(pre.outerHTML) === '```rust\nfn main() {\n    let xy = 1;\n}\n```\n',
       '打ったところだけ変わって、行も空白もそのまま', round(pre.outerHTML));

    // **触っていない枠は、読んだときの文字のまま**（`~~~` で書いた枠が
    // ``` で戻ると、触ってもいないノートが同期先で差分になる）。
    const same = document.createElement('pre');
    same.dataset.md = '~~~rust\nfn main() {}\n~~~';
    same.innerHTML = '<code class="language-rust">fn&nbsp;main()&nbsp;{}</code>';
    ok(round(same.outerHTML) === '~~~rust\nfn main() {}\n~~~\n',
       '触っていない枠は、囲みの形まで元のまま', round(same.outerHTML));
}

console.log('コードブロックの中の Enter は、枠を割らずに改行するか');
{
    // **既定は枠を二つに割る** ── 二行目を書いたつもりが、枠が二つになって
    // 出る（本物のアプリで確かめた・2026-09-20・依頼 644）。
    box.innerHTML = '<pre><code class="language-js">const a = 1;</code></pre>';
    const code = box.querySelector('code');
    const r = document.createRange();
    r.setStart(code.firstChild, code.firstChild.data.length);
    r.collapse(true);
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
    ok(checkFenceReturn(box) === true, '枠の中では受ける');
    ok(box.querySelectorAll('pre').length === 1, '枠は一つのまま',
       box.querySelectorAll('pre').length);
    ok(fenceText(code) === 'const a = 1;', '末尾の詰め物は文字に出ない', fenceText(code));
    // **行末で押したときは、改行を二つ。** 一つだけだと caret の行き先が
    // 無く、押したのに何も起きていないように見える（`<pre>` の末尾の改行）。
    ok(code.textContent.endsWith('\n\n'), '行末で押したら、立てる行を一つ用意する',
       JSON.stringify(code.textContent));

    // 枠の外では受けない ── ふつうの段落の Enter は、これまでどおり。
    box.innerHTML = '<p>ふつうの段落</p>';
    const p = box.querySelector('p');
    const r2 = document.createRange();
    r2.setStart(p.firstChild, 3);
    r2.collapse(true);
    sel.removeAllRanges();
    sel.addRange(r2);
    ok(checkFenceReturn(box) === false, '枠の外では受けない');
}

console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
process.exit(bad ? 1 : 0);
