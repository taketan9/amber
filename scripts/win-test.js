#!/usr/bin/env node
/* Windows で踏んだところを、Mac の上で踏み直す。
 *
 *     node scripts/win-test.js
 *
 * **同じ形の不具合を、二度出した。** どちらも mac では一生出ない:
 *
 *   * 道の区切りを `/` だと思っていた ── Windows の道は `C:\Users\…` で、
 *     `split('/')` は道まるごとを返す。書き出したファイルの名前が道になり、
 *     画像の在りかは空になって**画像が一枚も出なくなった**
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
    // **在りかが空になると、画像が一枚も出ない。** 実際にそうなった。
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

    // ── 画像の在りか ──
    //
    // 会社の Windows で「この画像は読めません」と出た。`'file://' + 道` は
    // mac の道（`/` で始まる）だと**たまたま**斜線が三本になって通るが、
    // Windows の道（`C:\…`）では `C:` が機械の名前として読まれ、円記号は
    // `%5C` に化ける ── mac では一生出ない。
    console.log('画像の在りかを、画像に渡せる形にする');
    ok(fileURL('/Users/t/Documents/amber/attachments/01.png')
        === 'file:///Users/t/Documents/amber/attachments/01.png',
        'mac の道は、斜線三本');
    ok(fileURL('C:\\Users\\t502960\\Documents\\amber\\attachments\\01_rag_start.png')
        === 'file:///C:/Users/t502960/Documents/amber/attachments/01_rag_start.png',
        'Windows の道は、円記号を斜線に直して頭に一本足す',
        fileURL('C:\\Users\\t502960\\Documents\\amber\\attachments\\01_rag_start.png'));
    ok(!fileURL('C:\\Users\\t\\画像.png').includes('%5C'),
        '円記号は %5C のまま残さない',
        fileURL('C:\\Users\\t\\画像.png'));
    ok(fileURL('\\\\server\\share\\画像.png').startsWith('file://server/share/'),
        'ネットワークの置き場所は、斜線二本のまま（機械の名前が入る）',
        fileURL('\\\\server\\share\\画像.png'));
    ok(fileURL('/Users/t/あ い/画像.png') === 'file:///Users/t/%E3%81%82%20%E3%81%84/%E7%94%BB%E5%83%8F.png',
        '空白と日本語は、逃がす',
        fileURL('/Users/t/あ い/画像.png'));
    ok(fileURL('') === 'file:///', '道が無くても落ちない');

    // 道を繋いだうえで、ちゃんと URL になるか（実物と同じ順で通す）。
    console.log('ノートの隣の画像を、道からたどる');
    const dir = dirOf('C:\\Users\\t502960\\Documents\\amber\\手順.md');
    ok(dir === 'C:\\Users\\t502960\\Documents\\amber\\', '道の頭が取れる', dir);
    ok(fileURL(dir + 'attachments/01_rag_start.png')
        === 'file:///C:/Users/t502960/Documents/amber/attachments/01_rag_start.png',
        'ノートの隣の attachments へ繋がる',
        fileURL(dir + 'attachments/01_rag_start.png'));
}

// **道を短く見せるところも、Windows の道で切れているか**（依頼 603）。
// `leafOf` で同じ取りこぼしを踏んでいる（依頼 596）── `/` でしか割らない
// 関数は、Windows の道を**一つも切らずにまるごと**返す。画面の上では
// 「やけに横に長い一行」として出るので、見ただけでは道の話だと分からない。
console.log('道を、一行に収まる形にできるか');
{
    const cut = src.indexOf('function shortPath(at)');
    const end = src.indexOf('\n}', cut) + 2;
    if (cut < 0 || end < 2) {
        console.error('gui/renderer.js から shortPath を切り出せません');
        process.exit(2);
    }
    const make = (root) => (0, eval)(
        '(function (state) {\n' + src.slice(cut, end) + '\nreturn shortPath;\n})'
    )({ root });

    const winHome = make('C:\\Users\\t502960\\Documents\\amber');
    ok(winHome('C:\\Users\\t502960\\Documents\\OneNote') === '~\\Documents\\OneNote',
        'Windows の家の下は ~ に畳む', winHome('C:\\Users\\t502960\\Documents\\OneNote'));
    const deep = winHome('C:\\Users\\t502960\\Documents\\a\\b\\c\\d\\OneNote');
    ok(deep.includes('…') && deep.endsWith('d\\OneNote'),
        '深い Windows の道は、中を … にする', deep);
    ok(!deep.includes('/'), '区切りは、その道が使っているほうのまま', deep);

    const macHome = make('/Users/taketan/Documents/amber');
    ok(macHome('/Users/taketan/Documents/OneNote') === '~/Documents/OneNote',
        'mac の家の下も ~ に畳む', macHome('/Users/taketan/Documents/OneNote'));
    const deepMac = macHome('/Users/taketan/a/b/c/d/e/f');
    ok(deepMac.includes('…') && !deepMac.includes('\\'),
        '深い mac の道も … にする（円記号は混ぜない）', deepMac);
    // **家の外の道は、そのまま。** 勝手に `~` を付けると別の場所に見える。
    ok(winHome('D:\\share\\notes') === 'D:\\share\\notes',
        '家の外は、そのまま', winHome('D:\\share\\notes'));
}

// **F5 は、Windows の人が反射で押す鍵**（依頼 604・本人「保存ディレクトリの
// 中身をごっそり削除したときなど、表示がなかなかアンバー側に伝わらない。
// F5 で更新するとかの機能はほしいね」）。
//
// 見張り（`fs.watch`）は、見張っているフォルダそのものが消えると落ちる ──
// そのとき画面だけが古いまま残る。押せば必ず読み直す道が要る。
//
// **どこを打っていても効くこと**を見る ── エディタの中に居るときこそ
// 押される鍵で、そこで素通りすると「効かない」として出る。
console.log('F5 で読み直せるか');
{
    // **受け口は一つではない。** `keydown` を聞く場所は三つあり、
    // 最初に見つかったものは ⌘S のほう ── 頭から探すと別の関数を切り出す。
    const head = src.indexOf("const inField = e.target === el('find');");
    const stop = src.indexOf("if (e.code === 'Escape')", head);
    if (head < 0 || stop < 0) {
        console.error('gui/renderer.js から keydown の頭を切り出せません');
        process.exit(2);
    }
    const body = src.slice(head, stop);
    let asked = 0;
    let zenned = 0;
    const box = {
        el: () => ({ contains: () => false }),
        cmdRefresh: () => { asked++; },
        setZen: () => { zenned++; },
        zen: false,
    };
    // eslint-disable-next-line no-eval
    const press = (0, eval)('(function (box) { with (box) { return function (e) {\n'
        + body + '\n}; } })')(box);
    const ev = (code, more) => {
        let stopped = 0;
        press({ code, target: {}, preventDefault: () => { stopped++; },
                metaKey: false, ctrlKey: false, shiftKey: false, ...(more || {}) });
        return stopped;
    };

    asked = 0;
    ok(ev('F5') === 1 && asked === 1, 'F5 で読み直す', asked);
    asked = 0;
    ok(ev('KeyR', { ctrlKey: true }) === 1 && asked === 1, 'Ctrl+R でも読み直す', asked);
    asked = 0;
    ok(ev('KeyR', { metaKey: true }) === 1 && asked === 1, '⌘R でも読み直す', asked);
    // **⇧ を足したものは別の鍵。** 掴んでしまうと、そちらが効かなくなる。
    asked = 0;
    ok(ev('KeyR', { ctrlKey: true, shiftKey: true }) === 0 && asked === 0,
        'Ctrl+⇧+R は掴まない', asked);
    asked = 0;
    ok(ev('KeyR') === 0 && asked === 0, '素の R は字のまま', asked);
    // 隣の鍵を巻き込んでいないか。
    asked = 0; zenned = 0;
    ok(ev('F12') === 1 && zenned === 1 && asked === 0, 'F12 は今までどおり', [zenned, asked]);
}

// **消えたノートのタブは残さない**（依頼 612・本人「ゴミ箱にすてたはずの
// ノートがタブの表示に残り続けてしまう」）。
//
// 消す道は四つある（⋯ から一本・選んでまとめて・フォルダごと・同期が
// 向こうの削除を下ろしたとき）。四か所に同じ一行を足すと、三か所目で忘れる
// ── 数え直したあとの `reload` で一度だけ見る。
console.log('消えたノートのタブを残さないか');
{
    ok(/dropGoneTabs\(\);/.test(src), '数え直したあとに、一度だけ見る');
    const at = src.indexOf('function dropGoneTabs()');
    ok(at > 0, 'その一本がある');
    const body = src.slice(at, src.indexOf('\n}', at));
    // **書かずに外す。** `closeTab` は書きかけをファイルへ落とすので、
    // 消したはずのノートが書き戻って生き返る。
    ok(!body.includes('closeTab('), '書き戻さずに外す（消したノートを生き返らせない）', body.slice(0, 200));
    ok(!body.includes('saveTab('), '書きかけをファイルへ落とさない');
    // **一覧に無い＝消えた、ではない。** 外付けを抜いた回まで閉じない。
    // **名前が出ているだけでは足りない** ── 読んだ結果で本当に外している
    // かを見る（数えているのに使っていない、で一度黙った）。
    ok(/troubled\.some\(/.test(body) && /placeTrouble/.test(body),
       '困っている保存ディレクトリの下は触らない', body.slice(0, 300));
    ok(body.includes('state.guest'), '単発で開いている一本は触らない');
    ok(body.includes('trail'), 'たどった道からも抜く');
}

// **右押しは、押した場所で変わらない**（依頼 612・本人「どちらも同じ
// ポップアップにできる？」）。
//
// 前は題の右押しだけ手書きの四つで、一覧の行や ⋯ とは別のものが出ていた。
// 同じノートを右押ししているのに、押した場所で出るものが変わる。
console.log('右押しの献立が、押した場所で変わらないか');
{
    const four = ['タイトルを直す', 'ファイル名を写す', '場所をコピー'];
    for (const name of four) {
        ok(new RegExp("name: '" + name + "'[^}]*menu: true").test(src)
           || new RegExp("name: '" + name + "',[\\s\\S]{0,120}menu: true").test(src),
           '「' + name + '」は命令の表にある（＝どの右押しからも出る）');
    }
    // 題の右押しは、自分で献立を書かない ── 書けば、その日から二つになる。
    const at = src.indexOf("el('title').addEventListener('contextmenu'");
    ok(at > 0, '題の右押しがある');
    const body = src.slice(at, src.indexOf('});', at));
    ok(body.includes('openMenu('), '題の右押しは、同じ献立を呼ぶ', body.slice(0, 200));
    ok(!body.includes('popMenu('), '題の右押しは、自分で献立を書かない', body.slice(0, 200));
}

// **マウスを乗せたら、そこが選び目**（依頼 613・本人「マウスがオンボード
// されても選択のハイライトが変わらない」）。
//
// 「はい」に乗せて押しているのに光っているのは「いいえ」のまま、が
// いちばん怖い（ゴミ箱の確かめ）。
console.log('小窓は、マウスを乗せたら選び目が動くか');
{
    const at = src.indexOf("row.onclick = () => closeSheet(");
    ok(at > 0, '小窓の行に押しが付いている');
    const body = src.slice(at, at + 900);
    ok(body.includes('row.onmouseenter'), '乗せたときも受ける', body.slice(0, 120));
    ok(/at = k/.test(body), '乗せた行を選び目にする（鍵盤と同じ場所を動かす）');
    ok(/classList\.remove\('on'\)/.test(body) && /classList\.add\('on'\)/.test(body),
       '印だけ移す（一行ごとに描き直さない）');
}

console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
process.exit(bad ? 1 : 0);
