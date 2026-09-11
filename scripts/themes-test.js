#!/usr/bin/env node
/* **amber のテーマは cian と同じか**（依頼 495）。
 *
 *     node scripts/themes-test.js          # 隣の cian と見比べる（無ければ飛ばす）
 *     node scripts/themes-test.js --write  # cian から `gui/palettes.js` を作り直す
 *
 * 本人が決めたこと（2026-09-12）: 「全テーマを全く同一に合わせたい」。cian は
 * 十八の配色（`cian-core/src/theme.rs` の `PRESETS`）と窓の三つの装い（白磁・
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

/// cian の窓の三つの装い（`index.html` の `:root` と `[data-look=…]`）。
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

/// 窓の三つの装い（白磁・陰翳・端末譲り）── cian の \`index.html\` の変数そのまま。
const CIAN_LOOKS = {
${['hakuji', 'inei', 'terminal'].map(look).join('\n')}
};
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
        console.log('書きました: gui/palettes.js（' + palettes.length + ' 配色・3 装い）');
        return;
    }
    console.log('cian と同じか ── 十八の配色と三つの装い');
    ok(palettes.length === 18, 'cian の配色は十八', palettes.length);
    const mine = fs.readFileSync(OUT, 'utf8');
    ok(mine === render(palettes, looks), 'gui/palettes.js は cian の写しのまま（違えば --write で作り直す）');
    // 窓の側が全部を出しているか。
    const renderer = fs.readFileSync(path.join(here, 'gui/renderer.js'), 'utf8');
    for (const p of palettes) ok(renderer.includes("'" + p.name + "'") || renderer.includes('CIAN_PALETTES'), '窓に ' + p.name + ' がある');
    for (const k of ['hakuji', 'inei', 'terminal']) ok(renderer.includes("'" + k + "'"), '窓に装い ' + k + ' がある');
    console.log(bad ? '\n' + bad + ' 件ちがいます' : '\ncian と同じです（18 配色・3 装い）');
    process.exit(bad ? 1 : 0);
})();
