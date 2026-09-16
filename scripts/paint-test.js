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

/* **同じ予定は、どの見方でも同じ色**（依頼 478）。
 *
 * カレンダーには見方が五つある（月の升目・週と日の帯・終日の段・
 * グループの帯・グループの升目）。色は五か所に別々に書いてあるので、
 * 種類を一つ足すと**どこかを書き忘れる** ── 実際、チームの紫を足した
 * とき、月の升目と平の週表の二か所が琥珀のまま残っていた。同じ予定が
 * 見方によって別のものに見えるのは、いちばん困る。
 */
{
    const src = fs.readFileSync(path.join(__dirname, '..', 'gui', 'index.html'), 'utf8');
    // 色を付けている種類（自分のノートは既定の琥珀なので数えない）。
    const kinds = ['away', 'here', 'team'];
    const places = [
        ['月の升目', '#calbox .d .ev.'],
        ['週と日の帯', '#calbox .blk.'],
        ['終日の段', '#calbox .allday .ad.'],
        ['グループの帯', '#calbox .crowd .bar.'],
        ['グループの升目', '#calbox .crowd .chip.'],
    ];
    for (const kind of kinds) {
        for (const [where, prefix] of places) {
            // **前方一致では数えない。** `.team` は `.teamX` にも当たるので、
            // 名前の切れ目まで見る（変異テストがそこで黙った）。
            const at = new RegExp(
                (prefix + kind).replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '(?![\\w-])');
            if (at.test(src)) continue;
            bad += 1;
            console.log(`✗ ${where}に「${kind}」の色がありません`
                + ` ── 同じ予定が、見方によって別の色になります`);
        }
    }
    console.log(`予定の色: ${kinds.length} 種 × ${places.length} の見方`);
}

/* **押せるものは、乗せたら応える**（依頼 613・本人「マウスが上に乗っている
 * 時の挙動を全般的に見直ししてもらえないかな？」）。
 *
 * 指の形（`cursor: pointer`）だけ出ていて色が一つも動かないと、押せると
 * 思ってもらえない ── 実際、カレンダーの予定も「使われていない画像」の升も
 * 押せるのに、乗せても何も起きなかった。
 *
 * **`#sheet .it` はここで数えない** ── あれは鍵盤と同じ選び目を動かすので、
 * CSS の `:hover` ではなく `onmouseenter` が受ける（`win-test.js` が見る）。
 */
{
    const src = fs.readFileSync(path.join(__dirname, '..', 'gui', 'index.html'), 'utf8');
    const css = src.slice(src.indexOf('<style>'), src.indexOf('</style>'));
    // 指の形を出している顔ぶれ（`#sheet .it` は上のとおり別扱い）。
    const want = ['#band .k', '#read .gadget button', '#sparebox .cell',
                  '#calbox .blk', '#calbox .crowd .chip', '#calbox .crowd .bar',
                  '#calbox .allday .ad', '#calbox .crowd .span',
                  '.row', '.dest', '.tab', '#picked button', '#more button'];
    for (const sel of want) {
        // **名前の切れ目まで見る。** `#sparebox .cell` の検査が
        // `#sparebox .cell.on:hover` に当たって、素の手応えを外しても
        // 通っていた（変異テストがそこで黙った）。
        const at = new RegExp(sel.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + ':hover');
        if (at.test(css)) continue;
        bad += 1;
        console.log(`✗ ${sel} は押せるのに、乗せても何も起きません`);
    }
    console.log(`乗せたら応えるもの: ${want.length} 種`);
}

console.log(bad ? `\n${bad} 件、色が揃っていません` : '\n呼んでいる色は、ぜんぶ定義されていて、見方ごとに揃っています');
process.exit(bad ? 1 : 0);
