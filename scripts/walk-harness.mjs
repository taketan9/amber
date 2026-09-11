/* 総ざらいの**足場**。窓と話す口・一つ動かして見張る `step`・落ちたものの
 * 帳面。`walk.mjs`（ぜんぶ）と `walk-sync.mjs`（同期だけ）が同じものを使う。
 *
 *     import { step, run, bad, tally, sleep, ready, report, NOTES } from './walk-harness.mjs';
 *
 * **窓へ送る字の中に、逆引用符と円記号を書かないこと。** ここは
 * テンプレートの中なので、そこでテンプレートが閉じる・改行が本物になる
 * ── 三度踏んだ（2026-09-09）。註にも書けない。改行が要るなら
 * String.fromCharCode(10)。
 */
export const PORT = process.env.PORT || 9333;
/// 試し場のノートが置いてある道（`walk.sh` が渡す）。
export const NOTES = process.env.NOTES || '';

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

export const sleep = (ms) => new Promise((go) => setTimeout(go, ms));

/// 窓の中で一つ動かす。返ってくるのは値か、落ちた理由。
///
/// **待ちきりにしない。** 小窓を開ける命令を `await` すると、閉じる人が
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
    if (r === 'まった') return { bad: '返ってきません（小窓が開いたまま待っている？）' };
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

/// **黙って見逃すもの。** 窓のせいでないもの・試す場所のせいのもの。
const QUIET = [
    'Autofill.enable',                 // CDP を繋いだときに Chromium が言う
    'Request Autofill.setAddresses',
    'net::ERR_FILE_NOT_FOUND',         // 試す場所に置いていない絵
];


/// 落ちたものの帳面と、動かした数。
export const tally = { ran: 0 };

/// **窓の台本とエンジンが立ち上がるのを待つ。** 窓が出た直後の一回目は、
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

/// 報せて、窓との口を閉じて、終わる。落ちたものだけ出す。ぜんぶ通れば一行。
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
