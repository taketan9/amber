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
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

/// A port of this run's own.
///
/// It was fixed, and a window left over from the run before answered on it —
/// so the keys went to a dead sandbox and the report described someone else's
/// files. The pid is enough to keep two runs apart, and `whose()` below checks
/// that the window it reached is in fact this one's.
const PORT = 9200 + (process.pid % 300);
const ROOT = path.join(__dirname, '..');

/// Keys as you would say them out loud, turned into what CDP wants.
const NAMED = {
    Esc: 'Escape', Enter: 'Enter', Tab: 'Tab', Space: ' ',
    Down: 'ArrowDown', Up: 'ArrowUp', Left: 'ArrowLeft', Right: 'ArrowRight',
    Bksp: 'Backspace', F5: 'F5',
};

function parseKey(spec) {
    // `Mod` is the platform's own: Cmd on macOS, Ctrl everywhere else. Monaco
    // binds Ctrl+S that way and is right to — but the driver was sending Ctrl
    // on a Mac, where it reaches nothing, and reporting the save key as dead.
    spec = spec.replace(/^Mod\+/, process.platform === 'darwin' ? 'Meta+' : 'Ctrl+');
    const parts = spec.split('+');
    const base = parts.pop();
    const mods = parts.map((m) => m.toLowerCase());
    let bits = 0;
    if (mods.includes('alt')) bits |= 1;
    if (mods.includes('ctrl')) bits |= 2;
    if (mods.includes('meta') || mods.includes('cmd')) bits |= 4;
    if (mods.includes('shift')) bits |= 8;
    const key = NAMED[base] || base;
    return {
        key,
        modifiers: bits,
        text: key.length === 1 && bits < 2 ? key : undefined,
        ...virtual(key),
    };
}

/// The key's number and its physical code.
///
/// **Without these, CDP sends keyCode 0.** A page reading `e.key` — everything
/// cian's own handlers do — never notices. Monaco does not read `e.key`: it
/// resolves its keybindings from the number, so Ctrl+S arrived as a keystroke
/// with no identity and the save silently never ran. The driver had reported
/// the key as dead, which was true and not the reason.
const VKEY = {
    Escape: 27, Enter: 13, Tab: 9, Backspace: 8, ' ': 32,
    ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40,
    Home: 36, End: 35, PageUp: 33, PageDown: 34, Delete: 46, Insert: 45,
};
const CODE = {
    Escape: 'Escape', Enter: 'Enter', Tab: 'Tab', Backspace: 'Backspace', ' ': 'Space',
    ArrowLeft: 'ArrowLeft', ArrowUp: 'ArrowUp', ArrowRight: 'ArrowRight', ArrowDown: 'ArrowDown',
    Home: 'Home', End: 'End', PageUp: 'PageUp', PageDown: 'PageDown',
    Delete: 'Delete', Insert: 'Insert',
};

function virtual(key) {
    if (VKEY[key]) return { code: CODE[key], windowsVirtualKeyCode: VKEY[key] };
    if (/^F\d{1,2}$/.test(key)) {
        return { code: key, windowsVirtualKeyCode: 111 + Number(key.slice(1)) };
    }
    if (key.length === 1) {
        const up = key.toUpperCase();
        if (up >= 'A' && up <= 'Z') {
            return { code: `Key${up}`, windowsVirtualKeyCode: up.charCodeAt(0) };
        }
        if (up >= '0' && up <= '9') {
            return { code: `Digit${up}`, windowsVirtualKeyCode: up.charCodeAt(0) };
        }
    }
    // Punctuation and anything else: the page reads `e.key` for those, and
    // nothing here binds a chord to one.
    return {};
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const argText = (a) => a.value ?? a.description ?? a.unserializableValue ?? JSON.stringify(a.preview ?? '');

/// Shift_JIS bytes. Node cannot encode it, and pulling in iconv for a test
/// fixture would be a dependency for the sake of three lines — but every
/// character used here is in JIS X 0208, so the table is one `Intl`-free map
/// built from what the platform *can* decode.
function sjis(text) {
    const out = [];
    for (const ch of text) {
        const code = ch.codePointAt(0);
        if (code < 0x80) { out.push(code); continue; }
        const found = SJIS_TABLE.get(ch);
        if (found === undefined) throw new Error(`no Shift_JIS byte pair for ${ch}`);
        out.push(found >> 8, found & 0xff);
    }
    return Buffer.from(out);
}

/// Built by asking the platform to decode every two-byte pair once. The
/// decoder is the authority on the mapping, so the table cannot disagree with
/// what the engine will read back.
const SJIS_TABLE = (() => {
    const dec = new TextDecoder('shift_jis', { fatal: false });
    const map = new Map();
    for (let hi = 0x81; hi <= 0xef; hi++) {
        for (let lo = 0x40; lo <= 0xfc; lo++) {
            if (lo === 0x7f) continue;
            const ch = dec.decode(Buffer.from([hi, lo]));
            if (ch.length === 1 && ch !== '\uFFFD' && !map.has(ch)) map.set(ch, (hi << 8) | lo);
        }
    }
    return map;
})();

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

/// Wait until the window is showing this run's sandbox, and say so if it never
/// does. Attaching to the wrong window is silent otherwise: the keys land, the
/// status line answers, and every line of the report is about another
/// directory.
async function settle(cdp, sand) {
    for (let i = 0; i < 40; i++) {
        const cwd = await cdp.read('state?.left?.cwd ?? null');
        if (cwd && cwd.endsWith(path.join(path.basename(sand), 'from'))) return;
        await sleep(200);
    }
    throw new Error('the window never opened on this run\'s sandbox');
}

class Cdp {
    constructor(ws) { this.ws = ws; this.id = 0; this.waiting = new Map(); this.said = []; }

    static async open(url) {
        const ws = new WebSocket(url);
        await new Promise((ok, no) => { ws.onopen = ok; ws.onerror = no; });
        const cdp = new Cdp(ws);
        ws.onmessage = (e) => {
            const msg = JSON.parse(e.data);
            // Everything the page says, not only what it prints to stderr.
            // The first Monaco run opened nothing and reported nothing, and
            // the reason — a plain exception — was sitting in a console this
            // was not reading.
            if (msg.method === 'Runtime.consoleAPICalled' && msg.params.type !== 'log') {
                cdp.said.push(`${msg.params.type}: ${msg.params.args.map(argText).join(' ')}`);
            }
            if (msg.method === 'Runtime.exceptionThrown') {
                cdp.said.push(`例外: ${msg.params.exceptionDetails.exception?.description
                    || msg.params.exceptionDetails.text}`);
            }
            if (msg.method === 'Log.entryAdded' && msg.params.entry.level === 'error') {
                cdp.said.push(`${msg.params.entry.source}: ${msg.params.entry.text}`);
            }
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
        // `type:…` puts a string in, one character at a time, so a prompt can
        // be answered. Everything else is one key.
        // `wait:1200` gives something slow a moment. The editor's runtime
        // takes about a second to load the first time, and reading the window
        // 120 ms after F3 said the key had done nothing.
        if (spec.startsWith('wait:')) {
            await sleep(Number(spec.slice(5)));
            return;
        }
        if (spec.startsWith('type:')) {
            for (const ch of spec.slice(5)) await this.press(ch);
            return;
        }
        const k = parseKey(spec);
        for (const type of ['keyDown', 'keyUp']) {
            await this.send('Input.dispatchKeyEvent', {
                type: type === 'keyDown' && k.text ? 'keyDown' : type,
                key: k.key,
                code: k.code,
                windowsVirtualKeyCode: k.windowsVirtualKeyCode,
                nativeVirtualKeyCode: k.windowsVirtualKeyCode,
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
    // Not just present — actually on top. The confirmation opened behind the
    // editor for a while, and merely being un-hidden said it was up all along.
    asking: (() => {
        const head = document.querySelector('#ask:not([hidden]) .head');
        if (!head) return null;
        const b = head.getBoundingClientRect();
        const at = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2);
        return head.contains(at) || at === head ? head.textContent : '(裏に隠れている)';
    })(),
    rows: document.querySelectorAll('#find:not([hidden]) .hit').length,
    at: [...document.querySelectorAll('#find:not([hidden]) .hit')].findIndex((e) => e.classList.contains('on')),
    focused: document.activeElement?.dataset?.answer ?? null,
    frame: (() => {
        const v = document.querySelector('#view:not([hidden])');
        return v ? getComputedStyle(v).boxShadow.replace(/px/g, '') : null;
    })(),
    prompt: (() => {
        const i = document.querySelector('.vfoot input');
        if (!i) return null;
        const b = i.getBoundingClientRect();
        const f = document.querySelector('.vfoot').getBoundingClientRect();
        // No template literal here: LOOK is one, and a backtick inside it
        // ends it. Twice now.
        return '左端から ' + Math.round(b.left - f.left) + 'px  幅 ' + Math.round(b.width) + 'px';
    })(),
    cursor: state?.[state.focus]?.cursor,
    cwd: state?.[state.focus]?.cwd,
    typed: document.querySelector('input:not([hidden])')?.value ?? null,
    report: document.querySelector('#report:not([hidden])')
        ? { name: document.getElementById('r-name').textContent,
            about: document.getElementById('r-about').textContent,
            rows: document.querySelectorAll('#report .hit').length,
            first: document.querySelector('#report .hit')?.textContent }
        : null,
    view: document.querySelector('#view:not([hidden])')
        ? { about: document.getElementById('v-about').textContent,
            foot: document.getElementById('v-foot').textContent,
            first: document.querySelector('.view-line')?.textContent,
            lines: document.querySelectorAll('.view-line').length }
        : null,
    marks: state?.[state.focus]?.entries?.filter((x) => x.marked).map((x) => x.name) ?? [],
    scroll: document.querySelector('#find:not([hidden]) .hits')?.scrollTop ?? 0,
    focus: state?.focus,
    left: state?.left ? state.left.entries.length : null,
    marked: state?.[state.focus]?.marked,
})`;

async function main() {
    // Its own sandbox, always. The round presses keys that copy, move and
    // delete; pointed at a home directory it would do all three there. The
    // left pane opens on it, and `z` reaches `to`.
    const sand = fs.mkdtempSync(path.join(os.tmpdir(), 'cian-drive-'));
    // `from` holds files only, so `Space` always marks a file. It marked the
    // `to` directory once, the engine correctly refused to put it inside
    // itself, and the round read as a paste that had quietly done nothing.
    fs.mkdirSync(path.join(sand, 'from'));
    fs.mkdirSync(path.join(sand, 'to'));
    for (const name of ['あ.txt', 'b.md', 'c.rs']) {
        // Long enough that G and gg have somewhere to go, and one of them in
        // Shift_JIS — the encoding the viewer exists to get right, and the one
        // a machine in Tokyo meets in every log it did not write.
        const body = Array.from({ length: 40 }, (_, i) => `${i + 1} 行目 ${name} テスト`).join('\n');
        if (name === 'あ.txt') {
            fs.writeFileSync(path.join(sand, 'from', name), sjis(body + '\n'));
        } else {
            fs.writeFileSync(path.join(sand, 'from', name), body + '\n');
        }
    }

    const keys = process.argv.slice(2);
    const round = keys.length ? keys.map((k) => [k, '']) : [
        [',', 'ソート'], [',', 'ソートもう一度'],
        ['T', 'トグルを開く'], ['Down', ''], ['Enter', '配色を送る'], ['Esc', '閉じる'],
        ['?', 'ヘルプを開く'], ['Esc', '閉じる'],
        ['Space', 'マーク'], ['V', '反転'], ['Ctrl+a', '全マーク'],
        ['Tab', 'ペイン切替'], ['Ctrl+l', '右へ'], ['Ctrl+h', '左へ'],
        ['F5', '読み直し'], ['p', 'パスをコピー'],
        ['o', 'ペインを揃える'],
        ['Space', 'ひとつ持つ'], ['Ctrl+c', 'クリップボードへ'],
        ['Tab', '反対ペインへ'],
        ['z', 'パスで移動'], [`type:${sand}/to`, ''], ['Enter', 'to へ'],
        ['Ctrl+v', '貼り付け'],
        ['Tab', '左へ'], ['Down', ''], ['Enter', 'ファイルを読む'],
        ['F3', 'エディタで開く'], ['wait:3000', ''],
        ['type:XX', '打つ'], ['Mod+s', '保存'], ['wait:900', ''],
        ['Esc', ''], ['Esc', ''], ['Esc', '3回で閉じる'],
    ];

    const el = spawn(process.env.CIAN_ELECTRON
        || path.join(__dirname, 'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'),
        [__dirname, path.join(sand, 'from'), `--remote-debugging-port=${PORT}`],
        { cwd: ROOT, stdio: ['ignore', 'pipe', 'pipe'] });

    const crashes = [];
    for (const s of [el.stdout, el.stderr]) {
        s.on('data', (b) => {
            const t = String(b);
            if (/Uncaught|ReferenceError|TypeError|is not a function/.test(t)) crashes.push(t.trim());
        });
    }

    let bad = 0;
    let said = [];
    try {
        const cdp = await Cdp.open(await target());
        said = cdp.said;
        await cdp.send('Runtime.enable');
        await cdp.send('Log.enable');
        await settle(cdp, sand);

        for (const [key, what] of round) {
            const before = await cdp.read(LOOK);
            await cdp.press(key);
            const after = await cdp.read(LOOK);
            const moved = JSON.stringify(before) !== JSON.stringify(after);
            const note = what ? `  ${what}` : '';
            const asking = after.prompt ? `  ｜: ${after.prompt}  枠 ${after.frame}`
                : (after.asking ? `  ⟨${after.asking}⟩ 焦点=${after.focused}` : null);
            const rep = after.report
                ? `  ▤ ${after.report.name} ｜${after.report.about}｜ ${after.report.rows}行  «${after.report.first}»`
                : null;
            const marks = asking ?? rep ?? (after.view
                ? `  ｜${after.view.foot}  ${after.view.about}  «${after.view.first}»`
                : (after.marks.length ? `  [${after.marks.join(' ')}]` : ''));
            console.log(`${moved ? '  ' : '× '}${key.padEnd(8)}${note.padEnd(16)} ${after.status}${marks}`);
            if (!moved) bad++;
        }
        // Let the last job finish before looking. A copy started by the
        // final key is still running when the loop ends.
        await sleep(600);
        console.log(`\n最後の状態: ${(await cdp.read(LOOK)).status}`);
        console.log('砂場:');
        // Bytes, not just names: the editor's whole promise is that a file
        // goes back the way it came, and a name tells you nothing about that.
        const edited = path.join(sand, 'from', 'あ.txt');
        const raw = fs.readFileSync(edited);
        const head = [...raw.subarray(0, 12)].map((b) => b.toString(16).padStart(2, '0')).join(' ');
        console.log(`  あ.txt  ${raw.length} バイト  先頭: ${head}`);
        for (const dir of ['from', 'to']) {
            const at = path.join(sand, dir);
            const names = fs.readdirSync(at).sort().join('  ');
            console.log(`  ${dir}/  ${names || '(空)'}`);
        }
    } finally {
        el.kill();
        fs.rmSync(sand, { recursive: true, force: true });
    }

    if (crashes.length) {
        console.log('\n落ちた:');
        crashes.forEach((c) => console.log('  ' + c));
    }
    if (said.length) {
        console.log('\nページが言ったこと:');
        said.forEach((c) => console.log('  ' + c));
    }
    // A key that changed nothing is not always wrong — pressing `,` twice only
    // reverses — so this reports rather than fails. The crashes are the failure.
    console.log(`\n動かなかったキー ${bad} 件、例外 ${crashes.length} 件`);
    process.exit(crashes.length ? 1 : 0);
}

main().catch((e) => { console.error(e.message); process.exit(1); });
