#!/usr/bin/env node
// **八色の表が、三か所で揃っているか**（依頼 711）。
//
//     node scripts/colors-test.js
//
// 色を決めるのは core（`caltint`）── `#色:カーマイン` の「カーマイン」を
// `#c0392b` に直すのはあちらの仕事。それでも **iPhone とデスクトップ版も同じ表を
// 持っている** ── 選ぶ丸を並べるのに要るからで、そこは画面の仕事。
//
// **三つが食い違うと、黙って嘘になる。** 画面に「カーマイン」と出して選ばせ、
// core が別の色を返すと、選んだ色と塗られた色が違う。しかも**一方の端末だけ**
// ずれるので、気づくのは「iPhone と Mac で色が違う」と言われたとき。
//
// これは `packaging/amber.py`（葉の形を二か所で一致させる）と同じ形の検査。
// 同じものを持たせないのが本筋だが、八色の表は画面が丸を描くのに要るので、
// **持たせたうえで、ずれたら落とす。**
'use strict';

const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const read = (p) => fs.readFileSync(path.join(root, p), 'utf8');

let bad = 0;
function no(why) {
    console.error('✗ ' + why);
    bad += 1;
}

/// core の表（`crates/amber-core/src/caltint.rs` の `COLORS`）。
function fromCore() {
    const src = read('crates/amber-core/src/caltint.rs');
    const block = src.match(/pub const COLORS: \[\(&str, &str\); (\d+)\] = \[([\s\S]*?)\n\];/);
    if (!block) { no('core の八色の表が見つかりません（caltint.rs の COLORS）'); return null; }
    const said = Number(block[1]);
    const out = [...block[2].matchAll(/\("(#[0-9a-f]{6})",\s*"([^"]+)"\)/g)].map((m) => [m[1], m[2]]);
    // **宣言した数と、並んでいる数が合っているか。** 合っていないと、
    // 書いたつもりの色が一つ落ちていても気づけない。
    if (out.length !== said) no(`core は ${said} 色と言っているのに、${out.length} 色しか並んでいません`);
    return out;
}

/// iPhone の表（`ios/Cian/CalPrefs.swift` の `colors`）。
function fromPhone() {
    const src = read('ios/Cian/CalPrefs.swift');
    const block = src.match(/static let colors: \[\(hex: String, name: String\)\] = \[([\s\S]*?)\n    \]/);
    if (!block) { no('iPhone の八色の表が見つかりません（CalPrefs.swift の colors）'); return null; }
    return [...block[1].matchAll(/\("(#[0-9a-f]{6})",\s*"([^"]+)"\)/g)].map((m) => [m[1], m[2]]);
}

/// デスクトップ版の表（`gui/renderer.js` の `CAL_COLORS`）。
function fromWindow() {
    const src = read('gui/renderer.js');
    const block = src.match(/const CAL_COLORS = \[([\s\S]*?)\n\];/);
    if (!block) { no('デスクトップ版の八色の表が見つかりません（renderer.js の CAL_COLORS）'); return null; }
    return [...block[1].matchAll(/\['(#[0-9a-f]{6})',\s*'([^']+)'\]/g)].map((m) => [m[1], m[2]]);
}

const core = fromCore();
const phone = fromPhone();
const win = fromWindow();

if (core && core.length === 0) no('core の表が空です');

// **並び順まで見る。** 決めていない色を順に配るときの並びでもあるので
// （`LANE_COLORS`）、順が違えば二人目の色が端末によって変わる。
for (const [who, got] of [['iPhone', phone], ['デスクトップ版', win]]) {
    if (!core || !got) continue;
    if (got.length !== core.length) {
        no(`${who} は ${got.length} 色、core は ${core.length} 色 ── 数が違います`);
        continue;
    }
    for (let i = 0; i < core.length; i += 1) {
        if (got[i][0] !== core[i][0] || got[i][1] !== core[i][1]) {
            no(`${who} の ${i + 1} 番目が違います: ${got[i][0]} ${got[i][1]}`
               + ` ── core は ${core[i][0]} ${core[i][1]}`);
        }
    }
}

if (bad) {
    console.error('');
    console.error('八色の表が食い違っています。**core に合わせてください**'
                  + '（色を決めるのは core の仕事です）。');
    process.exit(1);
}

console.log(`八色の表は三か所で揃っています（${core.length} 色・core / iPhone / デスクトップ版）`);
