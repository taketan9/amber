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
 * そのまま出している。**3 つ目の表が無いほうが強い約束**なので、
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
        // 3 つ以上そろって初めて「表」── 一語だけなら、ただの文字。
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

/* ── はじめの案内とようこそ画面の文（依頼 654・656・657） ──
 *
 * **二つの amber で同じ字。** 家族で両方を使うとき、同じボタンの説明が
 * 端末によって違うのは、いちばん質の悪いずれ方 ── どちらが正しいのか
 * 誰にも分からない。文は本人が書いたものなので、片方だけ直せば鳴る。
 */
const lines = (text, from, to) => {
    const a = text.indexOf(from);
    if (a < 0) return null;
    const b = text.indexOf(to, a);
    if (b < 0) return null;
    return text.slice(a, b);
};
/// 人に見せる字だけを、出てきた順に。
///
/// **`say:` の値だけ見る。** 引用符の中をぜんぶ拾うと、指す先の選択子
/// （`#rail .head[data-head="フォルダ"]`）まで数に入って、あるはずのない
/// ずれを毎回報せる（実際に一度そうなった）。
///
/// JS は `\n`、Swift も `\n` で書くので、そのまま並べて比べられる。
const quoted = (chunk) => [...chunk.matchAll(/say:\s*(?:'([^'\\]*(?:\\.[^'\\]*)*)'|"([^"\\]*(?:\\.[^"\\]*)*)")/g)]
    .map((m) => (m[1] === undefined ? m[2] : m[1]));

{
    const win = read('gui/renderer.js');
    const phone = read('ios/Cian/Tour.swift');
    const a = lines(win, 'const TOUR = [', '\n];');
    const b = lines(phone, 'static let steps: [Step] = [', '\n    ]');
    if (!a || !b) {
        bad += 1;
        console.log('✗ はじめの案内の文を切り出せません（' + (a ? 'iPhone' : 'ウィンドウ') + '）');
    } else {
        const x = quoted(a);
        const y = quoted(b);
        if (x.join('\u0000') !== y.join('\u0000')) {
            bad += 1;
            console.log('✗ はじめの案内の文が、ウィンドウと iPhone で違います');
            for (let i = 0; i < Math.max(x.length, y.length); i += 1) {
                if (x[i] !== y[i]) console.log('  ' + (i + 1) + ': 窓「' + (x[i] || '') + '」/ iPhone「' + (y[i] || '') + '」');
            }
        } else {
            console.log('✓ はじめの案内の文は、両方で同じ（' + x.length + ' 件）');
        }
    }
}

/// ようこそ画面は組みの並び（`['見出し', '添え']`）なので、引用符の中を
/// 順に拾う。ここに選択子は入らない。
const phrases = (chunk) => [...chunk.matchAll(/'([^'\\]*(?:\\.[^'\\]*)*)'|"([^"\\]*(?:\\.[^"\\]*)*)"/g)]
    .map((m) => (m[1] === undefined ? m[2] : m[1]));

{
    const win = read('gui/renderer.js');
    const phone = read('ios/Cian/Hello.swift');
    const a = lines(win, 'const HELLO_SELL = [', '\n];');
    const b = lines(phone, 'private static let sell: [(String, String)] = [', '\n    ]');
    if (!a || !b) {
        bad += 1;
        console.log('✗ ようこそ画面の文を切り出せません（' + (a ? 'iPhone' : 'ウィンドウ') + '）');
    } else {
        const x = phrases(a);
        const y = phrases(b);
        if (x.join('\u0000') !== y.join('\u0000')) {
            bad += 1;
            console.log('✗ ようこそ画面の文が、ウィンドウと iPhone で違います');
            for (let i = 0; i < Math.max(x.length, y.length); i += 1) {
                if (x[i] !== y[i]) console.log('  ' + (i + 1) + ': 窓「' + (x[i] || '') + '」/ iPhone「' + (y[i] || '') + '」');
            }
        } else {
            console.log('✓ ようこそ画面の文は、両方で同じ（' + x.length + ' 件）');
        }
    }
}

console.log('');
if (bad) {
    console.log(KINDS.length + ' 件中 ' + bad + ' 件ずれています');
    process.exit(1);
}
console.log('同じものを、同じ名前で呼んでいます（' + KINDS.length + ' 件）');
