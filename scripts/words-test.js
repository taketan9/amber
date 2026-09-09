#!/usr/bin/env node
/* **同じものを、同じ名前で呼んでいるか**（依頼 463）。
 *
 *     node scripts/words-test.js
 *
 * 注記の見出し（NOTE・TIP・…）の名前は**三か所にある** ── 核が組む字、
 * 窓が選ばせる一覧、電話が出す見出し。三つとも同じでなければ、同じノートが
 * 端末によって違う言葉で出る。
 *
 * **実際にずれていた。** 核と窓は「ノート／こつ／大事／注意／危険」、電話
 * だけ「おぼえておく／こつ／大事／注意／あぶない」── 誰も気づかないまま
 * 置いていかれていた（本人が言葉を直そうとして見つかった）。
 *
 * 見るのは形ではなく**字そのもの**。増やすときは、三か所とも直すことになる。
 */
'use strict';
const fs = require('fs');
const path = require('path');
const root = path.join(__dirname, '..');
const read = (p) => fs.readFileSync(path.join(root, p), 'utf8');

const KINDS = ['note', 'tip', 'important', 'warning', 'caution'];

/// 核（Rust）── `"note" => "備忘",` の並び。
function fromCore() {
    const s = read('crates/amber-core/src/markdown.rs');
    const out = {};
    for (const k of KINDS.slice(0, 4)) {
        const m = new RegExp('"' + k + '"\\s*=>\\s*"([^"]+)"').exec(s);
        if (m) out[k] = m[1];
    }
    // `caution` は `_ =>`（残りぜんぶ）で受けている。
    const rest = /_\s*=>\s*"([^"]+)",\n\s*\};\n\s*out\.push_str\(&format!\(\n\s*"<div class=\\"alert/.exec(s)
        || /"warning" => "[^"]+",\n\s*_ => "([^"]+)"/.exec(s);
    if (rest) out.caution = rest[1];
    return out;
}

/// 窓（JS）── 選ばせる一覧。
function fromWindow() {
    const s = read('gui/renderer.js');
    const out = {};
    for (const k of KINDS) {
        const m = new RegExp("\\{ name: '([^']+)',[^}]*value: '" + k.toUpperCase() + "' \\}").exec(s);
        if (m) out[k] = m[1];
    }
    return out;
}

/// 電話（Swift）── 見出しの表。
function fromPhone() {
    const s = read('ios/Cian/Reading.swift');
    const out = {};
    for (const k of KINDS) {
        const m = new RegExp('"' + k + '":\\s*\\("([^"]+)"').exec(s);
        if (m) out[k] = m[1];
    }
    return out;
}

const three = { 核: fromCore(), 窓: fromWindow(), 電話: fromPhone() };
let bad = 0;
for (const k of KINDS) {
    const said = Object.entries(three).map(([who, table]) => [who, table[k]]);
    const missing = said.filter(([, v]) => !v).map(([who]) => who);
    if (missing.length) {
        bad += 1;
        console.log('✗ ' + k + ' の名前が読めません: ' + missing.join('・'));
        continue;
    }
    const uniq = [...new Set(said.map(([, v]) => v))];
    if (uniq.length > 1) {
        bad += 1;
        console.log('✗ ' + k + ' の名前がずれています: '
            + said.map(([who, v]) => who + '「' + v + '」').join(' / '));
    }
}

console.log('');
if (bad) {
    console.log(KINDS.length + ' 件中 ' + bad + ' 件ずれています');
    process.exit(1);
}
console.log('同じものを、同じ名前で呼んでいます（' + KINDS.length + ' 件）');
