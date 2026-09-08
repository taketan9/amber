#!/usr/bin/env node
/* 字 → 面 → 字 が、元に戻るか。**総当たりで。**
 *
 *     もとの .md ──▶ to_html（core）──▶ 面 ──▶ paperToMd（js）──▶ 戻った .md
 *                                                      ↑
 *                                     ここが もとの .md と同じか
 *
 * `paper-test.js` は面 → 字の一方向を、**手で書いた HTML**から見ている。
 * こちらは**本物の往復**を見る ── 組む側は core（Rust）、戻す側は窓と
 * 電話が共有している切り出し（js）。二つの言語をまたぐので、エンジンの
 * 実行ファイルが要る。
 *
 * **なぜ要るのか。** 2026-09-08 に実物で踏んだ ── 「表示」の面で一文字
 * 打っただけで、打っていない行が五種類書き換わった（段落の改行が空白に、
 * `*` の点が `-` に、`1. 1.` が `1. 2.` に、引用の二行が一行に、行頭の
 * 全角空白が消える）。同期しているフォルダなら、それが全部むこうへ差分と
 * して飛ぶ。**一つずつの形は `paper-test.js` が通っていた。落ちたのは
 * 往復と、組み合わせ。**
 *
 * 見方は三つ（`PAPER.ja.md` 二章）:
 *   強い一致 ── 字がそのまま戻る。ここを目指す
 *   弱い一致 ── 末尾の空行の数だけ違う。許す（`sameNote` と同じ考え）
 *   落第     ── それ以外。「たぶん同じ意味」で通さない
 *
 * `null`（戻せない）は落第にしない ── 「戻せないなら書かない」が正しい。
 * ただし**どの形で出たかは数えて出す**。
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

// 組む側は core。**実行ファイルが無ければ飛ばす** ── この試験のためだけに
// `cargo build` を強いない（`paper-test.js` が jsdom を強いないのと同じ）。
const engine = [
    path.join(root, 'target', 'release', 'amber-server'),
    path.join(root, 'gui', 'amber-server'),
].find((at) => fs.existsSync(at));
if (!engine) {
    console.log('エンジンがありません（cargo build --release -p amber-server）── 飛ばします');
    process.exit(0);
}

/* ── 面（切り出しをそのまま動かす） ── */

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

/// 一周まわす。面に組んで、字に戻す。
async function trip(md) {
    box.innerHTML = await toHtml(md);
    // **`data-md` は組んだときに持たせる。** 本物と同じ順（`drawRead` は
    // `innerHTML` → `armRead` → `armPaper`）。
    armPaper(box, md, true);
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
    '升 空': '- [ ] やること',
    '升 済み': '- [x] やったこと',
    '升 大文字': '- [X] やったこと',
    '升 混ぜ': '- [ ] あ\n- [x] い',
    '升 入れ子': '- [ ] あ\n  - [ ] こ',
    '引用 一行': '> ひとこと',
    '引用 二行': '> 一行目\n> 二行目',
    '引用 二重': '> 外\n> > 中',
    '引用の中の点': '> - あ\n> - い',
    '引用の中の升': '> - [ ] あ',
    '引用の中の見出し': '> ## 見出し',
    '注記 NOTE': '> [!NOTE]\n> 覚えておくこと。',
    '注記 五種': '> [!TIP]\n> こつ。',
    '注記の中の点': '> [!WARNING]\n> - あ\n> - い',
    '表 ふつう': '| 名前 | 値 |\n| --- | --- |\n| あ | 1 |',
    '表 揃え 左': '| 名前 | 値 |\n| :--- | --- |\n| あ | 1 |',
    '表 揃え 右': '| 名前 | 値 |\n| ---: | --- |\n| あ | 1 |',
    '表 揃え 中': '| 名前 | 値 |\n| :---: | --- |\n| あ | 1 |',
    '表 空のセル': '| 名前 | 値 |\n| --- | --- |\n| あ |  |',
    '表 三行': '| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |',
    '枠 言語なし': '```\nそのままの字\n```',
    '枠 言語あり': '```rust\nfn main() {}\n```',
    '枠 波線': '~~~\nそのままの字\n~~~',
    '枠 中に記号': '```\n- これは点ではない\n# これは見出しではない\n```',
    '図': '```mermaid\nflowchart LR\n  A --> B\n```',
    '水平線 ハイフン': 'あ\n\n---\n\nい',
    '水平線 星': 'あ\n\n***\n\nい',
    '水平線 下線': 'あ\n\n___\n\nい',
    '強調 太字': 'これは **太い** 字。',
    '強調 斜体': 'これは *斜め* の字。',
    '強調 取り消し': 'これは ~~消した~~ 字。',
    '強調 コード': 'これは `code` です。',
    '強調 日本語の直後': 'あ**い**う',
    '強調 入れ子': '**太くて *斜め* な字**',
    'リンク': '[見せる字](https://example.com/)',
    'リンク 題が空': '[](https://example.com/)',
    '絵': '![題](attachments/あ.png)',
    '絵 題が空': '![](attachments/あ.png)',
    '字下げ 全角空白': '　字下げた段落です。',
    '字下げ 二つ': '　　二段さがり。',
};

// 組み合わせ。**一つずつは通っても、入れ子で落ちる。**
const MIX = {
    '見出しと段落と点': '# 題\n\n段落。\n\n- あ\n- い',
    '点のあとに表': '- あ\n- い\n\n| a | b |\n| --- | --- |\n| 1 | 2 |',
    '引用のあとに枠': '> ひとこと\n\n```\nコード\n```',
    '注記のあとに図': '> [!NOTE]\n> みて。\n\n```mermaid\nflowchart LR\n  A --> B\n```',
    '升と段落を混ぜる': '- [ ] あ\n\n段落。\n\n- [x] い',
    '枠のあとに段落': '```\nコード\n```\n\nそのあと。',
    '図で始まる': '```mermaid\nflowchart LR\n  A --> B\n```\n\nそのあと。',
    '表で終わる': '段落。\n\n| a |\n| --- |\n| 1 |',
    '枠で終わる': '段落。\n\n```\nコード\n```',
    '飾りの入った点': '- **太い** と *斜め*\n- `code` と [リンク](https://x/)',
    '飾りの入った表': '| **太い** | `code` |\n| --- | --- |\n| [リンク](https://x/) | あ |',
    '飾りの入った見出し': '## **太い** 見出し',
    '引用の中の枠': '> ```\n> コード\n> ```',
    '全部入り': '# 題\n\n段落の一行目。\n段落の二行目。\n\n- [ ] やること\n- [x] やったこと\n\n> [!TIP]\n> こつ。\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\nおわり。',
};

// 末尾と前書き。**別の輪で回す** ── ここは `head` を渡す道が違う。
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
    console.log('字 → 面 → 字（' + 全部 + ' 通り）');
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
