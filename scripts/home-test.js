#!/usr/bin/env node
/* 試験用の家（`AMBER_TEST_HOME`）が、本物の場所を一つも返さないか（依頼 717）。
 *
 *     node scripts/home-test.js
 *
 * macOS の Electron は `$HOME` を作り替えても `appData` と `documents` を
 * 移さないので、試験と撮影が**本人の本物の `amber.json` を読み書きしていた**。
 * crmaine の撮影では、撮った画面に本物のノート一覧が写り、カレンダーの見方が
 * 書き換わった（2026-11-20）。
 *
 * ここでは Electron を起こさずに見る。本物のアプリで同じことを見るのは
 * 総ざらいの最初の段（`walk.mjs` の「試験用の家：本物の場所を一つも返さない」）と、
 * `walk.sh` の終わりの「本物の設定は変わっていません」。
 */
'use strict';
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const read = (p) => fs.readFileSync(path.join(root, p), 'utf8');
const main = read('gui/main.js');

let bad = 0;
const ok = (yes, what, got) => {
    console.log((yes ? '  ✓ ' : '  ✗ ') + what);
    if (!yes) { bad++; if (got !== undefined) console.log('      ' + JSON.stringify(got)); }
};

// ── 試験用の家を決めるところを切り出す ──
//
// `TEST_HOME` の決め方と `testDirs` を、`main.js` からそのまま取る。写すと、
// 写した側だけが直る。`process` は差し替えて渡す ── 本物の環境変数を
// 読ませないためと、`process.exit` を受け止めるため。
const from = main.indexOf('const TEST_HOME = (() => {');
const to = main.indexOf('// **名前を先に決める。**');
if (from < 0 || to < 0 || to < from) {
    console.error('gui/main.js から試験用の家の決め方を切り出せません'
        + '（`const TEST_HOME` から「名前を先に決める」までの並びが変わりました）');
    process.exit(2);
}
const body = main.slice(from, to);
const load = (env) => {
    const fake = {
        env,
        exit: (code) => { throw new Error('起動しない（' + code + '）'); },
    };
    const quiet = { error: () => {} };
    // eslint-disable-next-line no-new-func
    return new Function('process', 'path', 'console',
        body + '\nreturn { TEST_HOME, testDirs };')(fake, path, quiet);
};

console.log('試験用の家を渡したとき、本物の場所を一つも返さないか');
{
    const home = '/tmp/試験の家';
    const { TEST_HOME, testDirs } = load({ AMBER_TEST_HOME: home });
    ok(TEST_HOME === home, '渡した家をそのまま使う', TEST_HOME);
    const dirs = testDirs(TEST_HOME);
    ok(['appData', 'documents', 'downloads', 'desktop'].every((k) => k in dirs),
        '憶えごと・ドキュメント・ダウンロード・デスクトップの四つを返す', Object.keys(dirs));
    const outside = Object.values(dirs).filter((d) => !d.startsWith(home + path.sep));
    ok(outside.length === 0, 'どれも試験用の家の下にある', outside);
    // **本物の場所を一つも返さない。** 本人の家と、その下の決まった場所。
    const real = require('os').homedir();
    const places = ['Library/Application Support', 'Documents', 'Downloads', 'Desktop']
        .map((d) => path.join(real, d));
    const leaked = Object.values(dirs).filter((d) => places.some((p) => d === p || d.startsWith(p + path.sep)));
    ok(leaked.length === 0, '本物の場所を一つも返さない', leaked);
    // **crmaine と同じ並び**（`CRMAINE_RECORD_HOME`）── 同じ家を渡せば、
    // crmaine の中の amber と単体の amber が同じ試験用の amber.json を見る。
    ok(dirs.appData === path.join(home, 'appData') && dirs.documents === path.join(home, 'Documents')
        && dirs.downloads === path.join(home, 'Downloads') && dirs.desktop === path.join(home, 'Desktop'),
        '並びは crmaine と同じ（appData・Documents・Downloads・Desktop）', dirs);

    // 前後の空白は落とす。末尾の区切りも畳む（`//` が混ざると、道を文字で
    // 見比べるところで自分の書いたものと分からなくなる ── `walk.sh` の註）。
    const t = load({ AMBER_TEST_HOME: '  /tmp/試験の家/  ' }).TEST_HOME;
    ok(t === '/tmp/試験の家/' || t === '/tmp/試験の家', '前後の空白を落とす', t);
}

console.log('渡さないときは、今までどおり');
{
    ok(load({}).TEST_HOME === '', '何も渡さなければ空（OS に訊く）');
    ok(load({ AMBER_TEST_HOME: '   ' }).TEST_HOME === '', '空白だけでも空');
}

console.log('書き間違えたまま、本物の家で走らない');
{
    // **相対のパスは起動しない。** 黙って本物の家に戻って走るのが、いちばん
    // 起きてほしくないこと。
    let stopped = '';
    try { load({ AMBER_TEST_HOME: 'tmp/試験の家' }); } catch (e) { stopped = e.message; }
    ok(stopped.startsWith('起動しない'), '相対のパスなら起動しない', stopped);
}

console.log('四か所とも、試験用の家を見ているか');
{
    ok(/app\.setPath\('userData', path\.join\(TEST_HOME \? testDirs\(TEST_HOME\)\.appData : app\.getPath\('appData'\), 'amber'\)\)/.test(main),
        '憶えごと（userData）');
    const kd = main.slice(main.indexOf("ipcMain.handle('amber:knownDirs'"));
    const kdBody = kd.slice(0, kd.indexOf('});'));
    ok(/const get = TEST_HOME\s*\?\s*\(k\) => testDirs\(TEST_HOME\)\[k\]/.test(kdBody),
        'ドキュメント・ダウンロード・デスクトップ（knownDirs）', kdBody.slice(0, 200));
    const fr = main.slice(main.indexOf('function firstRoot()'));
    ok(/const docs = TEST_HOME \? testDirs\(TEST_HOME\)\.documents : /.test(fr.slice(0, fr.indexOf('\n}'))),
        '初めて開いたときの置き場所（firstRoot）── 見落とすと本物の ~/Documents/cian が開く');
    const cl = main.slice(main.indexOf('function clouds()'));
    const clBody = cl.slice(0, cl.indexOf('\n}'));
    ok(/const home = TEST_HOME \|\| os\.homedir\(\);/.test(clBody), 'クラウドのフォルダ（clouds）');
    // Windows の、家の外を指す三つ（環境変数・ドライブ名）は、試験用の家では見ない。
    const guarded = clBody.slice(clBody.indexOf('if (!TEST_HOME) {'));
    ok(/if \(!TEST_HOME\) \{[\s\S]*process\.env\.OneDrive[\s\S]*OneDriveCommercial[\s\S]*G:/.test(guarded),
        'Windows の OneDrive・ドライブ名は、試験用の家では見ない');
}

console.log('試験の道具が、本物の amber.json を写したり消したりしないか');
for (const f of ['scripts/walk.sh', 'scripts/first-run.sh']) {
    const src = read(f);
    // **起こす行の上で見る。** 註にも同じ名前が書いてあるので、ファイル全体で
    // 探すと、起こす行から外しても註に当たって通る（実際に黙った）。
    ok(/HOME="\$home" AMBER_TEST_HOME="\$home"/.test(src), f + ' は試験用の家を渡す');
    // **まっさらな引き出しは鍵束に問い合わせる。** 渡さないと本人の画面に
    // 「キーチェーンが見つかりません」が出て止まる（2026-09-20）。
    ok(/npx electron [^\n]*--use-mock-keychain/.test(src), f + ' は鍵束の代わりを使う（--use-mock-keychain）');
    // 前は写して、終わったら書き戻していた（first-run.sh は消してもいた）。
    ok(!/amber\.json\.mine/.test(src), f + ' は本物を写さない');
    ok(!/rm -f "\$mine"/.test(src), f + ' は本物を消さない');
    ok(!/cp [^\n]*"\$mine"\s*$/m.test(src), f + ' は本物に書き戻さない');
    // **終わりに、本物が変わっていないかを見る。**
    ok(/print_mine\)" = "\$before"/.test(src) && /exit 3/.test(src),
        f + ' は終わりに本物の設定を見比べ、変わっていたら失敗で終える');
}

console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
process.exit(bad ? 1 : 0);
