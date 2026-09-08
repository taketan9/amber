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
 */
const PORT = process.env.PORT || 9333;

/* ── 窓と話す ── */

const tabs = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
const page = tabs.find((x) => x.type === 'page');
if (!page) {
    console.error(`窓が見つかりません（${PORT} で出ていますか）`);
    process.exit(2);
}
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((go, no) => { ws.onopen = go; ws.onerror = no; });

let id = 0;
const waits = new Map();
/// 窓が言ったこと（error と warning と、飛んだ例外）。
let noise = [];
ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && waits.has(m.id)) { waits.get(m.id)(m); waits.delete(m.id); return; }
    if (m.method === 'Runtime.consoleAPICalled' && ['error', 'assert'].includes(m.params.type)) {
        noise.push('console: ' + m.params.args
            .map((a) => a.value ?? a.description ?? '?').join(' ').slice(0, 200));
    }
    if (m.method === 'Runtime.exceptionThrown') {
        const d = m.params.exceptionDetails;
        noise.push('例外: ' + String(d.exception?.description || d.text).split('\n')[0].slice(0, 200));
    }
};
const send = (method, params) => new Promise((go) => {
    const n = ++id;
    waits.set(n, go);
    ws.send(JSON.stringify({ id: n, method, params }));
});
await send('Runtime.enable');

const sleep = (ms) => new Promise((go) => setTimeout(go, ms));

/// 窓の中で一つ動かす。返ってくるのは値か、落ちた理由。
///
/// **待ちきりにしない。** 小窓を開ける命令を `await` すると、閉じる人が
/// いないので永久に返ってこない ── 総ざらいが黙って止まる（実際に止めた）。
/// 待つのをやめたことは、落第として出す。
async function run(src) {
    const r = await Promise.race([
        send('Runtime.evaluate', {
            expression: `(async () => { ${src} })()`,
            awaitPromise: true, returnByValue: true, userGesture: true,
        }),
        sleep(Number(process.env.PATIENCE || 8000)).then(() => 'まった'),
    ]);
    if (r === 'まった') return { bad: '返ってきません（小窓が開いたまま待っている？）' };
    const bad = r.result?.exceptionDetails;
    if (bad) {
        return { bad: String(bad.exception?.description || bad.text).split('\n')[0].slice(0, 300) };
    }
    return { value: r.result?.result?.value };
}

/* ── 見張りながら、一つ動かす ── */

const bad = [];
let ran = 0;

/// `name` を動かして、落ちなかったか・言わなかったか・戻せるかを見る。
///
/// `want` を渡すと、返り値がそれと合うかも見る（合わなければ落第）。
async function step(name, src, want) {
    noise = [];
    ran += 1;
    const r = await run(src);
    await sleep(Number(process.env.WAIT || 260));
    const said = noise.filter((s) => !QUIET.some((q) => s.includes(q)));
    // **触ったあと、まだ字に戻せるか。** ここが `null` になったノートは、
    // 見た目は何ともないのに、そこから先の保存が黙って止まる。
    const back = await run(`
        if (!state.open || view === 'write') return 'skip';
        return paperToMd(el('read'), state.head) === null ? 'もう字に戻せません' : 'ok';
    `);
    const why = [];
    if (r.bad) why.push(r.bad);
    if (said.length) why.push(...said);
    if (back.value && back.value !== 'ok' && back.value !== 'skip') why.push(back.value);
    if (want !== undefined && r.value !== want) {
        why.push(`返り値が ${JSON.stringify(r.value)}（ほしいのは ${JSON.stringify(want)}）`);
    }
    if (why.length) bad.push({ name, why });
}

/// **黙って見逃すもの。** 窓のせいでないもの・試す場所のせいのもの。
const QUIET = [
    'Autofill.enable',                 // CDP を繋いだときに Chromium が言う
    'Request Autofill.setAddresses',
    'net::ERR_FILE_NOT_FOUND',         // 試す場所に置いていない絵
];

/* ── 総ざらい ── */

const path = (n) => `state.root + '/${n}'`;

// 一。開いて、見る
await step('読み込み直す', `await reload({ quiet: true }); return state.notes.length > 0;`, true);
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
await step('言葉で探す', `
    el('find').value = '牛乳';
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
// **小窓を開ける命令は `await` しない。** ★ は置き場所を訊いてくる。
await step('ブックマークに登録', `
    cmdStar();
    await new Promise((g) => setTimeout(g, 500));
    const asked = !el('veil').hidden;
    closeSheet(null);
    return asked || starred(state.open);`, true);
await step('テンプレートから作る', `
    const rows = state.notes.filter((n) => n.book === TEMPLATES);
    if (!rows.length) return 'ひな型なし';
    const r = await ask('copy', { path: rows[0].path, dir: state.root });
    await reload({ quiet: true });
    await openNote(r.path);
    return !!state.open;`, true);

// 十。フォルダとタグ
await step('新しいフォルダ', `
    await ask('mkbook', { dir: state.root + '/歩き試し' });
    await reload({ quiet: true });
    return state.books.includes('歩き試し');`, true);
await step('フォルダへ移す', `
    await openNote(${path('からっぽ.md')});
    await ask('move', { path: state.open.path, dir: state.root + '/歩き試し' });
    await reload({ quiet: true });
    return state.notes.some((n) => n.book === '歩き試し');`, true);
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

// 十二。Web から取り込む（手元に立てたページがあるときだけ）
if (process.env.SITE) {
    await step('Web から取り込む', `
        const got = await window.amber.fetchPage(${JSON.stringify(process.env.SITE)});
        if (got.error) return got.error;
        const md = webToMd(bestPart(webClean(got.html, got.url)).outerHTML, got.url);
        return md.includes('#') && md.length > 20;`, true);
}

// 十三。後始末 ── 歩いた跡を消す（ゴミ箱へは入れない: OS の外へ出る）
await step('片づける', `
    for (const n of state.notes.filter((x) => x.book === '歩き試し'
            || /複製|新しいノート|週報/.test(x.title || ''))) {
        try { await ask('delete', { path: n.path }); } catch { /* もう無い */ }
    }
    await reload({ quiet: true });
    return true;`, true);

/* ── 報せ ── */

console.log('');
if (!bad.length) {
    console.log(`${ran} とおり動かして、落ちたものはありません`);
    ws.close();
    process.exit(0);
}
console.log(`${ran} とおり動かして、${bad.length} 件おかしいです`);
console.log('');
for (const b of bad) {
    console.log('✗ ' + b.name);
    for (const w of b.why) console.log('   ' + w);
}
ws.close();
process.exit(1);
