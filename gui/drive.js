// Press keys at the real window, and read back what happened.
//
// **The keys had never been tried.** Every check up to now went to the engine
// over the pipe, which is the half that was already right; the half that broke
// was the renderer, and it only breaks once a key is pressed. Two faults —
// a call to a function that did not exist, and a second `refresh()` quietly
// replacing the first — got through `node --check`, `cargo test` and the audit
// and landed on Taketan instead.
//
//     node gui/drive.js            # the standard round
//     node gui/drive.js , T ? Esc  # or whatever keys you want to see
//
// Electron is started with a debugging port and driven over CDP. No package:
// Node has had a WebSocket of its own since 22, and adding a dependency to a
// project whose whole point is that it builds offline would be a poor trade.

const { spawn } = require('node:child_process');
const path = require('node:path');

const PORT = 9223;
const ROOT = path.join(__dirname, '..');

/// Keys as you would say them out loud, turned into what CDP wants.
const NAMED = {
    Esc: 'Escape', Enter: 'Enter', Tab: 'Tab', Space: ' ',
    Down: 'ArrowDown', Up: 'ArrowUp', Left: 'ArrowLeft', Right: 'ArrowRight',
    Bksp: 'Backspace', F5: 'F5',
};

function parseKey(spec) {
    const parts = spec.split('+');
    const base = parts.pop();
    const mods = parts.map((m) => m.toLowerCase());
    let bits = 0;
    if (mods.includes('alt')) bits |= 1;
    if (mods.includes('ctrl')) bits |= 2;
    if (mods.includes('meta') || mods.includes('cmd')) bits |= 4;
    if (mods.includes('shift')) bits |= 8;
    const key = NAMED[base] || base;
    return { key, modifiers: bits, text: key.length === 1 && bits < 2 ? key : undefined };
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function target() {
    // Electron takes a moment to open the port; the page target appears after.
    for (let i = 0; i < 60; i++) {
        try {
            const res = await fetch(`http://127.0.0.1:${PORT}/json`);
            const page = (await res.json()).find((t) => t.type === 'page');
            if (page) return page.webSocketDebuggerUrl;
        } catch { /* not up yet */ }
        await sleep(250);
    }
    throw new Error('the window never appeared on the debugging port');
}

class Cdp {
    constructor(ws) { this.ws = ws; this.id = 0; this.waiting = new Map(); }

    static async open(url) {
        const ws = new WebSocket(url);
        await new Promise((ok, no) => { ws.onopen = ok; ws.onerror = no; });
        const cdp = new Cdp(ws);
        ws.onmessage = (e) => {
            const msg = JSON.parse(e.data);
            const w = cdp.waiting.get(msg.id);
            if (!w) return;
            cdp.waiting.delete(msg.id);
            msg.error ? w.no(new Error(msg.error.message)) : w.ok(msg.result);
        };
        return cdp;
    }

    send(method, params = {}) {
        const id = ++this.id;
        this.ws.send(JSON.stringify({ id, method, params }));
        return new Promise((ok, no) => this.waiting.set(id, { ok, no }));
    }

    async press(spec) {
        const k = parseKey(spec);
        for (const type of ['keyDown', 'keyUp']) {
            await this.send('Input.dispatchKeyEvent', {
                type: type === 'keyDown' && k.text ? 'keyDown' : type,
                key: k.key,
                text: type === 'keyDown' ? k.text : undefined,
                modifiers: k.modifiers,
            });
        }
        await sleep(120);
    }

    async read(expr) {
        const r = await this.send('Runtime.evaluate', {
            expression: expr, returnByValue: true, awaitPromise: true,
        });
        if (r.exceptionDetails) throw new Error(r.exceptionDetails.text);
        return r.result.value;
    }
}

/// What the window says about itself: the status line, and whichever sheet is
/// up. Enough to tell a key that worked from a key that did nothing.
const LOOK = `({
    status: document.querySelector('#status')?.textContent?.trim() ?? '',
    sheet: document.querySelector('#find:not([hidden])') ? 'sheet' : null,
    rows: document.querySelectorAll('#find:not([hidden]) .hit').length,
    at: [...document.querySelectorAll('#find:not([hidden]) .hit')].findIndex((e) => e.classList.contains('on')),
    scroll: document.querySelector('#find:not([hidden]) .hits')?.scrollTop ?? 0,
    focus: state?.focus,
    left: state?.left ? state.left.entries.length : null,
    marked: state?.[state.focus]?.marked,
})`;

async function main() {
    const keys = process.argv.slice(2);
    const round = keys.length ? keys.map((k) => [k, '']) : [
        [',', 'ソート'], [',', 'ソートもう一度'],
        ['T', 'トグルを開く'], ['Down', ''], ['Enter', '配色を送る'], ['Esc', '閉じる'],
        ['?', 'ヘルプを開く'], ['Esc', '閉じる'],
        ['Space', 'マーク'], ['V', '反転'], ['Ctrl+a', '全マーク'],
        ['Tab', 'ペイン切替'], ['Ctrl+l', '右へ'], ['Ctrl+h', '左へ'],
        ['F5', '読み直し'], ['p', 'パスをコピー'],
        ['o', 'ペインを揃える'],
    ];

    const el = spawn(process.env.CIAN_ELECTRON
        || path.join(__dirname, 'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'),
        [__dirname, `--remote-debugging-port=${PORT}`],
        { cwd: ROOT, stdio: ['ignore', 'pipe', 'pipe'] });

    const crashes = [];
    for (const s of [el.stdout, el.stderr]) {
        s.on('data', (b) => {
            const t = String(b);
            if (/Uncaught|ReferenceError|TypeError|is not a function/.test(t)) crashes.push(t.trim());
        });
    }

    let bad = 0;
    try {
        const cdp = await Cdp.open(await target());
        await cdp.send('Runtime.enable');
        await sleep(1200);

        for (const [key, what] of round) {
            const before = await cdp.read(LOOK);
            await cdp.press(key);
            const after = await cdp.read(LOOK);
            const moved = JSON.stringify(before) !== JSON.stringify(after);
            const note = what ? `  ${what}` : '';
            console.log(`${moved ? '  ' : '× '}${key.padEnd(8)}${note.padEnd(16)} ${after.status}`);
            if (!moved) bad++;
        }
    } finally {
        el.kill();
    }

    if (crashes.length) {
        console.log('\n落ちた:');
        crashes.forEach((c) => console.log('  ' + c));
    }
    // A key that changed nothing is not always wrong — pressing `,` twice only
    // reverses — so this reports rather than fails. The crashes are the failure.
    console.log(`\n動かなかったキー ${bad} 件、例外 ${crashes.length} 件`);
    process.exit(crashes.length ? 1 : 0);
}

main().catch((e) => { console.error(e.message); process.exit(1); });
