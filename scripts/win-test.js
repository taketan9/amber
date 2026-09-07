#!/usr/bin/env node
/* Windows で踏んだところを、Mac の上で踏み直す。
 *
 *     node scripts/win-test.js
 *
 * **同じ形の不具合を、二度出した。** どちらも mac では一生出ない:
 *
 *   * 道の区切りを `/` だと思っていた ── Windows の道は `C:\Users\…` で、
 *     `split('/')` は道まるごとを返す。書き出したファイルの名前が道になり、
 *     絵の在りかは空になって**絵が一枚も出なくなった**
 *   * Enter は一つだと思っていた ── フルサイズの鍵盤（会社の机にたいてい
 *     載っている）は右の Enter を `NumpadEnter` として送る。点と番号は
 *     画面が勝手に続けるので、**升だけが出ない**という形で現れた
 *
 * どちらも「実機で押されるまで分からなかった」ものだが、**判断そのものは
 * 純粋な関数**なので、道と鍵の形さえ渡せばここで捕まる。実機の代わりには
 * ならない（会社の OneDrive にゴミ箱が無い、は再現できない）が、
 * **半分はここで止められる**。
 *
 * `gui/renderer.js` から切り出して試す ── 写すと、写した側だけが直る。
 */
'use strict';
const fs = require('fs');
const path = require('path');

const src = fs.readFileSync(path.join(__dirname, '..', 'gui', 'renderer.js'), 'utf8');
const from = src.indexOf('const isEnter =');
const to = src.indexOf('const ask =');
if (from < 0 || to < 0 || to < from) {
    console.error('gui/renderer.js から道と鍵の道具を切り出せません'
        + '（`isEnter` から `dirOf` までの並びが変わりました）');
    process.exit(2);
}
// eslint-disable-next-line no-eval
const { isEnter, baseOf, dirOf } = (0, eval)(
    src.slice(from, to) + '\n({ isEnter, baseOf, dirOf })'
);

let bad = 0;
const ok = (yes, what, got) => {
    console.log((yes ? '  ✓ ' : '  ✗ ') + what);
    if (!yes) { bad++; if (got !== undefined) console.log('      ' + JSON.stringify(got)); }
};

console.log('Windows の道を、切り分けられるか');
{
    const win = 'C:\\Users\\t502960\\Documents\\amber\\買い物.md';
    ok(baseOf(win) === '買い物.md', '名前だけを取る', baseOf(win));
    ok(dirOf(win) === 'C:\\Users\\t502960\\Documents\\amber\\', '在りかを取る', dirOf(win));
    // **在りかが空になると、絵が一枚も出ない。** 実際にそうなった。
    ok(dirOf(win) !== '', '在りかが空にならない', dirOf(win));

    const nix = '/Users/x/Documents/amber/買い物.md';
    ok(baseOf(nix) === '買い物.md', 'mac でも同じ', baseOf(nix));
    ok(dirOf(nix) === '/Users/x/Documents/amber/', 'mac の在りか', dirOf(nix));

    // 空白と全角を含む道（会社の端末にはよくある）。
    const sp = 'C:\\Users\\山田 太郎\\Documents\\amber\\週報 2026.md';
    ok(baseOf(sp) === '週報 2026.md', '空白と全角が混ざっても', baseOf(sp));

    // 名前だけ・空・null で落ちない。
    ok(baseOf('ノート.md') === 'ノート.md', '道が無くても');
    ok(baseOf('') === '' && dirOf('') === '', '空でも落ちない');
    ok(baseOf(null) === '' && dirOf(null) === '', 'null でも落ちない');
}

console.log('鍵盤の右の Enter も、Enter として受けるか');
{
    ok(isEnter({ code: 'Enter' }) === true, 'ふつうの Enter');
    // **これを見ていなかった。** 会社の机の鍵盤はたいていフルサイズ。
    ok(isEnter({ code: 'NumpadEnter' }) === true, '数字の脇の Enter');
    ok(isEnter({ code: 'Space' }) === false, 'ほかの鍵は受けない');
    ok(isEnter({ code: 'NumpadAdd' }) === false, '数字の脇のほかの鍵も受けない');
}

console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
process.exit(bad ? 1 : 0);
