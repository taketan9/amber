#!/usr/bin/env node
/*
 * **持ち込む一式をまとめる**（依頼 550）── ネットに出られない会社の Windows で
 * amber を組むために、家で用意しておくもの。
 *
 *     node scripts/offline-kit.js --out dist [--zip]
 *
 * 出来上がり:
 *
 *     dist/amber-kit-2.13.0/
 *       組み方.txt                          ← そのまま打てる一行が書いてある
 *       amber/                              ← いまの HEAD の中身（git archive）
 *         gui/vendor/                       ← **git に入っていないので、ここで足す**
 *       electron-v33.4.11-win32-x64/        ← Electron の一式
 *       amber-server-win-x64.exe            ← エンジン（CRT ごと静的・置くだけで動く）
 *       rcedit-x64.exe                      ← exe の画像と名前を焼く道具（任意）
 *
 * **ここは網に出る。** 出られない側でやることを、出られる側に寄せるための
 * 道具なので、足りないものは黙って飛ばさず、**どこから取るかを言って止まる**
 * ── 半端な一式を持ち込んで、会社で気づくのがいちばん高くつく。
 */
const fs = require('node:fs');
const path = require('node:path');
const { execFileSync } = require('node:child_process');
const { zipDir } = require('./zip');

const ROOT = path.join(__dirname, '..');
const arg = (name, fallback = null) => {
    const i = process.argv.indexOf('--' + name);
    if (i < 0) return fallback;
    const v = process.argv[i + 1];
    return !v || v.startsWith('--') ? true : v;
};
const has = (name) => process.argv.includes('--' + name);

const version = (fs.readFileSync(path.join(ROOT, 'Cargo.toml'), 'utf8')
    .match(/^version = "(.+?)"/m) || [, '0.0.0'])[1];

function copy(from, to) {
    const st = fs.lstatSync(from);
    if (st.isSymbolicLink()) return;
    if (st.isDirectory()) {
        fs.mkdirSync(to, { recursive: true });
        for (const n of fs.readdirSync(from)) copy(path.join(from, n), path.join(to, n));
    } else {
        fs.mkdirSync(path.dirname(to), { recursive: true });
        fs.copyFileSync(from, to);
        fs.chmodSync(to, st.mode);
    }
}

function stop(what, how) {
    console.error('NG: ' + what);
    console.error('  ' + how);
    process.exit(1);
}

const out = path.resolve(arg('out') || path.join(ROOT, 'dist'));
const kit = path.join(out, 'amber-kit-' + version);
fs.rmSync(kit, { recursive: true, force: true });
fs.mkdirSync(kit, { recursive: true });

// ── 一. amber そのもの ──────────────────────────────────────────
// **`git archive` で出す** ── 手元の `target/` や `node_modules/` を
// 巻き込まない。追っていないものは持っていかない、が既定として正しい。
const src = path.join(kit, 'amber');
fs.mkdirSync(src, { recursive: true });
execFileSync('sh', ['-c',
    `git -C '${ROOT}' archive --format=tar HEAD | tar -x -C '${src}'`]);
const dirty = execFileSync('git', ['-C', ROOT, 'status', '--porcelain'], { encoding: 'utf8' }).trim();
if (dirty) {
    // **黙って古いものを詰めない。** `git archive` が見るのは HEAD なので、
    // 直したばかりのものは入らない ── 会社で「直したはずなのに直っていない」
    // になる。止めはしないが、必ず言う。
    console.log('注意: コミットしていない変更があります。入るのは HEAD の中身です:');
    for (const l of dirty.split('\n').slice(0, 8)) console.log('  ' + l);
}

// ── 二. gui/vendor（git に入っていない） ─────────────────────────
const vendor = path.join(ROOT, 'gui', 'vendor');
if (!fs.existsSync(path.join(vendor, 'monaco', 'vs', 'loader.js'))) {
    stop('gui/vendor/ がありません（エディタと図の実体）。',
         'cd gui && npm install && node vendor.js');
}
copy(vendor, path.join(src, 'gui', 'vendor'));

// ── 三. Electron の win32 一式 ──────────────────────────────────
let electron = arg('electron');
if (!electron) {
    const beside = fs.readdirSync(path.join(ROOT, '..'))
        .filter((n) => /^electron-v.*win32-x64$/.test(n)).sort().pop();
    if (beside) electron = path.join(ROOT, '..', beside);
}
if (!electron || !fs.existsSync(path.join(electron, 'electron.exe'))) {
    stop('Windows の Electron がありません。',
         'https://github.com/electron/electron/releases から electron-v33.4.11-win32-x64.zip を落として展開し、--electron で指してください');
}
const electronName = path.basename(electron);
copy(electron, path.join(kit, electronName));

// ── 四. エンジン ────────────────────────────────────────────────
// **CRT ごと静的に組んである一枚**（release.yml の註）── 置くだけで動く。
let engine = arg('engine') || arg('server');
if (!engine) {
    const at = path.join(out, 'amber-server-win-x64.exe');
    if (fs.existsSync(at)) engine = at;
}
if (!engine || !fs.existsSync(engine)) {
    stop('Windows のエンジン（amber-server-win-x64.exe）がありません。',
         'gh release download --pattern amber-server-win-x64.exe --dir ' + out + '  （または --engine で指す）');
}
copy(engine, path.join(kit, 'amber-server-win-x64.exe'));

// ── 五. rcedit（任意） ──────────────────────────────────────────
const rcedit = arg('rcedit');
let rceditName = null;
if (rcedit && rcedit !== true && fs.existsSync(rcedit)) {
    rceditName = path.basename(rcedit);
    copy(rcedit, path.join(kit, rceditName));
}

// ── 六. 組み方 ──────────────────────────────────────────────────
// **そのまま打てる一行にする。** 註を行の中に混ぜると、貼り付けたときに
// そこで壊れる ── 註は下に置く（実際に混ぜてしまい、打てない字になった）。
const line = ['node scripts\\pack.js --out dist --platform win32',
    '  --electron ..\\' + electronName,
    '  --engine ..\\amber-server-win-x64.exe',
    ...(rceditName ? ['  --rcedit ..\\' + rceditName] : []),
    '  --zip'].join(' ^\r\n');
fs.writeFileSync(path.join(kit, '組み方.txt'), [
    'ambər ' + version + ' を、ネットに出られない Windows で組む',
    '',
    'いるもの: この一式と、Node.js（node --version が通ること）だけ。',
    '網には一度も出ません。',
    '',
    '1. この一式を丸ごと、その機械のどこかに置く（例: C:\\amber-kit）',
    '2. コマンドプロンプトで amber のフォルダに入る',
    '',
    '   cd C:\\amber-kit\\amber',
    '',
    '3. 組む',
    '',
    line,
    '',
    '   dist\\amber-win-x64\\amber.exe ができます。',
    '   --zip を付けると dist\\amber-win-x64-' + version + '.zip も出ます。',
    '',
    ...(rceditName ? [] : [
        '（exe の画像と名前は Electron のままです。焼くなら rcedit-x64.exe を',
        '  この一式に入れて、--rcedit ..\\rcedit-x64.exe を足してください）',
        '',
    ]),
    '困ったら:',
    '  「gui/vendor/ がありません」 … この一式の amber\\gui\\vendor が',
    '                                 欠けています。持ち込み直してください。',
    '  「Windows のエンジンがありません」 … --engine の道が違います。',
    '',
].join('\r\n'));

const mb = (dir) => {
    let n = 0;
    const walk = (at) => {
        const st = fs.lstatSync(at);
        if (st.isSymbolicLink()) return;
        if (st.isDirectory()) for (const c of fs.readdirSync(at)) walk(path.join(at, c));
        else n += st.size;
    };
    walk(dir);
    return (n / 1024 / 1024).toFixed(1) + ' MB';
};
console.log('できました: ' + kit + '  (' + mb(kit) + ')');
console.log('  amber/                    いまの HEAD ＋ gui/vendor');
console.log('  ' + electronName);
console.log('  amber-server-win-x64.exe');
if (rceditName) console.log('  ' + rceditName);
console.log('  組み方.txt');
if (has('zip')) {
    const zip = kit + '.zip';
    fs.rmSync(zip, { force: true });
    const n = zipDir(kit, zip);
    console.log('zip: ' + zip + '  (' + mb(path.dirname(zip)) + ' のうち ' + n + ' 件)');
}
