/* 総ざらいの**足場**。デスクトップ版と話す口・一つ動かして見張る `step`・落ちたものの
 * 帳画面。`walk.mjs`（ぜんぶ）と `walk-sync.mjs`（同期だけ）が同じものを使う。
 *
 *     import { step, run, bad, tally, sleep, ready, report, NOTES } from './walk-harness.mjs';
 *
 * **ウィンドウへ送る文字の中に、逆引用符と円記号を書かないこと。** ここは
 * テンプレートの中なので、そこでテンプレートが閉じる・改行が本物になる
 * ── 三度踏んだ（2026-09-09）。註にも書けない。改行が要るなら
 * String.fromCharCode(10)。
 */
export const PORT = process.env.PORT || 9333;
/// 試し場のノートが置いてある道（`walk.sh` が渡す）。
export const NOTES = process.env.NOTES || '';

/* ── デスクトップ版と話す ── */

const tabs = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
const page = tabs.find((x) => x.type === 'page');
if (!page) {
    console.error(`デスクトップ版が見つかりません（${PORT} で出ていますか）`);
    process.exit(2);
}
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((go, no) => { ws.onopen = go; ws.onerror = no; });

let id = 0;
const waits = new Map();
/// デスクトップ版が言ったこと（error と warning と、飛んだ例外）。
let noise = [];
let pausedAt = null;
ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && waits.has(m.id)) { waits.get(m.id)(m); waits.delete(m.id); return; }
    if (m.method === 'Debugger.paused') { if (pausedAt) pausedAt(m.params); return; }
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

export const sleep = (ms) => new Promise((go) => setTimeout(go, ms));

/// デスクトップ版の中で一つ動かす。返ってくるのは値か、落ちた理由。
///
/// **待ちきりにしない。** 小デスクトップ版を開ける命令を `await` すると、閉じる人が
/// いないので永久に返ってこない ── 総ざらいが黙って止まる（実際に止めた）。
/// 待つのをやめたことは、落第として出す。
export async function run(src) {
    const r = await Promise.race([
        send('Runtime.evaluate', {
            expression: `(async () => { ${src} })()`,
            awaitPromise: true, returnByValue: true, userGesture: true,
        }),
        sleep(Number(process.env.PATIENCE || 8000)).then(() => 'まった'),
    ]);
    if (r === 'まった') {
        // **どこで待っているかを言う。** デスクトップ版の JS が回りっぱなしなら 1+1 も返らず、
        // 約束が解けないだけなら 1+1 は返る ── 直す場所がまるで違う。
        const alive = await Promise.race([
            send('Runtime.evaluate', { expression: '1+1', returnByValue: true }),
            sleep(2000).then(() => null),
        ]);
        const busy = await Promise.race([
            send('Runtime.evaluate', { expression: 'JSON.stringify({ syncBusy, veil: !el("veil").hidden, ev: !el("evform").hidden, dirty: state.dirty })', returnByValue: true }),
            sleep(2000).then(() => null),
        ]);
        if (!alive) {
            // 眠らされているだけかもしれない ── 十秒おいてもう一度だけ訊く。
            await sleep(10000);
            const again = await Promise.race([
                send('Runtime.evaluate', { expression: '1+1', returnByValue: true }),
                sleep(3000).then(() => null),
            ]);
            if (again) return { bad: '返ってきません（デスクトップ版は十秒ほど止まっていた）' };
            // **固まったデスクトップ版に、残りの段を押しても意味が無い** ── 一段ごとに十秒
            // 待って二時間かける前に、**どこで回っているか**を取って止まる。
            // 回りっぱなしの JS も `Debugger.pause` なら止められる（次の割り込みで）。
            const where = await whereStuck();
            return { bad: 'デスクトップ版が固まっている（1+1 も返らない）', frozen: true, where };
        }
        return { bad: '返ってきません（デスクトップ版は生きている・' + (busy && busy.result && busy.result.result ? busy.result.result.value : '?') + '）' };
    }
    const bad = r.result?.exceptionDetails;
    if (bad) {
        return { bad: String(bad.exception?.description || bad.text).split('\n')[0].slice(0, 300) };
    }
    return { value: r.result?.result?.value };
}

/* ── 見張りながら、一つ動かす ── */

export const bad = [];

/// `name` を動かして、落ちなかったか・言わなかったか・戻せるかを見る。
///
/// `want` を渡すと、返り値がそれと合うかも見る（合わなければ落第）。
export async function step(name, src, want) {
    noise = [];
    tally.ran += 1;
    const r = await run(src);
    await sleep(Number(process.env.WAIT || 260));
    const said = noise.filter((s) => !QUIET.some((q) => s.includes(q)));
    // **触ったあと、まだテキストに戻せるか。** ここが `null` になったノートは、
    // 見た目は何ともないのに、そこから先の保存が黙って止まる。
    const back = await run(`
        if (!state.open || view === 'write') return 'skip';
        return paperToMd(el('read'), state.head) === null ? 'もう文字に戻せません' : 'ok';
    `);
    // **机の上が、しまってあるノートとずれていないか**（2026-09-20）。
    // タブは `t.path`、しまってあるノートは `t.keep.open.path` で同じ一本を
    // 指している。移した（共有に入れる・外す・フォルダへ移す）ときに片方
    // だけ繋ぎ直すと、そのタブへ戻ったとたん `state.open` が**もう無い
    // パス**になり、そこから先の保存が黙って落ちる ── 画面は何ともない
    // ので、**気づくのは同期が「変わっていません」と言うとき**。実際に
    // 同期の段が 7 つ落ちていて、元をたどると 100 段ほど前のここだった。
    const tabsOk = await run(`
        const ずれ = tabs.filter((t) => t.keep && t.keep.open && t.keep.open.path !== t.path);
        if (!ずれ.length) return 'ok';
        return '机の上としまってあるノートがずれています: ' + JSON.stringify(ずれ.map((t) => [
            t.path.replace(state.root, ''), t.keep.open.path.replace(state.root, '')]));
    `);
    const why = [];
    if (r.bad) why.push(r.bad);
    if (tabsOk.value && tabsOk.value !== 'ok') why.push(tabsOk.value);
    if (r.frozen) {
        why.push(...(r.where || []));
        bad.push({ name, why });
        report();
    }
    if (said.length) why.push(...said);
    if (back.value && back.value !== 'ok' && back.value !== 'skip') why.push(back.value);
    if (typeof want === 'function') {
        // 見張り方を渡された ── 速さのように、値そのものではなく
        // 「その範囲か」を見たいとき。
        const said2 = want(r.value);
        if (said2 !== true) why.push(String(said2));
    } else if (want !== undefined && r.value !== want) {
        why.push(`返り値が ${JSON.stringify(r.value)}（ほしいのは ${JSON.stringify(want)}）`);
    }
    if (why.length) bad.push({ name, why });
}

/// **黙って見逃すもの。** デスクトップ版のせいでないもの・試す場所のせいのもの。
const QUIET = [
    'Autofill.enable',                 // CDP を繋いだときに Chromium が言う
    'Request Autofill.setAddresses',
    'net::ERR_FILE_NOT_FOUND',         // 試す場所に置いていない絵
];


/// 固まったデスクトップ版の、いまの呼び出しの列（上から六つ）。取れなければ空。
async function whereStuck() {
    const got = new Promise((go) => { pausedAt = go; });
    send('Debugger.enable');
    send('Debugger.pause');
    const p = await Promise.race([got, sleep(6000).then(() => null)]);
    pausedAt = null;
    if (!p) return ['（止められませんでした ── 描く側そのものが応えない）'];
    const rows = (p.callFrames || []).slice(0, 8).map((f) =>
        (f.functionName || '（無名）') + ' @ ' + String(f.url || '').split('/').pop() + ':' + (f.location.lineNumber + 1));
    send('Debugger.resume');
    return rows.length ? rows : ['（列が空）'];
}

/// 落ちたものの帳画面と、動かした数。
export const tally = { ran: 0 };

/// **デスクトップ版の台本とエンジンが立ち上がるのを待つ。** デスクトップ版が出た直後の一回目は、
/// まだ子が起きていないことがある ── 一度きりで見ると、たまに落ちる検査に
/// なる（実際に何度か落ちた）。**時々鳴る検査は、無いより悪い。**
/// そのあと、勝手に運ぶのを切る（同期の段で手で呼ぶ ── 裏で運ぶと数が動く）。
export async function ready() {
    await step('読み込み直す', `
        // renderer.js が評価されていないことがある（reload is not defined）。
        for (let i = 0; i < 40 && typeof reload !== 'function'; i += 1) {
            await new Promise((g) => setTimeout(g, 250));
        }
        for (let i = 0; i < 20; i += 1) {
            await reload({ quiet: true });
            if (state.notes.length > 0) return true;
            await new Promise((g) => setTimeout(g, 250));
        }
        return 'ノートが一本も読めません';
    `, true);
    await step('同期を手だけにする', `
        syncAuto = false;
        // 改名も手だけに（固定のファイル名で押して回るので）── 改名の段で入れる。
        nameAuto = false;
        clearTimeout(syncTimer);
        // 開いた直後の一度目が裏で走っているなら、終わるまで待つ。
        for (let i = 0; i < 80 && syncBusy; i += 1) await new Promise((g) => setTimeout(g, 250));
        return syncBusy ? '裏の同期が終わりません' : true;`, true);
}

/// 報せて、デスクトップ版との口を閉じて、終わる。落ちたものだけ出す。ぜんぶ通れば一行。
export function report(times = []) {
    console.log('');
    if (times.length) {
        console.log('大きいノート（一万二千行）で測ったもの:');
        for (const t of times) console.log('  ' + t);
        console.log('');
    }
    if (!bad.length) {
        console.log(`${tally.ran} とおり動かして、落ちたものはありません`);
        ws.close();
        process.exit(0);
    }
    console.log(`${tally.ran} とおり動かして、${bad.length} 件おかしいです`);
    console.log('');
    for (const b of bad) {
        console.log('✗ ' + b.name);
        for (const w of b.why) console.log('   ' + w);
    }
    ws.close();
    process.exit(1);
}
