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

// **鍵の言い換えは、土台のふりをして試す。** `MAC` は `navigator` を見て
// 決まるので、Windows のふりをしてから切り出す ── mac で走らせても
// Windows の答えが出る（この試験の値打ちはそこ）。
// `navigator` は新しい node では書き換えられない（読むだけ）ので、
// 切り出しの中だけで名前を隠す。
const keySrc = src.slice(src.indexOf('const MAC ='), src.indexOf('const ask ='));
// eslint-disable-next-line no-eval
const { keyText } = (0, eval)(
    '(function (navigator) {\n' + keySrc + '\nreturn { keyText };\n})'
)({ userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' });

const from = src.indexOf('const isEnter =');
const to = src.indexOf('const ask =');
if (from < 0 || to < 0 || to < from) {
    console.error('gui/renderer.js から道と鍵の道具を切り出せません'
        + '（`isEnter` から `fileURL` までの並びが変わりました）');
    process.exit(2);
}
// eslint-disable-next-line no-eval
const { isEnter, baseOf, dirOf, fileURL } = (0, eval)(
    src.slice(from, to) + '\n({ isEnter, baseOf, dirOf, fileURL })'
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

    // ── 鍵の並び ──
    //
    // 会社の Windows で **`⌘` が出ていた**（本人が見た・2026-09-08）。
    // あちらに `⌘` という鍵は無いので、**押しようがない案内**が出ていた
    // ことになる ── mac では一生出ない。表には mac の記号で書いておき、
    // 出すときに言い換える。
    console.log('鍵の並びを、その土台の言葉で');
    ok(keyText('⌘N') === 'Ctrl+N', '⌘ は Ctrl', keyText('⌘N'));
    ok(keyText('⌘⇧O') === 'Ctrl+Shift+O', '重ねた鍵は + で繋ぐ', keyText('⌘⇧O'));
    ok(keyText('⌥') === 'Alt', '⌥ は Alt', keyText('⌥'));
    ok(keyText('⌃') === 'Ctrl', '⌃ も Ctrl', keyText('⌃'));
    ok(keyText('F12') === 'F12', '記号の無いものは、そのまま', keyText('F12'));
    ok(keyText('Esc') === 'Esc', 'Esc もそのまま', keyText('Esc'));
    ok(keyText('⌘←') === 'Ctrl+←', '矢印はどちらの土台でも矢印', keyText('⌘←'));
    ok(keyText('⌥ 押し') === 'Alt 押し',
        '記号のあとの言葉は、+ で繋がない（説明であって鍵ではない）', keyText('⌥ 押し'));
    ok(keyText('') === '' && keyText(undefined) === '', '鍵が無くても落ちない');
    ok(!/[⌘⌃⇧⌥]/.test(
        ['⌘N', '⌘⇧O', '⌘/', '⌘⇧/', '⌘←', '⌘→', '⌘⇧P', '⌘E', '⌘D', '⌘S']
            .map(keyText).join(' ')),
        'mac の記号が一つも残らない');

    // ── 絵の在りか ──
    //
    // 会社の Windows で「この絵は読めません」と出た。`'file://' + 道` は
    // mac の道（`/` で始まる）だと**たまたま**斜線が三本になって通るが、
    // Windows の道（`C:\…`）では `C:` が機械の名前として読まれ、円記号は
    // `%5C` に化ける ── mac では一生出ない。
    console.log('絵の在りかを、絵に渡せる形にする');
    ok(fileURL('/Users/t/Documents/amber/attachments/01.png')
        === 'file:///Users/t/Documents/amber/attachments/01.png',
        'mac の道は、斜線三本');
    ok(fileURL('C:\\Users\\t502960\\Documents\\amber\\attachments\\01_rag_start.png')
        === 'file:///C:/Users/t502960/Documents/amber/attachments/01_rag_start.png',
        'Windows の道は、円記号を斜線に直して頭に一本足す',
        fileURL('C:\\Users\\t502960\\Documents\\amber\\attachments\\01_rag_start.png'));
    ok(!fileURL('C:\\Users\\t\\絵.png').includes('%5C'),
        '円記号は %5C のまま残さない',
        fileURL('C:\\Users\\t\\絵.png'));
    ok(fileURL('\\\\server\\share\\絵.png').startsWith('file://server/share/'),
        'ネットワークの置き場所は、斜線二本のまま（機械の名前が入る）',
        fileURL('\\\\server\\share\\絵.png'));
    ok(fileURL('/Users/t/あ い/絵.png') === 'file:///Users/t/%E3%81%82%20%E3%81%84/%E7%B5%B5.png',
        '空白と日本語は、逃がす',
        fileURL('/Users/t/あ い/絵.png'));
    ok(fileURL('') === 'file:///', '道が無くても落ちない');

    // 道を繋いだうえで、ちゃんと URL になるか（実物と同じ順で通す）。
    console.log('ノートの隣の絵を、道からたどる');
    const dir = dirOf('C:\\Users\\t502960\\Documents\\amber\\手順.md');
    ok(dir === 'C:\\Users\\t502960\\Documents\\amber\\', '道の頭が取れる', dir);
    ok(fileURL(dir + 'attachments/01_rag_start.png')
        === 'file:///C:/Users/t502960/Documents/amber/attachments/01_rag_start.png',
        'ノートの隣の attachments へ繋がる',
        fileURL(dir + 'attachments/01_rag_start.png'));
}

console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
process.exit(bad ? 1 : 0);
