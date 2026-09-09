#!/usr/bin/env node
/* 初めて開いた人が見るもの（`first-run.sh` が呼ぶ）。
 *
 * **窓へ送る字の中に、逆引用符と円記号を書かないこと** ── そこで
 * テンプレートが閉じる（`walk.mjs` の頭と同じ罠）。
 */
import { existsSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const PORT = process.env.PORT || 9334;
const HOME_DIR = process.env.HOME_DIR || '';

const tabs = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
const page = tabs.find((x) => x.type === 'page');
if (!page) { console.error('窓が見つかりません'); process.exit(2); }
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((go, no) => { ws.onopen = go; ws.onerror = no; });

let id = 0;
const waits = new Map();
let noise = [];
ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && waits.has(m.id)) { waits.get(m.id)(m); waits.delete(m.id); return; }
    if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') {
        noise.push('console: ' + m.params.args.map((a) => a.value ?? '?').join(' ').slice(0, 200));
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
const run = async (src) => {
    const r = await send('Runtime.evaluate', {
        expression: `(async () => { ${src} })()`,
        awaitPromise: true, returnByValue: true, userGesture: true,
    });
    const bad = r.result?.exceptionDetails;
    if (bad) return { bad: String(bad.exception?.description || bad.text).split('\n')[0] };
    return { value: r.result?.result?.value };
};

const bad = [];
const ok = async (name, src, want) => {
    noise = [];
    const r = await run(src);
    await sleep(300);
    const why = [];
    if (r.bad) why.push(r.bad);
    if (noise.length) why.push(...noise);
    if (want !== undefined && r.value !== want) {
        why.push(`返り値が ${JSON.stringify(r.value)}（ほしいのは ${JSON.stringify(want)}）`);
    }
    if (why.length) bad.push({ name, why });
};

// 初めの組み立てが済むまで待つ。
await sleep(2500);

// 一。置き場所が決まる。
await ok('置き場所が決まる',
    `return state.root.endsWith('/Documents/amber');`, true);

// 二。見本が入る ── 空の窓を見せない。
await ok('見本のノートが入っている',
    `return state.notes.length > 0;`, true);
await ok('「ようこそ」がある',
    `return state.notes.some((n) => /ようこそ/.test(n.title || ''));`, true);

// 三。読める形で出る ── 組めずに白いまま、を許さない。
await ok('見本が読める形で出る', `
    const one = state.notes.find((n) => /ようこそ/.test(n.title || ''));
    if (!one) return 'ようこそがありません';
    await openNote(one.path);
    await new Promise((g) => setTimeout(g, 800));
    const box = el('read');
    if (box.children.length < 5) return 'かたまりが ' + box.children.length + ' しかありません';
    if (!box.querySelector('h1, h2')) return '見出しが組めていません';
    if (!box.querySelector('table')) return '表が組めていません';
    return true;`, true);
await ok('見本が字に戻せる', `
    return paperToMd(el('read'), state.head) === null ? '戻せません' : true;`, true);

// 四。絵も付いてくる（見本は絵を一枚使っている）。
if (HOME_DIR) {
    const notes = join(HOME_DIR, 'Documents', 'amber');
    const shot = join(notes, 'attachments');
    if (!existsSync(shot) || !readdirSync(shot).length) {
        bad.push({ name: '見本の絵', why: ['attachments が空です（絵が付いてきていません）'] });
    }
}

// 五。二度目に開いても増えない ── 同じ名前は飛ばす。
await ok('二度置いても増えない', `
    const was = state.notes.length;
    const r = await window.amber.welcome(state.root);
    await reload({ quiet: true });
    return state.notes.length === was ? true : '増えました（' + was + ' → ' + state.notes.length + '）';`, true);

console.log('');
if (!bad.length) {
    console.log('初めて開いた人の道は、通っています');
    ws.close();
    process.exit(0);
}
console.log(`${bad.length} 件おかしいです`);
for (const b of bad) {
    console.log('✗ ' + b.name);
    for (const w of b.why) console.log('   ' + w);
}
ws.close();
process.exit(1);
