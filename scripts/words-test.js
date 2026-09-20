#!/usr/bin/env node
/* **同じものを、同じ名前で呼んでいるか**（依頼 463）。
 *
 *     node scripts/words-test.js
 *
 * 注記の見出し（NOTE・TIP・…）の名前が、**核とデスクトップ版で同じか。**
 *
 * **実際にずれていた。** 核とデスクトップ版は「ノート／こつ／大事／注意／危険」、iPhone
 * だけ「おぼえておく／こつ／大事／注意／あぶない」── 誰も気づかないまま
 * 置いていかれていた（本人が言葉を直そうとして見つかった）。
 *
 * **2026-09-16、iPhone の表は無くなった**（依頼 609）── あれは死んだ画面
 * （`Reading`）の中にあり、iPhone の「表示」画面は前から核の `to_html` を
 * そのまま出している。**三つ目の表が無いほうが強い約束**なので、
 * 「iPhone は自分の表を持たない」を検査のほうに移した ── また生えたら鳴る。
 *
 * 見るのは形ではなく**文字そのもの**。増やすときは、二か所とも直すことになる。
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

/// ウィンドウ（JS）── 選ばせる一覧。
function fromWindow() {
    const s = read('gui/renderer.js');
    const out = {};
    for (const k of KINDS) {
        const m = new RegExp("\\{ name: '([^']+)',[^}]*value: '" + k.toUpperCase() + "' \\}").exec(s);
        if (m) out[k] = m[1];
    }
    return out;
}

/// **iPhone は、自分の表を持たない。**
///
/// 「表示」画面は核の `to_html` をそのまま出す ── 名前を Swift でもう一度
/// 書けば、その日から二つの amber が同じノートを違う言葉で呼ぶ。
/// ここで見るのは「無いこと」。
function phoneHasItsOwn() {
    const dir = path.join(root, 'ios', 'Cian');
    const found = [];
    for (const name of fs.readdirSync(dir).filter((n) => n.endsWith('.swift'))) {
        const s = fs.readFileSync(path.join(dir, name), 'utf8');
        // 三つ以上そろって初めて「表」── 一語だけなら、ただの文字。
        const hit = KINDS.filter((k) => new RegExp('"' + k + '"\\s*:\\s*\\("').test(s));
        if (hit.length >= 3) found.push(name + '（' + hit.join('・') + '）');
    }
    return found;
}

const three = { 核: fromCore(), ウィンドウ: fromWindow() };
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

const own = phoneHasItsOwn();
if (own.length) {
    bad += 1;
    console.log('✗ iPhone が自分の見出しの表を持っています: ' + own.join(' / '));
    console.log('  表示の画面は核の to_html を出します ── 名前をもう一度書くと、');
    console.log('  その日から同じノートが端末によって違う言葉で出ます。');
}

console.log('');
if (bad) {
    console.log(KINDS.length + ' 件中 ' + bad + ' 件ずれています');
    process.exit(1);
}
console.log('同じものを、同じ名前で呼んでいます（' + KINDS.length + ' 件）');
