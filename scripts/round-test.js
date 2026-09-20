#!/usr/bin/env node
/* 文字 → 画面 → 文字 が、元に戻るか。**総当たりで。**
 *
 *     もとの .md ──▶ to_html（core）──▶ 画面 ──▶ paperToMd（js）──▶ 戻った .md
 *                                                      ↑
 *                                     ここが もとの .md と同じか
 *
 * `paper-test.js` は画面 → 文字の一方向を、**手で書いた HTML**から見ている。
 * こちらは**本物の往復**を見る ── ビルドする側は core（Rust）、戻す側はデスクトップ版と
 * iPhone が共有している切り出し（js）。二つの言語をまたぐので、エンジンの
 * 実行ファイルが要る。
 *
 * **なぜ要るのか。** 2026-09-08 に実物で踏んだ ── 「表示」画面で一文字
 * 打っただけで、打っていない行が五種類書き換わった（段落の改行が空白に、
 * `*` の点が `-` に、`1. 1.` が `1. 2.` に、引用の二行が一行に、行頭の
 * 全角空白が消える）。同期しているフォルダなら、それが全部むこうへ差分と
 * して飛ぶ。**一つずつの形は `paper-test.js` が通っていた。落ちたのは
 * 往復と、組み合わせ。**
 *
 * 見方は三つ（`PAPER.ja.md` 二章）:
 *   強い一致 ── 文字がそのまま戻る。ここを目指す
 *   弱い一致 ── 末尾の空行の数だけ違う。許す（`sameNote` と同じ考え）
 *   落第     ── それ以外。「たぶん同じ意味」で通さない
 *
 * `null`（戻せない）は落第にしない ── 「戻せないなら書かない」が正しい。
 * ただし**どの形で出たかは数えて出す**。
 *
 * **決めて丸めているものは、丸めた文字で書いておく。** 表の空のセルは全角
 * 空白で埋まって戻る（依頼 94・190）── ここに元の文字を書くと、決めごとが
 * 落第として出続け、そのうち誰も見なくなる。
 *
 *     node scripts/round-test.js          # 落第だけ出す
 *     node scripts/round-test.js --all    # ぜんぶ出す
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

// ビルドする側は core。**実行ファイルが無ければ飛ばす** ── この試験のためだけに
// `cargo build` を強いない（`paper-test.js` が jsdom を強いないのと同じ）。
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

/* ── 画面（切り出しをそのまま動かす） ── */

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
// **画像の掛け替えも、本物を通す。** `drawRead` はビルドしたあとに
// `findPictures` を呼び、`<img>` を `<figure>` に包んでラベルを移す
// （`keepMark`）── これを通さずに回すと「画像が丸ごと消える」という
// 嘘の落第が出る（実際に出して、実機で確かめて分かった）。
// 切り出しの外にあるので、名前で抜いて同じ環境を用意する。
const grab = (head) => {
    const at = src.indexOf(head);
    if (at < 0) { console.error(head + ' が見つかりません'); process.exit(2); }
    const end = src.indexOf('\n}\n', at);
    return src.slice(at, end + 3);
};
// eslint-disable-next-line no-eval
(0, eval)(src.slice(from, to)
    + grab('function findPictures(')
    // デスクトップ版の持ちもの ── 画像の在りかをビルドするのに要るぶんだけ。**中身は見ない**
    // ので、パスの組み立ては本物でなくてよい（`fileURL` は win-test が見る）。
    + 'function fileURL(at) { return "file://" + at; }\n'
    + 'const dirOf = (at) => String(at || "").replace(/[^/\\\\]*$/, "");\n'
    + 'const escapeHtml = (s) => String(s);\n');
const box = document.getElementById('paper');
// `findPictures` は `el('read')` と `state.open` を見る。**同じ箱を渡す。**
global.el = () => box;
global.state = { open: { path: '/notes/試し.md' } };
global.window.amber = { fileBytes: async () => null };

/* ── エンジンに組んでもらう ── */

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
const ask = (method, params) => new Promise((go) => {
    waiting.push(go);
    child.stdin.write(JSON.stringify({ id: ++id, method, params }) + '\n');
});
const toHtml = async (text) => {
    const r = await ask('html', { text });
    if (r.error) throw new Error(r.error);
    return r.ok.html || '';
};

/* ── 往復 ── */

/// 一周まわす。画面に組んで、テキストに戻す。
async function trip(md) {
    box.innerHTML = await toHtml(md);
    // **本物と同じ順。** `drawRead` は `innerHTML` → `armRead`（＝`armPaper`）
    // → `findPictures`。順を変えると、ラベルを持たせる前に掛け替えることになる。
    armPaper(box, md, true);
    findPictures();
    return paperToMd(box, '');
}

/// 末尾の空行の数だけ違うか。`sameNote`（`gui/renderer.js`）と同じ考え。
const nearly = (a, b) => String(a).replace(/\n+$/, '') === String(b).replace(/\n+$/, '');

/// どこで食い違ったかを、行で言う。
function firstOff(a, b) {
    const A = String(a).split('\n');
    const B = String(b).split('\n');
    for (let i = 0; i < Math.max(A.length, B.length); i++) {
        if (A[i] !== B[i]) return { 行: i + 1, もと: A[i], 戻り: B[i] };
    }
    return null;
}

/* ── 回す形 ── */

// 一つずつ。**`PAPER.ja.md` 二章の表がそのまま並んでいる。**
const ONE = {
    '見出し 一段': '# あ',
    '見出し 六段': '###### あ',
    '見出し ぜんぶ': '# 一\n\n## 二\n\n### 三\n\n#### 四\n\n##### 五\n\n###### 六',
    '段落': 'ふつうの段落です。',
    '段落 二行': '一行目です。\n二行目です。',
    '段落 三行': '一行目。\n二行目。\n三行目。',
    '段落 行末の空白二つ': '一行目。  \n二行目。',
    '段落 行末の逆斜線': '一行目。\\\n二行目。',
    '段落 二つ': 'ひとつめ。\n\nふたつめ。',
    '点 ハイフン': '- あ\n- い',
    '点 星': '* あ\n* い',
    '点 プラス': '+ あ\n+ い',
    '点 入れ子': '- あ\n  - こ\n- い',
    '番号 ふつう': '1. あ\n2. い',
    '番号 ぜんぶ 1': '1. あ\n1. い\n1. う',
    '番号 丸括弧': '1) あ\n2) い',
    '番号 途中から': '3. あ\n4. い',
    'セル 空': '- [ ] やること',
    'セル 済み': '- [x] やったこと',
    'セル 大文字': '- [X] やったこと',
    'セル 混ぜ': '- [ ] あ\n- [x] い',
    'セル 入れ子': '- [ ] あ\n  - [ ] こ',
    '引用 一行': '> ひとこと',
    '引用 二行': '> 一行目\n> 二行目',
    '引用 二重': '> 外\n> > 中',
    '引用の中の点': '> - あ\n> - い',
    '引用の中のセル': '> - [ ] あ',
    '引用の中の見出し': '> ## 見出し',
    '注記 NOTE': '> [!NOTE]\n> 覚えておくこと。',
    '注記 五種': '> [!TIP]\n> こつ。',
    '注記の中の点': '> [!WARNING]\n> - あ\n> - い',
    '表 ふつう': '| 名前 | 値 |\n| --- | --- |\n| あ | 1 |',
    '表 揃え 左': '| 名前 | 値 |\n| :--- | --- |\n| あ | 1 |',
    '表 揃え 右': '| 名前 | 値 |\n| ---: | --- |\n| あ | 1 |',
    '表 揃え 中': '| 名前 | 値 |\n| :---: | --- |\n| あ | 1 |',
    // **空のセルは、全角空白で埋まって戻る。** 依頼 94・190 で二度決めた
    // こと ── 中身が空のセルは描く側によっては消え、消えた表は「作れなかった」
    // に見える。丸めているのではなく、**そう決めた**（だから戻る文字で書く）。
    '表 空のセル': '| 名前 | 値 |\n| --- | --- |\n| あ | 　 |',
    '表 三行': '| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |',
    '枠 言語なし': '```\nそのままの文字\n```',
    '枠 言語あり': '```rust\nfn main() {}\n```',
    '枠 波線': '~~~\nそのままの文字\n~~~',
    '枠 中に記号': '```\n- これは点ではない\n# これは見出しではない\n```',
    '図': '```mermaid\nflowchart LR\n  A --> B\n```',
    '水平線 ハイフン': 'あ\n\n---\n\nい',
    '水平線 星': 'あ\n\n***\n\nい',
    '水平線 下線': 'あ\n\n___\n\nい',
    '強調 太字': 'これは **太い** 文字。',
    '強調 斜体': 'これは *斜め* の文字。',
    '強調 取り消し': 'これは ~~消した~~ 文字。',
    '強調 コード': 'これは `code` です。',
    '強調 日本語の直後': 'あ**い**う',
    '強調 入れ子': '**太くて *斜め* な文字**',
    'リンク': '[見せる文字](https://example.com/)',
    'リンク 題が空': '[](https://example.com/)',
    // **裸の URL は、押せるようになっても文字が変わらない**（依頼 606）。
    // ここで戻り値が `[https://x](https://x)` になれば、打っていない記号が
    // ファイルに増えたということ ── 同期していれば、それが全部むこうへ飛ぶ。
    // **折りたたみ**（依頼 619）── 畳んだ中身を書き戻しで失わない。
    // 閉じたまま保存しただけで中が書き換わる、がいちばん怖い。
    //
    // **「段落のすぐ下」は置いていない。** 空行を挟まずに置いたかたまりは
    // 保存で空行が一つ増えるが、それは**表も枠も引用も同じ**（数えた）──
    // 畳みの話ではなく、この画面の元からの均し方。
    '折りたたみ': '<details>\n<summary>見出し</summary>\n\n中身\n\n</details>',
    '折りたたみ 中に枠': '<details>\n<summary>ながいコード</summary>\n\n```js\nconst a = 1;\n```\n\n</details>',
    '折りたたみ 中に表': '<details>\n<summary>表</summary>\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n</details>',
    '折りたたみ 見出しに書式': '<details>\n<summary>ながい **コード**</summary>\n\n中身\n\n</details>',
    '折りたたみ 見出し無し': '<details>\n\n中身\n\n</details>',
    '折りたたみ はじめから開く': '<details open>\n<summary>見出し</summary>\n\n中身\n\n</details>',
    '折りたたみ 入れ子': '<details>\n<summary>そと</summary>\n\n<details>\n<summary>なか</summary>\n\nおく\n\n</details>\n\n</details>',
    '折りたたみ 前後に段': 'まえ。\n\n<details>\n<summary>見出し</summary>\n\n中身\n\n</details>\n\nあと。',
    '裸の URL': '見て https://example.com/a',
    '裸の URL 文の途中': '見て https://example.com/a 。つぎ',
    '裸の URL と句点': 'ここ https://example.com/a。',
    '裸の URL 括弧の中': '（https://example.com/a）',
    '裸の URL 二つ': 'https://example.com/a と https://example.com/b',
    '裸の URL と名前つき': 'https://x/a と [名前](https://x/b)',
    // **初めて開いた人に見せる絵**（依頼 437）── 半分の高さの記号で
    // 描いてあるので、一文字でも欠けると文字が崩れる。
    '初めての絵': '```amber\n██████╗  ██████████╗ █████╗\n╚═══██║  ██╔═██╔═██╗ ██╔═██╗\n╚█████╔╝ ╚═╝ ╚═╝ ╚═╝ ╚████╔╝\n```',
    '画像': '![題](attachments/あ.png)',
    '画像 題が空': '![](attachments/あ.png)',
    // **大きさの指示は、題の中に書いてある**（Marp と同じ・依頼 413）。
    // ビルドする側は `style` に移して題から外すが、**戻すときは書いた行のまま**
    // ── 外した指示が戻らないと、保存のたびに画像が元の大きさへ戻る。
    '画像 大きさ': '![width:200px](attachments/あ.png)',
    '画像 大きさと題': '![猫 w:200 h:80%](attachments/あ.png)',
    // **生の HTML は、文字として戻ること。** core はラベルを逃がして文字にするので
    // （`esc`）、画面に知らないラベルは現れない ── 人が書いた `<details>` が、
    // 保存のたびに削られたりしないことを、ここで見張る。
    '生の HTML': '<details>\n<summary>ひらく</summary>\n中身\n</details>',
    '行の中の札': 'これは <sup>上付き</sup> です。',
    'script も文字': '<script>alert(1)</script>',
    '記号を逃がした文字': 'a &lt; b &amp; c',
    'インデント 全角空白': '　インデントた段落です。',
    'インデント 二つ': '　　二段さがり。',
};

// 組み合わせ。**一つずつは通っても、入れ子で落ちる。**
const MIX = {
    '見出しと段落と点': '# 題\n\n段落。\n\n- あ\n- い',
    '点のあとに表': '- あ\n- い\n\n| a | b |\n| --- | --- |\n| 1 | 2 |',
    '引用のあとに枠': '> ひとこと\n\n```\nコード\n```',
    '注記のあとに図': '> [!NOTE]\n> みて。\n\n```mermaid\nflowchart LR\n  A --> B\n```',
    'セルと段落を混ぜる': '- [ ] あ\n\n段落。\n\n- [x] い',
    '枠のあとに段落': '```\nコード\n```\n\nそのあと。',
    '図で始まる': '```mermaid\nflowchart LR\n  A --> B\n```\n\nそのあと。',
    '表で終わる': '段落。\n\n| a |\n| --- |\n| 1 |',
    '枠で終わる': '段落。\n\n```\nコード\n```',
    '書式の入った点': '- **太い** と *斜め*\n- `code` と [リンク](https://x/)',
    '書式の入った表': '| **太い** | `code` |\n| --- | --- |\n| [リンク](https://x/) | あ |',
    '書式の入った見出し': '## **太い** 見出し',
    '引用の中の枠': '> ```\n> コード\n> ```',
    '全部入り': '# 題\n\n段落の一行目。\n段落の二行目。\n\n- [ ] やること\n- [x] やったこと\n\n> [!TIP]\n> こつ。\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\nおわり。',
};

// 末尾と前書き。**別の輪で回す** ── ここは `head` を渡すパスが違う。
const TAIL = {
    '末尾に改行なし': 'あ',
    '末尾に改行ひとつ': 'あ\n',
    '末尾に空行ひとつ': 'あ\n\n',
    '末尾に空行ふたつ': 'あ\n\n\n',
};

/* ── 回す ── */

const all = process.argv.includes('--all');
const 落第 = [];
const 弱い = [];
const 戻せない = [];
let 強い = 0;

async function run(name, md) {
    let one;
    try {
        one = await trip(md);
    } catch (e) {
        落第.push({ name, なぜ: '組めません: ' + e.message });
        return;
    }
    if (one === null) { 戻せない.push(name); return; }

    // 一周目。
    if (one === md) 強い++;
    else if (nearly(one, md)) 弱い.push({ name, 戻り: one });
    else 落第.push({ name, もと: md, 戻り: one, どこ: firstOff(md, one) });

    // 二周目 ── **形が動き続けないこと。** 動き続けるなら、保存のたびに
    // ノートが書き換わり、同期先で毎回差分になる。
    let two;
    try {
        two = await trip(one);
    } catch (e) {
        落第.push({ name: name + '（二周目）', なぜ: '組めません: ' + e.message });
        return;
    }
    if (two !== null && !nearly(two, one)) {
        落第.push({ name: name + '（二周目）', もと: one, 戻り: two, どこ: firstOff(one, two) });
    }
}

(async () => {
    const sets = [['一つずつ', ONE], ['組み合わせ', MIX], ['末尾', TAIL]];
    for (const [, table] of sets) {
        for (const [name, md] of Object.entries(table)) await run(name, md);
    }
    child.stdin.end();

    const 全部 = Object.keys(ONE).length + Object.keys(MIX).length + Object.keys(TAIL).length;
    console.log('文字 → 画面 → 文字（' + 全部 + ' 通り）');
    console.log('  強い一致  ' + 強い);
    console.log('  弱い一致  ' + 弱い.length + '（末尾の空行の数だけ違う ── 許す）');
    console.log('  戻せない  ' + 戻せない.length + (戻せない.length ? '：' + 戻せない.join('・') : ''));
    console.log('  落第      ' + 落第.length);

    if (all && 弱い.length) {
        console.log('\n弱い一致:');
        for (const w of 弱い) console.log('  ・' + w.name);
    }
    if (落第.length) {
        console.log('\n落第:');
        for (const b of 落第) {
            console.log('  ✗ ' + b.name + (b.なぜ ? ' ── ' + b.なぜ : ''));
            if (b.どこ) {
                console.log('      ' + b.どこ.行 + ' 行目');
                console.log('      もと ' + JSON.stringify(b.どこ.もと));
                console.log('      戻り ' + JSON.stringify(b.どこ.戻り));
            }
        }
    }
    console.log(落第.length ? '\n' + 落第.length + ' 通りが戻りません' : '\nぜんぶ戻りました');
    process.exit(落第.length ? 1 : 0);
})();
