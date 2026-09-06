#!/usr/bin/env node
/* 同梱する側との約束を、まだ守れているか。
 *
 * amber の画面は crmaine の中でも動いている。あちらが握っているのは
 * **四つ**で、そこが黙って変わると「札は出るのに何も起きない」という形で
 * 出る ── いちばん辿りにくい壊れ方で、しかも壊したこちらには何も起きない。
 *
 * だから機械に見張らせる。名前を変えるなという意味ではなく、**変えるときに
 * ここが鳴る**ようにしておく（鳴ったら crmaine に一声かけてから直す）。
 *
 *     node scripts/contract.js
 */
'use strict';
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const read = (p) => fs.readFileSync(path.join(root, p), 'utf8');

let bad = 0;
const ok = (yes, what, why) => {
    console.log((yes ? '  ✓ ' : '  ✗ ') + what);
    if (!yes) { bad++; if (why) console.log('      ' + why); }
};

/* ── 一 ── 描く側に見せている口の、名前と形 ───────────────────
 *
 * crmaine はこのうち九つを呼ぶ。どれを呼んでいるかはあちらの事情なので、
 * **出している十五を丸ごと約束にする** ── 使われていないと思った一つを
 * 消した日に、それが呼ばれていた側の九つだった、が起こりうる。
 */
console.log('描く側に見せている口');
{
    const src = read('gui/preload.js');
    const WANT = [
        'call', 'recall', 'remember', 'pickFolder', 'pickFile', 'saveAs',
        'openLink', 'fileBytes', 'onGuest', 'pathOf', 'trash', 'saveText',
        'savePDF', 'ring', 'clipboardImage', 'appVersion', 'watch', 'onChanged', 'scratch',
    ];
    // `exposeInMainWorld('amber', { … })` の中の、頭に来る名前だけ。
    const body = src.slice(src.indexOf("exposeInMainWorld('amber'"));
    const found = [...body.matchAll(/^\s{4}(\w+):/gm)].map((m) => m[1]);
    for (const name of WANT) {
        ok(found.includes(name), 'window.amber.' + name,
            'preload.js から消えたか、名前が変わっています');
    }
    const extra = found.filter((n) => !WANT.includes(n));
    // 増やすぶんには壊れない。**増えたことは言う** ── 約束の表に足すかを
    // 決めるのは人で、黙って surface が広がるのは別の問題になる。
    if (extra.length) console.log('  ・ 増えています（表に足すか決めてください）: ' + extra.join(', '));
}

/* ── 二 ── エンジンは「画面の隣」に居る ─────────────────────── */
console.log('エンジンの探し方');
{
    const src = read('gui/engine.js');
    ok(/const beside = path\.join\(__dirname, exe\)/.test(src),
        '画面の隣の amber-server(.exe) を先に見る',
        '同梱する側はそこへ実行ファイルを置いています');
    ok(/'amber-server\.exe'/.test(src) && /'amber-server'/.test(src),
        '名前は amber-server / amber-server.exe');
}

/* ── 三 ── 画面が指している印 ──────────────────────────────── */
console.log('印の在りか');
{
    ok(read('gui/renderer.js').includes('"../packaging/amber-mark.png"'),
        '画面は ../packaging/amber-mark.png を指す',
        '同梱する側は gui/ の隣に packaging/ を置いています');
    ok(fs.existsSync(path.join(root, 'packaging/amber-mark.png')),
        'その道にファイルがある',
        'python3 packaging/amber_icon.py で焼けます');
}

/* ── 四 ── 画面の組み立て手順 ──────────────────────────────── */
console.log('画面の組み立て');
{
    const vendor = read('gui/vendor.js');
    ok(/const OUT = path\.join\(HERE, 'vendor'\)/.test(vendor),
        '積み先は gui/vendor/');
    ok(/node_modules/.test(vendor),
        'npm install が置いたものを写す（落としてはこない）');
    for (const [file, what] of [['gui/run.sh', 'Mac'], ['gui/run.bat', 'Windows']]) {
        const s = read(file);
        ok(/npm install/.test(s) && /vendor\.js/.test(s),
            what + ' の走らせ方が npm install → node vendor.js を通る');
    }
    // Monaco・vim・図の三つ。どれが欠けても「窓は開くが中身が無い」。
    for (const need of ['monaco/vs/loader.js', 'monaco-vim/monaco-vim.umd.js',
                        'mermaid/mermaid.min.js']) {
        ok(vendor.includes(need), '積むもの: ' + need);
    }
}

console.log(bad
    ? '\n' + bad + ' つ、同梱する側の前提が変わっています。'
        + '\n直す前に crmaine に一声かけてください（黙って変わると'
        + '「札は出るのに何も起きない」という形で出ます）。'
    : '\n同梱する側との約束は、ぜんぶ守られています');
process.exit(bad ? 1 : 0);
