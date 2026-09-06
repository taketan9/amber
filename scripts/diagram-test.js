#!/usr/bin/env node
/* 図の工房が、図を失わないか。
 *
 * **表で直せる、とは「字に戻せる」ということ。** 読み戻して組み直した字が
 * 元と一字でも違えば、その差は保存の瞬間にノートへ入る ── 触っていない
 * ところが勝手に書き換わる。だから `cmdDiagram` の出す八つ全部について、
 * 「読んで、組み直して、元と同じ」を見る。
 *
 * もう一つは逆側。**読めない形を、読めたことにしない。** 枝の枝や区切りの
 * 二つある予定表を平らに読むと、書き戻したときに黙って形が変わる。そういう
 * ものは `null` を返し、字で直す面に落ちるのが正しい。
 *
 *     node scripts/diagram-test.js
 */
'use strict';
const fs = require('fs');
const path = require('path');

// renderer.js は窓の中でしか動かない（`el()` も `document` も要る）ので、
// **図の読み書きのところだけを切り出して**動かす。ここは DOM に触らない。
const file = path.join(__dirname, '..', 'gui', 'renderer.js');
const src = fs.readFileSync(file, 'utf8');
const from = src.indexOf('function mmdKind(');
const to = src.indexOf('const FLOW_SHAPE');
if (from < 0 || to < 0 || to < from) {
    console.error('gui/renderer.js から図の読み書きを切り出せません'
        + '（`mmdKind` から `FLOW_SHAPE` までの並びが変わりました）');
    process.exit(2);
}
// **間接呼びの `eval`。** そのまま `eval(...)` と書くと、切り出した関数が
// この一枚の中に閉じてしまい（このファイルは strict）、下から名前で呼べない。
(0, eval)(src.slice(from, to));

let bad = 0;
const ok = (yes, what) => {
    console.log((yes ? '  ✓ ' : '  ✗ ') + what);
    if (!yes) bad++;
};

// `cmdDiagram` が出す八つ。ここを直したら、あちらも直っているか見ること。
const MADE = {
    '流れ図': 'flowchart LR\n  A[書く]\n  B[見直す]\n  C[出す]\n  A --> B\n  B --> C',
    '分かれ道': 'flowchart LR\n  A{足りている？}\n  B[出す]\n  C[足す]\n'
        + '  A -->|はい| B\n  A -->|いいえ| C',
    'マインドマップ': 'mindmap\n  root((来年やること))\n    仕事\n      資格をとる\n    家\n    体\n    学び',
    '年表': 'timeline\n  title 今年\n  4月 : 引っ越し\n  7月 : 新しい仕事\n  11月 : 旅行',
    '予定表': 'gantt\n  title 段取り\n  dateFormat YYYY-MM-DD\n  axisFormat %m/%d\n'
        + '  section やること\n  下ごしらえ :t0, 2026-09-10, 3d\n  本番 :t1, 2026-09-13, 5d',
    '四象限': 'quadrantChart\n  title やることの置きどころ\n'
        + '  x-axis いつでも --> いま\n  y-axis 小さい --> 大きい\n'
        + '  quadrant-1 すぐやる\n  quadrant-2 段取りする\n'
        + '  quadrant-3 あとで\n  quadrant-4 誰かに頼む\n'
        + '  "週報": [0.9, 0.8]\n  "片付け": [0.2, 0.3]',
    'やりとり': 'sequenceDiagram\n  participant 私\n  participant 相手\n'
        + '  私->>相手: お願いする\n  相手-->>私: わかった',
    '円グラフ': 'pie showData\n  "仕事" : 5\n  "家" : 3\n  "ほか" : 2',
};

console.log('作った図が、そのまま戻るか');
for (const [name, want] of Object.entries(MADE)) {
    const d = mmdParse(want);
    if (!d) { ok(false, name + ' ── 読めない'); continue; }
    const got = mmdBuild(d);
    ok(got === want, name);
    if (got !== want) console.log('--- 元 ---\n' + want + '\n--- 戻 ---\n' + got);
    // 二度目も同じか。**一度目だけ合うのは、合っていない** ── 工房は
    // 開いて閉じてまた開かれる。
    if (d) ok(mmdBuild(mmdParse(got) || d) === got, name + '（二度目）');
}

console.log('読めない形を、読めたことにしないか');
for (const [why, s] of [
    ['形の付いた枝', 'mindmap\n  root((A))\n    B[四角]'],
    ['区切りが二つある予定表', 'gantt\n  section 一\n  a :t0, 2026-01-01, 1d\n'
        + '  section 二\n  b :t1, 2026-01-02, 1d'],
    ['囲みのある流れ図', 'flowchart LR\n  subgraph S\n  A[x]\n  end'],
    ['点線の流れ図', 'flowchart LR\n  A[x]\n  B[y]\n  A -.-> B'],
    ['amber の知らない図', 'classDiagram\n  A <|-- B'],
    ['図ですらないもの', 'これはただの字'],
]) ok(mmdParse(s) === null, why);

console.log('枝の枝を、深さのまま持って帰るか');
{
    const want = 'mindmap\n  root((来年))\n    仕事\n      資格\n      引き継ぎ\n    家\n      片付け';
    const d = mmdParse(want);
    ok(!!d, '枝の枝を読める');
    ok(d && d.rows.map((r) => r.at).join(',') === '0,1,1,0,1', '深さを取り違えない');
    ok(mmdBuild(d) === want, '同じ字に戻る');

    // 空白が四つでも二つでも「一段下」は一段下。
    const wide = mmdParse('mindmap\n  root((A))\n        B\n                C');
    ok(wide && wide.rows.map((r) => r.at).join(',') === '0,1', '字下げの幅に頼らない');

    // 親のいない孫は、詰めて親のある形に ── そのまま持つと、書き戻した
    // ときに mermaid がその枝を捨てる。
    const jump = mmdParse('mindmap\n  root((A))\n          B\n    C');
    ok(jump && jump.rows.map((r) => r.at).join(',') === '0,0', '一段飛ばしを詰める');
}

console.log('消した箱を指す線を、残さないか');
{
    const d = mmdParse('flowchart LR\n  A[一]\n  B[二]\n  A --> B');
    d.rows.splice(1, 1);                       // 「二」を消す（線はそのまま）
    ok(!/-->/.test(mmdBuild(d)), '箱を消したら、その線も出さない');
}

console.log('打ち間違いで図を壊さないか');
{
    const d = mmdParse('quadrantChart\n  x-axis a --> b\n  "x": [0.5, 0.5]');
    d.rows[0].b = '9';                          // 0〜1 の外
    d.rows[0].c = 'あ';                         // 数ですらない
    const got = mmdBuild(d);
    ok(/\[0\.5, 1\]/.test(got), '0〜1 に収める（' + got.split('\n').pop().trim() + '）');
    const f = mmdParse('flowchart LR\n  A[一]\n  B[二]\n  A --> B');
    f.rows[0].a = '一[二]|三';                  // 形を壊す字
    ok(/A\[一二三\]/.test(mmdBuild(f)), '括弧と縦棒を落とす');
}

console.log('箱ごとの色');
{
    const painted = 'flowchart LR\n  A[一]\n  B[二]\n  A --> B\n'
        + '  style A fill:#F2DAD8,stroke:#C4564E,color:#3a2408';
    const d = mmdParse(painted);
    ok(!!d, '色の付いた図も、表になる');
    ok(d.rows[0].color === '#C4564E', '色は箱に付いて読み戻る', d.rows[0].color);
    ok(d.rows[1].color === undefined, '色の無い箱には付かない');
    // **往復して同じ字に戻る。** ここがずれると、開いて閉じただけで
    // ノートが同期先の差分になる。
    ok(mmdBuild(d).trim() === painted.trim(), '往復しても同じ字', mmdBuild(d));

    // 色を外したら、style の行ごと消える ── 色なしの style が残ると、
    // mermaid はそれを「透明に塗れ」と読む。
    d.rows[0].color = undefined;
    ok(!/style/.test(mmdBuild(d)), '色を外すと、行ごと消える', mmdBuild(d));

    // 箱を消したら、その箱の色も出さない（線と同じ扱い）。
    const e = mmdParse(painted);
    e.rows.splice(0, 1);
    ok(!/style A/.test(mmdBuild(e)), '消した箱の色は残さない', mmdBuild(e));

    // **こちらが書いた形だけ読む。** 手で書いた凝った `style` を色だけの
    // 表に押し込むと、書き戻したときに残りが消える。
    ok(mmdParse('flowchart LR\n  A[一]\n  style A fill:red,stroke-width:4px') === null,
       '知らない形の style は、表にしない');

    // 淡くする計算そのもの。
    ok(soften('#C4564E', 0.22) === '#F2DAD8', '白と混ぜて淡くする', soften('#C4564E', 0.22));
    ok(soften('#000000', 0) === '#FFFFFF', '混ぜきると白');
}

console.log(bad ? '\n' + bad + ' 件ちがいます' : '\nぜんぶ通りました');
process.exit(bad ? 1 : 0);
