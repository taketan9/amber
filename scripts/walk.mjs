#!/usr/bin/env node
/* 窓の**総ざらい**。人が押すところを、片端から実際に動かす。
 *
 *     scripts/walk.sh          # 場所を作り、窓を出し、これを走らせ、片づける
 *     node scripts/walk.mjs    # 既に 9333 で出ている窓に対して走らせる
 *
 * **なぜ要るのか。** 一つずつの試験（`round-test` など）は通っているのに、
 * 実物では落ちる、が何度もあった ── 落ちるのはたいてい「押したときに
 * しか通らない道」で、そこは単体の試験が触れない。
 *
 * 見ているのは三つ:
 *   一。例外が飛ばないこと（`Runtime.exceptionThrown`）
 *   二。`console.error` / `warning` が出ないこと
 *   三。**触ったあと、そのノートがまだ字に戻せること**（`paperToMd`）──
 *       戻せないノートは、そこから先の保存が黙って止まる
 *
 * 落ちたものだけ出す。ぜんぶ通れば一行。
 *
 * **窓へ送る字の中に、逆引用符と円記号を書かないこと。** ここは
 * テンプレートの中なので、そこでテンプレートが閉じる・改行が本物になる
 * ── 三度踏んだ（2026-09-09）。註にも書けない。改行が要るなら
 * String.fromCharCode(10)。
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { step, run, bad, tally, sleep, ready, report, NOTES } from './walk-harness.mjs';
import { syncWalk } from './walk-sync.mjs';

/* ── 総ざらい ── */

const path = (n) => `state.root + '/${n}'`;

// 一。開いて、見る
await ready();
await step('ノートを開く（よくばり）', `await openNote(${path('よくばり.md')}); return !!state.open;`, true);
await step('表示 → コード', `setView('write'); return view;`, 'write');
await step('コード → 並べて表示', `setView('split'); return view;`, 'split');
await step('並べて表示 → 表示', `setView('read'); return view;`, 'read');
// **入切は「変わったか」で見る。** 憶えている設定がどちらから始まるか
// 分からないので、絶対の値で見ると、前に触った人の設定で落第になる。
await step('目次を入れ替える', `const was = tocOn; toggleToc(); return tocOn !== was;`, true);
await step('目次を戻す', `const was = tocOn; toggleToc(); return tocOn !== was;`, true);
await step('ノートだけ大きく', `setZen(true); return zen;`, true);
await step('もとに戻す', `setZen(false); return zen;`, false);
await step('字を大きく', `setFont(fontStep + 1); return true;`, true);
await step('字を小さく', `setFont(fontStep - 1); return true;`, true);
await step('字の大きさを戻す', `setFont(0); return fontStep;`, 0);
await step('左の列を畳む', `const was = railOff; toggleRail(); return railOff !== was;`, true);
await step('左の列を出す', `const was = railOff; toggleRail(); return railOff !== was;`, true);
await step('一覧を畳む', `const was = listOff; toggleList(); return listOff !== was;`, true);
await step('一覧を出す', `const was = listOff; toggleList(); return listOff !== was;`, true);

// 二。行き先と絞り込み
await step('フォルダへ', `state.dest = { kind: 'book', what: '仕事' }; drawRail(); drawList(); return true;`, true);
await step('タグへ', `state.dest = { kind: 'tag', what: '仕事' }; drawRail(); drawList(); return true;`, true);
await step('ブックマークへ', `state.dest = { kind: 'star', what: '' }; drawRail(); drawList(); return true;`, true);
await step('すべてのノートへ', `state.dest = { kind: 'all', what: '' }; drawRail(); drawList(); return true;`, true);
await step('並び順を回す', `
    for (let i = 0; i < ORDERS.length; i += 1) {
        order = ORDERS[(ORDERS.findIndex(([k]) => k === order) + 1) % ORDERS.length][0];
        drawOrder(); drawList();
    }
    return order.length > 0;`, true);
// **本文の中の言葉で引く。** 題で引くと「買い物」と「買い物リスト」の
// 二本に当たり、数で見張れない（部分一致はそれで正しい）。
// **一本にしか無い言葉で引く。** 「買い物」は題で二本に当たり、
// 「牛乳」は競合の控えにも入っている ── 数で見張るなら一本のものを。
await step('言葉で探す', `
    el('find').value = 'パン';
    el('find').dispatchEvent(new Event('input'));
    await new Promise((g) => setTimeout(g, 500));
    const n = shownNotes().length;
    el('find').value = '';
    el('find').dispatchEvent(new Event('input'));
    await new Promise((g) => setTimeout(g, 400));
    return n;`, 1);

// 三。タブ
await step('新しいタブで開く', `await openNote(${path('買い物.md')}, { tab: true }); return tabs.length >= 2;`, true);
await step('タブを行き来する', `
    const to = tabs[0];
    stashTab();
    showing = to.path;
    restoreTab(to);
    afterTab();
    return showing === to.path;`, true);
await step('タブを閉じる', `
    const n = tabs.length;
    await closeTab(tabs[tabs.length - 1].path);
    return tabs.length === n - 1;`, true);
await step('前に見たノート', `await walk(-1); return !!state.open;`, true);
await step('次に見たノート', `await walk(1); return !!state.open;`, true);

// 四。まとめて選ぶ
await step('すべて選ぶ', `pickAll(); return state.picked.size > 0;`, true);
await step('選びを解く', `unpickAll(); return state.picked.size;`, 0);

// 五。表示の面で書く（記号ぜんぶ）
await step('よくばりを開き直す', `await openNote(${path('よくばり.md')}); setView('read'); return view;`, 'read');
for (const [name] of JSON.parse(process.env.MARKS || '[]')) {
    // 帯の記号は、下でまとめて回す（名前は窓から取る）
    void name;
}
const marks = await run(`return MARKS.flat().filter((m) => m[0] !== '|' && m[2]).map((m) => m[0]);`);
for (const name of marks.value || []) {
    // 絵と図と絵文字は、窓の外（ファイル選び・工房・板）を開くので別に見る
    if (['画像', 'フロー', '注記', '絵文字'].includes(name)) continue;
    await step('表示の面：' + name, `
        const p = [...el('read').children].find((n) => n.tagName === 'P');
        if (p) { const r = document.createRange(); r.selectNodeContents(p); r.collapse(false);
                 const s = getSelection(); s.removeAllRanges(); s.addRange(r); el('read').focus(); }
        MARKS.flat().find((m) => m[0] === ${JSON.stringify(name)})[2]();
        return true;`, true);
}
await step('升を押す', `
    const b = el('read').querySelector('.box');
    if (!b) return 'なし';
    b.click();
    return true;`, true);

// 六。コードの面で書く
await step('コードの面へ', `setView('write'); return view;`, 'write');
for (const name of marks.value || []) {
    if (['画像', 'フロー', '注記', '絵文字'].includes(name)) continue;
    await step('コードの面：' + name, `
        editor.setPosition({ lineNumber: 3, column: 1 });
        MARKS.flat().find((m) => m[0] === ${JSON.stringify(name)})[2]();
        return true;`, true);
}
await step('一つ戻す', `editor.trigger('walk', 'undo'); return true;`, true);
await step('やり直す', `editor.trigger('walk', 'redo'); return true;`, true);
await step('表示の面へ戻す', `setView('read'); return view;`, 'read');

// 七。絵文字と絵の大きさ
await step('絵文字の板を出す', `await openEmoji(); return !el('emoji').hidden;`, true);
await step('絵文字を探す', `el('emojifind').value = 'おめでとう'; drawFaces();
    return document.querySelectorAll('#emojigrid button').length;`, 1);
await step('絵文字を入れる', `
    const p = [...el('read').children].find((n) => n.tagName === 'P');
    const r = document.createRange(); r.selectNodeContents(p); r.collapse(false);
    const s = getSelection(); s.removeAllRanges(); s.addRange(r); el('read').focus();
    document.querySelector('#emojigrid button').click();
    return el('read').textContent.includes('🎉');`, true);
await step('絵文字の板を閉じる', `closeEmoji(); return el('emoji').hidden;`, true);
await step('絵の大きさを変える', `
    const f = el('read').querySelector('figure');
    if (!f) return 'なし';
    await readSourceEdit(async (md) => (await ask('imgsize', { line: md, width: '200px' })).line, f);
    return whole().includes('w:200px');`, true);
await step('絵の大きさを戻す', `
    const f = el('read').querySelector('figure');
    if (!f) return 'なし';
    await readSourceEdit(async (md) => (await ask('imgsize', { line: md, width: null })).line, f);
    return !whole().includes('w:200px');`, true);

// 八。貼り付け（よそから来た HTML）
await step('ブラウザから貼る', `
    const r = el('read');
    const p = [...r.children].find((n) => n.tagName === 'P');
    const rg = document.createRange(); rg.selectNodeContents(p); rg.collapse(false);
    const s = getSelection(); s.removeAllRanges(); s.addRange(rg); r.focus();
    const dt = new DataTransfer();
    dt.setData('text/plain', '見出し\\n一つめ');
    dt.setData('text/html', '<h2>貼った見出し</h2><ul><li>一つめ</li></ul><pre><code>x</code></pre>');
    r.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
    await new Promise((go) => setTimeout(go, 900));
    return [...r.querySelectorAll('pre')].every((n) => n.dataset.md !== undefined);`, true);

// 九。ノートそのもの
await step('新しいノート', `const at = await newNote(); return !!at;`, true);
// 新しいノートの小窓（依頼 513・電話の形）── 題とタグを入れて作成／何も入れずに作成。
await step('新しいノートの小窓：タイトルとタグを入れて作成', `
    const p = cmdNewNote();
    await new Promise((g) => setTimeout(g, 300));
    if (el('newform').hidden) return '小窓が出ていません';
    el('nntitle').value = '小窓から作ったノート';
    el('nntag').value = '小窓';
    el('nnok').click();
    const at = await p;
    if (!at) return '作れませんでした';
    await new Promise((g) => setTimeout(g, 500));
    const n = state.notes.find((x) => x.path === at);
    if (!n || n.title !== '小窓から作ったノート') return '題が ' + (n && n.title);
    if (!(n.tags || []).includes('小窓')) return 'タグが付いていません: ' + JSON.stringify(n && n.tags);
    return true;`, true);
await step('新しいノートの小窓：何も入れずに作成でも作れる', `
    const p = cmdNewNote();
    await new Promise((g) => setTimeout(g, 300));
    el('nnok').click();
    const at = await p;
    return at ? true : '作れませんでした';`, true);
// 自動保存の入切（依頼 512）── 切ると打っても書かれず、「保存」が出る。
await step('自動保存：切ると打っても書かれず、「保存」を押すと書かれる', `
    await openNote(${path('買い物.md')});
    const was = autoSave;
    autoSave = false;
    editor.setValue(editor.getValue() + '\\n自動保存を切って書いた行');
    await new Promise((g) => setTimeout(g, 1600));
    const onDisk = (await ask('read', { path: state.open.path })).text;
    const shown = !el('savenow').hidden;
    const notWritten = !onDisk.includes('自動保存を切って書いた行');
    await save();
    const wrote = (await ask('read', { path: state.open.path })).text.includes('自動保存を切って書いた行');
    autoSave = was; drawSaveNow();
    if (!shown) return '「保存」が出ていません';
    if (!notWritten) return '切っているのに書かれました';
    if (!wrote) return '「保存」で書かれません';
    if (!el('savenow').hidden) return '入に戻したのに「保存」が残っています';
    return true;`, true);
await step('複製', `await cmdDup(); return state.notes.length > 0;`, true);
await step('題を直す', `
    await openNote(${path('買い物.md')});
    el('title').textContent = '買い物（直した）';
    await titleDone(true);
    return whole().includes('title: 買い物（直した）');`, true);
await step('題を戻す', `
    el('title').textContent = '買い物';
    await titleDone(true);
    return whole().includes('title: 買い物');`, true);
// 九の二。**ファイル名は題に合わせる**（依頼 492）── 欄から出た瞬間・離れたとき・同じ題。
await step('名前：題の欄から出た瞬間に、ファイル名が題になる', `
    nameAuto = true;
    try {
        await openNote(${path('買い物.md')});
        el('title').textContent = '買いもの';
        await titleDone(true);
        if (!state.open.path.endsWith('/買いもの.md')) return '道が ' + state.open.path;
        if (state.notes.some((n) => n.path.endsWith('/買い物.md'))) return '古い名前が一覧に残っています';
        el('title').textContent = '買い物';
        await titleDone(true);
        return state.open.path.endsWith('/買い物.md') ? true : '戻した道が ' + state.open.path;
    } finally { nameAuto = false; }`, true);
if (NOTES) {
    tally.ran += 1;
    try { readFileSync(NOTES + '/買い物.md', 'utf8'); } catch (e) { bad.push({ name: '名前：ファイルも戻っている', why: [e.message] }); }
}
await step('名前：一行目で題が決まるノートは、離れたときに名前が揃う', `
    nameAuto = true;
    try {
        await openNote(${path('改名.md')});
        await openNote(${path('よくばり.md')});
        const n = state.notes.find((x) => x.path.endsWith('/名前は一行目から.md'));
        if (!n) return '揃っていません: ' + state.notes.filter((x) => /改名|一行目/.test(x.path)).map((x) => x.path.split('/').pop()).join(' / ');
        return state.notes.some((x) => x.path.endsWith('/改名.md')) ? '古い名前が残っています' : true;
    } finally { nameAuto = false; }`, true);
await step('名前：同じ題は .2 になる', `
    nameAuto = true;
    try {
        await openNote(${path('二本目.md')});
        await openNote(${path('よくばり.md')});
        const n = state.notes.find((x) => x.path.endsWith('/買い物.2.md'));
        return n ? true : '無い: ' + state.notes.filter((x) => /買い物|二本目/.test(x.path)).map((x) => x.path.split('/').pop()).join(' / ');
    } finally { nameAuto = false; }`, true);

// **小窓を開ける命令は `await` しない。** ★ は置き場所を訊いてくる。
await step('ブックマークに登録', `
    cmdStar();
    await new Promise((g) => setTimeout(g, 500));
    const asked = !el('veil').hidden;
    closeSheet(null);
    return asked || starred(state.open);`, true);
await step('このノートをテンプレートにする', `
    await openNote(${path('買い物.md')});
    const before = state.notes.filter((n) => relOf(n.book) === TEMPLATES).length;
    const made = await cmdToTemplate();
    if (!made) return '写せません: ' + el('say').textContent;
    const after = state.notes.filter((n) => relOf(n.book) === TEMPLATES).length;
    return after === before + 1 ? true : '「テンプレート」が ' + before + ' → ' + after;`, true);
await step('見本のテンプレートを入れる（三枚・二度目は増えない）', `
    const a = await window.amber.templates(state.root);
    const b = await window.amber.templates(state.root);
    await reload({ quiet: true });
    const names = state.notes.filter((n) => relOf(n.book) === TEMPLATES).map((n) => n.title);
    if (a.put !== 3 || b.put !== 0) return JSON.stringify([a.put, b.put]);
    return ['週報', '議事録', '買い物リスト'].every((t) => names.includes(t)) ? true : names.join(' / ');`, true);
await step('テンプレートから作る', `
    const rows = state.notes.filter((n) => relOf(n.book) === TEMPLATES);
    if (!rows.length) return 'ひな型なし';
    const r = await ask('copy', { path: rows[0].path, dir: state.root });
    await reload({ quiet: true });
    await openNote(r.path);
    return !!state.open;`, true);

// 十。フォルダとタグ
await step('新しいフォルダ', `
    await ask('mkbook', { dir: state.root + '/歩き試し' });
    await reload({ quiet: true });
    // フォルダは絶対の道（依頼 511）。
    return state.books.includes(state.root + '/歩き試し');`, true);
await step('フォルダへ移す', `
    await openNote(${path('からっぽ.md')});
    await ask('move', { path: state.open.path, dir: state.root + '/歩き試し' });
    await reload({ quiet: true });
    return state.notes.some((n) => n.book === state.root + '/歩き試し');`, true);
await step('フォルダに色を付ける', `
    await ask('paint', { path: state.root, name: '歩き試し', color: '#D07A2E' });
    await reload({ quiet: true });
    return true;`, true);

// 十一。履歴と、見せるだけのもの
// **小窓を開ける命令は `await` しない。** 閉じる人がいないので返らない。
await step('過去バージョンを開く', `
    cmdHistory();
    await new Promise((g) => setTimeout(g, 500));
    const out = !el('veil').hidden;
    closeSheet(null);
    return out;`, true);
await step('マークダウンの書き方', `cmdSyntax(); await new Promise((g) => setTimeout(g, 200)); closeSheet(null); return true;`, true);
await step('ショートカット一覧', `cmdKeys(); await new Promise((g) => setTimeout(g, 200)); closeSheet(null); return true;`, true);
await step('何をしますか（パレット）', `
    palette();
    await new Promise((g) => setTimeout(g, 400));
    const n = document.querySelectorAll('#sheet .it').length;
    closeSheet(null);
    return n > 10;`, true);
await step('献立（⋯）を出す', `
    await openNote(${path('買い物.md')});
    openMenu({ x: 300, y: 300 });
    const n = document.querySelectorAll('#more button').length;
    closeMenu();
    return n > 3;`, true);
await step('設定の献立を出す', `
    openMenu({ x: 300, y: 300 }, 'app');
    const n = document.querySelectorAll('#more button').length;
    closeMenu();
    return n > 3;`, true);

// 十二。**右押し、ぜんぶ。** 窓には八か所ある ── どれも「押した瞬間に
// しか通らない道」で、単体の試験は一つも触っていない。
const RIGHT = [
    ['左の列の行き先', `el('rail').querySelector('.dest')`],
    ['一覧の行', `el('list').querySelector('.row')`],
    ['一覧の空きどころ', `el('list')`],
    ['左の列の空きどころ', `el('rail')`],
    ['帯の題', `el('title')`],
    ['読む面の字の上', `[...el('read').children].find((n) => n.tagName === 'P')`],
    ['読む面の表の上', `el('read').querySelector('td')`],
    ['読む面の升の上', `el('read').querySelector('.box')`],
    ['読む面のリンクの上', `el('read').querySelector('a')`],
];
await step('よくばりを開く（右押しのため）', `await openNote(${path('よくばり.md')}); setView('read'); return view;`, 'read');
for (const [name, pick] of RIGHT) {
    await step('右押し：' + name, `
        closeMenu();
        const n = ${pick};
        if (!n) return 'なし';
        n.dispatchEvent(new MouseEvent('contextmenu',
            { bubbles: true, cancelable: true, clientX: 300, clientY: 300 }));
        await new Promise((g) => setTimeout(g, 250));
        const out = document.querySelectorAll('#more button').length;
        closeMenu();
        return out > 0;`, true);
}
await step('右押し：タブ', `
    closeMenu();
    await openNote(${path('買い物.md')}, { tab: true });
    const d = el('strip').querySelector('.tab');
    if (!d) return 'なし';
    d.dispatchEvent(new MouseEvent('contextmenu',
        { bubbles: true, cancelable: true, clientX: 300, clientY: 300 }));
    await new Promise((g) => setTimeout(g, 250));
    const out = document.querySelectorAll('#more button').length;
    closeMenu();
    return out > 0;`, true);
await step('右押し：目次の見出し', `
    closeMenu();
    await openNote(${path('よくばり.md')});
    if (!tocOn) toggleToc();
    await new Promise((g) => setTimeout(g, 600));
    const h = el('toc').querySelector('.h');
    if (!h) return 'なし';
    h.dispatchEvent(new MouseEvent('contextmenu',
        { bubbles: true, cancelable: true, clientX: 300, clientY: 300 }));
    await new Promise((g) => setTimeout(g, 250));
    const out = document.querySelectorAll('#more button').length;
    closeMenu();
    if (tocOn) toggleToc();
    return out > 0;`, true);

// 十三。まとめて選んだときの献立（一括タグ・移す・消す）
await step('選んで献立を出す', `
    pickAll();
    pickedMenu({ x: 300, y: 300 });
    await new Promise((g) => setTimeout(g, 250));
    const out = document.querySelectorAll('#more button').length;
    closeMenu();
    unpickAll();
    return out > 2;`, true);

// 十三の二。**テーマぜんぶ**（依頼 495）── 琥珀の三つと、cian と同じ二十一。
// どれを着せても字と紙の色が分かれていて、着せ替えで例外が飛ばないこと。
await step('テーマ：二十四の配色ぜんぶ着せられる', `
    const was = theme;
    const out = [];
    for (const [k, n] of THEMES) {
        setTheme(k);
        await new Promise((g) => setTimeout(g, 30));
        const css = getComputedStyle(document.documentElement);
        const ink = css.getPropertyValue('--ink').trim();
        const paper = css.getPropertyValue('--paper').trim();
        if (!ink || !paper || ink === paper) out.push(n + ': ' + ink + '/' + paper);
    }
    setTheme(was);
    if (THEMES.length !== 24) return '配色が ' + THEMES.length + ' 種です（24 のはず）';
    return out.length ? out.join(' / ') : true;`, true);

// 十四。**残りの命令。** 小窓を開けるものは `await` しない。
const LATER = [
    ['タグ設定', `cmdTags();`],
    ['フォルダへ移動', `cmdMove();`],
    ['通知設定', `cmdRemind();`],
    ['テーマ', `cmdTheme();`],
    ['vim の入切', `cmdVim();`],
    ['行番号', `cmdLineNo();`],
    ['ambər について', `cmdAbout();`],
    ['ノートを探す', `openFind();`],
    ['期間で絞る', `openDrawer('when');`],
    // 同期 ── サインインしていなければ「Google でサインイン」の小窓が出て、閉じられる。
    ['同期', `cmdSync();`],
];
for (const [name, call] of LATER) {
    await step('命令：' + name, `
        await openNote(${path('よくばり.md')});
        ${call}
        await new Promise((g) => setTimeout(g, 400));
        closeSheet(null);
        closeMenu();
        if (typeof closeDrawer === "function") closeDrawer();
        return true;`, true);
}
await step('命令：現状バージョン保存', `await cmdKeepNow(); return true;`, true);

/// **訊いてくる命令は、返事をしてやる。** `await` すると返ってこない
/// （小窓が閉じられるのを待っている）── 呼びっぱなしにして、出た小窓に
/// 答える。答えないと、その先の道を一度も通らない。
///
/// **一度では足りない。** 「家族と共有する」は棚を作るかを訊いたあと、
/// もう一度**名前**を訊く ── 一度しか答えていなくて、途中で止まっていた
/// （走査で気づいた）。選ぶ小窓なら一つめを押し、打つ小窓なら字を入れる。
const answering = (call, then) => `
    ${call}
    for (let i = 0; i < 5; i += 1) {
        await new Promise((g) => setTimeout(g, 450));
        if (el('veil').hidden) break;
        const first = document.querySelector('#veil #sheet .items .it');
        if (first) { first.click(); continue; }
        const box = document.querySelector('#veil input');
        closeSheet(box && box.value ? box.value : '試し');
    }
    await new Promise((g) => setTimeout(g, 900));
    ${then}`;

await step('命令：家族と共有する', answering(
    `await openNote(${path('買い物.md')}); cmdToShare();`,
    `return state.notes.some((n) => n.shared);`), true);
// **見るのは、そのノート一本。** 棚を作った時点で、その中に元から
// 居たノート（`家族/買い物リスト.md`）も共有になる ── 「一本も共有されて
// いないこと」では、いつまでも真にならない（走査で気づいた）。
await step('命令：共有をやめる', answering(
    `const was = state.open.title; cmdToShare();`,
    `return !state.notes.some((n) => n.title === was && n.shared);`), true);
await step('命令：見本のノートを入れる', answering(
    `cmdWelcome();`,
    `return state.notes.length > 0;`), true);

// 十五。**うまくいかないとき。** ここが今まで一度も見られていなかった。
await step('無いノートを開く', `
    try { await openNote(state.root + '/ありません.md'); } catch { /* 断られてよい */ }
    return true;`, true);
await step('無い道を core に訊く', `
    try { await ask('read', { path: state.root + '/ありません.md' }); return '通った'; }
    catch { return true; }`, true);
await step('外へ出られないとき', `
    const r = await window.amber.fetchPage('http://127.0.0.1:1/');
    return typeof r.error === 'string';`, true);
await step('ページでない道', `
    const r = await window.amber.fetchPage('file:///etc/hosts');
    return typeof r.error === 'string';`, true);
await step('道の形になっていないもの', `
    const r = await window.amber.fetchPage('とりこんで');
    return typeof r.error === 'string';`, true);
await step('壊れた HTML を貼る', `
    const md = webToMd('<div><p>本文<span>途切れ', 'https://example.com/');
    return md.includes('本文');`, true);
await step('空を貼る', `return webToMd('', '') === '';`, true);
await step('競合の控えがあるノート', `
    const clash = state.notes.find((n) => /競合コピー/.test(n.path));
    if (!clash) return 'なし';
    await openNote(clash.path);
    return !!state.open;`, true);

// 十六。Web から取り込む（手元に立てたページがあるときだけ）
if (process.env.SITE) {
    await step('Web から取り込む', `
        const got = await window.amber.fetchPage(${JSON.stringify(process.env.SITE)});
        if (got.error) return got.error;
        // **窓の cmdClip と同じ呼び方で。** 別の呼び方で確かめると、
        // 呼び方が変わった日にここだけ古いまま通ってしまう
        // （この中に逆引用符は書けない ── 頭の注意書きのとおり）。
        const md = webToMd(bestPart(got.html), got.url);
        return md.includes('#') && md.length > 20;`, true);
}

/* ── 十六の二。**鍵だけで一周できるか**（依頼 447） ──
 *
 * 鍵は二か所に書いてある ── **見せる側**（CMDS の key）と、**効かせる側**
 * （keydown の if の並び）。別々なので、片方だけ直る日が来る: 献立にも
 * パレットにも出ているのに、押しても何も起きない鍵ができあがる。
 *
 * 見るのは二つ。**同じ鍵を二つの命令が名乗っていないこと**（先に書いた
 * ほうが勝ち、あとのほうは永久に押せない）と、**表に載っている鍵を押すと
 * 窓の姿が変わること**。
 */

await step('鍵：同じ鍵を、二つの命令が名乗っていない', `
    const seen = new Map();
    const dup = [];
    for (const c of CMDS) {
        if (!c.key) continue;
        if (seen.has(c.key)) dup.push(c.key + ' は ' + seen.get(c.key) + ' と ' + c.id);
        else seen.set(c.key, c.id);
    }
    return dup.length ? dup.join(' / ') : true;
`, true);

// **ここだけ長く待つ。** 四十本の鍵を一本ずつ、押す前に姿を戻して
// から押すので、既定の待ちでは足りない（待ちきれずに落第になった）。
const patience = process.env.PATIENCE;
process.env.PATIENCE = '60000';
await step('鍵：表に載っている鍵が、ぜんぶ効く', `
    // たどれる跡を作っておく ── 跡が無いと、前へ戻る鍵は正しく何もしない。
    for (const n of state.notes.slice(0, 2)) {
        await openNote(n.path);
        await new Promise((g) => setTimeout(g, 120));
    }
    const CODE = { '/': 'Slash', '←': 'ArrowLeft', '→': 'ArrowRight',
                   '+': 'Equal', '−': 'Minus', '0': 'Digit0', 'Esc': 'Escape' };
    const spec = (label) => {
        const meta = label.includes('⌘');
        const shift = label.includes('⇧');
        const alt = label.includes('⌥');
        const rest = label.replace(/[⌘⇧⌥ ]/g, '');
        const fkey = rest.charCodeAt(0) === 70 && rest.length > 1
            && !isNaN(Number(rest.slice(1)));
        if (fkey) return { code: rest, key: rest, meta, shift, alt };
        return { code: CODE[rest] || ('Key' + rest.toUpperCase()),
                 key: rest.toLowerCase(), meta, shift, alt };
    };
    const press = (label) => {
        const s = spec(label);
        document.dispatchEvent(new KeyboardEvent('keydown', {
            code: s.code, key: s.key, metaKey: s.meta, ctrlKey: false,
            shiftKey: s.shift, altKey: s.alt, bubbles: true, cancelable: true,
        }));
    };
    // **窓の姿。** 何が起きたかまでは見ない ── 見ようとすると命令ごとの
    // 見張りを四十本書くことになり、そちらが先に腐る。
    const snap = () => JSON.stringify({
        view, zen, fontStep, railOff, listOff, tocOn,
        veil: !el('veil').hidden, more: !el('more').hidden,
        emoji: !el('emoji').hidden, newform: !el('newform').hidden, open: state.open,
        tabs: (state.tabs || []).length, notes: state.notes.length,
        find: document.activeElement === el('find'),
        said: el('say').classList.contains('on') ? el('say').textContent : '',
        len: (state.open && editor) ? editor.getValue().length : 0,
    });
    const reset = () => {
        if (!el('emoji').hidden) closeEmoji();
        if (!el('more').hidden) closeMenu();
        if (!el('veil').hidden) closeSheet(null);
        if (!el('newform').hidden) el('nncancel').click();
        if (zen) setZen(false);
        setFont(0, true);
        if (railOff) toggleRail();
        if (listOff) toggleList();
        if (tocOn) toggleToc();
        setView('read');
        if (document.activeElement === el('find')) el('find').blur();
        el('say').classList.remove('on');
    };
    // **押す前に、効く余地を作る。** 押しても姿が変わらないのは
    // 「鍵が死んでいる」ときと「もう そうなっている」ときの二通りある
    // ── 後者で鳴らすと、この検査はすぐ信じられなくなる。
    const pause = (ms) => new Promise((g) => setTimeout(g, ms));
    const NOTE = ${path('よくばり.md')};
    await openNote(NOTE);
    await pause(150);
    const first = editor ? editor.getValue() : '';
    // 戻す先を積む。**開き直すと消える**ので、その鍵の直前にまく
    // （前へ戻る鍵が先に走って、ノートを開き直している）。
    // **二回いる** ── 一回目は「いまの姿」を憶えるだけで、積まれるのは
    // 二回目から（keepStep）。**書く面で**まかないと、保存は面の側の字を
    // 採るので、エディタに入れた字が書き込まれない。
    const seed = async () => {
        await openNote(NOTE);
        await pause(150);
        setView('write');
        await pause(150);
        for (const tail of ['x', 'xy']) {
            editor.setValue(first + tail);
            state.dirty = true;
            await save();
            await pause(200);
        }
    };
    const ready = {
        // 字の大きさは reset で 0 に戻るので、0 に戻す鍵だけ余地が要る。
        font0: async () => { setFont(2, true); },
        undo: seed,
        redo: async () => { await seed(); press('⌘Z'); await pause(240); },
    };
    const dead = [];
    for (const c of CMDS) {
        if (!c.key) continue;
        // **OS の小窓を開ける鍵は押さない** ── 閉じる人がいないので、
        // ここで総ざらいが止まる（頭の注意書きと同じ理由）。
        if (c.id === 'outside') continue;
        reset();
        if (ready[c.id]) await ready[c.id]();
        await new Promise((g) => setTimeout(g, 90));
        const before = snap();
        press(c.key);
        await new Promise((g) => setTimeout(g, 220));
        if (snap() === before) dead.push(c.id + ' ' + c.key);
    }
    reset();
    // 触った字は戻す ── このあとの往復が、崩れたノートで始まらないように。
    await openNote(NOTE);
    await pause(150);
    setView('write');
    await pause(120);
    if (editor && editor.getValue() !== first) {
        editor.setValue(first);
        state.dirty = true;
        await save();
        await pause(200);
    }
    return dead.length ? dead.join(' / ') : true;
`, true);
if (patience === undefined) delete process.env.PATIENCE; else process.env.PATIENCE = patience;

/* ── 十六の三。**ノートから使われていない画像**（依頼 449） ── */

await step('使われていない画像：指されている一枚は巻き込まない', `
    const got = await window.amber.call('spare', { path: state.root });
    const names = (got.pictures || []).map((p) => p.rel);
    if (names.some((n) => n.includes('1788000001'))) return '使っている画像が出ています';
    if (!names.some((n) => n.includes('1788000002'))) return '使っていない画像が出ていません';
    if ((got.unsure || []).length) return '読めないノートがあります: ' + got.unsure.join('・');
    return true;
`, true);

await step('使われていない画像：小さく並ぶ', `
    await cmdSpare();
    await new Promise((g) => setTimeout(g, 300));
    const cells = el('spare').querySelectorAll('.cell').length;
    const shots = el('spare').querySelectorAll('.cell img').length;
    el('spare').hidden = true;
    return cells > 0 && cells === shots;
`, true);

/* ── 十六の三の二。**引きずる帯に、押すものを埋めない**（依頼 477） ── */

await step('帯：押せるものが、窓を引きずる四角の中に埋まっていない', `
    // **none は「引きずらない」ではなく「切り抜かない」。** 親が drag の
    // 四角なら、何も書いていない子はその四角に含まれたままで、OS が先に
    // 押しを取る ── onclick は一度も鳴らない。
    //
    // **作った押しでは捕まらない。** el.click() は OS を通らないので、
    // 総ざらいはずっと素通りしていた（題が打てないのに「通りました」）。
    // 見るのは、押しの通り道ではなく**四角のほう**。
    const region = (x) => getComputedStyle(x).webkitAppRegion;
    const stuck = [];
    for (const x of document.querySelectorAll(
        'button, input, textarea, select, a[href], [contenteditable], .dest, .row, .it, .slot')) {
        if (x.closest('[hidden]')) continue;
        let at = x;
        while (at) {
            const r = region(at);
            if (r === 'no-drag') break;
            if (r === 'drag') {
                stuck.push('#' + (x.id || '') + '.' + (x.className || x.tagName));
                break;
            }
            at = at.parentElement;
        }
    }
    return stuck.length ? '引きずる帯の中に ' + stuck.length + ' 個: ' + stuck.slice(0, 5).join(' / ')
        : true;
`, true);

/* ── 十六の四。**カレンダー**（依頼 453） ── */

await step('カレンダー：左の列から開ける', `
    const row = el('rail').querySelector('.dest[data-kind="cal"]');
    if (!row) return '左の列にありません（パレットを知っている人しか辿り着けません）';
    if (row.textContent.trim() !== 'カレンダー') return '名前が ' + row.textContent.trim() + ' です';
    row.dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true }));
    await new Promise((g) => setTimeout(g, 800));
    const open = !el('cal').hidden;
    // **一覧は動かない** ── カレンダーは行き先ではないので、押しても
    // 一覧が空にならないこと。
    const kept = state.dest.kind !== 'cal';
    // **開いているあいだは、左の列のカレンダーが光る**（依頼 477）── そして
    // 行き先の光り（すべてのノート）は消える（依頼 510）。
    const lit = el('rail').querySelector('.dest[data-kind="cal"]').classList.contains('on');
    const other = [...el('rail').querySelectorAll('.dest.on')].filter((d) => d.dataset.kind !== 'cal').length;
    // **ノートと同じ場所に出る**（依頼 478）── 小窓ではないので、
    // ノートの面は引っ込んでいる。
    const wide = el('cal').closest('#pane') && el('work').hidden;
    calShut();
    if (!open) return '開きませんでした';
    if (!lit) return '左の列が光りません';
    if (other) return 'カレンダーを開いているのに、ほかの行き先も ' + other + ' つ光っています';
    if (!wide) return 'ノートと同じ場所に出ていません';
    if (el('rail').querySelector('.dest[data-kind="cal"]').classList.contains('on')) {
        return '閉じても光ったままです';
    }
    return kept ? true : '一覧の行き先まで変わりました';
`, true);

await step('カレンダー：ひと月ぶんが出る', `
    // **月から始める。** 見方は設定に憶えられているので、前に日や週で閉じて
    // いれば日や週で開く ── 升目の数が合わなくなる（実際になった・2026-09-11）。
    calView = 'month';
    calGroup = false;
    calMonth = { y: 2026, m: 9 };
    calDay = '2026-09-09';
    await cmdCalendar();
    await new Promise((g) => setTimeout(g, 400));
    if (el('cal').hidden) return '開きません';
    const cells = el('cal').querySelectorAll('.d[data-day]').length;
    if (cells !== 30) return '九月なのに ' + cells + ' 日あります';
    const one = calSlots.filter((s) => s.kind === 'once' && s.title === '面談');
    const rep = calSlots.filter((s) => s.kind === 'repeat' && s.title === '週報');
    if (one.length !== 1) return '一度きりが ' + one.length + ' 件です';
    if (rep.length !== 5) return '毎週水曜が ' + rep.length + ' 回です';
    return true;
`, true);

await step('カレンダー：日・週・月を切り替えられる', `
    calGroup = false;   // 設定に憶えられた「みんなの表」が入っていると、時間の表は出ない
    calMonth = { y: 2026, m: 9 };
    calDay = '2026-09-09';
    for (const v of ['week', 'day', 'month']) {
        const b = el('cal').querySelector('.seg button[data-view=' + JSON.stringify(v) + ']');
        if (!b) return v + ' の切り替えがありません';
        b.click();
        await new Promise((g) => setTimeout(g, 700));
        if (calView !== v) return v + ' に変わりません';
        const hours = !el('cal').querySelector('.hours').hidden;
        if (v === 'month' && hours) return '月なのに時間の表が出ています';
        if (v !== 'month' && !hours) return v + ' なのに時間の表が出ていません';
    }
    return true;
`, true);

await step('カレンダー：終日の段に、ノートは出さない', `
    calView = 'week';
    calDay = '2026-09-09';
    await drawCal();
    await new Promise((g) => setTimeout(g, 700));
    // 面談は remind: を持つので帯に出る。その日に書いたノートとしても
    // 数えられるが、終日の段には出さない（上と下に二度並ばない）。
    const ad = [...el('cal').querySelectorAll('.ad')].map((x) => x.textContent);
    const twice = ad.filter((t) => t.includes('面談')).length;
    calView = 'month';
    await drawCal();
    return twice === 0 ? true : '終日の段に ' + twice + ' 回出ています';
`, true);

await step('カレンダー：予定に出ているノートを、升目でもう一度出さない', `
    // 面談は remind: を持つので予定として出る。その日に書いたノートとしても
    // 数えられるが、同じ升目に二度並べない（二つあるように見える）。
    calView = 'month';
    calDay = '2026-09-09';
    await drawCal();
    await new Promise((g) => setTimeout(g, 700));
    const cell = el('cal').querySelector('.d[data-day="2026-09-09"]');
    if (!cell) return '九日の升目がありません';
    const hits = cell.textContent.split('面談').length - 1;
    return hits === 1 ? true : '面談が ' + hits + ' 回出ています';
`, true);

await step('カレンダー：升目を二度押しすると、その日に予定を足す小窓が出る', `
    calView = 'month'; calGroup = false; calMonth = { y: 2026, m: 9 }; calDay = '2026-09-09';
    await drawCal();
    const cell = el('cal').querySelector('.d[data-day="2026-09-17"]');
    cell.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, clientX: 10, clientY: 10 }));
    await new Promise((g) => setTimeout(g, 300));
    const open = !el('evform').hidden;
    const head = el('evform').querySelector('.hd').textContent;
    el('evcancel').click();
    if (!open) return '小窓が出ません';
    return head.includes('9/17') || head.includes('17日') || head.includes('09-17') ? true : '日が違います: ' + head;`, true);
await step('カレンダー：右押しすると「予定を追加」の献立が出る', `
    const cell = el('cal').querySelector('.d[data-day="2026-09-17"]');
    cell.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 300, clientY: 300 }));
    await new Promise((g) => setTimeout(g, 250));
    const rows = [...el('more').querySelectorAll('button')].map((b) => b.textContent);
    closeMenu();
    return rows.some((r) => r.includes('予定を追加')) ? true : JSON.stringify(rows);`, true);
await step('カレンダー：予定の上で右押しすると、直す道が出る', `
    const chip = el('cal').querySelector('.d[data-day="2026-09-09"] .ev[data-at]');
    if (!chip) return '面談の札がありません';
    chip.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 300, clientY: 300 }));
    await new Promise((g) => setTimeout(g, 250));
    const rows = [...el('more').querySelectorAll('button')].map((b) => b.textContent);
    closeMenu();
    return rows.some((r) => r.includes('ノートを開く')) && rows.some((r) => r.includes('予定を追加')) ? true : JSON.stringify(rows);`, true);
await step('カレンダー：土日を隠すと、月は五列・週は五本になる', `
    calWeekend = false;
    calView = 'month';
    await drawCal();
    const cells = el('cal').querySelectorAll('.d[data-day]').length;
    calView = 'week';
    await drawCal();
    const lanes = el('cal').querySelectorAll('.lane').length;
    calWeekend = true;
    calView = 'month';
    await drawCal();
    const back = el('cal').querySelectorAll('.d[data-day]').length;
    if (cells !== 22) return '九月の平日が ' + cells + ' 日あります（22 のはず）';
    if (lanes !== 5) return '週が ' + lanes + ' 本です';
    return back === 30 ? true : '戻すと ' + back + ' 日です';`, true);
await step('カレンダー：出さない予定表のものは、どの見方にも出ない', `
    calHide = ['me'];
    calView = 'month';
    await drawCal();
    // 予定の札（.ev.once）が消えていること ── 面談は「その日に書いたノート」としても
    // 並ぶので、字で見ると残る。
    const month = !!el('cal').querySelector('.ev.once, .ev.repeat');
    calView = 'week'; calDay = '2026-09-09';
    await drawCal();
    const week = !!el('cal').querySelector('.blk, .ad');
    calHide = [];
    calView = 'month';
    await drawCal();
    const back = !!el('cal').querySelector('.ev.once');
    if (month) return '隠したのに月に出ています';
    if (week) return '隠したのに週に出ています';
    return back ? true : '戻しても出ません';`, true);

await step('カレンダー：みんなの表は人ごとに色が違い、右押しで色を選べる', `
    calView = 'week'; calGroup = true; calDay = '2026-09-09';
    const was = teamPeople;
    teamPeople = [{ name: '花木', mail: 'hanaki@example.com' }, { name: '山田', mail: 'yamada@example.com' }, { name: '佐藤', mail: 'sato@example.com' }];
    await drawCal();
    const rows = [...el('cal').querySelectorAll('.crowd .ln')];
    const colors = rows.map((r) => r.style.getPropertyValue('--lane').trim());
    const who = el('cal').querySelector('.crowd .rows .who[data-key="team:hanaki@example.com"]');
    let menu = [];
    if (who) {
        who.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 300, clientY: 300 }));
        await new Promise((g) => setTimeout(g, 200));
        menu = [...el('more').querySelectorAll('button')].map((b) => b.textContent);
        closeMenu();
    }
    calColors = { 'team:hanaki@example.com': '#3b78c9' };
    await drawCal();
    const blue = el('cal').querySelector('.crowd .ln .who[data-key="team:hanaki@example.com"]');
    const got = blue && blue.closest('.ln').style.getPropertyValue('--lane').trim();
    teamPeople = was; calColors = {}; calGroup = false; calView = 'month';
    await drawCal();
    if (rows.length < 4) return '段が ' + rows.length + ' 本です';
    if (colors.some((c) => !c)) return '色の無い段があります: ' + JSON.stringify(colors);
    // 並んだ順に 青・ピンク・黄・緑・紫（五色を回す）。
    if (new Set(colors.slice(0, 4)).size !== 4) return '色が偏っています: ' + JSON.stringify(colors);
    if (colors[0] !== '#3b78c9' || colors[1] !== '#e0669c') return '並び順の色が違います: ' + JSON.stringify(colors);
    if (!menu.some((m) => m.includes('ピンク'))) return '色の献立が出ません: ' + JSON.stringify(menu);
    return got === '#3b78c9' ? true : '選んだ色になりません: ' + got;`, true);
await step('カレンダー：個人カレンダーの色を選ぶと、札の色が変わる', `
    calHereColor = '#e8702a'; paintHereColor();
    const v = getComputedStyle(document.documentElement).getPropertyValue('--cal-here').trim();
    calHereColor = ''; paintHereColor();
    const back = getComputedStyle(document.documentElement).getPropertyValue('--cal-here').trim();
    return v === '#e8702a' && back === '' ? true : JSON.stringify([v, back]);`, true);

await step('カレンダー：予定を足す小窓は、空のまま登録できず・終日なら時刻を選べない', `
    setTimeout(() => {
        el('evtitle').value = '';
        el('evok').click();
    }, 200);
    const p = askEvent('試し', '');
    await new Promise((g) => setTimeout(g, 400));
    const e1 = el('everr').hidden ? '' : el('everr').textContent;
    el('evtitle').value = '空の時刻';
    el('evstart').value = '';
    el('evok').click();
    await new Promise((g) => setTimeout(g, 100));
    const e2 = el('everr').hidden ? '' : el('everr').textContent;
    el('evall').checked = true;
    el('evall').dispatchEvent(new Event('change'));
    const off = el('evstart').disabled && el('evend').disabled;
    el('evok').click();
    const got = await p;
    if (!e1.includes('タイトル')) return 'タイトル無しで通っています: ' + JSON.stringify(e1);
    if (!e2.includes('開始')) return '時刻無しで通っています: ' + JSON.stringify(e2);
    if (!off) return '終日なのに時刻を選べます';
    return got && got.allDay && got.title === '空の時刻' && got.start === '' ? true : JSON.stringify(got);
`, true);

await step('カレンダー：予定を足すと、ノートが一本できる', `
    const was = state.notes.length;
    calDay = '2026-09-11';
    // 小窓は一枚（依頼 493）── タイトルを打ち、開始を選び、登録を押す。
    setTimeout(() => {
        el('evtitle').value = '走査の予定';
        el('evstart').value = '11:00';
        el('evstart').dispatchEvent(new Event('change'));
        if (el('evend').value !== '12:00') { closeSheet(null); el('evcancel').click(); return; }
        el('evok').click();
    }, 300);
    calAdd(calDay);
    // **出るまで待つ**（六秒まで）── 組み直しはノートの数で遅くなる。
    // 出なかったときは、何が出ていたかを言う（「出ていません」では直せない）。
    let made = [];
    const t0 = performance.now();
    for (let i = 0; i < 60; i += 1) {
        await new Promise((g) => setTimeout(g, 250));
        // **予定として出た一件を数える。** 同じ日に「書いたノート」としても
        // 並ぶ（作った日が今日なら）── そちらは数えない。2026-09-11 に走らせて
        // 初めて二件になり、日付に釣られる検査だと分かった。
        made = calSlots.filter((s) => s.day === '2026-09-11' && s.title === '走査の予定' && s.kind === 'once');
        if (made.length) break;
    }
    const took = Math.round(performance.now() - t0);
    if (state.notes.length !== was + 1) return 'ノートが増えていません（' + el('say').textContent + '）';
    // **三秒を超えたら遅い**（人が待てる上限・大きいノートの検査と同じ考え）。
    if (made.length === 1 && took > 3000) return '出るまで ' + took + ' ミリ秒かかりました（3000 まで）';
    if (made.length !== 1) {
        return '足した日に出ていません: ' + made.length + ' 件 hereOn=' + hereOn + ' 言い分=' + JSON.stringify(el('say').textContent)
            + ' その日=' + JSON.stringify(calSlots.filter((s) => s.day === '2026-09-11' && s.kind !== 'note')
                .map((s) => [s.kind, s.title, [...String(s.title)].map((c) => c.charCodeAt(0).toString(16)).join(' '), s.at, s.day]));
    }
    if (made[0].at !== '11:00') return '時刻が ' + made[0].at + ' です';
    calShut();
    return true;
`, true);

/* ── 十六の四の二。**みんなの予定を、人ごとに**（依頼 471） ── */

if (process.env.TEAMCSV) {
    await step('チームの予定表：読んでいなければ、その字ごと出ない', `
        // **初めて amber を開いた人の画面に、会社の話を出さない**
        // （依頼 473・475）。読んでいないときは「更新」も時点も出ない。
        teamFile = '';
        teamPlans = null;
        window.amber.remember({ teamFile });
        calMonth = { y: 2026, m: 9 };
        calDay = '2026-09-09';
        calView = 'day';
        calGroup = true;
        calHide = [];
        await cmdCalendar();
        await new Promise((g) => setTimeout(g, 1500));
        if (!el('cal').querySelector('.teamat').hidden) return '時点が出ています';
        if (!el('cal').querySelector('.teamnow').hidden) return '「更新」が出ています';
        if (teamTick) return '読みにいく約束が置かれています';
        return true;
    `, true);

    await step('グループカレンダー：CSV を読むと、人ごとの段ができる', `
        teamFile = ${JSON.stringify(process.env.TEAMCSV)};
        teamPlans = null;
        window.amber.remember({ teamFile });
        calMonth = { y: 2026, m: 9 };
        calDay = '2026-09-09';
        calView = 'day';
        calGroup = true;
        calHide = [];
        await cmdCalendar();
        await new Promise((g) => setTimeout(g, 1500));
        if (el('cal').querySelector('.crowd').hidden) return 'グループカレンダーが出ていません';
        const names = [...el('cal').querySelectorAll('.crowd .rows .who')]
            .map((x) => x.textContent.trim());
        for (const w of ['山田 武', '鈴木 一郎', '佐藤 花']) {
            if (!names.includes(w)) return w + ' の段がありません';
        }
        // **予定の無い人も段を持つ**（佐藤は「空き時間」しかない）。
        if (!names.includes('自分のノート')) return '自分の段がありません';
        return true;
    `, true);

    await step('チームの予定表：取り決めのややこしいところが、そのまま出る', `
        const team = calSlots.filter((s) => s.kind === 'team');
        // 件名の中の読点で、列がずれない。
        if (!team.some((s) => s.title === '設計、および見積')) return '読点で列がずれています';
        // 取り消された予定は出ない。
        if (team.some((s) => s.title === '消えた会議')) return '取り消した予定が出ています';
        // 「空き時間」は予定ではない。
        if (team.some((s) => s.title === 'あき')) return '空き時間が出ています';
        // 件名の見えない予定は、そうと分かる形で出る。
        const shut = team.filter((s) => s.shut);
        if (shut.length !== 1) return '非公開が ' + shut.length + ' 件です';
        if (shut[0].title !== '非公開') return '非公開の出し方が ' + shut[0].title + ' です';
        // **件名の取れない予定を「空」とは書かない** ── 直前で「空き時間」を
        // 落としているので、同じ字だと「空いている」と読める。
        if (team.some((s) => s.title === '空')) return '「空」と出ています';
        return true;
    `, true);

    await step('チームの予定表：いつ時点の紙かが、そのまま入口になる', `
        const at = el('cal').querySelector('.teamat');
        if (at.hidden) return 'いつ時点かが出ていません';
        if (!at.textContent.includes('9/9 08:15')) return '「' + at.textContent + '」としか出ていません';
        if (el('cal').querySelector('.teamnow').hidden) return '「更新」が出ていません';
        return true;
    `, true);

    await step('チームの予定表：日を替えても読み直さない', `
        // **前の月へ戻ったら紙が入れ替わっていた**、が画面の上でいちばん
        // 分かりにくい壊れ方（依頼 476）。日を替えても、持っている一枚から
        // 選び直すだけ ── ファイルは開かない。
        const sheet = teamPlans;
        const was = teamAt;
        calDay = '2026-09-16';
        calMonth = { y: 2026, m: 10 };
        await drawCal();
        await new Promise((g) => setTimeout(g, 1200));
        calMonth = { y: 2026, m: 9 };
        calDay = '2026-09-09';
        await drawCal();
        await new Promise((g) => setTimeout(g, 1200));
        if (teamPlans !== sheet) return '日を替えただけで読み直しました';
        return teamAt === was ? true : '時点が ' + teamAt + ' に変わりました';
    `, true);

    await step('チームの予定表：「更新」を押すと、読みにいく', `
        const sheet = teamPlans;
        el('cal').querySelector('.teamnow').click();
        await new Promise((g) => setTimeout(g, 2500));
        if (teamPlans === sheet) return '押しても読みにいきません';
        if (teamAt !== '9/9 08:15') return '時点が ' + teamAt + ' になりました';
        return teamPlans.length === sheet.length ? true : '中身が変わりました';
    `, true);

    await step('チームの予定表：次に読みにいくのは、毎時十分', `
        // 元の CSV は毎時零分に置き換わる ── 零分ちょうどに読むと、
        // 書いている途中の紙を読むことがある。
        const nine = nextTenPast(new Date('2026-09-10T09:00:00'));
        if (nine.getHours() !== 9 || nine.getMinutes() !== 10) return '九時零分の次が ' + nine;
        const past = nextTenPast(new Date('2026-09-10T09:10:00'));
        if (past.getHours() !== 10) return '九時十分ちょうどの次が ' + past;
        const late = nextTenPast(new Date('2026-09-10T23:40:00'));
        if (late.getHours() !== 0 || late.getDate() !== 11) return '日をまたげません: ' + late;
        return teamTick ? true : '約束が置かれていません';
    `, true);

    await step('チームの予定表：何日もある終日は、日ごとに出る', `
        calView = 'week';
        await drawCal();
        await new Promise((g) => setTimeout(g, 1200));
        const trip = calSlots.filter((s) => s.kind === 'team' && s.title === '出張');
        if (trip.length !== 2) return '出張が ' + trip.length + ' 日です';
        if (trip[0].day !== '2026-09-10') return '初日が ' + trip[0].day + ' です';
        if (trip.some((s) => s.day === '2026-09-12')) return '終わりの日まで出ています';
        return true;
    `, true);

    await step('チームの予定表：週は月をまたいでも白紙にならない', `
        // 九月二十八日からの週は十月に入る ── 九月ぶんだけ読むと、
        // 十月の四日が「予定が無い」ように見える。
        calView = 'week';
        calDay = '2026-09-30';
        await drawCal();
        await new Promise((g) => setTimeout(g, 1500));
        const oct = calMonths().filter((x) => x.m === 10);
        if (!oct.length) return '十月を読んでいません';
        return true;
    `, true);


    await step('グループカレンダー：人を絞れる', `
        calView = 'day'; calDay = '2026-09-09'; calHide = [];
        await drawCal(); await new Promise((g) => setTimeout(g, 900));
        const all = [...el('cal').querySelectorAll('.crowd .rows .who')];
        const one = all.find((x) => x.textContent.trim() === '山田 武');
        if (!one) return '山田 武 の段がありません';
        one.click();
        await new Promise((g) => setTimeout(g, 900));
        const now = [...el('cal').querySelectorAll('.crowd .rows .who')]
            .map((x) => x.textContent.trim());
        if (now.includes('山田 武')) return '押しても引っ込みません';
        if (!now.includes('鈴木 一郎')) return 'ほかの人まで消えました';
        const btn = el('cal').querySelector('.whobtn');
        if (btn.hidden) return '「人を選ぶ」が出ていません';
        if (!btn.textContent.includes('/')) return '何人中何人かが出ていません';
        // 戻す。
        calHide = [];
        await drawCal();
        await new Promise((g) => setTimeout(g, 900));
        return [...el('cal').querySelectorAll('.crowd .rows .who')]
            .map((x) => x.textContent.trim()).includes('山田 武')
            ? true : '戻せません';
    `, true);

    await step('グループカレンダー：目盛りと段の区切りが、同じところにある', `
        const c = el('cal').querySelector('.crowd');
        const ticks = c.querySelector('.ticks').getBoundingClientRect().x;
        const track = c.querySelector('.rows .track').getBoundingClientRect().x;
        // **名前の長さで段の欄が広がると、その段だけ目盛りがずれる。**
        return Math.abs(ticks - track) < 1
            ? true : '目盛りと段が ' + Math.round(track - ticks) + ' ずれています';
    `, true);

    await step('グループカレンダー：終日は「終日」と出る', `
        calDay = '2026-09-10';
        await drawCal(); await new Promise((g) => setTimeout(g, 900));
        const band = [...el('cal').querySelectorAll('.crowd .span')];
        if (!band.length) return '終日の帯がありません';
        if (!band.some((x) => x.textContent.includes('終日'))) return '「終日」と書いていません';
        if (band.some((x) => x.textContent.includes('00:00'))) return '00:00 と出ています';
        return true;
    `, true);

    await step('チームの予定表：合言葉を打つまで、一覧に出てこない', `
        // 「何をしますか」に並ぶ行を、そのまま組み立てて確かめる。
        const rows = CMDS.filter(canRun).map((c) => ({ name: c.name, word: c.word }));
        const secret = rows.filter((r) => r.word);
        if (secret.length !== 2) return '合言葉つきが ' + secret.length + ' 件です';
        const shown = (q) => rows.filter((i) => (i.word ? q.includes(i.word)
            : !q || i.name.toLowerCase().includes(q)));
        if (shown('').some((r) => r.word)) return '何も打たないのに出ています';
        if (shown('よてい').some((r) => r.word)) return '「よてい」で出てしまいます';
        if (shown(TEAM_WORD).filter((r) => r.word).length !== 2) return '合言葉で出てきません';
        return true;
    `, true);

    await step('チームの予定表：読むのをやめられる', `
        await cmdTeamOff();
        await new Promise((g) => setTimeout(g, 1200));
        if (teamFile) return 'まだ読んでいます';
        // **読んでいない人のカレンダーには、会社の話が一つも出ない。**
        if (!el('cal').querySelector('.teamat').hidden) return '入口が出たままです';
        if (!el('cal').querySelector('.teamnow').hidden) return '「更新」が出たままです';
        if (teamTick) return '読みにいく約束が残っています';
        if (calSlots.some((s) => s.kind === 'team')) return 'まだ予定が残っています';
        calGroup = false;
        calView = 'month';
        calShut();
        return true;
    `, true);
}

/* ── 十六の五。**よその予定表**（依頼 456） ── */

if (process.env.SITE) {
    await step('よその予定表：読むようにする', `
        // **何も読んでいないところから始める。** 設定は本物のほうに
        // 書かれる（macOS の Electron は appData に $HOME を見ない）ので、
        // 前に手で試したものが残っていることがある ── 実際に残っていた。
        away = [];
        window.amber.remember({ away });
        setTimeout(() => closeSheet(${JSON.stringify(process.env.SITE + 'away.ics')}), 250);
        await cmdSubscribe();
        await new Promise((g) => setTimeout(g, 1200));
        if (away.length !== 1) return '購読が ' + away.length + ' 件です';
        return away[0].name === '家の予定' ? true : '名前が ' + away[0].name + ' です';
    `, true);

    await step('よその予定表：一度きり・またぐもの・毎週が並ぶ', `
        calMonth = { y: 2026, m: 9 };
        calDay = '2026-09-21';
        await cmdCalendar();
        await new Promise((g) => setTimeout(g, 1500));
        const out = calSlots.filter((s) => s.kind === 'away');
        const trip = out.filter((s) => s.title === '旅行').map((s) => s.day);
        const bin = out.filter((s) => s.title === 'ごみ出し');
        if (trip.length !== 3) return '終日でまたぐものが ' + trip.length + ' 日です';
        if (trip[0] !== '2026-09-21') return 'またぐ初日が ' + trip[0] + ' です';
        if (bin.length !== 4) return '毎週月曜が ' + bin.length + ' 回です';
        const one = out.find((s) => s.title === '歯医者');
        if (!one || one.at !== '18:30') return '時刻が読めていません';
        return true;
    `, true);

    await step('よその予定表：押しても、直せるふりをしない', `
        calView = 'week';
        calGroup = false;
        calDay = '2026-09-21';
        await drawCal();
        await new Promise((g) => setTimeout(g, 900));
        const ad = el('cal').querySelector('.ad.away');
        if (!ad) return 'よその予定が出ていません';
        if (ad.dataset.at) return 'ノートが無いのに、開く先を持っています';
        ad.click();
        await new Promise((g) => setTimeout(g, 300));
        calView = 'month';
        await drawCal();
        return el('say').textContent.includes('直せません') ? true : '何も言いません';
    `, true);

    await step('よその予定表：読むのをやめる', `
        setTimeout(() => closeSheet(away[0].url), 250);
        await cmdUnsubscribe();
        await new Promise((g) => setTimeout(g, 600));
        calShut();
        return away.length === 0 ? true : 'まだ ' + away.length + ' 件あります';
    `, true);
}

/* ── 十六の六。**ノートの途中に書き込む**（依頼 461） ──
 *
 * **これまで、入れるものは端でしか確かめていなかった。** 端に入るなら
 * 途中にも入るだろう、は成り立たない ── 押した瞬間に焦点がボタンへ移り、
 * caret が先頭へ落ちるからで、それは端に居るときには目に見えない。
 * 本人が見つけた（ノートの途中で絵文字を入れたら、頭に入った）。
 */

/// 読む面の、長い段落の「途中」に caret を置く。
const midway = `
    setView('read');
    await new Promise((g) => setTimeout(g, 350));
    const box = el('read');
    const walk = document.createTreeWalker(box, NodeFilter.SHOW_TEXT);
    let node = null;
    while (walk.nextNode()) {
        if (walk.currentNode.data.trim().length > 12) { node = walk.currentNode; break; }
    }
    if (!node) return 'ノートに、途中を作れる行がありません';
    const line = node.parentElement;
    const half = 6;
    const was = line.textContent;
    const spot = document.createRange();
    spot.setStart(node, half);
    spot.collapse(true);
    box.focus();
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(spot);
    document.dispatchEvent(new Event('selectionchange'));
`;

await step('途中に書く：絵文字が、caret のところに入る', `
    await openNote(${path('途中.md')});
    ${midway}
    // **人が押したときと同じ形にする** ── 板を開くと焦点は板の欄へ移り、
    // 読む面の選択は消える。ここで caret を憶えていないと頭に入る。
    await openEmoji();
    await new Promise((g) => setTimeout(g, 300));
    if (document.activeElement === box) return '板を開いても焦点が移っていません（試しになりません）';
    putFace('😀');
    await new Promise((g) => setTimeout(g, 250));
    closeEmoji();
    const now = line.textContent;
    if (now === was) {
        const at = el('read').textContent.indexOf('😀');
        return at < 0 ? '入りませんでした'
            : 'ちがうところに入りました（ノートの ' + at + ' 文字目）';
    }
    if (now.startsWith('😀')) return 'その行の頭に入りました';
    if (el('read').textContent.indexOf('😀') < 6) return 'ノートの頭に入りました';
    if (now !== was.slice(0, half) + '😀' + was.slice(half)) {
        return 'caret のところではありません: ' + now.slice(0, 24);
    }
    return true;
`, true);

await step('途中に書く：記号も、caret のところに効く', `
    await openNote(${path('途中.md')});
    ${midway}
    // 途中の三文字を選んでから太字にする。**帯のボタンは焦点を奪わない**
    // （依頼 204）ので、選んだままで効くのが正しい姿。
    const pick = document.createRange();
    pick.setStart(node, half);
    pick.setEnd(node, Math.min(node.data.length, half + 3));
    sel.removeAllRanges();
    sel.addRange(pick);
    document.execCommand('bold');
    const bold = line.querySelector('b, strong');
    if (!bold) return '太字になりませんでした';
    if (line.textContent.startsWith(bold.textContent)) return '行の頭が太字になりました';
    if (!was.includes(bold.textContent)) return '選んでいない字が太字になりました';
    return true;
`, true);

/* ── 十六の七。**この機械の予定表**（依頼 462） ── */

await step('この機械の予定表：口が繋がっている', `
    const got = await window.amber.cal(['month', 2026, 9]);
    if (!got) return '返事がありません';
    // 許可が無ければ「無い」と言うのが正しい姿 ── 落ちないことを見る。
    if (got.error) return typeof got.error === 'string' ? true : '答えの形が違います';
    return Array.isArray(got.days) ? true : '日の一覧がありません';
`, true);

await step('この機械の予定表：許可が無くても、カレンダーは開く', `
    hereOn = false;
    calMonth = { y: 2026, m: 9 };
    await cmdCalendar();
    await new Promise((g) => setTimeout(g, 700));
    const open = !el('cal').hidden;
    const mine = calSlots.filter((s) => s.kind !== 'here').length;
    calShut();
    if (!open) return '開きませんでした';
    return mine > 0 ? true : '自分の予定まで消えました';
`, true);

/* ── 十七。**触ったあと、壊れていないか** ──
 *
 * ここがこの走査のいちばんの目当て。**面を行き来しただけで字が変わる**、
 * が実際にあった（2026-09-08・段落の改行が空白に、`*` の点が `-` に、
 * `1. 1.` が `1. 2.` に…）。同期しているフォルダなら、それが全部むこうへ
 * 差分として飛ぶ。
 *
 * 見張りは一つ ── **何もしない往復では、字が一文字も変わらない。**
 */

/// 面を行き来して、字が変わっていないかを見る。
const trip = (name, prepare) => step('往復：' + name, `
    await openNote(${path('往復.md')});
    // **毎回、元の字から始める。** 前の往復が崩したノートで次を回すと、
    // 崩れたもの同士を比べて「変わっていません」になる（一度そうなった）。
    if (window.__pristine === undefined) {
        window.__pristine = whole();
    } else if (whole() !== window.__pristine) {
        setView('write');
        await new Promise((g) => setTimeout(g, 250));
        loading = true;
        editor.setValue(state.head ? window.__pristine.slice(state.head.length) : window.__pristine);
        loading = false;
        state.dirty = true;
        await save();
    }
    setView('read');
    await new Promise((g) => setTimeout(g, 600));
    ${prepare || ''}
    const was = whole();
    for (let i = 0; i < 3; i += 1) {
        setView('write');
        await new Promise((g) => setTimeout(g, 250));
        setView('read');
        await new Promise((g) => setTimeout(g, 400));
        // **書き戻しを、必ず一度通す。**
        //
        // 面を替えるだけでは書き戻しが走らない ── 替えただけの往復は何も
        // 確かめていなかった（前後の空白を落とす壊し方を入れても鳴らな
        // かった。変異させて初めて分かった・2026-09-09）。人が一文字
        // 打った時と同じ合図を出して、面の字をノートへ返させる。
        el('read').dispatchEvent(new Event('input'));
        await new Promise((g) => setTimeout(g, 1100));
    }
    const now = whole();
    if (now === was) return true;
    // どこが変わったかを言う ── 「変わりました」だけでは直せない。
    //
    // **改行は数で書く。** ここは窓へ送る字（テンプレート）の中なので、
    // 円記号で書くと走査の側で本物の改行になり、送る字が途中で切れる。
    // 逆引用符も同じ理由で書けない ── そこでテンプレートが閉じる。
    const nl = String.fromCharCode(10);
    const a = was.split(nl);
    const b = now.split(nl);
    for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
        if (a[i] !== b[i]) return (i + 1) + ' 行目: ' + JSON.stringify(a[i]) + ' → ' + JSON.stringify(b[i]);
    }
    return '長さが違います（' + a.length + ' → ' + b.length + '）';`, true);

await trip('何もしないで三往復');
await trip('並べて表示をはさむ', `
    setView('split');
    await new Promise((g) => setTimeout(g, 400));
    setView('read');
    await new Promise((g) => setTimeout(g, 400));`);
await trip('一文字打ってから', `
    const p = [...el('read').children].find((n) => n.tagName === 'P');
    const r = document.createRange(); r.selectNodeContents(p); r.collapse(false);
    const s = getSelection(); s.removeAllRanges(); s.addRange(r);
    el('read').focus();
    document.execCommand('insertText', false, 'あ');
    await new Promise((g) => setTimeout(g, 1200));`);
await trip('升を押してから', `
    const tick = el('read').querySelector('.box');
    if (tick) tick.click();
    await new Promise((g) => setTimeout(g, 1200));`);
await trip('絵文字を入れてから', `
    await openEmoji();
    el('emojifind').value = 'おめでとう'; drawFaces();
    const p = [...el('read').children].find((n) => n.tagName === 'P');
    const r = document.createRange(); r.selectNodeContents(p); r.collapse(false);
    const s = getSelection(); s.removeAllRanges(); s.addRange(r);
    el('read').focus();
    document.querySelector('#emojigrid button').click();
    closeEmoji();
    await new Promise((g) => setTimeout(g, 1200));`);
await trip('絵の大きさを変えてから', `
    const f = el('read').querySelector('figure');
    if (f) await readSourceEdit(async (md) =>
        (await ask('imgsize', { line: md, width: '400px' })).line, f);
    await new Promise((g) => setTimeout(g, 800));`);
await trip('ブラウザから貼ってから', `
    const r0 = el('read');
    const p = [...r0.children].find((n) => n.tagName === 'P');
    const rg = document.createRange(); rg.selectNodeContents(p); rg.collapse(false);
    const s = getSelection(); s.removeAllRanges(); s.addRange(rg);
    r0.focus();
    const dt = new DataTransfer();
    dt.setData('text/plain', '見出し');
    dt.setData('text/html', '<h3>貼った見出し</h3><ul><li>一つ</li></ul>');
    r0.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
    await new Promise((g) => setTimeout(g, 1200));`);
await trip('記号を付けてから', `
    const p = [...el('read').children].find((n) => n.tagName === 'P');
    const r = document.createRange(); r.selectNodeContents(p); r.collapse(false);
    const s = getSelection(); s.removeAllRanges(); s.addRange(r);
    el('read').focus();
    MARKS.flat().find((m) => m[0] === '引用')[2]();
    await new Promise((g) => setTimeout(g, 1200));`);

// **ノートを替えても混ざらない**（`switch-test` の実物版）。
await step('往復：ノートを替えても混ざらない', `
    await openNote(${path('よくばり.md')});
    setView('write');
    await new Promise((g) => setTimeout(g, 300));
    const a = whole();
    await openNote(${path('買い物.md')});
    await new Promise((g) => setTimeout(g, 400));
    const b = whole();
    await openNote(${path('よくばり.md')});
    await new Promise((g) => setTimeout(g, 400));
    if (whole() !== a) return 'よくばりの字が変わりました';
    await openNote(${path('買い物.md')});
    await new Promise((g) => setTimeout(g, 400));
    if (whole() !== b) return '買い物の字が変わりました';
    setView('read');
    return true;`, true);

/* ── 十八。**よそから来た形のノートを、そのまま返すか**（依頼 429）──
 *
 * Windows で作られたノート（CRLF）、BOM 付き、古い日本語（Shift_JIS）。
 * core は読んだときの形のまま書き戻すが、**窓を通したときもそうか**は
 * 誰も見ていなかった。開いて、打った時と同じ合図を出して、保存させてから
 * **バイトを見る**（画面では分からない）。
 */
const SHAPES = [
    ['改行CRLF.md', (b) => b.includes('\r\n'), 'CRLF が LF になりました'],
    ['BOM付き.md', (b) => b[0] === 0xef && b[1] === 0xbb && b[2] === 0xbf, 'BOM が落ちました'],
    // Shift_JIS は UTF-8 では出ない並び ── 「本」は 0x96 0x7b。
    ['日本語SJIS.md', (b) => b.includes(Buffer.from([0x96, 0x7b])), 'Shift_JIS が UTF-8 になりました'],
];
for (const [name, ok, why] of SHAPES) {
    // **中身を本当に変える。** 合図だけでは保存が走らない（同じ字なら
    // 書かない）── 壊しても鳴らなかったのはそれだった。一文字入れる。
    await step('形を保つ：' + name, `
        await openNote(state.root + '/' + ${JSON.stringify(name)});
        // **開けたことを、先に確かめる。** 開けないと前のノートが開いた
        // ままで、そこに打って「通った」になる ── 実際にそうなって、
        // 一覧から消えているノートを見落としかけた。
        if (!state.open || !state.open.path.endsWith(${JSON.stringify(name)})) {
            return '一覧にありません（開けませんでした）';
        }
        setView('read');
        await new Promise((g) => setTimeout(g, 500));
        const p = [...el('read').children].find((n) => n.tagName === 'P');
        const r = document.createRange(); r.selectNodeContents(p); r.collapse(false);
        const s = getSelection(); s.removeAllRanges(); s.addRange(r);
        el('read').focus();
        document.execCommand('insertText', false, 'あ');
        await new Promise((g) => setTimeout(g, 1500));
        return whole().includes('本文です。あ');`, true);
    if (!NOTES) continue;
    tally.ran += 1;
    try {
        const bytes = readFileSync(NOTES + '/' + name);
        if (!ok(bytes)) bad.push({ name: 'バイト：' + name, why: [why] });
    } catch (e) {
        bad.push({ name: 'バイト：' + name, why: ['読めません: ' + e.message] });
    }
}

/* ── 十九。**大きいノートでも保つか**（依頼 431）──
 *
 * 一万二千行。開く・面を替える・打つ・保存する、それぞれに**時間の上限**を
 * 置く ── 速さは一度測ったきりで、遅くなったことに気づく仕掛けが無かった。
 * 上限は「人が待てるか」で決める（開くのに三秒かかったら、もう道具ではない）。
 */
/// 測った時間。**通っても出す** ── 遅くなっていく気配は、落第になる前に
/// 見えていたほうがよい。
const times = [];
const timed = (name, limit, src) => step('大きいノート：' + name, `
    const t0 = performance.now();
    ${src}
    return Math.round(performance.now() - t0);`, (ms) => {
    if (typeof ms !== 'number') return String(ms);
    times.push(`${name}: ${ms} ミリ秒（${limit} まで）`);
    return ms < limit ? true : `${ms} ミリ秒かかりました（${limit} まで）`;
});

await timed('開く', 3000, `
    await openNote(${path('大きいノート.md')});
    if (!state.open || !state.open.path.endsWith('大きいノート.md')) return '開けません';
    setView('read');
    await new Promise((g) => setTimeout(g, 100));`);
await timed('コードの面へ', 2000, `setView('write'); await new Promise((g) => setTimeout(g, 100));`);
await timed('表示の面へ', 3000, `setView('read'); await new Promise((g) => setTimeout(g, 100));`);
await step('大きいノート：打って保存する', `
    const p = [...el('read').children].find((n) => n.tagName === 'P');
    const r = document.createRange(); r.selectNodeContents(p); r.collapse(false);
    const s = getSelection(); s.removeAllRanges(); s.addRange(r);
    el('read').focus();
    const t0 = performance.now();
    document.execCommand('insertText', false, 'あ');
    await new Promise((g) => setTimeout(g, 2500));
    const ms = Math.round(performance.now() - t0);
    if (!whole().includes('あ')) return '打った字が残っていません';
    return ms < 2600 ? true : ms + ' ミリ秒かかりました';`, true);
await step('大きいノート：目次も出る', `
    if (!tocOn) toggleToc();
    await new Promise((g) => setTimeout(g, 1500));
    const n = el('toc').querySelectorAll('.h').length;
    if (tocOn) toggleToc();
    return n > 100;`, true);

/* ── 二十。**同じノートを二か所から書き換える**（依頼 433）──
 *
 * クラウドで同じ棚を触っていると起きること。amber は**どちらも捨てない**
 * ── 分かれる前・こちら・向こうの三つを core に渡して混ぜる。ここまでは
 * core の試験が見ているが、**窓を通した本物**は誰も通していなかった。
 *
 * 走査が「向こうの端末」の役をやる: 窓が開いたままのノートを、横から
 * 書き換える。
 */
await step('混ぜる：開く', `
    await openNote(${path('混ぜる.md')});
    if (!state.open || !state.open.path.endsWith('混ぜる.md')) return '開けません';
    setView('write');
    await new Promise((g) => setTimeout(g, 400));
    return whole().includes('はじめの行');`, true);

if (NOTES) {
    // **向こうの端末が、末尾に一行足した。**
    tally.ran += 1;
    try {
        const at = NOTES + '/混ぜる.md';
        const was = readFileSync(at, 'utf8');
        writeFileSync(at, was.replace('おわりの行。', 'おわりの行。\n\n向こうが足した行。'));
    } catch (e) {
        bad.push({ name: '混ぜる：横から書き換える', why: [e.message] });
    }
    await sleep(900);
}

await step('混ぜる：こちらでも打って、両方残る', `
    // こちらは頭のほうに足す ── 同じ行を取り合わない形。
    const was = whole();
    const now = was.replace('はじめの行。', 'はじめの行。こちらが足した字。');
    loading = true;
    editor.setValue(state.head ? now.slice(state.head.length) : now);
    loading = false;
    state.dirty = true;
    await save();
    await new Promise((g) => setTimeout(g, 1200));
    const out = whole();
    if (!out.includes('こちらが足した字')) return 'こちらの字が消えました';
    if (!out.includes('向こうが足した行')) return '向こうの行が消えました';
    return true;`, true);

if (NOTES) {
    tally.ran += 1;
    try {
        const got = readFileSync(NOTES + '/混ぜる.md', 'utf8');
        if (!got.includes('こちらが足した字') || !got.includes('向こうが足した行')) {
            bad.push({ name: '混ぜる：ファイルにも両方ある',
                why: ['ファイルには片方しかありません: ' + JSON.stringify(got.slice(0, 200))] });
        }
    } catch (e) {
        bad.push({ name: '混ぜる：ファイルにも両方ある', why: [e.message] });
    }
}

/* ── 二十の一の二。**同じ行を両方で直した**（依頼 487・網の決めごと） ──
 *
 * 別々の場所なら黙って混ざる（上）。同じ行なら**両方残して、帯と選び口が
 * 出て、人が選ぶ**。選んだら片方が消えて、帯が消える。
 */
// **先にこちらを打ちかけにしてから、向こうが書く。** 打ちかけでないと、
// 窓は外の変わりを黙って拾い直す（依頼 480）ので、ぶつかりようがない。
await step('同じ行：こちらで同じ行を打ちかけにする', `
    const was = whole();
    if (!was.includes('おわりの行。')) return '「おわりの行。」が見当たりません';
    const now = was.replace('おわりの行。', 'おわりの行。こちらが直した。');
    loading = true;
    editor.setValue(state.head ? now.slice(state.head.length) : now);
    loading = false;
    state.dirty = true;
    return true;`, true);
if (NOTES) {
    tally.ran += 1;
    try {
        const at = NOTES + '/混ぜる.md';
        const was = readFileSync(at, 'utf8');
        writeFileSync(at, was.replace('おわりの行。', 'おわりの行。向こうが直した。'));
    } catch (e) {
        bad.push({ name: '同じ行：横から書き換える', why: [e.message] });
    }
    await sleep(2800);
}
await step('同じ行：保存すると両方残り、帯と選び口が出る', `
    state.dirty = true;
    await save();
    await new Promise((g) => setTimeout(g, 1500));
    const out = whole();
    if (!out.includes('おわりの行。こちらが直した。')) return 'こちらの行が消えました';
    if (!out.includes('おわりの行。向こうが直した。')) return '向こうの行が消えました（両方残るはず）';
    if (!incoming || !incoming.spots || incoming.spots.length !== 1) {
        return 'ぶつかった場所が ' + (incoming && incoming.spots ? incoming.spots.length : 'なし') + ' か所です（1 のはず）';
    }
    if (el('band').hidden) return '帯が出ていません';
    if (!el('band').textContent.includes('か所')) return '帯が数を言っていません: ' + el('band').textContent;
    setView('read');
    await new Promise((g) => setTimeout(g, 700));
    const g = el('read').querySelector('.gadget');
    if (!g) return '読む面に選び口が出ていません';
    if (!g.querySelector('button')) return '選び口にボタンがありません';
    // **選び口の字は、書き戻しに混ざらない。**
    const back = paperToMd(el('read'), state.head);
    if (back === null) return '選び口を置いたら字に戻せなくなりました';
    if (back.includes('こちらを残す')) return '選び口の字が本文に混ざります';
    return true;`, true);
await step('同じ行：「こちらを残す」を押すと、向こうの行が消えて帯も消える', `
    const g = el('read').querySelector('.gadget');
    if (!g) return '選び口がありません';
    g.querySelector('button[data-w="ours"]').click();
    await new Promise((g) => setTimeout(g, 1500));
    const out = whole();
    if (out.includes('おわりの行。向こうが直した。')) return '向こうの行が残っています';
    if (!out.includes('おわりの行。こちらが直した。')) return 'こちらの行まで消えました';
    if (el('read').querySelector('.gadget')) return '選び口が残っています';
    if (!el('band').hidden && el('band').textContent.includes('か所')) return '帯が「か所」のまま残っています';
    return true;`, true);
if (NOTES) {
    tally.ran += 1;
    try {
        const got = readFileSync(NOTES + '/混ぜる.md', 'utf8');
        if (got.includes('向こうが直した') || !got.includes('こちらが直した')) {
            bad.push({ name: '同じ行：ファイルにも選んだほうだけ', why: ['ファイル: ' + JSON.stringify(got.slice(0, 200))] });
        }
    } catch (e) {
        bad.push({ name: '同じ行：ファイルにも選んだほうだけ', why: [e.message] });
    }
}

/* ── 二十の二。**二台で同じフォルダを触る**（依頼 479） ── */

if (NOTES) {
    // **「向こう」は node 側が演じる。** 歩みの本体は窓の中で走るので、
    // ファイルを直に書けるのはこちらだけ ── 電話がフォルダに書いたのと
    // 同じことを、ここでやる。
    const other = NOTES + '/二台目.md';
    const wrote = (line) => writeFileSync(other,
        '---\ncreated: 2026-09-05\n---\n# 二台目\n\n' + line + '\n');

    wrote('はじめの字。');
    await sleep(1800);

    await step('二台目：向こうが置いたノートが、こちらに出る', `
        const n = state.notes.find((x) => x.title === '二台目');
        if (!n) return '置いたノートが一覧に出ません';
        await openNote(n.path);
        await new Promise((g) => setTimeout(g, 1200));
        return whole().includes('はじめの字') ? true : '開けていません';
    `, true);

    wrote('向こうが直した字。');
    await sleep(2500);

    await step('二台目：開いているノートが外で変わったら、拾い直す', `
        // **黙って古いまま残る**のがいちばん悪い ── 見ている人に
        // 気づく手立てが無い。
        const now = whole();
        return now.includes('向こうが直した字')
            ? true : '古いまま残っています: ' + JSON.stringify(now.slice(0, 120));
    `, true);

    await step('二台目：こちらで保存する', `
        loading = true;
        editor.setValue('# 二台目\\n\\nこちらで保存した字。\\n');
        loading = false;
        state.dirty = true;
        await save();
        await new Promise((g) => setTimeout(g, 2000));
        // **打ち消しを憶えたままにしない。** 前は「最後に自分で書いた道」を
        // 消さなかったので、そのノートが外で何度変わっても無視し続けた。
        return lastWrote ? '保存の跳ね返りを、まだ待ち構えています' : true;
    `, true);

    wrote('保存のあとに向こうが直した。');
    await sleep(2500);

    await step('二台目：自分が保存したあとでも、向こうの直しは届く', `
        return whole().includes('保存のあとに向こうが直した')
            ? true : '保存したノートは、そのあとずっと無視されます';
    `, true);

    await step('二台目：打ちかけの字を置く', `
        loading = true;
        editor.setValue('# 二台目\\n\\nいま打ちかけの字。\\n');
        loading = false;
        state.dirty = true;
        return true;
    `, true);

    wrote('外から横取り。');
    await sleep(2800);

    await step('二台目：打ちかけの字を、外から消させない', `
        // **打っているあいだは、下から書き換えない。** 向こうの字は
        // ファイルに残ったまま待つ ── 画面の字が、打った覚えのないものに
        // 変わるのがいちばん怖い。
        return whole().includes('いま打ちかけの字')
            ? true : '打っていた字が消えました';
    `, true);

    await step('二台目：保存したときに、両方とも残る', `
        // 突き合わせるのは保存のとき ── そこで**どちらも失わない**。
        state.dirty = true;
        await save();
        await new Promise((g) => setTimeout(g, 2000));
        const now = whole();
        if (!now.includes('いま打ちかけの字')) return '打っていた字が消えました';
        if (!now.includes('外から横取り')) return '向こうの字が消えました';
        return true;
    `, true);
}

// 二十の三。同期 ── 偽の Drive と上げ下ろし（`walk-sync.mjs`）。
await syncWalk();

// 二十の四。**保存ディレクトリを二つ**（依頼 511・乙）── 足す・見る・作る・
// 移す・運ぶ・外す。二つ目は `NOTES2`（walk.sh が置く）。設定には書かない
// （`savePlaces` を黙らせて、状態だけ動かす ── 本物の設定を汚さないため）。
const NOTES2 = process.env.NOTES2 || '';
const DRIVE2 = process.env.DRIVE || '';
const drive2 = async (p, body) => {
    const r = await fetch(DRIVE2 + p, body === undefined ? {} : { method: 'POST', body: JSON.stringify(body) });
    return r.json();
};
if (NOTES2) {
    const P2 = JSON.stringify(NOTES2);
    await step('保存ディレクトリ：一つのときは、列に親の行が無い', `
        if (state.places.length !== 1) return '場所が ' + state.places.length + ' つあります';
        if (document.querySelector('#rail .dest[data-kind="place"]')) return '親の行が出ています';
        if (!CMDS.some((c) => c.id === 'places' && c.name === '保存ディレクトリの追加・変更・削除')) return '⚙ の名前が違います';
        return true;`, true);
    await step('保存ディレクトリ：二つ目を足すと、フォルダの段にそれぞれの親が並ぶ', `
        window.__savePlaces = savePlaces;
        savePlaces = () => { state.root = state.places[0].dir; };
        state.places.push({ name: '二つ目', dir: ${P2}, sync: 'none', at: '二つ目' });
        await rewatch();
        await reload({ quiet: true });
        const rows = [...document.querySelectorAll('#rail .dest[data-kind="place"]')].map((d) => d.querySelector('.nm').textContent);
        if (rows.join('/') !== state.places.map((p) => p.name).join('/')) return '親の行: ' + rows.join('/');
        if (!state.notes.some((n) => n.place === '二つ目' && n.title === '別口')) return '二つ目のノートが一覧に来ていません';
        const b = state.books.find((x) => x === ${P2} + '/持ち帰り');
        if (!b) return '二つ目のフォルダが無い: ' + state.books.join(' / ');
        const d = [...document.querySelectorAll('#rail .dest[data-kind="book"]')].find((x) => x.dataset.what === b);
        if (!d || d.dataset.depth !== '1') return '二つ目のフォルダが一段下がっていない（' + (d && d.dataset.depth) + '）';
        // 一つ目の親には Drive の札、二つ目には無い。
        const badges = [...document.querySelectorAll('#rail .dest[data-kind="place"]')].map((x) => !!x.querySelector('.sy'));
        if (badges.join() !== 'true,false') return 'Drive の札: ' + badges.join();
        return true;`, true);
    await step('保存ディレクトリ：親の行を押すと、その中のノートだけが並ぶ', `
        const d = [...document.querySelectorAll('#rail .dest[data-kind="place"]')].find((x) => x.dataset.what === ${P2});
        d.dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true }));
        await new Promise((g) => setTimeout(g, 250));
        if (state.dest.kind !== 'place') return '行き先が ' + state.dest.kind;
        const shown = document.querySelectorAll('#rows .row').length;
        const want = state.notes.filter((n) => n.root === ${P2}).length;
        if (shown !== want) return shown + ' 本（' + want + ' 本のはず）';
        if (el('where').textContent !== '二つ目') return '見出しが ' + el('where').textContent;
        return true;`, true);
    await step('保存ディレクトリ：二つ目を見ながら作ると、二つ目に出来る', `
        const at = await newNote('二つ目の新しいノート');
        if (!at || !at.startsWith(${P2} + '/')) return '出来た道: ' + at;
        return true;`, true);
    await step('保存ディレクトリ：移す先の一覧は、保存ディレクトリの名前を頭に', `
        const names = bookChoices().map((c) => c.name);
        if (!names.includes('二つ目（トップページ）')) return names.join(' / ');
        if (!names.includes('二つ目 › 持ち帰り')) return names.join(' / ');
        if (!names.some((n) => n.endsWith(' › 仕事'))) return names.join(' / ');
        return true;`, true);
    await step('保存ディレクトリ：二つ目のノートを一つ目のフォルダへ移せる', `
        const to = state.root + '/仕事';
        const r = await moveOp(state.open.path, to);
        await reload({ quiet: true });
        if (!r.path.startsWith(to + '/')) return '移った先: ' + r.path;
        await openNote(r.path);
        if (state.open.place !== state.places[0].name) return '一覧の上では ' + state.open.place;
        return true;`, true);
    await step('保存ディレクトリ：親の行の右押しに、同期先・名前・場所・外す', `
        const d = [...document.querySelectorAll('#rail .dest[data-kind="place"]')].find((x) => x.dataset.what === ${P2});
        d.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 40, clientY: 200 }));
        await new Promise((g) => setTimeout(g, 250));
        const t = el('more').textContent;
        el('more').hidden = true;
        for (const w of ['同期先', '名前を変える', '場所を変える…', '外す', 'この中にフォルダを作る']) if (!t.includes(w)) return '献立に「' + w + '」が無い: ' + t;
        return true;`, true);
    if (DRIVE2) {
        await step('同期：二つ目を Drive にすると、運んだ列に保存ディレクトリの名前が付く', `
            state.places[1].sync = 'drive';
            drawSyncState();
            const r = await syncNow('手');
            if (!r) return '運びませんでした';
            if (r.trouble.length) return '困りごと: ' + r.trouble[0];
            if (!r.places['二つ目'] || !r.places['二つ目'].up) return '二つ目のぶんが上がっていません: ' + JSON.stringify(r.places);
            const box = el('syncsay');
            if (box.hidden || !box.textContent.includes('二つ目:')) return '列: ' + box.textContent;
            return true;`, true);
        tally.ran += 1;
        {
            const there = await drive2('/_list');
            const rels = there.map((f) => String((f.appProperties || {}).rel || ''));
            if (!rels.includes('二つ目/別口.md') || !rels.includes('二つ目/持ち帰り/宿題.md')) {
                bad.push({ name: '同期：二つ目は向こうの ambər/二つ目/ の下', why: ['向こうの一覧: ' + rels.filter((r) => r.includes('二つ目') || r.includes('別口')).join(' / ')] });
            }
            if (rels.some((r) => r.startsWith('二つ目/仕事/'))) {
                bad.push({ name: '同期：一つ目のフォルダが二つ目に紛れない', why: ['二つ目/仕事/ が向こうにあります'] });
            }
        }
        await step('同期：ふだんの一行には、保存ディレクトリの名前が出ない', `
            syncFresh = null; drawSyncState();
            const t = el('syncmark').textContent;
            if (t.includes('二つ目')) return t;
            if (!t.includes('同期しています')) return t;
            return true;`, true);
        await step('同期：二つ目を「同期しない」に戻すと、一つ目だけを運ぶ', `
            state.places[1].sync = 'none';
            const r = await syncNow('手');
            if (!r) return '運びませんでした';
            if (Object.keys(r.places).join() !== state.places[0].name) return Object.keys(r.places).join();
            return true;`, true);
    }
    await step('同期の様子：どの保存ディレクトリも「同期しない」なら、灰色の一行と「同期先を選ぶ」', `
        const was = state.places.map((p) => p.sync);
        for (const p of state.places) p.sync = 'none';
        drawSyncState();
        const m = el('syncmark');
        const out = !m.hidden && m.textContent.includes('同期していません') && m.textContent.includes('同期先を選ぶ') && el('syncsay').hidden;
        state.places.forEach((p, i) => { p.sync = was[i]; });
        drawSyncState();
        return out ? true : (m.hidden ? '一行が出ていません' : m.textContent);`, true);
    await step('保存ディレクトリ：外すと、一覧からも列からも消える（ファイルは残る）', answering(
        `placeDrop(state.places[1]);`,
        `savePlaces = window.__savePlaces;
        if (state.places.length !== 1) return '場所が ' + state.places.length + ' つ';
        if (document.querySelector('#rail .dest[data-kind="place"]')) return '親の行が残っています';
        if (state.notes.some((n) => n.root === ${P2})) return '二つ目のノートが一覧に残っています';
        return true;`), true);
}

// 二十一。後始末 ── 歩いた跡を消す（ゴミ箱へは入れない: OS の外へ出る）
await step('片づける', `
    for (const n of state.notes.filter((x) => relOf(x.book) === '歩き試し'
            || /複製|新しいノート|週報|二台目|名前は一行目から|小窓から作ったノート/.test(x.title || '')
            || /買い物\.2\.md$/.test(x.path))) {
        try { await ask('delete', { path: n.path }); } catch { /* もう無い */ }
    }
    await reload({ quiet: true });
    return true;`, true);

/* ── 報せ ── */
report(times);
