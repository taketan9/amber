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
    // `+` is both a key and the separator between modifiers, so splitting on
    // it left the spec `"+"` with no key at all and the page was handed an
    // empty string. A step pressing `+` therefore looked like a key the app
    // ignored, and the app had never been given anything to ignore. Stand the
    // final `+` aside before splitting.
    let plus = false;
    if (spec.endsWith('+') && spec.length > 1 ? spec[spec.length - 2] === '+' : spec === '+') {
        plus = true;
        spec = spec.slice(0, -1);
    }
    const parts = spec.split('+');
    if (plus) parts.push('+');
    const base = parts.pop();
    const mods = parts.map((m) => m.toLowerCase());
    let bits = 0;
    if (mods.includes('alt')) bits |= 1;
    if (mods.includes('ctrl')) bits |= 2;
    if (mods.includes('meta') || mods.includes('cmd')) bits |= 4;
    if (mods.includes('shift')) bits |= 8;
    const key = NAMED[base] || base;
    const v = virtual(key);
    if (v.needsShift) bits |= 8;
    delete v.needsShift;
    return {
        key,
        modifiers: bits,
        text: key.length === 1 && (bits & ~8) < 2 ? key : undefined,
        ...v,
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
    // Punctuation. cian's own handlers read `e.key` and would not care, but
    // Monaco resolves its bindings from the number — so a chord on `]` or `,`
    // was untestable until these were here.
    const PUNCT = {
        ';': ['Semicolon', 186], '=': ['Equal', 187], ',': ['Comma', 188],
        '-': ['Minus', 189], '.': ['Period', 190], '/': ['Slash', 191],
        '`': ['Backquote', 192], '[': ['BracketLeft', 219], '\\': ['Backslash', 220],
        ']': ['BracketRight', 221], "'": ['Quote', 222],
        // The shifted ones. Without a virtual key code the page is handed an
        // empty `key`, so a step pressing `+` looked like a key the app
        // ignored — and the app was never given anything to ignore.
        '+': ['Equal', 187, true], ':': ['Semicolon', 186, true], '?': ['Slash', 191, true],
        '<': ['Comma', 188, true], '>': ['Period', 190, true], '~': ['Backquote', 192, true],
        '_': ['Minus', 189, true], '"': ['Quote', 222, true], '|': ['Backslash', 220, true],
    };
    const p = PUNCT[key];
    // The third element says "this character *is* the shifted one", and the
    // shift has to be in the modifiers or Chromium works the key back out
    // from the physical position and hands the page `=` where `+` was meant —
    // or, with no code at all, an empty string.
    if (p) return { code: p[0], windowsVirtualKeyCode: p[1], needsShift: !!p[2] };
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
        // Before renderer.js has been evaluated, `state` is not defined and
        // the read throws — which is "not ready yet", the exact thing this
        // loop exists to wait out. It killed the whole run instead, every
        // run, once mermaid's 3.4MB slowed the page past the first poll.
        let cwd = null;
        try {
            cwd = await cdp.read('state?.left?.cwd ?? null');
        } catch { /* the page is still loading */ }
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
                // The URL too: "failed to load resource" without saying which
                // resource is the least useful true sentence a browser says.
                const e = msg.params.entry;
                cdp.said.push(`${e.source}: ${e.text}${e.url ? '  ← ' + e.url : ''}`);
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
        if (r.exceptionDetails) {
            // `.text` on its own is the word "Uncaught" and nothing else,
            // which is worse than no message at all — it names the category
            // and hides the fault.
            const d = r.exceptionDetails;
            throw new Error(d.exception?.description || d.exception?.value || d.text);
        }
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
    shell: document.querySelector('#shell:not([hidden])')
        ? { about: document.getElementById('s-about').textContent,
            panes: [...document.querySelectorAll('#s-panes .sgrid')]
                .map((n) => n.style.left + '+' + n.style.width
                    + (n.classList.contains('on') ? '◀' : '')).join(' '),
            text: [...document.querySelectorAll('#s-panes .sgrid.on > div')]
                // Doubled on purpose: LOOK is a template literal, and inside
                // one a lone \\s is just an s. The regex reaching the page was
                // /s+$/ — it trimmed trailing letters and left the spaces.
                .map((d) => d.textContent.replace(/[ ]+$/, ''))
                .filter(Boolean).slice(-3).join(' ⏎ ') }
        : null,
    report: document.querySelector('#report:not([hidden])')
        ? { name: document.getElementById('r-name').textContent,
            about: document.getElementById('r-about').textContent,
            rows: document.querySelectorAll('#report .hit').length,
            first: document.querySelector('#report .hit')?.textContent }
        : null,
    view: document.querySelector('#view:not([hidden])')
        ? { pic: document.querySelector('#v-pic:not([hidden]) img, #v-pic:not([hidden]) embed')
                ? document.querySelector('#v-pic img, #v-pic embed').src.slice(0, 24) + '…' : null,
            about: document.getElementById('v-about').textContent,
            foot: document.getElementById('v-foot').textContent,
            first: document.querySelector('.view-line')?.textContent,
            lines: document.querySelectorAll('.view-line').length }
        : null,
    // The viewer's own idea of itself, not just its DOM. "F3 did nothing" has
    // two very different causes — never asked, or asked and stuck half-open —
    // and the sheet's hidden attribute cannot tell them apart.
    // No backtick in here: LOOK is one template literal, and one inside ends it.
    vstate: (() => { try { return (viewer.on ? 'on' : 'off') + (viewer.opening ? '+opening' : ''); } catch { return null; } })(),
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
    // A diagram, for the mermaid preview.
    fs.writeFileSync(path.join(sand, 'from', 'zu.md'),
        '# 図\n\n```mermaid\ngraph LR\n  A[開始] --> B{判定}\n  B -->|yes| C[実行]\n  B -->|no| D[終了]\n```\n\nおわり\n');
    // Brackets, for `%`.
    fs.writeFileSync(path.join(sand, 'from', 'k.rs'),
        'fn main() {\n    let x = (1 + 2);\n    println!("hi");\n}\n');
    // A binary, for the hex editor.
    fs.writeFileSync(path.join(sand, 'from', 'z.bin'), Buffer.from('HELLO WORLD\u0000\u0001', 'latin1'));
    // A picture, for F3 on something the window draws rather than reads.
    fs.writeFileSync(path.join(sand, 'from', 'p.png'), Buffer.from(
        'iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAYAAADED76LAAAAHElEQVQoz2NgGAWjYBSMglEwCkbB'
        + 'KBgFo2AUjAIAB9wAAeEjBaEAAAAASUVORK5CYII=', 'base64'));
    for (const name of ['あ.txt', 'b.md', 'c.rs']) {
        // Long enough that G and gg have somewhere to go, and one of them in
        // Shift_JIS — the encoding the viewer exists to get right, and the one
        // a machine in Tokyo meets in every log it did not write.
        // The markdown one gets headings, so :outline has something to find.
        const body = name === 'b.md'
            ? ['# 見出し一', '本文', '## 小見出し A', '本文', '## 小見出し B', '本文',
               '# 見出し二', ...Array.from({ length: 33 }, (_, i) => `${i + 1} 行目`)].join('\n')
            : Array.from({ length: 40 }, (_, i) => `${i + 1} 行目 ${name} テスト`).join('\n');
        if (name === 'あ.txt') {
            fs.writeFileSync(path.join(sand, 'from', name), sjis(body + '\n'));
        } else {
            fs.writeFileSync(path.join(sand, 'from', name), body + '\n');
        }
    }

    const keys = process.argv.slice(2);
    const round = keys.length ? keys.map((k) => [k, '']) : [
        [',', 'ソート'], [',', 'ソートもう一度'],
        // The first row is 隠しファイル in both builds now (cian-tui's
        // `toggle_rows` order). It said 配色を送る and pressed whatever was
        // second, which turned input-sync on and left it on for the rest of
        // the round — a label that had drifted from what the key does.
        ['T', 'トグルを開く'], ['Enter', '隠しファイルを切り替える'], ['Esc', '閉じる'],
        // With an input method holding the character. A terminal never sees
        // this event, which is why both builds shipped a helper to switch the
        // IME off; the window reads the physical key instead and leaves the
        // person's input method alone.
        ['ime:j', 'IME 中でも下へ'], ['ime:k', 'IME 中でも上へ'],
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
        // vim style is the default in both builds now, so the round asks for
        // insert mode before typing. It typed `XX` into normal mode instead —
        // two motions — and then "saved" a file it had not changed, which is a
        // write test that cannot fail.
        ['i', '挿入モードへ'], ['type:XX', '打つ'], ['Esc', 'ノーマルへ'],
        ['Mod+s', '保存'], ['wait:900', ''],
        ['Esc', ''], ['Esc', ''], ['Esc', '3回で閉じる'],
    ];

    // Its own config directory, inside the sandbox.
    //
    // Without this the driver ran against the real ~/.config/cian, and every
    // `:editstyle vim` or `:view icons` it typed while testing was written
    // into somebody's actual settings — quietly, and the next run then
    // started from whatever the last test had left. A test that changes what
    // it is testing is not a test. An explicit CIAN_CONFIG_DIR still wins, so
    // a keymap.lua can be handed in on purpose.
    const conf = process.env.CIAN_CONFIG_DIR || path.join(sand, 'config');
    fs.mkdirSync(conf, { recursive: true });

    const el = spawn(process.env.CIAN_ELECTRON
        || path.join(__dirname, 'node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'),
        [__dirname, path.join(sand, 'from'), `--remote-debugging-port=${PORT}`],
        { cwd: ROOT, stdio: ['ignore', 'pipe', 'pipe'],
          env: { ...process.env, CIAN_CONFIG_DIR: conf } });

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

        // What a state looks like in one line. Written once: the `click:` and
        // `ime:` steps printed only the status, so a menu that opened was
        // invisible in the report — which is the fault that once made a
        // working F3 read as dead.
        const marks = (after) => {
            const asking = after.prompt ? `  ｜: ${after.prompt}  枠 ${after.frame}`
                : (after.asking ? `  ⟨${after.asking}⟩ 焦点=${after.focused}` : null);
            const menu = after.sheet && after.rows
                ? `  ▣ ${after.rows}項目（${after.at + 1}番目）`
                : null;
            const sh = after.shell
                ? `  ▸ ${after.shell.about}  [${after.shell.panes}]  «${after.shell.text}»`
                : null;
            const rep = after.report
                ? `  ▤ ${after.report.name} ｜${after.report.about}｜ ${after.report.rows}行  «${after.report.first}»`
                : null;
            const vst = after.vstate && after.vstate !== 'off' ? `  ◈${after.vstate}` : '';
            const view = after.view
                ? (after.view.pic
                    ? `  ▦ ${after.view.about}  ${after.view.pic}`
                    : `  ｜${after.view.foot}  ${after.view.about}  «${after.view.first}»`)
                : null;
            return vst + (asking ?? rep ?? menu ?? view ?? sh
                ?? (after.marks.length ? `  [${after.marks.join(' ')}]` : ''));
        };

        for (const [key, what] of round) {
            // `list` reads instead of pressing: every row of whatever sheet is
            // open, with its right-hand column. The menus were compared with
            // cian-tui's by reading source on both sides, which is how six
            // labels drifted without anything noticing — a menu nobody ever
            // reads back is a menu that says whatever it last said.
            // `shot:name` writes a PNG next to the sandbox. Reading rows back
            // says what a menu *says*; only a picture says whether it fits.
            if (key.startsWith('shot:')) {
                const png = await cdp.send('Page.captureScreenshot', { format: 'png' });
                // Not in the sandbox: that is deleted when the run ends, and
                // a picture you cannot open afterwards is not evidence.
                const dir = path.join(os.tmpdir(), 'cian-shots');
                fs.mkdirSync(dir, { recursive: true });
                const at = path.join(dir, `${key.slice(5)}.png`);
                fs.writeFileSync(at, Buffer.from(png.data, 'base64'));
                console.log(`  shot    ${what || key.slice(5)}  → ${at}`);
                continue;
            }
            // `click:<css>` — press the mouse on the first match. The whole
            // mouse surface was untestable: every check up to now sent keys,
            // and the differences that kept surviving (the ◀ ▶ arrows, the
            // breadcrumb segments, the dividers) are all things you can only
            // reach with a pointer.
            if (key.startsWith('click:')) {
                const sel = key.slice(6);
                const box = await cdp.read(`(() => {
                    const n = document.querySelector(${JSON.stringify(sel)});
                    if (!n) return null;
                    const b = n.getBoundingClientRect();
                    return { x: Math.round(b.left + b.width / 2), y: Math.round(b.top + b.height / 2) };
                })()`);
                if (!box) {
                    console.log(`× ${key.padEnd(8)}${(what || '').padEnd(16)} 見つかりません`);
                    bad++;
                    continue;
                }
                const before = await cdp.read(LOOK);
                for (const type of ['mousePressed', 'mouseReleased']) {
                    await cdp.send('Input.dispatchMouseEvent', {
                        type, x: box.x, y: box.y, button: 'left', clickCount: 1,
                    });
                }
                await sleep(250);
                const after = await cdp.read(LOOK);
                const moved = JSON.stringify(before) !== JSON.stringify(after);
                console.log(`${moved ? '  ' : '× '}${key.padEnd(8)}${(what || '').padEnd(16)} ${after.status}${marks(after)}`);
                if (!moved) bad++;
                continue;
            }
            // `ime:j` — the keydown a browser reports while an input method
            // holds the character: `Process`, virtual key 229, and the
            // physical key still named. It is the only way to test the IME
            // road without a Japanese IME on this machine, and the road exists
            // precisely because a terminal never sees this event at all.
            if (key.startsWith('ime:')) {
                const ch = key.slice(4);
                const before = await cdp.read(LOOK);
                for (const type of ['rawKeyDown', 'keyUp']) {
                    await cdp.send('Input.dispatchKeyEvent', {
                        type,
                        key: 'Process',
                        code: `Key${ch.toUpperCase()}`,
                        windowsVirtualKeyCode: 229,
                        nativeVirtualKeyCode: 229,
                        modifiers: ch === ch.toUpperCase() && /[A-Z]/.test(ch) ? 8 : 0,
                    });
                }
                await sleep(200);
                const after = await cdp.read(LOOK);
                const moved = JSON.stringify(before) !== JSON.stringify(after);
                console.log(`${moved ? '  ' : '× '}${key.padEnd(8)}${(what || '').padEnd(16)} ${after.status}${marks(after)}`);
                if (!moved) bad++;
                continue;
            }
            // `drag:<css>:dx,dy` — press on the first match, move by that
            // many pixels, release. The dividers and the file rows are the
            // two things in this window that only a held pointer can work,
            // and neither could be reached from here.
            if (key.startsWith('drag:')) {
                const cut = key.lastIndexOf(':');
                const sel = key.slice(5, cut);
                const [dx, dy] = key.slice(cut + 1).split(',').map(Number);
                const box = await cdp.read(`(() => {
                    const n = document.querySelector(${JSON.stringify(sel)});
                    if (!n) return null;
                    const b = n.getBoundingClientRect();
                    return { x: Math.round(b.left + b.width / 2), y: Math.round(b.top + b.height / 2) };
                })()`);
                if (!box) {
                    console.log(`× ${key.padEnd(8)}${(what || '').padEnd(16)} 見つかりません`);
                    bad++;
                    continue;
                }
                const before = await cdp.read(LOOK);
                const at = { x: box.x, y: box.y };
                await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', ...at, button: 'left', clickCount: 1 });
                // In steps: a drag handler that follows the pointer needs
                // moves to follow, and one jump is not a drag.
                for (let i = 1; i <= 5; i++) {
                    await cdp.send('Input.dispatchMouseEvent', {
                        type: 'mouseMoved',
                        x: box.x + Math.round((dx * i) / 5),
                        y: box.y + Math.round((dy * i) / 5),
                        button: 'left', buttons: 1,
                    });
                    await sleep(40);
                }
                await cdp.send('Input.dispatchMouseEvent', {
                    type: 'mouseReleased', x: box.x + dx, y: box.y + dy, button: 'left', clickCount: 1,
                });
                await sleep(300);
                const after = await cdp.read(LOOK);
                const moved = JSON.stringify(before) !== JSON.stringify(after);
                console.log(`${moved ? '  ' : '× '}${key.padEnd(8)}${(what || '').padEnd(16)} ${after.status}${marks(after)}`);
                if (!moved) bad++;
                continue;
            }
            // `read:<expr>` — evaluate something in the page and print it.
            // Added the third time a state was diagnosed by reasoning about
            // which listener ran first, which is a way of being wrong slowly.
            if (key.startsWith('read:')) {
                let out;
                try {
                    out = await cdp.read(key.slice(5));
                } catch (err) {
                    out = `例外: ${err.message}`;
                }
                console.log(`  read    ${what || key.slice(5)} = ${JSON.stringify(out)}`);
                continue;
            }
            if (key === 'list') {
                const rows = await cdp.read(`[...document.querySelectorAll('#find:not([hidden]) .hit, #report:not([hidden]) .hit')]
                    .map((e) => e.textContent.replace(/\\s+/g, ' ').trim())`);
                console.log(`  list    ${what}`);
                for (const r of rows) console.log(`            ${r}`);
                continue;
            }
            const before = await cdp.read(LOOK);
            await cdp.press(key);
            const after = await cdp.read(LOOK);
            const moved = JSON.stringify(before) !== JSON.stringify(after);
            const note = what ? `  ${what}` : '';
            // The viewer before the shell. The shell panel is open from
            // startup now, so a report that prefers it can never show the
            // viewer — which read as "F3 did nothing" for a whole afternoon
            // while F3 was working fine.
            console.log(`${moved ? '  ' : '× '}${key.padEnd(8)}${note.padEnd(16)} ${after.status}${marks(after)}`);
            if (!moved) bad++;
        }
        // Let the last job finish before looking. A copy started by the
        // final key is still running when the loop ends.
        await sleep(600);
        console.log(`\n最後の状態: ${(await cdp.read(LOOK)).status}`);
        console.log('砂場:');
        for (const extra of ['from/展開先']) {
            const at = path.join(sand, ...extra.split('/'));
            if (fs.existsSync(at)) {
                console.log(`  ${extra}/  ${fs.readdirSync(at).sort().join('  ') || '(空)'}`);
            }
        }
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
