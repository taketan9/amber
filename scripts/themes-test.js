#!/usr/bin/env node
/* **amber のテーマは cian と同じか**（依頼 495）。
 *
 *     node scripts/themes-test.js          # 隣の cian と見比べる（無ければ飛ばす）
 *     node scripts/themes-test.js --write  # cian から `gui/palettes.js` を作り直す
 *
 * 本人が決めたこと（2026-09-12）: 「全テーマを全く同一に合わせたい」。cian は
 * 十八の配色（`cian-core/src/theme.rs` の `PRESETS`）とデスクトップ版の3 つの装い（白磁・
 * 陰翳・端末譲り・`gui/index.html`）を持つ。amber は cian を知らない（依存は
 * cian → amber の一方向）ので、**表を写して持ち、ここで写しが古くなっていないか
 * を見る**。cian 側で色が一つ変わった日に、ここが鳴る。
 */
'use strict';
const fs = require('node:fs');
const path = require('node:path');

const here = path.dirname(__dirname);
const cian = path.join(path.dirname(here), 'cian');
const OUT = path.join(here, 'gui', 'palettes.js');
/// iPhone の側の写し（Swift）。デスクトップ版の変数に組み替えた**あと**の色を持つ ── 組み替えの
/// 算数は `palettes.js` の `amberVarsOf` 一つ（デスクトップ版と iPhone で答えがずれない）。
const OUT_SWIFT = path.join(here, 'ios', 'Cian', 'Palettes.swift');
const LABELS = { hakuji: '白磁', inei: '陰翳', terminal: '端末譲り' };

const FIELDS = ['bg', 'fg', 'dim', 'border', 'accent', 'sel', 'visual', 'mark', 'popup', 'status',
    'blue', 'yellow', 'cyan', 'magenta', 'red', 'green', 'doc'];
const LOOK_VARS = ['bg', 'pane', 'pane-off', 'line', 'text', 'dim', 'dir', 'accent', 'accent-dim',
    'on-accent', 'sel-strong', 'row-hover', 'mark'];

/// cian-core の `theme.rs` から、十八の配色を並び順のまま。
function readPalettes() {
    const src = fs.readFileSync(path.join(cian, 'crates/cian-core/src/theme.rs'), 'utf8');
    const consts = new Map();
    for (const m of src.matchAll(/pub const ([A-Z_]+): Spec = Spec \{([^}]*)\}/g)) {
        const spec = {};
        for (const f of m[2].matchAll(/(\w+):\s*0x([0-9a-fA-F]{6})/g)) spec[f[1]] = '#' + f[2].toLowerCase();
        consts.set(m[1], spec);
    }
    const order = src.slice(src.indexOf('pub const PRESETS'));
    const out = [];
    for (const m of order.matchAll(/\("([a-z0-9-]+)",\s*([A-Z_]+)\)/g)) {
        const spec = consts.get(m[2]);
        if (!spec) throw new Error('見つかりません: ' + m[2]);
        out.push({ name: m[1], ...Object.fromEntries(FIELDS.map((k) => [k, spec[k]])) });
    }
    return out;
}

/// cian のデスクトップ版の3 つの装い（`index.html` の `:root` と `[data-look=…]`）。
function readLooks() {
    const src = fs.readFileSync(path.join(cian, 'gui/index.html'), 'utf8');
    const block = (start) => {
        const at = src.indexOf(start);
        if (at < 0) throw new Error('見つかりません: ' + start);
        const end = src.indexOf('\n}', at);
        const vars = {};
        for (const m of src.slice(at, end).matchAll(/--([a-z-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) {
            if (!(m[1] in vars)) vars[m[1]] = m[2].toLowerCase();
        }
        return Object.fromEntries(LOOK_VARS.map((k) => [k, vars[k]]));
    };
    return { hakuji: block(':root {'), inei: block(':root[data-look="inei"]'), terminal: block(':root[data-look="terminal"]') };
}

function render(palettes, looks) {
    const row = (p) => '    { name: ' + JSON.stringify(p.name) + ', '
        + FIELDS.map((k) => k + ': ' + JSON.stringify(p[k])).join(', ') + ' },';
    const look = (k) => '    ' + k + ': { ' + LOOK_VARS.map((v) => JSON.stringify(v) + ': ' + JSON.stringify(looks[k][v])).join(', ') + ' },';
    return `/* **cian の配色の写し**（依頼 495）。手で直さない ──
 *
 *     node scripts/themes-test.js --write
 *
 * で隣の cian（\`crates/cian-core/src/theme.rs\` と \`gui/index.html\`）から作り直す。
 * 本人が決めた: 「全テーマを全く同一に合わせたい」。amber は cian を知らない
 * （依存は cian → amber の一方向）ので、表を写して持つ。写しが古くなれば
 * \`themes-test\` が鳴る。
 */
'use strict';

/// 十八の配色。並び順は cian の \`:theme\` と同じ。
const CIAN_PALETTES = [
${palettes.map(row).join('\n')}
];

/// デスクトップ版の3 つの装い（白磁・陰翳・端末譲り）── cian の \`index.html\` の変数そのまま。
const CIAN_LOOKS = {
${['hakuji', 'inei', 'terminal'].map(look).join('\n')}
};

/* ── cian の色を、amber の十五の変数に組み替える ──
 * **ここが唯一の算数。** ウィンドウ（renderer.js）もiPhone（Palettes.swift を作るとき）も
 * これを通る ── 二か所に書くと、片方だけ直した日に同じ配色が二つの顔になる。 */

/// 明るい色か（cian-core の \`is_light\` と同じ・Rec. 601）。
function lightColor(hex) {
    const n = parseInt(String(hex).slice(1), 16);
    const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    return (299 * r + 587 * g + 114 * b) / 1000 > 128;
}
/// \`a\` を \`b\` へ \`t\` だけ寄せた色（0 なら a、1 なら b）。
function mixColor(a, b, t) {
    const rgb = (h) => [1, 3, 5].map((i) => parseInt(String(h).slice(i, i + 2), 16));
    const [ar, ag, ab] = rgb(a);
    const [br, bg, bb] = rgb(b);
    const one = (x, y) => Math.round(x + (y - x) * t).toString(16).padStart(2, '0');
    return '#' + one(ar, br) + one(ag, bg) + one(ab, bb);
}
/// 名前から \`{ light, vars }\`（amber の十五の変数）。知らない名前なら null。
function amberVarsOf(name) {
    const look = CIAN_LOOKS[name];
    if (look) {
        const light = lightColor(look.pane);
        const deep = light ? look.dir : look.accent;
        return { light, vars: {
            '--paper': look.pane, '--bg': look.bg, '--rail': look['pane-off'],
            '--list': mixColor(look.pane, look['pane-off'], 0.5),
            '--line': look.line, '--line-2': mixColor(look.line, look.pane, 0.5),
            '--ink': look.text, '--ink-2': mixColor(look.text, look.dim, 0.45), '--ink-3': look.dim,
            '--amber': look.accent, '--amber-soft': look['accent-dim'], '--amber-deep': deep,
            '--sel': look['sel-strong'], '--hover': look['row-hover'], '--brand-s': deep,
        } };
    }
    const p = CIAN_PALETTES.find((x) => x.name === name);
    if (!p) return null;
    const light = lightColor(p.bg);
    // 明るい紙では、リンクやセルに乗る濃い側を文字のほうへ寄せて読めるようにする。
    const deep = light ? mixColor(p.accent, p.fg, 0.3) : p.accent;
    return { light, vars: {
        '--paper': p.bg, '--rail': p.popup, '--list': mixColor(p.bg, p.popup, 0.5),
        '--bg': mixColor(p.bg, p.popup, 0.35),
        '--line': mixColor(p.border, p.bg, 0.4), '--line-2': mixColor(p.border, p.bg, 0.7),
        '--ink': p.fg, '--ink-2': mixColor(p.fg, p.dim, 0.45), '--ink-3': p.dim,
        '--amber': p.accent, '--amber-soft': mixColor(p.accent, p.bg, 0.55), '--amber-deep': deep,
        '--sel': p.sel, '--hover': mixColor(p.sel, p.bg, 0.5), '--brand-s': deep,
    } };
}

if (typeof module !== 'undefined') module.exports = { CIAN_PALETTES, CIAN_LOOKS, lightColor, mixColor, amberVarsOf };
`;
}

/// iPhone の写し。名前・出す名前・明るいか・十五の変数（組み替え済み）。
function renderSwift(js) {
    const names = ['hakuji', 'inei', 'terminal', ...js.CIAN_PALETTES.map((p) => p.name)];
    const rows = names.map((n) => {
        const got = js.amberVarsOf(n);
        const vars = Object.entries(got.vars).map(([k, v]) => '"' + k + '": "' + v + '"').join(', ');
        return '        Palette(name: "' + n + '", label: "' + (LABELS[n] || n) + '", light: ' + got.light + ', vars: [' + vars + ']),';
    });
    return `// **cian の配色の写し**（依頼 499）。手で直さない ──
//
//     node scripts/themes-test.js --write
//
// で隣の cian から作り直す（デスクトップ版の \`gui/palettes.js\` と同じ元・同じ算数）。
// デスクトップ版の十五の変数に組み替えたあとの色を持つ。並びは cian と同じ。

/// 一つの配色。\`vars\` は画面（WKWebView）の CSS 変数にそのまま差す。
struct Palette {
    let name: String
    let label: String
    let light: Bool
    let vars: [String: String]
}

enum Palettes {
    static let all: [Palette] = [
${rows.join('\n')}
    ]
    static func named(_ name: String) -> Palette? { all.first { $0.name == name } }
}
`;
}

let bad = 0;
const ok = (cond, what, extra) => {
    if (cond) { console.log('  ✓ ' + what); return; }
    bad += 1;
    console.log('  ✗ ' + what + (extra !== undefined ? '\n     ' + JSON.stringify(extra).slice(0, 300) : ''));
};

(function main() {
    if (!fs.existsSync(path.join(cian, 'crates/cian-core/src/theme.rs'))) {
        console.log('隣に cian がありません ── 見比べずに終わります');
        process.exit(0);
    }
    const palettes = readPalettes();
    const looks = readLooks();
    if (process.argv.includes('--write')) {
        fs.writeFileSync(OUT, render(palettes, looks));
        delete require.cache[require.resolve(OUT)];
        fs.writeFileSync(OUT_SWIFT, renderSwift(require(OUT)));
        console.log('書きました: gui/palettes.js と ios/Cian/Palettes.swift（' + palettes.length + ' 配色・3 装い）');
        return;
    }
    console.log('cian と同じか ── 十八の配色と3 つの装い');
    ok(palettes.length === 18, 'cian の配色は十八', palettes.length);
    const mine = fs.readFileSync(OUT, 'utf8');
    ok(mine === render(palettes, looks), 'gui/palettes.js は cian の写しのまま（違えば --write で作り直す）');
    const swift = fs.readFileSync(OUT_SWIFT, 'utf8');
    ok(swift === renderSwift(require(OUT)), 'ios/Cian/Palettes.swift は同じ元から作られたまま');
    // デスクトップ版の側が全部を出しているか。
    const renderer = fs.readFileSync(path.join(here, 'gui/renderer.js'), 'utf8');
    for (const p of palettes) ok(renderer.includes("'" + p.name + "'") || renderer.includes('CIAN_PALETTES'), 'デスクトップ版に ' + p.name + ' がある');
    for (const k of ['hakuji', 'inei', 'terminal']) ok(renderer.includes("'" + k + "'"), 'デスクトップ版に装い ' + k + ' がある');
    console.log(bad ? '\n' + bad + ' 件ちがいます' : '\ncian と同じです（18 配色・3 装い）');
    process.exit(bad ? 1 : 0);
})();
