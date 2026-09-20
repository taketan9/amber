#!/usr/bin/env node
/* 一覧の並び順が、押すたびに回るか（依頼 643）。
 *
 *     node scripts/order-test.js
 *
 * 本人が決めた: **同じボタンを押し続けて、3 つの物差し × 2 つの向きを回る。**
 * 6 回押せば元に戻り、行き止まりが無い。
 *
 * **絵で確かめない。** 6 とおりを撮って目で読むより、回り方そのものを
 * 機械に言わせるほうが確かで、次に誰かが並びを足したときにも効く。
 * デスクトップ版の `ORDERS` を `renderer.js` から切り出して、そのまま回す
 * （写しを持つと、写しだけが正しくなる）。
 *
 * iPhone 側は `NotesStore.Order` に同じ 6 つが同じ順で並んでいるか**だけ**を
 * 見る ── あちらの並べ替えは Swift なのでここからは回せないが、**並ぶ順が
 * ずれていないこと**は字で確かめられる。
 */
'use strict';
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
let bad = 0;
const ok = (cond, what) => {
    if (cond) return;
    bad++;
    console.error('  ✗ ' + what);
};

/// デスクトップ版から `ORDERS` を切り出す。**写さずに読む。**
function ordersFromRenderer() {
    const src = fs.readFileSync(path.join(root, 'gui', 'renderer.js'), 'utf8');
    const a = src.indexOf('const ORDERS = [');
    const b = src.indexOf('];', a);
    if (a < 0 || b < 0) throw new Error('renderer.js から ORDERS を切り出せません');
    // eslint-disable-next-line no-new-func
    return new Function(src.slice(a, b + 2) + '\nreturn ORDERS;')();
}

const ORDERS = ordersFromRenderer();

console.log('並び順 ── 押すたびに回るか');

// 一。6 つある。3 つの物差し × 2 つの向き。
ok(ORDERS.length === 6, `6 とおりのはずが ${ORDERS.length} とおり`);
for (const key of ['updated', 'created', 'title']) {
    const mine = ORDERS.filter(([k]) => k === key);
    ok(mine.length === 2, `${key} が 2 とおりではない（${mine.length}）`);
    ok(mine.some(([, a]) => a === true) && mine.some(([, a]) => a === false),
        `${key} に昇順と降順が揃っていない`);
}

// 二。**同じものが二度出てこない。** 出ると、押しても戻ってこない場所ができる。
const seen = new Set(ORDERS.map(([k, a]) => k + ':' + a));
ok(seen.size === ORDERS.length, '同じ並びが二度出てきます');

// 三。**押し続ければ元に戻る。** 6 回で一周、途中では戻らない。
{
    let at = 0;
    const path0 = [];
    for (let i = 0; i < ORDERS.length; i++) {
        path0.push(ORDERS[at][2]);
        at = (at + 1) % ORDERS.length;
    }
    ok(at === 0, '6 回押しても元に戻りません');
    ok(new Set(path0).size === ORDERS.length, '一周のあいだに同じ札が二度出ます');
}

// 四。**札は向きを言う。** 矢印の無い札は、押した人に何も伝えない。
for (const [key, asc, label] of ORDERS) {
    ok(/[↑↓]/.test(label), `「${label}」に向きの矢印がありません`);
    // 上に来るのが小さいほうなら ↑、大きいほうなら ↓。
    //
    // **タイトル順だけ、既定の向きが逆。** あいうえお順（昇順）が人の言う
    // 「ふつう」で、日付は新しい順（降順）が「ふつう」── どちらも ↑↓ の
    // 意味は同じで、既定にどちらを選んだかが違うだけ。
    ok(label.includes(asc ? '↑' : '↓'), `「${label}」の矢印が向きと合っていません`);
}

// 五。**既定は更新順の新しい順**（いちばん上に、最後に書いていたもの）。
ok(ORDERS[0][0] === 'updated' && ORDERS[0][1] === false,
    `最初の並びが「更新順 ↓」ではありません（${ORDERS[0][2]}）`);

// 六。**iPhone に同じ 6 つが、同じ順で並んでいる。**
{
    const swift = fs.readFileSync(path.join(root, 'ios', 'Cian', 'NotesStore.swift'), 'utf8');
    const m = /enum Order: String, CaseIterable, Identifiable \{\s*\n\s*case ([^\n]+)/.exec(swift);
    ok(!!m, 'NotesStore.swift から Order の並びを読めません');
    if (m) {
        const cases = m[1].split(',').map((s) => s.trim());
        ok(cases.length === ORDERS.length,
            `iPhone は ${cases.length} とおり、デスクトップ版は ${ORDERS.length} とおり`);
        // 名前の付け方は言語ごとに違ってよいが、**物差しの並ぶ順**は同じでなければ
        // ならない ── 違うと「3 回押したら何になるか」が端末で変わる。
        const mine = ORDERS.map(([k, a]) => (k === 'title' ? (a ? 'titleAsc' : 'titleDesc')
            : (a ? k + 'Asc' : k)));
        ok(JSON.stringify(cases) === JSON.stringify(mine),
            `iPhone の並ぶ順が違います\n      iPhone: ${cases.join(' → ')}\n      こちら: ${mine.join(' → ')}`);
    }
}

if (bad) {
    console.error(`\n${bad} 件ちがいます`);
    process.exit(1);
}
console.log(`ぜんぶ通りました（${ORDERS.length} とおり・押し続ければ元に戻ります）`);
