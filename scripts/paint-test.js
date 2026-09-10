#!/usr/bin/env node
/* **無い色を使っていないか**（依頼 474）。
 *
 *     node scripts/paint-test.js
 *
 * CSS の変数は、綴りを間違えても**何も言わずに効かなくなる**。
 * `var(--ink3)` と書いたところは（本当は `--ink-3`）、字の色なら
 * 親から継いだ色になり、背景なら**透明**になる ── 画面は出ているのに、
 * 描いたはずの帯がどこにも無い、という形で現れる。
 *
 * 実際に出た: カレンダーの「終日」の帯が一日ぶん敷いてあるのに、
 * 画面には何も無かった（`background: color-mix(…var(--ink3)…)` が
 * まるごと透明になっていた）。日曜の赤も、ずっと出ていなかった。
 *
 * だから **`var(--…)` で呼んでいる名前が、どこかで定義されているか**を
 * 数える。中身が正しいかまでは見ない ── そこは目で見るしかない。
 */
'use strict';
const fs = require('fs');
const path = require('path');

const files = ['gui/index.html'];
let bad = 0;

for (const rel of files) {
    const src = fs.readFileSync(path.join(__dirname, '..', rel), 'utf8');
    // 定義されている名前（`--なにか:` の形で書いてあるもの）。
    const known = new Set();
    for (const m of src.matchAll(/(--[a-zA-Z0-9-]+)\s*:/g)) known.add(m[1]);
    // 呼んでいる名前。
    const used = new Map();
    for (const m of src.matchAll(/var\(\s*(--[a-zA-Z0-9-]+)/g)) {
        used.set(m[1], (used.get(m[1]) || 0) + 1);
    }
    for (const [name, times] of used) {
        if (known.has(name)) continue;
        // **予備が書いてあるなら、それでよい**（`var(--x, #fff)`）。
        const withFallback = new RegExp(
            'var\\(\\s*' + name.replace(/-/g, '\\-') + '\\s*,').test(src);
        if (withFallback) continue;
        bad += 1;
        console.log(`✗ ${rel}: ${name} を ${times} か所で呼んでいますが、`
            + 'どこにも定義がありません');
    }
    console.log(`${rel}: 色の名前 ${known.size} 個、呼び出し ${used.size} 種`);
}

console.log(bad ? `\n${bad} 件、無い色を呼んでいます` : '\n呼んでいる色は、ぜんぶ定義されています');
process.exit(bad ? 1 : 0);
