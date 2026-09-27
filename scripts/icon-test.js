#!/usr/bin/env node
/* 押せるものの絵が、記号の文字で書かれていないか（2026-09-27）。
 *
 *     node scripts/icon-test.js
 *
 * **記号文字は書体まかせ。** `⚙` も `☰` も `‹` も、太さ・大きさ・向きが
 * 環境の書体で変わる。隣に並ぶ手で描いた絵（鐘・矢印）と明らかに別人に
 * 見えるので、`gui/renderer.js` の上のほうに「矢印は文字ではなく線で描く」と
 * 書いてある ── **その決めごとから、設定と目次と送りと閉じるが漏れていた**
 * （crmaine が紹介動画を撮っていて見つけた）。
 *
 * 決めごとを覚えているあいだしか効かないので、ここで見張る。
 *
 * **見るのは「絵として置かれた記号」だけ。** 言葉の隣の `▾`（`ノート ▾`）や、
 * キーの表記（`⌘N`）や、一覧の行頭の印（`● `）は文字でよい ── あれは絵では
 * なく、読む字の一部。
 */
'use strict';
const fs = require('node:fs');
const path = require('node:path');

const ROOT = path.join(__dirname, '..');

/// 絵の代わりに使われがちな記号。**仮名・漢字・英数字は入れない。**
const MARKS = /^[ -⯿〈-】︐-﹯！-｠\u{1F300}-\u{1FAFF}\s]+$/u;

/// 直さないと決めたもの。**一つずつ理由を書く** ── 理由の書けない例外は、
/// 例外ではなく直し忘れ。
const KEEP = [
    {
        what: "box.textContent = done ? '☑' : '☐';",
        why: 'core（`markdown.rs`）が HTML に入れる字で、画面が置いたものではない。'
            + ' iPhone は同じ字を隠して CSS で描いている（`Paper.swift`）ので、'
            + '**直すなら core と両方** ── 本人に相談中（2026-09-27）。',
    },
    {
        what: "box.textContent = done ? '☐' : '☑';",
        why: '上と同じ一組（押した瞬間に裏返すほう）。',
    },
    {
        what: "pad.textContent = '└';",
        why: '押せるものではない ── 取り込みの一覧で、枝の深さを見せる飾り。',
    },
];

function rows(file, text) {
    const out = [];
    const lines = text.split('\n');
    if (file.endsWith('.html')) {
        // `<button …>ここ</button>` の「ここ」が記号だけなら、絵のつもり。
        const re = /<button\b[^>]*>([^<]*)<\/button>/g;
        lines.forEach((line, i) => {
            for (let m = re.exec(line); m; m = re.exec(line)) {
                const inner = m[1].trim();
                if (inner && MARKS.test(inner)) out.push([i + 1, inner, m[0].slice(0, 90)]);
            }
        });
        return out;
    }
    // `textContent = '✕'` / `textContent = 済 ? '☑' : '☐'` ── **右辺まるごとが
    // 記号のときだけ**。`名前 + ' ▾'` のように言葉に足しているものは、絵では
    // なく読む字の一部なので見ない（そこを見ると、ほぼ全部の行が鳴る）。
    const re = /(?:textContent|innerHTML)\s*=\s*([^;]+);/g;
    const only = (v) => {
        const t = v.trim();
        const m = /^'([^']*)'$/.exec(t);
        return !!m && !!m[1].trim() && MARKS.test(m[1].trim());
    };
    lines.forEach((line, i) => {
        for (let m = re.exec(line); m; m = re.exec(line)) {
            const rhs = m[1].trim();
            const tern = /^[^?]+\?([^:]+):(.+)$/.exec(rhs);
            const marks = tern ? [tern[1], tern[2]] : [rhs];
            if (!marks.every(only)) continue;
            const got = marks.map((x) => x.trim().slice(1, -1).trim()).join(' / ');
            out.push([i + 1, got, line.trim().slice(0, 90)]);
        }
    });
    return out;
}

function main() {
    const files = ['gui/index.html', 'gui/renderer.js'];
    const bad = [];
    for (const f of files) {
        const text = fs.readFileSync(path.join(ROOT, f), 'utf8');
        for (const [n, mark, line] of rows(f, text)) {
            if (KEEP.some((k) => line.includes(k.what))) continue;
            bad.push({ f, n, mark, line });
        }
    }
    if (bad.length) {
        console.error(`押せるものに記号の文字が ${bad.length} か所あります`
            + '（線で描いてください ── `GEAR_ICON` などの隣に）:\n');
        for (const b of bad) {
            console.error(`  ${b.f}:${b.n}  「${b.mark}」`);
            console.error(`      ${b.line}`);
        }
        process.exit(1);
    }
    console.log(`押せるものの絵は、ぜんぶ線で描いてあります`
        + `（${files.length} ファイル・直さないと決めたもの ${KEEP.length} 件）`);
}

main();
