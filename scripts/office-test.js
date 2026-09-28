#!/usr/bin/env node
/* 会社向けのビルドから、外のネットワークに触るものが本当に消えているか（依頼 602）。
 *
 *     node scripts/office-test.js
 *
 * 本人の言葉（2026-09-15）──「会社でビルドする際には、『同期』に関する
 * 機能や表示はすべてクローズにしたい」「Google カレンダーなども同様
 * 会社の環境では見えなくて良い」「Asset にアップしてもらう資材には
 * 見えないまたは機能を削ぎ落としたものにしてほしい」。
 *
 * **「メニューから消えた」では足りない。** amber には ⌘⇧P があり、そこは
 * 名前で探す道 ── メニューにだけ蓋をすると、打てば出てくる。だから消える
 * ところは一か所（`canRun`）にして、**両方のパスがそこを通る**ことを見る。
 *
 * **残すものも見る。** カレンダーの画面そのものと、チームの CSV は会社の
 * ための道具（会社の Outlook が書き出したファイルを読むだけで、外へは
 * 何も出さない）── 一緒に消すと、いちばん要るものが消える。
 */
'use strict';
const fs = require('fs');
const path = require('path');

const src = fs.readFileSync(path.join(__dirname, '..', 'gui', 'renderer.js'), 'utf8');

let bad = 0;
const ok = (yes, what, got) => {
    console.log((yes ? '  ✓ ' : '  ✗ ') + what);
    if (!yes) { bad++; if (got !== undefined) console.log('      ' + JSON.stringify(got)); }
};

/** 名前が何であれ `undefined` を返す箱 ── `run: cmdSync` のような、
 *  ここでは呼ばない参照を通すため。 */
const anything = new Proxy({}, { has: () => true, get: () => undefined });

function cutOut(head, tail) {
    const from = src.indexOf(head);
    const to = src.indexOf(tail, from);
    if (from < 0 || to < 0) {
        console.error('gui/renderer.js から切り出せません: ' + head);
        process.exit(2);
    }
    return src.slice(from, to + tail.length);
}

const cmdsSrc = cutOut('const CMDS = [', '\n];');
const canRunSrc = cutOut('const canRun = (c) =>', ';');

/** `OFFICE` を差し替えて、その版で走る命令の一覧を取る。 */
function commands(office) {
    // eslint-disable-next-line no-eval
    return (0, eval)('(function (box) { with (box) {\n'
        + 'const OFFICE = ' + (office ? 'true' : 'false') + ';\n'
        // **ぜんぶ出る状態から始める。** ノートを開いていない・グループを
        // 持っていない机では、閉じる前から出ていない命令がある ── それを
        // 「閉じられた」と数えると、検査が報告しない。
        + 'const state = { open: { path: \'x\' } };\n'
        + 'const groupCal = { id: \'g\' };\n'
        + cmdsSrc + '\n' + canRunSrc + '\n'
        + 'return CMDS.filter(canRun).map((c) => c.id);\n'
        + '} })')(anything);
}

console.log('会社向けのビルドでは、外へ運ぶものが出ない');
{
    const full = commands(false);
    const office = commands(true);
    // **外へ運ぶもの** ── Google Drive の同期、iCal の購読（＝カレンダーの
    // 同期）、グループでの共有。
    for (const id of ['sync', 'sub', 'unsub', 'toshare', 'groupdrop']) {
        ok(full.includes(id), '通常のビルドには「' + id + '」がある', full);
        ok(!office.includes(id), '会社向けのビルドに「' + id + '」は無い', office);
    }
    // **残すもの** ── カレンダーの画面と、チームの CSV（会社の Outlook のため
    // の道具で、読むだけ・外へは何も出さない）と、ふだんの道具。
    for (const id of ['cal', 'calset', 'team', 'teamoff', 'places', 'new', 'refresh']) {
        ok(office.includes(id), '会社向けのビルドにも「' + id + '」は残る', office);
    }
    // 閉じるのは、閉じると決めたものだけ。
    const gone = full.filter((id) => !office.includes(id));
    ok(gone.length === 5, '閉じたのは五つだけ', gone);
}

console.log('会社向けのビルドの、はじめの一枚（依頼 654）');
{
    // **本物の枝を通す。** 「一行目が落ちる」を検査の側で書き写すと、
    // 画面の側だけ直した日に黙る ── 切り出した枝をそのまま走らせる。
    const sellSrc = cutOut('const HELLO_SELL = [', '\n];');
    const pick = (re, what) => {
        const m = re.exec(src);
        if (!m) { console.error('gui/renderer.js から切り出せません: ' + what); process.exit(2); }
        return m[0];
    };
    const sliceSrc = pick(/HELLO_SELL\.slice\([^)]*\)/, 'はじめの一枚の切り出し');
    const goSrc = pick(/OFFICE \? '[^']*' : 'Google で始める'/, 'はじめの一枚のボタン');
    const run = (office, tail) => (0, eval)('(function () {\n'
        + 'const OFFICE = ' + (office ? 'true' : 'false') + ';\n'
        + sellSrc + '\nreturn ' + tail + ';\n})')();

    const full = run(false, sliceSrc).map((r) => r.join(' '));
    const office = run(true, sliceSrc).map((r) => r.join(' '));
    ok(full.length === 3, '通常のビルドは三行', full);
    ok(office.length === 2, '会社向けのビルドは二行', office);
    // **共有が閉じているので、「家族やグループと」はそこでは嘘になる。**
    ok(full.some((t) => t.includes('家族やグループと')), '通常のビルドは共有を売る', full);
    ok(!office.some((t) => t.includes('家族やグループと')), '会社向けのビルドは共有を売らない', office);
    // サインインさせる先が無いので、代わりに置き場所を訊く。
    ok(run(false, goSrc) === 'Google で始める', '通常のビルドはサインインへ', run(false, goSrc));
    ok(!/Google/.test(run(true, goSrc)), '会社向けのビルドはサインインへ行かない', run(true, goSrc));
}

console.log('会社向けのビルドの、はじめの案内（依頼 656）');
{
    // ここも**本物の枝を通す** ── 「三つ目が落ちる」を検査の側で書き写すと、
    // 画面の側だけ直した日に黙る。
    const tourSrc = cutOut('const TOUR = [', '\n];');
    const m = /TOUR\.filter\(\(r\) => [^)]*\)\)/.exec(src);
    if (!m) { console.error('gui/renderer.js から案内の選り分けを切り出せません'); process.exit(2); }
    const run = (office) => (0, eval)('(function () {\n'
        + 'const OFFICE = ' + (office ? 'true' : 'false') + ';\n'
        + tourSrc + '\nreturn ' + m[0] + ';\n})')();

    const full = run(false).map((r) => r.say);
    const office = run(true).map((r) => r.say);
    ok(full.length === 5, '通常のビルドは五段', full.length);
    ok(office.length === 4, '会社向けのビルドは四段', office.length);
    // **共有の段だけが落ちる。** 予定表は会社のための道具なので残す
    // （上の「残すもの」と同じ考え）。
    ok(full.some((t) => t.includes('共有に便利です')), '通常のビルドは共有を案内する', full);
    ok(!office.some((t) => t.includes('共有に便利です')), '会社向けのビルドは共有を案内しない', office);
    ok(office.some((t) => t.includes('予定表はここから')), '会社向けのビルドにも予定表は残る', office);
    ok(office.some((t) => t.includes('ここから押すだけ')), '会社向けのビルドにも書く道具は残る', office);
}

console.log('版のラベルの読み方');
{
    // `main.js` は `edition.json` を隣から読む。**環境変数が勝つ**
    // （手元で見比べるため）。ここは、その二行が居ることだけ見る ──
    // Electron を立ち上げずに `edition()` は呼べない。
    const main = fs.readFileSync(path.join(__dirname, '..', 'gui', 'main.js'), 'utf8');
    // **本物の `edition()` を通す。** 文字が在ることだけ見ていた版は、
    // 説明の中の `AMBER_EDITION` を数えて通っていた（報告しない検査、七度目）。
    const cut = main.indexOf('function edition()');
    const end = main.indexOf('\n}', cut) + 2;
    if (cut < 0 || end < 2) { console.error('gui/main.js から edition を切り出せません'); process.exit(2); }
    const editionWith = (env,札) => (0, eval)(
        '(function (process, fs, path, __dirname) {\n' + main.slice(cut, end) + '\nreturn edition;\n})'
    )({ env }, { readFileSync: () => { if (札 === null) throw new Error('ありません'); return 札; } },
      { join: (...a) => a.join('/') }, '/どこか')();

    ok(editionWith({}, null) === 'full', 'ラベルが無ければ、通常のビルド');
    ok(editionWith({}, '{"edition":"office"}') === 'office', '隣の edition.json を読む');
    ok(editionWith({ AMBER_EDITION: 'office' }, null) === 'office', '環境変数で上書きできる');
    // **環境変数が勝つ。** 手元で会社向けの見え方を確かめるための道。
    ok(editionWith({ AMBER_EDITION: 'full' }, '{"edition":"office"}') === 'full',
        '札より環境変数が勝つ');
    ok(editionWith({}, 'こわれている') === 'full', 'ラベルが壊れていても落ちない');
    ok(/ipcMain\.handle\('amber:edition'/.test(main), '描く側から訊ける');
    const pre = fs.readFileSync(path.join(__dirname, '..', 'gui', 'preload.js'), 'utf8');
    ok(/amber:edition/.test(pre), '細い一本にも通してある');
    // **ビルドする側が書く。** 書かなければ、配布物はふつうの版のまま。
    const pack = fs.readFileSync(path.join(__dirname, '..', 'scripts', 'pack.js'), 'utf8');
    ok(/edition\.json/.test(pack), 'ビルドする側が版のラベルを置く');
    ok(/'amber-win-x64' \+ \(edition === 'full' \? '' : '-' \+ edition\)/.test(pack),
        '会社向けのビルドは、名前で見分けられる');
}

console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
process.exit(bad ? 1 : 0);
