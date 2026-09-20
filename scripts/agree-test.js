#!/usr/bin/env node
/* デスクトップ版と iPhone が、**同じノートを同じテキストに戻すか**（依頼 427）。
 *
 *                     ┌─ デスクトップ版の糊（findPictures）──┐
 *     .md ─ to_html ─┤                           ├─ paperToMd ─ 戻った .md
 *                     └─ iPhone の糊（Paper.swift）─┘
 *                                                      ここが同じか
 *
 * **切り出しを共有している意味は、そこにある。** テキストに戻す一本
 * （`paperToMd`／`blockToMd`）は両方が同じものを使うが、**その前後の糊は
 * 別々に書いてある** ── デスクトップ版は `gui/renderer.js`、iPhone は `ios/Cian/Paper.swift`
 * の中の JS。片方だけ足したり忘れたりすると、同じノートが端末によって
 * 別の文字で保存される。
 *
 * 実際にそうなった（2026-09-08）: デスクトップ版は `<img>` を `<figure>` で包んで
 * 元の文字を持たせていたが、**iPhone は包んでいなかった** ── 表示の画面で一度
 * 打つだけで、iPhone でだけ画像が消えた。往復の試験（`round-test`）はデスクトップ版の糊
 * しか通していないので、素通りしていた。
 *
 * 見るのは二つ:
 *   一。同じ `.md` が、両方の糊を通して同じテキストに戻ること
 *   二。**iPhone の糊が、切り出しの関数を上書きしていないこと** ──
 *       同じ名前で書き直すと、片方だけ直した日に静かにずれる
 *
 *     node scripts/agree-test.js
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
    path.join(root, 'target', 'debug', 'amber-server.exe'),
    path.join(root, 'target', 'debug', 'amber-server'),
    path.join(root, 'gui', 'amber-server.exe'),
    path.join(root, 'gui', 'amber-server'),
].find((at) => fs.existsSync(at));
if (!engine) {
    console.log('エンジンがありません（cargo build -p amber-server）── 飛ばします');
    process.exit(0);
}

/* ── 切り出しと、二つの糊 ── */

const src = fs.readFileSync(path.join(root, 'gui', 'renderer.js'), 'utf8');
const from = src.indexOf('function richBlock(');
const to = src.indexOf('/// このデスクトップ版の「表示」画面を、上の切り出しに繋ぐ薄い包み。');
if (from < 0 || to < 0) {
    console.error('gui/renderer.js から「表示」画面を切り出せません');
    process.exit(2);
}
const slice = src.slice(from, to);

/// デスクトップ版の糊 ── `drawRead` がビルドしたあとに呼ぶもの。
const grab = (head) => {
    const at = src.indexOf(head);
    if (at < 0) { console.error(head + ' が見つかりません'); process.exit(2); }
    return src.slice(at, src.indexOf('\n}\n', at) + 3);
};
const windowGlue = grab('function findPictures(');

/// iPhone の糊 ── `Paper.swift` の中の `window.show` が、ラベルを配ったあとに
/// する画像の包み。**そこだけを抜く**（前後は Swift の文字列の中）。
const swift = fs.readFileSync(path.join(root, 'ios', 'Cian', 'Paper.swift'), 'utf8');
const phoneFrom = swift.indexOf("for (const img of box.querySelectorAll('img')) {\n        const alt");
if (phoneFrom < 0) {
    console.error('ios/Cian/Paper.swift から画像の包みを切り出せません');
    process.exit(2);
}
const phoneTo = swift.indexOf('\n      }\n', phoneFrom);
const phoneGlue = swift.slice(phoneFrom, phoneTo + 9);

/* ── 一。名前がぶつかっていないか ── */

let bad = 0;
const names = [...slice.matchAll(/^function ([A-Za-z_$][\w$]*)\s*\(/gm)].map((m) => m[1]);
// iPhone の中の JS ぜんぶ（Swift の `page` の中身）。
const phoneJs = swift.slice(swift.indexOf('static let page'));
for (const n of names) {
    // 切り出しと同じ名前を、iPhone が自分で書き直していないか。
    const again = new RegExp('(?:^|\\n)\\s*(?:function\\s+' + n + '\\s*\\(|const\\s+' + n + '\\s*=)');
    if (again.test(phoneJs)) {
        bad += 1;
        console.log('✗ iPhone が「' + n + '」を自分で書き直しています ── '
            + '切り出しと同じ名前は、片方だけ直した日に静かにずれます');
    }
}

/* ── 二。同じノートが、同じテキストに戻るか ── */

const dom = new JSDOM('<!doctype html><body><div id="paper"></div></body>');
global.window = dom.window;
global.document = dom.window.document;
global.Node = dom.window.Node;
global.DOMParser = dom.window.DOMParser;
global.URL = dom.window.URL;
global.getSelection = () => dom.window.getSelection();
// eslint-disable-next-line no-eval
(0, eval)(slice
    + windowGlue
    + 'function fileURL(at) { return "file://" + at; }\n'
    + 'const dirOf = (at) => String(at || "").replace(/[^/\\\\]*$/, "");\n'
    + 'const escapeHtml = (s) => String(s);\n'
    // iPhone の糊は、そのままだと `box` を外から取る ── 包んで名前を付ける。
    + 'function phonePictures(box) {\n' + phoneGlue + '\n}\n');
const box = document.getElementById('paper');
global.el = () => box;
global.state = { open: { path: '/notes/試し.md' } };
global.window.amber = { fileBytes: async () => null };

/// 一件ずつエンジンに組んでもらう。
const child = spawn(engine, [], { stdio: ['pipe', 'pipe', 'inherit'] });
let buf = '';
const pending = new Map();
let seq = 0;
child.stdout.on('data', (d) => {
    buf += d.toString();
    for (let n = buf.indexOf('\n'); n >= 0; n = buf.indexOf('\n')) {
        const line = buf.slice(0, n);
        buf = buf.slice(n + 1);
        if (!line.trim()) continue;
        const m = JSON.parse(line);
        pending.get(m.id)?.(m);
        pending.delete(m.id);
    }
});
const call = (method, params) => new Promise((go) => {
    const id = ++seq;
    pending.set(id, go);
    child.stdin.write(JSON.stringify({ id, method, params }) + '\n');
});

/// 試すノート。**画像のあるもの**を厚めに ── ずれたのはそこだった。
const CASES = [
    '![題](attachments/あ.png)',
    '![](attachments/あ.png)',
    '![width:200px](attachments/あ.png)',
    '![猫 w:200 h:80%](attachments/あ.png)',
    '本文の前\n\n![題](attachments/あ.png)\n\n本文のあと',
    '![外の画像](https://example.com/a.png)',
    '# 見出し\n\n段落。\n\n![題](attachments/あ.png)\n\n- 一つ\n- 二つ',
    '| 朝 | 夕 |\n| --- | --- |\n| 掃除 | 片づけ |\n\n![題](attachments/あ.png)',
    '```rust\nfn main() {}\n```\n\n![題](attachments/あ.png)',
    '> 引用\n\n![題](attachments/あ.png)\n\n---',
    '- [ ] やること\n- [x] やった\n\n![題](attachments/あ.png)',
];

(async () => {
    for (const md of CASES) {
        const html = (await call('html', { text: md })).ok?.html;
        if (html === undefined) { console.log('✗ 組めません: ' + JSON.stringify(md)); bad += 1; continue; }

        // デスクトップ版の順（`drawRead`）── ラベルを配ってから包む。
        box.innerHTML = html;
        armPaper(box, md, true);
        findPictures();
        const asWindow = paperToMd(box, '');

        // iPhone の順（`Paper.swift` の `window.show`）── 同じ順。
        box.innerHTML = html;
        for (const img of box.querySelectorAll('img')) {
            const at = img.getAttribute('src') || '';
            if (!/^[a-z]+:/i.test(at)) img.src = 'amber://note/' + at;
        }
        armPaper(box, md, true);
        phonePictures(box);
        const asPhone = paperToMd(box, '');

        if (asWindow !== asPhone) {
            bad += 1;
            console.log('✗ デスクトップ版と iPhone で文字が違います: ' + JSON.stringify(md));
            console.log('   ウィンドウ　: ' + JSON.stringify(asWindow));
            console.log('   iPhone: ' + JSON.stringify(asPhone));
        }
    }

    child.kill();
    console.log('');
    if (bad) {
        console.log(CASES.length + ' 件中、' + bad + ' 件ずれています');
        process.exit(1);
    }
    console.log('デスクトップ版と iPhone は、同じ文字に戻ります（' + CASES.length + ' 件）');
})();
